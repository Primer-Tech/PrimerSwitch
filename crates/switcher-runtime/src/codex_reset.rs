//! Explicit reset redemption using the normal account reader and a durable
//! idempotency key; neither polling nor automatic selection redeems resets.
use super::*;

pub(crate) struct ResetJob {
    reading: QuotaJob,
    key: String,
    retry: bool,
}

impl ResetJob {
    pub(crate) async fn run(self) -> QuotaReport {
        let reading = self.reading;
        let (context, role) = match &reading.target {
            QuotaTarget::Active(context) => (context.clone(), ClientRole::Active),
            QuotaTarget::Owned { context, .. } => (context.clone(), ClientRole::Owned),
        };
        let (outcome, reset) = match reading
            .factory
            .start(reading.executable.as_ref(), context, role)
            .await
        {
            Ok(mut client) => {
                let result = redeem(client.as_mut(), &reading.binding, &self.key, self.retry).await;
                let _ = client.shutdown().await;
                result
            }
            Err(error) => (Err(error), Err(error)),
        };
        QuotaReport {
            account_id: reading.account_id,
            binding: reading.binding,
            owned: reading.target.finish().await,
            outcome,
            reset: Some((self.key, reset)),
        }
    }
}

async fn redeem(
    client: &mut dyn CodexService,
    binding: &Binding,
    key: &str,
    retry: bool,
) -> (
    Result<CodexRateLimits, CodexError>,
    Result<ResetOutcome, CodexError>,
) {
    let before = match client.read_rate_limits().await {
        Ok(limits) => limits,
        Err(error) => return (Err(error), Err(error)),
    };
    // A copied login is not enough: require current backend workspace evidence
    // before making a request that can consume a credit.
    if !binding.complete() || before.account_id != binding.workspace {
        return (
            Err(CodexError::IdentityMismatch),
            Err(CodexError::IdentityMismatch),
        );
    }
    if !retry {
        match before
            .rate_limit_reset_credits
            .as_ref()
            .map(|v| v.available_count)
        {
            Some(0) => return (Ok(before), Ok(ResetOutcome::NoCredit)),
            Some(count) if count > 0 => (),
            _ => return (Ok(before), Err(CodexError::Protocol)),
        }
    }
    // A retry must still reach consume when the count is now zero: that can be
    // the effect of the original request whose response was lost.
    let reset = client.consume_reset(key).await;
    let after = client.read_rate_limits().await;
    (after, reset)
}

impl CodexEngine {
    pub(super) fn reset_capability(&self, account: &SavedAccount, now: i64) -> CodexCapability {
        let reason = self.common_block().or_else(|| {
            if self.pending_login.is_some() {
                Some(CodexReason::Busy)
            } else if self.switching.is_some() {
                Some(CodexReason::SwitchInProgress)
            } else if account.auth_kind != AuthKind::ManagedChatgpt {
                Some(CodexReason::UnsupportedAuth)
            } else if account.needs_sign_in {
                Some(CodexReason::SignInRequired)
            } else if !account.binding.complete() {
                Some(CodexReason::IdentityUnverified)
            } else if account.pending_reset_key.is_some() {
                None
            } else if self.quota_state(account, now) != CodexQuotaState::Fresh {
                Some(CodexReason::ResetUnavailable)
            } else {
                match account
                    .quota
                    .as_ref()
                    .and_then(|q| q.reset_credits_available)
                {
                    Some(count) if count > 0 => None,
                    Some(0) => Some(CodexReason::NoResetCredits),
                    _ => Some(CodexReason::ResetUnavailable),
                }
            }
        });
        reason.map_or_else(CodexCapability::allowed, CodexCapability::blocked)
    }

    pub(crate) fn plan_reset(
        &mut self,
        id: &str,
        vault: &Vault,
        now: i64,
    ) -> Result<ResetJob, CodexReason> {
        self.ready()?;
        let index = self.account_index(id)?;
        let capability = self.reset_capability(&self.saved.accounts[index], now);
        if let Some(reason) = capability.blocked_reason {
            return Err(reason);
        }
        let retry = self.saved.accounts[index].pending_reset_key.is_some();
        let key = self.saved.accounts[index]
            .pending_reset_key
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let reading = self.plan_quota(id, now, true)?;
        self.saved.accounts[index].pending_reset_key = Some(key.clone());
        self.saved.accounts[index].last_reset_outcome = None;
        // The key is encrypted and durable before the child can send consume.
        if let Err(error) = self.save(vault) {
            self.writable = false;
            self.reason = Some(error);
            return Err(error);
        }
        self.error = None;
        Ok(ResetJob {
            reading,
            key,
            retry,
        })
    }
}
