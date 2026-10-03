// Maps a Codex snapshot into the shared provider-page view-models.
import { t, type Language } from './i18n';
import {
  codexAccountPlan,
  codexCanSwitch,
  codexCredits,
  codexExtraLimits,
  codexInitial,
  codexLastRead,
  codexLimitName,
  codexLimitState,
  codexMainLimit,
  codexMessage,
  codexSecondaryLine,
  codexUsingCredits,
  codexWindowLabel,
  codexWindows,
} from './codex-format';
import type { CodexPending } from './codex-controller';
import type { CodexAccount, CodexSnapshot, CodexWindow } from './codex-types';
import type { Settings } from './types';
import {
  alertText,
  allLimitedText,
  meter,
  nextReason,
  orderEmptyText,
  orderEntries,
  rankRows,
  statusView,
  type AccountRowView,
  type MeterView,
  type NoteView,
  type PageView,
  type StatusKind,
} from './provider-view';

export interface CodexViewOptions {
  locale: Language;
  now: number;
  /** The automation settings Codex shares with Claude, once they are loaded. */
  settings: Settings | null;
  pending?: CodexPending | null;
  pendingId?: string | null;
}

const FIVE_HOURS = 300;
const WEEK = 10080;
const DEFAULT_THRESHOLD = 95;

/** A primary meter: the shared label when the window is the usual one, else its real length. */
function primaryMeter(
  window: CodexWindow | null,
  slot: 'short' | 'long',
  account: CodexAccount,
  locale: Language,
): MeterView {
  const minutes = window?.windowDurationMins ?? null;
  const usual = slot === 'short' ? FIVE_HOURS : WEEK;
  const label =
    minutes === null || minutes === usual
      ? t(locale, slot === 'short' ? 'fiveHourWindow' : 'weekly')
      : codexWindowLabel(minutes, slot, locale);
  return meter(
    slot === 'short' ? 'fiveHour' : 'weekly',
    label,
    window?.usedPercent ?? null,
    window?.resetsAt ?? null,
    slot === 'short' ? 'blue' : 'green',
    codexUsingCredits(account, window) ? t(locale, 'usingCredits') : null,
  );
}

function extraMeters(account: CodexAccount, locale: Language): MeterView[] {
  return codexExtraLimits(account).flatMap((limit) => {
    const slots = codexWindows(limit);
    const name = codexLimitName(limit, locale);
    return (['short', 'long'] as const)
      .filter((slot) => !!slots[slot])
      .map((slot) => {
        const window = slots[slot]!;
        return meter(
          `${limit.key}:${slot}`,
          `${codexWindowLabel(window.windowDurationMins, slot, locale)} · ${name}`,
          window.usedPercent,
          window.resetsAt,
          'violet',
        );
      });
  });
}

function row(
  account: CodexAccount,
  snapshot: CodexSnapshot,
  options: CodexViewOptions,
  threshold: number,
  switchingId: string | null,
): AccountRowView & { limitKind: 'limited' | 'credits' | 'threshold' | null } {
  const { locale, now, pending = null, pendingId = null } = options;
  const windows = codexWindows(codexMainLimit(account));
  const main = [windows.short, windows.long].filter(
    (window): window is CodexWindow => !!window,
  );
  const provider = codexLimitState(account);
  const credits = main.some((window) => codexUsingCredits(account, window));
  // The switch threshold is the limit for both providers; a provider limit always is.
  const atThreshold = main.some((window) => window.usedPercent >= threshold);
  const limited = provider.limited || atThreshold;
  const blocking = main.filter((window) => window.usedPercent >= threshold);
  const freesAt = !limited
    ? null
    : blocking.length && blocking.every((window) => window.resetsAt !== null)
      ? Math.max(...blocking.map((window) => window.resetsAt!))
      : provider.freesAt;
  const limitKind = credits
    ? 'credits'
    : provider.limited
      ? 'limited'
      : atThreshold
        ? 'threshold'
        : null;
  const chatgpt = account.authKind === 'chatgpt';
  const checking =
    (pending === 'codex_refresh_account' && pendingId === account.id) ||
    (pending === 'codex_refresh_all' && chatgpt && !account.needsSignIn);
  const kind: StatusKind = account.needsSignIn
    ? 'signIn'
    : account.selected
      ? 'active'
      : checking
        ? 'checking'
        : account.authKind === 'apiKey'
          ? 'apiKey'
          : account.authKind === 'unsupported'
            ? 'unsupported'
            : !account.quota
              ? 'unread'
              : limited
                ? credits
                  ? 'credits'
                  : 'limited'
                : snapshot.nextId === account.id
                  ? 'next'
                  : 'ready';
  const capability = snapshot.capabilities.switchAccount;
  const blocked =
    capability.enabled &&
    !account.switchable.enabled &&
    !account.selected &&
    !account.needsSignIn
      ? codexMessage(account.switchable.blockedReason, locale)
      : null;
  const failure =
    account.error && !account.needsSignIn
      ? codexMessage(account.error, locale)
      : null;
  const notes: NoteView[] = [];
  if (account.authKind === 'apiKey')
    notes.push({ tone: 'muted', text: t(locale, 'codexApiQuota') });
  else if (chatgpt && !account.quota && !account.needsSignIn)
    notes.push({ tone: 'muted', text: t(locale, 'codexNoQuota') });
  else if (account.quota && !main.length)
    notes.push({ tone: 'muted', text: t(locale, 'codexNoWindows') });
  if (account.quota?.ordinaryUsageAllowed === false)
    notes.push({ tone: 'warn', text: t(locale, 'codexUsageBlocked') });
  if (codexMainLimit(account)?.spendControlReached === true)
    notes.push({ tone: 'warn', text: t(locale, 'codexSpendBlocked') });
  if (failure) notes.push({ tone: 'warn', text: failure });
  const status = statusView(
    {
      kind,
      limit:
        account.selected && limited ? (credits ? 'credits' : 'limited') : null,
      freesAt,
      failure,
      blocked,
    },
    now,
    locale,
  );
  return {
    id: account.id,
    name: account.name,
    initial: codexInitial(account),
    secondary: codexSecondaryLine(account) || null,
    plan: codexAccountPlan(account, locale),
    active: account.selected,
    next: snapshot.nextId === account.id && !account.selected,
    switching: switchingId === account.id,
    fiveHour: primaryMeter(windows.short, 'short', account, locale),
    weekly: primaryMeter(windows.long, 'long', account, locale),
    extras: extraMeters(account, locale),
    // The active row is busy refreshing: say so beside "Active".
    status:
      account.selected && checking && !account.needsSignIn
        ? {
            ...status,
            detail: [t(locale, 'statusChecking'), status.detail]
              .filter(Boolean)
              .join(' · '),
          }
        : status,
    limited,
    freesAt,
    limitKind,
    notes,
    readAt: account.quotaReadAt,
    savedReading:
      account.quotaState === 'cached' || account.quotaState === 'unavailable',
    credits: codexCredits(account, locale),
    identity: t(
      locale,
      account.needsSignIn
        ? 'identitySignIn'
        : account.authKind === 'apiKey'
          ? 'identityApiKey'
          : account.authKind === 'unsupported'
            ? 'identityUnsupported'
            : account.identityVerified
              ? 'identityVerified'
              : 'identityUnverified',
    ),
    identityOk: chatgpt && account.identityVerified && !account.needsSignIn,
    primary: account.needsSignIn
      ? 'signIn'
      : account.selected
        ? null
        : 'switch',
    canSwitch: codexCanSwitch(snapshot, account),
    switchReason:
      blocked ??
      (!capability.enabled && !account.selected
        ? codexMessage(capability.blockedReason, locale)
        : null),
    canSignIn:
      account.needsSignIn && snapshot.capabilities.loginBrowser.enabled,
    canRefresh:
      snapshot.capabilities.refreshQuota.enabled &&
      chatgpt &&
      !account.needsSignIn,
    canDelete: !account.selected && snapshot.capabilities.deleteSaved.enabled,
    deleteReason: account.selected ? t(locale, 'deleteActiveReason') : null,
  };
}

/** The Codex tab's page: the same view-model the Claude tab uses. */
export function codexView(
  snapshot: CodexSnapshot | null,
  options: CodexViewOptions,
): PageView {
  const { locale, now, settings, pending = null, pendingId = null } = options;
  const threshold = settings?.threshold ?? DEFAULT_THRESHOLD;
  if (!snapshot)
    return {
      provider: 'codex',
      loaded: false,
      demo: false,
      rows: [],
      active: null,
      next: null,
      nextReason: nextReason(settings, locale),
      model: null,
      order: [],
      orderEmpty: orderEmptyText([], locale),
      alert: null,
      allLimited: null,
      signIn: [],
      updatedAt: null,
      threshold,
      settings,
    };
  const switchingId =
    snapshot.switching?.targetId ??
    (pending === 'codex_switch_account' ? pendingId : null);
  const built = snapshot.accounts.map((account) =>
    row(account, snapshot, options, threshold, switchingId),
  );
  const rows = rankRows(
    built.map(({ limitKind: _limit, ...rest }) => rest),
    snapshot.order,
  );
  const activeBuilt =
    built.find((r) => r.id === snapshot.selectedId) ??
    built.find((r) => r.active) ??
    null;
  const active = rows.find((r) => r.id === activeBuilt?.id) ?? null;
  return {
    provider: 'codex',
    loaded: true,
    demo: snapshot.demo,
    rows,
    active,
    // Exactly the account the native side ranks next.
    next: rows.find((r) => r.id === snapshot.nextId && !r.active) ?? null,
    nextReason: nextReason(settings, locale),
    model: snapshot.activeModel,
    order: orderEntries(snapshot.order, rows),
    orderEmpty: orderEmptyText(rows, locale),
    alert:
      activeBuilt?.limitKind && activeBuilt.status.kind !== 'signIn'
        ? alertText(
            activeBuilt.limitKind,
            activeBuilt.freesAt,
            threshold,
            now,
            locale,
          )
        : null,
    allLimited:
      built.length && built.every((r) => r.limited)
        ? allLimitedText(rows, now, locale)
        : null,
    signIn: rows.filter((r) => r.status.kind === 'signIn'),
    updatedAt: codexLastRead(snapshot),
    threshold,
    settings,
  };
}
