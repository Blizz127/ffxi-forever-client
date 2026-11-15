//! Retail key functions (FUN_1011ff30): a DIK key state built each frame (FUN_10005520), the binding table
//! 0x103619f0 with its context masks 0x103612e8, and the compact keyboard's virtual pad (FUN_1011fbc0). The tables
//! come from FFXiMain; Key Assignment rewrites them as FUN_10195590 / FUN_10195940 / FUN_10195910 do.

use winit::keyboard::KeyCode;

/// The numpad's movement keys belong to the physical input path. With NumLock off, winit also reports them as
/// logical arrows; forwarding those to egui would move a menu cursor at the same time as the character. Text
/// fields still receive digits with NumLock on. With it off, the existing physical input path permits movement
/// even while typing, so those aliases must not also move the caret or chat history.
pub fn world_ui_key(code: KeyCode, logical: &winit::keyboard::Key) -> bool {
    use winit::keyboard::{Key, NamedKey};
    !(matches!(code, KeyCode::Numpad2 | KeyCode::Numpad4 | KeyCode::Numpad6 | KeyCode::Numpad8)
        && matches!(
            logical,
            Key::Named(NamedKey::ArrowDown | NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::ArrowUp)
        ))
}

/// Key function ids 0x00..0xba (0xb7..0xba test a second pad word, inferred: the gamepad).
pub const IDS: usize = 0xbb;
const K2_ROWS: usize = 29;
/// Key repeat (input +0x94 = 16, FUN_10005210): past 16 ticks held it fires and drops back to 14. Counted in
/// retail frames of 2 ticks, as retail ran.
const REPEAT_DELAY: f32 = 16.0;
const REPEAT_BACK: f32 = 2.0;
const FRAME_TICKS: f32 = 2.0;

/// Key state bits (FUN_10120340..FUN_10120530).
const REPEAT: u8 = 1;
const PRESS: u8 = 2;
const HELD: u8 = 4;
const RELEASE: u8 = 8;

/// Forever: J opens the Journal.
#[cfg(feature = "forever")]
pub const JOURNAL_DIK: u8 = 0x24;
/// Forever: Crafting Log key choices (DIK, egui key, label). Only keys every stock keyboard table leaves free
/// (checked against the player's FFXiMain.dll in `stock_tables_and_window_keys`).
#[cfg(feature = "forever")]
pub const RECIPE_KEYS: &[(u8, egui::Key, &str)] = &[
    (0x16, egui::Key::U, "U"),
    (0x18, egui::Key::O, "O"),
    (0x19, egui::Key::P, "P"),
    (0x22, egui::Key::G, "G"),
    (0x30, egui::Key::B, "B"),
];

/// Key-state modes of FUN_1011f850.
pub mod mode {
    pub const REPEAT: i32 = 0;
    pub const PRESS: i32 = 1;
    pub const REPEAT_CONSUME: i32 = 2;
    pub const PRESS_CONSUME: i32 = 3;
    pub const HELD: i32 = 4;
}

/// Pad bit each key function's callback 0 tests (PTR_FUN_10361e28; MODLOG "Input layer"): the compact keys' and a
/// controller's (client/gamepad.rs) slots.
const PAD: &[(u8, u64)] = &[
    (0x00, 0x40),
    (0x27, 0x40),
    (0x2b, 0x40),
    (0x31, 0x40),
    (0x01, 0x80),
    (0x28, 0x80),
    (0x2c, 0x80),
    (0x32, 0x80),
    (0x02, 0x10),
    (0x29, 0x10),
    (0x2d, 0x10),
    (0x33, 0x10),
    (0x03, 0x20),
    (0x2a, 0x20),
    (0x2e, 0x20),
    (0x34, 0x20),
    (0x0d, 1),
    (0x0e, 1),
    (0x8f, 1),
    (0x91, 1),
    (0x93, 1),
    (0x0f, 8),
    (0x90, 8),
    (0x92, 8),
    (0x94, 8),
    (0x95, 8),
    (0x9c, 8),
    (0x11, 4),
    (0x22, 4),
    (0x78, 4),
    (0x18, 2),
    (0x19, 2),
    (0x25, 2),
    (0x35, 2),
    (0x8d, 2),
    (0x10, 0x4000),
    (0x7d, 0x4000),
    (0xa7, 0x4000),
    (0x12, 0x100),
    (0x15, 0x100),
    (0x1c, 0x100),
    (0x1f, 0x100),
    (0x13, 0x8000),
    (0x14, 0x8000),
    (0x16, 0x200),
    (0x21, 0x200),
    (0x86, 0x200),
    (0x89, 0x200),
    (0x17, 0x2000),
    (0x20, 0x2000),
    (0x85, 0x2000),
    (0x8a, 0x2000),
    (0x9a, 0x2000),
    (0x1a, 0x1000),
    (0x1e, 0x1000),
    (0x24, 0x1000),
    (0x26, 0x10000),
    (0x2f, 0x10000),
    (0x56, 0x10000),
    (0x30, 0x400),
    (0x44, 0x400),
    (0x7e, 0x400),
    (0xa6, 0x400),
    (0x45, 0x2000_0000),
    (0x4f, 0x800_0000),
    (0xa4, 0x800_0000),
    (0x50, 0x1000_0000),
    (0xa5, 0x1000_0000),
    (0x55, 0x4000_0000),
    // The second word (+0x224, slots 32 / 33 / 35 / 36): pad only (MODLOG "Gamepad").
    (0xb7, 1 << 32),
    (0xb8, 1 << 33),
    (0xb9, 1 << 35),
    (0xba, 1 << 36),
];

/// A compact keyboard record (0x103623c8, 0x10 bytes): key, modifier rule, state mask, pad bits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compact {
    pub key: u8,
    pub rule: u8,
    pub states: u8,
    pub bits: u64,
}

/// The game around the keys this frame.
#[derive(Clone, Copy, Default)]
pub struct Frame {
    /// Frame length in 1/60 s ticks.
    pub ticks: f32,
    /// The chat line is open (FUN_101adbd0).
    pub typing: bool,
    /// Keyboard type, config 0x42: 0 full, 1 / 2 compact.
    pub keyboard: u32,
    /// A window is active (FUN_101584c0).
    pub window: bool,
    /// Key Assignment (k1assign / k2assign) is open: the compact keys are off and flag 0x10 fails.
    pub assigning: bool,
    /// Ours: the Modern scheme is on and no menu, list or prompt has the keys (controls.rs).
    pub modern: bool,
    /// Forever navigation canvas: permit movement axes without enabling other map-masked actions.
    #[cfg(feature = "forever")]
    pub navigation_move: bool,
    /// Forever Modern UI and controls, with no focused window/event: bare L owns the tracker.
    #[cfg(feature = "forever")]
    pub tracker_key: bool,
    /// Forever window keys (DIK codes, 0 = off): the Journal (J) and the Crafting Log (N unless changed).
    #[cfg(feature = "forever")]
    pub journal_dik: u8,
    #[cfg(feature = "forever")]
    pub recipe_dik: u8,
}

#[derive(Default)]
struct Pad {
    held: u64,
    edge: u64,
    repeat: u64,
    /// +0x248 and the first-frame flag (+0xda8).
    counter: f32,
    armed: bool,
}

pub struct Input {
    /// Table defaults from FFXiMain and the live copies Key Assignment rewrites.
    base_bindings: Vec<[u16; 3]>,
    base_compact: Vec<Compact>,
    pub bindings: Vec<[u16; 3]>,
    masks: Vec<u16>,
    pub compact: Vec<Compact>,
    k1: Vec<Vec<u8>>,
    k2: Vec<Vec<u16>>,
    /// The config values the tables were last rewritten from.
    assigned: Option<Vec<u32>>,
    keys: [u8; 256],
    /// Device state from the window, and presses seen since the last frame (a tap shorter than a frame counts).
    down: [bool; 256],
    tapped: [bool; 256],
    counters: [f32; 256],
    clock: f32,
    pad: Pad,
    /// The input context (DAT_10362678): 0xffff in the field, 0x26 with the map open, 0x42 laying out the Mog House.
    pub context: u16,
    typing: bool,
    assigning: bool,
    window: bool,
    /// Ours: the Modern scheme's keys count this frame, and the keys Key Assignment gave Q / E (the type 2 turn
    /// ids 0x67 / 0x66).
    modern: bool,
    #[cfg(feature = "forever")]
    navigation_move: bool,
    #[cfg(feature = "forever")]
    tracker_key: bool,
    #[cfg(feature = "forever")]
    panel_diks: [u8; 2],
    turn_keys: [u8; 2],
    /// DAT_104cf22c: a handler took Enter / Esc this frame (FUN_101202a0); only mode 4 tests still pass.
    consumed: bool,
    /// NumLock, from the last numpad digit's logical key (winit reports no toggle state).
    numlock: bool,
    /// The gamepad (client/gamepad.rs): the DIK keys its buttons hold, and its sticks as the axes read them
    /// (movement x / y, camera x / y; y is positive down, as the arrow keys' axes are). The pad is another keyboard
    /// here: the keys go through the same state as the window's.
    pad_down: [bool; 256],
    pad_sticks: [f32; 4],
    /// The gamepad's map zoom (axis 0x7c): the right stick's Y while the map has the pad, nothing else reads it (axis 7,
    /// the camera's, would pitch the view under the map).
    pad_zoom: f32,
    /// A keyboard key went down this frame (before the chat line takes Enter / Esc).
    pressed_any: bool,
    /// A game controller's sticks (client/gamepad.rs, not retail): left X / Y, right X / Y in -1 ..= 1, right and
    /// down positive, as the axes.
    stick: [f32; 4],
    /// A game controller's pad word (client/gamepad.rs: retail's FUN_10005a40 bits), ORed with the compact keys'.
    device: u64,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            base_bindings: vec![[0; 3]; IDS],
            base_compact: Vec::new(),
            bindings: vec![[0; 3]; IDS],
            masks: vec![0; IDS],
            compact: Vec::new(),
            k1: Vec::new(),
            k2: Vec::new(),
            assigned: None,
            keys: [0; 256],
            down: [false; 256],
            tapped: [false; 256],
            counters: [0.0; 256],
            clock: 0.0,
            pad: Pad::default(),
            context: 0xffff,
            typing: false,
            assigning: false,
            window: false,
            modern: false,
            #[cfg(feature = "forever")]
            navigation_move: false,
            #[cfg(feature = "forever")]
            tracker_key: false,
            #[cfg(feature = "forever")]
            panel_diks: [0; 2],
            turn_keys: [0x10, 0x12],
            consumed: false,
            numlock: true,
            pad_down: [false; 256],
            pad_sticks: [0.0; 4],
            pad_zoom: 0.0,
            pressed_any: false,
            stick: [0.0; 4],
            device: 0,
        }
    }
}

impl Input {
    /// The tables from FFXiMain.dll. Missing data leaves them empty: no key function fires.
    pub fn load(&mut self, ffximain: &[u8]) {
        let va = |a: u32, n: usize| crate::dat::battle::va_bytes(ffximain, a, n);
        let Ok(t) = crate::dat::tables::of(ffximain).map_err(|e| log::warn!("key tables: {e:#}")) else { return };
        let u16s = |b: &[u8]| b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<u16>>();
        if let (Some(b), Some(m)) = (va(t.bindings, IDS * 6), va(t.masks, IDS * 2)) {
            self.base_bindings = u16s(b).chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
            self.masks = u16s(m);
        }
        self.base_compact.clear();
        let mut at = t.compact;
        while let Some(r) = va(at, 16) {
            let bits = u64::from_le_bytes(r[8..16].try_into().unwrap());
            if bits == 0 {
                break;
            }
            self.base_compact.push(Compact { key: r[0], rule: r[1], states: r[2], bits });
            at += 16;
        }
        self.k1.clear();
        let mut at = t.k1;
        while let Some(r) = va(at, 4).filter(|r| r[0] != 0xff) {
            self.k1.push(r.iter().copied().take_while(|&i| i != 0xff).collect());
            at += 4;
        }
        self.k2 = va(t.k2, K2_ROWS * 10).map_or(Vec::new(), |b| {
            u16s(b).chunks(5).map(|r| r.iter().copied().take_while(|&i| i != 0xffff).collect()).collect()
        });
        self.bindings = self.base_bindings.clone();
        self.compact = self.base_compact.clone();
        self.assigned = None;
    }

    /// Key Assignment (k1 config 0x4a.., k2 0x6a..) rewrites slot 0 of the k2 ids with Ctrl / Alt + key, the
    /// compact records' keys, and with keyboard type 2 the turn ids 0xaa / 0xa9 (no Ctrl / Alt).
    /// `kb` is the keyboard type in force (the Controls scheme's, controls.rs: the Config's 0x42 under Original).
    pub fn assign(&mut self, game: &crate::gamecfg::Values, kb: u32) {
        let now: Vec<u32> = std::iter::once(kb).chain((0x4a..0x8a).map(|id| game.get(id))).collect();
        if self.assigned.as_ref() == Some(&now) {
            return;
        }
        self.bindings = self.base_bindings.clone();
        self.compact = self.base_compact.clone();
        let slot0 = |b: &mut Vec<[u16; 3]>, id: usize, flag: u16, key: u32| {
            if let Some(s) = b.get_mut(id) {
                s[0] = if key & 0xff != 0 { flag << 8 | (key & 0xff) as u16 } else { 0 };
            }
        };
        for (row, ids) in self.k2.iter().enumerate() {
            for &id in ids {
                slot0(&mut self.bindings, id as usize, 0x10, game.get(0x6a + row as u16));
            }
        }
        for (row, recs) in self.k1.iter().enumerate() {
            for &r in recs {
                if let Some(c) = self.compact.get_mut(r as usize) {
                    c.key = game.get(0x4a + row as u16) as u8;
                }
            }
        }
        // Ours: the Modern scheme's Q / E are the type 2 turn keys (0x67 left, 0x66 right), at any keyboard type.
        self.turn_keys = [game.get(0x67) as u8, game.get(0x66) as u8];
        if kb == 2 {
            slot0(&mut self.bindings, 0xaa, 0x80, game.get(0x66));
            slot0(&mut self.bindings, 0xa9, 0x80, game.get(0x67));
        }
        self.assigned = Some(now);
    }

    /// A key from the window. `logical` tells NumLock: a numpad digit types a digit only with it on.
    pub fn key(&mut self, code: KeyCode, pressed: bool, logical: &winit::keyboard::Key) {
        let Some(k) = dik(code) else { return };
        if matches!(k, 0x47..=0x49 | 0x4b..=0x4d | 0x4f..=0x52) {
            self.numlock = matches!(logical, winit::keyboard::Key::Character(_));
        }
        self.set(k, pressed);
    }

    /// A key going down or up by DIK code (the window, and test drives act:down / act:up).
    pub fn set(&mut self, k: u8, pressed: bool) {
        self.down[k as usize] = pressed;
        if pressed {
            self.tapped[k as usize] = true;
        }
    }

    /// FUN_10124070: a raw key's press bit this frame, by DIK code.
    pub fn pressed(&self, k: u8) -> bool {
        self.keys[k as usize] & PRESS != 0
    }

    /// A press and release within one frame (test drives, `act:` keys).
    pub fn tap(&mut self, k: u8) {
        self.tapped[k as usize] = true;
    }

    /// A drive's egui key event: the key and its modifiers count as tapped this frame.
    pub fn drive_event(&mut self, e: &egui::Event) {
        if let egui::Event::Key { key, pressed: true, modifiers, .. } = e
            && let Some(k) = egui_dik(*key)
        {
            self.tap(k);
            for (on, m) in [(modifiers.ctrl, 0x1d), (modifiers.shift, 0x2a), (modifiers.alt, 0x38)] {
                if on {
                    self.tap(m);
                }
            }
        }
    }

    /// The controller's pad word for the next frames (client/gamepad.rs).
    pub fn set_device_pad(&mut self, bits: u64) {
        self.device = bits;
    }

    /// The controller's sticks for the next frames (client/gamepad.rs).
    pub fn set_stick(&mut self, stick: [f32; 4]) {
        self.stick = stick;
    }

    /// A keyboard key went down this frame.
    pub fn key_pressed_any(&self) -> bool {
        self.pressed_any
    }

    /// The controller's sticks alone: movement X / Y, camera X / Y.
    pub fn device_stick(&self) -> [f32; 4] {
        if self.typing {
            [0.0; 4]
        } else {
            std::array::from_fn(|i| {
                if self.pad_sticks[i].abs() >= self.stick[i].abs() { self.pad_sticks[i] } else { self.stick[i] }
            })
        }
    }

    /// A window had the keys last frame (arrows go to it, not to the camera).
    pub fn window_active(&self) -> bool {
        self.window
    }

    /// Drops every key (focus lost: the window gets no releases).
    pub fn clear(&mut self) {
        self.down = [false; 256];
        self.tapped = [false; 256];
        self.set_pad(&[], [0.0; 4]);
    }

    /// The gamepad's state for the next frame: the DIK keys its buttons hold (an empty list when none is in use) and
    /// its sticks. Call it before `begin_frame`.
    pub fn set_pad(&mut self, keys: &[u8], sticks: [f32; 4]) {
        self.pad_down = [false; 256];
        for &k in keys {
            self.pad_down[k as usize] = true;
        }
        self.pad_sticks = sticks;
        self.pad_zoom = 0.0;
    }

    /// The pad's map zoom for the next frame (call after `set_pad`, which clears it): negative zooms in (the zoom key
    /// 0xa4's sign, map.rs `update`), as the right stick pushed up does.
    pub fn set_pad_zoom(&mut self, zoom: f32) {
        self.pad_zoom = zoom;
    }

    /// The frame's key state (FUN_10005520), the chat line's Enter / Esc, then the compact pad (FUN_1011fbc0).
    pub fn begin_frame(&mut self, f: Frame) {
        self.consumed = false;
        self.typing = f.typing;
        self.assigning = f.assigning;
        self.window = f.window;
        self.modern = f.modern;
        #[cfg(feature = "forever")]
        {
            self.navigation_move = f.navigation_move && !f.typing && !f.assigning;
            self.tracker_key = f.tracker_key && f.modern && !f.typing && !f.assigning && !f.window;
            self.panel_diks = [f.journal_dik, f.recipe_dik];
        }
        self.clock += f.ticks;
        let frames = (self.clock / FRAME_TICKS).floor();
        self.clock -= frames * FRAME_TICKS;
        for k in 0..256 {
            let now = self.down[k] || self.tapped[k] || self.pad_down[k];
            self.tapped[k] = false;
            let s = &mut self.keys[k];
            if now {
                if *s & HELD == 0 {
                    *s = HELD | PRESS | REPEAT;
                    self.counters[k] = 0.0;
                } else {
                    *s &= !(REPEAT | PRESS | RELEASE);
                    for _ in 0..frames as u32 {
                        self.counters[k] += FRAME_TICKS;
                        if self.counters[k] > REPEAT_DELAY {
                            *s |= REPEAT;
                            self.counters[k] = REPEAT_DELAY - REPEAT_BACK;
                        }
                    }
                }
            } else {
                *s = if *s & HELD != 0 { RELEASE } else { 0 };
                self.counters[k] = 0.0;
            }
        }
        self.pressed_any = self.keys.iter().any(|&s| s & PRESS != 0);
        // The chat line takes Enter / Esc before any key function sees them.
        if f.typing {
            for k in [0x1c, 0x9c, 0x01] {
                if self.keys[k] & PRESS != 0 {
                    self.keys[k] &= !(REPEAT | PRESS | RELEASE);
                }
            }
        }
        self.pad_frame(&f, frames as u32);
    }

    fn pad_frame(&mut self, f: &Frame, frames: u32) {
        let prev = self.pad.held;
        let keys =
            if matches!(f.keyboard, 1 | 2) && !f.typing && !f.assigning { self.compact_bits(f.window) } else { 0 };
        // FUN_10005a40: the device's bits and the compact keys' share the word, its edges and its repeat.
        let held = keys | self.device;
        let mut edge = held & !prev;
        let mut repeat = edge;
        // Up or down drops left / right from the edges.
        if edge & 0xc0 != 0 {
            edge &= !0x30;
        }
        if repeat & 0xc0 != 0 {
            repeat &= !0x30;
        }
        if edge != 0 {
            self.pad.counter = 0.0;
            self.pad.armed = false;
        } else if held != 0 {
            for _ in 0..frames {
                self.pad.counter += FRAME_TICKS;
                if !self.pad.armed {
                    self.pad.armed = true;
                } else if self.pad.counter > REPEAT_DELAY {
                    repeat |= held;
                    self.pad.counter = REPEAT_DELAY - REPEAT_BACK;
                }
            }
        }
        self.pad.held = held;
        self.pad.edge = edge;
        self.pad.repeat = repeat;
    }

    /// FUN_1011fbc0: the bits of every record whose key is held under its modifier rule and state mask.
    fn compact_bits(&self, window: bool) -> u64 {
        let (ctrl, alt, shift) = (self.ctrl(), self.alt(), self.shift());
        let states = if window { 5 } else { 6 };
        self.compact
            .iter()
            .filter(|c| c.key != 0 && c.states & states == c.states && self.held(c.key))
            .filter(|c| match (ctrl, alt) {
                (false, false) => matches!(c.rule, 0 | 7) || (c.rule == 4 && shift) || (c.rule == 8 && !shift),
                _ => {
                    (ctrl && !alt && c.rule == 1)
                        || (alt && !ctrl && c.rule == 2)
                        || c.rule == 7
                        || (c.rule == 0 && !self.k2_key(c.key))
                }
            })
            .filter(|_c| {
                #[cfg(feature = "forever")]
                if _c.key == 0x26 && self.tracker_key_live() {
                    return false;
                }
                #[cfg(feature = "forever")]
                if _c.key == JOURNAL_DIK && self.journal_key_live() {
                    return false;
                }
                #[cfg(feature = "forever")]
                if _c.key == 0x23 && self.minimal_key_live() {
                    return false;
                }
                true
            })
            .fold(0, |b, c| b | c.bits)
    }

    /// FUN_101953b0: the key is some k2 row's (slot 0 of its first id).
    fn k2_key(&self, key: u8) -> bool {
        self.k2
            .iter()
            .filter_map(|r| r.first())
            .any(|&id| self.bindings.get(id as usize).is_some_and(|b| b[0] as u8 == key))
    }

    /// The compact pad's held bits (the stick directions W / A / S / D and I / J / K / L, among others).
    #[cfg(test)]
    pub fn pad_held(&self) -> u64 {
        self.pad.held
    }

    /// FUN_1011fe30 for the movement (4 X, 5 Y) and camera (6 X, 7 Y) axes: the larger of the raw keys
    /// (FUN_1011f300: numpad 6 / 4, 2 / 8, arrows) and the compact stick (W / A / S / D, I / J / K / L; byte
    /// (b - 0x80) / 128, the later record winning). While typing only numpad movement with NumLock off counts;
    /// with a window active the arrows need Shift (FUN_1011f7b0 / FUN_1011f910).
    pub fn axis(&self, id: usize) -> f32 {
        self.axis_with_pad(id, true)
    }

    pub fn camera_key_axis(&self, id: usize) -> f32 {
        self.axis_with_pad(id, false)
    }

    fn axis_enabled(&self, id: usize) -> bool {
        #[cfg(feature = "forever")]
        if self.navigation_move && matches!(id, 4 | 5) && self.masks[id] != 0 {
            return true;
        }
        self.masks[id] & self.context != 0
    }

    fn axis_with_pad(&self, id: usize, include_camera_pad: bool) -> f32 {
        if !matches!(id, 4..=7 | 0x79 | 0x7a | 0x7c | 0x9d | 0x9e) || !self.axis_enabled(id) {
            return 0.0;
        }
        let key = |p: u8, n: u8| self.held(p) as i32 as f32 - self.held(n) as i32 as f32;
        let stick = |p: u64, n: u64| -> f32 {
            if self.pad.held & p != 0 {
                127.0 / 128.0
            } else if self.pad.held & n != 0 {
                -1.0
            } else {
                0.0
            }
        };
        // A controller's stick bytes (FUN_10005a40 +0x250..+0x253, client/gamepad.rs) count as the compact stick of
        // the same axis, the larger one winning; not while typing. A window does not stop them: the sticks never drive
        // windows (the d-pad does). The camera's 6 / 7 leave them out: the camera reads them with its own sense
        // (device_stick).
        let pad_stick = match id {
            _ if self.typing => 0.0,
            4 | 0x79 | 0x9d => self.stick[0],
            5 | 0x7a | 0x9e => self.stick[1],
            0x7c => self.stick[3],
            _ => 0.0,
        };
        let arrows = !self.typing && (!self.window || self.shift());
        let (keys, pad) = match id {
            4 => (if self.typing && self.numlock { 0.0 } else { key(0x4d, 0x4b) }, stick(0x20_0000, 0x40_0000)),
            5 => (if self.typing && self.numlock { 0.0 } else { key(0x50, 0x48) }, stick(0x100_0000, 0x80_0000)),
            // The map's scroll (0x79 / 0x7a: numpad 6 / 4, 2 / 8 and the compact W / A / S / D stick, FUN_10122e30) and
            // zoom stick (0x7c: compact I / K; its keys are the caller's).
            0x79 => (if self.typing && self.numlock { 0.0 } else { key(0x4d, 0x4b) }, stick(0x20_0000, 0x40_0000)),
            0x7a => (if self.typing && self.numlock { 0.0 } else { key(0x50, 0x48) }, stick(0x100_0000, 0x80_0000)),
            0x7c => (0.0, stick(0x10_0000, 0x8_0000)),
            // The map's cursor mode (FUN_10122e30(7 / 8): numpad only, and the left stick).
            0x9d => (if self.typing && self.numlock { 0.0 } else { key(0x4d, 0x4b) }, stick(0x20_0000, 0x40_0000)),
            0x9e => (if self.typing && self.numlock { 0.0 } else { key(0x50, 0x48) }, stick(0x100_0000, 0x80_0000)),
            6 => (if arrows { key(0xcd, 0xcb) } else { 0.0 }, stick(0x2_0000, 0x4_0000)),
            _ => (if arrows { key(0xd0, 0xc8) } else { 0.0 }, stick(0x10_0000, 0x8_0000)),
        };
        // The gamepad's stick, the same axis (movement x / y, camera x / y; the map's scroll and zoom use the same two
        // sticks). Typing: no walking, as the numpad movement above.
        let analog = match id {
            6 | 7 if !include_camera_pad => 0.0,
            4 | 0x79 | 0x9d => self.pad_sticks[0],
            5 | 0x7a | 0x9e => self.pad_sticks[1],
            6 => self.pad_sticks[2],
            0x7c => self.pad_zoom,
            _ => self.pad_sticks[3],
        };
        let pad = if analog.abs() > pad.abs() && !(self.typing && matches!(id, 4 | 5)) { analog } else { pad };
        let pad = if pad_stick.abs() > pad.abs() { pad_stick } else { pad };
        if keys.abs() >= pad.abs() { keys } else { pad }
    }

    /// Ours (not retail): the key of the compact record that sets pad bit `bit` (a stick direction, the autorun key),
    /// as Key Assignment left it; `default` before the tables are read. 0 is a key taken off.
    fn compact_key(&self, bit: u64, default: u8) -> u8 {
        self.compact.iter().find(|c| c.bits == bit).map_or(default, |c| c.key)
    }

    /// Whether the Modern keys count now: the scheme is on, no text field, menu or Key Assignment has the keys, the
    /// input context is the field's, and Ctrl / Alt (the menu shortcuts' letters) are up.
    fn modern_live(&self) -> bool {
        self.modern && !self.typing && !self.assigning && self.axis_enabled(4) && !self.ctrl() && !self.alt()
    }

    /// Ours (not retail): the Modern scheme's keys held now. W / S / A / D are the compact stick's keys and Q / E the
    /// type 2 turn keys, so Key Assignment (k1 rows, Turn Right / Left) moves them. Nothing while a text field, a
    /// menu or Key Assignment has the keys.
    pub fn modern_keys(&self) -> super::controls::Keys {
        if !self.modern_live() {
            return Default::default();
        }
        let down = |k: u8| k != 0 && self.held(k);
        super::controls::Keys {
            fwd: down(self.compact_key(0x80_0000, 0x11)),
            back: down(self.compact_key(0x100_0000, 0x1f)),
            left: down(self.compact_key(0x40_0000, 0x1e)),
            right: down(self.compact_key(0x20_0000, 0x20)),
            q: down(self.turn_keys[0]),
            e: down(self.turn_keys[1]),
        }
    }

    /// Ours: the Modern scheme's autorun key went down this frame (the compact autorun key, R unless moved).
    pub fn modern_autorun(&self) -> bool {
        let k = self.compact_key(0x100, 0x13);
        self.modern_live() && k != 0 && self.pressed(k)
    }

    /// The bare M convenience never steals a configured key. Reserve an
    /// existing unmodified assignment even outside its current input context.
    pub fn map_shortcut(&self) -> bool {
        const M: u8 = 0x32;
        self.pressed(M)
            && !self.typing
            && !self.assigning
            && !self.consumed
            && !self.ctrl()
            && !self.alt()
            && !self.shift()
            && !self.turn_keys.contains(&M)
            && !self
                .bindings
                .iter()
                .enumerate()
                .any(|(id, row)| self.masks[id] != 0 && row.iter().any(|&b| b as u8 == M && (b >> 8) & 0x1f == 0))
            && !self.compact.iter().any(|c| c.key == M && c.bits != 0 && matches!(c.rule, 0 | 7 | 8))
    }

    /// L replaces only the unchanged compact L records in Forever's Modern
    /// controls. Its stock menu-navigation records remain active when a window has focus.
    /// Custom bindings win even outside their current context; Ctrl/Alt+L
    /// still belongs to linkshell chat. Classic and Original/Legacy controls never opt in.
    #[cfg(feature = "forever")]
    fn tracker_binding_free(&self) -> bool {
        const L: u8 = 0x26;
        !self.turn_keys.contains(&L)
            && !self
                .bindings
                .iter()
                .enumerate()
                .any(|(id, row)| self.masks[id] != 0 && row.iter().any(|&b| b as u8 == L && (b >> 8) & 0x1f == 0))
            && !self
                .compact
                .iter()
                .any(|c| c.key == L && c.bits != 0 && matches!(c.rule, 0 | 7 | 8) && !self.base_compact.contains(c))
    }

    #[cfg(feature = "forever")]
    fn tracker_key_live(&self) -> bool {
        self.tracker_key && !self.ctrl() && !self.alt() && !self.shift() && self.tracker_binding_free()
    }

    #[cfg(feature = "forever")]
    pub fn tracker_shortcut(&self) -> bool {
        self.tracker_key_live() && !self.consumed && self.pressed(0x26)
    }

    /// Ctrl+Shift+H, the Minimal HUD key (Forever Modern controls). The stock H record is a compact one (rule 0) that the
    /// game would also act on with Ctrl held, so it is held back while this is live (compact_bits); a player's own binding
    /// of H with a modifier, or H as a turn key, keeps the key.
    #[cfg(feature = "forever")]
    fn minimal_key_live(&self) -> bool {
        const H: u8 = 0x23;
        self.tracker_key
            && self.ctrl()
            && self.shift()
            && !self.alt()
            && !self.turn_keys.contains(&H)
            && !self
                .bindings
                .iter()
                .enumerate()
                .any(|(id, row)| self.masks[id] != 0 && row.iter().any(|&b| b as u8 == H && (b >> 8) & 0x1f != 0))
    }

    #[cfg(feature = "forever")]
    pub fn minimal_shortcut(&self) -> bool {
        self.minimal_key_live() && !self.consumed && self.pressed(0x23)
    }

    /// J follows L: in Forever's Modern controls it replaces only the unchanged stock compact J records (camera and
    /// menu keys in the stock tables) in the field, never a custom binding, turn key or remapped compact record.
    #[cfg(feature = "forever")]
    fn journal_key_free(&self) -> bool {
        const J: u8 = JOURNAL_DIK;
        !self.turn_keys.contains(&J)
            && !self
                .bindings
                .iter()
                .enumerate()
                .any(|(id, row)| self.masks[id] != 0 && row.iter().any(|&b| b as u8 == J && (b >> 8) & 0x1f == 0))
            && !self
                .compact
                .iter()
                .any(|c| c.key == J && c.bits != 0 && matches!(c.rule, 0 | 7 | 8) && !self.base_compact.contains(c))
    }

    #[cfg(feature = "forever")]
    fn journal_key_live(&self) -> bool {
        self.panel_diks[0] == JOURNAL_DIK
            && self.tracker_key
            && !self.ctrl()
            && !self.alt()
            && !self.shift()
            && self.journal_key_free()
    }

    /// J went down this frame and opens the Journal.
    #[cfg(feature = "forever")]
    pub fn journal_shortcut(&self) -> bool {
        self.journal_key_live() && !self.consumed && self.pressed(JOURNAL_DIK)
    }

    /// The Crafting Log key must be entirely free: no binding, turn key or compact record (stock or not) uses it.
    #[cfg(feature = "forever")]
    fn recipe_key_free(&self, key: u8) -> bool {
        key != 0
            && key != 0x26
            && key != JOURNAL_DIK
            && !self.turn_keys.contains(&key)
            && !self
                .bindings
                .iter()
                .enumerate()
                .any(|(id, row)| self.masks[id] != 0 && row.iter().any(|&b| b as u8 == key && (b >> 8) & 0x1f == 0))
            && !self.compact.iter().any(|c| c.key == key && c.bits != 0 && matches!(c.rule, 0 | 7 | 8))
    }

    /// The Crafting Log key went down this frame.
    #[cfg(feature = "forever")]
    pub fn recipe_shortcut(&self) -> bool {
        let key = self.panel_diks[1];
        self.tracker_key
            && !self.ctrl()
            && !self.alt()
            && !self.shift()
            && !self.consumed
            && self.pressed(key)
            && self.recipe_key_free(key)
    }

    /// The Crafting Log key as an egui key (the settings offer only keys the stock tables leave free).
    #[cfg(feature = "forever")]
    pub fn recipe_egui_key(&self) -> Option<egui::Key> {
        RECIPE_KEYS.iter().find(|k| k.0 == self.panel_diks[1]).map(|k| k.1)
    }

    fn held(&self, k: u8) -> bool {
        self.keys[k as usize] & HELD != 0
    }

    /// The DIK codes held now, the window flag and typing (the F12 lines).
    pub fn debug_state(&self) -> String {
        let held: Vec<String> = (0..=255u8).filter(|&k| self.held(k)).map(|k| format!("{k:02x}")).collect();
        format!("input window {} typing {} held [{}]", self.window, self.typing, held.join(" "))
    }

    fn ctrl(&self) -> bool {
        self.held(0x1d) || self.held(0x9d)
    }

    fn alt(&self) -> bool {
        self.held(0x38) || self.held(0xb8)
    }

    fn shift(&self) -> bool {
        self.held(0x2a) || self.held(0x36)
    }

    /// FUN_101202a0: Enter / Esc were taken; key functions other than mode 4 fail for the rest of the frame.
    pub fn consume(&mut self) {
        self.consumed = true;
    }

    /// FUN_1011ff30: key function `id` in `mode` (see `mode`).
    pub fn test(&mut self, id: usize, mode: i32) -> bool {
        if (self.consumed && mode != mode::HELD) || id >= IDS {
            return false;
        }
        let m = self.masks[id];
        if m == 0 || m & self.context == 0 || (m & 0x41 != 0 && self.typing) {
            return false;
        }
        if self.pad_test(id, mode) {
            return true;
        }
        for slot in 0..3 {
            let b = self.bindings[id][slot];
            if b != 0 && self.modifiers_ok((b >> 8) as u8) && self.key_test(b as u8, mode) {
                return true;
            }
        }
        false
    }

    fn modifiers_ok(&self, fl: u8) -> bool {
        let (ctrl, alt, shift) = (self.ctrl(), self.alt(), self.shift());
        !(fl & 0x40 != 0 && self.typing)
            && (fl & 0x80 == 0 || (!ctrl && !alt))
            && (fl & 0x01 == 0 || shift)
            && (fl & 0x02 == 0 || ctrl)
            && (fl & 0x04 == 0 || alt)
            && (fl & 0x08 == 0 || (ctrl && alt))
            && (fl & 0x10 == 0 || ((ctrl || alt) && !self.assigning))
            && (fl & 0x20 == 0 || !shift)
    }

    /// FUN_1011f850, with LCtrl / LShift / LAlt bindings meaning "either one held", in mode 4 only.
    fn key_test(&mut self, k: u8, mode: i32) -> bool {
        match k {
            0x1d => return mode == mode::HELD && self.ctrl(),
            0x2a => return mode == mode::HELD && self.shift(),
            0x38 => return mode == mode::HELD && self.alt(),
            _ => {}
        }
        // With NumLock on the chat line types the numpad digits.
        if self.typing && self.numlock && matches!(k, 0x47..=0x49 | 0x4b..=0x4d | 0x4f..=0x52) {
            return false;
        }
        let s = &mut self.keys[k as usize];
        let (bit, consume) = match mode {
            mode::REPEAT => (REPEAT, false),
            mode::PRESS => (PRESS, false),
            mode::REPEAT_CONSUME => (REPEAT, true),
            mode::PRESS_CONSUME => (PRESS, true),
            mode::HELD => (HELD, false),
            _ => return false,
        };
        let hit = *s & bit != 0;
        if hit && consume {
            *s &= !bit;
        }
        hit
    }

    /// The pad callback's test (FUN_1011da00 and kin): +0x238 repeat, +0x228 edge, +0x220 held. Modes 2 / 3 keep
    /// only this bit's edge, as retail does.
    fn pad_test(&mut self, id: usize, mode: i32) -> bool {
        // 0xb0..0xb3 (the ground-position cursor): LT or RT held (slot 9 / 13) and the d-pad's up / down / right /
        // left (slots 6 / 7 / 4 / 5) in the mode asked.
        let combo = match id {
            0xb0 => Some(0x40),
            0xb1 => Some(0x80),
            0xb2 => Some(0x10),
            0xb3 => Some(0x20),
            _ => None,
        };
        let bit = match combo {
            Some(_) if self.pad.held & 0x2200 == 0 => return false,
            Some(b) => b,
            None => match PAD.iter().find(|p| p.0 as usize == id) {
                Some(&(_, b)) => b,
                None => return false,
            },
        };
        let p = &mut self.pad;
        match mode {
            mode::REPEAT => p.repeat & bit != 0,
            mode::PRESS => p.edge & bit != 0,
            mode::REPEAT_CONSUME => {
                p.edge &= bit;
                p.repeat & bit != 0
            }
            mode::PRESS_CONSUME => {
                p.edge &= bit;
                p.edge != 0
            }
            mode::HELD => p.held & bit != 0,
            _ => false,
        }
    }
}

/// The DirectInput code of a key.
pub fn dik(code: KeyCode) -> Option<u8> {
    use KeyCode as K;
    Some(match code {
        K::Escape => 0x01,
        K::Digit1 => 0x02,
        K::Digit2 => 0x03,
        K::Digit3 => 0x04,
        K::Digit4 => 0x05,
        K::Digit5 => 0x06,
        K::Digit6 => 0x07,
        K::Digit7 => 0x08,
        K::Digit8 => 0x09,
        K::Digit9 => 0x0a,
        K::Digit0 => 0x0b,
        K::Minus => 0x0c,
        K::Equal => 0x0d,
        K::Backspace => 0x0e,
        K::Tab => 0x0f,
        K::KeyQ => 0x10,
        K::KeyW => 0x11,
        K::KeyE => 0x12,
        K::KeyR => 0x13,
        K::KeyT => 0x14,
        K::KeyY => 0x15,
        K::KeyU => 0x16,
        K::KeyI => 0x17,
        K::KeyO => 0x18,
        K::KeyP => 0x19,
        K::BracketLeft => 0x1a,
        K::BracketRight => 0x1b,
        K::Enter => 0x1c,
        K::ControlLeft => 0x1d,
        K::KeyA => 0x1e,
        K::KeyS => 0x1f,
        K::KeyD => 0x20,
        K::KeyF => 0x21,
        K::KeyG => 0x22,
        K::KeyH => 0x23,
        K::KeyJ => 0x24,
        K::KeyK => 0x25,
        K::KeyL => 0x26,
        K::Semicolon => 0x27,
        K::Quote => 0x28,
        K::Backquote => 0x29,
        K::ShiftLeft => 0x2a,
        K::Backslash => 0x2b,
        K::KeyZ => 0x2c,
        K::KeyX => 0x2d,
        K::KeyC => 0x2e,
        K::KeyV => 0x2f,
        K::KeyB => 0x30,
        K::KeyN => 0x31,
        K::KeyM => 0x32,
        K::Comma => 0x33,
        K::Period => 0x34,
        K::Slash => 0x35,
        K::ShiftRight => 0x36,
        K::NumpadMultiply => 0x37,
        K::AltLeft => 0x38,
        K::Space => 0x39,
        K::CapsLock => 0x3a,
        K::F1 => 0x3b,
        K::F2 => 0x3c,
        K::F3 => 0x3d,
        K::F4 => 0x3e,
        K::F5 => 0x3f,
        K::F6 => 0x40,
        K::F7 => 0x41,
        K::F8 => 0x42,
        K::F9 => 0x43,
        K::F10 => 0x44,
        K::NumLock => 0x45,
        K::ScrollLock => 0x46,
        K::Numpad7 => 0x47,
        K::Numpad8 => 0x48,
        K::Numpad9 => 0x49,
        K::NumpadSubtract => 0x4a,
        K::Numpad4 => 0x4b,
        K::Numpad5 => 0x4c,
        K::Numpad6 => 0x4d,
        K::NumpadAdd => 0x4e,
        K::Numpad1 => 0x4f,
        K::Numpad2 => 0x50,
        K::Numpad3 => 0x51,
        K::Numpad0 => 0x52,
        K::NumpadDecimal => 0x53,
        K::IntlBackslash => 0x56,
        K::F11 => 0x57,
        K::F12 => 0x58,
        K::NumpadEnter => 0x9c,
        K::ControlRight => 0x9d,
        K::NumpadComma => 0xb3,
        K::NumpadDivide => 0xb5,
        K::PrintScreen => 0xb7,
        K::AltRight => 0xb8,
        K::Pause => 0xc5,
        K::Home => 0xc7,
        K::ArrowUp => 0xc8,
        K::PageUp => 0xc9,
        K::ArrowLeft => 0xcb,
        K::ArrowRight => 0xcd,
        K::End => 0xcf,
        K::ArrowDown => 0xd0,
        K::PageDown => 0xd1,
        K::Insert => 0xd2,
        K::Delete => 0xd3,
        K::SuperLeft => 0xdb,
        K::SuperRight => 0xdc,
        K::ContextMenu => 0xdd,
        _ => return None,
    })
}

/// The DirectInput code of an egui key (test drives; egui does not tell the numpad apart).
fn egui_dik(key: egui::Key) -> Option<u8> {
    use egui::Key as E;
    Some(match key {
        E::Escape => 0x01,
        E::Minus => 0x0c,
        E::Equals => 0x0d,
        E::Backspace => 0x0e,
        E::Tab => 0x0f,
        E::OpenBracket => 0x1a,
        E::CloseBracket => 0x1b,
        E::Enter => 0x1c,
        E::Semicolon => 0x27,
        E::Quote => 0x28,
        E::Backtick => 0x29,
        E::Backslash => 0x2b,
        E::Comma => 0x33,
        E::Period => 0x34,
        E::Slash => 0x35,
        E::Space => 0x39,
        E::Home => 0xc7,
        E::ArrowUp => 0xc8,
        E::PageUp => 0xc9,
        E::ArrowLeft => 0xcb,
        E::ArrowRight => 0xcd,
        E::End => 0xcf,
        E::ArrowDown => 0xd0,
        E::PageDown => 0xd1,
        E::Insert => 0xd2,
        E::Delete => 0xd3,
        _ => {
            let n = key.name();
            let letters = b"QWERTYUIOP\0\0\0\0ASDFGHJKL\0\0\0\0\0ZXCVBNM";
            if n.len() == 1
                && let Some(i) = letters.iter().position(|&c| c == n.as_bytes()[0])
            {
                return Some(0x10 + i as u8);
            }
            if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() {
                return Some(match n.as_bytes()[0] {
                    b'0' => 0x0b,
                    d => 0x02 + (d - b'1'),
                });
            }
            let f = n.strip_prefix('F').and_then(|s| s.parse::<u8>().ok())?;
            return match f {
                1..=10 => Some(0x3b + f - 1),
                11 => Some(0x57),
                12 => Some(0x58),
                _ => None,
            };
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_quality_map_key_press_only_and_text_or_assignment_suppression() {
        let press = |i: &mut Input, f| {
            i.down[0x32] = true;
            i.begin_frame(f);
        };
        let mut i = Input::default();
        press(&mut i, frame());
        assert!(i.map_shortcut());
        for _ in 0..40 {
            i.begin_frame(frame());
            assert!(!i.map_shortcut());
        }
        i.down[0x32] = false;
        i.begin_frame(frame());
        press(&mut i, frame());
        assert!(i.map_shortcut());
        i.consume();
        assert!(!i.map_shortcut());
        for f in [Frame { typing: true, ..frame() }, Frame { assigning: true, ..frame() }] {
            let mut i = Input::default();
            press(&mut i, f);
            assert!(!i.map_shortcut());
            i.begin_frame(frame());
            assert!(!i.map_shortcut(), "leaving a text field must not activate a held M");
        }
        for key in [0x1d, 0x9d, 0x38, 0xb8, 0x2a, 0x36] {
            let mut i = Input::default();
            i.down[key] = true;
            press(&mut i, frame());
            assert!(!i.map_shortcut());
        }
    }

    #[test]
    fn shared_quality_map_key_preserves_bindings_and_magic_shortcut() {
        let mut i = Input::default();
        i.down[0x32] = true;
        i.begin_frame(frame());
        i.masks[0x37] = 0x3e;
        i.bindings[0x37][0] = 0x1032; // Ctrl/Alt+M remains Magic; bare M is free.
        assert!(i.map_shortcut());
        i.down[0x1d] = true;
        i.begin_frame(frame());
        assert!(!i.map_shortcut());
        assert!(i.test(0x37, mode::HELD));
        i.down[0x1d] = false;
        i.down[0x32] = false;
        i.begin_frame(frame());
        i.down[0x32] = true;
        i.begin_frame(frame());
        for slot in 0..3 {
            i.bindings[4][slot] = 0x8032;
            i.masks[4] = 0x08;
            assert!(!i.map_shortcut());
            i.bindings[4][slot] = 0;
        }
        for rule in [0, 7, 8] {
            i.compact = vec![Compact { key: 0x32, rule, states: 4, bits: 0x80_0000 }];
            assert!(!i.map_shortcut());
        }
        i.compact.clear();
        i.turn_keys[0] = 0x32;
        assert!(!i.map_shortcut());
    }

    #[test]
    #[ignore = "requires player-owned FFXiMain.dll"]
    fn shared_quality_map_key_retail_defaults_and_live_remap() {
        for kb in 0..=2 {
            let mut i = real().expect("player-owned FFXiMain.dll fixture required");
            assert_eq!(i.compact.len(), 42, "retail tables must actually load");
            let mut v = crate::gamecfg::Values::default();
            i.assign(&v, kb);
            i.down[0x32] = true;
            i.begin_frame(Frame { keyboard: kb, ..frame() });
            assert!(i.map_shortcut(), "default keyboard {kb} must leave bare M free");
            v.set(0x66, 0x32);
            i.assign(&v, kb);
            assert!(!i.map_shortcut(), "custom M turn assignment must win");
        }
    }

    /// The Minimal HUD key: Ctrl+Shift+H, a press only, only in Forever Modern controls out of typing; the stock H compact
    /// record is held back while it is live (so the game does not also act), but not for a plain H; a player's own
    /// modified binding of H keeps it.
    #[cfg(feature = "forever")]
    #[test]
    fn minimal_hud_key_is_ctrl_shift_h_and_holds_back_the_stock_h_record() {
        let f = Frame { modern: true, tracker_key: true, ..frame() };
        let press = |held: &[u8]| {
            let mut i = Input::default();
            for k in held {
                i.down[*k as usize] = true;
            }
            i.begin_frame(f);
            i
        };
        let (ctrl, shift, alt, h) = (0x1d, 0x2a, 0x38, 0x23);
        assert!(press(&[ctrl, shift, h]).minimal_shortcut());
        assert!(press(&[0x9d, 0x36, h]).minimal_shortcut(), "the right-hand keys too");
        for held in [vec![h], vec![ctrl, h], vec![shift, h], vec![ctrl, shift, alt, h], vec![ctrl, shift]] {
            assert!(!press(&held).minimal_shortcut(), "{held:?}");
        }
        // A press only: held, it does not repeat, and consume() takes it.
        let mut i = Input::default();
        i.down[ctrl as usize] = true;
        i.down[shift as usize] = true;
        i.down[h as usize] = true;
        i.begin_frame(f);
        assert!(i.minimal_shortcut());
        i.begin_frame(f);
        assert!(!i.minimal_shortcut());
        let mut i = press(&[ctrl, shift, h]);
        i.consume();
        assert!(!i.minimal_shortcut());
        for blocked in [Frame { typing: true, ..f }, Frame { tracker_key: false, ..f }, Frame { assigning: true, ..f }]
        {
            let mut i = Input::default();
            for k in [ctrl, shift, h] {
                i.down[k as usize] = true;
            }
            i.begin_frame(blocked);
            assert!(!i.minimal_shortcut());
        }
        // The stock H record (rule 0, states 4: the field) would fire with Ctrl held; it is held back while the key is
        // live, and left alone for a plain H.
        let stock = Compact { key: h, rule: 0, states: 4, bits: 0x400 };
        let mut i = press(&[ctrl, shift, h]);
        i.compact = vec![stock];
        assert_eq!(i.compact_bits(false), 0, "the game must not also act on Ctrl+Shift+H");
        let mut plain = press(&[h]);
        plain.compact = vec![stock];
        assert_eq!(plain.compact_bits(false), 0x400, "a plain H keeps its stock function");
        // A player's own modified binding of H, or H as a turn key, keeps the key.
        let mut i = press(&[ctrl, shift, h]);
        i.bindings[4][0] = 0x0123;
        i.masks[4] = 0x08;
        assert!(!i.minimal_shortcut());
        i.bindings[4][0] = 0;
        i.turn_keys[0] = h;
        assert!(!i.minimal_shortcut());
    }

    #[cfg(feature = "forever")]
    #[test]
    fn tracker_key_press_only_and_focus_modifiers_or_custom_bindings_win() {
        let f = Frame { modern: true, tracker_key: true, ..frame() };
        let mut i = Input::default();
        i.down[0x26] = true;
        i.begin_frame(f);
        assert!(i.tracker_shortcut());
        for _ in 0..40 {
            i.begin_frame(f);
            assert!(!i.tracker_shortcut());
        }
        i.down[0x26] = false;
        i.begin_frame(f);
        i.down[0x26] = true;
        i.begin_frame(f);
        assert!(i.tracker_shortcut());
        i.consume();
        assert!(!i.tracker_shortcut());
        for blocked in [
            Frame { typing: true, ..f },
            Frame { assigning: true, ..f },
            Frame { tracker_key: false, ..f },
            Frame { modern: false, ..f },
            Frame { window: true, ..f },
        ] {
            let mut i = Input::default();
            i.down[0x26] = true;
            i.begin_frame(blocked);
            assert!(!i.tracker_shortcut());
            i.begin_frame(f);
            assert!(!i.tracker_shortcut(), "leaving focus must not toggle a held L");
        }
        for modifier in [0x1d, 0x9d, 0x38, 0xb8, 0x2a, 0x36] {
            let mut i = Input::default();
            i.down[modifier] = true;
            i.down[0x26] = true;
            i.begin_frame(f);
            assert!(!i.tracker_shortcut());
        }
        let mut i = Input::default();
        i.down[0x26] = true;
        i.begin_frame(f);
        for slot in 0..3 {
            i.bindings[4][slot] = 0x8026;
            i.masks[4] = 0x08;
            assert!(!i.tracker_shortcut());
            i.bindings[4][slot] = 0;
        }
        for rule in [0, 7, 8] {
            i.compact = vec![Compact { key: 0x26, rule, states: 4, bits: 0x80_0000 }];
            assert!(!i.tracker_shortcut());
        }
        i.compact.clear();
        i.turn_keys[0] = 0x26;
        assert!(!i.tracker_shortcut());
    }

    #[cfg(feature = "forever")]
    fn panel_frame() -> Frame {
        Frame { modern: true, tracker_key: true, journal_dik: JOURNAL_DIK, recipe_dik: 0x16, ..frame() }
    }

    #[cfg(feature = "forever")]
    #[test]
    fn window_keys_open_on_press_only_in_the_field_and_never_take_an_assigned_key() {
        let f = panel_frame();
        for (dik, open) in
            [(JOURNAL_DIK, Input::journal_shortcut as fn(&Input) -> bool), (0x16, Input::recipe_shortcut)]
        {
            let mut i = Input::default();
            i.down[dik as usize] = true;
            i.begin_frame(f);
            assert!(open(&i), "{dik:#x} opens on its press");
            for _ in 0..40 {
                i.begin_frame(f);
                assert!(!open(&i), "held key never repeats");
            }
            i.consume();
            assert!(!open(&i));
            // tracker_key is false under Classic controls/UI, a focused window, an event or hidden UI (frame builder).
            for blocked in [
                Frame { typing: true, ..f },
                Frame { assigning: true, ..f },
                Frame { tracker_key: false, ..f },
                Frame { modern: false, ..f },
                Frame { window: true, ..f },
            ] {
                let mut i = Input::default();
                i.down[dik as usize] = true;
                i.begin_frame(blocked);
                assert!(!open(&i));
                i.begin_frame(f);
                assert!(!open(&i), "leaving focus must not open on a held key");
            }
            for modifier in [0x1d, 0x9d, 0x38, 0xb8, 0x2a, 0x36] {
                let mut i = Input::default();
                i.down[modifier] = true;
                i.down[dik as usize] = true;
                i.begin_frame(f);
                assert!(!open(&i));
            }
            let fresh = |i: &mut Input| {
                i.down[dik as usize] = false;
                i.begin_frame(f);
                i.down[dik as usize] = true;
                i.begin_frame(f);
            };
            let mut i = Input::default();
            i.bindings[4][1] = 0x8000 | dik as u16;
            i.masks[4] = 0x08;
            fresh(&mut i);
            assert!(!open(&i), "a Key Assignment binding keeps the key");
            i.bindings[4][1] = 0;
            i.compact = vec![Compact { key: dik, rule: 0, states: 4, bits: 0x10 }];
            fresh(&mut i);
            assert!(!open(&i), "a remapped compact record keeps the key");
            i.compact.clear();
            i.turn_keys[0] = dik;
            fresh(&mut i);
            assert!(!open(&i), "a turn key keeps the key");
            i.turn_keys = [0x10, 0x12];
            fresh(&mut i);
            assert!(open(&i));
        }
        // J replaces only unchanged stock compact J records; the Crafting Log key never takes a stock record.
        let stock = Compact { key: JOURNAL_DIK, rule: 8, states: 6, bits: 0x40000 };
        let mut i = Input::default();
        i.base_compact = vec![stock];
        i.compact = vec![stock];
        i.down[JOURNAL_DIK as usize] = true;
        i.begin_frame(f);
        assert!(i.journal_shortcut());
        assert_eq!(i.compact_bits(false) & 0x40000, 0, "J does not also turn the camera while it opens the Journal");
        i.begin_frame(Frame { tracker_key: false, ..f });
        assert_ne!(i.compact_bits(false) & 0x40000, 0, "stock J camera key works when J is not the Journal key");
        let stock_u = Compact { key: 0x16, rule: 0, states: 4, bits: 8 };
        let mut i = Input::default();
        i.base_compact = vec![stock_u];
        i.compact = vec![stock_u];
        i.down[0x16] = true;
        i.begin_frame(f);
        assert!(!i.recipe_shortcut(), "a stock record on the recipe key keeps it");
        let mut i = Input::default();
        i.down[JOURNAL_DIK as usize] = true;
        i.begin_frame(Frame { journal_dik: 0, ..f });
        assert!(!i.journal_shortcut(), "Journal key OFF");
        let mut i = Input::default();
        i.down[0x16] = true;
        i.begin_frame(Frame { recipe_dik: 0, ..f });
        assert!(!i.recipe_shortcut(), "Crafting Log key OFF");
        for taken in [0x26u8, JOURNAL_DIK] {
            let mut i = Input::default();
            i.down[taken as usize] = true;
            i.begin_frame(Frame { recipe_dik: taken, ..f });
            assert!(!i.recipe_shortcut(), "{taken:#x} stays L / J");
        }
        let i = Input { panel_diks: [JOURNAL_DIK, 0x16], ..Input::default() };
        assert_eq!(i.recipe_egui_key(), Some(egui::Key::U));
    }

    #[cfg(feature = "forever")]
    #[test]
    #[ignore = "requires player-owned FFXiMain.dll"]
    fn stock_tables_and_window_keys() {
        let mut i = real().expect("player-owned FFXiMain.dll fixture required");
        let mut v = crate::gamecfg::Values::default();
        let f = panel_frame();
        for kb in [0, 1, 2] {
            i.assign(&v, kb);
            const LETTERS: [(char, u8); 26] = [
                ('Q', 0x10),
                ('W', 0x11),
                ('E', 0x12),
                ('R', 0x13),
                ('T', 0x14),
                ('Y', 0x15),
                ('U', 0x16),
                ('I', 0x17),
                ('O', 0x18),
                ('P', 0x19),
                ('A', 0x1e),
                ('S', 0x1f),
                ('D', 0x20),
                ('F', 0x21),
                ('G', 0x22),
                ('H', 0x23),
                ('J', 0x24),
                ('K', 0x25),
                ('L', 0x26),
                ('Z', 0x2c),
                ('X', 0x2d),
                ('C', 0x2e),
                ('V', 0x2f),
                ('B', 0x30),
                ('N', 0x31),
                ('M', 0x32),
            ];
            let free: Vec<char> = LETTERS.iter().filter(|l| i.recipe_key_free(l.1)).map(|l| l.0).collect();
            println!("keyboard {kb}: letters free of every stock record: {free:?}");
            for &(dik, _, label) in RECIPE_KEYS {
                assert!(i.recipe_key_free(dik), "offered Crafting Log key {label} is free in stock keyboard {kb}");
            }
            assert!(i.journal_key_free(), "stock keyboard {kb}: only stock compact records use J");
        }
        // The Modern scheme runs keyboard type 0 (controls.rs).
        i.assign(&v, 0);
        i.down[JOURNAL_DIK as usize] = true;
        i.begin_frame(f);
        assert!(i.journal_shortcut(), "requested J opens the Journal under the stock Modern tables");
        assert_eq!(
            i.compact_bits(false) & i.base_compact.iter().filter(|c| c.key == JOURNAL_DIK).fold(0, |b, c| b | c.bits),
            0,
            "and does not also send J's stock camera bits"
        );
        i.begin_frame(Frame { window: true, ..f });
        assert!(!i.journal_shortcut(), "stock J menu navigation stays with focused windows");
        i.down[JOURNAL_DIK as usize] = false;
        i.begin_frame(f);
        v.set(0x66, JOURNAL_DIK as u32);
        i.assign(&v, 0);
        i.down[JOURNAL_DIK as usize] = true;
        i.begin_frame(f);
        assert!(!i.journal_shortcut(), "a saved turn assignment on J wins");
        i.down[JOURNAL_DIK as usize] = false;
        v.set(0x66, 0x12);
        i.assign(&v, 0);
        i.down[0x16] = true;
        i.begin_frame(f);
        assert!(i.recipe_shortcut(), "default U opens the Crafting Log under the stock Modern tables");
        i.begin_frame(Frame { typing: true, ..f });
        assert!(!i.recipe_shortcut());
    }

    #[cfg(feature = "forever")]
    #[test]
    #[ignore = "requires player-owned FFXiMain.dll"]
    fn tracker_key_retail_camera_default_linkshell_and_live_remaps() {
        let mut i = real().expect("player-owned FFXiMain.dll fixture required");
        assert_eq!(i.compact.len(), 42);
        let mut v = crate::gamecfg::Values::default();
        i.assign(&v, 2);
        let f = Frame { modern: true, tracker_key: true, ..frame() };
        i.down[0x26] = true;
        i.begin_frame(frame());
        assert!(i.camera_key_axis(6) > 0.0, "stock L is camera right");
        assert!(!i.tracker_shortcut());
        i.down[0x26] = false;
        i.begin_frame(f);
        i.down[0x26] = true;
        i.begin_frame(f);
        assert!(i.tracker_shortcut(), "only the stock compact L records may be replaced");
        assert_eq!(i.camera_key_axis(6), 0.0, "tracker press must not turn the camera");
        i.down[0xcd] = true;
        i.begin_frame(f);
        assert_eq!(i.camera_key_axis(6), 1.0);
        i.down[0xcd] = false;
        i.set_pad(&[], [0.0, 0.0, 0.75, 0.0]);
        i.begin_frame(f);
        assert_eq!(i.axis(6), 0.75);
        i.set_pad(&[], [0.0; 4]);
        for modifier in [0x1d, 0x38] {
            i.down[modifier] = true;
            i.begin_frame(f);
            assert!(!i.tracker_shortcut());
            assert_eq!(super::super::chat_shortcut(egui::Key::L), Some("/linkshell "));
            i.down[modifier] = false;
        }
        i.begin_frame(Frame { window: true, ..f });
        assert!(!i.tracker_shortcut());
        assert_eq!(i.pad_held() & 0x10, 0x10, "stock L menu navigation is retained under window focus");
        i.begin_frame(Frame { tracker_key: false, ..f });
        assert!(i.camera_key_axis(6) > 0.0);
        v.set(0x66, 0x26);
        i.assign(&v, 2);
        i.down[0x26] = false;
        i.begin_frame(f);
        i.down[0x26] = true;
        i.begin_frame(f);
        assert!(!i.tracker_shortcut(), "a saved turn assignment wins");
        assert!(i.modern_keys().e);
        v.set(0x66, 0x12);
        v.set(0x4a, 0x26);
        i.assign(&v, 2);
        i.down[0x26] = false;
        i.begin_frame(f);
        i.down[0x26] = true;
        i.begin_frame(f);
        assert!(!i.tracker_shortcut());
        assert!(i.modern_keys().fwd, "custom movement L wins");
    }

    #[test]
    fn numpad_movement_does_not_also_navigate_menus() {
        use winit::keyboard::{Key, NamedKey};
        for (code, arrow, digit, axis, value) in [
            (KeyCode::Numpad2, NamedKey::ArrowDown, "2", 5, 1.0),
            (KeyCode::Numpad4, NamedKey::ArrowLeft, "4", 4, -1.0),
            (KeyCode::Numpad6, NamedKey::ArrowRight, "6", 4, 1.0),
            (KeyCode::Numpad8, NamedKey::ArrowUp, "8", 5, -1.0),
        ] {
            for typing in [false, true] {
                let mut i = Input::default();
                // The physical movement axis is enabled in the field/window context, as in the loaded key table.
                i.masks[axis] = 0xffff;
                let logical = Key::Named(arrow);
                assert!(!world_ui_key(code, &logical));
                i.key(code, true, &logical);
                i.begin_frame(Frame { window: true, typing, ..frame() });
                assert_eq!(i.axis(axis), value);
                i.key(code, false, &logical);
                i.begin_frame(Frame { window: true, typing, ..frame() });
                assert_eq!(i.axis(axis), 0.0);
                // NumLock on: the digit can be typed; typing then suppresses physical movement.
                let logical = Key::Character(digit.into());
                assert!(world_ui_key(code, &logical));
                i.key(code, true, &logical);
                i.begin_frame(Frame { window: true, typing, ..frame() });
                assert_eq!(i.axis(axis), if typing { 0.0 } else { value });
                i.clear();
                i.begin_frame(frame());
                assert_eq!(i.axis(axis), 0.0);
            }
        }
        for (code, arrow) in [
            (KeyCode::ArrowDown, NamedKey::ArrowDown),
            (KeyCode::ArrowLeft, NamedKey::ArrowLeft),
            (KeyCode::ArrowRight, NamedKey::ArrowRight),
            (KeyCode::ArrowUp, NamedKey::ArrowUp),
        ] {
            assert!(world_ui_key(code, &Key::Named(arrow)));
        }
    }

    fn real() -> Option<Input> {
        let dll = std::fs::read(crate::dat::ffximain(&crate::config::Config::load().ffxi_dir())).ok()?;
        let mut i = Input::default();
        i.load(&dll);
        Some(i)
    }

    fn frame() -> Frame {
        Frame { ticks: 1.0, keyboard: 2, ..Default::default() }
    }

    /// The tables match retail's (MODLOG "Input layer"): Num* on 0x44 (not typing, no Ctrl / Alt), Ctrl / Alt + F and
    /// Num+ on 0x78, 42 compact records, k1 / k2 rows.
    #[test]
    fn tables_from_ffximain() {
        let Some(i) = real() else { return };
        assert_eq!(i.bindings[0x44], [0xc037, 0, 0]);
        assert_eq!(i.bindings[0x78], [0x1021, 0x804e, 0]);
        assert_eq!((i.masks[0x78], i.masks[0x37], i.masks[0x44]), (0xffff, 0x3e, 0x08));
        assert_eq!(i.compact.len(), 42);
        assert_eq!(i.compact[6], Compact { key: 0x23, rule: 0, states: 4, bits: 0x400 });
        assert_eq!((i.k1.len(), i.k1[4].as_slice()), (28, &[11u8, 15, 19, 23][..]));
        assert_eq!((i.k2.len(), i.k2[0].as_slice()), (K2_ROWS, &[0x3bu16][..]));
    }

    /// The gamepad is another keyboard: its held keys press and repeat as the window's do, its sticks are axes the
    /// keys can outvote, and nothing changes while no pad is in use.
    #[test]
    fn gamepad_keys_and_sticks() {
        let mut i = Input::default();
        i.masks = vec![0xffff; IDS];
        i.begin_frame(frame());
        assert_eq!((i.axis(4), i.axis(5), i.axis(6), i.axis(7)), (0.0, 0.0, 0.0, 0.0));
        i.set_pad(&[0x1c], [0.5, -1.0, 0.25, 0.0]);
        i.begin_frame(frame());
        assert!(i.pressed(0x1c) && i.held(0x1c));
        assert_eq!((i.axis(4), i.axis(5), i.axis(6), i.axis(7)), (0.5, -1.0, 0.25, 0.0));
        // The map's scroll reads the left stick, its zoom the right one.
        assert_eq!((i.axis(0x79), i.axis(0x7a), i.axis(0x7c)), (0.5, -1.0, 0.0));
        // The zoom is its own value: the camera stick (axis 7) no longer moves it, and it does not tilt the camera.
        i.set_pad(&[], [0.0, 0.0, 0.0, 0.75]);
        i.begin_frame(frame());
        assert_eq!((i.axis(7), i.axis(0x7c)), (0.75, 0.0));
        i.set_pad(&[], [0.0; 4]);
        i.set_pad_zoom(-0.5);
        i.begin_frame(frame());
        assert_eq!((i.axis(7), i.axis(0x7c)), (0.0, -0.5));
        // The next set_pad (every frame) clears it.
        i.set_pad(&[], [0.0; 4]);
        i.begin_frame(frame());
        assert_eq!(i.axis(0x7c), 0.0);
        i.set_pad(&[0x1c], [0.5, -1.0, 0.25, 0.0]);
        i.begin_frame(frame());
        i.begin_frame(frame());
        assert!(!i.pressed(0x1c) && i.held(0x1c));
        // A key beats a weaker stick (the larger wins), a stronger stick beats a key.
        i.set_pad(&[], [0.5, 0.0, 0.0, 0.0]);
        i.key(KeyCode::Numpad6, true, &winit::keyboard::Key::Character("6".into()));
        i.begin_frame(frame());
        assert!(!i.held(0x1c) && i.axis(4) == 1.0);
        // Camera key inversion must not invert or double the controller stick.
        i.key(KeyCode::Numpad6, false, &winit::keyboard::Key::Character("6".into()));
        i.set_pad(&[], [0.0, 0.0, 0.75, -0.5]);
        i.begin_frame(frame());
        assert_eq!((i.axis(6), i.axis(7)), (0.75, -0.5));
        assert_eq!((i.camera_key_axis(6), i.camera_key_axis(7)), (0.0, 0.0));
        assert_eq!(i.device_stick(), [0.0, 0.0, 0.75, -0.5]);
        // Typing: the stick does not walk.
        i.set_pad(&[], [1.0, 1.0, 0.0, 0.0]);
        i.key(KeyCode::Numpad6, false, &winit::keyboard::Key::Character("6".into()));
        i.begin_frame(Frame { typing: true, ..frame() });
        assert_eq!((i.axis(4), i.axis(5)), (0.0, 0.0));
        // Losing the window's focus drops the pad's keys too.
        i.set_pad(&[0x1d], [0.0; 4]);
        i.clear();
        i.begin_frame(frame());
        assert!(!i.held(0x1d));
    }

    /// 0xb0..0xb3 want LT or RT held with the d-pad (slots 9 / 13 and 6 / 7 / 4 / 5).
    #[test]
    fn pad_ground_cursor_combos() {
        let mut i = Input::default();
        i.set_device_pad(0x10);
        i.begin_frame(frame());
        assert!(!i.pad_test(0xb2, mode::PRESS));
        i.set_device_pad(0);
        i.begin_frame(frame());
        i.set_device_pad(0x200 | 0x10);
        i.begin_frame(frame());
        assert!(i.pad_test(0xb2, mode::PRESS) && i.pad_test(0xb2, mode::REPEAT) && !i.pad_test(0xb3, mode::HELD));
        i.set_device_pad(0x2000 | 0x40);
        i.begin_frame(frame());
        assert!(i.pad_test(0xb0, mode::HELD) && !i.pad_test(0xb1, mode::HELD));
    }

    #[test]
    fn press_hold_repeat_release() {
        let Some(mut i) = real() else { return };
        let nl = winit::keyboard::Key::Named(winit::keyboard::NamedKey::NumLock);
        i.key(KeyCode::NumpadAdd, true, &nl);
        i.begin_frame(frame());
        assert!(i.test(0x78, mode::PRESS) && i.test(0x78, mode::REPEAT));
        // Held at 60 fps: retail frames of 2 ticks, past 16 the first repeat (17 or 18 ticks by the frame clock's
        // phase), then every 4.
        let mut repeats = Vec::new();
        for t in 1..=30 {
            i.begin_frame(frame());
            assert!(!i.test(0x78, mode::PRESS) && i.test(0x78, mode::HELD));
            if i.test(0x78, mode::REPEAT) {
                repeats.push(t);
            }
        }
        assert!(
            (17..=18).contains(&repeats[0]) && repeats.windows(2).all(|w| w[1] - w[0] == 4) && repeats.len() == 4,
            "{repeats:?}"
        );
        i.key(KeyCode::NumpadAdd, false, &nl);
        i.begin_frame(frame());
        assert!(!i.test(0x78, mode::HELD));
        // A tap inside one frame still presses.
        i.key(KeyCode::NumpadAdd, true, &nl);
        i.key(KeyCode::NumpadAdd, false, &nl);
        i.begin_frame(frame());
        assert!(i.test(0x78, mode::PRESS));
    }

    #[test]
    fn modifiers_typing_and_consume() {
        let Some(mut i) = real() else { return };
        let k = winit::keyboard::Key::Character("f".into());
        // Ctrl + F is 0x78 (flag 0x10); with a full keyboard bare F is not.
        i.key(KeyCode::KeyF, true, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        assert!(!i.test(0x78, mode::PRESS));
        i.key(KeyCode::ControlLeft, true, &k);
        i.key(KeyCode::KeyF, false, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        i.key(KeyCode::KeyF, true, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        assert!(i.test(0x78, mode::PRESS));
        // Not while Key Assignment is open (the gate), nor typing (mask 0xffff).
        i.key(KeyCode::KeyF, false, &k);
        i.begin_frame(Frame { keyboard: 0, ..frame() });
        i.key(KeyCode::KeyF, true, &k);
        i.begin_frame(Frame { keyboard: 0, assigning: true, ..frame() });
        assert!(!i.test(0x78, mode::PRESS));
        i.key(KeyCode::ControlLeft, false, &k);
        i.key(KeyCode::KeyF, false, &k);
        i.begin_frame(frame());
        // Ctrl / Alt + T (0x38, mask 0x3e) works while typing; Num+ (0x78) does not.
        i.key(KeyCode::AltLeft, true, &k);
        i.key(KeyCode::KeyT, true, &k);
        i.key(KeyCode::NumpadAdd, true, &k);
        i.begin_frame(Frame { typing: true, ..frame() });
        assert!(i.test(0x38, mode::PRESS) && !i.test(0x78, mode::PRESS));
        // A consumed frame fails everything but mode 4; mode 3 eats the press.
        i.begin_frame(frame());
        i.key(KeyCode::AltLeft, false, &k);
        i.key(KeyCode::KeyT, false, &k);
        i.key(KeyCode::Escape, true, &k);
        i.begin_frame(frame());
        assert!(i.test(0x92, mode::PRESS_CONSUME) && !i.test(0x92, mode::PRESS));
        i.consume();
        assert!(!i.test(0x94, mode::PRESS) && i.test(0x94, mode::HELD));
    }

    /// Compact keyboard: bare F is the pad's 4 bit (0x78 fires); W holds the stick; Ctrl + I (a k2 key) is the
    /// Items shortcut, not the camera.
    #[test]
    fn compact_keys() {
        let Some(mut i) = real() else { return };
        let k = winit::keyboard::Key::Character("x".into());
        i.key(KeyCode::KeyF, true, &k);
        i.key(KeyCode::KeyW, true, &k);
        i.begin_frame(frame());
        assert!(i.test(0x78, mode::PRESS));
        assert_eq!(i.pad_held() & 0x800004, 0x800004);
        i.begin_frame(frame());
        assert!(!i.test(0x78, mode::PRESS) && i.test(0x78, mode::HELD));
        i.key(KeyCode::KeyF, false, &k);
        i.key(KeyCode::KeyW, false, &k);
        i.key(KeyCode::ControlLeft, true, &k);
        i.key(KeyCode::KeyI, true, &k);
        i.begin_frame(frame());
        assert!(i.test(0x39, mode::PRESS));
        assert_eq!(i.pad_held() & 0x80000, 0);
        // Typing turns the compact keys off.
        i.key(KeyCode::ControlLeft, false, &k);
        i.key(KeyCode::KeyI, false, &k);
        i.key(KeyCode::KeyH, true, &k);
        i.begin_frame(Frame { typing: true, ..frame() });
        assert!(!i.test(0x44, mode::PRESS));
    }

    /// Modern's keys come from the compact records of FFXiMain: one record per stick direction (W / S / A / D) and the
    /// autorun record (R), and the type 2 turn keys' defaults are Q / E. Skipped without the game's DLL.
    #[test]
    fn modern_keys_from_the_real_tables() {
        let Some(i) = real() else { return };
        assert_eq!(
            [0x80_0000, 0x100_0000, 0x40_0000, 0x20_0000, 0x100].map(|b| i
                .compact
                .iter()
                .filter(|c| c.bits == b)
                .map(|c| c.key)
                .collect::<Vec<_>>()),
            [vec![0x11], vec![0x1f], vec![0x1e], vec![0x20], vec![0x13]]
        );
        assert_eq!(i.turn_keys, [0x10, 0x12]);
    }

    /// The Modern scheme's keys (controls.rs): W / S / A / D and Q / E held, only while the scheme is on and nothing
    /// else has the keys; Key Assignment's keys replace them.
    #[test]
    fn modern_keys() {
        use super::super::controls::Keys;
        let mut i = Input::default();
        i.masks = vec![0xffff; IDS];
        let k = winit::keyboard::Key::Character("x".into());
        let modern = || Frame { modern: true, keyboard: 0, ..frame() };
        for c in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyE] {
            i.key(c, true, &k);
        }
        // Off while the scheme is: nothing (the other schemes' keys go through the key functions).
        i.begin_frame(Frame { modern: false, ..modern() });
        assert_eq!(i.modern_keys(), Keys::default());
        i.begin_frame(modern());
        assert_eq!(i.modern_keys(), Keys { fwd: true, left: true, e: true, ..Default::default() });
        // A text field, Key Assignment, the wrong input context and Ctrl / Alt (a menu shortcut) silence it.
        i.begin_frame(Frame { typing: true, ..modern() });
        assert_eq!(i.modern_keys(), Keys::default());
        i.begin_frame(Frame { assigning: true, ..modern() });
        assert_eq!(i.modern_keys(), Keys::default());
        i.context = 0x26;
        i.masks[4] = 0x0001;
        i.begin_frame(modern());
        assert_eq!(i.modern_keys(), Keys::default());
        i.context = 0xffff;
        i.masks[4] = 0xffff;
        i.key(KeyCode::AltLeft, true, &k);
        i.begin_frame(modern());
        assert_eq!(i.modern_keys(), Keys::default());
        i.key(KeyCode::AltLeft, false, &k);
        // Key Assignment: the stick's up key moved to I (0x17), Turn Right to G (0x22), the autorun key to T (0x14).
        i.base_compact = vec![
            Compact { key: 0x17, rule: 0, states: 6, bits: 0x80_0000 },
            Compact { key: 0x14, rule: 0, states: 6, bits: 0x100 },
        ];
        let mut v = crate::gamecfg::Values::default();
        v.set(0x66, 0x22);
        i.assign(&v, 0);
        i.begin_frame(modern());
        // W no longer walks; I does, and G turns right (E no longer does).
        assert_eq!(i.modern_keys(), Keys { left: true, ..Default::default() });
        i.key(KeyCode::KeyI, true, &k);
        i.key(KeyCode::KeyG, true, &k);
        i.key(KeyCode::KeyT, true, &k);
        i.begin_frame(modern());
        assert_eq!(i.modern_keys(), Keys { fwd: true, left: true, e: true, ..Default::default() });
        assert!(i.modern_autorun());
        i.begin_frame(modern());
        assert!(!i.modern_autorun());
        // Losing the focus drops them all.
        i.clear();
        i.begin_frame(modern());
        assert_eq!(i.modern_keys(), Keys::default());
    }

    /// Key Assignment: k2 row 0 (Attack, 0x3b) to G; k1 row 6 (Menu Cursor & Camera Left) to U; turns on type 2.
    #[test]
    fn key_assignment() {
        let Some(mut i) = real() else { return };
        let mut v = crate::gamecfg::Values::default();
        v.set(0x6a, 0x22);
        v.set(0x50, 0x16);
        i.assign(&v, v.get(0x42));
        assert_eq!(i.bindings[0x3b][0], 0x1022);
        assert!([13usize, 17, 21, 25].iter().all(|&r| i.compact[r].key == 0x16));
        assert_eq!((i.bindings[0xaa][0], i.bindings[0xa9][0]), (0x8012, 0x8010));
        v.set(0x42, 0);
        i.assign(&v, v.get(0x42));
        assert_eq!(i.bindings[0xaa][0], i.base_bindings[0xaa][0]);
    }
}
