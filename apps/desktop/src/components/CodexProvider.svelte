<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { language, t } from '../lib/i18n';
  import { age, countdown } from '../lib/format';
  import {
    codexMessage,
    codexLimitName,
    codexWindowLabel,
  } from '../lib/codex-format';
  import type { CodexController } from '../lib/codex-controller';
  import {
    hasCodexSelectionEvidence,
    type CodexAccount,
  } from '../lib/codex-types';
  import Icon from './Icon.svelte';
  import ProviderTabs from './ProviderTabs.svelte';
  import QuotaBar from './QuotaBar.svelte';
  import CodexQuotas from './CodexQuotas.svelte';
  import Modal from './Modal.svelte';
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
  }: {
    controller: CodexController;
    now: number;
    onclaude: () => void;
    onsettings: () => void;
    externalBlocked?: boolean;
    externalDemo?: boolean;
    settingsAvailable?: boolean;
  } = $props();
  let snapshot = $derived($controller.snapshot);
  let selected = $derived(
    snapshot?.accounts.find((a) => a.id === snapshot.selectedId) ?? null,
  );
  let disabled = $derived(
    externalBlocked ||
      externalDemo ||
      !!$controller.pending ||
      !!snapshot?.busy ||
      !!snapshot?.demo ||
      $controller.login.open ||
      $controller.preparation.open,
  );
  let addOpen = $state(false),
    addButton: HTMLButtonElement;
  let detailsId = $state<string | null>(null),
    deleteTarget = $state<CodexAccount | null>(null),
    returnFocus = $state<HTMLElement | null>(null),
    acknowledged = $state(false);
  let details = $derived(
    snapshot?.accounts.find((a) => a.id === detailsId) ?? null,
  );
  onMount(() => {
    void controller.start();
  });
  $effect(() => {
    if (!$controller.preparation.open) acknowledged = false;
  });
  function add(action: () => void) {
    addOpen = false;
    returnFocus = addButton;
    addButton?.focus();
    action();
  }
  async function cancelLogin() {
    await controller.cancelLogin();
    await tick();
    if (returnFocus?.isConnected) returnFocus.focus();
  }
  async function cancelSelection() {
    await controller.dismissPreparation();
    await tick();
    if (
      returnFocus?.isConnected &&
      !(returnFocus instanceof HTMLButtonElement && returnFocus.disabled)
    )
      returnFocus.focus();
    else document.getElementById('provider-codex')?.focus();
  }
  async function applySelection() {
    if (await controller.applySwitch(acknowledged)) {
      await tick();
      document.getElementById('provider-codex')?.focus();
    }
  }
  function closeAdd(event: KeyboardEvent) {
    if (event.key === 'Escape' && addOpen) {
      event.preventDefault();
      addOpen = false;
      addButton?.focus();
    }
  }
  function openDetails(account: CodexAccount, trigger: HTMLElement) {
    returnFocus = trigger;
    detailsId = account.id;
  }
  async function selectAccount(account: CodexAccount, trigger: HTMLElement) {
    returnFocus = trigger;
    await controller.prepareSwitch(account);
  }
  function canSelect(account: CodexAccount) {
    return (
      !disabled &&
      !account.selected &&
      hasCodexSelectionEvidence(account) &&
      account.manualSwitch.enabled &&
      !!snapshot?.capabilities.manualSwitch.enabled
    );
  }
</script>

<svelte:window onkeydown={closeAdd} />
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
          !!$controller.pending ||
          $controller.login.open ||
          $controller.preparation.open}
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
    {#if $controller.error || snapshot?.error}<div
        class="error-notice"
        role="alert"
      >
        <span
          >{codexMessage(
            $controller.error ?? snapshot?.error ?? null,
            $language,
          )}</span
        >{#if $controller.error}<button
            class="icon-button"
            aria-label={t($language, 'dismissMessage')}
            onclick={() => controller.dismissError()}>×</button
          >{/if}
      </div>{/if}
    {#if $controller.notice}<div class="codex-success" role="status">
        {t(
          $language,
          $controller.notice === 'loginComplete'
            ? 'codexLoginComplete'
            : $controller.notice === 'switchComplete'
              ? 'codexSwitchComplete'
              : 'codexDeleteComplete',
        )}
      </div>{/if}
    {#if $controller.pending || snapshot?.busy}<div
        class="pending-notice"
        role="status"
      >
        <span class="spinner" aria-hidden="true"></span>{t(
          $language,
          'codexWorking',
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
          {#if snapshot && snapshot.availability !== 'supported'}<section
              class="panel codex-setup"
            >
              <div class="panel-heading">
                <h2><Icon name="settings" />{t($language, 'codexSetup')}</h2>
                <button
                  {disabled}
                  onclick={() => controller.action('codex_discover')}
                  ><Icon name="refresh" />{t(
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
          <section
            class="panel active-panel"
            aria-labelledby="codex-active-heading"
          >
            <div class="panel-heading">
              <h2 id="codex-active-heading">
                <span class="status-dot" class:inactive={!selected}></span>{t(
                  $language,
                  'codexSelectedAccount',
                )}
              </h2>
              {#if selected}<button
                  class="quiet-button"
                  disabled={disabled ||
                    !snapshot?.capabilities.refreshQuota.enabled ||
                    selected.authKind !== 'chatgpt'}
                  aria-label={t($language, 'codexRefreshLabel', {
                    name: selected.name,
                  })}
                  onclick={() =>
                    controller.action('codex_refresh_account', {
                      id: selected!.id,
                    })}><Icon name="refresh" />{t($language, 'refresh')}</button
                >{/if}
            </div>
            {#if selected}
              <div class="active-title">
                <span class="avatar large-avatar"
                  >{selected.name.slice(0, 1).toUpperCase()}</span
                >
                <div>
                  <div class="identity-line">
                    <h3>{selected.name}</h3>
                    <span class="badge accent"
                      >{t($language, 'codexSelected')}</span
                    ><span class="badge muted"
                      >{t(
                        $language,
                        selected.authKind === 'chatgpt'
                          ? 'codexChatGPT'
                          : selected.authKind === 'apiKey'
                            ? 'codexApiKey'
                            : 'codexUnsupportedAuth',
                      )}</span
                    >
                  </div>
                  <p>
                    {selected.email ??
                      t(
                        $language,
                        'emailUnavailable',
                      )}{#if selected.workspaceName}<span>
                        · {selected.workspaceName}</span
                      >{/if}
                  </p>
                </div>
                {#if snapshot?.activeModel}<span class="active-model"
                    >{snapshot.activeModel}</span
                  >{/if}
              </div>
              <CodexQuotas account={selected} {now} />
              <div class="active-caption">
                <span
                  >{t(
                    $language,
                    selected.quotaState === 'cached' ? 'cachedAge' : 'readAge',
                    { age: age(selected.quotaReadAt, now, $language) },
                  )}</span
                ><button
                  class="text-button"
                  onclick={(event) =>
                    openDetails(selected!, event.currentTarget)}
                  >{t($language, 'viewAccountDetails')}<Icon
                    name="next"
                    size={14}
                  /></button
                >
              </div>
              {#if selected.error}<p class="warning-text">
                  {codexMessage(selected.error, $language)}
                </p>{/if}
            {:else}<div class="empty-active">
                <div>
                  <h3>
                    {t(
                      $language,
                      snapshot ? 'codexNoSelected' : 'loadingAccounts',
                    )}
                  </h3>
                  <p>{t($language, 'codexImportOrAdd')}</p>
                </div>
              </div>{/if}
          </section>
          <section
            class="saved-section"
            aria-labelledby="codex-accounts-heading"
          >
            <div class="section-heading">
              <h2 id="codex-accounts-heading">
                {t($language, 'savedAccounts')}<span class="count"
                  >{snapshot?.accounts.length ?? 0}</span
                >
              </h2>
              <div
                class="add-menu"
                role="group"
                aria-label={t($language, 'addActions')}
              >
                <button
                  class="primary"
                  bind:this={addButton}
                  disabled={disabled ||
                    !snapshot ||
                    (!snapshot.capabilities.loginBrowser.enabled &&
                      !snapshot.capabilities.importCurrent.enabled)}
                  aria-expanded={addOpen}
                  aria-controls="codex-add-actions"
                  onclick={() => (addOpen = !addOpen)}
                  ><Icon name="plus" size={16} />{t(
                    $language,
                    'addAccount',
                  )}<Icon name="chevron" size={14} /></button
                >{#if addOpen}<div id="codex-add-actions" class="add-options">
                    <button
                      disabled={disabled ||
                        !snapshot?.capabilities.loginBrowser.enabled}
                      onclick={() =>
                        add(() => {
                          void controller.beginLogin();
                        })}>{t($language, 'codexAddAccount')}</button
                    ><button
                      disabled={disabled ||
                        !snapshot?.capabilities.importCurrent.enabled}
                      onclick={() =>
                        add(() => {
                          void controller.action('codex_import_current');
                        })}>{t($language, 'codexImportCurrent')}</button
                    >
                  </div>{/if}
              </div>
            </div>
            {#if snapshot?.accounts.length}<div class="table-scroll">
                <table class="account-table codex-account-table">
                  <thead
                    ><tr
                      ><th>{t($language, 'account')}</th><th
                        >{t($language, 'codexSavedQuota')}</th
                      ><th>{t($language, 'actions')}</th></tr
                    ></thead
                  ><tbody>
                    {#each snapshot.accounts as account (account.id)}<tr
                        class:current-row={account.selected}
                        ><td
                          ><div class="table-identity">
                            <span class="avatar"
                              >{account.name.slice(0, 1).toUpperCase()}</span
                            >
                            <div>
                              <div class="identity-line">
                                <strong>{account.name}</strong
                                >{#if account.selected}<span class="row-badge"
                                    >{t($language, 'codexSelected')}</span
                                  >{/if}
                              </div>
                              <span class="account-email"
                                >{account.email ??
                                  t(
                                    $language,
                                    account.authKind === 'apiKey'
                                      ? 'codexApiKey'
                                      : 'emailUnavailable',
                                  )}</span
                              >{#if account.workspaceName}<small
                                  >{account.workspaceName}</small
                                >{/if}{#if !account.identityVerified}<small
                                  class="warning-text"
                                  >{t($language, 'codexQuotaUnverified')}</small
                                >{/if}
                            </div>
                          </div></td
                        >
                        <td
                          >{#if account.authKind === 'chatgpt' && account.identityVerified && account.quota?.limits[0]?.primary}<QuotaBar
                              compact
                              value={account.quota.limits[0].primary
                                .usedPercent}
                              label={codexWindowLabel(
                                account.quota.limits[0].primary
                                  .windowDurationMins,
                                false,
                                $language,
                              )}
                              {now}
                              threshold={100}
                            /><small class="codex-table-reading"
                              >{t(
                                $language,
                                account.quotaState === 'cached'
                                  ? 'cachedAge'
                                  : 'readAge',
                                {
                                  age: age(account.quotaReadAt, now, $language),
                                },
                              )}</small
                            >{:else}<span class="subtle"
                              >{t($language, 'codexNoQuotaShort')}</span
                            >{/if}</td
                        >
                        <td
                          ><div class="row-actions">
                            <button
                              class="switch-button"
                              disabled={!canSelect(account)}
                              title={!account.manualSwitch.enabled
                                ? codexMessage(
                                    account.manualSwitch.blockedReason,
                                    $language,
                                  )
                                : undefined}
                              onclick={(event) =>
                                selectAccount(account, event.currentTarget)}
                              >{t(
                                $language,
                                account.selected
                                  ? 'codexSelected'
                                  : 'codexSelect',
                              )}</button
                            ><button
                              class="icon-button"
                              aria-label={t($language, 'detailsLabel', {
                                name: account.name,
                              })}
                              onclick={(event) =>
                                openDetails(account, event.currentTarget)}
                              ><Icon name="more" /></button
                            >
                          </div></td
                        ></tr
                      >{/each}
                  </tbody>
                </table>
              </div>{:else if snapshot}<div class="panel empty-accounts">
                <h3>{t($language, 'codexEmptyTitle')}</h3>
                <p>{t($language, 'codexImportOrAdd')}</p>
                <button
                  disabled={disabled ||
                    !snapshot.capabilities.importCurrent.enabled}
                  onclick={() => controller.action('codex_import_current')}
                  >{t($language, 'codexImportCurrent')}</button
                >
              </div>{/if}
          </section>
        </div>
        <aside
          class="insights-column"
          aria-label={t($language, 'accountInsights')}
        >
          <section class="panel">
            <h2><Icon name="next" />{t($language, 'codexManualTitle')}</h2>
            <p>{t($language, 'codexManualDescription')}</p>
            <p>{t($language, 'codexCloseClients')}</p>
            <div class="codex-mode-badge">
              {t($language, 'codexManualOnly')}
            </div>
          </section>
          <section class="panel">
            <div class="panel-heading">
              <h2>
                <Icon name="settings" />{t($language, 'codexCompatibility')}
              </h2>
              <button
                class="icon-button"
                aria-label={t($language, 'codexCheckSetup')}
                {disabled}
                onclick={() => controller.action('codex_discover')}
                ><Icon name="refresh" size={16} /></button
              >
            </div>
            <dl class="automation-list">
              <div>
                <dt>{t($language, 'codexVersion')}</dt>
                <dd>
                  {snapshot?.executableVersion ?? t($language, 'unknown')}
                </dd>
              </div>
              <div>
                <dt>{t($language, 'codexBrowserLogin')}</dt>
                <dd>
                  {t(
                    $language,
                    snapshot?.capabilities.loginBrowser.enabled
                      ? 'codexAvailable'
                      : 'codexUnavailable',
                  )}
                </dd>
              </div>
            </dl>
            {#if snapshot?.blockedReason}<p>
                {codexMessage(snapshot.blockedReason, $language)}
              </p>{/if}
            <p>{t($language, 'codexSupportScope')}</p>
          </section>
          <section class="panel">
            <h2><Icon name="credits" />{t($language, 'codexResetCredits')}</h2>
            <div class="credits-value">
              <strong>{selected?.quota?.resetCreditsAvailable ?? '—'}</strong
              ><span>{t($language, 'codexReadOnly')}</span>
            </div>
            <p>{t($language, 'codexNoCreditActions')}</p>
          </section>
        </aside>
      </main>
    </div>
    <footer class="workspace-footer">
      <div>
        <span class="status-dot" class:inactive={!selected?.quotaReadAt}
        ></span><span
          >{selected?.quotaReadAt
            ? t($language, 'updatedAge', {
                age: age(selected.quotaReadAt, now, $language),
              })
            : t($language, 'noUpdates')}</span
        >
      </div>
      <span class="version">PrimerSwitch {meta.version}</span><button
        class="quiet-button"
        {disabled}
        onclick={() => controller.action('codex_discover')}
        ><Icon name="refresh" size={15} />{t(
          $language,
          'codexCheckSetup',
        )}</button
      >
    </footer>
  </div>
</div>
{#if $controller.login.open}<Modal
    title={t($language, 'codexAddAccount')}
    {returnFocus}
    onclose={() => {
      void cancelLogin();
    }}
  >
    <p class="modal-intro">{t($language, 'codexLoginDescription')}</p>
    <p class="help">{t($language, 'codexIsolatedLogin')}</p>
    {#if $controller.login.error}<p class="warning-text" role="alert">
        {codexMessage($controller.login.error, $language)}
      </p>{:else}<p class="working-message" role="status">
        <span class="spinner" aria-hidden="true"></span>{t(
          $language,
          $controller.login.working
            ? 'openingBrowser'
            : $controller.login.session?.status === 'verifying'
              ? 'codexVerifyingLogin'
              : 'codexWaitingLogin',
        )}
      </p>{/if}
    <div class="modal-actions">
      <button onclick={() => cancelLogin()}>{t($language, 'cancel')}</button>
    </div>
  </Modal>{/if}
{#if $controller.preparation.open}<Modal
    title={t($language, 'codexSelectTitle')}
    {returnFocus}
    onclose={() => {
      void cancelSelection();
    }}
  >
    <p class="modal-intro">
      {t($language, 'codexSelectDescription', {
        name: $controller.preparation.target?.name ?? '',
      })}
    </p>
    <p>{t($language, 'codexCloseClients')}</p>
    <p class="help">{t($language, 'codexNativeGuard')}</p>
    {#if $controller.preparation.error}<p class="warning-text" role="alert">
        {codexMessage($controller.preparation.error, $language)}
      </p>{/if}
    {#if $controller.preparation.working}<p
        class="working-message"
        role="status"
      >
        <span class="spinner" aria-hidden="true"></span>{t(
          $language,
          'codexWorking',
        )}
      </p>{/if}
    <label class="codex-ack"
      ><input
        type="checkbox"
        bind:checked={acknowledged}
        disabled={!!$controller.pending}
      /><span>{t($language, 'codexClosedAcknowledgement')}</span></label
    >
    {#if $controller.preparation.value && $controller.preparation.value.expiresAt <= now}<p
        class="warning-text"
        role="alert"
      >
        {t($language, 'codexReason_invalidPreparation')}
      </p>{/if}
    <div class="modal-actions">
      <button
        disabled={$controller.pending === 'codex_apply_switch'}
        onclick={() => cancelSelection()}>{t($language, 'cancel')}</button
      >{#if !$controller.preparation.value || $controller.preparation.value.expiresAt <= now}<button
          class="primary"
          disabled={!!$controller.pending || externalBlocked}
          onclick={() => {
            const target = $controller.preparation.target;
            if (target) void controller.prepareSwitch(target);
          }}>{t($language, 'codexCheckAgain')}</button
        >{:else}<button
          class="primary"
          disabled={!acknowledged ||
            !!$controller.pending ||
            externalBlocked ||
            !!snapshot?.demo}
          onclick={() => applySelection()}>{t($language, 'codexSelect')}</button
        >{/if}
    </div>
  </Modal>{/if}
{#if details}<Modal
    title={details.name}
    {returnFocus}
    onclose={() => (detailsId = null)}
  >
    <div class="codex-details">
      <p class="subtle">{details.email ?? t($language, 'emailUnavailable')}</p>
      <dl class="automation-list">
        <div>
          <dt>{t($language, 'codexAuthKind')}</dt>
          <dd>
            {t(
              $language,
              details.authKind === 'chatgpt'
                ? 'codexChatGPT'
                : details.authKind === 'apiKey'
                  ? 'codexApiKey'
                  : 'codexUnsupportedAuth',
            )}
          </dd>
        </div>
        <div>
          <dt>{t($language, 'codexQuotaVerification')}</dt>
          <dd>
            {t(
              $language,
              details.identityVerified
                ? 'codexVerified'
                : 'codexQuotaUnverified',
            )}
          </dd>
        </div>
        <div>
          <dt>{t($language, 'codexWorkspace')}</dt>
          <dd>
            {details.workspaceName ??
              details.workspaceId ??
              t($language, 'unknown')}
          </dd>
        </div>
      </dl>
      <CodexQuotas account={details} {now} />{#if details.error}<p
          class="warning-text"
        >
          {codexMessage(details.error, $language)}
        </p>{/if}
      <p class="help">{t($language, 'codexActiveQuotaOnly')}</p>
      <div class="modal-actions">
        <button
          class="danger-button"
          disabled={disabled || !snapshot?.capabilities.deleteSaved.enabled}
          aria-label={t($language, 'deleteLabel', { name: details.name })}
          onclick={() => {
            deleteTarget = details;
            detailsId = null;
          }}>{t($language, 'deleteAccount')}</button
        >{#if details.selected}<button
            disabled={disabled ||
              !snapshot?.capabilities.refreshQuota.enabled ||
              details.authKind !== 'chatgpt'}
            aria-label={t($language, 'codexRefreshLabel', {
              name: details.name,
            })}
            onclick={() =>
              controller.action('codex_refresh_account', { id: details!.id })}
            >{t($language, 'refresh')}</button
          >{/if}
      </div>
    </div>
  </Modal>{/if}
{#if deleteTarget}<Modal
    title={t($language, 'deleteTitle')}
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
        disabled={!!$controller.pending}
        onclick={() => (deleteTarget = null)}>{t($language, 'cancel')}</button
      ><button
        class="danger-button"
        {disabled}
        onclick={async () => {
          const target = deleteTarget;
          if (
            target &&
            (await controller.action('codex_delete_account', { id: target.id }))
          )
            deleteTarget = null;
        }}>{t($language, 'deleteAccount')}</button
      >
    </div></Modal
  >{/if}
