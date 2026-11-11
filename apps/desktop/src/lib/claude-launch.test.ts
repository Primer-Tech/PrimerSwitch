import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { claudeView } from './claude-view';
import {
  launchBlock,
  launchBlockText,
  launchNoticeView,
  launchReasonText,
  needsSignIn,
} from './claude-launch';
import { t } from './i18n';
import {
  launchOutcomeSchema,
  snapshotSchema,
  type AccountView,
  type Snapshot,
} from './types';
import { account, snapshot } from '../test/fixtures';

const now = 1_800_000_000;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(now * 1000);
});
afterEach(() => vi.useRealTimers());

type Fable = { label: string; percent: number; resetsAt: number | null };
const rowOf = (raw: Snapshot, id: string, locale: 'en' | 'ro' = 'en') =>
  claudeView(raw, { locale, now }).rows.find((r) => r.id === id)!;
function withFable(
  id: string,
  fable: Fable | null | undefined,
  overrides: Partial<AccountView> = {},
): AccountView {
  const result = account(id, overrides);
  if (result.usage) result.usage.fable = fable;
  return result;
}
const supported = (...accounts: AccountView[]) =>
  snapshot({ accounts, consumptionPlan: [], launchSupported: true });

describe('weekly Fable window', () => {
  it("shows the provider's own Fable row whichever model is selected", () => {
    const raw = snapshot({
      activeModel: 'Opus',
      consumptionPlan: [],
      accounts: [
        withFable('a', {
          label: 'Fable',
          percent: 36,
          resetsAt: now + 2 * 86400,
        }),
        account('b'),
      ],
    });
    const studio = rowOf(raw, 'a');
    expect(studio.fable).toMatchObject({
      label: 'Weekly · Fable',
      value: 36,
      resetsAt: now + 2 * 86400,
    });
    // The account-wide and binding weekly values are separate readings.
    expect(studio.weekly.value).toBe(47);
    expect(studio.weeklyBinding.value).toBe(58);
  });
  it('keeps Fable apart from the selected model and from the account-wide week', () => {
    const raw = snapshot({
      activeModel: 'Sonnet',
      consumptionPlan: [],
      accounts: [
        withFable('a', { label: 'Fable', percent: 12, resetsAt: now + 86400 }),
        account('b'),
      ],
    });
    const studio = rowOf(raw, 'a');
    expect(studio.weeklyBinding).toMatchObject({ value: 58, tag: 'Sonnet' });
    expect(studio.fable?.value).toBe(12);
    expect(studio.fable?.tone).toBe('violet');
  });
  it('treats a missing reading as unknown, never zero and never another window', () => {
    for (const fable of [null, undefined]) {
      const raw = snapshot({
        consumptionPlan: [],
        accounts: [withFable('a', fable), account('b')],
      });
      const studio = rowOf(raw, 'a');
      expect(studio.fable).toMatchObject({
        label: 'Weekly · Fable',
        value: null,
        resetsAt: null,
      });
      // Neighbouring windows are untouched by the missing row.
      expect(studio.weekly.value).toBe(47);
      expect(studio.weeklyBinding.value).toBe(58);
    }
    const unread = snapshot({
      consumptionPlan: [],
      accounts: [account('a', { usage: null, usageAt: null })],
    });
    expect(rowOf(unread, 'a').fable).toMatchObject({ value: null });
  });
  it('shows a real zero as zero', () => {
    const raw = snapshot({
      consumptionPlan: [],
      accounts: [
        withFable('a', { label: 'Fable', percent: 0, resetsAt: now + 3600 }),
      ],
    });
    expect(rowOf(raw, 'a').fable).toMatchObject({ value: 0 });
  });
  it('carries the actual label and its own reset time', () => {
    const raw = snapshot({
      consumptionPlan: [],
      accounts: [
        withFable('a', {
          label: 'Fable 5.2',
          percent: 40,
          resetsAt: now + 5 * 86400,
        }),
      ],
    });
    const studio = rowOf(raw, 'a');
    expect(studio.fable).toMatchObject({
      label: 'Weekly · Fable 5.2',
      resetsAt: now + 5 * 86400,
    });
    // The account-wide week keeps its own reset.
    expect(studio.weekly.resetsAt).toBe(now + 3 * 86400);
    // The prefix follows the language; the provider's own model label does not.
    expect(rowOf(raw, 'a', 'ro').fable?.label).toBe(
      `${t('ro', 'weekly')} · Fable 5.2`,
    );
  });
  it('lists Fable once in the details and keeps every other scoped limit', () => {
    const fable = { label: 'Fable', percent: 36, resetsAt: now + 86400 };
    const listed = account('a');
    listed.usage!.scopedLimits = [
      { label: 'Sonnet', percent: 58, resetsAt: now + 3 * 86400 },
      { ...fable },
      { label: 'Code review', percent: 9, resetsAt: null },
    ];
    listed.usage!.fable = { ...fable };
    expect(
      rowOf(supported(listed), 'a').extras.map((extra) => extra.label),
    ).toEqual(['Weekly · Sonnet', 'Weekly · Fable', 'Weekly · Code review']);
    // A reading that lacks it among the scoped rows still shows it, once.
    const apart = withFable('a', fable);
    expect(
      rowOf(supported(apart), 'a').extras.map((extra) => extra.label),
    ).toEqual(['Weekly · Sonnet', 'Weekly · Fable']);
    // Without a Fable reading the details add nothing.
    expect(
      rowOf(supported(withFable('a', null)), 'a').extras.map(
        (extra) => extra.label,
      ),
    ).toEqual(['Weekly · Sonnet']);
  });
});

describe('launch availability', () => {
  it('offers the active account too, unlike a switch', () => {
    const raw = supported(account('a'), account('b'));
    const active = rowOf(raw, 'a');
    expect(active).toMatchObject({
      canLaunch: true,
      launchReason: null,
      canSwitch: false,
    });
    expect(rowOf(raw, 'b')).toMatchObject({ canLaunch: true, canSwitch: true });
  });
  it.each([
    ['older snapshot without the flag', undefined],
    ['unsupported build', false],
  ])('stays unavailable for an %s', (_, flag) => {
    const raw = snapshot({
      consumptionPlan: [],
      accounts: [account('a'), account('b')],
      launchSupported: flag,
    });
    for (const id of ['a', 'b'])
      expect(rowOf(raw, id)).toMatchObject({
        canLaunch: false,
        launchReason: t('en', 'launchUnsupported'),
      });
  });
  it('is read-only in the preview', () => {
    const raw = snapshot({
      consumptionPlan: [],
      accounts: [account('a'), account('b')],
      launchSupported: true,
      demo: true,
    });
    expect(rowOf(raw, 'b')).toMatchObject({
      canLaunch: false,
      launchReason: t('en', 'demoOnly'),
    });
  });
  it('waits for a trusted identity and a usable sign-in', () => {
    const raw = supported(
      account('a', { active: true, error: 'activeSessionExpired' }),
      account('b', { signInRequired: true, error: 'signInRequired' }),
      account('c', { identityVerified: false }),
    );
    expect(rowOf(raw, 'a')).toMatchObject({
      canLaunch: false,
      launchReason: t('en', 'launchNeedsSignIn'),
    });
    expect(rowOf(raw, 'b')).toMatchObject({
      canLaunch: false,
      launchReason: t('en', 'launchNeedsSignIn'),
    });
    expect(rowOf(raw, 'c')).toMatchObject({
      canLaunch: false,
      launchReason: t('en', 'launchNeedsVerified'),
    });
  });
  it('reports the most fundamental reason first, in both languages', () => {
    const demo = snapshot({ demo: true, launchSupported: false });
    const broken = account('b', {
      signInRequired: true,
      identityVerified: false,
    });
    expect(launchBlock(demo, broken)).toBe('demo');
    expect(launchBlock({ demo: false, launchSupported: false }, broken)).toBe(
      'unsupported',
    );
    expect(launchBlock({ demo: false, launchSupported: true }, broken)).toBe(
      'signIn',
    );
    expect(
      launchBlock(
        { demo: false, launchSupported: true },
        account('b', { identityVerified: false }),
      ),
    ).toBe('unverified');
    expect(
      launchBlock({ demo: false, launchSupported: true }, account('b')),
    ).toBe(null);
    expect(launchBlockText('signIn', 'ro')).toBe(t('ro', 'launchNeedsSignIn'));
    expect(needsSignIn(account('a', { error: 'activeSessionExpired' }))).toBe(
      true,
    );
    expect(needsSignIn(account('b', { error: 'activeSessionExpired' }))).toBe(
      false,
    );
  });
  it('explains why authorization is needed with the account name', () => {
    expect(launchReasonText('firstUse', 'en', 'Personal')).toContain(
      'Personal',
    );
    expect(launchReasonText('identityChanged', 'ro', 'Personal')).toContain(
      'Personal',
    );
    expect(launchReasonText('expired', 'en', 'Personal')).toBe(
      t('en', 'launchAuthExpired', { name: 'Personal' }),
    );
    expect(launchReasonText(null, 'en', 'Personal')).toBe(
      t('en', 'launchAuthFirstUse', { name: 'Personal' }),
    );
  });
  it('words the finished-launch notice with the account, or without it', () => {
    const dismiss = vi.fn();
    const accounts = [account('a'), account('b')];
    expect(launchNoticeView(null, accounts, 'en', dismiss)).toBeNull();
    const named = launchNoticeView({ accountId: 'b' }, accounts, 'en', dismiss);
    expect(named).toMatchObject({
      tone: 'success',
      role: 'status',
      text: t('en', 'launchStarted', { name: 'Personal' }),
    });
    named!.dismiss!();
    expect(dismiss).toHaveBeenCalledOnce();
    expect(
      launchNoticeView({ accountId: 'gone' }, accounts, 'ro', dismiss)?.text,
    ).toBe(t('ro', 'launchStartedAnonymous'));
  });
});

describe('launch and Fable schemas', () => {
  it('accepts the new snapshot fields and older snapshots without them', () => {
    const fresh = snapshot({ launchSupported: true });
    fresh.accounts[0].usage!.fable = {
      label: 'Fable',
      percent: 36,
      resetsAt: null,
    };
    expect(snapshotSchema.parse(fresh).accounts[0].usage?.fable?.percent).toBe(
      36,
    );
    fresh.accounts[1].usage!.fable = null;
    expect(snapshotSchema.parse(fresh).accounts[1].usage?.fable).toBeNull();
    const old = snapshotSchema.parse(snapshot());
    expect(old.launchSupported).toBeUndefined();
    expect(old.accounts[0].usage?.fable).toBeUndefined();
  });
  it('rejects malformed or credential-bearing launch data', () => {
    const odd = snapshot();
    odd.accounts[0].usage!.fable = {
      label: 'Fable',
      percent: -1,
      resetsAt: null,
    };
    expect(snapshotSchema.safeParse(odd).success).toBe(false);
    const extra = snapshot();
    Object.assign(extra.accounts[0].usage!, {
      fable: {
        label: 'Fable',
        percent: 1,
        resetsAt: null,
        accessToken: 'never-display',
      },
    });
    expect(snapshotSchema.safeParse(extra).success).toBe(false);
    expect(
      snapshotSchema.safeParse({ ...snapshot(), launchSupported: 'yes' })
        .success,
    ).toBe(false);
  });
  it('parses every launch outcome and nothing else', () => {
    for (const outcome of [
      { status: 'launched' },
      { status: 'authorized' },
      { status: 'cancelled' },
      { status: 'authorizationRequired', reason: 'firstUse' },
      { status: 'authorizationRequired', reason: 'identityChanged' },
      { status: 'authorizationRequired', reason: 'expired' },
    ])
      expect(launchOutcomeSchema.parse(outcome)).toEqual(outcome);
    for (const bad of [
      { status: 'authorizationRequired' },
      { status: 'authorizationRequired', reason: 'other' },
      { status: 'launched', path: 'C:\\project' },
      { status: 'started' },
      { reason: 'firstUse' },
      'launched',
      null,
    ])
      expect(launchOutcomeSchema.safeParse(bad).success, String(bad)).toBe(
        false,
      );
  });
});
