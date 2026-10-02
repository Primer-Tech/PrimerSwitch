# Dependency attribution and preview packaging

PrimerSwitch's new code is MIT licensed. Dependencies and installer components retain their own licenses. The private original Swift application is excluded from this repository and is not re-licensed by it.

Build an unsigned preview on its native operating system:

```sh
npm --prefix apps/desktop ci
python -B scripts/package_preview.py --bundle nsis
# On macOS: use --bundle dmg
```

Python 3.11+, pinned Rust and Node 24 are required. Add --offline when Cargo and bundler dependencies are already cached; --prepare-only verifies attribution without creating an installer. Linux production packaging follows its vault/native qualification.

The command uses the actual Vite production configuration to record rendered Rollup modules and emitted assets. It verifies installed package identities, source/lockfile hashes and output hashes. Positive generated runtime helpers are attributed too. Unattributed assets, external dependencies and stale inventory stop packaging.

The native inventory conservatively includes locked desktop normal/build dependencies and proc macros, excluding dev-only crates. It may include code used only during compilation; it does not assert binary reachability. Supplied license/notice files are preserved in full, including nested attribution. [Pinned upstream texts](upstream/manifest.json) fill omissions in published crate archives without replacing copyright notices or choosing an invented license.

For the unchanged MPL dependencies cssparser, cssparser-macros, dtoa-short, option-ext and selectors, generated notices provide full license text and exact original source archive URLs, archive checksums and repository revisions. Original package contents are verified against those archives. Rustix 1.1.5 also retains all supplied Apache/MIT alternatives, the LLVM exception and copyright files; its exact declared expression is reviewed only with verified unchanged source. The packager accepts only these exact verified review findings; missing shipped texts or other unresolved findings stop the build. Changing a dependency version requires reviewing the corresponding policy and provenance.

The production renderer contains @tauri-apps/api, esm-env, Svelte, Zod and the Vite module-preload helper. is-reference and locate-character declare MIT without supplying full texts upstream; neither contributes rendered code to this bundle. Their missing declarations remain auxiliary compiler evidence and are not disguised as supplied copyright texts.

Windows preview installers use zlib compression and retain the exact NSIS 3.11 COPYING, pinned nsis-tauri-utils 0.5.3 license texts and source links in [installer attribution](packaging/manifest.json). The actual generated compressor, framework COPYING and plugin binary hashes are checked after packaging. These components retain their own terms; complete original NSIS COPYING is preserved, including descriptions of alternative compressors.

Installed preview resources include THIRD_PARTY_NOTICES.txt, THIRD_PARTY_METADATA.json and THIRD_PARTY_REPORT.json, plus installer-notices/. The same files, renderer inventory, package manifest and SHA256SUMS.txt accompany workflow artifacts. Generated outputs stay in ignored .artifacts and are regenerated for the native target. Metadata uses public source URLs and relative paths, never local account-store paths.

OS-provided runtimes are separate from this dependency inventory. Windows uses an installed WebView2 runtime or the standard Microsoft download/bootstrapper flow. Preview artifacts are unsigned. Installed notification identity, upgrades and live provider compatibility remain qualification tasks in the [progress ledger](../migration/PROGRESS.md).
