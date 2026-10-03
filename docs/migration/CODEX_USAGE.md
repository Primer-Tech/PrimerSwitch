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

## Add accounts

- **Sign in with ChatGPT…** opens the browser login in a private, temporary Codex home, so adding an account never changes the active one.
- **Import current Codex login** saves whatever `codex login` produced. PrimerSwitch also does this by itself: when it sees a login it does not know in `auth.json`, it keeps it as a new account.
- **Import from Codex Switcher** reads `~/.codex-switcher/accounts.json` from lampese's Codex Switcher (store version 1) and saves its ChatGPT accounts with their names (API-key entries are kept but cannot be selected). If an account exists in both, the copy whose token was issued later wins. Close Codex Switcher afterwards: it also refreshes tokens in the background and force-closes `codex.exe` processes when it switches, so running both makes them invalidate each other's sign-ins.

## Usage readings

The active account is read through your Codex home at the check interval from Settings (5 minutes by default), and every minute while it is within 10 points of the switch threshold or at a limit with automatic switching on; a reading is skipped while its token is about to expire, so Codex refreshes it first. Other accounts are read about once an hour, and sooner after one of their windows resets, each in its own temporary private home; if Codex rotates a token there, the new token is saved. **Refresh** reads one account now; **Refresh all** reads every account. Two rejected readings in a row mark that account **Sign in again** (one can be a temporary refresh failure); it cannot be selected until you sign in once more, and it is retried only every six hours meanwhile. The temporary homes disable Codex's plugin-marketplace sync, so a reading downloads nothing extra.

PrimerSwitch never sends inference requests, never consumes reset credits and never logs out an account. The active account cannot be deleted; switch to another account first.

## Automatic switching

Automatic switching, the switch threshold, the check interval and the weekly reset order in Settings apply to Claude and Codex alike (owner decision, 2026-10-03). Starting the weekly window and using resets stay Claude-only. With automatic switching on, each background check (every minute) decides after its reading: when the active account's reading is fresh (it named this account during this session and is at most 15 minutes old) and either main window, 5-hour or weekly, is at the threshold or Codex reports a limit, PrimerSwitch switches to the next account with exactly the seamless switch described above, so open terminals reconnect. It re-reads the target first when the target's reading is older than 15 minutes. It never switches while a switch or sign-in is running or while the saved accounts are read-only, switches at most once every 10 minutes, and does nothing when no other account is usable. A notification names the new account, why it switched and what open terminals do. A failed automatic switch notifies at most every 30 minutes, and so does the case where every account is at its limit, with the account that frees up first when that is known.

## Which account is next

Rust ranks the Codex accounts with the same rules as Claude, and the **Next up** card shows that account; **Next in order** under the account list shows every usable account in that ranking (the snapshot's `order`, whose first entry is `nextId`), and the list itself puts the active account first, then that order. An account can be next when it is a saved ChatGPT sign-in that can be switched to, its latest reading succeeded, and both main windows are below the threshold with no limit reported (a spent window, a reached-limit flag or blocked included usage). Accounts with both windows at least 5 points below the threshold come first. Then, with **Use the account whose weekly limit resets soonest first** on (the default), the account whose weekly limit resets soonest (to the hour) goes first, which spends quota that would otherwise expire unused; with it off, the account with the most weekly usage left goes first, then the soonest weekly reset. Remaining ties go to the lower 5-hour usage, then a stable order.

## Credits beyond the plan

When an account keeps working on purchased credits, Codex reports more than 100% used. PrimerSwitch shows the real number, fills the bar to 100% and adds **Using credits** to that window and to the account's status when the account has credits. Such an account counts as limited: it is never next, and when it is the active account automatic switching moves to a usable one, so credits are not spent while included usage is available on another account.

## Data and privacy

Saved sign-ins are encrypted in the PrimerSwitch vault (DPAPI on Windows, Keychain-held key on macOS, Secret Service on Linux) and never reach the window. Temporary homes used for reading inactive accounts are wiped after each reading. Claude accounts, settings and the vault key are unaffected by Codex operations.
