<script lang="ts">
  import { language, t } from '../lib/i18n';
  import Modal from './Modal.svelte';
  let {
    text,
    pending,
    disabled,
    error = null,
    returnFocus = null,
    onconfirm,
    oncancel,
  }: {
    /** What deleting this account changes, in the provider's words. */
    text: string;
    pending: boolean;
    disabled: boolean;
    error?: string | null;
    returnFocus?: HTMLElement | null;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();
</script>

<Modal title={t($language, 'deleteTitle')} {returnFocus} onclose={oncancel}
  ><p class="modal-intro">{text}</p>
  {#if error}<p class="warning-text" role="alert">{error}</p>{/if}
  <div class="modal-actions">
    <button disabled={pending} onclick={oncancel}
      >{t($language, 'cancel')}</button
    ><button class="danger-button" {disabled} onclick={onconfirm}
      >{pending
        ? t($language, 'deleting')
        : t($language, 'deleteAccount')}</button
    >
  </div></Modal
>
