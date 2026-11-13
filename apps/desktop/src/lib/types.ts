import { z } from 'zod';

const timestamp = z.number().int().nullable();
const percent = z.number().finite().nonnegative();
const windowSchema = z
  .object({ utilization: percent, resetsAt: timestamp })
  .strict();
/** Redacted runtime error: a catalog key, never provider or file text. */
export const errorViewSchema = z
  .object({
    code: z.string().regex(/^[A-Za-z]{1,64}$/),
    accountId: z.string().nullable(),
    // Only ever a fixed environment-variable or settings name.
    param: z
      .string()
      .regex(/^[A-Za-z0-9_]{1,64}$/)
      .nullable(),
    action: z.enum(['autoSwitch']).nullable(),
    at: z.number().int(),
  })
  .strict();
export const settingsSchema = z
  .object({
    pollInterval: z
      .number()
      .int()
      .min(120)
      .max(900)
      .refine((v) => v % 30 === 0),
    threshold: z.number().finite().min(50).max(100),
    claudeThreshold: z.number().finite().min(50).max(100).nullable().optional(),
    codexThreshold: z.number().finite().min(50).max(100).nullable().optional(),
    autoSwitchEnabled: z.boolean(),
    autoStartWindowEnabled: z.boolean(),
    autoUseResetsEnabled: z.boolean(),
    appearance: z.enum(['system', 'light', 'dark']),
    language: z.enum(['en', 'ro']).default('en'),
    /** Claude and Codex: the account whose weekly limit resets soonest goes first. */
    preferSoonestWeeklyReset: z.boolean(),
  })
  .strict();
export const accountAutomationSchema = z
  .object({
    enabled: z.boolean(),
    threshold: z.number().finite().min(50).max(100).nullable(),
    priority: z.number().int().min(1).max(99).nullable(),
    returnOnFiveHourReset: z.boolean(),
  })
  .strict();
export type AccountAutomation = z.infer<typeof accountAutomationSchema>;
export const defaultAutomation: AccountAutomation = {
  enabled: true,
  threshold: null,
  priority: null,
  returnOnFiveHourReset: false,
};
/** Optional provider values preserve older installations' single saved default. */
export function providerThreshold(
  settings: Settings | null,
  provider: 'claude' | 'codex',
): number {
  return (
    (provider === 'claude'
      ? settings?.claudeThreshold
      : settings?.codexThreshold) ??
    settings?.threshold ??
    95
  );
}
/** The four kinds of banked Claude reset, in the order the runtime lists them. */
export const resetKindSchema = z.enum(['session', 'weekly', 'fable', 'full']);
export type ResetKind = z.infer<typeof resetKindSchema>;
export const resetKinds: readonly ResetKind[] = resetKindSchema.options;
/** Why a kind of reset cannot be used now; each one is a catalog key suffix. */
export const resetReasonSchema = z.enum([
  'cooldown',
  'blockedFiveHour',
  'blockedSevenDay',
  'blockedOpus',
  'blockedSonnet',
  'blockedOther',
  'cannotClear',
  'needsLimit',
  'unavailable',
  'pendingClaim',
]);
export type ResetReason = z.infer<typeof resetReasonSchema>;
/** One kind of banked reset an account holds. No grant or request id is ever sent. */
export const resetOfferSchema = z
  .object({
    kind: resetKindSchema,
    resetsLeft: z.number().int().nonnegative(),
    resetsTotal: z.number().int().nonnegative(),
    endsAt: timestamp,
    usableNow: z.boolean(),
    unavailableReason: resetReasonSchema.nullable(),
    /** Using it now would waste part of a limit not yet reached; needs confirming. */
    earlyUse: z
      .object({ percentLeft: z.number().int().min(0).max(100).nullable() })
      .strict()
      .nullable(),
  })
  .strict();
export type ResetOffer = z.infer<typeof resetOfferSchema>;
export const accountSchema = z
  .object({
    id: z.string(),
    name: z.string(),
    email: z.string(),
    workspaceLabel: z.string().max(512).nullable().optional(),
    provider: z.literal('claude'),
    automation: accountAutomationSchema.optional(),
    effectiveThreshold: z.number().finite().min(50).max(100).optional(),
    active: z.boolean(),
    isNext: z.boolean(),
    exhausted: z.boolean(),
    identityVerified: z.boolean(),
    usage: z
      .object({
        fiveHour: windowSchema,
        sevenDay: windowSchema,
        weeklyOverall: percent,
        weeklyModel: percent,
        weeklyOverallResetsAt: timestamp.optional(),
        weeklyModelResetsAt: timestamp.optional(),
        scopedLimits: z.array(
          z
            .object({ label: z.string(), percent, resetsAt: timestamp })
            .strict(),
        ),
        /**
         * The provider's own weekly Fable row, whatever model is selected. Missing
         * or null means unknown: never zero, and never another window's value.
         */
        fable: z
          .object({ label: z.string(), percent, resetsAt: timestamp })
          .strict()
          .nullable()
          .optional(),
      })
      .strict()
      .nullable(),
    usageAt: timestamp,
    scopedAt: timestamp.optional(),
    decisionFresh: z.boolean().optional(),
    // Stable error code of the last failed reading.
    error: z.string().nullable(),
    signInRequired: z.boolean().optional(),
    freesAt: timestamp.optional(),
    subscriptionStatus: z.string().nullable(),
    planTier: z.string().nullable(),
    // The real plan (claude_max, claude_pro, claude_team, claude_enterprise) and the
    // seat tier; null or absent while unknown.
    organizationType: z.string().nullable().optional(),
    seatTier: z.string().nullable().optional(),
    renewalDay: z.number().int().min(1).max(31).nullable(),
    nextRenewalAt: timestamp,
    resets: z
      .object({
        available: z.number().int().nonnegative().nullable(),
        expiresAt: timestamp,
        cooldownUntil: timestamp,
        pending: z.boolean(),
        lastOutcome: z.string().nullable(),
        offers: z.array(resetOfferSchema),
      })
      .strict(),
    primedAt: timestamp,
    fiveHourPrimedAt: timestamp,
  })
  .strict();
export const snapshotSchema = z
  .object({
    revision: z.number().int().nonnegative(),
    provider: z.literal('claude'),
    accounts: z.array(accountSchema),
    activeId: z.string().nullable(),
    activeModel: z.string().nullable(),
    settings: settingsSchema,
    lastRefreshAt: timestamp,
    busy: z.boolean(),
    error: errorViewSchema.nullable(),
    notice: errorViewSchema.nullable().optional(),
    demo: z.boolean(),
    consumptionPlan: z.array(z.string()).optional(),
    returnHoldUntil: timestamp.optional(),
    /** Launching a saved account in its own Claude window exists in this build. */
    launchSupported: z.boolean().optional(),
  })
  .strict();
export type Snapshot = z.infer<typeof snapshotSchema>;
export type AccountView = z.infer<typeof accountSchema>;
export type ErrorView = z.infer<typeof errorViewSchema>;
export type Settings = z.infer<typeof settingsSchema>;
export type LoginSession = { id: string; url: string };
export type ImportPreview = {
  id: string;
  valid: number;
  invalid: number;
  names: string[];
};
export const defaults: Settings = {
  pollInterval: 300,
  threshold: 95,
  autoSwitchEnabled: true,
  autoStartWindowEnabled: true,
  autoUseResetsEnabled: true,
  appearance: 'dark',
  language: 'en',
  preferSoonestWeeklyReset: true,
};
export const loginSchema = z
  .object({ id: z.string().min(1), url: z.string().url() })
  .strict();
/** Models a launched Claude window can start with; `default` keeps shared settings. */
export const launchModelSchema = z.enum(['default', 'opus', 'sonnet', 'fable']);
export type LaunchModel = z.infer<typeof launchModelSchema>;
export const launchModels: readonly LaunchModel[] = launchModelSchema.options;
/** Why a launch profile must be authorized (again) before it can start. */
export const profileAuthReasonSchema = z.enum([
  'firstUse',
  'identityChanged',
  'expired',
]);
export type ProfileAuthReason = z.infer<typeof profileAuthReasonSchema>;
/** Result of a launch or of finishing a launch authorization. */
export const launchOutcomeSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('launched') }).strict(),
  z.object({ status: z.literal('authorized') }).strict(),
  z.object({ status: z.literal('cancelled') }).strict(),
  z
    .object({
      status: z.literal('authorizationRequired'),
      reason: profileAuthReasonSchema,
    })
    .strict(),
]);
export type LaunchOutcome = z.infer<typeof launchOutcomeSchema>;
export const previewSchema = z
  .object({
    id: z.string().min(1),
    valid: z.number().int().nonnegative(),
    invalid: z.number().int().nonnegative(),
    names: z.array(z.string()),
  })
  .strict()
  .nullable();
