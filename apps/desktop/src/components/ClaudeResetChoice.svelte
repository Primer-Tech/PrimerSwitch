<script lang="ts">
  import { language, t } from '../lib/i18n';
  import type { ResetRowView } from '../lib/claude-resets';
  import type { ResetKind } from '../lib/types';
  import Modal from './Modal.svelte';
  let {
    name,
    rows,
    working,
    disabled,
    error = null,
    returnFocus = null,
    onconfirm,
    oncancel,
  }: {
    /** The account the reset is used on. */
    name: string;
    /** The kinds that can be used now. Kinds that cannot are not offered. */
    rows: ResetRowView[];
    /** The request is running. */
    working: boolean;
    disabled: boolean;
    error?: string | null;
    returnFocus?: HTMLElement | null;
    onconfirm: (kind: ResetKind) => void;
    oncancel: () => void;
  } = $props();
  const uid = $props.id();
  // Nothing is chosen at first: a wrong pick would spend a scarce reset.
  let picked = $state<ResetKind | null>(null);
  // A kind that is no longer offered cannot stay chosen.
  let pickedRow = $derived(rows.find((row) => row.kind === picked) ?? null);
</script>

<Modal
  title={t($language, 'resetChooseTitle')}
  {returnFocus}
  onclose={() => {
    if (!working) oncancel();
  }}
>
  <p class="modal-intro choice-intro">
    {t($language, 'resetConfirmAccount', { name })}
  </p>
  <div
    class="reset-choices"
    role="radiogroup"
    aria-label={t($language, 'resetChooseTitle')}
  >
    {#each rows as row (row.kind)}
      <label class="reset-choice" class:picked={picked === row.kind}>
        <input
          type="radio"
          name={`${uid}-kind`}
          value={row.kind}
          bind:group={picked}
          disabled={disabled || working}
          aria-labelledby={`${uid}-${row.kind}-name`}
          aria-describedby={`${uid}-${row.kind}-left ${uid}-${row.kind}-effect ${uid}-${row.kind}-by`}
        />
        <span class="choice-text">
          <span class="choice-head"
            ><strong id={`${uid}-${row.kind}-name`}>{row.label}</strong><span
              class="choice-left"
              id={`${uid}-${row.kind}-left`}>{row.left}</span
            ></span
          >
          <span class="help choice-detail" id={`${uid}-${row.kind}-effect`}
            >{row.effect}</span
          >
          <span class="help choice-detail" id={`${uid}-${row.kind}-by`}
            >{row.useBy}</span
          >
        </span>
      </label>
    {/each}
  </div>
  {#if !pickedRow}<p class="help choice-prompt" id={`${uid}-prompt`}>
      {t($language, 'resetChoosePrompt')}
    </p>{/if}
  {#if pickedRow?.retry}<p class="help choice-note">
      {t($language, 'resetConfirmRetry')}
    </p>{/if}
  {#if pickedRow?.early}<p class="warning-text choice-note" role="alert">
      {pickedRow.early.text}
    </p>{/if}
  {#if error}<p class="warning-text choice-note" role="alert">{error}</p>{/if}
  <div class="modal-actions">
    <button disabled={working} onclick={oncancel}
      >{t($language, 'resetConfirmNo')}</button
    ><button
      class="primary"
      disabled={!pickedRow || disabled || working}
      aria-describedby={pickedRow ? undefined : `${uid}-prompt`}
      onclick={() => {
        if (pickedRow && !disabled && !working) onconfirm(pickedRow.kind);
      }}
      >{working
        ? t($language, 'resetUsing')
        : t($language, 'resetConfirmYes')}</button
    >
  </div>
</Modal>

<style>
  .choice-intro {
    margin-bottom: 12px;
  }
  .reset-choices {
    display: grid;
    gap: 10px;
  }
  .reset-choice {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-raised);
    cursor: pointer;
  }
  .reset-choice:hover {
    background: var(--surface-hover);
  }
  .reset-choice.picked {
    border-color: var(--accent);
    background: var(--accent-dark);
  }
  .reset-choice:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .reset-choice input[type='radio'] {
    appearance: auto;
    flex: none;
    width: 16px;
    height: 16px;
    margin: 3px 0 0;
    padding: 0;
    border: 0;
    background: transparent;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .reset-choice input[type='radio']:focus-visible {
    outline: none;
  }
  .choice-text {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .choice-head {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    font-size: 0.84rem;
  }
  .choice-left {
    color: var(--subtle);
  }
  .choice-detail {
    overflow-wrap: anywhere;
  }
  .choice-prompt,
  .choice-note {
    margin-top: 12px;
  }
</style>
