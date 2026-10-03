# Codex accounts in PrimerSwitch

PrimerSwitch keeps several ChatGPT sign-ins for the Codex CLI, shows each account's 5-hour and weekly usage, and switches the active one **without closing your terminals**. Research and evidence: [CODEX_RESEARCH.md](CODEX_RESEARCH.md), [PROGRESS.md](PROGRESS.md).

## Requirements

- Codex CLI **0.160.0 or newer** (0.x). Install or update it with `npm install -g @openai/codex`. PrimerSwitch finds the native executable on PATH or in the global npm package and checks `codex --version`.
- The default FILE credential store (`~/.codex/auth.json`, or `$CODEX_HOME/auth.json`). A `cli_auth_credentials_store` other than `file`, a custom `model_provider`, `forced_login_method`/`forced_chatgpt_workspace_id`, a default `profile`, or custom `chatgpt_base_url`/`openai_base_url` make switching ineffective or unsafe; PrimerSwitch then explains what blocks it and never rewrites your configuration.

## How switching works

Since 0.160.0 every interactive Codex terminal is a client of one shared **app-server daemon** per `CODEX_HOME`, and that daemon holds the loaded sign-in. When you choose **Switch**:

1. PrimerSwitch saves the sign-in that is about to be replaced (Codex rotates tokens on its own, so the latest copy is kept; an unknown login is saved as a new account).
2. It writes the selected account's sign-in to `auth.json` atomically. Nothing else in the Codex home changes.
3. If the daemon is running, it runs Codex's official `codex app-server daemon restart`. The daemon finishes running turns first (graceful drain, 60 s by default) and saves its threads; new prompts during the drain are refused with "Server is draining".
4. Each open terminal shows **Reconnecting…** for a few seconds, resumes the same conversation with your typed input intact, and continues on the new account. A turn that was cut off at the end of the drain continues automatically.

If no daemon is running, only the file is written and the next Codex session uses the new account.

Processes that hold their own sign-in keep the previous account until they restart: the Codex desktop app, IDE extensions, `codex exec`, and terminals started with `--no-daemon`. PrimerSwitch counts them after a switch and says so. A rare race is handled too: if such a process refreshes its old token right after the switch, Codex merges those tokens into the new `auth.json`; PrimerSwitch notices the mixed file, keeps the tokens with their real account and restores the selected one (restarting the daemon again when needed).

Resumed conversations carry encrypted reasoning that belongs to the previous account. Switching between two personal accounts has been reported to work; if a resumed thread fails after switching between a personal account and a workspace, start a new thread.

## Add accounts

- **Sign in with ChatGPT…** opens the browser login in a private, temporary Codex home, so adding an account never changes the active one.
- **Import current Codex sign-in** saves whatever `codex login` produced. PrimerSwitch also does this by itself: when it sees a login it does not know in `auth.json`, it keeps it as a new account.
- **Import from Codex Switcher** reads `~/.codex-switcher/accounts.json` from lampese's Codex Switcher (store version 1) and saves its ChatGPT accounts with their names (API-key entries are kept but cannot be selected). If an account exists in both, the copy whose token was issued later wins. Close Codex Switcher afterwards: it also refreshes tokens in the background and force-closes `codex.exe` processes when it switches, so running both makes them invalidate each other's sign-ins.

## Usage readings

The active account is read every 10 minutes through your Codex home (skipped while its token is about to expire, so Codex refreshes it first). Other accounts are read about once an hour, and sooner after one of their windows resets, each in its own temporary private home; if Codex rotates a token there, the new token is saved. **Refresh** reads one account now; **Refresh all** reads every account. A rejected sign-in marks that account **Sign in again**; it cannot be selected until you sign in once more.

PrimerSwitch never sends inference requests, never consumes reset credits and never logs out an account. The active account cannot be deleted; switch to another account first.

## Data and privacy

Saved sign-ins are encrypted in the PrimerSwitch vault (DPAPI on Windows, Keychain-held key on macOS, Secret Service on Linux) and never reach the window. Temporary homes used for reading inactive accounts are wiped after each reading. Claude accounts, settings and the vault key are unaffected by Codex operations.
