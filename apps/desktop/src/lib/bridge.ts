import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AccountAutomation, Settings } from './types';
import { t } from './i18n';

export interface CommandArgs {
  get_snapshot: undefined;
  refresh_all: undefined;
  refresh_account: { id: string };
  switch_account: { id: string };
  delete_account: { id: string };
  set_renewal_day: { id: string; day: number | null };
  update_settings: { settings: Settings };
  update_account_automation: { id: string; automation: AccountAutomation };
  resume_automatic_returns: undefined;
  import_current: undefined;
  begin_login: { accountId?: string | null } | undefined;
  finish_login: { id: string; code: string };
  cancel_login: { id: string };
  preview_legacy_import: undefined;
  apply_legacy_import: { previewId: string };
}
export type Command = keyof CommandArgs;
export interface Bridge {
  call<C extends Command>(command: C, args?: CommandArgs[C]): Promise<unknown>;
  subscribe(callback: (snapshot: unknown) => void): Promise<() => void>;
}
const commands: readonly Command[] = [
  'get_snapshot',
  'refresh_all',
  'refresh_account',
  'switch_account',
  'delete_account',
  'set_renewal_day',
  'update_settings',
  'update_account_automation',
  'resume_automatic_returns',
  'import_current',
  'begin_login',
  'finish_login',
  'cancel_login',
  'preview_legacy_import',
  'apply_legacy_import',
];
export const nativeBridge: Bridge = {
  call(command, args) {
    if (!commands.includes(command))
      return Promise.reject(new Error(t('en', 'actionUnavailable')));
    return invoke(command, args);
  },
  subscribe(callback) {
    return listen('snapshot_changed', (event) => callback(event.payload));
  },
};
