<script lang="ts">
  import { t } from '../lib/i18n';
  import { untrack } from 'svelte';
  import { settingsSchema, type Settings } from '../lib/types';
  import Modal from './Modal.svelte';
  import { safeError } from '../lib/controller';
  import { percentage } from '../lib/format';
  let {
    settings,
    disabled,
    onsave,
    onclose,
    error = null,
  }: {
    settings: Settings;
    disabled: boolean;
    onsave: (settings: Settings) => Promise<boolean>;
    onclose: () => void;
    error?: string | null;
  } = $props();
  let draft = $state<Settings>({ ...untrack(() => settings) });
  let saving = $state(false);
  let locale = $derived(draft.language);
  let validationError = $state<string | null>(null);
  async function save() {
    validationError = null;
    const result = settingsSchema.safeParse(draft);
    if (!result.success) {
      validationError = t(locale, 'invalidSettings');
      return;
    }
    saving = true;
    if (await onsave(result.data)) onclose();
    saving = false;
  }
</script>

<Modal title={t(locale, 'settings')} {locale} {onclose}>
  <form
    novalidate
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
  >
    <p class="modal-intro">{t(locale, 'settingsDescription')}</p>
    <fieldset disabled={disabled || saving}>
      <legend class="sr-only">{t(locale, 'automation')}</legend>
      <label class="toggle-row"
        ><span
          ><strong>{t(locale, 'automaticSwitch')}</strong><small
            >{t(locale, 'autoSwitchHelp')}</small
          ></span
        ><input
          type="checkbox"
          role="switch"
          bind:checked={draft.autoSwitchEnabled}
        /></label
      >
      <label class="toggle-row"
        ><span
          ><strong>{t(locale, 'primeWindow')}</strong><small
            >{t(locale, 'primeHelp')}</small
          ></span
        ><input
          type="checkbox"
          role="switch"
          bind:checked={draft.autoStartWindowEnabled}
        /></label
      >
      <label class="toggle-row"
        ><span
          ><strong>{t(locale, 'autoResets')}</strong><small
            >{t(locale, 'resetsHelp')}</small
          ></span
        ><input
          type="checkbox"
          role="switch"
          bind:checked={draft.autoUseResetsEnabled}
        /></label
      >
      <div class="settings-control">
        <label for="poll"
          >{t(locale, 'pollInterval')}<strong
            >{t(locale, 'minutes', {
              count: percentage(draft.pollInterval / 60, locale),
            })}</strong
          ></label
        >
        <input
          id="poll"
          type="range"
          min="120"
          max="900"
          step="30"
          bind:value={draft.pollInterval}
        />
        <div class="range-labels">
          <span>{t(locale, 'minutes', { count: 2 })}</span><span
            >{t(locale, 'minutes', { count: 15 })}</span
          >
        </div>
      </div>
      <p class="help">{t(locale, 'pollingHelp')}</p>
      <div class="settings-control">
        <label for="threshold"
          >{t(locale, 'switchThreshold')}<strong
            >{percentage(draft.threshold, locale)}%</strong
          ></label
        >
        <input
          id="threshold"
          type="number"
          min="50"
          max="100"
          step="1"
          required
          bind:value={draft.threshold}
        />
      </div>
      <div class="settings-control">
        <label for="appearance">{t(locale, 'appearance')}</label><select
          id="appearance"
          bind:value={draft.appearance}
          ><option value="system">{t(locale, 'system')}</option><option
            value="light">{t(locale, 'light')}</option
          ><option value="dark">{t(locale, 'dark')}</option></select
        >
      </div>
      <div class="settings-control">
        <label for="language">{t(locale, 'language')}</label><select
          id="language"
          bind:value={draft.language}
          ><option value="en">English</option><option value="ro">Română</option
          ></select
        >
      </div>
    </fieldset>
    {#if validationError || error}<p class="warning-text" role="alert">
        {safeError(validationError ?? error, locale)}
      </p>{/if}
    <div class="modal-actions">
      <button type="button" onclick={onclose}>{t(locale, 'cancel')}</button
      ><button class="primary" type="submit" disabled={disabled || saving}
        >{saving ? t(locale, 'saving') : t(locale, 'save')}</button
      >
    </div>
  </form>
</Modal>
