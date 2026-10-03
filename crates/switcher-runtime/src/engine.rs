use crate::views::*;
#[path = "codex_runtime.rs"]
mod codex_runtime;
use crate::{
    CodexLoginLaunch,
    codex_engine::{CodexCommand, CodexEngine, CodexOutput},
    codex_views::*,
};
use provider_claude::{ClaudeClient, ClientError, PendingLogin};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use switcher_core::*;
use switcher_platform::{ActiveSnapshot, ActiveStore, CliPaths, Vault, app_data_dir};
use tokio::sync::{Mutex, broadcast};

const RECORD: &str = "runtime-state";
const USAGE_MAX_AGE: i64 = 900;
const SCOPED_MAX_AGE: i64 = 1800;

pub trait Clock: Send + Sync {
    fn now(&self) -> i64;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> i64 {
        chrono::Utc::now().timestamp()
    }
}
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("Local data could not be read or saved. Original files were preserved.")]
    Storage,
    #[error(
        "The system credential store is locked, unavailable, or has no qualified backend. Saved accounts were preserved."
    )]
    UnavailableStorage,
    #[error(
        "This Claude context uses an authentication method or policy that does not support account switching."
    )]
    UnsupportedContext,
    #[error(
        "The encrypted data could not be authenticated or its key is missing. Existing files were preserved."
    )]
    VaultIntegrity,
    #[error("This account is no longer available.")]
    MissingAccount,
    #[error("Claude authentication changed externally. Refresh before continuing.")]
    ExternalChange,
    #[error("Token ownership could not be verified for this account.")]
    Identity,
    #[error("Claude did not provide a fresh reading. Try again later.")]
    Provider,
    #[error("These settings are not valid.")]
    Settings,
    #[error("This view is read-only.")]
    ReadOnly,
    #[error("This login expired or was cancelled.")]
    Login,
    #[error("This import is no longer valid. Select the folder again.")]
    Import,
    #[error("The previous reset request requires reconciliation before another attempt.")]
    PendingReset,
    #[error("The Claude Code version could not be detected. Install a recognized version.")]
    CliVersion,
}

impl RuntimeError {
    pub fn message(&self, locale: &str) -> String {
        if locale != "ro" {
            return self.to_string();
        }
        match self {
            Self::Storage=>"Datele locale nu au putut fi citite sau salvate. Fișierele originale sunt păstrate.",
            Self::UnavailableStorage=>"Seiful sistemului este blocat, indisponibil sau nu are un backend calificat. Conturile salvate nu au fost modificate.",
            Self::UnsupportedContext=>"Acest context Claude folosește o metodă de autentificare sau o politică incompatibilă cu schimbarea conturilor.",
            Self::VaultIntegrity=>"Datele criptate nu au putut fi autentificate sau cheia lipsește. Fișierele existente sunt păstrate.",
            Self::MissingAccount=>"Contul nu mai este disponibil.",
            Self::ExternalChange=>"Autentificarea Claude s-a schimbat. Reîmprospătează înainte de a continua.",
            Self::Identity=>"Identitatea tokenului nu a putut fi verificată pentru acest cont.",
            Self::Provider=>"Claude nu a furnizat o citire proaspătă. Reîncearcă mai târziu.",
            Self::Settings=>"Setările introduse nu sunt valide.",
            Self::ReadOnly=>"Această vizualizare este doar pentru citire.",
            Self::Login=>"Autentificarea a expirat sau a fost anulată.",
            Self::Import=>"Importul nu mai este valid. Selectează din nou dosarul.",
            Self::PendingReset=>"Cererea de reset anterioară trebuie reconciliată înainte de o nouă încercare.",
            Self::CliVersion=>"Versiunea Claude Code nu a putut fi detectată. Instalează o versiune recunoscută.",
        }.into()
    }
}

impl From<switcher_platform::PlatformError> for RuntimeError {
    fn from(e: switcher_platform::PlatformError) -> Self {
        match e {
            switcher_platform::PlatformError::Conflict => Self::ExternalChange,
            switcher_platform::PlatformError::KeyUnavailable
            | switcher_platform::PlatformError::UnsupportedSecretService => {
                Self::UnavailableStorage
            }
            switcher_platform::PlatformError::UnsupportedContext => Self::UnsupportedContext,
            switcher_platform::PlatformError::Authentication
            | switcher_platform::PlatformError::KeyLost => Self::VaultIntegrity,
            _ => Self::Storage,
        }
    }
}
impl From<ClientError> for RuntimeError {
    fn from(_: ClientError) -> Self {
        Self::Provider
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Reading {
    scoped_at: Option<i64>,
    complete: bool,
    error: Option<String>,
    priming_attempt_at: Option<i64>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Persisted {
    accounts: Vec<StoredAccount>,
    settings: Settings,
    readings: BTreeMap<String, Reading>,
    last_refresh_at: Option<i64>,
}
struct PreviewFile {
    account: StoredAccount,
}
#[derive(PartialEq, Eq)]
struct LegacyEntry {
    path: PathBuf,
    len: u64,
    is_file: bool,
    is_symlink: bool,
    modified: Option<std::time::SystemTime>,
}
struct Preview {
    id: String,
    created_at: i64,
    directory: PathBuf,
    manifest: Vec<LegacyEntry>,
    sources: Vec<(PathBuf, Vec<u8>)>,
    files: Vec<PreviewFile>,
}
struct Engine {
    codex: CodexEngine,
    saved: Persisted,
    vault: Option<Vault>,
    active: Option<ActiveStore>,
    client: Option<ClaudeClient>,
    clock: Arc<dyn Clock>,
    spacing: bool,
    active_id: Option<String>,
    active_model: Option<String>,
    fingerprint: Option<String>,
    pending_login: Option<PendingLogin>,
    login_intent: Arc<AtomicU64>,
    preview: Option<Preview>,
    revision: u64,
    error: Option<String>,
    demo: bool,
    exhausted: bool,
    last_exhausted_notice: Option<i64>,
    last_full_at: Option<i64>,
    notifications: broadcast::Sender<Notification>,
}
struct Inner {
    owner: Mutex<Engine>,
    cache: RwLock<Snapshot>,
    codex_cache: RwLock<CodexSnapshot>,
    codex_snapshots: broadcast::Sender<CodexSnapshot>,
    codex_intent: Arc<AtomicU64>,
    /// Serializes multi-phase Codex work (quota readers, switches) whose slow parts
    /// run outside the owner lock.
    codex_job: Mutex<()>,
    codex_scheduler_started: AtomicBool,
    snapshots: broadcast::Sender<Snapshot>,
    notifications: broadcast::Sender<Notification>,
    refresh_generation: AtomicU64,
    account_refresh_generation: RwLock<BTreeMap<String, u64>>,
    login_intent: Arc<AtomicU64>,
    login_id: RwLock<Option<String>>,
    scheduler_started: AtomicBool,
}
#[derive(Clone)]
pub struct RuntimeHandle {
    inner: Arc<Inner>,
}
enum Command {
    RefreshAll,
    RefreshAccount(String, u64),
    Switch(String),
    Delete(String),
    Renewal(String, Option<u8>),
    Settings(Settings),
    ImportCurrent,
    BeginLogin(u64),
    FinishLogin(String, String, u64),
    CancelLogin(String),
    Preview(PathBuf),
    ApplyPreview(String),
    Tick(bool),
}
enum Output {
    Snapshot(Snapshot),
    Login(LoginSession),
    Preview(ImportPreview),
}

impl RuntimeHandle {
    /// Production entry. Discovery/storage recovery is local; provider requests begin
    /// only on the serialized scheduler or an explicitly requested mutation.
    pub async fn open() -> Result<Self, RuntimeError> {
        let paths = CliPaths::discover()?;
        let vault_directory = app_data_dir()?;
        let vault = tokio::task::spawn_blocking(move || Vault::open(vault_directory))
            .await
            .map_err(|_| RuntimeError::UnavailableStorage)??;
        let active = ActiveStore::new(paths);
        let version = provider_claude::detect_cli_version()
            .unwrap_or_else(|| provider_claude::FALLBACK_CLI_VERSION.into());
        let handle = Self::construct(
            vault,
            active,
            ClaudeClient::new(&version)?,
            Arc::new(SystemClock),
            true,
        )?;
        handle.start_scheduler();
        Ok(handle)
    }
    pub fn from_parts(
        vault: Vault,
        active: ActiveStore,
        client: ClaudeClient,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, RuntimeError> {
        Self::construct(vault, active, client, clock, false)
    }
    fn construct(
        vault: Vault,
        active: ActiveStore,
        client: ClaudeClient,
        clock: Arc<dyn Clock>,
        spacing: bool,
    ) -> Result<Self, RuntimeError> {
        active.recover(&vault)?;
        let mut saved = vault.load::<Persisted>(RECORD)?.unwrap_or_default();
        saved
            .settings
            .validate()
            .map_err(|_| RuntimeError::Settings)?;
        // A cached owner assertion from another process is not a runtime proof.
        for account in &mut saved.accounts {
            account.identity_verified = false;
        }
        let login_intent = Arc::new(AtomicU64::new(0));
        let (notifications, _) = broadcast::channel(64);
        let (snapshots, _) = broadcast::channel(64);
        let codex = CodexEngine::load(&vault);
        let codex_intent = codex.intent.clone();
        let (codex_snapshots, _) = broadcast::channel(64);
        let codex_snapshot = codex.snapshot(false, clock.now());
        let mut engine = Engine {
            codex,
            saved,
            vault: Some(vault),
            active: Some(active),
            client: Some(client),
            clock,
            spacing,
            active_id: None,
            active_model: None,
            fingerprint: None,
            pending_login: None,
            login_intent: login_intent.clone(),
            preview: None,
            revision: 0,
            error: None,
            demo: false,
            exhausted: false,
            last_exhausted_notice: None,
            last_full_at: None,
            notifications: notifications.clone(),
        };
        engine.observe_active()?;
        let snapshot = engine.snapshot(false);
        Ok(Self {
            inner: Arc::new(Inner {
                owner: Mutex::new(engine),
                cache: RwLock::new(snapshot),
                codex_cache: RwLock::new(codex_snapshot),
                codex_snapshots,
                codex_intent,
                codex_job: Mutex::new(()),
                codex_scheduler_started: AtomicBool::new(false),
                snapshots,
                notifications,
                refresh_generation: AtomicU64::new(0),
                account_refresh_generation: RwLock::new(BTreeMap::new()),
                login_intent,
                login_id: RwLock::new(None),
                scheduler_started: AtomicBool::new(false),
            }),
        })
    }
    pub fn demo() -> Self {
        let now = chrono::Utc::now().timestamp();
        let login_intent = Arc::new(AtomicU64::new(0));
        let (notifications, _) = broadcast::channel(64);
        let (snapshots, _) = broadcast::channel(64);
        let mut saved = Persisted::default();
        for (index, name, five, seven) in [
            (0, "Primary account", 72.0, 81.0),
            (1, "Secondary account", 29.0, 57.0),
            (2, "Cont de rezervă", 100.0, 100.0),
        ] {
            let id = format!("demo-{index}");
            saved.accounts.push(StoredAccount {
                id: id.clone(),
                name: name.into(),
                oauth_account: json!({"emailAddress":format!("demo{index}@example.invalid")}),
                identity_verified: true,
                last_usage: Some(UsageResponse {
                    five_hour: UsageWindow {
                        utilization: five,
                        resets_at: Some(now + 3600),
                    },
                    seven_day: UsageWindow {
                        utilization: seven,
                        resets_at: Some(now + 86400 * (index + 1)),
                    },
                    ..UsageResponse::default()
                }),
                last_usage_at: Some(now),
                ..StoredAccount::default()
            });
            saved.readings.insert(
                id,
                Reading {
                    scoped_at: Some(now),
                    complete: true,
                    error: None,
                    priming_attempt_at: None,
                },
            );
        }
        let codex = CodexEngine::demo(now);
        let codex_intent = codex.intent.clone();
        let (codex_snapshots, _) = broadcast::channel(64);
        let codex_snapshot = codex.snapshot(false, now);
        let engine = Engine {
            codex,
            saved,
            vault: None,
            active: None,
            client: None,
            clock: Arc::new(SystemClock),
            spacing: false,
            active_id: Some("demo-0".into()),
            active_model: Some("sonnet".into()),
            fingerprint: None,
            pending_login: None,
            login_intent: login_intent.clone(),
            preview: None,
            revision: 0,
            error: None,
            demo: true,
            exhausted: false,
            last_exhausted_notice: None,
            last_full_at: None,
            notifications: notifications.clone(),
        };
        let snapshot = engine.snapshot(false);
        Self {
            inner: Arc::new(Inner {
                owner: Mutex::new(engine),
                cache: RwLock::new(snapshot),
                codex_cache: RwLock::new(codex_snapshot),
                codex_snapshots,
                codex_intent,
                codex_job: Mutex::new(()),
                codex_scheduler_started: AtomicBool::new(false),
                snapshots,
                notifications,
                refresh_generation: AtomicU64::new(0),
                account_refresh_generation: RwLock::new(BTreeMap::new()),
                login_intent,
                login_id: RwLock::new(None),
                scheduler_started: AtomicBool::new(false),
            }),
        }
    }
    /// Pure cached projection: this never touches home files or the network.
    pub fn get_snapshot(&self) -> Snapshot {
        self.inner
            .cache
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Snapshot> {
        self.inner.snapshots.subscribe()
    }
    /// A degraded window remains usable without opening any credentials or stores.
    pub fn unavailable(error: RuntimeError) -> Self {
        let handle = Self::demo();
        {
            let mut engine = handle
                .inner
                .owner
                .try_lock()
                .expect("new owner is unlocked");
            engine.saved.accounts.clear();
            engine.saved.readings.clear();
            engine.active_id = None;
            engine.active_model = None;
            engine.demo = false;
            engine.codex.unavailable();
            engine.error = Some(error.message(&engine.saved.settings.language));
            handle.publish(&mut engine, false);
        }
        handle
    }
    pub fn notifications(&self) -> broadcast::Receiver<Notification> {
        self.inner.notifications.subscribe()
    }
    pub fn start_scheduler(&self) {
        if self.inner.scheduler_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let handle = Self { inner };
                let ordinary = {
                    let engine = handle.inner.owner.lock().await;
                    let now = engine.clock.now();
                    engine.last_full_at.is_none_or(|last| {
                        now < last || now - last >= engine.saved.settings.poll_interval as i64
                    })
                };
                let _ = handle.run(Command::Tick(ordinary), None).await;
                let delay = {
                    let engine = handle.inner.owner.lock().await;
                    let active = engine
                        .active_id
                        .as_ref()
                        .and_then(|id| engine.saved.accounts.iter().find(|a| &a.id == id))
                        .and_then(|a| a.last_usage.as_ref());
                    next_poll_delay(
                        engine.saved.settings.poll_interval,
                        active,
                        engine.saved.settings.threshold,
                        engine.active_model.as_deref(),
                        engine.exhausted,
                    )
                };
                drop(handle);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        });
    }
    fn publish(&self, engine: &mut Engine, busy: bool) {
        engine.revision = engine.revision.saturating_add(1);
        let snapshot = engine.snapshot(busy);
        *self.inner.cache.write().unwrap_or_else(|p| p.into_inner()) = snapshot.clone();
        let _ = self.inner.snapshots.send(snapshot);
        self.publish_codex(engine, busy);
    }
    fn publish_codex(&self, engine: &mut Engine, busy: bool) {
        engine.codex.revision = engine.codex.revision.saturating_add(1);
        let snapshot = engine.codex.snapshot(busy, engine.clock.now());
        *self
            .inner
            .codex_cache
            .write()
            .unwrap_or_else(|p| p.into_inner()) = snapshot.clone();
        let _ = self.inner.codex_snapshots.send(snapshot);
    }
    pub fn get_codex_snapshot(&self) -> CodexSnapshot {
        self.inner
            .codex_cache
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn subscribe_codex(&self) -> broadcast::Receiver<CodexSnapshot> {
        self.inner.codex_snapshots.subscribe()
    }
    async fn run(&self, command: Command, coalesce: Option<u64>) -> Result<Output, RuntimeError> {
        let mut engine = self.inner.owner.lock().await;
        if engine.demo || engine.vault.is_none() {
            return Err(RuntimeError::ReadOnly);
        }
        if coalesce.is_some_and(|generation| {
            generation != self.inner.refresh_generation.load(Ordering::Acquire)
        }) {
            return Ok(Output::Snapshot(self.get_snapshot()));
        }
        let account_refresh = if let Command::RefreshAccount(id, generation) = &command {
            if self
                .inner
                .account_refresh_generation
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .get(id)
                .copied()
                .unwrap_or(0)
                != *generation
            {
                return Ok(Output::Snapshot(self.get_snapshot()));
            }
            Some(id.clone())
        } else {
            None
        };
        let full = matches!(command, Command::RefreshAll | Command::Tick(true));
        engine.error = None;
        self.publish(&mut engine, true);
        let result = engine.command(command).await;
        *self
            .inner
            .login_id
            .write()
            .unwrap_or_else(|p| p.into_inner()) =
            engine.pending_login.as_ref().map(|p| p.id.clone());
        let result = match engine.persist() {
            Ok(()) => result,
            Err(e) => Err(e),
        };
        if let Err(e) = &result {
            engine.error = Some(e.message(&engine.saved.settings.language));
        }
        self.publish(&mut engine, false);
        if full {
            self.inner
                .refresh_generation
                .fetch_add(1, Ordering::Release);
        }
        if full || account_refresh.is_some() {
            let mut generations = self
                .inner
                .account_refresh_generation
                .write()
                .unwrap_or_else(|p| p.into_inner());
            let ids = if full {
                engine.saved.accounts.iter().map(|a| a.id.clone()).collect()
            } else {
                account_refresh.into_iter().collect::<Vec<_>>()
            };
            for id in ids {
                let generation = generations.entry(id).or_default();
                *generation = generation.saturating_add(1);
            }
        }
        match result {
            Ok(Some(output)) => Ok(output),
            Ok(None) => Ok(Output::Snapshot(self.get_snapshot())),
            Err(e) => Err(e),
        }
    }
    async fn snapshot_command(&self, command: Command) -> Result<Snapshot, RuntimeError> {
        match self.run(command, None).await? {
            Output::Snapshot(s) => Ok(s),
            _ => unreachable!(),
        }
    }
    pub async fn refresh_all(&self) -> Result<Snapshot, RuntimeError> {
        let generation = self.inner.refresh_generation.load(Ordering::Acquire);
        match self.run(Command::RefreshAll, Some(generation)).await? {
            Output::Snapshot(s) => Ok(s),
            _ => unreachable!(),
        }
    }
    pub async fn refresh_account(&self, id: &str) -> Result<Snapshot, RuntimeError> {
        let generation = self
            .inner
            .account_refresh_generation
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .copied()
            .unwrap_or(0);
        self.snapshot_command(Command::RefreshAccount(id.into(), generation))
            .await
    }
    pub async fn switch_account(&self, id: &str) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::Switch(id.into())).await
    }
    pub async fn delete_account(&self, id: &str) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::Delete(id.into())).await
    }
    pub async fn set_renewal_day(
        &self,
        id: &str,
        day: Option<u8>,
    ) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::Renewal(id.into(), day))
            .await
    }
    pub async fn update_settings(
        &self,
        settings: impl Into<Settings>,
    ) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::Settings(settings.into()))
            .await
    }
    pub async fn import_current(&self) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::ImportCurrent).await
    }
    pub async fn begin_login(&self) -> Result<LoginSession, RuntimeError> {
        match self
            .run(
                Command::BeginLogin(self.inner.login_intent.fetch_add(1, Ordering::AcqRel) + 1),
                None,
            )
            .await?
        {
            Output::Login(s) => Ok(s),
            _ => unreachable!(),
        }
    }
    pub async fn finish_login(&self, id: &str, code: &str) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::FinishLogin(
            id.into(),
            code.into(),
            self.inner.login_intent.load(Ordering::Acquire),
        ))
        .await
    }
    pub async fn cancel_login(&self, id: &str) -> Result<Snapshot, RuntimeError> {
        if self
            .inner
            .login_id
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .as_deref()
            == Some(id)
        {
            self.inner.login_intent.fetch_add(1, Ordering::AcqRel);
        }
        self.snapshot_command(Command::CancelLogin(id.into())).await
    }
    /// Path must come from the native backend folder picker, never frontend IPC.
    pub async fn preview_legacy_import(
        &self,
        path: PathBuf,
    ) -> Result<ImportPreview, RuntimeError> {
        match self.run(Command::Preview(path), None).await? {
            Output::Preview(p) => Ok(p),
            _ => unreachable!(),
        }
    }
    pub async fn apply_legacy_import(&self, id: &str) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::ApplyPreview(id.into()))
            .await
    }
    /// Explicitly controlled fixture scheduler entry; ordinary=false is inference-only.
    pub async fn tick(&self, ordinary: bool) -> Result<Snapshot, RuntimeError> {
        self.snapshot_command(Command::Tick(ordinary)).await
    }
}

impl Engine {
    fn persist(&self) -> Result<(), RuntimeError> {
        self.vault
            .as_ref()
            .ok_or(RuntimeError::ReadOnly)?
            .save(RECORD, &self.saved)?;
        Ok(())
    }
    fn index(&self, id: &str) -> Result<usize, RuntimeError> {
        self.saved
            .accounts
            .iter()
            .position(|a| a.id == id && a.provider == ProviderId::Claude)
            .ok_or(RuntimeError::MissingAccount)
    }
    fn active_store(&self) -> Result<&ActiveStore, RuntimeError> {
        self.active.as_ref().ok_or(RuntimeError::ReadOnly)
    }
    fn client(&self) -> Result<ClaudeClient, RuntimeError> {
        self.client.clone().ok_or(RuntimeError::ReadOnly)
    }
    fn live(&self) -> Result<ActiveSnapshot, RuntimeError> {
        Ok(self.active_store()?.read()?)
    }
    fn observe_active(&mut self) -> Result<(), RuntimeError> {
        let live = self.live()?;
        let owner = live.identity.get("accountUuid").and_then(Value::as_str);
        let org = live
            .identity
            .get("organizationUuid")
            .and_then(Value::as_str);
        let active = self
            .saved
            .accounts
            .iter()
            .find(|a| {
                a.account_uuid() == owner
                    && owner.is_some()
                    && a.organization_uuid() == org
                    && a.provider == ProviderId::Claude
            })
            .map(|a| a.id.clone());
        if self
            .fingerprint
            .as_ref()
            .is_some_and(|fp| fp != &live.fingerprint)
        {
            for a in &mut self.saved.accounts {
                a.identity_verified = false;
            }
            for r in self.saved.readings.values_mut() {
                r.complete = false;
            }
        }
        self.active_id = active;
        self.fingerprint = Some(live.fingerprint);
        self.active_model = self.active_store()?.paths.default_model()?;
        Ok(())
    }
    fn fresh(&self, a: &StoredAccount) -> bool {
        let now = self.clock.now();
        let Some(r) = self.saved.readings.get(&a.id) else {
            return false;
        };
        a.identity_verified
            && r.complete
            && r.error.is_none()
            && recent(a.last_usage_at, now, USAGE_MAX_AGE)
            && (self.active_model.is_none() || recent(r.scoped_at, now, SCOPED_MAX_AGE))
    }
    fn infos(&self, decisions: bool) -> Vec<AccountUsageInfo> {
        self.saved
            .accounts
            .iter()
            .filter(|a| a.provider == ProviderId::Claude)
            .map(|a| AccountUsageInfo {
                account: a.clone(),
                usage: if !decisions || self.fresh(a) {
                    a.last_usage.clone()
                } else {
                    None
                },
                is_active: self.active_id.as_deref() == Some(&a.id),
            })
            .collect()
    }
    fn snapshot(&self, busy: bool) -> Snapshot {
        let now = self.clock.now();
        let infos = self.infos(false);
        let next = best_candidate(
            &infos,
            self.saved.settings.threshold,
            self.active_model.as_deref(),
        );
        let plan = consumption_plan(
            &infos,
            self.saved.settings.threshold,
            self.active_model.as_deref(),
        );
        let accounts = self
            .saved
            .accounts
            .iter()
            .filter(|a| a.provider == ProviderId::Claude)
            .map(|a| {
                let usage = a.last_usage.as_ref();
                let offers = a.reset_status.as_ref().map(|s| s.available(now));
                let available = offers.as_ref().map(|gs| {
                    gs.iter()
                        .fold(0_u32, |sum, g| sum.saturating_add(g.resets_left))
                });
                let exhausted = usage.is_some_and(|u| {
                    u.five_hour.utilization >= self.saved.settings.threshold
                        || u.weekly_utilization_for_model(self.active_model.as_deref())
                            >= self.saved.settings.threshold
                });
                AccountView {
                    id: a.id.clone(),
                    name: safe_label(&a.name),
                    email: safe_label(a.email().unwrap_or(&a.name)),
                    provider: a.provider,
                    active: self.active_id.as_deref() == Some(&a.id),
                    is_next: next.as_deref() == Some(&a.id),
                    exhausted,
                    identity_verified: a.identity_verified,
                    usage: usage.map(|u| UsageView::new(u, self.active_model.as_deref())),
                    usage_at: a.last_usage_at,
                    scoped_at: self.saved.readings.get(&a.id).and_then(|r| r.scoped_at),
                    decision_fresh: self.fresh(a),
                    error: self.saved.readings.get(&a.id).and_then(|r| {
                        r.error
                            .as_deref()
                            .map(|text| safe_error_text(text, &self.saved.settings.language))
                    }),
                    subscription_status: a.subscription_status.as_deref().map(safe_label),
                    plan_tier: a.plan_tier.as_deref().map(safe_label),
                    renewal_day: a.renewal_day,
                    next_renewal_at: next_renewal(a, now),
                    resets: ResetView {
                        available,
                        expires_at: offers
                            .as_ref()
                            .and_then(|gs| gs.iter().filter_map(|g| g.ends_at).min()),
                        cooldown_until: a.reset_status.as_ref().and_then(|s| s.cooldown_until),
                        pending: a.pending_reset_claim.is_some(),
                        last_outcome: a
                            .last_reset_outcome
                            .as_ref()
                            .map(|text| safe_reset_text(text, &self.saved.settings.language)),
                    },
                    primed_at: a.primed_for_reset_at,
                }
            })
            .collect();
        Snapshot {
            revision: self.revision,
            provider: ProviderId::Claude,
            accounts,
            active_id: self.active_id.clone(),
            active_model: self.active_model.clone(),
            settings: SettingsView::from(&self.saved.settings),
            last_refresh_at: self.saved.last_refresh_at,
            busy,
            error: self.error.clone(),
            demo: self.demo,
            consumption_plan: plan,
        }
    }
    async fn command(&mut self, command: Command) -> Result<Option<Output>, RuntimeError> {
        match command {
            Command::RefreshAll => self.cycle(true, true).await?,
            Command::Tick(ordinary) => self.cycle(ordinary, false).await?,
            Command::RefreshAccount(id, _) => {
                self.observe_active()?;
                self.poll_account(&id, true, false).await?;
                self.saved.last_refresh_at = Some(self.clock.now());
            }
            Command::Switch(id) => {
                self.switch_to(&id, false, false).await?;
            }
            Command::Delete(id) => {
                let i = self.index(&id)?;
                self.saved.accounts.remove(i);
                self.saved.readings.remove(&id);
                if self.active_id.as_deref() == Some(&id) {
                    self.active_id = None;
                }
            }
            Command::Renewal(id, day) => {
                if day.is_some_and(|d| !(1..=31).contains(&d)) {
                    return Err(RuntimeError::Settings);
                }
                let i = self.index(&id)?;
                self.saved.accounts[i].renewal_day = day;
            }
            Command::Settings(settings) => {
                settings.validate().map_err(|_| RuntimeError::Settings)?;
                self.saved.settings = settings;
            }
            Command::ImportCurrent => self.import_current().await?,
            Command::BeginLogin(epoch) => {
                if epoch != self.login_intent.load(Ordering::Acquire) {
                    return Err(RuntimeError::Login);
                }
                let login = PendingLogin::new(self.clock.now());
                let output = LoginSession {
                    id: login.id.clone(),
                    url: login.url.clone(),
                };
                self.pending_login = Some(login);
                return Ok(Some(Output::Login(output)));
            }
            Command::FinishLogin(id, code, epoch) => {
                if self.pending_login.as_ref().is_none_or(|p| p.id != id)
                    || epoch != self.login_intent.load(Ordering::Acquire)
                {
                    return Err(RuntimeError::Login);
                }
                let pending = self
                    .pending_login
                    .take()
                    .filter(|p| p.id == id)
                    .ok_or(RuntimeError::Login)?;
                let login = self
                    .client()?
                    .finish_login(&pending, &code, self.clock.now())
                    .await
                    .map_err(|_| RuntimeError::Login)?;
                if epoch != self.login_intent.load(Ordering::Acquire) {
                    return Err(RuntimeError::Login);
                }
                let mut account = StoredAccount::new(
                    login
                        .profile
                        .identity
                        .get("emailAddress")
                        .and_then(Value::as_str)
                        .unwrap_or("Claude account"),
                    login.profile.identity.clone(),
                    login.credentials,
                    self.clock.now(),
                );
                login.profile.apply(&mut account, self.clock.now());
                self.merge_account(account, true);
            }
            Command::CancelLogin(id) => {
                if self.pending_login.as_ref().is_some_and(|p| p.id == id) {
                    self.pending_login = None;
                }
            }
            Command::Preview(path) => {
                return self.preview_import(path).map(|p| Some(Output::Preview(p)));
            }
            Command::ApplyPreview(id) => self.apply_preview(&id)?,
        }
        Ok(None)
    }
    fn t<'a>(&self, en: &'a str, ro: &'a str) -> &'a str {
        if self.saved.settings.language == "ro" {
            ro
        } else {
            en
        }
    }
    fn notify(&self, body: String) {
        let _ = self.notifications.send(Notification {
            title: "PrimerSwitch".into(),
            body,
        });
    }
}
fn recent(at: Option<i64>, now: i64, age: i64) -> bool {
    at.is_some_and(|at| at <= now + 30 && now.saturating_sub(at) <= age)
}
fn same_owner(a: &StoredAccount, identity: &Value) -> bool {
    a.account_uuid().is_some()
        && a.account_uuid() == identity.get("accountUuid").and_then(Value::as_str)
        && a.organization_uuid() == identity.get("organizationUuid").and_then(Value::as_str)
}
fn safe_label(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}

impl Engine {
    fn ensure_context(&mut self, expected: &str) -> Result<(), RuntimeError> {
        if self.live()?.fingerprint != expected {
            self.observe_active()?;
            return Err(RuntimeError::ExternalChange);
        }
        Ok(())
    }
    async fn verify_profile(
        &mut self,
        id: &str,
        token: &str,
        expected: &str,
    ) -> Result<(), RuntimeError> {
        let profile = self.client()?.profile(token).await;
        self.ensure_context(expected)?;
        let profile = profile.map_err(|_| RuntimeError::Identity)?;
        let i = self.index(id)?;
        if !same_owner(&self.saved.accounts[i], &profile.identity) {
            self.saved.accounts[i].identity_verified = false;
            self.saved.readings.entry(id.into()).or_default().complete = false;
            self.persist()?;
            return Err(RuntimeError::Identity);
        }
        profile.apply(&mut self.saved.accounts[i], self.clock.now());
        Ok(())
    }
    async fn adopt_active(&mut self, id: &str, expected: &str) -> Result<String, RuntimeError> {
        let live = self.live()?;
        if live.fingerprint != expected {
            return Err(RuntimeError::ExternalChange);
        }
        let i = self.index(id)?;
        if !same_owner(&self.saved.accounts[i], &live.identity) {
            return Err(RuntimeError::Identity);
        }
        let token = live
            .credentials
            .get("claudeAiOauth")
            .and_then(|v| v.get("accessToken"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(RuntimeError::Identity)?
            .to_owned();
        let needs_profile = !self.saved.accounts[i].identity_verified
            || self.saved.accounts[i].access_token() != Some(&token)
            || !recent(
                self.saved.accounts[i].profile_checked_at,
                self.clock.now(),
                86400,
            );
        if needs_profile {
            self.verify_profile(id, &token, expected).await?;
        }
        self.ensure_context(expected)?;
        // Only the CLI OAuth blob is adopted; saved credential extensions survive.
        let i = self.index(id)?;
        if !self.saved.accounts[i].credentials.is_object() {
            self.saved.accounts[i].credentials = json!({});
        }
        if self.saved.accounts[i].credentials.get("claudeAiOauth")
            != live.credentials.get("claudeAiOauth")
        {
            self.saved.accounts[i].credentials["claudeAiOauth"] =
                live.credentials["claudeAiOauth"].clone();
            self.saved.accounts[i].saved_at = self.clock.now();
            self.persist()?;
        }
        Ok(token)
    }
    async fn refresh_inactive(
        &mut self,
        id: &str,
        forced: bool,
        expected: &str,
    ) -> Result<(), RuntimeError> {
        self.ensure_context(expected)?;
        let i = self.index(id)?;
        let live = self.live()?;
        if same_owner(&self.saved.accounts[i], &live.identity) {
            return Err(RuntimeError::ExternalChange);
        }
        let now = self.clock.now();
        if !forced
            && self.saved.accounts[i]
                .expires_at_ms()
                .is_none_or(|t| t > now.saturating_add(120).saturating_mul(1000))
        {
            return Ok(());
        }
        if recent(self.saved.accounts[i].refresh_fail_at, now, 900) {
            return Err(RuntimeError::Provider);
        }
        let refresh = self.saved.accounts[i]
            .refresh_token()
            .ok_or(RuntimeError::Identity)?
            .to_owned();
        let result = self.client()?.refresh_token(&refresh).await;
        let i = self.index(id)?;
        match result {
            Ok(tokens) => {
                tokens.apply_account(&mut self.saved.accounts[i], self.clock.now());
                self.saved.accounts[i].identity_verified = false;
                self.persist()?;
                self.ensure_context(expected)?;
                Ok(())
            }
            Err(_) => {
                self.saved.accounts[i].refresh_fail_at = Some(now);
                self.persist()?;
                self.ensure_context(expected)?;
                Err(RuntimeError::Provider)
            }
        }
    }
    fn endpoint_allowed(&self, id: &str, force: bool) -> bool {
        let Ok(i) = self.index(id) else {
            return false;
        };
        let a = &self.saved.accounts[i];
        let now = self.clock.now();
        if a.rate_limited_until.is_some_and(|until| until > now) {
            return false;
        }
        if recent(a.last_endpoint_attempt_at, now, 120) {
            return false;
        }
        force
            || !recent(
                if self.active_id.as_deref() == Some(id) {
                    a.last_endpoint_read_at
                } else {
                    a.last_endpoint_read_at.or(a.last_usage_at)
                },
                now,
                if self.active_id.as_deref() == Some(id) {
                    1800
                } else {
                    900
                },
            )
    }
    fn endpoint_error(&mut self, id: &str, error: &ClientError) -> Result<(), RuntimeError> {
        let i = self.index(id)?;
        let now = self.clock.now();
        if let ClientError::RateLimited { retry_after } = error {
            let a = &mut self.saved.accounts[i];
            a.consecutive_rate_limits = a.consecutive_rate_limits.saturating_add(1);
            let exponent = a.consecutive_rate_limits.saturating_sub(1).min(4);
            let backoff = (60_u64.saturating_mul(1 << exponent))
                .min(900)
                .max(retry_after.unwrap_or(0))
                .min(i64::MAX as u64);
            a.rate_limited_until = Some(now.saturating_add(backoff as i64));
        }
        Ok(())
    }
    async fn metadata(&mut self, id: &str, expected: &str) -> Result<UsageResponse, RuntimeError> {
        let i = self.index(id)?;
        self.saved.accounts[i].last_endpoint_attempt_at = Some(self.clock.now());
        self.persist()?;
        let token = self.saved.accounts[i]
            .access_token()
            .ok_or(RuntimeError::Identity)?
            .to_owned();
        let mut result = self.client()?.metadata(&token).await;
        self.ensure_context(expected)?;
        if matches!(result, Err(ClientError::Unauthorized)) && self.active_id.as_deref() != Some(id)
        {
            // One bounded retry; rotated tokens are durable before another request.
            self.refresh_inactive(id, true, expected).await?;
            let i = self.index(id)?;
            let token = self.saved.accounts[i]
                .access_token()
                .ok_or(RuntimeError::Identity)?
                .to_owned();
            self.verify_profile(id, &token, expected).await?;
            result = self.client()?.metadata(&token).await;
            self.ensure_context(expected)?;
        }
        match result {
            Ok(usage) => {
                let now = self.clock.now();
                let i = self.index(id)?;
                let a = &mut self.saved.accounts[i];
                a.last_endpoint_read_at = Some(now);
                a.consecutive_rate_limits = 0;
                a.rate_limited_until = None;
                a.reset_status = usage.cedar_ember.clone();
                a.reset_status_at = Some(now);
                self.saved.readings.entry(id.into()).or_default().scoped_at = Some(now);
                self.reconcile_pending(id)?;
                Ok(usage)
            }
            Err(error) => {
                self.endpoint_error(id, &error)?;
                Err(RuntimeError::Provider)
            }
        }
    }
    async fn poll_account(
        &mut self,
        id: &str,
        force: bool,
        fast: bool,
    ) -> Result<(), RuntimeError> {
        let expected = self.live()?.fingerprint;
        let active = self.active_id.as_deref() == Some(id);
        let i = self.index(id)?;
        if self.saved.accounts[i].provider != ProviderId::Claude {
            return Err(RuntimeError::MissingAccount);
        }
        let result: Result<(), RuntimeError> = async {
            let token = if active {
                self.adopt_active(id, &expected).await?
            } else {
                self.refresh_inactive(id, false, &expected).await?;
                let i = self.index(id)?;
                let token = self.saved.accounts[i]
                    .access_token()
                    .ok_or(RuntimeError::Identity)?
                    .to_owned();
                if !self.saved.accounts[i].identity_verified
                    || !recent(
                        self.saved.accounts[i].profile_checked_at,
                        self.clock.now(),
                        86400,
                    )
                {
                    self.verify_profile(id, &token, &expected).await?;
                }
                token
            };
            let mut live_usage = None;
            let mut live_complete = false;
            if active || fast {
                let i = self.index(id)?;
                let previous = self.saved.accounts[i].last_usage.clone();
                match self.client()?.inference(&token, previous.as_ref()).await {
                    Ok(read) => {
                        self.ensure_context(&expected)?;
                        let i = self.index(id)?;
                        let mut usage = read.usage;
                        live_complete = read.complete;
                        // Live account-wide windows always win over metadata. Only
                        // scoped rows are carried; freshness is tracked independently.
                        usage.limits = previous
                            .as_ref()
                            .and_then(|u| u.limits.clone())
                            .map(|ls| ls.into_iter().filter(|l| l.kind != "weekly_all").collect());
                        usage.seven_day_sonnet =
                            previous.as_ref().and_then(|u| u.seven_day_sonnet.clone());
                        self.saved.accounts[i].last_usage = Some(usage.clone());
                        self.saved.accounts[i].last_usage_at = Some(self.clock.now());
                        self.saved.readings.entry(id.into()).or_default().complete = read.complete;
                        if read.accepted {
                            self.resolve_priming_by_inference(id)?;
                        }
                        live_usage = Some(usage);
                    }
                    Err(_) => {
                        self.ensure_context(&expected)?;
                        self.saved.readings.entry(id.into()).or_default().complete = false;
                        if fast {
                            return Err(RuntimeError::Provider);
                        }
                    }
                }
            }
            if !fast && self.endpoint_allowed(id, force || (active && live_usage.is_none())) {
                match self.metadata(id, &expected).await {
                    Ok(metadata) => {
                        let i = self.index(id)?;
                        let usage = if let Some(mut live) = live_usage.clone() {
                            if !live_complete {
                                if live.five_hour.utilization < 100.0 {
                                    live.five_hour = metadata.five_hour.clone();
                                }
                                if live.seven_day.utilization < 100.0 {
                                    live.seven_day = metadata.seven_day.clone();
                                }
                            }
                            live.limits = metadata.limits.map(|ls| {
                                ls.into_iter().filter(|l| l.kind != "weekly_all").collect()
                            });
                            live.seven_day_sonnet = metadata.seven_day_sonnet;
                            live.extra_usage = metadata.extra_usage;
                            live.cedar_ember = metadata.cedar_ember;
                            live
                        } else {
                            metadata
                        };
                        self.saved.accounts[i].last_usage = Some(usage);
                        self.saved.accounts[i].last_usage_at = Some(self.clock.now());
                        self.saved.readings.entry(id.into()).or_default().complete = true;
                    }
                    Err(error) => {
                        if live_usage.is_none() {
                            return Err(error);
                        }
                    }
                }
            } else if !active && !fast && !self.fresh(&self.saved.accounts[self.index(id)?]) {
                return Err(RuntimeError::Provider);
            }
            let i = self.index(id)?;
            if self.saved.accounts[i].last_usage.is_none() {
                return Err(RuntimeError::Provider);
            }
            self.saved.readings.entry(id.into()).or_default().error = None;
            if !fast {
                self.prime(id, &expected).await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = &result {
            self.saved.readings.entry(id.into()).or_default().error =
                Some(error.message(&self.saved.settings.language));
        }
        self.persist()?;
        result
    }
    async fn cycle(&mut self, ordinary: bool, force: bool) -> Result<(), RuntimeError> {
        self.observe_active()?;
        let now = self.clock.now();
        let mut ids = vec![];
        if let Some(active) = self.active_id.clone() {
            ids.push(active);
        }
        if ordinary {
            self.last_full_at = Some(now);
            for a in &self.saved.accounts {
                if a.provider == ProviderId::Claude
                    && self.active_id.as_deref() != Some(&a.id)
                    && (force || !a.identity_verified || !recent(a.last_usage_at, now, 900))
                {
                    ids.push(a.id.clone());
                }
            }
        }
        let mut last_error = None;
        for (index, id) in ids.iter().enumerate() {
            if index > 0 && self.spacing {
                tokio::time::sleep(Duration::from_secs(8)).await;
            }
            if let Err(error) = self.poll_account(id, force, !ordinary).await {
                last_error = Some(error);
            }
        }
        self.saved.last_refresh_at = Some(self.clock.now());
        // Freshness failures keep cached figures visible but suspend decisions.
        self.automate(!ordinary).await?;
        if self.active_id.as_ref().is_some_and(|id| {
            self.saved
                .readings
                .get(id)
                .is_some_and(|r| r.error.is_some())
        }) && let Some(error) = last_error
        {
            return Err(error);
        }
        Ok(())
    }
    async fn switch_to(
        &mut self,
        id: &str,
        fast: bool,
        require_usable: bool,
    ) -> Result<(), RuntimeError> {
        self.observe_active()?;
        let expected = self.live()?.fingerprint;
        self.index(id)?;
        if self.active_id.as_deref() == Some(id) {
            return Ok(());
        }
        self.poll_account(id, true, fast).await?;
        self.ensure_context(&expected)?;
        let i = self.index(id)?;
        if !self.saved.accounts[i].identity_verified {
            return Err(RuntimeError::Identity);
        }
        let target = self.saved.accounts[i].clone();
        if require_usable {
            let reading = self.saved.readings.get(id).ok_or(RuntimeError::Provider)?;
            let info = AccountUsageInfo {
                account: target.clone(),
                usage: target.last_usage.clone(),
                is_active: false,
            };
            if !self.fresh(&target)
                || !recent(target.last_usage_at, self.clock.now(), 120)
                || (self.active_model.is_some()
                    && !recent(reading.scoped_at, self.clock.now(), 120))
                || !is_usable(
                    &info,
                    self.saved.settings.threshold,
                    self.active_model.as_deref(),
                )
            {
                return Err(RuntimeError::Provider);
            }
        }
        // Capture and verify outgoing CLI rotation immediately before CAS switching.
        if let Some(outgoing) = self.active_id.clone() {
            self.adopt_active(&outgoing, &expected).await?;
        }
        self.ensure_context(&expected)?;
        self.active_store()?.switch_checked(
            &target.credentials,
            &target.oauth_account,
            &expected,
            self.vault.as_ref().ok_or(RuntimeError::Storage)?,
        )?;
        let live = self.live()?;
        self.active_id = Some(id.into());
        self.fingerprint = Some(live.fingerprint);
        self.exhausted = false;
        self.saved.accounts[i].identity_verified = true;
        self.persist()?;
        self.notify(format!(
            "{} {}",
            self.t("Switched to", "Comutat pe"),
            safe_label(target.email().unwrap_or(&target.name))
        ));
        // The next cycle reads the new active token; old action lists are discarded.
        Ok(())
    }
    fn resolve_priming_by_inference(&mut self, id: &str) -> Result<(), RuntimeError> {
        let i = self.index(id)?;
        let now = self.clock.now();
        let a = &mut self.saved.accounts[i];
        if let Some(old) = a.expected_weekly_reset_at
            && old <= now
            && a.last_usage
                .as_ref()
                .and_then(UsageResponse::weekly_all_resets_at)
                .is_some_and(|next| next > old)
        {
            a.primed_for_reset_at = Some(old);
            a.priming_pending_for = None;
        }
        Ok(())
    }
    async fn prime(&mut self, id: &str, expected: &str) -> Result<(), RuntimeError> {
        let i = self.index(id)?;
        let now = self.clock.now();
        let next = self.saved.accounts[i]
            .last_usage
            .as_ref()
            .and_then(UsageResponse::weekly_all_resets_at);
        let Some(old) = self.saved.accounts[i].expected_weekly_reset_at else {
            self.saved.accounts[i].expected_weekly_reset_at = next;
            return Ok(());
        };
        if self.saved.accounts[i].priming_pending_for == Some(old) && next.is_some_and(|n| n > old)
        {
            self.saved.accounts[i].primed_for_reset_at = Some(old);
            self.saved.accounts[i].priming_pending_for = None;
        }
        if old > now || self.saved.accounts[i].primed_for_reset_at == Some(old) {
            if next.is_some() {
                self.saved.accounts[i].expected_weekly_reset_at = next;
            }
            return Ok(());
        }
        if !self.saved.settings.auto_start_window_enabled {
            self.saved.accounts[i].primed_for_reset_at = Some(old);
            self.saved.accounts[i].expected_weekly_reset_at = next;
            return Ok(());
        }
        if !self.saved.accounts[i].identity_verified {
            return Ok(());
        }
        // Reconcile pending attempts with a fresh metadata observation first. A
        // retry is never issued immediately after a crash/ambiguous network reply.
        if self.saved.accounts[i].priming_pending_for.is_some() {
            let last = self
                .saved
                .readings
                .get(id)
                .and_then(|r| r.priming_attempt_at);
            if recent(last, now, 900)
                || !recent(self.saved.accounts[i].last_endpoint_read_at, now, 120)
            {
                return Ok(());
            }
        }
        self.saved.accounts[i].priming_pending_for = Some(old);
        self.saved
            .readings
            .entry(id.into())
            .or_default()
            .priming_attempt_at = Some(now);
        self.persist()?;
        let token = self.saved.accounts[i]
            .access_token()
            .ok_or(RuntimeError::Identity)?
            .to_owned();
        let previous = self.saved.accounts[i].last_usage.clone();
        let result = self.client()?.inference(&token, previous.as_ref()).await;
        self.ensure_context(expected)?;
        let i = self.index(id)?;
        match result {
            Ok(read) if read.accepted => {
                let mut usage = read.usage;
                usage.limits = previous
                    .as_ref()
                    .and_then(|u| u.limits.clone())
                    .map(|ls| ls.into_iter().filter(|l| l.kind != "weekly_all").collect());
                usage.seven_day_sonnet = previous.as_ref().and_then(|u| u.seven_day_sonnet.clone());
                self.saved.accounts[i].last_usage = Some(usage);
                self.saved.accounts[i].last_usage_at = Some(self.clock.now());
                self.saved.readings.entry(id.into()).or_default().complete = read.complete;
                self.saved.accounts[i].primed_for_reset_at = Some(old);
                self.saved.accounts[i].priming_pending_for = None;
                self.saved.accounts[i].expected_weekly_reset_at = self.saved.accounts[i]
                    .last_usage
                    .as_ref()
                    .and_then(UsageResponse::weekly_all_resets_at);
                self.notify(
                    self.t(
                        "The weekly window was started.",
                        "Fereastra săptămânală a fost pornită.",
                    )
                    .into(),
                );
            }
            Ok(_) => {
                self.saved.accounts[i].priming_pending_for = None;
            }
            Err(
                ClientError::Unauthorized
                | ClientError::RateLimited { .. }
                | ClientError::Http(400..=499),
            ) => {
                self.saved.accounts[i].priming_pending_for = None;
            }
            Err(_) => {} // An unknown outcome retains its durable reconciliation marker.
        }
        self.persist()?;
        Ok(())
    }
    fn reconcile_pending(&mut self, id: &str) -> Result<(), RuntimeError> {
        let i = self.index(id)?;
        let a = &mut self.saved.accounts[i];
        let Some(p) = &a.pending_reset_claim else {
            return Ok(());
        };
        let Some(status) = &a.reset_status else {
            return Ok(());
        };
        if !status.grants.iter().any(|g| g.id == p.grant_id) {
            a.last_reset_outcome = Some(ResetOutcome::Transport.text().into());
        }
        Ok(())
    }
    async fn automate(&mut self, fast: bool) -> Result<(), RuntimeError> {
        let now = self.clock.now();
        if self.saved.settings.auto_use_resets_enabled {
            let candidates: Vec<_> = self
                .saved
                .accounts
                .iter()
                .filter(|a| {
                    self.fresh(a)
                        && (a.pending_reset_claim.is_some()
                            || a.reset_status
                                .as_ref()
                                .is_some_and(|s| !s.grants.is_empty()))
                })
                .map(|a| a.id.clone())
                .collect();
            for id in &candidates {
                let i = self.index(id)?;
                if !fast
                    && !recent(self.saved.accounts[i].reset_status_at, now, 120)
                    && self.endpoint_allowed(id, true)
                {
                    let expected = self.live()?.fingerprint;
                    self.metadata(id, &expected).await?;
                }
            }
            for id in candidates {
                let i = self.index(&id)?;
                let Some(pending) = self.saved.accounts[i].pending_reset_claim.clone() else {
                    continue;
                };
                let missing = self.saved.accounts[i]
                    .reset_status
                    .as_ref()
                    .is_some_and(|s| !s.grants.iter().any(|g| g.id == pending.grant_id));
                if missing
                    && recent(self.saved.accounts[i].reset_status_at, now, 120)
                    && self.saved.accounts[i]
                        .last_reset_attempt_at
                        .is_none_or(|last| now.saturating_sub(last) >= 300)
                    && self.claim(&id, &pending.grant_id, true).await?
                    && self.active_id.as_deref() == Some(&id)
                {
                    return Ok(());
                }
            }
        }
        let mut infos = self.infos(true);
        // Reset offers need an independently fresh metadata observation. Unknown
        // inactive budget prevents last-resort exhaustion from being assumed.
        let all_fresh = infos.iter().all(|i| i.usage.is_some());
        for info in &mut infos {
            if !recent(info.account.reset_status_at, now, 120) {
                info.account.reset_status = None;
            }
            if !all_fresh && info.is_active {
                info.usage = None;
            }
        }
        if self.saved.settings.auto_use_resets_enabled {
            for action in decide_resets(
                &infos,
                self.saved.settings.threshold,
                self.active_model.as_deref(),
                now,
            ) {
                let i = self.index(&action.account_id)?;
                if !self.fresh(&self.saved.accounts[i]) {
                    continue;
                }
                if action.switch_first {
                    self.switch_to(&action.account_id, fast, false).await?;
                }
                let reset = self
                    .claim(&action.account_id, &action.grant_id, fast)
                    .await?;
                if reset && self.active_id.as_deref() == Some(&action.account_id) {
                    return Ok(());
                }
                if action.switch_first {
                    return Ok(());
                }
            }
        }
        let infos = self.infos(true);
        let Some(active) = infos.iter().find(|i| i.is_active && i.usage.is_some()) else {
            return Ok(());
        };
        if is_usable(
            active,
            self.saved.settings.threshold,
            self.active_model.as_deref(),
        ) {
            self.exhausted = false;
            return Ok(());
        }
        if let Some(candidate) = best_candidate(
            &infos,
            self.saved.settings.threshold,
            self.active_model.as_deref(),
        ) {
            if self.saved.settings.auto_switch_enabled {
                self.switch_to(&candidate, fast, true).await?;
            }
        } else if infos.iter().all(|i| i.usage.is_some()) {
            self.exhausted = true;
            if self
                .last_exhausted_notice
                .is_none_or(|last| now.saturating_sub(last) >= 1800)
            {
                self.last_exhausted_notice = Some(now);
                let earliest = infos
                    .iter()
                    .filter_map(|i| i.usage.as_ref().and_then(UsageResponse::earliest_reset))
                    .min();
                self.notify(if earliest.is_some() {
                    self.t(
                        "All accounts are at their limits. Waiting for their windows to reopen.",
                        "Toate conturile sunt la limită. Așteptăm redeschiderea ferestrelor.",
                    )
                    .into()
                } else {
                    self.t(
                        "All accounts are at their limits.",
                        "Toate conturile sunt la limită.",
                    )
                    .into()
                });
            }
        }
        Ok(())
    }
    async fn claim(&mut self, id: &str, grant: &str, fast: bool) -> Result<bool, RuntimeError> {
        let expected = self.live()?.fingerprint;
        let i = self.index(id)?;
        if !self.fresh(&self.saved.accounts[i]) {
            return Err(RuntimeError::Provider);
        }
        if !ResetGrant::is_valid_id(grant) {
            return Err(RuntimeError::PendingReset);
        }
        let org = self.saved.accounts[i]
            .organization_uuid()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            .ok_or(RuntimeError::Identity)?
            .to_owned();
        // Ordinary mutation preflight can refresh eligibility. Strict fast ticks
        // use only already-fresh offers and never issue metadata requests.
        if !fast && self.endpoint_allowed(id, true) {
            let _ = self.metadata(id, &expected).await?;
        }
        let i = self.index(id)?;
        let now = self.clock.now();
        let status = self.saved.accounts[i]
            .reset_status
            .as_ref()
            .ok_or(RuntimeError::Provider)?;
        let replay = self.saved.accounts[i]
            .pending_reset_claim
            .as_ref()
            .is_some_and(|p| p.grant_id == grant);
        if !recent(self.saved.accounts[i].reset_status_at, now, 120)
            || (!replay && !status.available(now).iter().any(|g| g.id == grant))
        {
            return Err(RuntimeError::Provider);
        }
        let mut infos = self.infos(true);
        let all_fresh = infos.iter().all(|i| i.usage.is_some());
        for info in &mut infos {
            if !recent(info.account.reset_status_at, now, 120) {
                info.account.reset_status = None;
            }
            if !all_fresh && info.is_active {
                info.usage = None;
            }
        }
        if replay
            && self.saved.accounts[i]
                .last_reset_attempt_at
                .is_some_and(|last| now.saturating_sub(last) < 300)
        {
            return Err(RuntimeError::PendingReset);
        }
        if !replay
            && !decide_resets(
                &infos,
                self.saved.settings.threshold,
                self.active_model.as_deref(),
                now,
            )
            .iter()
            .any(|action| action.account_id == id && action.grant_id == grant)
        {
            return Err(RuntimeError::Provider);
        }
        let pending = prepare_reset_claim(
            grant,
            self.saved.accounts[i].pending_reset_claim.as_ref(),
            now,
        )
        .map_err(|_| RuntimeError::PendingReset)?;
        let token = self.saved.accounts[i]
            .access_token()
            .filter(|s| !s.is_empty())
            .ok_or(RuntimeError::Identity)?
            .to_owned();
        self.saved.accounts[i].pending_reset_claim = Some(pending.clone());
        self.saved.accounts[i].last_reset_attempt_at = Some(now);
        self.persist()?;
        self.ensure_context(&expected)?;
        let outcome = self
            .client()?
            .claim_reset(&org, grant, &pending.request_id, &token)
            .await;
        // Preserve the received outcome on its stable account before checking CLI
        // ownership: an external login cannot erase knowledge of a scarce effect.
        let i = self.index(id)?;
        let a = &mut self.saved.accounts[i];
        apply_reset_outcome(&mut a.reset_status, grant, &outcome);
        a.last_reset_outcome = Some(outcome.text_for(&self.saved.settings.language).into());
        if outcome.is_final() {
            a.pending_reset_claim = None;
        }
        let success = matches!(outcome, ResetOutcome::Reset { .. });
        if success {
            self.saved.readings.entry(id.into()).or_default().complete = false;
            self.saved.accounts[i].last_usage_at = None;
        }
        self.persist()?;
        self.ensure_context(&expected)?;
        if success {
            self.notify(format!(
                "{} {} — {}",
                self.t("Reset used on", "Reset folosit pe"),
                safe_label(
                    self.saved.accounts[i]
                        .email()
                        .unwrap_or(&self.saved.accounts[i].name)
                ),
                self.t("limits refilled", "limite umplute")
            ));
        }
        Ok(success)
    }
    async fn import_current(&mut self) -> Result<(), RuntimeError> {
        let live = self.live()?;
        let token = live
            .credentials
            .get("claudeAiOauth")
            .and_then(|v| v.get("accessToken"))
            .and_then(Value::as_str)
            .ok_or(RuntimeError::Identity)?
            .to_owned();
        let profile = self.client()?.profile(&token).await?;
        self.ensure_context(&live.fingerprint)?;
        let mut account = StoredAccount::new(
            live.identity
                .get("emailAddress")
                .and_then(Value::as_str)
                .unwrap_or("Claude account"),
            live.identity.clone(),
            live.credentials,
            self.clock.now(),
        );
        if !same_owner(&account, &profile.identity) {
            return Err(RuntimeError::Identity);
        }
        profile.apply(&mut account, self.clock.now());
        self.merge_account(account, true);
        self.observe_active()?;
        Ok(())
    }
    fn merge_account(&mut self, mut incoming: StoredAccount, fresh_tokens: bool) {
        if let Some(existing) = self.saved.accounts.iter_mut().find(|a| {
            a.provider == incoming.provider
                && a.account_uuid().is_some()
                && a.account_uuid() == incoming.account_uuid()
                && a.organization_uuid() == incoming.organization_uuid()
        }) {
            if fresh_tokens {
                if !existing.credentials.is_object() {
                    existing.credentials = json!({});
                }
                existing.credentials["claudeAiOauth"] =
                    incoming.credentials["claudeAiOauth"].clone();
                existing.oauth_account = incoming.oauth_account;
                existing.saved_at = incoming.saved_at;
                existing.identity_verified = incoming.identity_verified;
                existing.subscription_status = incoming.subscription_status;
                existing.subscription_started_at = incoming.subscription_started_at;
                existing.plan_tier = incoming.plan_tier;
                existing.profile_checked_at = incoming.profile_checked_at;
            }
            // A replayed legacy import must never replace newer rotations, renewal,
            // cached observations, or unresolved operations in the encrypted vault.
        } else {
            incoming.identity_verified = fresh_tokens && incoming.identity_verified;
            self.saved.accounts.push(incoming);
        }
    }
    fn preview_import(&mut self, path: PathBuf) -> Result<ImportPreview, RuntimeError> {
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| RuntimeError::Import)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(RuntimeError::Import);
        }
        let directory = path.canonicalize().map_err(|_| RuntimeError::Import)?;
        let manifest = legacy_manifest(&directory)?;
        let json_files: Vec<_> = manifest
            .iter()
            .filter(|e| {
                e.path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            })
            .collect();
        if json_files.len() > 500 {
            return Err(RuntimeError::Import);
        }
        let mut total = 0;
        let mut files = vec![];
        let mut sources = vec![];
        let mut invalid = 0;
        for entry in json_files {
            if entry.is_symlink || !entry.is_file || entry.len > 2 * 1024 * 1024 {
                invalid += 1;
                continue;
            }
            total += entry.len;
            if total > 16 * 1024 * 1024 {
                return Err(RuntimeError::Import);
            }
            let bytes = bounded_legacy_read(&entry.path)?;
            let account = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| import_legacy(&v, self.clock.now()).ok());
            match account {
                Some(account) => files.push(PreviewFile { account }),
                None => invalid += 1,
            }
            // Track damaged records too: a changed invalid file invalidates review.
            sources.push((entry.path.clone(), bytes));
        }
        if legacy_manifest(&directory)? != manifest {
            return Err(RuntimeError::Import);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let result = ImportPreview {
            id: id.clone(),
            valid: files.len(),
            invalid,
            names: files.iter().map(|f| safe_label(&f.account.name)).collect(),
        };
        self.preview = Some(Preview {
            id,
            created_at: self.clock.now(),
            directory,
            manifest,
            sources,
            files,
        });
        Ok(result)
    }
    fn apply_preview(&mut self, id: &str) -> Result<(), RuntimeError> {
        if self.preview.as_ref().is_none_or(|p| p.id != id) {
            return Err(RuntimeError::Import);
        }
        let preview = self.preview.take().ok_or(RuntimeError::Import)?;
        let now = self.clock.now();
        if now < preview.created_at
            || now.saturating_sub(preview.created_at) >= 600
            || legacy_manifest(&preview.directory)? != preview.manifest
        {
            return Err(RuntimeError::Import);
        }
        for (path, bytes) in &preview.sources {
            let metadata = std::fs::symlink_metadata(path).map_err(|_| RuntimeError::Import)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() != bytes.len() as u64
                || bounded_legacy_read(path)? != *bytes
            {
                return Err(RuntimeError::Import);
            }
        }
        // All validation completes before any record merge or vault persistence.
        if legacy_manifest(&preview.directory)? != preview.manifest {
            return Err(RuntimeError::Import);
        }
        for file in preview.files {
            self.merge_account(file.account, false);
        }
        Ok(())
    }
}

fn legacy_manifest(directory: &std::path::Path) -> Result<Vec<LegacyEntry>, RuntimeError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|_| RuntimeError::Import)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(RuntimeError::Import);
    }
    let mut entries = vec![];
    for entry in std::fs::read_dir(directory).map_err(|_| RuntimeError::Import)? {
        let path = entry.map_err(|_| RuntimeError::Import)?.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| RuntimeError::Import)?;
        entries.push(LegacyEntry {
            path,
            len: metadata.len(),
            is_file: metadata.is_file(),
            is_symlink: metadata.file_type().is_symlink(),
            modified: metadata.modified().ok(),
        });
        if entries.len() > 1000 {
            return Err(RuntimeError::Import);
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
fn bounded_legacy_read(path: &std::path::Path) -> Result<Vec<u8>, RuntimeError> {
    const MAX: usize = 2 * 1024 * 1024;
    let file = std::fs::File::open(path).map_err(|_| RuntimeError::Import)?;
    let mut bytes = vec![];
    file.take(MAX as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RuntimeError::Import)?;
    if bytes.len() > MAX {
        return Err(RuntimeError::Import);
    }
    Ok(bytes)
}

fn safe_reset_text(text: &str, locale: &str) -> String {
    let fixed = [
        ResetOutcome::Reset {
            resets_left: None,
            cooldown_until: None,
        },
        ResetOutcome::AlreadyUsed,
        ResetOutcome::NotLimited,
        ResetOutcome::Cooldown { until: None },
        ResetOutcome::Ineligible { reason: None },
        ResetOutcome::Unavailable { reason: None },
        ResetOutcome::Transport,
        ResetOutcome::AuthError,
    ];
    if let Some(outcome) = fixed
        .iter()
        .find(|o| o.text() == text || o.text_for("en") == text)
    {
        outcome.text_for(locale).into()
    } else if locale == "ro" {
        "Ultimul rezultat al resetului este păstrat.".into()
    } else {
        "A historical reset result was preserved.".into()
    }
}
fn safe_error_text(text: &str, locale: &str) -> String {
    let errors = [
        RuntimeError::Storage,
        RuntimeError::UnavailableStorage,
        RuntimeError::UnsupportedContext,
        RuntimeError::VaultIntegrity,
        RuntimeError::MissingAccount,
        RuntimeError::ExternalChange,
        RuntimeError::Identity,
        RuntimeError::Provider,
        RuntimeError::Settings,
        RuntimeError::ReadOnly,
        RuntimeError::Login,
        RuntimeError::Import,
        RuntimeError::PendingReset,
        RuntimeError::CliVersion,
    ];
    errors
        .iter()
        .find(|e| e.message("en") == text || e.message("ro") == text)
        .unwrap_or(&RuntimeError::Provider)
        .message(locale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use provider_claude::{
        HttpRequest, HttpResponse, INFERENCE_URL, PROFILE_URL, TOKEN_URL, Transport, USAGE_URL,
    };
    use std::{
        collections::VecDeque,
        sync::{Mutex as StdMutex, atomic::AtomicI64},
    };
    use tokio::sync::Notify;
    struct FakeClock(AtomicI64);
    impl Clock for FakeClock {
        fn now(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }
    type Hook = Box<dyn FnOnce(&HttpRequest) + Send>;
    type Pause = (&'static str, Arc<Notify>, Arc<Notify>);
    struct Fake {
        calls: StdMutex<Vec<HttpRequest>>,
        replies: StdMutex<VecDeque<(&'static str, Result<HttpResponse, ClientError>)>>,
        hook: StdMutex<Option<Hook>>,
        pause: StdMutex<Option<Pause>>,
        usage: StdMutex<Value>,
    }
    impl Default for Fake {
        fn default() -> Self {
            Self {
                calls: StdMutex::new(vec![]),
                replies: StdMutex::new(VecDeque::new()),
                hook: StdMutex::new(None),
                pause: StdMutex::new(None),
                usage: StdMutex::new(
                    json!({"five_hour":{"utilization":20,"resets_at":10000},"seven_day":{"utilization":20,"resets_at":90000},"limits":[],"cedar_ember":{"grants":[]}}),
                ),
            }
        }
    }
    fn response(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            body,
            headers: BTreeMap::new(),
        }
    }
    #[async_trait]
    impl Transport for Fake {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, ClientError> {
            let token = request
                .headers
                .get("authorization")
                .cloned()
                .unwrap_or_default();
            let url = request.url.clone();
            if let Some(hook) = self.hook.lock().unwrap().take() {
                hook(&request);
            }
            self.calls.lock().unwrap().push(request);
            let pause = {
                let mut guard = self.pause.lock().unwrap();
                if guard.as_ref().is_some_and(|(target, _, _)| *target == url) {
                    guard.take()
                } else {
                    None
                }
            };
            if let Some((_, started, resume)) = pause {
                started.notify_one();
                resume.notified().await;
            }
            {
                let mut replies = self.replies.lock().unwrap();
                if replies.front().is_some_and(|(target, _)| *target == url) {
                    return replies.pop_front().unwrap().1;
                }
            }
            match url.as_str() {
                PROFILE_URL => {
                    let owner = if token.contains("token-b") { "b" } else { "a" };
                    Ok(response(
                        200,
                        json!({"account":{"uuid":owner,"email":format!("{owner}@example.invalid")},"organization":{"uuid":format!("org-{owner}"),"subscription_status":"active","rate_limit_tier":"max"}}),
                    ))
                }
                USAGE_URL => Ok(response(200, self.usage.lock().unwrap().clone())),
                INFERENCE_URL => Ok(HttpResponse {
                    status: 200,
                    body: json!({"usage":{"input_tokens":1,"output_tokens":1}}),
                    headers: BTreeMap::from([
                        (
                            "anthropic-ratelimit-unified-5h-utilization".into(),
                            "0.2".into(),
                        ),
                        (
                            "anthropic-ratelimit-unified-7d-utilization".into(),
                            "0.2".into(),
                        ),
                        (
                            "anthropic-ratelimit-unified-5h-reset".into(),
                            "10000".into(),
                        ),
                        (
                            "anthropic-ratelimit-unified-7d-reset".into(),
                            "90000".into(),
                        ),
                    ]),
                }),
                TOKEN_URL => Ok(response(
                    200,
                    json!({"access_token":"token-b-rotated-SENTINEL","refresh_token":"refresh-b-rotated-SENTINEL","expires_in":28800}),
                )),
                _ if url.ends_with("/reset_rate_limits") => {
                    Ok(response(200, json!({"result":"reset","resets_left":0})))
                }
                _ => Err(ClientError::InvalidResponse),
            }
        }
    }
    fn account(id: &str, now: i64) -> StoredAccount {
        StoredAccount {
            id: id.into(),
            name: format!("Cont {id}"),
            oauth_account: json!({"accountUuid":id,"organizationUuid":format!("org-{id}"),"emailAddress":format!("{id}@example.invalid")}),
            credentials: json!({"preserve":true,"claudeAiOauth":{"accessToken":format!("token-{id}-SENTINEL"),"refreshToken":format!("refresh-{id}-SENTINEL"),"expiresAt":(now+3600)*1000,"extension":true}}),
            last_usage: Some(UsageResponse {
                five_hour: UsageWindow {
                    utilization: 20.0,
                    resets_at: Some(10000),
                },
                seven_day: UsageWindow {
                    utilization: 20.0,
                    resets_at: Some(90000),
                },
                ..UsageResponse::default()
            }),
            last_usage_at: Some(now),
            profile_checked_at: Some(now),
            identity_verified: true,
            ..StoredAccount::default()
        }
    }
    fn fixture(
        accounts: Vec<StoredAccount>,
        active_id: Option<&str>,
        now: i64,
        fake: Arc<Fake>,
    ) -> (tempfile::TempDir, RuntimeHandle, Arc<FakeClock>, CliPaths) {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let temp = tempfile::Builder::new()
            .prefix("primerswitch-runtime-")
            .tempdir_in(parent)
            .unwrap();
        let paths = CliPaths::for_home(temp.path().join("home"));
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(&paths.settings_file, b"{}").unwrap();
        if let Some(id) = active_id {
            let a = accounts.iter().find(|a| a.id == id).unwrap();
            std::fs::write(
                &paths.credentials_file,
                serde_json::to_vec(&a.credentials).unwrap(),
            )
            .unwrap();
            std::fs::write(
                &paths.global_config_file,
                serde_json::to_vec(&json!({"oauthAccount":a.oauth_account,"keep":true})).unwrap(),
            )
            .unwrap();
        }
        let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
        let saved = Persisted {
            readings: accounts
                .iter()
                .map(|a| {
                    (
                        a.id.clone(),
                        Reading {
                            complete: true,
                            scoped_at: Some(now),
                            ..Reading::default()
                        },
                    )
                })
                .collect(),
            accounts,
            settings: Settings {
                auto_switch_enabled: false,
                auto_use_resets_enabled: false,
                auto_start_window_enabled: false,
                ..Settings::default()
            },
            last_refresh_at: None,
        };
        vault.save(RECORD, &saved).unwrap();
        let clock = Arc::new(FakeClock(AtomicI64::new(now)));
        let client = ClaudeClient::with_transport("2.1.287", fake).unwrap();
        let handle = RuntimeHandle::from_parts(
            vault,
            ActiveStore::file(paths.clone()),
            client,
            clock.clone(),
        )
        .unwrap();
        (temp, handle, clock, paths)
    }
    fn count(fake: &Fake, url: &str) -> usize {
        fake.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.url == url)
            .count()
    }
    #[tokio::test]
    async fn preview_one_state_survives_branded_reinstall_and_settings_mutation() {
        // Historical contract: release 7e8411509488a5bf7217f10d7c6cccbf726c4562.
        // Literal synthetic JSON avoids deriving the old format from new defaults.
        let historical: Value = serde_json::from_str(r#"{
          "settings": {"poll_interval":900,"threshold":70.5,"auto_switch_enabled":false,"auto_start_window_enabled":false,"auto_use_resets_enabled":false,"appearance":"light","language":"ro"},
          "last_refresh_at":997,
          "readings": {
            "a":{"scoped_at":995,"complete":true,"error":null,"priming_attempt_at":994},
            "b":{"scoped_at":993,"complete":false,"error":"Historical fixture error","priming_attempt_at":992}
          },
          "accounts": [
            {
              "id":"a","provider":"claude","name":"Historical A","saved_at":801,
              "oauth_account":{"accountUuid":"a","organizationUuid":"org-a","emailAddress":"a@example.invalid","oldIdentityExtension":{"nested":[1,"keep-a"]}},
              "credentials":{"oldCredentialExtension":["keep-a",{"flag":true}],"claudeAiOauth":{"accessToken":"HISTORICAL_FIXTURE_ACCESS_A","refreshToken":"HISTORICAL_FIXTURE_REFRESH_A","expiresAt":4600000,"oldOauthExtension":{"opaque":"keep-a"}}},
              "refresh_fail_at":802,"rate_limited_until":1800,"expected_weekly_reset_at":90000,"primed_for_reset_at":86400,"consecutive_rate_limits":3,
              "last_usage": {
                "five_hour":{"utilization":21.5,"resets_at":10000},
                "seven_day":{"utilization":62.5,"resets_at":90000},
                "seven_day_sonnet":{"utilization":48.5,"resets_at":91000},
                "extra_usage":{"is_enabled":true,"historicalExtension":["keep-quota"]},
                "limits":[
                  {"kind":"weekly_all","group":"weekly","percent":62.5,"resets_at":90000,"scope":{"historicalLimitExtension":"keep-total"}},
                  {"kind":"weekly_model","group":"weekly","percent":48.5,"resets_at":91000,"scope":{"model":{"display_name":"Sonnet"},"historicalLimitExtension":"keep-model"}}
                ],
                "cedar_ember":{"eligible":false,"ineligible_reason":"fixture","grants":[],"next_grant_id":null,"cooldown_until":2000,"weekly_resets_at":90000}
              },
              "last_usage_at":991,"last_endpoint_read_at":990,"last_endpoint_attempt_at":989,
              "subscription_status":"active","subscription_started_at":700,"plan_tier":"max","profile_checked_at":988,"renewal_day":31,
              "reset_status": {
                "eligible":true,"ineligible_reason":null,
                "grants":[{"id":"historical_grant","resets_left":2,"starts_at":800,"ends_at":90000,"clears":["weekly_all"],"paused":false,"usable_now":true,"use_requires_limit":true}],
                "next_grant_id":"historical_grant","cooldown_until":1900,"weekly_resets_at":90000
              },
              "reset_status_at":987,"pending_reset_claim":{"grant_id":"historical_grant","request_id":"historical-request-a","created_at":986},
              "last_reset_outcome":"transport failure","last_reset_attempt_at":985,"identity_verified":true,"priming_pending_for":92000
            },
            {
              "id":"b","provider":"claude","name":"Historical B","saved_at":701,
              "oauth_account":{"accountUuid":"b","organizationUuid":"org-b","emailAddress":"b@example.invalid","oldIdentityExtension":{"nested":[2,"keep-b"]}},
              "credentials":{"oldCredentialExtension":["keep-b",{"flag":false}],"claudeAiOauth":{"accessToken":"HISTORICAL_FIXTURE_ACCESS_B","refreshToken":"HISTORICAL_FIXTURE_REFRESH_B","expiresAt":4700000,"oldOauthExtension":{"opaque":"keep-b"}}},
              "refresh_fail_at":null,"rate_limited_until":null,"expected_weekly_reset_at":91000,"primed_for_reset_at":87000,"consecutive_rate_limits":1,
              "last_usage":null,"last_usage_at":981,"last_endpoint_read_at":980,"last_endpoint_attempt_at":979,
              "subscription_status":"inactive","subscription_started_at":600,"plan_tier":"pro","profile_checked_at":978,"renewal_day":15,
              "reset_status":{"eligible":false,"ineligible_reason":"fixture-b","grants":[],"next_grant_id":null,"cooldown_until":null,"weekly_resets_at":91000},
              "reset_status_at":977,"pending_reset_claim":null,"last_reset_outcome":"reset already used","last_reset_attempt_at":976,"identity_verified":true,"priming_pending_for":null
            }
          ]
        }"#).unwrap();
        let parsed: Persisted = serde_json::from_value(historical.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), historical);
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let temp = tempfile::Builder::new()
            .prefix("primerswitch-preview-one-compatibility-")
            .tempdir_in(parent)
            .unwrap();
        let paths = CliPaths::for_home(temp.path().join("fixture-home"));
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(&paths.settings_file, b"{}").unwrap();
        let directory = temp.path().join("fixture-vault");
        let injected_key = [7; 32];
        let vault = Vault::with_key(directory.clone(), injected_key).unwrap();
        // Synthetic persisted key fixture; never touches an OS credential store.
        let key_file = directory.join("fixture-injected-master-key.bin");
        std::fs::write(&key_file, injected_key).unwrap();
        vault.save(RECORD, &historical).unwrap();
        let state_file = directory.join("runtime-state.vault");
        let initial_ciphertext = std::fs::read(&state_file).unwrap();
        let fake = Arc::new(Fake::default());
        let clock = Arc::new(FakeClock(AtomicI64::new(1000)));
        let handle = RuntimeHandle::from_parts(
            vault,
            ActiveStore::file(paths.clone()),
            ClaudeClient::with_transport("2.1.287", fake.clone()).unwrap(),
            clock.clone(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&state_file).unwrap(), initial_ciphertext);
        assert_eq!(std::fs::read(&key_file).unwrap(), injected_key);
        assert!(fake.calls.lock().unwrap().is_empty());
        let snapshot = handle.get_snapshot();
        assert_eq!(snapshot.accounts.len(), 2);
        assert_eq!(snapshot.settings.appearance, "light");
        assert_eq!(snapshot.settings.language, "ro");
        assert_eq!(snapshot.settings.poll_interval, 900);
        assert_eq!(snapshot.settings.threshold, 70.5);
        assert!(snapshot.accounts.iter().all(|a| !a.identity_verified));
        let public = serde_json::to_string(&snapshot).unwrap();
        assert!(!public.contains("HISTORICAL_FIXTURE_ACCESS"));
        assert!(!public.contains("HISTORICAL_FIXTURE_REFRESH"));
        let mut expected = historical.clone();
        // Ownership is intentionally reverified after process restart. All other
        // account, quota, renewal and pending scheduling/reset fields stay intact.
        for account in expected["accounts"].as_array_mut().unwrap() {
            account["identity_verified"] = json!(false);
        }
        {
            let engine = handle.inner.owner.lock().await;
            assert_eq!(serde_json::to_value(&engine.saved).unwrap(), expected);
        }
        let mut settings: Settings = snapshot.settings.into();
        settings.threshold = 71.5;
        handle.update_settings(settings).await.unwrap();
        expected["settings"]["threshold"] = json!(71.5);
        drop(handle);
        assert_eq!(std::fs::read(&key_file).unwrap(), injected_key);
        let vault = Vault::with_key(directory.clone(), injected_key).unwrap();
        assert_eq!(vault.load::<Value>(RECORD).unwrap().unwrap(), expected);
        let after_mutation = std::fs::read(&state_file).unwrap();
        assert_ne!(after_mutation, initial_ciphertext);
        let reopened = RuntimeHandle::from_parts(
            vault,
            ActiveStore::file(paths),
            ClaudeClient::with_transport("2.1.287", fake.clone()).unwrap(),
            clock,
        )
        .unwrap();
        assert_eq!(std::fs::read(&state_file).unwrap(), after_mutation);
        assert_eq!(std::fs::read(&key_file).unwrap(), injected_key);
        let engine = reopened.inner.owner.lock().await;
        assert_eq!(serde_json::to_value(&engine.saved).unwrap(), expected);
        assert!(fake.calls.lock().unwrap().is_empty());
        assert_eq!(reopened.get_snapshot().settings.threshold, 71.5);
    }

    #[tokio::test]
    async fn demo_and_degraded_snapshots_are_read_only_and_redacted() {
        let demo = RuntimeHandle::demo();
        let json = serde_json::to_string(&demo.get_snapshot()).unwrap();
        assert!(json.contains("consumptionPlan"));
        assert!(!json.contains("credentials"));
        assert!(!json.contains("accessToken"));
        assert!(demo.get_snapshot().demo);
        assert_eq!(
            demo.refresh_all().await.unwrap_err(),
            RuntimeError::ReadOnly
        );
        let unavailable = RuntimeHandle::unavailable(RuntimeError::UnsupportedContext);
        let snapshot = unavailable.get_snapshot();
        assert!(!snapshot.demo);
        assert!(snapshot.accounts.is_empty());
        assert!(snapshot.error.unwrap().contains("does not support"));
        assert!(
            RuntimeError::UnsupportedContext
                .message("ro")
                .contains("incompatibilă")
        );
        assert_eq!(
            unavailable.import_current().await.unwrap_err(),
            RuntimeError::ReadOnly
        );
    }
    #[tokio::test]
    async fn cached_snapshot_is_zero_io_and_fast_ticks_never_metadata() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        for _ in 0..20 {
            let _ = handle.get_snapshot();
        }
        assert!(fake.calls.lock().unwrap().is_empty());
        handle.tick(false).await.unwrap();
        assert_eq!(count(&fake, PROFILE_URL), 1);
        assert_eq!(count(&fake, INFERENCE_URL), 1);
        assert_eq!(count(&fake, USAGE_URL), 0);
        assert_eq!(count(&fake, TOKEN_URL), 0);
        fake.replies
            .lock()
            .unwrap()
            .push_back((INFERENCE_URL, Err(ClientError::Transport)));
        clock.0.store(1030, Ordering::SeqCst);
        assert_eq!(
            handle.tick(false).await.unwrap_err(),
            RuntimeError::Provider
        );
        assert_eq!(count(&fake, USAGE_URL), 0);
        assert!(!handle.get_snapshot().accounts[0].decision_fresh);
        assert!(handle.get_snapshot().accounts[0].usage.is_some());
    }
    #[tokio::test]
    async fn fresh_inactive_startup_cache_avoids_metadata_and_expired_active_never_refreshes() {
        let fake = Arc::new(Fake::default());
        let mut a = account("a", 1000);
        a.credentials["claudeAiOauth"]["expiresAt"] = json!(0);
        let (_temp, handle, _clock, _paths) =
            fixture(vec![a, account("b", 1000)], Some("a"), 1000, fake.clone());
        handle.tick(true).await.unwrap();
        assert_eq!(count(&fake, TOKEN_URL), 0);
        assert_eq!(count(&fake, USAGE_URL), 1); // active enrichment only
        assert_eq!(count(&fake, PROFILE_URL), 2);
        assert!(
            handle
                .get_snapshot()
                .accounts
                .iter()
                .all(|a| a.identity_verified)
        );
    }
    #[tokio::test]
    async fn inactive_rotation_is_durable_before_profile_or_usage() {
        let fake = Arc::new(Fake::default());
        let mut b = account("b", 1000);
        b.credentials["claudeAiOauth"]["expiresAt"] = json!(1000000);
        let (temp, handle, _clock, _paths) =
            fixture(vec![account("a", 1000), b], Some("a"), 1000, fake.clone());
        // Fail the next ownership profile after rotation. The new refresh token must
        // already be in the encrypted record despite all later work failing.
        fake.replies.lock().unwrap().push_back((
            PROFILE_URL,
            Ok(response(
                200,
                json!({"account":{"uuid":"a"},"organization":{"uuid":"org-a"}}),
            )),
        ));
        fake.replies
            .lock()
            .unwrap()
            .push_back((PROFILE_URL, Err(ClientError::Transport)));
        handle.tick(true).await.unwrap();
        let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
        let saved = vault.load::<Persisted>(RECORD).unwrap().unwrap();
        let b = saved.accounts.iter().find(|a| a.id == "b").unwrap();
        assert_eq!(b.refresh_token(), Some("refresh-b-rotated-SENTINEL"));
        assert!(!b.identity_verified);
        assert_eq!(count(&fake, TOKEN_URL), 1);
        assert_eq!(count(&fake, USAGE_URL), 1);
        assert!(
            !serde_json::to_string(&handle.get_snapshot())
                .unwrap()
                .contains("SENTINEL")
        );
    }
    #[tokio::test]
    async fn external_writer_during_profile_cannot_poison_saved_account() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, _clock, paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        let credentials = paths.credentials_file.clone();
        *fake.hook.lock().unwrap() = Some(Box::new(move |_| {
            std::fs::write(
                credentials,
                serde_json::to_vec(&json!({"claudeAiOauth":{"accessToken":"external-token"}}))
                    .unwrap(),
            )
            .unwrap();
        }));
        assert_eq!(
            handle.tick(false).await.unwrap_err(),
            RuntimeError::ExternalChange
        );
        let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
        let saved = vault.load::<Persisted>(RECORD).unwrap().unwrap();
        assert_eq!(saved.accounts[0].access_token(), Some("token-a-SENTINEL"));
        assert!(!saved.accounts[0].identity_verified);
        assert_eq!(count(&fake, INFERENCE_URL), 0);
    }
    #[tokio::test]
    async fn mismatched_profile_blocks_manual_switch_without_active_writes() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, _clock, paths) = fixture(
            vec![account("a", 1000), account("b", 1000)],
            Some("a"),
            1000,
            fake.clone(),
        );
        let before = std::fs::read(&paths.credentials_file).unwrap();
        fake.replies.lock().unwrap().push_back((
            PROFILE_URL,
            Ok(response(
                200,
                json!({"account":{"uuid":"wrong"},"organization":{"uuid":"org-b"}}),
            )),
        ));
        assert_eq!(
            handle.switch_account("b").await.unwrap_err(),
            RuntimeError::Identity
        );
        assert_eq!(std::fs::read(paths.credentials_file).unwrap(), before);
        assert_eq!(handle.get_snapshot().active_id.as_deref(), Some("a"));
    }
    #[tokio::test]
    async fn metadata_cooldown_survives_live_inference_and_restart() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, clock, paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        fake.replies.lock().unwrap().push_back((
            USAGE_URL,
            Err(ClientError::RateLimited {
                retry_after: Some(600),
            }),
        ));
        handle.tick(true).await.unwrap();
        clock.0.store(1300, Ordering::SeqCst);
        handle.tick(true).await.unwrap();
        assert_eq!(count(&fake, USAGE_URL), 1);
        let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
        let saved = vault.load::<Persisted>(RECORD).unwrap().unwrap();
        assert_eq!(saved.accounts[0].rate_limited_until, Some(1600));
        assert_eq!(saved.accounts[0].consecutive_rate_limits, 1);
        let restarted = RuntimeHandle::from_parts(
            vault,
            ActiveStore::file(paths),
            ClaudeClient::with_transport("2.1.287", fake.clone()).unwrap(),
            clock.clone(),
        )
        .unwrap();
        restarted.tick(false).await.unwrap();
        assert_eq!(count(&fake, USAGE_URL), 1);
    }
    #[tokio::test]
    async fn concurrent_refreshes_coalesce_and_snapshot_stays_readable_while_busy() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, _clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        let started = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        *fake.pause.lock().unwrap() = Some((PROFILE_URL, started.clone(), resume.clone()));
        let first = {
            let h = handle.clone();
            tokio::spawn(async move { h.refresh_all().await })
        };
        started.notified().await;
        assert!(handle.get_snapshot().busy);
        let second = {
            let h = handle.clone();
            tokio::spawn(async move { h.refresh_all().await })
        };
        tokio::task::yield_now().await;
        resume.notify_one();
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        assert_eq!(count(&fake, INFERENCE_URL), 1);
        assert!(!handle.get_snapshot().busy);
    }
    #[tokio::test]
    async fn legacy_preview_detects_changed_source_and_reimport_preserves_durable_state() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, _clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake);
        handle.set_renewal_day("a", Some(31)).await.unwrap();
        let directory = temp.path().join("legacy");
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("a.json");
        let legacy = json!({"name":"Old name","savedAt":"2024-02-29T00:00:00Z","oauthAccount":{"accountUuid":"a","organizationUuid":"org-a"},"credentials":{"claudeAiOauth":{"accessToken":"old-token","refreshToken":"old-refresh"}},"renewalDay":1});
        std::fs::write(&source, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let preview = handle
            .preview_legacy_import(directory.clone())
            .await
            .unwrap();
        assert_eq!(preview.valid, 1);
        std::fs::write(&source, b"{}").unwrap();
        assert_eq!(
            handle.apply_legacy_import(&preview.id).await.unwrap_err(),
            RuntimeError::Import
        );
        std::fs::write(&source, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let preview = handle.preview_legacy_import(directory).await.unwrap();
        handle.apply_legacy_import(&preview.id).await.unwrap();
        let engine = handle.inner.owner.lock().await;
        assert_eq!(engine.saved.accounts.len(), 1);
        assert_eq!(engine.saved.accounts[0].renewal_day, Some(31));
        assert_eq!(
            engine.saved.accounts[0].access_token(),
            Some("token-a-SENTINEL")
        );
        assert_eq!(
            std::fs::read(&source).unwrap(),
            serde_json::to_vec(&legacy).unwrap()
        );
    }
    #[tokio::test]
    async fn reset_pending_key_is_persisted_before_send_and_reused_after_lost_reply() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        handle.tick(true).await.unwrap();
        let reset_url = "https://api.anthropic.com/api/organizations/org-a/reset_rate_limits";
        fake.replies
            .lock()
            .unwrap()
            .push_back((reset_url, Err(ClientError::Transport)));
        let request_id = {
            let mut engine = handle.inner.owner.lock().await;
            engine.saved.settings.auto_use_resets_enabled = true;
            let a = &mut engine.saved.accounts[0];
            a.last_usage.as_mut().unwrap().five_hour.utilization = 100.0;
            a.reset_status = Some(ResetStatus {
                grants: vec![ResetGrant {
                    id: "g".into(),
                    resets_left: 1,
                    ..ResetGrant::default()
                }],
                ..ResetStatus::default()
            });
            a.reset_status_at = Some(1000);
            engine.claim("a", "g", true).await.unwrap();
            let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
            vault.load::<Persisted>(RECORD).unwrap().unwrap().accounts[0]
                .pending_reset_claim
                .as_ref()
                .unwrap()
                .request_id
                .clone()
        };
        clock.0.store(1300, Ordering::SeqCst);
        {
            let mut engine = handle.inner.owner.lock().await;
            assert!(engine.claim("a", "g", false).await.unwrap());
            assert!(engine.saved.accounts[0].pending_reset_claim.is_none());
            assert!(!engine.saved.readings["a"].complete);
        }
        let calls = fake.calls.lock().unwrap();
        let claims: Vec<_> = calls.iter().filter(|c| c.url == reset_url).collect();
        assert_eq!(claims.len(), 2);
        for claim in claims {
            assert_eq!(claim.body.as_ref().unwrap()["request_id"], request_id);
        }
    }
    #[tokio::test]
    async fn reset_success_blocks_same_cycle_switch_on_stale_full_usage() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, _clock, _paths) = fixture(
            vec![account("a", 1000), account("b", 1000)],
            Some("a"),
            1000,
            fake.clone(),
        );
        handle.tick(true).await.unwrap();
        let mut engine = handle.inner.owner.lock().await;
        engine.saved.settings.auto_switch_enabled = true;
        engine.saved.settings.auto_use_resets_enabled = true;
        for account in &mut engine.saved.accounts {
            account.last_usage.as_mut().unwrap().five_hour.utilization = 100.0;
        }
        engine.saved.accounts[0].reset_status = Some(ResetStatus {
            grants: vec![ResetGrant {
                id: "g".into(),
                resets_left: 1,
                ..ResetGrant::default()
            }],
            ..ResetStatus::default()
        });
        engine.saved.accounts[0].reset_status_at = Some(1000);
        engine.automate(true).await.unwrap();
        assert_eq!(engine.active_id.as_deref(), Some("a"));
        assert!(!engine.saved.readings["a"].complete);
        assert_eq!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.url.ends_with("/reset_rate_limits"))
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn prime_marker_is_durable_and_unknown_reply_requires_reconciliation() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, clock, _paths) =
            fixture(vec![account("b", 1000)], None, 1000, fake.clone());
        handle.tick(true).await.unwrap();
        {
            let mut engine = handle.inner.owner.lock().await;
            engine.saved.settings.auto_start_window_enabled = true;
            engine.saved.accounts[0].expected_weekly_reset_at = Some(900);
            engine.saved.accounts[0]
                .last_usage
                .as_mut()
                .unwrap()
                .seven_day
                .resets_at = None;
            fake.replies
                .lock()
                .unwrap()
                .push_back((INFERENCE_URL, Err(ClientError::Transport)));
            let expected = engine.live().unwrap().fingerprint;
            engine.prime("b", &expected).await.unwrap();
        }
        let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
        let saved = vault.load::<Persisted>(RECORD).unwrap().unwrap();
        assert_eq!(saved.accounts[0].priming_pending_for, Some(900));
        assert_eq!(count(&fake, INFERENCE_URL), 1);
        clock.0.store(1030, Ordering::SeqCst);
        {
            let mut engine = handle.inner.owner.lock().await;
            let expected = engine.live().unwrap().fingerprint;
            engine.prime("b", &expected).await.unwrap();
        }
        assert_eq!(count(&fake, INFERENCE_URL), 1);
        {
            let mut engine = handle.inner.owner.lock().await;
            engine.saved.accounts[0]
                .last_usage
                .as_mut()
                .unwrap()
                .seven_day
                .resets_at = Some(90000);
            let expected = engine.live().unwrap().fingerprint;
            engine.prime("b", &expected).await.unwrap();
            assert_eq!(engine.saved.accounts[0].primed_for_reset_at, Some(900));
            assert_eq!(engine.saved.accounts[0].priming_pending_for, None);
        }
        assert_eq!(count(&fake, INFERENCE_URL), 1);
    }
    #[tokio::test]
    async fn cancelled_deferred_login_and_stale_ids_never_add_an_account() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, _clock, _paths) = fixture(vec![], None, 1000, fake.clone());
        let first = handle.begin_login().await.unwrap();
        let second = handle.begin_login().await.unwrap();
        assert_eq!(
            handle
                .finish_login(&first.id, "invalid#old")
                .await
                .unwrap_err(),
            RuntimeError::Login
        );
        assert_eq!(
            handle
                .inner
                .owner
                .lock()
                .await
                .pending_login
                .as_ref()
                .unwrap()
                .id,
            second.id
        );
        let state = second
            .url
            .split("state=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        let started = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        *fake.pause.lock().unwrap() = Some((TOKEN_URL, started.clone(), resume.clone()));
        let completion = {
            let h = handle.clone();
            let id = second.id.clone();
            let code = format!("fixture#{state}");
            tokio::spawn(async move { h.finish_login(&id, &code).await })
        };
        started.notified().await;
        let cancellation = {
            let h = handle.clone();
            let id = second.id.clone();
            tokio::spawn(async move { h.cancel_login(&id).await })
        };
        tokio::task::yield_now().await;
        resume.notify_one();
        assert_eq!(completion.await.unwrap().unwrap_err(), RuntimeError::Login);
        cancellation.await.unwrap().unwrap();
        assert!(handle.get_snapshot().accounts.is_empty());
    }
    #[tokio::test]
    async fn partial_refusal_only_becomes_complete_after_current_metadata_fills_missing_main() {
        let fake = Arc::new(Fake::default());
        *fake.usage.lock().unwrap() =
            json!({"five_hour":{"utilization":57},"seven_day":{"utilization":20},"limits":[]});
        fake.replies.lock().unwrap().push_back((
            INFERENCE_URL,
            Ok(HttpResponse {
                status: 429,
                body: Value::Null,
                headers: BTreeMap::from([
                    (
                        "anthropic-ratelimit-unified-status".into(),
                        "rejected".into(),
                    ),
                    (
                        "anthropic-ratelimit-unified-representative-claim".into(),
                        "seven_day".into(),
                    ),
                ]),
            }),
        ));
        let (_temp, handle, _clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake);
        handle.tick(true).await.unwrap();
        let snapshot = handle.get_snapshot();
        let usage = snapshot.accounts[0].usage.as_ref().unwrap();
        assert_eq!(usage.five_hour.utilization, 57.0);
        assert_eq!(usage.seven_day.utilization, 100.0);
        assert!(snapshot.accounts[0].decision_fresh);
    }
    #[tokio::test]
    async fn prime_keeps_fresh_scoped_exhaustion_and_drops_old_weekly_all() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, _clock, _paths) = fixture(vec![account("b", 1000)], None, 1000, fake);
        handle.tick(true).await.unwrap();
        let mut engine = handle.inner.owner.lock().await;
        engine.saved.settings.auto_start_window_enabled = true;
        engine.active_model = Some("sonnet".into());
        let a = &mut engine.saved.accounts[0];
        a.expected_weekly_reset_at = Some(900);
        a.last_usage.as_mut().unwrap().limits = Some(vec![
            UsageLimit {
                kind: "weekly_all".into(),
                group: "weekly".into(),
                percent: 99.0,
                ..UsageLimit::default()
            },
            UsageLimit {
                kind: "weekly_scoped".into(),
                group: "weekly".into(),
                percent: 100.0,
                scope: Some(json!({"model":{"display_name":"Sonnet"}})),
                ..UsageLimit::default()
            },
        ]);
        let expected = engine.live().unwrap().fingerprint;
        engine.prime("b", &expected).await.unwrap();
        let usage = engine.saved.accounts[0].last_usage.as_ref().unwrap();
        assert_eq!(usage.weekly_utilization(), 20.0);
        assert_eq!(usage.weekly_utilization_for_model(Some("sonnet")), 100.0);
        assert!(!is_usable(&engine.infos(true)[0], 95.0, Some("sonnet")));
    }
    #[tokio::test]
    async fn automatic_target_is_rechecked_after_preflight_and_stale_cooldown_never_switches() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, clock, paths) = fixture(
            vec![account("a", 1000), account("b", 1000)],
            Some("a"),
            1000,
            fake.clone(),
        );
        handle.tick(true).await.unwrap();
        clock.0.store(1300, Ordering::SeqCst);
        *fake.usage.lock().unwrap() =
            json!({"five_hour":{"utilization":100},"seven_day":{"utilization":20},"limits":[]});
        let before = std::fs::read(&paths.credentials_file).unwrap();
        let mut engine = handle.inner.owner.lock().await;
        assert_eq!(
            engine.switch_to("b", false, true).await.unwrap_err(),
            RuntimeError::Provider
        );
        assert_eq!(std::fs::read(&paths.credentials_file).unwrap(), before);
        let i = engine.index("b").unwrap();
        engine.saved.accounts[i]
            .last_usage
            .as_mut()
            .unwrap()
            .five_hour
            .utilization = 20.0;
        engine.saved.accounts[i].last_usage_at = Some(1000);
        engine.saved.accounts[i].rate_limited_until = Some(2000);
        assert_eq!(
            engine.switch_to("b", false, true).await.unwrap_err(),
            RuntimeError::Provider
        );
        assert_eq!(std::fs::read(&paths.credentials_file).unwrap(), before);
    }
    #[tokio::test]
    async fn disappeared_offer_retains_pending_identity_until_explicit_same_key_verdict() {
        let fake = Arc::new(Fake::default());
        let (_temp, handle, clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        handle.tick(true).await.unwrap();
        clock.0.store(1300, Ordering::SeqCst);
        let mut engine = handle.inner.owner.lock().await;
        let pending = prepare_reset_claim("g", None, 1000).unwrap();
        engine.saved.accounts[0].pending_reset_claim = Some(pending.clone());
        engine.saved.accounts[0].last_reset_attempt_at = Some(1000);
        engine.saved.accounts[0].reset_status_at = Some(1300);
        engine.saved.accounts[0].reset_status = Some(ResetStatus::default());
        engine.reconcile_pending("a").unwrap();
        assert_eq!(
            engine.saved.accounts[0].pending_reset_claim,
            Some(pending.clone())
        );
        fake.replies.lock().unwrap().push_back((
            "https://api.anthropic.com/api/organizations/org-a/reset_rate_limits",
            Ok(response(200, json!({"result":"already_used"}))),
        ));
        assert!(!engine.claim("a", "g", true).await.unwrap());
        assert!(engine.saved.accounts[0].pending_reset_claim.is_none());
        assert_eq!(
            fake.calls
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .body
                .as_ref()
                .unwrap()["request_id"],
            pending.request_id
        );
    }
    fn legacy_fixture() -> Value {
        json!({"name":"Legacy B","oauthAccount":{"accountUuid":"b","organizationUuid":"org-b"},"credentials":{"claudeAiOauth":{"accessToken":"legacy-b-token"}}})
    }
    #[tokio::test]
    async fn preview_expiry_and_stale_ids_preserve_newer_review_without_merging() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake);
        let directory = temp.path().join("legacy-expiry");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("b.json"),
            serde_json::to_vec(&legacy_fixture()).unwrap(),
        )
        .unwrap();
        let first = handle
            .preview_legacy_import(directory.clone())
            .await
            .unwrap();
        let second = handle.preview_legacy_import(directory).await.unwrap();
        assert_eq!(
            handle.apply_legacy_import(&first.id).await.unwrap_err(),
            RuntimeError::Import
        );
        assert_eq!(
            handle.inner.owner.lock().await.preview.as_ref().unwrap().id,
            second.id
        );
        clock.0.store(1600, Ordering::SeqCst);
        assert_eq!(
            handle.apply_legacy_import(&second.id).await.unwrap_err(),
            RuntimeError::Import
        );
        assert_eq!(handle.get_snapshot().accounts.len(), 1);
    }
    #[tokio::test]
    async fn directory_additions_and_changed_damaged_records_invalidate_entire_preview() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, _clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake);
        let directory = temp.path().join("legacy-manifest");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("b.json"),
            serde_json::to_vec(&legacy_fixture()).unwrap(),
        )
        .unwrap();
        std::fs::write(directory.join("damaged.json"), b"{}").unwrap();
        let preview = handle
            .preview_legacy_import(directory.clone())
            .await
            .unwrap();
        assert_eq!((preview.valid, preview.invalid), (1, 1));
        std::fs::write(directory.join("added.json"), b"{}").unwrap();
        assert_eq!(
            handle.apply_legacy_import(&preview.id).await.unwrap_err(),
            RuntimeError::Import
        );
        assert_eq!(handle.get_snapshot().accounts.len(), 1);
        std::fs::remove_file(directory.join("added.json")).unwrap();
        let preview = handle
            .preview_legacy_import(directory.clone())
            .await
            .unwrap();
        std::fs::write(directory.join("damaged.json"), b"[]").unwrap();
        assert_eq!(
            handle.apply_legacy_import(&preview.id).await.unwrap_err(),
            RuntimeError::Import
        );
        assert_eq!(handle.get_snapshot().accounts.len(), 1);
        let preview = handle.preview_legacy_import(directory).await.unwrap();
        handle.apply_legacy_import(&preview.id).await.unwrap();
        assert_eq!(handle.get_snapshot().accounts.len(), 2);
    }
    #[tokio::test]
    async fn invalid_claim_identity_never_creates_a_durable_unsent_pending_request() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, _clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        handle.tick(true).await.unwrap();
        let before = fake.calls.lock().unwrap().len();
        let mut engine = handle.inner.owner.lock().await;
        engine.saved.accounts[0]
            .last_usage
            .as_mut()
            .unwrap()
            .five_hour
            .utilization = 100.0;
        engine.saved.accounts[0].reset_status = Some(ResetStatus {
            grants: vec![ResetGrant {
                id: "g".into(),
                resets_left: 1,
                ..ResetGrant::default()
            }],
            ..ResetStatus::default()
        });
        engine.saved.accounts[0].reset_status_at = Some(1000);
        for organization in [
            "../bad".to_owned(),
            String::new(),
            "a".repeat(65),
            "bad_org".to_owned(),
        ] {
            engine.saved.accounts[0].oauth_account["organizationUuid"] = json!(organization);
            assert_eq!(
                engine.claim("a", "g", true).await.unwrap_err(),
                RuntimeError::Identity
            );
            assert!(engine.saved.accounts[0].pending_reset_claim.is_none());
            assert!(engine.saved.accounts[0].last_reset_attempt_at.is_none());
        }
        engine.saved.accounts[0].oauth_account["organizationUuid"] = json!("org-a");
        engine.saved.accounts[0].credentials["claudeAiOauth"]["accessToken"] = json!("");
        assert_eq!(
            engine.claim("a", "g", true).await.unwrap_err(),
            RuntimeError::Identity
        );
        assert!(engine.saved.accounts[0].pending_reset_claim.is_none());
        assert_eq!(fake.calls.lock().unwrap().len(), before);
        let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
        assert!(
            vault.load::<Persisted>(RECORD).unwrap().unwrap().accounts[0]
                .pending_reset_claim
                .is_none()
        );
    }
    #[tokio::test]
    async fn language_defaults_migrate_and_safe_errors_and_outcomes_translate() {
        let old: Settings = serde_json::from_value(json!({"poll_interval":300})).unwrap();
        assert_eq!(old.language, "en");
        let fake = Arc::new(Fake::default());
        let (_temp, handle, _clock, _paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake);
        {
            let mut engine = handle.inner.owner.lock().await;
            engine.saved.readings.get_mut("a").unwrap().error =
                Some("SENTINEL_TOKEN_IN_LEGACY_ERROR".into());
            engine.saved.accounts[0].last_reset_outcome =
                Some("SENTINEL_TOKEN_IN_LEGACY_OUTCOME".into());
            let json = serde_json::to_string(&engine.snapshot(false)).unwrap();
            assert!(!json.contains("SENTINEL"));
            engine.saved.accounts[0].last_reset_outcome =
                Some(ResetOutcome::AlreadyUsed.text().into());
        }
        let mut settings: Settings = handle.get_snapshot().settings.into();
        settings.language = "ro".into();
        let ro = handle.update_settings(settings).await.unwrap();
        assert_eq!(
            ro.accounts[0].resets.last_outcome.as_deref(),
            Some("reset deja folosit")
        );
        assert!(ro.accounts[0].error.as_ref().unwrap().contains("proaspătă"));
        let mut settings: Settings = ro.settings.into();
        settings.language = "en".into();
        let en = handle.update_settings(settings).await.unwrap();
        assert_eq!(
            en.accounts[0].resets.last_outcome.as_deref(),
            Some("reset already used")
        );
        assert!(
            en.accounts[0]
                .error
                .as_ref()
                .unwrap()
                .contains("fresh reading")
        );
        assert!(!RuntimeError::Storage.message("ro").contains('?'));
        assert_eq!(
            RuntimeError::Storage.message("en"),
            RuntimeError::Storage.to_string()
        );
    }

    #[tokio::test]
    async fn codex_cache_and_mutations_preserve_claude_ciphertext_and_use_one_owner() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, _, paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        let state = temp.path().join("vault/runtime-state.vault");
        let before = std::fs::read(&state).unwrap();
        let codex_before = serde_json::to_value(handle.get_codex_snapshot()).unwrap();
        // A corrupt unrelated store after startup must never be opened by cached polling.
        std::fs::write(paths.config_dir.join("unrelated-fixture"), b"opaque").unwrap();
        for _ in 0..10 {
            assert_eq!(
                serde_json::to_value(handle.get_codex_snapshot()).unwrap(),
                codex_before
            );
        }
        assert!(!temp.path().join("vault/codex-state.vault").exists());
        let owner = handle.inner.owner.lock().await;
        let denied = tokio::time::timeout(
            Duration::from_millis(100),
            handle.codex_poll_login("fixture-only"),
        )
        .await
        .unwrap();
        assert_eq!(denied.err(), Some(CodexReason::Busy));
        assert_eq!(std::fs::read(&state).unwrap(), before);
        drop(owner);
        let mut changes = handle.subscribe_codex();
        assert_eq!(
            handle.codex_delete_account("missing").await.err(),
            Some(CodexReason::IdentityUnverified)
        );
        assert!(changes.try_recv().unwrap().busy);
        assert!(!changes.try_recv().unwrap().busy);
        assert_eq!(std::fs::read(&state).unwrap(), before);
        assert!(fake.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn damaged_codex_record_preserves_claude_settings_and_original_envelope() {
        let fake = Arc::new(Fake::default());
        let (temp, handle, clock, paths) =
            fixture(vec![account("a", 1000)], Some("a"), 1000, fake.clone());
        drop(handle);
        let directory = temp.path().join("vault");
        let damaged = b"deliberate-fixture-codex-damaged-envelope";
        let codex_file = directory.join("codex-state.vault");
        std::fs::write(&codex_file, damaged).unwrap();
        let handle = RuntimeHandle::from_parts(
            Vault::with_key(directory, [7; 32]).unwrap(),
            ActiveStore::file(paths),
            ClaudeClient::with_transport("2.1.287", fake.clone()).unwrap(),
            clock,
        )
        .unwrap();
        let snapshot = handle.get_codex_snapshot();
        assert_eq!(snapshot.blocked_reason, Some(CodexReason::VaultUnavailable));
        assert!(!snapshot.capabilities.login_browser.enabled);
        assert!(!snapshot.capabilities.delete_saved.enabled);
        let mut settings = handle.get_snapshot().settings;
        settings.language = "ro".into();
        handle.update_settings(settings).await.unwrap();
        assert_eq!(handle.get_snapshot().settings.language, "ro");
        assert_eq!(std::fs::read(codex_file).unwrap(), damaged);
        assert_eq!(
            handle.codex_import_current().await.err(),
            Some(CodexReason::VaultUnavailable)
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }
}
