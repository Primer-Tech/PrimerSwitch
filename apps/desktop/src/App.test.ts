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
async function details(name = 'Personal') {
  await fireEvent.click(
    await screen.findByRole('button', {
      name: t('en', 'detailsLabel', { name }),
    }),
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
  it('renders three authoritative active quotas and exact backend consumption order', async () => {
    setup();
    await screen.findAllByText('a@example.invalid');
    expect(screen.getAllByRole('meter')).toHaveLength(9);
    expect(screen.getByRole('meter', { name: 'Last 5 hours' })).toHaveAttribute(
      'aria-valuenow',
      '68',
    );
    expect(
      screen.getByRole('meter', { name: 'Weekly · overall' }),
    ).toHaveAttribute('aria-valuenow');
    expect(
      within(screen.getByRole('list')).getAllByRole('listitem')[0],
    ).toHaveTextContent('Personal');
    expect(
      screen.getByRole('columnheader', {
        name: t('en', 'weeklyEffectiveHelp'),
      }),
    ).toHaveAttribute('title', t('en', 'weeklyEffectiveHelp'));
    const dialog = await details('Studio');
    expect(dialog).toHaveTextContent('Available resets');
    expect(dialog).toHaveTextContent('Weekly window started');
    expect(dialog).toHaveTextContent('selected model');
  });

  it('uses authoritative overall/model reset projections when their dates differ', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-10-02T12:00:00Z'));
    const raw = snapshot();
    const now = Math.floor(Date.now() / 1000);
    raw.accounts[0].usage!.weeklyOverallResetsAt = now + 3600;
    raw.accounts[0].usage!.weeklyModelResetsAt = now + 7200;
    setup(raw);
    await vi.advanceTimersByTimeAsync(0);
    expect(
      screen
        .getByRole('meter', { name: 'Weekly · overall' })
        .closest('.usage-quota'),
    ).toHaveTextContent('Resets in 1h 0m 0s');
    expect(
      screen
        .getByRole('meter', { name: 'Weekly · selected model' })
        .closest('.usage-quota'),
    ).toHaveTextContent('Resets in 2h 0m 0s');
  });
  it('shows unknown binding resets without borrowing a known aggregate date', async () => {
    const raw = snapshot();
    delete raw.accounts[0].usage!.weeklyOverallResetsAt;
    raw.accounts[0].usage!.weeklyModelResetsAt = null;
    setup(raw);
    await screen.findAllByText('a@example.invalid');
    expect(
      screen
        .getByRole('meter', { name: 'Weekly · overall' })
        .closest('.usage-quota'),
    ).toHaveTextContent('Reset time unknown');
    expect(
      screen
        .getByRole('meter', { name: 'Weekly · selected model' })
        .closest('.usage-quota'),
    ).toHaveTextContent('Reset time unknown');
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

  it('restores the stable table trigger after canceling deletion from account details', async () => {
    setup();
    const trigger = await screen.findByRole('button', {
      name: 'Account details and actions for Personal',
    });
    await fireEvent.click(trigger);
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
  it('retains three enabled toggles, polling bounds, and fractional thresholds', async () => {
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
    expect(screen.getAllByRole('switch')).toHaveLength(3);
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
  it('disables every mutation in demo while keeping details readable', async () => {
    const { call } = setup(snapshot({ demo: true }));
    await screen.findByText('Preview');
    expect(
      screen.getByRole('status', { name: 'Demo data · actions are disabled.' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: /Add account/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Switch' })).toBeDisabled();
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
    await screen.findByText('No usage data');
    expect(
      screen.getByRole('meter', { name: 'Last 5 hours' }),
    ).not.toHaveAttribute('aria-valuenow');
    expect(screen.getByText('The last reading is retained.')).toBeVisible();
  });
  it('opens the add disclosure by keyboard and closes it with Escape', async () => {
    setup();
    const trigger = await screen.findByRole('button', { name: /Add account/ });
    trigger.focus();
    await fireEvent.click(trigger);
    const option = screen.getByRole('button', { name: 'Import archive' });
    option.focus();
    await fireEvent.keyDown(option, { key: 'Escape' });
    expect(
      screen.queryByRole('button', { name: 'Import archive' }),
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
      screen.getByRole('button', { name: 'Add a Claude account' }),
    );
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
      within(text.parentElement!).getByRole('button', {
        name: 'Dismiss message',
      }),
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
    const reason = await screen.findByText(t('en', 'switchChecking'));
    const button = screen.getByRole('button', { name: 'Switch' });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('aria-describedby', reason.id);
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
    expect(await screen.findByText(t('en', 'switchNeedsSignIn'))).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Switch' }),
    ).not.toBeInTheDocument();
    call.mockResolvedValueOnce({
      id: 'login-1',
      url: 'https://platform.claude.com/oauth/authorize',
    });
    await fireEvent.click(
      screen.getByRole('button', { name: 'Sign in again to Personal' }),
    );
    await waitFor(() => expect(call).toHaveBeenCalledWith('begin_login'));
  });
  it('shows when exhausted accounts free up and who frees up first', async () => {
    const now = Math.floor(Date.now() / 1000);
    const raw = snapshot({
      accounts: [
        account('a', { exhausted: true, freesAt: now + 7800 }),
        account('b', { exhausted: true, freesAt: now + 3600 }),
      ],
      consumptionPlan: [],
    });
    setup(raw);
    expect(
      await screen.findByText(
        t('en', 'allExhaustedUntil', { name: 'Personal', duration: '1h 0m' }),
      ),
    ).toBeVisible();
    expect(
      screen.getByText(t('en', 'freesIn', { duration: '2h 10m' })),
    ).toBeVisible();
    language.set('ro');
    const ro = { ...raw, revision: 2 };
    ro.settings = { ...raw.settings, language: 'ro' };
    cleanup();
    setup(ro);
    expect(
      await screen.findByText(
        'Toate conturile sunt la limită. Primul cont liber va fi Personal, în 1 h 0 min.',
      ),
    ).toBeVisible();
    expect(screen.getByText('Se eliberează în 2 h 10 min')).toBeVisible();
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
      screen.queryByRole('button', { name: 'Import current account' }),
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
    expect(await screen.findByText('Max 20×')).toBeVisible();
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
