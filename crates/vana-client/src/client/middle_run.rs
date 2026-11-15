//! Forever middle-click autorun. No binding/config writes, capture, targeting, or packets.
use glam::Vec3;

#[derive(Clone, Copy)]
pub(super) struct Gate {
    pub forever: bool,
    pub fresh: bool,
    pub focused: bool,
    pub pointer_free: bool,
    pub ui_free: bool,
    pub ready: bool,
    pub other_movement: bool,
    pub cancelled_navigation: bool,
}

pub(super) fn toggle(g: Gate, running: bool, forward: Vec3) -> Option<(bool, Vec3)> {
    if !g.forever || !g.fresh || !g.focused || !g.pointer_free || !g.ui_free || g.cancelled_navigation {
        return None;
    }
    if running {
        return Some((false, Vec3::ZERO));
    }
    if !g.ready || g.other_movement {
        return None;
    }
    if !forward.is_finite() {
        return None;
    }
    let direction = Vec3::new(forward.x, 0.0, forward.z).try_normalize()?;
    Some((true, direction))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::controls::Pointer;
    fn gate() -> Gate {
        Gate {
            forever: true,
            fresh: true,
            focused: true,
            pointer_free: true,
            ui_free: true,
            ready: true,
            other_movement: false,
            cancelled_navigation: false,
        }
    }
    #[test]
    fn click_hold_release_click_toggles_once_and_does_not_request_capture() {
        let mut pointer = Pointer::default();
        let mut running = false;
        let mut direction = Vec3::ZERO;
        for (pressed, want) in [(true, true), (true, true), (false, true), (false, true), (true, false), (false, false)]
        {
            let g = Gate { fresh: pointer.middle_button(pressed, true), ..gate() };
            if let Some(state) = toggle(g, running, Vec3::new(3.0, -7.0, 4.0)) {
                (running, direction) = state;
            }
            assert_eq!(running, want);
            assert_eq!(direction, if want { Vec3::new(0.6, 0.0, 0.8) } else { Vec3::ZERO });
            assert!(!pointer.look_requested(Some((50.0, 50.0)), true));
            assert_eq!(pointer.any_down(), pressed);
            assert!(!pointer.left && !pointer.right && pointer.press_at.is_none() && pointer.right_press.is_none());
        }
    }
    #[test]
    fn blocked_contexts_never_start_stop_or_defer_a_toggle() {
        let good = gate();
        for g in [
            Gate { forever: false, ..good },
            Gate { focused: false, ..good },
            Gate { fresh: false, ..good },
            Gate { pointer_free: false, ..good },
            Gate { ui_free: false, ..good },
            Gate { cancelled_navigation: true, ..good },
        ] {
            assert!(toggle(g, false, Vec3::Z).is_none());
            assert!(toggle(g, true, Vec3::Z).is_none());
        }
        for g in [Gate { ready: false, ..good }, Gate { other_movement: true, ..good }] {
            assert!(toggle(g, false, Vec3::Z).is_none());
            assert_eq!(
                toggle(g, true, Vec3::Z),
                Some((false, Vec3::ZERO)),
                "a valid stop press remains safe when starting is not"
            );
        }
        let mut p = Pointer::default();
        assert!(toggle(Gate { fresh: p.middle_button(true, true), ui_free: false, ..good }, false, Vec3::Z).is_none());
        // Closing a menu / regaining focus while held cannot turn the rejected press into movement.
        assert!(toggle(Gate { fresh: p.middle_button(true, true), ..good }, false, Vec3::Z).is_none());
        assert!(toggle(Gate { fresh: p.middle_button(false, true), ..good }, false, Vec3::Z).is_none());
        assert_eq!(toggle(Gate { fresh: p.middle_button(true, true), ..good }, false, Vec3::Z), Some((true, Vec3::Z)));
    }
    #[test]
    fn navigation_cancel_click_is_stop_only_and_direction_is_safe() {
        let mut p = Pointer::default();
        let mut running = false;
        // Native window handling already cancelled AutoNav before forwarding this same press.
        assert!(
            toggle(Gate { fresh: p.middle_button(true, true), cancelled_navigation: true, ..gate() }, running, Vec3::Z)
                .is_none()
        );
        assert!(toggle(Gate { fresh: p.middle_button(false, true), ..gate() }, running, Vec3::Z).is_none());
        if let Some((on, _)) = toggle(Gate { fresh: p.middle_button(true, true), ..gate() }, running, Vec3::Z) {
            running = on;
        }
        assert!(running);
        for forward in [Vec3::ZERO, Vec3::Y, Vec3::splat(f32::NAN), Vec3::splat(f32::INFINITY)] {
            assert!(toggle(gate(), false, forward).is_none());
            assert_eq!(toggle(gate(), true, forward), Some((false, Vec3::ZERO)));
        }
    }
    #[test]
    fn reservation_does_not_change_classic_or_right_look_and_focus_clears_it() {
        let mut p = Pointer::default();
        assert!(p.middle_button(true, false));
        assert!(p.look_requested(None, true), "regular client retains existing middle camera behavior");
        p.clear();
        p.middle_button(true, true);
        assert!(!p.look_requested(None, true));
        p.right = true;
        assert!(p.look_requested(None, true), "right camera input remains independent");
        p.middle_button(false, true);
        assert!(p.look_requested(None, true));
        let mut looking = true;
        crate::client::controls::lose_focus(&mut p, &mut looking);
        assert!(!looking && !p.any_down() && !p.middle_reserved);
        assert!(!p.middle_button(false, true), "release after focus loss must never toggle");
    }
}
