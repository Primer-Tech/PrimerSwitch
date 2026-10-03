//! Opaque FILE-mode Codex storage (Codex >= 0.160.0). No network or Codex execution.
mod context;
mod generation;
mod process;
mod store;

pub use context::{
    CodexContextEvidence, CodexFileContext, CodexPolicyState, CodexProviderMode, CodexSourceRole,
    CodexStorageMode, CodexWatchedSource, create_private_context, read_private_auth,
    write_private_auth,
};
pub use generation::StoreGeneration;
pub use process::{
    CodexProcessSummary, macos_policy_is_unrestricted, process_environment, process_is_elevated,
    scan_codex_processes,
};
pub use store::{CodexAuthSnapshot, CodexFileStore};

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
    #[error(transparent)]
    Storage(#[from] crate::PlatformError),
}
impl From<std::io::Error> for CodexStoreError {
    fn from(_: std::io::Error) -> Self {
        Self::Storage(crate::PlatformError::Io)
    }
}
