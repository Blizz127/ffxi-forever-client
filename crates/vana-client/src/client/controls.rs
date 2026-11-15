//! Ours (not retail): the Controls choice of Settings. Three schemes (a fourth, Forever, on Forever) for the keyboard and mouse, independent of the
//! interface style:
//!
//! - Modern: WASD and the mouse as in WoW (the default). W / S walk forward / back, A / D strafe (or turn: the "A / D"
//!   row), Q / E the other pair; the right button held is mouselook (the character faces where the camera looks and
//!   A / D / Q / E strafe), both buttons run forward, a right click on an actor interacts as Enter on the target does.
//!   Retail's compact keyboard (keyboard type 1 / 2) is off under it, so the other letters keep no retail function.
//! - FFXI WASD: retail's keyboard type 2 (compact keys, Q / E turn, Shift faces the camera) whatever the Config says.
//! - Original: retail's keyboard as the Config's Keyboard type sets it, nothing of ours on the keys.
//!
//! - Forever (Forever flavour on a Forever server only): Modern's keys and mouse, plus a right-click *tap* (short and
//!   still, not a look drag) backs out of the focused menu or window one level, as Escape and the Back button do. A
//!   widget that already gives the right click a meaning keeps it. The rules for the tap live here (`Tap`); the
//!   dispatch is in modern/mod.rs.
//!
//! The mouse as before (left drag orbits the camera, the wheel zooms, a left click targets, a right drag looks, both
//! buttons run) is the same in all three. Modern's own: the keys, and the right click that interacts (the right button
//! then leaves the target as it is after a look drag; retail's right click clears it). The pure rules live here so they
//! can be tested without a window.

use std::time::Duration;

/// The Controls row's choices (config `controls`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Original,
    FfxiWasd,
    Modern,
    /// Modern plus the right-click tap that backs out of menus (Forever only; see `resolve`).
    Hybrid,
}

impl Scheme {
    pub fn from_config(v: u32) -> Self {
        match v {
            0 => Self::Original,
            1 => Self::FfxiWasd,
            3 => Self::Hybrid,
            _ => Self::Modern,
        }
    }

    /// The scheme in force: Forever exists only on a Forever server of the Forever flavour (`forever`: the config's
    /// is_forever and not classic_only), and is Modern elsewhere. The saved choice is left alone so it comes back
    /// when the player is on Forever again; every use of the scheme goes through here.
    pub fn resolve(config: u32, forever: bool) -> Self {
        match Self::from_config(config) {
            Self::Hybrid if !forever => Self::Modern,
            s => s,
        }
    }

    /// Modern's keys, movement and right click (Forever has all of them).
    pub fn is_modern(self) -> bool {
        matches!(self, Self::Modern | Self::Hybrid)
    }

    /// A right-click tap backs out of the focused menu (the Forever flavour's modern layer reads it).
    #[cfg_attr(not(feature = "forever"), allow(dead_code))]
    pub fn right_back(self) -> bool {
        self == Self::Hybrid
    }

    /// The keyboard type (config 0x42: 0 full, 1 / 2 compact) the key functions run under. Modern turns the compact
    /// keyboard off (its own WASD replaces the compact stick); FFXI WASD is type 2; Original is what the Config says.
    pub fn keyboard_type(self, config: u32) -> u32 {
        match self {
            Self::Original => config,
            Self::FfxiWasd => 2,
            Self::Modern | Self::Hybrid => 0,
        }
    }
}

/// The Modern keys held now, by role (their keys come from Key Assignment, see input.rs `modern_keys`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    /// W, S, A, D: the compact stick's up, down, left and right.
    pub fwd: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    /// Q and E: the type 2 turn keys.
    pub q: bool,
    pub e: bool,
}

/// What the Modern keys and buttons ask of the movement this frame: `ax` strafe (right positive), `ay` forward
/// (negative, as the movement axes are), `turn` the character's turn (right positive, as retail's 0xaa).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    pub ax: f32,
    pub ay: f32,
    pub turn: f32,
}

/// The Modern scheme's movement. A / D strafe and Q / E turn, or with `ad_turn` the other way round. The right button
/// held (mouselook: the character already faces the camera) makes all four strafe, as WoW does. Both buttons run
/// forward.
pub fn intent(ad_turn: bool, k: Keys, left_button: bool, right_button: bool) -> Intent {
    let axis = |p: bool, n: bool| p as i32 as f32 - n as i32 as f32;
    let both = left_button && right_button;
    let (a_d, q_e) = (axis(k.right, k.left), axis(k.e, k.q));
    let (mut side, mut turn) = if ad_turn { (q_e, a_d) } else { (a_d, q_e) };
    if right_button {
        side += turn;
        turn = 0.0;
    }
    Intent { ax: side.clamp(-1.0, 1.0), ay: axis(k.back, k.fwd || both), turn }
}

/// A right button release that interacts rather than ends a mouselook: it moved less than a few pixels and was not
/// held long.
pub fn is_click(moved: f64, held: Option<Duration>) -> bool {
    moved < CLICK_MOVE && held.is_some_and(|h| h < CLICK_TIME)
}

/// Pixels of look motion, and time, below which a right button press and release is a click.
const CLICK_MOVE: f64 = 6.0;
const CLICK_TIME: Duration = Duration::from_millis(350);

/// A right button press and release that may be a tap (the Forever scheme's back-out). It is seen before the UI can
/// consume the press, so it is a tap whatever lay under the pointer. Not a tap: held too long or moved (the cursor
/// between press and release, or the look motion fed in meanwhile), another button pressed or held, no press seen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tap {
    down: Option<(std::time::Instant, Option<(f64, f64)>)>,
    /// Look motion since the press (pixels, or raw counts while the pointer is locked).
    moved: f64,
    /// Another button went down during the press.
    spoiled: bool,
}

impl Tap {
    /// The right button went down; `other_held`: the left or middle button already is.
    pub fn press(&mut self, now: std::time::Instant, at: Option<(f64, f64)>, other_held: bool) {
        *self = Self { down: Some((now, at)), moved: 0.0, spoiled: other_held };
    }

    /// Another button went down: this is no tap (both buttons run).
    pub fn spoil(&mut self) {
        self.spoiled = true;
    }

    /// Look motion while the button is down.
    pub fn look(&mut self, d: f64) {
        if self.down.is_some() {
            self.moved += d;
        }
    }

    /// The right button went up: was it a tap? Always forgets the press (a second release is none).
    pub fn release(&mut self, now: std::time::Instant, at: Option<(f64, f64)>, other_held: bool) -> bool {
        let t = std::mem::take(self);
        let Some((since, from)) = t.down else { return false };
        let cursor = from.zip(at).map_or(0.0, |(a, b)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt());
        !t.spoiled && !other_held && is_click(t.moved.max(cursor), Some(now.saturating_duration_since(since)))
    }
}

/// The mouse buttons and where they went down (what the world's clicks and drags read).
#[derive(Default)]
pub struct Pointer {
    /// The right button's tap in progress (the Forever scheme).
    pub tap: Tap,
    /// An input context took over this gesture. Keep button positions for map
    /// panning, but do not resume camera capture until all buttons are released.
    pub camera_blocked: bool,
    /// The left, right (a look drag, mouselook) and middle buttons.
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    /// Forever reserves middle for autorun/UI; it must not also capture the camera.
    pub middle_reserved: bool,
    /// Where (and when) the left button went down, and the right one: the drag rule (FUN_10125040).
    pub press_at: Option<(f64, f64)>,
    pub press_time: Option<std::time::Instant>,
    pub right_press: Option<((f64, f64), std::time::Instant)>,
    /// Where the right button went down (the camera arrow's reference).
    pub drag_from: Option<(f64, f64)>,
    /// Look motion since the right button went down (pixels, or raw counts while the pointer is locked).
    pub right_moved: f64,
}

impl Pointer {
    /// A map/event taking over cancels the camera gesture while retaining map
    /// panning input; closing it while a button remains held must
    /// not resurrect the old camera drag.
    pub fn look_requested(&mut self, at: Option<(f64, f64)>, available: bool) -> bool {
        if !available {
            self.camera_blocked = self.any_down();
            return false;
        }
        if self.camera_blocked {
            return false;
        }
        self.right
            || (self.middle && !self.middle_reserved)
            || (self.left && self.press_at.zip(at).is_some_and(|(a, b)| (a.0 - b.0).abs() + (a.1 - b.1).abs() >= 5.0))
    }

    /// One action per physical press, including a press that a UI context rejects.
    pub fn middle_button(&mut self, pressed: bool, reserved: bool) -> bool {
        let fresh = pressed && !self.middle;
        self.middle = pressed;
        self.middle_reserved = pressed && reserved;
        fresh
    }

    pub fn any_down(&self) -> bool {
        self.left || self.right || self.middle
    }

    /// A button event, before the UI or the world see it (so `left`, `middle` are as before it): true when a right
    /// button release completes a tap.
    pub fn tap_event(&mut self, right: bool, pressed: bool, now: std::time::Instant, at: Option<(f64, f64)>) -> bool {
        match (right, pressed) {
            (true, true) => self.tap.press(now, at, self.left || self.middle),
            (true, false) => return self.tap.release(now, at, self.left || self.middle),
            (false, true) => self.tap.spoil(),
            (false, false) => {}
        }
        false
    }

    /// Focus changed: no button is held and no drag is under way (the window gets no releases while it is away, and a
    /// stale both-buttons state kept the character running).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Own the window which granted a camera grab. Redraw temporarily removes Gfx
/// from App; losing that optional field must never lose the native release handle.
pub struct Capture<W> {
    window: Option<W>,
    releasing: bool,
}
impl<W> Default for Capture<W> {
    fn default() -> Self {
        Self { window: None, releasing: false }
    }
}
impl<W> Capture<W> {
    pub fn held(&self) -> bool {
        self.window.is_some()
    }
    pub fn active(&self) -> bool {
        self.held() && !self.releasing
    }
    pub fn releasing(&self) -> bool {
        self.releasing
    }
    pub fn begin(&mut self, window: W) {
        self.window = Some(window);
        self.releasing = false;
    }
    /// A failed native release retains its handle for retry, and stops camera motion.
    pub fn release(&mut self, release: impl FnOnce(&W) -> bool) -> bool {
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        self.releasing = true;
        if !release(window) {
            return false;
        }
        self.window = None;
        self.releasing = false;
        true
    }
}

/// Focus lost or gained: the buttons, the drag and the mouselook state all cleared. The caller gives the pointer back
/// (App::lose_focus).
pub fn lose_focus(ptr: &mut Pointer, mouse_look: &mut bool) {
    ptr.clear();
    *mouse_look = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_gesture_is_cancelled_when_map_or_event_takes_over() {
        let mut p = Pointer {
            right: true,
            drag_from: Some((20.0, 30.0)),
            right_press: Some(((20.0, 30.0), std::time::Instant::now())),
            ..Default::default()
        };
        assert!(p.look_requested(Some((25.0, 30.0)), true));
        assert!(!p.look_requested(Some((25.0, 30.0)), false));
        assert!(p.right && p.camera_blocked);
        assert!(!p.look_requested(Some((50.0, 30.0)), true));
        p.clear();
        p.left = true;
        p.press_at = Some((50.0, 30.0));
        assert!(!p.look_requested(Some((52.0, 30.0)), true));
        assert!(p.look_requested(Some((56.0, 30.0)), true));
        p.look_requested(Some((56.0, 30.0)), false);
        assert!(p.left && p.press_at.is_some()); // The map still needs its pan origin.
        assert!(!p.look_requested(Some((60.0, 30.0)), true));
        p.clear();
        p.right = true;
        assert!(p.look_requested(Some((60.0, 30.0)), true));
    }

    #[test]
    fn map_open_during_redraw_releases_native_handle_and_close_needs_fresh_press() {
        use std::{cell::Cell, rc::Rc};
        // Fake native grab state is shared by the window, exactly as the real Arc<Window>.
        let window = Rc::new(Cell::new(true));
        let mut gfx = Some(window.clone());
        let mut capture = Capture::default();
        capture.begin(gfx.as_ref().unwrap().clone());
        let mut pointer = Pointer { right: true, drag_from: Some((20.0, 30.0)), ..Default::default() };
        assert!(pointer.look_requested(Some((40.0, 30.0)), true));
        let frame = gfx.take(); // M is handled inside redraw, with App.gfx absent.
        assert!(gfx.is_none());
        assert!(!pointer.look_requested(Some((40.0, 30.0)), false));
        assert!(capture.release(|window| {
            window.set(false);
            true
        }));
        assert!(!window.get() && !capture.held());
        gfx = frame;
        // M/Escape closes the map while the old right button is still down.
        assert!(!pointer.look_requested(Some((60.0, 30.0)), true));
        pointer.clear(); // Button up, even if UI consumed it.
        pointer.right = true;
        assert!(pointer.look_requested(Some((60.0, 30.0)), true));
        window.set(true);
        capture.begin(gfx.as_ref().unwrap().clone());
        assert!(capture.active());
        pointer.clear();
        assert!(capture.release(|window| {
            window.set(false);
            true
        }));
        assert!(!window.get());
    }

    #[test]
    fn native_release_failure_retains_handle_and_recovery_is_repeatable() {
        let mut capture = Capture::default();
        capture.begin(17);
        assert!(!capture.release(|w| {
            assert_eq!(*w, 17);
            false
        }));
        assert!(capture.held() && capture.releasing() && !capture.active());
        assert!(capture.release(|w| {
            assert_eq!(*w, 17);
            true
        }));
        assert!(!capture.held() && !capture.releasing());
        assert!(!capture.release(|_| panic!("already released")));
        // Explicit Escape/focus recovery can release a window even when old app bookkeeping was lost.
        capture.begin(18);
        assert!(capture.release(|w| *w == 18));
        // Map opened without mouse-look must not attempt a phantom native release.
        assert!(!capture.release(|_| panic!("no camera grab")));
    }

    fn k(s: &str) -> Keys {
        Keys {
            fwd: s.contains('w'),
            back: s.contains('s'),
            left: s.contains('a'),
            right: s.contains('d'),
            q: s.contains('q'),
            e: s.contains('e'),
        }
    }

    #[test]
    fn schemes_and_keyboard_types() {
        assert_eq!(
            (Scheme::from_config(0), Scheme::from_config(1), Scheme::from_config(2), Scheme::from_config(9)),
            (Scheme::Original, Scheme::FfxiWasd, Scheme::Modern, Scheme::Modern)
        );
        assert_eq!(Scheme::from_config(3), Scheme::Hybrid);
        // Original keeps the Config's type, FFXI WASD is 2, Modern leaves the compact keyboard off.
        for config in 0..3 {
            assert_eq!(Scheme::Original.keyboard_type(config), config);
            assert_eq!(Scheme::FfxiWasd.keyboard_type(config), 2);
            assert_eq!(Scheme::Modern.keyboard_type(config), 0);
        }
    }

    #[test]
    fn hybrid_scheme_maps_config_and_keeps_modern_keys() {
        // Forever is config 3; it has Modern's keys, movement and right click, and only it backs out on a tap.
        assert_eq!(Scheme::from_config(3), Scheme::Hybrid);
        for s in [Scheme::Modern, Scheme::Hybrid] {
            assert!(s.is_modern());
            for config in 0..3 {
                assert_eq!(s.keyboard_type(config), 0);
            }
        }
        assert!(!Scheme::Original.is_modern() && !Scheme::FfxiWasd.is_modern());
        assert_eq!(
            [Scheme::Original, Scheme::FfxiWasd, Scheme::Modern, Scheme::Hybrid].map(Scheme::right_back),
            [false, false, false, true]
        );
        // Off Forever the saved choice plays as Modern; the others are as saved.
        assert_eq!(Scheme::resolve(3, false), Scheme::Modern);
        assert_eq!(Scheme::resolve(3, true), Scheme::Hybrid);
        for (config, want) in [(0, Scheme::Original), (1, Scheme::FfxiWasd), (2, Scheme::Modern)] {
            assert_eq!((Scheme::resolve(config, true), Scheme::resolve(config, false)), (want, want));
        }
        // Movement is Modern's: the same intent under both.
        assert_eq!(intent(false, k("wd"), false, true), Intent { ax: 1.0, ay: -1.0, turn: 0.0 });
        assert_eq!(intent(false, k(""), true, true).ay, -1.0);
    }

    #[test]
    fn tap_tracker_clean_tap_only() {
        let t0 = std::time::Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let at = Some((100.0, 100.0));
        let mut tap = Tap::default();
        // No press seen.
        assert!(!tap.release(ms(50), at, false));
        // A clean tap, and the same press is not a tap twice.
        tap.press(ms(0), at, false);
        assert!(tap.release(ms(100), at, false));
        assert!(!tap.release(ms(110), at, false));
        // Too long.
        tap.press(ms(0), at, false);
        assert!(!tap.release(ms(400), at, false));
        // Cursor moved 7 px (but 5 px is still a tap).
        tap.press(ms(0), at, false);
        assert!(!tap.release(ms(100), Some((107.0, 100.0)), false));
        tap.press(ms(0), at, false);
        assert!(tap.release(ms(100), Some((103.0, 104.0)), false));
        // Look motion with a locked pointer (the cursor does not move).
        tap.press(ms(0), at, false);
        tap.look(4.0);
        tap.look(3.0);
        assert!(!tap.release(ms(100), at, false));
        tap.press(ms(0), at, false);
        tap.look(5.0);
        assert!(tap.release(ms(100), at, false));
        // Look motion with no press in progress counts for nothing; a new press starts clean.
        tap.look(500.0);
        tap.press(ms(0), at, false);
        assert!(tap.release(ms(100), at, false));
        // The left (or middle) button held at the press or release, or pressed in between: both-buttons run, no tap.
        tap.press(ms(0), at, true);
        assert!(!tap.release(ms(100), at, false));
        tap.press(ms(0), at, false);
        assert!(!tap.release(ms(100), at, true));
        tap.press(ms(0), at, false);
        tap.spoil();
        assert!(!tap.release(ms(100), at, false));
        // Unknown cursor positions count as no cursor travel.
        tap.press(ms(0), None, false);
        assert!(tap.release(ms(100), None, false));
        // Cleared between (focus loss, a mod taking the release): no tap.
        tap.press(ms(0), at, false);
        let mut p = Pointer { tap, ..Default::default() };
        p.clear();
        assert!(!p.tap.release(ms(100), at, false));
    }

    #[test]
    fn right_tap_never_sets_look_or_right() {
        let t0 = std::time::Instant::now();
        let at = Some((10.0, 10.0));
        let mut p = Pointer::default();
        assert!(!p.tap_event(true, true, t0, at));
        // The tap is judged before the world's own button handling: nothing of the gesture is set by it.
        assert!(!p.right && !p.left && !p.middle && !p.camera_blocked && p.right_press.is_none());
        assert!(!p.look_requested(at, true));
        assert!(p.tap_event(true, false, t0 + Duration::from_millis(80), at));
        assert!(!p.right && !p.any_down() && p.drag_from.is_none());
        // A left press while the right is down spoils it; a left already held at the press does too.
        p.tap_event(true, true, t0, at);
        assert!(!p.tap_event(false, true, t0, at));
        assert!(!p.tap_event(true, false, t0 + Duration::from_millis(80), at));
        p.left = true;
        p.tap_event(true, true, t0, at);
        assert!(!p.tap_event(true, false, t0 + Duration::from_millis(80), at));
        p.left = false;
        // Other buttons' releases and a left press with no right down are no tap.
        assert!(!p.tap_event(false, false, t0, at));
        assert!(!p.tap_event(false, true, t0, at));
    }

    #[test]
    fn wasd_walks_and_strafes() {
        assert_eq!(intent(false, k(""), false, false), Intent::default());
        assert_eq!(intent(false, k("w"), false, false), Intent { ay: -1.0, ..Default::default() });
        assert_eq!(intent(false, k("s"), false, false), Intent { ay: 1.0, ..Default::default() });
        assert_eq!(intent(false, k("a"), false, false), Intent { ax: -1.0, ..Default::default() });
        assert_eq!(intent(false, k("wd"), false, false), Intent { ax: 1.0, ay: -1.0, turn: 0.0 });
        // Opposite keys cancel.
        assert_eq!(intent(false, k("ws"), false, false), Intent::default());
        assert_eq!(intent(false, k("ad"), false, false), Intent::default());
    }

    #[test]
    fn q_e_turn_unless_a_d_do() {
        // A / D strafe: Q / E turn (E right, as retail's 0xaa).
        assert_eq!(intent(false, k("e"), false, false), Intent { turn: 1.0, ..Default::default() });
        assert_eq!(intent(false, k("q"), false, false), Intent { turn: -1.0, ..Default::default() });
        // A / D turn: Q / E strafe.
        assert_eq!(intent(true, k("d"), false, false), Intent { turn: 1.0, ..Default::default() });
        assert_eq!(intent(true, k("a"), false, false), Intent { turn: -1.0, ..Default::default() });
        assert_eq!(intent(true, k("q"), false, false), Intent { ax: -1.0, ..Default::default() });
        assert_eq!(intent(true, k("e"), false, false), Intent { ax: 1.0, ..Default::default() });
        // Turning while walking forward is both at once.
        assert_eq!(intent(true, k("wd"), false, false), Intent { ay: -1.0, turn: 1.0, ..Default::default() });
    }

    #[test]
    fn mouselook_strafes_with_all_four_keys() {
        for ad_turn in [false, true] {
            assert_eq!(intent(ad_turn, k("a"), false, true), Intent { ax: -1.0, ..Default::default() });
            assert_eq!(intent(ad_turn, k("d"), false, true), Intent { ax: 1.0, ..Default::default() });
            assert_eq!(intent(ad_turn, k("q"), false, true), Intent { ax: -1.0, ..Default::default() });
            assert_eq!(intent(ad_turn, k("e"), false, true), Intent { ax: 1.0, ..Default::default() });
            // Never past full: A and Q together are one strafe.
            assert_eq!(intent(ad_turn, k("aq"), false, true).ax, -1.0);
            assert_eq!(intent(ad_turn, k("w"), false, true).turn, 0.0);
        }
    }

    #[test]
    fn both_buttons_run_forward() {
        assert_eq!(intent(false, k(""), true, true), Intent { ay: -1.0, ..Default::default() });
        assert_eq!(intent(false, k(""), true, false), Intent::default());
        assert_eq!(intent(false, k(""), false, true), Intent::default());
        // Back against both buttons stands still; W with them is still forward.
        assert_eq!(intent(false, k("s"), true, true).ay, 0.0);
        assert_eq!(intent(false, k("w"), true, true).ay, -1.0);
        assert_eq!(intent(false, k("a"), true, true), Intent { ax: -1.0, ay: -1.0, turn: 0.0 });
    }

    #[test]
    fn a_right_click_is_short_and_still() {
        let ms = Duration::from_millis;
        assert!(is_click(0.0, Some(ms(80))));
        assert!(is_click(5.9, Some(ms(300))));
        assert!(!is_click(6.0, Some(ms(80))));
        assert!(!is_click(0.0, Some(ms(350))));
        assert!(!is_click(0.0, Some(ms(900))));
        // Never pressed in the window (a release alone).
        assert!(!is_click(0.0, None));
    }

    #[test]
    fn focus_loss_clears_buttons_and_mouselook() {
        let now = std::time::Instant::now();
        let mut p = Pointer {
            tap: Tap::default(),
            camera_blocked: true,
            left: true,
            right: true,
            middle: true,
            middle_reserved: true,
            press_at: Some((1.0, 2.0)),
            press_time: Some(now),
            right_press: Some(((3.0, 4.0), now)),
            drag_from: Some((3.0, 4.0)),
            right_moved: 40.0,
        };
        let mut look = true;
        assert!(p.any_down());
        lose_focus(&mut p, &mut look);
        assert!(!p.any_down() && !look);
        assert!(p.press_at.is_none() && p.press_time.is_none() && p.right_press.is_none() && p.drag_from.is_none());
        assert_eq!(p.right_moved, 0.0);
        // With nothing held the Modern intent is the idle one: no stuck run.
        assert_eq!(intent(false, Keys::default(), p.left, p.right), Intent::default());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod capture_native_tests {
    use super::{Capture, Pointer};
    use std::sync::Arc;
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        window::{CursorGrabMode, Window, WindowId},
    };

    /// Explicitly opt in on the dedicated offline display; never use an owner's desktop.
    #[test]
    #[ignore = "requires dedicated Xvfb :77; no game/login"]
    fn map_capture_native_window_survives_redraw_and_recovers() {
        assert_eq!(std::env::var("DISPLAY").as_deref(), Ok(":77"));
        assert_eq!(std::env::var("VANA_CURSOR_SMOKE").as_deref(), Ok("1"));
        fn warp_outside() -> (i32, i32) {
            let script = r#"
import ctypes as c
x=c.CDLL('libX11.so.6')
x.XOpenDisplay.restype=c.c_void_p
x.XDefaultRootWindow.argtypes=[c.c_void_p];x.XDefaultRootWindow.restype=c.c_ulong
x.XWarpPointer.argtypes=[c.c_void_p,c.c_ulong,c.c_ulong,c.c_int,c.c_int,c.c_uint,c.c_uint,c.c_int,c.c_int]
x.XSync.argtypes=[c.c_void_p,c.c_int]
x.XQueryPointer.argtypes=[c.c_void_p,c.c_ulong,c.POINTER(c.c_ulong),c.POINTER(c.c_ulong),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_uint)]
x.XCloseDisplay.argtypes=[c.c_void_p]
d=x.XOpenDisplay(b':77');assert d
r=x.XDefaultRootWindow(d)
x.XWarpPointer(d,0,r,0,0,0,0,600,500);x.XSync(d,0)
a=c.c_ulong();b=c.c_ulong();rx=c.c_int();ry=c.c_int();wx=c.c_int();wy=c.c_int();mask=c.c_uint()
assert x.XQueryPointer(d,r,c.byref(a),c.byref(b),c.byref(rx),c.byref(ry),c.byref(wx),c.byref(wy),c.byref(mask))
print(rx.value,ry.value);x.XCloseDisplay(d)
"#;
            let r = std::process::Command::new("python3").args(["-c", script]).output().unwrap();
            assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
            let s = String::from_utf8(r.stdout).unwrap();
            let mut n = s.split_whitespace().map(|v| v.parse().unwrap());
            (n.next().unwrap(), n.next().unwrap())
        }
        struct Smoke {
            window: Option<Arc<Window>>,
            done: bool,
            started: std::time::Instant,
        }
        impl ApplicationHandler for Smoke {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let w = Arc::new(
                    event_loop
                        .create_window(
                            Window::default_attributes()
                                .with_title("Vana offline cursor capture smoke")
                                .with_inner_size(winit::dpi::PhysicalSize::new(128, 128))
                                .with_position(winit::dpi::PhysicalPosition::new(64, 64)),
                        )
                        .unwrap(),
                );
                w.request_redraw();
                self.window = Some(w);
            }
            fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
                if self.done || !matches!(event, WindowEvent::RedrawRequested) {
                    return;
                }
                let mut capture = Capture::default();
                let window = self.window.take().unwrap(); // Same ownership gap as App::redraw.
                for cycle in 0..2 {
                    window.set_cursor_grab(CursorGrabMode::Confined).unwrap();
                    capture.begin(window.clone());
                    let before = warp_outside();
                    assert_ne!(before, (600, 500), "native confinement must be established");
                    let mut pointer = Pointer { right: true, ..Default::default() };
                    assert!(!pointer.look_requested(Some((80.0, 80.0)), false)); // Map takes input.
                    assert!(self.window.is_none());
                    assert!(capture.release(|w| w.set_cursor_grab(CursorGrabMode::None).is_ok()));
                    let after = warp_outside();
                    assert_eq!(after, (600, 500), "pointer still confined after map open");
                    assert!(!pointer.look_requested(Some((80.0, 80.0)), true)); // Map closes; held gesture stays blocked.
                    pointer.clear();
                    pointer.right = true;
                    assert!(pointer.look_requested(Some((80.0, 80.0)), true));
                    println!(
                        "native cycle {cycle}: confined {before:?}, map release {after:?}, fresh camera gesture PASS"
                    );
                }
                self.window = Some(window);
                self.done = true;
                event_loop.exit();
            }
            fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
                if self.started.elapsed().as_secs() > 5 {
                    event_loop.exit();
                }
            }
        }
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut b = EventLoop::builder();
        b.with_x11().with_any_thread(true);
        let e = b.build().unwrap();
        let mut app = Smoke { window: None, done: false, started: std::time::Instant::now() };
        e.run_app(&mut app).unwrap();
        assert!(app.done, "native cursor smoke did not reach redraw");
    }
}
