#!/usr/bin/env python3
"""Hermetic Windows update qualification guards; never execute an installer."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import qualify_windows_update as qualification


class GuardTests(unittest.TestCase):
    def test_only_ephemeral_hosted_windows_x64_is_allowed(self):
        hosted = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted"}
        qualification.validate_ci_host("Windows", "AMD64", hosted)
        for system, machine, environment in [
            ("Linux", "x86_64", hosted), ("Windows", "ARM64", hosted),
            ("Windows", "AMD64", {}),
            ("Windows", "AMD64", {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "self-hosted"}),
        ]:
            with self.assertRaises(qualification.QualificationError):
                qualification.validate_ci_host(system, machine, environment)

    def test_personal_host_default_refuses_before_any_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory) / "uncreated-artifacts"
            with patch.object(qualification, "ARTIFACTS", artifacts), patch.object(qualification, "REPORT", artifacts / "report.json"), patch.object(qualification.platform, "system", return_value="Windows"), patch.object(qualification.platform, "machine", return_value="AMD64"), patch.dict(qualification.os.environ, {}, clear=True), patch.object(qualification.sys, "argv", ["qualify_windows_update.py"]), patch.object(qualification, "run_silent") as installer, patch.object(qualification, "registry_state") as registry, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(qualification.main(), 1)
            installer.assert_not_called()
            registry.assert_not_called()
            self.assertFalse(artifacts.exists())

    def test_existing_registration_is_never_adopted(self):
        qualification.require_clean_registration({})
        for name in qualification.REGISTRY_KEYS:
            with self.assertRaisesRegex(qualification.QualificationError, "existing"):
                qualification.require_clean_registration({name: {}})

    def fixture_package(self, root):
        package = root / "target/release/bundle/nsis/PrimerSwitch_0.1.1_x64-setup.exe"
        package.parent.mkdir(parents=True)
        package.write_bytes(b"MZ hermetic fixture installer never executed")
        binary = root / "target/release/primerswitch.exe"
        binary.write_bytes(b"MZ hermetic fixture application never executed")
        config = root / "apps/desktop/src-tauri/tauri.conf.json"
        config.parent.mkdir(parents=True)
        config.write_text(json.dumps({"productName": "PrimerSwitch", "identifier": "com.primertech.primerswitch", "version": "0.1.1", "bundle": {"publisher": "Primer-Tech", "windows": {"nsis": {"installMode": "currentUser"}}}}), encoding="utf8")
        evidence = root / ".artifacts/evidence.json"
        evidence.parent.mkdir()
        evidence.write_bytes(b"fixture evidence")
        manifest = {"formatVersion": 2, "bundle": "nsis", "target": "x86_64-pc-windows-msvc", "unsignedPreview": True, "sourceCommit": "a" * 40,
                    "packages": [{"path": package.relative_to(root).as_posix(), "bytes": package.stat().st_size, "sha256": qualification.sha256(package.read_bytes())}],
                    "evidenceSha256": {"evidence.json": qualification.sha256(evidence.read_bytes())},
                    "packagingInputSha256": {config.relative_to(root).as_posix(): qualification.sha256(config.read_bytes())}}
        return manifest, package, binary, evidence

    def test_exact_manifest_inputs_are_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest, package, binary, _ = self.fixture_package(root)
            self.assertEqual(qualification.verify_package_manifest(manifest, root, "a" * 40), (package, binary))

    def test_installer_hash_source_commit_and_evidence_mutation_fail(self):
        for mutation in ("installer", "source", "evidence"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                manifest, package, _, evidence = self.fixture_package(root)
                if mutation == "installer":
                    package.write_bytes(b"MZ changed installer")
                elif mutation == "source":
                    manifest["sourceCommit"] = "b" * 40
                else:
                    evidence.write_bytes(b"changed evidence")
                with self.assertRaises(qualification.QualificationError):
                    qualification.verify_package_manifest(manifest, root, "a" * 40)

    def test_manifest_cannot_select_an_outside_installer(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest, _, _, _ = self.fixture_package(root)
            manifest["packages"][0]["path"] = "../outside.exe"
            with self.assertRaises(qualification.QualificationError):
                qualification.verify_package_manifest(manifest, root, "a" * 40)

    def test_only_marked_fresh_contained_directory_is_eligible_for_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            fixture = base / "primerswitch-update-fixture"
            fixture.mkdir()
            marker = fixture / "fixture.marker"
            marker.write_text(qualification.MARKER, encoding="utf8")
            qualification.validate_fixture(fixture, base)
            with self.assertRaises(qualification.QualificationError):
                qualification.validate_fixture(fixture, fixture)
            marker.write_text("foreign marker", encoding="utf8")
            with self.assertRaises(qualification.QualificationError):
                qualification.validate_fixture(fixture, base)

    def test_sentinel_and_application_mutations_are_detected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            application = root / "primerswitch.exe"
            application.write_bytes(b"MZ fixture application")
            digest = qualification.sha256(application.read_bytes())
            for name, body in qualification.SENTINELS.items():
                (root / name).write_bytes(body)
            self.assertTrue(qualification.verify_retention(root, digest)["allSentinelBytesPreserved"])
            (root / "master-key.dpapi").write_bytes(b"changed synthetic key")
            with self.assertRaises(qualification.QualificationError):
                qualification.verify_retention(root, digest)
            (root / "master-key.dpapi").write_bytes(qualification.SENTINELS["master-key.dpapi"])
            application.write_bytes(b"MZ old application")
            with self.assertRaises(qualification.QualificationError):
                qualification.verify_retention(root, digest)

    def test_hash_failure_diagnostics_distinguish_stale_binary_without_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            install = root / "installation"
            install.mkdir()
            old_body = b"MZ historical installed application"
            (install / "primerswitch.exe").write_bytes(old_body)
            expected = root / "compiled-new.exe"
            expected.write_bytes(b"MZ updated compiled application")
            for name, body in qualification.SENTINELS.items():
                (install / name).write_bytes(body)
            state = {qualification.REGISTRY_KEYS[0]: {"DisplayVersion": "0.1.1"}}
            evidence = qualification.installation_evidence(install, state, qualification.sha256(old_body), expected)
            self.assertEqual(evidence["registeredVersion"], "0.1.1")
            self.assertTrue(evidence["matchesOldApplication"])
            self.assertFalse(evidence["matchesExpectedNewApplication"])
            self.assertTrue(all(evidence["sentinelBytesPreserved"].values()))
            self.assertIsNotNone(evidence["firstDifferentByteOffset"])
            self.assertNotIn(str(root), json.dumps(evidence))
            with self.assertRaisesRegex(qualification.QualificationError, "installed-application-hash-mismatch"):
                qualification.verify_retention(install, qualification.sha256(expected.read_bytes()))

    def test_registry_paths_must_belong_to_the_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            state = {qualification.REGISTRY_KEYS[0]: {"DisplayName": "PrimerSwitch", "Publisher": "Primer-Tech", "DisplayVersion": "0.1.1", "UninstallString": '"' + str(root / "uninstall.exe") + '"'}, qualification.REGISTRY_KEYS[1]: {"": str(root)}}
            qualification.verify_registration(state, root, "0.1.1")
            state[qualification.REGISTRY_KEYS[1]][""] = str(root.parent)
            with self.assertRaises(qualification.QualificationError):
                qualification.verify_registration(state, root, "0.1.1")


if __name__ == "__main__":
    unittest.main()
