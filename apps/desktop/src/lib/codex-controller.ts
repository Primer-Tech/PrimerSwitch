import { writable } from 'svelte/store';
import {
  codexNativeBridge,
  type CodexBridge,
  type CodexCommandArgs,
} from './codex-bridge';
import {
  codexSnapshotSchema,
  codexLoginSchema,
  codexReasonSchema,
  type CodexSnapshot,
  type CodexLogin,
  type CodexAccount,
  type CodexReason,
  type CodexResetOutcome,
} from './codex-types';
export type CodexError = CodexReason | 'actionFailed' | 'unsafeData';
/** Only stable native reason strings are shown; anything else becomes a generic failure. */
export function codexError(error: unknown): CodexError {
  const parsed = codexReasonSchema.safeParse(
    typeof error === 'string'
      ? error
      : error instanceof Error
        ? error.message
        : null,
  );
  return parsed.success ? parsed.data : 'actionFailed';
}
export type CodexNotice =
  | { kind: 'resetComplete'; accountId: string; outcome: CodexResetOutcome }
  | { kind: 'loginComplete'; accountId: string | null }
  | { kind: 'deleteComplete' }
  | { kind: 'importedCurrent'; added: number }
  | { kind: 'importedSwitcher'; added: number }
  | {
      kind: 'switched';
      accountId: string;
      name: string;
      daemonRestarted: boolean;
      otherClients: number;
      warning: CodexReason | null;
    };
export type CodexPending =
  | 'codex_discover'
  | 'codex_import_current'
  | 'codex_import_switcher'
  | 'codex_refresh_account'
  | 'codex_consume_reset'
  | 'codex_refresh_all'
  | 'codex_delete_account'
  | 'codex_switch_account'
  | 'codex_begin_login'
  | 'codex_cancel_login';
export interface CodexState {
  snapshot: CodexSnapshot | null;
  pending: CodexPending | null;
  /** Account targeted by the pending refresh, delete or switch. */
  pendingId: string | null;
  error: CodexError | null;
  notice: CodexNotice | null;
  login: {
    open: boolean;
    /** Saved account being signed in again, for display only. */
    accountId: string | null;
    session: CodexLogin | null;
    working: boolean;
    error: CodexError | null;
  };
}
type SnapshotCommand =
  | 'codex_discover'
  | 'codex_import_current'
  | 'codex_import_switcher'
  | 'codex_refresh_account'
  | 'codex_refresh_all'
  | 'codex_delete_account';
const capabilityFor = {
  codex_discover: null,
  codex_import_current: 'importCurrent',
  codex_import_switcher: 'importSwitcher',
  codex_refresh_account: 'refreshQuota',
  codex_refresh_all: 'refreshQuota',
  codex_delete_account: 'deleteSaved',
} as const satisfies Record<
  SnapshotCommand,
  keyof CodexSnapshot['capabilities'] | null
>;
const closedLogin = {
  open: false,
  accountId: null,
  session: null,
  working: false,
  error: null,
} satisfies CodexState['login'];
const unsafe = Symbol('unsafeData');
export function createCodexController(bridge: CodexBridge = codexNativeBridge) {
  let state: CodexState = {
    snapshot: null,
    pending: null,
    pendingId: null,
    error: null,
    notice: null,
    login: closedLogin,
  };
  const store = writable(state);
  let disposed = false,
    started = false,
    reading = false,
    pollingLogin = false,
    loginGeneration = 0;
  let cachePoll: ReturnType<typeof setInterval> | undefined,
    loginPoll: ReturnType<typeof setInterval> | undefined,
    unlisten: (() => void) | undefined;
  const set = (patch: Partial<CodexState>) => {
    if (!disposed) {
      state = { ...state, ...patch };
      store.set(state);
    }
  };
  const failure = (error: unknown): CodexError =>
    error === unsafe ? 'unsafeData' : codexError(error);
  const stopLoginPoll = () => {
    if (loginPoll) clearInterval(loginPoll);
    loginPoll = undefined;
  };
  const busy = () =>
    disposed ||
    !!state.pending ||
    !!state.snapshot?.busy ||
    !!state.snapshot?.demo ||
    !!state.snapshot?.switching;
  const blocked = () => busy() || state.login.open;
  /** Validates a native snapshot and applies it unless a newer revision is already shown. */
  function accept(raw: unknown): CodexSnapshot {
    const parsed = codexSnapshotSchema.safeParse(raw);
    if (!parsed.success) throw unsafe;
    const next = parsed.data;
    if (disposed || (state.snapshot && next.revision < state.snapshot.revision))
      return next;
    set({ snapshot: next });
    const session = state.login.session;
    if (state.login.open && session && next.login?.id === session.id) {
      if (next.login.status === 'complete') {
        stopLoginPoll();
        loginGeneration++;
        set({
          login: closedLogin,
          notice: { kind: 'loginComplete', accountId: state.login.accountId },
        });
      } else if (next.login.status === 'failed') {
        stopLoginPoll();
        set({
          login: {
            ...state.login,
            session: next.login,
            working: false,
            error: next.login.error ?? 'actionFailed',
          },
        });
      } else
        set({ login: { ...state.login, session: next.login, working: false } });
    }
    return next;
  }
  async function read() {
    if (disposed || reading) return;
    reading = true;
    try {
      accept(await bridge.call('get_codex_snapshot'));
    } catch (error) {
      set({ error: failure(error) });
    } finally {
      reading = false;
    }
  }
  async function start() {
    if (started || disposed) return;
    started = true;
    try {
      const cleanup = await bridge.subscribe((raw) => {
        try {
          accept(raw);
        } catch {
          set({ error: 'unsafeData' });
        }
      });
      if (disposed) cleanup();
      else unlisten = cleanup;
    } catch {
      /* Cache-only snapshot polling remains available without events. */
    }
    if (disposed) return;
    await read();
    if (!disposed) {
      cachePoll = setInterval(() => {
        void read();
      }, 5000);
      if (!state.snapshot?.demo) await action('codex_discover');
    }
  }
  async function action<C extends SnapshotCommand>(
    command: C,
    args?: CodexCommandArgs[C],
  ): Promise<boolean> {
    const snapshot = state.snapshot;
    if (blocked()) return false;
    const capability = capabilityFor[command];
    if (capability && !snapshot?.capabilities[capability].enabled) return false;
    const target = args && 'id' in args ? args.id : null;
    if (target !== null) {
      const account = snapshot?.accounts.find((a) => a.id === target);
      if (!account) return false;
      if (command === 'codex_refresh_account' && account.authKind !== 'chatgpt')
        return false;
      // The account Codex is signed in with cannot be removed; switch away first.
      if (
        command === 'codex_delete_account' &&
        (account.selected || account.reset.pending)
      )
        return false;
    }
    const before = snapshot?.accounts.length ?? 0;
    set({ pending: command, pendingId: target, error: null, notice: null });
    try {
      const result = accept(await bridge.call(command, args));
      if (result.error) return false;
      const added = Math.max(0, result.accounts.length - before);
      if (command === 'codex_delete_account')
        set({ notice: { kind: 'deleteComplete' } });
      else if (command === 'codex_import_current')
        set({ notice: { kind: 'importedCurrent', added } });
      else if (command === 'codex_import_switcher')
        set({ notice: { kind: 'importedSwitcher', added } });
      return true;
    } catch (error) {
      set({ error: failure(error) });
      void read();
      return false;
    } finally {
      set({ pending: null, pendingId: null });
    }
  }
  /**
   * One native call performs the whole switch. While it runs, snapshot events carry
   * `switching`; the resolved snapshot's `lastSwitch` is the authoritative outcome.
   */
  async function switchAccount(target: CodexAccount): Promise<boolean> {
    const snapshot = state.snapshot;
    const account = snapshot?.accounts.find((a) => a.id === target.id);
    if (
      blocked() ||
      !snapshot ||
      !account ||
      !snapshot.capabilities.switchAccount.enabled ||
      !account.switchable.enabled ||
      account.selected ||
      account.needsSignIn
    )
      return false;
    set({
      pending: 'codex_switch_account',
      pendingId: account.id,
      error: null,
      notice: null,
    });
    try {
      const result = accept(
        await bridge.call('codex_switch_account', { id: account.id }),
      );
      const outcome = result.lastSwitch;
      if (!outcome || outcome.accountId !== account.id) {
        set({ error: result.error ?? 'actionFailed' });
        return false;
      }
      set({
        notice: {
          kind: 'switched',
          accountId: account.id,
          name: account.name,
          daemonRestarted: outcome.daemonRestarted,
          otherClients: outcome.otherClients,
          warning: outcome.error,
        },
      });
      return true;
    } catch (error) {
      set({ error: failure(error) });
      void read();
      return false;
    } finally {
      set({ pending: null, pendingId: null });
    }
  }
  async function consumeReset(id: string): Promise<boolean> {
    const account = state.snapshot?.accounts.find(
      (account) => account.id === id,
    );
    if (blocked() || !account?.reset.usable.enabled) return false;
    set({
      pending: 'codex_consume_reset',
      pendingId: id,
      error: null,
      notice: null,
    });
    try {
      const result = accept(await bridge.call('codex_consume_reset', { id }));
      const reset = result.accounts.find((account) => account.id === id)?.reset;
      if (reset?.lastOutcome && !reset.pending) {
        set({
          notice: {
            kind: 'resetComplete',
            accountId: id,
            outcome: reset.lastOutcome,
          },
        });
        return true;
      }
      set({ error: result.error ?? 'resetUnconfirmed' });
      return false;
    } catch (error) {
      set({ error: failure(error) });
      void read();
      return false;
    } finally {
      set({ pending: null, pendingId: null });
    }
  }
  async function pollLogin() {
    const session = state.login.session;
    if (disposed || !state.login.open || !session || pollingLogin) return;
    const mine = loginGeneration;
    pollingLogin = true;
    try {
      const result = await bridge.call('codex_poll_login', { id: session.id });
      if (!disposed && mine === loginGeneration && state.login.open)
        accept(result);
    } catch (error) {
      if (mine === loginGeneration) {
        const reason = failure(error);
        if (reason === 'busy') return;
        stopLoginPoll();
        set({
          login: { ...state.login, working: false, error: reason },
        });
      }
    } finally {
      pollingLogin = false;
    }
  }
  /** Opens the managed browser sign-in; `forAccount` only labels a sign-in-again flow. */
  async function beginLogin(forAccount: CodexAccount | null = null) {
    if (blocked() || !state.snapshot?.capabilities.loginBrowser.enabled) return;
    const mine = ++loginGeneration;
    set({
      pending: 'codex_begin_login',
      error: null,
      notice: null,
      login: {
        open: true,
        accountId: forAccount?.id ?? null,
        session: null,
        working: true,
        error: null,
      },
    });
    try {
      const session = codexLoginSchema.parse(
        await bridge.call('codex_begin_login'),
      );
      if (disposed || mine !== loginGeneration || !state.login.open) {
        await bridge.call('codex_cancel_login', { id: session.id });
        return;
      }
      set({
        login: {
          ...state.login,
          session,
          working: false,
          error:
            session.status === 'failed'
              ? (session.error ?? 'actionFailed')
              : null,
        },
      });
      if (session.status === 'complete') {
        set({
          login: closedLogin,
          notice: { kind: 'loginComplete', accountId: forAccount?.id ?? null },
        });
        await read();
      } else if (session.status !== 'failed') {
        stopLoginPoll();
        loginPoll = setInterval(() => {
          void pollLogin();
        }, 1000);
      }
    } catch (error) {
      if (mine === loginGeneration)
        set({
          login: { ...state.login, working: false, error: failure(error) },
        });
    } finally {
      if (state.pending === 'codex_begin_login') set({ pending: null });
    }
  }
  async function cancelLogin() {
    const session = state.login.session;
    loginGeneration++;
    stopLoginPoll();
    set({ login: closedLogin });
    if (session) {
      set({ pending: 'codex_cancel_login' });
      try {
        accept(await bridge.call('codex_cancel_login', { id: session.id }));
      } catch (error) {
        set({ error: failure(error) });
      } finally {
        if (state.pending === 'codex_cancel_login') set({ pending: null });
      }
    }
  }
  function dispose() {
    if (disposed) return;
    const session = state.login.session;
    disposed = true;
    loginGeneration++;
    stopLoginPoll();
    if (cachePoll) clearInterval(cachePoll);
    unlisten?.();
    if (session)
      void bridge
        .call('codex_cancel_login', { id: session.id })
        .catch(() => {});
  }
  return {
    subscribe: store.subscribe,
    start,
    read,
    action,
    discover: () => action('codex_discover'),
    importCurrent: () => action('codex_import_current'),
    importSwitcher: () => action('codex_import_switcher'),
    refreshAccount: (id: string) => action('codex_refresh_account', { id }),
    refreshAll: () => action('codex_refresh_all'),
    deleteAccount: (id: string) => action('codex_delete_account', { id }),
    switchAccount,
    consumeReset,
    beginLogin,
    cancelLogin,
    pollLogin,
    dispose,
    dismissError: () => set({ error: null }),
    dismissNotice: () => set({ notice: null }),
  };
}
export type CodexController = ReturnType<typeof createCodexController>;
