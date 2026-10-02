use serde_json::Value;
use std::process::Command;

fn run(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_switcher-diagnostics"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn fixture_cli_reports_only_safe_metadata() {
    let output = run(&["--fixtures"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["networkRequests"], 0);
    assert!(value["checks"].as_array().unwrap().len() >= 10);
    let text = String::from_utf8(output.stdout).unwrap();
    for forbidden in [
        "SENTINEL",
        "accessToken",
        "refreshToken",
        "credentials",
        "fixture@example.invalid",
    ] {
        assert!(!text.contains(forbidden));
    }
    assert!(output.stderr.is_empty());
}

#[test]
fn demonstration_snapshot_is_redacted_and_read_only() {
    let output = run(&["--demo-snapshot"]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["demo"], true);
    assert_eq!(value["provider"], "claude");
    assert!(!value["accounts"].as_array().unwrap().is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    for forbidden in [
        "accessToken",
        "refreshToken",
        "credentials",
        "claudeAiOauth",
    ] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn unsupported_arguments_fail_without_echoing_supplied_secrets() {
    for arguments in [
        &["--live"][..],
        &["--fixtures", "--home", "SECRET_SENTINEL"][..],
        &["SECRET_SENTINEL"][..],
    ] {
        let output = run(arguments);
        assert_eq!(output.status.code(), Some(2));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("SECRET_SENTINEL")
        );
    }
    assert!(run(&["--help"]).status.success());
}
