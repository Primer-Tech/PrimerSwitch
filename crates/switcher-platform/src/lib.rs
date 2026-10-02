//! Native storage and recoverable Claude CLI switching. No provider requests occur here.
mod active;
mod files;
mod paths;
mod protection;
mod vault;

pub use active::{ActiveSnapshot, ActiveStore};
pub use paths::{CliPaths, app_data_dir};
pub use vault::Vault;

pub type Result<T> = std::result::Result<T, PlatformError>;

/// Deliberately excludes paths, file contents and underlying native error text.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("Storage could not be read or written")]
    Io,
    #[error("Stored JSON is invalid; the original file was preserved")]
    InvalidJson,
    #[error("The encrypted vault could not be authenticated; existing files were preserved")]
    Authentication,
    #[error("The operating system credential store is locked or unavailable")]
    KeyUnavailable,
    #[error("The vault master key is missing; existing encrypted files were preserved")]
    KeyLost,
    #[error(
        "This Linux installation has no qualified Secret Service backend; saved credentials are unavailable"
    )]
    UnsupportedSecretService,
    #[error("This Claude configuration or authentication mode is not supported for switching")]
    UnsupportedContext,
    #[error("A storage operation is already in progress")]
    Busy,
    #[error("Claude credentials changed externally; automatic recovery was stopped")]
    Conflict,
    #[error("The storage path is unsafe")]
    UnsafePath,
    #[error("The credential or identity payload is invalid")]
    InvalidPayload,
}

impl From<std::io::Error> for PlatformError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

impl From<serde_json::Error> for PlatformError {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidJson
    }
}
