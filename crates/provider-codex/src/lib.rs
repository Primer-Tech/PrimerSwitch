//! Codex managed-auth adapter. Credentials remain opaque in Rust.
mod auth;
mod client;
mod daemon;
mod discovery;
mod models;
mod wire;

pub use auth::{
    AuthInspection, AuthKind, OpaqueAuth, UnverifiedRoutingClaims, api_key_auth_payload,
    chatgpt_auth_payload, merge_rotated_tokens, normalize_hybrid,
};
pub use client::{
    BrowserLoginChallenge, CancelOutcome, CodexClient, CodexContext, CodexService,
    ConfigurationInspection, LoginPoll,
};
pub use daemon::{DaemonState, daemon_state, restart_daemon};
pub use discovery::{
    CodexCandidate, DiscoveryOptions, VerifiedCodexExecutable, discover_candidates,
    verify_executable,
};
pub use models::*;

/// Oldest Codex CLI whose terminals are clients of the shared app-server daemon.
/// Switching rewrites auth.json and restarts that daemon; terminals reconnect.
pub const MIN_CODEX_VERSION: &str = "0.160.0";
/// Release whose sources were inspected for the auth.json and daemon contracts.
pub const PINNED_SOURCE_COMMIT: &str = "a956835d020762cb2b570053af06f643a11c0ecc";

/// Parse `codex-cli 0.160.0` or a bare `0.162.0-alpha.10`. Returns the version text
/// when it is at least [`MIN_CODEX_VERSION`] and below 1.0 (an untested major).
pub fn supported_version(text: &str) -> Option<String> {
    let text = text.trim();
    let version = text.strip_prefix("codex-cli ").unwrap_or(text).trim();
    if version.is_empty() || version.len() > 64 || version.chars().any(char::is_whitespace) {
        return None;
    }
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut number = || -> Option<u64> {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let parsed = (number()?, number()?, number()?);
    if parts.next().is_some() {
        return None;
    }
    let minimum = (0, 160, 0);
    (parsed.0 == 0 && parsed >= minimum).then(|| version.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CodexError {
    #[error("Codex is not installed.")]
    NotFound,
    #[error("The Codex installation is unsupported.")]
    UnsupportedInstallation,
    #[error("This Codex version is unsupported.")]
    UnsupportedVersion,
    #[error("The Codex context is unsafe or unavailable.")]
    UnsafeContext,
    #[error("Codex policy does not permit this operation.")]
    PolicyRestricted,
    #[error("Codex could not complete this request.")]
    ServiceUnavailable,
    #[error("The saved Codex sign-in is no longer valid.")]
    AuthRequired,
    #[error("Could not start Codex.")]
    SpawnFailed,
    #[error("Codex did not respond in time.")]
    Timeout,
    #[error("Codex stopped unexpectedly.")]
    ChildExited,
    #[error("Codex returned an unsupported response.")]
    Protocol,
    #[error("Codex exceeded the response limit.")]
    OutputLimit,
    #[error("Codex sign-in did not complete.")]
    LoginFailed,
    #[error("Codex sign-in was cancelled.")]
    Cancelled,
    #[error("Codex account information is unavailable.")]
    AccountUnavailable,
    #[error("This Codex authentication mode is unsupported.")]
    UnsupportedAuthMode,
    #[error("The Codex account changed. Reconcile it before continuing.")]
    IdentityMismatch,
    #[error("The Codex background server could not be restarted.")]
    DaemonUnavailable,
}

#[cfg(test)]
mod version_tests {
    use super::supported_version;
    #[test]
    fn minimum_version_and_prereleases() {
        assert_eq!(
            supported_version("codex-cli 0.160.0").as_deref(),
            Some("0.160.0")
        );
        assert_eq!(supported_version("0.161.3\n").as_deref(), Some("0.161.3"));
        assert_eq!(
            supported_version("codex-cli 0.162.0-alpha.10").as_deref(),
            Some("0.162.0-alpha.10")
        );
        for rejected in [
            "codex-cli 0.159.9",
            "codex-cli 1.0.0",
            "codex-cli 0.160",
            "codex-cli 0.160.0.1",
            "codex-cli 0.160.x",
            "",
            "codex-cli 0.160.0 extra",
        ] {
            assert_eq!(supported_version(rejected), None, "{rejected}");
        }
    }
}
