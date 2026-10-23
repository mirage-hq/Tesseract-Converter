# Premiere Pro conversion support

[Formats](README.md) · [CLI](../../apps/tesseract-conv/README.md) ·
[Library API](../../crates/premiere_file/README.md) ·
[Linked AEP packages](../hybrid-adobe-export.md)

Both directions are partial. Import converts **one selected sequence** into one
`project.tsrct`; export creates `project.prproj` from current edited FX content.
It does not restore hidden original native records, extra timelines or full
application state. Static support does not imply keyed support or equivalent export.

FX variables have no Premiere equivalent. Export writes each linked value as the
value it reads now; variable definitions and links are not exported. A `document variables` approximation reports this flattening.

## Project selection and publication

- Plain UTF-8 XML and gzip-compressed `.prproj` files use the same graph validation.
- Discover sequences with `inspect --metadata-only`. Select the intended exact
  GUID with `--sequence`; names and list order are not selectors. Nested sequences
  count, and omission is valid only when exactly one selectable sequence exists.
- Conversion includes reachable dependencies, not a batch of independent roots.
- Output is a fresh directory with an existing parent. Check validates without
  publishing; Write reports published artifacts only after successful completion.
- Publication uses hard links where supported and exclusive-copy fallback otherwise.
  It does not overwrite existing entries or guarantee crash-atomic output.

## Timeline and nesting

| Feature | Premiere → FX | FX → Premiere / limits |
| --- | --- | --- |
| Canvas | Positive origin-based square-pixel canvases, including portrait/custom sizes. | Document canvas retained; malformed/off-origin/non-square declarations can reject. |
| Cuts, trims, moves, deletions and gaps | Current selected placement ranges and stacking become editable layers. | Current edits used; no original edit-decision replay. Root gaps can remain black and nested gaps transparent. |
| Timing precision | Native ticks round to integer milliseconds, ties forward. | Picture boundaries snap to output frames; audio retains milliseconds. A collapsed placement can omit its enclosing nest or reject. |
| Frame rate | Supported native clocks retained through bounded time projection. | Explicit 23.976/24/25/29.97/30/50/59.94/60 fps, default 30; original cadence is not automatically restored. |
| Speed, reverse and frame hold | Bounded current source selections and supported remap/hold controls. | Supported constant speed/reverse/holds; other curves can use a diagnosed constant-speed selection where safe. Not general nonlinear timing fidelity. |
| Nested sequences | Independent editable groups with bounded placement clocks and Motion/Opacity. | Supported groups become generated nests. Shared editing identity, arbitrary canvas/clock/effect combinations and retimed audio are not generally retained. |
| Empty nested sequences (template drop zones) | A nested sequence with no native members imports as an empty group that keeps its placement's Motion, Opacity and effects, with the Feature diagnostic `empty template placeholder sequence "<name>" has no source occurrences; Premiere renders it as nothing…`. Its linked audio item plays nothing and is dropped with the same diagnostic; the sequence end it set is kept. A selected empty top-level sequence becomes a one-frame empty canvas (approximation). An empty input of a Track Matte Key or of a disabled/output-off placement keeps its earlier omission. | A childless group exports, without a report, as an empty nested sequence (one empty video track) carrying the group's Motion, Opacity and effects; no audio item is written for it, so the native sequence ends with its last picture. A document with no other content (an imported empty top-level sequence) is not exported. Evidence: scripted Premiere 26.5.2 fixture `template_placeholder_nest.prproj` (`template_fixtures.provenance.json`); its direct Premiere render shows the graded V1 chart through the empty V2 nest (ran). Adobe open, save, reopen and readback of our 29.97 fps export (ran: both sequences, the empty nest and Lumetri Exposure 0.5 unchanged) and its render (ran: frames 0–59 MAD ≤ 0.03 against the native reference). |
| Track editing locks (`IsSyncLocked`, `IsLocked`) | Admitted without a report: they lock timeline editing and have no picture or sound effect. FX has no track flags, so they are not retained. Premiere 26.5.2 (`Track` Version 4) writes only non-default values (`IsSyncLocked false`, `IsLocked true`); older saves (`Track` Version 3) write both elements on every track. | Not written; Premiere applies its defaults (sync lock on, unlocked), so a non-default lock is not restored. Evidence: scripted Premiere 26.5.2 fixture `template_track_locks.prproj` (V1 sync lock off, V2 locked; native save, reopen and QE readback ran) imports with no report; reader test on edited XML for `IsSyncLocked true`. Adobe render: not applicable (no picture semantics); Adobe open of an export carrying these tracks: unrun. |
| Older-save sequence fields (`Sequence` Version 11, `VideoTrackGroup` Version 10) | Admitted without a report when they have no picture semantics: the sequence `ID` (editing identity), `FieldType` 0 (progressive), an `ImmersiveVideoVRConfiguration` with no projection or stereoscopic layout (whatever its captured/field-of-view angles), and an absent sequence component chain (no sequence-level effects). `FieldType` other than 0 and a VR projection or stereo layout are reported `nondefault … not converted`. | Not written as saved; the writer emits its current 26.5 sequence records. Evidence: reader test on edited fixture XML; local licensed corpus (Premiere 23 save, 239 sequences, each with these four fields). Adobe proof: unrun. |
| Enabled state and visibility | Supported clip state retained; track-level visibility can flatten into per-clip state. | Per-clip controls, not full original track organization. |
| Cross Dissolve | Bounded opacity/isolation mappings with supported source handles. | Canonical current forms can reconstruct native transitions; other edits retain supported pictures, not original transition identity. |
| Multicam and merged clips | Selected bounded camera cuts and separately admitted picture/sound. | Ordinary edited clips; unused cameras, multicam/merged editing state and arbitrary audio routing are lost. |

Round-trip success does not imply matching native sampling at frame boundaries.
Retime, mixed rates, nested source tails, effect clocks and coverage dependencies
have stricter combinations than plain clips.

## Media and source identity

Media is packaged from admitted current sources without silent transcoding.
Live path aliases must agree; a valid package-local or explicit relocated identity
can resolve supported stale authored paths. Changed hashes/conflicting candidates
reject rather than guessing by filename. Use the
[relocation contract](../../crates/premiere_file/README.md#explicit-media-relinking)
and [replacement-map workflow](../media-preparation.md) when needed.

| Source family | Important boundary |
| --- | --- |
| H.264 / HEVC | Supported MP4/MOV sample entries, profiles, color/layout and physical source clocks only; codec name alone is insufficient. Native HEVC export acceptance is not broadly established. |
| ProRes | Export retains admitted ProRes source bytes. Import admits the bounded `ap4h` ProRes 4444 profile through native FFmpeg, with a warning that WebCodecs playback is unavailable; other profiles are not implied. |
| QuickTime Animation / QTRLE | Requires explicit supported whole-source preparation and a source-bound map, not automatic import encoding. |
| Raster stills / PSD | Admitted image/merged-preview profiles, alpha declarations and decoded bounds; not every file an image decoder can open is a supported native placement. |
| Numbered images | Bounded consecutive source sequences and supported occurrence clocks. |
| Irregular presentation | Selected warned profiles retain original bytes and unit playback; arbitrary VFR, edits and retiming are not generally supported. |
| Audio/data streams | Consumed sound has its own strict admission. Selected unused streams do not establish sound support or permit ambiguous routing. |

Inspect reached audio/video and require `media_admission: ready` for a media-ready
handoff. Inspection can succeed with blocked sources and does not assess stills,
effects or app rendering. A prepared file must pass target-specific admission.
Experimental HDR working/output controls do not relax ordinary footage admission.

## Motion, compositing, masks and effects

| Feature family | Current mapping and key limits |
| --- | --- |
| Motion Position/Anchor/Scale/Rotation/Opacity | Supported numeric values and compatible owner-clock keys; coupled axes, spatial handles and nonuniform scale have property-specific limits. Single-axis Position export can retain the static partner; that does not admit every unpaired spatial track. |
| Blend modes | Mapped native forms and bounded staging; unsupported blends can diagnose fallback or omit an affected occurrence. FX and Adobe blend math need not match. |
| Crop / Linear Wipe | Supported mask-guide forms and bounded completion keys. Animated edges, competing masks, unsupported geometry and coverage clocks can omit the picture. |
| Opacity masks / Track Matte Key | Supported guide/provider profiles and selected animation. Mask/effect order, feather, alpha/luma behavior and dependent clocks are not generally equivalent. |
| Saved Object Mask | Admitted saved-mask samples can map to timed editable coverage; this does not restore native tracking/editing state. |
| Adjustment layers | Bounded lower-stack effects, coverage and Geometry2 staging. Invalid dependent stages can remove covered picture, not merely one effect. |
| Effect stacks | Supported order, bypass and compatible keys. Unknown decorative effects can be omitted locally; unsupported coverage-changing effects may omit their picture. Measured: Premiere 26.5.2 applies the saved chain with the highest `Index` first. |
| Geometry2 / Transform / Corner Pin | Bounded editable affine/projective mappings; curved paths, skew, retime, opacity, masks and host canvas have separate restrictions. Native identity/sampling can change. |
| Blur, shadow and glow | Mapped controls have kernel, radius, clipping and alpha differences. Matching numeric values is not a visual guarantee. |
| Color/channel controls | Selected Levels, brightness/contrast, ProcAmp (`exposure` or `channelMixer`), Gamma Correction, Tint, Invert, Ramp, Mosaic and related profiles. Lumetri Color imports as one editable `primaryGrade`, `colorCurves` (RGB Curves, Color Wheels), `hueCurves` (Hue Saturation Curves) and `vignette` (and a browsed `.cube` Look / Input LUT as `lookTransform` / source `inputTransform`) with diagnosed sections; see [Lumetri Color](#lumetri-color). Not a general Lumetri round trip. |
| Noise, grain and third-party effects | Selected native layouts/profiles only; defaults, scale, timing and plugin identity may differ. |
| Ultra Key | Narrow static import surrogate with diagnosed shader/quality limits; not general keyer fidelity or native plugin export. |

Unmapped active coverage cannot safely be treated like a decorative omission.
Read whether a diagnostic drops a property, effect, picture occurrence or whole
stage. Some precision/layout/metadata losses are not warned about. No generic
effect translator or arbitrary native plugin replay exists.

### Film Impact profiles

Pop is **disabled by default**. `--allow-film-impact-pop` consents to a bounded,
sampled static geometry approximation on supported hosts, not native plugin
preservation. Early blur/fade, pivot, curves and intersample behavior remain
approximate. Stroke has measured neutral profiles (6/99, 6/100 and 66/99); other
sizes can use a diagnosed fixed-outline approximation. Unsupported Hide Source
or opaque coverage must not expose a source that should be hidden.

### Lumetri Color

Lumetri Color (`AE.ADBE Lumetri`) is read and written against the 130-parameter
layout of the pinned `human_lumetri_contrast.xml` save; the Premiere 26.5.2 save
`color_lumetri_basic.prproj` (sequence `5c919df6-dea9-4620-9eb3-611b6fc20602`)
has the same records. The reader finds every control by `ParameterID`, not by
position, so the Premiere 22/23 layout (component version 7, body version 5,
103 parameters: no Look Color Space, HDR tone-mapping toggles, later hue curves
or LUT/Look asset slots; `Tone`/`WB Selector` names) reads the same Basic,
Creative, Curves and Vignette controls. Rows are per feature and direction. Every approximation
below is emitted as a contextual diagnostic naming the Lumetri component.

**Native export is on.** A Premiere 26.5.2 authority probe (`headless-adobe`) found
that saved Lumetri scalars drive the render (an edited Exposure scalar beside
an unchanged native Blob rendered about two stops darker), and that the `Blob`
(ParameterID 1) of a fully graded Basic/Creative/Vignette is the constant
two-byte sentinel `FE FE` (base64 `/v4=`). Premiere refuses to open a project
whose Lumetri Blob has no value, so every written Lumetri carries the Blob
sentinel, the empty-valued repeats of each binary hash and the constant
140-byte `PremiereFilterPrivateData`, exactly as the native saves store them;
neither holds grade values. One Lumetri Color is written per FX `primaryGrade`,
`vignette` or `colorCurves`, its other sections at the pinned defaults, except that a
`primaryGrade` directly followed by a `vignette` on the same layer (same
enabled state, no compositing options, static vignette) is written into one
Lumetri Color, as Lumetri applies Vignette after Basic and Creative. Import adds
an effect only for a section that changes the picture: a grade with any
non-neutral or keyed control, a vignette with a nonzero Amount whose section is
on. So `[grade]`, `[vignette]`, `[grade, vignette]` and `[vignette, grade]`
reread as themselves through our reader, with no neutral companion.
Component order is preserved. Premiere 26.5.2 applies the saved chain with the
highest `Index` first. Two native projects, authored through `headless-adobe`
with Invert and a Lumetri Color Exposure +2 added in both orders, show it:
the saved chain [Lumetri, Invert] renders an ungraded grey patch of 92 as
218.5, and [Invert, Lumetri] renders it as 111. Only highest-`Index`-first gives both
values. ExtendScript lists `clip.components` in saved `Index` order, so the
first component in that list is applied last. The writer's `chain_render_order`
convention (`schema/effects.rs`) matches this. An FX `[vignette, grade]` writes
two components (grade at `Index` 1, vignette at `Index` 2). Its render is
decoded-frame identical (60/60 frames) to the native project that applies
vignette, then grade, and it differs from the reverse order by MAD 5.15/255. A
merged FX `[grade, vignette]` equals the native grade-then-vignette render
within dither (MAD 0.07/255) and differs from the reverse order by MAD
5.15/255: inside one Lumetri Color, Basic Correction applies before Vignette, so
the merge keeps the FX order. These orders were measured with one chart still,
an Exposure +2 grade and a Vignette Amount −4, at 1920 × 1080 only.
In the CLI's automatic hybrid export, the grade and vignette scale
approximations are not routing losses: the linked After Effects route writes
the same Lumetri controls by the same scales (the native `color_ae_lumetri_basic`
and `color_lumetri_basic` renders of the same scalars differ by MAD 0.32/255),
so a graded picture keeps its native Lumetri at every sequence rate. Clamps,
unwritten keys and every other loss still route as before. A masked Lumetri
(import or effect-mask export) stays unmapped: the writer has no masked form.

Proof status: import of `color_lumetri_basic` is pinned and asserted
(structural; its reference render is the native Premiere render recorded in
`color_fixtures.provenance.json`, not a converter comparison; like the other
colour and template renders cited in this file, it is local evidence, not
committed, and no test reads it). Structural export
proof: the written component equals the native save record for record
(child-element order follows the shared writer), and it rereads through our
reader. Adobe acceptance of the single-grade form (Premiere 26.5.2, `tsrct-conv`
binary built at `b4f10f52a3`, 29.97 fps CLI export of an explicit FX document whose `primaryGrade` equals
`color_lumetri_basic`'s values, same chart still): Premiere opened, saved and
reopened the export; all 130 Lumetri controls read back equal to the native
fixture's reopened readback, and the direct render is decoded-frame identical
to `color_lumetri_basic.premiere-direct-v1.mp4` (60/60 frames, MAD 0). This
proves acceptance of these nine Basic scalars at these values only; the
mapping scales remain an approximation of FX's own renderer. The native route
at exact 24/25/30/50/60 fps (where the CLI previously linked an After Effects
composition) and the merged single-component `[grade, vignette]` form were
written after that binary. They were checked again with the `tsrct-conv` binary
built at `424a89f1e4` (sha256
`c05c10ba29814a7c433864de328ce66d8ebeea7c777c05eae51266d6d5dc0f62`) in
Premiere 26.5.2, with the same chart still. The grade-only 30 fps CLI export is
a `.prproj` with one `AE.ADBE Lumetri` and no linked AEP. Its render is
decoded-frame identical to `color_lumetri_basic.premiere-direct-v1.mp4` (60/60
frames, MAD 0). A `[primaryGrade, vignette]` export is one Lumetri Color at 30
and 29.97 fps. It reads back exactly (130 controls; only Amount differs from the
grade-only export) and renders the reference grade in the unvignetted centre
(MAD 0.28). The native route at 24, 25, 50 and 60 fps was not opened in
Premiere: Adobe acceptance: unrun.

The scales were then refitted to measured Lumetri renders (converter built at
`f065e813c5`; see the Basic Correction row below). The import→export round trip
still writes `color_lumetri_basic`'s own saved values back, so the acceptance
above carries over; an explicit FX document now maps to different saved values
than under the earlier scales. FX render of a fresh import of
`color_lumetri_basic` (`tsrct export`, release renderer built at `94971429d9`, 30
fps, first 60 frames) against `color_lumetri_basic.premiere-direct-v1.mp4`:
mean 10.54 codes, max 46 (was 29.35, max 76, with the earlier scales). This
fails the 2-code gate and is diagnosed: even the ten primaryGrade controls
fitted freely to this chart's six colours leave 2.3 codes, so primaryGrade v1
cannot pass it at any mapping.

| Feature | Premiere → FX | FX → Premiere |
| --- | --- | --- |
| Basic Correction Temperature, Tint, Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Saturation; Creative Vibrance | One `primaryGrade` (v1) per Lumetri, with compatible keys on each control. **Approximation** with scales fitted to Premiere 26.5.2 renders (single-control Lumetri probes on an in-house ramp, lossless renders, against the FX `primaryGrade` equations): Temperature/116, −Tint/100 (Lumetri's positive Tint is magenta), Exposure stops/2.6, Contrast/360, Highlights/240, Shadows/510, Whites/54, Blacks/124, (Saturation − 100)/110, Vibrance/167 (the earlier range scales /300, /300, /2.2, /50, /100 …, never fitted, put the `color_lumetri_basic` grade 29.4 codes from Premiere's render). Measured: Lumetri grades in gamma-2.4 linear light (Saturation 0 of pure red is code 134 = 0.2126^(1/2.4); a negative Exposure is exactly a stops/2.4 code gain), adds a highlight shoulder to a positive Exposure (pixels with a channel at white stay unchanged), makes Whites a white-point gain (±100 = ±1 linear stop, exactly FX exposure ±0.415) and white balance a luminance-keeping per-channel gain, and uses tone curves that keep black and white fixed. primaryGrade's code-value equations reproduce none of these, so the residual stays above the 2-code gate: mean/max 8-bit error at UI −50/+50 (Temperature/Tint ±100, Exposure ±0.5, Saturation 50/150) Exposure 1.4/3 and 5.7/32, Contrast 4.0/12 and 2.3/12, Highlights 6.2/21 and 6.1/19, Shadows 6.7/20 and 6.8/22, Whites 16.5/34 and 14.9/47, Blacks 2.6/37 and 5.2/37, Temperature 1.7/5 and 6.9/19, Tint 22.7/59 and 11.6/34, Saturation 13.3/83 and 5.0/61, Vibrance 20.1/118 and 2.0/15 (the import diagnostic states these numbers). Even all ten primaryGrade controls fitted freely to one setting leave Highlights ±50 at 2.9–3.5, Saturation 50% at 10.7 and Vibrance −50 at 14.7 codes. `color_lumetri_basic` FX render vs Premiere's: see the gate result in the proof paragraph above. | One native Lumetri Color per `primaryGrade`: inverse fitted scales and keys; a saved value rounds to 1e-6 when that reimports to the same FX value (an imported Exposure 0.5, FX 0.192307692, is written back as 0.5), else to 1e-8, Blob sentinel and constant private data, other sections at pinned defaults. **Approximation** (same scales). Adobe acceptance (single grade, Premiere 26.5.2): at 29.97 fps (binary built at `b4f10f52a3`) opened + readback equal + render identical to the native reference; at 30 fps on the native route, with no linked AEP (binary built at `424a89f1e4`), render identical to the native reference (60/60 frames), see above. Native route at 24, 25, 50 and 60 fps: Adobe acceptance: unrun. |
| Saved values beyond primaryGrade's range (Temperature beyond ±116, Tint beyond ±100, Whites beyond ±54, Blacks beyond ±124, Saturation above 210, folded Saturation) | Clamped (static and per key) with an approximation diagnostic. | FX values whose saved image lies beyond the saved range clamp with a diagnostic: Exposure beyond ±7/2.6, Contrast beyond ±150/360, Highlights beyond ±150/240, Shadows beyond ±150/510, Saturation below −100/110, Vibrance beyond ±100/167. |
| Creative Saturation | A static value folds multiplicatively into Saturation (approximation: one FX control no longer distinguishes Basic and Creative). Keyed: omitted with a diagnostic. | Written as neutral 100. |
| Section enables (Basic Correction, Creative) and effect bypass | A section that is off imports its controls neutral, with a diagnostic per non-neutral control (a grade left neutral adds no effect); bypass becomes the FX `enabled` flag. | Sections written on; bypass written from `enabled`. |
| Vignette Amount, Midpoint, Feather | FX `vignette` by a fit to After Effects 26.5 renders of the shared Lumetri plug-in (16 settings on a flat grey frame; `aftereffects_file::lumetri::vignette`): amount from a measured table of Amount, radius and feather from the ramp centre 0.31 + 0.0038·Midpoint + 0.00048·Feather and half-width Feather·(0.00201 + 0.0000367·Midpoint); static only, when Amount is nonzero. Amount 0: no effect. Section off: no effect, with a diagnostic for a nonzero Amount. Roundness: omitted with a diagnostic. (Earlier: −Amount/5, Midpoint/100, Feather/100, which darkened even the centre.) | The Vignette section by the inverse of that fit, clamped to the saved ranges with a diagnostic, Roundness 0: of the grade's Lumetri Color when the vignette directly follows a grade, else of its own. Keyed vignette controls are not written (the effect is reported not exported). The fit is a declared approximation, not a routing loss. | **Approximation**: within 1.69 mean codes over the 16 flat-grey AE settings. Premiere 26.5.2: a scripted Lumetri (Amount −2, Midpoint 40, Feather 60, headless-adobe job 11987) on the chart renders (job 11997) within 60-frame mean 4.52 (max 55) of the FX render of its fresh import (converter `f55a3583d6`); an FX vignette (0.444, 0.716, 0.385) exported (Lumetri −2, 50, 50) renders in Premiere (job 12001) within 3.34 (max 54) of FX. Earlier evidence (binary `424a89f1e4`): `[grade, vignette]` merged into one Lumetri Color at 30 and 29.97 fps read back and rendered. Native route at 24, 25, 50 and 60 fps: as before. |
| Creative Look with a browsed `.cube` (path slot 24, popup 25 = 0, Intensity 26, Look Color Space 130) | One `lookTransform` (v1) after the Lumetri's `primaryGrade`: `lut3d` from `jerboa:sdr-rec709-display:v1` to `jerboa:sdr-rec709-display:v1`, trilinear, `mix` = Intensity/100. The `.cube` is packaged as an FX asset (`premiere-lut-<n>`, one per file), found beside the project by its saved name, else at the saved absolute path, else taken from the copy Premiere embeds in `Embedded LUTs` (ParameterID 98) when the `Blob` graph's `<embeddedlut>` references it for that path (written as a `.cube` of the saved name with the same `f32` samples; only the pinned `ITX3` layout is read). The path slot decides (UTF-16LE, one NUL terminator; a slot saved empty with another slot's `BinaryHash` shares its value); the `Blob` shader graph, when present, must name the same file. Pinned evidence (`color_lumetri_look_cube`, human-authored in Premiere 26.5.2, plus edited variants rendered by Premiere 26.5.2): the native render equals the chart with the `.cube` applied to its 8-bit Rec. 709 code values once per slot (MAD 0.95/255 for both slots); a sentinel `Blob` with the embedded samples cleared still renders both LUTs; a slot without its NUL terminator, or a missing `.cube` with a sentinel `Blob`, renders none. **Approximation**: Premiere interpolates the LUT tetrahedrally at the project LUT Interpolation it saves by default (`lutInterpolationMethod` 1) while FX `lut3d` is trilinear; every imported Look / Input LUT carries that diagnostic. Measured in Premiere 26.5.2 with the in-house channel-mixing 17³ LUT (local evidence, not committed) browsed into the Look slot of an uncommitted edit of this fixture and rendered over an in-house 8-bit RGB sweep: the render sits at 0.93 between float trilinear (0) and tetrahedral (1) predictions over the 688 621 pixels where they differ by ≥ 2 codes; the same project with `lutInterpolationMethod` 0 renders trilinearly (0.02). The two interpolations differ by mean 0.70, max 13 codes on that sweep; they are equal for per-channel affine LUTs such as `color_warm_tint.cube`. Intensity above 100 clamps to `mix` 1 with a diagnostic (Lumetri extrapolates); a Look Color Space other than 28 imports as Rec. 709 with a diagnostic; Creative Vibrance/Saturation, which Lumetri applies after the Look, travel in the `primaryGrade` before it (diagnostic). **Omitted** with a diagnostic naming *Lumetri Color > Creative > Look*: a built-in Look (popup ≠ 0) or `LookAsset`, a missing or non-3D `.cube` without a referenced embedded copy (Premiere renders such a slot without the LUT when the `Blob` is the sentinel; with the native `Blob` graph the committed fixture's sanitized paths name no existing file and Premiere still renders both LUTs from the embedded copy), a slot without its NUL terminator, a `Blob` graph naming another file, keys. FX-render gate (converter built at `4af02fb391`, sha256 `193c586d…6af0`; release `tsrct` at `94971429d9` through the export queue; first 60 frames, all pixels, vs the Premiere render of the same project): fixture (Input LUT → `source.inputTransform`, Look → `lookTransform`) vs its reference **MAD 0.65/255** (max 13); Look-only variant 0.39; Input-LUT-only variant 0.39; channel-mixing Look on the RGB sweep 1.40 (max 40; 1.12 against the same project rendered with `lutInterpolationMethod` 0; Premiere's ungraded sweep floor 0.84) — all within the 2-code gate. | A `lookTransform` whose `lut3d` reads and writes `jerboa:sdr-rec709-display:v1` exports as its own Lumetri Color with only Creative > Look set: the `.cube` asset is written into the package `media` folder, its absolute path saved NUL-terminated in path slot 24 with a content-derived `BinaryHash`, popup 0, Intensity = `mix`×100, `Blob` sentinel, no embedded samples, every other control at its pinned default. **Approximation** (retained, non-routing): Premiere interpolates the LUT tetrahedrally at the exported `lutInterpolationMethod` 1 while FX is trilinear (measured, import cell); the diagnostic states it. Other encodings, a Look without a LUT, a keyed `mix` or a masked Look: not written, reported. Adobe acceptance (Premiere 26.5.2, binary built at `70a85a5097`, 29.97 fps CLI export of a fresh import of `color_lumetri_look_cube`): readback of all 130 controls equals the native save except the `Blob`, the two path slots and the embedded samples (Look path = our file, Intensity 100); its render is file-identical to the native single-LUT variant of the fixture (Input LUT removed), and `mix` 0.3 renders as the chart blended 30 % toward the LUT result (MAD 0.83/255; 0.2 or 0.4 give 1.66/1.69). Round trip of the fixture (fresh import → CLI export, converter built at `4af02fb391`; the source `inputTransform` exports as the clip Input LUT, row *Clip Input LUT*): no linked AEP at 29.97 or 30 fps; Premiere 26.5.2 renders (relink `skip_when_resolved`, which keeps the clip Input LUT) the 29.97 fps export **file-identical** to `color_lumetri_look_cube.premiere-direct-v1.mp4` and the 30 fps export **decoded-frame identical** to it (60/60, MAD 0). |
| Basic Correction Input LUT with a browsed `.cube` (path slot 4, popup 5 = 0) | The same `lut3d` as a full-mix `lookTransform` at the Lumetri's position, before its grade. When it is the media layer's first effect, enabled and without compositing options and the source has no Input Transform yet, it becomes the source `inputTransform` instead (`jerboa:sdr-rec709-display:v1` → `jerboa:sdr-rec709-display:v1`, which applies before every effect, as Lumetri applies its Input LUT first); otherwise the `lookTransform` stays, with a diagnostic. Same path-slot rules, packaging and omissions as the Look (diagnostics name *Lumetri Color > Basic Correction > Input LUT*). Built-in Input LUTs (popup ≠ 0) and `LUTAsset` remain **omitted**. | A source `inputTransform` is not written as a Lumetri Input LUT: it exports as its native equal, the clip Interpret Footage Input LUT (row *Clip Input LUT*), which Premiere applies before every effect; no picture loss is reported, so nothing routes to a linked AEP. A full-mix `lookTransform` exports as a Creative Look (row above). Adobe acceptance (Premiere 26.5.2): the fixture round trip renders identical to its reference at 29.97 and 30 fps (row above). |
| Premiere 22/23 layout (component version 7, 103 parameters) | Read by `ParameterID`: Basic/Creative scalars, Vignette and RGB Curves import as above. `HDR White` (63), `HDR Specular` (64) and `HDR Range` (65) have no FX counterpart: a value other than the saved default (100, 0, 100) is an omission diagnostic (*Basic Correction HDR controls*, *RGB Curves HDR Range*); the Color Wheels `HDR White` (66) belongs to the Color Wheels row. An empty Hue vs Sat curve (kind 1, no points, the diagonal left in its unused slots) is the identity and is not reported. Evidence: synthetic older-layout test `older_lumetri_layout_imports_basic_and_creative_scalars_by_parameter_id`; eight Lumetri components of a licensed local Premiere 23 template (243 instances, one layout; not committed), placed in an in-house project, convert with only the documented omissions, and Premiere 26.5.2 opened and rendered that project (numbers in the lane report). | Not written: export always writes the 26.5.2 layout. |
| RGB Curves (39) | One FX `colorCurves` (v1), after the grade (and after a Creative Look's `lookTransform`, in Lumetri's section order), when a curve differs from the diagonal and the Curves section is on. Layout and rendering measured in Premiere 26.5.2 on in-house byte patterns (8-clip Rec. 709 probe over an 8-bit grey ramp and hue/saturation sweeps, lossless RGB render): four 520-byte records (`u32` kind 0, `u32` count, up to 32 little-endian `(f64 x, f64 y)` points; unused slots are not read) for Luma, Red, Green, Blue in that order; coordinates are display code values 0–1; Premiere draws a natural cubic spline (zero end curvature) through the points and clamps it to 0–1: the spline predicts the render within 0.76 codes on four patterns, and Red/Green/Blue act on their channel alone (within 1 code on colour bands). **Approximation**: the spline is fitted by at most 32 straight segments within one 8-bit code; the Luma curve equals FX Master on greys but leaves fully saturated colours unchanged, so FX Master differs on coloured pixels (up to 64 codes, mean 17.9, for a midpoint raised 0.5→0.75). Section off with changed curves: no effect, diagnosed. Keyed, undecodable, or an end point off x 0 / x 1: omitted with the reason, other sections kept. Import render comparison (binary built at `1c48d3aa9d`; FX render by a release `tsrct` build at `9283dfdf95` through the export queue): the imported probe renders within 2 codes of Premiere on every pixel for the Red, Green and Blue curves (MAD ≤ 0.19) and within 2 codes on the grey ramp for every Luma curve; on coloured pixels the Luma-as-Master approximation differs by up to 64 / 35 / 24 / 117 codes (MAD 6.2 / 4.2 / 1.7 / 10.0) for the four Luma patterns. Five RGB Curves values of a licensed local Premiere 23 template, rendered in an in-house probe, follow the Luma-then-channel spline model within 0.9 codes. Because export folds a non-identity FX Master into the Red, Green and Blue curves (next cell), a Luma curve does not survive a round trip unchanged: it reimports as Master and re-exports as three folded channel curves with an identity Luma, which the import diagnostic states (test `lumetri_luma_curve_reimports_as_master_and_reexports_folded_into_the_channels`). | FX `colorCurves` → its own native Lumetri Color RGB Curves at every sequence rate, other sections at the pinned defaults, content-derived `BinaryHash` (SHA-256 digits + byte length + 12); never a routing loss, so the CLI hybrid never moves it to a linked After Effects scope. With an identity Master, Luma is written as it is and each Red/Green/Blue curve keeps its FX points when Premiere's natural cubic spline through them stays within one 8-bit code of FX's straight segments (simple curves stay editable as authored); a curve the spline would draw further off (a sharp corner overshoots, e.g. 45.70 codes for Red (0,0)(0.3,0.7)(1,1)) is **resampled**: written through at most 32 points chosen on 8-bit input codes (end codes and the FX corners first, then greedily where the spline deviates most), and its diagnostic names the curves, their deviation through FX's own points, that points were added for rendering fidelity (so Premiere no longer shows FX's points for them), and the deviation through the written points. A non-identity Master, which FX applies per channel before the channel curves, is **folded** into the channel curves: each channel `c` is written as `c(Master(x))` through at most 32 points chosen on 8-bit input codes (end codes and Master corners first, then greedily where Premiere's natural cubic spline deviates most), and Luma stays identity, because Lumetri's Luma curve leaves saturated colours unchanged (measured up to 117 codes off a per-channel Master). **Approximation** (retained native, diagnosed per export): Premiere's spline through the written points differs from FX's straight segments by the reported largest deviation on 8-bit input codes; the folded diagnostic also names the fold and the identity Luma. Tests: `edited_color_curves_export_as_native_lumetri_rgb_curves_and_reimport`, `master_folds_into_channel_curves_within_one_code_of_the_fx_polyline`, `sharp_channel_curve_is_written_through_points_premiere_draws_within_one_code`, `sharp_color_curve_exports_through_added_points_and_reimports_within_one_code` (reimported Red within one code of the FX polyline), CLI `hybrid::tests::lumetri_curves::color_curves_with_a_master_export_natively_at_30_fps_folded_into_the_channels` (30 fps: one `AE.ADBE Lumetri`, no `.aep`, reread channels within 2 codes of `c(Master(x))`). Adobe acceptance (Premiere 26.5.2): (1) **Master fold, 30 fps** — converter built at `56e038b6bf` (tsrct-conv sha256 `0c0be696…e2ee`), `--fps 30` CLI export of an in-house four-clip ramp document (no effect; Master (0,0)(0.5,0.65)(1,1) + Red line 0.1–0.9; Master (0,0)(0.3,0.7)(1,1) + three-point Blue; five-point Master + three-point Red + Green line 0.2–0.8): `.prproj` with three Lumetri and no linked AEP; reported deviations 0.84 / 0.86 / 0.97 codes; Premiere rendered it (lossless RGB, 120 frames at 30 fps) within **max 2 codes** (MAD 0.22 / 0.19 / 0.21, p99.9 1 code; untouched clip max 1, MAD 0.001) of an FX render of the same document (release `tsrct` at `94971429d9`, export queue) at frame 30k+15 of each clip, all pixels, against effect sizes of MAD 18.9 / 35.7 / 22.7. (2) **Identity Master, 29.97 fps** — binary built at `1c48d3aa9d`: an FX document reproducing the eight in-house probe clips opened and rendered decoded-frame identical to the native probe render on all eight clips (lossless RGB, MAD 0, max 0); its reported deviations (12.27, 10.69, 45.70 codes) match the measured FX-render vs Premiere-render differences of the same points (13, 11, 46 codes). That document's Master clips predate the fold (written then as Luma) and were not re-rendered with it. (3) **Identity Master, 30 fps** — straight Red 0.1–0.9 / Blue 0.2–0.8 (deviation 0): no linked AEP, Premiere render within 1 code of the exact per-channel lines (MAD 0.003). (4) **Identity Master, sharp curves resampled, 30 fps** — converter built at `434a48a7f0` (tsrct-conv sha256 `ae1f4b9c…7734`), `--fps 30` CLI export of an in-house four-clip ramp document (no effect; three-point Red (0,0)(0.3,0.7)(1,1); three-point Green (0,0)(0.5,0.25)(1,1); the same Red + Blue line 0.1–0.9): `.prproj` with three Lumetri and no linked AEP; own-point deviations 45.70 / 12.27 / 45.70 codes reported and resampled to 0.59 / 0.62 / 0.59 codes, the Blue line kept verbatim; Premiere rendered it (lossless RGB, 120 frames at 30 fps) within **max 2 codes** (MAD 0.11 / 0.07 / 0.16, p99.9 1 code; untouched clip max 1, MAD 0.001) of an FX render of the same document (release `tsrct` at `94971429d9`, export queue) at frame 30k+15 of each clip, all pixels, against effect sizes of MAD 12.1 / 10.5 / 16.7 (the same points written verbatim measured 46 / 13 codes off, proof (2)). At 24, 25, 50 and 60 fps: Adobe acceptance: unrun. |
| Hue Saturation Curves (42/106/108/110/112) | One FX `hueCurves` after the RGB Curves' `colorCurves`, when a curve has points and the Curves section is on. Each curve is a 520-byte record of `(x, y)` points (kind 3 for the hue-keyed curves, kind 2 for Luma vs Sat and Sat vs Sat, as Premiere 26.5.2 saves them), `y` a signed adjustment, 0 neutral. Measured in Premiere 26.5.2 (16 in-house curves on an 8-bit ramp with saturated, pastel and saturation-sweep rows, headless-adobe render jobs 12013/12014; and the wave-5 probe): monotone cubic Hermite interpolation (Fritsch–Carlson; periodic for the hue curves, flat ends otherwise); Hue vs Sat, Luma vs Sat and Sat vs Sat scale gamma-2.4 linear-light chroma by `1 + y` at constant linear luminance; Hue vs Hue turns display chroma about the grey axis by `−180°·y`; Hue vs Luma adds `y` times the display luma (greys unchanged); the hue curves key on the HSV hue of linear light, Luma vs Sat on display Rec. 709 luma, Sat vs Sat on the display chroma length. This model is within 0.03–0.25 mean 8-bit codes of Premiere for each saturation and hue curve (Hue vs Luma 1.0–1.6). FX `hueCurves` keys on display HSV and keeps display luma, so the conversion matches the two at reference colours (HSV saturation 0.65, value 0.75; twelve hues for the Luma vs Sat and Sat vs Sat curves) and fits at most 16 linear points within 0.004. **Approximation**: the fresh import of the 16 probe curves rendered by FX (converter `793864bcf2`) is a mean 9.1 codes from Premiere's renders against a mean effect of 16.4 (per curve 0.7–17.5 against 1.3–32.8, most on fully saturated colours; two small bumps on saturated blues end 0.4–0.7 codes above their effect). **Omitted** with a diagnostic: points under another kind (Premiere 22/23 saves Hue vs Sat as kind 1), a single off-neutral point, keys; section off: diagnosed. Tests: `lumetri::hue_curves::tests` (aftereffects_file), `written_lumetri_hue_curves_and_wheels_are_their_own_components_and_reread_exactly`. | A fresh Lumetri Color whose Hue Saturation Curves carry the FX curves by the inverse matching (at most 32 points of Lumetri's spline within 0.004 of the matched values). **Approximation, declared** (not a routing loss). Premiere 26.5.2 opened and rendered a chart export (a Hue vs Sat dip on greens, a Hue vs Hue turn of reds, Luma vs Sat lowering shadow saturation; job 12056, converter `793864bcf2`) within a 60-frame mean of **2.78** (max 27) of the FX render, against an FX effect of 7.67. Test: `hue_curves_and_tonal_color_export_as_lumetri_sections_and_reimport`. |
| Color Wheels & Match (47/66/101/103) | One FX `colorCurves` (v1, identity Master), after the RGB Curves' `colorCurves`, when the Shadows, Midtones or Highlights wheel is moved and the section is on. Layout measured in Premiere 26.5.2 (in-house probe): 96 bytes = four little-endian `(f64 hue angle in degrees, f64 luma slider, f64 amount)` triples, neutral `(0, 0.5, 0)`, for Shadows, Midtones, Highlights and a fourth wheel that is inert in the default three-wheel mode. Rendering measured on 63 in-house wheel settings (8-bit grey ramp with saturated, pastel and warm rows, Rec. 709 sequence, lossless renders): every coloured pixel equals the grey ramp's output at its channel code, so the wheels are three per-channel transfers. With `c(hue) = hsv(hue, 1, 1) − Rec. 709 luma`: Shadows lift `L = amount/2·c + (luma − 0.5)/2`, `y = x + L(1 − x)` (a negative lift ramps in from black over a fitted width); Midtones power `E = 1 − amount·c − (luma − 0.5)`, `y^E`; Highlights gain `G = 1 + amount/2·c + (luma − 0.5)/2`, `y·G` (a gain above 1 ramps out towards white over a fitted width). **Approximation**: the fitted model is at most 3.2 8-bit codes from Premiere (mean 0.29) over the 63 settings; each channel is written as at most 32 straight segments within one code of the model, through which Premiere's RGB Curves spline also stays within one code; the wheels are no longer editable as wheels. Section off with moved wheels: no effect, diagnosed. **Omitted** with a diagnostic: a changed wheel-mode checkbox (101/103; one-wheel and four-wheel modes were not measured), a changed HDR White (66), keys, a midtones power below 0.2 (outside the measured range). Fixture `lumetri_color_wheels_probe` (eight in-house wheel settings, Premiere 26.5.2 scripted host + offline payload patch, provenance JSON; reference `tests/references/premiere/color/lumetri_color_wheels_probe.premiere-direct-v1.mp4`). Tests: `color_wheels_probe_imports_each_clip_as_channel_curves_that_follow_premiere_renders` (decoded triples; every imported channel within 4 codes of the Premiere render at seven codes; each layer's curves export as native RGB Curves with identity Luma), `lumetri_color_wheels_follow_rgb_curves_and_respect_the_wheel_mode`. Import render comparison (converter built at `7f71c2e6b3`; FX render by the release `tsrct` at `94971429d9`, export queue, ProRes): the fresh import of the fixture renders within **max 4 codes** of Premiere's render of the fixture at frame 30k+15 of each of the eight clips, all pixels (MAD 0.06–0.53, p99.9 ≤ 3), against effect sizes of MAD 9.2–22.8. AE Lumetri wheels: not read (see the AE ledger). | FX `colorCurves` imported from wheels exports as Lumetri RGB Curves (row above), never as wheels. Adobe acceptance (Premiere 26.5.2, converter built at `7f71c2e6b3`): `--fps 30` CLI export of the fresh import of `lumetri_color_wheels_probe`: `.prproj` with eight Lumetri RGB Curves and no linked AEP (reported spline deviations 0.65–0.97 codes); Premiere rendered it within **max 3 codes** (MAD 0.06–0.30, p99.9 ≤ 3) of its render of the original wheels and within 2 codes (MAD ≤ 0.51) of the FX render, all pixels, frame 30k+15 of each clip. FX `tonalColor` exports as Lumetri Color Wheels: each channel's lift, gain and power (the measured model) is least-squares fitted to `tonalColor`'s output on a grey ramp and 720 HSV colours, then written as the Shadows, Midtones and Highlights wheels (hue, luma, amount; luma or amount outside 0 to 1: not written, reported). **Approximation, declared**: `tonalColor` shifts chroma by luma window with a gamut fit that vanishes at black and white, while the wheels act per channel. Premiere 26.5.2 rendered a warm-shadows, green-midtones, cool-highlights chart export (job 12074, converter `ad6ea39269`) within a 60-frame mean of 6.57 (max 25) of the FX render against an FX effect of 7.83, a small gain over omitting it. Keys: not written. At 24, 25, 50 and 60 fps: Adobe acceptance unrun. |
| HSL Secondary (key 73, correction 85 and scalars 70–93) | **Omitted** with a diagnostic when changed; no qualified `compositingOptions`. Key layout measured in Premiere 26.5.2 (32 in-house key values on an in-house HSL chart, lossless renders; 24 of them with a Saturation 0 correction, measured as the retained chroma): 40 bytes = a leading byte that changed nothing in three paired probes, then Hue, Saturation and Lightness, each as little-endian `(f32 centre, f32 range, f32 softness)` in 0–1 plus an enable byte. Measured gap to the FX qualifier (v1: HSV hue/saturation, Rec. 709 luminance, smoothstep shoulders): (1) Saturation and Lightness select **HLS** saturation and lightness (Lightness centre 0.25 selects HLS lightness 0.20–0.30 at every hue; Saturation centre 0.25 selects HLS saturation 0.20–0.30 at lightness 0.5, where HSV saturation is 0.33–0.46), so no FX range reproduces them on general content; (2) a soft hue edge falls near-linearly from a cusp at the centre (centre 0, range 0, softness 0.2: 0.84 at 3°, 0.60 at 18°, 0 at about 69°), where FX's smoothstep shoulder is about 0.1 of the matte higher at a fifth of its width; (3) the range/softness-to-width mapping is not linear (range 0.3 + softness 0.1 selects a hard ±33°, range 0.1 without softness selects nothing). The 192-byte correction (85) and the refine scalars were not decoded. Test: `hsl_secondary_hue_key_is_omitted_with_the_measured_qualifier_gap`. Adobe proof of the measurement: probe renders ran (lane W evidence, outside git); no import/export mapping, so no acceptance render. | A qualified effect is not written as HSL Secondary (linked route); the same measured gap applies. |
| HDR White/Specular/tone controls, Faded Film, Creative Sharpen, Shadow/Highlight Tint, Tint Balance, embedded media LUT | **Omitted** with a diagnostic when changed; no primaryGrade equivalent. | Written at pinned defaults. |
| Creative Look / Basic Correction Input LUT set to a built-in LUT (popup) | **Omitted** with a diagnostic: Premiere 26.5.2 saves only the popup index for a built-in LUT (scripted probe, headless-adobe job 11958: Look popup 10, no path, no embedded samples, `Blob` the `FE FE` sentinel); the LUT ships with Premiere, so its samples are not in the project. After Effects saves only the name (see the AE ledger). | — |
| FX `vibrance`, `temperatureTint` (static) | — | One Lumetri Color each, through the equivalent `primaryGrade` and its fitted scales: `vibrance` Vibrance/Saturation /100 are `primaryGrade` vibrance/saturation; `temperatureTint`'s additive channel offsets are matched at mid grey by `primaryGrade` gains (0.62 per 100). Keyed effects are not written (reported). A clamped value is a routing loss as for `primaryGrade`. **Approximation, declared** (not a routing loss). Premiere 26.5.2 opened and rendered each export (headless-adobe jobs 11999–12000; converter `f55a3583d6`; chart, 60 frames) at mean 1.64 (`vibrance` 50 / saturation −30) and 6.99 (`temperatureTint` 60 / −40) codes from the FX render. |
| ProcAmp (Brightness, Contrast, Hue, Saturation) | Measured in Premiere 26.5.2 (12 in-house settings on the 8-bit ramp; headless-adobe jobs 12016 author, 12020 render): BT.601 Y'CbCr on display codes, luma × Contrast/100 + Brightness/100 and `(Cb, Cr)` turned by Hue degrees (red towards yellow) and scaled by Saturation/100, an affine display-RGB transform (within 0.25 mean codes of every probe, the floor of a free affine fit). Hue 0 with Contrast equal to Saturation: an FX `exposure` (stops log2(Contrast/100), offset Brightness/100; Brightness keys kept). Any other setting: the FX `channelMixer` of that transform (static; beyond the mixer's ±2 weights: omitted with a diagnostic). Split view on: omitted. The fresh import of all 16 probe clips (12 ProcAmp, 4 Gamma Correction) rendered by FX (converter `cd2e2f8cd8`) is within 0.18–0.67 mean codes of Premiere's render, against effects of 10.7–83. Alpha handling: unmeasured. Tests: `proc_amp_imports_as_exposure_or_as_its_exact_channel_mixer`, `proc_amp_brightness_imports_editable_offset_on_the_source_clock`, `proc_amp_matrix_decomposes_back_to_its_controls`, `written_proc_amp_and_gamma_correction_reread_exactly`. | FX `exposure` (static): ProcAmps with Contrast and Saturation `100·2^(stops/n)` (n = 1, or the stops rounded up above one stop, as Contrast stops at 200), the last with Brightness `100·offset`, then a Gamma Correction (next row) for a gamma other than 1; exact but for the Gamma rounding. Premiere 26.5.2 renders (converter `793864bcf2`): 0.6 stops, offset 0.05, gamma 1.25 (job 12058) within 0.36 (max 13) of FX against an FX effect of 64.9; 1.5 stops as two ProcAmps (job 12059) within 0.20 against 94.9. FX `channelMixer` in ProcAmp's form (as a ProcAmp imports): that ProcAmp, exactly. FX `hueSaturation` (no Colorize, static): ProcAmp Hue, Saturation `100 + saturation` and Brightness `lightness` at Contrast 100. **Approximation, declared**: ProcAmp turns and scales BT.601 chroma at constant luma, `hueSaturation` HSV hue and saturation at constant value; Premiere rendered hue 20, saturation −30, lightness 5 (job 12060) within 11.8 (max 63) of FX against an FX effect of 26.5 (a least-squares fitted ProcAmp generalised worse). Keys and Colorize: not written (reported). Test: `static_exposure_exports_as_proc_amp_and_gamma_correction_and_reimports`, `static_hue_saturation_exports_as_proc_amp_and_reimports`. |
| Gamma Correction (Gamma 1–28) | An FX `exposure` gamma of 10/Gamma: Premiere 26.5.2 raises display codes to Gamma/10 (Gamma 1, 5, 15, 25 on the grey ramp, fitted exponents within 0.007); the private data repeats the whole Gamma. Import within 0.30–0.43 mean codes of Premiere (effects 22–76). Keys: omitted. | Written after the ProcAmps of an FX `exposure` whose gamma is not 1, as the nearest whole Gamma (FX gamma 2.2 becomes Gamma 5, FX gamma 2.0, reported). |
| `channelMixer` (other than ProcAmp's form), `selectiveColor` | — | No equal native Premiere effect: the Premiere 26.5.2 video-effect list (138 effects, QE DOM, headless-adobe job 11964) has no Channel Mixer, Selective Color, Color Balance or Hue/Saturation; its "Channel Mix" (Film Impact) selects a source per channel rather than mixing weights (control list, job 11980). Not written (linked route). ProcAmp holds only a channelMixer of its own BT.601 luma/chroma form (row above). |

## Text, graphics and captions

Supported Type-tool and saved graphic content remains editable, including selected
Text/Shape objects, transforms, ordered paints and compatible Source Text keys.
Source Graphic and linked-AEP content have separate native clocks and identities.
Supported static saved capsule bindings are not a full MOGRT/controller engine.

- Uniform text and selected complete-line style blocks have bounded mappings.
  Inline mixed styles, layout, selectors and keyed mixed topology are not general support.
- Text fields can retain held changes; this is not arbitrary continuous rich-text animation.
- Font identity, availability and embedded bytes are different. Imported names do
  not obtain fonts; export needs a usable identity. Install licensed fonts in Adobe.
- Box/point baselines, leading, stroke placement, shadows and glyph metrics can differ.
- Box text that shrinks to fit (`boxFit: "shrink"`) has no Premiere equivalent.
  Export omits the text layer with a diagnostic; a graphic capsule keeps the
  authored size with a warning.
- Supported caption cues and backgrounds are partial. Full track styling, language
  behavior, app metadata and unsupported presentation can be lost.

## Audio

Supported sound retains separate placements, trims, mute, gain and bounded fades.
Selected source/clip/track/master controls flatten into editable sound; native
routing, proxy/merged editing identity, inserts, channel layouts and nonlinear
transitions are not all preserved. Unknown/unsafe routing can omit sound without
removing independent picture. Retimed/nested sound has tighter limits than plain
clips. RGB video evidence does not establish audio or synchronization fidelity.

## Color policy and HDR

Import emits legacy FX color policy, except for a sequence whose Color Setup
working colour space is ACEScct, which imports as the FX ACES policy (see
[ACES colour management](#aces-colour-management-acescct-color-setup)). Export
accepts validated legacy policy with optional experimental `--hdr pq|hlg`
working/output targets, and an FX ACES document as an ACEScct sequence.
Point-color conversion, 8-bit native paint quantization and settings/readback do
not establish HDR footage, effect/interpolation or high-precision rendering. See
the [FX policy](../../crates/fx_schema/src/color_management.rs) and
[HDR limits](../after-effects-support.md#hdr-conversion).

### Clip colour interpretation and HDR sequence settings

| Feature | Premiere → FX | FX → Premiere | Evidence |
| --- | --- | --- | --- |
| Sequence graphics white | An admitted PQ/HLG sequence writes its `graphicsWhiteLuminance` (100–1000) as `hdrGraphicsWhiteNits`; HDR Graphic points decode against that white. An SDR sequence showing an HDR Graphic keeps the FX default 609. | `hdrGraphicsWhiteNits` (default 609) becomes project and sequence `graphicsWhiteLuminance` under explicit `--hdr`, saved as a JSON integer when integral (`203`, as Premiere saves it). | Premiere 26.5.2 `color_hdr_pq_sequence.prproj` (sequence `5f9c5055-…`, white 203): fresh import gives 203; our `--hdr pq` export of that import writes the native sequence and project colour JSON. Premiere 26.5.2 `color_hdr_hlg_sequence.prproj` (sequence `45238be6-…`, Rec.2100 HLG, white 203, Lumetri Exposure +1): the same import/export assertions (`native_hlg_sequence_imports_its_graphics_white_and_exports_the_native_sequence_colour_fields`). Native 203 equals the host default for both transfers, so these cases discriminate 203 from FX 609 only. |
| Rec.2100 sequence target | A Rec.2100 working space admits with or without `OutputColorSpace`, and with or without `ToneMappingAlgorithm` / `GamutCompressionAlgorithm`: Premiere 26.5.2 saves a natively authored PQ or HLG sequence without all three (its defaults). A Rec.2100 sequence created from a clip also saves `ToneMappingKnee`, `0.20000000298023224` (PQ) or `0.18000000715255737` (HLG); that transfer's value is not reported, any other value (or any knee on an SDR sequence) is reported as `nondefault ToneMappingKnee not converted`. `ToneMappingDesaturation` `0.5` (the Premiere 26.5.x saved default) is not reported; any other value is reported as `nondefault ToneMappingDesaturation not converted`. | Every native PQ sequence field is written with the native value (`ColorManagementSettings`, `WorkingColorSpace`, `WorkingColorSpaceConfiguration`, `AllowLinearCompositing`, `AutoInputGamutCompressionEnabled`, `ToneMappingDesaturation` `0.5`, `IsGraphicsWhiteSameAsProject`, `IsColorAwareEffectsEnabledSameAsProject`). The writer also keeps its earlier explicit `OutputColorSpace` (BT.2100 PQ/HLG 10-bit, peak 1000), `ToneMappingAlgorithm` `5` and `GamutCompressionAlgorithm` `1`, which the native sequence omits. | Structural comparison with the native fixture. Adobe acceptance of a `--hdr pq` export (graphics white 203, one Lumetri): Premiere opened and reopened it, reporting working space `Rec. 2100 PQ` and graphics white 203, and rendered it through the native ProRes 422 HQ PQ route (10-bit, BT.2020/SMPTE 2084, admitted). A fresh import of `color_hdr_pq_sequence` exported with `--hdr pq` (binary built at `424a89f1e4`) renders through the same ProRes 422 HQ PQ route decoded-frame identical to `color_hdr_pq_sequence.premiere-direct-pq-v1.mov` (60/60 frames, 10-bit MAD 0), so the explicit output fields give the native picture for this case. HLG (tsrct-conv sha256 `e1894563…6482128` built at `94971429d9`, Premiere 26.5.2): a fresh import of `color_hdr_hlg_sequence` exported with `--hdr hlg --fps 30` renders through the native ProRes 422 HQ HLG route (white 203) decoded-frame identical to `color_hdr_hlg_sequence.premiere-direct-hlg-v1.mov` (60/60 frames, 10-bit MAD 0). Output colour space itself is not readable through ExtendScript. |
| HDR delivery target | FX has no HDR target field. Import reports a hint naming `tsrct export --hdr pq` or `--hdr hlg`. | `--hdr pq\|hlg` stays an explicit export option. | Structural only. |
| HDR source media | 10-bit BT.2020 HLG/PQ HEVC with Premiere's saved source profile keeps its bytes in SDR and PQ/HLG sequences; preparation byte-copies HDR sources and rejects remux/encode. | The same bytes and source profile are written. | Pinned HLG HEVC fixture. Premiere 26.5.2 `color_hdr_pq_media.prproj` (sequence `20d47a25-…`) and `color_hdr_hlg_media.prproj` (sequence `a24bd2f3-…`): Rec.2100 sequences created from the in-house HEVC Main 10 clips `color_hdr_pq_clip.mp4` (ST 2086 + CTA-861.3 metadata) and `color_hdr_hlg_clip.mp4`; fresh import keeps the clip bytes and reports the pass-through, `--hdr` export packages byte-identical media with the native source profile and working space (`native_pq_media_sequence_imports_the_hevc_pq_clip_and_exports_its_bytes_unchanged`, `native_hlg_media_sequence_…`); preparation byte-copies both clips (`media_transcode` `hdr_preparation_copies_the_pinned_pq_and_hlg_hevc_clips_unchanged`). Adobe acceptance (tsrct-conv sha256 `e1894563…6482128` built at `94971429d9`, Premiere 26.5.2): Premiere's ProRes HDR render of each native sequence matches its source clip (10-bit luma MAD 0.17 PQ / 0.33 HLG, max 2 / 1), and our `--hdr pq` / `--hdr hlg --fps 30` export of each fresh import renders decoded-frame identical to that native render (60/60 frames, 10-bit MAD 0). |
| Clip Color Space Override | In an ACEScct sequence, an override naming a camera that the FX ACES pipeline converts imports as `source.inputTransform` `cameraInput` (see [ACES colour management](#aces-colour-management-acescct-color-setup)). Otherwise, without an Input LUT, not converted: legacy FX has no input colour space without a LUT (`cameraInput` is ACES-only). A feature report names the override and the name-mapped camera encoding (table below), so a vendor LUT can be added as `source.inputTransform`; original bytes and placement are kept. Rec.709 on an SDR source, and Rec.2100 equal to the source profile, add no report (FX already decodes the clip that way). Rec.709 on an HLG/PQ source is reported, not converted: Premiere shows the HDR bytes as Rec.709 while FX decodes them as BT.2100, so the picture differs; `IsColorSpacOverridden` `false` keeps a saved label inactive. With an Input LUT, a mapped override becomes the LUT's `inputEncoding` and is not reported. | A `lut3d` from a camera encoding in the table into `jerboa:sdr-rec709-display:v1` writes the media's `OverriddenColorSpace` `{"baseColorProfile":{"colorProfileName":<label>},"baseProfileType":1}` and `IsColorSpacOverridden` `true` together with the clip Input LUT (next row). If the `.cube` asset cannot be read, the override alone is written and the LUT is reported lost (Premiere's own camera-log conversion replaces it). A still writes no override (the pinned grammar is a video stream's): its LUT is written and the override reported. Clips sharing a media share its first written interpretation (override and Input LUT); a differing clip is reported. | Premiere 26.5.2 `color_clip_colorspace_override.prproj` (sequence `ba91dc24-…`, Rec.709 H.264 overridden to "Sony S-Log3/S-Gamut3.Cine"): fresh import report and generated-field equality with the native stream (`camera_lut_input_transform_exports_the_native_override_and_input_lut_grammar`). Rec.709 on an HLG/PQ source: reader test on a mutated pinned stream (`clip_rec709_override_on_an_hdr_source_is_reported`); no native HDR-source override fixture. Override + Input LUT together: no native fixture (both grammars are pinned separately); the LUT-reads-the-override-encoding order is unproven. Adobe render (tsrct-conv sha256 `e1894563…6482128` built at `94971429d9`, Premiere 26.5.2): our `--fps 30` export of the fresh import (override not written) and our 29.97 fps export of an FX `lut3d` from `camera:sony-slog3-sgamut3-cine:v1` (override written, before the Input LUT was also written) both open and render identical to `color_clip_colorspace_override.premiere-direct-v1.mp4`, but that reference is itself YUV-identical to the decoded source clip: the override is not visible in the direct render route, so the render cannot discriminate it (structural only). Override + Input LUT export: Adobe render unrun. |
| Clip Input LUT | Premiere 26.5.2 saves a browsed Input LUT as a `LUTAsset` record (`Id` GUID, `Displayname`, `Filename`, base64 `Blob`) named by the media `VideoStream`'s `InputLUTAsset`, with `InputLUTSpecified` `true`, `InputLUTID`, `InputLUTDisplayName`, `InputLUTFileName`; no file path is saved. The `Blob` is an `ITX3` table: `ITX3`, `u32` edge, `u32` 3, `u32` 0, `f32`×3 domain min, `f32`×3 domain max, then edge³ RGB `f32` in `.cube` row order (red fastest), little-endian. Import decodes it into `source.inputTransform` `lut3d` (trilinear, output `jerboa:sdr-rec709-display:v1`) of every Image/Video layer of that media and packages the samples as a `.cube` asset `premiere-input-lut-<GUID>` (shortest round-trip `f32` text, so the table is exact); the input encoding is the camera encoding of a mapped Color Space Override, Rec.709 display for a Rec.709 override, else `jerboa:sdr-rec709-display:v1` with the approximation `Input LUT source encoding is unknown`. Older streams with only `InputLUTSpecified` / `InputLUTID` (no `LUTAsset`), and unknown `ITX3` layouts, are reported and the picture keeps converting without the LUT. | A `lut3d` into `jerboa:sdr-rec709-display:v1` whose `.cube` asset parses as a 3D LUT writes the same records (samples re-encoded as `ITX3`; GUID derived from the samples) and the `.cube` file beside `project.prproj`; no loss is reported when its input is a mapped camera encoding (plus override) or Rec.709 display. Another input encoding writes the LUT and reports that Premiere is not told the input. Another output encoding, or an unreadable asset, writes no LUT and is reported; `cameraInput` writes the equal Color Space Override in an ACES document (see [ACES colour management](#aces-colour-management-acescct-color-setup)) and is reported in a legacy one. | Premiere 26.5.2 `color_clip_input_lut.prproj` (human-authored in the UI, sequence `302bb7b5-…`, `color_warm_tint.cube` on `a1_bg_chart.png`): fresh import → lut3d with the browsed file's rows (`native_clip_input_lut_imports_as_the_source_lut3d_with_its_embedded_samples`); export of that import → `LUTAsset` samples byte-equal to the native `Blob` (`source_lut3d_exports_the_native_clip_input_lut_grammar`). Adobe: see the acceptance note below the table (open, readback and renders ran). Premiere 25.6.6 stream with only `InputLUTSpecified=true` stays reported. **Approximation** (both directions, diagnosed): Premiere interpolates the clip Input LUT tetrahedrally too (render at 0.93 between the trilinear and tetrahedral predictions with the channel-mixing LUT), FX trilinearly. |

The native Premiere renders named in this table (the `.premiere-direct-*.mov`
HDR ProRes renders) are local evidence, not committed: their SHA-256 and
requests are in `color_fixtures.provenance.json`, and no test reads them.

SDR pictures in a Rec.2100 sequence (accepted design difference): Premiere
26.5.2 decodes SDR media with BT.1886 (gamma 2.4) and converts HLG through the
BT.2100 inverse OOTF; the FX renderer decodes it with the sRGB curve and writes
HLG without an OOTF. Both put SDR white at the graphics white. Measured without
a grade: 3.6 (PQ) / 5.7 (HLG) mean 8-bit codes, mostly in dark tones. A Lumetri
grade in such a sequence can lift SDR content above graphics white; FX
`primaryGrade` stops at graphics white (accepted limit).

Clip Input LUT Adobe acceptance (Premiere 26.5.2). Premiere's relink
(`changeMediaPath`) resets the clip Input LUT to none, so renders use the
headless-adobe opt-in `relink: "skip_when_resolved"`, which opens a project whose
saved `ActualMediaFilePath` already names the pinned media and never relinks; the
native fixture renders from an uncommitted copy whose saved media path is the
absolute path of the committed chart (nothing else changed). Native render
(local evidence, not committed; adobe job 11548, `getInputLUTID()` equal to the saved GUID at open, before and
after export): graded, and file-identical to the renders of the Look-only and
Input-LUT-only variants of `color_lumetri_look_cube` (one application of the
`.cube` to the Rec. 709 display code values). FX-render gate (converter built at
`4af02fb391`, sha256 `193c586d…6af0`; release `tsrct` at `94971429d9` through the
export queue): the fresh import rendered by FX vs that native render, first 60 frames,
all pixels: **MAD 0.39/255** (max 13). Export: our 29.97 fps CLI export of that
import (`LUTAsset` with our GUID) renders **file-identical** to the native render and
the 30 fps export **decoded-frame identical** (60/60); Premiere reads our GUID back
before and after export. An FX document whose `source.inputTransform` is the
channel-mixing 17³ LUT exports (30 fps) as a clip Input LUT that
Premiere renders tetrahedrally (0.93) on the RGB sweep, MAD 1.40 (max 38) from the
FX render of the same document. A default (relinking) render of either project is
ungraded and is not a reference.

Name-level mapping of Premiere override labels to Tesseract camera encodings
(curve and gamut named alike; no per-label colour measurement):

| Premiere Color Space Override | Tesseract camera encoding |
| --- | --- |
| Apple Log/Rec. 2020 | `camera:apple-log:v1` |
| ARRI LogC3/Wide Gamut3 | `camera:arri-logc3:v1` |
| ARRI LogC4/Wide Gamut4 | `camera:arri-logc4:v1` |
| Canon Log3/Cinema Gamut | `camera:canon-log3-cinema-gamut:v1` |
| Sony S-Log3/S-Gamut3 | `camera:sony-slog3-sgamut3:v1` |
| Sony S-Log3/S-Gamut3.Cine | `camera:sony-slog3-sgamut3-cine:v1` |
| Panasonic V-Log/V-Gamut | `camera:panasonic-vlog:v1` |
| Red Log3G10/Wide Gamut | `camera:red-log3g10:v1` |

F-Log (not F-Log2), D-Log (not D-Log M) and Canon Log2 have no mapped encoding.
Premiere 26.5.2 also offers "Nikon N-Log/Rec2020", "Fuji F-Log2/Rec. 2020" and
"Fuji F-Log2/F-GamutC"; they are not mapped to `camera:nikon-nlog:v1` or
`camera:fujifilm-flog2:v1` yet, so they are reported without an encoding.
Blackmagic Film Gen5 and DJI D-Log M are not offered. Only
"Sony S-Log3/S-Gamut3.Cine" is pinned as a saved override; the other labels are
spelled as in Premiere 26.5.2's offered override list.

Still approximate or unconverted: tone mapping and gamut compression (the
generated `5`/`1` controls or Premiere's omitted defaults admit; neither
converts), tone-map desaturation, auto tone-map/log colour management, HDR
Lumetri controls, linear compositing, and HDR rendering precision. An admitted
white is not proof of matching Premiere tone/gamut output.

### ACES colour management (ACEScct Color Setup)

Premiere 26.5.2 offers the sequence working colour spaces Rec. 709, Rec. 2100 HLG,
Rec. 2100 PQ and ACEScct (`workingColorSpaceList`, settable by script) and saves an
ACEScct sequence on its `VideoTrackGroup` as `WorkingColorSpace`
`{"baseColorProfile":{"colorProfileName":"ACEScct"},"baseProfileType":1}`,
`WorkingColorSpaceConfiguration` id `ACEScct` (not linearized),
`AutoInputGamutCompressionEnabled` `true`, `AllowLinearCompositing` `false`,
`ToneMappingDesaturation` `0.5` and the host-default `ColorManagementSettings`
(`autoToneMapEnabled` `true`); it saves no `OutputColorSpace` for the default
Rec. 709 output and no tone-mapping or gamut-compression algorithm. Auto Tone
Mapping and Auto Input Gamut Compression are scriptable sequence settings and
persist; the output colour space has no setter. Clip input colour spaces are
Interpret Footage Color Space Overrides (row above); the offered list includes
ACEScct, Apple Log, ARRI LogC3/LogC4, Canon Log/Log2/Log3, DJI D-Log, Fuji F-Log /
F-Log2, Leica L-Log, Nikon N-Log, Panasonic V-Log, RED Log3G10, Sony S-Log/S-Log2/
S-Log3 (S-Gamut3, S-Gamut3.Cine), Rec. 2020/2100 and their (Scene) variants,
P3-D65 PQ/HLG, Rec. 601/709 and sRGB.

| Feature | Premiere → FX | FX → Premiere | Fixture / tests | Adobe proof |
| --- | --- | --- | --- | --- |
| ACEScct working colour space | Imports as `colorManagement` `acescct` / `aces2.0` / `footageOnly` (graphics and text stay display-referred, as Premiere's SDR inputs round-trip display-preserving), with an approximation naming the different output rendering. A non-pinned configuration, linear compositing, Auto Tone Mapping or Auto Input Gamut Compression off, a saved tone-mapping or gamut-compression algorithm, or an unknown output colour space is reported; the working space still imports as ACES. A Rec. 2100 output colour space adds a `tsrct export --hdr` hint. A nested sequence's own working space does not change its document. | An FX ACES document writes an ACEScct sequence with the native field texts above and no `OutputColorSpace`; `wholeProgram` / `perClip` are reported (Premiere has no per-layer colour domain). With `--hdr pq\|hlg` the Rec. 2100 output colour space and generated tone-mapping fields are written and reported (no native fixture for that route). | `color_aces_pr_sequence.prproj` (sequence `866909df-…`); `color_aces::native_aces_sequence_imports_as_an_fx_aces_document_with_a_camera_input`, `color_aces::fx_aces_document_exports_the_native_acescct_sequence_and_camera_override`, `convert::tests::still::color_management_aces_exports_an_acescct_sequence` | Ran: our `--fps 30` CLI export of the fresh import renders the chart (ACEScct + Lumetri) decoded-identical to the native reference (frames 0–59, MAD 0). |
| Clip camera input | In an ACEScct sequence, an override naming a camera of the FX ACES pipeline (`fx_conv::aces`: Apple Log, ARRI LogC3/LogC4, Canon Log3, Panasonic V-Log, RED Log3G10, Sony S-Log3 S-Gamut3 / S-Gamut3.Cine) imports as `source.inputTransform` `{"type":"cameraInput"}`; a camera FX ACES cannot convert, or any override in a legacy sequence, is reported. An Input LUT keeps precedence (`lut3d`). | `cameraInput` writes the equal Color Space Override on the clip's media in an ACES document; a still, a camera without a Premiere label, or `cameraInput` in a legacy document are reported. | same | Ran: Premiere opens our export and reads the override back exactly like the native fixture (`getOverrideColorSpace` "Sony S-Log3/S-Gamut3.Cine", jobs 11632/11633). Render of our export's log clip: blocked: `render_premiere` relinks with `changeMediaPath`, which resets the override to none (shown on both the native fixture and our export), and `relink: skip_when_resolved` fails on our package ("Verified saved media is offline"). |

All Adobe and FX renders below are local measurement evidence (not committed: no in-repo test consumes them); only the fixture and its tests are in the repository. Measured output gap (8-bit full-frame MAD; frame 30 = chart, frame 90 = S-Log3 clip;
oracle = OCIO 2.6 `studio-config-v4.0.0_aces-v2.0_ocio-v2.5`, Rec.1886 Rec.709
display, ACES 2.0 SDR 100 nits view, which is the FX ACES SDR output; the in-house
clip decodes through the oracle to the chart within MAD 0.6):

| Comparison | Chart (frame 30) | S-Log3 clip (frame 90) |
| --- | --- | --- |
| Premiere ACEScct render (no Lumetri) vs oracle | MAD 23.5, max 53: Premiere is display-preserving for SDR input (grey steps 40/120/200 render 40/120/200; ACES 2.0 renders 24/99/157) | MAD 24.6, max 91: brighter highlights and more saturated colours (grey steps 28/109/209 vs 24/100/157) |
| Same with Auto Tone Mapping off, Auto Input Gamut Compression off, or both | identical to the default (119/119 frames, MAD 0) | identical to the default |
| Native fixture render vs FX render (`tsrct` release `94971429d9`) of the fresh import | MAD 73.0: Lumetri Exposure +1 in Premiere's ACEScct drives grey 120 to 255, the import measured here wrote `primaryGrade.exposure` 0.4545 (the earlier Exposure/2.2 scale; the current fitted Exposure/2.6 scale is unmeasured in ACEScct), 0.45 scene stops under ACES 2.0 | MAD 48.6: the FX render shows the log clip as display video (`cameraInput` not applied by that renderer build, also with software decode); the FX output differs from the oracle by MAD 25.5 |
| Our `--fps 30` export render vs the native render | decoded-identical (frames 0–59, MAD 0) | MAD 40.9: the render route's relink resets the override (readback above), so this half is not a conversion measurement |

No scriptable Premiere setting brings its ACEScct output closer to ACES 2.0:
Auto Tone Mapping and Auto Input Gamut Compression change nothing on these
SDR-range scenes, and the output colour space has no setter (other output
choices are unmeasured). The ACES 2.0 SDR output is ~23–25 codes (MAD) darker
and less contrasty in highlights than Premiere's ACEScct output for the same
source; this is a measured approximation in both directions, not a fidelity
pass. HDR (Rec. 2100 output) renders of an ACEScct sequence: unrun.

**Acceptance status: implemented, not accepted.** The ACEScct mapping is
implemented in both directions and our export renders the chart identical to
the native sequence, but the FX render does not match Premiere (chart MAD 73.0
with Lumetri; ACEScct output vs ACES 2.0 23.5 / 24.6 without). Treating
Premiere's ACEScct output as a documented approximation of ACES 2.0 depends on
the After Effects OCIO ACES 2.0 output matching the FX render, which is not met
(chart MAD 2.72 > 2; log clip blocked by the FX renderer's missing `cameraInput`;
see the After Effects ledger). Render of an exported camera override: blocked by
the render route's relink. HDR: unrun.

## Linked picture packages

Ordinary CLI export can replace dependency-connected picture scopes with editable
AEPs while retaining native audio. Exact linked rates are 24/25/30/50/60 fps;
fractional sequence rates retain native results with diagnostics. Import resolves
supported links by exact file and composition GUID. This is not universal AE
feature support. See [hybrid conversion](../hybrid-adobe-export.md).

## Evidence and failure handling

The [fixture manifest](../../tests/manifest.json) selects the checked-in strict
Premiere reference corpus. [Fixture documentation](../../tests/README.md) records
provenance, thresholds and case-specific limits; registration is not a new test run.
Native-format, mapper and public conversion tests live under
[the adapter](../../crates/premiere_file/).

Software assertions, Adobe reopening/control readback and independent render
comparisons answer different questions. Native acceptance and picture evidence
are case-specific; neither implies alpha/audio fidelity or every feature combination.
Known kernel, sampling, font/layout and timing differences remain. Keep the source,
read diagnostics and verify the edited output in the destination application.
