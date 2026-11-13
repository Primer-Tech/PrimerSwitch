//! Manual use of banked Claude resets, by kind. Pure functions over the saved reset
//! status: no I/O and no clock (the caller passes `now`). The kind classification, the
//! held/usable tests and the early-use rule mirror what Claude Code itself applies to
//! the same server data, so PrimerSwitch and the CLI agree on what can be used.
//!
//! None of this is automatic policy: `decide_resets` is unchanged and never calls it.
use crate::{ResetGrant, ResetStatus};
use serde::{Deserialize, Serialize};

/// Windows a reset can clear, or be blocked by. Other names are not understood and are
/// ignored wherever a kind or a cleared window is decided.
pub const RESET_WINDOWS: [&str; 8] = [
    "five_hour",
    "seven_day",
    "seven_day_overage_included",
    "seven_day_opus",
    "seven_day_sonnet",
    "seven_day_cowork",
    "seven_day_omelette",
    "seven_day_oauth_apps",
];
pub fn is_reset_window(name: &str) -> bool {
    RESET_WINDOWS.contains(&name)
}
/// A replayed claim waits at least this long after the previous attempt (B19).
pub const CLAIM_REPLAY_FLOOR: i64 = 300;

/// The four kinds of banked reset, in the order Claude Code lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResetKind {
    /// The five-hour window.
    Session,
    Weekly,
    /// Returns what Fable used to the weekly limit.
    Fable,
    Full,
}
impl ResetKind {
    pub const ALL: [Self; 4] = [Self::Session, Self::Weekly, Self::Fable, Self::Full];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Weekly => "weekly",
            Self::Fable => "fable",
            Self::Full => "full",
        }
    }
    /// The server's `type` field names only these three kinds; Fable is derived from
    /// `gives_back_weekly`, never from a type.
    pub fn parse_wire_type(value: &str) -> Option<Self> {
        match value {
            "session" => Some(Self::Session),
            "weekly" => Some(Self::Weekly),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// Why the server (or this client) says a grant cannot be used now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetUnavailable {
    Cooldown,
    BlockedFiveHour,
    BlockedSevenDay,
    BlockedOpus,
    BlockedSonnet,
    BlockedOther,
    CannotClear,
    NeedsLimit,
    Unavailable,
}
impl ResetUnavailable {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "cooldown" => Self::Cooldown,
            "blocked_five_hour" => Self::BlockedFiveHour,
            "blocked_seven_day" => Self::BlockedSevenDay,
            "blocked_opus" => Self::BlockedOpus,
            "blocked_sonnet" => Self::BlockedSonnet,
            "blocked_other" => Self::BlockedOther,
            "cannot_clear" => Self::CannotClear,
            "needs_limit" => Self::NeedsLimit,
            "unavailable" => Self::Unavailable,
            _ => return None,
        })
    }
}

impl ResetGrant {
    /// Claude Code's classification, in its order: a grant that gives back the weekly
    /// limit is a Fable reset; otherwise the server's own `type`; otherwise the windows
    /// it clears (only the five-hour window: session; only other windows: weekly;
    /// both or none: full). Windows that are not understood are ignored.
    pub fn kind(&self) -> ResetKind {
        if self.gives_back_weekly {
            return ResetKind::Fable;
        }
        if let Some(kind) = self.grant_type {
            return kind;
        }
        let five_hour = self.clears.iter().any(|w| w == "five_hour");
        let other = self
            .clears
            .iter()
            .any(|w| w != "five_hour" && is_reset_window(w));
        match (five_hour, other) {
            (true, false) => ResetKind::Session,
            (false, true) => ResetKind::Weekly,
            _ => ResetKind::Full,
        }
    }
    /// Whether the grant lists `window` among the windows it clears.
    fn lists_window(&self, window: &str) -> bool {
        is_reset_window(window) && self.clears.iter().any(|w| w == window)
    }
    /// Whether the grant clears `window`. A grant that gives back the weekly limit also
    /// clears `seven_day`, as Claude Code treats it.
    pub fn clears_window(&self, window: &str) -> bool {
        self.lists_window(window) || (window == "seven_day" && self.gives_back_weekly)
    }
    /// Held: unpaused with uses left, or usable right now (Claude Code's own test).
    pub fn is_held(&self) -> bool {
        !self.paused && (self.resets_left > 0 || self.usable_now)
    }
}

/// How much of the limit an early use would waste. `percent_left` is `None` when the
/// server did not say, or the limit has no plain name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarlyUse {
    pub percent_left: Option<u8>,
}

/// What a manual claim may do with a usable grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManualGate {
    /// The account is at a limit this grant clears.
    Ready,
    /// The grant may be used without a limit, but the account is not at one.
    Early(EarlyUse),
    /// The grant refills only a limit the account has not reached.
    NeedsLimit,
}

/// Why a manual claim is refused without sending anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualRefusal {
    /// No grant of this kind can be used now.
    NotUsable,
    /// The grant refills a limit that has not been reached.
    NeedsLimit,
    /// An early use needs the user's explicit confirmation first.
    EarlyUseNeedsConfirmation,
}

/// The grant a manual claim would send, and whether it is an early use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualClaim {
    pub grant_id: String,
    pub early: Option<EarlyUse>,
}

/// One kind of reset the account holds, as the user sees it. No grant id leaves the
/// core through the view; `grant_id` is the grant a claim of this kind would use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetOffer {
    pub kind: ResetKind,
    pub resets_left: u32,
    pub resets_total: u32,
    /// The earliest end among the held grants of this kind.
    pub ends_at: Option<i64>,
    pub grant_id: Option<String>,
    pub usable_now: bool,
    pub unavailable: Option<ResetUnavailable>,
    pub early_use: Option<EarlyUse>,
}

fn is_live(grant: &ResetGrant, now: i64) -> bool {
    grant.is_held() && grant.ends_at.is_none_or(|end| end > now)
}
fn can_use_now(status: &ResetStatus, grant: &ResetGrant, now: i64) -> bool {
    ResetGrant::is_valid_id(&grant.id)
        && grant.usable_now
        && !grant.paused
        && grant.resets_left > 0
        && grant.starts_at.is_none_or(|start| start <= now)
        && grant.ends_at.is_none_or(|end| end > now)
        && status.eligible != Some(false)
        && status.cooldown_until.is_none_or(|until| until <= now)
}
/// The grants of `kind` that count as held, in server order.
fn held_of_kind(status: &ResetStatus, kind: ResetKind, now: i64) -> Vec<&ResetGrant> {
    status
        .grants
        .iter()
        .filter(|g| g.kind() == kind && is_live(g, now))
        .collect()
}
fn pick<'a>(status: &ResetStatus, held: &[&'a ResetGrant], now: i64) -> Option<&'a ResetGrant> {
    let usable: Vec<&ResetGrant> = held
        .iter()
        .copied()
        .filter(|g| can_use_now(status, g, now))
        .collect();
    status
        .next_grant_id
        .as_deref()
        .and_then(|next| usable.iter().copied().find(|g| g.id == next))
        .or_else(|| usable.first().copied())
}
/// The grant a manual claim of `kind` would use: the first usable one in server order,
/// preferring the server's `next_grant_id` when it is among them.
pub fn select_manual_grant(status: &ResetStatus, kind: ResetKind, now: i64) -> Option<&ResetGrant> {
    pick(status, &held_of_kind(status, kind, now), now)
}
/// The kind of the grant `id`, when the status lists it.
pub fn grant_kind(status: &ResetStatus, id: &str) -> Option<ResetKind> {
    status
        .grants
        .iter()
        .find(|g| g.id == id)
        .map(ResetGrant::kind)
}

/// The share of the limit left when a grant is used early: 100 minus the highest
/// percent used among the windows it lists, as Claude Code words it.
pub fn early_use(grant: &ResetGrant) -> EarlyUse {
    let mut best: Option<(&str, u8)> = None;
    for window in RESET_WINDOWS {
        if grant.lists_window(window)
            && let Some(&used) = grant.percent_used.get(window)
            && best.is_none_or(|(_, top)| used > top)
        {
            best = Some((window, used));
        }
    }
    let named = |window: &str| {
        matches!(
            window,
            "five_hour"
                | "seven_day"
                | "seven_day_opus"
                | "seven_day_overage_included"
                | "seven_day_sonnet"
        )
    };
    EarlyUse {
        percent_left: best
            .filter(|(window, used)| *used < 100 && named(window))
            .map(|(_, used)| 100 - used),
    }
}
/// Whether using `grant` now needs a limit it has not reached, or would be early.
pub fn manual_gate(status: &ResetStatus, grant: &ResetGrant) -> ManualGate {
    if status.exhausted.iter().any(|w| grant.clears_window(w)) {
        ManualGate::Ready
    } else if grant.use_requires_limit {
        ManualGate::NeedsLimit
    } else {
        ManualGate::Early(early_use(grant))
    }
}
/// Plans a manual claim of `kind`. Refuses (sending nothing) when no grant of the kind
/// is usable, when the grant needs a limit the account has not reached, or when it is
/// an early use and `allow_early` is false.
pub fn plan_manual_claim(
    status: &ResetStatus,
    kind: ResetKind,
    now: i64,
    allow_early: bool,
) -> Result<ManualClaim, ManualRefusal> {
    let grant = select_manual_grant(status, kind, now).ok_or(ManualRefusal::NotUsable)?;
    match manual_gate(status, grant) {
        ManualGate::Ready => Ok(ManualClaim {
            grant_id: grant.id.clone(),
            early: None,
        }),
        ManualGate::NeedsLimit => Err(ManualRefusal::NeedsLimit),
        ManualGate::Early(early) if allow_early => Ok(ManualClaim {
            grant_id: grant.id.clone(),
            early: Some(early),
        }),
        ManualGate::Early(_) => Err(ManualRefusal::EarlyUseNeedsConfirmation),
    }
}

/// Why a held grant cannot be used, in Claude Code's order: the server's own reason,
/// then a cooldown, then the limit that blocks it, then a limit it cannot clear.
pub fn unavailable_reason(status: &ResetStatus, grant: &ResetGrant, now: i64) -> ResetUnavailable {
    if let Some(reason) = grant.unavailable_reason {
        return reason;
    }
    if status.eligible == Some(false) {
        return ResetUnavailable::Unavailable;
    }
    if status.cooldown_until.is_some_and(|until| until > now) {
        return ResetUnavailable::Cooldown;
    }
    for window in &grant.blocking {
        match window.as_str() {
            "five_hour" => return ResetUnavailable::BlockedFiveHour,
            "seven_day" => return ResetUnavailable::BlockedSevenDay,
            "seven_day_opus" => return ResetUnavailable::BlockedOpus,
            "seven_day_sonnet" => return ResetUnavailable::BlockedSonnet,
            _ => {}
        }
    }
    if !grant.blocking.is_empty() {
        return ResetUnavailable::BlockedOther;
    }
    if status.at_limit && !status.exhausted.iter().any(|w| grant.clears_window(w)) {
        return ResetUnavailable::CannotClear;
    }
    if grant.use_requires_limit {
        ResetUnavailable::NeedsLimit
    } else {
        ResetUnavailable::Unavailable
    }
}

/// One offer per kind the account holds, in display order. A claim of the offered kind
/// is possible exactly when `usable_now`; `early_use` marks one that needs confirming.
pub fn reset_offers(status: &ResetStatus, now: i64) -> Vec<ResetOffer> {
    let mut offers = vec![];
    for kind in ResetKind::ALL {
        let held = held_of_kind(status, kind, now);
        let Some(first) = held.first() else {
            continue;
        };
        let selected = pick(status, &held, now);
        let (usable_now, unavailable, early) = match selected {
            Some(grant) => match manual_gate(status, grant) {
                ManualGate::Ready => (true, None, None),
                ManualGate::Early(early) => (true, None, Some(early)),
                ManualGate::NeedsLimit => (false, Some(ResetUnavailable::NeedsLimit), None),
            },
            None => (false, Some(unavailable_reason(status, first, now)), None),
        };
        offers.push(ResetOffer {
            kind,
            resets_left: held
                .iter()
                .map(|g| g.resets_left)
                .fold(0, u32::saturating_add),
            resets_total: held
                .iter()
                .map(|g| g.resets_total)
                .fold(0, u32::saturating_add),
            ends_at: held.iter().filter_map(|g| g.ends_at).min(),
            grant_id: selected.map(|g| g.id.clone()),
            usable_now,
            unavailable,
            early_use: early,
        });
    }
    offers
}
