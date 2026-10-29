<script lang="ts">
  import { language, t } from '../lib/i18n';
  import Modal from './Modal.svelte';
  let {
    pending,
    disabled,
    error = null,
    warning = null,
    returnFocus = null,
    onconfirm,
    oncancel,
  }: {
    /** The native save is running. */
    pending: boolean;
    disabled: boolean;
    error?: string | null;
    /** An extra warning for this key, shown under the billing text. */
    warning?: string | null;
    returnFocus?: HTMLElement | null;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();
</script>

<Modal
  title={t($language, 'codexFallbackConfirmTitle')}
  {returnFocus}
  onclose={oncancel}
  ><p class="modal-intro">{t($language, 'codexFallbackConfirmText')}</p>
  {#if warning}<p class="warning-text">{warning}</p>{/if}
  {#if error}<p class="warning-text" role="alert">{error}</p>{/if}
  <div class="modal-actions">
    <button disabled={pending} onclick={oncancel}
      >{t($language, 'cancel')}</button
    ><button class="primary" {disabled} onclick={onconfirm}
      >{pending
        ? t($language, 'saving')
        : t($language, 'codexFallbackConfirmAction')}</button
    >
  </div></Modal
>
