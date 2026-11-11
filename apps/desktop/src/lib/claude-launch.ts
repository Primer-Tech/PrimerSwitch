// Pure rules and wording for launching a saved Claude account in its own window.
import { t, type Language, type MessageKey } from './i18n';
import type { LaunchNotice } from './controller';
import type { NoticeView } from './provider-view';
import type {
  AccountView,
  LaunchModel,
  ProfileAuthReason,
  Snapshot,
} from './types';

/** Why a launch is unavailable for as long as the account or build stays like this. */
export type LaunchBlock = 'demo' | 'unsupported' | 'signIn' | 'unverified';

/** A saved sign-in that needs repair, as the account list shows it. */
export function needsSignIn(account: AccountView): boolean {
  return (
    !!account.signInRequired ||
    (account.active && account.error === 'activeSessionExpired')
  );
}

/**
 * Whether the renderer offers a launch for `account`. Launching the active account
 * is fine (unlike a switch). The native precheck stays authoritative; this only
 * keeps the button honest.
 */
export function launchBlock(
  snapshot: Pick<Snapshot, 'demo' | 'launchSupported'>,
  account: AccountView,
): LaunchBlock | null {
  if (snapshot.demo) return 'demo';
  if (snapshot.launchSupported !== true) return 'unsupported';
  if (needsSignIn(account)) return 'signIn';
  if (!account.identityVerified) return 'unverified';
  return null;
}

const blockKeys: Record<LaunchBlock, MessageKey> = {
  demo: 'demoOnly',
  unsupported: 'launchUnsupported',
  signIn: 'launchNeedsSignIn',
  unverified: 'launchNeedsVerified',
};
export function launchBlockText(block: LaunchBlock, locale: Language): string {
  return t(locale, blockKeys[block]);
}

export const launchModelLabels: Record<LaunchModel, MessageKey> = {
  default: 'launchModelDefault',
  opus: 'launchModelOpus',
  sonnet: 'launchModelSonnet',
  fable: 'launchModelFable',
};

const reasonKeys: Record<ProfileAuthReason, MessageKey> = {
  firstUse: 'launchAuthFirstUse',
  identityChanged: 'launchAuthIdentityChanged',
  expired: 'launchAuthExpired',
};
export function launchReasonText(
  reason: ProfileAuthReason | null,
  locale: Language,
  name: string,
): string {
  return t(locale, reasonKeys[reason ?? 'firstUse'], { name });
}

/** The notice after a launch that finished; null when there is none. */
export function launchNoticeView(
  notice: LaunchNotice | null,
  accounts: readonly AccountView[],
  locale: Language,
  dismiss: () => void,
): NoticeView | null {
  if (!notice) return null;
  const account = accounts.find(
    (candidate) => candidate.id === notice.accountId,
  );
  return {
    key: 'launch',
    tone: 'success',
    role: 'status',
    text: account
      ? t(locale, 'launchStarted', { name: account.name })
      : t(locale, 'launchStartedAnonymous'),
    dismiss,
  };
}
