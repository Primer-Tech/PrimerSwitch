import { describe, it, expect } from 'vitest';
import { t, plural, subscriptionLabel } from './i18n';
import {
  percentage,
  date,
  age,
  duration,
  planLabel,
  planTypeLabel,
} from './format';
import { defaults, settingsSchema } from './types';
import { safeError } from './controller';
import { en } from './locales/en';
import { ro } from './locales/ro';
describe('localized presentation', () => {
  it('defaults to English and dark while preserving saved appearance and fractional settings', () => {
    expect(defaults.language).toBe('en');
    expect(defaults.appearance).toBe('dark');
    expect(
      settingsSchema.parse({
        ...defaults,
        appearance: 'system',
        threshold: 99.5,
      }).threshold,
    ).toBe(99.5);
  });
  it('interpolates account labels and selects Romanian plural forms', () => {
    expect(t('en', 'moreLabel', { name: 'Studio' })).toBe(
      'More actions for Studio',
    );
    expect(plural('en', 'importInvalid', 1)).toBe('1 file cannot be imported.');
    expect(plural('en', 'importInvalid', 2)).toBe(
      '2 files cannot be imported.',
    );
    expect(plural('ro', 'importInvalid', 1)).toContain('1 fișier');
    expect(plural('ro', 'importInvalid', 2)).toContain('2 fișiere');
    expect(plural('ro', 'importInvalid', 20)).toContain('20 de fișiere');
  });
  it('localizes numbers, dates, durations, unknown values and errors', () => {
    expect(percentage(99.5, 'en')).toBe('99.5');
    expect(percentage(99.5, 'ro')).toBe('99,5');
    expect(date(null, 'ro')).toBe(t('ro', 'unknown'));
    expect(date(1700000000, 'en')).not.toBe(date(1700000000, 'ro'));
    // One duration format for every countdown, compact in English and spelled
    // out in Romanian, where day counts agree with their number.
    expect(duration(86400, 0, 'en')).toBe('1d 0h');
    expect(duration(86400, 0, 'ro')).toBe('1 zi 0 h');
    expect(duration(2 * 86400 + 3 * 3600, 0, 'ro')).toBe('2 zile 3 h');
    expect(duration(3661, 0, 'en')).toBe('1h 2m');
    expect(duration(2 * 3600 + 600, 0, 'en')).toBe('2h 10m');
    expect(duration(2 * 3600 + 600, 0, 'ro')).toBe('2 h 10 min');
    expect(duration(45 * 60, 0, 'en')).toBe('45m');
    expect(duration(20 * 86400, 0, 'ro')).toBe('20 de zile 0 h');
    expect(duration(30, 0, 'en')).toBe('1m');
    expect(planLabel('default_claude_max_20x')).toBe('Max 20×');
    expect(planLabel('default_claude_max_5x')).toBe('Max 5×');
    expect(planLabel('default_claude_pro')).toBe('Pro');
    expect(planLabel('max')).toBeNull();
    expect(planLabel('enterprise_custom')).toBeNull();
    expect(planLabel(null)).toBeNull();
    expect(t('ro', 'resetCooldown', { duration: '1 h' })).not.toContain(
      'așteptare',
    );
    expect(t('ro', 'unknownStatus')).toBe('Necunoscut');
    expect(age(null, 0, 'en')).toBe('No reading yet');
    expect(safeError(new Error('access_token secret'), 'ro')).toBe(
      t('ro', 'failedAction'),
    );
    expect(safeError(t('en', 'externalChange'), 'ro')).toBe(
      t('ro', 'externalChange'),
    );
    expect(subscriptionLabel('active', 'ro')).toBe('Activ');
  });
  it('labels the real Claude plan from the organization type and the tier', () => {
    const max5 = 'default_claude_max_5x';
    const max20 = 'default_claude_max_20x';
    // The two real cases: a Team seat and a personal Max share one tier string.
    expect(planLabel(max5, 'claude_team')).toBe('Team 5×');
    expect(planLabel(max5, 'claude_max')).toBe('Max 5×');
    expect(planLabel(max20, 'claude_team')).toBe('Team 20×');
    expect(planLabel(max20, 'claude_max')).toBe('Max 20×');
    // The type decides the plan; the tier only adds an allowance it recognises.
    expect(planLabel(null, 'claude_max')).toBe('Max');
    expect(planLabel('default_raven', 'claude_max')).toBe('Max');
    expect(planLabel(null, 'claude_team')).toBe('Team');
    expect(planLabel('default_raven', 'claude_team')).toBe('Team');
    expect(planLabel(max5, 'claude_pro')).toBe('Pro');
    expect(planLabel(null, 'claude_pro')).toBe('Pro');
    expect(planLabel(max5, 'claude_enterprise')).toBe('Enterprise');
    expect(planLabel(null, 'claude_enterprise')).toBe('Enterprise');
    expect(planLabel(max5, ' Claude_Team ')).toBe('Team 5×');
    // No known type: today's tier-only behaviour, and Team is never invented.
    for (const unknown of [undefined, null, '', 'claude_edu', 'team', 'max']) {
      expect(planLabel(max5, unknown)).toBe('Max 5×');
      expect(planLabel(max20, unknown)).toBe('Max 20×');
      expect(planLabel('default_claude_pro', unknown)).toBe('Pro');
      expect(planLabel('default_raven', unknown)).toBeNull();
      expect(planLabel(null, unknown)).toBeNull();
    }
  });
  it('names the plan type in both languages and hides it while unknown', () => {
    expect(planTypeLabel('claude_max', 'en')).toBe('Personal');
    expect(planTypeLabel('claude_pro', 'en')).toBe('Personal');
    expect(planTypeLabel('claude_team', 'en')).toBe('Team');
    expect(planTypeLabel('claude_enterprise', 'en')).toBe('Enterprise');
    expect(planTypeLabel('claude_max', 'ro')).toBe(t('ro', 'planTypePersonal'));
    expect(planTypeLabel('claude_team', 'ro')).toBe(t('ro', 'planTypeTeam'));
    expect(planTypeLabel('claude_enterprise', 'ro')).toBe(
      t('ro', 'planTypeEnterprise'),
    );
    expect(t('en', 'detailsPlanType')).toBe('Plan type');
    expect(t('ro', 'detailsPlanType')).toBe('Tip de plan');
    for (const unknown of [undefined, null, '', 'claude_edu', 'personal']) {
      expect(planTypeLabel(unknown, 'en')).toBeNull();
      expect(planTypeLabel(unknown, 'ro')).toBeNull();
    }
  });
  it('uses one vocabulary for both providers in both languages', () => {
    expect(Object.keys(ro).sort()).toEqual(Object.keys(en).sort());
    // Romanian uses comma-below ș and ț, never the cedilla forms.
    const romanian = Object.values(ro).join('');
    expect(romanian).not.toMatch(/[ŞşŢţ]/);
    expect(romanian).toMatch(/[șț]/);
    for (const [key, text] of Object.entries(ro))
      expect(text.trim(), key).not.toBe('');
    // The shared page speaks of the same windows and statuses on both tabs.
    expect(
      ['fiveHourWindow', 'weekly', 'nextUp', 'usageOrder'].map((key) =>
        t('en', key as keyof typeof en),
      ),
    ).toEqual(['5-hour window', 'Weekly', 'Next up', 'Next in order']);
    expect(t('ro', 'fiveHourWindow')).toBe('Fereastra de 5 ore');
  });
});
