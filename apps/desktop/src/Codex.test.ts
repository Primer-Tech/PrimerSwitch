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
import {
  codexSnapshot,
  codexAccount,
  codexCapability,
} from './test/codex-fixtures';
import { snapshot } from './test/fixtures';
import { language, t } from './lib/i18n';
async function setup(raw = codexSnapshot(), locale: 'en' | 'ro' = 'en') {
  const claude = snapshot();
  claude.settings.language = locale;
  claude.demo = raw.demo;
  const claudeCall = vi.fn().mockResolvedValue(claude),
    call = vi.fn<CodexBridge['call']>().mockResolvedValue(raw);
  const controller = createController({
      call: claudeCall,
      subscribe: async () => () => {},
    }),
    codexController = createCodexController({
      call,
      subscribe: async () => () => {},
    });
  render(App, { controller, codexController });
  await screen.findAllByText('a@example.invalid');
  const tab = screen.getByRole('tab', { name: 'Codex' });
  tab.focus();
  await fireEvent.click(tab);
  await screen.findByRole('heading', {
    name: t(locale, 'codexSelectedAccount'),
  });
  await waitFor(() => expect(get(codexController).snapshot).not.toBeNull());
  await waitFor(() => expect(get(codexController).pending).toBeNull());
  return { call, claudeCall, codexController };
}
afterEach(() => language.set('en'));
describe('Codex dashboard and guarded workflows', () => {
  it('renders every native quota group, nullable permissions, credit strings and unknown reset times', async () => {
    await setup();
    expect(
      screen.getByText('Included usage permission is unknown'),
    ).toBeVisible();
    const active = within(
      screen
        .getByRole('heading', { name: 'Selected for new clients' })
        .closest('section')!,
    );
    expect(
      active.getByRole('meter', { name: '5-hour window' }),
    ).toHaveAttribute('aria-valuenow', '42');
    expect(active.getByRole('meter', { name: '7-day window' })).toHaveAttribute(
      'aria-valuenow',
      '19',
    );
    expect(
      active
        .getByRole('meter', { name: '1-day window' })
        .closest('.usage-quota'),
    ).toHaveTextContent('Reset time unknown');
    expect(screen.getByText('12.50')).toBeVisible();
    expect(screen.getByText('Personal Codex').closest('tr')).toHaveTextContent(
      'Saved reading',
    );
    expect(
      screen.queryByRole('button', { name: 'Manage automation' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText('Consumption order')).not.toBeInTheDocument();
    expect(
      screen.queryByText('Weekly · selected model'),
    ).not.toBeInTheDocument();
  });
  it('supports keyboard provider navigation and keeps the active Codex tab on Codex', async () => {
    const { claudeCall } = await setup();
    const codex = screen.getByRole('tab', { name: 'Codex' });
    await fireEvent.click(codex);
    expect(screen.getByRole('tab', { name: 'Codex' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
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
      screen.getByRole('button', { name: 'Add a Codex account' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent('no code needs to be pasted');
    expect(within(dialog).queryByRole('textbox')).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText('Waiting for browser sign-in…')).toBeVisible(),
    );
    await fireEvent(dialog, new Event('cancel', { cancelable: true }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(call).toHaveBeenCalledWith('codex_cancel_login', { id: 'login-ui' });
    await waitFor(() => expect(add).toHaveFocus());
  });
  it('requires a closed-client acknowledgment before applying an opaque preparation', async () => {
    const { call } = await setup();
    call.mockResolvedValueOnce({
      id: 'prepare-ui',
      accountId: 'codex-b',
      expiresAt: Math.floor(Date.now() / 1000) + 60,
    });
    await fireEvent.click(
      screen.getByRole('button', { name: 'Select account' }),
    );
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent('Reopen them after the change');
    const apply = within(dialog).getByRole('button', {
      name: 'Select account',
    });
    await waitFor(() =>
      expect(within(dialog).getByRole('checkbox')).toBeEnabled(),
    );
    expect(apply).toBeDisabled();
    await fireEvent.click(within(dialog).getByRole('checkbox'));
    expect(apply).toBeEnabled();
    call.mockRejectedValueOnce('clientsRunning');
    await fireEvent.click(apply);
    await waitFor(() =>
      expect(within(dialog).getByRole('alert')).toHaveTextContent(
        'Close Codex clients',
      ),
    );
    expect(
      screen.queryByText('Account selected. Reopen Codex clients to use it.'),
    ).not.toBeInTheDocument();
    expect(call).toHaveBeenLastCalledWith('codex_apply_switch', {
      preparationId: 'prepare-ui',
      clientsClosedAcknowledged: true,
    });
  });
  it('cancels the native preparation when the switch dialog is dismissed', async () => {
    const { call } = await setup();
    const trigger = screen.getByRole('button', { name: 'Select account' });
    call.mockResolvedValueOnce({
      id: 'prepare-cancel',
      accountId: 'codex-b',
      expiresAt: Math.floor(Date.now() / 1000) + 60,
    });
    await fireEvent.click(trigger);
    const dialog = screen.getByRole('dialog');
    await waitFor(() =>
      expect(within(dialog).getByRole('checkbox')).toBeEnabled(),
    );
    await fireEvent.click(
      within(dialog).getByRole('button', { name: 'Cancel' }),
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('codex_cancel_preparation', {
        id: 'prepare-cancel',
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('announces authoritative selection and restores usable provider focus', async () => {
    const { call } = await setup();
    call.mockResolvedValueOnce({
      id: 'prepare-success',
      accountId: 'codex-b',
      expiresAt: Math.floor(Date.now() / 1000) + 60,
    });
    await fireEvent.click(
      screen.getByRole('button', { name: 'Select account' }),
    );
    const dialog = screen.getByRole('dialog');
    await waitFor(() =>
      expect(within(dialog).getByRole('checkbox')).toBeEnabled(),
    );
    await fireEvent.click(within(dialog).getByRole('checkbox'));
    const selected = codexSnapshot({ revision: 2, selectedId: 'codex-b' });
    selected.accounts[0].selected = false;
    selected.accounts[1].selected = true;
    call.mockResolvedValueOnce(selected);
    await fireEvent.click(
      within(dialog).getByRole('button', { name: 'Select account' }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(
      screen.getByText('Account selected. Reopen Codex clients to use it.'),
    ).toBeVisible();
    await waitFor(() =>
      expect(screen.getByRole('tab', { name: 'Codex' })).toHaveFocus(),
    );
    expect(
      screen.getByRole('heading', { name: 'Personal Codex' }),
    ).toBeVisible();
  });
  it('refreshes only the selected account and imports current explicitly', async () => {
    const { call } = await setup();
    await fireEvent.click(
      screen.getByRole('button', {
        name: 'Refresh Codex quota for Studio Codex',
      }),
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('codex_refresh_account', {
        id: 'codex-a',
      }),
    );
    await fireEvent.click(
      screen.getByRole('button', {
        name: 'Account details and actions for Personal Codex',
      }),
    );
    const dialog = screen.getByRole('dialog');
    expect(
      within(dialog).queryByRole('button', { name: /Refresh/ }),
    ).not.toBeInTheDocument();
    await fireEvent.click(
      within(dialog).getByRole('button', { name: 'Close' }),
    );
    await fireEvent.click(screen.getByRole('button', { name: /Add account/ }));
    await fireEvent.click(
      screen.getByRole('button', { name: 'Import current Codex account' }),
    );
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('codex_import_current', undefined),
    );
  });
  it('confirms saved deletion without claiming logout or exposing API-key quota support', async () => {
    const raw = codexSnapshot();
    raw.accounts.push(
      codexAccount('api-fixture', {
        name: 'API workspace',
        authKind: 'apiKey',
        identityVerified: false,
        selected: false,
        quota: codexAccount().quota,
        quotaReadAt: null,
        manualSwitch: codexCapability('unsupportedAuth'),
      }),
    );
    const { call } = await setup(raw);
    const row = screen.getByText('API workspace').closest('tr')!;
    expect(
      within(row).getByRole('button', { name: 'Select account' }),
    ).toBeDisabled();
    const trigger = screen.getByRole('button', {
      name: 'Account details and actions for API workspace',
    });
    await fireEvent.click(trigger);
    expect(screen.getByRole('dialog')).toHaveTextContent(
      'do not have ChatGPT subscription quota',
    );
    expect(
      within(screen.getByRole('dialog')).queryByRole('meter'),
    ).not.toBeInTheDocument();
    await fireEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', {
        name: 'Delete account API workspace',
      }),
    );
    expect(screen.getByRole('dialog')).toHaveTextContent(
      'current Codex sign-in remains unchanged',
    );
    expect(
      call.mock.calls.some(([name]) => name === 'codex_delete_account'),
    ).toBe(false);
    await fireEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', {
        name: 'Cancel',
      }),
    );
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it('permits historical owned accounts after restart and rejects enabled claims-only rows', async () => {
    const raw = codexSnapshot();
    raw.accounts[1].identityVerified = false;
    raw.accounts[1].identityEvidence = 'managedLogin';
    raw.accounts[1].name = 'Historical managed';
    raw.accounts.push(
      codexAccount('historical-backend', {
        name: 'Historical backend',
        selected: false,
        identityVerified: false,
        identityEvidence: 'backendVerified',
      }),
    );
    raw.accounts.push(
      codexAccount('forged-claims', {
        name: 'Unverified claims',
        selected: false,
        identityVerified: true,
        identityEvidence: 'claimsOnly',
      }),
    );
    const { call } = await setup(raw);
    const managed = within(
      screen.getByText('Historical managed').closest('tr')!,
    ).getByRole('button', { name: 'Select account' });
    expect(managed).toBeEnabled();
    expect(
      screen.getByText('Historical managed').closest('tr'),
    ).toHaveTextContent(
      'Quota ownership has not been verified in this session.',
    );
    expect(
      within(screen.getByText('Historical backend').closest('tr')!).getByRole(
        'button',
        { name: 'Select account' },
      ),
    ).toBeEnabled();
    expect(
      within(screen.getByText('Unverified claims').closest('tr')!).getByRole(
        'button',
        { name: 'Select account' },
      ),
    ).toBeDisabled();
    expect(
      within(screen.getByText('Historical managed').closest('tr')!).queryByRole(
        'meter',
      ),
    ).not.toBeInTheDocument();
    call.mockResolvedValueOnce({
      id: 'historical-prepare',
      accountId: 'codex-b',
      expiresAt: Math.floor(Date.now() / 1000) + 60,
    });
    await fireEvent.click(managed);
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith('codex_prepare_switch', {
        id: 'codex-b',
      }),
    );
  });
  it('shows unsupported stores explicitly and keeps restricted actions disabled', async () => {
    const raw = codexSnapshot({
      availability: 'unsupported',
      blockedReason: 'unsupportedStore',
    });
    Object.keys(raw.capabilities).forEach((key) => {
      raw.capabilities[key as keyof typeof raw.capabilities] =
        codexCapability('unsupportedStore');
    });
    await setup(raw);
    expect(
      screen.getAllByText(/credential store is not supported yet/).length,
    ).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: /Add account/ })).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Select account' }),
    ).toBeDisabled();
  });
  it('retains global Romanian settings and read-only demo across providers', async () => {
    const { call } = await setup(codexSnapshot({ demo: true }), 'ro');
    expect(document.documentElement.lang).toBe('ro');
    expect(
      screen.getByRole('button', { name: new RegExp(t('ro', 'addAccount')) }),
    ).toBeDisabled();
    expect(screen.getByRole('tab', { name: 'Codex' })).toBeEnabled();
    await fireEvent.click(
      screen.getByRole('button', { name: t('ro', 'openSettings') }),
    );
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent(t('ro', 'claudeAutomation'));
    expect(
      within(dialog).getByRole('button', { name: t('ro', 'save') }),
    ).toBeDisabled();
    expect(within(dialog).getByLabelText(t('ro', 'language'))).toHaveValue(
      'ro',
    );
    expect(
      call.mock.calls.every(([name]) => name === 'get_codex_snapshot'),
    ).toBe(true);
  });
});
