import { describe, it, expect } from 'vitest';
import { t, plural, subscriptionLabel } from './i18n';
import { percentage, date, age, duration, planLabel } from './format';
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
