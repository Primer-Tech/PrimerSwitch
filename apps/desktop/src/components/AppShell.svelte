<script lang="ts">
  import type { Snippet } from 'svelte';
  import { language, t } from '../lib/i18n';
  import { age } from '../lib/format';
  import type { NoticeView, ProviderKey } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  import ProviderTabs from './ProviderTabs.svelte';
  import Notices from './Notices.svelte';
  import meta from '../../package.json';
  import primerLogo from '../assets/primer-logo.png';
  let {
    provider,
    subtitle,
    tabsDisabled = false,
    onprovider,
    settingsDisabled = false,
    onsettings,
    notices,
    updatedAt,
    now,
    refreshAll,
    children,
  }: {
    provider: ProviderKey;
    subtitle: string;
    tabsDisabled?: boolean;
    onprovider: (provider: ProviderKey) => void;
    settingsDisabled?: boolean;
    onsettings: () => void;
    notices: NoticeView[];
    /** The latest usage reading shown on this tab. */
    updatedAt: number | null;
    now: number;
    refreshAll: { label: string; disabled: boolean; run: () => void };
    children: Snippet;
  } = $props();
</script>

<div class="desktop-shell">
  <aside class="sidebar" aria-label={t($language, 'navigation')}>
    <div class="sidebar-brand" role="img" aria-label="PrimerSwitch">
      <img class="primer-mark" src={primerLogo} alt="" aria-hidden="true" />
      <span class="primer-wordmark" aria-hidden="true">
        <strong>primer</strong><span>Switch</span>
      </span>
    </div>
    <nav>
      <button
        class="nav-current"
        aria-current="page"
        onclick={() =>
          document
            .getElementById('accounts-heading')
            ?.scrollIntoView({ behavior: 'smooth' })}
        ><Icon name="accounts" /><span>{t($language, 'accounts')}</span></button
      ><button
        aria-label={t($language, 'openSettings')}
        disabled={settingsDisabled}
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
      <div class="workspace-title">
        <h1>{t($language, 'accounts')}</h1>
        <p>{subtitle}</p>
      </div>
      <ProviderTabs
        active={provider}
        onselect={(next) => {
          if (next !== provider) onprovider(next);
        }}
        disabled={tabsDisabled}
      />
    </header>
    <Notices {notices} />
    <div
      id="provider-content"
      role="tabpanel"
      aria-labelledby={`provider-${provider}`}
      tabindex="0"
    >
      {@render children()}
    </div>
    <footer class="workspace-footer">
      <div>
        <span class="status-dot" class:inactive={!updatedAt}></span><span
          >{updatedAt
            ? t($language, 'updatedAge', {
                age: age(updatedAt, now, $language),
              })
            : t($language, 'noReadings')}</span
        >
      </div>
      <span class="version">PrimerSwitch {meta.version}</span><button
        class="quiet-button"
        disabled={refreshAll.disabled}
        onclick={refreshAll.run}
        ><Icon name="refresh" size={15} />{refreshAll.label}</button
      >
    </footer>
  </div>
</div>
