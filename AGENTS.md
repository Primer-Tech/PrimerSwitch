# PrimerSwitch implementation instructions

The user authorized implementation, creation of the public Primer-Tech/PrimerSwitch repository, and parallel GPT-6.1 Sol agents on 2026-10-02. Start with docs/migration/PROGRESS.md and IMPLEMENTATION_PLAN.md. Code is MIT. The original unlicensed Swift app and private research are retained separately in the owner's private ClaudeSwitch checkout; do not copy that archive or history into this repository.

Keep credentials/OAuth/provider policies/storage/scheduling in Rust. Use provider-tagged identities and dynamic quotas; Codex remains a future separate adapter. Preserve Claude defaults and rules in BEHAVIOR_SPEC.md, and record deliberate reliability fixes. No raw secrets/response bodies/credential Debug data over IPC or logs. Do not run any tests against real user homes, Claude/Codex auth files, Keychain items or authenticated provider endpoints. Native fixture tests and fake transports are required.

One runtime owner serializes mutations. Refresh only inactive Claude accounts, preserve rotated tokens before other side effects, verify token ownership, retain unrelated JSON, use encrypted journaled switching with external-writer detection. Never restore an ambiguous token snapshot over a newer external login. Persist reset keys before claims and reconcile unknown outcomes without minting another key.

Agents must respect assigned paths and the shared contract in docs/DEVELOPMENT_CONTRACT.md. Do not edit other agents' files or workspace manifests without coordination. Each handoff reports exact checks, OS/version, limitations and changed files. Source inspection, fixtures, compilation and native/live evidence are distinct. Windows first; Mac and Linux support is reported honestly and tested separately.

