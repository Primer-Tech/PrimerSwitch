import type {
  CodexAccount,
  CodexSnapshot,
  CodexReason,
} from '../lib/codex-types';
export const codexCapability = (blockedReason: CodexReason | null = null) => ({
  enabled: blockedReason === null,
  blockedReason,
});
export function codexAccount(
  id = 'codex-a',
  overrides: Partial<CodexAccount> = {},
): CodexAccount {
  const now = Math.floor(Date.now() / 1000);
  return {
    id,
    provider: 'codex',
    name: id === 'codex-a' ? 'Studio Codex' : 'Personal Codex',
    email: `${id}@example.invalid`,
    workspaceId: 'workspace-fixture',
    workspaceName: 'Studio workspace',
    authKind: 'chatgpt',
    identityEvidence: 'backendVerified',
    identityVerified: true,
    selected: id === 'codex-a',
    manualSwitch: codexCapability(),
    quota: {
      ordinaryUsageAllowed: null,
      resetCreditsAvailable: null,
      limits: [
        {
          key: 'codex',
          limitId: 'codex',
          limitName: 'Codex',
          normalModelSlug: null,
          primary: {
            usedPercent: 42,
            windowDurationMins: 300,
            resetsAt: now + 7200,
          },
          secondary: {
            usedPercent: 19,
            windowDurationMins: 10080,
            resetsAt: now + 3 * 86400,
          },
          planType: 'plus',
          credits: { hasCredits: true, unlimited: false, balance: '12.50' },
          spendControlReached: null,
          rateLimitReachedType: null,
        },
        {
          key: 'review',
          limitId: 'code_review',
          limitName: 'Code review',
          normalModelSlug: null,
          primary: null,
          secondary: {
            usedPercent: 76,
            windowDurationMins: 1440,
            resetsAt: null,
          },
          planType: null,
          credits: null,
          spendControlReached: false,
          rateLimitReachedType: null,
        },
      ],
    },
    quotaReadAt: now - 32,
    quotaState: 'fresh',
    error: null,
    ...overrides,
  };
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
    accounts: [
      codexAccount(),
      codexAccount('codex-b', {
        selected: false,
        workspaceName: 'Personal workspace',
        quotaState: 'cached',
      }),
    ],
    selectedId: 'codex-a',
    activeModel: null,
    capabilities: {
      loginBrowser: codexCapability(),
      importCurrent: codexCapability(),
      refreshQuota: codexCapability(),
      manualSwitch: codexCapability(),
      deleteSaved: codexCapability(),
    },
    login: null,
    busy: false,
    error: null,
    demo: false,
    ...overrides,
  };
}
