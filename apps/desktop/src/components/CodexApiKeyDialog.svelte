<script lang="ts">
  import { tick } from 'svelte';
  import { language, t } from '../lib/i18n';
  import { codexMessage } from '../lib/codex-format';
  import type { CodexError } from '../lib/codex-controller';
  import Modal from './Modal.svelte';
  let {
    pending,
    error = null,
    returnFocus = null,
    onsubmit,
    oncancel,
  }: {
    /** The native save is running. */
    pending: boolean;
    /** Why the last save failed, as a stable reason; it never carries the key. */
    error?: CodexError | null;
    returnFocus?: HTMLElement | null;
    /** Resolves true once the key is saved. */
    onsubmit: (name: string, key: string) => Promise<boolean>;
    oncancel: () => void;
  } = $props();
  const uid = $props.id();
  let name = $state('');
  let key = $state('');
  let working = $state(false);
  let nameInput = $state<HTMLInputElement | null>(null);
  let keyInput = $state<HTMLInputElement | null>(null);
  let canSave = $derived(!!name.trim() && !!key.trim() && !pending && !working);
  $effect(() => {
    nameInput?.focus();
  });
  async function submit() {
    if (!canSave) return;
    // The key lives in this field only. It is emptied before anything else happens and
    // again when the call ends, whatever the outcome; a failed save never keeps it.
    const typed = key.trim();
    key = '';
    working = true;
    try {
      await onsubmit(name.trim(), typed);
    } finally {
      key = '';
      working = false;
    }
    await tick();
    keyInput?.focus();
  }
  function close() {
    key = '';
    oncancel();
  }
</script>

<Modal title={t($language, 'codexApiKeyTitle')} {returnFocus} onclose={close}>
  <form
    class="api-key-form"
    onsubmit={(event) => {
      event.preventDefault();
      void submit();
    }}
  >
    <p class="modal-intro">{t($language, 'codexApiKeyDescription')}</p>
    <label class="field-label" for={`${uid}-name`}
      >{t($language, 'codexApiKeyNameLabel')}</label
    ><input
      id={`${uid}-name`}
      type="text"
      autocomplete="off"
      spellcheck="false"
      maxlength="60"
      required
      bind:this={nameInput}
      bind:value={name}
      disabled={working}
    />
    <label class="field-label" for={`${uid}-key`}
      >{t($language, 'codexApiKeyLabel')}</label
    ><input
      id={`${uid}-key`}
      type="password"
      autocomplete="off"
      spellcheck="false"
      required
      aria-describedby={`${uid}-help`}
      bind:this={keyInput}
      bind:value={key}
      disabled={working}
    />
    <p class="help" id={`${uid}-help`}>{t($language, 'codexApiKeyHelp')}</p>
    {#if error}<p class="warning-text" role="alert">
        {codexMessage(error, $language)}
      </p>{/if}
    <div class="modal-actions">
      <button type="button" onclick={close}>{t($language, 'cancel')}</button
      ><button class="primary" type="submit" disabled={!canSave}
        >{pending || working
          ? t($language, 'saving')
          : t($language, 'codexApiKeySave')}</button
      >
    </div>
  </form>
</Modal>

<style>
  .api-key-form input {
    width: 100%;
    margin-bottom: 14px;
  }
  .api-key-form .help {
    margin: -4px 0 0;
  }
  .api-key-form .warning-text {
    margin-top: 12px;
  }
</style>
