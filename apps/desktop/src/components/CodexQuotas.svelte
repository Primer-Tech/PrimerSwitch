<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { codexLimitName, codexWindowLabel } from '../lib/codex-format';
  import type { CodexAccount } from '../lib/codex-types';
  import QuotaBar from './QuotaBar.svelte';
  let { account, now }: { account: CodexAccount; now: number } = $props();
</script>

{#if account.authKind === 'chatgpt' && account.identityVerified && account.quota}
  <div class="codex-permission" role="status">
    <span
      class="status-dot"
      class:inactive={account.quota.ordinaryUsageAllowed !== true}
    ></span>{t(
      $language,
      account.quota.ordinaryUsageAllowed === true
        ? 'codexUsageAllowed'
        : account.quota.ordinaryUsageAllowed === false
          ? 'codexUsageBlocked'
          : 'codexUsageUnknown',
    )}
  </div>
  {#each account.quota.limits as limit (limit.key)}
    <section class="codex-limit" aria-label={codexLimitName(limit, $language)}>
      <div class="codex-limit-heading">
        <h3>{codexLimitName(limit, $language)}</h3>
        {#if limit.planType}<span class="badge muted">{limit.planType}</span
          >{/if}
      </div>
      {#if limit.primary || limit.secondary}<div class="codex-quota-grid">
          {#if limit.primary}<QuotaBar
              value={limit.primary.usedPercent}
              label={codexWindowLabel(
                limit.primary.windowDurationMins,
                false,
                $language,
              )}
              resetAt={limit.primary.resetsAt}
              {now}
              threshold={100}
            />{/if}
          {#if limit.secondary}<QuotaBar
              value={limit.secondary.usedPercent}
              label={codexWindowLabel(
                limit.secondary.windowDurationMins,
                true,
                $language,
              )}
              resetAt={limit.secondary.resetsAt}
              {now}
              threshold={100}
              tone="green"
            />{/if}
        </div>{:else}<p class="subtle">{t($language, 'codexNoWindows')}</p>{/if}
      {#if limit.spendControlReached === true}<p class="warning-text">
          {t($language, 'codexSpendBlocked')}
        </p>{/if}
      {#if limit.rateLimitReachedType}<p class="warning-text">
          {t($language, 'codexLimitReached')}
        </p>{/if}
      <div class="codex-credit-line">
        <span>{t($language, 'codexCredits')}</span><strong
          >{limit.credits === null
            ? t($language, 'unknown')
            : limit.credits.unlimited
              ? t($language, 'codexUnlimited')
              : (limit.credits.balance ??
                t(
                  $language,
                  limit.credits.hasCredits
                    ? 'codexCreditsAvailable'
                    : 'codexNoCredits',
                ))}</strong
        >
      </div>
    </section>
  {/each}
  {#if !account.quota.limits.length}<p class="subtle">
      {t($language, 'codexNoWindows')}
    </p>{/if}
{:else}<p class="codex-empty-quota">
    {t(
      $language,
      account.authKind === 'apiKey' ? 'codexApiQuota' : 'codexNoQuota',
    )}
  </p>{/if}
