use chrono::{Datelike, Local, LocalResult, NaiveDate, TimeZone, Timelike};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    #[default]
    Claude,
    Codex,
}

/// API timestamps are seconds; CLI credential expiry is read separately in milliseconds.
pub fn parse_timestamp(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|n| n.is_finite() && *n >= i64::MIN as f64 && *n < i64::MAX as f64)
                .map(|n| n.floor() as i64)
        }),
        Value::String(s) => chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.timestamp())
            .or_else(|| s.parse::<i64>().ok())
            .or_else(|| {
                s.parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite() && *n >= i64::MIN as f64 && *n < i64::MAX as f64)
                    .map(|n| n.floor() as i64)
            }),
        _ => None,
    }
}
fn timestamp<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(parse_timestamp(&Value::deserialize(d)?))
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct StoredAccount {
    pub id: String,
    pub provider: ProviderId,
    pub name: String,
    pub saved_at: i64,
    pub oauth_account: Value,
    pub credentials: Value,
    pub refresh_fail_at: Option<i64>,
    pub rate_limited_until: Option<i64>,
    pub expected_weekly_reset_at: Option<i64>,
    pub primed_for_reset_at: Option<i64>,
    pub consecutive_rate_limits: u32,
    pub last_usage: Option<UsageResponse>,
    pub last_usage_at: Option<i64>,
    pub last_endpoint_read_at: Option<i64>,
    pub last_endpoint_attempt_at: Option<i64>,
    pub subscription_status: Option<String>,
    pub subscription_started_at: Option<i64>,
    pub plan_tier: Option<String>,
    pub profile_checked_at: Option<i64>,
    pub renewal_day: Option<u8>,
    pub reset_status: Option<ResetStatus>,
    pub reset_status_at: Option<i64>,
    pub pending_reset_claim: Option<PendingResetClaim>,
    pub last_reset_outcome: Option<String>,
    pub last_reset_attempt_at: Option<i64>,
    pub identity_verified: bool,
    pub priming_pending_for: Option<i64>,
}
impl fmt::Debug for StoredAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredAccount")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("credentials", &"[REDACTED]")
            .field("oauth_account", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}
impl StoredAccount {
    pub fn new(name: impl Into<String>, identity: Value, credentials: Value, now: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            oauth_account: identity,
            credentials,
            saved_at: now,
            ..Self::default()
        }
    }
    pub fn account_uuid(&self) -> Option<&str> {
        self.oauth_account.get("accountUuid")?.as_str()
    }
    pub fn organization_uuid(&self) -> Option<&str> {
        self.oauth_account.get("organizationUuid")?.as_str()
    }
    pub fn email(&self) -> Option<&str> {
        self.oauth_account.get("emailAddress")?.as_str()
    }
    pub fn access_token(&self) -> Option<&str> {
        self.credentials
            .get("claudeAiOauth")?
            .get("accessToken")?
            .as_str()
    }
    pub fn refresh_token(&self) -> Option<&str> {
        self.credentials
            .get("claudeAiOauth")?
            .get("refreshToken")?
            .as_str()
    }
    pub fn expires_at_ms(&self) -> Option<i64> {
        let value = self.credentials.get("claudeAiOauth")?.get("expiresAt")?;
        if value.is_number() {
            parse_timestamp(value)
        } else {
            None
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct UsageWindow {
    pub utilization: f64,
    #[serde(default, deserialize_with = "timestamp")]
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct UsageLimit {
    pub kind: String,
    pub group: String,
    pub percent: f64,
    #[serde(default, deserialize_with = "timestamp")]
    pub resets_at: Option<i64>,
    #[serde(default)]
    pub scope: Option<Value>,
}
impl UsageLimit {
    pub fn label(&self) -> &str {
        self.scope
            .as_ref()
            .and_then(|s| s.get("model"))
            .and_then(|s| s.get("display_name"))
            .and_then(Value::as_str)
            .unwrap_or("total")
    }
}
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct UsageResponse {
    pub five_hour: UsageWindow,
    pub seven_day: UsageWindow,
    pub seven_day_sonnet: Option<UsageWindow>,
    pub extra_usage: Option<Value>,
    pub limits: Option<Vec<UsageLimit>>,
    pub cedar_ember: Option<ResetStatus>,
}
impl<'de> Deserialize<'de> for UsageResponse {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            five_hour: UsageWindow,
            seven_day: UsageWindow,
            seven_day_sonnet: Option<UsageWindow>,
            extra_usage: Option<Value>,
            limits: Option<Vec<UsageLimit>>,
            cedar_ember: Option<Value>,
        }
        let w = Wire::deserialize(d)?;
        if !valid_percent(w.five_hour.utilization)
            || !valid_percent(w.seven_day.utilization)
            || w.seven_day_sonnet
                .as_ref()
                .is_some_and(|w| !valid_percent(w.utilization))
            || w.limits
                .as_ref()
                .is_some_and(|ls| ls.iter().any(|l| !valid_percent(l.percent)))
        {
            return Err(serde::de::Error::custom("Invalid utilization"));
        }
        Ok(Self {
            five_hour: w.five_hour,
            seven_day: w.seven_day,
            seven_day_sonnet: w.seven_day_sonnet,
            extra_usage: w.extra_usage,
            limits: w.limits,
            cedar_ember: w.cedar_ember.and_then(|v| serde_json::from_value(v).ok()),
        })
    }
}
pub(crate) fn valid_percent(n: f64) -> bool {
    n.is_finite() && n >= 0.0
}
impl UsageResponse {
    pub fn weekly_utilization(&self) -> f64 {
        self.limits
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|l| l.group == "weekly" && l.kind == "weekly_all")
            .map_or(self.seven_day.utilization, |l| l.percent)
    }
    pub fn scoped_weekly_limits(&self) -> Vec<&UsageLimit> {
        let mut limits: Vec<_> = self
            .limits
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|l| l.group == "weekly" && l.kind != "weekly_all")
            .collect();
        limits.sort_by(|a, b| {
            b.percent
                .total_cmp(&a.percent)
                .then_with(|| a.label().cmp(b.label()))
        });
        limits
    }
    pub fn scoped_limit(&self, model: Option<&str>) -> Option<&UsageLimit> {
        let needle = model.filter(|s| !s.is_empty())?.to_lowercase();
        self.scoped_weekly_limits().into_iter().find(|l| {
            let name = l.label().to_lowercase();
            !name.is_empty()
                && (needle.starts_with(&name)
                    || name.starts_with(&needle)
                    || needle.contains(&name)
                    || name.contains(&needle))
        })
    }
    pub fn weekly_utilization_for_model(&self, model: Option<&str>) -> f64 {
        self.scoped_limit(model)
            .map_or(self.weekly_utilization(), |l| {
                self.weekly_utilization().max(l.percent)
            })
    }
    pub fn weekly_resets_at(&self) -> Option<i64> {
        self.limits
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|l| l.group == "weekly")
            .filter_map(|l| l.resets_at)
            .chain(self.seven_day.resets_at)
            .min()
    }
    pub fn weekly_all_resets_at(&self) -> Option<i64> {
        self.limits
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|l| l.group == "weekly" && l.kind == "weekly_all")
            .and_then(|l| l.resets_at)
            .or(self.seven_day.resets_at)
    }
    pub fn weekly_reset_bucket(&self) -> Option<i64> {
        self.weekly_resets_at().map(|t| t.div_euclid(3600) * 3600)
    }
    pub fn earliest_reset(&self) -> Option<i64> {
        self.five_hour
            .resets_at
            .into_iter()
            .chain(self.seven_day.resets_at)
            .min()
    }
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ResetGrant {
    pub id: String,
    pub resets_left: u32,
    pub starts_at: Option<i64>,
    pub ends_at: Option<i64>,
    pub clears: Vec<String>,
    pub paused: bool,
    pub usable_now: bool,
    pub use_requires_limit: bool,
}
impl Default for ResetGrant {
    fn default() -> Self {
        Self {
            id: String::new(),
            resets_left: 0,
            starts_at: None,
            ends_at: None,
            clears: vec![],
            paused: false,
            usable_now: false,
            use_requires_limit: true,
        }
    }
}
impl ResetGrant {
    pub fn is_valid_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 40
            && id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
    }
}
impl<'de> Deserialize<'de> for ResetGrant {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let id = v
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| Self::is_valid_id(s))
            .ok_or_else(|| serde::de::Error::custom("Invalid grant"))?
            .to_owned();
        let resets_left = v
            .get("resets_left")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| serde::de::Error::custom("Invalid grant"))?;
        Ok(Self {
            id,
            resets_left,
            starts_at: v.get("starts_at").and_then(parse_timestamp),
            ends_at: v.get("ends_at").and_then(parse_timestamp),
            clears: v
                .get("clears")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            paused: v.get("paused").and_then(Value::as_bool).unwrap_or(false),
            usable_now: v
                .get("usable_now")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            use_requires_limit: v
                .get("use_requires_limit")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        })
    }
}
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct ResetStatus {
    pub eligible: Option<bool>,
    pub ineligible_reason: Option<String>,
    pub grants: Vec<ResetGrant>,
    pub next_grant_id: Option<String>,
    pub cooldown_until: Option<i64>,
    pub weekly_resets_at: Option<i64>,
}
impl<'de> Deserialize<'de> for ResetStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        if !v.is_object() {
            return Err(serde::de::Error::custom("Invalid reset status"));
        }
        Ok(Self {
            eligible: v.get("eligible").and_then(Value::as_bool),
            ineligible_reason: v
                .get("ineligible_reason")
                .and_then(Value::as_str)
                .map(str::to_owned),
            grants: v
                .get("grants")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect()
                })
                .unwrap_or_default(),
            next_grant_id: v
                .get("next_grant_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            cooldown_until: v.get("cooldown_until").and_then(parse_timestamp),
            weekly_resets_at: v.get("weekly_resets_at").and_then(parse_timestamp),
        })
    }
}
impl ResetStatus {
    pub fn available(&self, now: i64) -> Vec<&ResetGrant> {
        if self.eligible == Some(false) || self.cooldown_until.is_some_and(|t| t > now) {
            return vec![];
        }
        self.grants
            .iter()
            .filter(|g| {
                ResetGrant::is_valid_id(&g.id)
                    && g.resets_left > 0
                    && !g.paused
                    && g.starts_at.is_none_or(|t| t <= now)
                    && g.ends_at.is_none_or(|t| t > now)
            })
            .collect()
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PendingResetClaim {
    #[serde(alias = "grantId")]
    pub grant_id: String,
    #[serde(alias = "requestId")]
    pub request_id: String,
    #[serde(alias = "createdAt", deserialize_with = "required_timestamp")]
    pub created_at: i64,
}
fn required_timestamp<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    timestamp(d)?.ok_or_else(|| serde::de::Error::custom("Invalid timestamp"))
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub poll_interval: u64,
    pub threshold: f64,
    pub auto_switch_enabled: bool,
    pub auto_start_window_enabled: bool,
    pub auto_use_resets_enabled: bool,
    pub appearance: String,
    pub language: String,
}
// New-vault dark theme follows the owner-selected Dark A design (2026-10-02).
// Explicit persisted appearance values remain unchanged.
impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_interval: 300,
            threshold: 95.0,
            auto_switch_enabled: true,
            auto_start_window_enabled: true,
            auto_use_resets_enabled: true,
            appearance: "dark".into(),
            language: "en".into(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), crate::CoreError> {
        if !(120..=900).contains(&self.poll_interval)
            || !self.poll_interval.is_multiple_of(30)
            || !self.threshold.is_finite()
            || !(50.0..=100.0).contains(&self.threshold)
            || !matches!(self.appearance.as_str(), "system" | "light" | "dark")
            || !matches!(self.language.as_str(), "en" | "ro")
        {
            return Err(crate::CoreError::InvalidSettings);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct AccountUsageInfo {
    pub account: StoredAccount,
    pub usage: Option<UsageResponse>,
    pub is_active: bool,
}

/// Local calendar renewal estimate. The absent-anchor fallback is stable midnight.
pub fn next_renewal(account: &StoredAccount, now: i64) -> Option<i64> {
    let day = account.renewal_day.filter(|d| (1..=31).contains(d))?;
    let current = Local.timestamp_opt(now, 0).single()?;
    let anchor = account
        .subscription_started_at
        .and_then(|t| Local.timestamp_opt(t, 0).single());
    let (hour, minute) = anchor.map_or((0, 0), |a| (a.hour(), a.minute()));
    for offset in 0..=2 {
        let month_index = current.year() * 12 + current.month0() as i32 + offset;
        let year = month_index.div_euclid(12);
        let month = month_index.rem_euclid(12) as u32 + 1;
        let next_index = month_index + 1;
        let end = NaiveDate::from_ymd_opt(
            next_index.div_euclid(12),
            next_index.rem_euclid(12) as u32 + 1,
            1,
        )?
        .pred_opt()?
        .day();
        let naive = NaiveDate::from_ymd_opt(year, month, u32::from(day).min(end))?
            .and_hms_opt(hour, minute, 0)?;
        // DST gaps advance to the first valid minute; repeats use the earlier occurrence.
        for shift in 0..=180 {
            let shifted = naive + chrono::Duration::minutes(shift);
            let candidate = match Local.from_local_datetime(&shifted) {
                LocalResult::Single(t) => Some(t),
                LocalResult::Ambiguous(a, b) => Some(a.min(b)),
                LocalResult::None => None,
            };
            if let Some(t) = candidate {
                if t.timestamp() > now {
                    return Some(t.timestamp());
                }
                break;
            }
        }
    }
    None
}

pub fn import_legacy(value: &Value, now: i64) -> Result<StoredAccount, crate::CoreError> {
    let source = value.as_object().ok_or(crate::CoreError::InvalidLegacy)?;
    let name = source
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or(crate::CoreError::InvalidLegacy)?;
    let identity = source
        .get("oauthAccount")
        .or_else(|| source.get("oauth_account"))
        .filter(|v| v.is_object())
        .ok_or(crate::CoreError::InvalidLegacy)?;
    let credentials = source
        .get("credentials")
        .filter(|v| v.is_object())
        .ok_or(crate::CoreError::InvalidLegacy)?;
    let oauth = credentials
        .get("claudeAiOauth")
        .filter(|v| v.is_object())
        .ok_or(crate::CoreError::InvalidLegacy)?;
    if oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(crate::CoreError::InvalidLegacy);
    }
    let mut normalized = serde_json::Map::new();
    for (key, value) in source {
        let mut snake = String::new();
        for c in key.chars() {
            if c.is_ascii_uppercase() {
                snake.push('_');
                snake.push(c.to_ascii_lowercase());
            } else {
                snake.push(c);
            }
        }
        let is_time = snake.ends_with("_at")
            || snake == "rate_limited_until"
            || snake == "priming_pending_for";
        normalized.insert(
            snake,
            if is_time {
                parse_timestamp(value)
                    .map(Value::from)
                    .unwrap_or(Value::Null)
            } else {
                value.clone()
            },
        );
    }
    normalized.insert(
        "saved_at".into(),
        Value::from(
            source
                .get("savedAt")
                .or_else(|| source.get("saved_at"))
                .and_then(parse_timestamp)
                .unwrap_or(now),
        ),
    );
    // Corrupted caches can be omitted. An unreadable unresolved claim cannot be
    // discarded safely: fail the import and leave the original record intact.
    if normalized.get("pending_reset_claim").is_some_and(|v| {
        !v.is_null() && serde_json::from_value::<PendingResetClaim>(v.clone()).is_err()
    }) {
        return Err(crate::CoreError::InvalidLegacy);
    }
    for field in ["last_usage", "reset_status"] {
        if let Some(v) = normalized.get(field) {
            let valid = match field {
                "last_usage" => serde_json::from_value::<UsageResponse>(v.clone()).is_ok(),
                "reset_status" => serde_json::from_value::<ResetStatus>(v.clone()).is_ok(),
                _ => unreachable!(),
            };
            if !valid {
                normalized.insert(field.into(), Value::Null);
            }
        }
    }
    let mut account: StoredAccount = serde_json::from_value(Value::Object(normalized))
        .map_err(|_| crate::CoreError::InvalidLegacy)?;
    account.name = name.to_owned();
    account.oauth_account = identity.clone();
    account.credentials = credentials.clone();
    if account.id.is_empty() {
        account.id = uuid::Uuid::new_v4().to_string();
    }
    account.identity_verified = false;
    if account.renewal_day.is_some_and(|d| !(1..=31).contains(&d)) {
        account.renewal_day = None;
    }
    Ok(account)
}
