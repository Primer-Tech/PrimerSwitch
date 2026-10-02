#![cfg(feature = "fixture-process")]
use provider_codex::*;
use std::path::PathBuf;
fn fixture_context(mode: &str) -> (tempfile::TempDir, CodexContext) {
    let home = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(
        home.path().join(".primerswitch-codex-fixture"),
        format!("PrimerSwitch fake Codex child v1\n{mode}\n"),
    )
    .unwrap();
    let context = CodexContext::isolated(home.path().into(), home.path().into()).unwrap();
    (home, context)
}
fn candidate() -> CodexCandidate {
    discover_candidates(&DiscoveryOptions {
        explicit_native: Some(PathBuf::from(env!(
            "CARGO_BIN_EXE_primerswitch-codex-fixture"
        ))),
        ..Default::default()
    })
    .unwrap()
    .pop()
    .unwrap()
}
#[tokio::test]
async fn native_fake_managed_login_and_child_cleanup() {
    let (_home, context) = fixture_context("normal");
    let executable = verify_executable(candidate(), &context).await.unwrap();
    assert_eq!(executable.version(), "0.160.0");
    let mut client = CodexClient::start_login(&executable, context)
        .await
        .unwrap();
    client.begin_browser_login().await.unwrap();
    assert_eq!(client.poll_login().unwrap(), LoginPoll::Completed);
    assert!(matches!(
        client.read_account().await.unwrap().account,
        Some(CodexAccount::Chatgpt { .. })
    ));
    client.inspect_configuration().await.unwrap();
    client.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_fake_cancel_does_not_touch_auth() {
    let (home, context) = fixture_context("cancel");
    let executable = verify_executable(candidate(), &context).await.unwrap();
    let mut client = CodexClient::start_login(&executable, context)
        .await
        .unwrap();
    client.begin_browser_login().await.unwrap();
    assert_eq!(client.poll_login().unwrap(), LoginPoll::Pending);
    assert_eq!(
        client.cancel_login().await.unwrap(),
        CancelOutcome::Cancelled
    );
    client.shutdown().await.unwrap();
    assert!(!home.path().join("auth.json").exists());
}
#[tokio::test]
async fn native_fake_version_limits_and_wrong_home() {
    for (mode, expected) in [
        ("old-version", CodexError::UnsupportedVersion),
        ("version-overflow", CodexError::OutputLimit),
    ] {
        let (_home, context) = fixture_context(mode);
        assert_eq!(
            verify_executable(candidate(), &context).await.unwrap_err(),
            expected
        );
    }
    let (home, context) = fixture_context("wrong-home");
    std::fs::create_dir(home.path().join("wrong")).unwrap();
    let executable = verify_executable(candidate(), &context).await.unwrap();
    assert_eq!(
        CodexClient::start_login(&executable, context)
            .await
            .unwrap_err(),
        CodexError::UnsafeContext
    );
}
#[tokio::test]
async fn native_fake_output_overflow_and_active_quota() {
    let (_home, context) = fixture_context("frame-overflow");
    let executable = verify_executable(candidate(), &context).await.unwrap();
    assert_eq!(
        CodexClient::start_login(&executable, context)
            .await
            .unwrap_err(),
        CodexError::OutputLimit
    );
    let (_home, isolated) = fixture_context("normal");
    let executable = verify_executable(candidate(), &isolated).await.unwrap();
    let active =
        CodexContext::approved_file(isolated.home().into(), isolated.home().into()).unwrap();
    let mut client = CodexClient::start_active(&executable, active)
        .await
        .unwrap();
    let quota = client.read_rate_limits().await.unwrap();
    assert_eq!(quota.account_id.as_deref(), Some("fixture-workspace"));
    assert_eq!(quota.ordinary_usage_allowed, Some(true));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_fake_stalled_child_is_killed_and_reaped() {
    let (home, context) = fixture_context("shutdown-hang");
    let executable = verify_executable(candidate(), &context).await.unwrap();
    let client = CodexClient::start_login(&executable, context)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(9), client.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(!home.path().join("unexpected-survivor").exists());
    assert!(!home.path().join("auth.json").exists());
}
