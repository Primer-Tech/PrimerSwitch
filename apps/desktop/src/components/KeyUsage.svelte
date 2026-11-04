<script lang="ts">
  import { language, t, type MessageKey } from '../lib/i18n';
  import { compactCount, date } from '../lib/format';
  import type { CodexKeyUsage } from '../lib/codex-types';

  let { usage }: { usage: CodexKeyUsage | null | undefined } = $props();
  const windows = [
    ['last24h', 'keyUsage24h'],
    ['last7d', 'keyUsage7d'],
    ['last30d', 'keyUsage30d'],
    ['total', 'keyUsageTotal'],
  ] as const satisfies ReadonlyArray<
    readonly [keyof CodexKeyUsage['windows'], MessageKey]
  >;
  const counters = [
    ['input', 'keyUsageInput'],
    ['cached', 'keyUsageCached'],
    ['output', 'keyUsageOutput'],
    ['total', 'keyUsageTokensTotal'],
  ] as const satisfies ReadonlyArray<
    readonly [keyof CodexKeyUsage['windows']['total'], MessageKey]
  >;
  let hasUsage = $derived(
    !!usage && Object.values(usage.windows.total).some((count) => count > 0),
  );
</script>

<section class="consumption" aria-label={t($language, 'keyConsumption')}>
  <h3>{t($language, 'keyConsumption')}</h3>
  {#if usage && hasUsage}
    <div class="windows">
      {#each windows as [window, label] (window)}
        <div class="window">
          <h4>{t($language, label)}</h4>
          <dl>
            {#each counters as [counter, name] (counter)}
              <div class:total={counter === 'total'}>
                <dt>{t($language, name)}</dt>
                <dd>
                  {compactCount(usage.windows[window][counter], $language)}
                </dd>
              </div>
            {/each}
          </dl>
        </div>
      {/each}
    </div>
  {:else}
    <p class="empty">{t($language, 'keyUsageEmpty')}</p>
  {/if}
  {#if usage?.trackedSince != null}
    <p class="tracking">
      {t($language, 'keyUsageSince', {
        date: date(usage.trackedSince, $language),
      })}
    </p>
  {/if}
  {#if usage && usage.state !== 'idle'}
    <p class="state" role="status">
      {t(
        $language,
        usage.state === 'scanning' ? 'keyUsageScanning' : 'keyUsageUnavailable',
      )}
    </p>
  {/if}
  <p class="scope">
    {t($language, 'keyUsageLocal')}
    {t($language, 'keyUsageAuthority')}
  </p>
</section>

<style>
  .consumption {
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface);
  }
  h3 {
    margin: 0 0 10px;
    font-size: 0.78rem;
    font-weight: 600;
  }
  .windows {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 14px;
  }
  .window {
    min-width: 0;
  }
  h4 {
    margin: 0 0 7px;
    font-size: 0.72rem;
    font-weight: 600;
  }
  dl {
    margin: 0;
    font-size: 0.68rem;
    line-height: 1.5;
  }
  dl > div {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 6px;
    margin-top: 3px;
  }
  dt {
    color: var(--subtle);
  }
  dd {
    margin: 0;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .total {
    padding-top: 4px;
    border-top: 1px solid var(--line);
    font-weight: 600;
  }
  p {
    margin: 8px 0 0;
    color: var(--subtle);
    font-size: 0.68rem;
    line-height: 1.5;
  }
  .empty {
    font-size: 0.76rem;
  }
  @media (max-width: 700px) {
    .windows {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
