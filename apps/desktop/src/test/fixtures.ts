import { defaults, type AccountView, type Snapshot } from '../lib/types';

export function account(
  id: string,
  overrides: Partial<AccountView> = {},
): AccountView {
  const now = Math.floor(Date.now() / 1000);
  return {
    id,
    name: id === 'a' ? 'Studio' : 'Personal',
    email: `${id}@example.invalid`,
    provider: 'claude',
    active: id === 'a',
    isNext: id === 'b',
    exhausted: false,
    identityVerified: true,
    usage: {
      fiveHour: { utilization: id === 'a' ? 68 : 23, resetsAt: now + 3600 },
      sevenDay: {
        utilization: id === 'a' ? 47 : 18,
        resetsAt: now + 3 * 86400,
      },
      weeklyOverall: id === 'a' ? 47 : 18,
      weeklyModel: id === 'a' ? 58 : 24,
      weeklyOverallResetsAt: now + 3 * 86400,
      weeklyModelResetsAt: now + 3 * 86400,
      scopedLimits: [
        {
          label: 'Sonnet',
          percent: id === 'a' ? 58 : 24,
          resetsAt: now + 3 * 86400,
        },
      ],
    },
    usageAt: now - 34,
    scopedAt: now - 34,
    decisionFresh: true,
    error: null,
    signInRequired: false,
    freesAt: null,
    subscriptionStatus: 'Activ',
    planTier: id === 'a' ? 'Max 5×' : 'Pro',
    renewalDay: 15,
    nextRenewalAt: now + 13 * 86400,
    resets: {
      available: 2,
      expiresAt: now + 5 * 86400,
      cooldownUntil: null,
      pending: false,
      lastOutcome: null,
    },
    primedAt: now - 3 * 86400,
    fiveHourPrimedAt: now - 2 * 3600,
    ...overrides,
  };
}
export function snapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    revision: 1,
    provider: 'claude',
    accounts: [account('a'), account('b')],
    activeId: 'a',
    activeModel: 'Sonnet',
    settings: { ...defaults },
    lastRefreshAt: Math.floor(Date.now() / 1000) - 34,
    busy: false,
    error: null,
    notice: null,
    demo: false,
    consumptionPlan: ['b', 'a'],
    ...overrides,
  };
}
