import { describe, expect, it } from 'vitest';
import {
  exactPercent,
  parsePercent,
  stepPercent,
  parsePriority,
} from './automation';
import {
  accountAutomationSchema,
  defaults,
  providerThreshold,
  settingsSchema,
  snapshotSchema,
} from './types';
import { codexSnapshotSchema } from './codex-types';
import { snapshot } from '../test/fixtures';
import { codexSnapshot } from '../test/codex-fixtures';
import { claudeView } from './claude-view';
import { codexView } from './codex-view';

describe('automation contracts and threshold inheritance', () => {
  it('retains the legacy default while allowing independent nullable overrides', () => {
    const settings = settingsSchema.parse({
      ...defaults,
      threshold: 93.75,
      claudeThreshold: null,
      codexThreshold: 97.125,
    });
    expect(providerThreshold(settings, 'claude')).toBe(93.75);
    expect(providerThreshold(settings, 'codex')).toBe(97.125);
    expect(providerThreshold({ ...defaults, threshold: 82.5 }, 'codex')).toBe(
      82.5,
    );
    expect(
      settingsSchema.safeParse({ ...settings, claudeThreshold: NaN }).success,
    ).toBe(false);
    expect(
      settingsSchema.safeParse({ ...settings, codexThreshold: 100.001 })
        .success,
    ).toBe(false);
  });
  it('accepts older snapshots and latent excluded preferences but rejects invalid ranges or extra IPC data', () => {
    expect(snapshotSchema.safeParse(snapshot()).success).toBe(true);
    expect(codexSnapshotSchema.safeParse(codexSnapshot()).success).toBe(true);
    const preference = {
      enabled: false,
      threshold: 82.125,
      priority: 1,
      returnOnFiveHourReset: true,
    };
    expect(accountAutomationSchema.safeParse(preference).success).toBe(true);
    for (const invalid of [
      { threshold: Infinity },
      { threshold: 49.9 },
      { priority: 0 },
      { priority: 100 },
      { priority: 1.5 },
      { credentials: 'forbidden' },
    ]) {
      expect(
        accountAutomationSchema.safeParse({ ...preference, ...invalid })
          .success,
      ).toBe(false);
    }
    const claude = snapshot({ returnHoldUntil: 1_800_000_900 });
    claude.accounts[0].automation = preference;
    claude.accounts[0].effectiveThreshold = 82.125;
    expect(snapshotSchema.safeParse(claude).success).toBe(true);
    const codex = codexSnapshot({ returnHoldUntil: 1_800_000_900 });
    codex.accounts[0].automation = preference;
    codex.accounts[0].effectiveThreshold = 82.125;
    expect(codexSnapshotSchema.safeParse(codex).success).toBe(true);
  });
  it('uses each account effective threshold for Claude alerts and keeps the provider summary independent', () => {
    const raw = snapshot({
      settings: { ...defaults, claudeThreshold: 94, codexThreshold: 77 },
      returnHoldUntil: 1_800_000_900,
    });
    raw.accounts[0].automation = {
      enabled: true,
      threshold: 62.125,
      priority: 3,
      returnOnFiveHourReset: true,
    };
    raw.accounts[0].effectiveThreshold = 62.125;
    raw.accounts[0].exhausted = true;
    const view = claudeView(raw, { locale: 'en', now: 1_800_000_000 });
    expect(view.threshold).toBe(94);
    expect(view.active?.effectiveThreshold).toBe(62.125);
    expect(view.next?.effectiveThreshold).toBe(94);
    expect(view.alert).toContain('62.125%');
    expect(view.returnHoldUntil).toBe(1_800_000_900);
  });
  it('uses the Codex account threshold for near-limit status and alert, without borrowing Claude defaults', () => {
    const raw = codexSnapshot({ returnHoldUntil: 1_800_000_900 });
    raw.accounts[0].automation = {
      enabled: true,
      threshold: 50.125,
      priority: 1,
      returnOnFiveHourReset: true,
    };
    raw.accounts[0].effectiveThreshold = 50.125;
    raw.accounts[0].quota!.limits[0].primary!.usedPercent = 60;
    const view = codexView(raw, {
      locale: 'en',
      now: 1_800_000_000,
      settings: { ...defaults, claudeThreshold: 70, codexThreshold: 97.75 },
    });
    expect(view.threshold).toBe(97.75);
    expect(view.active?.effectiveThreshold).toBe(50.125);
    expect(view.active?.status.detail).toContain('Near limit');
    expect(view.alert).toContain('50.125%');
    expect(view.next?.effectiveThreshold).toBe(97.75);
    expect(view.returnHoldUntil).toBe(1_800_000_900);
  });
});

describe('scroll-safe decimal settings parsing', () => {
  it('accepts both decimal separators and keeps exact saved text', () => {
    expect(parsePercent(' 99,125 ')).toBe(99.125);
    expect(parsePercent('99.125')).toBe(99.125);
    expect(parsePercent('099.12500')).toBe(99.125);
    expect(parsePercent('99.1234567890123456789')).toBeNull();
    expect(exactPercent(99.125)).toBe('99.125');
    expect(exactPercent(99.125, 'ro')).toBe('99,125');
    expect(stepPercent('99.5', -1)).toBe('98.5');
    expect(stepPercent('98,125', 1)).toBe('99,125');
    expect(stepPercent('99.5', 1)).toBeNull();
    expect(stepPercent('50.5', -1)).toBeNull();
    expect(parsePriority('')).toBeNull();
    expect(parsePriority('3')).toBe(3);
    expect(parsePriority('1.5')).toBeUndefined();
  });
});
