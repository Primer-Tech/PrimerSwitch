<script lang="ts">
  import { t, type Language } from '../lib/i18n';
  import { parsePercent, stepPercent } from '../lib/automation';
  let {
    id,
    label,
    value = $bindable('95'),
    locale = 'en',
    disabled = false,
    showError = false,
  }: {
    id: string;
    label: string;
    value?: string;
    locale?: Language;
    disabled?: boolean;
    showError?: boolean;
  } = $props();
  let invalid = $derived(parsePercent(value) === null);
  function step(direction: -1 | 1) {
    const next = stepPercent(value, direction);
    if (next !== null) value = next;
  }
</script>

<div class="settings-control percent-control">
  <label for={id}>{label}</label>
  <div class="percent-input">
    <button
      type="button"
      disabled={disabled || stepPercent(value, -1) === null}
      aria-label={t(locale, 'decreasePercent', { label })}
      onclick={() => step(-1)}>−</button
    >
    <div class="percent-value">
      <input
        {id}
        type="text"
        inputmode="decimal"
        autocomplete="off"
        spellcheck="false"
        bind:value
        {disabled}
        aria-invalid={showError && invalid}
        aria-describedby={`${id}-help${showError && invalid ? ` ${id}-error` : ''}`}
      />
      <span aria-hidden="true">%</span>
    </div>
    <button
      type="button"
      disabled={disabled || stepPercent(value, 1) === null}
      aria-label={t(locale, 'increasePercent', { label })}
      onclick={() => step(1)}>+</button
    >
  </div>
  <p class="help" id={`${id}-help`}>{t(locale, 'percentHelp')}</p>
  {#if showError && invalid}<p
      class="warning-text"
      id={`${id}-error`}
      role="alert"
    >
      {t(locale, 'invalidPercent')}
    </p>{/if}
</div>

<style>
  .percent-input {
    display: flex;
    align-items: stretch;
    gap: 6px;
    max-width: 230px;
  }
  .percent-value {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  .percent-value input {
    width: 100%;
    padding-right: 30px;
    font-variant-numeric: tabular-nums;
  }
  .percent-value span {
    position: absolute;
    right: 12px;
    top: 50%;
    transform: translateY(-50%);
    pointer-events: none;
    color: var(--subtle);
  }
  .percent-input button {
    flex: none;
    width: 36px;
    min-width: 36px;
    padding: 6px;
  }
  .percent-control .help {
    margin: 6px 0 0;
  }
</style>
