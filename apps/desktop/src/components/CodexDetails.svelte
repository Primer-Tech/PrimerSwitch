<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { age } from '../lib/format';
  import {
    codexAccountPlan,
    codexCanSwitch,
    codexInitial,
    codexMessage,
  } from '../lib/codex-format';
  import type { CodexAccount, CodexSnapshot } from '../lib/codex-types';
  import CodexIcon from './CodexIcon.svelte';
  import CodexQuotas from './CodexQuotas.svelte';
  import CodexStatus from './CodexStatus.svelte';
  let {
    account,
    snapshot,
    now,
    locked,
    checking,
    onswitch,
    onsignin,
    onrefresh,
    ondelete,
  }: {
    account: CodexAccount;
    snapshot: CodexSnapshot;
    now: number;
    locked: boolean;
    checking: boolean;
    onswitch: (trigger: HTMLElement) => void;
    onsignin: (trigger: HTMLElement) => void;
    onrefresh: () => void;
    ondelete: () => void;
  } = $props();
  let plan = $derived(codexAccountPlan(account, $language));
  let blocked = $derived(
    snapshot.capabilities.switchAccount.enabled &&
      !account.switchable.enabled &&
      !account.selected &&
      !account.needsSignIn
      ? account.switchable.blockedReason
      : null,
  );
</script>

<div class="codex-details">
  <div class="summary">
    <span class="avatar large-avatar" aria-hidden="true"
      >{codexInitial(account)}</span
    >
    <div>
      {#if account.email && account.email !== account.name}<p class="email">
          {account.email}
        </p>{/if}
      <CodexStatus {account} {now} {checking} />
    </div>
  </div>
  <CodexQuotas {account} {now} />
  <dl class="facts">
    <div>
      <dt>{t($language, 'codexDetailsSignIn')}</dt>
      <dd>
        {t(
          $language,
          account.authKind === 'chatgpt'
            ? 'codexAuthChatGPT'
            : account.authKind === 'apiKey'
              ? 'codexAuthApiKey'
              : 'codexAuthUnsupported',
        )}
      </dd>
    </div>
    {#if plan}<div>
        <dt>{t($language, 'codexDetailsPlan')}</dt>
        <dd>{plan}</dd>
      </div>{/if}
    {#if account.workspaceName}<div>
        <dt>{t($language, 'codexDetailsWorkspace')}</dt>
        <dd>{account.workspaceName}</dd>
      </div>{/if}
    <div>
      <dt>{t($language, 'codexDetailsChecked')}</dt>
      <dd>
        {account.quotaReadAt === null
          ? t($language, 'codexStatusUnread')
          : age(account.quotaReadAt, now, $language)}
      </dd>
    </div>
  </dl>
  {#if account.error && !account.needsSignIn}<p class="warning-text">
      {codexMessage(account.error, $language)}
    </p>{/if}
  {#if account.needsSignIn}<p class="warning-text">
      {codexMessage('signInRequired', $language)}
    </p>{:else if blocked}<p class="help">
      {codexMessage(blocked, $language)}
    </p>{/if}
  {#if account.selected}<p class="help" id="codex-delete-hint">
      {codexMessage('activeAccount', $language)}
    </p>{/if}
  <div class="actions">
    <button
      class="delete"
      disabled={locked ||
        account.selected ||
        !snapshot.capabilities.deleteSaved.enabled}
      title={account.selected
        ? codexMessage('activeAccount', $language)
        : undefined}
      aria-describedby={account.selected ? 'codex-delete-hint' : undefined}
      onclick={ondelete}
      ><CodexIcon name="trash" size={15} />{t(
        $language,
        'codexDetailsDelete',
      )}</button
    >
    <div class="primary-actions">
      {#if account.authKind === 'chatgpt' && !account.needsSignIn}<button
          disabled={locked || !snapshot.capabilities.refreshQuota.enabled}
          aria-label={t($language, 'codexRefreshLabel', { name: account.name })}
          onclick={onrefresh}>{t($language, 'codexRefresh')}</button
        >{/if}
      {#if account.needsSignIn}<button
          class="primary"
          disabled={locked || !snapshot.capabilities.loginBrowser.enabled}
          onclick={(event) => onsignin(event.currentTarget)}
          >{t($language, 'codexStatusSignIn')}</button
        >{:else if !account.selected}<button
          class="primary"
          disabled={locked || !codexCanSwitch(snapshot, account)}
          title={blocked ? codexMessage(blocked, $language) : undefined}
          onclick={(event) => onswitch(event.currentTarget)}
          >{t($language, 'codexSwitch')}</button
        >{/if}
    </div>
  </div>
</div>

<style>
  .codex-details {
    min-width: 0;
    margin-top: 16px;
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }
  .summary > div {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .email {
    margin: 0;
    color: var(--subtle);
    font-size: 0.78rem;
    overflow-wrap: anywhere;
  }
  .facts {
    margin: 16px 0 0;
    font-size: 0.78rem;
  }
  .facts > div {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 9px 0;
    border-bottom: 1px solid var(--line);
  }
  .facts dt {
    color: var(--subtle);
  }
  .facts dd {
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
  }
  .codex-details > p {
    margin-top: 12px;
  }
  .codex-details :global(.quota-label strong) {
    font-size: 1.45rem;
  }
  .actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 22px;
  }
  .primary-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  button.delete {
    color: var(--danger);
    background: transparent;
    border-color: color-mix(in srgb, var(--danger) 40%, var(--line));
  }
  button.delete:hover:not(:disabled) {
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    border-color: var(--danger);
  }
</style>
