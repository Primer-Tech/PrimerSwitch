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
- **Import from Codex Switcher** reads `~/.codex-switcher/accounts.json` from lampese's Codex Switcher (store version 1) and saves its ChatGPT accounts with their names (its API-key entries are kept as API-key accounts). If an account exists in both, the copy whose token was issued later wins. Close Codex Switcher afterwards: it also refreshes tokens in the background and force-closes `codex.exe` processes when it switches, so running both makes them invalidate each other's sign-ins.

- **Add API key…** saves an OpenAI API key under a name you choose. The key is typed into a password field, goes straight to the encrypted vault and never comes back to the window; the list shows only its last four characters. Saving does not switch to it.

## API-key accounts

An API-key account is switched like any other: **Switch** writes the key to `auth.json` in the same way as a ChatGPT sign-in (the outgoing login is kept first), then restarts the Codex daemon. Switch back to a ChatGPT account the same way. Use is billed per token on your OpenAI API account, outside any ChatGPT plan, so there are no plan limits to show: the account has no usage windows, is never read, and **Refresh** is off for it. You choose it with Switch, or you allow the automatic fallback to it (see below). Ordinary automatic switching and preferred returns never select an API key, and an API key is never a source of headroom. A key from a `codex login --with-api-key` is adopted the same way as an unknown ChatGPT login, without a duplicate when the key is already saved. If an older Codex process (before 0.163) finishes a ChatGPT refresh after the key was written, its new tokens are saved to their ChatGPT account and the key stays selected, so switching back still works. Delete removes the saved key; an API-key login that is still active in `auth.json` stays until you switch.

### Flex processing for an API key

In the details of an API key, **Use Flex processing** makes Codex ask for the cheaper, slower Flex tier while that key is the active login. It needs Codex 0.163.0 or newer. A request can fail when Flex capacity is full, and Codex does not retry that error.

PrimerSwitch does this by setting the top-level `service_tier` in `config.toml` of your Codex home to `"flex"` while the key is active, and by putting your old value back (or removing the line) when you switch to ChatGPT or to a key without Flex. It changes only that one value and keeps comments, line endings and all other settings as they were. If the value cannot be set, the switch to the key does not happen; this includes a `service_tier` that is a multi-line string or longer than 256 bytes, which PrimerSwitch does not change. If it cannot be put back, the switch to ChatGPT still happens, you see a warning, and PrimerSwitch tries again every minute. If you change `service_tier` yourself meanwhile, your value is kept.

Only sessions started after the change use the new tier. A session that is already open keeps its tier, and a session started while Flex was on fails on ChatGPT with `Unsupported service_tier: flex`: start a new session. A project `.codex/config.toml` or a command-line setting can override the value.

### Automatic fallback to an API key

In the details of an API key, **Use as fallback** lets PrimerSwitch switch to that key by itself when every ChatGPT account is at its limit, and switch back by itself when one recovers. It is off by default. Turning it on asks you to confirm, because from then on PrimerSwitch can start billing this key without asking again: "Codex bills this key per token, outside your ChatGPT plan. PrimerSwitch will switch to it by itself when every ChatGPT account is at its limit and switch back when one recovers. Set a monthly budget on the OpenAI project." PrimerSwitch cannot see or cap what the key spends (OpenAI offers no usage reading for a key), so the budget on the OpenAI project is the only cap.

The setting only acts while **Automatic switching** is on; the details show a hint when it is off, and the setting can still be saved. PrimerSwitch switches to the key only when all of these hold at the moment of the switch, and it checks them again just before it writes the login:

- the active login is a saved ChatGPT account with a fresh reading (at most 15 minutes old) that is at its threshold or at a limit;
- no other ChatGPT account can take over, and every other ChatGPT account that is included in automatic switching is proven at its limit by a reading whose reset time has not passed. An account that was never read, failed to read, needs a new sign-in, or whose reset time has passed is read first, and nothing is billed until the reading proves it;
- no switch or sign-in is running, the setup is not blocked, and the last automatic switch was more than 10 minutes ago;
- at least one key has the fallback on, is included in automatic switching, has no error and can be switched to (a Flex key also needs Codex 0.163.0 or newer, and its Flex setting is applied before the login is written). With several such keys, the one with the lowest priority number is used, then the lowest id.

If any condition fails, nothing changes and you get the usual "all accounts are at their limit" notice. If the automatic switch itself fails, you get one notice and PrimerSwitch does not try again within the 10-minute cooldown. When it works you get a notification: "Every ChatGPT account is at its limit. Switched to API key X. Codex now bills per token. PrimerSwitch returns by itself when an account recovers."

While the key is active, the Codex page shows a banner: "Using API key X (billed per token) since HH:MM because every ChatGPT account is at its limit. PrimerSwitch returns automatically when one recovers." **Switch back now** switches to a ChatGPT account at once. PrimerSwitch keeps reading the ChatGPT accounts meanwhile (within about two minutes once a window's reset time has passed, otherwise hourly). It switches back when a ChatGPT account has a fresh reading with headroom (the same ranking as ordinary automatic switching), the cooldown has passed, and, if you picked the key yourself, a manual hold of 15 minutes has passed. If the best account's reading is stale, it is read first. The notification says "Account Y has usage again. Switched back from API key X." A switch to any other login, by you or outside the app, ends the fallback. To stay on a key, turn its fallback off.

If the same key has **Use Flex processing** on, both work together, and the details warn you: a Codex session started while the key is active with Flex fails on ChatGPT after the automatic return until you start a new session.

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

Automatic switching, the check interval and the weekly reset order remain shared. Version 0.2.4 gives Claude and Codex independent default thresholds in Settings; account details can inherit that default or override it, exclude an account from automatic switching, and set an explicit priority (1 highest). Starting the 5-hour and weekly windows and automatic reset use stay Claude-only; manual Codex resets are described above. With automatic switching on, each background check (every minute) decides after its reading: when the active account's reading is fresh (it named this account during this session and is at most 15 minutes old) and either reported main window is at that account's effective threshold or Codex reports a limit, PrimerSwitch switches with the seamless transaction described above. It re-reads a stale target first, never switches during another switch/sign-in or while saved accounts are read-only, switches at most once every 10 minutes, and does nothing when no other account is usable, unless an API key with the fallback on can take over (see Automatic fallback to an API key). Notifications explain the account and reason; repeated failures and all-limited notices are throttled.

**Return after 5-hour reset** opts a prioritized account into automatic return after a five-hour window observed above its preferred-headroom ceiling (effective threshold minus five points) naturally recovers. A fresh backend-identified reading must confirm the reset and preferred headroom in every reported main window; a weekly-only account can still be used for normal switching but cannot prove five-hour recovery. Only higher-priority accounts pull the user back. Manual selection pauses returns for 15 minutes, shown with **Resume returns** on the Automation card. The pause, cooldown and observed/handled reset ledger survive restarting PrimerSwitch; banked reset credits are never consumed for this feature.

## Which account is next

Rust ranks the Codex accounts with the same rules as Claude, and the **Next up** card shows that account; **Next in order** under the account list shows every usable account in that ranking (the snapshot's `order`, whose first entry is `nextId`), and the list itself puts the active account first, then that order. An account can be next when it is a saved ChatGPT sign-in that can be switched to, its latest reading succeeded, and it reports at least one main window, with every reported window below the threshold and no limit reported (a spent window, a reached-limit flag or blocked included usage). **Some Codex plans report only a weekly window.** From 0.2.1 these accounts participate in both Next up and automatic switching; window duration determines whether a primary or secondary window is short or weekly. Missing windows remain absent in the display and encrypted saved state, and no main windows means availability is unknown. Accounts with every reported window at least 5 points below the threshold come first. Then, with **Use the account whose weekly limit resets soonest first** on (the default), the account whose weekly limit resets soonest (to the hour) goes first, which spends quota that would otherwise expire unused; with it off, the account with the most weekly usage left goes first, then the soonest weekly reset. Unknown weekly usage/reset sorts last when that comparison is needed. Remaining ties go to the lower known short-window usage, then a stable order. In the account list an account that is not usable this way reads **Near limit** when a main window is at or over the threshold but Codex still serves it, and **Limit reached** when Codex reports a real limit (a window at 100%, a reached-limit flag or blocked included usage).

## Credits beyond the plan

When an account keeps working on purchased credits, Codex reports more than 100% used. PrimerSwitch shows the real number, fills the bar to 100% and adds **Using credits** to that window and to the account's status when the account has credits. Such an account counts as limited: it is never next, and when it is the active account automatic switching moves to a usable one, so credits are not spent while included usage is available on another account.

## Data and privacy

Saved sign-ins are encrypted in the PrimerSwitch vault (DPAPI on Windows, Keychain-held key on macOS, Secret Service on Linux) and never reach the window. Temporary homes used for reading inactive accounts are wiped after each reading. Claude accounts, settings and the vault key are unaffected by Codex operations.
