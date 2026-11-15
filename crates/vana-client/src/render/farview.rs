//! Far View (Forever Enhanced): the Clipping setting taken past 100%. Retail's Clipping factor X scales each environment's fog
//! and the scenery's draw distance together (dat/weather.rs `clipping`: D = X * raw, fog end = X * raw end, fog start 0);
//! Far View multiplies X by 1.5 or 2 in open-air zones so the horizon, the fog that hides it and the distance things stop being
//! drawn all move out as one, exactly as retail's own control would if it went past full. Interiors, fixed-light environments and
//! tight-fog zones (a raw draw distance under 200: dungeons, towers) keep their own distance.

/// Far View's levels as Settings shows them: the multiplier on X.
pub const LEVELS: [(u32, f32); 3] = [(0, 1.0), (1, 1.5), (2, 2.0)];

/// The multiplier for a saved level (an unknown level is the nearest known one).
pub fn factor(level: u32) -> f32 {
    LEVELS[(level as usize).min(LEVELS.len() - 1)].1
}

/// Whether an environment takes Far View: an open-air one (not the flags-bit-0 fixed light of interiors) whose retail draw
/// distance `raw` (@0x58) is a field's, not a building's or a tower's.
pub fn eligible(raw_draw_distance: f32, fixed: bool) -> bool {
    !fixed && raw_draw_distance >= 200.0
}

/// The Clipping factor to use: `x` times `factor` where the environment is eligible, else `x` unchanged (also exactly `x` at
/// factor 1, so the Retail level and Original change nothing).
pub fn x(x: f32, factor: f32, raw_draw_distance: f32, fixed: bool) -> f32 {
    if factor > 1.0 && eligible(raw_draw_distance, fixed) { x * factor } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retail_is_the_identity_and_only_open_air_is_extended() {
        assert_eq!((factor(0), factor(1), factor(2), factor(9)), (1.0, 1.5, 2.0, 2.0));
        for raw in [43.0, 99.0, 302.0, 401.0] {
            assert_eq!(x(0.84, 1.0, raw, false), 0.84);
        }
        assert_eq!(x(1.0, 1.5, 302.0, false), 1.5);
        assert_eq!(x(0.5, 2.0, 252.5, false), 1.0);
        // Interiors, fixed light and tight-fog zones keep retail's.
        assert_eq!(x(1.0, 2.0, 302.0, true), 1.0);
        assert_eq!(x(1.0, 2.0, 99.0, false), 1.0);
        // Through clipping(): D and K scale together, the fog end with K.
        let (d, k) = crate::dat::weather::clipping(302.0, x(1.0, 2.0, 302.0, false), 241);
        assert_eq!((d, k), (604.0, 2.0));
    }
}
