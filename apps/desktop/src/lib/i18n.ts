import { writable } from 'svelte/store';
import { en } from './locales/en';
import { ro } from './locales/ro';

export type Language = 'en' | 'ro';
export type MessageKey = keyof typeof en;
export const language = writable<Language>('en');
export const localeName = (locale: Language): string =>
  locale === 'ro' ? 'ro-RO' : 'en-US';
export function t(
  locale: Language,
  key: MessageKey,
  values: Record<string, string | number> = {},
): string {
  const template = (locale === 'ro' ? ro : en)[key];
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    String(values[name] ?? `{${name}}`),
  );
}
const plurals = {
  en: {
    importReady: {
      one: '{count} account can be imported. Original files will be retained.',
      other:
        '{count} accounts can be imported. Original files will be retained.',
    },
    importInvalid: {
      one: '{count} file cannot be imported.',
      other: '{count} files cannot be imported.',
    },
  },
  ro: {
    importReady: {
      one: '{count} cont poate fi importat. Fișierele originale vor fi păstrate.',
      few: '{count} conturi pot fi importate. Fișierele originale vor fi păstrate.',
      other:
        '{count} de conturi pot fi importate. Fișierele originale vor fi păstrate.',
    },
    importInvalid: {
      one: '{count} fișier nu poate fi importat.',
      few: '{count} fișiere nu pot fi importate.',
      other: '{count} de fișiere nu pot fi importate.',
    },
  },
};
export function plural(
  locale: Language,
  key: 'importReady' | 'importInvalid',
  count: number,
): string {
  const category = new Intl.PluralRules(localeName(locale)).select(count);
  const forms: Partial<Record<Intl.LDMLPluralRule, string>> =
    plurals[locale][key];
  return (forms[category] ?? forms.other!).replace(
    '{count}',
    new Intl.NumberFormat(localeName(locale)).format(count),
  );
}
/** Native errors remain redacted strings; recognized stable messages localize without parsing response bodies. */
export function localizeNativeMessage(text: string, locale: Language): string {
  const entry = (Object.keys(en) as MessageKey[]).find(
    (key) => en[key] === text || ro[key] === text,
  );
  return entry ? t(locale, entry) : text;
}
export function isCatalogMessage(text: string): boolean {
  return (
    Object.values(en).includes(text as (typeof en)[MessageKey]) ||
    Object.values(ro).includes(text)
  );
}

export function subscriptionLabel(
  status: string | null,
  locale: Language,
): string {
  const normalized = status?.toLowerCase();
  const key =
    normalized === 'active' || normalized === 'activ'
      ? 'active'
      : normalized === 'inactive'
        ? 'subscriptionInactive'
        : normalized === 'paused'
          ? 'subscriptionPaused'
          : normalized === 'canceled' || normalized === 'cancelled'
            ? 'subscriptionCanceled'
            : normalized === 'trial' || normalized === 'trialing'
              ? 'subscriptionTrial'
              : null;
  return key ? t(locale, key) : (status ?? t(locale, 'unknown'));
}
