import type { Language } from './i18n';

/** Keep the saved decimal visible; Intl's usage rounding is unsuitable for settings. */
export function exactPercent(value: number, locale: Language = 'en'): string {
  return locale === 'ro' ? String(value).replace('.', ',') : String(value);
}

/** Accept either decimal separator, with no rounding, clamping or partial parsing. */
export function parsePercent(text: string): number | null {
  const cleaned = text.trim();
  if (!/^\d+(?:[.,]\d+)?$/.test(cleaned)) return null;
  const value = Number(cleaned.replace(',', '.'));
  const normalized = cleaned
    .replace(',', '.')
    .replace(/^0+(?=\d)/, '')
    .replace(/(\.\d*?)0+$/, '$1')
    .replace(/\.$/, '');
  // Refuse a decimal the runtime cannot preserve exactly as entered.
  return Number.isFinite(value) &&
    value >= 50 &&
    value <= 100 &&
    String(value) === normalized
    ? value
    : null;
}

/** Deliberate one-point steps retain the fractional part instead of snapping to integers. */
export function stepPercent(text: string, direction: -1 | 1): string | null {
  if (parsePercent(text) === null) return null;
  const [whole, fraction] = text.trim().replace(',', '.').split('.');
  const next = `${Number(whole) + direction}${fraction === undefined ? '' : `.${fraction}`}`;
  return parsePercent(next) === null
    ? null
    : text.includes(',')
      ? next.replace('.', ',')
      : next;
}

export function parsePriority(text: string): number | null | undefined {
  if (!text.trim()) return null;
  if (!/^\d+$/.test(text.trim())) return undefined;
  const value = Number(text);
  return Number.isInteger(value) && value >= 1 && value <= 99
    ? value
    : undefined;
}
