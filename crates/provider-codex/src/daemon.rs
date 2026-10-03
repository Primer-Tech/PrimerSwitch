//! Codex's shared app-server daemon. Since 0.160.0 every interactive terminal (TUI)
//! is a client of one daemon per CODEX_HOME, and that daemon owns the loaded auth.
//! `codex app-server daemon restart` drains it gracefully (running turns finish, new
//! turns are refused), saves loaded threads and starts a fresh daemon that reads the
//! current auth.json. Terminals reconnect on their own and resume the same thread.
use crate::{CodexError, VerifiedCodexExecutable};
use std::{
    ffi::OsString,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonState {
    Running,
    NotRunning,
}

const STATUS_TIMEOUT: Duration = Duration::from_secs(20);
/// The drain waits for running turns (60 s by default, at most 300 s) and the
/// daemon's own operation lock allows 375 s.
const RESTART_TIMEOUT: Duration = Duration::from_secs(420);
const OUTPUT_LIMIT: usize = 64 * 1024;

/// Probe the daemon for the executable's default CODEX_HOME. A failed probe means no
/// reachable daemon; terminals started later will launch one themselves.
pub async fn daemon_state(executable: &VerifiedCodexExecutable) -> Result<DaemonState, CodexError> {
    Ok(
        match run(executable, "version", STATUS_TIMEOUT, true, None).await? {
            Some(status) if status == "running" => DaemonState::Running,
            _ => DaemonState::NotRunning,
        },
    )
}

/// Restart the running daemon so it loads the current auth.json. Callers must check
/// [`daemon_state`] first: on a stopped daemon this command would start a new one.
/// `environment` should be the running daemon's own environment: the new daemon
/// inherits it, and agent shell commands in every terminal run with it.
pub async fn restart_daemon(
    executable: &VerifiedCodexExecutable,
    environment: Option<&[(OsString, OsString)]>,
) -> Result<(), CodexError> {
    // Never kill the CLI midway: the old daemon may already be stopped.
    match run(executable, "restart", RESTART_TIMEOUT, false, environment).await? {
        Some(status) if status == "restarted" => Ok(()),
        _ => Err(CodexError::DaemonUnavailable),
    }
}

/// Run `codex app-server daemon <command>` hidden. A restarted daemon inherits the
/// environment of this command (Codex normalizes only path variables): the given one,
/// or the user's own. Returns the reported `status`, or None if the command failed.
async fn run(
    executable: &VerifiedCodexExecutable,
    command: &str,
    limit: Duration,
    kill_on_timeout: bool,
    environment: Option<&[(OsString, OsString)]>,
) -> Result<Option<String>, CodexError> {
    let mut process = Command::new(executable.recheck()?);
    if let Some(environment) = environment {
        process
            .env_clear()
            .envs(environment.iter().map(|(name, value)| (name, value)));
    }
    process
        .args(["app-server", "daemon", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(kill_on_timeout);
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_dir())
    {
        process.current_dir(home);
    }
    #[cfg(windows)]
    process.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = process.spawn().map_err(|_| CodexError::SpawnFailed)?;
    let mut stdout = child.stdout.take().ok_or(CodexError::SpawnFailed)?;
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buffer);
    // A detached daemon could inherit the pipe; never wait for EOF, only for exit.
    let reader = tokio::spawn(async move {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            let Ok(mut bytes) = sink.lock() else { break };
            if bytes.len() + n > OUTPUT_LIMIT {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
    });
    let exit = match timeout(limit, child.wait()).await {
        Ok(result) => result.map_err(|_| CodexError::ChildExited)?,
        Err(_) => {
            reader.abort();
            if kill_on_timeout {
                let _ = child.start_kill();
            }
            return Err(CodexError::Timeout);
        }
    };
    let _ = timeout(Duration::from_secs(2), reader).await;
    if !exit.success() {
        return Ok(None);
    }
    let bytes = buffer.lock().map_err(|_| CodexError::Protocol)?.clone();
    Ok(last_status(&bytes))
}

fn last_status(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.lines().rev().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        value["status"]
            .as_str()
            .filter(|status| status.len() <= 32 && status.bytes().all(|b| b.is_ascii_alphabetic()))
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::last_status;
    #[test]
    fn status_is_read_from_the_last_json_line() {
        let output = b"Installing daemon from CLI version 0.160.0 into C:\\x...\n\
{\"status\":\"restarted\",\"backend\":\"pid\",\"pid\":45064,\"socketPath\":\"C:\\\\x\"}\n";
        assert_eq!(last_status(output).as_deref(), Some("restarted"));
        assert_eq!(
            last_status(b"{\"status\":\"running\"}\r\n").as_deref(),
            Some("running")
        );
        assert_eq!(last_status(b"error: no daemon\n"), None);
        assert_eq!(last_status(b"{\"status\":\"<script>\"}\n"), None);
        assert_eq!(last_status(b"\xff\xfe"), None);
    }
}
