import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import AccountsTable from './components/AccountsTable.svelte';
import { claudeView } from './lib/claude-view';
import { createController } from './lib/controller';
import type { Bridge, Command } from './lib/bridge';
import { language, t } from './lib/i18n';
import { defaults, type AccountView, type Snapshot } from './lib/types';
import { account, snapshot } from './test/fixtures';

type Handler = (args: unknown) => unknown;
/** Commands that simply answer with the current snapshot. */
const readOnly: readonly Command[] = [
  'get_snapshot',
  'refresh_account',
  'refresh_all',
  'cancel_claude_profile_login',
];
function setup(raw: Snapshot = snapshot({ launchSupported: true })) {
  const replies: Partial<Record<Command, Handler>> = {};
  const call = vi.fn<Bridge['call']>();
  call.mockImplementation(async (command: Command, args?: unknown) => {
    const reply = replies[command];
    if (reply) return reply(args);
    if (readOnly.includes(command)) return raw;
    throw new Error(`unexpected command ${command}`);
  });
  const controller = createController({
    call,
    subscribe: async () => () => {},
  });
  render(App, { controller });
  return {
    call,
    controller,
    reply: (command: Command, handler: Handler) => {
      replies[command] = handler;
    },
  };
}
const commands = (call: ReturnType<typeof setup>['call']) =>
  call.mock.calls.map(([command]) => command);
const table = () => within(screen.getByRole('table'));
const row = (name: string) =>
  within(
    within(screen.getByRole('table').querySelector('tbody')!)
      .getByText(name)
      .closest('tr')!,
  );
const launchButton = (name: string) =>
  table().getByRole('button', { name: `Launch Claude with ${name}` });
const ready = () => screen.findAllByText('a@example.invalid');
const session = (id = 'profile-1') => ({
  id,
  url: 'https://example.invalid/authorize',
});
async function openDialog(name = 'Personal') {
  const trigger = launchButton(name);
  await fireEvent.click(trigger);
  return { trigger, dialog: within(screen.getByRole('dialog')) };
}
async function openDetails(name = 'Personal') {
  const more = await screen.findByRole('button', {
    name: t('en', 'moreLabel', { name }),
  });
  await fireEvent.click(more);
  const menu = document.getElementById(more.getAttribute('aria-controls')!)!;
  await fireEvent.click(within(menu).getByRole('button', { name: 'Details' }));
  return within(screen.getByRole('dialog'));
}
/** A click that moves focus first, as a real pointer or key press does. */
async function press(button: HTMLElement) {
  button.focus();
  await fireEvent.click(button);
}
/** Reaches the code step of a first launch for `Personal`. */
async function toCodeStep(s: ReturnType<typeof setup>) {
  await ready();
  s.reply('claude_launch', () => ({
    status: 'authorizationRequired',
    reason: 'firstUse',
  }));
  s.reply('begin_claude_profile_login', () => session());
  const { dialog } = await openDialog();
  await press(dialog.getByRole('button', { name: 'Launch' }));
  await press(await dialog.findByRole('button', { name: 'Open browser' }));
  const input = await dialog.findByLabelText('Authorization code');
  await waitFor(() => expect(input).toBeEnabled());
  return { dialog, input };
}
function withFable(
  raw: AccountView,
  fable: { label: string; percent: number; resetsAt: number | null } | null,
) {
  raw.usage!.fable = fable;
  return raw;
}
afterEach(() => {
  vi.useRealTimers();
  language.set('en');
});

describe('weekly Fable in the Claude list', () => {
  const now = Math.floor(Date.now() / 1000);
  const accounts = () => [
    withFable(account('a'), {
      label: 'Fable',
      percent: 36,
      resetsAt: now + 2 * 86400 + 3600,
    }),
    withFable(account('b'), null),
  ];
  it('adds a Weekly Fable column next to Weekly, with a neutral state when unknown', async () => {
    // The selected model is Opus, yet Fable still shows its own reading.
    setup(
      snapshot({
        activeModel: 'Opus',
        launchSupported: true,
        accounts: accounts(),
      }),
    );
    await ready();
    expect(
      table()
        .getAllByRole('columnheader')
        .map((header) => header.textContent?.trim()),
    ).toEqual([
      'Account',
      '5-hour window',
      'Weekly',
      'Weekly Fable',
      'Status',
      'Actions',
    ]);
    // Only the Claude table measures itself for the stacked card layout.
    expect(screen.getByRole('table').closest('.table-wrap')).toHaveClass(
      'with-fable',
    );
    const known = row('Studio').getByRole('meter', { name: 'Weekly · Fable' });
    expect(known).toHaveAttribute('aria-valuenow', '36');
    expect(known.closest('.usage-quota')).toHaveTextContent('36%');
    expect(known.closest('.usage-quota')).toHaveTextContent(/Resets in 2d/);
    // The account-wide Weekly stays its own reading.
    expect(
      row('Studio').getByRole('meter', { name: 'Weekly' }),
    ).toHaveAttribute('aria-valuenow', '58');
    const unknown = row('Personal').getByRole('meter', {
      name: 'Weekly · Fable',
    });
    expect(unknown).not.toHaveAttribute('aria-valuenow');
    expect(unknown).toHaveAttribute('aria-valuetext', 'Data unavailable');
    expect(unknown.closest('.usage-quota')).toHaveTextContent('—');
    expect(unknown.closest('.usage-quota')).not.toHaveTextContent('0%');
  });
  it('shows Fable once in the details, and says so when it is missing', async () => {
    setup(snapshot({ launchSupported: true, accounts: accounts() }));
    await ready();
    const known = await openDetails('Studio');
    expect(
      known.getAllByRole('meter', { name: 'Weekly · Fable' }),
    ).toHaveLength(1);
    expect(known.getByRole('meter', { name: 'Weekly · Sonnet' })).toBeVisible();
    expect(known.queryByText(t('en', 'fableUnavailable'))).toBeNull();
    await fireEvent(
      screen.getByRole('dialog'),
      new Event('cancel', { cancelable: true }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    const missing = await openDetails('Personal');
    expect(missing.getByText(t('en', 'fableUnavailable'))).toBeVisible();
    expect(missing.queryByRole('meter', { name: 'Weekly · Fable' })).toBeNull();
  });
  it('is localized in Romanian', async () => {
    setup(
      snapshot({
        launchSupported: true,
        accounts: accounts(),
        settings: { ...defaults, language: 'ro' },
      }),
    );
    expect(
      await screen.findByRole('columnheader', { name: 'Fable săptămânal' }),
    ).toBeVisible();
    expect(
      screen.getByRole('button', {
        name: t('ro', 'launchLabel', { name: 'Personal' }),
      }),
    ).toBeVisible();
  });
  it('renders no Fable column and no launch action for rows that have neither', () => {
    const rows = claudeView(snapshot({ launchSupported: true }), {
      locale: 'en',
      now,
    }).rows.map((entry) => {
      const copy = { ...entry };
      delete copy.fable;
      delete copy.canLaunch;
      delete copy.launchReason;
      return copy;
    });
    render(AccountsTable, {
      rows,
      now,
      threshold: 95,
      locked: false,
      lockReason: null,
      onswitch: vi.fn(),
      onsignin: vi.fn(),
      ondetails: vi.fn(),
      onrefresh: vi.fn(),
      ondelete: vi.fn(),
    });
    expect(
      table()
        .getAllByRole('columnheader')
        .map((header) => header.textContent?.trim()),
    ).toEqual(['Account', '5-hour window', 'Weekly', 'Status', 'Actions']);
    expect(screen.getByRole('table').closest('.table-wrap')).not.toHaveClass(
      'with-fable',
    );
    expect(table().queryByRole('button', { name: /Launch/ })).toBeNull();
    expect(screen.getAllByRole('meter')).toHaveLength(4);
  });
});

describe('Launch Claude from the account list', () => {
  it('launches another account with the chosen model, without switching', async () => {
    const { call, controller, reply } = setup();
    await ready();
    reply('claude_launch', () => ({ status: 'launched' }));
    const { trigger, dialog } = await openDialog('Personal');
    expect(screen.getByRole('dialog')).toHaveAccessibleName('Launch Claude');
    expect(dialog.getByText('Personal')).toBeVisible();
    expect(dialog.getByText('b@example.invalid')).toBeVisible();
    // The shared settings are the default; the explanation is short and honest.
    const model = dialog.getByLabelText('Model');
    expect(
      within(model)
        .getAllByRole('option')
        .map((option) => option.textContent),
    ).toEqual([
      'Default (your shared Claude settings)',
      'Opus',
      'Sonnet',
      'Fable',
    ]);
    expect(model).toHaveValue('default');
    expect(dialog.getByText(t('en', 'launchSharing'))).toBeVisible();
    await fireEvent.change(model, { target: { value: 'opus' } });
    await fireEvent.click(dialog.getByRole('button', { name: 'Launch' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(call).toHaveBeenCalledWith('claude_launch', {
      id: 'b',
      model: 'opus',
    });
    expect(commands(call)).not.toContain('switch_account');
    expect(get(controller).snapshot?.activeId).toBe('a');
    const notice = screen.getByText(
      t('en', 'launchStarted', { name: 'Personal' }),
    );
    expect(notice).toBeVisible();
    await waitFor(() => expect(trigger).toHaveFocus());
    await fireEvent.click(
      within(notice.closest<HTMLElement>('[role="status"]')!).getByRole(
        'button',
        { name: 'Dismiss message' },
      ),
    );
    expect(
      screen.queryByText(t('en', 'launchStarted', { name: 'Personal' })),
    ).toBeNull();
  });
  it.each(['sonnet', 'fable'] as const)(
    'sends the %s model for the account already in use',
    async (model) => {
      const { call, reply } = setup();
      await ready();
      reply('claude_launch', () => ({ status: 'launched' }));
      expect(launchButton('Studio')).toBeEnabled();
      const { dialog } = await openDialog('Studio');
      await fireEvent.change(dialog.getByLabelText('Model'), {
        target: { value: model },
      });
      await fireEvent.click(dialog.getByRole('button', { name: 'Launch' }));
      await waitFor(() =>
        expect(call).toHaveBeenCalledWith('claude_launch', { id: 'a', model }),
      );
      expect(commands(call)).not.toContain('switch_account');
    },
  );
  it('opens from the account details and closes them first', async () => {
    const { call, reply } = setup();
    await ready();
    reply('claude_launch', () => ({ status: 'cancelled' }));
    const details = await openDetails('Personal');
    await fireEvent.click(
      details.getByRole('button', { name: 'Launch Claude with Personal' }),
    );
    await waitFor(() => expect(screen.getAllByRole('dialog')).toHaveLength(1));
    const dialog = within(screen.getByRole('dialog'));
    expect(
      dialog.getByRole('heading', { name: 'Launch Claude' }),
    ).toBeVisible();
    expect(dialog.getByText('b@example.invalid')).toBeVisible();
    expect(commands(call)).not.toContain('claude_launch');
  });
  it('returns cleanly when the native folder choice is cancelled', async () => {
    const { call, reply } = setup();
    await ready();
    reply('claude_launch', () => ({ status: 'cancelled' }));
    const { trigger, dialog } = await openDialog();
    await fireEvent.click(dialog.getByRole('button', { name: 'Launch' }));
    expect(await dialog.findByText(t('en', 'launchNoFolder'))).toBeVisible();
    expect(dialog.getByRole('button', { name: 'Launch' })).toBeEnabled();
    expect(commands(call)).not.toContain('begin_claude_profile_login');
    expect(commands(call)).not.toContain('begin_login');
    await fireEvent.click(dialog.getByRole('button', { name: 'Cancel' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(commands(call)).not.toContain('cancel_claude_profile_login');
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('closes with Escape and gives focus back to the launch button', async () => {
    setup();
    await ready();
    const { trigger } = await openDialog();
    await fireEvent(
      screen.getByRole('dialog'),
      new Event('cancel', { cancelable: true }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('shows a failed start, stays usable and releases every lock', async () => {
    const { call, reply } = setup();
    await ready();
    reply('claude_launch', () => {
      throw 'launchFailed';
    });
    const { dialog } = await openDialog();
    await fireEvent.click(dialog.getByRole('button', { name: 'Launch' }));
    expect(await dialog.findByRole('alert')).toHaveTextContent(
      t('en', 'launchFailed'),
    );
    expect(dialog.getByRole('button', { name: 'Launch' })).toBeEnabled();
    reply('claude_launch', () => {
      throw 'launchNotInstalled';
    });
    await fireEvent.click(dialog.getByRole('button', { name: 'Launch' }));
    await waitFor(() =>
      expect(dialog.getByRole('alert')).toHaveTextContent(
        t('en', 'launchNotInstalled'),
      ),
    );
    await fireEvent.click(dialog.getByRole('button', { name: 'Cancel' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    // Nothing stays locked: the list is usable again.
    expect(launchButton('Personal')).toBeEnabled();
    expect(table().getByRole('button', { name: 'Switch' })).toBeEnabled();
    await fireEvent.click(
      screen.getByRole('button', { name: 'Refresh usage for Studio' }),
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('refresh_account', { id: 'a' }),
    );
    expect(commands(call)).not.toContain('switch_account');
  });
});

describe('launch authorization', () => {
  it('asks before opening the browser, then launches after the pasted code', async () => {
    const s = setup();
    const { dialog, input } = await toCodeStep(s);
    expect(s.call).toHaveBeenCalledWith('begin_claude_profile_login', {
      accountId: 'b',
    });
    // Focus follows the step: the code field is where the user types next.
    await waitFor(() => expect(input).toHaveFocus());
    await fireEvent.input(input, { target: { value: 'code#state' } });
    s.reply('finish_claude_profile_login', () => ({ status: 'launched' }));
    await fireEvent.click(
      dialog.getByRole('button', { name: 'Authorize and launch' }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(s.call).toHaveBeenCalledWith('finish_claude_profile_login', {
      id: 'profile-1',
      code: 'code#state',
    });
    for (const ordinary of [
      'begin_login',
      'finish_login',
      'cancel_login',
      'switch_account',
    ] as const)
      expect(commands(s.call), ordinary).not.toContain(ordinary);
    expect(
      screen.getByText(t('en', 'launchStarted', { name: 'Personal' })),
    ).toBeVisible();
  });
  it('explains the first launch before any browser opens', async () => {
    const s = setup();
    await ready();
    s.reply('claude_launch', () => ({
      status: 'authorizationRequired',
      reason: 'firstUse',
    }));
    const { dialog } = await openDialog();
    await press(dialog.getByRole('button', { name: 'Launch' }));
    expect(
      await dialog.findByText(
        t('en', 'launchAuthFirstUse', { name: 'Personal' }),
      ),
    ).toBeVisible();
    expect(commands(s.call)).not.toContain('begin_claude_profile_login');
    expect(dialog.queryByLabelText('Authorization code')).toBeNull();
    await waitFor(() =>
      expect(
        dialog.getByRole('button', { name: 'Open browser' }),
      ).toHaveFocus(),
    );
  });
  it('reports a wrong login, then retries with a new browser sign-in', async () => {
    const s = setup();
    const { dialog, input } = await toCodeStep(s);
    await fireEvent.input(input, { target: { value: 'wrong#state' } });
    s.reply('finish_claude_profile_login', () => {
      throw 'loginIdentityMismatch';
    });
    await fireEvent.click(
      dialog.getByRole('button', { name: 'Authorize and launch' }),
    );
    expect(await dialog.findByRole('alert')).toHaveTextContent(
      t('en', 'launchWrongLogin'),
    );
    // The consumed code cannot be sent again; a saved account is never added.
    expect(dialog.getByLabelText('Authorization code')).toBeDisabled();
    expect(dialog.getByLabelText('Authorization code')).toHaveValue('');
    expect(
      dialog.getByRole('button', { name: 'Authorize and launch' }),
    ).toBeDisabled();
    s.reply('begin_claude_profile_login', () => session('profile-2'));
    await fireEvent.click(
      dialog.getByRole('button', { name: 'Open browser again' }),
    );
    await waitFor(() =>
      expect(dialog.getByLabelText('Authorization code')).toBeEnabled(),
    );
    expect(dialog.queryByRole('alert')).toBeNull();
    expect(s.call).not.toHaveBeenCalledWith('cancel_claude_profile_login', {
      id: '',
    });
    await fireEvent.input(dialog.getByLabelText('Authorization code'), {
      target: { value: 'right#state' },
    });
    s.reply('finish_claude_profile_login', () => ({ status: 'launched' }));
    await fireEvent.click(
      dialog.getByRole('button', { name: 'Authorize and launch' }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(s.call).toHaveBeenLastCalledWith('finish_claude_profile_login', {
      id: 'profile-2',
      code: 'right#state',
    });
  });
  it('rejects an incomplete code without calling the native side', async () => {
    const s = setup();
    const { dialog, input } = await toCodeStep(s);
    await fireEvent.input(input, { target: { value: 'no-state' } });
    await fireEvent.click(
      dialog.getByRole('button', { name: 'Authorize and launch' }),
    );
    expect(await dialog.findByRole('alert')).toHaveTextContent(
      t('en', 'fullCodeRequired'),
    );
    expect(commands(s.call)).not.toContain('finish_claude_profile_login');
  });
  it('recovers from a failed start after authorization', async () => {
    const s = setup();
    const { dialog, input } = await toCodeStep(s);
    await fireEvent.input(input, { target: { value: 'code#state' } });
    s.reply('finish_claude_profile_login', () => {
      throw 'launchFailed';
    });
    await fireEvent.click(
      dialog.getByRole('button', { name: 'Authorize and launch' }),
    );
    expect(await dialog.findByRole('alert')).toHaveTextContent(
      t('en', 'launchFailed'),
    );
    await fireEvent.click(dialog.getByRole('button', { name: 'Back' }));
    await waitFor(() =>
      expect(dialog.getByRole('button', { name: 'Launch' })).toBeEnabled(),
    );
    expect(dialog.queryByRole('alert')).toBeNull();
    expect(s.call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: '',
    });
    s.reply('claude_launch', () => ({ status: 'launched' }));
    await fireEvent.click(dialog.getByRole('button', { name: 'Launch' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(commands(s.call)).not.toContain('switch_account');
  });
  it('cancels the bound sign-in and releases the window', async () => {
    const s = setup();
    const { dialog } = await toCodeStep(s);
    await fireEvent.click(dialog.getByRole('button', { name: 'Cancel' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(s.call).toHaveBeenCalledWith('cancel_claude_profile_login', {
      id: 'profile-1',
    });
    expect(commands(s.call)).not.toContain('cancel_login');
    expect(launchButton('Personal')).toBeEnabled();
  });
  it('drops a launch that waits for authorization when cancelled before the browser', async () => {
    const s = setup();
    await ready();
    s.reply('claude_launch', () => ({
      status: 'authorizationRequired',
      reason: 'expired',
    }));
    const { dialog } = await openDialog();
    await fireEvent.click(dialog.getByRole('button', { name: 'Launch' }));
    expect(
      await dialog.findByText(
        t('en', 'launchAuthExpired', { name: 'Personal' }),
      ),
    ).toBeVisible();
    await fireEvent.click(dialog.getByRole('button', { name: 'Cancel' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(s.call).toHaveBeenLastCalledWith('cancel_claude_profile_login', {
      id: '',
    });
  });
});

describe('launch is locked out', () => {
  const reasonOf = (name: string) =>
    launchButton(name).closest('span')?.getAttribute('title');
  it.each([
    [
      'an unsupported build',
      snapshot({ launchSupported: false }),
      'launchUnsupported',
    ],
    ['an older native build', snapshot(), 'launchUnsupported'],
    [
      'the preview',
      snapshot({ launchSupported: true, demo: true }),
      'demoOnly',
    ],
  ] as const)('is disabled with its reason for %s', async (_, raw, key) => {
    const { call } = setup(raw);
    await ready();
    for (const name of ['Studio', 'Personal']) {
      expect(launchButton(name)).toBeDisabled();
      expect(reasonOf(name)).toBe(t('en', key));
      await fireEvent.click(launchButton(name));
    }
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(commands(call)).not.toContain('claude_launch');
  });
  it('disables only the account that must sign in or be verified first', async () => {
    const { call } = setup(
      snapshot({
        launchSupported: true,
        accounts: [
          account('a'),
          account('b', {
            name: 'Signed out',
            signInRequired: true,
            error: 'signInRequired',
            decisionFresh: false,
          }),
          account('c', { name: 'Checking', identityVerified: false }),
        ],
      }),
    );
    await ready();
    expect(launchButton('Studio')).toBeEnabled();
    const signIn = launchButton('Signed out');
    const unverified = launchButton('Checking');
    expect(signIn).toBeDisabled();
    expect(signIn.closest('span')).toHaveAttribute(
      'title',
      t('en', 'launchNeedsSignIn'),
    );
    expect(unverified).toBeDisabled();
    expect(unverified.closest('span')).toHaveAttribute(
      'title',
      t('en', 'launchNeedsVerified'),
    );
    await fireEvent.click(signIn);
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(commands(call)).not.toContain('claude_launch');
  });
  it('waits while an action or a refresh is running', async () => {
    setup(snapshot({ launchSupported: true, busy: true }));
    await ready();
    expect(launchButton('Personal')).toBeDisabled();
    expect(reasonOf('Personal')).toBe(t('en', 'switchBusy'));
  });
  it('keeps the rest of the window locked while the dialog is open', async () => {
    const { call } = setup();
    await ready();
    await openDialog();
    // Every list action waits for the dialog, so nothing mixes with the launch.
    expect(launchButton('Studio')).toBeDisabled();
    expect(table().getByRole('button', { name: 'Switch' })).toBeDisabled();
    expect(commands(call)).not.toContain('switch_account');
  });
});
