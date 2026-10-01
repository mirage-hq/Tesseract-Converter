# Premiere conversion

`Premiere::inspect_media` and `import_with_media_map` share native target/path
resolution and admission. Used unsupported video is fatal, including supported
linked AE pictures; it is not silently omitted. Explicit FFmpeg preparation is
outside this format crate. See [media preparation](../../docs/media-preparation.md)
for backend, alpha/audio, source identity and proof boundaries. Export is unchanged.

Case names below identify historical observations, not paths to bundled run
reports or proof of the current checkout. Supplementary screenshots and execution
journals have been removed; current corpus selection and pinned references live
in [`tests/manifest.json`](../../tests/manifest.json). Native source fixtures and
unique Adobe control evidence remain under this crate's `tests/fixtures/`.

Experimental After Effects Dynamic Link support is documented in the
[hybrid implementation/evidence ledger](../../docs/hybrid-adobe-export.md).
`PrMedia::after_effects_composition()` exposes the native composition GUID without
pretending the AEP is decoded video.

**Linked-composition import (picture only).** Ordinary `.prproj` → `.tsrct`
conversion imports each video clip of a linked composition as editable FX.
Media inspection (`tesseract_output.rs`) resolves the AEP as it resolves any
package file, reads it once per resolved path (`linked_compositions.rs`), binds
it to the hashed bytes and selects the composition by its native GUID in that
exact file, through `aftereffects_file`'s file-bound linked import. Names never
select a composition. A missing AEP, an AEP producer without identity evidence,
a GUID with no composition, or a composition whose canvas differs from
Premiere's link record omits that media's clips with the reason; a composition
shorter than the source range that a clip shows, or one that forms no editable
picture, omits that clip. Other clips convert; a changed, unreadable or
unpackageable file stops the conversion. The clip itself follows the video-clip rules of `import_video_clip`
(Motion, Opacity and their keys, Crop/Wipe/Opacity masks, stage groups, effects,
track matte, Enable, speed and Time Remapping). Only its picture layer differs
(`convert/after_effects.rs`): a group on the video layer's clip clock, so the
clip's keys keep their clip times. When the clip shows the composition from a
source In, at another speed or remapped, a `Premiere linked source` group under
it maps the clip clock to the composition clock; its children are the
composition's root Group and a `Linked composition canvas` guide, the canvas
rect whose Add mask on that root clips its content to the composition's canvas,
as After Effects renders it. The clip's Motion, masks and effects apply to the
clipped picture above. On the document clock the clip group carries an identity-rate
`playback`, because the FX runtime evaluates keyframes under time remapping from
the first Group with `playback`, on the document clock
(`document_clock_seed`). Under a stage group or nest that starts later, a
time-remapped linked composition is reported as an approximation: its animation
under time remapping runs early by that start. On a retimed linked clip the effect keys are
omitted, keeping static values, as Motion and Opacity keys already are, because
native keys match that clip clock only at unit forward speed. Each placement,
including repeats and nested copies, imports its own copy with fresh layer, item,
effect and keyframe identities after every identity the document has used. The
composition's footage is packaged under one `premiere-aep-<n>-item-<id>`
namespace per resolved AEP; the one normalized file that backs each asset
(normalized PSDs) lives until the archive is written. A composition's enabled
motion blur sets the document's one shutter as a Transform's does; a clip that
requests other settings is reported as an approximation. The AEP and
every local file that the pictures were read from (footage, PSD/AI sources, a
relinked source's missing authored path) join the pre-publication media checks,
in Check and Write, and packaged footage must keep the bytes conversion read.

Premiere plays a linked composition's sound only through its audio track items,
so the picture's After Effects audio is muted (hidden audio layers, disabled
video sound, no volume keys). Those audio items are **not converted**: the reader
omits each with a contextual reason (`LINKED_AUDIO_REASON`); linked-audio
occurrence import is unimplemented. Frame blending on a linked clip is reported
and not converted. Fidelity follows the existing After Effects import
approximations, which are reported per linked media record; no converted render
has been compared with Adobe here.

`Premiere::import_with_linked_compositions` takes each placement's content from a
caller-supplied importer (`LinkedCompositionResolver`) in place of that import:
the same placement, clock, canvas, blend, stage and omission rules apply, and a
failure that the built-in import would omit (an After Effects profile, identity
or content that forms no picture) omits the clip; any other importer failure stops
the conversion. The importer reports its own notes, and the lifetime and freshness
of the assets that it returns are its caller's: only the AEP is rechecked.

The CLI coordinates hybrid package export through the two format libraries. A
bounded large-project AME run exercised a generated `.prproj` with linked AEPs;
Premiere editing-UI acceptance and independent render fidelity remain unverified.

`Premiere::stage_with_after_effects_overlay` stages an explicitly supplied native
remainder and adds one full-canvas, topmost Dynamic Link occurrence. Its
`AfterEffectsOverlay` input carries the independently established GUID, canvas,
rate, intrinsic duration and frame-aligned placement. Source in/out equals
timeline in/out: extraction must retain the original composition clock. Native
media is inspected, bound and copied before the foreign AEP is inserted, so an
AEP is never probed as video or resolved as an archive asset. The overlay canvas
and frame rate must match the native sequence, whatever its canvas size, and
native-only export defaults are unchanged.

`StagedPremiereExport` owns only its temporary Premiere project/native media;
its report inventories those files, **not** the foreign AEP. The coordinator must
supply the AEP at `after_effects_path()` (`media/compositions.aep`) and preserve
its local dependency layout, validate independent topmost scope extraction and
source freshness, and publish/roll back the complete package. Authored absolute
paths target the fresh final output, never staging. Drop/error cleans only the
private staging directory. This API performs no Adobe operation and is not
extraction-safety, generated-project acceptance, render, or relocation proof.

### Replaying prepared source slots (bounded staging API)

`PreparedPremiereExport::packing_recipe()` exposes preparation-local source and
container tokens. `retained_picture_boundaries(container)` identifies slots with
actual native picture placements (not pending empty nests), so a coordinator can
avoid replacing retained content with an AEP that omitted its source owner. `stage_with_picture_replacements()` consumes that retained
lowering once and returns `StagedPicturePremiereExport`, whose
`after_effects_paths()` lists the foreign AEP subtrees the coordinator must supply.
It does not publish, evaluate scripts again, copy foreign AEPs as native assets,
or certify the caller's semantic/dependency plan. Incomplete capture or invalid
ownership/clock requests reject; ordinary native export remains available after
capture exhaustion. See the [evidence ledger](../../docs/hybrid-adobe-export.md)
for CPU verification and remaining audio, routing, import and Adobe-proof gaps.

### Reusing a prepared native export (P1 groundwork)

`Premiere::prepare_export(archive, document, options)` retains one bounded JS
preparation, media inspection and native lowering result. The returned
`PreparedPremiereExport` borrows the original archive/document and exposes
`original_document()`, the distinct `prepared_document()`, and `losses()`.
Repeated inspection does not rerun JS. Consuming `stage_native()` or
`stage_with_after_effects_overlay()` emits that same retained native result;
ordinary export and the older inspection/overlay entry points share this path.
The original remains available for future AE extraction, rather than replacing
it with Premiere's fitted keys. Options/rate are fixed at preparation.

`StagedNativePremiereExport` owns only native project/media files and has no
foreign AEP requirement. The existing overlay handle keeps its AEP contract.
Both validate the writer during staging and clean only their private directory
on error/drop; neither publishes final output. Copying verifies native asset
bytes. Whole-archive freshness/common-package publication remains the caller's
responsibility; ordinary export retains its existing source-hash checks.
A changed ZIP header/tail is not necessarily a changed media payload, and the
borrowed-source staging API makes no whole-file snapshot/locking guarantee.

Empty native content retains inspection observations, but emission returns the
existing no-content error. `has_native_content` is not a capability or fidelity
verdict. CPU reuse/integrity/cleanup tests are not Adobe proof.

### Editable linked compositions

`Premiere::import_with_linked_compositions` accepts a file/GUID-qualified callback
returning typed FX content, the first unused numeric ID and asset paths. Its
content takes the ordinary linked placement described above (clip clock, source
group, canvas clip, native clip Motion/Opacity/effects/matte handling). The caller
retains temporary foreign media until archive writing completes. Native source
hashes, the AEP's own hash, fresh output and publication checks still apply; the
caller's assets are packaged as returned and are not rechecked.

Ordinary Premiere import, including the CLI route, resolves linked compositions
itself (above); the CLI export route automatically packages required editable AEP
picture scopes. Linked audio, unsupported links, canvas mismatches, short
composition ranges and conversion limits are reported per clip, and conflicting
motion-blur settings are approximated. See the
[hybrid ledger](../../docs/hybrid-adobe-export.md) for implementation and proof
status. Parallel semantic census, control witnesses and writer observers were
removed; future features belong in the actual converter and behavioral tests.

### Observing native export losses (routing groundwork only)

`Premiere::inspect_export_losses(archive, document, options)` first runs ordinary
bounded JS-to-keyframe preparation on the supplied view, then verifies its
archive-backed native sources and runs the same native lowerer without
staging/publishing a project. The additive `ExportLossReport` retains the ordinary
human `diagnostics` and records loss events with a document, layer-subtree,
exact-layer or typed `PropertyTarget` source. Actual field guards distinguish
picture, audio, description-only metadata and shared-context losses; unaudited
semantics and opaque metadata remain `Unclassified`. Targets are not resolved to
owning layers by this API. No warning text is parsed, and existing standalone
import/export mappings, diagnostics and CLI behavior are unchanged.

The event list retains every routing event and its provenance; the former
1024-event and 1 MiB text ceilings prevented otherwise representable large
projects from reaching editable AE fallback. `losses_truncated` remains as a
compatibility marker and is false for the current collector. Human diagnostics
keep their existing bounds. Instrumented sites record before ordinary diagnostic
deduplication/truncation. Existing
buffered nest-motion messages are replayed as unclassified subtree observations;
their original pre-buffer events are not reconstructed. Aggregated script-baking
and written-key summaries remain document-scoped and unclassified, including
informational success counts; their presence is not evidence of a lost picture.
Inspection uses the same bake/lower/written-key diagnostic stream as ordinary
export, not a raw-JS preview that would incorrectly call supported scripts missing.
No-native-content
lowering retains its report with `has_native_content = false`; ordinary export
still returns its original no-convertible-content error, on a custom canvas as
on 1920x1080. Other source-inspection and lowering errors propagate.
Gap coverage is checked during final emission after supplied picture replacements,
not during preparation; native-only emission retains the same gap check.

**This is a partial loss observation, not a capability or fidelity verdict.**
Silence is not positive semantic coverage. `has_native_content` is not native
writer validation, Adobe acceptance, losslessness or dependency-safe extraction.
The CLI coordinator uses these actual source locations for root-scope
selection, without treating zero losses as a preservation certificate. Tests
assert CPU-side provenance, bounds, diagnostics and error behavior; they do
not provide new Adobe-native feature or render proof. See the
[shared implementation/evidence ledger](../../docs/hybrid-adobe-export.md).

The library owns Premiere parsing, typed conversion, media validation, staging,
and publication. The CLI supplies arguments and presents success or error text.
Serde encodes the typed native XML schema and embedded native JSON.
Conversion data is not passed through `serde_json::Value` or a generic XML tree.

`PrProjectFile` is the shared structured model for the convertible Premiere subset.
It contains sequences, video tracks, audio occurrences, timing, dimensions, and
a project-wide media table. `PrMedia` holds the video stream, the audio stream,
or both. A native video stream keeps its positive `SourceFrameRate` in ticks per
frame; the sequence uses the separate eight-rate `FrameRate` whitelist. File
inspection keeps the timescale, sample count and either a constant sample
clock or a proven quantized nominal clock in `VideoTiming`. Its duration comes
from the actual file endpoint, rather than a fabricated average frame duration.
A video track holds media occurrences and Type-tool graphics. A graphic
holds single-style text and shape objects; its synthetic generator media is not a
media-table entry. Each occurrence refers to shared `PrMedia` source facts through a
`MediaId`. On Premiere input, `MediaId` identifies the native Media record,
including its `ObjectID` or `ObjectUID` namespace. On Tesseract input, it is the
asset ID. Identity is the native record or asset ID, not path plus hash.
Separate media records remain separate assets even when they refer to the same
file. Repeated placements of one record share one asset. `PrVideoStream::kind`
distinguishes video streams from Premiere stills (`IsStill`): a still has no
media clock, so its occurrences carry Premiere's synthetic source range, and
import compares only its dimensions with the image file, not its native still
`FrameRate` or `Duration`. The shared timeline rules apply to stills unchanged
(`schema/still.rs`, `image_media.rs`). `PrMediaKind::ColorMatte` marks Color
Matte generator media (no file, paths or asset), recognised by its `COLR`
`FilePath`, not by the generator `ImplementationID` it shares with Graphic and
Black Video media. Timeline rules treat a matte as an opaque occurrence; file
inspection and packaging skip it. `PrMediaKind::Adjustment` marks the Black
Video media of an adjustment layer (`schema/adjustment.rs`): the reader sets it
only for a placement whose `VideoClip` carries `AdjustmentLayer` true, and then
requires a master clip with `IsAdjustmentLayer` and the `BLAK` generator record
(`reader/adjustment.rs`). The native 26.5 stream may omit `FrameRate` only
with `IsFrameRateOverridden=true` and `OveriddenFrameRate=8467200000`;
explicit `FrameRate` keeps precedence when both are present (the older native
fixture saves 29.97 with a 30 fps override). A plain Black Video clip stays a generator-still
omission and a flagged clip without its master clip is omitted. An adjustment
has no picture of its own: it is never packaged, it covers no gap, a sequence of
adjustments alone has no convertible content, and in FX it is an
`AdjustmentLayer` over the layers below it (`convert/adjustment.rs`). Exported matte records have not been reopened
in Premiere: their `ImporterPrefs` `BinaryHash` is a random UUID, as the
file-media writer's hashes are, not Adobe's colour-derived value, and their
`ModificationState` copies the most common corpus value. The model does not
contain commands, reports, output policy, source hashes, or serialized XML.

The format implementation has explicit responsibilities:

```text
src/schema/     shared `PrProjectFile` model, native XML records (`native*.rs`),
                identifiers, writer defaults, and native parameter layouts
src/format/
  graph/        bounded roxmltree graph, reference traversal, and selected
                record dispatch directly from element nodes
  text_payload  Source Text payload codec shared by the reader and writer
  reader/       gzip decoding, timeline selection, reference resolution and
                native-wire checks; `PrProjectFile::load` lives here
  writer/       typed record construction, quick-xml encoding, and gzip output
```

The reader rejects malformed references and unsupported native Motion fields
before mapping them, because the completed model does not retain those fields.
Once selected sequences are assembled, `PrProjectFile::validate()` checks their
timelines and project-wide media references. The writer runs the same model
validation before its output-specific restrictions and XML encoding. Thus load
is more than deserialization, but model-level rules have one validation entry.

Directional mappings live under `src/convert/`. Premiere-to-Tesseract conversion
builds typed `fx_schema` layers and animation tracks and returns the existing
`TesseractFileBuilder`. Tesseract-to-Premiere conversion returns a
`PrProjectFile`. Operation modules resolve media, retain source identities, and
publish files safely. The keyframe path reads and writes intrinsic Motion
Position, Anchor Point, Rotation, uniform Scale and Scale Width keys and
intrinsic Opacity keys as occurrence-local animation and editable FX property
keys.
Internal native animations are property-specific enum variants; Position keeps
point-valued keys and spatial tangents (`PrPropertyAnimation::Position`) rather
than misrepresenting its spatial data as scalar, and Anchor Point keys are
points too (`PrPropertyAnimation::AnchorPoint`).

Graphic text follows the same owners. `schema/text.rs` owns the Text and
Vector Motion parameter layouts. `format/text_payload.rs` owns the Premiere 26
Source Text value: a length prefix, a magic number, and a FlatBuffer. The
reader bounds every offset and rejects unknown FlatBuffer slots, inline mixed
styles, legacy encodings, nondefault fixed graphic parameters, keys on a
parameter that `schema/text.rs` does not mark as keyable, and Bezier keys that
it does not mark as measured, so unmodeled styling is reported instead of dropped. Premiere writes each distinct
binary value once and later copies name it by `BinaryHash`; the graph resolves
those references. A hash defined with different values omits the graphic that
names it instead of taking the first definition. The reader composes a static
Vector Motion into the text transform and its keys, and keeps a keyed one, or
a static one whose composition would take the text outside its native ranges,
as `PrGraphic::vector_motion`. The writer builds the payload with
`flatbuffers` and emits a private generator media, a master clip outside the
project panel, and one component per text or shape object from the current
document, after an intrinsic Vector Motion component when the graphic has keyed
or retained static Vector Motion.
Before encoding, it rejects a text or shape layer name (its component's
`InstanceName`) with a character that XML 1.0 cannot represent, and a graphic
whose source out-point (its generator in-point plus its duration) passes the
signed 64-bit tick range. Such a layer fails the whole export, in check mode as
in execution; names have no length limit. See [Fonts](#fonts) for font identity.

Point text retains the native document's vertical alignment around its origin.
Import writes explicit line spacing (native 120% of font size plus native
leading), rather than choosing line spacing from the packaged font's metrics.
Center and bottom alignment shift the local anchor by half or all of the
first-to-last baseline advance, so source rotation and scale still apply around
the original point. Empty and trailing lines count toward that advance; a
single line keeps its original anchor. Held Source Text keys keep text, size,
line spacing and the derived anchor together on the generator clock. Export
recognizes coherent held alignment keys and recovers native point alignment;
an arbitrary keyed text pivot is diagnosed instead of silently discarded.
Static point text exports its equivalent baseline placement with top alignment.

Static point-text style runs that cover complete lines import as independently
editable Text children under one graphic group. Each line keeps its font,
size, fill, stroke, caps, tracking and Unicode; the group owns the source
transform, transform keys, opacity and any supported common shadow. The
incoming line's automatic spacing advances its baseline. This bounded mapping
does not support inline style changes, mixed-style box text, explicit native
leading on mixed styles, full justification, or keyed mixed-style Source Text.
These forms are diagnosed. Editing/export uses the existing graphic-object
path and produces independent uniform native Text objects, rather than
recreating the original mixed Source Text payload. Existing shadow and
background restrictions continue to apply. Font glyph coverage, including
emoji fallback, depends on the packaged font and renderer.

The unchanged native fragments in
[`tests/fixtures/point_text_lines`](tests/fixtures/point_text_lines/README.md)
exercise the real reader, hash resolution, line styles, editable group clocks
and local baselines. Their provenance includes independent native frame
evidence; these CPU assertions do not constitute Adobe playback/reopen proof.

A graphic holds its objects, Text and Shape (`PrGraphicObject`), in the order
of its component chain, which is its paint order: Premiere draws the first
listed object in front. `format/shape_payload.rs` owns a Shape's two binary
values, stored like Source Text: the Path (version 2: one contour of corner
or smooth vertices with absolute tangents in layer pixels, and a closed byte)
and the Appearance FlatBuffer, whose slots `schema/text.rs` models only in
the forms that AME calibration renders measured (fill, the fill switch, a
centred stroke, the shadow); any other slot or value omits the graphic.
`stroke_join` is the one place that decides whether the FX renderer draws a stroke's
corners as Premiere does, from the corner classes bounded by calibration-2's
measured triangle corners (71.996° mitered, 36.008° beveled; other angles
inferred). With two or more objects, or one under keyed Vector Motion or a
clip Opacity, import makes the graphic a group whose layers are the objects;
a static Vector Motion folds only into a single object. The writer gives the
objects component IDs 4 upward in chain order, and a Vector Motion the next
one, which it writes whenever a graphic's group transform is not the
identity. Any FX rectangle that is no Color Matte exports as a graphic of
one Shape: `convert/color_matte.rs` hands the Shape rules the shape layer that
draws it (`rect_as_shape`), with FX's rounded outline and fill paint; a
gradient that they do not export (`graphic::unexported_gradient`) is drawn
with the rectangle's fill colour and reported.

The text object's Position, Scale, Rotation and Opacity keys reuse the Motion
key readers and writers (`format/reader/animation.rs`,
`format/writer/tracks/animation.rs`) and `PrPropertyAnimation`. They use the
graphic's generator clock, so `PrGraphic::in_ticks` keeps the placement's
`InPoint` and a key's layer time is its time minus that. The writer places
the clip at `in_ticks`, which export sets one hour into the generator, and
marks keyed graphic parameters `IsTimeVarying` as Premiere 26.5.1 saves them.
`convert/graphic.rs` maps the keys to the text layer's FX tracks and back.
Linear and Hold keys convert on every keyable parameter. A temporal Bezier's
handles are the speeds stored on its keys, and an Adobe probe (Premiere
26.5.1 readback within 3e-6 at 25 samples; its AME render agrees, a tenth of
the unit misses by 13 to 22) measured them in value per second, the clip
Motion unit, on Text Scale and Opacity and Vector Motion Scale and Rotation.
Bezier keys there convert with the Motion Bezier rules
(`GraphicParamSpec::bezier_speeds_verified`), except a Bezier key right after
a Linear or Hold key whose in-handle bends the segment into it, or right after
a Linear key whose stored out-handle bends it (F23, measured on clip Motion
Scale only), which the probe did not cover: the reader omits that graphic. The in-handle is compared
exactly, with no tolerance: only an influence of exactly zero or a speed
exactly equal to the segment's average value per second is neutral. Export
writes Bezier keys with the Motion key writer, which ends a segment on a key
in the mode of the segment after it (Linear `0` for the last key) that keeps
the in-handle, except on a key that starts a Hold (the Motion key rules
below). In an Oracle export gate, Premiere 26.5.1 read such a segment
back within 3.41e-6 of the Bezier, where a Linear end would miss by up to
2.43 (Adobe reopen; that it follows the in-handle is inferred from these
values), and AME rendered the curve (render). On
Position and Text Rotation, C1 independently measured additional import
forms: straight, tangent-free Bezier→Bezier Position on Text and Vector Motion,
and Bezier→Bezier Text Rotation in stored degrees per second. Import also
accepts Linear→Bezier Text Scale and Hold→Bezier Text Opacity. Other mixed
modes and curved temporal Bezier Position stay omitted. The reader-only
admission does not extend export: Position and Text Rotation cubic easing
still reports the property and keeps its static value. Saved Linear speeds vary (one, a tenth or a hundredth
of the value per second) and are ignored. Spatial tangents convert.
Each Text object keeps its supported transform and Source Text tracks, even
when it shares a graphic with other Text or static Shape objects. Import
targets each track to that object's layer and subtracts the placement InPoint
from its source time. Shape keys, object masks and unsupported key forms
still omit the graphic. This admission change does not extend export.

Keyed Vector Motion moves the whole graphic, so import makes it a graphic
group: an FX group with the Vector Motion as its transform and tracks, over
the placement's range, whose layers are the graphic's objects on the same
clock. Export maps a group whose only layer is a text layer back to one
graphic: a keyed group becomes keyed Vector Motion, written in the layout that
Premiere 26.5.1 saves (component ID 5, no private data, the static parameters
of Adobe-saved graphics), and a static group is composed into the text, or
written as that static Vector Motion when composing would take the text
outside its native ranges (a 200% group over a 3,000% text). Premiere 26.5.1
reopened an export of that form and read back Vector Motion Scale 200 and
Text Scale 3000, not a folded 6000 (Adobe reopen), and AME drew its text
within 0.41 px of the model (render). Other folds that leave the range, a
rotated one and a group that mixes keyed and static values are inferred from
it; a nonuniform scale converts in neither direction. The
graphic clip's own Opacity fades the whole graphic too: the video Opacity
reader (`read_video_compositing`) reads it, and a nondefault or keyed one also
makes the group, as the group's opacity and Opacity keys. Export writes them
back with the media clip's `opacity_records`, first in the chain and without
`DefaultOpacity`, as Premiere 26.5.1 saved the keyed clip Opacity of
`premiere_isolated_graphic_clip_opacity_keys_26_5`: [Opacity, Text]. With
keyed Vector Motion too, export writes and import reads [Opacity, Vector
Motion, Text]; that order is inferred, not seen in an Adobe save. A chain
with neither `DefaultOpacity` nor an Opacity component reads as opacity 100:
the video reader's existing fallback, not a graphic form in the Adobe
evidence (no corpus graphic has it). Its Blend Mode converts as a media
clip's, on the group, which a blended graphic of one object also gets; export
writes an ungrouped layer's blend on its graphic, and the blend of an object
inside a graphic group, of one object or several, which Premiere has no field
for, as Normal with one warning. Clip Motion must keep its default, and its Bezier keys follow the probed
parameters' rules. Group
effects, masks, track matte, background, time remap, motion blur,
nonuniform scale, skew or 3D rotation, or a text that does not span the group,
omit the graphic. Masks or a track matte on the text omit it too,
for a root text as for a group's text (`unsupported_graphic_text`), because
graphic export writes neither.

The text shadow has the same split. `schema/text_shadow.rs` owns the enabled
shadow model and its value ranges. `format/text_payload/shadow.rs` reads and
writes its Source Text document slots and refuses to write an out-of-range
shadow. `convert/text_shadow.rs` maps it to one FX `DropShadow` effect and
back, keeps it only on filled, unscaled, unrotated text, omits an out-of-range
shadow as a feature while the text converts, and names every other text-layer
effect that export omits. AME calibration renders verify the slots and the
mapping: the angle (slot 13) in degrees clockwise from up toward the shadow,
the distance in pixels, the blur as a Gaussian of 0.0966 px σ per unit, the
size as a 0.489 px dilation per unit, and the opacity as a linear-light blend.
The ledger states the residuals and bounds.

Caption tracks reuse that text path. `format/reader/caption.rs` reads each
`CaptionDataClipTrack` in the data track group once the video tracks have fixed
the frame and cadence. A cue's `Block/FormattedTextData` and its track's
`CaptionDataTemplateStyle` are the Source Text encoding with the caption
document markers seen in Premiere 25.5 and 26.5.1 (`text_payload::decode_caption`).
Each cue becomes a timed `PrGraphic` on a lane appended above the video tracks,
so the converter imports it as FX box text and the writer exports it as a
Type-tool graphic, never as a caption track. Premiere draws each cue from its
own payload and uses the track template only as a default (Adobe-verified), so
the reader takes the cue's style from its `FormattedTextData` and decodes the
template only to omit a track whose layout is unknown. The default-position
corpus cues store no geometry; the reader places the bottom default where
Premiere draws it, with the last line's baseline at 95% of the frame height as
box text whose line slots end there (Adobe-verified; see the ledger), centred
or, for a left-justified cue, starting 12.5% of the frame width in (inferred),
and stacks a higher caption track above a lower one (Adobe-verified). Strokes
and shadows convert as for Type-tool text. A background box
(`PrTextBackground`: slots 17–20 and 34) is kept in the document and imports
as an FX group box around the cue's text at the calibrated 48 px; an
unverified box (another size, an opacity other than 100, a radius above half
the box's least height, or an omitted slot) is omitted as a feature and the
text converts; on export the box is written only around an unscaled,
unrotated, opaque text layer without keys (`convert::graphic`). Other styling
and other placements, including a stored position (slot 33), are omitted with
the attribute named. Cues on a hidden track and
disabled cues become disabled graphics that import as hidden layers
(inferred). Caption content never fails its sequence. Type-tool text keeps no
background: `text_payload::decode` reports an enabled one as omitted.

The Premiere mode on an outgoing key maps to FX easing on the next (incoming)
key: Linear (`0`) and Hold (`4`) remain editable, Bezier (`5`) becomes a cubic
Bézier built from the two keys' speed and influence (a Bezier between equal
values converts, as Linear, only when each of its two handles has zero speed or
zero influence), and other numeric modes omit the clip; the unverified secondary
mode is discarded. As Premiere reads scalar keys (F23, Oracle run C6, probed
on Motion Scale), a Linear key before a Bezier key eases into it with both
stored handles, and handles on the chord keep it Linear; point (Position) keys
were not probed and keep that interval Linear. A Hold key holds whatever
follows. FX cubic Bézier tracks export as Bezier
with the matching speed and influence. Export rebuilds native keys from the
current FX track rather than reusing the import XML. A scalar key saved with
Hold (`4`) is the exception: Premiere 26.5.1 ignores its in-handle and arrives
with a zero-length one (Adobe readbacks of an exported graphic clip Opacity
and of a clip Motion Scale probe), so the reader reads the Bezier into it as
(x1, y1, 1, 1). Export writes an FX curve into a key that starts a Hold only
when Premiere draws it the same: a cubic already arriving that way, a straight
one, Linear or Hold. Any other curve omits its property's keys, as other keys
Premiere cannot hold do. Point (Position) keys keep their stored in-handle in
both directions; that form is unprobed.
Scale keys under Uniform Scale map to paired FX ScaleX/ScaleY tracks, which
export writes back only when both axes have identical keys over equal static
scales. While Uniform Scale is on, import takes Scale for both axes and ignores
the static Scale Width, which Premiere leaves unchanged, as AME renders it.
Without Uniform Scale, Scale Width keys map to the FX ScaleX track alone, and
export writes a ScaleX track whose Y axis has no keys as Scale Width keys with
Uniform Scale off, whatever the static scales: equal static axes are no
uniform Scale once one axis moves alone. Anchor Point keys map to the
AnchorPointX/AnchorPointY pair in source pixels, as the static anchor does,
beside Position keys in canvas pixels; their keyframe ids name their own
property, so the two pairs never share one. Both directions admit only the
form that AME's render of `feature_motion_anchor_scale_width_probe.prproj`
(case `premiere_motion_anchor_scale_width_probe_20260930`) measured
(`PrPropertyAnimation::unmeasured_form`): Linear keys, and Anchor Point keys
without spatial tangents. That render is 1920 x 1080 media on a 1920 x 1080
sequence without rotation or source trim; other frames, rotation, stills
and nests are inferred, not Adobe-verified. One generated export of that
form passed a bounded Adobe export gate
([fixture index](../../tests/README.md#measured-motion-anchor-point-and-scale-width-import)). Other Anchor
Point and Scale Width key forms, Scale Width keys under Uniform Scale and
Scale Height keys without it stay unsupported: the reader omits the clip, and
export reports the property and keeps its static value, as other Motion keys
that cannot convert do. On
import, native ticks round to signed milliseconds (at most 0.5 ms); exported times have millisecond
precision. The conversion-accuracy ledger is in
[`apps/tesseract-conv/README.md`](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations).
Update it and regression tests whenever mappings or precision change. Do not
add user-facing approximation warnings. Linear and Hold Motion and Opacity keys
passed a Premiere 26.5.1 reopen and an AME render (JRB-2081 export gate, below);
Bezier keys and the generated graph still require that check, and no
application-level compatibility is claimed.

Intrinsic Motion and Opacity have two native layouts, with one parameter table
each in `schema/motion.rs` and `schema/opacity.rs`. Premiere 26.3 writes
`Bypass` `false` and seven Motion parameters. Premiere 26.5 writes no `Bypass`,
keeps control types only on Rotation and the primary Blend Mode, adds Motion Crop
Left, Top, Right and Bottom as Motion parameters 8-11, and raises the primary
Blend Mode's upper bound from 26 to 27. Both layouts read the Blend Mode from
parameter 2 alone (`PrBlendMode`, one table in `schema/opacity.rs`): an AME
render of a Premiere 26.5.1 save (`premiere_isolated_blend_codes_26_5`)
measures 26 codes as their modes on encoded values and renders (22, 0) and
(1, 0) as (22, 10) and (1, 5). Export writes each mode's canonical pair, the
chart's (that the Blend Mode menu writes the same parameter 3 is inferred); an
export gate measured the written pairs on stills, and Screen on a Color Matte,
a graphic of one shape and an adjustment layer and Multiply on a nest. Dissolve
(6), code 27 and any other value are unmeasured and convert as Normal with one
warning; FX's Darker and Lighter Color select by channel sum where Premiere
selects by BT.709 luma, one warning each. Adobe save: Premiere 26.5.1 writes
(18, 0) on a new clip and keeps (22, 10) when it converts an 11.1.2 project (31
of 31 components, component Version 7 raised to 9).
The reader selects the layout by the absent `Bypass` and maps values the same
way in both layouts. Each parameter must match its table's ParameterID, name and
record type. Opacity parameters in both layouts, and Motion parameters in the
26.5 layout, must also match its ClassID, control type and bounds (Motion also
its UI bounds). Saves in the 26.3 layout vary those Motion fields (Premiere 9-14
write a Scale UpperUIBound of 100 or none), so they stay unchecked there. A
`Bypass` on a parameter, which no save writes, omits the clip. Motion Crop at 0
converts; a nonzero or keyed Motion Crop omits the clip, because Crop has no
mapping yet. A bypassed 26.5 component is unobserved, so a component `Bypass`
other than `false` still omits the clip. The writer keeps the 26.3 layout and
writes Normal as (18, 0). Adobe reopen and render (the export gate of
`premiere_isolated_motion_opacity_26_5`): Premiere 26.5.1 reopens its export with
(18, 0) and the Motion and Opacity values it reads from the fixture, except S2's
Scale Width (60 for 100, with Uniform Scale on), and AME renders the export with
decoded frames identical to the fixture render (300 of 300, MSE 0).

Crop and Linear Wipe also have a Premiere 26.5.1 form (Oracle run C6: the P0
probe and the F1 and F1m saves). Its component has no `Bypass` or `Intrinsic`
(26.3: both `false`), and only in that form does a missing `Bypass` mean
active. The Crop's parameters then match `CROP_PARAMS_26_5` in
`schema/crop.rs` exactly: the 26.3 table's ids, names, ClassIDs and bounds,
with `ParameterControlType` only on Edge Feather, no bounds on the Zoom
checkbox and no `IsTimeVarying`. The wipe is named "Linear Wipe (Legacy)" and
keeps its three 26.3 parameters. Any other shape fails with the 26.3 form's
messages. The writer keeps the 26.3 forms.

The forward import parses one native graph and decodes only the selected
sequence and its nested dependencies. Target listing reads sequence GUIDs and
names, dimensions, FPS, direct track counts and duration from native XML metadata,
without converting timelines or inspecting media. The validated Tesseract
builder is retained through check completion or writing; the document is not
rebuilt.

Shared record tags, class IDs, class versions, paths, and writer defaults live in
`src/schema/`. Each native XML record has one concrete definition in `native*.rs`:
reader and writer use the same type, with no reader-only record aliases. Typed
writer constructors turn generated IDs into opaque wire references without
changing UUID generation. Records not needed for the selected sequence still
have definitions, but the reader does not traverse or decode them. A selected
record's references and `xsi:nil` use are checked when it is decoded; unused
records cannot block conversion. Graph record dispatch, reference resolution,
and fail-closed validation remain reader operations rather than another schema.
Explicit contract tests cover identities, references, and limits. `roxmltree`
parses and bounds the native graph, and its element nodes supply topology and
lenient lookups; `quick-xml` decodes each selected record from its byte range
into the shared native types. The writer separates
XML encoding, project settings, sequences, media, audio tracks, and video tracks.
Its graph entry allocates every typed object identity once. Record-family
constructors consume those identities and build the complete typed records.
`quick-xml` is the only production writer for XML syntax.
The reader, converter, and writer share the sequence timeline rules. Both
conversion directions use the same media inspection rules. Import compares each
native media record's frame rate, duration, and dimensions with the file facts once.
If the dimensions differ, its placements are omitted with the native and file
sizes reported; other media in the sequence still convert. Malformed files and
I/O failures retain their existing error behavior.
Export builds each media record from the file facts and checks every referencing
layer against them. One container rule, `media::admitted_container`, admits MP4/MOV
video, PNG/JPEG stills and MP4/MOV/M4A/WAV/MP3 sound-only media for import,
package binding and the writer, so the writer also rejects a video record named
as an M4A, WAV or MP3 file. Import packages each file with its container's asset
kind (MP4/MOV Video, M4A/WAV/MP3 Audio, PNG/JPEG Image). A sound-only MP4 or MOV,
such as the audio-only Media that export writes for an audio layer of an A/V MP4,
therefore stays a Video asset and exports again after a round trip. Package media
binding stays in the publication code. The writer uses validated borrowed media
paths; it does not assume that optional model paths are present.

Import accepts a media record with no `RelativePath` when a live absolute
`FilePath` or `ActualMediaFilePath` identifies its source. Every live candidate
must have identical bytes, including relative hints and absolute aliases.
Missing aliases may be stale; conflicting live aliases stop conversion. An empty
or relative absolute alias still rejects, and Windows drive aliases on a POSIX
host still require a package-local relative hint.

`src/video_format.rs` admits containers and codecs (H.264 `avc1`, HEVC Main and
Main 10 `hvc1`, with an iPhone Dolby Vision record only when its base layer is
plain HEVC) and rejects every other file extension or sample entry by name; the
Dolby Vision entries `dvh1`/`dvhe` reject with a reason naming the engine, whose
sample-entry tables do not map them. Explicit colour
metadata other than BT.709 (BT.2020 primaries, PQ or HLG transfer, P3 primaries,
the BT.2020nc matrix; `media_metadata::ColourDescription`) passes through in the
unchanged bytes with one `Approximated` report per media record in each
direction (`ColourDescription::merge` reconciles the `colr` box and every SPS
field by field and rejects explicit codes that disagree, BT.709 included);
other codes still reject. Tracks are classified by handler type: timecode,
iPhone `mebx`/`meta` and other nonvisual data tracks are ignored; `sbtl`,
`text`, `subt` and `clcp` subtitle or caption tracks, a second video track and a
second audio track reject. AME has
rendered the derived HEVC input; Premiere 26.5.1 reopened the generated HDR
project with its media online and its HEVC `CodecType` and HLG
`OriginalColorSpace` unchanged on Save As (`oracle/M2/gate/facts.md`); no
generated HEVC project has been rendered. Apple ProRes rejects because the web editor and
player decode through WebCodecs, which has no ProRes decoder. Native decode
exists (FFmpeg, VideoToolbox) with a measured colour shift, so lifting the
rejection needs a web decode path or a transcode-on-import policy. Export writes
the inspected sample-entry code as `CodecType`, an inferred rule: Premiere
24.3-26.5.1 corpus projects store `avc1` for H.264, but the Premiere 12.1.1
`copy_and_paste_effects` project stores `AVC1` for a lowercase `avc1` file, and
Premiere 26.5.1 saved `HEVC` (1212503619) for every HEVC master of the
save-only HDR fixture (`oracle/M2/hdr/facts.md`), so HEVC writes that code. The same fixture gives the two source `OriginalColorSpace`
texts the reader accepts for HDR video (`schema::HdrProfile`: BT.2100 HLG and
PQ, 10-bit, with the SDR profile data); export writes them for a 10-bit
BT.2020nc HLG or PQ source and the BT.709 text for every other source. The
HEVC record of `feature_video_formats_strict.prproj` is an edited copy of an
H.264 record with `OriginalColorSpace` and `AlphaType` unchanged; the HLG
record of `feature_hdr_passthrough_strict.prproj` additionally carries the
saved HEVC `CodecType` and HLG profile.

The reverse operation generates a bounded `PremiereProjectXml` only inside the
writer path. `--check` validates the model and encoding, then drops the XML.
Execution writes the same XML bytes. Writer tests read generated projects back
and compare their semantics; conversion does not reparse its own output.
Generated XML is not stored in `PrProjectFile` or long-lived operation state.

```rust
use premiere_file::{premiere_to_tesseract, tesseract_to_premiere};

premiere_to_tesseract("project.prproj", "tesseract_output", None, false)?;
tesseract_to_premiere("tesseract_output/project.tsrct", "premiere_output", false)?;
```

Set `check` to `true` to complete conversion and validation without writing.
Premiere-to-Tesseract writes exactly `project.tsrct`. A sequence GUID may be
omitted only when the native project contains exactly one selectable sequence;
otherwise select one with `--sequence <GUID>`. Run
`tsrct-conv inspect INPUT` to list every selectable sequence, including
nested sequences, without probing media or creating output.

`tesseract_to_premiere` writes a 30 fps sequence; `Premiere`'s
`ExportFromTesseract` takes `PremiereExportOptions { frame_rate }` to select
another [export rate](#export-rate).

Both operations return `Result<Vec<Omission>, ConversionError>`. An
`Omission`'s `kind` is `Approximated` for a feature that converted with the
nearest form, not exactly (a portable `DiagnosticKind::Approximation`), and
`Omitted` otherwise (a portable `Warning`, which also covers caveats).
Premiere-to-Tesseract returns omissions for source content it cannot convert.
It keeps valid occurrences in the selected sequence and its nested graph.
Explicit selection does not depend on successfully resolving unrelated sequence
topology, so malformed unselected nesting cannot block it. A selected cyclic placement is omitted
while valid sibling clips remain. Conversion fails when selection is absent or
ambiguous, the GUID is invalid, or the selected timeline cannot convert.
Tesseract-to-Premiere returns the document content that
export omits or approximates. Each list reports an omission once, in
first-report order, and holds at most 1024 omissions and 1 MiB of their record
and reason text. A list that reaches either bound ends with
`feature omission list: later omissions are not listed; one conversion reports at most 1024 omissions and 1048576 bytes of their text`,
and later omissions are not listed.
Neither operation replaces existing output.
Source and media identities are checked again before publication. The source
project is hashed before conversion, after validation, and after staging.
Media aliases are rechecked after conversion and before publication. The
Tesseract writer hashes media while copying it; conversion compares those
digests with the inspected sources instead of rereading archived payloads.
The archive is written directly into import-owned staging. The import removes
staging and its published file on ordinary failure. Publication
(`publication.rs`) hard-links the staged file
into the new output directory. Where the output filesystem cannot link (for
example FAT32, exFAT or an SMB share), it copies the file, with its staged
permissions, into an exclusively created destination and syncs it; any link
failure other than an existing entry or a missing path takes this copy, so a
genuine I/O failure is reported by the copy. Neither path replaces or follows
an existing entry, and a failed copy removes only its own partial file. The
copy takes only the standard permissions (the Unix mode or the Windows
read-only flag), not ACLs, extended attributes or ownership, and fails if the
filesystem refuses them. Only injected link refusals test the copy; it has not
run on a real FAT32, exFAT or SMB volume. A copy is visible at its final name
while it is written, and neither path syncs the output directory. Rollback
removes published files by path, so it can also remove a file that another
process put in place of one of them. A process crash can leave partial output.
Publication is not atomic, and there is no completion marker.
Failures preserve their underlying error.

The former XML, Source Text/Path/Appearance payload, occurrence/key count,
expanded-layer, retained-timeline and nested-read byte quotas are removed.
TSRCT project/metadata and shared FX track byte quotas are also removed.
Schema, framing, native field widths, cycle/depth and integrity checks remain.
Source Text and nested copies still materialize in memory; no bounded-RSS or
giant-file guarantee is implied. Older capped readers may reject larger output.
The former internal capacity report is no longer distributed; the safeguards
above remain in force, but its detailed probe evidence is unavailable here.

## Effect stacks

`PrVideoOccurrence::effects` holds a clip's standard (non-intrinsic) effects in
stack order, the order in which Premiere applies them, as typed `PrEffect`s
(`schema/effects.rs`): an enabled flag,
the inverse of native `Bypass`, typed static parameters, and the keys of
animated parameters. The parameter variant selects the native identity from
the schema table: match name, English display name, filter type and parameter
layout. Gaussian Blur (`AE.ADBE Gaussian Blur 2`, "Gaussian Blur (Legacy)"
in Premiere 26, Blur Dimensions Horizontal and Vertical), current Gaussian Blur
(`AE.Impact_Blur_FX`, Film Impact's uniform subset), Corner Pin
(`AE.ADBE Corner Pin`), Directional Blur (`AE.ADBE Motion Blur`,
"Directional Blur (Legacy)" in Premiere 26), current Directional Blur
(`AE.Impact_Directional_Blur_FX`), Levels (`PR.ADBE Levels`),
Brightness & Contrast (`AE.ADBE Brightness & Contrast 2`), Invert
(`AE.ADBE Invert`), Tint (`AE.ADBE Tint`), Premiere 26.5.1's Black &
White (`AE.ADBE Black & White`, a record without parameters), Ramp
(`AE.ADBE Ramp`), Mosaic (`AE.ADBE Mosaic`, "Mosaic (Legacy)" in
Premiere 26) and Transform (`AE.ADBE Geometry`) are modeled;
`convert/effects.rs` maps them to FX `gaussianBlur`, `cornerPin`,
`directionalBlur`, `levels`, `brightnessContrast`, for Invert a `levels`
with complementary outputs, for Tint and Black & White a `tintTritone`, for
Ramp a `gradientRamp`, for Mosaic a `mosaic`, and bypass to `EffectRecord`
`enabled`; a Transform is no effect record but the transform of the clip's
video under a stage group (below). The corpus has Gaussian Blurs with keyed and with
static Blurriness, but none sits on a clip that converts.

Current Gaussian Blur imports as the same editable FX `gaussianBlur`, with
Blurriness = Amount × 5.7 for the static value and each key. FX Blurriness
keeps its Legacy (AE) meaning: on the pinned chart, AME renders Amount `a` with
the edge spread of Legacy Blurriness 5.63a to 5.70a (gamma-2.4 fits) or 5.78a to
5.81a (encoded fits) (`premiere_isolated_film_impact_blur_26_5`).
The kernels differ, and resolution scaling is unmeasured. Import rounds these
FX values to 1e-9, so an exported value reimports unchanged. Uniform Blur must
be on, or Amount must be static and equal to Thickness; otherwise the blur is
directional and is omitted. Chromatic Aberration must be zero. Edge Behavior 1
and 2 map to repeated edges and a transparent exterior; mirror mode 0 is
omitted. Keys on controls other than Amount, and hidden values other than
Premiere 26.5.1's defaults, omit the effect, not its clip. Angle, Seed and
Thickness are not kept: with Uniform Blur on, Thickness and Seed leave the blur
unchanged, and Angle 45 widens it by 3 % (pinned probe). The Applied Version
stamp can differ. Film Impact 26.2 saves the same record without the hidden
ParameterIDs 8300 and 8301 (20 parameters); import reads that layout under the
same rules, selected by its count and checked by identity. Synthetic tests
derive that layout from the 26.5.1 record; no 26.2 save is in this repository.

Export writes every FX Gaussian Blur as the current Gaussian Blur, with Amount
= Blurriness / 5.7 for the static value and each key and Premiere 26.5.1's
defaults for the other 20 parameters (Uniform Blur on, Angle 0). A static value
or key above Blurriness 5700 (Amount 1000) omits the effect with its reason;
nothing is clamped, and the Legacy record is only read. The writer's 22-parameter layout
matches the Premiere-saved fragment. Premiere 26.5.1 reopens an export and
reads back Amount, both edge modes and Linear and Hold keys, and AME renders
its strength, keys, bypass and edge modes as written (export gate, same
evidence folder). FX does not keep a blur's native identity, so a Legacy source
exports as the current blur.

An `EffectParamSpec` binding names the FX parameter that a native parameter's
keys animate: Legacy Blurriness binds to the scalar `blurriness`, Film Impact
Amount to `blurriness` × 5.7, each Corner
Pin corner, a point, to two FX scalars (for example `upperLeftX` and
`upperLeftY`), the Directional Blur Direction and Blur Length to the
scalars `direction` and `blurLength`, the current Directional Blur Angle and
Amount to `direction` and `blurLength` × 1.6, the Brightness & Contrast
Brightness and Contrast to `brightness` and `contrast`, the Invert Blend
With Original to the `levels` scalar `outputWhite`, whose complement
`outputBlack` import keys alongside it, each Tint colour (Map Black To, Map
White To) to three FX channel scalars (for example `whiteR`, `whiteG` and
`whiteB`), the Tint Amount to Tint to `amount`, each Ramp endpoint (Start
of Ramp, End of Ramp), a point, to two FX scalars (`startX` and `startY`),
each Ramp colour to three channel scalars, the Ramp Blend With Original
to `blend`, one minus the native fraction, and the Mosaic Horizontal Blocks
and Vertical Blocks to `horizontalBlocks` and `verticalBlocks`. The
Transform bindings name the staged video's layer properties (or A4's
keyed-picture group for the measured Alpha-matte form) instead
(`positionX/Y`, `scaleY` and, under Uniform Scale, `scaleX` for Scale
Height, `scaleX` for Scale Width, `rotation`, `opacity`); its Skew and Skew
Axis bindings only let the reader read keys that
`PrTransform::ensure_convertible` rejects, and its Shutter Angle binding
names the composition's `shutterAngle`, which takes the first key's value.
Keys on any other parameter omit the effect. A checkbox parameter's spec
name is blank: the corpus Gaussian Blur's Repeat Edge Pixels is saved as
`<Name> </Name>` and Premiere 26.5.1's Mosaic Sharp Colors and both Transform checkboxes
(Uniform Scale, Use Composition's Shutter Angle) without a `Name` element,
and `EffectParamSpec::accepts_name` accepts either for such a spec (the
writer writes the spec name when it is not empty). The reader reads a
bound parameter's keys with the Motion scalar or point key reader into
`PrEffectParamAnimation`s; import and export map them to FX `effectProperty`
keyframe tracks with the Motion key conversions and export rules, and the
writer writes them with the Motion key writer. A keyed parameter ignores
`StartKeyframe` and the cached `CurrentValue`; its static value is its first
key's, which export writes as `StartKeyframe`, because before a later first
key AME renders that key's value (Oracle run D).

Corner Pin corners are normalized to the clip's own frame, origin top left and
y down, in Premiere (Oracle run E1, on a portrait clip in a landscape sequence)
and in FX, so their values convert unchanged; off-frame values stay. E1
measured Premiere's warp as a perspective (projective) one, as FX draws it. A
keyed corner becomes two FX tracks with the point keys' times and easing. Only
straight spatial paths convert: every spatial
control point, a key plus its tangent, must lie within 1e-9 of the clip frame
of the segment to the next key, because Premiere writes float noise into the
automatic tangents of straight paths (1.4e-17 in the corpus spin of
`mixkit-41`). On a straight path a corner moves by its temporal easing alone,
so the tangents are not kept. A quad that is degenerate or non-convex at any
time omits the effect, because no perspective warp maps the clip frame onto
it: at the static corners and at any corner key, and between two consecutive
key times of any corner, where each corner stays on one point or moves along
its straight segment. Between keys the
check is exact when every moving corner is Linear or all share one Bezier
easing between the same keys, because each turn of the quad is then a
quadratic in one parameter. Otherwise it bounds each corner's eased progress
on its own, overshoot included, and requires a strictly convex quad at every
combination of these bounds: sound but conservative, so a Corner Pin whose
corners move with different easings can be omitted although its quad stays
convex. Turns are compared with zero in f64, without a tolerance. On export, a
corner's x and y tracks become one native
point track only when their key times and easing match; when only one
coordinate has a track, the other keeps its static value at every key, which
is exact because the corner then moves on a straight line. Other track pairs,
Bezier easing between two keys on one point and the quads above omit the
effect. The corner records follow the corpus Premiere 12.1 Corner Pins
(`PointComponentParam` 3 with `ParameterControlType` 6, the Motion writer's
point form) with linear spatial keys. Premiere 26.5.1 opens an edited export
of `premiere_isolated_corner_pin` with these records without a warning, reads
its corners and keys back as written, and AME renders it as edited (evidence
in `premiere_isolated_corner_pin`).

The Directional Blur's Direction and Blur Length are values in the clip's own
frame, which Premiere blurs before Motion, while FX `directionalBlur` blurs in
composition space. Both map through the host clip's static similarity
(`ClipToComposition`); a host that is not one omits the blur with its form.
A blur on a stage group is omitted in both directions
(`STAGED_DIRECTIONAL_BLUR_REASON`): no Adobe case verifies its map through the
group's Motion, which supporting it needs. For the same reason export omits a
blur on a nest placement, or on a clip inside a nest whose placement writes
Motion (`NESTED_DIRECTIONAL_BLUR_REASON`). The ledger lists the forms, the
measured length factor (one clip) and E2's kernel measurements.

The current Directional Blur blurs in the clip's frame too: a probe clip at
Scale 50 and Rotation 30 blurs along its own axis, as a Legacy one does. Its
Angle is a Legacy Direction and Amount × 1.6 a Legacy Blur Length: on the
pinned chart at 1080p, AME renders Amount `a` with the edge spread of Legacy
Blur Length 1.59a to 1.63a (gamma-2.4 fits), and Amount 35 with a transparent
exterior as Legacy Blur Length 55 to a mean difference of 1.7/255
(`premiere_isolated_film_impact_directional_blur_26_5`).
Import converts both, static and keyed, and maps them as a Legacy blur's.
Chromatic Aberration must be zero. Edge Behavior 2 is the Legacy blur's
transparent exterior; Premiere's default 1 repeats edges and imports as the
same blur, which differs only near the frame edges (ledger). Mirrored edges
(0), keys on other controls and hidden values other than Premiere 26.5.1's
defaults omit the effect, not its clip. Seed is not kept (Seed 1234 leaves the
probe's blur unchanged). Its 20 parameters are the current Gaussian Blur's
without Thickness and Uniform Blur, with the same hidden values (Oracle run A
catalog).

Premiere renders a clip's `VideoComponentChain` in descending component
`Index`: the component at Index 0 renders last. Materialized intrinsic Motion
and Opacity sit at the lowest Indexes, so they apply after every standard
effect. The stack order, the order in which the effects apply and which FX
keeps, is therefore the reverse of the chain's `Index` order. The reader and
the writer share this one mapping (`schema::chain_render_order`, used by
`split_chain` in `reader/effects.rs` and by `placement_records` in
`writer/tracks/video.rs`).
This is Adobe evidence from Oracle run C6 (F24): in the F1 render
(`premiere_isolated_stage_order_26_5`) a Crop or Linear Wipe at Index 0 with a
Gaussian Blur at Index 1 rendered a sharp edge, so the blur applied first, and
the swapped layout a soft one; a probe that swapped the component `ID`s
rendered the same frames, so `ID` does not order the render.

Levels is Premiere's own filter (`VideoFilterType` 1), with (RGB), (R), (G)
and (B) rows of Black Input, White Input, Black Output and White Output
Level and Gamma. Only the master (RGB) row maps: its levels to FX `levels`
`inputBlack`, `inputWhite`, `outputBlack` and `outputWhite` unchanged, and
its Gamma, stored in hundredths, to `gamma` (an `EffectParamBinding::Integer`
with divisor 100); keyed master levels take the keyed blur's key path.
Premiere 26.5.1 saves Levels (Oracle run E4) without `Bypass` or
`Intrinsic`, with every `ParameterID` −1, so its parameters are identified
by `Name`, and with a `PremiereFilterPrivateData` that repeats the 20
`StartKeyframe` values as little-endian u16s; an empty copy names an earlier
identical blob by `BinaryHash`, as Premiere does for Motion. Import omits
the Levels when a (R), (G) or (B) row is not neutral (0, 255, 0, 255, 100) or
is keyed, the private data has another length or disagrees with a
`StartKeyframe`, a `Name` is missing, repeated or unknown, a `ParameterID` is
not −1, or `Bypass` or `Intrinsic` is present (a bypassed Levels is
unverified). Both directions omit it at input black at or above input white
or output black above output white, forms that no Adobe render measured; a
keyed level is bounded by its keys, Bezier overshoot included, so two keyed
levels of a pair are checked conservatively. Export checks the FX values and
keys and then their rounded values, so rounding can neither hide nor make such
a form. Export rounds each value and key
to a whole level or hundredth of Gamma, omits a value outside Premiere's range
(never clamped) and a disabled Levels, and writes the Premiere 26.5.1 form:
`VideoFilterComponent` 9 with `Component` 7, no flags, `ParameterID` −1,
`IsTimeVarying` `true` on a keyed level as E4 saves it, and the private data of the written `StartKeyframe`s under a random
`BinaryHash` (inferred). AME renders Premiere's Levels as the FX formula on
clipped 8-bit values, Gamma as the exponent 1/(Gamma/100) (Oracle run E4).
Premiere 26.5.1 opens an edited export of `premiere_isolated_levels` with
these records without a warning and reads its values, flags and keys back as
written, so it accepts the private data (not inspected directly), and AME
renders it as edited (a scoped export gate; evidence in
`premiere_isolated_levels`). The Premiere UI was not
inspected, and the case stays `structural_only`.

Brightness and Contrast convert unchanged, from -100 to 100 in Premiere;
export omits an effect with an FX value outside that range, which FX allows
for brightness, instead of clamping it. FX draws the effect with libpag's
After Effects approximation, not the per-channel model that Oracle run E3 fits
to Premiere's render; the ledger gives that renderer gap, with E3's numbers, as
the inferred cause of the measured score failure.

Invert converts only for Channel 0 (RGB), as a `levels` with neutral inputs
and Gamma, output white 255 · Blend / 100 and output black its complement,
which renders (1 − b)(1 − v) + b·v, the blend of the inverted frame with the
original that Oracle run E5 fits to Premiere's render on encoded values,
exactly by algebra on the same clipped encoded input; a keyed Blend keys both
outputs at the same times with the same easing. Every
saved Invert carries a `PremiereFilterPrivateData` of uninitialized process
memory (`EffectSpec::opaque_private_data`), which import accepts and ignores
and export does not write. Export writes a `levels` of exactly that form back
as an Invert, keyed complements included, except the static identity (output
white 255, a Blend of 100 that shows the original), which stays a Levels; any
other `levels` exports as `PR.ADBE Levels`. Premiere 26.5.1 opens an edited
export of `premiere_isolated_invert` with these records, without the private
data, without a warning and reads its values and keys back as written, and
AME renders it as edited (a scoped export gate over Channel 0 with static
Blends and Linear and Hold keys; a bypassed Invert and Bezier keys are not
Adobe-verified; evidence in `premiere_isolated_invert`).
Another or a keyed Channel and a
Blend outside 0 to 100, static or on a key, omit the effect. Against the
pinned AME render of `premiere_isolated_invert` a fresh conversion measures
below the standard floors (0.978144 mean, 0.939703 minimum on the keyed ramp):
the Blend fitted to the render agrees with the key model to within 0.002
(0.998 against 1.000 through f165 and after f225; 0.252 against 0.253 at
f179), and the remaining difference on saturated bars is **inferred** from
E5's fit to be Premiere's blend with the unclipped original, plus this
source's edge and texture gap, so the case stays `structural_only` (the
ledger has the numbers).

Tint and Black & White convert as FX `tintTritone`: Oracle runs E6 and E7 fit
Premiere's renders of both to Rec. 601 luma (0.2993, 0.5883, 0.114) of the
encoded values, mapped from Map Black To to Map White To and mixed with the
original by the Amount, which is the `tintTritone` shader on the same clipped
encoded input; the remaining difference is the unclipped-source gap of this
source (E6 §4, mean ≤ 2.95 levels on the clipped input). A native colour is
a u64 of four 16-bit channels, alpha, red, green and blue from the high end,
with the 8-bit value in each channel's high byte (every one of the 1,050
corpus channel values has a zero low byte); `PrColour` reads it as three
8-bit channels and ignores the alpha (0 on Tint's defaults, 0xff00
elsewhere; the default white renders white, E6 clip A), and import writes
each channel as its share of 255, all seven `tintTritone` fields explicit.
Colour keys are read in the 8-field scalar key form with the value parsed as
an integer, because a double cannot hold every 64-bit colour exactly; a
keyed colour becomes three channel tracks at the same times with the same
easing, as Premiere interpolates a Linear segment per channel (E6 clip E). A
Black & White has no parameters and imports as Tint's defaults, a full
grayscale (E7). Export writes every `tintTritone` as a Tint, a Black & White
included, with each channel rounded to 8 bits (at most 1/510) and alpha
0xff00; a keyed colour needs all three channel tracks keyed at the same times
with the same easing, and the Amount, static or keyed, must lie in 0 to 100.
Omitted with a reason: a colour with a nonzero low byte, a Bezier segment
between colour keys (Premiere's colour Bezier is unverified) in either
direction, an Amount outside 0 to 100, a `tintTritone` without one of its
seven values (FX renders the shader default, which the converter does not
assume), a channel outside 0 to 1, and channel tracks that are not keyed
together. The corpus's older `PR.ADBE Black & White` (`VideoFilterType` 1,
one unnamed `ArbVideoComponentParam`) is another effect and stays unknown.
Premiere 26.5.1 opens an edited export of
`premiere_isolated_tint` with these records without a warning and reads its
colours, Amounts and keys back as written, and AME renders it as edited (a
scoped export gate over static colours, a static and a keyed Amount with
Linear and Hold keys and Linear Map White To keys; its clip A is the Black &
White export form; a bypassed Tint, Hold and Bezier colour keys and Map Black
To keys are not Adobe-verified; evidence in `premiere_isolated_tint`
and `premiere_isolated_black_white/`). Against the pinned AME renders of
`premiere_isolated_tint` and `premiere_isolated_black_white` fresh conversions
measure below the standard floors (0.963428 / 0.942453 and 0.963178 /
0.956250): Tesseract renders the FX model within 0.6 levels at every probed
frame, and the difference on the saturated bars (up to 14 levels at Amount 50)
is **inferred** from E6's and E7's fits to be Premiere's use of the unclipped
source, plus this source's edge and texture gap, so both cases stay
`structural_only`; the pinned `premiere_isolated_effect_stack` render, whose
Tint now converts, scores 0.996817 / 0.991971 and is enrolled as a gate (the
ledger has the numbers).

Ramp converts as FX `gradientRamp` for a linear, axis-aligned ramp on a clip
whose frame is the canvas. Oracle run E10 fits Premiere's render to the two
colours mixed on encoded values along the axis from Start of Ramp to End of
Ramp (mean 0.26–0.75 levels on the unblended frames; a linear-light mix is
31–53 levels off) and Blend With Original to a mix with the original, which
is the `gradientRamp` shader with `blend` = 1 − Blend With Original. The
shader measures the ramp in the layer frame's UV and Premiere in clip pixels,
so the two agree only where those coincide: the E10 probe measured a corpus
diagonal ramp within 0.20–0.25 levels of the pixel model and 50.1 levels
from the UV one, and a radial ramp 0.18–0.19 against 13.0 (an ellipse in
UV). Hence only a ramp whose endpoints share x
or y at every time converts, and only on a clip at identity static Motion
without Position, Scale or Rotation keys whose source frame is the
sequence's (`wipe_frame_reason`, the Linear Wipe's frame rule; Opacity keys
and a pivot moved with its position leave the frame); a Ramp on a stage
group's video or a nest placement is omitted, because no Adobe case
verifies the frame the shader measures in there. Endpoints are clip-frame
fractions, possibly outside the frame, and import writes them as frame UV
unchanged; colours are `PrColour`s (8-bit channels, alpha ignored and
written 0xff00, as for Tint); Ramp Shape must be 0 (linear) and Ramp
Scatter 0 (FX has no scatter), and export writes both. Keys: Blend With
Original keys map to `blend` by the same affine involution, so their easing
carries over; an endpoint's point keys become an x and a y track, and a
keyed coordinate must keep the axis at every key while the two endpoints
stay at least `PrRamp::MIN_LENGTH` = 0.0032 of the frame apart over their
keys, Bézier overshoot included (`PrRamp::ensure_aligned`): the FX shader
divides by `max(dot(axis, axis), 1e-5)` in f32, so a ramp shorter than
√1e-5 ≈ 0.00316 of the frame is stretched over √1e-5 instead of its own
length (a 0.001 ramp renders a tenth of its gradient at its end), and the
bound is rounded up so that f32 rounding cannot bring a converted ramp to
the floor; Premiere has no such floor. Colour keys follow the Tint rules.
Omitted with a reason: a radial ramp, a scatter, a diagonal, degenerate or
too short axis, keys that leave the axis or let the endpoints meet or come
too close, a colour with a nonzero low byte,
a Blend outside 0 to 1, a `gradientRamp` without one of its twelve values,
a `shape` other than 0, and a host whose frame is not the canvas. `1 − (1 −
v)` is computed unrounded, so a Blend returns with a small absolute
roundoff: at most 2⁻⁵³ (the rounding of `1 − v`), which is exact for 0.5,
0.25 and the fixture's float32 0.3, one ulp for 0.3 as an f64 and, for a
`v` below 2⁻⁵³, all of `v` (a 2⁻⁵⁴ blend returns as 0); Premiere stores the
fraction as a float32 (2⁻²⁴ relative), far coarser than that. Premiere 26.5.1
opens an edited export of `premiere_isolated_ramp` with these records without
a warning and reads its points, colours, Blend and keys back as written, and
AME renders it as edited (a scoped export gate over static points and colours,
a static Blend, Linear and Hold Blend keys and Linear End of Ramp keys; a
bypassed Ramp, Bezier Blend keys and colour or Start of Ramp keys are not
Adobe-verified; evidence in `premiere_isolated_ramp`). Against
the pinned AME render of `premiere_isolated_ramp` a fresh conversion measures
below the standard floors (0.898756 / 0.834071) although its pixels are
within 0.24–0.68 levels of Adobe's on every unblended frame (1.9–2.9 on the
blended ones, the **inferred** unclipped-source gap): the pure-ramp clips
score lowest, which is **inferred** to be the similarity metric's structure
term on a featureless gradient, so the case stays `structural_only` (the
ledger has the numbers).

Mosaic converts as FX `mosaic` with Sharp Colors on. Oracle run E8 (23 AME
frames of `premiere_isolated_mosaic`) shows flat blocks equal to the source
pixel at the floor of the block's centre (mean 0.09–1.42 levels, 89–98 % of
blocks within 3), a grid from the top-left corner with fractional block
widths when the counts do not divide the frame (probe P2: a 7 × 5 grid's
edges at k·1920/7 and k·216), and Hold count keys switching exactly at the
key's frame; the `mosaic` shader lays the same grid over the layer's content
rect (`floor(uv · blocks) / blocks + 0.5 / blocks`) and samples the block
centre, so the two differ by at most one source pixel per block. Block
counts are fractions of the frame in both engines and Premiere renders clip
effects before Motion, so a Mosaic converts on a media clip at any static or
keyed Motion (**measured** by the export gate on a
clip at uniform Scale 50, whose 32 × 18 blocks are 60 × 60 source pixels drawn
as 30 × 30 canvas pixels, and **inferred** for other Motion); a Mosaic
on a stage group's video or a nest placement is omitted, because no Adobe
case verifies the frame the shader spans there. Omitted with a reason
(`PrMosaic::ensure_convertible`, the reader's and the exporter's): Sharp
Colors off (Premiere then averages each block, which E8's probe P1 measured
29.5–35.1 levels from the centre sample), a count that is not a whole number
from 1 to 4000, static or on a key (no rounding), Linear or Bézier count keys
(the FX renderer would draw fractional counts between them, where Premiere's
stepping is unmeasured), a keyed Sharp Colors, another parameter layout, and a `mosaic`
without `sharpColors` or with it false. Premiere 26's default "Mosaic" is
Film Impact's `AE.Impact_Mosaic_FX`, an unknown effect. Premiere 26.5.1 opens
an edited export of `premiere_isolated_mosaic` with these records without a
warning and reads its counts, Sharp Colors and Hold keys back as written, and
AME renders it as edited (a scoped export gate over static counts, Hold count
keys and a Scale 50 host; a bypassed Mosaic is not Adobe-verified; evidence in
`premiere_isolated_mosaic`). Against the pinned AME render of
`premiere_isolated_mosaic` a fresh conversion measures below
the standard floors (0.959402 / 0.931278): the grids coincide and 76–95 % of
the blocks agree within 3 levels; the rest (0.73–4.67 levels mean per frame,
single blocks up to 135) is inferred, from model fits on seven of eight probed
frames, to be the shader's 2 × 2 bilinear sample at an integer block centre
against Premiere's single floor pixel on source edges — clip A's 120-px blocks
fit the bilinear sample better and stay unresolved. No converter defect was
identified; the case stays `structural_only` (the ledger has the numbers).

Transform (`AE.ADBE Geometry`) converts as the transform of the clip's video
under a mask-less stage group. Oracle run E11 (34 AME frames of
`premiere_isolated_transform_26_5`, Premiere 26.5.1) measured Premiere's
Transform as an affine in the clip's source frame applied before Motion
(T10: clip E's Position offset is displaced along the 30° Motion axes,
(239.6, 0.1) px) that clips nothing to that frame (T2: clip E shows 476,960
content pixels outside the Motion-only rectangle), so the FX model is the
existing stage group (`premiere_to_tesseract.rs`, "Premiere stage N")
without a mask, carrying the clip's Motion, Opacity and their keys, whose
video takes the Transform: `anchorPoint` and `position` are the effect's
points times the source frame (T7: B's anchor 1440:540 lands on 960:540
within 0.75 px), `rotation` the effect's (T4: +30 renders +29.998°
clockwise), `scale` Scale Height on both axes under Uniform Scale (T3: B
renders 0.49997 × 0.49995 with Scale Width 100 saved) and `[Width, Height]`
otherwise (**inferred**: E11 has no non-uniform clip), `skew` the effect's
and `skewAxis` the Skew Axis − 90° under a nonzero Skew: at rotation 0
Premiere shears by R(90° − axis)·Shear·R(axis − 90°) (the slice 22 export
gate measured Skew 30 at Skew Axis −30 within 0.0003 per coefficient; T5's
Skew Axis 45, within 0.00026, fits this form as it fits the negated axis)
and FX by R(−axis)·Shear·R(axis). Under Skew 0, which shears nothing at any
axis, `skewAxis` is 0 and a nonzero Skew Axis is reported ("Skew Axis N
without Skew is not retained (no render effect)"). The effect's Position,
Scale Height, Scale Width and Rotation keys become the video's
`positionX/Y`, `scaleX/Y` (both axes from Scale Height under Uniform Scale)
and `rotation` tracks on the clip's clock from its source In, as Motion keys
do (T9: D reads back 100/150/200/200/50 and 0/45/90 with the Hold jump at
8.000 s; the `premiere_keyframes` guard evaluates the same values and the
scene bake places D at 200 %/45° and E's displacement along the Motion
axes). The source frame must be the canvas (E11 measured 1920 × 1080
on 1920 × 1080 only). Effects that apply before the Transform stay on the
video under their staged-host rules; the ones that apply after it are
omitted. Parameters that FX expresses only approximately convert
with one warning each (`PrTransform::approximations`, the importer's and the
exporter's; the ledger has the measured errors): Opacity other than 100 or
keyed becomes the video's `opacity` (sRGB for Premiere's linear light, T6);
the composition's shutter angle off with a Shutter Angle above 0
becomes the video's `motionBlur` and the composition's one shutter at that
angle with phase 0 (T12), the first blurred clip's angle for the
whole composition, a keyed angle at its first key; Sampling 1 becomes
bilinear sampling; a nonzero Skew with a Rotation, or with axes that
are not equal at every time (equal only under Uniform Scale or with equal
statics and identical Scale Width and Scale Height tracks, never by equal
key ranges alone), converts with FX's composition (T5's bound). Scale Width
keys under Uniform Scale, which do not render (inferred from T3's static Scale
Width), are not imported, an omission (`PrTransform::unimported_scale_width_keys`).
Omitted with a reason
(`PrTransform::ensure_convertible`, the reader's and the exporter's, and the
reader): keyed Anchor Point, checkboxes or Sampling, Skew or Skew Axis keys
(static skew only was measured), a bypassed Transform, a
second active Transform on the clip, convertible or not: the reader counts the
active records before it drops the unconvertible ones, so both are omitted
(E11 measured one), a Transform with a Crop, Linear Wipe or Opacity mask
(the mask keeps its stage), and a Transform with Track Matte Key outside the
measured A4 form described below, media that is not sequence-sized, and
adjustment, nest, graphic, still and Color Matte hosts. `AE.ADBE Geometry2`
imports its measured centered positive uniform zoom as a Corner Pin, including
Scale Height keys with their original temporal easing. The admitted form has
full Opacity, zero Skew/Skew Axis/Rotation/Shutter Angle, composition shutter
selected and bilinear Sampling. Other Geometry2 forms remain diagnosed effect
omissions. The Corner Pin also renders on an adjustment composite; export uses
the existing Corner Pin writer, preserving the editable warp rather than the
Geometry2 effect identity.

A4 import admits a single Linear Position Transform with Alpha Track Matte Key
in either saved effect order. Both native orders move the fill and its matte
window together. The keyed-picture group carries the Transform and its Position
tracks, with identity children, using the existing matte reparenting and local
clocks. This form requires default values for other Transform parameters,
default clip Motion, opaque normal blending, speed 1, an unshared canvas-sized
static still matte with no active standard effects and exactly matching ranges.
The reader checks native chains before dropping unmapped effects. A matte outside
this form denies the new Transform admission: the Transform is reported as a
Feature omission and the existing keyed fill and matte conversion are retained.
The ordinary invalid-matte and source-size guards still apply. Other masks,
key modes, matte channels and values remain outside this admission; Geometry2
remains unsupported. The public A4 source and independent AME reference are
pinned in `tests/manifest.json`; export behavior is unchanged.

Export recognises a Transform stage by import's group name
(`STAGE_GROUP_NAME`, "Premiere stage "), because a Premiere nest of one moved
clip imports to the same shape and keeps exporting as a nest. Such a
group with no masks and one video
child exports as one clip: the group's Motion, Opacity and keys, the video's
effects, then one Transform last in the chain (`E-chain.xml`: Motion at
`Index` 0, Transform at 1) built by `effects::export_transform_stage`, the
same construction `unsupported_stage` runs to classify the group: points
divided by the source frame, Uniform Scale on when the axes are equal
statically with one shared track, Skew Axis `skewAxis` + 90° (0 without a
skew; beyond the native bounds, the equivalent from 0° to 180°, as the
shear repeats every 180°), the video's opacity as Opacity, bilinear
Sampling, and the composition's shutter angle unless the video's motion
blur exports as the Transform's own shutter (the checkbox off at the
composition's angle, when the composition blurs within 0–360°; a nonzero
shutter phase is reported), with the approximation warnings of
import; the video's Position, Scale, Rotation and Opacity tracks as the
parameters' keys through the Motion key exporters. A stage that the rule
rejects (Anchor Point, Skew, Skew Axis or 3D keys, 3D fields, a value outside
the native bounds, the identity without keys) exports as a nest exactly as a
mask stage that one clip cannot carry does, where the nest's own
reports name what its Motion cannot carry. Against the pinned AME render of
`premiere_isolated_transform_26_5` (a long-term Asset, freshly resolved into
an empty cache; the fixture is the Oracle's path-rebased copy of Premiere's
save, the file AME rendered) a fresh conversion measures 0.961480 / 0.850644:
clips A and F render their approximations (A 0.900–0.908 for the sRGB
blend, F 0.851 on its first, unblurred frame and 0.966–0.973 after); clips
B, C, D and E render Premiere's geometry (affine fits within 0.0018 per
coefficient, 0.36 px and 0.03° of E11's Adobe fits) with pixels within
0.28–0.97 levels mean of Adobe's, yet 8 of their 29 samples score under
0.98, the Ramp case's metric finding (**inferred**); the case stays
`structural_only` (the ledger has the numbers).

`read_occurrence` splits an occurrence chain once (`reader/effects.rs`):
intrinsic components and active Crop or Linear Wipe go to their existing readers; other
standard effects are read after the occurrence converts. A component without `Intrinsic` reads as a
standard effect, and one without `Bypass` as active. Premiere 26.5.1 saves its
Gaussian Blurs without both flags, and AME renders them as active effects
(Oracle run D); the derived `premiere_isolated_text_point` fixture (a
py-premiere record on a 26.3 scaffold) also omits them, while real 24.3
(`color`) and 25.5 (`practice_files_transcription_magic`) projects write both. `Unique` `false`,
seen once on a 25.5 component, is accepted. Its meaning is unknown, so any other
value omits the effect. Effects match by match name and parameters by
`ParameterID` and `Name` (Levels by `Name`); record versions are not checked.

An active effect that changes what its clip covers or its transparency omits
the whole occurrence: the Keying effects and Radial Wipe in
`COVERAGE_EFFECTS`, and any effect whose `SubComponents` reference a mask in
either saved form (`AE.ADBE AEMask`, `AE.ADBE AEMask2`; `mask_match_name`,
JRB-2028; a bypassed one is omitted as a feature by the effect reader's child
list). Each entry is a prefix of a match name or
English display name: the observed Radial Wipe match name, an
`AE.ADBE Legacy Key ` prefix inferred from Track Matte Key's, and the display
names of the eight keyers whose match names are unobserved, so a localized
project keeps such a clip and reports the keyer as an unknown effect. An
active Track Matte Key matches that prefix but is the clip's mask
(`TRACK_MATTE_KEY`, JRB-2023): the chain reader skips it before the list
applies and the Motion reader reads it.
Premiere renders a clip without its bypassed effects, so a
bypassed one keeps its clip and is reported as a bypassed effect; a missing
`Bypass` reads as active and an invalid one counts as active. Wipe transitions
between clips belong to JRB-1985. Supported intrinsic Opacity continues through
its own reader; Vector Motion
(`AE.ADBE Graphic Group`) still omits the occurrence.

A non-default active Crop, a supported cardinal Linear Wipe, a static mask on
the intrinsic Opacity (`schema/mask.rs`, `reader/mask.rs`; JRB-2028) or a
Track Matte Key (`PrTrackMatte`, `reader/effects.rs::read_track_matte`;
JRB-2023) is a clip's one mask. Premiere applies the standard effects in stack
order (descending chain `Index`), Crop, wipe and Track Matte Key among them,
and then Motion and Opacity (Oracle run C6), so an Opacity mask applies after
every effect and the reader counts them all as applied before it; FX applies a
layer's masks and track matte before its effects.

A Track Matte Key (`AE.ADBE Legacy Key Track Matte`, the 12.x 7/5 and 14.4 8/6
record forms of the corpus and 26.5.1's 9/7 form with v10 parameters, three
static parameters, no private data; Matte None is 4294967295) keys the
clip's source frame by one channel of another video track's output at the same
time: Composite Using 0 is Matte Alpha and 1 Matte Luma (Rec. 601 weights on
encoded values, fixture G2, which FX's Rec. 709 `luma` approximates: the
converter reports it on the clip, `LUMA_MATTE_APPROXIMATION`), and Reverse with
Matte Alpha is one minus the alpha (`PrMatteChannel`, fixture G3a); Reverse
with Matte Luma fails closed (G3b: Premiere gives the matte clip's zero-luma
exterior full coverage, FX's `lumaInverted` none). Premiere does not draw the
matte clip while it is consumed (G1). Its Matte names the matte track by the
persistent `Track/ID`, not the `Index` (`horror_title` stores 7 for the track
at `Index` 5 after a deletion), so `read_tracks` maps every kept track's ID to
its position among the kept tracks (the model's track index; an omitted track
leaves no gap, so the native `Index` would name the wrong track) before it
reads any item, and `read_placement` resolves the key to
`PrTrackMatte { track_index, channel }`. Once every item and nest is kept,
`resolve_track_mattes` drops each keyed placement whose matte track is not
strictly above it or does not hold exactly one enabled item or nest spanning
exactly the placement's range, or whose matte item has a Crop, wipe, Opacity
mask or key of its own (`schema::check_track_matte`, which the model
validation shares, so export never writes a keyed clip without its matte),
and each keyed placement that is disabled or on a muted track
(`DISABLED_KEYED_PLACEMENT`: FX draws a hidden layer's matte source as
content, and whether Premiere draws a disabled fill's matte is unmeasured).
An active key consumes its matte track over the clip's range whether or not
the clip converts (fixture G1b): `read_tracks` records each active key's matte
track and range before it reads the placement (`claimed_matte_track_ids`, from
the key's static Matte alone, so a key whose other controls do not convert
still consumes its matte), and `consume_claimed_mattes` drops the matte clips over an omitted clip's range
that no kept clip keys, naming the omitted clip; `omit_unconsumed_mattes` does
the same in the converter for a clip omitted while it converted.
A nest placement keeps its key (`PrNestOccurrence::track_matte`); a still or
Color Matte fill is omitted through the classifier (`OccurrenceEdit::TrackMatte`).
The matte item converts as its ordinary layer (a video, still, Color Matte
rectangle, graphic or nest). A Color Matte that a key uses gets no head Cross
Dissolve (Legacy) ramp (`import_transitions` checks `matte_consumers`): a
dissolving matte source is unmeasured. Import puts the FX `trackMatte` on the fill's own
video or nest group when the clip is at default static Motion with no effect
applied before the key (flat: FX evaluates a sibling source at its own canvas
position, as Premiere keys the sequence-sized frame); otherwise the clip
stages (below) and the matte moves under the stage group as its direct child
on the group clock, so the group's Motion moves it with the video, as
Premiere's Motion moves the keyed picture (fixture G5). A matte at unit speed
keeps its source times there (`into_stage` changes only the parent and the
range); a matte clip at another speed or with Time Remapping has playback
keys on the sequence clock, which the group clock would misread, so such a
clip is omitted with its reason (`STAGED_RETIMED_MATTE_REASON`), as export
omits a stage group whose child source has playback keys.

The Opacity component names its mask in `SubComponents`, one record in one of
three saved forms (`schema/mask.rs`, one parameter table each): the corpus
`AE.ADBE AEMask` v7/c5 (13 parameters) and v8/c6 (15) forms, and the
`AE.ADBE AEMask2` v9/c7 form with 35 parameters and no `Bypass` or
`Intrinsic` that Premiere 26.5.1 saves every mask in (fixture
`feature_opacity_masks_26_5_strict`, G5: it accepted and upgraded XML-written
v8/c6 masks; the export gate measured the same upgrade of this writer's v7/c5
records, values and vertex bodies intact:
`premiere_isolated_opacity_masks_26_5`). Every component
reader that decodes a `VideoFilterComponent` must accept or
reject `sub_components`: Motion, Crop and Linear Wipe, the graphic Text, Shape
and Vector Motion components and a graphic's clip Opacity reject one with a
named reason (a mask on a graphic is JRB-2083). The mask reader converts one
static mask whose every parameter other than the Path, Feather, Opacity,
Expansion (at 0) and Inverted holds the one value the saves show
(`MaskControl::Default`, `MaskParamRole::Binary`; in the 26.5.1 form mask
Position and Anchor Point must be equal, `MaskControl::Centre`); its Mask Path
(the corpus `2cin` value, or the 26.5.1 envelope around the same 28-byte
vertex bodies, read through `binary_param` with `BinaryHash` deduplication)
becomes a paint-less `ShapeLayer` guide in source pixels (the fixture's G1),
the Feather the FX feather on both axes (reported as an approximation in both
directions, `MASK_FEATHER_APPROXIMATION`), Mask Opacity the FX mask opacity
and Inverted `inverted`, which converts only at Mask Opacity 100
(`PrMask::validate`, both directions: Premiere scales the inverted coverage,
FX inverts the scaled coverage). A bypassed corpus-form mask converts no mask
and is reported; a 26.5.1 `Bypass` is unobserved and omits the occurrence. The classifier (`OccurrenceEdit::OpacityMask`) omits the
occurrence of a still, Color Matte or nest that carries one, and
`mask_boundary` omits a clip with an Opacity mask beside a Crop or wipe, or a
Track Matte Key beside any of the three.
The reader keeps converted effects on one side of the mask and counts those
that apply before it (`effects_above_mask`), which have higher chain Indexes
than the mask. Converted effects on both sides are omitted
with `MASK_EFFECT_ORDER_REASON`, and the clip keeps its mask.
`PrVideoOccurrence::mask_boundary` (`schema/mod.rs`) then decides:

- Crop with a wipe, or a wipe on media that is not sequence-sized: the
  occurrence is omitted.
- A Crop with no converted effect applied before it: one video layer. Its
  guide repeats the video's transform and Motion keys, so the Crop stays in
  the video's frame.
- A wipe with no converted effect applied before it, on a clip at default
  static Motion: one video layer, whose sequence-sized guide is then the clip
  frame.
- Otherwise the clip stages: a `GroupLayer` takes its range, Enable, Motion,
  Opacity and their keys and the mask. The video under it keeps the effects
  that apply before the mask, its source range and playback on the group
  clock, and the guide sits beside it; both are at the identity in the video's
  frame. The group carries no effects, so the effects that apply after a wipe
  staged for Motion are reported. Guide and mask ids are as for a flat clip
  and the group takes the next one, so each staged clip shifts later ids by
  one.

Unsupported active wipes omit the occurrence through the existing wipe reader.
Bypassed Crop or wipe keeps its clip and is reported as an unmapped bypassed
effect. Export checks every mask once (`canonical_mask`), so a clip never exports
without its mask. The check reads every authored key, including keys that
export otherwise omits. The mask is one Add mask without keys, expansion or
unequal feather axes. The guide's shape, not its name, selects the check: a
black sequence-sized rectangle whose only animation is one Scale track is a
wipe guide, unless a flat guide's video has that track too; any other
rectangle guide is a Crop guide. A Crop guide is a square-cornered, non-inverted, fully
opaque rectangle whose keys are its video's: a flat guide repeats the video's
transform and transform keys or, unkeyed beside an unkeyed video with both at
Scale 100 and Rotation 0 without skew or 3D, sits at another anchor and
position, which translate its rectangle, and a staged one sits at the identity without
keys. A wipe guide scales from the frame edge that its angle names, with one
Scale track and no other key, and a flat wipe's video maps its frame onto the
canvas unchanged. A shape guide is an Opacity mask (`CanonicalMask::Opacity`,
`writer/tracks/mask.rs`): one closed contour without path modifiers,
primitives or rounded anchors (`premiere_path` rejects a nonzero corner radius
for graphic shapes and mask guides alike), in the video's frame as an
untranslated Crop guide is (a shape guide has no translated form), written as
the v7/c5 mask record (which Premiere 26.5.1 accepts and
re-saves as `AEMask2`, the export gate) with the path back in unit-frame
fractions, inverted only at mask opacity 1, its feather reported as an
approximation (the gate measures Feather 20 at 18.4–18.8 px natively and
38.0–38.1 px in FX, 10–90 % edge widths); the guide's
paints are not drawn and not checked, and a shape that only that mask
references is consumed with the clip. The writer places Opacity after every effect, where Premiere
applies its mask, while FX applies a video's own mask before its effects, so a
flat video exports its Opacity mask only without effects; a stage group carries
the mask over the video's effects. Any other mask omits the clip, and a nest
carries no Opacity mask. A track matte (`CanonicalMask::TrackMatte`,
`canonical_track_matte`) on a video, a nest group or a stage group with no
masks exports as a Track Matte Key in the 12.x 7/5 record form
(`writer/tracks/animation.rs::track_matte_records`: Matte = the matte track's
written ID, `video_track_id` = index + 1; Composite Using and Reverse from the
channel, `alphaInverted` writing Reverse `true`) when its source is a video
that itself exports (`clip_omitted`) or a still whose coverage export keeps
(`still::unsupported_matte_still`, over the still exporter's own table of
reset properties, `unexported_still_properties`, plus
effects, corner radius and keys: a still as content accepts that loss with a
Feature report, a matte does not, or an Opacity 0 still would export opaque
and show its clip whole), beside the clip over its range, or the stage group's direct child
over the group's range, not hidden and without a matte or masks of its own,
and when the clip maps its sequence-sized frame onto the canvas unchanged
(`sequence_sized_layer_is_clip_frame`, the Linear Wipe's check: a flat video
or nest group at default static Motion without Motion keys, or a stage group,
whose Motion moves the child source with the clip; a moved or keyed flat clip
keeps its sibling source fixed in FX where Premiere's Motion would move the
keyed picture); an inverted mode (Premiere's Reverse), a moved, keyed or
non-sequence-sized flat clip, or a Color Matte, graphic or nest source omits
the clip. `export_layers` exports every track matte source after the
other layers of its list, on the lowest track above every exported placement
that keys it (`MatteConsumers`), and then writes that track's index into each
placement's key, which stays at `UNPLACED_MATTE_TRACK` until then so that the
model validation rejects a keyed clip whose source never placed; a stage
group's child source exports as a clone of the child over the group's range
(`export_staged_matte`; its embedded sound is reported, not exported). A source
that no exported placement keys is omitted, because FX draws it only through
the clips that key it. Premiere 26.5.1 opens an edited export of
`premiere_isolated_track_matte_key_26_5` in this 7/5 form without a warning,
lists every key with its Matte, Composite Using and Reverse as written, re-saves
the records as its own 9/7 form with the values unchanged, and AME renders the
edits (Reverse on and off, Luma to Alpha, the staged clip at Scale 75) as
edited with the keyed edge sharp after a blur (a scoped export gate; evidence in
`premiere_isolated_track_matte_key_26_5`, `export_gate`). The
Premiere UI was not inspected, and the case stays `structural_only`. A stage group
(one mask whose guide is one of its two children, the other a video, or a
track matte whose source is one of them) exports as
one clip when every group and video field that a clip cannot carry is neutral
and the group's keys are Position, Rotation, Scale and Opacity keys; otherwise
it exports as a nest ([Nested sequences](#nested-sequences)), or is omitted
when its video does not span it or when it carries a track matte (a nest's
source is its sibling, a stage group's its child, so the stage keeps its own
reason). Media and sound inspection (`exported_video_layers`,
`exported_clip_videos`) ask the same check (`clip_omitted`), so neither the
picture nor the sound of an omitted clip is inspected. An audio layer's asset is
always inspected. A still exports no Crop, Linear Wipe or Track Matte Key, so
an image with a mask or track matte is omitted whole
(`unsupported_image_masks`), and the image collector (`exported_image_layers`)
leaves its media uninspected. A stage group's video source is inspected with
the group's clip (`stage_matte_video`).

A Crop or Linear Wipe on a moved or keyed clip follows the classifier and
export rules above, which replace #4465's Motion rules (the ledger's "Crop and
Linear Wipe with Motion" row names each change). #4465's retiming and wipe-key
rules stay: a retimed, reversed or time-remapped clip drops its Motion and
Opacity keys in both directions (each reported), so a static Crop still
converts; import omits the occurrence of a wipe on such a clip or with
colliding keys or a non-cardinal angle, and export stops on a Linear Wipe of
such a clip or one whose keys it cannot write. That a flat wipe's canvas-sized
guide is its clip frame at the identity static Motion (scale 100, rotation 0,
anchor on the position) is geometric reasoning, not an Adobe-verified rule.

Every other effect that cannot be represented (unknown, keyed on an unbound
parameter or with unsupported keys, single-axis, a conflicting cached
`CurrentValue` on a static parameter, or unexpected native content) is omitted
and reported with its match name, display name, versions, stack position (1 is
the first effect applied), clip, track and time range. The occurrence and its
other effects are kept. An unknown bypassed effect is reported the same way,
marked `bypassed`. Standard effects on a sequence's own chain still reject
that sequence.

`writer/tracks/effects.rs` writes the effect records, in stack order, after the
records of intrinsic Motion (record order, not chain order), with `Bypass` from
the enabled flag, and `writer/tracks/video.rs` numbers the chain in reverse
stack order: Opacity and Motion at the lowest Indexes, then the other
components from the last applied to the first. Three writer choices are
inferred, not observed: the Motion writer's record generation
(`VideoFilterComponent` 7 with `Component` 5), component IDs from 4 (corpus
chains start their standard effects at 3 in 140 chains and at 4 in 76), and a
static Blurriness record modeled on the blur's static popup and checkbox
records. A keyed parameter is written like a keyed Motion parameter: its keys,
no `IsTimeVarying`, and its first key as `StartKeyframe`. Premiere 26.5.1 opens
an edited export of `premiere_isolated_gaussian_blur_keys_26_5` with these
records without a warning and reads its blur stack and keys back as written,
and AME renders it as edited (evidence in
`premiere_isolated_gaussian_blur_keys_26_5`).
Export writes an FX Directional Blur as the current Directional Blur, in the
9/7 form that Premiere 26.5.1 saves: its Angle and Amount, Edge Behavior 2,
`Bypass` `true` when disabled, and Premiere 26.5.1's defaults for the other 17
parameters. Amount 1000 is Blur Length 1600 in the clip's frame, above
Legacy's 1000, so the Legacy records are no longer written; a longer value or
key omits the effect.
Premiere 26.5.1 reopens an export and reads back its values and keys, and AME
renders it as written (export gate, same evidence folder). The Legacy records,
which the reader still reads, copy the corpus ones of Premiere 12.1 and 14.4,
with the display name `Directional Blur`, which Premiere 26.5.1 shows as
"Directional Blur (Legacy)" (evidence in
`premiere_isolated_directional_blur`).
The Brightness & Contrast records copy the corpus ones of Premiere 12.1
(`ParameterControlType` 2) with the display name `Brightness & Contrast`,
which Premiere 26.5.1 opens and shows as written (evidence in
`premiere_isolated_brightness_contrast`).
The Invert records copy the corpus ones of Premiere 14.4 (`ParameterControlType`
7 and 2) without the opaque private data that Premiere saves; Premiere 26.5.1
reopened four such records without a warning and read every value and key back
exactly (31 checks, 0 differences; evidence in
`premiere_isolated_invert`). A bypassed Invert and Bezier
Blend keys are not Adobe-verified.
The Tint records copy the corpus ones of Premiere 12.1 (`abstract_slideshow`:
colour parameters with `ParameterControlType` 5 and bounds 0 to 2^64 − 1,
Amount with control type 2); colour keys are written with zero handles, as a
Linear or Hold segment has no velocity. Premiere 26.5.1 opens them and reads
them back as written (the export gate of `premiere_isolated_tint`). No Black &
White record is written: a `tintTritone` at Tint's defaults is a Tint.
The Ramp records copy the corpus ones of Premiere 12.1 (`corporate_slideshow`:
points with `ParameterControlType` 6, colours with control type 5 and bounds
0 to 2^64 − 1, the Shape popup with control type 7, Scatter and Blend with
control type 2 and Scatter's UI bound 50); point keys are written with
straight spatial fields and colour keys with zero handles. Premiere 26.5.1
opens them and reads them back as written (the export gate of
`premiere_isolated_ramp`). The Mosaic records are
written in the same corpus generation (counts with `ParameterControlType`
1, bounds 1 to 4000 and UI bound 200; the checkbox with control type 4 and
**no `Name` element**, as Premiere 26.5.1 saves it) with whole counts and
Hold keys; Premiere 26.5.1 opens them and reads them back as written (the export
gate of `premiere_isolated_mosaic`). The Transform
records are written in that generation too (points with control type 6,
checkboxes with control type 4 and no `Name`, scalars with control type 2
and the angles with 3, Sampling with control type 7 and
`DiscontinuousInterpolate`) with Premiere 26.5.1's bounds and the scale
axes' ±200 slider range (`LowerUIBound`/`UpperUIBound`), keyed parameters in
Motion's form (the first key as `StartKeyframe`, no `IsTimeVarying`) and
point keys with straight spatial fields; Premiere 26.5.1 opens them and reads
them back as written (the export gate of `premiere_isolated_transform_26_5`).
Effects that apply before a Crop or wipe sit at higher Indexes than it; its
component `ID` stays 3, and the `ID`-swap probe shows, at the frames it
compared, that `ID` does not order the render.
Export omits FX effects without a mapping and effects with animated
parameters that have no binding rather than flattening them. The
conversion-accuracy ledger records each case.

## Script animation export

FX → Premiere export bakes layer-time `JsScript` animation into editable native
keys for the existing scalar and paired native controls listed below. `convert::script_bake` runs once per export, in
`save_tesseract_as_premiere` before media inspection, so that inspection and
the writer read the same baked document. It uses the public `fx_keyframe_bake`
runtime, scalar fitter and stable key identities, not an Adobe expression
runtime, and adds no native effect, owner or capacity. Only the baked animator
records change, in an export-owned document; the input archive, authored keys
(disabled tracks included) and unknown sibling fields stay intact. A document
without scripts exports from the borrowed original, without a copy or a
diagnostic.

| Owner, as export places it | Scripted FX targets | Native keys |
| --- | --- | --- |
| Video or media-video clip, at the root or in nests | Opacity, Rotation, Position X/Y, Scale X/Y | Opacity; Motion Rotation, Position and uniform Scale |
| Root video clip's own sound; root audio layer | Volume | Volume, as Linear pieces |
| Stage group (one clip) or nest group placement | Opacity, Rotation, Position X/Y, Scale X/Y | the clip's or nest's Opacity and Motion |
| Adjustment layer, at the root or in nests | Opacity | the adjustment clip's Opacity |
| Root graphic group | Opacity; Rotation, Position X/Y, Scale X/Y | clip Opacity; Vector Motion |
| Root text graphic, or the one text of a root graphic group | Opacity, Rotation, Position X/Y, Scale X/Y | text parameters |
| Linear Wipe guide rectangle | Scale X or Y | Linear Wipe Transition Completion |
| Crop guide rectangle | Rotation, Position X/Y, Scale X/Y equal to its video's | its video's Motion |
| Effect on a clip, nest or adjustment layer | Gaussian Blur `blurriness`; Directional Blur `direction`, `blurLength`; Brightness & Contrast; Levels `inputBlack`, `inputWhite`, `outputBlack`, `outputWhite`, `gamma`; Corner Pin corners; Tint `amount` | the effect's bound parameters, in their native units (the current blurs' Amount) |
| Levels in Invert's form | `outputWhite` with `outputBlack` as its complement, the only animated parameters | Invert Blend With Original |

Paired targets follow the native keys. Position X/Y share key times and easing
as one point; a Position with one scripted axis keeps the other axis's static
value at every key. Scale Y must stay within its tolerance of the Scale X
curve, and both axes take the Scale X keys. A Corner Pin corner is one point.
An Invert's output black takes the exact complement of the output white at the
same keys when its script follows that complement; otherwise both outputs key
Levels. A scripted axis whose partner has authored keys or an unbaked script is
not baked; when one axis's script fails, it reports its error and its partner
the pairing. Graphic parameters without verified Bezier speeds (Position, and
text Rotation) take Linear and Hold keys only, and scalar keys never have a
cubic arrival into a key that starts a Hold.

Each script runs on its owner's layer clock, as the FX runtime does: from 0
through the owner's active duration, with key times in owner-local
milliseconds (from a clip's source in-point, or from a nest's start). An owner
or enclosing group with a playback remap or an enabled Posterize Time is not
baked, because the writer converts neither clock. The input has
`input.time.seconds`, `input.time.milliseconds`, the runtime's stable
`input.randomSeed` of the target (or of its `randomSeedTarget`, which must be a
bound target; the seed ids are pinned by tests), no dependencies and empty
reference tables. Script dependencies and layer references are not baked: no
public crate evaluates the animation graph, and export has no live resource
context. Legacy or mixed `code` clocks and unknown script fields are not baked
either.

Evaluation samples every integer millisecond of the owner's window once, in
ascending order, in a fresh Boa realm. A second fresh realm then evaluates up
to 64 evenly spaced times of the window in descending order and must reproduce
each sample exactly. This catches common history-dependent scripts (a global
counter, a remembered previous time, `Math.random`), but it does not prove any
script free of history or nondeterminism: history can matter at times the
probe skips, and the runtime's playback realm shares state across tracks. The
shared fitter fits the samples; segments that native keys cannot hold are
fitted again within their own window, and the fitted curve is checked by
arithmetic against every sampled millisecond. Tolerance caps are 0.1
(percentage point, degree or native effect unit, and a tenth of a level for an
Invert's outputs), 0.5 px for Position, half a pixel of the longer side of the
clip's frame for Corner Pin fractions (a video's `sourceRect`, or the canvas of
a nest or adjustment layer), and half a native step for Levels levels and Gamma
hundredths, which export rounds; Volume uses only the fitter's tolerance
relative to its range. The fitter may choose a tighter tolerance. None of this
bounds sub-millisecond behavior or establishes Adobe render fidelity.

A script keeps its animator, with a diagnostic grouped by reason, when it
fails to parse, throws or reaches Boa's loop, recursion or VM stack limits,
returns a non-finite
or non-numeric value, fails the order probe, leaves 0–100 (Opacity, Linear Wipe
completion, Tint Amount) or goes negative (Volume), breaks a uniform Scale,
exceeds the parser-stack-backed 32 KiB source bound, or
exhausts the fitter's work bound. Values are never clamped and keys never
truncated, and other content keeps exporting. Scripts on owners that export no
keys are reported without being evaluated: shapes, objects of graphics with
several objects, graphics, text, rectangles and audio inside nests, stills,
Color Mattes, groups without a video or adjustment layer, other layer types,
3D, skew, trim and stroke targets, mask properties, effects without a mapping
or a bound parameter, and text Source Text fields such as font size, whose
native keys are text document snapshots rather than scalar or paired controls.
Tint's three-channel native colour keys are also outside this scalar/paired baker;
the existing conversion of authored colour keys is unchanged.
The owner index never fails the export: a layer that the writer rejects, such
as a video without its source range, fails the export only when the writer
reaches it, as without scripts.

Each export fixes its budgets before any script runs: 2^25 JavaScript calls,
counted up front from every baked track's samples and probes, and 2^20
generated keys. Counts saturate so that overflowing windows, pairs or batch totals
fail admission before any worker starts or sample buffer is allocated. A track costs one call per millisecond of its owner's window
plus up to 64 probes, so the call budget depends on duration: N scripts on
W-millisecond windows fit when N × (W + 65) ≤ 2^25, such as 15,000 scripts on
windows of up to 2.1 s, or 1,000 on windows of up to 33 s. A cooperative
20-minute bound is checked between calls. Any of them fails the export before
publication. Boa has no interrupt, so a single call is bounded only by its
loop, recursion and VM stack limits, not by time or heap: this is not safe
execution of arbitrary untrusted code.

Up to eight threads evaluate independent tracks; results, key ids and
diagnostics do not depend on the schedule. Boa parses nested expressions
recursively, and its VM limits do not bound that native recursion, so every
evaluation thread has a stack of 32 KiB per byte of the batch's longest script
source, at least 16 MiB and at most 1 GiB for the 32 KiB source bound. The
worst construct probed, an unclosed parenthesis per byte, takes 19.4 KiB per
source byte in release builds and in this workspace's dev builds, whose profile
optimizes the parser in `fx_keyframe_bake`. This is an empirical margin for
those build profiles, not a proof: code that a script builds at run time
(`eval`, `Function`, `RegExp` on a generated string) and native recursion over
deep run-time values (`String`, `JSON.stringify`) are not bounded by the source
size and can still overflow the stack and abort the process. If no evaluation
thread starts, the export fails before publication.

After the writer runs, export reports how many baked tracks it wrote and names,
under its owner, each baked track that it did not write (for example, one on a
layer that export omits for another reason). The writer records written keys
only where it places their native owner.

Scale is bounded by the budgets above, not measured by a checked-in test. One
recorded run of an earlier revision baked and wrote 15,200 scripts (400
one-second clips in seven nests, each with Motion, Opacity and four Corner Pins
scripted: at most 16,188,000 calls and 344,563 keys) in about 10 to 11 s on
eight workers with a 2.5 to 3.0 GB peak footprint, most of it the baked
document's one JSON serialization and reparse; a mix of every mapped effect at
that scale exceeded this crate's 500,000-node reader limit, which is not a
Premiere limit. Those are historical structural measurements, not Adobe proof.
The crate's reader does not convert keys or effects on nest placements, so
tests check those on the written model rather than by readback.

A document without scripts exports from the borrowed original document: the
writer receives the same input as before baking existed.

Paired Position, uniform Scale and Rotation on one trimmed, offset video have
independent Adobe control/reopen/readback evidence and seven equal native frame
pairs. A fresh original-FX comparison passes unchanged full-resolution RGB24
thresholds: mean 0.997580 and minimum 0.996307 across 15 samples, against required
0.99/0.98. Final converter output equivalence and unchanged production renderer
code establish applicability. The [native Motion proof](tests/fixtures/script_motion_proof/README.md)
records the identities, method and limits. This proves only that Motion fixture,
not effects, graphics, audio, nested owners or arbitrary scripts.

The earlier two-property candidate's original-FX comparison failed the unchanged
score policy (mean 0.943460, minimum 0.619020 at 0.25 s); its record is the
"History" section of the native Motion proof. No
renderer, sample exclusion or threshold change was used to turn that historical
failure into the separate Motion pass.

## Unsupported edits

`schema::occurrence_edits` (`PrVideoOccurrence::edits` for an occurrence) lists
the non-neutral edits of a media or nest placement in one order: Linear Wipe
(transition); Motion keys, Opacity keys, static Motion Position, Anchor Point,
Scale and Rotation, Crop, Opacity mask, Track Matte Key and Opacity (picture);
playback rate and time remap (clock). A neutral value is the one the reader gives an omitted component.
Each check keeps its policy and messages. A still image (`reader/still.rs`
`keep_occurrence`) loses its occurrence to a Crop, Linear Wipe, Opacity mask
or Track Matte Key and a Color Matte to its first picture or transition edit
other than a static Opacity, which its rectangle carries;
both keep it through clock edits, which are reported. Import gives an image
layer no key, so a keyed still would draw unkeyed: it is omitted, and
`consume_claimed_mattes` drops its matte clips, which Premiere does not draw
(fixture G1b). A still's Motion, Opacity and keys convert as a
video clip's. An adjustment layer (same check, `schema::adjustment::retains_edit`)
keeps Opacity and Opacity keys, which its FX layer carries. Static Position
and positive uniform Scale at default Anchor/Rotation/Opacity/Normal blend,
without keys or retiming, also keep the occurrence when its active native
effects are empty or static full RGB Invert (A3). A nonpainting canvas-sized
rectangle with that Motion masks the identity adjustment, changing effect
coverage without moving the lower composite. Other edits, including Crop,
Linear Wipe, native masks and clock edits, still omit the occurrence. Masked
adjustments remain unsupported on export. That omission is not lossless:
Premiere renders the cropped region of a Crop-only adjustment black (26.5.1
fixture), so the picture differs in that span. Motion keys and Opacity keys
are separate edits for that reason. A nested placement (`reader/nested.rs`
`read_nest`) is omitted for its first edit other than a Track Matte Key, a
forward playback rate, which its saved window must confirm, and, without a
Track Matte Key, its Motion and Motion keys.
A media occurrence's Crop or wipe has a second classifier over the same fields
and its effect counts, `PrVideoOccurrence::mask_boundary` (see
[Effect stacks](#effect-stacks)); the reader omits an occurrence it rejects
beside `keep_occurrence`. Two checks stay outside both:

- The coverage-effect and mask checks (`reader/effects.rs`) inspect native
  effect components before the occurrence exists.
- `read_occurrence` rejects Motion or Opacity keys on a Color Matte, and Scale
  to Frame Size on any media, before validation and effect reading, with its
  own message.

## Timing

Both directions accept constant-rate media at 23.976 (24000/1001), 24, 25,
29.97 (30000/1001), 30, 50, 59.94 (60000/1001), and 60 fps. `FrameRate` holds
these rates as exact fractions. Each sequence and media record has its own rate.
Video clips may play at constant speed or in reverse, and imported Time
Remapping becomes editable playback keys; see [Retiming](#retiming). Conversion
keeps timeline coverage and shared cuts; it does not keep identical source frames.
It does not guarantee that arbitrary Premiere features or complete project
structure survive conversion through both formats. Tests of both directions
cover only the supported fields that they name.

| Direction | Sequence | Timing rule |
| --- | --- | --- |
| Premiere to Tesseract | All eight rates | Round absolute timeline start and end to milliseconds, nearest with ties forward. Each boundary moves at most 0.5 ms. Set duration to end minus start. Round source-in to milliseconds and, at unit speed, use the active duration as the source duration. |
| Tesseract to Premiere | 30 fps, or the [export rate](#export-rate) | Snap absolute timeline start and end to the nearest sequence frame, ties forward. Each boundary moves at most half a frame (16.7 ms at 30 fps). Keep source-in exact in milliseconds, without snapping it. At unit speed, derive source-out from source-in plus the snapped duration. |

Adjacent clips stay adjacent. Source trims need not align to either frame grid.
Media records keep the native source's frame duration and duration. Inspected
file clocks stay separate; the admission rules below compare them without
changing playback speed or source bytes. All tick and millisecond conversion
uses checked integer arithmetic.

### Sequence end

`PrSequence::timeline_end_ticks` is the last `VideoClipTrackItem` end of the
selected video group, read leniently so that an omitted item still ends the
timeline; an unreadable or invalid range is skipped and an off-grid `End` snaps
to the nearest sequence frame, ties forward. The work area and
`OriginalDuration` are ignored (inferred;
[evidence](../../tests/README.md#sequence-end-evidence)). The
document duration is the later of that end and the exact last converted audio
end. Export writes the document end as `MZ.WorkOutPoint` and `OriginalDuration`;
because Premiere renders and this reader reimports only to the last occurrence,
a different end is reported as a `document.duration` omission.

### Media end

For constant sample clocks, listed native source rates still require exact
equality with the file's rate and duration. Physical video with an unlisted
positive native frame duration can import when the file has nonzero constant
sample durations and an exact, contiguous presentation grid, the native duration
equals the inspected sample count times the native frame duration, and both
complete durations round to the same millisecond with ties forward. Each rounded duration is computed directly
from its own integer clock. Existing codec, sample bounds and full-presentation
edit-list checks still apply. Stills, generators, linked compositions and sequence
rates keep their existing rate restrictions; export rejects unlisted file rates.

Selected picture import has a bounded exception when whole-source edit or
unlisted average-endpoint checks fail. Every consumed source interval must be
forward unit playback without Time Remapping through every containing nest,
inside the first positive rate 1 edit, with that edit's media origin equal to the
first physical PTS, and before the final physical presentation sample. Decode
tables must cover the exact sample count with positive contiguous durations;
CTTS must have a supported version and exact coverage, and sorted physical PTS
must be unique. Legacy v0 signed offsets follow the unchanged shared parser/player:
existing exact-CFR/full-edit admission is retained; new irregular admission requires
nonnegative first PTS equal to edit origin, normalized PTS inside physical duration,
selected unit interior intervals and an explicit legacy-clock diagnostic. Native
duration still equals physical sample count times raw native frame ticks. Source bytes, source ranges and sample timestamps
are preserved; no CFR grid or source endpoint is invented. One approximation
reports this bounded admission. Crossing later edits, retiming and uncertain
final-frame use reject. Existing accepted regular clocks and fractional movie
tails keep their full-source behavior; export and no-selected-use inspection
retain the full-source contract.

Selected native inspection and import share this context only after raw native
inventory proves every physical picture/nest placement survives in each parsed
sequence instance, every native sound placement is retained, and native references
are resolved. Omitted duplicate uses, failed model loads or unmatched flattened
nested sound fall back to strict whole-source inspection. Synthetic graphic or
adjustment omissions do not authorize or prevent physical placement coverage.

A selected picture that never consumes embedded sound may retain multiple audio
and nondrawing metadata streams unchanged; actual sound use still requires the
existing unambiguous audio admission. Physical video may retain an unmirrored
quarter-turn native orientation only when it agrees with the exact container
matrix. The orientation comes from the matrix alone, so import admits it for any
use, whole source or selected intervals. Encoded dimensions are validated; Motion,
masks and sourceRect use displayed dimensions, while the decoder supplies
rotation. Skew, mirroring and other translation reject. Export of a rotated
source is unsupported and reports that it cannot be exported.
Supported 8-bit 4:2:0 basic SDR may retain full-range H264/HEVC flags; explicit VUI
and nclx range declarations must agree, with an explicit full-range bitstream
signal. HDR, 10-bit full range and reserved flags remain unsupported. Independent unchanged-player decode/orientation pixel probes
support these admission limits; they are not a native fidelity certification.

A nonconstant sample clock can import as Quantized only when every composition offset is
zero and every sample start and the complete endpoint exactly equal
`round_half_up(frame_index * timescale * fps_denominator / fps_numerator)` for
exactly one of the eight supported nominal rates. Positive contiguous decode
samples and declared duration remain mandatory. This admits a microsecond 30 fps
clock whose durations alternate 33333/33334. Shifted/doubled boundaries do not
become Quantized; they can only enter the separately warned Irregular class after
its physical/native checks. Short coarse grids matching multiple rates reject. A known native rate must equal the proven nominal rate;
all such clocks also require the exact native count product and directly matching
rounded endpoints. The edit check uses the actual last sample duration.
Quantized clocks are import-only: even nominal 30 fps remains rejected on export.

This is a bounded relaxation reported once per admitted media record. It does
not establish identical intermediate source-frame selection: a 2997/125 fps file
and its native declaration can have a matching complete duration while frame
12 rounds to 501 ms in the file and 500 ms in the native clock. Independent Adobe
render comparison for these relaxations is incomplete. Irregular presentation
has a separate bounded import approximation described below. Duplicate/out-of-range
presentation timestamps, inconsistent declarations, different rounded endpoints
and overflowing declarations still reject.

Source-in must be before the exact media end. The final sequence sample
(`source_out - sequence_frame_duration`) can be at most 1.5 ms past that end. The
1.5 ms is three nearest-millisecond errors: source-in, timeline start, and timeline
end. Native reading and writing use the same rule, so a file that export writes
loads again. Thus source-out can be at most one sequence frame plus 1.5 ms past the
media end (about 34.8 ms at 30 fps), and the last source image holds for the final
frame. The allowance applies to every input, rounded or not. Conversion does not
clamp a clip to the media end, change its speed, or change the asset's intrinsic
duration.

For example, at 30 fps, a 34 ms clip with source-in 967 ms in a 1000 ms source
exports as one frame sampled at 967 ms. A 67 ms clip with source-in 969 ms needs a
second sample more than 1.5 ms past the end, so it rejects.

### Saved OutPoint

A forward unit-speed clip plays from In for its timeline duration, and
Premiere can save an `OutPoint` up to one sequence frame from In plus that
duration. The reader takes In plus the duration as the clip's source end,
which the media-end rule above then bounds. A larger difference, an empty or reversed saved range, another
speed, reverse and Time Remapping keep the saved `OutPoint`, which the source
span check rejects unless it matches the clock. Synthetic structural tests
cover these cases; no Adobe-native fixture in this repository proves the rule.

### Retiming

Import maps a native constant `PlaybackSpeed` other than 1, or `PlayBackwards`,
to two linear FX playback keys over the rounded active range, from the rounded
source start to its end (reversed for reverse), and a Time Remapping ramp to one
FX playback key per native key on the full media clock. A ramp that FX cannot hold omits
its occurrence. Export writes a bounded two-key linear playback as native speed
and reverse. The keys must span the active input window after `inputOffsetMs`;
their values select a nonempty forward or reversed interval within `sourceRange`.
The rate is that selected millisecond source span over the frame-snapped active span, so it can
differ from the FX rate by that snap. Any other playback curve, and a
`source.timeRemap` whose source range is longer or shorter than the active
range, omits its occurrence ("time remapping was not exported: Premiere export
writes constant speed only"), because unit speed would show other frames. FX
does not render `source.timeRemap`, so one over an equal span exports at unit
speed with a report.

Native keys lie on the source clock, and no Adobe fixture pins how Premiere
places them on a retimed clip. When the native rate is not 1, import omits the
clip's intrinsic Motion and Opacity keys per feature and keeps its playback and
static values, so a Crop follows the static Motion. The reader already omits a
time-remapped clip with Motion or Opacity keys. Export omits Motion and Opacity
keys per feature when the rate it writes is not 1, and keeps them at unit speed.
A canonical Crop is still written and follows the static Motion that
remains. A Linear Wipe on such a clip omits its occurrence on import, as on a
time-remapped clip, and stops the export. Effect parameter keys are outside this
rule, and their timing on a retimed clip is not verified. Sound stays at unit
speed (see [Audio](#audio)).

### Rejections

Tesseract-to-Premiere conversion rejects these inputs with sequence, track,
layer, or media context. Premiere-to-Tesseract conversion omits and reports each
affected occurrence instead. An unlisted sequence frame rate omits its whole timeline.

- an unlisted, quantized or irregular source clock on export, or an irregular import failing its bounded unit-playback/native/physical checks;
- on export, a layer whose source and active durations differ without
  `playback` or `source.timeRemap`;
- a native or document rate or duration that fails the applicable source
  admission rules above;
- a clip that snapping collapses to zero frames;
- a source-in at or after the media end, or a final sample past the allowance;
- an MP4 edit list that fails the full-source contract and the selected-picture
  first-edit/interior exception above.

The movie duration of that segment may be the ceiling of the exact sample
duration in the movie time base, less than one movie tick and less than one source
frame longer, so a coarse clock cannot hide a missing frame. It may also end less
than one source frame early, as iPhone captures do (`IMG_2439.mov`: 1208/600 s of
49 frames of 25/600 s, which Premiere 26.5.1 saves as 49 frames). For example, a
250.25 ms source accepts a 209 to 251 ms movie duration. Sample timing stays
authoritative: every sample is kept.

Very short clips can collapse on export, and an input with a partial tail at
another sequence rate can exceed the allowance. Not every native interval has an
equivalent at the export rate.

### Export rate

Export writes a 30 fps sequence unless the caller selects 23.976, 24, 25, 29.97
or 59.94 fps. Nested sequences and the black canvas use the same rate. The
sequence `MZ.Sequence.VideoTimeDisplayFormat` is the code that Adobe-saved corpus
sequences of that rate store (`format/writer/sequence.rs`). No corpus sequence
runs at 50 or 60 fps, so those rates reject. Stills, Color Mattes and graphics
start one hour into their synthetic clock, rounded down to a frame; the 23.976
and 59.94 fps values are inferred. Generator clips keep `DefaultIsDropFrame`
`false` and all media log `TimecodeFormat` 104, although Adobe-saved generators
use the drop-frame state and code of their rate; these fields set only the
timecode display. AME 26.5 renders a probe exported at each of the five other
rates with the written rate, frame count and cuts. Premiere's timecode display
was not inspected.

### Evidence limits

Timing bounds do not guarantee identical source frames, even at matching rates.
For example, at 30 fps, a clip placed at frame 2 (66.667 ms) starts at 67 ms, so
each of its source requests is 0.333 ms earlier than exact. Near a source frame
boundary, a decoder can then show the previous frame. This crate does not test
decoder frame selection. Native decoder tail handling has a separate
pixel-producing regression. A manual check in Premiere Pro 26.5.1 opened, saved,
reopened, and rendered converted projects at each supported rate, one mixed-rate
timeline, and one rounding-only tail. Chrome WebPlayer played the same converted
documents. These checks are not pinned references or score gates, and they did
not compare exact source frames.

## Audio

Each native sound placement becomes one editable `Audio` layer. Imported picture
layers are silent, so the sound of a linked A/V source plays once. Export writes
one sound placement for each audio layer, and for each video layer that has a
positive volume or volume keys and a source with sound.

The reader folds static source, clip, track, and master levels and mutes,
including the timeline mute of a track, the clip's Clip Gain (`AudioClip/Gain`)
and its intrinsic clip Volume into the layer volume. The Volume `Level` scale
depends on the project layout (`PrVolumeLayout`). Level keys stay on the source
clock as `PrVolumeKeys`, apart from the other stages that multiply them, and
import as an `AudioVolume` key track whose Linear segments are fitted to
Premiere's fader curve (`convert/audio.rs`). A key track that cannot import,
such as one with two keys that round to the same millisecond, is reported
once and leaves its sound's layer, placement and source at zero gain
(`convert::premiere_to_tesseract::audio_layers`, shared by root and nested
sounds): the value before the keys would play through their silent
intervals. It reports other automation, pan,
solo, insert effects, transitions, and Volume or Channel Volume values that
cannot convert as omissions; a track that lists any audio transition reports
one omission, and its placements play at their own levels without the fade. It
omits a placement that remaps channels or uses a layout other than mono or
stereo; older projects wrap `AudioChannelLayout` arrays, which read as the
array. The source level is read from the placement's master clip, whose
Source Monitor state (the `monitor.*` keys and an empty `AMM.CurrentSolo`
list) is skipped; a master clip that does not decode omits the placement. A
broken or ambiguous link to one audio track or sound placement omits
only that track or placement; the other audio still converts. Links outside
those lists, such as a track's panner or transition items or the group's master
track, still omit the whole track or audio group. The writer gives each
sound placement its own stereo track with a unity fader, so overlapping sounds
need no mixing, and writes its level and keys in the Premiere 26.5.1 clip
Volume only, with Clip Gain for a boost. A recognized imported fit writes back
as its native Linear segment: export recognizes a fitted segment of Levels at
or below 0 dB when it writes those Levels unchanged, or rescaled by Clip Gain
or the other stages while they stay above −60 dB, with a mono exception below.
Any other curve, including a fit of Levels above 0 dB or a rescaled fade from
silence, becomes Linear pieces on the millisecond grid (`convert/audio.rs`), and `PrAudioOccurrence::validate` checks ordered, finite
Volume keys without the former 4096-key count quota. The
[ledger](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations)
lists the scales, the curve errors and each omission. For a direct mono media
source on the measured static centered stereo route, import folds the
1/√2 amplitude factor (−3.0103 dB) into the effective static volume and the
gain multiplying Level keys. Admission requires matching, unremapped source
and clip channels, a stereo `StereoToStereoPanProcessor` with one static
Balance of 0.5, and the measured default stereo master/inlet routing. Unknown,
automated or noncenter routes keep their omission behavior and skip this
normalization. A nested sequence's stereo mix is not normalized again.
Export writes the same centered route and divides effective mono static/key
gains by 1/√2 before splitting them into Level and Clip Gain; an effective
mono gain of 1 therefore writes +3.0103 dB rather than a default native chain.
Fit recognition predicts the written mono Levels without this factor, up to
3.01 dB below them. That scale does not change a fit whose gains stay above
−60 dB, but an imported mono fade from or to silence can become bounded Linear
pieces (for example four keys instead of two) even when export writes its
Levels unchanged. The native levels fixture establishes the factor, and static
gain is now measured in both directions: a Tesseract render of that fixture's
import puts its mono placements −0.013 and −0.012 dB from AME, and AME renders
a generated FX mono unity export within 0.001 dB of unity. General audio
fidelity still fails on sample count and timing; the ledger has the
measurements and limits. This does not add pan, channel mapping or AAC timing
support. Adobe proof does not cover mono key interpolation, noncenter or other
routes, or transport playback.

Adobe Media Encoder 2026 (build 85) loaded and rendered three generated
projects before export moved levels into the clip Volume: the round trips of
the two pinned audio cases and a synthetic six-second document with five sound
placements (one more track than the native scaffold), a mono source, and
levels 2.0, 0.25, and 0. The WAV
renders match the source WAVs mixed per the document at sample precision;
the muted placement is silent, and the mono placement measures 0.70703
(−3.011 dB) on both channels. The linked A/V render has an AAC stream that is
byte-identical to the render of the Adobe-authored project, and all 150 frames
are identical. Premiere Pro's UI was not inspected.

Sound uses the [timing](#timing) rules of video, with two differences. Export
keeps audio boundaries exact in milliseconds; they do not snap to the sequence
frame grid, so an exported picture can move up to half a frame relative to its own
sound. Source-out can pass the audio end only by the 1.5 ms rounding allowance;
sound has no final-frame hold. Sound plays at unit speed and forward: import
omits a placement with another `PlaybackSpeed`, `PlayBackwards`, or
`TimeRemapping`, and
export reports and drops the sound of a video layer whose picture is retimed
and omits, as an occurrence, an audio layer whose `playback` makes its source
duration differ from its active duration
(`retimed audio layer was not exported`); without `playback` that mismatch
still rejects.
Media inspection supplies the channel layout,
sample rate, and exact duration. MP4-family sources use the MP4 parser and the
container edit list, so AAC priming is excluded. As Premiere does, inspection
rounds the edit segment to whole samples and accepts a segment that runs past
the last sample; muxers commonly give the sound the picture's duration. WAV and
MP3 are demuxed, not decoded; when the demuxer rejects a file with either
extension whose bytes are an MP4 container (`ftyp` after the first box size, not
a RIFF header), the reason names the MP4 data, since content does not choose the
demuxer. Import checks the native `AudioStream` only for media with a sound
placement in the selected sequence or its nests. Pictures import muted, so an
unused embedded stream cannot block otherwise valid picture. For physical video
with a sound placement, incompatible native audio layout, rate, or duration
fails selected-sequence media admission before conversion. Audio-only sources
retain their safe-omission behavior. Export rejects a
`sourceIntrinsicDuration` that differs from the file duration in milliseconds.
An unsupported native audio layout or sample rate no longer removes a valid
native video stream: import reports the sound feature, omits its audio placements,
and keeps the muted video placements subject to ordinary picture validation.
Malformed native numbers, channel-layout JSON and references retain their error
behavior. This reader rule does not change audio-file inspection or add support
for a new sound format.

Export reports source sound that it cannot write instead of failing: more than
two channels, non-AAC MP4/MOV audio such as PCM `sowt` or `lpcm`, an unsupported
or retimed edit list, and malformed audio. An audio layer is omitted as an
occurrence. The embedded sound of an audible video is omitted as a feature
(`embedded audio was not exported: …`), and the picture still exports. A
failed read, or an archive entry that fails its source hash, still stops the
export, even when its sound is unsupported. On import, a failed media read also
stops the batch instead of omitting the media's placements; only a WAV or MP3
stream that ends early counts as malformed audio.

## Fonts

The converter never consults the FX renderer's bundled font catalog. Import stores a
Premiere text font as `fontFamily` = its PostScript name and `fontStyle` = `""`.
The editable schema fills an absent style with `Regular`, so an empty style is
written only on purpose: it marks a stored PostScript name. The converted
document packages no font files, so import reports each font once as not
packaged, naming the first layer that uses it and counting the others. A font
name that contains `/` cannot form the family/style key; its graphic is omitted.

Export takes each text layer's PostScript name from the first rule that applies:

1. A packaged font whose registry face (`metadata.json` `fonts`) matches the
   family and style by its family/style names, its typographic names, or a
   selection name built with `fx_schema::custom_font_selection_name`. A matched
   named instance without its own PostScript name omits the layer.
2. An empty style: the family is the PostScript name.
3. Otherwise the layer is omitted: the document does not package the font, and
   `tsrct` would refuse to render it.

## Nested sequences

A video track item whose source is another sequence is a nested placement
(`PrNestOccurrence`, held by its track). Its placed VideoClip plays the inner
sequence's VideoSequenceSource from InPoint to OutPoint, and its SubClip names
the inner sequence's master clip, whose AudioClip and VideoClip play that
sequence's own sources. Adobe-saved nests have this shape (for example the
`food_lower_third` and `phone_title` corpus projects), but neither converts
today: the `food_lower_third` nest has a nondefault Position, and the
`phone_title` nest has effects. Apart from the Premiere 26.5.1 saves of hidden
nests (`premiere_isolated_hidden_nest_26_5`) and of a nest with its sound
(`premiere_isolated_images_nests_26_5`), every positive nest test uses
hand-edited XML. Each placement keeps its own copy of the inner timeline, and
every copy refers to the same project media records.

A nest's sound is its audio item: an AudioClipTrackItem whose AudioClip plays
the inner sequence's AudioSequenceSource in stereo, linked to the video item by
a `Link` record (`premiere_isolated_images_nests_26_5`, items 115/116, Link
107). The reader pairs the two by sequence, ranges and Enable
(`nested::pair_sounds`) and folds the item's static gain into the copy's
sounds; a nest without one is silent. The item plays its sequence from In,
at normal speed, until End: Premiere 26.5.1 saved an item shortened at its
tail with its untrimmed Out (`premiere_isolated_nest_audio_outer_keys_26_5`:
End 7 s, In 1.5 s, Out 8.5 s; shortened through its scripting API, a trim
in the UI is unverified), and AME stops its sound at End. An item longer
than its Out - In, at another speed, reversed or remapped is omitted with
its reason. An item that no converted nest carries
(its picture does not convert, it has no video item, or its range or Enable
differs) plays alone (`nested::play_alone`): each inner sound that its In/Out
shows, through every nest level, becomes a sound of the sequence that holds
the item, at its Start/End and gain, at zero gain when its Enable is off. A
hidden picture keeps its hidden group without that sound; that the item's own
Enable, not its picture's, decides whether it is heard is inferred. Each of
several items for one placement plays alone the same way, and the nest keeps
only its picture (inferred: each item is a clip of its own track). So does an
item whose Volume Level has Linear or Hold keys, whatever its picture
(`nested::apply_level_keys`): each sound that it plays takes those keys,
moved from the item's source clock, the clock of its In/Out (as Premiere
26.5.1 saves them and AME plays them in that case), to the sound's own,
keys outside the sound included, with the sound's gain and the item's
other stages as their `PrVolumeKeys` gain. A keyed sound keeps its own
keys when the item's Level holds over its source range, and takes the
item's when its own Level holds, the held Level folded into the gain.
Where both only step (each segment a Hold or a Linear segment between
equal Levels), it takes their product as Hold keys at every time either
has a key (`nested::step_product`); that composition is exact, while a
Linear segment of the keys a sound takes still imports as the fitted fader
curve (Audio, above). Where both change
over it and one of them changes along a Linear segment, even outside the
sound, one key track cannot hold their product, and that sound alone is
omitted with the reason. So is a sound whose clock moves the keys past
Premiere's tick range, whose gain or Hold product overflows, or that does
not validate: each sound is heard or omitted on its own, reported on the
item. An item whose own gain overflows, or whose inner sequence cannot be
read, is omitted with its reason. An unreadable Volume, a keyed Mute and
one channel omit the nest's sound with their reason. The writer
gives each nest whose sequence has sound one such item, at default Volume and
with the video item's Enable, and the link. A group with sound but no picture
is not exported: Premiere 26.5.1 draws a nested sequence without video as
opaque black (IN2 export gate, Part A); a nest with its picture and sound
reopens linked and plays both (Part B). Inner stills, Color Mattes and
graphics convert inside the nest with their own rules; a nested graphic is
structurally tested only (no Adobe-saved nest holds one).

The ordinary sequence reader reads the inner sequence. A top-level read reuses
each finished inner read for every placement at the same remaining nesting
depth, including a read whose placement back into an open sequence closed a
cycle. A placement of a timeline on a nesting cycle (`graph::cyclic_sequences`)
is omitted without being read.

A placement is checked against that one read without copying it. Placements
kept by their track's overlap rule are copied without the former 1024-layer or
256 MiB accounting quotas. This preserves the existing materialization design;
it is not a memory optimization or a guarantee against allocation failure.

A nest is read by the same placement reader as media
(`format::reader::video::read_placement`): tone mapping, geometry,
`OriginalSubClipTimeOffset`, clip Enable, component chain, range,
SubClip/VideoClip, playback speed, time remapping, markers, in/out, linked
transitions and the master clip follow one set of rules, so master marks
annotate the source as they do for media.

A placement converts only when it is plain: normal speed with start, end, in
and out on the sequence frame grid at the outer frame rate, or one constant
forward speed (below); an out point within the inner sequence; the outer
canvas size; intrinsic Motion, static or keyed, only
without a Track Matte Key; default Opacity without keys; no effects other than
a Track Matte Key, which keys the canvas-sized picture and becomes the group's
track matte; and a master clip that plays the same sequence source. Clip Enable and track output hide it as
they hide media. Otherwise the placement is omitted and
reported. So is a placement more than `MAX_NEST_DEPTH` (8) levels below its
top-level timeline. This recursive-depth safeguard remains independently of
cycle checks; the former direct/expanded occurrence and byte quotas do not.

Import creates one `GroupLayer` per placement over the placement's range,
hidden when the placement is. An unmoved placement's group has an identity
transform. A moved one takes the placement's Motion as a clip's layer does,
for the canvas-sized picture (Anchor Point and Position times the canvas), and
its Motion keys on the placement's source clock. Before a later first key
Premiere shows that key's value, so the reader gives a keyed property its
first key's static value. Premiere draws only the nested
sequence's frame, so a moved group is clipped by an Add mask whose guide is a
child Rect of the frame at the identity, the form from which a nest's Crop
exports; the whole frame exports as no Crop. Its children are the part of the inner timeline
that the placement shows, clipped to that window and moved to the group clock.
A constant-rate inner media clip trims its source bounds proportionally to its
authored source and timeline spans, retaining its editable playback keys,
including reverse playback. Source-time remapping inside the nest remains a
diagnosed per-occurrence omission. Each moved boundary lands on the millisecond of its rounded absolute outer
time, so nested boundaries follow the top-level rounding rule. The inner black
canvas is not copied, so inner gaps stay transparent and show lower outer
tracks. AME renders them transparent too: the AME reference of
`premiere_isolated_nested_sequence` shows the red outer clip at outer 9-10 s.
That is AME's reading of a derived nest, not of a Premiere-saved one.
Inline children use the same video mapping as root layers, including supported
Motion, Opacity and Crop; Crop guides stay in the same group as their video.
Linked transitions on a nest are reported as omitted, even without track-level
transition membership.

A retimed placement plays its window from In to Out over its range at one
constant forward speed. Premiere counts a nest's speed in inner frames per
outer frame (Out − In = duration × speed × the inner frame duration ÷ the
outer frame duration). The reader
admits a speed that the saved window confirms within 4 ticks and rejects
reverse and time remapping; the window may lie off every frame grid and its
sequence may have another rate. Import keeps the inner placements that share
time with the window at their inner-clock places, untrimmed, under one linear
group playback from the placement onto the window, each end rounded once to a
millisecond, so In applies once. Its Motion keys and its Frame Blending or
Optical Flow are reported, not converted (static values stay), and it has no
sound: an audio item plays at normal speed, so none pairs with it. A retimed
nest inside a normal-speed nest trims its window in proportion and keeps its
rate. Export writes no retimed nest. The nest tests use synthetic records;
no Adobe-native fixture or render in this repository proves the mixed-rate
rule or the Motion behavior.

Export writes each group (no background or its keys,
time remap or motion blur; a name of 1 to 255 characters) as a new inner
sequence of its children and a placement from inner time zero over the group's
range. Boundaries snap as absolute document times. The placement carries the
group's edits as a clip carries its layer's, with the clip's exporters, checks,
chain and records: Motion (its anchor normalized to the canvas-sized picture),
Opacity, its Blend Mode, Opacity, Position, Anchor Point, Rotation, uniform Scale and Scale Width keys counted from the
placement start, one Crop or Linear Wipe mask whose guide is a child of the
group, or one track matte whose source is a sibling of the group
(`canonical_mask`; the guide is no content of the nest, and the source is
placed above the nest as for a clip), then the group's effects, and Enable. Premiere 26.5.1 reopens such an export as written, and
AME renders it (a scoped gate on one document, not a visual score; evidence in
`premiere_export_edited_group`). A Normal group without masks or
a track matte reports once when a layer inside it blends and its Opacity is full
at some time of its range, as import reports its nest: FX draws such a group
into its parent at every frame where it is at full Opacity and renders no
effect, where Premiere composites the nest alone. The Opacity is full by its
static 100, a key or held value of 100 in the range (including a value held from
keys outside it), or a Linear value of 100 that the range meets. The report is
conditional, that the group passes the blends through at any frame, if there is
one, where its Opacity is full and none of its effects isolates it, when only an
upper bound reaches 100 (a cubic Bézier ease's control values, or a Linear value
approached at the range's end) or when the group has an effect that FX may
render. FX renders nothing on a group for a disabled effect, a person or depth
matte, Posterize Time or an effect of an unknown type; whether it renders
another effect can depend on its values at a frame (a blur of 0 renders
nothing), which export does not decide. So any other effect makes the report
conditional, a conservative bound that also reports a group that the effect
isolates at every frame. Motion that moves no pixel
stays Premiere's default, so a plain group writes the plain placement. A group
whose own animation the
placement cannot carry whole (a key on another property, Scale keys that differ
between the axes, or a track that the key export rejects) is omitted, so no key
is dropped from a placement. A stage group exports as one clip
([Effect stacks](#effect-stacks)), or as a nest when one clip cannot carry it
and its video spans the group; other groups are omitted with their reason, and
media under them is not inspected. A group longer than its children would place past
the end of its inner sequence, so it rejects the export. A nest never counts as
canvas coverage: the black canvas must lie under every part of a nest range that
no media covers. The writer emits a Sequence, master clip, sources and project
item for every inner sequence, which do not load as top-level timelines. Export
does not rebuild sharing: copies of one Premiere sequence become separate inner
sequences. Native writing rejects transition records at every sequence depth,
rather than silently dropping inner transitions.

## Tests

Run `make test` and `make clippy` from the repository root.
Add cases by responsibility, not by feature ticket:

| Owner | Coverage |
| --- | --- |
| `format::tests::{graph, reader, audio}` | Native records, references, selection, and unsupported fields |
| `format::tests::animation` | Motion and Opacity keys: easing, handles, flags, both layouts and blend pairs, and a Bezier into a Hold key (a scalar against Premiere's readback; a Point key unchanged) |
| `format::tests::graphic` | Type-tool graphic records, keys (Bezier against the probe's saved keys and readbacks, and a Bezier into a Hold key against an export gate's readbacks), the clip Opacity, `DefaultOpacity` read as on a media clip, transform composition, objects in chain order with Shapes, and graphic omissions |
| `format::shape_payload::tests` | Shape Path and Appearance payloads from calibration projects that Premiere saved and AME rendered, round trips, and fail-closed slots and values |
| `format::tests::caption` | Caption track records, cue timing and lanes, bottom placement, per-cue styles against the template, background boxes, visibility checks, and caption omissions |
| `format::text_payload::tests` | Source Text payloads from real Premiere projects, round trips, and fail-closed slots |
| `convert::text_shadow::tests` | Text shadow units in both directions, shadow limits, and omitted text-layer effects |
| `convert::graphic::tests` | Graphic keys in both directions: FX tracks, generator-clock timing, reported properties, keys beyond former count quotas, Bezier curves written back to the probe's saved speeds, the clip Opacity as group opacity, edits read back, each graphic-group rejection, and root or group text with masks or a track matte; shapes: stroke joins by corner angle, shadows, keyed Vector Motion over several objects, and each shape rejection |
| `format::tests::timeline` | Ranges, frame alignment, overlap, gaps, source bounds, and animation key limits |
| `convert::{premiere_to_tesseract, tesseract_to_premiere}::tests` | Each direction's mapping, document properties, canvas rules, text font identity, persisted time precision, and the export of a Bezier into a key that starts a Hold |
| `format::tests::writer` | Generated native records, references, escaping, and encoding limits |
| `format::tests::effects`, `convert::effects::tests` | Standard effect records, the chain split, stack order, bypass, keyed parameters, and effect and occurrence omissions |
| `format::tests::nested`, `format::reader::nested::tests`, `convert::nested::tests` | Nested placements: native shape, limits, read bounds and rejections, group mapping, export, and writer read-back |
| Public conversion tests | Media binding, archives, relocation, publication, and check/execution parity |
| `tests::operations` | Prepared operations, late source changes, and staging cleanup |
| `tests::omissions` | The bounded omission list: first-report order, deduplication, the 1024-entry and 1 MiB limits, the final summary, and merged lists |
| `publication::tests`, `tesseract_import::tests`, `premiere_package::tests` | Hard-link publication, the exclusive-copy fallback with staged permissions, existing-entry and symlink safety, and removal of owned partial and published files |
| `tests::audio_media` | Audio layout, sample rate, and presentation duration from real containers |
| `tests::video_format` | File names, sample-entry codecs, HEVC parameter sets, colour pass-through, Dolby Vision configurations, data tracks, and `fiel` field metadata |

Model and mapper tests use in-memory inputs and explicit expected ranges.
The two mapper directions have independent input fixtures. Shared test support
contains only concrete document loading and file-writing functions. Do not build
an archive to test another timing, track, or document-property case.

Keep boundary coverage where it catches a different failure: for example, raw
archive JSON must not lose fractional timing through typed normalization, and
repeated placements must still package one asset. Representative reader/writer
checks also prove that the shared timeline validator is called.

Rust tests prove structural rules and file handling. The pinned single-style
point-text fixture has independent AME exports and an approved Premiere/FX render
comparison in [`tests/README.md`](../../tests/README.md).
That narrow evidence does not prove other text styles or Premiere UI state.
Successful self-reading does not establish Adobe compatibility or rendering
fidelity.

The measured default Film Impact Pop profile imports as editable scale/position
keys on supported flat pictures, preserving source clocks and independent tail
Dissolve opacity. See the [profile boundary and evidence limits](../../docs/formats/premiere.md#measured-default-film-impact-pop-entrance).

A default Cross Dissolve (Legacy) imports only as the incoming-only head of a
static Normal Color Matte on the document clock that no Track Matte Key uses:
two editable Linear Opacity keys from 0 at the cut to the matte's static
Opacity at the transition end, which replace that value. The Legacy curve,
frame phase and compositing space are unmeasured, so the ramp is a declared
approximation; every other form keeps its `Cross Dissolve detected` omission
([format table](../../docs/formats/premiere.md#timeline-playback-visibility-and-nesting)).
Synthetic structural tests cover this mapping; it is not a visual pass.

[Irregular presentation timing](../../docs/formats/premiere.md#irregular-presentation-timing--import-only-approximation)
imports as an explicitly warned approximation only after bounded physical PTS,
full-source edit, unlisted native rate, exact sample-count/duration and rounded
endpoint checks, or the proved selected-picture interior exception above. Original
media bytes and unit source playback are preserved. If any selected root/nested
picture use is retimed or has Time Remapping, all placements of that
irregular media are omitted; Constant and quantized rules remain separate, and export stays Constant-only.
Aggregate agreement does not prove intermediate native frame selection. The Gail
missing-timestamp discriminator remains pending independent native evidence.

Measured neutral Film Impact Stroke imports as an editable physical-picture
wrapper for the static 6/99, 6/100 and 66/99 profiles. Original source clocks and
Pop/Dissolve owners are preserved; the border is explicitly approximate. See
[the profile and host bounds](../../docs/formats/premiere.md#measured-neutral-film-impact-stroke-profiles).
