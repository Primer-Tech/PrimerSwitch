<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { duration, percentage } from '../lib/format';
  import type { AccountRowView } from '../lib/provider-view';
  import QuotaBar from './QuotaBar.svelte';
  let {
    row,
    now,
    threshold,
  }: {
    row: AccountRowView;
    now: number;
    threshold: number;
  } = $props();
  const clamp = (value: number) => Math.min(100, Math.max(0, value));
</script>

<div class="usage-meters">
  <div class="primary-meters">
    <QuotaBar meter={row.fiveHour} {now} {threshold} />
    <QuotaBar meter={row.weekly} {now} {threshold} />
  </div>
  {#if row.extras.length}<ul class="extra-meters">
      {#each row.extras as extra (extra.key)}<li>
          <span class="extra-name">{extra.label}</span><span
            class="extra-track"
            class:near={extra.value !== null &&
              extra.value >= threshold &&
              extra.value < 100}
            class:full={extra.value !== null && extra.value >= 100}
            role="meter"
            aria-label={extra.label}
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={extra.value === null
              ? undefined
              : clamp(extra.value)}
            aria-valuetext={extra.value === null
              ? t($language, 'dataUnavailable')
              : t($language, 'usageMeter', {
                  value: percentage(extra.value, $language),
                })}
            ><span style:width={clamp(extra.value ?? 0) + '%'}></span></span
          ><strong
            >{extra.value === null
              ? '—'
              : percentage(extra.value, $language) + '%'}</strong
          ><small class="extra-reset"
            >{#if extra.resetsAt !== null && extra.resetsAt > now}{t(
                $language,
                'resetIn',
                { duration: duration(extra.resetsAt, now, $language) },
              )}{/if}</small
          >
        </li>{/each}
    </ul>{/if}
</div>

<style>
  .usage-meters {
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 12px 15px;
  }
  .primary-meters {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 16px;
  }
  .primary-meters > :global(.usage-quota + .usage-quota) {
    border-left: 1px solid var(--line);
    padding-left: 16px;
  }
  .extra-meters {
    list-style: none;
    margin: 10px 0 0;
    padding: 8px 0 0;
    border-top: 1px solid var(--line);
    display: grid;
    grid-template-columns:
      minmax(0, max-content) minmax(60px, 1fr)
      max-content max-content;
    align-items: center;
    column-gap: 12px;
    row-gap: 8px;
  }
  .extra-meters li {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    align-items: center;
    font-size: 0.74rem;
  }
  .extra-name {
    min-width: 0;
    color: var(--subtle);
    overflow-wrap: anywhere;
  }
  .extra-track {
    display: block;
    height: 5px;
    border-radius: 4px;
    background: var(--track);
    overflow: hidden;
  }
  .extra-track span {
    display: block;
    height: 100%;
    border-radius: 4px;
    background: var(--violet);
  }
  .extra-track.near span {
    background: var(--warn);
  }
  .extra-track.full span {
    background: var(--danger);
  }
  .extra-meters strong {
    font-weight: 600;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .extra-reset {
    color: var(--subtle);
    font-size: 0.68rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  @media (max-width: 560px) {
    .usage-meters {
      padding: 12px 10px;
    }
    .primary-meters {
      gap: 10px;
    }
    .primary-meters > :global(.usage-quota + .usage-quota) {
      padding-left: 10px;
    }
    .extra-meters {
      grid-template-columns:
        minmax(0, max-content) minmax(40px, 1fr)
        max-content;
      column-gap: 9px;
    }
    .extra-reset {
      display: none;
    }
  }
</style>
