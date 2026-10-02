# Migration verification and acceptance plan

Status: test design only. See PROGRESS.md for actual commands and results; this document defines the acceptance inventory. Current preservation/documentation checks are recorded in [PROGRESS.md](PROGRESS.md). Use [BEHAVIOR_SPEC.md](BEHAVIOR_SPEC.md) requirement IDs and the [implementation packages](IMPLEMENTATION_PLAN.md) to keep assertions traceable.

## 1. Test boundaries and evidence labels

Ordinary tests use injected clocks, fake provider transport, temporary fixture homes, dummy credential stores and fake notifications/browser launches. They must not discover the actual user home, use default `CODEX_HOME`/Claude paths, open the real Keychain item, import real credentials, or send authenticated network requests. A sentinel transport rejects every unregistered URL. Use `example.test` identities and unmistakably fake token strings; do not copy the old research reports' emails or responses into fixtures.

Evidence levels are distinct:

| Label | Proves | Does not prove |
| --- | --- | --- |
| Source inspection | Baseline rules and documented contracts | Actual OS/session behavior |
| Unit/contract fixture | Pure decisions and exact mock request/response handling | Server entitlement or endpoint availability |
| Compile/package | Target builds and package is produced | Native credential access, notifications or quota reads |
| Native fixture smoke | Actual OS APIs in a dedicated test context | Real account/session adoption |
| Live compatibility | Selected CLI/account contract on the named OS/version | Other versions, accounts, OSes or seamless in-flight continuation |

Live validation is a later separate test scope. A reset/inference call is a real side effect, so plan fixture coverage first and record any live usage explicitly. This planning task did not switch accounts, consume resets, import real credentials or make authenticated API calls. Do not run the original probes on the real home: even `--probe-switch-logic` writes a backup.

## 2. Golden fixtures and model/policy tests

Keep compact sanitized fixtures with explicit provenance: source symbol + commit or primary protocol version. Derive expected outputs from the specification, not from a ported function's output. The Swift probe contains useful examples, but its debug guards and home mutation must not enter production code.

| Test ID | Scenarios / expected result | Requirements / package |
| --- | --- | --- |
| P01 — Account schema | Minimal old record; full new record; absent/null optional fields; unknown OAuth/config fields retained; malformed credential record reported and preserved; ISO date vs epoch seconds vs token milliseconds | B01, D01; M01/M04 |
| P02 — Main/scoped quotas | `weekly_all` override; matching scoped model is the maximum; exhausted model does not block another model; missing model; case/prefix/substring matching; unknown labels retained; scoped rows sorted; malformed optional reset block retains usage | B10/B11/B16; M01/M03 |
| P03 — Percent precision | Header fractions `0.29`, `0.57`, `0.95`, `1.0`; round to 29/57/95/100. Test threshold equality and reject nonfinite/invalid values without manufacturing a good reading | B10/B13; M01/M03 |
| P04 — Candidate boundaries | At threshold, neither usable nor headroom. At `T−5`, preferred if strictly below T. At T=100 and margin=0, 100 remains unusable. No data excluded. Active excluded from target but included in consumption plan | B13; M01 |
| P05 — Spend ordering | Prefer 94/94 with later reset over 99/20 with earlier reset at T=100. If no preferred headroom, an account below T still qualifies. Same UTC hour chooses lower five-hour usage; different hours choose earlier reset; missing reset last; stable final tie-breaker | B13; M01 |
| P06 — Auto-switch outcome | Active below threshold stays; either binding window reaches T triggers selection; no active returns no action; no current reading suspends; no candidate yields exhausted/earliest main reset and 30-minute notification throttle | B12/B14, D08; M01/M05 |
| P07 — Reset availability | Remaining count, paused, future start, exact expiry, cooldown and explicit ineligible filter; server `next_grant_id` respected even if another expires earlier; malformed grant skipped; omitted `usable_now` false and requires-limit true | B16; M01/M03 |
| P08 — Reset last resort | Another usable account blocks last-resort use; active grant preferred; inactive grant means switch then claim even with ordinary auto-switch off; all blocking windows must be cleared; threshold 95 does not satisfy a required real 100; model-only binding needs appropriate coverage | B16/B17; M01/M05 |
| P09 — Expiry guard | Exactly 3-hour boundary; expired grant excluded; inactive claim does not switch; usable-now required; true-limit requirement; one action/account; last-resort selection excluded from that cycle's expiry loop | B18; M01/M05 |
| P10 — Policy distinctions | Last resort's source behavior does not add `usable_now`; seven-day-overage coverage; simultaneous model/account-wide assumption captured; disable each source guard in a test double to show the regression fixture detects its absence | B16–B18; M01 |
| P11 — Reset verdicts | All named outcomes; unknown named result; 401/403; throttling 429; non-2xx parseable verdict; unreadable/5xx ambiguity; success decrement/remove/next-ID/cooldown; already-used removes grant; no false success | B19; M01/M03 |
| P12 — Request identity | Same-grant pending ID reused on lost reply/restart; 5-minute retry floor; 30-minute + newer-status gate after final refusal; unresolved >24-hour claim reconciled before another key; grant IDs/organization/request paths reject injection | B19, D12; M01/M05 |
| P13 — Renewal and dates | Unknown day; 1/28/29/30/31 across leap and short months; local DST boundaries; strictly future time; absent subscription start uses documented stable fallback; manual day never inferred from plan creation | B03; M01 |

Test negative/unknown inputs where they affect a real decision. Avoid enumerating dozens of decorative UI strings as separate unit tests or writing tests that simply repeat implementation branches.

## 3. Claude request and response contracts

| Test ID | Meaningful checks | Requirements / package |
| --- | --- | --- |
| H01 — PKCE and login | Correct random length/base64url/S256, authorize fields/scopes, code exchange request, pasted whitespace handling, matching state, expired/canceled/replaced login rejected, account added without switching | B04, D03; M03 |
| H02 — Refresh lifecycle | Expiry milliseconds and 120-second margin; optional returned refresh token preserves old token; scopes update; failure cooldown; durable rotation occurs before next request; one forced inactive unauthorized retry | B09, D10/D11; M03 |
| H03 — Active ownership | No active refresh request ever sent; live token unchanged vs changed; profile proves owner; owner unreadable/mismatched fails without poisoning saved account; external identity changes during await invalidate result | B08, D04/D09; M03/M05 |
| H04 — Metadata | URL/query/beta/user-agent/timeouts; fractional/plain ISO resets; optional rows/extra data; missing required window fails; 429 numeric and HTTP-date retry handling; reset status parsing cannot invalidate unrelated usage | B11/B16, D11; M03 |
| H05 — Tiny inference | Exact endpoint/model/body/system text/max tokens/headers; successful headers vs missing headers; mixed-case names; tokens captured only as numeric counts; no extra completion for a known valid inference snapshot | B10/B15; M03 |
| H06 — Budget vs throttle | Rejected five-hour/seven-day claim clamps that window to 100; main utilization >=100 proves exhaustion through the unified-header path; missing other window retains old observation/unknown quality; generic 429 stays throttle; model-only claim without main evidence stays error | B10/B12; M03 |
| H07 — Enrichment isolation | Fresh scoped metadata carried; stale `weekly_all` cannot reduce live usage; live weekly refusal remains >=100 despite enrichment; repeated failed enrichment respects attempt backoff; successful inference does not clear metadata cooldown | B11, D06; M03/M05 |
| H08 — Fast tick | Success and failure paths both make zero usage-metadata enrichment/fallback requests on a strict fast tick. Required mutation preflight is coalesced into ordinary work, not one endpoint call per fast tick | B14, D07; M03/M05 |
| H09 — Reset claim | Exact program/grant/request/organization payload; pending ID durably saved before sending; invalid path/ID no request; network/429/lost reply pending; final result updates state; source 5xx verdict handling vs intentional conservative deviation explicit | B19, D12; M03/M05 |
| H10 — Network/log boundary | No credentials forwarded on redirect; origin allowlist, certificate failure, bounded response, timeout and proxy error; sentinel auth strings never appear in logs, Debug output, notification details, IPC or diagnostic export | D13; M03/M06 |

Capture request counts as well as values: rate-limit avoidance is part of the feature. Simulated time advances must not turn every startup/near-limit tick into another enrichment/profile call. Identity verification calls are counted separately from quota reads.

## 4. Filesystem, vault and recoverable-switch tests

Run portable failure tests against fake platform APIs. Repeat platform semantics with native fixture stores before accepting that OS.

| Test ID | Setup / invariant | Requirements / package |
| --- | --- | --- |
| S01 — Path context | Default home; Unicode/spaces; custom config directory; secure-storage override unset/empty/nonempty; separate contexts never share cached identity or locks; unresolved override is unsupported rather than guessed | B06, D04; M00/M02 |
| S02 — Preserve unrelated data | Fixtures contain MCP/auth extensions/preferences in both files; only target auth/identity patches change. Invalid JSON is preserved with an error, not replaced by an empty object | B06/B07; M02/M04 |
| S03 — Vault protection | Round trip through fake and native DPAPI/Keychain/Secret Service; nonce uniqueness; ciphertext/AAD tamper; wrong user/key/schema; locked/missing store; existing ciphertext never overwritten by a newly generated key | B01, D02; M02 |
| S04 — Private files | Account IDs cannot traverse paths; temporary files/journals/backups are private before secret writes; atomic replacement retains restrictive ACL/mode; no credential remnants in logs or unprotected temp directory | D01/D02/D04/D13; M02/M04 |
| S05 — Transaction failures | Fault before/after journal prepare, auth write, identity write, verification, commit and cleanup. Restart yields verified target, verified rollback or explicit conflict; never a reported successful mixed identity/token state | B07, D04; M04 |
| S06 — External writer | CLI rotates outgoing token while validation awaits; external login changes one or both stores midwrite/recovery; rollback does not clobber external changes; latest proven outgoing tokens retained | B07/B08, D04/D10; M04/M05 |
| S07 — File contention | Windows open handle/replace failure, existing/missing destination, disk full/access denied, lock already held, process exit; temp cleanup is scoped; destination never unconditionally deleted to make rename succeed | D04; M02/M04 |
| S08 — Keychain semantics | Exact Mac item selector/config hash/fallback evidence; update existing item without unwanted extra item/ACL change; refusal/locked keychain handled; fixture item cleaned without touching real Claude item | B06, D04; M02/M07 |
| S09 — Migration round trip | Every B01 field imported; old/new optional keys; saved ISO dates and nested cached resets; duplicate email/different UUID; identical record rerun; corrupted record reported; preview token invalidated by changed source | B01/B03/B05, D01/D05; M04/M07 |
| S10 — Import after rotation | New vault token changed since first import; resumed import retains it, manual renewal and pending claim; no duplicate identity; source directory/preferences byte-identical | B05/B19, D05/D10/D12; M04/M07 |

Use a separate fake JSON credential file even on a Mac for portable tests. A mocked Keychain result is not S08 native evidence. Secret Service compilation does not prove a running Linux service is available. No test may delete the actual user data directory or resolve a cleanup target outside its test root.

## 5. Scheduler, recovery and concurrent actions

| Test ID | Event sequence / expected outcome | Requirements / package |
| --- | --- | --- |
| R01 — Cadence | Fresh inactive cache at startup skips requests until 900 seconds; never-read accounts due; full-cycle 8-second spacing; ordinary interval min120; near band >=T−3 uses30; exhausted state returns ordinary cadence | B02/B14; M05 |
| R02 — Endpoint budgets | Failure→429→success on metadata with interleaved successful inference; cooldown survives restart; fast ticks never bypass it; manual refresh coalesces; unauthorized retry records throttling correctly | D06/D07/D11; M05 |
| R03 — Freshness | Old active reading shown after failure but no switch/reset; candidate older than preflight limit is revalidated; stale relevant model row suspends decision; fresh absent row is valid; reset eligibility checked before claim | B12/B14/B19, D08; M05 |
| R04 — Priming | First reset just remembered; crossing triggers once per account; inactive account included without switching; restart/toggle-off marker retained; success uses headers; failure retries; ambiguous/crashed request reconciles before duplicate | B15, D12; M05 |
| R05 — Reset ordering | Last-resort inactive refresh→durable ID→switch→claim; failed switch sends no claim; expiry claim inactive does not switch; successful active reset invalidates usage and suppresses same-cycle auto-switch; switched action list discarded | B17–B19; M05 |
| R06 — Pending reset recovery | Terminate after journal but before send, after send/lost response, after verdict but before cleanup; same logical key used; no new claim for ambiguous old result; disappeared offer reconciled; per-account automation paused if unresolved | B19, D12; M05 |
| R07 — Concurrent user work | Timer + refresh + switch + delete + import while responses wait; stable IDs/generations prevent wrong-card update, resurrection, double refresh or overlapping writes; rotated credentials follow deliberate preservation rule | D09/D10; M05 |
| R08 — Suspend/time changes | Sleep/resume produces one due recomputation; wall clock back/forward/future persisted timestamps bounded; monotonic cooldown scheduling; no missed-tick burst or immediate notification storm | B02/B14; M05 |
| R09 — Identity/profile | Profile owner mismatch makes account unverified for decisions, even if usage parse succeeded; stale subscription label retained separately; failed daily profile does not reset usable quotas to zero | B03/B08/B12, D08; M05 |

Tests drive channels/clock without multi-minute real sleeps. A single runtime owner should make these deterministic. Check command completion and visible errors, not merely absence of a panic.

## 6. UI, IPC and native acceptance

Use renderer tests with sanitized snapshots and at least one fixture-driven desktop E2E path. Do not expose test control commands in release builds. Current Tauri options include an embedded automation service on all three OSes and native `tauri-driver` on Windows/Linux; choose a supported method per target. [Tauri test documentation](https://v2.tauri.app/develop/tests/webdriver/).

| Test ID | Acceptance | Requirements / package |
| --- | --- | --- |
| U01 — Account workflow | Add/cancel/paste/login, import, switch pending/success/failure, per-card refresh and confirmed deletion; duplicate actions blocked; redacted revisions reconcile correctly | B04/B05/B20; M06 |
| U02 — Display parity | Active/scoped/main rings, percentage/reset time, consumption order, active/next/exhausted badges, stale vs no data, subscription/manual renewal, reset offers/outcomes and priming state all appear | B03/B11/B16/B20; M06 |
| U03 — Controls and accessibility | Exact ranges/defaults and three toggles; settings survive restart; Romanian strings/diacritics; keyboard focus, readable 125/150/200 percent DPI, light/dark/system; one-second countdown causes zero network traffic | B02/B20; M06 |
| U04 — IPC boundary | Snapshot/event/action/error serialization rejects secret fields; only allowlisted commands; remote navigation blocked; no arbitrary shell/file/URL endpoint; stale preview/login IDs rejected | D03/D13; M06 |
| N01 — Installed Windows | Install/upgrade/uninstall, WebView2 availability, single instance, normal window lifecycle, installed notifications/permission denial; vault retained on upgrade/uninstall; fixture import/switch/recovery in native package | B20/B21; M06/M08 |
| N02 — macOS | Relocated original build; new arm64/x64 evidence separately; Keychain item/access semantics, file fallback, CFPreferences import, notification denial, app restart after fault; default/custom context | B06/B07/B20; M07/M08 |
| N03 — Linux later | Qualified distro/desktop dependencies, XDG path, Secret Service running/locked/missing, permissions, Wayland/X11 and notifications; window remains usable without tray-click support | B06/B20/B21; M09 |
| N04 — Live CLI adoption | Selected CLI versions on each OS, outgoing rotation captured, subsequent new query identity, already-running/in-flight/parked behavior observed separately; no promise of universal seamless continuation | B07/B08; M08/M09 |

N04 need not send expensive work or redeem grants to verify basic switching. Use an explicitly chosen account/test context and record any actual request. Record private OAuth/reset capability as verified/absent/unknown separately; lacking an eligible reset offer is not evidence that the reset endpoint failed.

## 7. Future Codex evidence

The [future provider plan](FUTURE_CODEX.md) defines C00–C04 gates. Add test groups when those packages begin:

| Group | Scenarios |
| --- | --- |
| C-PROTOCOL | Version-generated schema, initialization order, login cancellation/completion, async notifications, ID mismatch, bounded messages, process exit/restart and no-refresh account read |
| C-STORAGE | File/keyring/auto/ephemeral, managed method/workspace/store restrictions, private-home isolation, concurrent refreshed cache, no global logout before switch, preservation of config/profiles/MCP/history |
| C-IMPORT | Exact current switcher format once known, duplicate/workspace identity, newer rotation retained, corrupted source retained, idempotent merge |
| C-QUOTA | Dynamic limit buckets, optional primary/secondary, missing/unknown windows, credits count vs partial detail list, API-key unsupported quota state, read/events without dummy inference |
| C-ADOPTION | Owned sidecar vs global CLI vs already-running IDE/desktop scope; labeled isolated launch; native evidence per surface |
| C-RESET | Future separate feature only: UUID replay/lost reply/restart/already-completed result, opaque credit IDs, reread quotas, no inherited default-on Claude redemption |

## 8. Commands and completion record

The public repository validation command is:

```sh
python scripts/check_repository.py
```

After M00 creates/pins the workspace and frontend, establish these or equivalent named scripts. They are proposed commands, **not checks that currently pass**:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm --prefix apps/desktop ci
npm --prefix apps/desktop run check
npm --prefix apps/desktop run test
npm --prefix apps/desktop run build
```

Run pure-crate checks first when the desktop/platform dependencies are not available, e.g. `cargo test -p switcher-core --locked`. Record platform-qualified package build/E2E commands only after scripts exist. Native package checks are additional to unit tests, not aliases for them.

For each completed package, update the progress ledger with exact command, date/platform/version, evidence label, pass/fail, paths changed, relevant IDs covered and remaining gaps. A command failure cannot be converted to “passed with caveat”; record the failure and its effect. After passing relevant checks, repeat/broaden only for a new change or unresolved concern.

Release acceptance requires a requirement-to-evidence matrix for B01–B21 and D01–D13, no unresolved credential-corruption/identity-mismatch failure, package evidence for claimed OSes, and clear compatibility status for private Claude endpoints. Future Codex support is independently qualified and is not an excuse to defer an existing Claude feature.
