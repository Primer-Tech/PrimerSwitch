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
    resets: Arc<Mutex<VecDeque<Result<ResetOutcome, CodexError>>>>,
    after_resets: Arc<Mutex<VecDeque<Result<CodexRateLimits, CodexError>>>>,
    reset_keys: Arc<Mutex<Vec<String>>>,
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
            resets: self.resets.clone(),
            after_resets: self.after_resets.clone(),
            reset_keys: self.reset_keys.clone(),
            after_reset: false,
        }))
    }
}
struct FakeClient {
    home: PathBuf,
    plan: Plan,
    calls: Arc<Mutex<Vec<String>>>,
    resets: Arc<Mutex<VecDeque<Result<ResetOutcome, CodexError>>>>,
    after_resets: Arc<Mutex<VecDeque<Result<CodexRateLimits, CodexError>>>>,
    reset_keys: Arc<Mutex<Vec<String>>>,
    after_reset: bool,
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
        if self.after_reset {
            self.after_resets
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| self.plan.quota.clone())
        } else {
            self.plan.quota.clone()
        }
    }
    async fn consume_reset(&mut self, key: &str) -> Result<ResetOutcome, CodexError> {
        self.calls.lock().unwrap().push("reset".into());
        self.reset_keys.lock().unwrap().push(key.into());
        self.after_reset = true;
        self.resets
            .lock()
            .unwrap()
            .pop_front()
            .expect("every reset is explicitly planned")
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

fn reset_quota(
    workspace: Option<&str>,
    count: Option<i64>,
    plan: &str,
    used: i32,
) -> CodexRateLimits {
    let mut limits = quota(workspace, used, used);
    limits.rate_limits.plan_type = Some(plan.into());
    limits.rate_limit_reset_credits = count.map(|available_count| ResetCreditSummary {
        available_count,
        // Detail rows can be capped or absent. The count remains authoritative.
        credits: Some(vec![]),
    });
    limits
}
fn allow_fixture_reset(f: &mut Fixture, id: &str, plan: &str) {
    let index = f.engine.account_index(id).unwrap();
    let workspace = f.engine.saved.accounts[index].binding.workspace.clone();
    f.engine.saved.accounts[index].quota =
        Some(quota_view(&reset_quota(workspace.as_deref(), Some(2), plan, 100)).unwrap());
    f.engine.saved.accounts[index].quota_read_at = Some(100);
    f.engine.verified_now.insert(id.into());
    f.engine.save(&f.vault).unwrap();
}
fn fixture_reset_credit(id: &str, expires_at: Option<i64>) -> ResetCredit {
    ResetCredit {
        id: id.into(),
        reset_type: "codexRateLimits".into(),
        status: "available".into(),
        granted_at: 100,
        expires_at,
        title: Some("private-credit-title-sentinel".into()),
        description: Some("private-credit-description-sentinel".into()),
    }
}
#[test]
fn reset_expiry_details_preserve_dates_and_count_without_exporting_private_rows() {
    let mut limits = reset_quota(Some("ws-a"), Some(7), "team", 100);
    let mut redeemed = fixture_reset_credit("redeemed-private-id", Some(1000));
    redeemed.status = "redeemed".into();
    let mut other_type = fixture_reset_credit("unknown-type-private-id", Some(1000));
    other_type.reset_type = "futureResetType".into();
    limits.rate_limit_reset_credits.as_mut().unwrap().credits = Some(vec![
        fixture_reset_credit("later-private-id", Some(1_800_003_600)),
        fixture_reset_credit("forever-private-id", None),
        fixture_reset_credit("earlier-private-id", Some(1_800_000_000)),
        fixture_reset_credit("earlier-private-id", Some(1_800_000_000)),
        redeemed,
        other_type,
        fixture_reset_credit("invalid-private-id", Some(-1)),
        fixture_reset_credit("overflow-private-id", Some(i64::MAX)),
    ]);
    let view = quota_view(&limits).unwrap();
    let expected = json!([
        {"expiresAt":1_800_000_000_i64},
        {"expiresAt":1_800_003_600_i64},
        {"expiresAt":null}
    ]);
    let mut serialized = serde_json::to_value(&view).unwrap();
    assert_eq!(serialized["resetCreditsAvailable"], 7);
    assert_eq!(serialized["resetCreditDetails"], expected);
    assert!(!serialized.to_string().contains("private-"));
    let mut f = Fixture::new();
    f.engine.saved.accounts[0].quota = Some(view);
    f.engine.save(&f.vault).unwrap();
    let reopened = CodexEngine::load(&f.vault);
    assert_eq!(
        serde_json::to_value(reopened.saved.accounts[0].quota.as_ref().unwrap()).unwrap()["resetCreditDetails"],
        expected
    );
    // Historical cached quotas do not gain a field merely by round-tripping.
    serialized
        .as_object_mut()
        .unwrap()
        .remove("resetCreditDetails");
    let old: CodexQuotaView = serde_json::from_value(serialized.clone()).unwrap();
    assert!(old.reset_credit_details.is_none());
    assert_eq!(serde_json::to_value(old).unwrap(), serialized);
    limits.rate_limit_reset_credits.as_mut().unwrap().credits = None;
    assert!(quota_view(&limits).unwrap().reset_credit_details.is_none());
    limits.rate_limit_reset_credits.as_mut().unwrap().credits = Some(vec![]);
    assert!(
        quota_view(&limits)
            .unwrap()
            .reset_credit_details
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn explicit_codex_resets_work_for_personal_and_team_plans_and_refresh_usage() {
    for plan in [
        "plus",
        "pro",
        "team",
        "business",
        "self_serve_business_prolite",
    ] {
        let mut f = Fixture::new();
        let a = f.id_of("ws-a");
        allow_fixture_reset(&mut f, &a, plan);
        let active = f.active();
        f.factory.enqueue(Plan::quota(Ok(reset_quota(
            Some("ws-a"),
            Some(2),
            plan,
            100,
        ))));
        f.factory
            .resets
            .lock()
            .unwrap()
            .push_back(Ok(ResetOutcome::Reset));
        let mut after = reset_quota(Some("ws-a"), Some(1), plan, 0);
        after.rate_limit_reset_credits.as_mut().unwrap().credits =
            Some(vec![fixture_reset_credit(
                "remaining-private-id",
                Some(1000),
            )]);
        f.factory.after_resets.lock().unwrap().push_back(Ok(after));
        let job = f.engine.plan_reset(&a, &f.vault, 110).unwrap();
        let key = f.engine.saved.accounts[0]
            .pending_reset_key
            .clone()
            .unwrap();
        assert_eq!(
            f.vault.load::<Saved>(RECORD).unwrap().unwrap().accounts[0]
                .pending_reset_key
                .as_deref(),
            Some(key.as_str())
        );
        assert!(
            f.factory.reset_keys.lock().unwrap().is_empty(),
            "key persisted before any consume"
        );
        let report = job.run().await;
        f.engine.finish_quota(report, &f.vault, 120).unwrap();
        let view = f.view(&a);
        assert!(!view.reset.pending);
        assert_eq!(view.reset.last_outcome, Some(ResetOutcome::Reset));
        assert_eq!(
            view.quota.as_ref().unwrap().reset_credits_available,
            Some(1)
        );
        assert_eq!(
            view.quota
                .as_ref()
                .unwrap()
                .reset_credit_details
                .as_ref()
                .unwrap()[0]
                .expires_at,
            Some(1000)
        );
        assert_eq!(
            view.quota.as_ref().unwrap().limits[0]
                .primary
                .as_ref()
                .unwrap()
                .used_percent,
            0
        );
        assert_eq!(
            f.factory.calls(),
            vec!["start-Active", "quota", "reset", "quota", "shutdown"]
        );
        assert_eq!(f.active(), active);
        assert!(f.daemon.calls.lock().unwrap().is_empty());
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains(&key) && !json.contains("secret-sentinel"));
        f.claude_untouched();
    }
}
#[tokio::test]
async fn uncertain_codex_reset_survives_restart_and_reuses_key_even_when_no_credits_remain() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    allow_fixture_reset(&mut f, &a, "team");
    f.factory.enqueue(Plan::quota(Ok(reset_quota(
        Some("ws-a"),
        Some(1),
        "team",
        100,
    ))));
    f.factory
        .resets
        .lock()
        .unwrap()
        .push_back(Err(CodexError::Timeout));
    f.factory
        .after_resets
        .lock()
        .unwrap()
        .push_back(Ok(reset_quota(Some("ws-a"), Some(0), "team", 0)));
    let report = f.engine.plan_reset(&a, &f.vault, 110).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 120).unwrap();
    assert!(f.view(&a).reset.pending);
    assert_eq!(f.view(&a).reset.last_outcome, None);
    let key = f.factory.reset_keys.lock().unwrap()[0].clone();
    let installation = f.engine.installation.take();
    f.engine = CodexEngine::load(&f.vault).with_test_seams(
        f.factory.clone(),
        f.daemon.clone(),
        f.inventory.clone(),
        f.switcher.clone(),
    );
    f.engine.installation = installation;
    f.engine.observe(&f.vault, 130).unwrap();
    assert!(f.view(&a).reset.pending && f.view(&a).reset.usable.enabled);
    f.factory.enqueue(Plan::quota(Ok(reset_quota(
        Some("ws-a"),
        Some(0),
        "team",
        0,
    ))));
    f.factory
        .resets
        .lock()
        .unwrap()
        .push_back(Ok(ResetOutcome::AlreadyRedeemed));
    let report = f.engine.plan_reset(&a, &f.vault, 140).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 150).unwrap();
    assert_eq!(
        *f.factory.reset_keys.lock().unwrap(),
        vec![key.clone(), key]
    );
    assert_eq!(
        f.view(&a).reset.last_outcome,
        Some(ResetOutcome::AlreadyRedeemed)
    );
    assert!(!f.view(&a).reset.pending);
    f.claude_untouched();
}
#[tokio::test]
async fn reset_never_consumes_when_backend_owner_is_missing_or_changed() {
    for workspace in [None, Some("other-workspace")] {
        let mut f = Fixture::new();
        let a = f.id_of("ws-a");
        allow_fixture_reset(&mut f, &a, "team");
        f.factory.enqueue(Plan::quota(Ok(reset_quota(
            workspace,
            Some(2),
            "team",
            100,
        ))));
        let report = f.engine.plan_reset(&a, &f.vault, 110).unwrap().run().await;
        f.engine.finish_quota(report, &f.vault, 120).unwrap();
        assert!(f.factory.reset_keys.lock().unwrap().is_empty());
        assert_eq!(f.view(&a).error, Some(CodexReason::IdentityMismatch));
        assert!(f.view(&a).reset.pending);
    }
}
#[tokio::test]
async fn inactive_reset_adopts_rotations_without_switching_and_failed_refresh_discards_old_usage() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "initial");
    allow_fixture_reset(&mut f, &b, "business");
    let active = f.active();
    let rotated = auth("user-b", "ws-b", "rotated-reset");
    let mut plan = Plan::quota(Ok(reset_quota(Some("ws-b"), Some(1), "business", 100)));
    plan.rotate_shutdown = Some(rotated.clone());
    f.factory.enqueue(plan);
    f.factory
        .resets
        .lock()
        .unwrap()
        .push_back(Ok(ResetOutcome::Reset));
    f.factory
        .after_resets
        .lock()
        .unwrap()
        .push_back(Err(CodexError::ServiceUnavailable));
    let report = f.engine.plan_reset(&b, &f.vault, 110).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 120).unwrap();
    assert_eq!(f.saved_bytes("ws-b"), rotated);
    assert_eq!(f.active(), active);
    assert!(f.view(&b).quota.is_none());
    assert_eq!(f.view(&b).reset.last_outcome, Some(ResetOutcome::Reset));
    assert!(!f.view(&b).reset.pending);
    assert_eq!(f.factory.calls()[0], "start-Owned");
    f.claude_untouched();
}
#[tokio::test]
async fn reset_revalidates_count_and_retains_definite_non_consuming_outcomes() {
    for outcome in [ResetOutcome::NothingToReset, ResetOutcome::NoCredit] {
        let mut f = Fixture::new();
        let a = f.id_of("ws-a");
        allow_fixture_reset(&mut f, &a, "team");
        f.factory.enqueue(Plan::quota(Ok(reset_quota(
            Some("ws-a"),
            Some(1),
            "team",
            50,
        ))));
        f.factory.resets.lock().unwrap().push_back(Ok(outcome));
        let report = f.engine.plan_reset(&a, &f.vault, 110).unwrap().run().await;
        f.engine.finish_quota(report, &f.vault, 120).unwrap();
        assert!(!f.view(&a).reset.pending);
        assert_eq!(f.view(&a).reset.last_outcome, Some(outcome));
    }
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    allow_fixture_reset(&mut f, &a, "team");
    f.factory.enqueue(Plan::quota(Ok(reset_quota(
        Some("ws-a"),
        Some(0),
        "team",
        100,
    ))));
    let report = f.engine.plan_reset(&a, &f.vault, 110).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 120).unwrap();
    assert!(f.factory.reset_keys.lock().unwrap().is_empty());
    assert_eq!(f.view(&a).reset.last_outcome, Some(ResetOutcome::NoCredit));
}
#[test]
fn reset_capabilities_distinguish_unknown_zero_and_stale_without_plan_restrictions() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    allow_fixture_reset(&mut f, &a, "team");
    assert!(f.view(&a).reset.usable.enabled);
    f.engine.saved.accounts[0]
        .quota
        .as_mut()
        .unwrap()
        .reset_credits_available = None;
    assert_eq!(
        f.view(&a).reset.usable.blocked_reason,
        Some(CodexReason::ResetUnavailable)
    );
    f.engine.saved.accounts[0]
        .quota
        .as_mut()
        .unwrap()
        .reset_credits_available = Some(0);
    assert_eq!(
        f.engine.plan_reset(&a, &f.vault, 110).err(),
        Some(CodexReason::NoResetCredits)
    );
    allow_fixture_reset(&mut f, &a, "team");
    assert_eq!(
        f.engine
            .reset_capability(&f.engine.saved.accounts[0], 2000)
            .blocked_reason,
        Some(CodexReason::ResetUnavailable)
    );
    f.engine.demo = true;
    assert_eq!(
        f.engine.plan_reset(&a, &f.vault, 110).err(),
        Some(CodexReason::VaultUnavailable)
    );
    assert!(f.factory.calls().is_empty());
}
#[tokio::test]
async fn pending_codex_reset_cannot_be_deleted_and_storage_failure_sends_nothing() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "initial");
    allow_fixture_reset(&mut f, &b, "team");
    let job = f.engine.plan_reset(&b, &f.vault, 110).unwrap();
    assert_eq!(
        f.engine
            .command(CodexCommand::Delete(b.clone()), &f.vault, 120)
            .await
            .err(),
        Some(CodexReason::ResetUnconfirmed)
    );
    drop(job);
    assert!(f.factory.reset_keys.lock().unwrap().is_empty());

    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    allow_fixture_reset(&mut f, &a, "team");
    let record = f._root.path().join("vault/codex-state.vault");
    let kept = f._root.path().join("vault/codex-state-fixture-kept.vault");
    fs::rename(&record, &kept).unwrap();
    fs::create_dir(&record).unwrap();
    assert_eq!(
        f.engine.plan_reset(&a, &f.vault, 110).err(),
        Some(CodexReason::VaultUnavailable)
    );
    assert!(!f.engine.writable);
    assert!(f.factory.calls().is_empty());
    assert!(kept.is_file());
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
    // An imported API key is kept and can be selected by hand (see the API-key tests
    // below); agent identities, access tokens and Bedrock logins never can.
    let index = f
        .engine
        .adopt_or_import(
            OpaqueAuth::parse(api_key_auth_payload("sk-secret-sentinel").unwrap()).unwrap(),
            CodexIdentityEvidence::ClaimsOnly,
            210,
        )
        .unwrap();
    let api = f.engine.saved.accounts[index].id.clone();
    assert!(f.view(&api).switchable.enabled);
    f.engine.saved.accounts[index].auth_kind = AuthKind::Unsupported;
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
            reset_credit_details: None,
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
    // a (active), b (updated in place), c (new, named), API key (new, switchable by hand).
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
        reset_credit_details: None,
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
    engine.observe_return_window(index, at);
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
        previous_id: active.clone(),
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
    // An API-key login has no plan limits and is never an automatic target.
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
    f.engine.begin_auto_switch(NOW, &f.vault).unwrap();
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

fn account_preferences(f: &mut Fixture, id: &str, automation: AccountAutomation) {
    let index = f.engine.account_index(id).unwrap();
    f.engine.saved.accounts[index].automation = automation;
}
fn preferred(f: &mut Fixture, id: &str, threshold: Option<f64>, priority: u16) {
    account_preferences(
        f,
        id,
        AccountAutomation {
            threshold,
            priority: Some(priority),
            return_on_five_hour_reset: true,
            ..AccountAutomation::default()
        },
    );
}
fn return_to(f: &Fixture, id: &str) -> CodexAutomation {
    let CodexAutomation::Switch(mut plan) = switch_to(f, id, 0) else {
        unreachable!()
    };
    plan.peak = None;
    CodexAutomation::Return(plan)
}
fn reopen(f: &mut Fixture, now: i64) {
    let mut engine = CodexEngine::load(&f.vault).with_test_seams(
        f.factory.clone(),
        f.daemon.clone(),
        f.inventory.clone(),
        f.switcher.clone(),
    );
    engine.installation = f.engine.installation.take();
    engine.availability = CodexAvailability::Supported;
    engine.discovered_at = Some(now);
    engine.observe(&f.vault, now).unwrap();
    f.engine = engine;
}

#[tokio::test]
async fn codex_account_preferences_are_local_validated_decimal_and_omission_preserving() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let original_auth = f.saved_bytes("ws-b");
    let historical = serde_json::to_value(&f.engine.saved).unwrap();
    assert!(historical.get("return_hold_until").is_none());
    assert!(historical.get("last_auto_switch_at").is_none());
    assert!(historical["accounts"].as_array().unwrap().iter().all(|a| {
        a.get("automation").is_none() && a.get("preferred_return_reset_at").is_none()
    }));
    let view = AccountAutomationView {
        enabled: false,
        threshold: Some(99.5),
        priority: Some(2),
        return_on_five_hour_reset: true,
    };
    f.engine
        .command(
            CodexCommand::UpdateAccountAutomation(b.clone(), view.clone()),
            &f.vault,
            NOW,
        )
        .await
        .unwrap();
    assert_eq!(f.view(&b).automation, view);
    assert_eq!(f.view(&b).effective_threshold, 99.5);
    assert_eq!(f.view(&a).automation, AccountAutomationView::default());
    assert_eq!(f.saved_bytes("ws-b"), original_auth);
    assert!(f.factory.calls().is_empty());
    for threshold in [49.9, 100.1, f64::NAN, f64::INFINITY] {
        let invalid = AccountAutomationView {
            threshold: Some(threshold),
            ..view.clone()
        };
        assert_eq!(
            f.engine
                .command(
                    CodexCommand::UpdateAccountAutomation(b.clone(), invalid),
                    &f.vault,
                    NOW
                )
                .await
                .err(),
            Some(CodexReason::InvalidAutomation)
        );
        assert_eq!(f.view(&b).automation, view);
    }
    for priority in [0, 100] {
        let invalid = AccountAutomationView {
            priority: Some(priority),
            ..view.clone()
        };
        assert_eq!(
            f.engine
                .command(
                    CodexCommand::UpdateAccountAutomation(b.clone(), invalid),
                    &f.vault,
                    NOW
                )
                .await
                .err(),
            Some(CodexReason::InvalidAutomation)
        );
    }
    f.engine
        .command(
            CodexCommand::UpdateAccountAutomation(b.clone(), AccountAutomationView::default()),
            &f.vault,
            NOW,
        )
        .await
        .unwrap();
    let current = serde_json::to_value(&f.engine.saved).unwrap();
    assert!(
        current["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a.get("automation").is_none())
    );
    reopen(&mut f, NOW);
    assert_eq!(f.view(&b).automation, AccountAutomationView::default());
    f.claude_untouched();
}

#[test]
fn codex_provider_and_account_thresholds_drive_selection_cadence_status_and_priority() {
    let settings = Settings {
        threshold: 94.0,
        claude_threshold: Some(70.5),
        codex_threshold: Some(96.5),
        ..Settings::default()
    };
    let policy = CodexPolicy::from(&settings);
    assert_eq!(policy.threshold, 96.5);
    assert_eq!(settings.threshold_for(ProviderId::Claude), 70.5);
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    account_preferences(
        &mut f,
        &a,
        AccountAutomation {
            threshold: Some(80.0),
            ..AccountAutomation::default()
        },
    );
    account_preferences(
        &mut f,
        &b,
        AccountAutomation {
            threshold: Some(60.0),
            priority: Some(1),
            ..AccountAutomation::default()
        },
    );
    account_preferences(
        &mut f,
        &c,
        AccountAutomation {
            threshold: Some(99.5),
            priority: Some(2),
            ..AccountAutomation::default()
        },
    );
    f.read(&a, main_quota(79, 20), NOW);
    f.read(&b, main_quota(60, 20), NOW);
    f.read(&c, main_quota(91, 20), NOW);
    assert_eq!(f.next(&policy), Some(c.clone()));
    assert_eq!(
        f.engine.active_interval(&policy),
        ACTIVE_NEAR_LIMIT_INTERVAL
    );
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    f.read(&a, main_quota_at(80, NOW + 1200, 20, NOW + 86400), NOW);
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &c, 80));
    let view = f.engine.snapshot(false, NOW, &policy);
    let active = view.accounts.iter().find(|v| v.id == a).unwrap();
    assert!(active.exhausted);
    assert_eq!(active.effective_threshold, 80.0);
    assert_eq!(active.frees_at, Some(NOW + 1200));
    // Headroom comes before priority, and each uses its own threshold.
    f.read(&b, main_quota(58, 20), NOW);
    assert_eq!(f.order(&policy), vec![c.clone(), b.clone()]);
    f.read(&b, main_quota(50, 20), NOW);
    assert_eq!(f.order(&policy), vec![b.clone(), c.clone()]);
    let index = f.engine.account_index(&b).unwrap();
    f.engine.saved.accounts[index].automation.enabled = false;
    assert_eq!(f.order(&policy), vec![c.clone()]);
    f.read(&a, main_quota(69, 20), NOW);
    assert_eq!(f.engine.active_interval(&policy), policy.poll_interval);
    f.claude_untouched();
}

#[test]
fn lowering_the_codex_default_arms_the_already_observed_five_hour_window() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    preferred(&mut f, &a, None, 1);
    f.read(&a, main_quota(80, 20), NOW);
    assert!(
        f.engine.saved.accounts[0]
            .preferred_return_reset_at
            .is_none()
    );
    let settings = Settings {
        threshold: 95.0,
        codex_threshold: Some(80.0),
        claude_threshold: Some(99.5),
        ..Settings::default()
    };
    f.engine.set_policy(&CodexPolicy::from(&settings), NOW);
    assert_eq!(
        f.engine.saved.accounts[0].preferred_return_reset_at,
        Some(NOW + 3600)
    );
    assert!(f.factory.calls().is_empty());
    f.claude_untouched();
}

#[tokio::test]
async fn an_eighty_percent_rotation_returns_once_after_natural_recovery_across_restart() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    preferred(&mut f, &a, Some(80.0), 1);
    f.read(&a, main_quota_at(80, NOW + 1200, 20, NOW + 86400), NOW);
    f.read(&b, main_quota(20, 20), NOW);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        switch_to(&f, &b, 80)
    );
    let ai = f.engine.account_index(&a).unwrap();
    assert_eq!(
        f.engine.saved.accounts[ai].preferred_return_reset_at,
        Some(NOW + 1200)
    );
    f.engine.begin_auto_switch(NOW, &f.vault).unwrap();
    let job = f.engine.begin_switch(&b, &f.vault, NOW).unwrap().unwrap();
    f.engine
        .finish_switch(&job, &job.run().await, &f.vault, NOW + 1, true)
        .unwrap();
    reopen(&mut f, NOW + 1200);
    assert_eq!(
        f.engine.saved.accounts[ai].preferred_return_reset_at,
        Some(NOW + 1200)
    );
    assert_eq!(
        f.engine.due_quota(NOW + 1200, &CodexPolicy::default()),
        Some(a.clone())
    );
    f.read(&b, main_quota(20, 20), NOW + 1200);
    assert_eq!(
        f.engine.automation(NOW + 1200, &CodexPolicy::default()),
        CodexAutomation::Refresh(a.clone())
    );
    f.read(
        &a,
        main_quota_at(0, NOW + 18000, 20, NOW + 86400),
        NOW + 1200,
    );
    assert_eq!(
        f.engine.automation(NOW + 1200, &CodexPolicy::default()),
        return_to(&f, &a)
    );
    f.engine.begin_auto_switch(NOW + 1200, &f.vault).unwrap();
    let job = f
        .engine
        .begin_switch(&a, &f.vault, NOW + 1200)
        .unwrap()
        .unwrap();
    f.engine
        .finish_switch(&job, &job.run().await, &f.vault, NOW + 1201, true)
        .unwrap();
    assert!(f.engine.saved.return_hold_until.is_none());
    assert_eq!(f.engine.saved.accounts[ai].preferred_return_reset_at, None);
    assert_eq!(
        f.engine.saved.accounts[ai].preferred_return_handled_reset_at,
        Some(NOW + 1200)
    );
    reopen(&mut f, NOW + 1801);
    f.write_active(&auth("user-b", "ws-b", "saved"));
    f.engine.observe(&f.vault, NOW + 1801).unwrap();
    f.engine.saved.return_hold_until = None;
    f.read(&b, main_quota(20, 20), NOW + 1801);
    f.read(
        &a,
        main_quota_at(0, NOW + 18000, 20, NOW + 86400),
        NOW + 1801,
    );
    assert_eq!(
        f.engine.automation(NOW + 1801, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    f.claude_untouched();
}

#[test]
fn preferred_returns_respect_all_reported_headroom_missing_windows_identity_and_priorities() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    preferred(&mut f, &b, Some(80.0), 1);
    f.read(
        &b,
        main_quota_at(80, NOW - 600, 20, NOW + 86400),
        NOW - 1800,
    );
    f.read(&a, main_quota(20, 20), NOW);
    let recovery = main_quota_at(0, NOW + 18000, 20, NOW + 86400);
    for week in [76, 80, 99, 100] {
        f.read(&b, main_quota_at(0, NOW + 18000, week, NOW + 86400), NOW);
        assert_eq!(
            f.engine.automation(NOW, &CodexPolicy::default()),
            CodexAutomation::Idle
        );
        assert_ne!(
            f.engine
                .due_quota(NOW + RETURN_RETRY, &CodexPolicy::default()),
            Some(b.clone())
        );
    }
    let mut missing_five = recovery.clone();
    missing_five.limits[0].primary = None;
    f.read(&b, missing_five, NOW);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    let mut unknown_duration = recovery.clone();
    unknown_duration.limits[0]
        .primary
        .as_mut()
        .unwrap()
        .window_duration_mins = None;
    f.read(&b, unknown_duration, NOW);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    let mut unknown_new_date = recovery.clone();
    unknown_new_date.limits[0]
        .primary
        .as_mut()
        .unwrap()
        .resets_at = None;
    unknown_new_date.limits[0]
        .primary
        .as_mut()
        .unwrap()
        .used_percent = 1;
    f.read(&b, unknown_new_date.clone(), NOW);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    // Explicit zero use with no started window is fresh recovery evidence.
    unknown_new_date.limits[0]
        .primary
        .as_mut()
        .unwrap()
        .used_percent = 0;
    unknown_new_date.limits[0]
        .secondary
        .as_mut()
        .unwrap()
        .resets_at = None;
    f.read(&b, unknown_new_date, NOW);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        return_to(&f, &b)
    );
    let index = f.engine.account_index(&b).unwrap();
    f.engine.saved.accounts[index].pending_reset_key = Some("fixture-pending-key".into());
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    f.engine.saved.accounts[index].pending_reset_key = None;
    f.engine.verified_now.remove(&b);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    f.read(&b, recovery, NOW);
    f.engine.saved.accounts[index].automation.enabled = false;
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    f.engine.saved.accounts[index].automation.enabled = true;
    let ai = f.engine.account_index(&a).unwrap();
    f.engine.saved.accounts[ai].automation.priority = Some(1);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    f.engine.saved.accounts[ai].automation.priority = Some(2);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        return_to(&f, &b)
    );
}

#[tokio::test]
async fn manual_return_holds_resume_and_failure_cooldowns_survive_restart() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    preferred(&mut f, &b, None, 1);
    f.read(
        &b,
        main_quota_at(100, NOW - 600, 20, NOW + 86400),
        NOW - 1800,
    );
    f.read(&b, main_quota_at(0, NOW + 18000, 20, NOW + 86400), NOW);
    f.read(&a, main_quota(20, 20), NOW);
    f.engine.hold_automatic_returns(NOW, &f.vault).unwrap();
    reopen(&mut f, NOW);
    f.read(&a, main_quota(20, 20), NOW);
    f.read(&b, main_quota_at(0, NOW + 18000, 20, NOW + 86400), NOW);
    assert_eq!(
        f.engine
            .snapshot(false, NOW, &CodexPolicy::default())
            .return_hold_until,
        Some(NOW + 900)
    );
    assert_eq!(
        f.engine.automation(NOW + 899, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    assert_eq!(
        f.engine.automation(NOW + 900, &CodexPolicy::default()),
        return_to(&f, &b)
    );
    f.engine
        .command(CodexCommand::ResumeAutomaticReturns, &f.vault, NOW)
        .await
        .unwrap();
    assert_eq!(
        f.engine
            .snapshot(false, NOW, &CodexPolicy::default())
            .return_hold_until,
        None
    );
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        return_to(&f, &b)
    );
    f.engine.begin_auto_switch(NOW, &f.vault).unwrap();
    *f.inventory.0.lock().unwrap() = CodexProcessSummary {
        daemon_running: true,
        other_clients: 99,
        ..CodexProcessSummary::default()
    };
    assert_eq!(
        f.engine.begin_switch(&b, &f.vault, NOW).err(),
        Some(CodexReason::DaemonRestartFailed)
    );
    reopen(&mut f, NOW + 1);
    f.read(&a, main_quota(20, 20), NOW + 1);
    f.read(&b, main_quota_at(0, NOW + 18000, 20, NOW + 86400), NOW + 1);
    assert_eq!(
        f.engine.automation(NOW + 599, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    assert_eq!(
        f.engine.automation(NOW + 600, &CodexPolicy::default()),
        return_to(&f, &b)
    );
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
    f.claude_untouched();
}

#[tokio::test]
async fn failed_or_unidentified_recovery_readings_are_backed_off_and_never_authorize_returns() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    preferred(&mut f, &b, None, 1);
    f.read(
        &b,
        main_quota_at(100, NOW - 600, 20, NOW + 86400),
        NOW - 1800,
    );
    f.read(&a, main_quota(20, 20), NOW);
    f.factory
        .enqueue(Plan::quota(Err(CodexError::ServiceUnavailable)));
    let report = f.engine.plan_quota(&b, NOW, false).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, NOW).unwrap();
    assert_eq!(
        f.engine.automation(NOW + 60, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    assert_ne!(
        f.engine.due_quota(NOW + 60, &CodexPolicy::default()),
        Some(b.clone())
    );
    reopen(&mut f, NOW + 60);
    f.read(&a, main_quota(20, 20), NOW + 60);
    assert_eq!(
        f.engine.automation(NOW + 60, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    assert_eq!(
        f.engine
            .automation(NOW + ERROR_RETRY, &CodexPolicy::default()),
        CodexAutomation::Refresh(b.clone())
    );
    f.read(
        &b,
        main_quota_at(0, NOW + 18000, 20, NOW + 86400),
        NOW + ERROR_RETRY,
    );
    let mut unknown = quota(None, 0, 20);
    unknown.rate_limits.primary.as_mut().unwrap().resets_at = Some(NOW + 18000);
    f.factory.enqueue(Plan::quota(Ok(unknown)));
    let report = f
        .engine
        .plan_quota(&b, NOW + ERROR_RETRY + 1, false)
        .unwrap()
        .run()
        .await;
    f.engine
        .finish_quota(report, &f.vault, NOW + ERROR_RETRY + 1)
        .unwrap();
    assert!(!f.engine.verified_now.contains(&b));
    assert_eq!(
        f.engine
            .automation(NOW + ERROR_RETRY + 1, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    assert!(f.factory.reset_keys.lock().unwrap().is_empty());
    f.claude_untouched();
}

#[tokio::test]
async fn a_banked_codex_reset_handles_the_natural_return_marker_without_automatic_redemption() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    preferred(&mut f, &b, None, 1);
    allow_fixture_reset(&mut f, &b, "pro");
    let index = f.engine.account_index(&b).unwrap();
    f.engine.observe_return_window(index, 100);
    assert_eq!(
        f.engine.saved.accounts[index].preferred_return_reset_at,
        Some(9000)
    );
    f.factory.enqueue(Plan::quota(Ok(reset_quota(
        Some("ws-b"),
        Some(2),
        "pro",
        100,
    ))));
    f.factory
        .resets
        .lock()
        .unwrap()
        .push_back(Ok(ResetOutcome::Reset));
    f.factory
        .after_resets
        .lock()
        .unwrap()
        .push_back(Ok(quota(Some("ws-b"), 0, 20)));
    let report = f.engine.plan_reset(&b, &f.vault, 110).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, 120).unwrap();
    assert_eq!(
        f.engine.saved.accounts[index].preferred_return_reset_at,
        None
    );
    assert_eq!(
        f.engine.saved.accounts[index].preferred_return_handled_reset_at,
        Some(9000)
    );
    assert_eq!(f.factory.reset_keys.lock().unwrap().len(), 1);
    reopen(&mut f, 9100);
    assert_eq!(
        f.engine.saved.accounts[index].preferred_return_reset_at,
        None
    );
    f.claude_untouched();
}

#[tokio::test]
async fn background_codex_return_reuses_seamless_switch_and_reports_natural_recovery() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    preferred(&mut f, &b, Some(80.0), 1);
    f.read(
        &b,
        main_quota_at(80, NOW - 600, 20, NOW + 86400),
        NOW - 1800,
    );
    f.read(&a, main_quota(20, 20), NOW);
    let mut recovery = quota(Some("ws-b"), 0, 20);
    recovery.rate_limits.primary.as_mut().unwrap().resets_at = None;
    recovery.rate_limits.secondary.as_mut().unwrap().resets_at = None;
    f.factory.enqueue(Plan::quota(Ok(recovery.clone())));
    f.factory.enqueue(Plan::quota(Ok(recovery)));
    let daemon = f.daemon.clone();
    let factory = f.factory.clone();
    let (_temp, handle) = runtime(
        &mut f,
        Settings {
            claude_threshold: Some(50.0),
            codex_threshold: Some(95.0),
            ..Settings::default()
        },
    )
    .await;
    let mut notifications = handle.notifications();
    handle.codex_tick().await;
    let notice = notifications.try_recv().unwrap();
    assert_eq!(notice.title, "Codex switched to user-b@example.invalid");
    assert!(notice.body.contains("5-hour limit recovered"));
    assert!(!notice.body.contains("reached its limit"));
    assert_eq!(handle.get_codex_snapshot().selected_id, Some(b.clone()));
    assert_eq!(handle.get_codex_snapshot().return_hold_until, None);
    assert_eq!(*daemon.calls.lock().unwrap(), vec!["state", "restart"]);
    until(|| {
        factory
            .calls()
            .iter()
            .filter(|call| *call == "shutdown")
            .count()
            == 2
    })
    .await;
    handle.codex_tick().await;
    assert_eq!(daemon.calls.lock().unwrap().len(), 2);
    assert!(notifications.try_recv().is_err());
    assert!(factory.reset_keys.lock().unwrap().is_empty());
}

fn ready_return(f: &mut Fixture) -> (String, String, CodexAutoSwitch) {
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    preferred(f, &b, Some(80.0), 1);
    f.read(
        &b,
        main_quota_at(80, NOW - 600, 20, NOW + 86400),
        NOW - 1800,
    );
    f.read(&a, main_quota(20, 20), NOW);
    f.read(&b, main_quota_at(0, NOW + 18000, 20, NOW + 86400), NOW);
    let CodexAutomation::Return(plan) = f.engine.automation(NOW, &CodexPolicy::default()) else {
        panic!("fixture has a recovered higher-priority account");
    };
    (a, b, plan)
}

#[tokio::test]
async fn external_owner_selection_persists_a_hold_but_same_owner_rotation_does_not() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    f.write_active(&auth("user-a", "ws-a", "rotated"));
    f.engine.observe(&f.vault, NOW).unwrap();
    assert_eq!(f.engine.selected_id, Some(a));
    assert_eq!(f.engine.saved.return_hold_until, None);
    let external = auth("user-c", "ws-c", "external");
    f.write_active(&external);
    f.engine.observe(&f.vault, NOW + 1).unwrap();
    let c = f.id_of("ws-c");
    assert_eq!(f.engine.selected_id, Some(c.clone()));
    assert_eq!(f.saved_bytes("ws-c"), external);
    assert_eq!(f.engine.saved.return_hold_until, Some(NOW + 901));
    reopen(&mut f, NOW + 2);
    assert_eq!(f.engine.saved.return_hold_until, Some(NOW + 901));
    f.engine
        .command(CodexCommand::ResumeAutomaticReturns, &f.vault, NOW + 3)
        .await
        .unwrap();
    f.write_active(&auth("user-c", "ws-c", "rotated"));
    f.engine.observe(&f.vault, NOW + 4).unwrap();
    assert_eq!(f.engine.selected_id, Some(c));
    assert_eq!(f.engine.saved.return_hold_until, None);
    assert!(f.factory.calls().is_empty());
    f.claude_untouched();
}

#[test]
fn a_prepared_codex_return_preserves_an_external_login_before_or_during_replacement() {
    for during_replace in [false, true] {
        let mut f = Fixture::new();
        let (a, _, plan) = ready_return(&mut f);
        // Owner/generation protection is unchanged when outgoing quota is stale.
        f.engine.verified_now.remove(&a);
        f.engine
            .account_errors
            .insert(a, CodexReason::ProviderUnavailable);
        let external = auth("user-c", "ws-c", "external");
        if during_replace {
            let path = f.home.join("auth.json");
            let bytes = external.clone();
            f.engine.before_automatic_replace =
                Some(Box::new(move || fs::write(path, bytes).unwrap()));
        } else {
            f.write_active(&external);
        }
        f.engine.begin_auto_switch(NOW, &f.vault).unwrap();
        assert_eq!(
            f.engine
                .begin_automatic_switch(&plan, true, &CodexPolicy::default(), &f.vault, NOW)
                .err(),
            Some(CodexReason::ExternalChange)
        );
        assert_eq!(f.active(), external);
        assert_eq!(f.saved_bytes("ws-c"), external);
        assert_eq!(f.engine.selected_id, Some(f.id_of("ws-c")));
        assert_eq!(
            f.engine.saved.return_hold_until,
            Some(NOW + MANUAL_RETURN_HOLD)
        );
        assert!(f.daemon.calls.lock().unwrap().is_empty());
        reopen(&mut f, NOW + 1);
        assert_eq!(
            f.engine.saved.return_hold_until,
            Some(NOW + MANUAL_RETURN_HOLD)
        );
        f.claude_untouched();
    }
}

#[tokio::test]
async fn external_selection_during_return_preflight_and_preference_edits_cancel_the_old_plan() {
    let mut f = Fixture::new();
    let (_, b, plan) = ready_return(&mut f);
    // An inactive quota reader finishes while another client selects account c.
    let mut recovery = quota(Some("ws-b"), 0, 20);
    recovery.rate_limits.primary.as_mut().unwrap().resets_at = Some(NOW + 18000);
    f.factory.enqueue(Plan::quota(Ok(recovery)));
    let report = f.engine.plan_quota(&b, NOW, false).unwrap().run().await;
    let external = auth("user-c", "ws-c", "external");
    f.write_active(&external);
    f.engine.finish_quota(report, &f.vault, NOW).unwrap();
    let c = f.id_of("ws-c");
    f.read(&c, main_quota(20, 20), NOW);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    f.engine.begin_auto_switch(NOW, &f.vault).unwrap();
    assert_eq!(
        f.engine
            .begin_automatic_switch(&plan, true, &CodexPolicy::default(), &f.vault, NOW)
            .err(),
        Some(CodexReason::ExternalChange)
    );
    assert_eq!(f.active(), external);
    assert!(f.daemon.calls.lock().unwrap().is_empty());
    // A local preference or manual hold written after planning is revalidated too.
    let mut f = Fixture::new();
    let (_, b, plan) = ready_return(&mut f);
    let index = f.engine.account_index(&b).unwrap();
    f.engine.saved.accounts[index].automation.enabled = false;
    f.engine.begin_auto_switch(NOW, &f.vault).unwrap();
    assert_eq!(
        f.engine
            .begin_automatic_switch(&plan, true, &CodexPolicy::default(), &f.vault, NOW)
            .err(),
        Some(CodexReason::ExternalChange)
    );
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
    f.engine.saved.accounts[index].automation.enabled = true;
    f.engine.hold_automatic_returns(NOW, &f.vault).unwrap();
    assert_eq!(
        f.engine
            .begin_automatic_switch(&plan, true, &CodexPolicy::default(), &f.vault, NOW)
            .err(),
        Some(CodexReason::ExternalChange)
    );
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
    f.claude_untouched();
}

async fn background_read(
    f: &mut Fixture,
    id: &str,
    workspace: &str,
    five: i32,
    week: i32,
    now: i64,
) {
    let mut reading = quota(Some(workspace), five, week);
    reading.rate_limits.primary.as_mut().unwrap().resets_at = Some(now + 18000);
    reading.rate_limits.secondary.as_mut().unwrap().resets_at = Some(now + 7 * 86400);
    f.factory.enqueue(Plan::quota(Ok(reading)));
    let report = f.engine.plan_quota(id, now, false).unwrap().run().await;
    f.engine.finish_quota(report, &f.vault, now).unwrap();
}

#[tokio::test]
async fn minute_active_polling_alternates_with_due_inactive_reset_and_hourly_readings() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let c = f.add("user-c", "ws-c", "saved");
    f.read(
        &b,
        main_quota_at(100, NOW + 30, 20, NOW + 86400),
        NOW - 4000,
    );
    f.read(&c, main_quota(20, 20), NOW - 4000);
    background_read(&mut f, &a, "ws-a", 90, 20, NOW).await;
    let policy = CodexPolicy::default();
    assert_eq!(f.engine.active_interval(&policy), 60);
    // Both the active and inactive accounts stay due at these minute ticks.
    // Reset recovery gets its turn, then the active account, then hourly work.
    assert_eq!(f.engine.due_quota(NOW + 60, &policy), Some(b.clone()));
    background_read(&mut f, &b, "ws-b", 0, 20, NOW + 60).await;
    assert_eq!(f.engine.due_quota(NOW + 120, &policy), Some(a.clone()));
    background_read(&mut f, &a, "ws-a", 90, 20, NOW + 120).await;
    assert_eq!(f.engine.due_quota(NOW + 180, &policy), Some(c.clone()));
    background_read(&mut f, &c, "ws-c", 20, 20, NOW + 180).await;
    assert_eq!(f.engine.due_quota(NOW + 240, &policy), Some(a));
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
    assert!(f.factory.reset_keys.lock().unwrap().is_empty());
    f.claude_untouched();
}

#[test]
fn known_inactive_reset_retry_floors_are_independent_of_the_active_check_interval() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    f.read(&a, main_quota(20, 20), NOW);
    f.read(
        &b,
        main_quota_at(10, NOW + RETURN_RETRY, 20, NOW + 86400),
        NOW,
    );
    let policy = CodexPolicy {
        poll_interval: 900,
        ..CodexPolicy::default()
    };
    assert_eq!(f.engine.due_quota(NOW + RETURN_RETRY - 1, &policy), None);
    assert_eq!(
        f.engine.due_quota(NOW + RETURN_RETRY, &policy),
        Some(b.clone())
    );
    // A failed reading still waits900 seconds rather than the successful reset floor.
    let index = f.engine.account_index(&b).unwrap();
    f.engine.saved.accounts[index].last_attempt_at = Some(NOW + 60);
    f.engine
        .account_errors
        .insert(b.clone(), CodexReason::ProviderUnavailable);
    assert_eq!(f.engine.due_quota(NOW + RETURN_RETRY, &policy), None);
    assert_eq!(f.engine.due_quota(NOW + 60 + ERROR_RETRY, &policy), Some(b));
    assert!(f.factory.calls().is_empty());
}

#[tokio::test]
async fn a_weekly_blocked_preferred_return_is_rechecked_after_its_later_weekly_reset() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    preferred(&mut f, &b, Some(80.0), 1);
    f.read(&b, main_quota_at(80, NOW - 3600, 76, NOW + 120), NOW - 4200);
    f.read(&b, main_quota_at(0, NOW + 18000, 76, NOW + 120), NOW);
    background_read(&mut f, &a, "ws-a", 90, 20, NOW).await;
    let policy = CodexPolicy::default();
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    assert_eq!(f.engine.due_quota(NOW + 120, &policy), Some(b.clone()));
    // An unsuccessful weekly-reset recheck preserves its longer retry floor.
    f.factory
        .enqueue(Plan::quota(Err(CodexError::ServiceUnavailable)));
    let report = f
        .engine
        .plan_quota(&b, NOW + 120, false)
        .unwrap()
        .run()
        .await;
    f.engine.finish_quota(report, &f.vault, NOW + 120).unwrap();
    assert_ne!(f.engine.due_quota(NOW + 240, &policy), Some(b.clone()));
    background_read(&mut f, &a, "ws-a", 90, 20, NOW + 120 + ERROR_RETRY - 60).await;
    assert_eq!(
        f.engine.due_quota(NOW + 120 + ERROR_RETRY, &policy),
        Some(b.clone())
    );
    background_read(&mut f, &b, "ws-b", 0, 0, NOW + 120 + ERROR_RETRY).await;
    assert_eq!(
        f.engine.automation(NOW + 120 + ERROR_RETRY, &policy),
        return_to(&f, &b)
    );
    assert!(f.factory.reset_keys.lock().unwrap().is_empty());
    f.claude_untouched();
}

#[tokio::test]
async fn background_sign_in_retries_are_not_starved_by_a_near_limit_active_account() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let index = f.engine.account_index(&b).unwrap();
    f.engine.saved.accounts[index].needs_sign_in = true;
    f.engine.saved.accounts[index].last_attempt_at = Some(NOW - SIGN_IN_RETRY);
    background_read(&mut f, &a, "ws-a", 90, 20, NOW).await;
    assert_eq!(
        f.engine.due_quota(NOW + 60, &CodexPolicy::default()),
        Some(b.clone())
    );
    background_read(&mut f, &b, "ws-b", 20, 20, NOW + 60).await;
    assert!(!f.engine.saved.accounts[index].needs_sign_in);
    assert_eq!(
        f.engine.due_quota(NOW + 120, &CodexPolicy::default()),
        Some(a)
    );
    f.claude_untouched();
}

#[tokio::test]
async fn spent_history_survives_disabled_returns_restart_recovery_and_late_opt_in() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    // Keep history even when the account has no priority or return preference yet.
    f.read(&b, main_quota_at(95, NOW + 300, 20, NOW + 86400), NOW);
    let index = f.engine.account_index(&b).unwrap();
    assert_eq!(
        f.engine.saved.accounts[index].preferred_return_reset_at,
        Some(NOW + 300)
    );
    f.engine.save(&f.vault).unwrap();
    reopen(&mut f, NOW + 600);
    assert!(!f.engine.verified_now.contains(&b));
    background_read(&mut f, &b, "ws-b", 0, 20, NOW + 600).await;
    assert_eq!(
        f.engine.saved.accounts[index].preferred_return_reset_at,
        Some(NOW + 300)
    );
    f.engine
        .command(
            CodexCommand::UpdateAccountAutomation(
                b.clone(),
                AccountAutomationView {
                    enabled: true,
                    threshold: None,
                    priority: Some(1),
                    return_on_five_hour_reset: true,
                },
            ),
            &f.vault,
            NOW + 600,
        )
        .await
        .unwrap();
    // Outgoing account a has no fresh quota after restart; its live owner is known.
    assert_eq!(f.engine.selected_id, Some(a));
    assert_eq!(
        f.engine.automation(NOW + 600, &CodexPolicy::default()),
        return_to(&f, &b)
    );
    f.claude_untouched();
}

#[tokio::test]
async fn legacy_cached_spent_history_can_arm_after_restart_without_session_verification() {
    let mut f = Fixture::new();
    let b = f.add("user-b", "ws-b", "saved");
    account_preferences(
        &mut f,
        &b,
        AccountAutomation {
            threshold: Some(80.0),
            ..AccountAutomation::default()
        },
    );
    f.read(&b, main_quota_at(80, NOW + 300, 20, NOW + 86400), NOW);
    let index = f.engine.account_index(&b).unwrap();
    // Simulate an older record from before spent history was retained by default.
    f.engine.saved.accounts[index].preferred_return_reset_at = None;
    f.engine.save(&f.vault).unwrap();
    reopen(&mut f, NOW + 600);
    assert!(!f.engine.verified_now.contains(&b));
    f.engine
        .command(
            CodexCommand::UpdateAccountAutomation(
                b.clone(),
                AccountAutomationView {
                    enabled: true,
                    threshold: Some(80.0),
                    priority: Some(1),
                    return_on_five_hour_reset: true,
                },
            ),
            &f.vault,
            NOW + 600,
        )
        .await
        .unwrap();
    assert_eq!(
        f.engine.saved.accounts[index].preferred_return_reset_at,
        Some(NOW + 300)
    );
    // The historical marker alone never supplies fresh post-reset usage/identity.
    assert_eq!(
        f.engine.automation(NOW + 600, &CodexPolicy::default()),
        CodexAutomation::Refresh(b.clone())
    );
    assert!(f.factory.calls().is_empty());
    background_read(&mut f, &b, "ws-b", 0, 20, NOW + 600).await;
    assert_eq!(
        f.engine.automation(NOW + 600, &CodexPolicy::default()),
        return_to(&f, &b)
    );
    f.claude_untouched();
}

#[tokio::test]
async fn near_headroom_history_arms_ninety_five_at_ninety_seven_but_not_ninety_two() {
    for used in [92, 95] {
        for legacy_missing_history in [false, true] {
            let mut f = Fixture::new();
            let b = f.add("user-b", "ws-b", "saved");
            account_preferences(
                &mut f,
                &b,
                AccountAutomation {
                    threshold: Some(97.0),
                    ..AccountAutomation::default()
                },
            );
            f.read(&b, main_quota_at(used, NOW + 300, 30, NOW + 86400), NOW);
            let index = f.engine.account_index(&b).unwrap();
            let observed_reset = (used > 92).then_some(NOW + 300);
            assert_eq!(
                f.engine.saved.accounts[index].preferred_return_reset_at,
                observed_reset
            );
            if legacy_missing_history {
                f.engine.saved.accounts[index].preferred_return_reset_at = None;
            }
            f.engine.save(&f.vault).unwrap();
            reopen(&mut f, NOW + 600);
            assert!(!f.engine.verified_now.contains(&b));
            f.engine
                .command(
                    CodexCommand::UpdateAccountAutomation(
                        b.clone(),
                        AccountAutomationView {
                            enabled: true,
                            threshold: Some(97.0),
                            priority: Some(1),
                            return_on_five_hour_reset: true,
                        },
                    ),
                    &f.vault,
                    NOW + 600,
                )
                .await
                .unwrap();
            assert_eq!(
                f.engine.saved.accounts[index].preferred_return_reset_at,
                observed_reset
            );
            background_read(&mut f, &b, "ws-b", 0, 30, NOW + 600).await;
            assert_eq!(
                f.engine.automation(NOW + 600, &CodexPolicy::default()),
                if used > 92 {
                    return_to(&f, &b)
                } else {
                    CodexAutomation::Idle
                }
            );
            f.claude_untouched();
        }
    }
}

#[tokio::test]
async fn a_natural_return_survives_failed_outgoing_quota_but_keeps_target_and_owner_guards() {
    let mut f = Fixture::new();
    let (a, b, plan) = ready_return(&mut f);
    let index = f.engine.account_index(&a).unwrap();
    f.engine.saved.accounts[index].quota = None;
    f.engine.saved.accounts[index].quota_read_at = None;
    f.engine.saved.accounts[index].needs_sign_in = true;
    f.engine.verified_now.remove(&a);
    f.engine
        .account_errors
        .insert(a.clone(), CodexReason::SignInRequired);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        return_to(&f, &b)
    );
    // The fresh target proof remains mandatory despite relaxing outgoing usage.
    f.engine.verified_now.remove(&b);
    assert_eq!(
        f.engine.automation(NOW, &CodexPolicy::default()),
        CodexAutomation::Idle
    );
    assert_eq!(
        f.engine
            .begin_automatic_switch(&plan, true, &CodexPolicy::default(), &f.vault, NOW)
            .err(),
        Some(CodexReason::ExternalChange)
    );
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
    f.read(&b, main_quota_at(0, NOW + 18000, 20, NOW + 86400), NOW);
    f.engine.begin_auto_switch(NOW, &f.vault).unwrap();
    let job = f
        .engine
        .begin_automatic_switch(&plan, true, &CodexPolicy::default(), &f.vault, NOW)
        .unwrap()
        .unwrap();
    f.engine
        .finish_switch(&job, &job.run().await, &f.vault, NOW + 1, true)
        .unwrap();
    assert_eq!(f.engine.selected_id, Some(b));
    assert_eq!(f.active(), auth("user-b", "ws-b", "saved"));
    assert!(f.factory.calls().is_empty());
    f.claude_untouched();
}

// API-key accounts: saved by name, selected only by hand, never by automation.
const FAKE_KEY: &str = "sk-proj-SENTINEL-0123456789abcdef";
fn key_file(key: &str) -> Vec<u8> {
    api_key_auth_payload(key).unwrap()
}
impl Fixture {
    async fn add_key(&mut self, name: &str, key: &str) -> Result<(), CodexReason> {
        self.engine
            .command(
                CodexCommand::AddApiKey(name.into(), Zeroizing::new(key.into())),
                &self.vault,
                150,
            )
            .await
            .map(|_| ())
    }
    fn key_id(&self, key: &str) -> String {
        let wanted = key_file(key);
        self.engine
            .saved
            .accounts
            .iter()
            .find(|a| a.auth_kind == AuthKind::ApiKey && same_api_key(&a.auth, &wanted))
            .unwrap()
            .id
            .clone()
    }
    fn saved_by_id(&self, id: &str) -> Vec<u8> {
        self.engine.saved.accounts[self.engine.account_index(id).unwrap()]
            .auth
            .clone()
    }
    fn vault_holds(&self, needle: &str) -> bool {
        fs::read_dir(self._root.path().join("vault"))
            .unwrap()
            .flatten()
            .any(|entry| {
                fs::read(entry.path()).is_ok_and(|bytes| {
                    bytes
                        .windows(needle.len())
                        .any(|window| window == needle.as_bytes())
                })
            })
    }
}

#[tokio::test]
async fn an_api_key_is_saved_by_name_without_selecting_it_or_touching_the_active_login() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let active = f.active();
    // Pasted whitespace around the key and the name is dropped.
    f.add_key("  Work key ", &format!("  {FAKE_KEY}\r\n"))
        .await
        .unwrap();
    let key = f.key_id(FAKE_KEY);
    assert_eq!(f.engine.saved.accounts.len(), 2);
    assert_eq!(f.engine.selected_id.as_deref(), Some(a.as_str()));
    assert_eq!(f.active(), active);
    let view = f.view(&key);
    assert_eq!(view.name, "Work key");
    assert_eq!(view.auth_kind, "apiKey");
    assert_eq!(view.api_key_tail.as_deref(), Some("cdef"));
    assert!(!view.selected && view.switchable.enabled && !view.needs_sign_in);
    assert!(view.error.is_none() && view.quota.is_none());
    assert!(view.email.is_none() && view.workspace_id.is_none());
    assert_eq!(f.view(&a).api_key_tail, None);
    // Saved as the exact file Codex writes for a key login, and encrypted at rest.
    assert_eq!(f.saved_by_id(&key), key_file(FAKE_KEY));
    assert!(!f.vault_holds("SENTINEL"));
    reopen(&mut f, 160);
    let key = f.key_id(FAKE_KEY);
    assert_eq!(f.engine.saved.accounts.len(), 2);
    assert_eq!(f.view(&key).name, "Work key");
    // Only the last four characters cross into the snapshot, its JSON and its Debug.
    let snapshot = f.engine.snapshot(false, 300, &CodexPolicy::default());
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(json.contains(r#""apiKeyTail":"cdef""#), "{json}");
    for printed in [json, format!("{snapshot:?}")] {
        assert!(!printed.contains("SENTINEL") && !printed.contains("sk-proj"));
    }
    // The same key again changes nothing, whatever the name; another key is another account.
    f.add_key("Another name", FAKE_KEY).await.unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 2);
    assert_eq!(f.view(&key).name, "Work key");
    f.add_key("Second", "sk-proj-SENTINEL-0123456789abcdeg")
        .await
        .unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 3);
    assert_eq!(f.engine.selected_id.as_deref(), Some(a.as_str()));
    f.claude_untouched();
}

#[tokio::test]
async fn invalid_names_and_keys_are_refused_without_echo_or_change() {
    let mut f = Fixture::new();
    let before = serde_json::to_vec(&f.engine.saved).unwrap();
    let long = "n".repeat(61);
    for (name, key) in [
        ("", FAKE_KEY),
        ("   ", FAKE_KEY),
        (long.as_str(), FAKE_KEY),
        ("bad\u{7}name", FAKE_KEY),
        ("bad\nname", FAKE_KEY),
        ("Name", ""),
        ("Name", "sk-short"),
        ("Name", "pk-proj-SENTINEL-0123456789abcdef"),
        ("Name", "sk-proj-SENTINEL 0123456789abcdef"),
        ("Name", "sk-proj-SENTINEL-0123456789abcd\u{e9}"),
    ] {
        assert_eq!(
            f.add_key(name, key).await.err(),
            Some(CodexReason::InvalidApiKey),
            "{name:?}"
        );
    }
    assert_eq!(serde_json::to_vec(&f.engine.saved).unwrap(), before);
    assert_eq!(CodexReason::InvalidApiKey.to_string(), "invalidApiKey");
    // The longest accepted name is 60 characters, counted as characters.
    f.add_key(&"\u{e9}".repeat(60), FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    assert_eq!(f.view(&key).name.chars().count(), 60);
}

#[tokio::test]
async fn adding_a_key_is_blocked_like_importing_the_current_login() {
    let mut f = Fixture::new();
    let snapshot = f.engine.snapshot(false, 300, &CodexPolicy::default());
    assert!(snapshot.capabilities.add_api_key.enabled);
    f.engine.switching = Some(CodexSwitchView {
        target_id: "x".into(),
        stage: CodexSwitchStage::Restarting,
        started_at: 1,
    });
    assert_eq!(
        f.add_key("Key", FAKE_KEY).await.err(),
        Some(CodexReason::Busy)
    );
    f.engine.switching = None;
    // A configuration Codex stores outside a file is not usable.
    f.engine.installation.as_mut().unwrap().qualified.context = None;
    let snapshot = f.engine.snapshot(false, 300, &CodexPolicy::default());
    assert_eq!(
        snapshot.capabilities.add_api_key.blocked_reason,
        Some(CodexReason::UnsupportedStore)
    );
    assert_eq!(
        f.add_key("Key", FAKE_KEY).await.err(),
        Some(CodexReason::UnsupportedStore)
    );
    assert_eq!(f.engine.saved.accounts.len(), 1);
    // Not installed, unavailable vault and the read-only demo.
    let blocked = |engine: &CodexEngine| {
        engine
            .snapshot(false, 300, &CodexPolicy::default())
            .capabilities
            .add_api_key
            .blocked_reason
    };
    assert_eq!(
        blocked(&CodexEngine::empty(false)),
        Some(CodexReason::NotInstalled)
    );
    let mut unavailable = CodexEngine::empty(false);
    unavailable.unavailable();
    assert_eq!(blocked(&unavailable), Some(CodexReason::VaultUnavailable));
    let mut demo = CodexEngine::demo(1000);
    assert_eq!(blocked(&demo), Some(CodexReason::VaultUnavailable));
    assert_eq!(
        demo.command(
            CodexCommand::AddApiKey("Key".into(), Zeroizing::new(FAKE_KEY.into())),
            &f.vault,
            1000
        )
        .await
        .err(),
        Some(CodexReason::VaultUnavailable)
    );
    assert_eq!(demo.saved.accounts.len(), 3);
}

#[tokio::test]
async fn an_api_key_and_a_chatgpt_account_switch_both_ways_keeping_every_outgoing_login() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    f.add_key("Work key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    // Codex refreshed a's token before the switch: the rotation is kept.
    let rotated = auth("user-a", "ws-a", "rotated");
    f.write_active(&rotated);
    let outcome = f.switch(&key).await.unwrap();
    assert!(outcome.daemon_restarted);
    assert_eq!(outcome.error, None);
    assert_eq!(f.active(), key_file(FAKE_KEY));
    assert_eq!(f.engine.selected_id.as_deref(), Some(key.as_str()));
    assert_eq!(f.saved_bytes("ws-a"), rotated);
    assert_eq!(*f.daemon.calls.lock().unwrap(), vec!["state", "restart"]);
    assert!(f.view(&key).selected && !f.view(&key).switchable.enabled);
    assert!(f.view(&a).switchable.enabled);
    // Back to ChatGPT: the key stays saved byte for byte, nothing is duplicated.
    f.switch(&a).await.unwrap();
    assert_eq!(f.active(), rotated);
    assert_eq!(f.engine.selected_id.as_deref(), Some(a.as_str()));
    assert_eq!(f.saved_by_id(&key), key_file(FAKE_KEY));
    assert_eq!(f.engine.saved.accounts.len(), 2);
    f.switch(&key).await.unwrap();
    assert_eq!(f.active(), key_file(FAKE_KEY));
    assert_eq!(f.engine.saved.accounts.len(), 2);
    // A key has no plan limits to read, so no Codex client was ever started.
    assert!(f.factory.calls().is_empty());
    f.claude_untouched();
}

#[tokio::test]
async fn an_external_key_login_is_followed_and_never_duplicated_or_lost() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    // `codex login --with-api-key` in a terminal.
    let external: &[u8] =
        br#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-proj-SENTINEL-externalkey-000000"}"#;
    f.write_active(external);
    f.engine.observe(&f.vault, NOW).unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 2);
    let key = f.key_id("sk-proj-SENTINEL-externalkey-000000");
    assert_eq!(f.engine.selected_id.as_deref(), Some(key.as_str()));
    assert_eq!(f.saved_by_id(&key), external);
    assert_eq!(f.view(&key).name, "Codex API key");
    assert_eq!(f.view(&key).api_key_tail.as_deref(), Some("0000"));
    assert_eq!(
        f.engine.saved.return_hold_until,
        Some(NOW + MANUAL_RETURN_HOLD)
    );
    // The same key in another layout is the same account.
    let compact: &[u8] =
        br#"{"OPENAI_API_KEY":"sk-proj-SENTINEL-externalkey-000000","auth_mode":"apikey"}"#;
    f.write_active(compact);
    f.engine.observe(&f.vault, NOW + 1).unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 2);
    assert_eq!(f.saved_by_id(&key), compact);
    f.add_key("Named later", "sk-proj-SENTINEL-externalkey-000000")
        .await
        .unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 2);
    assert_eq!(f.view(&key).name, "Codex API key");
    // The ChatGPT login stays saved; switching away and back keeps the key's file.
    f.switch(&a).await.unwrap();
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
    f.switch(&key).await.unwrap();
    assert_eq!(f.active(), compact);
    // A key login the observer has not seen yet is saved as the outgoing login.
    let late: &[u8] =
        br#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-proj-SENTINEL-latekey-0000000"}"#;
    f.write_active(late);
    f.switch(&a).await.unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 3);
    let late_id = f.key_id("sk-proj-SENTINEL-latekey-0000000");
    f.switch(&late_id).await.unwrap();
    assert_eq!(f.active(), late);
    f.claude_untouched();
}

#[tokio::test]
async fn deleting_a_key_follows_the_rules_for_every_account() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    f.add_key("First", FAKE_KEY).await.unwrap();
    f.add_key("Second", "sk-proj-SENTINEL-0123456789abcdeg")
        .await
        .unwrap();
    let first = f.key_id(FAKE_KEY);
    let second = f.key_id("sk-proj-SENTINEL-0123456789abcdeg");
    // A key that is not the active login can be deleted.
    f.engine
        .command(CodexCommand::Delete(second), &f.vault, 300)
        .await
        .unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 2);
    assert_eq!(CodexEngine::load(&f.vault).saved.accounts.len(), 2);
    // The active key cannot, like the active ChatGPT account; auth.json stays as it is.
    f.switch(&first).await.unwrap();
    assert_eq!(
        f.engine
            .command(CodexCommand::Delete(first.clone()), &f.vault, 310)
            .await
            .err(),
        Some(CodexReason::ActiveAccount)
    );
    assert_eq!(f.active(), key_file(FAKE_KEY));
    assert_eq!(f.engine.saved.accounts.len(), 2);
    f.switch(&a).await.unwrap();
    f.engine
        .command(CodexCommand::Delete(first), &f.vault, 320)
        .await
        .unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 1);
    assert_eq!(f.active(), auth("user-a", "ws-a", "initial"));
}

#[test]
fn automation_never_picks_an_api_key_even_when_it_has_a_quota_and_a_return_marker() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    let b = f.add("user-b", "ws-b", "saved");
    let policy = CodexPolicy::default();
    let index = f
        .engine
        .adopt_or_import(
            OpaqueAuth::parse(key_file(FAKE_KEY)).unwrap(),
            CodexIdentityEvidence::ClaimsOnly,
            NOW,
        )
        .unwrap();
    let key = f.engine.saved.accounts[index].id.clone();
    // The key has everything a ChatGPT target would need: a fresh, unlimited reading,
    // preferences that make it the first choice and an observed spent window.
    f.read(&key, main_quota(1, 1), NOW);
    f.engine.saved.accounts[index].automation = AccountAutomation {
        priority: Some(1),
        return_on_five_hour_reset: true,
        ..AccountAutomation::default()
    };
    f.engine.saved.accounts[index].preferred_return_reset_at = Some(NOW - 10);
    f.read(&a, main_quota(97, 40), NOW);
    f.read(&b, main_quota(100, 10), NOW);
    assert!(f.view(&key).switchable.enabled);
    assert_eq!(f.next(&policy), None);
    assert!(f.order(&policy).is_empty());
    // a is at its threshold and every ChatGPT account is limited: all limited, no switch.
    assert!(matches!(
        f.engine.automation(NOW, &policy),
        CodexAutomation::Exhausted { .. }
    ));
    // With a usable ChatGPT account it is that account, never the key.
    f.read(&b, main_quota(10, 10), NOW);
    assert_eq!(f.next(&policy), Some(b.clone()));
    assert_eq!(f.engine.automation(NOW, &policy), switch_to(&f, &b, 97));
    // A natural return never targets it, and nothing is ever read for it in the background.
    f.read(&a, main_quota(20, 20), NOW);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
    assert_ne!(
        f.engine.due_quota(NOW + 100_000, &policy).as_deref(),
        Some(key.as_str())
    );
    // A prepared plan that names the key is refused before anything is written.
    let plan = CodexAutoSwitch {
        previous_id: a.clone(),
        target_id: key.clone(),
        target_name: "Key".into(),
        previous_name: "A".into(),
        peak: Some(97),
    };
    f.read(&a, main_quota(97, 40), NOW);
    let active = f.active();
    for returning in [false, true] {
        assert_eq!(
            f.engine
                .begin_automatic_switch(&plan, returning, &policy, &f.vault, NOW)
                .err(),
            Some(CodexReason::ExternalChange)
        );
    }
    assert_eq!(f.active(), active);
    assert!(f.daemon.calls.lock().unwrap().is_empty());
    // Once a key is the active login, automation leaves it alone, whatever else is ready.
    f.engine.saved.accounts[index].quota = None;
    f.engine.selected_id = Some(key);
    assert_eq!(f.engine.automation(NOW, &policy), CodexAutomation::Idle);
}

#[tokio::test]
async fn refreshing_an_api_key_reads_nothing_and_leaves_no_error_or_sign_in_state() {
    let mut f = Fixture::new();
    let a = f.id_of("ws-a");
    f.add_key("Work key", FAKE_KEY).await.unwrap();
    let key = f.key_id(FAKE_KEY);
    // "Refresh all" reads the ChatGPT account only.
    f.factory
        .enqueue(Plan::quota(Ok(quota(Some("ws-a"), 20, 30))));
    let (factory, daemon) = (f.factory.clone(), f.daemon.clone());
    let (_temp, handle) = runtime(&mut f, Settings::default()).await;
    // Through the runtime handle, like the desktop commands.
    let snapshot = handle
        .codex_add_api_key("Second key", "sk-proj-SENTINEL-0123456789abcdeg".into())
        .await
        .unwrap();
    assert_eq!(snapshot.accounts.len(), 3);
    assert_eq!(snapshot.selected_id.as_deref(), Some(a.as_str()));
    handle.codex_refresh_account(&key).await.unwrap();
    let snapshot = handle.codex_refresh_all().await.unwrap();
    assert_eq!(factory.calls(), vec!["start-Active", "quota", "shutdown"]);
    assert!(snapshot.error.is_none());
    // Switch to the key: the follow-up reading of the new active account is skipped too.
    let snapshot = handle.codex_switch_account(&key).await.unwrap();
    assert_eq!(snapshot.selected_id.as_deref(), Some(key.as_str()));
    assert_eq!(f.active(), key_file(FAKE_KEY));
    assert_eq!(*daemon.calls.lock().unwrap(), vec!["state", "restart"]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle.codex_refresh_account(&key).await.unwrap();
    handle.codex_tick().await;
    let snapshot = handle.get_codex_snapshot();
    assert!(snapshot.error.is_none());
    assert_eq!(factory.calls().len(), 3);
    let view = snapshot.accounts.iter().find(|v| v.id == key).unwrap();
    assert!(view.selected && view.error.is_none() && !view.needs_sign_in);
    assert!(view.quota.is_none() && view.quota_read_at.is_none());
    assert_eq!(view.api_key_tail.as_deref(), Some("cdef"));
    // A key is never read in the background either.
    handle
        .edit_codex_for_test(|engine, _| {
            assert_ne!(
                engine
                    .due_quota(NOW + 1_000_000, &CodexPolicy::default())
                    .as_deref(),
                Some(key.as_str())
            );
        })
        .await;
}
