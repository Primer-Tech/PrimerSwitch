<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { percentage } from '../lib/format';
  import type { AccountRowView, PageView } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  import QuotaBar from './QuotaBar.svelte';
  let {
    view,
    now,
    locked,
    lockReason,
    onswitch,
  }: {
    view: PageView;
    now: number;
    locked: boolean;
    lockReason: string | null;
    onswitch: (row: AccountRowView, trigger: HTMLElement) => void;
  } = $props();
  let next = $derived(view.next);
  let meta = $derived(
    next ? [next.plan, next.secondary].filter(Boolean).join(' · ') : '',
  );
</script>

<section class="panel side-panel next-panel" aria-labelledby="next-heading">
  <h2 id="next-heading"><Icon name="next" />{t($language, 'nextUp')}</h2>
  {#if next}
    <div class="next-identity">
      <span class="avatar" aria-hidden="true">{next.initial}</span>
      <div>
        <strong title={next.name}>{next.name}</strong>
        {#if meta}<span>{meta}</span>{/if}
      </div>
    </div>
    <div class="next-quotas">
      <QuotaBar
        compact
        reset={false}
        meter={next.fiveHour}
        {now}
        threshold={view.threshold}
      /><QuotaBar
        compact
        reset={false}
        meter={next.weekly}
        {now}
        threshold={view.threshold}
      />
    </div>
    <p>{view.nextReason}</p>
    {#if view.settings?.autoSwitchEnabled}<p class="next-automatic">
        {t($language, 'nextAutomatic', {
          threshold: percentage(view.threshold, $language),
        })}
      </p>{/if}
    <button
      class="primary switch-now"
      disabled={locked || !next.canSwitch}
      title={locked
        ? (lockReason ?? undefined)
        : (next.switchReason ?? undefined)}
      aria-label={t($language, 'switchNowLabel', { name: next.name })}
      onclick={(event) => onswitch(next!, event.currentTarget)}
      ><Icon name="swap" size={16} />{t($language, 'switchNow')}</button
    >
  {:else}<p>{t($language, 'noNext')}</p>{/if}
</section>

<style>
  .next-identity {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 11px 0 12px;
  }
  .next-identity > div {
    min-width: 0;
  }
  .next-identity strong {
    display: block;
    font-size: 0.84rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .next-identity div > span {
    display: block;
    font-size: 0.7rem;
    color: var(--subtle);
    margin-top: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .next-identity .avatar {
    color: var(--green);
    background: color-mix(in srgb, var(--green) 13%, var(--surface));
  }
  .next-quotas {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }
  .next-quotas :global(.quota-label) {
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }
  .next-quotas :global(.quota-label strong) {
    font-size: 1rem;
  }
  p.next-automatic {
    margin-top: 4px;
  }
  .switch-now {
    width: 100%;
    margin-top: 10px;
  }
</style>
