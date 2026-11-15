//! Small owner-local Windurst pilot. Reconstruct existing edges; do not invent art,
//! change UVs or activate a mod. All source-derived files stay in the chosen folder.
use super::*;

fn eligible(w: u32, h: u32, rgba: &[u8]) -> bool {
    (32..=512).contains(&w)
        && (32..=512).contains(&h)
        && rgba.len() == w as usize * h as usize * 4
        && rgba.chunks_exact(4).all(|p| p[3] >= 238)
}

/// Periodic Catmull-Rom, blended with bilinear reconstruction and clamped to the
/// nearest source colors. Periodic sampling preserves repeat tiling; clamping
/// prevents bright/dark halos. Material selection happens before this filter;
/// stored alpha (including stock 238/255 DXT dithering) is replicated unchanged.
fn reconstruct(w: u32, h: u32, src: &[u8], detail: f32) -> Result<Vec<u8>> {
    anyhow::ensure!(eligible(w, h, src), "pilot accepts only opaque 32..512 pixel scenery textures");
    anyhow::ensure!(detail.is_finite() && (0.0..=1.0).contains(&detail), "detail must be between 0 and 1");
    let cubic = |a: f32, b: f32, c: f32, d: f32, t: f32| {
        b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
    };
    let pixel = |x: i32, y: i32, c: usize| {
        src[((y.rem_euclid(h as i32) as usize * w as usize) + x.rem_euclid(w as i32) as usize) * 4 + c] as f32
    };
    let mut out = vec![255; (w * h * 16) as usize];
    for y in 0..h * 2 {
        for x in 0..w * 2 {
            let fx = (x as f32 + 0.5) * 0.5 - 0.5;
            let fy = (y as f32 + 0.5) * 0.5 - 0.5;
            let ix = fx.floor() as i32;
            let iy = fy.floor() as i32;
            let tx = fx - ix as f32;
            let ty = fy - iy as f32;
            out[((y * w * 2 + x) * 4) as usize + 3] = src[(((y / 2) * w + x / 2) * 4) as usize + 3];
            for c in 0..3 {
                let corners = [pixel(ix, iy, c), pixel(ix + 1, iy, c), pixel(ix, iy + 1, c), pixel(ix + 1, iy + 1, c)];
                let base = (corners[0] * (1.0 - tx) + corners[1] * tx) * (1.0 - ty)
                    + (corners[2] * (1.0 - tx) + corners[3] * tx) * ty;
                let row = |dy| {
                    cubic(
                        pixel(ix - 1, iy + dy, c),
                        pixel(ix, iy + dy, c),
                        pixel(ix + 1, iy + dy, c),
                        pixel(ix + 2, iy + dy, c),
                        tx,
                    )
                };
                let sharp = cubic(row(-1), row(0), row(1), row(2), ty);
                let lo = corners.into_iter().fold(f32::INFINITY, f32::min);
                let hi = corners.into_iter().fold(f32::NEG_INFINITY, f32::max);
                out[((y * w * 2 + x) * 4) as usize + c] = (base + (sharp - base) * detail).clamp(lo, hi).round() as u8;
            }
        }
    }
    Ok(out)
}

/// A texture shared with any cutout/blended/animated mesh is excluded. Images
/// outside the decoded Windurst surface-atlas namespace also stay stock, even
/// if a sky/effect happens to use an opaque mesh.
fn exclusive_surfaces<'a>(bindings: impl Iterator<Item = (&'a str, bool)>) -> std::collections::BTreeSet<String> {
    let mut usage = std::collections::BTreeMap::new();
    for (name, opaque) in bindings {
        // Windurst's decoded surface atlas namespace. Sky stars can also have
        // opaque MMB bindings; keep those and every non-surface namespace stock.
        let opaque = opaque && name.starts_with("model   ");
        usage.entry(name.trim().to_owned()).and_modify(|v| *v &= opaque).or_insert(opaque);
    }
    usage.into_iter().filter_map(|(name, opaque)| opaque.then_some(name)).collect()
}

/// Windurst Woods by default; an explicit pilot may choose another Windurst zone.
/// A fresh output is a private workspace, not a redistributable texture package.
pub fn windurst(zone: u32, out: &Path, detail: f32) -> Result<()> {
    let cfg = crate::config::Config::load();
    anyhow::ensure!(cfg.is_forever(), "the local Windurst pilot requires the Forever server profile");
    anyhow::ensure!((238..=242).contains(&zone), "choose a Windurst zone (238..242)");
    anyhow::ensure!(detail.is_finite() && (0.0..=1.0).contains(&detail), "detail must be between 0 and 1");
    protect_install(out, &cfg.ffxi_dir())?;
    let index = dat::DatIndex::open_stock(&cfg.ffxi_dir())?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(dat::ffximain(&cfg.ffxi_dir()))?)?;
    let scene = dat::zone::load_scene(&keys, &index.read(dat::zone::file_id(zone))?)?;
    let surfaces = exclusive_surfaces(scene.meshes.iter().flat_map(|mesh| {
        mesh.models.iter().map(move |m| {
            (m.texture.as_str(), !mesh.name.starts_with('_') && !mesh.animated && m.blending & 0x8000 == 0)
        })
    }));
    let surface_keys: std::collections::BTreeSet<_> =
        scene.textures.iter().filter(|t| surfaces.contains(t.name.trim())).map(key).collect();
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(out).context("choose a new local pilot folder")?;
    let export = out.join("original");
    let processed = out.join("reconstructed");
    let overlay = out.join("pack");
    export_zone(zone, &export)?;
    std::fs::create_dir(&processed)?;
    let manifest = std::fs::read_to_string(export.join("manifest.tsv"))?;
    let keys: std::collections::BTreeSet<_> = manifest.lines().filter_map(|l| l.split('\t').nth(2)).collect();
    let mut report = String::from("key\twidth\theight\tresult\n");
    let mut count = 0;
    for key in keys {
        let (w, h, rgba) = load_png(&export.join(format!("{key}.png")))?;
        if !surface_keys.contains(key) || !eligible(w, h, &rgba) {
            report.push_str(&format!("{key}\t{w}\t{h}\tstock: non-opaque material, alpha or size outside pilot\n"));
            continue;
        }
        let rebuilt = reconstruct(w, h, &rgba, detail)?;
        save_png(&processed.join(format!("{key}.png")), w * 2, h * 2, &rebuilt)?;
        report.push_str(&format!("{key}\t{w}\t{h}\t2x local reconstruction\n"));
        count += 1;
    }
    anyhow::ensure!(count > 0, "no suitable opaque surfaces in this zone");
    // Limit the pack manifest as well: identical pixels may also occur under a
    // different image name used by an effect. Never replace that effect's chunk.
    let pack_input = out.join("pack-input");
    std::fs::create_dir(&pack_input)?;
    let selected = manifest
        .lines()
        .filter(|line| {
            let f: Vec<_> = line.split('\t').collect();
            f.len() >= 4 && surfaces.contains(f[3].trim()) && processed.join(format!("{}.png", f[2])).is_file()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(pack_input.join("manifest.tsv"), selected)?;
    pack(&pack_input, &processed, &overlay, None)?;
    std::fs::write(out.join("selection.tsv"), report)?;
    std::fs::write(
        out.join("README.txt"),
        format!(
            "Owner-local Windurst texture pilot: zone {zone}, {count} opaque textures, detail {detail:.2}.\n\nThis is 2x reconstruction of existing edges, not AI-generated art or recovered detail. Colors are bounded to neighboring stock pixels. Stored alpha is replicated unchanged. Cutout/blended/animated materials and images without an opaque scenery binding remain stock, as do geometry and UVs.\n\nNo mod was installed or enabled, and Original/Enhanced settings were not changed. To try it, copy the pack folder into the Forever profile's mods directory under a unique name, then enable that folder in Settings > Mods and restart voluntarily. Disable it and restart to restore stock textures. Original graphics and texture mods are independent; disable this pack for a fully stock appearance.\n\nKeep this workspace private: original PNGs and generated overlays derive from your own game installation. Never include them in a client drop or upload them. The original installation was not modified.\n"
        ),
    )?;
    println!("{count} opaque Windurst textures reconstructed locally at 2x; overlay is disabled and not installed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_selection_excludes_fx_and_any_shared_transparent_surface() {
        let names = exclusive_surfaces(
            [
                ("model   wall", true),
                ("model   shared", true),
                ("model   shared", false),
                ("model   tree", false),
                ("star    star02", true),
            ]
            .into_iter(),
        );
        assert_eq!(names, std::collections::BTreeSet::from(["model   wall".to_owned()]));
        assert!(!names.contains("lens-flare"));
    }
    #[test]
    fn local_detail_keeps_rectangular_shape_palette_and_alpha() {
        let flat: Vec<_> = (0..32 * 64).flat_map(|_| [73, 119, 181, 255]).collect();
        assert_eq!(
            reconstruct(32, 64, &flat, 0.5).unwrap(),
            (0..64 * 128).flat_map(|_| [73, 119, 181, 255]).collect::<Vec<_>>()
        );
        let mut dithered = flat.clone();
        for p in dithered.chunks_exact_mut(8) {
            p[3] = 238;
        }
        let rebuilt = reconstruct(32, 64, &dithered, 0.5).unwrap();
        for y in 0..128 {
            for x in 0..64 {
                assert_eq!(rebuilt[(y * 64 + x) * 4 + 3], dithered[((y / 2) * 32 + x / 2) * 4 + 3]);
            }
        }
        let mut cutout = flat.clone();
        cutout[3] = 0;
        assert!(!eligible(32, 64, &cutout));
        assert!(reconstruct(32, 64, &cutout, 0.5).is_err());
        assert!(reconstruct(32, 64, &flat, f32::NAN).is_err());
        assert!(reconstruct(32, 64, &flat, 1.1).is_err());
    }
    #[test]
    fn periodic_reconstruction_survives_tile_translation_without_new_extremes() {
        let w = 32usize;
        let h = 64usize;
        let source: Vec<_> =
            (0..h).flat_map(|y| (0..w).flat_map(move |x| [40 + ((x * 7 + y * 3) % 120) as u8, 80, 160, 255])).collect();
        let shift = |src: &[u8], w: usize, h: usize, dx: usize, dy: usize| -> Vec<u8> {
            (0..h)
                .flat_map(|y| {
                    (0..w).flat_map(move |x| {
                        src[(((y + dy) % h * w + (x + dx) % w) * 4)..(((y + dy) % h * w + (x + dx) % w) * 4 + 4)]
                            .iter()
                            .copied()
                    })
                })
                .collect()
        };
        let actual = reconstruct(w as u32, h as u32, &source, 0.5).unwrap();
        let shifted = reconstruct(w as u32, h as u32, &shift(&source, w, h, 7, 11), 0.5).unwrap();
        assert_eq!(shifted, shift(&actual, w * 2, h * 2, 14, 22));
        assert!(actual.chunks_exact(4).all(|p| (40..=159).contains(&p[0]) && p[1..] == [80, 160, 255]));
    }
}
