# Conversion fixture index

`manifest.json` is the executable **Premiere** corpus index. After Effects uses
its own fixtures, tooling and [support ledger](../docs/after-effects-support.md);
do not register AEP compositions as Premiere sequences.

## Local tests and reference media

Run `make test` from the conversion workspace for the checked-in Rust tests and
native fixtures. These tests do not score Adobe references.

The manifest pins each project's selected sequence UID and package-relative input
paths. `files[].repo_path`, when present, locates a checked-in input;
`files[].path` identifies its path inside the staged package. All 41 strict
`video_reference` cases, their MP4s and their shared Arial font are checked in
under `tests/references/premiere/`; test workflows use no remote Asset service.
The former 85 `structural_only` diagnostic cases were removed rather than treated
as visual coverage. Duplicate package names (including case-insensitive
collisions), missing inputs and a `project` path absent from `files` are errors.

Local staging, Adobe/AME export and rendering/scoring are separate opt-in
operations. The strict rendering gate needs external rendering tools and the
enclosing repository checkout. Registration or passing Rust tests does not establish
visual fidelity, and this documentation does not claim that the current checkout's
visual gates were run.

## Evidence contract

The manifest contains **41 `video_reference` cases**. The manifest, rather than
a historical execution log, is the source of truth for case selection and proof
status. Each case records a comparison that satisfied its required `score_policy`,
and the strict `--references` gate selects these cases. This is not a claim that
the current checkout has been rendered or scored.

A case may have at most one `reference_video`: an independently
Adobe/AME-exported MP4. Strict cases read the checked-in file via `repo_path`;
records also retain the selected sequence UID, source project and AME preset
provenance. This does not establish complete support.

Scored cases require mean similarity **0.98**, minimum-frame similarity **0.97**,
and the manifest's explicit sample interval (0.25 or 0.5 seconds). Earlier proofs
cite the previous 0.99 mean / 0.98 minimum floors. The point-text case has
tighter floors because a blank frame could otherwise pass. Change the shared
floors only by an explicit team policy decision that applies to every standard
case. Never lower one case's threshold or replace an independent reference to
turn a mismatch into a pass.
Missing media, unsupported conversion and duration/FPS/resolution drift are
failures to obtain proof, not valid scores. RGB comparisons
do not establish alpha, audio or editable-control fidelity.

Source relocation is not Adobe export proof; this strict import corpus does not
establish editable export coverage.

## Measured C1 graphic Bezier import

`premiere_isolated_graphic_bezier_position_rotation_26_5` pins the unchanged
Premiere 26.5.1 C1 save, sequence
`c8acf9c1-34b2-4086-9f55-d528950a7059`, repository red video and
Arial-BoldMT font. Its independent AME 2026 build 85 reference is checked in at
`tests/references/premiere/videos/premiere_isolated_graphic_bezier_position_rotation_26_5.mp4`,
1920×1080 at 30 fps for 10 seconds, using the pinned Match Source preset.
The source uses repository scaffolds and own-value controls; UI was not inspected.

Public regression `adobe_measured_graphic_bezier_keys_import_on_the_generator_clock`
asserts source In 3600 seconds, graphic range 0–9 seconds, per-layer targets,
curve values/times and all four handles. Measured import forms are straight,
tangent-free Bezier→Bezier Text/Vector Motion Position, Bezier→Bezier Text
Rotation, Linear→Bezier Text Scale and Hold→Bezier Text Opacity. The saved
Rotation out speed is 15 (Premiere rewrote the authored 150); import follows
the saved value rendered by AME. The prior native readbacks measured Position
speed in normalized Euclidean units per second and Rotation in degrees per
second. Curved temporal Bezier paths, other mixed Position/Rotation modes,
Shape keys and other bent scalar mode pairs remain unsupported. Export is
unchanged. The case is `video_reference` with the standard 0.98 mean / 0.97
minimum policy at 0.25-second intervals.

A fresh 10-second FX render from independent implementation commit
`e42393796ded024b131dac55fc1b87ce02371054` matches the pinned Adobe reference:
41 grid observations score mean **0.9985037141392804**, minimum
**0.99702803519244** at 4.5 seconds. Ten additional observations at frames
33, 51, 69, 87, 123, 177, 228, 245, 246 and 247 score mean **0.9985762**,
minimum **0.998198** at 1.1 seconds: **51 observations** from one successful
render, with no new native calls. Paired frames show matching position,
rotation and clipping, with the visible
white glyph at frame 245 becoming partially transparent at frames 246–247 in
both outputs. Late text is mostly outside the canvas; those samples establish
the visible portion only. No blank or offline frames were observed.

## Measured A4 Transform with Alpha Track Matte import

`premiere_isolated_transform_track_matte_26_5` uses Premiere 26.5.1's saved
project, sequence `18832324-570e-4e73-8460-84b8c8150813`, and three repository media
fixtures. The source was authored from repository media and own-value controls;
logged XML edits added Position keys and split the matte into matching placement
ranges, then Premiere reopened, read back and saved the canonical project.
UI was not inspected. The independent 1920×1080, 30 fps, 5-second AME build 85
reference is checked in at
`tests/references/premiere/videos/premiere_isolated_transform_track_matte_26_5.mp4`.

Two fill clips at 0–2.5 and 2.5–5 seconds save Transform and Track Matte Key in
opposite orders. Native measurements show both the fill and the Alpha matte
window moving together: identity at local frame 7, +96/+27 pixels at frame 30,
and +192/+54 pixels at frame 60. Import therefore carries the Transform on the
keyed-picture group, leaving its video and static matte children at identity.
The public regression checks group ownership, the second group's clock offset,
Linear Position keys at group-local 500/1500 ms and the consumed matte children.

Admission is limited to one active Transform with Linear Position keys and an
Alpha key, a matching canvas-sized static still matte, default clip Motion,
normal blend, opacity 100 and speed 1. Other Transform values and parameters,
curved or non-Linear keys, Luma/Reverse, additional active native effects,
non-default or keyed matte Motion and retimed/shared mattes stay outside this
measured form. Existing mask and matte-range restrictions remain. The case is
`video_reference` with the standard 0.98 mean / 0.97 frame floors at 0.25-second
intervals; export is unchanged.

Independent comparison of implementation commit
`87bf2ff137907f2310948bfa4d9e63eb6a2c74ee` used one fresh 5-second FX render,
with no native calls or uploads. Its 21 grid observations score mean
**0.9904118710182356**, minimum **0.9835834115997164** at 1.25 seconds.
Six additional full-resolution RGB-hybrid comparisons at frames 7, 30, 60, 82,
105 and 135 score mean **0.9930123333333333**, minimum **0.990921** at frame 30:
**27 observations across 23 distinct frames** from the same render.
Paired critical frames and the worst grid frame were inspected: both saved
orders move the fill and matte together, and measured Adobe/FX matte-window
boundaries differ by at most **0.011 pixels**. Both similarity floors pass;
this proof covers the measured form above.

## Vertical canvas (Premiere 26.5.1): video reference

`premiere_isolated_vertical_canvas_26_5` is an independent Premiere 26.5.1 save
of the scene of the XML-derived vertical canvas fixture: sequence "Vertical canvas 26.5"
(`716eb057-b99d-4972-a9eb-6054b62eaa7a`), Custom editing mode, 1080×1920 at
30 fps, one default-Motion clip of the committed 1080×1920 source on V1 for 2 s.
Its project SHA-256 is
`de9225fefbc1158e74c3bc9a55c1c5c63b0125c3bd7e9f4872ab733fc5355770`
(10,867 bytes, absolute media paths as saved); its AME render is checked in at
`tests/references/premiere/videos/premiere_isolated_vertical_canvas_26_5.mp4`
(157,866 bytes, SHA-256
`9bece53059e7529372a86ed75376f10d18e69d00d3a4963cabda312cb8007641`).
`premiere_26_5_vertical_canvas_imports_at_its_own_size` checks its editable
import. Its one report is the save-form `ClipTrackItem/TrackItem/Node` of
`VideoClipTrackItem:63`, at feature scope; no occurrence is lost. A fresh
import by candidate `d39604799` scored 0.99264173 / 0.98756221 across nine
0.25-second samples (worst at 0.25 s) under the standard policy.

The Premiere export of the derived case's import (project SHA-256
`ca688897ff41e3c4d714ee6ec3c2f51ef1ac0e128e8d2eefeeab766fdeed3316`) reopened in
Premiere 26.5.1 with its media online, and its AME render equals the native
render at all 60 frames. At frame 0 both renders differ from the source media
by a colour-cell maximum of 4.0 levels, above the 3.0 bound declared for that
proof; geometry is exact and the last frame is within the bound. That recorded
failure is accepted by a user exception for this root-canvas proof only: the
bound and the measurement are unchanged, and the cause (AME's start-of-stream
encoding) is inferred, not verified. Not covered: an Adobe open or render of a
custom-size Premiere + AE package, captions, mixed-size nests, audio (the media
has none) and alpha; Effect Controls values were not inspected.

## Multi-object Text key import regression

`feature_multi_text_transform_keys_26_5_derived.prproj` (SHA-256
`412300d9e6ba4c0f3364fffe04054cc58fa41553461cc4d9ce1cc380d48b707e`)
is a narrow derivative of the Premiere 26.5.1 G-probe P3b save (original
SHA-256 `8c60fb54d2dfd4ea5c6fe712b9b125f8e8f002d4d94ab9a48cdbd54131d69f81`).
It deletes the unrelated final graphic at 4–6 seconds, its track/UI membership,
and records 59, 75, 76, 98–101 and 210–255; this removes the bundled template
Shape payload. The tested item 58, chain 73, components 94–96 and parameters
160–209 are byte-for-byte unchanged; project/UI membership records are edited
to remove references to the discarded placement. The first isolation revision
removed active membership but accidentally retained alternating orphan records;
this revision verifies every discarded ID and reference is absent.
The source uses repository scaffolds and own-value Text/Vector Motion fields;
no third-party media is packaged. Sequence
`c8acf9c1-34b2-4086-9f55-d528950a7059`, track item 58 at 2–4 seconds,
contains keyed Vector Motion, keyed Text `py` and static Text `ok`.
The native authoring log records keys, reopen and save, with zero AME exports.
Public regression `adobe_derived_multi_text_keys_target_only_their_own_layer` checks
placement, object order, independent layer IDs, static sibling values and
per-object/group key time, value and interpolation. The unchanged original
is retained only in local evidence, outside the public source boundary.

`feature_multi_text_source_keys_26_5_derived.prproj` (SHA-256
`efa8677cde35d9ff28c0964a482f221e5b16b6aeb653b8ee93914bccc88fd49c`)
is a supplementary XML-derived combination. Starting with that native source,
it trims item 58 to source 0.5–2.5 seconds, clones the first Text's transform
keys onto the second with distinct values, copies the two unchanged Source Text
snapshot lists from `feature_text_hold_keys_gradient_26_5_strict.prproj`
(parameters 122/148) to its Texts at source 0.5/1.5 seconds, and appends static
Rectangle component 120 and its parameters from
`feature_graphic_shapes_26_5_strict.prproj`, remapped to record IDs 4000–4018
and component ID 7 (distinct from Vector Motion 6 and Texts 4/5).
`derived_multi_text_source_keys_keep_independent_tracks_and_trimmed_clocks`
checks the public importer with two keyed Texts, their distinct Source Text
content/style tracks, shared Vector Motion, static Shape and trimmed clocks.
This combined source has never been saved in Adobe. It is the strict case
`premiere_multi_text_source_keys_import_20260930`: Adobe Media Encoder 2026
exported its sequence "Single-style point text"
(`c8acf9c1-34b2-4086-9f55-d528950a7059`, 1920×1080 at 30 fps for 4 seconds)
with the pinned Match Source preset and no media to relink. The export
helper's `app.buildNumber` returned 2; the installed application was
separately verified as 26.5.2.2. The render is checked in at
`tests/references/premiere/videos/premiere_multi_text_source_keys_import_20260930.mp4`
(376,100 bytes, SHA-256
`b39b457a092280c0cdecdaebeac5e50802ccb23b36c480e1fc5ca901e81d14b3`).
A fresh conversion and render by tools built from candidate `836a56e0d`
scored mean 0.99613357 / minimum 0.99485466 over 17 samples at 0.25-second
intervals (worst at 0 s), at the 0.99 mean / 0.98 minimum floors that this
comparison used, tighter than the standard floors. Ten additional
full-resolution RGB-hybrid comparisons at frames 59–61, 74–76, 89–91 and 119
score mean 0.9970889, minimum 0.996127. Paired critical frames were
inspected: TWO becomes THREE at frame 90, the two Texts keep independent
transforms and fades, the static blue Shape stays under the shared Vector
Motion, and the graphic occupies 2–4 seconds. Audio, export and Premiere UI
values are not covered; export still omits keyed multi-object graphics. The
transform-key fixture remains structural evidence only.

## Measured Motion Anchor Point and Scale Width import

`premiere_motion_anchor_scale_width_probe_20260930` reads the structural
derivative `feature_motion_anchor_scale_width_probe.prproj` (7,718 bytes,
SHA-256 `429745911a526d1d99f008f09c68b05853a63d2a8ddf0c100dc6e48b9690f809`;
see its provenance file) and `feature_timecoded_source.mp4`. Its sequence
`feature_motion_static_transform_strict`
(`c8acf9c1-34b2-4086-9f55-d528950a7059`) is 1920×1080 at 30 fps with one
2-second clip: Linear Anchor Point keys 0.25:0.25 → 0.5:0.5 over 0–0.8 s and
Linear Scale Width keys 50 → 100 over 1–1.8 s, Uniform Scale off, Scale
Height 50. AME exported it once (job `be41e5f3d80144a2bf41254eca106a4b`,
`onItemEncodeComplete=true`) with the pinned Match Source preset, from a copy
that only adds absolute paths to the same hash-verified media. The export
helper's `app.buildNumber` returned 2; the installed application was
separately verified as 26.5.2.2. The render is checked in at
`tests/references/premiere/videos/premiere_motion_anchor_scale_width_probe_20260930.mp4`
(257,537 bytes, SHA-256
`f88570a7764a341dce4857bab71b7e00ecec52f392c69be18cbb542af25f200b`; 60
frames, 2 seconds).

A fresh conversion and render by tools built from
`b4a3135979bfbfe58462f30753d9cbf40e48e493`, whose production inputs equal
candidate `d0aa643d3` (that commit changes only two test JSON fixtures),
scored mean 0.99463344 / minimum 0.98706935 over 9 samples at 0.25-second
intervals (worst at 0 s), at the 0.99 mean / 0.98 minimum floors that this
comparison used, tighter than the standard floors. All 60 frames fit the
native model, Position [960, 540] plus each axis's Scale times the source
pixel less the source anchor, within the 2-pixel gate: at most 0.5 px on the
picture's edges and 0.708 px on timecode landmarks. Paired critical frames 0,
1, 12, 23–25, 29–31, 42, 53–55 and 59 were inspected: timecodes match, the
anchor moves the picture and the later widening keeps its height. Edges and
text are slightly softer, so this is not pixel identity. Not covered: audio
(both renders carry AAC stereo; no sound acceptance), other source or canvas
sizes, rotation, source trims, other key forms, animated Scale Height and
Premiere UI values. A generated export's render is not an independent import
reference and is not registered here.

**Export gate: PASS** (bounded) for candidate
`d0aa643d3f8af54c1cd6c7a8820062f2cbd5c6c0` (tools built from `b4a313597`). The
explicit FX edit (`0d1aa7cc68ee9c23af7788d390e3c8ff7c51f40917379a4ee6aa114b1a9f5927`)
holds Anchor Point 960, 540 to 0.2 s, moves it linearly to 720, 675 at 0.8 s
and widens Scale Width 50 → 75 over 1–1.8 s at Scale Height 50. Premiere opened
its generated project (`c7cf896163b0d9a3cf26d3e4e6f6a5e5e703ffc648aa6bf6e82f644b1fe11549`)
without repair or relink; its transport advanced to the end, and the last frame
showed the widened picture. Premiere saved and reopened it
(`dcecd25bb07dd929c51dec545db892545abc2e98e55cf31de5ba680ecc8fc053`: same keys,
Uniform Scale off; Scale Height renamed Scale, four zero Crop parameters added).
AME's render of the generated project
(`a31e8e16dc4be8e5dd343b154520c689f32f4079487de7e966dcf2091ba32e6b`) fits the
edit within 2 px at all 60 frames (17 inspected). Not covered: a render of the
saved copy, continuous playback, audio, colour and Effect Controls values.

## Current limitations and maintenance

See the [Premiere support matrix](../docs/formats/premiere.md) and
[crate reference](../crates/premiere_file/README.md) for supported mappings,
approximations, known mismatches and direction-specific limits. Keep source
fixtures, provenance, executable assertions and manifest identities together
when adding or changing a case.

Historical comparison screenshots and run reports are not distributed with this
workspace. Removing those supplementary reports does not change a case's recorded
status or establish new proof. Reproduce a comparison using the pinned inputs and
independent reference before claiming a current visual pass.

### Sharpen

The Sharpen structural tests use the native-derived Premiere 26.5.1 source
`feature_sharpen_strict.prproj` (SHA-256
`9f7f263d7110e0440a9a08191a44e0e38486c175d0c16256cd56b77b980a295c`),
sequence `72a26059-6f85-4033-827d-63692bb9859b`. An ID-only MasterClip Node
was removed from the native save; controls, keys and media are unchanged.
Amounts A–F are 0/40/100/keyed/4000/100. D starts at 6 s with source In
0.5 s and source keys (1 s,20), (1.5 s,80), (2.5 s,50), Linear then Hold.
F has Scale 50 and its Sharpen is omitted by the bounded host policy.

`format::tests::effects::sharpen_native_source_reads_amounts_and_source_keys`
checks saved records; `convert::effects::tests::sharpen_native_import_keeps_amount_defaults_keys_and_rejects_scaled_host`
checks editable mapping. The `sharpen_edited_export_*` and
`sharpen_edited_keys_*` tests cover current FX export and record readback.
The fixture is structural-only evidence: no published reference or converter
fidelity pass. It is distributed as part of the tracked converter source, not
enrolled in `manifest.json`, which admits only strict video-reference cases.
Retained high-gain calibration failed; native non-default Motion order, alpha
and independently Adobe-read/rendered generated export remain unproved. See the
[Sharpen boundary](../crates/premiere_file/README.md#effect-stacks).
