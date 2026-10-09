//! Codex operations on the runtime handle. Slow Codex work (quota readers, the daemon
//! restart and its graceful drain) runs outside the owner lock, serialized by
//! `codex_job`, so Claude actions and polling are never blocked by it.
use super::*;
use crate::codex_engine::{CodexAutoSwitch, CodexAutomation, QuotaJob, ResetJob, SwitchJob};

/// Background cadence: follow auth.json, read at most one due quota, then decide
/// automatic switching.
const CODEX_TICK: Duration = Duration::from_secs(60);
const REDISCOVER_AFTER: i64 = 600;
/// Readings automatic switching may take in one tick to confirm targets that were not
/// read recently; it decides again on the next tick.
const AUTO_TARGET_READS: usize = 2;

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
        let _job = self.codex_job_lock().await?;
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
    pub async fn codex_consume_reset(&self, id: &str) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        let job: ResetJob = {
            let mut engine = self.inner.owner.lock().await;
            let now = engine.clock.now();
            let result = {
                let Engine { codex, vault, .. } = &mut *engine;
                codex.plan_reset(
                    id,
                    vault.as_ref().ok_or(CodexReason::VaultUnavailable)?,
                    now,
                )
            };
            if let Err(error) = &result {
                engine.codex.error = Some(*error);
            }
            self.publish_codex_only(&mut engine, result.is_ok());
            result?
        };
        let report = job.run().await;
        let mut engine = self.inner.owner.lock().await;
        let now = engine.clock.now();
        let result = {
            let Engine { codex, vault, .. } = &mut *engine;
            codex.finish_quota(
                report,
                vault.as_ref().ok_or(CodexReason::VaultUnavailable)?,
                now,
            )
        };
        if let Err(error) = &result {
            engine.codex.error = Some(*error);
        }
        self.publish_codex_only(&mut engine, false);
        result?;
        Ok(self.get_codex_snapshot())
    }
    /// Read every saved ChatGPT account, the active one first. Per-account failures
    /// are recorded on the account and do not stop the others.
    pub async fn codex_refresh_all(&self) -> Result<CodexSnapshot, CodexReason> {
        let _job = self.codex_job_lock().await?;
        let ids = {
            let engine = self.inner.owner.lock().await;
            let policy = CodexPolicy::from(&engine.saved.settings);
            let snapshot = engine.codex.snapshot(false, engine.clock.now(), &policy);
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
        self.codex_switch_locked(id).await
    }
    /// The seamless switch shared by the window and automatic switching; the caller
    /// holds `codex_job`.
    async fn codex_switch_locked(&self, id: &str) -> Result<CodexSnapshot, CodexReason> {
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
    /// external logins, a hybrid write), read at most one due quota, then decide
    /// automatic switching.
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
            let policy = CodexPolicy::from(&engine.saved.settings);
            engine.codex.due_quota(engine.clock.now(), &policy)
        };
        if let Some(id) = due {
            let _ = self.codex_quota(&id, false).await;
        }
        self.codex_automate().await;
    }
    /// Automatic switching, after the tick's readings; the caller holds `codex_job`.
    /// It reuses the window's seamless switch, at most once per cooldown, and only on
    /// fresh readings of the active account and of the target.
    async fn codex_automate(&self) {
        let mut read: Vec<String> = Vec::new();
        loop {
            let (step, now) = {
                let engine = self.inner.owner.lock().await;
                if engine.demo || engine.vault.is_none() {
                    return;
                }
                let now = engine.clock.now();
                let policy = CodexPolicy::from(&engine.saved.settings);
                (engine.codex.automation(now, &policy), now)
            };
            match step {
                CodexAutomation::Idle => return,
                CodexAutomation::Refresh(id) => {
                    if read.len() >= AUTO_TARGET_READS || read.contains(&id) {
                        return;
                    }
                    let _ = self.codex_quota(&id, false).await;
                    read.push(id);
                }
                CodexAutomation::Exhausted { frees_first } => {
                    let mut engine = self.inner.owner.lock().await;
                    if engine.codex.exhausted_notice_due(now) {
                        let body = codex_exhausted_text(engine.ro(), frees_first.as_ref(), now);
                        engine.notify(body);
                    }
                    return;
                }
                CodexAutomation::Switch(plan) => {
                    self.inner.owner.lock().await.codex.begin_auto_switch(now);
                    let result = self.codex_switch_locked(&plan.target_id).await;
                    let mut engine = self.inner.owner.lock().await;
                    let ro = engine.ro();
                    match result {
                        Ok(snapshot) => {
                            if let Some(outcome) = snapshot
                                .last_switch
                                .as_ref()
                                .filter(|outcome| outcome.account_id == plan.target_id)
                            {
                                let threshold = engine.saved.settings.threshold;
                                let (title, body) =
                                    codex_switched_text(ro, &plan, outcome, threshold);
                                engine.notify_with(title, body);
                            }
                        }
                        Err(reason) => {
                            let now = engine.clock.now();
                            if engine
                                .codex
                                .failure_notice_due(&plan.target_id, reason, now)
                            {
                                engine.notify(codex_failed_text(ro, &plan.target_name));
                            }
                        }
                    }
                    return;
                }
            }
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

/// The notice after an automatic Codex switch: title and body.
fn codex_switched_text(
    ro: bool,
    plan: &CodexAutoSwitch,
    outcome: &CodexLastSwitchView,
    threshold: f64,
) -> (String, String) {
    let (target, previous) = (
        safe_label(&plan.target_name),
        safe_label(&plan.previous_name),
    );
    let title = if ro {
        format!("Codex a comutat pe {target}")
    } else {
        format!("Codex switched to {target}")
    };
    let reached = match plan.peak.filter(|peak| f64::from(*peak) >= threshold) {
        Some(percent) if ro => format!("{previous} a ajuns la {percent}%"),
        Some(percent) => format!("{previous} reached {percent}%"),
        None if ro => format!("{previous} a atins limita"),
        None => format!("{previous} reached its limit"),
    };
    let terminals = match (outcome.error.is_some(), outcome.daemon_restarted, ro) {
        (true, _, false) => "open terminals keep the previous account until you restart them.",
        (true, _, true) => "terminalele deschise păstrează contul anterior până le repornești.",
        (false, true, false) => "open terminals reconnect automatically.",
        (false, true, true) => "terminalele deschise se reconectează automat.",
        (false, false, false) => "the next Codex session uses it.",
        (false, false, true) => "următoarea sesiune Codex îl folosește.",
    };
    (title, format!("{reached} · {terminals}"))
}
/// Automatic switching wanted to switch, but every Codex account is at its limit.
fn codex_exhausted_text(ro: bool, frees_first: Option<&(String, i64)>, now: i64) -> String {
    let mut body = if ro {
        "Toate conturile Codex sunt la limită."
    } else {
        "All Codex accounts are at their limit."
    }
    .to_owned();
    if let Some((name, at)) = frees_first.filter(|(_, at)| *at > now) {
        let (label, wait, time) = (
            safe_label(name),
            duration_text(at - now, ro),
            local_time_text(*at, now, ro),
        );
        body.push(' ');
        body.push_str(&if ro {
            format!("Primul cont liber va fi {label}, în {wait} ({time}).")
        } else {
            format!("{label} frees up first, in {wait} ({time}).")
        });
    }
    body
}
fn codex_failed_text(ro: bool, name: &str) -> String {
    let label = safe_label(name);
    if ro {
        format!(
            "Codex nu a putut fi comutat automat pe {label}. Deschide PrimerSwitch pentru detalii."
        )
    } else {
        format!("Could not switch Codex to {label} automatically. Open PrimerSwitch for details.")
    }
}

#[cfg(test)]
impl RuntimeHandle {
    /// Fixture hook: edit the Codex engine (for example to install one with fake
    /// seams) and the shared settings, then publish.
    pub(crate) async fn edit_codex_for_test(
        &self,
        edit: impl FnOnce(&mut CodexEngine, &mut Settings),
    ) {
        let mut engine = self.inner.owner.lock().await;
        let Engine { codex, saved, .. } = &mut *engine;
        edit(codex, &mut saved.settings);
        self.publish(&mut engine, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_engine::CodexAutoSwitch;
    fn plan(peak: Option<i32>) -> CodexAutoSwitch {
        CodexAutoSwitch {
            target_id: "b".into(),
            target_name: "b@example.invalid".into(),
            previous_name: "a@example.invalid".into(),
            peak,
        }
    }
    fn outcome(daemon_restarted: bool, error: Option<CodexReason>) -> CodexLastSwitchView {
        CodexLastSwitchView {
            account_id: "b".into(),
            at: 0,
            daemon_restarted,
            other_clients: 0,
            error,
        }
    }
    #[test]
    fn automatic_switch_notices_say_why_and_what_terminals_do_in_both_languages() {
        let live = outcome(true, None);
        assert_eq!(
            codex_switched_text(false, &plan(Some(105)), &live, 95.0),
            (
                "Codex switched to b@example.invalid".into(),
                "a@example.invalid reached 105% · open terminals reconnect automatically.".into()
            )
        );
        assert_eq!(
            codex_switched_text(true, &plan(Some(96)), &live, 95.0),
            (
                "Codex a comutat pe b@example.invalid".into(),
                "a@example.invalid a ajuns la 96% · terminalele deschise se reconectează automat."
                    .into()
            )
        );
        // A reached-limit flag below the threshold names the limit, not a percentage.
        assert_eq!(
            codex_switched_text(false, &plan(Some(40)), &outcome(false, None), 95.0).1,
            "a@example.invalid reached its limit · the next Codex session uses it."
        );
        assert_eq!(
            codex_switched_text(true, &plan(None), &outcome(false, None), 95.0).1,
            "a@example.invalid a atins limita · următoarea sesiune Codex îl folosește."
        );
        let failed = outcome(false, Some(CodexReason::DaemonRestartFailed));
        assert_eq!(
            codex_switched_text(false, &plan(Some(97)), &failed, 95.0).1,
            "a@example.invalid reached 97% · open terminals keep the previous account until you restart them."
        );
        assert_eq!(
            codex_switched_text(true, &plan(Some(97)), &failed, 95.0).1,
            "a@example.invalid a ajuns la 97% · terminalele deschise păstrează contul anterior până le repornești."
        );
        assert_eq!(
            codex_failed_text(false, "b\u{7}@example.invalid"),
            "Could not switch Codex to b@example.invalid automatically. Open PrimerSwitch for details."
        );
        assert_eq!(
            codex_failed_text(true, "b@example.invalid"),
            "Codex nu a putut fi comutat automat pe b@example.invalid. Deschide PrimerSwitch pentru detalii."
        );
    }
    #[test]
    fn the_all_limited_notice_names_who_frees_up_first_only_when_known() {
        assert_eq!(
            codex_exhausted_text(false, None, 1000),
            "All Codex accounts are at their limit."
        );
        let first = ("b@example.invalid".to_owned(), 1000 + 2 * 3600 + 600);
        let english = codex_exhausted_text(false, Some(&first), 1000);
        assert!(
            english.starts_with(
                "All Codex accounts are at their limit. b@example.invalid frees up first, in 2h 10m ("
            ),
            "{english}"
        );
        let romanian = codex_exhausted_text(true, Some(&first), 1000);
        assert!(
            romanian.starts_with(
                "Toate conturile Codex sunt la limită. Primul cont liber va fi b@example.invalid, în 2 h 10 min ("
            ),
            "{romanian}"
        );
        // A reset time already passed adds nothing misleading.
        assert_eq!(
            codex_exhausted_text(true, Some(&("b".into(), 900)), 1000),
            "Toate conturile Codex sunt la limită."
        );
    }
}
