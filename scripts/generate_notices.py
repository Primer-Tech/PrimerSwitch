#!/usr/bin/env python3
"""Generate local, deterministic packaging attribution; Python 3.11+.

Uses locked/offline Cargo metadata for a selected native target and the installed
frontend production dependency closure, optionally narrowed to hash-verified
Vite bundle evidence. Conservatively includes native build/proc-macro crates.
--overrides imports project-local attribution with primary URL/revision/hash
provenance; no network is used by this helper. Does not claim
binary reachability or a complete legal audit. No network, account stores, Git
history, credential files, or runtime provider requests are accessed.

Full locally supplied LICENSE/LICENCE/COPYING/COPYRIGHT/NOTICE and equivalent
files are reproduced, including nested vendored attribution. Missing texts,
unrecognized expressions, and restrictive/nonstandard expressions require
review. Exit 1 means review findings, 2 means generation could not complete.
Outputs initially belong in ignored .artifacts, not the published source tree.
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
import tarfile
import tomllib
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent
TEXT_NAME = re.compile(r'^(?:licen[cs]e|copying|copyright|notice|unlicense|ofl|third[-_ ]party)(?:[._ -].*)?$', re.I)
PRIVATE_PATH = re.compile(r'(?:[A-Za-z]:[\\/]Users[\\/]|/(?:Users|home)/|\\\\[^\s\\]+\\Users\\)', re.I)
RECOGNIZED = {'MIT', 'MIT-0', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', 'CC0-1.0', 'BSL-1.0', 'Unicode-3.0', 'Unicode-DFS-2016', 'MPL-2.0', 'OpenSSL', 'Unlicense', '0BSD', 'NCSA', 'BlueOak-1.0.0', 'CDLA-Permissive-2.0'}
REVIEW_LICENSES = {'MPL-2.0', 'OpenSSL'}

class GenerationError(Exception):
    pass


def encoded_json(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n'


def safe_url(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    parts = urlsplit(value.removeprefix('registry+').removeprefix('git+'))
    if parts.scheme not in {'http', 'https'} or parts.username or parts.password:
        return None
    # Repository or registry identity, never credentials/query parameters.
    return f'{parts.scheme}://{parts.netloc}{parts.path}'


def issue(issues: list[dict], ecosystem: str, name: str, version: str, code: str, file: str | None = None) -> None:
    entry = {'ecosystem': ecosystem, 'name': name, 'version': version, 'check': code}
    if file:
        entry['file'] = file
    if entry not in issues:
        issues.append(entry)


def collect_texts(directory: Path, explicit: str | None, identity: dict, issues: list[dict]) -> list[dict]:
    candidates: set[Path] = set()
    for base, dirs, files in os.walk(directory, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in {'.git', 'node_modules', 'target', '__pycache__'} and not (Path(base) / d).is_symlink())
        for name in sorted(files):
            if TEXT_NAME.fullmatch(name):
                candidate = Path(base) / name
                if candidate.is_file() and not candidate.is_symlink():
                    candidates.add(candidate)
    if explicit:
        candidate = (directory / explicit).resolve()
        if candidate.is_relative_to(directory.resolve()) and candidate.is_file() and not candidate.is_symlink():
            candidates.add(candidate)
        else:
            issue(issues, **identity, code='declared-license-file-unavailable')
    texts = []
    for candidate in sorted(candidates, key=lambda p: p.relative_to(directory).as_posix()):
        label = candidate.relative_to(directory).as_posix()
        try:
            # UTF-8 and Latin-1 round-trip local attribution; never replace text.
            raw = candidate.read_bytes()
            try:
                body = raw.decode('utf-8-sig')
            except UnicodeDecodeError:
                body = raw.decode('latin-1')
                issue(issues, **identity, code='non-utf8-attribution', file=label)
            body = body.replace('\r\n', '\n').replace('\r', '\n')
            if PRIVATE_PATH.search(body) or '\x00' in body:
                issue(issues, **identity, code='unsafe-attribution-text-omitted', file=label)
                continue
            texts.append({'file': label, 'sha256': hashlib.sha256(body.encode('utf8')).hexdigest(), 'text': body})
        except OSError:
            issue(issues, **identity, code='unreadable-attribution', file=label)
    if not any(t['text'].strip() and not t['file'].lower().endswith('.spdx') for t in texts):
        issue(issues, **identity, code='missing-full-license-text')
    return texts


def record(ecosystem: str, name: str, version: str, directory: Path, license_value: object, explicit: str | None, repository: object, source: object, issues: list[dict]) -> dict:
    identity = {'ecosystem': ecosystem, 'name': name, 'version': version}
    expression = license_value if isinstance(license_value, str) else None
    if not expression:
        issue(issues, **identity, code='missing-license-expression')
    else:
        tokens = set(re.findall(r'[A-Za-z0-9][A-Za-z0-9.+-]*', expression)) - {'AND', 'OR', 'WITH'}
        if tokens - RECOGNIZED:
            issue(issues, **identity, code='unrecognized-license-expression')
        if tokens & REVIEW_LICENSES or 'WITH' in expression:
            issue(issues, **identity, code='license-expression-review')
    return {**identity, 'license': expression, 'repository': safe_url(repository), 'source': safe_url(source), 'texts': collect_texts(directory, explicit, identity, issues)}


def source_provenance(package: dict, checksums: dict, issues: list[dict]) -> dict:
    name, version = package['name'], package['version']
    directory = Path(package['manifest_path']).parent
    details = {'sourceArchive': f'https://static.crates.io/crates/{name}/{name}-{version}.crate', 'sourceArchiveSha256': checksums.get((name, version))}
    vcs_file = directory / '.cargo_vcs_info.json'
    if vcs_file.is_file():
        vcs = read_manifest(vcs_file)
        commit = vcs.get('git', {}).get('sha1')
        if isinstance(commit, str) and re.fullmatch(r'[0-9a-f]{40}', commit):
            details['pinnedCommit'] = commit
            details['sourcePathInRepository'] = vcs.get('path_in_vcs', '')
    if 'MPL-2.0' not in (package.get('license') or ''):
        return details
    archive = directory.parents[2] / 'cache' / directory.parent.name / f'{name}-{version}.crate'
    if not archive.is_file():
        issue(issues, 'cargo', name, version, 'mpl-source-archive-not-cached')
        return details
    if hashlib.sha256(archive.read_bytes()).hexdigest() != details['sourceArchiveSha256']:
        raise GenerationError('cargo-source-archive-checksum-mismatch')
    count = 0
    with tarfile.open(archive, 'r:gz') as packaged:
        for member in packaged.getmembers():
            if not member.isfile():
                continue
            relative = PurePosixPath(member.name).parts[1:]
            if not relative or '..' in relative:
                raise GenerationError('cargo-source-archive-shape-invalid')
            actual = directory.joinpath(*relative)
            if not actual.resolve().is_relative_to(directory.resolve()) or not actual.is_file() or actual.read_bytes() != packaged.extractfile(member).read():
                raise GenerationError('cargo-cached-source-modified')
            count += 1
    details['cachedSourceMatchesLockedArchive'] = True
    details['verifiedSourceFileCount'] = count
    details['sourceIntegrity'] = {'status': 'verified', 'archiveUrl': details['sourceArchive'], 'archiveSha256': details['sourceArchiveSha256'], 'upstreamRevision': details.get('pinnedCommit'), 'verifiedSourceFileCount': count}
    return details


def cargo_records(target: str, issues: list[dict]) -> list[dict]:
    try:
        process = subprocess.run(['cargo', 'metadata', '--manifest-path', str(ROOT / 'Cargo.toml'), '--format-version', '1', '--locked', '--offline', '--filter-platform', target, '--features', 'primerswitch-desktop/custom-protocol'], cwd=ROOT, capture_output=True, encoding='utf8')
    except (OSError, UnicodeError):
        raise GenerationError('cargo-metadata-unavailable') from None
    if process.returncode:
        # stderr can include local cache paths; do not emit it.
        raise GenerationError('cargo-offline-metadata-failed')
    try:
        metadata = json.loads(process.stdout)
        packages = {p['id']: p for p in metadata['packages']}
        workspace = set(metadata['workspace_members'])
        desktop = next(p['id'] for p in metadata['packages'] if p['name'] == 'primerswitch-desktop' and p['id'] in workspace)
    except (KeyError, ValueError, StopIteration):
        raise GenerationError('cargo-metadata-shape-invalid') from None
    # Cargo metadata unifies workspace features, so its graph alone can include
    # crates not enabled for this packaged binary. Cargo tree selects the exact
    # desktop feature graph; metadata supplies identities and cached manifests.
    try:
        tree = subprocess.run(['cargo', 'tree', '--manifest-path', str(ROOT / 'Cargo.toml'), '-p', 'primerswitch-desktop', '--target', target, '--edges', 'normal,build', '--features', 'custom-protocol', '--locked', '--offline', '--prefix', 'none', '--format', '{p}'], cwd=ROOT, capture_output=True, encoding='utf8')
    except (OSError, UnicodeError):
        raise GenerationError('cargo-tree-unavailable') from None
    if tree.returncode:
        raise GenerationError('cargo-offline-tree-failed')
    selected = set(re.findall(r'^([^\s]+) v([^\s]+)', tree.stdout, re.M))
    if (packages[desktop]['name'], packages[desktop]['version']) not in selected:
        raise GenerationError('cargo-tree-shape-invalid')
    visited = {pid for pid, p in packages.items() if (p['name'], p['version']) in selected}
    if len(visited) != len(selected):
        raise GenerationError('cargo-tree-identity-ambiguous')
    checksums = {(p['name'], p['version']): p.get('checksum') for p in tomllib.loads((ROOT / 'Cargo.lock').read_text(encoding='utf8'))['package']}
    result = []
    for package_id in visited - workspace:
        p = packages[package_id]
        r = record('cargo', p['name'], p['version'], Path(p['manifest_path']).parent, p.get('license'), p.get('license_file'), p.get('repository'), p.get('source'), issues)
        r.update(source_provenance(p, checksums, issues))
        result.append(r)
    return result


def npm_records(issues: list[dict], inventory_path: Path | None = None) -> list[dict]:
    frontend = ROOT / 'apps' / 'desktop'
    try:
        manifest = json.loads((frontend / 'package.json').read_text(encoding='utf8'))
        lock = json.loads((frontend / 'package-lock.json').read_text(encoding='utf8'))
        packages = lock['packages']
    except (OSError, ValueError, KeyError):
        raise GenerationError('frontend-metadata-unavailable') from None
    if lock.get('lockfileVersion') not in {2, 3}:
        raise GenerationError('unsupported-npm-lock-format')
    if packages[''].get('dependencies', {}) != manifest.get('dependencies', {}):
        raise GenerationError('frontend-manifest-lock-mismatch')

    def resolve(parent: str, name: str) -> str | None:
        # npm resolves nested node_modules and then each enclosing ancestor.
        scope = PurePosixPath(parent)
        while True:
            candidate = (scope / 'node_modules' / name).as_posix()
            if candidate in packages:
                return candidate
            if str(scope) == '.':
                return None
            scope = scope.parent

    visited: set[str] = set()
    pending: list[tuple[str, str, bool]] = [('', name, False) for name in sorted(manifest.get('dependencies', {}))]
    while pending:
        parent, name, optional = pending.pop()
        location = resolve(parent, name)
        if location is None:
            if not optional:
                issue(issues, 'npm', name, 'unresolved', 'dependency-not-in-lock')
            continue
        if location in visited:
            continue
        visited.add(location)
        p = packages[location]
        optional_deps = p.get('optionalDependencies', {})
        peer_meta = p.get('peerDependenciesMeta', {})
        for dep in p.get('dependencies', {}):
            pending.append((location, dep, dep in optional_deps))
        for dep in optional_deps:
            pending.append((location, dep, True))
        for dep in p.get('peerDependencies', {}):
            pending.append((location, dep, peer_meta.get(dep, {}).get('optional', False)))
    # Generated bundler helpers can ship from a declared development package.
    # Include exactly inventory-selected package roots from the whole lockfile.
    if inventory_path is not None:
        bundle = read_manifest(inventory_path)
        for item in bundle.get('packages', []):
            location = item.get('location')
            if location not in packages or not str(location).startswith('node_modules/'):
                raise GenerationError('frontend-inventory-package-not-in-lock')
            visited.add(location)
    result = []
    for location in sorted(visited):
        p = packages[location]
        directory = frontend / location
        try:
            installed = json.loads((directory / 'package.json').read_text(encoding='utf8'))
        except (OSError, ValueError):
            issue(issues, 'npm', p.get('name', location.split('node_modules/')[-1]), p.get('version', 'unknown'), 'installed-package-unavailable')
            continue
        name = installed['name']
        version = p['version']
        if installed.get('version') != version:
            raise GenerationError('installed-frontend-lock-version-mismatch')
        repository = installed.get('repository')
        if isinstance(repository, dict):
            repository = repository.get('url')
        result.append(record('npm', name, version, directory, installed.get('license', p.get('license')), None, repository, p.get('resolved'), issues))
    # Multiple installed instances may be identical; retain each text if different.
    unique = {encoded_json(r): r for r in result}
    return list(unique.values())


def read_manifest(path: Path) -> object:
    try:
        return json.loads(path.read_text(encoding='utf-8-sig'))
    except (OSError, UnicodeError, ValueError):
        raise GenerationError('attribution-input-unavailable') from None


def verified_file(label: str, digest: str) -> tuple[bytes, str]:
    if not isinstance(label, str) or not isinstance(digest, str) or not re.fullmatch(r'[0-9a-f]{64}', digest):
        raise GenerationError('attribution-input-shape-invalid')
    file = ROOT / label
    resolved = file.resolve()
    if Path(label).is_absolute() or not resolved.is_relative_to(ROOT) or file.is_symlink():
        raise GenerationError('attribution-file-outside-project')
    data = resolved.read_bytes()
    if hashlib.sha256(data).hexdigest() != digest:
        raise GenerationError('attribution-input-hash-mismatch')
    return data, resolved.relative_to(ROOT).as_posix()


def import_overrides(records: list[dict], issues: list[dict], manifest: Path | None) -> list[dict]:
    if manifest is None:
        return []
    entries = read_manifest(manifest)
    if isinstance(entries, dict):
        entries = entries.get('licenses')
    if not isinstance(entries, list):
        raise GenerationError('attribution-overrides-shape-invalid')
    by_identity = {(r['ecosystem'], r['name'], r['version']): r for r in records}
    imported = []
    for entry in entries:
        if not isinstance(entry, dict):
            raise GenerationError('attribution-overrides-shape-invalid')
        key = (entry.get('ecosystem', 'cargo'), entry.get('package'), entry.get('version'))
        if key not in by_identity:
            if any(active[0:2] == key[0:2] for active in by_identity):
                raise GenerationError('attribution-override-version-mismatch')
            # A shared manifest may contain platform-specific packages absent
            # from this target. Never fabricate an entry in the selected scope.
            continue
        r = by_identity[key]
        url = entry.get('url')
        commit = entry.get('pinnedCommit')
        if safe_url(url) != url or not isinstance(commit, str) or not re.fullmatch(r'[0-9a-f]{40}', commit):
            raise GenerationError('attribution-provenance-invalid')
        reference = entry.get('referenceUrl')
        if commit not in url and (not reference or safe_url(reference) != reference or commit not in reference):
            raise GenerationError('attribution-source-not-revision-pinned')
        if entry.get('spdx') not in set(re.findall(r'[A-Za-z0-9][A-Za-z0-9.+-]*', r['license'] or '')):
            raise GenerationError('attribution-override-license-mismatch')
        data, label = verified_file(entry.get('file'), entry.get('sha256'))
        try:
            body = data.decode('utf-8-sig').replace('\r\n', '\n').replace('\r', '\n')
        except UnicodeError:
            raise GenerationError('attribution-override-not-utf8') from None
        if len(body.strip()) < 100 or PRIVATE_PATH.search(body) or '\x00' in body:
            raise GenerationError('attribution-override-text-invalid')
        evidence = {key: entry[key] for key in ('url', 'repository', 'pinnedCommit', 'sha256', 'spdx', 'referenceUrl', 'licenseReference') if key in entry}
        evidence['file'] = label
        if PRIVATE_PATH.search(encoded_json(evidence)):
            raise GenerationError('attribution-provenance-local-path')
        r['texts'].append({'file': 'upstream/' + Path(label).name, 'sha256': hashlib.sha256(body.encode('utf8')).hexdigest(), 'sourceSha256': entry['sha256'], 'provenance': evidence, 'text': body})
        imported.append({'ecosystem': key[0], 'name': key[1], 'version': key[2], **evidence})
        issues[:] = [i for i in issues if not (i['ecosystem'] == key[0] and i['name'] == key[1] and i['version'] == key[2] and i['check'] == 'missing-full-license-text')]
    for r in records:
        r['texts'].sort(key=lambda t: (t['file'], t['sha256']))
    return sorted(imported, key=encoded_json)


def select_frontend(records: list[dict], issues: list[dict], inventory_path: Path | None) -> tuple[list[dict], dict | None, list[dict]]:
    if inventory_path is None:
        return records, None, []
    inventory = read_manifest(inventory_path)
    if not isinstance(inventory, dict) or inventory.get('formatVersion') != 1:
        raise GenerationError('frontend-inventory-shape-invalid')
    frontend = ROOT / 'apps' / 'desktop'
    for field, file in [('packageLockSha256', 'package-lock.json'), ('packageManifestSha256', 'package.json')]:
        if inventory.get(field) != hashlib.sha256((frontend / file).read_bytes()).hexdigest():
            raise GenerationError('frontend-inventory-input-mismatch')
    for item in inventory.get('inputs', []):
        verified_file(item.get('path'), item.get('sha256'))
    selected: set[tuple[str, str]] = set()
    lock = read_manifest(frontend / 'package-lock.json')['packages']
    if not isinstance(inventory.get('packages'), list) or not isinstance(inventory.get('chunks'), list) or not inventory['chunks']:
        raise GenerationError('frontend-inventory-shape-invalid')
    for p in inventory['packages']:
        location = p.get('location')
        if location not in lock or not str(location).startswith('node_modules/'):
            raise GenerationError('frontend-inventory-package-not-in-lock')
        installed = read_manifest(frontend / location / 'package.json')
        if p.get('name') != installed.get('name') or p.get('version') != lock[location].get('version') or p.get('version') != installed.get('version'):
            raise GenerationError('frontend-inventory-package-version-mismatch')
        selected.add((p['name'], p['version']))
    for chunk in inventory['chunks'] + inventory.get('assets', []):
        filename = chunk.get('file')
        if not isinstance(filename, str) or filename.startswith('/') or '..' in PurePosixPath(filename).parts:
            raise GenerationError('frontend-inventory-chunk-invalid')
        verified_file('apps/desktop/dist/' + filename, chunk.get('sha256'))
    available = {(r['name'], r['version']) for r in records if r['ecosystem'] == 'npm'}
    if not selected <= available:
        raise GenerationError('frontend-bundled-package-not-in-installed-inventory')
    excluded = sorted(available - selected)
    removed = [i for i in issues if i['ecosystem'] == 'npm' and (i['name'], i['version']) in excluded]
    issues[:] = [i for i in issues if i not in removed]
    scope = {'sha256': hashlib.sha256(inventory_path.read_bytes()).hexdigest(), 'build': inventory.get('build'), 'inputs': sorted(inventory.get('inputs', []), key=encoded_json), 'packages': sorted(inventory['packages'], key=encoded_json), 'chunks': sorted(inventory['chunks'], key=encoded_json), 'assets': sorted(inventory.get('assets', []), key=encoded_json), 'excludedDeclaredPackages': [{'name': n, 'version': v} for n, v in excluded]}
    return [r for r in records if r['ecosystem'] != 'npm' or (r['name'], r['version']) in selected], scope, sorted(removed, key=encoded_json)


def render(records: list[dict], target: str, complete: bool, bundled: bool) -> str:
    chunks = ['PrimerSwitch third-party dependency notices\n',
              f'Target: {target}\n',
              f'Attribution review status: {"no scoped findings" if complete else "REVIEW REQUIRED; see THIRD_PARTY_REPORT.json"}\n',
              'Scope: locked native desktop non-dev dependency closure, conservatively including build/proc-macro dependencies; ' + ('frontend packages contributing to the verified production bundle module inventory.' if bundled else 'declared frontend production closure, including compiler and installed peer dependencies.') + ' Native inventory is not proof of binary reachability or a legal conclusion. System-provided runtimes and installer/bootstrapper redistribution terms require separate packaging review.\n',
              'License expressions are package-declared metadata. All discovered package license and notice files follow in full. No alternative license has been selected or invented.\n']
    for r in records:
        chunks.append('\n' + '=' * 78 + '\n')
        chunks.append(f"{r['ecosystem']}: {r['name']} {r['version']}\nDeclared license: {r['license'] or 'NOT DECLARED'}\n")
        for field in ('repository', 'source'):
            if r[field]:
                chunks.append(f'{field.capitalize()}: {r[field]}\n')
        if r.get('sourceArchive'):
            chunks.append(f"Source archive: {r['sourceArchive']}\nArchive SHA256: {r['sourceArchiveSha256']}\n")
        if r.get('pinnedCommit'):
            chunks.append(f"Upstream revision: {r['pinnedCommit']}\n")
        if r.get('cachedSourceMatchesLockedArchive'):
            chunks.append('Cached source verified unmodified against the locked upstream archive.\n')
        if not r['texts']:
            chunks.append('MISSING LOCAL FULL LICENSE TEXT; review required.\n')
        for t in r['texts']:
            chunks.append(f"\n--- {t['file']} ---\n")
            if 'provenance' in t:
                chunks.append(f"Upstream source: {t['provenance']['url']}\nPinned revision: {t['provenance']['pinnedCommit']}\nSource SHA256: {t['sourceSha256']}\n\n")
            chunks.append(t['text'])
            if not t['text'].endswith('\n'):
                chunks.append('\n')
    return ''.join(chunks)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', default='x86_64-pc-windows-msvc', help='locked Cargo target triple')
    parser.add_argument('--overrides', help='offline project-relative verified upstream attribution manifest')
    parser.add_argument('--frontend-inventory', help='verified Vite/Rollup production bundle inventory')
    parser.add_argument('--output-dir', default='.artifacts', help='directory within ignored .artifacts')
    args = parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9_-]+', args.target):
        print('Notice generation failed: invalid-target.', file=sys.stderr)
        return 2
    output = (ROOT / args.output_dir).resolve()
    artifacts = (ROOT / '.artifacts').resolve()
    if not artifacts.is_relative_to(ROOT) or not output.is_relative_to(artifacts):
        print('Notice generation failed: output-must-be-in-artifacts.', file=sys.stderr)
        return 2
    try:
        issues: list[dict] = []
        for label in (args.overrides, args.frontend_inventory):
            if label and (Path(label).is_absolute() or not (ROOT / label).resolve().is_relative_to(ROOT) or (ROOT / label).is_symlink()):
                raise GenerationError('attribution-input-outside-project')
        records = cargo_records(args.target, issues) + npm_records(issues, ROOT / args.frontend_inventory if args.frontend_inventory else None)
        imported = import_overrides(records, issues, ROOT / args.overrides if args.overrides else None)
        records, frontend_scope, excluded_findings = select_frontend(records, issues, ROOT / args.frontend_inventory if args.frontend_inventory else None)
        records.sort(key=lambda r: (r['ecosystem'], r['name'], r['version'], encoded_json(r)))
        by_identity = {(r['ecosystem'], r['name'], r['version']): r for r in records}
        for finding in issues:
            r = by_identity.get((finding['ecosystem'], finding['name'], finding['version']))
            if r and finding['check'] == 'license-expression-review':
                finding['declaredLicense'] = r['license']
                finding['sourceIntegrity'] = r.get('sourceIntegrity', {'status': 'not-verified'})
        issues.sort(key=encoded_json)
        text = render(records, args.target, not issues, frontend_scope is not None)
        input_names = {'Cargo.toml', 'Cargo.lock', 'apps/desktop/package.json', 'apps/desktop/package-lock.json'}
        workspace = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf8'))['workspace']
        for member in workspace['members']:
            for manifest in ROOT.glob(member.rstrip('/') + '/Cargo.toml'):
                if not manifest.resolve().is_relative_to(ROOT) or manifest.is_symlink():
                    raise GenerationError('unsafe-workspace-input')
                input_names.add(manifest.relative_to(ROOT).as_posix())
        inputs = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in sorted(input_names)}
        metadata = {'formatVersion': 1, 'target': args.target, 'nativeFeatures': ['custom-protocol'], 'inputSha256': inputs, 'upstreamOverrides': imported, 'frontendBundleInventory': frontend_scope, 'dependencies': [{k: v for k, v in r.items() if k != 'texts'} | {'texts': [{k: v for k, v in t.items() if k != 'text'} for t in r['texts']]} for r in records]}
        report = {'formatVersion': 1, 'target': args.target, 'dependencyCount': len(records), 'licenseTextCount': sum(len(r['texts']) for r in records), 'cargoDependencyCount': sum(r['ecosystem'] == 'cargo' for r in records), 'npmDependencyCount': sum(r['ecosystem'] == 'npm' for r in records), 'reviewRequired': bool(issues), 'findings': issues, 'excludedCompilerFindings': excluded_findings, 'missingLicenseTextCount': sum(i['check'] == 'missing-full-license-text' for i in issues), 'excludedScope': ['OS-provided runtimes', 'installer/bootstrapper redistribution terms', 'binary reachability determination', 'legal conclusions']}
        if any(PRIVATE_PATH.search(content) for content in (text, encoded_json(metadata), encoded_json(report))):
            raise GenerationError('unsafe-output-local-path')
        output.mkdir(parents=True, exist_ok=True)
        for filename, content in [('THIRD_PARTY_NOTICES.txt', text), ('THIRD_PARTY_METADATA.json', encoded_json(metadata)), ('THIRD_PARTY_REPORT.json', encoded_json(report))]:
            (output / filename).write_text(content, encoding='utf8', newline='\n')
        print(encoded_json({k: report[k] for k in ('target', 'dependencyCount', 'licenseTextCount', 'cargoDependencyCount', 'npmDependencyCount', 'missingLicenseTextCount', 'reviewRequired')}), end='')
        return 1 if issues else 0
    except (GenerationError, OSError, tarfile.TarError, ValueError, KeyError, TypeError) as exc:
        code = str(exc) if isinstance(exc, GenerationError) else 'attribution-input-or-filesystem-error'
        print(f'Notice generation failed: {code}.', file=sys.stderr)
        return 2

if __name__ == '__main__':
    sys.exit(main())
