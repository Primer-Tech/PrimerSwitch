import { get } from 'svelte/store';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createCodexController,
  codexError,
  type CodexController,
} from './codex-controller';
import type { CodexBridge } from './codex-bridge';
import { codexSnapshot, codexCapability } from '../test/codex-fixtures';
import { codexSnapshotSchema } from './codex-types';
const controllers: CodexController[] = [];
afterEach(() => {
  controllers.splice(0).forEach((c) => c.dispose());
  vi.useRealTimers();
});
function setup(raw = codexSnapshot()) {
  let event: (raw: unknown) => void = () => {};
  const call = vi.fn<CodexBridge['call']>().mockResolvedValue(raw),
    unlisten = vi.fn();
  const c = createCodexController({
    call,
    subscribe: async (cb) => {
      event = cb;
      return unlisten;
    },
  });
  controllers.push(c);
  return { c, call, unlisten, event: (value: unknown) => event(value) };
}
function deferred() {
  let resolve!: (value: unknown) => void;
  const promise = new Promise<unknown>((r) => (resolve = r));
  return { promise, resolve };
}
const login = { id: 'login-fixture', status: 'waiting', error: null };
const preparation = () => ({
  id: 'prepare-fixture',
  accountId: 'codex-b',
  expiresAt: Math.floor(Date.now() / 1000) + 60,
});
describe('Codex ownership, managed completion and redacted IPC', () => {
  it('keeps every bucket and nullable permission without a Claude quota projection', () => {
    const raw = codexSnapshot();
    expect(codexSnapshotSchema.parse(raw)).toEqual(raw);
    expect(raw.accounts[0].quota?.limits).toHaveLength(2);
    expect(raw.accounts[0].quota?.ordinaryUsageAllowed).toBeNull();
    expect(
      codexSnapshotSchema.safeParse({ ...raw, accessToken: 'secret-sentinel' })
        .success,
    ).toBe(false);
    expect(
      codexSnapshotSchema.safeParse({ ...raw, error: 'Bearer secret-sentinel' })
        .success,
    ).toBe(false);
  });
  it('retains cached data and hides arbitrary native errors and credential-shaped events', async () => {
    const { c, call, event } = setup();
    await c.start();
    call.mockRejectedValueOnce('Bearer secret-sentinel');
    await c.action('codex_refresh_account', { id: 'codex-a' });
    expect(get(c).error).toBe('actionFailed');
    event({
      ...codexSnapshot({ revision: 9 }),
      credentials: { token: 'secret-sentinel' },
    });
    expect(get(c).snapshot?.revision).toBe(1);
    expect(get(c).error).toBe('unsafeData');
    expect(codexError('clientsRunning')).toBe('clientsRunning');
    expect(codexError({ token: 'secret-sentinel' })).toBe('actionFailed');
  });
  it('reconciles late command replies against the latest event revision', async () => {
    const { c, call, event } = setup();
    await c.start();
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    const action = c.action('codex_import_current');
    event(codexSnapshot({ revision: 7 }));
    result.resolve(codexSnapshot({ revision: 2 }));
    await action;
    expect(get(c).snapshot?.revision).toBe(7);
    expect(get(c).pending).toBeNull();
  });
  it('discovers once, then uses cache-only fallback reads and cleans up', async () => {
    vi.useFakeTimers();
    const { c, call, unlisten } = setup();
    await c.start();
    await vi.advanceTimersByTimeAsync(15000);
    expect(
      call.mock.calls.filter(([name]) => name === 'codex_discover'),
    ).toHaveLength(1);
    expect(
      call.mock.calls.filter(([name]) => name === 'get_codex_snapshot'),
    ).toHaveLength(4);
    expect(
      call.mock.calls.some(([name]) => name === 'codex_refresh_account'),
    ).toBe(false);
    c.dispose();
    expect(unlisten).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(5000);
    expect(call).toHaveBeenCalledTimes(5);
  });
  it('blocks demo, unavailable capabilities, inactive refresh and overlapping actions', async () => {
    const { c, call, event } = setup();
    await c.start();
    expect(await c.action('codex_refresh_account', { id: 'codex-b' })).toBe(
      false,
    );
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    const first = c.action('codex_import_current');
    expect(await c.action('codex_delete_account', { id: 'codex-b' })).toBe(
      false,
    );
    result.resolve(codexSnapshot({ revision: 2 }));
    await first;
    const restricted = codexSnapshot({ revision: 3 });
    restricted.capabilities.loginBrowser = codexCapability('unsupportedStore');
    restricted.capabilities.refreshQuota = codexCapability('unsupportedStore');
    event(restricted);
    await c.beginLogin();
    expect(await c.action('codex_refresh_account', { id: 'codex-a' })).toBe(
      false,
    );
    event(codexSnapshot({ revision: 4, demo: true }));
    expect(await c.action('codex_import_current')).toBe(false);
    expect(call).toHaveBeenCalledTimes(3);
  });
  it('never discovers or mutates a demo snapshot', async () => {
    const { c, call } = setup(codexSnapshot({ demo: true }));
    await c.start();
    await c.beginLogin();
    expect(call).toHaveBeenCalledTimes(1);
  });
  it('completes managed browser login via its own poll without a pasted-code command', async () => {
    vi.useFakeTimers();
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(login);
    await c.beginLogin();
    expect(await c.action('codex_import_current')).toBe(false);
    call.mockResolvedValueOnce(
      codexSnapshot({ revision: 2, login: { ...login, status: 'complete' } }),
    );
    await vi.advanceTimersByTimeAsync(1000);
    expect(call).toHaveBeenLastCalledWith('codex_poll_login', { id: login.id });
    expect(get(c).login.open).toBe(false);
    expect(get(c).notice).toBe('loginComplete');
  });
  it('retries shared-owner contention without ending managed login polling', async () => {
    vi.useFakeTimers();
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(login);
    await c.beginLogin();
    call.mockRejectedValueOnce('busy');
    await vi.advanceTimersByTimeAsync(1000);
    expect(get(c).login.error).toBeNull();
    expect(get(c).login.open).toBe(true);
    call.mockResolvedValueOnce(
      codexSnapshot({ revision: 2, login: { ...login, status: 'complete' } }),
    );
    await vi.advanceTimersByTimeAsync(1000);
    expect(get(c).notice).toBe('loginComplete');
  });
  it('cancels a managed session returned after cancellation or disposal', async () => {
    const { c, call } = setup();
    await c.start();
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    const begin = c.beginLogin();
    await c.cancelLogin();
    c.dispose();
    result.resolve(login);
    await begin;
    expect(call).toHaveBeenLastCalledWith('codex_cancel_login', {
      id: login.id,
    });
  });
  it('ignores a late completion poll after cancel, retaining newer cache state', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(login);
    await c.beginLogin();
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    const poll = c.pollLogin();
    call.mockResolvedValueOnce(codexSnapshot({ revision: 3 }));
    await c.cancelLogin();
    result.resolve(
      codexSnapshot({ revision: 8, login: { ...login, status: 'complete' } }),
    );
    await poll;
    expect(get(c).snapshot?.revision).toBe(3);
    expect(get(c).login.open).toBe(false);
    expect(get(c).notice).toBeNull();
  });
  it.each(['managedLogin', 'backendVerified'] as const)(
    'permits durable %s evidence without claiming fresh quota verification',
    async (evidence) => {
      const raw = codexSnapshot();
      raw.accounts[1].identityVerified = false;
      raw.accounts[1].identityEvidence = evidence;
      const { c, call } = setup(raw);
      await c.start();
      call.mockResolvedValueOnce(preparation());
      await c.prepareSwitch(get(c).snapshot!.accounts[1]);
      expect(call).toHaveBeenLastCalledWith('codex_prepare_switch', {
        id: 'codex-b',
      });
      expect(get(c).snapshot?.accounts[1].identityVerified).toBe(false);
      expect(get(c).preparation.value?.accountId).toBe('codex-b');
    },
  );
  it('refuses forged enabled selection with claims-only evidence', async () => {
    const raw = codexSnapshot();
    raw.accounts[1].identityVerified = true;
    raw.accounts[1].identityEvidence = 'claimsOnly';
    const { c, call } = setup(raw);
    await c.start();
    await c.prepareSwitch(get(c).snapshot!.accounts[1]);
    expect(
      call.mock.calls.some(([name]) => name === 'codex_prepare_switch'),
    ).toBe(false);
    expect(get(c).preparation.open).toBe(false);
  });
  it('requires acknowledgment and a fresh owner-bound preparation', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(preparation());
    await c.prepareSwitch(get(c).snapshot!.accounts[1]);
    expect(await c.applySwitch(false)).toBe(false);
    expect(call).toHaveBeenCalledTimes(3);
    call.mockResolvedValueOnce(
      codexSnapshot({ revision: 2, selectedId: 'codex-b' }),
    );
    expect(await c.applySwitch(true)).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_apply_switch', {
      preparationId: 'prepare-fixture',
      clientsClosedAcknowledged: true,
    });
    expect(get(c).notice).toBe('switchComplete');
  });
  it('rejects mismatched and expired preparation without writing', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce({ ...preparation(), accountId: 'wrong' });
    await c.prepareSwitch(get(c).snapshot!.accounts[1]);
    expect(get(c).preparation.error).toBe('identityMismatch');
    expect(await c.applySwitch(true)).toBe(false);
    call.mockResolvedValueOnce({ ...preparation(), expiresAt: 0 });
    await c.prepareSwitch(get(c).snapshot!.accounts[1]);
    expect(await c.applySwitch(true)).toBe(false);
    expect(get(c).preparation.error).toBe('invalidPreparation');
    expect(call).toHaveBeenLastCalledWith('codex_cancel_preparation', {
      id: 'prepare-fixture',
    });
    expect(
      call.mock.calls.some(([name]) => name === 'codex_apply_switch'),
    ).toBe(false);
  });
  it('cancels a valid native preparation by receipt and retains cache on blocked cleanup', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(preparation());
    await c.prepareSwitch(get(c).snapshot!.accounts[1]);
    call.mockRejectedValueOnce('externalChange');
    await c.dismissPreparation();
    expect(call).toHaveBeenLastCalledWith('codex_cancel_preparation', {
      id: 'prepare-fixture',
    });
    expect(get(c).preparation.open).toBe(false);
    expect(get(c).error).toBe('externalChange');
    expect(get(c).snapshot?.selectedId).toBe('codex-a');
    expect(get(c).notice).toBeNull();
  });
  it('cancels a held preparation on disposal without applying it', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(preparation());
    await c.prepareSwitch(get(c).snapshot!.accounts[1]);
    c.dispose();
    expect(call).toHaveBeenLastCalledWith('codex_cancel_preparation', {
      id: 'prepare-fixture',
    });
    expect(
      call.mock.calls.some(([name]) => name === 'codex_apply_switch'),
    ).toBe(false);
  });
  it('ignores a canceled preparation response and never claims a native guard failure switched', async () => {
    const { c, call } = setup();
    await c.start();
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    const pending = c.prepareSwitch(get(c).snapshot!.accounts[1]);
    c.dismissPreparation();
    result.resolve(preparation());
    await pending;
    expect(get(c).preparation.open).toBe(false);
    expect(call).toHaveBeenLastCalledWith('codex_cancel_preparation', {
      id: 'prepare-fixture',
    });
    call.mockResolvedValueOnce(preparation());
    await c.prepareSwitch(get(c).snapshot!.accounts[1]);
    call.mockRejectedValueOnce('clientsRunning');
    expect(await c.applySwitch(true)).toBe(false);
    expect(get(c).snapshot?.selectedId).toBe('codex-a');
    expect(get(c).preparation.error).toBe('clientsRunning');
    expect(get(c).preparation.value).toBeNull();
    expect(get(c).notice).toBeNull();
  });
});
