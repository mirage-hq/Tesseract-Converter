# After Effects native file adapter

Rust-owned bounded RIFX/AEP reader, best-effort **AEP → editable FX** import, and
experimental **edited FX → new AEP** export. The exporter writes current FX
content (including layer-local scalar/ShapePath-JS-to-editable-keys preparation
with redundant-sample reduction and fresh playback validation); it does **not** patch or replay the source AEP. Import never generates
`JsScript`. Native Shape/Mask Path keys have a bounded editable mapping in both
 directions; see [the Path-key evidence and limitations](../../docs/after-effects-support.md#native-shape-and-mask-path-keys).
Missing features and
approximations carry contextual diagnostics, but success or `--check` does not
mean that all pixels, alpha, audio or native controls were preserved. Run only
trusted scripts with external resource limits; VM bounds are not isolation.

## Video admission and explicit media preparation

Used unsupported video fails Check/Write rather than becoming an omitted layer.
`AfterEffects::inspect_media` uses native target reachability and relocation;
`import_with_media_map` applies source-bound replacements before the same admission
checks. Container/codec admission uses read-only libavformat inspection by default;
it never decodes or transcodes media. The separate
[preparation workflow](../../docs/media-preparation.md) owns explicit transcoding.
Building with `default-features = false` avoids native FFmpeg linkage but makes
video admission return an explicit unavailable-backend error. Independent Adobe/render,
alpha/color/audio proof for this workflow remains unmeasured; export is unchanged.

## PSD image import (bounded subset)

Local PSD v1 RGB8 footage can be normalized to PNG-backed FX `ImageLayer`s:
merged footage uses the stored composite; individual-layer footage requires the
native Photoshop layer ID **and** record index and retains cropped/full-canvas
placement. Decoding supports raw/PackBits compression without fixed input-byte,
pixel, layer-count or cumulative-read quotas. PSD v1 format constraints and
malformed-data checks remain; callers own process resource limits. Unsupported
profiles, layer masks/effects/blending, ambiguous selectors and malformed media
produce contextual omissions rather than a whole-image substitution. Existing
AE layer controls remain editable; Photoshop internals do not. Embedded color
profiles are not converted, with an explicit approximation diagnostic.

This is the user-approved **PSD import-only** increment. PDF-compatible AI and
PNG/PSD → fresh AEP export are deferred. Native author/readback and local Adobe
30fps renders exist for two pinned cases; Asset publication and fresh FX-render
comparison are incomplete, and alpha fidelity is unverified. See the
[support/evidence ledger](../../docs/after-effects-support.md#psd-footage-import).

## Optional Adobe expression-sample import

Normal AEP import remains Rust-only and Adobe-free. Import can optionally consume
numeric Transform and ordinary Effect Parade expression samples captured separately
in Adobe and bound to the exact source SHA-256. Adobe capture orchestration is
outside this public workspace; import fits supplied samples into existing editable
scalar keys. The converter does not evaluate expression source, generate `JsScript`, or
emit one key per sample.

```sh
tsrct-conv convert /path/to/source.aep --to tesseract \
  --composition 20432 \
  --expression-samples /path/to/expression-samples.json \
  -o /path/to/output
```

`--expression-samples` is accepted only for AEP → Tesseract import. The selected
composition, source bytes and sidecar identity must match. Version 2 explicitly
binds either the selected root or all compositions and records AE's actual native
expression-clock timestamp for every requested millisecond; legacy version 1 is
accepted only for a single-composition source. Capture is limited to the selected
affine reachable graph and rejects reachable Time Remap edges. The temporary
clock property and all of its scratch items are removed before source expressions
are evaluated. The Adobe capture helper retains its own execution bounds
(60s/property, 250,000 total value-vectors, 4,096 records and 128MiB JSON).
The Rust sidecar importer no longer imposes those JSON/record/sample quotas;
source binding, scope, clock order and finite-value validation remain. Shape/Mask subtrees, Layer Styles and nested effect parameters
are outside this helper's sampled-property surface and are reported separately.
Adobe may show a project-conversion prompt that requires manual acknowledgement.
A helper timeout does not cancel the native AE operation: inspect AE and the
receipt before retrying, and never start a concurrent blind retry.

See the [current import-only evidence and limitation ledger](../../docs/after-effects-support.md#sampled-expressions-fill-and-sparse-shadow--current-import-only-checkpoint)
for the exact source/sidecar/video identities, current 25-record capture, seven
uncaptured Shape/Mask-subtree expressions, Fill approximation, sparse Shadow fix
and missing proof. The original Mixkit source/media/sidecar remain local and
uncommitted. The minimal sampled-Position source and its separately named v2
sidecar are public regression fixtures.

## PDF-compatible Illustrator footage import

A reached local `.ai` still source whose bytes begin with `%PDF-` can be lowered
into existing editable FX `Shape` layers. The first increment intentionally uses
a restricted, strict, single-page whole-artwork profile: classic xref only, no
encryption, no optional-content/Illustrator layer selection, no differing
CropBox, and validated source/object/stream/operator/path/Form structure. It maps
solid DeviceRGB/DeviceGray fills and strokes, compound cubic paths, fill rules,
dash/cap/join/miter controls, graphics-state transforms, page origin/unit/rotation,
and Form XObjects with validated geometric bounds. A consumed editable Shape guide clips paints
at the PDF page boundary; FX siblings are stored topmost-first and static source
content remains available throughout the occurrence's remapped clock.
Unsupported authored clipping, color spaces, alpha,
gradients, text, images and unknown render state produce contextual omissions;
there is no PNG/raw-AI fallback and import emits no `JsScript`.

Eager object/xref-stream expansion is disabled with the parser's zero-byte
loader limit, not a byte-string scan, because that format profile is unsupported.
Content decoding and retained vector caches have no fixed byte allowance.
This is not a process or total-memory sandbox. Existing ordinary image/video/audio
behavior is unchanged. Native AEP source-selector semantics are not established:
multi-page, differing CropBox and optional-content sources are rejected, as are
mismatches between the PDF footprint and AEP source dimensions. Matching dimensions
are **not proof** of a whole-artwork selector: same-sized native layer selections
remain unverified, with an explicit whole-artwork interpretation diagnostic.
The checked-in `.ai` cases are
specification-built PDFs, not Illustrator-native proof. Adobe/Illustrator open,
RGB/alpha comparison and Asset publication remain unrun; FX → AEP/AI restoration
is outside this approved import-only increment.

## Intro corrections and bounded Radial Wipe export

Bounded Path easing, zero-start Rect animation, coupled-ease tolerance, Source-stage
and combined Alpha mattes, and signed sibling Rotation have targeted regressions.
The full current Intro comparison was delivered and visually accepted; formal
full-project scoring remains incomplete. Noise/Grain is excluded.

The 50% hard Radial Wipe importer uses editable half-plane Shape/PathMask geometry.
Export now recognizes a guarded generic version of that geometry and writes a
fresh native Radial Wipe with the edited center and Rotation keys. It does not
replay the original AEP or change FX schema/runtime behavior. An independently
Adobe-authored edited oracle and fresh exported AEP matched all60 RGB frames and
two complete RGBA samples; the controls were independently read in Adobe. The
initial native MP4 references and the additional direct-Angle-key reference are
long-term Assets with fresh download/hash verification. Direct Angle keys also
reimport as editable Rotation keys.
Import edge-alpha equality, arbitrary Wipe modes/feather, and full Intro export
fidelity are not established. See the [direction-specific proof and limitation
ledger](../../docs/after-effects-support.md#radial-wipe-export-follow-up--edited-native-controls-and-independent-proof).

## CLI examples

From `opensource/conv` after `make build` (or use `$CARGO_TARGET_DIR/debug`):

```sh
target/debug/tsrct-conv inspect source.aep
target/debug/tsrct-conv inspect source.aep --json
target/debug/tsrct-conv convert source.aep --to tesseract --composition 1 --output imported
target/debug/tsrct-conv convert imported/project.tsrct --to after-effects --output fresh-native --fps 30
target/debug/tsrct-conv convert imported/project.tsrct --to after-effects --output fresh-native --check
```

Target listing reads bounded native project metadata only and includes nested
compositions. Each target reports its item ID/name, native canvas, frame rate,
duration, and direct (non-recursive) layer count; AE track counts remain unknown.
Listing does not probe media or perform conversion. Import may omit
`--composition` only when the AEP contains exactly one composition. Every
multi-composition AEP requires an explicit item ID, even when only one composition
is an unreferenced root.

The output **parent** must exist and the final directory must not. Use the
native composition item ID, not an invented sequence GUID. FX export defaults
to 24fps unless `--fps` is supplied; import uses source timing. Import produces
`project.tsrct` and export produces `project.aep`. `--check` publishes no final
output (temporary staging may be used and cleaned) and can report omissions.
The typed [`fx_conv` interface](../fx_conv/README.md) returns native diagnostics
and exact planned/published project and media filenames in both modes. Export's
`AfterEffectsExportOptions`, including FPS, is the trait's options type. Conversion by itself does **not** render either project.
See the [CLI reference](../../apps/tesseract-conv/README.md) for argument and
publication details. Native Adobe reference rendering, imported-FX rendering and
FX→AEP export are separate operations with separate proof requirements.

## Generation-time project digest

Both staged handles expose `generated_project_sha256()` from the writer's exact
output, not a later reread of the mutable stage file. Hybrid package capture checks
this digest and retains that same capture. This detects root-project drift; it
is not native-control/Adobe proof or external-media provenance.

## Picture-only staging for hybrid coordinators

`AfterEffects::stage_picture_only_document` returns a
`StagedAfterEffectsPictureExport` with the same owned directory/report/root
accessors as ordinary staging. Final native audio switches are disabled across
emitted compositions, without deleting audio owners or media. This is intentional
suppression for separately verified Premiere audio ownership, not proof that the
complete hybrid output has correct audible behavior. Ordinary export is unchanged.
See the [hybrid evidence ledger](../../docs/hybrid-adobe-export.md) for structural
tests, current scope restrictions and missing independent Adobe proof.
`stage_picture_layers` uses the same writer on a borrowed contiguous root range;
selection and dependency closure belong to the coordinator, not a second schema
or control-assessment system. Staged handles expose `omitted_layer_ids()` for
source layers/subtrees dropped during lowering or dependency pruning. This is a
typed omission fact, not a fidelity certificate: effect approximations are still
reported separately. Coordinators can retain their native content rather than
replace it with a partially populated AEP.

## Owned staging for package coordinators

`AfterEffects::stage_document(archive, document, staging_parent, options)` prepares
an unpublished AEP from an explicitly selected editable document and the archive's
assets. `StagedAfterEffectsExport` owns a fresh temporary directory, exposes its
relative project/media artifact inventory and diagnostics, and removes only its
private tree when dropped. A coordinator must preserve that relative layout,
validate safe scope extraction, and publish its complete package separately.
This API does not partition FX, launch Adobe, or change native-only CLI defaults.
Check and Write use the same lowering/preparation path (including temporary file
writes); only Write publishes final files.

`root_composition()` returns the emitted root's name, numeric ID, canvas and
rounded native rate/duration. Its Dynamic Link GUID is limited to the current
writer's fixed AE26 root profile, directly observed in one nonempty generated
Rect AEP. It is not a general ID-to-GUID formula or a content-acceptance guarantee:
a separate generated Text project failed native opening. See the
[exact probe and limitations](tests/fixtures/hybrid/README.md) and
[hybrid implementation ledger](../../docs/hybrid-adobe-export.md).

## File-bound Dynamic Link import

`AfterEffects::prepare_linked_import(path)` reads one bounded immutable source
snapshot. `resolve_composition(&[u8; 16])` accepts UUID bytes in network order and
returns a selection borrowed from that exact file; names and encounter order are
never selectors. `selection.import_to_tesseract(output, mode)` reuses the parsed
snapshot and the ordinary editable media pipeline, checking source SHA-256 before
conversion and again before publication. Normalized temporary media remains owned
until archive writing finishes. These checks are not a filesystem lock or an
atomic snapshot of all external media.

`selection.import_picture(target, &mut media)` imports the same selection as the
picture of one Premiere placement, for ordinary Premiere conversion: a
`LinkedPicture` whose root Group is a child of `target.parent`, whose generated
layer, item and effect identities (and the keyframe ids built from them) start at
`target.first_id`, and whose footage assets are named in `target.asset_namespace`,
one per resolved AEP. Audio is muted, because Premiere plays a link's sound only
through audio track items, the preview background is not materialized, and root
layers take their composition's motion-blur switch. `LinkedMedia` collects each
asset once, with only the normalized file that backs it, and the SHA-256 of every
local file that the pictures were read from. The host packages it with `add_to`,
keeps it until its archive is written, and owns freshness before publishing:
compare `source_sha256`, call `verify_sources` (re-reads footage and PSD/AI
sources, and requires a relinked source's authored path to stay missing) and,
once written, `verify_packaged` (compares packaged footage digests).

The experimental identity profiles are deliberately restricted to the observed
AE26.5x89 macOS and AE26.3x87 `head` revision/subtype/producer words, whose native
GUID-to-item evidence is public (H-IDENTITY-01) and private respectively, and to
the nonzero item-ID/zero-suffix GUID layout. Unknown profiles, GUID layouts,
absent/non-composition IDs and unsafe structural identities fail explicitly; there
is no name/root fallback. Ordinary numeric-ID import is unchanged and does not
acquire this producer restriction. No Adobe installation is needed at conversion
time.

Footage whose absolute authored path is missing is looked up once at AE's native
alias relative location (the `ascendcount_base` ancestor of the AEP plus the last
`ascendcount_target` path components), as AE relinks a moved project; an existing
absolute file wins and unfit counts are diagnosed. If both locations are missing,
inspection and every import also try the adjacent Adobe Collect Files layout:
`(Footage)/<native project folders>/<authored basename>`. This is an exact path,
not a recursive filename search. Folder-alias image sequences append their native
frame filenames. Conflicting source identities, unsafe folders, and symlinks
escaping the adjacent collection remain unresolved. The fallback is diagnosed;
its bytes are not authenticated against the unavailable original. Found video
undergoes normal admission, so unsupported codecs require explicit preparation
rather than silently becoming omitted missing media. See the
[collected-footage evidence and limits](../../docs/after-effects-support.md#adjacent-collected-footage--inspection-and-import-path-correction).

The [native identity case](tests/fixtures/hybrid/identity/README.md) independently
pins two **same-name** compositions (IDs1 and16), their native Premiere GUID
payloads, editable red/blue import assertions and three hash-verified long-term
native reference Assets. These references establish native identity discrimination,
not fresh-converter RGB/alpha/audio fidelity. Premiere occurrence resolution and
normal CLI linked picture import live in `premiere_file`, through `import_picture`.
`selection.import_editable_picture(first_id, asset_prefix)` is the entry point for a
caller-supplied Premiere importer: the same conversion, media preflight, muting and
best-effort diagnostics as `import_picture`, with asset ids
`<asset_prefix>aep-local-item-<id>` and a root Group without a parent. Retain the
returned value until the final archive is written; the freshness of its assets is
the caller's to check. Linked-audio occurrence import remains **unimplemented**, and
no expression sample sidecar is supplied through these APIs. The standalone AE CLI
route is unchanged. See the [hybrid ledger](../../docs/hybrid-adobe-export.md) for
both-direction limitations and the unmeasured Adobe acceptance/render/audio/edit/
relocation proof.

## Choose the right reference

- [Current support/approximation ledger](../../docs/after-effects-support.md):
  feature × direction, diagnosed fallback and **separate** structural, Adobe
  control, RGB, alpha and audio evidence. In particular, eligible Boolean
  parametric operand keys, bounded Adjustment/Layer Styles and scalar-script
  preparation have different proof boundaries.
- [Format overview](../../docs/formats/after-effects.md) and
  [Effect Parade control appendix](../../docs/formats/after-effects-effects.md).
- [Case execution checkpoint](../../docs/after-effects-test-results.md) and
  [native fixtures](tests/fixtures/README.md).

Independent Adobe opening/control readback and rendered comparisons exist only
for specific pinned cases; they do not establish general bidirectional fidelity.
The original archive stays unchanged on export. Never present an own-reader
roundtrip or an empty-writer probe as independent editable export proof.

The import-only [RGB Invert and repeat-edge blur checkpoint](../../docs/after-effects-support.md#rgb-invert-and-repeat-edge-blur-bounds--import-only-checkpoint)
uses existing Levels and Gaussian Blur fields, with explicit omissions for
unsupported Invert controls. Adjustment blur bounds follow the composition canvas.

The import-only [cached text/caption checkpoint](../../docs/after-effects-support.md#cached-text-baseline-and-bounded-caption-pills--import-only-checkpoint)
consumes valid cached boxed-text baselines once and recognizes one complete
static-control caption reveal program. Its editable Rect width uses diagnosed
fixed source glyph bounds; text/font edits and missing-font substitution do not
recompute width. Separate Fill/Stroke ownership preserves authored color, opacity
and paint blend modes.

The [static CC Toner approximation](../../docs/after-effects-support.md#static-cc-toner-ramps--import-only-approximation)
uses existing grayscale TintTritone plus RGB ColorCurves for native Tritone and
Pentone controls/defaults. Luma and interpolation remain approximations. Partial
Original mixing is bounded to a sole enabled Toner on an ungated normal Adjustment
with static full layer opacity; existing alpha-compositing limitations remain
explicit. This does not add native CC Toner export reconstruction.

The [full-span Adjustment Posterize Time import](../../docs/after-effects-support.md#full-span-adjustment-posterize-time--import-only-checkpoint)
uses unheld interval gates around independent, source-zero static-rate visual
Groups. It preserves below-stack source clocks and editable animation; above
siblings stay live. Hold-only rates and closed visual ownership are required.
Partial spans, crossing references, physical time-based media in the held scope,
expressions and non-Hold schedules retain the original unsupported adjustment.

The [bounded self-inverse mask-stage import](../../docs/after-effects-support.md#bounded-self-inverse-mask-stage--import-approximation) uses two editable helper Groups
to preserve mask, shadow and inverse-mask order on a validated opaque solid.
General Set Matte selectors and omitted Linear Wipes remain unsupported.

A separate bounded foreign-mask profile imports an exact one-hop sibling mask
path as an independent editable copy, or proves equal independently authored
mask geometry and timing/easing controls. It stages the inverse matte with shadows,
Glow and a validated legacy centered Transform; optional opposed Wipes remain
diagnosed omissions. Fixed-direction Drop Shadow distance keys become editable
Vector2 offset keys. See the support ledger for
clock/default guards and remaining native raster/appearance limitations.

The recognized caption pill profile also lowers its complete Solid Composite
Source Opacity formula once to an editable linear opacity ramp on the isolated
fill/stroke paint Group. Fade-in frame counts use the native composition rate
and source-local inPoint; zero fade-out and transparent Normal background are
required. Existing width/anchor tracks and separate paint opacity remain intact.
Sparse controls use the source-proven canonical profile; supplied conflicting
local declarations or explicit controls retain diagnosed best-effort geometry.
Source-only matte samples retain geometry without the occurrence-effect fade;
ordinary and All Effects samples include it. Existing-source regressions and
Premiere movie diagnostics support this mapping; independent feature fidelity
remains unverified.

Import also keeps modern unselected mattes, still-image lifetimes, the
`Fade In+Out - frames` preset as owner Opacity keys (one fade owner per
occurrence) and Geometry2 on a still as an editable source-plane Group. See the
[ledger section](../../docs/after-effects-support.md#mattes-still-lifetimes-frame-fades-and-still-geometry2--bounded-import-correction)
for their profiles, omissions and synthetic coverage; their export is unverified.

Uniform finite positive full-run point-text glyph scales can use an editable
inner Text transform, retaining the outer occurrence transform and clock.
Box/path/mixed-run text and affected animator geometry keep the omission
warning. Geometric scaling also scales tracking and raster stroke, so exact
native per-glyph behavior remains unverified. This does not establish native
font substitution or title fidelity.


Static Both-dimensions Fast Box Blur uses a diagnosed, source-derived Gaussian
variance approximation. Native RGB color speeds use shared vector progress
for unchanged-alpha Bezier keys. Uniform point-text scale now retains bounded
vertical-only Position3D and scalar Stroke Width/Opacity animators. The guarded
inverse-matte post-effect half turn also rotates screen-space shadow offsets
and their editable tracks. See the support ledger for exact native regressions,
control/default guards and remaining kernel/alpha limitations.

Bounded Fractal Noise import uses the existing editable TurbulentNoise effect
for static uniform Basic/Spline Normal stages on proven opaque solid planes.
A separate zero-contrast Multiply stage retains an opacity-weighted midgray
attenuation through editable Exposure. A static equal-axis Basic/Spline
Multiply/Screen generator can use an independent finite source plane and
source-over blend Group. Native controls and popup defaults drive these diagnosed
approximations; unsupported anisotropic, dynamic and spatial stages keep omission
diagnostics. Independent blend planes preserve effect order and visibility.
Disabling only their generator effect retains opaque carrier paint, so bypass
the generator Group to preserve transparent prefix alpha. See the
[Fractal Noise ledger](../../docs/after-effects-support.md#bounded-fractal-noise--import-approximations)
for kernel, scale, evolution, HDR and native contrast-formula limits.

Transparent self-input Vegas Intensity contours on structurally grayscale
content use a diagnosed editable full-outline approximation. Original source
IDs and keyframes feed shared Alpha-matted proxies; native contour width and
stroke color remain controls. Segmented sweep, hardness, tolerance, opacity
gradients and native edge alpha are not reproduced. White alpha-output gates
retain both native text paints through separate fill-only/stroke-only Text
children, with independently remapped animator and item IDs.

A static uniform, unrotated Premiere linked placement compensates raw
DropShadow pixel lengths and signed SimpleChoker radii, including their
editable tracks. GaussianBlur already inherits world scale and is untouched.
Dynamic/nonuniform placement and nonidentity internal scale retain explicit
diagnostics. Straight spatial keys with one shared positive speed and zero
tangents use vector-distance progress, preserving mirrored Position3D timing.

Hard-edge Linear Wipes on finite planar raster Solids become editable projected
canvas half-plane masks, with native Completion tracks and a bounded optional
post-wipe normalized Geometry2 Anchor translation. Same-layer direct control
aliases are copied into independent editable values or keys. Feather, mixed
effect stacks and nondefault Transform controls remain unsupported. Angled
completion normalization and edge rasterization are analytical approximations,
with no native pixel-equivalence claim.

The black-composite / Lightness-alpha approximation requires grayscale paint or
a full-strength grayscale Tint with equal channel tracks. Arbitrary media and
colored paint are declined unless a later full grayscale Tint establishes the
needed grayscale input. Ordinary text paint remains an explicit approximation
for intrinsic color-font rasterization and native Lightness/unmatting edges.

Existing FX Glow still approximates native Add / Behind with its current
premultiplied-over halo composition. Native threshold extraction, alpha and
blur-kernel behavior are not established. No new Glow implementation or full
neon-title parity is claimed. Mixed shadow/matte
Wipe stacks remain outside the isolated Solid Wipe profile above.
