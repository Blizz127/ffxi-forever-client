//! Forever's current-zone Home Point feed. Whole snapshots replace atomically;
//! multipart lines have no generation id, so only consecutive ordered parts join.
use super::World;
use super::net::NetCommand;
use glam::Vec3;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crystal {
    pub pos: Vec3,
    pub registered: bool,
}

#[derive(Deserialize)]
struct Wire {
    z: u16,
    h: Vec<(f32, f32, f32, u8)>,
    i: Option<u8>,
    n: Option<u8>,
}

struct Partial {
    zone: u16,
    next: u8,
    total: u8,
    points: Vec<Crystal>,
    at: Instant,
}
#[derive(Default)]
pub struct HpMap {
    zone: Option<u16>,
    pub points: Vec<Crystal>,
    partial: Option<Partial>,
    feed: Option<bool>,
}

impl HpMap {
    pub fn reset_zone(&mut self) {
        *self = Self::default();
    }
    fn receive(&mut self, text: &str, current_zone: u16, now: Instant) -> Result<(), ()> {
        if text.len() > 150 {
            return Err(());
        }
        let wire: Wire = serde_json::from_str(text.strip_prefix("HPM1 ").ok_or(())?).map_err(|_| ())?;
        if wire.z != current_zone
            || wire.h.len() > 64
            || wire.h.iter().any(|&(x, y, z, r)| {
                !x.is_finite()
                    || !y.is_finite()
                    || !z.is_finite()
                    || x.abs() > 100000.0
                    || y.abs() > 100000.0
                    || z.abs() > 100000.0
                    || r > 1
            })
        {
            return Err(());
        }
        let points: Vec<_> =
            wire.h.into_iter().map(|(x, y, z, r)| Crystal { pos: Vec3::new(x, y, z), registered: r == 1 }).collect();
        match (wire.i, wire.n) {
            (None, None) => {
                self.partial = None;
                self.zone = Some(wire.z);
                self.points = points;
            }
            (Some(i), Some(n)) if n > 0 && n <= 16 && i > 0 && i <= n => {
                if i == 1 {
                    self.partial = Some(Partial { zone: wire.z, next: 1, total: n, points: Vec::new(), at: now });
                }
                let p = self.partial.as_mut().ok_or(())?;
                if p.zone != wire.z
                    || p.total != n
                    || p.next != i
                    || now.duration_since(p.at) > Duration::from_secs(5)
                    || p.points.len() + points.len() > 64
                {
                    self.partial = None;
                    return Err(());
                }
                p.points.extend(points);
                p.next += 1;
                if i == n {
                    let done = self.partial.take().unwrap();
                    self.zone = Some(done.zone);
                    self.points = done.points;
                }
            }
            _ => {
                self.partial = None;
                return Err(());
            }
        }
        Ok(())
    }
}

pub fn receive(w: &mut World, kind: u8, from: &str, text: &str) -> bool {
    if !super::training::system_feed_source(w.forever, &w.name, kind, from) {
        return false;
    }
    let Some(version) = text.split_once(' ').map(|x| x.0).and_then(|x| x.strip_prefix("HPM")) else { return false };
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if version == "1"
        && let Some(zone) = w.zone
    {
        if w.hpmap.receive(text, zone, Instant::now()).is_err() {
            log::debug!("hpmap: ignored incomplete or malformed HPM1 snapshot");
        }
    }
    true
}

pub fn tick(w: &mut World) {
    if !w.forever || w.zone.is_none() || w.zoning_since.is_some() || w.loading.is_some() || w.downloading.is_some() {
        return;
    }
    let desired = w.map.is_some() || (w.modern && w.extra_minimap && !w.ui_hidden);
    if w.hpmap.feed != Some(desired) {
        w.hpmap.feed = Some(desired);
        let _ = w
            .net
            .commands
            .send(NetCommand::Chat { kind: 0, text: if desired { "!hpmap on" } else { "!hpmap off" }.into() });
    }
    if w.hpmap.partial.as_ref().is_some_and(|p| p.at.elapsed() > Duration::from_secs(5)) {
        w.hpmap.partial = None;
    }
}

/// Use the installed teleport table for floor identity when a server crystal matches
/// it; otherwise the loaded current zone's collision map provides the submap key.
pub fn on_map(w: &World, region: &super::map::Region) -> Vec<Crystal> {
    if !w.forever || w.hpmap.zone != Some(region.zone as u16) || w.zone != Some(region.zone as u16) {
        return Vec::new();
    }
    let table = w.item_info.as_ref().map(|c| c.points());
    w.hpmap
        .points
        .iter()
        .copied()
        .filter(|c| {
            let key = table
                .as_ref()
                .and_then(|t| {
                    t.iter()
                        .find(|p| {
                            p.kind == 0
                                && p.zone == region.zone as u16
                                && Vec3::new(p.x, p.y, p.z).distance_squared(c.pos) < 1.0
                        })
                        .map(|p| p.map)
                })
                .or_else(|| w.ground.as_ref().filter(|_| w.pos_zone == region.zone as u16).map(|g| g.map_key(c.pos)));
            key == Some(region.key as u8)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn single_snapshot_registration_and_empty_zone() {
        let mut m = HpMap::default();
        let now = Instant::now();
        m.receive("HPM1 {\"z\":230,\"h\":[[-85.5,1.0,-66.5,1],[-165,0,11,0]]}", 230, now).unwrap();
        assert_eq!(m.points.len(), 2);
        assert!(m.points[0].registered);
        assert!(!m.points[1].registered);
        m.receive("HPM1 {\"z\":230,\"h\":[]}", 230, now).unwrap();
        assert!(m.points.is_empty());
    }
    #[test]
    fn multipart_replaces_only_when_complete_and_never_mixes_zones() {
        let mut m = HpMap::default();
        let now = Instant::now();
        m.receive("HPM1 {\"z\":230,\"h\":[[1,2,3,1]]}", 230, now).unwrap();
        let first = "HPM1 {\"z\":230,\"i\":1,\"n\":2,\"h\":[[4,5,6,0]]}";
        let last = "HPM1 {\"z\":230,\"i\":2,\"n\":2,\"h\":[[7,8,9,1]]}";
        m.receive(first, 230, now).unwrap();
        assert_eq!(m.points[0].pos.x, 1.0);
        assert!(m.receive(last, 231, now).is_err());
        assert_eq!(m.points.len(), 1);
        m.receive(last, 230, now).unwrap();
        assert_eq!(m.points.len(), 2);
        assert_eq!(m.points[0].pos.x, 4.0);
        m.reset_zone();
        assert!(m.receive(last, 230, now).is_err());
        assert!(m.points.is_empty());
    }
    #[test]
    fn malformed_inconsistent_and_expired_parts_do_not_publish() {
        let mut m = HpMap::default();
        let now = Instant::now();
        let first = "HPM1 {\"z\":230,\"i\":1,\"n\":2,\"h\":[[1,2,3,1]]}";
        let last = "HPM1 {\"z\":230,\"i\":2,\"n\":2,\"h\":[]}";
        m.receive(first, 230, now).unwrap();
        assert!(m.receive(last, 230, now + Duration::from_secs(6)).is_err());
        for bad in [
            "HPM2 {}",
            "HPM1 {\"z\":230,\"i\":0,\"n\":2,\"h\":[]}",
            "HPM1 {\"z\":230,\"h\":[[1,2,3,2]]}",
            "HPM1 {\"z\":230,\"h\":[[1,2,3]]}",
        ] {
            assert!(m.receive(bad, 230, now).is_err());
        }
        assert!(m.points.is_empty());
    }
}
