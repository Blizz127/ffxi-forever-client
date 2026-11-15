# Built-in Extras and the upstream mods

Upstream (lenonk/vana-client, "Extras as mods", Nov 2026) turned the client's Extras into Lua mods and stopped
bundling mods with the client. This fork (Forever and classic alike) keeps those Extras built in, as before:
Text Auto-Advance, Trust Speech and Ready Lines, `/find`, Map Overlay (Ctrl/Alt+O), Clock, Target Distance / Target
HP %, Battle Text and Battle Names, Timestamps on All Lines, Synthesis Amount, Timers, Training Regime, Skillchain
Helper, the Minimap (radar, mouse pin, home points), the job-panel slot, the `!hpmap` feed, Pending Actions and the
battle menu's Battle Suggestions. Their settings keys and defaults are the old ones.

**Duplicate rule: the built-in Extra wins.** `client/lua/mod.rs` `BUILT_IN` lists the standard mods that are
upstream's conversions of these; a mod with one of those ids is never started (it logs "the client has this Extra
built in"), so nothing is drawn or done twice, and `client/modsui.rs` does not offer them in Download Standard
Mods. Every other upstream mod, the mod API, the DAT Packs and Mods pages and the catalogue work as upstream's.

Mods may still use the native hooks upstream added (`ui.native.slot`, `actions.synth`, `filters.battle`, edge-window
stand-ins for panels the fork keeps); a mod that is not one of the built-in ids is unaffected.

**The DPS meter (`dps`) is carried in the binary**, as on develop (`lua/mod.rs` `IN_BINARY`): it runs unless
turned off in the config's `mods` or replaced by an enabled folder of the same name in the mods folder (a downloaded copy
replaces it, the binary's copy is then not started; a mod's id is loaded once). It is the current assets/mods/dps, whose
`/dps` is develop's and whose new Damage Meter window setting is off by default.
