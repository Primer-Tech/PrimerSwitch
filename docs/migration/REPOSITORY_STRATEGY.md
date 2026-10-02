# Public repository and legacy preservation

The owner approved the name PrimerSwitch, a separate open-source Primer-Tech repository, and implementation on 2026-10-02. The public repository is [Primer-Tech/PrimerSwitch](https://github.com/Primer-Tech/PrimerSwitch).

The Rust, Svelte, scripts and new documentation in this repository are MIT licensed. The original private ClaudeSwitch repository had no detected license; its Swift source, research and Git history remain in the separate private checkout. They are not included in the public commit or re-licensed by this project's LICENSE. Its 19 canonical Git blobs were preserved under its own archive folder, with a private preservation manifest and verifier. The archived app is the retained Mac implementation until the replacement passes native acceptance.

Publication uses a new Git repository, not a remote change on the private checkout. Include reviewed source, Cargo/npm locks, fixture-only tests, documentation, generated design proposals and newly created icons. Exclude credentials, user vaults, local build state, private research, historical account examples and the old repository's objects/remotes. The public repository validator checks these defined boundaries and local documentation links; it is not a comprehensive detector for every possible secret.

CI has a Windows/macOS/Linux build and fixture matrix without provider accounts. A manually invoked packaging workflow creates unsigned Windows/Mac preview artifacts. Successful compilation does not establish native Keychain behavior, installed notifications, existing-session login adoption or authenticated endpoint compatibility. Those gates remain visible in the progress ledger. Signed/notarized stable releases follow platform qualification.

The final local layout is two sibling checkouts under the owner's Primer workspace: ClaudeSwitch for the private preserved baseline and PrimerSwitch for this public project. Codex and later providers join PrimerSwitch through separate versioned contracts. Neither provider's auth fields or policy should be forced into the other's schema.
