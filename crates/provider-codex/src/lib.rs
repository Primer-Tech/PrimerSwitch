//! Codex managed-auth adapter. Credentials remain opaque in Rust.
mod auth;
mod client;
mod daemon;
mod discovery;
mod models;
mod wire;

pub use auth::{
    AuthInspection, AuthKind, OpaqueAuth, UnverifiedRoutingClaims, api_key_auth_payload,
    api_key_tail, chatgpt_auth_payload, merge_rotated_tokens, normalize_hybrid, same_api_key,
    split_key_with_tokens, validate_api_key,
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

/// First Codex whose `service_tier = "flex"` setting was verified (against 0.163.0-alpha.2
/// and the 0.163.0 sources): a configured value is sent unchanged in the request.
pub const MIN_FLEX_CODEX_VERSION: &str = "0.163.0";

/// `major.minor.patch` of `codex-cli 0.160.0` or a bare `0.162.0-alpha.10`; the
/// pre-release and build suffixes are ignored.
fn core_version(text: &str) -> Option<((u64, u64, u64), String)> {
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
    Some((parsed, version.to_owned()))
}

/// Parse `codex-cli 0.160.0` or a bare `0.162.0-alpha.10`. Returns the version text
/// when it is at least [`MIN_CODEX_VERSION`] and below 1.0 (an untested major).
pub fn supported_version(text: &str) -> Option<String> {
    let (parsed, version) = core_version(text)?;
    let minimum = (0, 160, 0);
    (parsed.0 == 0 && parsed >= minimum).then_some(version)
}

/// The version is at least [`MIN_FLEX_CODEX_VERSION`] (and below 1.0), so a configured
/// Flex service tier is honoured. A pre-release of that version counts.
pub fn supports_flex(text: &str) -> bool {
    let minimum = core_version(MIN_FLEX_CODEX_VERSION).map(|(parsed, _)| parsed);
    core_version(text).is_some_and(|(parsed, _)| parsed.0 == 0 && Some(parsed) >= minimum)
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
    #[error("The API key is not in the expected format.")]
    InvalidApiKey,
}

#[cfg(test)]
mod version_tests {
    use super::{supported_version, supports_flex};
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
    #[test]
    fn flex_needs_codex_0_163_or_a_prerelease_of_it() {
        for supported in [
            "codex-cli 0.163.0",
            "0.163.0-alpha.2",
            "0.164.1",
            "codex-cli 0.200.0+build",
        ] {
            assert!(supports_flex(supported), "{supported}");
        }
        for unsupported in [
            "codex-cli 0.162.9",
            "0.160.0",
            "0.162.0-alpha.10",
            "1.0.0",
            "codex-cli 0.163",
            "",
            "nonsense",
        ] {
            assert!(!supports_flex(unsupported), "{unsupported}");
        }
    }
}
