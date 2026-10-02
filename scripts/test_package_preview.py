#!/usr/bin/env python3
"""Hermetic negative fixtures for preview packaging; never builds or opens a vault."""
from __future__ import annotations
import hashlib
import io
import tarfile
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import package_preview as packaging
import qualify_linux_preview as qualification

TARGET = "x86_64-unknown-linux-gnu"


def digest(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


class PackagingFixtures(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="primerswitch-package-fixture-")
        self.root = Path(self.temp.name).resolve()
        self.artifacts = self.root / ".artifacts"
        self.desktop = self.root / "apps/desktop"
        self.artifacts.mkdir()
        (self.desktop / "dist").mkdir(parents=True)
        self.patches = [patch.object(packaging, "ROOT", self.root),
                        patch.object(packaging, "ARTIFACTS", self.artifacts),
                        patch.object(packaging, "DESKTOP", self.desktop)]
        for value in self.patches:
            value.start()
        self.addCleanup(self.temp.cleanup)
        for value in self.patches:
            self.addCleanup(value.stop)
        (self.root / "Cargo.lock").write_text('[[package]]\nname = "target-lexicon"\nversion = "0.12.16"\nchecksum = "' + "b" * 64 + '"\n')
        (self.artifacts / "THIRD_PARTY_NOTICES.txt").write_text("Exact fixture license text")
        self.metadata = {"target": TARGET, "inputSha256": {"Cargo.lock": digest((self.root / "Cargo.lock").read_bytes())}}
        self.report = {"target": TARGET, "missingLicenseTextCount": 0, "dependencyCount": 1, "findings": []}
        self.write_evidence()

    def write_evidence(self):
        for name, body in (("THIRD_PARTY_METADATA.json", self.metadata), ("THIRD_PARTY_REPORT.json", self.report)):
            (self.artifacts / name).write_text(json.dumps(body))

    def rejects(self, message, action):
        with self.assertRaisesRegex(packaging.PackagingError, "^" + message + "$"):
            action()

    def test_native_hosts_are_required_even_with_linux_named_rust_toolchain(self):
        for bundle, target, system in (("deb", TARGET, "Windows"), ("rpm", TARGET, "Darwin"),
                                      ("nsis", TARGET, "Linux"), ("dmg", TARGET, "Linux")):
            with self.subTest(bundle=bundle):
                self.rejects("bundle-requires-matching-native-host", lambda: packaging.validate_host(bundle, target, system))
        packaging.validate_host("deb", TARGET, "Linux")
        packaging.validate_host("rpm", TARGET, "Linux")
        packaging.validate_host("nsis", "x86_64-pc-windows-msvc", "Windows")
        packaging.validate_host("dmg", "aarch64-apple-darwin", "Darwin")

    def test_linux_arm64_is_not_accidentally_claimed_qualified(self):
        self.rejects("linux-preview-requires-x86_64-gnu-host", lambda: packaging.validate_host("deb", "aarch64-unknown-linux-gnu", "Linux"))

    def test_attribution_containment_uses_the_same_canonical_root_as_its_inputs(self):
        # A non-canonical root models runner temp aliases (/var -> /private/var,
        # Windows junctions) without depending on privileged symlink creation.
        root_alias = self.root / "apps" / ".."
        with patch.object(packaging, "ROOT", root_alias):
            packaging.verify_attribution(TARGET)
            self.metadata["inputSha256"] = {"../outside": "a" * 64}
            self.write_evidence()
            self.rejects("unsafe-attribution-input", lambda: packaging.verify_attribution(TARGET))

    def test_contained_input_symlinks_are_still_rejected_after_root_normalization(self):
        link = self.root / "linked-Cargo.lock"
        try:
            link.symlink_to(self.root / "Cargo.lock")
        except (OSError, NotImplementedError):
            self.skipTest("This host does not permit fixture symlink creation")
        self.metadata["inputSha256"] = {link.name: digest((self.root / "Cargo.lock").read_bytes())}
        self.write_evidence()
        with patch.object(packaging, "ROOT", self.root / "apps" / ".."):
            self.rejects("unsafe-attribution-input", lambda: packaging.verify_attribution(TARGET))

    def test_cross_target_metadata_cannot_be_reused(self):
        self.metadata["target"] = "x86_64-pc-windows-msvc"
        self.write_evidence()
        self.rejects("incomplete-native-attribution", lambda: packaging.verify_attribution(TARGET))

    def test_missing_full_license_text_stops_packaging(self):
        self.report["missingLicenseTextCount"] = 1
        self.write_evidence()
        self.rejects("incomplete-native-attribution", lambda: packaging.verify_attribution(TARGET))

    def test_dependency_change_invalidates_attribution(self):
        (self.root / "Cargo.lock").write_text("changed dependency tree")
        self.rejects("dependency-input-changed-after-attribution", lambda: packaging.verify_attribution(TARGET))

    def test_evidence_paths_cannot_escape_checkout(self):
        self.metadata["inputSha256"] = {"../outside": "a" * 64}
        self.write_evidence()
        self.rejects("unsafe-attribution-input", lambda: packaging.verify_attribution(TARGET))

    def reviewed_finding(self):
        return {"check": "license-expression-review", "ecosystem": "cargo", "name": "target-lexicon",
                "version": "0.12.16", "declaredLicense": "Apache-2.0 WITH LLVM-exception",
                "sourceIntegrity": {"status": "verified", "archiveSha256": "b" * 64,
                                    "archiveUrl": "https://static.crates.io/crates/target-lexicon/target-lexicon-0.12.16.crate",
                                    "upstreamRevision": "fixture-pinned-revision", "verifiedSourceFileCount": 22}}

    def test_reviewed_expression_requires_the_exact_locked_archive(self):
        self.report["findings"] = [self.reviewed_finding()]
        self.write_evidence()
        packaging.verify_attribution(TARGET)
        self.report["findings"][0]["sourceIntegrity"]["archiveSha256"] = "c" * 64
        self.write_evidence()
        self.rejects("unresolved-attribution-finding", lambda: packaging.verify_attribution(TARGET))

    def test_unknown_expression_or_changed_version_still_fails(self):
        for key, value in (("declaredLicense", "GPL-3.0-only"), ("version", "0.12.17"), ("name", "new-crate")):
            finding = self.reviewed_finding()
            finding[key] = value
            self.report["findings"] = [finding]
            self.write_evidence()
            self.rejects("unresolved-attribution-finding", lambda: packaging.verify_attribution(TARGET))

    def renderer(self):
        body = b"fixture renderer"
        (self.desktop / "dist/index.html").write_bytes(body)
        inventory = {"chunks": [], "assets": [{"file": "index.html", "sha256": digest(body)}]}
        (self.artifacts / "frontend-bundle-inventory.json").write_text(json.dumps(inventory))
        return inventory

    def test_renderer_changed_after_attribution_is_rejected(self):
        self.renderer()
        (self.desktop / "dist/index.html").write_text("changed rendered output")
        self.rejects("renderer-changed-after-attribution", packaging.verify_bundle)

    def test_uninventoried_output_and_duplicate_records_are_rejected(self):
        inventory = self.renderer()
        (self.desktop / "dist/unlisted.js").write_text("unattributed code")
        self.rejects("renderer-inventory-incomplete", packaging.verify_bundle)
        (self.desktop / "dist/unlisted.js").unlink()
        inventory["assets"].append(inventory["assets"][0])
        (self.artifacts / "frontend-bundle-inventory.json").write_text(json.dumps(inventory))
        self.rejects("invalid-renderer-output", packaging.verify_bundle)

    def test_previous_packages_are_removed_before_build(self):
        directory = self.root / "target/release/bundle/deb"
        directory.mkdir(parents=True)
        (directory / "old.deb").write_bytes(b"previously successful build")
        (directory / "diagnostic.txt").write_text("keep other evidence")
        (self.artifacts / "package-manifest.json").write_text("old manifest")
        (self.artifacts / "SHA256SUMS.txt").write_text("old hashes")
        packaging.clear_package_outputs("deb")
        self.assertEqual(list(directory.glob("*.deb")), [])
        self.assertTrue((directory / "diagnostic.txt").exists())
        self.assertFalse((self.artifacts / "package-manifest.json").exists())
        self.assertFalse((self.artifacts / "SHA256SUMS.txt").exists())

    def test_linux_and_mac_resources_do_not_include_windows_components(self):
        installer = {"licenses": [{"file": "docs/legal/packaging/NSIS-COPYING.txt"}]}
        for bundle in ("deb", "rpm", "dmg"):
            self.assertEqual(packaging.preview_config(bundle, None)["bundle"]["resources"], {})
        self.assertIn("../../../docs/legal/packaging/NSIS-COPYING.txt", packaging.preview_config("nsis", installer)["bundle"]["resources"])

    def payload(self):
        bodies = {name: (self.artifacts / name).read_bytes() for name in packaging.EVIDENCE_FILES if (self.artifacts / name).exists()}
        files = {"./usr/lib/PrimerSwitch/" + name: body for name, body in bodies.items()}
        files["./usr/bin/primerswitch"] = b"\x7fELFfixture executable"
        return files, {name: digest(body) for name, body in bodies.items()}

    def test_linux_package_requires_unchanged_embedded_license_evidence(self):
        files, evidence = self.payload()
        packaging.verify_linux_payload(files, evidence)
        files["./usr/lib/PrimerSwitch/THIRD_PARTY_NOTICES.txt"] = b"license removed"
        self.rejects("linux-bundled-notices-missing-or-changed", lambda: packaging.verify_linux_payload(files, evidence))

    def test_linux_os_library_and_unreviewed_sidecar_bundling_is_rejected(self):
        for path, body in (("./usr/lib/PrimerSwitch/libwebkit2gtk.so.0", b"library"),
                           ("./usr/lib/PrimerSwitch/installer-notices/NSIS-COPYING.txt", b"windows"),
                           ("./usr/bin/unreviewed-helper", b"\x7fELFhelper")):
            files, evidence = self.payload()
            files[path] = body
            self.rejects("unreviewed-linux-bundled-component", lambda: packaging.verify_linux_payload(files, evidence))

    def test_rpm_dependencies_derive_direct_sonames_and_symbol_versions(self):
        dynamic = " 0x01 (NEEDED) Shared library: [libc.so.6]\n 0x01 (NEEDED) Shared library: [libdbus-1.so.3]"
        versions = "0x10: Version: 1 File: libc.so.6 Cnt: 2\n0x20: Name: GLIBC_2.39 Flags: none\n0x30: Name: GLIBC_2.34 Flags: none"
        self.assertEqual(packaging.rpm_elf_requirements(dynamic, versions),
                         ["libc.so.6()(64bit)", "libc.so.6(GLIBC_2.34)(64bit)", "libc.so.6(GLIBC_2.39)(64bit)", "libdbus-1.so.3()(64bit)"])
        self.rejects("linux-elf-dependencies-undetected", lambda: packaging.rpm_elf_requirements("", versions))
        self.rejects("linux-elf-version-provider-undetected", lambda: packaging.rpm_elf_requirements(dynamic, "File: unknown.so\nName: UNREVIEWED_1"))

    def test_site_font_and_full_license_bytes_are_both_pinned(self):
        manifest = {"font": {"file": "apps/desktop/src/assets/fonts/comfortaa-600.subset.woff2", "sha256": digest(b"original font")},
                    "license": {"file": "docs/legal/brand/Comfortaa-OFL.txt", "sha256": digest(b"full OFL"), "spdx": "OFL-1.1"}}
        for key, body in (("font", b"original font"), ("license", b"full OFL")):
            path = self.root / manifest[key]["file"]
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(body)
        (self.root / "docs/legal/brand/manifest.json").write_text(json.dumps(manifest))
        packaging.verify_brand_notices()
        (self.root / manifest["font"]["file"]).write_bytes(b"modified subset")
        self.rejects("brand-font-or-license-changed", packaging.verify_brand_notices)

    def test_linux_build_baseline_is_recorded_from_the_actual_host(self):
        self.assertEqual(packaging.linux_build_distribution('ID=ubuntu\nVERSION_ID="24.04"\nPRETTY_NAME="Ubuntu 24.04"')["VERSION_ID"], "24.04")
        for text in ('ID=ubuntu\nVERSION_ID="22.04"', 'ID=fedora\nVERSION_ID="44"', ''):
            self.rejects("linux-preview-requires-ubuntu-24.04-build-host", lambda: packaging.linux_build_distribution(text))

    def test_deb_archive_is_checked_for_required_provider_and_actual_embedded_notices(self):
        files, evidence = self.payload()
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:") as archive:
            for name, body in files.items():
                entry = tarfile.TarInfo(name)
                entry.size = len(body)
                archive.addfile(entry, io.BytesIO(body))
        dependencies = ["libwebkit2gtk-4.1-0", "gnome-keyring | keepassxc"]
        def tool_output(arguments):
            if "--fsys-tarfile" in arguments: return output.getvalue()
            if arguments[-1] == "Architecture": return b"amd64"
            return b"libwebkit2gtk-4.1-0, gnome-keyring | keepassxc"
        with patch.object(packaging, "checked_output", side_effect=tool_output), patch.object(packaging, "executable", side_effect=lambda name: name):
            result = packaging.verify_linux_package(self.root / "fixture.deb", "deb", evidence, dependencies)
            self.assertFalse(result["installedDesktopQualified"])
            self.rejects("linux-runtime-dependency-missing", lambda: packaging.verify_linux_package(self.root / "fixture.deb", "deb", evidence, dependencies + ["missing-secret-service"]))

    def test_valid_rpm_payload_can_be_read_without_filesystem_extraction(self):
        def entry(name, body, mode):
            name_bytes = name.encode() + b"\0"
            fields = [1, mode, 0, 0, 1, 0, len(body), 0, 0, 0, 0, len(name_bytes), 0]
            record = b"070701" + b"".join(f"{field:08x}".encode() for field in fields) + name_bytes
            record += b"\0" * (-len(record) % 4)
            record += body
            record += b"\0" * (-len(record) % 4)
            return record
        payload = entry("./usr/lib/PrimerSwitch/notice.txt", b"original notice", 0o100644)
        payload += entry("TRAILER!!!", b"", 0)
        self.assertEqual(packaging.cpio_files(payload), {"./usr/lib/PrimerSwitch/notice.txt": b"original notice"})
        link = entry("./usr/lib/PrimerSwitch/lib.so", b"/system/lib.so", 0o120777)
        self.rejects("unreviewed-linux-payload-link", lambda: packaging.cpio_files(link + payload))

    def test_installed_qualification_refuses_non_ephemeral_or_wrong_platform_hosts(self):
        env = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted"}
        release = 'ID=ubuntu\nVERSION_ID="24.04"'
        qualification.validate_ci_host("Linux", "x86_64", env, release)
        for system, machine, environment, release_text in (("Windows", "AMD64", env, release),
                                                          ("Linux", "aarch64", env, release),
                                                          ("Linux", "x86_64", {}, release),
                                                          ("Linux", "x86_64", env | {"RUNNER_ENVIRONMENT": "self-hosted"}, release),
                                                          ("Linux", "x86_64", env, 'ID=ubuntu\nVERSION_ID="22.04"')):
            with self.assertRaises(qualification.QualificationError):
                qualification.validate_ci_host(system, machine, environment, release_text)

    def installed_manifest(self):
        package = self.root / "target/release/bundle/deb/fixture.deb"
        package.parent.mkdir(parents=True)
        package.write_bytes(b"own fixture package")
        config = self.root / "apps/desktop/src-tauri/tauri.conf.json"
        config.parent.mkdir(parents=True)
        config.write_text("fixture config")
        return {"unsignedPreview": True, "bundle": "deb", "target": TARGET,
                "packages": [{"path": package.relative_to(self.root).as_posix(), "bytes": package.stat().st_size, "sha256": digest(package.read_bytes())}],
                "packagingInputSha256": {config.relative_to(self.root).as_posix(): digest(config.read_bytes())},
                "evidenceSha256": {"THIRD_PARTY_NOTICES.txt": digest((self.artifacts / "THIRD_PARTY_NOTICES.txt").read_bytes())},
                "linuxPackageVerification": {"bundledNoticeHashesVerified": True, "systemLibrariesBundled": False}}

    def test_installed_qualification_never_trusts_replaced_packages_or_changed_build_inputs(self):
        manifest = self.installed_manifest()
        package = qualification.verify_package_manifest(manifest, self.root)
        package.write_bytes(b"replaced package")
        with self.assertRaisesRegex(qualification.QualificationError, "preview-package-hash-mismatch"):
            qualification.verify_package_manifest(manifest, self.root)
        package.write_bytes(b"own fixture package")
        (self.root / "apps/desktop/src-tauri/tauri.conf.json").write_text("changed source config")
        with self.assertRaisesRegex(qualification.QualificationError, "preview-input-changed"):
            qualification.verify_package_manifest(manifest, self.root)

    def test_installed_qualification_rejects_manifest_path_escape(self):
        manifest = self.installed_manifest()
        manifest["packages"][0]["path"] = "../foreign.deb"
        with self.assertRaisesRegex(qualification.QualificationError, "unsafe-preview-package-path"):
            qualification.verify_package_manifest(manifest, self.root)

    def test_accessibility_observation_dispatches_registration_events_before_reading_children(self):
        class States:
            def contains(self, state): return True
        class Node:
            def __init__(self, name): self.name, self.children, self.cleared = name, [], False
            @property
            def childCount(self): return len(self.children)
            def getRoleName(self): return "application"
            def getState(self): return States()
            def queryText(self): raise NotImplementedError
            def getChildAtIndex(self, index): return self.children[index]
            def clearCache(self): self.cleared = True
        desktop, application = Node("fixture desktop"), Node("PrimerSwitch")
        class Context:
            def __init__(self): self.events = [lambda: desktop.children.append(application)]
            def pending(self): return bool(self.events)
            def iteration(self, blocking):
                self.assertNonBlocking = not blocking
                self.events.pop(0)()
        context = Context()
        spi = SimpleNamespace(STATE_ENABLED=1, Registry=SimpleNamespace(getDesktop=lambda index: desktop))
        glib = SimpleNamespace(MainContext=SimpleNamespace(default=lambda: context))
        rows = qualification.observe_accessibility_desktop(spi, glib)
        self.assertEqual([row["name"] for row in rows], ["fixture desktop", "PrimerSwitch"])
        self.assertTrue(desktop.cleared)
        self.assertTrue(context.assertNonBlocking)

    def stable_identity(self):
        return {"productName": "PrimerSwitch", "identifier": "com.primertech.primerswitch", "version": "0.1.1",
                "bundle": {"publisher": "Primer-Tech", "windows": {"allowDowngrades": False, "nsis": {"installMode": "currentUser"}},
                           "resources": {"source-license.txt": "installer-notices/LICENSE.txt"}}}

    def test_upgrade_version_progression_preserves_storage_identity_and_rejects_custom_hooks(self):
        config = self.stable_identity()
        cargo = {"package": {"default-run": "primerswitch"}, "bin": [{"name": "primerswitch"}]}
        packaging.verify_preview_identity(config, cargo)
        config["version"] = "0.1.2"
        packaging.verify_preview_identity(config, cargo)
        config["bundle"]["windows"]["nsis"]["installerHooks"] = "custom-hooks.nsh"
        self.rejects("unreviewed-nsis-template-or-hooks", lambda: packaging.verify_preview_identity(config, cargo))
        del config["bundle"]["windows"]["nsis"]["installerHooks"]
        config["identifier"] = "com.primertech.primerswitch2"
        self.rejects("preview-identity-would-change-existing-installation", lambda: packaging.verify_preview_identity(config, cargo))

    def nsis_cleanup_fixture(self):
        identities = {"PRODUCTNAME": "PrimerSwitch", "BUNDLEID": "com.primertech.primerswitch", "MANUFACTURER": "Primer-Tech", "INSTALLMODE": "currentUser", "MAINBINARYNAME": "primerswitch"}
        return "\n".join(f'!define {name} "{value}"' for name, value in identities.items()) + "\n" + r'''
Delete "$INSTDIR\${MAINBINARYNAME}.exe"
Delete "$INSTDIR\installer-notices\LICENSE.txt"
RMDir /REBOOTOK "$INSTDIR\installer-notices"
RMDir "$INSTDIR"
${If} $DeleteAppDataCheckboxState = 1
${AndIf} $UpdateMode <> 1
RmDir /r "$APPDATA\${BUNDLEID}"
RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
${EndIf}
'''

    def test_installer_resource_cleanup_preserves_co_located_settings_accounts_and_key_files(self):
        baseline = self.nsis_cleanup_fixture()
        destinations = ["installer-notices/LICENSE.txt"]
        packaging.verify_nsis_data_preservation(baseline, destinations)
        for harmful in (r'RMDir /r "$INSTDIR"', r'Delete "$INSTDIR"',
                        r'Delete "$INSTDIR\runtime-state.vault"', r'Delete "$INSTDIR\*.vault"',
                        r'Delete "$INSTDIR\master-key.dpapi"', r'RMDir /r "$LOCALAPPDATA\PrimerSwitch"',
                        r'Delete "$INSTDIR\custom-data.json"'):
            with self.subTest(cleanup=harmful):
                with self.assertRaises(packaging.PackagingError):
                    packaging.verify_nsis_data_preservation(baseline + "\n" + harmful + "\n", destinations)
        self.rejects("installer-app-data-cleanup-not-update-guarded", lambda: packaging.verify_nsis_data_preservation(baseline.replace('${AndIf} $UpdateMode <> 1', ''), destinations))

    def test_malformed_rpm_payload_is_rejected_without_extraction(self):
        self.rejects("invalid-rpm-payload", lambda: packaging.cpio_files(b"untrusted payload"))


if __name__ == "__main__":
    unittest.main()
