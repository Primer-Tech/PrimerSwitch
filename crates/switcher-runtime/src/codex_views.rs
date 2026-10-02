use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexReason {
    NotInstalled,
    UnsupportedVersion,
    UnsupportedStore,
    UnsupportedAuth,
    PolicyRestricted,
    ExternalCredentials,
    StoreConflict,
    VaultUnavailable,
    IdentityUnverified,
    IdentityMismatch,
    ClientsRunning,
    ProcessInventoryUnavailable,
    ExternalChange,
    ReconciliationRequired,
    LoginExpired,
    LoginCanceled,
    ProviderUnavailable,
    InvalidPreparation,
    Busy,
}
impl std::fmt::Display for CodexReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| std::fmt::Error)?;
        f.write_str(value.as_str().ok_or(std::fmt::Error)?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCapability {
    pub enabled: bool,
    pub blocked_reason: Option<CodexReason>,
}
impl CodexCapability {
    pub(crate) fn allowed() -> Self {
        Self {
            enabled: true,
            blocked_reason: None,
        }
    }
    pub(crate) fn blocked(reason: CodexReason) -> Self {
        Self {
            enabled: false,
            blocked_reason: Some(reason),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCapabilities {
    pub login_browser: CodexCapability,
    pub import_current: CodexCapability,
    pub refresh_quota: CodexCapability,
    pub manual_switch: CodexCapability,
    pub delete_saved: CodexCapability,
}
impl CodexCapabilities {
    pub(crate) fn blocked(reason: CodexReason) -> Self {
        Self {
            login_browser: CodexCapability::blocked(reason),
            import_current: CodexCapability::blocked(reason),
            refresh_quota: CodexCapability::blocked(reason),
            manual_switch: CodexCapability::blocked(reason),
            delete_saved: CodexCapability::blocked(reason),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexAvailability {
    NotInstalled,
    Unqualified,
    Supported,
    Unsupported,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexIdentityEvidence {
    None,
    ClaimsOnly,
    ManagedLogin,
    BackendVerified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexQuotaState {
    Unread,
    Fresh,
    Cached,
    Unavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexWindowView {
    pub used_percent: i32,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCreditsView {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLimitView {
    pub key: String,
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub normal_model_slug: Option<String>,
    pub primary: Option<CodexWindowView>,
    pub secondary: Option<CodexWindowView>,
    pub plan_type: Option<String>,
    pub credits: Option<CodexCreditsView>,
    pub spend_control_reached: Option<bool>,
    pub rate_limit_reached_type: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaView {
    pub ordinary_usage_allowed: Option<bool>,
    pub limits: Vec<CodexLimitView>,
    pub reset_credits_available: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountView {
    pub id: String,
    pub provider: switcher_core::ProviderId,
    pub name: String,
    pub email: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_name: Option<String>,
    pub auth_kind: String,
    pub identity_evidence: CodexIdentityEvidence,
    pub identity_verified: bool,
    pub selected: bool,
    pub manual_switch: CodexCapability,
    pub quota: Option<CodexQuotaView>,
    pub quota_read_at: Option<i64>,
    pub quota_state: CodexQuotaState,
    pub error: Option<CodexReason>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexLoginStatus {
    Waiting,
    Verifying,
    Complete,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLoginView {
    pub id: String,
    pub status: CodexLoginStatus,
    pub error: Option<CodexReason>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSnapshot {
    pub revision: u64,
    pub provider: switcher_core::ProviderId,
    pub availability: CodexAvailability,
    pub blocked_reason: Option<CodexReason>,
    pub executable_version: Option<String>,
    pub accounts: Vec<CodexAccountView>,
    pub selected_id: Option<String>,
    pub active_model: Option<String>,
    pub capabilities: CodexCapabilities,
    pub login: Option<CodexLoginView>,
    pub busy: bool,
    pub error: Option<CodexReason>,
    pub demo: bool,
}
impl CodexSnapshot {
    pub(crate) fn empty(demo: bool) -> Self {
        Self {
            revision: 0,
            provider: switcher_core::ProviderId::Codex,
            availability: CodexAvailability::Unqualified,
            blocked_reason: None,
            executable_version: None,
            accounts: Vec::new(),
            selected_id: None,
            active_model: None,
            capabilities: CodexCapabilities::blocked(CodexReason::NotInstalled),
            login: None,
            busy: false,
            error: None,
            demo,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexPreparationView {
    pub id: String,
    pub account_id: String,
    pub expires_at: i64,
}
