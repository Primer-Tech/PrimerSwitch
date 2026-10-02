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
    pub error: Option<String>,
    pub subscription_status: Option<String>,
    pub plan_tier: Option<String>,
    pub renewal_day: Option<u8>,
    pub next_renewal_at: Option<i64>,
    pub resets: ResetView,
    pub primed_at: Option<i64>,
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
    pub error: Option<String>,
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
