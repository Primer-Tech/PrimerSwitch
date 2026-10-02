<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { age, countdown, date, percentage } from '../lib/format';
  import { safeError } from '../lib/controller';
  import type { Snapshot, AccountView } from '../lib/types';
  import Icon from './Icon.svelte';
  import QuotaBar from './QuotaBar.svelte';
  import meta from '../../package.json';
  import primerLogo from '../assets/primer-logo.png';
  let {
    snapshot,
    now,
    disabled,
    operation,
    error,
    pending,
    onsettings,
    onlogin,
    oncurrent,
    onarchive,
    onrefresh,
    onrefreshactive,
    onswitch,
    ondetails,
    ondismiss,
  }: {
    snapshot: Snapshot | null;
    now: number;
    disabled: boolean;
    operation: string | null;
    error: string | null;
    pending: string | null;
    onsettings: () => void;
    onlogin: () => void;
    oncurrent: () => void;
    onarchive: () => void;
    onrefresh: () => void;
    onrefreshactive: (id: string) => void;
    onswitch: (a: AccountView) => void;
    ondetails: (a: AccountView, trigger: HTMLElement) => void;
    ondismiss: () => void;
  } = $props();
  let active = $derived(
    snapshot?.accounts.find((a) => a.id === snapshot.activeId) ?? null,
  );
  let next = $derived(snapshot?.accounts.find((a) => a.isNext) ?? null);
  let plan = $derived(
    (snapshot?.consumptionPlan ?? [])
      .map((id) => snapshot?.accounts.find((a) => a.id === id))
      .filter((a): a is AccountView => !!a),
  );
  let addOpen = $state(false),
    addButton: HTMLButtonElement;
  function add(action: () => void) {
    addOpen = false;
    addButton?.focus();
    action();
  }
  function closeAdd(event: KeyboardEvent) {
    if (event.key === 'Escape' && addOpen) {
      event.preventDefault();
      addOpen = false;
      addButton?.focus();
    }
  }
</script>

<svelte:window onkeydown={closeAdd} />
<div class="desktop-shell">
  <aside class="sidebar" aria-label={t($language, 'navigation')}>
    <div class="sidebar-brand" role="img" aria-label="PrimerSwitch">
      <img class="primer-mark" src={primerLogo} alt="" aria-hidden="true" />
      <span class="primer-wordmark" aria-hidden="true">
        <strong>primer</strong><span>Switch</span>
      </span>
    </div>
    <nav>
      <button
        class="nav-current"
        aria-current="page"
        onclick={() =>
          document
            .getElementById('accounts-heading')
            ?.scrollIntoView({ behavior: 'smooth' })}
        ><Icon name="accounts" /><span>{t($language, 'accounts')}</span></button
      ><button
        aria-label={t($language, 'openSettings')}
        disabled={!snapshot}
        onclick={onsettings}
        ><Icon name="settings" /><span>{t($language, 'settings')}</span></button
      >
    </nav>
    <div class="sidebar-bottom">
      <span class="status-dot"></span><span
        >{t($language, 'localAccounts')}</span
      >
    </div>
  </aside>
  <div class="workspace">
    <header class="workspace-header">
      <div>
        <h1>{t($language, 'accounts')}</h1>
        <p>{t($language, 'dashboardDescription')}</p>
      </div>
      <div class="providers">
        <span class="provider-tab">✳ Claude</span><button disabled
          >Codex <small>{t($language, 'comingSoon')}</small></button
        >
      </div>
    </header>
    {#if snapshot?.demo}<div class="demo-notice" role="status">
        <strong>{t($language, 'preview')}</strong><span
          >{t($language, 'demoNotice')}</span
        >
      </div>{/if}
    {#if error || snapshot?.error}<div class="error-notice" role="alert">
        <span>{safeError(error ?? snapshot?.error, $language)}</span
        >{#if error}<button
            aria-label={t($language, 'dismissMessage')}
            class="icon-button"
            onclick={ondismiss}>×</button
          >{/if}
      </div>{/if}
    {#if pending || snapshot?.busy}<div class="pending-notice" role="status">
        <span class="spinner" aria-hidden="true"></span>{operation?.startsWith(
          'switch:',
        )
          ? t($language, 'switchingAccount')
          : t($language, 'updatingAccounts')}
      </div>{/if}
    <main class="dashboard-grid">
      <div class="main-column">
        <section class="panel active-panel" aria-labelledby="active-heading">
          <div class="panel-heading">
            <h2 id="active-heading">
              <span class="status-dot" class:inactive={!active}></span>{t(
                $language,
                'activeAccount',
              )}
            </h2>
            {#if active}<button
                class="quiet-button"
                {disabled}
                aria-label={t($language, 'refreshLabel', { name: active.name })}
                onclick={() => onrefreshactive(active!.id)}
                ><Icon name="refresh" />{t($language, 'refresh')}</button
              >{/if}
          </div>
          {#if active}
            <div class="active-title">
              <span class="avatar large-avatar"
                >{active.name.slice(0, 1).toUpperCase()}</span
              >
              <div>
                <div class="identity-line">
                  <h3>{active.name}</h3>
                  {#if active.planTier}<span class="badge muted"
                      >{active.planTier}</span
                    >{/if}<span class="badge accent"
                    >{t($language, 'active')}</span
                  >
                </div>
                <p>{active.email || t($language, 'emailUnavailable')}</p>
              </div>
              <span class="active-model"
                >{snapshot?.activeModel ?? t($language, 'defaultModel')}</span
              >
            </div>
            <div class="active-quotas">
              <QuotaBar
                value={active.usage?.fiveHour.utilization ?? null}
                label={t($language, 'lastFiveHours')}
                resetAt={active.usage?.fiveHour.resetsAt ?? null}
                {now}
                threshold={snapshot?.settings.threshold}
              />
              <QuotaBar
                value={active.usage?.weeklyOverall ?? null}
                label={t($language, 'weeklyOverall')}
                resetAt={active.usage?.weeklyOverallResetsAt ?? null}
                {now}
                tone="green"
                threshold={snapshot?.settings.threshold}
              />
              <QuotaBar
                value={active.usage?.weeklyModel ?? null}
                label={t($language, 'weeklySelected')}
                description={t($language, 'weeklyEffectiveHelp')}
                resetAt={active.usage?.weeklyModelResetsAt ?? null}
                {now}
                tone="violet"
                threshold={snapshot?.settings.threshold}
              />
            </div>
            <div class="active-caption">
              <span
                class:warning-text={!!active.error ||
                  active.decisionFresh === false}
                >{!active.usage
                  ? t($language, 'usageUnavailable')
                  : t(
                      $language,
                      active.error || active.decisionFresh === false
                        ? 'cachedAge'
                        : 'readAge',
                      { age: age(active.usageAt, now, $language) },
                    )}</span
              ><button
                class="text-button"
                onclick={(event) => ondetails(active!, event.currentTarget)}
                >{t($language, 'viewAccountDetails')}
                <Icon name="next" size={14} /></button
              >
            </div>
            {#if active.decisionFresh === false}<p class="waiting-message">
                {t($language, 'automationWaiting')}
              </p>{/if}
          {:else}<div class="empty-active">
              <div>
                <h3>
                  {snapshot
                    ? t($language, 'noActive')
                    : t($language, 'loadingAccounts')}
                </h3>
                <p>
                  {snapshot
                    ? t($language, 'importOrAdd')
                    : t($language, 'loadingState')}
                </p>
              </div>
            </div>{/if}
        </section>
        <section class="saved-section" aria-labelledby="accounts-heading">
          <div class="section-heading">
            <h2 id="accounts-heading">
              {t($language, 'savedAccounts')}
              <span class="count">{snapshot?.accounts.length ?? 0}</span>
            </h2>
            <div
              class="add-menu"
              role="group"
              aria-label={t($language, 'addActions')}
            >
              <button
                class="primary"
                bind:this={addButton}
                {disabled}
                aria-expanded={addOpen}
                aria-controls="add-actions"
                onclick={() => (addOpen = !addOpen)}
                ><Icon name="plus" size={16} />{t($language, 'addAccount')}<Icon
                  name="chevron"
                  size={14}
                /></button
              >{#if addOpen}<div class="add-options" id="add-actions">
                  <button {disabled} onclick={() => add(onlogin)}
                    ><Icon name="plus" />{t($language, 'addClaude')}</button
                  ><button {disabled} onclick={() => add(oncurrent)}
                    ><Icon name="import" />{t(
                      $language,
                      'importCurrent',
                    )}</button
                  ><button {disabled} onclick={() => add(onarchive)}
                    ><Icon name="archive" />{t(
                      $language,
                      'importArchive',
                    )}</button
                  >
                </div>{/if}
            </div>
          </div>
          {#if snapshot?.accounts.length}<div class="table-scroll">
              <table class="account-table">
                <thead
                  ><tr
                    ><th>{t($language, 'account')}</th><th
                      >{t($language, 'fiveHourUsage')}</th
                    ><th
                      title={t($language, 'weeklyEffectiveHelp')}
                      aria-label={t($language, 'weeklyEffectiveHelp')}
                      >{t($language, 'weeklyEffective')}</th
                    ><th
                      ><span class="sr-only">{t($language, 'actions')}</span
                      ></th
                    ></tr
                  ></thead
                ><tbody>
                  {#each snapshot.accounts as account (account.id)}<tr
                      class:current-row={account.active}
                      ><td
                        ><div class="table-identity">
                          <span class="avatar"
                            >{account.name.slice(0, 1).toUpperCase()}</span
                          >
                          <div>
                            <div class="identity-line">
                              <strong>{account.name}</strong
                              >{#if account.active}<span class="row-badge"
                                  >{t($language, 'active')}</span
                                >{:else if account.isNext}<span
                                  class="row-badge next-badge"
                                  >{t($language, 'next')}</span
                                >{/if}
                            </div>
                            <span class="account-email"
                              >{account.email ||
                                t($language, 'emailUnavailable')}</span
                            >{#if account.error && account.usage}<small
                                class="warning-text"
                                >{t($language, 'lastReadingKept')}</small
                              >{:else if !account.usage}<small
                                >{t($language, 'noUsage')}</small
                              >{:else if account.decisionFresh === false}<small
                                >{t($language, 'cachedAge', {
                                  age: age(account.usageAt, now, $language),
                                })}</small
                              >{/if}
                          </div>
                        </div></td
                      >
                      <td
                        ><QuotaBar
                          compact
                          label={t($language, 'fiveHours')}
                          value={account.usage?.fiveHour.utilization ?? null}
                          {now}
                          threshold={snapshot.settings.threshold}
                        /></td
                      ><td
                        ><QuotaBar
                          compact
                          label={t($language, 'weeklyEffective')}
                          description={t($language, 'weeklyEffectiveHelp')}
                          value={account.usage?.weeklyModel ?? null}
                          {now}
                          tone="green"
                          threshold={snapshot.settings.threshold}
                        /></td
                      >
                      <td
                        ><div class="row-actions">
                          <button
                            class="switch-button"
                            disabled={disabled ||
                              account.active ||
                              !account.identityVerified}
                            onclick={() => onswitch(account)}
                            >{account.active
                              ? t($language, 'currentAccount')
                              : operation === 'switch:' + account.id
                                ? t($language, 'switching')
                                : t($language, 'switch')}</button
                          ><button
                            class="icon-button"
                            aria-label={t($language, 'detailsLabel', {
                              name: account.name,
                            })}
                            onclick={(event) =>
                              ondetails(account, event.currentTarget)}
                            ><Icon name="more" /></button
                          >
                        </div></td
                      ></tr
                    >{/each}
                </tbody>
              </table>
            </div>{:else if snapshot}<div class="panel empty-accounts">
              <h3>{t($language, 'emptyTitle')}</h3>
              <p>{t($language, 'emptyDescription')}</p>
              <button {disabled} onclick={oncurrent}
                >{t($language, 'importCurrent')}</button
              >
            </div>{/if}
        </section>
        <section
          class="consumption-strip"
          aria-label={t($language, 'consumptionOrder')}
        >
          <span>{t($language, 'consumptionOrderShort')}</span
          >{#if plan.length}<ol>
              {#each plan as account, index (account.id)}<li>
                  <span class="plan-number">{index + 1}</span
                  >{account.name}{#if index < plan.length - 1}<Icon
                      name="next"
                      size={13}
                    />{/if}
                </li>{/each}
            </ol>{:else}<small
              >{snapshot?.accounts.length
                ? t($language, 'noUsable')
                : t($language, 'afterFirstRead')}</small
            >{/if}
        </section>
      </div>
      <aside
        class="insights-column"
        aria-label={t($language, 'accountInsights')}
      >
        <section class="panel next-panel">
          <h2><Icon name="next" />{t($language, 'nextUp')}</h2>
          {#if next}<div class="next-identity">
              <span class="avatar">{next.name.slice(0, 1).toUpperCase()}</span>
              <div>
                <strong>{next.name}</strong><span
                  >{next.email || t($language, 'emailUnavailable')}</span
                >
              </div>
            </div>
            <div class="next-quotas">
              <QuotaBar
                compact
                label={t($language, 'fiveHours')}
                value={next.usage?.fiveHour.utilization ?? null}
                {now}
              /><QuotaBar
                compact
                label={t($language, 'weeklyEffective')}
                description={t($language, 'weeklyEffectiveHelp')}
                value={next.usage?.weeklyModel ?? null}
                {now}
                tone="green"
              />
            </div>
            <p>{t($language, 'nextUpDescription')}</p>{:else}<p>
              {t($language, 'noNext')}
            </p>{/if}
        </section>
        <section class="panel automation-panel">
          <div class="panel-heading">
            <h2><Icon name="automation" />{t($language, 'automation')}</h2>
            <button
              class="icon-button"
              aria-label={t($language, 'manageAutomation')}
              disabled={!snapshot}
              onclick={onsettings}><Icon name="settings" size={16} /></button
            >
          </div>
          <p class="automation-state">
            <span
              class="status-dot"
              class:inactive={!snapshot?.settings.autoSwitchEnabled}
            ></span>{t(
              $language,
              snapshot?.settings.autoSwitchEnabled
                ? 'autoSwitchOn'
                : 'autoSwitchOff',
            )}
          </p>
          <p>
            {t($language, 'automationThresholdHelp', {
              threshold: percentage(
                snapshot?.settings.threshold ?? 95,
                $language,
              ),
            })}
          </p>
          <dl class="automation-list">
            <div>
              <dt>{t($language, 'primeWindow')}</dt>
              <dd class:enabled={snapshot?.settings.autoStartWindowEnabled}>
                {t(
                  $language,
                  snapshot?.settings.autoStartWindowEnabled ? 'on' : 'off',
                )}
              </dd>
            </div>
            <div>
              <dt>{t($language, 'autoResets')}</dt>
              <dd class:enabled={snapshot?.settings.autoUseResetsEnabled}>
                {t(
                  $language,
                  snapshot?.settings.autoUseResetsEnabled ? 'on' : 'off',
                )}
              </dd>
            </div>
            <div>
              <dt>{t($language, 'pollInterval')}</dt>
              <dd>
                {t($language, 'minutes', {
                  count: percentage(
                    (snapshot?.settings.pollInterval ?? 300) / 60,
                    $language,
                  ),
                })}
              </dd>
            </div>
          </dl>
        </section>
        <section class="panel credits-panel">
          <h2><Icon name="credits" />{t($language, 'creditsRenewal')}</h2>
          <div class="credits-value">
            <strong>{active?.resets.available ?? '—'}</strong><span
              >{t($language, 'availableResets')}</span
            >
          </div>

          {#if active?.resets.expiresAt}<small
              >{t($language, 'expiresIn', {
                duration: countdown(active.resets.expiresAt, now, $language),
              })}</small
            >{/if}{#if active?.resets.pending}<p class="warning-text">
              {t($language, 'checkingReset')}
            </p>{/if}{#if active?.resets.cooldownUntil && active.resets.cooldownUntil > now}<p
              class="warning-text"
            >
              {t($language, 'resetCooldown', {
                duration: countdown(
                  active.resets.cooldownUntil,
                  now,
                  $language,
                ),
              })}
            </p>{/if}
          <div class="renewal-summary">
            <Icon name="calendar" />
            <div>
              <strong>{t($language, 'estimatedRenewal')}</strong><span
                >{date(active?.nextRenewalAt ?? null, $language)}</span
              ><small>{t($language, 'manualRenewalDescription')}</small>
            </div>
          </div>
        </section>
      </aside>
    </main>
    <footer class="workspace-footer">
      <div>
        <span class="status-dot" class:inactive={!snapshot?.lastRefreshAt}
        ></span><span
          >{snapshot?.lastRefreshAt
            ? t($language, 'updatedAge', {
                age: age(snapshot.lastRefreshAt, now, $language),
              })
            : t($language, 'noUpdates')}</span
        >
      </div>
      <span class="version">PrimerSwitch {meta.version}</span><button
        class="quiet-button"
        {disabled}
        onclick={onrefresh}
        ><Icon name="refresh" size={15} />{t($language, 'refreshAll')}</button
      >
    </footer>
  </div>
</div>
