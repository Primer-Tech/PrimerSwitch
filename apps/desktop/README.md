# PrimerSwitch desktop frontend

Svelte 5, TypeScript and Vite. The bundled UI uses only the allowlisted native commands in `src/lib/bridge.ts`; account credentials and provider requests stay in Rust. Strict runtime schemas reject unknown IPC fields, and older revisions never replace newer snapshots. Failed actions retain the cached display.

From this directory, run:

```sh
npm ci
npm run check
npm test
npm run build
npm run format:check
```

`npm run dev` starts Vite on `http://127.0.0.1:1420`. The main entry requires the Tauri runtime. For a standalone visual preview, open `/demo.html` on the development server. This separate entry uses synthetic accounts, blocks mutations and is excluded from the production bundle. No preview action invokes a provider or reads a real account.

The UI subscribes to `snapshot_changed` and falls back to a pure `get_snapshot` read every five seconds. One-second countdowns render locally and never trigger provider requests. Login codes remain transient form input and are cleared when the modal closes. Native dialogs provide keyboard focus trapping and Escape cancellation; light, dark and system themes and responsive layouts use system fonts without remote assets.

Fixture tests cover account workflows, redacted IPC validation, duplicate/busy/demo blocking, revision races, late login cancellation, import previews, deletion confirmation, cached errors, settings, manual renewal and countdown isolation. These tests are frontend evidence only; native packaging, OS notifications and authenticated provider compatibility require their separate qualification gates.

The dashboard follows the approved charcoal/blue direction with an active quota panel, saved-account table and next-account, automation and renewal summaries. Per-account details expose scoped usage, reset outcomes/cooldowns/pending state, priming, renewal controls, refresh, switching and confirmed deletion. Weekly table values use the effective binding window: the higher of overall and selected-model weekly usage; accessible descriptions and tooltips state this rule. Unknown credits remain unknown, and renewal dates are explicitly manual estimates.

English is the initial language; English and Romanian catalogs cover interface text, dialogs, accessible names, fallbacks, plurals, numbers, dates and durations. The language selector persists through native settings. New settings default to dark; existing dark, light or system appearance is preserved. The read-only development preview accepts only bounded `?theme=dark|light&lang=en|ro` display choices. Fixture tests cover keyboard Escape/focus restoration for settings, account details, deletion and the add disclosure.