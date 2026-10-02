#!/usr/bin/env python3
"""Install and inspect the native demo only on an ephemeral Ubuntu 24.04 GitHub runner.

No real homes, account files, credential stores, authenticated endpoints or
release instrumentation are used. The distro Python worker inspects the actual
installed WebKitGTK renderer through AT-SPI. RPM qualification is separate.
"""
from __future__ import annotations
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
ARTIFACTS = ROOT / ".artifacts"
REPORT = ARTIFACTS / "linux-installed-demo-report.json"
SCREENSHOT = ARTIFACTS / "linux-installed-demo.png"
MARKER = "PrimerSwitch isolated Linux installed demo v1"


class QualificationError(Exception):
    pass


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise QualificationError(reason)


def sha256(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def validate_ci_host(system: str, machine: str, environment: dict, release_text: str) -> None:
    release = dict(line.split("=", 1) for line in release_text.splitlines() if "=" in line)
    require(system == "Linux" and machine in {"x86_64", "AMD64"}, "requires-native-linux-x86_64")
    require(environment.get("GITHUB_ACTIONS") == "true" and environment.get("RUNNER_ENVIRONMENT") == "github-hosted", "requires-ephemeral-github-hosted-runner")
    require(release.get("ID", "").strip('"') == "ubuntu" and release.get("VERSION_ID", "").strip('"') == "24.04", "requires-ubuntu-24.04")


def output(arguments: list[str], *, timeout: int = 30, environment: dict | None = None) -> bytes:
    return subprocess.run(arguments, cwd=ROOT, env=environment, check=True, capture_output=True, timeout=timeout).stdout


def write_report(report: dict) -> None:
    ARTIFACTS.mkdir(exist_ok=True)
    REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf8")


def verify_package_manifest(manifest: dict, root: Path) -> Path:
    require(manifest.get("unsignedPreview") is True and manifest.get("bundle") == "deb" and manifest.get("target") == "x86_64-unknown-linux-gnu", "requires-native-deb-manifest")
    records = manifest.get("packages")
    require(isinstance(records, list) and len(records) == 1, "ambiguous-preview-package")
    record = records[0]
    relative = Path(record["path"])
    path = root / relative
    expected_directory = root / "target/release/bundle/deb"
    require(not relative.is_absolute() and not path.is_symlink() and path.resolve().parent == expected_directory.resolve() and path.suffix == ".deb", "unsafe-preview-package-path")
    require(path.is_file() and path.stat().st_size == record["bytes"] and sha256(path.read_bytes()) == record["sha256"], "preview-package-hash-mismatch")
    for field, base in (("evidenceSha256", root / ".artifacts"), ("packagingInputSha256", root)):
        entries = manifest.get(field)
        require(isinstance(entries, dict) and bool(entries), "missing-preview-input-evidence")
        for name, digest in entries.items():
            source = base / name
            require(not source.is_symlink() and source.resolve().is_relative_to(base.resolve()) and source.is_file(), "unsafe-preview-input-evidence")
            require(sha256(source.read_bytes()) == digest, "preview-input-changed")
    require("apps/desktop/src-tauri/tauri.conf.json" in manifest["packagingInputSha256"], "missing-preview-config-evidence")
    checks = manifest.get("linuxPackageVerification", {})
    require(checks.get("bundledNoticeHashesVerified") is True and checks.get("systemLibrariesBundled") is False, "missing-linux-payload-verification")
    return path


def verify_deb_identity(path: Path) -> tuple[str, str, str]:
    config = json.loads((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text(encoding="utf8"))
    expected_name = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "-", config["productName"]).lower()
    name = output(["dpkg-deb", "-f", str(path), "Package"]).decode().strip()
    version = output(["dpkg-deb", "-f", str(path), "Version"]).decode().strip()
    architecture = output(["dpkg-deb", "-f", str(path), "Architecture"]).decode().strip()
    require((name, version, architecture) == (expected_name, config["version"], "amd64"), "preview-deb-identity-mismatch")
    payload = output(["dpkg-deb", "--fsys-tarfile", str(path)])
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:") as archive:
        entries = [entry for entry in archive if entry.name.removeprefix("./") == "usr/bin/primerswitch"]
        require(len(entries) == 1 and entries[0].isfile() and entries[0].mode & 0o111 != 0, "preview-deb-executable-missing")
        binary_hash = sha256(archive.extractfile(entries[0]).read())
    control = output(["dpkg-deb", "--ctrl-tarfile", str(path)])
    with tarfile.open(fileobj=io.BytesIO(control), mode="r:") as archive:
        require(not any(Path(entry.name).name in {"preinst", "postinst", "prerm", "postrm", "triggers"} for entry in archive), "unreviewed-deb-maintainer-script")
    return name, version, binary_hash


def fixture_environment(directory: Path) -> dict:
    require((directory / "fixture.marker").read_text() == MARKER, "isolated-fixture-marker-missing")
    directories = {"HOME": "home", "XDG_CONFIG_HOME": "config", "XDG_DATA_HOME": "data", "XDG_CACHE_HOME": "cache", "XDG_STATE_HOME": "state", "XDG_RUNTIME_DIR": "runtime"}
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
                   "GTK_MODULES": "atk-bridge", "ACCESSIBILITY_ENABLED": "1",
                   "GSETTINGS_BACKEND": "memory", "GDK_BACKEND": "x11"}
    for key, relative in directories.items():
        path = directory / relative
        path.mkdir(mode=0o700)
        path.chmod(0o700)
        environment[key] = str(path)
    return environment


def accessible_nodes(node, pyatspi, *, limit: int = 6000) -> list[dict]:
    rows = []
    pending = [(node, None, 0)]
    while pending and len(rows) < limit:
        current, parent, depth = pending.pop()
        if depth > 45:
            continue
        try:
            name = current.name or ""
            role = current.getRoleName()
            states = current.getState()
            text = ""
            try:
                interface = current.queryText()
                text = interface.getText(0, min(interface.characterCount, 4096))
            except (NotImplementedError, AttributeError):
                pass
            index = len(rows)
            rows.append({"name": name, "role": role, "text": text, "parent": parent,
                         "enabled": states.contains(pyatspi.STATE_ENABLED), "accessible": current})
            for child_index in reversed(range(min(current.childCount, 1000))):
                child = current.getChildAtIndex(child_index)
                if child is not None:
                    pending.append((child, index, depth + 1))
        except Exception:
            # Accessibility objects can disappear during initial WebKit load.
            continue
    return rows


def window_details() -> dict | None:
    tree = output(["xwininfo", "-root", "-tree"], timeout=8).decode("utf8", errors="replace")
    for line in tree.splitlines():
        match = re.search(r'(0x[0-9a-f]+) "(PrimerSwitch[^\"]*)".*? (\d+)x(\d+)\+', line, re.I)
        if match:
            details = output(["xwininfo", "-id", match.group(1)], timeout=8).decode("utf8", errors="replace")
            require("Map State: IsViewable" in details, "native-demo-window-not-visible")
            width = int(re.search(r"Width: (\d+)", details).group(1))
            height = int(re.search(r"Height: (\d+)", details).group(1))
            require(width >= 560 and height >= 620, "native-demo-window-too-small")
            return {"id": match.group(1), "title": match.group(2), "width": width, "height": height}
    return None


def worker(directory: Path) -> int:
    report = {"formatVersion": 1, "status": "failed", "scope": "Installed Ubuntu 24.04 x86_64 demo accessibility and native window; no provider/live account qualification", "checks": {}}
    application = None
    try:
        require((directory / "fixture.marker").read_text() == MARKER, "isolated-fixture-marker-missing")
        for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"):
            value = Path(os.environ[key])
            require(value.resolve().is_relative_to(directory.resolve()) and stat.S_IMODE(value.stat().st_mode) == 0o700, "unsafe-demo-environment")
        require(bool(os.environ.get("DISPLAY")) and bool(os.environ.get("DBUS_SESSION_BUS_ADDRESS")), "fresh-display-or-session-bus-missing")
        # Enable accessibility only on this newly created private session bus.
        output(["gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus",
                "--method", "org.freedesktop.DBus.Properties.Set", "org.a11y.Status", "IsEnabled", "<true>"])
        import pyatspi
        application = subprocess.Popen(["/usr/bin/primerswitch", "--demo"], env=os.environ.copy(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        report["checks"]["installedDemoLaunched"] = True
        deadline = time.monotonic() + 75
        rows, window = [], None
        while time.monotonic() < deadline:
            require(application.poll() is None, "installed-demo-exited-before-rendering")
            window = window_details()
            rows = accessible_nodes(pyatspi.Registry.getDesktop(0), pyatspi)
            content = "\n".join(row["name"] + " " + row["text"] for row in rows)
            if window and "Demo data" in content and "actions are disabled" in content:
                break
            time.sleep(0.5)
        else:
            raise QualificationError("installed-demo-accessible-renderer-timeout")
        report["accessibilityNodeCount"] = len(rows)
        report["observedLabels"] = sorted({row["name"] for row in rows if row["name"] in {"Accounts", "Open settings", "Automation", "Refresh all", "Switch", "Preview"}})
        require(any(row["name"] == "Accounts" for row in rows), "english-accounts-navigation-missing")
        settings = [row for row in rows if row["name"] == "Open settings" and row["role"] == "push button"]
        require(len(settings) == 1 and settings[0]["enabled"], "settings-navigation-unavailable")
        require(any(row["name"] == "Automation" and row["role"] != "push button" for row in rows), "automation-panel-missing")
        require(not any(row["name"] == "Automation" and row["role"] == "push button" for row in rows), "duplicate-automation-navigation-present")
        refresh = [row for row in rows if row["role"] == "push button" and (row["name"] == "Refresh all" or row["name"].startswith("Refresh account "))]
        switches = [row for row in rows if row["role"] == "push button" and row["name"] == "Switch"]
        require(bool(refresh) and bool(switches) and all(not row["enabled"] for row in refresh + switches), "demo-mutation-controls-not-disabled")
        output(["import", "-window", window["id"], str(SCREENSHOT)], timeout=15)
        require(SCREENSHOT.is_file() and SCREENSHOT.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"), "native-demo-screenshot-missing")
        report["window"] = {key: value for key, value in window.items() if key != "id"}
        report["screenshotSha256"] = sha256(SCREENSHOT.read_bytes())
        report["checks"].update({"visibleNativeWindow": True, "englishAccountsAndSettings": True,
                                 "nativeDemoSnapshotRendered": True, "duplicateAutomationNavigationAbsent": True,
                                 "automationPanelPresent": True, "demoMutationButtonsDisabled": True})
        require(settings[0]["accessible"].queryAction().doAction(0), "settings-navigation-action-failed")
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            dialog_rows = accessible_nodes(pyatspi.Registry.getDesktop(0), pyatspi)
            saves = [row for row in dialog_rows if row["name"] == "Save" and row["role"] == "push button"]
            if saves:
                require(all(not row["enabled"] for row in saves), "demo-settings-save-not-disabled")
                report["checks"]["settingsDialogReadOnly"] = True
                break
            time.sleep(0.25)
        else:
            raise QualificationError("native-settings-dialog-timeout")
        # The in-memory demo must not create CLI credential or encrypted-vault files.
        forbidden = {".claude", ".claude.json", ".credentials.json", ".vault.lock"}
        require(not any(path.name in forbidden or path.suffix == ".vault" or path.name.startswith("master-key.") for path in directory.rglob("*")), "demo-created-credential-or-vault-file")
        report["checks"]["isolatedHomeNoCredentialWrites"] = True
        report["status"] = "passed"
        return 0
    except Exception as error:
        report["error"] = str(error) if isinstance(error, QualificationError) else "installed-demo-qualification-could-not-complete"
        report["failureType"] = type(error).__name__
        return 2
    finally:
        if application is not None and application.poll() is None:
            application.terminate()
            try:
                application.wait(timeout=10)
            except subprocess.TimeoutExpired:
                application.kill()
                application.wait(timeout=5)
        write_report(report)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", choices=("deb",), default="deb")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--fixture-dir", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker:
        require(args.fixture_dir is not None, "isolated-fixture-required")
        return worker(args.fixture_dir)
    report = {"formatVersion": 1, "status": "failed", "checks": {}}
    try:
        release = Path("/etc/os-release").read_text() if platform.system() == "Linux" else ""
        validate_ci_host(platform.system(), platform.machine(), os.environ, release)
        ARTIFACTS.mkdir(exist_ok=True)
        REPORT.unlink(missing_ok=True)
        SCREENSHOT.unlink(missing_ok=True)
        manifest = json.loads((ARTIFACTS / "package-manifest.json").read_text(encoding="utf8"))
        package = verify_package_manifest(manifest, ROOT)
        require(manifest.get("sourceCommit") == output(["git", "rev-parse", "HEAD"]).decode().strip(), "preview-source-commit-mismatch")
        name, version, binary_hash = verify_deb_identity(package)
        subprocess.run(["sudo", "apt-get", "install", "-y", str(package)], cwd=ROOT, check=True, timeout=180)
        installed = Path("/usr/bin/primerswitch")
        require(installed.is_file() and not installed.is_symlink() and sha256(installed.read_bytes()) == binary_hash, "installed-executable-hash-mismatch")
        require(output(["dpkg-query", "-W", "-f", "${Version}", name]).decode().strip() == version, "installed-package-version-mismatch")
        with tempfile.TemporaryDirectory(prefix="primerswitch-installed-demo-") as temporary:
            directory = Path(temporary)
            directory.chmod(0o700)
            (directory / "fixture.marker").write_text(MARKER)
            environment = fixture_environment(directory)
            command = ["dbus-run-session", "--", "xvfb-run", "-a", "--server-args=-screen 0 1440x1000x24",
                       "/usr/bin/python3", str(Path(__file__).resolve()), "--worker", "--fixture-dir", str(directory)]
            process = subprocess.Popen(command, cwd=ROOT, env=environment, start_new_session=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                code = process.wait(timeout=120)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
                raise QualificationError("isolated-native-demo-timeout")
        require(REPORT.is_file(), "installed-demo-report-missing")
        report = json.loads(REPORT.read_text(encoding="utf8"))
        report["packageSha256"] = sha256(package.read_bytes())
        report["installedExecutableSha256"] = binary_hash
        report["packageName"] = name
        report["packageVersion"] = version
        report["sourceCommit"] = manifest.get("sourceCommit")
        report["checks"]["verifiedOwnPackageInstalled"] = True
        write_report(report)
        require(code == 0 and report.get("status") == "passed", report.get("error", "installed-demo-check-failed"))
        print("Installed Linux demo qualification passed; report and native PNG captured.")
        return 0
    except Exception as error:
        report["status"] = "failed"
        report["error"] = str(error) if isinstance(error, QualificationError) else "installed-demo-qualification-could-not-complete"
        report["failureType"] = type(error).__name__
        write_report(report)
        print("Installed Linux demo qualification failed: " + report["error"], file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
