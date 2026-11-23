<script lang="ts">
  import { language, t } from '../lib/i18n';
  import type { ResetRowView } from '../lib/claude-resets';
  import Modal from './Modal.svelte';
  import ResetConfirmation from './ResetConfirmation.svelte';
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
    row: ResetRowView | null;
    /** The request is running. */
    working: boolean;
    disabled: boolean;
    error?: string | null;
    returnFocus?: HTMLElement | null;
    onconfirm: (confirmation: string) => void;
    oncancel: () => void;
  } = $props();
</script>

<Modal
  title={t($language, row ? 'resetConfirmTitle' : 'resetConfirmRetryTitle')}
  {returnFocus}
  onclose={() => {
    if (!working) oncancel();
  }}
>
  <p class="modal-intro">
    {#if row}<strong>{row.label}</strong> · {t(
        $language,
        'resetConfirmAccount',
        {
          name,
        },
      )}<br />{row.effect}<br />{row.deadline === null
        ? t($language, 'resetConfirmDetailNoDate', { left: row.resetsLeft })
        : t($language, 'resetConfirmDetail', {
            left: row.resetsLeft,
            date: row.deadline,
          })}{:else}
      {t($language, 'resetConfirmAccount', { name })}
    {/if}
  </p>
  {#if !row || row.retry}<p class="help">
      {t($language, 'resetConfirmRetry')}
    </p>{/if}
  {#if row?.early}<p class="warning-text" role="alert">{row.early.text}</p>{/if}
  {#if error}<p class="warning-text" role="alert">{error}</p>{/if}
  <ResetConfirmation
    {working}
    disabled={disabled || row?.canUse === false}
    cancelLabel={t($language, 'resetConfirmNo')}
    {onconfirm}
    {oncancel}
  />
</Modal>
