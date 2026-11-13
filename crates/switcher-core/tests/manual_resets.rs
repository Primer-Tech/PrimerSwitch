//! Manual use of banked Claude resets by kind: classification, tolerant parsing,
//! persistence compatibility, grant selection and the provider's real answers.
//! Fixtures only; nothing here reaches a provider.
use serde_json::{Value, json};
use switcher_core::*;

const NOW: i64 = 1_000_000;

fn grant(value: Value) -> ResetGrant {
    serde_json::from_value(value).expect("a grant with an id and resets_left parses")
}
fn status(value: Value) -> ResetStatus {
    serde_json::from_value(value).expect("a status object parses")
}
fn usable(id: &str, clears: Value) -> Value {
    json!({"id":id,"resets_left":1,"usable_now":true,"use_requires_limit":false,"clears":clears})
}

#[test]
fn kind_classification_follows_claude_code_in_order() {
    // (gives_back_weekly, type, clears) -> kind.
    let table: Vec<(Value, ResetKind)> = vec![
        // 1. gives_back_weekly always wins, even over an explicit type.
        (
            json!({"gives_back_weekly":true,"type":"weekly","clears":["five_hour"]}),
            ResetKind::Fable,
        ),
        (json!({"gives_back_weekly":true}), ResetKind::Fable),
        // 2. An explicit type wins over the cleared windows.
        (
            json!({"type":"session","clears":["seven_day"]}),
            ResetKind::Session,
        ),
        (
            json!({"type":"weekly","clears":["five_hour"]}),
            ResetKind::Weekly,
        ),
        (
            json!({"type":"full","clears":["five_hour"]}),
            ResetKind::Full,
        ),
        // 3. Otherwise the cleared windows decide.
        (json!({"clears":["five_hour"]}), ResetKind::Session),
        (json!({"clears":["seven_day"]}), ResetKind::Weekly),
        (
            json!({"clears":["seven_day_opus","seven_day_sonnet"]}),
            ResetKind::Weekly,
        ),
        (json!({"clears":["five_hour","seven_day"]}), ResetKind::Full),
        (
            json!({"clears":["five_hour","seven_day_opus"]}),
            ResetKind::Full,
        ),
        (json!({"clears":[]}), ResetKind::Full),
        (json!({}), ResetKind::Full),
        // Names that are not understood are dropped before the decision.
        (
            json!({"clears":["five_hour","seven_day_future"]}),
            ResetKind::Session,
        ),
        (json!({"clears":["seven_day_future"]}), ResetKind::Full),
        // The server's own `type` is limited to three values; anything else is ignored.
        (
            json!({"type":"fable","clears":["seven_day"]}),
            ResetKind::Weekly,
        ),
        (json!({"type":7,"clears":["five_hour"]}), ResetKind::Session),
    ];
    for (extra, expected) in table {
        let mut value = json!({"id":"g","resets_left":1});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(grant(value).kind(), expected, "{extra}");
    }
}

#[test]
fn clears_window_counts_gives_back_weekly_as_covering_seven_day() {
    let weekly = grant(json!({"id":"g","resets_left":1,"clears":["seven_day_opus"]}));
    assert!(weekly.clears_window("seven_day_opus"));
    assert!(!weekly.clears_window("seven_day"));
    let fable = grant(
        json!({"id":"f","resets_left":1,"gives_back_weekly":true,"clears":["seven_day_overage_included"]}),
    );
    assert!(fable.clears_window("seven_day"));
    assert!(fable.clears_window("seven_day_overage_included"));
    assert!(!fable.clears_window("five_hour"));
    // Only the weekly window is covered, and only for a grant that gives it back.
    let plain = grant(json!({"id":"p","resets_left":1,"clears":["seven_day_overage_included"]}));
    assert!(!plain.clears_window("seven_day"));
    // A name that is not understood is never cleared, even when the grant lists it.
    let odd = grant(json!({"id":"o","resets_left":1,"clears":["seven_day_future"]}));
    assert!(!odd.clears_window("seven_day_future"));
}

#[test]
fn held_means_unpaused_with_uses_left_or_usable_now() {
    let base = |extra: Value| {
        let mut value = json!({"id":"g","resets_left":0});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        grant(value)
    };
    assert!(!base(json!({})).is_held());
    assert!(base(json!({"resets_left":2})).is_held());
    assert!(base(json!({"usable_now":true})).is_held());
    assert!(!base(json!({"resets_left":2,"paused":true})).is_held());
    assert!(!base(json!({"usable_now":true,"paused":true})).is_held());
}

#[test]
fn every_descriptive_field_parses_and_a_malformed_one_keeps_the_grant() {
    let full = grant(json!({
        "id":"fable-1","label":"Fable reset","resets_total":3,"resets_left":2,
        "starts_at":"2026-11-01T00:00:00Z","ends_at":"2026-12-01T00:00:00Z",
        "clears":["seven_day_overage_included"],"paused":false,"usable_now":true,
        "use_requires_limit":false,"percent_used":{"seven_day_overage_included":40,"junk":5},
        "blocking":["five_hour","seven_day_future"],"type":"full","gives_back_weekly":true,
        "unavailable_reason":"cooldown"
    }));
    assert_eq!(full.label, "Fable reset");
    assert_eq!((full.resets_total, full.resets_left), (3, 2));
    assert_eq!(full.grant_type, Some(ResetKind::Full));
    assert!(full.gives_back_weekly);
    assert_eq!(full.percent_used.len(), 1);
    assert_eq!(full.percent_used["seven_day_overage_included"], 40);
    assert_eq!(full.blocking, vec!["five_hour".to_owned()]);
    assert_eq!(full.unavailable_reason, Some(ResetUnavailable::Cooldown));
    assert_eq!(full.kind(), ResetKind::Fable);

    // Defaults from Claude Code's schema when the optional fields are absent.
    let bare = grant(json!({"id":"g","resets_left":1}));
    assert_eq!(bare.label, "");
    assert_eq!(bare.resets_total, 0);
    assert_eq!(bare.grant_type, None);
    assert!(!bare.gives_back_weekly);
    assert!(bare.percent_used.is_empty() && bare.blocking.is_empty());
    assert_eq!(bare.unavailable_reason, None);
    assert!(bare.use_requires_limit && !bare.usable_now && !bare.paused);

    // Every malformed optional field falls back; the grant itself survives.
    let malformed = grant(json!({
        "id":"g","resets_left":1,"label":42,"resets_total":-1,"type":"nope",
        "gives_back_weekly":"yes","percent_used":[1,2],"blocking":"five_hour",
        "unavailable_reason":"mystery","clears":"five_hour"
    }));
    assert_eq!(malformed.label, "");
    assert_eq!(malformed.resets_total, 0);
    assert_eq!(malformed.grant_type, None);
    assert!(!malformed.gives_back_weekly);
    assert!(malformed.percent_used.is_empty() && malformed.blocking.is_empty());
    assert_eq!(malformed.unavailable_reason, None);
    assert_eq!(malformed.resets_left, 1);

    // Percentages are whole numbers 0..=100, as Claude Code reads them.
    let percents = grant(json!({
        "id":"g","resets_left":1,
        "percent_used":{"five_hour":100,"seven_day":101,"seven_day_opus":-1,
                        "seven_day_sonnet":12.5,"seven_day_cowork":50.0}
    }));
    assert_eq!(percents.percent_used.get("five_hour"), Some(&100));
    assert_eq!(percents.percent_used.get("seven_day_cowork"), Some(&50));
    assert_eq!(percents.percent_used.len(), 2);

    // A long label is bounded; the renderer never receives it anyway.
    let long = grant(json!({"id":"g","resets_left":1,"label":"x".repeat(500)}));
    assert_eq!(long.label.chars().count(), 100);
}

#[test]
fn a_bad_grant_or_field_never_discards_the_usage_or_the_other_grants() {
    let usage: UsageResponse = serde_json::from_value(json!({
        "five_hour":{"utilization":29,"resets_at":"2026-11-13T00:00:00Z"},
        "seven_day":{"utilization":57,"resets_at":null},
        "cedar_ember":{
            "eligible":true,"at_limit":"yes","exhausted":["five_hour","junk",7],
            "grants":[
                {"id":"BAD ID","resets_left":1},
                {"id":"no-count"},
                {"id":"ok","resets_left":1,"type":"weekly","percent_used":"nope"},
                17
            ]
        }
    }))
    .unwrap();
    assert_eq!(usage.five_hour.utilization, 29.0);
    let status = usage.cedar_ember.expect("the status block survives");
    assert_eq!(status.grants.len(), 1);
    assert_eq!(status.grants[0].id, "ok");
    assert_eq!(status.grants[0].kind(), ResetKind::Weekly);
    assert!(!status.at_limit, "a malformed flag falls back to false");
    assert_eq!(status.exhausted, vec!["five_hour".to_owned()]);
    // A status block that is not an object is dropped; the usage stays.
    let usage: UsageResponse = serde_json::from_value(json!({
        "five_hour":{"utilization":1,"resets_at":null},
        "seven_day":{"utilization":2,"resets_at":null},
        "cedar_ember":"invalid"
    }))
    .unwrap();
    assert!(usage.cedar_ember.is_none());
}

#[test]
fn records_without_the_new_fields_load_unchanged_and_round_trip_byte_for_byte() {
    // The exact shape earlier releases saved (field order included).
    let old_status = r#"{"eligible":true,"ineligible_reason":null,"grants":[{"id":"g","resets_left":2,"starts_at":null,"ends_at":1000,"clears":["five_hour"],"paused":false,"usable_now":true,"use_requires_limit":true}],"next_grant_id":"g","cooldown_until":null,"weekly_resets_at":86400}"#;
    let parsed: ResetStatus = serde_json::from_str(old_status).unwrap();
    assert_eq!(parsed.grants[0].kind(), ResetKind::Session);
    assert!(!parsed.at_limit && parsed.exhausted.is_empty());
    assert_eq!(serde_json::to_string(&parsed).unwrap(), old_status);

    // A whole saved account, with the old reset block inside its last usage.
    let old_account = format!(
        r#"{{"id":"a","reset_status":{old_status},"last_usage":{{"five_hour":{{"utilization":1.0,"resets_at":null}},"seven_day":{{"utilization":2.0,"resets_at":null}},"seven_day_sonnet":null,"extra_usage":null,"limits":null,"cedar_ember":{old_status}}}}}"#
    );
    let account: StoredAccount = serde_json::from_str(&old_account).unwrap();
    assert_eq!(account.reset_status.as_ref().unwrap().grants.len(), 1);
    let saved = serde_json::to_value(&account).unwrap();
    assert_eq!(
        saved["reset_status"],
        serde_json::from_str::<Value>(old_status).unwrap()
    );
    assert_eq!(
        saved["last_usage"]["cedar_ember"],
        serde_json::from_str::<Value>(old_status).unwrap()
    );
}

#[test]
fn new_fields_are_written_only_when_present_and_survive_a_round_trip() {
    let rich = status(json!({
        "eligible":true,"at_limit":true,"exhausted":["seven_day"],
        "grants":[{"id":"f","label":"Fable","resets_total":2,"resets_left":1,"usable_now":true,
                   "type":"weekly","gives_back_weekly":true,"percent_used":{"seven_day":90},
                   "blocking":["five_hour"],"unavailable_reason":"needs_limit",
                   "clears":["seven_day_overage_included"]}]
    }));
    let text = serde_json::to_string(&rich).unwrap();
    assert!(text.contains(r#""at_limit":true"#) && text.contains(r#""exhausted":["seven_day"]"#));
    assert!(text.contains(r#""type":"weekly""#) && text.contains(r#""gives_back_weekly":true"#));
    assert!(text.contains(r#""unavailable_reason":"needs_limit""#));
    let again: ResetStatus = serde_json::from_str(&text).unwrap();
    assert_eq!(again, rich);
    assert_eq!(serde_json::to_string(&again).unwrap(), text);
    // A default grant writes none of the new keys.
    let bare = serde_json::to_value(ResetGrant::default()).unwrap();
    for key in [
        "label",
        "resets_total",
        "type",
        "gives_back_weekly",
        "percent_used",
        "blocking",
        "unavailable_reason",
    ] {
        assert!(bare.get(key).is_none(), "{key} must be omitted");
    }
}

#[test]
fn the_server_order_and_next_grant_decide_which_grant_a_kind_uses() {
    let s = status(json!({
        "next_grant_id":"later",
        "grants":[
            usable("first", json!(["five_hour"])),
            usable("later", json!(["five_hour"])),
            usable("other-kind", json!(["seven_day"]))
        ]
    }));
    let pick = |s: &ResetStatus, kind| select_manual_grant(s, kind, NOW).map(|g| g.id.clone());
    // next_grant_id is preferred when it is among the usable grants of the kind.
    assert_eq!(pick(&s, ResetKind::Session).as_deref(), Some("later"));
    // It is ignored for a kind it does not belong to.
    assert_eq!(pick(&s, ResetKind::Weekly).as_deref(), Some("other-kind"));
    // Without it (or when it is not usable) the first one in server order wins.
    let mut no_next = s.clone();
    no_next.next_grant_id = None;
    assert_eq!(pick(&no_next, ResetKind::Session).as_deref(), Some("first"));
    let mut next_unusable = s.clone();
    next_unusable.grants[1].usable_now = false;
    assert_eq!(
        pick(&next_unusable, ResetKind::Session).as_deref(),
        Some("first")
    );
    // No grant of the kind at all.
    assert_eq!(pick(&s, ResetKind::Fable), None);
}

#[test]
fn unusable_grants_are_skipped() {
    let kinds =
        |s: &ResetStatus| select_manual_grant(s, ResetKind::Session, NOW).map(|g| g.id.clone());
    let make = |extra: Value| {
        let mut value = usable("g", json!(["five_hour"]));
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        status(json!({"grants":[value, usable("ok", json!(["five_hour"]))]}))
    };
    for (what, extra) in [
        ("not usable now", json!({"usable_now":false})),
        ("paused", json!({"paused":true})),
        ("spent", json!({"resets_left":0})),
        ("expired", json!({"ends_at": NOW})),
        ("not started", json!({"starts_at": NOW + 1})),
    ] {
        assert_eq!(kinds(&make(extra)).as_deref(), Some("ok"), "{what}");
    }
    // A grant that ends later, or started earlier, is usable.
    let live = make(json!({"ends_at": NOW + 1, "starts_at": NOW}));
    assert_eq!(kinds(&live).as_deref(), Some("g"));
    // A cooldown or an ineligible account leaves nothing to use.
    let mut cooling = make(json!({}));
    cooling.cooldown_until = Some(NOW + 60);
    assert_eq!(kinds(&cooling), None);
    cooling.cooldown_until = Some(NOW);
    assert_eq!(
        kinds(&cooling).as_deref(),
        Some("g"),
        "a past cooldown is over"
    );
    let mut ineligible = make(json!({}));
    ineligible.eligible = Some(false);
    assert_eq!(kinds(&ineligible), None);
    // An id the provider would reject is never selected.
    let mut bad_id = make(json!({}));
    bad_id.grants[0].id = "NOT VALID".into();
    assert_eq!(kinds(&bad_id).as_deref(), Some("ok"));
}

#[test]
fn a_manual_claim_needs_a_reached_limit_or_an_explicit_early_confirmation() {
    let weekly = |extra: Value, exhausted: Value| {
        let mut grant = usable("w", json!(["seven_day"]));
        grant
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        status(json!({"exhausted":exhausted,"grants":[grant]}))
    };
    let plan = |s: &ResetStatus, allow| plan_manual_claim(s, ResetKind::Weekly, NOW, allow);

    // At the limit the grant clears: ready, early or not.
    let at_limit = weekly(json!({"use_requires_limit":true}), json!(["seven_day"]));
    assert_eq!(
        plan(&at_limit, false),
        Ok(ManualClaim {
            grant_id: "w".into(),
            early: None
        })
    );
    // A grant that needs a limit the account has not reached is refused, even when the
    // caller allows early use.
    let needs = weekly(json!({"use_requires_limit":true}), json!(["five_hour"]));
    assert_eq!(plan(&needs, true), Err(ManualRefusal::NeedsLimit));
    assert_eq!(
        plan(&weekly(json!({"use_requires_limit":true}), json!([])), true),
        Err(ManualRefusal::NeedsLimit)
    );
    // A grant that does not require a limit is an early use: refused until confirmed.
    let early = weekly(json!({"use_requires_limit":false}), json!([]));
    assert_eq!(
        plan(&early, false),
        Err(ManualRefusal::EarlyUseNeedsConfirmation)
    );
    assert_eq!(
        plan(&early, true),
        Ok(ManualClaim {
            grant_id: "w".into(),
            early: Some(EarlyUse { percent_left: None })
        })
    );
    // At a limit the grant does NOT clear, it is still early.
    let elsewhere = weekly(json!({"use_requires_limit":false}), json!(["five_hour"]));
    assert_eq!(
        plan(&elsewhere, false),
        Err(ManualRefusal::EarlyUseNeedsConfirmation)
    );
    // Nothing usable of the requested kind.
    assert_eq!(
        plan_manual_claim(&early, ResetKind::Full, NOW, true),
        Err(ManualRefusal::NotUsable)
    );
}

#[test]
fn a_fable_reset_counts_the_weekly_limit_as_its_own() {
    let fable = |exhausted: Value| {
        status(json!({"exhausted":exhausted,"grants":[{
            "id":"f","resets_left":1,"usable_now":true,"use_requires_limit":true,
            "gives_back_weekly":true,"clears":["seven_day_overage_included"]}]}))
    };
    let plan = |s: &ResetStatus| plan_manual_claim(s, ResetKind::Fable, NOW, false);
    assert!(plan(&fable(json!(["seven_day"]))).is_ok());
    assert!(plan(&fable(json!(["seven_day_overage_included"]))).is_ok());
    assert_eq!(
        plan(&fable(json!(["five_hour"]))),
        Err(ManualRefusal::NeedsLimit)
    );
}

#[test]
fn the_early_use_percentage_matches_claude_codes_wording_rule() {
    let left = |g: Value| {
        let mut value = json!({"id":"g","resets_left":1});
        value
            .as_object_mut()
            .unwrap()
            .extend(g.as_object().unwrap().clone());
        early_use(&grant(value)).percent_left
    };
    // 100 minus the highest percent used among the windows the grant lists.
    assert_eq!(
        left(
            json!({"clears":["five_hour","seven_day"],"percent_used":{"five_hour":30,"seven_day":65}})
        ),
        Some(35)
    );
    // Windows the grant does not list do not count.
    assert_eq!(
        left(json!({"clears":["five_hour"],"percent_used":{"five_hour":30,"seven_day":99}})),
        Some(70)
    );
    // No numbers, or a limit with no plain name, or nothing left: no percentage.
    assert_eq!(left(json!({"clears":["five_hour"]})), None);
    assert_eq!(
        left(json!({"clears":["seven_day_cowork"],"percent_used":{"seven_day_cowork":10}})),
        None
    );
    assert_eq!(
        left(json!({"clears":["five_hour"],"percent_used":{"five_hour":100}})),
        None
    );
    assert_eq!(
        left(json!({"clears":["five_hour"],"percent_used":{"five_hour":0}})),
        Some(100)
    );
}

#[test]
fn offers_list_each_held_kind_once_in_display_order_with_no_ids_needed_by_the_view() {
    let s = status(json!({
        "next_grant_id":"w2",
        "grants":[
            {"id":"full1","resets_total":2,"resets_left":1,"ends_at":NOW+900,"usable_now":true,
             "use_requires_limit":false,"clears":["five_hour","seven_day"]},
            {"id":"f1","resets_total":3,"resets_left":2,"ends_at":NOW+500,"usable_now":true,
             "use_requires_limit":true,"gives_back_weekly":true,"clears":["seven_day_overage_included"]},
            {"id":"w1","resets_total":1,"resets_left":1,"ends_at":NOW+700,"usable_now":false,
             "use_requires_limit":true,"clears":["seven_day"]},
            {"id":"w2","resets_total":1,"resets_left":1,"ends_at":NOW+300,"usable_now":true,
             "use_requires_limit":true,"clears":["seven_day"]},
            {"id":"s1","resets_total":1,"resets_left":1,"usable_now":true,"use_requires_limit":true,
             "clears":["five_hour"]},
            {"id":"gone","resets_left":1,"ends_at":NOW-1,"clears":["five_hour"]},
            {"id":"paused","resets_left":1,"paused":true,"clears":["five_hour"]},
            {"id":"spent","resets_left":0,"clears":["five_hour"]}
        ],
        "exhausted":["seven_day"]
    }));
    let offers = reset_offers(&s, NOW);
    let kinds: Vec<ResetKind> = offers.iter().map(|o| o.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ResetKind::Session,
            ResetKind::Weekly,
            ResetKind::Fable,
            ResetKind::Full
        ]
    );
    // Session: only the live grant counts; expired, paused and spent ones are not held.
    let session = &offers[0];
    assert_eq!(
        (session.resets_left, session.resets_total, session.ends_at),
        (1, 1, None)
    );
    assert!(
        !session.usable_now,
        "needs a five-hour limit the account has not reached"
    );
    assert_eq!(session.unavailable, Some(ResetUnavailable::NeedsLimit));
    // Weekly: two grants, earliest end, the next grant is the one a claim uses.
    let weekly = &offers[1];
    assert_eq!((weekly.resets_left, weekly.resets_total), (2, 2));
    assert_eq!(weekly.ends_at, Some(NOW + 300));
    assert_eq!(weekly.grant_id.as_deref(), Some("w2"));
    assert!(weekly.usable_now && weekly.early_use.is_none());
    // Fable: weekly is exhausted and Fable covers it.
    let fable = &offers[2];
    assert_eq!((fable.resets_left, fable.resets_total), (2, 3));
    assert!(fable.usable_now);
    // Full clears the weekly window too, so it is a plain use at this limit.
    let full = &offers[3];
    assert!(full.usable_now && full.early_use.is_none());
    // With no limit reached, Full (no limit required) is an early use, and Fable
    // (limit required) waits for one.
    let mut calm = s.clone();
    calm.exhausted.clear();
    let offers = reset_offers(&calm, NOW);
    assert_eq!(offers[3].early_use, Some(EarlyUse { percent_left: None }));
    assert!(offers[3].usable_now);
    assert!(!offers[2].usable_now);
    assert_eq!(offers[2].unavailable, Some(ResetUnavailable::NeedsLimit));
    // Nothing held: no offers at all.
    assert!(reset_offers(&ResetStatus::default(), NOW).is_empty());
}

#[test]
fn an_unusable_kind_says_why_in_claude_codes_order() {
    let reason = |status_extra: Value, grant_extra: Value| {
        let mut g = json!({"id":"g","resets_left":1,"clears":["five_hour"]});
        g.as_object_mut()
            .unwrap()
            .extend(grant_extra.as_object().unwrap().clone());
        let mut s = json!({"grants":[g]});
        s.as_object_mut()
            .unwrap()
            .extend(status_extra.as_object().unwrap().clone());
        let offers = reset_offers(&status(s), NOW);
        assert!(!offers[0].usable_now, "{grant_extra}");
        offers[0].unavailable
    };
    use ResetUnavailable::*;
    // The server's own reason comes first.
    assert_eq!(
        reason(
            json!({"cooldown_until":NOW+5}),
            json!({"unavailable_reason":"blocked_opus"})
        ),
        Some(BlockedOpus)
    );
    assert_eq!(
        reason(json!({"cooldown_until":NOW+5}), json!({})),
        Some(Cooldown)
    );
    assert_eq!(
        reason(json!({}), json!({"blocking":["five_hour"]})),
        Some(BlockedFiveHour)
    );
    assert_eq!(
        reason(json!({}), json!({"blocking":["seven_day"]})),
        Some(BlockedSevenDay)
    );
    assert_eq!(
        reason(json!({}), json!({"blocking":["seven_day_opus"]})),
        Some(BlockedOpus)
    );
    assert_eq!(
        reason(json!({}), json!({"blocking":["seven_day_sonnet"]})),
        Some(BlockedSonnet)
    );
    assert_eq!(
        reason(json!({}), json!({"blocking":["seven_day_cowork"]})),
        Some(BlockedOther)
    );
    assert_eq!(
        reason(
            json!({"at_limit":true,"exhausted":["seven_day"]}),
            json!({})
        ),
        Some(CannotClear)
    );
    assert_eq!(
        reason(json!({}), json!({"use_requires_limit":true})),
        Some(NeedsLimit)
    );
    assert_eq!(
        reason(json!({}), json!({"use_requires_limit":false})),
        Some(Unavailable)
    );
    assert_eq!(
        reason(json!({"eligible":false}), json!({})),
        Some(Unavailable)
    );
}

#[test]
fn the_providers_real_answers_keep_or_clear_the_pending_key_correctly() {
    let outcome = |status: u16, body: Value| parse_reset_outcome(status, &body);
    // Ambiguous answers: the reset may or may not have happened. The key is kept.
    for reason in ["stamp_indeterminate", "reset_unconfirmed"] {
        for result in [
            "unavailable",
            "ineligible",
            "cooldown",
            "not_limited",
            "mystery",
        ] {
            let answer = outcome(200, json!({"result":result,"reason":reason}));
            assert_eq!(answer, ResetOutcome::Transport, "{result}/{reason}");
            assert!(!answer.is_final());
        }
    }
    // A definite verdict wins over an ambiguity hint.
    assert!(matches!(
        outcome(
            200,
            json!({"result":"reset","reason":"reset_unconfirmed","resets_left":1})
        ),
        ResetOutcome::Reset {
            resets_left: Some(1),
            ..
        }
    ));
    assert_eq!(
        outcome(
            200,
            json!({"result":"already_used","reason":"stamp_indeterminate"})
        ),
        ResetOutcome::AlreadyUsed
    );
    // Definite refusals are final and clear the key.
    let not_next = outcome(
        200,
        json!({"result":"unavailable","reason":"not_next_grant"}),
    );
    assert_eq!(
        not_next,
        ResetOutcome::Unavailable {
            reason: Some("not_next_grant".into())
        }
    );
    assert!(not_next.is_final());
    assert_eq!(
        outcome(
            200,
            json!({"result":"unavailable","reason":"unknown_grant"})
        ),
        ResetOutcome::Unavailable {
            reason: Some("unknown_grant".into())
        }
    );
    assert_eq!(
        outcome(200, json!({"result":"not_limited","reason":"not_limited"})),
        ResetOutcome::NotLimited
    );
    assert_eq!(
        outcome(200, json!({"result":"cooldown","cooldown_until":NOW+60})),
        ResetOutcome::Cooldown {
            until: Some(NOW + 60)
        }
    );
    // Server trouble and unreadable answers stay unconfirmed.
    assert_eq!(
        outcome(503, json!({"result":"unavailable"})),
        ResetOutcome::Transport
    );
    assert_eq!(outcome(200, json!({})), ResetOutcome::Transport);
    // Arbitrary provider text never reaches the stored reason or the display text.
    let hostile = outcome(
        200,
        json!({"result":"unavailable","reason":"token sk-ant-SECRET"}),
    );
    assert_eq!(hostile, ResetOutcome::Unavailable { reason: None });
    for locale in ["en", "ro"] {
        for o in [&not_next, &hostile, &ResetOutcome::Transport] {
            assert!(!o.text_for(locale).contains("SECRET"));
        }
    }
    assert_ne!(not_next.text_for("en"), hostile.text_for("en"));
    assert_ne!(not_next.text_for("ro"), hostile.text_for("ro"));
}

#[test]
fn a_manual_refusal_or_success_updates_the_saved_status_like_an_automatic_one() {
    let mut s = Some(status(json!({
        "next_grant_id":"g",
        "grants":[usable("g", json!(["five_hour"])), usable("h", json!(["five_hour"]))]
    })));
    apply_reset_outcome(
        &mut s,
        "g",
        &ResetOutcome::Reset {
            resets_left: Some(0),
            cooldown_until: Some(NOW + 60),
        },
    );
    let after = s.as_ref().unwrap();
    assert_eq!(
        after
            .grants
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>(),
        vec!["h"]
    );
    assert_eq!(after.next_grant_id, None);
    assert_eq!(after.cooldown_until, Some(NOW + 60));
}
