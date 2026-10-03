<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { language, t } from '../lib/i18n';
  import { age } from '../lib/format';
  import {
    codexImportedMessage,
    codexLastRead,
    codexMessage,
    codexNextAccount,
  } from '../lib/codex-format';
  import type { CodexController, CodexNotice } from '../lib/codex-controller';
  import type { CodexAccount } from '../lib/codex-types';
  import type { Settings } from '../lib/types';
  import Icon from './Icon.svelte';
  import CodexIcon from './CodexIcon.svelte';
  import ProviderTabs from './ProviderTabs.svelte';
  import Modal from './Modal.svelte';
  import CodexActiveCard from './CodexActiveCard.svelte';
  import CodexAccountsTable from './CodexAccountsTable.svelte';
  import CodexInsights from './CodexInsights.svelte';
  import CodexSwitchBanner from './CodexSwitchBanner.svelte';
  import CodexDetails from './CodexDetails.svelte';
  import primerLogo from '../assets/primer-logo.png';
  import meta from '../../package.json';
  let {
    controller,
    now,
    onclaude,
    onsettings,
    externalBlocked = false,
    externalDemo = false,
    settingsAvailable = true,
    automation = null,
  }: {
    controller: CodexController;
    now: number;
    onclaude: () => void;
    onsettings: () => void;
    externalBlocked?: boolean;
    externalDemo?: boolean;
    settingsAvailable?: boolean;
    /** The automation settings Codex shares with Claude. */
    automation?: Settings | null;
  } = $props();
  let snapshot = $derived($controller.snapshot);
  let pending = $derived($controller.pending);
  let selected = $derived(
    snapshot?.accounts.find((a) => a.id === snapshot.selectedId) ??
      snapshot?.accounts.find((a) => a.selected) ??
      null,
  );
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
  let nextBest = $derived(snapshot ? codexNextAccount(snapshot) : null);
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
  let lastRead = $derived(codexLastRead(snapshot));
  let setupProblem = $derived(
    !!snapshot &&
      snapshot.availability !== 'supported' &&
      !(snapshot.availability === 'unqualified' && !snapshot.blockedReason),
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
  let showWarnings = $derived(
    warnings.length > 0 && warnings.join(',') !== dismissedWarnings,
  );
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
  let addOpen = $state(false);
  let addButton = $state<HTMLButtonElement | null>(null);
  let addMenu = $state<HTMLElement | null>(null);
  let noticeElement = $state<HTMLElement | null>(null);
  let errorElement = $state<HTMLElement | null>(null);
  let detailsId = $state<string | null>(null),
    deleteTarget = $state<CodexAccount | null>(null),
    returnFocus = $state<HTMLElement | null>(null);
  let details = $derived(
    snapshot?.accounts.find((a) => a.id === detailsId) ?? null,
  );
  onMount(() => {
    void controller.start();
  });
  function noticeText(notice: CodexNotice): string {
    switch (notice.kind) {
      case 'loginComplete': {
        const account = snapshot?.accounts.find(
          (a) => a.id === notice.accountId,
        );
        return account
          ? t($language, 'codexSignedInAgain', { name: account.name })
          : t($language, 'codexLoginComplete');
      }
      case 'deleteComplete':
        return t($language, 'codexDeleteComplete');
      case 'importedCurrent':
        return codexImportedMessage('current', notice.added, $language);
      case 'importedSwitcher':
        return codexImportedMessage('switcher', notice.added, $language);
      case 'switched': {
        const name =
          snapshot?.accounts.find((a) => a.id === notice.accountId)?.name ??
          notice.name;
        return t(
          $language,
          notice.daemonRestarted ? 'codexSwitchedLive' : 'codexSwitchedNext',
          { name },
        );
      }
    }
  }
  function focusOutcome(trigger: HTMLElement | null) {
    const active = document.activeElement;
    if (
      trigger?.isConnected &&
      active &&
      active !== document.body &&
      !(active instanceof HTMLButtonElement && active.disabled)
    )
      return;
    setTimeout(() => (noticeElement ?? errorElement)?.focus(), 0);
  }
  async function switchTo(account: CodexAccount, trigger: HTMLElement | null) {
    detailsId = null;
    await controller.switchAccount(account);
    await tick();
    focusOutcome(trigger);
  }
  function signIn(account: CodexAccount | null, trigger: HTMLElement | null) {
    detailsId = null;
    returnFocus = trigger;
    void controller.beginLogin(account);
  }
  function openDetails(account: CodexAccount, trigger: HTMLElement) {
    returnFocus = trigger;
    detailsId = account.id;
  }
  function askDelete(account: CodexAccount, trigger: HTMLElement | null) {
    returnFocus = trigger;
    detailsId = null;
    deleteTarget = account;
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
  function add(run: () => void) {
    addOpen = false;
    returnFocus = addButton;
    addButton?.focus();
    run();
  }
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && addOpen) {
      event.preventDefault();
      addOpen = false;
      addButton?.focus();
    }
  }
  function pointerdown(event: PointerEvent) {
    if (
      addOpen &&
      !(event.target instanceof Node && addMenu?.contains(event.target))
    )
      addOpen = false;
  }
</script>

<svelte:window onkeydown={keydown} onpointerdown={pointerdown} />
<div class="desktop-shell">
  <aside class="sidebar" aria-label={t($language, 'navigation')}>
    <div class="sidebar-brand" role="img" aria-label="PrimerSwitch">
      <img
        class="primer-mark"
        src={primerLogo}
        alt=""
        aria-hidden="true"
      /><span class="primer-wordmark" aria-hidden="true"
        ><strong>primer</strong><span>Switch</span></span
      >
    </div>
    <nav>
      <button
        class="nav-current"
        aria-current="page"
        onclick={() =>
          document
            .getElementById('codex-accounts-heading')
            ?.scrollIntoView({ behavior: 'smooth' })}
        ><Icon name="accounts" /><span>{t($language, 'accounts')}</span></button
      ><button
        aria-label={t($language, 'openSettings')}
        disabled={!settingsAvailable}
        onclick={onsettings}
        ><Icon name="settings" /><span>{t($language, 'settings')}</span></button
      >
    </nav>
    <div class="sidebar-bottom">
      <span class="status-dot"></span><span
        >{t($language, 'localAccounts')}</span
      >
    </div>
  </aside>
  <div class="workspace">
    <header class="workspace-header">
      <div>
        <h1>{t($language, 'accounts')}</h1>
        <p>{t($language, 'codexDescription')}</p>
      </div>
      <ProviderTabs
        active="codex"
        onselect={(provider) => {
          if (provider === 'claude') onclaude();
        }}
        disabled={externalBlocked ||
          !!pending ||
          !!switching ||
          $controller.login.open}
      />
    </header>
    {#if snapshot?.demo}<div
        class="demo-notice"
        role="status"
        aria-label={t($language, 'demoNotice')}
      >
        <strong>{t($language, 'preview')}</strong><span
          >{t($language, 'demoNotice')}</span
        >
      </div>{/if}
    <div class="codex-live" aria-live="polite">
      {#if switching || switchPending}<CodexSwitchBanner
          name={switchTarget?.name ?? ''}
          stage={switching?.stage ?? 'saving'}
          elapsed={now - (switching?.startedAt ?? switchStartedLocally ?? now)}
        />{/if}
    </div>
    {#if snapshot?.environment.codexSwitcherRunning}<div
        class="codex-banner warn"
        role="note"
      >
        <CodexIcon name="warning" />
        <p>
          <strong>{t($language, 'codexSwitcherTitle')}</strong>
          {t($language, 'codexSwitcherText')}
        </p>
        {#if snapshot.capabilities.importSwitcher.enabled}<button
            disabled={locked}
            onclick={() => controller.importSwitcher()}
            >{pending === 'codex_import_switcher'
              ? t($language, 'codexImporting')
              : t($language, 'codexSwitcherImport')}</button
          >{/if}
      </div>{/if}
    {#if showWarnings}<div
        class="codex-banner warn"
        role="note"
        aria-label={t($language, 'codexWarnings')}
      >
        <CodexIcon name="info" />
        <ul>
          {#each warnings as warning (warning)}<li>
              {codexMessage(warning, $language)}
            </li>{/each}
        </ul>
        <button
          class="icon-button"
          aria-label={t($language, 'dismissMessage')}
          onclick={() => (dismissedWarnings = warnings.join(','))}>×</button
        >
      </div>{/if}
    {#if errorReason}<div
        class="error-notice"
        role="alert"
        tabindex="-1"
        bind:this={errorElement}
      >
        <span>{codexMessage(errorReason, $language)}</span><button
          class="icon-button"
          aria-label={t($language, 'dismissMessage')}
          onclick={dismissError}>×</button
        >
      </div>{/if}
    {#if $controller.notice}
      {@const notice = $controller.notice}
      {@const warning = notice.kind === 'switched' ? notice.warning : null}
      <div
        class="codex-notice"
        class:warn={!!warning}
        role="status"
        tabindex="-1"
        bind:this={noticeElement}
      >
        <CodexIcon name={warning ? 'warning' : 'check'} />
        <div>
          <p>{noticeText(notice)}</p>
          {#if notice.kind === 'switched' && notice.otherClients > 0}<p
              class="sub"
            >
              {t($language, 'codexSwitchedOtherClients')}
            </p>{/if}
          {#if warning}<p class="sub warning">
              {codexMessage(warning, $language)}
            </p>{/if}
        </div>
        <button
          class="icon-button"
          aria-label={t($language, 'dismissMessage')}
          onclick={() => controller.dismissNotice()}>×</button
        >
      </div>
    {/if}
    {#if snapshot?.busy && !pending && !switching && !$controller.login.open}<div
        class="pending-notice"
        role="status"
      >
        <span class="spinner" aria-hidden="true"></span>{t(
          $language,
          'codexBusy',
        )}
      </div>{/if}
    <div
      id="provider-content"
      role="tabpanel"
      aria-labelledby="provider-codex"
      tabindex="0"
    >
      <main class="dashboard-grid codex-dashboard">
        <div class="main-column">
          {#if snapshot && setupProblem}<section
              class="panel codex-setup-panel"
              aria-labelledby="codex-setup-panel-heading"
            >
              <div class="panel-heading">
                <h2 id="codex-setup-panel-heading">
                  <Icon name="settings" />{t($language, 'codexSetupTitle')}
                </h2>
                <button disabled={locked} onclick={() => controller.discover()}
                  ><Icon name="refresh" size={16} />{t(
                    $language,
                    'codexCheckSetup',
                  )}</button
                >
              </div>
              <p>
                {codexMessage(
                  snapshot.blockedReason ??
                    (snapshot.availability === 'notInstalled'
                      ? 'notInstalled'
                      : 'unsupportedVersion'),
                  $language,
                )}
              </p>
            </section>{/if}
          <CodexActiveCard
            {snapshot}
            account={selected}
            {now}
            {locked}
            refreshing={!!selected &&
              ((pending === 'codex_refresh_account' &&
                $controller.pendingId === selected.id) ||
                pending === 'codex_refresh_all')}
            {nextBest}
            onrefresh={(account) => controller.refreshAccount(account.id)}
            ondetails={openDetails}
            onswitch={(account, trigger) => switchTo(account, trigger)}
            onsignin={(account, trigger) => signIn(account, trigger)}
          />
          <section
            class="saved-section"
            aria-labelledby="codex-accounts-heading"
          >
            <div class="section-heading">
              <h2 id="codex-accounts-heading">
                {t($language, 'codexSavedAccounts')}<span class="count"
                  >{snapshot?.accounts.length ?? 0}</span
                >
              </h2>
              <div
                class="add-menu"
                role="group"
                aria-label={t($language, 'codexAddActions')}
                bind:this={addMenu}
              >
                <button
                  class="primary"
                  bind:this={addButton}
                  disabled={locked || !canAdd}
                  aria-expanded={addOpen}
                  aria-controls="codex-add-actions"
                  onclick={() => (addOpen = !addOpen)}
                  ><Icon name="plus" size={16} />{pending ===
                    'codex_import_current' ||
                  pending === 'codex_import_switcher'
                    ? t($language, 'codexImporting')
                    : t($language, 'codexAddAccount')}<Icon
                    name="chevron"
                    size={14}
                  /></button
                >{#if addOpen}<div id="codex-add-actions" class="add-options">
                    <button
                      disabled={locked ||
                        !snapshot?.capabilities.loginBrowser.enabled}
                      onclick={() => add(() => signIn(null, addButton))}
                      ><Icon name="plus" />{t(
                        $language,
                        'codexAddSignIn',
                      )}</button
                    ><button
                      disabled={locked ||
                        !snapshot?.capabilities.importCurrent.enabled}
                      onclick={() =>
                        add(() => {
                          void controller.importCurrent();
                        })}
                      ><Icon name="import" />{t(
                        $language,
                        'codexAddImportCurrent',
                      )}</button
                    >{#if snapshot?.capabilities.importSwitcher.enabled}<button
                        disabled={locked}
                        onclick={() =>
                          add(() => {
                            void controller.importSwitcher();
                          })}
                        ><Icon name="archive" />{t(
                          $language,
                          'codexAddImportSwitcher',
                        )}</button
                      >{/if}
                  </div>{/if}
              </div>
            </div>
            {#if snapshot?.accounts.length}<CodexAccountsTable
                {snapshot}
                {now}
                {locked}
                {pending}
                pendingId={$controller.pendingId}
                onswitch={(account, trigger) => switchTo(account, trigger)}
                onsignin={(account, trigger) => signIn(account, trigger)}
                ondetails={openDetails}
                onrefresh={(account) => controller.refreshAccount(account.id)}
                ondelete={askDelete}
              />{:else if snapshot}<div
                class="panel empty-accounts codex-empty"
              >
                <span class="empty-account-icon"
                  ><CodexIcon name="terminal" size={22} /></span
                >
                <h3>{t($language, 'codexEmptyTitle')}</h3>
                <p>{t($language, 'codexEmptyText')}</p>
                <div class="empty-actions">
                  <button
                    class="primary"
                    disabled={locked ||
                      !snapshot.capabilities.loginBrowser.enabled}
                    onclick={(event) => signIn(null, event.currentTarget)}
                    >{t($language, 'codexAddSignIn')}</button
                  ><button
                    disabled={locked ||
                      !snapshot.capabilities.importCurrent.enabled}
                    onclick={() => controller.importCurrent()}
                    >{t($language, 'codexAddImportCurrent')}</button
                  >{#if snapshot.capabilities.importSwitcher.enabled}<button
                      disabled={locked}
                      onclick={() => controller.importSwitcher()}
                      >{t($language, 'codexAddImportSwitcher')}</button
                    >{/if}
                </div>
              </div>{/if}
          </section>
        </div>
        <aside
          class="insights-column"
          aria-label={t($language, 'accountInsights')}
        >
          <CodexInsights
            {snapshot}
            {now}
            {locked}
            {nextBest}
            {automation}
            checkingSetup={pending === 'codex_discover'}
            onswitch={(account, trigger) => switchTo(account, trigger)}
            ondiscover={() => controller.discover()}
          />
        </aside>
      </main>
    </div>
    <footer class="workspace-footer">
      <div>
        <span class="status-dot" class:inactive={!lastRead}></span><span
          >{lastRead
            ? t($language, 'codexUpdatedAge', {
                age: age(lastRead, now, $language),
              })
            : t($language, 'codexNoReadings')}</span
        >
      </div>
      <span class="version">PrimerSwitch {meta.version}</span><button
        class="quiet-button"
        disabled={locked || !canRefreshAll}
        onclick={() => controller.refreshAll()}
        ><Icon name="refresh" size={15} />{pending === 'codex_refresh_all'
          ? t($language, 'codexRefreshing')
          : t($language, 'codexRefreshAll')}</button
      >
    </footer>
  </div>
</div>
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
{#if details && snapshot}<Modal
    title={details.name}
    {returnFocus}
    onclose={() => (detailsId = null)}
  >
    <CodexDetails
      account={details}
      {snapshot}
      {now}
      {locked}
      checking={(pending === 'codex_refresh_account' &&
        $controller.pendingId === details.id) ||
        pending === 'codex_refresh_all'}
      onswitch={() => switchTo(details!, returnFocus)}
      onsignin={() => signIn(details, returnFocus)}
      onrefresh={() => controller.refreshAccount(details!.id)}
      ondelete={() => askDelete(details!, returnFocus)}
    />
  </Modal>{/if}
{#if deleteTarget}<Modal
    title={t($language, 'codexDeleteTitle')}
    {returnFocus}
    onclose={() => (deleteTarget = null)}
    ><p class="modal-intro">
      {t($language, 'codexDeleteDescription', { name: deleteTarget.name })}
    </p>
    {#if $controller.error}<p class="warning-text" role="alert">
        {codexMessage($controller.error, $language)}
      </p>{/if}
    <div class="modal-actions">
      <button
        disabled={pending === 'codex_delete_account'}
        onclick={() => (deleteTarget = null)}>{t($language, 'cancel')}</button
      ><button class="danger-button" disabled={locked} onclick={confirmDelete}
        >{pending === 'codex_delete_account'
          ? t($language, 'codexDeleting')
          : t($language, 'codexDeleteConfirm')}</button
      >
    </div></Modal
  >{/if}

<style>
  :global(:root) {
    --codex-warn: #f0c46f;
  }
  :global(:root[data-theme='light']) {
    --codex-warn: #8a5300;
  }
  @media (prefers-color-scheme: light) {
    :global(:root:not([data-theme='dark'])) {
      --codex-warn: #8a5300;
    }
  }
  .codex-banner,
  .codex-notice {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px 14px 12px 16px;
    margin-bottom: 17px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface);
    font-size: 0.79rem;
    line-height: 1.55;
  }
  .codex-banner > :global(svg),
  .codex-notice > :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  .codex-banner.warn {
    border-color: color-mix(in srgb, var(--codex-warn) 45%, var(--line));
    background: color-mix(in srgb, var(--codex-warn) 7%, var(--surface));
  }
  .codex-banner.warn > :global(svg) {
    color: var(--codex-warn);
  }
  .codex-banner p,
  .codex-banner ul {
    flex: 1;
    min-width: 0;
    margin: 0;
  }
  .codex-banner ul {
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .codex-banner strong {
    color: var(--codex-warn);
  }
  .codex-banner button:not(.icon-button) {
    flex: none;
    align-self: center;
    font-size: 0.76rem;
    padding: 0.45rem 0.8rem;
  }
  .codex-banner .icon-button,
  .codex-notice .icon-button {
    margin: -5px -4px -5px 0;
  }
  .codex-notice {
    border-color: color-mix(in srgb, var(--green) 40%, var(--line));
    background: color-mix(in srgb, var(--green) 6%, var(--surface));
  }
  .codex-notice > :global(svg) {
    color: var(--green);
  }
  .codex-notice > div {
    flex: 1;
    min-width: 0;
  }
  .codex-notice p {
    margin: 0;
  }
  .codex-notice .sub {
    color: var(--subtle);
    margin-top: 3px;
  }
  .codex-notice .sub.warning {
    color: var(--codex-warn);
  }
  .codex-notice.warn {
    border-color: color-mix(in srgb, var(--codex-warn) 45%, var(--line));
    background: color-mix(in srgb, var(--codex-warn) 7%, var(--surface));
  }
  .codex-notice.warn > :global(svg) {
    color: var(--codex-warn);
  }
  .codex-notice:focus-visible,
  .error-notice:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .panel.codex-setup-panel {
    padding: 17px 19px;
    margin-bottom: 18px;
    border-color: color-mix(in srgb, var(--danger) 40%, var(--line));
  }
  .codex-setup-panel p {
    margin: 10px 0 0;
    font-size: 0.8rem;
    line-height: 1.6;
    color: var(--subtle);
  }
  .codex-setup-panel button {
    font-size: 0.76rem;
    padding: 0.45rem 0.8rem;
  }
  .codex-empty .empty-actions {
    display: flex;
    justify-content: center;
    flex-wrap: wrap;
    gap: 9px;
  }
  .codex-empty .empty-account-icon {
    margin-bottom: 14px;
  }
  .add-options button {
    gap: 9px;
  }
  @media (max-width: 560px) {
    .codex-banner {
      flex-wrap: wrap;
    }
    .codex-banner p {
      flex-basis: calc(100% - 34px);
    }
    .codex-banner button:not(.icon-button) {
      margin-left: 30px;
    }
  }
</style>
