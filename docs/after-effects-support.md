# After Effects conversion support

This is the current support reference for AEP ↔ editable FX conversion.
Import is best effort; fresh native AEP export is experimental. No direction is
lossless or generally render-equivalent. Use the [CLI guide](../apps/tesseract-conv/README.md)
for commands and [effect controls](formats/after-effects-effects.md) for plugin mappings.

## Selection and output

Import selects one composition by exact native item ID, including its reachable
precompositions and assets. Use `inspect --metadata-only` to list IDs; omission
is accepted only when exactly one composition exists. Names are not selectors.

Import writes `project.tsrct`. Export writes a new `project.aep` from current FX
edits, plus external assets and used embedded fonts when needed. It does not patch
or replay the original AEP. Export FPS defaults to 24; an explicit positive rate
up to 240 is accepted. Keep the whole output package together.

`--check` performs conversion/validation without publishing the final package.
Warnings can accompany success. Malformed input, unsafe graphs, invalid media,
integrity failures and I/O errors can stop conversion rather than produce output.

## Feature summary

**Partial** means only bounded controls, clocks or combinations are representable.
Static support does not imply animation support, and import support does not imply
export support. The limits below apply even when related controls convert.

| Area | AEP → FX | FX → AEP |
| --- | --- | --- |
| Composition and layers | Selected reachable graph, ordered editable layers, canvas and duration. | Fresh native root, IDs, supported layers and precompositions. |
| Timing and hierarchy | Bounded starts, stretches, time remaps, parent/matte chains and independent precomp occurrences. | Finite compatible clocks, supported parenting and source variants. |
| Transforms | Numeric position, anchor, scale, rotation, opacity and compatible keys; bounded 2.5D/camera profiles. | Supported numeric keys, selected native 3D sidecars/camera and shutter/frame-blend settings. |
| Shapes | Solids, paths, parametric shapes, ordered paints, bounded groups and Boolean operations. | Supported native shapes, paint stacks, geometry keys and bounded Merge Paths. |
| Text | Point/box documents, selected animators and compatible field tracks. | Fresh editable text and selected native animator controls. |
| Masks and mattes | Supported mask guides, Path tracks and matte relationships. | Supported static/keyed guides and bounded native matte relations. |
| Adjustment layers | Editable stack scope/order, mapped effects and supported gates. | Native solid-backed adjustments with supported effects, clocks and coverage. |
| Effects | Mapped plugin controls and compatible numeric tracks. | Fresh plugin records from current FX values and supported keys. |
| Layer styles | Nine representable FX style forms. | Supported native style identities and compatible scalar keys. |
| Media | Admitted local footage, supported replacements and bounded still/vector decoding. | Packaged admitted source bytes or explicitly prepared assets. |
| Audio | AV/audio separation, mute and bounded gain/clock mappings. | Supported WAVE/QuickTime audio, trim, gain and finite source switches. |
| Expressions/scripts | Closed numeric expression profiles or validated native sample sidecars. | Admitted FX scripts prepared into editable keys; no native expression restoration. |

## Clocks and transforms

Native time projects into integer milliseconds; fractional boundaries, reverse
Hold segments and unsupported easing can differ. Repeated native source instances
become independent editable copies, not shared editing identities. Cycles,
unbounded expansion and invalid references are stopped or diagnosed.

Groups do not preserve every fixed native canvas/clipping rule. Source origins,
parent transforms, projection, blur expansion and nested effects can alter bounds.
An admitted finite working plane is not proof that offscreen contributions match.
General user camera/light/material systems and arbitrary coupled spatial easing
are outside established support.

## Shapes and Path keys

<a id="native-shape-and-mask-path-keys"></a>

Native Shape and Mask Path keys retain compatible times, contours and supported
Linear/Hold/Bezier controls. Non-Hold geometry needs compatible topology;
unsupported interpolation, per-point controls and clock combinations diagnose an
omission or static fallback. Compound paths use separate native contour tracks;
changing contour counts require supported Hold/disappearance profiles, not invented
particle correspondence. Shared paint editing can be lost by a diagnosed split.

Mask export needs a representable guide/owner relationship, clocks and relative
transforms. Copying a guide does not preserve live shared editing. Unsupported
coverage must not be treated as a harmless decorative loss.

Boolean, stroke, gradient and parametric-shape controls have narrower combination
limits than plain paths. Variable-width strokes, animated gradient geometry and
unproved modifier combinations can be omitted. Path control agreement does not
establish raster agreement: selected Shape comparisons still fail strict coverage
gates. See the [Path fixtures](../crates/aftereffects_file/tests/fixtures/path-animation/README.md),
[Path proof record](../crates/aftereffects_file/tests/fixtures/path-keys-proof/README.md)
and [animated seam profile](after-effects-animated-seams.md).

## Text, masks and compositing

<a id="adjustment-layer-bidirectional-checkpoint"></a>
<a id="box-text-vertical-alignment--export-only-omission-correction"></a>
<a id="deep-blue-v13-destination-media-and-typed-text-export-repair--partial"></a>
<a id="layer-styles--current-implementation-first-checkpoint"></a>

Fonts, shaping, baselines, box layout, selectors and text-on-path remain bounded.
`--available-fonts` explicitly declares PostScript availability for import; missing
faces use diagnosed Inter substitutions. It does not obtain or install fonts.
Export can use verified embedded physical fonts for identity and bounds, and
packages used files under `fonts/` with a manifest. Install licensed faces before
opening/rendering in AE; packaging alone does not activate them.

Supported Stencil/Silhouette Alpha import can mask the accumulated lower stack.
Partial spans, preserve-transparency dependencies, adjustment sources, luma modes
and unsafe restructuring have narrower admission. Blend math, feather, wet/dry
adjustment mixing and nested alpha are not generally AE-equivalent.

Layer Styles are a separate native phase, not Effect Parade. Shadow/glow,
Bevel/Emboss, Satin, overlays and Stroke have selected mappings; Pattern Overlay,
noise/contour/gradient geometry, ordering and non-scalar animation can be lost.
See the [style reference](after-effects-evidence/layer-styles.md) for exact controls.

## Effects

<a id="huesaturation-master-values--partial-export-repair"></a>

Mapped effects use fresh controls from current edits. Static integer Hue/Saturation
Master export has a bounded mapping; unsupported animation keeps diagnosed bases.
CC Vignette import is omitted, while export has only a static fallback. Unsupported
CustomShader output omits its rendering owner rather than showing an unshaded fill.
See the [effect reference](formats/after-effects-effects.md) for controls and units.

## Media and audio

<a id="audio-layer-and-property-animation-completion-checkpoint"></a>
<a id="psd-footage-import"></a>

Inspect the selected target's reached audio/video before import. A successful
inspection can report blocked media; `media_admission`, not video readiness alone,
covers sound. It does not assess still images or rendering. Import never silently
transcodes. Use explicit whole-source preparation and source-bound maps as described
in [media preparation](media-preparation.md).

Supported raster, PSD and OpenEXR profiles are bounded by decoder, structure,
color and allocation checks. PDF-compatible Illustrator import is a narrow
vector subset lowered to editable shapes: clipping, text, images, non-RGB paints
and other PDF/AI forms are not generally preserved. There is no general AI/PDF
renderer fallback.

Audio gain can collapse stereo differences; continuous gain/dB curves and unsupported
clocks are approximate. Selected audio regressions and native references do not
establish general audible fidelity. Linked AEP import is picture-only; keep its
separately represented native Premiere sound independent.

## Expressions and FX script preparation

Import supports a closed expression profile, not the full AE API. It evaluates
supported occurrence-local numeric controls after Essential overrides; unsupported
APIs, ambiguous references, dependency cycles and unsupported destinations retain
contextual fallback. Native sample JSON must bind the exact source, target and
clock. Captured values and fresh evaluation are different evidence.
See [numeric expression import](ae-expression-evaluation.md).

AEP export prepares admitted layer-local scalar, ShapePath and text-content FX
scripts on a **4× output-FPS** grid, rounded to integer milliseconds with owner
endpoints. Scalar/Path reduction creates editable numeric/geometry keys; observed
text changes become Hold Source Text keys. Subframe pulses, intersample motion,
topology changes and stateful behavior can be missed or rejected. Neither this
sampling nor fresh-runtime probes prove arbitrary program purity.

No generated JS, original native expression restoration or rasterized substitute
is implied. VM stack/recursion limits do not stop nonterminating scripts or provide wall-clock
or heap isolation; use trusted programs and external process/resource limits.

## HDR conversion

`--hdr pq|hlg` explicitly selects experimental BT.2100 export. Omission keeps
legacy SDR. Owned ICC profiles retain their established filenames/descriptions;
[profile policy](../profiles/README.md) describes standards, calibration and limits.

Admitted PQ/HLG point colors can map to extended-sRGB FX values with graphics white
normalized to 609 nits by default. This is a point-color bridge, not full footage,
gradient, effect or interpolation-domain color management. Unknown/linearized
profiles and unsupported points retain contextual approximations; gamut reduction
is a diagnosed color loss. Native working/output settings or neutral calibration
do not establish chromatic, alpha or high-precision render fidelity. Historical
saturated comparisons include failures and remain failures.

### ACES native conversion boundary

FX can persist validated ACES policy and render-domain fields, but this workspace
does not render ACES or map that policy to native Adobe controls. ACES export
rejects explicitly. See the [canonical FX color policy](../crates/fx_schema/src/color_management.rs).
Persistence and successful save/round-trip tests do not establish ACES conversion.

## Linked Premiere output

<a id="linked-picture-destination-media-and-source-clocks"></a>

The CLI can stage selected AEP picture scopes inside a Premiere package and import
supported links back into editable FX. Exact linked rates are 24/25/30/50/60 fps.
Dependency, source-canvas, coverage and clock restrictions still apply; an AEP
fallback does not make omitted FX shaders/effects supported. See
[hybrid conversion](hybrid-adobe-export.md) for routing, loss comparison and media handling.

## Evidence and known failures

Keep these checks separate:

1. Software structure: parser, editable values, fresh native records and internal round trips.
2. Native acceptance/control readback: Adobe opens the exact export and reports its controls.
3. Independent render comparison: the generated/imported result matches a separate native reference.

None substitutes for another. RGB comparisons do not establish alpha or audio.
The [evidence guide](after-effects-test-results.md) links exact historical receipts.
Selected cases have native readback/render evidence, but broad support is unproved.
Known limitations include Shape raster differences, effect-kernel differences,
unsupported Vignette animation, and historical generated-text/temporal-effect
exports rejected by Adobe. Old measured failures are not current blanket passes.

Run `make test-aftereffects-file` for ordinary software tests and
`make check-aep-support-ledger` for diagnostic coverage. Native-editor execution,
rendering and scoring are separate authorized operations; these docs do not claim
they ran on the current checkout. Fixture hashes, native sources, references and
measurement receipts are not modified by documentation cleanup.

## Diagnostic code inventory

Codes are defined in [diagnostic.rs](../crates/aftereffects_file/src/diagnostic.rs).
Read the contextual composition/layer message for the affected content. A code
inventory is not an exhaustive loss detector or a promise of native fidelity.

| Code | Meaning and consequence |
| --- | --- |
| `AE-VERSION` | Producer/version differs from established records; compatibility is uncertain. |
| `AE-SELECTION` | Retained selection identifier; ambiguous current imports fail and require an exact composition ID. |
| `AE-PROJECT-METADATA` | Project organization/editor metadata is not fully reconstructed. |
| `AE-COLOR-MANAGEMENT` | Color interpretation or conversion is bounded/approximate; see HDR/ACES limits. |
| `AE-COMPOSITION-SETTINGS` | Some per-composition settings are not represented. |
| `AE-GROUP-BOUNDS` | Fixed native canvas and child-bounded FX geometry can differ. |
| `AE-INDEPENDENT-COPIES` | Reused native sources become independent editable instances. |
| `AE-PLACEHOLDER` | Missing/unsupported source becomes non-rendering content; pixels are lost. |
| `AE-PROPERTIES` | A property, expression, effect or animation is defaulted, approximated or omitted. |
| `AE-PARENTING` | Parent chains or inherited controls are incompletely represented. |
| `AE-TRACK-MATTE` | A matte relation/provider is unsupported or incomplete. |
| `AE-BLEND-MODE` | Transfer-mode mapping/fallback can change pixels. |
| `AE-LAYER-SWITCHES` | Some rendering switches are unsupported or approximated. |
| `AE-LAYER-METADATA` | Labels, comments and other editor metadata are not retained as controls. |
| `AE-TIMING` | Time projection, invalid ranges or fallback clocks can change playback. |
| `AE-MISSING-REFERENCE` | Missing source/parent/matte is omitted or represented as a placeholder. |
| `AE-CYCLE` | A cyclic graph branch is stopped; independent branches remain. |
| `AE-EXPANSION-LIMIT` | A bounded graph expansion omits excessive nesting; this is not process memory isolation. |
| `AE-UNKNOWN-ITEM` | Unknown source kinds are not guessed into visible content. |
| `[AE-EXPORT]` | Contextual native-export omission/approximation; independent siblings remain where safe. |
