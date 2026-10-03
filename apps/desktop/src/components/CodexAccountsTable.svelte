<script lang="ts">
  import { language, t } from '../lib/i18n';
  import {
    codexAccountPlan,
    codexCanSwitch,
    codexInitial,
    codexMainLimit,
    codexMessage,
    codexSecondaryLine,
    codexWindowLabel,
    codexWindows,
  } from '../lib/codex-format';
  import type { CodexPending } from '../lib/codex-controller';
  import type { CodexAccount, CodexSnapshot } from '../lib/codex-types';
  import Icon from './Icon.svelte';
  import QuotaBar from './QuotaBar.svelte';
  import CodexStatus from './CodexStatus.svelte';
  let {
    snapshot,
    now,
    locked,
    pending,
    pendingId,
    onswitch,
    onsignin,
    ondetails,
    onrefresh,
    ondelete,
  }: {
    snapshot: CodexSnapshot;
    now: number;
    /** True while any mutation, sign-in, switch or demo lock is active. */
    locked: boolean;
    pending: CodexPending | null;
    pendingId: string | null;
    onswitch: (account: CodexAccount, trigger: HTMLElement) => void;
    onsignin: (account: CodexAccount, trigger: HTMLElement) => void;
    ondetails: (account: CodexAccount, trigger: HTMLElement) => void;
    onrefresh: (account: CodexAccount, trigger: HTMLElement) => void;
    ondelete: (account: CodexAccount, trigger: HTMLElement) => void;
  } = $props();
  let rows = $derived(
    snapshot.accounts.map((account) => ({
      account,
      windows: codexWindows(codexMainLimit(account)),
      plan: codexAccountPlan(account, $language),
      secondary: codexSecondaryLine(account),
    })),
  );
  // Column titles follow the real window durations; 5 hours and weekly are the usual ones.
  let shortTitle = $derived(
    codexWindowLabel(
      rows.find((row) => row.windows.short?.windowDurationMins)?.windows.short
        ?.windowDurationMins ?? 300,
      'short',
      $language,
    ),
  );
  let longTitle = $derived(
    codexWindowLabel(
      rows.find((row) => row.windows.long?.windowDurationMins)?.windows.long
        ?.windowDurationMins ?? 10080,
      'long',
      $language,
    ),
  );
  let menuId = $state<string | null>(null);
  let triggers: Record<string, HTMLButtonElement | null> = $state({});
  let switchingId = $derived(
    snapshot.switching?.targetId ??
      (pending === 'codex_switch_account' ? pendingId : null),
  );
  function checking(account: CodexAccount) {
    return (
      (pending === 'codex_refresh_account' && pendingId === account.id) ||
      (pending === 'codex_refresh_all' &&
        account.authKind === 'chatgpt' &&
        !account.needsSignIn)
    );
  }
  /** A per-account reason, only when switching is otherwise available. */
  function blockedReason(account: CodexAccount) {
    return snapshot.capabilities.switchAccount.enabled &&
      !account.switchable.enabled &&
      !account.selected &&
      !account.needsSignIn
      ? account.switchable.blockedReason
      : null;
  }
  function canRefresh(account: CodexAccount) {
    return (
      !locked &&
      snapshot.capabilities.refreshQuota.enabled &&
      account.authKind === 'chatgpt' &&
      !account.needsSignIn
    );
  }
  function closeMenu(focusTrigger: boolean) {
    const id = menuId;
    menuId = null;
    if (focusTrigger && id) triggers[id]?.focus();
  }
  function choose(
    account: CodexAccount,
    run: (account: CodexAccount, trigger: HTMLElement) => void,
  ) {
    const trigger = triggers[account.id];
    closeMenu(true);
    if (trigger) run(account, trigger);
  }
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && menuId) {
      event.preventDefault();
      closeMenu(true);
    }
  }
  function pointerdown(event: PointerEvent) {
    if (!menuId) return;
    const owner =
      event.target instanceof Element
        ? event.target.closest('[data-codex-menu]')
        : null;
    if (owner?.getAttribute('data-codex-menu') !== menuId) closeMenu(false);
  }
  function focusout(event: FocusEvent, id: string) {
    const next = event.relatedTarget;
    // Only a real focus move elsewhere closes the menu; clicks are handled on pointerdown.
    if (
      menuId === id &&
      next instanceof Node &&
      !(event.currentTarget as HTMLElement).contains(next)
    )
      closeMenu(false);
  }
</script>

<svelte:window onkeydown={keydown} onpointerdown={pointerdown} />
<div class="codex-table-wrap">
  <table class="account-table codex-table">
    <thead
      ><tr
        ><th scope="col">{t($language, 'codexColAccount')}</th><th scope="col"
          >{shortTitle}</th
        ><th scope="col">{longTitle}</th><th scope="col"
          >{t($language, 'codexColStatus')}</th
        ><th scope="col"
          ><span class="sr-only">{t($language, 'codexColActions')}</span></th
        ></tr
      ></thead
    ><tbody>
      {#each rows as row, index (row.account.id)}
        {@const account = row.account}
        {@const reason = blockedReason(account)}
        <tr
          class:current-row={account.selected}
          class:switching-row={switchingId === account.id}
          ><td class="cell-account"
            ><div class="table-identity">
              <span class="avatar" aria-hidden="true"
                >{codexInitial(account)}</span
              >
              <div>
                <strong class="account-name" title={account.name}
                  >{account.name}</strong
                >
                {#if row.plan || row.secondary}<span class="account-meta"
                    >{#if row.plan}<span class="badge muted">{row.plan}</span
                      >{/if}{#if row.secondary}<span class="meta-text"
                        >{row.secondary}</span
                      >{/if}</span
                  >{/if}
              </div>
            </div></td
          ><td class="cell-meter"
            >{#if row.windows.short}<QuotaBar
                compact
                label={codexWindowLabel(
                  row.windows.short.windowDurationMins,
                  'short',
                  $language,
                )}
                value={row.windows.short.usedPercent}
                {now}
                threshold={100}
              />{:else}<span class="no-reading" aria-hidden="true">—</span><span
                class="sr-only">{t($language, 'dataUnavailable')}</span
              >{/if}</td
          ><td class="cell-meter"
            >{#if row.windows.long}<QuotaBar
                compact
                label={codexWindowLabel(
                  row.windows.long.windowDurationMins,
                  'long',
                  $language,
                )}
                value={row.windows.long.usedPercent}
                {now}
                threshold={100}
                tone="green"
              />{:else}<span class="no-reading" aria-hidden="true">—</span><span
                class="sr-only">{t($language, 'dataUnavailable')}</span
              >{/if}</td
          ><td class="cell-status"
            ><CodexStatus
              {account}
              {now}
              checking={checking(account)}
              blockedReason={reason}
            /></td
          ><td class="cell-actions"
            ><div class="row-actions">
              {#if account.needsSignIn}<button
                  class="switch-button sign-in-button"
                  disabled={locked ||
                    !snapshot.capabilities.loginBrowser.enabled}
                  aria-label={t($language, 'codexSignInLabel', {
                    name: account.name,
                  })}
                  onclick={(event) => onsignin(account, event.currentTarget)}
                  >{t($language, 'codexSignIn')}</button
                >{:else if !account.selected}<button
                  class="switch-button"
                  disabled={locked || !codexCanSwitch(snapshot, account)}
                  aria-label={t($language, 'codexSwitchLabel', {
                    name: account.name,
                  })}
                  title={reason ? codexMessage(reason, $language) : undefined}
                  onclick={(event) => onswitch(account, event.currentTarget)}
                  >{switchingId === account.id
                    ? t($language, 'codexSwitchingShort')
                    : t($language, 'codexSwitch')}</button
                >{/if}
              <div
                class="row-menu"
                data-codex-menu={account.id}
                onfocusout={(event) => focusout(event, account.id)}
              >
                <button
                  class="icon-button"
                  bind:this={triggers[account.id]}
                  aria-label={t($language, 'codexMoreLabel', {
                    name: account.name,
                  })}
                  aria-expanded={menuId === account.id}
                  aria-controls={`codex-row-menu-${index}`}
                  onclick={() =>
                    (menuId = menuId === account.id ? null : account.id)}
                  ><Icon name="more" /></button
                >{#if menuId === account.id}<div
                    class="menu"
                    id={`codex-row-menu-${index}`}
                  >
                    <button onclick={() => choose(account, ondetails)}
                      >{t($language, 'codexMenuDetails')}</button
                    ><button
                      disabled={!canRefresh(account)}
                      onclick={() => choose(account, onrefresh)}
                      >{t($language, 'codexMenuRefresh')}</button
                    ><button
                      class="danger"
                      disabled={locked ||
                        account.selected ||
                        !snapshot.capabilities.deleteSaved.enabled}
                      title={account.selected
                        ? codexMessage('activeAccount', $language)
                        : undefined}
                      aria-describedby={account.selected
                        ? `codex-row-menu-${index}-hint`
                        : undefined}
                      onclick={() => choose(account, ondelete)}
                      >{t($language, 'codexMenuDelete')}</button
                    >{#if account.selected}<p
                        class="menu-hint"
                        id={`codex-row-menu-${index}-hint`}
                      >
                        {codexMessage('activeAccount', $language)}
                      </p>{/if}
                  </div>{/if}
              </div>
            </div></td
          ></tr
        >
      {/each}
    </tbody>
  </table>
</div>

<style>
  .codex-table-wrap {
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--surface);
  }
  .codex-table th:first-child {
    width: 34%;
    border-top-left-radius: 8px;
  }
  .codex-table th:nth-child(2),
  .codex-table th:nth-child(3) {
    width: 15%;
  }
  .codex-table th:nth-child(4) {
    width: 19%;
  }
  .codex-table th:last-child {
    width: 17%;
    border-top-right-radius: 8px;
  }
  .codex-table td {
    vertical-align: middle;
  }
  .codex-table .cell-meter :global(.quota-label > span) {
    display: none;
  }
  .codex-table .cell-meter {
    padding-right: 18px;
  }
  .account-name {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account-meta {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    margin-top: 5px;
    color: var(--subtle);
    font-size: 0.72rem;
  }
  .account-meta .badge {
    flex: none;
  }
  .meta-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .table-identity > div {
    flex: 1;
  }
  .no-reading {
    color: var(--subtle);
  }
  tr.current-row td:first-child {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  tr.switching-row td {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .row-actions {
    gap: 4px;
  }
  .row-actions .switch-button {
    min-width: 72px;
  }
  .row-actions .switch-button:not(:disabled) {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
  }
  .row-actions .switch-button:not(:disabled):hover {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
    border-color: var(--accent);
  }
  .row-actions .sign-in-button:not(:disabled) {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 45%, var(--line));
    background: color-mix(in srgb, var(--danger) 8%, var(--surface));
  }
  .row-menu {
    position: relative;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 5;
    min-width: 180px;
    padding: 5px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface-raised);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
  }
  .menu button {
    justify-content: flex-start;
    border: 0;
    background: none;
    font-weight: 500;
    padding: 9px 10px;
    min-height: 34px;
  }
  .menu button:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  .menu button.danger {
    color: var(--danger);
  }
  .menu-hint {
    max-width: 210px;
    margin: 0;
    padding: 2px 10px 7px;
    color: var(--subtle);
    font-size: 0.7rem;
    line-height: 1.45;
  }
  @media (max-width: 720px) {
    .codex-table,
    .codex-table tbody {
      display: block;
      min-width: 0;
    }
    .codex-table thead {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .codex-table tr {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      grid-template-areas:
        'account account actions'
        'short long status';
      column-gap: 14px;
      row-gap: 12px;
      padding: 14px;
      border-top: 1px solid var(--line);
    }
    .codex-table tbody tr:first-child {
      border-top: 0;
    }
    .codex-table td {
      display: block;
      border: 0;
      padding: 0;
      min-width: 0;
    }
    .codex-table .cell-account {
      grid-area: account;
    }
    .codex-table .cell-meter:nth-child(2) {
      grid-area: short;
    }
    .codex-table .cell-meter:nth-child(3) {
      grid-area: long;
    }
    .codex-table .cell-status {
      grid-area: status;
    }
    .codex-table .cell-actions {
      grid-area: actions;
      align-self: center;
    }
    .codex-table .cell-meter :global(.quota-label > span) {
      display: block;
    }
    tr.current-row td:first-child {
      box-shadow: none;
    }
    tr.current-row {
      box-shadow: inset 3px 0 0 var(--accent);
    }
    .table-identity .avatar {
      display: flex;
    }
  }
</style>
