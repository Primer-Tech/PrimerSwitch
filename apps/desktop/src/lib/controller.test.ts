import { get } from 'svelte/store';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  codeMessage,
  createController,
  errorText,
  safeError,
  type Controller,
} from './controller';
import { snapshot } from '../test/fixtures';
import { snapshotSchema, defaults } from './types';
import type { Bridge } from './bridge';
import { t } from './i18n';

const controllers: Controller[] = [];
afterEach(() => {
  controllers.splice(0).forEach((c) => c.dispose());
  vi.useRealTimers();
});
function setup() {
  let event: (snapshot: unknown) => void = () => {};
  const unlisten = vi.fn();
  const call = vi.fn<Bridge['call']>().mockResolvedValue(snapshot());
  const c = createController({
    call,
    subscribe: async (cb) => {
      event = cb;
      return unlisten;
    },
  });
  controllers.push(c);
  return { c, call, event: (raw: unknown) => event(raw), unlisten };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe('redacted IPC and runtime reconciliation', () => {
  it('keeps the latest revision when an old command response arrives', async () => {
    const { c, call, event } = setup();
    await c.start();
    const result = deferred<unknown>();
    call.mockReturnValueOnce(result.promise);
    const action = c.action('switch_account', { id: 'b' });
    event(snapshot({ revision: 7, activeId: 'b' }));
    result.resolve(snapshot({ revision: 2 }));
    await action;
    expect(get(c).snapshot?.revision).toBe(7);
    expect(get(c).pending).toBeNull();
  });
  it('blocks duplicate commands and busy/demo mutations', async () => {
    const { c, call, event } = setup();
    await c.start();
    const result = deferred<unknown>();
    call.mockReturnValueOnce(result.promise);
    const first = c.action('refresh_account', { id: 'b' });
    expect(await c.action('delete_account', { id: 'b' })).toBe(false);
    result.resolve(snapshot({ revision: 2 }));
    await first;
    event(snapshot({ revision: 3, busy: true }));
    expect(await c.action('refresh_all')).toBe(false);
    event(snapshot({ revision: 4, demo: true }));
    expect(await c.action('import_current')).toBe(false);
    expect(call).toHaveBeenCalledTimes(2);
  });
  it('retains cached data on errors and rejects credential-bearing payloads', async () => {
    const { c, call, event } = setup();
    await c.start();
    call.mockRejectedValueOnce('Serviciul nu este disponibil.');
    await c.action('refresh_all');
    expect(get(c).snapshot?.accounts[0].usage?.fiveHour.utilization).toBe(68);
    expect(get(c).error).toBe('Serviciul nu este disponibil.');
    event({
      ...snapshot({ revision: 3 }),
      credentials: { access_token: 'never-display' },
    });
    expect(get(c).snapshot?.revision).toBe(1);
    expect(get(c).error).toContain('securely');
    expect(
      snapshotSchema.safeParse({ ...snapshot(), accessToken: 'forbidden' })
        .success,
    ).toBe(false);
    expect(safeError({ access_token: 'forbidden' })).not.toContain('forbidden');
    expect(safeError('Bearer private-value')).not.toContain('private-value');
  });
  it('uses only pure snapshot reads in fallback polling and cleans up listeners', async () => {
    vi.useFakeTimers();
    const { c, call, unlisten } = setup();
    await c.start();
    await vi.advanceTimersByTimeAsync(15000);
    expect(
      call.mock.calls.every(([command]) => command === 'get_snapshot'),
    ).toBe(true);
    expect(call).toHaveBeenCalledTimes(4);
    c.dispose();
    expect(unlisten).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(10000);
    expect(call).toHaveBeenCalledTimes(4);
  });
  it('cancels a login returned after its modal was closed', async () => {
    const { c, call } = setup();
    await c.start();
    const result = deferred<unknown>();
    call.mockReturnValueOnce(result.promise);
    const begin = c.beginLogin();
    await c.cancelLogin();
    result.resolve({
      id: 'late-login',
      url: 'https://platform.claude.com/oauth/authorize',
    });
    await begin;
    expect(call).toHaveBeenLastCalledWith('cancel_login', { id: 'late-login' });
    expect(get(c).login.open).toBe(false);
  });
  it('validates pasted completion and does not allow parallel login actions', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce({
      id: 'login-1',
      url: 'https://platform.claude.com/oauth/authorize',
    });
    await c.beginLogin();
    await c.finishLogin('wrong');
    expect(get(c).login.error).toContain('#');
    expect(await c.action('refresh_all')).toBe(false);
    call.mockResolvedValueOnce(snapshot({ revision: 2 }));
    await c.finishLogin(' code # state ');
    expect(call).toHaveBeenLastCalledWith('finish_login', {
      id: 'login-1',
      code: 'code#state',
    });
    expect(get(c).login.open).toBe(false);
  });
  it('does not reopen a canceled login when completion fails late', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce({
      id: 'login-1',
      url: 'https://platform.claude.com/oauth/authorize',
    });
    await c.beginLogin();
    const result = deferred<unknown>();
    call.mockReturnValueOnce(result.promise);
    const completion = c.finishLogin('code#state');
    await c.cancelLogin();
    result.resolve(snapshot({ revision: 2 }));
    await completion;
    expect(get(c).login.open).toBe(false);
  });
  it('previews an import before applying its opaque preview ID', async () => {
    const { c, call } = setup();
    await c.start();
    call.mockResolvedValueOnce({
      id: 'preview-1',
      valid: 2,
      invalid: 1,
      names: ['Personal', 'Studio'],
    });
    await c.action('preview_legacy_import');
    expect(get(c).preview?.invalid).toBe(1);
    call.mockResolvedValueOnce(snapshot({ revision: 2 }));
    await c.action('apply_legacy_import', { previewId: 'preview-1' });
    expect(call).toHaveBeenLastCalledWith('apply_legacy_import', {
      previewId: 'preview-1',
    });
  });
  it('maps stable error codes and fixed variable names to catalog text', () => {
    expect(safeError('providerError', 'ro')).toBe(t('ro', 'providerError'));
    // Rust replies equal the catalog, so wording about sign-ins is not swallowed.
    expect(safeError(t('en', 'signInRequired'), 'ro')).toBe(
      t('ro', 'signInRequired'),
    );
    expect(safeError(t('ro', 'identityError'), 'en')).toBe(
      t('en', 'identityError'),
    );
    // A blocking variable is named even when its name contains "TOKEN".
    const blocked = t('en', 'unsupportedEnvironment', {
      name: 'ANTHROPIC_AUTH_TOKEN',
    });
    expect(safeError(blocked, 'ro')).toBe(
      t('ro', 'unsupportedEnvironment', { name: 'ANTHROPIC_AUTH_TOKEN' }),
    );
    expect(
      safeError(
        t('en', 'unsupportedEnvironment', { name: 'leaked secret value' }),
        'en',
      ),
    ).toBe(t('en', 'failedAction'));
    expect(codeMessage('notACatalogKey', 'en')).toBe(t('en', 'failedAction'));
    expect(codeMessage('unsupportedSetting', 'en', 'apiKeyHelper')).toContain(
      'apiKeyHelper',
    );
    const accounts = snapshot().accounts;
    expect(
      errorText(
        {
          code: 'providerError',
          accountId: 'a',
          param: null,
          action: null,
          at: 1,
        },
        accounts,
        'en',
      ),
    ).toBe(`Studio: ${t('en', 'providerError')}`);
    expect(
      errorText(
        {
          code: 'providerError',
          accountId: 'b',
          param: null,
          action: 'autoSwitch',
          at: 1,
        },
        accounts,
        'ro',
      ),
    ).toBe(
      t('ro', 'autoSwitchFailed', {
        name: 'Personal',
        reason: t('ro', 'providerError'),
      }),
    );
    expect(
      snapshotSchema.safeParse(
        snapshot({
          error: {
            code: 'provider error <b>',
            accountId: null,
            param: null,
            action: null,
            at: 1,
          },
        }),
      ).success,
    ).toBe(false);
  });
  it('enforces all default settings and valid bounds', () => {
    expect(defaults).toEqual({
      pollInterval: 300,
      threshold: 95,
      autoSwitchEnabled: true,
      autoStartWindowEnabled: true,
      autoUseResetsEnabled: true,
      appearance: 'dark',
      language: 'en',
    });
    expect(
      snapshotSchema.safeParse(
        snapshot({ settings: { ...defaults, pollInterval: 121 } }),
      ).success,
    ).toBe(false);
    expect(
      snapshotSchema.safeParse(
        snapshot({ settings: { ...defaults, threshold: 101 } }),
      ).success,
    ).toBe(false);
  });
});
