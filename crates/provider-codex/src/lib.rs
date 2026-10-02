//! Pinned Codex managed-auth adapter. Credentials remain opaque in Rust.
mod auth;
mod client;
mod discovery;
mod models;
mod wire;

pub use auth::{AuthKind, OpaqueAuth, UnverifiedRoutingClaims};
pub use client::{
    BrowserLoginChallenge, CancelOutcome, CodexClient, CodexContext, CodexService,
    ConfigurationInspection, LoginPoll,
};
pub use discovery::{
    CodexCandidate, DiscoveryOptions, VerifiedCodexExecutable, discover_candidates,
    verify_executable,
};
pub use models::*;

pub const SUPPORTED_CODEX_VERSION: &str = "0.160.0";
pub const PINNED_SOURCE_COMMIT: &str = "a956835d020762cb2b570053af06f643a11c0ecc";

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
}
