<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { safeError } from '../lib/controller';
  import { resetsView, type ResetSelection } from '../lib/claude-resets';
  import type { AccountView } from '../lib/types';
  let {
    account,
    now,
    locked,
    working = false,
    heading = null,
    onuse,
  }: {
    account: AccountView;
    now: number;
    /** A command is running, or the view is a demo: nothing can be started. */
    locked: boolean;
    /** A request to use a reset on this account is running. */
    working?: boolean;
    /** A section title for the details; the side card brings its own. */
    heading?: string | null;
    /**
     * Asks to use a reset. It passes the kinds that can be used now; the page asks which
     * one when there are several, and confirms it with the user first.
     */
    onuse: (kinds: ResetSelection[], trigger: HTMLElement) => void;
  } = $props();
  const uid = $props.id();
  let resets = $derived(resetsView(account, now, $language));
  let canUse = $derived(
    !!account.resets.retryPending || resets.usable.length > 0,
  );
  // While nothing can be used the button points at the reason it is disabled.
  let describedBy = $derived(
    canUse
      ? undefined
      : resets.rows.length === 0
        ? `${uid}-none`
        : resets.sharedReason
          ? `${uid}-${resets.rows[0].kind}`
          : `${uid}-generic`,
  );
</script>

<section
  class="reset-controls"
  aria-labelledby={heading ? `${uid}-heading` : undefined}
>
  {#if heading}<h3 class="reset-heading" id={`${uid}-heading`}>
      {heading}
    </h3>{/if}
  {#if resets.rows.length}
    <ul class="reset-rows" aria-label={t($language, 'availableResets')}>
      {#each resets.rows as row (row.kind)}
        <li>
          <p class="reset-name">
            <span>{row.label}</span><span class="reset-left">{row.left}</span>
          </p>
          <p class="help reset-meta">{row.effect} · {row.useBy}</p>
          {#if row.reason}<p
              class="help reset-reason"
              id={`${uid}-${row.kind}`}
            >
              {row.reason}
            </p>{/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="help" id={`${uid}-none`}>{t($language, 'resetsNone')}</p>
  {/if}
  {#if describedBy === `${uid}-generic`}<p class="help" id={describedBy}>
      {t($language, 'resetNoneUsable')}
    </p>{/if}
  <button
    class="primary reset-use"
    disabled={locked || working || !canUse}
    aria-describedby={describedBy}
    onclick={(event) => {
      if (locked || working || !canUse) return;
      onuse(
        account.resets.retryPending
          ? ['pending']
          : resets.usable.map((row) => row.kind),
        event.currentTarget,
      );
    }}
    >{working
      ? t($language, 'resetUsing')
      : t(
          $language,
          resets.retry || account.resets.retryPending
            ? 'resetRetry'
            : 'resetUse',
        )}</button
  >
  {#if account.decisionFresh === false && resets.rows.length}<p class="help">
      {t($language, 'resetsCached')}
    </p>{/if}
  {#if resets.pending}<p class="help warning-text">
      {t($language, 'checkingReset')}
    </p>{/if}
  {#if resets.lastOutcome}<p class="help" role="status">
      {safeError(resets.lastOutcome, $language)}
    </p>{/if}
</section>

<style>
  .reset-controls {
    min-width: 0;
  }
  .reset-heading {
    margin: 18px 0 0;
    font-size: 0.84rem;
  }
  .reset-rows {
    display: grid;
    gap: 12px;
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
  }
  .reset-name {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    margin: 0;
    font-size: 0.84rem;
    font-weight: 600;
  }
  .reset-left {
    font-weight: 400;
    color: var(--subtle);
  }
  .reset-meta,
  .reset-reason {
    margin: 2px 0 0;
    overflow-wrap: anywhere;
  }
  .reset-use {
    margin-top: 14px;
    font-size: 0.78rem;
  }
</style>
