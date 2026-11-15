//! Temporary HUD collision avoidance. Never writes a saved placement or shrinks party rows.
use egui::{Rect, pos2, vec2};

pub(super) fn place(wanted: Rect, screen: Rect, obstacles: &[Rect]) -> Rect {
    let bounds = Rect::from_min_max(
        pos2(screen.left() + 4.0, screen.top() + super::theme::BELOW_BAR),
        screen.right_bottom() - vec2(4.0, 4.0),
    );
    if bounds.width() < 1.0 || bounds.height() < 1.0 {
        return Rect::from_min_size(screen.min, vec2(1.0, 1.0));
    }
    let size = vec2(wanted.width().min(bounds.width()).max(1.0), wanted.height().min(bounds.height()).max(1.0));
    let origin = pos2(
        wanted.left().clamp(bounds.left(), bounds.right() - size.x),
        wanted.top().clamp(bounds.top(), bounds.bottom() - size.y),
    );
    let desired = Rect::from_min_size(origin, size);
    let blocks: Vec<_> =
        obstacles.iter().filter(|r| r.is_positive()).map(|r| r.expand(super::theme::PANEL_GAP)).collect();
    if blocks.iter().all(|r| !r.intersects(desired)) {
        return desired;
    }
    let mut xs = vec![origin.x, bounds.left(), bounds.right() - size.x];
    for b in &blocks {
        xs.extend([b.left() - size.x, b.right()]);
    }
    let mut best: Option<((f32, u8, f32), Rect)> = None;
    for x in xs {
        if x < bounds.left() || x + size.x > bounds.right() {
            continue;
        }
        let mut spans: Vec<_> = blocks
            .iter()
            .filter(|b| b.right() > x && b.left() < x + size.x)
            .map(|b| (b.top().max(bounds.top()), b.bottom().min(bounds.bottom())))
            .filter(|(a, b)| b > a)
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        spans.push((bounds.bottom(), bounds.bottom()));
        let mut low = bounds.top();
        for (top, bottom) in spans {
            let h = (top - low).min(size.y);
            if h >= 96.0_f32.min(size.y) {
                let y = origin.y.clamp(low, (top - h).max(low));
                let rect = Rect::from_min_size(pos2(x, y), vec2(size.x, h));
                // Preserve size first; within that, keep the column and move upward
                // when possible. A crowded column can use a neighbouring clear one.
                let score = (size.y - h, if (x - origin.x).abs() < 0.5 { 0 } else { 1 }, rect.min.distance_sq(origin));
                if best.as_ref().is_none_or(|(old, _)| score < *old) {
                    best = Some((score, rect));
                }
            }
            low = low.max(bottom);
        }
    }
    best.map_or(desired, |(_, r)| r)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crowded_screen_uses_scroll_height_and_recovers() {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0));
        let wanted = Rect::from_min_size(pos2(436.0, 100.0), vec2(360.0, 400.0));
        let top = Rect::from_min_max(pos2(0.0, 40.0), pos2(800.0, 220.0));
        let bottom = Rect::from_min_max(pos2(0.0, 430.0), pos2(800.0, 600.0));
        let r = place(wanted, screen, &[top, bottom]);
        assert!(r.height() < wanted.height() && r.height() >= 96.0);
        assert!(!r.intersects(top) && !r.intersects(bottom));
        assert_eq!(place(wanted, screen, &[]), wanted);
    }
    #[test]
    fn grows_party_upward_without_overlapping_or_mutating_requested_rect() {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0));
        let wanted = Rect::from_min_size(pos2(916.0, 420.0), vec2(360.0, 250.0));
        for n in 1..=6 {
            let party = Rect::from_min_max(pos2(936.0, 790.0 - (90.0 + n as f32 * 60.0)), pos2(1276.0, 790.0));
            let r = place(wanted, screen, &[party]);
            assert!(!r.intersects(party));
            assert!(screen.contains_rect(r));
            assert_eq!(r.size(), wanted.size());
        }
        let party = Rect::from_min_max(pos2(936.0, 560.0), pos2(1276.0, 790.0));
        assert_eq!(place(wanted, screen, &[party]).left(), wanted.left());
        let minimap = Rect::from_min_max(pos2(980.0, 50.0), pos2(1276.0, 380.0));
        let r = place(wanted, screen, &[party, minimap]);
        assert!(r.left() < wanted.left());
        assert!(!r.intersects(party));
        assert!(!r.intersects(minimap));
        let custom = Rect::from_min_size(pos2(80.0, 100.0), wanted.size());
        assert_eq!(place(custom, screen, &[party, minimap]), custom);
    }
}
