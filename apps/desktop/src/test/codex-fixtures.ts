import type {
  CodexAccount,
  CodexLimit,
  CodexSnapshot,
  CodexReason,
} from '../lib/codex-types';
export const codexCapability = (blockedReason: CodexReason | null = null) => ({
  enabled: blockedReason === null,
  blockedReason,
});
const minute = 60,
  hour = 3600,
  day = 86400;
/** The main Codex limit: a 5-hour window and a weekly window. */
export function codexMainLimit(
  fiveHour: number,
  weekly: number,
  overrides: Partial<CodexLimit> = {},
  now = Math.floor(Date.now() / 1000),
): CodexLimit {
  return {
    key: 'default',
    limitId: 'codex',
    limitName: 'Codex',
    normalModelSlug: null,
    primary: {
      usedPercent: fiveHour,
      windowDurationMins: 300,
      resetsAt: now + 3 * hour + 12 * minute,
    },
    secondary: {
      usedPercent: weekly,
      windowDurationMins: 10080,
      resetsAt: now + 4 * day + 6 * hour,
    },
    planType: null,
    credits: null,
    spendControlReached: false,
    rateLimitReachedType: null,
    ...overrides,
  };
}
/** Fictional accounts; names default to the email, as the native side does. */
export function codexAccount(
  id = 'codex-studio',
  overrides: Partial<CodexAccount> = {},
): CodexAccount {
  const now = Math.floor(Date.now() / 1000);
  const email = `${id.replace(/^codex-/, '')}@example.invalid`;
  return {
    id,
    provider: 'codex',
    name: email,
    email,
    workspaceId: `workspace-${id}`,
    workspaceName: null,
    authKind: 'chatgpt',
    planType: 'pro',
    identityEvidence: 'backendVerified',
    identityVerified: true,
    selected: false,
    switchable: codexCapability(),
    quota: {
      ordinaryUsageAllowed: true,
      resetCreditsAvailable: null,
      limits: [codexMainLimit(20, 30, {}, now)],
    },
    quotaReadAt: now - 2 * minute,
    quotaState: 'fresh',
    error: null,
    needsSignIn: false,
    ...overrides,
  };
}
/**
 * Three realistic accounts: the active Pro one around 40% / 35% with a code-review
 * limit and credits, a Plus one at its 5-hour limit (frees in about 2h 10m), and a
 * lightly used Pro Lite one that is the best next account.
 */
export function codexAccounts(
  now = Math.floor(Date.now() / 1000),
): CodexAccount[] {
  return [
    codexAccount('codex-studio', {
      selected: true,
      planType: 'pro',
      quota: {
        ordinaryUsageAllowed: true,
        resetCreditsAvailable: null,
        limits: [
          codexMainLimit(
            40,
            35,
            {
              credits: { hasCredits: true, unlimited: false, balance: '12.50' },
            },
            now,
          ),
          {
            key: 'bucket:codex',
            limitId: 'codex',
            limitName: 'Codex',
            normalModelSlug: null,
            primary: {
              usedPercent: 40,
              windowDurationMins: 300,
              resetsAt: now + 3 * hour + 12 * minute,
            },
            secondary: null,
            planType: null,
            credits: null,
            spendControlReached: null,
            rateLimitReachedType: null,
          },
          {
            key: 'bucket:code-review',
            limitId: 'code-review',
            limitName: 'Code review',
            normalModelSlug: null,
            primary: {
              usedPercent: 12,
              windowDurationMins: 10080,
              resetsAt: now + 4 * day + 6 * hour,
            },
            secondary: null,
            planType: null,
            credits: null,
            spendControlReached: null,
            rateLimitReachedType: null,
          },
        ],
      },
      quotaReadAt: now - 2 * minute,
    }),
    codexAccount('codex-personal', {
      planType: 'plus',
      quota: {
        ordinaryUsageAllowed: true,
        resetCreditsAvailable: null,
        limits: [
          codexMainLimit(
            100,
            62,
            {
              primary: {
                usedPercent: 100,
                windowDurationMins: 300,
                resetsAt: now + 2 * hour + 10 * minute,
              },
              secondary: {
                usedPercent: 62,
                windowDurationMins: 10080,
                resetsAt: now + 2 * day + 3 * hour,
              },
              rateLimitReachedType: 'primary',
            },
            now,
          ),
        ],
      },
      quotaReadAt: now - 6 * minute,
      quotaState: 'cached',
    }),
    codexAccount('codex-research', {
      planType: 'prolite',
      quota: {
        ordinaryUsageAllowed: true,
        resetCreditsAvailable: null,
        limits: [
          codexMainLimit(
            8,
            14,
            {
              primary: {
                usedPercent: 8,
                windowDurationMins: 300,
                resetsAt: now + 4 * hour + 30 * minute,
              },
              secondary: {
                usedPercent: 14,
                windowDurationMins: 10080,
                resetsAt: now + 6 * day + 2 * hour,
              },
            },
            now,
          ),
        ],
      },
      quotaReadAt: now - 4 * minute,
    }),
  ];
}
export function codexSnapshot(
  overrides: Partial<CodexSnapshot> = {},
): CodexSnapshot {
  return {
    revision: 1,
    provider: 'codex',
    availability: 'supported',
    blockedReason: null,
    executableVersion: '0.160.0',
    accounts: codexAccounts(),
    selectedId: 'codex-studio',
    activeModel: null,
    capabilities: {
      loginBrowser: codexCapability(),
      importCurrent: codexCapability(),
      importSwitcher: codexCapability('switcherUnavailable'),
      refreshQuota: codexCapability(),
      switchAccount: codexCapability(),
      deleteSaved: codexCapability(),
    },
    login: null,
    busy: false,
    error: null,
    demo: false,
    switching: null,
    lastSwitch: null,
    environment: {
      daemonRunning: true,
      otherClients: 0,
      codexSwitcherRunning: false,
    },
    warnings: [],
    ...overrides,
  };
}
/** The read-only preview: Codex Switcher is running, so its banner is visible. */
export function codexDemoSnapshot(
  overrides: Partial<CodexSnapshot> = {},
): CodexSnapshot {
  return codexSnapshot({
    demo: true,
    activeModel: 'gpt-5-codex',
    capabilities: {
      loginBrowser: codexCapability(),
      importCurrent: codexCapability(),
      importSwitcher: codexCapability(),
      refreshQuota: codexCapability(),
      switchAccount: codexCapability(),
      deleteSaved: codexCapability(),
    },
    environment: {
      daemonRunning: true,
      otherClients: 1,
      codexSwitcherRunning: true,
    },
    ...overrides,
  });
}
