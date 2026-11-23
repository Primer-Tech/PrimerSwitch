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
const blocked = (
  kind: ResetOffer['kind'],
  reason: NonNullable<ResetOffer['unavailableReason']>,
  overrides: Partial<ResetOffer> = {},
): ResetOffer =>
  offer({ kind, usableNow: false, unavailableReason: reason, ...overrides });
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
/** Three kinds can be used (Full as an early use); the 5-hour one needs a limit. */
const full = (): ResetOffer[] => [
  blocked('session', 'needsLimit', { resetsLeft: 1 }),
  offer({ kind: 'weekly' }),
  offer({ kind: 'fable', resetsLeft: 1 }),
  offer({
    kind: 'full',
    resetsLeft: 1,
    endsAt: null,
    earlyUse: { percentLeft: 35 },
  }),
];
/** Exactly one kind can be used. */
const single = (): ResetOffer[] => [
  blocked('session', 'needsLimit', { resetsLeft: 1 }),
  offer({ kind: 'fable', resetsLeft: 1 }),
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
const resetSection = (details: Awaited<ReturnType<typeof openDetails>>) =>
  within(details.getByRole('region', { name: /^Available resets/ }));
const question = () => screen.getByRole('dialog', { name: 'Use a reset?' });
const chooser = () =>
  screen.getByRole('dialog', { name: 'Which reset do you want to use?' });
const noQuestion = () => {
  expect(screen.queryByRole('dialog', { name: 'Use a reset?' })).toBeNull();
  expect(
    screen.queryByRole('dialog', { name: 'Which reset do you want to use?' }),
  ).toBeNull();
};
const resetInput = (dialog: ReturnType<typeof within>) =>
  dialog.getByRole('textbox', { name: /RESET/ });
async function confirmReset(dialog: ReturnType<typeof within>) {
  await fireEvent.input(resetInput(dialog), { target: { value: 'RESET' } });
  await fireEvent.click(dialog.getByRole('button', { name: 'OK' }));
}
afterEach(() => language.set('en'));

describe('Claude banked resets', () => {
  it('lists each kind in the details as information, with one Use reset button below', async () => {
    setup(snapshot({ accounts: accounts(full()) }));
    const details = await openDetails();
    const section = resetSection(details);
    expect(section.getByRole('heading').textContent).toContain(
      'Available resets',
    );
    const list = section.getByRole('list');
    const rows = within(list).getAllByRole('listitem');
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
    // The kind that needs a limit explains itself in its row.
    expect(
      within(rows[0]).getByText('Usable once you reach a limit it refills.'),
    ).toBeVisible();
    // The rows only inform: the one action is a single button below them.
    expect(within(list).queryAllByRole('button')).toHaveLength(0);
    const buttons = section.getAllByRole('button');
    expect(buttons).toHaveLength(1);
    expect(buttons[0]).toHaveTextContent('Use reset');
    expect(buttons[0]).toBeEnabled();
    expect(buttons[0]).not.toHaveAttribute('aria-describedby');
    expect(
      list.compareDocumentPosition(buttons[0]) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  describe('with one usable kind', () => {
    it('goes straight to the question for that kind, with no choice step', async () => {
      const s = setup(snapshot({ accounts: accounts(single()) }));
      let finish!: (raw: unknown) => void;
      s.reply(
        'claude_use_reset',
        () => new Promise((resolve) => (finish = resolve)),
      );
      const details = await openDetails();
      await fireEvent.click(
        resetSection(details).getByRole('button', { name: 'Use reset' }),
      );
      // Nothing is sent until the question is answered, and no chooser is shown.
      const confirm = within(question());
      expect(
        screen.queryByRole('dialog', {
          name: 'Which reset do you want to use?',
        }),
      ).toBeNull();
      expect(confirm.queryByRole('radiogroup')).toBeNull();
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
      expect(confirm.getByText('Fable reset')).toBeVisible();
      expect(
        confirm.getByText(/Returns what Fable used to your weekly limit\./),
      ).toBeVisible();
      expect(confirm.getByText(/Account: Personal/)).toBeVisible();
      expect(confirm.getByText(/1 left · use by/)).toBeVisible();
      expect(confirm.queryByRole('alert')).toBeNull();
      const ok = confirm.getByRole('button', { name: 'OK' });
      for (const value of ['', 'reset', 'RESET ']) {
        await fireEvent.input(resetInput(confirm), { target: { value } });
        expect(ok).toBeDisabled();
        await fireEvent.click(ok);
      }
      await fireEvent.input(resetInput(confirm), {
        target: { value: 'RESET' },
      });
      expect(ok).toBeEnabled();
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
      await fireEvent.click(ok);
      await waitFor(() =>
        expect(calls(s.call, 'claude_use_reset')).toEqual([
          [
            'claude_use_reset',
            {
              id: 'b',
              kind: 'fable',
              allowEarly: false,
              confirmation: 'RESET',
            },
          ],
        ]),
      );
      // While it runs the button says so, and nothing else can be started.
      expect(
        await confirm.findByRole('button', { name: 'Resetting usage…' }),
      ).toBeDisabled();
      expect(
        resetSection(details).getByRole('button', {
          name: 'Resetting usage…',
        }),
      ).toBeDisabled();
      finish(
        snapshot({
          revision: 5,
          accounts: accounts([single()[0]], [], {
            lastOutcome: 'reset used — limits refilled',
            available: 4,
          }),
        }),
      );
      await waitFor(() => noQuestion());
      // The outcome stays visible in the details, and the spent kind is gone.
      expect(
        await details.findByText('reset used — limits refilled'),
      ).toBeVisible();
      expect(
        await resetSection(details).findByRole('button', { name: 'Use reset' }),
      ).toBeDisabled();
      expect(calls(s.call, 'switch_account')).toHaveLength(0);
    });

    it('keeps the reset when the question is declined', async () => {
      const s = setup(snapshot({ accounts: accounts(single()) }));
      const details = await openDetails();
      await fireEvent.click(
        resetSection(details).getByRole('button', { name: 'Use reset' }),
      );
      await fireEvent.click(
        within(question()).getByRole('button', { name: 'No, keep it' }),
      );
      await waitFor(() => noQuestion());
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
    });

    it('warns about an early use with the percent left, and sends the confirmation', async () => {
      const s = setup(
        snapshot({
          accounts: accounts([
            blocked('weekly', 'needsLimit'),
            offer({ kind: 'full', earlyUse: { percentLeft: 35 } }),
          ]),
        }),
      );
      s.reply('claude_use_reset', () =>
        snapshot({ revision: 7, accounts: accounts([]) }),
      );
      const details = await openDetails();
      await fireEvent.click(
        resetSection(details).getByRole('button', { name: 'Use reset' }),
      );
      const confirm = within(question());
      expect(
        confirm.getByText(
          'You still have 35% of your limit left. Use this reset anyway?',
        ),
      ).toBeVisible();
      await confirmReset(confirm);
      await waitFor(() =>
        expect(calls(s.call, 'claude_use_reset')).toEqual([
          [
            'claude_use_reset',
            { id: 'b', kind: 'full', allowEarly: true, confirmation: 'RESET' },
          ],
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
        resetSection(details).getByRole('button', { name: 'Use reset' }),
      );
      expect(
        screen.getByText(
          "You haven't reached a limit yet. Use this reset anyway?",
        ),
      ).toBeVisible();
    });

    it('keeps the question open with the reason when the native side refuses', async () => {
      const s = setup(snapshot({ accounts: accounts(single()) }));
      s.reply('claude_use_reset', () => {
        throw new Error(t('en', 'resetNeedsLimit'));
      });
      const details = await openDetails();
      await fireEvent.click(
        resetSection(details).getByRole('button', { name: 'Use reset' }),
      );
      const confirm = within(question());
      await confirmReset(confirm);
      expect(
        await confirm.findByText(
          'This reset is usable once you reach a limit it refills.',
        ),
      ).toBeVisible();
      expect(confirm.getByRole('button', { name: 'OK' })).toBeDisabled();
      expect(resetInput(confirm)).toHaveValue('');
    });

    it('requires confirmation to replay a pending claim whose offer disappeared', async () => {
      const s = setup(
        snapshot({
          accounts: accounts([], [], { pending: true, retryPending: true }),
        }),
      );
      s.reply('claude_use_reset', () => snapshot({ revision: 8 }));
      const details = await openDetails();
      await fireEvent.click(
        resetSection(details).getByRole('button', { name: 'Retry' }),
      );
      const confirm = within(
        screen.getByRole('dialog', { name: 'Retry the previous reset?' }),
      );
      expect(confirm.getByRole('button', { name: 'OK' })).toBeDisabled();
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
      await confirmReset(confirm);
      expect(calls(s.call, 'claude_use_reset')).toEqual([
        [
          'claude_use_reset',
          { id: 'b', kind: null, allowEarly: false, confirmation: 'RESET' },
        ],
      ]);
    });

    it('says Retry for an unconfirmed request and still goes straight to its kind', async () => {
      const s = setup(
        snapshot({
          accounts: accounts(
            [offer({ kind: 'weekly' }), blocked('fable', 'pendingClaim')],
            [],
            {
              pending: true,
              lastOutcome: 'reset response unconfirmed — retry pending',
            },
          ),
        }),
      );
      const details = await openDetails();
      const section = resetSection(details);
      // The kind that waits for its answer is blocked; the button repeats the request.
      expect(
        section.getByText('Waiting to confirm the last attempt.'),
      ).toBeVisible();
      expect(section.queryByRole('button', { name: 'Use reset' })).toBeNull();
      expect(
        details.getByText('reset response unconfirmed — retry pending'),
      ).toBeVisible();
      await fireEvent.click(section.getByRole('button', { name: 'Retry' }));
      const confirm = within(question());
      expect(confirm.getByText('Weekly reset')).toBeVisible();
      expect(
        confirm.getByText(
          /Sending the same request again cannot use a second reset/,
        ),
      ).toBeVisible();
      expect(
        screen.queryByRole('dialog', {
          name: 'Which reset do you want to use?',
        }),
      ).toBeNull();
      s.reply('claude_use_reset', () => snapshot({ revision: 8 }));
      await confirmReset(confirm);
      await waitFor(() =>
        expect(calls(s.call, 'claude_use_reset')).toEqual([
          [
            'claude_use_reset',
            {
              id: 'b',
              kind: 'weekly',
              allowEarly: false,
              confirmation: 'RESET',
            },
          ],
        ]),
      );
    });
  });

  describe('with several usable kinds', () => {
    async function openChooser() {
      const s = setup(snapshot({ accounts: accounts(full()) }));
      const details = await openDetails();
      const trigger = resetSection(details).getByRole('button', {
        name: 'Use reset',
      });
      trigger.focus();
      await fireEvent.click(trigger);
      return { s, details, trigger, choice: within(chooser()) };
    }

    it('asks which one in a single dialog, with nothing chosen and confirm disabled', async () => {
      const { s, choice } = await openChooser();
      expect(screen.queryByRole('dialog', { name: 'Use a reset?' })).toBeNull();
      expect(choice.getByText(/Account: Personal/)).toBeVisible();
      const group = within(choice.getByRole('radiogroup'));
      // Only the kinds that can be used are options, in display order.
      const options = group.getAllByRole('radio');
      expect(options.map((o) => o.getAttribute('value'))).toEqual([
        'weekly',
        'fable',
        'full',
      ]);
      expect(group.queryByRole('radio', { name: '5-hour reset' })).toBeNull();
      for (const option of options) expect(option).not.toBeChecked();
      // Each option says what it is, what it does, how many are left and its date.
      expect(group.getByRole('radio', { name: 'Fable reset' })).toBeVisible();
      expect(
        group.getByRole('radio', { name: 'Fable reset' }),
      ).toHaveAccessibleDescription(
        /^1 left Returns what Fable used to your weekly limit\. Use by .*\d{4}/,
      );
      expect(
        group.getByRole('radio', { name: 'Full reset' }),
      ).toHaveAccessibleDescription(
        '1 left Refills all your limits now. No expiry date',
      );
      expect(
        group.getByRole('radio', { name: 'Weekly reset' }),
      ).toHaveAccessibleDescription(/^2 left Refills your weekly limit now\./);
      // No early-use warning until an early kind is chosen, and confirm waits.
      expect(choice.queryByRole('alert')).toBeNull();
      const yes = choice.getByRole('button', { name: 'OK' });
      expect(yes).toBeDisabled();
      expect(yes).toHaveAccessibleDescription('Choose one to continue.');
      expect(choice.getByRole('button', { name: 'No, keep it' })).toBeEnabled();
      await fireEvent.click(yes);
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
    });

    it('uses the kind that was picked, with no early-use confirmation for an ordinary one', async () => {
      const { s, choice } = await openChooser();
      s.reply('claude_use_reset', () => snapshot({ revision: 6 }));
      await fireEvent.click(choice.getByRole('radio', { name: 'Fable reset' }));
      expect(choice.getByRole('radio', { name: 'Fable reset' })).toBeChecked();
      expect(
        choice.getByRole('radio', { name: 'Full reset' }),
      ).not.toBeChecked();
      expect(choice.queryByRole('alert')).toBeNull();
      expect(choice.queryByText('Choose one to continue.')).toBeNull();
      expect(choice.getByRole('button', { name: 'OK' })).toBeDisabled();
      await confirmReset(choice);
      await waitFor(() =>
        expect(calls(s.call, 'claude_use_reset')).toEqual([
          [
            'claude_use_reset',
            {
              id: 'b',
              kind: 'fable',
              allowEarly: false,
              confirmation: 'RESET',
            },
          ],
        ]),
      );
      await waitFor(() => noQuestion());
      expect(calls(s.call, 'switch_account')).toHaveLength(0);
    });

    it('shows the early-use warning for the picked kind and sends the confirmation with it', async () => {
      const { s, choice } = await openChooser();
      s.reply('claude_use_reset', () => snapshot({ revision: 6 }));
      await fireEvent.click(choice.getByRole('radio', { name: 'Full reset' }));
      const warning = choice.getByRole('alert');
      expect(warning).toHaveTextContent(
        'You still have 35% of your limit left. Use this reset anyway?',
      );
      await fireEvent.input(resetInput(choice), { target: { value: 'RESET' } });
      expect(choice.getByRole('button', { name: 'OK' })).toBeEnabled();
      // The warning and typed confirmation belong to the picked kind only.
      await fireEvent.click(choice.getByRole('radio', { name: 'Fable reset' }));
      expect(choice.queryByRole('alert')).toBeNull();
      expect(resetInput(choice)).toHaveValue('');
      expect(choice.getByRole('button', { name: 'OK' })).toBeDisabled();
      await fireEvent.click(choice.getByRole('radio', { name: 'Full reset' }));
      expect(choice.getByRole('alert')).toBeVisible();
      await confirmReset(choice);
      await waitFor(() =>
        expect(calls(s.call, 'claude_use_reset')).toEqual([
          [
            'claude_use_reset',
            { id: 'b', kind: 'full', allowEarly: true, confirmation: 'RESET' },
          ],
        ]),
      );
    });

    it('locks the choice while the request runs and keeps it open with the reason on a refusal', async () => {
      const { s, choice } = await openChooser();
      let fail!: (error: Error) => void;
      s.reply(
        'claude_use_reset',
        () => new Promise((_, reject) => (fail = reject)),
      );
      await fireEvent.click(
        choice.getByRole('radio', { name: 'Weekly reset' }),
      );
      await confirmReset(choice);
      const busy = await choice.findByRole('button', {
        name: 'Resetting usage…',
      });
      expect(busy).toBeDisabled();
      for (const option of choice.getAllByRole('radio'))
        expect(option).toBeDisabled();
      expect(
        choice.getByRole('button', { name: 'No, keep it' }),
      ).toBeDisabled();
      fail(new Error(t('en', 'resetNeedsLimit')));
      expect(
        await choice.findByText(
          'This reset is usable once you reach a limit it refills.',
        ),
      ).toBeVisible();
      // The pick stays, so the owner can try again or choose another kind.
      expect(choice.getByRole('radio', { name: 'Weekly reset' })).toBeChecked();
      expect(await choice.findByRole('button', { name: 'OK' })).toBeDisabled();
      expect(resetInput(choice)).toHaveValue('');
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(1);
    });

    it('sends nothing when nothing is picked and the dialog is declined or dismissed', async () => {
      const { s } = await openChooser();
      await fireEvent.click(
        within(chooser()).getByRole('button', { name: 'No, keep it' }),
      );
      await waitFor(() => noQuestion());
      // A new question starts with nothing chosen again.
      await fireEvent.click(
        resetSection(
          within(screen.getByRole('dialog', { name: 'Personal' })),
        ).getByRole('button', { name: 'Use reset' }),
      );
      for (const option of within(chooser()).getAllByRole('radio'))
        expect(option).not.toBeChecked();
      await fireEvent(chooser(), new Event('cancel', { cancelable: true }));
      await waitFor(() => noQuestion());
      expect(calls(s.call, 'claude_use_reset')).toHaveLength(0);
    });

    it('is a modal dialog, and gives focus back to the button that opened it', async () => {
      const showModal = vi.spyOn(HTMLDialogElement.prototype, 'showModal');
      const { trigger } = await openChooser();
      // The native modal dialog traps focus and closes on Escape.
      expect(showModal.mock.contexts).toContain(chooser());
      showModal.mockRestore();
      expect(trigger).toBeEnabled();
      await fireEvent.click(
        within(chooser()).getByRole('button', { name: 'No, keep it' }),
      );
      await waitFor(() => expect(document.activeElement).toBe(trigger));
      await fireEvent.click(trigger);
      await fireEvent(chooser(), new Event('cancel', { cancelable: true }));
      await waitFor(() => expect(document.activeElement).toBe(trigger));
      await fireEvent.click(trigger);
      await fireEvent.click(
        within(chooser()).getByRole('button', { name: 'Close' }),
      );
      await waitFor(() => expect(document.activeElement).toBe(trigger));
    });

    it('gives focus back to the button after a use that leaves another one to use', async () => {
      const { s, trigger, choice } = await openChooser();
      s.reply('claude_use_reset', () =>
        snapshot({
          revision: 9,
          accounts: accounts(full().filter((o) => o.kind !== 'fable')),
        }),
      );
      await fireEvent.click(choice.getByRole('radio', { name: 'Fable reset' }));
      await confirmReset(choice);
      await waitFor(() => noQuestion());
      await waitFor(() => expect(document.activeElement).toBe(trigger));
    });

    it('drops a pick whose kind is no longer usable', async () => {
      const { s, details, choice } = await openChooser();
      await fireEvent.click(choice.getByRole('radio', { name: 'Full reset' }));
      await fireEvent.input(resetInput(choice), { target: { value: 'RESET' } });
      expect(choice.getByRole('button', { name: 'OK' })).toBeEnabled();
      s.reply('refresh_account', () =>
        snapshot({
          revision: 10,
          accounts: accounts(full().filter((o) => o.kind !== 'full')),
        }),
      );
      await fireEvent.click(details.getByRole('button', { name: /^Refresh/ }));
      await waitFor(() =>
        expect(choice.queryByRole('radio', { name: 'Full reset' })).toBeNull(),
      );
      expect(choice.queryByRole('alert')).toBeNull();
      expect(choice.getByRole('button', { name: 'OK' })).toBeDisabled();
    });
  });

  describe('with nothing usable', () => {
    it('disables the button and points it at the reason the kind gives', async () => {
      setup(
        snapshot({
          accounts: accounts([blocked('session', 'blockedSevenDay')]),
        }),
      );
      const details = await openDetails();
      const button = resetSection(details).getByRole('button', {
        name: 'Use reset',
      });
      expect(button).toBeDisabled();
      expect(button).toHaveAccessibleDescription(
        "Can't be used while you're at your weekly limit.",
      );
    });

    it('says so in one sentence when the kinds give different reasons', async () => {
      setup(
        snapshot({
          accounts: accounts([
            blocked('session', 'needsLimit'),
            blocked('fable', 'cannotClear'),
          ]),
        }),
      );
      const details = await openDetails();
      const section = resetSection(details);
      const button = section.getByRole('button', { name: 'Use reset' });
      expect(button).toBeDisabled();
      expect(button).toHaveAccessibleDescription(
        'None of these resets can be used right now.',
      );
      // Each row still names its own reason.
      expect(
        section.getByText('Usable once you reach a limit it refills.'),
      ).toBeVisible();
      expect(
        section.getByText("Doesn't refill the limit you've reached."),
      ).toBeVisible();
    });

    it('says there are none when the account holds no reset', async () => {
      setup(snapshot({ accounts: accounts([]) }));
      const details = await openDetails();
      expect(details.getByText('No resets available.')).toBeVisible();
      const button = resetSection(details).getByRole('button', {
        name: 'Use reset',
      });
      expect(button).toBeDisabled();
      expect(button).toHaveAccessibleDescription('No resets available.');
    });
  });

  it('also shows the active account its resets in the side card, with the same single button', async () => {
    const s = setup(
      snapshot({
        accounts: accounts(
          [],
          [
            offer({ kind: 'fable', resetsLeft: 1 }),
            blocked('session', 'blockedFiveHour'),
          ],
        ),
      }),
    );
    const card = within(
      await screen.findByRole('region', { name: 'Credits & renewal' }),
    );
    expect(await card.findByText('Fable reset')).toBeVisible();
    expect(card.getByText('5-hour reset')).toBeVisible();
    expect(card.getAllByRole('button')).toHaveLength(1);
    s.reply('claude_use_reset', () => snapshot({ revision: 3 }));
    const trigger = card.getByRole('button', { name: 'Use reset' });
    trigger.focus();
    await fireEvent.click(trigger);
    await confirmReset(within(question()));
    await waitFor(() =>
      expect(calls(s.call, 'claude_use_reset')).toEqual([
        [
          'claude_use_reset',
          { id: 'a', kind: 'fable', allowEarly: false, confirmation: 'RESET' },
        ],
      ]),
    );
  });

  it('gives focus back to the side card button after the single question closes', async () => {
    setup(
      snapshot({
        accounts: accounts([], [offer({ kind: 'fable', resetsLeft: 1 })]),
      }),
    );
    const card = within(
      await screen.findByRole('region', { name: 'Credits & renewal' }),
    );
    const trigger = await card.findByRole('button', { name: 'Use reset' });
    trigger.focus();
    await fireEvent.click(trigger);
    await fireEvent.click(
      within(question()).getByRole('button', { name: 'No, keep it' }),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    await fireEvent.click(trigger);
    await fireEvent(question(), new Event('cancel', { cancelable: true }));
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('opens the chooser from the side card when the active account has several usable kinds', async () => {
    const s = setup(
      snapshot({
        accounts: accounts(
          [],
          [
            offer({ kind: 'fable', resetsLeft: 1 }),
            offer({ kind: 'full', earlyUse: { percentLeft: 12 } }),
          ],
        ),
      }),
    );
    const card = within(
      await screen.findByRole('region', { name: 'Credits & renewal' }),
    );
    await fireEvent.click(
      await card.findByRole('button', { name: 'Use reset' }),
    );
    const choice = within(chooser());
    expect(choice.getByText(/Account: Studio/)).toBeVisible();
    for (const option of choice.getAllByRole('radio'))
      expect(option).not.toBeChecked();
    expect(choice.getByRole('button', { name: 'OK' })).toBeDisabled();
    s.reply('claude_use_reset', () => snapshot({ revision: 3 }));
    await fireEvent.click(choice.getByRole('radio', { name: 'Full reset' }));
    expect(choice.getByRole('alert')).toHaveTextContent(
      'You still have 12% of your limit left.',
    );
    await confirmReset(choice);
    await waitFor(() =>
      expect(calls(s.call, 'claude_use_reset')).toEqual([
        [
          'claude_use_reset',
          { id: 'a', kind: 'full', allowEarly: true, confirmation: 'RESET' },
        ],
      ]),
    );
  });

  it('offers nothing to start in a demo', async () => {
    setup(snapshot({ demo: true, accounts: accounts(full()) }));
    const details = await openDetails();
    const button = resetSection(details).getByRole('button', {
      name: 'Use reset',
    });
    expect(button).toBeDisabled();
    await fireEvent.click(button);
    noQuestion();
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
    expect(
      details.getByText(
        'Poate fi folosită după ce atingi o limită pe care o reface.',
      ),
    ).toBeVisible();
    const button = details.getByRole('button', {
      name: 'Folosește resetarea',
    });
    expect(button).toBeEnabled();
    await fireEvent.click(button);
    const choice = within(
      screen.getByRole('dialog', { name: 'Ce resetare vrei să folosești?' }),
    );
    expect(choice.getByRole('radiogroup')).toBeVisible();
    for (const option of choice.getAllByRole('radio'))
      expect(option).not.toBeChecked();
    const yes = choice.getByRole('button', { name: 'OK' });
    expect(yes).toBeDisabled();
    expect(yes).toHaveAccessibleDescription('Alege una ca să continui.');
    await fireEvent.click(
      choice.getByRole('radio', { name: 'Resetare completă' }),
    );
    expect(
      choice.getByText('Încă ai 35% din limită. Folosești resetarea oricum?'),
    ).toBeVisible();
    expect(
      choice.getByRole('radio', { name: 'Resetare completă' }),
    ).toHaveAccessibleDescription(/^Rămase: 1 Reface acum toate limitele\./);
    expect(choice.getByRole('button', { name: 'Nu, o păstrez' })).toBeEnabled();
    await confirmReset(choice);
    await waitFor(() =>
      expect(calls(s.call, 'claude_use_reset')).toEqual([
        [
          'claude_use_reset',
          { id: 'b', kind: 'full', allowEarly: true, confirmation: 'RESET' },
        ],
      ]),
    );
    expect(document.body.textContent).not.toMatch(/[ŞşŢţ]/);
  });

  it('speaks Romanian for the single question and for nothing usable', async () => {
    language.set('ro');
    setup(
      snapshot({
        settings: { ...defaults, language: 'ro' },
        accounts: accounts(single()),
      }),
    );
    const details = await openDetails('Personal', 'ro');
    await fireEvent.click(
      details.getByRole('button', { name: 'Folosește resetarea' }),
    );
    expect(
      within(
        screen.getByRole('dialog', { name: 'Folosești o resetare?' }),
      ).getByText('Resetare Fable'),
    ).toBeVisible();
    expect(t('ro', 'resetNoneUsable')).toBe(
      'Nicio resetare nu poate fi folosită acum.',
    );
    expect(t('ro', 'resetsNone')).toBe('Nicio resetare disponibilă.');
  });
});
