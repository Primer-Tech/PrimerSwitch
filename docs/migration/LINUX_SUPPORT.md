# Linux preview support

## Target and packages

The Linux preview targets Ubuntu 24.04 x86_64, with glibc 2.39+, GTK 3, WebKitGTK 4.1, a session D-Bus and an unlocked persistent Secret Service keyring. Native CI runs real GNOME Keyring fixtures in temporary homes and fresh D-Bus sessions. The Debian packaging job also installs the generated package on its ephemeral Ubuntu runner and exercises the installed demo through WebKitGTK accessibility. Consult [the evidence ledger](PROGRESS.md) for completed run links; implementation is not authenticated-provider acceptance.

The .deb declares actual Ubuntu runtime dependencies and a GNOME Keyring/KeePassXC alternative. KeePassXC must have its Secret Service integration enabled and supply an unlocked persistent default collection. A package manager installing a keyring is distinct from starting/unlocking its service in the desktop session.

The .rpm is generated from the same Ubuntu baseline, with SONAME and versioned-symbol requirements derived from its executable and a Secret Service provider-owned service-file requirement. It is not yet installed-qualified on Fedora/openSUSE. ARM64, older distributions and Wayland-specific acceptance remain separate targets. System GTK/WebKit libraries are dependencies, not bundled copies.

## Use

Install a released .deb with:

```sh
sudo apt install ./PrimerSwitch_0.1.1_amd64.deb
primerswitch
```

Use the actual downloaded filename if it differs. The application menu opens PrimerSwitch too. English and dark appearance are the default; Romanian and light/system appearance are available in Settings. Automation preferences live in Settings. Closing the app stops automation. Later package upgrades retain existing settings/accounts; see [updating without a reset](UPGRADES.md).

To inspect the interface without reading credentials or calling providers:

```sh
primerswitch --demo
```

Normal account switching uses the default Claude CLI files. Custom authentication/storage overrides fail closed. Do not infer authenticated Claude contract compatibility solely from passing fixture tests.

## Secure storage

Saved accounts, journals and backups remain AEAD-encrypted, with private directories/files. The vault master key is an application-owned Secret Service item sent through an encrypted DH session. It must belong to the persistent default collection; an ephemeral session collection is rejected. GNOME Keyring normalizes binary-secret MIME metadata to text/plain and adds its Generic schema after restarting; only these documented metadata forms are accepted, while the original 32-byte key and exact application/vault ownership are still required.

Missing/locked services, ambiguous entries, malformed keys and changed/lost keys preserve existing ciphertext and return a safe vault-unavailable/integrity error. There is no plaintext fallback or silent replacement. A durable creation marker protects uncertain key creation. D-Bus methods and the whole key operation are bounded; a rare service-requested creation prompt can appear after unlocked preflight.

The key namespace is bound to the canonical vault directory. Moving/copying data to another path or machine does not migrate its protected key. Unlock the desktop keyring through its normal desktop integration before starting PrimerSwitch.

## Build and verify

Install the [native Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), the pinned Rust toolchain, Node 24 and Python 3.11+, then run:

```sh
npm --prefix apps/desktop ci
python -B scripts/package_preview.py --bundle deb
```

The wrapper verifies locked dependencies, source/license hashes, the actual renderer assets and package content. It bundles dependency/Comfortaa notices and records per-artifact SHA256 and provenance. AppImage is not part of this preview.

```sh
cargo test --locked
python -B scripts/linux_keyring_fixture.py
```

Ordinary fixtures never touch a native keyring. The explicit runner provisions and removes its own temporary keyring/D-Bus environment, including daemon/process restart, ephemeral default rejection and missing-service checks. The installed-demo harness is restricted to ephemeral Ubuntu GitHub runners; it does not install software on a developer's normal host. No test authenticates against user accounts.
