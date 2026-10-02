import { z } from 'zod';

const timestamp = z.number().int().nullable();
const percent = z.number().finite().nonnegative();
const windowSchema = z
  .object({ utilization: percent, resetsAt: timestamp })
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
    autoSwitchEnabled: z.boolean(),
    autoStartWindowEnabled: z.boolean(),
    autoUseResetsEnabled: z.boolean(),
    appearance: z.enum(['system', 'light', 'dark']),
    language: z.enum(['en', 'ro']).default('en'),
  })
  .strict();
export const accountSchema = z
  .object({
    id: z.string(),
    name: z.string(),
    email: z.string(),
    provider: z.literal('claude'),
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
      })
      .strict()
      .nullable(),
    usageAt: timestamp,
    scopedAt: timestamp.optional(),
    decisionFresh: z.boolean().optional(),
    error: z.string().nullable(),
    subscriptionStatus: z.string().nullable(),
    planTier: z.string().nullable(),
    renewalDay: z.number().int().min(1).max(31).nullable(),
    nextRenewalAt: timestamp,
    resets: z
      .object({
        available: z.number().int().nonnegative().nullable(),
        expiresAt: timestamp,
        cooldownUntil: timestamp,
        pending: z.boolean(),
        lastOutcome: z.string().nullable(),
      })
      .strict(),
    primedAt: timestamp,
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
    error: z.string().nullable(),
    demo: z.boolean(),
    consumptionPlan: z.array(z.string()).optional(),
  })
  .strict();
export type Snapshot = z.infer<typeof snapshotSchema>;
export type AccountView = z.infer<typeof accountSchema>;
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
};
export const loginSchema = z
  .object({ id: z.string().min(1), url: z.string().url() })
  .strict();
export const previewSchema = z
  .object({
    id: z.string().min(1),
    valid: z.number().int().nonnegative(),
    invalid: z.number().int().nonnegative(),
    names: z.array(z.string()),
  })
  .strict()
  .nullable();
