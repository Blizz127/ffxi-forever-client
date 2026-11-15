# Forever client: graphics status

Updated 2026-11-15 10:27 UTC for coordinator task fbdbefc3841f (owner order of 2026-11-14 2:18 PM CST: graphics upgrades
are the top priority; gate 1: every upgrade off = retail behaviour and look) and the owner's later word of 11:43 PM
CST ("turn everything on to the new graphics settings for forever by default"), which replaced that order's "off by
default".

One read-only feed observation at **09:12 UTC** found owner-native `1.0.12-forever.143` (channel `f61ffd0e...`)
and regular FF11 `1.0.8-vana.20` (`d3804d29...`). This is not owner-device installation evidence. Existing
`1.0.12-vana.25` and Forever145 are sealed QA/publisher handoffs; neither is replayed. The remaining deployment
step belongs to the authorized QA/sole-publisher route, including its actual zero-human/access/rollback holds.

Forever **146** (`9520e5ad`, packaged10:27 UTC) retains the144 renderer and preset implementation. It adds
Rhapsody intake12 while keeping intake11, a Settings page picker, and defaults for the three Controller fields.
Both dist builds passed;46 Rhapsody checks and24actual notebook font rows (max982<=1056),7correctedController
checks,11modern checks retained by file identity, and an actual classic compile. Six actual pictures were viewed;
two Woods comparisons byte-equal144; WineCLI and archive/backup/stage checks passed. Original's offline Settings
fixture proves picker visibility only; retail row bodies are omitted. No live input/story, audio-output, realGPU
or nativeWindowsGUI proof. ONE existing834a QA packet is reported, publication unconfirmed; no lead publishing.
Sealed144/145/146 remain immutable. Actual predecessor and rollback remain the sole publisher's responsibility.

**Real-GPU status, for the whole table: real-GPU test pending.** This lane has no GPU: every picture and number below
comes from software Vulkan (lavapipe) offline renders, unit tests and the offline measuring tools. The only real-GPU
facts are read-only observations of the owner's device log from 2026-11-12 (build 108): DLSS evaluating on an RTX
5070 Ti Laptop at 1969x1231 -> 2560x1600. From 143 the client log carries one line at window creation with what the
adapter and the display can do: the owner's next session will give the first such line. No HDR display has shown any
of the HDR work.

## Gate status

| Gate | State in 144 | What is missing |
|---|---|---|
| 1: all upgrades off = retail look | **Shown against the classic flavour, by the launch switch.** Started with `--original-look` (or `FFXI_ORIGINAL=1`) the client draws, byte for byte, what 143's Original look drew, which equalled the classic flavour's picture on three scenes; the saved file is not touched. Picking Original in Settings gives the original look plus the player's own choices in options that no look owns (colour grading, character shadows, basic ambient occlusion, light shading, draw distance, effect sprites, 3D aspect, HDR output): equal to the switch for every profile that left those alone. | Never compared with the retail program itself (FL-1: "retail look" rests on upstream vana-client's reading of it; the reverse-engineering tool is blocked and the retail DLL on disk is packed). Known differences that are upstream's defaults and the same in the classic flavour: 16x anisotropic filtering, Reduce Motion on, the window's aspect instead of the config ratio, a 60 fps limit, control scheme 2, and three retail config-tool defaults (mip mapping on, environment animation 2, bump mapping on). By design a software (CPU) adapter halves the scene size and sets anisotropy 1 the first time. The interface is the Modern UI by default. |
| 2: every upgrade an option; the default | **Every upgrade is an option, and since 144 the default is ON** (the owner's word of 2026-11-14 23:43 CST): each Forever profile, new or existing, is moved once to the Forever look with every finished upgrade; brightness, the upscaler, render scale, filtering and all non-graphics settings are kept; a Custom setup is stored and the Look row's Custom brings it back; a later choice of Original / Enhanced / Custom stays. Original is one pick away and one launch switch away. | Not in the default because they depend on the hardware or are experimental: the upscalers (DLSS / FSR), HDR Output and HDR Highlights, the High tiers of ambient occlusion and fog, Effect Sprites, Render Scale. |

## Current graphics features (every one an option; the Forever look turns the finished ones on)

| Area | In the client | Since | Verified how |
|---|---|---|---|
| Looks | Original, Enhanced, Forever, Custom; saved presets follow their look; Original keeps a Custom setup; launch switch to the original look | 99-143 | renders; look-follow runtime proof each build; fresh == Original == under the switch on the exact binary (143) |
| Lighting | hemisphere ambient, zone lighting grade, Lamp Lights, Lamp Light Shading, Linear Shading (experimental), Forever tone curve + time-of-day LUT, brightness slider | 82-129 | renders; parity audit against the WoW Forever reference numbers (build 111) |
| Shadows | sun shadow cascades (Near / Wide / Far / Forever 4x2048), soft filter, far-cascade cache | 69-108 | renders; cache on/off byte-equal or within 0.002/255 |
| Ambient occlusion, fog | AO (+ High tier), volumetric fog, froxel fog with lamp in-scatter (+ High tier), fog layers | 71-113 | renders; motion tool: both High tiers stable |
| Anti-aliasing | SMAA, CMAA2; DLAA with DLSS | 98 | renders |
| Upscalers | FSR 1, FSR 2 (all builds); DLSS super resolution / DLAA with bundled K/L/M presets (private Linux build only; Windows ships without it) | 66-108 | lavapipe tests; DLSS: owner device log only |
| HDR | HDR Output (scRGB), HDR Rendering (float scene), HDR Highlights with emitters: sun, moon, lamps, stars (experimental) | 98-139 | offline HDR test window + false-colour stills; **no HDR display** |
| Draw distance | Far View, Draw Distance, Prepare Nearby Areas, persistent texture cache | 71-112 | renders; cold vs warm cache byte-equal |
| Textures | Region ground textures rebuilt from the user's own files in 77 of 84 base zones; Scenery / Character / Map "Sharper"; Near Detail; tiling variation | 77-125 | stills per zone; motion tool for shimmer |
| Sky, water | sky clouds in heavy weather, enhanced water, sun shafts | 69-71 | renders |
| Resolution, UI scale | Render Scale 25-400% with the real drawn size shown and a memory cap; Render Resolution (retail's); Interface Size Auto; UI Scale; Compact HUD; Hide HUD in Cutscenes | 74-143 | size-rule tests; stills at 25 / 50 / 100 / 200 / 400%; one live observation of the HUD fade (another lane, build 140) |
| Options menu | unified Settings with pages, Apply / Cancel on every row; a "Requested / Active now" line with the reason in the Upscaler, Upscaler Quality, HDR Rendering, HDR Output, HDR Highlights, Fog Volume Detail, AO Quality and Froxel fog rows | 58-143 | settings tests every build; status under forced capabilities; settings pages rendered on lavapipe |
| Measuring | capability line in the log at start; per-pass GPU timing (F12); GPU autosample blocks in the log (private builds) | 106-143 | offline; **the first real numbers need the owner's log** |

## Missing or unfinished

| Item | State |
|---|---|
| Real-GPU verification | everything, and 144 turns the heaviest options on by default: the capability line in the log, frame cost per pass, DLSS / FSR quality, shadows and fog at 2560x1600, the pause while an area's textures are built at its first visit |
| HDR on a display | never seen; wgpu sends no HDR metadata |
| Temporal anti-aliasing without DLSS | FSR 2 at native size flickers. Isolated phase8 probe did not improve:23pairs mean4.760/36.10%pixels over4/255 versus phase16 mean4.754/35.76%; unchanged16control reproduced exactly, both pictures viewed. Held; next luma-format probe is source-only. No real GPU or motion proof. |
| The ships' sea | Enhanced Water over the sea of the four ship zones is built on a branch; a line at about 60 units with speckle beyond, cause unknown (not depth fighting: tested); held |
| MSAA | none (single-sample scene); large |
| DLSS on Windows | shim built (build 68 line), not shipped; owner/NVIDIA-terms hold remains. Linux 145 bundles runtime 310.9.16 privately. Frame generation is not implemented. |
| Options menu | Legacy category Defaults already exists. Modern topic defaults need an explicit safe row mapping; the category picker is isolated source with focused input checks in progress, not shipped. |
| Texture coverage | 77 of 84 base zones with 144; the seven ship / airship zones have nothing at a density the rebuild helps (their decks stay with Scenery Sharper) |
| AO High / Fog High in the look | waiting for real GPU cost |
| HDR star lift behind thick cloud; one zone family with a nested star folder | known limits of 139 |
| Retail comparison | No direct retail-program comparison (FL-1). REA export addresses are recorded; function bodies remain unread because provider preflight is blocked and the image is packed. The three config-tool defaults are pinned in 144, which does not verify their retail-binary origin. |

## Next

1. 146 is packaged and reported for QA. Continue isolated next composition550201f0 focused checks when measured
   CPU contention permits;146 and the existing publisher route stay independent.
2. Imported component gate passed22/22; validate the new composition before considering another release, preserving
   actual asset/converter/connected-activation holds.
3. Isolated FSR luma-history format diagnostic; phase count did not explain the flicker. History accumulates.
4. Extend topic defaults only after explicit safe mappings; then MSAA.
Real-GPU items stay marked pending until the owner's hardware has run them. The lane's lead is a Codex session from
2026-11-15 (release/CODEX-HANDOFF-20261115.md).
