"""Run the isolated macOS Keychain/vault qualification on a hosted runner."""

from __future__ import annotations

import json
import os
import platform
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / ".artifacts" / "macos-native-report.json"


def main() -> int:
    if platform.system() != "Darwin":
        print("macOS native qualification requires a Darwin runner", file=sys.stderr)
        return 1
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.unlink(missing_ok=True)
    environment = os.environ.copy()
    environment["PRIMERSWITCH_MACOS_KEYCHAIN_QUALIFICATION"] = "1"
    environment["PRIMERSWITCH_MACOS_REPORT"] = str(REPORT)
    command = [
        "cargo",
        "test",
        "--locked",
        "-p",
        "switcher-platform",
        "--test",
        "macos_native",
        "--",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    completed = subprocess.run(command, cwd=ROOT, env=environment, check=False)
    if completed.returncode != 0:
        print("macOS native qualification failed", file=sys.stderr)
        return completed.returncode or 1
    if not REPORT.is_file():
        print("macOS native qualification produced no report", file=sys.stderr)
        return 1
    report = json.loads(REPORT.read_text(encoding="utf-8"))
    if report.get("status") != "passed" or report.get("realCredentialsAccessed") is not False:
        print("macOS native qualification report failed closed", file=sys.stderr)
        return 1
    print("macOS native Keychain and vault qualification passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
