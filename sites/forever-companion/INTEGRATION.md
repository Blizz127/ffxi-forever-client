# Forever Companion: game integration findings

Investigated read-only on 2026-11-11. The web app's sandbox and imported inventory are local planning data. They are not a connected game account and cannot authorize real crafting or auction-house transactions.

## Existing sources

The companion uses the same ID-based catalog bundled in the client:

- `crates/vana-client/src/client/synth_era_public.tsv`: 1,004 recipes, SHA-256 `af19e885c716be366db2d3c9ab2f6a488f0a69c11aa368ddad0c683b1f75419f`.
- `crates/vana-client/src/client/synth_era_public.source.json`: source server `e5f65c12c91a4dc0a2b6474f3922fc7cb5227dd7`, catalog commit `74129ca`.
- `crates/vana-client/src/client/crafting_log.rs`: the in-game catalog parser, search/filter rules, availability checks and base-era craft cap of 70. Its `visible` rule additionally judges result-item era eligibility and all involved craft levels. An ID in the bundled catalog alone does not prove that recipe is currently available.
- In-game item display names and icons come from the player's DATs. For readable public catalog labels, the local reference SQL at `[redacted-path]` has `itemid`, `name`, `sortname`, `stackSize`, item flags and AH category; it is static content, not character data. Those labels are identifiers formatted for the companion, not extracted localized DAT display names.

The server checkout inspected was `[redacted-path]` at `faa18b85a4338cbf4d09843377d78797bcd979fa`. Its current public TSV contains 919 recipes and has SHA-256 `254deeafeeaf41cf5bc670130fb2e3382488157b7c99beebb17c5f8b8f6c469b`. This differs from the client's frozen catalog; do not silently substitute it and claim identical client contents. The server's generator is `tools/synth_era_public.py`, with tests in `tools/test_synth_era_public.py`.

## Authoritative crafting

Within `[redacted-path]`:

- `server/crates/xi-map/src/synth.rs`: recipe resolution, era decisions, crafting formulas, success/break/HQ rolls, material-loss rolls and skill-up rules.
- `server/crates/xi-map/src/session/synth.rs`: native request 0x096, inventory stack claims, active craft state, timing, result packets, material consumption and persistence.
- `server/crates/xi-map/src/db.rs`: `synth_recipes()` loads DB recipe rows; the current session resolves the enabled recipe and HQ fallback.
- `server/crates/xi-map/src/e2e/craft_e2e.rs` and `e2e/craft_journey_e2e.rs`: existing focused behavior evidence.

Important rules for a future API:

1. Resolve recipe identity on the server, including era overlay, disabled content, craft cap and fallback HQ tiers. Never accept browser-declared results, chances or prices as authority.
2. Each required craft uses the floor `current_skill_tenths >= recipe_level * 10 - 150`; the 15-level margin is not a guaranteed success claim. Required key items and Rare result ownership also apply.
3. Allocate fresh unlocked main-inventory stacks on the map thread immediately before starting. Character status, event/death/action restrictions, item reservations and inventory capacity matter.
4. The native implementation has a 15-second request interval and a nominal 16-second active craft, modified by craft speed. Consume/claim/release, breaks, HQ and skill changes remain server operations.
5. Native departure while crafting has existing material-loss behavior. A browser session's disconnect should not accidentally be modeled as a native character leaving mid-craft; durable operation completion/reconciliation must be designed.

## Authoritative auction house

- `server/crates/xi-map/src/session/auction.rs`: browse-session status, listings, commission, bidding, canceling and atomic inventory/gil changes.
- `server/crates/xi-map/src/auction.rs`: request types, settings and result packets.
- `server/crates/xi-map/src/db.rs`: `ah_cheapest`, listing reads and guarded `AhBuy`/`AhCancel` persistence.
- `server/crates/xi-search/src/handler.rs`: native AH category lists and price history, specifically `handle_auction_house_request` and `handle_auction_house_history`.
- `server/crates/xi-map/src/session/tests/auction_tests.rs` and `e2e/auction_e2e.rs`: focused existing checks.

Buying finds the cheapest open listing at or below the offered bid; the winning buyer pays their bid. The existing guarded listing update, inventory grant and gil deduction commit in one transaction. The seller is paid by the DB trigger, not by a second map-side payment. A web integration must preserve this behavior and cannot model an FFXI bid as a guaranteed fixed-price purchase.

Native map commands currently require a zone whose AuctionHouse flag is set and a permitted commerce state. Remote access needs an explicit server-supported companion action policy. It cannot merely skip the native gate in a web proxy. Era, Rare/Exclusive/NoAuction rules, denylist, category, free space, stack size, gil and listing races still apply.

## Network and identity findings

No authenticated HTTP endpoints for character inventory, crafting or AH transactions were found in the inspected server projects. The existing services expose native protocols:

- `xi-connect/src/server.rs`: authentication over TLS and native data/view TCP connections.
- `xi-connect/src/auth.rs`: password verification, account status, TOTP refusal when validation is unavailable, optional Forever launcher client-lock enforcement, and random profile/session credential creation.
- `xi-search/src/server.rs`: native TCP browse/history protocol, not HTTP.
- `xi-map/src/main.rs`: UDP game sessions and the map loop.

The DIT “companion bridge” (`xi-map/src/bridge.rs`, `bridge_socket.rs`, `population/*`) is a different subsystem for personas, observed chat and bot intents. It is not a user-account inventory/crafting API. Existing DIT Unix-socket ownership checks do not establish a phone user's account identity.

Native profile hashes, native session keys and one-use launcher launch tokens are not browser access tokens. A future bridge needs account-owned character verification, revocation and a short-lived browser session. Existing native account status, client-lock and OTP policy must remain meaningful; copying native login credentials into browser storage is not the integration path.

## Engine coupling and concurrency

The authoritative actions are private methods of a large `Session` with live in-memory inventory, gil, effects and crafting state. `xi-map` currently has no library target exposing them. `main.rs` owns the session map and advances it on one loop, including existing supersession handling. Adding an HTTP server that independently loads and rewrites the same character would create two writers and risks restoring stale inventory on the next native save.

`accounts_sessions` is keyed by character and uniquely by account. Native login, bot startup, zone transitions and logout all interact with it; current SQL lives in `xi-connect/src/sql.rs` and `xi-map/src/db.rs`. The inspected code already handles the danger of an old session deleting or saving over a newer one. A companion needs the same care:

- Online character actions should enter a bounded map-thread command queue and use the owning `Session`.
- Offline operations need an atomic account/character lease respected by native login. Checking “no session exists” and then writing is racy.
- Do not repurpose a bot pseudo-peer or supersede a human game session to obtain a crafting session.
- Mutations need operation IDs, durable receipts/reconciliation and replay protection, particularly around uncertain commit or browser retry.
- Revalidate inventory versions, gil, session/lease ownership, price and recipe eligibility at commit time. A stale browser snapshot is advisory only.

This is a multi-service feature, not a small HTTP wrapper. A bounded first server increment should be authenticated read-only character selection/inventory plus market browse/history. Craft and bid require identity, lease/routing and durable operation semantics before enabling live writes.

## Proposed browser contract for a later server increment

This is a proposed contract, not existing endpoints:

| Operation | Proposed route | Authority |
| --- | --- | --- |
| Session/character selection | `GET /api/v1/session` | Authenticated owner and characters only |
| Fresh inventory and skills | `GET /api/v1/characters/:id/snapshot` | Server snapshot with revision and timestamp |
| Current era catalog | `GET /api/v1/recipes` | Same runtime policy as the owning map |
| Stock and history | `GET /api/v1/auction/items/:id` | Native market semantics; no invented stock/prices |
| Submit bid | `POST /api/v1/characters/:id/bids` | Offered gil, single/stack, operation ID; server validates and commits |
| Submit craft | `POST /api/v1/characters/:id/crafts` | Recipe ID, operation ID; server resolves current stacks and rules |
| Operation receipt | `GET /api/v1/operations/:id` | Owner-only terminal/pending state, consumed items and results |

The browser should use a same-origin backend with an authenticated HttpOnly session and CSRF protection for mutations. Imported JSON, local sandbox inventory and simulated market actions never become requests for live transactions.

## Scope and evidence limits

No live host, accounts, credentials or player saves were read. No game transaction, server deployment, account change, restart or API connection was performed. Server source was not modified. `[redacted-path]` keeps the game-server lead as sole live integrator; any eventual server installation must use that existing route.
