<script lang="ts">
  import { language, t } from '../lib/i18n';
  import {
    codexDuration,
    codexMessage,
    codexStatus,
  } from '../lib/codex-format';
  import type { CodexAccount, CodexReason } from '../lib/codex-types';
  let {
    account,
    now,
    checking = false,
    blockedReason = null,
  }: {
    account: CodexAccount;
    now: number;
    checking?: boolean;
    /** A per-account reason why switching is unavailable, shown as a hint. */
    blockedReason?: CodexReason | null;
  } = $props();
  let status = $derived(codexStatus(account));
  let label = $derived(
    t(
      $language,
      checking
        ? 'codexStatusChecking'
        : status.kind === 'signIn'
          ? 'codexStatusSignIn'
          : status.kind === 'active'
            ? 'codexStatusActive'
            : status.kind === 'apiKey'
              ? 'codexStatusApiKey'
              : status.kind === 'unsupported'
                ? 'codexStatusUnsupported'
                : status.kind === 'unread'
                  ? 'codexStatusUnread'
                  : status.kind === 'limited'
                    ? 'codexStatusLimited'
                    : 'codexStatusReady',
    ),
  );
  let limitDetail = $derived(
    !status.limited || status.freesAt === null
      ? status.kind === 'active' && status.limited
        ? t($language, 'codexStatusLimited')
        : null
      : status.freesAt <= now
        ? t($language, 'codexStatusFreeAgain')
        : t(
            $language,
            status.kind === 'active'
              ? 'codexLimitReachedFreesIn'
              : 'codexStatusFreesIn',
            { duration: codexDuration(status.freesAt - now, $language) },
          ),
  );
  let detail = $derived.by((): { text: string; title?: string } | null => {
    if (checking || status.kind === 'signIn') return null;
    if (status.kind === 'apiKey')
      return { text: t($language, 'codexStatusApiKeyDetail') };
    if (limitDetail) return { text: limitDetail };
    if (account.error)
      return {
        text: t($language, 'codexStatusCheckFailed'),
        title: codexMessage(account.error, $language),
      };
    if (blockedReason && status.kind !== 'unsupported')
      return {
        text: t($language, 'codexStatusSwitchBlocked'),
        title: codexMessage(blockedReason, $language),
      };
    return null;
  });
</script>

<span
  class="codex-status"
  data-kind={checking ? 'checking' : status.kind}
  class:limited={status.limited}
>
  <span class="dot" aria-hidden="true"></span><span class="text"
    ><span class="label">{label}</span>{#if detail}<small title={detail.title}
        >{detail.text}</small
      >{/if}</span
  >
</span>

<style>
  .codex-status {
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
    font-size: 0.7rem;
    color: var(--subtle);
    overflow-wrap: anywhere;
  }
  small[title] {
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
  [data-kind='limited'] .dot,
  [data-kind='signIn'] .dot,
  .limited .dot {
    background: var(--danger);
  }
  [data-kind='limited'] .label,
  [data-kind='signIn'] .label {
    color: var(--danger);
  }
  [data-kind='unread'] .dot,
  [data-kind='apiKey'] .dot,
  [data-kind='unsupported'] .dot {
    background: transparent;
    border: 1.5px solid var(--subtle);
  }
  [data-kind='unread'] .label,
  [data-kind='apiKey'] .label,
  [data-kind='unsupported'] .label {
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
    animation: codex-spin 0.8s linear infinite;
  }
  [data-kind='checking'] .label {
    color: var(--subtle);
    font-weight: 500;
  }
  @keyframes codex-spin {
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
