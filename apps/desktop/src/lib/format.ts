import { localeName, t, type Language } from './i18n';

const number = (value: number, locale: Language) =>
  new Intl.NumberFormat(localeName(locale)).format(value);
/** Romanian day counts agree with their number: 1 zi, 2 zile, 20 de zile. */
function romanianDays(days: number): string {
  const category = new Intl.PluralRules('ro-RO').select(days);
  const n = number(days, 'ro');
  return category === 'one'
    ? `${n} zi`
    : category === 'few'
      ? `${n} zile`
      : `${n} de zile`;
}
/** Units for a countdown: compact in English, spelled out in Romanian. */
function units(
  locale: Language,
  days: number,
  hours: number,
  minutes: number,
  seconds: number | null,
): string {
  const n = (value: number) => number(value, locale);
  const ro = locale === 'ro';
  const parts: string[] = [];
  if (days) parts.push(ro ? romanianDays(days) : `${n(days)}d`);
  if (days || hours) parts.push(ro ? `${n(hours)} h` : `${n(hours)}h`);
  parts.push(ro ? `${n(minutes)} min` : `${n(minutes)}m`);
  if (seconds !== null && !days)
    parts.push(ro ? `${n(seconds)} s` : `${n(seconds)}s`);
  return parts.join(' ');
}

export function countdown(
  timestamp: number | null,
  now: number,
  locale: Language = 'en',
): string {
  if (timestamp === null) return t(locale, 'resetUnknown');
  const seconds = Math.max(0, Math.ceil(timestamp - now));
  if (!seconds) return t(locale, 'resetting');
  return units(
    locale,
    Math.floor(seconds / 86400),
    Math.floor(seconds / 3600) % 24,
    Math.floor(seconds / 60) % 60,
    seconds % 60,
  );
}
/** Coarse time left, rounded up to the minute: "2h 10m" / "2 h 10 min". */
export function duration(
  timestamp: number,
  now: number,
  locale: Language = 'en',
): string {
  const minutes = Math.max(1, Math.ceil((timestamp - now) / 60));
  const days = Math.floor(minutes / 1440),
    hours = Math.floor(minutes / 60) % 24;
  if (days)
    return locale === 'ro'
      ? `${romanianDays(days)} ${number(hours, locale)} h`
      : `${number(days, locale)}d ${number(hours, locale)}h`;
  return units(locale, 0, hours, minutes % 60, null);
}
/** Readable plan from Claude's raw rate-limit tier; unknown tiers stay hidden. */
export function planLabel(tier: string | null | undefined): string | null {
  const value = (tier ?? '').toLowerCase();
  const max = /max[_\s-]*(5|20)\s*[x×]/.exec(value);
  if (max) return `Max ${max[1]}×`;
  return /(^|[_\s-])pro($|[_\s-])/.test(value) ? 'Pro' : null;
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
