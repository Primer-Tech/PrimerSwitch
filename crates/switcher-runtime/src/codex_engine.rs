//! Codex accounts under the serialized runtime owner. Fast state changes run under
//! the owner lock; slow Codex children (quota readers, the daemon restart) run outside
//! it as prepared jobs, serialized by a separate Codex job lock (codex_runtime.rs).
//!
//! Switching: since Codex 0.160.0 every terminal is a client of one shared app-server
//! daemon per CODEX_HOME that owns the loaded login. A switch preserves the outgoing
//! login, writes the selected one to auth.json atomically and runs Codex's official
//! `codex app-server daemon restart`; terminals reconnect and resume their threads.
use crate::{AccountAutomationView, codex_context::*, codex_views::*};
use async_trait::async_trait;
use provider_codex::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use switcher_core::{
    AccountAutomation, CandidateOrder, CandidateRank, HEADROOM_MARGIN, ProviderId, Settings,
    compare_candidates, reset_hour_bucket,
};
use switcher_platform::{
    Vault,
    codex::{CodexStoreError, *},
};
use tempfile::TempDir;
use zeroize::{Zeroize, Zeroizing};

#[path = "codex_reset.rs"]
mod reset;
pub(crate) use reset::ResetJob;

const RECORD: &str = "codex-state";
const QUARANTINE: &str = "codex-state-quarantine";
/// Background reading of the active account while it is within `NEAR_LIMIT_BAND`
/// points of the switch threshold, or at a limit, with automatic switching on.
/// Otherwise the active account follows Claude's check interval.
pub(crate) const ACTIVE_NEAR_LIMIT_INTERVAL: i64 = 60;
const NEAR_LIMIT_BAND: f64 = 10.0;
/// Inactive accounts only change when their windows reset.
pub(crate) const INACTIVE_QUOTA_INTERVAL: i64 = 3600;
const ERROR_RETRY: i64 = 900;
/// A rejected sign-in is retried rarely; a new sign-in or a manual refresh clears it.
const SIGN_IN_RETRY: i64 = 6 * 3600;
const FRESH_SECONDS: i64 = 900;
const RETURN_RETRY: i64 = 120;
const MANUAL_RETURN_HOLD: i64 = 900;
/// Leave a nearly expired active token to Codex's own refresh in the background.
const ACTIVE_EXPIRY_MARGIN: i64 = 600;
/// At most one automatic switch attempt per ten minutes.
pub(crate) const AUTO_SWITCH_COOLDOWN: i64 = 600;
/// An automatic-switch failure (per target and reason) and the every-account-limited
/// notice repeat at most this often.
pub(crate) const AUTO_NOTICE_INTERVAL: i64 = 1800;

/// The automation settings Codex shares with Claude. They are persisted once, in the
/// Claude runtime record, and passed in wherever Codex needs them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CodexPolicy {
    pub auto_switch: bool,
    pub threshold: f64,
    /// Ordinary cadence of active readings, in seconds: Claude's check interval.
    pub poll_interval: i64,
    pub order: CandidateOrder,
}
impl From<&Settings> for CodexPolicy {
    fn from(settings: &Settings) -> Self {
        Self {
            auto_switch: settings.auto_switch_enabled,
            threshold: settings.threshold_for(ProviderId::Codex),
            poll_interval: i64::try_from(settings.poll_interval).unwrap_or(i64::MAX),
            order: settings.candidate_order(),
        }
    }
}
impl Default for CodexPolicy {
    fn default() -> Self {
        Self::from(&Settings::default())
    }
}
/// What automatic switching should do now (see `CodexEngine::automation`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CodexAutomation {
    Idle,
    /// The best target was not read recently: read it, then decide again.
    Refresh(String),
    Switch(CodexAutoSwitch),
    /// A preferred account's observed spent five-hour window naturally recovered.
    Return(CodexAutoSwitch),
    /// The active account needs replacing, but every other account is at its limit.
    Exhausted {
        /// The account whose blocking windows reopen first, and when.
        frees_first: Option<(String, i64)>,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CodexAutoSwitch {
    pub previous_id: String,
    pub target_id: String,
    pub target_name: String,
    pub previous_name: String,
    /// The active account's highest main-window usage; below the threshold when only
    /// a reached-limit flag triggered the switch.
    pub peak: Option<i32>,
}

/// The main Codex limit. Either window may be absent; duration determines its role.
fn main_limit(quota: &CodexQuotaView) -> Option<&CodexLimitView> {
    quota.limits.iter().find(|limit| limit.key == "default")
}
fn main_windows(quota: &CodexQuotaView) -> impl Iterator<Item = &CodexWindowView> {
    main_limit(quota)
        .into_iter()
        .flat_map(|limit| [&limit.primary, &limit.secondary])
        .flatten()
}
/// The same duration-based short/long placement as the renderer's `codexWindows`.
/// Unknown durations retain the native primary/secondary positions.
fn main_window_slots(limit: &CodexLimitView) -> [Option<&CodexWindowView>; 2] {
    let mut slots = [None, None];
    for (index, window) in [limit.primary.as_ref(), limit.secondary.as_ref()]
        .into_iter()
        .enumerate()
    {
        let Some(window) = window else { continue };
        let long = window
            .window_duration_mins
            .map_or(index == 1, |mins| mins > 1440);
        let preferred = usize::from(long);
        let slot = if slots[preferred].is_none() {
            preferred
        } else {
            1 - preferred
        };
        slots[slot] = Some(window);
    }
    slots
}
/// Codex says the plan's usage is unavailable: a main window is spent (above 100 means
/// usage continues on purchased credits), a limit was reached or included usage is off.
pub(crate) fn quota_limited(quota: &CodexQuotaView) -> bool {
    quota.ordinary_usage_allowed == Some(false)
        || main_limit(quota).is_some_and(|limit| limit.rate_limit_reached_type.is_some())
        || main_windows(quota).any(|window| window.used_percent >= 100)
}
/// A main window is at or above `level` percent, or the account is limited.
fn reaches(quota: &CodexQuotaView, level: f64) -> bool {
    quota_limited(quota) || main_windows(quota).any(|w| f64::from(w.used_percent) >= level)
}
/// At least one reported main window, all strictly below `threshold`, with nothing
/// else limiting the account. An omitted window is neither required nor free quota.
fn usable_rank(account: &SavedAccount, threshold: f64) -> Option<CandidateRank<'_>> {
    let quota = account.quota.as_ref()?;
    if quota_limited(quota) {
        return None;
    }
    let limit = main_limit(quota)?;
    let [five, week] = main_window_slots(limit);
    let windows = [five, week];
    if windows.iter().all(Option::is_none)
        || windows
            .iter()
            .flatten()
            .any(|w| f64::from(w.used_percent) >= threshold)
    {
        return None;
    }
    let margin = threshold - HEADROOM_MARGIN;
    Some(CandidateRank {
        id: &account.id,
        priority: account.automation.priority,
        headroom: windows
            .iter()
            .flatten()
            .all(|w| f64::from(w.used_percent) <= margin),
        // Unknown usage sorts last when needed for ranking, never as unused quota.
        weekly_used: week.map_or(f64::INFINITY, |w| f64::from(w.used_percent)),
        weekly_reset_bucket: week.and_then(|w| w.resets_at).map(reset_hour_bucket),
        five_hour_used: five.map_or(f64::INFINITY, |w| f64::from(w.used_percent)),
    })
}
/// When the main windows at or above `threshold` reopen: the latest of their resets.
/// `None` when one is unknown, or when only a flag limits the account.
fn frees_at(quota: &CodexQuotaView, threshold: f64) -> Option<i64> {
    let resets: Option<Vec<i64>> = main_windows(quota)
        .filter(|window| f64::from(window.used_percent) >= threshold)
        .map(|window| window.resets_at)
        .collect();
    resets?.into_iter().max()
}

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
    #[serde(default)]
    needs_sign_in: bool,
    #[serde(default)]
    last_attempt_at: Option<i64>,
    /// Consecutive readings that ended in an authentication error. One can be a
    /// transient refresh failure; two in a row mean the sign-in is gone.
    #[serde(default)]
    auth_failures: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_reset_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_reset_outcome: Option<ResetOutcome>,
    #[serde(default, skip_serializing_if = "AccountAutomation::is_default")]
    automation: AccountAutomation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preferred_return_reset_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preferred_return_handled_reset_at: Option<i64>,
    /// API key only: Codex's `service_tier` is "flex" while this key is the active login.
    #[serde(default, skip_serializing_if = "is_false")]
    use_flex: bool,
}
fn is_false(value: &bool) -> bool {
    !*value
}
impl Drop for SavedAccount {
    fn drop(&mut self) {
        self.auth.zeroize();
    }
}
impl SavedAccount {
    /// Replace the saved login with a newer copy of the same account's login and keep
    /// the binding in step with it (claims such as the email can change between token
    /// generations). Callers have already checked that the owner is unchanged.
    fn set_auth(&mut self, auth: &OpaqueAuth) -> Result<(), CodexReason> {
        let binding = Binding::from_auth(auth)?;
        self.auth.zeroize();
        self.auth = auth.as_bytes().to_vec();
        self.auth_kind = auth.kind();
        self.binding = binding;
        Ok(())
    }
    fn set_merged(&mut self, bytes: Vec<u8>) -> Result<(), CodexReason> {
        let auth = OpaqueAuth::parse(bytes).map_err(provider_reason)?;
        self.set_auth(&auth)
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
        Self::from_claims(auth.routing_claims())
    }
    fn from_claims(claims: &UnverifiedRoutingClaims) -> Result<Self, CodexReason> {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    return_hold_until: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_auto_switch_at: Option<i64>,
    /// Set while PrimerSwitch has put `service_tier = "flex"` into Codex's config.toml:
    /// what to put back, kept across restarts so a crash never leaves Flex on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flex_applied: Option<FlexMark>,
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            format: 1,
            accounts: Vec::new(),
            return_hold_until: None,
            last_auto_switch_at: None,
            flex_applied: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientRole {
    Login,
    Active,
    Owned,
}
#[async_trait]
pub(crate) trait ClientFactory: Send + Sync {
    async fn start(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        context: CodexContext,
        role: ClientRole,
    ) -> Result<Box<dyn CodexService>, CodexError>;
}
struct NativeFactory;
#[async_trait]
impl ClientFactory for NativeFactory {
    async fn start(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        context: CodexContext,
        role: ClientRole,
    ) -> Result<Box<dyn CodexService>, CodexError> {
        let executable = executable.ok_or(CodexError::NotFound)?;
        Ok(Box::new(match role {
            ClientRole::Login => CodexClient::start_login(executable, context).await?,
            ClientRole::Active => CodexClient::start_active(executable, context).await?,
            ClientRole::Owned => CodexClient::start_owned(executable, context).await?,
        }))
    }
}
#[async_trait]
pub(crate) trait DaemonControl: Send + Sync {
    async fn state(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
    ) -> Result<DaemonState, CodexError>;
    async fn restart(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        environment: Option<&[(OsString, OsString)]>,
    ) -> Result<(), CodexError>;
}
struct NativeDaemon;
#[async_trait]
impl DaemonControl for NativeDaemon {
    async fn state(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
    ) -> Result<DaemonState, CodexError> {
        daemon_state(executable.ok_or(CodexError::NotFound)?).await
    }
    async fn restart(
        &self,
        executable: Option<&VerifiedCodexExecutable>,
        environment: Option<&[(OsString, OsString)]>,
    ) -> Result<(), CodexError> {
        restart_daemon(executable.ok_or(CodexError::NotFound)?, environment).await
    }
}
pub(crate) trait ProcessInventory: Send + Sync {
    fn scan(&self, home: &Path) -> CodexProcessSummary;
    fn environment(&self, pid: u32) -> Option<Vec<(OsString, OsString)>>;
    fn elevated(&self) -> bool;
}
struct NativeInventory;
impl ProcessInventory for NativeInventory {
    fn scan(&self, home: &Path) -> CodexProcessSummary {
        scan_codex_processes(home)
    }
    fn environment(&self, pid: u32) -> Option<Vec<(OsString, OsString)>> {
        process_environment(pid)
    }
    fn elevated(&self) -> bool {
        process_is_elevated()
    }
}
/// lampese "Codex Switcher" (`~/.codex-switcher/accounts.json`, store version 1).
pub(crate) trait SwitcherSource: Send + Sync {
    fn exists(&self) -> bool;
    fn read(&self) -> Result<Zeroizing<Vec<u8>>, CodexReason>;
}
struct NativeSwitcher;
impl NativeSwitcher {
    fn path() -> Option<PathBuf> {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .map(|home| home.join(".codex-switcher").join("accounts.json"))
    }
}
impl SwitcherSource for NativeSwitcher {
    fn exists(&self) -> bool {
        Self::path().is_some_and(|path| path.is_file())
    }
    fn read(&self) -> Result<Zeroizing<Vec<u8>>, CodexReason> {
        let path = Self::path().ok_or(CodexReason::SwitcherUnavailable)?;
        let meta =
            std::fs::symlink_metadata(&path).map_err(|_| CodexReason::SwitcherUnavailable)?;
        if !meta.is_file() || meta.len() > 16 * 1024 * 1024 {
            return Err(CodexReason::SwitcherUnavailable);
        }
        std::fs::read(path)
            .map(Zeroizing::new)
            .map_err(|_| CodexReason::SwitcherUnavailable)
    }
}

struct Installation {
    executable: Option<VerifiedCodexExecutable>,
    executable_path: PathBuf,
    version: String,
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

/// A quota reading prepared under the owner lock and run outside it.
pub(crate) struct QuotaJob {
    account_id: String,
    binding: Binding,
    executable: Option<VerifiedCodexExecutable>,
    factory: Arc<dyn ClientFactory>,
    target: QuotaTarget,
}
enum QuotaTarget {
    /// The active login, read through the user's own CODEX_HOME.
    Active(CodexContext),
    /// A saved login copied into an application-owned home; Codex may rotate it there.
    Owned {
        home: TempDir,
        context: CodexContext,
        original: Zeroizing<Vec<u8>>,
    },
}
/// The saved login copied into an owned home, and that file after the reading.
struct OwnedResult {
    original: Zeroizing<Vec<u8>>,
    rotated: Option<Zeroizing<Vec<u8>>>,
}
pub(crate) struct QuotaReport {
    account_id: String,
    binding: Binding,
    owned: Option<OwnedResult>,
    outcome: Result<CodexRateLimits, CodexError>,
    reset: Option<(String, Result<ResetOutcome, CodexError>)>,
}
impl QuotaJob {
    pub(crate) async fn run(self) -> QuotaReport {
        let (context, role) = match &self.target {
            QuotaTarget::Active(context) => (context.clone(), ClientRole::Active),
            QuotaTarget::Owned { context, .. } => (context.clone(), ClientRole::Owned),
        };
        let outcome = async {
            let mut client = self
                .factory
                .start(self.executable.as_ref(), context, role)
                .await?;
            let result = client.read_rate_limits().await;
            let _ = client.shutdown().await;
            result
        }
        .await;
        let owned = self.target.finish().await;
        QuotaReport {
            account_id: self.account_id,
            binding: self.binding,
            owned,
            outcome,
            reset: None,
        }
    }
}
impl QuotaTarget {
    async fn finish(self) -> Option<OwnedResult> {
        match self {
            QuotaTarget::Owned { home, original, .. } => {
                // Codex writes auth.json in place; give a just-exited child's file a
                // moment instead of losing a rotation to a transient sharing error.
                let mut rotated = None;
                for _ in 0..10 {
                    match read_private_auth(home.path()) {
                        Ok(bytes) => {
                            rotated = bytes;
                            break;
                        }
                        Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                    }
                }
                scrub_owned_home(home.path());
                drop(home);
                Some(OwnedResult { original, rotated })
            }
            QuotaTarget::Active(_) => None,
        }
    }
}
/// Overwrite and remove the private copy of a login, then the whole directory,
/// including read-only files a Codex child may have created.
fn scrub_owned_home(home: &Path) {
    let auth = home.join("auth.json");
    for _ in 0..20 {
        if let Ok(meta) = std::fs::metadata(&auth) {
            let _ = std::fs::write(&auth, vec![0u8; meta.len() as usize]);
        }
        if std::fs::remove_file(&auth).is_ok() || !auth.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    remove_tree(home);
}
fn remove_tree(path: &Path) {
    fn writable(path: &Path, depth: usize) {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return;
        };
        if meta.is_dir() && depth < 64 {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    writable(&entry.path(), depth + 1);
                }
            }
        } else if meta.permissions().readonly() {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(path, permissions);
        }
    }
    writable(path, 0);
    let _ = std::fs::remove_dir_all(path);
}
/// Owned homes left behind by a crash: wipe the ones older than an hour.
fn remove_stale_owned_homes() {
    let Ok(parent) = std::env::temp_dir().canonicalize() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten().take(10_000) {
        let name = entry.file_name();
        let stale = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age.as_secs() > 3600);
        if stale
            && name.to_string_lossy().starts_with("primerswitch-codex-")
            && entry.file_type().is_ok_and(|t| t.is_dir())
        {
            scrub_owned_home(&entry.path());
        }
    }
}

/// The slow part of a switch, run outside the owner lock.
pub(crate) struct SwitchJob {
    target_id: String,
    executable: Option<VerifiedCodexExecutable>,
    daemon: Arc<dyn DaemonControl>,
    /// The running daemon's own environment, so the restarted daemon (and the shell
    /// commands it runs for every terminal) keeps it.
    environment: Option<Vec<(OsString, OsString)>>,
}
pub(crate) struct SwitchOutcome {
    daemon_running: bool,
    restart: Option<Result<(), CodexError>>,
}
impl SwitchJob {
    pub(crate) async fn run(&self) -> SwitchOutcome {
        match self.daemon.state(self.executable.as_ref()).await {
            Ok(DaemonState::Running) => SwitchOutcome {
                daemon_running: true,
                restart: Some(
                    self.daemon
                        .restart(self.executable.as_ref(), self.environment.as_deref())
                        .await,
                ),
            },
            Ok(DaemonState::NotRunning) => SwitchOutcome {
                daemon_running: false,
                restart: None,
            },
            // The probe itself failed: report it, but never start a daemon blindly.
            Err(error) => SwitchOutcome {
                daemon_running: false,
                restart: Some(Err(error)),
            },
        }
    }
    /// A second restart after repairing a file that an old process rewrote mid-drain.
    pub(crate) async fn restart_again(&self) -> SwitchOutcome {
        self.run().await
    }
    pub(crate) fn target_id(&self) -> &str {
        &self.target_id
    }
}

pub(crate) enum CodexCommand {
    Discover,
    BeginLogin(u64),
    PollLogin(String),
    CancelLogin(String),
    ImportCurrent,
    ImportSwitcher,
    /// Display name and the typed key; the key is wiped when the command is dropped.
    AddApiKey(String, Zeroizing<String>),
    /// Turn Flex processing on or off for one saved API key.
    SetApiKeyFlex(String, bool),
    Delete(String),
    UpdateAccountAutomation(String, AccountAutomationView),
    ResumeAutomaticReturns,
    Observe,
}
pub(crate) enum CodexOutput {
    Snapshot,
    Login(CodexLoginLaunch),
}
pub(crate) struct CodexEngine {
    saved: Saved,
    writable: bool,
    installation: Option<Installation>,
    factory: Arc<dyn ClientFactory>,
    daemon: Arc<dyn DaemonControl>,
    processes: Arc<dyn ProcessInventory>,
    switcher: Arc<dyn SwitcherSource>,
    pub intent: Arc<AtomicU64>,
    pending_login: Option<PendingLogin>,
    login_view: Option<CodexLoginView>,
    /// Saved account created or refreshed by the last completed browser sign-in.
    pub(crate) last_login_account: Option<String>,
    selected_id: Option<String>,
    observed_generation: Option<StoreGeneration>,
    verified_now: BTreeSet<String>,
    account_errors: BTreeMap<String, CodexReason>,
    availability: CodexAvailability,
    reason: Option<CodexReason>,
    pub error: Option<CodexReason>,
    pub revision: u64,
    demo: bool,
    switching: Option<CodexSwitchView>,
    last_switch: Option<CodexLastSwitchView>,
    /// Flex change made by the switch in progress, reported with its outcome.
    flex_notice: Option<CodexFlexChange>,
    /// Why the last attempt to turn Flex on for the active Flex key failed.
    flex_apply_failed: Option<CodexReason>,
    environment: CodexEnvironmentView,
    switcher_available: bool,
    discovered_at: Option<i64>,
    /// Session-only notice throttling; cooldowns and manual holds are durable.
    last_exhausted_notice_at: Option<i64>,
    last_auto_failure: Option<(String, CodexReason, i64)>,
    /// Current provider default supplied by the serialized runtime owner. The
    /// authoritative value lives only in shared Settings, not this Codex record.
    policy_threshold: f64,
    /// Alternate background slots when both the active account and an inactive
    /// account are due. Frequent active checks must not starve reset recovery.
    last_background_active: Option<bool>,
    #[cfg(test)]
    before_automatic_replace: Option<Box<dyn FnOnce() + Send>>,
}
impl CodexEngine {
    pub fn load(vault: &Vault) -> Self {
        let mut engine = Self::empty(false);
        match vault.load::<Saved>(RECORD) {
            Ok(Some(saved)) if saved.format == 1 && saved.accounts.len() <= 1000 => {
                let (saved, quarantined) = sanitize(saved);
                if !quarantined.accounts.is_empty() {
                    // Keep unreadable records (still encrypted) instead of refusing the store.
                    let mut kept: Saved = vault.load(QUARANTINE).ok().flatten().unwrap_or_default();
                    kept.accounts.extend(quarantined.accounts);
                    let _ = vault.save(QUARANTINE, &kept);
                    let _ = vault.save(RECORD, &saved);
                }
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
            daemon: Arc::new(NativeDaemon),
            processes: Arc::new(NativeInventory),
            switcher: Arc::new(NativeSwitcher),
            intent: Arc::new(AtomicU64::new(0)),
            pending_login: None,
            login_view: None,
            last_login_account: None,
            selected_id: None,
            observed_generation: None,
            verified_now: BTreeSet::new(),
            account_errors: Default::default(),
            availability: CodexAvailability::Unqualified,
            reason: None,
            error: None,
            revision: 0,
            demo,
            switching: None,
            last_switch: None,
            flex_notice: None,
            flex_apply_failed: None,
            environment: CodexEnvironmentView::default(),
            switcher_available: false,
            discovered_at: None,
            last_exhausted_notice_at: None,
            last_auto_failure: None,
            policy_threshold: CodexPolicy::default().threshold,
            last_background_active: None,
            #[cfg(test)]
            before_automatic_replace: None,
        }
    }
    /// Discover once at startup, and retry a failed discovery every `retry` seconds.
    pub(crate) fn needs_discovery(&self, now: i64, retry: i64) -> bool {
        if self.demo || !self.writable || self.switching.is_some() || self.pending_login.is_some() {
            return false;
        }
        match self.discovered_at {
            None => true,
            Some(at) => self.installation.is_none() && (now < at || now - at >= retry),
        }
    }

    pub fn demo(now: i64) -> Self {
        let mut engine = Self::empty(true);
        engine.availability = CodexAvailability::Supported;
        let window = |used: i32, minutes: i64, reset: i64| CodexWindowView {
            used_percent: used,
            window_duration_mins: Some(minutes),
            resets_at: Some(now + reset),
        };
        for (index, name, plan, five, week, five_reset) in [
            (0, "Studio Codex", "pro", 42, 35, 7200),
            (1, "Personal Codex", "plus", 100, 61, 8100),
            (2, "Research Codex", "prolite", 8, 12, 15_300),
        ] {
            let id = format!("codex-demo-{index}");
            engine.saved.accounts.push(SavedAccount {
                id: id.clone(),
                name: name.into(),
                auth: Vec::new(),
                auth_kind: AuthKind::ManagedChatgpt,
                binding: Binding {
                    user: Some(format!("codex-demo-user-{index}")),
                    workspace: Some(format!("codex-demo-workspace-{index}")),
                    email: Some(format!("codex-{index}@example.invalid")),
                },
                evidence: CodexIdentityEvidence::BackendVerified,
                managed_origin: true,
                quota: Some(CodexQuotaView {
                    ordinary_usage_allowed: Some(five < 100),
                    limits: vec![CodexLimitView {
                        key: "default".into(),
                        limit_id: Some("codex".into()),
                        limit_name: Some("Codex".into()),
                        normal_model_slug: None,
                        primary: Some(window(five, 300, five_reset)),
                        secondary: Some(window(week, 10080, 259_200)),
                        plan_type: Some(plan.into()),
                        credits: None,
                        spend_control_reached: Some(false),
                        rate_limit_reached_type: (five >= 100).then(|| "primary".into()),
                    }],
                    reset_credits_available: None,
                    reset_credit_details: None,
                }),
                quota_read_at: Some(now),
                needs_sign_in: false,
                last_attempt_at: Some(now),
                auth_failures: 0,
                pending_reset_key: None,
                last_reset_outcome: None,
                automation: AccountAutomation::default(),
                preferred_return_reset_at: None,
                preferred_return_handled_reset_at: None,
                use_flex: false,
            });
            engine.verified_now.insert(id);
        }
        engine.selected_id = Some("codex-demo-0".into());
        engine.environment = CodexEnvironmentView {
            daemon_running: Some(true),
            other_clients: 0,
            codex_switcher_running: false,
        };
        engine
    }
    pub fn unavailable(&mut self) {
        *self = Self::empty(false);
        self.writable = false;
        self.reason = Some(CodexReason::VaultUnavailable);
    }
    pub(crate) fn set_policy(&mut self, policy: &CodexPolicy, now: i64) {
        self.policy_threshold = policy.threshold;
        // A lower provider default can trigger a switch from an already fresh
        // observation, before the next reader. Arm that same observed window.
        for index in 0..self.saved.accounts.len() {
            self.observe_return_window(index, now);
        }
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
        if self.demo || !self.writable {
            return Err(CodexReason::VaultUnavailable);
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
        Ok(CodexFileStore::new(context))
    }
    /// Re-evaluate the configuration from scratch before any mutation: Codex edits
    /// config.toml itself, and edits are judged on their content, not as conflicts.
    fn requalify(&mut self) -> Result<(), CodexReason> {
        let installation = self
            .installation
            .as_mut()
            .ok_or(CodexReason::NotInstalled)?;
        installation.qualified = installation.paths.qualify(
            &installation.executable_path,
            installation.workspace.path(),
            &installation.version,
        )?;
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
    fn common_block(&self) -> Option<CodexReason> {
        if self.demo || !self.writable {
            Some(CodexReason::VaultUnavailable)
        } else if self.installation.is_none() {
            Some(self.reason.unwrap_or(CodexReason::NotInstalled))
        } else {
            None
        }
    }
    fn switch_capability(&self, account: &SavedAccount) -> CodexCapability {
        if self.selected_id.as_deref() == Some(&account.id) {
            return CodexCapability {
                enabled: false,
                blocked_reason: None,
            };
        }
        let reason = self.common_block().or_else(|| {
            if self.switching.is_some() {
                Some(CodexReason::SwitchInProgress)
            } else if account.auth_kind == AuthKind::Unsupported {
                Some(CodexReason::UnsupportedAuth)
            } else if account.auth_kind == AuthKind::ApiKey {
                // A key has no sign-in to expire and no workspace to verify. A Flex key
                // is only switched to when Flex can be set: never at the standard tier.
                account.use_flex.then(|| self.flex_block()).flatten()
            } else if account.needs_sign_in {
                Some(CodexReason::SignInRequired)
            } else if !account.binding.complete() {
                Some(CodexReason::IdentityUnverified)
            } else {
                None
            }
        });
        reason.map_or_else(CodexCapability::allowed, CodexCapability::blocked)
    }
    /// Why Flex cannot be set now: no qualified Codex home, or a Codex older than the
    /// first version it was verified on.
    fn flex_block(&self) -> Option<CodexReason> {
        self.store().err().or_else(|| {
            self.installation
                .as_ref()
                .is_some_and(|installation| !supports_flex(&installation.version))
                .then_some(CodexReason::FlexUnsupportedVersion)
        })
    }
    /// Whether the Flex setting of `account` can be changed. Turning it off is always
    /// possible; turning it on needs [`Self::flex_block`] to be clear.
    fn flex_capability(&self, account: &SavedAccount) -> CodexCapability {
        let reason = self.common_block().or_else(|| {
            if self.switching.is_some() {
                Some(CodexReason::SwitchInProgress)
            } else if account.auth_kind != AuthKind::ApiKey {
                Some(CodexReason::UnsupportedAuth)
            } else if account.use_flex {
                None
            } else {
                self.flex_block()
            }
        });
        reason.map_or_else(CodexCapability::allowed, CodexCapability::blocked)
    }
    /// The active login is an API key that uses Flex.
    fn flex_wanted(&self) -> bool {
        self.selected()
            .is_some_and(|account| account.auth_kind == AuthKind::ApiKey && account.use_flex)
    }
    /// Flex is set but the active login is not a Flex key: ChatGPT rejects it.
    fn flex_restore_pending(&self) -> bool {
        self.saved.flex_applied.is_some() && !self.flex_wanted()
    }
    fn flex_config(&self) -> Result<CodexConfigFlex, CodexReason> {
        Ok(CodexConfigFlex::new(self.store()?.context().clone()))
    }
    /// Set `service_tier = "flex"`. What to put back is saved durably BEFORE the file
    /// changes, so no crash can leave Flex in config.toml without a mark: a mark with a
    /// file that does not say Flex is cleared by the next restore, and a mark with Flex
    /// in the file is restored. Does nothing while this app's own setting is in place.
    fn apply_flex(&mut self, vault: &Vault) -> Result<(), CodexReason> {
        let config = self
            .flex_config()
            .map_err(|_| CodexReason::FlexConfigFailed)?;
        if self.saved.flex_applied.is_some()
            && config
                .current()
                .is_ok_and(|value| value.as_deref() == Some("flex"))
        {
            return Ok(());
        }
        let mark = config.plan().map_err(|_| CodexReason::FlexConfigFailed)?;
        let before = self.saved.flex_applied.replace(mark);
        if let Err(error) = self.save(vault) {
            // Nothing was written to config.toml yet.
            self.saved.flex_applied = before;
            return Err(error);
        }
        if config.apply(vault).is_err() {
            // The file may still have taken the change (a conflict after the replace):
            // then the mark stays and the next restore puts the value back.
            if config
                .current()
                .is_ok_and(|value| value.as_deref() != Some("flex"))
            {
                self.saved.flex_applied = None;
                let _ = self.save(vault);
            }
            return Err(CodexReason::FlexConfigFailed);
        }
        // The new file is a normal Codex config: take it as the qualified state.
        let _ = self.requalify();
        Ok(())
    }
    /// Put the saved `service_tier` back unless the owner changed it since. A failure
    /// keeps the mark, so it is retried. Returns whether Flex is off in the file now.
    fn restore_flex(&mut self, vault: &Vault) -> Result<bool, CodexReason> {
        let Some(mark) = self.saved.flex_applied.clone() else {
            return Ok(false);
        };
        let off = self
            .flex_config()
            .ok()
            .and_then(|config| config.restore(&mark, vault).ok())
            .ok_or(CodexReason::FlexRestoreFailed)?;
        self.saved.flex_applied = None;
        self.save(vault)?;
        let _ = self.requalify();
        Ok(off)
    }
    /// Make config.toml agree with the active login: Flex is on exactly while a Flex key
    /// is active. Runs at startup, after every observed change of the login (also one
    /// made outside the app), after a toggle and as the retry of a failed restore.
    fn reconcile_flex(&mut self, vault: &Vault) -> Result<(), CodexReason> {
        if self.demo || !self.writable || self.switching.is_some() || self.store().is_err() {
            return Ok(());
        }
        let result = match (self.flex_wanted(), self.saved.flex_applied.is_some()) {
            (false, true) => self.restore_flex(vault).map(|_| ()),
            (true, false) => match self.flex_block() {
                Some(reason) => Err(reason),
                None => self.apply_flex(vault),
            },
            _ => Ok(()),
        };
        self.flex_apply_failed = result.err();
        result
    }
    /// The Flex part of a switch, done before the login file is written: on for a Flex
    /// key (a failure aborts the switch, which would otherwise bill the standard tier),
    /// off again when leaving Flex (a failure never blocks the switch).
    fn switch_flex(&mut self, to_flex: bool, vault: &Vault) -> Result<(), CodexReason> {
        if to_flex {
            self.apply_flex(vault)?;
            self.flex_notice = Some(CodexFlexChange::On);
        } else if self.saved.flex_applied.is_some() {
            match self.restore_flex(vault) {
                Ok(off) => self.flex_notice = off.then_some(CodexFlexChange::Off),
                Err(CodexReason::FlexRestoreFailed) => (),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    fn selected(&self) -> Option<&SavedAccount> {
        let id = self.selected_id.as_deref()?;
        self.saved.accounts.iter().find(|a| a.id == id)
    }
    fn quota_state(&self, account: &SavedAccount, now: i64) -> CodexQuotaState {
        if self.account_errors.contains_key(&account.id) {
            CodexQuotaState::Unavailable
        } else if account.quota.is_none() {
            CodexQuotaState::Unread
        } else if self.verified_now.contains(&account.id)
            && account
                .quota_read_at
                .is_some_and(|time| now >= time && now - time <= FRESH_SECONDS)
        {
            CodexQuotaState::Fresh
        } else {
            CodexQuotaState::Cached
        }
    }
    /// Accounts automatic switching may use, with their ranking inputs: switchable
    /// saved ChatGPT sign-ins (not active, not signed out) whose latest reading
    /// succeeded and shows at least one main window, all below the threshold, and no limit.
    /// An API key is billed per token and is only ever selected by hand.
    fn candidates(
        &self,
        threshold: f64,
    ) -> impl Iterator<Item = (&SavedAccount, CandidateRank<'_>)> {
        self.saved
            .accounts
            .iter()
            .filter(move |a| {
                a.auth_kind == AuthKind::ManagedChatgpt
                    && a.automation.enabled
                    && !self.account_errors.contains_key(&a.id)
                    && self.switch_capability(a).enabled
            })
            .filter_map(move |a| {
                Some((
                    a,
                    usable_rank(a, a.automation.effective_threshold(threshold))?,
                ))
            })
    }
    /// The account automatic switching uses next, by the order shared with Claude:
    /// preferred headroom, then `policy.order`, then the lower 5-hour usage, then id.
    fn next_candidate(&self, policy: &CodexPolicy) -> Option<&SavedAccount> {
        self.candidates(policy.threshold)
            .min_by(|(_, a), (_, b)| compare_candidates(a, b, policy.order))
            .map(|(account, _)| account)
    }
    /// Every candidate, best first, by the same comparator as `next_candidate`, so the
    /// first entry is always the next account. The window shows it as the usage order.
    fn ranked_candidates(&self, policy: &CodexPolicy) -> Vec<&SavedAccount> {
        let mut ranked: Vec<_> = self.candidates(policy.threshold).collect();
        ranked.sort_by(|(_, a), (_, b)| compare_candidates(a, b, policy.order));
        ranked.into_iter().map(|(account, _)| account).collect()
    }
    /// Decide automatic switching. A switch needs automatic switching on, no switch,
    /// sign-in or cooldown in progress and a fresh target reading. Exhaustion
    /// switching also needs fresh active usage; a natural return instead validates
    /// the current active owner through the guarded credential transaction.
    pub(crate) fn automation(&self, now: i64, policy: &CodexPolicy) -> CodexAutomation {
        let cooling = self
            .saved
            .last_auto_switch_at
            .is_some_and(|at| now < at || now.saturating_sub(at) < AUTO_SWITCH_COOLDOWN);
        if !policy.auto_switch
            || cooling
            || self.common_block().is_some()
            || self.switching.is_some()
            || self.pending_login.is_some()
        {
            return CodexAutomation::Idle;
        }
        let Some(active) = self.selected() else {
            return CodexAutomation::Idle;
        };
        if self.quota_state(active, now) != CodexQuotaState::Fresh {
            return self.automatic_return(active, now, policy);
        }
        let threshold = active.automation.effective_threshold(policy.threshold);
        let Some(quota) = active.quota.as_ref().filter(|q| reaches(q, threshold)) else {
            return self.automatic_return(active, now, policy);
        };
        match self.next_candidate(policy) {
            Some(target) if self.quota_state(target, now) == CodexQuotaState::Fresh => {
                CodexAutomation::Switch(CodexAutoSwitch {
                    previous_id: active.id.clone(),
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    previous_name: active.name.clone(),
                    peak: main_windows(quota).map(|w| w.used_percent).max(),
                })
            }
            Some(target) => CodexAutomation::Refresh(target.id.clone()),
            None if self.others_known_unusable(policy.threshold) => CodexAutomation::Exhausted {
                frees_first: self
                    .saved
                    .accounts
                    .iter()
                    .filter(|a| a.auth_kind == AuthKind::ManagedChatgpt && a.automation.enabled)
                    .filter_map(|a| {
                        Some((
                            frees_at(
                                a.quota.as_ref()?,
                                a.automation.effective_threshold(policy.threshold),
                            )?,
                            a,
                        ))
                    })
                    .min_by_key(|(at, _)| *at)
                    .map(|(at, a)| (a.name.clone(), at)),
            },
            None => CodexAutomation::Idle,
        }
    }
    /// Returning is opt-in, only to an explicitly higher priority, and requires an
    /// observed natural reset plus a fresh backend-identified recovery reading.
    fn automatic_return(
        &self,
        active: &SavedAccount,
        now: i64,
        policy: &CodexPolicy,
    ) -> CodexAutomation {
        if active.auth_kind != AuthKind::ManagedChatgpt
            || !active.binding.complete()
            || self
                .saved
                .return_hold_until
                .is_some_and(|until| until > now)
        {
            return CodexAutomation::Idle;
        }
        let mut targets: Vec<_> = self
            .saved
            .accounts
            .iter()
            .filter(|a| {
                a.auth_kind == AuthKind::ManagedChatgpt
                    && a.automation.enabled
                    && a.automation.return_on_five_hour_reset
                    && a.automation.is_higher_priority_than(&active.automation)
                    && self.switch_capability(a).enabled
                    && a.pending_reset_key.is_none()
                    && a.preferred_return_reset_at.is_some_and(|reset| {
                        reset <= now
                            && a.preferred_return_handled_reset_at
                                .is_none_or(|handled| handled < reset)
                    })
            })
            .collect();
        targets.sort_by(|a, b| {
            a.automation
                .priority
                .cmp(&b.automation.priority)
                .then_with(|| {
                    match (
                        usable_rank(a, a.automation.effective_threshold(policy.threshold)),
                        usable_rank(b, b.automation.effective_threshold(policy.threshold)),
                    ) {
                        (Some(a), Some(b)) => compare_candidates(&a, &b, policy.order),
                        _ => a.id.cmp(&b.id),
                    }
                })
        });
        for target in targets {
            let reset = target.preferred_return_reset_at.unwrap();
            let read_after_reset = target.quota_read_at.is_some_and(|read| read >= reset);
            let recovered = natural_five_hour_recovery(target, reset, now);
            let headroom = usable_rank(
                target,
                target.automation.effective_threshold(policy.threshold),
            )
            .is_some_and(|rank| rank.headroom);
            // A known post-reset weekly limit, a missing window or a missing date
            // waits for ordinary/reset scheduling, never a tight recovery loop.
            if read_after_reset && !(recovered && headroom) {
                continue;
            }
            if read_after_reset && self.quota_state(target, now) == CodexQuotaState::Fresh {
                return CodexAutomation::Return(CodexAutoSwitch {
                    previous_id: active.id.clone(),
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    previous_name: active.name.clone(),
                    peak: None,
                });
            }
            if self.recovery_read_due(target, now) {
                return CodexAutomation::Refresh(target.id.clone());
            }
        }
        CodexAutomation::Idle
    }
    fn recovery_read_due(&self, account: &SavedAccount, now: i64) -> bool {
        let failed = self.account_errors.contains_key(&account.id)
            || account
                .last_attempt_at
                .is_some_and(|attempt| account.quota_read_at.is_none_or(|read| attempt > read));
        let floor = if failed { ERROR_RETRY } else { RETURN_RETRY };
        account
            .last_attempt_at
            .is_none_or(|at| now >= at && now.saturating_sub(at) >= floor)
    }
    pub(crate) fn selected_threshold(&self, policy: &CodexPolicy) -> f64 {
        self.selected().map_or(policy.threshold, |a| {
            a.automation.effective_threshold(policy.threshold)
        })
    }
    /// Every other saved ChatGPT account has a successful reading that rules it out;
    /// an unread, failed or signed-out account leaves "all limited" unproven.
    fn others_known_unusable(&self, threshold: f64) -> bool {
        self.saved
            .accounts
            .iter()
            .filter(|a| {
                self.selected_id.as_deref() != Some(&a.id)
                    && a.auth_kind == AuthKind::ManagedChatgpt
                    && a.automation.enabled
            })
            .all(|a| {
                !a.needs_sign_in
                    && !self.account_errors.contains_key(&a.id)
                    && a.quota
                        .as_ref()
                        .is_some_and(|q| reaches(q, a.automation.effective_threshold(threshold)))
            })
    }
    /// An automatic switch is starting: the cooldown runs whatever its outcome.
    pub(crate) fn begin_auto_switch(&mut self, now: i64, vault: &Vault) -> Result<(), CodexReason> {
        self.saved.last_auto_switch_at = Some(now);
        self.save_automation(vault)
    }
    pub(crate) fn hold_automatic_returns(
        &mut self,
        now: i64,
        vault: &Vault,
    ) -> Result<(), CodexReason> {
        self.saved.return_hold_until = Some(now.saturating_add(MANUAL_RETURN_HOLD));
        self.save_automation(vault)
    }
    fn save_automation(&mut self, vault: &Vault) -> Result<(), CodexReason> {
        if let Err(error) = self.save(vault) {
            self.writable = false;
            self.reason = Some(error);
            return Err(error);
        }
        Ok(())
    }
    /// Whether to send the every-account-limited notice now (and record it).
    pub(crate) fn exhausted_notice_due(&mut self, now: i64) -> bool {
        if self
            .last_exhausted_notice_at
            .is_some_and(|at| now >= at && now - at < AUTO_NOTICE_INTERVAL)
        {
            return false;
        }
        self.last_exhausted_notice_at = Some(now);
        true
    }
    /// Whether to notify this automatic switch failure now (and record it).
    pub(crate) fn failure_notice_due(
        &mut self,
        target: &str,
        reason: CodexReason,
        now: i64,
    ) -> bool {
        if self
            .last_auto_failure
            .as_ref()
            .is_some_and(|(id, last, at)| {
                id == target && *last == reason && now >= *at && now - at < AUTO_NOTICE_INTERVAL
            })
        {
            return false;
        }
        self.last_auto_failure = Some((target.into(), reason, now));
        true
    }
    pub fn snapshot(&self, busy: bool, now: i64, policy: &CodexPolicy) -> CodexSnapshot {
        let mut result = CodexSnapshot::empty(self.demo);
        result.revision = self.revision;
        result.busy = busy;
        result.error = self.error;
        result.availability = self.availability;
        result.blocked_reason = self.reason;
        result.login = self.login_view.clone();
        result.selected_id = self.selected_id.clone();
        result.next_id = self.next_candidate(policy).map(|a| a.id.clone());
        result.order = self
            .ranked_candidates(policy)
            .into_iter()
            .map(|a| a.id.clone())
            .collect();
        result.switching = self.switching.clone();
        result.last_switch = self.last_switch.clone();
        result.return_hold_until = self.saved.return_hold_until.filter(|until| *until > now);
        result.environment = self.environment.clone();
        if self.demo {
            result.executable_version = Some(MIN_CODEX_VERSION.into());
            result.active_model = Some("gpt-6".into());
        }
        if let Some(installation) = &self.installation {
            result.executable_version = Some(installation.version.clone());
            result.active_model = installation.qualified.active_model.clone();
        }
        if !self.demo && ContextPaths::credential_overrides_present() {
            result.warnings.push(CodexReason::ExternalCredentials);
        }
        if !self.demo && self.flex_restore_pending() {
            result.warnings.push(CodexReason::FlexRestoreFailed);
        }
        if let Some(reason) = self.flex_apply_failed
            && !self.demo
            && self.flex_wanted()
            && self.saved.flex_applied.is_none()
        {
            result.warnings.push(reason);
        }
        let common = self.common_block();
        let capability = |reason: Option<CodexReason>| {
            reason.map_or_else(CodexCapability::allowed, CodexCapability::blocked)
        };
        result.capabilities = CodexCapabilities {
            login_browser: capability(common),
            import_current: capability(common.or_else(|| self.store().err())),
            import_switcher: capability(common.or_else(|| {
                (!self.switcher_available).then_some(CodexReason::SwitcherUnavailable)
            })),
            add_api_key: capability(common.or_else(|| self.store().err())),
            refresh_quota: capability(common),
            switch_account: capability(
                common.or(self
                    .switching
                    .is_some()
                    .then_some(CodexReason::SwitchInProgress)),
            ),
            delete_saved: capability(
                (!self.writable || self.demo).then_some(CodexReason::VaultUnavailable),
            ),
        };
        result.accounts = self
            .saved
            .accounts
            .iter()
            .map(|account| {
                let plan_type = account
                    .quota
                    .as_ref()
                    .and_then(|q| q.limits.iter().find(|l| l.key == "default"))
                    .and_then(|l| l.plan_type.clone())
                    .or_else(|| {
                        OpaqueAuth::inspect(&account.auth)
                            .ok()
                            .and_then(|i| i.claims.plan_type)
                    })
                    .map(|plan| plan.to_ascii_lowercase());
                CodexAccountView {
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
                    plan_type,
                    identity_evidence: account.evidence,
                    identity_verified: self.verified_now.contains(&account.id),
                    api_key_tail: (account.auth_kind == AuthKind::ApiKey)
                        .then(|| api_key_tail(&account.auth))
                        .flatten(),
                    selected: self.selected_id.as_ref() == Some(&account.id),
                    switchable: self.switch_capability(account),
                    use_flex: account.use_flex,
                    flex: self.flex_capability(account),
                    quota: account.quota.clone(),
                    quota_read_at: account.quota_read_at,
                    quota_state: self.quota_state(account, now),
                    error: self.account_errors.get(&account.id).copied(),
                    needs_sign_in: account.needs_sign_in,
                    automation: (&account.automation).into(),
                    effective_threshold: account.automation.effective_threshold(policy.threshold),
                    exhausted: account.quota.as_ref().is_some_and(|q| {
                        reaches(q, account.automation.effective_threshold(policy.threshold))
                    }),
                    frees_at: account.quota.as_ref().and_then(|q| {
                        frees_at(q, account.automation.effective_threshold(policy.threshold))
                    }),
                    reset: CodexResetView {
                        pending: account.pending_reset_key.is_some(),
                        last_outcome: account.last_reset_outcome,
                        usable: self.reset_capability(account, now),
                    },
                }
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
            && !self.demo
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
        if !matches!(command, CodexCommand::Observe | CodexCommand::PollLogin(_)) {
            self.error = None;
        }
        match command {
            CodexCommand::Discover => {
                if let Err(error) = self.discover(vault, now).await {
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
                self.import_current(vault, now)?;
            }
            CodexCommand::ImportSwitcher => {
                self.import_switcher(vault, now)?;
            }
            CodexCommand::AddApiKey(name, key) => {
                self.add_api_key(&name, &key, vault, now)?;
            }
            CodexCommand::SetApiKeyFlex(id, enabled) => {
                self.set_api_key_flex(&id, enabled, vault, now)?;
            }
            CodexCommand::Delete(id) => {
                if self.pending_login.is_some() || self.switching.is_some() {
                    return Err(CodexReason::Busy);
                }
                let index = self.account_index(&id)?;
                if self.selected_id.as_ref() == Some(&id) {
                    // auth.json still holds it; Codex keeps using it until another switch.
                    return Err(CodexReason::ActiveAccount);
                }
                if self.saved.accounts[index].pending_reset_key.is_some() {
                    return Err(CodexReason::ResetUnconfirmed);
                }
                self.saved.accounts.remove(index);
                self.verified_now.remove(&id);
                self.account_errors.remove(&id);
                self.save(vault)?;
            }
            CodexCommand::UpdateAccountAutomation(id, view) => {
                if self.pending_login.is_some() || self.switching.is_some() {
                    return Err(CodexReason::Busy);
                }
                let automation = AccountAutomation::from(view);
                automation
                    .validate()
                    .map_err(|_| CodexReason::InvalidAutomation)?;
                let index = self.account_index(&id)?;
                self.saved.accounts[index].automation = automation;
                self.observe_return_window(index, now);
                self.save(vault)?;
            }
            CodexCommand::ResumeAutomaticReturns => {
                self.saved.return_hold_until = None;
                self.save(vault)?;
            }
            CodexCommand::Observe => {
                self.observe(vault, now)?;
            }
        }
        Ok(CodexOutput::Snapshot)
    }
    async fn discover(&mut self, vault: &Vault, now: i64) -> Result<(), CodexReason> {
        if self.pending_login.is_some() || self.switching.is_some() {
            return Err(CodexReason::Busy);
        }
        self.discovered_at = Some(now);
        self.installation = None;
        self.selected_id = None;
        self.observed_generation = None;
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
        let version = executable.version().to_owned();
        let qualified = match paths.qualify(&executable_path, workspace.path(), &version) {
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
            version,
            paths,
            workspace,
            qualified,
        });
        self.availability = CodexAvailability::Supported;
        self.reason = None;
        if let Ok(store) = self.store() {
            // The 0.2.0 candidate's two-step journal is obsolete; its tokens are saved.
            let _ = store.discard_legacy_journal(vault);
        }
        remove_stale_owned_homes();
        // A transient read of auth.json must not turn a working setup into "unsupported";
        // the background tick observes again.
        if let Err(error) = self.observe(vault, now) {
            self.error = Some(error);
        }
        Ok(())
    }
    fn refresh_environment(&mut self) {
        self.switcher_available = self.switcher.exists();
        let Some(home) = self.installation.as_ref().map(|i| i.paths.home.clone()) else {
            return;
        };
        let summary = self.processes.scan(&home);
        self.environment = CodexEnvironmentView {
            daemon_running: cfg!(any(windows, target_os = "linux", target_os = "macos"))
                .then_some(summary.daemon_running),
            other_clients: summary.other_clients,
            codex_switcher_running: summary.switcher_running,
        };
    }
    /// Follow the live login (see [`Self::observe_login`]) and then make config.toml agree
    /// with it: Flex is never left on for a login that is not a Flex key, and a failed
    /// restore is retried here on every tick.
    pub(crate) fn observe(&mut self, vault: &Vault, now: i64) -> Result<(), CodexReason> {
        let observed = self.observe_login(vault, now);
        // A Flex failure is reported through the snapshot's warnings, not as an error
        // of following the login.
        let _ = self.reconcile_flex(vault);
        observed
    }
    /// Track the live auth.json: adopt Codex's token rotations into the saved record,
    /// keep an external `codex login` as a new saved account, and repair a hybrid file.
    fn observe_login(&mut self, vault: &Vault, now: i64) -> Result<(), CodexReason> {
        if self.demo || !self.writable || self.switching.is_some() {
            return Ok(());
        }
        self.refresh_environment();
        let Ok(store) = self.store() else {
            return Ok(());
        };
        let current = store.read().map_err(store_reason)?;
        if self.observed_generation.as_ref() == Some(&current.generation) {
            return Ok(());
        }
        let previous = self.selected_id.clone();
        let observed_before = self.observed_generation.is_some();
        let Some(bytes) = current.auth_bytes() else {
            self.selected_id = None;
            self.observed_generation = Some(current.generation);
            if self.note_external_selection(previous.as_deref(), observed_before, now) {
                self.save_automation(vault)?;
            }
            return Ok(());
        };
        match OpaqueAuth::inspect(bytes) {
            Ok(inspection) if inspection.hybrid => {
                self.repair_hybrid(bytes, &current, &store, vault, now)?;
                if self.note_external_selection(previous.as_deref(), observed_before, now) {
                    self.save_automation(vault)?;
                }
                return Ok(());
            }
            Ok(inspection) if inspection.kind != AuthKind::Unsupported => {
                let auth = OpaqueAuth::parse(bytes.to_vec()).map_err(provider_reason)?;
                let index = self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
                self.selected_id = Some(self.saved.accounts[index].id.clone());
                self.note_external_selection(previous.as_deref(), observed_before, now);
                self.save(vault)?;
            }
            _ => {
                if let Some((key, chatgpt)) = split_key_with_tokens(bytes) {
                    // Codex is still in key mode; the late tokens belong to a saved account.
                    let index = self.adopt_key_with_tokens(&key, &chatgpt, now)?;
                    self.selected_id = Some(self.saved.accounts[index].id.clone());
                    self.note_external_selection(previous.as_deref(), observed_before, now);
                    self.save(vault)?;
                } else {
                    self.selected_id = None;
                    if self.note_external_selection(previous.as_deref(), observed_before, now) {
                        self.save_automation(vault)?;
                    }
                }
            }
        }
        self.observed_generation = Some(current.generation);
        Ok(())
    }
    fn note_external_selection(
        &mut self,
        previous: Option<&str>,
        observed: bool,
        now: i64,
    ) -> bool {
        if observed && previous != self.selected_id.as_deref() {
            self.saved.return_hold_until = Some(now.saturating_add(MANUAL_RETURN_HOLD));
            return true;
        }
        false
    }
    /// Save `auth` into the matching account (a rotation of the same login) or add it
    /// as a new account. Never drops a login PrimerSwitch has not seen before.
    fn adopt_or_import(
        &mut self,
        auth: OpaqueAuth,
        evidence: CodexIdentityEvidence,
        _now: i64,
    ) -> Result<usize, CodexReason> {
        let binding = Binding::from_auth(&auth)?;
        // The same key written by Codex or by this app differs only in formatting.
        let existing = self.saved.accounts.iter().position(|a| {
            a.auth_kind == auth.kind()
                && (a.binding.same_owner(&binding)
                    || a.auth == auth.as_bytes()
                    || (a.auth_kind == AuthKind::ApiKey && same_api_key(&a.auth, auth.as_bytes())))
        });
        if let Some(index) = existing {
            let account = &mut self.saved.accounts[index];
            if account.auth != auth.as_bytes() {
                account.set_auth(&auth)?;
                // A login that Codex just refreshed is valid again.
                account.needs_sign_in = false;
                account.auth_failures = 0;
                self.account_errors.remove(&account.id.clone());
            } else {
                account.binding = binding;
            }
            if evidence != CodexIdentityEvidence::ClaimsOnly {
                account.evidence = evidence;
            }
            account.managed_origin |= evidence == CodexIdentityEvidence::ManagedLogin;
            return Ok(index);
        }
        if self.saved.accounts.len() >= 1000 {
            return Err(CodexReason::StoreConflict);
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
            managed_origin: evidence == CodexIdentityEvidence::ManagedLogin,
            quota: None,
            quota_read_at: None,
            needs_sign_in: false,
            last_attempt_at: None,
            auth_failures: 0,
            pending_reset_key: None,
            last_reset_outcome: None,
            automation: AccountAutomation::default(),
            preferred_return_reset_at: None,
            preferred_return_handled_reset_at: None,
            use_flex: false,
        });
        Ok(self.saved.accounts.len() - 1)
    }
    /// A key file that gained ChatGPT tokens (see `split_key_with_tokens`): move the
    /// rotated tokens into the saved account of the same owner, or keep them as a new
    /// account, and adopt the key. Returns the key's account. The file is not rewritten;
    /// the next switch replaces it.
    fn adopt_key_with_tokens(
        &mut self,
        key: &[u8],
        chatgpt: &[u8],
        now: i64,
    ) -> Result<usize, CodexReason> {
        let (user, workspace) = OpaqueAuth::inspect(chatgpt)
            .map_err(provider_reason)?
            .claims
            .token_owner();
        let owner = Binding {
            user,
            workspace,
            email: None,
        };
        if let Some(account) = self
            .saved
            .accounts
            .iter_mut()
            .find(|a| a.auth_kind == AuthKind::ManagedChatgpt && a.binding.same_owner(&owner))
        {
            if let Ok(merged) = merge_rotated_tokens(&account.auth, chatgpt) {
                account.set_merged(merged)?;
                account.needs_sign_in = false;
            }
        } else {
            let auth = OpaqueAuth::parse(chatgpt.to_vec()).map_err(provider_reason)?;
            self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
        }
        let auth = OpaqueAuth::parse(key.to_vec()).map_err(provider_reason)?;
        self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)
    }
    /// A Codex process that still held another account refreshed after the file was
    /// switched: its `persist_tokens` merged that account's tokens into the current
    /// file. Move them to their owner and restore the account the file is labelled for.
    fn repair_hybrid(
        &mut self,
        bytes: &[u8],
        current: &CodexAuthSnapshot,
        store: &CodexFileStore,
        vault: &Vault,
        now: i64,
    ) -> Result<(), CodexReason> {
        let inspection = OpaqueAuth::inspect(bytes).map_err(provider_reason)?;
        let (user, workspace) = inspection.claims.token_owner();
        let owner = Binding {
            user,
            workspace,
            email: None,
        };
        if let Some(account) = self
            .saved
            .accounts
            .iter_mut()
            .find(|a| a.auth_kind == AuthKind::ManagedChatgpt && a.binding.same_owner(&owner))
        {
            if let Ok(merged) = merge_rotated_tokens(&account.auth, bytes) {
                account.set_merged(merged)?;
                account.needs_sign_in = false;
            }
        } else if let Ok(fixed) = normalize_hybrid(bytes) {
            let auth = OpaqueAuth::parse(fixed).map_err(provider_reason)?;
            self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
        }
        let label = inspection.claims.token_account_id.clone();
        let restore = self
            .selected_id
            .as_ref()
            .and_then(|id| self.saved.accounts.iter().find(|a| &a.id == id))
            .filter(|a| a.binding.workspace == label)
            .or_else(|| {
                self.saved.accounts.iter().find(|a| {
                    a.auth_kind == AuthKind::ManagedChatgpt
                        && label.is_some()
                        && a.binding.workspace == label
                })
            })
            .map(|a| (a.id.clone(), Zeroizing::new(a.auth.clone())));
        self.save(vault)?;
        match restore {
            Some((id, auth)) => {
                let written = store
                    .replace(&auth, &current.generation, vault)
                    .map_err(store_reason)?;
                self.selected_id = Some(id);
                self.observed_generation = Some(written.generation);
            }
            None => {
                self.selected_id = None;
                self.observed_generation = Some(current.generation.clone());
            }
        }
        Ok(())
    }
    async fn begin_login(&mut self, intent: u64) -> Result<CodexLoginLaunch, CodexReason> {
        self.ready()?;
        if self.pending_login.is_some() || self.switching.is_some() {
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
            .start(installation.executable.as_ref(), context, ClientRole::Login)
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
            Ok(binding)
        }
        .await;
        let shutdown = pending.client.shutdown().await.map_err(provider_reason);
        let complete = (|| {
            let initial_binding = result?;
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
            let index = self.adopt_or_import(auth, CodexIdentityEvidence::ManagedLogin, now)?;
            let account = &mut self.saved.accounts[index];
            account.needs_sign_in = false;
            let account_id = account.id.clone();
            self.account_errors.remove(&account_id);
            self.save(vault)?;
            self.last_login_account = Some(account_id);
            Ok(())
        })();
        scrub_owned_home(pending.home.path());
        drop(pending.home);
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
    fn import_current(&mut self, vault: &Vault, now: i64) -> Result<(), CodexReason> {
        if self.pending_login.is_some() || self.switching.is_some() {
            return Err(CodexReason::Busy);
        }
        self.ready()?;
        self.requalify()?;
        let store = self.store()?;
        let current = store.read().map_err(store_reason)?;
        let bytes = current.auth_bytes().ok_or(CodexReason::UnsupportedAuth)?;
        let inspection = OpaqueAuth::inspect(bytes).map_err(provider_reason)?;
        if inspection.hybrid {
            return self.repair_hybrid(bytes, &current, &store, vault, now);
        }
        let split = split_key_with_tokens(bytes);
        if inspection.kind == AuthKind::Unsupported && split.is_none() {
            return Err(CodexReason::UnsupportedAuth);
        }
        let index = match split {
            Some((key, chatgpt)) => self.adopt_key_with_tokens(&key, &chatgpt, now)?,
            None => {
                let auth = OpaqueAuth::parse(bytes.to_vec()).map_err(provider_reason)?;
                self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?
            }
        };
        self.save(vault)?;
        self.selected_id = Some(self.saved.accounts[index].id.clone());
        self.observed_generation = Some(current.generation);
        Ok(())
    }
    /// Save an OpenAI API key as a new account. It is never selected here; a switch
    /// writes it like any other saved login. The same key again changes nothing.
    fn add_api_key(
        &mut self,
        name: &str,
        key: &str,
        vault: &Vault,
        now: i64,
    ) -> Result<(), CodexReason> {
        if self.pending_login.is_some() || self.switching.is_some() {
            return Err(CodexReason::Busy);
        }
        self.ready()?;
        self.store()?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
            return Err(CodexReason::InvalidApiKey);
        }
        let key = validate_api_key(key).map_err(provider_reason)?;
        let payload = Zeroizing::new(api_key_auth_payload(&key).map_err(provider_reason)?);
        let auth = OpaqueAuth::parse(payload.to_vec()).map_err(provider_reason)?;
        let before = self.saved.accounts.len();
        let index = self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
        if self.saved.accounts.len() > before {
            self.saved.accounts[index].name = name.to_owned();
        }
        self.save(vault)
    }
    /// Turn Flex on or off for a saved API key. When the key is the active login, the
    /// change reaches config.toml at once; a failure to turn it on is reported and the
    /// setting stays off, a failure to turn it off keeps the restore pending (retried).
    fn set_api_key_flex(
        &mut self,
        id: &str,
        enabled: bool,
        vault: &Vault,
        now: i64,
    ) -> Result<(), CodexReason> {
        if self.pending_login.is_some() || self.switching.is_some() {
            return Err(CodexReason::Busy);
        }
        // The active login can have changed outside the app since the last tick: Flex is
        // decided on the live one, never on a stale selection. A failed observation does
        // not stop the toggle.
        let _ = self.observe_login(vault, now);
        let index = self.account_index(id)?;
        if self.saved.accounts[index].auth_kind != AuthKind::ApiKey {
            return Err(CodexReason::UnsupportedAuth);
        }
        if self.saved.accounts[index].use_flex == enabled {
            return Ok(());
        }
        if let Some(reason) = self.common_block() {
            return Err(reason);
        }
        if enabled {
            // The config is judged again before it is edited: a restricted layer blocks.
            self.requalify()?;
            if let Some(reason) = self.flex_block() {
                return Err(reason);
            }
        }
        self.saved.accounts[index].use_flex = enabled;
        if let Err(error) = self.reconcile_flex(vault) {
            if enabled {
                self.saved.accounts[index].use_flex = false;
                // The mark saved before the config write also carried the new setting.
                let _ = self.save(vault);
            } else {
                self.save(vault)?;
            }
            return Err(error);
        }
        self.save(vault)
    }
    /// Import every account saved by lampese "Codex Switcher". For an account that is
    /// already saved here, the copy whose id_token was issued later wins.
    fn import_switcher(&mut self, vault: &Vault, now: i64) -> Result<(), CodexReason> {
        if self.pending_login.is_some() || self.switching.is_some() {
            return Err(CodexReason::Busy);
        }
        self.ready()?;
        let bytes = self.switcher.read()?;
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| CodexReason::SwitcherUnavailable)?;
        let stamp = chrono::DateTime::<chrono::Utc>::from_timestamp(now, 0)
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut imported = 0usize;
        let result = (|| {
            let accounts = value["accounts"]
                .as_array()
                .ok_or(CodexReason::SwitcherUnavailable)?;
            for item in accounts.iter().take(500) {
                let data = &item["auth_data"];
                let text = |key: &str| data[key].as_str().filter(|v| !v.is_empty());
                let payload = match data["type"].as_str() {
                    Some("chat_g_p_t" | "chatgpt" | "chat_gpt") => {
                        match (
                            text("id_token"),
                            text("access_token"),
                            text("refresh_token"),
                        ) {
                            (Some(id), Some(access), Some(refresh)) => chatgpt_auth_payload(
                                id,
                                access,
                                refresh,
                                text("account_id"),
                                &stamp,
                            ),
                            _ => continue,
                        }
                    }
                    Some("api_key") => match text("key").map(str::trim).filter(|k| !k.is_empty()) {
                        Some(key) => api_key_auth_payload(key),
                        None => continue,
                    },
                    _ => continue,
                };
                let Ok(auth) = payload.and_then(OpaqueAuth::parse) else {
                    continue;
                };
                // A FedRAMP or otherwise unusable entry is skipped, never fatal.
                if self
                    .import_record(auth, item["name"].as_str(), now)
                    .unwrap_or(false)
                {
                    imported += 1;
                }
            }
            Ok(())
        })();
        wipe_json(&mut value);
        result?;
        if imported == 0 && self.saved.accounts.is_empty() {
            return Err(CodexReason::SwitcherUnavailable);
        }
        self.save(vault)
    }
    fn import_record(
        &mut self,
        auth: OpaqueAuth,
        name: Option<&str>,
        now: i64,
    ) -> Result<bool, CodexReason> {
        let binding = Binding::from_auth(&auth)?;
        let issued = auth.routing_claims().issued_at;
        if let Some(account) = self.saved.accounts.iter_mut().find(|a| {
            a.auth_kind == auth.kind()
                && (a.binding.same_owner(&binding)
                    || a.auth == auth.as_bytes()
                    || (a.auth_kind == AuthKind::ApiKey && same_api_key(&a.auth, auth.as_bytes())))
        }) {
            let saved_issued = OpaqueAuth::inspect(&account.auth)
                .ok()
                .and_then(|i| i.claims.issued_at);
            let is_active = self.selected_id.as_deref() == Some(&account.id);
            if !is_active && issued.is_some() && issued > saved_issued {
                account.set_auth(&auth)?;
                account.needs_sign_in = false;
                account.auth_failures = 0;
            }
            return Ok(false);
        }
        let index = self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
        if let Some(name) = name.and_then(safe_text).filter(|n| n.len() <= 120) {
            self.saved.accounts[index].name = name;
        }
        Ok(true)
    }

    /// Seconds between background readings of the active account: Claude's check
    /// interval, or every minute while its latest reading is within `NEAR_LIMIT_BAND`
    /// points of the threshold (or limited) and automatic switching is on.
    pub(crate) fn active_interval(&self, policy: &CodexPolicy) -> i64 {
        let near = self.selected().is_some_and(|a| {
            a.quota.as_ref().is_some_and(|q| {
                reaches(
                    q,
                    a.automation.effective_threshold(policy.threshold) - NEAR_LIMIT_BAND,
                )
            })
        });
        if policy.auto_switch && near {
            ACTIVE_NEAR_LIMIT_INTERVAL
        } else {
            policy.poll_interval
        }
    }
    /// Service known reset deadlines and overdue inactive readings fairly alongside
    /// the active account, which can be due on every scheduler tick near a limit.
    pub(crate) fn due_quota(&self, now: i64, policy: &CodexPolicy) -> Option<String> {
        if self.common_block().is_some() || self.switching.is_some() {
            return None;
        }
        let interval = self.active_interval(policy);
        let eligible = |a: &&SavedAccount| {
            a.auth_kind == AuthKind::ManagedChatgpt
                && (!a.needs_sign_in
                    || a.last_attempt_at
                        .is_none_or(|t| now < t || now - t >= SIGN_IN_RETRY))
        };
        // An opted-in preferred account gets the short recovery retry floor around
        // its observed reset. Other known resets use that floor too once successful
        // reads prove a newly elapsed window; failures retain their longer backoff.
        let preferred_due = if policy.auto_switch
            && self.pending_login.is_none()
            && self
                .saved
                .last_auto_switch_at
                .is_none_or(|at| now >= at && now.saturating_sub(at) >= AUTO_SWITCH_COOLDOWN)
            && self
                .saved
                .return_hold_until
                .is_none_or(|until| until <= now)
            && let Some(active) = self.selected()
            && let Some(target) = self
                .saved
                .accounts
                .iter()
                .filter(|a| {
                    a.auth_kind == AuthKind::ManagedChatgpt
                        && a.automation.enabled
                        && a.automation.return_on_five_hour_reset
                        && a.automation.is_higher_priority_than(&active.automation)
                        && self.switch_capability(a).enabled
                        && a.pending_reset_key.is_none()
                        && a.preferred_return_reset_at.is_some_and(|reset| {
                            reset <= now && a.quota_read_at.is_none_or(|read| read < reset)
                        })
                        && self.recovery_read_due(a, now)
                })
                .min_by_key(|a| (a.automation.priority, &a.id))
        {
            Some(target)
        } else {
            None
        };
        let active_due = self.selected().filter(eligible).filter(|active| {
            active
                .last_attempt_at
                .is_none_or(|t| now < t || now - t >= ERROR_RETRY.min(interval))
                && active
                    .quota_read_at
                    .is_none_or(|t| now < t || now - t >= interval)
        });
        let inactive_due = preferred_due.or_else(|| {
            self.saved
                .accounts
                .iter()
                .filter(|a| self.selected_id.as_ref() != Some(&a.id))
                .filter(eligible)
                .filter(|a| {
                    (if reset_passed(a, now) {
                        self.recovery_read_due(a, now)
                    } else {
                        a.last_attempt_at
                            .is_none_or(|t| now < t || now - t >= ERROR_RETRY)
                    }) && (a
                        .quota_read_at
                        .is_none_or(|t| now < t || now - t >= INACTIVE_QUOTA_INTERVAL)
                        || reset_passed(a, now))
                })
                .min_by_key(|a| {
                    (
                        !reset_passed(a, now),
                        a.quota_read_at.unwrap_or(i64::MIN),
                        &a.id,
                    )
                })
        });
        let next = match (active_due, inactive_due) {
            (Some(active), Some(inactive)) => {
                let urgent = preferred_due.is_some() || reset_passed(inactive, now);
                if self.last_background_active == Some(true)
                    || (self.last_background_active.is_none() && urgent)
                {
                    inactive
                } else {
                    active
                }
            }
            (Some(active), None) => active,
            (None, Some(inactive)) => inactive,
            (None, None) => return None,
        };
        Some(next.id.clone())
    }
    /// An API key has no plan limits: there is nothing to read, and not reading it is
    /// not a failure.
    pub(crate) fn reads_quota(&self, id: &str) -> bool {
        !self
            .saved
            .accounts
            .iter()
            .any(|a| a.id == id && a.auth_kind == AuthKind::ApiKey)
    }
    /// Prepare a quota reading. `manual` readings ignore back-off and token-expiry margins.
    pub(crate) fn plan_quota(
        &mut self,
        id: &str,
        now: i64,
        manual: bool,
    ) -> Result<QuotaJob, CodexReason> {
        self.ready()?;
        if self.switching.is_some() {
            return Err(CodexReason::SwitchInProgress);
        }
        let index = self.account_index(id)?;
        if self.saved.accounts[index].auth_kind != AuthKind::ManagedChatgpt {
            return Err(CodexReason::UnsupportedAuth);
        }
        if !manual {
            self.last_background_active = Some(self.selected_id.as_deref() == Some(id));
        }
        // System and managed layers apply to owned readers too.
        self.requalify()?;
        let binding = self.saved.accounts[index].binding.clone();
        let installation = self
            .installation
            .as_ref()
            .ok_or(CodexReason::NotInstalled)?;
        let executable = installation.executable.clone();
        let target = if self.selected_id.as_deref() == Some(id) {
            let store = self.store()?;
            let current = store.read().map_err(store_reason)?;
            let bytes = current.auth_bytes().ok_or(CodexReason::ExternalChange)?;
            let live = OpaqueAuth::parse(bytes.to_vec()).map_err(provider_reason)?;
            if !Binding::from_auth(&live)?.same_owner(&binding) {
                return Err(CodexReason::ExternalChange);
            }
            if !manual
                && live
                    .routing_claims()
                    .access_expires_at
                    .is_some_and(|exp| exp - now < ACTIVE_EXPIRY_MARGIN)
            {
                // Codex refreshes this token within the next minute; read afterwards,
                // and let the background turn move on to the other accounts meanwhile.
                self.saved.accounts[index].last_attempt_at = Some(now);
                return Err(CodexReason::Busy);
            }
            QuotaTarget::Active(
                CodexContext::approved_file(
                    store.context().home().to_owned(),
                    installation.workspace.path().to_owned(),
                )
                .map_err(provider_reason)?,
            )
        } else {
            let original = Zeroizing::new(self.saved.accounts[index].auth.clone());
            let home = owned_home()?;
            write_private_auth(home.path(), &original).map_err(store_reason)?;
            let context = CodexContext::owned(home.path().to_owned(), home.path().to_owned())
                .map_err(provider_reason)?;
            QuotaTarget::Owned {
                home,
                context,
                original,
            }
        };
        self.saved.accounts[index].last_attempt_at = Some(now);
        Ok(QuotaJob {
            account_id: id.into(),
            binding,
            executable,
            factory: self.factory.clone(),
            target,
        })
    }
    /// Preparation can fail before a reader exists (for example after policy
    /// changes). Persist its retry floor too, so a due return never retries every
    /// tick or immediately after restart.
    pub(crate) fn record_quota_preparation_failure(
        &mut self,
        id: &str,
        reason: CodexReason,
        vault: &Vault,
        now: i64,
    ) -> Result<(), CodexReason> {
        if matches!(reason, CodexReason::Busy | CodexReason::SwitchInProgress) {
            return Ok(());
        }
        if let Ok(index) = self.account_index(id) {
            self.saved.accounts[index].last_attempt_at = Some(now);
            self.account_errors.insert(id.into(), reason);
            self.verified_now.remove(id);
            self.save_automation(vault)?;
        }
        Ok(())
    }
    pub(crate) fn finish_quota(
        &mut self,
        report: QuotaReport,
        vault: &Vault,
        now: i64,
    ) -> Result<(), CodexReason> {
        let Ok(index) = self.account_index(&report.account_id) else {
            return Ok(());
        };
        if let Some(OwnedResult {
            original,
            rotated: Some(rotated),
        }) = &report.owned
            && rotated.as_slice() != original.as_slice()
            && self.saved.accounts[index].auth == original.as_slice()
            && let Ok(auth) = OpaqueAuth::parse(rotated.to_vec())
            && Binding::from_auth(&auth).is_ok_and(|b| b.same_owner(&report.binding))
        {
            self.saved.accounts[index].set_auth(&auth)?;
        }
        let id = report.account_id.clone();
        let mut reset_error = None;
        if let Some((key, outcome)) = &report.reset {
            let account = &mut self.saved.accounts[index];
            if account.pending_reset_key.as_ref() != Some(key)
                || !account.binding.same_owner(&report.binding)
            {
                return Err(CodexReason::ExternalChange);
            }
            match outcome {
                Ok(outcome) => {
                    account.last_reset_outcome = Some(*outcome);
                    account.pending_reset_key = None;
                    // A completed reset invalidates old percentages even if the
                    // required post-redemption read fails.
                    if matches!(outcome, ResetOutcome::Reset | ResetOutcome::AlreadyRedeemed) {
                        account.quota = None;
                        account.quota_read_at = None;
                        handle_return_marker(account);
                    }
                }
                Err(error) => {
                    reset_error = Some(if *error == CodexError::IdentityMismatch {
                        CodexReason::IdentityMismatch
                    } else {
                        CodexReason::ResetUnconfirmed
                    });
                }
            }
        }
        if !matches!(report.outcome, Err(CodexError::AuthRequired)) {
            self.saved.accounts[index].auth_failures = 0;
        }
        match report.outcome {
            Ok(limits)
                if limits.account_id.is_some() && limits.account_id != report.binding.workspace =>
            {
                self.verified_now.remove(&id);
                self.account_errors
                    .insert(id, CodexReason::IdentityMismatch);
            }
            Ok(limits) => match quota_view(&limits) {
                Ok(view) => {
                    // Only a reading that names this workspace proves whose usage it is.
                    let proven = limits.account_id.is_some();
                    let account = &mut self.saved.accounts[index];
                    account.quota = Some(view);
                    account.quota_read_at = Some(now);
                    account.needs_sign_in = false;
                    if proven {
                        account.evidence = CodexIdentityEvidence::BackendVerified;
                        self.verified_now.insert(id.clone());
                    }
                    self.account_errors.remove(&id);
                    if proven {
                        self.observe_return_window(index, now);
                    } else {
                        // An unlabelled successful reply cannot reuse older identity
                        // evidence to authorize a preferred return.
                        self.verified_now.remove(&id);
                    }
                }
                Err(error) => {
                    self.account_errors.insert(id, error);
                }
            },
            Err(CodexError::AuthRequired) => {
                // One 401 can be a transient refresh failure; two in a row mean the
                // saved sign-in is gone.
                let account = &mut self.saved.accounts[index];
                account.auth_failures = account.auth_failures.saturating_add(1);
                self.verified_now.remove(&id);
                if account.auth_failures >= 2 {
                    account.needs_sign_in = true;
                    self.account_errors.insert(id, CodexReason::SignInRequired);
                } else {
                    self.account_errors
                        .insert(id, CodexReason::ProviderUnavailable);
                }
            }
            Err(error) => {
                if error == CodexError::UnsupportedInstallation {
                    // Codex was updated or replaced: discover it again.
                    self.installation = None;
                    self.discovered_at = None;
                }
                self.account_errors.insert(id, provider_reason(error));
            }
        }
        if let Some(error) = reset_error {
            self.account_errors.insert(report.account_id, error);
            self.error = Some(error);
        }
        if let Err(error) = self.save(vault) {
            // The durable pending key may still be on disk. Never allow a new
            // redemption until reopening that record succeeds.
            self.writable = false;
            self.reason = Some(error);
            return Err(error);
        }
        // An active reading may have let Codex rotate auth.json; adopt it now.
        self.observe(vault, now)
    }

    /// First, locked half of a switch: keep the outgoing login, write the selected one.
    /// Returns None when the account is already active.
    pub(crate) fn begin_switch(
        &mut self,
        id: &str,
        vault: &Vault,
        now: i64,
    ) -> Result<Option<SwitchJob>, CodexReason> {
        self.begin_switch_guarded(id, vault, now, None)
    }
    pub(crate) fn begin_automatic_switch(
        &mut self,
        plan: &CodexAutoSwitch,
        returning: bool,
        policy: &CodexPolicy,
        vault: &Vault,
        now: i64,
    ) -> Result<Option<SwitchJob>, CodexReason> {
        self.begin_switch_guarded(&plan.target_id, vault, now, Some((plan, returning, policy)))
    }
    fn automatic_target_still_eligible(
        &self,
        plan: &CodexAutoSwitch,
        returning: bool,
        policy: &CodexPolicy,
        now: i64,
    ) -> bool {
        let Some(active) = self.selected().filter(|a| a.id == plan.previous_id) else {
            return false;
        };
        // Last line of defence: only a saved ChatGPT sign-in is an automatic target.
        if !policy.auto_switch
            || !self
                .saved
                .accounts
                .iter()
                .any(|a| a.id == plan.target_id && a.auth_kind == AuthKind::ManagedChatgpt)
        {
            return false;
        }
        if returning {
            matches!(self.automatic_return(active, now, policy), CodexAutomation::Return(current) if current.target_id == plan.target_id)
        } else {
            self.quota_state(active, now) == CodexQuotaState::Fresh
                && active.quota.as_ref().is_some_and(|quota| {
                    reaches(
                        quota,
                        active.automation.effective_threshold(policy.threshold),
                    )
                })
                && self.next_candidate(policy).is_some_and(|target| {
                    target.id == plan.target_id
                        && self.quota_state(target, now) == CodexQuotaState::Fresh
                })
        }
    }
    fn begin_switch_guarded(
        &mut self,
        id: &str,
        vault: &Vault,
        now: i64,
        automatic: Option<(&CodexAutoSwitch, bool, &CodexPolicy)>,
    ) -> Result<Option<SwitchJob>, CodexReason> {
        self.ready()?;
        if self.switching.is_some() {
            return Err(CodexReason::SwitchInProgress);
        }
        if self.pending_login.is_some() {
            return Err(CodexReason::Busy);
        }
        self.requalify()?;
        if let Some((plan, returning, policy)) = automatic {
            self.observe(vault, now)?;
            if !self.automatic_target_still_eligible(plan, returning, policy, now) {
                return Err(CodexReason::ExternalChange);
            }
        }
        let index = self.account_index(id)?;
        if self.selected_id.as_deref() == Some(id) {
            return Ok(None);
        }
        let capability = self.switch_capability(&self.saved.accounts[index]);
        if !capability.enabled {
            return Err(capability
                .blocked_reason
                .unwrap_or(CodexReason::IdentityUnverified));
        }
        let home = self
            .installation
            .as_ref()
            .ok_or(CodexReason::NotInstalled)?
            .paths
            .home
            .clone();
        // Codex refuses to restart its daemon from an elevated process: switching the
        // file alone would leave every open terminal on the previous account.
        if self.processes.elevated() && self.processes.scan(&home).daemon_running {
            return Err(CodexReason::DaemonRestartFailed);
        }
        if self.store().is_err() {
            if home.exists() {
                return Err(CodexReason::UnsupportedStore);
            }
            create_private_context(home).map_err(store_reason)?;
            self.requalify()?;
        }
        let store = self.store()?;
        let expected_owner = if automatic.is_some() {
            Some(
                self.selected()
                    .ok_or(CodexReason::ExternalChange)?
                    .binding
                    .clone(),
            )
        } else {
            None
        };
        let flex_target = {
            let target = &self.saved.accounts[index];
            target.auth_kind == AuthKind::ApiKey && target.use_flex
        };
        self.flex_notice = None;
        let mut attempts = 0;
        let written = (|| -> Result<CodexAuthSnapshot, CodexReason> {
            loop {
                attempts += 1;
                let current = store.read().map_err(store_reason)?;
                if let Some(expected) = &expected_owner {
                    let same_owner = current.auth_bytes().is_some_and(|bytes| {
                        OpaqueAuth::parse(bytes.to_vec()).is_ok_and(|auth| {
                            Binding::from_auth(&auth).is_ok_and(|owner| owner.same_owner(expected))
                        })
                    });
                    if !same_owner {
                        // Follow and preserve the external selection before returning;
                        // it must never become the outgoing login of this auto plan.
                        self.observe(vault, now)?;
                        self.hold_automatic_returns(now, vault)?;
                        return Err(CodexReason::ExternalChange);
                    }
                    if let Some((plan, returning, policy)) = automatic
                        && !self.automatic_target_still_eligible(plan, returning, policy, now)
                    {
                        return Err(CodexReason::ExternalChange);
                    }
                }
                if let Some(bytes) = current.auth_bytes() {
                    self.preserve_outgoing(bytes, now)?;
                }
                self.save(vault)?;
                let incoming =
                    Zeroizing::new(self.saved.accounts[self.account_index(id)?].auth.clone());
                #[cfg(test)]
                if automatic.is_some()
                    && let Some(writer) = self.before_automatic_replace.take()
                {
                    writer();
                }
                // Flex is settled before the login file is written: a Flex key is never
                // switched to without it, and Flex never stays on for another login.
                self.switch_flex(flex_target, vault)?;
                match store.replace(&incoming, &current.generation, vault) {
                    Ok(snapshot) => return Ok(snapshot),
                    // Codex refreshed the outgoing token meanwhile: keep it and try again.
                    Err(CodexStoreError::ExternalChange) if attempts < 4 => continue,
                    Err(error) => return Err(store_reason(error)),
                }
            }
        })();
        let written = match written {
            Ok(written) => written,
            Err(error) => {
                // The login did not change: config.toml must agree with it again.
                self.flex_notice = None;
                let _ = self.reconcile_flex(vault);
                return Err(error);
            }
        };
        self.selected_id = Some(id.into());
        self.observed_generation = Some(written.generation);
        self.last_switch = None;
        self.switching = Some(CodexSwitchView {
            target_id: id.into(),
            stage: CodexSwitchStage::Restarting,
            started_at: now,
        });
        let environment = self
            .installation
            .as_ref()
            .map(|i| self.processes.scan(&i.paths.home))
            .and_then(|summary| summary.daemon_pid)
            .and_then(|pid| self.processes.environment(pid));
        Ok(Some(SwitchJob {
            target_id: id.into(),
            executable: self
                .installation
                .as_ref()
                .and_then(|i| i.executable.clone()),
            daemon: self.daemon.clone(),
            environment,
        }))
    }
    /// Keep the login that is about to be replaced: a rotation of a saved account, an
    /// unknown login (saved as a new account), or a hybrid left by an old process.
    fn preserve_outgoing(&mut self, bytes: &[u8], now: i64) -> Result<(), CodexReason> {
        let inspection = OpaqueAuth::inspect(bytes).map_err(|_| CodexReason::UnsupportedAuth)?;
        if inspection.hybrid {
            let (user, workspace) = inspection.claims.token_owner();
            let owner = Binding {
                user,
                workspace,
                email: None,
            };
            if let Some(account) =
                self.saved.accounts.iter_mut().find(|a| {
                    a.auth_kind == AuthKind::ManagedChatgpt && a.binding.same_owner(&owner)
                })
            {
                if let Ok(merged) = merge_rotated_tokens(&account.auth, bytes) {
                    account.set_merged(merged)?;
                }
            } else {
                let fixed = normalize_hybrid(bytes).map_err(provider_reason)?;
                let auth = OpaqueAuth::parse(fixed).map_err(provider_reason)?;
                self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
            }
            return Ok(());
        }
        if inspection.kind == AuthKind::Unsupported {
            // A key with late ChatGPT tokens is kept; agent identity, personal access
            // token or Bedrock: never overwrite it.
            let (key, chatgpt) =
                split_key_with_tokens(bytes).ok_or(CodexReason::UnsupportedAuth)?;
            self.adopt_key_with_tokens(&key, &chatgpt, now)?;
            return Ok(());
        }
        let auth = OpaqueAuth::parse(bytes.to_vec()).map_err(provider_reason)?;
        self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
        Ok(())
    }
    pub(crate) fn set_switch_stage(&mut self, stage: CodexSwitchStage) {
        if let Some(switching) = &mut self.switching {
            switching.stage = stage;
        }
    }
    /// Last, locked half of a switch. Returns true when an old process rewrote the
    /// file during the drain and the daemon must be restarted once more.
    pub(crate) fn finish_switch(
        &mut self,
        job: &SwitchJob,
        outcome: &SwitchOutcome,
        vault: &Vault,
        now: i64,
        allow_repair: bool,
    ) -> Result<bool, CodexReason> {
        self.set_switch_stage(CodexSwitchStage::Verifying);
        let mut repaired = false;
        if let Ok(store) = self.store()
            && let Ok(current) = store.read()
            && Some(&current.generation) != self.observed_generation.as_ref()
        {
            match current.auth_bytes().map(OpaqueAuth::inspect) {
                Some(Ok(inspection)) if inspection.hybrid => {
                    let bytes = Zeroizing::new(current.auth_bytes().unwrap_or_default().to_vec());
                    self.switching = None;
                    self.repair_hybrid(&bytes, &current, &store, vault, now)?;
                    repaired = true;
                }
                Some(Ok(inspection)) if inspection.kind != AuthKind::Unsupported => {
                    // The new daemon refreshed the selected login (or someone ran
                    // `codex login`): keep it and follow it.
                    if let Some(bytes) = current.auth_bytes()
                        && let Ok(auth) = OpaqueAuth::parse(bytes.to_vec())
                    {
                        let previous = self.selected_id.clone();
                        let index =
                            self.adopt_or_import(auth, CodexIdentityEvidence::ClaimsOnly, now)?;
                        self.selected_id = Some(self.saved.accounts[index].id.clone());
                        self.note_external_selection(previous.as_deref(), true, now);
                    }
                    self.observed_generation = Some(current.generation);
                }
                _ => {
                    self.switching = None;
                    let previous = self.selected_id.take();
                    // A key that only gained late ChatGPT tokens stays active. Anything
                    // else is not the target: the switch did not stick (ExternalChange).
                    if let Some((key, chatgpt)) =
                        current.auth_bytes().and_then(split_key_with_tokens)
                    {
                        let index = self.adopt_key_with_tokens(&key, &chatgpt, now)?;
                        self.selected_id = Some(self.saved.accounts[index].id.clone());
                    }
                    self.note_external_selection(previous.as_deref(), true, now);
                    self.observed_generation = Some(current.generation);
                }
            }
        }
        if repaired && allow_repair && outcome.daemon_running {
            self.switching = Some(CodexSwitchView {
                target_id: job.target_id.clone(),
                stage: CodexSwitchStage::Restarting,
                started_at: now,
            });
            return Ok(true);
        }
        self.refresh_environment();
        let restarted = outcome.daemon_running && matches!(outcome.restart, Some(Ok(())));
        self.switching = None;
        // Flex follows the login the switch ended on, whoever changed it meanwhile.
        let _ = self.reconcile_flex(vault);
        self.last_switch = Some(CodexLastSwitchView {
            account_id: job.target_id.clone(),
            at: now,
            daemon_restarted: restarted,
            other_clients: self.environment.other_clients,
            error: matches!(outcome.restart, Some(Err(_)))
                .then_some(CodexReason::DaemonRestartFailed)
                .or_else(|| {
                    self.flex_restore_pending()
                        .then_some(CodexReason::FlexRestoreFailed)
                }),
            flex: self.flex_notice.take(),
        });
        if let Ok(index) = self.account_index(&job.target_id) {
            self.observe_return_window(index, now);
        }
        self.save(vault)?;
        if self.selected_id.as_deref() != Some(&job.target_id) {
            return Err(CodexReason::ExternalChange);
        }
        Ok(false)
    }
    /// A switch whose locked first half succeeded must never stay "in progress".
    pub(crate) fn abort_switch(&mut self) {
        self.switching = None;
        self.flex_notice = None;
    }
    fn observe_return_window(&mut self, index: usize, now: i64) {
        let active = self.selected_id.as_deref() == Some(&self.saved.accounts[index].id);
        let verified = self.verified_now.contains(&self.saved.accounts[index].id);
        let account = &mut self.saved.accounts[index];
        // Capturing history is separate from authorizing a switch. Keep an actual
        // attributed near-limit observation across restarts and preference changes; a
        // return still needs a fresh workspace-verified target read after reset.
        if active
            && verified
            && let Some(reset) = account.preferred_return_reset_at
            && natural_five_hour_recovery(account, reset, now)
        {
            handle_return_marker(account);
        }
        if account.auth_kind == AuthKind::ManagedChatgpt
            && account.binding.complete()
            && let (Some(quota), Some(read)) = (&account.quota, account.quota_read_at)
            && read <= now
            && let Some(reset) = main_windows(quota)
                .filter(|w| {
                    w.window_duration_mins == Some(300)
                        && f64::from(w.used_percent)
                            > account
                                .automation
                                .effective_threshold(self.policy_threshold)
                                - HEADROOM_MARGIN
                })
                .filter_map(|w| w.resets_at)
                .filter(|reset| *reset > read)
                .max()
            && account
                .preferred_return_handled_reset_at
                .is_none_or(|handled| handled < reset)
        {
            account.preferred_return_reset_at = Some(reset);
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_seams(
        mut self,
        factory: Arc<dyn ClientFactory>,
        daemon: Arc<dyn DaemonControl>,
        processes: Arc<dyn ProcessInventory>,
        switcher: Arc<dyn SwitcherSource>,
    ) -> Self {
        self.factory = factory;
        self.daemon = daemon;
        self.processes = processes;
        self.switcher = switcher;
        self
    }
}
fn natural_five_hour_recovery(account: &SavedAccount, reset: i64, now: i64) -> bool {
    reset <= now
        && account
            .quota_read_at
            .is_some_and(|read| read >= reset && read <= now)
        && account.quota.as_ref().is_some_and(|quota| {
            main_windows(quota).any(|window| {
                window.window_duration_mins == Some(300)
                    && window.used_percent < 100
                    && match window.resets_at {
                        Some(next) => next > now && next > reset,
                        None => window.used_percent == 0,
                    }
            })
        })
}
fn handle_return_marker(account: &mut SavedAccount) {
    if let Some(reset) = account.preferred_return_reset_at.take() {
        account.preferred_return_handled_reset_at = Some(
            account
                .preferred_return_handled_reset_at
                .map_or(reset, |handled| handled.max(reset)),
        );
    }
}
fn reset_passed(account: &SavedAccount, now: i64) -> bool {
    let (Some(quota), Some(read)) = (&account.quota, account.quota_read_at) else {
        return false;
    };
    quota.limits.iter().any(|limit| {
        [&limit.primary, &limit.secondary]
            .into_iter()
            .flatten()
            .any(|w| w.used_percent > 0 && w.resets_at.is_some_and(|r| r > read && r <= now))
    })
}
fn wipe_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => text.zeroize(),
        serde_json::Value::Array(items) => items.iter_mut().for_each(wipe_json),
        serde_json::Value::Object(items) => items.values_mut().for_each(wipe_json),
        _ => (),
    }
}
/// Recompute every binding from its login and set aside records whose login no longer
/// parses, has a duplicate id or an unsafe name.
fn sanitize(saved: Saved) -> (Saved, Saved) {
    let mut good = Saved::default();
    let mut bad = Saved::default();
    good.return_hold_until = saved.return_hold_until;
    good.last_auto_switch_at = saved.last_auto_switch_at;
    good.flex_applied = saved.flex_applied;
    let mut ids = BTreeSet::new();
    for mut account in saved.accounts.into_iter() {
        let auth = OpaqueAuth::parse(account.auth.clone()).ok();
        let usable = !account.id.is_empty()
            && account.id.len() <= 256
            && safe_text(&account.name).is_some()
            && !ids.contains(&account.id)
            && account.automation.validate().is_ok()
            && auth
                .as_ref()
                .is_some_and(|auth| account.set_auth(auth).is_ok());
        if usable {
            ids.insert(account.id.clone());
            good.accounts.push(account);
        } else {
            bad.accounts.push(account);
        }
    }
    (good, bad)
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
    let home = create_private_context(canonical).map_err(store_reason)?;
    // Codex clones its ~24 MB plugin marketplace into every new home at startup; an
    // owned sign-in or quota reader needs none of it.
    std::fs::write(home.join("config.toml"), b"[features]\nplugins = false\n")
        .map_err(|_| CodexReason::StoreConflict)?;
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
        CodexError::AuthRequired => CodexReason::SignInRequired,
        CodexError::DaemonUnavailable => CodexReason::DaemonRestartFailed,
        CodexError::InvalidApiKey => CodexReason::InvalidApiKey,
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
            // The default bucket is usually repeated under its own limit id.
            if Some(key.as_str()) == quota.rate_limits.limit_id.as_deref() {
                continue;
            }
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
        reset_credit_details: reset_expiry_details(quota),
    })
}

/// Dates only: never export opaque credit IDs, display strings or raw rows.
fn reset_expiry_details(quota: &CodexRateLimits) -> Option<Vec<CodexResetCreditView>> {
    let credits = quota.rate_limit_reset_credits.as_ref()?.credits.as_ref()?;
    let mut seen = BTreeSet::new();
    let mut details: Vec<_> = credits
        .iter()
        .take(256)
        .filter(|credit| {
            credit.status == "available"
                && credit.reset_type == "codexRateLimits"
                && !credit.id.is_empty()
                && credit
                    .expires_at
                    .is_none_or(|date| (0..=253_402_300_799).contains(&date))
                && seen.insert(&credit.id)
        })
        .map(|credit| CodexResetCreditView {
            expires_at: credit.expires_at,
        })
        .collect();
    details.sort_by_key(|credit| (credit.expires_at.is_none(), credit.expires_at));
    Some(details)
}

#[cfg(test)]
#[path = "codex_engine_tests.rs"]
mod tests;
