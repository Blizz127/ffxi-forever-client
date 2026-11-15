# Forever Companion

An installable browser companion for desktop and phone, with an original FFXIV-inspired crafting-log layout and the actual FFXI Forever recipe catalog.

## Run

The portable preview uses Python 3.9 or newer and opens your default browser:

```sh
python3 start-companion.py
```

On your Bazzite PC, use the installed **Forever Companion Preview** desktop shortcut. It starts a loopback server only when needed. The launcher uses a transient user service on Linux when available; nothing starts automatically at login. The game client is separate.

Alternatively, use Node.js 20 or newer. No packages or build step are needed.

```sh
cd sites/forever-companion
npm start
```

Open http://127.0.0.1:4317 on the machine running the server. `PORT=4318 npm start` selects another port. On Windows, set `PORT` in your shell first if needed.

To preview from a phone on the same private network, run `HOST=0.0.0.0 npm start` and open `http://<devbox-LAN-IP>:4317` on the phone. Loopback URLs on the phone refer to the phone, not the devbox. Install/offline support requires HTTPS or localhost; ordinary LAN HTTP supports the responsive interface but generally cannot install the service worker.

For deployment, publish the contents of `dist/` at an HTTPS site root. The app is static and has no runtime dependencies, external fonts or external assets. Its manifest offers standalone installation on supporting desktop/Android browsers; on iOS use Safari's Add to Home Screen. Raster 192px/512px and SVG icons are included; install availability varies by browser. The service worker caches the catalog and shell for offline browsing after the first successful visit.

## What works

- Same 1,004 recipe IDs and recipe definitions bundled in the current client, across eight crafts.
- Base-era visibility (919 recipes), skill-cap ranges, recipe/ingredient search, materials-ready and favorites filters.
- Crystal, ingredients, normal result quantities, HQ tiers, subcraft and key-item requirements.
- Quantity planning, combined shopping list and missing-ingredient auction searches.
- Local practice market with generated stock/prices, confirmation, gil deduction and inventory updates.
- Local practice synthesis with material consumption and a guaranteed normal result; no success-rate/HQ/break/skill-up simulation.
- Editable sample inventory and craft skills, local persistence, JSON import/export, activity log.
- Responsive desktop/phone layout and keyboard-accessible controls.

## Game connection

**This version is a local sandbox and planner. It has no game-account login and makes no real purchases or crafts.** Sample character data, gil, prices and stock are labeled throughout. It does not store or ask for game credentials. Imported files remain sandbox data.

No authenticated HTTP API for these actions exists in the inspected server. [INTEGRATION.md](INTEGRATION.md) records the relevant native server engines, rules, session concurrency requirements and proposed API. Live transactions need authentication, map-thread routing or a lease shared with native login, atomic updates and durable operation receipts. This app must not write character DB tables independently. The local server returns HTTP 503 for `/api/*` to make unavailable game integration explicit.

Sites registration was attempted once on 2026-11-11 and returned `sites_access_disabled`: Sites is not enabled for this workspace. No project ID or public deployment was created. Local implementation and testing continued.

## Installed preview lifecycle

Test builds on the owner PC are kept under `~/.local/share/forever-companion-preview/releases/` with a `current` symlink and a per-build file hash manifest. The desktop/menu shortcut opens that symlink. The transient service is `forever-companion-preview-4317`; stop it with `systemctl --user stop forever-companion-preview-4317`. During updates, stop only that service after verifying its identity, switch `current`, then start the launcher again. The stable browser origin preserves sandbox data across builds.

## Verify

```sh
npm test
```

Browser smoke tests live in `tests/browser-smoke.mjs`; see its header for isolated Playwright/browser configuration. They cover desktop/mobile, recipe and shopping flows, practice mutations, reload persistence and offline behavior. Screenshots are in `tests/screenshots/`.

## Catalog provenance

`dist/data/recipes.json` includes source SHA-256 values and the in-game catalog source identity. `tools/export-recipes.py` regenerates it from the client TSVs and a read-only public `item_basic.sql`. All 1,759 referenced item labels come from humanized public SQL identifiers, with full name coverage; localized display names and item artwork from the player's DATs are not included. Base visibility is advisory; actual account era, character status, key items, inventory and skills remain server decisions.

Public reference item metadata is derived from the LandSandBoat project's `sql/item_basic.sql` as present in the local reference checkout, licensed under GPLv3; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the bundled license. The interface code and SVG icon were created for this companion. No Square Enix art, music, fonts or other game client assets are shipped.
