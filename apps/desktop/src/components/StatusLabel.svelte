<script lang="ts">
  import type { StatusView } from '../lib/provider-view';
  let {
    status,
    id,
  }: {
    status: StatusView;
    /** Lets a disabled Switch button point at this status as its reason. */
    id?: string;
  } = $props();
</script>

<span class="status" data-kind={status.kind} {id}>
  <span class="dot" aria-hidden="true"></span><span class="text"
    ><span class="label">{status.label}</span>{#if status.detail}<small
        title={status.title ?? undefined}
        class:explained={!!status.title}>{status.detail}</small
      >{/if}</span
  >
</span>

<style>
  .status {
    display: inline-flex;
    align-items: flex-start;
    gap: 8px;
    min-width: 0;
    font-size: 0.78rem;
    line-height: 1.35;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .label {
    font-weight: 600;
    color: var(--text);
  }
  small {
    font-size: 0.69rem;
    color: var(--subtle);
    overflow-wrap: anywhere;
  }
  small.explained {
    text-decoration: underline dotted;
    text-underline-offset: 3px;
    cursor: help;
  }
  .dot {
    width: 8px;
    height: 8px;
    margin-top: 0.36em;
    border-radius: 50%;
    flex: none;
    background: var(--green);
  }
  [data-kind='active'] .dot {
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  [data-kind='active'] .label {
    color: var(--accent);
  }
  [data-kind='next'] .label {
    color: var(--green);
  }
  [data-kind='limited'] .dot,
  [data-kind='signIn'] .dot,
  [data-kind='failed'] .dot {
    background: var(--danger);
  }
  [data-kind='limited'] .label,
  [data-kind='signIn'] .label,
  [data-kind='failed'] .label {
    color: var(--danger);
  }
  [data-kind='credits'] .dot,
  [data-kind='near'] .dot {
    background: var(--warn);
  }
  [data-kind='credits'] .label,
  [data-kind='near'] .label {
    color: var(--warn);
  }
  [data-kind='unread'] .dot,
  [data-kind='apiKey'] .dot,
  [data-kind='unsupported'] .dot {
    background: transparent;
    border: 1.5px solid var(--subtle);
  }
  [data-kind='unread'] .label,
  [data-kind='apiKey'] .label,
  [data-kind='unsupported'] .label,
  [data-kind='checking'] .label {
    color: var(--subtle);
    font-weight: 500;
  }
  [data-kind='checking'] .dot {
    background: transparent;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    width: 10px;
    height: 10px;
    margin-top: 0.28em;
    animation: status-spin 0.8s linear infinite;
  }
  @keyframes status-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    [data-kind='checking'] .dot {
      animation: none;
    }
  }
</style>
