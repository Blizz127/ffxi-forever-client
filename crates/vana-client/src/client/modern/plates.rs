//! The modern name plates: the name in Inter Bold Italic with a dark outline, in retail's name colour and the target's
//! pulse, at retail's place and scale, depth-tested in the same UI pass so walls still hide them; retail's badges left
//! of the name and the master stars above it. No bars over heads (user, 2026-10-26). Each name is rasterized once per
//! size bucket and uploaded as a UI image; a frame only draws quads.

use super::super::game_ui::GameUi;
use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Raster heights (em, px): the smallest at or above the drawn size is used, so a plate is never minified much.
const BUCKETS: [f32; 4] = [16.0, 28.0, 48.0, 80.0];
/// The drawn em in retail plate units (times the plate's y scale): capitals as tall as retail's letters (about 10.6
/// units); the baseline below the anchor so the text's middle sits where retail's does.
const EM: f32 = 15.5;
const BASELINE: f32 = 8.8;
/// Extra space between letters, in em: retail's plates are widely set.
const TRACKING: f32 = 0.08;

/// A rasterized name: its image size, the text's left, right and baseline in it.
#[derive(Clone, Copy)]
struct Raster {
    w: u32,
    h: u32,
    left: f32,
    right: f32,
    baseline: f32,
}

fn metrics() -> &'static Mutex<HashMap<String, Raster>> {
    static M: OnceLock<Mutex<HashMap<String, Raster>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// White text with a soft dark outline (straight alpha), padded for the italic overhang and the outline.
fn rasterize(text: &str, px: f32) -> Option<(Raster, Vec<u8>)> {
    let font = FontRef::try_from_slice(super::theme::font("Inter-BoldItalic")?).ok()?;
    let scale = PxScale::from(px);
    let sf = font.as_scaled(scale);
    let r = (px / 10.0).max(1.5);
    let pad = (r + px * 0.25).ceil();
    let mut caret = 0.0f32;
    let mut prev = None;
    let mut glyphs = Vec::new();
    for c in text.chars() {
        let id = sf.glyph_id(c);
        if let Some(p) = prev {
            caret += sf.kern(p, id);
        }
        glyphs.push(id.with_scale_and_position(scale, point(pad + caret, pad + sf.ascent())));
        caret += sf.h_advance(id) + TRACKING * px;
        prev = Some(id);
    }
    let (w, h) = ((caret + 2.0 * pad).ceil() as usize, (sf.ascent() - sf.descent() + 2.0 * pad).ceil() as usize);
    let mut cov = vec![0f32; w * h];
    for g in glyphs {
        let Some(o) = font.outline_glyph(g) else { continue };
        let b = o.px_bounds();
        o.draw(|x, y, c| {
            let (px, py) = (b.min.x as i32 + x as i32, b.min.y as i32 + y as i32);
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                let k = py as usize * w + px as usize;
                cov[k] = (cov[k] + c).min(1.0);
            }
        });
    }
    // The outline: coverage spread over a disc of radius r, fading over its last pixel.
    let ri = r.ceil() as i32;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let mut o = 0f32;
            for dy in -ri..=ri {
                for dx in -ri..=ri {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                        continue;
                    }
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    let fall = (r + 1.0 - d).clamp(0.0, 1.0);
                    o = o.max(cov[sy as usize * w + sx as usize] * fall);
                }
            }
            let f = cov[y as usize * w + x as usize];
            let o = o * 0.85;
            let a = f + o * (1.0 - f);
            let k = (y as usize * w + x as usize) * 4;
            if a > 0.0 {
                // White over a near-black outline.
                let c = (255.0 * f + 12.0 * o * (1.0 - f)) / a;
                out[k..k + 4].copy_from_slice(&[
                    c as u8,
                    c as u8,
                    (c + 2.0 * o * (1.0 - f)).min(255.0) as u8,
                    (a * 255.0) as u8,
                ]);
            }
        }
    }
    Some((
        Raster { w: w as u32, h: h as u32, left: pad, right: pad + caret - TRACKING * px, baseline: pad + sf.ascent() },
        out,
    ))
}

/// A plate: the name centred on x with its baseline BASELINE units below y, tinted (ARGB, 0x80 = 1.0) and pulsed;
/// retail's badges just left of it and its master stars above, darkened with the name by the screen fade `shade`
/// (`tint` and `icon` come faded already).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    gu: &mut GameUi,
    prefix: &[u8],
    name: &str,
    x: f32,
    y: f32,
    scale: [f32; 2],
    tint: u32,
    pulse: f32,
    icon: u32,
    stars: bool,
    shade: [f32; 3],
) {
    if name.is_empty() {
        return;
    }
    let em = EM * scale[1];
    let px = BUCKETS.iter().copied().find(|&b| b >= em).unwrap_or(BUCKETS[3]);
    let key = format!("plate {px} {name}");
    let raster = {
        let mut m = metrics().lock().unwrap();
        match m.get(&key).copied().filter(|_| gu.has_image(&key)) {
            Some(r) => r,
            None => {
                let Some((r, rgba)) = rasterize(name, px) else { return };
                gu.image(&key, r.w, r.h, rgba);
                m.insert(key.clone(), r);
                r
            }
        }
    };
    let k = em / px;
    let text_w = (raster.right - raster.left) * k;
    let left = x - text_w / 2.0 - raster.left * k;
    let top = y + BASELINE * scale[1] - raster.baseline * k;
    let c = |sh: u32| ((tint >> sh & 0xff) as f32 / 128.0).min(1.0);
    gu.image_rect(
        &key,
        left.round(),
        top.round(),
        raster.w as f32 * k,
        raster.h as f32 * k,
        [c(16), c(8), c(0), pulse.clamp(0.0, 1.0)],
    );
    // Retail's badges (GM, new adventurer, linkshell ...) end just left of the name; the stars stay above.
    // The badges right to left from the name: the first at the text's height, those after it smaller; the job master
    // stars above the name.
    let size = EM * 0.95 * scale[1];
    let mut bx = x - text_w / 2.0 - 4.0 * scale[0];
    let cy = top + raster.baseline * k - 0.36 * EM * scale[1];
    for (i, b) in badges(prefix).into_iter().enumerate() {
        let Some(key) = badge(gu, b) else { continue };
        let d = if i == 0 { size } else { size * 0.75 };
        bx -= d;
        // The pearl takes its linkshell's colour (retail's icon tint for 0x92).
        let c = if b == 0x92 {
            [16, 8, 0].map(|sh| ((icon >> sh & 0xff) as f32 / 128.0).min(1.0))
        } else {
            shade.map(|k| k.min(1.0))
        };
        gu.image_rect(&key, bx.round(), (cy - d / 2.0).round(), d, d, [c[0], c[1], c[2], pulse.clamp(0.0, 1.0)]);
        bx -= 1.0 * scale[0];
    }
    if stars && let Some(key) = badge(gu, 0xad) {
        let d = size * 0.6;
        for (dx, big) in [(-1.0, false), (0.0, true), (1.0, false)] {
            let s = if big { d * 1.25 } else { d };
            let [r, g, b] = shade.map(|k| k.min(1.0));
            gu.image_rect(
                &key,
                x + dx * d * 1.1 - s / 2.0,
                top - s + 2.0 * scale[1],
                s,
                s,
                [r, g, b, pulse.clamp(0.0, 1.0)],
            );
        }
    }
}

/// The modern badges by retail's glyph byte (assets/modern/badges, from the VectorCraft set the user approved).
const BADGES: [(u8, &[u8]); 32] = [
    (0x8e, include_bytes!("../../../assets/modern/badges/badge_8e.png")),
    (0x8f, include_bytes!("../../../assets/modern/badges/badge_8f.png")),
    (0x90, include_bytes!("../../../assets/modern/badges/badge_90.png")),
    (0x91, include_bytes!("../../../assets/modern/badges/badge_91.png")),
    (0x92, include_bytes!("../../../assets/modern/badges/badge_92.png")),
    (0x93, include_bytes!("../../../assets/modern/badges/badge_93.png")),
    (0x94, include_bytes!("../../../assets/modern/badges/badge_94.png")),
    (0x95, include_bytes!("../../../assets/modern/badges/badge_95.png")),
    (0x96, include_bytes!("../../../assets/modern/badges/badge_96.png")),
    (0x99, include_bytes!("../../../assets/modern/badges/badge_99.png")),
    (0x9c, include_bytes!("../../../assets/modern/badges/badge_9c.png")),
    (0x9d, include_bytes!("../../../assets/modern/badges/badge_9d.png")),
    (0x9e, include_bytes!("../../../assets/modern/badges/badge_9e.png")),
    (0x9f, include_bytes!("../../../assets/modern/badges/badge_9f.png")),
    (0xa0, include_bytes!("../../../assets/modern/badges/badge_a0.png")),
    (0xa1, include_bytes!("../../../assets/modern/badges/badge_a1.png")),
    (0xa2, include_bytes!("../../../assets/modern/badges/badge_a2.png")),
    (0xa3, include_bytes!("../../../assets/modern/badges/badge_a3.png")),
    (0xa4, include_bytes!("../../../assets/modern/badges/badge_a4.png")),
    (0xa5, include_bytes!("../../../assets/modern/badges/badge_a5.png")),
    (0xa6, include_bytes!("../../../assets/modern/badges/badge_a6.png")),
    (0xa7, include_bytes!("../../../assets/modern/badges/badge_a7.png")),
    (0xa8, include_bytes!("../../../assets/modern/badges/badge_a8.png")),
    (0xa9, include_bytes!("../../../assets/modern/badges/badge_a9.png")),
    (0xaa, include_bytes!("../../../assets/modern/badges/badge_aa.png")),
    (0xab, include_bytes!("../../../assets/modern/badges/badge_ab.png")),
    (0xac, include_bytes!("../../../assets/modern/badges/badge_ac.png")),
    (0xad, include_bytes!("../../../assets/modern/badges/badge_ad.png")),
    (0xae, include_bytes!("../../../assets/modern/badges/badge_ae.png")),
    (0xaf, include_bytes!("../../../assets/modern/badges/badge_af.png")),
    (0xb0, include_bytes!("../../../assets/modern/badges/badge_b0.png")),
    (0xb1, include_bytes!("../../../assets/modern/badges/badge_b1.png")),
];

/// The prefix's badges in order: rank bytes 0xc8..0xcd as the sword and shield with their 0..2 stars (FUN_10086210),
/// the GM levels' shared triangle, the recruiter's star already in its orb; letters left out.
fn badges(prefix: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &c in prefix {
        match c {
            0xc8..=0xcd => {
                out.push(0xa9);
                out.extend((0..(c - 0xc8) % 3).map(|_| 0xaa));
            }
            0x96..=0x98 => out.push(0x96),
            0x8e..=0xb1 => out.push(c),
            _ => {}
        }
    }
    out
}

/// A badge's image in the UI pass, uploaded once; None for a byte without one.
fn badge(gu: &mut GameUi, b: u8) -> Option<String> {
    let key = format!("badge {b:02x}");
    if gu.has_image(&key) {
        return Some(key);
    }
    let png = BADGES.iter().find(|(k, _)| *k == b)?.1;
    let mut d = png::Decoder::new(std::io::Cursor::new(png));
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0u8; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    gu.image(&key, info.width, info.height, buf);
    Some(key)
}

/// Original vector emblem: an open parchment book with a small blue crystal seal.
/// Shared by the depth-tested world marker and offline UI preview; no borrowed game artwork.
pub(super) fn book_faces() -> Vec<([[f32; 2]; 4], [f32; 4])> {
    let gold = [0.65, 0.40, 0.10, 1.0];
    let paper = [1.0, 0.91, 0.65, 1.0];
    let mut v = vec![
        ([[-0.55, -0.42], [0.0, -0.25], [-0.55, 0.32], [0.0, 0.50]], gold),
        ([[0.0, -0.25], [0.55, -0.42], [0.0, 0.50], [0.55, 0.32]], gold),
        ([[-0.47, -0.32], [-0.04, -0.19], [-0.47, 0.25], [-0.04, 0.39]], paper),
        ([[0.04, -0.19], [0.47, -0.32], [0.04, 0.39], [0.47, 0.25]], paper),
    ];
    for y in [-0.16, 0.0, 0.16] {
        v.push(([[-0.39, y - 0.04], [-0.13, y + 0.04], [-0.39, y], [-0.13, y + 0.08]], gold));
        v.push(([[0.13, y + 0.04], [0.39, y - 0.04], [0.13, y + 0.08], [0.39, y]], gold));
    }
    v.push(([[0.0, -0.65], [0.16, -0.40], [-0.16, -0.40], [0.0, -0.15]], [0.12, 0.78, 1.0, 1.0]));
    v
}
use super::super::qtrack::{Giver, Indicator, MissionMark, MissionState, Nation, RowIcon};
fn indicator_faces(kind: Indicator) -> Vec<Face> {
    if kind == Indicator::Available {
        return book_faces();
    }
    let edge = [0.65, 0.40, 0.10, 1.0];
    let paper = [1.0, 0.91, 0.65, 1.0];
    let seal = [0.1, 0.85, 0.55, 1.0];
    vec![
        ([[-0.34, -0.57], [0.34, -0.57], [-0.34, 0.45], [0.34, 0.45]], edge),
        ([[-0.25, -0.48], [0.25, -0.48], [-0.25, 0.34], [0.25, 0.34]], paper),
        ([[-0.45, -0.59], [0.45, -0.59], [-0.45, -0.40], [0.45, -0.40]], paper),
        ([[-0.45, 0.31], [0.45, 0.31], [-0.45, 0.50], [0.45, 0.50]], paper),
        ([[-0.12, 0.03], [0.0, 0.05], [-0.22, 0.58], [-0.08, 0.49]], edge),
        ([[0.0, 0.05], [0.12, 0.03], [0.08, 0.49], [0.22, 0.58]], edge),
        ([[0.0, -0.23], [0.28, 0.04], [-0.28, 0.04], [0.0, 0.31]], seal),
        ([[-0.16, 0.01], [-0.09, -0.04], [-0.06, 0.12], [0.0, 0.18]], paper),
        ([[-0.06, 0.12], [0.0, 0.18], [0.14, -0.12], [0.20, -0.06]], paper),
    ]
}
type Face = ([[f32; 2]; 4], [f32; 4]);
/// A convex polygon from its corners in drawing order (top-left, top-right, bottom-right, bottom-left), stored in the
/// strip order the batch takes (p0, p1, p3, p2).
fn poly(tl: [f32; 2], tr: [f32; 2], br: [f32; 2], bl: [f32; 2], color: [f32; 4]) -> Face {
    ([tl, tr, bl, br], color)
}
/// Flat-top hexagon of half-width `r`, in two quads.
fn hexagon(r: f32, color: [f32; 4], out: &mut Vec<Face>) {
    let (w, h) = (r, r * 0.9);
    out.push(poly([-w * 0.5, -h], [w * 0.5, -h], [w, 0.0], [-w, 0.0], color));
    out.push(poly([-w, 0.0], [w, 0.0], [w * 0.5, h], [-w * 0.5, h], color));
}
/// A four-pointed star (two crossed diamonds) with arm length `r` and half-width `w`.
fn star(r: f32, w: f32, color: [f32; 4], out: &mut Vec<Face>) {
    out.push(poly([0.0, -r], [w, 0.0], [0.0, r], [-w, 0.0], color));
    out.push(poly([-r, 0.0], [0.0, -w], [r, 0.0], [0.0, w], color));
}
/// Heater shield: a rectangle over a tapering point, grown by `g` for the dark outline.
fn shield(g: f32, color: [f32; 4], out: &mut Vec<Face>) {
    let (hw, top, mid, tip) = (0.38 + g, -0.52 - g, 0.04, 0.58 + g * 1.4);
    out.push(poly([-hw, top], [hw, top], [hw, mid], [-hw, mid], color));
    out.push(poly([-hw, mid], [hw, mid], [0.07, tip], [-0.07, tip], color));
}
fn grey(c: [f32; 4]) -> [f32; 4] {
    let l = c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
    let mix = |v: f32| ((v + (l - v) * 0.88) * 0.78).clamp(0.0, 1.0);
    [mix(c[0]), mix(c[1]), mix(c[2]), c[3]]
}
/// Original vector nation emblems for missions (no game artwork): each nation has its own silhouette so the
/// grey step state still tells them apart. San d'Oria: a red heater shield with a gold cross. Bastok: a blue
/// hex nut ringed in silver. Windurst: a green four-pointed star with a gold heart.
/// Start: full colour, bobbing, glowing halo. Step: desaturated, smaller, inside a ring (a "go here" target).
/// `phase` is the animation clock in turns (0..1); the shots use a fixed phase.
pub(super) fn emblem_faces(mark: MissionMark, phase: f32) -> Vec<Face> {
    emblem_with(mark, phase, true)
}
/// `halo`: the Start glow rings behind the emblem (the tracker rows draw it without).
fn emblem_with(mark: MissionMark, phase: f32, halo_on: bool) -> Vec<Face> {
    let (turns, bright) = ((phase * std::f32::consts::TAU).sin(), mark.state == MissionState::Start);
    let ink = [0.06, 0.05, 0.08, 1.0];
    let mut f: Vec<Face> = Vec::new();
    match mark.nation {
        Nation::SanDoria => {
            let (red, gold) = ([0.82, 0.10, 0.12, 1.0], [1.0, 0.80, 0.25, 1.0]);
            shield(0.07, ink, &mut f);
            shield(0.0, red, &mut f);
            f.push(poly([-0.07, -0.44], [0.07, -0.44], [0.07, 0.44], [-0.07, 0.44], gold));
            f.push(poly([-0.30, -0.27], [0.30, -0.27], [0.30, -0.13], [-0.30, -0.13], gold));
        }
        Nation::Bastok => {
            let (blue, silver) = ([0.12, 0.32, 0.82, 1.0], [0.86, 0.89, 0.96, 1.0]);
            hexagon(0.68, ink, &mut f);
            hexagon(0.58, blue, &mut f);
            hexagon(0.38, silver, &mut f);
            hexagon(0.17, blue, &mut f);
        }
        Nation::Windurst => {
            let (green, gold) = ([0.12, 0.64, 0.30, 1.0], [1.0, 0.82, 0.30, 1.0]);
            star(0.70, 0.30, ink, &mut f);
            star(0.60, 0.22, green, &mut f);
            star(0.20, 0.20, gold, &mut f);
        }
    }
    let dy;
    if bright {
        // Glow: two translucent halo rings behind the emblem, pulsing; the emblem itself brightens a little.
        dy = -0.05 * turns;
        let a = 0.20 + 0.12 * turns;
        let lift = 1.0 + 0.10 * turns;
        let mut halo = Vec::new();
        hexagon(0.76, [1.0, 0.86, 0.45, a * 0.6], &mut halo);
        hexagon(0.70, [1.0, 0.92, 0.60, a], &mut halo);
        for (_, c) in f.iter_mut() {
            *c = [(c[0] * lift).min(1.0), (c[1] * lift).min(1.0), (c[2] * lift).min(1.0), c[3]];
        }
        if halo_on {
            halo.append(&mut f);
            f = halo;
        }
    } else {
        dy = 0.0;
        let s = 0.80;
        for (p, c) in f.iter_mut() {
            *p = p.map(|v| [v[0] * s, v[1] * s]);
            *c = grey(*c);
        }
        // A ring of twelve segments around the dimmed emblem.
        let (inner, outer, ring) = (0.69, 0.78, [0.78, 0.80, 0.84, 1.0]);
        let at = |k: usize, r: f32| {
            let t = k as f32 / 12.0 * std::f32::consts::TAU;
            [t.sin() * r, -t.cos() * r]
        };
        for k in 0..12 {
            f.push(poly(at(k, outer), at(k + 1, outer), at(k + 1, inner), at(k, inner), ring));
        }
    }
    for (p, _) in f.iter_mut() {
        *p = p.map(|v| [v[0], v[1] + dy]);
    }
    f
}
/// One icon over an NPC: the quest book, the ready scroll or a nation's mission emblem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GiverIcon {
    Quest(Indicator),
    Mission(MissionMark),
}
impl GiverIcon {
    fn faces(self, phase: f32) -> Vec<Face> {
        match self {
            GiverIcon::Quest(kind) => indicator_faces(kind),
            GiverIcon::Mission(mark) => emblem_faces(mark, phase),
        }
    }
}
/// The icons for one NPC in draw order: book, scroll, then mission emblems.
pub fn giver_icons(g: &Giver) -> Vec<GiverIcon> {
    let mut v = Vec::new();
    if g.available {
        v.push(GiverIcon::Quest(Indicator::Available));
    }
    if g.turn_in {
        v.push(GiverIcon::Quest(Indicator::TurnIn));
    }
    v.extend(g.missions.iter().copied().map(GiverIcon::Mission));
    v
}
/// A centred row of icons `pitch` apart over (x, y). Only Start emblems bob/glow with `phase`.
pub fn giver_row(gu: &mut GameUi, g: &Giver, x: f32, y: f32, size: f32, pitch: f32, shade: [f32; 3], phase: f32) {
    let icons = giver_icons(g);
    let left = x - (icons.len().saturating_sub(1)) as f32 * pitch * 0.5;
    for (i, icon) in icons.into_iter().enumerate() {
        let x = left + i as f32 * pitch;
        for (corners, color) in icon.faces(phase) {
            let xy = corners.map(|p| [x + p[0] * size, y + p[1] * size]);
            let c = [color[0] * shade[0], color[1] * shade[1], color[2] * shade[2], color[3]];
            gu.quad(xy, [c; 4]);
        }
    }
}
/// A small original quill (blue feather, gold shaft, dark nib) for "in progress", nib down-left, drawn at `scale`
/// about (cx, cy). Readable at 16 px: a dark outline under every piece.
fn quill_at(cx: f32, cy: f32, scale: f32, out: &mut Vec<Face>) {
    let (ink, vane, shaft, nib) =
        ([0.06, 0.05, 0.08, 1.0], [0.36, 0.66, 0.96, 1.0], [0.86, 0.66, 0.22, 1.0], [0.10, 0.08, 0.12, 1.0]);
    let (n0, t) = ([-0.46f32, 0.50], [0.50f32, -0.50]);
    let ax = [t[0] - n0[0], t[1] - n0[1]];
    let len = (ax[0] * ax[0] + ax[1] * ax[1]).sqrt();
    let d = [ax[0] / len, ax[1] / len];
    let nrm = [-d[1], d[0]];
    let at = |f: f32, side: f32| [cx + (n0[0] + ax[0] * f + nrm[0] * side) * scale, cy + (n0[1] + ax[1] * f + nrm[1] * side) * scale];
    let mut piece = |f0: f32, f1: f32, w: f32, rhombus: bool, color: [f32; 4]| {
        if rhombus {
            let m = (f0 + f1) * 0.5;
            out.push(poly(at(f0, 0.0), at(m, w), at(f1, 0.0), at(m, -w), color));
        } else {
            out.push(poly(at(f0, w), at(f1, w), at(f1, -w), at(f0, -w), color));
        }
    };
    piece(-0.08, 1.08, 0.075, false, ink);
    piece(0.24, 1.06, 0.30, true, ink);
    piece(0.0, 0.74, 0.04, false, shaft);
    piece(0.30, 1.0, 0.23, true, vane);
    piece(0.0, 0.22, 0.045, false, nib);
}
/// What a tracker row's icon shows, from `qtrack::RowIcon`; the same geometry as the world and map icons.
fn row_faces(icon: RowIcon) -> Vec<Face> {
    let mut v = Vec::new();
    match icon {
        RowIcon::Book => v = indicator_faces(Indicator::Available),
        RowIcon::Scroll => v = indicator_faces(Indicator::TurnIn),
        RowIcon::Quill => quill_at(0.0, 0.0, 1.0, &mut v),
        RowIcon::Emblem(mark) => v = emblem_with(mark, 0.0, true),
        RowIcon::MissionProgress(nation) => {
            v = emblem_with(MissionMark { nation, state: MissionState::Start }, 0.0, false);
            for (p, _) in v.iter_mut() {
                *p = p.map(|q| [q[0] * 0.82 - 0.06, q[1] * 0.82 - 0.06]);
            }
            quill_at(0.40, 0.40, 0.62, &mut v);
        }
    }
    v
}
/// Paint a tracker row icon (egui) in a square slot; the hover text names the state.
pub(super) const ROW_ICON_SLOT: egui::Vec2 = egui::vec2(22.0, 22.0);
pub(super) fn row_icon(ui: &mut egui::Ui, icon: RowIcon) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(ROW_ICON_SLOT, egui::Sense::hover());
    for (p, c) in row_faces(icon) {
        let p = p.map(|p| r.center() + egui::vec2(p[0], p[1]) * 13.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![p[0], p[1], p[3], p[2]],
            egui::Color32::from_rgba_unmultiplied(
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                (c[3] * 255.0) as u8,
            ),
            egui::Stroke::NONE,
        ));
    }
    resp.on_hover_text(icon.label())
}
fn preview_faces(ui: &mut egui::Ui, box_factor: f32, faces: impl Fn() -> Vec<Face>) {
    ui.horizontal(|ui| {
        for size in [14.0, 20.0, 28.0] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(size * box_factor, size * box_factor), egui::Sense::hover());
            for (p, c) in faces() {
                let p = p.map(|p| r.center() + egui::vec2(p[0], p[1]) * size);
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![p[0], p[1], p[3], p[2]],
                    egui::Color32::from_rgba_unmultiplied(
                        (c[0] * 255.0) as u8,
                        (c[1] * 255.0) as u8,
                        (c[2] * 255.0) as u8,
                        (c[3] * 255.0) as u8,
                    ),
                    egui::Stroke::NONE,
                ));
            }
        }
    });
}
pub(super) fn book_preview(ui: &mut egui::Ui) {
    for (kind, label) in [
        (Indicator::Available, "Available quest · open crystal book"),
        (Indicator::TurnIn, "Ready to turn in · sealed scroll"),
    ] {
        ui.label(label);
        preview_faces(ui, 1.4, || indicator_faces(kind));
    }
    ui.small("Confirmed server states only. Unknown availability or completion has no icon.");
}

/// Offline preview: the three nations' mission emblems, start (bright) over step (grey), at three sizes.
pub(super) fn emblem_preview(ui: &mut egui::Ui) {
    for state in [MissionState::Start, MissionState::Step] {
        for nation in Nation::ALL {
            let mark = MissionMark { nation, state };
            ui.label(mark.label());
            preview_faces(ui, 1.7, || emblem_faces(mark, 0.25));
        }
    }
}

#[cfg(test)]
mod indicator_tests {
    use super::*;
    fn assert_valid(faces: &[Face], bound: f32) {
        for (p, color) in faces {
            assert!(p.iter().flatten().all(|v| v.is_finite() && v.abs() <= bound));
            assert!(color.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
            let p = [p[0], p[1], p[3], p[2]];
            let mut signs = Vec::new();
            for i in 0..4 {
                let a = p[i];
                let b = p[(i + 1) % 4];
                let c = p[(i + 2) % 4];
                let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
                assert!(cross.abs() > 0.00001, "degenerate indicator face");
                signs.push(cross.signum());
            }
            assert!(signs.iter().all(|s| *s == signs[0]), "convex renderer cannot accept a folded face");
        }
    }
    #[test]
    fn indicators_have_distinct_silhouettes_and_valid_convex_faces() {
        let available = indicator_faces(Indicator::Available);
        let ready = indicator_faces(Indicator::TurnIn);
        assert_ne!(available, ready);
        for faces in [available, ready] {
            assert_valid(&faces, 0.7);
        }
    }
    #[test]
    fn mission_emblems_are_valid_convex_distinct_and_grey_when_a_step() {
        let mut seen: Vec<Vec<Face>> = Vec::new();
        for nation in Nation::ALL {
            for state in [MissionState::Start, MissionState::Step] {
                for phase in [0.0, 0.25, 0.5, 0.75, 0.999] {
                    let faces = emblem_faces(MissionMark { nation, state }, phase);
                    assert_valid(&faces, 0.85);
                    let saturation = |c: [f32; 4]| c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2]);
                    let most = faces.iter().filter(|(_, c)| c[3] >= 1.0).map(|(_, c)| saturation(*c)).fold(0.0, f32::max);
                    match state {
                        // The step state keeps no strong colour; the start state keeps its nation colour.
                        MissionState::Step => assert!(most < 0.2, "{nation:?} step is not grey ({most})"),
                        MissionState::Start => assert!(most > 0.5, "{nation:?} start lost its colour ({most})"),
                    }
                }
                seen.push(emblem_faces(MissionMark { nation, state }, 0.25));
            }
        }
        for a in 0..seen.len() {
            for b in a + 1..seen.len() {
                assert_ne!(seen[a], seen[b], "emblems {a} and {b} are identical");
            }
        }
        // Silhouettes differ per nation even without colour: compare the grey states' outline faces.
        let step = |n| emblem_faces(MissionMark { nation: n, state: MissionState::Step }, 0.0).len();
        assert!(step(Nation::SanDoria) != step(Nation::Windurst) || step(Nation::Bastok) != step(Nation::Windurst));
        // Only Start moves: Step is identical at every phase.
        let m = MissionMark { nation: Nation::Bastok, state: MissionState::Step };
        assert_eq!(emblem_faces(m, 0.1), emblem_faces(m, 0.6));
        let s = MissionMark { nation: Nation::Bastok, state: MissionState::Start };
        assert_ne!(emblem_faces(s, 0.1), emblem_faces(s, 0.6));
    }
    #[test]
    fn tracker_row_icons_are_valid_convex_and_distinct() {
        let mut all: Vec<(RowIcon, Vec<Face>)> = vec![
            (RowIcon::Book, row_faces(RowIcon::Book)),
            (RowIcon::Quill, row_faces(RowIcon::Quill)),
            (RowIcon::Scroll, row_faces(RowIcon::Scroll)),
        ];
        for nation in Nation::ALL {
            for icon in [
                RowIcon::Emblem(MissionMark { nation, state: MissionState::Start }),
                RowIcon::Emblem(MissionMark { nation, state: MissionState::Step }),
                RowIcon::MissionProgress(nation),
            ] {
                all.push((icon, row_faces(icon)));
            }
        }
        for (icon, faces) in &all {
            assert_valid(faces, 1.0);
            assert!(!icon.label().is_empty());
        }
        for a in 0..all.len() {
            for b in a + 1..all.len() {
                assert_ne!(all[a].1, all[b].1, "{:?} and {:?} draw the same", all[a].0, all[b].0);
                assert_ne!(all[a].0.label(), all[b].0.label());
            }
        }
        // The in-progress mission is the full-colour emblem plus the quill badge, so it has more faces than the plain one.
        let plain = emblem_with(MissionMark { nation: Nation::Bastok, state: MissionState::Start }, 0.0, false).len();
        assert_eq!(row_faces(RowIcon::MissionProgress(Nation::Bastok)).len(), plain + 5);
    }
    #[test]
    fn giver_icons_keep_book_and_scroll_for_quests_and_add_emblems_for_missions() {
        let start = MissionMark { nation: Nation::Bastok, state: MissionState::Start };
        assert!(giver_icons(&Giver::default()).is_empty());
        let quest = Giver { available: true, turn_in: true, missions: Vec::new() };
        assert_eq!(
            giver_icons(&quest),
            vec![GiverIcon::Quest(Indicator::Available), GiverIcon::Quest(Indicator::TurnIn)]
        );
        let all = Giver { available: true, turn_in: false, missions: vec![start] };
        assert_eq!(giver_icons(&all), vec![GiverIcon::Quest(Indicator::Available), GiverIcon::Mission(start)]);
        assert_eq!(all.count(), 2);
        assert!(!Giver::default().any() && all.any());
    }
}
