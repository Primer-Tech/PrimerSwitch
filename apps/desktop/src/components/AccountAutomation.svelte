<script lang="ts">
  import { untrack } from 'svelte';
  import { language, t } from '../lib/i18n';
  import {
    accountAutomationSchema,
    type AccountAutomation,
  } from '../lib/types';
  import { exactPercent, parsePercent, parsePriority } from '../lib/automation';
  import PercentField from './PercentField.svelte';
  let {
    automation,
    providerDefault,
    locked,
    supported = true,
    error = null,
    onsave,
  }: {
    automation: AccountAutomation;
    providerDefault: number;
    locked: boolean;
    supported?: boolean;
    error?: string | null;
    onsave: (
      automation: AccountAutomation,
    ) => Promise<AccountAutomation | null>;
  } = $props();
  const uid = $props.id();
  let draft = $state({ ...untrack(() => automation) });
  let custom = $state(untrack(() => automation.threshold !== null));
  let percent = $state(
    exactPercent(
      untrack(() => automation.threshold ?? providerDefault),
      untrack(() => $language),
    ),
  );
  let priority = $state(String(untrack(() => automation.priority ?? '')));
  let attempted = $state(false);
  let saving = $state(false);
  let saveFailed = $state(false);
  let saved = $state<AccountAutomation | null>(null);
  let priorityValue = $derived(parsePriority(priority));
  let disabled = $derived(locked || saving || !supported);
  function reset(value: AccountAutomation = automation) {
    draft = { ...value };
    custom = value.threshold !== null;
    percent = exactPercent(value.threshold ?? providerDefault, $language);
    priority = String(value.priority ?? '');
    attempted = false;
    saveFailed = false;
  }
  async function save() {
    attempted = true;
    saveFailed = false;
    saved = null;
    const threshold = custom ? parsePercent(percent) : null;
    if (custom && threshold === null) {
      document.getElementById(`${uid}-threshold`)?.focus();
      return;
    }
    if (
      priorityValue === undefined ||
      (draft.enabled && draft.returnOnFiveHourReset && priorityValue === null)
    ) {
      document.getElementById(`${uid}-priority`)?.focus();
      return;
    }
    const parsed = accountAutomationSchema.safeParse({
      ...draft,
      threshold,
      priority: priorityValue,
    });
    if (!parsed.success || disabled) return;
    saving = true;
    try {
      const result = await onsave(parsed.data);
      if (result) {
        reset(result);
        saved = result;
      } else saveFailed = true;
    } finally {
      saving = false;
    }
  }
</script>

<section class="account-automation" aria-labelledby={`${uid}-heading`}>
  <h3 id={`${uid}-heading`}>{t($language, 'accountAutomation')}</h3>
  <p class="help">{t($language, 'accountAutomationHelp')}</p>
  {#if !supported}<p class="help">
      {t($language, 'automationUnsupported')}
    </p>{/if}
  <form
    novalidate
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <fieldset {disabled}>
      <legend class="sr-only">{t($language, 'accountAutomation')}</legend>
      <label class="toggle-row">
        <span
          ><strong>{t($language, 'includeAutomatic')}</strong><small
            >{t($language, 'includeAutomaticHelp')}</small
          ></span
        >
        <input
          type="checkbox"
          role="switch"
          checked={draft.enabled}
          onchange={(event) => {
            draft.enabled = event.currentTarget.checked;
            saved = null;
          }}
        />
      </label>
      <div class="settings-control">
        <label for={`${uid}-mode`}>{t($language, 'switchThresholdUsed')}</label>
        <select id={`${uid}-mode`} bind:value={custom}>
          <option value={false}
            >{t($language, 'useProviderDefault', {
              threshold: exactPercent(providerDefault, $language),
            })}</option
          >
          <option value={true}>{t($language, 'customThreshold')}</option>
        </select>
      </div>
      {#if custom}<PercentField
          id={`${uid}-threshold`}
          label={t($language, 'customThresholdUsed')}
          bind:value={percent}
          locale={$language}
          {disabled}
          showError={attempted}
        />{/if}
      <div class="settings-control">
        <label for={`${uid}-priority`}>{t($language, 'accountPriority')}</label>
        <input
          id={`${uid}-priority`}
          class="priority-input"
          type="text"
          inputmode="numeric"
          autocomplete="off"
          bind:value={priority}
          aria-invalid={attempted &&
            (priorityValue === undefined ||
              (draft.enabled &&
                draft.returnOnFiveHourReset &&
                priorityValue === null))}
          aria-describedby={`${uid}-priority-help`}
        />
        <p class="help" id={`${uid}-priority-help`}>
          {t($language, 'accountPriorityHelp')}
        </p>
        {#if attempted && priorityValue === undefined}<p
            class="warning-text"
            role="alert"
          >
            {t($language, 'invalidPriority')}
          </p>{/if}
        {#if attempted && draft.enabled && draft.returnOnFiveHourReset && priorityValue === null}<p
            class="warning-text"
            role="alert"
          >
            {t($language, 'returnPriorityRequired')}
          </p>{/if}
      </div>
      <label class="toggle-row">
        <span
          ><strong>{t($language, 'returnAfterReset')}</strong><small
            >{t($language, 'returnAfterResetHelp')}</small
          ></span
        >
        <input
          type="checkbox"
          role="switch"
          bind:checked={draft.returnOnFiveHourReset}
          disabled={disabled ||
            ((!draft.enabled || priorityValue == null) &&
              !draft.returnOnFiveHourReset)}
        />
      </label>
      {#if draft.returnOnFiveHourReset && !draft.enabled}<p class="help">
          {t($language, 'returnExcludedHelp')}
        </p>{/if}
    </fieldset>
    <p class="help">{t($language, 'manualReturnHelp')}</p>
    {#if saveFailed}<p class="warning-text" role="alert">
        {error ?? t($language, 'failedAction')}
      </p>{/if}
    {#if saved}<p class="saved-value" role="status">
        {t($language, 'accountAutomationSaved', {
          threshold: exactPercent(
            saved.threshold ?? providerDefault,
            $language,
          ),
          source: t(
            $language,
            saved.threshold === null ? 'providerDefault' : 'customThreshold',
          ),
          priority: saved.priority ?? t($language, 'priorityAutomatic'),
          state: t(
            $language,
            saved.enabled ? 'includedAutomatic' : 'excludedAutomatic',
          ),
          returnState: t($language, saved.returnOnFiveHourReset ? 'on' : 'off'),
        })}
      </p>{/if}
    <div class="modal-actions sticky-actions">
      <button
        type="button"
        {disabled}
        onclick={() => {
          reset();
          saved = null;
        }}>{t($language, 'cancel')}</button
      >
      <button class="primary" type="submit" {disabled}
        >{t($language, saving ? 'saving' : 'saveAutomation')}</button
      >
    </div>
  </form>
</section>

<style>
  .account-automation {
    margin-top: 20px;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }
  h3 {
    font-size: 0.84rem;
    margin: 0 0 8px;
  }
  .priority-input {
    width: min(100%, 120px);
  }
  .saved-value {
    font-size: 0.75rem;
    line-height: 1.6;
    color: var(--green);
    margin: 12px 0 0;
  }
  .modal-actions {
    margin-top: 14px;
  }
</style>
