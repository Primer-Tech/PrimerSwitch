// Maps a Claude snapshot into the shared provider-page view-models.
import { t, type Language } from './i18n';
import { planLabel } from './format';
import { codeMessage } from './controller';
import type { AccountView, Snapshot } from './types';
import {
  alertText,
  allLimitedText,
  initial,
  meter,
  nextReason,
  orderEmptyText,
  orderEntries,
  rankRows,
  statusView,
  type AccountRowView,
  type LimitKind,
  type NoteView,
  type PageView,
  type StatusKind,
} from './provider-view';

export interface ClaudeViewOptions {
  locale: Language;
  now: number;
  /** The account a switch started from this window is moving to. */
  switchingId?: string | null;
}

/** Model readings older than 30 minutes are flagged in the details. */
const SCOPED_STALE_SECONDS = 1800;

/**
 * Claude's `exhausted` means at or over the switch threshold. Only a binding window
 * (the 5-hour window, or the weekly value automation uses) at 100% is a real limit;
 * below that the account is near its limit.
 */
function limitOf(account: AccountView): LimitKind | null {
  if (!account.exhausted) return null;
  const usage = account.usage;
  return usage &&
    (usage.fiveHour.utilization >= 100 || usage.weeklyModel >= 100)
    ? 'limited'
    : 'near';
}

function row(
  account: AccountView,
  { locale, now, switchingId = null }: ClaudeViewOptions,
): AccountRowView {
  const usage = account.usage;
  const signIn = !!account.signInRequired;
  const limit = limitOf(account);
  const kind: StatusKind = signIn
    ? 'signIn'
    : account.active
      ? 'active'
      : !account.identityVerified
        ? account.error
          ? 'failed'
          : 'checking'
        : !usage
          ? 'unread'
          : limit
            ? limit
            : account.isNext
              ? 'next'
              : 'ready';
  const failure =
    account.error && !signIn ? codeMessage(account.error, locale) : null;
  const freesAt = account.exhausted ? (account.freesAt ?? null) : null;
  const notes: NoteView[] = [];
  if (failure)
    notes.push({
      tone: 'warn',
      text: usage ? `${failure} ${t(locale, 'lastReadingKept')}` : failure,
    });
  if (account.active && account.decisionFresh === false)
    notes.push({ tone: 'muted', text: t(locale, 'automationWaiting') });
  if (
    usage?.scopedLimits.length &&
    account.scopedAt !== undefined &&
    account.scopedAt !== null &&
    now - account.scopedAt > SCOPED_STALE_SECONDS
  )
    notes.push({ tone: 'muted', text: t(locale, 'scopedStale') });
  const weekly = t(locale, 'weekly');
  return {
    id: account.id,
    name: account.name,
    initial: initial(account.name || account.email || 'C'),
    secondary: account.email || null,
    plan: planLabel(account.planTier),
    active: account.active,
    next: account.isNext,
    switching: switchingId === account.id,
    fiveHour: meter(
      'fiveHour',
      t(locale, 'fiveHourWindow'),
      usage?.fiveHour.utilization ?? null,
      usage?.fiveHour.resetsAt ?? null,
      'blue',
    ),
    // The account-wide weekly window; never borrows another window's reset time.
    weekly: meter(
      'weekly',
      weekly,
      usage?.weeklyOverall ?? null,
      usage?.weeklyOverallResetsAt ?? null,
      'green',
    ),
    extras: (usage?.scopedLimits ?? []).map((limit) =>
      meter(
        `scoped:${limit.label}`,
        `${weekly} · ${limit.label}`,
        limit.percent,
        limit.resetsAt,
        'violet',
      ),
    ),
    status: statusView(
      {
        kind,
        limit: account.active ? limit : null,
        freesAt,
        failure,
        resets: account.exhausted
          ? {
              available: account.resets.available,
              usableAt: account.resets.cooldownUntil,
            }
          : null,
        savedAt:
          account.decisionFresh === false && usage ? account.usageAt : null,
      },
      now,
      locale,
    ),
    limited: account.exhausted,
    freesAt,
    notes,
    readAt: account.usageAt,
    savedReading: !!account.error || account.decisionFresh === false,
    credits: null,
    identity: t(
      locale,
      signIn
        ? 'identitySignIn'
        : account.identityVerified
          ? 'identityVerified'
          : account.error
            ? 'identityUnverified'
            : 'identityChecking',
    ),
    identityOk: account.identityVerified && !signIn,
    primary: account.active ? null : signIn ? 'signIn' : 'switch',
    canSwitch: !account.active && !signIn && account.identityVerified,
    switchReason:
      account.active || signIn || account.identityVerified
        ? null
        : t(locale, account.error ? 'switchUnverified' : 'switchChecking'),
    canSignIn: signIn && !account.active,
    canRefresh: true,
    // Same rule as Codex: switch away before removing the account in use.
    canDelete: !account.active,
    deleteReason: account.active ? t(locale, 'deleteActiveReason') : null,
  };
}

/** The Claude tab's page: the same view-model the Codex tab uses. */
export function claudeView(
  snapshot: Snapshot | null,
  options: ClaudeViewOptions,
): PageView {
  const { locale, now } = options;
  if (!snapshot)
    return {
      provider: 'claude',
      loaded: false,
      demo: false,
      rows: [],
      active: null,
      next: null,
      nextReason: nextReason(null, locale),
      model: null,
      order: [],
      orderEmpty: orderEmptyText([], locale),
      alert: null,
      alertLimit: null,
      allLimited: null,
      signIn: [],
      updatedAt: null,
      threshold: 95,
      settings: null,
    };
  const settings = snapshot.settings;
  const plan = snapshot.consumptionPlan ?? [];
  const unsorted = snapshot.accounts.map((account) => row(account, options));
  const rows = rankRows(unsorted, plan);
  const active =
    rows.find((r) => r.id === snapshot.activeId) ??
    rows.find((r) => r.active) ??
    null;
  const activeAccount = snapshot.accounts.find((a) => a.id === active?.id);
  const activeLimit = activeAccount ? limitOf(activeAccount) : null;
  return {
    provider: 'claude',
    loaded: true,
    demo: snapshot.demo,
    rows,
    active,
    next: rows.find((r) => r.next && !r.active) ?? null,
    nextReason: nextReason(settings, locale),
    model: snapshot.activeModel ?? t(locale, 'defaultModel'),
    order: orderEntries(plan, rows),
    orderEmpty: orderEmptyText(rows, locale),
    alert:
      activeAccount && activeLimit
        ? alertText(
            activeLimit,
            activeAccount.freesAt ?? null,
            settings.threshold,
            now,
            locale,
          )
        : null,
    alertLimit: activeLimit,
    allLimited:
      snapshot.accounts.length &&
      snapshot.accounts.every((account) => account.exhausted)
        ? allLimitedText(rows, now, locale)
        : null,
    signIn: rows.filter((r) => r.canSignIn),
    updatedAt: snapshot.lastRefreshAt,
    threshold: settings.threshold,
    settings,
  };
}
