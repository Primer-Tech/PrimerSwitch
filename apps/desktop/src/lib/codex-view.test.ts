import { describe, expect, it } from 'vitest';
import { codexView, type CodexViewOptions } from './codex-view';
import { codexMessage } from './codex-format';
import { t } from './i18n';
import { defaults, type Settings } from './types';
import {
  codexAccount,
  codexAccounts,
  codexCapability,
  codexMainLimit,
  codexSnapshot,
} from '../test/codex-fixtures';
import type { CodexSnapshot } from './codex-types';

const now = 1_800_000_000;
const minute = 60,
  hour = 3600,
  day = 86400;
const fixture = (overrides: Partial<CodexSnapshot> = {}) =>
  codexSnapshot({ accounts: codexAccounts(now), ...overrides });
const view = (
  raw: CodexSnapshot,
  options: Partial<CodexViewOptions> = {},
): ReturnType<typeof codexView> =>
  codexView(raw, { locale: 'en', now, settings: defaults, ...options });
const row = (
  raw: CodexSnapshot,
  id: string,
  options: Partial<CodexViewOptions> = {},
) => view(raw, options).rows.find((r) => r.id === id)!;

describe('Codex adapter: meters', () => {
  it('maps the main limit to the shared windows and other limits to extra rows', () => {
    const studio = row(fixture(), 'codex-studio');
    expect(studio.fiveHour).toMatchObject({
      label: '5-hour window',
      value: 40,
      resetsAt: now + 3 * hour + 12 * minute,
      note: null,
    });
    expect(studio.weekly).toMatchObject({
      label: 'Weekly',
      value: 35,
      resetsAt: now + 4 * day + 6 * hour,
    });
    // Codex has one main weekly window: the list shows the same one.
    expect(studio.weeklyBinding).toBe(studio.weekly);
    expect(studio.weekly.tag).toBeNull();
    // The per-id copy of the main limit is not repeated.
    expect(studio.extras.map((extra) => [extra.label, extra.value])).toEqual([
      ['Weekly · Code review', 12],
    ]);
    expect(studio.credits).toBe('12.50');
    expect(studio.plan).toBe('Pro');
    expect(row(fixture(), 'codex-research').plan).toBe('Pro Lite');
  });
  it('labels an unusual window by its real length and keeps a missing one empty', () => {
    const raw = fixture();
    raw.accounts[2].quota!.limits[0] = codexMainLimit(
      10,
      0,
      {
        primary: { usedPercent: 10, windowDurationMins: 1440, resetsAt: null },
        secondary: null,
      },
      now,
    );
    const research = row(raw, 'codex-research');
    expect(research.fiveHour).toMatchObject({ label: 'Daily', value: 10 });
    expect(research.weekly).toMatchObject({ label: 'Weekly', value: null });
  });
  it('marks a window past its plan on credits and says the account uses credits', () => {
    const raw = fixture();
    raw.accounts[0].quota!.limits[0].secondary!.usedPercent = 105;
    const personal = raw.accounts[1].quota!.limits[0];
    personal.primary!.usedPercent = 20;
    personal.secondary!.usedPercent = 104;
    personal.rateLimitReachedType = null;
    personal.credits = { hasCredits: false, unlimited: false, balance: '3.00' };
    const studio = row(raw, 'codex-studio');
    expect(studio.weekly.note).toBe('Using credits');
    expect(studio.fiveHour.note).toBeNull();
    expect(studio.status).toMatchObject({
      kind: 'active',
      label: 'Active',
      detail: 'Using credits · frees in 4d 6h',
    });
    expect(row(raw, 'codex-personal').status).toMatchObject({
      kind: 'credits',
      label: 'Using credits',
      detail: 'Frees in 2d 3h',
    });
    // Past the plan without credits is simply the limit.
    raw.accounts[2].quota!.limits[0].secondary!.usedPercent = 102;
    expect(row(raw, 'codex-research').status.label).toBe('Limit reached');
    expect(row(raw, 'codex-research').weekly.note).toBeNull();
  });
});

describe('Codex adapter: statuses in the shared words', () => {
  it('names active, next, limited and ready accounts', () => {
    const raw = fixture();
    raw.accounts.push(
      codexAccount('codex-spare', {
        quota: {
          ordinaryUsageAllowed: true,
          resetCreditsAvailable: null,
          limits: [codexMainLimit(5, 5, {}, now)],
        },
      }),
    );
    expect(row(raw, 'codex-studio').status.label).toBe('Active');
    expect(row(raw, 'codex-research').status.label).toBe('Next');
    expect(row(raw, 'codex-personal').status).toMatchObject({
      kind: 'limited',
      label: 'Limit reached',
      detail: 'Frees in 2h 10m',
    });
    expect(row(raw, 'codex-spare').status.label).toBe('Ready');
    expect(row(raw, 'codex-personal', { locale: 'ro' }).status).toMatchObject({
      label: 'Limită atinsă',
      detail: 'Se eliberează în 2 h 10 min',
    });
  });
  it('calls a window at the switch threshold "Near limit" until a real limit', () => {
    const raw = fixture();
    raw.accounts[2].quota!.limits[0].primary!.usedPercent = 96;
    const research = row(raw, 'codex-research');
    // Not usable for automation, but Codex still serves it.
    expect(research.limited).toBe(true);
    expect(research.status).toMatchObject({
      kind: 'near',
      label: 'Near limit',
      detail: 'Resets in 4h 30m',
    });
    expect(row(raw, 'codex-research', { locale: 'ro' }).status).toMatchObject({
      label: 'Aproape de limită',
      detail: 'Se resetează în 4 h 30 min',
    });
    expect(
      row(raw, 'codex-research', {
        settings: { ...defaults, threshold: 98 },
      }).status.label,
    ).toBe('Next');
    // A reached-limit flag or blocked usage is a real limit at any percentage.
    raw.accounts[2].quota!.limits[0].rateLimitReachedType = 'primary';
    expect(row(raw, 'codex-research').status.label).toBe('Limit reached');
    raw.accounts[2].quota!.limits[0].rateLimitReachedType = null;
    raw.accounts[2].quota!.ordinaryUsageAllowed = false;
    expect(row(raw, 'codex-research').status.label).toBe('Limit reached');
  });
  it('says it is likely free again once the reset has passed', () => {
    const raw = fixture();
    raw.accounts[1].quota!.limits[0].primary!.resetsAt = now - minute;
    expect(row(raw, 'codex-personal').status.detail).toBe(
      'Likely free again · refresh to confirm',
    );
  });
  it('asks for a new sign-in, shows running checks and explains failures', () => {
    const raw = fixture();
    Object.assign(raw.accounts[2], {
      needsSignIn: true,
      error: 'signInRequired',
      quotaState: 'unavailable',
      switchable: codexCapability('signInRequired'),
    });
    raw.nextId = null;
    raw.order = [];
    const research = row(raw, 'codex-research');
    expect(research.status).toMatchObject({
      kind: 'signIn',
      label: 'Sign in again',
      detail: null,
    });
    expect(research).toMatchObject({
      primary: 'signIn',
      canSignIn: true,
      canSwitch: false,
      canRefresh: false,
    });
    expect(view(raw).signIn.map((r) => r.id)).toEqual(['codex-research']);
    expect(
      row(raw, 'codex-personal', {
        pending: 'codex_refresh_account',
        pendingId: 'codex-personal',
      }).status.label,
    ).toBe('Checking…');
    const all = view(raw, { pending: 'codex_refresh_all' });
    expect(all.rows.find((r) => r.id === 'codex-personal')!.status.label).toBe(
      'Checking…',
    );
    // The active account keeps its label and says it is being checked.
    expect(all.active!.status).toMatchObject({
      label: 'Active',
      detail: 'Checking…',
    });
    // A signed-out account is not refreshed.
    expect(all.rows.find((r) => r.id === 'codex-research')!.status.label).toBe(
      'Sign in again',
    );
    const failed = fixture();
    failed.accounts[2].error = 'providerUnavailable';
    expect(row(failed, 'codex-research').status).toMatchObject({
      label: 'Next',
      detail: 'Last check failed',
      title: codexMessage('providerUnavailable', 'en'),
    });
  });
  it('explains unread, API-key and blocked accounts', () => {
    const raw = fixture();
    raw.accounts.push(
      codexAccount('codex-new', { quota: null, quotaReadAt: null }),
      codexAccount('codex-api', {
        authKind: 'apiKey',
        quota: null,
        switchable: codexCapability('unsupportedAuth'),
      }),
      codexAccount('codex-unconfirmed', {
        switchable: codexCapability('identityUnverified'),
      }),
    );
    expect(row(raw, 'codex-new').status.label).toBe('Not checked yet');
    expect(row(raw, 'codex-new').notes[0].text).toBe(t('en', 'codexNoQuota'));
    expect(row(raw, 'codex-api').status).toMatchObject({
      label: 'API key',
      detail: 'No ChatGPT plan limits',
    });
    expect(row(raw, 'codex-api')).toMatchObject({
      canSwitch: false,
      canRefresh: false,
      switchReason: codexMessage('unsupportedAuth', 'en'),
    });
    expect(row(raw, 'codex-unconfirmed').status).toMatchObject({
      label: 'Ready',
      detail: 'Can’t switch to this account',
      title: codexMessage('identityUnverified', 'en'),
    });
  });
});

describe('Codex adapter: the page', () => {
  it('ranks the active account first, then the native order, then by when accounts free up', () => {
    const raw = fixture({ order: ['codex-research'] });
    raw.accounts.unshift(
      codexAccount('codex-unread', { quota: null, quotaReadAt: null }),
    );
    expect(view(raw).rows.map((r) => r.id)).toEqual([
      'codex-studio',
      'codex-research',
      'codex-personal',
      'codex-unread',
    ]);
    expect(view(raw).order).toEqual([
      { id: 'codex-research', name: 'research@example.invalid' },
    ]);
  });
  it('shows exactly the account the native side ranks next and never ranks itself', () => {
    const raw = fixture();
    raw.accounts.push(
      codexAccount('codex-tie', {
        quota: {
          ordinaryUsageAllowed: true,
          resetCreditsAvailable: null,
          limits: [codexMainLimit(3, 1, {}, now)],
        },
      }),
    );
    expect(view(raw).next?.id).toBe('codex-research');
    expect(view({ ...raw, nextId: 'codex-tie' }).next?.id).toBe('codex-tie');
    expect(view({ ...raw, nextId: null }).next).toBeNull();
    expect(view({ ...raw, nextId: 'codex-missing' }).next).toBeNull();
    expect(view({ ...raw, nextId: 'codex-studio' }).next).toBeNull();
  });
  it('explains the next account by the shared reset-order setting', () => {
    const mostLeft: Settings = { ...defaults, preferSoonestWeeklyReset: false };
    expect(view(fixture()).nextReason).toBe('Its weekly limit resets soonest.');
    expect(view(fixture(), { settings: mostLeft }).nextReason).toBe(
      'It has the most weekly usage left.',
    );
    expect(view(fixture(), { settings: null }).nextReason).toBe(
      'Its weekly limit resets soonest.',
    );
    expect(
      view(fixture(), { settings: mostLeft, locale: 'ro' }).nextReason,
    ).toBe(t('ro', 'nextWhyMostLeft'));
  });
  it('calls out an active account at its limit or at the switch threshold', () => {
    expect(view(fixture())).toMatchObject({ alert: null, alertLimit: null });
    const limited = fixture();
    limited.accounts[0].quota!.limits[0].primary!.usedPercent = 100;
    expect(view(limited)).toMatchObject({
      alert: 'Limit reached · frees in 3h 12m',
      alertLimit: 'limited',
    });
    expect(row(limited, 'codex-studio').status.detail).toBe(
      'Limit reached · frees in 3h 12m',
    );
    const threshold = fixture();
    threshold.accounts[0].quota!.limits[0].primary!.usedPercent = 96;
    expect(view(threshold)).toMatchObject({
      alert: 'Reached the 95% switch threshold · resets in 3h 12m',
      alertLimit: 'near',
    });
    expect(row(threshold, 'codex-studio').status.detail).toBe(
      'Near limit · resets in 3h 12m',
    );
  });
  it('says who frees up first when every account is at or near its limit', () => {
    const raw = fixture();
    raw.accounts[0].quota!.limits[0].primary!.usedPercent = 100;
    raw.accounts[2].quota!.limits[0].secondary!.usedPercent = 96;
    expect(view(raw).allLimited).toBe(
      'Every account is at or near its limit. personal@example.invalid frees up first, in 2h 10m.',
    );
    expect(view(fixture()).allLimited).toBeNull();
  });
  it('keeps Delete off the active account and says why, as Claude does', () => {
    const page = view(fixture());
    expect(page.active).toMatchObject({
      primary: null,
      canDelete: false,
      deleteReason: t('en', 'deleteActiveReason'),
    });
    // The same words as the native reason for a refused removal.
    expect(t('en', 'deleteActiveReason')).toBe(
      codexMessage('activeAccount', 'en'),
    );
    expect(t('ro', 'deleteActiveReason')).toBe(
      codexMessage('activeAccount', 'ro'),
    );
    expect(page.rows[1]).toMatchObject({
      primary: 'switch',
      canSwitch: true,
      canDelete: true,
    });
    expect(
      codexView(null, { locale: 'en', now, settings: null }),
    ).toMatchObject({ loaded: false, rows: [], threshold: 95 });
  });
});
