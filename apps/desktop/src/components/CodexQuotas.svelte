<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { percentage } from '../lib/format';
  import {
    codexCredits,
    codexDuration,
    codexExtraLimits,
    codexLimitName,
    codexMainLimit,
    codexWindowLabel,
    codexWindows,
  } from '../lib/codex-format';
  import type { CodexAccount, CodexWindow } from '../lib/codex-types';
  import QuotaBar from './QuotaBar.svelte';
  let {
    account,
    now,
    showCredits = true,
  }: {
    account: CodexAccount;
    now: number;
    /** The active card shows credits in its caption instead. */
    showCredits?: boolean;
  } = $props();
  let main = $derived(codexMainLimit(account));
  let windows = $derived(codexWindows(main));
  let extras = $derived(
    codexExtraLimits(account).flatMap((limit) => {
      const slots = codexWindows(limit);
      return (
        [
          ['short', slots.short],
          ['long', slots.long],
        ] as const
      )
        .filter((entry): entry is ['short' | 'long', CodexWindow] => !!entry[1])
        .map(([slot, window], index) => ({
          key: `${limit.key}:${slot}`,
          name: codexLimitName(limit, $language),
          first: index === 0,
          window,
          label: codexWindowLabel(window.windowDurationMins, slot, $language),
        }));
    }),
  );
  let credits = $derived(codexCredits(account, $language));
  const clamp = (value: number) => Math.min(100, Math.max(0, value));
</script>

{#if account.authKind === 'apiKey'}
  <p class="codex-quota-empty">{t($language, 'codexApiQuota')}</p>
{:else if !account.quota}
  <p class="codex-quota-empty">{t($language, 'codexNoQuota')}</p>
{:else}
  <div class="codex-meters">
    {#if windows.short || windows.long}<div class="codex-main-windows">
        {#if windows.short}<QuotaBar
            value={windows.short.usedPercent}
            label={codexWindowLabel(
              windows.short.windowDurationMins,
              'short',
              $language,
            )}
            resetAt={windows.short.resetsAt}
            {now}
            threshold={100}
          />{/if}
        {#if windows.long}<QuotaBar
            value={windows.long.usedPercent}
            label={codexWindowLabel(
              windows.long.windowDurationMins,
              'long',
              $language,
            )}
            resetAt={windows.long.resetsAt}
            {now}
            threshold={100}
            tone="green"
          />{/if}
      </div>{:else}<p class="codex-quota-note">
        {t($language, 'codexNoWindows')}
      </p>{/if}
    {#if extras.length}<ul class="codex-extra-limits">
        {#each extras as extra (extra.key)}<li>
            <span class="extra-name"
              >{#if extra.first}{extra.name}{/if}<small>{extra.label}</small
              ></span
            ><span
              class="extra-track"
              class:full={extra.window.usedPercent >= 100}
              role="meter"
              aria-label={`${extra.name} · ${extra.label}`}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={clamp(extra.window.usedPercent)}
              aria-valuetext={t($language, 'usageMeter', {
                value: percentage(extra.window.usedPercent, $language),
              })}
              ><span style:width={clamp(extra.window.usedPercent) + '%'}
              ></span></span
            ><strong>{percentage(extra.window.usedPercent, $language)}%</strong
            ><small class="extra-reset"
              >{#if extra.window.resetsAt !== null && extra.window.resetsAt > now}{t(
                  $language,
                  'codexResetsIn',
                  {
                    duration: codexDuration(
                      extra.window.resetsAt - now,
                      $language,
                    ),
                  },
                )}{/if}</small
            >
          </li>{/each}
      </ul>{/if}
    {#if credits && showCredits}<div class="codex-credits">
        <span>{t($language, 'codexCredits')}</span><strong>{credits}</strong>
      </div>{/if}
  </div>
  {#if account.quota.ordinaryUsageAllowed === false}<p
      class="warning-text codex-quota-warning"
    >
      {t($language, 'codexUsageBlocked')}
    </p>{/if}
  {#if main?.spendControlReached === true}<p
      class="warning-text codex-quota-warning"
    >
      {t($language, 'codexSpendBlocked')}
    </p>{/if}
{/if}

<style>
  .codex-meters {
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 17px;
  }
  .codex-main-windows {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 18px;
  }
  .codex-main-windows > :global(.usage-quota + .usage-quota) {
    border-left: 1px solid var(--line);
    padding-left: 18px;
  }
  .codex-main-windows :global(.quota-label) {
    min-height: 0;
  }
  .codex-quota-empty,
  .codex-quota-note {
    color: var(--subtle);
    font-size: 0.8rem;
    line-height: 1.6;
  }
  .codex-quota-empty {
    padding: 18px;
    border: 1px dashed var(--line);
    border-radius: 8px;
  }
  .codex-extra-limits {
    list-style: none;
    margin: 15px 0 0;
    padding: 12px 0 0;
    border-top: 1px solid var(--line);
    display: grid;
    grid-template-columns:
      minmax(0, max-content) minmax(70px, 1fr)
      max-content max-content;
    align-items: center;
    column-gap: 14px;
    row-gap: 9px;
  }
  .codex-extra-limits li {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    align-items: center;
    font-size: 0.75rem;
  }
  .extra-name {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .extra-name small {
    color: var(--subtle);
    font-weight: 400;
    font-size: 0.7rem;
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
  .extra-track.full span {
    background: var(--danger);
  }
  .codex-extra-limits strong {
    font-weight: 600;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .extra-reset {
    color: var(--subtle);
    font-size: 0.7rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .codex-credits {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-top: 14px;
    padding-top: 11px;
    border-top: 1px solid var(--line);
    font-size: 0.75rem;
    color: var(--subtle);
  }
  .codex-credits strong {
    color: var(--text);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .codex-quota-warning {
    margin-top: 10px;
  }
  @media (max-width: 560px) {
    .codex-meters {
      padding: 13px 11px;
    }
    .codex-main-windows {
      gap: 10px;
    }
    .codex-main-windows > :global(.usage-quota + .usage-quota) {
      padding-left: 10px;
    }
    .codex-extra-limits {
      grid-template-columns:
        minmax(0, max-content) minmax(48px, 1fr)
        max-content;
      column-gap: 10px;
    }
    .extra-reset {
      display: none;
    }
  }
</style>
