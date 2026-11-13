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
/**
 * Time left, rounded up to the minute; one format for every countdown on both tabs:
 * "45m", "2h 10m", "3d 4h" in English, "45 min", "2 h 10 min", "3 zile 4 h" in Romanian.
 */
export function duration(
  timestamp: number,
  now: number,
  locale: Language = 'en',
): string {
  const n = (value: number) => number(value, locale);
  const ro = locale === 'ro';
  const minutes = Math.max(1, Math.ceil((timestamp - now) / 60));
  const days = Math.floor(minutes / 1440),
    hours = Math.floor(minutes / 60) % 24;
  if (days)
    return ro
      ? `${romanianDays(days)} ${n(hours)} h`
      : `${n(days)}d ${n(hours)}h`;
  const parts: string[] = [];
  if (hours) parts.push(ro ? `${n(hours)} h` : `${n(hours)}h`);
  parts.push(ro ? `${n(minutes % 60)} min` : `${n(minutes % 60)}m`);
  return parts.join(' ');
}
type ClaudePlanKind = 'max' | 'pro' | 'team' | 'enterprise';
/** The plan Claude reports as `organization_type`; any other value is unknown. */
function planKind(
  organizationType: string | null | undefined,
): ClaudePlanKind | null {
  switch ((organizationType ?? '').trim().toLowerCase()) {
    case 'claude_max':
      return 'max';
    case 'claude_pro':
      return 'pro';
    case 'claude_team':
      return 'team';
    case 'claude_enterprise':
      return 'enterprise';
    default:
      return null;
  }
}
/**
 * Readable Claude plan. The organization type says what the plan is; the raw
 * rate-limit tier only adds the allowance, because a Team seat and a personal Max
 * can carry the same tier. Without a known type the tier alone decides, as before,
 * and "Team" is never guessed. Unknown tiers stay hidden.
 */
export function planLabel(
  tier: string | null | undefined,
  organizationType?: string | null,
): string | null {
  const value = (tier ?? '').toLowerCase();
  const max = /max[_\s-]*(5|20)\s*[x×]/.exec(value);
  switch (planKind(organizationType)) {
    case 'max':
      return max ? `Max ${max[1]}×` : 'Max';
    case 'pro':
      return 'Pro';
    case 'team':
      return max ? `Team ${max[1]}×` : 'Team';
    case 'enterprise':
      return 'Enterprise';
  }
  if (max) return `Max ${max[1]}×`;
  return /(^|[_\s-])pro($|[_\s-])/.test(value) ? 'Pro' : null;
}
/** Personal (Max, Pro), Team or Enterprise from the organization type; else unknown. */
export function planTypeLabel(
  organizationType: string | null | undefined,
  locale: Language,
): string | null {
  switch (planKind(organizationType)) {
    case 'max':
    case 'pro':
      return t(locale, 'planTypePersonal');
    case 'team':
      return t(locale, 'planTypeTeam');
    case 'enterprise':
      return t(locale, 'planTypeEnterprise');
    default:
      return null;
  }
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
/** Compact local token counts, with the language's decimal separator and units. */
export function compactCount(value: number, locale: Language = 'en'): string {
  return new Intl.NumberFormat(localeName(locale), {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(value);
}
