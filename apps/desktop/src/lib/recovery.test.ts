import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { createController } from './controller';
import { claudeView } from './claude-view';
import { codexView } from './codex-view';
import { account, snapshot } from '../test/fixtures';
import {
  codexAccount,
  codexCapability,
  codexSnapshot,
} from '../test/codex-fixtures';
import { defaultAutomation, defaults, type AccountView } from './types';
import type { CodexAccount } from './codex-types';
import type { Bridge } from './bridge';

afterEach(() => vi.useRealTimers());
const now = 1_800_000_000;

describe('unavailable native recommendations are never advertised', () => {
  it('keeps same-email Claude identities separate and exposes their original workspaces', () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(now * 1000);
    const raw = snapshot({
      accounts: [
        account('a', {
          name: 'same@example.invalid',
          email: 'same@example.invalid',
          workspaceLabel: 'Personal · …11112222',
        }),
        account('b', {
          name: 'same@example.invalid',
          email: 'same@example.invalid',
          workspaceLabel: 'Personal · …33334444',
        }),
      ],
    });
    const page = claudeView(raw, { locale: 'en', now });
    expect(page.rows.map((row) => row.id)).toEqual(['a', 'b']);
    expect(page.rows[0].secondary).toContain('Personal · …11112222');
    expect(page.rows[1].secondary).toContain('Personal · …33334444');
    expect(page.order[0].name).toBe(
      'same@example.invalid · Personal · …33334444',
    );
  });
  it.each([
    { signInRequired: true, error: 'signInRequired' },
    { error: 'providerError' },
    { identityVerified: false },
    { decisionFresh: false, usageAt: now - 5 * 3600 },
    { automation: { ...defaultAutomation, enabled: false } },
    { exhausted: true },
    { usage: null, usageAt: null },
  ] satisfies Partial<AccountView>[])(
    'filters Claude stale next/order entry with %j without inventing a replacement',
    (patch) => {
      vi.useFakeTimers({ toFake: ['Date'] });
      vi.setSystemTime(now * 1000);
      const raw = snapshot({
        accounts: [
          account('a'),
          account('b', patch),
          account('c', { isNext: false }),
        ],
        consumptionPlan: ['b', 'c', 'a'],
      });
      const page = claudeView(raw, { locale: 'en', now });
      expect(page.next).toBeNull();
      expect(page.order.map((entry) => entry.id)).toEqual(['c']);
      expect(page.rows.find((row) => row.id === 'b')!.next).toBe(false);
      expect(page.rows.find((row) => row.id === 'b')!.status.label).not.toBe(
        'Next',
      );
    },
  );

  it.each([
    { needsSignIn: true, error: 'signInRequired' },
    { error: 'providerUnavailable' },
    { identityVerified: false },
    { quotaState: 'cached', quotaReadAt: now - 5 * 3600 },
    { automation: { ...defaultAutomation, enabled: false } },
    { exhausted: true },
    { switchable: codexCapability('identityUnverified') },
    { quota: null, quotaReadAt: null },
  ] satisfies Partial<CodexAccount>[])(
    'filters Codex stale next/order entry with %j without inventing a replacement',
    (patch) => {
      const raw = codexSnapshot({
        accounts: [
          codexAccount('active', { selected: true }),
          codexAccount('retry', patch),
          codexAccount('spare'),
        ],
        selectedId: 'active',
        nextId: 'retry',
        order: ['retry', 'spare'],
      });
      const page = codexView(raw, { locale: 'en', now, settings: defaults });
      expect(page.next).toBeNull();
      expect(page.order.map((entry) => entry.id)).toEqual(['spare']);
      expect(page.rows.find((row) => row.id === 'retry')!.next).toBe(false);
      expect(
        page.rows.find((row) => row.id === 'retry')!.status.label,
      ).not.toBe('Next');
    },
  );

  it('retains native order and valid recommendations in a read-only demo', () => {
    const raw = codexSnapshot({ demo: true });
    raw.capabilities.switchAccount = codexCapability('vaultUnavailable');
    raw.accounts.forEach((account) => {
      account.switchable = codexCapability('vaultUnavailable');
    });
    const page = codexView(raw, { locale: 'en', now, settings: defaults });
    expect(page.next?.id).toBe(raw.nextId);
    expect(page.order.map((entry) => entry.id)).toEqual(
      raw.order.filter((id) => id !== raw.selectedId),
    );
  });
});

describe('targeted Claude reauthentication', () => {
  it('preserves account intent during begin, failed completion and retry; cancellation clears it for Add', async () => {
    const raw = snapshot();
    const call = vi.fn<Bridge['call']>().mockResolvedValue(raw);
    const controller = createController({
      call,
      subscribe: async () => () => {},
    });
    try {
      await controller.start();
      call.mockResolvedValueOnce({
        id: 'retry-session',
        url: 'https://example.invalid/authorize',
      });
      await controller.beginLogin('b');
      expect(call).toHaveBeenLastCalledWith('begin_login', { accountId: 'b' });
      expect(get(controller).login.accountId).toBe('b');
      call.mockRejectedValueOnce('loginIdentityMismatch');
      await controller.finishLogin('wrong-owner#state');
      expect(call).toHaveBeenLastCalledWith('finish_login', {
        id: 'retry-session',
        code: 'wrong-owner#state',
      });
      expect(get(controller).login).toMatchObject({
        open: true,
        accountId: 'b',
        working: false,
      });
      expect(
        get(controller).snapshot?.accounts.map((account) => account.id),
      ).toEqual(['a', 'b']);
      expect(get(controller).login.error).toBeTruthy();
      call.mockResolvedValueOnce(raw).mockResolvedValueOnce({
        id: 'retry-session-2',
        url: 'https://example.invalid/authorize',
      });
      await controller.restartLogin();
      expect(call).toHaveBeenNthCalledWith(
        call.mock.calls.length - 1,
        'cancel_login',
        { id: 'retry-session' },
      );
      expect(call).toHaveBeenLastCalledWith('begin_login', { accountId: 'b' });
      expect(get(controller).login).toMatchObject({
        open: true,
        accountId: 'b',
        session: { id: 'retry-session-2' },
        error: null,
      });
      call.mockResolvedValueOnce(raw);
      await controller.cancelLogin();
      expect(get(controller).login.accountId).toBeNull();
      call.mockResolvedValueOnce({
        id: 'add-session',
        url: 'https://example.invalid/authorize',
      });
      await controller.beginLogin();
      expect(call).toHaveBeenLastCalledWith('begin_login');
      expect(get(controller).login.accountId).toBeNull();
    } finally {
      controller.dispose();
    }
  });

  it('rejects a missing target without opening login or sending a native command', async () => {
    const call = vi.fn<Bridge['call']>().mockResolvedValue(snapshot());
    const controller = createController({
      call,
      subscribe: async () => () => {},
    });
    try {
      await controller.start();
      const before = call.mock.calls.length;
      await controller.beginLogin('deleted-account');
      expect(call.mock.calls.length).toBe(before);
      expect(get(controller).login).toMatchObject({
        open: false,
        accountId: null,
      });
    } finally {
      controller.dispose();
    }
  });
  it('does not reopen a canceled restart after the old session cleanup resolves', async () => {
    const raw = snapshot();
    const call = vi.fn<Bridge['call']>().mockResolvedValue(raw);
    const controller = createController({
      call,
      subscribe: async () => () => {},
    });
    let finishCleanup!: (raw: unknown) => void;
    try {
      await controller.start();
      call.mockResolvedValueOnce({
        id: 'old-session',
        url: 'https://example.invalid/authorize',
      });
      await controller.beginLogin('b');
      call.mockReturnValueOnce(
        new Promise((resolve) => {
          finishCleanup = resolve;
        }),
      );
      const restarting = controller.restartLogin();
      expect(get(controller).login).toMatchObject({
        accountId: 'b',
        session: null,
        working: true,
      });
      await controller.cancelLogin();
      finishCleanup(raw);
      await restarting;
      expect(get(controller).login).toMatchObject({
        open: false,
        accountId: null,
        session: null,
      });
      expect(
        call.mock.calls.filter(([command]) => command === 'begin_login'),
      ).toHaveLength(1);
    } finally {
      controller.dispose();
    }
  });
  it('retries rejected session cleanup before reopening the browser for the same saved target', async () => {
    const raw = snapshot();
    const call = vi.fn<Bridge['call']>().mockResolvedValue(raw);
    const controller = createController({
      call,
      subscribe: async () => () => {},
    });
    let finishCleanup!: (raw: unknown) => void;
    try {
      await controller.start();
      call.mockResolvedValueOnce({
        id: 'old-session',
        url: 'https://example.invalid/authorize',
      });
      await controller.beginLogin('b');
      call.mockRejectedValueOnce('providerError');
      await controller.restartLogin();
      expect(get(controller).login).toMatchObject({
        open: true,
        accountId: 'b',
        session: { id: 'old-session' },
        working: false,
      });
      expect(get(controller).login.error).toBeTruthy();
      expect(
        call.mock.calls.filter(([command]) => command === 'begin_login'),
      ).toHaveLength(1);
      call
        .mockReturnValueOnce(
          new Promise((resolve) => {
            finishCleanup = resolve;
          }),
        )
        .mockResolvedValueOnce({
          id: 'new-session',
          url: 'https://example.invalid/authorize',
        });
      const restarting = controller.restartLogin();
      expect(call).toHaveBeenLastCalledWith('cancel_login', {
        id: 'old-session',
      });
      expect(
        call.mock.calls.filter(([command]) => command === 'begin_login'),
      ).toHaveLength(1);
      finishCleanup(raw);
      await restarting;
      expect(call.mock.calls.slice(-2)).toEqual([
        ['cancel_login', { id: 'old-session' }],
        ['begin_login', { accountId: 'b' }],
      ]);
      expect(get(controller).login).toMatchObject({
        open: true,
        accountId: 'b',
        session: { id: 'new-session' },
        working: false,
        error: null,
      });
    } finally {
      controller.dispose();
    }
  });
});

describe('automatic return status reports facts consistently', () => {
  it.each(['claude', 'codex'] as const)(
    'explains exact weekly headroom ceiling and strict priority ties for %s',
    (provider) => {
      vi.useFakeTimers({ toFake: ['Date'] });
      vi.setSystemTime(now * 1000);
      const claude = snapshot();
      const codex = codexSnapshot();
      const pref = {
        ...defaultAutomation,
        threshold: 95,
        priority: 1,
        returnOnFiveHourReset: true,
      };
      claude.accounts[0].automation = { ...defaultAutomation, priority: 1 };
      claude.accounts[1].automation = pref;
      claude.accounts[1].usage!.weeklyModel = 92;
      codex.accounts[0].automation = { ...defaultAutomation, priority: 1 };
      codex.accounts[2].automation = pref;
      codex.accounts[2].quota!.limits[0].secondary!.usedPercent = 92;
      const page = () =>
        provider === 'claude'
          ? claudeView(claude, { locale: 'en', now })
          : codexView(codex, { locale: 'en', now, settings: defaults });
      expect(
        page().returnNotes.some((note) =>
          note.text.includes(
            '90% used required for return (95% threshold minus 5 points)',
          ),
        ),
      ).toBe(true);
      claude.accounts[1].usage!.weeklyModel = 90;
      codex.accounts[2].quota!.limits[0].secondary!.usedPercent = 90;
      const notes = page().returnNotes.map((note) => note.text);
      expect(
        notes.some((text) =>
          text.includes('Equal priorities do not trigger returns'),
        ),
      ).toBe(true);
      expect(notes.some((text) => text.includes('weekly usage exceeds'))).toBe(
        false,
      );
    },
  );
  it.each(['claude', 'codex'] as const)(
    'shows no opt-in, separate weekly/auth blockers and confirmed-current state for %s',
    (provider) => {
      vi.useFakeTimers({ toFake: ['Date'] });
      vi.setSystemTime(now * 1000);
      const claude = snapshot();
      const codex = codexSnapshot();
      const page = () =>
        provider === 'claude'
          ? claudeView(claude, { locale: 'en', now })
          : codexView(codex, { locale: 'en', now, settings: defaults });
      expect(page().returnNotes[0].text).toContain(
        'No accounts are set to return after reset',
      );
      const preference = {
        ...defaultAutomation,
        priority: 1,
        returnOnFiveHourReset: true,
      };
      if (provider === 'claude') {
        claude.accounts[0].automation = preference;
        claude.accounts[0].usage!.weeklyModel = 100;
        claude.accounts[1].automation = preference;
        claude.accounts[1].signInRequired = true;
      } else {
        codex.accounts[0].automation = preference;
        codex.accounts[0].quota!.limits[0].secondary!.usedPercent = 100;
        codex.accounts[2].automation = preference;
        codex.accounts[2].needsSignIn = true;
      }
      const notes = page().returnNotes.map((note) => note.text);
      expect(
        notes.some((text) =>
          text.includes('A 5-hour reset does not restore weekly quota'),
        ),
      ).toBe(true);
      expect(
        notes.some((text) =>
          text.includes('sign in again before automatic returns'),
        ),
      ).toBe(true);
    },
  );
});
