import { t, type Language, type MessageKey } from './i18n';
import type { CodexError } from './codex-controller';
import type { CodexLimit } from './codex-types';
export function codexMessage(
  reason: CodexError | null,
  locale: Language,
): string {
  if (reason === 'unsafeData') return t(locale, 'unsafeData');
  if (!reason || reason === 'actionFailed')
    return t(locale, 'codexActionFailed');
  return t(locale, ('codexReason_' + reason) as MessageKey);
}
export function codexWindowLabel(
  minutes: number | null,
  secondary: boolean,
  locale: Language,
): string {
  if (minutes === null)
    return t(locale, secondary ? 'codexSecondaryWindow' : 'codexPrimaryWindow');
  if (locale === 'ro') {
    const unit =
      minutes % 1440 === 0 ? 'day' : minutes % 60 === 0 ? 'hour' : 'minute';
    const value =
      unit === 'day'
        ? minutes / 1440
        : unit === 'hour'
          ? minutes / 60
          : minutes;
    return t(locale, 'codexWindowDuration', {
      duration: new Intl.NumberFormat('ro-RO', {
        style: 'unit',
        unit,
        unitDisplay: 'long',
      }).format(value),
    });
  }
  const n = new Intl.NumberFormat('en-US').format(
    minutes % 1440 === 0
      ? minutes / 1440
      : minutes % 60 === 0
        ? minutes / 60
        : minutes,
  );
  return t(
    locale,
    minutes % 1440 === 0
      ? 'codexWindowDays'
      : minutes % 60 === 0
        ? 'codexWindowHours'
        : 'codexWindowMinutes',
    { count: n },
  );
}
export function codexLimitName(limit: CodexLimit, locale: Language): string {
  return (
    limit.limitName ??
    limit.normalModelSlug ??
    limit.limitId ??
    t(locale, 'codexQuotaGroup')
  );
}
