#!/usr/bin/env python3
"""Read-only checks for the files proposed for PrimerSwitch's public repository.

Checks the public layout, sensitive filenames, a scoped set of private/secret
markers, relative Markdown destinations, and package/application metadata.
This is not a complete secret scanner, a cryptographic audit, or a license audit.
It does not inspect Git history, access account stores, or use the network.

Run with Python 3.11+ from any directory. --hashes prints a deterministic SHA-256
manifest to stdout after validation; it does not write a manifest or require Git
history. Generated outputs are excluded from traversal, but are rejected if
tracked/staged. Findings contain relative path:line and a check name only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tomllib
from urllib.parse import unquote, urlsplit


PUBLIC_ROOT_FILES = {
    ".gitattributes", ".gitignore", "AGENTS.md", "Cargo.toml", "Cargo.lock",
    "rust-toolchain.toml", "LICENSE", "README.md", "SECURITY.md",
    "CONTRIBUTING.md", "CHANGELOG.md", "CODE_OF_CONDUCT.md",
}
PUBLIC_ROOT_DIRECTORIES = {".github", "apps", "crates", "docs", "scripts", "tests"}
GENERATED_DIRECTORIES = {
    "target", "node_modules", ".git", ".artifacts", "gen", "test-results",
    "dist", ".svelte-kit", ".vite", "coverage", "playwright-report", "__pycache__",
}
PRIVATE_COMPONENTS = {
    "legacy", "private", "private-research", "private_research", "research",
    "upstream", "baseline-private", "baseline_private", ".aws", ".codex", ".agents",
}
PRIVATE_FILENAMES = {"research.md", "preservation_manifest.json", "baseline_manifest.json"}
SECRET_FILENAMES = {"auth.json", ".credentials.json", "master-key.dpapi", "master-key.keychain"}
SECRET_SUFFIXES = {".vault", ".key", ".pfx", ".p12", ".dpapi"}
ARCHIVE_SUFFIXES = {".bundle", ".zip", ".7z", ".rar", ".tar", ".gz", ".exe", ".msi", ".dmg", ".deb", ".rpm", ".AppImage"}
TEXT_SUFFIXES = {
    ".md", ".rs", ".py", ".toml", ".json", ".ts", ".js", ".mjs", ".cjs",
    ".svelte", ".html", ".css", ".yaml", ".yml", ".svg", ".xml", ".txt",
    ".ps1", ".sh", ".lock", ".gitignore", ".gitattributes",
}
SAFE_EMAIL_DOMAINS = {"example.com", "example.org", "example.net", "example.invalid", "test.invalid"}
PUBLIC_CONTACTS = {"support@anthropic.com", "support@openai.com", "security@github.com"}
PLACEHOLDER_USERS = {"fixture", "fixture-user", "test", "test-user", "user", "username", "yourname"}
APP_ID = "com.primertech.primerswitch"
# A public upstream copyright contact is attribution, not a real account marker.
# Exception applies only to the exact reviewed immutable license bytes/address.
REVIEWED_ATTRIBUTION_EMAILS = {
    "docs/legal/packaging/NSIS-COPYING.txt": (
        "e7dd514003ab96cb3ddccbc028fe5c795fccf57dc41f21cfb9d4dd16ead23bf5",
        {"jseward" + "@" + "acm.org"},
    ),
}


class Checks:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.findings: set[tuple[str, int, str]] = set()

    def fail(self, path: str | Path, line: int, name: str) -> None:
        label = str(path).replace("\\", "/")
        # Filenames can contain control characters. Never let them create log lines.
        label = "".join(character if character.isprintable() else "?" for character in label)
        self.findings.add((label, max(1, line), name))

    def inside(self, path: Path) -> bool:
        try:
            path.resolve().relative_to(self.root)
            return True
        except (ValueError, OSError, RuntimeError):
            return False

    def read(self, relative: str) -> str | None:
        path = self.root / relative
        if not self.inside(path) or path.is_symlink():
            self.fail(relative, 1, "unsafe-path")
            return None
        try:
            return path.read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            self.fail(relative, 1, "unreadable-required-file")
            return None

    def document(self, relative: str, kind: str) -> dict:
        text = self.read(relative)
        if text is None:
            return {}
        try:
            parsed = tomllib.loads(text) if kind == "toml" else json.loads(text)
            if not isinstance(parsed, dict):
                raise ValueError
            return parsed
        except (ValueError, tomllib.TOMLDecodeError):
            self.fail(relative, 1, "invalid-package-metadata")
            return {}


def inventory(root: Path) -> tuple[list[str], str]:
    """Use the exact repository index, never an unrelated parent repository."""
    try:
        top = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "--show-toplevel"],
            capture_output=True, check=True, timeout=10,
        ).stdout.decode("utf-8", "surrogateescape").strip()
        if Path(top).resolve() == root:
            result = subprocess.run(
                ["git", "-C", str(root), "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
                capture_output=True, check=True, timeout=10,
            )
            paths = result.stdout.decode("utf-8", "surrogateescape").split("\0")
            return sorted(set(path for path in paths if path)), "git index and unignored candidates"
    except (OSError, subprocess.SubprocessError, ValueError):
        pass
    files: list[str] = []
    for current, directories, names in os.walk(root, followlinks=False):
        # Retain symlink directories in the inventory so validation can reject them.
        for directory in directories:
            candidate = Path(current) / directory
            if candidate.is_symlink() and directory not in GENERATED_DIRECTORIES:
                files.append(candidate.relative_to(root).as_posix())
        directories[:] = [
            directory for directory in directories
            if directory not in GENERATED_DIRECTORIES and not (Path(current) / directory).is_symlink()
        ]
        for name in names:
            if name.startswith(("_qa_", ".qa-")) and Path(name).suffix in {".py", ".mjs", ".cjs", ".ps1"}:
                continue
            files.append((Path(current) / name).relative_to(root).as_posix())
    return sorted(set(files)), "pruned working-tree traversal"


def verified_legal_files(checks: Checks) -> set[str]:
    """Only exact hash-pinned attribution files may use the upstream directory."""
    manifest = "docs/legal/upstream/manifest.json"
    path = checks.root / manifest
    if not path.is_file() or not checks.inside(path) or path.is_symlink():
        return set()
    allowed = {manifest}
    try:
        parsed = json.loads(path.read_text(encoding="utf-8-sig"))
        if parsed.get("formatVersion") != 1 or not isinstance(parsed.get("licenses"), list):
            raise ValueError
        for entry in parsed["licenses"]:
            relative = entry["file"]
            parts = PurePosixPath(relative).parts
            if len(parts) != 4 or parts[:3] != ("docs", "legal", "upstream") or not re.fullmatch(r"rust-[a-z0-9_-]+-[0-9]+(?:\.[0-9]+)*-LICENSE(?:-[A-Z0-9.-]+)?", parts[3]):
                raise ValueError
            candidate = checks.root / relative
            if not checks.inside(candidate) or candidate.is_symlink() or hashlib.sha256(candidate.read_bytes()).hexdigest() != entry["sha256"]:
                raise ValueError
            allowed.add(relative)
    except (OSError, ValueError, TypeError, KeyError):
        checks.fail(manifest, 1, "invalid-upstream-attribution-manifest")
        return {manifest}
    return allowed


def check_layout(checks: Checks, files: list[str]) -> None:
    legal_files = verified_legal_files(checks)
    for relative in files:
        parts = PurePosixPath(relative).parts
        if not parts or ".." in parts or PurePosixPath(relative).is_absolute():
            checks.fail(relative, 1, "unsafe-path")
            continue
        if (len(parts) == 1 and parts[0] not in PUBLIC_ROOT_FILES) or (
            len(parts) > 1 and parts[0] not in PUBLIC_ROOT_DIRECTORIES
        ):
            checks.fail(relative, 1, "public-root-allowlist")
        folded = [part.casefold() for part in parts]
        if any(part in PRIVATE_COMPONENTS for part in folded if not (part == "upstream" and relative in legal_files)) or folded[-1] in PRIVATE_FILENAMES or folded[-1].endswith(".swift"):
            checks.fail(relative, 1, "private-source-or-research-file")
        if any(part in GENERATED_DIRECTORIES for part in parts):
            checks.fail(relative, 1, "generated-file-in-public-inventory")
        name = parts[-1].casefold()
        if name in SECRET_FILENAMES or Path(name).suffix in SECRET_SUFFIXES or (
            (name == ".env" or name.startswith(".env.")) and name != ".env.example"
        ):
            checks.fail(relative, 1, "credential-or-private-key-filename")
        if Path(name).suffix in ARCHIVE_SUFFIXES:
            checks.fail(relative, 1, "archive-or-package-in-source-tree")
        path = checks.root / relative
        if not checks.inside(path) or path.is_symlink():
            checks.fail(relative, 1, "unsafe-path")
        elif not path.is_file():
            checks.fail(relative, 1, "inventory-file-missing")


def check_markers(checks: Checks, relative: str, text: str) -> None:
    attribution_emails: set[str] = set()
    reviewed = REVIEWED_ATTRIBUTION_EMAILS.get(relative)
    if reviewed:
        try:
            path = checks.root / relative
            if checks.inside(path) and not path.is_symlink() and hashlib.sha256(path.read_bytes()).hexdigest() == reviewed[0]:
                attribution_emails = reviewed[1]
        except OSError:
            pass
    email = re.compile(r"(?<![\w.-])([A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@(?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,})")
    owner_path = re.compile(r"(?:[A-Za-z]:[\\/]+Users[\\/]+|/(?:Users|home)/)([A-Za-z0-9_.-]+)", re.IGNORECASE)
    private_source = re.compile(r"(?:Primer[/\\]+ClaudeSwitch[/\\]+legacy|legacy[/\\]+macos[/\\]+Sources[/\\]+ClaudeSwitch)", re.IGNORECASE)
    secret = re.compile(r"(?:sk-ant-|sk-proj-|sk-)[A-Za-z0-9_-]{24,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}")
    for number, line in enumerate(text.splitlines(), 1):
        if private_source.search(line):
            checks.fail(relative, number, "private-source-reference")
        if any(match.group(1).casefold() not in PLACEHOLDER_USERS for match in owner_path.finditer(line)):
            checks.fail(relative, number, "local-owner-path")
        for match in email.finditer(line):
            address = match.group(1).casefold()
            if re.fullmatch(r"[^@]+@[1-9]x\.(?:png|jpg|jpeg|webp|svg)", address):
                continue  # Conventional retina image filenames are not emails.
            if address.split("@", 1)[1] not in SAFE_EMAIL_DOMAINS and address not in PUBLIC_CONTACTS and address not in attribution_emails:
                checks.fail(relative, number, "nonfixture-email-marker")
        # Explicit synthetic sentinels are legitimate fixture inputs. This narrow
        # exception is another reason the check is not a complete secret scanner.
        if any("SENTINEL" not in match.group() for match in secret.finditer(line)):
            checks.fail(relative, number, "known-secret-marker")


def markdown_prose(text: str) -> str:
    """Blank fenced/inline code while retaining newlines for useful line numbers."""
    lines = text.splitlines(keepends=True)
    fence: tuple[str, int] | None = None
    output: list[str] = []
    for line in lines:
        match = re.match(r"^ {0,3}(`{3,}|~{3,})", line)
        if fence is not None:
            if match and match.group(1)[0] == fence[0] and len(match.group(1)) >= fence[1] and not line[match.end():].strip():
                fence = None
            output.append("".join("\n" if char == "\n" else " " for char in line))
        elif match:
            fence = (match.group(1)[0], len(match.group(1)))
            output.append("".join("\n" if char == "\n" else " " for char in line))
        else:
            output.append(re.sub(r"(`+)(?!`)(.*?)(?<!`)\1(?!`)", lambda m: " " * len(m.group()), line))
    return "".join(output)


def markdown_targets(text: str) -> list[tuple[int, str]]:
    prose = markdown_prose(text)
    found: list[tuple[int, str]] = []
    # Inline links and images. Track nested parentheses and escaped delimiters.
    brackets: list[int] = []
    positions: list[tuple[int, int]] = []
    escaped = False
    for index, character in enumerate(prose):
        if escaped:
            escaped = False
        elif character == "\\":
            escaped = True
        elif character == "[":
            brackets.append(index)
        elif character == "]" and brackets:
            opening = brackets.pop()
            if prose[index:index + 2] == "](":
                positions.append((opening, index + 2))
    for opening, start in positions:
        cursor, depth, angle, escaped, quote = start, 1, False, False, None
        while cursor < len(prose):
            char = prose[cursor]
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif quote is not None:
                if char == quote:
                    quote = None
            elif not angle and char in {"\"", "'"}:
                quote = char
            elif char == "<":
                angle = True
            elif char == ">":
                angle = False
            elif not angle and char == "(":
                depth += 1
            elif not angle and char == ")":
                depth -= 1
                if depth == 0:
                    destination = prose[start:cursor].strip()
                    if destination.startswith("<"):
                        destination = destination[1:destination.find(">")]
                    else:
                        destination = re.split(r"\s+", destination, maxsplit=1)[0]
                    found.append((prose.count("\n", 0, opening) + 1, destination))
                    break
            cursor += 1
    # Checking every reference destination also covers full/collapsed/shortcut uses.
    for match in re.finditer(r"(?m)^ {0,3}\[(?!\^)[^\]\n]+\]:[ \t]*(<[^>\n]+>|[^\s]+)", prose):
        found.append((prose.count("\n", 0, match.start()) + 1, match.group(1).strip("<>")))
    for match in re.finditer(r"<(?:img|a)\b[^>]*?\b(?:src|href)\s*=\s*([\"'])(.*?)\1", prose, re.IGNORECASE):
        found.append((prose.count("\n", 0, match.start()) + 1, match.group(2)))
    return found


def check_links(checks: Checks, relative: str, text: str) -> None:
    for line, target in markdown_targets(text):
        target = re.sub(r"\\([\\`*{}\[\]()#+.! _-])", r"\1", target)
        if not target or target.startswith("#"):
            continue
        try:
            parsed = urlsplit(target)
        except ValueError:
            checks.fail(relative, line, "invalid-markdown-destination")
            continue
        if parsed.scheme or parsed.netloc:
            continue  # External destinations are not fetched or validated.
        destination = unquote(parsed.path).replace("\\", "/")
        path = (checks.root / destination.lstrip("/")) if destination.startswith("/") else ((checks.root / relative).parent / destination)
        if not checks.inside(path):
            checks.fail(relative, line, "markdown-link-outside-public-tree")
        elif not path.exists():
            checks.fail(relative, line, "markdown-link-missing")


def check_contract(checks: Checks) -> None:
    for relative in ("README.md", "LICENSE", "docs/migration/PROGRESS.md", "docs/migration/REPOSITORY_STRATEGY.md"):
        checks.read(relative)
    cargo = checks.document("Cargo.toml", "toml")
    workspace = cargo.get("workspace", {})
    package = workspace.get("package", {})
    version = package.get("version")
    if not isinstance(version, str) or package.get("license") != "MIT":
        checks.fail("Cargo.toml", 1, "workspace-version-or-license-contract")
    npm = checks.document("apps/desktop/package.json", "json")
    lock = checks.document("apps/desktop/package-lock.json", "json")
    tauri = checks.document("apps/desktop/src-tauri/tauri.conf.json", "json")
    for relative, document in (("apps/desktop/package.json", npm), ("apps/desktop/package-lock.json", lock), ("apps/desktop/src-tauri/tauri.conf.json", tauri)):
        if document.get("version") != version:
            checks.fail(relative, 1, "application-version-mismatch")
    if lock.get("packages", {}).get("", {}).get("version") != version or lock.get("name") != npm.get("name"):
        checks.fail("apps/desktop/package-lock.json", 1, "npm-lock-contract")
    if tauri.get("identifier") != APP_ID or tauri.get("productName") != "PrimerSwitch":
        checks.fail("apps/desktop/src-tauri/tauri.conf.json", 1, "tauri-application-identity")
    cargo_lock = checks.document("Cargo.lock", "toml")
    locked = {entry.get("name"): entry.get("version") for entry in cargo_lock.get("package", []) if "source" not in entry}
    for member in workspace.get("members", []):
        if not isinstance(member, str) or not checks.inside(checks.root / member):
            checks.fail("Cargo.toml", 1, "workspace-member-outside-public-tree")
            continue
        manifest = f"{member}/Cargo.toml"
        member_package = checks.document(manifest, "toml").get("package", {})
        member_version = member_package.get("version")
        if member_version != version and member_version != {"workspace": True}:
            checks.fail(manifest, 1, "cargo-package-version-mismatch")
        if locked.get(member_package.get("name")) != version:
            checks.fail("Cargo.lock", 1, "cargo-workspace-lock-version-mismatch")
    toolchain = checks.document("rust-toolchain.toml", "toml").get("toolchain", {}).get("channel", "")
    minimum = package.get("rust-version", "")
    pattern = r"\d+\.\d+(?:\.\d+)?"
    if not re.fullmatch(pattern, str(toolchain)) or not re.fullmatch(pattern, str(minimum)) or (
        tuple(int(part) for part in str(toolchain).split(".")) < tuple(int(part) for part in str(minimum).split("."))
    ):
        checks.fail("rust-toolchain.toml", 1, "pinned-rust-version-contract")
    for relative in ("apps/desktop/index.html", "apps/desktop/demo.html"):
        text = checks.read(relative)
        if text is not None and not re.search(r"<html\b[^>]*\blang\s*=\s*([\"'])en\1", text, re.IGNORECASE):
            checks.fail(relative, 1, "english-default-locale-contract")


def run(root: Path, hashes: bool = False) -> int:
    checks = Checks(root.resolve())
    files, source = inventory(checks.root)
    check_layout(checks, files)
    for relative in files:
        path = checks.root / relative
        if not checks.inside(path) or not path.is_file() or path.is_symlink():
            continue
        if path.suffix.casefold() not in TEXT_SUFFIXES and path.name not in {"LICENSE", ".gitignore", ".gitattributes"}:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            checks.fail(relative, 1, "unreadable-public-text")
            continue
        check_markers(checks, relative, text)
        if path.suffix.casefold() == ".md":
            check_links(checks, relative, text)
    check_contract(checks)
    if checks.findings:
        for path, line, name in sorted(checks.findings):
            print(f"{path}:{line}: {name}")
        print(f"Repository checks failed: {len(checks.findings)} scoped findings.")
        return 1
    if hashes:
        manifest = {relative: hashlib.sha256((checks.root / relative).read_bytes()).hexdigest() for relative in files}
        print(json.dumps({"algorithm": "sha256", "inventory": source, "files": manifest}, indent=2, sort_keys=True))
    else:
        print(f"Repository checks passed: {len(files)} files ({source}).")
        print("Scope: public layout, selected private/secret markers, local Markdown destinations, package/app/locale metadata. Not a complete secret or license audit.")
    return 0


def main() -> int:
    class SafeArgumentParser(argparse.ArgumentParser):
        def error(self, message: str) -> None:
            self.exit(2, "scripts/check_repository.py:1: unsupported-arguments\n")

    parser = SafeArgumentParser(description=__doc__)
    parser.add_argument("--hashes", action="store_true", help="Print a read-only SHA-256 inventory after validation")
    arguments = parser.parse_args()
    return run(Path(__file__).resolve().parent.parent, arguments.hashes)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, TypeError, KeyError):
        # Never print raw exceptions: a path or supplied value might be sensitive.
        print("scripts/check_repository.py:1: repository-check-internal-failure")
        sys.exit(2)
