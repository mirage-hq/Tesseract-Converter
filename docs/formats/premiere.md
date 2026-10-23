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
| Effect stacks | Supported order, bypass and compatible keys. Unknown decorative effects can be omitted locally; unsupported coverage-changing effects may omit their picture. |
| Geometry2 / Transform / Corner Pin | Bounded editable affine/projective mappings; curved paths, skew, retime, opacity, masks and host canvas have separate restrictions. Native identity/sampling can change. |
| Blur, shadow and glow | Mapped controls have kernel, radius, clipping and alpha differences. Matching numeric values is not a visual guarantee. |
| Color/channel controls | Selected Levels, brightness/contrast, Tint, Invert, Ramp, Mosaic and related profiles; Lumetri recovery is a partial editable replacement, not a general Lumetri round trip. |
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

Import emits legacy FX color policy. Export accepts validated legacy policy and
optional experimental `--hdr pq|hlg` working/output targets. Point-color conversion,
8-bit native paint quantization and settings/readback do not establish HDR footage,
effect/interpolation or high-precision rendering. ACES policy can persist in FX,
but native ACES export rejects. See the [FX policy](../../crates/fx_schema/src/color_management.rs)
and [HDR limits](../after-effects-support.md#hdr-conversion).

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
