import { get } from 'svelte/store';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createController, type Controller } from './controller';
import type { Bridge, Command } from './bridge';
import { account, snapshot } from '../test/fixtures';
import { t } from './i18n';
import type { Snapshot } from './types';

const controllers: Controller[] = [];
afterEach(() => {
  controllers.splice(0).forEach((c) => c.dispose());
  vi.useRealTimers();
});

type Handler = (args: unknown) => unknown;
/** Commands that simply answer with the current snapshot. */
const readOnly: readonly Command[] = [
  'get_snapshot',
  'refresh_all',
  'cancel_login',
  'cancel_claude_profile_login',
];
/**
 * A fake bridge: reads and cancellations answer with the current snapshot, every
 * other command needs an explicit reply so an unexpected call fails loudly.
 */
function setup(raw: Snapshot = snapshot({ launchSupported: true })) {
  let emit: (value: unknown) => void = () => {};
  const replies: Partial<Record<Command, Handler>> = {};
  const call = vi.fn<Bridge['call']>();
  call.mockImplementation(async (command: Command, args?: unknown) => {
    const reply = replies[command];
    if (reply) return reply(args);
    if (readOnly.includes(command)) return raw;
    throw new Error(`unexpected command ${command}`);
  });
  const controller = createController({
    call,
    subscribe: async (callback) => {
      emit = callback;
      return () => {};
    },
  });
  controllers.push(controller);
  return {
    controller,
    call,
    emit: (value: unknown) => emit(value),
    reply: (command: Command, handler: Handler) => {
      replies[command] = handler;
    },
  };
}
type Setup = ReturnType<typeof setup>;
const names = (call: Setup['call']) => call.mock.calls.map(([name]) => name);
const count = (call: Setup['call'], name: Command) =>
  names(call).filter((candidate) => candidate === name).length;
const session = (id = 'profile-1') => ({
  id,
  url: 'https://example.invalid/authorize',
});
const required = (reason = 'firstUse') => ({
  status: 'authorizationRequired',
  reason,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
/** Opens the dialog for `b` and reaches the browser step of a first launch. */
async function atAuthorization(s: Setup) {
  await s.controller.start();
  s.controller.openLaunch('b');
  s.reply('claude_launch', () => required());
  await s.controller.startLaunch();
}
/** ... and past it: a bound session waits for the pasted code. */
async function atCode(s: Setup) {
  await atAuthorization(s);
  s.reply('begin_claude_profile_login', () => session());
  await s.controller.beginLaunchAuthorization();
}

describe('launching a saved account', () => {
  it.each(['default', 'opus', 'sonnet', 'fable'] as const)(
    'launches another account with the %s model without switching the login',
    async (model) => {
      const { controller, call, reply } = setup();
      await controller.start();
      expect(controller.openLaunch('b')).toBe(true);
      expect(get(controller).launch).toMatchObject({
        open: true,
        accountId: 'b',
        stage: 'choose',
        model: 'default',
        working: false,
        error: null,
      });
      controller.setLaunchModel(model);
      reply('claude_launch', () => ({ status: 'launched' }));
      await controller.startLaunch();
      expect(call).toHaveBeenCalledWith('claude_launch', { id: 'b', model });
      expect(names(call)).not.toContain('switch_account');
      const state = get(controller);
      expect(state.launch.open).toBe(false);
      expect(state.launchNotice).toEqual({ accountId: 'b' });
      expect(state.snapshot?.activeId).toBe('a');
      // The renderer sends an id and a model: no path, executable or command line.
      expect(call).toHaveBeenLastCalledWith('claude_launch', {
        id: 'b',
        model,
      });
    },
  );
  it('also launches the account that is already active', async () => {
    const { controller, call, reply } = setup();
    await controller.start();
    expect(controller.openLaunch('a')).toBe(true);
    reply('claude_launch', () => ({ status: 'launched' }));
    await controller.startLaunch();
    expect(call).toHaveBeenCalledWith('claude_launch', {
      id: 'a',
      model: 'default',
    });
    expect(names(call)).not.toContain('switch_account');
  });
  it('ignores an unknown model and keeps the choice while a launch runs', async () => {
    const { controller, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    controller.setLaunchModel('gpt-5' as never);
    expect(get(controller).launch.model).toBe('default');
    const gate = deferred<unknown>();
    reply('claude_launch', () => gate.promise);
    const running = controller.startLaunch();
    controller.setLaunchModel('opus');
    expect(get(controller).launch.model).toBe('default');
    gate.resolve({ status: 'launched' });
    await running;
  });
  it('returns cleanly when no folder was chosen, without any authorization', async () => {
    const { controller, call, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    reply('claude_launch', () => ({ status: 'cancelled' }));
    await controller.startLaunch();
    expect(get(controller).launch).toMatchObject({
      open: true,
      stage: 'choose',
      working: false,
      step: null,
      info: 'cancelled',
      error: null,
    });
    expect(names(call)).not.toContain('begin_claude_profile_login');
    expect(names(call)).not.toContain('begin_login');
    expect(get(controller).launchNotice).toBeNull();
    // The same dialog can try again.
    reply('claude_launch', () => ({ status: 'launched' }));
    await controller.startLaunch();
    expect(get(controller).launch.open).toBe(false);
    expect(count(call, 'claude_launch')).toBe(2);
  });
  it('shows a failed start and never stays locked afterwards', async () => {
    const { controller, call, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    reply('claude_launch', () => {
      throw 'launchNotInstalled';
    });
    await controller.startLaunch();
    expect(get(controller).launch).toMatchObject({
      open: true,
      working: false,
      step: null,
      error: t('en', 'launchNotInstalled'),
    });
    expect(await controller.action('refresh_all')).toBe(false);
    // Nothing is held natively before a folder was chosen.
    await controller.cancelLaunch();
    expect(names(call)).not.toContain('cancel_claude_profile_login');
    expect(get(controller).launch.open).toBe(false);
    expect(await controller.action('refresh_all')).toBe(true);
  });
  it('keeps the dialog usable after each launch error code', async () => {
    const { controller, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    for (const code of [
      'launchFailed',
      'launchFolderInvalid',
      'launchUnsupported',
      'launchHistorySplit',
      'signInRequired',
      'missingAccount',
    ]) {
      reply('claude_launch', () => {
        throw code;
      });
      await controller.startLaunch();
      expect(get(controller).launch.error, code).toBe(t('en', code as never));
      expect(get(controller).launch.working).toBe(false);
    }
    reply('claude_launch', () => {
      throw new Error('access_token leaked');
    });
    await controller.startLaunch();
    expect(get(controller).launch.error).toBe(t('en', 'failedAction'));
  });
  it('rejects a malformed reply instead of acting on it', async () => {
    const { controller, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    for (const bad of [
      { status: 'launched', path: 'C:\\project' },
      { status: 'authorizationRequired', reason: 'whatever' },
      'launched',
      null,
    ]) {
      reply('claude_launch', () => bad);
      await controller.startLaunch();
      expect(get(controller).launch).toMatchObject({
        open: true,
        stage: 'choose',
        working: false,
        error: t('en', 'unsafeData'),
      });
    }
  });
  it('does not start twice while a start is running', async () => {
    const { controller, call, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    const gate = deferred<unknown>();
    reply('claude_launch', () => gate.promise);
    const first = controller.startLaunch();
    expect(get(controller).launch).toMatchObject({
      working: true,
      step: 'launch',
    });
    await controller.startLaunch();
    expect(count(call, 'claude_launch')).toBe(1);
    gate.resolve({ status: 'cancelled' });
    await first;
  });
});

describe('first-use authorization', () => {
  it('asks before opening the browser, then launches after the pasted code', async () => {
    const s = setup();
    await atAuthorization(s);
    const { controller, call, reply } = s;
    expect(get(controller).launch).toMatchObject({
      stage: 'authorize',
      reason: 'firstUse',
      session: null,
      working: false,
    });
    // Only an explicit action opens the browser.
    expect(names(call)).not.toContain('begin_claude_profile_login');
    reply('begin_claude_profile_login', () => session());
    await controller.beginLaunchAuthorization();
    expect(call).toHaveBeenCalledWith('begin_claude_profile_login', {
      accountId: 'b',
    });
    expect(get(controller).launch).toMatchObject({
      stage: 'code',
      session: session(),
      working: false,
    });
    // A partial code is rejected locally.
    await controller.finishLaunchAuthorization('missing-state');
    expect(get(controller).launch.error).toBe(t('en', 'fullCodeRequired'));
    expect(names(call)).not.toContain('finish_claude_profile_login');
    reply('finish_claude_profile_login', () => ({ status: 'launched' }));
    await controller.finishLaunchAuthorization(' code #state \n');
    expect(call).toHaveBeenCalledWith('finish_claude_profile_login', {
      id: 'profile-1',
      code: 'code#state',
    });
    expect(get(controller).launch.open).toBe(false);
    expect(get(controller).launchNotice).toEqual({ accountId: 'b' });
    // Ordinary sign-in and switching are never involved.
    for (const ordinary of [
      'begin_login',
      'finish_login',
      'cancel_login',
      'switch_account',
    ] as const)
      expect(names(call), ordinary).not.toContain(ordinary);
  });
  it.each(['identityChanged', 'expired'])(
    'keeps the %s reason for the same account-bound flow',
    async (reason) => {
      const s = setup();
      await s.controller.start();
      s.controller.openLaunch('b');
      s.reply('claude_launch', () => required(reason));
      await s.controller.startLaunch();
      expect(get(s.controller).launch).toMatchObject({
        stage: 'authorize',
        reason,
      });
    },
  );
  it('reports a wrong login, keeps the waiting launch and allows another try', async () => {
    const s = setup();
    await atCode(s);
    const { controller, call, reply } = s;
    reply('finish_claude_profile_login', () => {
      throw 'loginIdentityMismatch';
    });
    await controller.finishLaunchAuthorization('wrong#state');
    expect(get(controller).launch).toMatchObject({
      open: true,
      stage: 'code',
      // The native side consumed the sign-in.
      session: null,
      working: false,
      step: null,
      error: t('en', 'launchWrongLogin'),
    });
    // The pasted code cannot be reused, and a different account was never saved.
    await controller.finishLaunchAuthorization('again#state');
    expect(count(call, 'finish_claude_profile_login')).toBe(1);
    // Another browser sign-in replaces it without dropping the waiting folder.
    reply('begin_claude_profile_login', () => session('profile-2'));
    await controller.beginLaunchAuthorization();
    expect(call).not.toHaveBeenCalledWith('cancel_claude_profile_login', {
      id: '',
    });
    expect(get(controller).launch).toMatchObject({
      stage: 'code',
      session: session('profile-2'),
      error: null,
    });
    reply('finish_claude_profile_login', () => ({ status: 'launched' }));
    await controller.finishLaunchAuthorization('right#state');
    expect(call).toHaveBeenLastCalledWith('finish_claude_profile_login', {
      id: 'profile-2',
      code: 'right#state',
    });
    expect(get(controller).launch.open).toBe(false);
    expect(count(call, 'begin_claude_profile_login')).toBe(2);
  });
  it('asks again when the sign-in belongs to someone else after authorization', async () => {
    const s = setup();
    await atCode(s);
    s.reply('finish_claude_profile_login', () => required('identityChanged'));
    await s.controller.finishLaunchAuthorization('code#state');
    expect(get(s.controller).launch).toMatchObject({
      open: true,
      stage: 'authorize',
      reason: 'identityChanged',
      session: null,
      working: false,
    });
  });
  it('reports a failed start after authorization and goes back to the model choice', async () => {
    const s = setup();
    await atCode(s);
    const { controller, call, reply } = s;
    reply('finish_claude_profile_login', () => {
      throw 'launchFailed';
    });
    await controller.finishLaunchAuthorization('code#state');
    expect(get(controller).launch).toMatchObject({
      stage: 'code',
      session: null,
      working: false,
      error: t('en', 'launchFailed'),
    });
    await controller.backToLaunch();
    // The waiting launch is dropped; the account stays authorized.
    expect(call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: '',
    });
    expect(get(controller).launch).toMatchObject({
      open: true,
      stage: 'choose',
      reason: null,
      session: null,
      working: false,
      error: null,
    });
    reply('claude_launch', () => ({ status: 'launched' }));
    await controller.startLaunch();
    expect(get(controller).launch.open).toBe(false);
    expect(get(controller).launchNotice).toEqual({ accountId: 'b' });
  });
  it('treats authorization without a waiting launch as ready, not as a launch', async () => {
    const s = setup();
    await atCode(s);
    s.reply('finish_claude_profile_login', () => ({ status: 'authorized' }));
    await s.controller.finishLaunchAuthorization('code#state');
    expect(get(s.controller).launch).toMatchObject({
      open: true,
      stage: 'choose',
      info: 'authorized',
      session: null,
    });
    expect(get(s.controller).launchNotice).toBeNull();
  });
  it('reports a browser that could not open and lets the user retry or cancel', async () => {
    const s = setup();
    await atAuthorization(s);
    const { controller, call, reply } = s;
    reply('begin_claude_profile_login', () => {
      throw new Error('browser failed with token=secret');
    });
    await controller.beginLaunchAuthorization();
    expect(get(controller).launch).toMatchObject({
      stage: 'authorize',
      session: null,
      working: false,
      error: t('en', 'failedAction'),
    });
    reply('begin_claude_profile_login', () => session());
    await controller.beginLaunchAuthorization();
    expect(get(controller).launch).toMatchObject({
      stage: 'code',
      error: null,
    });
    await controller.cancelLaunch();
    expect(call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: 'profile-1',
    });
    expect(get(controller).launch.open).toBe(false);
  });
});

describe('cancelling and late replies', () => {
  it('drops a waiting launch that has no session yet', async () => {
    const s = setup();
    await atAuthorization(s);
    await s.controller.cancelLaunch();
    expect(s.call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: '',
    });
    expect(get(s.controller).launch.open).toBe(false);
    expect(await s.controller.action('refresh_all')).toBe(true);
  });
  it('cancels the bound sign-in by its session', async () => {
    const s = setup();
    await atCode(s);
    await s.controller.cancelLaunch();
    expect(s.call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: 'profile-1',
    });
    expect(get(s.controller).launch.open).toBe(false);
    // A late button press does nothing once closed.
    await s.controller.finishLaunchAuthorization('code#state');
    expect(names(s.call)).not.toContain('finish_claude_profile_login');
  });
  it('ignores a folder choice that finishes after cancellation, but cleans up its authorization', async () => {
    const { controller, call, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    const gate = deferred<unknown>();
    reply('claude_launch', () => gate.promise);
    const running = controller.startLaunch();
    await controller.cancelLaunch();
    expect(names(call)).not.toContain('cancel_claude_profile_login');
    gate.resolve(required());
    await running;
    expect(call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: '',
    });
    expect(get(controller).launch.open).toBe(false);
    expect(get(controller).launchNotice).toBeNull();
  });
  it('reports a launch that completed after the dialog was cancelled', async () => {
    const { controller, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    const gate = deferred<unknown>();
    reply('claude_launch', () => gate.promise);
    const running = controller.startLaunch();
    await controller.cancelLaunch();
    gate.resolve({ status: 'launched' });
    await running;
    expect(get(controller).launch.open).toBe(false);
    expect(get(controller).launchNotice).toEqual({ accountId: 'b' });
  });
  it('does not touch a newer dialog when an old reply arrives', async () => {
    const { controller, call, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    const old = deferred<unknown>();
    reply('claude_launch', () => old.promise);
    const stale = controller.startLaunch();
    await controller.cancelLaunch();
    controller.openLaunch('a');
    old.resolve(required());
    await stale;
    expect(get(controller).launch).toMatchObject({
      open: true,
      accountId: 'a',
      stage: 'choose',
      working: false,
    });
    // The newer dialog's own waiting launch is not cancelled by the old reply.
    expect(names(call)).not.toContain('cancel_claude_profile_login');
  });
  it('cancels a browser sign-in that opens after the dialog was cancelled', async () => {
    const s = setup();
    await atAuthorization(s);
    const { controller, call, reply } = s;
    const gate = deferred<unknown>();
    reply('begin_claude_profile_login', () => gate.promise);
    const opening = controller.beginLaunchAuthorization();
    expect(get(controller).launch).toMatchObject({
      working: true,
      step: 'browser',
    });
    await controller.cancelLaunch();
    gate.resolve(session('late'));
    await opening;
    expect(call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: 'late',
    });
    expect(get(controller).launch.open).toBe(false);
  });
  it('reports a launch that finished while the code was being exchanged', async () => {
    const s = setup();
    await atCode(s);
    const { controller, call, reply } = s;
    const gate = deferred<unknown>();
    reply('finish_claude_profile_login', () => gate.promise);
    const finishing = controller.finishLaunchAuthorization('code#state');
    expect(get(controller).launch).toMatchObject({
      working: true,
      step: 'code',
    });
    await controller.cancelLaunch();
    expect(call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: 'profile-1',
    });
    gate.resolve({ status: 'launched' });
    await finishing;
    expect(get(controller).launch.open).toBe(false);
    expect(get(controller).launchNotice).toEqual({ accountId: 'b' });
  });
  it('drops the waiting launch when the window goes away', async () => {
    const s = setup();
    await atCode(s);
    s.controller.dispose();
    expect(s.call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: 'profile-1',
    });
  });
  it('lets the finished-launch notice be dismissed and replaced', async () => {
    const { controller, reply } = setup();
    await controller.start();
    controller.openLaunch('b');
    reply('claude_launch', () => ({ status: 'launched' }));
    await controller.startLaunch();
    expect(get(controller).launchNotice).not.toBeNull();
    controller.dismissLaunchNotice();
    expect(get(controller).launchNotice).toBeNull();
    // The next launch replaces it; opening a dialog clears a stale one.
    controller.openLaunch('a');
    await controller.startLaunch();
    expect(get(controller).launchNotice).toEqual({ accountId: 'a' });
    controller.openLaunch('b');
    expect(get(controller).launchNotice).toBeNull();
  });
});

describe('locks', () => {
  const accountsWith = (overrides: Parameters<typeof account>[1]) => [
    account('a'),
    account('b', overrides),
  ];
  it.each<[string, () => Snapshot]>([
    ['an older snapshot without the capability', () => snapshot()],
    ['an unsupported build', () => snapshot({ launchSupported: false })],
    ['the preview', () => snapshot({ launchSupported: true, demo: true })],
    [
      'a running mutation',
      () => snapshot({ launchSupported: true, busy: true }),
    ],
    [
      'an account that needs to sign in',
      () =>
        snapshot({
          launchSupported: true,
          accounts: accountsWith({
            signInRequired: true,
            error: 'signInRequired',
          }),
        }),
    ],
    [
      'an unverified identity',
      () =>
        snapshot({
          launchSupported: true,
          accounts: accountsWith({ identityVerified: false }),
        }),
    ],
  ])('does not open for %s', async (_, build) => {
    const { controller, call } = setup(build());
    await controller.start();
    expect(controller.openLaunch('b')).toBe(false);
    expect(get(controller).launch.open).toBe(false);
    await controller.startLaunch();
    expect(names(call)).not.toContain('claude_launch');
  });
  it('does not open for the active account whose session expired, or an unknown id', async () => {
    const { controller } = setup(
      snapshot({
        launchSupported: true,
        accounts: [
          account('a', { error: 'activeSessionExpired' }),
          account('b'),
        ],
      }),
    );
    await controller.start();
    expect(controller.openLaunch('a')).toBe(false);
    expect(controller.openLaunch('missing')).toBe(false);
    expect(controller.openLaunch('b')).toBe(true);
  });
  it('does not open during an action, a sign-in or another launch', async () => {
    const { controller, reply } = setup();
    await controller.start();
    const gate = deferred<unknown>();
    reply('refresh_all', () => gate.promise);
    const refreshing = controller.action('refresh_all');
    expect(controller.openLaunch('b')).toBe(false);
    gate.resolve(snapshot({ launchSupported: true, revision: 2 }));
    await refreshing;
    reply('begin_login', () => session('login-1'));
    await controller.beginLogin();
    expect(controller.openLaunch('b')).toBe(false);
    await controller.cancelLogin();
    expect(controller.openLaunch('b')).toBe(true);
    expect(controller.openLaunch('a')).toBe(false);
  });
  it('keeps Add and Repair, mutations and the preview out while a launch is open', async () => {
    const { controller, call } = setup();
    await controller.start();
    controller.openLaunch('b');
    const before = call.mock.calls.length;
    expect(await controller.action('switch_account', { id: 'b' })).toBe(false);
    expect(await controller.action('refresh_all')).toBe(false);
    await controller.beginLogin('b');
    expect(call.mock.calls.length).toBe(before);
    expect(get(controller).login.open).toBe(false);
    await controller.cancelLaunch();
    expect(await controller.action('refresh_all')).toBe(true);
  });
  it('refuses to start when the account became unavailable meanwhile', async () => {
    const { controller, call, emit } = setup();
    await controller.start();
    controller.openLaunch('b');
    emit(snapshot({ revision: 2, launchSupported: true, demo: true }));
    await controller.startLaunch();
    expect(get(controller).launch.error).toBe(t('en', 'demoOnly'));
    emit(snapshot({ revision: 3, launchSupported: true, busy: true }));
    await controller.startLaunch();
    expect(get(controller).launch.error).toBe(t('en', 'switchBusy'));
    emit(
      snapshot({
        revision: 4,
        launchSupported: true,
        accounts: [account('a')],
      }),
    );
    await controller.startLaunch();
    expect(get(controller).launch.error).toBe(t('en', 'missingAccount'));
    expect(names(call)).not.toContain('claude_launch');
    // The dialog can still be closed.
    await controller.cancelLaunch();
    expect(get(controller).launch.open).toBe(false);
  });
});
