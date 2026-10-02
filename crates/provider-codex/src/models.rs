use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CodexAccount {
    #[serde(rename = "chatgpt")]
    Chatgpt {
        email: Option<String>,
        #[serde(rename = "planType")]
        plan_type: String,
    },
    ApiKey {},
    AmazonBedrock {
        #[serde(rename = "usesCodexManagedCredentials", default)]
        uses_codex_managed_credentials: bool,
    },
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountObservation {
    pub account: Option<CodexAccount>,
    pub requires_openai_auth: bool,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRateLimits {
    pub ordinary_usage_allowed: Option<bool>,
    pub account_id: Option<String>,
    pub rate_limits: CodexRateLimitBucket,
    pub rate_limits_by_limit_id: Option<BTreeMap<String, CodexRateLimitBucket>>,
    pub rate_limit_reset_credits: Option<ResetCreditSummary>,
    #[serde(default, flatten)]
    extensions: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRateLimitBucket {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub normal_model_slug: Option<String>,
    pub primary: Option<CodexRateLimitWindow>,
    pub secondary: Option<CodexRateLimitWindow>,
    pub credits: Option<CodexCredits>,
    pub individual_limit: Option<SpendControlLimit>,
    pub spend_control_reached: Option<bool>,
    pub plan_type: Option<String>,
    pub rate_limit_reached_type: Option<String>,
    #[serde(default, flatten)]
    extensions: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRateLimitWindow {
    pub used_percent: i32,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexCredits {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpendControlLimit {
    pub limit: String,
    pub used: String,
    pub remaining_percent: i32,
    pub resets_at: i64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCreditSummary {
    pub available_count: i64,
    pub credits: Option<Vec<ResetCredit>>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredit {
    pub id: String,
    pub reset_type: String,
    pub status: String,
    pub granted_at: i64,
    pub expires_at: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
}
macro_rules! redacted_debug { ($($ty:ty),*) => {$(
impl fmt::Debug for $ty { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(concat!(stringify!($ty), "([REDACTED])")) } }
)*}; }
redacted_debug!(
    CodexAccount,
    AccountObservation,
    CodexRateLimits,
    CodexRateLimitBucket,
    CodexRateLimitWindow,
    CodexCredits,
    SpendControlLimit,
    ResetCreditSummary,
    ResetCredit
);
