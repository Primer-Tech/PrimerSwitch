//! Offline fixtures derived from pinned Codex 0.160.0 public serializers.
//! No executable, authentication store, network, or native keyring is accessed.
use super::*;
use provider_codex::{CodexError, ConfigurationInspection};
use serde_json::{Value, json};

fn pinned_config() -> Value {
    // Config is snake_case; ConfigToml default maps are serialized and flattened
    // into ApiConfig. Sources: v2/config.rs:278-315, config_toml.rs:324-361,
    // config_manager_service.rs:137-149 at pinned a956835d020762cb2b570053af06f643a11c0ecc.
    serde_json::from_str(
        r#"{
        "model":null,"review_model":null,"model_context_window":null,
        "model_auto_compact_token_limit":null,"model_auto_compact_token_limit_scope":null,
        "model_provider":null,"approval_policy":null,"approvals_reviewer":null,
        "sandbox_mode":null,"sandbox_workspace_write":null,
        "forced_chatgpt_workspace_id":null,"forced_login_method":null,
        "web_search":null,"tools":null,"instructions":null,"developer_instructions":null,
        "compact_prompt":null,"model_reasoning_effort":null,"model_reasoning_summary":null,
        "model_verbosity":null,"service_tier":null,"analytics":null,"apps":null,
        "browser_use":null,"computer_use":null,"desktop":null,
        "model_providers":{},"profiles":{},"profile":null,
        "cli_auth_credentials_store":null,"chatgpt_base_url":null,
        "allow_login_shell":true,"features":{
            "api_key_model_discovery":false,"auth_elicitation":false,
            "background_paginated_rollout_migration":false,
            "codex_apps_mcp_2026_07_28":false,"mcp_2026_07_28":false,
            "memories":false,"mentions_v2":false,"remote_control":false,
            "remote_plugin":false,"tool_suggest":false,"windows_sandbox_service":false
        }
    }"#,
    )
    .unwrap()
}
fn inspection(config: Value, requirements: Value) -> ConfigurationInspection {
    ConfigurationInspection::from_response_bytes(
        &serde_json::to_vec(&json!({"config":config,"origins":{},"layers":[]})).unwrap(),
        &serde_json::to_vec(&json!({"requirements":requirements})).unwrap(),
    )
    .unwrap()
}
#[test]
fn pinned_default_config_shape_accepts_generated_empty_maps() {
    assert_eq!(
        validate_inspection(&inspection(pinned_config(), Value::Null)).unwrap(),
        None
    );
    let mut config = pinned_config();
    config["cli_auth_credentials_store"] = json!("file");
    config["model_provider"] = json!("openai");
    config["chatgpt_base_url"] = json!("https://chatgpt.com/backend-api/");
    config["model"] = json!("fixture-model");
    assert_eq!(
        validate_inspection(&inspection(config, Value::Null)).unwrap(),
        Some("fixture-model".into())
    );
}
#[test]
fn flattened_effective_overrides_cannot_hide_behind_default_provider() {
    for (key, value) in [
        (
            "model_providers",
            json!({"openai":{"base_url":"https://invalid.example/"}}),
        ),
        ("model_providers", json!([])),
        ("profiles", json!({"alternate":{"model_provider":"custom"}})),
        ("profiles", json!("malformed")),
        ("profile", json!("alternate")),
        ("openai_base_url", json!("https://invalid.example/")),
        ("chatgpt_base_url", json!("https://invalid.example/")),
        ("forced_login_method", json!("chatgpt")),
        ("forced_chatgpt_workspace_id", json!("fixture-workspace")),
        ("cli_auth_credentials_store", json!("auto")),
        ("model_provider", json!("custom")),
        ("unknown_auth_route", json!("fixture-only")),
    ] {
        let mut config = pinned_config();
        config["model_provider"] = json!("openai");
        config[key] = value;
        assert!(
            validate_inspection(&inspection(config, Value::Null)).is_err(),
            "accepted fixed fixture key {key}"
        );
    }
}
#[test]
fn camel_case_managed_auth_requirements_fail_closed() {
    // ConfigRequirements uses camelCase, unlike effective Config.
    for (key, value) in [
        ("modelProvider", json!("openai")),
        ("modelProviders", json!({})),
        ("allowedLoginMethods", json!(["api", "chatgpt"])),
        ("allowedLoginMethods", json!([])),
        ("allowedLoginMethods", json!("malformed")),
        ("cliAuthCredentialsStore", json!("file")),
        ("chatgptBaseUrl", json!("https://chatgpt.com/backend-api/")),
        ("enforceResidency", json!("us")),
    ] {
        let mut requirements = json!({});
        requirements[key] = value;
        assert!(
            validate_inspection(&inspection(pinned_config(), requirements)).is_err(),
            "accepted fixed fixture requirement {key}"
        );
    }
}
#[test]
fn null_known_requirements_and_unknown_null_are_not_restrictions() {
    let requirements = json!({"modelProvider":null,"modelProviders":null,
        "allowedLoginMethods":null,"cliAuthCredentialsStore":null,
        "chatgptBaseUrl":null,"enforceResidency":null,"futureRequirement":null});
    assert!(validate_inspection(&inspection(pinned_config(), requirements)).is_ok());
}
#[test]
fn unknown_non_null_requirements_are_rejected() {
    for value in [json!(true), json!({}), json!([]), json!("fixture-only")] {
        assert!(
            validate_inspection(&inspection(
                pinned_config(),
                json!({"futureRequirement":value})
            ))
            .is_err()
        );
    }
}
#[test]
fn malformed_inspection_shapes_are_rejected_and_debug_is_redacted() {
    for effective in [
        json!({"config":[],"origins":{},"layers":[]}),
        json!({"config":{},"origins":[],"layers":[]}),
        json!({"config":{},"origins":{},"layers":{}}),
    ] {
        assert_eq!(
            ConfigurationInspection::from_response_bytes(
                &serde_json::to_vec(&effective).unwrap(),
                b"{\"requirements\":null}"
            )
            .unwrap_err(),
            CodexError::Protocol
        );
    }
    for requirements in [
        b"{}".as_slice(),
        b"{\"requirements\":[]}".as_slice(),
        b"{\"requirements\":false}".as_slice(),
    ] {
        assert_eq!(
            ConfigurationInspection::from_response_bytes(
                b"{\"config\":{},\"origins\":{},\"layers\":[]}",
                requirements
            )
            .unwrap_err(),
            CodexError::Protocol
        );
    }
    let object = inspection(json!({"unknown_auth_route":"SENTINEL_SECRET"}), Value::Null);
    assert!(!format!("{object:?}").contains("SENTINEL_SECRET"));
}

#[test]
fn malformed_known_requirement_values_fail_closed() {
    for requirements in [
        json!({"allowedApprovalPolicies":"malformed"}),
        json!({"featureRequirements":["malformed"]}),
        json!({"network":false}),
        json!({"application":[]}),
    ] {
        assert!(validate_inspection(&inspection(pinned_config(), requirements)).is_err());
    }
}
