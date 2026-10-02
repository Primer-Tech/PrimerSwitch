#!/usr/bin/env python3
"""Replay current qualifiers against retained, source-bound native CI packages."""
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

REPOSITORY = "Primer-Tech/PrimerSwitch"


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise ValueError(reason)


def capture(arguments: list[str], cwd: Path) -> str:
    return subprocess.check_output(arguments, cwd=cwd, text=True, timeout=90)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", choices=("nsis", "deb"), required=True)
    bundle = parser.parse_args().bundle
    require(os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted", "requires-hosted-runner")
    require(platform.system() == {"nsis": "Windows", "deb": "Linux"}[bundle], "requires-matching-native-host")
    run_id = os.environ.get("PACKAGE_RUN", "")
    commit = os.environ.get("PACKAGE_COMMIT", "")
    require(re.fullmatch(r"[0-9]{1,20}", run_id) is not None and re.fullmatch(r"[0-9a-f]{40}", commit) is not None, "invalid-package-run-or-commit")
    workspace = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    require(Path.cwd().resolve() == workspace, "unexpected-replay-workspace")
    qualification = workspace / "qualification"
    payload = workspace / "package-source"
    require(all(not path.is_symlink() and path.is_dir() and path.resolve().parent == workspace for path in (qualification, payload)), "unsafe-replay-checkout")
    require(capture(["git", "rev-parse", "HEAD"], payload).strip() == commit, "package-checkout-mismatch")
    qualification_commit = capture(["git", "rev-parse", "HEAD"], qualification).strip()
    require(re.fullmatch(r"[0-9a-f]{40}", qualification_commit) is not None, "invalid-qualification-commit")
    run = json.loads(capture(["gh", "api", f"repos/{REPOSITORY}/actions/runs/{run_id}"], workspace))
    require(run.get("repository", {}).get("full_name") == REPOSITORY and run.get("head_repository", {}).get("full_name") == REPOSITORY and run.get("head_sha") == commit and run.get("head_branch") == "main" and run.get("event") == "workflow_dispatch" and run.get("path", "").split("@")[0] == ".github/workflows/packages.yml", "untrusted-package-run")
    label = {"nsis": "Windows-nsis", "deb": "Linux-deb"}[bundle]
    subprocess.run(["gh", "run", "download", run_id, "--repo", REPOSITORY, "-n", f"PrimerSwitch-{label}-build-evidence", "-D", str(payload)], cwd=workspace, check=True, timeout=180)
    script_name = {"nsis": "qualify_windows_update.py", "deb": "qualify_linux_preview.py"}[bundle]
    source = qualification / "scripts" / script_name
    destination = payload / "scripts" / script_name
    require(source.is_file() and not source.is_symlink() and destination.is_file() and not destination.is_symlink(), "unsafe-qualification-script")
    manifest = json.loads((payload / ".artifacts/package-manifest.json").read_text(encoding="utf8"))
    require(manifest.get("sourceCommit") == commit and "scripts/" + script_name not in manifest.get("packagingInputSha256", {}), "qualification-would-change-package-input")
    # Only the test harness changes; all manifest-bound source/output bytes and
    # the original source checkout stay available to its strict verifier.
    shutil.copyfile(source, destination)
    arguments = [sys.executable, "-B", str(destination)]
    if bundle == "nsis":
        previous = json.loads((payload / ".artifacts/windows-update-preservation-report.json").read_text(encoding="utf8"))
        require(previous.get("sourceCommit") == commit and hashlib.sha256((payload / "target/release/primerswitch.exe").read_bytes()).hexdigest() == previous.get("newApplicationSha256"), "retained-compiled-binary-mismatch")
        arguments += ["--qualification-source-commit", qualification_commit]
    completed = subprocess.run(arguments, cwd=payload, timeout=420)
    report_name = {"nsis": "windows-update-preservation-report.json", "deb": "linux-installed-demo-report.json"}[bundle]
    report_file = payload / ".artifacts" / report_name
    if report_file.is_file() and not report_file.is_symlink():
        report = json.loads(report_file.read_text(encoding="utf8"))
        report.update({"qualificationSourceCommit": qualification_commit, "packageWorkflowRun": int(run_id), "retainedPackageReplay": True, "qualificationScriptSha256": hashlib.sha256(destination.read_bytes()).hexdigest()})
        report_file.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf8")
    require(completed.returncode == 0, "native-qualification-failed")
    print("Retained native package qualification passed with original input hashes.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, KeyError, OSError, subprocess.SubprocessError):
        print("Retained native package qualification failed closed.", file=sys.stderr)
        sys.exit(2)
