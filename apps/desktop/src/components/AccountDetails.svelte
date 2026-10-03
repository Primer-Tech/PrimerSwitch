<script lang="ts">
  import type { Snippet } from 'svelte';
  import { language, t } from '../lib/i18n';
  import { age } from '../lib/format';
  import type { AccountRowView } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  import StatusLabel from './StatusLabel.svelte';
  import UsageMeters from './UsageMeters.svelte';
  let {
    row,
    now,
    threshold,
    locked,
    lockReason,
    facts,
    onrefresh,
    onswitch,
    onsignin,
    ondelete,
  }: {
    row: AccountRowView;
    now: number;
    threshold: number;
    locked: boolean;
    lockReason: string | null;
    /** Provider-specific facts, as `<div><dt/><dd/></div>` rows. */
    facts?: Snippet;
    onrefresh: (trigger: HTMLElement) => void;
    onswitch: (trigger: HTMLElement) => void;
    onsignin: (trigger: HTMLElement) => void;
    ondelete: (trigger: HTMLElement) => void;
  } = $props();
  const uid = $props.id();
</script>

<div class="details">
  <div class="summary">
    <span class="avatar large-avatar" aria-hidden="true">{row.initial}</span>
    <div class="summary-text">
      {#if row.plan || row.active}<div class="identity-line">
          {#if row.plan}<span class="badge muted">{row.plan}</span
            >{/if}{#if row.active}<span class="badge accent"
              >{t($language, 'active')}</span
            >{/if}
        </div>{/if}
      {#if row.secondary}<p class="secondary">{row.secondary}</p>{/if}
      <StatusLabel status={row.status} />
    </div>
  </div>
  <UsageMeters {row} {now} {threshold} />
  {#each row.notes as note, index (index)}<p
      class="note"
      class:warning-text={note.tone === 'warn'}
    >
      {note.text}
    </p>{/each}
  <p class="identity" class:ok={row.identityOk}>
    <Icon name={row.identityOk ? 'shield' : 'info'} size={16} /><span
      >{row.identity}</span
    >
  </p>
  <dl class="facts">
    {@render facts?.()}
    <div>
      <dt>{t($language, 'detailsChecked')}</dt>
      <dd class:warning-text={row.savedReading && row.readAt !== null}>
        {row.readAt === null
          ? t($language, 'statusUnread')
          : row.savedReading
            ? t($language, 'cachedAge', {
                age: age(row.readAt, now, $language),
              })
            : age(row.readAt, now, $language)}
      </dd>
    </div>
  </dl>
  <div class="actions">
    <button
      class="delete"
      disabled={locked || !row.canDelete}
      title={row.deleteReason ?? undefined}
      aria-label={t($language, 'deleteLabel', { name: row.name })}
      aria-describedby={row.deleteReason ? `${uid}-delete` : undefined}
      onclick={(event) => ondelete(event.currentTarget)}
      ><Icon name="trash" size={15} />{t($language, 'deleteAccount')}</button
    >
    <div class="primary-actions">
      {#if row.canRefresh}<button
          disabled={locked}
          title={locked ? (lockReason ?? undefined) : undefined}
          aria-label={t($language, 'refreshLabel', { name: row.name })}
          onclick={(event) => onrefresh(event.currentTarget)}
          ><Icon name="refresh" size={15} />{t($language, 'refresh')}</button
        >{/if}
      {#if row.primary === 'signIn'}<button
          class="primary"
          disabled={locked || !row.canSignIn}
          onclick={(event) => onsignin(event.currentTarget)}
          >{t($language, 'signInAgain')}</button
        >{:else if row.primary === 'switch'}<span
          class="switch-wrap"
          title={row.switchReason ??
            (locked ? (lockReason ?? undefined) : undefined)}
          ><button
            class="primary"
            disabled={locked || !row.canSwitch}
            aria-describedby={row.switchReason ? `${uid}-switch` : undefined}
            onclick={(event) => onswitch(event.currentTarget)}
            >{row.switching
              ? t($language, 'switching')
              : t($language, 'switch')}</button
          ></span
        >{/if}
    </div>
  </div>
  {#if row.switchReason && row.primary === 'switch'}<p
      class="help reason"
      id={`${uid}-switch`}
    >
      {row.switchReason}
    </p>{/if}
  {#if row.deleteReason}<p class="help reason" id={`${uid}-delete`}>
      {row.deleteReason}
    </p>{/if}
</div>

<style>
  .details {
    min-width: 0;
    margin-top: 14px;
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
  }
  .summary-text {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .secondary {
    margin: 0;
    color: var(--subtle);
    font-size: 0.78rem;
    overflow-wrap: anywhere;
  }
  .note {
    margin: 10px 0 0;
    font-size: 0.76rem;
    line-height: 1.5;
    color: var(--subtle);
  }
  .identity {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 14px 0 0;
    font-size: 0.78rem;
    line-height: 1.5;
    color: var(--subtle);
  }
  .identity :global(svg) {
    flex: none;
    margin-top: 1px;
  }
  .identity.ok :global(svg) {
    color: var(--green);
  }
  .facts {
    margin: 10px 0 0;
    font-size: 0.78rem;
  }
  .facts > :global(div) {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }
  .facts :global(dt) {
    color: var(--subtle);
  }
  .facts :global(dd) {
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
  }
  .facts :global(.fact-note) {
    display: block;
    margin-top: 3px;
    color: var(--subtle);
    font-size: 0.7rem;
  }
  .actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 20px;
  }
  .primary-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .switch-wrap {
    display: inline-flex;
  }
  button.delete {
    color: var(--danger);
    background: transparent;
    border-color: color-mix(in srgb, var(--danger) 40%, var(--line));
  }
  button.delete:hover:not(:disabled) {
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    border-color: var(--danger);
  }
  .reason {
    margin-top: 10px;
  }
</style>
