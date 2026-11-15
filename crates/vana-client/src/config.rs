//! Persisted settings and the app's data locations.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `base`/vana-client, moved there from the project's old name (ffxi-linux) the first time.
fn app_dir(base: Option<PathBuf>) -> PathBuf {
    let base = base.unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("vana-client");
    let old = base.join("ffxi-linux");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    dir
}

pub fn data_dir() -> PathBuf {
    app_dir(dirs::data_dir())
}

/// A server-sent character name as a file name: letters and digits as they are, anything else (and a Windows device
/// name) as '_' and the name's bytes in hex, so the file stays in its folder and two names never share one.
fn name_file(name: &str) -> String {
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric()) && !(cfg!(windows) && device_name(name)) {
        return format!("{name}.json");
    }
    format!("_{}.json", name.bytes().map(|b| format!("{b:02x}")).collect::<String>())
}

/// CON, PRN, AUX, NUL, COM0..9 and LPT0..9, which Windows opens as devices whatever the extension.
fn device_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(n.as_str(), "con" | "prn" | "aux" | "nul")
        || (n.len() == 4 && (n.starts_with("com") || n.starts_with("lpt")) && n.as_bytes()[3].is_ascii_digit())
}

/// A character's saved chat lines (the modern chat's history): in the data dir, or beside an FFXI_CONFIG copy.
pub fn chat_history_path(name: &str) -> PathBuf {
    let dir = match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("chat"),
        None => data_dir().join("chat"),
    };
    dir.join(name_file(name))
}

/// The regime folder: in the data dir, or beside an FFXI_CONFIG copy, so a test run never writes the player's.
fn regime_dir() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("regime"),
        None => data_dir().join("regime"),
    }
}

/// A character's training regime state (client/regime.rs): a folder per server ("127.0.0.1-54002", its search
/// address) and a file per character, so a same-named character on another server has its own.
pub fn regime_path(server: &str, name: &str) -> PathBuf {
    regime_dir().join(server).join(name_file(name))
}

/// Where earlier builds kept a character's regime (by name alone), read when the server's own file is not there yet;
/// with FFXI_CONFIG, the data dir's by-name file too.
pub fn regime_legacy_paths(name: &str) -> Vec<PathBuf> {
    let mut v = vec![regime_dir().join(name_file(name))];
    v.extend(regime_moved(std::env::var_os("FFXI_CONFIG").is_some()).map(|d| d.join(name_file(name))));
    v
}

/// With FFXI_CONFIG, the data dir's file for this server and character: builds up to 1.0.5 kept every run's regime
/// there, so it is read before the by-name files while the one beside the config is not there yet.
pub fn regime_moved_path(server: &str, name: &str) -> Option<PathBuf> {
    regime_moved(std::env::var_os("FFXI_CONFIG").is_some()).map(|d| d.join(server).join(name_file(name)))
}

/// The data dir's regime folder when FFXI_CONFIG puts the current one elsewhere.
fn regime_moved(config_copy: bool) -> Option<PathBuf> {
    config_copy.then(|| data_dir().join("regime"))
}

/// The servers' certificate fingerprints (net/tls.rs): in the data dir, or beside an FFXI_CONFIG copy.
pub fn known_servers_path() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("known_servers.json"),
        None => data_dir().join("known_servers.json"),
    }
}

/// The change notes an update left for the restarted client (client/notes.rs): in the data dir, or beside an
/// FFXI_CONFIG copy.
pub fn whats_new_path() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("whats-new.json"),
        None => data_dir().join("whats-new.json"),
    }
}

/// The client's log: in the data dir, or beside an FFXI_CONFIG copy (test runs keep their own).
pub fn log_path() -> PathBuf {
    match std::env::var_os("FFXI_CONFIG") {
        Some(p) => PathBuf::from(p).with_file_name("vana-client.log"),
        None => data_dir().join("vana-client.log"),
    }
}

/// A test run's copy of the config (FFXI_CONFIG): saving it never touches the user's file.
pub fn is_copy() -> bool {
    std::env::var_os("FFXI_CONFIG").is_some()
}

/// FFXI_CONFIG points test runs at a copy, so they never write the user's file.
pub(crate) fn config_path() -> PathBuf {
    std::env::var_os("FFXI_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| app_dir(dirs::config_dir()).join("config.json"))
}

/// A login that worked: the server, the account (the Square Enix ID for PlayOnline), PlayOnline's member ID, and its
/// own Remember password with the password(s) sealed for that server and account (secret.rs).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedLogin {
    pub server: String,
    pub username: String,
    pub pol_id: String,
    pub remember_password: bool,
    pub saved_password: String,
    /// Fields of a newer client, kept as they are.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

/// A named server (not retail): its name goes in the login screen's Server field, and its address and ports are used
/// to connect. Ports left out fall back to the config's own (`auth_port` and the rest).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NamedServer {
    pub name: String,
    pub host: String,
    pub auth_port: Option<u16>,
    pub data_port: Option<u16>,
    pub view_port: Option<u16>,
    pub profile_port: Option<u16>,
    pub irc_port: Option<u16>,
    /// The friend list, presence and mail through the server's profile service (xi_profile); unset follows
    /// `social_named_only`.
    pub social: Option<bool>,
    /// The loader version this server checks for ("2.1.2"); unset keeps the config's.
    pub loader_version: Option<String>,
}

/// An upscaler value above this build's highest (a profile from a build with more upscalers) becomes the highest, FSR 2.
#[cfg(feature = "forever")]
fn normalize_upscaler(g: &mut GraphicsCfg) {
    g.upscaler = g.upscaler.min(UPSCALER_MAX);
}

/// Ours: a saved Enhanced profile from before the layered fog and the sun shadow range lacks their keys, so it takes the
/// Enhanced preset's values once (and keeps whatever it saved from then on). Only the Forever flavour has the options, and
/// only an Enhanced profile is touched: Original and Custom keep what they saved.
#[cfg(feature = "forever")]
fn migrate_enhanced_retune(raw: &serde_json::Value, g: &mut GraphicsCfg) {
    if !g.enhanced {
        return;
    }
    let saved = raw.get("graphics");
    let has = |key: &str| saved.and_then(|s| s.get(key)).is_some();
    let preset = GraphicsCfg::enhanced();
    if !has("fog_ramp") {
        g.fog_ramp = preset.fog_ramp;
    }
    if !has("shadow_range") {
        g.shadow_range = preset.shadow_range;
    }
    // Far View keys on its own marker, `far_view`.
    if !has("far_view") {
        g.far_view = preset.far_view;
    }
    // Shadow Cascades key on their own marker, `shadow_cascades`.
    if !has("shadow_cascades") {
        g.shadow_cascades = preset.shadow_cascades;
    }
    // Character Shade keys on its own marker, `actor_sun_shadows`.
    if !has("actor_sun_shadows") {
        g.actor_sun_shadows = preset.actor_sun_shadows;
    }
    // Wave 4 (soft sun shadows) keys on its own marker, `soft_shadows`.
    if !has("soft_shadows") {
        g.soft_shadows = preset.soft_shadows;
    }
    // Wave 3 (authored lighting, fog layers, ambient-only occlusion) keys on its own marker, `light_grade`: a profile saved by
    // a client that already had `ao_light` (wave 2) still lacks it, so it still takes the retune once.
    if !has("light_grade") {
        g.light_grade = preset.light_grade;
        g.light_grade_amount = preset.light_grade_amount;
        g.fog_layers = preset.fog_layers;
        g.ao_light = preset.ao_light;
        // The old preset's strength (0.6) was tuned for All Light; Ambient Only takes more without eating the sun.
        if (g.ao_strength - 0.6).abs() < 1e-4 {
            g.ao_strength = preset.ao_strength;
        }
    }
    // Wave 4b (Enhanced Water) keys on its own marker, `water_enhanced`.
    if !has("water_enhanced") {
        g.water_enhanced = preset.water_enhanced;
    }
    // Sun shafts key on their own marker.
    // Volumetric fog keys on its own marker too.
    if !has("volumetric_fog") {
        g.volumetric_fog = preset.volumetric_fog;
        g.volumetric_fog_amount = preset.volumetric_fog_amount;
    }
    if !has("sun_shafts") {
        g.sun_shafts = preset.sun_shafts;
        g.sun_shafts_amount = preset.sun_shafts_amount;
    }
    // Wave 4 (texture sharpening) keys on its own marker too.
    if !has("texture_sharpening") {
        g.texture_sharpening = preset.texture_sharpening;
    }
    // Wave 4f (procedural clouds) keys on its own marker, `sky_clouds`.
    if !has("sky_clouds") {
        g.sky_clouds = preset.sky_clouds;
    }
}

/// The one-time move to Enhanced graphics (the owner: Forever's upgrades on by default), at load: a Forever profile, new or
/// still on the untouched original graphics, takes the Enhanced preset; Custom, an explicit Enhanced or anything changed is
/// kept, and so is any later choice of Original. The in-memory default stays the original graphics.
#[cfg(feature = "forever")]
fn migrate_graphics_default_on(c: &mut Config) {
    if c.graphics_default_on || c.classic_only || !c.is_forever() {
        return;
    }
    if c.graphics == GraphicsCfg::default() {
        c.graphics = GraphicsCfg::enhanced();
    }
    c.graphics_default_on = true;
}

/// Ours: the Forever Enhancements page's options that Config's defaults leave off (quest tracker, target distance and HP,
/// earth clock, the Training tracker, the Auto Navigation preference, and the display helpers: the timers window, the
/// training regime window, the skillchain helper and the classic UI's minimap) turn on once for a Forever profile (the
/// owner: "default to on"). The options whose default was already on (quest indicators, Auto Rotation at start, the Solo,
/// Job and EXP panels, con colours, the Modern minimap...) are not touched: their saved value is a choice. These stay off
/// whatever the move: Auto-equip (it changes what the character wears), the map overlay (it draws over the scene), Text
/// Auto-Advance (it skips dialogue), timestamps, Synthesis Amount and the content-hiding filters (Trust Speech, Ready
/// Lines). Auto Navigation only enables the helper: nothing moves without Start. Every save writes all of Config's keys,
/// so a key's absence says nothing about a choice: the marker `forever_enhancements_on` does. A config without it has not
/// had the move, so its ten values are set on here, whatever they say, and the marker is saved at once (load()). That
/// means an explicit Off saved before this marker existed turns on once; a choice made after it is kept. Config's own
/// Default stays off (the classic flavour and the tests read it). Classic_only sessions and non-Forever servers are
/// untouched and keep the marker unset, so a later Forever load still moves them. Returns whether it moved.
#[cfg(feature = "forever")]
fn migrate_forever_enhancements(c: &mut Config) -> bool {
    if c.forever_enhancements_on || c.classic_only || !c.is_forever() {
        return false;
    }
    c.quest_tracker = true;
    c.target_distance = true;
    c.target_hpp = true;
    c.earth_clock = true;
    c.training_hud = true;
    c.auto_navigation = true;
    c.timers = true;
    c.regime = true;
    c.skillchain_helper = true;
    c.minimap = true;
    c.forever_enhancements_on = true;
    true
}

/// Ours: the Forever Modern UI's Compact HUD (smaller panel margins, narrower tracker and chat, per-panel folds) turns on
/// once for a Forever profile (the owner: the HUD takes too much of the screen). The marker `forever_compact_on` is the
/// once: a config without it has not had the move, so Compact HUD is set on and the marker saved at once (load()); a
/// later explicit Off is a choice and is kept. Config's own Default stays off (the classic flavour and the tests read it),
/// and classic_only sessions and non-Forever servers keep the marker unset. Returns whether it moved.
#[cfg(feature = "forever")]
fn migrate_forever_compact(c: &mut Config) -> bool {
    if c.forever_compact_on || c.classic_only || !c.is_forever() {
        return false;
    }
    c.forever_compact_hud = true;
    c.forever_compact_on = true;
    true
}

/// Writes `data` to `path` whole or not at all: a temporary file beside it, flushed, then renamed over it (the rename
/// replaces the old file on every platform). A crash mid-save leaves the previous config, never a cut-short one (which
/// the launcher's merge would set aside as corrupt, losing every client setting).
fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Ours: the Modern controls are the default for new players only. A config the client saved before the controls
/// existed (it has `modern_ui`, written with every other key, but no `controls`) keeps the original keys; a fresh one,
/// or one only a launcher wrote, gets the default.
fn legacy_controls(v: &serde_json::Value, parsed: u32) -> u32 {
    // The classic-only client (regular FFXI) starts on the original controls too.
    let classic = v.get("classic_only").and_then(|b| b.as_bool()).unwrap_or(false);
    if v.get("controls").is_none() && (classic || v.get("modern_ui").is_some()) { 0 } else { parsed }
}

/// Ours: the containers kept sorted by default: the inventory, the Mog Safe and Storage, the Locker, the satchel, sack
/// and case, Mog Safe 2 (container ids 0, 1, 2, 4, 5, 6, 7, 9).
pub const AUTO_SORT_BAGS: u32 = 1 | 1 << 1 | 1 << 2 | 1 << 4 | 1 << 5 | 1 << 6 | 1 << 7 | 1 << 9;

/// `ui_scale`'s Auto.
pub const UI_SCALE_AUTO: u32 = 1;

/// A Forever default that is on (a profile without the key takes it).
#[cfg(feature = "forever")]
fn default_on() -> bool {
    true
}

fn ui_scale_auto() -> u32 {
    UI_SCALE_AUTO
}

/// The classic UI's stretch for `ui_scale` at this window size; None leaves it to the Menu Size.
pub fn ui_scale_factor(ui_scale: u32, window: [u32; 2]) -> Option<f32> {
    match ui_scale {
        0 => None,
        UI_SCALE_AUTO => Some((window[1] as f32 / 1080.0).max(1.0)),
        p => Some((p as f32 / 100.0).clamp(0.5, 4.0)),
    }
}

fn era_lock_on() -> bool {
    true
}

/// Ours (not retail): the optional post-processing over the drawn 3D scene, before the 2D layer (render/post.rs). All off by
/// default; with every option off no pass, pipeline or texture exists. A `graphics` object of its own, so the launcher's
/// merge of config.json keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsCfg {
    /// Forever's optional renderer enhancements; classic builds do not deserialize this setting.
    #[cfg(feature = "forever")]
    pub enhanced: bool,
    /// FXAA 3.11 (luma edge search) on the scene.
    pub fxaa: bool,
    /// Bloom: bright parts of the scene above `bloom_threshold` (0..1, in the picture's brightness) blurred and added back at
    /// `bloom_intensity` (0..2).
    pub bloom: bool,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    /// A filmic (ACES-fitted) curve on the scene with an `exposure` multiplier (0.25..4, 1 = as drawn); the scene is 8-bit, so
    /// this is a grade, not HDR.
    pub tonemap: bool,
    pub exposure: f32,
    /// Contrast-adaptive sharpening (AMD CAS style), strength 0..1; 0 = off.
    pub sharpen: f32,
    /// Saturation, 1 = as drawn, 0 = grey, up to 2.
    pub saturation: f32,
    /// Screen-space ambient occlusion from the scene's depth (render/postdepth.rs): creases and contact shadows darkened by
    /// `ao_strength` (0..2, 1 = as tuned; the scenery already has baked vertex lighting, so that is moderate), over
    /// `ao_radius` world units (0.25..4); it fades out with distance.
    pub ao: bool,
    pub ao_strength: f32,
    pub ao_radius: f32,
    /// Height fog: an exponential haze that is densest at the camera's look-at height and below it and thins above, tinted by
    /// the zone's own fog colour. `fog_density` 0..2 (1 = 0.004 a unit), `fog_height` the world units it takes to thin to 1/e
    /// (2..200). It adds to the retail distance fog (which is kept), not on top of it twice.
    pub fog: bool,
    pub fog_density: f32,
    pub fog_height: f32,
    /// Layered fog (Forever Enhanced, render/fogramp.rs): the height fog's density and height follow the Vana'diel time of
    /// day (thicker at dawn and dusk) and the weather (thicker in mist, rain, storms and fog), eased, on top of the zone's own
    /// fog colour, which already follows both. Needs `fog`. Classic builds do not deserialize this setting.
    #[cfg(feature = "forever")]
    pub fog_ramp: bool,
    /// What the ambient occlusion darkens (Forever; needs `ao`): 0 all the light of the scenery as before, 1 only its ambient
    /// light, so direct sun and lamp light keep their brightness and only shaded corners and creases darken
    /// (render/ambientshare.rs). Not in the Enhanced preset: chosen in Settings.
    #[cfg(feature = "forever")]
    pub ao_light: u32,
    /// Authored zone lighting (Forever Enhanced, render/lightgrade.rs): the zone's own sun, ambient, fog and sky colours re-graded
    /// toward Forever's look by its profile, the time of day and `light_grade_amount` (0..1.5, 1 = as authored; the Settings
    /// row shows percent). Classic builds do not deserialize these settings.
    #[cfg(feature = "forever")]
    pub light_grade: bool,
    #[cfg(feature = "forever")]
    pub light_grade_amount: f32,
    /// Volume-style fog layers on the Height Fog (render/fogramp.rs `FogLayers`): a ground mist, a glow toward the sun and a
    /// distance haze. Needs `fog`.
    #[cfg(feature = "forever")]
    pub fog_layers: bool,
    /// Enhanced Water (Forever, render/water.rs `fs_water_fx`): the river finish over known wading water gains two scrolling
    /// procedural wave layers, a Fresnel sky reflection, a sun glint and a shoreline foam band. Needs `enhanced` (the finish
    /// itself is Enhanced-only); off keeps the plain finish. Classic builds do not deserialize this setting.
    #[cfg(feature = "forever")]
    pub water_enhanced: bool,
    /// Sun shafts (Forever Enhanced, render/postdepth.rs): screen-space crepuscular rays from the sun's screen position, over
    /// the sky seen through trees, arches and buildings; strongest at dawn and golden hour, none at night, indoors, with the
    /// sun off screen or in heavy weather. `sun_shafts_amount` 0..1.5 (1 = as tuned; the Settings row shows percent).
    /// Classic builds do not deserialize these settings.
    #[cfg(feature = "forever")]
    pub sun_shafts: bool,
    #[cfg(feature = "forever")]
    pub sun_shafts_amount: f32,
    /// Volumetric fog (Forever Enhanced, render/postdepth.rs): the sun's light scattered by the fog along each view ray, marched
    /// half size through the fog's own density and (where the directional shadow map covers) through the sun's shadows, so light
    /// shafts come out of the fog under trees and arches. It adds light to the Height Fog, Layered Fog and Fog Layers (their
    /// thickness is still theirs; their sun glow stays) and needs the Height Fog on. `volumetric_fog_amount` 0..1.5
    /// (1 = as tuned; the Settings row shows percent). Classic builds do not deserialize these settings.
    #[cfg(feature = "forever")]
    pub volumetric_fog: bool,
    #[cfg(feature = "forever")]
    pub volumetric_fog_amount: f32,
    /// Detail textures (Forever Enhanced, zone.wgsl `detail_factor`): a procedural, data-free fine-grain layer over the opaque
    /// zone scenery (ground, floors, walls, rock), anchored in world space and strongest where the base texture is stretched.
    /// `detail_strength` 0..2 (1 = as tuned; the Settings row shows percent). It fades with distance and where it would
    /// shimmer. Classic builds do not deserialize these settings.
    #[cfg(feature = "forever")]
    pub detail_textures: bool,
    #[cfg(feature = "forever")]
    pub detail_strength: f32,
    /// Texture sharpening (zone.wgsl / render/detail.wgsl): where a base texture is magnified (stretched, blurry up close) a
    /// Catmull-Rom reconstruction, clamped to the local texels, replaces bilinear; 0 = off, 1 = plain Catmull-Rom, up to 1.5
    /// (a mild boost). Minified surfaces are untouched. Classic builds do not deserialize this setting.
    #[cfg(feature = "forever")]
    pub texture_sharpening: f32,
    /// Procedural clouds (Forever Enhanced, render/skyclouds.rs): a data-free, wind-drifting cloud layer in the sky pass over
    /// the zone's own sky rings, lit by its sun, in heavy weather only (clear and fine skies are untouched), none indoors. Needs `enhanced`. Classic builds do
    /// not deserialize this setting.
    #[cfg(feature = "forever")]
    pub sky_clouds: bool,
    /// Forever Enhanced's directional sun shadow coverage (render/forever_sun.rs): 0 Near (40 units around the camera's
    /// target, a 1024 map, the original), 1 Wide (80 units, 2048: the same sharpness over four times the ground), 2 Far (120
    /// units, 4096). Wider costs shadow-pass fill and 4 / 16 / 64 MB of video memory (16 / 36 / 64 MB with `soft_shadows`). Only read while `enhanced`.
    #[cfg(feature = "forever")]
    pub shadow_range: u32,
    /// Soft sun shadows (Forever Enhanced, render/forever_sun.rs + sunshadow.wgsl): a 16 tap filter of a fixed world width
    /// instead of the 3 by 3 one, a normal-offset and slope-scaled receiver bias instead of the large constant one, and a
    /// finer map (`sun_shadow_map`: Near 2048, Wide 3072; Far stays 4096). Only read while `enhanced`.
    #[cfg(feature = "forever")]
    pub soft_shadows: bool,
    /// Character Shade (Forever Enhanced, render/sunshadow.wgsl `actor_shade`): characters, NPCs and monsters standing in the
    /// scenery's sun shadow (under a tree or a roof) darken by an extra `ACTOR_SHADE` of their light, on top of the direct light
    /// the shadow already removes; faded out toward night. Only read while `enhanced`.
    #[cfg(feature = "forever")]
    pub actor_sun_shadows: bool,
    /// Shadow Cascades (Forever Enhanced, render/forever_sun.rs): three nested sun shadow maps around the camera's target (a 24, a
    /// 72 and a 200 unit radius, 2048 texels each, 48 MB; 1024 each when the card is short of memory) instead of the single map:
    /// finer shadows near and shadows far out, blended at their edges. They use the soft filter. When the card has no room for
    /// them the single map stays. Only read while `enhanced`.
    #[cfg(feature = "forever")]
    pub shadow_cascades: bool,
    /// Far View (Forever Enhanced, render/farview.rs): retail's Clipping setting taken past 100% in open-air zones, 0 Retail,
    /// 1 Far (1.5x), 2 Very Far (2x): the scenery's draw distance and the fog that hides the horizon move out together. Only
    /// read while `enhanced`.
    #[cfg(feature = "forever")]
    pub far_view: u32,
    /// A soft light on characters (NPCs, players, monsters), 0..1 (the Settings row shows percent): an ambient floor that
    /// keeps the night's hue and a faint fill from the camera's side, so the Enhanced graphics' deeper shadows (the tone
    /// curve, ambient occlusion, height fog) do not sink them in the dark. It acts only while one of those is on; 0 = off.
    pub character_light: f32,
    /// Forever's upscaler: 0 = the plain scaling pass (Catmull-Rom up, box down; unchanged), 1 = AMD FidelityFX Super
    /// Resolution 1.0 (render/fsr.rs). With it on the 3D scene draws at the window divided by the quality mode's factor
    /// (Background Resolution is overridden) and is upscaled in two passes (EASU, RCAS); with it off no FSR pass or texture
    /// exists. Not part of `active()`: it adds no post-processing pass. Classic builds do not deserialize it.
    #[cfg(feature = "forever")]
    pub upscaler: u32,
    /// The upscaler's quality mode, an index into `FSR_MODES`: Ultra Quality, Quality, Balanced, Performance.
    #[cfg(feature = "forever")]
    pub upscale_quality: u32,
}

/// The highest `GraphicsCfg::upscaler` value this build offers: 1 = FSR 1, 2 = AMD FSR 2 (3 is reserved).
#[cfg(feature = "forever")]
pub const UPSCALER_MAX: u32 = 2;

/// FSR 1's quality modes (AMD's recommended per-axis scale factors): the scene draws at the window divided by this.
#[cfg(feature = "forever")]
pub const FSR_MODES: [(&str, f32); 4] =
    [("Ultra Quality", 1.3), ("Quality", 1.5), ("Balanced", 1.7), ("Performance", 2.0)];

/// The 3D scene's size for the window and an upscale `factor` (per axis, at least one pixel).
#[cfg(feature = "forever")]
pub fn fsr_render_size(window: [u32; 2], factor: f32) -> [u32; 2] {
    window.map(|v| ((v as f32 / factor).round() as u32).max(1))
}

impl Default for GraphicsCfg {
    fn default() -> Self {
        Self {
            #[cfg(feature = "forever")]
            enhanced: false,
            fxaa: false,
            bloom: false,
            bloom_intensity: 0.4,
            bloom_threshold: 0.85,
            tonemap: false,
            exposure: 1.0,
            sharpen: 0.0,
            saturation: 1.0,
            ao: false,
            ao_strength: 1.0,
            ao_radius: 1.2,
            fog: false,
            fog_density: 0.8,
            fog_height: 20.0,
            #[cfg(feature = "forever")]
            fog_ramp: false,
            #[cfg(feature = "forever")]
            ao_light: 0,
            #[cfg(feature = "forever")]
            light_grade: false,
            #[cfg(feature = "forever")]
            light_grade_amount: 1.0,
            #[cfg(feature = "forever")]
            fog_layers: false,
            #[cfg(feature = "forever")]
            water_enhanced: false,
            #[cfg(feature = "forever")]
            sun_shafts: false,
            #[cfg(feature = "forever")]
            sun_shafts_amount: 1.0,
            #[cfg(feature = "forever")]
            volumetric_fog: false,
            #[cfg(feature = "forever")]
            volumetric_fog_amount: 1.0,
            #[cfg(feature = "forever")]
            detail_textures: false,
            #[cfg(feature = "forever")]
            detail_strength: 1.0,
            #[cfg(feature = "forever")]
            texture_sharpening: 0.0,
            #[cfg(feature = "forever")]
            sky_clouds: false,
            #[cfg(feature = "forever")]
            shadow_range: 0,
            #[cfg(feature = "forever")]
            soft_shadows: false,
            #[cfg(feature = "forever")]
            actor_sun_shadows: false,
            #[cfg(feature = "forever")]
            shadow_cascades: false,
            #[cfg(feature = "forever")]
            far_view: 0,
            character_light: 0.75,
            #[cfg(feature = "forever")]
            upscaler: 0,
            #[cfg(feature = "forever")]
            upscale_quality: 1,
        }
    }
}

/// How much of an actor's light Character Shade takes away at full sun shadow.
#[cfg(feature = "forever")]
pub const ACTOR_SHADE: f32 = 0.3;

impl GraphicsCfg {
    /// A restrained grade over the original art, at native resolution rather than automatic supersampling.
    #[cfg(feature = "forever")]
    pub fn enhanced() -> Self {
        Self {
            enhanced: true,
            fxaa: true,
            bloom: true,
            bloom_intensity: 0.12,
            bloom_threshold: 0.9,
            tonemap: true,
            sharpen: 0.2,
            ao: true,
            ao_strength: 0.9,
            ao_radius: 0.9,
            ao_light: 1,
            fog: true,
            fog_density: 0.35,
            fog_height: 18.0,
            fog_ramp: true,
            shadow_range: 1,
            soft_shadows: true,
            actor_sun_shadows: true,
            shadow_cascades: true,
            far_view: 1,
            light_grade: true,
            fog_layers: true,
            water_enhanced: true,
            // Texture Sharpening is in the preset (close-ups read crisper, no artifacts); the Detail Textures grain stays a
            // Settings choice (off): it read as marginal.
            texture_sharpening: 1.0,
            sun_shafts: true,
            volumetric_fog: true,
            sky_clouds: true,
            ..Default::default()
        }
    }
    /// The scale factor while AMD FSR 2 (temporal, render/fsr2.rs) is picked, else None: the same modes as FSR 1.
    pub fn fsr2_factor(&self) -> Option<f32> {
        #[cfg(feature = "forever")]
        // 2, or a value past this build's highest (a profile from a build with more upscalers) that is read as FSR 2 here.
        if self.upscaler == 2 || self.upscaler > UPSCALER_MAX {
            return Some(FSR_MODES[(self.upscale_quality as usize).min(FSR_MODES.len() - 1)].1);
        }
        None
    }
    /// The scale factor of whichever upscaler is picked.
    pub fn upscale_factor(&self) -> Option<f32> {
        self.fsr_factor().or(self.fsr2_factor())
    }
    /// The FSR scale factor while the upscaler is on (Forever builds only), else None.
    pub fn fsr_factor(&self) -> Option<f32> {
        #[cfg(feature = "forever")]
        if self.upscaler == 1 {
            return Some(FSR_MODES[(self.upscale_quality as usize).min(FSR_MODES.len() - 1)].1);
        }
        None
    }
    /// The effects as the renderer runs them under FSR: RCAS is the sharpening, so the contrast-adaptive pass (which would
    /// run before the upscale and be stretched by it) is dropped. Unchanged with the upscaler off.
    pub fn for_upscale(self) -> Self {
        if self.upscale_factor().is_some() { Self { sharpen: 0.0, ..self } } else { self }
    }
    pub fn bloom_on(&self) -> bool {
        self.bloom && self.bloom_intensity > 0.001
    }
    pub fn sharpen_on(&self) -> bool {
        self.sharpen > 0.005
    }
    pub fn saturation_on(&self) -> bool {
        (self.saturation - 1.0).abs() > 0.005
    }
    pub fn ao_on(&self) -> bool {
        self.ao && self.ao_strength > 0.005
    }
    pub fn fog_on(&self) -> bool {
        self.fog && self.fog_density > 0.0005
    }
    /// The directional sun shadow's map size in texels and half-width in world units: the original 1024 / 40 unless Enhanced
    /// chose a wider `shadow_range`.
    #[cfg(feature = "forever")]
    pub fn sun_shadow_map(&self) -> (u32, f32) {
        let soft = self.soft_shadows_on();
        match (self.enhanced, self.shadow_range) {
            (true, 1) => (if soft { 3072 } else { 2048 }, 80.0),
            (true, r) if r >= 2 => (4096, 120.0),
            _ => (if soft { 2048 } else { 1024 }, 40.0),
        }
    }
    /// The multiplier Far View puts on retail's Clipping factor X: 1 unless Enhanced chose Far or Very Far (always 1 in the
    /// classic flavour).
    pub fn far_view_factor(&self) -> f32 {
        #[cfg(feature = "forever")]
        {
            if self.enhanced { crate::render::farview::factor(self.far_view) } else { 1.0 }
        }
        #[cfg(not(feature = "forever"))]
        {
            1.0
        }
    }
    /// Shadow cascades: chosen, and only while Enhanced (Original and the classic flavour keep the single map).
    pub fn cascades_on(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.enhanced && self.shadow_cascades
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }
    /// Character Shade's strength for the shader: `ACTOR_SHADE` while chosen and Enhanced, else 0 (off).
    pub fn actor_shade_amount(&self) -> f32 {
        #[cfg(feature = "forever")]
        {
            if self.enhanced && self.actor_sun_shadows { ACTOR_SHADE } else { 0.0 }
        }
        #[cfg(not(feature = "forever"))]
        {
            0.0
        }
    }
    /// Soft sun shadows: chosen, and only while Enhanced (Original and the classic flavour keep the original filter).
    pub fn soft_shadows_on(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.enhanced && self.soft_shadows
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }
    /// The ambient-only occlusion: chosen (`ao_light` 1), with the occlusion on. The one decision the zone shaders' share, its
    /// capture and the post pass's weighting all follow; false in the classic flavour.
    pub fn ambient_ao_on(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.ao_on() && self.ao_light == 1
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }
    /// The layered fog's ramps: only with the height fog on (and never in the classic flavour).
    pub fn light_grade_on(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.light_grade && self.light_grade_amount > 0.005
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }
    /// The detail layer's strength the renderers use: 0 (off) unless `detail_textures` with a strength above zero; never in the
    /// classic flavour.
    pub fn detail_strength_on(&self) -> f32 {
        #[cfg(feature = "forever")]
        {
            if self.detail_textures { self.detail_strength.clamp(0.0, 2.0) } else { 0.0 }
        }
        #[cfg(not(feature = "forever"))]
        {
            0.0
        }
    }
    /// The volumetric fog's strength the renderers use: 0 (off) unless chosen with the Height Fog on; never in the classic flavour.
    pub fn volumetric_fog_on(&self) -> f32 {
        #[cfg(feature = "forever")]
        {
            if self.fog_on() && self.volumetric_fog { self.volumetric_fog_amount.clamp(0.0, 1.5) } else { 0.0 }
        }
        #[cfg(not(feature = "forever"))]
        {
            0.0
        }
    }
    /// The sun shafts' strength the renderers use: 0 (off) unless chosen; never in the classic flavour.
    pub fn sun_shafts_on(&self) -> f32 {
        #[cfg(feature = "forever")]
        {
            if self.sun_shafts { self.sun_shafts_amount.clamp(0.0, 1.5) } else { 0.0 }
        }
        #[cfg(not(feature = "forever"))]
        {
            0.0
        }
    }
    /// The sharpening amount the renderers use: 0 (off) unless chosen; never in the classic flavour.
    pub fn texture_sharpening_on(&self) -> f32 {
        #[cfg(feature = "forever")]
        {
            self.texture_sharpening.clamp(0.0, 1.5)
        }
        #[cfg(not(feature = "forever"))]
        {
            0.0
        }
    }
    /// The grading strength the renderers use: 0 unless `light_grade_on`.
    pub fn light_grade_strength(&self) -> f32 {
        #[cfg(feature = "forever")]
        {
            if self.light_grade_on() { self.light_grade_amount.clamp(0.0, 1.5) } else { 0.0 }
        }
        #[cfg(not(feature = "forever"))]
        {
            0.0
        }
    }
    /// The procedural clouds: chosen, in the Enhanced look (the sky pass is Enhanced's; classic never).
    pub fn sky_clouds_on(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.enhanced && self.sky_clouds
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }
    pub fn fog_layers_on(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.fog_on() && self.fog_layers
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }
    pub fn fog_ramp_on(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.fog_on() && self.fog_ramp
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }
    /// The character light the renderer uses: `character_light` with the tone curve on (which takes about a fifth off a
    /// night character), 0.3 of it with only ambient occlusion or height fog (they darken little), else 0, so retail and
    /// classic lighting stay as drawn.
    pub fn character_light_amount(&self) -> f32 {
        let set = self.character_light.clamp(0.0, 1.0);
        if self.tonemap {
            set
        } else if self.ao_on() || self.fog_on() {
            // Occlusion and fog alone darken a character only a little (about 2% close up), so they get a fraction.
            set * 0.3
        } else {
            0.0
        }
    }
    /// Either effect that reads the scene's depth (and so needs the camera).
    pub fn depth_on(&self) -> bool {
        self.ao_on() || self.fog_on() || self.sun_shafts_on() > 0.0
    }
    /// Anything the composite pass does (everything but FXAA and the depth effects).
    pub fn composite_on(&self) -> bool {
        self.bloom_on() || self.tonemap || self.sharpen_on() || self.saturation_on()
    }
    /// Whether the post pass runs at all; false = the render path is exactly the one without it.
    pub fn active(&self) -> bool {
        self.fxaa || self.composite_on() || self.depth_on()
    }
}

/// Server names compare without case or outer spaces ("PlayOnline", "playonline").
fn same_server(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// What a server's rest ticks look like, as learnt (client/resttick.rs keeps it moving).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RestModel {
    /// From the rest's start to the first heal seen (seconds), a mean over rests.
    pub first: f32,
    /// The intervals after each heal, means over rests; the last stands for every later one.
    pub gaps: Vec<f32>,
    /// How many rests each value holds ([first, gaps...]), capped so later rests still move it.
    pub seen: Vec<u32>,
    /// The server's look period (its ticks land on the first look after their time); 0: on time.
    pub period: f32,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Runtime overlay only; credentials never enter character UI profiles.
    #[cfg(feature = "forever")]
    #[serde(skip)]
    pub(crate) character_ui: crate::character_ui::Scope,
    /// Refuse to overwrite a config that could not be read or safely backed up.
    #[serde(skip)]
    pub(crate) save_blocked: bool,
    /// Holds the "FINAL FANTASY XI" folder, laid out as the Windows installer does.
    pub game_dir: PathBuf,
    pub installed: bool,

    pub server: String,
    /// The login's account; the Square Enix ID for PlayOnline.
    pub username: String,
    /// PlayOnline's member ID (8 characters), for the PlayOnline server.
    pub pol_id: String,
    /// Logins that worked, most recent first: the login screen's Server and Account lists (not retail).
    pub logins: Vec<SavedLogin>,
    /// The login screen's Remember password (not retail), and the password sealed for this server and account
    /// (secret.rs; hex, empty when none).
    pub remember_password: bool,
    pub saved_password: String,
    pub auth_port: u16,
    pub data_port: u16,
    pub view_port: u16,
    pub profile_port: u16,
    pub irc_port: u16,
    /// Named servers with their own address and ports (not retail; `resolved`).
    pub named_servers: Vec<NamedServer>,
    /// Not retail: only named servers with `social` set get the friend list, presence and mail; every other server
    /// gets none of them (no connection to its profile ports).
    pub social_named_only: bool,
    /// Not retail: the xiloader version reported at login ("2.1.2"); empty = the client's own (net::auth).
    pub loader_version: String,
    /// Version string sent to the lobby; LSB rejects clients older than its CLIENT_VER.
    pub client_version: String,
    /// cpal output device id ("pipewire:..."); empty = the system default.
    pub audio_device: String,
    /// Frame limit in frames per second (one of FRAME_LIMITS), 0 = unlimited. Not a retail option;
    /// game ticks stay at 60 Hz and motions at 30 frames a second whatever this is.
    pub frame_limit: u32,
    /// Comfort option: at most 60 FPS, no animated first-person head bob or effect camera shake.
    pub reduce_motion: bool,
    /// Anisotropic filtering for scenery, models and the creator (1 = off, up to 16). Not a retail option.
    pub anisotropy: u16,
    /// Anti-aliasing of the 3D scene: 0 off (retail), 1 SMAA (was FXAA before 2026-11-04). Not a retail option.
    pub antialias: u32,
    /// Colour grading of the 3D scene: 0 off (retail), 1 Vivid, 2 Warm, 3 Cool, 4 Film (render/scale.rs). Not a
    /// retail option.
    pub grading: u32,
    /// How far scenery, actors and effects draw, in percent of retail's distance (the zone's, scaled by the
    /// Clipping setting); 0 = no limit. Not a retail option.
    pub draw_distance: u32,
    /// Milliseconds between the datagrams sent to a private server's zone server (50..400; 400 is the game's, which a
    /// PlayOnline sign-in always keeps). The server answers each one, so its updates come sooner. Not a retail option.
    pub send_interval: u32,
    /// Party and alliance rows show TP (retail config 0xae bit 0, default off; ours on, as the add-ons do).
    pub party_tp: bool,
    /// Forever M-map presentation, separate from the saved add-on overlay.
    #[cfg(feature = "forever")]
    pub forever_map: crate::client::mapview::Prefs,
    /// Per-window opacity the player set (no row for it yet), by window: a modern panel's id, a mod's window
    /// ("mod:<mod>:<window>") or a mod's HUD window ("hud:<mod>:<window>"). It wins over a mod's own.
    #[serde(default)]
    pub window_opacity: std::collections::BTreeMap<String, f32>,
    /// Extra: the map overlay (Ctrl / Alt + O; the add-ons' map overlay).
    pub map_overlay: bool,
    /// Extra: Synthesize asks how many and repeats.
    pub synth_amount: bool,
    /// Extra (the timestamp add-on): retail's chat timestamps (config 0xb5) on every log line; seconds while 0xb5 is
    /// off.
    pub stamp_all_lines: bool,
    /// Extra (the distance / target-info add-ons): the target's distance in the target window, and its HP percent in the
    /// classic one (the modern one shows it anyway).
    pub target_distance: bool,
    pub target_hpp: bool,
    /// Ours (the Forever cutscene strip's Auto, client/modern/cutscene.rs): event pages that wait for a key move on after
    /// this many seconds (0 = off). Upstream's Text Auto-Advance moved into a mod; this stays native.
    pub auto_advance: u32,
    /// Not retail (Settings' Lamp Lights): real point lights at the zones' lamps in place of their baked lamplight
    /// (client/lamps.rs).
    pub lamp_lights: bool,
    /// Character shadows: 0 retail's (blob / silhouette), 1 real (a shadow map from the sun, render/sunshadow.rs,
    /// retail's where there is no sun). Not a retail option.
    pub actor_shadows: u32,
    /// The real shadows' map size in texels (1024, 2048 or 4096; render/sunshadow.rs). Not a retail option.
    pub shadow_map: u32,
    /// Ambient occlusion on the scenery (render/ssao.rs). Not a retail option.
    pub ssao: bool,
    /// Extra (the clock add-ons): Earth time, the moon and the next day beside the Vana'diel clock (client/clock.rs).
    pub earth_clock: bool,
    /// Ours (not retail, client/era.rs): while the server's lobby bits name no expansion the client hides the windows of
    /// later systems, offers the base jobs only and caps its own level fields at 60. Always on and never read from the
    /// file: what a character sees follows only what its server reports (a server unlocks by sending the bits).
    #[serde(skip, default = "era_lock_on")]
    pub era_lock: bool,
    /// Ours (not retail): the classic UI's scale. 0 = the Menu Size (0037 / 0038) as retail, 1 = Auto (100% up to
    /// 1080 lines, then the window's height over 1080), else a percent. It keeps the window's shape.
    #[serde(default = "ui_scale_auto")]
    pub ui_scale: u32,
    /// Ours: the map (and search) server address to use instead of the one the lobby (and each zone change) names, its ports
    /// kept; for a server reached through a proxy or a NAT that hands out an address the client cannot reach.
    pub map_host: Option<std::net::Ipv4Addr>,
    /// The lobby's expansion bits at the last login by server and account (`era::seen_key`): a bit set that was not
    /// there then is announced once.
    pub expansions_seen: std::collections::BTreeMap<String, u32>,
    /// The Game Master window (ours, client/gm.rs): its request (c2s 0x1F0) goes out at a zone-in only while this is
    /// on and only to a private server. No Settings row: only a server with its GM extension can use it, so it is set
    /// in config.json by hand.
    pub gm_window: bool,
    /// The version whose change notes were last shown (client/notes.rs): a newer client shows the notes since. A
    /// config from before the notes has none, so its first newer build shows that build's own.
    #[serde(default)]
    pub notes_seen: String,
    /// Extra (the trust chatter filters): trusts' party lines (battle message 711) are not printed.
    pub hide_trust_speech: bool,
    /// Extra (Battlemod's filters): "readies" / "starts casting" lines only for us and monsters.
    pub hide_ready_lines: bool,
    /// Extras (Battlemod's simplify and colours, client/battlemod.rs): battle lines in their short form; names in
    /// battle lines coloured by who they are.
    pub short_battle: bool,
    pub battle_name_colours: bool,
    /// Extra (the Timers add-on): the timers window, its dock (0 left, 1 right, 2 top), its drawer out, folded
    /// sections (a bit each).
    pub timers: bool,
    pub timers_dock: u32,
    pub timers_out: bool,
    pub timers_folded: u8,
    /// Extra (ours): the training regime window (client/regime.rs) and its drawer out.
    pub regime: bool,
    pub regime_out: bool,
    /// Forever Modern UI: authoritative Training tracker, off until enabled in its menu.
    #[cfg(feature = "forever")]
    pub training_hud: bool,
    /// Forever Modern UI objective guidance. On for a Forever profile by the one-time Forever Enhancements move (see
    /// forever_enhancements_on); Config's own Default and classic leave it off.
    #[cfg(feature = "forever")]
    pub quest_tracker: bool,
    /// Ours: the Forever Enhancements options' one-time default-on move has run (migrate_forever_enhancements). Absent from
    /// a config saved before it, so that config gets the move once.
    #[cfg(feature = "forever")]
    #[serde(default)]
    pub forever_enhancements_on: bool,
    /// Forever Modern UI: the Compact HUD (smaller margins, narrower tracker and chat, per-panel folds; Off restores the
    /// full layout). On for a Forever profile by the one-time move (see migrate_forever_compact).
    #[cfg(feature = "forever")]
    #[serde(default)]
    pub forever_compact_hud: bool,
    /// Ours: the Compact HUD's one-time default-on move has run. Absent from a config saved before it.
    #[cfg(feature = "forever")]
    #[serde(default)]
    pub forever_compact_on: bool,
    /// Opt-in movement controls; saving this never saves a running navigation leg.
    #[cfg(feature = "forever")]
    pub auto_navigation: bool,
    /// Forever Modern: equip clearly better new pieces automatically (never hands, never in combat); off by default.
    #[cfg(feature = "forever")]
    pub auto_equip_upgrades: bool,
    /// Forever: monster names coloured by how tough they are for us, EverQuest-style, from the server's /check answer
    /// (client/con.rs). On by default; a profile without the key gets it too.
    #[cfg(feature = "forever")]
    #[serde(default = "default_on")]
    pub con_colors: bool,
    /// Forever: a targeted hostile monster not read yet gets one /check by itself (client/con.rs, at most one per
    /// 1.5 s). On by default, with con_colors.
    #[cfg(feature = "forever")]
    #[serde(default = "default_on")]
    pub auto_check_on_target: bool,
    /// Forever: plan Auto Navigation routes on the locally baked navigation mesh (client/navgrid). On for a profile that
    /// never saved the key (the default); a saved choice is kept. Never active in the regular FFXI client
    /// (`nav_mesh_on`).
    #[cfg(feature = "forever")]
    pub nav_mesh: bool,
    /// Forever Modern controls: bare J opens the Journal (a key the player assigned elsewhere always wins).
    #[cfg(feature = "forever")]
    pub journal_key: bool,
    /// Forever Crafting Log: recipe ids starred as favourites.
    #[cfg(feature = "forever")]
    pub recipe_favorites: Vec<u32>,
    /// Forever Modern controls: the Crafting Log key as a DIK code (U by default, 0 = off).
    #[cfg(feature = "forever")]
    pub recipe_log_key: u8,
    /// Original environment art preview, applied to audited scenery at next zone load.
    #[cfg(feature = "forever")]
    pub environment_textures: bool,
    /// Separate opt-in: original Woods residential masonry at next zone load.
    #[cfg(feature = "forever")]
    pub woods_textures: bool,
    /// Forever only: server-confirmed available quest books; independent of the tracker HUD.
    #[cfg(feature = "forever")]
    pub quest_giver_markers: bool,
    /// Ours (client/rotation.rs): the one-button rotation presses itself while engaged; each session starts so, and
    /// /autocast on | off flips only the session.
    pub rotation_autocast: bool,
    /// Ours: a "Solo" label on the solo-mode icon (client/buffs.rs).
    pub solo_badge: bool,
    /// Ours: the modern UI's job panel under the minimap for jobs without Wide Scan (client/modern/jobpanel.rs).
    pub job_panel: bool,
    /// Ours: the modern UI's EXP bar in the party window, on FFXI Forever (client/expbar.rs). The Equipment window's bar is always there.
    pub exp_bar: bool,
    /// Ours: the classic UI only, the modern one not offered (the regular FFXI launcher entry sets it; FFXI Forever
    /// keeps the modern UI). Read-only for the client: it never writes it back changed.
    pub classic_only: bool,
    /// Ours: FFXI Forever's one-time move to the modern UI has run (the owner: modern by default, his own install too;
    /// later choices stay). A config without the key has not.
    #[serde(default)]
    pub modern_migrated: bool,
    /// Ours: FFXI Forever's one-time move to Enhanced graphics by default has run (the owner, 2026-11-08: "ship these
    /// default on"). A profile still on the untouched original graphics takes Enhanced once; later choices, Original
    /// included, stay. A config without the key has not.
    #[cfg(feature = "forever")]
    #[serde(default)]
    pub graphics_default_on: bool,
    /// Ours: the graphics were picked from the GPU and the screen once (the first run; never again after).
    #[serde(default)]
    pub graphics_detected: bool,
    /// Ours: the suggestions' and the rotation's thresholds (client/suggest.rs Tuning).
    pub rotation_adaptive: bool,
    pub rotation_cure_pct: u32,
    pub rotation_convert_pct: u32,
    pub rotation_renew_secs: u32,
    pub minimap_unowned: bool,
    /// Extra (the skillchain add-ons): in a party, the chains it can make and the open one on our target.
    pub skillchain_helper: bool,
    /// Extra (the minimap add-ons): the minimap, its place (0 top right, 1 top left), size (0..2) and the map pixels
    /// across (the wheel's zoom).
    pub minimap: bool,
    /// Ours: the modern UI's minimap (on unless turned off; the classic UI keeps `minimap`, an Extra off by default), and
    /// whether the minimap shows a zone's map without its key item (the map window keeps retail's rule).
    pub modern_minimap: bool,
    /// The minimap (the bundled Minimap mod's window): its classic place (0 top right, 1 top left), size (0..2) and
    /// the map pixels across (the wheel's zoom).
    pub minimap_dock: u32,
    pub minimap_size: u32,
    pub minimap_span: f32,
    /// Not retail (client/gamepad.rs): gamepad input on; the sticks' deadzone as a share of their travel (0..0.8) and
    /// the right stick's camera speed as a multiple of the keys'.
    pub gamepad: bool,
    pub gamepad_deadzone: f32,
    pub gamepad_camera_speed: f32,
    /// Not retail (client/controls.rs): the keyboard and mouse scheme. 0 Original (retail's keyboard as the game
    /// Config's Keyboard type sets it), 1 FFXI WASD (retail's keyboard type 2), 2 Modern (WASD and the mouse as in
    /// WoW, the default). `controls_ad_turn`: under Modern, A / D turn the character and Q / E strafe (default: A / D
    /// strafe and Q / E turn).
    pub controls: u32,
    pub controls_ad_turn: bool,
    /// Forever mouse look: rightward motion turns right unless explicitly inverted.
    #[cfg(feature = "forever")]
    pub invert_horizontal_mouse: bool,
    /// How opaque the map overlay's map is, 0..1: the Map Overlay Opacity row in Settings is its only writer (native
    /// Extras; the standard Map Overlay mod is a built-in Extra and is never started, so it sets no opacity). The HUD
    /// maps take it when no alpha is given, and the minimap's marker shades by it (the two markers alike).
    pub map_overlay_opacity: f32,
    /// Ours: resting's ticks as learnt per server (by its search address, "ip-port"; client/resttick.rs).
    pub rest_models: std::collections::BTreeMap<String, RestModel>,
    /// The drawn cursor's size (config 0021 off) in percent of retail's pixels on screen; 0 = auto (grows with the
    /// window height from 720). Not retail: it was sized for 640 x 480. The .ani cursors keep their own size.
    pub cursor_size: u32,
    /// A key press hides the mouse cursor until the mouse moves (retail, FUN_10125f70(0)); off keeps it shown (ours).
    pub keys_hide_cursor: bool,
    /// Ours (user, 2026-11-02): Mouse/Cam.'s Camera Keys, the keys' X / Y sense apart from the mouse: 0 Same (the
    /// camera rows, keys and mouse alike, as retail), 1 normal, 2 inverted.
    pub cam_keys_x: u32,
    pub cam_keys_y: u32,
    /// Ours (user, 2026-10-25): containers whose sort window's Auto stays on, bit = container: a new item there sorts
    /// it again. Retail's Auto sorts once; Manual turns ours off.
    pub auto_sort: u32,
    /// Ours: a new item merges into the container's partial stacks (c2s 0x03A, the server's own merge) without
    /// re-sorting it.
    pub auto_combine: bool,
    /// The "FINAL FANTASY XI Config" tool's values by registry name (HKLM\...\FinalFantasyXI\0000..0043,
    /// read at start-up by FUN_10016c30); missing ones take TOOL_DEFAULTS.
    pub tool: std::collections::BTreeMap<String, u32>,
    /// Projection aspect from the config ratio (0x43 / 0xab, FUN_100147a0) instead of the window's. Not retail's
    /// default: retail always used the ratio.
    pub retail_aspect: bool,
    /// The monitor full screen and borderless go on, by its name (empty = the first). Not retail: Wayland can't
    /// tell which monitor is the main one.
    pub monitor: String,
    /// The graphics adapter by name (empty = the high-performance one); read at start-up. Not retail.
    pub gpu: String,
    /// Set once a software adapter (WARP, llvmpipe) was first seen and the background was halved for it. Not retail.
    pub software_gpu_seen: bool,
    /// Ours (not retail): optional post-processing over the 3D scene, all off by default (render/post.rs).
    pub graphics: GraphicsCfg,
    /// FFXiMain's own options by config id (the in-game and lobby Config menus).
    pub game: crate::gamecfg::GameConfig,
    /// DAT overlays (not retail; dat::overlay): folders under mods_dir(), highest priority first.
    pub mods: Vec<ModRef>,
    /// Not retail (docs/modern-ui.md): the modern UI layer instead of the retail HUD windows it replaces, its panels'
    /// places by character, then panel, and the chat window's font size in points.
    pub modern_ui: bool,
    /// The modern layer's size in percent of its 1080-line design.
    pub modern_scale: u32,
    /// How opaque the modern windows' backgrounds are (0.1 ..= 1).
    pub modern_opacity: f32,
    pub modern_places: std::collections::BTreeMap<String, ModernLayout>,
    pub modern_chat_font: f32,
    /// The modern edge windows folded to their tabs, by panel (the minimap, Wide Scan).
    pub modern_folded: std::collections::BTreeMap<String, bool>,
    /// The Wide Scan flyout's kinds shown: 2 NPCs, 4 monsters (players, 1, always: LSB sends none).
    pub modern_scan_kinds: u8,
    /// The modern chat's All tab shows the battle lines too (the Battle tab has them alone).
    pub modern_battle_in_all: bool,
    /// The modern chat merges an action's lines that differ only in their number (BattleMod's condensing).
    pub modern_condense: bool,
    /// The modern chat's tabs hidden by its settings, by name (All always shows).
    pub modern_chat_hidden: Vec<String>,
    /// The list search's recent picks by character, newest first (listsearch.rs).
    pub modern_recent: std::collections::BTreeMap<String, Vec<String>>,
    /// The modern UI's mode presets (client/modern/modes.rs).
    pub modern_modes: ModernModes,
    /// Favourites by character, in the strip's order (client/favs.rs), and whether the strip shows (/favs).
    pub modern_favourites: std::collections::BTreeMap<String, Vec<String>>,
    pub modern_fav_strip: bool,
    /// How fast a mouse drag turns the camera, in % of the old fixed speed (ours; the controller is not touched).
    pub mouse_camera_speed: u32,
    /// Read the pad while the window is in the background (padmode000's [4], DISCL_BACKGROUND).
    pub gamepad_background: bool,
    /// The button settings in retail's padsin000 format (FFXiPadConfig's registry value: 32 numbers, XInput button
    /// numbers or axis codes 32 + n); empty = gamepad::DEFAULT_PADSIN.
    pub gamepad_padsin: String,
    /// padexsin000, the map used while slot 34 is held; empty = gamepad::DEFAULT_PADEXSIN.
    pub gamepad_padexsin: String,
    /// LAN voice chat (ours, hidden: Ctrl+Alt+Shift+V).
    pub voice: crate::voice::Settings,
    /// Fields a newer client wrote, kept and saved back as they are: an older client drops nothing it doesn't know.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

/// The modern UI's mode presets: per mode (Exploration, Combat, Management) the HUD panels it hides, the panels shown
/// in every mode, and whether panels fade in and out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModernModes {
    pub hidden: [Vec<String>; 3],
    pub pinned: Vec<String>,
    pub fades: bool,
}

impl Default for ModernModes {
    fn default() -> Self {
        // Combat hides the location panel (user, 2026-11-01) and the training regime; the skillchain helper only in
        // combat.
        let v = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect();
        Self {
            hidden: [v(&["skillchains"]), v(&["where", "regime"]), v(&["skillchains"])],
            pinned: Vec::new(),
            fades: true,
        }
    }
}

/// A modern UI panel's place: its left and top (or, for the panels that grow upward, its bottom `b`) as fractions of the
/// screen, so it keeps its place whatever the Interface Size or the resolution; for the chat window its room in points
/// (0: the default); or docked on another panel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModernPlace {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub b: f32,
    pub dock: Option<ModernDock>,
    /// An edge window let go away from the edges: it stands at x / y with its whole frame.
    pub float: bool,
    /// An edge window held by the top (1) or the bottom (2) edge, x its left; 0 the left or right edge by x's sign.
    pub edge: u8,
}

/// A panel sitting on another: `gap` points between its bottom and the other's top, `dx` from the other's left, or
/// from its right (`right`) for one placed from the right edge, whose left moves as it folds or grows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModernDock {
    pub to: String,
    pub gap: f32,
    pub dx: f32,
    #[serde(default)]
    pub right: bool,
}

/// A character's panel places by panel id.
pub type ModernLayout = std::collections::BTreeMap<String, ModernPlace>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModRef {
    pub name: String,
    pub enabled: bool,
}

/// Where mod folders live.
pub fn mods_dir() -> PathBuf {
    // VANA_MODS_DIR: another mods folder for test runs.
    std::env::var_os("VANA_MODS_DIR").map_or_else(|| data_dir().join("mods"), PathBuf::from)
}

/// The enabled mods' folders, highest priority first (none with FFXI_NO_MODS set).
pub fn enabled_mod_dirs() -> Vec<PathBuf> {
    if std::env::var_os("FFXI_NO_MODS").is_some() {
        return Vec::new();
    }
    Config::load().mods.iter().filter(|m| m.enabled).map(|m| mods_dir().join(&m.name)).filter(|d| d.is_dir()).collect()
}

/// Merged overlay DATs (rebuilt when a source changes).
pub fn merged_dir() -> PathBuf {
    app_dir(dirs::cache_dir()).join("merged")
}

/// Navigation-mesh bakes (client/navgrid): local only, regenerable from the user's own DAT, never packaged.
#[cfg(feature = "forever")]
pub fn navgrid_dir() -> PathBuf {
    app_dir(dirs::cache_dir()).join("navgrid")
}

/// The mod catalogue's last list and its downloads.
pub fn catalogue_dir() -> PathBuf {
    app_dir(dirs::cache_dir()).join("catalogue")
}

/// Config-tool values with our defaults (FFXiMain's own default in the comment where it differs).
pub const TOOL_DEFAULTS: &[(&str, u32)] = &[
    ("0000", 1),    // MIP mapping (FFXiMain 0)
    ("0001", 1280), // window width (FFXiMain 640, the tool 800)
    ("0002", 800),  // window height (480 / 600)
    ("0003", 0),    // background width, 0 = the window's (FFXiMain 512, the tool 800)
    ("0004", 0),    // background height (512 / 600)
    ("0007", 1),    // sound
    ("0011", 2),    // environment animation (FFXiMain 1)
    ("0017", 1),    // bump mapping (FFXiMain 0)
    ("0018", 1),    // texture compression
    ("0019", 1),    // map compression
    ("0021", 0),    // hardware mouse
    ("0022", 0),    // opening movie (ours: skipped unless asked for; retail 1)
    ("0023", 0),    // simplified character creation
    ("0028", 0),    // gamma base 0.0
    ("0029", 12),   // sound effect channels
    ("0030", 0),    // 3D LCD mode
    ("0034", 1),    // window mode (FFXiMain 0 full screen)
    ("0035", 1),    // sound in the background (FFXiMain 0)
    ("0036", 0),    // font quality
    ("0037", 0),    // UI width, 0 = the window's
    ("0038", 0),    // UI height
    ("0040", 0),    // graphics stabilization
    ("0043", 0),    // screenshots at screen size
];

/// Choices for `frame_limit` (0 = unlimited).
pub const FRAME_LIMITS: [u32; 14] = [30, 60, 75, 90, 100, 120, 144, 165, 180, 200, 240, 300, 360, 0];

impl Default for Config {
    fn default() -> Self {
        Self {
            #[cfg(feature = "forever")]
            character_ui: Default::default(),
            save_blocked: false,
            game_dir: data_dir().join("game"),
            installed: false,
            server: "127.0.0.1".into(),
            username: String::new(),
            pol_id: String::new(),
            logins: Vec::new(),
            remember_password: false,
            saved_password: String::new(),
            auth_port: 54231,
            data_port: 54230,
            view_port: 54001,
            profile_port: 51220,
            irc_port: 51240,
            named_servers: Vec::new(),
            social_named_only: false,
            loader_version: String::new(),
            client_version: "30261006_0".into(),
            audio_device: String::new(),
            frame_limit: 60,
            reduce_motion: true,
            anisotropy: 16,
            antialias: 0,
            grading: 0,
            draw_distance: 0,
            send_interval: 400,
            party_tp: true,
            #[cfg(feature = "forever")]
            forever_map: Default::default(),
            window_opacity: Default::default(),
            auto_advance: 0,
            map_overlay: false,
            synth_amount: false,
            stamp_all_lines: false,
            target_distance: false,
            target_hpp: false,
            earth_clock: false,
            era_lock: true,
            ui_scale: UI_SCALE_AUTO,
            map_host: None,
            expansions_seen: Default::default(),
            gm_window: false,
            notes_seen: env!("CARGO_PKG_VERSION").into(),
            lamp_lights: true,
            actor_shadows: 0,
            shadow_map: 2048,
            ssao: false,
            hide_trust_speech: false,
            hide_ready_lines: false,
            short_battle: false,
            battle_name_colours: false,
            timers: false,
            timers_dock: 0,
            timers_out: true,
            timers_folded: 0,
            regime: false,
            skillchain_helper: false,
            rotation_autocast: true,
            regime_out: true,
            #[cfg(feature = "forever")]
            training_hud: false,
            #[cfg(feature = "forever")]
            quest_tracker: false,
            #[cfg(feature = "forever")]
            forever_enhancements_on: false,
            #[cfg(feature = "forever")]
            forever_compact_hud: false,
            #[cfg(feature = "forever")]
            forever_compact_on: false,
            #[cfg(feature = "forever")]
            auto_navigation: false,
            #[cfg(feature = "forever")]
            auto_equip_upgrades: false,
            #[cfg(feature = "forever")]
            con_colors: true,
            #[cfg(feature = "forever")]
            auto_check_on_target: true,
            #[cfg(feature = "forever")]
            nav_mesh: true,
            #[cfg(feature = "forever")]
            journal_key: true,
            #[cfg(feature = "forever")]
            recipe_favorites: Vec::new(),
            #[cfg(feature = "forever")]
            recipe_log_key: 0x16,
            #[cfg(feature = "forever")]
            environment_textures: false,
            #[cfg(feature = "forever")]
            woods_textures: false,
            #[cfg(feature = "forever")]
            quest_giver_markers: true,
            solo_badge: true,
            job_panel: true,
            exp_bar: true,
            classic_only: false,
            modern_migrated: true,
            #[cfg(feature = "forever")]
            graphics_default_on: false,
            graphics_detected: false,
            rotation_adaptive: true,
            rotation_cure_pct: 75,
            rotation_convert_pct: 25,
            rotation_renew_secs: 30,
            minimap_unowned: true,
            minimap: false,
            modern_minimap: true,
            minimap_dock: 0,
            minimap_size: 1,
            minimap_span: 160.0,
            gamepad: true,
            gamepad_deadzone: crate::client::gamepad::DEFAULT_DEADZONE,
            gamepad_camera_speed: crate::client::gamepad::DEFAULT_CAMERA_SPEED,
            controls: 2,
            controls_ad_turn: false,
            #[cfg(feature = "forever")]
            invert_horizontal_mouse: false,
            map_overlay_opacity: 0.6,
            rest_models: Default::default(),
            cursor_size: 0,
            keys_hide_cursor: true,
            cam_keys_x: 0,
            cam_keys_y: 0,
            auto_sort: AUTO_SORT_BAGS,
            auto_combine: true,
            mods: Vec::new(),
            modern_ui: true,
            modern_scale: 60,
            modern_opacity: 0.85,
            modern_places: Default::default(),
            modern_chat_font: 15.0,
            modern_folded: Default::default(),
            modern_scan_kinds: 7,
            modern_battle_in_all: true,
            modern_condense: true,
            modern_chat_hidden: Vec::new(),
            modern_recent: Default::default(),
            modern_modes: Default::default(),
            modern_favourites: Default::default(),
            modern_fav_strip: true,
            mouse_camera_speed: 100,
            gamepad_background: false,
            gamepad_padsin: String::new(),
            gamepad_padexsin: String::new(),
            voice: Default::default(),
            unknown: Default::default(),
            tool: Default::default(),
            retail_aspect: false,
            monitor: String::new(),
            gpu: String::new(),
            software_gpu_seen: false,
            graphics: Default::default(),
            game: Default::default(),
        }
    }
}

impl Config {
    /// The cursor's on-screen scale for a window `height` pixels high.
    pub fn cursor_scale(&self, height: u32) -> f32 {
        match self.cursor_size {
            0 => ((height as f32 / 720.0) * 2.0).round().max(2.0) / 2.0,
            n => n as f32 / 100.0,
        }
    }

    fn load_file(path: &std::path::Path) -> (Self, Option<Vec<u8>>) {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(c) => (c, Some(bytes)),
                Err(e) => {
                    // Never replace an older recovery copy or expose saved login material.
                    let backup = (|| -> std::io::Result<PathBuf> {
                        use std::io::Write;
                        for i in 0..1000 {
                            let aside =
                                path.with_extension(if i == 0 { "json.bad".into() } else { format!("json.bad.{i}") });
                            let mut options = std::fs::OpenOptions::new();
                            options.write(true).create_new(true);
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::OpenOptionsExt;
                                options.mode(0o600);
                            }
                            match options.open(&aside) {
                                Ok(mut file) => {
                                    file.write_all(&bytes)?;
                                    file.sync_all()?;
                                    return Ok(aside);
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                                Err(e) => return Err(e),
                            }
                        }
                        Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "config backup slots exhausted"))
                    })();
                    let mut c = Self::default();
                    match backup {
                        Ok(aside) => log::error!(
                            "{}: invalid config at {}:{}; preserved in {}; defaults loaded",
                            path.display(),
                            e.line(),
                            e.column(),
                            aside.display()
                        ),
                        Err(error) => {
                            c.save_blocked = true;
                            log::error!(
                                "{}: invalid config at {}:{}; could not preserve config ({error}); saving is disabled",
                                path.display(),
                                e.line(),
                                e.column()
                            );
                        }
                    }
                    (c, Some(bytes))
                }
            },
            Err(e) => {
                let mut c = Self::default();
                if e.kind() != std::io::ErrorKind::NotFound {
                    c.save_blocked = true;
                    log::error!("{}: {e}; saving is disabled", path.display());
                }
                (c, None)
            }
        }
    }

    pub fn load() -> Self {
        let path = config_path();
        let (mut c, bytes) = Self::load_file(&path);
        // Read once: the keys as saved, before any save below writes them back.
        let raw = bytes.as_ref().and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok());
        if let Some(v) = raw.as_ref() {
            c.controls = legacy_controls(v, c.controls);
            #[cfg(feature = "forever")]
            migrate_enhanced_retune(v, &mut c.graphics);
        }
        #[cfg(feature = "forever")]
        if migrate_forever_enhancements(&mut c) {
            // Saved at once, so the marker is never left for a later save to write as off first.
            log::info!("config: FFXI Forever Enhancements turned on by default (once)");
            if let Err(e) = c.save() {
                log::warn!("saving the config: {e}");
            }
        }
        #[cfg(feature = "forever")]
        if migrate_forever_compact(&mut c) {
            log::info!("config: Compact HUD turned on by default (once)");
            if let Err(e) = c.save() {
                log::warn!("saving the config: {e}");
            }
        }
        // A config from before the lists: its one login starts them.
        if c.logins.is_empty() && !c.server.trim().is_empty() && !c.username.trim().is_empty() {
            c.remember_login();
        }
        c.gamepad_deadzone = c.gamepad_deadzone.clamp(0.0, crate::client::gamepad::MAX_DEADZONE);
        c.gamepad_camera_speed = c.gamepad_camera_speed.clamp(0.1, 5.0);
        // 3 is Forever's scheme; off Forever it is used as Modern, and the choice stays saved (Config::control_scheme).
        c.controls = c.controls.min(3);
        if c.classic_only {
            c.modern_ui = false;
        }
        // The regular FFXI client keeps retail's manual sorting unless chosen.
        if c.classic_only
            && bytes
                .as_ref()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
                .is_some_and(|v| v.get("auto_combine").is_none())
        {
            (c.auto_sort, c.auto_combine) = (0, false);
        }
        // A profile saved by a build with more upscalers (a value past this build's highest) keeps its pick as far as this build goes: FSR 2.
        #[cfg(feature = "forever")]
        normalize_upscaler(&mut c.graphics);
        #[cfg(feature = "forever")]
        if !c.graphics_default_on {
            migrate_graphics_default_on(&mut c);
            // Saved at once (as the modern UI's move is), so the move is not left to the next save and a later
            // Original choice is never taken for an untouched profile.
            if c.graphics_default_on {
                log::info!("config: FFXI Forever's Enhanced graphics turned on by default (once)");
                if let Err(e) = c.save() {
                    log::warn!("saving the config: {e}");
                }
            }
        }
        if !c.modern_migrated && !c.classic_only && c.is_forever() {
            (c.modern_ui, c.modern_migrated) = (true, true);
            // The owner: sorting and combining on by default too.
            (c.auto_sort, c.auto_combine) = (AUTO_SORT_BAGS, true);
            log::info!("config: FFXI Forever's modern UI turned on (once)");
            if let Err(e) = c.save() {
                log::warn!("saving the config: {e}");
            }
        }
        c
    }

    /// Preview art requires both the Forever profile and its Enhanced renderer.
    #[cfg(feature = "forever")]
    pub fn environment_textures_enabled(&self) -> bool {
        self.is_forever() && self.graphics.enhanced && self.environment_textures
    }

    #[cfg(feature = "forever")]
    pub fn environment_textures_enabled_for(&self, zone: u32) -> bool {
        self.is_forever()
            && self.graphics.enhanced
            && match zone {
                242 => self.environment_textures,
                241 => self.woods_textures,
                _ => false,
            }
    }

    /// The server is FFXI Forever: the named server picked is Forever's (its launcher entry names it so).
    /// The navigation mesh is in use: Forever only, and not in a regular-FFXI (classic_only) profile.
    #[cfg(feature = "forever")]
    pub fn nav_mesh_on(&self) -> bool {
        self.nav_mesh && !self.classic_only
    }
    pub fn is_forever(&self) -> bool {
        #[cfg(feature = "forever")]
        {
            self.named_servers
                .iter()
                .any(|n| same_server(&n.name, &self.server) && n.name.to_ascii_lowercase().contains("forever"))
        }
        #[cfg(not(feature = "forever"))]
        {
            false
        }
    }

    /// The Controls scheme in force (client/controls.rs): the saved choice, with Forever's own only on a Forever
    /// server of the Forever flavour.
    pub fn control_scheme(&self) -> crate::client::controls::Scheme {
        crate::client::controls::Scheme::resolve(self.controls, self.is_forever() && !self.classic_only)
    }

    /// Modern UI is a Forever feature. Evaluate at use time too: the login screen can switch servers without
    /// reloading the config, and older regular-client profiles may have saved modern_ui: true.
    pub fn modern_enabled(&self) -> bool {
        self.modern_ui && !self.classic_only && self.is_forever()
    }

    /// A copy to connect with: when `server` names one of `named_servers`, its address and ports replace the
    /// config's. Saved logins and passwords stay keyed by the name.
    pub fn resolved(&self) -> Self {
        let mut c = self.clone();
        if let Some(n) = self.named_servers.iter().find(|n| same_server(&n.name, &self.server)) {
            c.server = n.host.trim().to_string();
            c.auth_port = n.auth_port.unwrap_or(c.auth_port);
            c.data_port = n.data_port.unwrap_or(c.data_port);
            c.view_port = n.view_port.unwrap_or(c.view_port);
            c.profile_port = n.profile_port.unwrap_or(c.profile_port);
            c.irc_port = n.irc_port.unwrap_or(c.irc_port);
            if let Some(v) = &n.loader_version {
                c.loader_version = v.clone();
            }
        }
        c
    }

    /// The suggestions' thresholds, clamped to the settings' ranges.
    pub fn suggest_tuning(&self) -> crate::client::suggest::Tuning {
        crate::client::suggest::Tuning {
            adaptive: self.rotation_adaptive,
            cure_pct: self.rotation_cure_pct.clamp(20, 95),
            convert_pct: self.rotation_convert_pct.clamp(5, 60),
            renew_secs: self.rotation_renew_secs.clamp(5, 120),
        }
    }

    /// The loader version to report: `loader_version` as "major.minor.patch", or None for the client's own.
    pub fn loader_version(&self) -> Option<[u8; 3]> {
        let v: Vec<u8> = self.loader_version.trim().split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        v.try_into().ok()
    }

    /// Whether this login uses the server's profile service (friend list, presence, mail).
    pub fn social_enabled(&self) -> bool {
        let named = self.named_servers.iter().find(|n| same_server(&n.name, &self.server));
        named.and_then(|n| n.social).unwrap_or(!self.social_named_only)
    }

    /// The servers logged in to, most recent first, then the named servers not logged in to yet.
    pub fn servers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in self.logins.iter().map(|l| &l.server).chain(self.named_servers.iter().map(|n| &n.name)) {
            if !l.trim().is_empty() && !out.iter().any(|s| same_server(s, l)) {
                out.push(l.clone());
            }
        }
        out
    }

    /// The accounts logged in with on a server, most recent first.
    pub fn accounts(&self, server: &str) -> Vec<String> {
        self.logins.iter().filter(|l| same_server(&l.server, server)).map(|l| l.username.clone()).collect()
    }

    /// The saved login for a server and account; with no account, the server's most recent.
    pub fn saved_login(&self, server: &str, username: Option<&str>) -> Option<&SavedLogin> {
        self.logins.iter().find(|l| same_server(&l.server, server) && username.is_none_or(|u| l.username == u))
    }

    /// Makes a saved login the form's: its server, account, PlayOnline ID, Remember password and sealed passwords.
    pub fn use_login(&mut self, l: &SavedLogin) {
        self.server = l.server.clone();
        self.username = l.username.clone();
        self.pol_id = l.pol_id.clone();
        self.remember_password = l.remember_password;
        self.saved_password = l.saved_password.clone();
    }

    /// The form's login, first in the lists (after a login that worked, or a change to its Remember password).
    pub fn remember_login(&mut self) {
        let (server, username) = (self.server.trim().to_string(), self.username.trim().to_string());
        // A newer client's fields of this login stay with it.
        let unknown = self
            .logins
            .iter()
            .find(|o| same_server(&o.server, &server) && o.username == username)
            .map(|o| o.unknown.clone())
            .unwrap_or_default();
        let l = SavedLogin {
            server,
            username,
            pol_id: self.pol_id.clone(),
            remember_password: self.remember_password,
            saved_password: self.saved_password.clone(),
            unknown,
        };
        self.logins.retain(|o| !(same_server(&o.server, &l.server) && o.username == l.username));
        self.logins.insert(0, l);
    }

    /// Drops a saved login (with its password); with no account, every login of the server.
    pub fn forget_login(&mut self, server: &str, username: Option<&str>) {
        self.logins.retain(|l| !(same_server(&l.server, server) && username.is_none_or(|u| l.username == u)));
    }

    pub fn save(&self) -> Result<()> {
        anyhow::ensure!(
            !self.save_blocked,
            "config was unreadable or its recovery backup failed; original file preserved"
        );
        let path = config_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        #[cfg(feature = "forever")]
        let saved = self.character_ui.for_global_save(self)?;
        #[cfg(feature = "forever")]
        let data = serde_json::to_vec_pretty(&saved)?;
        #[cfg(not(feature = "forever"))]
        let data = serde_json::to_vec_pretty(self)?;
        write_atomic(&path, &data)?;
        Ok(())
    }

    pub fn ffxi_dir(&self) -> PathBuf {
        self.game_dir.join("FINAL FANTASY XI")
    }

    /// A Config-tool value (registry name "0000".."0043").
    pub fn tool(&self, name: &str) -> u32 {
        self.tool.get(name).copied().or_else(|| TOOL_DEFAULTS.iter().find(|d| d.0 == name).map(|d| d.1)).unwrap_or(0)
    }

    /// Sets a Config-tool value; the default is not stored.
    pub fn set_tool(&mut self, name: &str, v: u32) {
        if TOOL_DEFAULTS.iter().any(|d| d.0 == name && d.1 == v) {
            self.tool.remove(name);
        } else {
            self.tool.insert(name.to_string(), v);
        }
    }

    /// Window size (0001 / 0002).
    pub fn window_size(&self) -> [u32; 2] {
        [self.tool("0001"), self.tool("0002")].map(|v| v.clamp(320, 16384))
    }

    /// Window mode 0034: 0 full screen, 1 windowed, 2 borderless at the window size, 3 borderless at the
    /// desktop size.
    pub fn window_mode(&self) -> u32 {
        self.tool("0034").min(3)
    }

    /// The scene's size for a window of `window` pixels (0003 / 0004; 0 = the window's, not retail).
    pub fn background(&self, window: [u32; 2]) -> [u32; 2] {
        match [self.tool("0003"), self.tool("0004")] {
            [w, h] if w > 0 && h > 0 => [w.min(8192), h.min(8192)],
            _ => window,
        }
    }

    /// The UI layout size (0037 / 0038, 0 = the window's): laid out there and stretched to the window.
    pub fn ui_size(&self, window: [u32; 2]) -> [u32; 2] {
        // Ours: UI Scale lays the UI out at the window's size over the scale, so it keeps the window's shape.
        if let Some(k) = ui_scale_factor(self.ui_scale, window) {
            return [
                ((window[0] as f32 / k).round() as u32).max(320),
                ((window[1] as f32 / k).round() as u32).max(240),
            ];
        }
        match [self.tool("0037"), self.tool("0038")] {
            [w, h] if w >= 320 && h >= 240 => [w.min(window[0].max(320) * 4), h.min(window[1].max(240) * 4)],
            _ => window,
        }
    }

    /// Gamma base 0028 (an f32 in the DWORD, -1..1).
    pub fn gamma_base(&self) -> f32 {
        f32::from_bits(self.tool("0028")).clamp(-1.0, 1.0)
    }

    /// The requested limit, including the test override, bounded by the comfort option.
    pub fn frame_limit(&self) -> u32 {
        self.motion_frame_limit(
            std::env::var("FFXI_FRAME_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(self.frame_limit),
        )
    }

    pub fn motion_frame_limit(&self, requested: u32) -> u32 {
        if self.reduce_motion && (requested == 0 || requested > 60) { 60 } else { requested }
    }

    /// Shared image quality never enables Forever's lighting or colour treatment, even from an old config.
    pub fn graphics_effects(&self) -> GraphicsCfg {
        if !self.is_forever() {
            return GraphicsCfg { fxaa: self.graphics.fxaa, sharpen: self.graphics.sharpen, ..Default::default() };
        }
        self.graphics.clone()
    }

    pub fn lamp_lights_enabled(&self) -> bool {
        self.is_forever() && self.lamp_lights
    }

    /// Ours: upstream's optional real character shadows, ambient occlusion and colour grading are Enhanced graphics, so
    /// FFXI Forever's only (the classic flavour and a regular server keep retail's shadows and look, as with lamp lights).
    pub fn actor_shadows_enabled(&self) -> u32 {
        if self.is_forever() { self.actor_shadows } else { 0 }
    }
    /// The basic ambient occlusion (render/ssao.rs). It yields to the post pass's own (`graphics.ao`, render/postdepth.rs)
    /// when that is on, so the scenery is never darkened twice; this is the one place that decides it.
    pub fn ssao_enabled(&self) -> bool {
        self.is_forever() && self.ssao && !self.graphics.ao_on()
    }
    pub fn grading_enabled(&self) -> u32 {
        if self.is_forever() { self.grading } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The navigation mesh is on for a profile that never saved the key, a saved false stays false (also through a
    /// save and load), and it is never in use in a regular-FFXI (classic_only) profile.
    #[cfg(feature = "forever")]
    #[test]
    fn nav_mesh_defaults_on_for_unsaved_profiles_and_keeps_a_saved_choice() {
        let unsaved: Config = serde_json::from_str(r#"{"server":"FFXI Forever","modern_ui":true}"#).unwrap();
        assert!(unsaved.nav_mesh && unsaved.nav_mesh_on());
        assert!(Config::default().nav_mesh);
        let off: Config = serde_json::from_str(r#"{"server":"FFXI Forever","nav_mesh":false}"#).unwrap();
        assert!(!off.nav_mesh && !off.nav_mesh_on());
        let again: Config = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert!(!again.nav_mesh, "a saved false survives a save and load");
        let on: Config = serde_json::from_str(r#"{"server":"FFXI Forever","nav_mesh":true}"#).unwrap();
        assert!(on.nav_mesh_on());
        let classic: Config = serde_json::from_str(r#"{"classic_only":true}"#).unwrap();
        assert!(!classic.nav_mesh_on(), "never in the regular FFXI client's profile");
    }

    #[test]
    fn upstream_merge_config_recovery_preserves_each_copy_and_blocks_failed_backup() {
        let dir = std::env::temp_dir().join(format!(
            "vana-config-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let broken = b"{invalid fixture";
        std::fs::write(&path, broken).unwrap();
        std::fs::write(path.with_extension("json.bad"), b"older recovery").unwrap();
        let (c, _) = Config::load_file(&path);
        assert!(!c.save_blocked);
        assert_eq!(std::fs::read(&path).unwrap(), broken);
        assert_eq!(std::fs::read(path.with_extension("json.bad")).unwrap(), b"older recovery");
        assert_eq!(std::fs::read(path.with_extension("json.bad.1")).unwrap(), broken);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path.with_extension("json.bad.1")).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        for i in 2..1000 {
            std::fs::create_dir(path.with_extension(format!("json.bad.{i}"))).unwrap();
        }
        let (blocked, _) = Config::load_file(&path);
        assert!(blocked.save_blocked);
        assert!(blocked.save().is_err()); // exits before resolving or writing the real config path
        assert_eq!(std::fs::read(&path).unwrap(), broken);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shared_quality_rejects_forever_lighting_from_copied_config() {
        let mut c: Config = serde_json::from_str(r#"{"server":"regular","lamp_lights":true,"graphics":{"enhanced":true,"fxaa":true,"sharpen":0.3,"bloom":true,"tonemap":true,"ao":true,"fog":true,"saturation":1.5,"exposure":2.0}}"#).unwrap();
        for server in ["regular", "FFXI Forever"] {
            c.server = server.into();
            c.named_servers = vec![NamedServer { name: "FFXI Forever".into(), ..Default::default() }];
            let g = c.graphics_effects();
            assert!(g.fxaa && g.sharpen == 0.3);
            if !c.is_forever() {
                assert!(!g.bloom && !g.tonemap && !g.ao && !g.fog);
                assert_eq!((g.exposure, g.saturation), (1.0, 1.0));
                assert_eq!(g.character_light_amount(), 0.0);
                assert!(!c.lamp_lights_enabled());
            } else {
                assert!(g.bloom && g.tonemap && c.lamp_lights_enabled());
            }
        }
        assert!(c.graphics.bloom && c.graphics.tonemap, "preserve stored preferences");
    }

    #[test]
    fn motion_comfort_caps_legacy_configs_without_losing_the_saved_choice() {
        for requested in [0, 30, 60, 144, 300] {
            let mut c: Config = serde_json::from_value(serde_json::json!({"frame_limit": requested})).unwrap();
            assert!(c.reduce_motion);
            assert_eq!(c.frame_limit, requested);
            assert_eq!(c.motion_frame_limit(requested), if requested == 30 { 30 } else { 60 });
            c.reduce_motion = false;
            let roundtrip: Config = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
            assert!(!roundtrip.reduce_motion);
            assert_eq!(roundtrip.motion_frame_limit(requested), requested);
        }
        assert_eq!(Config::default().frame_limit, 60);
    }

    #[test]
    fn modern_ui_requires_the_selected_forever_server() {
        let mut c: Config = serde_json::from_str(r#"{"modern_ui":true,"server":"regular"}"#).unwrap();
        assert!(!c.modern_enabled());
        c.named_servers = vec![
            NamedServer { name: "regular".into(), ..Default::default() },
            NamedServer { name: "FFXI Forever".into(), ..Default::default() },
        ];
        assert!(!c.modern_enabled());
        c.server = "FFXI Forever".into();
        assert_eq!(c.modern_enabled(), cfg!(feature = "forever"));
        c.classic_only = true;
        assert!(!c.modern_enabled());
        c.classic_only = false;
        c.modern_ui = false;
        assert!(!c.modern_enabled());
        c.modern_ui = true;
        // Switching in the login form takes effect without reloading the config.
        c.server = "regular".into();
        assert!(!c.modern_enabled());
        c.server = "unregistered Forever".into();
        assert!(!c.modern_enabled());
        c.server = "PlayOnline".into();
        assert!(!c.modern_enabled());
    }

    /// A config from before `graphics`, and one naming only fxaa, keep every other option off / neutral.
    #[test]
    fn graphics_defaults_are_off() {
        let d = Config::default().graphics;
        assert!(
            !d.active()
                && !d.fxaa
                && !d.bloom
                && !d.tonemap
                && d.sharpen == 0.0
                && d.saturation == 1.0
                && d.exposure == 1.0
        );
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.graphics, GraphicsCfg::default());
        let c: Config = serde_json::from_str(r#"{"graphics":{"fxaa":true}}"#).unwrap();
        assert!(c.graphics.fxaa && c.graphics.active() && !c.graphics.composite_on());
        assert_eq!(GraphicsCfg { fxaa: false, ..c.graphics }, GraphicsCfg::default());
        // Bloom at zero intensity, sharpen 0 and saturation 1 do nothing.
        assert!(!GraphicsCfg { bloom: true, bloom_intensity: 0.0, ..Default::default() }.active());
        assert!(GraphicsCfg { sharpen: 0.5, ..Default::default() }.composite_on());
        assert!(GraphicsCfg { saturation: 0.0, ..Default::default() }.composite_on());
        // The depth effects are off by default, count as the post pass being on, and do nothing at zero strength / density.
        assert!(!d.ao && !d.fog && !d.depth_on());
        assert!(
            GraphicsCfg { ao: true, ..Default::default() }.active()
                && GraphicsCfg { fog: true, ..Default::default() }.depth_on()
        );
        assert!(!GraphicsCfg { ao: true, ao_strength: 0.0, ..Default::default() }.active());
        assert!(!GraphicsCfg { fog: true, fog_density: 0.0, ..Default::default() }.active());
        let c: Config = serde_json::from_str(r#"{"graphics":{"ao":true}}"#).unwrap();
        assert!(c.graphics.ao && c.graphics.ao_strength == 1.0 && !c.graphics.fog);
    }

    /// The character light: an old config gets the default; it acts only with an Enhanced effect that darkens the scene on
    /// (retail and classic lighting, and an FXAA-only pass, stay as drawn), is clamped, and 0 is off.
    #[test]
    fn character_light_only_with_the_enhanced_effects() {
        let old: Config = serde_json::from_str(r#"{"graphics":{"ao":true}}"#).unwrap();
        assert_eq!(old.graphics.character_light, 0.75);
        assert!((old.graphics.character_light_amount() - 0.225).abs() < 1e-6);
        let d = GraphicsCfg::default();
        assert_eq!(d.character_light_amount(), 0.0);
        assert_eq!(
            GraphicsCfg { fxaa: true, bloom: true, sharpen: 1.0, ..Default::default() }.character_light_amount(),
            0.0
        );
        assert_eq!(GraphicsCfg { tonemap: true, ..Default::default() }.character_light_amount(), 0.75);
        // Occlusion or fog alone (the High preset) get a fraction: they darken characters far less than the tone curve.
        assert!((GraphicsCfg { fog: true, ..Default::default() }.character_light_amount() - 0.225).abs() < 1e-6);
        assert!(
            (GraphicsCfg { ao: true, tonemap: false, ..Default::default() }.character_light_amount() - 0.225).abs()
                < 1e-6
        );
        assert_eq!(GraphicsCfg { ao: true, tonemap: true, ..Default::default() }.character_light_amount(), 0.75);
        assert_eq!(GraphicsCfg { ao: true, ao_strength: 0.0, ..Default::default() }.character_light_amount(), 0.0);
        assert_eq!(
            GraphicsCfg { tonemap: true, character_light: 0.0, ..Default::default() }.character_light_amount(),
            0.0
        );
        assert_eq!(
            GraphicsCfg { tonemap: true, character_light: 9.0, ..Default::default() }.character_light_amount(),
            1.0
        );
        // It alone never turns the post pass on.
        assert!(!GraphicsCfg { character_light: 1.0, ..Default::default() }.active());
    }

    #[cfg(feature = "forever")]
    #[test]
    fn upscaler_is_off_by_default_and_forever_only() {
        let d = GraphicsCfg::default();
        assert_eq!((d.upscaler, d.upscale_quality, d.fsr_factor()), (0, 1, None));
        // Not a post-processing pass: it never switches the post pass or the Enhanced/Custom detection on.
        let on = GraphicsCfg { upscaler: 1, ..Default::default() };
        assert!(!on.active());
        assert_eq!(on.fsr_factor(), Some(1.5));
        for (q, k) in [(0, 1.3), (1, 1.5), (2, 1.7), (3, 2.0), (9, 2.0)] {
            assert_eq!(GraphicsCfg { upscaler: 1, upscale_quality: q, ..Default::default() }.fsr_factor(), Some(k));
        }
        // 2 = FSR 2: its own factor (the same modes), never FSR 1's; a value past this build's highest (a private profile's) is FSR 2 here too.
        let fsr2 = GraphicsCfg { upscaler: 2, upscale_quality: 3, ..Default::default() };
        assert_eq!((fsr2.fsr_factor(), fsr2.fsr2_factor(), fsr2.upscale_factor()), (None, Some(2.0), Some(2.0)));
        assert!(fsr2.clone().for_upscale().sharpen == 0.0);
        let private = GraphicsCfg { upscaler: UPSCALER_MAX + 1, upscale_quality: 1, ..Default::default() };
        assert_eq!((private.fsr_factor(), private.fsr2_factor()), (None, Some(1.5)));
        let mut loaded = private.clone();
        normalize_upscaler(&mut loaded);
        assert_eq!(loaded.upscaler, 2);
        let mut ok = GraphicsCfg { upscaler: 1, ..Default::default() };
        normalize_upscaler(&mut ok);
        assert_eq!(ok.upscaler, 1);
        // Old configs have neither field; an upscaler pick round-trips.
        let old: GraphicsCfg = serde_json::from_str(r#"{"fxaa":true}"#).unwrap();
        assert_eq!((old.upscaler, old.upscale_quality), (0, 1));
        let back: GraphicsCfg = serde_json::from_str(
            &serde_json::to_string(&GraphicsCfg { upscaler: 1, upscale_quality: 3, ..Default::default() }).unwrap(),
        )
        .unwrap();
        assert_eq!((back.upscaler, back.upscale_quality), (1, 3));
        // Under FSR, RCAS is the sharpening: the contrast-adaptive pass is dropped, everything else stays.
        let e = GraphicsCfg { upscaler: 1, ..GraphicsCfg::enhanced() };
        let fx = e.clone().for_upscale();
        assert_eq!(fx.sharpen, 0.0);
        assert_eq!(GraphicsCfg { sharpen: e.sharpen, ..fx.clone() }, e);
        assert_eq!(GraphicsCfg::enhanced().for_upscale(), GraphicsCfg::enhanced());
        // A config saved by .66/.67 (upscaler 1, FSR 1) still loads as FSR 1; Enhanced and the defaults leave the Upscaler at Standard.
        let saved: GraphicsCfg = serde_json::from_str(r#"{"upscaler":1,"upscale_quality":2}"#).unwrap();
        assert_eq!((saved.upscaler, saved.fsr_factor(), saved.fsr2_factor()), (1, Some(1.7), None));
        let whole: Config = serde_json::from_str(r#"{"graphics":{"upscaler":1}}"#).unwrap();
        assert_eq!((whole.graphics.upscaler, whole.graphics.upscale_quality), (1, 1));
        assert_eq!(GraphicsCfg::enhanced().upscaler, 0);
        assert_eq!(GraphicsCfg::default().upscaler, 0);
        // A regular server in a Forever build never upscales, whatever an old config says.
        let mut c = Config::default();
        c.graphics = on;
        c.server = "regular".into();
        assert_eq!(c.graphics_effects().fsr_factor(), None);
        c.graphics.upscaler = 2;
        assert_eq!(c.graphics_effects().upscale_factor(), None);
    }

    #[test]
    fn modern_controls_only_for_new_configs() {
        let saved: serde_json::Value = serde_json::json!({"modern_ui": false, "server": "x"});
        let launcher: serde_json::Value = serde_json::json!({"game_dir": "/g", "server": "x"});
        let chosen: serde_json::Value = serde_json::json!({"modern_ui": true, "controls": 1});
        assert_eq!(legacy_controls(&saved, 2), 0);
        assert_eq!(legacy_controls(&launcher, 2), 2);
        assert_eq!(legacy_controls(&chosen, 1), 1);
        let vana: serde_json::Value = serde_json::json!({"game_dir": "/g", "classic_only": true});
        assert_eq!(legacy_controls(&vana, 2), 0);
    }

    #[test]
    fn ui_scale_keeps_the_window_shape_and_auto_is_retail_up_to_1080() {
        let mut c = Config::default();
        assert_eq!(c.ui_scale, UI_SCALE_AUTO);
        assert_eq!(c.ui_size([1920, 1080]), [1920, 1080]);
        assert_eq!(c.ui_size([1280, 800]), [1280, 800]);
        assert_eq!(c.ui_size([3840, 2160]), [1920, 1080]);
        assert_eq!(c.ui_size([2560, 1440]), [1920, 1080]);
        c.ui_scale = 150;
        assert_eq!(c.ui_size([1920, 1080]), [1280, 720]);
        // Menu Size: retail's 0037 / 0038 (0: the window).
        c.ui_scale = 0;
        assert_eq!(c.ui_size([3840, 2160]), [3840, 2160]);
    }

    #[test]
    fn named_server_resolves_address_and_ports() {
        let mut c = Config { server: " Example ".into(), ..Default::default() };
        c.named_servers.push(NamedServer {
            name: "example".into(),
            host: "example.invalid".into(),
            auth_port: Some(1),
            view_port: Some(3),
            ..Default::default()
        });
        let r = c.resolved();
        assert_eq!((r.server.as_str(), r.auth_port, r.data_port, r.view_port), ("example.invalid", 1, c.data_port, 3));
        assert!(c.servers().iter().any(|s| s == "example"));
        // A plain address is left alone.
        c.server = "127.0.0.1".into();
        assert_eq!(c.resolved().server, "127.0.0.1");
    }

    #[test]
    fn loader_version_parses_and_follows_the_named_server() {
        let mut c = Config::default();
        assert_eq!(c.loader_version(), None);
        c.loader_version = "2.2.0".into();
        c.named_servers.push(NamedServer {
            name: "old".into(),
            host: "example.invalid".into(),
            loader_version: Some("2.1.2".into()),
            ..Default::default()
        });
        assert_eq!(c.loader_version(), Some([2, 2, 0]));
        c.server = "old".into();
        assert_eq!(c.resolved().loader_version(), Some([2, 1, 2]));
        c.loader_version = "2.x".into();
        c.server = "other".into();
        assert_eq!(c.resolved().loader_version(), None);
    }

    #[test]
    fn social_follows_the_named_server() {
        let mut c = Config::default();
        c.named_servers.push(NamedServer { name: "on".into(), social: Some(true), ..Default::default() });
        c.named_servers.push(NamedServer { name: "off".into(), social: Some(false), ..Default::default() });
        c.named_servers.push(NamedServer { name: "unset".into(), ..Default::default() });
        let on = |c: &Config, s: &str| Config { server: s.into(), ..c.clone() }.social_enabled();
        assert!(on(&c, "on") && !on(&c, "off") && on(&c, "unset") && on(&c, "10.0.0.1"));
        c.social_named_only = true;
        assert!(on(&c, "on") && !on(&c, "off") && !on(&c, "unset") && !on(&c, "10.0.0.1"));
    }
}

#[cfg(test)]
mod upstream_tests {
    use super::*;

    /// The first key or value of `a` that `b` lacks or changes (objects compared key by key, recursively).
    fn lost(a: &serde_json::Value, b: &serde_json::Value, at: &str) -> Option<String> {
        match (a, b) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => a.iter().find_map(|(k, v)| {
                b.get(k).map_or_else(|| Some(format!("{at}.{k} dropped")), |w| lost(v, w, &format!("{at}.{k}")))
            }),
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) if a.len() == b.len() => {
                a.iter().zip(b).enumerate().find_map(|(i, (v, w))| lost(v, w, &format!("{at}[{i}]")))
            }
            _ => (a != b).then(|| format!("{at}: {a} became {b}")),
        }
    }

    /// The config as save writes it, read back (to_value would widen an f32: 0.33 to 0.33000001...).
    fn written(c: &Config) -> serde_json::Value {
        serde_json::from_slice(&serde_json::to_vec_pretty(c).unwrap()).unwrap()
    }

    fn kept(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        lost(a, b, "").inspect(|l| eprintln!("lost: {l}")).is_none()
    }

    /// Forever's graphics start on Enhanced; an untouched original profile moves once; later choices stay.
    #[cfg(feature = "forever")]
    #[test]
    fn forever_graphics_default_on_once() {
        let forever = |g: GraphicsCfg, done: bool| Config {
            named_servers: vec![NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            server: "FFXI Forever".into(),
            graphics: g,
            graphics_default_on: done,
            ..Default::default()
        };
        // A new Forever config (the defaults) moves to Enhanced at load; so does an untouched original profile, once.
        let mut fresh = forever(Config::default().graphics, Config::default().graphics_default_on);
        migrate_graphics_default_on(&mut fresh);
        assert_eq!(fresh.graphics, GraphicsCfg::enhanced());
        let mut c = forever(GraphicsCfg::default(), false);
        migrate_graphics_default_on(&mut c);
        assert_eq!((c.graphics.clone(), c.graphics_default_on), (GraphicsCfg::enhanced(), true));
        // Original chosen after the move is kept.
        let mut c = forever(GraphicsCfg::default(), true);
        migrate_graphics_default_on(&mut c);
        assert_eq!(c.graphics, GraphicsCfg::default());
        // A changed (Custom) profile is kept, and the flag is set.
        let custom = GraphicsCfg { bloom: true, ..Default::default() };
        let mut c = forever(custom.clone(), false);
        migrate_graphics_default_on(&mut c);
        assert_eq!((c.graphics, c.graphics_default_on), (custom, true));
        // classic_only (regular FFXI) is never moved.
        let mut c = Config { classic_only: true, ..forever(GraphicsCfg::default(), false) };
        migrate_graphics_default_on(&mut c);
        assert_eq!(c.graphics, GraphicsCfg::default());
    }

    /// The Forever Enhancements options move on once per config (the marker), whatever the file says about them: every
    /// Forever save writes all four keys, so a saved off before the marker turns on once. After the move a saved off stays
    /// off through a save and reload. Classic (regular FFXI) and non-Forever servers are untouched and keep the marker
    /// unset, so a later Forever load moves them. Config's own Default stays off.
    #[cfg(feature = "forever")]
    #[test]
    fn compact_hud_moves_once_for_forever_and_keeps_later_choices() {
        let forever = |c: Config| Config {
            named_servers: vec![NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            server: "FFXI Forever".into(),
            ..c
        };
        let round_trip = |c: &Config| -> Config { serde_json::from_slice(&serde_json::to_vec(c).unwrap()).unwrap() };
        assert!(!Config::default().forever_compact_hud, "Config's Default stays off");
        let mut c = round_trip(&forever(Config::default()));
        assert!(migrate_forever_compact(&mut c));
        assert!(c.forever_compact_hud && c.forever_compact_on);
        // A later explicit Off is kept through a save and reload; the move does not run again.
        c.forever_compact_hud = false;
        let mut again = round_trip(&c);
        assert!(!migrate_forever_compact(&mut again));
        assert!(!again.forever_compact_hud);
        // An old config without either key has not had the move.
        let old: Config = serde_json::from_str("{}").unwrap();
        assert!(!old.forever_compact_on && !old.forever_compact_hud);
        // Not Forever, or regular FFXI: untouched, marker unset.
        let mut other = Config { server: "Other".into(), ..Default::default() };
        assert!(!migrate_forever_compact(&mut other) && !other.forever_compact_on);
        let mut regular = Config { classic_only: true, ..forever(Config::default()) };
        assert!(!migrate_forever_compact(&mut regular) && !regular.forever_compact_hud && !regular.forever_compact_on);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn forever_enhancements_move_once_and_keep_later_choices() {
        let forever = |c: Config| Config {
            named_servers: vec![NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            server: "FFXI Forever".into(),
            ..c
        };
        let four = |c: &Config| {
            (
                (c.quest_tracker, c.target_distance, c.target_hpp, c.earth_clock, c.training_hud),
                (c.auto_navigation, c.timers, c.regime, c.skillchain_helper, c.minimap),
            )
        };
        let all_off = ((false, false, false, false, false), (false, false, false, false, false));
        let all_on = ((true, true, true, true, true), (true, true, true, true, true));
        let round_trip = |c: &Config| -> Config { serde_json::from_slice(&serde_json::to_vec(c).unwrap()).unwrap() };
        assert_eq!(four(&Config::default()), all_off, "Config's Default stays off");
        // What an install that saved once with this client before the marker holds: the four keys written as off.
        let mut c = round_trip(&forever(Config::default()));
        assert!(!c.forever_enhancements_on);
        assert!(migrate_forever_enhancements(&mut c));
        assert_eq!(four(&c), all_on);
        assert!(c.forever_enhancements_on);
        // The options whose default was already on, and the auto-equip safety option, are not touched by the move.
        assert!(c.quest_giver_markers && c.rotation_autocast && c.solo_badge && c.job_panel && c.exp_bar);
        assert!(!c.auto_equip_upgrades);
        // The options the move leaves off: they draw over the scene, skip dialogue, add text or hide content.
        assert!(!c.map_overlay && c.auto_advance == 0 && !c.stamp_all_lines && !c.synth_amount);
        assert!(!c.hide_trust_speech && !c.hide_ready_lines);
        // Once moved, a later explicit Off is kept through a save and reload, and the move does not run again.
        c.quest_tracker = false;
        let mut again = round_trip(&c);
        assert!(!migrate_forever_enhancements(&mut again));
        assert_eq!(four(&again), ((false, true, true, true, true), (true, true, true, true, true)));
        // Saved as non-Forever (no move, marker unset), then loaded as Forever: it moves.
        let mut other = Config { server: "Other".into(), ..Default::default() };
        assert!(!migrate_forever_enhancements(&mut other));
        assert!(!other.forever_enhancements_on);
        let mut c = forever(round_trip(&other));
        assert!(migrate_forever_enhancements(&mut c));
        assert_eq!(four(&c), all_on);
        // classic_only (regular FFXI) keeps Config's values and the marker unset.
        let mut c = Config { classic_only: true, ..forever(Config::default()) };
        assert!(!migrate_forever_enhancements(&mut c));
        assert_eq!(four(&c), all_off);
        assert!(!c.forever_enhancements_on);
    }

    #[test]
    fn config_saves_atomically_and_replaces_the_old_file() {
        let dir = std::env::temp_dir().join(format!("vana-config-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, b"{\"old\": true}").unwrap();
        write_atomic(&path, b"{\"new\": true}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"new\": true}");
        assert!(!dir.join("config.json.tmp").exists(), "no temporary file is left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A newer client's fields, at the top and in a saved login, survive a load and a save.
    #[test]
    fn unknown_fields_kept() {
        let text = r#"{"server": "x", "username": "u", "future": {"a": [1, 2]}, "logins": [{"server": "x", "username": "u", "later": 7}]}"#;
        let mut c: Config = serde_json::from_str(text).unwrap();
        assert_eq!(c.unknown.get("future"), Some(&serde_json::json!({"a": [1, 2]})));
        c.remember_login();
        let out = written(&c);
        assert!(kept(&serde_json::from_str(text).unwrap(), &out), "{out}");
    }

    /// The real config (when there is one) loses nothing through a load and a save (read only).
    #[test]
    fn real_config_round_trips() {
        let Ok(b) = std::fs::read(config_path()) else { return };
        let Ok(before) = serde_json::from_slice::<serde_json::Value>(&b) else { return };
        let c: Config = serde_json::from_slice(&b).expect("the config parses");
        assert!(kept(&before, &written(&c)));
    }

    /// Only a config copy (FFXI_CONFIG) moves the regime folder, so only then is the data dir's read as well.
    #[test]
    fn regime_moved_with_a_config_copy() {
        assert_eq!(super::regime_moved(false), None);
        assert_eq!(super::regime_moved(true), Some(super::data_dir().join("regime")));
    }

    #[test]
    fn name_file_stays_in_its_folder() {
        assert_eq!(name_file("Steven"), "Steven.json");
        assert_eq!(name_file("../x"), "_2e2e2f78.json");
        assert_eq!(name_file("a/b\\c"), "_612f625c63.json");
        assert_eq!(name_file(""), "_.json");
        // Names that differ only in a replaced character keep separate files.
        assert_ne!(name_file("a/b"), name_file("a\\b"));
        assert_eq!(chat_history_path("../x").file_name().unwrap(), "_2e2e2f78.json");
        assert!(device_name("Con") && device_name("nul") && device_name("COM1") && device_name("Lpt9"));
        assert!(!device_name("Conan") && !device_name("Comx") && !device_name("Nine"));
    }

    #[cfg(feature = "forever")]
    #[test]
    fn layered_fog_is_a_forever_enhanced_option_off_by_default() {
        // Off by default and saved configs without the key parse as off; Enhanced turns it on.
        assert!(!GraphicsCfg::default().fog_ramp && !GraphicsCfg::default().fog_ramp_on());
        let c: Config = serde_json::from_str(r#"{"graphics":{"fog":true}}"#).unwrap();
        assert!(c.graphics.fog_on() && !c.graphics.fog_ramp_on());
        let e = GraphicsCfg::enhanced();
        assert!(e.fog_ramp && e.fog_ramp_on());
        // It needs the height fog.
        assert!(!GraphicsCfg { fog: false, ..e.clone() }.fog_ramp_on());
        assert!(!GraphicsCfg { fog_density: 0.0, ..e }.fog_ramp_on());
        // Original is the default whole, stale ramp values included, once Enhanced is switched off the way Settings does it.
        assert_eq!(GraphicsCfg::default().fog_ramp, false);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn basic_ao_stands_aside_for_the_post_passes_ao() {
        let forever = || Config {
            server: "FFXI Forever".into(),
            named_servers: vec![NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            ssao: true,
            // The original graphics (a new config starts on Enhanced, which turns the post AO on).
            graphics: GraphicsCfg::default(),
            ..Config::default()
        };
        let mut c = forever();
        assert!(c.ssao_enabled(), "basic AO alone still runs");
        c.graphics.ao = true;
        assert!(!c.ssao_enabled(), "never both");
        // At zero strength the post AO is off, so the basic one is not suppressed for nothing.
        c.graphics.ao_strength = 0.0;
        assert!(c.ssao_enabled());
        // Enhanced does not set the basic AO, and a regular server never runs it.
        assert!(!{
            let mut e = forever();
            e.ssao = false;
            e.graphics = GraphicsCfg::enhanced();
            e.ssao_enabled()
        });
        let mut regular = Config { ssao: true, graphics: GraphicsCfg::default(), ..Config::default() };
        regular.graphics.ao = false;
        assert!(!regular.ssao_enabled());
    }

    #[cfg(feature = "forever")]
    #[test]
    fn the_enhanced_preset_carries_the_new_options_and_saved_profiles_take_them_once() {
        let e = GraphicsCfg::enhanced();
        assert!(e.fog_ramp && e.shadow_range == 1 && e.soft_shadows && e.sun_shadow_map() == (3072, 80.0));
        // Without Soft Shadows the maps are the wave-one sizes; with them Near and Wide step up and Far stays.
        let hard = |range| GraphicsCfg { shadow_range: range, soft_shadows: false, ..e.clone() }.sun_shadow_map();
        assert_eq!((hard(0), hard(1), hard(2)), ((1024, 40.0), (2048, 80.0), (4096, 120.0)));
        let soft = |range| GraphicsCfg { shadow_range: range, ..e.clone() }.sun_shadow_map();
        assert_eq!((soft(0), soft(1), soft(2)), ((2048, 40.0), (3072, 80.0), (4096, 120.0)));
        // Original keeps the original map whatever shadow_range says: the range is only read while Enhanced.
        assert_eq!(
            GraphicsCfg { enhanced: false, shadow_range: 2, ..Default::default() }.sun_shadow_map(),
            (1024, 40.0)
        );
        assert_eq!(GraphicsCfg { shadow_range: 0, ..e.clone() }.sun_shadow_map(), (2048, 40.0));
        assert_eq!(GraphicsCfg { shadow_range: 9, ..e.clone() }.sun_shadow_map(), (4096, 120.0));
        // Soft shadows are never on in Original, whatever the saved flag says.
        assert!(!GraphicsCfg { enhanced: false, soft_shadows: true, ..Default::default() }.soft_shadows_on());
        assert!(
            e.soft_shadows_on() && !GraphicsCfg::default().soft_shadows && !GraphicsCfg::default().soft_shadows_on()
        );
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            c.graphics
        };
        // An Enhanced profile saved before the options existed takes the preset's values.
        let old = migrate(r#"{"graphics":{"enhanced":true,"ao":true,"fog":true}}"#);
        assert!(old.fog_ramp && old.shadow_range == 1);
        // One that saved a choice keeps it, key by key.
        let kept = migrate(r#"{"graphics":{"enhanced":true,"fog":true,"fog_ramp":false,"shadow_range":2}}"#);
        assert!(!kept.fog_ramp && kept.shadow_range == 2);
        let half = migrate(r#"{"graphics":{"enhanced":true,"shadow_range":0}}"#);
        assert!(half.fog_ramp && half.shadow_range == 0);
        // Soft shadows key on their own marker: a profile that saved them off keeps them off, one without the key takes them.
        assert!(old.soft_shadows && half.soft_shadows);
        // Far View: off (Retail) in Original, Far in the preset, taken once by a profile without its key; the factor reads 1
        // unless Enhanced chose it (always 1 in classic).
        assert_eq!((GraphicsCfg::default().far_view, GraphicsCfg::default().far_view_factor()), (0, 1.0));
        assert!(e.far_view == 1 && e.far_view_factor() == 1.5 && old.far_view == 1 && half.far_view == 1);
        assert_eq!(GraphicsCfg { far_view: 2, ..e.clone() }.far_view_factor(), 2.0);
        assert_eq!(GraphicsCfg { enhanced: false, far_view: 2, ..Default::default() }.far_view_factor(), 1.0);
        let retail = migrate(r#"{"graphics":{"enhanced":true,"far_view":0}}"#);
        assert!(retail.far_view == 0 && retail.far_view_factor() == 1.0);
        let very = migrate(r#"{"graphics":{"enhanced":true,"far_view":2}}"#);
        assert_eq!(very.far_view_factor(), 2.0);
        // Shadow Cascades: off in Original, in the preset, taken once by a profile without its key, and only read while Enhanced.
        assert!(!GraphicsCfg::default().shadow_cascades && !GraphicsCfg::default().cascades_on());
        assert!(e.shadow_cascades && e.cascades_on() && old.shadow_cascades && half.shadow_cascades);
        assert!(!GraphicsCfg { enhanced: false, shadow_cascades: true, ..Default::default() }.cascades_on());
        let no_casc = migrate(r#"{"graphics":{"enhanced":true,"shadow_cascades":false}}"#);
        assert!(!no_casc.shadow_cascades && !no_casc.cascades_on());
        // Character Shade: off in Original, in the preset, taken once by a profile without its key, and its strength reads 0
        // unless Enhanced chose it.
        assert!(old.actor_sun_shadows && half.actor_sun_shadows && e.actor_sun_shadows);
        assert_eq!(e.actor_shade_amount(), ACTOR_SHADE);
        assert_eq!(GraphicsCfg::default().actor_shade_amount(), 0.0);
        assert_eq!(
            GraphicsCfg { enhanced: false, actor_sun_shadows: true, ..Default::default() }.actor_shade_amount(),
            0.0
        );
        assert_eq!(GraphicsCfg { actor_sun_shadows: false, ..e.clone() }.actor_shade_amount(), 0.0);
        let shade_off = migrate(r#"{"graphics":{"enhanced":true,"actor_sun_shadows":false}}"#);
        assert!(!shade_off.actor_sun_shadows && shade_off.actor_shade_amount() == 0.0);
        let off = migrate(r#"{"graphics":{"enhanced":true,"shadow_range":1,"soft_shadows":false}}"#);
        assert!(!off.soft_shadows && off.sun_shadow_map() == (2048, 80.0));
        let custom = migrate(r#"{"graphics":{"enhanced":false,"soft_shadows":true}}"#);
        assert!(custom.soft_shadows && !custom.soft_shadows_on());
        // Original and Custom are never touched.
        assert_eq!(migrate(r#"{"graphics":{"fog":true}}"#), GraphicsCfg { fog: true, ..Default::default() });
        assert_eq!(migrate("{}"), GraphicsCfg::default());
    }

    #[cfg(feature = "forever")]
    #[test]
    fn ambient_only_ao_needs_ao_and_is_in_the_preset_since_wave_three() {
        assert_eq!(GraphicsCfg::default().ao_light, 0);
        assert!(!GraphicsCfg::default().ambient_ao_on());
        // The wave-3 preset carries Ambient Only (a stronger occlusion that does not eat direct light).
        assert!(GraphicsCfg::enhanced().ao_on() && GraphicsCfg::enhanced().ambient_ao_on());
        let chosen = GraphicsCfg { ao_light: 1, ..GraphicsCfg::enhanced() };
        assert!(chosen.ambient_ao_on());
        assert!(!GraphicsCfg { ao_light: 0, ..GraphicsCfg::enhanced() }.ambient_ao_on());
        // It needs the occlusion on.
        assert!(!GraphicsCfg { ao: false, ao_light: 1, ..Default::default() }.ambient_ao_on());
        assert!(!GraphicsCfg { ao_strength: 0.0, ao_light: 1, ..GraphicsCfg::enhanced() }.ambient_ao_on());
        let saved: Config = serde_json::from_str(r#"{"graphics":{"enhanced":true,"ao":true}}"#).unwrap();
        assert_eq!(saved.graphics.ao_light, 0);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn wave_three_options_are_off_by_default_in_original_and_enhanced_takes_them_once_keyed_on_light_grade() {
        let d = GraphicsCfg::default();
        assert!(!d.light_grade && !d.fog_layers && d.light_grade_amount == 1.0 && d.light_grade_strength() == 0.0);
        assert!(!d.light_grade_on() && !d.fog_layers_on());
        let e = GraphicsCfg::enhanced();
        assert!(e.light_grade_on() && e.fog_layers_on() && e.light_grade_strength() == 1.0);
        // The strength reads 0 with the grade off or the amount at zero.
        assert_eq!(GraphicsCfg { light_grade: false, ..e.clone() }.light_grade_strength(), 0.0);
        assert_eq!(GraphicsCfg { light_grade_amount: 0.0, ..e.clone() }.light_grade_strength(), 0.0);
        assert_eq!(GraphicsCfg { light_grade_amount: 9.0, ..e.clone() }.light_grade_strength(), 1.5);
        // Layers need the Height Fog.
        assert!(!GraphicsCfg { fog: false, ..e.clone() }.fog_layers_on());
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            c.graphics
        };
        // A wave-2 Enhanced profile (it already saved `ao_light` and the older keys) still takes the wave-3 retune.
        let w2 = migrate(
            r#"{"graphics":{"enhanced":true,"ao":true,"ao_strength":0.6,"ao_light":0,"fog_ramp":true,"shadow_range":1,"fog":true}}"#,
        );
        assert!(w2.light_grade && w2.fog_layers && w2.ao_light == 1 && (w2.ao_strength - 0.9).abs() < 1e-4);
        // A tuned strength is kept; a profile that already has the marker keeps its choices.
        let tuned = migrate(r#"{"graphics":{"enhanced":true,"ao_strength":1.4}}"#);
        assert!(tuned.light_grade && (tuned.ao_strength - 1.4).abs() < 1e-4);
        let kept = migrate(r#"{"graphics":{"enhanced":true,"light_grade":false,"fog_layers":false,"ao_light":0}}"#);
        assert!(!kept.light_grade && !kept.fog_layers && kept.ao_light == 0);
        // Original and Custom are never touched.
        assert_eq!(migrate(r#"{"graphics":{"fog":true}}"#), GraphicsCfg { fog: true, ..Default::default() });
    }

    /// Enhanced Water is off in Original, on in the Enhanced preset, and a saved Enhanced profile without the key takes it once
    /// (keyed on `water_enhanced`); an explicit choice and Original/Custom profiles are kept.
    #[cfg(feature = "forever")]
    #[test]
    fn enhanced_water_is_in_the_preset_and_migrates_once_keyed_on_its_own_field() {
        assert!(!GraphicsCfg::default().water_enhanced);
        assert!(GraphicsCfg::enhanced().water_enhanced);
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            c.graphics
        };
        // A wave-3 Enhanced profile (it has `light_grade`) still takes water once.
        assert!(migrate(r#"{"graphics":{"enhanced":true,"light_grade":true,"fog_layers":true}}"#).water_enhanced);
        // An explicit off survives; so does an explicit on.
        assert!(!migrate(r#"{"graphics":{"enhanced":true,"light_grade":true,"water_enhanced":false}}"#).water_enhanced);
        assert!(migrate(r#"{"graphics":{"enhanced":true,"water_enhanced":true}}"#).water_enhanced);
        // Original and Custom keep what they saved (a stale key never turns Enhanced on).
        assert!(!migrate(r#"{"graphics":{"fog":true}}"#).water_enhanced);
        assert!(
            migrate(r#"{"graphics":{"water_enhanced":true}}"#).water_enhanced
                && !migrate(r#"{"graphics":{"water_enhanced":true}}"#).enhanced
        );
    }

    #[cfg(feature = "forever")]
    #[test]
    fn detail_grain_is_a_settings_choice_off_everywhere_and_never_migrated_in() {
        let d = GraphicsCfg::default();
        assert!(!d.detail_textures && d.detail_strength == 1.0 && d.detail_strength_on() == 0.0);
        let e = GraphicsCfg::enhanced();
        assert!(!e.detail_textures && e.detail_strength_on() == 0.0);
        assert_eq!(GraphicsCfg { detail_textures: true, ..e.clone() }.detail_strength_on(), 1.0);
        assert_eq!(GraphicsCfg { detail_textures: true, detail_strength: 9.0, ..e.clone() }.detail_strength_on(), 2.0);
        assert_eq!(GraphicsCfg { detail_textures: true, detail_strength: 0.0, ..e.clone() }.detail_strength_on(), 0.0);
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            c.graphics
        };
        // The migration never turns the grain on; a saved choice is kept.
        assert!(!migrate(r#"{"graphics":{"enhanced":true,"light_grade":true}}"#).detail_textures);
        let kept = migrate(
            r#"{"graphics":{"enhanced":true,"light_grade":true,"detail_textures":true,"detail_strength":0.5}}"#,
        );
        assert!(kept.detail_textures && kept.detail_strength == 0.5);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn texture_sharpening_is_a_chosen_amount_off_by_default_and_clamped() {
        assert_eq!(GraphicsCfg::default().texture_sharpening_on(), 0.0);
        assert_eq!(GraphicsCfg::enhanced().texture_sharpening_on(), 1.0);
        assert_eq!(GraphicsCfg { texture_sharpening: 1.0, ..Default::default() }.texture_sharpening_on(), 1.0);
        assert_eq!(GraphicsCfg { texture_sharpening: 9.0, ..Default::default() }.texture_sharpening_on(), 1.5);
        assert_eq!(GraphicsCfg { texture_sharpening: -1.0, ..Default::default() }.texture_sharpening_on(), 0.0);
        // A saved profile without the key reads it as off until the Enhanced migration gives it the preset's value, once;
        // a saved choice is kept; Original and Custom (not enhanced) are never touched.
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            let before = c.graphics.texture_sharpening_on();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            (before, c.graphics.texture_sharpening_on())
        };
        assert_eq!(migrate(r#"{"graphics":{"enhanced":true,"light_grade":true}}"#), (0.0, 1.0));
        assert_eq!(
            migrate(r#"{"graphics":{"enhanced":true,"light_grade":true,"texture_sharpening":0.0}}"#),
            (0.0, 0.0)
        );
        assert_eq!(migrate(r#"{"graphics":{"enhanced":true,"texture_sharpening":0.5}}"#), (0.5, 0.5));
        assert_eq!(migrate(r#"{"graphics":{"fog":true}}"#), (0.0, 0.0));
    }

    #[cfg(feature = "forever")]
    #[test]
    fn volumetric_fog_is_off_in_original_on_in_enhanced_needs_the_height_fog_and_migrates_in_once() {
        let d = GraphicsCfg::default();
        assert!(!d.volumetric_fog && d.volumetric_fog_amount == 1.0 && d.volumetric_fog_on() == 0.0);
        let e = GraphicsCfg::enhanced();
        assert!(e.volumetric_fog && e.volumetric_fog_on() == 1.0);
        assert_eq!(GraphicsCfg { volumetric_fog: false, ..e.clone() }.volumetric_fog_on(), 0.0);
        assert_eq!(GraphicsCfg { volumetric_fog_amount: 9.0, ..e.clone() }.volumetric_fog_on(), 1.5);
        // It marches the Height Fog's density: with the fog off it is off.
        assert_eq!(GraphicsCfg { fog: false, ..e.clone() }.volumetric_fog_on(), 0.0);
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            c.graphics
        };
        assert!(migrate(r#"{"graphics":{"enhanced":true,"light_grade":true,"sun_shafts":true}}"#).volumetric_fog);
        let kept = migrate(r#"{"graphics":{"enhanced":true,"volumetric_fog":false,"volumetric_fog_amount":0.5}}"#);
        assert!(!kept.volumetric_fog && kept.volumetric_fog_amount == 0.5);
        assert!(!migrate(r#"{"graphics":{"fog":true}}"#).volumetric_fog);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn sun_shafts_are_off_in_original_on_in_enhanced_and_migrated_in_once() {
        let d = GraphicsCfg::default();
        assert!(!d.sun_shafts && d.sun_shafts_amount == 1.0 && d.sun_shafts_on() == 0.0);
        let e = GraphicsCfg::enhanced();
        assert!(e.sun_shafts && e.sun_shafts_on() == 1.0);
        assert_eq!(GraphicsCfg { sun_shafts: false, ..e.clone() }.sun_shafts_on(), 0.0);
        assert_eq!(GraphicsCfg { sun_shafts_amount: 9.0, ..e.clone() }.sun_shafts_on(), 1.5);
        // They are a depth effect: with only them on the post pass still reads the depth.
        assert!(!d.depth_on());
        assert!(GraphicsCfg { sun_shafts: true, ..Default::default() }.depth_on());
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            c.graphics
        };
        // An Enhanced profile without the marker takes them once; a saved choice (off, or a strength) is kept; Original and
        // Custom are never touched.
        assert!(migrate(r#"{"graphics":{"enhanced":true,"light_grade":true}}"#).sun_shafts);
        let kept = migrate(r#"{"graphics":{"enhanced":true,"sun_shafts":false,"sun_shafts_amount":0.5}}"#);
        assert!(!kept.sun_shafts && kept.sun_shafts_amount == 0.5);
        assert!(!migrate(r#"{"graphics":{"fog":true}}"#).sun_shafts);
    }

    /// Procedural clouds are off in Original, on in the Enhanced preset, and a saved Enhanced profile without the key takes
    /// them once (keyed on `sky_clouds`); they need the Enhanced look.
    #[cfg(feature = "forever")]
    #[test]
    fn sky_clouds_are_in_the_preset_and_migrate_once_keyed_on_their_own_field() {
        assert!(!GraphicsCfg::default().sky_clouds && !GraphicsCfg::default().sky_clouds_on());
        assert!(GraphicsCfg::enhanced().sky_clouds && GraphicsCfg::enhanced().sky_clouds_on());
        // The flag alone (Original / Custom look) draws nothing.
        assert!(!GraphicsCfg { sky_clouds: true, ..Default::default() }.sky_clouds_on());
        let migrate = |json: &str| {
            let raw: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut c: Config = serde_json::from_value(raw.clone()).unwrap();
            migrate_enhanced_retune(&raw, &mut c.graphics);
            c.graphics
        };
        assert!(migrate(r#"{"graphics":{"enhanced":true,"light_grade":true,"water_enhanced":true}}"#).sky_clouds);
        assert!(!migrate(r#"{"graphics":{"enhanced":true,"light_grade":true,"sky_clouds":false}}"#).sky_clouds);
        assert!(!migrate(r#"{"graphics":{"fog":true}}"#).sky_clouds);
        assert!(!migrate(r#"{"graphics":{"sky_clouds":false}}"#).sky_clouds);
    }
}
