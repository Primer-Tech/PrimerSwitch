export const en = {
  // Shell, shared by both providers.
  navigation: 'Navigation',
  accounts: 'Accounts',
  settings: 'Settings',
  openSettings: 'Open settings',
  localAccounts: 'Local account manager',
  providers: 'Account providers',
  claudeSubtitle: 'Track usage and switch Claude Code accounts.',
  codexSubtitle:
    'Track usage and switch Codex accounts — open terminals reconnect on their own.',
  updatedAge: 'Updated {age}',
  noReadings: 'No readings yet',
  refreshAll: 'Refresh all',
  refreshing: 'Refreshing…',
  preview: 'Preview',
  demoNotice: 'Demo data · actions are disabled.',
  dismissMessage: 'Dismiss message',
  close: 'Close',
  cancel: 'Cancel',
  accountInsights: 'Account insights',

  // Notices.
  switchingAccount: 'Switching account…',
  updatingAccounts: 'Updating accounts…',
  allLimited: 'Every account is at or near its limit.',
  allLimitedUntil:
    'Every account is at or near its limit. {name} frees up first, in {duration}.',
  signInNeeded: 'Sign in again to keep using {names}.',

  // Active account card.
  activeAccount: 'Active account',
  active: 'Active',
  refresh: 'Refresh',
  refreshLabel: 'Refresh usage for {name}',
  defaultModel: 'Default model',
  fiveHourWindow: '5-hour window',
  weekly: 'Weekly',
  resetIn: 'Resets in {duration}',
  resetUnknown: 'Reset time unknown',
  resetting: 'Resetting',
  usingCredits: 'Using credits',
  credits: 'Credits',
  viewDetails: 'Details',
  cachedAge: 'Saved reading · {age}',
  automationWaiting: 'Automation is waiting for a current reading.',
  alertThreshold: 'Reached the {threshold}% switch threshold',
  switchToNow: 'Switch to {name} now',
  noActive: 'No active account',
  noActiveClaude:
    'Sign in with Claude or import the login Claude Code uses now.',
  noActiveCodex: 'Sign in with ChatGPT or import the login Codex uses now.',
  loadingAccounts: 'Loading accounts…',
  loadingState: 'Preparing the latest account state.',
  switchingUnavailable: 'Account switching is unavailable',

  // Saved accounts.
  savedAccounts: 'Saved accounts',
  addAccount: 'Add account',
  addActions: 'Add or import an account',
  claudeAddSignIn: 'Sign in with Claude…',
  claudeImportCurrent: 'Import current Claude Code login',
  claudeImportArchive: 'Import from ClaudeSwitch…',
  codexAddSignIn: 'Sign in with ChatGPT…',
  codexImportCurrent: 'Import current Codex login',
  codexImportSwitcher: 'Import from Codex Switcher',
  importing: 'Importing…',
  emptyTitle: 'No saved accounts yet',
  emptyClaude:
    'Sign in with Claude or import the login Claude Code uses now. Then switch between them in one click.',
  emptyCodex:
    'Sign in with ChatGPT or import the login Codex uses now. Then switch between them in one click.',
  account: 'Account',
  status: 'Status',
  actions: 'Actions',
  next: 'Next',
  statusReady: 'Ready',
  statusLimited: 'Limit reached',
  statusNearLimit: 'Near limit',
  statusChecking: 'Checking…',
  statusUnread: 'Not checked yet',
  statusFailed: 'Check failed',
  statusFailedDetail: 'Refresh to try again',
  statusApiKey: 'API key',
  statusApiKeyDetail: 'No ChatGPT plan limits',
  statusUnsupported: 'Unsupported sign-in',
  statusCheckFailed: 'Last check failed',
  statusCantSwitch: 'Can’t switch to this account',
  statusFreeAgain: 'Likely free again · refresh to confirm',
  statusLimitFreesIn: '{limit} · frees in {duration}',
  statusLimitResetsIn: '{limit} · resets in {duration}',
  statusResetsAvailable: 'Resets available: {count}',
  statusResetCooldown: 'Resets usable in {duration}',
  freesIn: 'Frees in {duration}',
  switch: 'Switch',
  switching: 'Switching…',
  signInAgain: 'Sign in again',
  moreLabel: 'More actions for {name}',
  delete: 'Delete',
  switchBusy: 'Wait for the current action to finish.',
  demoOnly: 'The preview is read-only.',
  switchChecking: 'Checking usage…',
  switchUnverified:
    'This account could not be checked yet. Refresh it to try again.',

  // Usage order.
  usageOrder: 'Next in order',
  orderEmpty: 'No other account has usage left below the switch threshold.',
  orderAfterFirstRead: 'Shown after the first reading.',

  // Next up.
  nextUp: 'Next up',
  nextWhySoonest: 'Its weekly limit resets soonest.',
  nextWhyMostLeft: 'It has the most weekly usage left.',
  nextAutomatic: 'Switches automatically at {threshold}%.',
  switchNow: 'Switch now',
  switchNowLabel: 'Switch now to {name}',
  noNext: 'No other account is ready to use.',

  // Automation.
  automation: 'Automation',
  manageAutomation: 'Manage automation',
  autoSwitchOn: 'Automatic switching on',
  autoSwitchOff: 'Automatic switching off',
  resetOrder: 'Order',
  resetOrderSoonest: 'Soonest reset first',
  resetOrderMostLeft: 'Most left first',
  on: 'On',
  off: 'Off',
  codexEnvDaemon: 'Open terminals reconnect automatically.',
  codexEnvNoDaemon:
    'No Codex terminal is open right now; a switch applies to the next session.',
  codexEnvApps:
    'The Codex app and VS Code keep their account until you restart them.',
  codexEnvOtherClients:
    'The Codex app or VS Code is open and keeps its account until restarted.',

  // Provider cards.
  creditsRenewal: 'Credits & renewal',
  availableResets: 'Available resets',
  expiresIn: 'Expires in {duration}',
  resetCooldown: 'Resets can be used again in {duration}',
  checkingReset: 'Checking the reset result…',
  estimatedRenewal: 'Estimated renewal',
  manualRenewalDescription: 'A user-entered estimate, not a billing date.',
  codexSetupTitle: 'Codex setup',
  codexCheckSetup: 'Check setup',
  codexSetupChecking: 'Checking your Codex setup…',
  codexSetupReady: 'Codex {version} · ready',
  codexSetupReadyUnknown: 'Codex · ready',
  codexSetupNotInstalled: 'Codex isn’t installed',
  codexSetupAttention: 'Codex {version} · needs attention',
  codexSetupAttentionUnknown: 'Codex · needs attention',
  codexHowSaves:
    'Switching saves the new sign-in and restarts Codex in the background.',
  codexHowResumes: 'Running turns finish first; nothing is closed.',

  // Account details.
  detailsSignIn: 'Sign-in',
  detailsWorkspace: 'Workspace',
  detailsChecked: 'Last checked',
  subscription: 'Subscription',
  subscriptionInactive: 'Inactive',
  subscriptionPaused: 'Paused',
  subscriptionCanceled: 'Canceled',
  subscriptionTrial: 'Trial',
  unknownStatus: 'Unknown',
  unknown: 'Unknown',
  unknownCount: 'Unknown',
  renewalLabel: 'Renewal day for {name}',
  renewalDay: 'Day {day}',
  manualEstimate: 'Manual estimate · {date}',
  primedLabel: 'Weekly window started',
  fiveHourPrimedLabel: '5-hour window started',
  scopedStale: 'Model readings are out of date.',
  lastReadingKept: 'The last reading is retained.',
  identityVerified: 'Verified: the saved sign-in belongs to this account.',
  identityChecking: 'Checking the saved sign-in…',
  identityUnverified: 'Not verified yet. Refresh to check it.',
  identitySignIn: 'The saved sign-in no longer works. Sign in again to use it.',
  identityApiKey: 'Uses an API key, so it has no plan limits.',
  identityUnsupported: 'Uses a sign-in PrimerSwitch can’t switch.',
  codexAuthChatGPT: 'ChatGPT account',
  codexAuthApiKey: 'API key',
  codexAuthUnsupported: 'Unsupported sign-in',
  deleteAccount: 'Delete account',
  deleteLabel: 'Delete account {name}',
  deleteActiveReason: 'Switch to another account before removing this one.',

  // Delete confirmation.
  deleteTitle: 'Delete saved account?',
  deleteClaude:
    '{name} will be removed from PrimerSwitch. The current Claude Code login stays available.',
  deleteCodex:
    '{name} will be removed from PrimerSwitch. This doesn’t sign you out of ChatGPT or change the account Codex uses now.',
  deleting: 'Deleting…',

  // Claude sign-in and import.
  claudeLoginTitle: 'Sign in with Claude',
  loginDescription:
    'Continue signing in in your browser. After authorization, copy the displayed code and paste it here.',
  loginStepOne: 'Sign in in the browser that opened.',
  loginStepTwo: 'Copy the entire code, including',
  loginStepThree: 'Confirm adding the account.',
  addingAccount: 'Adding account…',
  openingBrowser: 'Opening browser…',
  authorizationCode: 'Authorization code',
  loginNoActivate: 'Adding an account does not make it active automatically.',
  importTitle: 'Import from ClaudeSwitch',
  importAccounts: 'Import accounts',

  // Settings.
  automaticSwitch: 'Automatic switching',
  autoSwitchHelp: 'Switch accounts when usage reaches your chosen threshold.',
  preferSoonestReset: 'Use the account whose weekly limit resets soonest first',
  preferSoonestResetHelp:
    'Spends quota that would otherwise expire unused. Off: prefer the account with the most weekly usage left.',
  primeWindow: 'Start the 5-hour and weekly windows',
  primeHelp:
    "When an account's 5-hour or weekly window ends, send one very short message at the next check to start the next window, so its next reset comes sooner. This also applies to inactive accounts. If both windows end together, one message starts both.",
  autoResets: 'Use resets automatically',
  resetsHelp:
    'When every account is at its limit, use a banked reset. If the reset belongs to another account, PrimerSwitch switches to that account first, even when automatic switching is off. A reset about to expire is used on its own account without switching.',
  pollInterval: 'Check interval',
  minutes: '{count} min',
  pollingHelp:
    "At each check PrimerSwitch reads the active account's limits: for Claude it sends one very short message, for Codex none. Checks run more often near the limit.",
  switchThreshold: 'Switch threshold',
  appearance: 'Appearance',
  system: 'System',
  light: 'Light',
  dark: 'Dark',
  language: 'Language',
  save: 'Save',
  saving: 'Saving…',
  invalidSettings:
    'Choose an interval between 2 and 15 minutes and a threshold between 50% and 100%.',
  globalPreferences: 'Appearance and language',
  settingsProviderScope:
    'Automatic switching, the weekly reset order, the check interval and the switch threshold apply to Claude and Codex. Starting the 5-hour and weekly windows and using resets automatically apply to Claude only. Codex resets can be used manually in its account details. Appearance and language apply to both.',
  sharedAutomation: 'Automation for Claude and Codex',
  claudeOnlyAutomation: 'Claude only',

  // Formatting.
  noReading: 'No reading yet',
  secondsAgo: 'a few seconds ago',
  usageMeter: '{value} percent used',
  dataUnavailable: 'Data unavailable',

  // Claude runtime errors and notices: catalog keys that native replies match exactly.
  actionUnavailable: 'This action is unavailable.',
  failedAction: 'The action failed. Saved data has been retained.',
  unsafeData: 'The received data cannot be displayed securely.',
  fullCodeRequired: 'Paste the complete code, including the part after #.',
  previewDevOnly: 'Preview is available only in development.',
  storageError:
    'Local data could not be read or saved. Original files are retained.',
  vaultUnavailable:
    'The system vault is locked, unavailable or unsupported. Saved accounts have not been changed.',
  unsupportedContext:
    'This Claude context uses an authentication method or policy that does not support account switching.',
  unsupportedEnvironment:
    'Account switching is unavailable while the environment variable {name} is set. Remove it, then restart PrimerSwitch.',
  unsupportedSetting:
    "Account switching is unavailable while Claude Code's settings.json contains {name}. Remove that entry, then restart PrimerSwitch.",
  vaultIntegrity:
    'Encrypted data could not be authenticated, or its key is missing. Existing files are retained.',
  missingAccount: 'The account is no longer available.',
  externalChange: 'The Claude login changed. Refresh before continuing.',
  identityError: 'The account identity could not be verified.',
  providerError: 'Claude did not provide a fresh reading. Try again later.',
  signInRequired:
    "Claude no longer accepts this account's saved sign-in. Sign in again to keep using it.",
  activeSessionExpired:
    "Claude Code's session for this account has expired. Use Claude Code once to renew it, or sign in again there.",
  settingsError: 'The settings are not valid.',
  readOnlyError: 'This view is read-only.',
  loginExpired: 'The login expired or was canceled.',
  importExpired: 'The import is no longer valid. Select the folder again.',
  pendingResetError:
    'The previous reset request must be confirmed before another attempt.',
  cliVersionError:
    'The Claude Code version could not be detected. Install a recognized version.',
  browserError: 'Could not open the browser. Try signing in again.',
  loginCodeError: 'The sign-in code is invalid.',
  folderError: 'Could not select the folder.',
  folderNotLocal: 'The selected folder is not local.',
  switchInterrupted:
    'A previous account switch was interrupted. The current Claude Code login was left unchanged; switch again if needed.',
  switchRecoveryPending:
    'A previous account switch could not be completed yet. PrimerSwitch finishes it before the next switch.',
  outgoingUnverified:
    "Claude Code's sign-in for this account had expired, so it could not be verified while switching away. It was kept and is checked on the next update.",
  accountError: '{name}: {reason}',
  autoSwitchFailed: 'Could not switch to {name} automatically. {reason}',

  // Codex.
  codexActionFailed: 'That didn’t work. Your saved accounts are unchanged.',
  codexBusy: 'Codex is finishing another account action…',
  codexNoQuota:
    'Usage hasn’t been checked yet. Refresh to read the latest limits.',
  codexApiQuota:
    'This account uses an API key, so it has no ChatGPT plan limits.',
  codexNoWindows: 'No usage limits were reported for this account.',
  codexUsageBlocked: 'Included usage is blocked for this account.',
  codexSpendBlocked: 'The workspace spending limit has been reached.',
  codexCreditsUnlimited: 'Unlimited',
  codexCreditsAvailable: 'Available',
  codexResetCount: '{count} resets available',
  codexResetUnknown: 'Reset availability unknown',
  codexResetExpiryDates: 'Reset expiry dates',
  codexResetOne: '1 reset',
  codexResetMany: '{count} resets',
  codexResetExpires: 'Expires',
  codexResetNoExpiry: 'Does not expire',
  codexResetExpiryUnknown: 'Expiry dates unavailable.',
  codexResetExpiryPartial:
    'Expiry details available for {shown} of {count} resets.',
  codexUseReset: 'Use one reset',
  codexRetryReset: 'Check previous reset',
  codexUsingReset: 'Resetting usage…',
  codexRefreshResets: 'Refresh resets',
  codexResetHelp:
    'Uses one available reset on this account to reopen eligible usage windows.',
  codexResetPending:
    'The previous reset is unconfirmed. Check it again to recover the result without using another reset.',
  codexResetDone: 'Reset applied.',
  codexResetCached: 'Saved reading. Refresh to check the current availability.',
  codexResetNotLimited:
    'There is no eligible usage window to reset. No reset was used.',
  codexOtherLimit: 'Other limit',
  codexPlanFree: 'Free',
  codexWindowShort: 'Short window',
  codexWindowLong: 'Long window',
  codexWindowDaily: 'Daily',
  codexSwitchingTo: 'Switching to {name}',
  codexStageSaving: 'Saving the new sign-in…',
  codexStageRestarting:
    'Restarting Codex in the background. Open terminals reconnect on their own; running turns finish first (up to a minute).',
  codexStageVerifying: 'Checking the new account…',
  codexStepSave: 'Save',
  codexStepRestart: 'Restart',
  codexStepVerify: 'Check',
  codexSwitchedLive:
    'Codex now uses {name}. Open terminals reconnect automatically.',
  codexSwitchedNext:
    'Codex now uses {name}. It applies to the next Codex session you start.',
  codexSwitchedOtherClients:
    'The Codex app and VS Code keep the previous account until you restart them.',
  codexSwitcherTitle: 'Codex Switcher is running.',
  codexSwitcherText:
    'It rewrites the same sign-in and force-closes Codex terminals when it switches. Close it to avoid conflicts.',
  codexSwitcherImport: 'Import its accounts',
  codexWarnings: 'Setup warnings',
  codexLoginComplete:
    'Account added. Codex keeps using the current account until you switch.',
  codexSignedInAgain: 'You’re signed in again as {name}.',
  codexDeleteComplete: 'The saved account was deleted from PrimerSwitch.',
  codexImportedCurrent: 'The current Codex sign-in is now saved.',
  codexImportedCurrentExisting:
    'The current Codex sign-in was already saved, so it was updated.',
  codexImportedSwitcherOne: 'Imported {count} account from Codex Switcher.',
  codexImportedSwitcherFew: 'Imported {count} accounts from Codex Switcher.',
  codexImportedSwitcherOther: 'Imported {count} accounts from Codex Switcher.',
  codexImportedSwitcherNone:
    'No new accounts in Codex Switcher; everything was already saved.',
  codexLoginTitle: 'Sign in with ChatGPT',
  codexLoginAgainTitle: 'Sign in again',
  codexLoginDescription:
    'Finish signing in in your browser. This window updates on its own when you’re done, so there’s no code to paste.',
  codexLoginAgainDescription:
    'Sign in as {name} in your browser. This window updates on its own when you’re done, so there’s no code to paste.',
  codexLoginKeepsCurrent:
    'Adding an account doesn’t change which account Codex uses.',
  codexLoginOpening: 'Opening your browser…',
  codexLoginWaiting: 'Waiting for you to finish in the browser…',
  codexLoginVerifying: 'Checking the account…',
  codexReason_notInstalled:
    'Codex isn’t installed on this computer. Install it, then check setup again.',
  codexReason_unsupportedVersion:
    'This Codex version isn’t supported yet, so account changes are turned off.',
  codexReason_unsupportedStore:
    'Codex stores its sign-in in a place PrimerSwitch can’t use yet. Nothing was changed.',
  codexReason_unsupportedAuth:
    'This account uses an API key or another sign-in that can’t be switched.',
  codexReason_policyRestricted:
    'Your organization’s Codex policy doesn’t allow account changes.',
  codexReason_externalCredentials:
    'Codex is set to use credentials from elsewhere, such as an environment variable, which can override the account you pick here.',
  codexReason_storeConflict:
    'Codex has conflicting saved sign-ins. Sign in to Codex again, then check setup.',
  codexReason_vaultUnavailable:
    'Secure storage on this computer is locked or unavailable. Your saved accounts are unchanged.',
  codexReason_identityUnverified:
    'This account hasn’t been confirmed yet. Refresh it or sign in again.',
  codexReason_identityMismatch:
    'ChatGPT returned a different account than expected. Nothing was changed.',
  codexReason_externalChange:
    'The Codex sign-in was changed outside PrimerSwitch. Check setup, then try again.',
  codexReason_loginExpired: 'The sign-in took too long and expired. Try again.',
  codexReason_loginCanceled: 'The sign-in was canceled.',
  codexReason_providerUnavailable:
    'Codex didn’t respond as expected. Try again in a moment.',
  codexReason_busy:
    'Another account action is still running. Try again in a moment.',
  codexReason_daemonRestartFailed:
    'The new sign-in is saved, but Codex couldn’t restart in the background. Open terminals keep the previous account until you restart them.',
  codexReason_signInRequired:
    'This saved sign-in is no longer valid. Sign in again to use this account.',
  codexReason_switchInProgress:
    'Another switch is already running. Wait for it to finish.',
  codexReason_switcherUnavailable:
    'Codex Switcher’s accounts weren’t found or couldn’t be read.',
  codexReason_activeAccount:
    'Switch to another account before removing this one.',
  codexReason_resetUnavailable:
    'Refresh to check reset availability. Codex may not provide it for this account or workspace.',
  codexReason_noResetCredits: 'No resets are available for this account.',
  codexReason_resetUnconfirmed:
    'The reset result could not be confirmed. Check the previous reset to recover its result.',
} as const;
