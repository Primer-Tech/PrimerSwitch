<script lang="ts">
  import { language, t } from '../lib/i18n';
  import type { AddOption } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  let {
    label,
    disabled,
    options,
  }: {
    label: string;
    disabled: boolean;
    options: AddOption[];
  } = $props();
  let open = $state(false);
  let trigger = $state<HTMLButtonElement | null>(null);
  let menu = $state<HTMLElement | null>(null);
  function choose(option: AddOption) {
    open = false;
    trigger?.focus();
    if (trigger) option.run(trigger);
  }
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && open) {
      event.preventDefault();
      open = false;
      trigger?.focus();
    }
  }
  function pointerdown(event: PointerEvent) {
    if (open && !(event.target instanceof Node && menu?.contains(event.target)))
      open = false;
  }
</script>

<svelte:window onkeydown={keydown} onpointerdown={pointerdown} />
<div
  class="add-menu"
  role="group"
  aria-label={t($language, 'addActions')}
  bind:this={menu}
>
  <button
    class="primary"
    bind:this={trigger}
    {disabled}
    aria-expanded={open}
    aria-controls="add-actions"
    onclick={() => (open = !open)}
    ><Icon name="plus" size={16} />{label}<Icon
      name="chevron"
      size={14}
    /></button
  >{#if open}<div class="add-options" id="add-actions">
      {#each options as option (option.key)}<button
          disabled={option.disabled}
          onclick={() => choose(option)}
          ><Icon name={option.icon} />{option.label}</button
        >{/each}
    </div>{/if}
</div>

<style>
  .add-menu {
    position: relative;
    z-index: 3;
  }
  .add-menu > button {
    font-size: 0.78rem;
    min-height: 32px;
    padding: 0.4rem 0.75rem;
  }
  .add-options {
    position: absolute;
    right: 0;
    top: calc(100% + 6px);
    width: max-content;
    max-width: calc(100vw - 40px);
    padding: 5px;
    border: 1px solid var(--line);
    background: var(--surface-raised);
    box-shadow: var(--shadow);
    border-radius: 8px;
    display: flex;
    flex-direction: column;
  }
  .add-options button {
    background: none;
    border: 0;
    justify-content: flex-start;
    gap: 9px;
    padding: 9px 10px;
    font-weight: 500;
  }
  .add-options button:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  @media (max-width: 560px) {
    .add-menu > button {
      font-size: 0.71rem;
      gap: 5px;
      padding: 7px;
    }
  }
</style>
