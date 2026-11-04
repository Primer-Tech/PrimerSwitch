import { render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import AccountDetails from './components/AccountDetails.svelte';
import ActiveAccountCard from './components/ActiveAccountCard.svelte';
import { codexView } from './lib/codex-view';
import { compactCount, date } from './lib/format';
import { language, t, type Language } from './lib/i18n';
import type { CodexAccount } from './lib/codex-types';
import { defaults } from './lib/types';
import {
  codexAccount,
  codexApiKeyAccount,
  codexKeyUsage,
  codexSnapshot,
} from './test/codex-fixtures';

const now = 1_800_000_000;
function show(
  surface: 'details' | 'active',
  account: CodexAccount,
  locale: Language = 'en',
) {
  language.set(locale);
  const page = codexView(
    codexSnapshot({
      accounts: [{ ...account, selected: true }],
      selectedId: account.id,
      nextId: null,
      order: [],
    }),
    { locale, now, settings: defaults },
  );
  const shared = {
    now,
    locked: false,
    lockReason: null,
    onrefresh: vi.fn(),
    onswitch: vi.fn(),
    onsignin: vi.fn(),
  };
  if (surface === 'details')
    return render(AccountDetails, {
      ...shared,
      row: page.active!,
      threshold: page.threshold,
      ondelete: vi.fn(),
      onautomation: async () => null,
    });
  return render(ActiveAccountCard, {
    ...shared,
    view: page,
    empty: { title: 'No active account', text: null },
    ondetails: vi.fn(),
  });
}
afterEach(() => language.set('en'));

describe.each(['details', 'active'] as const)(
  'Local key consumption in %s',
  (surface) => {
    it.each(['en', 'ro'] as const)(
      'shows all native counters and local scope in %s',
      (locale) => {
        const usage = codexKeyUsage();
        const { container } = show(
          surface,
          codexApiKeyAccount('codex-key', { usage }),
          locale,
        );
        const consumption = screen.getByRole('region', {
          name: t(locale, 'keyConsumption'),
        });
        const block = within(consumption);
        const windows = [
          ['keyUsage24h', ['1.2K', '3.4K', '500', '5.1K']],
          ['keyUsage7d', ['12K', '34K', '5K', '51K']],
          ['keyUsage30d', ['1.2M', '3.4M', '500K', '5.1M']],
          ['keyUsageTotal', ['120M', '340M', '1.1B', '1.6B']],
        ] as const;
        for (const [label, counts] of windows) {
          const window = block.getByRole('heading', {
            name: t(locale, label),
          }).parentElement!;
          const definitions = Array.from(
            window.querySelectorAll('dd'),
            (element) => element.textContent,
          );
          expect(definitions).toEqual(
            locale === 'en'
              ? counts
              : Object.values(
                  usage.windows[
                    label === 'keyUsage24h'
                      ? 'last24h'
                      : label === 'keyUsage7d'
                        ? 'last7d'
                        : label === 'keyUsage30d'
                          ? 'last30d'
                          : 'total'
                  ],
                ).map((count) => compactCount(count, 'ro')),
          );
          expect(
            within(window).getByText(t(locale, 'keyUsageInput')),
          ).toBeVisible();
          expect(
            within(window).getByText(t(locale, 'keyUsageCached')),
          ).toBeVisible();
          expect(
            within(window).getByText(t(locale, 'keyUsageOutput')),
          ).toBeVisible();
          expect(
            within(window).getByText(t(locale, 'keyUsageTokensTotal')),
          ).toBeVisible();
        }
        expect(
          block.getByText(
            t(locale, 'keyUsageSince', {
              date: date(usage.trackedSince, locale),
            }),
          ),
        ).toBeVisible();
        expect(consumption).toHaveTextContent(t(locale, 'keyUsageLocal'));
        expect(consumption).toHaveTextContent(t(locale, 'keyUsageAuthority'));
        expect(screen.getByText(t(locale, 'codexApiQuota'))).toBeVisible();
        expect(screen.queryByRole('meter')).toBeNull();
        expect(consumption).not.toHaveTextContent('%');
        expect(container.querySelector('.usage-meters')).toBeNull();
        expect(block.queryByText(t(locale, 'keyUsageEmpty'))).toBeNull();
      },
    );

    it.each(['zero', 'absent', 'null'] as const)(
      'shows an honest empty state for %s usage',
      (kind) => {
        const key = codexApiKeyAccount();
        if (kind === 'absent') delete key.usage;
        if (kind === 'null') key.usage = null;
        show(surface, key);
        const consumption = screen.getByRole('region', { name: 'Consumption' });
        expect(
          within(consumption).getByText('No usage counted yet'),
        ).toBeVisible();
        expect(consumption.querySelector('dl')).toBeNull();
        expect(consumption).not.toHaveTextContent('Tracking since');
        expect(consumption).not.toHaveTextContent('%');
        expect(screen.queryByRole('meter')).toBeNull();
      },
    );

    it.each(['scanning', 'unavailable'] as const)(
      'keeps saved counters while %s and localizes the status',
      (state) => {
        show(
          surface,
          codexApiKeyAccount('codex-key', { usage: codexKeyUsage({ state }) }),
          'ro',
        );
        const consumption = screen.getByRole('region', { name: 'Consum' });
        expect(within(consumption).getByRole('status')).toHaveTextContent(
          t(
            'ro',
            state === 'scanning' ? 'keyUsageScanning' : 'keyUsageUnavailable',
          ),
        );
        expect(consumption.querySelectorAll('dd')).toHaveLength(16);
        expect(consumption).toHaveTextContent('Intrare (fără cache)');
        expect(consumption).toHaveTextContent('Ieșire');
        expect(consumption).toHaveTextContent('Urmărire din');
        expect(consumption).not.toHaveTextContent('%');
      },
    );

    it('localizes the empty state and the scope in Romanian', () => {
      show(surface, codexApiKeyAccount(), 'ro');
      const consumption = screen.getByRole('region', { name: 'Consum' });
      expect(
        within(consumption).getByText('Niciun consum contorizat încă'),
      ).toBeVisible();
      expect(consumption).toHaveTextContent(
        'Contorizat pe acest PC cât timp această cheie a fost activă.',
      );
      expect(consumption).toHaveTextContent(
        'Pagina de utilizare OpenAI este sursa oficială pentru facturare.',
      );
      expect(consumption.querySelector('dl')).toBeNull();
      expect(screen.queryByRole('meter')).toBeNull();
    });

    it('shows an established tracking date without inventing zero counters', () => {
      const key = codexApiKeyAccount();
      key.usage!.trackedSince = now;
      show(surface, key);
      const consumption = screen.getByRole('region', { name: 'Consumption' });
      expect(
        within(consumption).getByText('No usage counted yet'),
      ).toBeVisible();
      expect(consumption).toHaveTextContent(
        t('en', 'keyUsageSince', { date: date(now, 'en') }),
      );
      expect(consumption.querySelector('dl')).toBeNull();
      expect(screen.queryByRole('meter')).toBeNull();
    });

    it('leaves ChatGPT plan meters and hides key consumption', () => {
      show(surface, codexAccount());
      expect(screen.queryByRole('region', { name: 'Consumption' })).toBeNull();
      expect(screen.getAllByRole('meter')).toHaveLength(2);
      expect(
        screen.getByRole('meter', { name: '5-hour window' }),
      ).toHaveAttribute('aria-valuenow', '20');
    });
  },
);

it('formats compact token counts with localized decimals and units', () => {
  expect(compactCount(1_200, 'en')).toBe('1.2K');
  expect(compactCount(3_400_000, 'en')).toBe('3.4M');
  expect(compactCount(1_100_000_000, 'en')).toBe('1.1B');
  expect(compactCount(1_200, 'ro')).toBe('1,2\u00a0K');
  expect(compactCount(3_400_000, 'ro')).toBe('3,4\u00a0mil.');
  expect(compactCount(1_100_000_000, 'ro')).toBe('1,1\u00a0mld.');
});
