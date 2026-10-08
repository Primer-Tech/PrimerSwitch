# ClaudeSwitch behavior and parity specification

Status: implementation in progress; see [the evidence ledger](PROGRESS.md). Baseline: upstream commit `4b1b441084fccf5f717fcf41d1b5e668f18559a0`, preserved in the separate private checkout. Inspected on 2026-10-02 on Windows. This document describes source behavior; it does not claim that authenticated requests or macOS runtime behavior were validated here. The owner subsequently requested internationalization: English is the replacement's default, Romanian remains supported, and selected image-model Direction A for the replacement dark dashboard.

The replacement, now named **PrimerSwitch**, must carry every application feature below. Reliability changes are listed explicitly at the end; an implementer must not silently simplify policy, remove automation, or substitute a tray-only product. Future Codex/OpenAI support has a separate [specification](FUTURE_CODEX.md). Replace product-name labels intentionally; keep the archived ClaudeSwitch code and external CLI field names unchanged.

## 1. Source inventory

All links refer to the preserved source. Search the indicated symbols when adding tests.

| Source | Responsibility and useful entry points |
| --- | --- |
| `Models.swift` | `StoredAccount`, arbitrary `JSONValue`, tolerant record decoding, usage/model windows, renewal dates, `ResetGrant`, `ResetStatus`, `UsageJSON` |
| `AccountStore.swift` | JSON account directory, name sanitization, permissions, list/save/delete |
| `ClaudeConfig.swift` | Active identity, default model, `oauthAccount` replacement, one-time backup |
| `Keychain.swift` | Reading and updating the Claude Code Keychain item |
| `OAuth.swift` | PKCE login, token exchange/refresh, profile, user-agent detection |
| `UsageClient.swift` | Active/inactive token rules, inference/metadata usage reads, cooldowns |
| `WindowStarter.swift` | Tiny inference request, rate-limit header parsing, budget refusal classification |
| `SwitchEngine.swift` | Switch sequence, usable/headroom tests, candidate ordering, notifications |
| `ResetEngine.swift` | Last-resort/expiry policy, claim validation, outcomes, durable request identity |
| `Monitor.swift` | Settings, scheduler, profile/priming, resets before switching, account actions |
| `Views.swift` | Romanian interface, rings, cards, renewal editor, login modal, settings |
| `ClaudeSwitchApp.swift` | SwiftUI app, appearance, original diagnostic probes and policy examples |
| `build-app.sh`, `Package.swift` | Swift 6, macOS 14, normal desktop app, ad hoc signing |

The `experimental Chrome helper` saves/restores claude.ai cookies separately. The desktop Swift app does not call it. Preserve the helper as an independent archived component; browser-session switching is not an existing desktop feature to recreate through cookies.

## 2. Account data and settings

### B01 — Saved accounts

The original stores plaintext JSON in `~/Library/Application Support/ClaudeSwitch/accounts/<sanitized-name>.json`, with directory mode `0700` and file mode `0600`. It accepts Unicode alphanumerics plus `@ . - _` in names and replaces other characters with `_`. Names are also runtime IDs; collisions and swallowed filesystem errors are possible. `.claude-accounts` is not this application's store.

Each saved account contains the following state, which migration must preserve where valid. The legacy JSON encoder writes `Date` values (including `savedAt` and nested cached usage resets) as ISO-8601 strings; the explicitly numeric persisted timestamps below are epoch seconds, while OAuth expiry is milliseconds:

- `name`, `savedAt`, `oauthAccount`, full `credentials` object.
- `refreshFailAt`, `rateLimitedUntil`, `consecutiveRateLimits`.
- `lastUsage`, `lastUsageAt`, `lastEndpointReadAt`.
- `expectedWeeklyResetAt`, `primedForResetAt`.
- `subscriptionStatus`, `subscriptionStartedAt`, `planTier`, `profileCheckedAt`, `renewalDay`.
- `resetStatus`, `resetStatusAt`, `pendingResetClaim`, `lastResetOutcome`, `lastResetAttemptAt`.

OAuth fields live under `credentials.claudeAiOauth`; access/refresh tokens, expiry in epoch **milliseconds**, scopes, subscription metadata, and unknown fields must survive a round trip. `oauthAccount` includes account UUID, email, organization UUID/name and other CLI identity fields. Old records may lack newer optional fields. The replacement must use stable internal IDs and an encrypted saved vault, with an explicit legacy importer; it must not retain name-based file collisions.

### B02 — Defaults and controls

| Setting / timing | Original value | UI / persistence |
| --- | --- | --- |
| Ordinary poll interval | 300 seconds | `pollInterval`; slider 120–900, step 30; loader enforces only a minimum of 120 |
| Auto-switch threshold | 95 percent | `autoSwitchThreshold`; stepper 50–100, step 1 |
| Automatic switching | Enabled | `autoSwitchEnabled` |
| Start reopened weekly window | Enabled | `autoStartWindowEnabled` |
| Automatically use banked resets | Enabled | `autoUseResetsEnabled` |
| Weekly reset order | Soonest weekly reset first (added later; on is the original order) | `preferSoonestWeeklyReset`, default on; records saved before it existed load as on (B13) |
| Appearance | System | `appearance`: `system`, `light`, `dark` |
| Near-limit band and cadence | Threshold minus 3 points; 30 seconds | Active-only tick unless all accounts were found exhausted |
| Inactive usage cadence | 900 seconds | Persisted usage age avoids a startup burst |
| Gap between account reads | 8 seconds | Full cycle processes accounts sequentially |
| Active metadata enrichment | 1,800 seconds after last successful read | Inference supplies the normal active reading |
| Profile cadence | 86,400 seconds | Subscription/account information |
| Inactive refresh failure cooldown | 900 seconds | Persisted `refreshFailAt` |
| Metadata 429 cooldown | `min(60 × 2^(n−1), 900)` seconds | Respect a larger numeric `Retry-After` |
| Repeated exhausted notification | At most once per 1,800 seconds | Earliest main-window reset across accounts |

Preserve these Claude policy defaults in the replacement. The owner explicitly selected English as the new default language and dark as the initial appearance; existing explicit appearance settings remain preserved. Settings must be validated in Rust. Import out-of-range preferences by reporting/clamping them to supported UI ranges, rather than accepting arbitrary persisted values. Provider settings stay independent, with one owner decision (2026-10-03): automatic switching, the threshold, the check interval and the weekly reset order also drive Codex ([CODEX_USAGE.md](CODEX_USAGE.md)), with these Claude defaults, while priming and banked resets stay Claude-only.

### B03 — Subscription and renewal

Read subscription status/start time/tier from the profile at most once per day. Billing renewal is set manually as unknown or a day from 1 to 31; it is **not** inferred from `subscription_created_at`. Compute the next future renewal in the user's local Gregorian calendar, clamp day 29–31 to the month's final day, and retain the subscription start's hour/minute when available. Define a stable fallback time when the start is absent; the original uses the current time. A plan change or pause can move the billing cycle, so label this as a user-entered estimate.

## 3. Login, import and switching

### B04 — Browser login

The add-account modal opens a browser for a PKCE authorization flow and asks the user to paste `code#state`. The verifier is 32 random bytes, encoded base64url; the challenge is SHA-256/base64url and state is a UUID. Whitespace is removed from pasted input. Code exchange is followed by a profile read and creation of a stored account. Adding an account does **not** make it active automatically.

Preserve this workflow. Keep verifier/state in Rust and enforce that pasted state belongs to the pending, unexpired login. The original trusts the pasted state; matching it is an intentional correction. Cancellation or a second login must invalidate the prior pending request. Tokens and verifiers never enter frontend view models or logs.

### B05 — Import, refresh and delete

Import current Claude credentials and `oauthAccount`; original names use email, UUID, then a generated timestamp. Reimporting originally replaces the saved record and can lose renewal/cache/reset state. Merge by verified provider/account/organization identity in the replacement, retaining durable metadata and reporting identity inconsistencies.

Per-card refresh requests a new account reading. Deletion is confirmed and removes the saved account only: deleting the active card does not log the CLI out or erase its active credentials. Surface storage failures rather than reporting false success.

### B06 — Active Claude configuration

The default active identity is `oauthAccount` in `~/.claude.json`. Default model selection reads `~/.claude/settings.json` first, then `~/.claude.json`. This represents the default for new sessions; existing sessions may select a different model.

The original reads service `Claude Code-credentials` using macOS `/usr/bin/security`, first with `NSUserName()`, then service-only fallback if needed. Writes use `add-generic-password -U` to retain access attributes. The new macOS adapter must verify the actual item selector and access behavior for the supported CLI; do not blindly copy this selector to newer releases.

Windows/Linux use a Claude credential JSON file by default; macOS normally uses Keychain and may fall back to a file. Follow the CLI's effective storage choice and path override, not the operating system alone. Preserve other JSON fields, MCP configuration and preferences. [Claude authentication](https://code.claude.com/docs/en/authentication), [Claude directory](https://code.claude.com/docs/en/claude-directory).

### B07 — Switch sequence and continuity

The old sequence is:

1. Reread the outgoing live identity/credentials because the CLI may have rotated tokens; save them to the matching saved UUID.
2. Write target credentials to Keychain.
3. Replace only `oauthAccount` in `~/.claude.json`, preserving other root fields. Create `.claude.json.bak` once, then use a same-directory temporary file and rename.
4. Save the target account.

Manual switching does not first refresh an expired target, and the two active writes do not have shared rollback. The replacement must validate ownership, refresh inactive targets when needed, preserve fresh outgoing credentials, and implement recoverable multi-store writes. These are implementation improvements, not a change to which account the user selects.

The old `continuity research` concerns another machine and CLI version. Some subsequent queries may adopt new credentials; in-flight retry loops or parked workflows may retain an earlier account or stay stopped. Require native evidence before promising session adoption. Do not restart terminals, resume messages, or claim uninterrupted work as part of a credential switch.

## 4. Usage acquisition and interpretation

### B08 — Active token ownership

Never refresh the active Claude account from its saved refresh-token copy. The CLI owns that session and can rotate its token. Read live credentials each poll. If its access token changed and the saved UUID is known, read the profile owner before adopting it. If owner is unavailable or mismatched, fail the read and preserve the saved record. Recheck external identity after awaited work in the replacement.

### B09 — Inactive tokens

An inactive token is valid only when `now < expiresAt / 1000 − 120`. Otherwise refresh, retaining the prior refresh token if the response omits a replacement, updating expiry/scopes, and saving the rotated result. A refresh failure holds further attempts for 15 minutes. A metadata 401/403 on an inactive account forces expiry to zero, then one refresh and retry; active credentials are not refreshed on this path.

Persist rotated tokens before another side effect or potentially failing API call. All retry paths must enter the same cooldown logic; the original unauthorized retry bypasses part of it.

### B10 — Active inference reading

Normal active polling sends a tiny **real inference request**, whose headers expose main-window utilization and reset times. This consumes usage when accepted; it is not a telemetry-only endpoint. Preserve it for source parity and explain its purpose in the monitoring setting. Do not claim the original comment's approximate token count is a measurement here.

Header utilization is a fraction: round `fraction × 10000`, then divide by 100 to obtain percent. Reset headers are epoch seconds. At a budget refusal, a 429 can be a valid usage reading when the unified status/claim identifies a main window, or main-window utilization proves exhaustion. Clamp the spent window to at least 100; use previous information for an absent other window, otherwise the original defaults it to zero. A generic throttling 429 or a model-only rejection without main-window evidence is an error, not proof the account is exhausted. The replacement must retain an unknown marker when a carried/defaulted window is not decision-quality evidence.

### B11 — Metadata and model windows

Inactive accounts use the metadata endpoint. Active accounts use it periodically for scoped model windows and reset offers, with an ordinary-tick fallback if inference fails. Strict near-limit ticks must remain inference-only after the listed correction.

Required metadata windows: `five_hour`, `seven_day`. Optional: `seven_day_sonnet`, `extra_usage`, `limits`, `cedar_ember`. Weekly rows are `limits` entries with `group == "weekly"`; `weekly_all` overrides account-wide `seven_day`. For a configured model, the binding weekly percentage is the maximum of account-wide usage and its matching scoped row. Matching is case-insensitive prefix/substring matching, so future model labels must remain dynamic. An exhausted scoped model must not make the account exhausted for an unrelated model.

Display scoped rows highest utilization first. Candidate ordering uses the earliest weekly reset across weekly rows and the main weekly window, floored to a UTC epoch-hour bucket. Weekly priming instead uses the account-wide reset (`weekly_all`, else `seven_day`). These timestamps are deliberately different.

Carry scoped rows when fresh inference lacks them, but never carry an old `weekly_all` over a live account-wide reading. On a live weekly budget refusal, filter `weekly_all` from an enrichment result so slower metadata cannot reduce the observed 100 percent. Track scoped-data freshness separately in the replacement.

`cedar_ember` decoding is tolerant: a malformed optional block or individual malformed grant must not discard otherwise valid usage. An unavailable reset feature is not zero resets and is not a usage failure.

### B12 — Cached and failed readings

Show saved usage immediately after restart and retain it on a network error, with age and error text. Do not turn failure into zero usage. The old `apiUnavailable` flag checks whether any cached active usage exists, so stale data can still drive automatic decisions. The new freshness gate in the plan prevents that while keeping the display useful.

## 5. Switching policy and scheduling

### B13 — Candidate ordering

For threshold `T`, an account is usable only when **both** five-hour and model-aware weekly usage are strictly below `T`. No usage means unusable. A target has preferred headroom when usable and both values are at most `T − 5`; exactly `T` is still unusable even if a test requests margin zero.

Select a nonactive usable account by:

1. Preferred headroom before accounts without that margin.
2. Earliest weekly reset hour bucket; missing reset sorts last.
3. Lower five-hour usage within that bucket.

If none has headroom, still select a usable account. Preserve this fallback. Add stable account-ID ordering only as a final tie-breaker; the source does not specify a total order for exact ties.

The consumption plan includes all usable accounts, including active, in the same order. A healthy active account is not proactively switched merely because another resets sooner.

Owner option (2026-10-03): `preferSoonestWeeklyReset`, default on, keeps exactly this order. Off, step 2 becomes the lowest model-aware weekly usage (most weekly left), then the earliest weekly reset hour bucket; preferred headroom, the five-hour tie-break, the fallback, the ID tie-breaker and the plan's inclusion of the active account are unchanged. Records saved before the option existed load it as on and are not rewritten, so the Claude default is unchanged. Codex accounts are ranked with the same comparator.

### B14 — Poll orchestration and automatic switching

On each full cycle, reread configured model and active identity; poll the active account and due inactive accounts, spacing requests. Fast ticks poll only the active account. Cached inactive data younger than 15 minutes avoids unnecessary startup calls. Near-limit polling applies at `>= T − 3`, including model-aware weekly usage; it is suppressed after a result that all accounts are exhausted.

After readings/profile/priming, evaluate reset actions, then auto-switch unless a reset just succeeded on the active account. After a switch, reload active state and reread before deciding further actions. Replace the old recursive refresh with an event/state transition so one user click or timer cannot run multiple overlapping cycles.

No active account means no automatic switch. No usable active reading suspends it. At `>= T` in either binding window, use B13; if none qualifies, show the earliest five-hour/seven-day reset among accounts and throttle repeated exhausted notifications to 30 minutes. Successful and failed switches notify the user. Freshness and identity checks apply before any mutation.

### B15 — 5-hour and weekly window priming

Record the first observed account-wide weekly reset without sending a request. Once that remembered timestamp passes and was not primed, optionally send the same tiny inference request to start the next window. This applies to any polled account, including an inactive one, without switching the global login. Preserve the remembered reset and primed marker across restart. Successful priming updates usage/next reset and notifies; a definite refusal clears the pending marker for retry, while an unknown outcome retains it for reconciliation.

Owner improvement (2026-10-08): the existing `autoStartWindowEnabled` option also primes the 5-hour window. Remember each window's reset, handled reset and pending attempt independently. A due inactive account participates in the next ordinary check even while its cached usage is fresh. If both windows are due, save both pending markers before sending one inference, then update both from its result. An accepted ordinary or fast active-account inference already serves this purpose, including when reset dates are absent, and never needs a second priming request. Missing reset dates do not invent a new schedule. Unknown replies retain their markers across restart; retry only after 15 minutes and a metadata observation within the last 120 seconds. Keep English/Romanian setting descriptions, window-specific notifications and 5-hour start history accurate; a skipped or refused attempt does not become a successful 5-hour start. Priming remains Claude-only.

The original marks a reset primed even when the toggle is disabled, so re-enabling does not retroactively send for that reset. Preserve this toggle behavior unless a later product decision changes it. An already-pending unknown attempt is retained when disabled, so toggling off cannot erase its reset identity or manufacture a successful start. The replacement must persist an attempt marker before sending. An ambiguous network/crash outcome cannot provide exactly-once inference; reconcile with a fresh window read and avoid an immediate duplicate.

## 6. Banked Claude resets

### B16 — Offer and binding windows

Reset grants have `id`, `resets_left`, optional validity/cooldown times, `clears`, `paused`, `usable_now`, `use_requires_limit` and related metadata. Grant IDs match `^[a-z0-9_-]{1,40}$`. When decoded, missing `use_requires_limit` means true, missing `paused` means false, missing `usable_now` means false. A missing/empty `clears` list means five-hour plus seven-day; `seven_day_overage_included` also covers `seven_day`.

An offer requires remaining uses, unpaused state, start reached, end not reached, eligibility not explicitly false, and no cooldown. Respect the server's `next_grant_id` when present; otherwise choose the earliest-expiring offer. Unknown validity times sort after known expiry.

Binding windows are five-hour at/over threshold, account-wide seven-day at/over threshold, and `seven_day_<lowercase-model-label>` if only a matching scoped window is binding. A required real limit is checked at **100**, independently of the user threshold. Source behavior assumes account-wide weekly clearing covers a simultaneously exhausted model row; this is a compatibility assumption that needs verification, not documented server semantics.

### B17 — Last resort

Only when the active account is over threshold and **no other account is usable**, seek a grant that clears every blocking window on its account and satisfies a required real limit. Prefer the active account. Otherwise choose earliest grant expiry, then latest weekly reset. A selected inactive account is switched to before claiming.

The reset automation toggle is independent of ordinary auto-switching: an enabled last-resort reset can switch to its inactive account even when the ordinary auto-switch toggle is off. Preserve and explain that distinction in the UI.

The source's last-resort branch does not require `usable_now`; the expiry branch does. Capture that distinction in tests. Do not invent a common additional gate without verifying current contract semantics and recording a deliberate deviation.

### B18 — Expiry guard

For each other account not already selected this cycle, a usable-now offer expiring within **3 hours** may be claimed, with the real-limit condition when required. It can act on an inactive account **without switching**. At most one action per account per cycle. This is independent of whether another account has free budget; it prevents an expiring offer from being lost.

### B19 — Claims, retries and outcomes

Before sending, durably save a pending claim containing account, grant, request UUID and timestamp, plus last-attempt time. Retry an unconfirmed answer after at least 5 minutes using the same request ID for the same grant. The source abandons a pending request after 24 hours; the replacement must first reconcile ambiguous old claims rather than immediately risk a second redemption.

A final non-success verdict requires at least 30 minutes and reset status newer than the prior attempt before another claim. Claim outcomes are `reset`, `already_used`, `not_limited`, `cooldown`, `ineligible`, `unavailable` (including unknown named results), authentication failure, or unconfirmed transport. 401/403 are final auth failures; 429 is unconfirmed. A readable named verdict on other HTTP errors is accepted by the source, even though its comment mentions only 4xx. Without a named verdict, errors/unreadable bodies remain unconfirmed. Treat 5xx ambiguity conservatively in the replacement and test it explicitly.

On success, use supplied remaining uses or decrement, remove spent grants, clear the consumed `next_grant_id`, and apply cooldown. `already_used` removes that grant; cooldown verdict updates its time. A success invalidates pre-reset usage for decisions and schedules reread; do not auto-switch on the stale 100 in that cycle. After an action switched accounts, stop consuming the old action list and recompute.

Notifications combine switch + reset result. Reset refusal without a switch remains on the card; reset success notifies even without switching. Detailed outcomes remain visible on the card after restart.

## 7. Interface parity

### B20 — Desktop and cards

Preserve the Romanian interface strings and actions. A shared UI can adapt spacing to native fonts/DPI, but must retain:

- Active email, large five-hour and model-aware weekly rings, reset countdowns refreshed each second, consumption order, unavailable/stale indicators.
- Account cards with active/next/exhausted state, overall and binding scoped weekly figures, configured-model marker, small rings and countdowns.
- Subscription/tier/status, manual renewal menu, reset count/earliest expiry, latest reset outcome/details, window-priming status, delayed/error information.
- Switch for inactive accounts, per-card refresh, confirmed delete, browser login + paste completion, import current account, modal busy/cancel behavior.
- Poll slider, threshold stepper, all three automation toggles, system/light/dark selection, last-updated indicator and footer actions.

Provide visible pending state while switching and block duplicate actions. Do not render an unsupported or unknown future provider window as zero. Browser callbacks and notifications need platform adapters; all policy stays in Rust.

### B21 — Optional features and probes

The current app is a normal window (`LSUIElement` false). A tray icon, launch-on-login, automatic updater, historical dashboard, English translation and terminal launcher are future conveniences, not existing parity requirements. Do not delay core parity for them.

The old probes are not hermetic: `--probe` imports the real login; `--probe-usage` calls authenticated services and may refresh tokens; `--probe-switch-logic` finishes by creating a real config backup. New diagnostics must be fixture-only by default and emit redacted results.

## 8. Claude network contracts to isolate

These are exact source contracts, not a promise of a stable public subscription API. Place them in `provider-claude`, with version/capability evidence and sanitized contract fixtures. Never mix them with the future OpenAI adapter. The historical `reset research` is supporting evidence only.

| Operation | Contract in archived source | Timeout |
| --- | --- | --- |
| Authorize | `https://platform.claude.com/oauth/authorize`; client ID `9d1c250a-e61b-44d9-88ed-5944d1962f5e`; redirect `https://platform.claude.com/oauth/code/callback`; PKCE S256 | Browser |
| Token exchange / refresh | JSON POST `https://platform.claude.com/v1/oauth/token`; token response requires `access_token`; optional refresh token/scope; expiry defaults to 28,800 seconds | 30 s |
| Profile | GET `https://api.anthropic.com/api/oauth/profile`; Bearer, JSON, no-cache | 15 s |
| Usage and reset offers | GET `https://api.anthropic.com/api/oauth/usage?cedar_ember=1&skip_spend=1`; Bearer, `anthropic-beta: oauth-2025-04-20`, JSON | 20 s |
| Tiny inference / priming | POST `https://api.anthropic.com/v1/messages`; `claude-haiku-4-5-20251001`, `max_tokens: 1`, official CLI system text and user `hi`; same beta, `anthropic-version: 2023-06-01`, `x-app: cli` | 30 s |
| Claim reset | POST `https://api.anthropic.com/api/organizations/{organizationUuid}/reset_rate_limits`; JSON `{program:"cedar_ember",grant_id,request_id}`; Bearer, same beta | 25 s |

Login scopes are `org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload`; refresh scopes omit `org:create_api_key`. Use the source's request builders as fixtures for these exact fields, including the tiny request's system text and lowercased rate-limit header names.

User-agent is `claude-cli/<detected-version> (external, cli)`; original fallback is `2.1.259`. Its detector depends on older executable/`cli.js` layouts. The installed Windows npm release inspected here is **2.1.287**, whose executable is `bin/claude.exe` with no `cli.js`. Detect version/layout without invoking a login and verify private endpoint compatibility before retaining any fallback.

Validate reset organization IDs with `^[A-Za-z0-9-]{1,64}$` and request IDs with `^[A-Za-z0-9_-]{1,64}$` before path/body construction. Do not send credentials to arbitrary configured hosts or follow authenticated redirects to other origins.

## 9. Explicit reliability deviations

These preserve the application's intended functionality while fixing behavior that could lose data, repeat requests or make an unverified decision. Record their test IDs in the implementation ledger.

| ID | Source limitation | Required replacement behavior |
| --- | --- | --- |
| D01 | Name-based IDs and silent save/decode failures | Stable provider/account IDs; preserved damaged files; visible typed storage errors |
| D02 | Plaintext saved account collection | OS-protected encrypted vault; active CLI store remains compatible with the CLI |
| D03 | Pasted state is trusted | Match pending PKCE state, reject expired/canceled login completion |
| D04 | Keychain/config writes have no shared recovery; manual target can be expired | Ownership checks, target refresh, encrypted journal, conditional rollback and recovery |
| D05 | Import replaces durable metadata | Idempotent identity-based merge retaining renewal, pending operations and newer tokens |
| D06 | Successful inference resets metadata cooldown; failed enrichment has no attempt timestamp | Separate inference/metadata failure state; bounded enrichment attempts honoring cooldown |
| D07 | Successful fast inference can still trigger metadata enrichment despite `inferenceOnly` | No metadata request on a strict fast tick, whether inference succeeds or fails |
| D08 | Cached active/scoped/reset data can drive automation | Independent freshness/identity requirements, targeted revalidation before mutation |
| D09 | Async tasks address mutable array indices and recursive refreshes | Single runtime owner; stable IDs, revision-tagged results, coalesced work |
| D10 | Rotated tokens are persisted later by the caller | Durable token rotation before further network/mutation work |
| D11 | Unauthorized retry bypasses normal 429 state | One bounded retry path using the same backoff and timestamp handling |
| D12 | Priming persistence/ambiguous claims can repeat a scarce side effect | Durable attempt records, same-key reset retry, reconciliation before abandoning an unresolved claim |
| D13 | HTTP error snippets can reach tooltips | Redacted typed details; no authenticated bodies, tokens or raw credential data over IPC/logs |

Adding a stricter unsupported-feature gate, changing last-resort `usable_now`, changing model matching, altering defaults, or removing automatic priming/reset redemption requires an explicit documented decision and acceptance evidence; it is not an implicit refactor.
