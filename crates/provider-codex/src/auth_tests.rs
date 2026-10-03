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
