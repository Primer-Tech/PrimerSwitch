<script lang="ts">
  import { tick } from 'svelte';
  import { language, t } from '../lib/i18n';
  import { codeMessage, safeError, type Controller } from '../lib/controller';
  import {
    launchBlock,
    launchBlockText,
    launchModelLabels,
    launchReasonText,
  } from '../lib/claude-launch';
  import { launchModelSchema, launchModels } from '../lib/types';
  import Modal from './Modal.svelte';
  let {
    controller,
    returnFocus = null,
  }: {
    controller: Controller;
    /** The control that opened the dialog; it gets focus back when it closes. */
    returnFocus?: HTMLElement | null;
  } = $props();
  const uid = $props.id();
  let launch = $derived($controller.launch);
  let snapshot = $derived($controller.snapshot);
  let account = $derived(
    snapshot?.accounts.find((candidate) => candidate.id === launch.accountId) ??
      null,
  );
  // The same rules as the list: the native precheck still decides.
  let block = $derived(
    snapshot && account ? launchBlock(snapshot, account) : null,
  );
  let busy = $derived(!!snapshot?.busy || !!$controller.pending);
  let unavailable = $derived(
    !account
      ? codeMessage('missingAccount', $language)
      : block
        ? launchBlockText(block, $language)
        : busy
          ? t($language, 'switchBusy')
          : null,
  );
  let name = $derived(account?.name ?? '');
  let sessionId = $derived(launch.session?.id ?? null);
  let loginCode = $state('');
  let root = $state<HTMLElement | null>(null);
  // A new sign-in (or none) never inherits the code typed for an earlier one.
  $effect(() => {
    void sessionId;
    loginCode = '';
  });
  // A step that replaces the focused control (a stage change) must not strand
  // keyboard focus: move it to the new stage's main control.
  $effect(() => {
    void launch.stage;
    void launch.error;
    if (launch.working || !root) return;
    void tick().then(() => {
      const dialog = root?.closest('dialog');
      const active = document.activeElement;
      if (!dialog || (active && active !== dialog && dialog.contains(active)))
        return;
      root
        ?.querySelector<HTMLElement>('[data-autofocus]:not(:disabled)')
        ?.focus();
    });
  });
  const cancel = () => void controller.cancelLaunch();
</script>

{#if launch.open}
  <Modal title={t($language, 'launchTitle')} {returnFocus} onclose={cancel}>
    <div bind:this={root}>
      <p class="launch-account">
        <strong>{account?.name ?? t($language, 'unknown')}</strong
        >{#if account}<span
            >{[account.email, account.workspaceLabel]
              .filter(Boolean)
              .join(' · ')}</span
          >{/if}
      </p>
      {#if launch.stage === 'choose'}
        <form
          onsubmit={(event) => {
            event.preventDefault();
            void controller.startLaunch();
          }}
        >
          <p class="modal-intro">{t($language, 'launchIntro')}</p>
          <label class="field-label" for={`${uid}-model`}
            >{t($language, 'launchModel')}</label
          ><select
            id={`${uid}-model`}
            class="launch-model"
            value={launch.model}
            disabled={launch.working}
            onchange={(event) => {
              const model = launchModelSchema.safeParse(
                event.currentTarget.value,
              );
              if (model.success) controller.setLaunchModel(model.data);
            }}
            >{#each launchModels as model (model)}<option value={model}
                >{t($language, launchModelLabels[model])}</option
              >{/each}</select
          >
          <p class="help">{t($language, 'launchSharing')}</p>
          <p class="help">{t($language, 'launchQuotaHint')}</p>
          <p class="help">{t($language, 'launchFolderNext')}</p>
          {#if launch.working}<p class="working-message" role="status">
              <span class="spinner" aria-hidden="true"></span>{t(
                $language,
                'launchWaitingFolder',
              )}
            </p>{/if}
          {#if launch.info}<p class="help" role="status">
              {t(
                $language,
                launch.info === 'cancelled'
                  ? 'launchNoFolder'
                  : 'launchAuthorizedInfo',
              )}
            </p>{/if}
          {#if launch.error}<p class="warning-text" role="alert">
              {safeError(launch.error, $language)}
            </p>{:else if unavailable}<p class="warning-text">
              {unavailable}
            </p>{/if}
          <div class="modal-actions">
            <button type="button" onclick={cancel}
              >{t($language, 'cancel')}</button
            ><button
              class="primary"
              type="submit"
              data-autofocus
              disabled={launch.working || !!unavailable}
              >{t($language, 'launchAction')}</button
            >
          </div>
        </form>
      {:else if launch.stage === 'authorize'}
        <p class="modal-intro">
          {launchReasonText(launch.reason, $language, name)}
        </p>
        {#if account?.workspaceLabel}<p class="help">
            {t($language, 'claudeLoginOriginalWorkspace', {
              workspace: account.workspaceLabel,
            })}
          </p>{/if}
        <p class="help">{t($language, 'launchAuthNote')}</p>
        {#if launch.working}<p class="working-message" role="status">
            <span class="spinner" aria-hidden="true"></span>{t(
              $language,
              'openingBrowser',
            )}
          </p>{/if}
        {#if launch.error}<p class="warning-text" role="alert">
            {safeError(launch.error, $language)}
          </p>{:else if unavailable}<p class="warning-text">
            {unavailable}
          </p>{/if}
        <div class="modal-actions">
          <button type="button" onclick={cancel}
            >{t($language, 'cancel')}</button
          ><button
            class="primary"
            type="button"
            data-autofocus
            disabled={launch.working || !!unavailable}
            onclick={() => void controller.beginLaunchAuthorization()}
            >{t($language, 'launchOpenBrowser')}</button
          >
        </div>
      {:else}
        <form
          onsubmit={(event) => {
            event.preventDefault();
            void controller.finishLaunchAuthorization(loginCode);
          }}
        >
          <p class="modal-intro">{t($language, 'loginDescription')}</p>
          {#if account?.workspaceLabel}<p class="help">
              {t($language, 'claudeLoginOriginalWorkspace', {
                workspace: account.workspaceLabel,
              })}
            </p>{/if}
          <ol class="login-steps">
            <li><span>1</span>{t($language, 'loginStepOne')}</li>
            <li>
              <span>2</span>{t($language, 'loginStepTwo')} <code>#state</code>.
            </li>
            <li><span>3</span>{t($language, 'launchStepThree')}</li>
          </ol>
          {#if launch.working}<p class="working-message" role="status">
              <span class="spinner" aria-hidden="true"></span>{launch.session
                ? t($language, 'launchAuthorizing')
                : t($language, 'openingBrowser')}
            </p>{/if}
          <label class="field-label" for={`${uid}-code`}
            >{t($language, 'authorizationCode')}</label
          ><input
            id={`${uid}-code`}
            class="launch-code"
            type="password"
            autocomplete="off"
            spellcheck="false"
            placeholder="code#state"
            data-autofocus
            bind:value={loginCode}
            disabled={!launch.session || launch.working}
          />
          {#if launch.error}<p class="warning-text" role="alert">
              {safeError(launch.error, $language)}
            </p>{:else if unavailable}<p class="warning-text">
              {unavailable}
            </p>{/if}
          {#if launch.error || !launch.session}<div class="launch-retry">
              <button
                type="button"
                disabled={launch.working || !!unavailable}
                onclick={() => void controller.beginLaunchAuthorization()}
                >{t($language, 'openBrowserAgain')}</button
              ><button
                type="button"
                disabled={launch.working}
                onclick={() => void controller.backToLaunch()}
                >{t($language, 'launchBack')}</button
              >
            </div>{/if}
          <div class="modal-actions">
            <button type="button" onclick={cancel}
              >{t($language, 'cancel')}</button
            ><button
              class="primary"
              type="submit"
              disabled={!launch.session ||
                launch.working ||
                !!unavailable ||
                !loginCode.trim()}>{t($language, 'launchAuthorize')}</button
            >
          </div>
        </form>
      {/if}
    </div>
  </Modal>
{/if}

<style>
  .launch-account {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 14px 0 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .launch-account span {
    color: var(--subtle);
    font-size: 0.78rem;
  }
  .launch-model {
    width: 100%;
    margin-bottom: 12px;
  }
  .launch-code {
    width: 100%;
    margin-bottom: 12px;
  }
  .launch-retry {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 4px 0 12px;
  }
</style>
