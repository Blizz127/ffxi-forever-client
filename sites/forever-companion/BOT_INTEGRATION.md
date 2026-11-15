# Native `/fb` integration contract

Read-only source investigation for the owner's 2026-11-11 request: enable server bot commands first, then integrate remote crafting and auction-house access. No live query, build, test, deployment or player mutation was performed for this note.

## Source identities and the present gap

The client implementation is in `[redacted-path]`, branch `private/int96-20261110`; it already forwards `/fb` and has the Bots window. The main `forever-client` checkout's `develop` branch is not the private integration tree.

The native server implementation is already present in `[redacted-path]`, branch `ffxi/mob-rot-gas-r105-20261111`, inspected HEAD `9a0285ac` (the client's earlier audit cited its ancestor `39d08dbf`). `[redacted-path]` main is an older independent tree: its capability line still contains only `con1`, and absence of `/fb` there does not mean native implementation is missing everywhere.

The last retained runtime receipts inspected by the client's audit describe `v3.77-rc8`, map source `fe150668`, which has no native `/fb` and advertises `CAP1 {"con1":1}`. This note did not re-observe the running map; later installation must be established by the existing live integrator. Relevant retained evidence:

- `[redacted-path]`.
- `[redacted-path]`.
- `[redacted-path]`.

The missing deliverable is the coordinated native server/DIT runtime increment and actual client round trip, rather than another client-side command parser.

## Client wire contract

Private client `crates/vana-client/src/client/fb.rs`, `textcmd.rs`, `con.rs` and `modern/fbwin.rs` implement the following:

1. In the Modern interface, bare `/fb` opens or closes the Bots window. `/fb help` is the explicit server help request. With arguments, `/fb` forwards the normalized command as ordinary c2s chat `0x0B5`, kind `0` (say).
2. Forwarding remains closed until the server sends a system chat line whose exact prefix is `CAP1 ` and JSON object includes numeric `"fb":1`. Strings, booleans, absent keys and other numeric values do not grant support. A recognized new object refreshes both `con1` and `fb`; unknown keys are ignored.
3. The client normalizes whitespace, drops control characters and limits the carried UTF-8 line to 100 bytes. It adds no character ID, ownership assertion, position, target ID or GM identity. The existing shared chat gate permits one chat packet per second. A rejected command does not consume that gate.
4. Buttons map directly to the native verbs below. Bulk dismissal requires a second confirmation click. The server remains authoritative for all ownership, target, movement and gear decisions.

Private client107 work already adds optional party-chat fallback when `fb:1` is absent: `assist`, `hold`, `pull`, `rest`. These are live DIT worker keywords and have different semantics from some native commands (`hold` follows without fighting; native `stay` holds position). This fallback cannot summon, dismiss, list or equip bots. It is not native `/fb` support and must not be mistaken for the completed server integration.

## Native command grammar already implemented

Paths below are relative to the inspected native server tree.

| Command | Implementation | Semantics |
| --- | --- | --- |
| `/fb`, `/fb help` | `server/crates/xi-map/src/fb/command.rs`, `session/fb.rs` | Private system help; modern client uses bare `/fb` locally. |
| `/fb list` | `fb/listing.rs`, `session/fb.rs` | Current inviter-owned party bots, private text job/level/HP/MP status. No structured bot-status feed. |
| `/fb summon [name\|all]` | `fb/summon.rs`, `fb/placement.rs`, `session/fb_summon.rs` | Omitted target means all owned bots; grounded bounded nearby placement and actual-arrival reconciliation. |
| `/fb dismiss [name\|all]` | `fb/dismiss.rs`, `session/fb.rs` | Actual native party leave, cancellation and required save/event processing; queue admission is not completion. |
| `/fb follow`, `/fb stay`, `/fb rest` | `fb/orders.rs`, `session/fb_orders.rs` | All currently owned bots; no extra name/all argument. Persistent native owner orders. |
| `/fb attack` | `fb/orders.rs`, `session/fb_orders.rs` | Authenticated owner's current selected target; normal claim, era and readiness checks. |
| `/fb pull` | `fb/orders.rs`, `session/fb_orders.rs` | Unique owned Puller engages the owner's selected enemy once. No autonomous target search or lure/return loop. |
| `/fb assist` | `fb/orders.rs`, `session/fb_orders.rs` | Resolves the unique current party tank's current engaged target. |
| `/fb role <name> <tank\|healer\|dps\|puller>` | `fb/orders.rs`, `session/fb_orders.rs` | One exact owned name; `all` is refused. |
| `/fb gear scale [name\|all]`, `/fb gear bis [name\|all]` | `session/fb_gear.rs`, `fb/gear.rs`, `fb/gear_catalog.rs` | Curated real-item bot loadouts subject to actual era/equip policy; coverage is partial. |
| `/fb gear show <name>` | `session/fb_gear_read.rs` | Private current equipped-item read. |

The native first-slice parser recognizes case-insensitive exact `/fb`, permits printable ASCII only, limits the whole input to 200 bytes, and accepts letter-only bot names up to 16 bytes. Names resolve case-insensitively within one authoritative party snapshot. The narrower client 100-byte limit is safe; Unicode name passthrough can still elicit a native usage refusal. Recognized malformed `/fb` is consumed privately in every chat channel rather than broadcast. There are no implemented `formation`, `spacing`, `distance`, `fill`, `fallback` or `rally` verbs.

Dispatch is wired in `session.rs`'s `0x0B5` path before ordinary chat. Reads run through `main.rs::session::serve_fb_reads`; mutations run through `population/mod.rs::serve_fb_mutations` on the population game thread. This is not a Lua GM-command registration requirement.

## Capability discovery detail

Native `con1.rs` declares `CAP1 {"con1":1,"fb":1}`. `session/con1.rs::con_feed_pass` sends it once per zone after the normal delay, only with the map CON feed enabled and the character's QST1 quest tracker variable enabled (`!qtrack on`). It excludes bot/not-in-zone/shutting-down/leaving sessions. The normal private client must actually receive that line before its UI enables native requests.

`fb:1` was originally documented as the build1 help/list/summon/dismiss slice; the current client treats it as a single gate for all native controls, and the inspected newer native tree implements later verbs as well. A build1-only rollout must therefore report unsupported later verbs honestly; a future per-verb/version capability would improve gating. Do not set `fb:1` on rc8 merely to enable buttons: rc8 lacks the native dispatcher and would expose forwarded text as ordinary chat.

## Ownership and DIT cancellation dependency

`fb/authority.rs`, `fb/selection.rs` and native World party hooks bind a registered population bot to the human who invited it **after accepted native membership insertion**. Party leadership, a name prefix, a roster entry, an old party snapshot or client-supplied identity does not confer ownership. Existing pre-cutover unbound parties need ordinary accepted human reinvitation; do not fabricate bindings for convenience.

Help/list and rejected requests do not activate managed control. The first validated ready native mutation (summon/dismiss/order/gear) changes a sticky per-bot control lifetime. Managed intents then need immutable `{binding_revision,epoch}` tokens. Missing, stale or closed-state work is refused. This means deploying native mutation support without the matching DIT adapter can leave the current tokenless worker unable to command newly managed bots.

The existing authoritative specification is `docs/forever-fb-control-wire.md` (r11 through later additive order/role refinements). The DIT adapter must:

- Parse native `control_state` events and observations, preserve map incarnation and population login generation, and freeze each action's token when the action is created.
- Cancel delayed sends, retries, automatic modes and pending outcome correlations on epoch/binding changes or closed states. Late replies must match the original action ID and token; they must not affect a newer action of the same kind.
- Keep `legacy` cohorts unchanged, respect persistent Follow/Stay/Rest/Attack/Pull orders, recover coherent state after reconnect, and permit only the genuine current stamped invitation while `invited`.
- Never relabel old work with the newest cached token. Unknown states/orders fail closed. Native lifecycle, zoning, logout and recovery remain separate.

Native source is already wired to enforce stamps at intent admission and actual per-subpacket execution, cancel persistent goals and queue entries, and publish control state on reconnect and transitions. DIT source ownership and the actual matching deployed worker revision were unresolved in the retained readiness records; the existing coordinator/DIT/server leads must establish their current state rather than start a duplicate driver writer.

## Focused checks to use

The server already has focused regression fixtures; this investigation did not repeat them. Existing test names include:

- `fb_native_packet_admission_is_private_and_preserves_unrelated_chat`.
- `fb_native_list_uses_inviter_not_leader_and_current_bot_session`.
- `fb_native_chat_packet_summons_then_dismisses_through_real_executor`.
- `fb_native_control_dispatch_revalidates_each_subpacket`.
- `fb_native_control_queue_keeps_creation_stamp_and_cancels_old_goal`.
- `fb_native_summon_executor_confirms_same_zone_arrival_once_and_fences_old_work`.
- `fb_native_summon_executor_waits_for_real_cross_zone_publication`.
- `fb_native_summon_executor_refuses_missing_mesh_and_held_cross_zone_without_fencing`.
- `fb_follow_stay_native_cancels_old_work_and_rejects_driver_override`.
- `fb_pull_engages_only_the_unique_owned_puller_and_stay_cancels_it`.

They live in `server/crates/xi-map/src/session/tests/fb_*_tests.rs`, with pure grammar/authority/control/placement checks under `src/fb/`. The crate is `xi-map`; use the producer's existing test cache/entry/resource locks and narrow exact names, not another full build or broad suite solely to restate retained evidence.

Client focused tests in private `client/fb.rs` cover exact forwarding, strict capability gate and double-click admission. Native fixtures use fake stores/packet fixtures and are not actual retail-client login or gameplay evidence. The owner's current retail-client verification order still requires focused real-client evidence for the server increment, with source identity and unrun limits recorded.

## Delivery route and sequencing

The requested order is:

1. Existing server/kit producer selects the already-implemented native `/fb` increment and matching DIT adapter, preserving completed producer work and retained checks.
2. Existing sole live integrator handles the authorized runtime install with current owner/player/GM/access/restart restrictions, backup and rollback. The retired disposable test-server route must not be revived.
3. Confirm real receipt of numeric `CAP1.fb=1`, ordinary accepted bot ownership, private help/list, bounded summon arrival, follow/stay/rest and dismissal through the actual client/driver path. Run only the focused changed-feature checks needed for the installed slice.
4. Then integrate the remote character/crafting/AH API described in `INTEGRATION.md`. Bot control tokens do not authenticate a browser user or grant character inventory/gil access; the companion needs its own authenticated owner session and single-writer transaction path.

The retained kit/readiness notes describe Build1-first, matching-adapter/integrated-smoke and installation/restart holds. They establish why that prior artifact was unsealed, not a fresh global verdict about current authorization. The parent/coordinator must reconcile these against the latest owner directions and exact current integrator state. This document grants no live deployment or access change and performs none.

## Prepared capability-discovery correction

The isolated source candidate at `[redacted-path]`, commit `1e2219d6eb9817fcda353695dd8d09af9601ded9`, separates once-per-zone native `/fb` discovery from QST1 and CON1 opt-ins. A map with CON1 disabled advertises only numeric `fb:1`; feed preferences and human-session lifecycle gates remain. Plain retail clients may display one private CAP1 line. This is source-only and does not enable rc8.

Two exact checks of the actual changed `con1.rs` module passed; full native Session compilation, its two new fixtures, the updated E2E and real retail gameplay remain unrun. The patch applies to producer `500fafea` in an isolated index; producer files were untouched. See `[redacted-path]`. The old E2E capability fixture overlaps the server owner's `c7da90cf` literal-expectation correction; retain the new semantics and numeric `fb:1` when merging.
