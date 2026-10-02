//! Sanitized golden cases derived from BEHAVIOR_SPEC B01-B19 and legacy source
//! symbols Models, SwitchEngine, ResetEngine, WindowStarter at 4b1b441.
//! All timestamps and identities are fixtures; no home, network, or auth access.
use chrono::{Datelike, Local, TimeZone, Timelike};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use switcher_core::*;

fn usage(five: f64, seven: f64, weekly_reset: Option<i64>) -> UsageResponse {
    UsageResponse {
        five_hour: UsageWindow {
            utilization: five,
            resets_at: Some(15000),
        },
        seven_day: UsageWindow {
            utilization: seven,
            resets_at: weekly_reset,
        },
        ..UsageResponse::default()
    }
}
fn info(id: &str, five: f64, seven: f64, reset: Option<i64>, active: bool) -> AccountUsageInfo {
    AccountUsageInfo {
        account: StoredAccount {
            id: id.into(),
            ..StoredAccount::default()
        },
        usage: Some(usage(five, seven, reset)),
        is_active: active,
    }
}
fn grant(id: &str, end: Option<i64>) -> ResetGrant {
    ResetGrant {
        id: id.into(),
        resets_left: 1,
        ends_at: end,
        ..ResetGrant::default()
    }
}
fn with_grant(mut info: AccountUsageInfo, g: ResetGrant) -> AccountUsageInfo {
    info.account.reset_status = Some(ResetStatus {
        grants: vec![g],
        ..ResetStatus::default()
    });
    info
}
fn headers(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| (format!("anthropic-ratelimit-unified-{k}"), v.to_string()))
        .collect()
}

#[test]
fn settings_defaults_and_constraints() {
    let s = Settings::default();
    assert_eq!((s.poll_interval, s.threshold), (300, 95.0));
    assert!(s.auto_switch_enabled && s.auto_start_window_enabled && s.auto_use_resets_enabled);
    assert_eq!(s.appearance, "dark");
    assert_eq!(s.language, "en");
    for appearance in ["system", "light", "dark"] {
        let persisted: Settings = serde_json::from_value(json!({"appearance":appearance})).unwrap();
        assert_eq!(persisted.appearance, appearance);
        assert_eq!(persisted.language, "en");
    }
    assert!(s.validate().is_ok());
    for interval in [0, 119, 121, 901] {
        assert!(
            Settings {
                poll_interval: interval,
                ..s.clone()
            }
            .validate()
            .is_err()
        );
    }
    for threshold in [49.0, 101.0, f64::NAN, f64::INFINITY] {
        assert!(
            Settings {
                threshold,
                ..s.clone()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        Settings {
            appearance: "unknown".into(),
            ..s
        }
        .validate()
        .is_err()
    );
}
#[test]
fn strict_boundary_headroom_and_no_data() {
    let at = info("a", 95.0, 20.0, None, false);
    assert!(!is_usable(&at, 95.0, None));
    assert!(!has_headroom(&at, 95.0, None, 0.0));
    assert!(has_headroom(
        &info("a", 90.0, 90.0, None, false),
        95.0,
        None,
        5.0
    ));
    assert!(!has_headroom(
        &info("a", 100.0, 0.0, None, false),
        100.0,
        None,
        0.0
    ));
    let mut absent = at.clone();
    absent.usage = None;
    assert_eq!(best_candidate(&[absent], 95.0, None), None);
    assert!(!is_usable(
        &info("a", f64::NAN, 0.0, None, false),
        95.0,
        None
    ));
}
#[test]
fn headroom_precedes_perishability_and_fallback_is_retained() {
    let near = info("near", 99.0, 20.0, Some(3600), false);
    let preferred = info("preferred", 94.0, 94.0, Some(7200), false);
    assert_eq!(
        best_candidate(&[near.clone(), preferred], 100.0, None).as_deref(),
        Some("preferred")
    );
    assert_eq!(
        best_candidate(&[near], 100.0, None).as_deref(),
        Some("near")
    );
}
#[test]
fn hour_buckets_usage_missing_last_and_stable_ties() {
    let mut a = info("a", 50.0, 10.0, Some(3700), false);
    let b = info("b", 20.0, 10.0, Some(7100), false);
    assert_eq!(
        best_candidate(&[a.clone(), b.clone()], 95.0, None).as_deref(),
        Some("b")
    );
    a.usage.as_mut().unwrap().seven_day.resets_at = Some(3500);
    assert_eq!(
        best_candidate(&[a.clone(), b.clone()], 95.0, None).as_deref(),
        Some("a")
    );
    a.usage.as_mut().unwrap().seven_day.resets_at = None;
    assert_eq!(
        best_candidate(&[a, b.clone()], 95.0, None).as_deref(),
        Some("b")
    );
    let c = info("c", 20.0, 10.0, Some(7100), false);
    assert_eq!(consumption_plan(&[c, b], 95.0, None), vec!["b", "c"]);
}
#[test]
fn active_in_plan_excluded_as_target() {
    let a = info("a", 20.0, 20.0, Some(3500), true);
    let b = info("b", 20.0, 20.0, Some(7100), false);
    assert_eq!(
        consumption_plan(&[a.clone(), b.clone()], 95.0, None),
        vec!["a", "b"]
    );
    assert_eq!(best_candidate(&[a, b], 95.0, None).as_deref(), Some("b"));
}
#[test]
fn dynamic_model_matching_all_override_and_distinct_reset_times() {
    let u:UsageResponse=serde_json::from_value(json!({"five_hour":{"utilization":12,"resets_at":100},"seven_day":{"utilization":40,"resets_at":8000},"limits":[
        {"kind":"weekly_scoped","group":"weekly","percent":100,"resets_at":3500,"scope":{"model":{"display_name":"Opus 4.8"},"future":true}},
        {"kind":"weekly_all","group":"weekly","percent":60,"resets_at":9000},
        {"kind":"weekly_scoped","group":"weekly","percent":30,"scope":{"model":{"display_name":"Sonnet"}}}
    ]})).unwrap();
    assert_eq!(u.weekly_utilization(), 60.0);
    assert_eq!(
        u.weekly_utilization_for_model(Some("CLAUDE-OPUS 4.8-extra")),
        100.0
    );
    assert_eq!(u.weekly_utilization_for_model(Some("sonnet")), 60.0);
    assert_eq!(u.weekly_utilization_for_model(None), 60.0);
    assert_eq!(u.scoped_weekly_limits()[0].label(), "Opus 4.8");
    assert_eq!(u.weekly_resets_at(), Some(3500));
    assert_eq!(u.weekly_all_resets_at(), Some(9000));
    assert_eq!(u.weekly_reset_bucket(), Some(0));
    assert_eq!(u.earliest_reset(), Some(100));
    assert_eq!(
        u.scoped_limit(Some("OPUS"))
            .unwrap()
            .scope
            .as_ref()
            .unwrap()["future"],
        true
    );
}
#[test]
fn poll_near_limit_uses_model_and_exhaustion_restores_cadence() {
    assert_eq!(
        next_poll_delay(300, Some(&usage(91.0, 92.0, None)), 95.0, None, false),
        30
    );
    assert_eq!(
        next_poll_delay(300, Some(&usage(100.0, 100.0, None)), 95.0, None, true),
        300
    );
    assert_eq!(next_poll_delay(0, None, 95.0, None, false), 120);
}
#[test]
fn timestamp_formats_and_optional_reset_tolerance() {
    let iso = "2024-02-29T12:34:56.123Z";
    assert_eq!(parse_timestamp(&json!(iso)), Some(1709210096));
    assert_eq!(parse_timestamp(&json!(1709210096.5)), Some(1709210096));
    assert_eq!(parse_timestamp(&Value::Null), None);
    let mut raw = json!({"five_hour":{"utilization":29,"resets_at":iso},"seven_day":{"utilization":57,"resets_at":1709210096},"cedar_ember":[]});
    let u: UsageResponse = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(u.cedar_ember, None);
    assert_eq!(u.five_hour.resets_at, u.seven_day.resets_at);
    raw["cedar_ember"] = json!({"grants":[{"id":"valid","resets_left":2},{"id":"../invalid","resets_left":1},{"id":"missing"},{"id":"negative","resets_left":-1}]});
    let status = serde_json::from_value::<UsageResponse>(raw)
        .unwrap()
        .cedar_ember
        .unwrap();
    assert_eq!(status.grants.len(), 1);
    let g = &status.grants[0];
    assert!(!g.usable_now && !g.paused && g.use_requires_limit);
}
#[test]
fn offers_enforce_validity_eligibility_cooldown_and_next_id() {
    let mut s = ResetStatus {
        grants: vec![
            grant("live", Some(1001)),
            grant("expired", Some(1000)),
            ResetGrant {
                starts_at: Some(1001),
                ..grant("future", None)
            },
            ResetGrant {
                paused: true,
                ..grant("paused", None)
            },
            ResetGrant {
                resets_left: 0,
                ..grant("spent", None)
            },
        ],
        ..ResetStatus::default()
    };
    assert_eq!(
        s.available(1000)
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>(),
        vec!["live"]
    );
    s.eligible = Some(false);
    assert!(s.available(1000).is_empty());
    s.eligible = None;
    s.cooldown_until = Some(1001);
    assert!(s.available(1000).is_empty());
    s.cooldown_until = Some(1000);
    assert_eq!(s.available(1000).len(), 1);
    let mut i = with_grant(
        info("active", 100.0, 10.0, None, true),
        grant("early", Some(5000)),
    );
    let status = i.account.reset_status.as_mut().unwrap();
    status.grants.push(grant("next", Some(6000)));
    status.next_grant_id = Some("next".into());
    assert_eq!(
        decide_resets(&[i.clone()], 95.0, None, 1000)[0].grant_id,
        "next"
    );
    status_missing_next(&mut i);
    assert!(decide_resets(&[i], 95.0, None, 1000).is_empty());
}
fn status_missing_next(i: &mut AccountUsageInfo) {
    i.account.reset_status.as_mut().unwrap().next_grant_id = Some("absent".into());
}
#[test]
fn last_resort_requires_no_usable_alternative_and_prefers_active() {
    let a = with_grant(
        info("a", 100.0, 100.0, Some(8000), true),
        grant("a_grant", Some(20000)),
    );
    let b = with_grant(
        info("b", 100.0, 100.0, Some(9000), false),
        grant("b_grant", Some(15000)),
    );
    let result = decide_resets(&[a.clone(), b.clone()], 95.0, None, 1000);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].account_id, "a");
    assert!(!result[0].switch_first);
    assert!(
        decide_resets(
            &[a.clone(), info("free", 10.0, 10.0, None, false)],
            95.0,
            None,
            1000
        )
        .is_empty()
    );
    let mut no_grant = a;
    no_grant.account.reset_status = None;
    let result = decide_resets(&[no_grant, b], 95.0, None, 1000);
    assert_eq!(result[0].account_id, "b");
    assert!(result[0].switch_first);
}
#[test]
fn last_resort_real_limit_and_all_blocking_windows() {
    let g = ResetGrant {
        clears: vec!["five_hour".into()],
        ..grant("g", Some(20000))
    };
    assert!(
        decide_resets(
            &[with_grant(info("a", 100.0, 100.0, None, true), g.clone())],
            95.0,
            None,
            1000
        )
        .is_empty()
    );
    assert!(
        decide_resets(
            &[with_grant(info("a", 95.0, 20.0, None, true), g.clone())],
            95.0,
            None,
            1000
        )
        .is_empty()
    );
    assert_eq!(
        decide_resets(
            &[with_grant(
                info("a", 95.0, 20.0, None, true),
                ResetGrant {
                    use_requires_limit: false,
                    ..g
                }
            )],
            95.0,
            None,
            1000
        )
        .len(),
        1
    );
}
#[test]
fn model_only_binding_needs_model_clear_but_simultaneous_week_uses_compatibility_assumption() {
    let mut i = info("a", 20.0, 20.0, None, true);
    i.usage.as_mut().unwrap().limits = Some(vec![UsageLimit {
        kind: "weekly_scoped".into(),
        group: "weekly".into(),
        percent: 100.0,
        scope: Some(json!({"model":{"display_name":"Opus"}})),
        ..UsageLimit::default()
    }]);
    i = with_grant(i, grant("g", Some(20000)));
    assert!(decide_resets(&[i.clone()], 95.0, Some("opus"), 1000).is_empty());
    i.account.reset_status.as_mut().unwrap().grants[0].clears = vec!["seven_day_opus".into()];
    assert_eq!(
        decide_resets(&[i.clone()], 95.0, Some("opus"), 1000).len(),
        1
    );
    i.usage.as_mut().unwrap().seven_day.utilization = 100.0;
    i.account.reset_status.as_mut().unwrap().grants[0].clears =
        vec!["seven_day_overage_included".into()];
    assert_eq!(decide_resets(&[i], 95.0, Some("opus"), 1000).len(), 1);
}
#[test]
fn expiry_exact_boundary_usable_now_no_switch_and_single_action() {
    let a = info("a", 10.0, 10.0, None, true);
    let mut b = with_grant(
        info("b", 100.0, 10.0, None, false),
        ResetGrant {
            usable_now: true,
            ..grant("g", Some(11800))
        },
    );
    let result = decide_resets(&[a.clone(), b.clone()], 95.0, None, 1000);
    assert_eq!(result.len(), 1);
    assert!(!result[0].switch_first);
    assert_eq!(result[0].reason, "expiră curând");
    b.account.reset_status.as_mut().unwrap().grants[0].ends_at = Some(11801);
    assert!(decide_resets(&[a.clone(), b.clone()], 95.0, None, 1000).is_empty());
    b.account.reset_status.as_mut().unwrap().grants[0].ends_at = Some(11800);
    b.account.reset_status.as_mut().unwrap().grants[0].usable_now = false;
    assert!(decide_resets(&[a, b], 95.0, None, 1000).is_empty());
    let active = with_grant(
        info("x", 100.0, 10.0, None, true),
        ResetGrant {
            usable_now: true,
            ..grant("g", Some(11800))
        },
    );
    assert_eq!(decide_resets(&[active], 95.0, None, 1000).len(), 1);
}
#[test]
fn reset_retry_and_fresh_status_backoff_preserve_pending_key() {
    let mut i = with_grant(info("a", 100.0, 10.0, None, true), grant("g", Some(200000)));
    i.account.last_reset_attempt_at = Some(1000);
    i.account.reset_status_at = Some(1001);
    assert!(decide_resets(&[i.clone()], 95.0, None, 2799).is_empty());
    assert_eq!(decide_resets(&[i.clone()], 95.0, None, 2800).len(), 1);
    i.account.reset_status_at = Some(1000);
    assert!(decide_resets(&[i.clone()], 95.0, None, 2800).is_empty());
    let pending = prepare_reset_claim("g", None, 1000).unwrap();
    i.account.pending_reset_claim = Some(pending.clone());
    assert!(decide_resets(&[i.clone()], 95.0, None, 1299).is_empty());
    assert_eq!(decide_resets(&[i], 95.0, None, 1300).len(), 1);
    assert_eq!(
        prepare_reset_claim("g", Some(&pending), 100000).unwrap(),
        pending
    );
    assert_eq!(
        prepare_reset_claim("other", Some(&pending), 100000),
        Err(CoreError::PendingClaim)
    );
    assert!(prepare_reset_claim("../../x", None, 1).is_err());
}
#[test]
fn verdicts_finality_uncertainty_and_safe_text() {
    for (name, expected) in [
        ("already_used", ResetOutcome::AlreadyUsed),
        ("not_limited", ResetOutcome::NotLimited),
        ("cooldown", ResetOutcome::Cooldown { until: None }),
        ("ineligible", ResetOutcome::Ineligible { reason: None }),
        ("unavailable", ResetOutcome::Unavailable { reason: None }),
        ("future", ResetOutcome::Unavailable { reason: None }),
    ] {
        let o = parse_reset_outcome(400, &json!({"result":name,"reason":"SENTINEL_SECRET"}));
        assert_eq!(o, expected);
        assert!(o.is_final());
        assert!(!o.text().contains("SENTINEL"));
    }
    for status in [429, 500, 503] {
        assert_eq!(
            parse_reset_outcome(status, &json!({"result":"reset"})),
            ResetOutcome::Transport
        );
    }
    for status in [401, 403] {
        assert_eq!(
            parse_reset_outcome(status, &Value::Null),
            ResetOutcome::AuthError
        );
    }
    assert!(!parse_reset_outcome(200, &Value::Null).is_final());
    let o = parse_reset_outcome(
        200,
        &json!({"result":"reset","resets_left":1,"cooldown_until":"2024-02-29T12:34:56Z"}),
    );
    assert_eq!(
        o,
        ResetOutcome::Reset {
            resets_left: Some(1),
            cooldown_until: Some(1709210096)
        }
    );
}
#[test]
fn apply_success_decrement_remaining_next_id_and_cooldown() {
    let mut s = Some(ResetStatus {
        grants: vec![ResetGrant {
            resets_left: 2,
            ..grant("g", None)
        }],
        next_grant_id: Some("g".into()),
        ..ResetStatus::default()
    });
    apply_reset_outcome(
        &mut s,
        "g",
        &ResetOutcome::Reset {
            resets_left: None,
            cooldown_until: Some(100),
        },
    );
    assert_eq!(s.as_ref().unwrap().grants[0].resets_left, 1);
    assert_eq!(s.as_ref().unwrap().next_grant_id.as_deref(), Some("g"));
    assert_eq!(s.as_ref().unwrap().cooldown_until, Some(100));
    apply_reset_outcome(
        &mut s,
        "g",
        &ResetOutcome::Reset {
            resets_left: None,
            cooldown_until: None,
        },
    );
    assert!(s.as_ref().unwrap().grants.is_empty());
    assert_eq!(s.as_ref().unwrap().next_grant_id, None);
    s.as_mut().unwrap().grants.push(grant("g", None));
    s.as_mut().unwrap().next_grant_id = Some("g".into());
    apply_reset_outcome(&mut s, "g", &ResetOutcome::AlreadyUsed);
    assert!(s.as_ref().unwrap().grants.is_empty());
    assert_eq!(s.as_ref().unwrap().next_grant_id, None);
}
#[test]
fn headers_round_reject_invalid_and_match_case() {
    for (fraction, pct) in [
        ("0.29", 29.0),
        ("0.57", 57.0),
        ("0.95", 95.0),
        ("1.0", 100.0),
    ] {
        let h = headers(&[("5h-utilization", fraction), ("7d-utilization", "0.1")]);
        assert_eq!(usage_from_headers(&h).unwrap().five_hour.utilization, pct);
    }
    for value in ["NaN", "inf", "-0.1", "not-a-number"] {
        assert!(
            usage_from_headers(&headers(&[
                ("5h-utilization", value),
                ("7d-utilization", "0.2")
            ]))
            .is_none()
        );
    }
    let h = headers(&[("5h-utilization", "0.29"), ("7d-utilization", "0.57")])
        .into_iter()
        .map(|(k, v)| (k.to_uppercase(), v))
        .collect();
    assert!(usage_from_headers(&h).is_some());
}
#[test]
fn refusal_clamps_preserves_old_and_does_not_fabricate_unknown() {
    let h = headers(&[
        ("status", "rejected"),
        ("representative-claim", "seven_day"),
        ("7d-utilization", "0.95"),
        ("reset", "1000"),
    ]);
    assert!(usage_from_limit_refusal(&h, None).is_none());
    let old = usage(29.0, 20.0, Some(900));
    let u = usage_from_limit_refusal(&h, Some(&old)).unwrap();
    assert_eq!(u.seven_day.utilization, 100.0);
    assert_eq!(u.five_hour.utilization, 29.0);
    assert_eq!(u.seven_day.resets_at, Some(1000));
    assert!(
        usage_from_limit_refusal(
            &headers(&[
                ("status", "rejected"),
                ("representative-claim", "seven_day_opus")
            ]),
            Some(&old)
        )
        .is_none()
    );
    assert!(
        usage_from_limit_refusal(
            &headers(&[
                ("status", "allowed"),
                ("5h-utilization", "0.2"),
                ("7d-utilization", "0.3")
            ]),
            Some(&old)
        )
        .is_none()
    );
    assert_eq!(
        usage_from_limit_refusal(
            &headers(&[("5h-utilization", "1.0"), ("7d-utilization", "0.3")]),
            None
        )
        .unwrap()
        .five_hour
        .utilization,
        100.0
    );
}
#[test]
fn full_legacy_import_preserves_extensions_timestamps_and_pending_identity() {
    let legacy = json!({"name":"fixture","savedAt":"2024-02-29T12:34:56Z","oauthAccount":{"accountUuid":"account","organizationUuid":"org","emailAddress":"fixture@example.invalid","extension":true},"credentials":{"claudeAiOauth":{"accessToken":"SENTINEL_ACCESS","refreshToken":"SENTINEL_REFRESH","expiresAt":1709210096000_i64,"scopes":["user:inference"],"future":42},"otherAuth":{"sentinel":true}},"refreshFailAt":100.5,"rateLimitedUntil":200,"consecutiveRateLimits":2,"expectedWeeklyResetAt":300,"primedForResetAt":400,"lastUsage":{"five_hour":{"utilization":29,"resets_at":"2024-02-29T12:34:56Z"},"seven_day":{"utilization":57,"resets_at":1709210096}},"lastUsageAt":500,"lastEndpointReadAt":600,"subscriptionStatus":"active","subscriptionStartedAt":700,"planTier":"max","profileCheckedAt":800,"renewalDay":31,"resetStatus":{"cooldown_until":"2024-02-29T12:34:56Z","grants":[{"id":"g","resets_left":1}]},"resetStatusAt":900,"pendingResetClaim":{"grantId":"g","requestId":"request_same","createdAt":1000.5},"lastResetOutcome":"safe verdict","lastResetAttemptAt":1100});
    let a = import_legacy(&legacy, 1200).unwrap();
    assert_eq!(a.saved_at, 1709210096);
    assert_eq!(a.refresh_fail_at, Some(100));
    assert_eq!(a.rate_limited_until, Some(200));
    assert_eq!(a.expected_weekly_reset_at, Some(300));
    assert_eq!(a.primed_for_reset_at, Some(400));
    assert_eq!(a.last_usage_at, Some(500));
    assert_eq!(a.last_endpoint_read_at, Some(600));
    assert_eq!(a.subscription_started_at, Some(700));
    assert_eq!(a.profile_checked_at, Some(800));
    assert_eq!(a.reset_status_at, Some(900));
    assert_eq!(a.last_reset_attempt_at, Some(1100));
    assert_eq!(a.renewal_day, Some(31));
    assert_eq!(
        a.pending_reset_claim.as_ref().unwrap().request_id,
        "request_same"
    );
    assert_eq!(
        a.reset_status.as_ref().unwrap().cooldown_until,
        Some(1709210096)
    );
    assert_eq!(a.credentials, legacy["credentials"]);
    assert_eq!(a.oauth_account, legacy["oauthAccount"]);
    assert_eq!(a.expires_at_ms(), Some(1709210096000));
    assert_eq!(a.account_uuid(), Some("account"));
    assert_eq!(a.organization_uuid(), Some("org"));
    assert!(!a.identity_verified);
    assert!(!format!("{a:?}").contains("SENTINEL"));
    let round: StoredAccount = serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
    assert_eq!(round.credentials, a.credentials);
    assert_eq!(round.pending_reset_claim, a.pending_reset_claim);
    assert_eq!(round.id, a.id);
    assert!(import_legacy(&json!({"name":"bad","oauthAccount":{},"credentials":[]}), 0).is_err());
}
#[test]
fn local_renewal_clamps_month_and_uses_stable_fallback() {
    let now = Local
        .with_ymd_and_hms(2024, 2, 1, 12, 0, 0)
        .single()
        .unwrap()
        .timestamp();
    let mut a = StoredAccount {
        renewal_day: Some(31),
        ..StoredAccount::default()
    };
    let renewal = Local
        .timestamp_opt(next_renewal(&a, now).unwrap(), 0)
        .single()
        .unwrap();
    assert_eq!(
        (
            renewal.year(),
            renewal.month(),
            renewal.day(),
            renewal.hour(),
            renewal.minute()
        ),
        (2024, 2, 29, 0, 0)
    );
    assert_eq!(next_renewal(&a, now), next_renewal(&a, now + 60));
    let next = next_renewal(&a, renewal.timestamp()).unwrap();
    assert_eq!(Local.timestamp_opt(next, 0).single().unwrap().month(), 3);
    a.subscription_started_at = Some(
        Local
            .with_ymd_and_hms(2023, 1, 3, 14, 25, 0)
            .single()
            .unwrap()
            .timestamp(),
    );
    let anchored = Local
        .timestamp_opt(next_renewal(&a, now).unwrap(), 0)
        .single()
        .unwrap();
    assert_eq!((anchored.hour(), anchored.minute()), (14, 25));
    a.renewal_day = None;
    assert_eq!(next_renewal(&a, now), None);
}

#[test]
fn malformed_pending_import_fails_without_discarding_ambiguity() {
    let mut v = json!({"name":"fixture","oauthAccount":{},"credentials":{"claudeAiOauth":{"accessToken":"fixture-token","expiresAt":1709210096000.0}},"pendingResetClaim":{"grantId":"g","createdAt":1000}});
    assert_eq!(import_legacy(&v, 0).unwrap_err(), CoreError::InvalidLegacy);
    v["pendingResetClaim"] = Value::Null;
    v["lastUsage"] = json!({"corrupt":true});
    assert!(import_legacy(&v, 0).unwrap().last_usage.is_none());
    assert_eq!(
        import_legacy(&v, 0).unwrap().expires_at_ms(),
        Some(1709210096000)
    );
    let p = PendingResetClaim {
        grant_id: "g".into(),
        request_id: "../bad".into(),
        created_at: 1,
    };
    assert_eq!(
        prepare_reset_claim("g", Some(&p), 1000),
        Err(CoreError::InvalidIdentifier)
    );
    assert_eq!(parse_timestamp(&json!("1000.75")), Some(1000));
}
#[test]
fn renewal_days_across_short_and_leap_months() {
    for (year, month, last) in [(2023, 2, 28), (2024, 2, 29), (2024, 4, 30), (2024, 7, 31)] {
        let now = Local
            .with_ymd_and_hms(year, month, 1, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        for day in [1, 28, 29, 30, 31] {
            let a = StoredAccount {
                renewal_day: Some(day),
                ..StoredAccount::default()
            };
            let candidate = Local
                .timestamp_opt(next_renewal(&a, now).unwrap(), 0)
                .single()
                .unwrap();
            assert!(candidate.timestamp() > now);
            if day == 1 {
                assert_eq!(candidate.day(), 1);
                assert_eq!(candidate.month(), month + 1);
            } else {
                assert_eq!(candidate.day(), u32::from(day).min(last));
                assert_eq!(candidate.month(), month);
            }
        }
    }
}
