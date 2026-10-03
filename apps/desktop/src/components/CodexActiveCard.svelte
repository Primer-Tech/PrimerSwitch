<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { age } from '../lib/format';
  import {
    codexAccountPlan,
    codexCanSwitch,
    codexCredits,
    codexDuration,
    codexInitial,
    codexLimitState,
    codexMessage,
    codexSecondaryLine,
  } from '../lib/codex-format';
  import type { CodexAccount, CodexSnapshot } from '../lib/codex-types';
  import Icon from './Icon.svelte';
  import CodexIcon from './CodexIcon.svelte';
  import CodexQuotas from './CodexQuotas.svelte';
  let {
    snapshot,
    account,
    now,
    locked,
    refreshing,
    nextBest,
    onrefresh,
    ondetails,
    onswitch,
    onsignin,
  }: {
    snapshot: CodexSnapshot | null;
    account: CodexAccount | null;
    now: number;
    locked: boolean;
    refreshing: boolean;
    nextBest: CodexAccount | null;
    onrefresh: (account: CodexAccount) => void;
    ondetails: (account: CodexAccount, trigger: HTMLElement) => void;
    onswitch: (account: CodexAccount, trigger: HTMLElement) => void;
    onsignin: (account: CodexAccount, trigger: HTMLElement) => void;
  } = $props();
  let plan = $derived(account ? codexAccountPlan(account, $language) : null);
  let secondary = $derived(account ? codexSecondaryLine(account) : '');
  let limit = $derived(
    account ? codexLimitState(account) : { limited: false, freesAt: null },
  );
  let credits = $derived(account ? codexCredits(account, $language) : null);
  let canRefresh = $derived(
    !!account &&
      !locked &&
      !!snapshot?.capabilities.refreshQuota.enabled &&
      account.authKind === 'chatgpt' &&
      !account.needsSignIn,
  );
</script>

<section
  class="panel active-panel codex-active"
  aria-labelledby="codex-active-heading"
>
  <div class="panel-heading">
    <h2 id="codex-active-heading">
      <span class="status-dot" class:inactive={!account}></span>{t(
        $language,
        'codexActiveAccount',
      )}
    </h2>
    {#if account && account.authKind === 'chatgpt'}<button
        class="quiet-button"
        disabled={!canRefresh}
        aria-label={t($language, 'codexRefreshLabel', { name: account.name })}
        onclick={() => onrefresh(account!)}
        ><span class="refresh-icon" class:spinning={refreshing}
          ><Icon name="refresh" /></span
        >{t(
          $language,
          refreshing ? 'codexStatusChecking' : 'codexRefresh',
        )}</button
      >{/if}
  </div>
  {#if account}
    <div class="active-title">
      <span class="avatar large-avatar" aria-hidden="true"
        >{codexInitial(account)}</span
      >
      <div class="codex-identity">
        <div class="identity-line">
          <h3>{account.name}</h3>
          {#if plan}<span class="badge muted">{plan}</span>{/if}<span
            class="badge accent">{t($language, 'codexActiveBadge')}</span
          >
        </div>
        {#if secondary}<p>{secondary}</p>{/if}
      </div>
      {#if snapshot?.activeModel}<span class="active-model"
          >{snapshot.activeModel}</span
        >{/if}
    </div>
    {#if account.needsSignIn}<div class="codex-callout danger">
        <CodexIcon name="warning" />
        <p>{codexMessage('signInRequired', $language)}</p>
        <button
          class="primary"
          disabled={locked || !snapshot?.capabilities.loginBrowser.enabled}
          onclick={(event) => onsignin(account!, event.currentTarget)}
          >{t($language, 'codexStatusSignIn')}</button
        >
      </div>{/if}
    <CodexQuotas {account} {now} showCredits={false} />
    {#if limit.limited && !account.needsSignIn}<div class="codex-callout">
        <CodexIcon name="warning" />
        <p>
          {limit.freesAt !== null && limit.freesAt > now
            ? t($language, 'codexLimitReachedFreesIn', {
                duration: codexDuration(limit.freesAt - now, $language),
              })
            : t($language, 'codexStatusLimited')}
        </p>
        {#if nextBest}<button
            class="primary"
            disabled={locked ||
              !snapshot ||
              !codexCanSwitch(snapshot, nextBest)}
            onclick={(event) => onswitch(nextBest!, event.currentTarget)}
            >{t($language, 'codexSwitchTo', { name: nextBest.name })}</button
          >{/if}
      </div>{/if}
    {#if account.error && !account.needsSignIn}<p
        class="warning-text codex-active-error"
      >
        {codexMessage(account.error, $language)}
      </p>{/if}
    <div class="active-caption">
      <span
        >{account.quotaReadAt === null
          ? t($language, 'codexStatusUnread')
          : t($language, 'codexReadAge', {
              age: age(account.quotaReadAt, now, $language),
            })}{#if credits}<span class="caption-credits"
            >{t($language, 'codexCredits')} <strong>{credits}</strong></span
          >{/if}</span
      ><button
        class="text-button"
        onclick={(event) => ondetails(account!, event.currentTarget)}
        >{t($language, 'codexViewDetails')}<Icon
          name="next"
          size={14}
        /></button
      >
    </div>
  {:else}<div class="empty-active">
      <div>
        <h3>
          {t($language, snapshot ? 'codexNoActiveTitle' : 'codexLoading')}
        </h3>
        {#if snapshot}<p>{t($language, 'codexNoActiveText')}</p>{/if}
      </div>
    </div>{/if}
</section>

<style>
  .codex-identity {
    min-width: 0;
  }
  .codex-identity h3 {
    overflow-wrap: anywhere;
  }
  .refresh-icon {
    display: inline-flex;
  }
  .refresh-icon.spinning {
    animation: codex-spin 0.9s linear infinite;
  }
  .codex-callout {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 14px;
    padding: 11px 13px;
    border: 1px solid color-mix(in srgb, var(--danger) 35%, var(--line));
    border-radius: 8px;
    background: color-mix(in srgb, var(--danger) 7%, var(--surface));
    color: var(--danger);
    font-size: 0.78rem;
    line-height: 1.5;
  }
  .codex-callout.danger {
    margin: 0 0 14px;
  }
  .codex-callout > :global(svg) {
    flex: none;
  }
  .codex-callout p {
    flex: 1;
    min-width: 0;
  }
  .codex-callout button {
    flex: none;
    max-width: 55%;
    font-size: 0.75rem;
    padding: 0.45rem 0.75rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .codex-active-error {
    margin-top: 12px;
  }
  .caption-credits::before {
    content: '·';
    margin: 0 8px;
  }
  .caption-credits strong {
    color: var(--text);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  @keyframes codex-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .refresh-icon.spinning {
      animation: none;
    }
  }
  @media (max-width: 560px) {
    .codex-callout {
      flex-wrap: wrap;
    }
    .codex-callout button {
      max-width: 100%;
      width: 100%;
    }
  }
</style>
