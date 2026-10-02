#!/usr/bin/env python3
"""Run PrimerSwitch native storage fixtures in an isolated Linux keyring.

No inherited user bus, auth home, CLI credentials or provider traffic is used.
Daemon stdout/stderr and fixture output are intentionally never logged. The
only password is a public, static fixture value delivered on the daemon stdin.

References:
https://dbus.freedesktop.org/doc/dbus-daemon.1.html
https://wiki.gnome.org/Projects/GnomeKeyring/RunningDaemon
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET

PROJECT = Path(__file__).resolve().parents[1]
REPORT = PROJECT / ".artifacts" / "linux-native-storage-report.json"
MARKER = b"PrimerSwitch isolated keyring test v1\n"
# Public test-only input. It is never an argument, environment value or output.
FIXTURE_PASSWORD = b"PrimerSwitch isolated native fixture password v1"
BUS_CONFIG = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow own="*"/>
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
  </policy>
  <limit name="service_start_timeout">5000</limit>
</busconfig>
"""
TOOLS = ("cargo", "rustc", "dbus-run-session", "dbus-daemon", "gdbus", "gnome-keyring-daemon")
CASES = [
    "encrypted_master_key_create",
    "master_key_survives_daemon_and_process_restart",
    "ciphertext_reopen",
    "duplicate_item_rejected",
    "malformed_item_rejected",
    "wrong_content_type_rejected",
    "replaced_key_rejected",
    "session_default_alias_rejected",
    "ephemeral_key_not_adopted",
    "lost_key_preserves_ciphertext",
    "locked_collection_rejected_without_creation",
    "missing_service_preserves_ciphertext",
]


class FixtureFailure(Exception):
    """A deliberately redacted error carrying only a static operation name."""


def checked_run(command: list[str], *, env: dict[str, str], step: str,
                timeout: int = 300) -> subprocess.CompletedProcess[bytes]:
    try:
        result = subprocess.run(command, cwd=PROJECT, env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise FixtureFailure(step) from error
    if result.returncode:
        raise FixtureFailure(step)
    return result


def stop_owned(process: subprocess.Popen[bytes], *, group: bool = False) -> None:
    if process.poll() is not None:
        return
    try:
        if group:
            os.killpg(process.pid, signal.SIGTERM)
        else:
            process.terminate()
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        if group:
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        process.wait(timeout=10)
    except ProcessLookupError:
        process.wait(timeout=10)


def write_report(report: dict[str, object]) -> None:
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    # Report fields contain only static case names, versions, OS and status.
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=REPORT.parent,
                                     prefix=".linux-storage-", delete=False) as output:
        temporary = Path(output.name)
        json.dump(report, output, indent=2, sort_keys=True)
        output.write("\n")
    try:
        os.replace(temporary, REPORT)
    finally:
        temporary.unlink(missing_ok=True)


def child_environment(root: Path) -> dict[str, str]:
    # Do not inherit authentication, desktop, D-Bus or provider environment.
    # The explicit Cargo/Rustup locations retain only the build-tool context.
    original_home = Path.home()
    env = {
        "PATH": os.environ.get("PATH", os.defpath),
        "CARGO_HOME": os.environ.get("CARGO_HOME", str(original_home / ".cargo")),
        "RUSTUP_HOME": os.environ.get("RUSTUP_HOME", str(original_home / ".rustup")),
        "CARGO_NET_OFFLINE": "true",
        "CARGO_TERM_COLOR": "never",
        "HOME": str(root / "home"),
        "XDG_CONFIG_HOME": str(root / "config"),
        "XDG_DATA_HOME": str(root / "data"),
        "XDG_CACHE_HOME": str(root / "cache"),
        "XDG_RUNTIME_DIR": str(root / "runtime"),
        "TMPDIR": "/tmp",
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "PRIMERSWITCH_ISOLATED_KEYRING_TEST": "1",
        "PRIMERSWITCH_ISOLATED_HOME": str(root / "home"),
    }
    for name in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"):
        directory = Path(env[name])
        directory.mkdir(mode=0o700)
        directory.chmod(0o700)
    (Path(env["HOME"]) / ".primerswitch-isolated-keyring").write_bytes(MARKER)
    return env


def safe_versions(env: dict[str, str]) -> dict[str, str]:
    versions = {}
    for name in TOOLS:
        try:
            result = checked_run([name, "--version"], env=env, step="tool_version", timeout=10)
            # gdbus lacks --version on some GLib builds; absence is explicit.
            match = re.search(rb"\b\d+\.\d+(?:\.\d+)?\b", result.stdout + result.stderr)
            versions[name] = match.group(0).decode("ascii") if match else "not_reported"
        except FixtureFailure:
            versions[name] = "not_reported"
    return versions


def bus_call(env: dict[str, str], destination: str, path: str,
             method: str, *arguments: str) -> bytes:
    return checked_run(["gdbus", "call", "--session", "--dest", destination,
                        "--object-path", path, "--method", method, *arguments],
                       env=env, step="isolated_bus_call", timeout=5).stdout.strip()


def service_owned(env: dict[str, str]) -> bool:
    value = bus_call(env, "org.freedesktop.DBus", "/org/freedesktop/DBus",
                     "org.freedesktop.DBus.NameHasOwner", "org.freedesktop.secrets")
    if value not in (b"(true,)", b"(false,)"):
        raise FixtureFailure("unexpected_bus_owner_response")
    return value == b"(true,)"


def wait_service(env: dict[str, str], expected: bool,
                 daemon: subprocess.Popen[bytes] | None = None) -> None:
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if daemon is not None and daemon.poll() is not None:
            raise FixtureFailure("owned_keyring_daemon_exited")
        if service_owned(env) == expected:
            return
        time.sleep(0.2)
    raise FixtureFailure("isolated_service_readiness_timeout")


def start_daemon(env: dict[str, str]) -> subprocess.Popen[bytes]:
    control = Path(env["XDG_RUNTIME_DIR"]) / "keyring"
    control.mkdir(mode=0o700, exist_ok=True)
    control.chmod(0o700)
    try:
        daemon = subprocess.Popen([
            "gnome-keyring-daemon", "--foreground", "--unlock", "--components=secrets",
            "--control-directory", str(control),
        ], cwd=PROJECT, env=env, stdin=subprocess.PIPE,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except OSError as error:
        raise FixtureFailure("owned_keyring_daemon_start") from error
    try:
        assert daemon.stdin is not None
        daemon.stdin.write(FIXTURE_PASSWORD)
        daemon.stdin.close()
        wait_service(env, True, daemon)
        alias = bus_call(env, "org.freedesktop.secrets", "/org/freedesktop/secrets",
                         "org.freedesktop.Secret.Service.ReadAlias", "default")
        # GNOME --unlock creates/unlocks its on-disk login collection. No
        # transient session collection or arbitrary default is accepted.
        if alias != b"(objectpath '/org/freedesktop/secrets/collection/login',)":
            raise FixtureFailure("persistent_default_login_collection_required")
        return daemon
    except BaseException:
        stop_owned(daemon)
        raise


def native_test(env: dict[str, str], name: str) -> None:
    print("Running isolated fixture: " + name, flush=True)
    checked_run(["cargo", "test", "-p", "switcher-platform", "--locked", "--offline", name,
                 "--", "--ignored", "--test-threads=1"], env=env, step=name, timeout=420)


def inside(mode: str) -> int:
    if sys.platform != "linux" or os.environ.get("PRIMERSWITCH_ISOLATED_KEYRING_TEST") != "1":
        raise FixtureFailure("isolated_linux_context_required")
    home = Path(os.environ["HOME"]).resolve()
    expected = Path(os.environ["PRIMERSWITCH_ISOLATED_HOME"]).resolve()
    if home != expected or home.parent.parent != Path("/tmp") or not home.parent.name.startswith("primerswitch-native-"):
        raise FixtureFailure("temporary_home_required")
    if (home / ".primerswitch-isolated-keyring").read_bytes() != MARKER:
        raise FixtureFailure("temporary_home_marker_required")
    address = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
    if not address.startswith("unix:path="):
        raise FixtureFailure("fresh_session_bus_required")
    env = dict(os.environ)
    env["PRIMERSWITCH_TEST_DBUS_ADDRESS"] = address
    if mode == "missing":
        if service_owned(env):
            raise FixtureFailure("missing_service_bus_was_not_empty")
        env["PRIMERSWITCH_NATIVE_EXPECT_SERVICE"] = "missing"
        native_test(env, "linux_secret_service_missing_native")
        if service_owned(env):
            raise FixtureFailure("missing_service_was_activated")
        return 0
    daemon = start_daemon(env)
    try:
        env["PRIMERSWITCH_NATIVE_VAULT_DIR"] = str(home / "fixturepersistent")
        env["PRIMERSWITCH_NATIVE_PHASE"] = "create"
        native_test(env, "linux_secret_service_process_native")
        stop_owned(daemon)
        wait_service(env, False)
        daemon = start_daemon(env)
        env["PRIMERSWITCH_NATIVE_PHASE"] = "reopen"
        native_test(env, "linux_secret_service_process_native")
        env.pop("PRIMERSWITCH_NATIVE_PHASE")
        env.pop("PRIMERSWITCH_NATIVE_VAULT_DIR")
        native_test(env, "linux_secret_service_session_alias_native")
        native_test(env, "linux_secret_service_native")
    finally:
        stop_owned(daemon)
    return 0


def run_session(mode: str, config: Path, env: dict[str, str]) -> None:
    # The wrapper owns an entirely fresh process group; timeout cleanup cannot
    # target a user's pre-existing bus or daemon. The bus config has no service
    # activation directories, including for the positive service fixture.
    command = ["dbus-run-session", "--config-file=" + str(config), "--",
               sys.executable, "-B", str(Path(__file__).resolve()), "--inside", mode]
    try:
        process = subprocess.Popen(command, cwd=PROJECT, env=env, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=True)
    except OSError as error:
        raise FixtureFailure("isolated_bus_start") from error
    try:
        stdout, stderr = process.communicate(timeout=1000)
        if process.returncode:
            # Only a strictly static inner failure token may reach the report.
            match = re.search(rb"^Isolated fixture failed: ([a-z_]+)$", stderr, re.MULTILINE)
            step = match.group(1).decode("ascii") if match else "isolated_" + mode + "_session"
            raise FixtureFailure(step)
        # Internal output is constrained to fixture names. Rebuild from static
        # metadata instead of forwarding even trusted child output.
        del stdout
    except subprocess.TimeoutExpired as error:
        raise FixtureFailure("isolated_" + mode + "_timeout") from error
    finally:
        stop_owned(process, group=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--validate", action="store_true", help="validate runner configuration without starting services")
    parser.add_argument("--inside", choices=("service", "missing"), help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.validate:
        tree = ET.fromstring(BUS_CONFIG)
        assert tree.tag == "busconfig"
        assert not tree.findall("servicedir") and not tree.findall("standard_session_servicedirs")
        print("Isolated Linux keyring runner configuration is valid; no services started.")
        return 0
    if args.inside:
        try:
            return inside(args.inside)
        except (FixtureFailure, OSError, KeyError, AssertionError) as error:
            print("Isolated fixture failed: " + (str(error) if isinstance(error, FixtureFailure) else "fixture_context"), file=sys.stderr)
            return 1
    if sys.platform != "linux":
        parser.error("native execution requires Linux; --validate and --help are safe on other platforms")
    report: dict[str, object] = {
        "schemaVersion": 1,
        "status": "failed",
        "platform": {"system": platform.system(), "release": platform.release(), "architecture": platform.machine()},
        "cases": [],
        "toolVersions": {},
        "credentialContext": "temporary-home-and-private-dbus",
        "providerTraffic": False,
    }
    try:
        for name in TOOLS:
            if not shutil.which(name):
                raise FixtureFailure("required_tool_missing_" + name)
        with tempfile.TemporaryDirectory(prefix="primerswitch-native-", dir="/tmp") as temporary:
            root = Path(temporary).resolve()
            root.chmod(0o700)
            env = child_environment(root)
            report["toolVersions"] = safe_versions(env)
            config = root / "private-session.conf"
            config.write_text(BUS_CONFIG, encoding="utf-8")
            config.chmod(0o600)
            print("Running isolated native storage cases with daemon restart.", flush=True)
            run_session("service", config, env)
            # A new bus has no owner or activation directories, even though the
            # host distribution has Secret Service activation installed.
            print("Running isolated missing-service case.", flush=True)
            run_session("missing", config, env)
        report["status"] = "passed"
        report["cases"] = [{"name": name, "status": "passed"} for name in CASES]
        write_report(report)
        print("Linux native storage fixtures passed; redacted report written.")
        return 0
    except (FixtureFailure, OSError, AssertionError) as error:
        report["failureStep"] = str(error) if isinstance(error, FixtureFailure) else "fixture_runner"
        write_report(report)
        print("Linux native storage fixtures failed: " + str(report["failureStep"]), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
