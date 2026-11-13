//! Claude plan type (`organization_type`, `seat_tier`) on a saved account. Fixtures
//! only: no home, network, or auth access.
use serde_json::{Value, json};
use switcher_core::*;

fn account(oauth: Value) -> StoredAccount {
    StoredAccount::new("Fixture", oauth, json!({"claudeAiOauth":{}}), 1000)
}

#[test]
fn plan_codes_are_short_lowercase_identifiers_or_nothing() {
    for (input, expected) in [
        (json!("claude_team"), Some("claude_team")),
        (json!("claude_max"), Some("claude_max")),
        (json!("team_tier_1"), Some("team_tier_1")),
        (json!("Claude_Team"), Some("claude_team")),
        (json!("tier-2"), Some("tier-2")),
    ] {
        assert_eq!(plan_code(&input).as_deref(), expected, "{input}");
    }
    // 64 characters is the longest accepted code.
    assert_eq!(plan_code(&json!("a".repeat(64))), Some("a".repeat(64)));
    for odd in [
        json!(null),
        json!(5),
        json!(true),
        json!([]),
        json!({"name": "claude_team"}),
        json!(""),
        json!("claude team"),
        json!("claude_team\n"),
        json!("<b>team</b>"),
        json!("équipe"),
        json!("a".repeat(65)),
    ] {
        assert_eq!(plan_code(&odd), None, "{odd}");
    }
    assert_eq!(plan_code(&Value::Null), None);
}

#[test]
fn saved_fields_win_and_bring_their_own_seat_tier() {
    let mut a = account(json!({"organizationType":"claude_pro","seatTier":"cli_seat"}));
    a.organization_type = Some("claude_team".into());
    a.seat_tier = Some("team_tier_1".into());
    assert_eq!(
        a.plan_codes(),
        (Some("claude_team".into()), Some("team_tier_1".into()))
    );
    // A null seat tier in the profile answer is not filled from the CLI copy.
    a.seat_tier = None;
    assert_eq!(a.plan_codes(), (Some("claude_team".into()), None));
    // The saved copy is cleaned again on the way out (a hand-edited vault).
    a.organization_type = Some("Claude Team!".into());
    a.seat_tier = Some("x".into());
    assert_eq!(
        a.plan_codes(),
        (Some("claude_pro".into()), Some("cli_seat".into()))
    );
}

#[test]
fn the_cli_keys_in_oauth_account_stand_in_only_while_nothing_is_saved() {
    let a = account(json!({
        "accountUuid":"u","organizationType":"claude_team","seatTier":"team_tier_1"
    }));
    assert!(a.organization_type.is_none() && a.seat_tier.is_none());
    assert_eq!(
        a.plan_codes(),
        (Some("claude_team".into()), Some("team_tier_1".into()))
    );
    let only_type = account(json!({"organizationType":"claude_max"}));
    assert_eq!(only_type.plan_codes(), (Some("claude_max".into()), None));
    // An odd or missing type means unknown, and the seat tier is not shown alone.
    for oauth in [
        json!({"organizationType":7,"seatTier":"team_tier_1"}),
        json!({"organizationType":"not a code","seatTier":"team_tier_1"}),
        json!({"seatTier":"team_tier_1"}),
        json!({}),
        json!(null),
    ] {
        assert_eq!(account(oauth).plan_codes(), (None, None));
    }
    // Reading never writes: the saved identity stays as it was.
    let before = a.oauth_account.clone();
    let _ = a.plan_codes();
    assert_eq!(a.oauth_account, before);
}

#[test]
fn old_records_load_unchanged_and_unchanged_records_serialize_as_before() {
    // A record written before these fields existed.
    let old = json!({
        "id":"a","provider":"claude","name":"Old","saved_at":10,
        "oauth_account":{"accountUuid":"u","organizationUuid":"o"},
        "credentials":{"claudeAiOauth":{"accessToken":"fixture"}},
        "subscription_status":"active","plan_tier":"default_claude_max_5x",
        "profile_checked_at":99,"identity_verified":true
    });
    let loaded: StoredAccount = serde_json::from_value(old).unwrap();
    assert_eq!(loaded.organization_type, None);
    assert_eq!(loaded.seat_tier, None);
    let encoded = serde_json::to_value(&loaded).unwrap();
    assert!(encoded.get("organization_type").is_none());
    assert!(encoded.get("seat_tier").is_none());
    // Round trip: the same keys and values come back out.
    let again: StoredAccount = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(serde_json::to_value(&again).unwrap(), encoded);
    assert_eq!(encoded["plan_tier"], "default_claude_max_5x");
    // Present fields are written under their snake_case names and survive a trip.
    let mut known = loaded;
    known.organization_type = Some("claude_team".into());
    known.seat_tier = Some("team_tier_1".into());
    let encoded = serde_json::to_value(&known).unwrap();
    assert_eq!(encoded["organization_type"], "claude_team");
    assert_eq!(encoded["seat_tier"], "team_tier_1");
    let again: StoredAccount = serde_json::from_value(encoded).unwrap();
    assert_eq!(again.organization_type.as_deref(), Some("claude_team"));
    assert_eq!(again.seat_tier.as_deref(), Some("team_tier_1"));
}
