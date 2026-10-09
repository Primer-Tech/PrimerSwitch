<script lang="ts">
  import { language, t } from '../lib/i18n';
  import {
    codexMessage,
    codexResetOutcome,
    codexResetExpiryDate,
  } from '../lib/codex-format';
  import type { CodexAccount } from '../lib/codex-types';
  let {
    account,
    locked,
    working = false,
    onreset,
    onrefresh,
  }: {
    account: CodexAccount;
    locked: boolean;
    working?: boolean;
    onreset: () => void;
    onrefresh: () => void;
  } = $props();
  const uid = $props.id();
  let count = $derived(account.quota?.resetCreditsAvailable ?? null);
  let reason = $derived(account.reset.usable.blockedReason);
  let details = $derived(account.quota?.resetCreditDetails ?? []);
  let expiries = $derived.by(() => {
    const groups = new Map<number | null, number>();
    for (const credit of details)
      groups.set(credit.expiresAt, (groups.get(credit.expiresAt) ?? 0) + 1);
    return [...groups]
      .sort(([a], [b]) => (a ?? Infinity) - (b ?? Infinity))
      .map(([expiresAt, count]) => ({ expiresAt, count }));
  });
</script>

<div class="reset-controls">
  <p class="reset-count">
    {count === null
      ? t($language, 'codexResetUnknown')
      : t($language, 'codexResetCount', { count })}
  </p>
  {#if account.quotaState !== 'fresh' && count !== null}<p class="help">
      {t($language, 'codexResetCached')}
    </p>{/if}
  {#if count !== null && count > 0}
    {#if expiries.length}
      <ul
        class="reset-expiries"
        aria-label={t($language, 'codexResetExpiryDates')}
      >
        {#each expiries as expiry (expiry.expiresAt)}
          <li>
            <span class="reset-quantity"
              >{t(
                $language,
                expiry.count === 1 ? 'codexResetOne' : 'codexResetMany',
                { count: expiry.count },
              )}</span
            >
            <span>
              {#if expiry.expiresAt !== null}
                {t($language, 'codexResetExpires')}
                <time datetime={new Date(expiry.expiresAt * 1000).toISOString()}
                  >{codexResetExpiryDate(expiry.expiresAt, $language)}</time
                >
              {:else}{t($language, 'codexResetNoExpiry')}{/if}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
    {#if details.length < count}
      <p class="help">
        {details.length
          ? t($language, 'codexResetExpiryPartial', {
              shown: details.length,
              count,
            })
          : t($language, 'codexResetExpiryUnknown')}
      </p>
    {/if}
  {/if}
  <p class="help" id={`${uid}-help`}>
    {account.reset.pending
      ? t($language, 'codexResetPending')
      : reason
        ? codexMessage(reason, $language)
        : t($language, 'codexResetHelp')}
  </p>
  {#if account.reset.lastOutcome}<p class="help" role="status">
      {codexResetOutcome(account.reset.lastOutcome, $language)}
    </p>{/if}
  <div class="reset-actions">
    <button
      disabled={locked || account.authKind !== 'chatgpt' || account.needsSignIn}
      onclick={onrefresh}>{t($language, 'codexRefreshResets')}</button
    >
    <button
      class="primary"
      disabled={locked || !account.reset.usable.enabled}
      aria-describedby={`${uid}-help`}
      onclick={onreset}
      >{t(
        $language,
        working
          ? 'codexUsingReset'
          : account.reset.pending
            ? 'codexRetryReset'
            : 'codexUseReset',
      )}</button
    >
  </div>
</div>

<style>
  .reset-count {
    font-size: 0.9rem;
    font-weight: 600;
    margin: 10px 0;
  }
  .reset-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
  }
  .reset-actions button {
    font-size: 0.75rem;
  }
  .reset-expiries {
    display: grid;
    gap: 8px;
    list-style: none;
    margin: 12px 0;
    padding: 0;
    font-size: 0.75rem;
    line-height: 1.6;
  }
  .reset-expiries li {
    display: grid;
    gap: 2px;
  }
  .reset-quantity {
    font-weight: 600;
  }
</style>
