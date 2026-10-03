<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { age } from '../lib/format';
  import type { AccountRowView, PageView } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  import UsageMeters from './UsageMeters.svelte';
  let {
    view,
    now,
    locked,
    lockReason,
    refreshing = false,
    empty,
    onrefresh,
    ondetails,
    onswitch,
    onsignin,
  }: {
    view: PageView;
    now: number;
    /** Any mutation, sign-in, switch or demo lock. */
    locked: boolean;
    /** Why buttons are disabled while locked. */
    lockReason: string | null;
    refreshing?: boolean;
    /** What the card says without an active account. */
    empty: {
      title: string;
      text: string | null;
      action?: { label: string; disabled: boolean; run: () => void };
    };
    onrefresh: (row: AccountRowView, trigger: HTMLElement) => void;
    ondetails: (row: AccountRowView, trigger: HTMLElement) => void;
    onswitch: (row: AccountRowView, trigger: HTMLElement) => void;
    onsignin: (row: AccountRowView, trigger: HTMLElement) => void;
  } = $props();
  let active = $derived(view.active);
  let next = $derived(view.next);
  let caption = $derived(
    !active || active.readAt === null
      ? t($language, 'statusUnread')
      : t($language, active.savedReading ? 'cachedAge' : 'updatedAge', {
          age: age(active.readAt, now, $language),
        }),
  );
</script>

<section class="panel active-panel" aria-labelledby="active-heading">
  <div class="panel-heading">
    <h2 id="active-heading">
      <span class="status-dot" class:inactive={!active}></span>{t(
        $language,
        'activeAccount',
      )}
    </h2>
    {#if active?.canRefresh}<button
        class="quiet-button"
        disabled={locked}
        title={locked ? (lockReason ?? undefined) : undefined}
        aria-label={t($language, 'refreshLabel', { name: active.name })}
        onclick={(event) => onrefresh(active!, event.currentTarget)}
        ><span class="refresh-icon" class:spinning={refreshing}
          ><Icon name="refresh" size={16} /></span
        >{t($language, refreshing ? 'statusChecking' : 'refresh')}</button
      >{/if}
  </div>
  {#if active}
    <div class="active-title">
      <span class="avatar large-avatar" aria-hidden="true"
        >{active.initial}</span
      >
      <div class="active-identity">
        <div class="identity-line">
          <h3>{active.name}</h3>
          {#if active.plan}<span class="badge muted">{active.plan}</span
            >{/if}<span class="badge accent">{t($language, 'active')}</span>
        </div>
        {#if active.secondary}<p>{active.secondary}</p>{/if}
      </div>
      {#if view.model}<span class="active-model">{view.model}</span>{/if}
    </div>
    {#if active.status.kind === 'signIn'}<div class="callout danger sign-in">
        <Icon name="warning" />
        <p>{active.identity}</p>
        {#if active.canSignIn}<button
            class="primary"
            disabled={locked}
            onclick={(event) => onsignin(active!, event.currentTarget)}
            >{t($language, 'signInAgain')}</button
          >{/if}
      </div>{/if}
    <UsageMeters row={active} {now} threshold={view.threshold} />
    {#if view.alert && active.status.kind !== 'signIn'}<div
        class="callout"
        class:danger={view.alertLimit === 'limited'}
      >
        <Icon name="warning" />
        <p>{view.alert}</p>
        {#if next}<button
            class="primary"
            disabled={locked || !next.canSwitch}
            title={locked ? (lockReason ?? undefined) : undefined}
            onclick={(event) => onswitch(next!, event.currentTarget)}
            >{t($language, 'switchToNow', { name: next.name })}</button
          >{/if}
      </div>{/if}
    {#each active.notes as note, index (index)}<p
        class="card-note"
        class:warning-text={note.tone === 'warn'}
      >
        {note.text}
      </p>{/each}
    <div class="active-caption">
      <span class="caption-text"
        ><span class:warning-text={active.savedReading && !!active.readAt}
          >{caption}</span
        >{#if active.credits}<span class="caption-credits"
            >{t($language, 'credits')} <strong>{active.credits}</strong></span
          >{/if}</span
      ><button
        class="text-button"
        onclick={(event) => ondetails(active!, event.currentTarget)}
        >{t($language, 'viewDetails')}<Icon name="next" size={14} /></button
      >
    </div>
  {:else}<div class="empty-active">
      <h3>{empty.title}</h3>
      {#if empty.text}<p>{empty.text}</p>{/if}
      {#if empty.action}{@const action = empty.action}<button
          disabled={action.disabled}
          onclick={action.run}
          ><Icon name="refresh" size={16} />{action.label}</button
        >{/if}
    </div>{/if}
</section>

<style>
  .active-panel {
    padding: 14px 18px;
  }
  .active-title {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 11px 0 12px;
  }
  .active-identity {
    min-width: 0;
  }
  .active-identity h3 {
    margin: 0;
    font-size: 1.12rem;
    letter-spacing: -0.02em;
    overflow-wrap: anywhere;
  }
  .active-identity p {
    font-size: 0.76rem;
    color: var(--subtle);
    margin: 5px 0 0;
    overflow-wrap: anywhere;
  }
  .active-model {
    margin-left: auto;
    color: var(--subtle);
    font-size: 0.72rem;
    max-width: 150px;
    text-align: right;
    overflow-wrap: anywhere;
  }
  .refresh-icon {
    display: inline-flex;
  }
  .refresh-icon.spinning {
    animation: card-spin 0.9s linear infinite;
  }
  .callout {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 12px;
    padding: 9px 12px;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line));
    border-radius: 8px;
    background: color-mix(in srgb, var(--warn) 7%, var(--surface));
    color: var(--warn);
    font-size: 0.78rem;
    line-height: 1.45;
  }
  .callout.danger {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 40%, var(--line));
    background: color-mix(in srgb, var(--danger) 7%, var(--surface));
  }
  .callout.sign-in {
    margin: 0 0 12px;
  }
  .callout > :global(svg) {
    flex: none;
  }
  .callout p {
    flex: 1;
    min-width: 0;
    margin: 0;
  }
  .callout button {
    flex: none;
    max-width: 55%;
    font-size: 0.75rem;
    min-height: 30px;
    padding: 0.35rem 0.75rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .card-note {
    margin: 10px 0 0;
    font-size: 0.74rem;
    line-height: 1.5;
    color: var(--subtle);
  }
  .card-note.warning-text {
    color: var(--danger);
  }
  .active-caption {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: 10px;
    border-top: 1px solid var(--line);
    padding-top: 7px;
    font-size: 0.74rem;
    color: var(--subtle);
  }
  .active-caption .text-button {
    font-size: 0.74rem;
    padding: 4px 0;
    min-height: 0;
    gap: 5px;
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
  .empty-active {
    padding: 26px 0 12px;
  }
  .empty-active h3 {
    font-size: 1.3rem;
    font-weight: 550;
    letter-spacing: -0.03em;
  }
  .empty-active p {
    color: var(--subtle);
    font-size: 0.82rem;
    line-height: 1.6;
    margin-top: 8px;
    max-width: 62ch;
  }
  .empty-active button {
    margin-top: 14px;
    font-size: 0.76rem;
  }
  @keyframes card-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .refresh-icon.spinning {
      animation: none;
    }
  }
  @media (max-width: 1130px) {
    .active-model {
      display: none;
    }
  }
  @media (max-width: 560px) {
    .active-panel {
      padding: 14px;
    }
    .active-title {
      margin: 12px 0;
    }
    .active-identity h3 {
      font-size: 1rem;
    }
    .callout {
      flex-wrap: wrap;
    }
    .callout button {
      max-width: 100%;
      width: 100%;
    }
    .active-caption {
      align-items: flex-start;
      font-size: 0.68rem;
    }
  }
</style>
