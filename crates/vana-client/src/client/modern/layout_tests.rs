//! Tests of the HUD's default places (theme.rs SCREEN_GAP / PANEL_GAP, mod.rs default_place): the panels as the layer
//! leaves them after a few frames, with no GPU (egui lays out and tessellates on the CPU), at the window sizes the
//! client runs at and Interface Size's extremes.

use super::shot::{EFFECTS, Opts, sample};
use super::theme::{BELOW_BAR, PANEL_GAP, SCREEN_GAP};
use super::{Layout, Modern};
use egui::{Pos2, Rect, vec2};
use std::collections::HashMap;

/// The panels' rects (points) after eight frames of the shot's sample HUD, the screen in points, the top bar's span.
fn layout_of(width: u32, height: u32, scale: u32, layout: &mut Layout) -> (HashMap<String, Rect>, Rect, Option<Rect>) {
    layout_with(width, height, scale, layout, None, 1)
}

/// `layout_of` with a shot's `--panel` drawn over the HUD (and its `--panel-row`).
fn layout_with(
    width: u32,
    height: u32,
    scale: u32,
    layout: &mut Layout,
    panel: Option<&str>,
    row: usize,
) -> (HashMap<String, Rect>, Rect, Option<Rect>) {
    let ctx = egui::Context::default();
    let m = Modern::new(&ctx);
    let zoom = m.theme.zoom(height as f32, scale);
    ctx.set_zoom_factor(zoom);
    let opts = Opts {
        chat_font: 15.0,
        font: None,
        list: false,
        rows: None,
        borders: None,
        opacity: 0.85,
        kit: false,
        panel: panel.map(str::to_string),
        panel_row: row,
        alliance: false,
        menus: false,
        favs: false,
        modes: false,
        titles: false,
        settings: false,
        gm: false,
        controller: false,
        qa: Default::default(),
    };
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32) / zoom);
    for frame in 0..8 {
        let raw = egui::RawInput { screen_rect: Some(screen), time: Some(frame as f64 * 0.5), ..Default::default() };
        let _ = ctx.run_ui(raw, |ui| {
            // Frame 0 only sets the screen (egui's first frame has its default one).
            if frame > 0 {
                sample(&m, ui.ctx(), layout, &opts, true);
            }
        });
    }
    let rects = m.rects.borrow().clone();
    let bar = m.top_bar.get().map(|b| b.0);
    (rects, screen, bar)
}

/// The panels of the HUD's default places, with few effects and with a party's worth (four rows of icons): all on the
/// screen's margin, none over another or under the top bar, the chat window ending short of the party column (it ran
/// under the party window's rows, its gear and grip over them: the user's report, 2026-11-03), the Timers' place under
/// the effect icons, and the minimap (an edge window drawn by extras.rs at its default place, up to its largest size)
/// clear of the party column wherever the screen has the room for both. The combinations with no such room are printed.
#[test]
fn default_hud_does_not_overlap() {
    let mut cramped = Vec::new();
    for (w, h) in [(1280, 800), (1920, 1080), (2560, 1440), (3840, 2160)] {
        for scale in [50, 60, 100, 150] {
            for effects in [4, 32] {
                EFFECTS.with(|n| n.set(effects));
                let (rects, screen, bar) = layout_of(w, h, scale, &mut Layout::new());
                let at = format!("{w}x{h} at {scale}% with {effects} effects");
                let bar = bar.unwrap_or_else(|| panic!("{at}: no top bar"));
                let of = |id: &str| *rects.get(id).unwrap_or_else(|| panic!("{at}: no {id} panel: {rects:?}"));
                let ids = ["chat", "where", "party", "target", "cast"];
                for id in ids {
                    let r = of(id);
                    assert!(
                        r.min.x >= screen.min.x + SCREEN_GAP - 1.0 && r.max.x <= screen.max.x - SCREEN_GAP + 1.0,
                        "{at}: {id} {r:?} off the screen's margin {screen:?}"
                    );
                    assert!(r.max.y <= screen.max.y - SCREEN_GAP + 1.0, "{at}: {id} {r:?} past the bottom margin");
                    assert!(r.min.y >= bar.max.y, "{at}: {id} {r:?} under the top bar {bar:?}");
                }
                // The effect icons have no frame: what they draw is inside their margin.
                let status = of("status").shrink2(vec2(super::MARGIN.left as f32, super::MARGIN.top as f32));
                assert!(status.min.y >= bar.max.y, "{at}: status {status:?} under the top bar {bar:?}");
                let all = ids.iter().map(|id| (*id, of(id))).chain([("status", status)]).collect::<Vec<_>>();
                for (i, (a, ra)) in all.iter().enumerate() {
                    for (b, rb) in &all[i + 1..] {
                        // Docked panels share a border: a point of it does not count.
                        assert!(!ra.shrink(1.0).intersects(rb.shrink(1.0)), "{at}: {a} {ra:?} over {b} {rb:?}");
                    }
                }
                // The EXP bar is the party window's foot (hud.rs): inside it, so over no other panel.
                let exp = of("expbar");
                assert!(
                    of("party").expand(0.5).contains_rect(exp),
                    "{at}: the EXP bar {exp:?} is outside the party window {:?}",
                    of("party")
                );
                for (id, other) in all.iter().filter(|(id, _)| *id != "party") {
                    assert!(
                        !exp.shrink(1.0).intersects(other.shrink(1.0)),
                        "{at}: the EXP bar {exp:?} over {id} {other:?}"
                    );
                }
                let (chat, party, target) = (of("chat"), of("party"), of("target"));
                assert!(
                    chat.max.x <= party.min.x - PANEL_GAP + 0.5,
                    "{at}: chat {chat:?} runs into the party window {party:?}"
                );
                // The new default leaves room for the job helper beside the party.
                assert!(chat.width() <= screen.width() * 0.54, "{at}: chat {chat:?} wider than the owner default");
                assert!(target.max.y <= party.min.y, "{at}: target {target:?} over party {party:?}");
                // The Timers (left edge, extras.rs) start under the effect icons.
                let timers = super::default_place("timers", screen.size());
                assert!(
                    timers.y * screen.height() >= status.max.y,
                    "{at}: the Timers' place {timers:?} over the effect icons {status:?}"
                );
                // The minimap: flush right under the top bar, as wide as its largest size on screen (its layout pixels are
                // screen pixels, `zoom` points each) with its heading and margins.
                let zoom = w as f32 / screen.width();
                let d = super::super::minimap::SIZES[2] / zoom;
                let mini = Rect::from_min_max(
                    egui::pos2(screen.max.x - d - 30.0, BELOW_BAR),
                    egui::pos2(screen.max.x, BELOW_BAR + d + 70.0),
                );
                let place = super::default_place("minimap", screen.size());
                assert!(
                    place.y * screen.height() >= bar.max.y,
                    "{at}: the minimap's place {place:?} under the top bar {bar:?}"
                );
                let column = target.union(party);
                let room = screen.max.y - SCREEN_GAP - mini.max.y;
                if column.height() + PANEL_GAP <= room {
                    assert!(!mini.intersects(column), "{at}: minimap {mini:?} over the right column {column:?}");
                } else {
                    cramped.push(format!("{at}: column {:.0} high, room under the minimap {room:.0}", column.height()));
                }
            }
        }
    }
    EFFECTS.with(|n| n.set(0));
    println!(
        "no room for the minimap and the five-member party's column together ({}):\n{}",
        cramped.len(),
        cramped.join("\n")
    );
}

/// The EXP bar (hud.rs party_panel, the party window's foot; the shot's sample has it, with `--panel expbar` also the
/// limit points' second row) stands inside the party window and clear of every other panel, the top bar and the screen's
/// margins, at every window size and Interface Size: it is part of the window, so the window's exact height must hold it.
#[test]
fn exp_bar_stays_in_the_party_window() {
    for (w, h) in [(1280, 800), (1920, 1080), (2560, 1440), (3840, 2160)] {
        for scale in [50, 60, 100, 150] {
            for row in [0, 1] {
                EFFECTS.with(|n| n.set(4));
                let (rects, screen, bar) = layout_with(w, h, scale, &mut Layout::new(), Some("expbar"), row);
                let at = format!("{w}x{h} at {scale}% row {row}");
                let bar = bar.unwrap_or_else(|| panic!("{at}: no top bar"));
                let of = |id: &str| *rects.get(id).unwrap_or_else(|| panic!("{at}: no {id} panel: {rects:?}"));
                let (exp, party) = (of("expbar"), of("party"));
                // One row (33 points) for the EXP, two with the limit points.
                let rows = if row == 1 { 2.0 } else { 1.0 };
                assert!((exp.height() - 33.0 * rows).abs() < 0.5, "{at}: the bar is {} high", exp.height());
                assert!(party.expand(0.5).contains_rect(exp), "{at}: bar {exp:?} outside the party window {party:?}");
                assert!(
                    exp.max.y <= screen.max.y - SCREEN_GAP + 1.0 && exp.min.y >= bar.max.y,
                    "{at}: bar {exp:?} off the screen's margin {screen:?} or under the top bar {bar:?}"
                );
                for id in ["chat", "target", "cast", "where"] {
                    let other = of(id);
                    assert!(!exp.shrink(1.0).intersects(other.shrink(1.0)), "{at}: bar {exp:?} over {id} {other:?}");
                }
                // The window grew for it: still clear of the target window above and the chat window beside it.
                assert!(
                    of("target").max.y <= party.min.y && of("chat").max.x <= party.min.x - PANEL_GAP + 0.5,
                    "{at}: the party window {party:?} with the bar runs into the target or chat window"
                );
                assert!(party.min.y >= bar.max.y, "{at}: party {party:?} under the top bar {bar:?}");
            }
        }
    }
    EFFECTS.with(|n| n.set(0));
}

/// What a player saved stays theirs, kept on the screen and, for the chat window, clear of the party window and under
/// the top bar.
#[test]
fn saved_places_stay_on_screen() {
    use crate::config::ModernPlace as Place;
    for (w, h, scale) in [(1280, 800, 100), (1920, 1080, 60), (3840, 2160, 150)] {
        let mut layout = Layout::new();
        // A chat window saved wide and tall (a bigger window, a smaller Interface Size), the party window off the right
        // edge.
        layout.insert("chat".into(), Place { x: 0.02, b: 0.99, w: 6000.0, h: 2000.0, ..Default::default() });
        layout.insert("party".into(), Place { x: 0.98, b: 0.97, ..Default::default() });
        let (rects, screen, bar) = layout_of(w, h, scale, &mut layout);
        let at = format!("{w}x{h} at {scale}%");
        let (chat, party) = (rects["chat"], rects["party"]);
        assert!(
            party.max.x <= screen.max.x + 1.0 && party.min.x >= screen.min.x,
            "{at}: party {party:?} off the screen {screen:?}"
        );
        assert!(chat.max.x <= screen.max.x + 1.0, "{at}: chat {chat:?} off the screen {screen:?}");
        assert!(chat.max.x <= party.min.x - PANEL_GAP + 0.5, "{at}: chat {chat:?} under party {party:?}");
        assert!(chat.min.y >= bar.unwrap().max.y - 1.0, "{at}: chat {chat:?} up under the top bar");
        // The place saved is kept (not rewritten by the clamp).
        assert_eq!(layout["chat"].w, 6000.0, "{at}");
        assert_eq!(layout["chat"].h, 2000.0, "{at}");
        assert_eq!(layout["party"].x, 0.98, "{at}");
    }
}

/// The map window's buttons (mapwin.rs, in place of retail's map0 and mapscan sprites, which stood over the Network
/// box) stand clear of the whole top bar, the Network box above all, at every window size and Interface Size, for each
/// of its windows, and keep the screen's right margin. Overlaps with the panels the HUD keeps up while the map is open
/// are printed, as the minimap's are.
#[test]
fn map_buttons_avoid_the_top_bar_and_network() {
    let mut crowded = Vec::new();
    for (w, h) in [(1280, 800), (1920, 1080), (2560, 1440), (3840, 2160)] {
        for scale in [50, 60, 100, 150, 200] {
            for row in 0..3 {
                EFFECTS.with(|n| n.set(4));
                let (rects, screen, bar) = layout_with(w, h, scale, &mut Layout::new(), Some("map"), row);
                let at = format!("{w}x{h} at {scale}% row {row}");
                let bar = bar.unwrap_or_else(|| panic!("{at}: no top bar"));
                let r = *rects.get(super::mapwin::ID).unwrap_or_else(|| panic!("{at}: no map buttons: {rects:?}"));
                let net = super::topbar::network_rect(screen);
                assert!(!r.intersects(net), "{at}: map buttons {r:?} over the Network box {net:?}");
                assert!(
                    r.min.y >= bar.max.y && r.min.y >= net.max.y,
                    "{at}: map buttons {r:?} not under the top bar {bar:?} and the Network box {net:?}"
                );
                assert!(
                    r.max.x <= screen.max.x - SCREEN_GAP + 1.0 && r.min.x >= screen.min.x,
                    "{at}: map buttons {r:?} off the screen's margin {screen:?}"
                );
                assert!(r.max.y <= screen.max.y - SCREEN_GAP, "{at}: map buttons {r:?} past the bottom margin");
                for (id, other) in rects.iter().filter(|(id, _)| id.as_str() != super::mapwin::ID) {
                    if r.shrink(1.0).intersects(other.shrink(1.0)) {
                        // The right column and the chat window the buttons step aside for.
                        assert!(
                            !matches!(id.as_str(), "party" | "target" | "chat"),
                            "{at}: map buttons {r:?} over {id} {other:?}"
                        );
                        crowded.push(format!("{at}: over {id} {other:?}"));
                    }
                }
            }
        }
    }
    EFFECTS.with(|n| n.set(0));
    println!("map buttons over another panel ({}):\n{}", crowded.len(), crowded.join("\n"));
}

#[test]
fn owner_layout_defaults_and_saved_customization() {
    for (w, h, scale) in [(1280, 800, 60), (1920, 1080, 100), (2560, 1440, 100)] {
        let mut places = Layout::new();
        let (r, screen, _) = layout_with(w, h, scale, &mut places, Some("owner-layout"), 0);
        for id in ["chat", "where", "party", "widescan", "quest tracker", "minimap", "navigation"] {
            assert!(screen.expand(1.0).contains_rect(r[id]), "{w} {id}: {:?} outside {screen:?}", r[id]);
        }
        assert!(r["chat"].right() + PANEL_GAP - 1.0 <= r["widescan"].left());
        assert!(r["widescan"].right() + PANEL_GAP - 1.0 <= r["party"].left());
        assert!(r["quest tracker"].top() >= r["minimap"].bottom() + PANEL_GAP - 1.0);
        assert!(r["quest tracker"].bottom() <= r["party"].top());
        assert!(r["navigation"].center().x > screen.width() * 0.4 && r["navigation"].center().x < screen.width() * 0.6);
        // Rendering defaults must not write placements into the saved character layout.
        assert!(places.is_empty(), "default render wrote user settings: {places:?}");
        let saved:Layout=serde_json::from_str(r#"{"quest tracker":{"x":0.03,"y":0.24},"widescan":{"x":0.12,"y":0.42,"float":true,"w":310},"chat":{"x":0.01,"b":0.98,"w":550,"h":160},"navigation":{"x":0.35,"y":0.05}}"#).unwrap();
        let mut custom = saved.clone();
        let (r, _, _) = layout_with(w, h, scale, &mut custom, Some("owner-layout"), 0);
        assert_eq!(custom, saved, "render rewrote saved customization");
        assert!(r["quest tracker"].left() < screen.width() * 0.1);
        assert!(r["widescan"].left() < screen.width() * 0.2);
    }
}

#[test]
fn tracker_avoids_every_party_size_without_rewriting_saved_layout() {
    for (w, h, scale) in [(1280, 800, 60), (1920, 1080, 100), (2560, 1440, 100)] {
        for n in 1..=6 {
            for panel in ["party-layout", "party-layout-long"] {
                for saved in [r#"{}"#, r#"{"quest tracker":{"x":0.75,"y":0.60,"w":360,"h":280}}"#] {
                    let original: Layout = serde_json::from_str(saved).unwrap();
                    let mut places = original.clone();
                    let (r, screen, _) = layout_with(w, h, scale, &mut places, Some(panel), n);
                    let tracker = r["quest tracker"];
                    assert!(
                        screen.expand(1.0).contains_rect(tracker),
                        "{w}/{scale} {n} {panel}: {tracker:?} outside {screen:?}"
                    );
                    for id in ["party", "minimap", "widescan", "chat"] {
                        assert!(
                            !tracker.shrink(1.0).intersects(r[id].shrink(1.0)),
                            "{w}/{scale} {n} {panel} saved={saved}: tracker {tracker:?} over {id} {:?}",
                            r[id]
                        );
                    }
                    assert_eq!(places, original, "automatic adjustment persisted settings");
                }
            }
        }
    }
}

#[test]
fn tracker_reacts_to_party_join_and_leave_in_the_same_frame() {
    let ctx = egui::Context::default();
    let m = Modern::new(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0));
    let mut layout = Layout::new();
    let mut solo = None;
    for frame in 0..22 {
        let n = if (8..15).contains(&frame) { 6 } else { 1 };
        let opts = Opts {
            chat_font: 15.0,
            font: None,
            list: false,
            rows: None,
            borders: None,
            opacity: 0.85,
            kit: false,
            panel: Some("party-layout".into()),
            panel_row: n,
            alliance: false,
            menus: false,
            favs: false,
            modes: false,
            titles: false,
            settings: false,
            gm: false,
            controller: false,
            qa: Default::default(),
        };
        let _ = ctx.run_ui(
            egui::RawInput { screen_rect: Some(screen), time: Some(frame as f64 * 0.5), ..Default::default() },
            |ui| {
                if frame > 0 {
                    sample(&m, ui.ctx(), &mut layout, &opts, true);
                }
            },
        );
        if frame < 3 {
            continue;
        }
        let rects = m.rects.borrow();
        let tracker = rects["quest tracker"];
        assert!(
            !tracker.shrink(1.0).intersects(rects["party"].shrink(1.0)),
            "frame {frame}: {tracker:?} over {:?}",
            rects["party"]
        );
        if frame == 7 {
            solo = Some(tracker);
        }
        if frame == 21 {
            assert!(tracker.min.distance(solo.unwrap().min) < 1.0, "tracker failed to return after party shrank");
        }
        assert!(layout.is_empty());
    }
}

/// The area (points squared) the HUD panels take of the screen, summed over `ids`.
fn area(rects: &HashMap<String, Rect>, ids: &[&str]) -> f32 {
    ids.iter().map(|id| rects[*id]).map(|r| r.width() * r.height()).sum()
}

/// The panels of the owner-layout HUD that the Compact HUD tightens (the edge windows, the minimap and the job helper,
/// keep their own sizes) and the whole set.
const TIGHTENED: [&str; 4] = ["chat", "where", "party", "quest tracker"];
const HUD_SET: [&str; 7] = ["chat", "where", "party", "widescan", "quest tracker", "minimap", "navigation"];

/// The Compact HUD at the sizes the client runs at: the same HUD with the same guarantees (nothing off the screen's
/// margin, over another panel, or under the top bar; the EXP bar in the party window; the chat window short of the party
/// column), and a measurably smaller footprint than today's.
#[test]
fn compact_hud_does_not_overlap_and_takes_less_room() {
    let (mut worst, mut worst_all) = (f32::MAX, f32::MAX);
    for (w, h, scale) in [(1280, 800, 60), (1920, 1080, 100), (2560, 1440, 100)] {
        for effects in [4, 32] {
            EFFECTS.with(|n| n.set(effects));
            super::set_compact(false);
            let (full, _, _) = layout_with(w, h, scale, &mut Layout::new(), Some("owner-layout"), 0);
            super::set_compact(true);
            let (rects, screen, bar) = layout_with(w, h, scale, &mut Layout::new(), Some("owner-layout"), 0);
            let at = format!("{w}x{h} at {scale}% with {effects} effects");
            let _ = bar;
            let of = |id: &str| *rects.get(id).unwrap_or_else(|| panic!("{at}: no {id} panel: {rects:?}"));
            for id in HUD_SET {
                let r = of(id);
                assert!(screen.expand(1.0).contains_rect(r), "{at}: {id} {r:?} outside {screen:?}");
                assert!(r.min.y >= super::theme::BELOW_BAR - 1.0, "{at}: {id} {r:?} up under the top bar's room");
            }
            for (i, a) in HUD_SET.iter().enumerate() {
                for b in &HUD_SET[i + 1..] {
                    assert!(
                        !of(a).shrink(1.0).intersects(of(b).shrink(1.0)),
                        "{at}: compact {a} {:?} over {b} {:?}",
                        of(a),
                        of(b)
                    );
                }
            }
            assert!(of("party").expand(0.5).contains_rect(of("expbar")), "{at}: the EXP bar left the party window");
            assert!(of("chat").max.x <= of("party").min.x - PANEL_GAP + 0.5, "{at}: chat runs into the party window");
            // Smaller than today's (the full layout), panel by panel it tightens and in total.
            for id in TIGHTENED {
                let (c, f) = (rects[id], full[id]);
                assert!(
                    c.width() * c.height() < f.width() * f.height(),
                    "{at}: compact {id} {c:?} is not smaller than {f:?}"
                );
            }
            for id in HUD_SET {
                println!(
                    "AUDIT {w}x{h}@{scale} fx{effects} {id}: today {:.0}x{:.0}={:.0} compact {:.0}x{:.0}={:.0}",
                    full[id].width(),
                    full[id].height(),
                    full[id].area(),
                    rects[id].width(),
                    rects[id].height(),
                    rects[id].area()
                );
            }
            let ratio = area(&rects, &TIGHTENED) / area(&full, &TIGHTENED);
            println!(
                "{at}: tightened panels {:.0}% of today's; whole HUD {:.0}%",
                ratio * 100.0,
                area(&rects, &HUD_SET) / area(&full, &HUD_SET) * 100.0
            );
            worst = worst.min(1.0 - ratio);
            worst_all = worst_all.min(1.0 - area(&rects, &HUD_SET) / area(&full, &HUD_SET));
            // The always-shown panels keep their size: the target and the navigation arrow are not made smaller to fit.
            assert!(of("navigation").width() >= full["navigation"].width() - 0.5, "{at}: navigation shrank");
        }
    }
    EFFECTS.with(|n| n.set(0));
    super::set_compact(false);
    // At least 12% less room over today's for the panels it tightens and 10% for the whole HUD set (measured 2026-11-09
    // on this layout: 15% and 13%).
    assert!(worst >= 0.12, "the tightened panels save only {:.1}%", worst * 100.0);
    assert!(worst_all >= 0.10, "the whole HUD saves only {:.1}%", worst_all * 100.0);
}

/// Off restores today's layout exactly, and a saved placement or size is never moved or rewritten by the Compact HUD.
#[test]
fn compact_hud_off_is_todays_layout_and_saved_places_hold() {
    super::set_compact(false);
    let (off, _, _) = layout_with(1920, 1080, 100, &mut Layout::new(), Some("owner-layout"), 0);
    super::set_compact(true);
    let saved: Layout = serde_json::from_str(
        r#"{"quest tracker":{"x":0.03,"y":0.24,"w":410,"h":220},"chat":{"x":0.01,"b":0.98,"w":700,"h":200}}"#,
    )
    .unwrap();
    let mut custom = saved.clone();
    let (on, _, _) = layout_with(1920, 1080, 100, &mut custom, Some("owner-layout"), 0);
    assert_eq!(custom, saved, "the Compact HUD rewrote a saved placement");
    // The saved sizes are the player's: the tracker keeps its width and the chat its width, compact or not.
    super::set_compact(false);
    let (kept, _, _) = layout_with(1920, 1080, 100, &mut saved.clone(), Some("owner-layout"), 0);
    for id in ["quest tracker", "chat"] {
        assert!((on[id].width() - kept[id].width()).abs() < 1.5, "{id} width {:?} vs {:?}", on[id], kept[id]);
        assert_eq!(on[id].min.x, kept[id].min.x, "{id} was moved");
    }
    assert!(on["quest tracker"].width() > 400.0 && on["chat"].width() > 690.0);
    super::set_compact(false);
    let (again, _, _) = layout_with(1920, 1080, 100, &mut Layout::new(), Some("owner-layout"), 0);
    for id in HUD_SET {
        assert_eq!(again[id], off[id], "{id} differs once the Compact HUD is turned off again");
    }
}

/// Folding: each foldable panel shrinks to its title bar (the party window to its own row), the Minimal HUD folds them
/// all, and unfolding brings each back exactly; the panels that must stay shown never fold.
#[test]
fn folds_round_trip_and_the_always_shown_panels_stay() {
    use super::shot::SAMPLE_FOLDS;
    let drawn = |folds: &[&str]| {
        SAMPLE_FOLDS.with(|f| *f.borrow_mut() = folds.iter().map(|k| (k.to_string(), true)).collect());
        layout_with(1920, 1080, 100, &mut Layout::new(), Some("owner-layout"), 0).0
    };
    super::set_compact(true);
    let open = drawn(&[]);
    for (id, fold) in [("quest tracker", "quest tracker"), ("party", "party"), ("chat", "chat")] {
        let folded = drawn(&[fold]);
        assert!(folded[id].height() < open[id].height() - 8.0, "{id} did not fold: {:?} / {:?}", folded[id], open[id]);
        assert!(folded[id].height() > 20.0, "{id} folded away entirely: {:?}", folded[id]);
        // Unfolded again, it is what it was.
        let back = drawn(&[]);
        assert_eq!(back[id], open[id], "{id} changed after fold and unfold");
    }
    // The party window folds to its own row: HP, MP and TP stay (its title and one row are all it keeps).
    let party = drawn(&["party"])["party"];
    assert!(party.height() > 80.0, "a folded party window keeps our own row: {party:?}");
    // The Minimal HUD folds all of them at once and shows none of what it must keep folded.
    let minimal = drawn(&[super::MINIMAL_KEY]);
    for id in ["quest tracker", "party", "chat"] {
        assert!(minimal[id].height() < open[id].height() - 8.0, "{id} stayed open in the Minimal HUD");
    }
    // The room the Minimal HUD gives back, over the Compact HUD's open layout and over today's.
    super::set_compact(false);
    let today = layout_with(1920, 1080, 100, &mut Layout::new(), Some("owner-layout"), 0).0;
    super::set_compact(true);
    let (open_area, min_area, today_area) = (area(&open, &HUD_SET), area(&minimal, &HUD_SET), area(&today, &HUD_SET));
    println!(
        "MINIMAL 1920x1080: open compact {:.0}% of today's, Minimal HUD {:.0}% of today's",
        open_area / today_area * 100.0,
        min_area / today_area * 100.0
    );
    assert!(min_area < today_area * 0.7, "the Minimal HUD gives back too little: {min_area} of {today_area}");
    // Stop (inside the navigation panel) never folds, in any fold state; neither does the target (the default HUD).
    assert!(minimal.contains_key("navigation"), "navigation left the HUD in the Minimal HUD: {minimal:?}");
    assert_eq!(minimal["navigation"].size(), open["navigation"].size(), "navigation changed size in the Minimal HUD");
    SAMPLE_FOLDS.with(|f| f.borrow_mut().clear());
    let target_open = layout_with(1920, 1080, 100, &mut Layout::new(), None, 1).0;
    SAMPLE_FOLDS.with(|f| *f.borrow_mut() = [(super::MINIMAL_KEY.to_string(), true)].into());
    let target_min = layout_with(1920, 1080, 100, &mut Layout::new(), None, 1).0;
    assert_eq!(target_min["target"].size(), target_open["target"].size(), "the target changed in the Minimal HUD");
    assert!(target_min.contains_key("cast"), "the cast bar left the Minimal HUD");
    // Folds are ignored while the Compact HUD is off (Off restores today's layout, folds kept in the config).
    super::set_compact(false);
    let off = drawn(&["quest tracker", "party", "chat", super::MINIMAL_KEY]);
    super::set_compact(false);
    let plain = drawn(&[]);
    for id in HUD_SET {
        assert_eq!(off[id], plain[id], "{id}: folds applied with the Compact HUD off");
    }
    SAMPLE_FOLDS.with(|f| f.borrow_mut().clear());
}

/// The fold map's rules: a panel's button toggles its own fold; the Minimal HUD key folds every panel (only while the
/// Compact HUD is on) and back as each was; the button on a panel while the Minimal HUD is on opens just that panel, as an
/// exception that ends with the Minimal HUD and never changes a panel's own fold.
#[test]
fn fold_map_rules() {
    use super::{FOLD_IDS, folded_in, minimal_on, toggle_fold, toggle_minimal};
    use std::collections::BTreeMap;
    let mut f: BTreeMap<String, bool> = BTreeMap::new();
    assert!(!folded_in(&f, "party", true));
    toggle_fold(&mut f, "party");
    assert!(folded_in(&f, "party", true) && folded_in(&f, "party", false));
    assert!(!folded_in(&f, "chat", true));
    toggle_fold(&mut f, "party");
    assert!(!folded_in(&f, "party", true));
    // The Minimal HUD: every panel folded only while the Compact HUD is on; the own folds stay what they were.
    toggle_fold(&mut f, "chat");
    toggle_minimal(&mut f);
    assert!(minimal_on(&f));
    for id in FOLD_IDS {
        assert!(folded_in(&f, id, true), "{id}");
    }
    assert!(!folded_in(&f, "party", false), "no Minimal HUD without the Compact HUD");
    toggle_minimal(&mut f);
    assert!(!minimal_on(&f) && !folded_in(&f, "party", true) && folded_in(&f, "chat", true));
    // The button on a panel while the Minimal HUD is on: that panel only opens, the rest stay folded; a second press folds it
    // again; the exception and the Minimal HUD end together, and every own fold is as it was.
    toggle_minimal(&mut f);
    toggle_fold(&mut f, "quest tracker");
    assert!(minimal_on(&f));
    assert!(!folded_in(&f, "quest tracker", true));
    for id in FOLD_IDS.iter().filter(|i| **i != "quest tracker") {
        assert!(folded_in(&f, id, true), "{id} should stay folded");
    }
    toggle_fold(&mut f, "quest tracker");
    assert!(folded_in(&f, "quest tracker", true));
    toggle_fold(&mut f, "party");
    assert!(!folded_in(&f, "party", true));
    toggle_minimal(&mut f);
    assert!(!minimal_on(&f) && !f.keys().any(|k| k.starts_with("minimal open:")), "{f:?}");
    assert!(!folded_in(&f, "party", true) && !folded_in(&f, "quest tracker", true) && folded_in(&f, "chat", true));
    assert!(!f.get("party").copied().unwrap_or(false), "the Minimal HUD changed an own fold");
}

/// An NPC talking (a message waiting for a key) and a message being typed are never folded away, whatever the chat window's
/// fold and the Minimal HUD say; new lines otherwise stay hidden while it is folded (the tabs still count them).
#[test]
fn a_folded_chat_still_shows_an_npc_waiting_for_a_key() {
    use super::shot::{SAMPLE_FOLDS, SAMPLE_WAITING};
    super::set_compact(true);
    let drawn = |folds: &[&str], waiting: bool| {
        SAMPLE_FOLDS.with(|f| *f.borrow_mut() = folds.iter().map(|k| (k.to_string(), true)).collect());
        SAMPLE_WAITING.with(|w| w.set(waiting));
        layout_with(1920, 1080, 100, &mut Layout::new(), Some("owner-layout"), 0).0
    };
    let open = drawn(&[], false)["chat"];
    let folded = drawn(&["chat"], false)["chat"];
    assert!(folded.height() < open.height() - 8.0, "the chat did not fold: {folded:?} / {open:?}");
    for fold in [vec!["chat"], vec![super::MINIMAL_KEY]] {
        let waiting = drawn(&fold, true)["chat"];
        assert_eq!(waiting, open, "a folded chat hid an NPC waiting for a key ({fold:?})");
    }
    SAMPLE_FOLDS.with(|f| f.borrow_mut().clear());
    SAMPLE_WAITING.with(|w| w.set(false));
    super::set_compact(false);
}

/// Stop Navigation (the navigation panel's running state, `--panel navigation --panel-row 4`) is as it was in the Minimal
/// HUD: the panel is neither folded nor moved nor resized, and the tracker above it folds.
#[test]
fn stop_navigation_stays_in_the_minimal_hud() {
    use super::shot::SAMPLE_FOLDS;
    super::set_compact(true);
    SAMPLE_FOLDS.with(|f| f.borrow_mut().clear());
    let open = layout_with(1920, 1080, 100, &mut Layout::new(), Some("navigation"), 4).0;
    SAMPLE_FOLDS.with(|f| *f.borrow_mut() = [(super::MINIMAL_KEY.to_string(), true)].into());
    let minimal = layout_with(1920, 1080, 100, &mut Layout::new(), Some("navigation"), 4).0;
    assert!(open["navigation"].height() > 60.0, "the running navigation panel is missing: {:?}", open["navigation"]);
    assert_eq!(minimal["navigation"], open["navigation"], "the navigation panel changed in the Minimal HUD");
    assert!(minimal["quest tracker"].height() < open["quest tracker"].height() - 8.0, "the tracker did not fold");
    SAMPLE_FOLDS.with(|f| f.borrow_mut().clear());
    super::set_compact(false);
}

/// Turning the Compact HUD off brings back everything: panels the Minimal HUD folded and one a chevron opened under it
/// leave nothing folded (the edge windows' own folds are the player's own and are not touched).
#[test]
fn compact_off_after_the_minimal_hud_restores_every_panel() {
    use super::shot::SAMPLE_FOLDS;
    use super::{minimal_on, toggle_fold, toggle_minimal};
    let mut folds = std::collections::BTreeMap::new();
    toggle_minimal(&mut folds);
    toggle_fold(&mut folds, "quest tracker");
    assert!(minimal_on(&folds));
    SAMPLE_FOLDS.with(|f| *f.borrow_mut() = folds);
    super::set_compact(false);
    let off = layout_with(1920, 1080, 100, &mut Layout::new(), Some("owner-layout"), 0).0;
    SAMPLE_FOLDS.with(|f| f.borrow_mut().clear());
    let plain = layout_with(1920, 1080, 100, &mut Layout::new(), Some("owner-layout"), 0).0;
    for id in HUD_SET {
        assert_eq!(off[id], plain[id], "{id} stayed folded after the Compact HUD went off");
    }
}
