<script lang="ts">
  import { language, t, plural } from './lib/i18n';
  import { onMount } from 'svelte';
  import {
    createController,
    safeError,
    type Controller,
  } from './lib/controller';
  import type { AccountView, Settings as SettingsType } from './lib/types';
  import Dashboard from './components/Dashboard.svelte';
  import CodexProvider from './components/CodexProvider.svelte';
  import {
    createCodexController,
    type CodexController,
  } from './lib/codex-controller';
  import AccountCard from './components/AccountCard.svelte';
  import Settings from './components/Settings.svelte';
  import Modal from './components/Modal.svelte';

  let {
    controller = createController(),
    codexController = createCodexController(),
  }: { controller?: Controller; codexController?: CodexController } = $props();
  let provider = $state<'claude' | 'codex'>('claude');
  let now = $state(Math.floor(Date.now() / 1000));
  let settingsOpen = $state(false),
    loginCode = $state('');
  let deleteTarget = $state<AccountView | null>(null);
  let operation = $state<string | null>(null);
  let snapshot = $derived($controller.snapshot);
  let disabled = $derived(
    !!$controller.pending ||
      !!$codexController.pending ||
      !!$codexController.snapshot?.busy ||
      !!$codexController.snapshot?.demo ||
      $codexController.login.open ||
      $codexController.preparation.open ||
      !!snapshot?.busy ||
      !!snapshot?.demo ||
      $controller.login.open ||
      !snapshot,
  );
  let detailsId = $state<string | null>(null);
  let detailsReturn = $state<HTMLElement | null>(null);
  let details = $derived(
    snapshot?.accounts.find((a) => a.id === detailsId) ?? null,
  );

  onMount(() => {
    void controller.start();
    const interval = setInterval(() => {
      now = Math.floor(Date.now() / 1000);
    }, 1000);
    return () => {
      clearInterval(interval);
      controller.dispose();
      codexController.dispose();
    };
  });
  $effect(() => {
    document.documentElement.dataset.theme =
      snapshot?.settings.appearance ?? 'system';
    const locale = snapshot?.settings.language ?? 'en';
    language.set(locale);
    document.documentElement.lang = locale;
    document.title = snapshot?.demo
      ? `PrimerSwitch · ${t(locale, 'preview')}`
      : 'PrimerSwitch';
  });
  $effect(() => {
    if (!$controller.login.open) loginCode = '';
  });
  async function switchAccount(account: AccountView) {
    operation = `switch:${account.id}`;
    await controller.action('switch_account', { id: account.id });
    operation = null;
  }
  async function removeAccount() {
    const target = deleteTarget;
    if (
      target &&
      (await controller.action('delete_account', { id: target.id }))
    )
      deleteTarget = null;
  }
  async function saveSettings(settings: SettingsType) {
    return controller.action('update_settings', { settings });
  }
</script>

{#if provider === 'claude'}
  <Dashboard
    {snapshot}
    oncodex={() => (provider = 'codex')}
    providerDisabled={!!$controller.pending ||
      !!snapshot?.busy ||
      $controller.login.open ||
      !!$codexController.pending}
    {now}
    {disabled}
    {operation}
    error={$controller.error}
    pending={$controller.pending}
    onsettings={() => (settingsOpen = true)}
    onlogin={() => {
      void controller.beginLogin();
    }}
    oncurrent={() => {
      void controller.action('import_current');
    }}
    onarchive={() => {
      void controller.action('preview_legacy_import');
    }}
    onrefresh={() => {
      void controller.action('refresh_all');
    }}
    onrefreshactive={(id) => {
      void controller.action('refresh_account', { id });
    }}
    onswitch={(account) => {
      void switchAccount(account);
    }}
    ondetails={(account, trigger) => {
      detailsReturn = trigger;
      detailsId = account.id;
    }}
    ondismiss={() => controller.dismissError()}
  />
{:else}<CodexProvider
    controller={codexController}
    {now}
    onclaude={() => (provider = 'claude')}
    onsettings={() => (settingsOpen = true)}
    settingsAvailable={!!snapshot}
    externalDemo={!!snapshot?.demo}
    externalBlocked={!!$controller.pending ||
      !!snapshot?.busy ||
      $controller.login.open}
  />
{/if}
{#if details && snapshot}
  <Modal
    title={details.name}
    returnFocus={detailsReturn}
    onclose={() => (detailsId = null)}
    ><div class="details-modal">
      <AccountCard
        account={details}
        {now}
        threshold={snapshot.settings.threshold}
        model={snapshot.activeModel}
        {disabled}
        pending={operation}
        onrefresh={() => {
          void controller.action('refresh_account', { id: details!.id });
        }}
        onswitch={() => {
          void switchAccount(details!);
        }}
        ondelete={() => {
          deleteTarget = details;
          detailsId = null;
        }}
        onrenewal={(day) => {
          void controller.action('set_renewal_day', { id: details!.id, day });
        }}
      />
    </div></Modal
  >
{/if}

{#if settingsOpen && snapshot}<Settings
    settings={snapshot.settings}
    {disabled}
    error={$controller.error}
    onsave={saveSettings}
    onclose={() => (settingsOpen = false)}
  />{/if}

{#if $controller.login.open}
  <Modal
    title={t($language, 'addClaude')}
    onclose={() => {
      void controller.cancelLogin();
    }}
  >
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void controller.finishLogin(loginCode);
      }}
    >
      <p class="modal-intro">{t($language, 'loginDescription')}</p>
      <ol class="login-steps">
        <li><span>1</span>{t($language, 'loginStepOne')}</li>
        <li>
          <span>2</span>{t($language, 'loginStepTwo')} <code>#state</code>.
        </li>
        <li><span>3</span>{t($language, 'loginStepThree')}</li>
      </ol>
      {#if $controller.login.working}<p class="working-message" role="status">
          <span class="spinner" aria-hidden="true"></span>{$controller.login
            .session
            ? t($language, 'addingAccount')
            : t($language, 'openingBrowser')}
        </p>{/if}
      <label class="field-label" for="login-code"
        >{t($language, 'authorizationCode')}</label
      ><input
        id="login-code"
        type="password"
        autocomplete="off"
        spellcheck="false"
        placeholder="code#state"
        bind:value={loginCode}
        disabled={!$controller.login.session || $controller.login.working}
      />
      {#if $controller.login.error}<p class="warning-text" role="alert">
          {safeError($controller.login.error, $language)}
        </p>{/if}
      <p class="help">{t($language, 'loginNoActivate')}</p>
      <div class="modal-actions">
        <button type="button" onclick={() => controller.cancelLogin()}
          >{t($language, 'cancel')}</button
        ><button
          class="primary"
          type="submit"
          disabled={!$controller.login.session ||
            $controller.login.working ||
            !loginCode.trim()}>{t($language, 'addAccount')}</button
        >
      </div>
    </form>
  </Modal>
{/if}
{#if deleteTarget}
  <Modal
    title={t($language, 'deleteTitle')}
    returnFocus={detailsReturn}
    onclose={() => (deleteTarget = null)}
    ><p class="modal-intro">
      {t($language, 'deletePrefix')} <strong>{deleteTarget.name}</strong>
      {t($language, 'deleteDescription')}
    </p>
    {#if $controller.error}<p class="warning-text" role="alert">
        {safeError($controller.error, $language)}
      </p>{/if}
    <div class="modal-actions">
      <button
        disabled={!!$controller.pending}
        onclick={() => (deleteTarget = null)}>{t($language, 'cancel')}</button
      ><button
        class="danger-button"
        {disabled}
        onclick={() => {
          void removeAccount();
        }}
        >{$controller.pending === 'delete_account'
          ? t($language, 'deleting')
          : t($language, 'deleteAccount')}</button
      >
    </div></Modal
  >
{/if}
{#if $controller.preview}
  <Modal
    title={t($language, 'importTitle')}
    onclose={() => controller.dismissPreview()}
    ><p class="modal-intro">
      {plural($language, 'importReady', $controller.preview.valid)}
    </p>
    {#if $controller.preview.invalid}<p class="warning-text">
        {plural($language, 'importInvalid', $controller.preview.invalid)}
      </p>{/if}
    <ul class="import-list">
      {#each $controller.preview.names as name, index (`${name}:${index}`)}<li>
          {name}
        </li>{/each}
    </ul>
    {#if $controller.error}<p class="warning-text" role="alert">
        {safeError($controller.error, $language)}
      </p>{/if}
    <div class="modal-actions">
      <button
        disabled={!!$controller.pending}
        onclick={() => controller.dismissPreview()}
        >{t($language, 'cancel')}</button
      ><button
        class="primary"
        disabled={disabled || !$controller.preview.valid}
        onclick={async () => {
          const preview = $controller.preview;
          if (
            preview &&
            (await controller.action('apply_legacy_import', {
              previewId: preview.id,
            }))
          )
            controller.dismissPreview();
        }}
        >{$controller.pending === 'apply_legacy_import'
          ? t($language, 'importing')
          : t($language, 'importAccounts')}</button
      >
    </div></Modal
  >
{/if}
