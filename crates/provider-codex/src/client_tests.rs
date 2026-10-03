use super::*;
#[derive(Default)]
struct FixtureState {
    requests: Vec<Value>,
    closed: bool,
}
struct Scripted {
    plans: VecDeque<(&'static str, Vec<Value>)>,
    frames: VecDeque<SecretJson>,
    state: Arc<Mutex<FixtureState>>,
    hang: bool,
}
#[async_trait]
impl Transport for Scripted {
    async fn send(&mut self, bytes: &[u8]) -> Result<(), CodexError> {
        let frame = SecretJson::parse(bytes)?;
        self.state.lock().unwrap().requests.push(frame.0.clone());
        let (method, values) = self.plans.pop_front().ok_or(CodexError::Protocol)?;
        if frame.0["method"] != method {
            return Err(CodexError::Protocol);
        }
        for mut value in values {
            if value["id"] == "current" {
                value["id"] = frame.0["id"].clone();
            }
            self.frames.push_back(SecretJson(value));
        }
        Ok(())
    }
    async fn recv(&mut self) -> Result<SecretJson, CodexError> {
        if let Some(frame) = self.frames.pop_front() {
            Ok(frame)
        } else if self.hang {
            std::future::pending().await
        } else {
            Err(CodexError::ChildExited)
        }
    }
    fn try_recv(&mut self) -> Result<Option<SecretJson>, CodexError> {
        Ok(self.frames.pop_front())
    }
    fn abort(&mut self) {
        self.state.lock().unwrap().closed = true;
    }
    async fn shutdown(&mut self) -> Result<(), CodexError> {
        self.abort();
        Ok(())
    }
}
fn fake(
    isolated: bool,
    plans: Vec<(&'static str, Vec<Value>)>,
) -> (CodexClient, Arc<Mutex<FixtureState>>) {
    let state = Arc::new(Mutex::new(FixtureState::default()));
    let transport = Scripted {
        plans: plans.into(),
        frames: VecDeque::new(),
        state: state.clone(),
        hang: false,
    };
    (
        CodexClient {
            transport: Box::new(transport),
            id: 0,
            pending: None,
            early: VecDeque::new(),
            kind: if isolated {
                ContextKind::Isolated
            } else {
                ContextKind::Approved
            },
            dead: false,
        },
        state,
    )
}
fn reply(result: Value) -> Value {
    json!({"id":"current","result":result})
}
fn challenge() -> Value {
    reply(
        json!({"type":"chatgpt","loginId":"fixture-login","authUrl":"https://auth.openai.com/oauth/authorize?state=SECRET-STATE"}),
    )
}
fn completion(id: Value, success: bool) -> Value {
    json!({"method":"account/login/completed","params":{"loginId":id,"success":success,"error":"SECRET-ERROR"}})
}
fn account() -> Value {
    reply(
        json!({"account":{"type":"chatgpt","email":"fixture@example.invalid","planType":"plus"},"requiresOpenaiAuth":true}),
    )
}
fn private_context() -> (tempfile::TempDir, CodexContext) {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let context = CodexContext::isolated(dir.path().to_owned(), dir.path().to_owned()).unwrap();
    (dir, context)
}
#[tokio::test]
async fn handshake_order_and_private_home() {
    let (_dir, context) = private_context();
    let (mut client, state) = fake(
        true,
        vec![
            (
                "initialize",
                vec![reply(
                    json!({"codexHome":context.home,"platformOs":std::env::consts::OS,"platformFamily":if cfg!(windows){"windows"}else{"unix"},"userAgent":"SECRET"}),
                )],
            ),
            ("initialized", vec![]),
        ],
    );
    client.initialize(&context).await.unwrap();
    let state = state.lock().unwrap();
    assert_eq!(
        state.requests[0]["params"]["capabilities"]["experimentalApi"],
        false
    );
    assert_eq!(state.requests[1]["method"], "initialized");
    assert!(state.requests[0].get("jsonrpc").is_none());
}
#[tokio::test]
async fn early_completion_and_false_refresh_account() {
    let (mut client, state) = fake(
        true,
        vec![
            (
                "account/login/start",
                vec![completion(json!("fixture-login"), true), challenge()],
            ),
            ("account/read", vec![account()]),
        ],
    );
    let url = client.begin_browser_login().await.unwrap();
    assert!(!format!("{url:?}").contains("SECRET"));
    assert_eq!(client.poll_login().unwrap(), LoginPoll::Completed);
    assert!(matches!(
        client.read_account().await.unwrap().account,
        Some(crate::CodexAccount::Chatgpt { .. })
    ));
    assert_eq!(
        state.lock().unwrap().requests[1]["params"],
        json!({"refreshToken":false})
    );
}
#[tokio::test]
async fn unrelated_and_null_completion_never_authorize() {
    let (mut client, _) = fake(
        true,
        vec![(
            "account/login/start",
            vec![
                challenge(),
                completion(json!("other"), true),
                completion(Value::Null, true),
            ],
        )],
    );
    client.begin_browser_login().await.unwrap();
    for _ in 0..100 {
        assert_eq!(client.poll_login().unwrap(), LoginPoll::Pending);
    }
}
#[tokio::test]
async fn cancel_and_failed_completion() {
    let (mut client, state) = fake(
        true,
        vec![
            ("account/login/start", vec![challenge()]),
            (
                "account/login/cancel",
                vec![reply(json!({"status":"canceled"}))],
            ),
        ],
    );
    client.begin_browser_login().await.unwrap();
    assert_eq!(
        client.cancel_login().await.unwrap(),
        CancelOutcome::Cancelled
    );
    assert_eq!(
        state.lock().unwrap().requests[1]["params"],
        json!({"loginId":"fixture-login"})
    );
    assert_eq!(
        client.cancel_login().await.unwrap(),
        CancelOutcome::AlreadyFinished
    );
    let (mut failed, _) = fake(
        true,
        vec![(
            "account/login/start",
            vec![challenge(), completion(json!("fixture-login"), false)],
        )],
    );
    failed.begin_browser_login().await.unwrap();
    assert_eq!(failed.poll_login().unwrap(), LoginPoll::Failed);
}
#[tokio::test]
async fn malformed_url_errors_and_ambiguous_envelopes_redacted() {
    for frame in [
        reply(
            json!({"type":"chatgpt","loginId":"x","authUrl":"https://auth.openai.com.evil.invalid/oauth/authorize?SECRET"}),
        ),
        json!({"id":"current","error":{"code":500,"message":"SECRET-ERROR","data":"SECRET-DATA"}}),
        json!({"id":"current","result":{},"error":{"message":"SECRET"}}),
    ] {
        let (mut client, _) = fake(true, vec![("account/login/start", vec![frame])]);
        let error = client.begin_browser_login().await.unwrap_err();
        assert!(!format!("{error:?} {error}").contains("SECRET"));
    }
}
#[tokio::test]
async fn deadlines_and_flood_close_transport() {
    let state = Arc::new(Mutex::new(FixtureState::default()));
    let transport = Scripted {
        plans: vec![("account/read", vec![])].into(),
        frames: VecDeque::new(),
        state: state.clone(),
        hang: true,
    };
    let mut client = CodexClient {
        transport: Box::new(transport),
        id: 0,
        pending: None,
        early: VecDeque::new(),
        kind: ContextKind::Approved,
        dead: false,
    };
    assert_eq!(
        client
            .request("account/read", json!({}), 0)
            .await
            .unwrap_err(),
        CodexError::Timeout
    );
    assert!(state.lock().unwrap().closed);
    let notifications = std::iter::repeat_n(json!({"method":"unknown","params":null}), 65)
        .chain([account()])
        .collect();
    let (mut client, state) = fake(false, vec![("account/read", notifications)]);
    assert_eq!(
        client.read_account().await.unwrap_err(),
        CodexError::OutputLimit
    );
    assert!(state.lock().unwrap().closed);
}
#[tokio::test]
async fn login_poll_timeout_aborts() {
    let (mut client, state) = fake(true, vec![("account/login/start", vec![challenge()])]);
    client.begin_browser_login().await.unwrap();
    client.pending.as_mut().unwrap().deadline = Instant::now();
    assert_eq!(client.poll_login().unwrap_err(), CodexError::Timeout);
    assert!(state.lock().unwrap().closed);
}
#[tokio::test]
async fn quota_preserves_all_buckets_nulls_and_native_amounts() {
    let bucket = json!({"limitId":"codex","primary":{"usedPercent":125,"windowDurationMins":300,"resetsAt":null},"secondary":null,"planType":null,"credits":{"hasCredits":true,"unlimited":false,"balance":"1.2300"},"futureBucket":"retained"});
    let quota = json!({"ordinaryUsageAllowed":null,"accountId":null,"rateLimits":bucket,"rateLimitsByLimitId":{"codex":bucket,"future-model":{"limitId":"future-model","primary":null,"secondary":{"usedPercent":17,"windowDurationMins":10080,"resetsAt":123},"planType":"future-plan"}},"rateLimitResetCredits":{"availableCount":2,"credits":null},"futureResponse":true});
    let (mut client, state) = fake(false, vec![("account/rateLimits/read", vec![reply(quota)])]);
    let observed = client.read_rate_limits().await.unwrap();
    assert_eq!(
        observed.rate_limits.primary.as_ref().unwrap().used_percent,
        125
    );
    assert!(observed.account_id.is_none() && observed.ordinary_usage_allowed.is_none());
    assert_eq!(
        observed
            .rate_limits
            .credits
            .as_ref()
            .unwrap()
            .balance
            .as_deref(),
        Some("1.2300")
    );
    let buckets = observed.rate_limits_by_limit_id.as_ref().unwrap();
    assert_eq!(buckets.len(), 2);
    assert!(buckets["future-model"].primary.is_none());
    assert_eq!(
        serde_json::to_value(&observed).unwrap()["futureResponse"],
        true
    );
    assert_eq!(
        state.lock().unwrap().requests[0]["params"],
        json!({"supportsLunaReserve":false,"excludeResetCreditDetails":true})
    );
    let (mut isolated, _) = fake(true, vec![]);
    assert_eq!(
        isolated.read_rate_limits().await.unwrap_err(),
        CodexError::UnsafeContext
    );
}
#[tokio::test]
async fn configuration_inspection_is_private_and_redacted() {
    let (mut client, state) = fake(
        true,
        vec![
            (
                "config/read",
                vec![reply(
                    json!({"config":{"cli_auth_credentials_store":"file","secret":"SECRET"},"origins":{},"layers":[]}),
                )],
            ),
            (
                "configRequirements/read",
                vec![reply(json!({"requirements":null}))],
            ),
        ],
    );
    let inspection = client.inspect_configuration().await.unwrap();
    assert_eq!(state.lock().unwrap().requests[1]["params"], Value::Null);
    assert_eq!(
        inspection.effective_config()["config"]["cli_auth_credentials_store"],
        "file"
    );
    assert!(!format!("{inspection:?}").contains("SECRET"));
}
#[tokio::test]
async fn framing_fragments_crlf_and_multiple_lines() {
    let (mut writer, mut reader) = tokio::io::duplex(128);
    let (tx, mut rx) = mpsc::channel(64);
    let pump = tokio::spawn(async move { read_frames(&mut reader, tx).await });
    writer.write_all(b"{\"id\":1,\"res").await.unwrap();
    writer
        .write_all(b"ult\":{}}\r\n{\"method\":\"x\"}\n")
        .await
        .unwrap();
    drop(writer);
    assert_eq!(rx.recv().await.unwrap().0["id"], 1);
    assert_eq!(rx.recv().await.unwrap().0["method"], "x");
    assert_eq!(pump.await.unwrap().unwrap_err(), CodexError::ChildExited);
}
#[tokio::test]
async fn invalid_duplicate_unterminated_oversized_frames() {
    for bytes in [
        b"{bad}\n".to_vec(),
        b"{\"id\":1,\"id\":2}\n".to_vec(),
        vec![0xff, b'\n'],
        b"{}".to_vec(),
        vec![b'x'; MAX_FRAME + 1],
    ] {
        let mut reader = std::io::Cursor::new(bytes);
        let (tx, _rx) = mpsc::channel(64);
        assert!(matches!(
            read_frames(&mut reader, tx).await.unwrap_err(),
            CodexError::Protocol | CodexError::OutputLimit
        ));
    }
}
#[tokio::test]
async fn frame_queue_cap() {
    let mut reader = std::io::Cursor::new(b"{}\n".repeat(65));
    let (tx, _rx) = mpsc::channel(64);
    assert_eq!(
        read_frames(&mut reader, tx).await.unwrap_err(),
        CodexError::OutputLimit
    );
}
#[test]
fn context_and_browser_challenge_redaction() {
    let (dir, _context) = private_context();
    std::fs::write(dir.path().join("auth.json"), "SECRET").unwrap();
    assert_eq!(
        CodexContext::isolated(dir.path().to_owned(), dir.path().to_owned()).unwrap_err(),
        CodexError::UnsafeContext
    );
    assert!(
        !format!(
            "{:?}",
            BrowserLoginChallenge::from_authorization_url(
                "https://auth.openai.com/oauth/authorize?SECRET".into()
            )
            .unwrap()
        )
        .contains("SECRET")
    );
}

#[tokio::test]
async fn malformed_configuration_response_closes_transport() {
    let (mut client, state) = fake(
        false,
        vec![
            (
                "config/read",
                vec![reply(json!({"config":{},"origins":{},"layers":[]}))],
            ),
            (
                "configRequirements/read",
                vec![reply(json!({"requirements":[]}))],
            ),
        ],
    );
    assert_eq!(
        client.inspect_configuration().await.unwrap_err(),
        CodexError::Protocol
    );
    assert!(state.lock().unwrap().closed);
    assert_eq!(
        client.read_account().await.unwrap_err(),
        CodexError::ChildExited
    );
}

#[cfg(unix)]
#[test]
fn approved_home_allows_native_traversal_but_rejects_shared_writes() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("approved-home");
    let cwd = fixture.path().join("owned-cwd");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&cwd).unwrap();
    std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
    let accepted = CodexContext::approved_file(home.clone(), cwd.clone()).unwrap();
    assert_eq!(accepted.home(), std::fs::canonicalize(&home).unwrap());
    assert_eq!(
        std::fs::metadata(&home).unwrap().permissions().mode() & 0o777,
        0o755
    );
    for mode in [0o775, 0o777] {
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            CodexContext::approved_file(home.clone(), cwd.clone()).unwrap_err(),
            CodexError::UnsafeContext
        );
    }
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        CodexContext::approved_file(home, cwd).unwrap_err(),
        CodexError::UnsafeContext
    );
}
#[cfg(unix)]
#[test]
fn isolated_home_and_cwd_stay_strictly_private() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("private-home");
    let cwd = home.join("owned-cwd");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&cwd).unwrap();
    std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o700)).unwrap();
    for mode in [0o755, 0o775, 0o777] {
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            CodexContext::isolated(home.clone(), cwd.clone()).unwrap_err(),
            CodexError::UnsafeContext
        );
    }
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    CodexContext::isolated(home.clone(), cwd.clone()).unwrap();
    std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        CodexContext::isolated(home, cwd).unwrap_err(),
        CodexError::UnsafeContext
    );
}
#[test]
fn native_command_clears_overrides_and_preserves_fixed_context() {
    let (_fixture, context) = private_context();
    let mut command = Command::new("fake-unexecuted-fixture");
    command.env("CODEX_API_KEY", "fixture-only-secret");
    command.env("CODEX_HOME", "fixture-wrong-home");
    command.env("PRIMERSWITCH_UNSAFE_FIXTURE_ENV", "fixture-only");
    context.configure(&mut command);
    let env: std::collections::BTreeMap<_, _> = command.as_std().get_envs().collect();
    assert!(!env.contains_key(std::ffi::OsStr::new("CODEX_API_KEY")));
    assert!(!env.contains_key(std::ffi::OsStr::new("PRIMERSWITCH_UNSAFE_FIXTURE_ENV")));
    for name in [
        "CODEX_HOME",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "XDG_CONFIG_HOME",
    ] {
        assert_eq!(
            env[std::ffi::OsStr::new(name)],
            Some(context.home().as_os_str())
        );
    }
    assert_eq!(
        command.as_std().get_current_dir(),
        Some(context.cwd.as_path())
    );
}
