<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { safeError } from '../lib/controller';
  import { resetsView } from '../lib/claude-resets';
  import type { AccountView, ResetKind } from '../lib/types';
  let {
    account,
    now,
    locked,
    working = null,
    heading = null,
    onuse,
  }: {
    account: AccountView;
    now: number;
    /** A command is running, or the view is a demo: nothing can be started. */
    locked: boolean;
    /** The kind whose request is running for this account. */
    working?: ResetKind | null;
    /** A section title for the details; the side card brings its own. */
    heading?: string | null;
    /** Asks to use a kind; the page confirms it with the user first. */
    onuse: (kind: ResetKind, trigger: HTMLElement) => void;
  } = $props();
  const uid = $props.id();
  let resets = $derived(resetsView(account, now, $language));
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
          <div class="reset-text">
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
          </div>
          <button
            class="primary"
            disabled={locked || !row.canUse}
            aria-label={t(
              $language,
              row.retry ? 'resetRetryLabel' : 'resetUseLabel',
              { kind: row.label, name: account.name },
            )}
            aria-describedby={row.reason ? `${uid}-${row.kind}` : undefined}
            onclick={(event) => onuse(row.kind, event.currentTarget)}
            >{working === row.kind
              ? t($language, 'resetUsing')
              : t($language, row.retry ? 'resetRetry' : 'resetUse')}</button
          >
        </li>
      {/each}
    </ul>
  {:else}
    <p class="help">{t($language, 'resetsNone')}</p>
  {/if}
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
  .reset-rows li {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }
  .reset-text {
    min-width: 0;
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
  .reset-rows button {
    flex: none;
    font-size: 0.75rem;
  }
</style>
