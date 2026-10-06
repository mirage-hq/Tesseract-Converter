# After Effects native file adapter

## Guide-free export preparation

Documents without cross-layer mask guides or Text Path references skip the
speculative full native-lowering pass used to collect consumed guide IDs.
Inline masks still receive ordinary lowering and validation; any guide reference,
including a hidden or unresolved one, retains the existing probe. Script sampling,
fresh-runtime validation, generated output and diagnostics are unchanged.

## Oversized source viewport export

An otherwise valid Group whose all-time geometry overflows the native
composition dimensions is no longer omitted. On that precise overflow only, every
AEP export retries with a finite output-derived working plane using existing
planar inverse demand and native source-origin/camera handling. Editable native children,
effects, clocks and animation are retained; an already checked native source
clock also remains the retry's demand time domain. Rescued Group Radial Blur
centers/base and Point keys retain the logical FX composition point domain under
source-origin translation, rather than being retargeted to the cropped canvas.
Offscreen blur/motion-blur boundary contributions may differ. Known reach is
preserved; unproved approved blur reach is not guessed. Unsafe geometry, clocks,
ownership and unknown bounds remain rejected. Representable/collapsed export and
import are unchanged. There is no option or CLI flag. See the
[approximation and proof ledger](../../docs/after-effects-support.md#oversized-native-source-working-plane-approximation).
Native open/readback and RGB/alpha fidelity are unverified.

## Saved graphic Text-controller decoding

`graphic_template::SavedGraphicTemplate` binds a declared Essential Graphics
UUID to its explicit native composition/layer and by-name Source Text path.
Static single-style text snapshots reuse the ordinary AEP Source Text decoder;
current saved text/font/size/All Caps values can be applied independently without
changing the source or siblings. Missing/duplicate bindings and unsupported paths,
keyed/mixed-style documents or invalid values reject instead of choosing by name.
`editable_layers` feeds the existing Premiere Source Graphic seam in native
(top-to-bottom) layer order, using ordinary Text/Shape decoders and static native
parenting. A bounded Rectangle layout recipe estimates current glyph width from
source-average cached extent, with explicit missing-Slider and fidelity warnings.
It does not restore responsive controllers, stage an AEP or emit scripts. Ordinary
AEP import/export and the merged numeric-expression evaluator are unchanged.
See the [Premiere capsule limits](../premiere_file/README.md#saved-ae-capsule-graphics).

## Alpha stack transfer import

Full-composition ordinary Stencil Alpha / Silhouette Alpha sources now consume
source paint and mask the accumulated lower siblings using existing editable
Alpha / AlphaInverted TrackMattes. Identity wrappers retain source IDs, animation
targets, clocks and upper siblings. This is a stack operation, not a new FX blend
mode or a source-colored overlay. Partial-span sources, Adjustment sources,
Preserve Underlying Transparency dependencies and restructuring/depth conflicts
retain contextual fallback diagnostics. Non-rendering Null/helper kinds and
sources already diagnosed as placeholders are skipped with a contextual reason,
leaving the lower stack unchanged. Raster eligibility does not depend on opacity
or nonempty children; valid transparent/empty raster precomps remain supported.
Luma stencil/silhouette remain unsupported;
current inverted-luma masking is not a full luminance complement.

Licensed cases01–03 exercise the real CLI publication route through explicit
`AEP_ALPHA_STACK_CASE{01,02,03}_SOURCE` environment variables and exact hashes.
They are opt-in CPU structural regressions, not Adobe/alpha/RGB fidelity proof.
FX → AEP behavior is unchanged. See the support ledger's alpha-stack checkpoint.

## Archive-backed native font identity

FX → AEP staging now carries an ephemeral embedded-font context into Source Text
lowering. A unique physical family/style (or exact empty-style PostScript request)
is matched through archive face metadata; hash-verified bytes and face index must
confirm the PostScript name. Actual `glyf` versus `CFF ` tables select native `/2`
values 1 versus 0, established on independent AE26.5 native controls with
non-substitute font-object type/location readback and verified font bytes.
Missing, ambiguous, mismatched, variable or unknown/CFF2 faces retain the previous
candidate with a contextual diagnostic and no guessed format. No system font
lookup/install, renderer/schema change, vendor-version guess or substitution is
performed. Correct identity does not establish host font availability/fidelity.
Import is unchanged. See the [proof/limitation ledger](../../docs/after-effects-support.md#archive-backed-source-text-font-identity-and-outline-format).

## Embedded font package delivery

FX → AEP packages include hash-verified embedded physical font files used by the
final exported Text documents (including Hold font changes and nested compositions)
in `fonts/`, byte-for-byte, with `fonts/manifest.json` listing file, family, style,
PostScript name and SHA-256. Unused embedded faces are not copied. Collection files
remain intact. The conversion diagnostic requires installing these fonts before
opening/rendering in After Effects: this transport folder does not activate fonts.
Redistribution remains subject to the font licenses. Import is unchanged; packaging
does not establish font availability or pixel fidelity.

## Emitted Point Text bounds

The bounds-only physical-font projection consumes the same Source Text documents
as native lowering. Continuous Tracking/FillColor authored-time values survive as
editable Hold documents and use the emitted outline union. Between-key interpolation
remains approximated. Actual-original and native fidelity for this mapping remain unproved.
Unpainted whitespace contributes no enclosure but stays native editable Text.
Three actual-case camera/Text profiles and a Tracking-edited minimal pair have
bounded native acceptance and edit response. Candidate RGB scores, full-timeline
clipping, general shaping/font-substitution, alpha/audio and formal Asset proof
remain incomplete. CustomShader stays unsupported. A static zero-spread Group
Drop Shadow can now use the font projection, enclosing its offset shadow by AE
Size (`2 * FX blurRadius`) before the owner transform, including native 3D owners.
Other effect/spatial and native clock/near-plane guards remain unchanged. P037
Group810 is retained. Native Drop Shadow numeric descriptors now match the
independent oracle; Size/distance edits survive native save/reopen with all 60
editable children retained. This is not a whole-case visual pass. Import is unchanged. See the
[checkpoint](../../docs/after-effects-support.md#emitted-point-text-bounds-and-unpainted-whitespace--export-checkpoint).

Rust-owned bounded RIFX/AEP reader, best-effort **AEP → editable FX** import, and
experimental **edited FX → new AEP** export. The exporter writes current FX
content (including layer-local scalar/ShapePath/Source-Text-JS-to-editable-keys preparation
with redundant-sample reduction and fresh playback validation); it does **not** patch or replay the source AEP. Import never generates
`JsScript`. Native Shape/Mask Path keys have a bounded editable mapping in both
 directions; see [the Path-key evidence and limitations](../../docs/after-effects-support.md#native-shape-and-mask-path-keys).
A bounded experimental Box Source Text writer uses independent native default
records while replacing typed content/style/geometry/holds; it preserves native
COS scalar token types and class-header order. **Bounded static and two-Hold
cases with explicit ArialMT passed Adobe open/render and cleanup/READY. Full
project acceptance remains blocked:** a freshly generated original-project AEP
still produced a Text-reading error and failed cleanup verification under the
previous lifecycle. Current recovery verifies owned cleanup and fresh READY;
there is no manual-unlock latch or replay of the failed input. The proposed
Arial-to-ArialMT alias was withdrawn: no actual FX fallback/custom-font resolution
or requested font binary had been verified. This follow-up adds no font-name
aliases. Archive-free candidate-name lowering remains diagnosed;
its output is not proof of the user's actual resolved typeface. The template's
Myriad-Roman/Helvetica/AdobeInvisFont defaults have not been validated against FX
fallback/custom fonts. Box Text now omits inherited glyph/line caches and unknown
font vendor metadata, using native nonempty automatic-leading storage (0.01).
P047's 17 Box and 4 Point original text semantics have fresh native acceptance
and exact text/font/size readback; full-project/render proof remains separate. Default-options Point Text
now uses a separate cache-free native semantic profile, with generic font-changing
Hold indices and automatic-leading native defaults. Matched static Arial64/48
native renders and an unchanged P025 Text probe pass. Unknown per-font vendor
version is omitted rather than copied from Arial; fresh native Arial48 still
exactly matches its control. Actual VT323 resolution is unproved: an independently
authored reference fails missing-font admission despite generated strict receipts
reporting none. Native Text-control/fontObject readback, immutable references and
general/richer-owner fidelity remain incomplete.
Nondefault anchor/path/animators now retain the complete native Source Text envelope and editable sibling groups. Native numeric-control descriptor/bound envelopes and value-before-bounds ordering preserve authored values/keys; fresh P013 animator-only export has bounded native acceptance/cleanup/READY. Full-project render/SBS, exact selector/key readback and general fidelity remain separately measured proof requirements. See the [profile limitations](src/writer/native_text/README.md) and
[partial evidence ledger](../../docs/after-effects-support.md#deep-blue-v13-destination-media-and-typed-text-export-repair--partial).
AEP→FX implementation/proof is unchanged by this export-only follow-up.

Missing features and
approximations carry contextual diagnostics, but success or `--check` does not
mean that all pixels, alpha, audio or native controls were preserved. Run only
trusted scripts with external resource limits; VM bounds are not isolation.

FX script export uses a single owner-local 4× output-FPS sampling grid, rounded
to integer milliseconds and including both endpoints, for evaluation and fresh
validation. This replaces the former 1ms policy; no opt-in flag or legacy mode
remains. Scalar fitting uses Linear/Hold keys, text changes become editable Hold Source
Text keys, and Path fitting keeps the existing 0.01 geometry-space tolerance on
the sampled grid. Unsampled pulses,
topology/state changes and motion-blur fidelity are not guaranteed; a warning
identifies this approximation. This also applies to linked AEP scopes in Premiere
export, not native Premiere script preparation.
Independent tracks fit in bounded two-thread batches with separate fit/fresh VMs.
Key IDs (including failed Text reservations), diagnostics, progress and working-copy
updates are committed in source order; all workers finish before document validation.
Console writes also retain source order and synchronous I/O errors, so console-heavy
tracks can serialize. This adds no VM deadline or resource isolation: nonterminating
scripts still require external supervision, and wall-clock/random script behavior is
not made deterministic by parallel fitting. Sampling and fidelity limits are unchanged.
See the [performance evidence and limits](../../docs/after-effects-support.md#frame-rate-sampled-fx-script-baking--export-performance).

## Video admission and explicit media preparation

Used unsupported video fails Check/Write rather than becoming an omitted layer.
`AfterEffects::inspect_media` uses native target reachability and relocation;
`import_with_media_map` applies source-bound replacements before the same admission
checks. Container/codec admission uses read-only libavformat inspection by default;
it never decodes or transcodes media. The separate
[preparation workflow](../../docs/media-preparation.md) owns explicit transcoding.
Building with `default-features = false` avoids native FFmpeg linkage but makes
video admission return an explicit unavailable-backend error. Independent Adobe/render,
alpha/color/audio proof for this import workflow remains unmeasured.

Hybrid Premiere export can separately prepare unsupported selected video sources,
retain exact source durations, write held source clocks as enabled Time Remap keys,
and bound short affine Group geometry over visited time. Ordinary admission and
native Premiere fallback remain intact. See the [linked-picture implementation and limits](../../docs/after-effects-support.md#linked-picture-destination-media-and-source-clocks).

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

This was a **PSD import-only** increment. PDF-compatible AI and PSD → fresh AEP
export remain deferred. A later bounded JPEG/PNG → EXR preparation on FX → AEP
export is documented in the [support ledger](../../docs/after-effects-support.md#deep-blue-v13-destination-media-and-typed-text-export-repair--partial);
it does not reconstruct Photoshop internals or prove Adobe image/alpha fidelity. Native author/readback and local Adobe
30fps renders exist for two pinned cases; Asset publication and fresh FX-render
comparison are incomplete, and alpha fidelity is unverified. See the
[support/evidence ledger](../../docs/after-effects-support.md#psd-footage-import).

## Explicit font availability on AEP import

`AfterEffectsImportOptions.available_fonts` optionally supplies an authoritative
set of available **PostScript face names**. CLI callers use
`--available-fonts /path/to/fonts.json`, where the file is a JSON array, for example
`["Inter-Regular", "Inter-BoldItalic", "AuthorFace-Regular"]`.

A font absent from that explicit inventory is replaced in the editable document
by a same-style Inter PostScript face when listed, otherwise `Inter-Regular`.
The original authored name and replacement are retained in an `AE-PROPERTIES`
warning; glyph metrics, shaping and layout can change. This applies to static
Text and Source Text hold values, including Basic Text's authoritative face.
No inventory means **no new font rewriting or fallback warnings**. An explicit
empty array means no source face is available. Found faces remain unchanged.

This is a declaration of availability, not a filesystem/system-font probe or
font download. It does not package binaries: stage/import the selected font
bytes separately before rendering. If even Inter-Regular is absent from the
inventory, the warning explicitly reports that additional rendering requirement.
There are no font-name-specific mappings or new aliases, and no FX schema or
renderer changes. FX → AEP behavior is unchanged; a substituted document names
Inter, not the original native face. Native glyph/render fidelity is unmeasured.

```sh
tsrct-conv convert source.aep --to tesseract --composition 1 \
  --available-fonts fonts.json --output imported
```

## Optional Adobe expression-sample import

Normal AEP import remains Rust-only and Adobe-free. Import can optionally consume
numeric Transform and ordinary Effect Parade expression samples captured separately
in Adobe and bound to the exact source SHA-256. Adobe capture orchestration is
outside this public workspace; import fits supplied samples into existing editable
scalar keys. The converter also evaluates a closed, deterministic numeric subset
for direct layer Transform/ordinary Effect targets after occurrence-local Essential
overrides. Converter observations use the composition FPS and the same fitting path;
they are not Adobe evidence. It never generates `JsScript` or emits one key per
sample. See [admission, validation and pending native proof](../../docs/ae-expression-evaluation.md).
Unsupported source retains contextual diagnostics and the existing fallback.

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
Legacy v1/v2 Rust sidecar readers retain their prior quota-free behavior;
source binding, scope, clock order and finite-value validation remain. Explicit
version 3 adds numeric Shape measurement identities with a root-anchored ordered
`path` of `{index, match_name}` segments, including native Contents and leaf.
One-based native indices qualify repeated groups. Paths require 2..64 segments
and 1..1024 UTF-8-byte match names. Shape records are rejected in v1/v2; v3 retains
v2 scopes/clocks and enforces the fixed capture quotas. Ordinary-only captures
remain v2. This parser support does **not** add Shape-to-FX lowering or feature
export; these measurements require an explicit destination consumer. Path
geometry/masks, Layer Styles and nested effect parameters remain excluded.
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
into existing editable FX `Shape` layers. Foreign absolute Windows image/video/audio
paths may resolve from exact native relative alias counts; foreign AI images also
use the exact adjacent collected-footage hierarchy. Neither route guesses paths
or relaxes artwork admission. Gateway's pinned AI artwork resolves but remains
unsupported because of optional visibility and the restricted paint profile;
path resolution alone does not restore its panels. The first increment intentionally uses
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

Bounded Time Remap export accepts keys aligned with the visible endpoints and
authors them using the selected composition property clock. Inexact millisecond
times round to the nearest native tick with a diagnostic; an inward-rounded final
endpoint receives a same-value Hold guard outside the visible interval. Source
bounds, active in/out points and media admission remain unchanged. Native export
control/render proof for this timing repair is unrun; see the
[timing repair and limitations](../../docs/after-effects-support.md#native-time-remap-endpoint-and-selected-clock-export-repair).

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
writes). Only ordinary Write binds generated file aliases to canonical final
package media paths before hashing/publication, retaining native relative
relocation hints; Check and public staging keep portable relative aliases.
The borrowed progress observer reports `prepare AEP media` in deduplicated asset
units on ordinary and selected-picture staging, starting at 0/N and advancing
after each prepared, diagnostically omitted or scope-withheld asset. Failed assets are not counted. These counts do not imply conversion success
or fidelity. The opt-in `stage_document_with_control`,
`stage_picture_only_document_with_control` and `stage_picture_layers_with_control`
accept `AepPreparationControl { progress, cancelled: Some(&token) }`, borrowing a
caller-owned `AtomicBool`. Set it to true and leave it set until the call returns.
Cancellation returns `AepConversionError::Cancelled`, never an omission warning or
successful fallback, and drops the private stage/cache while preserving foreign
paths. Checks run before preparation, between assets, before reporting each asset,
and around native writing; MP3 and selected-video transcoding receive the same token.
Synchronous script preparation, image decoding and native writing are not interrupted
mid-operation. Existing entry points remain uncancelled wrappers with unchanged
progress/output behavior. Within-asset progress and CLI/hybrid cancellation
threading remain separate integration work.
File-footage Anchor XY is stored source-relative, as for Solids, after source
geometry/clock rebasing; native static repair evidence and keyed-anchor proof
limitations are recorded in the [support ledger](../../docs/after-effects-support.md#fresh-write-export-repair-and-remaining-limitations).

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
AE26.5x89 macOS, AE26.3x87 and Co-Editor format96/subtype6 `head`
revision/subtype/producer pairs, and to the nonzero item-ID/zero-suffix GUID layout.
The first two have public (H-IDENTITY-01) and private native identity evidence
respectively. The third is exactly `[0,96,0,6]` / `0x0f8a0656`:
[reduced native-derived evidence](tests/fixtures/hybrid/format96/provenance.json)
pins five Premiere GUIDs to the source AEP's item IDs/names. The reduced fixture
also keeps opening video53/layer54 with its native video metadata and a missing
path. `native_format96_links_keep_all_five_editable_pictures_with_missing_footage`
exercises public Premiere Check/Write and the host admission gate: editable
title/solid siblings survive genuinely missing linked footage with contextual
diagnostics; no missing-media slate is synthesized. Invalid present linked video
remains fatal. This is offline structural evidence, not render fidelity. Unknown profiles, GUID layouts,
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

Hard-edge Linear Wipes on finite planar raster Solids, dimension-matched
composition sources, or continuously rasterized source-free Shape layers become
editable projected finite-plane half-plane masks. A static, unskewed, invertible
2D Shape owner with a complete static finite planar parent chain is inverse-mapped
so the mask boundary remains on the composition plane after its emitted owner and
parent Transforms; its vector paint stays under the original owner/style/opacity
stages. Generated-camera composition normalization offsets apply only to
unparented owners. Native Completion tracks and exact direct
aliases are preferred; existing valid source-frame expression samples become
independent editable keys. Finite native keys and easing are retained without a
local key-count cap or easing-handle clamp; authored Completion key values must
remain within 0–100, and the finite guide extent covers the native easing convex
hull rather than imposing an easing-value cap. Effect declaration kinds, not
the leaf's Integer storage flag alone, distinguish continuous sliders/fixed/angle/
color/point controls from discrete checkbox/popups. Integer-encoded continuous
controls can therefore feed the existing evaluator as either the expression target
or a referenced dependency; unknown integer controls retain the conservative Hold
requirement. A bounded trailing Geometry2 Anchor
translation is consumed after a source-local mask; it remains unsupported for an
inverse-mapped Shape composition plane. A leading Geometry2 does not block the independent Wipe,
but its current footage-layer mapping remains separately omitted and diagnosed.
Feather, nonadjacent mixed stacks, nondefault trailing Transform controls and
Shape planes with dynamic or auto-oriented owner/parent geometry, incomplete
parent chains, skewed owners or singular combined planes remain unsupported.
Composition frame clipping, angled normalization and edge rasterization are
analytical approximations, with no native pixel-equivalence claim.

The black-composite / Lightness-alpha approximation requires grayscale paint or
a full-strength grayscale Tint with equal channel tracks. Arbitrary media and
colored paint are declined unless a later full grayscale Tint establishes the
needed grayscale input. Ordinary text paint remains an explicit approximation
for intrinsic color-font rasterization and native Lightness/unmatting edges.

Existing FX Glow still approximates native Add / Behind with its current
premultiplied-over halo composition. Native threshold extraction, alpha and
blur-kernel behavior are not established. No new Glow implementation or full
neon-title parity is claimed. Mixed shadow/matte
Wipe stacks remain outside the isolated finite-source Wipe profile above.

File-footage import now normalizes explicit Anchor XY by native source dimensions,
using the same path as Solid/Composition anchors, while omitted anchors retain
the existing pixel-center default. Export is unchanged. See the
[import-only evidence and limits](../../docs/after-effects-support.md#file-footage-anchor-source-units--import-correction).

### Bounded Set Matte Red graph

A pinned static premultiplied Red Set Matte source now imports as an editable
provider copy over black, public own/off ShiftChannels plus HSV desaturation,
and a Luma gate. Native bypass preserves the original picture; current graph
edits export through native effects and track mattes, not Set Matte replay.
Native exported grayscale/alpha transfer differs and is unmeasured. See the
[bidirectional evidence and limitations](../../docs/after-effects-support.md#set-matte-premultiplied-red--bounded-editable-graph).
