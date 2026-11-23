use crate::{AccountUsageInfo, CoreError, PendingResetClaim, ResetGrant, UsageResponse};
use std::cmp::Ordering;

/// A target has preferred headroom when both binding windows are at most
/// `threshold - HEADROOM_MARGIN` (B13).
pub const HEADROOM_MARGIN: f64 = 5.0;
/// Weekly resets compare by UTC hour, so resets within the same hour tie (B13).
pub fn reset_hour_bucket(at: i64) -> i64 {
    at.div_euclid(3600) * 3600
}
/// Which usable account goes first once preferred headroom is equal. Shared by Claude
/// and Codex; `Settings::prefer_soonest_weekly_reset` selects it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CandidateOrder {
    /// The original order (B13): the earliest weekly reset first, which spends quota
    /// that would otherwise expire unused, then the lower five-hour usage.
    #[default]
    SoonestWeeklyReset,
    /// The most weekly usage left first, then the earliest weekly reset, then the lower
    /// five-hour usage.
    MostWeeklyLeft,
}
impl CandidateOrder {
    pub fn from_preference(prefer_soonest_weekly_reset: bool) -> Self {
        if prefer_soonest_weekly_reset {
            Self::SoonestWeeklyReset
        } else {
            Self::MostWeeklyLeft
        }
    }
}
/// Provider-neutral ranking inputs of one usable candidate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidateRank<'a> {
    pub id: &'a str,
    /// Both binding windows are at most `threshold - HEADROOM_MARGIN`.
    pub headroom: bool,
    /// Explicit ascending priority, with unranked accounts last.
    pub priority: Option<u16>,
    /// Binding weekly usage in percent.
    pub weekly_used: f64,
    /// Weekly reset hour bucket; an unknown reset sorts last.
    pub weekly_reset_bucket: Option<i64>,
    pub five_hour_used: f64,
}
/// Total order over usable candidates: preferred headroom first, then `order`, then
/// the lower five-hour usage and finally the stable id.
pub fn compare_candidates(
    a: &CandidateRank<'_>,
    b: &CandidateRank<'_>,
    order: CandidateOrder,
) -> Ordering {
    let reset = || {
        a.weekly_reset_bucket
            .unwrap_or(i64::MAX)
            .cmp(&b.weekly_reset_bucket.unwrap_or(i64::MAX))
    };
    b.headroom
        .cmp(&a.headroom)
        .then_with(|| {
            a.priority
                .unwrap_or(u16::MAX)
                .cmp(&b.priority.unwrap_or(u16::MAX))
        })
        .then_with(|| match order {
            CandidateOrder::SoonestWeeklyReset => reset(),
            CandidateOrder::MostWeeklyLeft => {
                a.weekly_used.total_cmp(&b.weekly_used).then_with(reset)
            }
        })
        .then_with(|| a.five_hour_used.total_cmp(&b.five_hour_used))
        .then_with(|| a.id.cmp(b.id))
}

pub fn is_usable(info: &AccountUsageInfo, threshold: f64, model: Option<&str>) -> bool {
    let threshold = info.account.automation.effective_threshold(threshold);
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
            let threshold = info.account.automation.effective_threshold(threshold);
            u.five_hour.utilization <= threshold - margin
                && u.weekly_utilization_for_model(model) <= threshold - margin
        })
}
fn rank<'a>(info: &'a AccountUsageInfo, threshold: f64, model: Option<&str>) -> CandidateRank<'a> {
    let usage = info.usage.as_ref().expect("usable account has usage");
    CandidateRank {
        id: &info.account.id,
        headroom: has_headroom(info, threshold, model, HEADROOM_MARGIN),
        priority: info.account.automation.priority,
        weekly_used: usage.weekly_utilization_for_model(model),
        weekly_reset_bucket: usage.weekly_reset_bucket(),
        five_hour_used: usage.five_hour.utilization,
    }
}
fn compare(
    a: &AccountUsageInfo,
    b: &AccountUsageInfo,
    threshold: f64,
    model: Option<&str>,
    order: CandidateOrder,
) -> Ordering {
    compare_candidates(
        &rank(a, threshold, model),
        &rank(b, threshold, model),
        order,
    )
}
pub fn best_candidate(
    infos: &[AccountUsageInfo],
    threshold: f64,
    model: Option<&str>,
    order: CandidateOrder,
) -> Option<String> {
    infos
        .iter()
        .filter(|i| !i.is_active && i.account.automation.enabled && is_usable(i, threshold, model))
        .min_by(|a, b| compare(a, b, threshold, model, order))
        .map(|i| i.account.id.clone())
}
pub fn consumption_plan(
    infos: &[AccountUsageInfo],
    threshold: f64,
    model: Option<&str>,
    order: CandidateOrder,
) -> Vec<String> {
    let mut sorted: Vec<_> = infos
        .iter()
        .filter(|i| i.account.automation.enabled && is_usable(i, threshold, model))
        .collect();
    sorted.sort_by(|a, b| compare(a, b, threshold, model, order));
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
