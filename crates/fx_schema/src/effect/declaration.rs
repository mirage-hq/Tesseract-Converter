//! One effect declaration shared by stored and normalized document projections.
//!
//! The projections retain their own color/paint readers and runtime helpers.
//! Field names, variants, defaults and wire annotations must only be changed here.

/// Define the effect payload schema using the caller's existing payload types.
///
/// This declaration has no conditional variants or fields. The portable reader
/// and product normalization boundary deliberately bind different legacy/strict
/// payload readers; neither maintains its own copy of the effect schema.
#[doc(hidden)]
#[macro_export]
// Deliberately bind the caller's strict or lossless color reader, not this crate's.
#[allow(clippy::crate_in_macro_def)]
macro_rules! define_effect_payload_schema {
    () => {
        /// A persisted effect payload this FX reader does not understand yet.
        ///
        /// Unsupported effects retain their original JSON in the ordered effect stack
        /// so an older reader can render the rest of the layer and write the project
        /// back without deleting data authored by a newer reader. They are never
        /// accepted by the typed action schema and render as no-ops.
        #[derive(Debug, Clone, PartialEq)]
        pub struct UnsupportedLayerEffect {
            raw_payload: serde_json::Value,
        }

        impl UnsupportedLayerEffect {
            /// The unsupported effect's wire `type`, when present.
            #[must_use]
            pub fn effect_type(&self) -> Option<&str> {
                self.raw_payload.get("type")?.as_str()
            }

            /// The exact unsupported JSON payload retained for a later writer.
            #[must_use]
            pub fn raw_payload(&self) -> &serde_json::Value {
                &self.raw_payload
            }
        }

        /// One entry in a layer's effect stack.
        ///
        /// Effects apply in declaration order (top-of-list first), matching AE's
        /// Effect Controls panel. A layer with no effects renders normally.
        ///
        /// Effects post-process the layer's rendered output, including its alpha
        /// silhouette (shadow, glow, stroke, overlay, satin, and bevel), color,
        /// geometry, and custom shaders. The legacy `layerStyles` and standalone
        /// `dropShadow` representations remain as compatibility DTOs while their
        /// storage is migrated to this stack. Effect params animate through
        /// `dynamics` entries with an `effectProperty` target
        /// (`{"kind": "effectProperty", "effectId", "paramName"}`) keyed by the
        /// owning [`EffectInstance`] id; the animatable param names per effect are
        /// the camelCase field names (see [`LayerEffect::param_schema`]).
        ///
        /// [`LayerEffect::PersonMatte`] and [`LayerEffect::DepthMatte`] are matte
        /// sources: they replace a media layer's source frame with a generated luma
        /// matte. [`LayerEffect::PosterizeTime`] is a clock warp consumed by the
        /// scene builder. [`LayerEffect::LookTransform`] lowers to a color-LUT node
        /// effect at its authored stack position. The remaining variants are node
        /// effects that post-process the layer's assembled node via
        /// [`LayerEffect::to_node_effect`].
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, strum::VariantNames, TS)]
        #[serde(
            tag = "type",
            rename_all = "camelCase",
            rename_all_fields = "camelCase"
        )]
        #[strum(serialize_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum LayerEffect {
            /// ML person segmentation. Replaces a media layer's source frame with a
            /// grayscale matte where the person is white and the background is
            /// black. Designed to be used as the source of a sibling layer's
            /// [`crate::TrackMatte`] (typically [`crate::TrackMatteType::Luma`]).
            /// Supports video and still-image media sources; on every
            /// other layer type it is a no-op pass-through because there is no source
            /// frame to segment.
            PersonMatte {
                /// What the renderer substitutes when the segmenter produces an
                /// empty mask. Defaults to [`SegmentationEmptyFallback::White`].
                #[serde(default)]
                empty_fallback: SegmentationEmptyFallback,
            },
            /// ML monocular depth estimation. Replaces a media layer's source with a
            /// relative inverse-depth matte: nearer pixels are white and farther
            /// pixels are black. Supports video and still-image media
            /// sources and is a no-op pass-through on other layer types.
            ///
            /// Experimental: omitted from generated action bindings so Agave cannot
            /// discover or author it before release.
            #[ts(skip)]
            DepthMatte {
                /// What the renderer substitutes when depth estimation is unavailable.
                #[serde(default)]
                empty_fallback: SegmentationEmptyFallback,
            },
            /// Gaussian blur of the layer's rendered output. AE "Gaussian Blur";
            /// `blurriness` in pixels (maps to [`scene::NodeEffect::GaussianBlur`]).
            ///
            /// `repeat_edge_pixels` is AE "Repeat Edge Pixels": blur taps clamp to
            /// the content rect instead of sampling transparent beyond it, so
            /// content edges stay solid rather than fading out. `layer_size` is the
            /// host content's dimensions in layer-local pixels — the renderer sizes
            /// the clamp rect from it. When omitted, the lowering falls back to the
            /// layer's own content dims; importers that know the true bounds of an
            /// unsized host (a Group standing in for a PAG precomp) author it
            /// explicitly.
            GaussianBlur {
                /// Blur radius in pixels (0 = no blur).
                blurriness: NonNegativeProperty,
                /// AE-NATIVE: After Effects Gaussian Blur's "Repeat Edge Pixels"
                /// checkbox — not a deviation. `None` ≙ unchecked (the historical
                /// wire behavior).
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                repeat_edge_pixels: Option<bool>,
                /// NOT an AE parameter (deviation, PAG-import support): AE knows a
                /// layer's content bounds implicitly, but this wire's host layer
                /// (e.g. an unsized Group standing in for a PAG precomp) may not
                /// carry them, so importers author the bounds the clamp rect needs.
                /// Sourced from the PAG layer's content size by the PAG→FX
                /// converter.
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                layer_size: Option<(f64, f64)>,
            },
            /// AE "Glow" (maps to [`scene::NodeEffect::Glow`]). `glow_threshold` is
            /// the AE percentage (0-100; the renderer divides by 100),
            /// `glow_radius` in pixels, `glow_intensity` the AE multiplier
            /// (1.0 = default).
            Glow {
                /// Brightness threshold above which pixels bloom, as the AE
                /// percentage (0-100).
                glow_threshold: PercentageProperty,
                /// Bloom spread radius in pixels.
                glow_radius: NonNegativeProperty,
                /// Bloom strength multiplier (AE units; 1.0 = default).
                glow_intensity: NonNegativeProperty,
            },
            /// AE "Directional Blur": `direction` in degrees (AE convention: 0 = up,
            /// clockwise positive), `blur_length` in composition pixels. Converted
            /// to the renderer's UV-space velocity using the composition dimensions
            /// (maps to [`scene::NodeEffect::MotionBlur`]).
            DirectionalBlur {
                /// Blur direction in degrees (AE convention: 0 = up, clockwise
                /// positive).
                direction: f64,
                /// Blur length in composition pixels.
                blur_length: NonNegativeProperty,
            },
            /// AE "Pixel Motion Blur" (JRB-1474): optical-flow motion blur. The
            /// renderer estimates per-pixel motion vectors between the layer at the
            /// previous frame and the current frame, then synthesizes blur along the
            /// estimated motion (maps to [`scene::NodeEffect::PixelMotionBlur`]).
            /// Parameters match AE exactly:
            ///
            /// * `shutter_control` — AE "Shutter Control" popup
            ///   ([`ShutterControl::Automatic`] uses AE's composition motion-blur
            ///   defaults; [`ShutterControl::Manual`] honors the two fields below).
            /// * `shutter_angle` — AE "Shutter Angle" in degrees (default 180;
            ///   AE documents 1° ≈ no blur through 720° = two full frames).
            /// * `shutter_samples` — AE "Shutter Samples": taps integrated along
            ///   the motion vector (default 10; more = smoother, slower).
            /// * `vector_detail` — AE "Vector Detail" (0–100): how densely motion
            ///   vectors are estimated; 100 = one vector per pixel (default 20).
            PixelMotionBlur {
                /// AE "Shutter Control": `automatic` uses AE's composition defaults
                /// (180 degrees, 16 samples) and ignores the two fields below;
                /// `manual` honors them.
                #[serde(default)]
                shutter_control: ShutterControl,
                /// AE "Shutter Angle" in degrees (default 180; clamped 0..720 where
                /// 720 = two full frames of blur). Only read under `manual`.
                #[serde(default = "default_shutter_angle")]
                shutter_angle: NonNegativeProperty,
                /// AE "Shutter Samples": taps integrated along the motion vector
                /// (default 10; clamped 2..64; more = smoother, slower). Only read
                /// under `manual`.
                #[serde(default = "default_shutter_samples")]
                shutter_samples: PositiveProperty,
                /// AE "Vector Detail" (0-100): motion-vector estimation density;
                /// 100 = one vector per pixel (default 20).
                #[serde(default = "default_vector_detail")]
                vector_detail: PercentageProperty,
            },
            /// AE "Mosaic" (maps to [`scene::NodeEffect::Mosaic`]); parameters match
            /// AE exactly.
            Mosaic {
                /// Number of mosaic tiles across the width (integer >= 1).
                horizontal_blocks: PositiveProperty,
                /// Number of mosaic tiles down the height (integer >= 1).
                vertical_blocks: PositiveProperty,
                /// When true, each tile takes a single sampled color instead of the
                /// averaged block color.
                #[serde(default)]
                sharp_colors: bool,
            },
            /// Reader-only canonical creative look transform. It lowers at its exact
            /// ordered effect-stack position; reader-only means authoring remains gated,
            /// not that rendering ignores the persisted operation.
            LookTransform {
                /// Version fixing ordering, encoding, math, alpha, and fallback semantics.
                #[ts(type = "number")]
                semantic_version: crate::ColorTransformSemanticVersion,
                /// Transform representation evaluated in the display-referred working encoding.
                transform: crate::ColorTransform,
                /// Normalized wet/dry mix (`0` = exact bypass, `1` = fully applied).
                #[serde(default = "crate::color::default_color_mix")]
                #[ts(type = "number")]
                mix: crate::NormalizedColorMix,
            },
            /// Reader-only canonical primary grade. The payload fixes ten bounded
            /// controls, their operation order, SDR encoding, and straight-alpha math.
            PrimaryGrade(crate::PrimaryGrade),
            /// Jerboa SDR tonal color v1, not vendor LGG/Offset.
            TonalColor(crate::TonalColor),
            /// Reader-only, versioned Master/RGB curves in normalized display-referred SDR.
            /// Version 1 uses piecewise-linear interpolation on straight RGB, evaluates
            /// Master before the per-channel curves, and preserves alpha.
            ColorCurves {
                /// Version fixing interpolation, ordering, encoding, and alpha semantics.
                #[ts(type = "number")]
                semantic_version: crate::ColorCurvesSemanticVersion,
                /// Validated Master, red, green, and blue curves.
                curves: crate::ColorCurves,
            },
            /// AE "Brightness & Contrast" (maps to
            /// [`scene::NodeEffect::BrightnessContrast`]). AE units pass through
            /// unscaled — the renderer shader already normalizes exactly like libpag
            /// (contrast/300, brightness/250 or /650).
            BrightnessContrast {
                /// AE brightness, -150..150 (0 = unchanged).
                brightness: f64,
                /// AE contrast, -100..100 (0 = unchanged).
                contrast: f64,
            },
            /// AE "Shift Channels", constrained to what the levels backend can
            /// express: each output channel keeps its own input (`red`/`green`/
            /// `blue` when it names the SAME channel), or is forced to `fullOff` /
            /// `fullOn`. Cross-channel routing (e.g. take red from green) is not
            /// expressible via [`scene::NodeEffect::LevelsIndividual`] and falls
            /// back to keeping the channel (warn-once). Stacking offset copies with
            /// complementary channels produces real chromatic aberration.
            ShiftChannels {
                /// Source for the output red channel (`red` keeps it unchanged).
                #[serde(default = "ChannelSource::red")]
                take_red_from: ChannelSource,
                /// Source for the output green channel (`green` keeps it unchanged).
                #[serde(default = "ChannelSource::green")]
                take_green_from: ChannelSource,
                /// Source for the output blue channel (`blue` keeps it unchanged).
                #[serde(default = "ChannelSource::blue")]
                take_blue_from: ChannelSource,
            },
            /// AE "Hue/Saturation" (maps to [`scene::NodeEffect::HueSaturation`]).
            /// `hue` in degrees (-180..180); `saturation`/`lightness` in AE -100..100.
            /// When `colorize` is set, the image is desaturated then tinted using the
            /// `colorize_*` controls (`colorize_hue` 0..360, the others AE units).
            HueSaturation {
                /// Hue rotation in degrees, -180..180 (0 = unchanged).
                hue: f64,
                /// AE saturation, -100..100 (0 = unchanged).
                saturation: f64,
                /// AE lightness, -100..100 (0 = unchanged).
                lightness: f64,
                /// When true, desaturate the image then tint it with the
                /// `colorize*` controls below.
                #[serde(default)]
                colorize: bool,
                // The colorize controls are inert when `colorize` is false, so default
                // them too — otherwise a payload that omits the optional `colorize` flag
                // would still be forced to carry all three of these.
                /// Tint hue in degrees, 0..360. Inert unless `colorize` is true.
                #[serde(default)]
                colorize_hue: f64,
                /// Tint saturation (AE units). Inert unless `colorize` is true.
                #[serde(default)]
                colorize_saturation: f64,
                /// Tint lightness (AE units). Inert unless `colorize` is true.
                #[serde(default)]
                colorize_lightness: f64,
            },
            /// AE "Radial Blur" (maps to [`scene::NodeEffect::RadialBlur`]). `center_x`/
            /// `center_y` are the blur origin in UV space (0..1); `amount` is in libpag
            /// raw units (the shader scales/clamps it, useful range ~0..50).
            RadialBlur {
                /// Blur origin X in UV space, 0..1 (0.5 = center).
                center_x: f64,
                /// Blur origin Y in UV space, 0..1 (0.5 = center).
                center_y: f64,
                /// Blur strength in libpag raw units (useful range ~0..50).
                amount: f64,
            },
            /// AE "Levels" master controls (maps to [`scene::NodeEffect::LevelsIndividual`]
            /// with neutral per-channel curves). Input/output points are 0..255 (the
            /// shader divides by 255); `gamma` is the midpoint exponent (~0.1..10).
            Levels {
                /// Input black point, 0..255 (default 0).
                input_black: f64,
                /// Input white point, 0..255 (default 255).
                input_white: f64,
                /// Midpoint exponent, ~0.1..10 (1.0 = unchanged).
                gamma: f64,
                /// Output black point, 0..255 (default 0).
                output_black: f64,
                /// Output white point, 0..255 (default 255).
                output_white: f64,
            },
            /// AE "Bulge" (maps to [`scene::NodeEffect::Bulge`]). Center and radii are
            /// content-normalized [0,1] (0.5 = layer center); `bulge_height` is the
            /// pinch/bulge strength (negative pinches); `pinning` clamps displaced
            /// samples to the content edge. The renderer fills the transform-dependent
            /// geometry from the layer's content dims at lowering time.
            Bulge {
                /// Bulge center X, content-normalized 0..1 (0.5 = layer center).
                center_x: f64,
                /// Bulge center Y, content-normalized 0..1 (0.5 = layer center).
                center_y: f64,
                /// Ellipse radius X, content-normalized 0..1.
                horizontal_radius: f64,
                /// Ellipse radius Y, content-normalized 0..1.
                vertical_radius: f64,
                /// Pinch/bulge strength (negative pinches inward; useful range
                /// roughly -4..4, not clamped).
                bulge_height: f64,
                /// When true, clamps displaced samples to the content edge.
                #[serde(default)]
                pinning: bool,
            },
            /// AE "Corner Pin" (maps to [`scene::NodeEffect::CornerPin`]). The four
            /// corners are content-normalized [0,1] (default = identity quad: UL (0,0),
            /// UR (1,0), LL (0,1), LR (1,1)); the renderer projects them through the
            /// layer transform into a perspective quad.
            CornerPin {
                /// Upper-left corner X, content-normalized (identity 0).
                upper_left_x: f64,
                /// Upper-left corner Y, content-normalized (identity 0).
                upper_left_y: f64,
                /// Upper-right corner X, content-normalized (identity 1).
                upper_right_x: f64,
                /// Upper-right corner Y, content-normalized (identity 0).
                upper_right_y: f64,
                /// Lower-left corner X, content-normalized (identity 0).
                lower_left_x: f64,
                /// Lower-left corner Y, content-normalized (identity 1).
                lower_left_y: f64,
                /// Lower-right corner X, content-normalized (identity 1).
                lower_right_x: f64,
                /// Lower-right corner Y, content-normalized (identity 1).
                lower_right_y: f64,
            },
            /// AE "Motion Tile" (maps to [`scene::NodeEffect::MotionTile`]). `tile_center`
            /// is content-normalized [0,1]; tile/output sizes are AE percentages
            /// (100 = 1×); `phase` in degrees. `mirror_edges` flips alternate tiles.
            MotionTile {
                /// Tile center X, content-normalized 0..1 (0.5 = center).
                tile_center_x: f64,
                /// Tile center Y, content-normalized 0..1 (0.5 = center).
                tile_center_y: f64,
                /// Tile width as an AE percentage (100 = source size).
                tile_width: f64,
                /// Tile height as an AE percentage (100 = source size).
                tile_height: f64,
                /// Output region width as an AE percentage (100 = source size).
                output_width: f64,
                /// Output region height as an AE percentage (100 = source size).
                output_height: f64,
                /// When true, flips alternate tiles for seamless edges.
                #[serde(default)]
                mirror_edges: bool,
                /// Vertical tile phase offset in degrees (360 = one full tile).
                phase: f64,
            },
            /// Drop shadow cast behind the layer's alpha silhouette.
            DropShadow(DropShadow),
            /// Outer glow around the layer's alpha silhouette.
            OuterGlow(OuterGlowStyle),
            /// Stroke outline around the layer's alpha silhouette.
            Stroke(StrokeOutlineStyle),
            /// Gradient fill composited over the layer's current alpha.
            GradientOverlay(GradientOverlayStyle),
            /// Inner shadow cast onto the layer's alpha silhouette.
            InnerShadow(InnerShadowStyle),
            /// Inner glow radiating inward from the layer's alpha edge.
            InnerGlow(InnerGlowStyle),
            /// Satin contour sheen over the layer's alpha silhouette.
            Satin(SatinStyle),
            /// Bevel and emboss lighting over the layer's alpha edge.
            BevelEmboss(BevelEmbossStyle),
            /// Engine-owned Posterize layer effect.
            Posterize {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                levels: Option<f64>,
            },
            /// AE "Posterize Time": quantize this layer's effective time to
            /// `frame_rate` frames per second, so the layer's video frames, keyframed
            /// animation, and animated effects all hold and step together (e.g. at
            /// 8 fps) while the composition plays at full rate. Distinct from
            /// [`LayerEffect::Posterize`], which quantizes color levels.
            ///
            /// Not a pixel pass: by the time the renderer sees a node, time is fully
            /// resolved, so [`LayerEffect::to_node_effect`] returns `None` (like the
            /// matte sources) and the scene builder consumes the effect instead. The
            /// layer walk quantizes the layer's paint/sample clock
            /// ([`crate::Layer::evaluate_node`]) and the animation bake quantizes the
            /// per-target animator clock through the same shared warp
            /// ([`crate::AnimatorClockMap`]), so video sampling, keyframed motion,
            /// and reserved shader time params step on one grid anchored at the
            /// layer's in-point.
            ///
            /// Deliberate v1 deviations from AE (JRB-1778): quantization applies to
            /// the whole layer (source + its own animation + every effect) regardless
            /// of the effect's position in the stack, and `frameRate` is not
            /// animatable. Matching AE: EXTERNAL layers referenced as this layer's
            /// track matte or custom-shader inputs are not retimed by this layer's
            /// hold (a matte is its own layer) — posterize the source too, or
            /// posterize a Group containing both; everything inside a posterized
            /// Group holds with it, like an AE precomp.
            PosterizeTime {
                /// Target hold rate in frames per second (default 8).
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                frame_rate: Option<f64>,
            },
            /// Engine-owned Vignette layer effect.
            Vignette {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amount: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                radius: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                feather: Option<f64>,
            },
            /// Engine-owned FindEdges layer effect.
            FindEdges {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                invert: Option<f64>,
            },
            /// Engine-owned Exposure layer effect.
            Exposure {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                exposure: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                offset: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                gamma_correction: Option<f64>,
            },
            /// Engine-owned Vibrance layer effect.
            Vibrance {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                vibrance: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                saturation: Option<f64>,
            },
            /// Engine-owned TemperatureTint layer effect.
            TemperatureTint {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                temperature: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                tint: Option<f64>,
            },
            /// Engine-owned Twirl layer effect.
            Twirl {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                angle: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                radius: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_x: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_y: Option<f64>,
            },
            /// Engine-owned Ripple layer effect.
            Ripple {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amplitude: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                frequency: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                phase: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_x: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_y: Option<f64>,
            },
            /// Engine-owned Sharpen layer effect.
            Sharpen {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amount: Option<f64>,
            },
            /// Engine-owned LumaKey layer effect.
            LumaKey {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                threshold: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                softness: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                invert: Option<f64>,
            },
            /// Engine-owned SimpleChoker layer effect: shifts the matte edge by `choke`
            /// px (positive erodes, negative spreads; clamped to +/-10) and re-hardens it
            /// at the 50% alpha contour. With a Gaussian Blur earlier in the same stack
            /// this is the gooey/metaball recipe — author both effects on the Group
            /// containing the shapes so their alphas combine first; per-layer chains
            /// cannot merge.
            SimpleChoker {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                choke: Option<f64>,
            },
            /// Engine-owned Grain layer effect using After Effects Add Grain units.
            Grain {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amount: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                size: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                softness: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                aspect_ratio: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                seed: Option<f64>,
            },
            /// Engine-owned WaveWarp layer effect.
            WaveWarp {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                wave_height: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                wave_width: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                direction: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                phase: Option<f64>,
            },
            /// Engine-owned LensDistortion layer effect.
            LensDistortion {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amount: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_x: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_y: Option<f64>,
            },
            /// Engine-owned TintTritone layer effect.
            TintTritone {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                black_r: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                black_g: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                black_b: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                white_r: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                white_g: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                white_b: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amount: Option<f64>,
            },
            /// Engine-owned GradientRamp layer effect.
            GradientRamp {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                start_x: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                start_y: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                end_x: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                end_y: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                start_r: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                start_g: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                start_b: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                end_r: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                end_g: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                end_b: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                blend: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                shape: Option<f64>,
            },
            /// Engine-owned ChromaticAberration layer effect.
            ChromaticAberration {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amount: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                direction: Option<f64>,
            },
            /// Engine-owned TurbulentNoise layer effect.
            ///
            /// `noise_type` / `fractal_type` are AE's two popups. They are typed enums,
            /// not scalar params, so they are deliberately absent from
            /// [`Self::param_schema`] (the editor renders every schema entry as a
            /// slider) and are written through `UpdateFxLayerEffect`'s whole-effect
            /// replace rather than `SetFxLayerEffectParams` — see
            /// `LayerEffect::enum_backed_uniform_names`.
            TurbulentNoise {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                brightness: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                contrast: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                scale: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                complexity: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                sub_influence: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                evolution: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                rotation: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                offset_x: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                offset_y: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                invert: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                blend: Option<f64>,
                /// AE "Noise Type" popup: how the generator interpolates between
                /// lattice points. Omitting the field means `softLinear`, AE's default
                /// and the only mode the v33 effect shipped. Not a scalar param — write
                /// it with `UpdateFxLayerEffect`, not `SetFxLayerEffectParams`.
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                noise_type: Option<NoiseType>,
                /// AE "Fractal Type" popup: how the fractal's octaves are combined.
                /// Omitting the field means `basic`. `strings`, `rocky` and `cloudy`
                /// approximate AE's proprietary algorithms. Not a scalar param — write
                /// it with `UpdateFxLayerEffect`, not `SetFxLayerEffectParams`.
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                fractal_type: Option<FractalType>,
            },
            /// Engine-owned Fisheye layer effect.
            Fisheye {
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                amount: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_x: Option<f64>,
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(optional)]
                center_y: Option<f64>,
            },
            /// ENG-1280 / JRB-1143: an author- or AI-provided WGSL fragment shader, run
            /// as a single post-process pass over the layer's rendered output. The
            /// escape hatch for the long tail of effects we don't build in as typed
            /// variants — a new shot style ships as data (WGSL + params), not code.
            /// Compiled + naga-validated by the renderer (hash-cached); a compile error
            /// renders as a no-op pass-through. Each `params` entry is animator-bindable
            /// by name, exactly like a typed effect's params (see [`LayerEffect::param_schema`]).
            CustomShader {
                /// Stable, app-assigned label for this shader, and the upsert key:
                /// re-adding a custom shader with the same `name` replaces it in place
                /// (keeping its [`EffectId`]) rather than stacking a duplicate. Survives
                /// WGSL/param edits, unlike the renderer's internal content-hash compile
                /// key. NOT the addressing handle — mutations and animators target the
                /// effect by its [`EffectId`]. Unique among the layer's custom shaders
                /// (typed effects carry no name); maintained by the upsert, not validated
                /// on load.
                name: String,
                /// Agent/human-facing explanation of what this shader does — recorded by
                /// the author (often an AI) so the shader's intent travels with it and is
                /// shared back as context for later edits. Optional on the wire.
                #[serde(default, skip_serializing_if = "String::is_empty")]
                description: String,
                /// WGSL fragment source, bound against the single-pass effect layout
                /// (or the 2-texture multi-input layout when `texture_inputs` is
                /// non-empty).
                wgsl: String,
                /// Scalar UNIFORM VALUES the shader reads — the numeric knobs (amount,
                /// angle, colour channels, …). Packed std140 into the `Params` uniform
                /// buffer in declared order, one `f32` each. Distinct from
                /// `texture_inputs`: these are values, not textures. Each carries its
                /// own name/description/range; no registry.
                #[serde(default)]
                params: Vec<EffectParam>,
                /// Extra TEXTURE INPUTS the shader samples beyond the layer's own
                /// content — its `@binding(1..)` textures, e.g. a `"map"` slot for
                /// displacement. Distinct from `params` (scalar uniforms): each entry
                /// ([`EffectTextureInput`]) is a named texture slot bound to a source
                /// LAYER via `source_layer_id`. Empty = single-input. Drives the editor
                /// input pickers + the `SetFxLayerEffectInputBinding` surface; only bound
                /// slots resolve into the render tree, and a bound source layer is
                /// consumed (auto-hidden from standalone paint).
                #[serde(default, skip_serializing_if = "Vec::is_empty")]
                texture_inputs: Vec<EffectTextureInput>,
            },
            /// Persisted payload authored by a newer FX writer. This variant is
            /// created only by [`EffectInstance`]'s tolerant project reader; direct
            /// [`LayerEffect`] deserialization remains strict for typed actions.
            #[doc(hidden)]
            #[serde(skip)]
            #[ts(skip)]
            Unsupported(UnsupportedLayerEffect),
        }

        /// AE default "Shutter Angle" for [`LayerEffect::PixelMotionBlur`] (degrees).
        fn default_shutter_angle() -> NonNegativeProperty {
            NonNegativeProperty::new(180.0).expect("180.0 is finite and non-negative")
        }

        /// AE default "Shutter Samples" for [`LayerEffect::PixelMotionBlur`].
        fn default_shutter_samples() -> PositiveProperty {
            PositiveProperty::new(10.0).expect("10.0 is finite and positive")
        }

        /// AE default "Vector Detail" for [`LayerEffect::PixelMotionBlur`].
        fn default_vector_detail() -> PercentageProperty {
            PercentageProperty::new(20.0).expect("20.0 is within 0..=100")
        }
    };
}
