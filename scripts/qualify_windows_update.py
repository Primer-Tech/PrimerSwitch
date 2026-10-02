#!/usr/bin/env python3
"""Qualify installer file retention only on an ephemeral hosted Windows x64 runner.

Never starts PrimerSwitch or opens real accounts, CLI credentials or OS keyrings.
Synthetic files test installer retention; the separate historical runtime fixture
qualifies encrypted schema/reopen behavior. --validate and --help are read-only.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
ARTIFACTS = ROOT / ".artifacts"
REPORT = ARTIFACTS / "windows-update-preservation-report.json"
OLD_URL = "https://github.com/Primer-Tech/PrimerSwitch/releases/download/v0.1.0-preview.1/PrimerSwitch_0.1.0_x64-setup.exe"
OLD_BYTES = 5085753
OLD_SHA256 = "752841fbee0eff3a992766aae2600b67dd57b084122c601e49a558b64058684b"
MARKER = "PrimerSwitch hosted Windows installer fixture v1\n"
REGISTRY_KEYS = (
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PrimerSwitch",
    r"Software\Primer-Tech\PrimerSwitch",
)
SENTINELS = {
    "runtime-state.vault": b"PRMSV001\x00\xffSYNTHETIC_INSTALLER_RETENTION_ONLY_STATE\x00",
    "master-key.dpapi": b"\x00\xffSYNTHETIC_INSTALLER_RETENTION_ONLY_KEY\x00",
    "settings.json": b'{"appearance":"light","language":"ro","opaque":{"keep":[1,2]}}\n',
    "journal-fixture.vault": b"PRMSV001\x00\xffSYNTHETIC_INSTALLER_RETENTION_ONLY_JOURNAL\x00",
    "opaque-fixture.bin": bytes(range(256)),
}


class QualificationError(Exception):
    pass


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise QualificationError(reason)


def sha256(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def validate_ci_host(system: str, machine: str, environment: dict) -> None:
    require(system == "Windows" and machine.lower() in {"amd64", "x86_64"}, "requires-native-windows-x64")
    require(environment.get("GITHUB_ACTIONS") == "true" and environment.get("RUNNER_ENVIRONMENT") == "github-hosted", "requires-ephemeral-github-hosted-runner")


def safe_file(root: Path, label: object) -> Path:
    require(isinstance(label, str) and bool(label), "invalid-evidence-path")
    relative = Path(label)
    file = root / relative
    require(not relative.is_absolute() and ".." not in relative.parts and not file.is_symlink() and file.resolve().is_relative_to(root.resolve()) and file.is_file(), "unsafe-evidence-path")
    return file


def verify_package_manifest(manifest: dict, root: Path, commit: str) -> tuple[Path, Path]:
    require(manifest.get("formatVersion") == 2 and manifest.get("bundle") == "nsis" and manifest.get("target") == "x86_64-pc-windows-msvc" and manifest.get("unsignedPreview") is True, "requires-native-nsis-manifest")
    require(re.fullmatch(r"[0-9a-f]{40}", commit) is not None and manifest.get("sourceCommit") == commit, "package-source-commit-mismatch")
    records = manifest.get("packages")
    require(isinstance(records, list) and len(records) == 1, "ambiguous-preview-package")
    record = records[0]
    package = safe_file(root, record.get("path"))
    require(package.resolve().parent == (root / "target/release/bundle/nsis").resolve() and package.suffix == ".exe", "unsafe-preview-package-path")
    require(package.stat().st_size == record.get("bytes") and sha256(package.read_bytes()) == record.get("sha256"), "preview-package-hash-mismatch")
    for field, base in (("evidenceSha256", root / ".artifacts"), ("packagingInputSha256", root)):
        entries = manifest.get(field)
        require(isinstance(entries, dict) and bool(entries), "missing-preview-input-evidence")
        for name, digest in entries.items():
            require(sha256(safe_file(base, name).read_bytes()) == digest, "preview-input-changed")
    config_label = "apps/desktop/src-tauri/tauri.conf.json"
    require(config_label in manifest["packagingInputSha256"], "missing-preview-config-evidence")
    config = json.loads(safe_file(root, config_label).read_text(encoding="utf8"))
    require(config.get("productName") == "PrimerSwitch" and config.get("identifier") == "com.primertech.primerswitch" and config.get("bundle", {}).get("publisher") == "Primer-Tech" and config["bundle"].get("windows", {}).get("nsis", {}).get("installMode") == "currentUser", "preview-install-identity-changed")
    version = config.get("version", "")
    require(isinstance(version, str) and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is not None and tuple(map(int, version.split("."))) > (0, 1, 0), "qualification-requires-new-preview-version")
    binary = safe_file(root, "target/release/primerswitch.exe")
    return package, binary


def registry_state() -> dict:
    import winreg
    result = {}
    for name in REGISTRY_KEYS:
        for view in (winreg.KEY_WOW64_64KEY, winreg.KEY_WOW64_32KEY):
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, name, 0, winreg.KEY_READ | view) as key:
                    values = {}
                    for index in range(winreg.QueryInfoKey(key)[1]):
                        label, value, _ = winreg.EnumValue(key, index)
                        values[label] = value
                    require(name not in result or result[name] == values, "ambiguous-primerswitch-registry-views")
                    result[name] = values
            except FileNotFoundError:
                continue
    return result


def require_clean_registration(state: dict) -> None:
    require(not state, "existing-primerswitch-registration-refused")


def verify_registration(state: dict, directory: Path, version: str) -> None:
    require(set(state) == set(REGISTRY_KEYS), "fixture-install-registration-missing")
    uninstall, product = (state[name] for name in REGISTRY_KEYS)
    require(isinstance(product.get(""), str) and Path(product[""]).resolve() == directory.resolve(), "registration-outside-isolated-fixture")
    require(uninstall.get("DisplayName") == "PrimerSwitch" and uninstall.get("Publisher") == "Primer-Tech" and uninstall.get("DisplayVersion") == version, "fixture-install-identity-mismatch")
    command = uninstall.get("UninstallString")
    expected = str(directory / "uninstall.exe")
    require(isinstance(command, str) and command.strip('"') == expected, "fixture-uninstaller-identity-mismatch")


def validate_fixture(directory: Path, base: Path) -> None:
    require(directory.is_absolute() and not directory.is_symlink() and directory.resolve() == directory and directory.is_relative_to(base.resolve()) and directory != base.resolve() and directory.name.startswith("primerswitch-update-"), "unsafe-isolated-fixture")
    marker = directory / "fixture.marker"
    require(not marker.is_symlink() and marker.is_file() and marker.read_text(encoding="utf8") == MARKER, "isolated-fixture-marker-missing")
    # Windows junctions are reparse points even when is_symlink() is false.
    pending = [directory]
    while pending:
        file = pending.pop()
        metadata = file.lstat()
        require(not file.is_symlink() and not getattr(metadata, "st_file_attributes", 0) & 0x400, "reparse-point-in-isolated-fixture")
        if file.is_dir():
            pending.extend(file.iterdir())


def run_silent(executable: Path, arguments: list[str], directory: Path | None = None) -> None:
    # NSIS requires /D last and unquoted, including paths containing spaces.
    raw_tail = arguments[-1] if arguments and arguments[-1].startswith("_?=") else None
    quoted_arguments = arguments[:-1] if raw_tail else arguments
    command = subprocess.list2cmdline([str(executable), *quoted_arguments])
    if raw_tail is not None:
        require(not any(character in raw_tail for character in '\r\n"'), "unsafe-uninstall-directory")
        command += " " + raw_tail
    if directory is not None:
        require(not any(character in str(directory) for character in '\r\n"'), "unsafe-install-directory")
        command += " /D=" + str(directory)
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    completed = subprocess.run(command, executable=str(executable), cwd=ROOT, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, startupinfo=startup, creationflags=subprocess.CREATE_NO_WINDOW, timeout=180, check=False)
    require(completed.returncode == 0, "silent-installer-nonzero-exit")


def installation_evidence(directory: Path, state: dict, old_hash: str, expected_binary: Path) -> dict:
    """Capture only own fixture names/hashes and sanitized registry identity."""
    executable = directory / "primerswitch.exe"
    actual = executable.read_bytes() if executable.is_file() and not executable.is_symlink() else b""
    expected = expected_binary.read_bytes()
    actual_hash, expected_hash = sha256(actual), sha256(expected)
    first_difference = last_difference = None
    different_count = 0
    for index, (left, right) in enumerate(zip(actual, expected)):
        if left != right:
            if first_difference is None:
                first_difference = index
            last_difference = index
            different_count += 1
    if len(actual) != len(expected):
        if first_difference is None:
            first_difference = min(len(actual), len(expected))
        last_difference = max(len(actual), len(expected)) - 1
    version = state.get(REGISTRY_KEYS[0], {}).get("DisplayVersion")
    safe_version = version if isinstance(version, str) and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[.-][a-zA-Z0-9.-]+)?", version) else "unrecognized"
    files = []
    for file in sorted(directory.rglob("*")):
        if file.is_file():
            files.append({"file": file.relative_to(directory).as_posix(), "bytes": file.stat().st_size, "sha256": sha256(file.read_bytes())})
    return {"registeredVersion": safe_version, "installedApplicationSha256": actual_hash,
            "matchesOldApplication": actual_hash == old_hash, "matchesExpectedNewApplication": actual_hash == expected_hash,
            "expectedApplicationSha256AtPhase": expected_hash, "installedApplicationBytes": len(actual), "expectedApplicationBytes": len(expected),
            "firstDifferentByteOffset": first_difference, "lastDifferentByteOffset": last_difference,
            "differentOverlappingByteCount": different_count,
            "observedFiles": files,
            "sentinelBytesPreserved": {name: (directory / name).is_file() and (directory / name).read_bytes() == body for name, body in SENTINELS.items()}}


def verify_retention(directory: Path, expected_binary: str) -> dict:
    for name, original in SENTINELS.items():
        file = directory / name
        require(file.is_file() and not file.is_symlink() and file.read_bytes() == original, "fixture-sentinel-bytes-changed")
    application = directory / "primerswitch.exe"
    require(application.is_file() and not application.is_symlink() and sha256(application.read_bytes()) == expected_binary, "installed-application-hash-mismatch")
    return {"fileCount": len(SENTINELS), "allSentinelBytesPreserved": True, "installedApplicationSha256": expected_binary}


def download_old_installer(directory: Path) -> Path:
    request = urllib.request.Request(OLD_URL, headers={"User-Agent": "PrimerSwitch-isolated-update-qualification"})
    with urllib.request.urlopen(request, timeout=60) as response:
        body = response.read(OLD_BYTES + 1)
    require(len(body) == OLD_BYTES and sha256(body) == OLD_SHA256, "old-public-installer-pin-mismatch")
    path = directory / "pinned-preview-one-setup.exe"
    path.write_bytes(body)
    return path


def cleanup_fixture(directory: Path, base: Path, install: Path) -> None:
    validate_fixture(directory, base)
    state = registry_state()
    if state:
        # Never execute an uninstall command obtained from the registry.
        product = state.get(REGISTRY_KEYS[1], {})
        require(isinstance(product.get(""), str) and Path(product[""]).resolve() == install.resolve(), "cleanup-refused-foreign-registration")
        uninstaller = install / "uninstall.exe"
        if uninstaller.is_file() and not uninstaller.is_symlink():
            run_silent(uninstaller, ["/S", "_?=" + str(install)])
        import winreg
        for name, values in registry_state().items():
            if name == REGISTRY_KEYS[1]:
                require(isinstance(values.get(""), str) and Path(values[""]).resolve() == install.resolve(), "cleanup-refused-foreign-registration")
            elif name == REGISTRY_KEYS[0]:
                require(values.get("UninstallString", "").strip('"') == str(uninstaller), "cleanup-refused-foreign-uninstaller")
            for view in (winreg.KEY_WOW64_64KEY, winreg.KEY_WOW64_32KEY):
                try:
                    winreg.DeleteKeyEx(winreg.HKEY_CURRENT_USER, name, view, 0)
                except FileNotFoundError:
                    pass
    require_clean_registration(registry_state())
    validate_fixture(directory, base)
    shutil.rmtree(directory)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--validate", action="store_true", help="read-only safety/manifest validation; never install, download, seed or clean up")
    args = parser.parse_args()
    if not args.validate:
        try:
            validate_ci_host(platform.system(), platform.machine(), dict(os.environ))
        except QualificationError as error:
            print("Windows installer qualification refused: " + str(error), file=sys.stderr)
            return 1
    report = {"formatVersion": 1, "qualification": "isolated-installer-file-retention", "oldPublicInstallerSha256": OLD_SHA256, "oldPublicInstallerBytes": OLD_BYTES, "oldPublicInstallerUrl": OLD_URL, "installerMode": "silent official /UPDATE with an isolated /D directory; same new installer applied twice", "appStarted": False, "realCredentialsAccessed": False, "runtimeSchemaEvidence": "separate preview_one_state_survives_branded_reinstall_and_settings_mutation fixture", "ok": False}
    fixture = base = install = None
    stage = "host-guard"
    try:
        if args.validate:
            permitted = True
            try:
                validate_ci_host(platform.system(), platform.machine(), dict(os.environ))
            except QualificationError:
                permitted = False
            print(json.dumps({"mode": "read-only-validation", "hostPermitted": permitted, "manifestAvailable": (ARTIFACTS / "package-manifest.json").is_file(), "installerExecution": False}, sort_keys=True))
            if not (ARTIFACTS / "package-manifest.json").is_file():
                return 0
        else:
            validate_ci_host(platform.system(), platform.machine(), dict(os.environ))
        stage = "package-provenance"
        commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, encoding="ascii").stdout.strip()
        manifest = json.loads((ARTIFACTS / "package-manifest.json").read_text(encoding="utf8"))
        package, binary = verify_package_manifest(manifest, ROOT, commit)
        report.update({"sourceCommit": commit, "newInstallerSha256": sha256(package.read_bytes()), "newApplicationSha256": sha256(binary.read_bytes())})
        if args.validate:
            print(json.dumps({"packageInputsVerified": True, "installerExecution": False}, sort_keys=True))
            return 0
        stage = "clean-runner-registration"
        require_clean_registration(registry_state())
        stage = "isolated-fixture"
        value = os.environ.get("RUNNER_TEMP")
        require(bool(value) and Path(value).is_absolute() and Path(value).is_dir(), "hosted-runner-temp-unavailable")
        base = Path(value).resolve()
        fixture = Path(tempfile.mkdtemp(prefix="primerswitch-update-", dir=base)).resolve()
        (fixture / "fixture.marker").write_text(MARKER, encoding="utf8")
        install = fixture / "installation"
        validate_fixture(fixture, base)
        stage = "pinned-old-installer"
        old = download_old_installer(fixture)
        stage = "old-isolated-install"
        # /UPDATE avoids WebView2 bootstrap/download and still creates registration.
        run_silent(old, ["/S", "/UPDATE"], install)
        verify_registration(registry_state(), install, "0.1.0")
        require((install / "primerswitch.exe").is_file(), "old-application-not-installed")
        old_binary_hash = sha256((install / "primerswitch.exe").read_bytes())
        report["oldApplicationSha256"] = old_binary_hash
        require(old_binary_hash != report["newApplicationSha256"], "new-preview-binary-not-distinct")
        for name, body in SENTINELS.items():
            (install / name).write_bytes(body)
        config = json.loads((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_text(encoding="utf8"))
        phases = {}
        report["phases"] = phases
        report["sentinelSha256"] = {name: sha256(body) for name, body in SENTINELS.items()}
        for name in ("upgrade", "same-version-reinstall"):
            stage = name
            validate_fixture(fixture, base)
            require(sha256(package.read_bytes()) == report["newInstallerSha256"], "new-installer-changed-before-execution")
            run_silent(package, ["/S", "/UPDATE"], install)
            validate_fixture(fixture, base)
            state = registry_state()
            phases[name] = installation_evidence(install, state, old_binary_hash, binary)
            verify_registration(state, install, config["version"])
            require(phases[name]["expectedApplicationSha256AtPhase"] == report["newApplicationSha256"], "compiled-application-changed-during-qualification")
            phases[name].update(verify_retention(install, report["newApplicationSha256"]))
        report.update({"oldApplicationSha256": old_binary_hash, "phases": phases, "sentinelSha256": {name: sha256(body) for name, body in SENTINELS.items()}, "ok": True})
    except (QualificationError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        report["failure"] = str(error) if isinstance(error, QualificationError) else "qualification-tool-or-input-failure"
        report["failureStage"] = stage
    finally:
        if fixture is not None and install is not None:
            try:
                cleanup_fixture(fixture, base, install)
                report["ownFixtureCleanup"] = True
            except (QualificationError, OSError, ValueError, subprocess.SubprocessError):
                report.update({"ok": False, "ownFixtureCleanup": False, "cleanupFailure": "own-fixture-cleanup-failed-closed"})
        if not args.validate:
            ARTIFACTS.mkdir(exist_ok=True)
            REPORT.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf8")
    if not report["ok"]:
        print("Windows installer qualification failed: " + report.get("failure", "own-fixture-cleanup-failed-closed"), file=sys.stderr)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
