<script lang="ts">
  import { language, t, subscriptionLabel } from '../lib/i18n';
  import Ring from './Ring.svelte';
  import { age, date, countdown, percentage } from '../lib/format';
  import { codeMessage, safeError } from '../lib/controller';
  import type { AccountView } from '../lib/types';
  let {
    account,
    now,
    threshold,
    model,
    disabled,
    pending,
    onrefresh,
    onswitch,
    ondelete,
    onrenewal,
  }: {
    account: AccountView;
    now: number;
    threshold: number;
    model: string | null;
    disabled: boolean;
    pending: string | null;
    onrefresh: () => void;
    onswitch: () => void;
    ondelete: () => void;
    onrenewal: (day: number | null) => void;
  } = $props();
  let stale = $derived(account.usageAt !== null && now - account.usageAt > 900);
  let rows = $derived(
    [...(account.usage?.scopedLimits ?? [])].sort(
      (a, b) => b.percent - a.percent,
    ),
  );
</script>

<article
  class:active={account.active}
  class="account-card"
  aria-label={t($language, 'accountLabel', { name: account.name })}
>
  <div class="card-top">
    <div class="avatar" aria-hidden="true">
      {(account.name || account.email || 'C').slice(0, 1).toUpperCase()}
    </div>
    <div class="identity">
      <h3>{account.name}</h3>
      <p>{account.email || t($language, 'emailUnavailable')}</p>
    </div>
    <button
      class="icon-button delete-button"
      aria-label={t($language, 'deleteLabel', { name: account.name })}
      {disabled}
      onclick={ondelete}>×</button
    >
  </div>
  <div class="badges">
    {#if account.active}<span class="badge accent"
        >{t($language, 'active')}</span
      >{:else if account.isNext}<span class="badge accent"
        >{t($language, 'next')}</span
      >{/if}
    {#if account.exhausted}<span class="badge danger-badge"
        >{t($language, 'exhausted')}</span
      >{/if}
    {#if !account.identityVerified}<span class="badge muted"
        >{t($language, 'identityUnverified')}</span
      >{/if}
    {#if account.planTier}<span class="badge muted">{account.planTier}</span
      >{/if}
  </div>
  <div class="card-rings">
    <Ring
      label={t($language, 'fiveHours')}
      value={account.usage?.fiveHour.utilization ?? null}
      resetAt={account.usage?.fiveHour.resetsAt ?? null}
      {now}
      {threshold}
    />
    <Ring
      label={t($language, 'weekly')}
      value={account.usage?.weeklyModel ?? null}
      resetAt={account.usage?.weeklyModelResetsAt ?? null}
      {now}
      {threshold}
    />
  </div>
  {#if account.usage && (rows.length || account.usage.weeklyOverall !== account.usage.weeklyModel)}
    <div class="scoped-limits">
      <div class="scoped-row">
        <span>{t($language, 'allModels')}</span><strong
          >{percentage(account.usage.weeklyOverall, $language)}%</strong
        >
      </div>
      {#each rows as limit}<div class="scoped-row">
          <span
            >{limit.label}{#if model && (model
                .toLowerCase()
                .includes(limit.label.toLowerCase()) || limit.label
                  .toLowerCase()
                  .includes(model.toLowerCase()))}<span class="model-marker">
                • {t($language, 'selectedModel')}</span
              >{/if}<small>{countdown(limit.resetsAt, now, $language)}</small
            ></span
          ><strong>{percentage(limit.percent, $language)}%</strong>
        </div>{/each}
      {#if account.scopedAt !== undefined && account.scopedAt !== null && now - account.scopedAt > 1800}<small
          class="warning-text">{t($language, 'scopedStale')}</small
        >{/if}
    </div>
  {/if}
  <div class="account-details">
    <div>
      <span>{t($language, 'subscription')}</span><strong
        >{subscriptionLabel(account.subscriptionStatus, $language)}</strong
      >
    </div>
    <div class="renewal">
      <label for={`renewal-${account.id}`}
        >{t($language, 'estimatedRenewal')}</label
      ><select
        id={`renewal-${account.id}`}
        aria-label={t($language, 'renewalLabel', { name: account.name })}
        value={account.renewalDay ?? ''}
        {disabled}
        onchange={(event) => {
          const day =
            event.currentTarget.value === ''
              ? null
              : Number(event.currentTarget.value);
          event.currentTarget.value = String(account.renewalDay ?? '');
          onrenewal(day);
        }}
        ><option value="">{t($language, 'unknown')}</option
        >{#each Array.from({ length: 31 }, (_, i) => i + 1) as day}<option
            value={day}>{t($language, 'renewalDay', { day })}</option
          >{/each}</select
      >
    </div>
    {#if account.nextRenewalAt !== null}<p class="detail-note">
        {t($language, 'manualEstimate', {
          date: date(account.nextRenewalAt, $language),
        })}
      </p>{/if}
    <div>
      <span>{t($language, 'availableResets')}</span><strong
        >{account.resets.available ?? t($language, 'unknown')}</strong
      >
    </div>
    {#if account.resets.expiresAt !== null}<p class="detail-note">
        {t($language, 'expiresIn', {
          duration: countdown(account.resets.expiresAt, now, $language),
        })}
      </p>{/if}
    {#if account.resets.cooldownUntil !== null && account.resets.cooldownUntil > now}<p
        class="detail-note"
      >
        {t($language, 'resetCooldown', {
          duration: countdown(account.resets.cooldownUntil, now, $language),
        })}
      </p>{/if}
    {#if account.resets.pending}<p class="warning-text">
        {t($language, 'checkingReset')}
      </p>{/if}
    {#if account.resets.lastOutcome}<p class="detail-note">
        {safeError(account.resets.lastOutcome, $language)}
      </p>{/if}
    {#if account.primedAt !== null}<p class="detail-note">
        {t($language, 'primedDate', {
          date: date(account.primedAt, $language),
        })}
      </p>{/if}
  </div>
  <div class="reading-state">
    <span class:warning-text={stale || !!account.error}
      >{!account.usage
        ? t($language, 'noUsage')
        : stale
          ? t($language, 'cachedAge', {
              age: age(account.usageAt, now, $language),
            })
          : t($language, 'readAge', {
              age: age(account.usageAt, now, $language),
            })}</span
    >
    {#if account.decisionFresh === false && account.active}<small
        >{t($language, 'automationWaiting')}</small
      >{/if}
    {#if account.error}<small class="warning-text"
        >{codeMessage(account.error, $language)}{account.usage
          ? ' ' + t($language, 'lastReadingKept')
          : ''}</small
      >{/if}
  </div>
  <div class="card-actions">
    <button
      {disabled}
      onclick={onrefresh}
      aria-label={t($language, 'refreshLabel', { name: account.name })}
      >↻ {t($language, 'refresh')}</button
    >
    {#if !account.active}<button
        class="switch-button"
        disabled={disabled || !account.identityVerified}
        onclick={onswitch}
        >{pending === `switch:${account.id}`
          ? t($language, 'switching')
          : t($language, 'switch')}</button
      >{:else}<span class="active-caption"
        >{t($language, 'currentAccount')}</span
      >{/if}
  </div>
</article>
