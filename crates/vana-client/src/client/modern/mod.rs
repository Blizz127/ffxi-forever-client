//! The modern UI (docs/modern-ui.md; Settings > Client > Interface): an egui layer over the world, drawn with the
//! modern asset pack (assets/modern) and the bundled fonts, switched on at run time. The retail windows it replaces
//! are not drawn while it is on; the rest of the retail UI stays.
//!
//! Each panel is an egui Area that a drag on its background moves (its contents keep their clicks), kept by character
//! as screen fractions. The chat window also resizes by its corner grip: more room for the lines, nothing scaled.

mod auction;
mod bluset;
mod borders;
mod charlnk;
pub(super) mod chat;
mod chatsel;
mod combat;
mod command;
mod config;
mod craft;
mod ctxmenu;
mod cutscene;
mod equip;
mod era;
mod era_shot;
mod expbar;
mod extras;
mod favs;
mod game_menu;
mod gametext;
mod gearui;
mod gmwin;
mod hud;
mod hudmods;
mod inspect;
mod items;
mod jobpanel;
pub(super) mod journal;
mod keyboard;
mod kit;
#[cfg(test)]
mod layout_tests;
mod lists;
mod lobby;
mod loot;
mod macros;
mod mapwin;
mod maze;
mod modwin;
mod navigation;
mod options;
mod padwin;
mod pupset;
mod qtrack;
mod regioninfo;
mod tracker_layout;
mod training;
pub use modwin::Click as ModClick;
mod minor;
mod modes;
mod paint;
mod pending;
pub mod plates;
mod points;
mod rows;
mod search;
pub use search::SEARCH_LISTS;
mod post;
mod shop;
pub mod shot;
mod social;
mod status;
mod storage;
mod tabs;
mod theme;

/// The pack's names for mods (lua/skin.rs).
pub(in crate::client) mod pack {
    pub fn sprites() -> &'static std::collections::BTreeMap<String, [f32; 2]> {
        super::theme::pack_sprites()
    }
    pub fn colours() -> &'static Vec<String> {
        super::theme::pack_colours()
    }
    pub fn frames() -> &'static Vec<(String, f32, f32)> {
        super::theme::pack_frames()
    }
}
mod topbar;

use super::World;
use super::game_ui::GameUi;
pub use command::{BarFocus, CmdMenu, bar_focus, key as command_key};
pub use config::find_open as config_find_open;
pub use config::page_index as config_page_index;
pub use config::{
    find_events as config_find_events, finding as config_finding, key as config_key, lobby_key as lobby_config_key,
    lobby_settings as lobby_config_settings, open_mod_settings,
};
use egui::{Color32, Context, FontFamily, FontId, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Ui, Vec2, pos2, vec2};
pub use gmwin::{events as gm_events, takes_keys as gm_takes_keys};
pub use journal::key as journal_key;
pub use lists::key as magic_key;
pub use macros::key as macro_key;
pub use options::world_key as options_key;
pub use padwin::{events as pad_events, takes_keys as pad_takes_keys};
pub use status::key as status_key;
pub use storage::key as storage_key;
use theme::{Art, LABEL, NAME, STRONG};
pub use theme::{Theme, frame_image, sprite_pixels};

pub use crate::config::{ModernDock as Dock, ModernLayout as Layout, ModernPlace as Place};
use std::collections::HashMap;

pub struct Modern {
    pub theme: Theme,
    art: Art,
    /// The chat window's tab and whether its settings are open; game textures (status icons) as egui textures, by key.
    chat_tab: std::cell::Cell<u8>,
    chat_settings: std::cell::Cell<bool>,
    /// Compact HUD: the chat window's fold chevron was clicked this frame, and whether the window is folded.
    chat_fold_click: std::cell::Cell<bool>,
    chat_folded: std::cell::Cell<bool>,
    /// Where each chat tab was last seen (the log's length then; chat.rs unread).
    chat_seen: std::cell::RefCell<[Option<usize>; chat::TABS.len()]>,
    /// The login form's text size (lobby.rs), for the lobby's popups.
    lobby_text: std::cell::Cell<f32>,
    /// The windows' background opacity (config `modern_opacity`).
    opacity: std::cell::Cell<f32>,
    /// The panel a drag has really moved (show): a press alone moves nothing.
    dragging: std::cell::RefCell<Option<String>>,
    /// Natural tracker content height, independent of temporary overlap/scroll limits.
    tracker_height: std::cell::Cell<f32>,
    /// How menu rows look (rows.rs), and window borders (borders.rs).
    row_style: std::cell::Cell<rows::Style>,
    border_style: std::cell::Cell<borders::Style>,
    images: std::cell::RefCell<std::collections::HashMap<String, Option<egui::TextureHandle>>>,
    /// Status tooltips' name and description by (status id, small set), read once (hud.rs).
    pub status_tips: std::cell::RefCell<HashMap<(u16, bool), std::rc::Rc<(String, String)>>>,
    /// Macro lines and the spell or ability each names (textcmd::line_action), for greying the macro keys.
    macro_acts: std::cell::RefCell<HashMap<String, Option<(super::magic::Kind, u16)>>>,
    /// The grid text ("(E-8)") and when it was made: rebuilt every half second, as retail's every 30 ticks.
    grid: std::cell::RefCell<(Option<std::time::Instant>, String)>,
    marquee: std::cell::RefCell<topbar::Marquee>,
    /// Last frame's panel rects (what docked panels sit on before their target draws), a panel being moved or resized
    /// (the chat window then shows its full room, something to dock on), and the chat window's rect and fade for the
    /// game's stacked windows.
    rects: std::cell::RefCell<HashMap<String, Rect>>,
    arranging: std::cell::Cell<bool>,
    chat_edge: std::cell::Cell<Option<(Rect, bool)>>,
    zoom: f32,
    /// When the Wide Scan tab last asked for a list (retail asks only as the map opens; the tab keeps it fresh).
    scan_at: std::cell::Cell<Option<std::time::Instant>>,
    /// The last complete Wide Scan list (shown while the next one streams in, then swapped at once) and where it was
    /// asked from; the row picked (its options open, ringed on the minimap); Narrow down range: waiting for a click on
    /// the minimap, then the area kept (world x, z, radius).
    scan_rows: std::cell::RefCell<Vec<(super::widescan::Entry, String)>>,
    scan_origin: std::cell::Cell<(f32, f32)>,
    scan_pick: std::cell::Cell<Option<u16>>,
    scan_picking: std::cell::Cell<bool>,
    scan_area: std::cell::Cell<Option<(f32, f32, f32)>>,
    /// The job panel's rows and when they were taken (rebuilt a few times a second, jobpanel.rs).
    job_view: std::cell::RefCell<Option<(std::time::Instant, super::jobpanel::View)>>,
    /// Captions read from the game's name sprites (skills, jobs, ranks), by library and index.
    captions: std::cell::RefCell<HashMap<(String, usize), Option<String>>>,
    /// The full log's wheel, in rows not yet scrolled.
    wheel: std::cell::Cell<f32>,
    /// Where charlnk opens (beside the full log at the clicked line).
    charlnk_at: std::cell::Cell<Option<Pos2>>,
    /// The Status page's currencies: only those held.
    held_only: std::cell::Cell<bool>,
    /// The Journal's tab and cursor last drawn (a moved cursor is scrolled into view), and the quest list's line of
    /// each quest (headings between them).
    journal_seen: std::cell::Cell<(u8, usize)>,
    journal_rows: std::cell::RefCell<Vec<usize>>,
    /// egui's text styles enlarged (put back when the layer goes off).
    styled: bool,
    /// The mods' look (ui.skin) as last applied, and the panels drawn scaled (their layers' transforms).
    skin_ver: std::cell::Cell<u64>,
    scaled: std::cell::RefCell<std::collections::HashSet<String>>,
    /// Where the panels a mod hid would stand this frame (their last size), for mods that replace them, and each
    /// panel's size as last drawn.
    ghosts: std::cell::RefCell<HashMap<String, Rect>>,
    sizes: std::cell::RefCell<HashMap<String, Vec2>>,
    /// An edge window's heading dragged (or let go) this frame.
    edge_grip: std::cell::Cell<Option<extras::EdgeDrag>>,
    /// The edge window being dragged and where the pointer holds it (its ghost follows the pointer).
    edge_ghost: std::cell::RefCell<Option<(String, Vec2)>>,
    /// The top bar this frame: the span its boxes take and how far faded in (the top edge's windows sit under it).
    top_bar: std::cell::Cell<Option<(Rect, f32)>>,
    /// The skillchain helper's chains shown with all their ways.
    sc_open: std::cell::RefCell<std::collections::HashSet<u8>>,
    /// Each keyboard list's cursor last frame (`moved`).
    cursors: std::cell::RefCell<HashMap<&'static str, usize>>,
    /// The chat window's mouse selection (chatsel.rs): over the lines and its text this frame, the chat line's caret /
    /// anchor asked, whether the line has a selection, and the right-click menu and its pick.
    log_sel: std::cell::RefCell<Option<chatsel::LogSel>>,
    log_text: std::cell::RefCell<Option<String>>,
    line_req: std::cell::Cell<Option<(usize, Option<usize>)>>,
    line_sel: std::cell::Cell<bool>,
    /// The chat line's caret this frame, for an IME's candidate window.
    pub ime_caret: std::cell::Cell<Option<egui::Rect>>,
    chat_menu: std::cell::RefCell<Option<chatsel::Menu>>,
    chat_act: std::cell::RefCell<Option<chatsel::Act>>,
    /// The open right-click menu on an item, a party row or an actor (ctxmenu.rs).
    ctx_menu: std::cell::RefCell<Option<ctxmenu::Menu>>,
    /// The retail windows drawn this frame: the edge windows give way to them (they draw under every panel).
    retail: std::cell::RefCell<Vec<Rect>>,
    /// A place changed during a drag: saved once the pointer is let go.
    dirty: bool,
    back_clicked: std::cell::Cell<Option<&'static str>>,
    /// When we were last engaged (modes.rs holds Combat a little after).
    engaged_at: std::cell::Cell<Option<std::time::Instant>>,
    /// The display key drawn at and since when (a display shown the template for a while keeps it).
    display: Option<(String, std::time::Instant)>,
    /// The last name note broken (`name_lines`): the name, its room, the lines.
    name_split: std::cell::RefCell<Option<(String, u32, Vec<String>)>>,
}

impl Modern {
    pub fn new(ctx: &Context) -> Self {
        theme::install_fonts(ctx);
        Self {
            back_clicked: Default::default(),
            theme: Theme::load(),
            art: Art::load(ctx),
            chat_tab: Default::default(),
            chat_settings: Default::default(),
            chat_fold_click: Default::default(),
            chat_folded: Default::default(),
            chat_seen: Default::default(),
            dragging: Default::default(),
            tracker_height: Default::default(),
            lobby_text: Default::default(),
            opacity: std::cell::Cell::new(0.85),
            row_style: Default::default(),
            border_style: Default::default(),
            images: Default::default(),
            status_tips: Default::default(),
            macro_acts: Default::default(),
            grid: Default::default(),
            marquee: Default::default(),
            scan_at: Default::default(),
            scan_rows: Default::default(),
            scan_origin: Default::default(),
            scan_pick: Default::default(),
            scan_picking: Default::default(),
            scan_area: Default::default(),
            job_view: Default::default(),
            rects: Default::default(),
            arranging: Default::default(),
            chat_edge: Default::default(),
            zoom: 0.0,
            captions: Default::default(),
            held_only: Default::default(),
            wheel: Default::default(),
            charlnk_at: Default::default(),
            journal_seen: Default::default(),
            journal_rows: Default::default(),
            styled: false,
            skin_ver: Default::default(),
            scaled: Default::default(),
            ghosts: Default::default(),
            sizes: Default::default(),
            retail: Default::default(),
            edge_grip: Default::default(),
            edge_ghost: Default::default(),
            top_bar: Default::default(),
            sc_open: Default::default(),
            cursors: Default::default(),
            log_sel: Default::default(),
            log_text: Default::default(),
            line_req: Default::default(),
            line_sel: Default::default(),
            ime_caret: Default::default(),
            chat_menu: Default::default(),
            chat_act: Default::default(),
            ctx_menu: Default::default(),
            dirty: false,
            engaged_at: Default::default(),
            display: None,
            name_split: Default::default(),
        }
    }

    /// The windows' background opacity from the config (Settings' Panel Opacity).
    pub fn set_opacity(&self, o: f32) {
        self.opacity.set(o.clamp(0.1, 1.0));
    }

    /// A window background colour at the set opacity.
    pub(super) fn bg(&self, c: egui::Color32) -> egui::Color32 {
        c.gamma_multiply(self.opacity.get())
    }

    /// One frame: egui's zoom for the window height, the panels. True when the config (the layout, the chat font) is
    /// to be saved.
    pub fn ui(
        &mut self,
        ctx: &Context,
        gu: &GameUi,
        w: &mut World,
        window: [u32; 2],
        cfg: &mut crate::config::Config,
    ) -> bool {
        let zoom = self.theme.zoom(window[1] as f32, cfg.modern_scale);
        if (zoom - self.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            self.zoom = zoom;
        }
        // The mods' look (ui.skin), rebuilt when it changes.
        if let Some(s) = w.lua.skin_since(self.skin_ver.get()) {
            self.skin_ver.set(s.ver);
            let c = |c: [u8; 4]| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            self.theme.set_skin(s.colours.iter().filter_map(|(k, v)| v.last().map(|e| (k.clone(), c(e.1)))).collect());
            let sprites: Vec<_> = s
                .sprites
                .iter()
                .filter_map(|(k, v)| v.last().map(|e| (k.clone(), (e.1.key.clone(), e.1.img.clone()))))
                .collect();
            let frames: Vec<_> = s
                .frames
                .iter()
                .filter_map(|(k, v)| {
                    v.last().map(|e| {
                        (
                            k.clone(),
                            e.1.border.as_ref().map(|b| (b.key.clone(), b.img.clone())),
                            e.1.top.map(c),
                            e.1.bottom.map(c),
                        )
                    })
                })
                .collect();
            self.art.set_skin(ctx, &sprites, &frames);
            // egui's widget colours come from the palette.
            self.styled = false;
        }
        // egui's own text (tooltips, the few labels without a size) a little larger than its defaults.
        if !self.styled {
            let (bronze, gold, track) =
                (self.theme.color("bronze"), self.theme.color("gold"), self.theme.color("charcoal"));
            ctx.all_styles_mut(|s| {
                use egui::TextStyle as T;
                for (t, size) in [(T::Body, 14.5), (T::Button, 14.5), (T::Small, 12.5), (T::Monospace, 14.0)] {
                    if let Some(f) = s.text_styles.get_mut(&t) {
                        f.size = size;
                    }
                }
                // Scroll bars always shown where a list is longer than its room (egui's float hidden until hovered),
                // in the pack's colours.
                s.spacing.scroll = egui::style::ScrollStyle {
                    bar_width: 8.0,
                    handle_min_length: 24.0,
                    foreground_color: false,
                    ..egui::style::ScrollStyle::solid()
                };
                s.visuals.widgets.inactive.bg_fill = bronze;
                s.visuals.widgets.hovered.bg_fill = gold;
                s.visuals.widgets.active.bg_fill = gold;
                s.visuals.extreme_bg_color = track;
            });
            self.styled = true;
        }
        // The game's keys are not egui's: Tab (targeting) must not walk egui's focus onto a widget for Enter or Space
        // (talking, a door) to press it (a chat tab switched by itself). Nothing here keeps the keyboard focus.
        ctx.memory_mut(|mem| {
            if let Some(id) = mem.focused() {
                mem.surrender_focus(id);
            }
        });
        *self.retail.borrow_mut() =
            w.retail_frames.iter().map(|r| Rect::from_min_size(pos2(r[0], r[1]), vec2(r[2], r[3]))).collect();
        topbar::draw(self, ctx, &w.hud_top.view);
        pointers(self, ctx, w);
        let focus = focus_panel(w);
        let mode = modes::current(w, &self.engaged_at, focus);
        let mod_windows = w.lua.mode_windows();
        let vis = modes::visibility(ctx, &cfg.modern_modes, mode, &mod_windows);
        // The Lua mods' HUD elements, under every panel, as the modern layer draws its own (each HUD window faded by
        // the mode as a panel is).
        hudmods::draw(self, ctx, gu, w, &vis);
        // Places per display: this window size and UI scale's layout, else the character's last one (the
        // template); a change saves both. A display shown the template for 2 s keeps it as its own, so a change on
        // another display doesn't move its panels; sizes passed through while resizing store nothing. Ours: the
        // character is its UI profile's key (`ui_key`), so the places stay in that profile (character_ui.rs).
        let key = display_key(&w.ui_key, window, cfg.modern_scale);
        let steady = match &self.display {
            Some((k, since)) if *k == key => since.elapsed().as_secs_f32() >= 2.0,
            _ => {
                self.display = Some((key.clone(), std::time::Instant::now()));
                false
            }
        };
        let own = cfg.modern_places.remove(&key);
        let had_own = own.is_some();
        let mut layout = own.or_else(|| cfg.modern_places.get(&w.ui_key).cloned()).unwrap_or_default();
        // The Compact HUD (Forever Modern only) and the Minimal HUD key's toggle.
        set_compact(cfg.forever_compact_hud && w.forever);
        if std::mem::take(&mut w.minimal_hud_toggle) && compact() {
            toggle_minimal(&mut cfg.modern_folded);
            self.dirty = true;
            w.sounds.push(crate::audio::Ui::Select as u32);
        }
        let mut changed = false;
        let mut p = Panels {
            m: self,
            ctx,
            gu: Some(gu),
            layout: &mut layout,
            vis,
            changed: &mut changed,
            chat_font: &mut cfg.modern_chat_font,
            battle_in_all: &mut cfg.modern_battle_in_all,
            condense: &mut cfg.modern_condense,
            chat_hidden: &mut cfg.modern_chat_hidden,
            folded: &mut cfg.modern_folded,
            scan_kinds: &mut cfg.modern_scan_kinds,
            this: HashMap::new(),
            rooms: HashMap::new(),
            retail: HashMap::new(),
            focus,
            hidden: modern_hidden(w),
            placed: w.lua.placed(),
            opacity: w.lua.window_opacities(&w.window_opacity),
        };
        // The chat window first: what sits on it places by this frame's height.
        self.chat_edge.set(None);
        chat::panel(&mut p, w);
        // Establish the current menu bounds before edge panels decide whether to yield to them.
        command::draw(&mut p, w);
        extras::draw(&mut p, w);
        hud::draw(&mut p, w);
        // Keep the command layers above the HUD even though their bounds were needed before the edge panels.
        for id in ["command", "command sub"] {
            if p.this.contains_key(id) {
                ctx.move_to_top(LayerId::new(Order::Middle, Id::unique(("modern panel", id))));
            }
        }
        favs::draw(&mut p, w);
        // One pending panel: a mod's window on the "pending" panel draws instead of the native chip (client/pending.rs),
        // which yields to it. The standard Pending Actions mod is a built-in Extra and never starts (lua/mod.rs
        // BUILT_IN), so today only a player's own mod could take the panel.
        if !w.lua.open_windows().iter().any(|(_, x)| x.panel.as_deref() == Some("pending")) {
            pending::draw(&mut p, w);
        }
        combat::draw(&mut p, w);
        lists::draw(&mut p, w);
        items::draw(&mut p, w);
        equip::draw(&mut p, w);
        config::draw(&mut p, w);
        gmwin::draw(&mut p, w);
        padwin::draw(&mut p, w);
        modwin::draw(&mut p, w);
        // A mod HUD element's tooltip, unless a panel is over it.
        if let Some((title, text, _)) = w.lua.hud_tip()
            && let Some(at) = ctx.input(|i| i.pointer.hover_pos())
            && ctx.layer_id_at(at).is_none_or(|l| l.order == Order::Background)
        {
            self.tip_at(ctx, at, &title, &text);
        }
        status::draw(&mut p, w);
        points::draw(&mut p, w);
        journal::draw(&mut p, w);
        cutscene::draw(&mut p, w);
        social::draw(&mut p, w);
        regioninfo::draw(&mut p, w);
        craft::draw(&mut p, w);
        macros::draw(&mut p, w);
        storage::draw(&mut p, w);
        loot::draw(&mut p, w);
        shop::draw(&mut p, w);
        auction::draw(&mut p, w);
        post::draw(&mut p, w);
        inspect::draw(&mut p, w);
        inspect::dress(&mut p, w);
        minor::draw(&mut p, w);
        era::draw(&mut p, w);
        training::draw(&mut p, w);
        qtrack::draw(&mut p, w);
        navigation::draw(&mut p, w);
        // Journal, tracker and Special can restore an arrow even while its panel is hidden.
        if cfg.forever_map != w.map_prefs {
            cfg.forever_map = w.map_prefs.clone();
            *p.changed = true;
        }
        bluset::draw(&mut p, w);
        pupset::draw(&mut p, w);
        maze::draw(&mut p, w);
        mapwin::draw(&mut p, w);
        // The game's windows that stack on the log (a dialogue's choices ...) go on top of what rides on the chat
        // window (the location panel, the combat window), as retail's stack over its compass and action menu.
        if let Some((r, faded)) = self.chat_edge.get() {
            let riders =
                p.this.iter().filter(|(id, _)| id.as_str() != "chat" && p.rests_on(id, "chat")).map(|(_, r)| r.min.y);
            let top = riders.chain((!faded).then_some(r.min.y)).reduce(f32::min);
            if let Some(t) = top {
                self.chat_edge.set(Some((Rect::from_min_max(pos2(r.min.x, t), r.max), false)));
            }
        }
        game_menu::draw(&mut p, w);
        ko(&mut p, w);
        confirm(&mut p, w);
        choice(&mut p, w);
        gm_prompt(&mut p, w);
        charlnk::draw(&mut p, w);
        mogdoor(&mut p, w);
        event_input(&mut p, w);
        // The Forever scheme's right-click tap backs out like the Back button below, unless a widget took the click.
        let widget_took_it = ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new(kit::SECONDARY_TAKEN))).is_some();
        let tap = std::mem::take(&mut w.right_tap)
            && w.right_back
            && !widget_took_it
            && ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary) && !i.pointer.primary_down())
            && !ctx.egui_wants_keyboard_input();
        if tap && let Some(focus) = back_focus(w) {
            self.back_clicked.set(Some(focus));
        }
        if let Some(focus) = self.back_clicked.take() {
            back(w, focus);
        }
        if modes::editor(&mut p, &mut w.uimodes_open, &mut cfg.modern_modes, mode, &mod_windows) {
            *p.changed = true;
        }
        ctxmenu::draw(self, ctx, w);
        w.ctx_menu_open = self.ctx_menu.borrow().is_some() || self.chat_menu.borrow().is_some();
        keyboard::draw(&mut p, w);
        let this = std::mem::take(&mut p.this);
        // Where each panel is, for mods (ui.native.rect, HUD anchors), in window pixels.
        let ppp = ctx.pixels_per_point();
        let px = |r: &Rect| [r.min.x * ppp, r.min.y * ppp, r.width() * ppp, r.height() * ppp];
        let ghosts = std::mem::take(&mut *self.ghosts.borrow_mut());
        let rooms = std::mem::take(&mut p.rooms);
        w.lua.set_native_rects(
            this.iter().map(|(k, r)| (k.clone(), px(r))).collect(),
            ghosts.iter().map(|(k, r)| (k.clone(), px(r))).collect(),
        );
        w.lua.set_native_rooms(rooms.iter().map(|(k, r)| (k.clone(), px(r))).collect());
        *self.rects.borrow_mut() = this;
        if changed {
            cfg.modern_places.insert(w.ui_key.clone(), layout.clone());
        }
        let freeze = !had_own && !changed && steady && cfg.modern_places.contains_key(&w.ui_key);
        if had_own || changed || freeze {
            cfg.modern_places.insert(key, layout);
        }
        self.dirty |= freeze;
        // The chat settings act at once (the world reads it as lines print).
        w.condense = cfg.modern_condense;
        w.chat.set_fold(cfg.modern_condense);
        self.dirty |= changed;
        let save = self.dirty && !ctx.input(|i| i.pointer.any_down());
        if save {
            self.dirty = false;
        }
        save
    }

    /// The chat window's rect this frame and whether it is fading (then only its bottom edge counts).
    pub fn chat_rect(&self) -> Option<(Rect, bool)> {
        self.chat_edge.get()
    }
}

/// The target cursors in the pack's arrows, under the panels: gold over the target (dimmed on a prompt's saved
/// target), the prompt's pick blue in range, gold at its edge, red past it (retail's range tint); turned to point
/// down at the point, bobbing.
fn pointers(m: &Modern, ctx: &Context, w: &World) {
    let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::unique("modern pointers")));
    let s = ctx.content_rect();
    let bob = (ctx.input(|i| i.time) as f32 * 5.0).sin() * 3.0;
    for &(fx, fy, kind, tint) in &w.modern_pointers {
        let (name, colour) =
            if kind == 1 { ("indicators/target_arrow", Color32::from_gray(170)) } else { pointer_look(tint) };
        let Some(sp) = m.art.sprite(name) else { continue };
        let tip = pos2(s.min.x + fx * s.width(), s.min.y + fy * s.height() + bob);
        let size = 40.0;
        let r = Rect::from_min_max(pos2(tip.x - size / 2.0, tip.y - size), pos2(tip.x + size / 2.0, tip.y));
        painter.add(paint::turned(&sp, r, 2, colour));
    }
}

/// The pack's arrow for a retail cursor tint: the plain one gold, blue (in range) the subtarget arrow, yellow (its
/// edge) gold, red (out of range) gold turned red.
fn pointer_look(tint: u32) -> (&'static str, Color32) {
    match tint & 0x00ff_ffff {
        0x40_40d0 => ("indicators/subtarget", Color32::WHITE),
        0xd0_4040 => ("indicators/target_arrow", Color32::from_rgb(255, 110, 90)),
        _ => ("indicators/target_arrow", Color32::WHITE),
    }
}

/// With the layer off egui is back at its own size (the F12 lines).
pub fn off(ctx: &Context, m: Option<&mut Modern>) {
    let Some(m) = m else { return };
    if m.zoom != 0.0 {
        ctx.set_zoom_factor(1.0);
        m.zoom = 0.0;
    }
    if m.styled {
        let defaults = egui::Style::default();
        ctx.all_styles_mut(|s| {
            s.text_styles = defaults.text_styles.clone();
            s.spacing.scroll = defaults.spacing.scroll;
        });
        // The visuals back as the theme had them (dark or light).
        ctx.set_visuals_of(egui::Theme::Dark, egui::Visuals::dark());
        ctx.set_visuals_of(egui::Theme::Light, egui::Visuals::light());
        m.styled = false;
    }
}

/// A retail list menu as the modern pages draw it (client/partymenu.rs and the others fill it): its layout, cursor,
/// disabled rows, windowps label sprites swapped in, rows marked, values at a row's right.
pub struct MenuWin {
    pub name: &'static str,
    pub sel: u16,
    pub off: Vec<u16>,
    pub labels: Vec<(u16, usize)>,
    pub marked: Vec<u16>,
    pub values: Vec<(u16, String)>,
}

/// A panel's title row: an optional icon, the title, an optional note at the right.
struct Title<'a> {
    icon: Option<&'a str>,
    text: &'a str,
    note: Option<&'a str>,
    /// The note is a name: broken as retail's target window breaks one (`name_lines`).
    name: bool,
}

/// What drawing needs besides the panel itself: the art, egui, the game's data.
#[derive(Clone, Copy)]
struct Look<'a> {
    m: &'a Modern,
    ctx: &'a Context,
    gu: Option<&'a GameUi>,
}

/// The panels Lua mods hid; the chat window stays while typed in or needed by an event.
fn modern_hidden(w: &World) -> Vec<&'static str> {
    let mut h = w.lua.hidden_panels();
    if w.typing || super::event::active(w) || super::event::prompt(w) {
        h.retain(|p| *p != "chat");
    }
    h
}

/// The config `modern_places` key of a character's layout for one display: the window's size in pixels and the UI
/// scale (the plain name holds the character's latest layout, the template for a display not yet shown).
pub fn display_key(name: &str, window: [u32; 2], scale: u32) -> String {
    format!("{name} @{}x{} {scale}%", window[0], window[1])
}

/// Whether config `modern_places` key `key` is one of `name`'s layouts (the plain one or a display's).
pub fn layout_of(key: &str, name: &str) -> bool {
    key == name || key.strip_prefix(name).is_some_and(|rest| rest.starts_with(" @"))
}

/// What a frame's panels share.
struct Panels<'a> {
    m: &'a Modern,
    ctx: &'a Context,
    /// The game's data (status icons and texts); none in the offscreen shot.
    gu: Option<&'a GameUi>,
    layout: &'a mut Layout,
    changed: &'a mut bool,
    chat_font: &'a mut f32,
    /// The chat's All tab shows the battle lines (config modern_battle_in_all).
    battle_in_all: &'a mut bool,
    /// An action's battle lines merge (config modern_condense).
    condense: &'a mut bool,
    /// The chat tabs hidden (config modern_chat_hidden).
    chat_hidden: &'a mut Vec<String>,
    folded: &'a mut std::collections::BTreeMap<String, bool>,
    scan_kinds: &'a mut u8,
    /// The panels drawn this frame, as others dock on them (the chat window while it fades: its bottom edge).
    this: HashMap<String, Rect>,
    /// The whole room of the panels drawn folded or growing this frame (Show::full).
    rooms: HashMap<String, Rect>,
    /// Places retail works out this frame (the minimap left of the windows at the edge): the default until moved.
    retail: HashMap<String, Place>,
    /// The panel with the keys (focus_panel): drawn in the focused frame.
    focus: Option<&'static str>,
    /// The mode presets' panels' opacity this frame (modes.rs): 0 not drawn; others always drawn.
    vis: HashMap<String, f32>,
    /// Panels a Lua mod hid (ui.native.hide).
    hidden: Vec<&'static str>,
    /// Panels a Lua mod placed, scaled or faded (ui.native.place).
    placed: HashMap<String, super::lua::Placed>,
    /// Each window's opacity that is not 1 (per-window opacity: the player's, else its mod's), by panel id.
    opacity: HashMap<String, f32>,
}

/// The panel that takes the keys now, as the key dispatch (client/mod.rs) routes them: the prompt, the chat line, an
/// event's choice or input, the lists, the windows the command menu opens, the main menu, the action menu.
pub(super) fn focus_panel(w: &World) -> Option<&'static str> {
    use super::{event, searchmenu, treasure};
    Some(match () {
        _ if w.confirm.is_some() => "confirm",
        _ if w.game_menu.is_some() => "game menu",
        _ if w.era_view || w.era_new != 0 => "expansions",
        _ if w.typing => "chat",
        _ if super::training::active(w) => "training menu",
        _ if w.voice.focused() => "voice",
        _ if w.palette.0 != 0 => "macro palette",
        _ if w.ko_offer == 2 && w.dead() => "raise",
        _ if w.ko_open && w.ko_offer != 2 => "ko",
        _ if w.concierge_ui.is_some() => "window",
        _ if super::ranking::is_open(w) => "window",
        _ if w.maze.screen.is_some() => "window",
        _ if event::choice(w).is_some() => "choice",
        _ if w.gmtell.open.is_some() => "gm prompt",
        _ if event::input(w).is_some() => "event input",
        _ if event::active(w) => return None,
        _ if w.charlnk.is_some() => "charlnk",
        _ if w.polmail.line.is_some() || super::social_chat::composing(w) => "msgline",
        _ if w.polmail.social.history.is_some() => "banshee messages",
        _ if w.polmail.list.is_some() => "msglist",
        _ if w.friendui.as_ref().is_some_and(|u| u.list.is_some() && u.stat.is_none()) => "friendlist",
        _ if w.friendui.is_some() => "window",
        _ if w.modern && w.settings.as_ref().is_some_and(|s| s.windowed() || s.inline_review()) => options::ID,
        _ if w.settings.is_some() || w.config.is_some() || w.bluset.is_some() || w.pupset.is_some() => "window",
        _ if gmwin::shown(w) => "window",
        _ if padwin::shown(w) => "window",
        _ if w.magic.is_some() || w.ability.is_some() => "list",
        _ if w.cats.is_some() => "cats",
        _ if w.inspect.up() => "check",
        _ if w.seacom_edit.is_some() => "comment",
        _ if w.chatctrl.is_some() => "chatctrl",
        _ if treasure::active(w) => "loot",
        _ if w.levelsync.is_some() => "levelsync",
        _ if w.search.is_some() && !searchmenu::refining(w) && w.arealist.is_none() => "results",
        _ if w.arealist.is_some() && w.searchmenu.is_none() => "arealist",
        _ if w.linklist.is_some() => "linklist",
        _ if w.blklist.is_some() => "blklist",
        _ if w.mogdoor.is_some() => "mogdoor",
        _ if w.menu.is_none() && super::window_over_actions(w) => "window",
        _ if w.menu.is_some() && w.cmd.sub.is_some() => "command sub",
        _ if w.menu.is_some() => "command",
        _ if w.action_menu.is_some() => "combat",
        _ => return None,
    })
}

/// The panels a right-click tap does not back out of: typing (the chat line, a message, a comment, a name or number an
/// event asks for, the voice window), the combat action menu (it is the field's own, retail's right click clears the
/// target there), an event's choice or text (the Back button is not drawn there either: Escape could cancel the
/// server's event), the macro palette, and the K.O. and raise prompts.
const NO_RIGHT_BACK: [&str; 11] = [
    "combat",
    "chat",
    "msgline",
    "event input",
    "choice",
    "comment",
    "gm prompt",
    "voice",
    "macro palette",
    "raise",
    "ko",
];

/// What a right-click tap backs out of: the focused panel (`focus_panel`), unless it is one that taps leave alone
/// (`NO_RIGHT_BACK`), a text field has the keyboard, or a Settings window holds changes that backing out would
/// throw away (the Back button's Cancel does; a tap must never).
pub(super) fn back_focus(w: &World) -> Option<&'static str> {
    focus_panel(w).filter(|f| right_back_allowed(f, super::typing_elsewhere(w), settings_unsaved(w)))
}

fn settings_unsaved(w: &World) -> bool {
    w.settings.as_ref().is_some_and(|s| !s.can_back_out())
}

/// The rule of `back_focus` on its inputs.
fn right_back_allowed(focus: &str, typing: bool, unsaved_settings: bool) -> bool {
    // Escape in a deferred (Apply / Cancel) Settings window is Cancel, on the Settings page (options::ID) and on the
    // others (focus "window"): a tap never discards what is unapplied.
    !typing && !NO_RIGHT_BACK.contains(&focus) && !(matches!(focus, options::ID | "window") && unsaved_settings)
}

/// Back out of the panel that has focus: Settings goes back to what it was; any other gets Escape next frame (only
/// while it still has the focus).
fn back(w: &mut World, focus: &'static str) {
    if focus == options::ID {
        if let Some(s) = w.settings.as_mut() {
            s.cancel();
        }
    } else {
        w.mouse_back = Some(focus);
    }
}

/// The panel margins inside the frame; the resizable one keeps a taller strip below for its glyph.
const MARGIN: egui::Margin = egui::Margin { left: 22, right: 22, top: 16, bottom: 18 };
const ROOM_MARGIN: egui::Margin = egui::Margin { bottom: 26, ..MARGIN };
/// The Compact HUD's panel margins (the HUD panels only; menus and windows keep MARGIN).
const COMPACT_MARGIN: egui::Margin = egui::Margin { left: 14, right: 14, top: 10, bottom: 12 };
const COMPACT_ROOM_MARGIN: egui::Margin = egui::Margin { bottom: 22, ..COMPACT_MARGIN };

thread_local! {
    /// The Compact HUD (config forever_compact_hud) as this frame has it: set once a frame by draw() from the config and
    /// read by the HUD panels' margins and sizes. The layer draws on one thread; each test thread has its own.
    static COMPACT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
/// Whether the Compact HUD is on this frame.
pub(super) fn compact() -> bool {
    COMPACT.with(|c| c.get())
}
pub(super) fn set_compact(on: bool) {
    COMPACT.with(|c| c.set(on));
}
/// The job helper's width (the Wide Scan slot): 290 points, 250 in the Compact HUD.
pub(super) fn job_width() -> f32 {
    if compact() { 250.0 } else { 290.0 }
}
/// The title row with the gap under it (28, egui's 3 under it, then 6, or 3 in the Compact HUD).
pub(super) fn title_h() -> f32 {
    if compact() { 34.0 } else { 37.0 }
}
/// The margins of a HUD panel (`room`: the resizable ones with the strip for the grip) as the Compact HUD has them.
pub(super) fn margin_of(room: bool) -> egui::Margin {
    match (room, compact()) {
        (true, false) => ROOM_MARGIN,
        (false, false) => MARGIN,
        (true, true) => COMPACT_ROOM_MARGIN,
        (false, true) => COMPACT_MARGIN,
    }
}
/// The HUD panels the Compact HUD tightens: the ones always on the screen. The menus and the windows the player opens
/// keep the full margins.
const HUD_PANELS: [&str; 12] = [
    "quest tracker",
    "party",
    "target",
    "focus",
    "chat",
    "where",
    "navigation",
    "training hud",
    "cast",
    "pending",
    "countdown",
    "objective",
];
/// The panels a fold button on their title folds to the title bar (Compact HUD on). The party window, the chat window,
/// the minimap and the job helper fold in their own drawing; these go through Panels::show. Never folded: the target,
/// the navigation arrow with its Stop button, the chat line while typing, the notices.
const FOLDABLE: [&str; 2] = ["quest tracker", "training hud"];
/// The panels with a fold button on their title through Panels::show: the FOLDABLE ones (their body is not drawn while
/// folded) and the party window, whose drawing folds its rows down to our own.
const FOLD_BUTTON: [&str; 3] = ["quest tracker", "training hud", "party"];
/// Every panel a fold button or the Minimal HUD folds (the ids of the folds map).
pub const FOLD_IDS: [&str; 6] = ["quest tracker", "training hud", "party", "chat", "minimap", "widescan"];

/// The key in the character's folds that stands for the Minimal HUD (every foldable panel folded), kept apart from the
/// panels' own folds so turning it off brings each panel back as it was.
pub const MINIMAL_KEY: &str = "minimal hud";

/// Whether the Minimal HUD is on in a character's folds.
pub fn minimal_on(folded: &std::collections::BTreeMap<String, bool>) -> bool {
    folded.get(MINIMAL_KEY).copied().unwrap_or(false)
}

/// The key of the exception a panel has while the Minimal HUD is on: opened by its own chevron.
fn open_key(id: &str) -> String {
    format!("minimal open:{id}")
}

/// A panel's fold. While the Compact HUD is on and the Minimal HUD with it, every panel is folded but the ones the
/// player opened by their chevron; otherwise a panel's fold is its own. The Minimal HUD never changes a panel's own fold,
/// and with the Compact HUD off it does nothing at all.
pub fn folded_in(folded: &std::collections::BTreeMap<String, bool>, id: &str, compact: bool) -> bool {
    if compact && minimal_on(folded) {
        return !folded.get(&open_key(id)).copied().unwrap_or(false);
    }
    folded.get(id).copied().unwrap_or(false)
}

/// The fold button of panel `id`: folds it, or unfolds it. While the Minimal HUD is on it opens (or closes again) just
/// this panel, as an exception to the Minimal HUD; the panel's own fold is left alone.
pub fn toggle_fold(folded: &mut std::collections::BTreeMap<String, bool>, id: &str) {
    if minimal_on(folded) {
        let key = open_key(id);
        if folded.remove(&key).is_none() {
            folded.insert(key, true);
        }
    } else {
        let now = folded.get(id).copied().unwrap_or(false);
        folded.insert(id.into(), !now);
    }
}

/// The Minimal HUD's key: every foldable panel folded at once, and back as each was. Panels' own folds are untouched, and
/// the exceptions the chevrons made go with it.
pub fn toggle_minimal(folded: &mut std::collections::BTreeMap<String, bool>) {
    folded.retain(|k, _| !k.starts_with("minimal open:"));
    if minimal_on(folded) {
        folded.remove(MINIMAL_KEY);
    } else {
        folded.insert(MINIMAL_KEY.into(), true);
    }
}

/// A fold chevron on a panel's title bar: down while open, right while folded. True when clicked.
fn fold_button(ui: &mut Ui, m: &Modern, folded: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    let colour = if resp.hovered() { Color32::WHITE } else { m.theme.color("muted") };
    let (c, s) = (r.center(), 4.5);
    let pts = if folded {
        vec![c + vec2(-s * 0.6, -s), c + vec2(s * 0.6, 0.0), c + vec2(-s * 0.6, s)]
    } else {
        vec![c + vec2(-s, -s * 0.6), c + vec2(0.0, s * 0.6), c + vec2(s, -s * 0.6)]
    };
    ui.painter().add(Shape::line(pts, egui::Stroke::new(1.8, colour)));
    resp.on_hover_text(if folded {
        "Unfold this panel (Ctrl+Shift+H folds or unfolds them all: the Minimal HUD)"
    } else {
        "Fold this panel to its title bar (Ctrl+Shift+H folds them all: the Minimal HUD)"
    })
    .clicked()
}

/// The panels placed by their bottom, growing upward (the chat window, the party window).
const BOTTOM: [&str; 2] = ["chat", "party"];
/// The right edge's column (the main menu, its submenu and the windows it opens): the panels placed from the right
/// keep left of it; the edge windows stay put under it.
const COLUMN: [&str; 3] = ["command", "command sub", "window"];
/// The windows on a screen edge (extras.rs edge_window): they slide along it, so nothing docks on them.
const EDGE: [&str; 4] = ["timers", "minimap", "widescan", "regime"];

impl Dock {
    /// The side of the panel it sits on that `dx` counts from.
    fn x(&self, t: Rect) -> f32 {
        if self.right { t.max.x } else { t.min.x }
    }
}

/// Where each panel starts (the user's own arrangement, 2026-10-26; laid out in points from the screen's edges,
/// 2026-11-03, so no two overlap at any window size or Interface Size): the chat and party windows on the screen's
/// bottom, the chat window as wide as the room left of the party column (chat.rs default_room), the location panel on
/// the chat window and the combat window on it (retail's compass and action menu ride the log), the target on the
/// party window (retail's targetwi, flag 0x2000), the status icons, the command window and the Timers at the left under
/// the top bar, the main menu and the minimap at the right under it. `screen` is the screen's size in points.
fn default_place(id: &str, screen: Vec2) -> Place {
    use theme::{BELOW_BAR, COLUMN_W, PANEL_GAP as GAP, SCREEN_GAP as EDGE, TEXT_PAD};
    let (margin_left, margin_top) = (MARGIN.left as f32, MARGIN.top as f32);
    let (sw, sh) = (screen.x.max(1.0), screen.y.max(1.0));
    let at = |x, y| Place { x, y, ..Default::default() };
    // From the left edge, from the top bar's bottom, and (a fraction below zero) from the right edge.
    let left = |dx: f32, y: f32| at((EDGE + dx) / sw, y / sh);
    let right = |y: f32| at(-EDGE / sw, y / sh);
    let on =
        |to: &str| Place { dock: Some(Dock { to: to.into(), gap: GAP, dx: 0.0, right: false }), ..Default::default() };
    match id {
        "chat" => Place { x: EDGE / sw, b: 1.0 - EDGE / sh, ..Default::default() },
        "party" => Place { x: (sw - EDGE - COLUMN_W) / sw, b: 1.0 - EDGE / sh, ..Default::default() },
        "where" => {
            Place { dock: Some(Dock { to: "chat".into(), gap: -0.5, dx: 1.25, right: false }), ..Default::default() }
        }
        "combat" => on("where"),
        "target" => on("party"),
        // Ours: the focus and last targets on the target panel.
        "focus" => on("target"),
        // The magic and ability windows ride on the combat window (retail's sit on the log), the list on its categories.
        "cats" => on("combat"),
        "list" => on("cats"),
        // The effect icons under the top bar's title box, their first icon in line with its text (the panel's margin
        // is clear of the frame it does not draw).
        "status" => at((EDGE + TEXT_PAD - margin_left) / sw, (BELOW_BAR - margin_top) / sh),
        "command" => right(BELOW_BAR),
        // raid1 / raid2's places (y 48 and 150 of the 448 layout).
        "alliance1" => at(0.87, 0.107),
        "alliance2" => at(0.87, 0.335),
        // The cast bar centred under the top bar (it stood at retail's (16, 94), under the effect icons once there
        // were two rows of them), the 0x075 countdown under it, the objective's bars under that; the Timers on the
        // left edge under the most effect icons there are (four rows), the minimap on the right one.
        "cast" => at((sw - 300.0) / 2.0 / sw, BELOW_BAR / sh),
        // The pending action's chip just over it (pending.rs).
        "pending" => at((sw - 300.0) / 2.0 / sw, (BELOW_BAR - 52.0).max(0.0) / sh),
        "countdown" => at((sw - 300.0) / 2.0 / sw, (BELOW_BAR + 64.0) / sh),
        "objective" => at((sw - 300.0) / 2.0 / sw, (BELOW_BAR + 128.0) / sh),
        "timers" => at(0.0, (BELOW_BAR + 4.0 * 54.0 + GAP) / sh),
        // The Timers' three windows down the same edge (the recasts where the one window was).
        "timers_recasts" => at(0.0, (BELOW_BAR + 4.0 * 54.0 + GAP) / sh),
        // The training regime lower on the same edge.
        "regime" => at(0.0, 0.6),
        "training hud" => at(0.0, 0.35),
        "quest tracker" => right(sh * 0.375),
        // The job helper / Wide Scan shares the bottom beside the party, leaving the tracker below the map.
        "widescan" => {
            Place { x: ((sw - EDGE - COLUMN_W - GAP - job_width()).max(EDGE)) / sw, edge: 2, ..Default::default() }
        }
        "navigation" => at(((sw - 220.0) / 2.0).max(0.0) / sw, BELOW_BAR / sh),
        // The skillchain helper on the right edge, under the minimap.
        "skillchains" => at(-0.001, 0.25),
        // Under the top bar's network box, not over it.
        "minimap" => at(-0.001, BELOW_BAR / sh),
        // The command window: every window the command menu opens shows here. The item info rides the log as retail's
        // (mode 0, flag 0x1000).
        "window" | "expansions" | "training menu" => left(0.0, BELOW_BAR),
        // Upstream: the Controller window over the Settings window it is opened from. The command window's
        // placement above is HEAD's (the Modern default places), which upstream's at(0.0047, 0.0347) replaced.
        "controller" => at(0.03, 0.06),
        "iteminfo" => on("combat"),
        // The treasure pool rides the action menu it is opened from, over the item info of its cursor's row.
        "loot" => on("iteminfo"),
        // /check beside the command window's place.
        "check" | "blklist" => at(0.2, BELOW_BAR / sh),
        // Ours: the command palette above the screen's middle.
        "palette" => at(0.38, 0.22),
        // The chat mode window rides the action menu it is opened from.
        "chatctrl" => on("combat"),
        // The comment editor sits on the log as retail's (flag 0x1000).
        "comment" => on("combat"),
        // The favourites strip over the chat window's right end.
        "favourites" => at(0.42, 0.66),
        // The map's menu at the top right, as retail's map0; Markers and Change Map's list in its place.
        "map0" | "mapv3" | "maplist" | "mapscan" => at(-0.004, 0.1),
        // Wide Scan's list at retail's (16, 48).
        "scanlist" => at(0.0083, 0.1),
        // The voice chat window (ours) near the middle, its talking list on the left edge.
        "voice" => at(0.36, 0.16),
        "voice talk" => at(0.005, 0.42),
        _ => at(0.3, 0.3),
    }
}

/// The K.O. window (retail's "menu    dead", mp_dead in Monstrosity) and the raise prompt (resynus) in the kit's
/// buttons, with retail's captions; their keys stay the client's (client/mod.rs). The K.O. window sits above the
/// screen's middle, the raise prompt on the log as comyn.
fn ko(p: &mut Panels, w: &mut World) {
    use super::{Confirm, answer_raise, mon_dead_pick, monstrosity, vana_now};
    if !w.ko_open && w.ko_offer != 2 {
        return;
    }
    let (m, Some(gu)) = (p.m, p.gu) else { return };
    let screen = p.ctx.content_rect();
    let caption = |menu: &str, k: u16, d: &str| gu.child_caption(menu, k).unwrap_or_else(|| d.to_string());
    let size = m.theme.ui;
    let font = FontId::proportional(size);
    let width = (size * 7.5).max(120.0);
    if w.ko_open {
        const W: f32 = 340.0;
        let mon = monstrosity::active(w);
        let menu = if mon { "menu    mp_dead" } else { "menu    dead" };
        let title = gu.focus_text("menu    dead", 1)[1].clone().unwrap_or_else(|| "K.O.".into());
        let focused = w.confirm.is_none() && w.ko_offer != 2;
        let s = w.ko_deadline.saturating_sub(vana_now(w));
        let left = format!("Time left  {}:{:02}", s / 60, s % 60);
        let rows: Vec<(u16, String)> = if mon {
            vec![(1, caption(menu, 1, "Retry")), (2, caption(menu, 2, "Cancel"))]
        } else {
            vec![(1, caption(menu, 1, "Back to Home Point"))]
        };
        let mut pick = None;
        let at = pos2(screen.center().x - W / 2.0, screen.min.y + screen.height() * 0.3);
        p.panel_at(
            "ko",
            Some(Title { icon: Some("icons/warning"), text: &title, note: None, name: false }),
            W,
            at,
            |ui| {
                ui.label(egui::RichText::new(&left).size(size).color(m.theme.color("text")));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    for (k, label) in &rows {
                        let on = focused && (!mon || *k == w.mon_dead_sel);
                        let wide = if mon { width } else { W - 44.0 };
                        let (r, resp) = ui.allocate_exact_size(vec2(wide, size + 16.0), Sense::click());
                        let s = kit::Btn { lit: on, cursor: on, off: !focused };
                        kit::paint_button(ui.painter(), m, r, label, &font, s, resp.hovered() && focused);
                        if kit::clicked(&resp, s) {
                            pick = Some(*k);
                        }
                    }
                });
            },
        );
        match pick {
            Some(k) if mon => {
                w.mon_dead_sel = k;
                mon_dead_pick(w);
            }
            Some(_) => {
                w.confirm = Some((Confirm::HomePoint, false));
                w.sounds.push(crate::audio::Ui::Select as u32);
            }
            None => {}
        }
    }
    if w.ko_offer == 2 && w.confirm.is_none() && w.dead() {
        const W: f32 = 380.0;
        let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
        let h = m.rects.borrow().get("raise").map_or(110.0, |r| r.height());
        let at = pos2(screen.width() * 0.0121, edge - h - 6.0);
        let title = gu.focus_text("menu    resynus", 1)[1].clone().unwrap_or_else(|| "Revival".into());
        let (yes, no) = (caption("menu    resynus", 1, "Accept"), caption("menu    resynus", 2, "Decline"));
        let mut click = None;
        p.panel_at(
            "raise",
            Some(Title { icon: Some("icons/raise"), text: &title, note: None, name: false }),
            W,
            at,
            |ui| click = kit::yes_no(ui, m, "", [&yes, &no], w.raise_yes, size),
        );
        if let Some(v) = click {
            w.raise_yes = v;
            answer_raise(w, v);
            w.sounds.push(crate::audio::Ui::Select as u32);
        }
    }
}

/// The yes / no prompt (retail's comyn, stacked on the log): the kit's prompt; its keys stay retail's (Left / Right,
/// Enter, Escape is No). A click answers.
fn confirm(p: &mut Panels, w: &mut World) {
    let Some((_, yes)) = w.confirm else { return };
    let m = p.m;
    let screen = p.ctx.content_rect();
    // On the chat window, else the screen's bottom (a faded chat window folds into the screen's edge).
    let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
    let caption =
        |k: u16, d: &str| p.gu.and_then(|g| g.child_caption("menu    comyn", k)).unwrap_or_else(|| d.to_string());
    let (y_label, n_label) = (caption(1, "Yes"), caption(2, "No"));
    let text = w.confirm_text.clone();
    let h = m.rects.borrow().get("confirm").map_or(110.0, |r| r.height());
    let at = pos2(screen.width() * 0.0121, edge - h - 6.0);
    let mut click = None;
    p.panel_at("confirm", None, 380.0, at, |ui| {
        click = kit::yes_no(ui, m, &text, [&y_label, &n_label], yes, m.theme.ui)
    });
    if let Some(v) = click {
        if let Some(c) = w.confirm.as_mut() {
            c.1 = v;
        }
        w.confirm_click = Some(v);
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
}

/// A list stacked on the log as comyn is (an event's choice, the Mog House door menu): the question and every row in
/// its colour (in-line colour codes as the log's), the cursor's lit and kept in view; the row clicked.
fn log_list(
    p: &mut Panels,
    id: &str,
    question: &str,
    rows: &[(String, [u8; 4])],
    cursor: usize,
    free: bool,
) -> Option<usize> {
    const W: f32 = 460.0;
    const ROW: f32 = 28.0;
    let m = p.m;
    let screen = p.ctx.content_rect();
    let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
    let h = m.rects.borrow().get(id).map_or(160.0, |r| r.height());
    let at = pos2(screen.width() * 0.0121, edge - h - 6.0);
    let mut click = None;
    let focus = p.focus;
    p.panel_at(id, None, W, at, |ui| {
        let font = FontId::proportional(m.theme.ui);
        let job = |t: &str, base: [u8; 4], wrap: f32| chat::line_job(("", Color32::WHITE), t, base, &font, wrap);
        if !question.is_empty() {
            ui.label(job(question, [0x80; 4], W - 44.0));
            ui.add_space(6.0);
        }
        let seen = egui::Id::unique((id, "log list cursor seen"));
        let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cursor);
        ui.ctx().data_mut(|d| d.insert_temp(seen, cursor));
        egui::ScrollArea::vertical()
            .id_salt((id, "rows"))
            .max_height(8.0 * (ROW + 2.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (k, (text, base)) in rows.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(W - 58.0, ROW), Sense::click());
                    let face = match () {
                        _ if k == cursor => "controls/list_row_selected",
                        _ if resp.hovered() => "controls/list_row_hover",
                        _ => "controls/list_row_normal",
                    };
                    rows::face(ui.painter(), m, r, face, Color32::WHITE);
                    let clip = Rect::from_min_max(r.min, pos2(r.max.x - 8.0, r.max.y));
                    let g = ui.painter().layout_job(job(text, *base, f32::INFINITY));
                    ui.painter().with_clip_rect(clip).galley(
                        pos2(r.min.x + 24.0, r.center().y - g.size().y / 2.0),
                        g,
                        Color32::WHITE,
                    );
                    if k == cursor && follow {
                        ui.scroll_to_rect(r, None);
                    }
                    if free && resp.clicked() {
                        click = Some(k);
                    }
                    ui.add_space(2.0);
                }
            });
        if free && focus == Some(id) && craft::button(ui, m, "Back", 110.0, false, false) {
            m.back_clicked.set(focus);
        }
    });
    click
}

/// An event's choice (retail's query): its keys stay retail's (event.rs); a click picks.
fn choice(p: &mut Panels, w: &mut World) {
    let Some(c) = super::event::choice(w) else { return };
    let rows: Vec<(String, [u8; 4])> = c.options.iter().map(|o| (o.1.clone(), [0x80; 4])).collect();
    let (question, cursor) = (c.question.clone(), c.cursor);
    if let Some(k) = log_list(p, "choice", &question, &rows, cursor, w.confirm.is_none()) {
        super::event::set_choice(w, k);
        super::event::key(w, egui::Key::Enter);
    }
}

/// A GM prompt (retail's gmtell, the query's class): its keys stay gmtell.rs's; a click picks.
fn gm_prompt(p: &mut Panels, w: &mut World) {
    let Some(g) = w.gmtell.open.as_ref() else { return };
    let rows: Vec<(String, [u8; 4])> = g.options.iter().map(|o| (o.clone(), [0x80; 4])).collect();
    let (question, cursor) = (g.question.clone(), g.cursor);
    if let Some(k) = log_list(p, "gm prompt", &question, &rows, cursor, w.confirm.is_none()) {
        super::gmtell::pointer(w, k, true);
    }
}

/// An event's text input (retail's passinpu over the message line): its title (group 0xb by kind) or "Password:",
/// the text as typed with a steady caret; the keys stay event.rs's.
fn event_input(p: &mut Panels, w: &mut World) {
    let Some(inp) = super::event::input(w) else { return };
    let Some(gu) = p.gu else { return };
    let m = p.m;
    let title = match inp.kind {
        1..=5 => gu.group_b([0xb9, 0xbe, 0xb9, 0xc8, 0xc9][inp.kind as usize - 1]).unwrap_or_default(),
        _ => gu.child_caption("menu    passinpu", 1).unwrap_or_else(|| "Password:".into()),
    };
    let text = String::from_utf8_lossy(&inp.text).into_owned();
    let caret = inp.caret.min(text.len());
    let screen = p.ctx.content_rect();
    let edge = m.chat_edge.get().filter(|c| !c.1).map_or(screen.max.y, |(r, _)| r.min.y).min(screen.max.y);
    let h = m.rects.borrow().get("event input").map_or(80.0, |r| r.height());
    let at = pos2(screen.width() * 0.0121, edge - h - 6.0);
    let caret = text[..caret].chars().count();
    let mut action = None;
    p.panel_at("event input", None, 460.0, at, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&title).size(m.theme.ui).color(m.theme.color("gold")));
            kit::field(ui, m, &text, Some(caret), ui.available_width(), FontId::monospace(m.theme.ui), None);
        });
        action = input_buttons(ui, m, "Accept", "Back");
    });
    if let Some(key) = action {
        super::event::input_events(w, &[button_key(key)]);
    }
}

/// Route pointer buttons through the same action handlers as the keyboard.
pub(super) fn button_key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

pub(super) fn input_buttons(ui: &mut Ui, m: &Modern, accept: &str, cancel: &str) -> Option<egui::Key> {
    let mut action = None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if craft::button(ui, m, accept, 110.0, false, false) {
            action = Some(egui::Key::Enter);
        }
        if craft::button(ui, m, cancel, 110.0, false, false) {
            action = Some(egui::Key::Escape);
        }
    });
    action
}

fn modal_panel(focus: Option<&str>) -> bool {
    matches!(
        focus,
        Some(
            "game menu"
                | "confirm"
                | "raise"
                | "event input"
                | "msgline"
                | "banshee messages"
                | "choice"
                | "gm prompt"
                | "expansions"
                | "options"
        )
    )
}

fn panel_interactive(focus: Option<&str>, id: &str) -> bool {
    !modal_panel(focus) || focus == Some(id)
}

/// The Mog House door menu (FUN_101d62b0): its title and rows (the exit areas while choosing one).
fn mogdoor(p: &mut Panels, w: &mut World) {
    let Some(sel) = w.mogdoor.as_ref().map(|m| m.sel) else { return };
    let (title, rows) = super::mogdoor_rows(w);
    let rows: Vec<(String, [u8; 4])> = rows.into_iter().map(|r| (r.1, r.2)).collect();
    if let Some(k) = log_list(p, "mogdoor", &title, &rows, sel, w.confirm.is_none()) {
        if let Some(m) = w.mogdoor.as_mut() {
            m.sel = k;
        }
        super::mogdoor_pick(w);
    }
}

/// How a panel shows: its title, width (and the chat window's default room), resizable, at a fixed point (beside
/// another, not moved), its frame, its exact height when known (placing it by its bottom without a frame's lag), its
/// fade, and how much of its height shows from the bottom up (the chat window folds into its bottom edge as it fades).
/// The rect returned and docked on is the part showing. `cap` is room over the frame, in the panel (the party window's
/// experience bar), counted in `height`.
struct Show<'s> {
    title: Option<Title<'s>>,
    size: Vec2,
    resizable: bool,
    at: Option<Pos2>,
    frame: &'s str,
    height: Option<f32>,
    alpha: f32,
    fold: f32,
    cap: f32,
    /// A resizable panel's least room (the corner grip stops there; a smaller saved size shows at this).
    least: Vec2,
    /// The height it grows back to, when it folds or grows (the chat window): its whole room for ui.native.rect.
    full: Option<f32>,
}

impl Default for Show<'_> {
    fn default() -> Self {
        Self {
            title: None,
            size: Vec2::ZERO,
            resizable: false,
            at: None,
            frame: "window_normal",
            height: None,
            alpha: 1.0,
            fold: 1.0,
            cap: 0.0,
            least: vec2(240.0, 60.0),
            full: None,
        }
    }
}

/// Where `r` stands moved onto `screen` (as egui keeps an area on screen; one larger than the screen keeps its top left).
fn on_screen(r: Rect, screen: Rect) -> Pos2 {
    pos2(
        r.min.x.min(screen.max.x - r.width()).max(screen.min.x),
        r.min.y.min(screen.max.y - r.height()).max(screen.min.y),
    )
}

impl<'a> Panels<'a> {
    fn look(&self) -> Look<'a> {
        Look { m: self.m, ctx: self.ctx, gu: self.gu }
    }

    /// An edge or foldable panel's fold: its own, or the Minimal HUD's while the Compact HUD is on.
    fn fold_of(&self, id: &str) -> bool {
        folded_in(self.folded, id, compact())
    }

    /// A HUD panel folded by the Compact HUD (its own fold or the Minimal HUD's): never while the Compact HUD is off.
    fn hud_folded(&self, id: &str) -> bool {
        compact() && self.fold_of(id)
    }

    /// The mode's opacity for a panel (1 for every panel the modes don't list).
    fn vis(&self, id: &str) -> f32 {
        self.vis.get(id).copied().unwrap_or(1.0)
    }

    /// A panel `width` points wide.
    fn panel(&mut self, id: &str, title: Option<Title>, width: f32, add: impl FnOnce(&mut Ui)) -> Option<Rect> {
        self.framed(id, title, width, "window_normal", add)
    }

    /// A panel in one of the pack's other frames (window_alert ...).
    fn framed(
        &mut self,
        id: &str,
        title: Option<Title>,
        width: f32,
        frame: &str,
        add: impl FnOnce(&mut Ui),
    ) -> Option<Rect> {
        self.show(id, Show { title, size: vec2(width, 0.0), frame, ..Default::default() }, |ui, _| add(ui))
    }

    /// A panel the player sizes by its corner grip: `add` gets its room (saved width and height, else `size`, never
    /// under `least`). The room's height is the contents' below the title.
    fn sized(
        &mut self,
        id: &str,
        title: Option<Title>,
        size: Vec2,
        least: Vec2,
        add: impl FnOnce(&mut Ui, Vec2),
    ) -> Option<Rect> {
        self.show(id, Show { title, size, least, resizable: true, ..Default::default() }, add)
    }

    /// A panel shown beside another (the command submenu): at a screen point, not moved.
    fn panel_at(
        &mut self,
        id: &str,
        title: Option<Title>,
        width: f32,
        at: Pos2,
        add: impl FnOnce(&mut Ui),
    ) -> Option<Rect> {
        self.show(id, Show { title, size: vec2(width, 0.0), at: Some(at), ..Default::default() }, |ui, _| add(ui))
    }

    /// A painter over a panel drawn this frame, for marks around its frame.
    fn over(&self, id: &str) -> egui::Painter {
        self.ctx.layer_painter(egui::LayerId::new(Order::Middle, Id::unique(("modern panel", id))))
    }

    /// A panel's place: the saved one, else its default (a bottom-placed panel saved before it had one keeps the
    /// default bottom).
    fn place(&self, id: &str) -> Place {
        // The recasts' window takes the one Timers window's saved place, the buffs' and debuffs' windows steps under it
        // (its sections' order) until they are moved.
        let below = |k: f32| {
            let r = self.place("timers_recasts");
            Place { y: r.y + 0.12 * k, ..r }
        };
        let saved = self.layout.get(id).cloned().or_else(|| match id {
            "timers_recasts" => self.layout.get("timers").cloned(),
            "timers_buffs" => Some(below(1.0)),
            "timers_debuffs" => Some(below(2.0)),
            _ => None,
        });
        let mut p = saved
            .or_else(|| self.retail.get(id).cloned())
            .unwrap_or_else(|| default_place(id, self.ctx.content_rect().size()));
        if !self.layout.contains_key(id) && matches!(id, "cast" | "countdown" | "objective") {
            if let Some(nav) =
                self.this.get("navigation").copied().or_else(|| self.m.rects.borrow().get("navigation").copied())
            {
                let offset = match id {
                    "countdown" => 64.0,
                    "objective" => 128.0,
                    _ => 0.0,
                };
                p.y = p.y.max((nav.bottom() + theme::PANEL_GAP + offset) / self.ctx.content_rect().height().max(1.0));
            }
        }
        // Only an unsaved tracker follows the minimap's bottom; never rewrite a player's placement.
        if id == "quest tracker" && !self.layout.contains_key(id) {
            if let Some(map) =
                self.this.get("minimap").copied().or_else(|| self.m.rects.borrow().get("minimap").copied())
            {
                p.y = p.y.max((map.bottom() + theme::PANEL_GAP) / self.ctx.content_rect().height().max(1.0));
            }
        }
        // A dock on an edge window (saved before they refused them) gives way to the place itself.
        if p.dock.as_ref().is_some_and(|d| EDGE.contains(&d.to.as_str())) {
            p.dock = None;
        }
        if BOTTOM.contains(&id) && p.b <= 0.0 {
            p.b = default_place(id, self.ctx.content_rect().size()).b;
        }
        p
    }

    /// The resizable panel's room: its saved width and height, else `size`. The chat window's width stays on the screen
    /// and clear of the party window (chat_width).
    fn room(&self, id: &str, size: Vec2) -> Vec2 {
        let p = self.place(id);
        let (w, mut h) = (if p.w > 0.0 { p.w } else { size.x }, if p.h > 0.0 { p.h } else { size.y });
        if id == "chat" {
            // Not taller than the room under the top bar (a height saved on a bigger window).
            let most = self.ctx.content_rect().height() * p.b - theme::BELOW_BAR - chat::chrome();
            h = h.min(most.max(60.0));
        }
        vec2(if id == "chat" { self.chat_width(w, h, &p) } else { w }, h)
    }

    /// The chat window's width: `w` (saved, or the default's) cut to the screen's room right of its left edge, and to
    /// the room left of the party window where that stands in the window's way (its lines, tab row and gear were
    /// drawn under the party's rows). Ours: the party window may be anywhere, and what a player saved is theirs
    /// otherwise; the width saved stays as it was, it only gives way while something stands there.
    fn chat_width(&self, w: f32, lines_h: f32, place: &Place) -> f32 {
        use theme::{PANEL_GAP, SCREEN_GAP};
        const LEAST: f32 = 240.0;
        let s = self.ctx.content_rect();
        let rects = self.m.rects.borrow();
        let left = rects.get("chat").map_or(s.width() * place.x.max(0.0), |r| r.min.x);
        let mut room = (s.max.x - SCREEN_GAP - left).max(LEAST);
        // The window's most height: its lines' room and its chrome, as the log rule leaves it at most.
        let bottom = s.height() * place.b;
        let top = bottom - lines_h - chat::chrome();
        if let Some(party) = rects.get("party").filter(|r| r.min.x > left + LEAST && r.max.y > top && r.min.y < bottom)
        {
            room = room.min(party.min.x - PANEL_GAP - left);
        }
        w.min(room).max(LEAST)
    }

    /// A panel's top left for its height `h`: on the panel it is docked on, else from its place.
    fn resolve(&self, id: &str, place: &Place, h: f32) -> Pos2 {
        if let Some(d) = &place.dock
            && let Some(t) = self.target(&d.to, 0)
        {
            return pos2(d.x(t) + d.dx, t.min.y - d.gap - h);
        }
        let s = self.ctx.content_rect();
        // A bottom-placed panel's bottom on a whole pixel (its height changing must not move it).
        let ppp = self.ctx.pixels_per_point();
        let y =
            if BOTTOM.contains(&id) { (s.height() * place.b * ppp).round() / ppp - h } else { s.height() * place.y };
        if place.x >= 0.0 {
            return pos2(s.width() * place.x, y);
        }
        // From the right edge (its width as last drawn), and left of the command column there while it is open, as
        // retail's minimap keeps left of the windows at the edge.
        let rects = self.m.rects.borrow();
        let w = rects.get(id).map_or(0.0, |r| r.width());
        let mut right = s.width() * (1.0 + place.x);
        if !COLUMN.contains(&id) {
            for r in COLUMN.iter().filter_map(|c| self.this.get(*c).or(rects.get(*c))) {
                if r.max.x > right - w && r.min.y < y + h.max(1.0) && r.max.y > y {
                    right = right.min(r.min.x - 4.0);
                }
            }
        }
        pos2(right - w, y)
    }

    /// The rect a panel docks on: drawn this frame, else last frame, else (not shown) no height where it would sit,
    /// so what sits on it falls to what it sits on.
    fn target(&self, id: &str, depth: u8) -> Option<Rect> {
        if depth > 8 {
            return None;
        }
        if let Some(r) = self.this.get(id) {
            return Some(*r);
        }
        if let Some(r) = self.m.rects.borrow().get(id).copied() {
            return Some(r);
        }
        let place = self.place(id);
        let at = match &place.dock {
            Some(d) => {
                let t = self.target(&d.to, depth + 1)?;
                pos2(d.x(t) + d.dx, t.min.y - d.gap)
            }
            None => self.resolve(id, &Place { dock: None, ..place }, 0.0),
        };
        Some(Rect::from_min_size(at, Vec2::ZERO))
    }

    /// Whether `a` sits on `b`, directly or through others.
    fn rests_on(&self, a: &str, b: &str) -> bool {
        let mut cur = a.to_string();
        for _ in 0..8 {
            match self.place(&cur).dock {
                Some(d) if d.to == b => return true,
                Some(d) => cur = d.to,
                None => return false,
            }
        }
        false
    }

    /// A panel let go with its bottom within 5 pixels of another's top, over it, docks there and keeps the distance.
    fn dock_for(&self, id: &str, r: Rect) -> Option<Dock> {
        let tol = 5.0 / self.ctx.pixels_per_point();
        let last = self.m.rects.borrow();
        let mut best: Option<(f32, Dock)> = None;
        for (to, t) in self.this.iter().chain(last.iter().filter(|(k, _)| !self.this.contains_key(*k))) {
            let gap = t.min.y - r.max.y;
            if to == id
                || EDGE.contains(&to.as_str())
                || gap.abs() > tol
                || r.max.x <= t.min.x
                || r.min.x >= t.max.x
                || self.rests_on(to, id)
            {
                continue;
            }
            if best.as_ref().is_none_or(|(g, _)| gap.abs() < *g) {
                let right = self.place(to).x < 0.0;
                best = Some((
                    gap.abs(),
                    Dock { to: to.clone(), gap, dx: r.min.x - if right { t.max.x } else { t.min.x }, right },
                ));
            }
        }
        best.map(|b| b.1)
    }

    /// The panel itself: the pack's window frame behind its title row and contents, moved by a drag on its background
    /// unless placed beside another; a resizable one has the resize glyph in its corner. Returns its rect.
    fn show(&mut self, id: &str, o: Show, add: impl FnOnce(&mut Ui, Vec2)) -> Option<Rect> {
        // Hidden by the mode (modes.rs) or by a Lua mod (ui.native.hide): not drawn, so what docks on it falls to
        // what it sits on.
        // A mod's place (ui.native.place) fades it too, and its own opacity (per-window opacity); faded out, it is
        // not drawn either.
        let placed = self.placed.get(id).copied();
        let vis = self.vis(id) * placed.map_or(1.0, |p| p.alpha) * self.opacity.get(id).copied().unwrap_or(1.0);
        if self.hidden.contains(&id) {
            // Where it would stand now at its last size, for a mod's replacement (ui.native.rect, HUD anchors).
            let size = self.m.sizes.borrow().get(id).copied();
            let h = o.height.or(size.map(|s| s.y)).unwrap_or(0.0);
            let at = o.at.unwrap_or_else(|| self.resolve(id, &self.place(id), h));
            self.m
                .ghosts
                .borrow_mut()
                .insert(id.to_string(), Rect::from_min_size(at, vec2(size.map_or(o.size.x, |s| s.x), h)));
            return None;
        }
        if vis <= 0.01 {
            return None;
        }
        let (m, ctx) = (self.m, self.ctx);
        let screen = ctx.content_rect();
        let mut place = self.place(id);
        let mut room = if o.resizable { self.room(id, o.size).max(o.least) } else { o.size };
        // A size saved on a bigger window shows within this one (the saved size stays). The chat window keeps its own
        // rules (room, chat_width).
        if o.resizable && id != "chat" {
            room = room.min(screen.size() - vec2(16.0, 120.0)).max(o.least);
        }
        let h = o.height.or_else(|| m.rects.borrow().get(id).map(|r| r.height())).unwrap_or(0.0);
        // A mod's top left (window pixels, negative from the right or bottom edge) makes it a panel at a fixed point:
        // the player's drag no longer moves it, and its own place stays saved.
        let ppp = ctx.pixels_per_point();
        let at = o.at.or_else(|| {
            let p = placed.filter(|p| p.x.is_some() || p.y.is_some())?;
            let own = self.resolve(id, &place, h);
            let side =
                |v: Option<f32>, edge: f32, own: f32| v.map_or(own, |v| if v < 0.0 { edge + v / ppp } else { v / ppp });
            Some(pos2(side(p.x, screen.max.x, own.x), side(p.y, screen.max.y, own.y)))
        });
        let mut pos = at.unwrap_or_else(|| self.resolve(id, &place, h));
        // The command window steps right of a retail window it would hide (they draw under every panel): the item
        // pickers a page opens at the top left (Set Bazaar's prices, the hand-over).
        if id == "window" && at.is_none() {
            let size = vec2(o.size.x.max(1.0), h.max(1.0));
            for _ in 0..4 {
                let r = Rect::from_min_size(pos, size).shrink(2.0);
                let Some(right) =
                    m.retail.borrow().iter().filter(|f| f.intersects(r)).map(|f| f.max.x).reduce(f32::max)
                else {
                    break;
                };
                pos.x = right + 8.0;
            }
        }
        // A panel running past the screen's bottom rises until it fits (its place stays): rows under the edge could be
        // walked to but not seen.
        if at.is_none() && o.height.is_none() && !BOTTOM.contains(&id) && h > 0.0 {
            pos.y = pos.y.min(screen.max.y - h).max(screen.min.y);
        }
        // A panel placed by its exact height is kept on screen here, by that height: egui keeps an area on screen by its
        // size last frame, and shrinking against the screen's bottom (the chat window at its default place) that
        // lifted it by the height just lost, a pixel or a few more or less every frame, so its bottom edge shook.
        let exact = at.is_none() && o.height.is_some();
        if exact {
            let w = if o.resizable { room.x } else { m.rects.borrow().get(id).map_or(o.size.x, |r| r.width()) };
            pos = on_screen(Rect::from_min_size(pos, vec2(w, h)), screen);
        }
        let tracker = id == "quest tracker" && o.at.is_none();
        let mut tracker_body_limit = None;
        if tracker {
            let natural = m.tracker_height.get().max(h).max(240.0);
            let width = if place.w > 0.0 { place.w } else { room.x };
            let height = if place.h > 0.0 { place.h } else { natural };
            let wanted = Rect::from_min_size(pos, vec2(width, height));
            let dragging = m.dragging.borrow().as_deref() == Some(id)
                || ctx.is_being_dragged(Id::unique(("modern panel", id)).with("move"));
            let planned = if dragging {
                wanted
            } else {
                let obstacles: Vec<_> = ["party", "target", "minimap", "widescan", "chat"]
                    .iter()
                    .filter_map(|name| self.this.get(*name).copied())
                    .collect();
                tracker_layout::place(wanted, screen, &obstacles)
            };
            pos = planned.min;
            room.x = planned.width();
            tracker_body_limit = Some(
                (planned.height()
                    - if compact() { COMPACT_MARGIN } else { MARGIN }.sum().y
                    - if o.title.is_some() { title_h() } else { 0.0 })
                .max(20.0),
            );
        }
        let tight = compact() && HUD_PANELS.contains(&id);
        let margin = match (o.resizable, tight) {
            (true, false) => ROOM_MARGIN,
            (false, false) => MARGIN,
            (true, true) => COMPACT_ROOM_MARGIN,
            (false, true) => COMPACT_MARGIN,
        };
        // A fold button on the title of the foldable panels: folded by its own fold or by the Minimal HUD.
        let foldable = compact() && FOLD_BUTTON.contains(&id) && o.title.is_some();
        let folded = foldable && folded_in(self.folded, id, true);
        let skip_body = folded && FOLDABLE.contains(&id);
        let mut fold_clicked = false;
        let (title, frame, alpha, fold) = (o.title, o.frame, o.alpha, o.fold.clamp(0.0, 1.0));
        // The panel with the keys in the focused frame.
        let frame = if frame == "window_normal" && self.focus == Some(id) { "window_focused" } else { frame };
        // A panel at a fixed point may stand partly off screen (the Timers slid in).
        let focused = self.focus.filter(|f| *f == id);
        let interactive = panel_interactive(self.focus, id);
        let order = if modal_panel(self.focus) && focused.is_some() { Order::Foreground } else { Order::Middle };
        let shown = egui::Area::new(Id::unique(("modern panel", id)))
            .order(order)
            .current_pos(pos)
            .movable(interactive && o.at.is_none())
            .constrain(o.at.is_none() && !exact)
            .show(ctx, |ui| {
                ui.multiply_opacity(vis);
                if !interactive {
                    ui.disable();
                }
                // An area offers its contents only last frame's size: room to the screen's bottom, or a page grown taller
                // (a scroll area in it) stays clipped at its first height.
                // Not for the panels placed by their exact height (the chat window folding, the party window).
                if o.height.is_none() && !o.resizable {
                    ui.set_max_height((screen.max.y - pos.y).max(ui.min_rect().height()));
                }
                if fold < 1.0 {
                    let top = pos.y + h * (1.0 - fold);
                    ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_max(
                        pos2(f32::NEG_INFINITY, top),
                        pos2(f32::INFINITY, f32::INFINITY),
                    )));
                }
                let bg = ui.painter().add(Shape::Noop);
                let mut tracker_overflow = 0.0;
                if o.cap > 0.0 {
                    let keep = ui.spacing().item_spacing.y;
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.allocate_exact_size(vec2(room.x, o.cap), Sense::hover());
                    ui.spacing_mut().item_spacing.y = keep;
                }
                let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                    ui.set_width(room.x - margin.sum().x);
                    ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
                    if let Some(t) = &title {
                        if let Some(focus) =
                            focused.filter(|f| !matches!(*f, "chat" | "confirm" | "raise" | "ko" | "options"))
                        {
                            ui.horizontal(|ui| {
                                let title_width = (ui.available_width() - 62.0).max(40.0);
                                ui.allocate_ui(vec2(title_width, 28.0), |ui| title_row(ui, m, t));
                                if craft::button(ui, m, "Back", 54.0, false, false) {
                                    m.back_clicked.set(Some(focus));
                                }
                            });
                        } else if foldable {
                            ui.horizontal(|ui| {
                                let title_width = (ui.available_width() - 22.0 - ui.spacing().item_spacing.x).max(40.0);
                                ui.allocate_ui(vec2(title_width, 28.0), |ui| title_row(ui, m, t));
                                fold_clicked = fold_button(ui, m, folded);
                            });
                        } else {
                            title_row(ui, m, t);
                        }
                        ui.add_space(if tight { 3.0 } else { 6.0 });
                    }
                    if skip_body {
                        // Folded to the title bar: the body is not drawn at all.
                    } else if let Some(limit) = tracker_body_limit {
                        let body = egui::ScrollArea::vertical()
                            .id_salt("tracker body viewport")
                            .max_height(limit)
                            .min_scrolled_height(20.0)
                            .show(ui, |ui| add(ui, room));
                        tracker_overflow = (body.content_size.y - body.inner_rect.height()).max(0.0);
                    } else {
                        add(ui, room);
                    }
                    if title.is_none()
                        && let Some(focus) = focused.filter(|f| {
                            !matches!(*f, "chat" | "confirm" | "raise" | "ko" | "event input" | "choice" | "gm prompt")
                        })
                    {
                        if craft::button(ui, m, "Back", 110.0, false, false) {
                            m.back_clicked.set(Some(focus));
                        }
                    }
                });
                let r = inner.response.rect;
                if tracker {
                    m.tracker_height.set(r.height() + tracker_overflow);
                }
                let showing = Rect::from_min_max(pos2(r.min.x, r.max.y - r.height() * fold), r.max);
                ui.painter().set(bg, paint::faded(borders::window(m, frame, showing), alpha));
                if !o.resizable {
                    return None;
                }
                // The glyph sits left of the frame's corner ornament; the whole corner takes the drag.
                let g = ui.interact(
                    Rect::from_min_max(r.max - vec2(44.0, 28.0), r.max),
                    Id::unique(("modern grip", id)),
                    Sense::drag(),
                );
                let lit = g.hovered() || g.dragged();
                if let Some(s) = m.art.sprite("controls/resize") {
                    let c = if lit { Color32::WHITE } else { Color32::from_gray(170) };
                    ui.painter().add(paint::image(&s, r.max - vec2(40.0, 24.0), 16.0, c.gamma_multiply(alpha)));
                }
                if lit {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                }
                g.dragged().then(|| g.drag_delta())
            });
        if fold_clicked {
            toggle_fold(self.folded, id);
            *self.changed = true;
        }
        // The menu's windows draw over the edge windows (they stay where they were put).
        if COLUMN.contains(&id) {
            ctx.move_to_top(shown.response.layer_id);
        }
        // A mod's scale, about the corner it names (the top left by default); the pointer is transformed with it.
        let scale = placed.map_or(1.0, |p| p.scale);
        let (fx, fy) = placed.map_or((0.0, 0.0), |p| p.origin);
        let area = shown.response.rect;
        let pivot = area.min + vec2(area.width() * fx, area.height() * fy);
        let zoom = egui::emath::TSTransform::new(pivot.to_vec2() * (1.0 - scale), scale);
        m.sizes.borrow_mut().insert(id.to_string(), area.size());
        if (scale - 1.0).abs() > 0.001 {
            ctx.set_transform_layer(shown.response.layer_id, zoom);
            m.scaled.borrow_mut().insert(id.to_string());
        } else if m.scaled.borrow_mut().remove(id) {
            ctx.set_transform_layer(shown.response.layer_id, egui::emath::TSTransform::IDENTITY);
        }
        let mut r = shown.response.rect;
        if exact {
            // Dragged past the screen's edge, it is saved where it will show.
            r = Rect::from_min_size(on_screen(Rect::from_min_size(r.min, vec2(r.width(), h)), screen), r.size());
        }
        // A panel placed by its bottom (on another, on the screen's bottom, or risen to fit) is placed by last frame's
        // height, so the frame its contents grow or shrink (the item info moving to a longer or shorter item) it stood
        // that much too low or too high, then jumped. It moves now by the height it was drawn at.
        if at.is_none() && !exact && m.dragging.borrow().as_deref() != Some(id) && (r.height() - h).abs() > 0.5 {
            let top = |h: f32| {
                let y = self.resolve(id, &place, h).y;
                if o.height.is_none() && !BOTTOM.contains(&id) && h > 0.0 {
                    y.min(screen.max.y - h).max(screen.min.y)
                } else {
                    y
                }
            };
            let dy = top(r.height()) - top(h);
            if dy.abs() > 0.01 {
                ctx.transform_layer_shapes(
                    shown.response.layer_id,
                    egui::emath::TSTransform::from_translation(vec2(0.0, dy)),
                );
                r = r.translate(vec2(0.0, dy));
            }
        }
        let showing = zoom * Rect::from_min_max(pos2(r.min.x, r.max.y - r.height() * fold), r.max);
        // Placement trace (debug), only when a panel moves.
        thread_local!(static SEEN: std::cell::RefCell<HashMap<String, Pos2>> = Default::default());
        if log::log_enabled!(log::Level::Debug)
            && SEEN.with(|s| s.borrow_mut().insert(id.to_string(), pos).is_none_or(|p| (p - pos).length() > 0.5))
        {
            log::debug!("panel {id}: pos {pos:?} h {h} rect {r:?} showing {showing:?} dock {:?}", place.dock);
        }
        self.this.insert(id.to_string(), showing);
        if let Some(fh) = o.full {
            self.rooms.insert(id.to_string(), zoom * Rect::from_min_max(pos2(r.min.x, r.max.y - fh), r.max));
        }
        if at.is_some() {
            return Some(showing);
        }
        let mut changed = false;
        let bottom = BOTTOM.contains(&id);
        let (sw, sh) = (screen.width().max(1.0), screen.height().max(1.0));
        // egui drags a movable window from the press: a click that never moves it leaves its place (and dock) alone.
        let moved = shown.response.total_drag_delta().is_some_and(|d| d.length() > 2.0);
        if shown.response.dragged() && (moved || m.dragging.borrow().as_deref() == Some(id)) {
            *m.dragging.borrow_mut() = Some(id.to_string());
            m.arranging.set(true);
            place.dock = None;
            place.x = r.min.x / sw;
            if bottom {
                // From its top and the height it is placed by: a drawn rect a pixel taller crept it down every frame.
                place.b = (r.min.y + if h > 0.0 { h } else { r.height() }) / sh;
            } else {
                place.y = r.min.y / sh;
            }
            changed = true;
        }
        if shown.response.drag_stopped() && m.dragging.borrow().as_deref() == Some(id) {
            *m.dragging.borrow_mut() = None;
            place.dock = self.dock_for(id, r);
            changed = true;
        }
        // A resize moves the bottom with the pointer: a bottom-placed panel keeps its top.
        if let Some(g) = shown.inner {
            (place.w, place.h) = ((room.x + g.x).max(o.least.x), (room.y + g.y).max(o.least.y));
            if bottom && place.dock.is_none() {
                place.b = (r.min.y + if h > 0.0 { h } else { r.height() } + g.y) / sh;
            }
            m.arranging.set(true);
            changed = true;
        }
        if changed {
            self.layout.insert(id.to_string(), place);
            *self.changed = true;
        }
        Some(showing)
    }
}

impl Modern {
    /// Whether keyboard list `key`'s cursor moved to `at` since last frame: the row then scrolls into view (only then,
    /// so the wheel stays free).
    pub(super) fn moved(&self, key: &'static str, at: usize) -> bool {
        self.cursors.borrow_mut().insert(key, at) != Some(at)
    }

    /// A tooltip (ours) while the pointer rests on `r` and `ui`'s layer is the one under it: `title` in gold over `body`
    /// in the pack's tooltip frame.
    pub fn tip(&self, ui: &Ui, r: Rect, title: &str, body: &str) {
        self.tip_with(ui, r, || (title.to_string(), body.to_string()));
    }

    /// `tip` with its text made only while the pointer is on `r`.
    pub fn tip_with(&self, ui: &Ui, r: Rect, text: impl FnOnce() -> (String, String)) {
        let ctx = ui.ctx();
        let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        if r.contains(at) && ctx.layer_id_at(at) == Some(ui.layer_id()) {
            let (title, body) = text();
            self.tip_at(ctx, at, &title, &body);
        }
    }

    /// The tooltip by the pointer at `at`, once it has rested a moment (none while dragging).
    pub fn tip_at(&self, ctx: &Context, at: Pos2, title: &str, body: &str) {
        const REST: f32 = 0.35;
        if title.is_empty() && body.is_empty() || ctx.dragged_id().is_some() {
            return;
        }
        let still = ctx.input(|i| i.pointer.time_since_last_movement());
        if still < REST {
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(REST - still));
            return;
        }
        egui::Area::new(Id::new("modern tip"))
            .order(Order::Tooltip)
            .fixed_pos(at + vec2(16.0, 20.0))
            .constrain(true)
            .interactable(false)
            .show(ctx, |ui| {
                let bg = ui.painter().add(Shape::Noop);
                // Clear room inside the frame's border art all the way round.
                let inset = self.art.frame("window_tooltip").map_or(6.0, |f| f.inset);
                let margin = egui::Margin::symmetric((inset + 12.0).round() as i8, (inset + 10.0).round() as i8);
                let inner = egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    if !title.is_empty() {
                        ui.label(
                            egui::RichText::new(title)
                                .size(self.theme.ui)
                                .family(FontFamily::Name(STRONG.into()))
                                .color(self.theme.color("gold")),
                        );
                    }
                    if !body.is_empty() {
                        gametext::label(ui, self, body, self.theme.small, self.theme.color("text"));
                    }
                });
                ui.painter().set(bg, borders::window(self, "window_tooltip", inner.response.rect));
            });
    }

    fn grid(&self, w: &World, gu: &GameUi) -> String {
        let mut g = self.grid.borrow_mut();
        if g.0.is_none_or(|t| t.elapsed().as_secs_f32() >= 0.5) {
            *g = (Some(std::time::Instant::now()), display_grid(super::map::grid_text(w, gu.map_regions())));
        }
        g.1.clone()
    }

    /// A menu sprite from the game's files (a list's spell or ability icon) as an egui texture, made once.
    fn sprite_icon(&self, ctx: &Context, gu: &GameUi, lib: &str, index: usize) -> Option<egui::TextureId> {
        let key = format!("sprite {lib}:{index}");
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let (w, h, rgba) = gu.sprite_rgba(lib, index)?;
            Some(ctx.load_texture(
                key,
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                egui::TextureOptions::LINEAR,
            ))
        });
        t.as_ref().map(|t| t.id())
    }

    /// A mod's chat icon (chaticons.rs) as an egui texture, made once.
    pub fn chat_icon(&self, ctx: &Context, k: usize) -> Option<egui::TextureId> {
        let key = super::chaticons::texture(k);
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let (w, h, rgba) = super::chaticons::get(k)?;
            Some(ctx.load_texture(
                key,
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                egui::TextureOptions::LINEAR,
            ))
        });
        t.as_ref().map(|t| t.id())
    }

    /// A name sprite's caption as text, read once.
    fn caption(&self, gu: &GameUi, lib: &str, index: usize) -> Option<String> {
        self.captions
            .borrow_mut()
            .entry((lib.to_string(), index))
            .or_insert_with(|| gu.sprite_caption(lib, index))
            .clone()
    }

    /// An item's icon from the game's files (32 x 32) as an egui texture, made once.
    fn item_icon(&self, ctx: &Context, info: &super::items::ItemCache, id: u16) -> Option<egui::TextureId> {
        use crate::inventory::ItemInfo;
        let key = format!("item {id}");
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let rgba = info.icon(id)?;
            Some(ctx.load_texture(
                key,
                egui::ColorImage::from_rgba_unmultiplied([32, 32], &rgba),
                egui::TextureOptions::LINEAR,
            ))
        });
        t.as_ref().map(|t| t.id())
    }

    /// Ours: no line of the macro that names a spell or ability (/ma, /ja, /pet, /ws) can be used now; one with none
    /// never greys.
    fn macro_off(&self, w: &World, m: &super::macros::Macro) -> bool {
        let mut any = false;
        let mut acts = self.macro_acts.borrow_mut();
        for k in 0..6 {
            let line = crate::dat::font::decode_sjis(m.line(k));
            if line.is_empty() {
                continue;
            }
            let act = match acts.get(&line) {
                Some(a) => *a,
                // Not kept before the name tables load: every line would read as no action.
                None if w.names.is_none() || w.actions.is_none() => continue,
                None => *acts.entry(line.clone()).or_insert_with(|| super::textcmd::line_action(w, &line)),
            };
            let Some((kind, id)) = act else { continue };
            any = true;
            if super::magic::action_ready(w, kind, id) {
                return false;
            }
        }
        any
    }

    /// A status icon and its uv: the pack's (status_map.tsv, ours), else the game's (config 0x99 bit 2: the small set),
    /// made once.
    fn status_icon(&self, ctx: &Context, gu: &GameUi, small: bool, id: u16) -> Option<(egui::TextureId, Rect)> {
        if let Some(s) = status_art().get(&id).and_then(|name| self.art.sprite(&format!("status/{name}"))) {
            return Some((s.tex, s.uv));
        }
        let key = format!("status {}:{id}", small as u8);
        let mut images = self.images.borrow_mut();
        let t = images.entry(key.clone()).or_insert_with(|| {
            let (w, h, rgba) = gu.status_rgba(small, id)?;
            Some(ctx.load_texture(
                key,
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                egui::TextureOptions::LINEAR,
            ))
        });
        t.as_ref().map(|t| (t.id(), FULL_UV))
    }

    /// A spell's or ability's list icon and its uv: ours for its picture (magic::picture), else the game's sprite.
    fn list_icon(
        &self,
        ctx: &Context,
        gu: &GameUi,
        lib: &str,
        index: usize,
        picture: Option<u16>,
    ) -> Option<(egui::TextureId, Rect)> {
        if let Some(s) = picture.and_then(action_art).and_then(|id| self.art.sprite(id)) {
            return Some((s.tex, s.uv));
        }
        self.sprite_icon(ctx, gu, lib, index).map(|t| (t, FULL_UV))
    }

    /// A magic or ability list row's icon (ours for spells and abilities, the game's for mounts and emotes).
    fn row_icon(
        &self,
        ctx: &Context,
        gu: &GameUi,
        w: &World,
        kind: super::magic::Kind,
        id: u16,
    ) -> Option<(egui::TextureId, Rect)> {
        let (lib, k) = super::magic::row_icon(w, kind, id)?;
        self.list_icon(ctx, gu, lib, k, super::magic::picture(w, kind, id))
    }
}

/// A whole texture's uv.
const FULL_UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

/// Our art for a spell's or ability's list picture (magic::picture; assets/modern/action_map.tsv, alike pictures
/// sharing one), as a manifest id.
pub fn action_art(picture: u16) -> Option<&'static str> {
    static MAP: std::sync::OnceLock<HashMap<u16, String>> = std::sync::OnceLock::new();
    let map = MAP.get_or_init(|| {
        include_str!("../../../assets/modern/action_map.tsv")
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .filter_map(|(k, n)| Some((k.parse().ok()?, format!("actions/{n}"))))
            .collect()
    });
    map.get(&picture).map(String::as_str)
}

/// A numbered effect's count (Copy Image, Finishing Move, Aftermath, the Dazes), drawn as text over our art, which
/// leaves it out; none for a retail icon (its count is in the image).
fn status_count(id: u16) -> Option<&'static str> {
    if !status_art().contains_key(&id) {
        return None;
    }
    Some(match id {
        66 | 270 | 381 => "1",
        271 | 382 | 387 | 392 | 397 | 444 => "2",
        272 | 383 | 388 | 393 | 398 | 445 => "3",
        384 | 389 | 394 | 399 => "4",
        385 | 390 | 395 | 400 => "5",
        446 => "4+",
        _ => return None,
    })
}

/// Draws effect `id`'s count, if it has one, on the icon at `r`'s lower right corner: about half the icon's height,
/// at least 9 points.
fn draw_status_count(painter: &egui::Painter, r: Rect, id: u16) {
    if let Some(n) = status_count(id) {
        corner_number(painter, r, n, Color32::WHITE);
    }
}

/// A number on an icon's lower right corner (Utsusemi's shadows, a party member's level).
fn corner_number(painter: &egui::Painter, r: Rect, n: &str, colour: Color32) {
    let font = FontId::new((r.height() * 0.54).clamp(9.0, 16.0), FontFamily::Name(STRONG.into()));
    paint::outlined(painter, pos2(r.max.x + 1.0, r.max.y + 2.0), egui::Align2::RIGHT_BOTTOM, n, font, colour);
}

/// Our status icons by status id (assets/modern/status_map.tsv: ids sharing a retail image share ours).
fn status_art() -> &'static HashMap<u16, &'static str> {
    static MAP: std::sync::OnceLock<HashMap<u16, &'static str>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        include_str!("../../../assets/modern/status_map.tsv")
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .filter_map(|(id, n)| Some((id.parse().ok()?, n)))
            .collect()
    })
}

fn title_row(ui: &mut Ui, m: &Modern, t: &Title) {
    title_row_in(ui, m, t, m.theme.color("gold"));
}

/// A title row in another colour (the target's name in its retail name colour).
///
/// Ours: the note keeps to the room the title leaves (a long target name over "Commands" ran under the title). A name
/// note breaks in two as retail's target window breaks a name (FUN_101c9fa0, see `name_lines`), and the row grows to
/// hold both lines; another note is cut. A title too long for the row leaves the note at least 40% of it.
fn title_row_in(ui: &mut Ui, m: &Modern, t: &Title, colour: egui::Color32) {
    const GAP: f32 = 12.0;
    let width = ui.available_width();
    let icon = t.icon.and_then(|i| m.art.sprite(i));
    let lead = if icon.is_some() { 34.0 } else { 0.0 };
    let font = FontId::new(m.theme.title, FontFamily::Name(STRONG.into()));
    let small = FontId::proportional(m.theme.small);
    let muted = m.theme.color("muted");
    let painter = ui.painter().clone();
    let measure = |s: &str| painter.layout_no_wrap(s.to_string(), small.clone(), muted).size().x;
    let full = painter.layout_no_wrap(t.text.to_string(), font.clone(), colour).size().x;
    let note_w = t.note.map(measure).unwrap_or(0.0);
    let title_room =
        (if t.note.is_some() { width - lead - GAP - note_w.min(0.4 * width) } else { width - lead }).max(0.0);
    let title = one_line(&painter, t.text, font, colour, title_room);
    let room = (width - lead - full.min(title_room) - GAP).max(0.0);
    let lines = match t.note {
        Some(n) if t.name => {
            let mut cache = m.name_split.borrow_mut();
            match &*cache {
                Some((s, r, l)) if s == n && *r == room.to_bits() => l.clone(),
                _ => {
                    let l = name_lines(n, room, &measure);
                    *cache = Some((n.to_string(), room.to_bits(), l.clone()));
                    l
                }
            }
        }
        Some(n) => vec![n.to_string()],
        None => Vec::new(),
    };
    let line_h = painter.layout_no_wrap("Ag".into(), small.clone(), muted).size().y;
    let h = 28.0f32.max(lines.len() as f32 * line_h + 2.0);
    let (r, _) = ui.allocate_exact_size(vec2(width, h), egui::Sense::hover());
    if let Some(s) = icon {
        painter.add(paint::image(&s, pos2(r.min.x, r.center().y - 13.0), 26.0, egui::Color32::WHITE));
    }
    painter.galley(pos2(r.min.x + lead, r.center().y - title.size().y / 2.0), title, colour);
    let top = r.center().y - lines.len() as f32 * line_h / 2.0;
    for (k, l) in lines.iter().enumerate() {
        let g = one_line(&painter, l, small.clone(), muted, room);
        painter.galley(pos2(r.max.x - g.size().x, top + k as f32 * line_h), g, muted);
    }
    // Ours: a rule under every title, fading out to the right (the panels' one chrome).
    paint::fade_rule(&painter, r.min.x, r.max.x, r.max.y + 2.0, m.theme.color("bronze").gamma_multiply(0.6));
}

/// A name in at most two lines of `room`, as retail's target window breaks one (FUN_101c9fa0): one line when it fits
/// and has no ':'; else broken after the first ':' before the fit, else after the last ' ', '-', '@', ''' or ':' from
/// the fit down to a quarter of it, else at the fit; when what is left still does not fit, cut at the fit, and the
/// rest ends in an ellipsis (`one_line` when drawn). Ours: measured in chars by `measure`, not fontshp bytes, and the
/// search starts at the fit, not retail's fit + 1 (which can leave the first line one char over).
fn name_lines(s: &str, room: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let text = |a: usize, b: usize| c[a..b].iter().collect::<String>();
    let fit = (0..=c.len()).take_while(|&n| measure(&text(0, n)) <= room).last().unwrap_or(0);
    let colon = c.iter().position(|&ch| ch == ':');
    if fit >= c.len() && colon.is_none() {
        return vec![s.to_string()];
    }
    let brk = |ch: char| matches!(ch, ' ' | '-' | '@' | '\'' | ':');
    let cut = match colon {
        Some(i) if i < fit && i + 1 != fit => i + 1,
        _ => (fit / 4..=fit.min(c.len().saturating_sub(1))).rev().find(|&i| brk(c[i])).map_or(fit, |i| i + 1),
    };
    let cut = cut.clamp(1, c.len().max(1));
    let cut = if measure(text(cut, c.len()).trim_start()) <= room { cut } else { fit.max(1).min(c.len()) };
    let first = text(0, cut).trim_end().to_string();
    let rest = text(cut, c.len()).trim_start().to_string();
    if rest.is_empty() { vec![first] } else { vec![first, rest] }
}

/// `text` on one line of at most `width`, cut with an ellipsis.
pub(super) fn one_line(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, colour);
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    painter.layout_job(job)
}

#[cfg(test)]
mod controls_tests {
    use super::*;

    #[test]
    fn forever_controls_prompt_buttons_accept_cancel_and_respect_disabled_ui() {
        let ctx = Context::default();
        let modern = Modern::new(&ctx);
        let frame = |events: Vec<egui::Event>, enabled: bool| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(500.0, 200.0))),
                events,
                ..Default::default()
            });
            let mut action = None;
            egui::Area::new(Id::unique("controls-test")).fixed_pos(Pos2::ZERO).movable(false).show(&ctx, |ui| {
                ui.set_min_size(vec2(500.0, 200.0));
                if !enabled {
                    ui.disable();
                }
                action = input_buttons(ui, &modern, "Accept", "Cancel");
            });
            let _ = ctx.end_pass();
            action
        };
        frame(vec![], true);
        frame(vec![], true);
        for (x, expected) in [(45.0, egui::Key::Enter), (165.0, egui::Key::Escape)] {
            // egui hit-testing uses the previous frame's enabled widget rectangles.
            frame(vec![], true);
            let pos = pos2(x, 28.0);
            let pointer = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(pos), pointer(true)], true);
            assert_eq!(frame(vec![pointer(false)], true), Some(expected));
            frame(vec![pointer(true)], false);
            assert_eq!(frame(vec![pointer(false)], false), None);
        }
    }

    #[test]
    fn game_menu_modal_keeps_background_disabled_and_confirmation_on_top() {
        assert!(panel_interactive(Some("game menu"), "game menu"));
        for background in ["chat", "command", "combat", "training menu"] {
            assert!(!panel_interactive(Some("game menu"), background));
        }
        assert!(!panel_interactive(Some("confirm"), "game menu"));
        assert!(panel_interactive(Some("confirm"), "confirm"));
    }

    #[test]
    fn forever_controls_modal_blocks_background_panel_actions() {
        for focus in ["game menu", "confirm", "raise", "event input", "msgline", "choice", "gm prompt", "options"] {
            assert!(panel_interactive(Some(focus), focus));
            assert!(!panel_interactive(Some(focus), "command"));
            assert!(!panel_interactive(Some(focus), "combat"));
            assert!(!panel_interactive(Some(focus), "chat"));
        }
        assert!(panel_interactive(None, "command"));
        assert!(panel_interactive(Some("combat"), "chat"));
    }
}

/// An unavailable map coordinate is not a level range and adds no useful HUD text.
fn display_grid(grid: String) -> String {
    if grid == "(?-?)" { String::new() } else { grid }
}

#[cfg(test)]
mod grid_tests {
    #[test]
    fn forever_ui_omits_unknown_map_grid() {
        assert_eq!(super::display_grid("(?-?)".into()), "");
        assert_eq!(super::display_grid("(E-8)".into()), "(E-8)");
        assert_eq!(super::display_grid(String::new()), "");
    }
}

#[cfg(test)]
mod display_layout_tests {
    use super::{display_key, layout_of, name_lines};

    #[test]
    fn long_names_break_like_the_target_window() {
        // 10 px a char.
        let w = |s: &str| 10.0 * s.chars().count() as f32;
        assert_eq!(name_lines("Goblin", 100.0, &w), vec!["Goblin"]);
        assert_eq!(name_lines("Ophiotaurus of the Western", 120.0, &w), vec!["Ophiotaurus", "of the Western"]);
        assert_eq!(name_lines("Qu'Bia: Arena", 200.0, &w), vec!["Qu'Bia:", "Arena"]);
        assert_eq!(name_lines("Abcdefghijklmnopqrstuvwxyz", 100.0, &w), vec!["Abcdefghij", "klmnopqrstuvwxyz"]);
    }

    #[test]
    fn display_layouts_belong_to_their_character() {
        let k = display_key("Naji", [2560, 1440], 60);
        assert_eq!(k, "Naji @2560x1440 60%");
        assert!(layout_of(&k, "Naji") && layout_of("Naji", "Naji"));
        assert!(!layout_of(&k, "Naj") && !layout_of("Najim @1920x1080 60%", "Naji"));
    }

    /// Every icon the picture map names is in an atlas, its SVG in the pack.
    #[test]
    fn action_art_is_packed() {
        let svg = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/modern/svg");
        for l in include_str!("../../../assets/modern/action_map.tsv").lines() {
            let (k, _) = l.split_once('\t').unwrap();
            let id = super::action_art(k.parse().unwrap()).unwrap();
            assert!(super::sprite_pixels(id).is_some_and(|(w, h, _)| (w, h) == (128, 128)), "{id}");
            assert!(svg.join(format!("{id}.svg")).exists(), "{id}");
        }
    }
}

#[cfg(test)]
mod panel_tests {
    use super::*;

    /// A docked panel whose contents grow or shrink keeps its bottom on the panel it sits on the same frame (the item
    /// info moving between a long and a short item stood a frame too high or too low).
    #[test]
    fn docked_panel_keeps_its_bottom_as_it_resizes() {
        let ctx = egui::Context::default();
        let side = Some(16384);
        let mut o = ctx.run_ui(egui::RawInput { max_texture_side: side, ..Default::default() }, |_| {});
        o.textures_delta.clear();
        let m = Modern::new(&ctx);
        let mut layout = Layout::new();
        let mut bottoms = Vec::new();
        for (f, n) in [12usize, 12, 12, 2, 2, 12, 12, 1, 7].into_iter().enumerate() {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0))),
                time: Some(f as f64 * 0.1),
                max_texture_side: side,
                ..Default::default()
            };
            let mut info = None;
            let mut o = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx();
                let (mut changed, mut chat_font) = (false, 14.0);
                let mut p = Panels {
                    m: &m,
                    ctx,
                    gu: None,
                    layout: &mut layout,
                    vis: Default::default(),
                    changed: &mut changed,
                    chat_font: &mut chat_font,
                    battle_in_all: &mut true,
                    condense: &mut true,
                    chat_hidden: &mut Vec::new(),
                    folded: &mut Default::default(),
                    scan_kinds: &mut 7,
                    this: Default::default(),
                    rooms: Default::default(),
                    retail: Default::default(),
                    focus: None,
                    hidden: Vec::new(),
                    placed: Default::default(),
                    opacity: Default::default(),
                };
                let combat = p.panel("combat", None, 200.0, |ui| {
                    ui.label("Attack");
                });
                info = p
                    .panel("iteminfo", None, 420.0, |ui| {
                        for k in 0..n {
                            ui.label(format!("line {k}"));
                        }
                    })
                    .zip(combat);
                *m.rects.borrow_mut() = std::mem::take(&mut p.this);
            });
            o.textures_delta.clear();
            // Frame 0 has egui's default screen.
            if f > 0 {
                let (r, under) = info.unwrap();
                bottoms.push((r.max.y, under.min.y));
            }
        }
        let gap = default_place("iteminfo", vec2(1280.0, 720.0)).dock.unwrap().gap;
        for (bottom, under) in bottoms {
            assert!((bottom - (under - gap)).abs() < 0.5, "bottom {bottom} on {under}");
        }
    }
    /// A mod's place (ui.native.place): the panel at its top left (window pixels; negative from the right edge),
    /// scaled about it, and back where it was once released.
    #[test]
    fn mod_placed_panel_moves_and_scales() {
        let ctx = egui::Context::default();
        let side = Some(16384);
        let mut o = ctx.run_ui(egui::RawInput { max_texture_side: side, ..Default::default() }, |_| {});
        o.textures_delta.clear();
        let m = Modern::new(&ctx);
        let mut layout = Layout::new();
        let place =
            |x, scale| super::super::lua::Placed { x: Some(x), y: Some(100.0), scale, alpha: 1.0, origin: (0.0, 0.0) };
        let mut seen = Vec::new();
        // Frame 0 has egui's default screen.
        let corner = super::super::lua::Placed { origin: (1.0, 1.0), ..place(200.0, 2.0) };
        for (f, placed) in [
            None,
            None,
            Some(place(200.0, 1.0)),
            Some(place(200.0, 2.0)),
            Some(place(200.0, 2.0)),
            Some(place(-400.0, 1.0)),
            None,
            Some(corner),
            Some(corner),
        ]
        .into_iter()
        .enumerate()
        {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0))),
                time: Some(f as f64 * 0.1),
                max_texture_side: side,
                ..Default::default()
            };
            let mut o = ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx();
                let (mut changed, mut chat_font) = (false, 14.0);
                let placed = placed.map(|p| HashMap::from([("focus".to_string(), p)])).unwrap_or_default();
                let mut p = Panels {
                    m: &m,
                    ctx,
                    gu: None,
                    layout: &mut layout,
                    vis: Default::default(),
                    changed: &mut changed,
                    chat_font: &mut chat_font,
                    battle_in_all: &mut true,
                    condense: &mut true,
                    chat_hidden: &mut Vec::new(),
                    folded: &mut Default::default(),
                    scan_kinds: &mut 7,
                    this: Default::default(),
                    rooms: Default::default(),
                    retail: Default::default(),
                    focus: None,
                    hidden: Vec::new(),
                    placed,
                    opacity: Default::default(),
                };
                seen.push(p.panel("focus", None, 200.0, |ui| {
                    ui.label("Focus");
                }));
                *m.rects.borrow_mut() = std::mem::take(&mut p.this);
            });
            o.textures_delta.clear();
        }
        let r: Vec<Rect> = seen.into_iter().skip(1).map(Option::unwrap).collect();
        let ppp = ctx.pixels_per_point();
        assert!((r[1].min - pos2(200.0 / ppp, 100.0 / ppp)).length() < 0.5, "{:?}", r[1]);
        // Twice the size from the same top left.
        assert!(
            (r[3].min - r[1].min).length() < 0.5 && (r[3].width() - 2.0 * r[1].width()).abs() < 1.0,
            "{:?} {:?}",
            r[1],
            r[3]
        );
        assert!((r[4].min.x - (1280.0 - 400.0 / ppp)).abs() < 0.5, "{:?}", r[4]);
        // Released: its own place again, at its own size.
        assert!(
            (r[5].min - r[0].min).length() < 0.5 && (r[5].width() - r[0].width()).abs() < 0.5,
            "{:?} {:?}",
            r[0],
            r[5]
        );
        // Scaled about its bottom right: that corner stays.
        assert!(
            (r[7].max - r[1].max).length() < 0.5 && (r[7].width() - 2.0 * r[1].width()).abs() < 1.0,
            "{:?} {:?}",
            r[1],
            r[7]
        );
    }

    /// Mods' sprites keep the pack's logical size, with their own pixels; frames take their colours; released, the
    /// pack's again.
    #[test]
    fn skin_replaces_and_gives_back() {
        let ctx = egui::Context::default();
        let mut o = ctx.run_ui(egui::RawInput { max_texture_side: Some(16384), ..Default::default() }, |_| {});
        o.textures_delta.clear();
        let m = Modern::new(&ctx);
        let base = m.art.sprite("gauges/hp_full").unwrap();
        let img = std::sync::Arc::new((64u32, 8u32, vec![255u8; 64 * 8 * 4]));
        let red = egui::Color32::from_rgb(200, 0, 0);
        m.art.set_skin(
            &ctx,
            &[("gauges/hp_full".into(), ("test hp".into(), img))],
            &[("window_normal".into(), None, Some(red), None)],
        );
        let s = m.art.sprite("gauges/hp_full").unwrap();
        assert_ne!(s.tex, base.tex);
        assert_eq!((s.size, s.margins), (base.size, base.margins));
        assert_eq!(s.px, 64.0 / base.size.x);
        assert_eq!(m.art.frame("window_normal").unwrap().gradient[0], red);
        m.art.set_skin(&ctx, &[], &[]);
        assert_eq!(m.art.sprite("gauges/hp_full").unwrap().tex, base.tex);
        m.theme.set_skin(HashMap::from([("gold".to_string(), red)]));
        assert_eq!(m.theme.color("gold"), red);
    }
}

#[cfg(test)]
mod right_back_tests {
    use super::*;

    /// Which focused panels a right-click tap backs out of: all the Back button's, none of the ones it leaves alone.
    #[test]
    fn back_focus_table() {
        for f in
            ["game menu", "command", "command sub", "window", "list", "cats", "results", "loot", "confirm", "check"]
        {
            assert!(right_back_allowed(f, false, false), "{f}");
        }
        for f in NO_RIGHT_BACK {
            assert!(!right_back_allowed(f, false, false), "{f}");
        }
        // Typing elsewhere (a text field has the keys, a context menu is open) stops every one.
        assert!(!right_back_allowed("window", true, false));
        // Settings backs out only with nothing to lose.
        assert!(right_back_allowed(options::ID, false, false));
        assert!(!right_back_allowed(options::ID, false, true));
        // Unsaved Settings stop their windows (the Settings page, and the others that Escape cancels), not other panels.
        assert!(!right_back_allowed("window", false, true));
        assert!(right_back_allowed("window", false, false));
        assert!(right_back_allowed("command", false, true));
        assert!(right_back_allowed("game menu", false, true));
        // Everything the Back button is not drawn for is on the list, plus what only typing or combat uses.
        for f in ["chat", "confirm", "raise", "ko", "event input", "choice", "gm prompt"] {
            assert_eq!(!NO_RIGHT_BACK.contains(&f), f == "confirm", "{f}");
        }
    }
}
