import { localeName, t, type Language } from './i18n';

export function countdown(
  timestamp: number | null,
  now: number,
  locale: Language = 'en',
): string {
  if (timestamp === null) return t(locale, 'resetUnknown');
  const seconds = Math.max(0, Math.ceil(timestamp - now));
  if (!seconds) return t(locale, 'resetting');
  const days = Math.floor(seconds / 86400),
    hours = Math.floor(seconds / 3600) % 24;
  const minutes = Math.floor(seconds / 60) % 60,
    remainder = seconds % 60;
  const n = (value: number) =>
    new Intl.NumberFormat(localeName(locale)).format(value);
  return days
    ? `${n(days)}${locale === 'ro' ? 'z' : 'd'} ${n(hours)}h ${n(minutes)}m`
    : hours
      ? `${n(hours)}h ${n(minutes)}m ${n(remainder)}s`
      : `${n(minutes)}m ${n(remainder)}s`;
}
export function age(
  timestamp: number | null,
  now: number,
  locale: Language = 'en',
): string {
  if (timestamp === null) return t(locale, 'noReading');
  const delta = Math.max(0, now - timestamp);
  if (delta < 60) return t(locale, 'secondsAgo');
  return new Intl.RelativeTimeFormat(localeName(locale), {
    numeric: 'always',
    style: 'short',
  }).format(
    -Math.floor(delta / (delta < 3600 ? 60 : 3600)),
    delta < 3600 ? 'minute' : 'hour',
  );
}
export function date(
  timestamp: number | null,
  locale: Language = 'en',
): string {
  return timestamp === null
    ? t(locale, 'unknown')
    : new Intl.DateTimeFormat(localeName(locale), {
        day: 'numeric',
        month: 'short',
        year: 'numeric',
      }).format(timestamp * 1000);
}
export function percentage(value: number, locale: Language = 'en'): string {
  return new Intl.NumberFormat(localeName(locale), {
    maximumFractionDigits: 1,
  }).format(value);
}
