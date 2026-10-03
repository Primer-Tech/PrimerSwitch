<script lang="ts">
  import { language, t } from '../lib/i18n';
  import {
    codexAccountPlan,
    codexCanSwitch,
    codexInitial,
    codexMainLimit,
    codexWindowLabel,
    codexWindows,
  } from '../lib/codex-format';
  import type { CodexAccount, CodexSnapshot } from '../lib/codex-types';
  import Icon from './Icon.svelte';
  import CodexIcon from './CodexIcon.svelte';
  import QuotaBar from './QuotaBar.svelte';
  let {
    snapshot,
    now,
    locked,
    nextBest,
    checkingSetup,
    onswitch,
    ondiscover,
  }: {
    snapshot: CodexSnapshot | null;
    now: number;
    locked: boolean;
    nextBest: CodexAccount | null;
    checkingSetup: boolean;
    onswitch: (account: CodexAccount, trigger: HTMLElement) => void;
    ondiscover: () => void;
  } = $props();
  let windows = $derived(
    nextBest ? codexWindows(codexMainLimit(nextBest)) : null,
  );
  let plan = $derived(nextBest ? codexAccountPlan(nextBest, $language) : null);
  let environment = $derived(snapshot?.environment ?? null);
  let setupState = $derived(
    !snapshot ||
      checkingSetup ||
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

{#if nextBest && windows}<section
    class="panel next-panel codex-next"
    aria-labelledby="codex-next-heading"
  >
    <h2 id="codex-next-heading">
      <Icon name="next" />{t($language, 'codexNextBest')}
    </h2>
    <div class="next-identity">
      <span class="avatar" aria-hidden="true">{codexInitial(nextBest)}</span>
      <div>
        <strong title={nextBest.name}>{nextBest.name}</strong><span
          >{plan ? `${plan} · ` : ''}{t($language, 'codexStatusReady')}</span
        >
      </div>
    </div>
    <div class="next-quotas">
      {#if windows.short}<QuotaBar
          compact
          label={codexWindowLabel(
            windows.short.windowDurationMins,
            'short',
            $language,
          )}
          value={windows.short.usedPercent}
          {now}
          threshold={100}
        />{/if}{#if windows.long}<QuotaBar
          compact
          label={codexWindowLabel(
            windows.long.windowDurationMins,
            'long',
            $language,
          )}
          value={windows.long.usedPercent}
          {now}
          threshold={100}
          tone="green"
        />{/if}
    </div>
    <p>{t($language, 'codexNextBestText')}</p>
    <button
      class="primary switch-now"
      disabled={locked || !snapshot || !codexCanSwitch(snapshot, nextBest)}
      aria-label={t($language, 'codexSwitchNowLabel', { name: nextBest.name })}
      onclick={(event) => onswitch(nextBest!, event.currentTarget)}
      ><CodexIcon name="swap" size={16} />{t(
        $language,
        'codexSwitchNow',
      )}</button
    >
  </section>{/if}
<section class="panel codex-how" aria-labelledby="codex-how-heading">
  <h2 id="codex-how-heading">
    <CodexIcon name="terminal" />{t($language, 'codexHowTitle')}
  </h2>
  <p>{t($language, 'codexHowText')}</p>
  <p class="apps">{t($language, 'codexHowApps')}</p>
  {#if environment && (environment.daemonRunning !== null || environment.otherClients > 0)}<ul
      class="environment"
    >
      {#if environment.daemonRunning !== null}<li>
          <span class="status-dot" class:inactive={!environment.daemonRunning}
          ></span>{t(
            $language,
            environment.daemonRunning ? 'codexEnvDaemon' : 'codexEnvNoDaemon',
          )}
        </li>{/if}
      {#if environment.otherClients > 0}<li class="other-clients">
          <span class="status-dot"></span>{t($language, 'codexEnvOtherClients')}
        </li>{/if}
    </ul>{/if}
</section>
<section
  class="panel codex-setup-card"
  aria-label={t($language, 'codexSetupTitle')}
>
  <p class="setup-line" data-state={setupState}>
    {#if setupState === 'checking'}<span class="spinner" aria-hidden="true"
      ></span>{:else}<span class="status-dot"></span>{/if}<span
      >{setupText}</span
    >
  </p>
  <button
    class="text-button"
    disabled={locked || checkingSetup}
    onclick={ondiscover}
    ><Icon name="refresh" size={14} />{t($language, 'codexCheckSetup')}</button
  >
</section>

<style>
  .codex-next h2,
  .codex-how h2 {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .codex-next .next-identity strong {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .codex-next .next-identity > div {
    min-width: 0;
  }
  .codex-next :global(.quota-label) {
    min-height: 0;
  }
  .switch-now {
    width: 100%;
    margin-top: 15px;
  }
  .codex-how .apps {
    font-size: 0.72rem;
    padding-top: 11px;
    border-top: 1px solid var(--line);
  }
  .environment {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 0.72rem;
    line-height: 1.5;
    color: var(--text);
  }
  .environment li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .environment .status-dot {
    margin-top: 0.45em;
  }
  .environment .other-clients .status-dot {
    background: var(--codex-warn);
  }
  .panel.codex-setup-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 6px 12px;
    padding-top: 12px;
    padding-bottom: 12px;
  }
  .panel .setup-line {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.76rem;
    font-weight: 600;
    margin: 0;
    min-width: 0;
  }
  .setup-line[data-state='missing'] .status-dot,
  .setup-line[data-state='attention'] .status-dot {
    background: var(--danger);
  }
  .setup-line[data-state='checking'] {
    color: var(--subtle);
    font-weight: 500;
  }
  .codex-setup-card .text-button {
    font-size: 0.72rem;
    padding: 4px 0;
    min-height: 0;
    gap: 5px;
  }
</style>
