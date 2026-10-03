import { writable } from 'svelte/store';
import {
  isCatalogMessage,
  localizeNativeMessage,
  t,
  type Language,
  type MessageKey,
} from './i18n';
import { en } from './locales/en';
import { ro } from './locales/ro';
import {
  nativeBridge,
  type Bridge,
  type Command,
  type CommandArgs,
} from './bridge';
import {
  loginSchema,
  previewSchema,
  snapshotSchema,
  type AccountView,
  type ErrorView,
  type Snapshot,
  type LoginSession,
  type ImportPreview,
} from './types';

/** Stable Claude runtime error and notice codes; each one is a catalog key. */
const errorCodes: ReadonlySet<string> = new Set<MessageKey>([
  'storageError',
  'vaultUnavailable',
  'unsupportedContext',
  'unsupportedEnvironment',
  'unsupportedSetting',
  'vaultIntegrity',
  'missingAccount',
  'externalChange',
  'identityError',
  'providerError',
  'signInRequired',
  'activeSessionExpired',
  'settingsError',
  'readOnlyError',
  'loginExpired',
  'importExpired',
  'pendingResetError',
  'cliVersionError',
  'switchInterrupted',
  'switchRecoveryPending',
  'outgoingUnverified',
]);
/** Messages that name a fixed variable or settings entry. */
const namedTemplates = [
  'unsupportedEnvironment',
  'unsupportedSetting',
] as const;
const safeName = /^[A-Za-z0-9_]{1,64}$/;

/** Catalog text for a runtime error code; unknown codes never display as text. */
export function codeMessage(
  code: string | null | undefined,
  locale: Language,
  param?: string | null,
): string {
  if (!code || !errorCodes.has(code)) return t(locale, 'failedAction');
  return t(
    locale,
    code as MessageKey,
    param && safeName.test(param) ? { name: param } : {},
  );
}
/** Status text for a snapshot error or notice, naming its account when known. */
export function errorText(
  view: ErrorView,
  accounts: readonly AccountView[],
  locale: Language,
): string {
  const reason = codeMessage(view.code, locale, view.param);
  const account = view.accountId
    ? accounts.find((a) => a.id === view.accountId)
    : undefined;
  if (!account) return reason;
  return t(
    locale,
    view.action === 'autoSwitch' ? 'autoSwitchFailed' : 'accountError',
    {
      name: account.name,
      reason,
    },
  );
}
/** Re-renders a native message built from a `{name}` template in `locale`. */
function namedMessage(text: string, locale: Language): string | null {
  for (const key of namedTemplates)
    for (const catalog of [en, ro]) {
      const [before, after] = catalog[key].split('{name}');
      const name = text.slice(before.length, text.length - after.length);
      if (
        text.length > before.length + after.length &&
        text.startsWith(before) &&
        text.endsWith(after) &&
        safeName.test(name)
      )
        return t(locale, key, { name });
    }
  return null;
}

export interface ViewState {
  snapshot: Snapshot | null;
  pending: string | null;
  error: string | null;
  login: {
    open: boolean;
    session: LoginSession | null;
    working: boolean;
    error: string | null;
  };
  preview: ImportPreview | null;
}
/** Display only native safe errors; never serialize arbitrary thrown objects. */
export function safeError(error: unknown, locale: Language = 'en'): string {
  const text =
    typeof error === 'string'
      ? error
      : error instanceof Error
        ? error.message
        : '';
  const localized = localizeNativeMessage(text, locale);
  if (isCatalogMessage(text)) return localized;
  // A bare code (or a variable named by a fixed template) is safe catalog text.
  if (errorCodes.has(text)) return codeMessage(text, locale);
  const named = namedMessage(text, locale);
  if (named) return named;
  if (
    !text ||
    text.length > 350 ||
    /token|bearer|secret|credential|verifier|sk-ant-|access_token|refresh_token|[{}]/i.test(
      text,
    )
  )
    return t(locale, 'failedAction');
  return text;
}
export function createController(bridge: Bridge = nativeBridge) {
  let state: ViewState = {
    snapshot: null,
    pending: null,
    error: null,
    login: { open: false, session: null, working: false, error: null },
    preview: null,
  };
  const store = writable(state);
  const locale = () => state.snapshot?.settings.language ?? 'en';
  let disposed = false,
    generation = 0,
    reading = false;
  let unlisten: (() => void) | undefined;
  let poll: ReturnType<typeof setInterval> | undefined;
  const set = (patch: Partial<ViewState>) => {
    if (!disposed) {
      state = { ...state, ...patch };
      store.set(state);
    }
  };
  const accept = (raw: unknown) => {
    const next = snapshotSchema.parse(raw);
    if (
      !disposed &&
      (!state.snapshot || next.revision >= state.snapshot.revision)
    )
      set({ snapshot: next });
  };
  const blocked = () =>
    disposed ||
    !!state.pending ||
    !!state.snapshot?.busy ||
    !!state.snapshot?.demo;
  async function read() {
    if (disposed || reading) return;
    reading = true;
    try {
      accept(await bridge.call('get_snapshot'));
    } catch (error) {
      set({ error: safeError(error, locale()) });
    } finally {
      reading = false;
    }
  }
  async function start() {
    try {
      const cleanup = await bridge.subscribe((raw) => {
        try {
          accept(raw);
        } catch {
          set({ error: t(locale(), 'unsafeData') });
        }
      });
      if (disposed) cleanup();
      else unlisten = cleanup;
    } catch {
      /* Pure snapshot polling remains available when events are unavailable. */
    }
    if (disposed) return;
    await read();
    if (!disposed)
      poll = setInterval(() => {
        void read();
      }, 5000);
  }
  async function action<C extends Command>(
    command: C,
    args?: CommandArgs[C],
  ): Promise<boolean> {
    if (blocked() || state.login.open) return false;
    set({ pending: command, error: null });
    try {
      const raw = await bridge.call(command, args);
      if (command === 'preview_legacy_import')
        set({ preview: previewSchema.parse(raw) });
      else accept(raw);
      return true;
    } catch (error) {
      set({ error: safeError(error, locale()) });
      return false;
    } finally {
      set({ pending: null });
    }
  }
  async function beginLogin() {
    if (blocked() || state.login.open) return;
    const mine = ++generation;
    set({
      login: { open: true, session: null, working: true, error: null },
      error: null,
    });
    try {
      const session = loginSchema.parse(await bridge.call('begin_login'));
      if (mine !== generation || disposed) {
        await bridge.call('cancel_login', { id: session.id });
      } else
        set({ login: { open: true, session, working: false, error: null } });
    } catch (error) {
      if (mine === generation)
        set({
          login: {
            open: true,
            session: null,
            working: false,
            error: safeError(error, locale()),
          },
        });
    }
  }
  async function cancelLogin() {
    const session = state.login.session;
    generation++;
    set({ login: { open: false, session: null, working: false, error: null } });
    if (session) {
      try {
        accept(await bridge.call('cancel_login', { id: session.id }));
      } catch (error) {
        set({ error: safeError(error, locale()) });
      }
    }
  }
  async function finishLogin(code: string) {
    const session = state.login.session;
    if (!session || state.login.working || disposed || state.snapshot?.demo)
      return;
    const cleaned = code.replace(/\s/g, '');
    if (!/^[^#]+#[^#]+$/.test(cleaned)) {
      set({
        login: {
          ...state.login,
          error: t(locale(), 'fullCodeRequired'),
        },
      });
      return;
    }
    const mine = generation;
    set({ login: { ...state.login, working: true, error: null } });
    try {
      const result = await bridge.call('finish_login', {
        id: session.id,
        code: cleaned,
      });
      if (mine === generation && !disposed) {
        accept(result);
        set({
          login: { open: false, session: null, working: false, error: null },
        });
      }
    } catch (error) {
      if (mine === generation)
        set({
          login: {
            ...state.login,
            working: false,
            error: safeError(error, locale()),
          },
        });
    }
  }
  function dispose() {
    if (disposed) return;
    const session = state.login.session;
    disposed = true;
    generation++;
    if (poll) clearInterval(poll);
    unlisten?.();
    if (session)
      void bridge.call('cancel_login', { id: session.id }).catch(() => {});
  }
  return {
    subscribe: store.subscribe,
    start,
    read,
    action,
    beginLogin,
    cancelLogin,
    finishLogin,
    dismissError: () => set({ error: null }),
    dismissPreview: () => set({ preview: null }),
    dispose,
  };
}
export type Controller = ReturnType<typeof createController>;
