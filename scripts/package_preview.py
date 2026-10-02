#!/usr/bin/env python3
"""Build an unsigned native preview with verified, bundled dependency notices."""
from __future__ import annotations
import argparse
import hashlib
import json
import io
import platform
import stat
import tarfile
import tempfile
import tomllib
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
DESKTOP = ROOT / "apps" / "desktop"
ARTIFACTS = ROOT / ".artifacts"
MPL_VERSIONS = {
    "cssparser": "0.37.0",
    "cssparser-macros": "0.7.1",
    "dtoa-short": "0.3.5",
    "option-ext": "0.2.0",
    "selectors": "0.38.0",
}

REVIEWED_EXPRESSIONS = {name: (version, "MPL-2.0") for name, version in MPL_VERSIONS.items()}
REVIEWED_EXPRESSIONS["rustix"] = ("1.1.5", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT")
REVIEWED_EXPRESSIONS["linux-raw-sys"] = ("0.12.1", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT")
REVIEWED_EXPRESSIONS["target-lexicon"] = ("0.12.16", "Apache-2.0 WITH LLVM-exception")
BUNDLE_SUFFIXES = {"nsis": "*.exe", "dmg": "*.dmg", "deb": "*.deb", "rpm": "*.rpm"}
CONFIG_FILES = ("apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/src-tauri/tauri.preview.conf.json", "scripts/package_preview.py", "docs/legal/brand/manifest.json", "docs/legal/brand/Comfortaa-OFL.txt", "apps/desktop/src/assets/fonts/comfortaa-600.subset.woff2", "apps/desktop/src-tauri/Cargo.toml")
CURRENT_STAGE = "argument-or-native-host-validation"
FAILURE_REPORT = "package-failure.json"
EVIDENCE_FILES = ("frontend-bundle-inventory.json", "THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_METADATA.json", "THIRD_PARTY_REPORT.json")


class PackagingError(Exception):
    pass


def command(arguments: list[str], *, cwd: Path = ROOT, accepted: set[int] | None = None) -> None:
    process = subprocess.run(arguments, cwd=cwd, check=False)
    if process.returncode not in (accepted or {0}):
        raise PackagingError("build-step-failed")


def executable(name: str) -> str:
    result = shutil.which(name)
    if not result:
        raise PackagingError(f"required-tool-unavailable-{name}")
    return result


def load_json(name: str) -> dict:
    value = json.loads((ARTIFACTS / name).read_text(encoding="utf8"))
    if not isinstance(value, dict):
        raise PackagingError("invalid-packaging-evidence")
    return value


def verify_attribution(target: str) -> dict:
    metadata = load_json("THIRD_PARTY_METADATA.json")
    inputs = metadata.get("inputSha256")
    if not isinstance(inputs, dict) or not inputs:
        raise PackagingError("missing-attribution-input-hashes")
    for name, expected in inputs.items():
        original = ROOT / name
        path = original.resolve()
        if not path.is_relative_to(ROOT.resolve()) or original.is_symlink() or not re.fullmatch(r"[a-f0-9]{64}", expected):
            raise PackagingError("unsafe-attribution-input")
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise PackagingError("dependency-input-changed-after-attribution")
    report = load_json("THIRD_PARTY_REPORT.json")
    if metadata.get("target") != target or report.get("target") != target or report.get("missingLicenseTextCount") != 0:
        raise PackagingError("incomplete-native-attribution")
    if not isinstance(report.get("findings"), list) or not isinstance(report.get("dependencyCount"), int) or report["dependencyCount"] <= 0:
        raise PackagingError("invalid-attribution-report")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf8"))
    locked_checksums = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
    for finding in report["findings"]:
        name = finding.get("name")
        proof = finding.get("sourceIntegrity", {})
        # Exact reviewed dependencies have full license/exception texts and
        # checksum-verified original source links in the shipped notices.
        if (
            finding.get("check") != "license-expression-review"
            or finding.get("ecosystem") != "cargo"
            or REVIEWED_EXPRESSIONS.get(name) != (finding.get("version"), finding.get("declaredLicense"))
            or proof.get("status") != "verified"
            or not re.fullmatch(r"[a-f0-9]{64}", proof.get("archiveSha256", ""))
            or proof.get("archiveSha256") != locked_checksums.get((name, finding.get("version")))
            or proof.get("archiveUrl") != f"https://static.crates.io/crates/{name}/{name}-{finding.get('version')}.crate"
            or not proof.get("upstreamRevision")
            or not isinstance(proof.get("verifiedSourceFileCount"), int)
            or proof["verifiedSourceFileCount"] <= 0
        ):
            raise PackagingError("unresolved-attribution-finding")
    for name in ("THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_REPORT.json", "THIRD_PARTY_METADATA.json"):
        if not (ARTIFACTS / name).is_file():
            raise PackagingError("missing-bundled-notices")
    return report


def verify_bundle() -> dict:
    inventory = load_json("frontend-bundle-inventory.json")
    out_dir = DESKTOP / "dist"
    expected = {}
    for record in inventory.get("chunks", []) + inventory.get("assets", []):
        original = out_dir / record["file"]
        path = original.resolve()
        if record["file"] in expected or not path.is_relative_to(out_dir.resolve()) or not path.is_file() or original.is_symlink():
            raise PackagingError("invalid-renderer-output")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != record["sha256"]:
            raise PackagingError("renderer-changed-after-attribution")
        expected[record["file"]] = digest
    actual = {p.relative_to(out_dir).as_posix() for p in out_dir.rglob("*") if p.is_file()}
    if not expected or actual != set(expected):
        raise PackagingError("renderer-inventory-incomplete")
    return inventory



def verify_installer_notices() -> dict:
    manifest = json.loads((ROOT / "docs/legal/packaging/manifest.json").read_text(encoding="utf8"))
    for license_file in manifest["licenses"]:
        original = ROOT / license_file["file"]
        path = original.resolve()
        if not path.is_relative_to((ROOT / "docs/legal/packaging").resolve()) or original.is_symlink():
            raise PackagingError("unsafe-installer-notice")
        if hashlib.sha256(path.read_bytes()).hexdigest() != license_file["sha256"]:
            raise PackagingError("changed-installer-notice")
    return manifest


def verify_brand_notices() -> dict:
    manifest = json.loads((ROOT / "docs/legal/brand/manifest.json").read_text(encoding="utf8"))
    expected_paths = {"font": "apps/desktop/src/assets/fonts/comfortaa-600.subset.woff2",
                      "license": "docs/legal/brand/Comfortaa-OFL.txt"}
    for key, expected_path in expected_paths.items():
        record = manifest[key]
        path = ROOT / expected_path
        if record["file"] != expected_path or path.is_symlink() or not path.resolve().is_relative_to(ROOT.resolve()):
            raise PackagingError("unsafe-brand-attribution")
        if hashlib.sha256(path.read_bytes()).hexdigest() != record["sha256"]:
            raise PackagingError("brand-font-or-license-changed")
    if manifest["license"]["spdx"] != "OFL-1.1":
        raise PackagingError("unreviewed-brand-license")
    return manifest


def merge_config(base: object, overlay: object) -> object:
    if not isinstance(overlay, dict):
        return overlay
    result = dict(base) if isinstance(base, dict) else {}
    for key, value in overlay.items():
        if value is None:
            result.pop(key, None)
        else:
            result[key] = merge_config(result.get(key), value)
    return result


def effective_preview_config(bundle: str, overlay: dict) -> dict:
    directory = DESKTOP / "src-tauri"
    config = json.loads((directory / "tauri.conf.json").read_text(encoding="utf8"))
    platform_name = {"nsis": "windows", "dmg": "macos", "deb": "linux", "rpm": "linux"}[bundle]
    # Additional platform flavors must be explicitly bound/reviewed first;
    # silently applying one would defeat stable identity and input provenance.
    if any((directory / f"tauri.{platform_name}.conf{suffix}").exists() for suffix in (".json", ".json5", ".toml")):
        raise PackagingError("unreviewed-platform-config")
    preview = json.loads((directory / "tauri.preview.conf.json").read_text(encoding="utf8"))
    return merge_config(merge_config(config, preview), overlay)


def verify_preview_identity(config: dict, cargo_manifest: dict) -> None:
    bundle = config.get("bundle", {})
    nsis = bundle.get("windows", {}).get("nsis", {})
    package = cargo_manifest.get("package", {})
    require_binary = package.get("default-run") == "primerswitch" and any(record.get("name") == "primerswitch" for record in cargo_manifest.get("bin", []))
    if (config.get("productName") != "PrimerSwitch" or config.get("identifier") != "com.primertech.primerswitch"
            or bundle.get("publisher") != "Primer-Tech" or nsis.get("installMode") != "currentUser"
            or config.get("mainBinaryName", "primerswitch") != "primerswitch" or not require_binary):
        raise PackagingError("preview-identity-would-change-existing-installation")
    if bundle.get("windows", {}).get("allowDowngrades") is not False:
        raise PackagingError("preview-installer-would-permit-downgrade")
    if nsis.get("template") or nsis.get("installerHooks"):
        raise PackagingError("unreviewed-nsis-template-or-hooks")
    for destination in bundle.get("resources", {}).values():
        if re.search(r"(?:\.vault|master[-_]key|runtime[-_]state|switch[-_]journal)", destination, re.I):
            raise PackagingError("resource-would-overwrite-existing-app-data")


def verify_nsis_data_preservation(text: str, resource_destinations: list[str]) -> None:
    identities = {"PRODUCTNAME": "PrimerSwitch", "BUNDLEID": "com.primertech.primerswitch", "MANUFACTURER": "Primer-Tech", "INSTALLMODE": "currentUser", "MAINBINARYNAME": "primerswitch"}
    for key, expected in identities.items():
        if not re.search(r'^!define ' + key + r' "' + re.escape(expected) + r'"$', text, re.M):
            raise PackagingError("generated-installer-identity-changed")
    allowed_files = {"${MAINBINARYNAME}.exe", "uninstall.exe", "$OldMainBinaryName"}
    allowed_files.update(destination.replace("/", "\\") for destination in resource_destinations)
    allowed_directories = {str(Path(destination).parent).replace("/", "\\") for destination in resource_destinations if str(Path(destination).parent) != "."}
    data_guard = re.search(r'\$\{If\} \$DeleteAppDataCheckboxState = 1\s+\$\{AndIf\} \$UpdateMode <> 1(?P<body>.*?)\$\{EndIf\}', text, re.S)
    for match in re.finditer(r'^[ \t]*(Delete|RMDir)\b([^\r\n]*)', text, re.M | re.I):
        operation, arguments = match.group(1).lower(), match.group(2)
        paths = re.findall(r'"([^"\r\n]+)"', arguments)
        if len(paths) != 1:
            raise PackagingError("ambiguous-installer-cleanup")
        path = paths[0]
        recursive = bool(re.search(r'(?:^|\s)/r(?:\s|$)', arguments, re.I))
        if re.search(r'(?:\.vault|master[-_]key|runtime[-_]state|switch[-_]journal|[?*])', path, re.I):
            raise PackagingError("installer-would-remove-persisted-data")
        if path == "$INSTDIR":
            if operation != "rmdir" or recursive:
                raise PackagingError("installer-would-remove-persisted-data")
        elif path.startswith("$INSTDIR\\"):
            relative = path[len("$INSTDIR\\"):]
            if (operation == "delete" and relative not in allowed_files) or (operation == "rmdir" and relative not in allowed_directories):
                raise PackagingError("installer-would-remove-persisted-data")
        elif path in {"$APPDATA\\${BUNDLEID}", "$LOCALAPPDATA\\${BUNDLEID}"}:
            if operation != "rmdir" or not data_guard or not (data_guard.start() <= match.start() < data_guard.end()):
                raise PackagingError("installer-app-data-cleanup-not-update-guarded")
        elif not (path.startswith("$TEMP\\") or path.startswith("$SMPROGRAMS\\") or path.startswith("$DESKTOP\\")):
            raise PackagingError("unreviewed-installer-cleanup-location")


def verify_nsis_recipe(target: str, notices: dict, resource_destinations: list[str]) -> None:
    arch = {"x86_64": "x64", "aarch64": "arm64", "i686": "x86"}.get(target.split("-")[0])
    if not arch:
        raise PackagingError("unsupported-nsis-architecture")
    recipe = ROOT / "target" / "release" / "nsis" / arch / "installer.nsi"
    text = recipe.read_text(encoding="utf8")
    verify_nsis_data_preservation(text, resource_destinations)
    if not re.search(r'^!define ALLOWDOWNGRADES "false"$', text, re.M):
        raise PackagingError("generated-installer-would-permit-downgrade")
    if not re.search(r'^[ \t]*SetCompressor(?:[ \t]+/SOLID)?[ \t]+(?:"zlib"|zlib)[ \t]*$', text, re.MULTILINE | re.IGNORECASE):
        raise PackagingError("unexpected-installer-compressor")
    plugin_path = re.search(r'^!define ADDITIONALPLUGINSPATH "([^"]+)"$', text, re.MULTILINE)
    if not plugin_path:
        raise PackagingError("installer-plugin-location-unavailable")
    plugin = Path(plugin_path.group(1)) / "nsis_tauri_utils.dll"
    if hashlib.sha256(plugin.read_bytes()).hexdigest() != notices["nsisTauriUtils"]["cachedBinarySha256"]:
        raise PackagingError("installer-plugin-version-changed")
    nsis_copying = plugin.parent.parent.parent.parent / "COPYING"
    expected = next(entry["sha256"] for entry in notices["licenses"] if entry["package"] == "NSIS")
    if hashlib.sha256(nsis_copying.read_bytes()).hexdigest() != expected:
        raise PackagingError("installer-framework-version-changed")


def validate_host(bundle: str, target: str, host_os: str) -> None:
    required = {"nsis": ("pc-windows-msvc", "Windows"), "dmg": ("apple-darwin", "Darwin"),
                "deb": ("unknown-linux-gnu", "Linux"), "rpm": ("unknown-linux-gnu", "Linux")}
    suffix, operating_system = required[bundle]
    if host_os != operating_system or not target.endswith(suffix):
        raise PackagingError("bundle-requires-matching-native-host")
    if bundle in {"deb", "rpm"} and target != "x86_64-unknown-linux-gnu":
        raise PackagingError("linux-preview-requires-x86_64-gnu-host")


def linux_build_distribution(release_text: str) -> dict:
    distribution = {key: value.strip('"') for line in release_text.splitlines() if "=" in line
                    for key, value in [line.split("=", 1)] if key in {"ID", "VERSION_ID", "PRETTY_NAME"}}
    if distribution.get("ID") != "ubuntu" or distribution.get("VERSION_ID") != "24.04":
        raise PackagingError("linux-preview-requires-ubuntu-24.04-build-host")
    return distribution


def output_directory(bundle: str) -> Path:
    path = ROOT / "target" / "release" / "bundle" / bundle
    if path.is_symlink() or not path.resolve().is_relative_to(ROOT.resolve() / "target"):
        raise PackagingError("unsafe-installer-output")
    return path


def clear_package_outputs(bundle: str) -> None:
    # A failed build cannot publish a package from a previous successful run.
    path = output_directory(bundle)
    for stale in path.glob(BUNDLE_SUFFIXES[bundle]):
        if stale.is_symlink() or not stale.is_file():
            raise PackagingError("unsafe-installer-output")
        stale.unlink()
    for name in ("package-manifest.json", "SHA256SUMS.txt", FAILURE_REPORT):
        (ARTIFACTS / name).unlink(missing_ok=True)


def preview_config(bundle: str, installer_notices: dict | None) -> dict:
    # Tauri merges this overlay with the common preview configuration. Only
    # Windows NSIS actually ships these installer components.
    resources = {}
    if bundle == "nsis":
        assert installer_notices is not None
        for record in installer_notices["licenses"]:
            resources["../../../" + record["file"]] = "installer-notices/" + Path(record["file"]).name
        resources["../../../docs/legal/packaging/manifest.json"] = "installer-notices/manifest.json"
    return {"bundle": {"resources": resources}}


def hashes(names: tuple[str, ...], root: Path) -> dict:
    return {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in names}


def checked_output(arguments: list[str]) -> bytes:
    global CURRENT_STAGE
    # Fixed tool identity only: do not include arbitrary arguments, stderr or
    # exception messages in packaging diagnostics.
    tool = Path(arguments[0]).stem.lower()
    CURRENT_STAGE = "inspection-tool-" + (tool if tool in {"git", "rpm", "rpmbuild", "rpm2cpio", "readelf", "dpkg-deb"} else "other")
    try:
        return subprocess.run(arguments, cwd=ROOT, capture_output=True, check=True).stdout
    except subprocess.CalledProcessError as error:
        raise PackagingError(f"{CURRENT_STAGE}-failed-exit-{error.returncode}") from error



def cpio_files(data: bytes) -> dict[str, bytes]:
    # Read rpm2cpio's newc stream without extracting it into the filesystem.
    files = {}
    offset = 0
    while offset + 110 <= len(data):
        header = data[offset:offset + 110]
        if header[:6] not in (b"070701", b"070702"):
            raise PackagingError("invalid-rpm-payload")
        try:
            fields = [int(header[6 + i * 8:14 + i * 8], 16) for i in range(13)]
        except ValueError as error:
            raise PackagingError("invalid-rpm-payload-header") from error
        mode, size, name_size = fields[1], fields[6], fields[11]
        offset += 110
        if name_size <= 0 or len(data[offset:offset + name_size]) != name_size or data[offset + name_size - 1] != 0:
            raise PackagingError("invalid-rpm-payload-name")
        try:
            name = data[offset:offset + name_size].rstrip(b"\0").decode("utf8")
        except UnicodeDecodeError as error:
            raise PackagingError("invalid-rpm-payload-name") from error
        offset = (offset + name_size + 3) & ~3
        body = data[offset:offset + size]
        if len(body) != size:
            raise PackagingError("invalid-rpm-payload")
        offset = (offset + size + 3) & ~3
        if name == "TRAILER!!!":
            return files
        if stat.S_ISLNK(mode):
            raise PackagingError("unreviewed-linux-payload-link")
        if stat.S_ISREG(mode):
            if name in files:
                raise PackagingError("duplicate-linux-payload-file")
            files[name] = body
    raise PackagingError("invalid-rpm-payload")


def verify_linux_payload(files: dict[str, bytes], evidence: dict) -> None:
    for name, expected in evidence.items():
        if name == "frontend-bundle-inventory.json":
            continue
        candidates = [body for path, body in files.items() if Path(path).name == name]
        if len(candidates) != 1 or hashlib.sha256(candidates[0]).hexdigest() != expected:
            raise PackagingError("linux-bundled-notices-missing-or-changed")
    executables = 0
    for name, body in files.items():
        # deb/rpm reference OS libraries: adding bundled native libraries requires
        # a separate license/source review, including LGPL relinking obligations.
        if "installer-notices/" in name or ".so" in Path(name).name or Path(name).name.lower().endswith(".dll"):
            raise PackagingError("unreviewed-linux-bundled-component")
        if body.startswith(b"\x7fELF"):
            if name.removeprefix("./") != "usr/bin/primerswitch":
                raise PackagingError("unreviewed-linux-bundled-component")
            executables += 1
    if executables != 1:
        raise PackagingError("linux-package-executable-missing-or-ambiguous")


def verify_linux_package(path: Path, bundle: str, evidence: dict, runtime_dependencies: list[str], native_rpm: dict | None = None) -> dict:
    global CURRENT_STAGE
    if bundle == "deb":
        dependencies = checked_output([executable("dpkg-deb"), "-f", str(path), "Depends"]).decode("utf8").strip()
        architecture = checked_output([executable("dpkg-deb"), "-f", str(path), "Architecture"]).decode("utf8").strip()
        if architecture != "amd64":
            raise PackagingError("linux-package-architecture-mismatch")
        payload = checked_output([executable("dpkg-deb"), "--fsys-tarfile", str(path)])
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:") as archive:
            files = {}
            for entry in archive:
                if entry.issym() or entry.islnk():
                    raise PackagingError("unreviewed-linux-payload-link")
                if entry.isfile():
                    if entry.name in files:
                        raise PackagingError("duplicate-linux-payload-file")
                    files[entry.name] = archive.extractfile(entry).read()
    else:
        rpm = executable("rpm")
        checked_output([rpm, "--checksig", "--nosignature", str(path)])
        if checked_output([rpm, "-qp", "--scripts", "--triggers", "--filetriggers", str(path)]).strip():
            raise PackagingError("unreviewed-native-rpm-scriptlets")
        dependencies = checked_output([rpm, "-qp", "--requires", str(path)]).decode("utf8").strip()
        architecture = checked_output([rpm, "-qp", "--queryformat", "%{ARCH}", str(path)]).decode("utf8").strip()
        if architecture != "x86_64":
            raise PackagingError("linux-package-architecture-mismatch")
        payload = checked_output([executable("rpm2cpio"), str(path)])
        CURRENT_STAGE = "linux-rpm-payload-parsing"
        files = cpio_files(payload)
        if not native_rpm:
            raise PackagingError("native-rpm-staging-provenance-missing")
        identity = checked_output([rpm, "-qp", "--queryformat", "%{NAME}\n%{VERSION}\n%{RELEASE}", str(path)]).decode("ascii")
        config = json.loads((DESKTOP / "src-tauri/tauri.conf.json").read_text(encoding="utf8"))
        if identity != "primer-switch\n" + config["version"] + "\n1":
            raise PackagingError("native-rpm-package-identity-mismatch")
        verify_native_rpm_payload(files, native_rpm["payloadSha256"])
        records = checked_output([rpm, "-qp", "--queryformat", "[%{FILENAMES}|%{FILEMODES:octal}|%{FILEUSERNAME}|%{FILEGROUPNAME}\n]", str(path)]).decode("utf8")
        expected_records = {"/" + name: (mode, "root", "root") for name, mode in native_rpm["payloadModes"].items()}
        actual_records = {}
        for line in records.splitlines():
            name, mode, user, group = line.split("|")
            if name in actual_records:
                raise PackagingError("duplicate-native-rpm-file-metadata")
            actual_records[name] = (int(mode, 8) & 0o7777, user, group)
        if actual_records != expected_records:
            raise PackagingError("native-rpm-file-permissions-or-ownership-changed")
    CURRENT_STAGE = "linux-runtime-dependencies-and-bundled-evidence"
    for dependency in runtime_dependencies:
        if dependency not in (dependencies.splitlines() if bundle == "rpm" else dependencies):
            raise PackagingError("linux-runtime-dependency-missing")
    verify_linux_payload(files, evidence)
    return {"architecture": architecture, "declaredRuntimeDependencies": dependencies,
            "payloadFileCount": len(files), "bundledNoticeHashesVerified": True,
            "systemLibrariesBundled": False,
            "secretServiceRequirement": "Unlocked org.freedesktop.secrets on the desktop session bus; GNOME Keyring or compatible provider. KeePassXC requires Secret Service integration enabled.",
            "installedDesktopQualified": False}



def rpm_payload_inputs(config: dict, binary: Path) -> tuple[dict[str, bytes], dict[str, int], dict[str, str]]:
    """Read only reviewed native inputs; never consume a generated/broken RPM."""
    files, modes, source_hashes = {}, {}, {}

    def source(path: Path, destination: str, mode: int = 0o644) -> bytes:
        resolved = path.resolve()
        if path.is_symlink() or not resolved.is_relative_to(ROOT.resolve()) or not resolved.is_file():
            raise PackagingError("unsafe-native-rpm-source")
        body = resolved.read_bytes()
        if destination in files:
            raise PackagingError("duplicate-native-rpm-destination")
        files[destination], modes[destination] = body, mode
        source_hashes[resolved.relative_to(ROOT.resolve()).as_posix()] = hashlib.sha256(body).hexdigest()
        return body

    source(binary, "usr/bin/primerswitch", 0o755)
    expected_resources = {
        "../../../.artifacts/THIRD_PARTY_NOTICES.txt": "THIRD_PARTY_NOTICES.txt",
        "../../../.artifacts/THIRD_PARTY_REPORT.json": "THIRD_PARTY_REPORT.json",
        "../../../.artifacts/THIRD_PARTY_METADATA.json": "THIRD_PARTY_METADATA.json",
        "../../../docs/legal/brand/Comfortaa-OFL.txt": "brand-notices/Comfortaa-OFL.txt",
        "../../../docs/legal/brand/manifest.json": "brand-notices/manifest.json",
    }
    if config["bundle"].get("resources") != expected_resources:
        raise PackagingError("unreviewed-native-rpm-resources")
    for name, destination in expected_resources.items():
        source(DESKTOP / "src-tauri" / name, "usr/lib/PrimerSwitch/" + destination)
    source(ROOT / "LICENSE", "usr/share/licenses/primer-switch/LICENSE")
    png_count = 0
    for name in config["bundle"]["icon"]:
        if not name.endswith(".png"):
            continue
        path = DESKTOP / "src-tauri" / name
        # PNG signature + first IHDR dimensions; native icon inputs are already
        # generated/reviewed, no shell command or third-party image helper needed.
        resolved = path.resolve()
        if path.is_symlink() or not resolved.is_relative_to(ROOT.resolve()) or not resolved.is_file():
            raise PackagingError("unsafe-native-rpm-source")
        body = resolved.read_bytes()
        if body[:16] != b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" or len(body) < 33:
            raise PackagingError("invalid-native-rpm-icon")
        width, height = int.from_bytes(body[16:20], "big"), int.from_bytes(body[20:24], "big")
        if width != height or width not in {32, 128, 256}:
            raise PackagingError("unreviewed-native-rpm-icon-dimensions")
        source(path, f"usr/share/icons/hicolor/{width}x{height}/apps/PrimerSwitch.png")
        png_count += 1
    if png_count != 3:
        raise PackagingError("native-rpm-icon-set-incomplete")
    # Fixed executable and desktop identity preserve launch/storage behavior.
    desktop = ("[Desktop Entry]\nType=Application\nName=PrimerSwitch\n"
               "Comment=Claude account switching and quota monitoring\n"
               "Exec=primerswitch\nIcon=PrimerSwitch\nTerminal=false\nCategories=Development;\n")
    destination = "usr/share/applications/PrimerSwitch.desktop"
    files[destination], modes[destination] = desktop.encode("utf8"), 0o644
    return files, modes, source_hashes


def native_rpm_spec(version: str, dependencies: list[str], files: dict[str, bytes]) -> str:
    # Reject spec/macro/shell injection. Requirement syntax is limited to the
    # reviewed SONAME/version capabilities and one Secret Service provider file.
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise PackagingError("unreviewed-native-rpm-version")
    for dependency in dependencies:
        if not re.fullmatch(r"(?:[A-Za-z0-9_.+-]+\.so(?:\.[0-9]+)*(?:\([A-Za-z0-9_.+-]*\))?\(64bit\)|/usr/share/dbus-1/services/org\.freedesktop\.secrets\.service)", dependency):
            raise PackagingError("unsafe-native-rpm-requirement")
    for name in files:
        if not re.fullmatch(r"usr/[A-Za-z0-9_./+-]+", name) or ".." in name.split("/"):
            raise PackagingError("unsafe-native-rpm-destination")
    requires = "\n".join("Requires: " + value for value in sorted(set(dependencies)))
    entries = "\n".join("/" + value for value in sorted(files))
    # Build-only commands unpack our regular-file-only tar. No install/uninstall
    # scriptlets, triggers, network access or user-data paths are generated.
    return f"""%global _build_id_links none
%global debug_package %{{nil}}
%global __os_install_post %{{nil}}
%global _binary_payload w6.gzdio
Name: primer-switch
Version: {version}
Release: 1
Summary: Claude account switching and quota monitoring
License: MIT
Vendor: Primer-Tech
BuildArch: x86_64
AutoReqProv: no
Source0: payload.tar
{requires}

%description
PrimerSwitch native unsigned preview. Requires an unlocked Secret Service
provider on the desktop session bus. RPM installation remains unqualified.

%prep
%build
%install
install -d "%{{buildroot}}"
tar -xf "%{{SOURCE0}}" -C "%{{buildroot}}"

%files
%defattr(-,root,root,-)
{entries}
"""


def verify_native_rpm_payload(files: dict[str, bytes], expected: dict[str, str]) -> None:
    actual = {}
    for name, body in files.items():
        normalized = name.removeprefix("./")
        if normalized in actual or normalized.startswith("/") or ".." in normalized.split("/"):
            raise PackagingError("unsafe-native-rpm-payload-path")
        actual[normalized] = hashlib.sha256(body).hexdigest()
    if actual != expected:
        raise PackagingError("native-rpm-payload-changed-after-staging")


def build_native_rpm(config: dict, dependencies: list[str], binary: Path) -> dict:
    global CURRENT_STAGE
    CURRENT_STAGE = "native-rpm-input-staging"
    files, modes, sources = rpm_payload_inputs(config, binary)
    expected = {name: hashlib.sha256(body).hexdigest() for name, body in files.items()}
    spec = native_rpm_spec(config["version"], dependencies, files)
    spec_path = ARTIFACTS / "primerswitch-preview.spec"
    if spec_path.is_symlink() or not spec_path.resolve().is_relative_to(ROOT.resolve()):
        raise PackagingError("unsafe-native-rpm-spec-path")
    spec_path.write_text(spec, encoding="utf8", newline="\n")
    rpmbuild = executable("rpmbuild")
    tool_version = checked_output([rpmbuild, "--version"]).decode("ascii").strip()
    if not re.fullmatch(r"RPM version [0-9]+(?:\.[0-9]+)+", tool_version):
        raise PackagingError("native-rpm-tool-version-undetected")
    with tempfile.TemporaryDirectory(prefix="primerswitch-rpmbuild-", dir=ARTIFACTS) as temporary:
        top = Path(temporary).resolve()
        # These paths appear in native RPM macro expansions/build-only commands.
        if any(value in str(top) for value in ('%', '"', "'", "\\", "\n", "\r", "`", "$")):
            raise PackagingError("unsafe-native-rpm-build-directory")
        for name in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS", "tmp"):
            (top / name).mkdir()
        source_tar = top / "SOURCES/payload.tar"
        with tarfile.open(source_tar, "w", format=tarfile.USTAR_FORMAT) as archive:
            for name, body in sorted(files.items()):
                entry = tarfile.TarInfo(name)
                entry.size, entry.mode, entry.uid, entry.gid = len(body), modes[name], 0, 0
                entry.uname = entry.gname = "root"
                archive.addfile(entry, io.BytesIO(body))
        source_tar_hash = hashlib.sha256(source_tar.read_bytes()).hexdigest()
        CURRENT_STAGE = "native-rpm-build"
        command([rpmbuild, "-bb", "--define", f"_topdir {top}", "--define", f"_tmppath {top / 'tmp'}", str(spec_path)])
        if spec_path.read_bytes() != spec.encode() or hashlib.sha256(source_tar.read_bytes()).hexdigest() != source_tar_hash:
            raise PackagingError("native-rpm-build-input-changed")
        outputs = list((top / "RPMS").rglob("*.rpm"))
        if len(outputs) != 1 or outputs[0].is_symlink() or not outputs[0].is_file():
            raise PackagingError("native-rpm-output-missing-or-ambiguous")
        destination = output_directory("rpm") / f"PrimerSwitch-{config['version']}-1.x86_64.rpm"
        shutil.copyfile(outputs[0], destination)
    if sources != hashes(tuple(sources), ROOT):
        raise PackagingError("native-rpm-source-changed-during-build")
    return {"producer": "native-rpmbuild", "toolVersion": tool_version,
            "specPath": spec_path.relative_to(ROOT).as_posix(), "specSha256": hashlib.sha256(spec.encode()).hexdigest(),
            "sourceTarSha256": source_tar_hash, "sourceInputSha256": sources, "payloadSha256": expected,
            "payloadModes": modes, "maintainerScriptlets": False, "elfStrippingDisabled": True}

def rpm_elf_requirements(dynamic: str, versions: str) -> list[str]:
    # RPM's Tauri bundler stores explicit capabilities; it does not infer ELF
    # requirements or parse "name >= version" strings. Bind every direct SONAME
    # and versioned-symbol requirement to the just-built x86_64 executable.
    needed = set(re.findall(r"\(NEEDED\).*?\[([^]\r\n]+)\]", dynamic))
    if not needed or any("/" in name for name in needed):
        raise PackagingError("linux-elf-dependencies-undetected")
    result = {f"{name}()(64bit)" for name in needed}
    current = None
    for line in versions.splitlines():
        match = re.search(r"File: (\S+)", line)
        if match:
            current = match.group(1)
        version = re.search(r"Name: (\S+)", line)
        if version and current:
            if current not in needed:
                raise PackagingError("linux-elf-version-provider-undetected")
            result.add(f"{current}({version.group(1)})(64bit)")
    return sorted(result)


def main() -> int:
    global CURRENT_STAGE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", choices=tuple(BUNDLE_SUFFIXES), required=True)
    parser.add_argument("--offline", action="store_true", help="use only cached Cargo/bundler dependencies")
    parser.add_argument("--prepare-only", action="store_true", help="verify notices without building an installer")
    args = parser.parse_args()
    rustc, cargo, node = (executable(name) for name in ("rustc", "cargo", "node"))
    rust_version = subprocess.run([rustc, "-vV"], cwd=ROOT, capture_output=True, encoding="utf8", check=True)
    match = re.search(r"^host: ([A-Za-z0-9_-]+)$", rust_version.stdout, re.MULTILINE)
    if not match:
        raise PackagingError("native-host-undetected")
    target = match.group(1)
    validate_host(args.bundle, target, platform.system())
    distribution = None
    if args.bundle in {"deb", "rpm"}:
        distribution = linux_build_distribution(Path("/etc/os-release").read_text(encoding="utf8"))
    installer_notices = verify_installer_notices() if args.bundle == "nsis" else None
    brand_notices = verify_brand_notices()
    overlay = preview_config(args.bundle, installer_notices)
    effective_config = effective_preview_config(args.bundle, overlay)
    verify_preview_identity(effective_config, tomllib.loads((DESKTOP / "src-tauri/Cargo.toml").read_text(encoding="utf8")))
    ARTIFACTS.mkdir(exist_ok=True)
    clear_package_outputs(args.bundle)
    command([node, str(DESKTOP / "scripts" / "build-inventory.mjs")], cwd=DESKTOP)
    fetch = [cargo, "fetch", "--locked", "--target", target]
    if args.offline:
        fetch.append("--offline")
    command(fetch)
    command([sys.executable, "-B", str(ROOT / "scripts" / "generate_notices.py"),
             "--target", target, "--overrides", "docs/legal/upstream/manifest.json",
             "--frontend-inventory", ".artifacts/frontend-bundle-inventory.json"], accepted={0, 1})
    CURRENT_STAGE = "native-attribution-and-renderer-evidence"
    report = verify_attribution(target)
    inventory = verify_bundle()
    print(f"Preview attribution verified: {report['dependencyCount']} dependency records; no missing shipped texts.", flush=True)
    if args.prepare_only:
        return 0
    evidence_hashes = hashes(EVIDENCE_FILES, ARTIFACTS)
    config_hashes = hashes(CONFIG_FILES, ROOT)
    notices_hash = hashes(("docs/legal/packaging/manifest.json",), ROOT)["docs/legal/packaging/manifest.json"] if installer_notices else None
    overlay = preview_config(args.bundle, installer_notices)
    arguments = [node, str(DESKTOP / "node_modules" / "@tauri-apps" / "cli" / "tauri.js"),
                 "build", "--bundles", args.bundle, "--config", "src-tauri/tauri.preview.conf.json",
                 "--config", json.dumps(overlay), "--", "--locked", "--target-dir", str(ROOT / "target")]
    if args.offline:
        arguments.append("--offline")
    config = json.loads((DESKTOP / "src-tauri/tauri.conf.json").read_text(encoding="utf8"))
    runtime_dependencies = config["bundle"]["linux"].get(args.bundle, {}).get("depends", [])
    native_rpm = None
    if args.bundle == "rpm":
        # Build first so RPM requirements come from the same successful native
        # executable that will be packaged. No pre-existing binary is trusted.
        arguments[arguments.index("--bundles"):arguments.index("--bundles") + 2] = ["--no-bundle"]
        command(arguments, cwd=DESKTOP)
        binary = ROOT / "target/release/primerswitch"
        dynamic = checked_output([executable("readelf"), "--dynamic", str(binary)]).decode("utf8")
        versions = checked_output([executable("readelf"), "--version-info", str(binary)]).decode("utf8")
        runtime_dependencies = sorted(set(runtime_dependencies + rpm_elf_requirements(dynamic, versions)))
        overlay["bundle"]["linux"] = {"rpm": {"depends": runtime_dependencies}}
        native_rpm = build_native_rpm(effective_config, runtime_dependencies, binary)
    else:
        command(arguments, cwd=DESKTOP)
    if args.bundle == "nsis":
        verify_installer_notices()
        verify_nsis_recipe(target, installer_notices, list(effective_config["bundle"]["resources"].values()))
        if notices_hash != hashes(("docs/legal/packaging/manifest.json",), ROOT)["docs/legal/packaging/manifest.json"]:
            raise PackagingError("installer-attribution-changed-during-build")
    verify_attribution(target)
    verify_brand_notices()
    inventory = verify_bundle()
    if evidence_hashes != hashes(EVIDENCE_FILES, ARTIFACTS) or config_hashes != hashes(CONFIG_FILES, ROOT):
        raise PackagingError("packaging-evidence-changed-during-build")
    paths = sorted(output_directory(args.bundle).glob(BUNDLE_SUFFIXES[args.bundle]))
    if len(paths) != 1 or paths[0].is_symlink() or not paths[0].is_file():
        raise PackagingError("installer-output-missing-or-ambiguous")
    CURRENT_STAGE = "package-provenance-assembly"
    packages = [{"path": path.relative_to(ROOT).as_posix(), "bytes": path.stat().st_size,
                 "sha256": hashlib.sha256(path.read_bytes()).hexdigest()} for path in paths]
    manifest = {"formatVersion": 2, "target": target, "bundle": args.bundle, "unsignedPreview": True,
                "installerNoticesSha256": notices_hash, "packages": packages,
                "rendererPackages": [{"name": p["name"], "version": p["version"]} for p in inventory["packages"]],
                "evidenceSha256": evidence_hashes, "packagingInputSha256": config_hashes,
                "brandAttributionSha256": {"font": brand_notices["font"]["sha256"], "license": brand_notices["license"]["sha256"]},
                "bundleConfigOverlay": overlay, "buildHost": {"system": platform.system(), "release": platform.release()},
                "sourceCommit": checked_output([executable("git"), "rev-parse", "HEAD"]).decode("ascii").strip()}
    if args.bundle in {"deb", "rpm"}:
        bundled_evidence = evidence_hashes | {"Comfortaa-OFL.txt": brand_notices["license"]["sha256"],
                                               "manifest.json": config_hashes["docs/legal/brand/manifest.json"]}
        manifest["linuxPackageVerification"] = verify_linux_package(paths[0], args.bundle, bundled_evidence, runtime_dependencies, native_rpm)
        if native_rpm:
            manifest["nativeRpmBuild"] = native_rpm
        manifest["buildHost"]["distribution"] = distribution
        manifest["linuxBuildBaseline"] = "Ubuntu 24.04 x86_64 / glibc >= 2.39; RPM install compatibility requires separate native qualification"
    (ARTIFACTS / "package-manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf8")
    (ARTIFACTS / "SHA256SUMS.txt").write_text("".join(f"{p['sha256']}  {Path(p['path']).name}\n" for p in packages), encoding="utf8")
    for package in packages:
        print(f"Preview package: {package['path']} ({package['bytes']} bytes), SHA256 {package['sha256']}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (PackagingError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        message = str(error) if isinstance(error, PackagingError) else "packaging-could-not-complete"
        ARTIFACTS.mkdir(exist_ok=True)
        failure = {"formatVersion": 1, "failureStep": CURRENT_STAGE, "exceptionType": type(error).__name__,
                   "reason": message}
        (ARTIFACTS / FAILURE_REPORT).write_text(json.dumps(failure, indent=2, sort_keys=True) + "\n", encoding="utf8")
        print(f"Preview packaging failed: {message} (step: {CURRENT_STAGE}; type: {type(error).__name__}).", file=sys.stderr)
        sys.exit(2)
