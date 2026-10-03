<script lang="ts">
  import { language, t } from '../lib/i18n';
  import type { NoticeView } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  let { notices }: { notices: NoticeView[] } = $props();
  const icons: Record<NoticeView['tone'], string | null> = {
    progress: null,
    error: 'warning',
    success: 'check',
    warn: 'warning',
    info: 'info',
    demo: null,
  };
</script>

{#if notices.length}<div class="notices">
    {#each notices as notice (notice.key)}
      <div
        class="notice"
        data-tone={notice.tone}
        role={notice.role}
        id={notice.id}
        tabindex="-1"
        aria-label={notice.label}
      >
        {#if notice.tone === 'progress'}<span
            class="spinner"
            class:ring={!!notice.steps}
            aria-hidden="true"
          ></span>{:else if icons[notice.tone]}<span
            class="notice-icon"
            aria-hidden="true"><Icon name={icons[notice.tone]!} /></span
          >{/if}
        <div class="notice-body">
          <p class="notice-text">
            {#if notice.title}<strong>{notice.title}</strong>{' '}{/if}<span
              >{notice.text}</span
            >
          </p>
          {#if notice.items?.length}<ul class="notice-items">
              {#each notice.items as item (item)}<li>{item}</li>{/each}
            </ul>{/if}
          {#each notice.lines ?? [] as line, index (index)}<p
              class="notice-line"
              class:warn={line.tone === 'warn'}
            >
              {line.text}
            </p>{/each}
          {#if notice.steps}<ol class="notice-steps" aria-hidden="true">
              {#each notice.steps as step (step.label)}<li
                  data-state={step.state}
                >
                  <span class="marker"></span>{step.label}
                </li>{/each}
            </ol>{/if}
        </div>
        {#if notice.elapsed}<span class="elapsed" aria-hidden="true"
            >{notice.elapsed}</span
          >{/if}
        {#if notice.action}{@const action = notice.action}<button
            class="notice-action"
            disabled={action.disabled}
            onclick={(event) => action.run(event.currentTarget)}
            >{action.label}</button
          >{/if}
        {#if notice.dismiss}<button
            class="icon-button notice-dismiss"
            aria-label={t($language, 'dismissMessage')}
            onclick={notice.dismiss}>×</button
          >{/if}
      </div>
    {/each}
  </div>{/if}

<style>
  .notices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 12px;
  }
  .notice {
    display: flex;
    align-items: center;
    gap: 11px;
    min-height: 38px;
    padding: 6px 8px 6px 13px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--surface);
    font-size: 0.78rem;
    line-height: 1.5;
    color: var(--text);
  }
  .notice:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .notice-icon {
    display: inline-flex;
    flex: none;
  }
  .notice-body {
    flex: 1;
    min-width: 0;
  }
  .notice-text,
  .notice-line {
    margin: 0;
  }
  .notice-text strong {
    font-weight: 650;
  }
  .notice-line {
    color: var(--subtle);
    margin-top: 2px;
  }
  .notice-line.warn {
    color: var(--warn);
  }
  .notice-items {
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--subtle);
  }
  .notice-action {
    flex: none;
    font-size: 0.75rem;
    min-height: 30px;
    padding: 0.35rem 0.75rem;
  }
  .notice-dismiss {
    flex: none;
    align-self: center;
    margin: -4px -2px;
  }
  .elapsed {
    flex: none;
    align-self: flex-start;
    margin-top: 1px;
    color: var(--subtle);
    font-size: 0.74rem;
    font-variant-numeric: tabular-nums;
  }
  .spinner.ring {
    width: 18px;
    height: 18px;
    border-width: 2.5px;
    align-self: flex-start;
    margin-top: 1px;
  }
  [data-tone='progress'] {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--line));
    background:
      linear-gradient(
        100deg,
        color-mix(in srgb, var(--brand) 14%, transparent),
        transparent 70%
      ),
      var(--surface);
  }
  [data-tone='error'] {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 40%, var(--line));
    background: color-mix(in srgb, var(--danger) 6%, var(--surface));
  }
  [data-tone='success'] {
    border-color: color-mix(in srgb, var(--green) 40%, var(--line));
    background: color-mix(in srgb, var(--green) 6%, var(--surface));
  }
  [data-tone='success'] .notice-icon {
    color: var(--green);
  }
  [data-tone='warn'] {
    border-color: color-mix(in srgb, var(--warn) 45%, var(--line));
    background: color-mix(in srgb, var(--warn) 7%, var(--surface));
  }
  [data-tone='warn'] .notice-icon,
  [data-tone='warn'] .notice-text strong {
    color: var(--warn);
  }
  [data-tone='info'] {
    border-color: color-mix(in srgb, var(--accent) 35%, var(--line));
  }
  [data-tone='info'] .notice-icon {
    color: var(--accent);
  }
  [data-tone='demo'] .notice-text strong {
    color: var(--accent);
  }
  [data-tone='demo'] .notice-text span {
    color: var(--subtle);
  }
  .notice-steps {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 20px;
    list-style: none;
    margin: 7px 0 2px;
    padding: 0;
    font-size: 0.72rem;
    color: var(--subtle);
  }
  .notice-steps li {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .marker {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: 1.5px solid var(--subtle);
  }
  [data-state='done'] {
    color: var(--text);
  }
  [data-state='done'] .marker {
    background: var(--green);
    border-color: var(--green);
  }
  [data-state='current'] {
    color: var(--accent);
    font-weight: 600;
  }
  [data-state='current'] .marker {
    border-color: var(--accent);
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent);
  }
  @media (max-width: 560px) {
    .notice {
      flex-wrap: wrap;
    }
    .notice-body {
      flex-basis: calc(100% - 64px);
    }
    /* The dismiss button stays beside the text; the action moves below it. */
    .notice-dismiss {
      order: 1;
      align-self: flex-start;
    }
    .notice-action {
      order: 2;
      margin-left: 29px;
    }
  }
</style>
