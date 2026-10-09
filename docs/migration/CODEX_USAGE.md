# Codex accounts in PrimerSwitch

PrimerSwitch keeps several ChatGPT sign-ins for the Codex CLI, shows each account's 5-hour and weekly usage, and switches the active one **without closing your terminals**. Research and evidence: [CODEX_RESEARCH.md](CODEX_RESEARCH.md), [PROGRESS.md](PROGRESS.md).

## Requirements

- Codex CLI **0.160.0 or newer** (0.x). Install or update it with `npm install -g @openai/codex`. PrimerSwitch finds the native executable on PATH or in the global npm package and checks `codex --version`.
- The default FILE credential store (`~/.codex/auth.json`, or `$CODEX_HOME/auth.json`). A `cli_auth_credentials_store` other than `file`, a custom `model_provider`, `forced_login_method`/`forced_chatgpt_workspace_id`, a default `profile`, or custom `chatgpt_base_url`/`openai_base_url` make switching ineffective or unsafe; PrimerSwitch then explains what blocks it and never rewrites your configuration.

## How switching works

Since 0.160.0 every interactive Codex terminal is a client of one shared **app-server daemon** per `CODEX_HOME`, and that daemon holds the loaded sign-in. When you choose **Switch**:

1. PrimerSwitch saves the sign-in that is about to be replaced (Codex rotates tokens on its own, so the latest copy is kept; an unknown login is saved as a new account).
2. It writes the selected account's sign-in to `auth.json` atomically. Nothing else in the Codex home changes.
3. If the daemon is running, it runs Codex's official `codex app-server daemon restart` with the running daemon's own environment, so tools started from your terminals keep the same `PATH`, virtual environments and variables. The daemon finishes running turns first (graceful drain, 60 s by default) and saves its threads; new prompts during the drain are refused with "Server is draining". Codex refuses this restart from an administrator process, so PrimerSwitch running elevated does not switch while the daemon runs.
4. Each open terminal shows **Reconnecting…** for a few seconds, resumes the same conversation with your typed input intact, and continues on the new account. A turn that was cut off at the end of the drain continues automatically.

If no daemon is running, only the file is written and the next Codex session uses the new account.

Processes that hold their own sign-in keep the previous account until they restart: the Codex desktop app, IDE extensions, `codex exec`, and terminals started with `--no-daemon`. PrimerSwitch counts them after a switch and says so. A rare race is handled too: if such a process refreshes its old token right after the switch, Codex merges those tokens into the new `auth.json`; PrimerSwitch notices the mixed file, keeps the tokens with their real account and restores the selected one (restarting the daemon again when needed).

Resumed conversations carry encrypted reasoning that belongs to the previous account. Switching between two personal accounts has been reported to work; if a resumed thread fails after switching between a personal account and a workspace, start a new thread.

### Claude Code jobs through Ultracodex

The [0.2.1 release](https://github.com/Primer-Tech/PrimerSwitch/releases/tag/v0.2.1-preview.1)
includes a separate Ultracodex 0.3.7 compatibility package. Its runner retains
Codex thread history by default. After a managed ChatGPT quota failure it waits
for the selected account to change, then resumes the exact thread under the
same job id, request authentication, model, schema, directory and permissions.
Claude's existing relay keeps polling through the compatible `backoff` state.
Running commands are not interrupted merely because an account changed.

If the new account rejects encrypted reasoning, the runner continues once per
handoff in a fresh thread using the exact thread's visible messages and tool
records, leaving the original rollout intact. This checkpoint excludes hidden
reasoning/config and is bounded; missing essential context must be requested.
The wait is limited to 15 minutes and the original deadline, with at most eight
account handoffs. Cancellation and orphan detection remain active. Explicit
ephemeral jobs, external API-key authentication, missing history and unverified
cleanup cannot be recovered. Already-polled durable jobs from the installed
0.3.6 runner can be adopted once after their supervisor exits with a quota error,
while their original deadline and heartbeat remain valid.

This companion changes the Ultracodex launcher. Native `codex exec` has no live
account reload, and arbitrary other launchers require their own continuation
support. Offline fixtures cover the full handoff and encrypted-history fallback;
no authenticated provider run was used for release verification.

## Add accounts

- **Sign in with ChatGPT…** opens the browser login in a private, temporary Codex home, so adding an account never changes the active one.
- **Import current Codex login** saves whatever `codex login` produced. PrimerSwitch also does this by itself: when it sees a login it does not know in `auth.json`, it keeps it as a new account.
- **Import from Codex Switcher** reads `~/.codex-switcher/accounts.json` from lampese's Codex Switcher (store version 1) and saves its ChatGPT accounts with their names (API-key entries are kept but cannot be selected). If an account exists in both, the copy whose token was issued later wins. Close Codex Switcher afterwards: it also refreshes tokens in the background and force-closes `codex.exe` processes when it switches, so running both makes them invalidate each other's sign-ins.

## Usage readings

The active account is read through your Codex home at the check interval from Settings (5 minutes by default), and every minute while it is within 10 points of the switch threshold or at a limit with automatic switching on; a reading is skipped while its token is about to expire, so Codex refreshes it first. Other accounts are read about once an hour, and sooner after one of their windows resets, each in its own temporary private home; if Codex rotates a token there, the new token is saved. **Refresh** reads one account now; **Refresh all** reads every account. Two rejected readings in a row mark that account **Sign in again** (one can be a temporary refresh failure); it cannot be selected until you sign in once more, and it is retried only every six hours meanwhile. The temporary homes disable Codex's plugin-marketplace sync, so a reading downloads nothing extra.

PrimerSwitch never sends Codex inference requests or logs out an account. Reset credits are consumed only when you choose **Use one reset**. The active account cannot be deleted; switch to another account first. An account with an unconfirmed reset cannot be removed until that request is resolved.

## Manual usage resets (0.2.3 local update)

**Available resets** appears on the Codex dashboard and in each saved ChatGPT account's details, with the same controls for personal, Team and Business plans. **Refresh resets** fetches the separate native reset-credit details as well as usage; the previous count-only poll could leave availability unknown when the usage response omitted it. The service's `availableCount` is authoritative even when detail rows are capped or absent. Unknown availability is displayed explicitly and is never converted to zero or inferred from the plan name.

Available credits also show their expiry date and time in your local timezone, grouped by shared expiry and ordered earliest first. A backend `expiresAt: null` is labeled **Does not expire**. Missing detail rows show **Expiry dates unavailable**; a capped or filtered list explains how many of the available credits have details. Cached readings remain labeled as saved readings. Only sanitized dates reach the renderer; opaque credit identifiers and backend display strings remain private.

Choose **Use one reset** to redeem one available credit for that account. Rust rechecks the backend workspace identity and available count, durably saves an encrypted request key, calls Codex's official `account/rateLimitResetCredit/consume`, then reads the actual usage again. An inactive account uses its private reader; resetting it does not switch the active login. The backend decides which windows are eligible and can return that nothing needs resetting or no credits remain. The button stays disabled when availability is unknown, the reading is stale or no credits exist; eligibility and grants remain controlled by OpenAI.

After an uncertain response, **Check previous reset** reuses the original key, including after restarting PrimerSwitch or when the count has become zero. It cannot consume another credit for the same request. Known successful resets invalidate old percentages even if the subsequent usage read fails. Background polling and automatic switching do not redeem Codex resets.

The protocol follows [official Codex App Server documentation](https://learn.chatgpt.com/docs/app-server#auth-endpoints) and the reviewed 0.160.0 source. Local verification uses fake transports, synthetic account homes and a read-only native demo. It does not redeem a real credit or prove eligibility of a particular workspace.

## Automatic switching

Automatic switching, the switch threshold, the check interval and the weekly reset order in Settings apply to Claude and Codex alike (owner decision, 2026-10-03). Starting the 5-hour and weekly windows and automatic reset use stay Claude-only; manual Codex resets are described above. With automatic switching on, each background check (every minute) decides after its reading: when the active account's reading is fresh (it named this account during this session and is at most 15 minutes old) and either main window, 5-hour or weekly, is at the threshold or Codex reports a limit, PrimerSwitch switches to the next account with exactly the seamless switch described above, so open terminals reconnect. It re-reads the target first when the target's reading is older than 15 minutes. It never switches while a switch or sign-in is running or while the saved accounts are read-only, switches at most once every 10 minutes, and does nothing when no other account is usable. A notification names the new account, why it switched and what open terminals do. A failed automatic switch notifies at most every 30 minutes, and so does the case where every account is at its limit, with the account that frees up first when that is known.

## Which account is next

Rust ranks the Codex accounts with the same rules as Claude, and the **Next up** card shows that account; **Next in order** under the account list shows every usable account in that ranking (the snapshot's `order`, whose first entry is `nextId`), and the list itself puts the active account first, then that order. An account can be next when it is a saved ChatGPT sign-in that can be switched to, its latest reading succeeded, and it reports at least one main window, with every reported window below the threshold and no limit reported (a spent window, a reached-limit flag or blocked included usage). **Some Codex plans report only a weekly window.** From 0.2.1 these accounts participate in both Next up and automatic switching; window duration determines whether a primary or secondary window is short or weekly. Missing windows remain absent in the display and encrypted saved state, and no main windows means availability is unknown. Accounts with every reported window at least 5 points below the threshold come first. Then, with **Use the account whose weekly limit resets soonest first** on (the default), the account whose weekly limit resets soonest (to the hour) goes first, which spends quota that would otherwise expire unused; with it off, the account with the most weekly usage left goes first, then the soonest weekly reset. Unknown weekly usage/reset sorts last when that comparison is needed. Remaining ties go to the lower known short-window usage, then a stable order. In the account list an account that is not usable this way reads **Near limit** when a main window is at or over the threshold but Codex still serves it, and **Limit reached** when Codex reports a real limit (a window at 100%, a reached-limit flag or blocked included usage).

## Credits beyond the plan

When an account keeps working on purchased credits, Codex reports more than 100% used. PrimerSwitch shows the real number, fills the bar to 100% and adds **Using credits** to that window and to the account's status when the account has credits. Such an account counts as limited: it is never next, and when it is the active account automatic switching moves to a usable one, so credits are not spent while included usage is available on another account.

## Data and privacy

Saved sign-ins are encrypted in the PrimerSwitch vault (DPAPI on Windows, Keychain-held key on macOS, Secret Service on Linux) and never reach the window. Temporary homes used for reading inactive accounts are wiped after each reading. Claude accounts, settings and the vault key are unaffected by Codex operations.
