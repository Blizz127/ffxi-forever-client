# Remote crafting and auction-house implementation path

Investigated read-only on 2026-11-11 for the owner's instruction to finish `/fb` server command support first, then integrate the companion. This document describes source preparation, not deployed endpoints. The forwarded backend contracts below govern read mappings; earlier implementation discussion remains planning context. Existing game-engine findings remain in `INTEGRATION.md`.

## Existing identity can be reused without native passwords

Reference checkout: `[redacted-path]`, commit `5ad886e3`.

- `deploy/banshee-accounts/server.py:758`: `POST /v1/login`, `GET /v1/me` and the existing Banshee session bearer.
- `server.py:817`: `/v1/device/start`, `/v1/device/poll`, `/v1/device/approve` and `/device` support signing in on a phone or another device.
- `identity.py:652`: `is_provisioned(user_id, "ffxi-forever")` records established ownership; the native Forever account login equals the Banshee username.
- `backends.py:596`: `ffxi_account_ids()` is a credential-free lookup pattern for regular FFXI. Forever needs an equivalent against its configured database; do not accidentally query the regular realm.
- `FFXI-FOREVER.md`: native account provisioning is write-once; current access requires the existing admin/game-grant policy. The accounts service's DB grants currently allow selected account ID/login reads and account inserts, not arbitrary character inventory reads.

Add a credential-free companion grant route using the current Banshee session, `_blocked` checks, current game entitlement and established provisioned identity. Resolve the native account ID on the server. The realm must independently check that native account's current status and own the character check (`chars.accid`). A browser-supplied account ID or username is never authority.

A new short-lived proof needs a dedicated purpose/audience and verifier key, revocation/expiry and narrow scopes. Neither native profile hashes nor launcher tokens qualify. `launch_grant_forever.py` in that checkout is WoW/Battle.net work, not the FFXI grant. Existing `join_tickets.py` lacks an audience and must not silently become a cross-game token format.

Do not use `/v1/games/ffxi-forever/credential` for this flow: that route can provision an account and returns its native password. A companion should not request or retain native game credentials. The app backend should retain its Banshee session server-side and give the browser an HttpOnly same-origin session plus CSRF protection. Current accounts endpoints are bearer-based; cookie/CORS integration is additional work.

## Smallest authoritative server path

The current map loop owns `HashMap<SocketAddr, Session>` in `server/crates/xi-map/src/main.rs:421`. Neither xi-map nor xi-search exposes a character/crafting/AH HTTP API. `xi-map` has no HTTP dependencies and no library target exposing its private sessions.

A new companion-specific durable command queue fits the existing architecture better than independently writing character DB rows:

1. A companion gateway authenticates and records an owner-bound operation, with a bounded canonical payload, expiry, operation ID and request hash.
2. The map loop polls a small bounded batch and matches it to an existing current human session by account AND character. `Session::holds_current_entry()` must still hold at execution time.
3. A typed session operation reads fresh state or invokes the existing crafting/AH engine on that same map thread.
4. The operation's durable phase/result is committed together with its corresponding game writes. A retry retrieves the same receipt; it never replays a mutation because a response was lost.
5. The gateway returns the owner-only result. The normal client receives the same inventory/status packets the engine already queues.

`server/crates/xi-map/src/session/bridgetell.rs:61` (`pump_bridge_tells`) is a useful scheduling/ownership pattern: the main loop calls a bounded pump against current human sessions, and nothing is queried when the feature is disabled. Its chat delivery deletion/in-memory deduplication is **not** sufficient transaction semantics for item/gil mutations. Use new tables and a new authorization scope; do not repurpose `forever_bridge_in`, DIT chat/persona sockets or bot ownership authority.

The gateway can live alongside the existing accounts HTTP service or as a separate companion service. Its database permissions should cover its queue/receipts and a minimal own-account character projection, not raw inventory updates. The map already owns authoritative character/game state.

## Concrete increments after `/fb`

### 1. Authenticated reads

Implement the credential-free grant, character list, live-session snapshot, server-filtered recipe catalog and AH stock/history. A snapshot should return only required fields: character ID/name, active era, current crafting skills/key-item eligibility, unlocked usable main-inventory item counts, gil, inventory capacity, state revision and timestamp. Do not return raw `CharRows`: it contains much more state than the app needs.

For an online character, build the snapshot from the owning `Session`/`PlayerState`, not a stale DB dump. The current backend-v1 increment provides current online human Sessions only. Any saved/offline projection is future work and cannot authorize an action. `xi-search/src/handler.rs:182` and `:215` supply existing stock/history semantics; serialize the same result and era policy instead of inventing prices or exposing hidden asks.

The companion UI should add a real connected mode only after these routes are present. It must keep its local sandbox data separate from real inventory and never submit imported JSON as game state.

### 2. Online crafting and bids

A queue payload contains operation ID, character ID, expected revision, and either recipe ID or AH item/single-or-stack/offered gil. It does not contain arbitrary packets, browser-computed outcomes, material slots or a declared purchase price.

- Craft allocation and validation belong to `Session` on execution. Reuse `session/synth.rs` checks, recipe resolution, claims, cooldown, timing and outcome/skill-up code. Factor a typed entry point or build the internal combine request only after fresh slot allocation. Add operation tracking to the existing start/result/commit phases; do not infer success by scraping chat packets.
- Bids reuse `session/auction.rs` validation and `ah_transact`, which stages inventory and commits its writes with `DbWrite::AhBuy`. Attach the terminal receipt to the same transaction. A buyer pays the offered bid, stock may race, and the seller is paid once by the existing trigger.
- Preserve native era, character status, inventory reservations, Rare/Exclusive/NoAuction, free-space and gil checks. An explicit companion commerce policy is needed for remote AH access; the feature must not globally remove the native zone's AuctionHouse gate.
- Initially return an explicit offline/unavailable response if no current authorized session exists. Do not start a bot pseudo-session or supersede a human to get access.

### 3. True away-from-game operation

The requested on-the-go feature ultimately needs offline operation. Before enabling it, add an atomic account/character lease shared with native login, bot startup, map admission and logout. A check that `accounts_sessions` is currently empty followed by an independent write is racy.

A detached companion controller should load the same authoritative character/commerce engine under that lease, finish/reconcile its durable operation, save, and release ownership. Native login must either wait for a running operation or transfer ownership after completion; a stale session can never save over a new owner. This needs changes in xi-connect and xi-map together, plus additive schema and crash recovery. Do not reinterpret native session key rows as browser sessions or clear native session rows to make the companion appear offline.

## Proposed browser contract

These are source contracts for future endpoints, not existing APIs. Device and mutation body/receipt proposals remain unchanged; reads follow the forwarded backend-v1 mappings:

| Method/path | Request/result |
| --- | --- |
| `POST /api/v1/session/device` | Begin existing Banshee device sign-in through companion backend |
| `GET /api/v1/session` | Session and own selectable characters; no native password |
| `GET /api/v1/characters/:id/snapshot` | Unwrapped current online human Snapshot, validated UTC time, advisory read revision |
| `GET /api/v1/characters/:id/recipes` | Native catalog, ordered ingredient multiplicity, four optional result tiers, whole-level skill map and key item |
| `GET /api/v1/auction/items/:id?stack=0\|1&character_id=:character` | Own-character/current-Session market policy; selected listing count and newest-ten oldest-first completed sales; no current asks |
| `POST /api/v1/characters/:id/crafts` | One recipe ID, operation ID, expected revision; `202` operation receipt |
| `POST /api/v1/characters/:id/bids` | Item ID, single/stack, offered total gil, operation ID, expected revision |
| `GET /api/v1/operations/:id` | Owner-only queued/running/terminal state and authoritative outcome |
| `POST /api/v1/session/logout` | Revoke companion session and clear browser cookie |

For quantity crafting, show a cancellable sequence of individual server operations. Do not promise a single instantaneous batch or guaranteed normal results. Buying several units/stacks similarly needs explicit bounded requests, per-request receipts and a total spend limit.

## Forwarded backend-v1 read mapping

Consumed `[redacted-path]`, SHA-256 `fa3c7651f6904c38d2266fd69ea96a5eb8da4c7afed10c982a34e58ad5213cea`, from backend source `eff6da4a1d2923139fe0192fa9f53fdcad1379dc`. Archive SHA-256 `9bf28e137be666cf0c4c91ee32bf40e5bfc10c8a2b1a8c48119c781bd1aa36a3` was also checked. Existing platform task `0fcbfa0421bc` remains the backend owner. Original `1074` command, Snapshot and Receipt shapes are retained; this attachment supersedes only the earlier proposed read-to-BFF fields and global-market route semantics.

- Native `ReadResponse::Session` supplies at most 32 `{character_id,name}` records. The mapping produces `{id,name}` records only. Authentication, realm, CSRF and capabilities must come separately from a genuinely verified BFF session; a native character list does not establish authentication. No real capability is enabled.
- Native snapshot/catalog/market response wrappers are unwrapped. Unix-second `observed_at` and sale `sold_at` values convert to validated canonical ISO8601 UTC. Invalid, lossy or unrepresentable timestamps yield unavailable; no zero fallback is invented. Current snapshot reads require an online current human Session.
- Catalog rows keep `recipe_id`, `crystal_id`, the ordered `ItemCount` ingredient list, four normal/HQ optional result entries, whole-level craft-code skill map and optional `key_item`. Character/revision/time/era/craft-cap metadata stays attached. This uses the native runtime recipe table and policy, neither the client 1,004-row TSV nor the server 919-row TSV as a substitute. Output is bounded to 4,096 recipes and native read responses to 2 MiB.
- Auction reads require an owned character selector. Selected `stock` is `stack_listings` for a stack query or `single_listings` otherwise; both are counts of listings, never counts of individual units. The mapped response carries both counts, `stock_unit: "listings"`, selected character and observation time. Newest-ten native history stays in its oldest-first presentation, including public historical seller/buyer names. No current asks or atomic purchase quote is added.
- A `read:<map_instance>:<generation>:<observation>` revision is an advisory observation identity. Even equality with `expected_revision` cannot authorize mutations. The browser rejects read-only snapshot revisions when preparing a craft or bid; future mutations still need complete native decision-state revalidation and engine custody. Their existing operation-ID/body/receipt shapes remain unchanged.

`lib/backend-read-map.js` contains pure mapping/validation source only. It is not imported by the shipped HTTP servers and implements no listener, verifier, grant, transaction, map pump or credentials. `dist/remote.js` remains unimported by the UI. Fake-provider/transport checks prove mapping behavior, not production access or activation.

Forwarded upstream evidence is separately scoped: nine read/search fake-provider checks at `253309bb` and 24 canonical/queue fake-transaction checks at `eff6da4a`. Twelve composed map checks, actual SQL durability/concurrency, real identity/HTTP/map pump and native crafting/AH engines remain unrun or unimplemented as recorded in the attachment. The source queue is not a functioning live backend. Crafting still needs native material reservation, timed completion, skill-up handling and durable custody; it cannot be represented as a one-shot atomic craft. No launcher, DIT or game-join credential becomes companion authority.

## Additive backend-v2 retained revision mapping

Consumed the v2 archive `[redacted-path]`, SHA-256 `53d741c8eeb8ebfc6f912f8a2544484a90835a8ae0cf50d51cd4a05c8bbbaaab`, and verified all six manifest entries. The contract is retained in `references/backend-contract-v2-retained-read.json`; its vectors are copied verbatim to `tests/fixtures/backend-revision-v2-vectors.json`. This amendment extends revision-family acceptance only. The v1 wrappers, field mapping and original `1074` command/Snapshot/Receipt shapes remain unchanged.

Both snapshot and catalog mappers accept exactly `read:<u64>:<u32>:<u64>` and `obs1:<u64>:<u64>:<u64>`. Every component must be positive canonical ASCII decimal without signs or leading zeroes. Bounds use `BigInt`; the original string is returned without numeric conversion or rewriting between families. Malformed, zero, overflow and unknown-family values still yield unavailable.

`read:` stays view-only. An `obs1:` value identifies a server-retained projected-state lookup; it is not authentication, permission, complete compare-and-swap state or execution readiness. Separate snapshot and catalog reads have different keys. The browser keeps snapshot key A for `expected_revision`, rejects catalog key B as a replacement, and requires separately supplied verified capabilities before preparing a request. Actual craft/bid capabilities remain disabled and the UI/runtime remain unconnected.

Retained expiry is internal. No expiry field or TTL is derived from `observed_at`; the browser's existing 30-second freshness check is advisory and cannot establish native retention. Native admission must still check retained lookup, revocation/expiry, owner/generation, exact current state and engine custody. Fake transport checks exercise only browser preparation and mapping, not these native decisions.

The attachment reports two retained-read tests at `d730c84d` among 48 native checks and matching retained source files at `3a3b16b4`. That is forwarded evidence, not a local rerun or a full `3a` build, SQL durability or retail readiness claim. This source amendment adds no prerequisite to the server owner's approved rc70/2051 staging. Actual remote activation still follows real `/fb` readiness and the existing identity/schema/device/source/era gates.

## Additive backend-v3 owner receipt mapping

Consumed `[redacted-path]`, SHA-256 `c9edf395bd83bbb8cf8a92ef0bdf0a506798739e094b2708eb1d6a599ce17a98`, and verified all nine manifest files. The exact contract is attached as `references/backend-contract-v3-receipt-read.json` from native source `33a94fe13205915afb257e7ecc35de0ab79f5e45`. This adds the previously missing operation-receipt mapper only; snapshot, obs1 and original mutation/Receipt shapes are preserved.

The existing `GET /api/v1/operations/:id` selects the dedicated native `ReadService::read_receipt` boundary using only `{kind:"receipt",operation_id}`. Generic `read` and `read_retained` refuse this request. `receiptReadRequest()` requires an exact canonical lowercase UUIDv4; `mapReceiptRead()` validates the bounded `{kind:"receipt",receipt}` response, exact requested operation ID, positive u32 returned character and current trusted owner context, then unwraps the existing Receipt. Context must come from independent current BFF verification, never browser identity assertions. The pure mapper cannot establish identity or the native committed-read proof.

Receipt reads work without an online character, snapshot, obs1 key, original mutation body, `expected_revision` or craft/bid capability. Original mutation expiry does not become receipt expiry. Native `running` with `code:"pending_reconciliation"` stays nonterminal in the unchanged browser adapter. Missing, foreign, malformed, refused or uncertain native reads remain unavailable; no failed/succeeded record, inventory/gil delta, automatic retry or re-enqueue is fabricated. A succeeded receipt exposes its stored authoritative object result and does not establish native memory publication, packet delivery, completed obligations or custody release.

The four attached core/read fake-provider tests belong to historical source `cbd7146a`, not current `33a` source. Three receipt-related Rust files and the retained document match that historical source; `lib.rs` differs. No native compilation or test was rerun here, and historical document limits are retained as history. Dormant native execution/custody work described in `FINDINGS.json` is source preparation; earlier lists of missing engine work are dated findings, not a new wire requirement or current readiness conclusion.

Only pure source mapping and synthetic transport checks are added. Actual key issuance/verification, authentication, schema/provider/transport, runtime registration, activation, install and restart remain outside this amendment. The persistent-key proposal and `/fb`-first order are preserved.

The focused source subset passed 102 checks: the prior 71 plus 21 receipt-mapper and ten receipt-transport checks, with zero failures or skips. Native providers, real authentication, SQL durability, installed transport and gameplay are not exercised by these checks.

## Focused acceptance evidence

Use the existing fake stores and synthetic sessions for auth ownership, stale/revoked grants, wrong-realm/audience, queue bounds/expiry, changed-payload replay, stale revision, zoning/supersession, shared materials, item/gil rollback, AH racing stock and one-time seller payment. Cover lost reply/commit ambiguity by obtaining the same persisted receipt after a retry. Add a native-login/companion lease race test before offline writes.

Server changes require real retail-client login and changed-gameplay verification per `[redacted-path]`; replacement-client/headless evidence is supplemental. Source/build identity and unrun steps must be recorded. Release/install work remains with the existing sole game-server integrator; player/[redacted] protections, entry/resource locks, backups and rollback remain in force. No independent deployment or account grant is implied by this investigation.

## Prepared source increment

The isolated native worktree `[redacted-path]` now contains commit `1074c403a8ba9ecc126d69bd9ebce3e7f80b1c33`, based on `9a0285ac`. Its new `xi-companion` crate defines the typed request/snapshot/receipt contract and pure ownership, scope, lifetime, replay, capacity, current-session and phase validation. Nineteen focused tests passed, including executable JSON vectors. It does not implement authentication, durable storage, a listener, schema migration, engine hooks or offline execution, and xi-map does not call it.

The reviewable patch is `[redacted-path]`, SHA-256 `809093747275902c4395aab32a45ea18f59e8ece63965dd80bdf99962e2b4b67`. It is a source handoff for the existing producer after native `/fb` readiness, not a deployable remote API.

## Browser source prepared

`dist/remote.js` implements the proposed same-origin client adapter, with 25 focused fake-fetch tests in `tests/remote.test.mjs`, 14 existing pure mapping checks, 25 retained-revision checks and seven fake-transport checks. It checks current own-character/capability access, fresh online snapshots, revisions, CSRF and session changes. Craft and bid requests retain immutable operation IDs; lost replies require receipt reconciliation, and a queued response never changes local inventory. No credentials are stored by the adapter and no sandbox state is submitted. The v2 read-contract subset passed 71 focused checks with zero failures or skips; the existing sandbox UI, browser adapter and HTTP servers are unchanged. The supplied revision vectors began as expectations and are now exercised against both actual mappers; the supplied old-mapper reproduction was not counted as a passing amendment test.

The adapter is not imported or enabled by the current UI. Platform has prepared disabled read and queue source; there is still no functioning authenticated HTTP/map-pump path, verified real-SQL durability, native craft/AH engine hook or offline lease. It is prepared source for the next phase after `/fb` readiness, not a real game connection.

## Current result

Investigation, this implementation map, prepared client/mapping source and disabled backend contract attachments only. No server, accounts service, real account, native client process or live player data was changed or accessed. The Alienware companion remains the labeled sandbox until the real gateway/server routes are implemented and installed through the existing integrator.
