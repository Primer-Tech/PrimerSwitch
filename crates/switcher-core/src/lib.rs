//! Pure account models and Claude quota/reset policy. No I/O or provider requests.
mod manual_reset;
mod models;
mod policy;
mod protocol;
pub use manual_reset::*;
pub use models::*;
pub use policy::*;
pub use protocol::*;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("Invalid settings")]
    InvalidSettings,
    #[error("Invalid legacy account record")]
    InvalidLegacy,
    #[error("Invalid reset identifier")]
    InvalidIdentifier,
    #[error("An unresolved reset claim requires reconciliation")]
    PendingClaim,
}
