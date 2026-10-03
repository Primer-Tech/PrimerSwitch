//! Codex operations on the runtime handle. Slow Codex work (quota readers, the daemon
//! restart and its graceful drain) runs outside the owner lock, serialized by
//! `codex_job`, so Claude actions and polling are never blocked by it.
use super::*;
use crate::codex_engine::{QuotaJob, SwitchJob};

/// Background cadence: follow auth.json and read at most one due quota.
const CODEX_TICK: Duration = Duration::from_secs(60);
const REDISCOVER_AFTER: i64 = 600;

impl RuntimeHandle {
    fn publish_codex_only(&self, engine: &mut Engine, busy: bool) {
        self.publish_codex(engine, busy);
    }
    async fn run_codex(&self, command: CodexCommand) -> Result<CodexOutput, CodexReason> {
        let mut engine = if matches!(&command, CodexCommand::PollLogin(_)) {
            self.inner.owner.try_lock().map_err(|_| CodexReason::Busy)?
        } else {
            self.inner.owner.lock().await
        };
        if engine.demo || engine.vault.is_none() {
            return Err(CodexReason::VaultUnavailable);
        }
        let quiet = matches!(&command, CodexCommand::Observe | CodexCommand::PollLogin(_));
        if !quiet {
            self.publish_codex_only(&mut engine, true);
        }
        let now = engine.clock.now();
        let result = {
            let Engine { codex, vault, .. } = &mut *engine;
            codex
                .command(
                    command,
                    vault.as_ref().ok_or(CodexReason::VaultUnavailable)?,
                    now,
                )
                .await
        };
        if let Err(error) = &result {
            engine.codex.error = Some(*error);
        }
        self.publish_codex_only(&mut engine, false);
        result
    }
    async fn codex_snapshot_command(
        &self,
        command: CodexCommand,
    ) -> Result<CodexSnapshot, CodexReason> {
        self.run_codex(command).await?;
        Ok(self.get_codex_snapshot())
    }
    /// User mutations wait for a short background reading, never for a switch.
    async fn codex_job_lock(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, CodexReason> {
        if self.get_codex_snapshot().switching.is_some() {
            return Err(CodexReason::SwitchInProgress);
        }
        Ok(self.inner.codex_job.lock().await)
    }
    pub async fn shutdown_codex(&self) {
        self.inner.codex_intent.fetch_add(1, Ordering::AcqRel);
        let mut engine = self.inner.owner.lock().await;
        engine.codex.shutdown_owned().await;
    }
    pub async fn codex_discover(&self) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        self.codex_snapshot_command(CodexCommand::Discover).await
    }
    pub async fn codex_begin_login(&self) -> Result<CodexLoginLaunch, CodexReason> {
        let intent = self.inner.codex_intent.fetch_add(1, Ordering::AcqRel) + 1;
        match self.run_codex(CodexCommand::BeginLogin(intent)).await? {
            CodexOutput::Login(value) => Ok(value),
            _ => Err(CodexReason::ProviderUnavailable),
        }
    }
    pub async fn codex_poll_login(&self, id: &str) -> Result<CodexSnapshot, CodexReason> {
        let snapshot = self
            .codex_snapshot_command(CodexCommand::PollLogin(id.into()))
            .await?;
        if snapshot
            .login
            .as_ref()
            .is_some_and(|login| login.status == CodexLoginStatus::Complete)
        {
            let account = self
                .inner
                .owner
                .lock()
                .await
                .codex
                .last_login_account
                .take();
            if let Some(account) = account {
                self.spawn_codex_reading(account);
            }
        }
        Ok(snapshot)
    }
    pub async fn codex_cancel_login(&self, id: &str) -> Result<CodexSnapshot, CodexReason> {
        let current = self.get_codex_snapshot().login;
        if current.is_some_and(|p| p.id == id) {
            self.inner.codex_intent.fetch_add(1, Ordering::AcqRel);
        }
        self.codex_snapshot_command(CodexCommand::CancelLogin(id.into()))
            .await
    }
    pub async fn codex_import_current(&self) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        self.codex_snapshot_command(CodexCommand::ImportCurrent)
            .await
    }
    pub async fn codex_import_switcher(&self) -> Result<CodexSnapshot, CodexReason> {
        let snapshot = {
            let _job = self.codex_job_lock().await?;
            self.codex_snapshot_command(CodexCommand::ImportSwitcher)
                .await?
        };
        let this = self.clone();
        tokio::spawn(async move {
            let _ = this.codex_refresh_all().await;
        });
        Ok(snapshot)
    }
    pub async fn codex_delete_account(&self, id: &str) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        self.codex_snapshot_command(CodexCommand::Delete(id.into()))
            .await
    }
    pub async fn codex_refresh_account(&self, id: &str) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        self.codex_quota(id, true).await?;
        Ok(self.get_codex_snapshot())
    }
    /// Read every saved ChatGPT account, the active one first. Per-account failures
    /// are recorded on the account and do not stop the others.
    pub async fn codex_refresh_all(&self) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        let ids = {
            let engine = self.inner.owner.lock().await;
            let snapshot = engine.codex.snapshot(false, engine.clock.now());
            let mut ids: Vec<_> = snapshot
                .accounts
                .iter()
                .filter(|a| a.auth_kind == "chatgpt")
                .map(|a| (!a.selected, a.id.clone()))
                .collect();
            ids.sort();
            ids
        };
        for (_, id) in ids {
            let _ = self.codex_quota(&id, true).await;
        }
        Ok(self.get_codex_snapshot())
    }
    /// One quota reading; the caller holds `codex_job`.
    async fn codex_quota(&self, id: &str, manual: bool) -> Result<(), CodexReason> {
        let job: QuotaJob = {
            let mut engine = self.inner.owner.lock().await;
            if engine.demo || engine.vault.is_none() {
                return Err(CodexReason::VaultUnavailable);
            }
            let now = engine.clock.now();
            let job = engine.codex.plan_quota(id, now, manual);
            if manual && let Err(error) = &job {
                engine.codex.error = Some(*error);
                self.publish_codex_only(&mut engine, false);
            }
            job?
        };
        let report = job.run().await;
        let mut engine = self.inner.owner.lock().await;
        let now = engine.clock.now();
        let result = {
            let Engine { codex, vault, .. } = &mut *engine;
            match vault.as_ref() {
                Some(vault) => codex.finish_quota(report, vault, now),
                None => Err(CodexReason::VaultUnavailable),
            }
        };
        self.publish_codex_only(&mut engine, false);
        result
    }
    fn spawn_codex_reading(&self, id: String) {
        let this = self.clone();
        tokio::spawn(async move {
            let _job = this.inner.codex_job.lock().await;
            let _ = this.codex_quota(&id, true).await;
        });
    }
    /// Switch the active Codex login. Terminals attached to Codex's shared daemon
    /// reconnect on the new account; see codex_engine.rs.
    pub async fn codex_switch_account(&self, id: &str) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        let job: Option<SwitchJob> = {
            let mut engine = self.inner.owner.lock().await;
            if engine.demo || engine.vault.is_none() {
                return Err(CodexReason::VaultUnavailable);
            }
            let now = engine.clock.now();
            engine.codex.error = None;
            let result = {
                let Engine { codex, vault, .. } = &mut *engine;
                let vault = vault.as_ref().ok_or(CodexReason::VaultUnavailable)?;
                codex.begin_switch(id, vault, now)
            };
            if let Err(error) = &result {
                engine.codex.error = Some(*error);
            }
            self.publish_codex_only(&mut engine, false);
            result?
        };
        let Some(job) = job else {
            return Ok(self.get_codex_snapshot());
        };
        let mut outcome = job.run().await;
        let mut allow_repair = true;
        loop {
            let repair = {
                let mut engine = self.inner.owner.lock().await;
                let now = engine.clock.now();
                let result = {
                    let Engine { codex, vault, .. } = &mut *engine;
                    match vault.as_ref() {
                        Some(vault) => {
                            codex.finish_switch(&job, &outcome, vault, now, allow_repair)
                        }
                        None => Err(CodexReason::VaultUnavailable),
                    }
                };
                if let Err(error) = &result {
                    engine.codex.abort_switch();
                    engine.codex.error = Some(*error);
                }
                self.publish_codex_only(&mut engine, false);
                result?
            };
            if !repair {
                break;
            }
            allow_repair = false;
            outcome = job.restart_again().await;
        }
        self.spawn_codex_reading(job.target_id().to_owned());
        Ok(self.get_codex_snapshot())
    }
    /// Background step: discover Codex if needed, follow auth.json (rotations,
    /// external logins, a hybrid write) and read at most one due quota.
    pub async fn codex_tick(&self) {
        let Ok(_job) = self.inner.codex_job.try_lock() else {
            return;
        };
        let discover = {
            let engine = self.inner.owner.lock().await;
            if engine.demo || engine.vault.is_none() {
                return;
            }
            let now = engine.clock.now();
            engine.codex.needs_discovery(now, REDISCOVER_AFTER)
        };
        if discover {
            let _ = self.run_codex(CodexCommand::Discover).await;
        } else {
            let _ = self.run_codex(CodexCommand::Observe).await;
        }
        let due = {
            let engine = self.inner.owner.lock().await;
            engine.codex.due_quota(engine.clock.now())
        };
        if let Some(id) = due {
            let _ = self.codex_quota(&id, false).await;
        }
    }
    pub fn start_codex_scheduler(&self) {
        if self
            .inner
            .codex_scheduler_started
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let handle = Self { inner };
                handle.codex_tick().await;
                drop(handle);
                tokio::time::sleep(CODEX_TICK).await;
            }
        });
    }
}
