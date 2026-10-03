<script lang="ts">
  import { language, t } from '../lib/i18n';
  import type { OrderEntry } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  let { order, empty }: { order: OrderEntry[]; empty: string } = $props();
</script>

<section class="usage-order" aria-label={t($language, 'usageOrder')}>
  <span class="order-title">{t($language, 'usageOrder')}</span>
  {#if order.length}<ol>
      {#each order as entry, index (entry.id)}<li>
          <span class="order-number">{index + 1}</span><span
            class="order-name"
            title={entry.name}>{entry.name}</span
          >{#if index < order.length - 1}<span
              class="order-arrow"
              aria-hidden="true"><Icon name="next" size={13} /></span
            >{/if}
        </li>{/each}
    </ol>{:else}<small>{empty}</small>{/if}
</section>

<style>
  .usage-order {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 14px;
    border: 1px solid var(--line);
    background: var(--surface);
    border-radius: 8px;
    padding: 8px 14px;
    margin-top: 10px;
    font-size: 0.74rem;
  }
  .order-title {
    color: var(--subtle);
    font-size: 0.72rem;
  }
  ol {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
    list-style: none;
    margin: 0;
    padding: 0;
    min-width: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }
  .order-number {
    font-size: 0.64rem;
    width: 19px;
    height: 19px;
    flex: none;
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--subtle);
  }
  .order-name {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .order-arrow {
    display: inline-flex;
    color: var(--subtle);
  }
  small {
    color: var(--subtle);
    font-size: 0.72rem;
  }
</style>
