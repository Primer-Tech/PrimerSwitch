use crate::{AccountUsageInfo, CoreError, PendingResetClaim, ResetGrant, UsageResponse};
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub fn is_usable(info: &AccountUsageInfo, threshold: f64, model: Option<&str>) -> bool {
    info.usage.as_ref().is_some_and(|u| {
        threshold.is_finite()
            && crate::models::valid_percent(u.five_hour.utilization)
            && crate::models::valid_percent(u.weekly_utilization_for_model(model))
            && u.five_hour.utilization < threshold
            && u.weekly_utilization_for_model(model) < threshold
    })
}
pub fn has_headroom(
    info: &AccountUsageInfo,
    threshold: f64,
    model: Option<&str>,
    margin: f64,
) -> bool {
    is_usable(info, threshold, model)
        && margin.is_finite()
        && margin >= 0.0
        && info.usage.as_ref().is_some_and(|u| {
            u.five_hour.utilization <= threshold - margin
                && u.weekly_utilization_for_model(model) <= threshold - margin
        })
}
fn compare(
    a: &AccountUsageInfo,
    b: &AccountUsageInfo,
    threshold: f64,
    model: Option<&str>,
) -> Ordering {
    let ua = a.usage.as_ref().expect("usable account has usage");
    let ub = b.usage.as_ref().expect("usable account has usage");
    has_headroom(b, threshold, model, 5.0)
        .cmp(&has_headroom(a, threshold, model, 5.0))
        .then_with(|| {
            ua.weekly_reset_bucket()
                .unwrap_or(i64::MAX)
                .cmp(&ub.weekly_reset_bucket().unwrap_or(i64::MAX))
        })
        .then_with(|| {
            ua.five_hour
                .utilization
                .total_cmp(&ub.five_hour.utilization)
        })
        .then_with(|| a.account.id.cmp(&b.account.id))
}
pub fn best_candidate(
    infos: &[AccountUsageInfo],
    threshold: f64,
    model: Option<&str>,
) -> Option<String> {
    infos
        .iter()
        .filter(|i| !i.is_active && is_usable(i, threshold, model))
        .min_by(|a, b| compare(a, b, threshold, model))
        .map(|i| i.account.id.clone())
}
pub fn consumption_plan(
    infos: &[AccountUsageInfo],
    threshold: f64,
    model: Option<&str>,
) -> Vec<String> {
    let mut sorted: Vec<_> = infos
        .iter()
        .filter(|i| is_usable(i, threshold, model))
        .collect();
    sorted.sort_by(|a, b| compare(a, b, threshold, model));
    sorted.into_iter().map(|i| i.account.id.clone()).collect()
}
pub fn next_poll_delay(
    interval: u64,
    active_usage: Option<&UsageResponse>,
    threshold: f64,
    model: Option<&str>,
    exhausted: bool,
) -> u64 {
    if !exhausted
        && active_usage.is_some_and(|u| {
            u.five_hour.utilization >= threshold - 3.0
                || u.weekly_utilization_for_model(model) >= threshold - 3.0
        })
    {
        30
    } else {
        interval.max(120)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetAction {
    pub account_id: String,
    pub grant_id: String,
    pub reason: String,
    pub switch_first: bool,
}
fn binding_windows(
    usage: &UsageResponse,
    threshold: f64,
    model: Option<&str>,
) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    if usage.five_hour.utilization >= threshold {
        out.insert("five_hour".into(), usage.five_hour.utilization);
    }
    if usage.weekly_utilization() >= threshold {
        out.insert("seven_day".into(), usage.weekly_utilization());
    }
    if let Some(scoped) = usage.scoped_limit(model)
        && scoped.percent >= threshold
        && usage.weekly_utilization() < threshold
    {
        out.insert(
            format!("seven_day_{}", scoped.label().to_lowercase()),
            scoped.percent,
        );
    }
    out
}
fn clears(grant: &ResetGrant, window: &str) -> bool {
    if grant.clears.is_empty() {
        return matches!(window, "five_hour" | "seven_day");
    }
    grant
        .clears
        .iter()
        .any(|s| s == window || (s == "seven_day_overage_included" && window == "seven_day"))
}
fn at_limit(usage: Option<&UsageResponse>, grant: &ResetGrant, model: Option<&str>) -> bool {
    usage.is_some_and(|u| {
        binding_windows(u, 100.0, model)
            .keys()
            .any(|w| clears(grant, w))
    })
}
fn offer(info: &AccountUsageInfo, now: i64) -> Option<&ResetGrant> {
    let a = &info.account;
    let status = a.reset_status.as_ref()?;
    if let Some(last) = a.last_reset_attempt_at {
        let floor = if a.pending_reset_claim.is_some() {
            300
        } else {
            1800
        };
        if now.saturating_sub(last) < floor {
            return None;
        }
        if a.pending_reset_claim.is_none() && a.reset_status_at.unwrap_or(0) <= last {
            return None;
        }
    }
    // An unresolved old request never authorizes a new grant/key. Reconciliation is
    // owned by the runtime, including requests whose offer has since disappeared.
    let live = status.available(now);
    if let Some(pending) = &a.pending_reset_claim {
        if status
            .next_grant_id
            .as_ref()
            .is_some_and(|id| id != &pending.grant_id)
        {
            return None;
        }
        return live.into_iter().find(|g| g.id == pending.grant_id);
    }
    if let Some(next) = &status.next_grant_id {
        return live.into_iter().find(|g| &g.id == next);
    }
    live.into_iter().min_by(|a, b| {
        a.ends_at
            .unwrap_or(i64::MAX)
            .cmp(&b.ends_at.unwrap_or(i64::MAX))
            .then_with(|| a.id.cmp(&b.id))
    })
}
pub fn decide_resets(
    infos: &[AccountUsageInfo],
    threshold: f64,
    model: Option<&str>,
    now: i64,
) -> Vec<ResetAction> {
    let mut actions = vec![];
    if let Some(active) = infos.iter().find(|i| i.is_active)
        && active.usage.is_some()
        && !is_usable(active, threshold, model)
        && best_candidate(infos, threshold, model).is_none()
    {
        let pick = infos
            .iter()
            .filter_map(|i| {
                let g = offer(i, now)?;
                let u = i.usage.as_ref()?;
                let binding = binding_windows(u, threshold, model);
                if binding.is_empty()
                    || !binding.keys().all(|w| clears(g, w))
                    || (g.use_requires_limit && !at_limit(Some(u), g, model))
                {
                    return None;
                }
                Some((i, g))
            })
            .min_by(|(a, ga), (b, gb)| {
                b.is_active
                    .cmp(&a.is_active)
                    .then_with(|| {
                        ga.ends_at
                            .unwrap_or(i64::MAX)
                            .cmp(&gb.ends_at.unwrap_or(i64::MAX))
                    })
                    .then_with(|| {
                        b.usage
                            .as_ref()
                            .and_then(UsageResponse::weekly_resets_at)
                            .unwrap_or(i64::MIN)
                            .cmp(
                                &a.usage
                                    .as_ref()
                                    .and_then(UsageResponse::weekly_resets_at)
                                    .unwrap_or(i64::MIN),
                            )
                    })
                    .then_with(|| a.account.id.cmp(&b.account.id))
            });
        if let Some((i, g)) = pick {
            actions.push(ResetAction {
                account_id: i.account.id.clone(),
                grant_id: g.id.clone(),
                reason: "ultima soluție".into(),
                switch_first: !i.is_active,
            });
        }
    }
    for i in infos {
        if actions.iter().any(|a| a.account_id == i.account.id) {
            continue;
        }
        let Some(g) = offer(i, now) else {
            continue;
        };
        if !g.usable_now
            || g.ends_at.is_none_or(|t| t.saturating_sub(now) > 10800)
            || (g.use_requires_limit && !at_limit(i.usage.as_ref(), g, model))
        {
            continue;
        }
        actions.push(ResetAction {
            account_id: i.account.id.clone(),
            grant_id: g.id.clone(),
            reason: "expiră curând".into(),
            switch_first: false,
        });
    }
    actions
}
/// Preserve a pending idempotency key regardless of age. A different pending grant
/// must be reconciled before a replacement request can be prepared.
pub fn prepare_reset_claim(
    grant_id: &str,
    pending: Option<&PendingResetClaim>,
    now: i64,
) -> Result<PendingResetClaim, CoreError> {
    if !ResetGrant::is_valid_id(grant_id) {
        return Err(CoreError::InvalidIdentifier);
    }
    if let Some(p) = pending {
        if p.request_id.is_empty()
            || p.request_id.len() > 64
            || !p
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(CoreError::InvalidIdentifier);
        }
        return if p.grant_id == grant_id {
            Ok(p.clone())
        } else {
            Err(CoreError::PendingClaim)
        };
    }
    Ok(PendingResetClaim {
        grant_id: grant_id.into(),
        request_id: uuid::Uuid::new_v4().to_string(),
        created_at: now,
    })
}
