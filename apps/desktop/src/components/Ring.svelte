<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { percentage, countdown } from '../lib/format';
  let {
    value = null,
    label,
    resetAt = null,
    now,
    large = false,
    threshold = 95,
  }: {
    value?: number | null;
    label: string;
    resetAt?: number | null;
    now: number;
    large?: boolean;
    threshold?: number;
  } = $props();
  const circumference = 2 * Math.PI * 43;
  let danger = $derived(value !== null && value >= threshold);
</script>

<div class:large class:danger class="quota">
  <div
    class="ring"
    role="meter"
    aria-label={label}
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={value === null ? undefined : Math.min(100, value)}
    aria-valuetext={value === null
      ? t($language, 'dataUnavailable')
      : t($language, 'usageMeter', { value: percentage(value, $language) })}
  >
    <svg viewBox="0 0 100 100" aria-hidden="true">
      <circle class="ring-track" cx="50" cy="50" r="43" />
      {#if value !== null}<circle
          class="ring-value"
          cx="50"
          cy="50"
          r="43"
          stroke-dasharray={circumference}
          stroke-dashoffset={circumference *
            (1 - Math.min(100, Math.max(0, value)) / 100)}
        />{/if}
    </svg>
    <span class="ring-number"
      >{value === null
        ? '—'
        : percentage(value, $language)}{#if value !== null}<small>%</small
        >{/if}</span
    >
  </div>
  <div class="quota-caption">
    <strong>{label}</strong><span class="countdown"
      >{countdown(resetAt, now, $language)}</span
    >
  </div>
</div>
