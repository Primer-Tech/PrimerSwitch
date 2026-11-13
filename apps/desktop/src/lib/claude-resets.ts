// Pure view-model of an account's banked Claude resets: one row per kind, with the
// words for what it does, when to use it by and why it cannot be used right now.
import { localeName, t, type Language, type MessageKey } from './i18n';
import { duration } from './format';
import type { AccountView, ResetKind, ResetOffer, ResetReason } from './types';

const kindKeys: Record<ResetKind, MessageKey> = {
  session: 'resetKind_session',
  weekly: 'resetKind_weekly',
  fable: 'resetKind_fable',
  full: 'resetKind_full',
};
const effectKeys: Record<ResetKind, MessageKey> = {
  session: 'resetEffect_session',
  weekly: 'resetEffect_weekly',
  fable: 'resetEffect_fable',
  full: 'resetEffect_full',
};
const reasonKeys: Record<ResetReason, MessageKey> = {
  cooldown: 'resetReason_cooldown',
  blockedFiveHour: 'resetReason_blockedFiveHour',
  blockedSevenDay: 'resetReason_blockedSevenDay',
  blockedOpus: 'resetReason_blockedOpus',
  blockedSonnet: 'resetReason_blockedSonnet',
  blockedOther: 'resetReason_blockedOther',
  cannotClear: 'resetReason_cannotClear',
  needsLimit: 'resetReason_needsLimit',
  unavailable: 'resetReason_unavailable',
  pendingClaim: 'resetReason_pendingClaim',
};

export function resetKindLabel(kind: ResetKind, locale: Language): string {
  return t(locale, kindKeys[kind]);
}
/** Local date and time: a reset can end within hours, so the day alone is not enough. */
export function resetDeadline(timestamp: number, locale: Language): string {
  return new Intl.DateTimeFormat(localeName(locale), {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(timestamp * 1000);
}

/** One kind of reset the account holds, in the words the page shows. */
export interface ResetRowView {
  kind: ResetKind;
  /** "Fable reset", "Full reset", "5-hour reset", "Weekly reset". */
  label: string;
  /** What using it does; Fable's returns what Fable used to the weekly limit. */
  effect: string;
  /** "2 left". */
  left: string;
  resetsLeft: number;
  /** "Use by 20 Nov 2026, 14:00", or that it does not expire. */
  useBy: string;
  endsAt: number | null;
  /** The formatted end, for the confirmation; null when it does not expire. */
  deadline: string | null;
  /** The Use button can be pressed (the page may still lock it while it is busy). */
  canUse: boolean;
  /** Why it cannot be used now; null while it can. */
  reason: string | null;
  /** An early use: the question to confirm first, with the share of the limit left. */
  early: { percentLeft: number | null; text: string } | null;
  /** A previous request was not confirmed: pressing repeats that same request. */
  retry: boolean;
}
export interface ResetsView {
  rows: ResetRowView[];
  /** The count across all kinds; null while the reading has no reset block. */
  total: number | null;
  /** A request was sent and its answer has not been confirmed. */
  pending: boolean;
  cooldownUntil: number | null;
  /** The runtime's own words for the last attempt. */
  lastOutcome: string | null;
  /** The rows that can be used now, in display order. */
  usable: ResetRowView[];
  /** The one usable kind repeats an unconfirmed request. */
  retry: boolean;
  /**
   * While nothing can be used: the reason every kind gives, when it is the same one;
   * null when the reasons differ or there is no kind at all.
   */
  sharedReason: string | null;
}

function reasonText(
  reason: ResetReason,
  account: AccountView,
  now: number,
  locale: Language,
): string {
  const until = account.resets.cooldownUntil;
  if (reason === 'cooldown' && until !== null && until > now)
    return t(locale, 'resetCooldown', {
      duration: duration(until, now, locale),
    });
  return t(locale, reasonKeys[reason]);
}
function earlyText(percentLeft: number | null, locale: Language): string {
  return percentLeft === null
    ? t(locale, 'resetEarlyWarningUnknown')
    : t(locale, 'resetEarlyWarning', { percent: percentLeft });
}
function row(
  offer: ResetOffer,
  account: AccountView,
  now: number,
  locale: Language,
): ResetRowView {
  const retry = account.resets.pending && offer.usableNow;
  return {
    kind: offer.kind,
    label: t(locale, kindKeys[offer.kind]),
    effect: t(locale, effectKeys[offer.kind]),
    left: t(locale, 'resetsLeft', { count: offer.resetsLeft }),
    resetsLeft: offer.resetsLeft,
    useBy:
      offer.endsAt === null
        ? t(locale, 'resetNoExpiry')
        : t(locale, 'resetUseBy', {
            date: resetDeadline(offer.endsAt, locale),
          }),
    endsAt: offer.endsAt,
    deadline:
      offer.endsAt === null ? null : resetDeadline(offer.endsAt, locale),
    canUse: offer.usableNow,
    reason: offer.usableNow
      ? null
      : reasonText(
          offer.unavailableReason ?? 'unavailable',
          account,
          now,
          locale,
        ),
    early:
      offer.usableNow && offer.earlyUse
        ? {
            percentLeft: offer.earlyUse.percentLeft,
            text: earlyText(offer.earlyUse.percentLeft, locale),
          }
        : null,
    retry,
  };
}
export function resetsView(
  account: AccountView,
  now: number,
  locale: Language,
): ResetsView {
  const rows = account.resets.offers.map((offer) =>
    row(offer, account, now, locale),
  );
  const usable = rows.filter((r) => r.canUse);
  const reasons = new Set(rows.map((r) => r.reason));
  return {
    rows,
    total: account.resets.available,
    pending: account.resets.pending,
    cooldownUntil: account.resets.cooldownUntil,
    lastOutcome: account.resets.lastOutcome,
    usable,
    retry: usable.length === 1 && usable[0].retry,
    sharedReason:
      usable.length === 0 && rows.length > 0 && reasons.size === 1
        ? rows[0].reason
        : null,
  };
}
