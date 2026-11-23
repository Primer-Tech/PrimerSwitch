//! Launching a saved Claude subscription in its own dedicated profile.
//!
//! A profile is an independent authorization of the same account, issued by its own
//! identity-bound browser sign-in. It never copies the vault's grant, so the profile's
//! Claude Code session and PrimerSwitch never refresh one token from two places, and it
//! never reads, writes or switches the global Claude login. The profile intent here is
//! separate from Add/Repair: it can only ever write that account's profile directory.
use super::*;

/// What one launch needs about its account, collected without borrowing the engine.
struct LaunchTarget {
    login: LoginTarget,
    profile: ClaudeProfile,
    launcher: Arc<dyn ClaudeLauncher>,
}

enum OwnerProof {
    /// Decided without a request: `None` is only produced by an owner check that passed.
    Settled(Option<ProfileAuthReason>),
    /// The profile's own still-valid access token, to be checked with the provider.
    Ask(ClaudeClient, zeroize::Zeroizing<String>),
}

/// Compares both the account and the workspace the provider reports for this token.
async fn ask_owner(
    client: ClaudeClient,
    token: zeroize::Zeroizing<String>,
    target: &LaunchTarget,
) -> Result<Option<ProfileAuthReason>, RuntimeError> {
    match client.profile(token.as_str()).await {
        Ok(profile) if target.login.matches(&profile.identity) => Ok(None),
        Ok(_) => Ok(Some(ProfileAuthReason::IdentityChanged)),
        Err(ClientError::Unauthorized | ClientError::InvalidGrant) => {
            Ok(Some(ProfileAuthReason::Expired))
        }
        Err(_) => Err(RuntimeError::Provider),
    }
}

impl Engine {
    fn launch_target(&self, id: &str) -> Result<LaunchTarget, RuntimeError> {
        let config = self
            .launch
            .as_ref()
            .ok_or(RuntimeError::LaunchUnsupported)?;
        let i = self.index(id)?;
        let account = &self.saved.accounts[i];
        if account.provider != ProviderId::Claude {
            return Err(RuntimeError::MissingAccount);
        }
        if self
            .saved
            .readings
            .get(id)
            .is_some_and(|reading| reading.sign_in_required)
        {
            return Err(RuntimeError::SignInRequired);
        }
        let account_uuid = account
            .account_uuid()
            .filter(|uuid| !uuid.is_empty())
            .ok_or(RuntimeError::Identity)?;
        let organization_uuid = account
            .organization_uuid()
            .filter(|uuid| !uuid.is_empty())
            .ok_or(RuntimeError::Identity)?;
        let common = self.active_store()?.paths().clone();
        let profile = ClaudeProfile::new(&config.profiles_dir, id, common)?;
        Ok(LaunchTarget {
            login: LoginTarget {
                id: id.into(),
                account_uuid: account_uuid.into(),
                organization_uuid: organization_uuid.into(),
                active_fingerprint: None,
            },
            profile,
            launcher: config.launcher.clone(),
        })
    }

    /// Read-only: nothing is created, authorized or requested.
    pub(super) fn precheck_launch(&self, id: &str) -> Result<(), RuntimeError> {
        let target = self.launch_target(id)?;
        target
            .launcher
            .executable()
            .map(|_| ())
            .ok_or(RuntimeError::LaunchNotInstalled)
    }

    pub(super) async fn launch_profile(
        &mut self,
        id: String,
        model: LaunchModel,
        folder: PathBuf,
    ) -> Result<LaunchOutcome, RuntimeError> {
        self.start_profile(id, model, folder, false).await
    }

    /// `owner_proven` is true right after the profile was authorized by a verified
    /// sign-in, so the credential is not asked about its owner a second time.
    async fn start_profile(
        &mut self,
        id: String,
        model: LaunchModel,
        folder: PathBuf,
        owner_proven: bool,
    ) -> Result<LaunchOutcome, RuntimeError> {
        let target = self.launch_target(&id)?;
        let folder = validate_launch_folder(&folder).map_err(|_| RuntimeError::LaunchFolder)?;
        let executable = target
            .launcher
            .executable()
            .ok_or(RuntimeError::LaunchNotInstalled)?;
        let state = target
            .profile
            .state(&target.login.account_uuid, &target.login.organization_uuid)?;
        let reason = match state {
            ProfileState::NeedsLogin => Some(ProfileAuthReason::FirstUse),
            ProfileState::IdentityChanged => Some(ProfileAuthReason::IdentityChanged),
            ProfileState::Ready if owner_proven => None,
            ProfileState::Ready => match self.owner_proof(&target)? {
                OwnerProof::Settled(reason) => reason,
                // Owned values only: the shared engine borrow is not held across the
                // provider request.
                OwnerProof::Ask(client, token) => ask_owner(client, token, &target).await?,
            },
        };
        if let Some(reason) = reason {
            // The folder stays in Rust while the user authorizes the profile.
            self.pending_launch = Some(PendingLaunch {
                account_id: id,
                model,
                folder,
            });
            return Ok(LaunchOutcome::AuthorizationRequired { reason });
        }
        let plan = target.profile.prepare_for_folder(
            &target.login.account_uuid,
            &target.login.organization_uuid,
            &folder,
        )?;
        // History and project memory must stay one store; never fork it silently.
        if plan.shared.splits_history() {
            return Err(RuntimeError::LaunchHistorySplit);
        }
        let spec = build_launch_spec(&executable, &folder, model, &plan).map_err(|e| match e {
            PlatformError::UnsafePath => RuntimeError::LaunchFolder,
            other => other.into(),
        })?;
        target
            .launcher
            .spawn(&spec)
            .map_err(|_| RuntimeError::LaunchFailed)?;
        self.pending_launch = None;
        Ok(LaunchOutcome::Launched)
    }

    /// The cached `oauthAccount` is only a label; the provider names the credential's
    /// real owner. A credential whose owner cannot be read right now is never launched:
    /// an expired access token cannot be checked, and PrimerSwitch never refreshes the
    /// profile's grant (that refresh belongs to the Claude Code session), so it asks for
    /// a fresh independent sign-in instead. The decision is made synchronously from
    /// owned values; only the provider request itself is awaited.
    fn owner_proof(&self, target: &LaunchTarget) -> Result<OwnerProof, RuntimeError> {
        let Some(credential) = target.profile.credential()? else {
            return Ok(OwnerProof::Settled(Some(ProfileAuthReason::FirstUse)));
        };
        if !credential.is_current(self.clock.now()) {
            return Ok(OwnerProof::Settled(Some(ProfileAuthReason::Expired)));
        }
        Ok(OwnerProof::Ask(self.client()?, credential.access_token))
    }

    pub(super) fn begin_profile_login(
        &mut self,
        epoch: u64,
        id: &str,
    ) -> Result<LoginSession, RuntimeError> {
        if epoch != self.profile_intent.load(Ordering::Acquire) {
            return Err(RuntimeError::Login);
        }
        let target = self.launch_target(id)?;
        let email = self.saved.accounts[self.index(id)?]
            .email()
            .map(str::to_owned);
        let login = PendingLogin::new(self.clock.now())
            .with_account_hint(&target.login.organization_uuid, email.as_deref());
        let session = LoginSession {
            id: login.id.clone(),
            url: login.url.clone(),
        };
        // A new profile sign-in replaces an earlier one; ordinary Add/Repair sessions
        // are neither touched nor usable here.
        self.profile_login = Some(ProfileLogin {
            login,
            target: target.login,
        });
        Ok(session)
    }

    pub(super) async fn finish_profile_login(
        &mut self,
        id: &str,
        code: &str,
        epoch: u64,
    ) -> Result<LaunchOutcome, RuntimeError> {
        if self
            .profile_login
            .as_ref()
            .is_none_or(|pending| pending.login.id != id)
            || epoch != self.profile_intent.load(Ordering::Acquire)
        {
            return Err(RuntimeError::Login);
        }
        let ProfileLogin { login, target } =
            self.profile_login.take().ok_or(RuntimeError::Login)?;
        let result = self
            .client()?
            .finish_login(&login, code, self.clock.now())
            .await
            .map_err(|_| RuntimeError::Login)?;
        if epoch != self.profile_intent.load(Ordering::Acquire) {
            return Err(RuntimeError::Login);
        }
        // Both the account and the workspace must be the saved account's. Nothing is
        // written for any other login, and the vault and global login are never involved.
        let i = self.index(&target.id)?;
        if !target.matches(&result.profile.identity)
            || !target.matches(&self.saved.accounts[i].oauth_account)
        {
            return Err(RuntimeError::LoginIdentityMismatch);
        }
        let launch = self.launch_target(&target.id)?;
        launch
            .profile
            .authorize(&result.credentials, &result.profile.identity)?;
        match self.pending_launch.take() {
            Some(waiting) if waiting.account_id == target.id => {
                self.start_profile(waiting.account_id, waiting.model, waiting.folder, true)
                    .await
            }
            other => {
                self.pending_launch = other;
                Ok(LaunchOutcome::Authorized)
            }
        }
    }

    /// An empty id drops only a launch that was waiting for authorization.
    pub(super) fn cancel_profile_login(&mut self, id: &str) {
        let matched = self
            .profile_login
            .as_ref()
            .is_some_and(|pending| pending.login.id == id);
        if matched {
            self.profile_login = None;
        }
        if matched || id.is_empty() {
            self.pending_launch = None;
        }
    }

    /// Deleting an account removes its pending intents and its profile. A profile that
    /// cannot be removed stops the deletion, so credentials never outlive the account.
    pub(super) fn forget_launch_profile(&mut self, id: &str) -> Result<(), RuntimeError> {
        if self
            .profile_login
            .as_ref()
            .is_some_and(|pending| pending.target.id == id)
        {
            self.profile_login = None;
        }
        if self
            .pending_launch
            .as_ref()
            .is_some_and(|waiting| waiting.account_id == id)
        {
            self.pending_launch = None;
        }
        if let (Some(config), Some(active)) = (&self.launch, &self.active)
            && let Ok(profile) =
                ClaudeProfile::new(&config.profiles_dir, id, active.paths().clone())
        {
            profile.remove()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use provider_claude::{HttpRequest, HttpResponse, PROFILE_URL, TOKEN_URL, Transport};
    use std::{
        collections::VecDeque,
        sync::{Mutex as StdMutex, atomic::AtomicBool},
    };
    use switcher_platform::LaunchSpec;

    const NOW: i64 = 1_000_000;
    const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> i64 {
            NOW
        }
    }

    type Reply = (&'static str, Result<HttpResponse, ClientError>);
    #[derive(Default)]
    struct Fake {
        calls: StdMutex<Vec<HttpRequest>>,
        replies: StdMutex<VecDeque<Reply>>,
        /// Holds the next code exchange: (started, resume).
        gate: StdMutex<Option<(Arc<Notify>, Arc<Notify>)>>,
    }
    #[async_trait]
    impl Transport for Fake {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, ClientError> {
            let url = request.url.clone();
            self.calls.lock().unwrap().push(request);
            let gate = if url == TOKEN_URL {
                self.gate.lock().unwrap().take()
            } else {
                None
            };
            if let Some((started, resume)) = gate {
                started.notify_one();
                resume.notified().await;
            }
            let mut replies = self.replies.lock().unwrap();
            let expected = replies.front().is_some_and(|(target, _)| *target == url);
            if expected {
                replies.pop_front().unwrap().1
            } else {
                Err(ClientError::Transport)
            }
        }
    }
    fn ok(body: Value) -> Result<HttpResponse, ClientError> {
        Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body,
        })
    }
    fn tokens(marker: &str) -> Reply {
        (
            TOKEN_URL,
            ok(json!({
                "access_token": format!("profile-access-{marker}"),
                "refresh_token": format!("profile-refresh-{marker}"),
                "expires_in": 3600
            })),
        )
    }
    fn owner(account: &str, organization: &str) -> Reply {
        (
            PROFILE_URL,
            ok(json!({
                "account": {"uuid": account, "email": "owner@example.invalid"},
                "organization": {"uuid": organization}
            })),
        )
    }

    struct FakeLauncher {
        executable: StdMutex<Option<PathBuf>>,
        spawned: StdMutex<Vec<LaunchSpec>>,
        fail: AtomicBool,
    }
    impl ClaudeLauncher for FakeLauncher {
        fn executable(&self) -> Option<PathBuf> {
            self.executable.lock().unwrap().clone()
        }
        fn spawn(&self, spec: &LaunchSpec) -> std::io::Result<()> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("refused"));
            }
            self.spawned.lock().unwrap().push(spec.clone());
            Ok(())
        }
    }

    struct Env {
        temp: tempfile::TempDir,
        handle: RuntimeHandle,
        fake: Arc<Fake>,
        launcher: Arc<FakeLauncher>,
        paths: CliPaths,
        profiles: PathBuf,
        folder: PathBuf,
    }
    fn account(id: &str, owner: &str) -> StoredAccount {
        StoredAccount {
            id: id.into(),
            name: format!("Cont {owner}"),
            oauth_account: json!({
                "accountUuid": owner,
                "organizationUuid": format!("org-{owner}"),
                "emailAddress": format!("{owner}@example.invalid")
            }),
            credentials: json!({"claudeAiOauth": {
                "accessToken": format!("vault-access-{owner}"),
                "refreshToken": format!("vault-refresh-{owner}"),
                "expiresAt": (NOW + 3600) * 1000
            }}),
            last_usage_at: Some(NOW),
            identity_verified: true,
            ..StoredAccount::default()
        }
    }
    fn environment(configure: bool, sign_in_required: bool) -> Env {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let temp = tempfile::Builder::new()
            .prefix("primerswitch-launch-")
            .tempdir_in(parent)
            .unwrap();
        let paths = CliPaths::for_home(temp.path().join("home"));
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(&paths.settings_file, b"{}").unwrap();
        let (a, b) = (account(A, "a"), account(B, "b"));
        let mut global = a.credentials.clone();
        global["mcpOAuth"] = json!({"keep": 1});
        std::fs::write(
            &paths.credentials_file,
            serde_json::to_vec(&global).unwrap(),
        )
        .unwrap();
        std::fs::write(
            &paths.global_config_file,
            serde_json::to_vec(&json!({
                "oauthAccount": a.oauth_account,
                "mcpServers": {"docs": {"command": "docs"}},
                "keep": true
            }))
            .unwrap(),
        )
        .unwrap();
        let vault = Vault::with_key(temp.path().join("vault"), [7; 32]).unwrap();
        let mut readings = BTreeMap::new();
        for id in [A, B] {
            readings.insert(id.to_owned(), Reading::default());
        }
        readings.get_mut(B).unwrap().sign_in_required = sign_in_required;
        let saved = Persisted {
            accounts: vec![a, b],
            readings,
            settings: Settings {
                auto_switch_enabled: false,
                auto_start_window_enabled: false,
                ..Settings::default()
            },
            ..Persisted::default()
        };
        vault.save(RECORD, &saved).unwrap();
        let fake = Arc::new(Fake::default());
        let client = ClaudeClient::with_transport("2.1.287", fake.clone()).unwrap();
        let handle = RuntimeHandle::from_parts(
            vault,
            ActiveStore::file(paths.clone()),
            client,
            Arc::new(FixedClock),
        )
        .unwrap();
        let launcher = Arc::new(FakeLauncher {
            executable: StdMutex::new(Some(temp.path().join("claude.exe"))),
            spawned: StdMutex::new(vec![]),
            fail: AtomicBool::new(false),
        });
        let profiles = temp.path().join("data").join("claude-profiles");
        if configure {
            handle.configure_claude_launch(profiles.clone(), launcher.clone());
        }
        let folder = temp.path().join("work space");
        std::fs::create_dir_all(&folder).unwrap();
        Env {
            temp,
            handle,
            fake,
            launcher,
            paths,
            profiles,
            folder,
        }
    }
    impl Env {
        async fn launch(&self) -> Result<LaunchOutcome, RuntimeError> {
            self.handle
                .launch_claude(B, LaunchModel::Opus, self.folder.clone())
                .await
        }
        fn spawned(&self) -> Vec<LaunchSpec> {
            self.launcher.spawned.lock().unwrap().clone()
        }
        fn calls(&self, url: &str) -> usize {
            self.fake
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call.url == url)
                .count()
        }
        fn global(&self) -> (Vec<u8>, Vec<u8>) {
            (
                std::fs::read(&self.paths.credentials_file).unwrap(),
                std::fs::read(&self.paths.global_config_file).unwrap(),
            )
        }
        fn saved(&self) -> Persisted {
            Vault::with_key(self.temp.path().join("vault"), [7; 32])
                .unwrap()
                .load::<Persisted>(RECORD)
                .unwrap()
                .unwrap()
        }
        fn push(&self, replies: impl IntoIterator<Item = Reply>) {
            self.fake.replies.lock().unwrap().extend(replies);
        }
        /// An independent profile grant, as a previous sign-in would have left it.
        fn authorize_profile(&self, account: &str, organization: &str, expires_at_ms: i64) {
            ClaudeProfile::new(&self.profiles, B, self.paths.clone())
                .unwrap()
                .authorize(
                    &json!({"claudeAiOauth": {
                        "accessToken": "profile-access-one",
                        "refreshToken": "profile-refresh-one",
                        "expiresAt": expires_at_ms
                    }}),
                    &json!({"accountUuid": account, "organizationUuid": organization}),
                )
                .unwrap();
        }
        async fn sign_in(
            &self,
            replies: impl IntoIterator<Item = Reply>,
        ) -> Result<LaunchOutcome, RuntimeError> {
            let session = self.handle.begin_claude_profile_login(B).await.unwrap();
            self.push(replies);
            self.handle
                .finish_claude_profile_login(&session.id, &code_for(&session))
                .await
        }
    }
    fn code_for(session: &LoginSession) -> String {
        format!(
            "fixture#{}",
            session
                .url
                .split("state=")
                .nth(1)
                .unwrap()
                .split('&')
                .next()
                .unwrap()
        )
    }
    const FRESH: i64 = (NOW + 3600) * 1000;
    const EXPIRED: i64 = (NOW - 10) * 1000;

    #[tokio::test]
    async fn launching_is_disabled_unless_configured_and_for_accounts_that_cannot_launch() {
        let env = environment(false, false);
        assert!(!env.handle.get_snapshot().launch_supported);
        assert_eq!(
            env.handle.precheck_claude_launch(B).await.unwrap_err(),
            RuntimeError::LaunchUnsupported
        );
        assert_eq!(
            env.launch().await.unwrap_err(),
            RuntimeError::LaunchUnsupported
        );
        assert_eq!(
            env.handle.begin_claude_profile_login(B).await.unwrap_err(),
            RuntimeError::LaunchUnsupported
        );
        env.handle
            .configure_claude_launch(env.profiles.clone(), env.launcher.clone());
        assert!(env.handle.get_snapshot().launch_supported);
        env.handle.precheck_claude_launch(B).await.unwrap();
        assert_eq!(
            env.handle
                .precheck_claude_launch("missing")
                .await
                .unwrap_err(),
            RuntimeError::MissingAccount
        );
        *env.launcher.executable.lock().unwrap() = None;
        assert_eq!(
            env.handle.precheck_claude_launch(B).await.unwrap_err(),
            RuntimeError::LaunchNotInstalled
        );
        assert_eq!(
            env.launch().await.unwrap_err(),
            RuntimeError::LaunchNotInstalled
        );
        assert!(!env.profiles.exists() && env.fake.calls.lock().unwrap().is_empty());
        // A saved sign-in that needs repair is not launched under a guessed identity.
        let signed_out = environment(true, true);
        for result in [
            signed_out
                .handle
                .precheck_claude_launch(B)
                .await
                .unwrap_err(),
            signed_out.launch().await.unwrap_err(),
            signed_out
                .handle
                .begin_claude_profile_login(B)
                .await
                .unwrap_err(),
        ] {
            assert_eq!(result, RuntimeError::SignInRequired);
        }
        // Demo and degraded windows are read-only.
        let demo = RuntimeHandle::demo();
        assert!(!demo.get_snapshot().launch_supported);
        assert_eq!(
            demo.launch_claude("demo-0", LaunchModel::Default, env.folder.clone())
                .await
                .unwrap_err(),
            RuntimeError::ReadOnly
        );
        assert_eq!(
            demo.begin_claude_profile_login("demo-0").await.unwrap_err(),
            RuntimeError::ReadOnly
        );
    }

    #[tokio::test]
    async fn first_use_authorizes_only_the_profile_then_launches_the_selected_identity() {
        let env = environment(true, false);
        let global_before = env.global();
        let vault_before = env.saved().accounts[1].credentials.clone();
        let saved_before = serde_json::to_value(&env.saved().settings).unwrap();
        assert_eq!(
            env.launch().await.unwrap(),
            LaunchOutcome::AuthorizationRequired {
                reason: ProfileAuthReason::FirstUse
            }
        );
        assert!(env.spawned().is_empty() && env.fake.calls.lock().unwrap().is_empty());
        let session = env.handle.begin_claude_profile_login(B).await.unwrap();
        assert!(session.url.contains("orgUUID=org-b"));
        env.push([tokens("b"), owner("b", "org-b")]);
        let outcome = env
            .handle
            .finish_claude_profile_login(&session.id, &code_for(&session))
            .await
            .unwrap();
        assert_eq!(outcome, LaunchOutcome::Launched);
        let profile_dir = env.profiles.join(B);
        let spawned = env.spawned();
        assert_eq!(spawned.len(), 1);
        let spec = &spawned[0];
        assert_eq!(spec.working_dir, env.folder);
        assert_eq!(
            spec.env_set,
            vec![(
                std::ffi::OsString::from("CLAUDE_CONFIG_DIR"),
                profile_dir.clone().into_os_string()
            )]
        );
        assert_eq!(
            spec.args,
            vec![
                std::ffi::OsString::from("--settings"),
                env.paths.settings_file.clone().into_os_string(),
                std::ffi::OsString::from("--mcp-config"),
                profile_dir.join("shared-mcp.json").into_os_string(),
                std::ffi::OsString::from("--model"),
                std::ffi::OsString::from("opus"),
            ]
        );
        let printed = format!("{spec:?}");
        assert!(!printed.contains("profile-access") && !printed.contains("vault-"));
        // One authorization-code exchange and one owner read; nothing refreshed.
        assert_eq!(env.calls(TOKEN_URL), 1);
        assert_eq!(env.calls(PROFILE_URL), 1);
        let exchange = env.fake.calls.lock().unwrap()[0].body.clone().unwrap();
        assert_eq!(exchange["grant_type"], "authorization_code");
        // The profile holds its own grant; the vault and the global login are untouched.
        let grant = std::fs::read_to_string(profile_dir.join(".credentials.json")).unwrap();
        assert!(grant.contains("profile-refresh-b") && !grant.contains("vault-refresh-b"));
        assert_eq!(env.saved().accounts[1].credentials, vault_before);
        assert_eq!(env.global(), global_before);
        let snapshot = env.handle.get_snapshot();
        assert_eq!(snapshot.active_id.as_deref(), Some(A));
        assert_eq!(snapshot.return_hold_until, None);
        assert_eq!(
            serde_json::to_value(&env.saved().settings).unwrap(),
            saved_before
        );
        // Shared history is the real directory, not a copy.
        assert!(env.paths.config_dir.join("projects").is_dir());
        // A second launch needs no browser: the owner is proven with the profile's own
        // token, which is never exchanged or rotated.
        env.push([owner("b", "org-b")]);
        assert_eq!(env.launch().await.unwrap(), LaunchOutcome::Launched);
        assert_eq!(env.spawned().len(), 2);
        assert_eq!(env.calls(TOKEN_URL), 1);
        let owner_read =
            env.fake.calls.lock().unwrap().last().unwrap().headers["authorization"].clone();
        assert_eq!(owner_read, "Bearer profile-access-b");
    }

    #[tokio::test]
    async fn a_wrong_login_writes_nothing_and_the_waiting_folder_survives_a_retry() {
        for (account, organization) in [("a", "org-b"), ("b", "org-other"), ("x", "org-x")] {
            let env = environment(true, false);
            let global_before = env.global();
            let saved_before = serde_json::to_value(env.saved().accounts).unwrap();
            env.launch().await.unwrap();
            let error = env
                .sign_in([tokens("evil"), owner(account, organization)])
                .await
                .unwrap_err();
            assert_eq!(error, RuntimeError::LoginIdentityMismatch);
            assert!(!env.profiles.join(B).exists() && env.spawned().is_empty());
            assert_eq!(env.global(), global_before);
            assert_eq!(
                serde_json::to_value(env.saved().accounts).unwrap(),
                saved_before
            );
            // The next correct sign-in completes the same launch.
            assert_eq!(
                env.sign_in([tokens("b"), owner("b", "org-b")])
                    .await
                    .unwrap(),
                LaunchOutcome::Launched
            );
            assert_eq!(env.spawned()[0].working_dir, env.folder);
        }
    }

    #[tokio::test]
    async fn an_existing_profile_for_another_account_is_never_launched_or_silently_replaced() {
        for (account, organization) in [("a", "org-a"), ("b", "org-a"), ("a", "org-b")] {
            let env = environment(true, false);
            env.authorize_profile(account, organization, FRESH);
            let before = std::fs::read(env.profiles.join(B).join(".claude.json")).unwrap();
            assert_eq!(
                env.launch().await.unwrap(),
                LaunchOutcome::AuthorizationRequired {
                    reason: ProfileAuthReason::IdentityChanged
                }
            );
            assert!(env.spawned().is_empty() && env.fake.calls.lock().unwrap().is_empty());
            assert_eq!(
                std::fs::read(env.profiles.join(B).join(".claude.json")).unwrap(),
                before
            );
        }
        // An explicit new authorization replaces the profile's identity.
        let env = environment(true, false);
        env.authorize_profile("a", "org-a", FRESH);
        env.launch().await.unwrap();
        env.sign_in([tokens("b"), owner("b", "org-b")])
            .await
            .unwrap();
        let config = std::fs::read_to_string(env.profiles.join(B).join(".claude.json")).unwrap();
        assert!(config.contains("\"b\"") && !config.contains("org-a"));
    }

    #[tokio::test]
    async fn the_profile_credential_owner_is_proven_with_the_provider_and_never_refreshed() {
        let env = environment(true, false);
        env.authorize_profile("b", "org-b", FRESH);
        // The cached identity matches, but the credential belongs to someone else.
        for (account, organization) in [("someone-else", "org-b"), ("b", "other-org")] {
            env.push([owner(account, organization)]);
            assert_eq!(
                env.launch().await.unwrap(),
                LaunchOutcome::AuthorizationRequired {
                    reason: ProfileAuthReason::IdentityChanged
                }
            );
        }
        env.push([(PROFILE_URL, Err(ClientError::Unauthorized))]);
        assert_eq!(
            env.launch().await.unwrap(),
            LaunchOutcome::AuthorizationRequired {
                reason: ProfileAuthReason::Expired
            }
        );
        // An unverifiable owner is a visible failure, not a guess.
        env.push([(PROFILE_URL, Err(ClientError::Transport))]);
        assert_eq!(env.launch().await.unwrap_err(), RuntimeError::Provider);
        assert!(env.spawned().is_empty());
        env.push([owner("b", "org-b")]);
        assert_eq!(env.launch().await.unwrap(), LaunchOutcome::Launched);
        assert_eq!(env.calls(PROFILE_URL), 5);
        assert_eq!(env.calls(TOKEN_URL), 0);
        let first = env.fake.calls.lock().unwrap()[0].headers["authorization"].clone();
        assert_eq!(first, "Bearer profile-access-one");
    }

    #[tokio::test]
    async fn an_expired_profile_credential_is_never_trusted_or_refreshed() {
        // The cached identity names the right account, but an expired access token
        // cannot prove who owns the credential, and PrimerSwitch must not refresh the
        // profile's grant. A fresh independent sign-in is required instead.
        let env = environment(true, false);
        env.authorize_profile("b", "org-b", EXPIRED);
        let before = std::fs::read(env.profiles.join(B).join(".credentials.json")).unwrap();
        assert_eq!(
            env.launch().await.unwrap(),
            LaunchOutcome::AuthorizationRequired {
                reason: ProfileAuthReason::Expired
            }
        );
        assert!(env.spawned().is_empty() && env.fake.calls.lock().unwrap().is_empty());
        assert_eq!(
            std::fs::read(env.profiles.join(B).join(".credentials.json")).unwrap(),
            before
        );
        // That sign-in is explicit, identity-bound and completes the same launch.
        env.sign_in([tokens("b"), owner("b", "org-b")])
            .await
            .unwrap();
        assert_eq!(env.spawned().len(), 1);
        assert_eq!(env.calls(TOKEN_URL), 1);
        // The renewed profile grant is the new one; the vault grant was never used.
        let grant =
            std::fs::read_to_string(env.profiles.join(B).join(".credentials.json")).unwrap();
        assert!(grant.contains("profile-refresh-b") && !grant.contains("profile-refresh-one"));
        assert_eq!(
            env.saved().accounts[1].credentials,
            account(B, "b").credentials
        );
    }

    #[tokio::test]
    async fn cancelling_and_deleting_never_leave_a_usable_intent_or_credentials() {
        let env = environment(true, false);
        env.launch().await.unwrap();
        let session = env.handle.begin_claude_profile_login(B).await.unwrap();
        env.handle
            .cancel_claude_profile_login(&session.id)
            .await
            .unwrap();
        env.handle
            .cancel_claude_profile_login(&session.id)
            .await
            .unwrap();
        env.push([tokens("b"), owner("b", "org-b")]);
        assert_eq!(
            env.handle
                .finish_claude_profile_login(&session.id, &code_for(&session))
                .await
                .unwrap_err(),
            RuntimeError::Login
        );
        assert!(env.fake.calls.lock().unwrap().is_empty() && !env.profiles.join(B).exists());
        env.fake.replies.lock().unwrap().clear();
        // Dropping the waiting launch (no sign-in session yet) means no launch later.
        env.launch().await.unwrap();
        env.handle.cancel_claude_profile_login("").await.unwrap();
        env.push([tokens("b"), owner("b", "org-b")]);
        let second = env.handle.begin_claude_profile_login(B).await.unwrap();
        let outcome = env
            .handle
            .finish_claude_profile_login(&second.id, &code_for(&second))
            .await
            .unwrap();
        assert_eq!(outcome, LaunchOutcome::Authorized);
        assert!(env.spawned().is_empty() && env.profiles.join(B).exists());
        // Deleting the account removes its profile but never the shared history.
        std::fs::create_dir_all(env.paths.config_dir.join("projects/p")).unwrap();
        std::fs::write(
            env.paths.config_dir.join("projects/p/history.jsonl"),
            b"keep",
        )
        .unwrap();
        env.push([owner("b", "org-b")]);
        assert_eq!(env.launch().await.unwrap(), LaunchOutcome::Launched);
        env.handle.delete_account(B).await.unwrap();
        assert!(!env.profiles.join(B).exists());
        assert_eq!(
            std::fs::read(env.paths.config_dir.join("projects/p/history.jsonl")).unwrap(),
            b"keep"
        );
        assert_eq!(env.saved().accounts.len(), 1);
        // A pending sign-in for a deleted account cannot finish.
        let other = environment(true, false);
        other.launch().await.unwrap();
        let pending = other.handle.begin_claude_profile_login(B).await.unwrap();
        other.handle.delete_account(B).await.unwrap();
        assert_eq!(
            other
                .handle
                .finish_claude_profile_login(&pending.id, &code_for(&pending))
                .await
                .unwrap_err(),
            RuntimeError::Login
        );
    }

    #[tokio::test]
    async fn cancelling_during_the_code_exchange_authorizes_nothing() {
        let env = environment(true, false);
        env.launch().await.unwrap();
        let session = env.handle.begin_claude_profile_login(B).await.unwrap();
        env.push([tokens("b"), owner("b", "org-b")]);
        let (started, resume) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        *env.fake.gate.lock().unwrap() = Some((started.clone(), resume.clone()));
        let task = {
            let handle = env.handle.clone();
            let (id, code) = (session.id.clone(), code_for(&session));
            tokio::spawn(async move { handle.finish_claude_profile_login(&id, &code).await })
        };
        started.notified().await;
        // The first branch raises the cancellation epoch before the exchange resumes.
        let (cancelled, ()) =
            tokio::join!(env.handle.cancel_claude_profile_login(&session.id), async {
                resume.notify_one()
            });
        cancelled.unwrap();
        assert_eq!(task.await.unwrap().unwrap_err(), RuntimeError::Login);
        assert!(!env.profiles.join(B).exists() && env.spawned().is_empty());
    }

    #[tokio::test]
    async fn spawn_failures_and_bad_folders_are_visible_and_change_nothing() {
        let env = environment(true, false);
        env.authorize_profile("b", "org-b", FRESH);
        env.launcher.fail.store(true, Ordering::SeqCst);
        // The owner is verified explicitly before the (failing) spawn.
        env.push([owner("b", "org-b")]);
        assert_eq!(env.launch().await.unwrap_err(), RuntimeError::LaunchFailed);
        assert!(env.spawned().is_empty());
        assert!(env.handle.get_snapshot().error.is_some());
        for folder in [env.folder.join("missing"), PathBuf::from("relative")] {
            assert_eq!(
                env.handle
                    .launch_claude(B, LaunchModel::Default, folder)
                    .await
                    .unwrap_err(),
                RuntimeError::LaunchFolder
            );
        }
        env.launcher.fail.store(false, Ordering::SeqCst);
        env.push([owner("b", "org-b")]);
        assert_eq!(env.launch().await.unwrap(), LaunchOutcome::Launched);
        assert!(env.handle.get_snapshot().error.is_none());
        // The renderer-visible results carry no credential.
        let shape = serde_json::to_value([
            LaunchOutcome::Launched,
            LaunchOutcome::Authorized,
            LaunchOutcome::Cancelled,
            LaunchOutcome::AuthorizationRequired {
                reason: ProfileAuthReason::IdentityChanged,
            },
        ])
        .unwrap();
        assert_eq!(
            shape,
            json!([
                {"status": "launched"},
                {"status": "authorized"},
                {"status": "cancelled"},
                {"status": "authorizationRequired", "reason": "identityChanged"}
            ])
        );
    }

    #[tokio::test]
    async fn settings_that_would_change_billing_stop_the_launch_and_history_never_forks() {
        let env = environment(true, false);
        env.authorize_profile("b", "org-b", FRESH);
        env.push([owner("b", "org-b")]);
        std::fs::create_dir_all(env.folder.join(".claude")).unwrap();
        std::fs::write(
            env.folder.join(".claude/settings.local.json"),
            br#"{"env":{"ANTHROPIC_API_KEY":"SECRET_SENTINEL"}}"#,
        )
        .unwrap();
        let error = env.launch().await.unwrap_err();
        assert_eq!(error, RuntimeError::UnsupportedSetting("ANTHROPIC_API_KEY"));
        assert!(!error.message("en").contains("SENTINEL") && env.spawned().is_empty());
        // A profile that already holds its own project memory is reported, not merged.
        // (A fresh profile: an earlier launch would already have linked the shared one.)
        let env = environment(true, false);
        env.authorize_profile("b", "org-b", FRESH);
        env.push([owner("b", "org-b")]);
        let note = env.profiles.join(B).join("projects/slug/note.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, b"profile memory").unwrap();
        assert_eq!(
            env.launch().await.unwrap_err(),
            RuntimeError::LaunchHistorySplit
        );
        assert_eq!(std::fs::read(&note).unwrap(), b"profile memory");
        assert!(env.spawned().is_empty());
    }

    #[tokio::test]
    async fn dedicated_profile_commands_never_rewrite_the_global_vault_but_ordinary_ones_do() {
        let env = environment(true, false);
        // Startup clears cached identity proofs in memory only. A command that is not
        // about the global accounts must not write that view into the saved file.
        let untouched = serde_json::to_value(env.saved()).unwrap();
        let unchanged = || serde_json::to_value(env.saved()).unwrap() == untouched;
        env.handle.precheck_claude_launch(B).await.unwrap();
        assert!(unchanged());
        assert_eq!(
            env.launch().await.unwrap(),
            LaunchOutcome::AuthorizationRequired {
                reason: ProfileAuthReason::FirstUse
            }
        );
        env.handle.cancel_claude_profile_login("").await.unwrap();
        assert!(unchanged());
        // Failed dedicated operations: a wrong login, and a launch that is refused.
        env.launch().await.unwrap();
        assert_eq!(
            env.sign_in([tokens("evil"), owner("x", "org-x")])
                .await
                .unwrap_err(),
            RuntimeError::LoginIdentityMismatch
        );
        assert_eq!(
            env.handle
                .launch_claude(B, LaunchModel::Default, env.folder.join("missing"))
                .await
                .unwrap_err(),
            RuntimeError::LaunchFolder
        );
        assert!(unchanged());
        // Successful ones: the waiting launch completes, and a second launch needs no
        // browser.
        assert_eq!(
            env.sign_in([tokens("b"), owner("b", "org-b")])
                .await
                .unwrap(),
            LaunchOutcome::Launched
        );
        env.push([owner("b", "org-b")]);
        assert_eq!(env.launch().await.unwrap(), LaunchOutcome::Launched);
        env.handle.cancel_claude_profile_login("").await.unwrap();
        assert!(unchanged(), "accounts, credentials, readings and settings");
        // Ordinary commands keep persisting.
        env.handle.set_renewal_day(A, Some(7)).await.unwrap();
        let saved = env.saved();
        assert_eq!(saved.accounts[0].renewal_day, Some(7));
        assert_ne!(serde_json::to_value(&saved).unwrap(), untouched);
    }

    #[tokio::test]
    async fn profile_and_ordinary_sign_in_intents_cannot_complete_each_other() {
        let env = environment(true, false);
        env.launch().await.unwrap();
        let ordinary = env.handle.begin_login().await.unwrap();
        let profile = env.handle.begin_claude_profile_login(B).await.unwrap();
        env.push([tokens("b"), owner("b", "org-b")]);
        assert_eq!(
            env.handle
                .finish_login(&profile.id, &code_for(&profile))
                .await
                .unwrap_err(),
            RuntimeError::Login
        );
        assert_eq!(
            env.handle
                .finish_claude_profile_login(&ordinary.id, &code_for(&ordinary))
                .await
                .unwrap_err(),
            RuntimeError::Login
        );
        assert!(env.fake.calls.lock().unwrap().is_empty());
        // Neither failed attempt consumed the real one.
        assert_eq!(
            env.handle
                .finish_claude_profile_login(&profile.id, &code_for(&profile))
                .await
                .unwrap(),
            LaunchOutcome::Launched
        );
        assert_eq!(env.saved().accounts.len(), 2);
    }
}
