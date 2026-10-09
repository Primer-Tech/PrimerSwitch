import {
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
  codexCapability,
  codexDemoSnapshot,
  codexMainLimit,
  codexSnapshot,
} from './test/codex-fixtures';
import { snapshot } from './test/fixtures';
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
    expect(next.getByText('Switches automatically at 90%.')).toBeVisible();
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
    expect(section('Next up').getByText('Using credits')).toBeVisible();
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
