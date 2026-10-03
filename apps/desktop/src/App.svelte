<script lang="ts">
  import { language, t } from './lib/i18n';
  import { onMount } from 'svelte';
  import { createController, type Controller } from './lib/controller';
  import type { Settings as SettingsType } from './lib/types';
  import type { ProviderKey } from './lib/provider-view';
  import {
    createCodexController,
    type CodexController,
  } from './lib/codex-controller';
  import ClaudeProvider from './components/ClaudeProvider.svelte';
  import CodexProvider from './components/CodexProvider.svelte';
  import Settings from './components/Settings.svelte';

  let {
    controller = createController(),
    codexController = createCodexController(),
  }: { controller?: Controller; codexController?: CodexController } = $props();
  let provider = $state<ProviderKey>('claude');
  let now = $state(Math.floor(Date.now() / 1000));
  let settingsOpen = $state(false);
  let snapshot = $derived($controller.snapshot);
  /** Claude mutations wait while anything runs on either provider. */
  let locked = $derived(
    !!$controller.pending ||
      !!$codexController.pending ||
      !!$codexController.snapshot?.busy ||
      !!$codexController.snapshot?.demo ||
      !!$codexController.snapshot?.switching ||
      $codexController.login.open ||
      !!snapshot?.busy ||
      !!snapshot?.demo ||
      $controller.login.open ||
      !snapshot,
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
  async function saveSettings(settings: SettingsType) {
    return controller.action('update_settings', { settings });
  }
</script>

{#if provider === 'claude'}<ClaudeProvider
    {controller}
    {now}
    {locked}
    tabsDisabled={!!$controller.pending ||
      !!snapshot?.busy ||
      $controller.login.open ||
      !!$codexController.pending}
    onprovider={(next) => (provider = next)}
    settingsDisabled={!snapshot}
    onsettings={() => (settingsOpen = true)}
  />{:else}<CodexProvider
    controller={codexController}
    {now}
    onprovider={(next) => (provider = next)}
    onsettings={() => (settingsOpen = true)}
    settingsDisabled={!snapshot}
    automation={snapshot?.settings ?? null}
    externalDemo={!!snapshot?.demo}
    externalBlocked={!!$controller.pending ||
      !!snapshot?.busy ||
      $controller.login.open}
  />{/if}

{#if settingsOpen && snapshot}<Settings
    settings={snapshot.settings}
    disabled={locked}
    error={$controller.error}
    onsave={saveSettings}
    onclose={() => (settingsOpen = false)}
  />{/if}
