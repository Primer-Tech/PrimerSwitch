# Implementation and evidence ledger

Updated 2026-10-02. The owner authorized implementation, a new public repository and parallel GPT-6.1 Sol agents. The later UI instruction requires internationalization, English by default and image-model proposals shown before redesign. The owner selected Direction A (dark dashboard); the [design record](../design/PROPOSALS.md) preserves both proposals and the implemented screenshots.

## Current state

The public [Primer-Tech/PrimerSwitch repository](https://github.com/Primer-Tech/PrimerSwitch) has been created. The Windows x64 NSIS preview installer and bundled demo have been verified. Initial source publication and native CI results are being recorded. The private original Swift checkout and its Git history remain separately preserved; its 19 canonical source blobs were verified against upstream commit 4b1b441084fccf5f717fcf41d1b5e668f18559a0.

The Rust workspace implements pure Claude policy, encrypted storage, the Claude HTTP adapter, a serialized runtime, hermetic diagnostics and the Tauri native bridge. The Svelte renderer implements the approved dark dashboard, a light appearance, English/Romanian localization and all account/settings workflows. Codex is a documented future provider, with independent authentication and quota rules.

## Recorded verification

Local evidence is from Windows x64, build 10.0.26200, Rust/Cargo 1.96.1. Fixtures use temporary homes, injected keys and fake HTTP transports. No live sign-in, quota call, token refresh, priming request or reset redemption was performed against user accounts.

The final integrated command was cargo test --workspace --locked --offline: 82 tests passed. Workspace formatting and strict clippy with all targets also passed. The frontend has 25 passing fixtures, zero Svelte errors/warnings and passing formatting and production builds.

| Area | Recorded evidence | Remaining qualification |
| --- | --- | --- |
| Pure core | 22 golden fixtures for policy, schemas, headers, reset outcomes and local renewal dates | Authenticated contract evidence belongs to the provider gate |
| Platform storage | 21 Windows fixtures: DPAPI, current-user/SYSTEM ACLs, atomic replacement, encrypted recovery, conflicting writers, corruption/tamper preservation and model configuration precedence | Native Mac Keychain/fallback; production Linux Secret Service |
| Claude adapter | 11 fake-transport tests for PKCE, matching returned state, refresh rotation/extensions, generic vs exhausted 429, case-insensitive headers, metadata and reset request identity | Current supported-CLI/account compatibility in an isolated dedicated test context |
| Runtime | 23 fixtures for ownership during awaits, freshness, independent cadence/cooldowns, durable reset replay/priming, login cancellation, bounded import review, exact quota reset projections and redacted read-only snapshots | Live provider behavior and native account-switch acceptance |
| Diagnostics | 4 tests; 12 named hermetic checks passed | No production diagnostics that access actual credentials |
| Renderer | 25 controller/UI/i18n fixtures; strict types, format and production build; dark/light/en/ro/narrow/200% scale; Settings/details keyboard focus restored | Packaged UI verification is being recorded below |
| Native Windows | Native bridge/navigation fixture, optimized bundled build and NSIS x64 installer passed; packaged demo IPC/data, en/dark defaults,3 meters, read-only switch rejection, dialog focus restoration and no unexpected network resources verified | Installed notifications/upgrade and isolated authenticated compatibility remain untested |
| macOS/Linux | Initial CI: Ubuntu native/fixtures/clippy passed; macOS arm64 native/fixtures passed; Mac minimum14.0 | Windows elevated-token ACL fixture and macOS cfg clippy repairs being verified; native/live qualification remains separate |

The local preview installer was built with npm run tauri -- build --bundles nsis -- --locked --offline. Its initial SHA256 is 6eb4d108e1436a1a06f2333c23ee16069ee9d86c45c8b2ee5335218d91885937. The process-isolated WebView2 smoke used a temporary data folder and child-only debugging flags, with no development server. Hidden-host pixel capture timed out; renderer pixel and keyboard checks were verified separately. Installer creation is not installation/notification evidence. Third-party notice assembly is in progress before public binary distribution.

Generated demo HTML is excluded from the production renderer. The native executable's --demo selects an in-memory Rust runtime without opening credential stores, detecting a real login or making provider requests. Its fixture mutations are rejected.

## Deliberate changes

Stable IDs replace filename/name identity. Saved credentials and switch journals are encrypted. Rotated inactive credentials are persisted before later operations. Unknown refusal windows do not become unused quota. Decisions require independently fresh ownership, main/model usage and reset status. External login changes invalidate observations. Uncertain reset claims keep the same durable request identity, including when the offer disappears. Login cancellation is independent of the serialized exchange. Imported/provider error text is not trusted for display.

Claude policy defaults remain polling300 seconds, threshold95% and all three automation toggles enabled. Fast active ticks remain inference-only. English and dark are the owner-approved new-install defaults; Romanian and explicit saved system/light/dark settings remain supported. Provider payload names are unchanged. Effective model countdowns follow the binding weekly window, with tied windows requiring both reset dates.

## Work packages and resumption

| Package | State | Next required evidence |
| --- | --- | --- |
| M00-M05 | Implemented with Windows fixture evidence | Isolated supported-CLI native/live acceptance; version matrix |
| M06 | Implemented, renderer visually verified, bundled demo passed | Installed notifications and real native keyboard/window acceptance |
| M07 | Mac storage adapter implemented, native qualification pending | Keychain/fallback/ACL tests, CFPreferences legacy settings import, arm64/x64 validation |
| M08 | Windows unsigned preview built and bundled demo passed | Actual native CI; package attributions; installed smoke; signing/notarization for a later stable release |
| M09 | Lower-priority Linux work planned | Qualified Secret Service backend, distro/desktop/native packages |
| C00-C04 | Planned | Inspect the owner's current Codex switcher and implement the separate versioned adapter |
| PUB00-PUB02 | New public repo and reviewed clean tree prepared | Initial source publication and artifact evidence |

Resume by reading this ledger, AGENTS.md, the implementation/behavior/test plans and the shared development contract. Preserve the original checkout. Keep tests hermetic and qualification evidence separate from compilation. Do not describe fixture coverage as proof of authenticated parity. The current plan's remaining Mac preference import, Linux storage, signing and Codex tasks are explicit; they are not silently counted as delivered.

Initial source commit: 4e6a0795108b584ac6ca22b7e64092f83b8eefea. [First native CI run](https://github.com/Primer-Tech/PrimerSwitch/actions/runs/36998895790): Ubuntu passed; macOS compiled and passed fixtures/UI but failed one cfg-specific clippy condition; Windows exposed the distinction between TokenUser ACL grants and an elevated token's default file owner. The corrected fixture checks TokenUser independently and preserves existing directory/file owners, including atomic replacement. Production DACL behavior is unchanged. Platform fixtures now22/22; strict local platform clippy and formatting passed. Official workflow actions use verified Node24-based current major versions.
