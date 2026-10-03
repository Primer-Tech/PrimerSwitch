<script lang="ts">
  import { language, t } from '../lib/i18n';
  import type { AccountRowView } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  import QuotaBar from './QuotaBar.svelte';
  import StatusLabel from './StatusLabel.svelte';
  type RowAction = (row: AccountRowView, trigger: HTMLElement) => void;
  let {
    rows,
    now,
    threshold,
    locked,
    lockReason,
    onswitch,
    onsignin,
    ondetails,
    onrefresh,
    ondelete,
  }: {
    rows: AccountRowView[];
    now: number;
    threshold: number;
    /** True while any mutation, sign-in, switch or demo lock is active. */
    locked: boolean;
    lockReason: string | null;
    onswitch: RowAction;
    onsignin: RowAction;
    ondetails: RowAction;
    onrefresh: RowAction;
    ondelete: RowAction;
  } = $props();
  const uid = $props.id();
  let menuId = $state<string | null>(null);
  let triggers: Record<string, HTMLButtonElement | null> = $state({});
  function closeMenu(focusTrigger: boolean) {
    const id = menuId;
    menuId = null;
    if (focusTrigger && id) triggers[id]?.focus();
  }
  function choose(row: AccountRowView, run: RowAction) {
    const trigger = triggers[row.id];
    closeMenu(true);
    if (trigger) run(row, trigger);
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
        ? event.target.closest('[data-row-menu]')
        : null;
    if (owner?.getAttribute('data-row-menu') !== menuId) closeMenu(false);
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
<div class="table-wrap">
  <table class="account-table">
    <thead
      ><tr
        ><th scope="col">{t($language, 'account')}</th><th scope="col"
          >{t($language, 'fiveHourWindow')}</th
        ><th scope="col">{t($language, 'weekly')}</th><th scope="col"
          >{t($language, 'status')}</th
        ><th scope="col"
          ><span class="sr-only">{t($language, 'actions')}</span></th
        ></tr
      ></thead
    ><tbody>
      {#each rows as row, index (row.id)}
        {@const name = `${uid}-name-${index}`}
        {@const status = `${uid}-status-${index}`}
        {@const menu = `${uid}-menu-${index}`}
        <tr class:current-row={row.active} class:switching-row={row.switching}
          ><td class="cell-account"
            ><div class="table-identity">
              <span class="avatar" aria-hidden="true">{row.initial}</span>
              <div>
                <strong class="account-name" id={name} title={row.name}
                  >{row.name}</strong
                >
                {#if row.plan || row.secondary}<span class="account-meta"
                    >{#if row.plan}<span class="badge muted">{row.plan}</span
                      >{/if}{#if row.secondary}<span
                        class="meta-text"
                        title={row.secondary}>{row.secondary}</span
                      >{/if}</span
                  >{/if}
              </div>
            </div></td
          ><td class="cell-meter"
            ><QuotaBar compact meter={row.fiveHour} {now} {threshold} /></td
          ><td class="cell-meter"
            ><QuotaBar
              compact
              meter={row.weeklyBinding}
              {now}
              {threshold}
            /></td
          ><td class="cell-status"
            ><StatusLabel status={row.status} id={status} /></td
          ><td class="cell-actions"
            ><div class="row-actions">
              {#if row.primary === 'signIn'}<button
                  class="switch-button sign-in-button"
                  disabled={locked || !row.canSignIn}
                  title={locked ? (lockReason ?? undefined) : undefined}
                  aria-describedby={name}
                  onclick={(event) => onsignin(row, event.currentTarget)}
                  >{t($language, 'signInAgain')}</button
                >{:else if row.primary === 'switch'}<span
                  class="switch-wrap"
                  title={row.switchReason ??
                    (locked ? (lockReason ?? undefined) : undefined)}
                  ><button
                    class="switch-button"
                    disabled={locked || !row.canSwitch}
                    aria-describedby={row.switchReason
                      ? `${name} ${status}`
                      : name}
                    onclick={(event) => onswitch(row, event.currentTarget)}
                    >{row.switching
                      ? t($language, 'switching')
                      : t($language, 'switch')}</button
                  ></span
                >{/if}
              <div
                class="row-menu"
                data-row-menu={row.id}
                onfocusout={(event) => focusout(event, row.id)}
              >
                <button
                  class="icon-button"
                  bind:this={triggers[row.id]}
                  aria-label={t($language, 'moreLabel', { name: row.name })}
                  aria-expanded={menuId === row.id}
                  aria-controls={menu}
                  onclick={() => (menuId = menuId === row.id ? null : row.id)}
                  ><Icon name="more" /></button
                >{#if menuId === row.id}<div class="menu" id={menu}>
                    <button onclick={() => choose(row, ondetails)}
                      >{t($language, 'viewDetails')}</button
                    ><button
                      disabled={locked || !row.canRefresh}
                      onclick={() => choose(row, onrefresh)}
                      >{t($language, 'refresh')}</button
                    ><button
                      class="danger"
                      disabled={locked || !row.canDelete}
                      title={row.deleteReason ?? undefined}
                      aria-describedby={row.deleteReason
                        ? `${menu}-hint`
                        : undefined}
                      onclick={() => choose(row, ondelete)}
                      >{t($language, 'delete')}</button
                    >{#if row.deleteReason}<p
                        class="menu-hint"
                        id={`${menu}-hint`}
                      >
                        {row.deleteReason}
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
  .table-wrap {
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--surface);
  }
  .account-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
    table-layout: fixed;
  }
  .account-table th {
    text-align: left;
    font-weight: 500;
    color: var(--subtle);
    font-size: 0.68rem;
    background: var(--surface-raised);
    padding: 8px 12px;
  }
  .account-table th:first-child {
    width: 33%;
    border-top-left-radius: 8px;
  }
  .account-table th:nth-child(2),
  .account-table th:nth-child(3) {
    width: 16%;
  }
  .account-table th:nth-child(4) {
    width: 19%;
  }
  .account-table th:last-child {
    width: 16%;
    border-top-right-radius: 8px;
  }
  .account-table td {
    border-top: 1px solid var(--line);
    padding: 8px 12px;
    vertical-align: middle;
  }
  .cell-meter {
    padding-right: 18px !important;
  }
  /* The column header names the window; the card layout below shows it again. */
  .cell-meter :global(.quota-name) {
    display: none;
  }
  .table-identity {
    display: flex;
    gap: 10px;
    align-items: center;
    min-width: 0;
  }
  .table-identity > div {
    flex: 1;
    min-width: 0;
  }
  .account-name {
    display: block;
    min-width: 0;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account-meta {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    margin-top: 4px;
    color: var(--subtle);
    font-size: 0.71rem;
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
  tr.current-row td:first-child {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  tr.switching-row td {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .row-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
  }
  .switch-wrap {
    display: inline-flex;
  }
  .switch-button {
    min-width: 68px;
    min-height: 30px;
    font-size: 0.74rem;
    padding: 5px 10px;
    white-space: nowrap;
  }
  .switch-button:not(:disabled) {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
  }
  .switch-button:not(:disabled):hover {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
    border-color: var(--accent);
  }
  .sign-in-button:not(:disabled) {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 45%, var(--line));
    background: color-mix(in srgb, var(--danger) 8%, var(--surface));
  }
  .row-menu {
    position: relative;
  }
  .row-menu .icon-button {
    width: 30px;
    min-width: 30px;
    min-height: 30px;
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
    padding: 8px 10px;
    min-height: 32px;
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
  @media (max-width: 1130px) {
    .account-table td,
    .account-table th {
      padding-left: 9px;
      padding-right: 9px;
    }
    .table-identity .avatar {
      display: none;
    }
  }
  @media (max-width: 720px) {
    .account-table,
    .account-table tbody {
      display: block;
      min-width: 0;
    }
    .account-table thead {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .account-table tr {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      grid-template-areas:
        'account account actions'
        'short long status';
      column-gap: 14px;
      row-gap: 10px;
      padding: 12px;
      border-top: 1px solid var(--line);
    }
    .account-table tbody tr:first-child {
      border-top: 0;
    }
    .account-table td {
      display: block;
      border: 0;
      padding: 0 !important;
      min-width: 0;
    }
    .cell-account {
      grid-area: account;
    }
    .cell-meter :global(.quota-name) {
      display: block;
    }
    .cell-meter:nth-child(2) {
      grid-area: short;
    }
    .cell-meter:nth-child(3) {
      grid-area: long;
    }
    .cell-status {
      grid-area: status;
    }
    .cell-actions {
      grid-area: actions;
      align-self: center;
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
