//! Hermetic runtime fixtures: no real homes, Codex processes, daemons or providers.
use super::*;
use crate::{Clock, RuntimeHandle};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use provider_claude::{ClaudeClient, ClientError, HttpRequest, HttpResponse, Transport};
use serde_json::json;
use std::{
    collections::VecDeque,
    fs,
    sync::{
        Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use switcher_platform::{ActiveStore, CliPaths};

fn jwt(claims: serde_json::Value) -> String {
    format!(
        "header.{}.signature-secret-sentinel",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}
fn auth_with(user: &str, workspace: &str, revision: &str, access: String, iat: i64) -> Vec<u8> {
    let id_token = jwt(json!({"iat":iat,"email":format!("{user}@example.invalid"),
        "https://api.openai.com/auth":{"chatgpt_user_id":user,"chatgpt_account_id":workspace,
        "chatgpt_plan_type":"plus"}}));
    let mut bytes = serde_json::to_vec_pretty(&json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,
        "tokens":{"id_token":id_token,"access_token":access,
            "refresh_token":format!("refresh-secret-sentinel-{revision}"),"account_id":workspace},
        "future_extension":{"revision":revision,"opaque":[1,{"keep":true}]}}))
    .unwrap();
    bytes.push(b'\n');
    bytes
}
fn auth(user: &str, workspace: &str, revision: &str) -> Vec<u8> {
    auth_with(
        user,
        workspace,
        revision,
        format!("access-secret-sentinel-{revision}"),
        100,
    )
}
/// The file an old process holding `user`/`owner_ws` leaves after refreshing while the
/// file was labelled for `label_ws`: its tokens with the other account's id.
fn hybrid(user: &str, owner_ws: &str, label_ws: &str, revision: &str) -> Vec<u8> {
    let mut value: serde_json::Value =
        serde_json::from_slice(&auth(user, owner_ws, revision)).unwrap();
    value["tokens"]["account_id"] = label_ws.into();
    serde_json::to_vec_pretty(&value).unwrap()
}
fn tokens_of(bytes: &[u8]) -> (String, String) {
    let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    (
        value["tokens"]["refresh_token"].as_str().unwrap().into(),
        value["tokens"]["account_id"].as_str().unwrap().into(),
    )
}
fn quota(workspace: Option<&str>, five: i32, week: i32) -> CodexRateLimits {
    serde_json::from_value(json!({"accountId":workspace,"ordinaryUsageAllowed":true,
        "rateLimits":{"limitId":"codex","primary":{"usedPercent":five,"windowDurationMins":300,"resetsAt":9000},
            "secondary":{"usedPercent":week,"windowDurationMins":10080,"resetsAt":600000},"planType":"pro",
            "credits":{"hasCredits":true,"unlimited":false,"balance":"12.345"}},
        "rateLimitsByLimitId":{"codex":{"limitId":"codex","primary":null,"secondary":null},
            "code-review":{"limitId":"code-review","limitName":"Code review",
            "primary":{"usedPercent":4,"windowDurationMins":1440,"resetsAt":null},"secondary":null}},
        "rateLimitResetCredits":null})).unwrap()
}
fn default_inspection() -> ConfigurationInspection {
    ConfigurationInspection::from_response_bytes(br#"{"config":{"cli_auth_credentials_store":"file","model_provider":"openai","model":"fixture-model"},"origins":{},"layers":[]}"#,
        br#"{"requirements":null}"#).unwrap()
}

struct Plan {
    quota: Result<CodexRateLimits, CodexError>,
    login: LoginPoll,
    login_auth: Option<Vec<u8>>,
    rotate_quota: Option<Vec<u8>>,
    rotate_shutdown: Option<Vec<u8>>,
}
impl Plan {
    fn quota(result: Result<CodexRateLimits, CodexError>) -> Self {
        Self {
            quota: result,
            login: LoginPoll::Completed,
            login_auth: None,
            rotate_quota: None,
            rotate_shutdown: None,
        }
    }
}
#[derive(Default)]
struct FakeFactory {
    plans: Mutex<VecDeque<Plan>>,
    calls: Arc<Mutex<Vec<String>>>,
}
impl FakeFactory {
    fn enqueue(&self, plan: Plan) {
        self.plans.lock().unwrap().push_back(plan);
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
#[async_trait]
impl ClientFactory for FakeFactory {
    async fn start(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        context: CodexContext,
        role: ClientRole,
    ) -> Result<Box<dyn CodexService>, CodexError> {
        assert!(executable.is_none(), "fixtures never run a native Codex");
        self.calls.lock().unwrap().push(format!("start-{role:?}"));
        let plan = self
            .plans
            .lock()
            .unwrap()
            .pop_front()
            .expect("every fake client is explicitly planned");
        Ok(Box::new(FakeClient {
            home: context.home().to_owned(),
            plan,
            calls: self.calls.clone(),
        }))
    }
}
struct FakeClient {
    home: PathBuf,
    plan: Plan,
    calls: Arc<Mutex<Vec<String>>>,
}
#[async_trait]
impl CodexService for FakeClient {
    async fn inspect_configuration(&mut self) -> Result<ConfigurationInspection, CodexError> {
        self.calls.lock().unwrap().push("inspect".into());
        Ok(default_inspection())
    }
    async fn begin_browser_login(&mut self) -> Result<BrowserLoginChallenge, CodexError> {
        self.calls.lock().unwrap().push("begin".into());
        BrowserLoginChallenge::from_authorization_url(
            "https://auth.openai.com/oauth/authorize?fixture-secret-sentinel".into(),
        )
    }
    fn poll_login(&mut self) -> Result<LoginPoll, CodexError> {
        self.calls.lock().unwrap().push("poll".into());
        if self.plan.login == LoginPoll::Completed
            && let Some(bytes) = self.plan.login_auth.take()
        {
            fs::write(self.home.join("auth.json"), bytes).unwrap();
        }
        Ok(self.plan.login)
    }
    async fn cancel_login(&mut self) -> Result<CancelOutcome, CodexError> {
        self.calls.lock().unwrap().push("cancel".into());
        Ok(CancelOutcome::Cancelled)
    }
    async fn read_account(&mut self) -> Result<AccountObservation, CodexError> {
        self.calls.lock().unwrap().push("account".into());
        Ok(AccountObservation {
            account: Some(CodexAccount::Chatgpt {
                email: None,
                plan_type: "plus".into(),
            }),
            requires_openai_auth: true,
        })
    }
    async fn read_rate_limits(&mut self) -> Result<CodexRateLimits, CodexError> {
        self.calls.lock().unwrap().push("quota".into());
        assert!(self.home.join("auth.json").is_file());
        if let Some(bytes) = self.plan.rotate_quota.take() {
            fs::write(self.home.join("auth.json"), bytes).unwrap();
        }
        self.plan.quota.clone()
    }
    async fn shutdown(&mut self) -> Result<(), CodexError> {
        self.calls.lock().unwrap().push("shutdown".into());
        if let Some(bytes) = self.plan.rotate_shutdown.take() {
            fs::write(self.home.join("auth.json"), bytes).unwrap();
        }
        Ok(())
    }
}
struct FakeDaemon {
    state: Mutex<Result<DaemonState, CodexError>>,
    restarts: Mutex<VecDeque<Result<(), CodexError>>>,
    /// Written into the active auth.json during the next restart (an old process
    /// refreshing while the daemon drains).
    during_restart: Mutex<Option<Vec<u8>>>,
    home: PathBuf,
    calls: Mutex<Vec<&'static str>>,
    environment: Mutex<Option<Vec<(OsString, OsString)>>>,
}
#[async_trait]
impl DaemonControl for FakeDaemon {
    async fn state(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
    ) -> Result<DaemonState, CodexError> {
        assert!(executable.is_none());
        self.calls.lock().unwrap().push("state");
        *self.state.lock().unwrap()
    }
    async fn restart(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        environment: Option<&[(OsString, OsString)]>,
    ) -> Result<(), CodexError> {
        assert!(executable.is_none());
        self.calls.lock().unwrap().push("restart");
        *self.environment.lock().unwrap() = environment.map(<[_]>::to_vec);
        if let Some(bytes) = self.during_restart.lock().unwrap().take() {
            fs::write(self.home.join("auth.json"), bytes).unwrap();
        }
        self.restarts.lock().unwrap().pop_front().unwrap_or(Ok(()))
    }
}
#[derive(Default)]
struct FakeInventory(Mutex<CodexProcessSummary>);
impl ProcessInventory for FakeInventory {
    fn scan(&self, _: &Path) -> CodexProcessSummary {
        *self.0.lock().unwrap()
    }
    fn environment(&self, pid: u32) -> Option<Vec<(OsString, OsString)>> {
        (pid == 4242).then(|| vec![("PATH".into(), "C:\\venv\\Scripts".into())])
    }
    fn elevated(&self) -> bool {
        self.0.lock().unwrap().other_clients == 99
    }
}
#[derive(Default)]
struct FakeSwitcher(Mutex<Option<Vec<u8>>>);
impl SwitcherSource for FakeSwitcher {
    fn exists(&self) -> bool {
        self.0.lock().unwrap().is_some()
    }
    fn read(&self) -> Result<Zeroizing<Vec<u8>>, CodexReason> {
        self.0
            .lock()
            .unwrap()
            .clone()
            .map(Zeroizing::new)
            .ok_or(CodexReason::SwitcherUnavailable)
    }
}
struct Fixture {
    _root: TempDir,
    engine: CodexEngine,
    vault: Vault,
    factory: Arc<FakeFactory>,
    daemon: Arc<FakeDaemon>,
    inventory: Arc<FakeInventory>,
    switcher: Arc<FakeSwitcher>,
    home: PathBuf,
    claude_ciphertext: Vec<u8>,
    claude_path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root = tempfile::Builder::new()
            .prefix("primerswitch-runtime-fixture-")
            .tempdir_in(parent)
            .unwrap();
        let home = create_private_context(root.path().join("active-home")).unwrap();
        fs::write(home.join("auth.json"), auth("user-a", "ws-a", "initial")).unwrap();
        fs::write(
            home.join("config.toml"),
            b"cli_auth_credentials_store = \"file\"\nmodel = \"fixture-model\"\n",
        )
        .unwrap();
        let executable = root.path().join("fixture-codex-native");
        fs::write(&executable, b"non-executable-fixture").unwrap();
        let workspace = tempfile::Builder::new()
            .prefix("owned-cwd-")
            .tempdir_in(root.path())
            .unwrap();
        create_private_context(workspace.path().to_owned()).unwrap();
        let system = root.path().join("system");
        fs::create_dir(&system).unwrap();
        let paths = ContextPaths {
            home: home.clone(),
            system_config: system.join("config.toml"),
            system_requirements: system.join("requirements.toml"),
            legacy_requirements: system.join("managed_config.toml"),
            enforce_native_policy: false,
            project_ancestor_stop: Some(root.path().to_owned()),
        };
        let qualified = paths
            .qualify(&executable, workspace.path(), "0.160.0")
            .unwrap();
        let factory = Arc::new(FakeFactory::default());
        let daemon = Arc::new(FakeDaemon {
            state: Mutex::new(Ok(DaemonState::Running)),
            restarts: Mutex::new(VecDeque::new()),
            during_restart: Mutex::new(None),
            home: home.clone(),
            calls: Mutex::new(Vec::new()),
            environment: Mutex::new(None),
        });
        let inventory = Arc::new(FakeInventory::default());
        let switcher = Arc::new(FakeSwitcher::default());
        let mut engine = CodexEngine::empty(false).with_test_seams(
            factory.clone(),
            daemon.clone(),
            inventory.clone(),
            switcher.clone(),
        );
        engine.installation = Some(Installation {
            executable: None,
            executable_path: executable,
            version: "0.160.0".into(),
            paths,
            workspace,
            qualified,
        });
        engine.availability = CodexAvailability::Supported;
        let vault = Vault::with_key(root.path().join("vault"), [63; 32]).unwrap();
        vault
            .save(
                "runtime-state",
                &json!({"historicalClaude":"claude-secret-sentinel","settings":{"language":"ro"}}),
            )
            .unwrap();
        let claude_path = root.path().join("vault/runtime-state.vault");
        let claude_ciphertext = fs::read(&claude_path).unwrap();
        engine.observe(&vault, 100).unwrap();
        Self {
            _root: root,
            engine,
            vault,
            factory,
            daemon,
            inventory,
            switcher,
            home,
            claude_ciphertext,
            claude_path,
        }
    }
    fn active(&self) -> Vec<u8> {
        fs::read(self.home.join("auth.json")).unwrap()
    }
    fn write_active(&self, bytes: &[u8]) {
        fs::write(self.home.join("auth.json"), bytes).unwrap();
    }
    fn claude_untouched(&self) {
        assert_eq!(fs::read(&self.claude_path).unwrap(), self.claude_ciphertext);
    }
    fn id_of(&self, workspace: &str) -> String {
        self.engine
            .saved
            .accounts
            .iter()
            .find(|a| a.binding.workspace.as_deref() == Some(workspace))
            .unwrap()
            .id
            .clone()
    }
    fn saved_bytes(&self, workspace: &str) -> Vec<u8> {
        let saved: Saved = self.vault.load(RECORD).unwrap().unwrap();
        saved
            .accounts
            .iter()
            .find(|a| a.binding.workspace.as_deref() == Some(workspace))
            .unwrap()
            .auth
            .clone()
    }
    fn add(&mut self, user: &str, workspace: &str, revision: &str) -> String {
        let index = self
            .engine
            .adopt_or_import(
                OpaqueAuth::parse(auth(user, workspace, revision)).unwrap(),
                CodexIdentityEvidence::ManagedLogin,
                100,
            )
            .unwrap();
        self.engine.save(&self.vault).unwrap();
        self.engine.saved.accounts[index].id.clone()
    }
    async fn switch(&mut self, id: &str) -> Result<CodexLastSwitchView, CodexReason> {
        let job = self.engine.begin_switch(id, &self.vault, 200)?.unwrap();
        let mut outcome = job.run().await;
        let mut allow = true;
        while self
            .engine
            .finish_switch(&job, &outcome, &self.vault, 210, allow)?
        {
            allow = false;
            outcome = job.restart_again().await;
        }
        Ok(self.engine.last_switch.clone().unwrap())
    }
    fn view(&self, id: &str) -> CodexAccountView {
        self.engine
            .snapshot(false, 300, &CodexPolicy::default())
            .accounts
            .into_iter()
            .find(|a| a.id == id)
            .unwrap()
    }
}

#[test]
fn observe_adopts_rotations_and_keeps_every_new_login() {
    let mut f = Fixture::new();
    assert_eq!(f.engine.saved.accounts.len(), 1);
    let a = f.id_of("ws-a");
    assert_eq!(f.engine.selected_id.as_deref(), Some(a.as_str()));
    // Codex refreshed the active token: the saved copy follows it.
    let rotated = auth("user-a", "ws-a", "rotated");
    f.write_active(&rotated);
    f.engine.observe(&f.vault, 110).unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 1);
    assert_eq!(f.saved_bytes("ws-a"), rotated);
    // `codex login` with another account in a terminal: kept as a new account.
    f.write_active(&auth("user-b", "ws-b", "external"));
    f.engine.observe(&f.vault, 120).unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 2);
    assert_eq!(f.engine.selected_id, Some(f.id_of("ws-b")));
    assert_eq!(f.saved_bytes("ws-a"), rotated);
    // Unchanged file: no extra vault write.
    f.engine.observe(&f.vault, 130).unwrap();
    f.claude_untouched();
}

#[tokio::test]
async fn switch_keeps_outgoing_tokens_writes_incoming_and_restarts_running_daemon() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    // The active account was refreshed by Codex since the last observation.
    let outgoing = auth("user-a", "ws-a", "refreshed-before-switch");
    f.write_active(&outgoing);
    f.inventory.0.lock().unwrap().other_clients = 2;
    let result = f.switch(&b).await.unwrap();
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
    assert_eq!(f.saved_bytes("ws-a"), outgoing);
    assert_eq!(f.engine.selected_id.as_deref(), Some(b.as_str()));
    assert!(result.daemon_restarted);
    assert_eq!(result.error, None);
    assert_eq!(result.other_clients, 2);
    assert!(f.engine.switching.is_none());
    assert_eq!(*f.daemon.calls.lock().unwrap(), vec!["state", "restart"]);
    // Other files of the Codex home are untouched.
    assert_eq!(
        fs::read(f.home.join("config.toml")).unwrap(),
        b"cli_auth_credentials_store = \"file\"\nmodel = \"fixture-model\"\n"
    );
    let snapshot =
        serde_json::to_string(&f.engine.snapshot(false, 300, &CodexPolicy::default())).unwrap();
    assert!(!snapshot.contains("secret-sentinel"));
    f.claude_untouched();
}

#[tokio::test]
async fn switch_without_daemon_only_writes_the_file_and_failed_restart_is_reported() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    *f.daemon.state.lock().unwrap() = Ok(DaemonState::NotRunning);
    let result = f.switch(&b).await.unwrap();
    assert!(!result.daemon_restarted);
    assert_eq!(result.error, None);
    assert_eq!(*f.daemon.calls.lock().unwrap(), vec!["state"]);
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));

    // Running daemon, restart fails: the sign-in stays switched, the result says so.
    let a = f.id_of("ws-a");
    *f.daemon.state.lock().unwrap() = Ok(DaemonState::Running);
    f.daemon
        .restarts
        .lock()
        .unwrap()
        .push_back(Err(CodexError::DaemonUnavailable));
    let result = f.switch(&a).await.unwrap();
    assert!(!result.daemon_restarted);
    assert_eq!(result.error, Some(CodexReason::DaemonRestartFailed));
    assert_eq!(f.engine.selected_id.as_deref(), Some(a.as_str()));
    assert_eq!(tokens_of(&f.active()).1, "ws-a");
    assert!(f.engine.switching.is_none());
}

#[tokio::test]
async fn hybrid_left_by_an_old_process_during_drain_is_repaired_and_restarted_once_more() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    // While the daemon drains, a `codex exec` still holding A refreshes A and merges its
    // tokens into the file that now belongs to B.
    *f.daemon.during_restart.lock().unwrap() = Some(hybrid("user-a", "ws-a", "ws-b", "late"));
    let result = f.switch(&b).await.unwrap();
    // The second restart probes the daemon first, like the first one.
    assert_eq!(
        *f.daemon.calls.lock().unwrap(),
        vec!["state", "restart", "state", "restart"]
    );
    assert!(result.daemon_restarted);
    // B is back in auth.json; A's late rotation is kept in A's own record.
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
    let (refresh, workspace) = tokens_of(&f.saved_bytes("ws-a"));
    assert_eq!(refresh, "refresh-secret-sentinel-late");
    assert_eq!(workspace, "ws-a");
    assert_eq!(f.engine.selected_id.as_deref(), Some(b.as_str()));
    f.claude_untouched();
}

#[tokio::test]
async fn hybrid_seen_later_by_the_background_observer_is_repaired_too() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    *f.daemon.state.lock().unwrap() = Ok(DaemonState::NotRunning);
    f.switch(&b).await.unwrap();
    f.write_active(&hybrid("user-a", "ws-a", "ws-b", "much-later"));
    f.engine.observe(&f.vault, 400).unwrap();
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
    assert_eq!(
        tokens_of(&f.saved_bytes("ws-a")).0,
        "refresh-secret-sentinel-much-later"
    );
    // A hybrid of an account that was never saved becomes a new account.
    f.write_active(&hybrid("user-z", "ws-z", "ws-b", "unknown"));
    f.engine.observe(&f.vault, 410).unwrap();
    assert_eq!(tokens_of(&f.saved_bytes("ws-z")).1, "ws-z");
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
}

#[tokio::test]
async fn unsupported_current_logins_and_targets_are_never_overwritten_or_selected() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    let agent = br#"{"agent_identity":"agent-secret-sentinel"}"#;
    f.write_active(agent);
    assert_eq!(
        f.engine.begin_switch(&b, &f.vault, 200).err(),
        Some(CodexReason::UnsupportedAuth)
    );
    assert_eq!(f.active(), agent);
    assert!(f.engine.switching.is_none());
    f.write_active(&auth("user-a", "ws-a", "initial"));
    f.engine.observe(&f.vault, 210).unwrap();
    // An imported API key is kept but cannot be selected.
    let index = f
        .engine
        .adopt_or_import(
            OpaqueAuth::parse(api_key_auth_payload("sk-secret-sentinel").unwrap()).unwrap(),
            CodexIdentityEvidence::ClaimsOnly,
            210,
        )
        .unwrap();
    let api = f.engine.saved.accounts[index].id.clone();
    assert_eq!(
        f.view(&api).switchable.blocked_reason,
        Some(CodexReason::UnsupportedAuth)
    );
    assert_eq!(
        f.engine.begin_switch(&api, &f.vault, 220).err(),
        Some(CodexReason::UnsupportedAuth)
    );
    // Switching to the active account is a no-op.
    let a = f.id_of("ws-a");
    assert!(f.engine.begin_switch(&a, &f.vault, 230).unwrap().is_none());
    assert!(f.daemon.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn inactive_quota_is_read_in_an_owned_home_and_its_rotation_is_adopted() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    let active = f.active();
    let mut plan = Plan::quota(Ok(quota(Some("ws-b"), 100, 61)));
    plan.rotate_quota = Some(auth("user-b", "ws-b", "rotated-in-owned-home"));
    f.factory.enqueue(plan);
    let job = f.engine.plan_quota(&b, 280, false).unwrap();
    let report = job.run().await;
    f.engine.finish_quota(report, &f.vault, 290).unwrap();
    assert_eq!(f.factory.calls(), vec!["start-Owned", "quota", "shutdown"]);
    assert_eq!(
        f.saved_bytes("ws-b"),
        auth("user-b", "ws-b", "rotated-in-owned-home")
    );
    assert_eq!(f.active(), active);
    let view = f.view(&b);
    assert!(view.identity_verified);
    assert_eq!(view.quota_state, CodexQuotaState::Fresh);
    assert_eq!(view.plan_type.as_deref(), Some("pro"));
    let limits = &view.quota.as_ref().unwrap().limits;
    // The repeated default bucket is not shown twice; extra limits are kept.
    assert_eq!(
        limits.iter().map(|l| l.key.as_str()).collect::<Vec<_>>(),
        vec!["default", "bucket:code-review"]
    );
    assert_eq!(limits[0].secondary.as_ref().unwrap().used_percent, 61);
    f.claude_untouched();
}

#[tokio::test]
async fn active_quota_uses_the_real_home_and_leaves_an_expiring_token_to_codex() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let expiring = format!(
        "{header}.{}.sig",
        URL_SAFE_NO_PAD.encode(br#"{"exp":1000}"#)
    );
    f.write_active(&auth_with("user-a", "ws-a", "expiring", expiring, 100));
    f.engine.observe(&f.vault, 700).unwrap();
    assert_eq!(
        f.engine.plan_quota(&a, 700, false).err(),
        Some(CodexReason::Busy)
    );
    f.factory
        .enqueue(Plan::quota(Ok(quota(Some("ws-a"), 42, 35))));
    let job = f.engine.plan_quota(&a, 700, true).unwrap();
    let report = job.run().await;
    f.engine.finish_quota(report, &f.vault, 710).unwrap();
    assert_eq!(f.factory.calls(), vec!["start-Active", "quota", "shutdown"]);
    assert!(f.view(&a).identity_verified);
}

#[tokio::test]
async fn rejected_sign_in_and_foreign_workspace_readings_are_reported_per_account() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    // Two consecutive rejections (one could be a transient refresh failure).
    for at in [500, 2000] {
        f.factory
            .enqueue(Plan::quota(Err(CodexError::AuthRequired)));
        let report = f.engine.plan_quota(&b, at, true).unwrap().run().await;
        f.engine.finish_quota(report, &f.vault, at + 10).unwrap();
    }
    let view = f.view(&b);
    assert!(view.needs_sign_in);
    assert_eq!(view.error, Some(CodexReason::SignInRequired));
    assert_eq!(
        view.switchable.blocked_reason,
        Some(CodexReason::SignInRequired)
    );
    assert_eq!(
        f.engine.begin_switch(&b, &f.vault, 520).err(),
        Some(CodexReason::SignInRequired)
    );
    // A reading that belongs to another workspace is never shown for this account.
    f.factory
        .enqueue(Plan::quota(Ok(quota(Some("ws-other"), 10, 10))));
    let report = f.engine.plan_quota(&c, 530, false).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 540).unwrap();
    let view = f.view(&c);
    assert_eq!(view.error, Some(CodexReason::IdentityMismatch));
    assert!(view.quota.is_none());
    // A fresh sign-in of B (Codex rotated it) clears the sign-in requirement.
    f.engine
        .adopt_or_import(
            OpaqueAuth::parse(auth("user-b", "ws-b", "signed-in-again")).unwrap(),
            CodexIdentityEvidence::ManagedLogin,
            550,
        )
        .unwrap();
    assert!(!f.view(&b).needs_sign_in);
}

#[test]
fn background_readings_prefer_the_active_account_then_the_oldest_inactive_one() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    let policy = CodexPolicy::default();
    assert_eq!(
        f.engine.due_quota(1000, &policy).as_deref(),
        Some(a.as_str())
    );
    let set = |f: &mut Fixture, id: &str, read: i64| {
        let index = f.engine.account_index(id).unwrap();
        let account = &mut f.engine.saved.accounts[index];
        account.quota_read_at = Some(read);
        account.last_attempt_at = Some(read);
        account.quota = Some(CodexQuotaView {
            ordinary_usage_allowed: Some(true),
            limits: vec![],
            reset_credits_available: None,
        });
    };
    set(&mut f, &a, 1000);
    set(&mut f, &b, 900);
    assert_eq!(
        f.engine.due_quota(1000, &policy).as_deref(),
        Some(c.as_str())
    );
    set(&mut f, &c, 1000);
    assert_eq!(f.engine.due_quota(1100, &policy), None);
    assert_eq!(
        f.engine
            .due_quota(1000 + policy.poll_interval, &policy)
            .as_deref(),
        Some(a.as_str())
    );
    // The active account is read at Claude's check interval; once it is fresh, the
    // oldest inactive reading is next.
    set(&mut f, &a, 900 + INACTIVE_QUOTA_INTERVAL);
    assert_eq!(
        f.engine
            .due_quota(900 + INACTIVE_QUOTA_INTERVAL, &policy)
            .as_deref(),
        Some(b.as_str())
    );
    let index = f.engine.account_index(&b).unwrap();
    f.engine.saved.accounts[index].needs_sign_in = true;
    assert_ne!(
        f.engine
            .due_quota(900 + INACTIVE_QUOTA_INTERVAL, &policy)
            .as_deref(),
        Some(b.as_str())
    );
}

#[tokio::test]
async fn codex_switcher_accounts_are_imported_with_their_names_and_newer_tokens_win() {
    let mut f = Fixture::new();
    // B is saved here with an older id_token than Codex Switcher's copy.
    f.add("user-b", "ws-b", "older-here");
    let token = |user: &str, ws: &str, iat: i64| {
        jwt(json!({"iat":iat,"email":format!("{user}@example.invalid"),
            "https://api.openai.com/auth":{"chatgpt_user_id":user,"chatgpt_account_id":ws}}))
    };
    let store = json!({"version":1,"active_account_id":"x","masked_account_ids":[],"accounts":[
        {"id":"s1","name":"Work Pro","auth_mode":"chat_g_p_t","created_at":"2026-01-01T00:00:00Z",
         "auth_data":{"type":"chat_g_p_t","id_token":token("user-b","ws-b",900),
            "access_token":"access-secret-sentinel-switcher","refresh_token":"refresh-secret-sentinel-switcher-b","account_id":"ws-b"}},
        {"id":"s2","name":"Side Plus","auth_mode":"chat_g_p_t","created_at":"2026-01-01T00:00:00Z",
         "auth_data":{"type":"chat_g_p_t","id_token":token("user-c","ws-c",500),
            "access_token":"access-secret-sentinel-c","refresh_token":"refresh-secret-sentinel-c","account_id":"ws-c"}},
        {"id":"s3","name":"Key","auth_mode":"api_key","created_at":"2026-01-01T00:00:00Z",
         "auth_data":{"type":"api_key","key":"sk-secret-sentinel"}},
        {"id":"s4","name":"Broken","auth_mode":"chat_g_p_t","auth_data":{"type":"chat_g_p_t","id_token":"x"}}
    ]});
    *f.switcher.0.lock().unwrap() = Some(serde_json::to_vec(&store).unwrap());
    f.engine.observe(&f.vault, 300).unwrap();
    assert!(
        f.engine
            .snapshot(false, 300, &CodexPolicy::default())
            .capabilities
            .import_switcher
            .enabled
    );
    f.engine
        .command(CodexCommand::ImportSwitcher, &f.vault, 300)
        .await
        .unwrap();
    // a (active), b (updated in place), c (new, named), API key (new, not switchable).
    assert_eq!(f.engine.saved.accounts.len(), 4);
    assert_eq!(
        tokens_of(&f.saved_bytes("ws-b")).0,
        "refresh-secret-sentinel-switcher-b"
    );
    let c = f.id_of("ws-c");
    assert_eq!(f.view(&c).name, "Side Plus");
    assert!(f.view(&c).switchable.enabled);
    // The active account's file is never touched by an import.
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
    // A file in an unknown format is reported, not half-imported.
    *f.switcher.0.lock().unwrap() = Some(b"{\"accounts\":\"nope\"}".to_vec());
    assert_eq!(
        f.engine
            .command(CodexCommand::ImportSwitcher, &f.vault, 310)
            .await
            .err(),
        Some(CodexReason::SwitcherUnavailable)
    );
    f.claude_untouched();
}

#[tokio::test]
async fn the_active_account_cannot_be_deleted_others_can() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    assert_eq!(
        f.engine
            .command(CodexCommand::Delete(a.clone()), &f.vault, 300)
            .await
            .err(),
        Some(CodexReason::ActiveAccount)
    );
    f.engine
        .command(CodexCommand::Delete(b), &f.vault, 310)
        .await
        .unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 1);
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
}

#[tokio::test]
async fn browser_sign_in_saves_a_managed_account_without_touching_the_active_login() {
    let mut f = Fixture::new();
    let active = f.active();
    let mut plan = Plan::quota(Err(CodexError::ServiceUnavailable));
    plan.login_auth = Some(auth("user-b", "ws-b", "login"));
    f.factory.enqueue(plan);
    let launch = f.engine.begin_login(0).await.unwrap();
    assert!(!format!("{launch:?}").contains("secret-sentinel"));
    let owned = f
        .engine
        .pending_login
        .as_ref()
        .unwrap()
        .home
        .path()
        .to_owned();
    f.engine
        .poll_login(&launch.session.id, &f.vault, 300)
        .await
        .unwrap();
    let b = f.id_of("ws-b");
    assert_eq!(f.engine.last_login_account.as_deref(), Some(b.as_str()));
    assert_eq!(
        f.view(&b).identity_evidence,
        CodexIdentityEvidence::ManagedLogin
    );
    assert!(f.view(&b).switchable.enabled);
    assert_eq!(f.active(), active);
    assert!(!owned.exists());
    f.claude_untouched();
}

#[tokio::test]
async fn sign_in_whose_owner_changes_before_shutdown_is_rejected() {
    let mut f = Fixture::new();
    let mut plan = Plan::quota(Err(CodexError::ServiceUnavailable));
    plan.login_auth = Some(auth("user-b", "ws-b", "login"));
    plan.rotate_shutdown = Some(auth("user-c", "ws-c", "replacement"));
    f.factory.enqueue(plan);
    let launch = f.engine.begin_login(0).await.unwrap();
    assert_eq!(
        f.engine
            .poll_login(&launch.session.id, &f.vault, 300)
            .await
            .err(),
        Some(CodexReason::IdentityMismatch)
    );
    assert_eq!(f.engine.saved.accounts.len(), 1);
    assert_eq!(
        f.engine.login_view.as_ref().unwrap().status,
        CodexLoginStatus::Failed
    );
}

#[tokio::test]
async fn canceled_sign_in_leaves_active_auth_and_claude_ciphertext_unchanged() {
    let mut f = Fixture::new();
    let active = f.active();
    let mut plan = Plan::quota(Err(CodexError::ServiceUnavailable));
    plan.login = LoginPoll::Pending;
    f.factory.enqueue(plan);
    let launch = f.engine.begin_login(0).await.unwrap();
    let owned = f
        .engine
        .pending_login
        .as_ref()
        .unwrap()
        .home
        .path()
        .to_owned();
    f.engine.cancel_login(&launch.session.id).await.unwrap();
    assert!(f.engine.pending_login.is_none());
    assert!(!owned.exists());
    assert_eq!(
        f.engine.login_view.as_ref().unwrap().error,
        Some(CodexReason::LoginCanceled)
    );
    assert_eq!(f.active(), active);
    f.claude_untouched();
}

#[test]
fn snapshot_reports_environment_warnings_and_capabilities() {
    let mut f = Fixture::new();
    *f.inventory.0.lock().unwrap() = CodexProcessSummary {
        other_clients: 1,
        switcher_running: true,
        daemon_running: true,
        daemon_pid: Some(7),
    };
    f.engine.observe(&f.vault, 300).unwrap();
    let snapshot = f.engine.snapshot(false, 300, &CodexPolicy::default());
    assert_eq!(snapshot.environment.daemon_running, Some(true));
    assert!(snapshot.environment.codex_switcher_running);
    assert_eq!(snapshot.environment.other_clients, 1);
    assert_eq!(snapshot.executable_version.as_deref(), Some("0.160.0"));
    assert!(snapshot.capabilities.switch_account.enabled);
    assert!(!snapshot.capabilities.import_switcher.enabled);
    let a = f.id_of("ws-a");
    let view = f.view(&a);
    assert!(view.selected);
    assert!(!view.switchable.enabled);
    assert_eq!(view.switchable.blocked_reason, None);
}

#[tokio::test]
async fn demo_has_three_read_only_accounts_without_an_installation_or_client() {
    let mut engine = CodexEngine::demo(1000);
    let factory = Arc::new(FakeFactory::default());
    engine.factory = factory.clone();
    assert!(engine.installation.is_none());
    assert!(!engine.writable);
    let snapshot = engine.snapshot(false, 1000, &CodexPolicy::default());
    assert_eq!(snapshot.next_id, None);
    assert!(snapshot.demo);
    assert_eq!(snapshot.accounts.len(), 3);
    assert_eq!(snapshot.selected_id.as_deref(), Some("codex-demo-0"));
    assert!(snapshot.accounts.iter().all(|a| !a.switchable.enabled));
    for capability in [
        &snapshot.capabilities.login_browser,
        &snapshot.capabilities.import_current,
        &snapshot.capabilities.refresh_quota,
        &snapshot.capabilities.switch_account,
        &snapshot.capabilities.delete_saved,
    ] {
        assert!(!capability.enabled);
    }
    let parent = std::env::temp_dir().canonicalize().unwrap();
    let root = tempfile::Builder::new()
        .prefix("primerswitch-demo-fixture-")
        .tempdir_in(parent)
        .unwrap();
    let vault = Vault::with_key(root.path().join("vault"), [64; 32]).unwrap();
    assert_eq!(
        engine
            .command(CodexCommand::BeginLogin(0), &vault, 1000)
            .await
            .err(),
        Some(CodexReason::VaultUnavailable)
    );
    assert!(engine.begin_switch("codex-demo-1", &vault, 1000).is_err());
    assert!(factory.calls().is_empty());
    assert!(vault.load::<Saved>(RECORD).unwrap().is_none());
}

#[test]
fn a_home_that_is_a_git_repo_holding_codex_home_qualifies_but_project_reroutes_do_not() {
    let parent = std::env::temp_dir().canonicalize().unwrap();
    let root = tempfile::Builder::new()
        .prefix("primerswitch-gitHome-")
        .tempdir_in(parent)
        .unwrap();
    let user = root.path().join("user");
    fs::create_dir_all(user.join(".git")).unwrap();
    let home = create_private_context(user.join(".codex")).unwrap();
    fs::write(home.join("config.toml"), b"model = \"fixture-model\"\n").unwrap();
    let workspace = create_private_context(user.join("Temp").join("owned")).unwrap();
    let executable = root.path().join("fixture-codex-native");
    fs::write(&executable, b"non-executable-fixture").unwrap();
    let system = root.path().join("system");
    fs::create_dir(&system).unwrap();
    let paths = ContextPaths {
        home: home.clone(),
        system_config: system.join("config.toml"),
        system_requirements: system.join("requirements.toml"),
        legacy_requirements: system.join("managed_config.toml"),
        enforce_native_policy: false,
        project_ancestor_stop: Some(root.path().to_owned()),
    };
    let qualified = paths.qualify(&executable, &workspace, "0.160.0").unwrap();
    assert!(qualified.context.is_some());
    assert_eq!(qualified.active_model.as_deref(), Some("fixture-model"));
    // A project layer above the owned cwd that reroutes sign-in is still refused.
    let project = user.join("Temp").join(".codex");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("config.toml"), b"model_provider = \"azure\"\n").unwrap();
    assert_eq!(
        paths.qualify(&executable, &workspace, "0.160.0").err(),
        Some(CodexReason::UnsupportedAuth)
    );
}

fn access_for(user: &str, workspace: &str, serial: &str) -> String {
    jwt(
        json!({"exp":4_000_000_000i64,"serial":serial,"https://api.openai.com/auth":{
        "chatgpt_account_id":workspace,"chatgpt_user_id":user}}),
    )
}
/// A login whose access token carries its own identity claims, as Codex issues them.
fn auth_jwt(user: &str, workspace: &str, revision: &str) -> Vec<u8> {
    auth_with(
        user,
        workspace,
        revision,
        access_for(user, workspace, revision),
        100,
    )
}

#[tokio::test]
async fn refresh_without_id_token_during_the_drain_never_overwrites_the_new_account() {
    let mut f = Fixture::new();
    f.write_active(&auth_jwt("user-a", "ws-a", "initial"));
    f.engine.observe(&f.vault, 110).unwrap();
    let b = f.add("user-b", "ws-b", "saved");
    let saved_b = auth("user-b", "ws-b", "saved");
    // The old process holding A refreshes; the response has no id_token, so Codex keeps
    // B's id_token: B's id_token and label with A's access and refresh tokens.
    let mut mixed: serde_json::Value = serde_json::from_slice(&saved_b).unwrap();
    mixed["tokens"]["access_token"] = access_for("user-a", "ws-a", "late").into();
    mixed["tokens"]["refresh_token"] = "refresh-secret-sentinel-a-late".into();
    *f.daemon.during_restart.lock().unwrap() = Some(serde_json::to_vec(&mixed).unwrap());
    f.switch(&b).await.unwrap();
    // B is restored exactly; A received its rotated tokens and kept its own id_token.
    assert_eq!(f.active(), saved_b);
    assert_eq!(
        tokens_of(&f.saved_bytes("ws-b")).0,
        "refresh-secret-sentinel-saved"
    );
    let a = f.saved_bytes("ws-a");
    assert_eq!(tokens_of(&a).0, "refresh-secret-sentinel-a-late");
    let a = OpaqueAuth::parse(a).unwrap();
    assert_eq!(
        a.routing_claims().chatgpt_user_id.as_deref(),
        Some("user-a")
    );
    assert_eq!(
        *f.daemon.calls.lock().unwrap(),
        vec!["state", "restart", "state", "restart"]
    );
}

#[tokio::test]
async fn the_restart_keeps_the_running_daemons_environment() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    f.inventory.0.lock().unwrap().daemon_pid = Some(4242);
    f.switch(&b).await.unwrap();
    let environment = f.daemon.environment.lock().unwrap().clone().unwrap();
    assert_eq!(environment[0].0, "PATH");
}

#[test]
fn a_rotation_that_changes_the_email_keeps_the_store_loadable() {
    let f = Fixture::new();
    let mut renamed: serde_json::Value =
        serde_json::from_slice(&auth("user-a", "ws-a", "renamed")).unwrap();
    let claims = json!({"iat":200,"email":"new-address@example.invalid",
        "https://api.openai.com/auth":{"chatgpt_user_id":"user-a","chatgpt_account_id":"ws-a"}});
    renamed["tokens"]["id_token"] = jwt(claims).into();
    let mut f = f;
    f.write_active(&serde_json::to_vec(&renamed).unwrap());
    f.engine.observe(&f.vault, 200).unwrap();
    // Reloading the encrypted store must keep every account.
    let reloaded = CodexEngine::load(&f.vault);
    assert!(reloaded.writable);
    assert_eq!(reloaded.saved.accounts.len(), 1);
    assert_eq!(
        reloaded.saved.accounts[0].binding.email.as_deref(),
        Some("new-address@example.invalid")
    );
}

#[test]
fn a_damaged_record_is_quarantined_instead_of_refusing_every_account() {
    let mut f = Fixture::new();
    f.add("user-b", "ws-b", "saved");
    let mut saved: Saved = f.vault.load(RECORD).unwrap().unwrap();
    saved.accounts[1].auth = b"{\"broken\":true}".to_vec();
    f.vault.save(RECORD, &saved).unwrap();
    let reloaded = CodexEngine::load(&f.vault);
    assert!(reloaded.writable);
    assert_eq!(reloaded.saved.accounts.len(), 1);
    let quarantine: Saved = f.vault.load(QUARANTINE).unwrap().unwrap();
    assert_eq!(quarantine.accounts.len(), 1);
}

#[tokio::test]
async fn one_rejected_refresh_is_not_enough_to_ask_for_a_new_sign_in() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    for (attempt, at) in [(1, 500), (2, 2000)] {
        f.factory
            .enqueue(Plan::quota(Err(CodexError::AuthRequired)));
        let report = f.engine.plan_quota(&b, at, true).unwrap().run().await;
        f.engine.finish_quota(report, &f.vault, at + 5).unwrap();
        let view = f.view(&b);
        assert_eq!(view.needs_sign_in, attempt == 2, "attempt {attempt}");
    }
    // A successful reading resets the count.
    f.factory
        .enqueue(Plan::quota(Ok(quota(Some("ws-b"), 10, 10))));
    let report = f.engine.plan_quota(&b, 2100, true).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 2105).unwrap();
    assert!(!f.view(&b).needs_sign_in);
    assert_eq!(
        f.engine.saved.accounts[f.engine.account_index(&b).unwrap()].auth_failures,
        0
    );
}

#[tokio::test]
async fn an_expiring_active_token_does_not_starve_the_other_accounts() {
    let mut f = Fixture::new();
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let expiring = format!(
        "{header}.{}.sig",
        URL_SAFE_NO_PAD.encode(br#"{"exp":1000}"#)
    );
    f.write_active(&auth_with("user-a", "ws-a", "expiring", expiring, 100));
    f.engine.observe(&f.vault, 700).unwrap();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let policy = CodexPolicy::default();
    assert_eq!(
        f.engine.due_quota(700, &policy).as_deref(),
        Some(a.as_str())
    );
    assert_eq!(
        f.engine.plan_quota(&a, 700, false).err(),
        Some(CodexReason::Busy)
    );
    assert_eq!(
        f.engine.due_quota(701, &policy).as_deref(),
        Some(b.as_str())
    );
}

#[tokio::test]
async fn a_reading_without_an_account_id_is_shown_but_not_trusted_as_proof() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    f.factory.enqueue(Plan::quota(Ok(quota(None, 30, 20))));
    let report = f.engine.plan_quota(&b, 280, false).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 290).unwrap();
    let view = f.view(&b);
    assert!(view.quota.is_some());
    assert!(!view.identity_verified);
}

#[tokio::test]
async fn an_unusable_codex_switcher_entry_does_not_stop_the_import() {
    let mut f = Fixture::new();
    let token = |user: &str, ws: &str, fedramp: bool| {
        jwt(json!({"iat":500,"email":format!("{user}@example.invalid"),
            "https://api.openai.com/auth":{"chatgpt_user_id":user,"chatgpt_account_id":ws,
            "chatgpt_account_is_fedramp":fedramp}}))
    };
    let entry = |name: &str, user: &str, ws: &str, fedramp: bool| {
        json!({"name":name,"auth_mode":"chat_g_p_t","auth_data":{"type":"chat_g_p_t",
            "id_token":token(user, ws, fedramp),"access_token":"access-secret-sentinel-x",
            "refresh_token":format!("refresh-secret-sentinel-{user}"),"account_id":ws}})
    };
    let store = json!({"version":1,"accounts":[
        entry("Gov","user-g","ws-g",true), entry("After","user-c","ws-c",false)]});
    *f.switcher.0.lock().unwrap() = Some(serde_json::to_vec(&store).unwrap());
    f.engine.observe(&f.vault, 300).unwrap();
    f.engine
        .command(CodexCommand::ImportSwitcher, &f.vault, 300)
        .await
        .unwrap();
    assert_eq!(f.view(&f.id_of("ws-c")).name, "After");
}

#[test]
fn owned_homes_never_sync_the_plugin_marketplace() {
    let home = owned_home().unwrap();
    let config = fs::read_to_string(home.path().join("config.toml")).unwrap();
    assert!(config.contains("plugins = false"));
    let path = home.path().to_owned();
    let nested = path.join(".tmp").join("plugins").join(".git");
    fs::create_dir_all(&nested).unwrap();
    let pack = nested.join("pack.idx");
    fs::write(&pack, b"x").unwrap();
    let mut permissions = fs::metadata(&pack).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&pack, permissions).unwrap();
    fs::write(path.join("auth.json"), b"secret-sentinel").unwrap();
    scrub_owned_home(&path);
    drop(home);
    assert!(!path.exists());
}

#[tokio::test]
async fn an_elevated_app_never_half_switches_while_the_daemon_runs() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    let before = f.active();
    // other_clients == 99 marks the fake as elevated.
    *f.inventory.0.lock().unwrap() = CodexProcessSummary {
        other_clients: 99,
        switcher_running: false,
        daemon_running: true,
        daemon_pid: None,
    };
    assert_eq!(
        f.engine.begin_switch(&b, &f.vault, 200).err(),
        Some(CodexReason::DaemonRestartFailed)
    );
    assert_eq!(f.active(), before);
    assert!(f.engine.switching.is_none());
    // Without a running daemon there is nothing to restart: the switch proceeds.
    f.inventory.0.lock().unwrap().daemon_running = false;
    *f.daemon.state.lock().unwrap() = Ok(DaemonState::NotRunning);
    f.switch(&b).await.unwrap();
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
}

// Automation shared with Claude: ranking, cadence, automatic switching, credits.
const NOW: i64 = 1_000_000;
/// The main limit with a 5-hour window resetting at `five_reset` and a weekly window
/// resetting at `week_reset`.
fn main_quota_at(five: i32, five_reset: i64, week: i32, week_reset: i64) -> CodexQuotaView {
    let window = |used: i32, minutes: i64, reset: i64| CodexWindowView {
        used_percent: used,
        window_duration_mins: Some(minutes),
        resets_at: Some(reset),
    };
    CodexQuotaView {
        ordinary_usage_allowed: Some(true),
        limits: vec![CodexLimitView {
            key: "default".into(),
            limit_id: Some("codex".into()),
            limit_name: None,
            normal_model_slug: None,
            primary: Some(window(five, 300, five_reset)),
            secondary: Some(window(week, 10080, week_reset)),
            plan_type: Some("plus".into()),
            credits: None,
            spend_control_reached: Some(false),
            rate_limit_reached_type: None,
        }],
        reset_credits_available: None,
    }
}
fn main_quota(five: i32, week: i32) -> CodexQuotaView {
    main_quota_at(five, NOW + 3600, week, NOW + 5 * 86400)
}
/// Codex can report its only weekly window as primary or secondary.
fn weekly_only_quota(week: i32, reset: i64, primary: bool) -> CodexQuotaView {
    let mut quota = main_quota_at(0, NOW + 3600, week, reset);
    let limit = &mut quota.limits[0];
    limit.primary = if primary {
        limit.secondary.take()
    } else {
        None
    };
    quota
}
/// A successful reading of `id` at `at` that named its workspace.
fn record_reading(engine: &mut CodexEngine, id: &str, quota: CodexQuotaView, at: i64) {
    let index = engine.account_index(id).unwrap();
    let account = &mut engine.saved.accounts[index];
    account.quota = Some(quota);
    account.quota_read_at = Some(at);
    account.last_attempt_at = Some(at);
    engine.verified_now.insert(id.into());
    engine.account_errors.remove(id);
}
impl Fixture {
    fn read(&mut self, id: &str, quota: CodexQuotaView, at: i64) {
        record_reading(&mut self.engine, id, quota, at);
    }
    /// The next account; every check also proves the usage order starts with it.
    fn next(&self, policy: &CodexPolicy) -> Option<String> {
        let snapshot = self.engine.snapshot(false, NOW, policy);
        assert_eq!(snapshot.order.first(), snapshot.next_id.as_ref());
        snapshot.next_id
    }
    fn order(&self, policy: &CodexPolicy) -> Vec<String> {
        let snapshot = self.engine.snapshot(false, NOW, policy);
        assert_eq!(snapshot.order.first(), snapshot.next_id.as_ref());
        snapshot.order
    }
    fn quota_mut(&mut self, id: &str) -> &mut CodexQuotaView {
        let index = self.engine.account_index(id).unwrap();
        self.engine.saved.accounts[index].quota.as_mut().unwrap()
    }
}
fn switch_to(f: &Fixture, id: &str, peak: i32) -> CodexAutomation {
    let name = |id: &str| {
        let index = f.engine.account_index(id).unwrap();
        f.engine.saved.accounts[index].name.clone()
    };
    let active = f.engine.selected_id.clone().unwrap();
    CodexAutomation::Switch(CodexAutoSwitch {
        target_id: id.into(),
        target_name: name(id),
        previous_name: name(&active),
        peak: Some(peak),
    })
}

#[test]
fn the_next_codex_account_is_ranked_in_rust_with_the_order_shared_with_claude() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    let soonest = CodexPolicy::default();
    let most_left = CodexPolicy {
        order: CandidateOrder::MostWeeklyLeft,
        ..soonest
    };
    // Accounts without a reading are never next.
    assert_eq!(f.next(&soonest), None);
    f.read(&a, main_quota(40, 30), NOW);
    // b's week resets within the hour with 70% used; c resets in six days, 20% used.
    f.read(&b, main_quota_at(10, NOW + 3600, 70, NOW + 3600), NOW);
    f.read(&c, main_quota_at(30, NOW + 3600, 20, NOW + 6 * 86400), NOW);
    assert_eq!(f.next(&soonest), Some(b.clone()));
    assert_eq!(f.next(&most_left), Some(c.clone()));
    // The active account is never next, even when it would rank first.
    f.read(&a, main_quota_at(1, NOW + 60, 1, NOW + 60), NOW);
    assert_eq!(f.next(&soonest), Some(b.clone()));
    // Equal weekly usage: the earlier weekly reset, then the lower 5-hour usage.
    f.read(&b, main_quota_at(10, NOW + 3600, 20, NOW + 2 * 86400), NOW);
    assert_eq!(f.next(&most_left), Some(b.clone()));
    f.read(
        &b,
        main_quota_at(40, NOW + 3600, 20, NOW + 6 * 86400 + 60),
        NOW,
    );
    assert_eq!(f.next(&most_left), Some(c.clone()));
    // Preferred headroom (both windows at most 90% at a 95% threshold) comes first...
    f.read(&b, main_quota_at(91, NOW + 3600, 10, NOW + 3600), NOW);
    assert_eq!(f.next(&soonest), Some(c.clone()));
    // ...but an account without it is still the usable fallback.
    f.read(&c, main_quota(100, 20), NOW);
    assert_eq!(f.next(&soonest), Some(b.clone()));
    assert_eq!(
        f.engine.snapshot(false, NOW, &soonest).next_id,
        Some(b.clone())
    );
    // A lower threshold rules b out.
    assert_eq!(
        f.next(&CodexPolicy {
            threshold: 90.0,
            ..soonest
        }),
        None
    );
}

#[test]
fn weekly_only_codex_accounts_are_ranked_by_their_reported_windows() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    let d = f.add("user-d", "ws-d", "saved");
    let policy = CodexPolicy {
        threshold: 94.0,
        ..CodexPolicy::default()
    };
    // The reported failure: active 13%/95%, with three weekly-only alternatives.
    f.read(&a, main_quota(13, 95), NOW);
    f.read(
        &b,
        weekly_only_quota(19, NOW + 4 * 86400 + 23 * 3600, true),
        NOW,
    );
    f.read(
        &c,
        weekly_only_quota(10, NOW + 22 * 86400 + 22 * 3600, false),
        NOW,
    );
    f.read(
        &d,
        weekly_only_quota(2, NOW + 24 * 86400 + 12 * 3600, true),
        NOW,
    );
    assert_eq!(f.order(&policy), vec![b.clone(), c.clone(), d.clone()]);
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 95));
    let most_left = CodexPolicy {
        order: CandidateOrder::MostWeeklyLeft,
        ..policy
    };
    assert_eq!(f.order(&most_left), vec![d.clone(), c.clone(), b.clone()]);
    assert_eq!(f.engine.automation(NOW, &most_left), switch_to(&f, &d, 95));
    // Missing windows stay absent in IPC and persistence; no zero usage is invented.
    let limit = f.view(&b).quota.unwrap().limits.remove(0);
    assert!(limit.secondary.is_none());
    assert_eq!(limit.primary.unwrap().window_duration_mins, Some(10080));
    f.engine.save(&f.vault).unwrap();
    let saved: Saved = f.vault.load(RECORD).unwrap().unwrap();
    let limit = &saved
        .accounts
        .iter()
        .find(|account| account.id == b)
        .unwrap()
        .quota
        .as_ref()
        .unwrap()
        .limits[0];
    assert!(limit.secondary.is_none());
    assert_eq!(limit.primary.as_ref().unwrap().used_percent, 19);
    // Old readings are still revalidated before automatic switching.
    f.read(&b, weekly_only_quota(19, NOW + 4 * 86400, true), NOW - 901);
    assert_eq!(
        f.engine.automation(NOW, &policy),
        CodexAutomation::Refresh(b)
    );
    f.claude_untouched();
}

#[test]
fn single_codex_windows_obey_thresholds_headroom_and_provider_limits() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    let policy = CodexPolicy::default();
    f.read(&a, main_quota(13, 95), NOW);
    for primary in [true, false] {
        f.read(&b, weekly_only_quota(19, NOW + 3600, primary), NOW);
        assert_eq!(f.next(&policy), Some(b.clone()));
        for used in [95, 100, 105] {
            f.read(&b, weekly_only_quota(used, NOW + 3600, primary), NOW);
            assert_eq!(f.next(&policy), None);
        }
        f.read(&b, weekly_only_quota(19, NOW + 3600, primary), NOW);
        f.quota_mut(&b).ordinary_usage_allowed = Some(false);
        assert_eq!(f.next(&policy), None);
        f.read(&b, weekly_only_quota(19, NOW + 3600, primary), NOW);
        f.quota_mut(&b).limits[0].rate_limit_reached_type = Some("primary".into());
        assert_eq!(f.next(&policy), None);
    }
    // Preferred headroom uses the only reported window as well.
    f.read(&b, weekly_only_quota(91, NOW + 3600, true), NOW);
    f.read(&c, weekly_only_quota(20, NOW + 6 * 86400, true), NOW);
    assert_eq!(f.order(&policy), vec![c.clone(), b.clone()]);
    // A short-only account is usable, but its unknown weekly usage ranks last.
    let mut short_only = main_quota(10, 10);
    short_only.limits[0].secondary = None;
    f.read(&b, short_only.clone(), NOW);
    for order in [
        CandidateOrder::SoonestWeeklyReset,
        CandidateOrder::MostWeeklyLeft,
    ] {
        assert_eq!(
            f.order(&CodexPolicy { order, ..policy }),
            vec![c.clone(), b.clone()]
        );
    }
    f.read(&c, weekly_only_quota(95, NOW + 86400, true), NOW);
    assert_eq!(f.next(&policy), Some(b.clone()));
    // No main windows proves neither availability nor that every account is limited.
    short_only.limits[0].primary = None;
    f.read(&b, short_only, NOW);
    assert_eq!(f.next(&policy), None);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
}

#[test]
fn limited_unread_failed_or_unswitchable_accounts_are_never_next() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    let policy = CodexPolicy::default();
    let usable = main_quota(10, 10);
    f.read(&b, usable.clone(), NOW);
    assert_eq!(f.next(&policy), Some(b.clone()));
    // At or above the threshold.
    f.read(&b, main_quota(95, 10), NOW);
    assert_eq!(f.next(&policy), None);
    // Over 100% on purchased credits counts as limited.
    let mut credits = main_quota(10, 105);
    credits.limits[0].credits = Some(CodexCreditsView {
        has_credits: true,
        unlimited: false,
        balance: Some("12.50".into()),
    });
    assert!(quota_limited(&credits));
    f.read(&b, credits, NOW);
    assert_eq!(f.next(&policy), None);
    // A reached-limit flag, or blocked included usage, with low percentages.
    f.read(&b, usable.clone(), NOW);
    f.quota_mut(&b).limits[0].rate_limit_reached_type = Some("primary".into());
    assert_eq!(f.next(&policy), None);
    f.read(&b, usable.clone(), NOW);
    f.quota_mut(&b).ordinary_usage_allowed = Some(false);
    assert_eq!(f.next(&policy), None);
    // With no main windows there is no evidence of free usage.
    f.read(&b, usable.clone(), NOW);
    f.quota_mut(&b).limits[0].primary = None;
    f.quota_mut(&b).limits[0].secondary = None;
    assert_eq!(f.next(&policy), None);
    // A failed latest reading, a rejected sign-in or a switch in progress.
    f.read(&b, usable.clone(), NOW);
    f.engine
        .account_errors
        .insert(b.clone(), CodexReason::ProviderUnavailable);
    assert_eq!(f.next(&policy), None);
    f.read(&b, usable.clone(), NOW);
    let index = f.engine.account_index(&b).unwrap();
    f.engine.saved.accounts[index].needs_sign_in = true;
    assert_eq!(f.next(&policy), None);
    f.engine.saved.accounts[index].needs_sign_in = false;
    f.engine.switching = Some(CodexSwitchView {
        target_id: b.clone(),
        stage: CodexSwitchStage::Restarting,
        started_at: NOW,
    });
    assert_eq!(f.next(&policy), None);
    f.engine.switching = None;
    assert_eq!(f.next(&policy), Some(b.clone()));
    // An API-key login has no plan limits and cannot be selected.
    let index = f
        .engine
        .adopt_or_import(
            OpaqueAuth::parse(api_key_auth_payload("sk-secret-sentinel").unwrap()).unwrap(),
            CodexIdentityEvidence::ClaimsOnly,
            NOW,
        )
        .unwrap();
    let api = f.engine.saved.accounts[index].id.clone();
    f.read(&api, main_quota(1, 1), NOW);
    assert_eq!(f.next(&policy), Some(b.clone()));
    assert_eq!(f.order(&policy), vec![b]);
}

#[test]
fn the_codex_usage_order_ranks_every_usable_account_with_the_next_one_first() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    let d = f.add("user-d", "ws-d", "saved");
    let e = f.add("user-e", "ws-e", "saved");
    let soonest = CodexPolicy::default();
    let most_left = CodexPolicy {
        order: CandidateOrder::MostWeeklyLeft,
        ..soonest
    };
    // Nothing has been read yet, so nothing is ranked.
    assert!(f.order(&soonest).is_empty());
    f.read(&a, main_quota_at(1, NOW + 60, 1, NOW + 60), NOW);
    // b's week resets within the hour (70% used), d's in two days (50%), c's in six
    // days (20%); e stays unread. The active account a is never part of the order.
    f.read(&b, main_quota_at(10, NOW + 3600, 70, NOW + 3600), NOW);
    f.read(&c, main_quota_at(30, NOW + 3600, 20, NOW + 6 * 86400), NOW);
    f.read(&d, main_quota_at(20, NOW + 3600, 50, NOW + 2 * 86400), NOW);
    assert_eq!(f.order(&soonest), vec![b.clone(), d.clone(), c.clone()]);
    assert_eq!(f.order(&most_left), vec![c.clone(), d.clone(), b.clone()]);
    // A limited account and one whose latest reading failed drop out.
    f.read(&e, main_quota(100, 10), NOW);
    f.engine
        .account_errors
        .insert(d.clone(), CodexReason::ProviderUnavailable);
    assert_eq!(f.order(&soonest), vec![b.clone(), c.clone()]);
    // Without preferred headroom an account stays usable, after those with it.
    f.read(&b, main_quota_at(91, NOW + 3600, 10, NOW + 3600), NOW);
    assert_eq!(f.order(&soonest), vec![c.clone(), b.clone()]);
    // A signed-out account is never ranked.
    let index = f.engine.account_index(&c).unwrap();
    f.engine.saved.accounts[index].needs_sign_in = true;
    assert_eq!(f.order(&soonest), vec![b.clone()]);
    // The window receives the ranking as `order`, next to `nextId`.
    let json = serde_json::to_value(f.engine.snapshot(false, NOW, &soonest)).unwrap();
    assert_eq!(json["order"], json!([b.clone()]));
    assert_eq!(json["nextId"], json!(b));
}

#[tokio::test]
async fn automatic_switching_needs_a_fresh_active_reading_at_the_threshold() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let policy = CodexPolicy::default();
    f.read(&b, main_quota(10, 10), NOW);
    f.read(&a, main_quota(94, 40), NOW);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.read(&a, main_quota(96, 40), NOW);
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 96));
    // Automatic switching off: nothing.
    let off = CodexPolicy {
        auto_switch: false,
        ..policy
    };
    assert_eq!(f.engine.automation(NOW, &off), CodexAutomation::Idle);
    // The shared threshold decides.
    f.read(&a, main_quota(80, 40), NOW);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    let lower = CodexPolicy {
        threshold: 80.0,
        ..policy
    };
    assert_eq!(f.engine.automation(NOW, &lower), switch_to(&f, &b, 80));
    // A reached-limit flag switches below the threshold.
    f.read(&a, main_quota(50, 40), NOW);
    f.quota_mut(&a).limits[0].rate_limit_reached_type = Some("primary".into());
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 50));
    // Usage continuing on credits (over 100%) is a limit too.
    f.read(&a, main_quota(103, 40), NOW);
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 103));
    // Older than 15 minutes, failed or not proven this session: no decision.
    f.read(&a, main_quota(97, 40), NOW - 901);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.read(&a, main_quota(97, 40), NOW - 900);
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 97));
    f.engine
        .account_errors
        .insert(a.clone(), CodexReason::ProviderUnavailable);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.read(&a, main_quota(97, 40), NOW);
    f.engine.verified_now.remove(&a);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.read(&a, main_quota(97, 40), NOW);
    // Never during a switch or a sign-in.
    f.engine.switching = Some(CodexSwitchView {
        target_id: b.clone(),
        stage: CodexSwitchStage::Restarting,
        started_at: NOW,
    });
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.engine.switching = None;
    let mut plan = Plan::quota(Err(CodexError::ServiceUnavailable));
    plan.login = LoginPoll::Pending;
    f.factory.enqueue(plan);
    let launch = f.engine.begin_login(0).await.unwrap();
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.engine.cancel_login(&launch.session.id).await.unwrap();
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 97));
    // Never on a read-only store.
    f.engine.writable = false;
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.claude_untouched();
}

#[test]
fn automatic_switching_rereads_an_old_target_first_and_cools_down() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let policy = CodexPolicy::default();
    f.read(&a, main_quota(97, 40), NOW);
    // The target was read 20 minutes ago: read it again before switching to it.
    f.read(&b, main_quota(10, 10), NOW - 1200);
    assert_eq!(
        f.engine.automation(NOW, &policy),
        CodexAutomation::Refresh(b.clone())
    );
    f.read(&b, main_quota(10, 10), NOW);
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 97));
    // At most one automatic switch per ten minutes.
    f.engine.begin_auto_switch(NOW);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    assert_eq!(
        f.engine.automation(NOW + AUTO_SWITCH_COOLDOWN - 1, &policy),
        CodexAutomation::Idle
    );
    assert_eq!(
        f.engine.automation(NOW + AUTO_SWITCH_COOLDOWN, &policy),
        switch_to(&f, &b, 97)
    );
    // Failure notices for the same target and reason repeat at most every 30 minutes.
    let reason = CodexReason::DaemonRestartFailed;
    assert!(f.engine.failure_notice_due(&b, reason, NOW));
    assert!(
        !f.engine
            .failure_notice_due(&b, reason, NOW + AUTO_NOTICE_INTERVAL - 1)
    );
    assert!(
        f.engine
            .failure_notice_due(&b, CodexReason::StoreConflict, NOW + 1)
    );
    assert!(
        f.engine
            .failure_notice_due(&b, reason, NOW + AUTO_NOTICE_INTERVAL + 1)
    );
}

#[test]
fn without_a_usable_account_nothing_switches_and_all_limited_is_reported_when_known() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    let policy = CodexPolicy::default();
    f.read(&a, main_quota_at(100, NOW + 7200, 40, NOW + 86400), NOW);
    f.read(
        &b,
        main_quota_at(100, NOW + 600, 60, NOW + 86400),
        NOW - 3000,
    );
    // c was never read: "every account is limited" is unproven, so nothing happens.
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.read(&c, main_quota_at(30, NOW + 600, 99, NOW + 7200), NOW - 3000);
    let b_name = f.view(&b).name;
    assert_eq!(
        f.engine.automation(NOW, &policy),
        CodexAutomation::Exhausted {
            frees_first: Some((b_name, NOW + 600)),
        }
    );
    assert_eq!(f.next(&policy), None);
    // A signed-out account might be free: no claim that every account is limited.
    let index = f.engine.account_index(&c).unwrap();
    f.engine.saved.accounts[index].needs_sign_in = true;
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    // A single saved account at its limit is "every account".
    let mut alone = Fixture::new();
    let only = alone.id_of("ws-a");
    alone.read(&only, main_quota_at(99, NOW + 7200, 40, NOW + 86400), NOW);
    let only_name = alone.view(&only).name;
    assert_eq!(
        alone.engine.automation(NOW, &policy),
        CodexAutomation::Exhausted {
            frees_first: Some((only_name, NOW + 7200)),
        }
    );
    // The notice repeats at most every 30 minutes.
    assert!(alone.engine.exhausted_notice_due(NOW));
    assert!(
        !alone
            .engine
            .exhausted_notice_due(NOW + AUTO_NOTICE_INTERVAL - 1)
    );
    assert!(
        alone
            .engine
            .exhausted_notice_due(NOW + AUTO_NOTICE_INTERVAL)
    );
}

#[test]
fn the_active_account_is_read_at_the_check_interval_and_every_minute_near_the_threshold() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let policy = CodexPolicy::default();
    assert_eq!(policy.poll_interval, 300);
    f.read(&a, main_quota(40, 30), NOW);
    assert_eq!(f.engine.active_interval(&policy), 300);
    assert_eq!(f.engine.due_quota(NOW + 299, &policy), None);
    assert_eq!(f.engine.due_quota(NOW + 300, &policy), Some(a.clone()));
    let slower = CodexPolicy {
        poll_interval: 600,
        ..policy
    };
    assert_eq!(f.engine.due_quota(NOW + 300, &slower), None);
    assert_eq!(f.engine.due_quota(NOW + 600, &slower), Some(a.clone()));
    // Within ten points of the threshold (85% at 95%) with automatic switching on.
    f.read(&a, main_quota(30, 85), NOW);
    assert_eq!(
        f.engine.active_interval(&policy),
        ACTIVE_NEAR_LIMIT_INTERVAL
    );
    assert_eq!(f.engine.due_quota(NOW + 59, &policy), None);
    assert_eq!(f.engine.due_quota(NOW + 60, &policy), Some(a.clone()));
    let off = CodexPolicy {
        auto_switch: false,
        ..policy
    };
    assert_eq!(f.engine.active_interval(&off), 300);
    f.read(&a, main_quota(84, 30), NOW);
    assert_eq!(f.engine.active_interval(&policy), 300);
    // Limited by Codex at low usage counts as near.
    f.read(&a, main_quota(5, 5), NOW);
    f.quota_mut(&a).ordinary_usage_allowed = Some(false);
    assert_eq!(
        f.engine.active_interval(&policy),
        ACTIVE_NEAR_LIMIT_INTERVAL
    );
    // A failed attempt is retried at the same cadence, never sooner.
    let index = f.engine.account_index(&a).unwrap();
    f.engine.saved.accounts[index].last_attempt_at = Some(NOW + 30);
    assert_eq!(f.engine.due_quota(NOW + 60, &policy), None);
    assert_eq!(f.engine.due_quota(NOW + 90, &policy), Some(a));
}

/// Claude is never contacted by Codex automation.
struct NoClaude;
#[async_trait]
impl Transport for NoClaude {
    async fn send(&self, _: HttpRequest) -> Result<HttpResponse, ClientError> {
        Err(ClientError::InvalidResponse)
    }
}
struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
/// A runtime whose Codex engine is the fixture's (fake seams, fixture home), with the
/// given shared settings.
async fn runtime(f: &mut Fixture, settings: Settings) -> (TempDir, RuntimeHandle) {
    let parent = std::env::temp_dir().canonicalize().unwrap();
    let temp = tempfile::Builder::new()
        .prefix("primerswitch-codex-runtime-")
        .tempdir_in(parent)
        .unwrap();
    let paths = CliPaths::for_home(temp.path().join("claude-home"));
    fs::create_dir_all(&paths.config_dir).unwrap();
    fs::write(&paths.settings_file, b"{}").unwrap();
    let handle = RuntimeHandle::from_parts(
        Vault::with_key(temp.path().join("vault"), [11; 32]).unwrap(),
        ActiveStore::file(paths),
        ClaudeClient::with_transport("2.1.287", Arc::new(NoClaude)).unwrap(),
        Arc::new(TestClock(AtomicI64::new(NOW))),
    )
    .unwrap();
    // Already discovered: a tick never looks for the real installation.
    f.engine.discovered_at = Some(NOW);
    let codex = std::mem::replace(&mut f.engine, CodexEngine::empty(false));
    handle
        .edit_codex_for_test(move |engine, shared| {
            *engine = codex;
            *shared = settings;
        })
        .await;
    (temp, handle)
}
async fn until(condition: impl Fn() -> bool) {
    for _ in 0..400 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the fixture condition was never reached");
}

#[tokio::test]
async fn the_background_tick_switches_to_a_weekly_only_codex_account() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    f.read(&a, main_quota(13, 95), NOW);
    f.read(
        &b,
        weekly_only_quota(19, NOW + 4 * 86400 + 23 * 3600, true),
        NOW,
    );
    // Read the new active account through the fake service after switching.
    let mut reading = quota(Some("ws-b"), 0, 19);
    reading.rate_limits.primary = reading.rate_limits.secondary.take();
    f.factory.enqueue(Plan::quota(Ok(reading)));
    let daemon = f.daemon.clone();
    let settings = Settings {
        threshold: 94.0,
        poll_interval: 120,
        ..Settings::default()
    };
    let (_temp, handle) = runtime(&mut f, settings).await;
    let before = handle.get_codex_snapshot();
    assert_eq!(before.next_id, Some(b.clone()));
    assert_eq!(before.order, vec![b.clone()]);
    let mut notifications = handle.notifications();
    handle.codex_tick().await;
    let notice = notifications.try_recv().unwrap();
    assert_eq!(notice.title, "Codex switched to user-b@example.invalid");
    assert!(notice.body.contains("reached 95%"));
    assert_eq!(
        handle.get_codex_snapshot().selected_id.as_deref(),
        Some(b.as_str())
    );
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
    assert_eq!(*daemon.calls.lock().unwrap(), vec!["state", "restart"]);
    until(|| {
        handle.get_codex_snapshot().accounts.iter().any(|account| {
            account.id == b
                && account.quota_state == CodexQuotaState::Fresh
                && account.quota.as_ref().is_some_and(|quota| {
                    quota.limits[0].secondary.is_none()
                        && quota.limits[0].primary.as_ref().is_some_and(|window| {
                            window.used_percent == 19 && window.window_duration_mins == Some(10080)
                        })
                })
        })
    })
    .await;
}

#[tokio::test]
async fn the_background_tick_switches_codex_automatically_and_notifies() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    f.read(&a, main_quota(97, 40), NOW);
    f.read(&b, main_quota(10, 10), NOW);
    // The new active account is read right after the switch.
    f.factory
        .enqueue(Plan::quota(Ok(quota(Some("ws-b"), 12, 11))));
    let (daemon, factory) = (f.daemon.clone(), f.factory.clone());
    let settings = Settings {
        language: "en".into(),
        ..Settings::default()
    };
    let (_temp, handle) = runtime(&mut f, settings).await;
    assert_eq!(handle.get_codex_snapshot().next_id, Some(b.clone()));
    let mut notifications = handle.notifications();
    handle.codex_tick().await;
    let notice = notifications.try_recv().unwrap();
    assert_eq!(notice.title, "Codex switched to user-b@example.invalid");
    assert_eq!(
        notice.body,
        "user-a@example.invalid reached 97% · open terminals reconnect automatically."
    );
    let snapshot = handle.get_codex_snapshot();
    assert_eq!(snapshot.selected_id.as_deref(), Some(b.as_str()));
    assert_eq!(
        snapshot
            .last_switch
            .map(|s| (s.account_id, s.daemon_restarted)),
        Some((b.clone(), true))
    );
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
    assert_eq!(*daemon.calls.lock().unwrap(), vec!["state", "restart"]);
    until(|| factory.calls().iter().any(|call| call == "shutdown")).await;
    until(|| {
        handle.get_codex_snapshot().accounts.iter().any(|account| {
            account.id == b
                && account.quota.as_ref().is_some_and(|q| {
                    q.limits[0].primary.as_ref().map(|w| w.used_percent) == Some(12)
                })
        })
    })
    .await;
    // Within the cooldown nothing switches back, even with b at its limit and a free.
    handle
        .edit_codex_for_test(|engine, _| {
            record_reading(engine, &b, main_quota(99, 10), NOW);
            record_reading(engine, &a, main_quota(5, 5), NOW);
        })
        .await;
    assert_eq!(handle.get_codex_snapshot().next_id, Some(a.clone()));
    handle.codex_tick().await;
    assert_eq!(
        handle.get_codex_snapshot().selected_id.as_deref(),
        Some(b.as_str())
    );
    assert_eq!(daemon.calls.lock().unwrap().len(), 2);
    assert!(notifications.try_recv().is_err());
}

#[tokio::test]
async fn when_every_codex_account_is_limited_the_tick_notifies_once_and_never_switches() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    f.read(&a, main_quota_at(100, NOW + 3000, 40, NOW + 86400), NOW);
    f.read(&b, main_quota_at(100, NOW + 1200, 50, NOW + 86400), NOW);
    let daemon = f.daemon.clone();
    let settings = Settings {
        language: "ro".into(),
        ..Settings::default()
    };
    let (_temp, handle) = runtime(&mut f, settings).await;
    let mut notifications = handle.notifications();
    handle.codex_tick().await;
    let notice = notifications.try_recv().unwrap();
    assert_eq!(notice.title, "PrimerSwitch");
    assert!(
        notice.body.starts_with(
            "Toate conturile Codex sunt la limită. Primul cont liber va fi user-b@example.invalid, în 20 min ("
        ),
        "{}",
        notice.body
    );
    handle.codex_tick().await;
    assert!(notifications.try_recv().is_err());
    let snapshot = handle.get_codex_snapshot();
    assert_eq!(snapshot.selected_id.as_deref(), Some(a.as_str()));
    assert_eq!(snapshot.next_id, None);
    assert!(daemon.calls.lock().unwrap().is_empty());
    assert!(f.factory.calls().is_empty());
}

#[tokio::test]
async fn automatic_switching_stays_off_when_the_shared_setting_is_off() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    f.read(&a, main_quota(100, 40), NOW);
    f.read(&b, main_quota(10, 10), NOW);
    let daemon = f.daemon.clone();
    let settings = Settings {
        auto_switch_enabled: false,
        ..Settings::default()
    };
    let (_temp, handle) = runtime(&mut f, settings).await;
    let mut notifications = handle.notifications();
    handle.codex_tick().await;
    assert_eq!(
        handle.get_codex_snapshot().selected_id.as_deref(),
        Some(a.as_str())
    );
    // The window still recommends the next account.
    assert_eq!(handle.get_codex_snapshot().next_id, Some(b));
    assert!(daemon.calls.lock().unwrap().is_empty());
    assert!(notifications.try_recv().is_err());
}
