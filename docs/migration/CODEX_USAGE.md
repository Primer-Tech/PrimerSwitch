# Codex accounts in PrimerSwitch

The Codex adapter, native commands and desktop workflow are implemented in source. The 0.2.0 candidate passed the hosted integration, native CI and package qualification runs, but the public release is not published and authenticated/live compatibility is not claimed. This guide describes the qualified source workflow and its restrictions. See the [implementation contract](CODEX_IMPLEMENTATION.md), [pinned research](CODEX_RESEARCH.md) and [evidence ledger](PROGRESS.md).

## Set up a separate Codex installation

PrimerSwitch does not bundle Codex. Install **Codex 0.160.0** from the [official release](https://github.com/openai/codex/releases/tag/rust-v0.160.0). For an npm installation, pin the version:

```sh
npm install -g @openai/codex@0.160.0
```

Restart PrimerSwitch after changing the installation or PATH, open the **Codex** tab and choose **Check setup**. The first adapter accepts a qualified local setup using managed ChatGPT sign-in, the default OpenAI provider and **FILE** credential storage. Native discovery checks the actual executable/version; unknown versions are denied rather than treated as compatible.

A different store or policy is not a reason to rewrite your configuration. PrimerSwitch preserves the existing setup and reports restricted capabilities. It does not change `cli_auth_credentials_store` to force compatibility.

## Add or import an account

Use **Add account → Add a Codex account**, then finish sign-in in the browser opened by the native app. Codex manages the authorization callback and refresh lifecycle. PrimerSwitch waits for managed completion; there is no code or token to paste. Login runs in a private, owned credential context, so adding or canceling an account leaves the active Codex sign-in unchanged. Saved opaque credentials remain encrypted in the PrimerSwitch vault and never enter the WebView.

Use **Add account → Import current Codex account** to save the supported current sign-in. Close Codex clients first: import also checks for concurrent writers. Parsed token routing claims identify a candidate record; they do **not** establish verified account/workspace ownership. Import does not perform a provider read automatically. For a ChatGPT import, explicitly **Refresh** the selected account to obtain an owner-bound backend reading before relying on its quota or selecting it later. Missing or mismatched proof leaves the account unverified.

API-key imports are represented separately and encrypted. This delivery does not select API-key accounts, read their billing, offer a key-entry login form or present them as ChatGPT subscriptions. Removing a saved account deletes its PrimerSwitch record; it does not log the current Codex client out.

## Select an account manually

1. Close Codex applications, CLI/TUI sessions, IDE clients and terminals that are using the affected sign-in. Do not leave a background Codex client refreshing it.
2. Choose **Select account** on an eligible saved ChatGPT account. Eligibility uses durable managed-login or previously backend-verified ownership evidence plus native capabilities; it does not require a fresh quota reading in the current app session. Imported claims-only records and API keys are ineligible. PrimerSwitch prepares an encrypted transaction and checks the current context, credentials and process inventory.
3. Acknowledge that the clients are closed, then apply the selection. An expired check requires **Check again**. Canceling the dialog asks native code to discard only its unchanged preparation; a blocked cleanup remains an explicit error.
4. After an authoritative successful selection, reopen Codex clients. Selection applies to **newly opened clients**; it does not retarget existing sessions or change browser, ChatGPT web/desktop or cloud sign-ins.

The native guard checks again around the write, changes only the qualified auth payload, and retains encrypted reconciliation state if the result becomes uncertain. It preserves configuration, profiles, histories, skills and MCP data. It does not kill clients, log out first or blindly restore older credentials over an external sign-in. If the app reports an external change or reconciliation requirement, keep clients closed and check setup again. To adopt an external current login, explicitly import the current account and Refresh it: only a successful independent owner reading and durable refreshed-record save can retire the old journal. Failed proof or a damaged journal remains blocked. Do not treat a canceled dialog or missing success message as proof that active credentials were restored.

## Read quotas

**Refresh** reads only the selected managed ChatGPT account through an owned Codex app-server session. This verification can require closing other clients too. A supported proof includes the matching backend account/workspace identity and a non-null `ordinaryUsageAllowed` value. A percentage or reset date cannot substitute for that proof. `false` means included usage is blocked; unavailable/unknown permission is not inferred to be usable.

Current quota corroboration is separate from durable selection eligibility. An account can remain eligible to select after restarting while its quota is unverified for this session; selection must still verify the actual active workspace/backend before completing its journal.

The dashboard retains every native limit group, optional primary/secondary window, supplied duration/reset, plan and string credit balance. Missing windows and unknown reset times remain missing/unknown. Other saved readings are cached; they are not refreshed in parallel. Reset-credit counts are **read-only**: PrimerSwitch sends no dummy inference, consumes no credits/resets and sends no quota emails.

The existing automation settings apply only to **Claude**. Codex automatic selection, reset consumption and priming are absent. Appearance and English/Romanian language preferences remain global.

## Current restrictions and preservation

Keyring, auto and ephemeral auth stores, unsupported auth types, custom providers/endpoints, credential overrides, active/configured profiles, restricted or unknown managed policy, and other Codex versions are denied by this first compatibility adapter. Configuration is preserved rather than bypassed. Broader support requires separate source, fixture, OS and authenticated qualification gates.

Native process inventory is conservative and can block a change when visibility is incomplete. It cannot identify every arbitrarily renamed/copied/wrapped client, or atomically prevent a new client/concurrent writer from starting between checks. Close all relevant clients yourself; post-write external changes require reconciliation rather than a guarantee of rollback.

Enabling Codex retains the existing application data directory, vault master key, Claude account records and persisted settings. Codex accounts use a separate encrypted **codex-state** record; they are not inserted into Claude policy collections or used to rewrite its saved state. Keep the same OS user and existing vault/key storage when updating; see [upgrade preservation](UPGRADES.md). The 0.2.0 candidate's hosted installer, native CI and installed Ubuntu demo evidence are recorded in [PROGRESS.md](PROGRESS.md).

Importing records from another “Codex switcher” requires its **exact product/repository and version**. Current-account import is not a migration importer for an unnamed tool. Background polling, additional versions/stores, device login, API-key selection/billing and provider-specific automation remain future gates in [the Codex plan](FUTURE_CODEX.md).
