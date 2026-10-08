use serde::{Deserialize, Serialize};
use switcher_core::{ProviderId, Settings, UsageResponse};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub poll_interval: u64,
    pub threshold: f64,
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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub name: String,
    pub email: String,
    pub provider: ProviderId,
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
    /// Claude rejected the saved sign-in; only a new browser sign-in restores it.
    pub sign_in_required: bool,
    /// When an exhausted account's blocking windows reopen (see `frees_at`).
    pub frees_at: Option<i64>,
    pub subscription_status: Option<String>,
    pub plan_tier: Option<String>,
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
    use serde_json::json;
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
