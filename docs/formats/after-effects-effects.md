# After Effects Effects: control-level conversion support

**Control appendix, not the current proof ledger.** The numeric AR/RGB and
failure rollups below describe the pinned 55-case Adobe26.5x89 execution
checkpoint, not a fresh run on this branch. Later Hue/Saturation Master static
export repair and Vignette fallback have separate evidence in the
[current feature × direction ledger](../after-effects-support.md); an old red
control remains red at its original checkpoint. Independent readback is not
manual Adobe UI inspection; measured RGB is not alpha or fidelity proof.

This page records a **historical 30-type Effect Parade control panel**. Later
export lowering changes can supersede individual columns, particularly Drop
Shadow, Hue/Saturation and Vignette below. Follow the linked current ledger for
what is emitted now. It deliberately
does not summarize an effect as “supported” merely because its native match name
is recognized. The authoritative limitation ledger remains
[After Effects conversion support](../after-effects-support.md#effect-parade--direction-current-converter-checkpoint-in-progress),
and the historical measured run is recorded in
[the execution checkpoint](../after-effects-test-results.md) and
[case receipt](../after-effects-evidence/effects-coverage-results.json).

Implementation references:
[`mapping.rs`](../../crates/aftereffects_file/src/effects/mapping.rs),
[`special.rs`](../../crates/aftereffects_file/src/effects/special.rs),
[import](../../crates/aftereffects_file/src/structure_document/effects.rs),
[export lowering](../../crates/aftereffects_file/src/export_document/effects.rs),
[writer](../../crates/aftereffects_file/src/writer/effects.rs), and the independently
AE-authored [native catalog](../../crates/aftereffects_file/src/effects/definitions.json).

## How to read the evidence column

- **I-S** — pinned Adobe-native source has an editable import structure test in
  `effects::tests::native_controls::native_<effect>_{static,animated}` (or the
  Brightness/Contrast symbols called out below). This proves only the asserted
  editable values/tracks.
- **E-S** — explicit edited-FX input has a fresh-native structure test in
  `export_document::tests::effects_native_coverage::<symbol>`. The inventory
  guard is `every_mapped_type_field_and_animatable_target_has_an_explicit_case`.
- **AR** — independent Adobe 26.5x89 scripting read back the exported controls.
  `AR-fail` and `AR-rejected` are failures, not weaker passes.
- **P8** — stronger, selected export-panel readback from
  [the eight-case receipt](../after-effects-evidence/effects-native-panel.json),
  including native key interpolation where present.
- **RGB-I / RGB-E** — all 60 unique RGB frames were measured for fresh import /
  native export. Every numeric result is **measured, not a quality pass**. RGB
  does not establish alpha. The case receipt contains the actual values.
- **U** — unverified beyond code/catalog inventory.

Except for Vignette, the 55-case panel gives **I-S + E-S + AR + RGB-I +
RGB-E** for a static and compatible animated case. Vignette has pinned I-S,
fresh E-S, and measured RGB-I but no AR or RGB-E. Evidence is case-level: it
does not prove every control in a grouped row is visually isolated. The panel
has 29 static and 26 animated cases. Export structure passed
55/55; import structure passed 52/55; Adobe readback passed 50/55. Hue/Saturation
has two readback failures, and Pixel Motion Blur (two cases) plus Posterize Time
(one case) were rejected by Adobe. No manual Adobe UI inspection, alpha proof,
audio proof, or blanket render-fidelity pass exists.

## Shared conversion and failure rules

- Scalar controls use AE numeric values plus the affine conversions shown below.
  Static values and compatible Constant/Hold/Linear/cubic FX tracks can be
  exported. Tracks are authored from their knots, not frame-baked.
- A native Point or Color property merges component knot times. Linear/Hold is
  representable when components have compatible shared interpolation. Cubic
  Point/Color motion is omitted and the authored base is retained because AE
  uses shared temporal easing for the compound property.
- Static-only controls keep the initial value and diagnose animation or an
  expression. Popup controls described as replacements are changed by replacing
  the whole editable FX payload, not by an `effectProperty` scalar target.
- Duplicate targets, dependent animators, unprepared/unsupported `JsScript`, disabled animators without
  a visible constant, nonfinite values, incompatible component easing, more than
  10,000 merged knots, expressions, malformed native records, and unknown source
  dimensions retain a base/default where possible and emit contextual diagnostics.
- An unlisted native control is not round-tripped. Import diagnoses it; export
  writes the fresh plugin's catalog default unless a feature row records a
  plugin-specific untouched raw state (currently Add Grain). Unknown effects and
  unsupported FX effects are omitted while the owner and convertible siblings remain.
- Coordinate normalization uses the effect owner's **content** width/height.
  Shape/Text effects use the composition-sized native plane. Nested generated-
  precomposition effects retain supported controls and compatible keys, with a
  warning that content-only bounds may clip effect expansion; canvas sizes are
  not enlarged and render fidelity remains unverified. See the
  [nested-export checkpoint](../after-effects-support.md#nested-export-preservation).
  Display names and opaque plugin state are not restored. Eligible layer-local
  scalar scripts are prepared into keys by the separate export adapter before
  native Effect Parade lowering; unsupported scripts are not silently baked.

## Quick index

[Blur and glow](#blur-and-glow) · [Color and channel](#color-and-channel) ·
[Distortion and geometry](#distortion-and-geometry) ·
[Time, edge, matte and texture](#time-edge-matte-and-texture) ·
[Noise, tint and gradients](#noise-tint-and-gradients) ·
[Unmapped families](#unmapped-families)

## Blur and glow

### Gaussian Blur — FX `gaussianBlur` ↔ `ADBE Gaussian Blur 2`

| Control | Import (AE → FX) | Export (FX → AE) | Values, restrictions, omitted/defaulted controls | Evidence |
|---|---|---|---|---|
| Blurriness | `-0001` → `blurriness` | `blurriness` → `-0001` | Pixels; non-negative FX property; fresh FX default `0`. Animatable. | I-S `native_gaussian_blur_*`; E-S `gaussian_*`; AR; RGB-I/E. P8 `scalar-cubic` and `scalar-hold` verify selected keys/interpolation, not pixels. |
| Repeat Edge Pixels | `-0003` → `repeatEdgePixels` | Boolean → `-0003` | Static boolean; default `false`. Animation/expression retains initial value with a diagnostic. | Static I-S/E-S/AR; RGB measured. |
| Blur Dimensions | Not mapped | Catalog default is emitted | Only **Horizontal and Vertical** is modeled. Horizontal-only or vertical-only changes output and is not preserved. | U for nondefault modes. Uniform/repeated-edge panel content is not discriminating radius proof. |

### Glow — FX `glow` ↔ `ADBE Glo2`

| Control group | Import | Export | Values / omissions | Evidence |
|---|---|---|---|---|
| Glow Threshold | AE `-0002` 0…255 → FX `glowThreshold` 0…100% (`×100/255`) | Inverse scaling | Percentage; default `60`; animatable. | I-S `native_glow_*`; E-S `glow_*`; AR; RGB-I/E. P8 `disabled_glow` reads 102. |
| Glow Radius | `-0003` → `glowRadius` | Identity | Pixels, non-negative; default `10`; animatable. | Same case evidence; P8 reads 13. |
| Glow Intensity | `-0004` → `glowIntensity` | Identity | Non-negative AE multiplier; default `1`; animatable. | Same case evidence; P8 reads 2. |
| Enabled state | Native occurrence flag retained | FX record enabled flag authors native instance | Independent from controls. | P8 `export_document::tests::effects_native_panel::disabled_glow` verifies disabled native instance and Adobe render acceptance. |
| Glow Based On, Composite Original, Colors, color looping and other controls | Omitted with mapping warning | Fresh catalog defaults except Operation | Nondefaults can visibly change glow; no general equivalence claim. | U. |
| Glow Operation (`-0006`) | Not represented in FX; native value omitted with warning | Explicit Normal (2), replacing catalog Add (3) | Export-only premultiplied-over approximation; preserves colored source interior better in the private sun probe, but halo color/brightness still differs. Does not change Layer Style glows. | Existing independently authored native fixture defines Add (3); fresh-export own-reader regression, not Adobe-open/readback or scored render proof. [Direction ledger](../after-effects-support.md#glow-operation--export-only-normal-approximation). |

### Directional Blur — FX `directionalBlur` ↔ `ADBE Motion Blur`

| Control | Import | Export | Values / restrictions | Evidence |
|---|---|---|---|---|
| Direction | `-0001` → `direction` | Identity | Degrees, AE convention (0 = up, clockwise); default `0`; animatable. | I-S `native_directional_blur_*`; E-S `directional_*`; AR; RGB-I/E. |
| Blur Length | `-0002` → `blurLength` | Identity | Composition pixels, non-negative; default `0`; animatable. | Same. No algorithm/render-parity claim. |

### Pixel Motion Blur — FX `pixelMotionBlur` ↔ `ADBE OFMotionBlur`

| Control | Import | Export | Values / restrictions | Evidence |
|---|---|---|---|---|
| Shutter Control | `-0001`: 1=`manual`, 2=`automatic`; unknown → automatic + warning | Popup 1/2 | Static. Automatic ignores manual shutter keys. Popup animation/expression retains initial mode. | I-S; E-S. **AR-rejected** for static and animated geometry-motion exports; RGB-I measured, RGB-E unscored. |
| Shutter Angle | `-0002` → `shutterAngle` | Identity | Degrees; FX documents 0…720, default `180`; animatable only has meaning in manual mode. | I-S `native_pixel_motion_blur_*`; E-S `pixel_motion_*`; no successful Adobe export readback. |
| Shutter Samples | `-0003` → `shutterSamples` | Identity | Positive sample count; FX documents clamp 2…64; fresh default `16`; animatable in mapping. | Same rejection limitation. |
| Vector Detail | `-0004` → `vectorDetail` | Identity | 0…100%; default `20`; animatable. | Same rejection limitation; optical-flow parity unknown. |

## Color and channel

### Mosaic — FX `mosaic` ↔ `ADBE Mosaic`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Horizontal Blocks | `-0001` ↔ `horizontalBlocks` | Positive tile count; default `10`; animatable. | I-S `native_mosaic_*`; E-S `mosaic_*`; AR; RGB-I/E. |
| Vertical Blocks | `-0002` ↔ `verticalBlocks` | Positive tile count; default `10`; animatable. | Same. |
| Sharp Colors | `-0003` ↔ static `sharpColors`; bounded root-canvas export normalization | Import retains the boolean (default `false`). Export changes false to on only for proved contained static raw identity-affine Rect/Path geometry beneath an ungated TwoD root canvas Adjustment. TwoD owner geometry is inert; ThreeD owners, including z=0, are excluded because depth sorting can change the backdrop. Other domains retain the checkbox with an unsupported-domain diagnostic: native averaging is not faithful to FX centers. Animated checkbox omitted. | Static I-S/E-S/AR; `mosaic_root_canvas_normalizes_native_backed_controls_without_changing_keys_or_bypass` checks edited-FX eligible controls; `mosaic_unproven_leaf_retains_checkbox_counts_clocks_and_scales` and domain exclusions check preservation. Independent eligible AE export RGB/alpha and animated-domain mapping remain unproved. |

### Shift Channels — FX `shiftChannels` ↔ `ADBE Shift Channels`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Take Red/Green/Blue From | `-0002/-0003/-0004` ↔ own channel, `fullOn` (9), or `fullOff` (10) | Static popup routes only. Defaults red/green/blue. Cross-channel, luma, hue and other routes retain own channel with warning because the FX backend cannot render them. | I-S `native_shift_channels_static`; E-S `shift_channels_static`; AR; RGB-I/E measured. No animated case. |
| Take Alpha From | Import: native `-0001` is consumed only to diagnose; export: catalog default | Alpha routing is not represented; nondefault input becomes unchanged alpha. | U for alpha fidelity. |

### Drop Shadow effect — FX `dropShadow` ↔ `ADBE Drop Shadow`

The independent source tested an **Effect Parade** plugin on import. Current
FX → AEP export instead canonicalizes the shared FX DropShadow payload to a
**native Layer Style**; the plugin-export columns below are historical panel
expectations, **not the current writer contract**. The executable
`export_document::tests::effects_native_coverage::drop_shadow_static` assertion
therefore checks the exact Layer Style payload and contextual canonicalization
diagnostic while retaining the historical plugin oracle for provenance and
descriptive comparison, not semantic validation. It does not replace the
opposite-direction Effect Parade import assertion
`effects::tests::native_controls::native_drop_shadow_static` or turn historical
plugin readback into Layer Style export proof. See the
[current Layer Styles direction](../after-effects-support.md) and its
[per-style evidence](../after-effects-evidence/layer-styles.md).

| Control group | Import | Historical plugin-export expectation (superseded) | Values / restrictions | Historical evidence |
|---|---|---|---|---|
| Color + opacity | Native RGB `-0001`; opacity `-0002` supplies FX color alpha (`opacity/255`) | FX RGB → `-0001`; alpha ×255 → `-0002` | Static initial values only. Native color alpha is not independently preserved; combined-alpha fidelity unverified. | I-S `native_drop_shadow_static`; E-S `drop_shadow_static`; AR; RGB-I/E. |
| Direction + Distance / Offset | Static polar values `-0003/-0004` → `[distance·sin θ, -distance·cos θ]` pixels | FX `[x,y]` → direction `atan2(x,-y)` degrees and Euclidean distance | Static only; independent polar animation cannot be represented as affine Vec2 motion. Default FX offset `[0,0]`. | Same static structural/readback evidence; no animation case. |
| Softness / blur radius | `-0005` → non-negative `blurRadius` | Identity to `-0005` | Pixels; static initial value; default `0`. | Same. |
| Effect enabled | Native occurrence enabled → FX record | FX effect and payload enabled state combine | Default true. | Structure evidence; not alpha/composite proof. |
| Shadow Only | Consumed; nonzero diagnosed and normal composite retained | Catalog default | Unsupported. | U. |
| Spread / blend mode | No native separate spread | Nonzero `spreadRadius` omitted; non-`normal` blend exported as Normal | Changed shadow/compositing. | Explicit `shadow_payload_disabled_state_and_unsupported_blend_are_not_lost`; no Adobe fidelity proof. |

### Brightness & Contrast — FX `brightnessContrast` ↔ `ADBE Brightness & Contrast 2`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Brightness | `-0001` ↔ `brightness` | AE units −150…150; default 0; animatable. | Animated I-S passes. **Static I-S fails:** authored 20 imports as 0 in `effects::tests::coverage::native_brightness_contrast_static_keeps_signed_editable_controls`. E-S `brightness_*` and AR pass. RGB-I/E measured, not pass. |
| Contrast | `-0002` ↔ `contrast` | AE units −100…100; default 0; animatable. | `native_brightness_contrast_animation_keeps_distinct_linear_tracks`; E-S/AR pass; RGB measured. |
| Legacy/HDR mode | Import: omitted; export: fresh default | Color math can differ. | U. Export pilot has correct keys/readback but pixels are shifted −60/−40 and rendered animation freezes; this remains a failure. |

### Hue/Saturation — FX `hueSaturation` ↔ `ADBE HUE SATURATION`

**Historical panel, not current key support:** these `I-S` and `AR` failures
remain failures of the pinned panel. A later static signed-Master import assertion
and **static integer Master export repair** are recorded separately in the
[current ledger](../after-effects-support.md) and
[repair evidence](../after-effects-evidence/hue-master-export-repair.md).
Animated Master controls and the Colorize toggle are omitted on export with
base values retained; the `intended animatable` wording below is historical.

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Master Hue | `-0004` ↔ `hue` | Degrees −180…180; default 0; intended animatable. | **I-S fails** static and animated (`native_hue_saturation_static`, `native_hue_saturation_animated`): authored 15 reads 0. **AR-fail:** static expected 15 got 0; animated expected keys/samples got 0/no keys. RGB measured, not pass. |
| Master Saturation | `-0005` ↔ `saturation` | AE units −100…100; default 0; intended animatable. | Same I-S/AR failures (static expected 20; animated keys missing). |
| Master Lightness | `-0006` ↔ `lightness` | AE units −100…100; default 0; intended animatable. | Same failures (static expected −10; animated keys missing). |
| Colorize enabled | `-0007` ↔ `colorize` | Boolean; default false; mapping intends animation. | AR animated expected keys but got none. |
| Colorize Hue | `-0008` ↔ `colorizeHue` | Degrees 0…360; default 0; inert unless colorize; intended animatable. | E-S exists; source can key this leaf, but the case is not a full master-animation oracle. No successful blanket claim. |
| Colorize Saturation / Lightness | `-0009/-0010` ↔ respective controls | AE units; defaults 0; inert unless colorize; intended animatable. | Same limitation. |
| Individual color ranges | Import: omitted; export: fresh defaults | Only Master channel is represented. | U. Four native scripting master leaves could not be keyed in the independent source. |

### Radial Blur — FX `radialBlur` ↔ `ADBE Radial Blur`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Amount | `-0001` ↔ `amount` | Native spin amount; useful FX range roughly 0…50; default 0; animatable. | I-S `native_radial_blur_*`; E-S `radial_*`; AR; RGB-I/E. |
| Center X/Y | Point `-0002` pixels ↔ normalized `[x/width,y/height]` | Content-normalized; default 0.5/0.5. Linear/Hold split component knots merge; cubic Point omitted. | Same plus P8 `radial_point_offset_knots`: three native keys; max sampled point deviation 0.000311 px. |
| Type/quality/random seed | Import: omitted; export: fresh defaults | Only default Spin mode; Zoom is not represented. | U. |

### Levels — FX `levels` ← `ADBE Easy Levels2` / → `ADBE Pro Levels2`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Input Black / White | Easy `-0003/-0004` or Pro `-0004/-0005`, native 0…1 ↔ FX 0…255 | Defaults 0/255; animatable. Export intentionally changes plugin identity to Pro Levels Individual Controls. | I-S `native_levels_*`; E-S `levels_*`; AR; RGB-I/E. P8 `levels_normalized` reads 0.2/0.8. |
| Gamma | Easy `-0005` or Pro `-0006` ↔ `gamma` | Midpoint exponent, approximately 0.1…10; default 1; animatable. | Same; P8 reads 1.5. |
| Output Black / White | Easy `-0006/-0007` or Pro `-0007/-0008`, native 0…1 ↔ FX 0…255 | Defaults 0/255; animatable. | Same; P8 reads 0.1/0.9. |
| Per-channel RGB, alpha and clipping | Import: omitted; export: fresh Pro defaults | Easy Levels overrides were ignored by Adobe, hence Pro export. Original plugin identity is not preserved. | Nondefault master readback exists; samples do not isolate every level control; no fidelity pass. |

### Exposure — FX `exposure` ↔ `ADBE Exposure2`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Master Exposure | `-0003` ↔ `exposure` | Native units; default 0; animatable; no converter clamp documented. | I-S `native_exposure_*`; E-S `exposure_*`; AR; RGB-I/E. P8 reads 1.25. |
| Master Offset | `-0004` ↔ `offset` | Native units; default 0; animatable. | Same; P8 reads 0.125. |
| Master Gamma Correction | `-0005` ↔ `gammaCorrection` | Native units; default 1; animatable. | Same; P8 reads 0.8. |
| Per-channel and linear-light bypass | Import: omitted; export: fresh defaults | Not represented. | U. |

### Vibrance — FX `vibrance` ↔ `ADBE Vibrance`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Vibrance | `-0001` ↔ `vibrance` | Native units; default 25; animatable; no converter clamp documented. | I-S `native_vibrance_*`; E-S `vibrance_*`; AR; RGB-I/E. Shader color math is approximate. |
| Saturation | `-0002` ↔ `saturation` | Native units; default 0; animatable. | Same. |

## Distortion and geometry

### Bulge — FX `bulge` ↔ `ADBE Bulge`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Center X/Y | Point `-0003` pixels ↔ content-normalized X/Y | Defaults 0.5/0.5; animatable; compound Point restrictions apply. | I-S `native_bulge_*`; E-S `bulge_*`; AR; RGB-I/E. Off-center source is not fully visually discriminating. |
| Horizontal / Vertical Radius | `-0001` ÷ width; `-0002` ÷ height | Content-normalized, defaults 0.5/0.5; animatable. | Same. |
| Bulge Height | `-0004` ↔ `bulgeHeight` | Negative pinches, positive bulges; useful ~−4…4, not clamped; default 0; animatable. | Same. |
| Pinning | `-0007` ↔ boolean | Default false; animatable scalar mapping. | Same. |
| Taper / antialiasing | Import: omitted; export: fresh defaults | Output can differ. | U. |

### Corner Pin — FX `cornerPin` ↔ `ADBE Corner Pin`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Upper Left | Point `-0001` pixels ↔ `upperLeftX/Y` normalized by content size | Identity default `(0,0)`; out-of-bounds values allowed; animatable with compound Point restrictions. | I-S `native_corner_pin_*`; E-S `corner_pin_*`; AR; RGB-I/E. |
| Upper Right | `-0002` ↔ `upperRightX/Y` | Identity default `(1,0)`; same restrictions. | Same. |
| Lower Left | `-0003` ↔ `lowerLeftX/Y` | Identity default `(0,1)`; same restrictions. | Same. |
| Lower Right | `-0004` ↔ `lowerRightX/Y` | Identity default `(1,1)`; same restrictions. | Same. Compositor/resampling parity unverified. |

### Motion Tile — FX `motionTile` ↔ `ADBE Tile`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Tile Center X/Y | Point `-0001` pixels ↔ normalized X/Y | Defaults 0.5/0.5; animatable; Point restrictions apply. | I-S `native_motion_tile_*`; E-S `motion_tile_*`; AR; RGB-I/E. |
| Tile Width / Height | `-0002/-0003` ↔ FX | AE percent; 100 = source size; defaults 100; animatable. | Same. |
| Output Width / Height | `-0004/-0005` ↔ FX | AE percent; defaults 100; animatable. | Same. |
| Mirror Edges | `-0006` ↔ boolean | Default false; animatable scalar mapping. | Same. |
| Phase | `-0007` ↔ `phase` | Degrees; 360 = one tile; default 0; animatable. | Same. |
| Horizontal Phase Shift | Import: omitted; export: fresh default | Not represented. | U. |

### Twirl — FX `twirl` ↔ `ADBE Twirl`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Angle | `-0001` ↔ `angle` | Degrees; fresh FX default 120; animatable. | I-S `native_twirl_*`; E-S `twirl_*`; AR; RGB-I/E. P8 `twirl_center_and_angle` reads selected static value. |
| Radius | Native `-0002` percent ×0.01 ↔ FX normalized | Default 0.5; animatable. | Same; P8 static readback. |
| Center X/Y | Point `-0003` pixels ↔ normalized content coordinates | Defaults 0.5/0.5; compound Point restrictions. | Same; P8 and separate pinned static-Point regression. Shader geometry approximates AE. |

### Ripple — FX `ripple` ↔ `ADBE Ripple`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Amplitude | Native `-0006` pixels ÷ content width ↔ normalized `amplitude` | Default 0.03; animatable. **Import approximation:** after the ordinary lowering, including AE-evaluated expression fitting, the retained amplitude and every emitted amplitude key are scaled by one factor when the largest exceeds the visual strength limit `amplitude × frequency ≤ 1.25`, 25% beyond the FX ring fold-over threshold of 1. It is not a native unit conversion, and rings can still fold slightly near that limit. Key selection, times, easing, center, phase and frequency are retained; overshooting easing between keys is not bounded. Export is the unchanged unit inverse. See the [support ledger](../after-effects-support.md#ripple-amplitude-strength-limit--import-approximation). | I-S `native_ripple_*`; E-S `ripple_*`; AR; RGB-I/E. Selected source is weak visual discrimination; both coverage cases are within the limit and unchanged. Limit regressions: `native_ripple_amplitude_is_limited_only_beyond_the_fx_strength_limit` and the Ripple import-path cases in `structure_document::effects::tests`. |
| Center X/Y | Point `-0002` pixels ↔ normalized content coordinates | Defaults 0.5/0.5; animatable; Point restrictions. | Same. |
| Phase | `-0007` degrees ↔ FX radians (`×π/180`) | Default 0; animatable. | Same. |
| Wavelength / frequency | Static native `-0005` wavelength ↔ `frequency = 2π·width/wavelength` | Positive wavelength and width required; FX default frequency 30. Keyed wavelength is omitted because reciprocal interpolation is nonlinear. | Static value structure/readback only. |
| Speed, radius, conversion mode | Import: omitted; export: fresh defaults | Motion/shape can differ. | U. |

### Wave Warp — FX `waveWarp` ↔ `ADBE Wave Warp`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Wave Height | Native `-0002` pixels ÷ content height ↔ normalized `waveHeight` | Default 0.03; animatable. | I-S `native_wave_warp_*`; E-S `wave_warp_*`; AR; RGB-I/E. |
| Wave Width / wavelength | Static native `-0003` wavelength ↔ `waveWidth = width/wavelength` | Default 6; positive values required; keyed wavelength omitted. | Static mapping within cases; no animated target. |
| Direction | `-0004` ↔ `direction` | Degrees; default 90; animatable. | Same case evidence. |
| Phase | `-0007` degrees ↔ FX radians | Default 0; animatable. | Same. |
| Wave Type, Speed, Pinning, Antialiasing | Import: omitted; export: fresh defaults | Shader warp is approximate; native source includes unmapped intrinsic speed. | U for omitted controls; recorded RGB is not a pass. |

## Time, edge, matte and texture

### Posterize — FX `posterize` ↔ `ADBE Posterize`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Levels | `-0001` ↔ `levels` | Color-level count; default 6; animatable; no converter clamp documented. | I-S `native_posterize_*`; E-S `posterize_*`; AR; RGB-I/E. Transfer/quantization parity unverified. |

### Posterize Time — FX `posterizeTime` ↔ `ADBE Posterize Time`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Frame Rate | Initial `-0001` ↔ `frameRate` | FPS; default 8; **static only**. FX retimes the whole layer clock, unlike AE's effect-stack-local timing; effect order/visibility can differ. | I-S `native_posterize_time_static`; E-S `posterize_time_static`. **AR-rejected** with intrinsic geometry motion; RGB-I measured, RGB-E unscored. |

### Vignette — FX `vignette` ↔ `CS Vignette` (CC Vignette)

**Current AEP → FX import omits CC Vignette as unsupported**, with a named
warning and no substitute FX radial falloff. The kernels are not equivalent;
owner, masks and other effects remain. The table's `compatible keyframes`
describe historical structural expectations, not current import support.
Current FX → AEP export diagnoses animated CC Vignette,
omits the keys and retains the authored static base: Adobe continuous rendering
froze keyed controls. No current animated-export/readback/RGB pass is claimed.
See the [current ledger](../after-effects-support.md) for the direction boundary.

| Control | Import / historical export expectation | Values / restrictions | Evidence |
|---|---|---|---|
| Amount | Native `-0001` percent ×0.01 ↔ FX `amount` | Import keys supported; historical export intended compatible keys. **Current export: static base ×100 only; animation omitted with diagnosis**. | I-S `native_vignette_*`; E-S `vignette_*`; RGB-I measured. No AR/RGB-E. |
| Angle of View / radius | Native `-0002` ÷60 ↔ FX `radius` | Bounded linear approximation, not equivalent kernel semantics. **Current export: static base ×60; animation omitted with diagnosis**. | Same structural/import-render evidence. |
| FX feather | No native control; import uses `0.35` | Authored export value is omitted with a diagnostic. | E-S omission diagnostic; no Adobe proof. |
| Native Center / Pin Highlights | Nondefault or animated import controls are diagnosed and omitted | Fresh export uses centered `[width/2,height/2]` and zero canonical defaults, with an explicit replacement diagnostic. | I-S diagnostics; E-S reads `[60,40]` and `0` from a 120×80 owner. Alpha/highlight fidelity unverified. |

### Find Edges — FX `findEdges` ↔ `ADBE Find Edges`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Invert | `-0001` ↔ numeric `invert` | Native numeric units; default 1; animatable (not a Boolean coercion). | I-S `native_find_edges_*`; E-S `find_edges_*`; AR; RGB-I/E. |
| Blend With Original | Import: omitted; export: fresh default | Not represented. | U. |

### Sharpen — FX `sharpen` ↔ `ADBE Sharpen`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Amount | `-0001` ↔ `amount` | Native units; default 40; animatable; no converter clamp documented. | I-S `native_sharpen_*`; E-S `sharpen_*`; AR; RGB-I/E. Selected content weakly discriminates sharpness; shader is approximate. |

### Luma Key — FX `lumaKey` ↔ `ADBE Luma Key`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Threshold | Native `-0002` 0…255 ÷255 ↔ normalized `threshold` | Default 0.3; animatable. | I-S `native_luma_key_*`; E-S `luma_key_*`; AR; RGB-I/E. Alpha remains unverified. |
| Tolerance / softness | Native `-0003` percent ×0.01 ↔ normalized `softness` | Default 0.1; animatable. | Same. |
| Key Type | Import: only Luma accepted; export: fresh Luma default | Alternate key types unsupported. | U. |
| FX invert | Import: no mapped control; export: native default, no animated target | FX default 0; not part of the native mapping. | U. |
| Edge Thin / Edge Feather | Import: omitted; export: fresh defaults | Matte differs. | U. |

### Simple Choker — FX `simpleChoker` ↔ `ADBE Simple Choker`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Choke Matte | `-0002` ↔ `choke` | Pixels; FX renderer clamps approximately −10…10; default 1; animatable. | I-S `native_simple_choker_*`; E-S `choker_*`; AR; RGB-I/E. |
| Matte view mode | Import: omitted; export: fresh default | Not represented. | U. |

### Grain — FX `grain` ↔ `VISINF Grain Implant`

| Control | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Intensity | Native `-0008` ↔ persisted FX `amount`; animator target is `intensity` | Native Add Grain units; default amount 1; animatable. | I-S `native_grain_*`; E-S `grain_*`; AR; RGB-I/E. Target-name distinction is tested by `catalog::tests::grain_uses_uniform_name_not_persisted_field_name`. |
| Size | `-0007` ↔ `size` | Native units; default 1; animatable. | Same. |
| Softness | `-0130` ↔ `softness` | Native units; default 1; animatable. | Same. |
| Aspect Ratio | `-0030` ↔ `aspectRatio` | Native units; default 1; animatable. | Same. |
| Seed | `-0013` ↔ `seed` | Native numeric seed; default 0; animatable. | Same. |
| Channels, application, matching, masking, preview region and sub-settings | Import: omitted. Export: untouched raw plugin state; Preview mode keeps its owner-relative default region and guide box. | AE's scripting/descriptor defaults are not the values stored by an untouched Add Grain instance. Authoring those UI defaults into the instance selected the wrong output mode after the FX owner was cropped to a 120×80 native precomposition. Other omitted controls remain uneditable. | Native-source CPU regression for static/animated owner/control state; historical RGB-E remains measured low and was not rerun. Alpha unverified. |

## Noise, tint and gradients

### Tint — FX `tintTritone` ↔ `ADBE Tint`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Map Black To RGB | Color `-0001` RGB ↔ `blackR/G/B` | Normalized 0…1; defaults 0/0/0; animatable as one native Color; alpha omitted. Cubic Color motion retains base only. | I-S `native_tint_tritone_*`; E-S `tint_*`; AR; RGB-I/E. |
| Map White To RGB | Color `-0002` RGB ↔ `whiteR/G/B` | Normalized 0…1; defaults 1/1/1; same compound Color restrictions. | Same. |
| Amount to Tint | `-0003` ↔ `amount` | Percent; default 100; animatable. | Same. |
| Alpha / tritone midtone | Import: not mapped; export: fresh defaults, no midtone mapping | FX type name does not imply AE Tritone equivalence. | U. |

### Gradient Ramp — FX `gradientRamp` ↔ `ADBE Ramp`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Start Point X/Y | Point `-0001` pixels ↔ normalized `startX/Y` | Defaults 0/0; animatable; Point restrictions. | I-S `native_gradient_ramp_*`; E-S `ramp_*`; AR; RGB-I/E. P8 `ramp_rgba_split_knots` verifies selected split-knot export. |
| End Point X/Y | Point `-0003` pixels ↔ normalized `endX/Y` | Defaults 1/1; same restrictions. | Same. |
| Start Color RGB | Color `-0002` ↔ `startR/G/B` | Normalized 0…1; default black; shared Color interpolation restrictions; alpha omitted. | Same; P8 selected Linear RGB knot union. |
| End Color RGB | Color `-0004` ↔ `endR/G/B` | Normalized 0…1; default white; same restrictions. | Same. |
| Ramp Shape | Native popup `-0005` 1=linear, 2=radial ↔ FX 0/1 (`native−1`) | Numeric mapping is marked animatable; default linear. Other popup values are not established. | I-S/E-S/AR at selected values; no blanket popup proof. |
| Blend With Original / FX blend | Native normalized blend `-0007` ↔ FX `1−native` | Default FX 1 = full ramp; animatable. | I-S/E-S/AR; panel content is not fully visually discriminating. |
| Scatter and color alpha | Import: omitted; export: fresh defaults | Alpha/render parity unverified. | U. |

### Turbulent Noise — FX `turbulentNoise` ↔ `ADBE AIF Perlin Noise 3D`

| Control group | Import / export | Values / restrictions | Evidence |
|---|---|---|---|
| Fractal Type | Static popup `-0001` maps 1 basic, 3 turbulentSmooth, 4 turbulentBasic, 5 turbulentSharp, 11 max, 14 rocky, 15 cloudy, 18 strings | Whole-effect replacement; default basic. Unknown ordinal retains basic + warning. No scalar animator. | I-S static; E-S; AR; RGB-I/E. Proprietary modes are approximations. |
| Noise Type | Static popup `-0002`: 1 block, 2 linear, 3 softLinear, 4 spline | Whole-effect replacement; default softLinear. Unknown ordinal retains default + warning. | Same. |
| Invert | `-0003` ↔ numeric `invert` | Default 0; animatable; not Boolean-coerced. | I-S `native_turbulent_noise_*`; E-S `noise_*`; AR; RGB-I/E. |
| Contrast / Brightness | `-0004/-0005` ↔ controls | Native units; defaults 100/0; animatable; no converter clamp documented. | Same. |
| Rotation | `-0008` ↔ `rotation` | Native angular units; default 0; animatable. | Same. |
| Uniform Scale | `-0010` ↔ `scale` | Native percent-like units; default 100; animatable. Nonuniform scale omitted. | Same. |
| Offset Turbulence X/Y | Point `-0013` → width-percent coordinates with centered origin; inverse on export | `offsetX = nativeX·100/width−50`; `offsetY = nativeY·100/width−50·height/width`; defaults 0/0. Positive width required; Point restrictions apply. | Same structural/readback evidence; coordinate caveats apply. |
| Complexity | `-0015` ↔ `complexity` | Native units; default 6; animatable. | Same. |
| Sub Influence | `-0017` ↔ `subInfluence` | Native units; default 70; animatable. | Same. |
| Evolution | `-0020` ↔ `evolution` | Native units; default 0; animatable. | Same. |
| Scalar Blend | `-0025` percent ×0.01 ↔ `blend` | Default 1; **static only**. Animation retains initial value. | Static readback/structure; no animated target. |
| Native Blending Mode | Import: `-0026` consumed, only ordinal 2 accepted; export: fresh default | Other modes cannot be expressed by scalar FX blend. | U for nondefault modes. |
| Overflow and remaining controls | Import: omitted; export: fresh defaults | Shader noise is not Adobe noise. | RGB measured, explicitly not a fidelity pass. |

## Families outside the historical panel

These rows are intentionally separate from the historical control inventory.
TemperatureTint's later export-only approximation is documented explicitly;
recognition as an FX variant or similarity of a product name alone is not a
native mapping for the remaining unmapped families.

| Family | Import | Export | Handling / evidence |
|---|---|---|---|
| FX `TemperatureTint` | Per-channel Exposure records are not reconstructed as TemperatureTint; existing master-only import diagnostics remain | Export-only editable approximation: two ordered native `ADBE Exposure2` Individual Channels instances project Temperature into R/B offsets ±0.0012×temperature and Tint into R/B offsets −0.0006×tint and G offset 0.0012×tint, with neutral Master controls. Independent supported scalar keys and bypass are retained. | Generated control/key structure tests, not independently measured white-balance or RGB/alpha equivalence. Native colour space, premultiplication and intermediate clipping may differ. See the [current TemperatureTint ledger](../after-effects-support.md#temperaturetint-edited-export-approximation). Not included in the historical 30-type panel. |
| FX `LensDistortion`, `ChromaticAberration`, `Fisheye` | An AE effect with another match name is diagnosed, not guessed | Omitted; owner and siblings retained | No verified match/control mapping. Optics Compensation is **not** claimed as Lens Distortion/Fisheye. U. |
| `PersonMatte`, `DepthMatte` | No native effect equivalent | Omitted | ML-generated matte source cannot be represented as an editable native effect. U. |
| `LookTransform`, `PrimaryGrade`, `ColorCurves` | No arbitrary native grade/curve replay | Omitted | Reader-specific color operations. U. |
| `CustomShader` | No WGSL import, except the bounded Keylight 906 profile below | Entire rendering owner omitted, including its fill and any other effects; shader-free children/siblings retained. Diagnostic: `owner omitted: CustomShader-rendered layer`. No baking or generated script. | Dynamic parameter catalog has no verified native equivalent. U. |
| AE `Keylight 906` | Bounded static profile → one converter-owned `customShader` with editable Screen Colour, Screen Gain, Screen Balance and Clip White; sparse controls use recorded plugin defaults; other configurations are omitted with a reason | Omitted as `CustomShader` | SDR approximation of documented behaviour; see the [Keylight ledger entry](../after-effects-support.md#keylight-906-bounded-profile--import-only-customshader-approximation). U for export and Adobe fidelity. |
| Unknown `Unsupported` payload | Preserved only by tolerant FX reader, not converted | Omitted with contextual diagnostic | Owner and supported siblings remain. U. |
| FX Layer Styles: `OuterGlow`, `Stroke`, `GradientOverlay`, `InnerShadow`, `InnerGlow`, `Satin`, `BevelEmboss` | Not covered by this historical Effect Parade panel | Not covered by this historical Effect Parade panel | Current native Layer Style mappings and limitations are tracked in the [direction ledger](../after-effects-support.md) and [per-style evidence](../after-effects-evidence/layer-styles.md). `DropShadow` is shared with the effect model, so its direction-specific distinction is documented above. |

## Exact test and receipt map

- Import static/animated symbols are enumerated in
  [`effects/tests/native_controls.rs`](../../crates/aftereffects_file/src/effects/tests/native_controls.rs).
  Brightness/Contrast uses
  `effects::tests::coverage::native_brightness_contrast_static_keeps_signed_editable_controls`
  and `native_brightness_contrast_animation_keeps_distinct_linear_tracks`.
- Export case symbols are enumerated in
  [`export_document/tests/effects_native_coverage.rs`](../../crates/aftereffects_file/src/export_document/tests/effects_native_coverage.rs).
  Control completeness is guarded by
  `every_mapped_type_field_and_animatable_target_has_an_explicit_case`.
- Generic owner/sibling, disabled-state, animation, Point/Color and omission
  behavior is covered by
  [`export_document/tests/effects.rs`](../../crates/aftereffects_file/src/export_document/tests/effects.rs)
  and `effects_edge_coverage.rs`; those are structural tests, not Adobe proof.
- The checked-in evidence JSON records exact source/composition IDs, Asset and
  source hashes, FX/export hashes, test symbols, Adobe readback failures,
  per-case RGB scores and limitations. Historical receipts describe a past run;
  they are not a fresh result for the current branch.
