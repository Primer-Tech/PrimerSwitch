import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
export interface CodexCommandArgs {
  get_codex_snapshot: undefined;
  codex_discover: undefined;
  codex_begin_login: undefined;
  codex_poll_login: { id: string };
  codex_cancel_login: { id: string };
  codex_import_current: undefined;
  codex_import_switcher: undefined;
  codex_refresh_account: { id: string };
  codex_refresh_all: undefined;
  codex_switch_account: { id: string };
  codex_delete_account: { id: string };
}
export type CodexCommand = keyof CodexCommandArgs;
export interface CodexBridge {
  call<C extends CodexCommand>(
    command: C,
    args?: CodexCommandArgs[C],
  ): Promise<unknown>;
  subscribe(callback: (snapshot: unknown) => void): Promise<() => void>;
}
const commands: readonly CodexCommand[] = [
  'get_codex_snapshot',
  'codex_discover',
  'codex_begin_login',
  'codex_poll_login',
  'codex_cancel_login',
  'codex_import_current',
  'codex_import_switcher',
  'codex_refresh_account',
  'codex_refresh_all',
  'codex_switch_account',
  'codex_delete_account',
];
export const codexNativeBridge: CodexBridge = {
  call(command, args) {
    if (!commands.includes(command))
      return Promise.reject(new Error('providerUnavailable'));
    return invoke(command, args);
  },
  subscribe(callback) {
    return listen('codex_snapshot_changed', (event) => callback(event.payload));
  },
};
