# After Effects effect controls

This reference describes the mapped control families, not complete native plugin
support. Use [AE support](../after-effects-support.md) for layers, media, clocks,
text/styles and failure handling. Source mappings live in
[the effect adapter](../../crates/aftereffects_file/src/effects/), with fresh-native
lowering in [export effects](../../crates/aftereffects_file/src/export_document/effects.rs).
The [native catalog](../../crates/aftereffects_file/src/effects/definitions.json)
records plugin identities/defaults.

## Shared rules

- Export writes fresh controls from current FX values, not the original plugin
  instance, UI caches or opaque payloads. Unmapped controls use defaults or
  diagnosed replacements; unknown effects are not guessed by display name.
- Static support does not imply animation. Compatible scalar tracks can preserve
  knots/easing; popup/boolean controls and nonlinear unit conversions have narrower
  rules. Unsupported animation can retain the base with diagnostics.
- Native Point/Color properties couple component clocks and interpolation.
  Compatible components can merge; incompatible cubic/compound tracks may be
  omitted or approximated. Independent scalar support is not arbitrary vector support.
- Pixel/normalized coordinates depend on the owner's content plane. Generated
  precompositions, source origins, masks and effect expansion can change bounds.
  Numeric agreement does not prove clipping or pixel agreement.
- Read diagnostics for whether a property, effect or rendering owner is omitted.
  Coverage-changing effects cannot always be safely dropped while showing the source.

## Blur and glow

| FX / native effect | Mapped controls and units | Important limits |
| --- | --- | --- |
| `gaussianBlur` / `ADBE Gaussian Blur 2` | Blurriness in pixels; static Repeat Edge Pixels. | Both-axis blur profile; one-axis modes and animated repeat flags are not general support. FX radius/kernel and edge sampling differ. |
| `glow` / `ADBE Glo2` | Threshold converts native 0–255 to FX percent; radius in pixels; intensity multiplier and enabled state. | Unmapped color/composite controls differ. Export uses a Normal-operation approximation rather than assuming catalog Add matches FX. |
| `directionalBlur` / `ADBE Motion Blur` | Direction in degrees, blur length in composition pixels. | Native kernel/plane and blur extent are not equivalent by matching numbers. |
| `pixelMotionBlur` / `ADBE OFMotionBlur` | Manual/automatic shutter mode, shutter angle, samples and vector detail. | Optical flow is not general parity; historical geometry-motion exports were rejected by Adobe despite passing structure tests. |

## Color and channel

| FX / native effect | Mapped controls and units | Important limits |
| --- | --- | --- |
| `mosaic` / `ADBE Mosaic` | Integer horizontal/vertical block counts and Sharp Colors. | Bounds-dependent normalization/approximation has narrow admission. Grid origin, source clipping and animated geometry can differ. |
| `shiftChannels` / `ADBE Shift Channels` | Static own-channel, Full On and Full Off routes for RGB. | Cross-channel/derived routes and alpha routing are not general mappings. |
| `brightnessContrast` / `ADBE Brightness & Contrast 2` | Brightness/contrast native scalar units. | Legacy/HDR mode and kernel differences remain; historical control/render failures are recorded separately. |
| `hueSaturation` / `ADBE HUE SATURATION` | Bounded signed Master values and selected Colorize controls. | Static integer Master export is supported; animated Master/Colorize-toggle controls retain diagnosed base fallback. Individual color ranges are not generally preserved. |
| `levels` / Easy Levels import, Pro Levels export | Master input/output black/white convert native 0–1 to FX 0–255; gamma is the midpoint exponent. | Export changes plugin identity; per-channel/alpha/clipping combinations are not a general Levels round trip. |
| `exposure` / `ADBE Exposure2` | Master exposure, offset and gamma correction. | Per-channel/linear-light policy is not implied; color space and clipping can differ. |
| `vibrance` / `ADBE Vibrance` | Vibrance and saturation in native scalar units. | FX color math is approximate. |
| `tintTritone` / `ADBE Tint` | Black/white RGB endpoints and percent Amount to Tint. | The FX name does not imply AE Tritone or midtone support. Color alpha and incompatible component easing can be lost. |
| `gradientRamp` / `ADBE Ramp` | Normalized start/end points, RGB colors, linear/radial shape and blend. | Native blend inverts to FX `1 − native`; scatter, color alpha and unsupported coupled animation differ. |

### Drop Shadow and Layer Styles

Effect Parade `ADBE Drop Shadow` import maps static color/opacity, polar direction
and distance to a pixel offset, softness and enabled state. Shadow Only and other
unrepresented controls diagnose loss. Polar animation is not equivalent to
independent affine offset interpolation.

Current FX DropShadow export canonicalizes to a **native Layer Style**, not a
replayed Drop Shadow plugin. This changes native phase/identity. Nonzero spread,
blend behavior, style order and compatible animation have their own limitations.
Historical plugin readback does not prove this style export. See
[per-style controls and evidence](../after-effects-evidence/layer-styles.md).

### Temperature and Tint

Export has a bounded editable replacement using two ordered native Exposure
Individual Channels instances, with supported scalar keys and bypass. Import does
not reconstruct this pair as a Temperature/Tint effect. This is an approximation,
not independently verified white balance; clipping, color space and intermediate
premultiplication can differ.

## Distortion and geometry

| FX / native effect | Mapped controls and units | Important limits |
| --- | --- | --- |
| `bulge` / `ADBE Bulge` | Center/radii normalized to content size, height and pinning. | Taper/antialiasing and compound Point animation have narrower support. |
| `cornerPin` / `ADBE Corner Pin` | Four content-normalized corner points, including admitted off-frame values. | Coupled Point tracks, geometry, sampling and owner-plane restrictions still apply. |
| `motionTile` / `ADBE Tile` | Normalized center, tile/output dimensions in percent, mirror edges and phase in degrees. | Horizontal Phase Shift and unsupported wrap/expansion combinations are not retained. |
| `twirl` / `ADBE Twirl` | Angle in degrees, radius from native percent, content-normalized center. | Editable geometric approximation; resampling and native owner bounds can differ. |
| `ripple` / `ADBE Ripple` | Pixel amplitude divided by content width; center normalized; phase degrees → radians; static wavelength becomes frequency. | Import can reduce amplitude to the FX strength limit with a diagnostic. Keyed reciprocal wavelength and unrepresented native speed/radius/modes are lost. |
| `waveWarp` / `ADBE Wave Warp` | Height divided by content height; static width/wavelength relation; direction and phase. | Native wave type/speed/pinning/antialiasing and keyed reciprocal wavelength are not generally represented. |

## Time, matte and texture

| FX / native effect | Mapped controls and units | Important limits |
| --- | --- | --- |
| `posterize` / `ADBE Posterize` | Scalar color-level count. | Transfer/quantization behavior is not proved equal. |
| `posterizeTime` / `ADBE Posterize Time` | Static FPS. | FX can retime the whole layer rather than one effect-stack stage. Historical geometry-motion exports failed Adobe acceptance. |
| `findEdges` / `ADBE Find Edges` | Numeric Invert control. | Not a boolean coercion; Blend With Original and kernel behavior are not generally mapped. |
| `sharpen` / `ADBE Sharpen` | Scalar amount. | Approximate kernel; some references weakly discriminate the control. |
| `lumaKey` / `ADBE Luma Key` | Threshold native 0–255 → normalized FX; tolerance percent → softness. | Alternate key types, alpha-edge controls and invert are not general support; alpha fidelity is unproved. |
| `simpleChoker` / `ADBE Simple Choker` | Choke in pixels. | FX renderer limits visible radius; native matte-view modes and broad alpha fidelity are not established. |
| `grain` / `VISINF Grain Implant` | Amount, size, softness, aspect ratio and seed. | Persisted FX field is `amount`, animator target is `intensity`. Untouched native preview/application state and algorithm can visibly differ. |
| `turbulentNoise` / `ADBE AIF Perlin Noise 3D` | Selected static fractal/noise modes; invert, contrast, brightness, rotation, uniform scale, offset, complexity, sub-influence, evolution and static blend. | Proprietary noise, nonuniform scale, overflow, unrepresented blend modes and other controls differ. Popup support does not imply arbitrary modes/animation. |

### Vignette — FX `vignette` / `CS Vignette` (CC Vignette)

Import **omits CC Vignette as unsupported** with a named warning and no invented
radial substitute; the owner and supported siblings remain. Export retains a
static base: FX amount ×100 and radius ×60, with centered native defaults.
Authored feather and unsupported controls are diagnosed. Animated Amount/radius
are omitted with diagnostics because keyed exports had frozen continuous rendering.
Static/readback observations do not establish animated support or general kernel parity.

## Unmapped and specialized families

- Unknown native effects and unsupported FX grades/lens/ML effects have no generic
  editable translation. Similar names are not mapping evidence.
- `CustomShader` has no native AEP export equivalent. Its **rendering owner is
  omitted**, not merely its shader; independent shader-free content remains.
  No baking or generated script is implied.
- Keylight 906 has a narrow static import profile using a converter-owned shader
  with selected editable controls. It is an SDR approximation, not general Keylight
  equivalence, and does not become native-export support.
- Native Layer Styles are separate from Effect Parade. Their selected mappings
  belong in [the style reference](../after-effects-evidence/layer-styles.md).

## Evidence

Mapped source/control tests, generated native records, Adobe acceptance/control
readback and independent rendered comparisons are separate evidence. A native
catalog entry or own-reader round trip cannot establish visual support.
The [evidence guide](../after-effects-test-results.md) links exact historical
receipts, failures and unmeasured cases. It does not claim those tests ran on the
current checkout. RGB comparisons do not establish alpha/audio; selected controls
do not prove all hosts, combinations, key modes or native versions.
