<script lang="ts">
  import { language, t, type Language } from '../lib/i18n';
  import { untrack, type Snippet } from 'svelte';
  let {
    title,
    onclose,
    children,
    locale,
    returnFocus = null,
  }: {
    title: string;
    onclose: () => void;
    children: Snippet;
    locale?: Language;
    returnFocus?: HTMLElement | null;
  } = $props();
  let dialog: HTMLDialogElement;
  const uid = $props.id();
  function close() {
    // Close the native dialog before Svelte removes it. WebKitGTK otherwise
    // can retain the document's modal inert state for the next keyboard event.
    if (dialog?.open) dialog.close();
    onclose();
  }
  $effect(() => {
    const previous = untrack(() => returnFocus) ?? document.activeElement;
    dialog.showModal();
    return () => {
      dialog.close();
      // Svelte removes the dialog during teardown; restore focus after removal.
      queueMicrotask(() => {
        if (previous instanceof HTMLElement && previous.isConnected)
          previous.focus();
      });
    };
  });
</script>

<dialog
  bind:this={dialog}
  aria-labelledby={uid}
  oncancel={(event) => {
    event.preventDefault();
    close();
  }}
>
  <div class="modal-header">
    <h2 id={uid}>{title}</h2>
    <button
      class="icon-button"
      aria-label={t(locale ?? $language, 'close')}
      onclick={close}>×</button
    >
  </div>
  {@render children()}
</dialog>
