<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { percentage } from '../lib/format';
  import type { Settings } from '../lib/types';
  import type { AutomationItem } from '../lib/provider-view';
  import Icon from './Icon.svelte';
  let {
    settings,
    settingsDisabled,
    onsettings,
    items = [],
  }: {
    /** The automation settings both providers share; null until loaded. */
    settings: Settings | null;
    settingsDisabled: boolean;
    onsettings: () => void;
    /** Provider-specific lines inside the same card. */
    items?: AutomationItem[];
  } = $props();
  let toggles = $derived(
    items.filter((item) => item.kind === 'toggle') as Extract<
      AutomationItem,
      { kind: 'toggle' }
    >[],
  );
  let notes = $derived(
    items.filter((item) => item.kind === 'note') as Extract<
      AutomationItem,
      { kind: 'note' }
    >[],
  );
</script>

<section
  class="panel side-panel automation-panel"
  aria-labelledby="automation-heading"
>
  <div class="panel-heading">
    <h2 id="automation-heading">
      <Icon name="automation" />{t($language, 'automation')}
    </h2>
    <button
      class="icon-button"
      aria-label={t($language, 'manageAutomation')}
      disabled={settingsDisabled}
      onclick={onsettings}><Icon name="settings" size={16} /></button
    >
  </div>
  <p class="automation-state">
    <span class="status-dot" class:inactive={!settings?.autoSwitchEnabled}
    ></span>{t(
      $language,
      settings?.autoSwitchEnabled ? 'autoSwitchOn' : 'autoSwitchOff',
    )}
  </p>
  <dl class="automation-list">
    <div>
      <dt>{t($language, 'switchThreshold')}</dt>
      <dd>{percentage(settings?.threshold ?? 95, $language)}%</dd>
    </div>
    <div>
      <dt>{t($language, 'pollInterval')}</dt>
      <dd>
        {t($language, 'minutes', {
          count: percentage((settings?.pollInterval ?? 300) / 60, $language),
        })}
      </dd>
    </div>
    <div>
      <dt>{t($language, 'resetOrder')}</dt>
      <dd>
        {t(
          $language,
          settings?.preferSoonestWeeklyReset === false
            ? 'resetOrderMostLeft'
            : 'resetOrderSoonest',
        )}
      </dd>
    </div>
    {#each toggles as toggle (toggle.label)}<div>
        <dt>{toggle.label}</dt>
        <dd class:enabled={toggle.on}>
          {t($language, toggle.on ? 'on' : 'off')}
        </dd>
      </div>{/each}
  </dl>
  {#if notes.length}<ul class="automation-notes">
      {#each notes as note (note.text)}<li data-tone={note.tone}>
          <span class="status-dot"></span>{note.text}
        </li>{/each}
    </ul>{/if}
</section>

<style>
  .automation-state {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 12px 0 0;
    font-size: 0.76rem;
    font-weight: 600;
    color: var(--text);
  }
  .automation-list {
    margin: 10px 0 0;
    border-top: 1px solid var(--line);
    padding-top: 4px;
    font-size: 0.74rem;
  }
  .automation-list > div {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    margin: 6px 0;
  }
  .automation-list dt {
    color: var(--subtle);
  }
  .automation-list dd {
    margin: 0;
    text-align: right;
  }
  .automation-list dd.enabled {
    color: var(--green);
  }
  .automation-notes {
    list-style: none;
    margin: 8px 0 0;
    padding: 9px 0 0;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 0.72rem;
    line-height: 1.45;
    color: var(--text);
  }
  .automation-notes li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .automation-notes .status-dot {
    margin-top: 0.45em;
  }
  .automation-notes [data-tone='warn'] .status-dot {
    background: var(--warn);
  }
  .automation-notes [data-tone='muted'] {
    color: var(--subtle);
  }
  .automation-notes [data-tone='muted'] .status-dot {
    background: var(--subtle);
  }
</style>
