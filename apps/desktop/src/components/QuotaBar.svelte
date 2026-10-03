<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { duration, percentage } from '../lib/format';
  import type { MeterView } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  let {
    meter,
    now,
    threshold = 95,
    compact = false,
    reset = true,
  }: {
    meter: MeterView;
    now: number;
    /** At or above it the bar turns to the warning color. */
    threshold?: number;
    compact?: boolean;
    /** Show when the window resets. */
    reset?: boolean;
  } = $props();
  let value = $derived(meter.value);
  let percent = $derived(
    value === null ? '—' : percentage(value, $language) + '%',
  );
  let left = $derived(
    meter.resetsAt !== null && meter.resetsAt > now
      ? duration(meter.resetsAt, now, $language)
      : null,
  );
  let resetText = $derived(
    meter.resetsAt === null
      ? t($language, 'resetUnknown')
      : left
        ? t($language, 'resetIn', { duration: left })
        : t($language, 'resetting'),
  );
</script>

<!-- Amber from the switch threshold, red at the provider's own limit. -->
<div
  class="usage-quota"
  class:compact
  class:near-limit={value !== null && value >= threshold && value < 100}
  class:over-limit={value !== null && value >= 100}
  data-tone={meter.tone}
>
  <div class="quota-label">
    <span>{meter.label}</span><strong>{percent}</strong>
  </div>
  <div
    class="quota-track"
    role="meter"
    aria-label={meter.label}
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={value === null
      ? undefined
      : Math.min(100, Math.max(0, value))}
    aria-valuetext={value === null
      ? t($language, 'dataUnavailable')
      : [
          t($language, 'usageMeter', { value: percentage(value, $language) }),
          meter.note,
        ]
          .filter(Boolean)
          .join(' · ')}
  >
    <span style:width={Math.min(100, Math.max(0, value ?? 0)) + '%'}></span>
  </div>
  <!-- The meter's value text already announces the note. -->
  {#if meter.note}<small class="quota-note" aria-hidden="true"
      >{meter.note}</small
    >{/if}
  {#if reset && compact}
    <!-- Narrow cells: a clock and the time left; assistive tech hears the whole phrase. -->
    {#if left && value !== null}<small class="quota-reset" title={resetText}
        ><span class="sr-only">{resetText}</span><span
          class="reset-short"
          aria-hidden="true"><Icon name="clock" size={11} />{left}</span
        ></small
      >{/if}
  {:else if reset}<small class="quota-reset">{resetText}</small>{/if}
</div>

<style>
  .usage-quota {
    min-width: 0;
    --quota-color: var(--accent);
  }
  .usage-quota[data-tone='green'] {
    --quota-color: var(--green);
  }
  .usage-quota[data-tone='violet'] {
    --quota-color: var(--violet);
  }
  .quota-label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 0.74rem;
    color: var(--subtle);
  }
  .quota-label strong {
    font-size: 1.5rem;
    line-height: 1.15;
    letter-spacing: -0.03em;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .quota-track {
    height: 7px;
    border-radius: 4px;
    background: var(--track);
    margin-top: 8px;
    overflow: hidden;
  }
  .quota-track span {
    height: 100%;
    display: block;
    background: var(--quota-color);
    border-radius: 4px;
    transition: width 0.2s;
  }
  .near-limit .quota-track span {
    background: var(--warn);
  }
  .over-limit .quota-track span {
    background: var(--danger);
  }
  .quota-reset,
  .quota-note {
    display: block;
    margin-top: 7px;
    font-size: 0.71rem;
    line-height: 1.35;
    font-variant-numeric: tabular-nums;
  }
  .quota-reset {
    color: var(--subtle);
  }
  .quota-note {
    font-weight: 600;
    color: var(--warn);
  }
  .compact .quota-label {
    flex-direction: row;
    justify-content: space-between;
    align-items: baseline;
    gap: 6px;
    font-size: 0.7rem;
  }
  .compact .quota-label strong {
    font-size: 0.8rem;
    font-weight: 600;
    letter-spacing: 0;
  }
  .compact .quota-track {
    height: 5px;
    margin-top: 6px;
  }
  .compact .quota-reset,
  .compact .quota-note {
    margin-top: 5px;
    font-size: 0.66rem;
  }
  .compact .quota-note + .quota-reset {
    margin-top: 2px;
  }
  .reset-short {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  @media (max-width: 560px) {
    .quota-label {
      font-size: 0.66rem;
    }
    .quota-label strong {
      font-size: 1.3rem;
    }
    .quota-reset,
    .quota-note {
      font-size: 0.64rem;
    }
  }
</style>
