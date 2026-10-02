//! Hermetic runtime integration fixtures: no real homes, processes or providers.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use std::{
    collections::VecDeque,
    fs,
    io::Write,
    sync::{Mutex, atomic::AtomicBool},
};

fn auth(user: &str, workspace: &str, revision: &str) -> Vec<u8> {
    let claims = json!({"https://api.openai.com/auth":{
        "chatgpt_user_id":user,"chatgpt_account_id":workspace,"chatgpt_plan_type":"plus"}});
    let jwt = format!(
        "header.{}.signature-secret-sentinel",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    );
    let mut bytes = serde_json::to_vec_pretty(&json!({"auth_mode":"chatgpt","OPENAI_API_KEY":null,
        "tokens":{"id_token":jwt,"access_token":format!("access-secret-sentinel-{revision}"),
            "refresh_token":format!("refresh-secret-sentinel-{revision}"),"account_id":workspace},
        "future_extension":{"revision":revision,"opaque":[1,{"keep":true}]}}))
    .unwrap();
    bytes.push(b'\n');
    bytes
}
fn quota(workspace: Option<&str>, ordinary: Option<bool>) -> CodexRateLimits {
    serde_json::from_value(json!({"accountId":workspace,"ordinaryUsageAllowed":ordinary,
        "rateLimits":{"limitId":"main","primary":{"usedPercent":71,"windowDurationMins":300,"resetsAt":9000},
            "secondary":null,"planType":"plus","credits":{"hasCredits":true,"unlimited":false,"balance":"12.345"}},
        "rateLimitsByLimitId":{"default":{"limitId":"native-default-collision","primary":null,"secondary":null},
            "model-extra":{"limitId":"model-extra","primary":null,
            "secondary":{"usedPercent":4,"windowDurationMins":null,"resetsAt":null}}},
        "rateLimitResetCredits":{"availableCount":3,"credits":null}})).unwrap()
}
fn default_inspection() -> ConfigurationInspection {
    ConfigurationInspection::from_response_bytes(br#"{"config":{"cli_auth_credentials_store":"file","model_provider":"openai","model":"fixture-model"},"origins":{},"layers":[]}"#,
        br#"{"requirements":null}"#).unwrap()
}

struct Plan {
    observation: Result<AccountObservation, CodexError>,
    quota: Result<CodexRateLimits, CodexError>,
    login: LoginPoll,
    poll_error: Option<CodexError>,
    login_auth: Option<Vec<u8>>,
    rotate_quota: Option<Vec<u8>>,
    rotate_shutdown: Option<Vec<u8>>,
    cancel_intent_during_quota: Option<Arc<AtomicU64>>,
}
impl Plan {
    fn good(workspace: &str) -> Self {
        Self {
            observation: Ok(AccountObservation {
                account: Some(CodexAccount::Chatgpt {
                    email: None,
                    plan_type: "plus".into(),
                }),
                requires_openai_auth: true,
            }),
            quota: Ok(quota(Some(workspace), Some(true))),
            login: LoginPoll::Completed,
            poll_error: None,
            login_auth: None,
            rotate_quota: None,
            rotate_shutdown: None,
            cancel_intent_during_quota: None,
        }
    }
}
#[derive(Default)]
struct FakeFactory {
    plans: Mutex<VecDeque<Plan>>,
    calls: Arc<Mutex<Vec<&'static str>>>,
}
impl FakeFactory {
    fn enqueue(&self, plan: Plan) {
        self.plans.lock().unwrap().push_back(plan);
    }
}
#[async_trait]
impl ClientFactory for FakeFactory {
    async fn start(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        context: CodexContext,
        login: bool,
    ) -> Result<Box<dyn CodexService>, CodexError> {
        assert!(
            executable.is_none(),
            "fixtures never execute a native Codex binary"
        );
        self.calls
            .lock()
            .unwrap()
            .push(if login { "start-login" } else { "start-active" });
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
    calls: Arc<Mutex<Vec<&'static str>>>,
}
#[async_trait]
impl CodexService for FakeClient {
    async fn inspect_configuration(&mut self) -> Result<ConfigurationInspection, CodexError> {
        self.calls.lock().unwrap().push("inspect");
        Ok(default_inspection())
    }
    async fn begin_browser_login(&mut self) -> Result<BrowserLoginChallenge, CodexError> {
        self.calls.lock().unwrap().push("begin");
        BrowserLoginChallenge::from_authorization_url(
            "https://auth.openai.com/oauth/authorize?fixture-secret-sentinel".into(),
        )
    }
    fn poll_login(&mut self) -> Result<LoginPoll, CodexError> {
        self.calls.lock().unwrap().push("poll");
        if let Some(error) = self.plan.poll_error.take() {
            return Err(error);
        }
        if self.plan.login == LoginPoll::Completed
            && let Some(bytes) = self.plan.login_auth.take()
        {
            fs::write(self.home.join("auth.json"), bytes).unwrap();
        }
        Ok(self.plan.login)
    }
    async fn cancel_login(&mut self) -> Result<CancelOutcome, CodexError> {
        self.calls.lock().unwrap().push("cancel");
        Ok(CancelOutcome::Cancelled)
    }
    async fn read_account(&mut self) -> Result<AccountObservation, CodexError> {
        self.calls.lock().unwrap().push("account");
        self.plan.observation.clone()
    }
    async fn read_rate_limits(&mut self) -> Result<CodexRateLimits, CodexError> {
        self.calls.lock().unwrap().push("quota");
        if let Some(bytes) = self.plan.rotate_quota.take() {
            fs::write(self.home.join("auth.json"), bytes).unwrap();
        }
        if let Some(intent) = &self.plan.cancel_intent_during_quota {
            intent.fetch_add(1, Ordering::AcqRel);
        }
        self.plan.quota.clone()
    }
    async fn shutdown(&mut self) -> Result<(), CodexError> {
        self.calls.lock().unwrap().push("shutdown");
        if let Some(bytes) = self.plan.rotate_shutdown.take() {
            fs::write(self.home.join("auth.json"), bytes).unwrap();
        }
        Ok(())
    }
}
#[derive(Default)]
struct FakeGuard {
    running: AtomicBool,
    unknown: AtomicBool,
}
impl CodexWriteGuard for FakeGuard {
    fn check(&self, _: &CodexFileContext) -> CodexResult<()> {
        if self.unknown.load(Ordering::Acquire) {
            return Err(CodexStoreError::ProcessVisibility);
        }
        if self.running.load(Ordering::Acquire) {
            return Err(CodexStoreError::RunningClients);
        }
        Ok(())
    }
}
struct Fixture {
    _root: TempDir,
    engine: CodexEngine,
    vault: Vault,
    factory: Arc<FakeFactory>,
    guard: Arc<FakeGuard>,
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
        fs::write(
            home.join("auth.json"),
            auth("user-a", "workspace-a", "initial"),
        )
        .unwrap();
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
        let qualified = paths.qualify(&executable, workspace.path()).unwrap();
        let factory = Arc::new(FakeFactory::default());
        let guard = Arc::new(FakeGuard::default());
        let mut engine = CodexEngine::empty(false);
        engine.factory = factory.clone();
        engine.guard = guard.clone();
        engine.installation = Some(Installation {
            executable: None,
            executable_path: executable,
            paths,
            workspace,
            qualified,
        });
        engine.availability = CodexAvailability::Supported;
        engine.observe_current().unwrap();
        let vault = Vault::with_key(root.path().join("vault"), [63; 32]).unwrap();
        vault
            .save(
                "runtime-state",
                &json!({"historicalClaude":"claude-secret-sentinel","settings":{"language":"ro"}}),
            )
            .unwrap();
        let claude_path = root.path().join("vault/runtime-state.vault");
        let claude_ciphertext = fs::read(&claude_path).unwrap();
        Self {
            _root: root,
            engine,
            vault,
            factory,
            guard,
            home,
            claude_ciphertext,
            claude_path,
        }
    }
    fn active(&self) -> Vec<u8> {
        fs::read(self.home.join("auth.json")).unwrap()
    }
    fn claude_untouched(&self) {
        assert_eq!(fs::read(&self.claude_path).unwrap(), self.claude_ciphertext);
    }
    fn import(&mut self) -> String {
        self.engine.import_current(&self.vault).unwrap();
        self.engine.selected_id.clone().unwrap()
    }
    fn add_managed(&mut self, user: &str, workspace: &str) -> String {
        let index = self
            .engine
            .upsert(
                OpaqueAuth::parse(auth(user, workspace, "saved")).unwrap(),
                CodexIdentityEvidence::ManagedLogin,
                true,
            )
            .unwrap();
        self.engine.save(&self.vault).unwrap();
        self.engine.saved.accounts[index].id.clone()
    }
    fn reloaded(&self) -> Saved {
        self.vault.load(RECORD).unwrap().unwrap()
    }
}

#[test]
fn opaque_import_deduplicates_owner_and_preserves_api_mode_without_switching_it() {
    let mut f = Fixture::new();
    let first = f.active();
    let id = f.import();
    assert_eq!(f.engine.saved.accounts[0].auth, first);
    assert_eq!(
        f.engine.saved.accounts[0].evidence,
        CodexIdentityEvidence::ClaimsOnly
    );
    assert!(
        !f.engine.snapshot(false, 100).accounts[0]
            .manual_switch
            .enabled
    );
    let updated = auth("user-a", "workspace-a", "import-rotation");
    fs::write(f.home.join("auth.json"), &updated).unwrap();
    assert_eq!(f.import(), id);
    assert_eq!(f.engine.saved.accounts.len(), 1);
    assert_eq!(f.reloaded().accounts[0].auth, updated);
    let api=br#"{ "auth_mode":"apikey","OPENAI_API_KEY":"api-key-secret-sentinel","future_extension":{"keep":true}}"#;
    fs::write(f.home.join("auth.json"), api).unwrap();
    let api_id = f.import();
    assert_ne!(id, api_id);
    assert_eq!(f.engine.saved.accounts.len(), 2);
    let api_view = f
        .engine
        .snapshot(false, 100)
        .accounts
        .into_iter()
        .find(|a| a.id == api_id)
        .unwrap();
    assert_eq!(api_view.auth_kind, "apiKey");
    assert_eq!(
        api_view.manual_switch.blocked_reason,
        Some(CodexReason::UnsupportedAuth)
    );
    assert_eq!(f.reloaded().accounts[1].auth, api);
    let bad = br#"{"auth_mode":"apikey","OPENAI_API_KEY":"api-secret","tokens":{}}"#;
    fs::write(f.home.join("auth.json"), bad).unwrap();
    assert_eq!(
        f.engine.import_current(&f.vault).err(),
        Some(CodexReason::UnsupportedAuth)
    );
    assert_eq!(f.active(), bad);
    let dto = serde_json::to_string(&f.engine.snapshot(false, 100)).unwrap();
    for secret in [
        "secret-sentinel",
        "id_token",
        "access_token",
        "refresh_token",
        "future_extension",
        "auth.json",
    ] {
        assert!(!dto.contains(secret));
    }
    f.claude_untouched();
}
#[tokio::test]
async fn canceled_owned_login_leaves_active_auth_and_claude_ciphertext_unchanged() {
    let mut f = Fixture::new();
    let active = f.active();
    let mut plan = Plan::good("workspace-b");
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
    assert_ne!(owned, f.home);
    assert!(!format!("{launch:?}").contains("secret-sentinel"));
    f.engine.cancel_login(&launch.session.id).await.unwrap();
    assert!(f.engine.pending_login.is_none());
    assert!(!owned.exists());
    assert!(f.engine.saved.accounts.is_empty());
    assert_eq!(
        f.engine.login_view.as_ref().unwrap().error,
        Some(CodexReason::LoginCanceled)
    );
    assert_eq!(f.active(), active);
    assert!(
        f.factory
            .calls
            .lock()
            .unwrap()
            .ends_with(&["cancel", "shutdown"])
    );
    f.claude_untouched();
}
#[tokio::test]
async fn managed_login_cannot_change_owner_during_quota_or_shutdown() {
    for during_shutdown in [false, true] {
        let mut f = Fixture::new();
        let active = f.active();
        let mut plan = Plan::good("workspace-b");
        plan.login_auth = Some(auth("user-b", "workspace-b", "login"));
        if during_shutdown {
            plan.rotate_shutdown = Some(auth("user-c", "workspace-c", "replacement"));
        } else {
            plan.rotate_quota = Some(auth("user-c", "workspace-c", "replacement"));
        }
        f.factory.enqueue(plan);
        let launch = f.engine.begin_login(0).await.unwrap();
        assert_eq!(
            f.engine
                .poll_login(&launch.session.id, &f.vault, 100)
                .await
                .err(),
            Some(CodexReason::IdentityMismatch)
        );
        assert!(f.engine.saved.accounts.is_empty());
        assert_eq!(
            f.engine.login_view.as_ref().unwrap().status,
            CodexLoginStatus::Failed
        );
        assert_eq!(f.active(), active);
        assert_eq!(f.factory.calls.lock().unwrap().last(), Some(&"shutdown"));
        f.claude_untouched();
    }
}
#[tokio::test]
async fn late_login_cancellation_is_checked_before_any_saved_account_commit() {
    let mut f = Fixture::new();
    let mut plan = Plan::good("workspace-b");
    plan.login_auth = Some(auth("user-b", "workspace-b", "login"));
    plan.cancel_intent_during_quota = Some(f.engine.intent.clone());
    f.factory.enqueue(plan);
    let launch = f.engine.begin_login(0).await.unwrap();
    assert_eq!(
        f.engine
            .poll_login(&launch.session.id, &f.vault, 100)
            .await
            .err(),
        Some(CodexReason::LoginCanceled)
    );
    assert!(f.engine.saved.accounts.is_empty());
    assert!(f.vault.load::<Saved>(RECORD).unwrap().is_none());
    f.claude_untouched();
}
#[tokio::test]
async fn managed_login_missing_quota_proof_retains_managed_evidence_without_verified_quota() {
    for missing_workspace in [false, true] {
        let mut f = Fixture::new();
        let mut plan = Plan::good("workspace-b");
        plan.login_auth = Some(auth("user-b", "workspace-b", "login"));
        plan.quota = Ok(if missing_workspace {
            quota(None, Some(true))
        } else {
            quota(Some("workspace-b"), None)
        });
        f.factory.enqueue(plan);
        let launch = f.engine.begin_login(0).await.unwrap();
        f.engine
            .poll_login(&launch.session.id, &f.vault, 100)
            .await
            .unwrap();
        let snapshot = f.engine.snapshot(false, 100);
        assert_eq!(snapshot.accounts.len(), 1);
        assert_eq!(
            snapshot.accounts[0].identity_evidence,
            CodexIdentityEvidence::ManagedLogin
        );
        assert!(!snapshot.accounts[0].identity_verified);
        assert!(snapshot.accounts[0].quota.is_none());
        f.claude_untouched();
    }
}
#[tokio::test]
async fn quota_requires_independent_workspace_and_ordinary_usage_proof() {
    for case in 0..3 {
        let mut f = Fixture::new();
        let id = f.import();
        let mut plan = Plan::good("workspace-a");
        plan.quota = Ok(match case {
            0 => quota(Some("workspace-other"), Some(true)),
            1 => quota(Some("workspace-a"), None),
            _ => quota(None, Some(true)),
        });
        f.factory.enqueue(plan);
        assert_eq!(
            f.engine.refresh(&id, &f.vault, 100).await.err(),
            Some(CodexReason::IdentityMismatch)
        );
        let snapshot = f.engine.snapshot(false, 100);
        assert!(!snapshot.accounts[0].identity_verified);
        assert!(snapshot.accounts[0].quota.is_none());
        assert_eq!(
            f.reloaded().accounts[0].evidence,
            CodexIdentityEvidence::ClaimsOnly
        );
        f.claude_untouched();
    }
}
#[tokio::test]
async fn corroborated_multi_bucket_quota_keeps_nullable_windows_and_credit_precision() {
    let mut f = Fixture::new();
    let id = f.import();
    let mut plan = Plan::good("workspace-a");
    plan.quota = Ok(quota(Some("workspace-a"), Some(false)));
    f.factory.enqueue(plan);
    f.engine.refresh(&id, &f.vault, 100).await.unwrap();
    let snapshot = f.engine.snapshot(false, 100);
    assert!(snapshot.accounts[0].identity_verified);
    assert_eq!(snapshot.accounts[0].quota_state, CodexQuotaState::Fresh);
    let quotas = snapshot.accounts[0].quota.as_ref().unwrap();
    assert_eq!(quotas.ordinary_usage_allowed, Some(false));
    assert_eq!(quotas.limits.len(), 3);
    assert_eq!(
        quotas.limits[0]
            .credits
            .as_ref()
            .unwrap()
            .balance
            .as_deref(),
        Some("12.345")
    );
    assert!(quotas.limits[0].secondary.is_none());
    let extra = quotas
        .limits
        .iter()
        .find(|q| q.key == "bucket:model-extra")
        .unwrap();
    assert_eq!(extra.limit_id.as_deref(), Some("model-extra"));
    assert!(
        quotas.limits.iter().any(|q| q.key == "bucket:default"
            && q.limit_id.as_deref() == Some("native-default-collision"))
    );
    assert_eq!(
        quotas
            .limits
            .iter()
            .map(|q| &q.key)
            .collect::<BTreeSet<_>>()
            .len(),
        quotas.limits.len()
    );
    assert!(extra.primary.is_none());
    assert!(
        extra
            .secondary
            .as_ref()
            .unwrap()
            .window_duration_mins
            .is_none()
    );
    assert_eq!(quotas.reset_credits_available, Some(3));
    let dto = serde_json::to_string(&snapshot).unwrap();
    assert!(!dto.contains("secret-sentinel"));
    assert!(!dto.contains("https://api.openai.com/auth"));
    f.claude_untouched();
}
#[tokio::test]
async fn failed_provider_request_still_durably_adopts_owner_bound_rotation() {
    let mut f = Fixture::new();
    let id = f.import();
    let rotated = auth("user-a", "workspace-a", "rotated-after-failure");
    let mut plan = Plan::good("workspace-a");
    plan.rotate_quota = Some(rotated.clone());
    plan.quota = Err(CodexError::Timeout);
    f.factory.enqueue(plan);
    assert_eq!(
        f.engine.refresh(&id, &f.vault, 100).await.err(),
        Some(CodexReason::ProviderUnavailable)
    );
    assert_eq!(f.active(), rotated);
    assert_eq!(f.reloaded().accounts[0].auth, rotated);
    let snapshot = f.engine.snapshot(false, 100);
    assert!(!snapshot.accounts[0].identity_verified);
    assert_eq!(
        snapshot.accounts[0].quota_state,
        CodexQuotaState::Unavailable
    );
    assert_eq!(
        snapshot.accounts[0].error,
        Some(CodexReason::ProviderUnavailable)
    );
    f.claude_untouched();
}
#[tokio::test]
async fn changed_owner_after_owned_request_is_preserved_and_never_saved_as_previous_account() {
    let mut f = Fixture::new();
    let id = f.import();
    let original = f.reloaded().accounts[0].auth.clone();
    let replacement = auth("user-b", "workspace-b", "unexpected");
    let mut plan = Plan::good("workspace-a");
    plan.rotate_shutdown = Some(replacement.clone());
    f.factory.enqueue(plan);
    assert_eq!(
        f.engine.refresh(&id, &f.vault, 100).await.err(),
        Some(CodexReason::IdentityMismatch)
    );
    assert_eq!(f.active(), replacement);
    assert_eq!(f.reloaded().accounts[0].auth, original);
    assert!(!f.engine.verified_now.contains(&id));
    assert!(
        f.engine.selected_id.is_none(),
        "selected state must not claim the previous owner after the active owner changes"
    );
    f.claude_untouched();
}
#[tokio::test]
async fn manual_preparation_cancel_and_apply_preserve_outgoing_and_adopt_incoming_rotation() {
    let mut f = Fixture::new();
    let outgoing_id = f.import();
    let active = f.active();
    let incoming_id = f.add_managed("user-b", "workspace-b");
    let preparation = f.engine.prepare(&incoming_id, &f.vault, 100).unwrap();
    assert_eq!(f.active(), active);
    assert_eq!(
        f.engine
            .apply(&preparation.id, false, &f.vault, 101)
            .await
            .err(),
        Some(CodexReason::InvalidPreparation)
    );
    assert_eq!(f.active(), active);
    f.engine
        .command(
            CodexCommand::CancelPreparation(preparation.id),
            &f.vault,
            101,
        )
        .await
        .unwrap();
    assert_eq!(f.active(), active);
    assert!(f.engine.preparation.is_none());
    let preparation = f.engine.prepare(&incoming_id, &f.vault, 102).unwrap();
    let rotated = auth("user-b", "workspace-b", "new-selection-rotation");
    let mut plan = Plan::good("workspace-b");
    plan.rotate_quota = Some(rotated.clone());
    f.factory.enqueue(plan);
    f.engine
        .apply(&preparation.id, true, &f.vault, 103)
        .await
        .unwrap();
    assert_eq!(f.active(), rotated);
    assert_eq!(f.engine.selected_id.as_deref(), Some(incoming_id.as_str()));
    assert!(!f.engine.reconciling);
    assert!(f.engine.pending_switch.is_none());
    assert!(matches!(
        f.engine.store().unwrap().recover(&f.vault).unwrap(),
        CodexRecovery::Clean
    ));
    let saved = f.reloaded();
    assert_eq!(
        saved
            .accounts
            .iter()
            .find(|a| a.id == outgoing_id)
            .unwrap()
            .auth,
        active
    );
    assert_eq!(
        saved
            .accounts
            .iter()
            .find(|a| a.id == incoming_id)
            .unwrap()
            .auth,
        rotated
    );
    f.claude_untouched();
}
#[tokio::test]
async fn manual_selection_rejects_external_same_byte_generation_and_process_visibility_failures() {
    for case in 0..3 {
        let mut f = Fixture::new();
        f.import();
        let incoming = f.add_managed("user-b", "workspace-b");
        let active = f.active();
        let preparation = f.engine.prepare(&incoming, &f.vault, 100).unwrap();
        let expected = match case {
            0 => {
                let mut replacement = tempfile::NamedTempFile::new_in(&f.home).unwrap();
                replacement.write_all(&active).unwrap();
                replacement.persist(f.home.join("auth.json")).unwrap();
                CodexReason::ExternalChange
            }
            1 => {
                f.guard.running.store(true, Ordering::Release);
                CodexReason::ClientsRunning
            }
            _ => {
                f.guard.unknown.store(true, Ordering::Release);
                CodexReason::ProcessInventoryUnavailable
            }
        };
        assert_eq!(
            f.engine
                .apply(&preparation.id, true, &f.vault, 101)
                .await
                .err(),
            Some(expected)
        );
        assert_eq!(f.active(), active);
        assert!(f.engine.reconciling);
        assert!(f.factory.calls.lock().unwrap().is_empty());
        f.claude_untouched();
    }
}
#[tokio::test]
async fn interrupted_switch_recovery_verifies_new_owner_and_finishes_without_stale_rollback() {
    let mut f = Fixture::new();
    f.import();
    let incoming = f.add_managed("user-b", "workspace-b");
    let outgoing = f.active();
    let store = f.engine.store().unwrap();
    let before = store.read().unwrap();
    let bytes = f
        .engine
        .saved
        .accounts
        .iter()
        .find(|a| a.id == incoming)
        .unwrap()
        .auth
        .clone();
    let prepared = store
        .prepare_switch(&bytes, &before.generation, &f.vault)
        .unwrap();
    store.commit_file(&prepared, &f.vault).unwrap();
    assert_ne!(f.active(), outgoing);
    f.engine.pending_switch = None;
    f.engine.recover(&f.vault).unwrap();
    f.engine.observe_current().unwrap();
    assert!(f.engine.reconciling);
    assert_eq!(f.engine.selected_id.as_deref(), Some(incoming.as_str()));
    let mut plan = Plan::good("workspace-b");
    let rotated = auth("user-b", "workspace-b", "recovered-rotation");
    plan.rotate_shutdown = Some(rotated.clone());
    f.factory.enqueue(plan);
    f.engine.refresh(&incoming, &f.vault, 200).await.unwrap();
    assert_eq!(f.active(), rotated);
    assert!(!f.engine.reconciling);
    assert!(matches!(
        f.engine.store().unwrap().recover(&f.vault).unwrap(),
        CodexRecovery::Clean
    ));
    assert_eq!(
        f.reloaded()
            .accounts
            .iter()
            .find(|a| a.id == incoming)
            .unwrap()
            .auth,
        rotated
    );
    f.claude_untouched();
}

#[tokio::test]
async fn vault_failure_after_rotation_preserves_active_bytes_ciphertext_and_pending_journal() {
    let mut f = Fixture::new();
    f.import();
    let incoming = f.add_managed("user-b", "workspace-b");
    let preparation = f.engine.prepare(&incoming, &f.vault, 100).unwrap();
    let record = f._root.path().join("vault/codex-state.vault");
    let mut damaged = fs::read(&record).unwrap();
    damaged[40] ^= 1;
    fs::write(&record, &damaged).unwrap();
    let rotated = auth("user-b", "workspace-b", "vault-save-failure");
    let mut plan = Plan::good("workspace-b");
    plan.rotate_quota = Some(rotated.clone());
    f.factory.enqueue(plan);
    assert_eq!(
        f.engine
            .command(CodexCommand::Apply(preparation.id, true), &f.vault, 101)
            .await
            .err(),
        Some(CodexReason::VaultUnavailable)
    );
    assert_eq!(f.active(), rotated);
    assert_eq!(fs::read(record).unwrap(), damaged);
    assert!(f.engine.pending_switch.is_some());
    assert!(f.engine.reconciling);
    assert!(!f.engine.writable);
    assert!(f.engine.verified_now.is_empty());
    let snapshot = f.engine.snapshot(false, 101);
    assert_eq!(snapshot.blocked_reason, Some(CodexReason::VaultUnavailable));
    assert!(!snapshot.capabilities.manual_switch.enabled);
    assert!(
        f._root
            .path()
            .join("vault")
            .read_dir()
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("codex-journal-"))
    );
    assert!(matches!(
        f.engine.store().unwrap().recover(&f.vault),
        Err(CodexStoreError::ExternalChange)
    ));
    f.claude_untouched();
}
#[tokio::test]
async fn changed_external_import_cannot_inherit_historical_managed_or_backend_trust() {
    let mut f = Fixture::new();
    let id = f.add_managed("user-a", "workspace-a");
    f.engine.observe_current().unwrap();
    f.factory.enqueue(Plan::good("workspace-a"));
    f.engine.refresh(&id, &f.vault, 100).await.unwrap();
    assert!(f.engine.verified_now.contains(&id));
    assert!(f.engine.saved.accounts[0].managed_origin);
    let external = auth("user-a", "workspace-a", "external-import");
    fs::write(f.home.join("auth.json"), &external).unwrap();
    f.engine
        .command(CodexCommand::ImportCurrent, &f.vault, 101)
        .await
        .unwrap();
    assert_eq!(f.engine.saved.accounts.len(), 1);
    assert_eq!(f.reloaded().accounts[0].auth, external);
    assert_eq!(
        f.reloaded().accounts[0].evidence,
        CodexIdentityEvidence::ClaimsOnly
    );
    assert!(!f.engine.verified_now.contains(&id));
    let snapshot = f.engine.snapshot(false, 101);
    assert!(!snapshot.accounts[0].manual_switch.enabled);
    assert_eq!(snapshot.accounts[0].quota_state, CodexQuotaState::Cached);
    assert!(!snapshot.accounts[0].identity_verified);
    f.claude_untouched();
}
#[tokio::test]
async fn snapshot_is_cache_only_even_when_active_auth_changes_and_provider_plans_wait() {
    let mut f = Fixture::new();
    let id = f.import();
    f.factory.enqueue(Plan::good("workspace-a"));
    f.engine.refresh(&id, &f.vault, 100).await.unwrap();
    let before = serde_json::to_value(f.engine.snapshot(false, 100)).unwrap();
    let calls = f.factory.calls.lock().unwrap().len();
    f.factory.enqueue(Plan::good("workspace-a"));
    fs::write(f.home.join("auth.json"), b"corrupt-secret-sentinel").unwrap();
    for _ in 0..4 {
        assert_eq!(
            serde_json::to_value(f.engine.snapshot(false, 100)).unwrap(),
            before
        );
    }
    assert_eq!(f.factory.calls.lock().unwrap().len(), calls);
    assert_eq!(f.factory.plans.lock().unwrap().len(), 1);
    f.claude_untouched();
}

#[tokio::test]
async fn explicit_current_import_then_backend_proof_resolves_external_switch_conflict() {
    let mut f = Fixture::new();
    f.import();
    let incoming = f.add_managed("user-b", "workspace-b");
    let preparation = f.engine.prepare(&incoming, &f.vault, 100).unwrap();
    let external = auth("user-c", "workspace-c", "external-current");
    fs::write(f.home.join("auth.json"), &external).unwrap();
    assert_eq!(
        f.engine
            .command(CodexCommand::Apply(preparation.id, true), &f.vault, 101)
            .await
            .err(),
        Some(CodexReason::ExternalChange)
    );
    assert!(f.engine.preparation.is_none());
    assert!(f.engine.reconciling);
    assert_eq!(f.active(), external);
    let journal = f._root.path().join("vault").join(format!(
        "codex-journal-{}.vault",
        f.engine.store().unwrap().context().context_id()
    ));
    let pending_ciphertext = fs::read(&journal).unwrap();
    f.engine
        .command(CodexCommand::ImportCurrent, &f.vault, 102)
        .await
        .unwrap();
    let current_id = f.engine.selected_id.clone().unwrap();
    assert!(f.engine.external_reconciliation);
    assert!(f.engine.reconciling);
    assert_eq!(fs::read(&journal).unwrap(), pending_ciphertext);
    assert_eq!(f.active(), external);
    assert_eq!(
        f.engine
            .saved
            .accounts
            .iter()
            .find(|a| a.id == current_id)
            .unwrap()
            .evidence,
        CodexIdentityEvidence::ClaimsOnly
    );
    // Missing ordinary-usage proof may save an owner-bound rotation but cannot retire the journal.
    let mut unverified = Plan::good("workspace-c");
    unverified.quota = Ok(quota(Some("workspace-c"), None));
    f.factory.enqueue(unverified);
    assert_eq!(
        f.engine
            .command(CodexCommand::Refresh(current_id.clone()), &f.vault, 103)
            .await
            .err(),
        Some(CodexReason::IdentityMismatch)
    );
    assert_eq!(fs::read(&journal).unwrap(), pending_ciphertext);
    assert!(f.engine.external_reconciliation);
    assert!(f.engine.reconciling);
    assert!(!f.engine.verified_now.contains(&current_id));
    assert_eq!(f.active(), external);
    let rotated = auth("user-c", "workspace-c", "verified-external-rotation");
    let mut verified = Plan::good("workspace-c");
    verified.rotate_shutdown = Some(rotated.clone());
    f.factory.enqueue(verified);
    f.engine
        .command(CodexCommand::Refresh(current_id.clone()), &f.vault, 104)
        .await
        .unwrap();
    assert_eq!(f.active(), rotated);
    assert!(!journal.exists());
    assert!(!f.engine.external_reconciliation);
    assert!(!f.engine.reconciling);
    assert!(f.engine.verified_now.contains(&current_id));
    assert_eq!(
        f.reloaded()
            .accounts
            .iter()
            .find(|a| a.id == current_id)
            .unwrap()
            .auth,
        rotated
    );
    assert!(matches!(
        f.engine.store().unwrap().recover(&f.vault).unwrap(),
        CodexRecovery::Clean
    ));
    f.claude_untouched();
}
#[tokio::test]
async fn explicit_conflict_import_never_replaces_a_damaged_journal() {
    let mut f = Fixture::new();
    f.import();
    let incoming = f.add_managed("user-b", "workspace-b");
    let store = f.engine.store().unwrap();
    let before = store.read().unwrap();
    let bytes = f
        .engine
        .saved
        .accounts
        .iter()
        .find(|a| a.id == incoming)
        .unwrap()
        .auth
        .clone();
    store
        .prepare_switch(&bytes, &before.generation, &f.vault)
        .unwrap();
    let external = auth("user-c", "workspace-c", "current-with-damaged-journal");
    fs::write(f.home.join("auth.json"), &external).unwrap();
    let journal = f._root.path().join("vault").join(format!(
        "codex-journal-{}.vault",
        store.context().context_id()
    ));
    let mut damaged = fs::read(&journal).unwrap();
    damaged[40] ^= 1;
    fs::write(&journal, &damaged).unwrap();
    let accounts = f.engine.saved.accounts.len();
    assert_eq!(
        f.engine
            .command(CodexCommand::ImportCurrent, &f.vault, 102)
            .await
            .err(),
        Some(CodexReason::StoreConflict)
    );
    assert_eq!(fs::read(journal).unwrap(), damaged);
    assert_eq!(f.active(), external);
    assert_eq!(f.engine.saved.accounts.len(), accounts);
    assert!(!f.engine.external_reconciliation);
    assert!(f.engine.reconciling);
    assert!(f.factory.calls.lock().unwrap().is_empty());
    f.claude_untouched();
}

#[tokio::test]
async fn login_timeout_and_disconnect_close_owned_context_and_allow_a_new_attempt() {
    for (error, expected) in [
        (CodexError::Timeout, CodexReason::LoginExpired),
        (CodexError::ChildExited, CodexReason::ProviderUnavailable),
    ] {
        let mut f = Fixture::new();
        f.import();
        let active = f.active();
        let state_path = f._root.path().join("vault/codex-state.vault");
        let saved_ciphertext = fs::read(&state_path).unwrap();
        let mut plan = Plan::good("workspace-b");
        plan.poll_error = Some(error);
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
        assert!(owned.exists());
        assert_eq!(
            f.engine
                .command(
                    CodexCommand::PollLogin(launch.session.id.clone()),
                    &f.vault,
                    101
                )
                .await
                .err(),
            Some(expected)
        );
        assert!(f.engine.pending_login.is_none());
        assert!(!owned.exists());
        assert_eq!(f.active(), active);
        assert_eq!(fs::read(&state_path).unwrap(), saved_ciphertext);
        assert_eq!(f.engine.saved.accounts.len(), 1);
        let snapshot = f.engine.snapshot(false, 101);
        let login = snapshot.login.unwrap();
        assert_eq!(login.id, launch.session.id);
        assert_eq!(login.status, CodexLoginStatus::Failed);
        assert_eq!(login.error, Some(expected));
        assert!(
            f.factory
                .calls
                .lock()
                .unwrap()
                .ends_with(&["poll", "shutdown"])
        );
        let calls = f.factory.calls.lock().unwrap().len();
        assert_eq!(
            f.engine
                .poll_login(&launch.session.id, &f.vault, 102)
                .await
                .err(),
            Some(CodexReason::LoginExpired)
        );
        assert_eq!(f.factory.calls.lock().unwrap().len(), calls);
        let mut retry = Plan::good("workspace-b");
        retry.login = LoginPoll::Pending;
        f.factory.enqueue(retry);
        let next = f.engine.begin_login(0).await.unwrap();
        assert_ne!(next.session.id, launch.session.id);
        f.engine.cancel_login(&next.session.id).await.unwrap();
        f.claude_untouched();
    }
}
#[tokio::test]
async fn normal_pending_login_keeps_owned_context_until_explicit_cancellation() {
    let mut f = Fixture::new();
    f.import();
    let active = f.active();
    let state_path = f._root.path().join("vault/codex-state.vault");
    let saved_ciphertext = fs::read(&state_path).unwrap();
    let mut plan = Plan::good("workspace-b");
    plan.login = LoginPoll::Pending;
    plan.login_auth = Some(auth("user-b", "workspace-b", "not-completed"));
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
    for now in [101, 102] {
        f.engine
            .command(
                CodexCommand::PollLogin(launch.session.id.clone()),
                &f.vault,
                now,
            )
            .await
            .unwrap();
    }
    assert!(f.engine.pending_login.is_some());
    assert!(owned.exists());
    assert!(!owned.join("auth.json").exists());
    assert_eq!(
        f.engine.snapshot(false, 102).login.unwrap().status,
        CodexLoginStatus::Waiting
    );
    assert_eq!(f.active(), active);
    assert_eq!(fs::read(state_path).unwrap(), saved_ciphertext);
    let calls = f.factory.calls.lock().unwrap().clone();
    assert!(!calls.contains(&"shutdown"));
    assert!(!calls.contains(&"account"));
    assert!(!calls.contains(&"quota"));
    f.engine.cancel_login(&launch.session.id).await.unwrap();
    assert!(!owned.exists());
    assert!(f.engine.pending_login.is_none());
    f.claude_untouched();
}
#[tokio::test]
async fn demo_has_two_read_only_accounts_without_an_installation_or_client() {
    let mut engine = CodexEngine::demo(1000);
    let factory = Arc::new(FakeFactory::default());
    engine.factory = factory.clone();
    assert!(engine.installation.is_none());
    assert!(!engine.writable);
    assert!(
        engine
            .saved
            .accounts
            .iter()
            .all(|account| account.auth.is_empty())
    );
    let snapshot = engine.snapshot(false, 1000);
    assert!(snapshot.demo);
    assert_eq!(snapshot.accounts.len(), 2);
    assert_eq!(snapshot.selected_id.as_deref(), Some("codex-demo-0"));
    assert!(
        snapshot
            .accounts
            .iter()
            .all(|account| account.quota.is_some() && !account.manual_switch.enabled)
    );
    for capability in [
        &snapshot.capabilities.login_browser,
        &snapshot.capabilities.import_current,
        &snapshot.capabilities.refresh_quota,
        &snapshot.capabilities.manual_switch,
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
    assert!(factory.calls.lock().unwrap().is_empty());
    assert!(engine.installation.is_none());
    assert!(vault.load::<Saved>(RECORD).unwrap().is_none());
}
