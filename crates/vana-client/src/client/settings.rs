//! Our Settings and Extras pages (not retail). Settings holds what retail set in the Windows "FINAL FANTASY XI
//! Config" tool before starting (registry names 0000..0043, read by FUN_10016c30) plus our own display options;
//! Extras holds the gameplay additions (the add-ons' features), each off until turned on. Both are drawn as the
//! Config pages are (FUN_101144b0 over windowps 465: bullets, fontshp text, the buttons, markers and
//! sliders of conf2win / conf3win), at the pages' place in game and centred in the lobby. Both have several pages,
//! turned as miss00 turns its nations: red arrows (menu frames 25 / 26) at the top corners and the page's name beside
//! the left one (FUN_1022de70).

use super::game_ui::GameUi;
use crate::audio::Ui as Se;
use crate::config::{Config, FRAME_LIMITS};

mod win;
pub use win::{WCtl, WRow};

/// What the pages edit (merged back into the config by the app).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub tool: std::collections::BTreeMap<String, u32>,
    pub frame_limit: u32,
    pub reduce_motion: bool,
    pub anisotropy: u16,
    pub antialias: u32,
    pub grading: u32,
    pub draw_distance: u32,
    pub send_interval: u32,
    pub party_tp: bool,
    #[cfg(feature = "forever")]
    pub map_prefs: super::mapview::Prefs,
    pub map_overlay: bool,
    pub map_opacity: u32,
    pub synth_amount: bool,
    pub stamp_all_lines: bool,
    pub target_distance: bool,
    pub target_hpp: bool,
    pub earth_clock: bool,
    pub lamp_lights: bool,
    pub actor_shadows: u32,
    pub shadow_map: u32,
    pub ssao: bool,
    pub hide_trust_speech: bool,
    pub hide_ready_lines: bool,
    pub short_battle: bool,
    pub battle_name_colours: bool,
    pub auto_advance: u32,
    pub timers: bool,
    pub timers_dock: u32,
    pub regime: bool,
    pub skillchain_helper: bool,
    pub minimap: bool,
    pub modern_minimap: bool,
    pub solo_badge: bool,
    pub job_panel: bool,
    pub exp_bar: bool,
    #[cfg(feature = "forever")]
    pub quest_tracker: bool,
    #[cfg(feature = "forever")]
    pub quest_giver_markers: bool,
    #[cfg(feature = "forever")]
    pub auto_navigation: bool,
    #[cfg(feature = "forever")]
    pub training_hud: bool,
    #[cfg(feature = "forever")]
    pub journal_key: bool,
    #[cfg(feature = "forever")]
    pub auto_equip_upgrades: bool,
    #[cfg(feature = "forever")]
    pub con_colors: bool,
    #[cfg(feature = "forever")]
    pub compact_hud: bool,
    #[cfg(feature = "forever")]
    pub auto_check_on_target: bool,
    #[cfg(feature = "forever")]
    pub nav_mesh: bool,
    #[cfg(feature = "forever")]
    pub recipe_log_key: u8,
    #[cfg(feature = "forever")]
    pub environment_textures: bool,
    #[cfg(feature = "forever")]
    pub woods_textures: bool,
    /// Read-only: the server is FFXI Forever (the EXP bar row is for it).
    pub forever: bool,
    pub classic_only: bool,
    /// The containers kept sorted (config auto_sort; the rows set them all or none) and combining.
    pub auto_sort: u32,
    pub auto_combine: bool,
    pub rotation_adaptive: bool,
    pub rotation_autocast: bool,
    pub rotation_cure_pct: u32,
    pub rotation_convert_pct: u32,
    pub rotation_renew_secs: u32,
    pub minimap_unowned: bool,
    pub minimap_dock: u32,
    pub minimap_size: u32,
    pub music: u32,
    pub effects: u32,
    pub retail_aspect: bool,
    pub monitor: String,
    pub gpu: String,
    pub cursor_size: u32,
    /// The Gamepad page: on, the sticks' deadzone and the camera's speed, in percent.
    pub gamepad: bool,
    pub gamepad_deadzone: u32,
    pub gamepad_camera: u32,
    /// The Controls page's scheme (config controls: 0 Original, 1 FFXI WASD, 2 Modern, 3 Forever on Forever only) and
    /// Modern's A / D choice.
    pub controls: u32,
    pub controls_ad_turn: bool,
    #[cfg(feature = "forever")]
    pub invert_horizontal_mouse: bool,
    pub keys_hide_cursor: bool,
    pub modern_ui: bool,
    pub ui_scale: u32,
    /// The post-processing pass (config graphics; render/post.rs).
    pub graphics: crate::config::GraphicsCfg,
    pub modern_scale: u32,
    pub modern_opacity: u32,
    /// The mod packs in priority order (the Mods page; config `mods`).
    pub mods: Vec<crate::config::ModRef>,
    /// Panel Places' Reset pressed: this character's modern panels go back to where they start (not kept).
    pub reset_places: bool,
    /// Mouse drag camera speed in % (config mouse_camera_speed).
    pub mouse_camera_speed: u32,
}

impl Settings {
    pub fn of(c: &Config) -> Self {
        let pct = |v: f32| (v.clamp(0.0, 1.0) * 100.0).round() as u32;
        Self {
            tool: c.tool.clone(),
            frame_limit: c.frame_limit,
            reduce_motion: c.reduce_motion,
            anisotropy: c.anisotropy,
            antialias: c.antialias,
            grading: c.grading,
            draw_distance: c.draw_distance,
            send_interval: c.send_interval,
            party_tp: c.party_tp,
            #[cfg(feature = "forever")]
            map_prefs: c.forever_map.clone(),
            map_overlay: c.map_overlay,
            map_opacity: pct(c.map_overlay_opacity),
            synth_amount: c.synth_amount,
            stamp_all_lines: c.stamp_all_lines,
            target_distance: c.target_distance,
            target_hpp: c.target_hpp,
            earth_clock: c.earth_clock,
            lamp_lights: c.lamp_lights_enabled(),
            actor_shadows: c.actor_shadows,
            shadow_map: c.shadow_map,
            ssao: c.ssao,
            hide_trust_speech: c.hide_trust_speech,
            hide_ready_lines: c.hide_ready_lines,
            short_battle: c.short_battle,
            battle_name_colours: c.battle_name_colours,
            auto_advance: c.auto_advance,
            timers: c.timers,
            timers_dock: c.timers_dock,
            regime: c.regime,
            skillchain_helper: c.skillchain_helper,
            minimap: c.minimap,
            modern_minimap: c.modern_minimap,
            solo_badge: c.solo_badge,
            job_panel: c.job_panel,
            exp_bar: c.exp_bar,
            #[cfg(feature = "forever")]
            quest_tracker: c.quest_tracker,
            #[cfg(feature = "forever")]
            quest_giver_markers: c.quest_giver_markers,
            #[cfg(feature = "forever")]
            auto_navigation: c.auto_navigation,
            #[cfg(feature = "forever")]
            training_hud: c.training_hud,
            #[cfg(feature = "forever")]
            journal_key: c.journal_key,
            #[cfg(feature = "forever")]
            auto_equip_upgrades: c.auto_equip_upgrades,
            #[cfg(feature = "forever")]
            con_colors: c.con_colors,
            #[cfg(feature = "forever")]
            compact_hud: c.forever_compact_hud,
            #[cfg(feature = "forever")]
            auto_check_on_target: c.auto_check_on_target,
            #[cfg(feature = "forever")]
            nav_mesh: c.nav_mesh,
            #[cfg(feature = "forever")]
            recipe_log_key: c.recipe_log_key,
            #[cfg(feature = "forever")]
            environment_textures: c.environment_textures,
            #[cfg(feature = "forever")]
            woods_textures: c.woods_textures,
            forever: c.is_forever(),
            classic_only: c.classic_only,
            auto_sort: c.auto_sort,
            auto_combine: c.auto_combine,
            rotation_adaptive: c.rotation_adaptive,
            rotation_autocast: c.rotation_autocast,
            rotation_cure_pct: c.rotation_cure_pct,
            rotation_convert_pct: c.rotation_convert_pct,
            rotation_renew_secs: c.rotation_renew_secs,
            minimap_unowned: c.minimap_unowned,
            minimap_dock: c.minimap_dock,
            minimap_size: c.minimap_size,
            // The game's own volumes (Gameplay page, config 10 / 9; the lobby's 0xa8): set by the opener.
            music: 100,
            effects: 100,
            retail_aspect: c.retail_aspect,
            monitor: c.monitor.clone(),
            gpu: c.gpu.clone(),
            cursor_size: c.cursor_size,
            gamepad: c.gamepad,
            gamepad_deadzone: pct(c.gamepad_deadzone),
            gamepad_camera: (c.gamepad_camera_speed * 100.0).round().clamp(10.0, 500.0) as u32,
            controls: c.controls.min(3),
            controls_ad_turn: c.controls_ad_turn,
            #[cfg(feature = "forever")]
            invert_horizontal_mouse: c.invert_horizontal_mouse,
            keys_hide_cursor: c.keys_hide_cursor,
            modern_ui: c.modern_enabled(),
            ui_scale: c.ui_scale,
            graphics: c.graphics_effects(),
            modern_scale: c.modern_scale,
            modern_opacity: pct(c.modern_opacity),
            mods: crate::modpack::merged(&c.mods, &crate::modpack::folders()),
            reset_places: false,
            mouse_camera_speed: c.mouse_camera_speed,
        }
    }

    pub fn store(&self, c: &mut Config) {
        c.monitor = self.monitor.clone();
        c.gpu = self.gpu.clone();
        c.tool = self.tool.clone();
        c.frame_limit = self.frame_limit;
        c.reduce_motion = self.reduce_motion;
        c.anisotropy = self.anisotropy;
        c.antialias = self.antialias;
        c.grading = self.grading;
        c.draw_distance = self.draw_distance;
        c.send_interval = self.send_interval;
        c.party_tp = self.party_tp;
        #[cfg(feature = "forever")]
        if c.is_forever() {
            c.forever_map = self.map_prefs.clone();
        }
        c.map_overlay = self.map_overlay;
        c.map_overlay_opacity = self.map_opacity as f32 / 100.0;
        c.synth_amount = self.synth_amount;
        c.stamp_all_lines = self.stamp_all_lines;
        c.target_distance = self.target_distance;
        c.target_hpp = self.target_hpp;
        c.earth_clock = self.earth_clock;
        if c.is_forever() {
            c.lamp_lights = self.lamp_lights;
        }
        c.actor_shadows = self.actor_shadows;
        c.shadow_map = self.shadow_map;
        c.ssao = self.ssao;
        c.hide_trust_speech = self.hide_trust_speech;
        c.hide_ready_lines = self.hide_ready_lines;
        c.short_battle = self.short_battle;
        c.battle_name_colours = self.battle_name_colours;
        c.auto_advance = self.auto_advance;
        c.timers = self.timers;
        c.timers_dock = self.timers_dock;
        c.regime = self.regime;
        c.skillchain_helper = self.skillchain_helper;
        c.minimap = self.minimap;
        c.modern_minimap = self.modern_minimap;
        c.solo_badge = self.solo_badge;
        c.job_panel = self.job_panel;
        c.exp_bar = self.exp_bar;
        #[cfg(feature = "forever")]
        if self.forever {
            c.quest_tracker = self.quest_tracker;
            c.quest_giver_markers = self.quest_giver_markers;
            c.auto_navigation = self.auto_navigation;
            c.training_hud = self.training_hud;
            c.environment_textures = self.environment_textures;
            c.woods_textures = self.woods_textures;
            c.journal_key = self.journal_key;
            c.auto_equip_upgrades = self.auto_equip_upgrades;
            c.con_colors = self.con_colors;
            c.forever_compact_hud = self.compact_hud;
            c.auto_check_on_target = self.auto_check_on_target;
            c.nav_mesh = self.nav_mesh;
            c.recipe_log_key = self.recipe_log_key;
        }
        c.rotation_adaptive = self.rotation_adaptive;
        c.rotation_autocast = self.rotation_autocast;
        c.auto_sort = self.auto_sort;
        c.auto_combine = self.auto_combine;
        c.rotation_cure_pct = self.rotation_cure_pct;
        c.rotation_convert_pct = self.rotation_convert_pct;
        c.rotation_renew_secs = self.rotation_renew_secs;
        c.minimap_unowned = self.minimap_unowned;
        c.minimap_dock = self.minimap_dock;
        c.minimap_size = self.minimap_size;
        c.retail_aspect = self.retail_aspect;
        c.cursor_size = self.cursor_size;
        c.gamepad = self.gamepad;
        c.gamepad_deadzone = self.gamepad_deadzone as f32 / 100.0;
        c.gamepad_camera_speed = self.gamepad_camera as f32 / 100.0;
        // Forever's scheme is kept as saved off Forever (it plays as Modern there; the row does not offer it).
        if self.controls != 3 || (c.is_forever() && !c.classic_only) {
            c.controls = self.controls;
        }
        c.controls_ad_turn = self.controls_ad_turn;
        #[cfg(feature = "forever")]
        if c.is_forever() {
            c.invert_horizontal_mouse = self.invert_horizontal_mouse;
        }
        c.keys_hide_cursor = self.keys_hide_cursor;
        if c.is_forever() && !c.classic_only {
            c.modern_ui = self.modern_ui;
        }
        c.ui_scale = self.ui_scale;
        if c.is_forever() {
            c.graphics = self.graphics.clone();
        } else {
            // Preserve dormant Forever preferences when using a regular server in the full executable.
            c.graphics.fxaa = self.graphics.fxaa;
            c.graphics.sharpen = self.graphics.sharpen;
        }
        c.modern_scale = self.modern_scale;
        c.modern_opacity = self.modern_opacity as f32 / 100.0;
        c.mods = self.mods.clone();
        c.gamepad = self.gamepad;
        c.mouse_camera_speed = self.mouse_camera_speed;
    }
}

/// Which page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Settings,
    Extras,
}

/// When a change shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum When {
    Now,
    NextZone,
    Restart,
}

#[derive(Clone, Copy, Debug)]
enum Val {
    Tool(&'static str),
    /// Width / height names; the choices are sizes.
    Size(&'static str, &'static str, Sizes),
    Gamma,
    /// Ours: the Graphics page's preset (Original / High / Ultra / Custom), derived from the graphics rows, not kept.
    Preset,
    #[cfg(feature = "forever")]
    ForeverLook,
    #[cfg(feature = "forever")]
    Upscaler,
    #[cfg(feature = "forever")]
    UpscaleQuality,
    #[cfg(feature = "forever")]
    EnvironmentTextures,
    #[cfg(feature = "forever")]
    WoodsTextures,
    FrameLimit,
    ReduceMotion,
    Aniso,
    Antialias,
    Grading,
    DrawDistance,
    SendRate,
    PartyTp,
    #[cfg(feature = "forever")]
    MapView(u8),
    MapOverlay,
    MapOpacity,
    SynthAmount,
    StampAll,
    TargetDistance,
    TargetHpp,
    EarthClock,
    LampLights,
    ActorShadows,
    ShadowMap,
    /// Forever Enhanced's directional sun shadow coverage (GraphicsCfg::shadow_range); the row is Forever-only.
    ShadowRange,
    /// Forever Enhanced's Far View (GraphicsCfg::far_view); the row is Forever-only.
    FarView,
    /// Forever Enhanced's soft sun-shadow edges (GraphicsCfg::soft_shadows); the row is Forever-only.
    SoftShadows,
    /// Forever Enhanced's Character Shade (GraphicsCfg::actor_sun_shadows); the row is Forever-only.
    ActorShade,
    /// Forever Enhanced's Shadow Cascades (GraphicsCfg::shadow_cascades); the row is Forever-only.
    ShadowCascades,
    Ssao,
    TrustSpeech,
    ReadyLines,
    ShortBattle,
    NameColours,
    AutoAdvance,
    Timers,
    TimersDock,
    Skillchains,
    Regime,
    Minimap,
    ModernMinimap,
    SoloBadge,
    JobPanel,
    ExpBar,
    #[cfg(feature = "forever")]
    QuestTracker,
    /// Forever: the quest books on NPCs the server reports (config quest_giver_markers; Special's Quest Indicators too).
    #[cfg(feature = "forever")]
    QuestIndicators,
    /// Forever: the navigation helper on or off (config auto_navigation). Applied through the same handler as Special's.
    #[cfg(feature = "forever")]
    AutoNavigation,
    /// Forever Modern: the Training tracker panel (config training_hud); the Training menu's own toggle changes the same setting.
    #[cfg(feature = "forever")]
    TrainingHud,
    /// Forever: whether each session starts with the rotation's autocast (config rotation_autocast). Special's Auto Rotation
    /// changes only the session.
    #[cfg(feature = "forever")]
    AutoRotationStart,
    /// Forever Modern controls: J opens the Journal; the Crafting Log's key (config journal_key, recipe_log_key).
    #[cfg(feature = "forever")]
    JournalKey,
    /// Forever Modern: equip clearly better new pieces automatically (config auto_equip_upgrades; off by default).
    #[cfg(feature = "forever")]
    AutoEquipUpgrades,
    /// Forever: monster names by their /check colour (config con_colors), and the one-check-on-target (auto_check_on_target).
    #[cfg(feature = "forever")]
    ConColors,
    /// Forever Modern: the Compact HUD (config forever_compact_hud).
    #[cfg(feature = "forever")]
    CompactHud,
    #[cfg(feature = "forever")]
    AutoCheckTarget,
    /// Forever: Auto Navigation plans on the locally baked navigation mesh (config nav_mesh; on by default, a saved choice is kept).
    #[cfg(feature = "forever")]
    NavMesh,
    #[cfg(feature = "forever")]
    RecipeLogKey,
    /// The suggestions' thresholds (config rotation_*).
    RotAdaptive,
    /// Inventory: keep every bag sorted, combine stacks.
    KeepSorted,
    Combine,
    RotCure,
    RotConvert,
    RotRenew,
    MinimapUnowned,
    MinimapDock,
    MinimapSize,
    Music,
    Effects,
    RetailAspect,
    CursorSize,
    Gamepad,
    GamepadDeadzone,
    GamepadCamera,
    /// Ours: the keyboard and mouse scheme (config controls) and Modern's A / D keys (controls_ad_turn).
    Controls,
    ControlsAd,
    #[cfg(feature = "forever")]
    InvertHorizontalMouse,
    KeysHideCursor,
    /// The monitor for full screen, from the list the app passes in.
    Monitor,
    /// The graphics adapter ("Default" then the app's list).
    Gpu,
    ModernUi,
    UiScale,
    /// One click for the whole look (derived, not stored): Modern, Original (retail's), else Custom.
    Experience,
    /// The post-processing pass's options (config graphics), percents for the numbers.
    Fxaa,
    Bloom,
    BloomIntensity,
    BloomThreshold,
    Tonemap,
    Exposure,
    Sharpen,
    Saturation,
    Ao,
    AoStrength,
    AoRadius,
    /// What the occlusion darkens (GraphicsCfg::ao_light); the row is Forever-only.
    AoLight,
    Fog,
    FogDensity,
    FogHeight,
    /// Forever Enhanced's layered fog (GraphicsCfg::fog_ramp); the row exists in the Forever flavour only.
    FogRamp,
    /// Forever Enhanced's volume-style fog layers (GraphicsCfg::fog_layers); Forever-only.
    FogLayers,
    /// Forever Enhanced's procedural water finish (GraphicsCfg::water_enhanced); Forever-only.
    WaterEnhanced,
    /// Forever Enhanced's procedural clouds (GraphicsCfg::sky_clouds); Forever-only.
    SkyClouds,
    /// Forever Enhanced's authored zone lighting (GraphicsCfg::light_grade) and its strength in percent; Forever-only.
    LightGrade,
    LightGradeAmount,
    /// Forever Enhanced's sun shafts (GraphicsCfg::sun_shafts) and their strength in percent; Forever-only.
    SunShafts,
    SunShaftsAmount,
    /// Forever Enhanced's volumetric fog (GraphicsCfg::volumetric_fog) and its strength in percent; Forever-only.
    VolumetricFog,
    VolumetricFogAmount,
    /// Forever Enhanced's detail textures (GraphicsCfg::detail_textures) and their strength in percent; Forever-only.
    DetailTextures,
    DetailStrength,
    /// Forever's magnification texture sharpening amount in percent (GraphicsCfg::texture_sharpening); Forever-only.
    TextureSharpening,
    CharacterLight,
    ModernScale,
    ModernOpacity,
    ResetPlaces,
    MouseCamSpeed,
    /// A Lua mod's declared setting: its row on the mod's page (docs/lua-mods.md 4.12).
    Mod(usize),
    /// A mod's setting on one of our Extras tabs (`SettingsUi::tab_rows`).
    Tabbed(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sizes {
    Window,
    Background,
    Ui,
}

#[derive(Clone, Copy)]
struct SRow {
    label: &'static str,
    help: &'static str,
    val: Val,
    /// Named choices (value, label); empty for sizes and numbers. Up to three are buttons, more a slider.
    choices: &'static [(u32, &'static str)],
    when: When,
}

/// A row as the page uses it: a static table's, or a mod setting's.
#[derive(Clone)]
struct Row {
    label: String,
    help: String,
    val: Val,
    choices: Vec<(u32, String)>,
    when: When,
}

impl From<&SRow> for Row {
    fn from(r: &SRow) -> Self {
        Row {
            label: r.label.into(),
            help: r.help.into(),
            val: r.val,
            choices: r.choices.iter().map(|c| (c.0, c.1.to_string())).collect(),
            when: r.when,
        }
    }
}

/// A Lua mod's page on Extras: its declared settings that buttons and sliders can show.
#[derive(Clone, Debug)]
pub struct ModPage {
    pub id: String,
    pub name: String,
    pub rows: Vec<ModRow>,
}

/// A mod setting: its key, heading, help, the choices (label, value) and the one that holds.
#[derive(Clone, Debug)]
pub struct ModRow {
    pub key: String,
    pub label: String,
    pub help: String,
    pub choices: Vec<(String, serde_json::Value)>,
    pub at: usize,
    /// One of our Extras tabs it shows on (a bundled mod's, in the rows of the extra it replaces), at row `place`.
    pub tab: String,
    pub place: Option<usize>,
    pub place_modern: Option<usize>,
    /// The interface sound its value is an offset from, played as the value changes (manifest `preview_sound`).
    pub preview: Option<u32>,
}

const OFF_ON: &[(u32, &str)] = &[(1, "ON"), (0, "OFF")];
const DEADZONES: &[(u32, &str)] = &[
    (5, "5%"),
    (10, "10%"),
    (15, "15%"),
    (20, "20%"),
    (25, "25%"),
    (30, "30%"),
    (35, "35%"),
    (40, "40%"),
    (50, "50%"),
    (60, "60%"),
];
const CAMERA_SPEEDS: &[(u32, &str)] = &[
    (25, "0.25x"),
    (50, "0.5x"),
    (75, "0.75x"),
    (100, "1x"),
    (125, "1.25x"),
    (150, "1.5x"),
    (200, "2x"),
    (250, "2.5x"),
    (300, "3x"),
    (400, "4x"),
];
/// The classic UI's scale (config ui_scale): 0 the Menu Size, 1 Auto, else a percent.
const UI_SCALES: &[(u32, &str)] = &[
    (0, "Menu Size"),
    (1, "Auto"),
    (100, "100%"),
    (125, "125%"),
    (150, "150%"),
    (175, "175%"),
    (200, "200%"),
    (250, "250%"),
    (300, "300%"),
];

const MODERN_SCALES: &[(u32, &str)] = &[
    (50, "50%"),
    (55, "55%"),
    (60, "60%"),
    (65, "65%"),
    (70, "70%"),
    (75, "75%"),
    (80, "80%"),
    (85, "85%"),
    (90, "90%"),
    (95, "95%"),
    (100, "100%"),
    (105, "105%"),
    (110, "110%"),
    (115, "115%"),
    (120, "120%"),
    (125, "125%"),
    (130, "130%"),
    (135, "135%"),
    (140, "140%"),
    (145, "145%"),
    (150, "150%"),
];

/// A post-processing number as the row's percent.
fn pct(v: f32) -> u32 {
    (v * 100.0).round().max(0.0) as u32
}

const fn row(
    label: &'static str,
    help: &'static str,
    val: Val,
    choices: &'static [(u32, &'static str)],
    when: When,
) -> SRow {
    SRow { label, help, val, choices, when }
}

/// Ours (not retail): the Settings pages are player-facing categories, not the Windows Config tool's tabs (General,
/// Screen, Textures, Effects, Sound, Misc., Client ...). Post-processing is empty until its rows arrive (see POST).
const TABS: [&str; 10] = [
    "Display",
    "Graphics",
    "Lighting & Effects",
    "Post-processing",
    "Interface",
    "Sound",
    "Misc.",
    "Controls",
    "DAT Packs",
    "Mods",
];
/// The Graphics category (the preset, quality rows): where /graphics opens.
pub const GRAPHICS: usize = 1;
/// The Sound page (its volumes are the Gameplay page's in game).
const SOUND_TAB: usize = 5;
/// The Controls page: the keyboard and mouse scheme (client/controls.rs) and the gamepad (client/gamepad.rs).
const CONTROLS: usize = 7;
/// The Post-processing category, empty until it has rows.
const POST_TAB: usize = 3;
/// The DAT Packs and Mods pages (client/modsui.rs): their rows come from the mods folder, not a table.
const PACKS: usize = 8;
const MODS: usize = 9;
const EXTRA_TABS: [&str; 4] = ["Maps", "Timers", "Battle", "Misc."];

/// `in_game`: the volumes are the Gameplay page's there, so the Sound page leaves them out. `modern`: the minimap's and
/// the Timers' place and size are set by dragging them there, so those rows are left out.
fn rows(kind: Kind, tab: usize, in_game: bool, modern: bool) -> &'static [SRow] {
    use Val::*;
    use When::*;
    // Display: the window and what it is shown on.
    const DISPLAY: &[SRow] = &[
        row(
            "Window Mode",
            "0034: full screen, a window, or borderless at the window size or the desktop's.",
            Tool("0034"),
            &[(0, "Full Screen"), (1, "Windowed"), (2, "Borderless"), (3, "Desktop")],
            Now,
        ),
        row("Resolution", "0001 / 0002: the window's size.", Size("0001", "0002", Sizes::Window), &[], Now),
        row("Monitor", "The monitor the game opens on, in every window mode (not retail).", Monitor, &[], Now),
        row(
            "GPU",
            "The graphics card to draw with; Default is the high-performance one (not retail).",
            Gpu,
            &[],
            Restart,
        ),
        row(
            "Frame Limit",
            "Maximum frames drawn a second. Choose 30 for the original cadence or 60 for smoother motion. Reduce Motion limits higher choices to 60; simulation speed stays unchanged.",
            FrameLimit,
            &[],
            Now,
        ),
        row(
            "Reduce Motion",
            "Limits rendering to 60 FPS or your lower frame limit, steadies the first-person head height, and removes effect camera shake. Off restores your saved frame limit and original camera motion.",
            ReduceMotion,
            OFF_ON,
            Now,
        ),
        row(
            "3D Aspect",
            "Window: the window's shape. Config: retail's ratio from Misc. 2 / the lobby Config.",
            RetailAspect,
            &[(0, "Window"), (1, "Config")],
            Now,
        ),
    ];
    // Graphics: the Preset first (it sets the rows below it), then the quality rows.
    const GRAPHICS_ROWS: &[SRow] = &[
        row(
            "Preset",
            "Sets the quality rows below in one step: Original is this client's defaults; High and Ultra raise texture quality and edge smoothing (Ultra also renders at 1.5x). Lighting effects are available only on Forever. Custom: set by hand. The Display rows are never changed.",
            Preset,
            &[(0, "Original"), (1, "High"), (2, "Ultra"), (3, "Custom")],
            Now,
        ),
        row(
            "Render Resolution",
            "0003 / 0004: the 3D scene's resolution, scaled to the window. 1.5x and 2x draw the scene larger than the window and scale it down: supersampled anti-aliasing, smoother edges for much more GPU work.",
            Size("0003", "0004", Sizes::Background),
            &[],
            Now,
        ),
        row(
            "Draw Distance",
            "How far scenery, characters and effects draw: Retail is the game's distance, the others farther (not retail).",
            DrawDistance,
            &[(100, "Retail"), (150, "1.5x"), (200, "2x"), (300, "3x"), (0, "Unlimited")],
            Now,
        ),
        row(
            "Textures",
            "0018: compressed (BC1 / BC2) or uncompressed textures.",
            Tool("0018"),
            &[(0, "High"), (1, "Low"), (2, "Full")],
            NextZone,
        ),
        row("MIP Mapping", "0000: lower-resolution textures on distant objects.", Tool("0000"), OFF_ON, NextZone),
        row(
            "Anisotropic Filtering",
            "Anisotropic texture filtering: sharper textures at a slant (the ground ahead).",
            Aniso,
            &[(1, "Off"), (2, "2x"), (4, "4x"), (8, "8x"), (16, "16x")],
            NextZone,
        ),
        #[cfg(feature = "forever")]
        row(
            "Forever Graphics",
            "Original preserves the original lighting. Enhanced adds sun shadows, per-pixel lamps, soft occlusion, restrained bloom, atmosphere and sharpening, with 16x filtering and native resolution. Changes wait for Apply; Cancel keeps your current look. Custom keeps existing individual choices.",
            ForeverLook,
            &[(0, "Original"), (1, "Enhanced"), (2, "Custom")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Upscaler",
            "Standard scales the 3D scene to the window as before. FSR 1 (AMD FidelityFX Super Resolution 1.0, spatial) draws the 3D scene at a smaller size and sharpens it back up to the window: a faster frame for a little softness. FSR 2 (AMD, temporal) also uses the scene's motion over several frames to rebuild a sharper picture from the smaller render, working on any GPU; it can leave faint trails on fast particles. Either replaces Render Resolution while on, and sharpening comes from the upscaler: FSR 1 sharpens at a fixed strength and FSR 2 at the Sharpening setting (0 = none). Menus and text stay at the window's size. In the world only (not retail).",
            Upscaler,
            &[(0, "Standard"), (1, "FSR 1"), (2, "FSR 2")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Upscaler Quality",
            "How much smaller the upscaler draws the scene: Ultra Quality 1.3x, Quality 1.5x, Balanced 1.7x, Performance 2x less per side. Higher quality looks closer to native; Performance saves the most GPU work. Used only while the Upscaler is on.",
            UpscaleQuality,
            &[(0, "Ultra Quality"), (1, "Quality"), (2, "Balanced"), (3, "Performance")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Heavens Tower Textures",
            "Original keeps your installed art. Tower Preview covers the main hall stone floors, lower masonry and tall ashlar walls. Decorative bands, plants and separate room interiors remain unchanged. Requires Forever Graphics: Enhanced. Apply, then re-enter the zone or relog voluntarily; switching back also takes effect at the next zone load. Custom texture mods and all other scenery stay unchanged.",
            EnvironmentTextures,
            &[(0, "Original"), (1, "Tower Preview")],
            NextZone,
        ),
        #[cfg(feature = "forever")]
        row(
            "Windurst Woods Textures",
            "Original keeps your installed art. Masonry Preview replaces one audited residential entrance material with new stone artwork. Requires Forever Graphics: Enhanced. Apply, then load the zone again voluntarily. This is separate from Tower Preview; custom textures remain unchanged.",
            WoodsTextures,
            &[(0, "Original"), (1, "Masonry Preview")],
            NextZone,
        ),
        row(
            "2-D Mapping",
            "0019: map images compressed (DXT3) or at full quality.",
            Tool("0019"),
            &[(0, "Compressed"), (1, "Full")],
            NextZone,
        ),
        row(
            "Fonts",
            "0036: the font texture quality.",
            Tool("0036"),
            &[(0, "Compressed"), (1, "Full"), (2, "High")],
            Restart,
        ),
    ];
    // Lighting & Effects.
    const LIGHTING: &[SRow] = &[
        row(
            "Lamp Lights",
            "Baked: the lamplight painted onto the walls, as retail. Real: point lights at the lamps (not retail).",
            LampLights,
            &[(0, "Baked"), (1, "Real")],
            Now,
        ),
        row(
            "Bump Mapping",
            "0017: normal-mapped scenery; off lights it with the sun and moon together.",
            Tool("0017"),
            OFF_ON,
            NextZone,
        ),
        row(
            "Environment Animation",
            "0011: swaying vegetation (Normal 16 steps, Smooth 32).",
            Tool("0011"),
            &[(0, "Off"), (1, "Normal"), (2, "Smooth")],
            Now,
        ),
        row("Gamma", "0028: base gamma, added to the Gameplay page's R / G / B.", Gamma, &[], Now),
        row(
            "Graphics Stabilization",
            "0040: On interpolates the shine overlay's coordinates plainly (no seam wrap).",
            Tool("0040"),
            OFF_ON,
            Now,
        ),
        row("3D LCD Mode", "0030: side-by-side stereo for 3D screens.", Tool("0030"), OFF_ON, Now),
        // Upstream's optional graphics (Settings > Graphics there); a Forever Enhanced setting like the rows above.
        #[cfg(feature = "forever")]
        row(
            "Character Shadows",
            "Retail gives characters the game's simple shadows. Real makes characters cast proper shadows from the sun and moon, and from lamps when Lamp Lights is Real (not retail).",
            ActorShadows,
            &[(0, "Retail"), (1, "Real")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Shadow Quality",
            "How sharp Real shadows are. Higher is sharper and uses more video memory (not retail).",
            ShadowMap,
            &[(1024, "Low"), (2048, "Medium"), (4096, "High")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Sun Shadow Range",
            "How far the Enhanced sun shadows reach around you. Near is the original 40 units; Wide covers four times the ground at the same sharpness (16 MB of video memory, 36 MB with Soft Shadows); Far reaches 120 units (64 MB, for strong graphics cards). Used with Forever Graphics: Enhanced (not retail).",
            ShadowRange,
            &[(0, "Near"), (1, "Wide"), (2, "Far")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Far View",
            "Takes the game's Clipping past 100% in open-air zones: scenery draws farther and the fog that hides the horizon moves out with it, so distant hills and buildings stay in view. Far is 1.5x, Very Far 2x; costs more drawing. Interiors and towers are unchanged. Used with Forever Graphics: Enhanced (not retail).",
            FarView,
            &[(0, "Retail"), (1, "Far"), (2, "Very Far")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Soft Shadows",
            "Soft, filtered edges on the Enhanced sun shadows (they also fall on characters), with a finer shadow map (Near 16 MB, Wide 36 MB, Far unchanged at 64 MB of video memory) and a gentler bias so shadows stay attached at the base of walls and feet. Off keeps the original crisp 3 by 3 filter. Used with Forever Graphics: Enhanced (not retail).",
            SoftShadows,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Shadow Cascades",
            "Three nested sun shadow maps around you instead of one: sharp shadows close by (a 24 unit radius), more of the world in the middle, and shadows reaching 200 units out, blending into each other. Uses the soft shadow filter and 48 MB of video memory (12 MB when the card is short); with no room for them the single map stays (the graphics memory notice appears). Needs Forever Graphics: Enhanced (not retail).",
            ShadowCascades,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Character Shade",
            "Characters, NPCs and monsters darken a little more where the scenery's sun shadow falls on them, under a tree or a roof, instead of keeping their full light; it fades out toward night, where the soft character light keeps them readable. Used with Forever Graphics: Enhanced (not retail).",
            ActorShade,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Basic Ambient Occlusion",
            "Adds soft shade in corners and creases of the world, so buildings look more solid (not retail). The Post-processing page's Ambient Occlusion is Forever's own, with strength and radius; while it is on, this one stands aside so corners are not darkened twice.",
            Ssao,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Colour Grading",
            "Gives the 3D world a colour tint. Menus and text keep their colours (not retail).",
            Grading,
            &[(0, "Off"), (1, "Vivid"), (2, "Warm"), (3, "Cool"), (4, "Film")],
            Now,
        ),
        row(
            "Anti-aliasing",
            "Smooths jagged edges in the 3D world with SMAA. Menus and text are left alone (not retail). For even smoother edges, set Render Resolution higher than the window.",
            Antialias,
            &[(0, "Off"), (1, "SMAA")],
            Now,
        ),
    ];
    // Post-processing: one `row(...)` line each when the passes arrive (anti-aliasing, bloom, tone mapping, sharpening).
    const POST: &[SRow] = &[
        row(
            "Anti-aliasing (FXAA)",
            "Smooths jagged edges on the 3D scene, cheaply (not retail). Render Resolution 1.5x / 2x is the costlier, sharper way.",
            Fxaa,
            OFF_ON,
            Now,
        ),
        row(
            "Bloom",
            "A soft glow around the brightest parts of the scene: the sun, lamps, spells (not retail).",
            Bloom,
            OFF_ON,
            Now,
        ),
        row(
            "Bloom Intensity",
            "How strong the glow is.",
            BloomIntensity,
            &[
                (10, "10%"),
                (20, "20%"),
                (30, "30%"),
                (40, "40%"),
                (50, "50%"),
                (75, "75%"),
                (100, "100%"),
                (150, "150%"),
                (200, "200%"),
            ],
            Now,
        ),
        row(
            "Bloom Threshold",
            "How bright a part must be to glow; higher keeps it to the very brightest.",
            BloomThreshold,
            &[(60, "60%"), (70, "70%"), (75, "75%"), (80, "80%"), (85, "85%"), (90, "90%"), (95, "95%")],
            Now,
        ),
        row(
            "Tone Mapping",
            "A filmic colour curve: richer contrast in highlights and shadows (not retail).",
            Tonemap,
            OFF_ON,
            Now,
        ),
        row(
            "Exposure",
            "The scene's brightness under Tone Mapping.",
            Exposure,
            &[
                (50, "0.5x"),
                (75, "0.75x"),
                (90, "0.9x"),
                (100, "1.0x"),
                (110, "1.1x"),
                (125, "1.25x"),
                (150, "1.5x"),
                (200, "2.0x"),
            ],
            Now,
        ),
        row(
            "Sharpening",
            "Contrast-adaptive sharpening: crisper textures and edges (not retail).",
            Sharpen,
            &[
                (0, "Off"),
                (10, "10%"),
                (20, "20%"),
                (30, "30%"),
                (40, "40%"),
                (50, "50%"),
                (60, "60%"),
                (80, "80%"),
                (100, "100%"),
            ],
            Now,
        ),
        row(
            "Saturation",
            "Colour strength: 100% is as drawn.",
            Saturation,
            &[(50, "50%"), (75, "75%"), (90, "90%"), (100, "100%"), (110, "110%"), (125, "125%"), (150, "150%")],
            Now,
        ),
        row(
            "Ambient Occlusion",
            "Soft contact shadows where surfaces meet: under trees, at walls and ledges (SSAO; not retail).",
            Ao,
            OFF_ON,
            Now,
        ),
        row(
            "AO Strength",
            "How dark the contact shadows are.",
            AoStrength,
            &[(50, "50%"), (75, "75%"), (100, "100%"), (130, "130%"), (160, "160%"), (200, "200%")],
            Now,
        ),
        row(
            "AO Radius",
            "How far the shadows reach from a corner.",
            AoRadius,
            &[(50, "Small"), (80, "0.8"), (120, "Medium"), (150, "1.5"), (200, "Large"), (300, "3")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "AO Applies To",
            "All Light darkens everything in a corner, sunlit or not. Ambient Only darkens just the soft ambient light, so lit walls and floors keep their sun and lamp light and only the shaded creases deepen. Needs Ambient Occlusion on (not retail).",
            AoLight,
            &[(0, "All Light"), (1, "Ambient Only")],
            Now,
        ),
        row(
            "Height Fog",
            "Low-lying mist that thickens near the ground and with distance, in the zone's own fog colour (not retail).",
            Fog,
            OFF_ON,
            Now,
        ),
        row(
            "Fog Density",
            "How thick the mist is.",
            FogDensity,
            &[(30, "30%"), (50, "50%"), (80, "80%"), (100, "100%"), (130, "130%"), (160, "160%"), (200, "200%")],
            Now,
        ),
        row(
            "Fog Height",
            "How high the mist reaches above your feet.",
            FogHeight,
            &[(5, "Low"), (10, "10"), (20, "Medium"), (40, "40"), (80, "High"), (150, "Very High")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Layered Fog",
            "The mist follows the time of day and the weather: thicker at dawn and dusk, thicker and taller in mist, rain, storms, snow and fog, and eased as it changes. It keeps each zone's own fog colour and only works with Height Fog on (not retail).",
            FogRamp,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Fog Layers",
            "A second, volume-style layer on the Height Fog: a thin mist that hugs the ground and thickens at dawn and dusk and in wet weather, a warm glow in the haze toward the sun, and a distance haze that carries it. Needs Height Fog on (not retail).",
            FogLayers,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Enhanced Water",
            "Finishes wading water (rivers, canals and the sea at the shore) with two drifting layers of ripples, a sky reflection that grows toward the horizon, a sun glint that follows the zone's own sun and a foam edge where the water meets the bank. Water drawn as scenery is unchanged. Shown only with the Enhanced look selected (the plain river finish is Enhanced-only too; not retail).",
            WaterEnhanced,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Sky Clouds",
            "Thickens the sky in heavy weather (cloudy, rain, storms, snow, fog, dust): soft wind-drifting cloud lit by the sun, greyer and heavier the worse it gets, fading into the horizon haze. Clear and fine weather, and indoor zones, are left exactly as the zone has them; the retail weather clouds still draw over it. Shown only with the Enhanced look selected (not retail).",
            SkyClouds,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Zone Lighting",
            "Re-grades each zone's own light toward Forever's look: a warmer sun that turns golden near the horizon, cooler and richer shade, a deeper sky, a haze that is cool by day and gold at dawn and dusk, and nights that keep their colour. Directions, shadows and distances are unchanged (not retail).",
            LightGrade,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Zone Lighting Strength",
            "How strongly Zone Lighting re-grades the zone's own light. 100% is as authored.",
            LightGradeAmount,
            &[(25, "25%"), (50, "50%"), (75, "75%"), (100, "100%"), (125, "125%"), (150, "150%")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Sun Shafts",
            "Soft rays of light from a low sun through trees, arches and buildings: strongest at dawn and golden hour, never at night, indoors, with the sun off screen or in heavy rain, storm or fog. Drawn in the picture from where the sun is and what is in front of it (not retail).",
            SunShafts,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Sun Shafts Strength",
            "How strong the Sun Shafts are. 100% is as tuned; lower is subtler.",
            SunShaftsAmount,
            &[(25, "25%"), (50, "50%"), (75, "75%"), (100, "100%"), (125, "125%"), (150, "150%")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Volumetric Fog",
            "The sun's light scattered by the fog along the line of sight, marched through the fog's own thickness and the sun's shadows: a warm glow toward a low sun, and shafts of light coming out of the fog under trees and arches. It adds light on top of Height Fog (which it needs on) and Fog Layers; the fog's thickness is still Fog Density, Layered Fog and Fog Layers. Strongest at dawn and golden hour; none at night, indoors or in heavy rain, storm or fog (not retail).",
            VolumetricFog,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Volumetric Fog Strength",
            "How strong the Volumetric Fog's light is. 100% is as tuned; lower is subtler.",
            VolumetricFogAmount,
            &[(25, "25%"), (50, "50%"), (75, "75%"), (100, "100%"), (125, "125%"), (150, "150%")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Detail Textures",
            "Adds a fine, colour-neutral grain to the ground, floors, walls and rock up close, anchored in the world so it does not stretch with the texture. It fades with distance, never touches characters, water or see-through surfaces, and the original textures stay as they are (not retail).",
            DetailTextures,
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Detail Strength",
            "How strong the Detail Textures grain is. 100% is as tuned; lower is subtler.",
            DetailStrength,
            &[(25, "25%"), (50, "50%"), (75, "75%"), (100, "100%"), (150, "150%"), (200, "200%")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Texture Sharpening",
            "Where a ground, floor, wall or rock texture is stretched and blurry up close, redraws it with a smoother, slightly crisper reconstruction instead of plain blurring, kept within the texture's own colours. It does not add detail the texture never had, and it never touches textures that are already sharp, characters, water or see-through surfaces (not retail).",
            TextureSharpening,
            &[(0, "Off"), (50, "Low"), (100, "Medium"), (130, "High")],
            Now,
        ),
        row(
            "Character Light",
            "A soft fill light on characters and monsters in the dark, so the tone curve, occlusion and fog do not sink them; daylight is unchanged. Full with Tone Mapping, a little with only occlusion or fog (not retail).",
            CharacterLight,
            &[(0, "Off"), (25, "25%"), (50, "50%"), (75, "75%"), (100, "100%")],
            Now,
        ),
    ];
    const PLACES_ROW: SRow = row(
        "Panel Places",
        "Reset puts this character's modern windows back where they start (not retail).",
        ResetPlaces,
        &[(0, "As Moved"), (1, "Reset")],
        Now,
    );
    const EXPERIENCE_ROW: SRow = row(
        "Experience",
        "Modern: the new interface, WASD and mouse controls, the opening movie skipped. Original: the game as retail had it, its own windows, retail's keyboard, the Original graphics preset and the opening movie. Each setting below can still be changed on its own (not retail).",
        Experience,
        &[(0, "Modern"), (1, "Original"), (2, "Custom")],
        Now,
    );
    const INTERFACE_HEAD: [SRow; 5] = [
        row(
            "Interface",
            "Classic: the game's own windows. Modern: the new interface's windows where it has them (not retail).",
            ModernUi,
            &[(0, "Classic"), (1, "Modern")],
            Now,
        ),
        row(
            "UI Scale",
            "How big the game's windows and text are: Auto grows them with the window past 1080 lines; Menu Size uses the size below, as retail (not retail).",
            UiScale,
            UI_SCALES,
            Now,
        ),
        row(
            "Menu Size",
            "0037 / 0038: the resolution menus and text are laid out at, then stretched (with UI Scale on Menu Size).",
            Size("0037", "0038", Sizes::Ui),
            &[],
            Now,
        ),
        row("Interface Size", "The modern interface's size (not retail).", ModernScale, MODERN_SCALES, Now),
        row(
            "Panel Opacity",
            "How opaque the modern interface's window backgrounds are (not retail).",
            ModernOpacity,
            &[],
            Now,
        ),
    ];
    const PARTY_TP_ROW: SRow =
        row("Party TP", "Show TP in the party window (retail config 0xae, off there).", PartyTp, OFF_ON, Now);
    // Panel Places (a character's) is for in game only.
    const INTERFACE: &[SRow] = &[
        EXPERIENCE_ROW,
        INTERFACE_HEAD[0],
        INTERFACE_HEAD[1],
        INTERFACE_HEAD[2],
        INTERFACE_HEAD[3],
        INTERFACE_HEAD[4],
        PLACES_ROW,
        PARTY_TP_ROW,
        MINIMAP_ROWS[0],
        MINIMAP_ROWS[1],
        MINIMAP_ROWS[2],
        JOB_PANEL_ROW,
        EXP_BAR_ROW,
        #[cfg(feature = "forever")]
        QUEST_TRACKER_ROW,
        #[cfg(feature = "forever")]
        QUEST_INDICATORS_ROW,
        #[cfg(feature = "forever")]
        AUTO_NAV_ROW,
        #[cfg(feature = "forever")]
        TRAINING_HUD_ROW,
        #[cfg(feature = "forever")]
        AUTO_ROTATION_ROW,
        #[cfg(feature = "forever")]
        JOURNAL_KEY_ROW,
        #[cfg(feature = "forever")]
        AUTO_EQUIP_ROW,
        #[cfg(feature = "forever")]
        COMPACT_HUD_ROW,
        #[cfg(feature = "forever")]
        CON_COLORS_ROW,
        #[cfg(feature = "forever")]
        AUTO_CHECK_ROW,
        #[cfg(feature = "forever")]
        NAV_MESH_ROW,
        #[cfg(feature = "forever")]
        RECIPE_KEY_ROW,
        SOLO_ROW,
        INV_ROWS[0],
        INV_ROWS[1],
        ROT_ROWS[0],
        ROT_ROWS[1],
        ROT_ROWS[2],
        ROT_ROWS[3],
    ];
    const INV_ROWS: [SRow; 2] = [
        row(
            "Keep Bags Sorted",
            "Your inventory, satchel, sack, case and Mog House storage sort themselves again when an item comes in (each bag's sort window's Auto / Manual sets one bag).",
            KeepSorted,
            OFF_ON,
            Now,
        ),
        row(
            "Combine Stacks",
            "A new item joins the partial stacks of the same item in its bag (the server's own merge), even where the bag is not kept sorted.",
            Combine,
            OFF_ON,
            Now,
        ),
    ];
    const ROT_ROWS: [SRow; 4] = [
        row(
            "Adaptive Suggestions",
            "The battle menu's Suggested rows and the one-button rotation (/rot) react to the fight: cure who is low, lift ailments, put back what wears off, Convert when MP runs low. Off: only the fight's own actions (debuffs, weapon skills, damage).",
            RotAdaptive,
            OFF_ON,
            Now,
        ),
        row(
            "Cure Below",
            "Suggest a cure when you or a party member is at or under this much HP.",
            RotCure,
            &[(40, "40%"), (50, "50%"), (60, "60%"), (70, "70%"), (75, "75%"), (80, "80%"), (90, "90%")],
            Now,
        ),
        row(
            "Convert Below",
            "Suggest Convert (Red Mage) when your MP is under this much (and your HP is high).",
            RotConvert,
            &[(10, "10%"), (15, "15%"), (20, "20%"), (25, "25%"), (30, "30%"), (40, "40%")],
            Now,
        ),
        row(
            "Renew Buffs",
            "Suggest putting an effect back when it has this little time left.",
            RotRenew,
            &[(10, "10 s"), (20, "20 s"), (30, "30 s"), (45, "45 s"), (60, "60 s"), (90, "90 s")],
            Now,
        ),
    ];
    const JOB_PANEL_ROW: SRow = row(
        "Job Panel",
        "The modern interface's panel under the minimap by your main job (not retail): Red Mage buff timers and MP, White Mage party HP and ailments, Black Mage MP and nuke recasts, the melee jobs' TP and ability recasts, else who is near. Jobs with Wide Scan (Ranger, Beastmaster) keep Wide Scan there. Display only.",
        JobPanel,
        OFF_ON,
        Now,
    );
    const EXP_BAR_ROW: SRow = row(
        "EXP Bar",
        "The modern interface's EXP bar at the foot of the party window (FFXI Forever; not retail): your level, EXP and EXP to the next level as the server sent them, with the percent, and the limit and merit points once you earn them. The Equipment window always shows it. Display only.",
        ExpBar,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const JOURNAL_KEY_ROW: SRow = row(
        "Journal Key",
        "Forever Modern controls: J opens the Journal (missions, quests, key items) from the field. Like L for the tracker, it takes over only the stock J camera key; anything you assigned to J yourself keeps it. Escape or Back closes the Journal.",
        JournalKey,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const AUTO_EQUIP_ROW: SRow = row(
        "Auto-equip New Upgrades",
        "Forever Modern: when you obtain a piece that is clearly better than what you wear (every stat it has is at least as good, none worse, for your job's role), put it on while you stand idle. Never in combat, events or while casting, and never your weapons, shield, Range or Ammo. Hybrid jobs need a role chosen in the Equipment window. Off by default and never turned on for you, because it changes what your character wears without asking; the Equipment window's Equip Best button works either way.",
        AutoEquipUpgrades,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const COMPACT_HUD_ROW: SRow = row(
        "Compact HUD",
        "Forever Modern: a smaller HUD that takes less of the screen: tighter panel margins, a narrower quest tracker and chat window, and a fold button on each panel's title (the tracker, party, chat, minimap and job helper) that shrinks it to its title bar. Ctrl+Shift+H (the Minimal HUD key) folds every panel at once and brings them back. Panels you moved or resized keep their place and size; Off restores the full layout. HP, MP, TP, the target, the navigation arrow with its Stop button and the chat line while you type always stay shown.",
        CompactHud,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const CON_COLORS_ROW: SRow = row(
        "Monster Name Colours",
        "Forever: monster names show how tough they are for you, as in EverQuest: grey too weak, green easy prey, light blue a decent challenge, white an even match, yellow tough, orange very tough, red incredibly tough. Each colour is the server's own /check answer, kept for the session and redone after a level or job change. Claimed monsters keep their claim colours. Off gives the usual colours.",
        ConColors,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const AUTO_CHECK_ROW: SRow = row(
        "Check Monsters I Target",
        "Forever: when you target a hostile monster whose colour is not known yet, one /check goes out by itself (at most one every 1.5 seconds), and its answer stays out of the log. Typed or menu checks always show. Needs Monster Name Colours.",
        AutoCheckTarget,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const NAV_MESH_ROW: SRow = row(
        "Navigation Mesh",
        "Forever: Auto Navigation plans its routes on a walkable grid built from your own game files the first time it is needed in a zone (a few seconds in the background; it is kept in your cache folder and never leaves your computer). Routes are still checked against the real collision, and anything the grid cannot answer falls back to the ordinary planner. On by default; turn it off if travel ever behaves worse.",
        NavMesh,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const RECIPE_KEY_ROW: SRow = row(
        "Crafting Log Key",
        "Forever Modern controls: This key (U unless changed) opens the Crafting Log (recipes by craft and level, with your crystal and materials) from the field. Only keys the stock key tables leave free are offered; if you assign the key to something yourself, that assignment wins.",
        RecipeLogKey,
        &[(0, "OFF"), (0x16, "U"), (0x18, "O"), (0x19, "P"), (0x22, "G"), (0x30, "B")],
        Now,
    );
    #[cfg(feature = "forever")]
    const QUEST_TRACKER_ROW: SRow = row(
        "Quest Tracker",
        "Forever Modern UI quest and mission steps, numbered map markers and minimap guidance. L toggles in Modern controls when no custom L binding or window has the keys. Active server objectives are checked by default; click a name to guide to it. On by default; turn it off here to hide the guidance.",
        QuestTracker,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const QUEST_INDICATORS_ROW: SRow = row(
        "Quest Indicators",
        "Forever: a book marks each NPC the server reports as having a quest available to you, on the world and the map. Special's Quest Indicators changes this same setting. Only what the server reports is marked, so no book does not mean there is no quest. On by default.",
        QuestIndicators,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const AUTO_NAV_ROW: SRow = row(
        "Auto Navigation",
        "Forever: the navigation helper, as Special's Auto Navigation. On by default, but turning it on never moves your character: only Start on the navigation arrow does, and only along a clear, known approach, and any movement key stops it. Turn it off to remove the Start button.",
        AutoNavigation,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const AUTO_ROTATION_ROW: SRow = row(
        "Auto Rotation at Start",
        "Forever: whether each session starts with the one-button rotation acting by itself while you fight. Special's Auto Rotation and /autocast change only the current session. On by default.",
        AutoRotationStart,
        OFF_ON,
        Now,
    );
    #[cfg(feature = "forever")]
    const TRAINING_HUD_ROW: SRow = row(
        "Training Tracker",
        "Forever Modern UI: a small panel with the server's authoritative training progress (the Training menu's toggle changes this same setting). On by default; turn it off to hide the panel.",
        TrainingHud,
        OFF_ON,
        Now,
    );
    const SOLO_ROW: SRow = row(
        "Solo Mode Label",
        "A small Solo label on the solo-mode icon in your effects while Banshee's solo mode is on (!solomode); its tooltip lists the perks.",
        SoloBadge,
        OFF_ON,
        Now,
    );
    const MINIMAP_ROWS: [SRow; 3] = [
        row(
            "Minimap",
            "The modern interface's minimap: the zone around you, north up, with your arrow, your party and what is near; drag it to an edge, the wheel zooms, a corner resizes it. On by default (not retail).",
            ModernMinimap,
            OFF_ON,
            Now,
        ),
        row(
            "Minimap Size",
            "How big the minimap starts (a corner drag sets its own size).",
            MinimapSize,
            &[(0, "Small"), (1, "Medium"), (2, "Large")],
            Now,
        ),
        row(
            "Maps You Don't Own",
            "On: the minimap shows the zone's map even without its map key item. Off: as retail, only maps you own (a radar of what is near otherwise).",
            MinimapUnowned,
            OFF_ON,
            Now,
        ),
    ];
    const INTERFACE_LOBBY: &[SRow] =
        &[INTERFACE_HEAD[0], INTERFACE_HEAD[1], INTERFACE_HEAD[2], INTERFACE_HEAD[3], INTERFACE_HEAD[4], PARTY_TP_ROW];
    const SOUND: &[SRow] = &[
        row("Sound", "Turns all sound on or off.", Tool("0007"), OFF_ON, Now),
        row(
            "Sound Effect Channels",
            "How many sound effects can play at the same time. More means fewer sounds are cut off.",
            Tool("0029"),
            &[
                (12, "12"),
                (13, "13"),
                (14, "14"),
                (15, "15"),
                (16, "16"),
                (17, "17"),
                (18, "18"),
                (19, "19"),
                (20, "20"),
            ],
            Now,
        ),
        row("Sound in Background", "Keeps playing sound while you are in another window.", Tool("0035"), OFF_ON, Now),
        // The title's one volume (lobby config 0xa8); in game the Gameplay page has them (rows left out there).
        row(
            "Music Volume",
            "Music volume at the title screen. In game, use the sliders on the Gameplay page.",
            Music,
            &[],
            Now,
        ),
        row(
            "Sound Effects Volume",
            "Sound effects volume at the title screen. In game, use the sliders on the Gameplay page.",
            Effects,
            &[],
            Now,
        ),
    ];
    const MISC: &[SRow] = &[
        row(
            "Hardware Mouse",
            "Uses your system's mouse pointer with the game's cursor images, instead of a cursor the game draws.",
            Tool("0021"),
            OFF_ON,
            Now,
        ),
        row(
            "Cursor Size",
            "How big the game's cursor is, when Hardware Mouse is Off. Auto makes it bigger in bigger windows (not retail).",
            CursorSize,
            &[(0, "Auto"), (100, "1x"), (150, "1.5x"), (200, "2x"), (300, "3x"), (400, "4x")],
            Now,
        ),
        row(
            "Keys Hide the Cursor",
            "On, as the game does, a key press hides the mouse cursor until the mouse moves. Off keeps it shown while you walk with the keys (not retail).",
            KeysHideCursor,
            OFF_ON,
            Now,
        ),
        row(
            "Opening Movie",
            "Plays the opening movie before the title screen. Any key skips it.",
            Tool("0022"),
            OFF_ON,
            Restart,
        ),
        row(
            "Simple Character Creation",
            "Shows flat pictures instead of 3D models during character creation.",
            Tool("0023"),
            OFF_ON,
            Now,
        ),
        row(
            "Screenshot Size",
            "Also saves each screenshot at your screen's size, in ffxiuser/screenshots.",
            Tool("0043"),
            OFF_ON,
            Now,
        ),
        row(
            "Send Rate",
            "How often the client sends to a private server, which answers each time: faster brings what happens around you sooner. A PlayOnline sign-in keeps the game's rate (not retail).",
            SendRate,
            &[(400, "Retail"), (200, "2x"), (100, "4x"), (50, "8x")],
            Now,
        ),
    ];
    const CONTROLS_ROWS: &[SRow] = &[
        row(
            "Keyboard & Mouse",
            "Modern: WASD and the mouse as in WoW. W / S walk, A / D strafe, Q / E turn; hold the right button to look (the character faces where the camera looks), both buttons run, the left button turns the camera, a left click targets, a right click on someone talks to them or opens the action menu, R is autorun. FFXI has no jumping; Space opens the chat line. FFXI WASD: retail's keyboard type 2 (Q / E turn, Shift faces the camera). Original: retail's keyboard as the game Config's Keyboard type sets it. Forever (FFXI Forever only): Modern, and a quick right click (a tap, not a look drag) backs out of the open menu or window one level, as Escape and the Back button do; it never closes Settings with unsaved changes. A right click on something that already uses it (a list row's menu) keeps that use, and a right click on a character with nothing open talks to them as in Modern. The keys follow Key Assignment (not retail).",
            Controls,
            &[(2, "Modern"), (1, "FFXI WASD"), (0, "Original"), (3, "Forever")],
            Now,
        ),
        row(
            "A / D Keys",
            "Modern: A / D strafe (Q / E turn), or turn (Q / E strafe). While the right button is held all four strafe.",
            ControlsAd,
            &[(0, "Strafe"), (1, "Turn")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Invert Horizontal Mouse",
            "Off: moving the mouse right turns the view right. Hold the right button to turn the character too. On reverses horizontal mouse look; keyboard and gamepad camera controls stay unchanged.",
            InvertHorizontalMouse,
            OFF_ON,
            Now,
        ),
        row(
            "Gamepad",
            "A connected gamepad plays the game: the left stick walks, the right one turns the camera, the buttons are the keys' (not retail). Off, a pad does nothing.",
            Gamepad,
            OFF_ON,
            Now,
        ),
        row(
            "Stick Deadzone",
            "How far a stick must be pushed before it counts; raise it if the character or camera drifts on its own.",
            GamepadDeadzone,
            DEADZONES,
            Now,
        ),
        row(
            "Camera Speed",
            "How fast the right stick turns the camera, against the keys' speed.",
            GamepadCamera,
            CAMERA_SPEEDS,
            Now,
        ),
        row(
            "Mouse Camera Speed",
            "How fast the camera turns when you hold a mouse button and move the mouse. Lower it if the camera spins too fast. Controllers are not affected (not retail).",
            MouseCamSpeed,
            &[(25, "25%"), (50, "50%"), (75, "75%"), (100, "100%"), (150, "150%"), (200, "200%")],
            Now,
        ),
    ];
    const MAPS: &[SRow] = &[
        row(
            "Map Overlay",
            "Ctrl / Alt + O shows the zone map over the screen, as the map add-ons do. Off by default because it draws over the scene.",
            MapOverlay,
            OFF_ON,
            Now,
        ),
        row("Map Overlay Opacity", "How opaque the map overlay is.", MapOpacity, &[], Now),
        #[cfg(feature = "forever")]
        row(
            "M Map Placement",
            "Forever area maps: original layout, a large centered square, or a panel that follows the mouse while your character moves. It stays still when you stop so you can click it. Event and warp maps keep their original controls.",
            MapView(0),
            &[(0, "Original"), (1, "Centered"), (2, "Follow Mouse")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "M Map Size",
            "Size of the centered or following map, as a percentage of the screen's shorter side.",
            MapView(1),
            &[(40, "40%"), (50, "50%"), (60, "60%"), (70, "70%"), (80, "80%"), (90, "90%")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "M Map Opacity",
            "Map parchment opacity while standing still; markers remain readable. Original layout ignores this.",
            MapView(2),
            &[(20, "20%"), (40, "40%"), (60, "60%"), (80, "80%"), (100, "100%")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Moving Map Opacity",
            "Map parchment opacity while moving, so your character and the world remain visible.",
            MapView(3),
            &[(10, "10%"), (25, "25%"), (40, "40%"), (60, "60%"), (80, "80%"), (100, "100%")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "M Map Track Player",
            "Keep your live player marker in view. A left drag pauses tracking until the recenter key or the map is reopened. This does not move your character or the mouse.",
            MapView(4),
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Waypoint Pins",
            "Right-click an owned area map to set or replace your personal waypoint; right-click the pin again to clear it. Shown on the map and minimap. Kept across zones until cleared or logout; no automatic travel.",
            MapView(5),
            OFF_ON,
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Navigation Arrow",
            "Forever Modern UI: movable camera-relative arrow with distance in yalms. Auto follows your personal pin while set, otherwise the selected tracked objective. Remote objectives guide to a known zone exit. Direction only, not a walkable path or automatic travel.",
            MapView(6),
            &[(0, "OFF"), (1, "Objective"), (2, "Waypoint"), (3, "Auto")],
            Now,
        ),
        #[cfg(feature = "forever")]
        row(
            "Navigation Arrow Size",
            "Size of the navigation arrow; new profiles start at 50%. Saved sizes stay unchanged. Drag its panel to move it; position is remembered per character.",
            MapView(7),
            &[(50, "50%"), (75, "75%"), (100, "100%"), (125, "125%"), (150, "150%")],
            Now,
        ),
        row(
            "Minimap",
            "A small map in a corner that follows you and shows what is nearby. Scroll over it to zoom. FFXI Forever turns it on by default.",
            Minimap,
            OFF_ON,
            Now,
        ),
        row(
            "Minimap Position",
            "Where the minimap stands: the top right (beside the menus while one is open), or the top left under the cast bar.",
            MinimapDock,
            &[(0, "Top Right"), (1, "Top Left")],
            Now,
        ),
        row("Minimap Size", "How big the minimap is.", MinimapSize, &[(0, "Small"), (1, "Medium"), (2, "Large")], Now),
    ];
    const TIMERS_ROW: SRow = row(
        "Timers",
        "Shows spell and ability recasts, your effects and the ones you put on others and on monsters, in a window that slides in and out (/timers). FFXI Forever turns it on by default.",
        Timers,
        OFF_ON,
        Now,
    );
    const REGIME_ROW: SRow = row(
        "Training Regime",
        "While a Fields or Grounds of Valor regime is active, shows its targets and your kills in a window beside the timers. FFXI Forever turns it on by default.",
        Regime,
        OFF_ON,
        Now,
    );
    const SKILLCHAIN_ROW: SRow = row(
        "Skillchain Helper",
        "In a party, shows which skillchains your party can make together. On your target, it shows an open skillchain, what closes it, and when to magic burst. FFXI Forever turns it on by default.",
        Skillchains,
        OFF_ON,
        Now,
    );
    const TIMERS: &[SRow] = &[
        TIMERS_ROW,
        row(
            "Timers Position",
            "Docks the timers window on the left, on the right above the party list, or at the top.",
            TimersDock,
            &[(0, "Left"), (1, "Right"), (2, "Top")],
            Now,
        ),
        REGIME_ROW,
        SKILLCHAIN_ROW,
    ];
    const TIMERS_MODERN: &[SRow] = &[TIMERS_ROW, REGIME_ROW, SKILLCHAIN_ROW];
    // A page holds 7 rows: what touches battle has its own.
    const EXTRA_BATTLE: &[SRow] = &[
        row("Target Distance", "Shows how far away your target is in the target window.", TargetDistance, OFF_ON, Now),
        row(
            "Target HP %",
            "Shows your target's HP percent next to its HP bar in the classic target window. The modern one always shows it.",
            TargetHpp,
            OFF_ON,
            Now,
        ),
        row(
            "Trust Speech",
            "Shows or hides what your trusts say in the chat log. Shown by default: hiding removes lines from your log.",
            TrustSpeech,
            &[(0, "Shown"), (1, "Hidden")],
            Now,
        ),
        row(
            "Ready Lines",
            "Shows every \"readies\" and \"starts casting\" line, or only yours and monsters' so you see what is coming at you. All lines by default: the filter removes lines from your log.",
            ReadyLines,
            &[(0, "All"), (1, "Mine, Monsters'")],
            Now,
        ),
        row(
            "Battle Text",
            "Shows battle lines the way the game words them, or shortened, like \"Name [Fast Blade] → Wild Rabbit: 123\".",
            ShortBattle,
            &[(0, "Retail"), (1, "Short")],
            Now,
        ),
        row(
            "Battle Names",
            "Shows names in battle lines in one colour, or coloured by who they are: you, your party, your alliance, monsters you claimed, and others' claims.",
            NameColours,
            &[(0, "Plain"), (1, "Coloured")],
            Now,
        ),
    ];
    const EXTRA_MISC: &[SRow] = &[
        row(
            "Synthesis Amount",
            "After you press Synthesize, asks how many to make and crafts them one after another. Off by default: it repeats crafts for you.",
            SynthAmount,
            OFF_ON,
            Now,
        ),
        row(
            "Timestamps on All Lines",
            "Puts the time on every chat log line, not only chat. Off by default: it adds text to every line.",
            StampAll,
            OFF_ON,
            Now,
        ),
        row(
            "Clock",
            "Shows Earth time, the moon phase and the next Vana'diel day next to the game's clock.",
            EarthClock,
            OFF_ON,
            Now,
        ),
        row(
            "Text Auto-Advance",
            "Event messages that wait for a key move on by themselves after this many seconds. Off by default because it skips dialogue you may still be reading.",
            AutoAdvance,
            &[(0, "OFF"), (3, "3 s"), (5, "5 s"), (8, "8 s")],
            Now,
        ),
    ];
    match kind {
        Kind::Extras if modern => [MAPS, TIMERS_MODERN, EXTRA_BATTLE, EXTRA_MISC].get(tab).copied().unwrap_or(&[]),
        Kind::Extras => [MAPS, TIMERS, EXTRA_BATTLE, EXTRA_MISC].get(tab).copied().unwrap_or(&[]),
        Kind::Settings if in_game && tab == SOUND_TAB => &SOUND[..3],
        Kind::Settings if tab >= PACKS => &[],
        Kind::Settings if tab == CONTROLS => CONTROLS_ROWS,
        Kind::Settings => match tab {
            0 => DISPLAY,
            GRAPHICS => GRAPHICS_ROWS,
            2 => LIGHTING,
            POST_TAB => POST,
            4 if in_game => INTERFACE,
            4 => INTERFACE_LOBBY,
            SOUND_TAB => SOUND,
            6 => MISC,
            _ => &[],
        },
    }
}

const RESOLUTIONS: [[u32; 2]; 17] = [
    [640, 480],
    [800, 600],
    [1024, 768],
    [1280, 720],
    [1280, 800],
    [1280, 1024],
    [1366, 768],
    [1440, 900],
    [1600, 900],
    [1680, 1050],
    [1920, 1080],
    [1920, 1200],
    [2560, 1080],
    [2560, 1440],
    [2560, 1600],
    [3440, 1440],
    [3840, 2160],
];

/// The Config pages' size (conf2win and its siblings, 366 x 296 at (16, 48)).
const W: f32 = 366.0;
/// The Background row's warning while real shadows are on (ours).
const WARNING: &str = "Real shadows affected. Performance will be affected.";
const H: f32 = 296.0;
/// A row's pitch: its heading, then its buttons or slider under it.
const PITCH: f32 = 34.0;
/// Button columns as conf2win (two 88-px) and conf3win (three 64-px) place them; a slider at the buttons' left.
const WIDE: [f32; 2] = [78.0, 190.0];
const NARROW: [f32; 3] = [78.0, 174.0, 270.0];
const SLIDER_X: f32 = 88.0;
/// Where buttons widened for their captions start.
const WIDE_X: f32 = 32.0;
const SLIDER_W: f32 = 192.0;
/// miss00's page arrows (children 9 / 10: menu frames 25 / 26, 18 x 9) and its page name (FUN_1022de70, game font).
const ARROWS: [f32; 2] = [9.0, 343.0];
const ARROW_Y: f32 = 11.0;
const NAME_XY: (f32, f32) = (60.0, 10.0);

pub struct SettingsUi {
    #[cfg(feature = "forever")]
    pub preview_status: Option<String>,
    pub kind: Kind,
    pub s: Settings,
    /// Monitors as (name, "name  W x H") and GPU names, set by the app after opening.
    pub monitors: Vec<(String, String)>,
    pub gpus: Vec<String>,
    /// The window's pixels when opened (size lists).
    window: [u32; 2],
    /// Drawn where the in-game Config pages are (else centred, over the lobby).
    pub in_game: bool,
    /// The page (Settings).
    tab: usize,
    /// The cursor's row: 0 the page name (paged pages), then the rows from 1; the column within it.
    row: usize,
    col: usize,
    /// A slider being edited (Enter, then Left / Right, as the Config pages' sliders).
    edit: bool,
    pub sounds: Vec<Se>,
    /// Changed since the app last took it.
    pub dirty: bool,
    /// The pointer is over a control (the hit cursor).
    pub hover: bool,
    /// The Mods page's archives and moves, opened the first time the page shows.
    pub mods_ui: Option<super::modsui::Mods>,
    /// The folders in the mods folder that hold a Lua mod (the Mods page's; the rest are DAT packs).
    pub lua_names: std::collections::HashSet<String>,
    /// The modern DATs page's tab: 0 Installed, 1 Available.
    pub m_dat_tab: usize,
    /// The installed Lua mods' names by folder.
    pub lua_info: std::collections::HashMap<String, crate::modpack::LuaInfo>,
    /// The modern Mods page's cursor (stop, button) and a removal being asked (pack, Yes on the cursor).
    pub m_mods: (usize, usize),
    pub m_remove: Option<(usize, bool)>,
    /// Extras: the Lua mods' pages after our own, and their changes for the app to apply (mod, key, value).
    pub mod_pages: Vec<ModPage>,
    pub mod_changes: Vec<(String, String, serde_json::Value)>,
    /// Ours: the modern window's (modern/options.rs) state. The values when it opened or last Applied (Cancel goes back to
    /// them), where the keys are (the category list, the rows, the buttons) and which button, and that it is closing:
    /// the app drops it once what changed is stored.
    orig: Settings,
    pub zone: Zone,
    pub btn: usize,
    pub closing: bool,
    review: Option<DisplayReview>,
    /// Ours: hosted in the modern Config window's pages (modern/config.rs), where each change applies as it is made, in
    /// place of the Graphics & Interface window's drafts (Apply / Cancel / Keep).
    pub inline: bool,
    /// Mods' settings shown on our own Extras tabs: (mod, row).
    tab_rows: Vec<(String, ModRow)>,
}

struct DisplayReview {
    before: Settings,
    deadline: std::time::Instant,
    close: bool,
}

/// Where the modern window's keys act.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Zone {
    #[default]
    Cats,
    Rows,
    Buttons,
}

/// What a window key or button did to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Stay,
    /// OK: the values kept, the window closes.
    Ok,
    /// Cancel: the values put back, the window closes.
    Cancel,
}

/// The window's bottom buttons.
pub const WIN_BUTTONS: [&str; 4] = ["Defaults", "Apply", "OK", "Cancel"];

/// What a row's controls are.
enum Ctl {
    Buttons(usize),
    Slider,
}

impl SettingsUi {
    #[cfg(feature = "forever")]
    pub fn stage_tower_preview(&mut self, enabled: bool) {
        if !self.s.forever {
            return;
        }
        self.s.environment_textures = enabled;
        if enabled {
            self.s.graphics.enhanced = true;
        }
    }

    pub fn open(c: &Config, window: [u32; 2]) -> Self {
        Self::open_kind(c, window, Kind::Settings)
    }

    pub fn open_kind(c: &Config, window: [u32; 2], kind: Kind) -> Self {
        let mut s = Self {
            #[cfg(feature = "forever")]
            preview_status: None,
            kind,
            s: Settings::of(c),
            monitors: Vec::new(),
            gpus: Vec::new(),
            window,
            in_game: false,
            tab: 0,
            row: 0,
            col: 0,
            edit: false,
            sounds: Vec::new(),
            dirty: false,
            hover: false,
            mods_ui: None,
            m_mods: (0, 0),
            m_remove: None,
            mod_pages: Vec::new(),
            mod_changes: Vec::new(),
            orig: Settings::of(c),
            zone: Zone::Cats,
            btn: 2,
            closing: false,
            review: None,
            inline: false,
            tab_rows: Vec::new(),
            lua_names: crate::modpack::lua_folders(),
            m_dat_tab: 0,
            lua_info: crate::modpack::lua_info(),
        };
        s.col = s.marked_col();
        s
    }

    fn tabs(&self) -> Vec<String> {
        if self.kind == Kind::Extras {
            EXTRA_TABS.iter().map(|t| t.to_string()).chain(self.mod_pages.iter().map(|p| p.name.clone())).collect()
        } else {
            // Ours: the modern Graphics & Interface window has no DAT Packs / Mods categories: their buttons (a mod's
            // Settings, Download Standard Mods) open pages of the Config window (System > DATs / Mods), which has them.
            let n = if self.windowed() && self.s.modern_ui && self.in_game { PACKS } else { TABS.len() };
            TABS[..n].iter().map(|t| t.to_string()).collect()
        }
    }

    /// The Lua mod page on screen (Extras, past our own pages).
    fn mod_page(&self) -> Option<&ModPage> {
        if self.kind != Kind::Extras {
            return None;
        }
        self.mod_pages.get(self.tab.checked_sub(EXTRA_TABS.len())?)
    }

    fn rows(&self) -> Vec<Row> {
        if let Some(p) = self.mod_page() {
            return p
                .rows
                .iter()
                .enumerate()
                .map(|(i, r)| Row {
                    label: r.label.clone(),
                    help: r.help.clone(),
                    val: Val::Mod(i),
                    choices: r.choices.iter().enumerate().map(|(k, c)| (k as u32, c.0.clone())).collect(),
                    when: When::Now,
                })
                .collect();
        }
        // Only Forever offers the modern interface, even if an old regular-client config enabled it.
        let hidden = |v: &Val| {
            if !self.s.forever
                && matches!(
                    v,
                    Val::LampLights
                        | Val::Bloom
                        | Val::BloomIntensity
                        | Val::BloomThreshold
                        | Val::Tonemap
                        | Val::Exposure
                        | Val::Saturation
                        | Val::Ao
                        | Val::AoStrength
                        | Val::AoRadius
                        | Val::AoLight
                        | Val::Fog
                        | Val::FogDensity
                        | Val::FogHeight
                        | Val::FogRamp
                        | Val::FogLayers
                        | Val::WaterEnhanced
                        | Val::SkyClouds
                        | Val::LightGrade
                        | Val::LightGradeAmount
                        | Val::SunShafts
                        | Val::SunShaftsAmount
                        | Val::VolumetricFog
                        | Val::VolumetricFogAmount
                        | Val::DetailTextures
                        | Val::DetailStrength
                        | Val::TextureSharpening
                        | Val::CharacterLight
                )
            {
                return true;
            }
            #[cfg(feature = "forever")]
            if matches!(v, Val::MapView(_)) {
                return !self.s.forever;
            }
            if self.s.modern_ui && matches!(v, Val::MinimapDock | Val::MinimapSize) && self.kind == Kind::Extras {
                return true;
            }
            #[cfg(feature = "forever")]
            if matches!(
                v,
                Val::QuestTracker
                    | Val::QuestIndicators
                    | Val::AutoNavigation
                    | Val::TrainingHud
                    | Val::AutoRotationStart
                    | Val::JournalKey
                    | Val::AutoEquipUpgrades
                    | Val::ConColors
                    | Val::CompactHud
                    | Val::AutoCheckTarget
                    | Val::NavMesh
                    | Val::RecipeLogKey
            ) {
                return !self.s.forever || self.s.classic_only;
            }
            #[cfg(feature = "forever")]
            if matches!(
                v,
                Val::InvertHorizontalMouse
                    | Val::ForeverLook
                    | Val::Upscaler
                    | Val::UpscaleQuality
                    | Val::EnvironmentTextures
                    | Val::WoodsTextures
                    | Val::ActorShadows
                    | Val::ShadowMap
                    | Val::ShadowRange
                    | Val::SoftShadows
                    | Val::ActorShade
                    | Val::ShadowCascades
                    | Val::FarView
                    | Val::Ssao
                    | Val::Grading
            ) {
                return !self.s.forever;
            }
            ((self.s.classic_only || !self.s.forever)
                && matches!(
                    v,
                    Val::ModernUi
                        | Val::Experience
                        | Val::ModernScale
                        | Val::ModernOpacity
                        | Val::ResetPlaces
                        | Val::ModernMinimap
                        | Val::MinimapUnowned
                        | Val::JobPanel
                        | Val::ExpBar
                ))
                || (self.s.classic_only && matches!(v, Val::MinimapSize))
        };
        let mut out: Vec<Row> = rows(self.kind, self.tab, self.in_game, self.s.modern_ui)
            .iter()
            .filter(|r| !hidden(&r.val))
            .map(Row::from)
            .map(|r| self.offered(r))
            .collect();
        // Mods' settings on this Extras tab, at their places.
        if self.kind == Kind::Extras
            && let Some(tab) = EXTRA_TABS.get(self.tab)
        {
            let mut mine: Vec<(usize, Row)> = self
                .tab_rows
                .iter()
                .enumerate()
                .filter(|(_, (_, r))| r.tab == *tab)
                .map(|(i, (_, r))| {
                    (
                        (if self.s.modern_ui { r.place_modern.or(r.place) } else { r.place }).unwrap_or(usize::MAX),
                        self.tab_row(i),
                    )
                })
                .collect();
            mine.sort_by_key(|m| m.0);
            for (place, r) in mine {
                out.insert(place.min(out.len()), r);
            }
        }
        out
    }

    fn ctl(r: &Row) -> Ctl {
        match r.val {
            Val::Size(..)
            | Val::Gamma
            | Val::FrameLimit
            | Val::Monitor
            | Val::Gpu
            | Val::Music
            | Val::Effects
            | Val::MapOpacity
            | Val::ModernOpacity => Ctl::Slider,
            // The preset's four names are buttons (widened for their captions, as a long caption's are).
            Val::Preset | Val::Controls => Ctl::Buttons(r.choices.len()),
            _ if r.choices.len() <= 3 => Ctl::Buttons(r.choices.len()),
            _ => Ctl::Slider,
        }
    }

    /// The window's size times `f`, in pixels.
    fn scaled(&self, f: f32) -> [u32; 2] {
        [(self.window[0] as f32 * f).round() as u32, (self.window[1] as f32 * f).round() as u32]
    }

    fn sizes(&self, k: Sizes) -> Vec<[u32; 2]> {
        let h = self.window[1];
        let scaled = |f: f32| self.scaled(f);
        match k {
            Sizes::Window => RESOLUTIONS.to_vec(),
            // 0 = the window's; then scales of it and the tool's squares.
            Sizes::Background => {
                let mut v = vec![[0, 0], scaled(0.5), scaled(0.75), scaled(1.5), scaled(2.0)];
                v.extend([[512, 512], [1024, 1024], [2048, 2048], [4096, 4096]]);
                v
            }
            // The v5 tool: the window, then 95 %, 90 % ... while the height stays 512 or more.
            Sizes::Ui => {
                let mut v = vec![[0, 0]];
                let mut f = 0.95;
                while h as f32 * f >= 512.0 && f > 0.3 {
                    v.push(scaled(f));
                    f -= 0.05;
                }
                v
            }
        }
    }

    fn get(&self, v: Val) -> u32 {
        match v {
            Val::Tool(n) => self.tool_get(n),
            Val::Size(..) | Val::Gamma | Val::Monitor | Val::Gpu => 0,
            Val::Preset => self.preset(),
            #[cfg(feature = "forever")]
            Val::ForeverLook => {
                if self.s.graphics.enhanced {
                    1
                } else if self.s.graphics.active() {
                    2
                } else {
                    0
                }
            }
            #[cfg(feature = "forever")]
            Val::Upscaler => self.s.graphics.upscaler,
            #[cfg(feature = "forever")]
            Val::UpscaleQuality => self.s.graphics.upscale_quality,
            #[cfg(feature = "forever")]
            Val::EnvironmentTextures => self.s.environment_textures as u32,
            #[cfg(feature = "forever")]
            Val::WoodsTextures => self.s.woods_textures as u32,
            Val::FrameLimit => self.s.frame_limit,
            Val::ReduceMotion => self.s.reduce_motion as u32,
            Val::Aniso => self.s.anisotropy as u32,
            Val::Antialias => self.s.antialias,
            Val::Grading => self.s.grading,
            Val::DrawDistance => self.s.draw_distance,
            Val::SendRate => self.s.send_interval,
            Val::PartyTp => self.s.party_tp as u32,
            Val::ModernUi => self.s.modern_ui as u32,
            Val::UiScale => self.s.ui_scale,
            Val::Experience => self.experience(),
            Val::Fxaa => self.s.graphics.fxaa as u32,
            Val::Bloom => self.s.graphics.bloom as u32,
            Val::BloomIntensity => pct(self.s.graphics.bloom_intensity),
            Val::BloomThreshold => pct(self.s.graphics.bloom_threshold),
            Val::Tonemap => self.s.graphics.tonemap as u32,
            Val::Exposure => pct(self.s.graphics.exposure),
            Val::Sharpen => pct(self.s.graphics.sharpen),
            Val::Saturation => pct(self.s.graphics.saturation),
            Val::Ao => self.s.graphics.ao as u32,
            Val::AoStrength => pct(self.s.graphics.ao_strength),
            Val::AoRadius => pct(self.s.graphics.ao_radius),
            #[cfg(feature = "forever")]
            Val::AoLight => self.s.graphics.ao_light,
            #[cfg(not(feature = "forever"))]
            Val::AoLight => 0,
            Val::Fog => self.s.graphics.fog as u32,
            Val::FogDensity => pct(self.s.graphics.fog_density),
            Val::FogHeight => self.s.graphics.fog_height.round() as u32,
            #[cfg(feature = "forever")]
            Val::FogRamp => self.s.graphics.fog_ramp as u32,
            #[cfg(not(feature = "forever"))]
            Val::FogRamp => 0,
            #[cfg(feature = "forever")]
            Val::FogLayers => self.s.graphics.fog_layers as u32,
            #[cfg(not(feature = "forever"))]
            Val::FogLayers => 0,
            #[cfg(feature = "forever")]
            Val::WaterEnhanced => self.s.graphics.water_enhanced as u32,
            #[cfg(not(feature = "forever"))]
            Val::WaterEnhanced => 0,
            #[cfg(feature = "forever")]
            Val::SkyClouds => self.s.graphics.sky_clouds as u32,
            #[cfg(not(feature = "forever"))]
            Val::SkyClouds => 0,
            #[cfg(feature = "forever")]
            Val::LightGrade => self.s.graphics.light_grade as u32,
            #[cfg(not(feature = "forever"))]
            Val::LightGrade => 0,
            #[cfg(feature = "forever")]
            Val::LightGradeAmount => pct(self.s.graphics.light_grade_amount),
            #[cfg(not(feature = "forever"))]
            Val::LightGradeAmount => 0,
            #[cfg(feature = "forever")]
            Val::SunShafts => self.s.graphics.sun_shafts as u32,
            #[cfg(not(feature = "forever"))]
            Val::SunShafts => 0,
            #[cfg(feature = "forever")]
            Val::SunShaftsAmount => pct(self.s.graphics.sun_shafts_amount),
            #[cfg(not(feature = "forever"))]
            Val::SunShaftsAmount => 0,
            #[cfg(feature = "forever")]
            Val::VolumetricFog => self.s.graphics.volumetric_fog as u32,
            #[cfg(not(feature = "forever"))]
            Val::VolumetricFog => 0,
            #[cfg(feature = "forever")]
            Val::VolumetricFogAmount => pct(self.s.graphics.volumetric_fog_amount),
            #[cfg(not(feature = "forever"))]
            Val::VolumetricFogAmount => 0,
            #[cfg(feature = "forever")]
            Val::DetailTextures => self.s.graphics.detail_textures as u32,
            #[cfg(not(feature = "forever"))]
            Val::DetailTextures => 0,
            #[cfg(feature = "forever")]
            Val::DetailStrength => pct(self.s.graphics.detail_strength),
            #[cfg(not(feature = "forever"))]
            Val::DetailStrength => 0,
            #[cfg(feature = "forever")]
            Val::TextureSharpening => pct(self.s.graphics.texture_sharpening),
            #[cfg(not(feature = "forever"))]
            Val::TextureSharpening => 0,
            Val::CharacterLight => pct(self.s.graphics.character_light),
            Val::ModernScale => self.s.modern_scale,
            Val::ModernOpacity => self.s.modern_opacity,
            Val::MapOverlay => self.s.map_overlay as u32,
            Val::MapOpacity => self.s.map_opacity,
            Val::ResetPlaces => self.s.reset_places as u32,
            #[cfg(feature = "forever")]
            Val::MapView(n) => self.s.map_prefs.get(n),
            Val::MouseCamSpeed => self.s.mouse_camera_speed,
            Val::SynthAmount => self.s.synth_amount as u32,
            Val::StampAll => self.s.stamp_all_lines as u32,
            Val::TargetDistance => self.s.target_distance as u32,
            Val::TargetHpp => self.s.target_hpp as u32,
            Val::EarthClock => self.s.earth_clock as u32,
            Val::LampLights => self.s.lamp_lights as u32,
            Val::ActorShadows => self.s.actor_shadows,
            Val::ShadowMap => self.s.shadow_map,
            #[cfg(feature = "forever")]
            Val::ShadowRange => self.s.graphics.shadow_range,
            #[cfg(not(feature = "forever"))]
            Val::ShadowRange => 0,
            #[cfg(feature = "forever")]
            Val::FarView => self.s.graphics.far_view,
            #[cfg(not(feature = "forever"))]
            Val::FarView => 0,
            #[cfg(feature = "forever")]
            Val::SoftShadows => self.s.graphics.soft_shadows as u32,
            #[cfg(not(feature = "forever"))]
            Val::SoftShadows => 0,
            #[cfg(feature = "forever")]
            Val::ShadowCascades => self.s.graphics.shadow_cascades as u32,
            #[cfg(not(feature = "forever"))]
            Val::ShadowCascades => 0,
            #[cfg(feature = "forever")]
            Val::ActorShade => self.s.graphics.actor_sun_shadows as u32,
            #[cfg(not(feature = "forever"))]
            Val::ActorShade => 0,
            Val::Ssao => self.s.ssao as u32,
            Val::TrustSpeech => self.s.hide_trust_speech as u32,
            Val::ReadyLines => self.s.hide_ready_lines as u32,
            Val::ShortBattle => self.s.short_battle as u32,
            Val::NameColours => self.s.battle_name_colours as u32,
            Val::AutoAdvance => self.s.auto_advance,
            Val::Timers => self.s.timers as u32,
            Val::Skillchains => self.s.skillchain_helper as u32,
            Val::TimersDock => self.s.timers_dock,
            Val::Regime => self.s.regime as u32,
            Val::Minimap => self.s.minimap as u32,
            Val::ModernMinimap => self.s.modern_minimap as u32,
            Val::SoloBadge => self.s.solo_badge as u32,
            Val::JobPanel => self.s.job_panel as u32,
            Val::ExpBar => self.s.exp_bar as u32,
            #[cfg(feature = "forever")]
            Val::QuestTracker => self.s.quest_tracker as u32,
            #[cfg(feature = "forever")]
            Val::QuestIndicators => self.s.quest_giver_markers as u32,
            #[cfg(feature = "forever")]
            Val::AutoNavigation => self.s.auto_navigation as u32,
            #[cfg(feature = "forever")]
            Val::TrainingHud => self.s.training_hud as u32,
            #[cfg(feature = "forever")]
            Val::AutoRotationStart => self.s.rotation_autocast as u32,
            #[cfg(feature = "forever")]
            Val::JournalKey => self.s.journal_key as u32,
            #[cfg(feature = "forever")]
            Val::AutoEquipUpgrades => self.s.auto_equip_upgrades as u32,
            #[cfg(feature = "forever")]
            Val::ConColors => self.s.con_colors as u32,
            #[cfg(feature = "forever")]
            Val::CompactHud => self.s.compact_hud as u32,
            #[cfg(feature = "forever")]
            Val::AutoCheckTarget => self.s.auto_check_on_target as u32,
            #[cfg(feature = "forever")]
            Val::NavMesh => self.s.nav_mesh as u32,
            #[cfg(feature = "forever")]
            Val::RecipeLogKey => self.s.recipe_log_key as u32,
            Val::RotAdaptive => self.s.rotation_adaptive as u32,
            Val::KeepSorted => (self.s.auto_sort != 0) as u32,
            Val::Combine => self.s.auto_combine as u32,
            Val::RotCure => self.s.rotation_cure_pct,
            Val::RotConvert => self.s.rotation_convert_pct,
            Val::RotRenew => self.s.rotation_renew_secs,
            Val::MinimapUnowned => self.s.minimap_unowned as u32,
            Val::MinimapDock => self.s.minimap_dock,
            Val::MinimapSize => self.s.minimap_size,
            Val::Music => self.s.music,
            Val::Effects => self.s.effects,
            Val::RetailAspect => self.s.retail_aspect as u32,
            Val::CursorSize => self.s.cursor_size,
            Val::Gamepad => self.s.gamepad as u32,
            Val::Controls => self.s.controls,
            Val::ControlsAd => self.s.controls_ad_turn as u32,
            #[cfg(feature = "forever")]
            Val::InvertHorizontalMouse => self.s.invert_horizontal_mouse as u32,
            Val::GamepadDeadzone => self.s.gamepad_deadzone,
            Val::GamepadCamera => self.s.gamepad_camera,
            Val::KeysHideCursor => self.s.keys_hide_cursor as u32,
            Val::Tabbed(i) => self.tab_rows.get(i).map_or(0, |r| r.1.at as u32),
            Val::Mod(i) => self.mod_page().and_then(|p| p.rows.get(i)).map_or(0, |r| r.at as u32),
        }
    }

    fn set(&mut self, v: Val, x: u32) {
        match v {
            Val::Tool(n) => self.tool_set(n, x),
            Val::Preset => self.apply_preset(x),
            #[cfg(feature = "forever")]
            Val::ForeverLook if self.s.forever && x < 2 => {
                // The look presets leave the light level and the upscaler (a performance choice) as they are.
                let (character_light, upscaler, upscale_quality) =
                    (self.s.graphics.character_light, self.s.graphics.upscaler, self.s.graphics.upscale_quality);
                self.s.graphics = if x == 1 { crate::config::GraphicsCfg::enhanced() } else { Default::default() };
                self.s.graphics.character_light = character_light;
                (self.s.graphics.upscaler, self.s.graphics.upscale_quality) = (upscaler, upscale_quality);
                self.s.anisotropy = 16;
                self.s.lamp_lights = x == 1;
                self.tool_set("0000", 1);
                self.tool_set("0017", 1);
                self.tool_set("0003", 0);
                self.tool_set("0004", 0);
            }
            #[cfg(feature = "forever")]
            Val::ForeverLook if self.s.forever && x == 2 => self.s.graphics.enhanced = false,
            #[cfg(feature = "forever")]
            Val::ForeverLook => {}
            #[cfg(feature = "forever")]
            Val::Upscaler => {
                if self.s.forever {
                    self.s.graphics.upscaler = x.min(crate::config::UPSCALER_MAX);
                }
            }
            #[cfg(feature = "forever")]
            Val::UpscaleQuality => {
                if self.s.forever {
                    self.s.graphics.upscale_quality = x.min(crate::config::FSR_MODES.len() as u32 - 1);
                }
            }
            #[cfg(feature = "forever")]
            Val::EnvironmentTextures => {
                if self.s.forever {
                    self.s.environment_textures = x != 0;
                }
            }
            #[cfg(feature = "forever")]
            Val::WoodsTextures => {
                if self.s.forever {
                    self.s.woods_textures = x != 0;
                }
            }
            Val::ReduceMotion => self.s.reduce_motion = x != 0,
            Val::Aniso => self.s.anisotropy = x as u16,
            Val::Antialias => self.s.antialias = x,
            Val::Grading => self.s.grading = x,
            Val::DrawDistance => self.s.draw_distance = x,
            Val::SendRate => self.s.send_interval = x,
            Val::PartyTp => self.s.party_tp = x != 0,
            Val::ModernUi => self.s.modern_ui = x != 0,
            Val::UiScale => self.s.ui_scale = x,
            Val::Experience => self.apply_experience(x),
            Val::Fxaa => self.s.graphics.fxaa = x != 0,
            Val::Bloom => self.s.graphics.bloom = x != 0,
            Val::BloomIntensity => self.s.graphics.bloom_intensity = x as f32 / 100.0,
            Val::BloomThreshold => self.s.graphics.bloom_threshold = x as f32 / 100.0,
            Val::Tonemap => self.s.graphics.tonemap = x != 0,
            Val::Exposure => self.s.graphics.exposure = x as f32 / 100.0,
            Val::Sharpen => self.s.graphics.sharpen = x as f32 / 100.0,
            Val::Saturation => self.s.graphics.saturation = x as f32 / 100.0,
            Val::Ao => self.s.graphics.ao = x != 0,
            Val::AoStrength => self.s.graphics.ao_strength = x as f32 / 100.0,
            Val::AoRadius => self.s.graphics.ao_radius = x as f32 / 100.0,
            #[cfg(feature = "forever")]
            Val::AoLight => self.s.graphics.ao_light = x.min(1),
            #[cfg(not(feature = "forever"))]
            Val::AoLight => {}
            Val::Fog => self.s.graphics.fog = x != 0,
            Val::FogDensity => self.s.graphics.fog_density = x as f32 / 100.0,
            Val::FogHeight => self.s.graphics.fog_height = x as f32,
            #[cfg(feature = "forever")]
            Val::FogRamp => self.s.graphics.fog_ramp = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::FogRamp => {}
            #[cfg(feature = "forever")]
            Val::FogLayers => self.s.graphics.fog_layers = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::FogLayers => {}
            #[cfg(feature = "forever")]
            Val::WaterEnhanced => self.s.graphics.water_enhanced = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::WaterEnhanced => {}
            #[cfg(feature = "forever")]
            Val::SkyClouds => self.s.graphics.sky_clouds = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::SkyClouds => {}
            #[cfg(feature = "forever")]
            Val::LightGrade => self.s.graphics.light_grade = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::LightGrade => {}
            #[cfg(feature = "forever")]
            Val::LightGradeAmount => self.s.graphics.light_grade_amount = (x as f32 / 100.0).clamp(0.0, 1.5),
            #[cfg(not(feature = "forever"))]
            Val::LightGradeAmount => {}
            #[cfg(feature = "forever")]
            Val::SunShafts => self.s.graphics.sun_shafts = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::SunShafts => {}
            #[cfg(feature = "forever")]
            Val::SunShaftsAmount => self.s.graphics.sun_shafts_amount = (x as f32 / 100.0).clamp(0.0, 1.5),
            #[cfg(not(feature = "forever"))]
            Val::SunShaftsAmount => {}
            #[cfg(feature = "forever")]
            Val::VolumetricFog => self.s.graphics.volumetric_fog = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::VolumetricFog => {}
            #[cfg(feature = "forever")]
            Val::VolumetricFogAmount => self.s.graphics.volumetric_fog_amount = (x as f32 / 100.0).clamp(0.0, 1.5),
            #[cfg(not(feature = "forever"))]
            Val::VolumetricFogAmount => {}
            #[cfg(feature = "forever")]
            Val::DetailTextures => self.s.graphics.detail_textures = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::DetailTextures => {}
            #[cfg(feature = "forever")]
            Val::DetailStrength => self.s.graphics.detail_strength = (x as f32 / 100.0).clamp(0.0, 2.0),
            #[cfg(not(feature = "forever"))]
            Val::DetailStrength => {}
            #[cfg(feature = "forever")]
            Val::TextureSharpening => self.s.graphics.texture_sharpening = (x as f32 / 100.0).clamp(0.0, 1.5),
            #[cfg(not(feature = "forever"))]
            Val::TextureSharpening => {}
            Val::CharacterLight => self.s.graphics.character_light = x as f32 / 100.0,
            Val::ModernScale => self.s.modern_scale = x,
            Val::ModernOpacity => self.s.modern_opacity = x,
            Val::ResetPlaces => self.s.reset_places = x != 0,
            #[cfg(feature = "forever")]
            Val::MapView(n) => self.s.map_prefs.set(n, x),
            Val::MouseCamSpeed => self.s.mouse_camera_speed = x,
            Val::SynthAmount => self.s.synth_amount = x != 0,
            Val::StampAll => self.s.stamp_all_lines = x != 0,
            Val::TargetDistance => self.s.target_distance = x != 0,
            Val::TargetHpp => self.s.target_hpp = x != 0,
            Val::EarthClock => self.s.earth_clock = x != 0,
            Val::LampLights => self.s.lamp_lights = x != 0,
            Val::ActorShadows => self.s.actor_shadows = x,
            Val::ShadowMap => self.s.shadow_map = x,
            #[cfg(feature = "forever")]
            Val::ShadowRange => self.s.graphics.shadow_range = x.min(2),
            #[cfg(not(feature = "forever"))]
            Val::ShadowRange => {}
            #[cfg(feature = "forever")]
            Val::FarView => self.s.graphics.far_view = x.min(2),
            #[cfg(not(feature = "forever"))]
            Val::FarView => {}
            #[cfg(feature = "forever")]
            Val::SoftShadows => self.s.graphics.soft_shadows = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::SoftShadows => {}
            #[cfg(feature = "forever")]
            Val::ShadowCascades => self.s.graphics.shadow_cascades = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::ShadowCascades => {}
            #[cfg(feature = "forever")]
            Val::ActorShade => self.s.graphics.actor_sun_shadows = x != 0,
            #[cfg(not(feature = "forever"))]
            Val::ActorShade => {}
            Val::Ssao => self.s.ssao = x != 0,
            Val::TrustSpeech => self.s.hide_trust_speech = x != 0,
            Val::ReadyLines => self.s.hide_ready_lines = x != 0,
            Val::ShortBattle => self.s.short_battle = x != 0,
            Val::NameColours => self.s.battle_name_colours = x != 0,
            Val::AutoAdvance => self.s.auto_advance = x,
            Val::Timers => self.s.timers = x != 0,
            Val::Skillchains => self.s.skillchain_helper = x != 0,
            Val::TimersDock => self.s.timers_dock = x,
            Val::Regime => self.s.regime = x != 0,
            Val::Minimap => self.s.minimap = x != 0,
            Val::ModernMinimap => self.s.modern_minimap = x != 0,
            Val::MapOverlay => self.s.map_overlay = x != 0,
            Val::SoloBadge => self.s.solo_badge = x != 0,
            Val::JobPanel => self.s.job_panel = x != 0,
            Val::ExpBar => self.s.exp_bar = x != 0,
            #[cfg(feature = "forever")]
            Val::QuestTracker => self.s.quest_tracker = x != 0,
            #[cfg(feature = "forever")]
            Val::QuestIndicators => self.s.quest_giver_markers = x != 0,
            #[cfg(feature = "forever")]
            Val::AutoNavigation => self.s.auto_navigation = x != 0,
            #[cfg(feature = "forever")]
            Val::TrainingHud => self.s.training_hud = x != 0,
            #[cfg(feature = "forever")]
            Val::AutoRotationStart => self.s.rotation_autocast = x != 0,
            #[cfg(feature = "forever")]
            Val::JournalKey => self.s.journal_key = x != 0,
            #[cfg(feature = "forever")]
            Val::AutoEquipUpgrades => self.s.auto_equip_upgrades = x != 0,
            #[cfg(feature = "forever")]
            Val::ConColors => self.s.con_colors = x != 0,
            #[cfg(feature = "forever")]
            Val::CompactHud => self.s.compact_hud = x != 0,
            #[cfg(feature = "forever")]
            Val::AutoCheckTarget => self.s.auto_check_on_target = x != 0,
            #[cfg(feature = "forever")]
            Val::NavMesh => self.s.nav_mesh = x != 0,
            #[cfg(feature = "forever")]
            Val::RecipeLogKey => self.s.recipe_log_key = x as u8,
            Val::RotAdaptive => self.s.rotation_adaptive = x != 0,
            Val::KeepSorted if (x != 0) != (self.s.auto_sort != 0) => {
                self.s.auto_sort = if x != 0 { crate::config::AUTO_SORT_BAGS } else { 0 }
            }
            Val::KeepSorted => {}
            Val::Combine => self.s.auto_combine = x != 0,
            Val::RotCure => self.s.rotation_cure_pct = x,
            Val::RotConvert => self.s.rotation_convert_pct = x,
            Val::RotRenew => self.s.rotation_renew_secs = x,
            Val::MinimapUnowned => self.s.minimap_unowned = x != 0,
            Val::MinimapDock => self.s.minimap_dock = x,
            Val::MinimapSize => self.s.minimap_size = x,
            Val::RetailAspect => self.s.retail_aspect = x != 0,
            Val::CursorSize => self.s.cursor_size = x,
            Val::Gamepad => self.s.gamepad = x != 0,
            Val::Controls => self.s.controls = x.min(3),
            Val::ControlsAd => self.s.controls_ad_turn = x != 0,
            #[cfg(feature = "forever")]
            Val::InvertHorizontalMouse => self.s.invert_horizontal_mouse = x != 0,
            Val::GamepadDeadzone => self.s.gamepad_deadzone = x,
            Val::GamepadCamera => self.s.gamepad_camera = x,
            Val::KeysHideCursor => self.s.keys_hide_cursor = x != 0,
            Val::Tabbed(i) => {
                if let Some((id, r)) = self.tab_rows.get_mut(i)
                    && let Some(c) = r.choices.get(x as usize)
                    && r.at != x as usize
                {
                    r.at = x as usize;
                    self.mod_changes.push((id.clone(), r.key.clone(), c.1.clone()));
                }
            }
            Val::Mod(i) => {
                let Some(k) = self.tab.checked_sub(EXTRA_TABS.len()) else { return };
                if let Some(p) = self.mod_pages.get_mut(k)
                    && let Some(r) = p.rows.get_mut(i)
                    && let Some(c) = r.choices.get(x as usize)
                    && r.at != x as usize
                {
                    r.at = x as usize;
                    self.mod_changes.push((p.id.clone(), r.key.clone(), c.1.clone()));
                }
            }
            _ => {}
        }
    }

    fn tool_get(&self, n: &str) -> u32 {
        self.s
            .tool
            .get(n)
            .copied()
            .or_else(|| crate::config::TOOL_DEFAULTS.iter().find(|d| d.0 == n).map(|d| d.1))
            .unwrap_or(0)
    }

    fn tool_set(&mut self, n: &str, x: u32) {
        if crate::config::TOOL_DEFAULTS.iter().any(|d| d.0 == n && d.1 == x) {
            self.s.tool.remove(n);
        } else {
            self.s.tool.insert(n.to_string(), x);
        }
    }

    /// A slider row's stops and the one it is at.
    fn stops(&self, r: &Row) -> (usize, usize) {
        let pos = |list: &[u32], cur: u32| list.iter().position(|&x| x == cur).unwrap_or(0);
        match r.val {
            Val::Size(a, b, k) => {
                let list = self.sizes(k);
                let cur = [self.tool_get(a), self.tool_get(b)];
                (list.len(), list.iter().position(|&x| x == cur).unwrap_or(0))
            }
            Val::Gamma => {
                (21, ((f32::from_bits(self.tool_get("0028")) + 1.0) * 10.0).round().clamp(0.0, 20.0) as usize)
            }
            Val::FrameLimit => (FRAME_LIMITS.len(), pos(&FRAME_LIMITS, self.s.frame_limit)),
            Val::Monitor => {
                (self.monitors.len().max(1), self.monitors.iter().position(|m| m.0 == self.s.monitor).unwrap_or(0))
            }
            Val::Gpu => (self.gpus.len() + 1, self.gpus.iter().position(|g| *g == self.s.gpu).map_or(0, |i| i + 1)),
            Val::Music => (21, (self.s.music / 5) as usize),
            Val::Effects => (21, (self.s.effects / 5) as usize),
            Val::ModernOpacity => (19, (self.s.modern_opacity.clamp(10, 100) / 5 - 2) as usize),
            Val::MapOpacity => (19, (self.s.map_opacity.clamp(10, 100) / 5 - 2) as usize),
            v => {
                let list: Vec<u32> = r.choices.iter().map(|c| c.0).collect();
                (list.len(), pos(&list, self.get(v)))
            }
        }
    }

    /// Moves a slider row to stop `i`.
    fn set_stop(&mut self, r: &Row, i: usize) {
        let before = self.s.clone();
        match r.val {
            Val::Size(a, b, k) => {
                let list = self.sizes(k);
                let [x, y] = list[i.min(list.len() - 1)];
                self.tool_set(a, x);
                self.tool_set(b, y);
            }
            Val::Gamma => {
                let g = ((i.min(20) as f32 - 10.0) / 10.0 * 10.0).round() / 10.0;
                self.tool_set("0028", if g == 0.0 { 0 } else { g.to_bits() });
            }
            Val::FrameLimit => self.s.frame_limit = FRAME_LIMITS[i.min(FRAME_LIMITS.len() - 1)],
            Val::Monitor => {
                if let Some(m) = self.monitors.get(i) {
                    self.s.monitor = m.0.clone();
                }
            }
            Val::Gpu => {
                self.s.gpu = if i == 0 { String::new() } else { self.gpus.get(i - 1).cloned().unwrap_or_default() }
            }
            Val::Music => self.s.music = (i as u32 * 5).min(100),
            Val::Effects => self.s.effects = (i as u32 * 5).min(100),
            Val::ModernOpacity => self.s.modern_opacity = ((i as u32 + 2) * 5).clamp(10, 100),
            Val::MapOpacity => self.s.map_opacity = ((i as u32 + 2) * 5).clamp(10, 100),
            v => {
                if let Some(c) = r.choices.get(i) {
                    self.set(v, c.0);
                }
            }
        }
        if self.s != before {
            self.dirty = true;
        }
    }

    /// The row's value as a slider shows it.
    fn shown(&self, r: &Row) -> String {
        match r.val {
            Val::Size(a, b, _) => match [self.tool_get(a), self.tool_get(b)] {
                [0, _] | [_, 0] => "Window".into(),
                [x, y] => format!("{x}x{y}"),
            },
            Val::Gamma => format!("{:+.1}", f32::from_bits(self.tool_get("0028"))),
            Val::FrameLimit if self.s.reduce_motion && (self.s.frame_limit == 0 || self.s.frame_limit > 60) => {
                "60 (Reduce Motion)".into()
            }
            Val::FrameLimit if self.s.frame_limit == 0 => "Unlimited".into(),
            Val::FrameLimit => format!("{}", self.s.frame_limit),
            Val::Music => format!("{}%", self.s.music),
            Val::Effects => format!("{}%", self.s.effects),
            Val::ModernOpacity => format!("{}%", self.s.modern_opacity),
            Val::MapOpacity => format!("{}%", self.s.map_opacity),
            Val::Monitor => self
                .monitors
                .iter()
                .find(|m| m.0 == self.s.monitor)
                .or(self.monitors.first())
                .map_or_else(|| "Default".into(), |m| m.0.clone()),
            Val::Gpu if self.s.gpu.is_empty() => "Default".into(),
            // The name without the driver's part ("AMD Radeon RX 6800 XT (RADV NAVI21)").
            Val::Gpu => self.s.gpu.split(" (").next().unwrap_or_default().to_string(),
            v => {
                let x = self.get(v);
                r.choices.iter().find(|c| c.0 == x).map_or(x.to_string(), |c| c.1.to_string())
            }
        }
    }

    /// The column of a buttons row whose choice holds (the cursor's start on it).
    fn marked_col(&self) -> usize {
        if self.on_mods() {
            return self.mods_marked();
        }
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i).cloned()) else { return 0 };
        match Self::ctl(&r) {
            Ctl::Buttons(_) => r.choices.iter().position(|c| c.0 == self.get(r.val)).unwrap_or(0),
            Ctl::Slider => 0,
        }
    }

    fn cols(&self, row: usize) -> usize {
        if self.on_mods() {
            return self.mods_cols(row);
        }
        match row.checked_sub(1).and_then(|i| self.rows().get(i).cloned()) {
            None => 1,
            Some(r) => match Self::ctl(&r) {
                Ctl::Buttons(n) => n,
                Ctl::Slider => 1,
            },
        }
    }

    /// Turns the page by `d`, the cursor kept on its row where the page has it.
    fn turn(&mut self, d: i32) {
        self.tab = (self.tab as i32 + d).rem_euclid(self.tabs().len() as i32) as usize;
        if self.on_mods() {
            // An install's failure belongs to the page it was asked on.
            self.mods_ui.get_or_insert_with(super::modsui::Mods::open).error = None;
        }
        self.row = self.row.min(self.row_count());
        self.col = self.marked_col();
        self.sounds.push(Se::Cursor);
    }

    /// Enter on the cursor's control.
    fn enter(&mut self) {
        if self.on_mods() {
            return self.mods_enter();
        }
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i).cloned()) else { return };
        match Self::ctl(&r) {
            Ctl::Buttons(_) => {
                let before = self.s.clone();
                if let Some(c) = r.choices.get(self.col) {
                    self.set(r.val, c.0);
                }
                self.dirty |= self.s != before;
                self.sounds.push(Se::Select);
            }
            Ctl::Slider => {
                self.edit = true;
                self.sounds.push(Se::Select);
            }
        }
    }

    /// A key; returns true when the page closes (Escape). Left / Right move between a row's buttons; on the page name
    /// (a stop of ours above the rows) or a slider row, or with Ctrl / Alt (the lists' page modifier), they turn the
    /// page.
    pub fn key(&mut self, key: egui::Key) -> bool {
        self.key_mod(key, false)
    }

    /// Shift+Tab (the pad's LB): the previous tab.
    pub fn tab_back(&mut self) {
        self.sounds.clear();
        self.turn(-1);
    }

    pub fn key_mod(&mut self, key: egui::Key, page_mod: bool) -> bool {
        self.sounds.clear();
        if self.review_key(key) {
            return false;
        }
        if self.deferred() {
            if key == egui::Key::Escape && !self.edit && !self.on_mods() {
                self.cancel();
                return false; // The app applies the saved baseline before closing.
            }
            if key == egui::Key::Enter && page_mod {
                self.commit();
                return false;
            }
        }
        if self.on_mods() {
            let closed = self.mods_key(key, page_mod);
            if closed && self.deferred() {
                self.cancel();
                return false;
            }
            return closed;
        }
        if self.edit {
            let r = self.rows()[self.row - 1].clone();
            let (n, i) = self.stops(&r);
            match key {
                egui::Key::ArrowRight if i + 1 < n => self.set_stop(&r, i + 1),
                egui::Key::ArrowLeft if i > 0 => self.set_stop(&r, i - 1),
                egui::Key::Enter | egui::Key::Escape => {
                    self.edit = false;
                    self.sounds.push(if key == egui::Key::Enter { Se::Select } else { Se::Cancel });
                }
                _ => {}
            }
            return false;
        }
        let span = self.rows().len() + 1;
        match key {
            egui::Key::ArrowDown | egui::Key::ArrowUp => {
                let d = if key == egui::Key::ArrowDown { 1 } else { span - 1 };
                self.row = (self.row + d) % span;
                self.col = self.marked_col().min(self.cols(self.row) - 1);
                self.sounds.push(Se::Cursor);
            }
            egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                let c = self.cols(self.row);
                let d = if key == egui::Key::ArrowRight { 1 } else { -1 };
                if c > 1 && !page_mod {
                    self.col = (self.col as i32 + d).rem_euclid(c as i32) as usize;
                    self.sounds.push(Se::Cursor);
                } else {
                    self.turn(d);
                }
            }
            egui::Key::Enter => self.enter(),
            egui::Key::Tab | egui::Key::PageDown => self.turn(1),
            egui::Key::PageUp => self.turn(-1),
            egui::Key::Escape => return true,
            _ => {}
        }
        false
    }

    /// The page's top left.
    fn origin(&self, gu: &GameUi, sw: f32, sh: f32) -> (f32, f32) {
        let height = if self.deferred() { self.footer_y() + 28.0 } else { H };
        if self.in_game
            && let Some((x, y)) = gu.window_xy("menu    conf2win", sw, sh)
        {
            return (x, y.min((sh - height).max(0.0)));
        }
        (((sw - W) / 2.0).round(), ((sh - height) / 2.0).round().max(0.0))
    }

    /// The first heading, under the arrows (miss00 starts its rows at y 40).
    const TOP: f32 = 36.0;

    /// The page arrows' rects relative to the page (a little larger than the sprites, for the pointer).
    fn arrow_rects(&self) -> [[f32; 4]; 2] {
        ARROWS.map(|x| [x - 4.0, ARROW_Y - 4.0, 26.0, 17.0])
    }

    /// Every control's rect relative to the page: (row, col, rect).
    fn controls(&self) -> Vec<(usize, usize, [f32; 4])> {
        if self.on_mods() {
            return self.mods_controls();
        }
        let mut out = Vec::new();
        for (i, r) in self.rows().iter().enumerate() {
            let y = Self::TOP + PITCH * i as f32 + 14.0;
            // conf2win's two 88-px and conf3win's three 64-px columns; a caption that does not fit (12 px of the
            // button are its ends) widens every button of the row, from the bullets' column.
            let need =
                r.choices.iter().map(|c| super::super::render::screens::label_width(&c.1) + 12.0).fold(0.0, f32::max);
            match Self::ctl(r) {
                Ctl::Buttons(n) if n <= 2 && need <= 88.0 => {
                    for (c, x) in WIDE.iter().enumerate().take(n) {
                        out.push((i + 1, c, [*x, y, 88.0, 16.0]));
                    }
                }
                Ctl::Buttons(n) if n == 3 && need <= 64.0 => {
                    for (c, x) in NARROW.iter().enumerate().take(n) {
                        out.push((i + 1, c, [*x, y, 64.0, 16.0]));
                    }
                }
                Ctl::Buttons(n) => {
                    let mut widths = vec![need.ceil(); n];
                    let gap = 8.0 * n.saturating_sub(1) as f32;
                    let room = W - WIDE_X - 32.0;
                    if widths.iter().sum::<f32>() + gap > room {
                        // One longer caption need not widen its neighbours past the frame.
                        // Keep every caption and its padding; use individual widths only
                        // when that fits, leaving already-fitting retail rows unchanged.
                        let fitted: Vec<f32> = r
                            .choices
                            .iter()
                            .map(|c| (super::super::render::screens::label_width(&c.1) + 12.0).ceil())
                            .collect();
                        if fitted.iter().sum::<f32>() + gap <= room {
                            widths = fitted;
                        }
                    }
                    let mut x = WIDE_X;
                    for (c, w) in widths.into_iter().enumerate() {
                        out.push((i + 1, c, [x, y, w, 16.0]));
                        x += w + 8.0;
                    }
                }
                Ctl::Slider => out.push((i + 1, 0, [SLIDER_X, y, SLIDER_W, 16.0])),
            }
        }
        out
    }

    /// The pointer: controls follow it; a click is Enter, on a slider it sets the stop under the pointer.
    pub fn pointer(&mut self, gu: &GameUi, p: (f32, f32), moved: bool, clicked: bool, sw: f32, sh: f32) {
        self.sounds.clear();
        self.hover = false;
        let (x, y) = self.origin(gu, sw, sh);
        if self.review.is_some() {
            if let Some(k) =
                self.review_rects().iter().position(|r| super::inside(&[x + r[0], y + r[1], r[2], r[3]], p))
            {
                self.hover = true;
                if clicked {
                    self.finish_review(k == 0);
                }
            }
            return;
        }
        if self.deferred()
            && let Some(k) =
                self.footer_rects().iter().position(|r| super::inside(&[x + r[0], y + r[1], r[2], r[3]], p))
        {
            self.hover = true;
            if clicked {
                self.win_press(k + 1);
            }
            return;
        }
        if let Some(k) = self
            .arrow_rects()
            .iter()
            .position(|r| p.0 >= x + r[0] && p.1 >= y + r[1] && p.0 < x + r[0] + r[2] && p.1 < y + r[1] + r[3])
        {
            self.hover = true;
            if clicked {
                self.edit = false;
                self.turn(if k == 0 { -1 } else { 1 });
            }
            return;
        }
        if self.on_mods() && self.mods_ui.as_ref().is_some_and(|m| m.held.is_some()) {
            return;
        }
        let hit = self
            .controls()
            .into_iter()
            .find(|(_, _, r)| p.0 >= x + r[0] && p.1 >= y + r[1] && p.0 < x + r[0] + r[2] && p.1 < y + r[1] + r[3]);
        let Some((row, col, r)) = hit else { return };
        self.hover = true;
        if (moved || clicked) && (row, col) != (self.row, self.col) && !self.edit {
            self.row = row;
            self.col = col;
            self.sounds.push(Se::Cursor);
        }
        if !clicked {
            return;
        }
        if let Some(rw) = self.rows().get(row - 1).cloned()
            && matches!(Self::ctl(&rw), Ctl::Slider)
        {
            let (n, _) = self.stops(&rw);
            let f = ((p.0 - x - r[0]) / r[2]).clamp(0.0, 1.0);
            self.set_stop(&rw, (f * (n - 1) as f32).round() as usize);
            self.edit = false;
            self.sounds.push(Se::Select);
            return;
        }
        self.enter();
    }

    /// Draws the page; returns its rect.
    pub fn draw(&self, gu: &mut GameUi, sw: f32, sh: f32) -> [f32; 4] {
        const N: u32 = 0x8080_8080;
        const ON: u32 = 0x8080_5020;
        let (x, y) = self.origin(gu, sw, sh);
        let height = if self.deferred() { self.footer_y() + 28.0 } else { H };
        gu.frame(x, y, W, height);
        if let Some(seconds) = self.review_seconds() {
            gu.label(&format!("Keep these settings? ({seconds}s)"), x + 24.0, y + 28.0, N, false);
            gu.label("Reverts automatically if not kept.", x + 24.0, y + 52.0, N, false);
            for (label, r) in ["Keep", "Revert"].iter().zip(self.review_rects()) {
                button(gu, [x + r[0], y + r[1], r[2], r[3]], N);
                gu.label(label, x + r[0] + 8.0, y + r[1] + 2.0, N, false);
            }
            return [x, y, W, height];
        }
        if self.deferred() {
            for (i, r) in self.footer_rects().iter().enumerate() {
                button(gu, [x + r[0], y + r[1], r[2], r[3]], N);
                gu.label(WIN_BUTTONS[i + 1], x + r[0] + 8.0, y + r[1] + 2.0, N, false);
            }
        }
        // miss00: the arrows and the page's name.
        gu.sprite("menu    frames", 25, x + ARROWS[0], y + ARROW_Y, N);
        gu.sprite("menu    frames", 26, x + ARROWS[1], y + ARROW_Y, N);
        gu.text(&self.tabs()[self.tab], x + NAME_XY.0, y + NAME_XY.1, 0, [0x80; 4]);
        if self.row == 0 {
            gu.cursor(x + NAME_XY.0 - 4.0, y + NAME_XY.1);
        }
        if self.on_mods() {
            self.mods_draw(gu, x, y);
            return [x, y, W, height];
        }
        // A category with no rows yet says what is coming, in lines that fit the page.
        if let Some(note) = self.note() {
            let mut line = String::new();
            let mut ly = y + Self::TOP;
            for word in note.split(' ') {
                let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if !line.is_empty() && gu.label_width(&next) > W - 64.0 {
                    gu.label(&line, x + 32.0, ly, N, false);
                    ly += 18.0;
                    line = word.to_string();
                } else {
                    line = next;
                }
            }
            gu.label(&line, x + 32.0, ly, N, false);
            return [x, y, W, height];
        }
        let ctls = self.controls();
        let focus = |row: usize, col: usize| self.row == row && self.col == col;
        let mut hand = None;
        for (i, r) in self.rows().iter().enumerate() {
            let hy = y + Self::TOP + PITCH * i as f32;
            // The bullet (465 cell 2, drawn at its own place (20, 27) on the page) moved to the heading.
            if let Some(b) = gu.cell_bounds("menu    windowps", 465, 2) {
                gu.sprite_cells("menu    windowps", 465, 2..3, x + 20.0 - b[0], hy + 2.0 - b[1], N);
            }
            gu.label(&r.label, x + 32.0, hy, N, false);
            let icon =
                if self.warns(r) { gu.warning_icon(x + 32.0 + gu.label_width(&r.label) + 4.0, hy) + 4.0 } else { 0.0 };
            // A slider's value at the heading's end, right-aligned to the slider's (cut to the room left).
            if matches!(Self::ctl(r), Ctl::Slider) {
                let room = SLIDER_X + SLIDER_W - 32.0 - gu.label_width(&r.label) - icon - 16.0;
                let mut v = self.shown(r);
                while gu.label_width(&v) > room && v.pop().is_some() {}
                let vw = gu.label_width(&v);
                gu.label(&v, x + SLIDER_X + SLIDER_W - vw, hy, N, false);
            }
            for (_, c, b) in ctls.iter().filter(|c| c.0 == i + 1) {
                let on = focus(i + 1, *c) && !self.edit;
                match Self::ctl(r) {
                    Ctl::Buttons(_) => {
                        let d = if on { -1.0 } else { 0.0 };
                        button(gu, [x + b[0] + d, y + b[1] + d, b[2], b[3]], if on { ON } else { N });
                        gu.label(
                            &r.choices[*c].1,
                            x + b[0] + 5.0 + d,
                            y + b[1] + 1.0 + d,
                            if on { ON } else { N },
                            true,
                        );
                        if r.choices[*c].0 == self.get(r.val) {
                            marker(gu, x + b[0], y + b[1], b[2]);
                        }
                    }
                    Ctl::Slider => {
                        // The Config pages' slider: track windowps 383, fill 384 (brighter while edited).
                        let (n, k) = self.stops(r);
                        let f = if n > 1 { k as f32 / (n - 1) as f32 } else { 1.0 };
                        let editing = self.edit && focus(i + 1, 0);
                        // The track is baked into each page's backing (465 cells 8..10 under conf2win child 3 at (88, 80)).
                        gu.sprite_cells("menu    windowps", 465, 8..11, x + b[0] - 88.0, y + b[1] - 80.0, N);
                        if f > 0.0 {
                            gu.sprite_scaled(
                                "menu    windowps",
                                384,
                                x + b[0],
                                y + b[1],
                                f,
                                1.0,
                                if editing { 0xa0a0_a0a0 } else { N },
                            );
                        }
                    }
                }
                if focus(i + 1, *c) && !self.edit {
                    hand = Some((x + b[0], y + b[1]));
                }
            }
        }
        if let Some((hx, hy)) = hand {
            gu.cursor(hx, hy);
        }
        [x, y, W, height]
    }

    /// Puts the cursor on a page and row (offscreen shots; 0 the page name).
    pub fn goto(&mut self, tab: usize, row: usize) {
        self.tab = tab.min(self.tabs().len() - 1);
        if self.on_mods() {
            let m = self.mods_ui.get_or_insert_with(super::modsui::Mods::open);
            m.wait_scan();
            let _ = m.tick(&mut self.s.mods);
        }
        self.row = row.min(self.row_count());
        self.col = self.marked_col();
        if let Some(m) = self.mods_ui.as_mut().filter(|_| self.kind == Kind::Settings && self.tab >= PACKS) {
            m.follow(self.row);
        }
    }

    /// Extras turned to a Lua mod's page (`/lua settings`).
    pub fn goto_mod(&mut self, id: &str) {
        if let Some(k) = self.mod_pages.iter().position(|p| p.id == id) {
            self.goto(EXTRA_TABS.len() + k, 1);
        }
    }

    /// The help bar's text for the cursor's row, with when a change shows.
    /// Ours, not retail: the Background row carries a warning while real character shadows are on (their full-screen
    /// receive pass runs at the Background size, so a larger one costs more).
    fn warns(&self, r: &Row) -> bool {
        matches!(r.val, Val::Size(_, _, Sizes::Background)) && self.s.actor_shadows != 0
    }

    pub fn help(&self) -> String {
        if self.on_mods() {
            return if self.s.modern_ui { self.modern_mods_help() } else { self.mods_help() };
        }
        let Some(r) = self.row.checked_sub(1).and_then(|i| self.rows().get(i).cloned()) else {
            return format!("Left / Right: another page of {}.", self.title().to_lowercase());
        };
        self.row_help(&r)
    }

    fn row_help(&self, r: &Row) -> String {
        let when = match r.when {
            When::Now => "",
            When::NextZone => " Takes effect at the next zone load.",
            When::Restart => " Takes effect on restart.",
        };
        let warn = if self.warns(r) { format!("{WARNING} ") } else { String::new() };
        format!("{warn}{}{when}", r.help)
    }

    pub fn title(&self) -> &'static str {
        if self.kind == Kind::Extras { "Extras" } else { "Settings" }
    }

    /// The mods' Extras pages: rows that name one of our tabs go there, the rest make the mod's own page.
    pub fn set_mod_pages(&mut self, pages: Vec<ModPage>) {
        self.tab_rows.clear();
        self.mod_pages.clear();
        for mut p in pages {
            let (tabbed, own): (Vec<ModRow>, Vec<ModRow>) =
                p.rows.into_iter().partition(|r| EXTRA_TABS.contains(&r.tab.as_str()));
            self.tab_rows.extend(tabbed.into_iter().map(|r| (p.id.clone(), r)));
            if !own.is_empty() {
                p.rows = own;
                self.mod_pages.push(p);
            }
        }
    }

    /// A mod's setting on our Extras tabs as a row.
    fn tab_row(&self, i: usize) -> Row {
        let r = &self.tab_rows[i].1;
        Row {
            label: r.label.clone(),
            help: r.help.clone(),
            val: Val::Tabbed(i),
            choices: r.choices.iter().enumerate().map(|(k, c)| (k as u32, c.0.clone())).collect(),
            when: When::Now,
        }
    }
}

/// What Draw Distance and Far View say about each other, appended to their help (Forever): the two multiply (Far View moves the
/// zone's draw distance and the fog end, Draw Distance only scales the draw distance), and with Draw Distance on Unlimited, the
/// default, the draw-distance limit is already lifted, so Far View then moves the fog and the placements' own distances only.
/// `draw` is the Draw Distance value (percent of retail's, 0 = Unlimited); `far_view` Far View's level.
#[cfg(feature = "forever")]
fn distance_note(for_far_view: bool, draw: u32, enhanced: bool, far_view: u32) -> String {
    let f = crate::render::farview::factor(far_view);
    let level = ["Retail", "Far", "Very Far"][(far_view as usize).min(2)];
    let times = |x: f32| if x.fract() == 0.0 { format!("{x:.0}x") } else { format!("{x:.1}x") };
    let on = enhanced && f > 1.0;
    if for_far_view {
        if !enhanced {
            return " Needs Forever Graphics: Enhanced. Draw Distance (above) scales the same distance and compounds with it.".into();
        }
        return match draw {
            0 => " Draw Distance is Unlimited (the default), so the scenery distance limit is already lifted: Far View then moves the fog end and the placements' own distances only.".into(),
            100 => " Draw Distance is Retail, so Far View alone sets how far scenery draws.".into(),
            d => format!(
                " Draw Distance ({}) multiplies the distance Far View sets: together {} retail's in open-air zones; the fog ends at Far View's distance only.",
                times(d as f32 / 100.0),
                times(f * d as f32 / 100.0)
            ),
        };
    }
    if !on {
        return " Forever Graphics: Enhanced's Far View also moves this distance (with the fog) out; it compounds with this setting.".into();
    }
    match draw {
        0 => format!(" Unlimited lifts the zone's distance limit altogether, so Far View ({level}) then moves only the fog end and the placements' own distances."),
        100 => format!(" Far View ({level}) is on: \"Retail\" here is {} retail's distance in open-air zones.", times(f)),
        d => format!(
            " Far View ({level}) is on and compounds with this: {} here is {} retail's distance in open-air zones; the fog ends at Far View's distance only.",
            times(d as f32 / 100.0),
            times(f * d as f32 / 100.0)
        ),
    }
}

/// The Settings window's pages (client/modern/config.rs): any row of ours, Settings' or Extras', by its label, whatever
/// page this state is on. A row left out here (in game, or with the modern interface) is None.
impl SettingsUi {
    /// A row with only the choices this server offers: Forever's control scheme (3) on Forever only, never under
    /// classic_only. Both the page list and the modern layer's lookup (`find` / `item`) go through it.
    fn offered(&self, mut r: Row) -> Row {
        if matches!(r.val, Val::Controls) && (!self.s.forever || self.s.classic_only) {
            r.choices.retain(|c| c.0 != 3);
        }
        #[cfg(feature = "forever")]
        if self.s.forever && matches!(r.val, Val::ShadowRange) && self.s.graphics.cascades_on() {
            // Shadow Cascades size their own reach; this row sizes only the single map.
            r.help = format!("Shadow Cascades are on: they set the shadow reach; this applies when they're off. {}", r.help);
        }
        #[cfg(feature = "forever")]
        if self.s.forever && matches!(r.val, Val::DrawDistance | Val::FarView) {
            let enhanced = self.s.graphics.enhanced;
            r.help.push_str(&distance_note(matches!(r.val, Val::FarView), self.s.draw_distance, enhanced, self.s.graphics.far_view));
        }
        r
    }

    fn find(&self, label: &str) -> Option<Row> {
        let pages = (0..PACKS).map(|t| (Kind::Settings, t)).chain((0..EXTRA_TABS.len()).map(|t| (Kind::Extras, t)));
        let ours = pages
            .flat_map(|(k, t)| rows(k, t, self.in_game, self.s.modern_ui).iter())
            .find(|r| r.label == label)
            .map(Row::from)
            .map(|r| self.offered(r));
        // Else a mod's row on one of our tabs.
        ours.or_else(|| self.tab_rows.iter().position(|(_, r)| r.label == label).map(|i| self.tab_row(i)))
    }

    fn vrow(&self, r: &Row) -> VRow {
        let ctl = match Self::ctl(r) {
            Ctl::Buttons(_) => {
                VCtl::Buttons(r.choices.iter().map(|c| (c.1.to_string(), c.0 == self.get(r.val))).collect())
            }
            Ctl::Slider => {
                let (n, at) = self.stops(r);
                VCtl::Slider { n, at, shown: self.shown(r) }
            }
        };
        let label = if self.warns(r) { format!("{} \u{26a0}", r.label) } else { r.label.to_string() };
        #[cfg(feature = "forever")]
        let dim = matches!(r.val, Val::ShadowRange) && self.s.graphics.cascades_on();
        #[cfg(not(feature = "forever"))]
        let dim = false;
        VRow { label, ctl, dim }
    }

    /// Whether the row named `label` holds a retail setting (a config-tool value, a size, Gamma Base, Party TP).
    pub fn retail(&self, label: &str) -> bool {
        self.find(label).is_some_and(|r| matches!(r.val, Val::Tool(_) | Val::Size(..) | Val::Gamma | Val::PartyTp))
    }

    /// The row named `label` as the modern layer draws it, and its help line.
    pub fn item(&self, label: &str) -> Option<(VRow, String)> {
        let r = self.find(label)?;
        Some((self.vrow(&r), plain(&self.row_help(&r))))
    }

    /// The row named `label` set: its button `x` taken, or its slider to stop `x`.
    pub fn item_set(&mut self, label: &str, x: usize, click: bool) {
        let Some(r) = self.find(label) else { return };
        self.sounds.clear();
        match Self::ctl(&r) {
            Ctl::Buttons(_) => {
                let before = self.s.clone();
                if let Some(c) = r.choices.get(x) {
                    self.set(r.val, c.0);
                }
                self.dirty |= self.s != before;
                self.sounds.push(Se::Select);
            }
            Ctl::Slider => {
                if self.stops(&r).1 != x {
                    self.set_stop(&r, x);
                    if !click {
                        self.sounds.push(Se::Cursor);
                    }
                }
            }
        }
    }

    /// The Mods page (Settings) or the Lua mods' pages (Extras, after our three) turned to.
    pub fn goto_mods(&mut self) {
        self.goto(self.mods_tab(), 0);
    }

    /// The first mods tab: Mods (Settings), or the first Lua mod's page (Extras after its own).
    pub fn mods_tab(&self) -> usize {
        if self.kind == Kind::Extras { EXTRA_TABS.len() } else { PACKS }
    }

    /// Every label of ours, for the Settings window's test that each row has a place.
    #[cfg(test)]
    pub fn all_labels(in_game: bool, modern: bool) -> Vec<&'static str> {
        let pages = (0..PACKS).map(|t| (Kind::Settings, t)).chain((0..EXTRA_TABS.len()).map(|t| (Kind::Extras, t)));
        pages.flat_map(|(k, t)| rows(k, t, in_game, modern).iter()).map(|r| r.label).collect()
    }
}

/// A row as the modern layer draws it: its heading and controls (its help is the page's line while it has the cursor).
pub struct VRow {
    pub label: String,
    pub ctl: VCtl,
    /// The setting does nothing at the moment (another one overrides it): drawn greyed and not clickable (still set by keys).
    pub dim: bool,
}

/// Buttons (caption, the choice that holds), or a slider (stops, the one it is at, the value shown).
pub enum VCtl {
    Buttons(Vec<(String, bool)>),
    Slider { n: usize, at: usize, shown: String },
}

/// The page as the modern layer draws it; the cursor's row (0 the page name) and column, a slider being edited.
pub struct View {
    pub title: &'static str,
    pub tabs: Vec<String>,
    pub tab: usize,
    pub rows: Vec<VRow>,
    pub row: usize,
    pub col: usize,
    pub edit: bool,
    pub help: String,
}

/// A help line for the modern page: without the Config tool's value numbers ("0034: ") and our "(not retail)" notes.
fn plain(help: &str) -> String {
    // The Background row's warning goes first; the numbers come off the help behind it.
    if let Some(rest) = help.strip_prefix(WARNING).map(str::trim_start) {
        return format!("{WARNING} {}", plain(rest));
    }
    let body = match help.split_once(": ") {
        Some((head, rest))
            if head.starts_with(|c: char| c.is_ascii_digit())
                && head.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '/') =>
        {
            rest
        }
        _ => help,
    };
    let body = body.replace(" (not retail)", "");
    let mut c = body.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

impl SettingsUi {
    pub fn view(&self) -> View {
        let rows = if self.on_mods() {
            let offers = self.offers();
            let lua = self.lua_page();
            let installed = self.on_page().into_iter().enumerate().map(|(i, k)| {
                let m = &self.s.mods[k];
                let mut b = vec![("ON".into(), m.enabled), ("OFF".into(), !m.enabled)];
                if !lua {
                    b.push(("Move".into(), false));
                }
                VRow { label: format!("{}. {}", i + 1, m.name), ctl: VCtl::Buttons(b), dim: false }
            });
            installed
                .chain(
                    offers
                        .into_iter()
                        .map(|o| VRow { label: o.label, ctl: VCtl::Buttons(vec![(o.button.into(), false)]), dim: false }),
                )
                .collect()
        } else {
            self.rows().iter().map(|r| self.vrow(r)).collect()
        };
        View {
            title: self.title(),
            tabs: self.tabs(),
            tab: self.tab,
            rows,
            row: self.row,
            col: self.col,
            edit: self.edit,
            help: plain(&self.help()),
        }
    }

    /// The modern layer's mouse: page `t`.
    pub fn m_tab(&mut self, t: usize) {
        if t != self.tab {
            self.edit = false;
            self.turn(t as i32 - self.tab as i32);
        }
    }

    /// The cursor to a control (row from 1); true when it moved.
    pub fn m_point(&mut self, row: usize, col: usize) -> bool {
        if self.edit || self.mods_ui.as_ref().is_some_and(|m| m.held.is_some()) || (row, col) == (self.row, self.col) {
            return false;
        }
        (self.row, self.col) = (row, col);
        let (mods, row) = (self.on_mods(), self.row);
        if let Some(m) = self.mods_ui.as_mut().filter(|_| mods) {
            m.follow(row);
        }
        true
    }

    /// A click on a button: the cursor there and Enter.
    pub fn m_click(&mut self, row: usize, col: usize) {
        self.sounds.clear();
        self.m_point(row, col);
        if self.on_mods() {
            self.mods_enter();
        } else {
            self.enter();
        }
    }

    /// A slider set to stop `i` by the pointer.
    pub fn m_slide(&mut self, row: usize, i: usize) {
        self.sounds.clear();
        let Some(r) = row.checked_sub(1).and_then(|k| self.rows().get(k).cloned()) else { return };
        (self.row, self.col, self.edit) = (row, 0, false);
        let (_, at) = self.stops(&r);
        if at != i {
            self.set_stop(&r, i);
            self.sounds.push(Se::Cursor);
        }
    }
}

/// The DAT Packs and Mods pages (client/modsui.rs): the installed DAT packs (ON / OFF / Move) or Lua mods (ON / OFF),
/// then the offers of that kind (Install / Update).
impl SettingsUi {
    fn on_mods(&self) -> bool {
        self.kind == Kind::Settings && (self.tab == PACKS || self.tab == MODS)
    }

    /// The Mods page (Lua mods), not DAT Packs.
    pub fn lua_page(&self) -> bool {
        self.tab == MODS
    }

    /// The indexes in `s.mods` of the page's kind, in order.
    fn on_page(&self) -> Vec<usize> {
        let lua = self.lua_page();
        (0..self.s.mods.len()).filter(|&i| self.lua_names.contains(&self.s.mods[i].name) == lua).collect()
    }

    /// Rows below the page name.
    fn row_count(&self) -> usize {
        if self.on_mods() { self.mods_total() } else { self.rows().len() }
    }

    fn offers(&self) -> Vec<super::modsui::Offer> {
        self.mods_ui.as_ref().map(|m| m.offers(self.lua_page())).unwrap_or_default()
    }

    fn mods_total(&self) -> usize {
        self.on_page().len() + self.offers().len()
    }

    /// Buttons on a row: ON / OFF (and Move for a pack), or an offer's one.
    fn mods_cols(&self, row: usize) -> usize {
        match row.checked_sub(1) {
            Some(i) if i < self.on_page().len() => {
                if self.lua_page() {
                    2
                } else {
                    3
                }
            }
            _ => 1,
        }
    }

    fn mods_marked(&self) -> usize {
        let sh = self.on_page();
        match self.row.checked_sub(1).and_then(|i| sh.get(i)).map(|&k| &self.s.mods[k]) {
            Some(m) if !m.enabled => 1,
            _ => 0,
        }
    }

    /// Per frame: the archive scan, the catalogue and an install coming in.
    pub fn tick(&mut self) {
        self.expire_review(std::time::Instant::now());
        if let Some(m) = self.mods_ui.as_mut()
            && m.tick(&mut self.s.mods)
        {
            self.dirty = true;
            // A Lua mod just installed starts at once (in game).
            let now = crate::modpack::lua_folders();
            for id in now.iter().filter(|id| !self.lua_names.contains(*id)) {
                if self.s.mods.iter().any(|m| &m.name == id && m.enabled) {
                    self.mod_changes.push((id.clone(), super::lua::ENABLED.into(), true.into()));
                }
            }
            self.lua_names = now;
            self.lua_info = crate::modpack::lua_info();
        }
    }

    fn mods_key(&mut self, key: egui::Key, page_mod: bool) -> bool {
        use egui::Key as K;
        let sh = self.on_page();
        let n = sh.len();
        if let Some(before) = self.mods_ui.as_mut().and_then(|m| m.held.take()) {
            // Moving: Up / Down carry the pack, Enter places it, Escape puts the order back.
            let i = self.row - 1;
            let mut keep = Some(before);
            match key {
                K::ArrowUp if i > 0 => {
                    self.s.mods.swap(sh[i], sh[i - 1]);
                    self.row -= 1;
                    self.sounds.push(Se::Cursor);
                }
                K::ArrowDown if i + 1 < n => {
                    self.s.mods.swap(sh[i], sh[i + 1]);
                    self.row += 1;
                    self.sounds.push(Se::Cursor);
                }
                K::Enter => {
                    self.dirty |= keep.take().is_some_and(|b| b != self.s.mods);
                    self.sounds.push(Se::Select);
                }
                K::Escape => {
                    if let Some(b) = keep.take() {
                        let name = self.s.mods[sh[i]].name.clone();
                        self.s.mods = b;
                        self.row = self
                            .on_page()
                            .iter()
                            .position(|&k| self.s.mods[k].name == name)
                            .map_or(self.row, |k| k + 1);
                    }
                    self.sounds.push(Se::Cancel);
                }
                _ => {}
            }
            if let Some(m) = self.mods_ui.as_mut() {
                m.held = keep;
                m.follow(self.row);
            }
            return false;
        }
        match key {
            K::ArrowDown | K::ArrowUp => {
                let span = self.mods_total() + 1;
                let d = if key == K::ArrowDown { 1 } else { span - 1 };
                self.row = (self.row + d) % span;
                self.col = self.mods_marked().min(self.mods_cols(self.row) - 1);
                self.sounds.push(Se::Cursor);
            }
            K::ArrowLeft | K::ArrowRight => {
                let c = self.mods_cols(self.row);
                let d = if key == K::ArrowRight { 1 } else { -1 };
                if self.row > 0 && c > 1 && !page_mod {
                    self.col = (self.col as i32 + d).rem_euclid(c as i32) as usize;
                    self.sounds.push(Se::Cursor);
                } else {
                    self.turn(d);
                }
            }
            K::Enter => self.mods_enter(),
            K::Tab => self.turn(1),
            K::Escape => return true,
            _ => {}
        }
        if let Some(m) = self.mods_ui.as_mut() {
            m.follow(self.row);
        }
        false
    }

    fn mods_enter(&mut self) {
        let Some(i) = self.row.checked_sub(1) else { return };
        let sh = self.on_page();
        let lua = self.lua_page();
        if let Some(&k) = sh.get(i) {
            if self.col < 2 {
                let on = self.col == 0;
                self.dirty |= self.s.mods[k].enabled != on;
                self.s.mods[k].enabled = on;
            } else if let Some(m) = self.mods_ui.as_mut() {
                m.held = Some(self.s.mods.clone());
            }
            self.sounds.push(Se::Select);
        } else if self.mods_ui.as_mut().is_some_and(|m| m.install_offer(i - sh.len(), lua)) {
            self.sounds.push(Se::Select);
        } else {
            self.sounds.push(Se::Error);
        }
    }

    /// The shown rows' buttons, relative to the page: conf3win's three for a pack (two for a mod), conf2win's first
    /// for an offer.
    fn mods_controls(&self) -> Vec<(usize, usize, [f32; 4])> {
        let top = self.mods_ui.as_ref().map_or(0, |m| m.top);
        let n = self.on_page().len();
        let cols = if self.lua_page() { 2 } else { 3 };
        let mut out = Vec::new();
        for (vis, i) in (top..self.mods_total()).take(super::modsui::VISIBLE).enumerate() {
            let y = Self::TOP + PITCH * vis as f32 + 14.0;
            if i < n {
                for (c, bx) in NARROW.iter().enumerate().take(cols) {
                    out.push((i + 1, c, [*bx, y, 64.0, 16.0]));
                }
            } else {
                out.push((i + 1, 0, [WIDE[0], y, 88.0, 16.0]));
            }
        }
        out
    }

    fn mods_draw(&self, gu: &mut GameUi, x: f32, y: f32) {
        const N: u32 = 0x8080_8080;
        const ON: u32 = 0x8080_5020;
        let m = self.mods_ui.as_ref();
        let (top, held) = (m.map_or(0, |m| m.top), m.is_some_and(|m| m.held.is_some()));
        let sh = self.on_page();
        let n = sh.len();
        let total = self.mods_total();
        let offers = self.offers();
        for (vis, i) in (top..total).take(super::modsui::VISIBLE).enumerate() {
            let hy = y + Self::TOP + PITCH * vis as f32;
            if let Some(b) = gu.cell_bounds("menu    windowps", 465, 2) {
                gu.sprite_cells("menu    windowps", 465, 2..3, x + 20.0 - b[0], hy + 2.0 - b[1], N);
            }
            let (mut label, tint) = match (sh.get(i), offers.get(i.wrapping_sub(n))) {
                (Some(&k), _) => {
                    (format!("{}. {}", i + 1, self.s.mods[k].name), if held && self.row == i + 1 { ON } else { N })
                }
                // A row being installed shows its per cent, lit.
                (None, Some(o)) => (o.label.clone(), if o.label.contains("%  ") { ON } else { N }),
                _ => continue,
            };
            while gu.label_width(&label) > W - 52.0 && label.pop().is_some() {}
            gu.label(&label, x + 32.0, hy, tint, false);
        }
        let captions: [&str; 3] = ["ON", "OFF", "Move"];
        let mut hand = None;
        for (row, c, b) in self.mods_controls() {
            let on = self.row == row && self.col == c;
            let d = if on && !held { -1.0 } else { 0.0 };
            let tint = if on { ON } else { N };
            button(gu, [x + b[0] + d, y + b[1] + d, b[2], b[3]], tint);
            let cap = if row - 1 < n { captions[c] } else { offers.get(row - 1 - n).map_or("Install", |o| o.button) };
            gu.label(cap, x + b[0] + 5.0 + d, y + b[1] + 1.0 + d, tint, true);
            if row - 1 < n && c < 2 && self.s.mods[sh[row - 1]].enabled == (c == 0) {
                marker(gu, x + b[0], y + b[1], b[2]);
            }
            if on {
                hand = Some((x + b[0], y + b[1]));
            }
        }
        if let Some((hx, hy)) = hand {
            gu.cursor(hx, hy);
        }
        if total > super::modsui::VISIBLE {
            gu.scroll_bar(
                x + W - 12.0,
                y + Self::TOP,
                PITCH * super::modsui::VISIBLE as f32,
                total,
                super::modsui::VISIBLE,
                top,
            );
        }
    }

    /// The DATs page (`lua` false) or the Mods page turned to.
    pub fn goto_page(&mut self, lua: bool) {
        self.goto(if lua { MODS } else { PACKS }, 0);
        self.m_mods = (0, 0);
    }

    /// The modern DAT Packs or Mods page is the one open.
    pub fn on_mods_page(&self) -> bool {
        self.on_mods()
    }

    /// The page's installed packs or mods (name, on) in order, then its offers: archives found and catalogue entries.
    /// A Lua mod goes by its mod.toml's name.
    pub fn mods_rows(&self) -> (Vec<(String, bool)>, Vec<super::modsui::Offer>) {
        let name = |n: &String| self.lua_info.get(n).map_or_else(|| n.clone(), |i| i.name.clone());
        let packs = self.on_page().into_iter().map(|k| (name(&self.s.mods[k].name), self.s.mods[k].enabled)).collect();
        (packs, self.offers())
    }

    /// The page's installed pack or mod `i`'s folder (a Lua mod's id).
    pub fn mod_id(&self, i: usize) -> Option<String> {
        self.on_page().get(i).map(|&k| self.s.mods[k].name.clone())
    }

    /// The page's pack or mod `i` on or off.
    pub fn mod_enable(&mut self, i: usize, on: bool) {
        if let Some(&k) = self.on_page().get(i) {
            let lua = self.lua_page();
            let m = &mut self.s.mods[k];
            self.dirty |= m.enabled != on;
            m.enabled = on;
            // A Lua mod starts or stops at once (in game).
            if lua {
                self.mod_changes.push((m.name.clone(), super::lua::ENABLED.into(), on.into()));
            }
            self.sounds.push(Se::Select);
        }
    }

    /// The page's pack `i` a place up (higher priority) or down among the packs; true when it moved.
    pub fn mod_move(&mut self, i: usize, up: bool) -> bool {
        let sh = self.on_page();
        let j = if up { i.checked_sub(1) } else { Some(i + 1).filter(|&j| j < sh.len()) };
        let Some(j) = j.filter(|_| i < sh.len()) else {
            self.sounds.push(Se::Error);
            return false;
        };
        self.s.mods.swap(sh[i], sh[j]);
        self.dirty = true;
        self.sounds.push(Se::Select);
        true
    }

    /// The page's pack or mod `i` removed: its folder deleted and its entry dropped.
    pub fn mod_remove(&mut self, i: usize) {
        let Some(&k) = self.on_page().get(i) else { return };
        if self.lua_page() {
            self.mod_changes.push((self.s.mods[k].name.clone(), super::lua::ENABLED.into(), false.into()));
        }
        if super::modsui::remove_folder(&self.s.mods[k].name) {
            let m = self.s.mods.remove(k);
            self.lua_names.remove(&m.name);
            self.dirty = true;
            self.sounds.push(Se::Select);
        } else {
            self.sounds.push(Se::Error);
        }
    }

    /// The page's offer `k` (an archive found, a catalogue entry) installed.
    pub fn archive_install(&mut self, k: usize) {
        let lua = self.lua_page();
        let ok = self.mods_ui.as_mut().is_some_and(|m| m.install_offer(k, lua));
        self.sounds.push(if ok { Se::Select } else { Se::Error });
    }

    pub fn mods_mut(&mut self) -> Option<&mut super::modsui::Mods> {
        self.mods_ui.as_mut()
    }

    pub fn mods_state(&self) -> Option<&super::modsui::Mods> {
        self.mods_ui.as_ref()
    }

    /// The modern page's help for its cursor (m_mods: the tabs, Install from File, the installed, the offers).
    fn modern_mods_help(&self) -> String {
        let lua = self.lua_page();
        if self.mods_ui.as_ref().is_some_and(|m| m.browse.is_some()) {
            return if lua {
                "Enter: open the folder, or install the Lua mod's folder or archive (zip, 7z, rar). Escape: back."
            } else {
                "Enter: open the folder, or install the archive (zip, 7z, rar). Escape: back."
            }
            .into();
        }
        let n = self.on_page().len();
        match self.m_mods {
            (0, _) => "Left / Right or Tab: another page of settings.".into(),
            (1, _) if lua => {
                "Pick a Lua mod's archive (zip, 7z, rar) or folder to install. It starts at the next login.".into()
            }
            (1, _) => "Pick a DAT pack archive (zip, 7z, rar) to install. Takes effect on restart.".into(),
            (st, b) if st >= 2 && st - 2 < n => match (b, lua) {
                (0, true) => "This mod on or off. Takes effect at the next login.",
                (0, false) => "This pack on or off. Takes effect on restart.",
                (1 | 2, _) => "A higher pack's textures win over a lower one's. Takes effect on restart.",
                (_, true) => "Deletes this mod's folder. Its settings and saved data are kept.",
                _ => "Deletes this pack's folder.",
            }
            .into(),
            (st, _) => self.offers().into_iter().nth(st.saturating_sub(2 + n)).map_or_else(String::new, |o| o.help),
        }
    }

    fn mods_help(&self) -> String {
        let n = self.on_page().len();
        let m = self.mods_ui.as_ref();
        match self.row.checked_sub(1) {
            None => "Left / Right: another page of settings.".into(),
            Some(_) if m.is_some_and(|m| m.held.is_some()) => {
                "Up / Down: move the pack. Enter: place it here. Escape: put it back.".into()
            }
            Some(_) if m.and_then(|m| m.error.as_ref()).is_some() => {
                format!("The last install failed: {}", m.and_then(|m| m.error.clone()).unwrap_or_default())
            }
            Some(i) if i < n && self.lua_page() => "A Lua mod: ON / OFF. Takes effect at the next login.".into(),
            Some(i) if i < n => format!(
                "DAT pack {} of {n}: a higher one's textures win. ON / OFF, or Move it. Takes effect on restart.",
                i + 1
            ),
            Some(i) => self.offers().into_iter().nth(i - n).map_or_else(String::new, |o| o.help),
        }
    }
}

/// A blank Config page button: conf2win's 88-px "ON" (windowps 105) or conf3win's 64-px "Log" (379) without its
/// caption, over the dark copy at (+2, +2) that the windows' own sprites bake in under each button (comwin 12 and
/// windowps 465: buttonto cells coloured (1, 1, 1, 169), alpha blended) and the additive body needs; other widths
/// stretch 105's two buttonto cells.
pub(super) fn button(gu: &mut GameUi, r: [f32; 4], tint: u32) {
    let k = match r[2] as u32 {
        88 => Some(105),
        64 => Some(379),
        _ => None,
    };
    if let Some(k) = k {
        // Tint 1 / 128 takes the cells' colour to about 0; blend word 0x44 = PS2 (Cs - Cd) * As + Cd.
        gu.sprite_base("menu    windowps", k, r[0] + 2.0, r[1] + 2.0, 0x8001_0101, 0x44);
        gu.sprite_base("menu    windowps", k, r[0], r[1], tint, 0);
        return;
    }
    // Other widths: 105's two buttonto cells stretched (the left one to all but the right end's 40 px), base first.
    let left = (r[2] - 40.0).max(8.0);
    for (dx, t, blend) in [(2.0, 0x8001_0101, 0x44), (0.0, tint, 0)] {
        gu.cell_stretch_blend("menu    windowps", 105, 0, r[0] + dx, r[1] + dx, left, r[3], t, blend);
        gu.cell_stretch_blend("menu    windowps", 105, 1, r[0] + dx + left, r[1] + dx, r[2] - left, r[3], t, blend);
    }
}

/// The chosen button's red underline: windowps 387 at (x + 37, y + 4) for an 88-px button, 412 for a 64-px one
/// (FUN_1018f4a0 / FUN_1018f510); other widths stretch 387's middle.
pub(super) fn marker(gu: &mut GameUi, x: f32, y: f32, w: f32) {
    match w as u32 {
        88 => {
            gu.sprite("menu    windowps", 387, x + 37.0, y + 4.0, 0x8080_8080);
        }
        64 => {
            gu.sprite("menu    windowps", 412, x + 37.0, y + 4.0, 0x8080_8080);
        }
        _ => {
            gu.cell_stretch("menu    windowps", 387, 0, x + 6.0, y + 17.0, 9.0, 3.0, 0x8080_8080);
            gu.cell_stretch("menu    windowps", 387, 2, x + 15.0, y + 17.0, w - 30.0, 3.0, 0x8080_8080);
            gu.cell_stretch("menu    windowps", 387, 1, x + w - 15.0, y + 17.0, 8.0, 3.0, 0x8080_8080);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "forever")]
    fn forever_ui() -> (Config, SettingsUi) {
        let c = Config {
            server: "FFXI Forever".into(),
            named_servers: vec![crate::config::NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            ..Default::default()
        };
        let ui = SettingsUi::open(&c, [1280, 800]);
        (c, ui)
    }

    #[cfg(feature = "forever")]
    #[test]
    fn map_placement_buttons_fit_frame_and_keep_full_captions() {
        let (c, _) = forever_ui();
        for modern in [false, true] {
            let mut ui = SettingsUi::open_kind(&c, [1280, 800], Kind::Extras);
            ui.s.modern_ui = modern;
            let rows = ui.rows();
            let row = rows.iter().position(|r| matches!(r.val, Val::MapView(0))).unwrap() + 1;
            let choices = &rows[row - 1].choices;
            assert_eq!(
                choices.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(),
                ["Original", "Centered", "Follow Mouse"]
            );
            let controls: Vec<_> = ui.controls().into_iter().filter(|c| c.0 == row).collect();
            assert_eq!(controls.len(), 3);
            let mut end = 0.0;
            for (_, col, [x, _, w, _]) in controls {
                assert!(x >= end && x + w <= W - 32.0, "placement choice {col} must stay inside the frame");
                assert!(
                    super::super::super::render::screens::label_width(&choices[col].1) + 12.0 <= w,
                    "full caption and button ends must fit"
                );
                end = x + w;
                ui.m_click(row, col);
                assert_eq!(ui.s.map_prefs.placement, choices[col].0);
            }
        }
    }

    #[cfg(feature = "forever")]
    #[test]
    fn forever_map_settings_are_staged_and_absent_on_regular() {
        let (mut c, _) = forever_ui();
        c.map_overlay = true;
        c.map_overlay_opacity = 0.35;
        let before = c.forever_map.clone();
        let mut ui = SettingsUi::open_kind(&c, [1280, 800], Kind::Extras);
        assert_eq!(ui.rows().iter().filter(|r| matches!(r.val, Val::MapView(_))).count(), 8);
        ui.set(Val::MapView(0), 2);
        ui.set(Val::MapView(3), 10);
        ui.set(Val::MapView(6), 0);
        ui.set(Val::MapView(7), 150);
        ui.effective().store(&mut c);
        assert_eq!(c.forever_map, before);
        ui.cancel();
        assert_eq!(ui.s.map_prefs, before);
        ui.begin();
        ui.set(Val::MapView(0), 2);
        ui.set(Val::MapView(3), 10);
        ui.set(Val::MapView(6), 2);
        ui.set(Val::MapView(7), 125);
        ui.win_press(1);
        ui.effective().store(&mut c);
        assert_eq!((c.forever_map.placement, c.forever_map.moving_opacity), (2, 10));
        assert_eq!((c.forever_map.navigation, c.forever_map.navigation_scale), (2, 125));
        ui.begin();
        ui.set(Val::MapView(7), 50);
        ui.cancel();
        assert_eq!(ui.s.map_prefs.navigation_scale, 125);
        ui.begin();
        ui.set(Val::MapView(7), 50);
        ui.win_press(1);
        ui.effective().store(&mut c);
        assert_eq!(c.forever_map.navigation_scale, 50);
        assert!(c.map_overlay);
        assert_eq!(c.map_overlay_opacity, 0.35);
        c.server = "regular".into();
        let ui = SettingsUi::open_kind(&c, [1280, 800], Kind::Extras);
        assert!(!ui.rows().iter().any(|r| matches!(r.val, Val::MapView(_))));
    }

    #[cfg(feature = "forever")]
    #[test]
    fn enhanced_graphics_drafts_revert_and_requires_forever() {
        let (mut c, mut ui) = forever_ui();
        ui.set(Val::ForeverLook, 1);
        ui.effective().store(&mut c);
        assert!(!c.graphics.enhanced);
        ui.cancel();
        assert!(!ui.s.graphics.enhanced);
        ui.begin();
        ui.set(Val::ForeverLook, 1);
        ui.win_press(1);
        assert!(ui.review.is_some());
        ui.effective().store(&mut c);
        assert!(c.graphics_effects().enhanced && c.graphics_effects().ao_on());
        assert_eq!(c.graphics.character_light, 0.75);
        assert_eq!(c.background([1920, 1080]), [1920, 1080]);
        c.server = "regular".into();
        assert!(!c.graphics_effects().enhanced && !c.graphics_effects().ao_on() && !c.graphics_effects().tonemap);
        assert!(c.graphics_effects().fxaa && c.graphics_effects().sharpen_on());
        ui.finish_review(false);
        assert!(!ui.s.graphics.enhanced);
        let mut regular = SettingsUi::open(&c, [1280, 800]);
        regular.tab = 1;
        assert!(!regular.rows().iter().any(|r| matches!(r.val, Val::ForeverLook)));
    }

    #[cfg(feature = "forever")]
    #[test]
    fn layered_fog_and_sun_shadow_range_are_forever_rows_that_store_and_reset() {
        let (mut c, mut ui) = forever_ui();
        let tabs = |ui: &mut SettingsUi, v: fn(&Val) -> bool| {
            (0..TABS.len())
                .filter(|&t| {
                    ui.tab = t;
                    ui.rows().iter().any(|r| v(&r.val))
                })
                .count()
        };
        assert_eq!(tabs(&mut ui, |v| matches!(v, Val::FogRamp)), 1);
        assert_eq!(tabs(&mut ui, |v| matches!(v, Val::ShadowRange)), 1);
        assert_eq!(tabs(&mut ui, |v| matches!(v, Val::SoftShadows)), 1);
        assert_eq!(tabs(&mut ui, |v| matches!(v, Val::FarView)), 1);
        assert_eq!(tabs(&mut ui, |v| matches!(v, Val::ActorShade)), 1);
        assert_eq!(tabs(&mut ui, |v| matches!(v, Val::ShadowCascades)), 1);
        assert_eq!(tabs(&mut ui, |v| matches!(v, Val::AoLight)), 1);
        for v in [
            |v: &Val| matches!(v, Val::FogLayers),
            |v: &Val| matches!(v, Val::WaterEnhanced),
            |v: &Val| matches!(v, Val::SkyClouds),
            |v: &Val| matches!(v, Val::LightGrade),
            |v: &Val| matches!(v, Val::LightGradeAmount),
            |v: &Val| matches!(v, Val::DetailTextures),
            |v: &Val| matches!(v, Val::DetailStrength),
            |v: &Val| matches!(v, Val::TextureSharpening),
            |v: &Val| matches!(v, Val::SunShafts),
            |v: &Val| matches!(v, Val::SunShaftsAmount),
            |v: &Val| matches!(v, Val::VolumetricFog),
            |v: &Val| matches!(v, Val::VolumetricFogAmount),
        ] {
            assert_eq!(tabs(&mut ui, v), 1);
        }
        ui.begin();
        ui.set(Val::ForeverLook, 1);
        // Enhanced brings the new options with it, wave 3's included.
        assert!(ui.s.graphics.fog_ramp && ui.s.graphics.shadow_range == 1);
        assert!(ui.s.graphics.light_grade && ui.s.graphics.fog_layers && ui.s.graphics.ao_light == 1);
        assert!(ui.s.graphics.soft_shadows && ui.get(Val::SoftShadows) == 1);
        assert_eq!(ui.get(Val::WaterEnhanced), 1);
        ui.set(Val::WaterEnhanced, 0);
        assert!(!ui.s.graphics.water_enhanced);
        ui.set(Val::WaterEnhanced, 1);
        assert_eq!(ui.get(Val::FarView), 1);
        ui.set(Val::FarView, 2);
        assert_eq!((ui.get(Val::FarView), ui.s.graphics.far_view_factor()), (2, 2.0));
        ui.set(Val::FarView, 9);
        assert_eq!(ui.get(Val::FarView), 2);
        ui.set(Val::FarView, 0);
        assert_eq!(ui.s.graphics.far_view_factor(), 1.0);
        ui.set(Val::FarView, 1);
        assert!(ui.s.graphics.shadow_cascades && ui.get(Val::ShadowCascades) == 1);
        ui.set(Val::ShadowCascades, 0);
        assert!(!ui.s.graphics.cascades_on());
        ui.set(Val::ShadowCascades, 1);
        assert!(ui.s.graphics.actor_sun_shadows && ui.get(Val::ActorShade) == 1);
        ui.set(Val::ActorShade, 0);
        assert_eq!(ui.s.graphics.actor_shade_amount(), 0.0);
        ui.set(Val::ActorShade, 1);
        assert_eq!(ui.get(Val::SkyClouds), 1);
        ui.set(Val::SkyClouds, 0);
        assert!(!ui.s.graphics.sky_clouds);
        ui.set(Val::SkyClouds, 1);
        assert_eq!((ui.get(Val::LightGrade), ui.get(Val::LightGradeAmount), ui.get(Val::FogLayers)), (1, 100, 1));
        ui.set(Val::ShadowRange, 2);
        ui.set(Val::SoftShadows, 0);
        assert_eq!(ui.get(Val::SoftShadows), 0);
        ui.set(Val::SoftShadows, 1);
        ui.set(Val::FogRamp, 1);
        ui.set(Val::LightGradeAmount, 150);
        ui.set(Val::LightGradeAmount, 900);
        assert_eq!(ui.get(Val::LightGradeAmount), 150);
        ui.set(Val::LightGradeAmount, 75);
        // Volumetric Fog comes with Enhanced (on, 100%), stores, clamps to 150%, needs the Height Fog and is dropped by Original.
        assert_eq!((ui.get(Val::VolumetricFog), ui.get(Val::VolumetricFogAmount)), (1, 100));
        ui.set(Val::VolumetricFogAmount, 900);
        assert_eq!(ui.get(Val::VolumetricFogAmount), 150);
        ui.set(Val::VolumetricFogAmount, 50);
        ui.set(Val::VolumetricFog, 0);
        assert_eq!(ui.s.graphics.volumetric_fog_on(), 0.0);
        ui.set(Val::VolumetricFog, 1);
        assert!((ui.s.graphics.volumetric_fog_on() - 0.5).abs() < 1e-4);
        // Sun Shafts come with Enhanced (on, 100%), store, clamp to 150% and are dropped by the Original look.
        assert_eq!((ui.get(Val::SunShafts), ui.get(Val::SunShaftsAmount)), (1, 100));
        ui.set(Val::SunShaftsAmount, 900);
        assert_eq!(ui.get(Val::SunShaftsAmount), 150);
        ui.set(Val::SunShaftsAmount, 50);
        ui.set(Val::SunShafts, 0);
        assert_eq!(ui.s.graphics.sun_shafts_on(), 0.0);
        ui.set(Val::SunShafts, 1);
        assert!((ui.s.graphics.sun_shafts_on() - 0.5).abs() < 1e-4);
        // Wave 4: Texture Sharpening comes with Enhanced (Medium); the Detail Textures grain does not (a Settings choice). Both
        // store, clamp and are dropped by the Original look.
        assert_eq!((ui.get(Val::DetailTextures), ui.get(Val::DetailStrength), ui.get(Val::TextureSharpening)), (0, 100, 100));
        ui.set(Val::DetailTextures, 1);
        ui.set(Val::DetailStrength, 250);
        assert_eq!(ui.get(Val::DetailStrength), 200);
        ui.set(Val::DetailStrength, 50);
        ui.set(Val::DetailTextures, 0);
        assert_eq!(ui.s.graphics.detail_strength_on(), 0.0);
        ui.set(Val::DetailTextures, 1);
        assert!((ui.s.graphics.detail_strength_on() - 0.5).abs() < 1e-4);
        // Texture Sharpening: stored, clamped to 150%, off by default and dropped by the Original look.
        ui.set(Val::TextureSharpening, 130);
        assert!((ui.s.graphics.texture_sharpening_on() - 1.3).abs() < 1e-4);
        ui.set(Val::TextureSharpening, 900);
        assert_eq!(ui.get(Val::TextureSharpening), 150);
        ui.set(Val::TextureSharpening, 0);
        ui.set(Val::AoLight, 0);
        assert_eq!(ui.s.graphics.ao_light, 0);
        ui.set(Val::AoLight, 1);
        ui.win_press(1);
        ui.finish_review(true);
        ui.effective().store(&mut c);
        assert_eq!(
            (c.graphics.shadow_range, c.graphics.fog_ramp, c.graphics.sun_shadow_map()),
            (2, true, (4096, 120.0))
        );
        assert!(c.graphics.soft_shadows);
        assert!(c.graphics.light_grade && (c.graphics.light_grade_amount - 0.75).abs() < 1e-4);
        // The Original look is the default graphics whole: stale ramp and range values do not survive it.
        let character_light = c.graphics.character_light;
        ui.begin();
        ui.set(Val::ForeverLook, 0);
        ui.win_press(1);
        ui.finish_review(true);
        ui.effective().store(&mut c);
        assert_eq!(c.graphics, crate::config::GraphicsCfg { character_light, ..Default::default() });
        assert_eq!(c.graphics.sun_shadow_map(), (1024, 40.0));
        // A regular server shows neither row.
        c.server = "regular".into();
        let mut regular = SettingsUi::open(&c, [1280, 800]);
        assert_eq!(
            tabs(&mut regular, |v| matches!(
                v,
                Val::FogRamp
                    | Val::ShadowRange
                    | Val::SoftShadows
                    | Val::ActorShade
                    | Val::ShadowCascades
                    | Val::FarView
                    | Val::AoLight
                    | Val::FogLayers
                    | Val::WaterEnhanced
                    | Val::SkyClouds
                    | Val::LightGrade
                    | Val::LightGradeAmount
                    | Val::SunShafts
                    | Val::SunShaftsAmount
                    | Val::VolumetricFog
                    | Val::VolumetricFogAmount
                    | Val::DetailTextures
                    | Val::DetailStrength
                    | Val::TextureSharpening
            )),
            0
        );
    }

    /// While Shadow Cascades are on, Sun Shadow Range says they set the reach, and is greyed (Forever).
    #[cfg(feature = "forever")]
    #[test]
    fn sun_shadow_range_says_shadow_cascades_set_the_reach_and_is_dimmed() {
        let (_c, mut ui) = forever_ui();
        ui.begin();
        ui.set(Val::ForeverLook, 1);
        let item = |ui: &SettingsUi| ui.item("Sun Shadow Range").unwrap();
        assert!(ui.s.graphics.cascades_on());
        let (row, help) = item(&ui);
        assert!(row.dim, "greyed while the cascades are on");
        assert!(
            help.starts_with("Shadow Cascades are on: they set the shadow reach; this applies when they're off."),
            "{help}"
        );
        assert!(help.contains("How far the Enhanced sun shadows reach"), "the row's own help follows: {help}");
        // Cascades off: the plain row.
        ui.set(Val::ShadowCascades, 0);
        let (row, help) = item(&ui);
        assert!(!row.dim && !help.contains("Shadow Cascades are on"), "{help}");
        // Original look: the cascades are not on either (they need Enhanced).
        ui.set(Val::ShadowCascades, 1);
        ui.set(Val::ForeverLook, 0);
        let (row, help) = item(&ui);
        assert!(!row.dim && !help.contains("Shadow Cascades are on"), "{help}");
        // The other rows are not dimmed.
        ui.set(Val::ForeverLook, 1);
        assert!(!ui.item("Soft Shadows").unwrap().0.dim && !ui.item("Shadow Cascades").unwrap().0.dim);
    }

    /// Draw Distance and Far View explain each other in their help, following the drafts of both (Forever).
    #[cfg(feature = "forever")]
    #[test]
    fn draw_distance_and_far_view_help_explain_how_they_compound() {
        let (_c, mut ui) = forever_ui();
        ui.begin();
        ui.set(Val::ForeverLook, 1);
        let help = |ui: &SettingsUi, l: &str| ui.find(l).unwrap().help;
        // The default Draw Distance is Unlimited: Far View says the distance limit is already lifted, Draw Distance says the same.
        ui.set(Val::DrawDistance, 0);
        assert!(help(&ui, "Far View").contains("Unlimited (the default)") && help(&ui, "Far View").contains("fog end"));
        assert!(help(&ui, "Draw Distance").contains("Unlimited lifts") && help(&ui, "Draw Distance").contains("Far View (Far)"));
        // Retail: Far View alone sets the distance; Draw Distance's Retail is Far View's multiple of retail's.
        ui.set(Val::DrawDistance, 100);
        assert!(help(&ui, "Far View").contains("Retail, so Far View alone"));
        assert!(help(&ui, "Draw Distance").contains("\"Retail\" here is 1.5x retail's distance"));
        // A multiple compounds.
        ui.set(Val::DrawDistance, 200);
        ui.set(Val::FarView, 2);
        assert!(help(&ui, "Far View").contains("(2x) multiplies") && help(&ui, "Far View").contains("together 4x"));
        assert!(help(&ui, "Draw Distance").contains("compounds") && help(&ui, "Draw Distance").contains("is 4x retail's"));
        // Far View at Retail (or Original): Draw Distance only points at it.
        ui.set(Val::FarView, 0);
        assert!(help(&ui, "Draw Distance").contains("also moves this distance"));
        ui.set(Val::ForeverLook, 0);
        assert!(help(&ui, "Far View").contains("Needs Forever Graphics: Enhanced"));
        // The static text is untouched in the table.
        assert!(rows(Kind::Settings, GRAPHICS, false, false).iter().any(|r| r.label == "Draw Distance" && !r.help.contains("Far View")));
    }

    #[cfg(feature = "forever")]
    #[test]
    fn upscaler_rows_are_forever_only_and_survive_the_look_presets() {
        let (mut c, mut ui) = forever_ui();
        ui.tab = 1;
        assert!(ui.rows().iter().any(|r| matches!(r.val, Val::Upscaler)), "Upscaler row on the Graphics page");
        assert!(ui.rows().iter().any(|r| matches!(r.val, Val::UpscaleQuality)), "Quality row on the Graphics page");
        assert_eq!((ui.get(Val::Upscaler), ui.get(Val::UpscaleQuality)), (0, 1));
        // The Enhanced look (like Original) leaves the Upscaler at Standard.
        ui.set(Val::ForeverLook, 1);
        assert_eq!(ui.get(Val::Upscaler), 0);
        ui.set(Val::ForeverLook, 0);
        assert_eq!(ui.get(Val::Upscaler), 0);
        let preset = ui.preset();
        ui.set(Val::Upscaler, 1);
        ui.set(Val::UpscaleQuality, 3);
        // Neither the preset row nor the Forever Graphics row relabels because of it.
        assert_eq!(ui.preset(), preset);
        assert_eq!(ui.get(Val::ForeverLook), 0);
        // The look presets change the lighting, not the upscaler.
        ui.set(Val::ForeverLook, 1);
        assert_eq!((ui.get(Val::Upscaler), ui.get(Val::UpscaleQuality)), (1, 3));
        assert!(ui.s.graphics.enhanced);
        ui.set(Val::ForeverLook, 0);
        assert_eq!((ui.get(Val::Upscaler), ui.get(Val::UpscaleQuality)), (1, 3));
        // Out-of-range values are clamped; the choice reaches the config.
        ui.set(Val::Upscaler, 7);
        ui.set(Val::UpscaleQuality, 7);
        assert_eq!((ui.get(Val::Upscaler), ui.get(Val::UpscaleQuality)), (crate::config::UPSCALER_MAX, 3));
        ui.set(Val::Upscaler, 1);
        ui.store_into(&mut c);
        assert_eq!(c.graphics_effects().fsr_factor(), Some(2.0));
        ui.set(Val::Upscaler, 0);
        ui.store_into(&mut c);
        assert_eq!(c.graphics_effects().fsr_factor(), None);
        // A regular server's window has no such rows and cannot set them.
        c.server = "regular".into();
        let mut regular = SettingsUi::open(&c, [1280, 800]);
        regular.tab = 1;
        assert!(!regular.rows().iter().any(|r| matches!(r.val, Val::Upscaler | Val::UpscaleQuality)));
        regular.set(Val::Upscaler, 1);
        assert_eq!(regular.get(Val::Upscaler), 0);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn environment_art_is_opt_in_deferred_and_forever_enhanced_only() {
        let (mut c, mut ui) = forever_ui();
        assert!(!c.environment_textures && !c.environment_textures_enabled());
        ui.tab = GRAPHICS;
        assert!(ui.rows().iter().any(|r| matches!(r.val, Val::EnvironmentTextures) && r.when == When::NextZone));
        ui.set(Val::EnvironmentTextures, 1);
        ui.effective().store(&mut c);
        assert!(!c.environment_textures);
        ui.cancel();
        assert!(!ui.s.environment_textures);
        ui.begin();
        ui.set(Val::EnvironmentTextures, 1);
        ui.win_press(1);
        ui.effective().store(&mut c);
        assert!(c.environment_textures && !c.environment_textures_enabled());
        c.graphics = crate::config::GraphicsCfg::enhanced();
        assert!(c.environment_textures_enabled());
        let loaded: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert!(loaded.environment_textures_enabled());
        c.graphics.enhanced = false;
        assert!(!c.environment_textures_enabled() && c.environment_textures);
        c.graphics.enhanced = true;
        c.server = "regular".into();
        assert!(!c.environment_textures_enabled());
        let mut regular = SettingsUi::open(&c, [1280, 800]);
        regular.tab = GRAPHICS;
        assert!(!regular.rows().iter().any(|r| matches!(r.val, Val::EnvironmentTextures)));
        regular.set(Val::EnvironmentTextures, 0);
        regular.s.store(&mut c);
        assert!(c.environment_textures, "preserve dormant preference");
    }

    #[cfg(feature = "forever")]
    #[test]
    fn tower_shortcut_is_staged_reversible_and_preserves_other_graphics() {
        let (mut c, _) = forever_ui();
        c.graphics.enhanced = false;
        c.graphics.bloom_intensity = 0.27;
        c.woods_textures = true;
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        ui.begin();
        ui.stage_tower_preview(true);
        ui.effective().store(&mut c);
        assert!(!c.environment_textures && !c.graphics.enhanced);
        ui.cancel();
        assert!(!ui.s.environment_textures);
        ui.begin();
        ui.stage_tower_preview(true);
        ui.win_press(1);
        assert!(ui.review.is_some());
        ui.finish_review(true);
        ui.effective().store(&mut c);
        assert!(c.environment_textures_enabled() && c.woods_textures);
        assert_eq!(c.graphics.bloom_intensity, 0.27);
        ui.stage_tower_preview(false);
        ui.win_press(1);
        ui.effective().store(&mut c);
        assert!(!c.environment_textures && c.graphics.enhanced && c.woods_textures);
        c.server = "regular".into();
        let mut normal = SettingsUi::open(&c, [1280, 800]);
        normal.stage_tower_preview(true);
        assert!(!normal.s.environment_textures);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn woods_art_is_independent_deferred_and_preserves_saved_tower_choice() {
        let (mut c, _) = forever_ui();
        c.environment_textures = true;
        c.graphics = crate::config::GraphicsCfg::enhanced();
        let old = serde_json::to_value(&c).unwrap();
        let mut old = old.as_object().unwrap().clone();
        old.remove("woods_textures");
        c = serde_json::from_value(serde_json::Value::Object(old)).unwrap();
        assert!(!c.woods_textures && c.environment_textures_enabled_for(242));
        assert!(!c.environment_textures_enabled_for(241));
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        ui.tab = GRAPHICS;
        assert!(ui.rows().iter().any(|r| matches!(r.val, Val::WoodsTextures) && r.when == When::NextZone));
        ui.set(Val::WoodsTextures, 1);
        ui.effective().store(&mut c);
        assert!(!c.woods_textures);
        ui.cancel();
        assert!(!ui.s.woods_textures);
        ui.begin();
        ui.set(Val::WoodsTextures, 1);
        ui.win_press(1);
        ui.effective().store(&mut c);
        assert!(c.environment_textures_enabled_for(241) && c.environment_textures_enabled_for(242));
        assert!(!c.environment_textures_enabled_for(240));
        let loaded: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert!(loaded.woods_textures && loaded.environment_textures);
        c.graphics.enhanced = false;
        assert!(!c.environment_textures_enabled_for(241));
        c.graphics.enhanced = true;
        c.server = "regular".into();
        assert!(!c.environment_textures_enabled_for(241));
        let mut regular = SettingsUi::open(&c, [1280, 800]);
        regular.tab = GRAPHICS;
        assert!(!regular.rows().iter().any(|r| matches!(r.val, Val::WoodsTextures)));
        regular.set(Val::WoodsTextures, 0);
        regular.s.store(&mut c);
        assert!(c.woods_textures, "preserve dormant preference on regular server");
    }

    #[cfg(feature = "forever")]
    #[test]
    fn quest_tracker_settings_apply_cancel_and_gates() {
        let (mut c, mut ui) = forever_ui();
        assert!(!c.quest_tracker);
        ui.set(Val::QuestTracker, 1);
        ui.effective().store(&mut c);
        assert!(!c.quest_tracker);
        ui.cancel();
        assert!(!ui.s.quest_tracker);
        ui.begin();
        ui.set(Val::QuestTracker, 1);
        ui.win_press(1);
        ui.effective().store(&mut c);
        assert!(serde_json::from_str::<Config>(&serde_json::to_string(&c).unwrap()).unwrap().quest_tracker);
        for classic_only in [false, true] {
            let c = if classic_only { Config { classic_only: true, ..c.clone() } } else { Config::default() };
            let mut ui = SettingsUi::open(&c, [1280, 800]);
            for tab in 0..TABS.len() {
                ui.tab = tab;
                assert!(!ui.rows().iter().any(|r| matches!(r.val, Val::QuestTracker)));
            }
        }
    }

    /// The Forever Enhancements rows read and write their config values (the page rows are the same settings as Special's and the
    /// Training menu's), store back through the draft, are on only in the Forever flavour's settings, and the one option that
    /// changes what the character wears stays Off.
    #[cfg(feature = "forever")]
    #[test]
    fn forever_enhancement_rows_store_their_config_values_and_auto_equip_stays_off() {
        let (mut c, mut ui) = forever_ui();
        c.quest_giver_markers = true;
        c.rotation_autocast = true;
        c.training_hud = true;
        c.auto_navigation = true;
        let mut ui2 = SettingsUi::open(&c, [1280, 800]);
        for (v, want) in [
            (Val::QuestIndicators, 1),
            (Val::AutoRotationStart, 1),
            (Val::TrainingHud, 1),
            (Val::AutoNavigation, 1),
            (Val::AutoEquipUpgrades, 0),
        ] {
            assert_eq!(ui2.get(v), want);
        }
        ui2.begin();
        for v in [Val::QuestIndicators, Val::AutoRotationStart, Val::TrainingHud, Val::AutoNavigation] {
            ui2.set(v, 0);
        }
        ui2.win_press(1);
        ui2.effective().store(&mut c);
        assert!(!c.quest_giver_markers && !c.rotation_autocast && !c.training_hud && !c.auto_navigation);
        assert!(!c.auto_equip_upgrades);
        // The rows exist in the Forever flavour's settings (the page test, every_setting_has_a_page, then gives each a place).
        let labels = SettingsUi::all_labels(true, true);
        for l in ["Training Tracker", "Quest Indicators", "Auto Navigation", "Auto Rotation at Start", "Quest Tracker"] {
            assert!(labels.contains(&l), "{l}");
        }
        let _ = &mut ui;
    }

    #[cfg(feature = "forever")]
    #[test]
    fn forever_controls_draft_cancel_apply_and_persistence() {
        let (mut c, mut ui) = forever_ui();
        assert!(!c.invert_horizontal_mouse && ui.deferred());
        ui.set(Val::InvertHorizontalMouse, 1);
        ui.effective().store(&mut c);
        assert!(!c.invert_horizontal_mouse);
        ui.cancel();
        assert!(!ui.s.invert_horizontal_mouse);
        ui.begin();
        ui.set(Val::InvertHorizontalMouse, 1);
        ui.win_press(1);
        ui.effective().store(&mut c);
        assert!(c.invert_horizontal_mouse && ui.review.is_none());
        let saved: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert!(saved.invert_horizontal_mouse);
        ui.set(Val::InvertHorizontalMouse, 0);
        ui.cancel();
        ui.effective().store(&mut c);
        assert!(c.invert_horizontal_mouse);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn forever_controls_display_apply_reverts_without_confirmation() {
        let (mut c, mut ui) = forever_ui();
        ui.set(Val::ModernUi, 0);
        ui.s.reset_places = true;
        ui.effective().store(&mut c);
        assert!(c.modern_enabled() && !ui.take_reset_places());
        assert_eq!(ui.win_press(2), Done::Stay);
        assert!(!ui.closing && ui.review.is_some());
        ui.effective().store(&mut c);
        assert!(!c.modern_enabled() && !ui.take_reset_places());
        let deadline = ui.review.as_ref().unwrap().deadline;
        ui.expire_review(deadline - std::time::Duration::from_millis(1));
        assert!(ui.review.is_some());
        ui.expire_review(deadline);
        ui.effective().store(&mut c);
        assert!(c.modern_enabled() && !ui.closing && !ui.take_reset_places());
    }

    #[cfg(feature = "forever")]
    #[test]
    fn forever_controls_display_keep_and_escape_revert() {
        let (mut c, mut ui) = forever_ui();
        ui.set(Val::ModernUi, 0);
        ui.win_press(2);
        assert!(ui.review_key(egui::Key::Escape));
        ui.effective().store(&mut c);
        assert!(c.modern_enabled());
        ui.set(Val::ModernUi, 0);
        ui.s.reset_places = true;
        ui.win_press(2);
        ui.finish_review(true);
        ui.effective().store(&mut c);
        assert!(!c.modern_enabled() && ui.closing && ui.review.is_none());
        assert!(ui.take_reset_places());
        assert!(!ui.take_reset_places());
        ui.begin();
        ui.set(Val::ModernUi, 1);
        ui.cancel();
        assert!(!ui.effective().modern_ui);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn forever_controls_display_resolution_is_deferred_and_reviewed() {
        let (mut c, mut ui) = forever_ui();
        let original = ui.s.tool.clone();
        ui.tool_set("0034", 2);
        ui.tool_set("0001", 1920);
        ui.tool_set("0002", 1080);
        assert_eq!(ui.effective().tool, original);
        ui.commit();
        assert!(ui.review.is_some());
        ui.finish_review(false);
        ui.effective().store(&mut c);
        assert_eq!(ui.s.tool, original);
    }

    #[test]
    fn shared_quality_regular_settings_apply_cancel_and_timed_revert() {
        let mut c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        assert!(ui.deferred());
        ui.s.frame_limit = 30;
        ui.effective().store(&mut c);
        assert_eq!(c.frame_limit, 60);
        ui.cancel();
        assert_eq!(ui.s.frame_limit, 60);
        ui.begin();
        ui.s.frame_limit = 30;
        ui.commit();
        ui.effective().store(&mut c);
        assert_eq!(c.frame_limit, 30);
        assert!(ui.review.is_none());
        ui.set(Val::Tool("0034"), 2);
        ui.effective().store(&mut c);
        assert_ne!(c.tool("0034"), 2);
        ui.commit();
        let deadline = ui.review.as_ref().unwrap().deadline;
        ui.effective().store(&mut c);
        assert_eq!(c.tool("0034"), 2);
        ui.expire_review(deadline);
        ui.effective().store(&mut c);
        assert_ne!(c.tool("0034"), 2);
        assert_eq!(c.frame_limit, 30, "revert preserves the earlier applied cap");
        assert!(!SettingsUi::open_kind(&c, [1280, 800], Kind::Extras).deferred());
    }

    #[test]
    fn shared_quality_regular_rows_and_presets_exclude_stylized_effects() {
        let c: Config = serde_json::from_str(r#"{"server":"regular","lamp_lights":true,"graphics":{"enhanced":true,"bloom":true,"tonemap":true,"ao":true,"fog":true,"saturation":1.5,"fxaa":true,"sharpen":0.2}}"#).unwrap();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        let mut labels = Vec::new();
        for tab in 0..TABS.len() {
            ui.tab = tab;
            labels.extend(ui.rows().into_iter().map(|r| r.label));
        }
        for name in ["Anisotropic Filtering", "MIP Mapping", "Anti-aliasing (FXAA)", "Sharpening", "Reduce Motion"] {
            assert!(labels.iter().any(|s| s == name), "missing {name}");
        }
        for name in [
            "Forever Graphics",
            "Environment Textures",
            "Lamp Lights",
            "Bloom",
            "Tone Mapping",
            "Ambient Occlusion",
            "Height Fog",
            "Character Light",
            "Interface",
        ] {
            assert!(!labels.iter().any(|s| s == name), "unexpected {name}");
        }
        for preset in 0..3 {
            ui.apply_preset(preset);
            assert!(
                !ui.s.lamp_lights
                    && !ui.s.graphics.bloom
                    && !ui.s.graphics.tonemap
                    && !ui.s.graphics.ao
                    && !ui.s.graphics.fog
            );
            assert_eq!(ui.s.graphics.saturation, 1.0);
            assert_eq!(ui.s.anisotropy, 16);
            assert_eq!(ui.tool_get("0000"), 1);
        }
        let mut saved = c.clone();
        ui.s.store(&mut saved);
        assert!(
            saved.graphics.bloom && saved.graphics.tonemap && saved.lamp_lights,
            "dormant preferences are preserved"
        );
        assert!(!saved.graphics_effects().bloom && !saved.lamp_lights_enabled());
    }

    #[test]
    #[ignore = "requires player-owned UI data"]
    fn shared_quality_classic_pointer_apply_cancel_keep_and_revert() -> anyhow::Result<()> {
        let fixture = Config::load();
        let index = crate::dat::DatIndex::open(&fixture.ffxi_dir())?;
        let dll = std::fs::read(fixture.ffxi_dir().join("FFXiMain.dll"))?;
        let gu = GameUi::new(super::super::game_ui::GameUiData::load(&index, &dll)?);
        let c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        let click = |ui: &mut SettingsUi, r: [f32; 4]| {
            let (x, y) = ui.origin(&gu, 1280.0, 800.0);
            ui.pointer(&gu, (x + r[0] + r[2] / 2.0, y + r[1] + r[3] / 2.0), true, true, 1280.0, 800.0);
        };
        ui.s.frame_limit = 30;
        let r = ui.footer_rects()[2];
        click(&mut ui, r);
        assert!(ui.closing);
        assert_eq!(ui.s.frame_limit, 60);
        ui.begin();
        ui.s.frame_limit = 30;
        let r = ui.footer_rects()[0];
        click(&mut ui, r);
        assert_eq!(ui.effective().frame_limit, 30);
        assert!(!ui.closing);
        ui.set(Val::Tool("0034"), 2);
        let r = ui.footer_rects()[1];
        click(&mut ui, r);
        assert!(ui.review.is_some() && !ui.closing);
        let r = ui.review_rects()[1];
        click(&mut ui, r);
        assert!(ui.review.is_none() && !ui.closing);
        assert_ne!(ui.tool_get("0034"), 2);
        ui.set(Val::Tool("0034"), 2);
        let r = ui.footer_rects()[1];
        click(&mut ui, r);
        let r = ui.review_rects()[0];
        click(&mut ui, r);
        assert!(ui.review.is_none() && ui.closing);
        assert_eq!(ui.tool_get("0034"), 2);
        Ok(())
    }

    #[cfg(not(feature = "forever"))]
    #[test]
    fn classic_flavor_exposes_no_forever_settings() {
        // Even a copied Forever profile cannot activate its windows or settings in the classic executable.
        let c: Config = serde_json::from_str(r#"{"server":"FFXI Forever","modern_ui":true,"auto_navigation":true,"classic_only":false,"named_servers":[{"name":"FFXI Forever"}]}"#).unwrap();
        assert!(!c.is_forever() && !c.modern_enabled());
        assert!(serde_json::to_value(&c).unwrap().get("quest_tracker").is_none());
        assert!(serde_json::to_value(&c).unwrap().get("quest_giver_markers").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("auto_navigation").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("auto_equip_upgrades").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("con_colors").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("forever_compact_hud").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("forever_compact_on").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("auto_check_on_target").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("nav_mesh").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("environment_textures").is_none());
        assert!(serde_json::to_value(Config::default()).unwrap().get("woods_textures").is_none());
        // A copied unknown key is preserved verbatim for round trips, not exposed
        // as a classic setting or compiled navigation feature.
        assert_eq!(c.unknown.get("auto_navigation"), Some(&serde_json::json!(true)));
        for kind in [Kind::Settings, Kind::Extras] {
            let mut ui = SettingsUi::open_kind(&c, [1280, 800], kind);
            assert!(!ui.s.forever && !ui.s.modern_ui);
            for tab in 0..ui.tabs().len() {
                ui.tab = tab;
                assert!(!ui.rows().iter().any(|r| matches!(
                    r.label.as_str(),
                    "Auto Navigation" | "Environment Textures" | "Navigation Mesh"
                )));
                assert!(!ui.rows().iter().any(|r| matches!(
                    r.val,
                    Val::ModernUi
                        | Val::Experience
                        | Val::ModernScale
                        | Val::ModernOpacity
                        | Val::ResetPlaces
                        | Val::ModernMinimap
                        | Val::MinimapUnowned
                        | Val::JobPanel
                        | Val::ExpBar
                )));
            }
        }
    }

    #[test]
    fn regular_server_cannot_select_modern_experience() {
        let mut c = Config { modern_ui: true, ..Default::default() };
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        assert!(!ui.s.modern_ui);
        for tab in 0..TABS.len() {
            ui.tab = tab;
            assert!(!ui.rows().iter().any(|r| matches!(r.val, Val::ModernUi | Val::Experience)));
        }
        // A stale snapshot cannot enable Modern on regular FFXI; its saved Forever preference is retained.
        ui.s.modern_ui = true;
        ui.s.store(&mut c);
        assert!(c.modern_ui && !c.modern_enabled());
    }

    /// A bundled mod's rows on our Extras tabs stand at their places; its other rows make its own page; a choice there
    /// goes to the app as the mod's change.
    #[test]
    fn mod_rows_on_our_tabs() {
        let mut ui = SettingsUi::open_kind(&Config::default(), [1280, 800], Kind::Extras);
        let row = |key: &str, tab: &str, place| ModRow {
            preview: None,
            key: key.into(),
            label: key.into(),
            help: String::new(),
            choices: vec![("OFF".into(), 0.into()), ("3 s".into(), 3.into())],
            at: 0,
            tab: tab.into(),
            place,
            place_modern: None,
        };
        ui.set_mod_pages(vec![ModPage {
            id: "m".into(),
            name: "M".into(),
            rows: vec![row("first", "Misc.", Some(0)), row("own", "", None), row("last", "Misc.", None)],
        }]);
        assert_eq!((ui.mod_pages.len(), ui.mod_pages[0].rows.len()), (1, 1));
        ui.tab = 3;
        let shown = ui.rows();
        assert_eq!((shown.first().unwrap().label.as_str(), shown.last().unwrap().label.as_str()), ("first", "last"));
        assert_eq!(shown.len(), rows(Kind::Extras, 3, false, false).len() + 2);
        ui.set(shown[0].val, 1);
        assert_eq!(ui.mod_changes, [("m".to_string(), "first".to_string(), serde_json::Value::from(3))]);
        assert_eq!(ui.get(shown[0].val), 1);
    }

    #[test]
    fn plain_help() {
        assert_eq!(plain("0034: full screen or a window."), "Full screen or a window.");
        assert_eq!(plain("0001 / 0002: the window's size."), "The window's size.");
        assert_eq!(plain("The monitor to use (not retail)."), "The monitor to use.");
        assert_eq!(plain("Window: the window's shape."), "Window: the window's shape.");
    }

    /// The Background row warns (help and modern label) only while real character shadows are on.
    #[test]
    fn background_warns_with_real_shadows() {
        let mut c = Config::default();
        for shadows in [0, 1] {
            c.actor_shadows = shadows;
            let mut ui = SettingsUi::open(&c, [1280, 800]);
            let (tab, i) = (0..TABS.len())
                .find_map(|t| {
                    ui.tab = t;
                    ui.rows().iter().position(|r| r.label == "Render Resolution").map(|i| (t, i))
                })
                .expect("a Render Resolution row");
            ui.goto(tab, i + 1);
            assert_eq!(ui.help().starts_with(WARNING), shadows != 0, "{}", ui.help());
            // The modern help drops the Config numbers behind the warning too.
            let help = ui.view().help;
            assert!(!help.contains("0003"), "{help}");
            assert_eq!(help.starts_with(WARNING), shadows != 0, "{help}");
            let label = &ui.view().rows[i].label;
            assert_eq!(label.contains('\u{26a0}'), shadows != 0, "{label}");
            // Other rows never warn.
            assert!(ui.rows().iter().filter(|r| r.label != "Render Resolution").all(|r| !ui.warns(r)));
        }
    }

    #[test]
    fn steps_and_store() {
        let mut c = Config { frame_limit: 300, ..Default::default() };
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        // Window mode (a slider of four): Windowed -> Borderless.
        ui.goto(0, 1);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.tool.get("0034"), Some(&2));
        // Render Resolution (under the Preset): Same as window -> 640 x 400.
        ui.goto(1, 2);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Escape);
        assert_eq!((ui.s.tool["0003"], ui.s.tool["0004"]), (640, 400));
        // Frame limit 300 -> 360 -> unlimited.
        ui.goto(0, 5);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.frame_limit, 0);
        assert!(ui.dirty);
        ui.s.store(&mut c);
        assert_eq!((c.window_mode(), c.background([1280, 800]), c.frame_limit), (2, [640, 400], 0));
        let back: Config = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert_eq!(Settings::of(&back), ui.s);
    }

    /// Extras: pages as Settings (Maps, Timers, Battle, Misc.), opened on the page name; buttons pick with Left / Right
    /// and Enter; Up from the page name wraps to the last row.
    #[test]
    fn extras_buttons() {
        // The classic pages (the modern UI, the default, leaves out the minimap's place and size).
        let c = Config { modern_ui: false, ..Default::default() };
        let mut ui = SettingsUi::open_kind(&c, [1280, 800], Kind::Extras);
        assert_eq!((ui.tab, ui.row), (0, 0));
        ui.key(egui::Key::ArrowLeft);
        assert_eq!(ui.tab, 3);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::ArrowDown);
        assert_eq!((ui.row, ui.col), (1, 1));
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.map_overlay && ui.dirty);
        // The last row, Minimap Size (Small / Medium / Large): Large; then its place, top left; then the minimap on.
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowUp);
        assert_eq!(ui.row, 5);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.minimap_size, 2);
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.minimap_dock, 1);
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.minimap);
        // Timers: Timers Position (Left / Right / Top): the right dock; then the timers on.
        ui.goto(1, 2);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert_eq!(ui.s.timers_dock, 1);
        ui.key(egui::Key::ArrowUp);
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.timers);
        // Misc.: Timestamps on All Lines.
        ui.goto(3, 2);
        ui.key(egui::Key::ArrowLeft);
        ui.key(egui::Key::Enter);
        assert!(ui.s.stamp_all_lines);
    }

    /// Settings: Left / Right turn the page on a slider row, move between buttons on a buttons row, and always turn
    /// it with the page modifier; the page wraps.
    #[test]
    fn pages_turn() {
        let c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        ui.goto(0, 1);
        ui.key(egui::Key::ArrowLeft);
        assert_eq!(ui.tab, MODS);
        ui.goto(0, 6);
        ui.key(egui::Key::ArrowRight);
        assert_eq!((ui.tab, ui.col), (0, 1));
        ui.key_mod(egui::Key::ArrowRight, true);
        assert_eq!((ui.tab, ui.row), (1, 6));
        // The page name: opened on it, Up from the first row reaches it, Left / Right there turn the page.
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        assert_eq!(ui.row, 0);
        ui.goto(2, 1);
        ui.key(egui::Key::ArrowUp);
        assert_eq!(ui.row, 0);
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::ArrowRight);
        assert_eq!(ui.tab, 4);
    }

    /// DAT Packs: ON / OFF mark a pack; Move carries it with Up / Down, Enter places it, Escape puts the order back.
    /// Lua mods are on the Mods page only, and the packs' moves step over them.
    #[test]
    fn mods_page() {
        use crate::config::ModRef;
        let c = Config::default();
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        let m = |n: &str, on: bool| ModRef { name: n.into(), enabled: on };
        ui.s.mods = vec![m("a", true), m("b", false), m("c", true)];
        ui.lua_names = std::collections::HashSet::new();
        ui.mods_ui = Some(super::super::modsui::Mods::empty());
        (ui.tab, ui.row, ui.col) = (PACKS, 1, 0);
        let names = |ui: &SettingsUi| ui.s.mods.iter().map(|m| m.name.clone()).collect::<Vec<_>>().join("");
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        assert!(!ui.s.mods[0].enabled && ui.dirty);
        ui.dirty = false;
        ui.key(egui::Key::ArrowRight);
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowDown);
        ui.key(egui::Key::Enter);
        assert_eq!((names(&ui), ui.row, ui.dirty), ("bac".into(), 2, true));
        ui.key(egui::Key::Enter);
        ui.key(egui::Key::ArrowDown);
        assert_eq!(names(&ui), "bca");
        ui.key(egui::Key::Escape);
        assert_eq!((names(&ui), ui.row), ("bac".into(), 2));
        // Down past the last pack wraps to the page name; Left / Right there turn the page.
        ui.key(egui::Key::ArrowDown);
        ui.key(egui::Key::ArrowDown);
        assert_eq!(ui.row, 0);
        ui.key(egui::Key::ArrowRight);
        assert_eq!(ui.tab, MODS);
        // "a" a Lua mod: the Mods page holds it alone, with ON / OFF; on DAT Packs, "b" moves down past it.
        ui.lua_names.insert("a".into());
        assert_eq!((ui.mods_rows().0, ui.mods_cols(1)), (vec![("a".to_string(), false)], 2));
        ui.goto(PACKS, 1);
        assert_eq!(ui.mods_rows().0.iter().map(|r| r.0.as_str()).collect::<String>(), "bc");
        assert!(ui.mod_move(0, false));
        assert_eq!(names(&ui), "cab");
    }

    /// The categories hold every row of the old tabs exactly once (Preset is new), in each place's own mode.
    #[test]
    fn categories_cover_every_old_row_once() {
        // Val's Debug of the 39 rows the General .. Gamepad tabs had (Panel Places only in game, the volumes only out of it).
        let old = [
            "Tool(\"0034\")",
            "Tool(\"0000\")",
            "Gamma",
            "Size(\"0001\", \"0002\", Window)",
            "Monitor",
            "Gpu",
            "Size(\"0003\", \"0004\", Background)",
            "UiScale",
            "Size(\"0037\", \"0038\", Ui)",
            "RetailAspect",
            "DrawDistance",
            "Tool(\"0018\")",
            "Tool(\"0019\")",
            "Tool(\"0036\")",
            "LampLights",
            "Tool(\"0011\")",
            "Tool(\"0017\")",
            "Tool(\"0040\")",
            "Tool(\"0030\")",
            "Tool(\"0007\")",
            "Tool(\"0029\")",
            "Tool(\"0035\")",
            "Music",
            "Effects",
            "Tool(\"0021\")",
            "CursorSize",
            "Tool(\"0022\")",
            "Tool(\"0023\")",
            "Tool(\"0043\")",
            "FrameLimit",
            "ReduceMotion",
            "Aniso",
            "SendRate",
            "PartyTp",
            "ModernUi",
            "ModernScale",
            "ModernOpacity",
            "ResetPlaces",
            "Gamepad",
            "GamepadDeadzone",
            "GamepadCamera",
            "KeysHideCursor",
            "MouseCamSpeed",
            "Antialias",
        ];
        let c = Config {
            server: "FFXI Forever".into(),
            named_servers: vec![crate::config::NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            ..Default::default()
        };
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        let mut seen: Vec<String> = Vec::new();
        for in_game in [true, false] {
            ui.in_game = in_game;
            let mut here = Vec::new();
            for tab in 0..TABS.len() {
                ui.tab = tab;
                here.extend(
                    ui.rows()
                        .iter()
                        .filter(|r| {
                            !matches!(
                                r.val,
                                Val::Preset
                                    | Val::Experience
                                    | Val::Controls
                                    | Val::ControlsAd
                                    | Val::Fxaa
                                    | Val::Bloom
                                    | Val::BloomIntensity
                                    | Val::BloomThreshold
                                    | Val::Tonemap
                                    | Val::Exposure
                                    | Val::Sharpen
                                    | Val::Saturation
                                    | Val::Ao
                                    | Val::AoStrength
                                    | Val::AoRadius
                                    | Val::AoLight
                                    | Val::Fog
                                    | Val::FogDensity
                                    | Val::FogHeight
                                    | Val::FogRamp
                                    | Val::FogLayers
                                    | Val::WaterEnhanced
                                    | Val::SkyClouds
                                    | Val::LightGrade
                                    | Val::LightGradeAmount
                                    | Val::SunShafts
                                    | Val::SunShaftsAmount
                                    | Val::VolumetricFog
                                    | Val::VolumetricFogAmount
                                    | Val::DetailTextures
                                    | Val::DetailStrength
                                    | Val::TextureSharpening
                                    | Val::ModernMinimap
                                    | Val::SoloBadge
                                    | Val::JobPanel
                                    | Val::ExpBar
                                    | Val::CharacterLight
                                    | Val::RotAdaptive
                                    | Val::KeepSorted
                                    | Val::Combine
                                    | Val::RotCure
                                    | Val::RotConvert
                                    | Val::RotRenew
                                    | Val::MinimapUnowned
                                    | Val::MinimapSize
                            )
                        })
                        .map(|r| format!("{:?}", r.val)),
                );
            }
            let mut dup = here.clone();
            dup.sort();
            dup.dedup();
            assert_eq!(dup.len(), here.len(), "a row is in two categories (in_game {in_game})");
            seen.extend(here);
        }
        seen.retain(|v| {
            !matches!(
                v.as_str(),
                "InvertHorizontalMouse"
                    | "ForeverLook"
                    | "Upscaler"
                    | "UpscaleQuality"
                    | "EnvironmentTextures"
                    | "WoodsTextures"
                    | "QuestTracker"
                    | "QuestIndicators"
                    | "AutoNavigation"
                    | "TrainingHud"
                    | "AutoRotationStart"
                    | "JournalKey"
                    | "AutoEquipUpgrades"
                    | "ConColors"
                    | "CompactHud"
                    | "AutoCheckTarget"
                    | "NavMesh"
                    | "RecipeLogKey"
                    | "ActorShadows"
                    | "ShadowMap"
                    | "ShadowRange"
                    | "SoftShadows"
                    | "FarView"
                    | "ActorShade"
                    | "ShadowCascades"
                    | "Ssao"
                    | "Grading"
            )
        });
        seen.sort();
        seen.dedup();
        let mut want: Vec<String> = old.iter().map(|s| s.to_string()).collect();
        if !cfg!(feature = "forever") {
            want.retain(|v| {
                !matches!(v.as_str(), "ModernUi" | "ModernScale" | "ModernOpacity" | "ResetPlaces" | "LampLights")
            });
        }
        want.sort();
        assert_eq!(seen, want);
        // The Preset leads Graphics and nothing else; Post-processing holds the post pass's options.
        ui.tab = GRAPHICS;
        assert!(matches!(ui.rows()[0].val, Val::Preset) && ui.rows()[1].label == "Render Resolution");
        ui.tab = POST_TAB;
        if cfg!(feature = "forever") {
            assert!(ui.rows().len() == 29 && matches!(ui.rows()[0].val, Val::Fxaa) && ui.note().is_none());
            // The Character Light row ends it (after the effects it offsets), shows the default, and a preset leaves it alone.
            assert!(
                ui.rows().last().is_some_and(|r| matches!(r.val, Val::CharacterLight) && r.label == "Character Light")
            );
            assert_eq!(ui.get(Val::CharacterLight), 75);
            ui.set(Val::CharacterLight, 40);
            assert_eq!(ui.s.graphics.character_light, 0.4);
            ui.set(Val::Preset, 2);
            assert_eq!(ui.get(Val::CharacterLight), 40);
        } else {
            assert_eq!(
                ui.rows().iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
                ["Anti-aliasing (FXAA)", "Sharpening"]
            );
        }
        ui.set(Val::Preset, 0);
        assert_eq!(ui.preset(), 0);
        assert_eq!(
            ui.tabs(),
            [
                "Display",
                "Graphics",
                "Lighting & Effects",
                "Post-processing",
                "Interface",
                "Sound",
                "Misc.",
                "Controls",
                "DAT Packs",
                "Mods"
            ]
        );
        // The Controls page: the keyboard and mouse scheme first (Modern the default), then A / D, then the gamepad rows.
        ui.tab = CONTROLS;
        let vals: Vec<String> = ui.rows().iter().map(|r| format!("{:?}", r.val)).collect();
        let mut expected =
            vec!["Controls", "ControlsAd", "Gamepad", "GamepadDeadzone", "GamepadCamera", "MouseCamSpeed"];
        if cfg!(feature = "forever") {
            expected.insert(2, "InvertHorizontalMouse");
        }
        assert_eq!(vals, expected);
        assert_eq!(ui.get(Val::Controls), 2);
        assert!(ui.rows()[0].help.contains("no jumping"));
    }

    /// Merge guard: the Config window's pages apply each change as it is made (not deferred, not the window), but a
    /// change that could strand the player still gets the timed Keep / Revert, and Revert puts the values back.
    #[test]
    fn inline_pages_keep_the_timed_revert() {
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        assert!(ui.deferred() && ui.windowed());
        ui.inline = true;
        assert!(!ui.deferred() && !ui.windowed() && !ui.inline_review());
        // An ordinary row: applied at once, nothing to review.
        let party = ui.get(Val::PartyTp);
        ui.set(Val::PartyTp, 1 - party);
        ui.inline_changed();
        assert!(!ui.inline_review() && ui.effective().party_tp == (party == 0));
        // The interface size is disruptive: reviewed, and Revert restores it.
        let before = ui.get(Val::ModernScale);
        ui.set(Val::ModernScale, if before == 125 { 100 } else { 125 });
        ui.inline_changed();
        assert!(ui.inline_review() && ui.review_seconds().is_some());
        ui.finish_review(false);
        assert!(!ui.inline_review());
        assert_eq!(ui.get(Val::ModernScale), before);
        assert!(ui.dirty, "the revert is stored");
        // Kept: stays.
        ui.set(Val::ModernScale, if before == 125 { 100 } else { 125 });
        ui.inline_changed();
        ui.finish_review(true);
        assert_ne!(ui.get(Val::ModernScale), before);
    }

    /// The Controls row's choices set `controls`; the A / D row `controls_ad_turn`; both reach the config.
    #[test]
    fn controls_rows_set_the_config() {
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        assert_eq!((ui.s.controls, ui.s.controls_ad_turn), (2, false));
        ui.tab = CONTROLS;
        let first = ui.rows()[0].clone();
        assert_eq!(ui.stops(&first), (3, 0));
        for (stop, want) in [(1, 1), (2, 0), (0, 2)] {
            ui.set_stop(&first, stop);
            assert_eq!(ui.s.controls, want);
        }
        ui.set(Val::ControlsAd, 1);
        let mut c = Config::default();
        ui.s.store(&mut c);
        assert_eq!((c.controls, c.controls_ad_turn), (2, true));
        // Defaults on this page brings the scheme back.
        ui.set(Val::Controls, 0);
        ui.reset_category();
        assert_eq!((ui.s.controls, ui.s.controls_ad_turn), (2, false));
    }

    /// The Experience row carries the controls: Modern is the Modern scheme, Original the Original one; a hand change
    /// is Custom.
    #[cfg(feature = "forever")]
    #[test]
    fn experience_sets_the_controls() {
        let c = Config {
            server: "FFXI Forever".into(),
            named_servers: vec![crate::config::NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            ..Default::default()
        };
        let mut ui = SettingsUi::open(&c, [1280, 800]);
        assert_eq!(ui.experience(), 0);
        ui.apply_experience(1);
        assert_eq!((ui.s.controls, ui.experience()), (0, 1));
        ui.set(Val::Controls, 1);
        assert_eq!(ui.experience(), win::EXPERIENCES as u32);
        ui.apply_experience(0);
        assert_eq!((ui.s.controls, ui.experience()), (2, 0));
        // The Original controls alone with the Modern interface are neither.
        ui.set(Val::Controls, 0);
        assert_eq!(ui.experience(), win::EXPERIENCES as u32);
    }

    #[cfg(feature = "forever")]
    fn forever_config(controls: u32) -> Config {
        Config {
            server: "FFXI Forever".into(),
            named_servers: vec![crate::config::NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            controls,
            ..Default::default()
        }
    }

    /// Forever's scheme is a fourth choice on Forever only; on a regular server or the classic-only client the Controls
    /// row keeps its three.
    #[cfg(feature = "forever")]
    #[test]
    fn hybrid_choice_hidden_for_classic() {
        let stops = |c: &Config| {
            let mut ui = SettingsUi::open(c, [1280, 800]);
            ui.tab = CONTROLS;
            let first = ui.rows()[0].clone();
            assert_eq!(first.label, "Keyboard & Mouse");
            (ui.stops(&first), first.choices.iter().map(|c| c.0).collect::<Vec<_>>())
        };
        assert_eq!(stops(&forever_config(2)), ((4, 0), vec![2, 1, 0, 3]));
        assert_eq!(stops(&forever_config(3)), ((4, 3), vec![2, 1, 0, 3]));
        assert_eq!(stops(&Config::default()).1, vec![2, 1, 0]);
        let classic = Config { classic_only: true, ..forever_config(3) };
        assert_eq!(stops(&classic).1, vec![2, 1, 0]);
        // The modern layer's lookup (the unified Settings window) offers the same choices (QA finding: it read the
        // unfiltered table and showed "Forever" on a regular server and under classic_only).
        let offered = |c: &Config| match SettingsUi::open(c, [1280, 800]).item("Keyboard & Mouse") {
            Some((VRow { ctl: VCtl::Buttons(b), .. }, _)) => b.len(),
            _ => panic!("Keyboard & Mouse is a button row"),
        };
        assert_eq!((offered(&forever_config(2)), offered(&Config::default()), offered(&classic)), (4, 3, 3));
        // Choosing it reaches the config; Original and back again too.
        let mut ui = SettingsUi::open(&forever_config(2), [1280, 800]);
        ui.tab = CONTROLS;
        let first = ui.rows()[0].clone();
        ui.set_stop(&first, 3);
        let mut c = forever_config(2);
        ui.s.store(&mut c);
        assert_eq!(c.controls, 3);
    }

    /// The saved choice outlives a visit to a regular server: it plays as Modern there and is not rewritten.
    #[cfg(feature = "forever")]
    #[test]
    fn hybrid_config_downgrades_without_forever() {
        use crate::client::controls::Scheme;
        assert_eq!(forever_config(3).control_scheme(), Scheme::Hybrid);
        assert_eq!(forever_config(2).control_scheme(), Scheme::Modern);
        assert_eq!(Config { controls: 3, ..Default::default() }.control_scheme(), Scheme::Modern);
        assert_eq!(Config { classic_only: true, ..forever_config(3) }.control_scheme(), Scheme::Modern);
        // Settings opened and applied off Forever keep the saved 3 (other rows change, this one is untouched) ...
        let regular = Config { controls: 3, ..Default::default() };
        let mut ui = SettingsUi::open(&regular, [1280, 800]);
        assert_eq!(ui.s.controls, 3);
        ui.set(Val::ControlsAd, 1);
        let mut c = regular.clone();
        ui.s.store(&mut c);
        assert_eq!((c.controls, c.controls_ad_turn), (3, true));
        // ... unless a visible choice is picked there.
        ui.set(Val::Controls, 1);
        ui.s.store(&mut c);
        assert_eq!(c.controls, 1);
        // A bad number from a file stays in range.
        assert_eq!(SettingsUi::open(&Config { controls: 40, ..Default::default() }, [1280, 800]).s.controls, 3);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn experience_accepts_hybrid() {
        let mut ui = SettingsUi::open(&forever_config(3), [1280, 800]);
        assert_eq!(ui.experience(), 0);
        // Picking Modern on the Experience row keeps Forever's scheme when it is the choice, and sets Modern's
        // otherwise.
        ui.apply_experience(0);
        assert_eq!((ui.s.controls, ui.experience()), (3, 0));
        ui.apply_experience(1);
        assert_eq!((ui.s.controls, ui.experience()), (0, 1));
        ui.apply_experience(0);
        assert_eq!((ui.s.controls, ui.experience()), (2, 0));
    }

    /// A right-click tap may back out of Settings only when Cancel would lose nothing.
    #[test]
    fn settings_back_out_needs_nothing_unsaved() {
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        assert!(ui.can_back_out());
        ui.set(Val::ControlsAd, 1);
        assert!(!ui.can_back_out());
        ui.cancel();
        assert!(ui.can_back_out());
        ui.set(Val::ControlsAd, 1);
        ui.commit();
        assert!(ui.can_back_out());
    }

    /// The rows a preset sets.
    fn graphics(s: &Settings) -> (u16, u32, bool, Vec<u32>) {
        let t = |n: &str| {
            s.tool
                .get(n)
                .copied()
                .unwrap_or_else(|| crate::config::TOOL_DEFAULTS.iter().find(|d| d.0 == n).map_or(0, |d| d.1))
        };
        (
            s.anisotropy,
            s.draw_distance,
            s.lamp_lights,
            ["0018", "0000", "0017", "0011", "0003", "0004"].iter().map(|n| t(n)).collect(),
        )
    }

    /// Original is the defaults, a hand change makes Custom, High and Ultra are found again.
    #[test]
    fn presets_are_detected() {
        let mut ui = SettingsUi::open(&Config::default(), [1920, 1080]);
        assert_eq!(ui.preset(), 0);
        ui.s.anisotropy = 4;
        assert_eq!(ui.preset(), 3);
        assert_eq!(ui.shown(&Row::from(&rows(Kind::Settings, GRAPHICS, true, false)[0])), "Custom");
        ui.set(Val::Preset, 1);
        assert_eq!(
            (ui.preset(), ui.s.anisotropy, ui.s.draw_distance, ui.s.lamp_lights, ui.s.graphics.fxaa),
            (1, 16, 0, false, true)
        );
        assert_eq!(ui.tool_get("0018"), 2);
        ui.set(Val::Preset, 2);
        assert_eq!(
            (ui.preset(), ui.s.draw_distance, [ui.tool_get("0003"), ui.tool_get("0004")], ui.s.graphics.tonemap),
            (2, 0, [2880, 1620], false)
        );
        // Custom is not a preset to choose.
        ui.set(Val::Preset, 3);
        assert_eq!(ui.preset(), 2);
        ui.set(Val::Preset, 0);
        assert_eq!(ui.preset(), 0);
        assert_eq!(ui.s, Settings::of(&Config::default()));
        // A row by hand off High.
        ui.set(Val::Preset, 1);
        ui.tool_set("0017", 0);
        assert_eq!(ui.preset(), 3);
    }

    /// A preset changes the graphics rows and nothing else, whatever the other settings are.
    #[test]
    fn preset_leaves_the_rest_alone() {
        let mut c = Config::default();
        c.frame_limit = 144;
        c.monitor = "DP-1".into();
        c.gpu = "Some GPU".into();
        c.modern_scale = 110;
        c.tool.insert("0034".into(), 2);
        c.tool.insert("0001".into(), 2560);
        c.tool.insert("0002".into(), 1440);
        c.tool.insert("0028".into(), 0.5f32.to_bits());
        c.tool.insert("0019".into(), 0);
        c.tool.insert("0036".into(), 2);
        let mut ui = SettingsUi::open(&c, [2560, 1440]);
        for k in [1, 2, 0] {
            let before = ui.s.clone();
            ui.set(Val::Preset, k);
            let after = ui.s.clone();
            // Put the graphics rows back: what is left must be what it was.
            let mut rest = after.clone();
            rest.anisotropy = before.anisotropy;
            rest.draw_distance = before.draw_distance;
            rest.lamp_lights = before.lamp_lights;
            (rest.graphics.fxaa, rest.graphics.tonemap, rest.graphics.sharpen) =
                (before.graphics.fxaa, before.graphics.tonemap, before.graphics.sharpen);
            (rest.graphics.ao, rest.graphics.fog) = (before.graphics.ao, before.graphics.fog);
            for n in ["0018", "0000", "0017", "0011", "0003", "0004"] {
                match before.tool.get(n) {
                    Some(v) => rest.tool.insert(n.into(), *v),
                    None => rest.tool.remove(n),
                };
            }
            assert_eq!(rest, before, "preset {k} touched a row outside the graphics rows");
        }
        // Ultra at 1440p: the scene at 1.5x the window.
        ui.set(Val::Preset, 2);
        assert_eq!(graphics(&ui.s).3[4..], [3840, 2160]);
    }

    /// Defaults on a category puts its rows back, on Graphics to Original, and leaves the other categories alone.
    #[test]
    fn defaults_reset_the_category() {
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        ui.set(Val::Preset, 2);
        ui.tool_set("0019", 0);
        ui.s.frame_limit = 60;
        ui.tab = GRAPHICS;
        ui.reset_category();
        assert_eq!(ui.preset(), 0);
        assert_eq!(ui.tool_get("0019"), 1);
        // Frame Limit is on Display: not touched.
        assert_eq!(ui.s.frame_limit, 60);
        assert!(ui.dirty);
    }

    /// The window's keys: categories, rows (Left / Right set at once, live), the buttons; Cancel puts the values back.
    #[test]
    fn window_keys_and_cancel() {
        use egui::Key as K;
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        ui.begin();
        ui.open_page("graphics");
        assert_eq!((ui.tab, ui.row, ui.zone), (GRAPHICS, 1, Zone::Rows));
        // Preset: Right is High, at once; Down to Render Resolution; Up and Up reach the categories.
        assert_eq!(ui.win_key(K::ArrowRight, false), Done::Stay);
        assert_eq!(ui.preset(), 1);
        assert!(ui.dirty);
        ui.win_key(K::ArrowDown, false);
        assert_eq!(ui.row, 2);
        ui.win_key(K::ArrowUp, false);
        ui.win_key(K::ArrowUp, false);
        assert_eq!(ui.zone, Zone::Cats);
        // Down on the list turns the category, Tab and Page Down too; Enter goes into the rows, Down past the last one to the buttons.
        ui.win_key(K::ArrowDown, false);
        assert_eq!(ui.tab, 2);
        ui.win_key(K::PageDown, false);
        ui.win_key(K::Tab, true);
        assert_eq!(ui.tab, 2);
        ui.win_key(K::Enter, false);
        assert_eq!(ui.zone, Zone::Rows);
        for _ in 0..ui.rows().len() {
            ui.win_key(K::ArrowDown, false);
        }
        assert_eq!(ui.zone, Zone::Buttons);
        assert_eq!(ui.btn, 2);
        // Enter on a toggle steps it: Graphics Stabilization in Lighting & Effects.
        ui.zone = Zone::Rows;
        ui.row = ui.rows().iter().position(|r| matches!(r.val, Val::Tool("0040"))).unwrap() + 1;
        ui.win_key(K::Enter, false);
        assert_eq!(ui.tool_get("0040"), 1);
        // Escape goes back to the categories, a second one cancels: the values are the open ones again.
        assert_eq!(ui.win_key(K::Escape, false), Done::Stay);
        assert_eq!(ui.zone, Zone::Cats);
        assert_eq!(ui.win_key(K::Escape, false), Done::Cancel);
        assert!(ui.closing && ui.dirty);
        assert_eq!(ui.s, Settings::of(&Config::default()));
        // Apply keeps the values as the new ones; Cancel then goes back to them; OK closes keeping them.
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        ui.begin();
        ui.set(Val::Preset, 1);
        assert_eq!(ui.win_press(1), Done::Stay);
        ui.set(Val::Preset, 2);
        assert_eq!(ui.win_press(3), Done::Cancel);
        assert_eq!(ui.preset(), 1);
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        ui.begin();
        ui.set(Val::Preset, 2);
        assert_eq!(ui.win_press(2), Done::Ok);
        assert_eq!(ui.preset(), 2);
    }

    /// The window's rows: a switch for ON / OFF, segments for a few names (Custom shows but is locked), sliders.
    #[test]
    fn window_row_controls() {
        let mut ui = SettingsUi::open(&Config::default(), [1280, 800]);
        ui.tab = GRAPHICS;
        let rows = ui.win_rows();
        assert!(matches!(&rows[0].ctl, WCtl::Segments { labels, at: 0, locked: Some(3) } if labels.len() == 4));
        assert!(matches!(&rows[1].ctl, WCtl::Slider { .. }));
        assert!(matches!(&rows[4].ctl, WCtl::Toggle(true)));
        assert_eq!(rows[3].when, "Applies on next zone");
        assert_eq!(rows[7].when, "Needs restart");
        assert!(rows[1].help.starts_with("The 3D scene's resolution"), "{}", rows[1].help);
    }
}
