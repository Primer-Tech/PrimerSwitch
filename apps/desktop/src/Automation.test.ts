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
import {
  defaultAutomation,
  defaults,
  type AccountAutomation,
  type Settings,
} from './lib/types';
import { snapshot } from './test/fixtures';
import { codexSnapshot } from './test/codex-fixtures';
import { language, t } from './lib/i18n';
import type { ProviderKey } from './lib/provider-view';

afterEach(() => language.set('en'));

async function setup(
  provider: ProviderKey = 'claude',
  settings: Partial<Settings> = {},
  preference: AccountAutomation = defaultAutomation,
) {
  let claude = snapshot({ settings: { ...defaults, ...settings } });
  let codex = codexSnapshot();
  const now = Math.floor(Date.now() / 1000);
  claude.returnHoldUntil = now + 15 * 60;
  codex.returnHoldUntil = now + 15 * 60;
  claude.accounts[0].automation = { ...preference };
  codex.accounts[0].automation = { ...preference };
  let emitClaude: (raw: unknown) => void = () => {};
  const claudeCall = vi.fn(async (command: string, args?: unknown) => {
    if (command === 'update_settings') {
      claude = {
        ...claude,
        revision: claude.revision + 1,
        settings: (args as { settings: Settings }).settings,
      };
    } else if (command === 'update_account_automation') {
      const { id, automation } = args as {
        id: string;
        automation: AccountAutomation;
      };
      claude = {
        ...claude,
        revision: claude.revision + 1,
        accounts: claude.accounts.map((a) =>
          a.id === id
            ? {
                ...a,
                automation,
                effectiveThreshold:
                  automation.threshold ??
                  claude.settings.claudeThreshold ??
                  claude.settings.threshold,
              }
            : a,
        ),
      };
    } else if (command === 'resume_automatic_returns') {
      claude = {
        ...claude,
        revision: claude.revision + 1,
        returnHoldUntil: null,
      };
    }
    return structuredClone(claude);
  });
  const codexCall = vi.fn(async (command: string, args?: unknown) => {
    if (command === 'codex_update_account_automation') {
      const { id, automation } = args as {
        id: string;
        automation: AccountAutomation;
      };
      codex = {
        ...codex,
        revision: codex.revision + 1,
        accounts: codex.accounts.map((a) =>
          a.id === id
            ? {
                ...a,
                automation,
                effectiveThreshold:
                  automation.threshold ??
                  claude.settings.codexThreshold ??
                  claude.settings.threshold,
              }
            : a,
        ),
      };
    } else if (command === 'codex_resume_automatic_returns') {
      codex = { ...codex, revision: codex.revision + 1, returnHoldUntil: null };
    }
    return structuredClone(codex);
  });
  const controller = createController({
    call: claudeCall,
    subscribe: async (emit) => {
      emitClaude = emit;
      return () => {};
    },
  });
  const codexController = createCodexController({
    call: codexCall,
    subscribe: async () => () => {},
  });
  render(App, { controller, codexController });
  await screen.findAllByText('a@example.invalid');
  if (provider === 'codex') {
    await fireEvent.click(screen.getByRole('tab', { name: 'Codex' }));
    await waitFor(() => expect(get(codexController).snapshot).not.toBeNull());
    await waitFor(() => expect(get(codexController).pending).toBeNull());
  }
  return { claudeCall, codexCall, controller, codexController, emitClaude };
}
const section = (name: string) =>
  within(screen.getByRole('heading', { name }).closest('section')!);
const settingsDialog = async () => {
  await fireEvent.click(screen.getByRole('button', { name: 'Open settings' }));
  return within(screen.getByRole('dialog'));
};
const detailsDialog = async () => {
  await fireEvent.click(
    section('Active account').getByRole('button', { name: 'Details' }),
  );
  return within(screen.getByRole('dialog'));
};

describe('independent defaults and reliable percentage editing', () => {
  it('starts both defaults from the legacy value and preserves a decimal when deliberately stepping or scrolling', async () => {
    const { claudeCall } = await setup('claude', { threshold: 99.5 });
    const dialog = await settingsDialog();
    const claude = dialog.getByLabelText(t('en', 'claudeThresholdUsed'));
    const codex = dialog.getByLabelText(t('en', 'codexThresholdUsed'));
    expect(claude).toHaveValue('99.5');
    expect(codex).toHaveValue('99.5');
    expect(claude).toHaveAttribute('type', 'text');
    claude.focus();
    await fireEvent.wheel(claude, { deltaY: 120 });
    expect(claude).toHaveValue('99.5');
    await fireEvent.click(
      dialog.getByRole('button', {
        name: t('en', 'decreasePercent', {
          label: t('en', 'claudeThresholdUsed'),
        }),
      }),
    );
    expect(claude).toHaveValue('98.5');
    await fireEvent.click(dialog.getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(claudeCall).toHaveBeenCalledWith('update_settings', {
        settings: {
          ...defaults,
          threshold: 99.5,
          claudeThreshold: 98.5,
          codexThreshold: 99.5,
        },
      }),
    );
    expect(
      await screen.findByText(
        'Saved defaults: Claude 98.5% used · Codex 99.5% used.',
      ),
    ).toBeVisible();
  });

  it.each(['claude', 'codex'] as const)(
    'saves both explicit provider defaults from the %s tab without coupling them',
    async (provider) => {
      const { claudeCall } = await setup(provider, {
        threshold: 95,
        claudeThreshold: 82.25,
        codexThreshold: 97.5,
      });
      const dialog = await settingsDialog();
      await fireEvent.input(
        dialog.getByLabelText(
          t(
            'en',
            provider === 'claude'
              ? 'claudeThresholdUsed'
              : 'codexThresholdUsed',
          ),
        ),
        { target: { value: '86,125' } },
      );
      await fireEvent.click(dialog.getByRole('button', { name: 'Save' }));
      await waitFor(() =>
        expect(claudeCall).toHaveBeenCalledWith('update_settings', {
          settings: {
            ...defaults,
            claudeThreshold: provider === 'claude' ? 86.125 : 82.25,
            codexThreshold: provider === 'codex' ? 86.125 : 97.5,
          },
        }),
      );
    },
  );

  it.each(['49.9', '100.1', '', '95abc', '99,5.5'])(
    'shows a validation error and preserves invalid draft %j without sending it',
    async (value) => {
      const { claudeCall } = await setup();
      const dialog = await settingsDialog();
      const field = dialog.getByLabelText(t('en', 'claudeThresholdUsed'));
      await fireEvent.input(field, { target: { value } });
      await fireEvent.click(dialog.getByRole('button', { name: 'Save' }));
      expect(dialog.getByRole('alert')).toHaveTextContent(
        t('en', 'invalidPercent'),
      );
      expect(field).toHaveValue(value);
      expect(field).toHaveFocus();
      expect(claudeCall).not.toHaveBeenCalledWith(
        'update_settings',
        expect.anything(),
      );
    },
  );

  it('retains an open draft through background snapshots and reports the actual native saved values', async () => {
    const { claudeCall, emitClaude } = await setup('claude', {
      claudeThreshold: 85,
      codexThreshold: 98,
    });
    const dialog = await settingsDialog();
    await fireEvent.input(
      dialog.getByLabelText(t('en', 'claudeThresholdUsed')),
      { target: { value: '88.125' } },
    );
    emitClaude(
      snapshot({
        revision: 5,
        settings: { ...defaults, claudeThreshold: 70, codexThreshold: 80 },
      }),
    );
    expect(dialog.getByLabelText(t('en', 'claudeThresholdUsed'))).toHaveValue(
      '88.125',
    );
    expect(dialog.getByLabelText(t('en', 'codexThresholdUsed'))).toHaveValue(
      '98',
    );
    claudeCall.mockResolvedValueOnce(
      snapshot({
        revision: 6,
        settings: {
          ...defaults,
          claudeThreshold: 88.125,
          codexThreshold: 97.875,
        },
      }),
    );
    await fireEvent.click(dialog.getByRole('button', { name: 'Save' }));
    expect(
      await screen.findByText(
        'Saved defaults: Claude 88.125% used · Codex 97.875% used.',
      ),
    ).toBeVisible();
  });
});

describe.each(['claude', 'codex'] as const)(
  '%s account automation uses the shared editor',
  (provider) => {
    it('saves an exact custom threshold, priority and return preference; can revert to provider inheritance', async () => {
      const state = await setup(provider, {
        claudeThreshold: 82.25,
        codexThreshold: 97.5,
      });
      const call = provider === 'claude' ? state.claudeCall : state.codexCall;
      const id = provider === 'claude' ? 'a' : 'codex-studio';
      const command =
        provider === 'claude'
          ? 'update_account_automation'
          : 'codex_update_account_automation';
      const dialog = await detailsDialog();
      const mode = dialog.getByLabelText(
        t('en', 'switchThresholdUsed'),
      ) as HTMLSelectElement;
      expect(mode.selectedOptions[0]).toHaveTextContent(
        provider === 'claude' ? '82.25% used' : '97.5% used',
      );
      const returns = dialog.getByRole('switch', {
        name: new RegExp(`^${t('en', 'returnAfterReset')}`),
      });
      expect(returns).toBeDisabled();
      await fireEvent.change(mode, { target: { selectedIndex: 1 } });
      await fireEvent.input(
        dialog.getByLabelText(t('en', 'customThresholdUsed')),
        { target: { value: '79,125' } },
      );
      await fireEvent.input(dialog.getByLabelText(t('en', 'accountPriority')), {
        target: { value: '1' },
      });
      expect(returns).toBeEnabled();
      await fireEvent.click(returns);
      await fireEvent.click(
        dialog.getByRole('button', { name: 'Save automation' }),
      );
      await waitFor(() =>
        expect(call).toHaveBeenCalledWith(command, {
          id,
          automation: {
            enabled: true,
            threshold: 79.125,
            priority: 1,
            returnOnFiveHourReset: true,
          },
        }),
      );
      expect(await dialog.findByRole('status')).toHaveTextContent(
        'Saved: 79.125% used (Custom threshold)',
      );
      expect(dialog.getByText(t('en', 'manualReturnHelp'))).toBeVisible();
      await fireEvent.change(mode, { target: { selectedIndex: 0 } });
      await fireEvent.click(
        dialog.getByRole('button', { name: 'Save automation' }),
      );
      await waitFor(() =>
        expect(call).toHaveBeenLastCalledWith(command, {
          id,
          automation: {
            enabled: true,
            threshold: null,
            priority: 1,
            returnOnFiveHourReset: true,
          },
        }),
      );
      expect(await dialog.findByRole('status')).toHaveTextContent(
        provider === 'claude'
          ? 'Saved: 82.25% used (Provider default)'
          : 'Saved: 97.5% used (Provider default)',
      );
    });

    it('preserves a saved return preference while excluded and rejects missing priority when included', async () => {
      const pref = {
        enabled: true,
        threshold: null,
        priority: 1,
        returnOnFiveHourReset: true,
      };
      const state = await setup(provider, {}, pref);
      const call = provider === 'claude' ? state.claudeCall : state.codexCall;
      const dialog = await detailsDialog();
      const include = dialog.getByRole('switch', {
        name: new RegExp(`^${t('en', 'includeAutomatic')}`),
      });
      await fireEvent.click(include);
      expect(
        dialog.getByRole('switch', {
          name: new RegExp(`^${t('en', 'returnAfterReset')}`),
        }),
      ).toBeChecked();
      expect(dialog.getByText(t('en', 'returnExcludedHelp'))).toBeVisible();
      await fireEvent.click(
        dialog.getByRole('button', { name: 'Save automation' }),
      );
      await waitFor(() =>
        expect(call).toHaveBeenCalledWith(
          expect.stringContaining('update_account_automation'),
          expect.objectContaining({ automation: { ...pref, enabled: false } }),
        ),
      );
      await fireEvent.click(include);
      await fireEvent.input(dialog.getByLabelText(t('en', 'accountPriority')), {
        target: { value: '' },
      });
      const before = call.mock.calls.length;
      await fireEvent.click(
        dialog.getByRole('button', { name: 'Save automation' }),
      );
      expect(dialog.getByRole('alert')).toHaveTextContent(
        t('en', 'returnPriorityRequired'),
      );
      expect(call.mock.calls.length).toBe(before);
    });

    it('cancels draft edits without a mutation and resumes the visible manual hold', async () => {
      const state = await setup(provider);
      const call = provider === 'claude' ? state.claudeCall : state.codexCall;
      const dialog = await detailsDialog();
      await fireEvent.input(dialog.getByLabelText(t('en', 'accountPriority')), {
        target: { value: '3' },
      });
      await fireEvent.click(dialog.getByRole('button', { name: 'Cancel' }));
      expect(dialog.getByLabelText(t('en', 'accountPriority'))).toHaveValue('');
      expect(
        call.mock.calls.some(([name]) =>
          name.includes('update_account_automation'),
        ),
      ).toBe(false);
      await fireEvent.click(dialog.getByRole('button', { name: 'Close' }));
      const automation = section('Automation');
      expect(automation.getByRole('status')).toHaveTextContent(
        'Automatic returns paused for 15m',
      );
      await fireEvent.click(
        automation.getByRole('button', { name: 'Resume automatic returns' }),
      );
      await waitFor(() =>
        expect(call).toHaveBeenCalledWith(
          provider === 'claude'
            ? 'resume_automatic_returns'
            : 'codex_resume_automatic_returns',
          undefined,
        ),
      );
      await waitFor(() =>
        expect(
          automation.queryByRole('button', {
            name: 'Resume automatic returns',
          }),
        ).toBeNull(),
      );
    });
  },
);
