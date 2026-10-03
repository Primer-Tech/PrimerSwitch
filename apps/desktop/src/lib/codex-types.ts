import { z } from 'zod';
const text = z.string().max(512);
const id = z.string().min(1).max(256);
const timestamp = z.number().int().nullable();
export const codexReasonSchema = z.enum([
  'notInstalled',
  'unsupportedVersion',
  'unsupportedStore',
  'unsupportedAuth',
  'policyRestricted',
  'externalCredentials',
  'storeConflict',
  'vaultUnavailable',
  'identityUnverified',
  'identityMismatch',
  'externalChange',
  'loginExpired',
  'loginCanceled',
  'providerUnavailable',
  'busy',
  'daemonRestartFailed',
  'signInRequired',
  'switchInProgress',
  'switcherUnavailable',
  'activeAccount',
]);
export type CodexReason = z.infer<typeof codexReasonSchema>;
export const codexCapabilitySchema = z
  .object({ enabled: z.boolean(), blockedReason: codexReasonSchema.nullable() })
  .strict();
const windowSchema = z
  .object({
    usedPercent: z.number().finite().nonnegative(),
    windowDurationMins: z.number().int().positive().nullable(),
    resetsAt: timestamp,
  })
  .strict();
const limitSchema = z
  .object({
    key: id,
    limitId: text.nullable(),
    limitName: text.nullable(),
    normalModelSlug: text.nullable(),
    primary: windowSchema.nullable(),
    secondary: windowSchema.nullable(),
    planType: text.nullable(),
    credits: z
      .object({
        hasCredits: z.boolean(),
        unlimited: z.boolean(),
        balance: text.nullable(),
      })
      .strict()
      .nullable(),
    spendControlReached: z.boolean().nullable(),
    rateLimitReachedType: text.nullable(),
  })
  .strict();
export const codexAccountSchema = z
  .object({
    id,
    provider: z.literal('codex'),
    name: text,
    email: text.nullable(),
    workspaceId: text.nullable(),
    workspaceName: text.nullable(),
    authKind: z.enum(['chatgpt', 'apiKey', 'unsupported']),
    planType: text.nullable(),
    identityEvidence: z.enum([
      'none',
      'claimsOnly',
      'managedLogin',
      'backendVerified',
    ]),
    identityVerified: z.boolean(),
    selected: z.boolean(),
    switchable: codexCapabilitySchema,
    quota: z
      .object({
        ordinaryUsageAllowed: z.boolean().nullable(),
        limits: z.array(limitSchema),
        resetCreditsAvailable: z.number().int().nonnegative().nullable(),
      })
      .strict()
      .nullable(),
    quotaReadAt: timestamp,
    quotaState: z.enum(['unread', 'fresh', 'cached', 'unavailable']),
    error: codexReasonSchema.nullable(),
    needsSignIn: z.boolean(),
  })
  .strict();
export const codexLoginSchema = z
  .object({
    id,
    status: z.enum(['waiting', 'verifying', 'complete', 'failed']),
    error: codexReasonSchema.nullable(),
  })
  .strict();
const switchingSchema = z
  .object({
    targetId: id,
    stage: z.enum(['saving', 'restarting', 'verifying']),
    startedAt: z.number().int(),
  })
  .strict();
const lastSwitchSchema = z
  .object({
    accountId: id,
    at: z.number().int(),
    daemonRestarted: z.boolean(),
    otherClients: z.number().int().nonnegative(),
    error: codexReasonSchema.nullable(),
  })
  .strict();
const environmentSchema = z
  .object({
    daemonRunning: z.boolean().nullable(),
    otherClients: z.number().int().nonnegative(),
    codexSwitcherRunning: z.boolean(),
  })
  .strict();
export const codexSnapshotSchema = z
  .object({
    revision: z.number().int().nonnegative(),
    provider: z.literal('codex'),
    availability: z.enum([
      'notInstalled',
      'unqualified',
      'supported',
      'unsupported',
    ]),
    blockedReason: codexReasonSchema.nullable(),
    executableVersion: text.nullable(),
    accounts: z.array(codexAccountSchema),
    selectedId: id.nullable(),
    /** The account automatic switching uses next, ranked natively. */
    nextId: id.nullable(),
    /** Every account automatic switching may use, best first; `order[0]` is `nextId`. */
    order: z.array(id),
    activeModel: text.nullable(),
    capabilities: z
      .object({
        loginBrowser: codexCapabilitySchema,
        importCurrent: codexCapabilitySchema,
        importSwitcher: codexCapabilitySchema,
        refreshQuota: codexCapabilitySchema,
        switchAccount: codexCapabilitySchema,
        deleteSaved: codexCapabilitySchema,
      })
      .strict(),
    login: codexLoginSchema.nullable(),
    busy: z.boolean(),
    error: codexReasonSchema.nullable(),
    demo: z.boolean(),
    switching: switchingSchema.nullable(),
    lastSwitch: lastSwitchSchema.nullable(),
    environment: environmentSchema,
    warnings: z.array(codexReasonSchema),
  })
  .strict();
export type CodexSnapshot = z.infer<typeof codexSnapshotSchema>;
export type CodexAccount = z.infer<typeof codexAccountSchema>;
export type CodexLogin = z.infer<typeof codexLoginSchema>;
export type CodexLimit = z.infer<typeof limitSchema>;
export type CodexWindow = z.infer<typeof windowSchema>;
export type CodexSwitching = z.infer<typeof switchingSchema>;
export type CodexLastSwitch = z.infer<typeof lastSwitchSchema>;
