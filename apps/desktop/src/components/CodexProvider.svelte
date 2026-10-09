<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { language, t } from '../lib/i18n';
  import {
    codexElapsed,
    codexImportedMessage,
    codexMessage,
    codexResetOutcome,
  } from '../lib/codex-format';
  import type { CodexController, CodexNotice } from '../lib/codex-controller';
  import type { CodexAccount } from '../lib/codex-types';
  import type { Settings } from '../lib/types';
  import { codexView } from '../lib/codex-view';
  import type {
    AccountRowView,
    AddOption,
    AutomationItem,
    NoticeView,
    ProviderKey,
  } from '../lib/provider-view';
  import ProviderPage from './ProviderPage.svelte';
  import AccountDetails from './AccountDetails.svelte';
  import DeleteDialog from './DeleteDialog.svelte';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  import CodexResets from './CodexResets.svelte';
  let {
    controller,
    now,
    onprovider,
    onsettings,
    externalBlocked = false,
    externalDemo = false,
    settingsDisabled = false,
    automation = null,
  }: {
    controller: CodexController;
    now: number;
    onprovider: (provider: ProviderKey) => void;
    onsettings: () => void;
    /** A Claude action or sign-in is running. */
    externalBlocked?: boolean;
    externalDemo?: boolean;
    settingsDisabled?: boolean;
    /** The automation settings Codex shares with Claude. */
    automation?: Settings | null;
  } = $props();
  let snapshot = $derived($controller.snapshot);
  let activeRaw = $derived(
    snapshot?.accounts.find((a) => a.id === snapshot.selectedId) ?? null,
  );
  let pending = $derived($controller.pending);
  let switching = $derived(snapshot?.switching ?? null);
  let switchPending = $derived(pending === 'codex_switch_account');
  /** Any lock that must keep mutations disabled. */
  let locked = $derived(
    externalBlocked ||
      externalDemo ||
      !!pending ||
      !!snapshot?.busy ||
      !!snapshot?.demo ||
      !!switching ||
      $controller.login.open,
  );
  let lockReason = $derived(
    t($language, snapshot?.demo || externalDemo ? 'demoOnly' : 'switchBusy'),
  );
  let view = $derived(
    codexView(snapshot, {
      locale: $language,
      now,
      settings: automation,
      pending,
      pendingId: $controller.pendingId,
    }),
  );
  let switchTargetId = $derived(
    switching?.targetId ?? (switchPending ? $controller.pendingId : null),
  );
  let switchTarget = $derived(
    snapshot?.accounts.find((a) => a.id === switchTargetId) ?? null,
  );
  let switchStartedLocally = $state<number | null>(null);
  $effect(() => {
    if (!switching && !switchPending) switchStartedLocally = null;
    else if (switchStartedLocally === null)
      switchStartedLocally = Math.floor(Date.now() / 1000);
  });
  let setupProblem = $derived(
    !!snapshot &&
      snapshot.availability !== 'supported' &&
      !(snapshot.availability === 'unqualified' && !snapshot.blockedReason),
  );
  let setupReason = $derived(
    snapshot && setupProblem
      ? codexMessage(
          snapshot.blockedReason ??
            (snapshot.availability === 'notInstalled'
              ? 'notInstalled'
              : 'unsupportedVersion'),
          $language,
        )
      : null,
  );
  let dismissedSnapshotError = $state<string | null>(null);
  $effect(() => {
    if (!snapshot?.error) dismissedSnapshotError = null;
  });
  let errorReason = $derived(
    $controller.error &&
      !(setupProblem && $controller.error === snapshot?.blockedReason)
      ? $controller.error
      : snapshot?.error && snapshot.error !== dismissedSnapshotError
        ? snapshot.error
        : null,
  );
  let warnings = $derived([...new Set(snapshot?.warnings ?? [])]);
  let dismissedWarnings = $state('');
  let dismissedSignIn = $state<string | null>(null);
  let canRefreshAll = $derived(
    !!snapshot?.capabilities.refreshQuota.enabled &&
      !!snapshot?.accounts.some(
        (a) => a.authKind === 'chatgpt' && !a.needsSignIn,
      ),
  );
  let canAdd = $derived(
    !!snapshot &&
      (snapshot.capabilities.loginBrowser.enabled ||
        snapshot.capabilities.importCurrent.enabled ||
        snapshot.capabilities.importSwitcher.enabled),
  );
  let loginFor = $derived(
    snapshot?.accounts.find((a) => a.id === $controller.login.accountId) ??
      null,
  );
  let detailsId = $state<string | null>(null),
    deleteTarget = $state<AccountRowView | null>(null),
    returnFocus = $state<HTMLElement | null>(null);
  let details = $derived(view.rows.find((r) => r.id === detailsId) ?? null);
  const account = (id: string | undefined): CodexAccount | null =>
    snapshot?.accounts.find((a) => a.id === id) ?? null;
  onMount(() => {
    void controller.start();
  });
  function noticeText(notice: CodexNotice): string {
    switch (notice.kind) {
      case 'loginComplete': {
        const target = account(notice.accountId ?? undefined);
        return target
          ? t($language, 'codexSignedInAgain', { name: target.name })
          : t($language, 'codexLoginComplete');
      }
      case 'deleteComplete':
        return t($language, 'codexDeleteComplete');
      case 'resetComplete':
        return codexResetOutcome(notice.outcome, $language);
      case 'importedCurrent':
        return codexImportedMessage('current', notice.added, $language);
      case 'importedSwitcher':
        return codexImportedMessage('switcher', notice.added, $language);
      case 'switched':
        return t(
          $language,
          notice.daemonRestarted ? 'codexSwitchedLive' : 'codexSwitchedNext',
          { name: account(notice.accountId)?.name ?? notice.name },
        );
    }
  }
  /** After an action, move focus to its result unless focus is still somewhere useful. */
  function focusOutcome(trigger: HTMLElement | null) {
    const active = document.activeElement;
    if (
      trigger?.isConnected &&
      active &&
      active !== document.body &&
      !(active instanceof HTMLButtonElement && active.disabled)
    )
      return;
    setTimeout(
      () =>
        (
          document.getElementById('notice-outcome') ??
          document.getElementById('notice-error')
        )?.focus(),
      0,
    );
  }
  async function switchTo(row: AccountRowView, trigger: HTMLElement | null) {
    detailsId = null;
    const target = account(row.id);
    if (!target) return;
    await controller.switchAccount(target);
    await tick();
    focusOutcome(trigger);
  }
  function signIn(row: AccountRowView | null, trigger: HTMLElement | null) {
    detailsId = null;
    returnFocus = trigger;
    void controller.beginLogin(row ? account(row.id) : null);
  }
  function askDelete(row: AccountRowView, trigger: HTMLElement | null) {
    returnFocus = trigger;
    detailsId = null;
    deleteTarget = row;
  }
  async function confirmDelete() {
    const target = deleteTarget;
    if (target && (await controller.deleteAccount(target.id))) {
      deleteTarget = null;
      await tick();
      focusOutcome(returnFocus);
    }
  }
  async function cancelLogin() {
    await controller.cancelLogin();
    await tick();
    if (returnFocus?.isConnected) returnFocus.focus();
  }
  function dismissError() {
    if ($controller.error) controller.dismissError();
    else dismissedSnapshotError = snapshot?.error ?? null;
  }
  const stages = ['saving', 'restarting', 'verifying'] as const;
  let notices = $derived.by(() => {
    const list: NoticeView[] = [];
    if (switching || switchPending) {
      const stage = switching?.stage ?? 'saving';
      const current = stages.indexOf(stage);
      list.push({
        key: 'switching',
        tone: 'progress',
        role: 'status',
        title: t($language, 'codexSwitchingTo', {
          name: switchTarget?.name ?? '',
        }),
        text: t(
          $language,
          stage === 'saving'
            ? 'codexStageSaving'
            : stage === 'restarting'
              ? 'codexStageRestarting'
              : 'codexStageVerifying',
        ),
        steps: stages.map((step, index) => ({
          label: t(
            $language,
            step === 'saving'
              ? 'codexStepSave'
              : step === 'restarting'
                ? 'codexStepRestart'
                : 'codexStepVerify',
          ),
          state:
            index < current ? 'done' : index === current ? 'current' : 'todo',
        })),
        elapsed: codexElapsed(
          now - (switching?.startedAt ?? switchStartedLocally ?? now),
        ),
      });
    } else if (snapshot?.busy && !pending && !$controller.login.open)
      list.push({
        key: 'busy',
        tone: 'progress',
        role: 'status',
        text: t($language, 'codexBusy'),
      });
    if (errorReason)
      list.push({
        key: 'error',
        tone: 'error',
        role: 'alert',
        id: 'notice-error',
        text: codexMessage(errorReason, $language),
        dismiss: dismissError,
      });
    if (setupReason && view.rows.length)
      list.push({
        key: 'setup',
        tone: 'warn',
        role: 'note',
        title: t($language, 'codexSetupTitle'),
        text: setupReason,
        action: {
          label: t($language, 'codexCheckSetup'),
          disabled: locked,
          run: () => void controller.discover(),
        },
      });
    const outcome = $controller.notice;
    if (outcome) {
      const warning = outcome.kind === 'switched' ? outcome.warning : null;
      list.push({
        key: 'outcome',
        tone: warning ? 'warn' : 'success',
        role: 'status',
        id: 'notice-outcome',
        text: noticeText(outcome),
        lines: [
          ...(outcome.kind === 'switched' && outcome.otherClients > 0
            ? [{ text: t($language, 'codexSwitchedOtherClients') }]
            : []),
          ...(warning
            ? [
                {
                  text: codexMessage(warning, $language),
                  tone: 'warn' as const,
                },
              ]
            : []),
        ],
        dismiss: () => controller.dismissNotice(),
      });
    }
    if (view.allLimited)
      list.push({
        key: 'limited',
        tone: 'info',
        role: 'status',
        text: view.allLimited,
      });
    const signInKey = view.signIn.map((r) => r.id).join(',');
    if (view.signIn.length && signInKey !== dismissedSignIn) {
      const only = view.signIn.length === 1 ? view.signIn[0] : null;
      list.push({
        key: 'sign-in',
        tone: 'warn',
        role: 'note',
        text: t($language, 'signInNeeded', {
          names: view.signIn.map((r) => r.name).join(', '),
        }),
        action:
          only && only.canSignIn
            ? {
                label: t($language, 'signInAgain'),
                disabled: locked,
                run: (trigger) => signIn(only, trigger),
              }
            : undefined,
        dismiss: () => (dismissedSignIn = signInKey),
      });
    }
    if (snapshot?.environment.codexSwitcherRunning)
      list.push({
        key: 'switcher',
        tone: 'warn',
        role: 'note',
        title: t($language, 'codexSwitcherTitle'),
        text: t($language, 'codexSwitcherText'),
        action: snapshot.capabilities.importSwitcher.enabled
          ? {
              label:
                pending === 'codex_import_switcher'
                  ? t($language, 'importing')
                  : t($language, 'codexSwitcherImport'),
              disabled: locked,
              run: () => void controller.importSwitcher(),
            }
          : undefined,
      });
    if (warnings.length && warnings.join(',') !== dismissedWarnings)
      list.push({
        key: 'warnings',
        tone: 'warn',
        role: 'note',
        label: t($language, 'codexWarnings'),
        text: t($language, 'codexWarnings'),
        items: warnings.map((warning) => codexMessage(warning, $language)),
        dismiss: () => (dismissedWarnings = warnings.join(',')),
      });
    if (snapshot?.demo)
      list.push({
        key: 'demo',
        tone: 'demo',
        role: 'status',
        title: t($language, 'preview'),
        text: t($language, 'demoNotice'),
        label: t($language, 'demoNotice'),
      });
    return list;
  });
  let importing = $derived(
    pending === 'codex_import_current' || pending === 'codex_import_switcher',
  );
  let addOptions = $derived<AddOption[]>([
    {
      key: 'sign-in',
      label: t($language, 'codexAddSignIn'),
      icon: 'plus',
      disabled: locked || !snapshot?.capabilities.loginBrowser.enabled,
      run: (trigger) => signIn(null, trigger),
    },
    {
      key: 'current',
      label: t($language, 'codexImportCurrent'),
      icon: 'import',
      disabled: locked || !snapshot?.capabilities.importCurrent.enabled,
      run: () => void controller.importCurrent(),
    },
    ...(snapshot?.capabilities.importSwitcher.enabled
      ? [
          {
            key: 'switcher',
            label: t($language, 'codexImportSwitcher'),
            icon: 'archive',
            disabled: locked,
            run: () => void controller.importSwitcher(),
          },
        ]
      : []),
  ]);
  let environment = $derived(snapshot?.environment ?? null);
  let automationItems = $derived<AutomationItem[]>([
    ...(environment?.daemonRunning === true
      ? [
          {
            kind: 'note' as const,
            tone: 'ok' as const,
            text: t($language, 'codexEnvDaemon'),
          },
        ]
      : environment?.daemonRunning === false
        ? [
            {
              kind: 'note' as const,
              tone: 'muted' as const,
              text: t($language, 'codexEnvNoDaemon'),
            },
          ]
        : []),
    environment && environment.otherClients > 0
      ? {
          kind: 'note' as const,
          tone: 'warn' as const,
          text: t($language, 'codexEnvOtherClients'),
        }
      : {
          kind: 'note' as const,
          tone: 'muted' as const,
          text: t($language, 'codexEnvApps'),
        },
  ]);
  let setupState = $derived(
    !snapshot ||
      pending === 'codex_discover' ||
      (snapshot.availability === 'unqualified' && !snapshot.blockedReason)
      ? 'checking'
      : snapshot.availability === 'supported'
        ? 'ready'
        : snapshot.availability === 'notInstalled'
          ? 'missing'
          : 'attention',
  );
  let setupText = $derived.by(() => {
    const version = snapshot?.executableVersion;
    if (setupState === 'checking') return t($language, 'codexSetupChecking');
    if (setupState === 'missing') return t($language, 'codexSetupNotInstalled');
    if (setupState === 'ready')
      return version
        ? t($language, 'codexSetupReady', { version })
        : t($language, 'codexSetupReadyUnknown');
    return version
      ? t($language, 'codexSetupAttention', { version })
      : t($language, 'codexSetupAttentionUnknown');
  });
</script>

<ProviderPage
  {view}
  {now}
  subtitle={t($language, 'codexSubtitle')}
  tabsDisabled={externalBlocked ||
    !!pending ||
    !!switching ||
    $controller.login.open}
  {onprovider}
  {settingsDisabled}
  {onsettings}
  {notices}
  refreshAll={{
    label: t(
      $language,
      pending === 'codex_refresh_all' ? 'refreshing' : 'refreshAll',
    ),
    disabled: locked || !canRefreshAll,
    run: () => void controller.refreshAll(),
  }}
  {locked}
  {lockReason}
  refreshingActive={!!view.active &&
    ((pending === 'codex_refresh_account' &&
      $controller.pendingId === view.active.id) ||
      pending === 'codex_refresh_all')}
  activeEmpty={setupReason && !view.rows.length
    ? {
        title: t($language, 'codexSetupTitle'),
        text: setupReason,
        action: {
          label: t($language, 'codexCheckSetup'),
          disabled: locked,
          run: () => void controller.discover(),
        },
      }
    : snapshot
      ? { title: t($language, 'noActive'), text: t($language, 'noActiveCodex') }
      : { title: t($language, 'loadingAccounts'), text: null }}
  addLabel={importing ? t($language, 'importing') : t($language, 'addAccount')}
  addDisabled={locked || !canAdd}
  {addOptions}
  emptyText={t($language, 'emptyCodex')}
  {automationItems}
  onswitch={(row, trigger) => void switchTo(row, trigger)}
  onsignin={(row, trigger) => signIn(row, trigger)}
  ondetails={(row, trigger) => {
    returnFocus = trigger;
    detailsId = row.id;
  }}
  onrefresh={(row) => void controller.refreshAccount(row.id)}
  ondelete={askDelete}
>
  {#snippet providerCard()}
    {#if activeRaw?.authKind === 'chatgpt'}
      <section class="panel side-panel" aria-labelledby="codex-resets-heading">
        <div class="panel-heading">
          <h2 id="codex-resets-heading">{t($language, 'availableResets')}</h2>
        </div>
        <CodexResets
          account={activeRaw}
          {locked}
          working={pending === 'codex_consume_reset' &&
            $controller.pendingId === activeRaw.id}
          onreset={() => void controller.consumeReset(activeRaw!.id)}
          onrefresh={() => void controller.refreshAccount(activeRaw!.id)}
        />
      </section>
    {/if}
    <section
      class="panel side-panel setup-panel"
      aria-labelledby="codex-setup-heading"
    >
      <div class="panel-heading">
        <h2 id="codex-setup-heading">
          <Icon name="terminal" />{t($language, 'codexSetupTitle')}
        </h2>
        <button
          class="icon-button"
          aria-label={t($language, 'codexCheckSetup')}
          title={t($language, 'codexCheckSetup')}
          disabled={locked || pending === 'codex_discover'}
          onclick={() => controller.discover()}
          ><Icon name="refresh" size={16} /></button
        >
      </div>
      <p class="setup-line" data-state={setupState}>
        {#if setupState === 'checking'}<span class="spinner" aria-hidden="true"
          ></span>{:else}<span class="status-dot"></span>{/if}<span
          >{setupText}</span
        >
      </p>
      <p>{t($language, 'codexHowSaves')}</p>
      <p>{t($language, 'codexHowResumes')}</p>
    </section>
  {/snippet}
</ProviderPage>

{#if $controller.login.open}<Modal
    title={t($language, loginFor ? 'codexLoginAgainTitle' : 'codexLoginTitle')}
    {returnFocus}
    onclose={() => {
      void cancelLogin();
    }}
  >
    <p class="modal-intro">
      {loginFor
        ? t($language, 'codexLoginAgainDescription', {
            name: loginFor.email ?? loginFor.name,
          })
        : t($language, 'codexLoginDescription')}
    </p>
    {#if !loginFor}<p class="help">
        {t($language, 'codexLoginKeepsCurrent')}
      </p>{/if}
    {#if $controller.login.error}<p class="warning-text" role="alert">
        {codexMessage($controller.login.error, $language)}
      </p>{:else}<p class="working-message" role="status">
        <span class="spinner" aria-hidden="true"></span>{t(
          $language,
          $controller.login.working
            ? 'codexLoginOpening'
            : $controller.login.session?.status === 'verifying'
              ? 'codexLoginVerifying'
              : 'codexLoginWaiting',
        )}
      </p>{/if}
    <div class="modal-actions">
      <button onclick={() => cancelLogin()}>{t($language, 'cancel')}</button>
    </div>
  </Modal>{/if}
{#if details}{@const saved = account(details.id)}<Modal
    title={details.name}
    {returnFocus}
    onclose={() => (detailsId = null)}
  >
    <AccountDetails
      row={details}
      {now}
      threshold={view.threshold}
      {locked}
      {lockReason}
      onrefresh={() => controller.refreshAccount(details!.id)}
      onswitch={() => void switchTo(details!, returnFocus)}
      onsignin={() => signIn(details, returnFocus)}
      ondelete={() => askDelete(details!, returnFocus)}
    >
      {#snippet extra()}
        {#if saved?.authKind === 'chatgpt'}
          <section aria-label={t($language, 'availableResets')}>
            <h3>{t($language, 'availableResets')}</h3>
            <CodexResets
              account={saved}
              {locked}
              working={pending === 'codex_consume_reset' &&
                $controller.pendingId === saved.id}
              onreset={() => void controller.consumeReset(saved.id)}
              onrefresh={() => void controller.refreshAccount(saved.id)}
            />
          </section>
        {/if}
      {/snippet}
      {#snippet facts()}
        {#if saved}
          <div>
            <dt>{t($language, 'detailsSignIn')}</dt>
            <dd>
              {t(
                $language,
                saved.authKind === 'chatgpt'
                  ? 'codexAuthChatGPT'
                  : saved.authKind === 'apiKey'
                    ? 'codexAuthApiKey'
                    : 'codexAuthUnsupported',
              )}
            </dd>
          </div>
          {#if saved.workspaceName}<div>
              <dt>{t($language, 'detailsWorkspace')}</dt>
              <dd>{saved.workspaceName}</dd>
            </div>{/if}
          {#if details?.credits}<div>
              <dt>{t($language, 'credits')}</dt>
              <dd>{details.credits}</dd>
            </div>{/if}
        {/if}
      {/snippet}
    </AccountDetails>
  </Modal>{/if}
{#if deleteTarget}<DeleteDialog
    text={t($language, 'deleteCodex', { name: deleteTarget.name })}
    pending={pending === 'codex_delete_account'}
    disabled={locked}
    error={$controller.error
      ? codexMessage($controller.error, $language)
      : null}
    {returnFocus}
    onconfirm={() => void confirmDelete()}
    oncancel={() => (deleteTarget = null)}
  />{/if}
