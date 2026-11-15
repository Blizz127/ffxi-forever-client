//! Forever character UI overlays. No network/auth/GM state; identity comes from
//! the authenticated lobby and is confirmed by the first matching map login.
use crate::{
    client::mapview::Prefs,
    config::{Config, ModernLayout},
};
use anyhow::{Context as _, Result, ensure};
use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Identity {
    realm: String,
    account: u32,
    character: u32,
}
impl Identity {
    fn key(&self) -> String {
        format!(
            "character-ui-v1-{}",
            hex::encode(digest(&SHA256, &serde_json::to_vec(self).expect("identity JSON")).as_ref())
        )
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Preferences {
    map: Prefs,
    tracker: bool,
    markers: bool,
    navigation: bool,
    training: bool,
    folded: BTreeMap<String, bool>,
    layout: ModernLayout,
    recent: Vec<String>,
    /// The modern UI's favourites strip (client/favs.rs) and this character's layouts per display (modern_places keys
    /// "<key> @<size> <scale>%", stored by their suffix). Profiles from before these existed read them empty.
    #[serde(default)]
    favourites: Vec<String>,
    #[serde(default)]
    displays: BTreeMap<String, ModernLayout>,
    /// The Forever Enhancements move (config forever_enhancements_on) had run for the config this was captured from. A
    /// profile without the key was saved before it, so its tracker, Training tracker and Auto Navigation preference (all off
    /// by the old default) move on once at confirm.
    #[serde(default)]
    enhancements: bool,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}
/// Where a key's per-display layouts start in the config's `modern_places` (modern/mod.rs `display_key`).
const DISPLAY_AT: &str = " @";
fn displays(c: &Config, key: &str) -> BTreeMap<String, ModernLayout> {
    c.modern_places
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix(key).filter(|r| r.starts_with(DISPLAY_AT)).map(|r| (r.to_owned(), v.clone()))
        })
        .collect()
}
fn remove_displays(c: &mut Config, key: &str) {
    c.modern_places.retain(|k, _| !k.strip_prefix(key).is_some_and(|r| r.starts_with(DISPLAY_AT)));
}
impl Preferences {
    fn capture(c: &Config, key: &str) -> Self {
        Self {
            map: c.forever_map.clone(),
            tracker: c.quest_tracker,
            markers: c.quest_giver_markers,
            navigation: c.auto_navigation,
            training: c.training_hud,
            folded: c.modern_folded.clone(),
            layout: c.modern_places.get(key).cloned().unwrap_or_default(),
            recent: c.modern_recent.get(key).cloned().unwrap_or_default(),
            favourites: c.modern_favourites.get(key).cloned().unwrap_or_default(),
            displays: displays(c, key),
            enhancements: c.forever_enhancements_on,
            unknown: Default::default(),
        }
    }
    fn apply(&self, c: &mut Config) {
        c.forever_map = self.map.clone();
        c.quest_tracker = self.tracker;
        c.quest_giver_markers = self.markers;
        c.auto_navigation = self.navigation;
        c.training_hud = self.training;
        c.modern_folded = self.folded.clone();
    }
    fn install(&self, c: &mut Config, key: &str) {
        self.apply(c);
        c.modern_places.insert(key.to_owned(), self.layout.clone());
        remove_displays(c, key);
        for (suffix, layout) in &self.displays {
            c.modern_places.insert(format!("{key}{suffix}"), layout.clone());
        }
        c.modern_recent.insert(key.to_owned(), self.recent.clone());
        c.modern_favourites.insert(key.to_owned(), self.favourites.clone());
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Document {
    schema_version: u32,
    identity: Identity,
    revision: u64,
    preferences: Preferences,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone)]
struct Overlay {
    identity: Option<Identity>,
    key: String,
    path: PathBuf,
    baseline: Preferences,
    seed: Preferences,
    confirmed: bool,
    blocked: bool,
    expected: RefCell<Option<Vec<u8>>>,
}
#[derive(Clone, Default)]
pub(crate) struct Scope {
    overlay: Option<Overlay>,
}

impl Scope {
    pub fn active(&self) -> bool {
        self.overlay.is_some()
    }
    /// Starts read-only. Never key by character name, account name or credentials.
    pub fn begin(c: &mut Config, account: Option<u32>, character: u32, name: &str) -> String {
        Self::end(c);
        let ep = c.resolved();
        let realm =
            format!("{}|{}|{}|{}", ep.server.trim().to_ascii_lowercase(), ep.auth_port, ep.data_port, ep.view_port);
        let identity = account.filter(|id| *id != 0).filter(|_| character != 0).map(|account| Identity {
            realm,
            account,
            character,
        });
        let key = identity.as_ref().map(Identity::key).unwrap_or_else(|| "character-ui-unconfirmed".into());
        let baseline = Preferences::capture(c, name);
        let mut seed = baseline.clone();
        // The legacy global Hide has no owner identity. Preserve it in the old
        // config but do not propagate it to every previously unseen character.
        if seed.map.navigation == 0 {
            seed.map.navigation = Prefs::default().navigation;
        }
        seed.navigation = false; // Never inherit another character's movement opt-in.
        seed.install(c, &key);
        let path = crate::config::config_path().parent().unwrap().join("character-ui").join(&key).join("profile.json");
        c.character_ui.overlay = Some(Overlay {
            identity,
            key: key.clone(),
            path,
            baseline,
            seed,
            confirmed: false,
            blocked: false,
            expected: RefCell::new(None),
        });
        key
    }
    /// Confirms authenticated selected identity against 0x00A. Zone re-entry
    /// does not reapply settings or restart navigation.
    pub fn confirm(c: &mut Config, character: u32) -> Result<bool> {
        let Some(mut o) = c.character_ui.overlay.take() else { return Ok(false) };
        let result = (|| {
            ensure!(
                o.identity.as_ref().is_some_and(|i| i.character == character),
                "character UI identity unconfirmed; persistence disabled"
            );
            ensure!(!o.blocked, "character UI profile is protected after an earlier load error");
            if o.confirmed {
                return Ok(false);
            }
            let bytes = read_optional(&o.path)?;
            let mut prefs = if let Some(bytes) = &bytes {
                let d: Document =
                    serde_json::from_slice(bytes).context("character UI profile unreadable; preserving it")?;
                ensure!(
                    d.schema_version == 1 && Some(&d.identity) == o.identity.as_ref(),
                    "character UI version/identity mismatch; preserving it"
                );
                d.preferences
            } else {
                o.seed.clone()
            };
            // A profile saved before the Forever Enhancements move: its tracker, Training tracker and Auto Navigation
            // preference are off only by the old default, so they move on once, as the global config's do (Auto Navigation
            // only enables the helper; nothing moves without Start). The schema stays 1 (an older build still reads the
            // file); the marker is written at the next save of this profile. A saved value after that is kept.
            if !prefs.enhancements && c.is_forever() && !c.classic_only {
                prefs.tracker = true;
                prefs.training = true;
                prefs.navigation = true;
                prefs.enhancements = true;
            }
            prefs.install(c, &o.key);
            *o.expected.borrow_mut() = bytes;
            o.confirmed = true;
            Ok(true)
        })();
        if result.is_err() {
            o.blocked = true;
        }
        c.character_ui.overlay = Some(o);
        result
    }
    pub fn end(c: &mut Config) {
        if let Some(o) = c.character_ui.overlay.take() {
            o.baseline.apply(c);
            c.modern_places.remove(&o.key);
            remove_displays(c, &o.key);
            c.modern_recent.remove(&o.key);
            c.modern_favourites.remove(&o.key);
        }
    }
    /// Save the selected overlay separately, then serialize global preferences
    /// with their untouched legacy baseline. Pending identity cannot write a profile.
    pub fn for_global_save(&self, c: &Config) -> Result<Config> {
        let mut global = c.clone();
        if let Some(o) = &self.overlay {
            if o.confirmed && !o.blocked {
                let prefs = Preferences::capture(c, &o.key);
                persist(o, prefs)?;
            }
            o.baseline.apply(&mut global);
            global.modern_places.remove(&o.key);
            remove_displays(&mut global, &o.key);
            global.modern_recent.remove(&o.key);
            global.modern_favourites.remove(&o.key);
        }
        global.character_ui = Default::default();
        Ok(global)
    }
}
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
struct FileGuard(PathBuf);
impl Drop for FileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn persist(o: &Overlay, mut prefs: Preferences) -> Result<()> {
    let identity = o.identity.as_ref().context("character UI identity missing")?;
    let dir = o.path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let lock = dir.join("write.lock");
    let handle = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .context("character UI writer busy; existing profile preserved")?;
    drop(handle);
    let _lock = FileGuard(lock);
    let mut expected = o.expected.borrow_mut();
    let actual = read_optional(&o.path)?;
    ensure!(actual == *expected, "character UI changed externally; reload before saving, existing profile preserved");
    let mut d = if let Some(bytes) = actual.as_ref() {
        serde_json::from_slice::<Document>(bytes)?
    } else {
        Document {
            schema_version: 1,
            identity: identity.clone(),
            revision: 0,
            preferences: prefs.clone(),
            unknown: Default::default(),
        }
    };
    ensure!(d.schema_version == 1 && d.identity == *identity, "character UI version/identity conflict");
    prefs.unknown = d.preferences.unknown.clone();
    if actual.is_some() && serde_json::to_value(&prefs)? == serde_json::to_value(&d.preferences)? {
        return Ok(());
    }
    d.preferences = prefs;
    d.revision = d.revision.checked_add(1).context("character UI revision exhausted")?;
    let bytes = serde_json::to_vec_pretty(&d)?;
    if let Some(old) = actual {
        replace_file(&dir.join("previous.json"), &old)?;
    }
    replace_file(&o.path, &bytes)?;
    *expected = Some(bytes);
    Ok(())
}
fn replace_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("pending");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .context("unfinished character UI write retained; refusing overwrite")?;
    let _cleanup = FileGuard(temp.clone());
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "vana-character-ui-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn begin(&self, c: &mut Config, account: Option<u32>, character: u32, name: &str) -> String {
            let key = Scope::begin(c, account, character, name);
            c.character_ui.overlay.as_mut().unwrap().path = self.0.join(&key).join("profile.json");
            key
        }
        fn save(&self, c: &Config) -> Config {
            c.character_ui.for_global_save(c).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn gmblizz_blizz_switch_relog_restart_and_rename_keep_distinct_choices() {
        let f = Fixture::new();
        let mut c = Config::default();
        c.forever_map.navigation = 0; // ambiguous pre-migration global Hide
        let gm = f.begin(&mut c, Some(7), 41, "GMBlizz");
        assert!(Scope::confirm(&mut c, 41).unwrap());
        assert_eq!(c.forever_map.navigation, 3);
        c.forever_map.navigation = 0;
        c.quest_tracker = true;
        c.modern_folded.insert("quest".into(), true);
        c.modern_places.entry(gm.clone()).or_default().insert("quest".into(), Default::default());
        // The upstream modern UI's per-display layouts and favourites follow the character's profile too.
        let display = format!("{gm} @2560x1440 60%");
        c.modern_places.entry(display.clone()).or_default().insert("quest".into(), Default::default());
        c.modern_favourites.insert(gm.clone(), vec!["m:1".into()]);
        let global = f.save(&c);
        assert_eq!(global.forever_map.navigation, 0); // legacy preserved, not adopted by siblings
        assert!(!global.modern_places.contains_key(&gm));
        assert!(!global.modern_places.contains_key(&display) && !global.modern_favourites.contains_key(&gm));
        let blizz = f.begin(&mut c, Some(7), 42, "Blizz");
        assert_ne!(gm, blizz);
        Scope::confirm(&mut c, 42).unwrap();
        assert_eq!(c.forever_map.navigation, 3);
        assert!(!c.auto_navigation);
        assert!(!c.modern_folded.contains_key("quest"));
        c.forever_map.navigation = 1;
        let global = f.save(&c);
        // Real serialization round-trip represents launcher/client restart.
        let mut c: Config = serde_json::from_slice(&serde_json::to_vec(&global).unwrap()).unwrap();
        assert_eq!(f.begin(&mut c, Some(7), 41, "RenamedGM"), gm);
        Scope::confirm(&mut c, 41).unwrap();
        assert_eq!(c.forever_map.navigation, 0);
        assert!(c.quest_tracker);
        assert!(c.modern_places[&gm].contains_key("quest"));
        assert!(c.modern_places[&display].contains_key("quest") && c.modern_favourites[&gm] == ["m:1"]);
        Scope::end(&mut c);
        assert!(!c.modern_places.contains_key(&display) && !c.modern_favourites.contains_key(&gm));
        f.begin(&mut c, Some(7), 42, "Blizz");
        Scope::confirm(&mut c, 42).unwrap();
        assert_eq!(c.forever_map.navigation, 1);
    }
    /// The Compact HUD's folds (a panel's own, and the Minimal HUD's key) are the character's, like the older folds: a
    /// sibling starts with none, and each is back after a restart.
    #[test]
    fn compact_hud_folds_and_minimal_hud_are_per_character() {
        let f = Fixture::new();
        let mut c = Config::default();
        f.begin(&mut c, Some(7), 41, "Folder");
        Scope::confirm(&mut c, 41).unwrap();
        for id in ["party", "chat", "quest tracker", "minimal hud"] {
            c.modern_folded.insert(id.into(), true);
        }
        let global = f.save(&c);
        assert!(
            !global.modern_folded.contains_key("minimal hud"),
            "the character's folds stay out of the global config"
        );
        f.begin(&mut c, Some(7), 42, "Sibling");
        Scope::confirm(&mut c, 42).unwrap();
        assert!(c.modern_folded.is_empty(), "a sibling starts unfolded: {:?}", c.modern_folded);
        c.modern_folded.insert("party".into(), true);
        let global = f.save(&c);
        // A restart (serialization round trip), then the first character again.
        let mut c: Config = serde_json::from_slice(&serde_json::to_vec(&global).unwrap()).unwrap();
        f.begin(&mut c, Some(7), 41, "Folder");
        Scope::confirm(&mut c, 41).unwrap();
        for id in ["party", "chat", "quest tracker", "minimal hud"] {
            assert_eq!(c.modern_folded.get(id), Some(&true), "{id}");
        }
        f.begin(&mut c, Some(7), 42, "Sibling");
        Scope::confirm(&mut c, 42).unwrap();
        assert_eq!(c.modern_folded.keys().collect::<Vec<_>>(), vec!["party"]);
    }
    #[test]
    fn realm_account_character_identity_not_display_name() {
        let f = Fixture::new();
        let mut c = Config::default();
        c.server = "realm-a.invalid".into();
        let a = f.begin(&mut c, Some(7), 41, "SameName");
        let b = f.begin(&mut c, Some(8), 41, "SameName");
        assert_ne!(a, b);
        c.server = "realm-b.invalid".into();
        assert_ne!(a, f.begin(&mut c, Some(7), 41, "SameName"));
    }
    #[test]
    fn interruption_unknown_identity_and_mismatched_login_never_write() {
        let f = Fixture::new();
        let mut c = Config::default();
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        c.forever_map.navigation = 0;
        f.save(&c);
        assert_eq!(std::fs::read_dir(&f.0).unwrap().count(), 0);
        Scope::end(&mut c);
        f.begin(&mut c, None, 41, "GMBlizz");
        assert!(Scope::confirm(&mut c, 41).is_err());
        f.save(&c);
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        assert!(Scope::confirm(&mut c, 42).is_err());
        f.save(&c);
        assert_eq!(std::fs::read_dir(&f.0).unwrap().count(), 0);
    }
    #[test]
    fn zoning_does_not_reload_old_values_or_restart_navigation() {
        let f = Fixture::new();
        let mut c = Config::default();
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        Scope::confirm(&mut c, 41).unwrap();
        c.forever_map.navigation = 0;
        assert!(!Scope::confirm(&mut c, 41).unwrap());
        assert_eq!(c.forever_map.navigation, 0);
    }
    #[test]
    fn device_preferences_and_legacy_name_layout_remain_global_and_unchanged() {
        let f = Fixture::new();
        let mut c = Config::default();
        c.modern_places.entry("GMBlizz".into()).or_default().insert("chat".into(), Default::default());
        let before = c.modern_places.clone();
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        Scope::confirm(&mut c, 41).unwrap();
        c.frame_limit = 30;
        c.modern_scale = 125;
        c.auto_navigation = true;
        c.training_hud = true;
        let saved = f.save(&c);
        assert_eq!(saved.frame_limit, 30);
        assert_eq!(saved.modern_scale, 125);
        assert!(!saved.auto_navigation);
        assert!(!saved.training_hud);
        assert_eq!(saved.modern_places, before);
    }
    #[test]
    fn conflict_preserves_external_profile_and_previous_revision() {
        let f = Fixture::new();
        let mut c = Config::default();
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        Scope::confirm(&mut c, 41).unwrap();
        f.save(&c);
        let path = c.character_ui.overlay.as_ref().unwrap().path.clone();
        let first = std::fs::read(&path).unwrap();
        c.forever_map.navigation = 0;
        f.save(&c);
        assert_eq!(std::fs::read(path.parent().unwrap().join("previous.json")).unwrap(), first);
        std::fs::write(&path, b"external change").unwrap();
        assert!(c.character_ui.for_global_save(&c).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external change");
    }
    /// A profile saved before the Forever Enhancements move (no marker; tracker, Training tracker and Auto Navigation off by
    /// the old default) turns them on once at confirm and writes the marker at its next save; a value the player turns off
    /// after that stays off.
    #[test]
    fn profile_saved_before_the_enhancement_move_turns_tracker_on_once() {
        let forever = || Config {
            server: "FFXI Forever".into(),
            named_servers: vec![crate::config::NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            ..Default::default()
        };
        let f = Fixture::new();
        let mut c = forever();
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        Scope::confirm(&mut c, 41).unwrap();
        c.quest_tracker = false;
        c.training_hud = false;
        c.auto_navigation = false;
        f.save(&c);
        let path = c.character_ui.overlay.as_ref().unwrap().path.clone();
        // Rewrite the profile as the previous build saved it: no marker, the three off.
        let mut doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let prefs = doc["preferences"].as_object_mut().unwrap();
        prefs.remove("enhancements");
        prefs.insert("tracker".into(), serde_json::json!(false));
        prefs.insert("training".into(), serde_json::json!(false));
        prefs.insert("navigation".into(), serde_json::json!(false));
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        // A new session whose global config has the marker: the old profile's tracker moves on.
        Scope::end(&mut c);
        let mut c = forever();
        c.forever_enhancements_on = true;
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        assert!(Scope::confirm(&mut c, 41).unwrap());
        assert!(c.quest_tracker && c.training_hud && c.auto_navigation);
        // The next save writes the marker and the three on.
        f.save(&c);
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["preferences"]["enhancements"], serde_json::json!(true));
        assert_eq!(doc["preferences"]["tracker"], serde_json::json!(true));
        assert_eq!(doc["preferences"]["training"], serde_json::json!(true));
        assert_eq!(doc["preferences"]["navigation"], serde_json::json!(true));
        // The player's Off after the move is kept on the next load.
        c.quest_tracker = false;
        c.training_hud = false;
        c.auto_navigation = false;
        f.save(&c);
        Scope::end(&mut c);
        let mut c = forever();
        c.forever_enhancements_on = true;
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        Scope::confirm(&mut c, 41).unwrap();
        assert!(!c.quest_tracker && !c.training_hud && !c.auto_navigation);
    }

    #[test]
    fn future_schema_corrupt_and_wrong_identity_profiles_are_never_overwritten() {
        for variant in [0, 1, 2] {
            let f = Fixture::new();
            let mut c = Config::default();
            f.begin(&mut c, Some(7), 41, "GMBlizz");
            Scope::confirm(&mut c, 41).unwrap();
            f.save(&c);
            let path = c.character_ui.overlay.as_ref().unwrap().path.clone();
            let mut d: Document = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if variant == 0 {
                d.schema_version = 99;
            } else {
                d.identity.character = 42;
            }
            let bad = if variant == 2 { b"broken".to_vec() } else { serde_json::to_vec(&d).unwrap() };
            std::fs::write(&path, &bad).unwrap();
            f.begin(&mut c, Some(7), 41, "GMBlizz");
            assert!(Scope::confirm(&mut c, 41).is_err());
            c.forever_map.navigation = 2;
            f.save(&c);
            assert_eq!(std::fs::read(&path).unwrap(), bad);
        }
    }
    #[test]
    fn pending_write_and_other_writer_lock_fail_without_overwrite() {
        for file in ["write.lock", "profile.pending"] {
            let f = Fixture::new();
            let mut c = Config::default();
            f.begin(&mut c, Some(7), 41, "GMBlizz");
            Scope::confirm(&mut c, 41).unwrap();
            let dir = c.character_ui.overlay.as_ref().unwrap().path.parent().unwrap().to_owned();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(file), b"preserve").unwrap();
            assert!(c.character_ui.for_global_save(&c).is_err());
            assert_eq!(std::fs::read(dir.join(file)).unwrap(), b"preserve");
            assert!(!dir.join("profile.json").exists());
        }
    }
    #[test]
    fn backup_payload_is_typed_and_profile_revisions_are_noop_stable() {
        let f = Fixture::new();
        let mut c = Config::default();
        c.username = "secret-user".into();
        c.saved_password = "secret-password".into();
        f.begin(&mut c, Some(7), 41, "GMBlizz");
        Scope::confirm(&mut c, 41).unwrap();
        f.save(&c);
        let path = c.character_ui.overlay.as_ref().unwrap().path.clone();
        let a = std::fs::read(&path).unwrap();
        f.save(&c);
        assert_eq!(a, std::fs::read(&path).unwrap());
        let text = String::from_utf8(a).unwrap();
        assert!(!text.contains("secret-user"));
        assert!(!text.contains("secret-password"));
        assert!(!text.contains("game_dir"));
    }
}
