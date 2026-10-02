//! Version-pinned, opaque FILE-mode Codex storage. No network or Codex execution.
mod context;
mod generation;
mod process;
mod store;

pub use context::{
    CodexContextEvidence, CodexFileContext, CodexPolicyState, CodexProviderMode, CodexSourceRole,
    CodexStorageMode, CodexWatchedSource, PINNED_CODEX_SCHEMA, PINNED_CODEX_VERSION,
    create_private_context, read_private_auth,
};
pub use generation::StoreGeneration;
pub use process::{CodexWriteGuard, NativeCodexWriteGuard, macos_policy_is_unrestricted};
pub use store::{
    CodexAuthSnapshot, CodexFileStore, CodexRecovery, PendingCodexSwitch, PreparedCodexSwitch,
};

pub type CodexResult<T> = std::result::Result<T, CodexStoreError>;

/// Safe static explanations only; neither source paths nor auth bytes are displayed.
#[derive(Debug, thiserror::Error)]
pub enum CodexStoreError {
    #[error("Codex context is not qualified for the pinned FILE-mode adapter")]
    UnsupportedContext,
    #[error("Codex credentials are not a supported managed ChatGPT payload")]
    UnsupportedAuth,
    #[error("Codex context or credentials changed externally; reconciliation is required")]
    ExternalChange,
    #[error("Close Codex clients before changing the selected account")]
    RunningClients,
    #[error("Codex process visibility is incomplete; selection is blocked")]
    ProcessVisibility,
    #[error("A Codex transaction requires verified reconciliation")]
    ReconciliationRequired,
    #[error("Codex transaction receipt is invalid or stale")]
    InvalidReceipt,
    #[error(transparent)]
    Storage(#[from] crate::PlatformError),
}
impl From<std::io::Error> for CodexStoreError {
    fn from(_: std::io::Error) -> Self {
        Self::Storage(crate::PlatformError::Io)
    }
}
