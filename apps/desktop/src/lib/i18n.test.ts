import { describe, it, expect } from 'vitest';
import { t, plural, subscriptionLabel } from './i18n';
import { percentage, date, countdown, age } from './format';
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
    expect(countdown(86400, 0, 'en')).toContain('1d');
    expect(countdown(86400, 0, 'ro')).toContain('1z');
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
