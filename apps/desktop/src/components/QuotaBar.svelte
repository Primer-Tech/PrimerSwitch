<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { percentage, countdown } from '../lib/format';
  let {
    value,
    label,
    resetAt = null,
    now,
    threshold = 95,
    tone = 'blue',
    compact = false,
    description,
  }: {
    value: number | null;
    label: string;
    resetAt?: number | null;
    now: number;
    threshold?: number;
    tone?: string;
    compact?: boolean;
    description?: string;
  } = $props();
  const descriptionId = $props.id();
  let percent = $derived(
    value === null ? '—' : percentage(value, $language) + '%',
  );
</script>

<div
  class="usage-quota"
  class:compact
  class:over-limit={value !== null && value >= threshold}
  data-tone={tone}
>
  <div class="quota-label"><span>{label}</span><strong>{percent}</strong></div>
  <div
    class="quota-track"
    role="meter"
    aria-label={label}
    aria-describedby={description ? descriptionId : undefined}
    title={description}
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={value === null
      ? undefined
      : Math.min(100, Math.max(0, value))}
    aria-valuetext={value === null
      ? t($language, 'dataUnavailable')
      : t($language, 'usageMeter', { value: percentage(value, $language) })}
  >
    <span style:width={Math.min(100, Math.max(0, value ?? 0)) + '%'}></span>
  </div>
  {#if description}<span class="sr-only" id={descriptionId}>{description}</span
    >{/if}
  {#if !compact}<small class="quota-reset"
      >{resetAt !== null && resetAt > now
        ? t($language, 'resetIn', {
            duration: countdown(resetAt, now, $language),
          })
        : countdown(resetAt, now, $language)}</small
    >{/if}
</div>
