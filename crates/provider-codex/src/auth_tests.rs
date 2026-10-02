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
