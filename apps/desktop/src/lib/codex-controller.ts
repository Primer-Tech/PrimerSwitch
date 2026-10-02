import { writable } from 'svelte/store';
import {
  codexNativeBridge,
  type CodexBridge,
  type CodexCommandArgs,
} from './codex-bridge';
import {
  codexSnapshotSchema,
  codexLoginSchema,
  codexPreparationSchema,
  codexReasonSchema,
  hasCodexSelectionEvidence,
  type CodexSnapshot,
  type CodexLogin,
  type CodexPreparation,
  type CodexAccount,
  type CodexReason,
} from './codex-types';
export type CodexError = CodexReason | 'actionFailed' | 'unsafeData';
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
export interface CodexState {
  snapshot: CodexSnapshot | null;
  pending: string | null;
  error: CodexError | null;
  notice: 'loginComplete' | 'switchComplete' | 'deleteComplete' | null;
  login: {
    open: boolean;
    session: CodexLogin | null;
    working: boolean;
    error: CodexError | null;
  };
  preparation: {
    open: boolean;
    target: CodexAccount | null;
    value: CodexPreparation | null;
    working: boolean;
    error: CodexError | null;
  };
}
type SnapshotCommand =
  | 'codex_discover'
  | 'codex_import_current'
  | 'codex_refresh_account'
  | 'codex_delete_account';
export function createCodexController(bridge: CodexBridge = codexNativeBridge) {
  let state: CodexState = {
    snapshot: null,
    pending: null,
    error: null,
    notice: null,
    login: { open: false, session: null, working: false, error: null },
    preparation: {
      open: false,
      target: null,
      value: null,
      working: false,
      error: null,
    },
  };
  const store = writable(state);
  let disposed = false,
    started = false,
    reading = false,
    pollingLogin = false,
    loginGeneration = 0,
    switchGeneration = 0;
  let cachePoll: ReturnType<typeof setInterval> | undefined,
    loginPoll: ReturnType<typeof setInterval> | undefined,
    unlisten: (() => void) | undefined;
  const set = (patch: Partial<CodexState>) => {
    if (!disposed) {
      state = { ...state, ...patch };
      store.set(state);
    }
  };
  const stopLoginPoll = () => {
    if (loginPoll) clearInterval(loginPoll);
    loginPoll = undefined;
  };
  const busy = () =>
    disposed ||
    !!state.pending ||
    !!state.snapshot?.busy ||
    !!state.snapshot?.demo;
  const blocked = () => busy() || state.login.open || state.preparation.open;
  function accept(raw: unknown) {
    const next = codexSnapshotSchema.parse(raw);
    if (disposed || (state.snapshot && next.revision < state.snapshot.revision))
      return;
    set({ snapshot: next });
    const session = state.login.session;
    if (state.login.open && session && next.login?.id === session.id) {
      if (next.login.status === 'complete') {
        stopLoginPoll();
        loginGeneration++;
        set({
          login: { open: false, session: null, working: false, error: null },
          notice: 'loginComplete',
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
  }
  async function read() {
    if (disposed || reading) return;
    reading = true;
    try {
      accept(await bridge.call('get_codex_snapshot'));
    } catch (error) {
      set({ error: codexError(error) });
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
    } catch {}
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
    if (blocked()) return false;
    const capability =
      command === 'codex_import_current'
        ? 'importCurrent'
        : command === 'codex_refresh_account'
          ? 'refreshQuota'
          : command === 'codex_delete_account'
            ? 'deleteSaved'
            : null;
    if (capability && !state.snapshot?.capabilities[capability].enabled)
      return false;
    if (
      command === 'codex_refresh_account' &&
      args &&
      'id' in args &&
      args.id !== state.snapshot?.selectedId
    )
      return false;
    set({ pending: command, error: null, notice: null });
    try {
      accept(await bridge.call(command, args));
      if (state.snapshot?.error) return false;
      if (command === 'codex_delete_account') set({ notice: 'deleteComplete' });
      return true;
    } catch (error) {
      set({ error: codexError(error) });
      return false;
    } finally {
      set({ pending: null });
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
        const reason = codexError(error);
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
  async function beginLogin() {
    if (blocked() || !state.snapshot?.capabilities.loginBrowser.enabled) return;
    const mine = ++loginGeneration;
    set({
      pending: 'codex_begin_login',
      error: null,
      notice: null,
      login: { open: true, session: null, working: true, error: null },
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
          open: true,
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
          login: { open: false, session: null, working: false, error: null },
          notice: 'loginComplete',
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
          login: { ...state.login, working: false, error: codexError(error) },
        });
    } finally {
      if (state.pending === 'codex_begin_login') set({ pending: null });
    }
  }
  async function cancelLogin() {
    const session = state.login.session;
    loginGeneration++;
    stopLoginPoll();
    set({ login: { open: false, session: null, working: false, error: null } });
    if (session) {
      set({ pending: 'codex_cancel_login' });
      try {
        accept(await bridge.call('codex_cancel_login', { id: session.id }));
      } catch (error) {
        set({ error: codexError(error) });
      } finally {
        if (state.pending === 'codex_cancel_login') set({ pending: null });
      }
    }
  }
  async function prepareSwitch(target: CodexAccount) {
    if (
      busy() ||
      state.login.open ||
      !state.snapshot?.capabilities.manualSwitch.enabled ||
      !target.manualSwitch.enabled ||
      !hasCodexSelectionEvidence(target) ||
      target.id === state.snapshot?.selectedId ||
      target.selected
    )
      return;
    const mine = ++switchGeneration;
    set({
      pending: 'codex_prepare_switch',
      notice: null,
      preparation: {
        open: true,
        target,
        value: null,
        working: true,
        error: null,
      },
    });
    try {
      const value = codexPreparationSchema.parse(
        await bridge.call('codex_prepare_switch', { id: target.id }),
      );
      if (value.accountId !== target.id) {
        await cancelPreparation(value.id);
        throw new Error('identityMismatch');
      }
      if (disposed || mine !== switchGeneration || !state.preparation.open) {
        await cancelPreparation(value.id);
        return;
      }
      set({
        preparation: {
          open: true,
          target,
          value,
          working: false,
          error: null,
        },
      });
    } catch (error) {
      if (mine === switchGeneration)
        set({
          preparation: {
            ...state.preparation,
            working: false,
            error: codexError(error),
          },
        });
    } finally {
      if (state.pending === 'codex_prepare_switch') set({ pending: null });
    }
  }
  async function cancelPreparation(id: string) {
    try {
      const raw = await bridge.call('codex_cancel_preparation', { id });
      if (!disposed) accept(raw);
    } catch (error) {
      if (!disposed) set({ error: codexError(error) });
    }
  }
  async function dismissPreparation() {
    if (state.pending === 'codex_apply_switch') return;
    const value = state.preparation.value;
    switchGeneration++;
    set({
      preparation: {
        open: false,
        target: null,
        value: null,
        working: false,
        error: null,
      },
    });
    if (value) {
      set({ pending: 'codex_cancel_preparation' });
      try {
        await cancelPreparation(value.id);
      } finally {
        if (state.pending === 'codex_cancel_preparation')
          set({ pending: null });
      }
    }
  }
  async function applySwitch(acknowledged: boolean) {
    const value = state.preparation.value;
    if (!acknowledged || !value || busy() || !state.preparation.open)
      return false;
    if (value.expiresAt <= Math.floor(Date.now() / 1000)) {
      set({
        pending: 'codex_cancel_preparation',
        preparation: {
          ...state.preparation,
          value: null,
          error: 'invalidPreparation',
        },
      });
      try {
        await cancelPreparation(value.id);
      } finally {
        if (state.pending === 'codex_cancel_preparation')
          set({ pending: null });
      }
      return false;
    }
    set({
      pending: 'codex_apply_switch',
      preparation: { ...state.preparation, working: true, error: null },
    });
    try {
      accept(
        await bridge.call('codex_apply_switch', {
          preparationId: value.id,
          clientsClosedAcknowledged: true,
        }),
      );
      if (
        state.snapshot?.error ||
        state.snapshot?.selectedId !== value.accountId
      ) {
        set({
          preparation: {
            ...state.preparation,
            working: false,
            value: null,
            error: state.snapshot?.error ?? 'identityMismatch',
          },
        });
        return false;
      }
      set({
        preparation: {
          open: false,
          target: null,
          value: null,
          working: false,
          error: null,
        },
        notice: 'switchComplete',
      });
      return true;
    } catch (error) {
      set({
        preparation: {
          ...state.preparation,
          value: null,
          working: false,
          error: codexError(error),
        },
      });
      return false;
    } finally {
      set({ pending: null });
    }
  }
  function dispose() {
    if (disposed) return;
    const session = state.login.session;
    const preparation = state.preparation.value;
    const applying = state.pending === 'codex_apply_switch';
    disposed = true;
    loginGeneration++;
    switchGeneration++;
    stopLoginPoll();
    if (cachePoll) clearInterval(cachePoll);
    unlisten?.();
    if (preparation && !applying) void cancelPreparation(preparation.id);
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
    beginLogin,
    cancelLogin,
    pollLogin,
    prepareSwitch,
    dismissPreparation,
    applySwitch,
    dispose,
    dismissError: () => set({ error: null, notice: null }),
  };
}
export type CodexController = ReturnType<typeof createCodexController>;
