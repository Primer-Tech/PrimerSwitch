import { localeName, t, type Language, type MessageKey } from './i18n';
import type { CodexError } from './codex-controller';
import type {
  CodexAccount,
  CodexLimit,
  CodexReason,
  CodexSnapshot,
  CodexWindow,
} from './codex-types';

const reasonKeys: Record<CodexReason, MessageKey> = {
  notInstalled: 'codexReason_notInstalled',
  unsupportedVersion: 'codexReason_unsupportedVersion',
  unsupportedStore: 'codexReason_unsupportedStore',
  unsupportedAuth: 'codexReason_unsupportedAuth',
  policyRestricted: 'codexReason_policyRestricted',
  externalCredentials: 'codexReason_externalCredentials',
  storeConflict: 'codexReason_storeConflict',
  vaultUnavailable: 'codexReason_vaultUnavailable',
  identityUnverified: 'codexReason_identityUnverified',
  identityMismatch: 'codexReason_identityMismatch',
  externalChange: 'codexReason_externalChange',
  loginExpired: 'codexReason_loginExpired',
  loginCanceled: 'codexReason_loginCanceled',
  providerUnavailable: 'codexReason_providerUnavailable',
  busy: 'codexReason_busy',
  daemonRestartFailed: 'codexReason_daemonRestartFailed',
  signInRequired: 'codexReason_signInRequired',
  switchInProgress: 'codexReason_switchInProgress',
  switcherUnavailable: 'codexReason_switcherUnavailable',
  activeAccount: 'codexReason_activeAccount',
};
/** Friendly, localized text for a native reason; unknown failures stay generic. */
export function codexMessage(
  reason: CodexError | null,
  locale: Language,
): string {
  if (reason === 'unsafeData') return t(locale, 'unsafeData');
  if (!reason || reason === 'actionFailed')
    return t(locale, 'codexActionFailed');
  return t(locale, reasonKeys[reason]);
}

const number = (value: number, locale: Language) =>
  new Intl.NumberFormat(localeName(locale)).format(value);
const unit = (
  value: number,
  name: 'minute' | 'hour' | 'day',
  locale: Language,
) =>
  new Intl.NumberFormat(localeName(locale), {
    style: 'unit',
    unit: name,
    unitDisplay: 'long',
  }).format(value);

/** A window label derived from its real duration ("5 hours", "Weekly"), never assumed. */
export function codexWindowLabel(
  minutes: number | null,
  slot: 'short' | 'long',
  locale: Language,
): string {
  if (minutes === null)
    return t(locale, slot === 'short' ? 'codexWindowShort' : 'codexWindowLong');
  if (minutes === 10080) return t(locale, 'weekly');
  if (minutes === 1440) return t(locale, 'codexWindowDaily');
  if (minutes % 1440 === 0) return unit(minutes / 1440, 'day', locale);
  if (minutes % 60 === 0) return unit(minutes / 60, 'hour', locale);
  return unit(minutes, 'minute', locale);
}

/** Elapsed time for live progress, "0:23". */
export function codexElapsed(seconds: number): string {
  const value = Math.max(0, Math.floor(seconds));
  return `${Math.floor(value / 60)}:${String(value % 60).padStart(2, '0')}`;
}

const plans: Record<string, string> = {
  plus: 'Plus',
  pro: 'Pro',
  prolite: 'Pro Lite',
  team: 'Team',
  business: 'Business',
  enterprise: 'Enterprise',
  edu: 'Edu',
};
/** Known ChatGPT plan names; unknown plan values are hidden rather than guessed. */
export function codexPlanLabel(
  planType: string | null,
  locale: Language,
): string | null {
  if (!planType) return null;
  const key = planType.toLowerCase().replace(/[^a-z]/g, '');
  if (key === 'free') return t(locale, 'codexPlanFree');
  return plans[key] ?? null;
}
export function codexAccountPlan(
  account: CodexAccount,
  locale: Language,
): string | null {
  return codexPlanLabel(
    account.planType ?? codexMainLimit(account)?.planType ?? null,
    locale,
  );
}

/** The main Codex limit is the one keyed `default`. */
export function codexMainLimit(account: CodexAccount): CodexLimit | null {
  return account.quota?.limits.find((limit) => limit.key === 'default') ?? null;
}
/** Additional limits, without the per-id copy of the main limit. */
export function codexExtraLimits(account: CodexAccount): CodexLimit[] {
  const limits = account.quota?.limits ?? [];
  const main = limits.find((limit) => limit.key === 'default');
  return limits.filter(
    (limit) =>
      limit.key !== 'default' &&
      !(main?.limitId && limit.limitId === main.limitId),
  );
}
export interface CodexWindowSlots {
  short: CodexWindow | null;
  long: CodexWindow | null;
}
/** Places a limit's windows by duration: up to a day is short, longer is long. */
export function codexWindows(limit: CodexLimit | null): CodexWindowSlots {
  const slots: CodexWindowSlots = { short: null, long: null };
  [limit?.primary ?? null, limit?.secondary ?? null].forEach(
    (window, index) => {
      if (!window) return;
      const long =
        window.windowDurationMins === null
          ? index === 1
          : window.windowDurationMins > 1440;
      const preferred = long ? 'long' : 'short';
      const other = long ? 'short' : 'long';
      if (!slots[preferred]) slots[preferred] = window;
      else if (!slots[other]) slots[other] = window;
    },
  );
  return slots;
}
export function codexLimitName(limit: CodexLimit, locale: Language): string {
  const raw = limit.limitName ?? limit.normalModelSlug ?? limit.limitId;
  if (!raw) return t(locale, 'codexOtherLimit');
  const words = raw.replace(/[-_]+/g, ' ').trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export interface CodexLimitState {
  limited: boolean;
  /** Latest reset among the exhausted windows; null when unknown. */
  freesAt: number | null;
}
/** Limited when a main window is exhausted, the provider reports a reached limit or usage is blocked. */
export function codexLimitState(account: CodexAccount): CodexLimitState {
  const quota = account.quota;
  if (!quota) return { limited: false, freesAt: null };
  const main = codexMainLimit(account);
  const exhausted = [main?.primary, main?.secondary].filter(
    (window): window is CodexWindow => !!window && window.usedPercent >= 100,
  );
  const limited =
    exhausted.length > 0 ||
    !!main?.rateLimitReachedType ||
    quota.ordinaryUsageAllowed === false;
  const resets = exhausted.map((window) => window.resetsAt);
  const freesAt =
    resets.length && resets.every((value) => value !== null)
      ? Math.max(...(resets as number[]))
      : null;
  return { limited, freesAt };
}

/** True when the account can be switched to right now, ignoring transient UI locks. */
export function codexCanSwitch(
  snapshot: CodexSnapshot,
  account: CodexAccount,
): boolean {
  return (
    snapshot.capabilities.switchAccount.enabled &&
    account.switchable.enabled &&
    !account.selected &&
    !account.needsSignIn
  );
}

/** The main limit has purchased credits: the flag, a balance or unlimited credits. */
function codexHasCredits(account: CodexAccount): boolean {
  const credits = codexMainLimit(account)?.credits;
  return (
    !!credits && (credits.hasCredits || credits.unlimited || !!credits.balance)
  );
}
/**
 * A main window past 100% while the main limit has credits: Codex keeps working on
 * purchased credits. The real percentage stays visible next to this hint.
 */
export function codexUsingCredits(
  account: CodexAccount,
  window: CodexWindow | null,
): boolean {
  return !!window && window.usedPercent > 100 && codexHasCredits(account);
}

/** Credits are shown only when the main limit reports some. */
export function codexCredits(
  account: CodexAccount,
  locale: Language,
): string | null {
  const credits = codexMainLimit(account)?.credits;
  if (!credits) return null;
  if (credits.unlimited) return t(locale, 'codexCreditsUnlimited');
  if (credits.balance) {
    const value = Number(credits.balance);
    if (!Number.isFinite(value)) return credits.balance;
    const decimals = Math.min(4, (credits.balance.split('.')[1] ?? '').length);
    return new Intl.NumberFormat(localeName(locale), {
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    }).format(value);
  }
  return credits.hasCredits ? t(locale, 'codexCreditsAvailable') : null;
}

/** "studio@example.invalid" is shown once when the name is the email. */
export function codexSecondaryLine(account: CodexAccount): string {
  return [
    account.email && account.email !== account.name ? account.email : null,
    account.workspaceName,
  ]
    .filter(Boolean)
    .join(' · ');
}
export function codexInitial(account: CodexAccount): string {
  return (account.name.trim().charAt(0) || '?').toUpperCase();
}
export function codexLastRead(snapshot: CodexSnapshot | null): number | null {
  const reads = (snapshot?.accounts ?? [])
    .map((account) => account.quotaReadAt)
    .filter((value): value is number => value !== null);
  return reads.length ? Math.max(...reads) : null;
}
export function codexImportedMessage(
  source: 'current' | 'switcher',
  added: number,
  locale: Language,
): string {
  if (source === 'current')
    return t(
      locale,
      added ? 'codexImportedCurrent' : 'codexImportedCurrentExisting',
    );
  if (!added) return t(locale, 'codexImportedSwitcherNone');
  const category = new Intl.PluralRules(localeName(locale)).select(added);
  return t(
    locale,
    category === 'one'
      ? 'codexImportedSwitcherOne'
      : category === 'few'
        ? 'codexImportedSwitcherFew'
        : 'codexImportedSwitcherOther',
    { count: number(added, locale) },
  );
}
