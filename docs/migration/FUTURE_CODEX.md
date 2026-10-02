# PrimerSwitch future Codex / OpenAI provider

Status: planned extension, not implemented. Requested by the user on 2026-10-02. Claude feature parity remains the first milestone; the [main architecture](IMPLEMENTATION_PLAN.md) must provide provider seams now so adding Codex does not require another rewrite.

The user currently uses a “codex switcher.” Its exact name/repository has not been supplied. Its name and version are prerequisites for any future compatibility importer. Do not assume a particular competitor, claim superiority, or write a format importer without inspecting the actual tool and its version. The improvements below are proposed acceptance criteria for this application.

## 1. Intended experience

PrimerSwitch manages Claude and Codex accounts in one desktop app, with provider tabs and a combined overview when useful. Each provider has an independently identified active CLI context, account list, usage observations, errors and automation settings. A switch action says which provider/context/account it changes. A Claude switch does not alter Codex, or the reverse. Future providers use the same boundaries after their own compatibility research.

Initial Codex delivery should support managed ChatGPT login, API-key connections, import of the current CLI login, saved-account selection, recoverable manual switching and supported quota display. Show ChatGPT subscription accounts and OpenAI API keys as different authentication/billing modes. API keys may need a user-defined label or project identity; do not fabricate an email/account UUID from a key or infer identity solely by decoding an unverified token.

The same encrypted vault, journal, redacted diagnostics and UI infrastructure can serve both providers. Provider-specific payloads remain separate. Retain organization/workspace constraints, optional plan metadata and unknown fields. Never expose a token, API key or credential blob in the WebView.

Potential improvements to evaluate against the current switcher:

- Recover interrupted switches without corrupting the login or overwriting a newer external login.
- Preserve the CLI's refreshed credentials instead of repeatedly restoring a stale snapshot.
- Display quota age, dynamic limit buckets and unsupported states accurately.
- Support keyring/file storage and managed workspace constraints deliberately.
- Offer Windows/macOS support through the same tested UI; qualify Linux separately.
- Import the existing tool's account records with preview, duplicate detection and an unchanged source directory.

These are design goals. A comparison requires C00 evidence and should acknowledge features the current tool already provides.

## 2. Verified facts and their architectural consequences

Public installation metadata on this Windows machine reports `@openai/codex` **0.160.0**, using `bin/codex.js` and npm shims. No `auth.json`, user configuration, token or active account was read during this investigation.

Official authentication documentation distinguishes ChatGPT sign-in from API-key billing. The CLI/IDE share a login cache; managed OAuth refresh occurs during use. Credentials can use `auth.json` under the effective `CODEX_HOME` (default `~/.codex`) or the OS store. Supported store modes are `file`, `keyring`, `auto`, and `ephemeral`; `auto` may fall back to a file. Login method, workspace and storage can be administratively constrained. These facts require a Codex-specific storage/ownership adapter. They do not prove the ChatGPT desktop app adopts a changed CLI login. [Codex authentication](https://learn.chatgpt.com/docs/auth).

Current config profiles are **configuration overlays**, not saved login accounts. Since 0.134.0, `--profile name` loads a separate `name.config.toml` over base config; the old `[profiles.name]`/top-level selector format is no longer supported. Preserve the user's actual profile layout and do not build account switching around that obsolete format. [Codex profiles](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles).

The official local app-server provides a versioned protocol: generate schemas from the chosen executable, use stdio JSON lines, and send `initialize` then `initialized`. Relevant surfaces include `account/read`, login/cancel/completion, `account/updated`, and rate-limit reads/events. A no-refresh account read uses `refreshToken:false`; managed login delegates token lifecycle to Codex. Dynamic quota fields and optional capabilities must be version-checked. External host-owned ChatGPT tokens are experimental and are not the proposed default. [App-server documentation](https://learn.chatgpt.com/docs/app-server).

The upstream code also defines auth-store configuration and isolated test-home patterns. Recheck source against the selected release rather than copying a changing `main` implementation into a permanent compatibility contract. [Config types](https://github.com/openai/codex/blob/main/codex-rs/config/src/config_toml.rs), [upstream test client](https://github.com/openai/codex/blob/main/codex-rs/app-server-test-client/README.md).

## 3. Integration choice and unresolved global-switch behavior

Prefer a Rust-managed **local sidecar** using the installed, compatible Codex executable and the supported account protocol. Run stdio, with bounded messages, request-ID correlation, timeouts, child-exit handling and redacted stderr. Hide helper console windows on Windows. Do not open a localhost TCP listener or scrape an authenticated browser session to implement basic switching.

At C01, establish the supported integration for this local desktop use, including current Sign in with ChatGPT guidance. The app-server documentation distinguishes local/open-source integrations from commercial/hosted services and directs commercial/hosted uses to the supported Sign in with ChatGPT route. This plan is for a local switcher, not a hosted auth gateway. Reassess that integration if distribution/product scope changes. [Documented integration guidance](https://learn.chatgpt.com/docs/app-server#auth-endpoints).

Two different operations must be designed and labeled separately:

| Operation | Meaning | Main open question |
| --- | --- | --- |
| Global CLI credential switch | Changes the login used by the user's existing effective Codex home/store | Whether a supported persistence API can select a previously managed login, and how already-running CLI/IDE processes adopt it |
| Isolated account launch | Starts an owned Codex process with a selected private credential context | How to share desired user config without duplicating live credentials, histories, permission databases or managed restrictions |

Changing the sidecar's active login alone does not demonstrate that unrelated CLI, IDE or desktop sessions switched. A child environment override does not change the parent shell. Do not market “switch everywhere” until the targeted surfaces have native evidence.

If the official protocol does not expose selection of a stored managed account, implement a narrow versioned active-cache adapter only after inspecting supported source/schema and fixture behavior. Apply the main plan's protected snapshots, journal, fingerprints and conditional recovery to the actual cache/keyring entry. Never use `account/logout` as the first switch step or independently reproduce undocumented OpenAI refresh endpoints. If a compatibility mode is unsupported, report it and provide a documented login/isolated-launch path rather than pretending it changed the global account.

Keyring selection may depend on effective home/context. Verify the service/account identity and fallback behavior per OS/version before creating isolated accounts. Do not change a real user's `cli_auth_credentials_store` to `file` for convenience, and do not replace entire `CODEX_HOME` directories. Keep `config.toml`, profiles, MCP credentials, histories, skills, plugins and permission state intact. Pass any test context override only in a child process environment; do not overwrite the shell's home variables.

## 4. Authentication and account lifecycle

Use provider-tagged auth kinds such as managed ChatGPT and OpenAI API key; actual fields are defined by the selected schema. Browser/device flows use the official CLI/app-server where supported. Device-code support is capability/setting-dependent. If an API-key login must use a CLI fallback, send the key on stdin, never a command argument. Store keys through the vault and clear transient UI input immediately.

Account creation should happen in an isolated, verified credential context so adding one account does not silently replace the user's active account. Respect inherited managed requirements and test that isolation actually holds for the selected keyring backend. Preserve required workspace selection and distinguish the same user in different workspaces as appropriate.

Keep a single credential owner per saved account context. A sidecar's refresh rotation is durably adopted by the switcher; a copied active snapshot is not a second independent refresher. If ownership/adoption cannot be established, require login again for that saved account and show a clear stale-session state. Keep imported records without repeatedly replaying invalidated refresh tokens.

Detect effective environment-key/custom-provider modes and managed login/workspace restrictions before a switch. A config profile's `model_provider`, custom provider or credential environment can determine which backend is used. Preserve that configuration and explain when the selected account cannot affect it. Do not override a managed policy or silently turn a ChatGPT subscription connection into billed API usage.

## 5. Quotas, history and optional automation

The Codex adapter maps dynamic quota buckets into the shared view model, retaining provider-native limit IDs and optional primary/secondary windows. Show supplied durations, resets, plan and credit information; an absent secondary window is absent, not zero. Account token/activity summaries, where supported, are optional history and are not interchangeable with subscription quota or an invoice.

Use read/event operations for quota display; **do not send a completion solely to discover Codex limits**. Start with manual switching and version/capability checks. C03 must establish cadence, cache freshness, workspace scope and shared refresh ownership before offering background reads for many saved accounts.

A future C04 may offer provider-specific automatic account selection after measurements and tests establish usable-window rules. Do not reuse Claude's 95 percent/five-point/weekly-reset ordering by default: configurable Codex policy needs its own explanation, model/bucket applicability and acceptance fixtures. No cross-provider automatic switch or automatic model translation is proposed.

Earned Codex reset credits may be exposed on supported versions. The documented consume operation uses a durable caller idempotency key and opaque optional credit ID. Outcomes distinguish a new reset, an already-completed redemption, no eligible window and no available credit; quota must be reread afterward. Keep these types separate from Claude's grant validation, HTTP endpoint and result strings. [Reset-credit protocol](https://learn.chatgpt.com/docs/app-server), [upstream request implementation](https://github.com/openai/codex/blob/main/codex-rs/tui/src/app/background_requests.rs).

Treat Codex reset redemption as a separately scoped future feature: initially show offers without spending them. Design any eventual user action/automation setting explicitly, with durable replay and account-scoped confirmation/policy. Claude's default-on reset toggle does not apply to Codex. Quota emails, credit purchases, subscription management and cloud/browser account switching are outside this extension; no email-sending method belongs in automatic quota monitoring.

## 6. Future work packages

| Package | Dependencies | Concrete deliverable | Acceptance gate |
| --- | --- | --- | --- |
| C00 — Existing-switcher comparison | Exact tool/version from user; shared model design | Inspect source/storage/actions; feature comparison; sanitized import format and unsupported cases | Evidence-based comparison; no invented competitor schema; migration preserves source records |
| C01 — Protocol/storage spike | M00–M04 contracts; selected Codex release | `provider-codex` skeleton, generated protocol schema evidence, capability matrix, isolated sidecar/path/keyring harness | Verified no-refresh read; isolated login cannot affect default context; exact active-store and managed-policy behavior; no claim of global switch from a sidecar-only result |
| C02 — Manual account management | C00 where importing existing tool, C01, M04/M06 | Browser/device/API-key login, current-account import, encrypted saved records, manual switch/recovery, provider tab | Stale token/adoption, file/keyring/auto/ephemeral, API-key vs ChatGPT, workspace restrictions, canceled login and crash recovery tests; native Windows then Mac |
| C03 — Quota dashboard | C01/C02; qualified read/event semantics | Dynamic multi-bucket display, optional activity, bounded scheduler, freshness, redacted diagnostics | Missing windows/partial lists/errors displayed accurately; no dummy inference; no refresh races across active/sidecar/cache |
| C04 — Optional policy and reset improvements | C03; measured native behavior | Clearly defined Codex automation, optional earned reset workflow, histories/isolated-launch conveniences | Independent policy tests; same-key replay; no implicit Claude reset settings; existing processes' adoption boundaries documented |

C00 can be completed as research when the tool URL is known. The other packages follow the Claude migration; they should not make M00 a complete multi-provider framework project.

## 7. Required evidence before claiming Codex support

- Verify installed protocol rather than assuming every current documentation method exists in 0.160.0 or another selected version. Keep generated schemas/compatibility records free of user data.
- Test the handshake, asynchronous account notifications, bounded errors/timeouts, sidecar restart, mismatched request IDs and cancellation.
- Prove credential-context isolation on each supported OS; test keyring unavailable/fallback and managed requirements with fixtures.
- Verify global active switching and next-request adoption independently in the targeted CLI/IDE surfaces. The ChatGPT desktop/web/cloud login is a separate scope until evidenced.
- Reimport a rotated account without losing its newer credentials or creating duplicates; never log token claims, keys or full auth files.
- Handle API-key accounts without a ChatGPT quota read, and single-window/multi-bucket/unknown-plan responses without fabricated percentages.
- If reset credits are eventually supported, persist request identity before sending and test restart/lost reply/already-completed outcomes against a fake sidecar, without consuming a real entitlement during ordinary tests.

Record the selected Codex version, supported storage modes, native surfaces tested and exact remaining limitations in [PROGRESS.md](PROGRESS.md) at each handoff.
