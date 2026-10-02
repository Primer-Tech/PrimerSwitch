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
import shutil
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
CODEX_SCREENSHOT = ARTIFACTS / "linux-installed-codex-demo.png"
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
                         "enabled": states.contains(pyatspi.STATE_ENABLED),
                         "selected": states.contains(pyatspi.STATE_SELECTED) if hasattr(pyatspi, "STATE_SELECTED") else False,
                         "focused": states.contains(pyatspi.STATE_FOCUSED) if hasattr(pyatspi, "STATE_FOCUSED") else False,
                         "accessible": current})
            for child_index in reversed(range(min(current.childCount, 1000))):
                child = current.getChildAtIndex(child_index)
                if child is not None:
                    pending.append((child, index, depth + 1))
        except Exception:
            # Accessibility objects can disappear during initial WebKit load.
            continue
    return rows


def observe_accessibility_desktop(pyatspi, glib) -> list[dict]:
    desktop = pyatspi.Registry.getDesktop(0)
    # libatspi registration/cache updates arrive on GLib's main context. Merely
    # polling cached child counts does not dispatch those pending events.
    context = glib.MainContext.default()
    for _ in range(256):
        if not context.pending():
            break
        context.iteration(False)
    desktop.clearCache()
    return accessible_nodes(desktop, pyatspi)


def window_details() -> dict | None:
    tree = output(["xwininfo", "-root", "-tree"], timeout=8).decode("utf8", errors="replace")
    for line in tree.splitlines():
        match = re.search(r'(0x[0-9a-f]+) "(PrimerSwitch[^\"]*)".*? (\d+)x(\d+)\+', line, re.I)
        if match:
            details = output(["xwininfo", "-id", match.group(1)], timeout=8).decode("utf8", errors="replace")
            # The X11 window/title may exist before GTK maps or sizes it.
            # Readiness polling waits; the final acceptance still needs both.
            if "Map State: IsViewable" not in details:
                continue
            width = int(re.search(r"Width: (\d+)", details).group(1))
            height = int(re.search(r"Height: (\d+)", details).group(1))
            if width < 560 or height < 620:
                continue
            return {"id": match.group(1), "title": match.group(2), "width": width, "height": height}
    return None



def capture_painted_window(window_id: str, screenshot: Path, *, duration: float = 8.0, previous_frame_sha256: str | None = None) -> dict:
    """Capture only the observed fixture window; AT-SPI can precede first paint."""
    require(bool(re.fullmatch(r"0x[0-9a-fA-F]+", window_id)), "invalid-fixture-window-id")
    deadline = time.monotonic() + duration
    attempts = 0
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        attempts += 1
        try:
            output(["import", "-window", window_id, str(screenshot)], timeout=min(2.0, remaining))
            require(screenshot.is_file() and screenshot.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"), "native-demo-screenshot-missing")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            metrics = output(["identify", "-format", "%k %[fx:mean]", str(screenshot)], timeout=min(2.0, remaining)).decode("ascii").strip()
            # ImageMagick's FX values are normalized to 0..1. The default demo
            # is dark; an initial light loading frame can have many text colors
            # even while AT-SPI already exposes the loaded demo DOM.
            match = re.fullmatch(r"([0-9]+) ([0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)", metrics)
            if match:
                count, mean = int(match.group(1)), float(match.group(2))
                distinct = previous_frame_sha256 is None or sha256(screenshot.read_bytes()) != previous_frame_sha256
                if count > 64 and 0 <= mean < 0.5 and distinct:
                    return {"uniqueColors": count, "normalizedMean": mean, "captureAttempts": attempts,
                            "firstPaintTimeoutSeconds": duration, "minimumUniqueColorsExclusive": 64,
                            "maximumNormalizedMeanExclusive": 0.5, "expectedDefaultTheme": "dark",
                            "differentFromPreviousProviderFrame": distinct if previous_frame_sha256 else None}
        except (QualificationError, OSError, UnicodeError, subprocess.SubprocessError):
            # A newly mapped window can briefly be unavailable to X11 capture.
            # Retrying remains bounded and never changes renderer/sandbox flags.
            pass
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        time.sleep(min(0.25, remaining))
    raise QualificationError("native-window-first-paint-timeout")


def provider_tab(rows: list[dict], provider: str, *, selected: bool) -> dict:
    tabs = [row for row in rows if row["role"] in {"page tab", "tab"}
            and (row["name"] == "Codex" if provider == "codex" else row["name"].endswith("Claude"))]
    require(len(tabs) == 1 and tabs[0]["enabled"] and tabs[0].get("selected") is selected,
            "provider-tab-state-invalid")
    return tabs[0]


def arrow_provider(tab: dict, direction: str, pyatspi, *, window_id: str | None = None) -> None:
    require(tab["accessible"].queryComponent().grabFocus(), "provider-tab-focus-failed")
    require(direction in {"left", "right"}, "invalid-provider-arrow")
    # Let the focus transition reach the WebKit child before sending the key.
    # xdotool uses the XTest path and is reliable with the isolated Xvfb display;
    # AT-SPI remains the hermetic fallback when the helper is unavailable.
    time.sleep(0.15)
    key = "Left" if direction == "left" else "Right"
    if shutil.which("xdotool"):
        if window_id:
            # Modal teardown can leave X11 focus on the transient WebKit dialog
            # even after AT-SPI has restored the tab's accessibility focus.
            # Re-focus the real native window before sending the DOM key event.
            output(["xdotool", "windowfocus", window_id], timeout=5)
        output(["xdotool", "key", "--clearmodifiers", key], timeout=5)
    else:
        # KEY_SYM interprets the value as an X11 keysym. This still exercises
        # the real ArrowLeft/ArrowRight DOM keyboard behavior.
        pyatspi.Registry.generateKeyboardEvent(0xff51 if direction == "left" else 0xff53, None, pyatspi.KEY_SYM)


def press_escape(pyatspi) -> None:
    """Dismiss the active HTML dialog through the real window keyboard path."""
    if shutil.which("xdotool"):
        output(["xdotool", "key", "--clearmodifiers", "Escape"], timeout=5)
    else:
        pyatspi.Registry.generateKeyboardEvent(0xff1b, None, pyatspi.KEY_SYM)


def wait_for_dialog_close(pyatspi, glib, application) -> list[dict]:
    """Wait for Svelte's dialog teardown, retrying Escape if WebKit defers it."""
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        require(application.poll() is None, "installed-demo-exited-during-dialog-close")
        rows = observe_accessibility_desktop(pyatspi, glib)
        if not any(row["role"] == "dialog" and row["name"] == "Personal Codex" for row in rows):
            return rows
        press_escape(pyatspi)
        time.sleep(0.25)
    raise QualificationError("codex-read-only-details-close-timeout")


def wait_for_provider(pyatspi, glib, application, provider: str) -> list[dict]:
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        require(application.poll() is None, "installed-demo-exited-during-provider-navigation")
        rows = observe_accessibility_desktop(pyatspi, glib)
        content = "\n".join(row["name"] + " " + row["text"] for row in rows)
        try:
            tab = provider_tab(rows, provider, selected=True)
            provider_tab(rows, "claude" if provider == "codex" else "codex", selected=False)
            ready = ("Studio Codex" in content and "Personal Codex" in content) if provider == "codex" else "Automation" in content
            if ready and "Demo data" in content and "actions are disabled" in content:
                return rows
        except QualificationError:
            pass
        time.sleep(0.25)
    raise QualificationError("provider-keyboard-navigation-timeout")


def accessible_section_content(rows: list[dict], label: str) -> str:
    headings = [i for i, row in enumerate(rows) if row["name"] == label and row["role"] == "heading"]
    require(len(headings) == 1 and rows[headings[0]]["parent"] is not None, "codex-read-only-count-panel-missing")
    section = rows[headings[0]]["parent"]
    members = []
    for index, row in enumerate(rows):
        ancestor = index
        for _ in range(46):
            if ancestor == section:
                members.append(row["name"] + " " + row["text"])
                break
            ancestor = rows[ancestor]["parent"]
            if ancestor is None:
                break
    return "\n".join(members)


def verify_codex_demo_rows(rows: list[dict]) -> dict:
    content = "\n".join(row["name"] + " " + row["text"] for row in rows)
    expected = ("Studio Codex", "Personal Codex", "codex-demo-0@example.invalid", "codex-demo-1@example.invalid",
                "Selected for new clients", "ChatGPT", "0.160.0", "Managed ChatGPT accounts",
                "FILE credential storage", "Close Codex apps and terminals", "Reopen them after the change.",
                "Code review", "Reset credits", "Read-only", "does not consume Codex credits or resets automatically.",
                "Demo data", "actions are disabled")
    require(all(label in content for label in expected), "native-codex-demo-labels-missing")
    meters = {row["name"] for row in rows if row["role"] in {"meter", "level bar", "progress bar"}}
    require({"5-hour window", "7-day window", "1-day window"} <= meters, "native-codex-quota-durations-missing")
    require(not any(row["name"] == "Automation" and row["role"] in {"push button", "button"} for row in rows), "duplicate-automation-navigation-present")
    buttons = [row for row in rows if row["role"] in {"push button", "button"}]
    selections = [row for row in buttons if row["name"] in {"Selected", "Select account"}]
    require(len(selections) == 2 and {row["name"] for row in selections} == {"Selected", "Select account"}, "native-codex-demo-account-count-mismatch")
    mutations = [row for row in buttons if row["name"] in {"Add account", "Check setup", "Selected", "Select account"}
                 or row["name"].startswith("Refresh Codex quota for ")]
    require(all(not row["enabled"] for row in mutations)
            and any(row["name"] == "Add account" for row in mutations)
            and any(row["name"] == "Check setup" for row in mutations)
            and any(row["name"] == "Refresh Codex quota for Studio Codex" for row in mutations), "native-codex-demo-mutation-controls-not-disabled")
    credits = accessible_section_content(rows, "Reset credits")
    require("Read-only" in credits and bool(re.search(r"(?<![0-9.])3(?![0-9.])", credits)), "native-codex-reset-count-not-read-only")
    provider_tab(rows, "codex", selected=True)
    provider_tab(rows, "claude", selected=False)
    return {"fictionalAccountCount": 2, "selectedAccount": "Studio Codex", "quotaWindowMinutes": [300, 10080, 1440],
            "compatibleCodexVersion": "0.160.0", "resetCreditsReadOnly": 3, "mutationControlsDisabled": len(mutations)}

def worker(directory: Path) -> int:
    report = {"formatVersion": 1, "status": "failed", "scope": "Installed Ubuntu 24.04 x86_64 demo accessibility and native window; no provider/live account qualification", "checks": {}}
    application = None
    application_log = None
    rows, window = [], None
    active_provider = "claude"
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
        from gi.repository import GLib
        pyatspi.setTimeout(2000, 10000)
        application_log = (directory / "native-demo-stderr.log").open("wb")
        application = subprocess.Popen(["/usr/bin/primerswitch", "--demo"], env=os.environ.copy(), stdout=subprocess.DEVNULL, stderr=application_log)
        report["checks"]["installedDemoLaunched"] = True
        deadline = time.monotonic() + 75
        while time.monotonic() < deadline:
            require(application.poll() is None, "installed-demo-exited-before-rendering")
            window = window_details()
            rows = observe_accessibility_desktop(pyatspi, GLib)
            content = "\n".join(row["name"] + " " + row["text"] for row in rows)
            if window and "Demo data" in content and "actions are disabled" in content:
                break
            time.sleep(0.5)
        else:
            raise QualificationError("installed-demo-accessible-renderer-timeout")
        report["accessibilityNodeCount"] = len(rows)
        report["observedLabels"] = sorted({row["name"] for row in rows if row["name"] in {"Accounts", "Open settings", "Automation", "Refresh all", "Switch", "Preview"}})
        require(any(row["name"] == "Accounts" for row in rows), "english-accounts-navigation-missing")
        settings = [row for row in rows if row["name"] == "Open settings" and row["role"] in {"push button", "button"}]
        require(len(settings) == 1 and settings[0]["enabled"], "settings-navigation-unavailable")
        require(any(row["name"] == "Automation" and row["role"] not in {"push button", "button"} for row in rows), "automation-panel-missing")
        require(not any(row["name"] == "Automation" and row["role"] in {"push button", "button"} for row in rows), "duplicate-automation-navigation-present")
        refresh = [row for row in rows if row["role"] in {"push button", "button"} and (row["name"] == "Refresh all" or row["name"].startswith("Refresh account "))]
        switches = [row for row in rows if row["role"] in {"push button", "button"} and row["name"] == "Switch"]
        require(bool(refresh) and bool(switches) and all(not row["enabled"] for row in refresh + switches), "demo-mutation-controls-not-disabled")
        report["screenshotVerification"] = capture_painted_window(window["id"], SCREENSHOT)
        report["window"] = {key: value for key, value in window.items() if key != "id"}
        report["screenshotSha256"] = sha256(SCREENSHOT.read_bytes())
        report["checks"].update({"visibleNativeWindow": True, "englishAccountsAndSettings": True,
                                 "nativeDemoSnapshotRendered": True, "nonblankNativeScreenshot": True, "expectedDarkDemoFrameCaptured": True, "duplicateAutomationNavigationAbsent": True,
                                 "automationPanelPresent": True, "demoMutationButtonsDisabled": True})
        # Switch providers through the real keyboard path, inspect Codex's
        # native in-memory fixture, then return to retain Claude/Settings checks.
        claude_tab = provider_tab(rows, "claude", selected=True)
        provider_tab(rows, "codex", selected=False)
        arrow_provider(claude_tab, "right", pyatspi, window_id=window["id"])
        active_provider = "codex"
        rows = wait_for_provider(pyatspi, GLib, application, "codex")
        report["codexDemoVerification"] = verify_codex_demo_rows(rows)
        # Allow the provider replacement to reach a compositor frame; require
        # its dark PNG to differ from the already accepted Claude PNG as well.
        time.sleep(0.5)
        report["codexScreenshotVerification"] = capture_painted_window(window["id"], CODEX_SCREENSHOT,
                                                                      previous_frame_sha256=report["screenshotSha256"])
        report["codexScreenshotSha256"] = sha256(CODEX_SCREENSHOT.read_bytes())
        report["checks"].update({"codexManagedAccountsRendered": True, "codexNativeQuotaWindowsRendered": True,
                                 "codexCloseAndReopenGuidance": True, "codexResetCreditsReadOnly": True,
                                 "codexMutationButtonsDisabled": True, "codexDarkScreenshotCaptured": True})
        details = [row for row in rows if row["name"] == "Account details and actions for Personal Codex" and row["role"] in {"push button", "button"}]
        require(len(details) == 1 and details[0]["enabled"] and details[0]["accessible"].queryAction().doAction(0), "codex-read-only-details-unavailable")
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            rows = observe_accessibility_desktop(pyatspi, GLib)
            deletes = [row for row in rows if row["role"] in {"push button", "button"} and row["name"] == "Delete account Personal Codex"]
            closes = [row for row in rows if row["role"] in {"push button", "button"} and row["name"] == "Close"]
            if deletes and closes:
                require(len(deletes) == 1 and not deletes[0]["enabled"], "native-codex-demo-delete-not-disabled")
                require(len(closes) == 1 and closes[0]["enabled"] and closes[0]["accessible"].queryAction().doAction(0), "codex-read-only-details-close-failed")
                report["checks"]["codexDetailsDeleteDisabled"] = True
                break
            time.sleep(0.25)
        else:
            raise QualificationError("codex-read-only-details-timeout")
        rows = wait_for_dialog_close(pyatspi, GLib, application)
        require(not any(row["role"] == "dialog" and row["name"] == "Personal Codex" for row in rows), "codex-read-only-details-close-timeout")
        rows = wait_for_provider(pyatspi, GLib, application, "codex")
        arrow_provider(provider_tab(rows, "codex", selected=True), "left", pyatspi, window_id=window["id"])
        active_provider = "claude"
        rows = wait_for_provider(pyatspi, GLib, application, "claude")
        report["checks"]["providerTabsArrowKeyboardRoundTrip"] = True
        settings = [row for row in rows if row["name"] == "Open settings" and row["role"] in {"push button", "button"}]
        require(len(settings) == 1 and settings[0]["enabled"], "settings-navigation-unavailable")
        require(settings[0]["accessible"].queryAction().doAction(0), "settings-navigation-action-failed")
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            dialog_rows = observe_accessibility_desktop(pyatspi, GLib)
            saves = [row for row in dialog_rows if row["name"] == "Save" and row["role"] in {"push button", "button"}]
            if saves:
                require(all(not row["enabled"] for row in saves), "demo-settings-save-not-disabled")
                report["checks"]["settingsDialogReadOnly"] = True
                break
            time.sleep(0.25)
        else:
            raise QualificationError("native-settings-dialog-timeout")
        # The in-memory demo must not create CLI credential or encrypted-vault files.
        forbidden = {".claude", ".claude.json", ".credentials.json", ".codex", "auth.json", ".vault.lock"}
        require(not any(path.name in forbidden or path.suffix == ".vault" or path.name.startswith("master-key.") for path in directory.rglob("*")), "demo-created-credential-or-vault-file")
        report["checks"]["isolatedHomeNoCredentialWrites"] = True
        report["status"] = "passed"
        return 0
    except Exception as error:
        report["error"] = str(error) if isinstance(error, QualificationError) else "installed-demo-qualification-could-not-complete"
        report["failureType"] = type(error).__name__
        return 2
    finally:
        # Capture only this fixture's display/process before termination. A blank
        # image plus native stderr distinguishes renderer failure from AT-SPI.
        if report["status"] != "passed":
            try:
                diagnostic_tree = output(["xwininfo", "-root", "-tree"], timeout=8).decode("utf8", errors="replace")
                report["x11WindowTree"] = diagnostic_tree[:4096]
            except Exception as error:
                report["x11DiagnosticFailureType"] = type(error).__name__
            report["accessibilityNodeCount"] = len(rows)
            report["accessibleNodes"] = [{key: (str(row[key])[:160] if key != "enabled" else row[key])
                                          for key in ("name", "role", "text", "enabled")} for row in rows[:150]]
            report["windowObserved"] = window is not None
            try:
                target = window["id"] if window else "root"
                diagnostic_screenshot = CODEX_SCREENSHOT if active_provider == "codex" else SCREENSHOT
                output(["import", "-window", target, str(diagnostic_screenshot)], timeout=15)
                if diagnostic_screenshot.is_file():
                    report["codexScreenshotSha256" if active_provider == "codex" else "screenshotSha256"] = sha256(diagnostic_screenshot.read_bytes())
                    report["screenshotScope"] = "isolated-native-window" if window else "isolated-xvfb-display"
            except Exception as error:
                report["screenshotFailureType"] = type(error).__name__
        if application is not None and application.poll() is None:
            application.terminate()
            try:
                application.wait(timeout=10)
            except subprocess.TimeoutExpired:
                application.kill()
                application.wait(timeout=5)
        if application_log is not None:
            application_log.close()
            log = (directory / "native-demo-stderr.log").read_bytes().decode("utf8", errors="replace")
            # The process receives only a fresh fixture environment and --demo;
            # remove even its disposable path from bounded diagnostic output.
            report["nativeStderrTail"] = log[-4096:].replace(str(directory), "<fixture>").replace(str(ROOT), "<workspace>")
        if application is not None:
            report["nativeProcessExitCode"] = application.returncode
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
        CODEX_SCREENSHOT.unlink(missing_ok=True)
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
                code = process.wait(timeout=180)
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
