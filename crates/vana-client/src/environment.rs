//! Original environment art preview. Only audited opaque scenery bindings change;
//! stock images remain available to blended/cutout models and installed mods win.
use crate::dat::zone::{Mmb, Texture, ZoneScene};
use std::sync::OnceLock;

const SLABS: &[u8] = include_bytes!("../../../assets/material-studies/20261106-environment/limestone-slabs-v1.png");
const MASONRY: &[u8] = include_bytes!("../../../assets/material-studies/20261106-environment/limestone-masonry-v1.png");
const WOODS: &[u8] = include_bytes!("../../../assets/material-studies/20261106-windurst/woods-masonry-v1.png");
const TOWER_WALL: &[u8] =
    include_bytes!("../../../assets/material-studies/20261106-tower-expanded/tower-ashlar-v1.png");
const TOWER_GROUND: &[u8] =
    include_bytes!("../../../assets/material-studies/20261106-tower-expanded/tower-ground-v1.png");
struct Material {
    zone: u32,
    alias: Option<&'static str>,
    name: &'static str,
    stock_key: &'static str,
    image: &'static [u8],
}
// Content fingerprints are of the owned-install inputs, not redistributed pixels.
const MATERIALS: &[Material] = &[
    Material {
        zone: 242,
        alias: Some("ffenv   twrwall1"),
        name: "model   w05_kb1",
        stock_key: "c14bc9464bc2e3d6",
        image: TOWER_WALL,
    },
    Material {
        zone: 242,
        alias: Some("ffenv   twrgrnd1"),
        name: "model   w05_yk2",
        stock_key: "8a3722bb3e703690",
        image: TOWER_GROUND,
    },
    Material { zone: 242, alias: None, name: "model   w05_yk1", stock_key: "22c9c02ea1d0e497", image: SLABS },
    Material { zone: 242, alias: None, name: "model   w05_kb2", stock_key: "a7dfd1d652707a9a", image: MASONRY },
    Material {
        zone: 241,
        alias: Some("ffenv   wdwood01"),
        name: "model   t_win06d",
        stock_key: "b539138075b87aec",
        image: WOODS,
    },
];
fn replacement_name(name: &str) -> String {
    format!("ffenv   {}", &name[8..])
}
fn eligible(mesh: &Mmb) -> bool {
    !mesh.animated && !mesh.name.starts_with('_')
}

/// No file writes, geometry/UV edits, downloads or fallback replacements. Returning
/// zero means stock scenery. A mismatching custom texture is deliberately untouched.
pub fn apply(enabled: bool, zone: u32, scene: &mut ZoneScene) -> usize {
    if !enabled || !MATERIALS.iter().any(|m| m.zone == zone) {
        return 0;
    }
    static ART: OnceLock<Vec<Option<(u32, u32, Vec<u8>)>>> = OnceLock::new();
    let art = ART.get_or_init(|| MATERIALS.iter().map(|m| crate::texpack::load_png_bytes(m.image).ok()).collect());
    let mut changed = 0;
    for (material, image) in MATERIALS.iter().zip(art) {
        if material.zone != zone {
            continue;
        }
        let Some((w, h, rgba)) = image else {
            continue;
        };
        let Some(stock) = scene.textures.iter().find(|t| t.name == material.name) else {
            continue;
        };
        if crate::texpack::key(stock) != material.stock_key
            || u64::from(*w) * u64::from(stock.height) != u64::from(*h) * u64::from(stock.width)
        {
            continue;
        }
        let replacement = material.alias.map(str::to_owned).unwrap_or_else(|| replacement_name(material.name));
        if scene.textures.iter().any(|t| t.name == replacement) {
            continue;
        }
        // Do not bind a private alias to a pre-existing unknown normal map.
        if material.alias.is_some() && scene.bumps.iter().any(|b| b.key.as_slice() == &replacement.as_bytes()[8..]) {
            continue;
        }
        let mut count = 0;
        for mesh in scene.meshes.iter_mut().filter(|m| eligible(m)) {
            for model in &mut mesh.models {
                if model.texture == material.name && model.blending & 0x8000 == 0 {
                    model.texture = replacement.clone();
                    count += 1;
                }
            }
        }
        if count > 0 {
            // The renderer bumps a whole mesh only if every part has a normal.
            // A neutral normal avoids old mortar relief without disabling the
            // neighbouring parts' existing bump maps. Bump-off stays off.
            if material.alias.is_some()
                && scene.bumps.iter().any(|b| b.key.as_slice() == &material.name.as_bytes()[8..])
            {
                let mut key = [0; 8];
                key.copy_from_slice(&replacement.as_bytes()[8..]);
                scene.bumps.push(crate::dat::bump::BumpMap {
                    key,
                    width: 4,
                    height: 4,
                    rgba: crate::dat::bump::normal_map(4, 4, &[0; 16]),
                });
            }
            scene.textures.push(Texture::new(replacement, 0, *w, *h, rgba.clone()));
            changed += count;
        }
    }
    log::info!("environment preview: zone {zone}, {changed} audited opaque bindings");
    changed
}

pub fn binding_count(scene: &ZoneScene) -> usize {
    scene.meshes.iter().flat_map(|m| &m.models).filter(|m| m.texture.starts_with("ffenv   ")).count()
}

pub fn tower_status(scene: Option<u16>, bindings: Option<usize>, loading: bool) -> String {
    if loading {
        return "Scenery is loading; preview status is not ready yet.".into();
    }
    match (scene,bindings) {
        (Some(242),Some(n)) if n>0 => "Heavens Tower: preview materials are selected in the loaded scenery.".into(),
        (Some(242),Some(_)) => "Heavens Tower: installed textures are loaded; preview is not bound. After Apply, the preview appears on your next voluntary zone load.".into(),
        _ => "Preview location: Heavens Tower main hall and stone floors/walls; separate room interiors remain unchanged.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Bounded metadata for actual triangle UV density and the adjoining ground seam.
    #[test]
    #[ignore = "Requires private owned DATs; geometry metadata stays private"]
    fn owned_woods_ground_triangle_metadata() {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open_stock(&cfg.ffxi_dir()).unwrap();
        let keys =
            crate::dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap())
                .unwrap();
        let scene = crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(241)).unwrap()).unwrap();
        let mesh = scene.meshes.iter().find(|m| m.name == "wi_misr01_1").expect("audited mesh absent");
        let placement = scene
            .placements
            .iter()
            .find(|p| {
                p.translate == [-20.0, 0.0, 20.0]
                    && p.lod_names(|name| scene.meshes.iter().any(|m| m.name == name))
                        .iter()
                        .any(|n| n.as_ref() == mesh.name)
            })
            .expect("audited placement absent");
        let matrix = glam::Mat4::from_cols_array_2d(&placement.matrix());
        for (part, model) in mesh.models.iter().enumerate().take(2) {
            let texture = scene.textures.iter().find(|t| t.name == model.texture).unwrap();
            let vertices:Vec<_>=model.vertices.iter().map(|v|serde_json::json!({"world":matrix.transform_point3(glam::Vec3::from(v.pos)).to_array(),"uv":v.uv})).collect();
            println!(
                "GROUND_PATCH {}",
                serde_json::json!({"zone":241,"mesh":mesh.name,"part":part,"texture":model.texture,"texture_size":[texture.width,texture.height],"key":crate::texpack::key(texture),"vertices":vertices,"indices":model.indices,"strip":model.strip,"blend":model.blending})
            );
        }
        assert_eq!(mesh.models[0].texture, "model   t_win00d");
        assert_eq!(mesh.models[1].texture, "model   t_win07d");
    }

    /// Read-only material/placement metadata for the private Windurst art audit.
    /// Prints no pixels and does not generate, install or enable a texture pack.
    #[test]
    #[ignore = "Requires private owned-install fixture; inventory output stays private"]
    fn owned_woods_material_binding_inventory() {
        material_binding_inventory(241, "model   t_win");
    }
    #[test]
    #[ignore = "Requires private owned-install fixture; inventory output stays private"]
    fn owned_tower_material_binding_inventory() {
        material_binding_inventory(242, "model   ");
    }
    fn material_binding_inventory(zone: u32, prefix: &str) {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open_stock(&cfg.ffxi_dir()).unwrap();
        let keys =
            crate::dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap())
                .unwrap();
        let scene = crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(zone)).unwrap()).unwrap();
        let bumps = crate::dat::bump::parse(&index.read(crate::dat::bump::file_id(zone)).unwrap());
        let mut seen = 0;
        for t in scene.textures.iter().filter(|t| t.name.starts_with(prefix)) {
            let mut bindings = Vec::new();
            for mesh in &scene.meshes {
                for (part, model) in mesh.models.iter().enumerate().filter(|(_, m)| m.texture == t.name) {
                    let lo = model.vertices.iter().fold([f32::MAX; 2], |mut a, v| {
                        for k in 0..2 {
                            a[k] = a[k].min(v.uv[k]);
                        }
                        a
                    });
                    let hi = model.vertices.iter().fold([f32::MIN; 2], |mut a, v| {
                        for k in 0..2 {
                            a[k] = a[k].max(v.uv[k]);
                        }
                        a
                    });
                    let placements:Vec<_>=scene.placements.iter().filter(|p|p.lod_names(|name|scene.meshes.iter().any(|m|m.name==name)).iter().any(|n|n.as_ref()==mesh.name)).map(|p| {
                        let matrix=glam::Mat4::from_cols_array_2d(&p.matrix());
                        let mut lo=glam::Vec3::splat(f32::MAX);let mut hi=glam::Vec3::splat(f32::MIN);
                        for v in &model.vertices {let p=matrix.transform_point3(glam::Vec3::from(v.pos));lo=lo.min(p);hi=hi.max(p);}
                        serde_json::json!({"mesh":p.name,"translate":p.translate,"scale":p.scale,"drawn":p.drawn(),"bounds":[lo.to_array(),hi.to_array()]})
                    }).collect();
                    bindings.push(serde_json::json!({"mesh":mesh.name,"part":part,"animated":mesh.animated,"blend":model.blending,"eligible_opaque":eligible(mesh)&&model.blending&0x8000==0,"uv":[lo,hi],"vertices":model.vertices.len(),"placements":placements}));
                }
            }
            let alpha: std::collections::BTreeSet<_> = t.rgba.chunks_exact(4).map(|p| p[3]).collect();
            let normal: Vec<_> = bumps
                .iter()
                .filter(|b| b.key.as_slice() == &t.name.as_bytes()[8..])
                .map(|b| serde_json::json!({"width":b.width,"height":b.height}))
                .collect();
            println!(
                "MATERIAL {}",
                serde_json::json!({"zone":zone,"name":t.name,"stock_key":crate::texpack::key(t),"width":t.width,"height":t.height,"alpha":alpha,"bump":normal,"bindings":bindings})
            );
            seen += 1;
        }
        assert!(seen > 0, "expected surface namespace missing: do not apply guessed bindings");
    }

    #[test]
    fn tower_status_distinguishes_saved_choice_from_loaded_materials() {
        assert!(tower_status(Some(242), Some(0), false).contains("preview is not bound"));
        assert!(tower_status(Some(242), Some(184), false).contains("selected in the loaded"));
        assert!(tower_status(Some(242), Some(184), true).contains("not ready"));
        assert!(tower_status(Some(425), Some(184), false).contains("separate room"));
        assert!(tower_status(None, None, false).contains("Preview location"));
    }
    #[test]
    fn original_art_has_matching_aspects_and_preserves_bump_suffix() {
        for m in MATERIALS {
            let (w, h, rgba) = crate::texpack::load_png_bytes(m.image).unwrap();
            assert!(w >= 512 && h >= 512 && w <= 2048 && h <= 2048);
            assert_eq!(h, if m.zone == 241 || m.name.ends_with("yk1") || m.name.ends_with("kb1") { w * 2 } else { w });
            assert!(rgba.chunks_exact(4).all(|p| p[3] == 255));
            if let Some(alias) = m.alias {
                assert_eq!(alias.len(), 16);
                assert_ne!(&alias[8..], &m.name[8..]);
            } else {
                assert_eq!(&replacement_name(m.name)[8..], &m.name[8..]);
            }
        }
    }
    #[test]
    #[ignore = "Requires the private owned-install fixture; never distribute its assets"]
    fn owned_tower_changes_only_audited_opaque_bindings() {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open_stock(&cfg.ffxi_dir()).unwrap();
        let keys =
            crate::dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap())
                .unwrap();
        let data = index.read(crate::dat::zone::file_id(242)).unwrap();
        let mut scene = crate::dat::zone::load_scene(&keys, &data).unwrap();
        scene.bumps = crate::dat::bump::parse(&index.read(crate::dat::bump::file_id(242)).unwrap());
        let old_bumps: Vec<_> = scene.bumps.iter().map(|b| (b.key, b.rgba.clone())).collect();
        let all_bumped = |s: &ZoneScene| {
            s.meshes
                .iter()
                .map(|m| {
                    m.models.iter().all(|p| {
                        p.texture.as_bytes().get(8..).is_some_and(|key| s.bumps.iter().any(|b| b.key.as_slice() == key))
                    })
                })
                .collect::<Vec<_>>()
        };
        let bumped_before = all_bumped(&scene);
        let tex = scene.textures.len();
        let geometry = |s: &ZoneScene| {
            format!(
                "{:?}",
                s.meshes
                    .iter()
                    .map(|m| (
                        &m.name,
                        m.animated,
                        m.models.iter().map(|p| (&p.vertices, &p.indices, p.blending, p.strip)).collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            )
        };
        let before = geometry(&scene);
        let original: Vec<_> =
            scene.meshes.iter().map(|m| m.models.iter().map(|p| p.texture.clone()).collect::<Vec<_>>()).collect();
        assert_eq!(apply(false, 242, &mut scene), 0);
        assert_eq!(apply(true, 241, &mut scene), 0);
        for m in MATERIALS {
            println!(
                "candidate {}: stock {}, opaque {}",
                m.name,
                scene.textures.iter().find(|t| t.name == m.name).map(crate::texpack::key).unwrap_or_default(),
                scene
                    .meshes
                    .iter()
                    .filter(|mesh| eligible(mesh))
                    .flat_map(|mesh| &mesh.models)
                    .filter(|p| p.texture == m.name && p.blending & 0x8000 == 0)
                    .count()
            );
        }
        let n = apply(true, 242, &mut scene);
        assert_eq!(n, 184);
        assert_eq!(binding_count(&scene), 184);
        assert_eq!(all_bumped(&scene), bumped_before);
        for (key, rgba) in &old_bumps {
            assert_eq!(&scene.bumps.iter().find(|b| b.key == *key).unwrap().rgba, rgba);
        }
        for name in ["twrwall1", "twrgrnd1"] {
            assert!(
                !scene.bumps.iter().any(|b| b.key.as_slice() == name.as_bytes()),
                "these source materials have no normal maps; do not invent them"
            );
        }
        assert_eq!(scene.textures.len(), tex + 4);
        assert_eq!(geometry(&scene), before);
        for (i, m) in scene.meshes.iter().enumerate() {
            for (j, p) in m.models.iter().enumerate() {
                if p.texture != original[i][j] {
                    assert!(eligible(m) && p.blending & 0x8000 == 0);
                    assert!(MATERIALS.iter().any(|a| a.name == original[i][j]));
                    let material = MATERIALS.iter().find(|a| a.name == original[i][j]).unwrap();
                    assert_eq!(
                        p.texture,
                        material.alias.map(str::to_owned).unwrap_or_else(|| replacement_name(material.name))
                    );
                }
            }
        }
        assert_eq!(apply(true, 242, &mut scene), 0, "idempotent");
        let mut modified = crate::dat::zone::load_scene(&keys, &data).unwrap();
        for t in &mut modified.textures {
            if MATERIALS.iter().any(|m| m.name == t.name) {
                t.rgba[0] ^= 1;
            }
        }
        assert_eq!(apply(true, 242, &mut modified), 0, "never overwrite a custom texture");
        println!("{n} opaque bindings,4 original material copies; stock/UVs/mesh flags retained");
    }
    #[test]
    #[ignore = "Requires private owned-install fixture; no assets are exported"]
    fn owned_woods_preview_preserves_geometry_mods_and_avoids_unrelated_bump() {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open_stock(&cfg.ffxi_dir()).unwrap();
        let keys =
            crate::dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap())
                .unwrap();
        let data = index.read(crate::dat::zone::file_id(241)).unwrap();
        let mut scene = crate::dat::zone::load_scene(&keys, &data).unwrap();
        scene.bumps = crate::dat::bump::parse(&index.read(crate::dat::bump::file_id(241)).unwrap());
        let all_bumped = |s: &ZoneScene| {
            s.meshes
                .iter()
                .map(|m| {
                    m.models.iter().all(|p| {
                        p.texture.as_bytes().get(8..).is_some_and(|key| s.bumps.iter().any(|b| b.key.as_slice() == key))
                    })
                })
                .collect::<Vec<_>>()
        };
        let before_bumped = all_bumped(&scene);
        let original_bumps: Vec<_> = scene.bumps.iter().map(|b| (b.key, b.width, b.height, b.rgba.clone())).collect();
        let original: Vec<_> =
            scene.meshes.iter().map(|m| m.models.iter().map(|p| p.texture.clone()).collect::<Vec<_>>()).collect();
        let geometry = |s: &ZoneScene| {
            format!(
                "{:?}",
                s.meshes
                    .iter()
                    .map(|m| (
                        &m.name,
                        m.animated,
                        m.models.iter().map(|p| (&p.vertices, &p.indices, p.blending, p.strip)).collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            )
        };
        let before = geometry(&scene);
        let textures = scene.textures.len();
        assert_eq!(apply(false, 241, &mut scene), 0);
        assert_eq!(apply(true, 242, &mut scene), 0, "Tower choice cannot enable Woods");
        assert_eq!(apply(true, 241, &mut scene), 3);
        assert_eq!(scene.textures.len(), textures + 1);
        assert_eq!(geometry(&scene), before);
        let material = MATERIALS.iter().find(|m| m.zone == 241).unwrap();
        assert_eq!(
            crate::texpack::key(scene.textures.iter().find(|t| t.name == material.name).unwrap()),
            material.stock_key,
            "original texture retained"
        );
        let alias = material.alias.unwrap();
        assert_eq!(all_bumped(&scene), before_bumped, "neighbouring parts retain bump eligibility");
        for (key, w, h, rgba) in &original_bumps {
            let b = scene.bumps.iter().find(|b| b.key == *key).unwrap();
            assert_eq!((b.width, b.height, &b.rgba), (*w, *h, rgba));
        }
        let flat = scene.bumps.iter().find(|b| b.key.as_slice() == &alias.as_bytes()[8..]).unwrap();
        assert_eq!((flat.width, flat.height), (4, 4));
        assert!(flat.rgba.chunks_exact(4).all(|p| p == [128, 128, 255, 0]));
        for (i, m) in scene.meshes.iter().enumerate() {
            for (j, p) in m.models.iter().enumerate() {
                if p.texture != original[i][j] {
                    assert_eq!(original[i][j], material.name);
                    assert_eq!(p.texture, alias);
                    assert!(eligible(m) && p.blending & 0x8000 == 0);
                }
            }
        }
        assert_eq!(apply(true, 241, &mut scene), 0, "idempotent");
        let mut modified = crate::dat::zone::load_scene(&keys, &data).unwrap();
        modified.textures.iter_mut().find(|t| t.name == material.name).unwrap().rgba[0] ^= 1;
        assert_eq!(apply(true, 241, &mut modified), 0, "installed custom art wins");
        let mut plain = crate::dat::zone::load_scene(&keys, &data).unwrap();
        assert_eq!(apply(true, 241, &mut plain), 3);
        assert!(plain.bumps.is_empty(), "bump-off stays off");
        println!("Woods:3 opaque parts; geometry/UVs and neighbouring bump eligibility retained; neutral new normal");
    }
}
