<script lang="ts">
  import { tick } from 'svelte';
  import { language, t } from '../lib/i18n';
  let {
    active,
    onselect,
    disabled = false,
  }: {
    active: 'claude' | 'codex';
    onselect: (provider: 'claude' | 'codex') => void;
    disabled?: boolean;
  } = $props();
  async function select(provider: 'claude' | 'codex') {
    if (disabled) return;
    onselect(provider);
    await tick();
    document.getElementById('provider-' + provider)?.focus();
  }
  function keyboard(event: KeyboardEvent) {
    if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      void select(
        event.key === 'Home'
          ? 'claude'
          : event.key === 'End'
            ? 'codex'
            : active === 'claude'
              ? 'codex'
              : 'claude',
      );
    }
  }
</script>

<div
  class="providers provider-tabs"
  role="tablist"
  tabindex="-1"
  aria-label={t($language, 'providers')}
>
  <button
    id="provider-claude"
    class:provider-tab={active === 'claude'}
    role="tab"
    aria-selected={active === 'claude'}
    aria-controls="provider-content"
    tabindex={active === 'claude' ? 0 : -1}
    {disabled}
    onkeydown={keyboard}
    onclick={() => select('claude')}>✳ Claude</button
  >
  <button
    id="provider-codex"
    class:provider-tab={active === 'codex'}
    role="tab"
    aria-selected={active === 'codex'}
    aria-controls="provider-content"
    tabindex={active === 'codex' ? 0 : -1}
    {disabled}
    onkeydown={keyboard}
    onclick={() => select('codex')}>Codex</button
  >
</div>
