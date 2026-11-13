<script lang="ts">
  import { language, t } from '../lib/i18n';
  import type { ResetRowView } from '../lib/claude-resets';
  import Modal from './Modal.svelte';
  let {
    name,
    row,
    working,
    disabled,
    error = null,
    returnFocus = null,
    onconfirm,
    oncancel,
  }: {
    /** The account the reset is used on. */
    name: string;
    row: ResetRowView;
    /** The request is running. */
    working: boolean;
    disabled: boolean;
    error?: string | null;
    returnFocus?: HTMLElement | null;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();
</script>

<Modal
  title={t($language, 'resetConfirmTitle')}
  {returnFocus}
  onclose={() => {
    if (!working) oncancel();
  }}
>
  <p class="modal-intro">
    <strong>{row.label}</strong> · {t($language, 'resetConfirmAccount', {
      name,
    })}<br />{row.effect}<br />{row.deadline === null
      ? t($language, 'resetConfirmDetailNoDate', { left: row.resetsLeft })
      : t($language, 'resetConfirmDetail', {
          left: row.resetsLeft,
          date: row.deadline,
        })}
  </p>
  {#if row.retry}<p class="help">{t($language, 'resetConfirmRetry')}</p>{/if}
  {#if row.early}<p class="warning-text" role="alert">{row.early.text}</p>{/if}
  {#if error}<p class="warning-text" role="alert">{error}</p>{/if}
  <div class="modal-actions">
    <button disabled={working} onclick={oncancel}
      >{t($language, 'resetConfirmNo')}</button
    ><button class="primary" disabled={disabled || working} onclick={onconfirm}
      >{working
        ? t($language, 'resetUsing')
        : t($language, 'resetConfirmYes')}</button
    >
  </div>
</Modal>
