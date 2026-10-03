import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { claudeView } from './claude-view';
import { codeMessage } from './controller';
import { t } from './i18n';
import { account, snapshot } from '../test/fixtures';
import type { AccountView, Snapshot } from './types';

const now = 1_800_000_000;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(now * 1000);
});
afterEach(() => vi.useRealTimers());
const view = (raw: Snapshot, locale: 'en' | 'ro' = 'en') =>
  claudeView(raw, { locale, now });
const row = (raw: Snapshot, id: string) =>
  view(raw).rows.find((r) => r.id === id)!;
const status = (raw: Snapshot, id: string) => row(raw, id).status;
const withAccounts = (...accounts: AccountView[]) =>
  snapshot({ accounts, consumptionPlan: [] });

describe('Claude adapter: meters', () => {
  it('shows the 5-hour window, the account-wide week and one row per model limit', () => {
    const studio = row(snapshot(), 'a');
    expect(studio.fiveHour).toMatchObject({
      label: '5-hour window',
      value: 68,
      resetsAt: now + 3600,
      note: null,
    });
    expect(studio.weekly).toMatchObject({
      label: 'Weekly',
      value: 47,
      resetsAt: now + 3 * 86400,
    });
    expect(studio.extras).toEqual([
      expect.objectContaining({
        label: 'Weekly · Sonnet',
        value: 58,
        resetsAt: now + 3 * 86400,
      }),
    ]);
    expect(row(snapshot(), 'a').plan).toBe('Max 5×');
  });
  it('never borrows a reset time and shows unknown usage as no value', () => {
    const raw = snapshot();
    delete raw.accounts[0].usage!.weeklyOverallResetsAt;
    expect(row(raw, 'a').weekly.resetsAt).toBeNull();
    const empty = withAccounts(account('a', { usage: null, usageAt: null }));
    const studio = row(empty, 'a');
    expect(studio.fiveHour.value).toBeNull();
    expect(studio.weekly.value).toBeNull();
    expect(studio.extras).toEqual([]);
    expect(studio.readAt).toBeNull();
  });
});

describe('Claude adapter: statuses in the shared words', () => {
  it('names active, next and ready accounts', () => {
    const raw = snapshot();
    raw.accounts.push(account('c', { name: 'Research', isNext: false }));
    expect(status(raw, 'a')).toEqual({
      kind: 'active',
      label: 'Active',
      detail: null,
      title: null,
    });
    expect(status(raw, 'b').label).toBe('Next');
    expect(status(raw, 'c').label).toBe('Ready');
  });
  it('says when a limited account frees up, or that it is likely free again', () => {
    const raw = withAccounts(
      account('a'),
      account('b', { exhausted: true, freesAt: now + 3600 }),
      account('c', { exhausted: true, freesAt: now - 60 }),
    );
    expect(status(raw, 'b')).toMatchObject({
      kind: 'limited',
      label: 'Limit reached',
      detail: 'Frees in 1h 0m · Resets available: 2',
    });
    expect(status(raw, 'c').detail).toBe(
      'Likely free again · refresh to confirm · Resets available: 2',
    );
    raw.accounts[1].resets.cooldownUntil = now + 1800;
    expect(status(raw, 'b').detail).toBe(
      'Frees in 1h 0m · Resets usable in 30m',
    );
    raw.accounts[1].resets.available = 0;
    expect(status(raw, 'b').detail).toBe('Frees in 1h 0m');
    expect(
      claudeView(raw, { locale: 'ro', now }).rows.find((r) => r.id === 'b')!
        .status.detail,
    ).toBe('Se eliberează în 1 h 0 min');
  });
  it('asks for a new sign-in and explains checks that are running or failed', () => {
    const raw = withAccounts(
      account('a'),
      account('b', { signInRequired: true, error: 'signInRequired' }),
      account('c', { identityVerified: false, isNext: false }),
      account('d', {
        identityVerified: false,
        isNext: false,
        error: 'identityError',
      }),
      account('e', { isNext: false, error: 'providerError' }),
      account('f', { isNext: false, usage: null, usageAt: null }),
      account('g', { isNext: false, decisionFresh: false, usageAt: now - 600 }),
    );
    const signIn = row(raw, 'b');
    expect(signIn.status).toMatchObject({
      kind: 'signIn',
      label: 'Sign in again',
      detail: null,
    });
    expect(signIn).toMatchObject({
      primary: 'signIn',
      canSignIn: true,
      canSwitch: false,
    });
    expect(row(raw, 'c').status).toMatchObject({
      kind: 'checking',
      label: 'Checking…',
    });
    expect(row(raw, 'c')).toMatchObject({
      canSwitch: false,
      switchReason: t('en', 'switchChecking'),
    });
    expect(row(raw, 'd').status).toEqual({
      kind: 'failed',
      label: 'Check failed',
      detail: 'Refresh to try again',
      title: codeMessage('identityError', 'en'),
    });
    expect(row(raw, 'd').switchReason).toBe(t('en', 'switchUnverified'));
    // A failed latest reading keeps the previous one and says so.
    expect(row(raw, 'e').status).toEqual({
      kind: 'ready',
      label: 'Ready',
      detail: 'Last check failed',
      title: codeMessage('providerError', 'en'),
    });
    expect(row(raw, 'e').notes[0]).toEqual({
      tone: 'warn',
      text: `${codeMessage('providerError', 'en')} ${t('en', 'lastReadingKept')}`,
    });
    expect(row(raw, 'f').status.label).toBe('Not checked yet');
    expect(row(raw, 'g').status.detail).toBe('Saved reading · 10 min. ago');
  });
  it('keeps the active label and adds its own limit', () => {
    const raw = withAccounts(
      account('a', { exhausted: true, freesAt: now + 7800 }),
      account('b'),
    );
    expect(status(raw, 'a')).toMatchObject({
      kind: 'active',
      label: 'Active',
      detail: 'Limit reached · frees in 2h 10m · Resets available: 2',
    });
  });
});

describe('Claude adapter: the page', () => {
  it('ranks the active account first, then the usage order, then by when accounts free up', () => {
    const raw = snapshot({
      accounts: [
        account('e', { name: 'Checking', identityVerified: false }),
        account('d', { name: 'Soon', exhausted: true, freesAt: now + 3600 }),
        account('r', { name: 'Research', isNext: false }),
        account('c', { name: 'Later', exhausted: true, freesAt: now + 7200 }),
        account('b'),
        account('a'),
      ],
      consumptionPlan: ['b', 'a', 'r'],
    });
    const page = view(raw);
    expect(page.rows.map((r) => r.name)).toEqual([
      'Studio',
      'Personal',
      'Research',
      'Soon',
      'Later',
      'Checking',
    ]);
    // The order strip lists what comes after the active account.
    expect(page.order.map((entry) => entry.name)).toEqual([
      'Personal',
      'Research',
    ]);
    expect(page.next?.name).toBe('Personal');
    expect(page.active?.name).toBe('Studio');
  });
  it('explains the next account by the reset-order setting, in both languages', () => {
    const raw = snapshot();
    expect(view(raw).nextReason).toBe('Its weekly limit resets soonest.');
    raw.settings.preferSoonestWeeklyReset = false;
    expect(view(raw).nextReason).toBe('It has the most weekly usage left.');
    expect(view(raw, 'ro').nextReason).toBe(t('ro', 'nextWhyMostLeft'));
  });
  it('calls out an active account at the switch threshold or at its limit', () => {
    expect(view(snapshot()).alert).toBeNull();
    const raw = withAccounts(
      account('a', { exhausted: true, freesAt: now + 7800 }),
      account('b'),
    );
    expect(view(raw).alert).toBe(
      'Reached the 95% switch threshold · frees in 2h 10m',
    );
    raw.accounts[0].usage!.fiveHour.utilization = 100;
    expect(view(raw).alert).toBe('Limit reached · frees in 2h 10m');
  });
  it('says who frees up first when every account is limited', () => {
    const raw = withAccounts(
      account('a', { exhausted: true, freesAt: now + 7800 }),
      account('b', { exhausted: true, freesAt: now + 3600 }),
    );
    expect(view(raw).allLimited).toBe(
      'Every account is at its limit. Personal frees up first, in 1h 0m.',
    );
    expect(view(raw, 'ro').allLimited).toBe(
      'Toate conturile sunt la limită. Primul se eliberează Personal, în 1 h 0 min.',
    );
    expect(view(snapshot()).allLimited).toBeNull();
  });
  it('keeps Delete off the active account with the reason Codex gives', () => {
    const page = view(snapshot());
    expect(page.active).toMatchObject({
      primary: null,
      canDelete: false,
      canRefresh: true,
      deleteReason: 'Switch to another account before removing this one.',
    });
    expect(page.rows[1]).toMatchObject({
      primary: 'switch',
      canSwitch: true,
      switchReason: null,
      canDelete: true,
      deleteReason: null,
    });
    expect(
      claudeView(snapshot(), { locale: 'ro', now }).active!.deleteReason,
    ).toBe('Comută pe alt cont înainte să-l ștergi pe acesta.');
    expect(page.model).toBe('Sonnet');
    expect(view(snapshot({ activeModel: null })).model).toBe('Default model');
    expect(claudeView(null, { locale: 'en', now })).toMatchObject({
      loaded: false,
      rows: [],
      active: null,
    });
  });
});
