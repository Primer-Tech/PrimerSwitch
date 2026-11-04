import { describe, expect, it } from 'vitest';
import {
  codexAccountSchema,
  codexKeyUsageSchema,
  codexSnapshotSchema,
} from './codex-types';
import { codexView } from './codex-view';
import { defaults } from './types';
import {
  codexAccount,
  codexApiKeyAccount,
  codexKeyUsage,
  codexSnapshot,
} from '../test/codex-fixtures';

describe('Sanitized local API-key consumption contract', () => {
  it('passes native totals through the row only for API keys', () => {
    const usage = codexKeyUsage();
    const raw = codexSnapshot({
      accounts: [codexApiKeyAccount('key', { usage }), codexAccount()],
    });
    const snapshot = codexSnapshotSchema.parse(raw);
    const page = codexView(snapshot, {
      locale: 'en',
      now: 1_800_000_000,
      settings: defaults,
    });
    expect(page.rows.find((row) => row.id === 'key')!.apiKey?.usage).toEqual(
      usage,
    );
    expect(page.rows.find((row) => row.id !== 'key')!.apiKey).toBeNull();
    expect(
      codexAccountSchema.safeParse(codexAccount('chatgpt', { usage })).success,
    ).toBe(false);
    expect(
      codexAccountSchema.safeParse(codexAccount('chatgpt', { usage: null }))
        .success,
    ).toBe(true);
    expect(codexAccountSchema.safeParse(codexAccount()).success).toBe(true);
  });

  it.each([
    -1,
    0.5,
    Number.NaN,
    Number.POSITIVE_INFINITY,
    Number.MAX_SAFE_INTEGER + 1,
  ])('rejects unsafe counter %s', (count) => {
    for (const counter of ['input', 'cached', 'output', 'total'] as const) {
      const usage = codexKeyUsage();
      usage.windows.last24h[counter] = count;
      expect(codexKeyUsageSchema.safeParse(usage).success).toBe(false);
    }
  });

  it('accepts safe integer boundaries and rejects unknown or credential-shaped fields at every level', () => {
    const usage = codexKeyUsage();
    usage.windows.total.total = Number.MAX_SAFE_INTEGER;
    expect(codexKeyUsageSchema.safeParse(usage).success).toBe(true);
    for (const invalid of [
      { ...usage, key: 'secret-sentinel' },
      { ...usage, records: [] },
      { ...usage, state: 'secret-sentinel' },
      { ...usage, windows: { ...usage.windows, sessionPath: 'private-path' } },
      {
        ...usage,
        windows: {
          ...usage.windows,
          total: { ...usage.windows.total, fingerprint: 'secret-sentinel' },
        },
      },
      { ...usage, windows: { ...usage.windows, last7d: undefined } },
      { ...usage, trackedSince: -1 },
      { ...usage, trackedSince: Number.MAX_SAFE_INTEGER },
    ])
      expect(codexKeyUsageSchema.safeParse(invalid).success).toBe(false);
  });
});
