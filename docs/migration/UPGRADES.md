# Updating without resetting your data

PrimerSwitch 0.1.1 and the 0.2.0 Codex update keep the application and storage identity of the 0.1.0 preview. The Primer logo, colors and simplified navigation change the interface; existing saved accounts and settings use the same encrypted record.

## Installing the branded update

Close PrimerSwitch, then run the new Windows installer under the same Windows user and keep the existing installation folder. You can install over the current version. You do not need to uninstall first, export accounts or sign in again solely because of the branding update.

On macOS, close the app and replace PrimerSwitch.app with the new app from the disk image. Saved data remains outside the app bundle. On Linux, install the newer package through the package manager under the same desktop user and keep the same XDG data location and persistent Secret Service keyring.

## Preserved state

The historical-format regression covers saved accounts and opaque credential extensions; language, appearance, polling, threshold and automation settings; quota observations, renewal dates and reset history; pending reset request identities, cooldowns, priming state and refresh history. Startup retains the existing encrypted record bytes. Account ownership is reverified after restarting, as in the previous release.

The stable locations are:

| Platform | Application data | Master key |
| --- | --- | --- |
| Windows | %LOCALAPPDATA%\PrimerSwitch | master-key.dpapi, protected for the same Windows user |
| macOS | ~/Library/Application Support/PrimerSwitch | Existing application-owned Keychain identity |
| Linux | $XDG_DATA_HOME/primerswitch, or ~/.local/share/primerswitch | Existing persistent Secret Service item bound to the canonical data directory |

The Windows default installation folder also contains the saved vault. The generated installer is checked to delete only reviewed application/resource files, preserve vault/key files and avoid recursive removal of this folder. Version 0.1.1 is recognized as an upgrade from 0.1.0; downgrades are disabled.

## Verification

The runtime regression opens a literal synthetic 0.1.0 saved-state document with injected keys and fake transports, checks startup leaves the original ciphertext/key bytes unchanged, changes one setting and reopens the same state. All remaining account and scheduling fields are retained, and no provider request occurs.

Installer identity and cleanup checks are separate from runtime compatibility. Native Windows installer upgrade/reinstall and Linux installed-demo results are recorded in [the evidence ledger](PROGRESS.md). The original ClaudeSwitch import remains a separate operation described in [the migration plan](IMPLEMENTATION_PLAN.md).

## Codex 0.2.0 update

Codex uses the new encrypted codex-state record with the existing master key, vault envelope and application directory. Enabling it does not rewrite runtime-state or change Claude settings. A damaged Codex envelope blocks only Codex while preserving that file and keeping Claude settings usable. Codex records, pending selection journals and unrelated opaque files are included in the new installer retention fixtures.

The 0.1.1-to-0.2.0 hosted installer gate is pinned to the actual published 0.1.1 installer bytes and checks upgrade followed by same-version reinstall. Its completed native evidence will be linked in PROGRESS.md before publishing 0.2.0. The historical 0.1.0 runtime-format fixture remains active.
