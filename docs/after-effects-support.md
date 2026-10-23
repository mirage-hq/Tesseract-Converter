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

FX variables have no After Effects equivalent. Export writes each linked value
as the value it reads now; variable definitions and links are not exported. An `[AE-EXPORT]` warning reports this flattening.

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

FX `boxFit: "shrink"` draws box text at the largest size, at most `fontSize`,
whose lines fit the box. AE Source Text has no such field, so export writes the
authored size with an `[AE-EXPORT]` diagnostic, and text that FX shrinks
overflows its box in AE. Import always produces `"none"`. No test, Adobe open,
readback or render proof covers this omission.

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
CC Vignette import is omitted; FX `vignette` exports as Lumetri Color's Vignette section (see the colour tables). Unsupported
CustomShader output omits its rendering owner rather than showing an unshaded fill.
See the [effect reference](formats/after-effects-effects.md) for controls and units.

### Native colour effects and Compositing Options

<a id="native-colour-effects-and-compositing-options"></a>

Native colour effects convert to and from canonical FX grades as editable
approximations (`AE-PROPERTIES` diagnostics). Adobe's colour math is not the FX
v1 equations. Control identities, labels and native ranges come from the
independently authored AE 26.5 colour catalog
(`tests/fixtures/effects/color_catalog.aep`, its fresh-host readback receipt and
the matching `definitions.json` entries). Every labelled control also requires
the instance's own `pard` label to match; a different label omits that control
with a diagnostic, never a guess. The catalog holds every effect at its Adobe
defaults. The natively authored single-feature fixtures
(`tests/fixtures/color_ae_{lumetri_basic,color_balance,channel_mixer,selective_color,effect_compositing_options}.aep`,
provenance in `tests/fixtures/color_ae_fixtures.provenance.json`) pin nondefault
values; fresh imports of each are asserted in
`structure_document::tests::native_color_effects`. Their aerender renders are
native evidence kept locally, not committed (SHA-256 and requests in the
provenance file). Adobe acceptance of our exports
(AE 26.5x89, `tsrct-conv --fps 30` from explicit FX documents with one chart
still layer and the fixtures' values; `inspect_aep` readback plus `render_aep`
30 fps through the same route as the references) is recorded per export row;
it proves AE accepts and renders these values, not that FX's own renderer
matches AE. The Levels Histogram and Exposure-bound checks ran with the
`tsrct-conv` binary built at `424a89f1e4` (sha256
`c05c10ba29814a7c433864de328ce66d8ebeea7c777c05eae51266d6d5dc0f62`) in AE 26.5x89.

| Native source → FX (import) | Replacement and units | Omitted / approximated | Evidence |
| --- | --- | --- | --- |
| `ADBE Lumetri` Basic/Creative → `primaryGrade` | Temperature /116, Tint −/100, Exposure stops /2.6 (stops /2.2 in an OCIO ACES project, see ACES colour management), Contrast /360, Highlights /240, Shadows /510, Whites /54, Blacks /124, Saturation (s−100)/110, Vibrance /167, scales fitted to Lumetri renders (single-control Premiere 26.5.2 probes on an in-house ramp; AE 26.5 renders the same scalars within 0.32/255, and an in-house AE 26.5 probe of eight single controls matched the Premiere probes); Creative Saturation folds multiplicatively; affine keys stay editable. | Out-of-range values clamp; out-of-range keys are dropped with the clamped initial value. Disabled sections are omitted. A browsed custom Input LUT / Look, RGB Curves, Hue Saturation Curves, Color Wheels and Vignette import as the separate rows below. HSL Secondary and split toning have no FX mapping (reported when set). A built-in Look or Input LUT (popup 5 and above) is reported by the name AE saves for it (`-0096` / `-0095`, UTF-16; AE 26.5 scripted probe: "SL BIG MINUS BLUE", "AlexaV3_K1S1_LogC2Video_DCIP3_EE"); AE saves no samples for a built-in LUT (it ships with the application), so it cannot be converted. Faded Film, Sharpen, Tint Balance and HDR controls have no FX field (omitted). A neutral Basic/Creative grade beside an imported section adds no `primaryGrade`, as in the Premiere importer. Lumetri tone math is not reproduced (gamma-2.4 linear-light operations, highlight shoulder, white-point Whites, luminance-keeping white balance): the import diagnostic states the measured residual per control, above the 2-code gate for every control at UI ±50 except a negative Exposure (see `docs/formats/premiere.md`, Lumetri Color). | native fixture `color_ae_lumetri_basic` (comp 1: Temp 30, Tint −15, Sat 130, Exposure 0.5, Contrast 30, Highlights −25, Shadows 25, Whites 15, Blacks −15) → asserted FX values (`native_lumetri_basic_fixture_imports_editable_primary_grade`); FX render vs the AE reference: see "Grade render gate" below |
| `ADBE Lumetri` custom Input LUT (`-0004`) / Look (`-0024`, Intensity `-0026`) → `lookTransform` | A `.cube` browsed in Basic Correction > Input LUT or Creative > Look: AE 26.5 saves it as Premiere does (`crate::lumetri::luts`): the absolute path in the path slot (UTF-16LE, NUL-terminated), the `Blob` (`-0001`) shader graph applying that path, and an `ITX3` copy of the samples in Embedded LUTs (`-0098`), equal to the file's values as `f32`. Import packages that copy as a `.cube` asset (`aep-lut-<content hash>`, shared with Apply Color LUT, six decimals: the in-house file byte for byte) and writes a `lookTransform` {`lut3d`, `jerboa:sdr-rec709-display:v1` in/out, `mix` = Intensity/100}: the Input LUT before the `primaryGrade`, the Look after it (Lumetri order); a neutral grade adds no effect. The popup (`-0005` / `-0025`) does not decide: AE saves it only in an instance's own `parT` (the fixture's Look instance has none) and reads back 2 when the file exists, 1 when it does not, rendering the LUT in both cases. | **Approximation, stated**: FX is trilinear on display values; the fixture's per-channel LUT cannot measure AE's interpolation of channel-mixing LUTs. Not imported, with the reason: no readable embedded copy (the file itself is not read), a `Blob` that applies another path or none, a saved popup that selects Browse, a separator or a built-in LUT, the section off, Intensity outside 0–200 or unequal to the `Blob`'s `__Intensity`. Intensity above 100 clamps to `mix` 1; Intensity keys keep the initial value; a Look Color Space (`-0130`) other than the fixture's 29 and Creative Vibrance/Saturation (applied after the Look in Lumetri, before it in FX) are stated. Export: `lookTransform` writes Apply Color LUT (row below), not a Lumetri Look. | human-authored AE 26.5 UI fixture `color_ae_lumetri_custom_lut` (comp 2 `Custom Look`, comp 15 `Custom Input LUT`, `color_warm_tint.cube` 17³; provenance `color_ae_fixtures.provenance.json`) → `adapter::apply_color_lut::lumetri_tests::ui_browsed_lumetri_luts_import_as_editable_look_transforms_and_export_as_apply_color_lut` (fresh import: one `lookTransform`, `mix` 1, packaged `.cube` within 1e-7 of the file; export → reimport within 16-bit rounding), `effects::color_grades::tests::lumetri_browsed_luts_*` (Input LUT before and Look after a graded `primaryGrade`, Intensity 60 → `mix` 0.6, missing copy, section off, conflicting popup), `lumetri::luts::tests`. Adobe (AE 26.5x89, `tsrct-conv` built at `fc243d7720`, sha256 `e69dd864…57e5`, `--fps 30`): native references (render_aep jobs 12087 Look, 12088 Input LUT) are the chart through the `.cube` once (frame 30: 0.65 mean codes from a trilinear application to the display codes, 12.96 from the ungraded chart); the sanitized committed bytes render decoded-frame identical (jobs 12089/12090). FX render of each fresh import vs the AE reference: 60-frame mean **0.34** (max 18); its export (Apply Color LUT) rendered by AE (jobs 12105/12108): **0.17** (max 2). |
| `ADBE Lumetri` RGB Curves (`-0039`) and Color Wheels (`-0047`) → `colorCurves` | AE saves the same arbitrary data under the same `ParameterID` as Premiere (Premiere `ParameterID` N is `ADBE Lumetri-00NN`), so the Premiere-measured conversions are shared (`crate::lumetri`): RGB Curves → one `colorCurves` (Lumetri's natural spline fitted within one code, Luma as Master), Color Wheels → one `colorCurves` (the measured lift/power/gain model), each after the grade in Lumetri order, when its section is on. | The curves/wheels approximations of the Premiere rows (Luma-as-Master on saturated colours; wheels within 3.2 codes of Premiere); keyed or expression-driven payloads and a section that is off are reported, not converted. Export writes these curves as a native Curves effect, not back into Lumetri. | unit tests `lumetri_curves_wheels_and_vignette_follow_the_grade_in_lumetri_order`, `lumetri_sections_off_or_unmapped_data_are_diagnosed`, `lumetri_built_in_look_is_omitted_by_its_saved_name`. Adobe: an AE 26.5 project authored by headless-adobe `run_jsx` (Lumetri on the chart, job 11971) with in-house RGB Curves (Red (0,0)(0.5,0.25)(1,1), Blue (0,0.2)(1,0.8)) and Color Wheels (Shadows 240°/0.3, Midtones 120°/0.4, Highlights 30°/0.3) patched into its saved payloads (same length) rendered in AE (job 11974); the fresh import (converter `5e0bd9579d`) rendered by FX: 60-frame mean 0.76, max 11 codes against an effect size of 31.4. Local evidence, not committed. |
| `ADBE Lumetri` Hue Saturation Curves (`-0042`, `-0106`, `-0108`, `-0110`, `-0112`) → `hueCurves` | The same five curve records as Premiere's (same `ParameterID`s), converted by the shared Premiere-measured model (`crate::lumetri::hue_curves`): matched to FX `hueCurves` at reference colours, after the RGB Curves' `colorCurves`, when a curve has points and the Curves section is on. | **Approximation** (see the Premiere ledger: FX keys on display HSV and keeps display luma, Lumetri works in gamma-2.4 linear light); points under another kind than 3 (hue) or 2 (Luma/Sat), a single off-neutral point and keys are reported, not converted; the section off is reported. | unit tests `lumetri_hue_saturation_curves_import_as_hue_curves_after_the_rgb_curves`, `lumetri::hue_curves::tests`. Adobe (AE 26.5x89): the `hueCurves` export below, opened and rendered by AE (headless-adobe jobs 12061 inspect, 12062 render), reimported (`--composition 1`, converter `cd2e2f8cd8`) and rendered by FX: 60-frame mean 2.96 (max 32) from AE's render, against an FX effect of 7.67; the FX round trip is within 0.17. Scripting cannot set the curve data, so no AE-authored curve is pinned. |
| `ADBE Lumetri` Vignette (`-0050`..`-0054`) → `vignette` | Amount, Midpoint and Feather by a fit to AE 26.5 renders of 16 settings on a flat grey frame (`crate::lumetri::vignette`): amount from a measured table of Amount (corner gain), radius and feather from the ramp centre 0.31 + 0.0038·Midpoint + 0.00048·Feather and half-width Feather·(0.00201 + 0.0000367·Midpoint). Shared with the Premiere importer. | Roundness is reported (falloff follows the layer aspect); keys keep the initial values; the section off is reported. **Approximation**: within 1.69 mean codes over the 16 flat-grey settings (4.1 for Amount +2); on the chart, FX vs AE 4.35 (was 31.1 with the earlier −Amount/5, Midpoint/100, Feather/100 mapping). | `effects::color_grades::tests` above; `lumetri::vignette::tests` (closed form vs the per-setting AE fits, inverse). Adobe: vignette probe (headless-adobe jobs 11975 author, 11977 render); the job-11960 project with Amount −2, Midpoint 40, Feather 60 (scripted) renders in AE (job 11963) and its fresh import renders in FX at 60-frame mean 4.35 (max 57). With curves and wheels as well (job 11961 render): FX vs AE 3.56 (30 frames). |
| `ADBE Color Balance 2` → `colorCurves` | Per-channel curves (FX Master identity): AE 26.5 Color Balance is an exact per-channel transfer (a table read from a grey ramp predicts every colour with error 0, 156 measured looks), modelled per channel as a Levels map on eight-bit codes. Preserve Luminosity off: input black max(0, −shadow), input white 255 − max(0, highlight), gamma 2^−((2·midtone + shadow + highlight)/200). On: shadows raise black points by max(shadows) − shadow, highlights lower white points by highlight − min(highlights), midtone gamma 2^−((midtone − (max+min of midtones)/2)/100). A brightening gamma has a slope-limited toe below code 32 (cubic, fitted); a black point at or above the white point is a step at the white point. Each channel table is fitted with ≤ 32 piecewise-linear knots within one code. | **Approximation, diagnosed**: the model is within 2 codes of 99.9 % of 156 measured AE tables (mean 0.12; 24 held-out random looks mean 0.37, worst entry 10 codes on a steep ramp of fractional balances). The nine balances and Preserve Luminosity are not editable as balances in FX (edit the curves) and keys keep their initial values (FX `colorCurves` is static). 16/32 bpc and OCIO projects are unmeasured. | native fixture `color_ae_color_balance` (Shadow Blue 40, Midtone Red 30, Highlight Green −25, Preserve Luminosity on) → asserted curves within 2 codes of AE's measured per-channel codes (`native_color_balance_fixture_imports_editable_per_channel_curves`); model vs AE tables (`effects::color_grades::color_balance::tests`, probes rendered by headless-adobe jobs 11930–11935 and 11944). FX render of the fresh import (converter built at `65d26c0a09`, release `tsrct` at `2ed36e21e8`) vs the AE reference: 60-frame mean 0.95, max 20 (was 34.5 as `tonalColor`) |
| `ADBE CHANNEL MIXER` → `channelMixer` | Weights and constants /100; Monochrome flag. | Static FX: keys keep the initial value. Constants beyond ±100 clamp to ±1. Clipping/alpha unverified. | native fixture `color_ae_channel_mixer` (Red-Red 70, Red-Green 30, Blue-Const 10); render unmeasured |
| `ADBE SelectiveColor` → `selectiveColor` | Nine Details groups × CMYK /100; Method popup 1/2 → relative/absolute. | Static FX: keys keep the initial value. A visible-slider/Details-group disagreement is diagnosed and the group value used (scripted authoring leaves the visible sliders at 0; the Details groups are authoritative — Blues changes render although Reds is the selected range). FX range weights and ink math differ. | native fixture `color_ae_selective_color` (Relative; Reds Cyan −60/Magenta 20, Blues Yellow −50); render unmeasured |
| `ADBE Apply Color LUT2` → `lookTransform` | AE keeps neither the picked `.cube` nor its path: it converts the LUT into an ICC v4 RGB→RGB device link (A2B0 `mAB `: identity A/M/B curves, identity matrix, the LUT as a 16-bit CLUT, red slowest) stored as the instance's sequence data (`sdat`, 28-byte header), and keeps only the file name (profile `desc`, `-0001` label `Using: <name>`). Import decodes that CLUT into a `.cube` asset (`aep-lut-<content hash>`, six decimals, red fastest) and an FX `lookTransform` {`lut3d`, `jerboa:sdr-rec709-display:v1` in/out, trilinear, mix 1}; equal LUTs share one asset. Linked (Dynamic Link) pictures package the same assets. | Approximated (diagnosed per instance): samples are AE's 16-bit codes, within 1/131070 of the picked file; FX applies the LUT with trilinear interpolation in display-referred SDR, while AE interpolates tetrahedrally at its default project LUT Interpolation Method (`lutInterpolationMethod` 1) in the working space (identical only with working space None); every instance's diagnostic states the measured interpolation gap (below). A profile other than this identity-curve device link (non-identity curves or matrix, non-cubic grid) or a missing `sdat` is omitted with the reason; no file lookup is attempted because AE stores no path. Effect Compositing Options follow the shared rule. | human-authored AE 26.5 UI fixture `color_ae_apply_color_lut` (comp 2, `color_warm_tint.cube` 17³) → asserted CLUT within 16-bit rounding of the picked file, registry `pard` bytes equal to its definition and instance (`effects::apply_color_lut::tests`), fresh import → packaged asset → export → reimport yields the same `lookTransform` (`adapter::apply_color_lut::tests`). Native reference `tests/references/aep/color/color_ae_apply_color_lut__composition_2.direct-v1.mp4` (aerender 30 fps): vs the same chart rendered by AE without the effect passed through the `.cube` with trilinear interpolation, frame 30 full-resolution MAD 0.62, max 2 (this LUT is per-channel affine, so it cannot discriminate trilinear from tetrahedral). FX-renderer comparison (converter sha256 `53c1aab1…cf7d1`; release `tsrct` at `94971429d9` through the export queue): the fresh import of comp 2 rendered by FX vs that reference, first 60 frames, all pixels: **MAD 0.50/255** (max 18 on patch edges, PSNR 46.1 dB) — passes the 2-code gate. Interpolation (AE 26.5x89, in-house channel-mixing 17³ LUT (local evidence, not committed), on an in-house 8-bit RGB sweep, against float predictions): AE renders **tetrahedrally** (least-squares position 0.993 between the trilinear (0) and tetrahedral (1) predictions over the 688 621 pixels where they differ by ≥ 2 codes); the two interpolations differ by mean 0.70, max 13 codes on the sweep. FX (trilinear, 0.063) vs AE on that LUT: MAD 1.33/255 (max 38; AE's ungraded sweep floor 0.75) — passes the gate with the interpolation residual. The FX choice (trilinear) and the affine-equal / channel-mixing-different claim are pinned on tiny generated LUTs by `effects::apply_color_lut::tests::lut_interpolation_is_trilinear_and_differs_from_tetrahedral_only_for_channel_mixing_luts`. |
| `ADBE CurvesCustom` → `colorCurves` | AE saves every channel (RGB, Red, Green, Blue, Alpha) twice: as curve-editor points (up to 16 eight-bit (input, output) pairs, a count and the selected point) and as a 256-entry eight-bit table that is the half-up natural cubic spline through those points (0-code error on the UI-authored fixture). AE renders the table: an export carrying the fixture's points over identity tables rendered identical to the ungraded chart (MAD 0). Import fits each table with ≤ 32 piecewise-linear knots (≤ 1 code at every sample); the RGB table becomes the FX Master curve and Red/Green/Blue stay separate. The diagnostic names the authored points and whether the table equals their spline. | FX curves are piecewise-linear, so AE's spline is approximated by its fitted table (the fixture's one authored point becomes 8 knots). A combined RGB + channel state is folded into the FX Red/Green/Blue curves (FX Master identity, the RGB curve no longer separately editable): AE applies each channel before the RGB curve (a Master x/2 + Red invert payload rendered as Red-then-RGB, MAD 0.73; FX's Master-then-channel order gives MAD 42.7), the reverse of FX. Animated curves and a nonidentity alpha table are omitted. Carries Compositing Options through the shared rule below. | Native fixture `color_ae_curves` (human-authored in the AE 26.5 UI: RGB point dragged to (127,162), comp 2, 8 bpc, working space None; original sha256 `96b6abaa…77e7`) → `effects::curves::fixture_tests::{ui_authored_curves_store_rgb_points_and_their_natural_spline_table, native_ui_curves_fixture_imports_the_rgb_curve_as_the_fx_master_curve}` (8 Master knots asserted, Red/Green/Blue identity). Native aerender reference `color_ae_curves__composition_2.direct-v1.mp4` equals the saved table applied to AE's render of the same project with Curves disabled (MAD 0.26, max 6). Import-direction render: FX render (`tsrct` release built at `94971429d9`) of the fresh import (converter `7997f13406`, sha256 `59cfb6f1…c68f`) vs the native reference: 60-frame mean abs diff 0.32 (max 16, p99 5); same pipeline with the curve removed vs AE with Curves disabled: 0.17 (max 13) — the effect itself moves the picture by 24.3. Synthetic tables for the fold, alpha and fit guards. |
| `ADBE Brightness & Contrast 2` → `brightnessContrast` | Brightness and Contrast keep their native units; affine keys stay editable. `Use Legacy (supports HDR)` (`-0003`) is read, never reported as an unmapped control. | FX has one kernel (contrast about 0.5 by 1 + C/300, then HSV-value brightness). AE 26.5 default mode (Use Legacy off) is approximated: mean 3.9–11.8/255, max 24/255 on an 8 bpc ramp. Use Legacy on is diagnosed by name with its measured formula (adds B/255, then scales about 0.5 by 100/(100 − C) for C > 0 or (100 + C)/100 otherwise) and kept as the same values: mean 7.0–22.8/255, max 53/255. HDR (above 1.0) behaviour of legacy mode unmeasured. | native fixture `color_ae_brightness_contrast_modes` (nine bands, both modes; AE 26.5x89, 8 bpc, working space None; reference `tests/references/aep/color/color_ae_brightness_contrast_modes__composition_1.direct-v1.mp4`); `structure_document::tests::native_brightness_contrast::native_brightness_contrast_modes_import_with_the_legacy_mode_named`. Measured: lossless AE render (job 10909) against the FX shader formula evaluated on the same ramp; an FX GPU render of the import is unrun. |
| Effect Compositing Options → `compositingOptions` | Effect Opacity (stored as a percentage, `tduM` 100) /100 → `effectOpacity`; each `ADBE Effect Mask` > `ADBE Effect Path Stream Ref` `tdli` mask identity → the same-layer FX mask imported from the layer mask with that `mkif` identity (`maskReferences`), for the one FX effect a native instance produces. After Effects 26.5 (aerender, 8 bpc; in-house probe of eight mask/reference layouts) renders a reference to a mask in mode None as no restriction, and restricts the effect to a referenced mask in mode Add without applying that mask to the layer. So a reference to a mode-None mask is omitted (the effect applies to the whole layer, diagnosed) and a referenced mode-Add mask imports as an FX mask in mode `none` with the reference (diagnosed); a referenced mask in another mode keeps it, diagnosed as unmeasured. | Opacity outside 0..100 is omitted; opacity keys keep the initial value. A reference whose mask is absent, ambiguous, animated or was not imported is diagnosed and that restriction omitted (the effect then applies to the whole layer). Matte/temporal/opaque FX classes reject options; multi-record lowerings (Toner, Roughen, 3D Glasses) do not split them. GPU Rendering choice is ignored. FX keeps the referenced mask's own layer-masking role (AE mode None imports as FX `none`, so it does not mask the layer). Authored Compositing Options on native effects that produce no FX effect record (omitted, or lowered to adjustment-layer opacity, a generator or a frame fade) are not imported, and neither are any on a layer whose FX effect records cannot be paired one-to-one with their native instances; both are diagnosed per native effect. | native fixture `color_ae_effect_compositing_options` (Levels Gamma 0.5, Effect Opacity 50, Mask Reference 1 → Mask 1, mode None) → asserted `effectOpacity` 0.5 and no mask reference (`native_effect_compositing_options_fixture_imports_opacity_and_drops_its_mode_none_mask_reference`); its aerender reference darkens both halves identically, as the probe predicts; unit tests `a_referenced_mode_add_mask_restricts_the_effect_without_masking_the_layer`, `a_referenced_mode_none_mask_leaves_the_effect_unrestricted`. FX render vs the reference: see "Grade render gate" |

| FX → native (export) | Replacement and units | Omitted / approximated | Evidence |
| --- | --- | --- | --- |
| `primaryGrade` → `ADBE Lumetri` | Fresh Lumetri with Basic/Creative scalars by the inverse of the fitted import scales (Temperature ×116, Tint ×−100, Exposure ×2.6 (×2.2 in an FX ACES document), Contrast ×360, Highlights ×240, Shadows ×510, Whites ×54, Blacks ×124, Saturation 100+110s, Vibrance ×167); scalar keys export as native keys. | Values and keys are clamped to each control's declared native valid range (`pard` float-slider valid min/max) with a diagnostic naming the control and the written value; with the fitted scales Exposure (FX ±5 ×2.6 = ±13 against ±7), Contrast (±2 ×360 against ±150), Highlights (×240), Shadows (×510), Saturation (100+110s below 0) and Vibrance (×167) can leave it; FX exposure beyond ±2.69 stops is written as ±7. The Adobe acceptance below ran with the earlier range scales (×300, ×2.2, ×50, ×100 …); it proves the native records, and the import→export round trip of the fixture writes its own saved values back under either scale set. AE clamps on load too: an Exposure of 8.8 (FX 4), written without the clamp, opened without error and read back as 7, the declared valid maximum, not the ±5 slider range. All 24 Lumetri arbitrary-data (`pard` kind 11) controls are written with AE 26.5's native default payloads (Blob/LUT-slot `FE FE` sentinel, identity RGB and Hue/Sat curves, neutral wheels/HSL, empty Auto Tone analytics) in the declaration table and as instance leaves, and each float slider's value is mirrored into its declaration as AE does; so curves, wheels, HSL and LUT state stay at default. Project-level effect definitions keep the registry defaults and always use the 24 fps property clock (`0x6000`, as in the catalog) at every composition rate, while instance leaves use the composition clock; native 30 fps saves store `0x7800` in their definitions too. The Adobe acceptance below ran on a build that still wrote `0x7800` in every arbitrary-data leaf, so AE's handling of the `0x6000` definition clock at other rates is unrun. Topic/button leaves (kinds 13–15) are omitted, as for every exported effect. | declaration table and arbitrary leaves byte-equal to `color_catalog.aep` and to the graded `color_ae_lumetri_basic` instance (`writer::effects::color_fixture_tests`); own-reader round trip. Adobe acceptance: AE opened the export and read back the nine Basic values exactly with every other control at the catalog default; render decoded-frame identical to `color_ae_lumetri_basic` (60/60 frames, MAD 0). Exposure bound (AE 26.5x89, binary built at `424a89f1e4`, before the writer clamped): `inspect_aep` of the 8.8 export read Exposure 7; the other controls it read (Temperature, Tint, Saturation, Contrast) were neutral. This export was not rendered. The clamped export in AE (tsrct-conv sha256 `e1894563…6482128` built at `94971429d9`, AE 26.5x89): our export of FX exposure 4 (written 7 with the range diagnostic) renders decoded-frame identical (60/60 frames, MAD 0) to a Lumetri with Exposure 7 scripted natively in AE (8 bpc, working space None) |
| `tonalColor` → `ADBE Color Balance 2` | Range balances ×100, Preserve Luminosity on; range keys export as native keys. | Global offsets fold equally into all three ranges (clamped to ±100); global keys are not folded. | declaration table byte-equal to `color_ae_color_balance` (no arbitrary data); own-reader round trip. Adobe acceptance: values and Preserve Luminosity on read back; render decoded-frame identical to `color_ae_color_balance` (60/60, MAD 0) |
| `channelMixer` → `ADBE CHANNEL MIXER` | Weights/constants ×100 and Monochrome. | Native clipping/alpha unmeasured. | declaration table byte-equal to `color_ae_channel_mixer` (no arbitrary data); own-reader round trip. Adobe acceptance: values read back; render decoded-frame identical to `color_ae_channel_mixer` (60/60, MAD 0) |
| `selectiveColor` → `ADBE SelectiveColor` | Details-group inks ×100 and Method; the visible sliders mirror Reds. | Native ink math unmeasured. Topic leaves (kinds 13/14) are omitted, as for every exported effect. AE reports the visible sliders as 0 after open (as in the native fixture), not the mirrored Reds values. | declaration table byte-equal to `color_ae_selective_color` (no arbitrary data); own-reader round trip. Adobe acceptance: Method and Details inks read back; render decoded-frame identical to `color_ae_selective_color` (60/60, MAD 0) |
| `colorCurves` → `ADBE CurvesCustom` | Fresh Curves whose 256-entry RGB/Red/Green/Blue tables sample the piecewise-linear FX curves (rounded to one code; alpha identity); AE renders these tables. Each nonidentity FX curve's knots are also written as that channel's editor points (no selected point) when they fit AE's 16-point editor at distinct eight-bit input codes, so AE's curve panel shows the FX shape. Definition/instance records and the default payload match the native catalog byte for byte. | AE's panel draws its natural cubic spline through the written knots; the diagnostic states how far that spline is from the rendered piecewise-linear table (1 code for the imported fixture curve), and editing the curve in AE regenerates the table from the spline. Curves with more than 16 knots, or knots that collide at eight-bit codes, keep the default diagonal editor points (table still exact). A Master curve beside channel curves is folded into the channel tables (native RGB identity, folded channels keep the default diagonal) because AE applies each channel before the RGB curve. | Catalog layout; own-reader round trip; `effects::curves::fixture_tests::imported_ui_curve_exports_its_knots_as_editor_points_and_reimports`, `effects::curves::tests::curves_encode_writes_fx_knots_as_editor_points_when_they_fit_the_native_editor`. Adobe acceptance (AE 26.5x89, converter built at `7997f13406`, sha256 `59cfb6f17cef80b02ef76a252dc799cd40ab69159799b079e4ed02a359e1c68f`, `--fps 30`): the fresh import of `color_ae_curves` exported back to AE opened and rendered against the native reference `color_ae_curves__composition_2.direct-v1.mp4` at 60-frame mean abs diff 0.17, max 1 code; a Master (0,0)(0.5,0.65)(1,1) + Red (0,0.1)(1,0.9) export (folded) rendered within MAD 0.63 (max 4) of FX's Master-then-Red order applied to AE's ungraded render. The fresh import of `color_ae_color_balance` (per-channel curves, converter `65d26c0a09`) exported back opened in AE (`inspect_aep`: one Curves effect) and rendered within 60-frame MAD 0.84 (max 4) of the native Color Balance reference and 0.26 (max 18) of the FX render of the import; the balances come back as a Curves effect, not as Color Balance. The curve data is not readable through scripting, so AE's panel display of the written points is not verified by Adobe (byte layout equals the UI-saved points block except the selected index). |
| `brightnessContrast` → `ADBE Brightness & Contrast 2` | Brightness/Contrast in native units; Use Legacy is written off (default mode, the closer of the two AE kernels to FX). | FX and AE default-mode kernels differ (mean 3.9–11.8/255, max 24/255 on an 8 bpc ramp); the export carries this as its effect note. | own-reader round trip: fixture import → export → reread keeps all nine values with `-0003` = 0 (`brightness_contrast_export_writes_use_legacy_off_with_the_fx_values`). Adobe acceptance: AE 26.5x89 opened and rendered the CLI export (fixture → `.tsrct` → `--to after-effects --fps 30`, binary at `5b11a54d40`; lossless `render_aep`, frame 0): the three default-mode bands match the native render within 1/255 (mean ≤ 0.26/255), the bare ramp and the identity band are identical, and the five non-identity legacy bands now render in default mode (mean 14.2–23.8/255 from the native legacy render, as documented). Control readback of the export: unrun. |
| `lookTransform` → `ADBE Apply Color LUT2` | A `lut3d` look in `jerboa:sdr-rec709-display:v1` (in and out) becomes a fresh Apply Color LUT at its stack position: the archive `.cube` (3D, unit domain, 2..255 grid) is encoded as the same device-link sequence data AE writes (16-bit codes, nearest rounding), the `-0001` declaration label names the archive file, and the project-level definition carries the first instance's LUT as AE saves it. The `.cube` bytes are also published beside `project.aep` (names made unique). `mix` < 1 becomes Effect Opacity `mix × 100` %, multiplied into an exported `compositingOptions` Effect Opacity. | Diagnosed: AE re-reads no file (the LUT lives in the project); AE's interpolation may differ from FX trilinear; samples outside 0..1 are clamped (counted); a bypass (`transform: null`), other encodings, a 1D/`LUT_3D_INPUT_RANGE`/non-unit-domain `.cube` or an unreadable asset omit the effect with the reason. The profile's preferred CMM/creator and profile ID are left empty, the date fixed, and the copyright text is our own; topic leaves (`-0001`/`-0002` instance leaves) are omitted as for every exported effect. Dynamic Link (Premiere-hosted) AEP exports embed the LUT but do not publish the `.cube`. | `effects::apply_color_lut::tests` (encoded A2B0/desc/pseq tags byte-equal to the fixture's; the picked `.cube` encodes to the fixture's CLUT byte for byte), `adapter::apply_color_lut::tests` (round trip, published `.cube`, `Using:` label). Adobe acceptance (AE 26.5x89, `tsrct-conv` built at `5b9fbb023e`, sha256 `53c145fb5492c0e9bb77c159df099389e7873ad0e2f9e2c4c2995f34d2847bfa`, `--fps 30`): an explicit FX document (chart Image layer + `lookTransform` with the picked `color_warm_tint.cube` asset, mix 1) and a fresh import → export of the fixture both opened in a fresh host, read back one enabled Apply Color LUT with Compositing Options at defaults (8 bpc, working space None — the same readback as the native fixture), and rendered decoded-frame identical to `color_ae_apply_color_lut__composition_2.direct-v1.mp4` (60/60 frames, MAD 0; the native 29.97 fps comp yields a 61st frame). Adobe proof of `mix` and interpolation (AE 26.5x89, converter sha256 `53c1aab1…cf7d1`, `--fps 30`): `mix` 0.5 on the chart exports as Effect Opacity 50 % and renders within MAD 0.56/255 of the 50 % blend of the ungraded and the native LUT renders (0 % / 100 % blends: 6.39 / 6.50), and an FX render of the same document is within MAD 0.18 (max 15) of AE's; the channel-mixing look renders tetrahedrally in AE (see the import row) and FX vs AE is MAD 1.33 (max 38). Interpolation: every export diagnostic states that AE interpolates tetrahedrally at the `lutInterpolationMethod` 1 the writer saves while FX is trilinear (equal for per-cell affine LUTs; up to 13 codes on the channel-mixing LUT). Unrun in Adobe: clamped samples, Dynamic Link exports. ExtendScript names the `-0001` group `<LUT not set>` for the native fixture and for both exports although all three render the LUT (panel label not inspected by a human). |
| `vignette` → `ADBE Lumetri` Vignette | A fresh Lumetri Color whose Vignette section carries the FX vignette by the inverse of the fitted import (Amount, Midpoint, Feather; Roundness 0; values outside the saved ranges clamp with a diagnostic); every other Lumetri section at AE 26.5's defaults. Replaces the earlier static CC Vignette fallback. | Keys are not exported (the mapping is not affine). The fitted mapping is an approximation (see the import row). | `export_document::tests::color_grades::vignette_exports_as_the_lumetri_vignette_section_that_reimports_editably`. Adobe acceptance (AE 26.5x89, converter `5e0bd9579d`, `--fps 30`): the fresh import of the curves/wheels/vignette probe exported back (two Curves effects and one Lumetri per layer) opened in AE; `inspect_aep` (job 12002) read the Vignette as Amount −2, Midpoint 40, Feather 60 (the source values); the render (job 12003) is within 30-frame mean 0.94 (max 14) of AE's render of the source and 3.47 of the FX render. The probe's second one-second layer is omitted by the export ("Group source clock cannot be represented exactly"; a timing limit of that probe layout, not of colour), so only its first 30 frames are scored. |
| `hueCurves` → `ADBE Lumetri` Hue Saturation Curves | A fresh Lumetri Color whose Hue Saturation Curves carry the FX curves by the inverse of the import matching (at most 32 points of Lumetri's spline within 0.004 of the matched values), every other section at AE 26.5's defaults; the payloads replace the default arbitrary data in the effect's parameter table and its instance leaves. | **Approximation, declared** (the import row); static by schema. | `export_document::tests::color_grades::hue_curves_export_as_lumetri_hue_saturation_curves_that_reimport`. Adobe acceptance (AE 26.5x89, converter `793864bcf2`, `--fps 30`): a chart export (Hue vs Sat dip on greens, Hue vs Hue turn of reds, Luma vs Sat in the shadows) opened in AE with one `ADBE Lumetri` (`inspect_aep` job 12061) and rendered (job 12062) within a 60-frame mean of 2.79 (max 32) of the FX render, against an FX effect of 7.67. |
| `compositingOptions` → effect Compositing Options | Effect Opacity → `ADBE Effect Mask Opacity` percentage (×100, bounds 0..100); each `maskReferences` entry → one `ADBE Effect Mask` ("Mask Reference n") whose `ADBE Effect Path Stream Ref` `tdli` is the native identity (one-based parade index) of the native mask exported from that FX mask on the same native layer. A referenced FX mask in mode `none` is written in mode Add, because AE 26.5 ignores a reference to a mode-None mask and restricts the effect to a referenced Add mask without masking the layer (measured); a referenced mask in another mode keeps it, diagnosed. A control at its default is not written (AE omits it); a neutral record keeps the empty group. | The HSL qualifier is omitted (AE has none outside Lumetri's opaque HSL Secondary). A reference is omitted with a diagnostic when its FX mask is not exported as exactly one native mask on the effect's native layer (omitted mask, inverted compound split into two contours, Group/carrier lowerings that move the effect away from its masks); with no resolved reference the effect applies to the whole layer. FX classes that forbid options (matte, temporal, opaque) and effects lowered to several native instances (`temperatureTint`) are written without options and diagnosed; native Layer Styles have no Compositing Options (diagnosed). Only the layout with both controls present is pinned natively; each control alone follows AE's omit-at-default rule (unpinned). Adobe render (tsrct-conv sha256 `e1894563…6482128` built at `94971429d9`, AE 26.5x89): our export of the fresh import of `color_ae_effect_compositing_options` renders decoded-frame identical (60/60 frames, MAD 0) to the fixture's reference; neither shows a mask restriction, as AE renders a mode-None reference (measured); with the mode-Add rule (tsrct-conv built at `f065e813c5`, AE 26.5x89), an FX document whose Levels references an FX mask in mode `none` (Effect Opacity 0.5) exports a mode-Add mask and its reference; AE renders it with the effect restricted to the mask and the layer uncut, against the FX render of the same document: 60 frames, mean 0.18, max 13 codes, PSNR 54.4 dB. | `Compositing Options` group byte-equal to the instance and project definition in `color_ae_effect_compositing_options.aep` (`writer::effects::color_fixture_tests::fresh_compositing_options_match_the_native_mask_reference_and_opacity_records`); FX → AEP → fresh import restores the mask reference and opacity (`export_document::tests::color_grades::compositing_options_export_native_mask_reference_and_opacity_that_reimport`) |
| `levels` → `ADBE Pro Levels2` Histogram | The Histogram arbitrary-data control (`-0002`, `pard` kind 11) is written with AE 26.5's native default payload (108 bytes, identical in `effects/catalog.aep`, `effects/levels_controls.aep` and `color_ae_effect_compositing_options.aep`) after its declaration and as the instance's `tdbs` + `aRbs` leaf, like Lumetri; both Levels plugins share it. The leaf descriptor carries the composition's property clock (also corrected for Lumetri/Curves leaves, which previously always wrote the 30 fps clock). | The payload is opaque and never encodes the FX levels values (the scalar controls do). AE also saves an 8208-byte plugin sequence-data record (`sdat`, `KLIW` header) in each Levels plugin record; the fresh writer omits plugin sequence data for every effect, as before. AE mirrors Levels slider values into their `pard` declarations as it does for Lumetri (the compositing fixture's Gamma `pard` holds 0.5, the catalog's 1.0); the writer does not mirror Levels values, an omission (the declarations keep the registry defaults). | declaration table and Histogram leaf byte-equal to `effects/catalog.aep` for both plugins (`writer::effects::color_fixture_tests::fresh_levels_records_carry_the_native_histogram_default`); Histogram leaf byte-equal to the 30 fps compositing fixture. Adobe acceptance (AE 26.5x89, binary built at `424a89f1e4`, 30 fps export of an FX `levels` document): AE opened the export without reset and read back Input Black 0.2, Input White 0.8, Gamma 1.6, Output Black 0.1 and Output White 0.9. The render is decoded-frame identical (60/60 frames, frame-30 MAD 0) to an in-AE scripted Levels with the same values (8 bpc, working space None, set explicitly in the native project); the omitted `sdat` record had no visible effect. The Histogram value is not readable through scripting |
| Image/Video/Media `source.inputTransform` | A `lut3d` reading and writing `jerboa:sdr-rec709-display:v1` becomes a fresh `ADBE Apply Color LUT2` at the **head** of the footage layer's effect stack (FX applies an Input Transform before every layer effect; AE footage has no per-source LUT), embedded exactly as a `lookTransform` export (same device link, `Using:` label, `.cube` published beside `project.aep`). | Diagnosed per layer: a re-import reads it as the layer's first `lookTransform`, not as `source.inputTransform`; AE interpolates tetrahedrally (as the `lookTransform` row). Other encodings (camera inputs), `cameraInput`, a future payload, and the half-canvas takeover placement export the footage ungraded with a layer diagnostic `source.inputTransform omitted: …`. | `adapter::apply_color_lut::tests::source_input_transform_lut_exports_as_a_head_of_stack_apply_color_lut` (fixture import with the LUT moved to the footage's `source.inputTransform` plus an authored Channel Mixer: written stack `ADBE Apply Color LUT2`, `ADBE CHANNEL MIXER`; CLUT equal to the native one; reimport yields the same `lookTransform`), `pr4442_media_cases::input_transform` (camera input stays omitted). Adobe acceptance (AE 26.5x89, converter sha256 `53c1aab1…cf7d1`, `--fps 30`): an FX document (chart Image whose `source.inputTransform` is the picked `color_warm_tint.cube`) renders **decoded-frame identical** to `color_ae_apply_color_lut__composition_2.direct-v1.mp4` (60/60, MAD 0); with the channel-mixing LUT on the RGB sweep it renders identical to the same LUT exported as a `lookTransform` (60/60); input + a `mix` 0.5 look of the same LUT reads back (`inspect_aep`) as two enabled Apply Color LUT instances at indices 1 and 2, and an FX render of that document is within MAD 0.60/255 (max 19) of AE's render; with the channel-mixing LUT, FX vs AE is MAD 1.33 (max 38). Saving `lutInterpolationMethod` 0 in the written project root did not change AE's render (still tetrahedral, 60/60 identical), so the writer keeps 1. |


#### Grade render gate

FX render of a fresh import (`tsrct export`, release renderer built at `94971429d9`, 30 fps, 1080p, first 60 frames)
scored against the committed Adobe reference of the native fixture (decoded 8-bit RGB mean absolute difference; gate
mean ≤ 2 codes). Converter built at `f065e813c5` against the earlier converter (`a504842df6`):

| fixture | before | after | verdict | reason |
| --- | --- | --- | --- | --- |
| `color_ae_lumetri_basic` (comp 1) | 29.15 (max 72) | 10.36 (max 30) | fail, diagnosed | fitted Lumetri scales; primaryGrade's code-value equations cannot follow Lumetri's linear-light math. Even the ten primaryGrade controls fitted freely to this chart's six colours leave 2.7 codes. |
| `color_ae_color_balance` (comp 1) | 34.54 (max 108) | 0.95 (max 20) | **pass** | imported as per-channel `colorCurves` from AE's measured transfer (converter `65d26c0a09`, renderer `2ed36e21e8`); as `tonalColor` it rendered 34.54. |
| `color_ae_effect_compositing_options` (comp 1) | 12.22 (max 41) | 0.34 (max 18) | **pass** | the mode-None mask reference is no longer imported as a restriction, as AE renders it. |

The Premiere counterpart `color_lumetri_basic` moves from 29.35 to 10.54 codes (`docs/formats/premiere.md`, Lumetri Color).

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

The owned ICC payloads and AE 26.5's built-in **"Rec.2100 PQ"** working profile
bind on import. The built-in profile is identified by its exact natively saved
ICC payload (`baseProfileType` 2, SHA-256 `e333f824…10bee0`, from
`tests/fixtures/color_ae_rec2100_pq_project.aep`), never by its name: it imports
as PQ HDR context whose point colours decode as BT.2020 PQ code values (absolute
luminance preserved) with FX `hdrGraphicsWhiteNits` **100**, the reference
luminance the profile's ICC `lumi` tag declares, so AE's own SDR-white mapping of
that profile stays FX white (AE stores no separate graphics-white control). The
owned profiles keep the FX default 609. Other built-in Rec.2100 variants
(PQ W100/W203/W300/W10000, HLG, HLG Scene) have no pinned native payload and stay
unsupported with a specific diagnostic; linearized working spaces stay
unsupported.

32 bpc super-white point colours (native code values above 1.0, e.g. the fixture's
Solid [2, 2, 2]) import and export in an admitted HDR context. The transfer curve
ends at its peak code 1.0 (PQ's inverse is undefined just above it), so beyond the
peak both directions continue it linearly: code `c > 1` is `c` times the peak light
(PQ: `c` × 10000 cd/m²), an FX colour above the peak at graphics white. A
contextual `AE-COLOR-MANAGEMENT` diagnostic names the codes on import and an
`AE-EXPORT` diagnostic on export; Source Text colour above the peak clips to the
peak code with a diagnostic. Without an admitted HDR context a super-white Solid
stays a diagnosed placeholder. Import and export return the source codes
(`structure_document::tests::hdr_native::native_built_in_rec2100_pq_project_imports_as_hdr_with_profile_white`,
`export_document::tests::hdr::native_rec2100_pq_super_white_solid_exports_its_super_white_codes_under_hdr_pq`,
`fx_conv` `super_white_codes_round_trip_only_with_headroom_admitted`). Adobe
acceptance (tsrct-conv sha256 `fd7e34db…275e11` built at `5dd6ea12e2`, AE 26.5x89,
AME 26.5.2): the aerender SDR route renders our `--hdr pq --fps 30` export of the
fresh import identical to the native render
`color_ae_rec2100_pq_project__composition_1.direct-v1.mp4` (60/60 frames, both
clip). The AME ProRes 422 HQ PQ route does **not** match the native render
`color_ae_rec2100_pq_project__composition_1.ame-pq-v1.mov`: Y 1003 vs 1019 (0/60
identical frames, 10-bit MAD 8.0, max 16). The exported code values are equal; the
working profiles differ. AE passes the native built-in profile's code 2.0 straight to
the PQ output, which clips it to Y 1019. Our export declares the owned PQ W203 v2 ICC,
and AE converts codes above 1.0 through it more gently: probe exports at codes 1.5 / 2 /
4 render Y 977 / 1003 / 1019, while code 1.0 renders 940 in both profiles. An
identical HDR render would need the export to declare AE's built-in profile, and this
change does not do that (open decision). An earlier converter clipped the Solid to the
peak code 1.0 (Y 940, MAD 39.5), and the original importer left a placeholder, which
rendered black.
`color_ae_rec2100_pq_solid.aep` (built-in "Rec.2100 PQ", 32 bpc, one Solid at PQ codes
[0.55, 0.45, 0.35]) does: its fresh import (white 100) exported with `--hdr pq` writes the same
PQ code values (`export_document::tests::hdr::native_rec2100_pq_solid_exports_its_pq_code_values_under_hdr_pq`).
Adobe acceptance (tsrct-conv sha256 `e1894563…6482128` built at `94971429d9`, AE 26.5x89, AME 26.5.2): AME renders that export through the
ProRes 422 HQ PQ route decoded-frame identical to its render of the native project
(`color_ae_rec2100_pq_solid__composition_1.ame-pq-v1.mov`; 60/60
frames, 10-bit MAD 0): import (white 100) and export reproduce this solid's PQ code values
exactly. Whether 100 nits is the level at which AE displays FX white is not separately measured; this is one
flat colour: gradients, footage, effects and FX-renderer agreement with AE's display of the
same value are unmeasured. Export keeps writing the owned ICC profile (AE reports it as
"Public BT.2100 PQ BT.2020 D65 W203 v2"), so a native reader sees a custom working profile,
not AE's built-in Rec.2100 entry. The native renders named in this section are
local evidence, not committed: their SHA-256 and requests are in
`tests/fixtures/color_ae_fixtures.provenance.json`, and no test reads them.

SDR working spaces and older producers import with the legacy numeric
interpretation and an `AE-COLOR-MANAGEMENT` diagnostic naming the declaration
(point colours are working-space code values that AE renders unchanged; AE's
display and output-module profile conversions are not modeled):

- AE 26.5's built-in **"sRGB IEC61966-2.1"** and **"Rec.709 Gamma 2.4"**
  working profiles, each bound by its exact natively saved ICC payload
  (`baseProfileType` 2; SHA-256 `2b3aa164…49af7e` and `d6e44b64…cb6c7`, from
  `tests/fixtures/color_ae_working_srgb.aep` and
  `color_ae_working_rec709_gamma24.aep`, 8 bpc). Linearized or linear-blending
  SDR working spaces stay unsupported.
- Projects saved before the `PwCs` declaration existed (project format < 96;
  observed in an After Effects 17.0 project, format 93, that is not
  committed). Such projects carry no `pcms`/`PwCs`/`pdvc` records; their
  working space is only the 16-byte ICC profile ID in `cpid`, with the
  referenced ICC payloads pooled in `LIST CPPl`. `cpid` all `0xFF` is working
  space "None" and is admitted (AE 26.5 reopens that project as working space
  None, 8 bpc, no linearization or linear blending — native readback). A
  non-None older `cpid` stays unsupported with the pooled profile's ICC
  description, because no older-format fixture with an ICC working space is
  pinned. Tests: `hdr::tests::pre_declaration_*` and
  `structure_document::tests::hdr_native::pre_declaration_project_converts_as_declared_legacy_with_the_producer_named`
  use an in-house fixture rewritten into the older layout (supplementary, not
  an independent older-producer save). Adobe proof: corpus open/readback ran
  (AE 26.5x89); no render comparison.

Admitted PQ/HLG point colors can map to extended-sRGB FX values with graphics white
normalized to 609 nits by default. This is a point-color bridge, not full footage,
gradient, effect or interpolation-domain color management. Unknown/linearized
profiles and unsupported points retain contextual approximations; gamut reduction
is a diagnosed color loss. Native working/output settings or neutral calibration
do not establish chromatic, alpha or high-precision render fidelity. Historical
saturated comparisons include failures and remain failures.

### ACES colour management (OCIO ACES 2.0)

After Effects 26.5 saves an OCIO colour-managed project as the `pcms` JSON
(`colorManagementSystem` 1, `ocioConfigurationFile` the built-in config's display
name), the `PwCs` working colour space and each footage item's `CLRS` `ocsp` input
colourspace as `baseProfileType` 3 declarations: `colorProfileName`
`<family>/<colourspace>`, `colorProfileData` base64 of `{"colorSpace1":<colourspace>}`
(`color_aces_ae_project.aep`). The FX ACES policy has no per-layer AE counterpart;
the mapping is document level (`ocio.rs`).

| Feature | AEP → FX | FX → AEP | Fixture / tests | Adobe proof |
| --- | --- | --- | --- | --- |
| OCIO ACES project | The built-in "ACES 2.0 Studio v4.0" or "ACES 2.0 CG v4.0" config with the ACEScg working colour space (or `{}`, the configs' `scene_linear` role) imports as `colorManagement` `acescct` / `aces2.0` / `wholeProgram`: AE composites every layer, including solids, text and effects, in ACEScg. Other OCIO configs (ACES 1.x, custom) or working spaces import as legacy with an `AE-COLOR-MANAGEMENT` diagnostic naming them. Display (`pdvc`) and output-module colour spaces are not read. | Any FX ACES document writes a 32 bpc OCIO project with "ACES 2.0 Studio v4.0" and `ACES/ACEScg`; `footageOnly` / `perClip` are reported (AE converts graphics too). `--hdr` with an ACES document is rejected (AE chooses HDR output at render time). No output-module colour space is written. | `color_aces_ae_project.aep` (comp 1); `ocio::fixture_tests::native_ocio_aces_project_imports_as_an_fx_aces_document_with_a_camera_input_and_exports_the_project_policy`, `export_document::tests::pr4442_media_cases::input_transform::aces_camera_input_writes_the_footage_ocio_input_colourspace`, `export_document::tests::color_management_aces_exports_an_ocio_aces_project`, `ocio::tests::ocio_declarations_name_their_colourspace` | Native project: AE 26.5x89 opened and resaved the declarations (working space reads `ACES/ACEScg`). Render of our export: see below. |
| Footage OCIO camera input | An `ocsp` camera colourspace of the ACES studio config becomes `source.inputTransform` `cameraInput` (`S-Log3 S-Gamut3.Cine` → `camera:sony-slog3-sgamut3-cine:v1`; nine cameras, `fx_conv::aces`). Display colourspaces (`sRGB - Display`, `Rec.1886 Rec.709 - Display`, the configs' file rules) add nothing: FX's footage interpretation equals them. Other colourspaces are reported. In a non-ACES document the `cameraInput` is removed and reported. | `cameraInput` writes the footage `ocsp` declaration with the studio-config family (`Input/Sony/S-Log3 S-Gamut3.Cine`). A Takeover-routed media layer keeps the omission diagnostic. | same | as above |

Scripting limits (AE 26.5x89, `run_jsx`): `project.colorManagementSystem = 1` and
`ocioConfigurationFile = "ACES 2.0 Studio v4.0"` apply; `workingSpace = "ACEScg"`
does not (it reads `ACES/ACEScg` after the declaration is saved, so the
family-qualified name is the likely setter value, untested); no footage OCIO
interpretation setter exists. The fixture was therefore authored by script
(OCIO config, 32 bpc, chart + in-house S-Log3/S-Gamut3.Cine clip, Lumetri
Exposure +1), its `PwCs` and the clip's `ocsp` were then set in the saved project,
and AE opened and resaved the project natively (`color_aces_ae_resave.jsx`); see the provenance.

All Adobe and FX renders below are local measurement evidence (not committed: no in-repo test consumes them); only the fixture and its tests are in the repository. Measured (8-bit, frame means over the chart):
`render_aep` writes the ACEScg working values (the default output module applies
no view transform), so AE's picture was taken through the same OCIO ACES 2.0 SDR
(Rec.709) view offline (OCIO 2.6, `studio-config-v4.0.0_aces-v2.0_ocio-v2.5`).
| Comparison | Chart (frame 30) | S-Log3 clip (frame 90) |
| --- | --- | --- |
| AE's own OCIO output (OCIO Display Transform: Rec.1886 Rec.709 display, ACES 2.0 SDR 100 nits view, on a measurement copy) vs the OCIO oracle | — | MAD 1.0 when the oracle reads the video-normalized code value ((CV−64)/876), MAD 7.1 on CV/1023: AE decodes legal-range camera video with the video-normalized convention |
| FX render (`tsrct` release `94971429d9`) of the fresh import vs AE's own OCIO output | MAD 2.7, max 13, p99 8 (Lumetri Exposure +1 is ×1.36 scene light in AE; an OCIO ACES project keeps the Exposure stops/2.2 scale in both directions, 0.4545 stops = ×1.37; the display-referred fitted /2.6 of the Lumetri Basic row applies only outside OCIO ACES projects; this ACES scale is an unaccepted approximation from this one chart sample) | MAD 28.7: that renderer build shows the clip as display video (`cameraInput` not applied); not a converter measurement |
| AE render of our `--fps 30` export vs the native reference (both ACEScg output) | frames 0–59 MAD 0.10, max 5 (H.264) | the HEVC clip is not exported by the fresh writer (diagnosed) |
| AE render of our export through the OCIO ACES 2.0 view vs FX render of the source document | MAD 5.0 (8-bit linear output re-encoded offline) | — |

An earlier export left the chart's `ocsp` empty and AE read its sRGB
codes as ACEScg (MAD 60.1 against the native render); the export now writes the
file-rule display declaration. AE's default render route writes ACEScg, so
renders for comparison need the view applied (OCIO Display Transform in a
measurement copy, or offline). AE HDR (PQ) output of an OCIO ACES project:
unrun (no output-module colour space control in the render route).

**Acceptance status: implemented, not accepted.** Both directions are
implemented and structurally tested, and AE's own render of our export matches
the native project. The FX-render gate is not met: the chart differs by MAD 2.72
from AE's own ACES 2.0 output (gate: 2 codes), the S-Log3 clip cannot be compared
until the FX renderer applies `cameraInput`, and HDR output is unrun. The
Premiere ACEScct approximation (see the Premiere ledger) was conditional on
this AE match, so it is not accepted either. `wholeProgram` is the mapping of
AE's actual compositing: AE converts every layer, including solids, text and
effects, into ACEScg, so footage-only would misstate the native picture.

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
| `AE-VERSION` | Producer/version differs from established records; compatibility is uncertain. The message names the producer release decoded from the `head` producer word and the project format. Layouts are measured from native saves of After Effects 25.6 (format 96), 26.0, 26.3 and 26.5 (format 97); a licensed After Effects 17.0 project (format 93, local only) converts all 80 measured compositions with documented diagnostics. |
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
