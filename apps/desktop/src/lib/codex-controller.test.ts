import { get } from 'svelte/store';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createCodexController,
  codexError,
  type CodexController,
} from './codex-controller';
import type { CodexBridge } from './codex-bridge';
import {
  codexAccount,
  codexApiKeyAccount,
  codexCapability,
  codexMainLimit,
  codexSnapshot,
} from '../test/codex-fixtures';
import {
  codexReasonSchema,
  codexSnapshotSchema,
  type CodexSnapshot,
} from './codex-types';
import {
  codexExtraLimits,
  codexLimitState,
  codexMessage,
  codexPlanLabel,
  codexUsingCredits,
  codexWindowLabel,
} from './codex-format';
import { en } from './locales/en';
import { ro } from './locales/ro';
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
  let reject!: (reason: unknown) => void;
  const promise = new Promise<unknown>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}
const commands = (call: ReturnType<typeof setup>['call']) =>
  call.mock.calls.map(([name]) => name);
const login = { id: 'login-fixture', status: 'waiting', error: null };
/** The snapshot the native side returns after switching to `id`. */
function switched(
  id: string,
  revision: number,
  outcome: Partial<NonNullable<CodexSnapshot['lastSwitch']>> = {},
) {
  const raw = codexSnapshot({ revision, selectedId: id });
  raw.accounts.forEach((account) => (account.selected = account.id === id));
  raw.lastSwitch = {
    accountId: id,
    at: Math.floor(Date.now() / 1000),
    daemonRestarted: true,
    otherClients: 0,
    error: null,
    ...outcome,
  };
  return raw;
}
describe('Codex contract and redacted IPC', () => {
  it('saves automation through the native snapshot and preserves busy/demo/unsupported gates', async () => {
    const { c, call, event } = setup();
    await c.start();
    const preference = {
      enabled: false,
      threshold: 81.125,
      priority: 1,
      returnOnFiveHourReset: true,
    };
    const saved = codexSnapshot({ revision: 2, returnHoldUntil: 900 });
    saved.accounts[2].automation = preference;
    saved.accounts[2].effectiveThreshold = 81.125;
    call.mockResolvedValueOnce(saved);
    expect(await c.updateAutomation(saved.accounts[2].id, preference)).toEqual(
      preference,
    );
    expect(call).toHaveBeenLastCalledWith('codex_update_account_automation', {
      id: saved.accounts[2].id,
      automation: preference,
    });
    const before = call.mock.calls.length;
    expect(
      await c.updateAutomation(saved.accounts[2].id, {
        ...preference,
        priority: 100,
      }),
    ).toBeNull();
    event(codexSnapshot({ revision: 3, busy: true }));
    expect(await c.resumeAutomaticReturns()).toBe(false);
    event(codexSnapshot({ revision: 4, demo: true }));
    expect(
      await c.updateAutomation(saved.accounts[2].id, preference),
    ).toBeNull();
    event(codexSnapshot({ revision: 5, availability: 'unsupported' }));
    expect(
      await c.updateAutomation(saved.accounts[2].id, preference),
    ).toBeNull();
    expect(call.mock.calls.length).toBe(before);
  });
  it('accepts older quotas and date-only details but rejects IDs and invalid expiry timestamps', () => {
    const raw = codexSnapshot();
    expect(codexSnapshotSchema.safeParse(raw).success).toBe(true);
    raw.accounts[0].quota!.resetCreditDetails = [
      { expiresAt: null },
      { expiresAt: 1_800_000_000 },
    ];
    expect(codexSnapshotSchema.safeParse(raw).success).toBe(true);
    for (const detail of [
      { expiresAt: -1 },
      { expiresAt: Number.MAX_SAFE_INTEGER },
      { expiresAt: 1_800_000_000, id: 'private-credit-id-sentinel' },
    ]) {
      raw.accounts[0].quota!.resetCreditDetails = [detail];
      expect(codexSnapshotSchema.safeParse(raw).success).toBe(false);
    }
  });
  it('rejects reset keys and arbitrary reset outcomes over IPC', () => {
    const raw = codexSnapshot();
    const account = raw.accounts[0];
    account.reset = {
      ...account.reset,
      idempotencyKey: 'secret-sentinel',
    } as unknown as typeof account.reset;
    expect(codexSnapshotSchema.safeParse(raw).success).toBe(false);
    account.reset = {
      pending: false,
      lastOutcome: 'secret-sentinel',
      usable: codexCapability(),
    } as unknown as typeof account.reset;
    expect(codexSnapshotSchema.safeParse(raw).success).toBe(false);
  });
  it('uses native reset capabilities and blocks cached, demo and concurrent redemptions', async () => {
    const raw = codexSnapshot();
    const { c, call, event } = setup(raw);
    await c.start();
    const before = call.mock.calls.length;
    expect(await c.consumeReset(raw.accounts[0].id)).toBe(false);
    expect(call).toHaveBeenCalledTimes(before);
    raw.revision++;
    raw.accounts[0].reset.usable = codexCapability();
    raw.demo = true;
    event(raw);
    expect(await c.consumeReset(raw.accounts[0].id)).toBe(false);
    raw.demo = false;
    raw.revision++;
    event(raw);
    const pending = deferred();
    call.mockReturnValueOnce(pending.promise);
    const first = c.consumeReset(raw.accounts[0].id);
    expect(await c.consumeReset(raw.accounts[0].id)).toBe(false);
    const result = structuredClone(raw);
    result.revision++;
    result.accounts[0].reset = {
      pending: true,
      lastOutcome: null,
      usable: codexCapability(),
    };
    result.error = 'resetUnconfirmed';
    pending.resolve(result);
    expect(await first).toBe(false);
    expect(get(c).error).toBe('resetUnconfirmed');
    expect(get(c).snapshot?.accounts[0].reset.pending).toBe(true);
  });
  it('parses the seamless-switch snapshot strictly and rejects retired or credential-shaped data', () => {
    const raw = codexSnapshot();
    expect(codexSnapshotSchema.parse(raw)).toEqual(raw);
    expect(
      codexSnapshotSchema.safeParse({ ...raw, accessToken: 'secret-sentinel' })
        .success,
    ).toBe(false);
    expect(
      codexSnapshotSchema.safeParse({ ...raw, error: 'Bearer secret-sentinel' })
        .success,
    ).toBe(false);
    for (const retired of [
      'clientsRunning',
      'processInventoryUnavailable',
      'reconciliationRequired',
      'invalidPreparation',
    ]) {
      expect(codexReasonSchema.safeParse(retired).success).toBe(false);
      expect(Object.keys(en)).not.toContain(`codexReason_${retired}`);
    }
    const legacy = {
      ...raw,
      capabilities: { ...raw.capabilities, manualSwitch: codexCapability() },
    };
    expect(codexSnapshotSchema.safeParse(legacy).success).toBe(false);
    // The next account always comes from the native side, even when there is none.
    const withoutNext: Record<string, unknown> = { ...raw };
    delete withoutNext.nextId;
    expect(codexSnapshotSchema.safeParse(withoutNext).success).toBe(false);
    expect(
      codexSnapshotSchema.safeParse({ ...raw, nextId: null, order: [] })
        .success,
    ).toBe(true);
    // So does the usage order, as a list of account ids.
    const withoutOrder: Record<string, unknown> = { ...raw };
    delete withoutOrder.order;
    expect(codexSnapshotSchema.safeParse(withoutOrder).success).toBe(false);
    expect(
      codexSnapshotSchema.safeParse({ ...raw, order: ['codex-research', ''] })
        .success,
    ).toBe(false);
    expect(
      codexSnapshotSchema.safeParse({ ...raw, order: 'codex-research' })
        .success,
    ).toBe(false);
    const withoutSignIn: Record<string, unknown> = { ...raw.accounts[0] };
    delete withoutSignIn.needsSignIn;
    expect(
      codexSnapshotSchema.safeParse({ ...raw, accounts: [withoutSignIn] })
        .success,
    ).toBe(false);
  });
  it('gives every reason friendly text in English and Romanian', () => {
    for (const reason of codexReasonSchema.options) {
      const english = codexMessage(reason, 'en'),
        romanian = codexMessage(reason, 'ro');
      expect(english).not.toMatch(/codexReason_/);
      expect(romanian).not.toMatch(/codexReason_/);
      expect(romanian).not.toBe(english);
    }
    expect(codexMessage('activeAccount', 'en')).toBe(
      'Switch to another account before removing this one.',
    );
    expect(codexMessage('activeAccount', 'ro')).toBe(
      'Comută pe alt cont înainte să-l ștergi pe acesta.',
    );
    expect(Object.keys(ro).sort()).toEqual(Object.keys(en).sort());
    const codexRomanian = Object.entries(ro)
      .filter(([key]) => key.startsWith('codex'))
      .map(([, text]) => text)
      .join('');
    expect(codexRomanian).not.toMatch(/[ŞşŢţ]/);
    expect(codexRomanian).toMatch(/[șț]/);
  });
  it('retains cached data and hides arbitrary native errors and credential-shaped events', async () => {
    const { c, call, event } = setup();
    await c.start();
    call.mockRejectedValueOnce('Bearer secret-sentinel');
    await c.refreshAccount('codex-studio');
    expect(get(c).error).toBe('actionFailed');
    event({
      ...codexSnapshot({ revision: 9 }),
      credentials: { token: 'secret-sentinel' },
    });
    expect(get(c).snapshot?.revision).toBe(1);
    expect(get(c).error).toBe('unsafeData');
    expect(codexError('switchInProgress')).toBe('switchInProgress');
    expect(codexError(new Error('daemonRestartFailed'))).toBe(
      'daemonRestartFailed',
    );
    expect(codexError('clientsRunning')).toBe('actionFailed');
    expect(codexError({ token: 'secret-sentinel' })).toBe('actionFailed');
  });
  it('reconciles late command replies against the latest event revision', async () => {
    const { c, call, event } = setup();
    await c.start();
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    const action = c.importCurrent();
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
    expect(commands(call).filter((n) => n === 'codex_discover')).toHaveLength(
      1,
    );
    expect(
      commands(call).filter((n) => n === 'get_codex_snapshot'),
    ).toHaveLength(4);
    expect(commands(call)).not.toContain('codex_refresh_account');
    c.dispose();
    expect(unlisten).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(5000);
    expect(call).toHaveBeenCalledTimes(5);
  });
});
describe('Codex one-call switching', () => {
  it('switches with a single native call and derives the notice from lastSwitch', async () => {
    const { c, call, event } = setup();
    await c.start();
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    const target = get(c).snapshot!.accounts[2];
    const pending = c.switchAccount(target);
    expect(get(c).pending).toBe('codex_switch_account');
    expect(get(c).pendingId).toBe('codex-research');
    event(
      codexSnapshot({
        revision: 2,
        busy: true,
        switching: {
          targetId: 'codex-research',
          stage: 'restarting',
          startedAt: Math.floor(Date.now() / 1000),
        },
      }),
    );
    expect(get(c).snapshot?.switching?.stage).toBe('restarting');
    expect(await c.refreshAll()).toBe(false);
    expect(await c.importCurrent()).toBe(false);
    result.resolve(switched('codex-research', 3, { otherClients: 2 }));
    expect(await pending).toBe(true);
    expect(call).toHaveBeenCalledWith('codex_switch_account', {
      id: 'codex-research',
    });
    expect(get(c).notice).toEqual({
      kind: 'switched',
      accountId: 'codex-research',
      name: 'research@example.invalid',
      daemonRestarted: true,
      otherClients: 2,
      warning: null,
      flex: null,
    });
    expect(get(c).snapshot?.selectedId).toBe('codex-research');
    expect(get(c).snapshot?.switching).toBeNull();
    expect(get(c).pending).toBeNull();
    expect(get(c).error).toBeNull();
    expect(
      commands(call).filter((n) => n === 'codex_switch_account'),
    ).toHaveLength(1);
    expect(commands(call)).not.toContain('codex_refresh_all');
  });
  it('reports a partial switch as a warning instead of a failure', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(
      switched('codex-research', 2, {
        daemonRestarted: false,
        error: 'daemonRestartFailed',
      }),
    );
    expect(await c.switchAccount(get(c).snapshot!.accounts[2])).toBe(true);
    expect(get(c).error).toBeNull();
    expect(get(c).notice).toMatchObject({
      kind: 'switched',
      daemonRestarted: false,
      warning: 'daemonRestartFailed',
    });
  });
  it('keeps the selection and a friendly reason when the switch fails', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockRejectedValueOnce('switchInProgress');
    expect(await c.switchAccount(get(c).snapshot!.accounts[2])).toBe(false);
    expect(get(c).error).toBe('switchInProgress');
    expect(get(c).notice).toBeNull();
    expect(get(c).snapshot?.selectedId).toBe('codex-studio');
    call.mockResolvedValueOnce(switched('codex-personal', 4));
    expect(await c.switchAccount(get(c).snapshot!.accounts[2])).toBe(false);
    expect(get(c).error).toBe('actionFailed');
    expect(get(c).notice).toBeNull();
  });
  it('refuses active, blocked, signed-out and demo targets but trusts switchable over evidence', async () => {
    const raw = codexSnapshot();
    raw.accounts[1].switchable = codexCapability('identityUnverified');
    raw.accounts.push(
      codexAccount('codex-expired', {
        needsSignIn: true,
        switchable: codexCapability('signInRequired'),
      }),
      codexAccount('codex-imported', {
        identityEvidence: 'claimsOnly',
        identityVerified: false,
      }),
    );
    const { c, call, event } = setup(raw);
    await c.start();
    const accounts = get(c).snapshot!.accounts;
    expect(await c.switchAccount(accounts[0])).toBe(false);
    expect(await c.switchAccount(accounts[1])).toBe(false);
    expect(await c.switchAccount(accounts[3])).toBe(false);
    expect(commands(call)).not.toContain('codex_switch_account');
    call.mockResolvedValueOnce(switched('codex-imported', 2));
    expect(await c.switchAccount(accounts[4])).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_switch_account', {
      id: 'codex-imported',
    });
    const blocked = codexSnapshot({ revision: 3 });
    blocked.capabilities.switchAccount = codexCapability('vaultUnavailable');
    event(blocked);
    expect(await c.switchAccount(get(c).snapshot!.accounts[2])).toBe(false);
    event(codexSnapshot({ revision: 4, demo: true }));
    expect(await c.switchAccount(get(c).snapshot!.accounts[2])).toBe(false);
    expect(
      commands(call).filter((n) => n === 'codex_switch_account'),
    ).toHaveLength(1);
  });
  it('blocks every other mutation while a switch from elsewhere is running', async () => {
    const { c, call, event } = setup();
    await c.start();
    const before = call.mock.calls.length;
    event(
      codexSnapshot({
        revision: 2,
        switching: {
          targetId: 'codex-personal',
          stage: 'saving',
          startedAt: Math.floor(Date.now() / 1000),
        },
      }),
    );
    expect(await c.refreshAccount('codex-personal')).toBe(false);
    expect(await c.deleteAccount('codex-personal')).toBe(false);
    expect(await c.importSwitcher()).toBe(false);
    await c.beginLogin();
    expect(await c.switchAccount(get(c).snapshot!.accounts[2])).toBe(false);
    expect(call.mock.calls.length).toBe(before);
  });
});
describe('Codex refresh, import and delete', () => {
  it('refreshes any ChatGPT account, refreshes all and imports from Codex Switcher', async () => {
    const raw = codexSnapshot();
    raw.accounts.push(
      codexAccount('codex-api', { authKind: 'apiKey', quota: null }),
    );
    const { c, call, event } = setup(raw);
    await c.start();
    expect(await c.refreshAccount('codex-personal')).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_refresh_account', {
      id: 'codex-personal',
    });
    expect(await c.refreshAccount('codex-api')).toBe(false);
    expect(await c.refreshAccount('missing')).toBe(false);
    expect(await c.refreshAll()).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_refresh_all', undefined);
    expect(await c.importSwitcher()).toBe(false);
    const enabled = codexSnapshot({ revision: 2 });
    enabled.capabilities.importSwitcher = codexCapability();
    event(enabled);
    const imported = codexSnapshot({ revision: 3 });
    imported.accounts.push(
      codexAccount('codex-one'),
      codexAccount('codex-two'),
    );
    call.mockResolvedValueOnce(imported);
    expect(await c.importSwitcher()).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_import_switcher', undefined);
    expect(get(c).notice).toEqual({ kind: 'importedSwitcher', added: 2 });
    call.mockResolvedValueOnce(imported);
    expect(await c.importCurrent()).toBe(true);
    expect(get(c).notice).toEqual({ kind: 'importedCurrent', added: 0 });
  });
  it('never deletes the active account and confirms other deletions', async () => {
    const { c, call } = setup();
    await c.start();
    expect(await c.deleteAccount('codex-studio')).toBe(false);
    expect(commands(call)).not.toContain('codex_delete_account');
    const remaining = codexSnapshot({ revision: 2 });
    remaining.accounts.splice(1, 1);
    call.mockResolvedValueOnce(remaining);
    expect(await c.deleteAccount('codex-personal')).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_delete_account', {
      id: 'codex-personal',
    });
    expect(get(c).notice).toEqual({ kind: 'deleteComplete' });
    call.mockRejectedValueOnce('activeAccount');
    expect(await c.deleteAccount('codex-research')).toBe(false);
    expect(get(c).error).toBe('activeAccount');
  });
  it('never discovers or mutates a demo snapshot', async () => {
    const { c, call } = setup(codexSnapshot({ demo: true }));
    await c.start();
    await c.beginLogin();
    expect(await c.addApiKey('Work key', FAKE_KEY)).toBe(false);
    expect(await c.refreshAll()).toBe(false);
    expect(await c.switchAccount(get(c).snapshot!.accounts[2])).toBe(false);
    expect(call).toHaveBeenCalledTimes(1);
  });
});
const FAKE_KEY = 'sk-proj-SENTINEL-0123456789abcdef';
describe('Codex API keys', () => {
  it('saves a key in one native call and keeps it out of state, notices and errors', async () => {
    const { c, call } = setup();
    await c.start();
    const saved = codexSnapshot({ revision: 2 });
    saved.accounts.push(codexApiKeyAccount());
    call.mockResolvedValueOnce(saved);
    expect(await c.addApiKey('Work key', FAKE_KEY)).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_add_api_key', {
      name: 'Work key',
      key: FAKE_KEY,
    });
    expect(get(c).notice).toEqual({
      kind: 'apiKeyAdded',
      accountId: 'codex-key',
    });
    expect(get(c).pending).toBeNull();
    // The same key again adds no account: nothing changed, and it is said plainly.
    call.mockResolvedValueOnce(
      codexSnapshot({ revision: 3, accounts: saved.accounts }),
    );
    expect(await c.addApiKey('Another name', FAKE_KEY)).toBe(true);
    expect(get(c).notice).toEqual({ kind: 'apiKeyExisting' });
    // Failures show a stable reason or a generic message, never the error's own text.
    call.mockRejectedValueOnce('invalidApiKey');
    expect(await c.addApiKey('Work key', 'sk-short')).toBe(false);
    expect(get(c).error).toBe('invalidApiKey');
    expect(get(c).notice).toBeNull();
    call.mockRejectedValueOnce(new Error(`Bad key ${FAKE_KEY}`));
    expect(await c.addApiKey('Work key', FAKE_KEY)).toBe(false);
    expect(get(c).error).toBe('actionFailed');
    expect(JSON.stringify(get(c))).not.toContain('SENTINEL');
    expect(codexMessage('invalidApiKey', 'en')).toBe(
      en.codexReason_invalidApiKey,
    );
    expect(codexMessage('invalidApiKey', 'ro')).toBe(
      ro.codexReason_invalidApiKey,
    );
  });
  it('is gated like every mutation and never sends empty input', async () => {
    const blocked = codexSnapshot();
    blocked.capabilities.addApiKey = codexCapability('vaultUnavailable');
    const { c, call, event } = setup(blocked);
    await c.start();
    const before = call.mock.calls.length;
    expect(await c.addApiKey('Work key', FAKE_KEY)).toBe(false);
    event(codexSnapshot({ revision: 2 }));
    expect(await c.addApiKey('  ', FAKE_KEY)).toBe(false);
    expect(await c.addApiKey('Work key', ' 	')).toBe(false);
    event(
      codexSnapshot({
        revision: 3,
        switching: {
          targetId: 'codex-personal',
          stage: 'saving',
          startedAt: Math.floor(Date.now() / 1000),
        },
      }),
    );
    expect(await c.addApiKey('Work key', FAKE_KEY)).toBe(false);
    expect(call.mock.calls.length).toBe(before);
  });
  it('shows a pending save and rejects a second one meanwhile', async () => {
    const { c, call } = setup();
    await c.start();
    const pending = deferred();
    call.mockReturnValueOnce(pending.promise);
    const first = c.addApiKey('Work key', FAKE_KEY);
    expect(get(c).pending).toBe('codex_add_api_key');
    expect(
      await c.addApiKey('Other', 'sk-proj-SENTINEL-0123456789abcdeg'),
    ).toBe(false);
    pending.resolve(codexSnapshot({ revision: 2 }));
    expect(await first).toBe(true);
    expect(get(c).pending).toBeNull();
    expect(
      call.mock.calls.filter(([name]) => name === 'codex_add_api_key'),
    ).toHaveLength(1);
  });
  it('rejects an account view that carries more than the last characters of a key', () => {
    const raw = codexSnapshot();
    raw.accounts.push(codexApiKeyAccount());
    expect(codexSnapshotSchema.safeParse(raw).success).toBe(true);
    const leaking = structuredClone(raw);
    leaking.accounts[3].apiKeyTail = FAKE_KEY;
    expect(codexSnapshotSchema.safeParse(leaking).success).toBe(false);
    const extra = structuredClone(raw) as unknown as {
      accounts: Record<string, unknown>[];
    };
    extra.accounts[3].apiKey = FAKE_KEY;
    expect(codexSnapshotSchema.safeParse(extra).success).toBe(false);
  });
});
describe('Codex Flex processing', () => {
  const withKey = (overrides = {}, revision = 1) => {
    const raw = codexSnapshot({ revision });
    raw.accounts.push(codexApiKeyAccount('codex-key', overrides));
    return raw;
  };
  it('turns Flex on or off for an API key in one native call', async () => {
    const { c, call } = setup(withKey());
    await c.start();
    call.mockResolvedValueOnce(withKey({ useFlex: true }, 2));
    expect(await c.setApiKeyFlex('codex-key', true)).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_set_api_key_flex', {
      id: 'codex-key',
      enabled: true,
    });
    expect(get(c).snapshot?.accounts[3].useFlex).toBe(true);
    expect(get(c).pending).toBeNull();
    call.mockResolvedValueOnce(withKey({ useFlex: false }, 3));
    expect(await c.setApiKeyFlex('codex-key', false)).toBe(true);
    expect(call).toHaveBeenLastCalledWith('codex_set_api_key_flex', {
      id: 'codex-key',
      enabled: false,
    });
  });
  it('is refused for a ChatGPT account, an unknown account or a blocked option', async () => {
    const { c, call } = setup(
      withKey({ flex: codexCapability('flexUnsupportedVersion') }),
    );
    await c.start();
    const before = call.mock.calls.length;
    expect(await c.setApiKeyFlex('codex-key', true)).toBe(false);
    expect(await c.setApiKeyFlex('codex-studio', true)).toBe(false);
    expect(await c.setApiKeyFlex('missing', true)).toBe(false);
    expect(call.mock.calls.length).toBe(before);
  });
  it('shows a stable reason when the native side refuses, and keeps the old setting', async () => {
    const { c, call } = setup(withKey());
    await c.start();
    call.mockRejectedValueOnce('flexConfigFailed');
    expect(await c.setApiKeyFlex('codex-key', true)).toBe(false);
    expect(get(c).error).toBe('flexConfigFailed');
    expect(get(c).snapshot?.accounts[3].useFlex).toBe(false);
    call.mockRejectedValueOnce(new Error('could not write the codex config'));
    expect(await c.setApiKeyFlex('codex-key', true)).toBe(false);
    expect(get(c).error).toBe('actionFailed');
  });
  it('carries what a switch did to Flex into its notice', async () => {
    const { c, call, event } = setup(withKey({ useFlex: true }));
    await c.start();
    let revision = 1;
    for (const flex of ['on', 'off', null] as const) {
      // ChatGPT is active before each switch to the key.
      event(withKey({ useFlex: true }, (revision += 2)));
      const raw = withKey({ useFlex: true }, revision + 1);
      raw.selectedId = 'codex-key';
      raw.accounts.forEach((a) => (a.selected = a.id === 'codex-key'));
      raw.lastSwitch = {
        accountId: 'codex-key',
        at: Math.floor(Date.now() / 1000),
        daemonRestarted: true,
        otherClients: 0,
        error: flex === null ? 'flexRestoreFailed' : null,
        ...(flex ? { flex } : {}),
      };
      call.mockResolvedValueOnce(raw);
      expect(await c.switchAccount(get(c).snapshot!.accounts[3])).toBe(true);
      expect(get(c).notice).toMatchObject({
        kind: 'switched',
        flex,
        warning: flex === null ? 'flexRestoreFailed' : null,
      });
    }
  });
  it('accepts the Flex fields and rejects unknown ones', () => {
    const raw = withKey({ useFlex: true });
    raw.lastSwitch = {
      accountId: 'codex-key',
      at: 1,
      daemonRestarted: true,
      otherClients: 0,
      error: null,
      flex: 'on',
    };
    expect(codexSnapshotSchema.safeParse(raw).success).toBe(true);
    const unknown = structuredClone(raw) as unknown as {
      lastSwitch: Record<string, unknown>;
    };
    unknown.lastSwitch.flex = 'fast';
    expect(codexSnapshotSchema.safeParse(unknown).success).toBe(false);
  });
});
describe('Codex managed browser sign-in', () => {
  it('completes managed browser login via its own poll without a pasted-code command', async () => {
    vi.useFakeTimers();
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce(login);
    await c.beginLogin();
    expect(await c.importCurrent()).toBe(false);
    call.mockResolvedValueOnce(
      codexSnapshot({ revision: 2, login: { ...login, status: 'complete' } }),
    );
    await vi.advanceTimersByTimeAsync(1000);
    expect(call).toHaveBeenLastCalledWith('codex_poll_login', { id: login.id });
    expect(get(c).login.open).toBe(false);
    expect(get(c).notice).toEqual({ kind: 'loginComplete', accountId: null });
  });
  it('labels a sign-in-again flow with the saved account', async () => {
    vi.useFakeTimers();
    const raw = codexSnapshot();
    raw.accounts[2].needsSignIn = true;
    const { c, call } = setup(raw);
    await c.start();
    call.mockResolvedValueOnce(login);
    await c.beginLogin(get(c).snapshot!.accounts[2]);
    expect(get(c).login.accountId).toBe('codex-research');
    expect(call).toHaveBeenLastCalledWith('codex_begin_login');
    call.mockResolvedValueOnce(
      codexSnapshot({ revision: 2, login: { ...login, status: 'complete' } }),
    );
    await vi.advanceTimersByTimeAsync(1000);
    expect(get(c).notice).toEqual({
      kind: 'loginComplete',
      accountId: 'codex-research',
    });
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
    expect(get(c).notice).toMatchObject({ kind: 'loginComplete' });
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
});
describe('Codex view derivations', () => {
  const now = 1_800_000_000;
  it('derives limits from the main limit only and frees at the latest exhausted reset', () => {
    const [studio, personal, research] = codexSnapshot().accounts;
    expect(codexLimitState(studio).limited).toBe(false);
    expect(codexLimitState(personal).limited).toBe(true);
    expect(codexLimitState(research).limited).toBe(false);
    const both = codexAccount('codex-both', {
      quota: {
        ordinaryUsageAllowed: true,
        resetCreditsAvailable: null,
        limits: [
          codexMainLimit(100, 100, {
            primary: {
              usedPercent: 100,
              windowDurationMins: 300,
              resetsAt: now + 600,
            },
            secondary: {
              usedPercent: 104,
              windowDurationMins: 10080,
              resetsAt: now + 9000,
            },
          }),
        ],
      },
    });
    expect(codexLimitState(both)).toEqual({
      limited: true,
      freesAt: now + 9000,
    });
    const unknownReset = codexAccount('codex-unknown', {
      quota: {
        ordinaryUsageAllowed: true,
        resetCreditsAvailable: null,
        limits: [
          codexMainLimit(100, 10, {
            primary: {
              usedPercent: 100,
              windowDurationMins: 300,
              resetsAt: null,
            },
          }),
        ],
      },
    });
    expect(codexLimitState(unknownReset)).toEqual({
      limited: true,
      freesAt: null,
    });
    const blocked = codexAccount('codex-blocked', {
      quota: {
        ordinaryUsageAllowed: false,
        resetCreditsAvailable: null,
        limits: [codexMainLimit(5, 5)],
      },
    });
    expect(codexLimitState(blocked)).toEqual({ limited: true, freesAt: null });
    const reached = codexAccount('codex-reached', {
      quota: {
        ordinaryUsageAllowed: null,
        resetCreditsAvailable: null,
        limits: [codexMainLimit(70, 20, { rateLimitReachedType: 'primary' })],
      },
    });
    expect(codexLimitState(reached).limited).toBe(true);
    const extraOnly = codexAccount('codex-extra', {
      quota: {
        ordinaryUsageAllowed: true,
        resetCreditsAvailable: null,
        limits: [
          codexMainLimit(10, 10),
          {
            ...codexMainLimit(100, 100),
            key: 'bucket:code-review',
            limitId: 'code-review',
            limitName: 'Code review',
          },
        ],
      },
    });
    expect(codexLimitState(extraOnly).limited).toBe(false);
    expect(codexLimitState(codexAccount('codex-new', { quota: null }))).toEqual(
      { limited: false, freesAt: null },
    );
  });
  it('marks only main windows past 100% with purchased credits as using credits', () => {
    const withCredits = (
      weekly: number,
      credits: NonNullable<ReturnType<typeof codexMainLimit>['credits']>,
    ) =>
      codexAccount('codex-credits', {
        quota: {
          ordinaryUsageAllowed: true,
          resetCreditsAvailable: null,
          limits: [codexMainLimit(20, weekly, { credits })],
        },
      });
    const flag = { hasCredits: true, unlimited: false, balance: null };
    const weekly = (account: ReturnType<typeof codexAccount>) =>
      account.quota!.limits[0].secondary;
    const over = withCredits(105, flag);
    expect(codexUsingCredits(over, weekly(over))).toBe(true);
    // The 5-hour window of the same account is within the plan.
    expect(codexUsingCredits(over, over.quota!.limits[0].primary)).toBe(false);
    for (const credits of [
      { hasCredits: false, unlimited: false, balance: '3.50' },
      { hasCredits: false, unlimited: true, balance: null },
    ]) {
      const account = withCredits(101, credits);
      expect(codexUsingCredits(account, weekly(account))).toBe(true);
    }
    // Exactly 100% is the plan limit itself, not credit usage.
    const full = withCredits(100, flag);
    expect(codexUsingCredits(full, weekly(full))).toBe(false);
    // Without credits a reading past 100% gets no hint.
    for (const credits of [
      { hasCredits: false, unlimited: false, balance: null },
      { hasCredits: false, unlimited: false, balance: '' },
    ]) {
      const account = withCredits(104, credits);
      expect(codexUsingCredits(account, weekly(account))).toBe(false);
    }
    expect(codexUsingCredits(over, null)).toBe(false);
    // Such an account is limited, so the native side never ranks it next.
    expect(codexLimitState(over).limited).toBe(true);
  });
  it('labels windows from real durations and known plans', () => {
    expect(codexWindowLabel(300, 'short', 'en')).toBe('5 hours');
    expect(codexWindowLabel(10080, 'long', 'en')).toBe('Weekly');
    expect(codexWindowLabel(1440, 'short', 'en')).toBe('Daily');
    expect(codexWindowLabel(120, 'short', 'en')).toBe('2 hours');
    expect(codexWindowLabel(4320, 'long', 'en')).toBe('3 days');
    expect(codexWindowLabel(null, 'short', 'en')).toBe('Short window');
    expect(codexWindowLabel(300, 'short', 'ro')).toBe('5 ore');
    expect(codexWindowLabel(10080, 'long', 'ro')).toBe('Săptămânal');
    expect(codexPlanLabel('prolite', 'en')).toBe('Pro Lite');
    expect(codexPlanLabel('pro_lite', 'en')).toBe('Pro Lite');
    expect(codexPlanLabel('plus', 'ro')).toBe('Plus');
    expect(codexPlanLabel('free', 'ro')).toBe('Gratuit');
    expect(codexPlanLabel('future-plan', 'en')).toBeNull();
    expect(codexPlanLabel(null, 'en')).toBeNull();
    expect(
      codexExtraLimits(codexSnapshot().accounts[0]).map((l) => l.key),
    ).toEqual(['bucket:code-review']);
  });
});
