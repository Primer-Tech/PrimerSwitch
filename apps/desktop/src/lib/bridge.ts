import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  AccountAutomation,
  LaunchModel,
  ResetKind,
  Settings,
} from './types';
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
  /** Uses one banked reset of a kind; the native side re-checks everything. */
  claude_use_reset: {
    id: string;
    kind: ResetKind | null;
    allowEarly: boolean;
    confirmation: string;
  };
  import_current: undefined;
  begin_login: { accountId?: string | null } | undefined;
  finish_login: { id: string; code: string };
  cancel_login: { id: string };
  /** The native side picks the folder and starts the process; no path is sent. */
  claude_launch: { id: string; model: LaunchModel };
  begin_claude_profile_login: { accountId: string };
  finish_claude_profile_login: { id: string; code: string };
  /** An empty id drops a launch that waits for authorization without a session. */
  cancel_claude_profile_login: { id: string };
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
  'claude_use_reset',
  'import_current',
  'begin_login',
  'finish_login',
  'cancel_login',
  'claude_launch',
  'begin_claude_profile_login',
  'finish_claude_profile_login',
  'cancel_claude_profile_login',
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
