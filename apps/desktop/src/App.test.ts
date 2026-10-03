import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import { createController } from './lib/controller';
import { snapshot, account } from './test/fixtures';
import type { Bridge } from './lib/bridge';
import { defaults } from './lib/types';
import { language, t } from './lib/i18n';
function setup(raw = snapshot()) {
  let emit: (snapshot: unknown) => void = () => {};
  const call = vi.fn<Bridge['call']>().mockResolvedValue(raw);
  const controller = createController({
    call,
    subscribe: async (callback) => {
      emit = callback;
      return () => {};
    },
  });
  render(App, { controller });
  return { call, controller, emit: (next: unknown) => emit(next) };
}
const section = (heading: string) =>
  within(screen.getByRole('heading', { name: heading }).closest('section')!);
const activeCard = () => section('Active account');
const row = (name: string) =>
  within(within(screen.getByRole('table')).getByText(name).closest('tr')!);
async function menu(name = 'Personal') {
  const trigger = await screen.findByRole('button', {
    name: t('en', 'moreLabel', { name }),
  });
  await fireEvent.click(trigger);
  return trigger;
}
/** The open row menu's own buttons. */
const menuItems = (trigger: HTMLElement) =>
  within(document.getElementById(trigger.getAttribute('aria-controls')!)!);
async function details(name = 'Personal') {
  const trigger = await menu(name);
  await fireEvent.click(
    menuItems(trigger).getByRole('button', { name: 'Details' }),
  );
  return screen.getByRole('dialog');
}
afterEach(() => {
  vi.useRealTimers();
  language.set('en');
});
describe('desktop workflows', () => {
  it('restores keyboard focus after Escape cancellation', async () => {
    setup();
    await screen.findAllByText('a@example.invalid');
    const trigger = screen.getByRole('button', { name: 'Open settings' });
    trigger.focus();
    await fireEvent.click(trigger);
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByRole('button', { name: 'Close' })).toHaveFocus();
    await fireEvent(dialog, new Event('cancel', { cancelable: true }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('shows the shared windows, the per-model weekly row and the exact backend usage order', async () => {
    setup();
    await screen.findAllByText('a@example.invalid');
    // Active card 3, two table rows of 2, Next up 2.
    expect(screen.getAllByRole('meter')).toHaveLength(9);
    const active = activeCard();
    expect(
      active.getByRole('meter', { name: '5-hour window' }),
    ).toHaveAttribute('aria-valuenow', '68');
    expect(active.getByRole('meter', { name: 'Weekly' })).toHaveAttribute(
      'aria-valuenow',
      '47',
    );
    expect(
      active.getByRole('meter', { name: 'Weekly · Sonnet' }),
    ).toHaveAttribute('aria-valuenow', '58');
    expect(active.getByText('Sonnet')).toBeVisible();
    expect(
      within(screen.getByRole('table'))
        .getAllByRole('columnheader')
        .map((header) => header.textContent?.trim()),
    ).toEqual(['Account', '5-hour window', 'Weekly', 'Status', 'Actions']);
    const order = within(screen.getByRole('region', { name: 'Next in order' }));
    expect(
      order.getAllByRole('listitem').map((item) => item.textContent),
    ).toEqual(['1Personal']);
    expect(row('Personal').getByText('Next')).toBeVisible();
    // The list and Next up show the weekly value automation uses: here Sonnet's.
    const listWeekly = row('Studio').getByRole('meter', { name: 'Weekly' });
    expect(listWeekly).toHaveAttribute('aria-valuenow', '58');
    expect(listWeekly).toHaveAttribute(
      'aria-valuetext',
      '58 percent used · Sonnet',
    );
    expect(listWeekly.closest('.usage-quota')).toHaveTextContent('58%· Sonnet');
    expect(
      section('Next up').getByRole('meter', { name: 'Weekly' }),
    ).toHaveAttribute('aria-valuenow', '24');
    const dialog = await details('Studio');
    expect(dialog).toHaveTextContent('Available resets');
    expect(dialog).toHaveTextContent('Weekly window started');
    expect(
      within(dialog).getByRole('meter', { name: 'Weekly · Sonnet' }),
    ).toBeVisible();
    expect(dialog).toHaveTextContent(t('en', 'identityVerified'));
  });
  it('shows each weekly window with its own reset time', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-02T12:00:00Z'));
    const raw = snapshot();
    const now = Math.floor(Date.now() / 1000);
    raw.accounts[0].usage!.weeklyOverallResetsAt = now + 3600;
    raw.accounts[0].usage!.scopedLimits[0].resetsAt = now + 7200;
    setup(raw);
    await vi.advanceTimersByTimeAsync(0);
    expect(
      activeCard()
        .getByRole('meter', { name: 'Weekly' })
        .closest('.usage-quota'),
    ).toHaveTextContent('Resets in 1h 0m');
    expect(
      activeCard()
        .getByRole('meter', { name: 'Weekly · Sonnet' })
        .closest('li'),
    ).toHaveTextContent('Resets in 2h 0m');
  });
  it('shows unknown resets without borrowing a known aggregate date', async () => {
    const raw = snapshot();
    delete raw.accounts[0].usage!.weeklyOverallResetsAt;
    raw.accounts[0].usage!.scopedLimits[0].resetsAt = null;
    setup(raw);
    await screen.findAllByText('a@example.invalid');
    expect(
      activeCard()
        .getByRole('meter', { name: 'Weekly' })
        .closest('.usage-quota'),
    ).toHaveTextContent('Reset time unknown');
    expect(
      activeCard()
        .getByRole('meter', { name: 'Weekly · Sonnet' })
        .closest('li'),
    ).not.toHaveTextContent('Resets in');
  });
  it('confirms deletion before sending the stable account ID', async () => {
    const { call } = setup();
    const dialog = await details();
    await fireEvent.click(
      within(dialog).getByRole('button', { name: 'Delete account Personal' }),
    );
    expect(call).not.toHaveBeenCalledWith('delete_account', expect.anything());
    expect(screen.getByRole('dialog')).toHaveTextContent(
      'current Claude Code login',
    );
    await fireEvent.click(
      screen.getByRole('button', { name: /^Delete account$/ }),
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('delete_account', { id: 'b' }),
    );
  });
  it('refreshes from the row menu and keeps Delete off the active account, as Codex does', async () => {
    const { call } = setup();
    const trigger = await menu('Studio');
    await fireEvent.click(
      menuItems(trigger).getByRole('button', { name: 'Refresh' }),
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('refresh_account', { id: 'a' }),
    );
    expect(trigger).toHaveFocus();
    await fireEvent.click(trigger);
    const removeActive = menuItems(trigger).getByRole('button', {
      name: 'Delete',
    });
    expect(removeActive).toBeDisabled();
    expect(removeActive).toHaveAccessibleDescription(
      'Switch to another account before removing this one.',
    );
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(trigger).toHaveFocus();
    // The active account's details say the same.
    const dialog = await details('Studio');
    const removeInDetails = within(dialog).getByRole('button', {
      name: 'Delete account Studio',
    });
    expect(removeInDetails).toBeDisabled();
    expect(removeInDetails).toHaveAccessibleDescription(
      t('en', 'deleteActiveReason'),
    );
    await fireEvent.click(
      within(dialog).getByRole('button', { name: 'Close' }),
    );
    // Any other account is deleted from its menu after confirmation.
    const personal = await menu('Personal');
    const remove = menuItems(personal).getByRole('button', { name: 'Delete' });
    await waitFor(() => expect(remove).toBeEnabled());
    await fireEvent.click(remove);
    expect(screen.getByRole('dialog')).toHaveTextContent(
      t('en', 'deleteClaude', { name: 'Personal' }),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    await waitFor(() => expect(personal).toHaveFocus());
    expect(call).not.toHaveBeenCalledWith('delete_account', expect.anything());
  });
  it('restores the stable table trigger after canceling deletion from account details', async () => {
    setup();
    const trigger = await screen.findByRole('button', {
      name: 'More actions for Personal',
    });
    await fireEvent.click(trigger);
    await fireEvent.click(
      menuItems(trigger).getByRole('button', { name: 'Details' }),
    );
    await fireEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', {
        name: 'Delete account Personal',
      }),
    );
    await fireEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', {
        name: 'Cancel',
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('retains four enabled toggles, polling bounds, and fractional thresholds', async () => {
    const raw = snapshot();
    raw.settings.threshold = 99.5;
    setup(raw);
    await screen.findAllByText('a@example.invalid');
    await fireEvent.click(
      screen.getByRole('button', { name: 'Open settings' }),
    );
    screen
      .getAllByRole('switch')
      .forEach((toggle) => expect(toggle).toBeChecked());
    expect(screen.getAllByRole('switch')).toHaveLength(4);
    expect(screen.getByRole('slider')).toHaveAttribute('min', '120');
    expect(screen.getByRole('slider')).toHaveAttribute('max', '900');
    expect(screen.getByRole('slider')).toHaveAttribute('step', '30');
    expect(screen.getByRole('spinbutton')).toHaveValue(99.5);
    expect(screen.getByText(t('en', 'primeHelp'))).toBeVisible();
    // Resets may switch accounts even with automatic switching off (B17): say so.
    expect(
      screen.getByText(/even when automatic switching is off/),
    ).toBeVisible();
  });
  it('saves the weekly reset order with the other automation settings', async () => {
    const { call } = setup();
    await screen.findAllByText('a@example.invalid');
    expect(
      section('Automation').getByText('Soonest reset first'),
    ).toBeVisible();
    expect(
      section('Next up').getByText(t('en', 'nextWhySoonest')),
    ).toBeVisible();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Open settings' }),
    );
    const order = screen.getByRole('switch', {
      name: new RegExp(`^${t('en', 'preferSoonestReset')}`),
    });
    expect(order).toBeChecked();
    expect(screen.getByText(t('en', 'preferSoonestResetHelp'))).toBeVisible();
    await fireEvent.click(order);
    expect(order).not.toBeChecked();
    const updated = snapshot({ revision: 2 });
    updated.settings.preferSoonestWeeklyReset = false;
    call.mockResolvedValueOnce(updated);
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('update_settings', {
        settings: { ...defaults, preferSoonestWeeklyReset: false },
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(section('Automation').getByText('Most left first')).toBeVisible();
    expect(
      section('Next up').getByText(t('en', 'nextWhyMostLeft')),
    ).toBeVisible();
    await fireEvent.click(
      section('Automation').getByRole('button', { name: 'Manage automation' }),
    );
    expect(
      screen.getByRole('switch', {
        name: new RegExp(`^${t('en', 'preferSoonestReset')}`),
      }),
    ).not.toBeChecked();
  });
  it('says which automation applies to Codex and keeps the Claude-only settings apart', async () => {
    setup();
    await screen.findAllByText('a@example.invalid');
    const automation = section('Automation');
    expect(automation.getByText('Start the weekly window')).toBeVisible();
    expect(automation.getByText('Use resets automatically')).toBeVisible();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Open settings' }),
    );
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent(t('en', 'settingsProviderScope'));
    expect(dialog).not.toHaveTextContent('Automation applies to Claude.');
    const shared = within(
      screen.getByRole('group', { name: t('en', 'sharedAutomation') }),
    );
    expect(
      shared
        .getAllByRole('switch')
        .map(
          (toggle) =>
            toggle.closest('label')?.querySelector('strong')?.textContent,
        ),
    ).toEqual([t('en', 'automaticSwitch'), t('en', 'preferSoonestReset')]);
    expect(shared.getByRole('slider')).toBeVisible();
    expect(shared.getByRole('spinbutton')).toBeVisible();
    expect(shared.getByText(t('en', 'pollingHelp'))).toBeVisible();
    const claudeOnly = within(
      screen.getByRole('group', { name: 'Claude only' }),
    );
    expect(
      claudeOnly
        .getAllByRole('switch')
        .map(
          (toggle) =>
            toggle.closest('label')?.querySelector('strong')?.textContent,
        ),
    ).toEqual([t('en', 'primeWindow'), t('en', 'autoResets')]);
  });
  it('disables every mutation in demo while keeping details readable', async () => {
    const { call } = setup(snapshot({ demo: true }));
    await screen.findByText('Preview');
    expect(
      screen.getByRole('status', { name: 'Demo data · actions are disabled.' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: /Add account/ })).toBeDisabled();
    const switchButton = screen.getByRole('button', { name: 'Switch' });
    expect(switchButton).toBeDisabled();
    expect(switchButton.parentElement).toHaveAttribute(
      'title',
      t('en', 'demoOnly'),
    );
    expect(screen.getByRole('button', { name: 'Refresh all' })).toBeDisabled();
    expect(
      section('Next up').getByRole('button', {
        name: 'Switch now to Personal',
      }),
    ).toBeDisabled();
    const dialog = await details();
    expect(
      within(dialog).getByRole('button', { name: 'Delete account Personal' }),
    ).toBeDisabled();
    expect(
      within(dialog).getByLabelText('Renewal day for Personal'),
    ).toBeDisabled();
    expect(
      call.mock.calls.every(([command]) => command === 'get_snapshot'),
    ).toBe(true);
  });
  it('shows unknown usage and preserves cached readings on failure', async () => {
    setup(
      snapshot({
        accounts: [
          account('a', { usage: null, usageAt: null }),
          account('b', { error: 'providerError', usageAt: 1 }),
        ],
      }),
    );
    await screen.findAllByText('a@example.invalid');
    expect(
      activeCard().getByRole('meter', { name: '5-hour window' }),
    ).not.toHaveAttribute('aria-valuenow');
    expect(activeCard().getByText('Not checked yet')).toBeVisible();
    const failed = row('Personal').getByText('Last check failed');
    expect(failed).toHaveAttribute('title', t('en', 'providerError'));
    const dialog = await details();
    expect(dialog).toHaveTextContent('The last reading is retained.');
  });
  it('opens the add disclosure by keyboard and closes it with Escape', async () => {
    setup();
    const trigger = await screen.findByRole('button', { name: /Add account/ });
    trigger.focus();
    await fireEvent.click(trigger);
    const option = screen.getByRole('button', {
      name: 'Import from ClaudeSwitch…',
    });
    expect(
      screen.getByRole('button', { name: 'Import current Claude Code login' }),
    ).toBeEnabled();
    option.focus();
    await fireEvent.keyDown(option, { key: 'Escape' });
    expect(
      screen.queryByRole('button', { name: 'Import from ClaudeSwitch…' }),
    ).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });
  it('completes browser login using the transient code and session ID', async () => {
    const { call } = setup();
    await screen.findAllByText('a@example.invalid');
    await fireEvent.click(screen.getByRole('button', { name: /Add account/ }));
    call.mockResolvedValueOnce({
      id: 'login-1',
      url: 'https://platform.claude.com/oauth/authorize',
    });
    await fireEvent.click(
      screen.getByRole('button', { name: 'Sign in with Claude…' }),
    );
    expect(
      within(screen.getByRole('dialog')).getByRole('heading', {
        name: 'Sign in with Claude',
      }),
    ).toBeVisible();
    const input = await screen.findByLabelText('Authorization code');
    await waitFor(() => expect(input).toBeEnabled());
    await fireEvent.input(input, { target: { value: 'code#state' } });
    call.mockResolvedValueOnce(snapshot({ revision: 2 }));
    await fireEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', {
        name: 'Add account',
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(call).toHaveBeenLastCalledWith('finish_login', {
      id: 'login-1',
      code: 'code#state',
    });
  });
  it('sends typed renewal/settings updates and persists Romanian language', async () => {
    const { call } = setup();
    const dialog = await details();
    await fireEvent.change(
      within(dialog).getByLabelText('Renewal day for Personal'),
      { target: { value: '31' } },
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('set_renewal_day', {
        id: 'b',
        day: 31,
      }),
    );
    await fireEvent.click(
      within(dialog).getByRole('button', { name: 'Close' }),
    );
    await fireEvent.click(
      screen.getByRole('button', { name: 'Open settings' }),
    );
    await fireEvent.input(screen.getByRole('spinbutton'), {
      target: { value: '92' },
    });
    await fireEvent.change(screen.getByLabelText('Appearance'), {
      target: { value: 'light' },
    });
    await fireEvent.change(screen.getByLabelText('Language'), {
      target: { value: 'ro' },
    });
    const updated = snapshot({ revision: 3 });
    updated.settings.language = 'ro';
    updated.settings.appearance = 'light';
    updated.settings.threshold = 92;
    call.mockResolvedValueOnce(updated);
    await fireEvent.click(screen.getByRole('button', { name: 'Salvează' }));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('update_settings', {
        settings: expect.objectContaining({
          threshold: 92,
          appearance: 'light',
          language: 'ro',
          autoStartWindowEnabled: true,
        }),
      }),
    );
    await screen.findByRole('heading', { name: 'Conturi', level: 1 });
    expect(document.documentElement.lang).toBe('ro');
    expect(
      screen.getByRole('button', { name: 'Deschide setările' }),
    ).toBeVisible();
    expect(screen.getByText(t('ro', 'claudeSubtitle'))).toBeVisible();
    expect(section('Automatizare').getByText('92%')).toBeVisible();
  });
  it('names the account in background errors and keeps a dismissed one hidden', async () => {
    const failing = {
      code: 'providerError',
      accountId: 'a',
      param: null,
      action: null,
      at: 5,
    };
    const { emit } = setup(snapshot({ error: failing }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(`Studio: ${t('en', 'providerError')}`);
    await fireEvent.click(
      within(alert).getByRole('button', { name: 'Dismiss message' }),
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    // The same failure on the next background cycle stays dismissed...
    emit(snapshot({ revision: 2, error: { ...failing, at: 9 } }));
    await waitFor(() =>
      expect(screen.queryByRole('alert')).not.toBeInTheDocument(),
    );
    // ...while a different one is shown, naming its account.
    emit(
      snapshot({
        revision: 3,
        error: {
          code: 'providerError',
          accountId: 'b',
          param: null,
          action: 'autoSwitch',
          at: 12,
        },
      }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      t('en', 'autoSwitchFailed', {
        name: 'Personal',
        reason: t('en', 'providerError'),
      }),
    );
  });
  it('shows a dismissable notice for an interrupted switch found at startup', async () => {
    setup(
      snapshot({
        notice: {
          code: 'switchInterrupted',
          accountId: null,
          param: null,
          action: null,
          at: 3,
        },
      }),
    );
    const text = await screen.findByText(t('en', 'switchInterrupted'));
    await fireEvent.click(
      within(text.closest('[role="status"]') as HTMLElement).getByRole(
        'button',
        { name: 'Dismiss message' },
      ),
    );
    expect(
      screen.queryByText(t('en', 'switchInterrupted')),
    ).not.toBeInTheDocument();
  });
  it('says why Switch is unavailable while an account is still being checked', async () => {
    setup(
      snapshot({
        accounts: [
          account('a'),
          account('b', { identityVerified: false, isNext: false }),
        ],
      }),
    );
    await screen.findAllByText('a@example.invalid');
    const personal = row('Personal');
    const status = personal.getByText('Checking…');
    const button = personal.getByRole('button', { name: 'Switch' });
    expect(button).toBeDisabled();
    expect(button.getAttribute('aria-describedby')).toContain(
      status.closest('[id]')!.id,
    );
    expect(button).toHaveAccessibleDescription(/Personal.*Checking…/);
    expect(button.parentElement).toHaveAttribute(
      'title',
      t('en', 'switchChecking'),
    );
    // A first-launch check is not reported as an identity problem.
    expect(screen.queryByText('Identity unverified')).not.toBeInTheDocument();
  });
  it('offers a browser sign-in for an account Claude no longer accepts', async () => {
    const { call } = setup(
      snapshot({
        accounts: [
          account('a'),
          account('b', { signInRequired: true, error: 'signInRequired' }),
        ],
      }),
    );
    await screen.findAllByText('a@example.invalid');
    const personal = row('Personal');
    expect(personal.queryByRole('button', { name: 'Switch' })).toBeNull();
    expect(
      screen.getByText(t('en', 'signInNeeded', { names: 'Personal' })),
    ).toBeVisible();
    call.mockResolvedValueOnce({
      id: 'login-1',
      url: 'https://platform.claude.com/oauth/authorize',
    });
    const trigger = personal.getByRole('button', { name: 'Sign in again' });
    expect(trigger).toHaveAccessibleDescription('Personal');
    await fireEvent.click(trigger);
    await waitFor(() => expect(call).toHaveBeenCalledWith('begin_login'));
    await fireEvent(
      screen.getByRole('dialog'),
      new Event('cancel', { cancelable: true }),
    );
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('shows when limited accounts free up and who frees up first', async () => {
    const now = Math.floor(Date.now() / 1000);
    const raw = snapshot({
      accounts: [
        account('a', { exhausted: true, freesAt: now + 7800 }),
        account('b', { exhausted: true, freesAt: now + 3600, isNext: false }),
      ],
      consumptionPlan: [],
    });
    // Both 5-hour windows are spent: Claude's own limit, not just the threshold.
    raw.accounts.forEach((spent) => (spent.usage!.fiveHour.utilization = 100));
    setup(raw);
    expect(
      await screen.findByText(
        t('en', 'allLimitedUntil', { name: 'Personal', duration: '1h 0m' }),
      ),
    ).toBeVisible();
    expect(row('Personal').getByText('Limit reached')).toBeVisible();
    expect(row('Personal').getByText(/^Frees in 1h 0m/)).toBeVisible();
    expect(
      row('Studio').getByText(/Limit reached · frees in 2h 10m/),
    ).toBeVisible();
    // The active card offers no switch when no other account is usable.
    expect(
      activeCard().queryByRole('button', { name: /^Switch to/ }),
    ).toBeNull();
    language.set('ro');
    const ro = { ...raw, revision: 2 };
    ro.settings = { ...raw.settings, language: 'ro' };
    cleanup();
    setup(ro);
    expect(
      await screen.findByText(
        'Toate conturile sunt la limită sau aproape de ea. Primul se eliberează Personal, în 1 h 0 min.',
      ),
    ).toBeVisible();
    expect(
      row('Personal').getByText(/^Se eliberează în 1 h 0 min/),
    ).toBeVisible();
  });
  it('calls an account at the switch threshold "Near limit" in amber', async () => {
    const now = Math.floor(Date.now() / 1000);
    const raw = snapshot({
      accounts: [
        account('a'),
        account('b', { exhausted: true, freesAt: now + 3600, isNext: false }),
      ],
      consumptionPlan: [],
    });
    raw.accounts[1].usage!.fiveHour.utilization = 96;
    setup(raw);
    await screen.findAllByText('a@example.invalid');
    const status = row('Personal')
      .getByText('Near limit')
      .closest('[data-kind]');
    expect(status).toHaveAttribute('data-kind', 'near');
    expect(status).toHaveTextContent('Resets in 1h 0m');
    expect(
      row('Personal')
        .getByRole('meter', { name: '5-hour window' })
        .closest('.usage-quota'),
    ).toHaveClass('near-limit');
  });
  it('offers the next account on the active card at the switch threshold', async () => {
    const now = Math.floor(Date.now() / 1000);
    const { call } = setup(
      snapshot({
        accounts: [
          account('a', { exhausted: true, freesAt: now + 7800 }),
          account('b'),
        ],
      }),
    );
    await screen.findAllByText('a@example.invalid');
    expect(
      activeCard().getByText(
        'Reached the 95% switch threshold · resets in 2h 10m',
      ),
    ).toBeVisible();
    call.mockResolvedValueOnce(snapshot({ revision: 2, activeId: 'b' }));
    await fireEvent.click(
      activeCard().getByRole('button', { name: 'Switch to Personal now' }),
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('switch_account', { id: 'b' }),
    );
  });
  it('names a blocking environment variable instead of showing an empty window', async () => {
    setup(
      snapshot({
        accounts: [],
        activeId: null,
        consumptionPlan: [],
        error: {
          code: 'unsupportedEnvironment',
          accountId: null,
          param: 'ANTHROPIC_API_KEY',
          action: null,
          at: 1,
        },
      }),
    );
    expect(
      await screen.findByRole('heading', {
        name: t('en', 'switchingUnavailable'),
      }),
    ).toBeVisible();
    // Stated once, in place of the empty state, not again as a banner.
    expect(
      screen.getByText(
        t('en', 'unsupportedEnvironment', { name: 'ANTHROPIC_API_KEY' }),
      ),
    ).toBeVisible();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', {
        name: 'Import current Claude Code login',
      }),
    ).not.toBeInTheDocument();
  });
  it('labels deletion in account details and maps raw plan tiers', async () => {
    setup(
      snapshot({
        accounts: [
          account('a', { planTier: 'default_claude_max_20x' }),
          account('b', { planTier: 'default_claude_max_5x' }),
        ],
      }),
    );
    expect((await screen.findAllByText('Max 20×'))[0]).toBeVisible();
    const dialog = await details();
    // The only "×" left in the dialog is its own close button.
    expect(
      within(dialog)
        .getAllByRole('button')
        .filter((b) => b.textContent === '×'),
    ).toHaveLength(1);
    const remove = within(dialog).getByRole('button', {
      name: 'Delete account Personal',
    });
    expect(remove).toHaveTextContent('Delete account');
    expect(within(dialog).getByText('Max 5×')).toBeVisible();
  });
  it('updates local countdowns without provider commands', async () => {
    vi.useFakeTimers();
    const { call } = setup();
    await vi.advanceTimersByTimeAsync(0);
    expect(screen.getAllByRole('meter')).toHaveLength(9);
    const before = call.mock.calls.length;
    await vi.advanceTimersByTimeAsync(2000);
    expect(call).toHaveBeenCalledTimes(before);
    expect(
      call.mock.calls.every(([command]) => command === 'get_snapshot'),
    ).toBe(true);
  });
});
