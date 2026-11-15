# Forever graphics

Settings > Graphics > Forever Graphics offers Original and Enhanced. Changing the choice is a draft until Apply; the 15-second Keep/Revert prompt restores the previous settings unless accepted. Classic has no Forever Graphics row or directional-shadow renderer. Existing saved custom graphics remain available; a fresh install stays Original instead of choosing High/Ultra automatically.

Enhanced uses the original DAT geometry, texture coordinates, sky/weather palette and ambient light. Its profile enables 16× anisotropic filtering, mipmaps, modest sharpening, FXAA, SSAO, subdued bloom, tonemapping and height fog at native scene resolution. The optional directional shadow map attenuates direct light, preserving ambient and lamp light. Indoor scenes without a sky retain their original lighting. Original disables the Enhanced profile; individual legacy graphics controls and texture mods remain independent choices. Reduce Motion and the 60 FPS default remain active in either profile.

Character grounding and scene lighting fixes apply to both flavours. No new game engine or replacement art is involved. The existing soft-character-light value is preserved when profiles change.

## Local textures

The renderer already supports local texture overlays. A generated pack belongs to the player's install and is never included in a client drop or uploaded by these tools. Keep the original installation intact. Use Settings > Mods to enable or disable the local pack, then restart the client so the scene reloads.

Start with one zone, using a new output folder:

```sh
vana-client tex-export-zone 242 --out /path/to/local-heavens-tower
```

This Forever-only command reads the current Forever profile's owned installation. It excludes pixel-addressed images and refuses to overwrite an existing output folder. The manifest works with the existing packer. For the whole installation, the existing workflow is:

```sh
vana-client tex-export --out /path/to/local-export
# Process those PNGs with a local upscaler, retaining each image's name and aspect ratio.
vana-client tex-pack --manifest /path/to/local-export --upscaled /path/to/local-upscaled --out /path/to/forever-mods/my-local-hd
```

`forever-mods` above means the Forever client's data directory's `mods` folder (the launcher may provide an isolated data directory). Packs discovered there start disabled. Use a modest 2× scale first; the DAT reader's maximum texture side is 4096. Export excludes UI/font/map files and sprite-cell textures, which depend on exact pixel dimensions. The packer preserves geometry and UVs, and creates compressed images with mipmaps. It does not download a model, start an external service, overwrite the source DATs, or turn a pack on automatically.

Low-resolution stock art and an actual UV bug are different problems. The inspected Heavens Tower floor images are 128×256 and 128×128 with unit placement scale. No incorrect client UV transform has been demonstrated. Filtering can improve distant/angled surfaces; upscaling cannot reliably recover detail that is absent from the source. Preserve the original shapes, palette and painted style.

Heavens Tower's installed map table has no record for zone 242, map key 0. The minimap fallback therefore reads Radar. This is independent of the held Home Point destination-preview work.

## Water finish

Enhanced adds a restrained sky-coloured reflective finish to wading rivers whose collision data explicitly supplies a water height. The surface is clipped against the river bed at the shoreline, fades at shallow edges and with distance, and depth-tests against characters and scenery. Original renders no extra surface. It leaves the game's water textures, geometry, palette and splash effects in place. This pass does not guess ocean, fountain or particle-water materials from their names; those retain their existing rendering.

## Windurst local reconstruction pilot

A small opt-in pilot can now generate a local 2× overlay directly from the owned
installation, without a model download or an external service:

```sh
vana-client tex-windurst-pilot --zone 241 --detail 0.5 --out /path/to/new-private-windurst-pilot
```

Zone 241 is Windurst Woods; the pilot accepts only Windurst zones 238–242.
`--detail` ranges from 0 (bilinear reconstruction) to 1 (sharper cubic
reconstruction), with 0.5 as the restrained default. This reconstructs existing
edges, not missing detail or new artwork. It processes 32–512 pixel textures bound exclusively to opaque, static scenery
meshes in Windurst's decoded surface-atlas namespace, preserves their aspect
ratio at exactly 2×, samples across
repeat boundaries, and bounds colors to nearby original pixels to avoid halos.
Stored alpha, including stock DXT dithering, is replicated without alteration.
Cutout, blended and animated materials, sky/effect-only images, sprite-cell images,
geometry, UVs, material names and placement scale remain unchanged.

The output is a **private workspace** containing original PNGs, reconstructed
PNGs, `selection.tsv`, and a `pack` folder. Nothing is installed or enabled. To try
it, copy `pack` into the Forever profile's Mods directory under a unique name,
enable it in Settings > Mods, and restart voluntarily. Disable that mod and
restart to restore stock textures. Original/Enhanced graphics and texture mods
remain independent choices; a fully stock appearance uses Original with the
texture mod disabled. Never upload the source-derived workspace or include its
contents in a client release. The command refuses an existing output folder.

The zone exporter and shared packer refuse output destinations inside the original
installation. The packer also rejects replacement aspect-ratio changes and reports
stock read, parse and output write failures. Encoding caches follow PNG content,
so editing a PNG and rebuilding with `--redo` does not reuse stale pixels. Invalid
replacements fail the affected output file and report an error; missing PNGs
still intentionally retain stock images. A failed multi-file run can leave earlier
successful private outputs, so do not enable a pack until the command succeeds.

## Original environment art preview

The owner requested new environmental artwork as well as lighting quality. The
first preview targets plain stone flooring and masonry in Heavens Tower. It uses
new text-generated original artwork, not enlarged game pixels or redistributed
DAT images. Material provenance and exact prompts are recorded under
`assets/material-studies/20261106-environment/`.

The **Environment Textures → Tower Preview** setting is off by default,
requires Forever Graphics → Enhanced, and takes effect after Apply at the next
zone load. Existing settings and custom texture replacements remain intact.
Only exact stock-content matches on static opaque scenery bindings are eligible;
transparent uses of the same texture retain the stock image. Geometry, UVs,
collision, decorative atlases and bump-map associations remain unchanged. Other
zones stay stock. This is an initial material preview, not an all-world remaster.

Focused development checks and two offline before/after scene views passed;
publication still requires the final build/startup checks and publisher receipt. The initial irregular
paving study is retained as a candidate only and is not selected for the preview.

## Layered fog, sun shadow range and AO (Enhanced)

* **Layered Fog** (Settings > Post-processing, on in Enhanced, needs Height Fog): the height fog's density and height follow the Vana'diel time of day (thicker at dawn and dusk; a clear day and night are unchanged) and the weather (thicker and taller in mist, rain, storms, snow and fog; a little less in heat), eased over a few seconds and snapped on a zone change. The zone's own fog colour, which already follows time and weather, is kept; wet weather draws it slightly toward grey. Zones whose retail fog already closes in get half of the change (never thinner fog than before from a thickening weather). It is a CPU-side multiplier on the existing fog: no extra pass or shader.
* **Sun Shadow Range** (Settings > Graphics, Enhanced only): Near is the original 40-unit, 1024 map; Wide (the Enhanced preset) covers 80 units on a 2048 map (same sharpness, 16 MB of video memory; 3072 and 36 MB with Soft Shadows, below); Far covers 120 units on a 4096 map (64 MB). The cost is shadow-pass fill and memory; the pass already draws every source instance. Original never allocates more than the 1024 map.
* **Soft Shadows** (Settings > Graphics next to Sun Shadow Range, Enhanced only, on in the preset; saved Enhanced profiles take it once, keyed on `soft_shadows`): the sun shadow's lookup becomes a 16-tap Poisson disc of hardware-filtered depth comparisons whose radius is a fixed world width (~0.16 units, 1.5-4 map texels) instead of the 3 by 3 filter, so the penumbra is the same width at every range and the staircase on edges is smoothed. The receiver bias becomes a world-space normal offset (1.5 texels x sin of the angle to the light) plus a constant 0.04 units plus the filter's reach x tan of that angle (clamped), replacing the larger constant bias, so shadows stay attached at wall bases and feet while slopes do not acne. The map steps up with it: Near 2048 (16 MB), Wide 3072 (36 MB instead of 16 MB), Far stays 4096 (64 MB). Characters and NPCs drawn by the zone shader take the same filter; the separate character pipeline never sampled the sun map. Off (or Original) is the original filter and bias, byte for byte. Not done: PCSS-style contact hardening (see Shadow Cascades below).
* **Shadow Cascades** (Settings > Graphics Enhancements > Lighting & Shadows, Enhanced only, on in the preset; saved Enhanced profiles take it once, keyed on `shadow_cascades`): the sun shadow map becomes a depth array (one layer: the single map as before; three: cascades) of nested maps around the camera's target with radii 24, 72 and 200 units (render/forever_sun.rs `CASCADES`), 2048 texels each (48 MB of video memory; 12 MB at 1024 when the card is short, and the single map when even that does not fit, then its own size ladder, then no sun shadows: every allocation is `Gpu::try_alloc`, so a full card never crashes it). Each cascade is snapped to its own texels (so none swims) and rendered in its own depth pass (three passes over the scenery's casters instead of one: the main cost); `sun_visibility_cascaded` (sunshadow.wgsl) uses the nearest cascade whose interior (inside 80% of its radius) holds the point and blends into the next across the outer band (the last blends into lit), with the soft filter's normal offset and slope bias per cascade, so near shadows are about 2.3 times finer than the Wide soft map (0.023 against 0.052 units a texel), the middle matches it, and shadows reach 200 units (Far was 120). Character Shade and actors look up the same function, so they follow. Each cascade's pass draws only the casters whose boxes meet its light-space box: the camera's GPU cull (render/cull.rs) runs once more per cascade with `cs_cull_vis_shadow` (the mesh box through its model, door part and voyage against the cascade's ortho box, plus a 1.5 unit margin for sway), into that cascade's own culled-instance and indirect-argument buffers (`Cull::shadow_sets`, allocated with `Gpu::try_alloc`: none fitting means the unculled passes as before). A caster outside that box cannot reach the depth map, so the shadows are unchanged; the far cascade also leaves out casters under 0.5 units across (under 2.5 of its texels). `FFXI_NO_CASCADE_CULL=1` draws every caster in every cascade and `FFXI_CASCADE_SKIP=0` turns the small-caster skip off, to compare. Off (or Original) is the single map, a 1-layer array sampled exactly as before. Not done: contact-hardening (PCSS) shadows: the blocker search needs raw depth reads (`textureLoad` on the depth texture), which the GL backend cannot compile, and the zone shader is shared by every backend, so it is left out rather than risk the GL fallback.
* **Character Shade** (Settings > Graphics after Soft Shadows, Enhanced only, on in the preset; saved Enhanced profiles take it once, keyed on `actor_sun_shadows`): in-world characters, NPCs and monsters have always taken the sun shadow map's visibility on their direct light (render/zone.wgsl `lit_actor`); retail's actor lighting is ambient-heavy, so that darkens them only slightly. Character Shade takes an extra 30% of the actor's light (not its emissive part) at full shadow, scaled by the filtered visibility so it follows the soft edge, and fades out toward night; the lookup is taken 4 units toward the sun, outside the actor's own body, so only scenery above a character (canopy, roof) shades it, not its own silhouette, where the soft Character Light keeps actors readable. Off, Original and classic multiply by exactly 1. (`render/character.wgsl` is the offline portrait renderer, not the in-world actor path.) Dev check: `vana-client actor-sun-shot --zone 241 --hour 16 --eye ... --target ... --actor x,y,z | --pixel-actor px,py` renders GPU-skinned actors in a zone with the configured graphics.
* **Far View** (Settings > Graphics, Occlusion & Fog; Retail / Far 1.5x / Very Far 2x; Far in the Enhanced preset, saved Enhanced profiles take it once keyed on `far_view`): retail's Clipping setting (config 0x3e) multiplies each environment's fog end and the scenery's draw distance together (dat/weather.rs `clipping`; retail fog starts at 0 and ends at 396-450 outdoors, scenery stops at 252-401). Far View multiplies that factor by 1.5 or 2 (render/farview.rs) in open-air environments only: not the flags-bit-0 fixed light of interiors, and not environments whose raw draw distance is under 200 (towers, dungeons). One factor feeds the lighting's fog (every zone and offline path), and the draw-distance and own-distance culling (GPU cull and the CPU run cull), so the horizon, the fog that hides it and the cull distance stay consistent. (With Settings' Draw Distance on Unlimited, the default, the zone's draw distance is already lifted, so Far View then moves the fog end and the placements' own distances only; the camera lists' beyond-distance stays retail's.) The projection's far plane (2000) and near plane are unchanged, so depth precision is the same. Because retail's fog is linear from the camera, a bigger distance also thins the haze at every range, so Far View is a slightly clearer, more distant picture, not only a farther one (not retail). It costs more drawing in open zones (about +15-25% on the software renderer at long-view cameras, not hardware-measured). Camera lists (retail's per-area visibility sets) still apply, so in city zones that use them only listed placements extend. Far View shows what the zone data contains: zone edges that retail's fog hid can become visible at Very Far, and a field's terrain could not be checked offline (zone-shot loads only a field's placed objects).
* **Ambient occlusion** is applied once: the basic Ambient Occlusion (Settings > Graphics) stands aside while the post pass's own is on.
* Saved Enhanced profiles from before these options take the preset's values once at load; Original and Custom are untouched. The view distance is unchanged: scenery already draws past retail's reach by default (Unlimited) and the retail fog ends beyond the retail draw distance, so raising the projection's far plane would only cost.
* **AO Applies To** (Settings > Post-processing, Enhanced/Custom, needs Ambient Occlusion): *All Light* (default, unchanged) darkens everything in a corner; *Ambient Only* darkens just the ambient part of the scenery's light, so sunlit and lamp-lit surfaces keep their brightness and only shaded creases deepen. The scenery shaders record the ambient share of their light in the scene's alpha while the option is on, a small capture pass stores it with the view depth, and the post pass weights its occlusion by it (characters and anything drawn later keep the plain occlusion). With the option off nothing is recorded or allocated and the output is unchanged. In the Enhanced preset since wave 3 (strength 0.9; ambient-only lets it be stronger without darkening direct light).

## Zone Lighting, Fog Layers (Enhanced, wave 3)

* **Zone Lighting** (+ **Zone Lighting Strength**, Settings > Post-processing, on in Enhanced; 100% = as authored, up to 150%): re-grades the zone's own retail lighting (`render/lightgrade.rs`). The sun turns warmer by day and golden near the horizon, the shade (ambient) cooler and richer, the sky richer with a deeper zenith by day, the fog/clear colour a cool haze by day and gold at the golden hours, and the night keeps a blue hue with a small lift so shapes read. A zone's profile is read once from its own noon light (an already-warm sun is warmed less, an already-blue shade cooled less, muted palettes enriched more, short-fog zones keep their haze, interiors are graded gently) with authored overrides for the Windurst zones; each colour is also shifted less the further it already leans the same way. The grade is blended by the sun's own height (continuous across midnight) and applied by one function to the zone and every area environment, so the offline renders show what the client draws. Directions, shadows, distances and the character light are untouched; tint maths runs in linearised colour but the scene is still shaded in gamma space (not a linear-light pipeline). Off (Original) the lighting is exactly the retail value.
* **Fog Layers** (needs Height Fog): a second volume-style layer after the height fog: a thin ground mist (denser at dawn and dusk and in wet weather), a warm glow in the haze toward the sun and a distance haze that carries it. One extra block in the existing depth post pass; no extra pass, texture or allocation. Open air only: zones whose retail fog closes in (interiors such as the Heavens Tower) or with fixed room light get none of it, and the Heavens Tower profile grades almost nothing, so interiors keep their retail look. The mist is restrained in clear weather and most visible in mist, rain, fog and at dawn and dusk.
* Saved Enhanced profiles lacking `light_grade` take the new preset once (Zone Lighting, Fog Layers, Ambient Only occlusion at 0.9); Original and Custom are untouched.
