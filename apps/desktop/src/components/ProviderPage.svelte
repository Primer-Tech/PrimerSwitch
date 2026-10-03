<script lang="ts">
  import type { Snippet } from 'svelte';
  import { language, t } from '../lib/i18n';
  import type {
    AccountRowView,
    AddOption,
    AutomationItem,
    NoticeView,
    PageView,
    ProviderKey,
  } from '../lib/provider-view';
  import AppShell from './AppShell.svelte';
  import ActiveAccountCard from './ActiveAccountCard.svelte';
  import AccountsTable from './AccountsTable.svelte';
  import AddMenu from './AddMenu.svelte';
  import UsageOrder from './UsageOrder.svelte';
  import NextUpCard from './NextUpCard.svelte';
  import AutomationCard from './AutomationCard.svelte';
  import Icon from './Icon.svelte';
  type RowAction = (row: AccountRowView, trigger: HTMLElement) => void;
  let {
    view,
    now,
    subtitle,
    tabsDisabled,
    onprovider,
    settingsDisabled,
    onsettings,
    notices,
    refreshAll,
    locked,
    lockReason,
    refreshingActive = false,
    activeEmpty,
    addLabel,
    addDisabled,
    addOptions,
    emptyText,
    emptyActions = true,
    automationItems,
    providerCard,
    onswitch,
    onsignin,
    ondetails,
    onrefresh,
    ondelete,
  }: {
    view: PageView;
    now: number;
    subtitle: string;
    tabsDisabled: boolean;
    onprovider: (provider: ProviderKey) => void;
    settingsDisabled: boolean;
    onsettings: () => void;
    notices: NoticeView[];
    refreshAll: { label: string; disabled: boolean; run: () => void };
    locked: boolean;
    lockReason: string | null;
    refreshingActive?: boolean;
    activeEmpty: {
      title: string;
      text: string | null;
      action?: { label: string; disabled: boolean; run: () => void };
    };
    addLabel: string;
    addDisabled: boolean;
    addOptions: AddOption[];
    /** What the empty account list says for this provider. */
    emptyText: string;
    emptyActions?: boolean;
    automationItems: AutomationItem[];
    /** The one card that differs per provider. */
    providerCard: Snippet;
    onswitch: RowAction;
    onsignin: RowAction;
    ondetails: RowAction;
    onrefresh: RowAction;
    ondelete: RowAction;
  } = $props();
</script>

<AppShell
  provider={view.provider}
  {subtitle}
  {tabsDisabled}
  {onprovider}
  {settingsDisabled}
  {onsettings}
  {notices}
  updatedAt={view.updatedAt}
  {now}
  {refreshAll}
>
  <main class="dashboard-grid">
    <div class="main-column">
      <ActiveAccountCard
        {view}
        {now}
        {locked}
        {lockReason}
        refreshing={refreshingActive}
        empty={activeEmpty}
        {onrefresh}
        {ondetails}
        {onswitch}
        {onsignin}
      />
      <section class="saved-section" aria-labelledby="accounts-heading">
        <div class="section-heading">
          <h2 id="accounts-heading">
            {t($language, 'savedAccounts')}<span class="count"
              >{view.rows.length}</span
            >
          </h2>
          <AddMenu
            label={addLabel}
            disabled={addDisabled}
            options={addOptions}
          />
        </div>
        {#if view.rows.length}<AccountsTable
            rows={view.rows}
            {now}
            threshold={view.threshold}
            {locked}
            {lockReason}
            {onswitch}
            {onsignin}
            {ondetails}
            {onrefresh}
            {ondelete}
          />{:else if view.loaded}<div class="panel empty-accounts">
            <span class="empty-account-icon" aria-hidden="true"
              ><Icon name="accounts" size={22} /></span
            >
            <h3>{t($language, 'emptyTitle')}</h3>
            <p>{emptyText}</p>
            {#if emptyActions}<div class="empty-actions">
                {#each addOptions as option, index (option.key)}<button
                    class:primary={index === 0}
                    disabled={option.disabled}
                    onclick={(event) => option.run(event.currentTarget)}
                    >{option.label}</button
                  >{/each}
              </div>{/if}
          </div>{/if}
        <UsageOrder order={view.order} empty={view.orderEmpty} />
      </section>
    </div>
    <aside class="insights-column" aria-label={t($language, 'accountInsights')}>
      <NextUpCard {view} {now} {locked} {lockReason} {onswitch} />
      <AutomationCard
        settings={view.settings}
        {settingsDisabled}
        {onsettings}
        items={automationItems}
      />
      {@render providerCard()}
    </aside>
  </main>
</AppShell>
