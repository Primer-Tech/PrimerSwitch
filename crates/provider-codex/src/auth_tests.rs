use super::*;
use serde_json::json;
fn payload(account: &str) -> Vec<u8> {
    let claims = json!({"email":"fixture@example.invalid","https://api.openai.com/auth":{"chatgpt_user_id":"fixture-user","chatgpt_account_id":account,"chatgpt_plan_type":"future-plan"}});
    let jwt = format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    );
    serde_json::to_vec(&json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":jwt,"access_token":"SECRET-ACCESS","refresh_token":"SECRET-REFRESH","account_id":account,"future_token_field":"SECRET-EXTENSION"},"future":{"nested":[1,true,"SECRET-UNKNOWN"]}})).unwrap()
}
#[test]
fn opaque_bytes_unverified_claims_and_redaction() {
    let bytes = payload("workspace-a");
    let auth = OpaqueAuth::parse(bytes.clone()).unwrap();
    assert_eq!(auth.as_bytes(), bytes);
    assert_eq!(auth.kind(), AuthKind::ManagedChatgpt);
    assert_eq!(
        auth.routing_claims().chatgpt_user_id.as_deref(),
        Some("fixture-user")
    );
    assert_eq!(
        auth.routing_claims().plan_type.as_deref(),
        Some("future-plan")
    );
    assert!(!format!("{auth:?} {:?}", auth.routing_claims()).contains("SECRET"));
}
#[test]
fn workspace_conflict_rejected() {
    let mut value: serde_json::Value = serde_json::from_slice(&payload("workspace-a")).unwrap();
    value["tokens"]["account_id"] = "workspace-b".into();
    assert_eq!(
        OpaqueAuth::parse(serde_json::to_vec(&value).unwrap()).unwrap_err(),
        CodexError::IdentityMismatch
    );
}
#[test]
fn api_key_and_unsupported_modes_preserve_bytes() {
    for (value, kind) in [
        (
            json!({"OPENAI_API_KEY":"SECRET-KEY","extension":42}),
            AuthKind::ApiKey,
        ),
        (
            json!({"auth_mode":"future-mode","tokens":{"future":"SECRET"}}),
            AuthKind::Unsupported,
        ),
        (
            json!({"personal_access_token":"SECRET"}),
            AuthKind::Unsupported,
        ),
        (
            json!({"auth_mode":42,"OPENAI_API_KEY":"SECRET"}),
            AuthKind::Unsupported,
        ),
    ] {
        let bytes = serde_json::to_vec(&value).unwrap();
        let auth = OpaqueAuth::parse(bytes.clone()).unwrap();
        assert_eq!(auth.kind(), kind);
        assert_eq!(auth.as_bytes(), bytes);
    }
}
#[test]
fn duplicate_keys_invalid_jwt_and_missing_refresh_fail_closed() {
    assert_eq!(
        OpaqueAuth::parse(br#"{"OPENAI_API_KEY":"SECRET","OPENAI_API_KEY":"other"}"#.to_vec())
            .unwrap_err(),
        CodexError::Protocol
    );
    let mut value: serde_json::Value = serde_json::from_slice(&payload("workspace-a")).unwrap();
    value["tokens"]["id_token"] = "bad.jwt".into();
    assert_eq!(
        OpaqueAuth::parse(serde_json::to_vec(&value).unwrap()).unwrap_err(),
        CodexError::Protocol
    );
    let mut value: serde_json::Value = serde_json::from_slice(&payload("workspace-a")).unwrap();
    value["tokens"]["refresh_token"] = serde_json::Value::Null;
    assert_eq!(
        OpaqueAuth::parse(serde_json::to_vec(&value).unwrap()).unwrap_err(),
        CodexError::UnsupportedAuthMode
    );
}

#[test]
fn malformed_routing_metadata_is_not_coerced_to_unrestricted() {
    let mut value: serde_json::Value = serde_json::from_slice(&payload("workspace-a")).unwrap();
    value["tokens"]["account_id"] = json!(42);
    assert_eq!(
        OpaqueAuth::parse(serde_json::to_vec(&value).unwrap()).unwrap_err(),
        CodexError::Protocol
    );
    let mut value: serde_json::Value = serde_json::from_slice(&payload("workspace-a")).unwrap();
    let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_is_fedramp":"true"}});
    value["tokens"]["id_token"] = json!(format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    ));
    assert_eq!(
        OpaqueAuth::parse(serde_json::to_vec(&value).unwrap()).unwrap_err(),
        CodexError::Protocol
    );
}
fn tokens_for(user: &str, account: &str, access: &str, refresh: &str) -> serde_json::Value {
    let claims = json!({"email":format!("{user}@example.invalid"),"https://api.openai.com/auth":{"chatgpt_user_id":user,"chatgpt_account_id":account,"chatgpt_plan_type":"plus"}});
    let jwt = format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    );
    json!({"id_token":jwt,"access_token":access,"refresh_token":refresh,"account_id":account})
}
#[test]
fn hybrid_file_is_inspected_but_never_parsed_strictly() {
    // Old process holding A refreshed after the file was switched to B.
    let mut hybrid = json!({"auth_mode":"chatgpt","tokens":tokens_for("user-a","ws-a","A2","RA2"),"last_refresh":"2026-10-03T10:00:00Z","b_extension":1});
    hybrid["tokens"]["account_id"] = "ws-b".into();
    let bytes = serde_json::to_vec(&hybrid).unwrap();
    let inspection = OpaqueAuth::inspect(&bytes).unwrap();
    assert!(inspection.hybrid);
    assert_eq!(inspection.kind, AuthKind::ManagedChatgpt);
    assert_eq!(
        inspection.claims.chatgpt_account_id.as_deref(),
        Some("ws-a")
    );
    assert_eq!(inspection.claims.token_account_id.as_deref(), Some("ws-b"));
    assert_eq!(
        OpaqueAuth::parse(bytes).unwrap_err(),
        CodexError::IdentityMismatch
    );
    let clean = OpaqueAuth::inspect(&payload("workspace-a")).unwrap();
    assert!(!clean.hybrid);
    assert!(!format!("{clean:?}").contains("SECRET"));
}
#[test]
fn rotated_tokens_move_into_the_owning_saved_record() {
    let saved_a = serde_json::to_vec(&json!({"auth_mode":"chatgpt","tokens":tokens_for("user-a","ws-a","A1","RA1"),"last_refresh":"2026-09-01T00:00:00Z","a_extension":{"keep":true}})).unwrap();
    let mut hybrid = json!({"auth_mode":"chatgpt","tokens":tokens_for("user-a","ws-a","A2","RA2"),"last_refresh":"2026-10-03T10:00:00Z"});
    hybrid["tokens"]["account_id"] = "ws-b".into();
    let merged = merge_rotated_tokens(&saved_a, &serde_json::to_vec(&hybrid).unwrap()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&merged).unwrap();
    assert_eq!(value["tokens"]["access_token"], "A2");
    assert_eq!(value["tokens"]["refresh_token"], "RA2");
    assert_eq!(value["tokens"]["account_id"], "ws-a");
    assert_eq!(value["last_refresh"], "2026-10-03T10:00:00Z");
    assert_eq!(value["a_extension"]["keep"], true);
    assert!(!OpaqueAuth::inspect(&merged).unwrap().hybrid);
    // Tokens of another user can never be merged into A's record.
    let foreign = serde_json::to_vec(
        &json!({"auth_mode":"chatgpt","tokens":tokens_for("user-b","ws-b","B2","RB2")}),
    )
    .unwrap();
    assert!(merge_rotated_tokens(&saved_a, &foreign).is_err());
    // A same-workspace, different-user token is rejected too.
    let teammate = serde_json::to_vec(
        &json!({"auth_mode":"chatgpt","tokens":tokens_for("user-c","ws-a","C2","RC2")}),
    )
    .unwrap();
    assert_eq!(
        merge_rotated_tokens(&saved_a, &teammate).unwrap_err(),
        CodexError::IdentityMismatch
    );
}
#[test]
fn import_payloads_hybrid_normalization_and_token_times() {
    let tokens = tokens_for(
        "user-a",
        "ws-a",
        "header.eyJleHAiOjE5MDAwMDAwMDB9.sig",
        "RA1",
    );
    let bytes = chatgpt_auth_payload(
        tokens["id_token"].as_str().unwrap(),
        tokens["access_token"].as_str().unwrap(),
        "RA1",
        Some("ws-a"),
        "2026-10-03T10:00:00Z",
    )
    .unwrap();
    let auth = OpaqueAuth::parse(bytes).unwrap();
    assert_eq!(auth.kind(), AuthKind::ManagedChatgpt);
    assert_eq!(auth.routing_claims().access_expires_at, Some(1_900_000_000));
    assert!(chatgpt_auth_payload("bad", "a", "r", None, "x").is_err());
    let key = api_key_auth_payload("sk-test-SECRET").unwrap();
    assert_eq!(OpaqueAuth::parse(key).unwrap().kind(), AuthKind::ApiKey);
    let mut hybrid = json!({"auth_mode":"chatgpt","tokens":tokens_for("user-a","ws-a","A2","RA2")});
    hybrid["tokens"]["account_id"] = "ws-b".into();
    let fixed = normalize_hybrid(&serde_json::to_vec(&hybrid).unwrap()).unwrap();
    let fixed = OpaqueAuth::parse(fixed).unwrap();
    assert_eq!(
        fixed.routing_claims().token_account_id.as_deref(),
        Some("ws-a")
    );
}
fn access_jwt(user: &str, workspace: &str, serial: &str) -> String {
    let claims = json!({"exp":1_900_000_000,"serial":serial,"https://api.openai.com/auth":{
        "chatgpt_account_id":workspace,"chatgpt_user_id":user,"chatgpt_account_user_id":format!("{user}__{workspace}")}});
    format!(
        "header.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}
fn file(id_user: &str, id_ws: &str, access: String, refresh: &str, label: &str) -> Vec<u8> {
    let mut tokens = tokens_for(id_user, id_ws, "unused", refresh);
    tokens["access_token"] = access.into();
    tokens["account_id"] = label.into();
    serde_json::to_vec(
        &json!({"auth_mode":"chatgpt","tokens":tokens,"last_refresh":"2026-10-03T11:00:00Z"}),
    )
    .unwrap()
}
#[test]
fn refresh_without_id_token_is_recognised_by_the_access_token() {
    // A's process refreshed after the switch to B; the response had no id_token, so
    // Codex kept B's id_token: B's id_token + B's label + A's access/refresh tokens.
    let mixed = file(
        "user-b",
        "ws-b",
        access_jwt("user-a", "ws-a", "2"),
        "RA2",
        "ws-b",
    );
    let inspection = OpaqueAuth::inspect(&mixed).unwrap();
    assert!(inspection.hybrid);
    assert_eq!(
        inspection.claims.token_owner(),
        (Some("user-a".into()), Some("ws-a".into()))
    );
    assert!(OpaqueAuth::parse(mixed.clone()).is_err());
    // The tokens go to A, A keeps its own id_token.
    let saved_a = file(
        "user-a",
        "ws-a",
        access_jwt("user-a", "ws-a", "1"),
        "RA1",
        "ws-a",
    );
    let merged = merge_rotated_tokens(&saved_a, &mixed).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&merged).unwrap();
    assert_eq!(value["tokens"]["refresh_token"], "RA2");
    assert_eq!(value["tokens"]["account_id"], "ws-a");
    let merged = OpaqueAuth::parse(merged).unwrap();
    assert_eq!(
        merged.routing_claims().chatgpt_user_id.as_deref(),
        Some("user-a")
    );
    // Never into B's record.
    let saved_b = file(
        "user-b",
        "ws-b",
        access_jwt("user-b", "ws-b", "1"),
        "RB1",
        "ws-b",
    );
    assert!(merge_rotated_tokens(&saved_b, &mixed).is_err());
    // It cannot be re-labelled into a standalone file without A's id_token.
    assert!(normalize_hybrid(&mixed).is_err());
}
#[test]
fn two_users_of_one_workspace_are_told_apart_by_the_access_token() {
    let mixed = file(
        "user-b",
        "ws-team",
        access_jwt("user-a", "ws-team", "2"),
        "RA2",
        "ws-team",
    );
    assert!(OpaqueAuth::inspect(&mixed).unwrap().hybrid);
    let clean = file(
        "user-a",
        "ws-team",
        access_jwt("user-a", "ws-team", "1"),
        "RA1",
        "ws-team",
    );
    let inspection = OpaqueAuth::inspect(&clean).unwrap();
    assert!(!inspection.hybrid);
    assert_eq!(inspection.claims.access_expires_at, Some(1_900_000_000));
    // An opaque (non-JWT) access token falls back to the id_token identity.
    let opaque = file("user-a", "ws-a", "opaque-token".into(), "RA1", "ws-a");
    let inspection = OpaqueAuth::inspect(&opaque).unwrap();
    assert!(!inspection.hybrid);
    assert_eq!(
        inspection.claims.token_owner(),
        (Some("user-a".into()), Some("ws-a".into()))
    );
}

const FAKE_KEY: &str = "sk-proj-SENTINEL-0123456789abcdef";
#[test]
fn api_key_payload_has_the_shape_codex_login_writes() {
    let bytes = api_key_auth_payload(FAKE_KEY).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Exactly the mode and the key: no tokens, no refresh stamp, no null placeholders.
    assert_eq!(
        value,
        json!({"auth_mode":"apikey","OPENAI_API_KEY":FAKE_KEY})
    );
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.find("auth_mode").unwrap() < text.find("OPENAI_API_KEY").unwrap());
    let auth = OpaqueAuth::parse(bytes.clone()).unwrap();
    assert_eq!(auth.kind(), AuthKind::ApiKey);
    assert_eq!(auth.as_bytes(), bytes);
    // Both the file Codex writes and the older file without a mode are API-key logins.
    for file in [
        format!(r#"{{"auth_mode":"apikey","OPENAI_API_KEY":"{FAKE_KEY}"}}"#),
        format!(r#"{{"OPENAI_API_KEY":"{FAKE_KEY}"}}"#),
        format!(r#"{{"auth_mode":"apikey","OPENAI_API_KEY":"{FAKE_KEY}","tokens":null}}"#),
    ] {
        let inspection = OpaqueAuth::inspect(file.as_bytes()).unwrap();
        assert_eq!(inspection.kind, AuthKind::ApiKey, "{file}");
        assert!(!inspection.hybrid);
        assert_eq!(
            OpaqueAuth::parse(file.clone().into_bytes()).unwrap().kind(),
            AuthKind::ApiKey
        );
    }
    // A key next to tokens, or in another mode, is never an API-key login.
    for file in [
        format!(r#"{{"auth_mode":"chatgpt","OPENAI_API_KEY":"{FAKE_KEY}"}}"#),
        format!(
            r#"{{"auth_mode":"apikey","OPENAI_API_KEY":"{FAKE_KEY}","tokens":{{"id_token":"i"}}}}"#
        ),
        format!(
            r#"{{"auth_mode":"apikey","OPENAI_API_KEY":"{FAKE_KEY}","personal_access_token":"p"}}"#
        ),
    ] {
        assert_eq!(
            OpaqueAuth::inspect(file.as_bytes()).unwrap().kind,
            AuthKind::Unsupported,
            "{file}"
        );
    }
    // Nothing prints the key.
    let inspection = OpaqueAuth::inspect(&bytes).unwrap();
    let printed = format!("{auth:?} {:?} {inspection:?}", auth.routing_claims());
    assert!(!printed.contains("SENTINEL") && !printed.contains("sk-"));
}
#[test]
fn api_key_validation_is_strict_and_never_echoes_the_input() {
    // Pasted whitespace around the key is dropped, the key itself is kept as typed.
    for padded in [
        FAKE_KEY.to_owned(),
        format!("  {FAKE_KEY}\r\n"),
        format!("\t{FAKE_KEY} "),
    ] {
        assert_eq!(validate_api_key(&padded).unwrap().as_str(), FAKE_KEY);
    }
    let at = |length: usize| format!("sk-{}", "a".repeat(length - 3));
    assert!(validate_api_key(&at(20)).is_ok());
    assert!(validate_api_key(&at(512)).is_ok());
    for (bad, why) in [
        (String::new(), "empty"),
        ("   \n".into(), "blank"),
        (at(19), "too short"),
        (at(513), "too long"),
        (format!("pk-{}", "a".repeat(30)), "prefix"),
        (format!("SK-{}", "a".repeat(30)), "prefix case"),
        (
            format!("sk-{} {}", "a".repeat(15), "b".repeat(15)),
            "inner space",
        ),
        (
            format!("sk-{}\tb{}", "a".repeat(15), "b".repeat(15)),
            "inner tab",
        ),
        (
            format!("sk-{}\nb{}", "a".repeat(15), "b".repeat(15)),
            "inner newline",
        ),
        (
            format!("sk-{}\u{7}{}", "a".repeat(15), "b".repeat(15)),
            "control",
        ),
        (
            format!("sk-{}é{}", "a".repeat(15), "b".repeat(15)),
            "non-ASCII",
        ),
        (
            format!("sk-{}\u{200b}{}", "a".repeat(15), "b".repeat(15)),
            "invisible",
        ),
    ] {
        let error = validate_api_key(&bad).unwrap_err();
        assert_eq!(error, CodexError::InvalidApiKey, "{why}");
        // The error carries nothing of the input.
        let printed = format!("{error} {error:?}");
        assert!(
            !printed.contains("sk-") && !printed.contains("aaaa"),
            "{why}"
        );
    }
}
#[test]
fn api_key_tail_and_comparison_ignore_formatting_and_never_expose_more_than_four_characters() {
    let pretty = api_key_auth_payload(FAKE_KEY).unwrap();
    let compact = format!(r#"{{"OPENAI_API_KEY":"{FAKE_KEY}","auth_mode":"apikey"}}"#).into_bytes();
    assert_eq!(api_key_tail(&pretty).as_deref(), Some("cdef"));
    assert_eq!(api_key_tail(&compact).as_deref(), Some("cdef"));
    assert!(same_api_key(&pretty, &compact));
    let other = api_key_auth_payload("sk-proj-SENTINEL-0123456789abcdeg").unwrap();
    assert!(!same_api_key(&pretty, &other));
    // A ChatGPT login or an unreadable file has neither a tail nor a match.
    let chatgpt = payload("workspace-a");
    assert_eq!(api_key_tail(&chatgpt), None);
    assert_eq!(api_key_tail(b"not json"), None);
    assert!(!same_api_key(&chatgpt, &chatgpt));
    assert!(!same_api_key(
        br#"{"OPENAI_API_KEY":" "}"#,
        br#"{"OPENAI_API_KEY":" "}"#
    ));
    // A key too short to hide behind four characters shows nothing.
    assert_eq!(api_key_tail(br#"{"OPENAI_API_KEY":"sk-short"}"#), None);
}
