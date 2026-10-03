import { describe, it, expect } from 'vitest';
import { t, plural, subscriptionLabel } from './i18n';
import {
  percentage,
  date,
  countdown,
  age,
  duration,
  planLabel,
} from './format';
import { defaults, settingsSchema } from './types';
import { safeError } from './controller';
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
    expect(t('en', 'detailsLabel', { name: 'Studio' })).toContain('Studio');
    expect(plural('en', 'importInvalid', 1)).toBe('1 file cannot be imported.');
    expect(plural('en', 'importInvalid', 2)).toBe(
      '2 files cannot be imported.',
    );
    expect(plural('ro', 'importInvalid', 1)).toContain('1 fișier');
    expect(plural('ro', 'importInvalid', 2)).toContain('2 fișiere');
    expect(plural('ro', 'importInvalid', 20)).toContain('20 de fișiere');
  });
  it('localizes numbers, dates, duration units, unknown values and errors', () => {
    expect(percentage(99.5, 'en')).toBe('99.5');
    expect(percentage(99.5, 'ro')).toBe('99,5');
    expect(date(null, 'ro')).toBe(t('ro', 'unknown'));
    expect(date(1700000000, 'en')).not.toBe(date(1700000000, 'ro'));
    expect(countdown(86400, 0, 'en')).toBe('1d 0h 0m');
    expect(countdown(3661, 0, 'en')).toBe('1h 1m 1s');
    // Romanian units are spelled out and agree with their number.
    expect(countdown(86400, 0, 'ro')).toBe('1 zi 0 h 0 min');
    expect(countdown(2 * 86400 + 3 * 3600, 0, 'ro')).toBe('2 zile 3 h 0 min');
    expect(countdown(3661, 0, 'ro')).toBe('1 h 1 min 1 s');
    expect(duration(2 * 3600 + 600, 0, 'en')).toBe('2h 10m');
    expect(duration(2 * 3600 + 600, 0, 'ro')).toBe('2 h 10 min');
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
    expect(t('ro', 'consumptionOrder')).toBe(t('ro', 'consumptionOrderShort'));
    expect(age(null, 0, 'en')).toBe('No reading yet');
    expect(safeError(new Error('access_token secret'), 'ro')).toBe(
      t('ro', 'failedAction'),
    );
    expect(safeError(t('en', 'externalChange'), 'ro')).toBe(
      t('ro', 'externalChange'),
    );
    expect(subscriptionLabel('active', 'ro')).toBe('Activ');
  });
});
