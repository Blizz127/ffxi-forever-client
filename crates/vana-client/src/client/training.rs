//! Forever's opt-in Field Manual/Grounds tome view. Progress is authoritative FVT1
//! data, separate from the generic client's locally inferred regime helper.
use super::World;
use super::net::NetCommand;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Goal {
    #[serde(default)]
    pub n: String,
    pub k: u16,
    pub q: u16,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Page {
    pub t: String,
    pub z: u16,
    #[serde(default)]
    pub zn: String,
    pub p: u16,
    pub r: u16,
    pub lv: [u16; 2],
    pub rep: u8,
    pub done: u8,
    pub g: Vec<Goal>,
}

/// No page is different from waiting for the first server reply.
fn parse(text: &str) -> Result<Option<Page>, ()> {
    if text.len() > 150 {
        return Err(());
    }
    let body = text.strip_prefix("FVT1 ").ok_or(())?;
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| ())?;
    match v.get("a").and_then(|a| a.as_u64()) {
        Some(0) => Ok(None),
        Some(1) => {
            let p: Page = serde_json::from_value(v).map_err(|_| ())?;
            if !matches!(p.t.as_str(), "fov" | "gov")
                || p.z >= 300
                || p.r == 0
                || p.lv[0] > p.lv[1]
                || p.rep > 1
                || p.done > 1
                || p.g.is_empty()
                || p.g.len() > 4
                || p.g.iter().any(|g| g.q == 0 || g.k > g.q || g.n.chars().any(char::is_control))
                || p.zn.chars().any(char::is_control)
            {
                return Err(());
            }
            Ok(Some(p))
        }
        _ => Err(()),
    }
}

#[derive(Default)]
pub struct Training {
    pub menu: bool,
    pub hud: bool,
    pub dirty: bool,
    pub page: Option<Page>,
    pub received: bool,
    feed: Option<bool>,
    pub completed: Option<(u16, u16, Instant)>,
}

impl Training {
    pub fn new(hud: bool) -> Self {
        Self { hud, ..Default::default() }
    }
    pub fn reset_zone(&mut self) {
        self.menu = false;
        self.page = None;
        self.received = false;
        self.completed = None;
        self.feed = None;
    }
    fn transition(&mut self, desired: bool) -> Option<bool> {
        if self.feed == Some(desired) {
            return None;
        }
        self.feed = Some(desired);
        Some(desired)
    }
}

pub fn active(w: &World) -> bool {
    w.forever && w.modern && w.training.menu
}

pub fn open(w: &mut World) {
    if !w.forever || !w.modern {
        return;
    }
    w.training.menu = true;
    w.menu = None;
    // Turning on requests a snapshot itself; an already enabled HUD needs an explicit refresh.
    if w.training.feed == Some(true) {
        let _ = w.net.commands.send(NetCommand::Chat { kind: 0, text: "!fovtrack feed".into() });
    }
}

pub fn key(w: &mut World, key: egui::Key) {
    match key {
        egui::Key::Escape => {
            w.training.menu = false;
        }
        egui::Key::Space | egui::Key::Enter => {
            w.training.hud = !w.training.hud;
            w.training.dirty = true;
        }
        _ => {}
    }
}

pub fn tick(w: &mut World) {
    if !w.forever || w.zone.is_none() || w.zoning_since.is_some() || w.loading.is_some() || w.downloading.is_some() {
        return;
    }
    let desired = w.modern && (w.training.menu || w.training.hud);
    if let Some(on) = w.training.transition(desired) {
        // This is an internal protocol request: no local chat echo, history entry,
        // typed-command Lua filter or change to the player's chosen chat channel.
        let _ = w.net.commands.send(NetCommand::Chat {
            kind: 0,
            text: if on { "!fovtrack feed on" } else { "!fovtrack feed off" }.into(),
        });
    }
    if w.training.completed.is_some_and(|(_, _, at)| at.elapsed() > Duration::from_secs(8)) {
        w.training.completed = None;
    }
}

/// LSB and the Rust map stamp printToPlayer's empty sender with the recipient's
/// character name. Accept that wire form as well as an actually empty sender,
/// but never consume player chat or another character's named system line.
pub(super) fn system_feed_source(forever: bool, own: &str, kind: u8, from: &str) -> bool {
    forever && kind == 6 && (from.is_empty() || (!own.is_empty() && from.eq_ignore_ascii_case(own)))
}

/// Consume only server system feed lines. Unknown versions and malformed feed lines
/// never become chat or Lua events; regular servers and player chat pass through.
pub fn receive(w: &mut World, kind: u8, from: &str, text: &str) -> bool {
    if !system_feed_source(w.forever, &w.name, kind, from) {
        return false;
    }
    let Some(version) = text.split_once(' ').map(|x| x.0).and_then(|x| x.strip_prefix("FVT")) else { return false };
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if version != "1" || !w.modern {
        return true;
    }
    match parse(text) {
        Ok(page) => {
            if let Some(p) = page.as_ref().filter(|p| p.done == 1) {
                w.training.completed = Some((p.z, p.p, Instant::now()));
            }
            w.training.page = page;
            w.training.received = true;
        }
        Err(()) => log::debug!("training: ignored malformed FVT1 snapshot"),
    }
    true
}

pub fn zone_name(w: &World, id: u16) -> String {
    w.names
        .as_ref()
        .and_then(|n| crate::dat::names::NameSource::place(&**n, id as u32))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Zone {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_feed_accepts_recipient_stamped_packets_without_swallowing_player_chat() {
        // The live .24 QA run received HPM1 with the recipient in sName, as
        // GP_SERV_COMMAND_CHAT_STD(PChar, SYSTEM_1, message, "") specifies.
        for from in ["", "Zzforeverqa", "zzforeverqa"] {
            assert!(system_feed_source(true, "Zzforeverqa", 6, from));
            assert!(!system_feed_source(false, "Zzforeverqa", 6, from));
            for kind in [0, 1, 3, 4, 5, 8, 0x1a] {
                assert!(!system_feed_source(true, "Zzforeverqa", kind, from));
            }
        }
        assert!(!system_feed_source(true, "Zzforeverqa", 6, "Someoneelse"));
        assert!(!system_feed_source(true, "", 6, "Zzforeverqa"));
        assert!(system_feed_source(true, "", 6, ""));
    }
    const ACTIVE: &str = "FVT1 {\"a\":1,\"t\":\"fov\",\"z\":115,\"zn\":\"West Sarutabaruta\",\"p\":1,\"r\":26,\"lv\":[1,5],\"rep\":1,\"done\":0,\"g\":[{\"n\":\"Bakru+2\",\"k\":1,\"q\":6}]}";
    #[test]
    fn hud_defaults_off_and_round_trips_only_preference() {
        let mut config = crate::config::Config::default();
        assert!(!config.training_hud);
        config.training_hud = true;
        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["training_hud"], true);
        let restored: crate::config::Config = serde_json::from_value(value).unwrap();
        assert!(restored.training_hud);
    }
    #[test]
    fn feed_active_clear_and_shortened_names() {
        let p = parse(ACTIVE).unwrap().unwrap();
        assert_eq!((p.z, p.p, p.g[0].k, p.g[0].q), (115, 1, 1, 6));
        assert_eq!(p.g[0].n, "Bakru+2");
        assert!(parse("FVT1 {\"a\":0}").unwrap().is_none());
        let shortened = ACTIVE.replace("\"West Sarutabaruta\"", "\"\"").replace("\"n\":\"Bakru+2\",", "");
        let p = parse(&shortened).unwrap().unwrap();
        assert!(p.zn.is_empty() && p.g[0].n.is_empty());
        assert!(
            parse(&ACTIVE.replace("\"done\":0", "\"done\":1").replace("\"k\":1", "\"k\":6")).unwrap().unwrap().done
                == 1
        );
    }
    #[test]
    fn malformed_and_unknown_version_do_not_replace_progress() {
        for bad in ["FVT2 {}", "FVT1 {", "FVT1 {\"a\":1}", "FVT1 {\"a\":2}"] {
            assert!(parse(bad).is_err());
        }
        assert!(parse(&ACTIVE.replace("\"q\":6", "\"q\":0")).is_err());
        assert!(parse(&format!("{ACTIVE}{}", " ".repeat(151))).is_err());
    }
    #[test]
    fn feed_reconciles_once_and_again_after_zone_change() {
        let mut t = Training::new(false);
        assert_eq!(t.transition(false), Some(false));
        assert_eq!(t.transition(false), None);
        t.menu = true;
        assert_eq!(t.transition(t.menu || t.hud), Some(true));
        t.hud = true;
        t.menu = false;
        assert_eq!(t.transition(t.menu || t.hud), None);
        t.hud = false;
        assert_eq!(t.transition(t.menu || t.hud), Some(false));
        t.reset_zone();
        assert_eq!(t.transition(false), Some(false));
    }
}
