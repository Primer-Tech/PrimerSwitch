<script lang="ts">
  import { language, t, plural, subscriptionLabel } from '../lib/i18n';
  import { date, duration } from '../lib/format';
  import { errorText, safeError, type Controller } from '../lib/controller';
  import type { AccountView, ErrorView } from '../lib/types';
  import { claudeView } from '../lib/claude-view';
  import type {
    AccountRowView,
    AddOption,
    AutomationItem,
    NoticeView,
    ProviderKey,
  } from '../lib/provider-view';
  import ProviderPage from './ProviderPage.svelte';
  import AccountDetails from './AccountDetails.svelte';
  import DeleteDialog from './DeleteDialog.svelte';
  import Modal from './Modal.svelte';
  import Icon from './Icon.svelte';
  let {
    controller,
    now,
    locked,
    tabsDisabled,
    onprovider,
    settingsDisabled,
    onsettings,
  }: {
    controller: Controller;
    now: number;
    /** Any pending action, busy runtime, sign-in or demo on either provider. */
    locked: boolean;
    tabsDisabled: boolean;
    onprovider: (provider: ProviderKey) => void;
    settingsDisabled: boolean;
    onsettings: () => void;
  } = $props();
  let snapshot = $derived($controller.snapshot);
  let pending = $derived($controller.pending);
  /** The row action this window started: `switch:<id>` or `refresh:<id>`. */
  let operation = $state<string | null>(null);
  let view = $derived(
    claudeView(snapshot, {
      locale: $language,
      now,
      switchingId: operation?.startsWith('switch:')
        ? operation.slice('switch:'.length)
        : null,
    }),
  );
  let lockReason = $derived(
    t($language, snapshot?.demo ? 'demoOnly' : 'switchBusy'),
  );
  let loginCode = $state('');
  let detailsId = $state<string | null>(null);
  let deleteTarget = $state<AccountRowView | null>(null);
  let returnFocus = $state<HTMLElement | null>(null);
  let details = $derived(view.rows.find((r) => r.id === detailsId) ?? null);
  const raw = (id: string | undefined): AccountView | null =>
    snapshot?.accounts.find((account) => account.id === id) ?? null;
  // A background error repeats on every tick while it lasts: once dismissed it stays
  // hidden until it clears or a different one appears. Notices are per occurrence.
  const errorKey = (error: ErrorView) =>
    `${error.code}|${error.accountId ?? ''}|${error.action ?? ''}`;
  let dismissedError = $state<string | null>(null),
    dismissedNotice = $state<string | null>(null),
    dismissedSignIn = $state<string | null>(null);
  $effect(() => {
    if (snapshot && !snapshot.busy && !snapshot.error) dismissedError = null;
  });
  $effect(() => {
    if (!$controller.login.open) loginCode = '';
  });
  // A context that cannot switch at all explains itself instead of an empty window.
  const blockingCodes = [
    'unsupportedContext',
    'unsupportedEnvironment',
    'unsupportedSetting',
  ];
  let blocked = $derived(
    snapshot &&
      !snapshot.accounts.length &&
      snapshot.error &&
      blockingCodes.includes(snapshot.error.code)
      ? snapshot.error
      : null,
  );
  let statusError = $derived(
    snapshot?.error &&
      snapshot.error !== blocked &&
      errorKey(snapshot.error) !== dismissedError
      ? snapshot.error
      : null,
  );
  let notice = $derived(
    snapshot?.notice &&
      `${errorKey(snapshot.notice)}|${snapshot.notice.at}` !== dismissedNotice
      ? snapshot.notice
      : null,
  );
  function dismissError() {
    // A rejected action is reported twice (reply and snapshot): dismiss both.
    if (snapshot?.error) dismissedError = errorKey(snapshot.error);
    controller.dismissError();
  }
  async function run(kind: 'switch' | 'refresh', row: AccountRowView) {
    operation = `${kind}:${row.id}`;
    await controller.action(
      kind === 'switch' ? 'switch_account' : 'refresh_account',
      { id: row.id },
    );
    operation = null;
  }
  function signIn(trigger: HTMLElement | null) {
    detailsId = null;
    returnFocus = trigger;
    void controller.beginLogin();
  }
  function askDelete(row: AccountRowView, trigger: HTMLElement | null) {
    returnFocus = trigger;
    detailsId = null;
    deleteTarget = row;
  }
  async function confirmDelete() {
    const target = deleteTarget;
    if (
      target &&
      (await controller.action('delete_account', { id: target.id }))
    )
      deleteTarget = null;
  }
  let notices = $derived.by(() => {
    const list: NoticeView[] = [];
    if (pending || snapshot?.busy)
      list.push({
        key: 'progress',
        tone: 'progress',
        role: 'status',
        text: t(
          $language,
          operation?.startsWith('switch:')
            ? 'switchingAccount'
            : 'updatingAccounts',
        ),
      });
    const error = $controller.error;
    if (error || statusError)
      list.push({
        key: 'error',
        tone: 'error',
        role: 'alert',
        text: error
          ? safeError(error, $language)
          : errorText(statusError!, snapshot?.accounts ?? [], $language),
        dismiss: dismissError,
      });
    if (notice)
      list.push({
        key: 'notice',
        tone: 'info',
        role: 'status',
        text: errorText(notice, snapshot?.accounts ?? [], $language),
        dismiss: () => (dismissedNotice = `${errorKey(notice!)}|${notice!.at}`),
      });
    if (view.allLimited)
      list.push({
        key: 'limited',
        tone: 'info',
        role: 'status',
        text: view.allLimited,
      });
    const signInKey = view.signIn.map((r) => r.id).join(',');
    if (view.signIn.length && signInKey !== dismissedSignIn)
      list.push({
        key: 'sign-in',
        tone: 'warn',
        role: 'note',
        text: t($language, 'signInNeeded', {
          names: view.signIn.map((r) => r.name).join(', '),
        }),
        action:
          view.signIn.length === 1
            ? {
                label: t($language, 'signInAgain'),
                disabled: locked,
                run: (trigger) => signIn(trigger),
              }
            : undefined,
        dismiss: () => (dismissedSignIn = signInKey),
      });
    if (snapshot?.demo)
      list.push({
        key: 'demo',
        tone: 'demo',
        role: 'status',
        title: t($language, 'preview'),
        text: t($language, 'demoNotice'),
        label: t($language, 'demoNotice'),
      });
    return list;
  });
  let addOptions = $derived<AddOption[]>([
    {
      key: 'sign-in',
      label: t($language, 'claudeAddSignIn'),
      icon: 'plus',
      disabled: locked,
      run: (trigger) => signIn(trigger),
    },
    {
      key: 'current',
      label: t($language, 'claudeImportCurrent'),
      icon: 'import',
      disabled: locked,
      run: () => void controller.action('import_current'),
    },
    {
      key: 'archive',
      label: t($language, 'claudeImportArchive'),
      icon: 'archive',
      disabled: locked,
      run: () => void controller.action('preview_legacy_import'),
    },
  ]);
  let automationItems = $derived<AutomationItem[]>([
    {
      kind: 'toggle',
      label: t($language, 'primeWindow'),
      on: !!snapshot?.settings.autoStartWindowEnabled,
    },
    {
      kind: 'toggle',
      label: t($language, 'autoResets'),
      on: !!snapshot?.settings.autoUseResetsEnabled,
    },
  ]);
  let activeRaw = $derived(raw(view.active?.id));
</script>

<ProviderPage
  {view}
  {now}
  subtitle={t($language, 'claudeSubtitle')}
  {tabsDisabled}
  {onprovider}
  {settingsDisabled}
  {onsettings}
  {notices}
  refreshAll={{
    label: t(
      $language,
      pending === 'refresh_all' ? 'refreshing' : 'refreshAll',
    ),
    disabled: locked,
    run: () => void controller.action('refresh_all'),
  }}
  {locked}
  {lockReason}
  refreshingActive={pending === 'refresh_all' ||
    (!!view.active && operation === `refresh:${view.active.id}`)}
  activeEmpty={blocked
    ? {
        title: t($language, 'switchingUnavailable'),
        text: errorText(blocked, [], $language),
      }
    : snapshot
      ? {
          title: t($language, 'noActive'),
          text: t($language, 'noActiveClaude'),
        }
      : {
          title: t($language, 'loadingAccounts'),
          text: t($language, 'loadingState'),
        }}
  addLabel={t($language, 'addAccount')}
  addDisabled={locked}
  {addOptions}
  emptyText={t($language, 'emptyClaude')}
  emptyActions={!blocked}
  {automationItems}
  onswitch={(row) => void run('switch', row)}
  onsignin={(_row, trigger) => signIn(trigger)}
  ondetails={(row, trigger) => {
    returnFocus = trigger;
    detailsId = row.id;
  }}
  onrefresh={(row) => void run('refresh', row)}
  ondelete={askDelete}
>
  {#snippet providerCard()}
    <section
      class="panel side-panel credits-panel"
      aria-labelledby="credits-heading"
    >
      <h2 id="credits-heading">
        <Icon name="credits" />{t($language, 'creditsRenewal')}
      </h2>
      <div class="credits-value">
        <strong>{activeRaw?.resets.available ?? '—'}</strong><span
          >{t($language, 'availableResets')}</span
        >
      </div>
      {#if activeRaw?.resets.expiresAt && activeRaw.resets.expiresAt > now}<small
          >{t($language, 'expiresIn', {
            duration: duration(activeRaw.resets.expiresAt, now, $language),
          })}</small
        >{/if}{#if activeRaw?.resets.pending}<p class="warning-text">
          {t($language, 'checkingReset')}
        </p>{/if}{#if activeRaw?.resets.cooldownUntil && activeRaw.resets.cooldownUntil > now}<p
          class="warning-text"
        >
          {t($language, 'resetCooldown', {
            duration: duration(activeRaw.resets.cooldownUntil, now, $language),
          })}
        </p>{/if}
      <div class="renewal-summary">
        <Icon name="calendar" />
        <div>
          <strong>{t($language, 'estimatedRenewal')}</strong><span
            >{date(activeRaw?.nextRenewalAt ?? null, $language)}</span
          ><small>{t($language, 'manualRenewalDescription')}</small>
        </div>
      </div>
    </section>
  {/snippet}
</ProviderPage>

{#if details}{@const account = raw(details.id)}<Modal
    title={details.name}
    {returnFocus}
    onclose={() => (detailsId = null)}
  >
    <AccountDetails
      row={details}
      {now}
      threshold={view.threshold}
      {locked}
      {lockReason}
      onrefresh={() => void run('refresh', details!)}
      onswitch={() => {
        const row = details!;
        detailsId = null;
        void run('switch', row);
      }}
      onsignin={() => signIn(returnFocus)}
      ondelete={() => askDelete(details!, returnFocus)}
    >
      {#snippet facts()}
        {#if account}
          <div>
            <dt>{t($language, 'subscription')}</dt>
            <dd>
              {account.subscriptionStatus
                ? subscriptionLabel(account.subscriptionStatus, $language)
                : t($language, 'unknownStatus')}
            </dd>
          </div>
          <div>
            <dt>
              <label for={`renewal-${account.id}`}
                >{t($language, 'estimatedRenewal')}</label
              >
            </dt>
            <dd>
              <select
                id={`renewal-${account.id}`}
                class="renewal-select"
                aria-label={t($language, 'renewalLabel', {
                  name: account.name,
                })}
                value={account.renewalDay ?? ''}
                disabled={locked}
                onchange={(event) => {
                  const day =
                    event.currentTarget.value === ''
                      ? null
                      : Number(event.currentTarget.value);
                  event.currentTarget.value = String(account.renewalDay ?? '');
                  void controller.action('set_renewal_day', {
                    id: account.id,
                    day,
                  });
                }}
                ><option value="">{t($language, 'unknown')}</option
                >{#each Array.from({ length: 31 }, (_, i) => i + 1) as day (day)}<option
                    value={day}>{t($language, 'renewalDay', { day })}</option
                  >{/each}</select
              >{#if account.nextRenewalAt !== null}<small class="fact-note"
                  >{t($language, 'manualEstimate', {
                    date: date(account.nextRenewalAt, $language),
                  })}</small
                >{/if}
            </dd>
          </div>
          <div>
            <dt>{t($language, 'availableResets')}</dt>
            <dd>
              {account.resets.available ?? t($language, 'unknownCount')}
              {#if account.resets.expiresAt !== null && account.resets.expiresAt > now}<small
                  class="fact-note"
                  >{t($language, 'expiresIn', {
                    duration: duration(
                      account.resets.expiresAt,
                      now,
                      $language,
                    ),
                  })}</small
                >{/if}{#if account.resets.cooldownUntil !== null && account.resets.cooldownUntil > now}<small
                  class="fact-note"
                  >{t($language, 'resetCooldown', {
                    duration: duration(
                      account.resets.cooldownUntil,
                      now,
                      $language,
                    ),
                  })}</small
                >{/if}{#if account.resets.pending}<small
                  class="fact-note warning-text"
                  >{t($language, 'checkingReset')}</small
                >{/if}{#if account.resets.lastOutcome}<small class="fact-note"
                  >{safeError(account.resets.lastOutcome, $language)}</small
                >{/if}
            </dd>
          </div>
          {#if account.primedAt !== null}<div>
              <dt>{t($language, 'primedLabel')}</dt>
              <dd>{date(account.primedAt, $language)}</dd>
            </div>{/if}
        {/if}
      {/snippet}
    </AccountDetails>
  </Modal>{/if}

{#if deleteTarget}<DeleteDialog
    text={t($language, 'deleteClaude', { name: deleteTarget.name })}
    pending={pending === 'delete_account'}
    disabled={locked}
    error={$controller.error ? safeError($controller.error, $language) : null}
    {returnFocus}
    onconfirm={() => void confirmDelete()}
    oncancel={() => (deleteTarget = null)}
  />{/if}

{#if $controller.login.open}
  <Modal
    title={t($language, 'claudeLoginTitle')}
    {returnFocus}
    onclose={() => {
      void controller.cancelLogin();
    }}
  >
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void controller.finishLogin(loginCode);
      }}
    >
      <p class="modal-intro">{t($language, 'loginDescription')}</p>
      <ol class="login-steps">
        <li><span>1</span>{t($language, 'loginStepOne')}</li>
        <li>
          <span>2</span>{t($language, 'loginStepTwo')} <code>#state</code>.
        </li>
        <li><span>3</span>{t($language, 'loginStepThree')}</li>
      </ol>
      {#if $controller.login.working}<p class="working-message" role="status">
          <span class="spinner" aria-hidden="true"></span>{$controller.login
            .session
            ? t($language, 'addingAccount')
            : t($language, 'openingBrowser')}
        </p>{/if}
      <label class="field-label" for="login-code"
        >{t($language, 'authorizationCode')}</label
      ><input
        id="login-code"
        type="password"
        autocomplete="off"
        spellcheck="false"
        placeholder="code#state"
        bind:value={loginCode}
        disabled={!$controller.login.session || $controller.login.working}
      />
      {#if $controller.login.error}<p class="warning-text" role="alert">
          {safeError($controller.login.error, $language)}
        </p>{/if}
      <p class="help">{t($language, 'loginNoActivate')}</p>
      <div class="modal-actions">
        <button type="button" onclick={() => controller.cancelLogin()}
          >{t($language, 'cancel')}</button
        ><button
          class="primary"
          type="submit"
          disabled={!$controller.login.session ||
            $controller.login.working ||
            !loginCode.trim()}>{t($language, 'addAccount')}</button
        >
      </div>
    </form>
  </Modal>
{/if}
{#if $controller.preview}
  <Modal
    title={t($language, 'importTitle')}
    onclose={() => controller.dismissPreview()}
    ><p class="modal-intro">
      {plural($language, 'importReady', $controller.preview.valid)}
    </p>
    {#if $controller.preview.invalid}<p class="warning-text">
        {plural($language, 'importInvalid', $controller.preview.invalid)}
      </p>{/if}
    <ul class="import-list">
      {#each $controller.preview.names as name, index (`${name}:${index}`)}<li>
          {name}
        </li>{/each}
    </ul>
    {#if $controller.error}<p class="warning-text" role="alert">
        {safeError($controller.error, $language)}
      </p>{/if}
    <div class="modal-actions">
      <button
        disabled={!!$controller.pending}
        onclick={() => controller.dismissPreview()}
        >{t($language, 'cancel')}</button
      ><button
        class="primary"
        disabled={locked || !$controller.preview.valid}
        onclick={async () => {
          const preview = $controller.preview;
          if (
            preview &&
            (await controller.action('apply_legacy_import', {
              previewId: preview.id,
            }))
          )
            controller.dismissPreview();
        }}
        >{$controller.pending === 'apply_legacy_import'
          ? t($language, 'importing')
          : t($language, 'importAccounts')}</button
      >
    </div></Modal
  >
{/if}
