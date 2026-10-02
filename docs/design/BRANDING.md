# Primer brand integration

The project owner requested Primer branding on 2026-10-02. The approved dark dashboard layout remains, with Primer's violet/lavender palette and genuine logo replacing the generic switch arrows.

The unchanged 512px logo was retrieved from [Primer's public icon](https://primer.tech/icons/icon-512x512.png), which the [official website](https://primer.tech/en) identifies as its organization logo. Its SHA256 is 420c32e097db47df368fcc6b12aea5fdfe94a188ad071adf6b303d9694136c16. The desktop source is apps/desktop/src-tauri/icons/primer-source.png; native ICO, ICNS and PNG sizes are generated with the locked Tauri icon command. The renderer uses the same original bytes in apps/desktop/src/assets/primer-logo.png. Brand assets are bundled locally and make no website/font requests at runtime.

The website uses a Comfortaa 600 Primer wordmark, violet #9370DB, lavender #E6E6FA and near-black surfaces. The exact public Primer-letter font subset is used unchanged for the Primer wordmark; Switch uses the normal interface font. Full upstream copyright/OFL and pinned source/hash provenance are in [the brand manifest](../legal/brand/manifest.json) and [font license](../legal/brand/Comfortaa-OFL.txt), included in generated dependency notices and packages. Primer logo/brand ownership remains distinct from the source-code MIT license.

The navigation now has Accounts and Settings. Automation preferences remain in Settings; the dashboard still shows current automation status. Dark and light variants use accessible accent shades; primary controls use a darker violet with white text.
