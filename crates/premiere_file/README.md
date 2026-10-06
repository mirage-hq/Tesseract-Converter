# Premiere conversion

`premiere_file` converts one Premiere sequence to an editable Tesseract FX
composition and exports edited FX content to a new Premiere project package.
It reads native `.prproj` XML, including gzip-compressed projects, without
launching Premiere or After Effects.

Conversion is best effort. Unsupported features produce contextual diagnostics;
malformed input, failed media admission, integrity failures and I/O errors can
stop conversion. Success does not mean that every edit survives or that the
rendered result matches Adobe output.

## Build and run

Run these commands from the converter workspace (`opensource/conv`):

```sh
make build

target/debug/tsrct-conv inspect source.prproj --metadata-only --json
target/debug/tsrct-conv inspect source.prproj --sequence 'GUID-from-inspect' --json

target/debug/tsrct-conv convert source.prproj --to tesseract \
  --sequence 'GUID-from-inspect' --output imported

target/debug/tsrct-conv convert imported/project.tsrct --to premiere \
  --output exported --fps 30

# Validate without publishing a final output directory.
target/debug/tsrct-conv convert source.prproj --to tesseract \
  --sequence 'GUID-from-inspect' --output checked --check
```

The binary is under `$CARGO_TARGET_DIR/debug` when that variable is set.
The output parent must exist. The destination directory must be new, including
no dangling symlink. Conversion writes `project.tsrct` on import and
`project.prproj` with packaged media on export.

Select a sequence by its exact native GUID, not its name. Selection can be
omitted only when the project has exactly one selectable sequence. Nested
sequences also appear in the target list. Metadata-only inspection lists native
targets without inspecting media or converting the timeline.

Ordinary inspection also checks reached media. Use explicit media preparation
and a source-bound media map when a source needs conversion; inspection itself
never starts a decoder process or changes the project.
See [media preparation](../../docs/media-preparation.md) and the
[CLI reference](../../apps/tesseract-conv/README.md).


The CLI can coordinate native Premiere content with editable linked AEP picture
scopes. The library's native export does not perform that cross-format routing.
See [hybrid export](../../docs/hybrid-adobe-export.md) for the package contract
and its limits.

## Rust API

The convenience functions use a Boolean `check` flag and export at 30 fps:

```rust,no_run
use premiere_file::{premiere_to_tesseract, tesseract_to_premiere};

fn main() -> Result<(), premiere_file::ConversionError> {
    let omissions = premiere_to_tesseract(
        "source.prproj", "imported", Some("native-sequence-GUID"), false,
    )?;
    for omission in omissions {
        eprintln!("{omission}");
    }
    tesseract_to_premiere("imported/project.tsrct", "exported", false)?;
    Ok(())
}
```

Use the shared `fx_conv` traits for explicit modes, frame rates, progress and
artifact reports:

```rust,no_run
use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
use premiere_file::{FrameRate, Premiere, PremiereExportOptions, PremiereImportOptions};
use std::path::Path;

fn main() -> Result<(), premiere_file::ConversionError> {
    let targets = Premiere.list_import_targets(Path::new("source.prproj"))?;
    let options = PremiereImportOptions {
        sequence: targets.first().map(|target| target.id.clone()),
    };
    Premiere.import_to_tesseract(
        Path::new("source.prproj"), Path::new("imported"),
        &options, ConversionMode::Check,
    )?;
    Premiere.export_from_tesseract(
        Path::new("project.tsrct"), Path::new("exported"),
        &PremiereExportOptions { frame_rate: Some(FrameRate::Fps25) },
        ConversionMode::Check,
    )?;
    Ok(())
}
```

A conversion report contains diagnostics and package-relative artifact paths.
Check mode reports planned artifacts; Write mode publishes them. Progress-aware
trait methods use the same conversion path.

`ConversionError` preserves its underlying source chain. Use `is_unsupported()`,
`is_missing_media()` and `is_io()` to classify those failures without parsing
display text. An unresolved required media path differs from a file-read failure
such as a file removed after resolution.

`PrProjectFile` exposes native project loading and read-only sequence/media
information. It is a supported-subset model, not a lossless representation of
all Premiere records.

## Supported subset

The tables below summarize the supported forms, not unconditional support for
every combination. A feature can have a narrower boundary on stills, graphics,
nests, adjustment layers or retimed owners. Diagnostics identify the affected
feature, occurrence, track or sequence.

The [format reference](../../docs/formats/premiere.md) and the
[CLI support tables](../../apps/tesseract-conv/README.md#supported-subset)
describe the detailed mappings and limitations.

| Content | Import | Export |
| --- | --- | --- |
| Video | Editable clips, source trims, supported Motion/Opacity, enable state and blend modes. | Current edited clips and admitted media, with timing snapped to the selected sequence grid. |
| Stills | Admitted PNG, JPEG and WebP as editable images; bounded Motion, Opacity, effects and masks. | Still media and supported current image edits. |
| Numbered images | Finite consecutive image sequences with a separate source clock. | Selected timed images as native still placements. |
| Text and graphics | Supported Source Text, shape/appearance payloads, SubGroups and numeric keys. | Current supported text, shape and group content, not hidden original payload replay. |
| Color Mattes | Editable full-canvas rectangles, with bounded Opacity, Crop and Track Matte forms. | Supported rectangles as native mattes; other forms may become graphics or be omitted. |
| Adjustment layers | Editable effect coverage, bounded Opacity and static coverage Motion, and a bounded Geometry2 (Transform) group approximation. | Supported current adjustment content and coverage guides. |
| Nests | Editable groups with supported placement, clocks, effects and separate audio. | Supported current groups as native nested sequences. |
| Captions | Supported caption content and presentation. | The supported caption subset; unsupported presentation is diagnosed. |
| Linked AEP compositions | Editable picture and independent sound selected by file and native composition GUID. | Supplied linked picture scopes through the package coordinator. |
| Multicam cuts | Bounded unit-speed cuts from the saved selected camera, as ordinary video clips. | Ordinary edited clips, not multicam editing state or unused cameras. |
| Proxy attachments | Primary media remains the source; preview attachments are not substituted. | Current primary media; attachment and preview-preference state is not reconstructed. |

### Descriptive movie metadata

Optional ISO/QuickTime `meta` and `udta` tags are not interpreted as FX content
and their payload grammar does not gate picture or sound admission. The packaged
asset retains the original tags and media bytes. Box headers use a constant-size
cursor rather than retaining unused atom ranges. Optional atom counts have no
fixed converter cap; actual allocation failure for consumed facts remains fatal.
Required box framing, sample identity, picture facts, source clocks and I/O checks remain.
This is editable-content retention, not validation of those tags or a new native
render-fidelity claim.

### Intrinsic Opacity metadata

Import reads saved Opacity and Blend Mode values independently of optional
editor bounds, ClassID and control-type metadata. These descriptive fields never
widen supported values: alpha remains 0–100, and blend IDs must be integers in
0–255 before using the existing mapping and diagnostics. Parameter identities,
names, bypass state and animation consistency checks remain.

Public native-fixture mutations cover metadata-independent retention of editable
values, masks and siblings, plus invalid blend-ID and out-of-range-alpha rejection.
Historical private-source checks established editable Screen and alpha keys;
this is structural evidence, not new Adobe
opening, UI inspection or alpha/RGB fidelity proof. Export is unchanged.

### Effect stacks

Mapped effects retain stack order and bypass state where their host supports the
mapping. Controls and supported animation become editable FX effects or groups.
Export writes current edited values, not cached native input.

The native subset includes bounded forms of Gaussian Blur, Directional Blur,
Corner Pin, Levels, Brightness & Contrast, RGB Invert, Tint, Black & White, Ramp,
Mosaic, Replicate, Posterize, Posterize Time, Sharpen, Find Edges, Transform,
Legacy Luma Key, Noise, Alpha Glow and Lens Distortion. Selected Film Impact and
Lumetri controls have narrower mappings or approximations. Matching an effect
name alone does not establish support: its saved parameter layout, values,
clock, mask and host must also fit the supported form.

Effects beside Crop, Linear Wipe, Opacity masks or Track Matte Key have ordering
and coverage restrictions. When one FX mask cannot represent the native chain,
conversion diagnoses the unsupported combination rather than silently changing
its processing order. Spatial kernels, edge behavior, noise and colour math can
differ even when the controls remain editable.

### Root adjustment Geometry2 suffix

Outside the unchanged scale-only adjustment zoom subset, a true neutral
adjustment with `AE.ADBE Geometry2` alone has a separate editable
composed Group G × M mapping (`convert/adjustment_geometry.rs`). A root sequence
matching the document canvas also admits apply order `[Geometry2, mapped suffix…]`:
selected Lumetri replacements, standalone Brightness & Contrast (indistinguishable
from Lumetri's Contrast replacement), Legacy/current Gaussian Blur, Sharpen and
Legacy/Modern Noise reuse their ordinary mappings. The Group owns
its interval and local clock, carries Position, positive uniform Scale, Rotation
and Anchor (including animation), and masks an unchanged lower picture through
a nonpainting sequence-canvas Rect/Add guide. Lower occurrences must share the
whole interval; crossing/partial windows, prefixes/mixed stacks and other sibling
effects are reported without rewriting the lower picture. Lower Adjustment layers
are not relocated into the picture stage. For the admitted suffix, top-to-bottom
ownership is upper siblings → original suffix Adjustment A → fresh G containing
lower pictures and its guide → time-disjoint lower siblings → root black canvas.
A keeps its effect IDs, enabled states and original layer-local key clocks; G has
no effects. Even an empty, disabled or rejected ordinary suffix keeps A, so a
valid G is not lost. A uses the existing document-plane/backdrop semantics,
including the root black canvas outside G; this is not an alpha equivalence claim.
In FX, Grain and contrast reduction can texture or lift that exposed black;
native transparent-exterior behavior for these suffixes remains unmeasured.
Import reports the A/G backdrop and whole-stage-omission limits. Export reports
those limits for a root Adjustment immediately above a same-range Geometry2
stage. This bounded adjacency check neither establishes suffix provenance nor
predicts native Geometry2 lowering, and does not cover intervening-layer or
different-range arrangements.
Intrinsic adjustment Motion is
still coverage, not G. Straight Position segments use temporal easing alone;
curved paths retain editable tangents with a diagnostic for parametric rather
than native constant-speed traversal. Skew, opacity mix, motion blur, bicubic
sampling, nonuniform scale and nonneutral intrinsic adjustment controls remain
outside this admission.

Export recognizes the imported `Premiere adjustment Geometry2 ` Group name,
validates its current unit clock and whole-canvas nonpainting guide, and writes
an ordinary identity nest with the current lower children plus a flagged native
Geometry2 adjustment. The independent suffix A exports as an ordinary true
adjustment above that identity nest, using current editable values/order/keys.
Lumetri export remains partial: only its Contrast replacement writes native
Brightness & Contrast; the other ordinary replacements are diagnosed and omitted,
not reconstructed as Lumetri.

In the suffix representation,
invalid G omits G and its lower picture while independent A can survive over the
remaining lower stack; it does not restore the missing dry picture. A rejected
suffix effect follows ordinary effect omission without removing valid G.

The `adjustment_geometry2_suffix_`
regressions cover composed ownership, native-derived donor payloads, independent
effect/layer IDs and clocks, current-edit native wire/reimport, moved-nest Opacity
and invalid-stage media-inspection agreement. These combined models are
supplementary, not independently Adobe-saved mixed-stack sources. Combined native
source/render proof is pending in both directions; generated native exports
have not been reopened in Adobe. Raster/edge-alpha differences, animated Anchor
rendering, and millisecond media-phase limits remain in the
[accuracy ledger](../../apps/tesseract-conv/README.md#conversion-accuracy-and-limitations).

### Per-channel Levels (bounded editable graph)

Static per-channel Levels can become independent master-then-channel branches
with channel selection, Screen composition and alpha recovery. Channel values
are not averaged into a master setting. The copies are independent editable
controls; finite precision, small-alpha division and clipping can differ.
Unsupported hosts and masked channel coverage retain their normal rejection.
The CLI can export edited channel branches through linked AEP scopes when the
native Premiere path cannot represent them directly.

### Premiere Invert Alpha (bounded editable replacement)

The bounded Alpha-only Invert mapping uses editable picture/matte groups, not
shader source or prerendered replacement footage. Original source timing and
independent sound remain separate. Unsupported owners, masks, clocks and
combinations keep their contextual diagnostics. This does not extend ordinary
RGB Invert to every native channel selector.

### Premiere Legacy Luma Key

Threshold and softness have an editable approximation with a minimum falloff.
Export writes supported current values and keys. Invalid or animated inversion
and unsupported coverage forms are diagnosed; no unsupported native control is
invented. This is not an exact native alpha-kernel replacement.

## Timing

Premiere uses integer ticks; editable FX uses integer milliseconds. Timeline
and source clocks are kept separate.

| Direction | Boundary handling |
| --- | --- |
| Premiere to Tesseract | Round absolute timeline endpoints to milliseconds, nearest with ties forward. Each endpoint moves at most 0.5 ms. Duration is the difference of rounded endpoints. |
| Tesseract to Premiere | Snap absolute timeline endpoints to the nearest output frame, ties forward. Each moves at most half a frame. Source-in remains exact in milliseconds; unit-speed source-out follows the snapped duration. |

Placement boundaries and sequence ends need not coincide with native frame
samples. Import retains coherent tick ranges without frame snapping; fractional
bounds alone do not reject graphics, captions or nested children. Inline
nests retain the residual phase when a parent origin is rounded to milliseconds,
with a contextual normalization diagnostic. Source bounds, key-clock consistency
and the existing reverse-source endpoint restrictions remain checked.

Shared cuts remain shared. Rounding can change which source frame a decoder
selects near a boundary, even when durations and adjacent cuts agree.

Import accepts listed frame rates and positive native sequence tick periods.
A non-listed native period carries a diagnostic; the editable document does not
store sequence cadence for automatic restoration on export. Media records retain
their own clocks and interpretation separately from the sequence clock.

### Point animation import

Import also admits spatial mode 0 with saved flags 2 only when all four tangents
are zero, using the same tangent-free representation as flags 0. Saved values,
source times and temporal easing retain their existing mapping; nonzero tangents
and other spatial forms keep their guards. The saved flag is not retained as an
FX editing control. Flag-2-specific native render fidelity remains unmeasured.

### Export rate

Export defaults to 30 fps. Supported output rates are 24000/1001, 24, 25,
30000/1001, 30, 50, 60000/1001 and 60 fps. Select one with `--fps` or
`PremiereExportOptions::frame_rate`.

### Retiming

Supported constant speeds, reverse playback and native Time Remapping import as
editable playback ranges or keys. Frame Blending and Optical Flow are not
reconstructed. Retimed Motion/effect keys can retain static values with a
diagnostic when their native clock cannot map to the edited owner.

Export has a bounded constant-speed representation. Unsupported playback keys,
held or reversed forms, and nested clock combinations are diagnosed rather than
claimed to round-trip exactly. Irregular media cadence and source interpretation
have their own admission rules; sequence sampling does not make physical media
constant-rate.

### Nested reverse (import only)

Unit reverse placements with matching frame clocks retain saved Clip In/Out and
reflect their picture window about the sequence source's saved OriginalDuration,
not the current inner duration. The reflected, frame-aligned window must lie
inside the actual child timeline. An ordinary occurrence Group carries Motion
and Opacity keys on the increasing Clip clock; an editable decreasing TimeRemap
on a separate picture Group drives untrimmed descendant animation. Independent
native sound items retain their existing forward clocks and never join that
reverse picture. Supported unmasked occurrence effects keep their editable
parameters on a separate increasing-clock stage before Motion; reverse effect
key timing is explicitly diagnosed as an unmeasured approximation. Nonunit
reverse, remap combinations, occurrence Transform and other unestablished
coverage clocks remain unsupported; static zero-feather Crop without occurrence
effects is retained. An unrepresentable occurrence Motion key parameter keeps
its authored static value with a feature-specific diagnostic; other keys,
descendants and source playback are preserved. Malformed required structure
and incoherent source clocks still fail validation.

Public native-wire scaffold, key-clock and publication regressions establish
structure, not independently Adobe-measured reverse-nest frame selection, RGB,
alpha or audible fidelity. Existing millisecond rounding applies. Export support
is unchanged: this mapping does not promise reverse-nest round-trip fidelity.

### Media interpretation

Saved pixel aspect ratio maps to editable nonuniform scale on the decoded
picture's axes. Export preserves media bytes and writes square-pixel native
interpretation so the scale is not applied twice. This is an editable
approximation; source-space masks and effect kernels can still differ.

Video admission checks container, codec, dimensions, source timing, rotation,
pixel aspect and colour declarations. Supported HDR bytes pass through with
colour diagnostics; contradictory declarations and unsupported codec forms do
not become valid merely because the filename ends in `.mp4` or `.mov`.

H.264 decoder-configuration reserved bits do not gate supported pictures.
Admission uses the encoded NAL length, parameter-set counts and extension values;
required record version, framing, parameter-set content and bounds remain checked.
No encoded media or configuration bytes are rewritten. Public native-derived
regressions assert editable clips, source trims, siblings and exact packaged bytes;
this is not a new native render-fidelity claim.

A media-header duration longer than the exact sample-table extent can import
only for proved picture-only unit-forward source intervals. The excess must be
smaller than the final decoded sample duration; every packet DTS must match the
unmodified STTS table, and the native count/duration must match the original
header. Selected intervals stay inside the first unit playback edit and end no
later than the final presentation sample's start, excluding that uncertain sample
and the declared tail. Original bytes, sample timestamps and clip trims are
retained with a diagnostic, not rewritten to a constant-rate grid. Whole-source,
interpreted/retimed, audio-consuming and unsupported-tail uses remain rejected.
A positive CTTS origin under one zero-media-time unit edit can likewise retain
selected picture-only content. Independent raw/displayed packet inspection must
prove one translation with identical sample identities, order, DTS and PTS. The
existing editable source trim/playback carries that offset, rounded forward to
milliseconds to avoid pre-origin requests; both native and rounded windows stay
inside proved sample coverage. The diagnostic records the exact offset and its
rounded value. Timeline placement, Motion/effect animation and masks keep their
original clocks. Whole-source, audio, negative/retimed/multiple-edit and unsafe
clock cases remain rejected. This is structural admission with a sub-millisecond
origin approximation, not Adobe frame-selection, visual or alpha proof.

### Explicit media relinking

Import can relocate media before native path admission with
`Premiere::import_with_media_relink` or CLI `--media-relink JSON`.
The version-one `MediaRelink` binds the exact input project SHA-256, selected
sequence UID, native media UID and exact authored `FilePath` to an absolute
local path and that file's SHA-256. This is explicit caller authority, not
proof that Adobe used the selected file. Public path-relocated inputs must
use their actual input hash, not an original render-reference hash.

Unknown or duplicate UIDs, mismatched source/target/path, conflicting live
aliases, changed files and retargeted symlinks reject. Native media, clock,
dimension and publication checks remain; bindings are checked again before
publication, including bindings whose occurrences were omitted.
`--media-map` instead substitutes prepared bytes after ordinary native
resolution. The CLI does not combine the two inputs. Export is unchanged.

### Numbered images

A numbered still source must declare a finite consecutive filename range with a
bounded source clock. Missing frames and inconsistent images are rejected.
Export writes the selected pictures as ordinary still occurrences. It does not
restore an AI controller or infer an unlimited sequence from directory contents.

Saved Object Mask `.prmf` coverage can be recovered into numbered PNG mattes.
The original source video remains editable. AI selection, propagation state and
re-propagation are not retained. See the
[Object Mask fixture limits](tests/fixtures/object_mask/README.md).

## Outbound physical picture media

Bounded progressive H.264 High10/sRGB and ProRes sources can use the existing
editable linked-AE scope while keeping independent native pictures and audio.
Export preserves original video bytes; it diagnoses unsupported AE interpretation
rather than remuxing, downconverting or baking. Container/header structure,
packet ranges, source clocks and archive integrity must pass first. Malformed
headers, conflicting declarations and I/O failures remain fatal. Import admission
and the omitted-root retention guard are unchanged.

`export_media_` regressions establish structural behavior, not full compressed-frame
decoding, Adobe colour/alpha/render fidelity or edit-propagation proof.

## Audio

Picture and sound have separate native occurrences and source windows. Import
keeps supported mute, gain, Volume keys and fades without inventing sound from a
picture-only source. Linked composition sound follows its independent audio
occurrence, not every linked picture placement.

Admitted mono/stereo audio can be packaged from WAV, MP3 and supported AAC
sources. Original channel selectors have bounded full-source mono extraction.
Export writes supported current gains, keys, trims and fades. Unsupported audio
layout or processing is diagnosed while supported picture and sound siblings
remain. After independent video admission, a missing embedded stream or an
unsupported native audio layout, sample rate or duration match omits only that
source's sound occurrences, including nested copies; its muted picture, source
windows and original bytes remain editable. Audio validation is not relaxed.
Malformed native records, failed reads and integrity checks still stop
publication. Native-derived metadata-mutation and nested model tests establish
structural retention, not independent Adobe render or audible fidelity. Export
is unchanged.

### Outbound source duration

Audio export accepts the nearest millisecond or the exact floor of the inspected
positive whole-sample duration. The floor representation has a diagnostic; it
does not change source samples, ticks, playback windows or gain. This is not a
general ±1 ms tolerance: genuine clock mismatches remain errors. Validation
uses the active audio asset, not an inactive original.

### Audio insert filters

Static bypass skips insert processing. The bounded native Fill Right with Left
form duplicates original channel 0 on the admitted centered route. Export uses
ordinary media and edited gain controls rather than reconstructing the insert
stack. Unsupported pan/routing, nested processing, spectral filters and pitch
transposition keep diagnostics; their normalized values are not guessed as dB.

### Custom fades and short spans

One-sided Custom Fade has a bounded squared-sine power-family approximation.
Outgoing fades use the time mirror of the incoming curve, not its amplitude
complement. Unsupported custom crossfades, shape types and parameters preserve
sound with a diagnostic. Very short fades can move their full-level edge inward
to a sequence frame, constrained by the clip and opposing fade. This changes
the envelope and is reported; it does not certify sample-level audio fidelity.

## Fonts

Import stores native text fonts by PostScript name, with an intentionally empty
style. It does not consult the renderer's bundled font catalog or package the
source fonts, and reports missing font delivery. An empty native Text without a
font uses the editable default with a diagnostic.

Export resolves a packaged font face from the archive's registry. An empty style
means the family already contains the PostScript name. Other unresolvable
family/style pairs omit the affected text. Conversion does not guarantee font
availability in Premiere or identical text metrics on another machine.

### Graphic export failure isolation

Within an admitted graphic group, an unsupported object or missing font omits
that object rather than independent supported text and shapes. Failed mask
sources still omit their dependent composites; they never reveal their consumers.
Group placement, clocks, visibility and transform admission remain unchanged.
An unsupported optional Source Text/alignment key unit retains the independently
valid base document and supported Motion keys, with a diagnostic; no partial
Source Text track is written. A mask whose Source Text keys fail is omitted with
its consumers instead of using altered static coverage.

Focused native-derived and direct export regressions establish editable structure,
ordering, placement and retained key accounting. Missing-font/key mutations are
structural controls, not independently Adobe-authored failures. Adobe reopening,
RGB/alpha fidelity and native font delivery for this recovery remain unverified.

### Saved AE capsule graphics

A bounded saved `.aegraphic` or `.mogrt` capsule can import as independent
editable Text and Shape objects through the typed AEP reader. Export uses the
current edited ordinary graphics; it does not replay or restore the capsule.
Ambiguous containers and controller identities remain errors. Saved text,
scalar/toggle/angle, RGB colour and point overrides use real UUID/property-path
bindings; unmapped overrides retain the template value with a parameter-local
diagnostic. RGB overrides retain template alpha. Legacy saved text profiles retain
supported text/font/size/All Caps while diagnosing unsupported faux styles.
Saved type-4 strings and type-8 layout groups decode separately from Text: their
current UTF-16 values retain parameter/UUID bindings, and group members must
resolve without duplicates or cycles. They are not AEP Source Text overrides.
Unsupported text animator/style fields do not discard the ordinary editable text;
one supported static all-character stroke-width animator keeps its existing mapping.
Wire regressions establish decoding, not native opening or render fidelity.
Responsive layout uses a bounded static estimate, not an expression runtime;
text edits do not automatically resize the imported shapes. Template animation,
precomposition/media children and unverified masks remain limited: unsupported
mask consumers stay hidden, never exposed unmasked. File-backed containers are
read with seeks: unused media is not buffered or decompressed, and its size/member
count does not impose a separate admission cap. Directory metadata and each
consumed AEP/nested-graphic expansion remain bounded to 64 MiB, with path,
duplicate-target, encryption and consumed CRC checks. Font availability and native
RGB/alpha fidelity have separate limits.

## Nested sequences

Nested video becomes groups on the nested source clock. Supported placement
Motion, Opacity, enable state and source windows remain editable. Canvas clips
and source/outer transforms keep separate ownership.

Cycles, excessive nesting, unsupported source spans, retiming and mask/effect
combinations omit the affected occurrence. A group exported as a nest may use
the outer canvas with clipping rather than restore its original inner size.
These normalizations have diagnostics where the semantics differ.

Export omits unmapped Group motion blur separately: otherwise supported native
nests, Motion/Opacity, keys, child order, placement and disabled state survive.
Supported single-clip mask/effect stages and adjustment Geometry2 stages keep
their existing representation; flattened picture owners and direct timed-image
placements also report the lost flag. The picture-domain `MotionBlur` field
loss belongs to the retained owner, not its whole subtree. No motion-blur loss
is reported for an owner that produced no native picture. Required clocks,
structural values and coverage admission remain unchanged. This is partial
export, not motion-blur fidelity or retimed Group support.

Bounded intrinsic Opacity masks retain separate coverage, picture and Motion
owners on supported equal-canvas nests. Differing-canvas Transform has a static
16:9 subset and narrower import-only keyed Geometry2 translation and taller
rotation envelopes; other controls retain their omissions. Physical-video Frame
Hold children can survive supported unit-forward, matching-rate nested windows
without admitting held outer composites.

### Import-only same-width taller keyed rotation

A source canvas taller than its same-width parent admits one active, unmasked
Geometry2 with static coincident Anchor/Position, neutral scale/opacity, no skew,
motion blur or bicubic sampling, and one scalar Rotation track. Linear and
Bézier Rotation keys remain editable, including their numeric overshoot; they
are not replaced by the final orientation. Intrinsic Motion requires a centred
static Anchor, centred X, static positive uniform Scale, zero Rotation/full
Opacity and at most straight vertical Position keys with bounded easing.

Geometry2 points use the taller source canvas before Motion; Motion Anchor
uses that source canvas and Position uses the parent. Separate editable groups
retain both controls and clocks, with the source guide before Geometry2 and no
stationary post-effect source clip. Ordinary Transform and edited export keep
the static 16:9 boundary. Original-source RGB samples show closing-content
recovery, not every animated pose: sparse earlier correspondences and a large
remaining frame-210 mismatch do not isolate foreground occlusion. Full-scene
RGB still fails the strict gate. General overflow, transparent-edge and
independent alpha-output fidelity remain unverified.

### Unselected Track Matte Key

A static Matte None keeps supported unkeyed picture content, nest children and
animation, with a feature diagnostic. It selects and conceals no other track;
its unused Composite Using and Reverse controls do not discard the placement.
It also does not count as another active effect beside a mapped effect or key.
Unreadable or animated Matte selectors and invalid selected coverage retain their
existing diagnostics and dependent-content omission. Public saved-record tests
establish editable structure only; native RGB/alpha fidelity is unmeasured.
Export is unchanged.

### Nested matte Motion

A supported nested matte source keeps static Motion and convertible Motion/Opacity
keys on its existing editable Group, with pre-Motion source-canvas clipping where
needed and its supported children. Track Matte Key still consumes that rendered group;
the source is never substituted as ordinary visible content. Native sampling
and edge/alpha fidelity remain unmeasured and have an approximation diagnostic.
Unconvertible matte Motion keys omit the provider and its consumers rather than
freeze coverage at a static value. Disabled, missing, ambiguous, mismatched-range
or independently masked providers retain their rejection. FX→Premiere's existing
nested matte export checks are unchanged; no native controller, source payload
or rendered footage is replayed.

### Nested occurrence effects

Mapped non-Transform effects can apply to an identity picture group before outer
Motion/Opacity. Group-relative spatial behavior is an approximation. Effects
without established group bounds and unsupported masked or retimed forms are
omitted with context. The separate Transform path has its own coverage rules.
Export retains the supported current group-to-nest picture boundary; it does not
establish support for every imported native effect.

Optional unsupported effect details do not discard a supported masked nest.
Its Track Matte Key or static Crop, values, children and mapped effect controls
and keys remain editable, with diagnostics only for the omitted details.
Mapped effects use the existing picture stage below the outer coverage owner;
Crop stays above that stage so spatial effects cannot reopen the cropped region.
Native effect/mask order and edge sampling can differ and are diagnosed.
Omitted Posterize Time leaves continuous sampling rather than its stepped cadence;
this is not temporal fidelity. Coverage-critical unsupported effects retain their
existing restrictions; no mask is removed and no hidden matte source is exposed.

## Timeline gaps and transitions

Export accepts uncovered gaps without requiring or inserting a black canvas,
and without a gap diagnostic. Import still adds an editable black canvas rectangle,
which export recognizes when it is unmodified.
Top-level gaps use Premiere's native black background; nested gaps can remain
transparent over lower content. This policy does not establish pixel or alpha
parity. Timing, duration and layer order remain unchanged.

Default Cross Dissolve has bounded one-sided and two-sided picture mappings.
Canonical current controls can export native Cross Dissolve records. Other
transitions and noncanonical edits retain their documented omissions or
approximations. See the [timeline support table](../../docs/formats/premiere.md#timeline-playback-visibility-and-nesting).

## Script animation export

FX `JsScript` animation can be sampled and fitted into editable native keys for
supported scalar and paired controls. Import does not generate scripts. Export
uses the shared JavaScript runtime and fitter, not Adobe expressions, and leaves
the input archive and authored keys unchanged.

Sampling uses integer owner-local milliseconds and a bounded reverse-order
probe. The probe catches common history-dependent scripts but does not prove
determinism. Fitting tolerances are numeric/control-space bounds, not render
fidelity bounds. Sub-millisecond behavior and motion blur can differ.

Unsupported targets, dependencies, nonfinite values, invalid clocks and failed
sampling/fitting keep diagnostics. Whole-export call/key budgets and a
cooperative elapsed-time bound can stop export before publication. A single
JavaScript call is not preempted by that deadline. This is not a sandbox for
untrusted scripts or a bounded-memory service.

## Prepared export for package coordinators

`Premiere::prepare_export` and its progress-aware variant borrow the archive and
an explicit editable document. They run native preparation, media inspection and
lowering once. `PreparedPremiereExport` exposes the original and prepared views,
observed export losses and a packing recipe from the retained traversal.

`stage_with_picture_replacements` consumes that preparation and replays supplied
source-slot replacements without repeating script evaluation or lowering. Pass
an empty slice for native-only staging. The returned
`StagedPicturePremiereExport` owns a private temporary directory, an artifact
report, a generation-time project digest and required package-relative AEP paths.
Dropping it removes its private files, not the final destination.

The coordinator must supply foreign AEP files, verify the full package and
source freshness, preserve relative paths, and publish the final package.
An empty loss report is not a capability or fidelity certificate.

## Unsupported edits

Conversion is not a lossless native-project archive. Unknown component layouts,
unmapped effects, unsupported animation/easing, mask topology, host combinations,
colour/codec forms and routing can be omitted or approximated. An unsupported
field does not authorize replaying hidden native bytes, generating geometry
scripts or changing the FX renderer/schema.

Supported siblings survive source-feature omissions. Import omits missing direct
Premiere videos and unsupported codec sources with contextual occurrence
diagnostics, including when other sources use explicit media relinks. Failed
mattes also omit their consumers; no opaque substitute, proxy or slate is added.
Inspection readiness remains strict. Native used-media inventory is checked even
when the editable reader omits an occurrence. Malformed present media, unsafe
paths, identity, source dimensions/clocks and I/O failures remain fatal.
Unsupported linked AEP video remains fatal; genuinely missing linked footage
has its existing contextual omission policy. This is partial editable recovery,
not a visual, alpha or audio fidelity claim. Export admission is unchanged.

## Validation and publication

Check and Write share conversion and validation. Check publishes no final
output, but it can create and clean temporary preparation files. It cannot
promise that a later filesystem write will succeed.

Write rechecks source and media identities before publishing. Publication uses
fresh destination paths and rolls back files it created on failure. It is not
an atomic directory transaction: concurrent path replacement or a process crash
can leave partial output. Keep source files stable until conversion returns.

## Tests

Run from `opensource/conv`:

```sh
make fmt
make clippy-premiere-file
make test-premiere-file

# CPU coverage that does not require FFmpeg media inspection.
make clippy-premiere-file cargo_args=--no-default-features
make test-premiere-file cargo_args=--no-default-features

make test-premiere-file filter=mask
make check-conversion-fixtures
```

The default `ffmpeg-library` feature requires FFmpeg 7 development libraries.
Use `FFMPEG_PKG_CONFIG_PATH` to select them if another FFmpeg version is the
system default. Without the feature, native metadata and pure conversion tests
remain available; tests that inspect or prepare MP4-family media are gated.

Fixtures cover native records, editable structure, rejection paths and fresh
package publication. Synthetic edits and internal round trips are supplementary
coverage, not independent Adobe acceptance or RGB/alpha/audio equality.

Public fixture derivatives anonymize absolute path metadata. Their current
hashes and original source hashes are recorded in
[path sanitization provenance](tests/fixtures/path-sanitization.json).
Native reference records retain the hash of the original rendered source, not
the derivative. Source identities, controls, timing and media sample bytes are
unchanged by path sanitization.

For the registered independent-reference cases and their specific comparison
policies, see the [test inventory](../../tests/README.md) and
[fixture manifest](../../tests/manifest.json). The
[format reference](../../docs/formats/premiere.md) distinguishes implemented
mappings from their validation limits. No overall Adobe fidelity pass is implied
by a successful Rust test run.

## Earlier Source Text profiles (import)

Saved graphic documents with the supported FlatBuffer markers can include an
empty default run in document slot 8. Import validates the run and retains
actual text when that separate insertion style differs or has an unmapped
control, with a warning that inserted text uses the editable document style. Nonempty or structurally
malformed run data remains unsupported. Existing paragraph box dimensions and
alignment retain their ordinary checks; other earlier document revisions do not
become supported.

Legacy UTF-16 JSON retains actual text, font, size, paint, box dimensions,
leading, justification and All Caps in the existing editable representation.
Optional inactive decoration fields need not form a complete profile; unused
values and version metadata are not admission requirements. Unmapped character,
paragraph and unknown controls get field-specific diagnostics instead of
omitting the whole text. Active or malformed mask controls remain unsupported
so recovery cannot expose concealed content. Required framing and consumed
style bounds remain checked; legacy Source Text keys and mixed actual character
spans are not added by this repair. Shared admitted-text validation still rejects
outline-only text (stroke without fill); this decoder repair does not remove
that separate validation guard or claim recovery of those outlined labels.

Gray fill/stroke colors retain their values. An unverified legacy color order
uses the white editable text default with a diagnostic; stroke-above-fill uses
the existing fill-above-stroke order. Invalid stroke width omits only the stroke,
and leading below the target's 0.8 em minimum uses automatic leading. Font bytes
remain unpackaged. `legacy_*` tests assert reduced public wire data and editable
FX text alongside independent video content. This is structural evidence, not
independent Adobe render fidelity, alpha proof or UI inspection. Export is
unchanged and writes its existing supported native profile, not hidden original
payloads.

## Missing saved PAR overrides (import)

An enabled PAR override with no saved value no longer discards otherwise
supported picture. Import retains a valid serialized OriginalPAR or legacy
PixelAspectRatio; otherwise it resolves the identity-checked original file and
uses its admitted movie metadata or PNG pixel aspect. PNG pHYs defines the
ratio as vertical/horizontal pixel density; when absent, the PNG specification
defines square pixels independently of image dimensions. Other image formats
without a known source ratio remain unsupported. The missing override and exact
fallback origin/ratio are diagnosed. Present invalid ratios, conflicting paths,
corrupt media, alpha, color, dimensions and clock checks remain unchanged.

`missing_par_override_*` exercises inherited ratios, a non-square real file,
editable publication and conflicting-alias rejection; `png_pixel_aspect_*`
checks non-square density, the format default and unsafe metadata. These are
structural/metadata checks, not native override fidelity or new render proof.
