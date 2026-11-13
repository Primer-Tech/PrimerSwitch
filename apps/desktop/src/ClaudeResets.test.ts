import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import { createController } from './lib/controller';
import type { Bridge, Command } from './lib/bridge';
import { language, t } from './lib/i18n';
import {
  defaults,
  type AccountView,
  type ResetOffer,
  type Snapshot,
} from './lib/types';
import { account, snapshot } from './test/fixtures';

type Handler = (args: unknown) => unknown;
const now = Math.floor(Date.now() / 1000);
const offer = (overrides: Partial<ResetOffer> = {}): ResetOffer => ({
  kind: 'weekly',
  resetsLeft: 2,
  resetsTotal: 3,
  endsAt: now + 5 * 86400,
  usableNow: true,
  unavailableReason: null,
  earlyUse: null,
  ...overrides,
});
/** Studio is active; Personal is saved but not in use. Both hold resets. */
function accounts(
  personal: ResetOffer[],
  studio: ResetOffer[] = [],
  personalResets: Partial<AccountView['resets']> = {},
): AccountView[] {
  const a = account('a');
  a.resets = { ...a.resets, offers: studio };
  const b = account('b');
  b.resets = { ...b.resets, offers: personal, ...personalResets };
  return [a, b];
}
const full = (): ResetOffer[] => [
  offer({
    kind: 'session',
    resetsLeft: 1,
    usableNow: false,
    unavailableReason: 'needsLimit',
  }),
  offer({ kind: 'weekly' }),
  offer({ kind: 'fable', resetsLeft: 1 }),
  offer({
    kind: 'full',
    resetsLeft: 1,
    endsAt: null,
    earlyUse: { percentLeft: 35 },
  }),
];
function setup(raw: Snapshot) {
  const replies: Partial<Record<Command, Handler>> = {};
  const call = vi.fn<Bridge['call']>();
  call.mockImplementation(async (command: Command, args?: unknown) => {
    const reply = replies[command];
    if (reply) return reply(args);
    if (command === 'get_snapshot' || command === 'refresh_account') return raw;
    throw new Error(`unexpected command ${command}`);
  });
  const controller = createController({
    call,
    subscribe: async () => () => {},
  });
  render(App, { controller });
  return {
    call,
    reply: (command: Command, handler: Handler) => {
      replies[command] = handler;
    },
  };
}
const calls = (call: ReturnType<typeof setup>['call'], command: Command) =>
  call.mock.calls.filter(([name]) => name === command);
async function openDetails(name = 'Personal', locale: 'en' | 'ro' = 'en') {
  const more = await screen.findByRole('button', {
    name: t(locale, 'moreLabel', { name }),
  });
  await fireEvent.click(more);
  const menu = document.getElementById(more.getAttribute('aria-controls')!)!;
  await fireEvent.click(
    within(menu).getByRole('button', { name: t(locale, 'viewDetails') }),
  );
  return within(screen.getByRole('dialog', { name }));
}
afterEach(() => language.set('en'));

describe('Claude banked resets', () => {
  it('lists each kind in the details with its count, its date and a Use button', async () => {
    setup(snapshot({ accounts: accounts(full()) }));
    const details = await openDetails();
    const section = within(
      details.getByRole('region', { name: /^Available resets/ }),
    );
    expect(section.getByRole('heading').textContent).toContain(
      'Available resets',
    );
    const rows = section.getAllByRole('listitem');
    expect(
      rows.map((row) => within(row).getByText(/ reset$/).textContent),
    ).toEqual(['5-hour reset', 'Weekly reset', 'Fable reset', 'Full reset']);
    expect(within(rows[1]).getByText('2 left')).toBeVisible();
    expect(rows[1].textContent).toMatch(/Use by .*\d{4}/);
    expect(rows[3].textContent).toContain('No expiry date');
    expect(
      within(rows[2]).getByText(
        /Returns what Fable used to your weekly limit\./,
      ),
    ).toBeVisible();
    // One primary action per row; the one that needs a limit explains itself.
    const buttons = section.getAllByRole('button');
    expect(buttons).toHaveLength(4);
    expect(buttons[0]).toBeDisabled();
    expect(buttons[0]).toHaveAccessibleDescription(
      'Usable once you reach a limit it refills.',
    );
    for (const button of buttons.slice(1)) expect(button).toBeEnabled();
    expect(
      section.getByRole('button', { name: 'Use a Fable reset on Personal' }),
    ).toBeEnabled();
  });

  it('asks first, then uses the chosen kind on the saved account without switching', async () => {
    const s = setup(snapshot({ accounts: accounts(full()) }));
    let finish!: (raw: unknown) => void;
    s.reply(
      'claude_use_reset',
      () => new Promise((resolve) => (finish = resolve)),
    );
    const details = await openDetails();
    await fireEvent.click(
      details.getByRole('button', { name: 'Use a Fable reset on Personal' }),
    );
    // Nothing is sent until the question is answered.
    const confirm = within(
      screen.getByRole('dialog', { name: 'Use a reset?' }),
    );
    expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
    expect(confirm.getByText('Fable reset')).toBeVisible();
    expect(
      confirm.getByText(/Returns what Fable used to your weekly limit\./),
    ).toBeVisible();
    expect(confirm.getByText(/Account: Personal/)).toBeVisible();
    expect(confirm.getByText(/1 left · use by/)).toBeVisible();
    expect(confirm.queryByRole('alert')).toBeNull();
    await fireEvent.click(confirm.getByRole('button', { name: 'Yes, use it' }));
    await waitFor(() =>
      expect(calls(s.call, 'claude_use_reset')).toEqual([
        ['claude_use_reset', { id: 'b', kind: 'fable', allowEarly: false }],
      ]),
    );
    // While it runs the button says so, and nothing else can be started.
    expect(
      await confirm.findByRole('button', { name: 'Resetting usage…' }),
    ).toBeDisabled();
    const after = snapshot({
      revision: 5,
      accounts: accounts(
        full().filter((o) => o.kind !== 'fable'),
        [],
        { lastOutcome: 'reset used — limits refilled', available: 4 },
      ),
    });
    finish(after);
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Use a reset?' })).toBeNull(),
    );
    // The outcome stays visible in the details, and the spent kind is gone.
    expect(
      await details.findByText('reset used — limits refilled'),
    ).toBeVisible();
    expect(
      details.queryByRole('button', { name: 'Use a Fable reset on Personal' }),
    ).toBeNull();
    expect(calls(s.call, 'switch_account')).toHaveLength(0);
  });

  it('keeps the reset when the question is declined', async () => {
    const s = setup(snapshot({ accounts: accounts(full()) }));
    const details = await openDetails();
    await fireEvent.click(
      details.getByRole('button', { name: 'Use a Weekly reset on Personal' }),
    );
    const confirm = within(
      screen.getByRole('dialog', { name: 'Use a reset?' }),
    );
    await fireEvent.click(confirm.getByRole('button', { name: 'No, keep it' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Use a reset?' })).toBeNull(),
    );
    expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
  });

  it('warns about an early use with the percent left, and sends the confirmation', async () => {
    const s = setup(snapshot({ accounts: accounts(full()) }));
    s.reply('claude_use_reset', () =>
      snapshot({ revision: 7, accounts: accounts([]) }),
    );
    const details = await openDetails();
    await fireEvent.click(
      details.getByRole('button', { name: 'Use a Full reset on Personal' }),
    );
    const confirm = within(
      screen.getByRole('dialog', { name: 'Use a reset?' }),
    );
    expect(
      confirm.getByText(
        'You still have 35% of your limit left. Use this reset anyway?',
      ),
    ).toBeVisible();
    await fireEvent.click(confirm.getByRole('button', { name: 'Yes, use it' }));
    await waitFor(() =>
      expect(calls(s.call, 'claude_use_reset')).toEqual([
        ['claude_use_reset', { id: 'b', kind: 'full', allowEarly: true }],
      ]),
    );
  });

  it('says so without a number when the provider gives none', async () => {
    setup(
      snapshot({
        accounts: accounts([
          offer({ kind: 'full', earlyUse: { percentLeft: null } }),
        ]),
      }),
    );
    const details = await openDetails();
    await fireEvent.click(
      details.getByRole('button', { name: 'Use a Full reset on Personal' }),
    );
    expect(
      screen.getByText(
        "You haven't reached a limit yet. Use this reset anyway?",
      ),
    ).toBeVisible();
  });

  it('keeps the question open with the reason when the native side refuses', async () => {
    const s = setup(snapshot({ accounts: accounts(full()) }));
    s.reply('claude_use_reset', () => {
      throw new Error(t('en', 'resetNeedsLimit'));
    });
    const details = await openDetails();
    await fireEvent.click(
      details.getByRole('button', { name: 'Use a Weekly reset on Personal' }),
    );
    const confirm = within(
      screen.getByRole('dialog', { name: 'Use a reset?' }),
    );
    await fireEvent.click(confirm.getByRole('button', { name: 'Yes, use it' }));
    expect(
      await confirm.findByText(
        'This reset is usable once you reach a limit it refills.',
      ),
    ).toBeVisible();
    expect(confirm.getByRole('button', { name: 'Yes, use it' })).toBeEnabled();
  });

  it('retries an unconfirmed request on its own kind and blocks the others', async () => {
    const pending = [
      offer({ kind: 'weekly' }),
      offer({
        kind: 'fable',
        usableNow: false,
        unavailableReason: 'pendingClaim',
      }),
    ];
    setup(
      snapshot({
        accounts: accounts(pending, [], {
          pending: true,
          lastOutcome: 'reset response unconfirmed — retry pending',
        }),
      }),
    );
    const details = await openDetails();
    expect(
      details.getByRole('button', {
        name: 'Retry the Weekly reset on Personal',
      }),
    ).toBeEnabled();
    const fable = details.getByRole('button', {
      name: 'Use a Fable reset on Personal',
    });
    expect(fable).toBeDisabled();
    expect(fable).toHaveAccessibleDescription(
      'Waiting to confirm the last attempt.',
    );
    expect(
      details.getByText('reset response unconfirmed — retry pending'),
    ).toBeVisible();
    await fireEvent.click(
      details.getByRole('button', {
        name: 'Retry the Weekly reset on Personal',
      }),
    );
    expect(
      screen.getByText(
        /Sending the same request again cannot use a second reset/,
      ),
    ).toBeVisible();
  });

  it('also shows the active account its resets in the side card', async () => {
    const s = setup(
      snapshot({
        accounts: accounts(
          [],
          [
            offer({ kind: 'fable', resetsLeft: 1 }),
            offer({
              kind: 'session',
              usableNow: false,
              unavailableReason: 'blockedFiveHour',
            }),
          ],
        ),
      }),
    );
    const card = within(
      await screen.findByRole('region', { name: 'Credits & renewal' }),
    );
    expect(await card.findByText('Fable reset')).toBeVisible();
    expect(card.getByText('5-hour reset')).toBeVisible();
    expect(
      card.getByRole('button', { name: 'Use a 5-hour reset on Studio' }),
    ).toBeDisabled();
    s.reply('claude_use_reset', () => snapshot({ revision: 3 }));
    await fireEvent.click(
      card.getByRole('button', { name: 'Use a Fable reset on Studio' }),
    );
    await fireEvent.click(
      within(screen.getByRole('dialog', { name: 'Use a reset?' })).getByRole(
        'button',
        { name: 'Yes, use it' },
      ),
    );
    await waitFor(() =>
      expect(calls(s.call, 'claude_use_reset')).toEqual([
        ['claude_use_reset', { id: 'a', kind: 'fable', allowEarly: false }],
      ]),
    );
  });

  it('says there are none when the account holds no reset', async () => {
    setup(snapshot({ accounts: accounts([]) }));
    const details = await openDetails();
    expect(details.getByText('No resets available.')).toBeVisible();
    expect(
      details
        .getByRole('region', { name: /^Available resets/ })
        .querySelector('button'),
    ).toBeNull();
  });

  it('offers nothing to start in a demo', async () => {
    setup(snapshot({ demo: true, accounts: accounts(full()) }));
    const details = await openDetails();
    for (const button of within(
      details.getByRole('region', { name: /^Available resets/ }),
    ).getAllByRole('button'))
      expect(button).toBeDisabled();
  });

  it('speaks Romanian throughout', async () => {
    language.set('ro');
    const s = setup(
      snapshot({
        settings: { ...defaults, language: 'ro' },
        accounts: accounts(full()),
      }),
    );
    s.reply('claude_use_reset', () => snapshot({ revision: 4 }));
    const details = await openDetails('Personal', 'ro');
    expect(details.getByText('Resetare săptămânală')).toBeVisible();
    expect(details.getByText('Resetare Fable')).toBeVisible();
    const needs = details.getByRole('button', {
      name: t('ro', 'resetUseLabel', {
        kind: 'Resetare de 5 ore',
        name: 'Personal',
      }),
    });
    expect(needs).toBeDisabled();
    expect(needs).toHaveAccessibleDescription(
      'Poate fi folosită după ce atingi o limită pe care o reface.',
    );
    await fireEvent.click(
      details.getByRole('button', {
        name: t('ro', 'resetUseLabel', {
          kind: 'Resetare completă',
          name: 'Personal',
        }),
      }),
    );
    const confirm = within(
      screen.getByRole('dialog', { name: 'Folosești o resetare?' }),
    );
    expect(
      confirm.getByText('Încă ai 35% din limită. Folosești resetarea oricum?'),
    ).toBeVisible();
    await fireEvent.click(
      confirm.getByRole('button', { name: 'Da, folosește' }),
    );
    await waitFor(() =>
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(1),
    );
    expect(confirm.queryByText(/[ŞşŢţ]/)).toBeNull();
  });
});
