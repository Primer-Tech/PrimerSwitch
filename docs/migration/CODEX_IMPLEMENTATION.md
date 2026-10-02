# Codex integration implementation contract

Status: authorized by the owner, follows the branded 0.1.1 release. Research is complete; implementation starts next. Resume with AGENTS.md, DEVELOPMENT_CONTRACT.md, PROGRESS.md and CODEX_RESEARCH.md. Original Claude records, settings, polling and provider behavior remain compatible.

## Supported first delivery

Add a real Codex provider in the same branded desktop app, English by default with Romanian. Pin the reviewed compatibility adapter to installed Codex 0.160.0, official commit a956835d020762cb2b570053af06f643a11c0ecc. Discover/verify the native executable without shell/npm launcher evaluation; unknown versions block credential writes. Codex is a separately installed local dependency.

Deliver isolated managed browser login, import of a supported current CLI login, encrypted saved accounts, guarded manual selection for newly opened clients, provider-native quotas and account deletion. Start with qualified FILE-mode managed ChatGPT contexts. Other modes are preserved/represented or clearly refused; never change the user's storage mode to force compatibility. API-key billing, keyring/auto/ephemeral writes, managed-policy exceptions and other versions need their own gates. Device login is optional only if the pinned stable schema and fake lifecycle are implemented. Existing switcher import waits for its actual name/version.

Adding an account uses a private owned CODEX_HOME so the active login is unchanged. Codex owns OAuth/refresh. No separate OpenAI refresh HTTP implementation, external internal-only chatgptAuthTokens API, dummy inference/quota completion, automatic credit consumption or email nudge. No Codex automatic switching or inherited Claude reset/priming policy in this delivery.

## Rust ownership and data preservation

Keep one serialized application mutation owner. Add a Codex state machine under the existing runtime owner and a separately redacted snapshot cache/event; preserve the existing Claude Snapshot and commands. Store Codex accounts in a new authenticated encrypted codex-state record using the same existing master key and application directory. Do not rewrite runtime-state just to enable the provider. Opaque auth bytes/unknown JSON fields belong in Rust and the vault, never IPC or logs.

Platform owns a separate CodexFileStore, not Claude's patching adapter. Qualify canonical target home, effective backend/provider/policies and environment before access; fail closed on unknown requirements. Replace only auth.json, never config/profiles/history/skills/MCP data or whole CODEX_HOME. Detect context and byte/native-generation changes, including identical-byte replacement. Prepare encrypted outgoing/incoming snapshots and journal before replacement. Do not automatically restore old tokens after uncertain external writes.

Require quiescent relevant current-user Codex clients before global selection, recheck immediately before/after mutation and instruct reopening clients. Do not kill user processes or inspect process argv/env/memory. Native process inventory failure blocks switching. A process snapshot cannot atomically stop new clients; the UI must say selection applies to newly opened clients, and retain reconciliation state if a writer appears.

Runtime owns bounded managed-client lifecycle and post-call auth rotation adoption. Adopt durable, owner-bound refreshed credentials before completing the journal. Existing/current account rotations must update the matching encrypted saved record rather than replay stale credentials. Independent backend workspace identity and managed-login evidence differ from unverified JWT routing claims; null/mismatched identity must never become a verified quota or switching proof.

## Redacted desktop contract

Keep Claude's strict DTO unchanged. Add CodexSnapshot with revision, provider, availability/version, allowlisted blockedReason, capability map, saved-account list, selectedId, provider-native login status, busy/error/demo. Add CodexAccountView with stable opaque id, name/email/workspace metadata, auth kind, evidence, selected state, capability, quota/read time/state and safe error. Do not expose executable paths, auth blobs, keys, raw RPC errors or token claims.

Quota retains every native limitId, optional primary/secondary duration/reset, plan, string credit balance, nullable ordinaryUsageAllowed and credit availability. Missing windows stay absent. Do not force Codex into Claude's five-hour/weekly/model/reset fields. Read quotas only through the owned Codex app-server session, initially active/explicit and manual rather than many competing inactive refreshers.

UI owns separate strict schemas/controller, provider selector, Codex dashboard and managed-login/switch dialogs. Keep approved dark branding and global appearance/language. Label existing automation controls Claude-only. Native commands/events use explicit codex prefixes. get_codex_snapshot is cache-only; polling it never invokes provider I/O. Poll managed login completion separately, with cancellation/generation checks, without holding the runtime mutex for browser wait.

## Parallel work packages

| Owner | Paths | Deliverable |
| --- | --- | --- |
| provider agent | crates/provider-codex/** | Native executable discovery/version fingerprint, typed bounded JSONL/stdio client, managed isolated login/cancel/poll, account/read and dynamic quota responses, opaque auth parser, fake transport/lifecycle tests |
| platform agent | crates/switcher-platform/src/codex/** plus coordinated exports/features | Qualified FILE context, exact-byte snapshots/generations, journaled selection/recovery, current-user native process guard, isolated temporary-home and writer/policy/path fixtures |
| UI agent | apps/desktop/** except src-tauri | Separate Codex types/bridge/controller/dashboard/dialogs, provider navigation and en/ro catalogs, meaningful cancellation/quota/error fixtures |
| root | runtime, workspace manifests/lockfiles, native bridge, docs/CI/publishing | Shared owner/cache/persistence, coordinated API contracts, integration fixtures, native matrix, packaging/release evidence |

Agents may not change another owner's files or manifests without coordination. No agent may test against actual user Claude/Codex auth/config/keyrings/providers, execute a real switch, kill clients or publish. Use injected keys/fake process inventories/transports and owned temporary homes. Root remains the integrator.

## Acceptance and handoff

Require opaque/unknown-field preservation, canceled login leaves active state unchanged, schema/version denial, multi-bucket/null quota semantics, secret-sentinel redaction, process appearance/PID reuse/visibility denial, same-byte generation replacement, every journal boundary, external mutation and verified rotation adoption. Integration must prove Claude historical records/settings/ciphertext remain intact after enabling Codex.

Run appropriate workspace formatting/tests/clippy, frontend types/fixtures/build, repository scan and native Windows/macOS/Ubuntu compilation. Actual installed Codex adoption and authenticated behavior remain separate explicit evidence until run in an isolated authorized account context. Preserve source/fixture/native/live distinctions in PROGRESS.md and release notes.
