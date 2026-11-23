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
  launchModelSchema,
  launchOutcomeSchema,
  loginSchema,
  resetKindSchema,
  previewSchema,
  snapshotSchema,
  accountAutomationSchema,
  type AccountAutomation,
  type AccountView,
  type ErrorView,
  type LaunchModel,
  type LaunchOutcome,
  type ProfileAuthReason,
  type ResetKind,
  type Snapshot,
  type LoginSession,
  type ImportPreview,
} from './types';
import { launchBlock, launchBlockText } from './claude-launch';

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
  'loginIdentityMismatch',
  'providerError',
  'signInRequired',
  'activeSessionExpired',
  'settingsError',
  'readOnlyError',
  'loginExpired',
  'importExpired',
  'pendingResetError',
  'resetNotUsable',
  'resetNeedsLimit',
  'earlyUseNeedsConfirmation',
  'cliVersionError',
  'switchInterrupted',
  'switchRecoveryPending',
  'outgoingUnverified',
  'launchUnsupported',
  'launchNotInstalled',
  'launchFolderInvalid',
  'launchHistorySplit',
  'launchFailed',
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

/** `choose`: model and folder; `authorize`: consent to a browser sign-in; `code`: paste it. */
export type LaunchStage = 'choose' | 'authorize' | 'code';
/**
 * One launch of a saved account in its own Claude window. It is separate from the
 * Add/Repair `login` state and never goes through `switch_account`.
 */
export interface LaunchState {
  open: boolean;
  /** The saved account to launch; the native side re-checks it on every command. */
  accountId: string | null;
  model: LaunchModel;
  stage: LaunchStage;
  /** Why the launch profile has to be authorized; set while `stage` is not `choose`. */
  reason: ProfileAuthReason | null;
  /** The browser sign-in bound to `accountId`; null once the native side consumed it. */
  session: LoginSession | null;
  working: boolean;
  /** The command in flight: the folder choice and start, the browser, or the code. */
  step: 'launch' | 'browser' | 'code' | null;
  /** A quiet outcome of the last step that is not an error. */
  info: 'cancelled' | 'authorized' | null;
  error: string | null;
}
/** A launch that finished; shown until dismissed. */
export interface LaunchNotice {
  accountId: string;
}
const closedLaunch: LaunchState = {
  open: false,
  accountId: null,
  model: 'default',
  stage: 'choose',
  reason: null,
  session: null,
  working: false,
  step: null,
  info: null,
  error: null,
};
export interface ViewState {
  snapshot: Snapshot | null;
  pending: string | null;
  error: string | null;
  login: {
    open: boolean;
    /** Saved account being reauthenticated; native session retains the intent. */
    accountId: string | null;
    session: LoginSession | null;
    working: boolean;
    error: string | null;
  };
  launch: LaunchState;
  launchNotice: LaunchNotice | null;
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
    login: {
      open: false,
      accountId: null,
      session: null,
      working: false,
      error: null,
    },
    launch: closedLaunch,
    launchNotice: null,
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
    if (blocked() || state.login.open || state.launch.open) return false;
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
  async function beginLogin(accountId: string | null = null) {
    if (blocked() || state.login.open || state.launch.open) return;
    if (
      accountId !== null &&
      !state.snapshot?.accounts.some((account) => account.id === accountId)
    )
      return;
    const mine = ++generation;
    set({
      login: {
        open: true,
        accountId,
        session: null,
        working: true,
        error: null,
      },
      error: null,
    });
    try {
      const session = loginSchema.parse(
        await (accountId === null
          ? bridge.call('begin_login')
          : bridge.call('begin_login', { accountId })),
      );
      if (mine !== generation || disposed) {
        await bridge.call('cancel_login', { id: session.id });
      } else
        set({
          login: {
            open: true,
            accountId,
            session,
            working: false,
            error: null,
          },
        });
    } catch (error) {
      if (mine === generation)
        set({
          login: {
            open: true,
            accountId,
            session: null,
            working: false,
            error: safeError(error, locale()),
          },
        });
    }
  }
  async function updateAutomation(
    id: string,
    automation: AccountAutomation,
  ): Promise<AccountAutomation | null> {
    const parsed = accountAutomationSchema.safeParse(automation);
    if (
      !parsed.success ||
      !state.snapshot?.accounts.some((account) => account.id === id)
    )
      return null;
    if (
      !(await action('update_account_automation', {
        id,
        automation: parsed.data,
      }))
    )
      return null;
    return (
      state.snapshot?.accounts.find((account) => account.id === id)
        ?.automation ?? null
    );
  }
  /**
   * Uses one banked reset of `kind` on a saved account, the active one or not. The
   * runtime decides everything; this only refuses what the latest snapshot already
   * shows cannot work, and sends the early-use confirmation only for an offer that is
   * an early use (a confirmation the user gave in the dialog for exactly that).
   */
  async function useReset(
    id: string,
    kind: ResetKind | null,
    confirmation: string,
    allowEarly = false,
  ): Promise<boolean> {
    const resets = state.snapshot?.accounts.find(
      (account) => account.id === id,
    )?.resets;
    if (kind === null && resets?.retryPending)
      return action('claude_use_reset', {
        id,
        kind,
        allowEarly: false,
        confirmation,
      });
    const parsed = resetKindSchema.safeParse(kind);
    const offer = resets?.offers.find((candidate) => candidate.kind === kind);
    if (!parsed.success || !offer || !offer.usableNow) {
      set({ error: codeMessage('resetNotUsable', locale()) });
      return false;
    }
    return action('claude_use_reset', {
      id,
      kind: parsed.data,
      allowEarly: allowEarly && offer.earlyUse !== null,
      confirmation,
    });
  }
  async function cancelLogin() {
    const session = state.login.session;
    generation++;
    set({
      login: {
        open: false,
        accountId: null,
        session: null,
        working: false,
        error: null,
      },
    });
    if (session) {
      try {
        accept(await bridge.call('cancel_login', { id: session.id }));
      } catch (error) {
        set({ error: safeError(error, locale()) });
      }
    }
  }
  async function restartLogin() {
    if (blocked() || !state.login.open || state.login.working) return;
    const accountId = state.login.accountId;
    const session = state.login.session;
    const mine = ++generation;
    set({
      login: {
        open: true,
        accountId,
        session: null,
        working: true,
        error: null,
      },
    });
    try {
      if (session) {
        const result = await bridge.call('cancel_login', { id: session.id });
        if (mine !== generation || disposed) return;
        accept(result);
      }
      if (mine !== generation || disposed) return;
      // Re-read the saved target after cancellation; never retarget by email.
      set({
        login: {
          open: false,
          accountId: null,
          session: null,
          working: false,
          error: null,
        },
      });
      await beginLogin(accountId);
    } catch (error) {
      if (mine === generation && !disposed)
        set({
          login: {
            open: true,
            accountId,
            // Retain cleanup intent so another restart cancels this session first.
            session,
            working: false,
            error: safeError(error, locale()),
          },
        });
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
          login: {
            open: false,
            accountId: null,
            session: null,
            working: false,
            error: null,
          },
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
  // Launching a saved account in its own Claude window. These steps never call
  // switch_account and never touch the Add/Repair `login` state above.
  let launchGeneration = 0;
  const setLaunch = (patch: Partial<LaunchState>) =>
    set({ launch: { ...state.launch, ...patch } });
  const launchError = (error: unknown) => {
    const text = safeError(error, locale());
    // The generic wording points to Add account, which cannot help a launch.
    return text === t(locale(), 'loginIdentityMismatch')
      ? t(locale(), 'launchWrongLogin')
      : text;
  };
  /** The native side consumes a sign-in once it answers, so it never outlives one reply. */
  const consumed = {
    session: null,
    working: false,
    step: null,
  } as const;
  /** Drops what the native side still holds for a launch: its session, or just the waiting launch. */
  async function dropNativeLaunch(session: LoginSession | null) {
    accept(
      await bridge.call('cancel_claude_profile_login', {
        id: session?.id ?? '',
      }),
    );
  }
  /** A reply that arrives after its dialog was cancelled, replaced or disposed. */
  function lateOutcome(outcome: LaunchOutcome, accountId: string) {
    if (outcome.status === 'launched') set({ launchNotice: { accountId } });
    // A launch that now waits for a sign-in nobody will finish would resume later.
    else if (
      outcome.status === 'authorizationRequired' &&
      (disposed || !state.launch.open)
    )
      void bridge
        .call('cancel_claude_profile_login', { id: '' })
        .catch(() => {});
  }
  function applyOutcome(outcome: LaunchOutcome, accountId: string) {
    if (outcome.status === 'launched') {
      set({ launch: { ...closedLaunch }, launchNotice: { accountId } });
    } else if (outcome.status === 'authorizationRequired') {
      setLaunch({
        ...consumed,
        stage: 'authorize',
        reason: outcome.reason,
        info: null,
        error: null,
      });
    } else {
      // Cancelled (no folder was chosen) or authorized without a waiting launch:
      // nothing was started, and the same dialog can try again.
      setLaunch({
        ...consumed,
        stage: 'choose',
        reason: null,
        info: outcome.status,
        error: null,
      });
    }
  }
  function openLaunch(accountId: string): boolean {
    if (blocked() || state.login.open || state.launch.open) return false;
    const account = state.snapshot?.accounts.find(
      (candidate) => candidate.id === accountId,
    );
    if (!state.snapshot || !account || launchBlock(state.snapshot, account))
      return false;
    launchGeneration++;
    set({
      launch: { ...closedLaunch, open: true, accountId },
      launchNotice: null,
      error: null,
    });
    return true;
  }
  function setLaunchModel(model: LaunchModel) {
    const parsed = launchModelSchema.safeParse(model);
    const { open, working, stage } = state.launch;
    if (parsed.success && open && !working && stage === 'choose')
      setLaunch({ model: parsed.data, info: null });
  }
  async function startLaunch() {
    const { open, working, stage, accountId, model } = state.launch;
    if (disposed || !open || working || stage !== 'choose' || !accountId)
      return;
    const snapshot = state.snapshot;
    const account = snapshot?.accounts.find(
      (candidate) => candidate.id === accountId,
    );
    const block = snapshot && account ? launchBlock(snapshot, account) : null;
    if (!account || block || snapshot?.busy || state.pending) {
      setLaunch({
        error: !account
          ? codeMessage('missingAccount', locale())
          : block
            ? launchBlockText(block, locale())
            : t(locale(), 'switchBusy'),
      });
      return;
    }
    const mine = ++launchGeneration;
    setLaunch({ working: true, step: 'launch', info: null, error: null });
    let raw: unknown;
    try {
      raw = await bridge.call('claude_launch', { id: accountId, model });
    } catch (error) {
      if (mine === launchGeneration && !disposed)
        setLaunch({ working: false, step: null, error: launchError(error) });
      return;
    }
    const outcome = launchOutcomeSchema.safeParse(raw);
    if (mine !== launchGeneration || disposed) {
      if (outcome.success) lateOutcome(outcome.data, accountId);
      return;
    }
    if (outcome.success) applyOutcome(outcome.data, accountId);
    else
      setLaunch({
        working: false,
        step: null,
        error: t(locale(), 'unsafeData'),
      });
  }
  /** Opens the account-bound browser sign-in; calling it again replaces an earlier one. */
  async function beginLaunchAuthorization() {
    const { open, working, stage, accountId } = state.launch;
    if (disposed || !open || working || stage === 'choose' || !accountId)
      return;
    const mine = ++launchGeneration;
    setLaunch({ working: true, step: 'browser', info: null, error: null });
    try {
      const session = loginSchema.parse(
        await bridge.call('begin_claude_profile_login', { accountId }),
      );
      if (mine !== launchGeneration || disposed)
        await bridge.call('cancel_claude_profile_login', { id: session.id });
      else setLaunch({ stage: 'code', session, working: false, step: null });
    } catch (error) {
      if (mine === launchGeneration && !disposed)
        setLaunch({
          stage: 'authorize',
          session: null,
          working: false,
          step: null,
          error: launchError(error),
        });
    }
  }
  async function finishLaunchAuthorization(code: string) {
    const { open, working, stage, session, accountId } = state.launch;
    if (
      disposed ||
      !open ||
      working ||
      stage !== 'code' ||
      !session ||
      !accountId ||
      state.snapshot?.demo
    )
      return;
    const cleaned = code.replace(/\s/g, '');
    if (!/^[^#]+#[^#]+$/.test(cleaned)) {
      setLaunch({ error: t(locale(), 'fullCodeRequired') });
      return;
    }
    const mine = ++launchGeneration;
    setLaunch({ working: true, step: 'code', info: null, error: null });
    let raw: unknown;
    try {
      raw = await bridge.call('finish_claude_profile_login', {
        id: session.id,
        code: cleaned,
      });
    } catch (error) {
      // Wrong login, expired code or a failed start: the session is gone, so another
      // attempt needs a new browser sign-in. A waiting launch is kept for it.
      if (mine === launchGeneration && !disposed)
        setLaunch({ ...consumed, error: launchError(error) });
      return;
    }
    const outcome = launchOutcomeSchema.safeParse(raw);
    if (mine !== launchGeneration || disposed) {
      if (outcome.success) lateOutcome(outcome.data, accountId);
      return;
    }
    if (outcome.success) applyOutcome(outcome.data, accountId);
    else setLaunch({ ...consumed, error: t(locale(), 'unsafeData') });
  }
  /** Leaves an authorization step for the model choice, dropping what waits natively. */
  async function backToLaunch() {
    const { open, working, stage, session } = state.launch;
    if (disposed || !open || working || stage === 'choose') return;
    launchGeneration++;
    setLaunch({
      ...consumed,
      stage: 'choose',
      reason: null,
      info: null,
      error: null,
    });
    try {
      await dropNativeLaunch(session);
    } catch (error) {
      if (state.launch.open) setLaunch({ error: launchError(error) });
    }
  }
  async function cancelLaunch() {
    const { open, stage, session } = state.launch;
    if (!open) return;
    launchGeneration++;
    set({ launch: { ...closedLaunch } });
    // Before a folder was chosen nothing is held natively. A reply that is still on
    // its way is handled when it arrives (see lateOutcome).
    if (stage === 'choose') return;
    try {
      await dropNativeLaunch(session);
    } catch (error) {
      set({ error: launchError(error) });
    }
  }
  function dispose() {
    if (disposed) return;
    const session = state.login.session;
    const launch = state.launch;
    disposed = true;
    generation++;
    launchGeneration++;
    if (poll) clearInterval(poll);
    unlisten?.();
    if (session)
      void bridge.call('cancel_login', { id: session.id }).catch(() => {});
    if (launch.open && launch.stage !== 'choose')
      void bridge
        .call('cancel_claude_profile_login', { id: launch.session?.id ?? '' })
        .catch(() => {});
  }
  return {
    subscribe: store.subscribe,
    start,
    read,
    action,
    updateAutomation,
    useReset,
    resumeAutomaticReturns: () => action('resume_automatic_returns'),
    beginLogin,
    cancelLogin,
    restartLogin,
    finishLogin,
    openLaunch,
    setLaunchModel,
    startLaunch,
    beginLaunchAuthorization,
    finishLaunchAuthorization,
    backToLaunch,
    cancelLaunch,
    dismissLaunchNotice: () => set({ launchNotice: null }),
    dismissError: () => set({ error: null }),
    dismissPreview: () => set({ preview: null }),
    dispose,
  };
}
export type Controller = ReturnType<typeof createController>;
