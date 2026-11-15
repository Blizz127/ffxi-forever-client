//! The EXP bar's data (ours, not retail; the windows are modern/expbar.rs): the level and experience the server sent,
//! and the limit and merit points where the era has them. Nothing is estimated: the values are the Status window's own
//! (s2c 0x061: EXP u16 @0x10, EXP to the next level u16 @0x12, master level @0x65, its flags @0x66, exemplar points
//! u32 @0x68 / next @0x6c; s2c 0x063 type 2: limit points u16 @8, bits u16 @0xa with the merits in bits 0..6 and bit 14
//! EXP capped, most merits @0xc). On FFXI Forever the bar shows in the modern Equipment window and in the party
//! window (config exp_bar).

use super::World;
use super::era::{self, Feature};
use super::items::CharStats;

/// The limit and merit points (the era has them, and the player is in merit mode or has some).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Points {
    pub limit: u16,
    pub merits: u8,
    pub max_merits: u8,
}

/// What the bar shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// "Lv" (the job level), or "M. Lv" with a master level unlocked.
    pub head: &'static str,
    pub level: u8,
    pub now: i64,
    pub next: i64,
    /// EXP capped (limit points are being earned): the bar is full.
    pub capped: bool,
    pub points: Option<Points>,
}

impl View {
    /// How full the bar is, 0..=1.
    pub fn fraction(&self) -> f32 {
        if self.next > 0 { (self.now as f32 / self.next as f32).clamp(0.0, 1.0) } else { 0.0 }
    }

    /// The percent as players read it: rounded down, so 100 only at a full bar.
    pub fn percent(&self) -> u32 {
        if self.next > 0 { (self.now.max(0) * 100 / self.next).min(100) as u32 } else { 0 }
    }
}

/// The bar's view from what the server sent. `merits` is the 0x063 type 2 triple (limit, bits, max). The era lock
/// (`no_merits`, `no_master`) drops what the base era (level cap 60) never had; `level` is the level the character
/// plays at (a synced level, else the main job's).
pub fn build(
    stats: &CharStats,
    level: Option<u8>,
    merits: Option<(u16, u16, u8)>,
    no_merits: bool,
    no_master: bool,
) -> Option<View> {
    if stats.main.1 == 0 && level.is_none_or(|l| l == 0) {
        return None;
    }
    let master = stats.ml.1 & 1 != 0 && !no_master;
    let capped = if master {
        stats.ml.1 & 2 != 0 && stats.ml_exp.0 >= stats.ml_exp.1.wrapping_sub(1)
    } else {
        merits.is_some_and(|m| m.1 & 0x4000 != 0)
    };
    let points = merits.filter(|_| !no_merits).and_then(|(limit, bits, max)| {
        let merits = (bits & 0x7f) as u8;
        // Quiet before the cap: no limit points yet, no merits, not in merit mode.
        (capped || limit > 0 || merits > 0).then_some(Points { limit, merits, max_merits: max })
    });
    Some(if master {
        View {
            head: "M. Lv",
            level: stats.ml.0,
            now: stats.ml_exp.0 as i64,
            next: stats.ml_exp.1 as i64,
            capped,
            points,
        }
    } else {
        View {
            head: "Lv",
            level: level.unwrap_or(stats.main.1),
            now: stats.exp.0 as i64,
            next: stats.exp.1 as i64,
            capped,
            points,
        }
    })
}

/// The bar for the running game: nothing before the character's stats arrive; under the era lock no limit / merit
/// points (the base era has no merits) and no master level.
pub fn view(w: &World) -> Option<View> {
    let stats = w.stats.as_ref()?;
    build(stats, w.real_level, w.merits, era::hidden(w, Feature::MeritPoints), era::hidden(w, Feature::MasterLevels))
}

/// The always-on bar in the party window: the modern UI on FFXI Forever with the setting on. The modern Equipment
/// window also requires Forever, but is independent of the setting.
pub fn on_main_screen(w: &World) -> bool {
    w.modern && w.forever && w.exp_bar
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(level: u8, exp: (u16, u16)) -> CharStats {
        CharStats { main: (1, level), exp, ..Default::default() }
    }

    #[test]
    fn the_servers_values_are_shown_as_sent() {
        let v = build(&stats(24, (1234, 5000)), None, None, false, false).unwrap();
        assert_eq!((v.head, v.level, v.now, v.next, v.capped, v.points), ("Lv", 24, 1234, 5000, false, None));
        assert_eq!(v.percent(), 24);
        assert!((v.fraction() - 0.2468).abs() < 1e-4);
        // A synced level shows as the level.
        assert_eq!(build(&stats(75, (0, 1)), Some(40), None, false, false).unwrap().level, 40);
    }

    #[test]
    fn the_percent_is_full_only_at_a_full_bar() {
        let mut v = build(&stats(10, (4999, 5000)), None, None, false, false).unwrap();
        assert_eq!(v.percent(), 99);
        v.now = 5000;
        assert_eq!(v.percent(), 100);
        // No next level (u16 0): an empty bar, no division.
        let v = build(&stats(99, (0, 0)), None, None, false, false).unwrap();
        assert_eq!((v.percent(), v.fraction()), (0, 0.0));
    }

    #[test]
    fn nothing_before_the_stats_hold_a_level() {
        assert_eq!(build(&CharStats::default(), None, None, false, false), None);
    }

    #[test]
    fn limit_and_merit_points_show_in_merit_mode_or_once_earned() {
        let s = stats(75, (55_999, 56_000));
        // Not yet earning: nothing about them.
        assert_eq!(build(&s, None, Some((0, 0, 10)), false, false).unwrap().points, None);
        // Merit mode on (bit 14), 3 merits of 10, 4,250 limit points.
        let v = build(&s, None, Some((4250, 0x4000 | 3, 10)), false, false).unwrap();
        assert!(v.capped);
        assert_eq!(v.points, Some(Points { limit: 4250, merits: 3, max_merits: 10 }));
        // Merits kept after leaving merit mode still show.
        assert_eq!(
            build(&s, None, Some((0, 2, 10)), false, false).unwrap().points,
            Some(Points { limit: 0, merits: 2, max_merits: 10 })
        );
    }

    #[test]
    fn the_era_lock_drops_merits_and_master_levels() {
        let mut s = stats(60, (100, 200));
        s.ml = (3, 1);
        s.ml_exp = (10, 100);
        let merits = Some((9000, 0x4000 | 5, 10));
        let v = build(&s, None, merits, true, true).unwrap();
        assert_eq!((v.head, v.level, v.now, v.next, v.points), ("Lv", 60, 100, 200, None));
        // Without the lock the master level leads.
        let v = build(&s, None, merits, false, false).unwrap();
        assert_eq!((v.head, v.level, v.now, v.next), ("M. Lv", 3, 10, 100));
    }

    #[test]
    fn the_setting_defaults_on_for_old_configs() {
        assert!(serde_json::from_str::<crate::config::Config>("{}").unwrap().exp_bar);
        assert!(serde_json::from_str::<crate::config::Config>("{\"exp_bar\":false}").map(|c| !c.exp_bar).unwrap());
    }
}
