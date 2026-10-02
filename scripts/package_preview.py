#!/usr/bin/env python3
"""Build an unsigned native preview with verified, bundled dependency notices."""
from __future__ import annotations
import argparse
import hashlib
import json
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
        path = (ROOT / name).resolve()
        if not path.is_relative_to(ROOT) or path.is_symlink() or not re.fullmatch(r"[a-f0-9]{64}", expected):
            raise PackagingError("unsafe-attribution-input")
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise PackagingError("dependency-input-changed-after-attribution")
    report = load_json("THIRD_PARTY_REPORT.json")
    if report.get("target") != target or report.get("missingLicenseTextCount") != 0:
        raise PackagingError("incomplete-native-attribution")
    for finding in report.get("findings", []):
        name = finding.get("name")
        proof = finding.get("sourceIntegrity", {})
        # Exact unmodified MPL dependencies have full texts and checksum-
        # verified original source links in the shipped notices.
        if (
            finding.get("check") != "license-expression-review"
            or finding.get("ecosystem") != "cargo"
            or MPL_VERSIONS.get(name) != finding.get("version")
            or finding.get("declaredLicense") != "MPL-2.0"
            or proof.get("status") != "verified"
            or not re.fullmatch(r"[a-f0-9]{64}", proof.get("archiveSha256", ""))
            or not proof.get("archiveUrl", "").startswith("https://static.crates.io/crates/")
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
        path = (out_dir / record["file"]).resolve()
        if not path.is_relative_to(out_dir.resolve()) or not path.is_file() or path.is_symlink():
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
        path = (ROOT / license_file["file"]).resolve()
        if not path.is_relative_to((ROOT / "docs/legal/packaging").resolve()) or path.is_symlink():
            raise PackagingError("unsafe-installer-notice")
        if hashlib.sha256(path.read_bytes()).hexdigest() != license_file["sha256"]:
            raise PackagingError("changed-installer-notice")
    return manifest


def verify_nsis_recipe(target: str, notices: dict) -> None:
    arch = {"x86_64": "x64", "aarch64": "arm64", "i686": "x86"}.get(target.split("-")[0])
    if not arch:
        raise PackagingError("unsupported-nsis-architecture")
    recipe = ROOT / "target" / "release" / "nsis" / arch / "installer.nsi"
    text = recipe.read_text(encoding="utf8")
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", choices=("nsis", "dmg"), required=True)
    parser.add_argument("--offline", action="store_true", help="use only cached Cargo/bundler dependencies")
    parser.add_argument("--prepare-only", action="store_true", help="verify notices without building an installer")
    args = parser.parse_args()
    rustc, cargo, node = (executable(name) for name in ("rustc", "cargo", "node"))
    rust_version = subprocess.run([rustc, "-vV"], cwd=ROOT, capture_output=True, encoding="utf8", check=True)
    match = re.search(r"^host: ([A-Za-z0-9_-]+)$", rust_version.stdout, re.MULTILINE)
    if not match:
        raise PackagingError("native-host-undetected")
    target = match.group(1)
    installer_notices = verify_installer_notices()
    required = "pc-windows-msvc" if args.bundle == "nsis" else "apple-darwin"
    if not target.endswith(required):
        raise PackagingError("bundle-requires-matching-native-host")
    for stale in ("package-manifest.json", "SHA256SUMS.txt"):
        (ARTIFACTS / stale).unlink(missing_ok=True)
    command([node, str(DESKTOP / "scripts" / "build-inventory.mjs")], cwd=DESKTOP)
    fetch = [cargo, "fetch", "--locked", "--target", target]
    if args.offline:
        fetch.append("--offline")
    command(fetch)
    command(
        [
            sys.executable, "-B", str(ROOT / "scripts" / "generate_notices.py"),
            "--target", target,
            "--overrides", "docs/legal/upstream/manifest.json",
            "--frontend-inventory", ".artifacts/frontend-bundle-inventory.json",
        ],
        accepted={0, 1},
    )
    report = verify_attribution(target)
    inventory = verify_bundle()
    print(f"Preview attribution verified: {report['dependencyCount']} dependency records; no missing shipped texts.", flush=True)
    if args.prepare_only:
        return 0
    evidence_hashes = {name: hashlib.sha256((ARTIFACTS / name).read_bytes()).hexdigest() for name in EVIDENCE_FILES}
    notices_hash = hashlib.sha256((ROOT / "docs/legal/packaging/manifest.json").read_bytes()).hexdigest()
    arguments = [
        node, str(DESKTOP / "node_modules" / "@tauri-apps" / "cli" / "tauri.js"),
        "build", "--bundles", args.bundle,
        "--config", "src-tauri/tauri.preview.conf.json",
        "--", "--locked",
    ]
    if args.offline:
        arguments.append("--offline")
    command(arguments, cwd=DESKTOP)
    verify_installer_notices()
    if args.bundle == "nsis":
        verify_nsis_recipe(target, installer_notices)
    verify_attribution(target)
    inventory = verify_bundle()
    if evidence_hashes != {name: hashlib.sha256((ARTIFACTS / name).read_bytes()).hexdigest() for name in EVIDENCE_FILES}:
        raise PackagingError("attribution-evidence-changed-during-build")
    if notices_hash != hashlib.sha256((ROOT / "docs/legal/packaging/manifest.json").read_bytes()).hexdigest():
        raise PackagingError("installer-attribution-changed-during-build")
    suffix = "*.exe" if args.bundle == "nsis" else "*.dmg"
    paths = sorted((ROOT / "target" / "release" / "bundle" / args.bundle).glob(suffix))
    if not paths:
        raise PackagingError("installer-output-missing")
    packages = [
        {
            "path": path.relative_to(ROOT).as_posix(),
            "bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
        for path in paths
    ]
    manifest = {
        "formatVersion": 1,
        "target": target,
        "bundle": args.bundle,
        "unsignedPreview": True,
        "installerNoticesSha256": notices_hash,
        "packages": packages,
        "rendererPackages": [
            {"name": package["name"], "version": package["version"]}
            for package in inventory["packages"]
        ],
        "evidenceSha256": evidence_hashes,
    }
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
        print(f"Preview packaging failed: {message}.", file=sys.stderr)
        sys.exit(2)
