<script lang="ts">
  import { language, t } from '../lib/i18n';

  let {
    working,
    disabled,
    describedBy,
    cancelLabel,
    onconfirm,
    oncancel,
  }: {
    working: boolean;
    disabled: boolean;
    describedBy?: string;
    cancelLabel?: string;
    onconfirm: (confirmation: string) => void;
    oncancel: () => void;
  } = $props();

  const keyword = 'RESET';
  let answer = $state('');
  let canConfirm = $derived(!disabled && !working && answer === keyword);
</script>

<label class="reset-confirmation">
  <span>{t($language, 'resetConfirmType', { keyword })}</span>
  <input
    type="text"
    bind:value={answer}
    disabled={disabled || working}
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
  />
</label>
<div class="modal-actions">
  <button type="button" disabled={working} onclick={oncancel}
    >{cancelLabel ?? t($language, 'cancel')}</button
  >
  <button
    type="button"
    class="primary"
    disabled={!canConfirm}
    aria-describedby={describedBy}
    onclick={() => {
      if (!canConfirm) return;
      const confirmation = answer;
      answer = '';
      onconfirm(confirmation);
    }}>{working ? t($language, 'resetUsing') : 'OK'}</button
  >
</div>

<style>
  .reset-confirmation {
    display: grid;
    gap: 8px;
    margin-top: 16px;
    font-size: 0.84rem;
  }
  .reset-confirmation input {
    min-width: 0;
  }
</style>
