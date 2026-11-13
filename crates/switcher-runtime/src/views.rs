use serde::{Deserialize, Serialize};
use switcher_core::{AccountAutomation, ProviderId, Settings, UsageResponse};
use switcher_core::{
    CLAIM_REPLAY_FLOOR, ResetKind, ResetOffer, ResetUnavailable, StoredAccount, grant_kind,
    reset_offers,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountAutomationView {
    pub enabled: bool,
    pub threshold: Option<f64>,
    pub priority: Option<u16>,
    pub return_on_five_hour_reset: bool,
}
impl Default for AccountAutomationView {
    fn default() -> Self {
        Self::from(&AccountAutomation::default())
    }
}
impl From<&AccountAutomation> for AccountAutomationView {
    fn from(value: &AccountAutomation) -> Self {
        Self {
            enabled: value.enabled,
            threshold: value.threshold,
            priority: value.priority,
            return_on_five_hour_reset: value.return_on_five_hour_reset,
        }
    }
}
impl From<AccountAutomationView> for AccountAutomation {
    fn from(value: AccountAutomationView) -> Self {
        Self {
            enabled: value.enabled,
            threshold: value.threshold,
            priority: value.priority,
            return_on_five_hour_reset: value.return_on_five_hour_reset,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub poll_interval: u64,
    pub threshold: f64,
    #[serde(default)]
    pub claude_threshold: Option<f64>,
    #[serde(default)]
    pub codex_threshold: Option<f64>,
    pub auto_switch_enabled: bool,
    pub auto_start_window_enabled: bool,
    pub auto_use_resets_enabled: bool,
    pub appearance: String,
    pub language: String,
    /// Claude and Codex: the account whose weekly limit resets soonest goes first.
    pub prefer_soonest_weekly_reset: bool,
}
impl From<&Settings> for SettingsView {
    fn from(s: &Settings) -> Self {
        Self {
            poll_interval: s.poll_interval,
            threshold: s.threshold,
            claude_threshold: s.claude_threshold,
            codex_threshold: s.codex_threshold,
            auto_switch_enabled: s.auto_switch_enabled,
            auto_start_window_enabled: s.auto_start_window_enabled,
            auto_use_resets_enabled: s.auto_use_resets_enabled,
            appearance: s.appearance.clone(),
            language: s.language.clone(),
            prefer_soonest_weekly_reset: s.prefer_soonest_weekly_reset,
        }
    }
}
impl From<SettingsView> for Settings {
    fn from(s: SettingsView) -> Self {
        Self {
            poll_interval: s.poll_interval,
            threshold: s.threshold,
            claude_threshold: s.claude_threshold,
            codex_threshold: s.codex_threshold,
            auto_switch_enabled: s.auto_switch_enabled,
            auto_start_window_enabled: s.auto_start_window_enabled,
            auto_use_resets_enabled: s.auto_use_resets_enabled,
            appearance: s.appearance,
            language: s.language,
            prefer_soonest_weekly_reset: s.prefer_soonest_weekly_reset,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowView {
    pub utilization: f64,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedLimitView {
    pub label: String,
    pub percent: f64,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageView {
    pub five_hour: WindowView,
    pub seven_day: WindowView,
    pub weekly_overall: f64,
    pub weekly_model: f64,
    pub weekly_overall_resets_at: Option<i64>,
    pub weekly_model_resets_at: Option<i64>,
    pub scoped_limits: Vec<ScopedLimitView>,
    /// The provider's own weekly Fable row, whatever model is selected. `None` means the
    /// reading has no such row: unknown, never zero, and never another window's value.
    #[serde(default)]
    pub fable: Option<ScopedLimitView>,
}
impl UsageView {
    pub(crate) fn new(u: &UsageResponse, model: Option<&str>) -> Self {
        let weekly_overall_resets_at = u.weekly_all_resets_at();
        let weekly_model_resets_at = match u.scoped_limit(model) {
            Some(scoped) if scoped.percent > u.weekly_utilization() => scoped.resets_at,
            Some(scoped) if scoped.percent == u.weekly_utilization() => weekly_overall_resets_at
                .zip(scoped.resets_at)
                .map(|(overall, model)| overall.max(model)),
            _ => weekly_overall_resets_at,
        };
        Self {
            five_hour: WindowView {
                utilization: u.five_hour.utilization,
                resets_at: u.five_hour.resets_at,
            },
            seven_day: WindowView {
                utilization: u.seven_day.utilization,
                resets_at: u.seven_day.resets_at,
            },
            weekly_overall: u.weekly_utilization(),
            weekly_model: u.weekly_utilization_for_model(model),
            weekly_overall_resets_at,
            weekly_model_resets_at,
            scoped_limits: u
                .scoped_weekly_limits()
                .iter()
                .map(|l| ScopedLimitView {
                    label: l.label().into(),
                    percent: l.percent,
                    resets_at: l.resets_at,
                })
                .collect(),
            fable: u.fable_limit().map(|l| ScopedLimitView {
                label: l.label().into(),
                percent: l.percent,
                resets_at: l.resets_at,
            }),
        }
    }
}
/// When the windows that block `u` at `threshold` reopen: the latest reset among the
/// five-hour window and the binding weekly windows (account-wide and the configured
/// model's scoped row). `None` when nothing blocks or a needed reset time is unknown.
pub(crate) fn frees_at(u: &UsageResponse, threshold: f64, model: Option<&str>) -> Option<i64> {
    let mut needed = vec![];
    if u.five_hour.utilization >= threshold {
        needed.push(u.five_hour.resets_at);
    }
    if u.weekly_utilization() >= threshold {
        needed.push(u.weekly_all_resets_at());
    }
    if let Some(scoped) = u.scoped_limit(model)
        && scoped.percent >= threshold
    {
        needed.push(scoped.resets_at);
    }
    if needed.is_empty() {
        return None;
    }
    needed
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .max()
}

/// A redacted, language-independent error: `code` is a stable UI catalog key, never
/// provider or file text. `param` is only ever a fixed name such as an environment
/// variable; `action` is `autoSwitch` when an automatic switch to `account_id` failed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorView {
    pub code: String,
    pub account_id: Option<String>,
    pub param: Option<String>,
    pub action: Option<String>,
    pub at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetView {
    pub available: Option<u32>,
    pub expires_at: Option<i64>,
    pub cooldown_until: Option<i64>,
    pub pending: bool,
    pub last_outcome: Option<String>,
    /// One entry per kind of reset the account holds, in display order. Neither grant
    /// ids nor request ids are part of it.
    #[serde(default)]
    pub offers: Vec<ResetOfferView>,
}
/// Why a kind of reset cannot be used now. The first nine are the provider's own
/// reasons; `PendingClaim` is this app's: a request is still waiting for its answer.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResetOfferReason {
    Cooldown,
    BlockedFiveHour,
    BlockedSevenDay,
    BlockedOpus,
    BlockedSonnet,
    BlockedOther,
    CannotClear,
    NeedsLimit,
    Unavailable,
    PendingClaim,
}
impl From<ResetUnavailable> for ResetOfferReason {
    fn from(reason: ResetUnavailable) -> Self {
        match reason {
            ResetUnavailable::Cooldown => Self::Cooldown,
            ResetUnavailable::BlockedFiveHour => Self::BlockedFiveHour,
            ResetUnavailable::BlockedSevenDay => Self::BlockedSevenDay,
            ResetUnavailable::BlockedOpus => Self::BlockedOpus,
            ResetUnavailable::BlockedSonnet => Self::BlockedSonnet,
            ResetUnavailable::BlockedOther => Self::BlockedOther,
            ResetUnavailable::CannotClear => Self::CannotClear,
            ResetUnavailable::NeedsLimit => Self::NeedsLimit,
            ResetUnavailable::Unavailable => Self::Unavailable,
        }
    }
}
/// Using this reset now would waste part of a limit that has not been reached.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EarlyUseView {
    /// Share of the limit still left, when the provider says so.
    pub percent_left: Option<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetOfferView {
    pub kind: ResetKind,
    pub resets_left: u32,
    pub resets_total: u32,
    /// The earliest end among the grants of this kind.
    pub ends_at: Option<i64>,
    /// A claim of this kind can be sent now (an early use still needs confirming).
    pub usable_now: bool,
    pub unavailable_reason: Option<ResetOfferReason>,
    pub early_use: Option<EarlyUseView>,
}
impl From<&ResetOffer> for ResetOfferView {
    fn from(offer: &ResetOffer) -> Self {
        Self {
            kind: offer.kind,
            resets_left: offer.resets_left,
            resets_total: offer.resets_total,
            ends_at: offer.ends_at,
            usable_now: offer.usable_now,
            unavailable_reason: offer.unavailable.map(Into::into),
            early_use: offer.early_use.map(|e| EarlyUseView {
                percent_left: e.percent_left,
            }),
        }
    }
}
/// The kinds of reset an account holds and whether each can be used now. A pending
/// claim limits the account to repeating that one request, after the replay floor.
pub(crate) fn offers_for(account: &StoredAccount, now: i64) -> Vec<ResetOfferView> {
    let Some(status) = account.reset_status.as_ref() else {
        return vec![];
    };
    let pending_kind = account
        .pending_reset_claim
        .as_ref()
        .map(|p| grant_kind(status, &p.grant_id));
    let waiting = account
        .last_reset_attempt_at
        .is_some_and(|last| now.saturating_sub(last) < CLAIM_REPLAY_FLOOR);
    reset_offers(status, now)
        .iter()
        .map(|offer| {
            let mut view = ResetOfferView::from(offer);
            if let Some(kind) = pending_kind {
                // The request for this kind was sent but not confirmed; any other kind
                // (or a request whose grant has since vanished) must wait for it.
                view.early_use = None;
                if kind != Some(offer.kind) || waiting {
                    view.usable_now = false;
                    view.unavailable_reason = Some(ResetOfferReason::PendingClaim);
                } else {
                    view.usable_now = true;
                    view.unavailable_reason = None;
                }
            }
            view
        })
        .collect()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub name: String,
    pub email: String,
    /// Display-only workspace name and short identity suffix; never used for matching.
    pub workspace_label: Option<String>,
    pub provider: ProviderId,
    pub automation: AccountAutomationView,
    pub effective_threshold: f64,
    pub active: bool,
    pub is_next: bool,
    pub exhausted: bool,
    pub identity_verified: bool,
    pub usage: Option<UsageView>,
    pub usage_at: Option<i64>,
    pub scoped_at: Option<i64>,
    pub decision_fresh: bool,
    /// Stable catalog key of the last reading failure for this account.
    pub error: Option<String>,
    /// Saved sign-in needs repair; explicit retry may recover a refreshable session.
    pub sign_in_required: bool,
    /// When an exhausted account's blocking windows reopen (see `frees_at`).
    pub frees_at: Option<i64>,
    pub subscription_status: Option<String>,
    pub plan_tier: Option<String>,
    /// Plan kind (`claude_max`, `claude_pro`, `claude_team`, `claude_enterprise`) and
    /// seat tier; short lowercase codes (see `StoredAccount::plan_codes`), or null.
    pub organization_type: Option<String>,
    pub seat_tier: Option<String>,
    pub renewal_day: Option<u8>,
    pub next_renewal_at: Option<i64>,
    pub resets: ResetView,
    pub primed_at: Option<i64>,
    pub five_hour_primed_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub revision: u64,
    pub provider: ProviderId,
    pub accounts: Vec<AccountView>,
    pub active_id: Option<String>,
    pub active_model: Option<String>,
    pub return_hold_until: Option<i64>,
    pub settings: SettingsView,
    pub last_refresh_at: Option<i64>,
    pub busy: bool,
    /// Outcome of the last command or background cycle; replaced by the next one.
    pub error: Option<ErrorView>,
    /// Session notice that stays until the window dismisses it (for example an
    /// interrupted switch found at startup).
    pub notice: Option<ErrorView>,
    pub demo: bool,
    pub consumption_plan: Vec<String>,
    /// Launching a saved account in its own Claude window is available in this build.
    #[serde(default)]
    pub launch_supported: bool,
}
/// Why a dedicated profile must be authorized (again) before it can launch.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProfileAuthReason {
    /// No authorization exists yet.
    FirstUse,
    /// The profile is signed in as another account or workspace.
    IdentityChanged,
    /// The provider no longer accepts the profile's sign-in.
    Expired,
}
/// Result of a launch request. `Cancelled` (no folder chosen) has no side effects.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum LaunchOutcome {
    Launched,
    Authorized,
    Cancelled,
    AuthorizationRequired { reason: ProfileAuthReason },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginSession {
    pub id: String,
    pub url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportPreview {
    pub id: String,
    pub valid: usize,
    pub invalid: usize,
    pub names: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn usage(
        overall: f64,
        model: f64,
        overall_reset: Option<i64>,
        model_reset: Option<i64>,
    ) -> UsageResponse {
        serde_json::from_value(json!({
            "five_hour":{"utilization":20,"resets_at":1000},
            "seven_day":{"utilization":30,"resets_at":overall_reset},
            "limits":[
                {"kind":"weekly_all","group":"weekly","percent":overall,"resets_at":overall_reset},
                {"kind":"weekly_scoped","group":"weekly","percent":model,"resets_at":model_reset,"scope":{"model":{"display_name":"Sonnet"}}}
            ]
        })).unwrap()
    }
    #[test]
    fn model_countdown_uses_the_binding_quota_without_fabricating_unknown_reset() {
        let u = usage(60.0, 100.0, Some(7000), Some(3000));
        let view = UsageView::new(&u, Some("sonnet"));
        assert_eq!(
            (view.weekly_overall, view.weekly_overall_resets_at),
            (60.0, Some(7000))
        );
        assert_eq!(
            (view.weekly_model, view.weekly_model_resets_at),
            (100.0, Some(3000))
        );
        let serialized = serde_json::to_value(&view).unwrap();
        assert_eq!(serialized["weeklyOverallResetsAt"], 7000);
        assert_eq!(serialized["weeklyModelResetsAt"], 3000);
        assert_eq!(
            UsageView::new(&usage(60.0, 100.0, Some(7000), None), Some("sonnet"))
                .weekly_model_resets_at,
            None
        );
        assert_eq!(
            UsageView::new(&usage(80.0, 50.0, Some(7000), Some(3000)), Some("sonnet"))
                .weekly_model_resets_at,
            Some(7000)
        );
        assert_eq!(
            UsageView::new(&u, Some("opus")).weekly_model_resets_at,
            Some(7000)
        );
        assert_eq!(UsageView::new(&u, None).weekly_model_resets_at, Some(7000));
    }
    #[test]
    fn frees_at_waits_for_every_blocking_window_and_never_guesses() {
        // Five-hour exhausted alone: the five-hour reset frees the account.
        let mut u = usage(40.0, 40.0, Some(7000), Some(3000));
        u.five_hour.utilization = 100.0;
        assert_eq!(frees_at(&u, 95.0, Some("sonnet")), Some(1000));
        // Five-hour and the configured model's weekly row both block: the later one wins.
        let mut u = usage(40.0, 97.0, Some(7000), Some(3000));
        u.five_hour.utilization = 96.0;
        assert_eq!(frees_at(&u, 95.0, Some("sonnet")), Some(3000));
        // The scoped row does not block another model.
        assert_eq!(frees_at(&u, 95.0, Some("opus")), Some(1000));
        // Account-wide weekly exhaustion uses the account-wide reset.
        assert_eq!(
            frees_at(&usage(99.0, 10.0, Some(7000), Some(3000)), 95.0, None),
            Some(7000)
        );
        // Unknown blocking reset, or nothing blocking, never invents a time.
        assert_eq!(
            frees_at(&usage(99.0, 10.0, None, Some(3000)), 95.0, None),
            None
        );
        assert_eq!(
            frees_at(&usage(10.0, 10.0, Some(7000), Some(3000)), 95.0, None),
            None
        );
    }
    fn with_limits(limits: Value) -> UsageResponse {
        serde_json::from_value(json!({
            "five_hour":{"utilization":20,"resets_at":1000},
            "seven_day":{"utilization":30,"resets_at":7000},
            "limits":limits
        }))
        .unwrap()
    }
    fn weekly(kind: &str, name: Option<&str>, percent: f64, resets_at: Value) -> Value {
        let mut limit =
            json!({"kind":kind,"group":"weekly","percent":percent,"resets_at":resets_at});
        if let Some(name) = name {
            limit["scope"] = json!({"model":{"display_name":name}});
        }
        limit
    }

    #[test]
    fn fable_is_the_providers_own_row_whichever_model_is_selected() {
        let u = with_limits(json!([
            weekly("weekly_all", None, 62.0, json!(7000)),
            weekly("weekly_scoped", Some("Sonnet"), 90.0, json!(3000)),
            weekly("weekly_scoped", Some("Fable"), 41.5, json!(5000)),
        ]));
        for model in [None, Some("opus"), Some("sonnet"), Some("claude-fable-5-2")] {
            let view = UsageView::new(&u, model);
            let fable = view.fable.as_ref().expect("fable row");
            assert_eq!(
                (fable.label.as_str(), fable.percent, fable.resets_at),
                ("Fable", 41.5, Some(5000)),
                "{model:?}"
            );
            // The overall week and the selected model keep their own values.
            assert_eq!(view.weekly_overall, 62.0);
            assert_eq!(view.scoped_limits.len(), 2);
        }
        // Selected Opus: the model window is the overall week, never Fable's number.
        assert_eq!(UsageView::new(&u, Some("opus")).weekly_model, 62.0);
        let serialized = serde_json::to_value(UsageView::new(&u, None)).unwrap();
        assert_eq!(serialized["fable"]["percent"], 41.5);
        assert_eq!(serialized["fable"]["resetsAt"], 5000);
    }

    #[test]
    fn a_missing_fable_row_is_unknown_never_zero_and_never_another_window() {
        for limits in [
            Value::Null,
            json!([]),
            json!([weekly("weekly_all", None, 62.0, json!(7000))]),
            json!([weekly("weekly_scoped", Some("Sonnet"), 99.0, json!(3000))]),
            // A row for a similar-looking name is not Fable.
            json!([weekly(
                "weekly_scoped",
                Some("Code review"),
                10.0,
                json!(3000)
            )]),
        ] {
            let u = with_limits(limits.clone());
            for model in [None, Some("fable"), Some("sonnet")] {
                let view = UsageView::new(&u, model);
                assert!(view.fable.is_none(), "{limits} {model:?}");
                assert_eq!(serde_json::to_value(&view).unwrap()["fable"], Value::Null);
            }
        }
        // No limits list at all (for example a reading built from response headers).
        let headers_only: UsageResponse = serde_json::from_value(json!({
            "five_hour":{"utilization":20,"resets_at":1000},
            "seven_day":{"utilization":30,"resets_at":7000}
        }))
        .unwrap();
        assert!(UsageView::new(&headers_only, Some("fable")).fable.is_none());
    }

    #[test]
    fn fable_keeps_its_own_reset_and_an_unknown_reset_stays_unknown() {
        let u = with_limits(json!([
            weekly("weekly_all", None, 62.0, json!(7000)),
            weekly("weekly_scoped", Some("Fable"), 12.0, Value::Null),
        ]));
        let fable = UsageView::new(&u, Some("opus")).fable.unwrap();
        assert_eq!((fable.percent, fable.resets_at), (12.0, None));
        // An explicit zero is a reported zero; the reset time is still its own.
        let u = with_limits(json!([
            weekly("weekly_all", None, 62.0, json!(7000)),
            weekly("weekly_scoped", Some("Fable"), 0.0, json!(9000)),
        ]));
        let fable = UsageView::new(&u, None).fable.unwrap();
        assert_eq!((fable.percent, fable.resets_at), (0.0, Some(9000)));
        // The row is matched by the provider's model name or kind, case-insensitively,
        // and only among scoped weekly limits.
        let u = with_limits(json!([
            weekly("weekly_fable", None, 33.0, json!(4000)),
            {"kind":"five_hour_fable","group":"five_hour","percent":99.0,"resets_at":1},
        ]));
        let fable = UsageView::new(&u, None).fable.unwrap();
        assert_eq!((fable.percent, fable.resets_at), (33.0, Some(4000)));
        let u = with_limits(json!([weekly(
            "weekly_scoped",
            Some("FABLE 5"),
            7.0,
            json!(1)
        )]));
        assert_eq!(UsageView::new(&u, None).fable.unwrap().percent, 7.0);
    }

    #[test]
    fn tied_binding_windows_require_both_reset_times_and_show_the_later_one() {
        for (overall, model, expected) in [
            (Some(7000), Some(3000), Some(7000)),
            (Some(3000), Some(7000), Some(7000)),
            (Some(7000), None, None),
            (None, Some(7000), None),
            (None, None, None),
        ] {
            let view = UsageView::new(&usage(80.0, 80.0, overall, model), Some("sonnet"));
            assert_eq!(view.weekly_model, 80.0);
            assert_eq!(view.weekly_model_resets_at, expected);
        }
    }
}
