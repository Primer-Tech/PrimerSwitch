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
  'clientsRunning',
  'processInventoryUnavailable',
  'externalChange',
  'reconciliationRequired',
  'loginExpired',
  'loginCanceled',
  'providerUnavailable',
  'invalidPreparation',
  'busy',
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
    identityEvidence: z.enum([
      'none',
      'claimsOnly',
      'managedLogin',
      'backendVerified',
    ]),
    identityVerified: z.boolean(),
    selected: z.boolean(),
    manualSwitch: codexCapabilitySchema,
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
  })
  .strict();
export const codexLoginSchema = z
  .object({
    id,
    status: z.enum(['waiting', 'verifying', 'complete', 'failed']),
    error: codexReasonSchema.nullable(),
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
    activeModel: text.nullable(),
    capabilities: z
      .object({
        loginBrowser: codexCapabilitySchema,
        importCurrent: codexCapabilitySchema,
        refreshQuota: codexCapabilitySchema,
        manualSwitch: codexCapabilitySchema,
        deleteSaved: codexCapabilitySchema,
      })
      .strict(),
    login: codexLoginSchema.nullable(),
    busy: z.boolean(),
    error: codexReasonSchema.nullable(),
    demo: z.boolean(),
  })
  .strict();
export const codexPreparationSchema = z
  .object({ id, accountId: id, expiresAt: z.number().int() })
  .strict();
export type CodexSnapshot = z.infer<typeof codexSnapshotSchema>;
export type CodexAccount = z.infer<typeof codexAccountSchema>;
export type CodexLogin = z.infer<typeof codexLoginSchema>;
export type CodexPreparation = z.infer<typeof codexPreparationSchema>;
export type CodexLimit = z.infer<typeof limitSchema>;

export function hasCodexSelectionEvidence(account: CodexAccount): boolean {
  return (
    account.authKind === 'chatgpt' &&
    (account.identityEvidence === 'managedLogin' ||
      account.identityEvidence === 'backendVerified')
  );
}
