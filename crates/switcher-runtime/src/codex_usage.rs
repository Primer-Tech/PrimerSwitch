//! Local API-key consumption. Selection evidence remains owned by CodexEngine;
//! rollout reads and atomic ledger writes run as one blocking job outside its lock.
use super::*;
#[cfg(not(test))]
use switcher_platform::codex::usage::scan_usage;
#[cfg(test)]
use switcher_platform::codex::usage::scan_usage_for_fixture;
use switcher_platform::codex::usage::{
    ActivationSpan, KeyActivationSpans, UsageCounters, UsageError, UsageLedger, UsageScanReport,
    UsageStore,
};

const MAX_SPANS: usize = 200;
const RESUME_SECONDS: i64 = 600;
const JS_MAX_INTEGER: u64 = (1 << 53) - 1;
const LAST_DISPLAYABLE_SECOND: i64 = 253_402_300_799;

#[derive(Default)]
pub(super) struct KeyUsageTracking {
    store: Option<UsageStore>,
    ledger: Option<UsageLedger>,
    /// None before the first actual read of the login in this process.
    observed: bool,
    active: Option<String>,
    pending: bool,
    scanning: bool,
    unavailable: bool,
    #[cfg(test)]
    before_scan: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub(crate) struct KeyUsageJob {
    store: UsageStore,
    sessions: Option<PathBuf>,
    spans: KeyActivationSpans,
    ids: BTreeSet<String>,
    ledger: Option<UsageLedger>,
    now: i64,
    #[cfg(test)]
    before_scan: Option<Arc<dyn Fn() + Send + Sync>>,
}
pub(crate) struct KeyUsageResult {
    ledger: UsageLedger,
    more: bool,
    unavailable: bool,
}
impl KeyUsageJob {
    /// The caller holds codex_job, never the runtime owner. No credential values
    /// or provider clients enter the blocking closure.
    pub(crate) fn run(self) -> Result<KeyUsageResult, UsageError> {
        #[cfg(not(test))]
        {
            self.run_scanner(scan_usage)
        }
        #[cfg(test)]
        {
            // Every unit-test entry, including jobs started by RuntimeHandle,
            // keeps the real byte cap but never races the OS scheduler.
            self.run_with_deadline(std::time::Duration::MAX)
        }
    }

    #[cfg(test)]
    pub(crate) fn run_with_deadline(
        self,
        duration: std::time::Duration,
    ) -> Result<KeyUsageResult, UsageError> {
        self.run_scanner(|sessions, spans, ledger, now| {
            scan_usage_for_fixture(sessions, spans, ledger, now, duration)
        })
    }

    fn run_scanner(
        self,
        scan: impl FnOnce(
            &Path,
            &KeyActivationSpans,
            UsageLedger,
            i64,
        ) -> Result<UsageScanReport, UsageError>,
    ) -> Result<KeyUsageResult, UsageError> {
        #[cfg(test)]
        if let Some(hook) = self.before_scan {
            hook();
        }
        let mut ledger = self.ledger.unwrap_or_else(|| self.store.load());
        ledger.retain_keys(&self.ids);
        let (ledger, more, unavailable) = if self.spans.is_empty() {
            // Deletion still commits even when there is nothing left to scan.
            (ledger, false, false)
        } else {
            let sessions = self.sessions.ok_or(UsageError::UnsafePath)?;
            let report = scan(&sessions, &self.spans, ledger, self.now)?;
            (
                report.ledger,
                report.budget_exhausted,
                !report.errors.is_empty(),
            )
        };
        self.store.save(&ledger)?;
        Ok(KeyUsageResult {
            ledger,
            more,
            unavailable,
        })
    }
}

#[cfg(test)]
impl KeyUsageResult {
    pub(crate) fn has_more(&self) -> bool {
        self.more
    }
}

impl CodexEngine {
    pub(super) fn bind_usage_store(&mut self, vault: &Vault) {
        if self.key_usage.store.is_none() && !self.demo {
            self.key_usage.store = Some(UsageStore::new(
                vault.directory().join("codex-key-usage.json"),
            ));
            // Load/catch up lazily on the blocking worker, including inactive keys.
            self.key_usage.pending = true;
        }
    }

    /// The few evidence boundaries all call here: a successful live read (including
    /// an unchanged file), successful auth replacement, import and switch completion.
    /// Unreadable/unqualified/skipped observations never extend a span.
    pub(super) fn record_usage_observation(&mut self, vault: &Vault, now: i64) {
        self.record_usage_login(self.selected_id.clone(), vault, now);
    }

    pub(super) fn record_usage_login(&mut self, selected: Option<String>, vault: &Vault, now: i64) {
        if self.demo || !self.writable || now < 0 {
            return;
        }
        self.bind_usage_store(vault);
        let key = selected.as_ref().and_then(|id| {
            self.saved
                .accounts
                .iter()
                .find(|a| &a.id == id && a.auth_kind == AuthKind::ApiKey)
                .map(|a| a.id.clone())
        });
        let first = !self.key_usage.observed;
        let changed = self.key_usage.active != key;
        let mut spans_changed = false;
        // A closed span stays at its last proven observation, avoiding attribution
        // of the unobserved interval before an external login change.
        if changed && self.key_usage.active.is_some() {
            self.key_usage.pending = true;
            // Seconds are inclusive. A new login owns its observation second;
            // never overlap two keys at a manual switch's shared boundary.
            if let Some(previous) = self
                .saved
                .accounts
                .iter_mut()
                .find(|a| Some(&a.id) == self.key_usage.active.as_ref())
                && let Some(last) = previous.usage_spans.last_mut()
                && last.end >= now
            {
                last.end = now.saturating_sub(1);
                if last.end < last.start {
                    previous.usage_spans.pop();
                }
                spans_changed = true;
            }
        }
        if let Some(id) = &key {
            let account = self
                .saved
                .accounts
                .iter_mut()
                .find(|a| &a.id == id)
                .unwrap();
            let continue_span = account.usage_spans.last().is_some_and(|span| {
                if first {
                    (0..=RESUME_SECONDS).contains(&now.saturating_sub(span.end))
                } else {
                    !changed && now >= span.end
                }
            });
            if continue_span {
                let last = account.usage_spans.last_mut().unwrap();
                if last.end != now {
                    last.end = now;
                    spans_changed = true;
                }
            } else {
                account.usage_spans.push(ActivationSpan {
                    start: now,
                    end: now,
                });
                let excess = account.usage_spans.len().saturating_sub(MAX_SPANS);
                account.usage_spans.drain(..excess);
                spans_changed = true;
            }
        }
        self.key_usage.observed = true;
        self.key_usage.active = key;
        if spans_changed {
            self.key_usage.pending = true;
            // Optional tracking must never make startup or switching fail.
            if self.save(vault).is_err() {
                self.key_usage.unavailable = true;
            }
        }
    }

    /// Called after successful saved-account mutations. Deletions and scan commits
    /// share codex_job, so an older scan cannot resurrect removed consumption.
    pub(super) fn prune_key_usage(&mut self, vault: &Vault) {
        self.bind_usage_store(vault);
        let ids = self.usage_key_ids();
        if let Some(ledger) = &mut self.key_usage.ledger {
            let before = ledger.keys.len();
            ledger.retain_keys(&ids);
            self.key_usage.pending |= before != ledger.keys.len();
        }
    }

    fn usage_key_ids(&self) -> BTreeSet<String> {
        self.saved
            .accounts
            .iter()
            .filter(|a| a.auth_kind == AuthKind::ApiKey)
            .map(|a| a.id.clone())
            .collect()
    }

    pub(crate) fn plan_key_usage(&mut self, now: i64) -> Option<KeyUsageJob> {
        if self.demo || !self.writable || !self.key_usage.pending || self.key_usage.scanning {
            return None;
        }
        let store = self.key_usage.store.clone()?;
        let spans: KeyActivationSpans = self
            .saved
            .accounts
            .iter()
            .filter(|a| a.auth_kind == AuthKind::ApiKey && !a.usage_spans.is_empty())
            .map(|a| (a.id.clone(), a.usage_spans.clone()))
            .collect();
        let sessions = self
            .store()
            .ok()
            .map(|store| store.context().home().join("sessions"));
        if !spans.is_empty() && sessions.is_none() {
            self.key_usage.unavailable = true;
            return None;
        }
        self.key_usage.pending = false;
        self.key_usage.scanning = true;
        Some(KeyUsageJob {
            store,
            sessions,
            spans,
            ids: self.usage_key_ids(),
            ledger: self.key_usage.ledger.clone(),
            now,
            #[cfg(test)]
            before_scan: self.key_usage.before_scan.take(),
        })
    }

    #[cfg(test)]
    pub(crate) fn usage_scan_hook(&mut self, hook: Arc<dyn Fn() + Send + Sync>) {
        self.key_usage.before_scan = Some(hook);
        self.key_usage.pending = true;
    }

    pub(crate) fn finish_key_usage(&mut self, result: Result<KeyUsageResult, UsageError>) {
        self.key_usage.scanning = false;
        match result {
            Ok(result) => {
                self.key_usage.ledger = Some(result.ledger);
                self.key_usage.pending |= result.more || result.unavailable;
                self.key_usage.unavailable = result.unavailable;
            }
            Err(_) => {
                self.key_usage.pending = true;
                self.key_usage.unavailable = true;
            }
        }
    }

    pub(super) fn key_usage_view(&self, account: &SavedAccount, now: i64) -> CodexKeyUsageView {
        let ledger = self
            .key_usage
            .ledger
            .as_ref()
            .and_then(|l| l.keys.get(&account.id));
        let windows = ledger.map(|l| l.windows(now));
        CodexKeyUsageView {
            tracked_since: ledger
                .map(|l| l.tracking_since)
                .or_else(|| account.usage_spans.first().map(|span| span.start))
                .map(|at| at.clamp(0, LAST_DISPLAYABLE_SECOND)),
            windows: windows.map_or_else(CodexUsageWindows::default, |w| CodexUsageWindows {
                last_24h: counts(w.last_24h),
                last_7d: counts(w.last_7d),
                last_30d: counts(w.last_30d),
                total: counts(w.all_time),
            }),
            state: if self.key_usage.unavailable {
                CodexUsageState::Unavailable
            } else if self.key_usage.scanning {
                CodexUsageState::Scanning
            } else {
                CodexUsageState::Idle
            },
        }
    }
}

fn counts(value: UsageCounters) -> CodexTokenCounts {
    CodexTokenCounts {
        input: value.input_uncached.min(JS_MAX_INTEGER),
        cached: value.cached.min(JS_MAX_INTEGER),
        output: value.output.min(JS_MAX_INTEGER),
        total: value.total.min(JS_MAX_INTEGER),
    }
}
