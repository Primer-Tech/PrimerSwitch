// Shared view-models for the provider page. Claude and Codex snapshots are mapped into
// these shapes by pure adapters (claude-view.ts, codex-view.ts), so both tabs render
// through the same components with the same vocabulary.
import { t, type Language, type MessageKey } from './i18n';
import { age, duration, percentage } from './format';
import type { Settings } from './types';

export type ProviderKey = 'claude' | 'codex';

/** One usage window: a primary meter or an extra row. */
export interface MeterView {
  key: string;
  /** Visible label: "5-hour window", "Weekly", "Weekly · Sonnet", "Weekly · Code review". */
  label: string;
  /** Percent used; Codex reports more than 100 while it runs on purchased credits. */
  value: number | null;
  resetsAt: number | null;
  /** A short visible hint under the bar, for example "Using credits". */
  note: string | null;
  /** A tiny tag beside the value, for example the model whose window binds. */
  tag: string | null;
  tone: 'blue' | 'green' | 'violet';
}

export type StatusKind =
  | 'active'
  | 'next'
  | 'ready'
  | 'near'
  | 'limited'
  | 'credits'
  | 'signIn'
  | 'checking'
  | 'unread'
  | 'failed'
  | 'apiKey'
  | 'unsupported';

/**
 * How far an account is from usable: `limited` and `credits` are real provider
 * limits (100% used, a reached-limit flag or blocked usage); `near` is at or over
 * the switch threshold without one. Automation skips all three alike.
 */
export type LimitKind = 'limited' | 'credits' | 'near';

/** One status, in the words both providers share. */
export interface StatusView {
  kind: StatusKind;
  label: string;
  detail: string | null;
  /** A longer explanation for a tooltip: why a check failed or switching is blocked. */
  title: string | null;
}

/** Inputs of a status; `statusView` turns them into the shared words. */
export interface StatusInput {
  kind: StatusKind;
  /** The active account is itself limited, on purchased credits or near its limit. */
  limit?: LimitKind | null;
  /** When the windows at or over the switch threshold reset; null when unknown. */
  freesAt?: number | null;
  /** Why the last check failed; the row keeps its last reading. */
  failure?: string | null;
  /** Why switching to this account is blocked. */
  blocked?: string | null;
  /** Claude: banked resets that can reopen a limited account. */
  resets?: { available: number | null; usableAt: number | null } | null;
  /** The reading is a saved one, not proven current: its time. */
  savedAt?: number | null;
}

const labels: Record<StatusKind, MessageKey> = {
  active: 'active',
  next: 'next',
  ready: 'statusReady',
  near: 'statusNearLimit',
  limited: 'statusLimited',
  credits: 'usingCredits',
  signIn: 'signInAgain',
  checking: 'statusChecking',
  unread: 'statusUnread',
  failed: 'statusFailed',
  apiKey: 'statusApiKey',
  unsupported: 'statusUnsupported',
};
const limitLabels: Record<LimitKind, MessageKey> = {
  limited: 'statusLimited',
  credits: 'usingCredits',
  near: 'statusNearLimit',
};

/**
 * When a limit lifts: a real limit "Frees in 2h 10m", an account near its limit
 * "Resets in 2h 10m"; "Likely free again · refresh to confirm" once that time has
 * passed, or nothing when it is unknown.
 */
function freesText(
  limit: LimitKind,
  freesAt: number | null | undefined,
  now: number,
  locale: Language,
): string | null {
  if (freesAt === null || freesAt === undefined) return null;
  if (freesAt <= now) return t(locale, 'statusFreeAgain');
  const left = duration(freesAt, now, locale);
  return t(locale, limit === 'near' ? 'resetIn' : 'freesIn', {
    duration: left,
  });
}
/** "Limit reached · frees in 2h 10m", "Near limit · resets in 2h 10m". */
function limitWithTime(
  label: string,
  limit: LimitKind,
  freesAt: number | null | undefined,
  now: number,
  locale: Language,
): string {
  return freesAt !== null && freesAt !== undefined && freesAt > now
    ? t(
        locale,
        limit === 'near' ? 'statusLimitResetsIn' : 'statusLimitFreesIn',
        {
          limit: label,
          duration: duration(freesAt, now, locale),
        },
      )
    : label;
}

/** The shared status words for one account, at `now`. */
export function statusView(
  input: StatusInput,
  now: number,
  locale: Language,
): StatusView {
  const { kind } = input;
  const parts: string[] = [];
  let title: string | null = null;
  if (kind === 'apiKey') parts.push(t(locale, 'statusApiKeyDetail'));
  if (kind === 'failed') {
    parts.push(t(locale, 'statusFailedDetail'));
    title = input.failure ?? null;
  }
  if (kind === 'limited' || kind === 'credits' || kind === 'near') {
    const frees = freesText(kind, input.freesAt, now, locale);
    if (frees) parts.push(frees);
  }
  if (kind === 'active' && input.limit)
    parts.push(
      limitWithTime(
        t(locale, limitLabels[input.limit]),
        input.limit,
        input.freesAt,
        now,
        locale,
      ),
    );
  const limitedNow =
    kind === 'limited' ||
    kind === 'credits' ||
    kind === 'near' ||
    !!input.limit;
  const resets = input.resets;
  if (limitedNow && resets?.available) {
    parts.push(
      resets.usableAt !== null && resets.usableAt > now
        ? t(locale, 'statusResetCooldown', {
            duration: duration(resets.usableAt, now, locale),
          })
        : t(locale, 'statusResetsAvailable', { count: resets.available }),
    );
  }
  const quiet = kind === 'signIn' || kind === 'checking' || kind === 'failed';
  if (!parts.length && input.failure && !quiet) {
    parts.push(t(locale, 'statusCheckFailed'));
    title = input.failure;
  }
  if (
    !parts.length &&
    input.blocked &&
    !quiet &&
    kind !== 'unsupported' &&
    kind !== 'apiKey'
  ) {
    parts.push(t(locale, 'statusCantSwitch'));
    title = input.blocked;
  }
  if (!parts.length && input.savedAt !== null && input.savedAt !== undefined)
    parts.push(
      t(locale, 'cachedAge', { age: age(input.savedAt, now, locale) }),
    );
  return {
    kind,
    label: t(locale, labels[kind]),
    detail: parts.length ? parts.join(' · ') : null,
    title,
  };
}

export interface NoteView {
  text: string;
  tone: 'warn' | 'muted';
}

/** One saved account, as both tabs show it. */
export interface AccountRowView {
  id: string;
  name: string;
  initial: string;
  /** Email and, for Codex, the workspace; null when the name already says it all. */
  secondary: string | null;
  plan: string | null;
  active: boolean;
  next: boolean;
  /** A switch to this account is running. */
  switching: boolean;
  fiveHour: MeterView;
  /** The account-wide weekly window (the active card and the details). */
  weekly: MeterView;
  /**
   * The weekly value automation uses (the list and Next up): for Claude the higher
   * of the account-wide and the selected model's window, tagged with the model when
   * that window binds; for Codex the same as `weekly`.
   */
  weeklyBinding: MeterView;
  /** Other windows: Claude's per-model weekly limits, Codex's other limits. */
  extras: MeterView[];
  status: StatusView;
  /** At or above the switch threshold, or at a provider limit. */
  limited: boolean;
  /** When a limited account frees up; null when unknown. */
  freesAt: number | null;
  /** Plain-words lines for the card and the details. */
  notes: NoteView[];
  /** The latest usage reading and whether it is only a saved one. */
  readAt: number | null;
  savedReading: boolean;
  credits: string | null;
  /** Plain-words verification of the saved sign-in. */
  identity: string;
  identityOk: boolean;
  /** The row's primary action; null on the active row. */
  primary: 'switch' | 'signIn' | null;
  /** The provider allows switching to it (transient locks aside). */
  canSwitch: boolean;
  /** Why Switch is unavailable for as long as the account stays like this. */
  switchReason: string | null;
  canSignIn: boolean;
  canRefresh: boolean;
  canDelete: boolean;
  deleteReason: string | null;
}

export interface OrderEntry {
  id: string;
  name: string;
}

/** Everything the provider page shows, for one provider. */
export interface PageView {
  provider: ProviderKey;
  loaded: boolean;
  demo: boolean;
  /** Active first, then the usage order, then the rest by when they free up. */
  rows: AccountRowView[];
  active: AccountRowView | null;
  next: AccountRowView | null;
  /** Why the next account goes first, per the reset-order setting. */
  nextReason: string;
  model: string | null;
  /** The accounts automatic switching would use after the active one, best first. */
  order: OrderEntry[];
  orderEmpty: string;
  /** The active account is at the switch threshold or at its limit. */
  alert: string | null;
  /** What the alert is about: a real limit shows in red, the others in amber. */
  alertLimit: LimitKind | null;
  /** Every account is at its limit, and who frees up first. */
  allLimited: string | null;
  /** Saved accounts that need a new sign-in. */
  signIn: AccountRowView[];
  updatedAt: number | null;
  threshold: number;
  settings: Settings | null;
}

/** A provider-specific line inside the shared Automation card. */
export type AutomationItem =
  | { kind: 'toggle'; label: string; on: boolean }
  | { kind: 'note'; text: string; tone: 'ok' | 'warn' | 'muted' };

/** One way to add an account: the "Add account" menu and the empty state list them. */
export interface AddOption {
  key: string;
  label: string;
  icon: string;
  disabled: boolean;
  run: (trigger: HTMLElement) => void;
}

/** One message in the notices strip; both tabs use the same strip and styling. */
export interface NoticeView {
  key: string;
  tone: 'progress' | 'error' | 'success' | 'warn' | 'info' | 'demo';
  role: 'status' | 'alert' | 'note';
  /** A short bold lead before the text. */
  title?: string;
  text: string;
  /** Smaller follow-up lines. */
  lines?: { text: string; tone?: 'warn' }[];
  /** A short list, for example setup warnings. */
  items?: string[];
  action?: {
    label: string;
    disabled: boolean;
    run: (trigger: HTMLElement) => void;
  };
  dismiss?: () => void;
  /** Lets the page move focus to the notice after an action. */
  id?: string;
  label?: string;
  /** A running switch: its steps and the time so far. */
  steps?: { label: string; state: 'done' | 'current' | 'todo' }[];
  elapsed?: string;
}

export function initial(name: string): string {
  return (name.trim().charAt(0) || '?').toUpperCase();
}

export function meter(
  key: string,
  label: string,
  value: number | null,
  resetsAt: number | null,
  tone: MeterView['tone'],
  note: string | null = null,
  tag: string | null = null,
): MeterView {
  return { key, label, value, resetsAt, note, tag, tone };
}

/**
 * The shared ranking: the active account first, then the native usage order, then
 * accounts that are ready but not ranked, limited accounts by when they free up, and
 * the rest (unread, checking, failed or signed out) in their saved order.
 */
export function rankRows(
  rows: AccountRowView[],
  order: readonly string[],
): AccountRowView[] {
  const rank = new Map(order.map((id, index) => [id, index]));
  const group = (row: AccountRowView) =>
    row.active
      ? 0
      : rank.has(row.id)
        ? 1
        : !row.limited &&
            (row.status.kind === 'ready' || row.status.kind === 'next')
          ? 2
          : row.limited
            ? 3
            : 4;
  return rows
    .map((row, index) => ({ row, index, group: group(row) }))
    .sort((a, b) => {
      if (a.group !== b.group) return a.group - b.group;
      if (a.group === 1) return rank.get(a.row.id)! - rank.get(b.row.id)!;
      if (a.group === 3) {
        const at = a.row.freesAt ?? Number.MAX_SAFE_INTEGER;
        const bt = b.row.freesAt ?? Number.MAX_SAFE_INTEGER;
        if (at !== bt) return at - bt;
      }
      return a.index - b.index;
    })
    .map(({ row }) => row);
}

/** The usage order after the active account, as names. */
export function orderEntries(
  order: readonly string[],
  rows: readonly AccountRowView[],
): OrderEntry[] {
  return order
    .map((id) => rows.find((row) => row.id === id))
    .filter((row): row is AccountRowView => !!row && !row.active)
    .map((row) => ({ id: row.id, name: row.name }));
}

/** Why the next account goes first, in the words of the reset-order setting. */
export function nextReason(
  settings: Settings | null,
  locale: Language,
): string {
  return t(
    locale,
    settings?.preferSoonestWeeklyReset === false
      ? 'nextWhyMostLeft'
      : 'nextWhySoonest',
  );
}

/** "Every account is at its limit. X frees up first, in 1h 0m." */
export function allLimitedText(
  rows: readonly AccountRowView[],
  now: number,
  locale: Language,
): string {
  const first = rows
    .filter((row) => row.freesAt !== null && row.freesAt > now)
    .sort((a, b) => a.freesAt! - b.freesAt!)[0];
  return first
    ? t(locale, 'allLimitedUntil', {
        name: first.name,
        duration: duration(first.freesAt!, now, locale),
      })
    : t(locale, 'allLimited');
}

/** The active-card callout: at the limit, or at the switch threshold. */
export function alertText(
  limit: LimitKind,
  freesAt: number | null,
  threshold: number,
  now: number,
  locale: Language,
): string {
  const text =
    limit === 'near'
      ? t(locale, 'alertThreshold', {
          threshold: percentage(threshold, locale),
        })
      : t(locale, limitLabels[limit]);
  return limitWithTime(text, limit, freesAt, now, locale);
}

/** Where the order strip explains an empty order. */
export function orderEmptyText(
  rows: readonly AccountRowView[],
  locale: Language,
): string {
  return t(
    locale,
    rows.some((row) => row.readAt !== null)
      ? 'orderEmpty'
      : 'orderAfterFirstRead',
  );
}
