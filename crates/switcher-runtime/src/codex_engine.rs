//! Codex state machine under the existing serialized runtime owner.
use crate::{codex_context::*, codex_views::*};
use async_trait::async_trait;
use provider_codex::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use switcher_core::ProviderId;
use switcher_platform::{Vault, codex::*};
use tempfile::TempDir;
use zeroize::Zeroize;

const RECORD: &str = "codex-state";
const PREPARATION_TTL: i64 = 120;

pub struct CodexLoginLaunch {
    pub session: CodexLoginView,
    challenge: BrowserLoginChallenge,
}
impl CodexLoginLaunch {
    pub fn authorization_url(&self) -> &str {
        self.challenge.authorization_url()
    }
}
impl std::fmt::Debug for CodexLoginLaunch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CodexLoginLaunch([REDACTED])")
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct SavedAccount {
    id: String,
    name: String,
    auth: Vec<u8>,
    auth_kind: AuthKind,
    binding: Binding,
    evidence: CodexIdentityEvidence,
    managed_origin: bool,
    quota: Option<CodexQuotaView>,
    quota_read_at: Option<i64>,
}
impl Drop for SavedAccount {
    fn drop(&mut self) {
        self.auth.zeroize();
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Binding {
    user: Option<String>,
    workspace: Option<String>,
    email: Option<String>,
}
impl Binding {
    fn from_auth(auth: &OpaqueAuth) -> Result<Self, CodexReason> {
        let claims = auth.routing_claims();
        if claims.is_fedramp {
            return Err(CodexReason::UnsupportedAuth);
        }
        Ok(Self {
            user: claims.chatgpt_user_id.clone(),
            workspace: claims
                .chatgpt_account_id
                .clone()
                .or_else(|| claims.token_account_id.clone()),
            email: claims.email.clone(),
        })
    }
    fn complete(&self) -> bool {
        self.user.is_some() && self.workspace.is_some()
    }
    fn same_owner(&self, other: &Self) -> bool {
        self.complete()
            && other.complete()
            && self.user == other.user
            && self.workspace == other.workspace
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    format: u32,
    accounts: Vec<SavedAccount>,
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            format: 1,
            accounts: Vec::new(),
        }
    }
}

#[async_trait]
pub(crate) trait ClientFactory: Send + Sync {
    async fn start(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        context: CodexContext,
        login: bool,
    ) -> Result<Box<dyn CodexService>, CodexError>;
}
struct NativeFactory;
#[async_trait]
impl ClientFactory for NativeFactory {
    async fn start(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        context: CodexContext,
        login: bool,
    ) -> Result<Box<dyn CodexService>, CodexError> {
        let executable = executable.ok_or(CodexError::NotFound)?;
        Ok(Box::new(if login {
            CodexClient::start_login(executable, context).await?
        } else {
            CodexClient::start_active(executable, context).await?
        }))
    }
}
struct Installation {
    executable: Option<VerifiedCodexExecutable>,
    executable_path: PathBuf,
    paths: ContextPaths,
    workspace: TempDir,
    qualified: QualifiedContext,
}
struct PendingLogin {
    view: CodexLoginView,
    intent: u64,
    home: TempDir,
    client: Box<dyn CodexService>,
}
struct Preparation {
    view: CodexPreparationView,
    receipt: PreparedCodexSwitch,
}

pub(crate) enum CodexCommand {
    Discover,
    BeginLogin(u64),
    PollLogin(String),
    CancelLogin(String),
    ImportCurrent,
    Refresh(String),
    Delete(String),
    Prepare(String),
    CancelPreparation(String),
    Apply(String, bool),
}
pub(crate) enum CodexOutput {
    Snapshot,
    Login(CodexLoginLaunch),
    Preparation(CodexPreparationView),
}
pub(crate) struct CodexEngine {
    saved: Saved,
    writable: bool,
    installation: Option<Installation>,
    factory: Arc<dyn ClientFactory>,
    guard: Arc<dyn CodexWriteGuard>,
    pub intent: Arc<AtomicU64>,
    pending_login: Option<PendingLogin>,
    login_view: Option<CodexLoginView>,
    preparation: Option<Preparation>,
    pending_switch: Option<PendingCodexSwitch>,
    selected_id: Option<String>,
    observed_generation: Option<StoreGeneration>,
    verified_now: BTreeSet<String>,
    account_errors: std::collections::BTreeMap<String, CodexReason>,
    availability: CodexAvailability,
    reason: Option<CodexReason>,
    pub error: Option<CodexReason>,
    pub revision: u64,
    demo: bool,
    reconciling: bool,
    external_reconciliation: bool,
}
impl CodexEngine {
    pub fn load(vault: &Vault) -> Self {
        let mut engine = Self::empty(false);
        match vault.load::<Saved>(RECORD) {
            Ok(Some(saved)) if valid_saved(&saved) => {
                engine.saved = saved;
            }
            Ok(None) => (),
            _ => {
                engine.writable = false;
                engine.reason = Some(CodexReason::VaultUnavailable);
            }
        }
        engine
    }
    pub fn empty(demo: bool) -> Self {
        Self {
            saved: Saved::default(),
            writable: !demo,
            installation: None,
            factory: Arc::new(NativeFactory),
            guard: Arc::new(NativeCodexWriteGuard),
            intent: Arc::new(AtomicU64::new(0)),
            pending_login: None,
            login_view: None,
            preparation: None,
            pending_switch: None,
            selected_id: None,
            observed_generation: None,
            verified_now: BTreeSet::new(),
            account_errors: Default::default(),
            availability: CodexAvailability::Unqualified,
            reason: None,
            error: None,
            revision: 0,
            demo,
            reconciling: false,
            external_reconciliation: false,
        }
    }

    pub fn demo(now: i64) -> Self {
        let mut engine = Self::empty(true);
        engine.availability = CodexAvailability::Supported;
        for (index, name, used) in [(0, "Studio Codex", 42), (1, "Personal Codex", 19)] {
            let id = format!("codex-demo-{index}");
            engine.saved.accounts.push(SavedAccount {
                id: id.clone(),
                name: name.into(),
                auth: Vec::new(),
                auth_kind: AuthKind::ManagedChatgpt,
                binding: Binding {
                    user: Some(format!("codex-demo-user-{index}")),
                    workspace: Some(format!("codex-demo-workspace-{index}")),
                    email: Some(format!("codex-demo-{index}@example.invalid")),
                },
                evidence: CodexIdentityEvidence::BackendVerified,
                managed_origin: true,
                quota: Some(CodexQuotaView {
                    ordinary_usage_allowed: Some(true),
                    limits: vec![
                        CodexLimitView {
                            key: "default".into(),
                            limit_id: Some("codex".into()),
                            limit_name: Some("Codex".into()),
                            normal_model_slug: None,
                            primary: Some(CodexWindowView {
                                used_percent: used,
                                window_duration_mins: Some(300),
                                resets_at: Some(now + 7200),
                            }),
                            secondary: Some(CodexWindowView {
                                used_percent: 19,
                                window_duration_mins: Some(10080),
                                resets_at: Some(now + 259200),
                            }),
                            plan_type: Some("plus".into()),
                            credits: Some(CodexCreditsView {
                                has_credits: true,
                                unlimited: false,
                                balance: Some("12.50".into()),
                            }),
                            spend_control_reached: Some(false),
                            rate_limit_reached_type: None,
                        },
                        CodexLimitView {
                            key: "bucket:code-review".into(),
                            limit_id: Some("code-review".into()),
                            limit_name: Some("Code review".into()),
                            normal_model_slug: None,
                            primary: Some(CodexWindowView {
                                used_percent: 76,
                                window_duration_mins: Some(1440),
                                resets_at: None,
                            }),
                            secondary: None,
                            plan_type: None,
                            credits: None,
                            spend_control_reached: None,
                            rate_limit_reached_type: None,
                        },
                    ],
                    reset_credits_available: Some(3),
                }),
                quota_read_at: Some(now),
            });
            engine.verified_now.insert(id);
        }
        engine.selected_id = Some("codex-demo-0".into());
        engine
    }
    pub fn unavailable(&mut self) {
        *self = Self::empty(false);
        self.writable = false;
        self.reason = Some(CodexReason::VaultUnavailable);
    }
    fn save(&self, vault: &Vault) -> Result<(), CodexReason> {
        if !self.writable {
            return Err(CodexReason::VaultUnavailable);
        }
        vault
            .save(RECORD, &self.saved)
            .map_err(|_| CodexReason::VaultUnavailable)
    }
    fn ready(&self) -> Result<&Installation, CodexReason> {
        if !self.writable {
            return Err(CodexReason::VaultUnavailable);
        }
        if self.reconciling {
            return Err(CodexReason::ReconciliationRequired);
        }
        self.installation
            .as_ref()
            .ok_or(self.reason.unwrap_or(CodexReason::NotInstalled))
    }
    fn store(&self) -> Result<CodexFileStore, CodexReason> {
        let installation = self
            .installation
            .as_ref()
            .ok_or(CodexReason::NotInstalled)?;
        let context = installation
            .qualified
            .context
            .clone()
            .ok_or(CodexReason::UnsupportedStore)?;
        Ok(CodexFileStore::new(context, self.guard.clone()))
    }
    fn requalify(&mut self) -> Result<(), CodexReason> {
        let installation = self
            .installation
            .as_mut()
            .ok_or(CodexReason::NotInstalled)?;
        installation.qualified.revalidate_static()?;
        installation.qualified = installation
            .paths
            .qualify(&installation.executable_path, installation.workspace.path())?;
        Ok(())
    }
    fn account_index(&self, id: &str) -> Result<usize, CodexReason> {
        if id.len() > 256 {
            return Err(CodexReason::IdentityUnverified);
        }
        self.saved
            .accounts
            .iter()
            .position(|a| a.id == id)
            .ok_or(CodexReason::IdentityUnverified)
    }
    fn switch_capability(&self, account: &SavedAccount) -> CodexCapability {
        let reason = if self.demo || !self.writable {
            Some(CodexReason::VaultUnavailable)
        } else if self.reconciling {
            Some(CodexReason::ReconciliationRequired)
        } else if self.installation.is_none() {
            Some(self.reason.unwrap_or(CodexReason::NotInstalled))
        } else if account.auth_kind != AuthKind::ManagedChatgpt {
            Some(CodexReason::UnsupportedAuth)
        } else if self.account_errors.get(&account.id).is_some_and(|reason| {
            matches!(
                reason,
                CodexReason::ExternalChange
                    | CodexReason::IdentityMismatch
                    | CodexReason::StoreConflict
                    | CodexReason::VaultUnavailable
                    | CodexReason::ReconciliationRequired
            )
        }) {
            self.account_errors.get(&account.id).copied()
        } else if !account.binding.complete()
            || !matches!(
                account.evidence,
                CodexIdentityEvidence::ManagedLogin | CodexIdentityEvidence::BackendVerified
            )
        {
            Some(CodexReason::IdentityUnverified)
        } else {
            None
        };
        reason.map_or_else(CodexCapability::allowed, CodexCapability::blocked)
    }
    pub fn snapshot(&self, busy: bool, now: i64) -> CodexSnapshot {
        let mut result = CodexSnapshot::empty(self.demo);
        result.revision = self.revision;
        result.busy = busy;
        result.error = self.error;
        result.availability = self.availability;
        result.blocked_reason = self.reason;
        if self.demo {
            result.executable_version = Some(SUPPORTED_CODEX_VERSION.into());
            result.active_model = Some("demo-model".into());
        }
        result.login = self.login_view.clone();
        result.selected_id = self.selected_id.clone();
        if let Some(installation) = &self.installation {
            result.executable_version = Some(SUPPORTED_CODEX_VERSION.into());
            result.active_model = installation.qualified.active_model.clone();
        }
        let common = if self.demo || !self.writable {
            Some(CodexReason::VaultUnavailable)
        } else if self.installation.is_none() {
            Some(self.reason.unwrap_or(CodexReason::NotInstalled))
        } else {
            None
        };
        let capability = |reason: Option<CodexReason>| {
            reason.map_or_else(CodexCapability::allowed, CodexCapability::blocked)
        };
        result.capabilities = CodexCapabilities {
            login_browser: capability(
                common.or(self
                    .reconciling
                    .then_some(CodexReason::ReconciliationRequired)),
            ),
            import_current: capability(common.or_else(|| self.store().err())),
            refresh_quota: capability(common.or_else(|| {
                match self
                    .selected_id
                    .as_ref()
                    .and_then(|id| self.saved.accounts.iter().find(|a| &a.id == id))
                {
                    None => Some(CodexReason::IdentityUnverified),
                    Some(account) if account.auth_kind != AuthKind::ManagedChatgpt => {
                        Some(CodexReason::UnsupportedAuth)
                    }
                    Some(account) if !account.binding.complete() => {
                        Some(CodexReason::IdentityUnverified)
                    }
                    Some(_) => None,
                }
            })),
            manual_switch: capability(
                common.or(self
                    .reconciling
                    .then_some(CodexReason::ReconciliationRequired)),
            ),
            delete_saved: capability(
                (!self.writable || self.demo).then_some(CodexReason::VaultUnavailable),
            ),
        };
        result.accounts = self
            .saved
            .accounts
            .iter()
            .map(|account| CodexAccountView {
                id: account.id.clone(),
                provider: ProviderId::Codex,
                name: account.name.clone(),
                email: account.binding.email.clone(),
                workspace_id: account.binding.workspace.clone(),
                workspace_name: None,
                auth_kind: match account.auth_kind {
                    AuthKind::ManagedChatgpt => "chatgpt",
                    AuthKind::ApiKey => "apiKey",
                    AuthKind::Unsupported => "unsupported",
                }
                .into(),
                identity_evidence: account.evidence,
                identity_verified: self.verified_now.contains(&account.id),
                selected: self.selected_id.as_ref() == Some(&account.id),
                manual_switch: self.switch_capability(account),
                quota: account.quota.clone(),
                quota_read_at: account.quota_read_at,
                quota_state: if self.account_errors.contains_key(&account.id) {
                    CodexQuotaState::Unavailable
                } else if account.quota.is_none() {
                    CodexQuotaState::Unread
                } else if self.verified_now.contains(&account.id)
                    && account
                        .quota_read_at
                        .is_some_and(|time| now >= time && now - time <= 900)
                {
                    CodexQuotaState::Fresh
                } else {
                    CodexQuotaState::Cached
                },
                error: self.account_errors.get(&account.id).copied(),
            })
            .collect();
        result
    }
    pub async fn command(
        &mut self,
        command: CodexCommand,
        vault: &Vault,
        now: i64,
    ) -> Result<CodexOutput, CodexReason> {
        let checkpoint = self.saved.clone();
        let selected = self.selected_id.clone();
        let result = self.execute(command, vault, now).await;
        if result
            .as_ref()
            .is_err_and(|error| *error == CodexReason::VaultUnavailable)
        {
            self.saved = checkpoint;
            self.selected_id = selected;
            self.verified_now.clear();
            self.writable = false;
            self.reason = Some(CodexReason::VaultUnavailable);
        }
        result
    }
    async fn execute(
        &mut self,
        command: CodexCommand,
        vault: &Vault,
        now: i64,
    ) -> Result<CodexOutput, CodexReason> {
        if self.demo || !self.writable {
            return Err(CodexReason::VaultUnavailable);
        }
        self.error = None;
        if self
            .preparation
            .as_ref()
            .is_some_and(|p| now >= p.view.expires_at)
        {
            self.preparation = None;
            self.recover(vault)?;
        }
        match command {
            CodexCommand::Discover => {
                if let Err(error) = self.discover(vault).await {
                    self.reason = Some(error);
                    self.availability = if error == CodexReason::NotInstalled {
                        CodexAvailability::NotInstalled
                    } else {
                        CodexAvailability::Unsupported
                    };
                    return Err(error);
                }
            }
            CodexCommand::BeginLogin(intent) => {
                return self.begin_login(intent).await.map(CodexOutput::Login);
            }
            CodexCommand::PollLogin(id) => {
                self.poll_login(&id, vault, now).await?;
            }
            CodexCommand::CancelLogin(id) => {
                self.cancel_login(&id).await?;
            }
            CodexCommand::ImportCurrent => {
                self.import_current(vault)?;
            }
            CodexCommand::Refresh(id) => {
                if let Err(error) = self.refresh(&id, vault, now).await {
                    self.account_errors.insert(id.clone(), error);
                    self.verified_now.remove(&id);
                    return Err(error);
                }
            }
            CodexCommand::Delete(id) => {
                if self.pending_login.is_some() || self.preparation.is_some() {
                    return Err(CodexReason::Busy);
                }
                let index = self.account_index(&id)?;
                self.saved.accounts.remove(index);
                self.verified_now.remove(&id);
                self.account_errors.remove(&id);
                if self.selected_id.as_ref() == Some(&id) {
                    self.selected_id = None;
                    self.observed_generation = None;
                }
                self.save(vault)?;
            }
            CodexCommand::Prepare(id) => {
                return self.prepare(&id, vault, now).map(CodexOutput::Preparation);
            }
            CodexCommand::CancelPreparation(id) => {
                if self.preparation.as_ref().is_some_and(|p| p.view.id == id) {
                    self.preparation = None;
                    self.recover(vault)?;
                }
            }
            CodexCommand::Apply(id, acknowledgment) => {
                self.apply(&id, acknowledgment, vault, now).await?;
            }
        }
        Ok(CodexOutput::Snapshot)
    }
    async fn discover(&mut self, vault: &Vault) -> Result<(), CodexReason> {
        if self.pending_login.is_some() {
            return Err(CodexReason::Busy);
        }
        self.preparation = None;
        self.installation = None;
        self.selected_id = None;
        self.observed_generation = None;
        self.verified_now.clear();
        self.availability = CodexAvailability::Unqualified;
        let paths = ContextPaths::discover()?;
        let workspace = owned_home()?;
        let context =
            CodexContext::isolated(workspace.path().to_owned(), workspace.path().to_owned())
                .map_err(provider_reason)?;
        let candidates =
            discover_candidates(&DiscoveryOptions::from_environment()).map_err(provider_reason)?;
        let mut last = CodexReason::NotInstalled;
        let mut verified = None;
        for candidate in &candidates {
            match verify_executable(candidate.clone(), &context).await {
                Ok(executable) => {
                    verified = Some(executable);
                    break;
                }
                Err(error) => {
                    last = provider_reason(error);
                }
            }
        }
        let Some(executable) = verified else {
            self.availability = if candidates.is_empty() {
                CodexAvailability::NotInstalled
            } else {
                CodexAvailability::Unsupported
            };
            self.reason = Some(last);
            return Err(last);
        };
        let executable_path = executable.native_path().to_owned();
        let qualified = match paths.qualify(&executable_path, workspace.path()) {
            Ok(value) => value,
            Err(error) => {
                self.availability = CodexAvailability::Unsupported;
                self.reason = Some(error);
                return Err(error);
            }
        };
        self.installation = Some(Installation {
            executable: Some(executable),
            executable_path,
            paths,
            workspace,
            qualified,
        });
        self.availability = CodexAvailability::Supported;
        self.reason = None;
        self.reconciling = false;
        self.recover(vault)?;
        self.observe_current()?;
        Ok(())
    }
    fn recover(&mut self, vault: &Vault) -> Result<(), CodexReason> {
        let store = match self.store() {
            Ok(store) => store,
            Err(CodexReason::UnsupportedStore) => return Ok(()),
            Err(error) => return Err(error),
        };
        match store.recover(vault).map_err(store_reason) {
            Ok(CodexRecovery::Clean | CodexRecovery::CanceledPreparation) => {
                self.pending_switch = None;
                self.reconciling = false;
                self.external_reconciliation = false;
            }
            Ok(CodexRecovery::NeedsVerification(pending)) => {
                self.pending_switch = Some(pending);
                self.reconciling = true;
            }
            Err(error) => {
                self.reconciling = true;
                self.reason = Some(CodexReason::ReconciliationRequired);
                return Err(error);
            }
        }
        Ok(())
    }
    fn observe_current(&mut self) -> Result<(), CodexReason> {
        let store = match self.store() {
            Ok(store) => store,
            Err(CodexReason::UnsupportedStore) => return Ok(()),
            Err(error) => return Err(error),
        };
        let snapshot = store.read().map_err(store_reason)?;
        let Some(bytes) = snapshot.auth_bytes() else {
            self.selected_id = None;
            self.observed_generation = Some(snapshot.generation);
            return Ok(());
        };
        let auth = OpaqueAuth::parse(bytes.to_vec()).map_err(provider_reason)?;
        let binding = Binding::from_auth(&auth)?;
        self.selected_id = self
            .saved
            .accounts
            .iter()
            .find(|a| {
                a.auth_kind == auth.kind() && (a.binding.same_owner(&binding) || a.auth == bytes)
            })
            .map(|a| a.id.clone());
        self.observed_generation = Some(snapshot.generation);
        Ok(())
    }
    async fn begin_login(&mut self, intent: u64) -> Result<CodexLoginLaunch, CodexReason> {
        self.ready()?;
        if self.preparation.is_some() || self.pending_login.is_some() {
            return Err(CodexReason::Busy);
        }
        self.requalify()?;
        let home = owned_home()?;
        let context = CodexContext::isolated(home.path().to_owned(), home.path().to_owned())
            .map_err(provider_reason)?;
        let installation = self
            .installation
            .as_ref()
            .ok_or(CodexReason::NotInstalled)?;
        let mut client = self
            .factory
            .start(installation.executable.as_ref(), context, true)
            .await
            .map_err(provider_reason)?;
        let result = async {
            let inspection = client
                .inspect_configuration()
                .await
                .map_err(provider_reason)?;
            validate_inspection(&inspection)?;
            client.begin_browser_login().await.map_err(provider_reason)
        }
        .await;
        let challenge = match result {
            Ok(value) if self.intent.load(Ordering::Acquire) == intent => value,
            Ok(_) => {
                let _ = client.cancel_login().await;
                let _ = client.shutdown().await;
                return Err(CodexReason::LoginCanceled);
            }
            Err(error) => {
                let _ = client.shutdown().await;
                return Err(error);
            }
        };
        let view = CodexLoginView {
            id: uuid::Uuid::new_v4().to_string(),
            status: CodexLoginStatus::Waiting,
            error: None,
        };
        self.login_view = Some(view.clone());
        self.pending_login = Some(PendingLogin {
            view: view.clone(),
            intent,
            home,
            client,
        });
        Ok(CodexLoginLaunch {
            session: view,
            challenge,
        })
    }
    pub async fn shutdown_owned(&mut self) {
        self.intent.fetch_add(1, Ordering::AcqRel);
        if let Some(mut pending) = self.pending_login.take() {
            let _ = pending.client.cancel_login().await;
            let _ = pending.client.shutdown().await;
        }
    }
    async fn cancel_login(&mut self, id: &str) -> Result<(), CodexReason> {
        if self.pending_login.as_ref().is_none_or(|p| p.view.id != id) {
            return Ok(());
        }
        let mut pending = self
            .pending_login
            .take()
            .ok_or(CodexReason::LoginCanceled)?;
        let _ = pending.client.cancel_login().await;
        pending.client.shutdown().await.map_err(provider_reason)?;
        self.login_view = Some(CodexLoginView {
            id: id.into(),
            status: CodexLoginStatus::Failed,
            error: Some(CodexReason::LoginCanceled),
        });
        Ok(())
    }
    async fn poll_login(&mut self, id: &str, vault: &Vault, now: i64) -> Result<(), CodexReason> {
        let pending = self
            .pending_login
            .as_mut()
            .filter(|p| p.view.id == id)
            .ok_or(CodexReason::LoginExpired)?;
        if self.intent.load(Ordering::Acquire) != pending.intent {
            return self.cancel_login(id).await;
        }
        let poll = match pending.client.poll_login() {
            Ok(poll) => poll,
            Err(error) => {
                let reason = if error == CodexError::Timeout {
                    CodexReason::LoginExpired
                } else {
                    provider_reason(error)
                };
                let mut pending = self.pending_login.take().ok_or(reason)?;
                let _ = pending.client.shutdown().await;
                self.login_view = Some(CodexLoginView {
                    id: id.into(),
                    status: CodexLoginStatus::Failed,
                    error: Some(reason),
                });
                return Err(reason);
            }
        };
        match poll {
            LoginPoll::Pending => return Ok(()),
            LoginPoll::Failed => {
                let mut pending = self.pending_login.take().ok_or(CodexReason::LoginExpired)?;
                let _ = pending.client.shutdown().await;
                self.login_view = Some(CodexLoginView {
                    id: id.into(),
                    status: CodexLoginStatus::Failed,
                    error: Some(CodexReason::ProviderUnavailable),
                });
                return Err(CodexReason::ProviderUnavailable);
            }
            LoginPoll::Completed => (),
        }
        let mut pending = self.pending_login.take().ok_or(CodexReason::LoginExpired)?;
        self.login_view = Some(CodexLoginView {
            id: id.into(),
            status: CodexLoginStatus::Verifying,
            error: None,
        });
        let result = async {
            let observation = pending
                .client
                .read_account()
                .await
                .map_err(provider_reason)?;
            if !matches!(observation.account, Some(CodexAccount::Chatgpt { .. })) {
                return Err(CodexReason::UnsupportedAuth);
            }
            let initial = read_private_auth(pending.home.path())
                .map_err(store_reason)?
                .ok_or(CodexReason::IdentityUnverified)?;
            let initial = OpaqueAuth::parse(initial.to_vec()).map_err(provider_reason)?;
            let binding = Binding::from_auth(&initial)?;
            if initial.kind() != AuthKind::ManagedChatgpt || !binding.complete() {
                return Err(CodexReason::IdentityUnverified);
            }
            let inspection = pending
                .client
                .inspect_configuration()
                .await
                .map_err(provider_reason)?;
            validate_inspection(&inspection)?;
            let quota = pending.client.read_rate_limits().await.ok();
            Ok((binding, quota))
        }
        .await;
        let shutdown = pending.client.shutdown().await.map_err(provider_reason);
        let complete = (|| {
            let (initial_binding, quota) = result?;
            shutdown?;
            if self.intent.load(Ordering::Acquire) != pending.intent {
                return Err(CodexReason::LoginCanceled);
            }
            let bytes = read_private_auth(pending.home.path())
                .map_err(store_reason)?
                .ok_or(CodexReason::IdentityUnverified)?;
            let auth = OpaqueAuth::parse(bytes.to_vec()).map_err(provider_reason)?;
            let binding = Binding::from_auth(&auth)?;
            if auth.kind() != AuthKind::ManagedChatgpt || !initial_binding.same_owner(&binding) {
                return Err(CodexReason::IdentityMismatch);
            }
            let installation = self
                .installation
                .as_ref()
                .ok_or(CodexReason::NotInstalled)?;
            let paths = ContextPaths {
                home: pending.home.path().to_owned(),
                system_config: installation.paths.system_config.clone(),
                system_requirements: installation.paths.system_requirements.clone(),
                legacy_requirements: if cfg!(windows) {
                    pending.home.path().join("managed_config.toml")
                } else {
                    installation.paths.legacy_requirements.clone()
                },
                enforce_native_policy: false,
                #[cfg(test)]
                project_ancestor_stop: Some(pending.home.path().to_owned()),
            };
            paths.qualify(&installation.executable_path, pending.home.path())?;
            let verified_quota = match quota {
                Some(quota)
                    if quota.account_id.as_ref() == binding.workspace.as_ref()
                        && quota.ordinary_usage_allowed.is_some() =>
                {
                    Some(quota_view(&quota)?)
                }
                Some(quota)
                    if quota.account_id.is_some()
                        && quota.account_id.as_ref() != binding.workspace.as_ref() =>
                {
                    return Err(CodexReason::IdentityMismatch);
                }
                _ => None,
            };
            let index = self.upsert(auth, CodexIdentityEvidence::ManagedLogin, true)?;
            let account = &mut self.saved.accounts[index];
            if let Some(quota) = verified_quota {
                account.quota = Some(quota);
                account.quota_read_at = Some(now);
                account.evidence = CodexIdentityEvidence::BackendVerified;
                self.verified_now.insert(account.id.clone());
            }
            self.account_errors.remove(&account.id);
            self.save(vault)?;
            Ok(())
        })();
        match complete {
            Ok(()) => {
                self.login_view = Some(CodexLoginView {
                    id: id.into(),
                    status: CodexLoginStatus::Complete,
                    error: None,
                });
                Ok(())
            }
            Err(error) => {
                self.login_view = Some(CodexLoginView {
                    id: id.into(),
                    status: CodexLoginStatus::Failed,
                    error: Some(error),
                });
                Err(error)
            }
        }
    }
    fn import_current(&mut self, vault: &Vault) -> Result<(), CodexReason> {
        if self.pending_login.is_some() || self.preparation.is_some() {
            return Err(CodexReason::Busy);
        }
        self.requalify()?;
        match self.recover(vault) {
            Ok(()) => (),
            Err(CodexReason::ExternalChange) => {
                // Explicit import adopts only the current login into encrypted storage.
                // Keep the authentic conflicting journal until a subsequent active
                // backend read corroborates the imported owner and durable rotation.
                self.external_reconciliation = true;
                self.pending_switch = None;
                self.reconciling = true;
                self.reason = Some(CodexReason::ReconciliationRequired);
            }
            Err(error) => return Err(error),
        }
        let store = self.store()?;
        store.check_quiescent().map_err(store_reason)?;
        let snapshot = store.read().map_err(store_reason)?;
        let auth = OpaqueAuth::parse(
            snapshot
                .auth_bytes()
                .ok_or(CodexReason::UnsupportedAuth)?
                .to_vec(),
        )
        .map_err(provider_reason)?;
        if auth.kind() == AuthKind::Unsupported {
            return Err(CodexReason::UnsupportedAuth);
        }
        let index = self.upsert(auth, CodexIdentityEvidence::ClaimsOnly, false)?;
        self.save(vault)?;
        store.check_quiescent().map_err(store_reason)?;
        if store.read().map_err(store_reason)?.generation != snapshot.generation {
            return Err(CodexReason::ExternalChange);
        }
        self.selected_id = Some(self.saved.accounts[index].id.clone());
        self.observed_generation = Some(snapshot.generation);
        Ok(())
    }
    fn upsert(
        &mut self,
        auth: OpaqueAuth,
        evidence: CodexIdentityEvidence,
        managed: bool,
    ) -> Result<usize, CodexReason> {
        let binding = Binding::from_auth(&auth)?;
        let existing = self.saved.accounts.iter().position(|a| {
            a.auth_kind == auth.kind()
                && (a.binding.same_owner(&binding) || a.auth == auth.as_bytes())
        });
        if let Some(index) = existing {
            let account = &mut self.saved.accounts[index];
            let same_bytes = account.auth == auth.as_bytes();
            account.auth.zeroize();
            account.auth = auth.as_bytes().to_vec();
            if !same_bytes && evidence == CodexIdentityEvidence::ClaimsOnly {
                account.evidence = CodexIdentityEvidence::ClaimsOnly;
                self.verified_now.remove(&account.id);
            }
            if evidence != CodexIdentityEvidence::ClaimsOnly {
                account.evidence = evidence;
            }
            account.managed_origin |= managed;
            account.binding = binding;
            return Ok(index);
        }
        let name = binding.email.clone().unwrap_or_else(|| {
            if auth.kind() == AuthKind::ApiKey {
                "Codex API key"
            } else {
                "Codex account"
            }
            .into()
        });
        self.saved.accounts.push(SavedAccount {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            auth: auth.as_bytes().to_vec(),
            auth_kind: auth.kind(),
            binding,
            evidence,
            managed_origin: managed,
            quota: None,
            quota_read_at: None,
        });
        Ok(self.saved.accounts.len() - 1)
    }
    async fn refresh(&mut self, id: &str, vault: &Vault, now: i64) -> Result<(), CodexReason> {
        if self.pending_login.is_some() || self.preparation.is_some() {
            return Err(CodexReason::Busy);
        }
        if self.selected_id.as_deref() != Some(id) {
            return Err(CodexReason::IdentityUnverified);
        }
        self.verify_active(id, vault, now).await
    }
    async fn verify_active(
        &mut self,
        id: &str,
        vault: &Vault,
        now: i64,
    ) -> Result<(), CodexReason> {
        self.requalify()?;
        let index = self.account_index(id)?;
        let binding = self.saved.accounts[index].binding.clone();
        if self.saved.accounts[index].auth_kind != AuthKind::ManagedChatgpt || !binding.complete() {
            return Err(CodexReason::UnsupportedAuth);
        }
        let store = self.store()?;
        store.check_quiescent().map_err(store_reason)?;
        let before = store.read().map_err(store_reason)?;
        if self
            .observed_generation
            .as_ref()
            .is_some_and(|generation| generation != &before.generation)
        {
            self.verified_now.remove(id);
            self.selected_id = None;
            return Err(CodexReason::ExternalChange);
        }
        let auth = OpaqueAuth::parse(
            before
                .auth_bytes()
                .ok_or(CodexReason::UnsupportedAuth)?
                .to_vec(),
        )
        .map_err(provider_reason)?;
        if !binding.same_owner(&Binding::from_auth(&auth)?) {
            self.selected_id = None;
            return Err(CodexReason::IdentityMismatch);
        }
        let installation = self
            .installation
            .as_ref()
            .ok_or(CodexReason::NotInstalled)?;
        let context = CodexContext::approved_file(
            store.context().home().to_owned(),
            installation.workspace.path().to_owned(),
        )
        .map_err(provider_reason)?;
        let mut client = self
            .factory
            .start(installation.executable.as_ref(), context, false)
            .await
            .map_err(provider_reason)?;
        let result = async {
            let account = client.read_account().await.map_err(provider_reason)?;
            if !matches!(account.account, Some(CodexAccount::Chatgpt { .. })) {
                return Err(CodexReason::UnsupportedAuth);
            }
            let inspection = client
                .inspect_configuration()
                .await
                .map_err(provider_reason)?;
            let model = validate_inspection(&inspection)?;
            let quota = client.read_rate_limits().await.map_err(provider_reason)?;
            if quota.account_id.as_ref() != binding.workspace.as_ref()
                || quota.ordinary_usage_allowed.is_none()
            {
                return Err(CodexReason::IdentityMismatch);
            }
            Ok((quota_view(&quota)?, model))
        }
        .await;
        let shutdown = client.shutdown().await.map_err(provider_reason);
        // Preserve owned rotation after every provider result, before completing a journal.
        self.requalify()?;
        let current_store = self.store()?;
        current_store.check_quiescent().map_err(store_reason)?;
        let after = current_store.read().map_err(store_reason)?;
        let fresh = OpaqueAuth::parse(
            after
                .auth_bytes()
                .ok_or(CodexReason::UnsupportedAuth)?
                .to_vec(),
        )
        .map_err(provider_reason)?;
        if fresh.kind() != AuthKind::ManagedChatgpt
            || !binding.same_owner(&Binding::from_auth(&fresh)?)
        {
            self.verified_now.remove(id);
            self.reconciling = self.pending_switch.is_some();
            self.selected_id = None;
            self.observed_generation = None;
            return Err(CodexReason::IdentityMismatch);
        }
        let account = &mut self.saved.accounts[index];
        account.auth.zeroize();
        account.auth = fresh.as_bytes().to_vec();
        let result = result.and_then(|value| shutdown.map(|_| value));
        if let Ok((quota, model)) = &result {
            account.quota = Some(quota.clone());
            account.quota_read_at = Some(now);
            account.evidence = CodexIdentityEvidence::BackendVerified;
            self.verified_now.insert(id.into());
            self.account_errors.remove(id);
            if let Some(installation) = &mut self.installation {
                installation.qualified.active_model = model.clone();
            }
        } else {
            self.verified_now.remove(id);
        }
        self.save(vault)?;
        if current_store.read().map_err(store_reason)?.generation != after.generation {
            return Err(CodexReason::ExternalChange);
        }
        self.observed_generation = Some(after.generation.clone());
        match result {
            Ok(_) => {
                if self.external_reconciliation {
                    current_store
                        .reconcile_verified_current(&after.generation, vault)
                        .map_err(store_reason)?;
                    self.external_reconciliation = false;
                    self.reconciling = false;
                    self.reason = None;
                } else if let Some(pending) = &self.pending_switch {
                    current_store
                        .finish_verified(pending, &after.generation, vault)
                        .map_err(store_reason)?;
                    self.pending_switch = None;
                    self.reconciling = false;
                    self.external_reconciliation = false;
                    self.reason = None;
                }
                Ok(())
            }
            Err(error) => {
                self.account_errors.insert(id.into(), error);
                Err(error)
            }
        }
    }
    fn prepare(
        &mut self,
        id: &str,
        vault: &Vault,
        now: i64,
    ) -> Result<CodexPreparationView, CodexReason> {
        self.ready()?;
        if self.pending_login.is_some() {
            return Err(CodexReason::Busy);
        }
        if self.preparation.take().is_some() {
            self.recover(vault)?;
        }
        let index = self.account_index(id)?;
        let capability = self.switch_capability(&self.saved.accounts[index]);
        if !capability.enabled {
            return Err(capability
                .blocked_reason
                .unwrap_or(CodexReason::IdentityUnverified));
        }
        self.requalify()?;
        if self.store().is_err() {
            let home = self
                .installation
                .as_ref()
                .ok_or(CodexReason::NotInstalled)?
                .paths
                .home
                .clone();
            create_private_context(home).map_err(store_reason)?;
            // Creating the target directory is explicit preparation; recapture its absent source receipts.
            let installation = self
                .installation
                .as_mut()
                .ok_or(CodexReason::NotInstalled)?;
            installation.qualified = installation
                .paths
                .qualify(&installation.executable_path, installation.workspace.path())?;
        }
        let store = self.store()?;
        store.check_quiescent().map_err(store_reason)?;
        let snapshot = store.read().map_err(store_reason)?;
        if let Some(bytes) = snapshot.auth_bytes() {
            let outgoing = OpaqueAuth::parse(bytes.to_vec()).map_err(provider_reason)?;
            let binding = Binding::from_auth(&outgoing)?;
            if let Some(account) = self
                .saved
                .accounts
                .iter_mut()
                .find(|a| a.binding.same_owner(&binding))
            {
                // Adopt the current outgoing rotation rather than replaying an old encrypted copy later.
                account.auth.zeroize();
                account.auth = bytes.to_vec();
                self.save(vault)?;
            }
        }
        let receipt = store
            .prepare_switch(
                &self.saved.accounts[index].auth,
                &snapshot.generation,
                vault,
            )
            .map_err(store_reason)?;
        let view = CodexPreparationView {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: id.into(),
            expires_at: now + PREPARATION_TTL,
        };
        self.preparation = Some(Preparation {
            view: view.clone(),
            receipt,
        });
        Ok(view)
    }
    async fn apply(
        &mut self,
        id: &str,
        acknowledgment: bool,
        vault: &Vault,
        now: i64,
    ) -> Result<(), CodexReason> {
        if !acknowledgment {
            return Err(CodexReason::InvalidPreparation);
        }
        if self.preparation.as_ref().is_none_or(|p| p.view.id != id) {
            return Err(CodexReason::InvalidPreparation);
        }
        let preparation = self
            .preparation
            .take()
            .ok_or(CodexReason::InvalidPreparation)?;
        if now >= preparation.view.expires_at {
            self.recover(vault)?;
            return Err(CodexReason::InvalidPreparation);
        }
        let account_id = preparation.view.account_id;
        let receipt = preparation.receipt;
        self.requalify()?;
        let store = self.store()?;
        self.reconciling = true;
        let pending = store.commit_file(&receipt, vault).map_err(store_reason)?;
        self.pending_switch = Some(pending.clone());
        self.preparation = None;
        self.selected_id = Some(account_id.clone());
        self.observed_generation = Some(pending.generation);
        self.verify_active(&account_id, vault, now).await
    }
}
fn valid_saved(saved: &Saved) -> bool {
    if saved.format != 1 || saved.accounts.len() > 1000 {
        return false;
    }
    let mut ids = BTreeSet::new();
    saved.accounts.iter().all(|account| {
        !account.id.is_empty()
            && account.id.len() <= 256
            && ids.insert(&account.id)
            && safe_text(&account.name).is_some()
            && OpaqueAuth::parse(account.auth.clone()).is_ok_and(|auth| {
                auth.kind() == account.auth_kind
                    && Binding::from_auth(&auth).is_ok_and(|binding| binding == account.binding)
            })
    })
}
fn owned_home() -> Result<TempDir, CodexReason> {
    let parent = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| CodexReason::StoreConflict)?;
    let directory = tempfile::Builder::new()
        .prefix("primerswitch-codex-")
        .tempdir_in(parent)
        .map_err(|_| CodexReason::StoreConflict)?;
    let canonical = directory
        .path()
        .canonicalize()
        .map_err(|_| CodexReason::StoreConflict)?;
    create_private_context(canonical).map_err(store_reason)?;
    Ok(directory)
}
pub(crate) fn provider_reason(error: CodexError) -> CodexReason {
    match error {
        CodexError::NotFound => CodexReason::NotInstalled,
        CodexError::UnsupportedVersion | CodexError::UnsupportedInstallation => {
            CodexReason::UnsupportedVersion
        }
        CodexError::UnsafeContext => CodexReason::UnsupportedStore,
        CodexError::PolicyRestricted => CodexReason::PolicyRestricted,
        CodexError::Cancelled => CodexReason::LoginCanceled,
        CodexError::UnsupportedAuthMode => CodexReason::UnsupportedAuth,
        CodexError::IdentityMismatch => CodexReason::IdentityMismatch,
        _ => CodexReason::ProviderUnavailable,
    }
}
fn quota_view(quota: &CodexRateLimits) -> Result<CodexQuotaView, CodexReason> {
    let mut limits = Vec::new();
    let mut add = |key: &str, bucket: &CodexRateLimitBucket| -> Result<(), CodexReason> {
        let window = |window: &Option<CodexRateLimitWindow>| -> Result<Option<CodexWindowView>, CodexReason> {
            window.as_ref().map(|value| {
                if value.used_percent < 0 || value.window_duration_mins.is_some_and(|v| v <= 0) { return Err(CodexReason::ProviderUnavailable); }
                Ok(CodexWindowView { used_percent: value.used_percent, window_duration_mins: value.window_duration_mins, resets_at: value.resets_at })
            }).transpose()
        };
        let text = |value: &Option<String>| value.as_deref().and_then(safe_text);
        let key = safe_text(key)
            .filter(|v| v.len() <= 256)
            .ok_or(CodexReason::ProviderUnavailable)?;
        let credits = bucket.credits.as_ref().map(|value| CodexCreditsView {
            has_credits: value.has_credits,
            unlimited: value.unlimited,
            balance: value
                .balance
                .as_ref()
                .filter(|v| {
                    v.len() <= 128
                        && !v.is_empty()
                        && v.bytes()
                            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
                })
                .cloned(),
        });
        limits.push(CodexLimitView {
            key,
            limit_id: text(&bucket.limit_id),
            limit_name: text(&bucket.limit_name),
            normal_model_slug: text(&bucket.normal_model_slug),
            primary: window(&bucket.primary)?,
            secondary: window(&bucket.secondary)?,
            plan_type: text(&bucket.plan_type),
            credits,
            spend_control_reached: bucket.spend_control_reached,
            rate_limit_reached_type: text(&bucket.rate_limit_reached_type),
        });
        Ok(())
    };
    add("default", &quota.rate_limits)?;
    if let Some(buckets) = &quota.rate_limits_by_limit_id {
        if buckets.len() > 256 {
            return Err(CodexReason::ProviderUnavailable);
        }
        for (key, bucket) in buckets {
            let key = format!("bucket:{key}");
            add(&key, bucket)?;
        }
    }
    Ok(CodexQuotaView {
        ordinary_usage_allowed: quota.ordinary_usage_allowed,
        limits,
        reset_credits_available: quota
            .rate_limit_reset_credits
            .as_ref()
            .map(|v| v.available_count)
            .filter(|v| *v >= 0),
    })
}

#[cfg(test)]
#[path = "codex_engine_tests.rs"]
mod tests;
