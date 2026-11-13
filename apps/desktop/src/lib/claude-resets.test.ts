import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { claudeView } from './claude-view';
import { resetsView } from './claude-resets';
import { createController, safeError } from './controller';
import type { Bridge } from './bridge';
import { t } from './i18n';
import { account, snapshot } from '../test/fixtures';
import {
  accountSchema,
  snapshotSchema,
  type AccountView,
  type ResetOffer,
} from './types';

const now = 1_800_000_000;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(now * 1000);
});
afterEach(() => vi.useRealTimers());

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
const withOffers = (
  offers: ResetOffer[],
  resets: Partial<AccountView['resets']> = {},
): AccountView => {
  const raw = account('b');
  raw.resets = { ...raw.resets, offers, ...resets };
  return raw;
};

describe('reset offers in the IPC schema', () => {
  it('accepts the native shape and rejects anything else', () => {
    const good = withOffers([offer({ earlyUse: { percentLeft: 35 } })]);
    expect(accountSchema.safeParse(good).success).toBe(true);
    expect(
      accountSchema.safeParse(
        withOffers([offer({ earlyUse: { percentLeft: null } })]),
      ).success,
    ).toBe(true);
    for (const bad of [
      { ...offer(), kind: 'monthly' },
      { ...offer(), unavailableReason: 'because' },
      { ...offer(), grantId: 'g-1' },
      { ...offer(), resetsLeft: -1 },
      { ...offer(), earlyUse: { percentLeft: 101 } },
      { ...offer(), earlyUse: { percentLeft: 5, limit: 'x' } },
    ])
      expect(
        accountSchema.safeParse(withOffers([bad as unknown as ResetOffer]))
          .success,
        JSON.stringify(bad),
      ).toBe(false);
    const missing = { ...good, resets: { ...good.resets } } as Record<
      string,
      unknown
    >;
    delete (missing.resets as Record<string, unknown>).offers;
    expect(accountSchema.safeParse(missing).success).toBe(false);
    expect(snapshotSchema.safeParse(snapshot()).success).toBe(true);
  });
});

describe('Claude resets view-model', () => {
  it('names each kind, its effect, how many are left and when to use it by', () => {
    const raw = withOffers([
      offer({ kind: 'session', resetsLeft: 1, endsAt: null }),
      offer({ kind: 'weekly' }),
      offer({ kind: 'fable', resetsLeft: 1 }),
      offer({
        kind: 'full',
        usableNow: false,
        unavailableReason: 'needsLimit',
      }),
    ]);
    const view = resetsView(raw, now, 'en');
    expect(view.rows.map((r) => r.label)).toEqual([
      '5-hour reset',
      'Weekly reset',
      'Fable reset',
      'Full reset',
    ]);
    expect(view.rows.map((r) => r.left)).toEqual([
      '1 left',
      '2 left',
      '1 left',
      '2 left',
    ]);
    const fable = view.rows[2];
    expect(fable.effect).toBe('Returns what Fable used to your weekly limit.');
    expect(fable.useBy).toMatch(/^Use by .*\d{4}/);
    expect(fable.deadline).not.toBeNull();
    expect(view.rows[0].useBy).toBe('No expiry date');
    expect(view.rows[0].deadline).toBeNull();
    expect(view.rows.map((r) => r.canUse)).toEqual([true, true, true, false]);
    expect(view.rows[3].reason).toBe(
      'Usable once you reach a limit it refills.',
    );
    expect(view.rows[0].reason).toBeNull();
    expect(view.total).toBe(2);
  });

  it('gives every provider reason its own plain words, with a live cooldown', () => {
    const reasons = [
      ['cooldown', 'On cooldown. Try again in a minute.'],
      ['blockedFiveHour', "Can't be used while you're at your 5-hour limit."],
      ['blockedSevenDay', "Can't be used while you're at your weekly limit."],
      ['blockedOpus', "Can't be used while you're at your Opus limit."],
      ['blockedSonnet', "Can't be used while you're at your Sonnet limit."],
      [
        'blockedOther',
        "Can't be used while you're at another limit it doesn't refill.",
      ],
      ['cannotClear', "Doesn't refill the limit you've reached."],
      ['needsLimit', 'Usable once you reach a limit it refills.'],
      ['unavailable', "Can't be used right now."],
      ['pendingClaim', 'Waiting to confirm the last attempt.'],
    ] as const;
    for (const [reason, text] of reasons) {
      const view = resetsView(
        withOffers([offer({ usableNow: false, unavailableReason: reason })]),
        now,
        'en',
      );
      expect(view.rows[0].canUse).toBe(false);
      expect(view.rows[0].reason, reason).toBe(text);
    }
    // A running cooldown says when resets can be used again.
    const cooling = resetsView(
      withOffers([offer({ usableNow: false, unavailableReason: 'cooldown' })], {
        cooldownUntil: now + 600,
      }),
      now,
      'en',
    );
    expect(cooling.rows[0].reason).toBe('Resets can be used again in 10m');
    // A reason the page cannot name never leaves the button enabled.
    const odd = resetsView(
      withOffers([offer({ usableNow: false, unavailableReason: null })]),
      now,
      'en',
    );
    expect(odd.rows[0].reason).toBe("Can't be used right now.");
  });

  it('asks the early-use question with the percent left, or without a number', () => {
    const early = resetsView(
      withOffers([
        offer({ kind: 'full', earlyUse: { percentLeft: 35 } }),
        offer({ kind: 'weekly', earlyUse: { percentLeft: null } }),
        offer({ kind: 'session' }),
      ]),
      now,
      'en',
    );
    expect(early.rows[0].early).toEqual({
      percentLeft: 35,
      text: 'You still have 35% of your limit left. Use this reset anyway?',
    });
    expect(early.rows[1].early).toEqual({
      percentLeft: null,
      text: "You haven't reached a limit yet. Use this reset anyway?",
    });
    expect(early.rows[2].early).toBeNull();
    // A kind that cannot be used is never presented as an early use.
    const blocked = resetsView(
      withOffers([
        offer({
          usableNow: false,
          unavailableReason: 'pendingClaim',
          earlyUse: { percentLeft: 35 },
        }),
      ]),
      now,
      'en',
    );
    expect(blocked.rows[0].early).toBeNull();
  });

  it('marks the kind whose earlier request is unconfirmed as a retry', () => {
    const view = resetsView(
      withOffers(
        [
          offer({ kind: 'weekly' }),
          offer({
            kind: 'fable',
            usableNow: false,
            unavailableReason: 'pendingClaim',
          }),
        ],
        {
          pending: true,
          lastOutcome: 'reset response unconfirmed — retry pending',
        },
      ),
      now,
      'en',
    );
    expect(view.rows.map((r) => r.retry)).toEqual([true, false]);
    expect(view.pending).toBe(true);
    expect(view.lastOutcome).toBe('reset response unconfirmed — retry pending');
  });

  it('is fully localized in Romanian with comma-below letters', () => {
    const view = resetsView(
      withOffers([
        offer({ kind: 'session' }),
        offer({ kind: 'weekly' }),
        offer({
          kind: 'fable',
          usableNow: false,
          unavailableReason: 'needsLimit',
        }),
        offer({ kind: 'full', earlyUse: { percentLeft: 40 } }),
      ]),
      now,
      'ro',
    );
    expect(view.rows.map((r) => r.label)).toEqual([
      'Resetare de 5 ore',
      'Resetare săptămânală',
      'Resetare Fable',
      'Resetare completă',
    ]);
    expect(view.rows[0].left).toBe('Rămase: 2');
    expect(view.rows[2].effect).toBe(
      'Returnează în limita săptămânală ce a consumat Fable.',
    );
    expect(view.rows[2].reason).toBe(
      'Poate fi folosită după ce atingi o limită pe care o reface.',
    );
    expect(view.rows[3].early?.text).toBe(
      'Încă ai 40% din limită. Folosești resetarea oricum?',
    );
    expect(view.rows[0].useBy).toMatch(/^De folosit până la /);
  });

  it('is part of every Claude row the page builds', () => {
    const raw = snapshot({
      accounts: [
        account('a'),
        withOffers([
          offer({ kind: 'fable' }),
          offer({
            kind: 'full',
            usableNow: false,
            unavailableReason: 'unavailable',
          }),
        ]),
      ],
    });
    const rows = claudeView(raw, { locale: 'en', now }).rows;
    expect(rows.find((r) => r.id === 'a')!.resets!.rows).toEqual([]);
    const b = rows.find((r) => r.id === 'b')!.resets!;
    expect(b.rows.map((r) => [r.kind, r.canUse])).toEqual([
      ['fable', true],
      ['full', false],
    ]);
  });
});

describe('using a reset through the controller', () => {
  function setup(raw = snapshot()) {
    const call = vi.fn<Bridge['call']>().mockResolvedValue(raw);
    const c = createController({ call, subscribe: async () => () => {} });
    return { c, call };
  }
  const usable = (kind: ResetOffer['kind'], extra: Partial<ResetOffer> = {}) =>
    snapshot({
      accounts: [account('a'), withOffers([offer({ kind, ...extra })])],
    });

  it('sends exactly the account, the kind and the confirmation, and takes the new snapshot', async () => {
    const { c, call } = setup(usable('fable'));
    await c.start();
    const after = usable('fable');
    after.revision = 9;
    after.accounts[1].resets.lastOutcome = 'reset used — limits refilled';
    call.mockResolvedValueOnce(after);
    expect(await c.useReset('b', 'fable', false)).toBe(true);
    expect(call).toHaveBeenLastCalledWith('claude_use_reset', {
      id: 'b',
      kind: 'fable',
      allowEarly: false,
    });
    expect(get(c).snapshot?.revision).toBe(9);
    expect(get(c).pending).toBeNull();
    c.dispose();
  });

  it('forwards the early-use confirmation only for an offer that is an early use', async () => {
    const early = setup(usable('full', { earlyUse: { percentLeft: 35 } }));
    await early.c.start();
    await early.c.useReset('b', 'full', true);
    expect(early.call).toHaveBeenLastCalledWith('claude_use_reset', {
      id: 'b',
      kind: 'full',
      allowEarly: true,
    });
    early.c.dispose();
    // A stale "yes" cannot turn an ordinary use into a confirmed early one.
    const plain = setup(usable('full'));
    await plain.c.start();
    await plain.c.useReset('b', 'full', true);
    expect(plain.call).toHaveBeenLastCalledWith('claude_use_reset', {
      id: 'b',
      kind: 'full',
      allowEarly: false,
    });
    plain.c.dispose();
  });

  it('sends nothing for a missing, unusable or unknown offer and says why', async () => {
    const blocked = usable('weekly', {
      usableNow: false,
      unavailableReason: 'needsLimit',
    });
    const { c, call } = setup(blocked);
    await c.start();
    const before = call.mock.calls.length;
    for (const [id, kind] of [
      ['b', 'weekly'],
      ['b', 'fable'],
      ['nobody', 'weekly'],
      ['b', 'monthly'],
    ] as const) {
      expect(await c.useReset(id, kind as never, true)).toBe(false);
      expect(get(c).error).toBe(t('en', 'resetNotUsable'));
    }
    expect(call.mock.calls.length).toBe(before);
    c.dispose();
  });

  it('is not started while another command runs or in the demo', async () => {
    const { c, call } = setup(usable('weekly'));
    await c.start();
    const slow = new Promise<unknown>((resolve) =>
      setTimeout(() => resolve(usable('weekly')), 10),
    );
    vi.useRealTimers();
    call.mockReturnValueOnce(slow);
    const first = c.useReset('b', 'weekly', false);
    const calls = call.mock.calls.length;
    expect(await c.useReset('b', 'weekly', false)).toBe(false);
    expect(call.mock.calls.length).toBe(calls);
    await first;
    c.dispose();
    const demo = setup({ ...usable('weekly'), demo: true });
    await demo.c.start();
    const before = demo.call.mock.calls.length;
    expect(await demo.c.useReset('b', 'weekly', false)).toBe(false);
    expect(demo.call.mock.calls.length).toBe(before);
    demo.c.dispose();
  });

  it('maps every native refusal to its catalog text, in the current language', async () => {
    const { c, call } = setup(usable('weekly'));
    await c.start();
    for (const key of [
      'resetNotUsable',
      'resetNeedsLimit',
      'earlyUseNeedsConfirmation',
      'pendingResetError',
      'providerError',
    ] as const) {
      // The native side answers with the message text of its own language.
      call.mockRejectedValueOnce(t('ro', key));
      expect(await c.useReset('b', 'weekly', false)).toBe(false);
      expect(get(c).error).toBe(t('en', key));
      expect(safeError(t('en', key), 'ro')).toBe(t('ro', key));
    }
    // Text that is not a catalog message and looks like a secret is never shown.
    call.mockRejectedValueOnce(new Error('access_token sk-ant-SECRET'));
    await c.useReset('b', 'weekly', false);
    expect(get(c).error).toBe(t('en', 'failedAction'));
    expect(get(c).pending).toBeNull();
    c.dispose();
  });
});
