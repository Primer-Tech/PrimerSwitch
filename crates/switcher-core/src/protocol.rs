use crate::{ResetStatus, UsageResponse, UsageWindow, parse_timestamp};
use serde_json::Value;
use std::collections::BTreeMap;

/// Round header fractions to one hundredth of a percentage point.
pub fn fraction_to_percent(fraction: f64) -> f64 {
    (fraction * 10000.0).round() / 100.0
}
fn normalized(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect()
}
fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .get(&format!("anthropic-ratelimit-unified-{name}"))
        .map(String::as_str)
}
fn percent(headers: &BTreeMap<String, String>, window: &str) -> Option<f64> {
    header(headers, &format!("{window}-utilization"))?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(fraction_to_percent)
        .filter(|n| n.is_finite())
}
fn reset(headers: &BTreeMap<String, String>, window: &str) -> Option<i64> {
    header(headers, &format!("{window}-reset"))
        .and_then(|s| parse_timestamp(&Value::String(s.into())))
}
pub fn usage_from_headers(headers: &BTreeMap<String, String>) -> Option<UsageResponse> {
    let headers = normalized(headers);
    Some(UsageResponse {
        five_hour: UsageWindow {
            utilization: percent(&headers, "5h")?,
            resets_at: reset(&headers, "5h"),
        },
        seven_day: UsageWindow {
            utilization: percent(&headers, "7d")?,
            resets_at: reset(&headers, "7d"),
        },
        ..UsageResponse::default()
    })
}
pub fn usage_from_limit_refusal(
    headers: &BTreeMap<String, String>,
    previous: Option<&UsageResponse>,
) -> Option<UsageResponse> {
    let h = normalized(headers);
    let rejected = header(&h, "status").is_some_and(|s| s.eq_ignore_ascii_case("rejected"));
    let claim = header(&h, "representative-claim").map(str::to_ascii_lowercase);
    let five = percent(&h, "5h");
    let seven = percent(&h, "7d");
    let five_spent =
        (rejected && claim.as_deref() == Some("five_hour")) || five.is_some_and(|n| n >= 100.0);
    let seven_spent =
        (rejected && claim.as_deref() == Some("seven_day")) || seven.is_some_and(|n| n >= 100.0);
    if !five_spent && !seven_spent {
        return None;
    }
    let claim_reset = header(&h, "reset").and_then(|s| parse_timestamp(&Value::String(s.into())));
    let window = |spent: bool,
                  pct: Option<f64>,
                  resets: Option<i64>,
                  old: Option<&UsageWindow>|
     -> Option<UsageWindow> {
        // Missing unspent windows remain unknown: without old evidence there is no
        // complete snapshot. Never manufacture a zero that permits a switch.
        let utilization = if spent {
            pct.unwrap_or(100.0).max(100.0)
        } else {
            pct.or_else(|| old.map(|w| w.utilization))?
        };
        Some(UsageWindow {
            utilization,
            resets_at: resets
                .or(if spent { claim_reset } else { None })
                .or_else(|| old.and_then(|w| w.resets_at)),
        })
    };
    Some(UsageResponse {
        five_hour: window(
            five_spent,
            five,
            reset(&h, "5h"),
            previous.map(|u| &u.five_hour),
        )?,
        seven_day: window(
            seven_spent,
            seven,
            reset(&h, "7d"),
            previous.map(|u| &u.seven_day),
        )?,
        ..UsageResponse::default()
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResetOutcome {
    Reset {
        resets_left: Option<u32>,
        cooldown_until: Option<i64>,
    },
    AlreadyUsed,
    NotLimited,
    Cooldown {
        until: Option<i64>,
    },
    Ineligible {
        reason: Option<String>,
    },
    Unavailable {
        reason: Option<String>,
    },
    Transport,
    AuthError,
}
impl ResetOutcome {
    pub fn text_for(&self, locale: &str) -> &'static str {
        if locale == "ro" {
            return self.text();
        }
        match self {
            Self::Reset { .. } => "reset used — limits refilled",
            Self::AlreadyUsed => "reset already used",
            Self::NotLimited => "reset unused: account is not at a limit",
            Self::Cooldown { .. } => "reset on cooldown",
            Self::Ineligible { .. } => "reset refused: account is ineligible",
            Self::Unavailable { .. } => "reset unavailable",
            Self::Transport => "reset response unconfirmed — retry pending",
            Self::AuthError => "reset token rejected",
        }
    }
    pub fn is_final(&self) -> bool {
        !matches!(self, Self::Transport)
    }
    /// Human-safe text does not interpolate unauthenticated provider reason strings.
    pub fn text(&self) -> &'static str {
        match self {
            Self::Reset { .. } => "reset folosit — limitele umplute",
            Self::AlreadyUsed => "reset deja folosit",
            Self::NotLimited => "reset nefolosit: contul nu e la limită",
            Self::Cooldown { .. } => "reset în pauză (cooldown)",
            Self::Ineligible { .. } => "reset refuzat: neeligibil",
            Self::Unavailable { .. } => "reset indisponibil",
            Self::Transport => "reset: răspuns neconfirmat — reîncercăm",
            Self::AuthError => "reset: token respins",
        }
    }
}
pub fn parse_reset_outcome(status: u16, body: &Value) -> ResetOutcome {
    if status == 401 || status == 403 {
        return ResetOutcome::AuthError;
    }
    if status == 429 || status >= 500 {
        return ResetOutcome::Transport;
    }
    let Some(result) = body.get("result").and_then(Value::as_str) else {
        return ResetOutcome::Transport;
    };
    let cooldown = body.get("cooldown_until").and_then(parse_timestamp);
    // Raw provider reasons can contain arbitrary response data. Retain only a small
    // set of protocol classifications; the user-facing text is always fixed.
    let reason = body
        .get("reason")
        .and_then(Value::as_str)
        .filter(|s| {
            matches!(
                *s,
                "not_eligible" | "expired" | "paused" | "disabled" | "no_grants" | "not_available"
            )
        })
        .map(str::to_owned);
    match result {
        "reset" => ResetOutcome::Reset {
            resets_left: body
                .get("resets_left")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            cooldown_until: cooldown,
        },
        "already_used" => ResetOutcome::AlreadyUsed,
        "not_limited" => ResetOutcome::NotLimited,
        "cooldown" => ResetOutcome::Cooldown { until: cooldown },
        "ineligible" => ResetOutcome::Ineligible { reason },
        _ => ResetOutcome::Unavailable { reason },
    }
}
pub fn apply_reset_outcome(
    status: &mut Option<ResetStatus>,
    grant_id: &str,
    outcome: &ResetOutcome,
) {
    let Some(status) = status else {
        return;
    };
    match outcome {
        ResetOutcome::Reset {
            resets_left,
            cooldown_until,
        } => {
            if let Some(g) = status.grants.iter_mut().find(|g| g.id == grant_id) {
                g.resets_left = resets_left.unwrap_or(g.resets_left.saturating_sub(1));
            }
            status.grants.retain(|g| g.resets_left > 0);
            if status.next_grant_id.as_deref() == Some(grant_id)
                && !status.grants.iter().any(|g| g.id == grant_id)
            {
                status.next_grant_id = None;
            }
            if cooldown_until.is_some() {
                status.cooldown_until = *cooldown_until;
            }
        }
        ResetOutcome::AlreadyUsed => {
            status.grants.retain(|g| g.id != grant_id);
            if status.next_grant_id.as_deref() == Some(grant_id) {
                status.next_grant_id = None;
            }
        }
        ResetOutcome::Cooldown { until: Some(until) } => status.cooldown_until = Some(*until),
        _ => {}
    }
}
