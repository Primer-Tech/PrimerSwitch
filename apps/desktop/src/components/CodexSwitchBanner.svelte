<script lang="ts">
  import { language, t } from '../lib/i18n';
  import { codexElapsed } from '../lib/codex-format';
  let {
    name,
    stage,
    elapsed,
  }: {
    name: string;
    stage: 'saving' | 'restarting' | 'verifying';
    elapsed: number;
  } = $props();
  const stages = ['saving', 'restarting', 'verifying'] as const;
  let current = $derived(stages.indexOf(stage));
</script>

<section class="codex-switching" aria-labelledby="codex-switching-title">
  <span class="ring" aria-hidden="true"></span>
  <div class="body">
    <div class="title-row">
      <h2 id="codex-switching-title">
        {t($language, 'codexSwitchingTo', { name })}
      </h2>
      <span class="elapsed" aria-hidden="true">{codexElapsed(elapsed)}</span>
    </div>
    <p class="stage">
      {t(
        $language,
        stage === 'saving'
          ? 'codexStageSaving'
          : stage === 'restarting'
            ? 'codexStageRestarting'
            : 'codexStageVerifying',
      )}
    </p>
    <ol class="steps" aria-hidden="true">
      {#each stages as step, index (step)}<li
          class:done={index < current}
          class:current={index === current}
        >
          <span class="marker"></span>{t(
            $language,
            step === 'saving'
              ? 'codexStepSave'
              : step === 'restarting'
                ? 'codexStepRestart'
                : 'codexStepVerify',
          )}
        </li>{/each}
    </ol>
  </div>
</section>

<style>
  .codex-switching {
    display: flex;
    align-items: flex-start;
    gap: 15px;
    padding: 16px 18px;
    margin-bottom: 17px;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--line));
    border-radius: 10px;
    background:
      linear-gradient(
        100deg,
        color-mix(in srgb, var(--brand) 16%, transparent),
        transparent 70%
      ),
      var(--surface);
  }
  .ring {
    flex: none;
    width: 22px;
    height: 22px;
    margin-top: 1px;
    border-radius: 50%;
    border: 2.5px solid color-mix(in srgb, var(--accent) 25%, transparent);
    border-top-color: var(--accent);
    animation: codex-spin 0.9s linear infinite;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .title-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  h2 {
    font-size: 0.92rem;
    font-weight: 650;
    margin: 0;
    overflow-wrap: anywhere;
  }
  .elapsed {
    color: var(--subtle);
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }
  .stage {
    margin: 6px 0 0;
    color: var(--text);
    font-size: 0.8rem;
    line-height: 1.55;
  }
  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 22px;
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    font-size: 0.72rem;
    color: var(--subtle);
  }
  .steps li {
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
  .steps li.done {
    color: var(--text);
  }
  .steps li.done .marker {
    background: var(--green);
    border-color: var(--green);
  }
  .steps li.current {
    color: var(--accent);
    font-weight: 600;
  }
  .steps li.current .marker {
    border-color: var(--accent);
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent);
  }
  @keyframes codex-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .ring {
      animation: none;
    }
  }
</style>
