import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import App from './App.svelte';
import { createController } from './lib/controller';
import { createCodexController } from './lib/codex-controller';
import type { CodexBridge } from './lib/codex-bridge';
import type { CodexSnapshot } from './lib/codex-types';
import {
  codexAccount,
  codexAccounts,
  codexApiKeyAccount,
  codexCapability,
  codexDemoSnapshot,
  codexMainLimit,
  codexSnapshot,
} from './test/codex-fixtures';
import { snapshot } from './test/fixtures';
import { codexClock } from './lib/codex-format';
import { language, t } from './lib/i18n';
import type { Settings } from './lib/types';
async function setup(
  raw = codexSnapshot(),
  locale: 'en' | 'ro' = 'en',
  settings: Partial<Settings> = {},
) {
  const claude = snapshot();
  claude.settings = { ...claude.settings, ...settings, language: locale };
  claude.demo = raw.demo;
  let emit: (value: unknown) => void = () => {};
  const claudeCall = vi.fn().mockResolvedValue(claude),
    call = vi.fn<CodexBridge['call']>().mockResolvedValue(raw);
  const controller = createController({
      call: claudeCall,
      subscribe: async () => () => {},
    }),
    codexController = createCodexController({
      call,
      subscribe: async (callback) => {
        emit = callback;
        return () => {};
      },
    });
  render(App, { controller, codexController });
  await screen.findAllByText('a@example.invalid');
  const tab = screen.getByRole('tab', { name: 'Codex' });
  tab.focus();
  await fireEvent.click(tab);
  await screen.findByText(t(locale, 'codexSubtitle'));
  await waitFor(() => expect(get(codexController).snapshot).not.toBeNull());
  await waitFor(() => expect(get(codexController).pending).toBeNull());
  return {
    call,
    claudeCall,
    codexController,
    emit: (value: unknown) => emit(value),
  };
}
function deferred() {
  let resolve!: (value: unknown) => void;
  const promise = new Promise<unknown>((ok) => (resolve = ok));
  return { promise, resolve };
}
const row = (email: string) =>
  within(screen.getByRole('table')).getByText(email).closest('tr')!;
const section = (heading: string) =>
  within(screen.getByRole('heading', { name: heading }).closest('section')!);
const activeCard = () => section('Active account');
function switched(
  id: string,
  revision: number,
  outcome: Partial<NonNullable<CodexSnapshot['lastSwitch']>> = {},
) {
  const raw = codexSnapshot({ revision, selectedId: id });
  raw.accounts.forEach((account) => (account.selected = account.id === id));
  raw.lastSwitch = {
    accountId: id,
    at: Math.floor(Date.now() / 1000),
    daemonRestarted: true,
    otherClients: 0,
    error: null,
    ...outcome,
  };
  return raw;
}
afterEach(() => language.set('en'));
describe('Codex accounts page', () => {
  it.each(['plus', 'team', 'business'])(
    'offers the same manual reset for a %s account',
    async (planType) => {
      const raw = codexSnapshot();
      const account = raw.accounts[0];
      account.planType = planType;
      account.quota!.resetCreditsAvailable = 2;
      const firstExpiry = Date.UTC(2030, 4, 20, 12, 34) / 1000;
      const secondExpiry = firstExpiry + 7 * 86400;
      account.quota!.resetCreditDetails = [
        { expiresAt: firstExpiry },
        { expiresAt: secondExpiry },
      ];
      account.reset.usable = codexCapability();
      const { call } = await setup(raw);
      const resets = section('Available resets');
      expect(resets.getByText('2 resets available')).toBeVisible();
      const result = structuredClone(raw);
      result.revision++;
      result.accounts[0].reset.lastOutcome = 'reset';
      result.accounts[0].quota!.resetCreditsAvailable = 1;
      result.accounts[0].quota!.resetCreditDetails = [
        { expiresAt: secondExpiry },
      ];
      result.accounts[0].quota!.limits[0].primary!.usedPercent = 0;
      const pending = deferred();
      call.mockReturnValueOnce(pending.promise);
      const button = resets.getByRole('button', { name: 'Use one reset' });
      await fireEvent.click(button);
      expect(call).toHaveBeenLastCalledWith('codex_consume_reset', {
        id: account.id,
      });
      expect(
        resets.getByRole('button', { name: 'Resetting usage…' }),
      ).toBeDisabled();
      expect(
        resets.getByRole('button', { name: 'Refresh resets' }),
      ).toBeDisabled();
      const before = call.mock.calls.filter(
        ([command]) => command === 'codex_consume_reset',
      ).length;
      await fireEvent.click(button);
      expect(
        call.mock.calls.filter(
          ([command]) => command === 'codex_consume_reset',
        ),
      ).toHaveLength(before);
      pending.resolve(result);
      await resets.findByText('1 resets available');
      const expiryList = resets.getByRole('list', {
        name: 'Reset expiry dates',
      });
      expect(expiryList.querySelectorAll('time')).toHaveLength(1);
      expect(expiryList.querySelector('time')).toHaveAttribute(
        'datetime',
        new Date(secondExpiry * 1000).toISOString(),
      );
      expect(await screen.findAllByText('Reset applied.')).not.toHaveLength(0);
      expect(
        activeCard().getByRole('meter', { name: '5-hour window' }),
      ).toHaveAttribute('aria-valuenow', '0');
    },
  );
  it.each(['en', 'ro'] as const)(
    'shows exact localized reset expiry dates and explicit non-expiring credits (%s)',
    async (locale) => {
      const raw = codexSnapshot();
      const expiry = Date.UTC(2030, 4, 20, 12, 34) / 1000;
      raw.accounts[0].quota!.resetCreditsAvailable = 3;
      raw.accounts[0].quota!.resetCreditDetails = [
        { expiresAt: null },
        { expiresAt: expiry },
        { expiresAt: expiry },
      ];
      await setup(raw, locale);
      const resets = section(t(locale, 'availableResets'));
      const list = resets.getByRole('list', {
        name: t(locale, 'codexResetExpiryDates'),
      });
      const rows = within(list).getAllByRole('listitem');
      expect(rows).toHaveLength(2);
      expect(
        within(rows[0]).getByText(locale === 'ro' ? '2 resetări' : '2 resets'),
      ).toBeVisible();
      const time = rows[0].querySelector('time')!;
      expect(time).toHaveAttribute(
        'datetime',
        new Date(expiry * 1000).toISOString(),
      );
      expect(time.textContent).toContain('2030');
      expect(time.textContent).toContain(locale === 'ro' ? 'mai' : 'May');
      expect(time.textContent).toMatch(/\d{1,2}:\d{2}/);
      expect(
        within(rows[1]).getByText(t(locale, 'codexResetNoExpiry')),
      ).toBeVisible();
      expect(
        resets.queryByText(t(locale, 'codexResetExpiryUnknown')),
      ).toBeNull();
    },
  );
  it.each([null, [], undefined])(
    'keeps unavailable expiry details distinct from non-expiring credits (%s)',
    async (details) => {
      const raw = codexSnapshot();
      raw.accounts[0].quota!.resetCreditsAvailable = 3;
      raw.accounts[0].quota!.resetCreditDetails = details;
      await setup(raw);
      const resets = section('Available resets');
      expect(resets.getByText('3 resets available')).toBeVisible();
      expect(resets.getByText('Expiry dates unavailable.')).toBeVisible();
      expect(resets.queryByText('Does not expire')).toBeNull();
      expect(resets.queryByRole('list')).toBeNull();
    },
  );
  it('shows partial expiry details without replacing the authoritative available count', async () => {
    const raw = codexSnapshot();
    raw.accounts[0].quota!.resetCreditsAvailable = 4;
    raw.accounts[0].quota!.resetCreditDetails = [
      { expiresAt: Date.UTC(2030, 4, 20, 12, 34) / 1000 },
    ];
    await setup(raw);
    const resets = section('Available resets');
    expect(resets.getByText('4 resets available')).toBeVisible();
    expect(
      resets.getByText('Expiry details available for 1 of 4 resets.'),
    ).toBeVisible();
    expect(resets.queryByText('Does not expire')).toBeNull();
  });
  it.each([
    [null, 'Reset availability unknown', 'resetUnavailable'],
    [0, '0 resets available', 'noResetCredits'],
  ] as const)(
    'keeps unknown and empty Teams reset states visible (%s)',
    async (count, label, reason) => {
      const raw = codexSnapshot();
      raw.accounts[0].planType = 'team';
      raw.accounts[0].quota!.resetCreditsAvailable = count;
      raw.accounts[0].reset.usable = codexCapability(reason);
      const { call } = await setup(raw);
      const resets = section('Available resets');
      expect(resets.getByText(label)).toBeVisible();
      expect(
        resets.getByRole('button', { name: 'Use one reset' }),
      ).toBeDisabled();
      await fireEvent.click(
        resets.getByRole('button', { name: 'Refresh resets' }),
      );
      expect(call).toHaveBeenLastCalledWith('codex_refresh_account', {
        id: raw.accounts[0].id,
      });
    },
  );
  it('shows the Romanian reset action and lets an uncertain reset be checked with zero credits', async () => {
    const raw = codexSnapshot();
    raw.accounts[0].planType = 'team';
    raw.accounts[0].quota!.resetCreditsAvailable = 0;
    raw.accounts[0].reset = {
      pending: true,
      lastOutcome: null,
      usable: codexCapability(),
    };
    const { call } = await setup(raw, 'ro');
    const resets = section('Resetări disponibile');
    expect(resets.getByText('Resetări disponibile: 0')).toBeVisible();
    const result = structuredClone(raw);
    result.revision++;
    result.accounts[0].reset.pending = false;
    result.accounts[0].reset.lastOutcome = 'alreadyRedeemed';
    result.accounts[0].reset.usable = codexCapability('noResetCredits');
    call.mockResolvedValueOnce(result);
    await fireEvent.click(
      resets.getByRole('button', { name: 'Verifică resetarea anterioară' }),
    );
    expect(call).toHaveBeenLastCalledWith('codex_consume_reset', {
      id: raw.accounts[0].id,
    });
    expect(
      await screen.findAllByText('Resetarea a fost aplicată.'),
    ).not.toHaveLength(0);
    expect(
      resets.getByRole('button', { name: 'Folosește o resetare' }),
    ).toBeDisabled();
  });
  it('shows the reset controls in an inactive account details dialog', async () => {
    const raw = codexSnapshot();
    raw.accounts[1].planType = 'team';
    raw.accounts[1].quota!.resetCreditsAvailable = 1;
    const expiry = Date.UTC(2030, 4, 20, 12, 34) / 1000;
    raw.accounts[1].quota!.resetCreditDetails = [{ expiresAt: expiry }];
    raw.accounts[1].reset.usable = codexCapability();
    const { call } = await setup(raw);
    await fireEvent.click(
      within(row('personal@example.invalid')).getByRole('button', {
        name: /More/,
      }),
    );
    await fireEvent.click(
      within(row('personal@example.invalid')).getByRole('button', {
        name: 'Details',
      }),
    );
    const dialog = within(screen.getByRole('dialog'));
    expect(dialog.getByText('1 resets available')).toBeVisible();
    expect(
      dialog
        .getByRole('list', { name: 'Reset expiry dates' })
        .querySelector('time'),
    ).toHaveAttribute('datetime', new Date(expiry * 1000).toISOString());
    await fireEvent.click(
      dialog.getByRole('button', { name: 'Use one reset' }),
    );
    expect(call).toHaveBeenLastCalledWith('codex_consume_reset', {
      id: raw.accounts[1].id,
    });
    expect(
      call.mock.calls.some(([command]) => command === 'codex_switch_account'),
    ).toBe(false);
  });
  it('shows every saved account with both windows, plan and a plain-language status', async () => {
    await setup();
    const table = screen.getByRole('table');
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((header) => header.textContent?.trim()),
    ).toEqual(['Account', '5-hour window', 'Weekly', 'Status', 'Actions']);
    const personal = within(row('personal@example.invalid'));
    expect(
      personal.getByRole('meter', { name: '5-hour window' }),
    ).toHaveAttribute('aria-valuenow', '100');
    expect(personal.getByRole('meter', { name: 'Weekly' })).toHaveAttribute(
      'aria-valuenow',
      '62',
    );
    expect(personal.getByText('Plus')).toBeVisible();
    expect(personal.getByText('Limit reached')).toBeVisible();
    expect(personal.getByText(/^Frees in 2h 1\dm$/)).toBeVisible();
    const research = within(row('research@example.invalid'));
    expect(research.getByText('Pro Lite')).toBeVisible();
    expect(research.getByText('Next')).toBeVisible();
    expect(research.getByRole('button', { name: 'Switch' })).toBeEnabled();
    expect(
      research.getByRole('button', { name: 'Switch' }),
    ).toHaveAccessibleDescription('research@example.invalid');
    const studio = within(row('studio@example.invalid'));
    expect(studio.getByText('Active')).toBeVisible();
    expect(studio.queryByRole('button', { name: /^Switch/ })).toBeNull();
    // The active account first, then the native usage order, then the rest.
    expect(
      within(table)
        .getAllByRole('row')
        .slice(1)
        .map((tr) => tr.querySelector('strong')?.textContent),
    ).toEqual([
      'studio@example.invalid',
      'research@example.invalid',
      'personal@example.invalid',
    ]);
    const active = activeCard();
    expect(active.getByRole('heading', { level: 3 })).toHaveTextContent(
      'studio@example.invalid',
    );
    expect(
      active.getByRole('meter', { name: '5-hour window' }),
    ).toHaveAttribute('aria-valuenow', '40');
    expect(active.getByRole('meter', { name: 'Weekly' })).toHaveAttribute(
      'aria-valuenow',
      '35',
    );
    expect(
      active.getByRole('meter', { name: 'Weekly · Code review' }),
    ).toHaveAttribute('aria-valuenow', '12');
    expect(active.queryByRole('meter', { name: /· Codex$/ })).toBeNull();
    expect(active.getByText('12.50')).toBeVisible();
    const setupCard = section('Codex setup');
    expect(setupCard.getByText('Codex 0.160.0 · ready')).toBeVisible();
    expect(setupCard.getByText(t('en', 'codexHowSaves'))).toBeVisible();
    expect(
      within(screen.getByRole('region', { name: 'Next in order' })).getByText(
        'research@example.invalid',
      ),
    ).toBeVisible();
    expect(
      section('Automation').getByText(t('en', 'codexEnvDaemon')),
    ).toBeVisible();
  });
  it('drops the close-your-clients model and its jargon', async () => {
    await setup();
    for (const retired of [
      /Close Codex/i,
      /Manual account selection/i,
      /Selected for new clients/i,
      /Read-only/i,
      /Included usage permission is unknown/i,
      /Quota ownership/i,
      /FILE credential storage/i,
      /Next best/i,
    ])
      expect(screen.queryByText(retired)).toBeNull();
    expect(screen.queryByRole('checkbox')).toBeNull();
    expect(screen.queryByRole('dialog')).toBeNull();
  });
  it('recommends the next account and switches with one call, showing progress then success', async () => {
    const { call, emit } = await setup();
    const next = section('Next up');
    expect(next.getByText('research@example.invalid')).toBeVisible();
    const result = deferred();
    call.mockReturnValueOnce(result.promise);
    await fireEvent.click(
      next.getByRole('button', {
        name: 'Switch now to research@example.invalid',
      }),
    );
    expect(call).toHaveBeenCalledWith('codex_switch_account', {
      id: 'codex-research',
    });
    expect(
      await screen.findByText('Switching to research@example.invalid'),
    ).toBeVisible();
    expect(screen.getByText('Saving the new sign-in…')).toBeVisible();
    expect(screen.getByRole('button', { name: /Add account/ })).toBeDisabled();
    expect(
      within(row('personal@example.invalid')).getByRole('button', {
        name: 'Switch',
      }),
    ).toBeDisabled();
    expect(
      within(row('research@example.invalid')).getByRole('button', {
        name: 'Switching…',
      }),
    ).toBeDisabled();
    emit(
      codexSnapshot({
        revision: 2,
        busy: true,
        switching: {
          targetId: 'codex-research',
          stage: 'restarting',
          startedAt: Math.floor(Date.now() / 1000),
        },
      }),
    );
    expect(
      await screen.findByText(t('en', 'codexStageRestarting')),
    ).toBeVisible();
    result.resolve(switched('codex-research', 3));
    expect(
      await screen.findByText(
        'Codex now uses research@example.invalid. Open terminals reconnect automatically.',
      ),
    ).toBeVisible();
    expect(screen.queryByText(t('en', 'codexStageRestarting'))).toBeNull();
    expect(activeCard().getByRole('heading', { level: 3 })).toHaveTextContent(
      'research@example.invalid',
    );
    expect(
      call.mock.calls.filter(([name]) => name === 'codex_switch_account'),
    ).toHaveLength(1);
  });
  it('recommends exactly the account the native side ranks next', async () => {
    const tie = () =>
      codexAccount('codex-tie', {
        quota: {
          ordinaryUsageAllowed: true,
          resetCreditsAvailable: null,
          limits: [codexMainLimit(3, 1)],
        },
      });
    const raw = codexSnapshot();
    raw.accounts.push(tie());
    const { emit } = await setup(raw);
    const next = () => section('Next up');
    expect(next().getByText('research@example.invalid')).toBeVisible();
    emit(
      codexSnapshot({
        revision: 2,
        accounts: [...codexAccounts(), tie()],
        nextId: 'codex-tie',
        order: ['codex-tie', 'codex-research'],
      }),
    );
    await waitFor(() =>
      expect(next().getByText('tie@example.invalid')).toBeVisible(),
    );
    expect(
      within(screen.getByRole('region', { name: 'Next in order' }))
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['1tie@example.invalid', '2research@example.invalid']);
    emit(codexSnapshot({ revision: 3, nextId: null, order: [] }));
    await waitFor(() =>
      expect(next().getByText(t('en', 'noNext'))).toBeVisible(),
    );
    expect(next().queryByRole('button')).toBeNull();
  });
  it('offers the next account on a limited active card only when there is one', async () => {
    const limited = (revision: number, nextId: string | null) => {
      const raw = codexSnapshot({ revision, nextId });
      raw.accounts[0].quota!.limits[0].primary!.usedPercent = 100;
      return raw;
    };
    const { emit } = await setup(limited(1, 'codex-research'));
    expect(activeCard().getByText(/^Limit reached · frees in/)).toBeVisible();
    expect(
      activeCard().getByRole('button', {
        name: 'Switch to research@example.invalid now',
      }),
    ).toBeEnabled();
    emit(limited(2, null));
    await waitFor(() =>
      expect(
        activeCard().queryByRole('button', { name: /^Switch to/ }),
      ).toBeNull(),
    );
  });
  it('explains the shared reset order and automatic switching on the next account', async () => {
    await setup(codexSnapshot(), 'en', {
      preferSoonestWeeklyReset: false,
      threshold: 90,
    });
    const next = section('Next up');
    expect(next.getByText(t('en', 'nextWhyMostLeft'))).toBeVisible();
    expect(
      next.getByText('Switches when studio@example.invalid reaches 90% used.'),
    ).toBeVisible();
    expect(section('Automation').getByText('Most left first')).toBeVisible();
  });
  it('uses the soonest-reset wording by default and none about automation when it is off', async () => {
    await setup(codexSnapshot(), 'ro', { autoSwitchEnabled: false });
    const next = section(t('ro', 'nextUp'));
    expect(next.getByText(t('ro', 'nextWhySoonest'))).toBeVisible();
    expect(next.queryByText(/Comută automat/)).toBeNull();
    expect(
      section(t('ro', 'automation')).getByText(t('ro', 'autoSwitchOff')),
    ).toBeVisible();
  });
  it('marks windows used past the plan on credits, keeps the real number and caps the bar', async () => {
    const raw = codexSnapshot({ nextId: 'codex-personal' });
    // The active account keeps working on its purchased credits at 105% weekly.
    raw.accounts[0].quota!.limits[0].secondary!.usedPercent = 105;
    // Personal is past its weekly plan too, with a credit balance but no flag.
    const personal = raw.accounts[1].quota!.limits[0];
    personal.primary!.usedPercent = 20;
    personal.secondary!.usedPercent = 104;
    personal.rateLimitReachedType = null;
    personal.credits = { hasCredits: false, unlimited: false, balance: '3.00' };
    // Research is past its plan without any credits: no hint.
    raw.accounts[2].quota!.limits[0].secondary!.usedPercent = 102;
    await setup(raw);
    const active = activeCard();
    const weekly = active.getByRole('meter', { name: 'Weekly' });
    expect(weekly).toHaveAttribute('aria-valuenow', '100');
    expect(weekly).toHaveAttribute(
      'aria-valuetext',
      '105 percent used · Using credits',
    );
    expect(weekly.querySelector('span')).toHaveStyle({ width: '100%' });
    const activeWeekly = weekly.closest('.usage-quota')!;
    expect(activeWeekly).toHaveTextContent('105%');
    expect(
      within(activeWeekly as HTMLElement).getByText('Using credits'),
    ).toBeVisible();
    expect(
      within(
        active
          .getByRole('meter', { name: '5-hour window' })
          .closest('.usage-quota') as HTMLElement,
      ).queryByText('Using credits'),
    ).toBeNull();
    expect(active.getByText(/^Using credits · frees in/)).toBeVisible();
    const personalRow = within(row('personal@example.invalid'));
    expect(personalRow.getByText('104%')).toBeVisible();
    // The meter hint and the status say the same thing.
    expect(personalRow.getAllByText('Using credits')).toHaveLength(2);
    const researchRow = within(row('research@example.invalid'));
    expect(researchRow.getByText('102%')).toBeVisible();
    expect(researchRow.queryByText('Using credits')).toBeNull();
    expect(researchRow.getByText('Limit reached')).toBeVisible();
    expect(section('Next up').getByText(t('en', 'noNext'))).toBeVisible();
    expect(
      section('Next up').queryByRole('button', { name: /^Switch now/ }),
    ).toBeNull();
  });
  it('shows a partial switch as a warning and moves focus to the result', async () => {
    const { call } = await setup();
    call.mockResolvedValueOnce(
      switched('codex-research', 2, {
        daemonRestarted: false,
        otherClients: 1,
        error: 'daemonRestartFailed',
      }),
    );
    await fireEvent.click(
      within(row('research@example.invalid')).getByRole('button', {
        name: 'Switch',
      }),
    );
    const notice = (
      await screen.findByText(
        'Codex now uses research@example.invalid. It applies to the next Codex session you start.',
      )
    ).closest('[role="status"]') as HTMLElement;
    expect(notice).toHaveTextContent(t('en', 'codexSwitchedOtherClients'));
    expect(notice).toHaveTextContent(
      t('en', 'codexReason_daemonRestartFailed'),
    );
    expect(screen.queryByRole('alert')).toBeNull();
    await waitFor(() => expect(notice).toHaveFocus());
    await fireEvent.click(
      within(notice).getByRole('button', { name: 'Dismiss message' }),
    );
    expect(notice).not.toBeInTheDocument();
  });
  it('explains a rejected switch without claiming success', async () => {
    const { call } = await setup();
    call.mockRejectedValueOnce('switchInProgress');
    await fireEvent.click(
      within(row('research@example.invalid')).getByRole('button', {
        name: 'Switch',
      }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      t('en', 'codexReason_switchInProgress'),
    );
    expect(screen.queryByText(/Codex now uses/)).toBeNull();
    expect(
      within(row('studio@example.invalid')).getByText('Active'),
    ).toBeVisible();
  });
  it('warns about Codex Switcher and imports its accounts', async () => {
    const raw = codexSnapshot();
    raw.environment.codexSwitcherRunning = true;
    raw.capabilities.importSwitcher = codexCapability();
    const { call } = await setup(raw);
    expect(screen.getByText('Codex Switcher is running.')).toBeVisible();
    expect(
      screen.getByText(/force-closes Codex terminals when it switches/),
    ).toBeVisible();
    const imported = codexSnapshot({ revision: 2 });
    imported.environment.codexSwitcherRunning = true;
    imported.capabilities.importSwitcher = codexCapability();
    imported.accounts.push(
      codexAccount('codex-team'),
      codexAccount('codex-lab'),
    );
    call.mockResolvedValueOnce(imported);
    await fireEvent.click(
      screen.getByRole('button', { name: 'Import its accounts' }),
    );
    expect(call).toHaveBeenCalledWith('codex_import_switcher', undefined);
    expect(
      await screen.findByText('Imported 2 accounts from Codex Switcher.'),
    ).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: /Add account/ }));
    expect(
      screen.getByRole('button', { name: 'Import from Codex Switcher' }),
    ).toBeEnabled();
  });
  it('lists only the available add-account entries', async () => {
    await setup();
    expect(screen.queryByText('Codex Switcher is running.')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: /Add account/ }));
    expect(
      screen.getByRole('button', { name: 'Sign in with ChatGPT…' }),
    ).toBeEnabled();
    expect(
      screen.getByRole('button', { name: 'Import current Codex login' }),
    ).toBeEnabled();
    expect(
      screen.queryByRole('button', { name: 'Import from Codex Switcher' }),
    ).toBeNull();
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(
      screen.queryByRole('button', { name: 'Sign in with ChatGPT…' }),
    ).toBeNull();
    expect(screen.getByRole('button', { name: /Add account/ })).toHaveFocus();
  });
  it('offers Sign in again for expired saved sign-ins', async () => {
    const raw = codexSnapshot();
    Object.assign(raw.accounts[2], {
      needsSignIn: true,
      error: 'signInRequired',
      quotaState: 'unavailable',
      switchable: codexCapability('signInRequired'),
    });
    // The native side never ranks a signed-out account next.
    raw.nextId = null;
    raw.order = [];
    const { call } = await setup(raw);
    const research = within(row('research@example.invalid'));
    expect(research.getAllByText('Sign in again')).toHaveLength(2);
    expect(research.queryByRole('button', { name: /^Switch/ })).toBeNull();
    expect(section('Next up').getByText(t('en', 'noNext'))).toBeVisible();
    expect(
      screen.getByText(
        t('en', 'signInNeeded', { names: 'research@example.invalid' }),
      ),
    ).toBeVisible();
    call.mockResolvedValueOnce({
      id: 'login-again',
      status: 'waiting',
      error: null,
    });
    const trigger = research.getByRole('button', { name: 'Sign in again' });
    expect(trigger).toHaveAccessibleDescription('research@example.invalid');
    await fireEvent.click(trigger);
    expect(call).toHaveBeenCalledWith('codex_begin_login');
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByRole('heading')).toHaveTextContent(
      'Sign in again',
    );
    expect(dialog).toHaveTextContent(
      'Sign in as research@example.invalid in your browser.',
    );
    await fireEvent(dialog, new Event('cancel', { cancelable: true }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(call).toHaveBeenCalledWith('codex_cancel_login', {
      id: 'login-again',
    });
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('uses managed browser completion with no pasted-code input and restores Add focus on cancellation', async () => {
    const { call } = await setup();
    const add = screen.getByRole('button', { name: /Add account/ });
    await fireEvent.click(add);
    call.mockResolvedValueOnce({
      id: 'login-ui',
      status: 'waiting',
      error: null,
    });
    await fireEvent.click(
      screen.getByRole('button', { name: 'Sign in with ChatGPT…' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent('there’s no code to paste');
    expect(dialog).toHaveTextContent(t('en', 'codexLoginKeepsCurrent'));
    expect(within(dialog).queryByRole('textbox')).toBeNull();
    await waitFor(() =>
      expect(
        screen.getByText('Waiting for you to finish in the browser…'),
      ).toBeVisible(),
    );
    await fireEvent(dialog, new Event('cancel', { cancelable: true }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(call).toHaveBeenCalledWith('codex_cancel_login', { id: 'login-ui' });
    await waitFor(() => expect(add).toHaveFocus());
  });
  it('refreshes any account from its menu, opens details and keeps Delete off the active account', async () => {
    const { call, codexController } = await setup();
    const personalMenu = within(row('personal@example.invalid')).getByRole(
      'button',
      { name: 'More actions for personal@example.invalid' },
    );
    const items = (trigger: HTMLElement) =>
      within(document.getElementById(trigger.getAttribute('aria-controls')!)!);
    await fireEvent.click(personalMenu);
    expect(personalMenu).toHaveAttribute('aria-expanded', 'true');
    await fireEvent.click(
      items(personalMenu).getByRole('button', { name: 'Refresh' }),
    );
    expect(call).toHaveBeenCalledWith('codex_refresh_account', {
      id: 'codex-personal',
    });
    expect(personalMenu).toHaveFocus();
    await waitFor(() => expect(get(codexController).pending).toBeNull());
    const studioMenu = within(row('studio@example.invalid')).getByRole(
      'button',
      { name: 'More actions for studio@example.invalid' },
    );
    await fireEvent.click(studioMenu);
    const deleteActive = items(studioMenu).getByRole('button', {
      name: 'Delete',
    });
    expect(deleteActive).toBeDisabled();
    expect(deleteActive).toHaveAccessibleDescription(
      'Switch to another account before removing this one.',
    );
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(studioMenu).toHaveFocus();
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull();
    await fireEvent.click(personalMenu);
    await fireEvent.click(
      items(personalMenu).getByRole('button', { name: 'Details' }),
    );
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByRole('heading', { level: 2 })).toHaveTextContent(
      'personal@example.invalid',
    );
    expect(dialog).toHaveTextContent('Limit reached');
    expect(dialog).toHaveTextContent('ChatGPT account');
    expect(dialog).toHaveTextContent('Plus');
    expect(dialog).toHaveTextContent(t('en', 'identityVerified'));
    expect(
      within(dialog).getByRole('meter', { name: '5-hour window' }),
    ).toHaveAttribute('aria-valuenow', '100');
    await fireEvent.click(
      within(dialog).getByRole('button', {
        name: 'Delete account personal@example.invalid',
      }),
    );
    const confirm = screen.getByRole('dialog');
    expect(confirm).toHaveTextContent(
      'doesn’t sign you out of ChatGPT or change the account Codex uses now',
    );
    await fireEvent.click(
      within(confirm).getByRole('button', { name: 'Cancel' }),
    );
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    await waitFor(() => expect(personalMenu).toHaveFocus());
    expect(
      call.mock.calls.some(([name]) => name === 'codex_delete_account'),
    ).toBe(false);
    await fireEvent.click(screen.getByRole('button', { name: /Refresh all/ }));
    expect(call).toHaveBeenCalledWith('codex_refresh_all', undefined);
  });
  it('shows unsupported setup explicitly and keeps actions disabled', async () => {
    const raw = codexSnapshot({
      availability: 'unsupported',
      blockedReason: 'unsupportedStore',
      nextId: null,
      order: [],
    });
    Object.keys(raw.capabilities).forEach((key) => {
      raw.capabilities[key as keyof typeof raw.capabilities] =
        codexCapability('unsupportedStore');
    });
    raw.accounts.forEach(
      (account) => (account.switchable = codexCapability('unsupportedStore')),
    );
    await setup(raw);
    const notice = screen
      .getByText(t('en', 'codexReason_unsupportedStore'))
      .closest('[role="note"]') as HTMLElement;
    expect(notice).toHaveTextContent('Codex setup');
    expect(
      within(notice).getByRole('button', { name: 'Check setup' }),
    ).toBeEnabled();
    expect(screen.getByRole('button', { name: /Add account/ })).toBeDisabled();
    const switchButton = within(row('research@example.invalid')).getByRole(
      'button',
      { name: 'Switch' },
    );
    expect(switchButton).toBeDisabled();
    expect(switchButton.parentElement).toHaveAttribute(
      'title',
      t('en', 'codexReason_unsupportedStore'),
    );
    expect(section('Next up').getByText(t('en', 'noNext'))).toBeVisible();
    expect(screen.getByText('Codex 0.160.0 · needs attention')).toBeVisible();
  });
  it('supports keyboard provider navigation and keeps the active Codex tab on Codex', async () => {
    const { claudeCall } = await setup();
    const codex = screen.getByRole('tab', { name: 'Codex' });
    expect(codex).toHaveAttribute('aria-selected', 'true');
    await fireEvent.keyDown(codex, { key: 'ArrowLeft' });
    await waitFor(() =>
      expect(screen.getByRole('tab', { name: /Claude/ })).toHaveFocus(),
    );
    await fireEvent.keyDown(screen.getByRole('tab', { name: /Claude/ }), {
      key: 'End',
    });
    await waitFor(() =>
      expect(screen.getByRole('tab', { name: 'Codex' })).toHaveFocus(),
    );
    expect(
      claudeCall.mock.calls.every(([name]) => name === 'get_snapshot'),
    ).toBe(true);
  });
  it('renders both tabs with the same page skeleton', async () => {
    await setup();
    const skeleton = () => ({
      headings: screen
        .getAllByRole('heading', { level: 2 })
        .map((heading) => heading.textContent!.replace(/\d+$/, '').trim()),
      columns: within(screen.getByRole('table'))
        .getAllByRole('columnheader')
        .map((header) => header.textContent?.trim()),
      order: !!screen.queryByRole('region', { name: 'Next in order' }),
      refreshAll: !!screen.queryByRole('button', { name: 'Refresh all' }),
      settings: !!screen.queryByRole('button', { name: 'Open settings' }),
    });
    const codex = skeleton();
    await fireEvent.click(screen.getByRole('tab', { name: /Claude/ }));
    await screen.findByText(t('en', 'claudeSubtitle'));
    const claude = skeleton();
    // Shared cards retain their order; Codex adds manual resets beside its setup.
    expect(codex.headings).toEqual([
      'Active account',
      'Saved accounts',
      'Next up',
      'Automation',
      'Available resets',
      'Codex setup',
    ]);
    expect(claude.headings).toEqual([
      'Active account',
      'Saved accounts',
      'Next up',
      'Automation',
      'Credits & renewal',
    ]);
    expect(claude.columns).toEqual(codex.columns);
    expect(claude).toMatchObject({
      order: true,
      refreshAll: true,
      settings: true,
    });
    expect(codex).toMatchObject({
      order: true,
      refreshAll: true,
      settings: true,
    });
  });
  it('keeps the Romanian demo read-only across providers', async () => {
    const { call } = await setup(codexDemoSnapshot(), 'ro');
    expect(document.documentElement.lang).toBe('ro');
    expect(screen.getByText(t('ro', 'codexSubtitle'))).toBeVisible();
    expect(screen.getByText('Codex Switcher rulează.')).toBeVisible();
    expect(
      screen.getByRole('button', { name: t('ro', 'codexSwitcherImport') }),
    ).toBeDisabled();
    expect(
      screen.getByRole('button', {
        name: new RegExp(t('ro', 'addAccount')),
      }),
    ).toBeDisabled();
    expect(section(t('ro', 'nextUp')).getByRole('button')).toBeDisabled();
    expect(
      within(row('personal@example.invalid')).getByText('Limită atinsă'),
    ).toBeVisible();
    expect(screen.getByRole('tab', { name: 'Codex' })).toBeEnabled();
    await fireEvent.click(
      screen.getByRole('button', { name: t('ro', 'openSettings') }),
    );
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent(t('ro', 'settingsProviderScope'));
    expect(
      within(dialog).getByRole('group', {
        name: t('ro', 'claudeOnlyAutomation'),
      }),
    ).toBeVisible();
    expect(
      within(dialog).getByRole('button', { name: t('ro', 'save') }),
    ).toBeDisabled();
    expect(
      call.mock.calls.every(([name]) => name === 'get_codex_snapshot'),
    ).toBe(true);
  });
});

const FAKE_KEY = 'sk-proj-SENTINEL-0123456789abcdef';
const withKey = (raw = codexSnapshot(), overrides = {}) => {
  raw.accounts.push(codexApiKeyAccount('codex-key', overrides));
  return raw;
};
async function openKeyDialog(locale: 'en' | 'ro' = 'en') {
  const add = screen.getByRole('button', {
    name: new RegExp(t(locale, 'addAccount')),
  });
  await fireEvent.click(add);
  await fireEvent.click(
    screen.getByRole('button', { name: t(locale, 'codexAddApiKey') }),
  );
  const dialog = await screen.findByRole('dialog');
  return {
    add,
    dialog,
    name: within(dialog).getByLabelText(
      t(locale, 'codexApiKeyNameLabel'),
    ) as HTMLInputElement,
    key: within(dialog).getByLabelText(
      t(locale, 'codexApiKeyLabel'),
    ) as HTMLInputElement,
    save: within(dialog).getByRole('button', {
      name: t(locale, 'codexApiKeySave'),
    }),
  };
}
const noKey = () => {
  expect(document.body.textContent).not.toContain('SENTINEL');
  expect(document.body.innerHTML).not.toContain('SENTINEL');
};
describe('Codex API key accounts', () => {
  it('adds an API key from a small dialog, trims it and never renders it', async () => {
    const { call, codexController } = await setup();
    const { add, dialog, name, key, save } = await openKeyDialog();
    expect(within(dialog).getByRole('heading', { level: 2 })).toHaveTextContent(
      'Add an OpenAI API key',
    );
    expect(dialog).toHaveTextContent(
      'never switches to it on its own unless you turn on Use as fallback for this key',
    );
    // A password field that browsers and spell checkers leave alone.
    expect(key).toHaveAttribute('type', 'password');
    expect(key).toHaveAttribute('autocomplete', 'off');
    expect(key).toHaveAttribute('spellcheck', 'false');
    expect(key).toHaveAccessibleDescription(t('en', 'codexApiKeyHelp'));
    expect(name).toHaveAttribute('maxlength', '60');
    expect(save).toBeDisabled();
    await fireEvent.input(name, { target: { value: '  Work key ' } });
    expect(save).toBeDisabled();
    await fireEvent.input(key, { target: { value: `  ${FAKE_KEY}  ` } });
    expect(save).toBeEnabled();
    const pending = deferred();
    call.mockReturnValueOnce(pending.promise);
    await fireEvent.click(save);
    expect(call).toHaveBeenCalledWith('codex_add_api_key', {
      name: 'Work key',
      key: FAKE_KEY,
    });
    // The key field is already empty while the native call runs.
    expect(key.value).toBe('');
    expect(
      within(dialog).getByRole('button', { name: 'Saving…' }),
    ).toBeDisabled();
    noKey();
    pending.resolve(withKey(codexSnapshot({ revision: 2 })));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    const notice = (
      await screen.findByText(/Saved Work key \(API key …cdef\)/)
    ).closest('[role="status"]') as HTMLElement;
    expect(notice).toHaveTextContent('platform.openai.com');
    expect(notice).toHaveTextContent('until you switch');
    // Focus goes back to the control that opened the dialog.
    await waitFor(() => expect(add).toHaveFocus());
    // The saved row: its ending, what it costs and a normal Switch button.
    const saved = within(row('Work key'));
    expect(saved.getByText('API key …cdef')).toBeVisible();
    expect(saved.getByText('API key')).toBeVisible();
    expect(
      saved.getByText('Billed per token · no ChatGPT plan limits'),
    ).toBeVisible();
    expect(saved.getByRole('button', { name: 'Switch' })).toBeEnabled();
    noKey();
    expect(JSON.stringify(get(codexController))).not.toContain('SENTINEL');
  });
  it('clears the key after a failed save and shows only a generic error', async () => {
    const { call, codexController } = await setup();
    const { dialog, name, key, save } = await openKeyDialog();
    await fireEvent.input(name, { target: { value: 'Work key' } });
    await fireEvent.input(key, { target: { value: FAKE_KEY } });
    call.mockRejectedValueOnce('invalidApiKey');
    await fireEvent.click(save);
    const alert = await within(dialog).findByRole('alert');
    expect(alert).toHaveTextContent(t('en', 'codexReason_invalidApiKey'));
    // The dialog stays open with the name kept and the key gone.
    expect(screen.getByRole('dialog')).toBe(dialog);
    expect(key.value).toBe('');
    expect(name.value).toBe('Work key');
    expect(
      within(dialog).getByRole('button', { name: 'Save key' }),
    ).toBeDisabled();
    noKey();
    // An error that echoes the key is never shown, only the generic failure.
    await fireEvent.input(key, { target: { value: FAKE_KEY } });
    call.mockRejectedValueOnce(new Error(`rejected ${FAKE_KEY}`));
    await fireEvent.click(
      within(dialog).getByRole('button', { name: 'Save key' }),
    );
    await waitFor(() =>
      expect(within(dialog).getByRole('alert')).toHaveTextContent(
        t('en', 'codexActionFailed'),
      ),
    );
    expect(key.value).toBe('');
    noKey();
    expect(JSON.stringify(get(codexController))).not.toContain('SENTINEL');
    expect(
      call.mock.calls.filter(([command]) => command === 'codex_add_api_key'),
    ).toHaveLength(2);
  });
  it('clears the key when the dialog is cancelled, escaped or closed', async () => {
    const { call } = await setup();
    const first = await openKeyDialog();
    await fireEvent.input(first.name, { target: { value: 'Work key' } });
    await fireEvent.input(first.key, { target: { value: FAKE_KEY } });
    await fireEvent.click(
      within(first.dialog).getByRole('button', { name: 'Cancel' }),
    );
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    await waitFor(() => expect(first.add).toHaveFocus());
    // Reopened, nothing of the earlier input is left.
    const second = await openKeyDialog();
    expect(second.name.value).toBe('');
    expect(second.key.value).toBe('');
    await fireEvent.input(second.key, { target: { value: FAKE_KEY } });
    await fireEvent(second.dialog, new Event('cancel', { cancelable: true }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    const third = await openKeyDialog();
    expect(third.key.value).toBe('');
    await fireEvent.input(third.key, { target: { value: FAKE_KEY } });
    await fireEvent.click(
      within(third.dialog).getByRole('button', { name: 'Close' }),
    );
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect((await openKeyDialog()).key.value).toBe('');
    expect(
      call.mock.calls.some(([command]) => command === 'codex_add_api_key'),
    ).toBe(false);
    noKey();
  });
  it('turns the entry off while storage or Codex cannot take a key', async () => {
    const raw = codexSnapshot();
    raw.capabilities.addApiKey = codexCapability('vaultUnavailable');
    await setup(raw);
    await fireEvent.click(screen.getByRole('button', { name: /Add account/ }));
    expect(screen.getByRole('button', { name: 'Add API key…' })).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Sign in with ChatGPT…' }),
    ).toBeEnabled();
  });
  it('switches to the key like to any account and shows it without usage windows', async () => {
    const { call } = await setup(withKey());
    const result = withKey(switched('codex-key', 2));
    result.accounts.forEach(
      (account) => (account.selected = account.id === 'codex-key'),
    );
    call.mockResolvedValueOnce(result);
    await fireEvent.click(
      within(row('Work key')).getByRole('button', { name: 'Switch' }),
    );
    expect(call).toHaveBeenCalledWith('codex_switch_account', {
      id: 'codex-key',
    });
    expect(
      await screen.findByText(
        'Codex now uses Work key. Open terminals reconnect automatically.',
      ),
    ).toBeVisible();
    const card = activeCard();
    expect(card.getByRole('heading', { name: 'Work key' })).toBeVisible();
    expect(card.getByText('API key …cdef')).toBeVisible();
    expect(
      card.getByText('API key, billed per token, no ChatGPT plan limits.'),
    ).toBeVisible();
    expect(card.queryByRole('meter')).toBeNull();
    expect(card.queryByRole('button', { name: /^Refresh/ })).toBeNull();
    expect(card.queryByText('Not checked yet')).toBeNull();
    // Back to ChatGPT with the same button on another row.
    expect(
      within(row('studio@example.invalid')).getByRole('button', {
        name: 'Switch',
      }),
    ).toBeEnabled();
  });
  it('shows the key ending and no usage in details, and cannot refresh it', async () => {
    const { call } = await setup(withKey());
    const menu = within(row('Work key')).getByRole('button', {
      name: 'More actions for Work key',
    });
    await fireEvent.click(menu);
    const items = within(
      document.getElementById(menu.getAttribute('aria-controls')!)!,
    );
    expect(items.getByRole('button', { name: 'Refresh' })).toBeDisabled();
    expect(items.getByRole('button', { name: 'Delete' })).toBeEnabled();
    await fireEvent.click(items.getByRole('button', { name: 'Details' }));
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent('API key ends in');
    expect(dialog).toHaveTextContent('…cdef');
    expect(dialog).toHaveTextContent(t('en', 'identityApiKey'));
    expect(dialog).toHaveTextContent('ChatGPT plan limits');
    expect(within(dialog).queryByRole('meter')).toBeNull();
    expect(within(dialog).queryByText('Checked')).toBeNull();
    expect(
      within(dialog).queryByRole('button', { name: /^Refresh/ }),
    ).toBeNull();
    expect(
      call.mock.calls.some(([command]) => command === 'codex_refresh_account'),
    ).toBe(false);
    noKey();
  });
  it('offers the same dialog in Romanian', async () => {
    await setup(codexSnapshot(), 'ro');
    const { dialog, name, key, save } = await openKeyDialog('ro');
    expect(within(dialog).getByRole('heading', { level: 2 })).toHaveTextContent(
      'Adaugă o cheie API OpenAI',
    );
    expect(name).toBeVisible();
    expect(key).toHaveAttribute('type', 'password');
    expect(save).toHaveTextContent('Salvează cheia');
    expect(dialog).toHaveTextContent('Începe cu sk-');
    expect(dialog).toHaveTextContent('o alegi tu cu Comută');
  });
});

async function openDetails(name: string, locale: 'en' | 'ro' = 'en') {
  const menu = within(row(name)).getByRole('button', {
    name: t(locale, 'moreLabel', { name }),
  });
  await fireEvent.click(menu);
  const items = within(
    document.getElementById(menu.getAttribute('aria-controls')!)!,
  );
  await fireEvent.click(
    items.getByRole('button', { name: t(locale, 'viewDetails') }),
  );
  return within(screen.getByRole('dialog'));
}
describe('Codex Flex processing', () => {
  it('turns Flex on for an API key from its details, with the cost and session warning', async () => {
    const { call } = await setup(withKey());
    const dialog = await openDetails('Work key');
    const toggle = dialog.getByRole('switch', { name: /Use Flex processing/ });
    expect(toggle).not.toBeChecked();
    expect(toggle).toBeEnabled();
    const help = dialog.getByText(/Cheaper, but slower/);
    expect(help).toHaveTextContent('a request can fail when Flex capacity');
    expect(help).toHaveTextContent('Codex does not retry that error');
    expect(help).toHaveTextContent('Only for this API key');
    expect(help).toHaveTextContent(
      'Sessions that are already open keep their current tier.',
    );
    expect(toggle).toHaveAccessibleDescription(help.textContent!);
    call.mockResolvedValueOnce(
      withKey(codexSnapshot({ revision: 2 }), { useFlex: true }),
    );
    await fireEvent.click(toggle);
    expect(call).toHaveBeenLastCalledWith('codex_set_api_key_flex', {
      id: 'codex-key',
      enabled: true,
    });
    await waitFor(() =>
      expect(
        dialog.getByRole('switch', { name: /Use Flex processing/ }),
      ).toBeChecked(),
    );
    // The key's row says it uses Flex.
    expect(
      within(row('Work key')).getByText('API key …cdef · Flex'),
    ).toBeVisible();
    // And off again.
    call.mockResolvedValueOnce(withKey(codexSnapshot({ revision: 3 })));
    await fireEvent.click(
      dialog.getByRole('switch', { name: /Use Flex processing/ }),
    );
    expect(call).toHaveBeenLastCalledWith('codex_set_api_key_flex', {
      id: 'codex-key',
      enabled: false,
    });
    noKey();
  });
  it('shows no Flex option for a ChatGPT account', async () => {
    await setup(withKey());
    const dialog = await openDetails('studio@example.invalid');
    expect(dialog.queryByRole('switch', { name: /Flex/ })).toBeNull();
    expect(dialog.queryByText(/Cheaper, but slower/)).toBeNull();
  });
  it('explains why the option or the switch is blocked', async () => {
    const raw = withKey(codexSnapshot(), {
      flex: codexCapability('flexUnsupportedVersion'),
    });
    await setup(raw);
    const dialog = await openDetails('Work key');
    expect(
      dialog.getByRole('switch', { name: /Use Flex processing/ }),
    ).toBeDisabled();
    expect(
      dialog.getByText(
        'Flex needs Codex 0.163.0 or newer. Update Codex to use it.',
      ),
    ).toBeVisible();
  });
  it('keeps a Flex key that cannot be set from being switched to, and lets Flex be turned off', async () => {
    const raw = withKey(codexSnapshot(), {
      useFlex: true,
      switchable: codexCapability('flexUnsupportedVersion'),
    });
    const { call } = await setup(raw);
    expect(
      within(row('Work key')).getByRole('button', { name: 'Switch' }),
    ).toBeDisabled();
    const dialog = await openDetails('Work key');
    const toggle = dialog.getByRole('switch', { name: /Use Flex processing/ });
    expect(toggle).toBeChecked();
    expect(toggle).toBeEnabled();
    call.mockResolvedValueOnce(withKey(codexSnapshot({ revision: 2 })));
    await fireEvent.click(toggle);
    expect(call).toHaveBeenLastCalledWith('codex_set_api_key_flex', {
      id: 'codex-key',
      enabled: false,
    });
  });
  it('shows a stable reason when the config cannot be changed and keeps the setting off', async () => {
    const { call } = await setup(withKey());
    const dialog = await openDetails('Work key');
    call.mockRejectedValueOnce('flexConfigFailed');
    await fireEvent.click(
      dialog.getByRole('switch', { name: /Use Flex processing/ }),
    );
    expect(
      await screen.findByText(
        /Flex could not be turned on because Codex's config\.toml/,
      ),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        dialog.getByRole('switch', { name: /Use Flex processing/ }),
      ).not.toBeChecked(),
    );
    noKey();
  });
  it('tells the owner what a switch did to Flex, in English and Romanian', async () => {
    for (const [locale, on, off] of [
      [
        'en',
        'Flex is on. New Codex sessions use Flex; sessions that are already open keep their tier.',
        "Flex is off again. A session started while Flex was on can fail on ChatGPT with 'Unsupported service_tier: flex'; start a new session.",
      ],
      [
        'ro',
        'Flex este activ. Sesiunile Codex noi folosesc Flex; sesiunile deja deschise își păstrează nivelul.',
        "Flex este dezactivat din nou. O sesiune pornită cât timp Flex era activ poate eșua pe ChatGPT cu 'Unsupported service_tier: flex'; pornește o sesiune nouă.",
      ],
    ] as const) {
      const { call } = await setup(
        withKey(codexSnapshot(), { useFlex: true }),
        locale,
      );
      call.mockResolvedValueOnce(
        withKey(switched('codex-key', 2, { flex: 'on' }), { useFlex: true }),
      );
      await fireEvent.click(
        within(row('Work key')).getByRole('button', {
          name: t(locale, 'switch'),
        }),
      );
      expect(await screen.findByText(on)).toBeVisible();
      call.mockResolvedValueOnce(
        withKey(switched('codex-studio', 3, { flex: 'off' }), {
          useFlex: true,
        }),
      );
      await fireEvent.click(
        within(row('studio@example.invalid')).getByRole('button', {
          name: t(locale, 'switch'),
        }),
      );
      expect(await screen.findByText(off)).toBeVisible();
      cleanup();
      language.set('en');
    }
  });
  it('warns while a restore is pending and never shows a secret', async () => {
    const raw = withKey(codexSnapshot());
    raw.warnings = ['flexRestoreFailed'];
    await setup(raw);
    expect(
      await screen.findByText(t('en', 'codexReason_flexRestoreFailed')),
    ).toBeVisible();
    noKey();
  });
});
const engaged = (
  raw = codexSnapshot(),
  since = 1_700_000_000,
  overrides = {},
) => {
  const snap = withKey(
    codexSnapshot({
      ...raw,
      revision: raw.revision + 1,
      selectedId: 'codex-key',
      fallbackEngaged: { keyId: 'codex-key', since },
    }),
    { fallback: true, ...overrides },
  );
  snap.accounts.forEach((a) => (a.selected = a.id === 'codex-key'));
  return snap;
};
describe('Codex automatic API-key fallback', () => {
  it('asks for billing consent before turning the fallback on, and saves nothing on cancel', async () => {
    const { call } = await setup(withKey());
    const opened = await openDetails('Work key');
    const toggle = opened.getByRole('switch', { name: /Use as fallback/ });
    expect(toggle).not.toBeChecked();
    expect(toggle).toBeEnabled();
    expect(opened.getByText(/switches to this key by itself/)).toBeVisible();
    expect(toggle).toHaveAccessibleDescription(t('en', 'codexFallbackHelp'));
    const calls = call.mock.calls.length;
    await fireEvent.click(toggle);
    // The consent window opens; nothing is saved yet.
    const consent = within(await screen.findByRole('dialog'));
    expect(
      consent.getByRole('heading', {
        name: 'Allow automatic billing on this key?',
      }),
    ).toBeVisible();
    expect(
      consent.getByText(
        'Codex bills this key per token, outside your ChatGPT plan. PrimerSwitch will switch to it by itself when every ChatGPT account is at its limit and switch back when one recovers. Set a monthly budget on the OpenAI project.',
      ),
    ).toBeVisible();
    expect(call.mock.calls.length).toBe(calls);
    await fireEvent.click(consent.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(call.mock.calls.length).toBe(calls);
    expect(within(row('Work key')).queryByText(/Fallback/)).toBeNull();
    noKey();
  });
  it('saves the setting only after the owner confirms, and tags the key row', async () => {
    const { call } = await setup(withKey());
    const opened = await openDetails('Work key');
    await fireEvent.click(
      opened.getByRole('switch', { name: /Use as fallback/ }),
    );
    const consent = within(await screen.findByRole('dialog'));
    call.mockResolvedValueOnce(
      withKey(codexSnapshot({ revision: 2 }), { fallback: true }),
    );
    await fireEvent.click(
      consent.getByRole('button', { name: 'Turn on fallback' }),
    );
    expect(call).toHaveBeenLastCalledWith('codex_set_api_key_fallback', {
      id: 'codex-key',
      enabled: true,
    });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(
      within(row('Work key')).getByText('API key …cdef · Fallback'),
    ).toBeVisible();
    // Turning it off again needs no confirmation.
    const again = await openDetails('Work key');
    const toggle = again.getByRole('switch', { name: /Use as fallback/ });
    expect(toggle).toBeChecked();
    call.mockResolvedValueOnce(withKey(codexSnapshot({ revision: 3 })));
    await fireEvent.click(toggle);
    expect(call).toHaveBeenLastCalledWith('codex_set_api_key_fallback', {
      id: 'codex-key',
      enabled: false,
    });
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    noKey();
  });
  it('shows an error and keeps the setting off when the save is refused', async () => {
    const { call, codexController } = await setup(withKey());
    const opened = await openDetails('Work key');
    await fireEvent.click(
      opened.getByRole('switch', { name: /Use as fallback/ }),
    );
    const consent = within(await screen.findByRole('dialog'));
    call.mockRejectedValueOnce('busy');
    await fireEvent.click(
      consent.getByRole('button', { name: 'Turn on fallback' }),
    );
    expect(await consent.findByRole('alert')).toHaveTextContent(
      t('en', 'codexReason_busy'),
    );
    expect(get(codexController).snapshot?.accounts[3].fallback).toBe(false);
    // The window stays open so the owner can try again or cancel.
    expect(screen.getByRole('dialog')).toBeVisible();
    noKey();
  });
  it('explains that the setting needs automatic switching, and still lets it be saved', async () => {
    await setup(
      withKey(codexSnapshot(), { fallbackHint: 'fallbackNeedsAutoSwitch' }),
    );
    const opened = await openDetails('Work key');
    expect(
      opened.getByText(t('en', 'codexReason_fallbackNeedsAutoSwitch')),
    ).toBeVisible();
    expect(
      opened.getByRole('switch', { name: /Use as fallback/ }),
    ).toBeEnabled();
  });
  it('shows why the setting cannot be changed now', async () => {
    await setup(
      withKey(codexSnapshot(), { fallbackToggle: codexCapability('busy') }),
    );
    const opened = await openDetails('Work key');
    expect(
      opened.getByRole('switch', { name: /Use as fallback/ }),
    ).toBeDisabled();
    expect(opened.getByText(t('en', 'codexReason_busy'))).toBeVisible();
  });
  it('warns that sessions started under Flex fail after the return, when both are on', async () => {
    await setup(withKey(codexSnapshot(), { fallback: true, useFlex: true }));
    const opened = await openDetails('Work key');
    expect(
      opened.getByText(
        'A Codex session started while this key is active with Flex fails on ChatGPT after the automatic return until you start a new one.',
      ),
    ).toBeVisible();
    cleanup();
    await setup(withKey(codexSnapshot(), { fallback: true, useFlex: false }));
    const without = await openDetails('Work key');
    expect(without.queryByText(/started while this key is active/)).toBeNull();
  });
  it('shows no fallback option for a ChatGPT account', async () => {
    await setup(withKey());
    const opened = await openDetails('studio@example.invalid');
    expect(opened.queryByRole('switch', { name: /fallback/i })).toBeNull();
  });
  it('shows a persistent banner while the fallback key is active, with a way back', async () => {
    const since = 1_700_000_000;
    const { call } = await setup(engaged(codexSnapshot(), since));
    const banner = screen.getByText(
      `Using API key Work key (billed per token) since ${codexClock(since, 'en')} because every ChatGPT account is at its limit. PrimerSwitch returns automatically when one recovers.`,
    );
    expect(banner).toBeVisible();
    // No way to dismiss it.
    expect(
      within(banner.closest('[role="status"]') as HTMLElement).queryByRole(
        'button',
        { name: /dismiss|close/i },
      ),
    ).toBeNull();
    expect(
      within(row('Work key')).getByText('API key …cdef · Fallback'),
    ).toBeVisible();
    const back = screen.getByRole('button', { name: 'Switch back now' });
    expect(back).toBeEnabled();
    const result = switched('codex-research', 3);
    call.mockResolvedValueOnce(withKey(result, { fallback: true }));
    await fireEvent.click(back);
    expect(call).toHaveBeenLastCalledWith('codex_switch_account', {
      id: 'codex-research',
    });
    await waitFor(() =>
      expect(screen.queryByText(/Using API key Work key/)).toBeNull(),
    );
    noKey();
  });
  it('shows no banner when no fallback is engaged, or after the key stopped being the active login', async () => {
    await setup(withKey(codexSnapshot(), { fallback: true }));
    expect(screen.queryByText(/billed per token\) since/)).toBeNull();
    expect(
      screen.queryByRole('button', { name: 'Switch back now' }),
    ).toBeNull();
    cleanup();
    // A leftover marker for a key that is not the active login must not promise a return.
    const stale = codexSnapshot({
      fallbackEngaged: { keyId: 'codex-key', since: 1_700_000_000 },
    });
    await setup(withKey(stale, { fallback: true }));
    expect(
      screen.queryByRole('button', { name: 'Switch back now' }),
    ).toBeNull();
    cleanup();
    // The key is the login but its setting was turned off: no automatic return is promised.
    await setup(engaged(codexSnapshot(), 1_700_000_000, { fallback: false }));
    expect(screen.queryByText(/billed per token\) since/)).toBeNull();
    expect(
      screen.queryByRole('button', { name: 'Switch back now' }),
    ).toBeNull();
  });
  it('offers no way back while no ChatGPT account can be switched to', async () => {
    const raw = engaged();
    raw.nextId = null;
    raw.accounts.forEach((a) => {
      if (a.authKind === 'chatgpt') a.switchable = codexCapability('busy');
    });
    await setup(raw);
    expect(screen.getByText(/Using API key Work key/)).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Switch back now' }),
    ).toBeNull();
  });
  it('says everything in Romanian too', async () => {
    const since = 1_700_000_000;
    await setup(engaged(codexSnapshot(), since), 'ro');
    expect(
      screen.getByText(
        `Se folosește cheia API Work key (facturată per token) din ${codexClock(since, 'ro')}, pentru că toate conturile ChatGPT sunt la limită. PrimerSwitch revine automat când unul se reface.`,
      ),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Revino acum' })).toBeEnabled();
    const opened = await openDetails('Work key', 'ro');
    expect(
      opened.getByRole('switch', { name: /Folosește ca rezervă/ }),
    ).toBeChecked();
    expect(
      within(row('Work key')).getByText(
        `${t('ro', 'codexApiKeyEnding', { tail: 'cdef' })} · Rezervă`,
      ),
    ).toBeVisible();
  });
  it('asks for consent in Romanian', async () => {
    await setup(withKey(), 'ro');
    const opened = await openDetails('Work key', 'ro');
    await fireEvent.click(
      opened.getByRole('switch', { name: /Folosește ca rezervă/ }),
    );
    const consent = within(await screen.findByRole('dialog'));
    expect(
      consent.getByText(t('ro', 'codexFallbackConfirmText')),
    ).toBeVisible();
    expect(
      consent.getByRole('button', { name: 'Pornește rezerva' }),
    ).toBeEnabled();
  });
  it('warns about Flex inside the consent window when the key uses Flex', async () => {
    await setup(withKey(codexSnapshot(), { useFlex: true }));
    const opened = await openDetails('Work key');
    await fireEvent.click(
      opened.getByRole('switch', { name: /Use as fallback/ }),
    );
    const consent = within(await screen.findByRole('dialog'));
    expect(
      consent.getByText(t('en', 'codexFallbackFlexWarning')),
    ).toBeVisible();
  });
});
