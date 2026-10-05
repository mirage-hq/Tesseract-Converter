//! Standard clip effects between the Premiere stack model and FX
//! `EffectRecord`s.
//!
//! Import appends each convertible effect in stack order, the order Premiere
//! applies them, and disables bypassed ones, so order and bypass stay editable.
//! Export keeps the current FX order and enabled state; an effect without a
//! Premiere mapping is omitted with a reason, the others keep their order. A
//! Gaussian or Directional Blur exports as the current (Film Impact) blur; a
//! value outside its Amount range omits the effect like any other range
//! failure, and the Legacy blurs are only read.
//!
//! Keyed parameters with an [`EffectParamBinding`] become FX `effectProperty`
//! keyframe tracks and back, with the key times, easing and rules of Motion.
//! A Legacy or current Directional Blur's values also map through its clip's
//! static Motion (see [`ClipToComposition`]), an Invert is a `levels` with
//! complementary outputs (see [`invert_levels`]), a Tint or Black & White is a
//! `tintTritone` whose colours are three channel scalars each (see
//! [`tint_tritone`]), a Ramp is a `gradientRamp` on a clip whose frame is
//! the canvas (see [`gradient_ramp`] and [`ImportHost::frame`]), and a
//! Replicate is a `motionTile` of whole copies on such a clip (see
//! [`motion_tile`]), which both directions report as an approximation
//! ([`PrReplicate::TILING_APPROXIMATION`]).
//! A Posterize is a `posterize` with the same whole Level, which both
//! directions report as an approximation ([`PrPosterize::QUANTIZER_APPROXIMATION`]).

use super::{
    keyframes,
    tesseract_to_premiere::{VideoKeyClock, WrittenAnimation, HELD_CLOCK_REASON},
};
use crate::schema::{alpha_glow_size, ALPHA_GLOW, ALPHA_GLOW_APPROXIMATION, ALPHA_GLOW_SIZE};
use crate::schema::{lens_curvature, LENS_APPROXIMATION, LENS_CURVATURE, LENS_DISTORTION};
use crate::schema::{
    normalize_legacy_luma, validate_legacy_luma, LEGACY_LUMA_CUTOFF, LEGACY_LUMA_KEY,
    LEGACY_LUMA_THRESHOLD,
};
use crate::{
    approximate,
    export_loss::{omit_field, ExportField, OmissionSink},
    media::MediaFacts,
    omit,
    schema::{
        EffectParamBinding, EffectParamSpec, EffectSpec, MaskBoundary, PrAnimatedProperty,
        PrBrightnessContrast, PrColour, PrColourKeyframe, PrCornerPin, PrDirectionalBlur, PrEffect,
        PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, PrFilmImpactBlur,
        PrFilmImpactDirectionalBlur, PrFindEdges, PrGaussianBlur, PrInvert, PrKeyframeEasing,
        PrLevelChannel, PrLevels, PrMediaKind, PrMosaic, PrPointKeyframe, PrPosterize, PrRamp,
        PrReplicate, PrScalarKeyframe, PrSharpen, PrStaticTransform, PrTint, PrTransform,
        PrVideoOccurrence, BRIGHTNESS_CONTRAST, BRIGHTNESS_CONTRAST_BRIGHTNESS,
        BRIGHTNESS_CONTRAST_CONTRAST, CORNER_PIN, DIRECTIONAL_BLUR_DIRECTION,
        DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_BLUR, FILM_IMPACT_BLUR_AMOUNT,
        FILM_IMPACT_DIRECTIONAL_BLUR, FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
        FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, INVERT_BLEND, LEGACY_LUMA_KEY_MAPPING_REASON, LEVELS,
        MOSAIC, MOSAIC_HORIZONTAL_BLOCKS, MOSAIC_VERTICAL_BLOCKS, NOISE, NOISE_AMOUNT,
        NOISE_APPROXIMATION, POSTERIZE, POSTERIZE_LEVEL, POSTERIZE_TIME, RAMP, RAMP_BLEND,
        RAMP_END, RAMP_END_COLOR, RAMP_START, RAMP_START_COLOR, REPLICATE, REPLICATE_COUNT,
        SHARPEN, SHARPEN_AMOUNT, SOURCE_CHAIN_NOT_CONVERTED, TINT, TINT_AMOUNT, TINT_MAP_BLACK_TO,
        TINT_MAP_WHITE_TO, TRANSFORM_OPACITY, TRANSFORM_POSITION, TRANSFORM_ROTATION,
        TRANSFORM_SCALE_HEIGHT, TRANSFORM_SCALE_WIDTH, TRANSFORM_SKEW, TRANSFORM_SKEW_AXIS,
    },
    Omission, OmissionScope,
};
use fx_schema::{
    animator::{
        AnimationGraph, AnimationGraphEntry, AnimatorData, PropertyKeyframe, PropertyKeyframeTrack,
    },
    effect::ChannelSource,
    EffectData, EffectId, EffectPayload, EffectRecord, FXComposition, ImageSource, Layer,
    LayerData, LayerEffect, LayerId, MediaSourceKind, NonNegativeProperty, PositiveProperty,
    PropType, PropertyTarget, PropertyValue, TimeOffset, Transform, VideoLayer,
};
use std::collections::{BTreeMap, BTreeSet};

/// Preserve editable temporal curves using the existing physical-video clock;
/// native effect timing, unlike the base picture playback, is not render-proven.
pub(super) const RETIMED_EFFECT_CLOCK_APPROXIMATION: &str = "retimed effect keys retained on the physical source clock driven by editable playback, without subtracting Source In or reversing the keys twice; native retimed effect-clock fidelity is unmeasured";

/// Why a Directional Blur needs a host whose Motion is a static similarity.
const SIMILARITY_RULE: &str = "a Directional Blur converts between Premiere's clip frame and FX composition space only through a static, uniform and positive scale and a static 2D rotation";

/// Why a Directional Blur on a stage group is omitted in both directions: its
/// map would be the group's Motion, which is the clip's, and no Adobe case
/// verifies that for a staged clip.
const STAGED_DIRECTIONAL_BLUR_REASON: &str = "a Directional Blur on a stage group is not converted; no Adobe case verifies its map through the group's Motion";

/// Why a Directional Blur on a nest placement, or on a clip inside a nest whose
/// placement writes Motion, is omitted on export: its map would also go through
/// the placement's Motion, and no Adobe case verifies that.
const NESTED_DIRECTIONAL_BLUR_REASON: &str = "a Directional Blur on a nested sequence, or inside one whose placement has Motion, is not converted; no Adobe case verifies its map through the nest's Motion";

const SHARPEN_HOST_RULE: &str = "Sharpen converts only on a canvas-sized frame at identity static Motion without Motion keys, skew, 3D transforms, stages, nested import hosts, nest placements or moved nests; other processing orders are unverified";
/// Both edge detectors sample neighboring pixels; transformed/nonmatching
/// frames do not establish the same grid. Native fidelity is still unmeasured.
const FIND_EDGES_FRAME_RULE: &str = "Find Edges converts only on a full-canvas host at identity static Motion without Motion keys or stage groups, and excludes nested sequences on export; Adobe's source-pixel grid otherwise differs from FX's effect frame";

const STILL_SHARPEN_REASON: &str =
    "Sharpen on a still image is not converted; its processing order is unverified";

/// Why a Ramp needs a host whose frame is the canvas: the FX shader measures
/// the ramp in the layer frame's UV, Premiere in clip pixels.
const FRAME_RULE: &str = "a Ramp converts only on a clip whose frame is the canvas, at identity static Motion without Motion keys, because the FX gradientRamp measures the ramp in the layer frame's UV and Premiere in clip pixels";

/// Why a Ramp on a stage group's video or on a nest placement is omitted in
/// both directions: the frame that the FX shader measures in there is not
/// verified against Premiere's clip frame by any Adobe case.
const STAGED_RAMP_REASON: &str = "a Ramp on a stage group or a nested sequence is not converted; no Adobe case verifies the frame that the FX gradientRamp measures in there";

/// Why a Mosaic on a stage group's video or on a nest placement is omitted in
/// both directions: the content rect over which the FX `mosaic` lays its grid
/// there is not verified against Premiere's clip frame by any Adobe case. A
/// plain media clip converts at any Motion.
const STAGED_MOSAIC_REASON: &str = "a Mosaic on a stage group or a nested sequence is not converted; no Adobe case verifies the frame over which the FX mosaic lays its grid there";

/// Why a Replicate needs a host whose frame is the canvas: the FX `motionTile`
/// tiles its layer's transformed content, and no Adobe case verifies that
/// grid against Premiere's, drawn in the clip frame before Motion, under any
/// other placement.
const REPLICATE_FRAME_RULE: &str = "a Replicate converts only on a clip whose frame is the canvas, at identity static Motion without Motion keys, because the FX motionTile tiles its layer's transformed content and no Adobe case verifies that grid under another placement";

/// Why a Replicate on a stage group's video or on a nest placement is omitted
/// in both directions: the frame that the FX `motionTile` tiles there is not
/// verified against Premiere's clip frame by any Adobe case.
const STAGED_REPLICATE_REASON: &str = "a Replicate on a stage group or a nested sequence is not converted; no Adobe case verifies the frame that the FX motionTile tiles there";

const STILL_REPLICATE_REASON: &str =
    "Replicate on a still image is not converted; its processing order is unverified";

/// Why a Transform converts on no host but a media clip's stage group: FX has
/// no transform effect, the staged video's transform carries it, a still
/// imports as one image layer without a stage group ([`import_still_effects`]),
/// and an adjustment layer's geometric transform does not render in FX.
const TRANSFORM_HOST_REASON: &str = "a Transform converts only as the transform of a media clip's stage group; FX has no transform effect, a still image imports no stage group, and an adjustment layer's geometric transform does not render";

/// Why a Transform among a master clip's source effects is omitted: FX has no
/// transform effect, and only a placement's own one Transform converts, as
/// its staged video's transform; nothing measures a stage
/// for a Transform applied before the placement's own effects.
const SOURCE_TRANSFORM_REASON: &str = "a Transform among a master clip's source effects is not converted: FX has no transform effect, and only a placement's own one Transform converts, as its staged video's transform";

/// Why a Corner Pin, a Mosaic or a Blur that repeats its edge pixels needs a
/// picture layer whose FX effects measure the clip's own frame: its corners
/// are fractions of that frame, and its grid and edge bounds span it. FX lays
/// a video layer's effects over its media frame, where Premiere applies
/// them, but a group's over the composition canvas, so a linked composition
/// of another size would map them wrongly.
const PICTURE_FRAME_RULE: &str = "a Corner Pin, Mosaic or Blur that repeats edge pixels converts only on a picture whose FX layer lays its effects over the clip's own frame, where Premiere applies them; FX lays a linked After Effects composition's group effects over the canvas";

/// The report, once on each master clip, of source effects that import
/// converts ([`import_source_effects`]): every placement takes its own copy,
/// so the editing that the master clip links is lost.
pub(crate) const LINKED_SOURCE_EDITING_REASON: &str = "linked editing of the source effects is not converted: each placement takes its own copy of them, before its own effects, so an edit to one copy changes no other placement, and export writes each copy in its placement's own chain";

/// FX holds the whole layer, not a native effect-stack input. Even on a
/// plain clip the native grid phase has not been measured, especially after trim.
const POSTERIZE_TIME_CLOCK_APPROXIMATION: &str = "FX holds source frames, layer animation and every effect on a grid anchored at the layer's in-point regardless of stack position; Premiere's sampling phase, including source trim, is unmeasured";
const POSTERIZE_TIME_OWNER_REASON: &str = "Posterize Time converts only on a plain video clip: FX adjustment effects do not hold time, stills and linked compositions are not physical video owners, and nested, group, staged and retimed clocks have no mapped native equivalent";
const POSTERIZE_TIME_ANIMATION_REASON: &str = "Posterize Time with other layer or effect animation is not converted: FX would hold those animations regardless of native stack position; the animations and other effects were kept";

/// Mirrors the existing FX runtime's 1–60 fps rate resolution; this
/// standalone converter cannot depend on the engine implementation.
const POSTERIZE_TIME_MIN_RATE: f64 = 1.0;
const POSTERIZE_TIME_MAX_RATE: f64 = 60.0;
const POSTERIZE_TIME_DEFAULT_RATE: f64 = 8.0;

fn resolved_posterize_time_rate(rate: Option<f64>) -> f64 {
    rate.filter(|rate| rate.is_finite() && *rate > 0.0)
        .map(|rate| rate.clamp(POSTERIZE_TIME_MIN_RATE, POSTERIZE_TIME_MAX_RATE))
        .unwrap_or(POSTERIZE_TIME_DEFAULT_RATE)
}

/// The facts of an imported clip that some effects' values depend on.
struct ImportHost {
    /// The clip's static similarity, for a Directional Blur.
    similarity: Result<ClipToComposition, String>,
    /// Why the clip's frame is not the canvas at identity static Motion
    /// without Motion keys, which a Ramp and a Replicate need
    /// ([`Self::frame`]), or `None` when it is.
    off_canvas: Option<&'static str>,
    /// Whether the clip converts under a stage group, on which a Ramp, a
    /// Mosaic ([`STAGED_MOSAIC_REASON`]) and a Replicate are omitted.
    staged: bool,
    /// Whether the picture layer's FX effects measure the clip's own frame,
    /// for a Corner Pin, a Mosaic or a Blur that repeats edge pixels, or why
    /// not ([`PICTURE_FRAME_RULE`]).
    own_frame: Result<(), String>,
}

impl ImportHost {
    /// The host facts of `clip`, whose media frame is `source` pixels on a
    /// `canvas`, converting under `boundary` on a picture layer whose FX
    /// effects measure `effects_frame` pixels.
    fn of_clip(
        clip: &PrVideoOccurrence,
        boundary: MaskBoundary,
        source: [u32; 2],
        effects_frame: [u32; 2],
        canvas: [u32; 2],
    ) -> Self {
        // Only a Directional Blur maps through the clip's Motion (see `fx_value`).
        let similarity = match boundary {
            MaskBoundary::Flat => ClipToComposition::of_clip(clip),
            MaskBoundary::Staged => Err(STAGED_DIRECTIONAL_BLUR_REASON.to_owned()),
        };
        let transform = clip.transform;
        let motion_animated = clip
            .animations
            .iter()
            .any(|animation| animation.property() != PrAnimatedProperty::Opacity);
        let own_frame = if effects_frame == source {
            Ok(())
        } else {
            Err(format!(
                "its picture's FX layer lays effects over {}x{} pixels, and Premiere over the clip's {}x{} frame; {PICTURE_FRAME_RULE}",
                effects_frame[0], effects_frame[1], source[0], source[1]
            ))
        };
        Self {
            similarity,
            off_canvas: super::premiere_to_tesseract::wipe_frame_reason(
                source,
                canvas,
                transform.scale,
                transform.rotation,
                transform.anchor_point == transform.position,
                motion_animated,
            ),
            staged: boundary == MaskBoundary::Staged,
            own_frame,
        }
    }

    /// Whether the clip's frame is the canvas at identity static Motion
    /// without Motion keys, as an effect measured in that frame needs, or why
    /// not: on a stage group the effect's `staged` reason, otherwise the
    /// frame's reason followed by the effect's `rule` ([`FRAME_RULE`],
    /// [`REPLICATE_FRAME_RULE`]).
    fn frame(&self, staged: &str, rule: &str) -> Result<(), String> {
        if self.staged {
            return Err(staged.to_owned());
        }
        match &self.off_canvas {
            None => Ok(()),
            Some(reason) => Err(format!("{reason}; {rule}")),
        }
    }
}

/// A host clip's static similarity from its own frame into composition space.
/// Premiere applies a Directional Blur in the clip's frame, before Motion
/// , while FX `directionalBlur` blurs composition pixels along
/// an absolute direction, so the Blur Length scales by `scale` and the
/// Direction turns by `rotation`. The media frame
/// maps one source pixel to one layer pixel in both directions. E2 verifies
/// the direction part, and the export gate's AME render measures both parts
/// on one clip (Scale 50, Rotation 30); other scales and rotations are not
/// measured.
#[derive(Debug, Clone, Copy)]
struct ClipToComposition {
    /// The uniform scale over 100, positive.
    scale: f64,
    /// The rotation in degrees, clockwise like the Direction.
    rotation: f64,
}

impl ClipToComposition {
    /// The similarity of an imported clip: its static uniform Scale and its
    /// static Rotation.
    fn of_clip(clip: &PrVideoOccurrence) -> Result<Self, String> {
        let keyed = clip
            .animations
            .iter()
            .find_map(|animation| match animation.property() {
                PrAnimatedProperty::UniformScale => Some("Scale"),
                PrAnimatedProperty::ScaleWidth => Some("Scale Width"),
                PrAnimatedProperty::Rotation => Some("Rotation"),
                // These move or fade the blurred frame without turning or
                // scaling it, as on export (`of_layer`).
                PrAnimatedProperty::Opacity
                | PrAnimatedProperty::Position
                | PrAnimatedProperty::AnchorPoint => None,
            });
        if let Some(property) = keyed {
            return Err(format!("its clip has keyed {property}; {SIMILARITY_RULE}"));
        }
        let PrStaticTransform {
            scale, rotation, ..
        } = clip.transform;
        Self::uniform("its clip's static Scale", scale, rotation)
    }

    /// The similarity of an exported layer: its FX static scale and rotation,
    /// which the Motion export must write unchanged. Premiere's Motion has no
    /// skew, 3D rotation, orientation or z position, so a layer that uses any
    /// of them, with a nonzero static value or a track, omits the blur. A stage
    /// group is no host ([`STAGED_DIRECTIONAL_BLUR_REASON`]), and neither is a
    /// nest placement or a clip that one moves ([`NESTED_DIRECTIONAL_BLUR_REASON`]).
    fn of_layer(host: EffectHost<'_>, dynamics: &AnimationGraph) -> Result<Self, String> {
        if host.staged {
            return Err(STAGED_DIRECTIONAL_BLUR_REASON.to_owned());
        }
        if host.nested {
            return Err(NESTED_DIRECTIONAL_BLUR_REASON.to_owned());
        }
        let animated = dynamics
            .entries()
            .iter()
            .find_map(|entry| match &entry.target {
                PropertyTarget::LayerProperty(target) if target.layer_id() == host.layer => {
                    match target.property_type() {
                        PropType::ScaleX | PropType::ScaleY => Some("scale"),
                        PropType::Rotation => Some("rotation"),
                        PropType::Skew => Some("skew"),
                        PropType::RotationX => Some("X rotation"),
                        PropType::RotationY => Some("Y rotation"),
                        PropType::OrientationX
                        | PropType::OrientationY
                        | PropType::OrientationZ => Some("orientation"),
                        PropType::PositionZ => Some("z position"),
                        // Position, anchor point and opacity move or fade the
                        // blurred frame without turning or scaling it. The
                        // skew axis does nothing while the skew is zero, and a
                        // nonzero or animated skew omits the blur itself.
                        _ => None,
                    }
                }
                _ => None,
            });
        let transform = host.transform;
        let form = if let Some(property) = animated {
            format!("its layer has animated {property}")
        } else if transform.skew != 0.0 {
            "its layer is skewed".to_owned()
        } else if transform.rotation_x != 0.0
            || transform.rotation_y != 0.0
            || transform.orientation != [0.0; 3]
        {
            "its layer has 3D rotation".to_owned()
        } else if let Some(z) = transform.position.z().filter(|z| *z != 0.0) {
            // The implicit camera scales a layer off the z = 0 plane.
            format!("its layer has z position {z}")
        } else {
            return Self::uniform(
                "its layer's static scale",
                transform.scale,
                transform.rotation,
            );
        };
        Err(format!("{form}; {SIMILARITY_RULE}"))
    }

    /// The similarity of a uniform, positive `scale` (horizontal and vertical
    /// percentages) and a `rotation`, or the reason that `scale`, which
    /// `subject` names, is not one.
    fn uniform(subject: &str, [width, height]: [f64; 2], rotation: f64) -> Result<Self, String> {
        if width != height {
            return Err(format!(
                "{subject} is nonuniform ({width}% by {height}%); {SIMILARITY_RULE}"
            ));
        }
        if width <= 0.0 {
            return Err(format!(
                "{subject} {width}% is not positive; {SIMILARITY_RULE}"
            ));
        }
        Ok(Self {
            scale: width / 100.0,
            rotation,
        })
    }

    /// The FX value of the Directional Blur parameter `param`, in FX units,
    /// for its clip-frame `value`. The map is affine with a positive factor,
    /// so normalized key easing carries over unchanged.
    fn composition_value(self, param: &EffectParamSpec, value: f64) -> f64 {
        if is_blur_length(param) {
            value * self.scale
        } else {
            value + self.rotation
        }
    }

    /// The clip-frame value of the Directional Blur parameter `param` for its
    /// FX `value`, the inverse of [`Self::composition_value`].
    fn clip_value(self, param: &EffectParamSpec, value: f64) -> f64 {
        if is_blur_length(param) {
            value / self.scale
        } else {
            value - self.rotation
        }
    }
}

/// FX `levels` output white per percent of an Invert's Blend With Original:
/// 255 levels per 100 percent, as the reduced ratio 51 : 20, so that every
/// whole-number Blend returns exactly from its FX value (the unreduced
/// quotient rounds 16 of them). Native samples fit Premiere's render of an
/// Invert of every channel with Blend b as (1 − b)(1 − v) + b·v on encoded
/// RGB, and FX `levels` with neutral inputs and Gamma renders
/// mix(outputBlack, outputWhite, v): with output white 255b and output black
/// 255(1 − b) the two are equal by algebra on the same clipped encoded input.
/// Adobe's render differs from that form on saturated bars; the cause (a
/// blend with the unclipped original) is inferred from E5's fit, not
/// established. Both maps are affine, so normalized key easing carries over
/// unchanged.
const INVERT_OUTPUT_WHITE_LEVELS: f64 = 51.0;
const INVERT_BLEND_PERCENT: f64 = 20.0;

/// The FX `levels` white, the top of its output range: the output white of
/// an Invert with a Blend of 100, which shows the original.
const LEVELS_WHITE: f64 = 255.0;

/// The FX output white of an Invert's native Blend With Original `blend`.
fn invert_output_white(blend: f64) -> f64 {
    blend * INVERT_OUTPUT_WHITE_LEVELS / INVERT_BLEND_PERCENT
}

/// The native Blend With Original of the FX output white `output_white`, the
/// inverse of [`invert_output_white`].
fn invert_blend(output_white: f64) -> f64 {
    output_white * INVERT_BLEND_PERCENT / INVERT_OUTPUT_WHITE_LEVELS
}

/// The FX output black of an Invert: the complement of its output white.
pub(super) fn invert_output_black(output_white: f64) -> f64 {
    LEVELS_WHITE - output_white
}

/// The other output of the FX parameter `param` of `payload`, a Levels in
/// Invert's static form ([`invert_levels`]), if it is one: export writes the
/// two outputs as one Invert Blend With Original track when they are keyed
/// as complements, and otherwise as Levels outputs ([`invert_form`]).
pub(super) fn invert_output_partner(payload: &EffectPayload, param: &str) -> Option<&'static str> {
    let EffectPayload::Known(levels @ LayerEffect::Levels { output_white, .. }) = payload else {
        return None;
    };
    if *levels != invert_levels(*output_white) {
        return None;
    }
    match param {
        "outputWhite" => Some("outputBlack"),
        "outputBlack" => Some("outputWhite"),
        _ => None,
    }
}

/// The FX `levels` that renders as Premiere's Invert of every channel with
/// the output white `output_white` ([`invert_output_white`]): neutral inputs
/// and Gamma, and complementary outputs. Export writes a Levels of exactly
/// this form back as an Invert ([`invert_form`]); any other Levels exports as
/// `PR.ADBE Levels`.
pub(super) fn invert_levels(output_white: f64) -> LayerEffect {
    LayerEffect::Levels {
        input_black: 0.0,
        input_white: LEVELS_WHITE,
        gamma: 1.0,
        output_black: invert_output_black(output_white),
        output_white,
    }
}

/// The FX `tintTritone` of a Tint: each colour channel a share of 255 and the
/// Amount unchanged. Premiere's Tint and Black & White map Rec. 601 luma of
/// the encoded values from Map Black To to Map White To and mix the result
/// with the original by the Amount (fitted on AME
/// frames: weights 0.2993, 0.5883, 0.114), which is the `tintTritone` shader.
/// Import writes all seven fields; export reads all seven, because FX renders
/// an absent one at its own default, which the converter does not assume.
fn tint_tritone(tint: PrTint) -> LayerEffect {
    let [black_r, black_g, black_b] = tint.black.fx().map(Some);
    let [white_r, white_g, white_b] = tint.white.fx().map(Some);
    LayerEffect::TintTritone {
        black_r,
        black_g,
        black_b,
        white_r,
        white_g,
        white_b,
        amount: Some(tint.amount),
    }
}

/// A Ramp as FX `gradientRamp`: the endpoints as frame UV, which equal the
/// clip-frame fractions on a canvas-size host, the colours as channel shares,
/// `blend` = 1 − Blend With Original ([`PrRamp::fx_blend`]) and the linear
/// shape.
fn gradient_ramp(ramp: PrRamp) -> LayerEffect {
    let [start_r, start_g, start_b] = ramp.start_colour.fx().map(Some);
    let [end_r, end_g, end_b] = ramp.end_colour.fx().map(Some);
    LayerEffect::GradientRamp {
        start_x: Some(ramp.start[0]),
        start_y: Some(ramp.start[1]),
        end_x: Some(ramp.end[0]),
        end_y: Some(ramp.end[1]),
        start_r,
        start_g,
        start_b,
        end_r,
        end_g,
        end_b,
        blend: Some(PrRamp::fx_blend(ramp.blend)),
        shape: Some(0.0),
    }
}

/// Whether `param` is a Legacy or current Directional Blur parameter, whose
/// values map through the clip's static similarity.
fn is_directional_blur_param(param: &EffectParamSpec) -> bool {
    [
        DIRECTIONAL_BLUR_DIRECTION,
        DIRECTIONAL_BLUR_LENGTH,
        FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE,
        FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
    ]
    .contains(param)
}

/// Whether `param` is a Directional Blur length (Legacy Blur Length or
/// current Amount) rather than an angle.
fn is_blur_length(param: &EffectParamSpec) -> bool {
    [DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT].contains(param)
}

/// The FX value of `param`'s native `value` on a clip placed by `host`: an
/// Invert's Blend With Original is its output white ([`invert_output_white`]),
/// a Ramp's Blend With Original is one minus the FX blend
/// ([`PrRamp::fx_blend`]), each other mapped parameter converts by its binding
/// ([`EffectParamSpec::fx_value`]), and a Directional Blur parameter then maps
/// through the clip's static similarity (see [`ClipToComposition`]).
fn fx_value(param: &EffectParamSpec, value: f64, host: &ImportHost) -> Result<f64, String> {
    if param == &crate::schema::LUMETRI_SATURATION {
        return Ok(value - 100.0);
    }

    if *param == INVERT_BLEND {
        return Ok(invert_output_white(value));
    }
    if *param == RAMP_BLEND {
        return Ok(PrRamp::fx_blend(value));
    }
    let value = param.fx_value(value);
    if !is_directional_blur_param(param) {
        return Ok(value);
    }
    let similarity = host.similarity.as_ref().map_err(String::clone)?;
    Ok(similarity.composition_value(param, value))
}

/// The exported clip that holds an effect stack.
#[derive(Debug, Clone, Copy)]
pub(super) struct EffectHost<'a> {
    /// The FX layer, whose layer-property tracks animate the clip's Motion.
    pub(super) layer: LayerId,
    /// Whether the clip is a still, whose Sharpen/Replicate processing order is unverified.
    pub(super) still: bool,
    /// Whether the clip is a stage group, on which a Directional Blur is
    /// omitted rather than mapped through `layer`'s Motion.
    pub(super) staged: bool,
    /// Whether the clip is a nest placement, or lies in a nest whose placement
    /// writes Motion, so that a Directional Blur is omitted rather than mapped
    /// through `layer`'s Motion alone.
    pub(super) nested: bool,
    /// Actual nest ownership, independent of the placement's Motion.
    pub(super) in_nest: bool,
    /// The layer's FX static transform.
    pub(super) transform: &'a Transform,
    /// The native tick from which the layer's key times count: a video's key
    /// origin (zero for explicit nonunit media-clock playback, unlike Motion),
    /// a nest's zero or an adjustment clip's In.
    pub(super) source_in: i64,
    /// The key clock of a video's own parameter keys: a parameter whose keys
    /// it cannot place keeps its static value. `None` for a nest, an
    /// adjustment layer and a retimed video.
    pub(super) video_keys: Option<VideoKeyClock>,
    /// Why parameter keys have no proven native clock, as under a ramp
    /// approximated at endpoint-average speed, even one, or an explicit Frame
    /// Hold; export then preserves the authored static values.
    pub(super) static_parameters_reason: Option<&'static str>,
    /// The clip's media frame in pixels (a nest's canvas), which a Ramp and a
    /// Replicate need to equal `canvas` ([`EffectHost::frame_is_canvas`]).
    pub(super) frame: [u32; 2],
    /// The sequence canvas in pixels.
    pub(super) canvas: [u32; 2],
}

impl EffectHost<'_> {
    /// Whether this layer's frame is the canvas at identity Motion, as an
    /// effect measured in that frame needs, or why not: on a stage group or
    /// a nest the effect's `staged` reason, otherwise the frame's reason
    /// followed by the effect's `rule` ([`FRAME_RULE`],
    /// [`REPLICATE_FRAME_RULE`]). The Motion export writes the static
    /// transform unchanged when it is the identity, so `written` needs no check.
    fn frame_is_canvas(
        &self,
        dynamics: &AnimationGraph,
        staged: &str,
        rule: &str,
    ) -> Result<(), String> {
        if self.staged || self.nested {
            return Err(staged.to_owned());
        }
        let motion_animated = dynamics.entries().iter().any(|entry| {
            matches!(&entry.target, PropertyTarget::LayerProperty(target)
                if target.layer_id() == self.layer
                    && target.property_type() != PropType::Opacity)
        });
        let transform = self.transform;
        match super::premiere_to_tesseract::wipe_frame_reason(
            self.frame,
            self.canvas,
            transform.scale,
            transform.rotation,
            transform.anchor_point == transform.position.xy_array(),
            motion_animated,
        ) {
            None => Ok(()),
            Some(reason) => Err(format!("{reason}; {rule}")),
        }
    }
}

/// Composition-unique FX effect ids, minted in import order.
#[derive(Debug)]
pub(super) struct EffectIdAllocator {
    next: u64,
}

impl Default for EffectIdAllocator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl EffectIdAllocator {
    pub(super) fn take(&mut self) -> EffectId {
        let id = EffectId::new(self.next);
        self.next += 1;
        id
    }

    /// The id that [`Self::take`] returns next.
    pub(super) fn next(&self) -> u64 {
        self.next
    }

    /// Skips the ids before `next`, which other content of the composition uses.
    pub(super) fn skip_to(&mut self, next: u64) {
        self.next = self.next.max(next);
    }
}

/// One coverage check for every picture host, after both source and placement
/// stacks have been imported. Generic key/record failures cannot leave an opaque
/// substitute. The native reader separately owns omissions before typed effects.
pub(super) fn retains_coverage(
    clip: &PrVideoOccurrence,
    imported: &[EffectRecord],
    omissions: &mut Vec<Omission>,
) -> bool {
    if clip
        .effects
        .iter()
        .chain(clip.source_effects.iter().flat_map(|stack| &stack.effects))
        .any(|effect| effect.mask.is_some())
    {
        omit(
            omissions,
            OmissionScope::Occurrence,
            clip.record(),
            "effect mask requires an isolated supported effect scope; occurrence omitted",
        );
        return false;
    }
    let expected = clip
        .effects
        .iter()
        .chain(clip.source_effects.iter().flat_map(|s| &s.effects))
        .filter(|e| e.requires_coverage())
        .count();
    let retained = imported
        .iter()
        .filter(|record| {
            matches!(
                record.data(),
                EffectData::Identified {
                    enabled: true,
                    effect: EffectPayload::Known(LayerEffect::LumaKey { .. }),
                    ..
                } | EffectData::Legacy(EffectPayload::Known(LayerEffect::LumaKey { .. }))
            )
        })
        .count();
    if retained == expected {
        return true;
    }
    for (source, stack) in std::iter::once((None, clip.effects.as_slice())).chain(
        clip.source_effects
            .iter()
            .map(|s| (Some(s.master.as_str()), s.effects.as_slice())),
    ) {
        for (position, effect) in (1..).zip(stack) {
            if effect.requires_coverage() {
                let owner = source.map_or_else(
                    || "placement".to_owned(),
                    |master| format!("source {master}"),
                );
                omit(omissions, OmissionScope::Occurrence, clip.record(), format!("{} ({}) at {owner} stack position {position}: not all enabled keys survived effect/animation conversion on this host; coverage would change, so the occurrence is omitted", effect.spec().display_name, effect.spec().match_name));
            }
        }
    }
    false
}

/// The effects of an occurrence's video layer in stack order, with
/// composition-unique ids, and the keyframe tracks of their keyed parameters on
/// layer `layer_id`. A staged clip's group carries its Crop or Linear Wipe but
/// no effects, so its video layer takes only the effects that apply before the
/// mask, and the ones that apply after it are reported. Legacy Luma stays on the
/// picture with a diagnosed order approximation rather than losing coverage. A Directional Blur on
/// a staged clip is omitted ([`STAGED_DIRECTIONAL_BLUR_REASON`]), and so are a
/// Ramp ([`STAGED_RAMP_REASON`]) and a Replicate
/// ([`STAGED_REPLICATE_REASON`]); both also need the clip's `source` frame to
/// be the `canvas` ([`FRAME_RULE`], [`REPLICATE_FRAME_RULE`]), and each
/// converted Replicate is reported as an approximation
/// ([`PrReplicate::TILING_APPROXIMATION`]). A clip staged for its one Transform
/// ([`PrVideoOccurrence::transform_stage`]) takes the effects that apply
/// before it; the Transform itself is the staged video's transform, not an
/// effect, and the effects that apply after it are reported. A Transform that
/// stages nothing is reported with the stage's reason, and one on a flat host
/// (an adjustment layer) with the host reason. The picture layer's FX effects
/// measure `effects_frame` pixels: a Corner Pin, Mosaic or Blur that repeats
/// edge pixels needs that to be the clip's `source` frame
/// ([`PICTURE_FRAME_RULE`]). On a clip that plays Time Remapping from another
/// In or speed ([`PrVideoOccurrence::remaps_from_in_or_speed`]), each keyed
/// parameter keeps its static value and its keys are reported.
/// Sharpen additionally excludes all nested hosts: the direct clip's frame
/// does not establish processing order through an enclosing placement.
#[expect(
    clippy::too_many_arguments,
    reason = "the clip, layer, mask boundary, native owner kind, frames, ids and omission sink"
)]
pub(super) fn import_effects(
    clip: &PrVideoOccurrence,
    layer_id: LayerId,
    media_clock_keys: bool,
    boundary: MaskBoundary,
    nested: bool,
    still: bool,
    source_kind: PrMediaKind,
    source: [u32; 2],
    effects_frame: [u32; 2],
    canvas: [u32; 2],
    ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> (
    Vec<EffectRecord>,
    Vec<(PropertyTarget, PropertyKeyframeTrack)>,
) {
    let stage = clip.transform_stage(source, canvas);
    let (carried, after) = match (boundary, stage) {
        (MaskBoundary::Flat, _) => (clip.effects.len(), ""),
        (MaskBoundary::Staged, Ok(Some((index, _, _)))) => (
            index,
            "the Transform, which the stage group's video carries as its transform, and that stage carries no effects",
        ),
        (MaskBoundary::Staged, Ok(None) | Err(_)) => (
            clip.effects_above_mask,
            "the Crop or Linear Wipe, which a group carries with the clip's Motion, and that group carries no effects",
        ),
    };
    let mut effects = Vec::with_capacity(carried);
    let mut tracks = Vec::new();
    let host = ImportHost::of_clip(clip, boundary, source, effects_frame, canvas);
    for (position, effect) in (1..).zip(&clip.effects) {
        // Alpha replacement needs an occurrence graph, not a scalar RGB effect.
        if matches!(
            effect.params,
            PrEffectParams::Invert(PrInvert { channel: 15, .. })
        ) {
            if !matches!(
                source_kind,
                PrMediaKind::Video { .. }
                    | PrMediaKind::Still { .. }
                    | PrMediaKind::AfterEffectsComposition(_)
            ) {
                omit(omissions, OmissionScope::Feature, clip.record(), "Invert Alpha requires an ordinary image/video occurrence; generator and numbered-image stacks retain their original picture without Alpha replacement");
            }
            continue;
        }
        if matches!(
            effect.params,
            PrEffectParams::Levels(PrLevels::Corrections(_))
        ) && matches!(
            source_kind,
            PrMediaKind::Video { .. } | PrMediaKind::Still { .. }
        ) {
            continue;
        }
        if boundary == MaskBoundary::Staged
            && matches!(stage, Ok(Some((index, _, _))) if index + 1 == position)
        {
            continue;
        }
        if position > carried && effect.requires_coverage() {
            approximate(omissions, clip.record(), format!("{} ({}) at stack position {position}: retained on the picture before {after} to preserve coverage; native effect/mask order is approximated", effect.spec().display_name, effect.spec().match_name));
        } else if position > carried {
            let state = if effect.enabled { "" } else { "bypassed " };
            omit(
                omissions,
                OmissionScope::Feature,
                clip.record(),
                format!(
                    "{state}{} effect at stack position {position} was not imported: it applies after {after}",
                    effect.spec().display_name
                ),
            );
            continue;
        }
        let imported = match (&effect.params, stage) {
            (PrEffectParams::Sharpen(_), _) if still => Err(STILL_SHARPEN_REASON.to_owned()),
            (PrEffectParams::Sharpen(_), _) if nested => Err(SHARPEN_HOST_RULE.to_owned()),
            (PrEffectParams::Replicate(_), _) if still => Err(STILL_REPLICATE_REASON.to_owned()),
            (PrEffectParams::PosterizeTime { frame_rate }, _) => {
                posterize_time_rate_at_in(*frame_rate, &effect.animations, clip.in_ticks)
                    .and_then(|rate| import_posterize_time(rate, clip, boundary, source_kind))
            }
            (PrEffectParams::Transform(_), Err(reason)) => Err(reason.to_owned()),
            _ => layer_effect(effect, &host),
        }
        .and_then(|layer_effect| {
            // Only the picture's clock is measured under such a remap.
            let alpha_clock_reason = matches!(effect.params, PrEffectParams::AlphaGlow { .. })
                .then(|| super::premiere_to_tesseract::retimed_keys_reason(clip)).flatten();
            let remapped = clip.remaps_from_in_or_speed() || alpha_clock_reason.is_some();
            let animations: &[PrEffectParamAnimation] = if remapped
                || matches!(effect.params, PrEffectParams::PosterizeTime { .. })
            {
                &[]
            } else {
                &effect.animations
            };
            let imported = effect_record(
                effect,
                layer_effect,
                animations,
                if media_clock_keys { 0 } else { clip.in_ticks },
                layer_id,
                &host,
                ids,
                clip.record(),
                omissions,
            )?;
            if media_clock_keys && !animations.is_empty() {
                approximate(omissions, clip.record(), format!(
                    "{} effect at stack position {position}: {RETIMED_EFFECT_CLOCK_APPROXIMATION}",
                    effect.spec().display_name));
            }
            if remapped {
                for animation in &effect.animations {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        clip.record(),
                        format!(
                            "{} effect at stack position {position}: {} keys were not imported: {}; the static value was kept",
                            effect.spec().display_name,
                            animation.param.label,
                            alpha_clock_reason.unwrap_or("their clock under Time Remapping from a source In or at another speed is unmeasured")
                        ),
                    );
                }
            }
            Ok(imported)
        });
        match imported {
            Ok((record, effect_tracks)) => {
                report_imported_effect(effect, position, clip.record(), omissions);
                if matches!(effect.params, PrEffectParams::PosterizeTime { .. }) {
                    if !effect.animations.is_empty() {
                        omit(
                            omissions,
                            OmissionScope::Feature,
                            clip.record(),
                            format!("Posterize Time effect at stack position {position}: Frame Rate animation was not imported; FX frameRate is not animatable; the rate at the clip's source In was kept, not CurrentValue"),
                        );
                    }
                    if effect.enabled {
                        approximate(
                            omissions,
                            clip.record(),
                            format!("Posterize Time effect at stack position {position}: {POSTERIZE_TIME_CLOCK_APPROXIMATION}"),
                        );
                    }
                }
                effects.push(record);
                tracks.extend(effect_tracks);
            }
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                clip.record(),
                format!(
                    "{} effect at stack position {position} was not imported: {error}",
                    effect.spec().display_name
                ),
            ),
        }
    }
    (effects, tracks)
}

/// Occurrence effects on the identity picture Group below a nest's Motion.
/// Group effect bounds are not the native canvas, so frame-dependent mappings
/// retain their existing stage restrictions. Retimed keys keep static values;
/// this does not extend the nest's picture or property clocks.
pub(super) fn import_nest_effects(
    nest: &crate::schema::PrNestOccurrence,
    layer_id: LayerId,
    key_origin: i64,
    ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> (
    Vec<EffectRecord>,
    Vec<(PropertyTarget, PropertyKeyframeTrack)>,
) {
    let host = ImportHost {
        similarity: Err(STAGED_DIRECTIONAL_BLUR_REASON.to_owned()),
        off_canvas: None,
        staged: true,
        own_frame: Err(format!(
            "a nested picture Group has no fixed native canvas bounds; {PICTURE_FRAME_RULE}"
        )),
    };
    let owner = nest.record();
    // Validated unit reverse has a separate, increasing occurrence-effect
    // stage; only its picture descendants run on the decreasing source clock.
    let retimed_keys = nest.is_retimed() && nest.playback_rate != -1.0;
    let mut effects = Vec::with_capacity(nest.effects.len());
    let mut tracks = Vec::new();
    for (position, effect) in (1..).zip(&nest.effects) {
        let animations = if retimed_keys {
            &[][..]
        } else {
            &effect.animations
        };
        let imported = layer_effect(effect, &host).and_then(|mapped| {
            effect_record(
                effect, mapped, animations, key_origin, layer_id, &host, ids, &owner, omissions,
            )
        });
        match imported {
            Ok((record, keys)) => {
                report_imported_effect(effect, position, &owner, omissions);
                if matches!(
                    effect.params,
                    PrEffectParams::LumetriVignette(_)
                        | PrEffectParams::Offset(_)
                        | PrEffectParams::Noise { .. }
                        | PrEffectParams::ModernNoise { .. }
                        | PrEffectParams::AlphaGlow { .. }
                ) {
                    approximate(omissions, &owner, format!("{} effect at stack position {position}: editable picture-Group bounds replace the native nested canvas; spatial falloff, tiling or noise coverage may differ", effect.spec().display_name));
                }
                for animation in effect.animations.iter().filter(|_| retimed_keys) {
                    omit(omissions, OmissionScope::Feature, &owner,
                        format!("{} effect at stack position {position}: {} keys were not imported: keys on a retimed nested sequence occurrence are not converted; the static value was kept", effect.spec().display_name, animation.param.label));
                }
                if nest.playback_rate == -1.0 && !effect.animations.is_empty() {
                    approximate(omissions, &owner, format!("{} effect at stack position {position}: reverse nested effect keys retained on the increasing occurrence clock, separate from decreasing descendant playback; native effect-clock fidelity is unmeasured", effect.spec().display_name));
                }
                effects.push(record);
                tracks.extend(keys);
            }
            Err(reason) => omit(
                omissions,
                OmissionScope::Feature,
                &owner,
                format!(
                    "{} effect at stack position {position} was not imported: {reason}",
                    effect.spec().display_name
                ),
            ),
        }
    }
    (effects, tracks)
}

fn report_imported_effect(
    effect: &PrEffect,
    position: usize,
    owner: &str,
    omissions: &mut Vec<Omission>,
) {
    let state = if effect.enabled { "" } else { "bypassed " };
    if matches!(effect.params, PrEffectParams::Replicate(_)) {
        approximate(
            omissions,
            owner,
            format!(
                "{state}{} effect at stack position {position} converts approximately: {}",
                effect.spec().display_name,
                PrReplicate::TILING_APPROXIMATION
            ),
        );
    }
    if let Some(reason) = effect_approximation(&effect.params) {
        approximate(
            omissions,
            owner,
            format!(
                "{state}{} effect at stack position {position} converts approximately: {reason}",
                effect.spec().display_name
            ),
        );
    }
    if matches!(
        effect.params,
        PrEffectParams::Noise { .. } | PrEffectParams::ModernNoise { .. }
    ) {
        approximate(
            omissions,
            owner,
            format!("Noise effect at stack position {position}: {NOISE_APPROXIMATION}"),
        );
    }
    if let PrEffectParams::FindEdges(edges) = effect.params {
        approximate(
            omissions,
            owner,
            format!(
                "Find Edges effect at stack position {position} converts approximately: {}",
                PrFindEdges::APPROXIMATION
            ),
        );
        if edges.blend != 0.0 || edges.blend_animated {
            omit(
                omissions,
                OmissionScope::Feature,
                owner,
                format!(
                    "Find Edges effect at stack position {position}: {}",
                    PrFindEdges::BLEND_OMISSION
                ),
            );
        }
    }
}

/// The source effects of `clip`'s master clip
/// ([`PrVideoOccurrence::source_effects`]) on its picture layer `layer_id`,
/// with composition-unique ids and the key tracks of their keyed parameters;
/// the caller puts them before the clip's own effects ([`import_effects`]).
///
/// Premiere applies them to the source, before the placement's whole
/// pipeline: the pinned `premiere_isolated_source_effects_26_5` render shows
/// a source Corner Pin before the placement's Motion, on the source clock.
/// The picture layer draws its effects before its transform, with key times
/// on the placement's clock from its source In, where Premiere draws both
/// stacks of a unit-speed placement; so the source effects convert at the
/// front of that one stack, and the caller stages the clip's mask after them
/// ([`PrVideoOccurrence::mask_boundary`]). They take the host of the clip's
/// own effects applied before its mask ([`ImportHost::of_clip`]): a
/// Directional Blur maps through its Motion, a Ramp, Mosaic or Directional
/// Blur keeps the rules of a flat or staged picture, and a Corner Pin,
/// Mosaic or Blur that repeats edge pixels needs the picture's FX layer to
/// lay its effects over the clip's own frame, which a video layer does and a
/// linked composition's group of another size than the canvas does not
/// ([`PICTURE_FRAME_RULE`]). Each placement converts its own copy with new ids, its keys
/// moved to its clock with those outside its trim kept ([`param_tracks`]);
/// constant-rate/reverse physical videos can instead keep absolute media-key
/// times, because their existing FX playback drives the effect clock. Other
/// retimed owners retain the static fallback. This inferred native clock is
/// diagnosed rather than claimed as render-proven. A Corner Pin
/// corner's curved spatial path converts as straight Linear keys within a
/// certified bound, reported as an approximation
/// ([`super::corner_path::straighten`]). An effect that
/// does not convert, a Transform always ([`SOURCE_TRANSFORM_REASON`]), is
/// reported for this placement and left out; the reader omits the placement
/// instead when a source Transform could hide it
/// ([`crate::schema::PrSourceEffects`]).
#[expect(
    clippy::too_many_arguments,
    reason = "the clip, layer, mask boundary, native owner kind, frames, ids and omission sink, as for `import_effects`"
)]
pub(super) fn import_source_effects(
    clip: &PrVideoOccurrence,
    layer_id: LayerId,
    media_clock_keys: bool,
    boundary: MaskBoundary,
    source: [u32; 2],
    effects_frame: [u32; 2],
    canvas: [u32; 2],
    ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> (
    Vec<EffectRecord>,
    Vec<(PropertyTarget, PropertyKeyframeTrack)>,
) {
    let mut effects = Vec::new();
    let mut tracks = Vec::new();
    let Some(stack) = &clip.source_effects else {
        return (effects, tracks);
    };
    let host = ImportHost::of_clip(clip, boundary, source, effects_frame, canvas);
    let retimed_keys =
        super::premiere_to_tesseract::retimed_keys_reason(clip).filter(|_| !media_clock_keys);
    let key_origin = if media_clock_keys { 0 } else { clip.in_ticks };
    for (position, effect) in (1..).zip(&stack.effects) {
        let name = effect.spec().display_name;
        let at = format!("source stack position {position} of {}", stack.master);
        // Straighten on the selected property clock: ordinary placement time
        // or physical media time. Unsupported remapped owners stay static.
        let straightened = match retimed_keys {
            Some(_) => None,
            None => match super::corner_path::straighten(effect, source, key_origin) {
                Ok(straightened) => straightened,
                Err(reason) => {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        clip.record(),
                        format!("{name} effect at {at} was not imported: {reason}"),
                    );
                    continue;
                }
            },
        };
        let effect = straightened.as_ref().map_or(effect, |pin| &pin.effect);
        let animations = match retimed_keys {
            Some(_) => &[],
            None => effect.animations.as_slice(),
        };
        let imported = match &effect.params {
            PrEffectParams::Sharpen(_) => Err(
                "Sharpen among a master clip's source effects is not converted; its processing order is unverified".to_owned(),
            ),
            PrEffectParams::Replicate(_) => Err(
                "Replicate among a master clip's source effects is not converted; its processing order is unverified".to_owned(),
            ),
            PrEffectParams::Transform(_) | PrEffectParams::AdjustmentGeometry2(_) => Err(SOURCE_TRANSFORM_REASON.to_owned()),
            PrEffectParams::PosterizeTime { .. } => Err(
                "source Posterize Time was not converted: FX holds the placement clock, not the master clip's source clock".to_owned(),
            ),
            _ => layer_effect(effect, &host),
        }
        .and_then(|layer_effect| {
            effect_record(
                effect,
                layer_effect,
                animations,
                key_origin,
                layer_id,
                &host,
                ids,
                clip.record(),
                omissions,
            )
        });
        match imported {
            Ok((record, effect_tracks)) => {
                if media_clock_keys && !animations.is_empty() {
                    approximate(
                        omissions,
                        clip.record(),
                        format!(
                            "{name} effect animation at {at}: {RETIMED_EFFECT_CLOCK_APPROXIMATION}"
                        ),
                    );
                }
                if let Some(reason) = retimed_keys.filter(|_| !effect.animations.is_empty()) {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        clip.record(),
                        format!(
                            "{name} effect animation at {at} was not imported: {reason}; static values were kept"
                        ),
                    );
                }
                if let Some(pin) = &straightened {
                    approximate(
                        omissions,
                        clip.record(),
                        pin.report(&format!("{name} effect at {at}")),
                    );
                }
                if let Some(reason) = effect_approximation(&effect.params) {
                    let state = if effect.enabled { "" } else { "bypassed " };
                    approximate(
                        omissions,
                        clip.record(),
                        format!(
                            "{state}{name} effect at {at} converts approximately: {}",
                            reason
                        ),
                    );
                }
                if matches!(
                    effect.params,
                    PrEffectParams::Noise { .. } | PrEffectParams::ModernNoise { .. }
                ) {
                    approximate(
                        omissions,
                        clip.record(),
                        format!("Noise effect at {at}: {NOISE_APPROXIMATION}"),
                    );
                }
                if let PrEffectParams::FindEdges(edges) = effect.params {
                    approximate(
                        omissions,
                        clip.record(),
                        format!(
                            "Find Edges effect at {at} converts approximately: {}",
                            PrFindEdges::APPROXIMATION
                        ),
                    );
                    if edges.blend != 0.0 || edges.blend_animated {
                        omit(
                            omissions,
                            OmissionScope::Feature,
                            clip.record(),
                            format!("Find Edges effect at {at}: {}", PrFindEdges::BLEND_OMISSION),
                        );
                    }
                }
                effects.push(record);
                tracks.extend(effect_tracks);
            }
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                clip.record(),
                format!("{name} effect at {at} was not imported: {error}"),
            ),
        }
    }
    (effects, tracks)
}

/// Reports, once per master clip, what import made of the source effects of
/// `clip`, whose layer converts: the linked editing that each placement's own
/// copy loses ([`LINKED_SOURCE_EDITING_REASON`]) when `converted` holds for
/// one of them, and otherwise the chain as not converted
/// ([`SOURCE_CHAIN_NOT_CONVERTED`]). Both can hold for one master clip.
pub(super) fn report_source_effects(
    clip: &PrVideoOccurrence,
    converted: bool,
    omissions: &mut Vec<Omission>,
) {
    let Some(stack) = &clip.source_effects else {
        return;
    };
    let reason = if converted {
        LINKED_SOURCE_EDITING_REASON
    } else {
        SOURCE_CHAIN_NOT_CONVERTED
    };
    omit(omissions, OmissionScope::Feature, &stack.master, reason);
}

/// The record of `effect`, drawn as `layer_effect`, with a new id from `ids`,
/// and the tracks of its keyed parameters `animations` on layer `layer_id`,
/// whose clock counts from `source_in` ([`param_tracks`]).
#[allow(
    clippy::too_many_arguments,
    reason = "effect identity, owner clock and diagnostic context remain explicit"
)]
fn effect_record(
    effect: &PrEffect,
    layer_effect: LayerEffect,
    animations: &[PrEffectParamAnimation],
    source_in: i64,
    layer_id: LayerId,
    host: &ImportHost,
    ids: &mut EffectIdAllocator,
    owner: &str,
    omissions: &mut Vec<Omission>,
) -> Result<(EffectRecord, Vec<(PropertyTarget, PropertyKeyframeTrack)>), String> {
    let id = ids.take();
    let record = EffectRecord::from_data(&EffectData::Identified {
        id,
        enabled: effect.enabled,
        effect: EffectPayload::Known(layer_effect),
    })
    .map_err(|error| error.to_string())?;
    let mut tracks = Vec::new();
    for animation in animations {
        match param_tracks(animation, id, source_in, layer_id, host) {
            Ok(imported) => tracks.extend(imported),
            Err(reason) if matches!(effect.params, PrEffectParams::AlphaGlow { .. }) => {
                approximate(
                    omissions,
                    owner,
                    format!(
                        "Alpha Glow {} animation flattened to its first saved value: {reason}",
                        animation.param.label
                    ),
                );
            }
            Err(reason) => return Err(reason),
        }
    }
    Ok((record, tracks))
}

/// The FX keyframe tracks of one keyed parameter, one per FX parameter of its
/// binding. As for Motion keys, times move from the source clock to the layer
/// clock and keys outside the trimmed range stay. Scalar values map as the
/// static ones do (see [`fx_value`]).
fn param_tracks(
    animation: &PrEffectParamAnimation,
    effect_id: EffectId,
    source_in: i64,
    layer_id: LayerId,
    host: &ImportHost,
) -> Result<Vec<(PropertyTarget, PropertyKeyframeTrack)>, String> {
    let label = animation.param.label;
    let track = |fx_param: &str, keys: Vec<(i64, f64, PrKeyframeEasing)>| {
        // The effect id keeps the key ids of two keyed effects on one layer apart.
        let track_name = format!("effect-{}-{fx_param}", effect_id.value());
        let keys = keys
            .into_iter()
            .enumerate()
            .map(|(index, (source_ticks, value, easing))| {
                let millis = keyframes::layer_millis(source_ticks, source_in)
                    .map_err(|error| error.to_string())?;
                Ok(PropertyKeyframe::new(
                    super::premiere_to_tesseract::keyframe_id(layer_id, &track_name, index),
                    TimeOffset::from_millis(millis),
                    PropertyValue::Float(value),
                    keyframes::fx_easing(easing),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        // Never collapse distinct native times that round to the same millisecond.
        let track = PropertyKeyframeTrack::new(keys).map_err(|error| {
            format!("Premiere {label} keyframe times/values cannot be imported: {error}")
        })?;
        Ok((PropertyTarget::effect_param(effect_id, fx_param), track))
    };
    match (&animation.keys, animation.param.binding) {
        (
            PrEffectParamKeys::Scalar(keys),
            Some(
                EffectParamBinding::Scalar(fx_param)
                | EffectParamBinding::ScaledScalar { name: fx_param, .. }
                | EffectParamBinding::Integer { name: fx_param, .. },
            ),
        ) => {
            let keys: Vec<_> = keys
                .iter()
                .map(|key| {
                    let value = fx_value(animation.param, key.value, host)?;
                    Ok((key.source_ticks, value, key.easing))
                })
                .collect::<Result<_, String>>()?;
            // An Invert's Blend also keys the output black, the complement of
            // the output white at every key (see `invert_levels`).
            let complements = (*animation.param == INVERT_BLEND).then(|| {
                keys.iter()
                    .map(|&(source_ticks, value, easing)| {
                        (source_ticks, invert_output_black(value), easing)
                    })
                    .collect()
            });
            let mut tracks = vec![track(fx_param, keys)?];
            if let Some(complements) = complements {
                tracks.push(track("outputBlack", complements)?);
            }
            Ok(tracks)
        }
        // Along a straight spatial path both coordinates follow the point
        // key's temporal easing. The reader keeps a curved one only for a
        // source Corner Pin, which `corner_path::straighten` turns into
        // straight keys first; a curved one here would lose its shape.
        (PrEffectParamKeys::Point(keys), Some(EffectParamBinding::Point { x, y })) => {
            if crate::schema::spatial::curved_segment(keys).is_some() {
                return Err(format!(
                    "keyframed {label} moves on a curved spatial path that was not straightened"
                ));
            }
            [x, y]
                .into_iter()
                .enumerate()
                .map(|(axis, fx_param)| {
                    let keys = keys
                        .iter()
                        .map(|key| (key.source_ticks, key.value[axis], key.easing))
                        .collect();
                    track(fx_param, keys)
                })
                .collect()
        }
        // Premiere interpolates each channel of a colour on its own (E6), as
        // the three FX channel tracks do.
        (
            PrEffectParamKeys::Colour(keys),
            Some(EffectParamBinding::Colour { red, green, blue }),
        ) => [red, green, blue]
            .into_iter()
            .enumerate()
            .map(|(channel, fx_param)| {
                let keys = keys
                    .iter()
                    .map(|key| (key.source_ticks, key.value.fx()[channel], key.easing))
                    .collect();
                track(fx_param, keys)
            })
            .collect(),
        // A Count keys its four tile fields at the same times with the same
        // easing; the reader keeps only Hold keys, so they change together.
        (
            PrEffectParamKeys::Scalar(keys),
            Some(EffectParamBinding::TileCount {
                width,
                height,
                center_x,
                center_y,
            }),
        ) => {
            let field_track = |fx_param: &str, of_count: fn(f64) -> f64| {
                let keys = keys
                    .iter()
                    .map(|key| (key.source_ticks, of_count(key.value), key.easing))
                    .collect();
                track(fx_param, keys)
            };
            Ok(vec![
                field_track(width, PrReplicate::tile_size)?,
                field_track(height, PrReplicate::tile_size)?,
                field_track(center_x, PrReplicate::tile_center)?,
                field_track(center_y, PrReplicate::tile_center)?,
            ])
        }
        _ => Err(format!("keyframed {label} has no FX parameter")),
    }
}

/// Report each effect of an occurrence of `kind` whose layer imports none, a
/// Color Matte's Rect layer; the layer itself still converts.
pub(super) fn omit_effects(clip: &PrVideoOccurrence, kind: &str, omissions: &mut Vec<Omission>) {
    omit_stroke(clip, kind, omissions);
    omit_each_effect(
        clip,
        &format!("effects on a {kind} are not converted"),
        omissions,
    );
}

/// Report the Film Impact Stroke of an occurrence of `kind`, which only an
/// opaque physical video carries.
fn omit_stroke(clip: &PrVideoOccurrence, kind: &str, omissions: &mut Vec<Omission>) {
    if clip.stroke.is_some() {
        omit(
            omissions,
            OmissionScope::Feature,
            clip.record(),
            format!("Film Impact Stroke was not imported: {kind} is not an opaque physical video"),
        );
    }
}

/// Report each effect of `clip` as not imported for `reason`.
fn omit_each_effect(clip: &PrVideoOccurrence, reason: &str, omissions: &mut Vec<Omission>) {
    for (position, effect) in (1..).zip(&clip.effects) {
        let state = if effect.enabled { "" } else { "bypassed " };
        omit(
            omissions,
            OmissionScope::Feature,
            clip.record(),
            format!(
                "{state}{} effect at stack position {position} was not imported: {reason}",
                effect.spec().display_name
            ),
        );
    }
}

/// The effects of a still image occurrence on its image layer `layer_id`,
/// which FX draws in the still's own `source` frame on a `canvas`, and the
/// keyframe tracks of their keyed parameters: those of a flat clip
/// ([`import_effects`]), in stack order, after the still's Crop or Opacity
/// mask, which FX applies first. A still imports no stage group, so when
/// Premiere applies its effects before that mask
/// ([`PrVideoOccurrence::mask_boundary`]) none is imported, and neither are
/// the effects of a `matte` still, which a Track Matte Key uses as its matte
/// and which export writes only at its defaults; each such effect is reported,
/// and so is a Film Impact Stroke. A stack containing Legacy Luma instead stays
/// editable after the mask, with a diagnosed order approximation.
#[expect(
    clippy::too_many_arguments,
    reason = "the still's native owner kind keeps clock effects off the physical-video mapping"
)]
pub(super) fn import_still_effects(
    clip: &PrVideoOccurrence,
    layer_id: LayerId,
    matte: bool,
    source_kind: PrMediaKind,
    source: [u32; 2],
    canvas: [u32; 2],
    ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> (
    Vec<EffectRecord>,
    Vec<(PropertyTarget, PropertyKeyframeTrack)>,
) {
    omit_stroke(clip, "still image", omissions);
    let mask = if !clip.crop.is_default() {
        Some("Crop")
    } else {
        clip.opacity_mask.as_ref().map(|_| "Opacity mask")
    };
    let reason = if matte {
        Some("the still is a Track Matte Key's matte, which converts without effects: a key over a matte still with effects is unmeasured".to_owned())
    } else {
        mask.filter(|_| clip.mask_boundary(source, canvas, 0) != Ok(MaskBoundary::Flat))
            .map(|mask| {
                format!("it applies before the still's {mask}, which FX applies before the image layer's effects, and a still imports no stage group")
            })
    };
    if let Some(reason) = reason {
        if clip.effects.iter().any(PrEffect::requires_coverage) {
            approximate(omissions, clip.record(), format!("{reason}; retain the editable stack after the still mask to preserve Legacy Luma coverage, approximating native order"));
        } else {
            omit_each_effect(clip, &reason, omissions);
            return (Vec::new(), Vec::new());
        }
    }
    import_effects(
        clip,
        layer_id,
        false,
        MaskBoundary::Flat,
        false,
        true,
        source_kind,
        source,
        source,
        canvas,
        ids,
        omissions,
    )
}

fn effect_approximation(params: &PrEffectParams) -> Option<&'static str> {
    match params {
        PrEffectParams::AlphaGlow { .. } => Some(ALPHA_GLOW_APPROXIMATION),
        PrEffectParams::LegacyLuma { .. } => Some(LEGACY_LUMA_KEY_MAPPING_REASON),
        PrEffectParams::Posterize(_) => Some(PrPosterize::QUANTIZER_APPROXIMATION),
        PrEffectParams::Sharpen(_) => Some(PrSharpen::KERNEL_APPROXIMATION),
        PrEffectParams::LensDistortion(_) => Some(LENS_APPROXIMATION),
        _ => None,
    }
}

fn layer_effect(effect: &PrEffect, host: &ImportHost) -> Result<LayerEffect, String> {
    if effect.mask.is_some() {
        return Err("effect mask requires an isolated supported effect scope".into());
    }
    match effect.params {
        PrEffectParams::AlphaGlow {
            size,
            brightness,
            color,
        } => {
            // Unlike a UV-grid effect, this surrogate follows the current alpha
            // silhouette. Its intrinsic radius is approximate under transforms.
            let [r, g, b] = color.fx();
            Ok(LayerEffect::OuterGlow(fx_schema::OuterGlowStyle {
                enabled: true,
                color: [r, g, b, brightness / 255.0],
                size: NonNegativeProperty::new(size).ok_or("invalid Alpha Glow size")?,
                spread: 0.0,
                range: 0.5,
                blend_mode: fx_schema::BlendMode::Normal,
            }))
        }
        // The layer bounds the repeated edge pixels.
        PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels,
        }) => {
            if repeat_edge_pixels {
                host.own_frame.as_ref().map_err(String::clone)?;
            }
            Ok(LayerEffect::GaussianBlur {
                blurriness: NonNegativeProperty::new(blurriness)
                    .ok_or("Blurriness must be finite and nonnegative")?,
                // FX reads an absent flag as unchecked; import keeps that canonical form.
                repeat_edge_pixels: repeat_edge_pixels.then_some(true),
                layer_size: None,
            })
        }
        PrEffectParams::FilmImpactBlur(PrFilmImpactBlur {
            amount,
            repeat_edge_pixels,
        }) => {
            if repeat_edge_pixels {
                host.own_frame.as_ref().map_err(String::clone)?;
            }
            Ok(LayerEffect::GaussianBlur {
                blurriness: NonNegativeProperty::new(FILM_IMPACT_BLUR_AMOUNT.fx_value(amount))
                    .ok_or("Amount must be finite and nonnegative")?,
                repeat_edge_pixels: repeat_edge_pixels.then_some(true),
                layer_size: None,
            })
        }
        // Both normalize each corner to the clip's own frame, which FX takes
        // to be the frame over which the layer lays its effects.
        PrEffectParams::CornerPin(PrCornerPin {
            corners: [upper_left, upper_right, lower_left, lower_right],
        }) => host.own_frame.clone().map(|()| LayerEffect::CornerPin {
            upper_left_x: upper_left[0],
            upper_left_y: upper_left[1],
            upper_right_x: upper_right[0],
            upper_right_y: upper_right[1],
            lower_left_x: lower_left[0],
            lower_left_y: lower_left[1],
            lower_right_x: lower_right[0],
            lower_right_y: lower_right[1],
        }),
        PrEffectParams::DirectionalBlur(PrDirectionalBlur {
            direction,
            blur_length,
        }) => Ok(LayerEffect::DirectionalBlur {
            direction: fx_value(&DIRECTIONAL_BLUR_DIRECTION, direction, host)?,
            blur_length: NonNegativeProperty::new(fx_value(
                &DIRECTIONAL_BLUR_LENGTH,
                blur_length,
                host,
            )?)
            .ok_or("Blur Length must be finite and nonnegative")?,
        }),
        PrEffectParams::FilmImpactDirectionalBlur(PrFilmImpactDirectionalBlur {
            angle,
            amount,
        }) => Ok(LayerEffect::DirectionalBlur {
            direction: fx_value(&FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, angle, host)?,
            blur_length: NonNegativeProperty::new(fx_value(
                &FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
                amount,
                host,
            )?)
            .ok_or("Amount must be finite and nonnegative")?,
        }),
        PrEffectParams::Levels(PrLevels::Channels(channels)) => {
            let [take_red_from, take_green_from, take_blue_from] =
                std::array::from_fn(|index| match channels[index] {
                    PrLevelChannel::Keep => [
                        ChannelSource::Red,
                        ChannelSource::Green,
                        ChannelSource::Blue,
                    ][index],
                    PrLevelChannel::Off => ChannelSource::FullOff,
                    PrLevelChannel::On => ChannelSource::FullOn,
                });
            Ok(LayerEffect::ShiftChannels {
                take_red_from,
                take_green_from,
                take_blue_from,
            })
        }
        PrEffectParams::Levels(PrLevels::Corrections(_)) => Err("Levels channel corrections require an ordinary picture graph; original master retained".into()),
        PrEffectParams::Levels(PrLevels::Master { rgb }) => {
            let [input_black, input_white, output_black, output_white, gamma] =
                std::array::from_fn(|index| LEVELS.params[index].fx_value(rgb[index]));
            Ok(LayerEffect::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
            })
        }
        PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness,
            contrast,
        }) => Ok(LayerEffect::BrightnessContrast {
            brightness: fx_value(&BRIGHTNESS_CONTRAST_BRIGHTNESS, brightness, host)?,
            contrast: fx_value(&BRIGHTNESS_CONTRAST_CONTRAST, contrast, host)?,
        }),
        PrEffectParams::Invert(PrInvert { channel: 15, .. }) => {
            Err("Invert Alpha requires an ordinary occurrence graph; source/adjustment stacks are unsupported".into())
        }
        PrEffectParams::Invert(PrInvert { blend, .. }) => {
            Ok(invert_levels(fx_value(&INVERT_BLEND, blend, host)?))
        }
        PrEffectParams::FindEdges(edges) => {
            host.frame(STAGED_RAMP_REASON, FRAME_RULE)
                .map_err(|_| FIND_EDGES_FRAME_RULE.to_owned())?;
            host.own_frame
                .as_ref()
                .map_err(|_| FIND_EDGES_FRAME_RULE.to_owned())?;
            Ok(LayerEffect::FindEdges {
                // Adobe checked = bright edges on black; FX uses 0 for that.
                invert: Some(if edges.invert { 0.0 } else { 1.0 }),
            })
        }
        PrEffectParams::Tint(PrTint {
            black,
            white,
            amount,
        }) => Ok(tint_tritone(PrTint {
            black,
            white,
            amount: fx_value(&TINT_AMOUNT, amount, host)?,
        })),
        // Black & White renders as Tint's defaults (E7). It has no parameters.
        PrEffectParams::BlackWhite => Ok(tint_tritone(PrTint::GRAYSCALE)),
        PrEffectParams::LegacyLuma { threshold, cutoff } => Ok(LayerEffect::LumaKey {
            threshold: Some(LEGACY_LUMA_THRESHOLD.fx_value(threshold)),
            softness: Some(fx_value(&LEGACY_LUMA_CUTOFF, cutoff, host)?),
            invert: Some(0.0),
        }),
        PrEffectParams::ModernNoise { amount, seed } => Ok(LayerEffect::Grain {
            amount: Some(NOISE_AMOUNT.fx_value(amount)),
            size: Some(1.0),
            softness: Some(0.0),
            aspect_ratio: Some(1.0),
            seed: Some(seed),
        }),
        PrEffectParams::Noise { amount } => Ok(LayerEffect::Grain {
            amount: Some(NOISE_AMOUNT.fx_value(amount)),
            size: Some(1.0),
            softness: Some(0.0),
            aspect_ratio: Some(1.0),
            seed: Some(0.0),
        }),
        PrEffectParams::LensDistortion(curvature) => {
            host.frame(STAGED_RAMP_REASON, FRAME_RULE).map_err(|_| {
                "Lens Distortion requires an unstaged identity canvas-sized input plane".to_owned()
            })?;
            host.own_frame.as_ref().map_err(String::clone)?;
            Ok(LayerEffect::LensDistortion {
                amount: Some(LENS_CURVATURE.fx_value(curvature)),
                center_x: Some(0.5),
                center_y: Some(0.5),
            })
        }
        PrEffectParams::Offset([tile_center_x, tile_center_y]) => Ok(LayerEffect::MotionTile {
            tile_center_x,
            tile_center_y,
            tile_width: 100.0,
            tile_height: 100.0,
            output_width: 100.0,
            output_height: 100.0,
            mirror_edges: false,
            phase: 0.0,
        }),
        PrEffectParams::LumetriVignette([amount, midpoint, feather]) => Ok(LayerEffect::Vignette {
            amount: Some(-amount / 5.0),
            radius: Some((midpoint / 100.0).max(0.001)),
            feather: Some((feather / 100.0).max(0.001)),
        }),
        PrEffectParams::LumetriTemperature(temperature) => Ok(LayerEffect::TemperatureTint {
            temperature: Some(temperature / 3.0),
            tint: Some(0.0),
        }),
        PrEffectParams::LumetriTint(tint) => Ok(LayerEffect::TemperatureTint {
            temperature: Some(0.0),
            tint: Some(-tint / 3.0),
        }),
        PrEffectParams::LumetriExposure(exposure) => Ok(LayerEffect::Exposure {
            exposure: Some(exposure),
            offset: Some(0.0),
            gamma_correction: Some(1.0),
        }),
        PrEffectParams::LumetriSaturation(saturation) => Ok(LayerEffect::HueSaturation {
            hue: 0.0,
            saturation: saturation - 100.0,
            lightness: 0.0,
            colorize: false,
            colorize_hue: 0.0,
            colorize_saturation: 0.0,
            colorize_lightness: 0.0,
        }),
        // The reader keeps only aligned linear ramps; the host frame is the
        // converter's rule.
        PrEffectParams::Ramp(ramp) => {
            host.frame(STAGED_RAMP_REASON, FRAME_RULE)?;
            Ok(gradient_ramp(ramp))
        }
        // The reader keeps only Sharp Colors on with whole, held counts; the
        // grid is the same fraction of the frame at any Motion.
        PrEffectParams::Mosaic(mosaic) => {
            if host.staged {
                return Err(STAGED_MOSAIC_REASON.to_owned());
            }
            host.own_frame.as_ref().map_err(String::clone)?;
            fx_mosaic(mosaic)
        }
        // The reader keeps only whole, held Counts; the host frame is the
        // converter's rule.
        PrEffectParams::Replicate(PrReplicate { count }) => {
            host.frame(STAGED_REPLICATE_REASON, REPLICATE_FRAME_RULE)?;
            Ok(motion_tile(f64::from(count)))
        }
        PrEffectParams::Sharpen(sharpen) => {
            host.frame(STAGED_RAMP_REASON, FRAME_RULE)
                .map_err(|_| SHARPEN_HOST_RULE.to_owned())?;
            Ok(LayerEffect::Sharpen {
                amount: Some(f64::from(sharpen.amount)),
            })
        }
        // The reader keeps only whole Levels with Hold keys. The Level is
        // always written: FX renders an absent `levels` at 6, and Premiere's
        // default is 7.
        PrEffectParams::Posterize(PrPosterize { level }) => Ok(LayerEffect::Posterize {
            levels: Some(f64::from(level)),
        }),
        // Clock effects require the owner's check, not this pixel-effect path.
        PrEffectParams::PosterizeTime { .. } => Err(POSTERIZE_TIME_OWNER_REASON.to_owned()),
        // A Transform is no effect record: the one Transform of a media clip
        // is its staged video's transform (`import_effects`), and every other
        // host fails closed here.
        PrEffectParams::Transform(_) | PrEffectParams::AdjustmentGeometry2(_) => Err(TRANSFORM_HOST_REASON.to_owned()),
    }
}

/// The FX `motionTile` of a Replicate Count: `count` × `count` whole copies
/// over the whole frame, without mirrored edges or phase ([`PrReplicate`]).
fn motion_tile(count: f64) -> LayerEffect {
    let (size, center) = (
        PrReplicate::tile_size(count),
        PrReplicate::tile_center(count),
    );
    LayerEffect::MotionTile {
        tile_center_x: center,
        tile_center_y: center,
        tile_width: size,
        tile_height: size,
        output_width: PrReplicate::FULL_FRAME_PERCENT,
        output_height: PrReplicate::FULL_FRAME_PERCENT,
        mirror_edges: false,
        phase: 0.0,
    }
}

/// Freeze the parsed native rate at source In. Only Linear and Hold interiors
/// have a bounded mapping here; endpoint/knot values do not need interpolation.
fn posterize_time_rate_at_in(
    static_rate: f64,
    animations: &[PrEffectParamAnimation],
    source_in: i64,
) -> Result<f64, String> {
    let Some(keys) = animations
        .iter()
        .find_map(|animation| animation.keys.scalar())
    else {
        return Ok(static_rate);
    };
    let Some(first) = keys.first() else {
        return Ok(static_rate);
    };
    if source_in <= first.source_ticks {
        return Ok(first.value);
    }
    for pair in keys.windows(2) {
        let (left, right) = (&pair[0], &pair[1]);
        if source_in == right.source_ticks {
            return Ok(right.value);
        }
        if source_in < right.source_ticks {
            return match right.easing {
                PrKeyframeEasing::Linear => {
                    // The reader orders keys. Subtract in i128 before converting
                    // the bounded interval fraction to the rate's f64 precision.
                    let progress = (i128::from(source_in) - i128::from(left.source_ticks)) as f64
                        / (i128::from(right.source_ticks) - i128::from(left.source_ticks)) as f64;
                    Ok(left.value + (right.value - left.value) * progress)
                }
                PrKeyframeEasing::Hold => Ok(left.value),
                PrKeyframeEasing::CubicBezier { .. } => Err("Frame Rate at source In lies inside unsupported easing; only Linear and Hold interiors are sampled; Posterize Time was omitted".to_owned()),
            };
        }
    }
    Ok(keys.last().map_or(static_rate, |key| key.value))
}

fn posterize_time_has_other_animation(
    layer: LayerId,
    id: Option<EffectId>,
    effects: &[EffectRecord],
    dynamics: &AnimationGraph,
) -> bool {
    dynamics.entries().iter().any(|entry| match &entry.target {
        PropertyTarget::LayerProperty(target) => target.layer_id() == layer,
        PropertyTarget::EffectProperty(target) => {
            Some(target.effect_id()) != id
                && effects.iter().any(|record| {
                    matches!(record.data(),
                EffectData::Identified { id, enabled: true, .. } if *id == target.effect_id())
                })
        }
        _ => false,
    })
}

/// Run after transitions have contributed their tracks, preserving those tracks
/// rather than applying an unmeasured whole-layer hold to them.
pub(super) fn finish_posterize_time_import(
    layers: &mut [Layer],
    dynamics: &AnimationGraph,
    in_nest: bool,
    omissions: &mut Vec<Omission>,
) -> crate::error::Result<()> {
    for layer in layers {
        let LayerData::Video(video) = layer.data() else {
            continue;
        };
        let mut retained = video.clone();
        retained.effects.retain(|effect| {
            let (id, enabled, payload) = match effect.data() {
                EffectData::Identified {
                    id,
                    enabled,
                    effect,
                } => (Some(*id), *enabled, effect),
                EffectData::Legacy(effect) => (None, true, effect),
            };
            if !matches!(
                payload,
                EffectPayload::Known(LayerEffect::PosterizeTime { .. })
            ) {
                return true;
            }
            let reason = if in_nest {
                Some(POSTERIZE_TIME_OWNER_REASON)
            } else if enabled
                && posterize_time_has_other_animation(video.id, id, &video.effects, dynamics)
            {
                Some(POSTERIZE_TIME_ANIMATION_REASON)
            } else {
                None
            };
            if let Some(reason) = reason {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    format!("video layer {}", video.id.value()),
                    reason,
                );
                false
            } else {
                true
            }
        });
        if retained.effects.len() != video.effects.len() {
            *layer = Layer::from_data(&LayerData::Video(retained))?;
        }
    }
    Ok(())
}

fn import_posterize_time(
    frame_rate: f64,
    clip: &PrVideoOccurrence,
    boundary: MaskBoundary,
    source_kind: PrMediaKind,
) -> Result<LayerEffect, String> {
    if !(POSTERIZE_TIME_MIN_RATE..=POSTERIZE_TIME_MAX_RATE).contains(&frame_rate) {
        return Err(
            "Frame Rate is outside the FX runtime's 1 to 60 fps range; it was not silently clamped"
                .to_owned(),
        );
    }
    if !matches!(source_kind, PrMediaKind::Video { .. })
        || boundary != MaskBoundary::Flat
        || clip.time_remap.is_some()
        || clip.playback_rate != 1.0
    {
        return Err(POSTERIZE_TIME_OWNER_REASON.to_owned());
    }
    Ok(LayerEffect::PosterizeTime {
        frame_rate: Some(frame_rate),
    })
}

/// The FX `mosaic` of a Mosaic with Sharp Colors on: the same counts.
fn fx_mosaic(mosaic: PrMosaic) -> Result<LayerEffect, String> {
    let count = |value: u32| {
        PositiveProperty::new(f64::from(value)).ok_or("a block count must be at least 1")
    };
    Ok(LayerEffect::Mosaic {
        horizontal_blocks: count(mosaic.horizontal)?,
        vertical_blocks: count(mosaic.vertical)?,
        sharp_colors: mosaic.sharp_colors,
    })
}

/// Ids of the effects on exported video layers, including those inside groups
/// that export as nests, on those groups, on exported adjustment layers and on
/// exported still images: those whose writer runs, which `still_facts` accepts
/// with the inspected `media_facts`, and that no track matte uses, since a
/// matte still exports only at its defaults, without effects.
/// [`export_effects`], or for a still with an Opacity mask
/// [`omit_unexported_effects`], reports an animated parameter of one of them
/// with its effect, so the generic animation-target omission skips those
/// targets. `canvas` is the sequence size.
pub(super) fn video_effect_ids(
    composition: &FXComposition,
    media_facts: &BTreeMap<String, MediaFacts>,
    canvas: [u32; 2],
) -> BTreeSet<EffectId> {
    let (layers, dynamics) = (composition.layers(), composition.dynamics());
    let mattes = super::tesseract_to_premiere::track_matte_sources(layers);
    let written_still = |layer: &&Layer| match layer.data() {
        LayerData::Image(image) => {
            let ImageSource::Asset(source) = &image.source;
            !mattes.contains(&image.id)
                && matches!(super::still::still_facts(source, media_facts), Ok(Ok(_)))
        }
        _ => false,
    };
    super::nested::exported_video_layers(layers, dynamics, canvas)
        .into_iter()
        .chain(super::nested::exported_image_layers(layers, dynamics, canvas).filter(written_still))
        .flat_map(|layer| match layer.data() {
            LayerData::Video(video) => video.effects.as_slice(),
            LayerData::Media(media) if media.source.kind == MediaSourceKind::Video => {
                media.effects.as_slice()
            }
            LayerData::Group(group) => group.effects.as_slice(),
            LayerData::Adjustment(adjustment) => adjustment.effects.as_slice(),
            LayerData::Image(image) => image.effects.as_slice(),
            _ => &[],
        })
        .filter_map(|record| match record.data() {
            EffectData::Identified { id, .. } => Some(*id),
            EffectData::Legacy(_) => None,
        })
        .collect()
}

/// Export a video layer's effect stack in its current order onto the clip
/// `host`, which export places, recording each exported effect in `written`
/// and reporting each exported Replicate or Posterize as an approximation.
pub(super) fn export_effects(
    effects: &[EffectRecord],
    dynamics: &AnimationGraph,
    host: EffectHost<'_>,
    written: &mut WrittenAnimation,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Vec<PrEffect> {
    let mut exported = Vec::with_capacity(effects.len());
    for (position, effect) in (1..).zip(effects) {
        // A pre-v13 record has no id, so no animator targets it, and is enabled.
        let (id, enabled, payload) = match effect.data() {
            EffectData::Identified {
                id,
                enabled,
                effect,
            } => (Some(*id), *enabled, effect),
            EffectData::Legacy(effect) => (None, true, effect),
        };
        let name = match id {
            Some(id) => format!("effect {}", id.value()),
            None => format!("effect at stack position {position}"),
        };
        // These owned mappings deliberately retain an editable native
        // approximation. Buffer their control diagnostics until success is known:
        // failed/omitted effects must still seed ordinary hybrid replacement.
        let retained_native = matches!(
            payload,
            EffectPayload::Known(
                LayerEffect::LensDistortion { .. }
                    | LayerEffect::Fisheye { .. }
                    | LayerEffect::OuterGlow(_)
                    | LayerEffect::Grain { .. }
            )
        );
        let mut control_diagnostics = Vec::new();
        let result = if enabled
            && matches!(
                payload,
                EffectPayload::Known(LayerEffect::PosterizeTime { .. })
            )
            && posterize_time_has_other_animation(host.layer, id, effects, dynamics)
        {
            Err(POSTERIZE_TIME_ANIMATION_REASON.to_owned())
        } else if retained_native {
            export_effect(
                id,
                enabled,
                payload,
                dynamics,
                host,
                record,
                &mut control_diagnostics,
            )
        } else {
            export_effect(id, enabled, payload, dynamics, host, record, omissions)
        };
        for diagnostic in control_diagnostics {
            if result.is_ok() {
                omissions.emit_retained_native_effect(diagnostic);
            } else {
                omissions.emit(diagnostic);
            }
        }
        match result {
            Ok(effect) => {
                if let Some(id) = id {
                    written.record_effect(id);
                }
                if let PrEffectParams::Replicate(_) = effect.params {
                    approximate(
                        omissions,
                        record,
                        format!(
                            "effects: {} {name} converts approximately: {}",
                            effect_type(payload),
                            PrReplicate::TILING_APPROXIMATION
                        ),
                    );
                }
                if let Some(reason) = effect_approximation(&effect.params) {
                    let message = format!(
                        "effects: {} {name} converts approximately: {}",
                        effect_type(payload),
                        reason
                    );
                    if retained_native {
                        omissions.emit_retained_native_effect(Omission {
                            scope: OmissionScope::Feature,
                            kind: crate::OmissionKind::Approximated,
                            record: record.to_owned(),
                            reason: message,
                        });
                    } else {
                        approximate(omissions, record, message);
                    }
                }
                if let PrEffectParams::FindEdges(_) = effect.params {
                    approximate(omissions, record, format!(
                        "effects: Find Edges {name} converts approximately: {}; Blend With Original is written as 0 from the current FX effect, without replaying native controls",
                        PrFindEdges::APPROXIMATION
                    ));
                }
                exported.push(effect);
            }
            Err(reason) => omit_unexported_effect(position, effect, &reason, record, omissions),
        }
    }
    exported
}

/// Report each of a layer's `effects`, none of which export writes, for
/// `reason`, as [`export_effects`] reports an effect that it cannot write.
pub(super) fn omit_unexported_effects(
    effects: &[EffectRecord],
    reason: &str,
    record: &str,
    omissions: &mut dyn OmissionSink,
) {
    for (position, effect) in (1..).zip(effects) {
        omit_unexported_effect(position, effect, reason, record, omissions);
    }
}

/// Report `effect`, at stack `position`, as not exported for `reason`; a
/// pre-v13 record has no id, so its position names it.
fn omit_unexported_effect(
    position: usize,
    effect: &EffectRecord,
    reason: &str,
    record: &str,
    omissions: &mut dyn OmissionSink,
) {
    let (name, enabled, payload) = match effect.data() {
        EffectData::Identified {
            id,
            enabled,
            effect,
        } => (format!("effect {}", id.value()), *enabled, effect),
        EffectData::Legacy(effect) => {
            (format!("effect at stack position {position}"), true, effect)
        }
    };
    let name = if matches!(payload, EffectPayload::Known(LayerEffect::LumaKey { .. })) {
        format!("{name} at stack position {position}")
    } else {
        name
    };
    let coverage = omitted_coverage_consequence(enabled, payload);
    omit(
        omissions,
        OmissionScope::Feature,
        record,
        format!(
            "effects: {} {name} was not exported: {reason}{coverage}",
            effect_type(payload)
        ),
    );
}

/// Coverage consequence shared by clip and graphic effect-omission routes.
pub(super) fn omitted_coverage_consequence(enabled: bool, payload: &EffectPayload) -> &'static str {
    if enabled && matches!(payload, EffectPayload::Known(LayerEffect::LumaKey { .. })) {
        "; omitting the enabled key may expose previously keyed pixels or cover underlying content"
    } else {
        ""
    }
}

/// The native effect whose parameter keys export writes for `payload`, if
/// any: an FX Invert is a `levels`, so it has the Levels parameters.
pub(super) fn effect_spec(payload: &EffectPayload) -> Option<&'static EffectSpec> {
    match payload {
        EffectPayload::Known(LayerEffect::GaussianBlur { .. }) => Some(&FILM_IMPACT_BLUR),
        EffectPayload::Known(LayerEffect::CornerPin { .. }) => Some(&CORNER_PIN),
        EffectPayload::Known(LayerEffect::DirectionalBlur { .. }) => {
            Some(&FILM_IMPACT_DIRECTIONAL_BLUR)
        }
        EffectPayload::Known(LayerEffect::Levels { .. }) => Some(&LEVELS),
        EffectPayload::Known(LayerEffect::BrightnessContrast { .. }) => Some(&BRIGHTNESS_CONTRAST),
        EffectPayload::Known(LayerEffect::TintTritone { .. }) => Some(&TINT),
        EffectPayload::Known(LayerEffect::GradientRamp { .. }) => Some(&RAMP),
        EffectPayload::Known(LayerEffect::Mosaic { .. }) => Some(&MOSAIC),
        EffectPayload::Known(LayerEffect::MotionTile { .. }) => Some(&REPLICATE),
        EffectPayload::Known(LayerEffect::Posterize { .. }) => Some(&POSTERIZE),
        EffectPayload::Known(LayerEffect::Sharpen { .. }) => Some(&SHARPEN),
        EffectPayload::Known(LayerEffect::LumaKey { .. }) => Some(&LEGACY_LUMA_KEY),
        EffectPayload::Known(LayerEffect::Grain { .. }) => Some(&NOISE),
        EffectPayload::Known(LayerEffect::OuterGlow(_)) => Some(&ALPHA_GLOW),
        EffectPayload::Known(LayerEffect::LensDistortion { .. }) => Some(&LENS_DISTORTION),
        EffectPayload::Known(LayerEffect::PosterizeTime { .. }) => Some(&POSTERIZE_TIME),
        _ => None,
    }
}

/// The exported effect, or the reason it has no exact Premiere equivalent.
fn export_effect(
    id: Option<EffectId>,
    enabled: bool,
    payload: &EffectPayload,
    dynamics: &AnimationGraph,
    host: EffectHost<'_>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    if let EffectPayload::Known(LayerEffect::Fisheye {
        amount,
        center_x,
        center_y,
    }) = payload
    {
        let amount = amount.unwrap_or(50.0);
        if !(0.0..=100.0).contains(&amount) || host.frame.contains(&0) {
            return Err("Fisheye requires finite amount 0..100 and a positive plane".into());
        }
        if center_x.unwrap_or(0.5) != 0.5 || center_y.unwrap_or(0.5) != 0.5 {
            return Err("Fisheye native Lens approximation requires center (0.5,0.5)".into());
        }
        // Fit the two inverse samplers at the horizontal quarter-frame radius.
        // Fisheye uses aspect-corrected tan(r*theta/r_max)/tan(theta), whereas
        // Lens uses r*(1+k*r*r). This one-point fit cannot preserve both the
        // central derivative and frame edges. Native Lens itself is uncalibrated.
        let theta = amount * 1.45 / 100.0;
        let r = 0.25_f64;
        let aspect = f64::from(host.frame[1]) / f64::from(host.frame[0]);
        let r_max = 0.5 * 1.0_f64.hypot(aspect);
        let coefficient = if theta <= 1e-4 {
            0.0
        } else {
            (((r / r_max * theta).tan() / theta.tan() * r_max / r) - 1.0) / (r * r)
        };
        let lens = EffectPayload::Known(LayerEffect::LensDistortion {
            amount: Some(coefficient.clamp(-1.0, 0.0)),
            center_x: Some(0.5),
            center_y: Some(0.5),
        });
        let result = export_effect(None, enabled, &lens, dynamics, host, record, omissions)?;
        approximate(omissions,record,format!("Fisheye approximated by current native Lens Curvature via horizontal quarter-frame inverse-sampler fit; polynomial coefficient {coefficient} limited to [-1,0]. Tangent projection, central magnification, circular aspect correction and edge-preserving support are not retained; native gain/fill/alpha remain uncalibrated"));
        if dynamics.entries().iter().any(|entry| {
            entry
                .target
                .effect_id()
                .is_some_and(|effect| Some(effect) == id)
        }) {
            approximate(omissions,record,"Fisheye animation flattened to current static controls; nonlinear projection fitting is not an affine key mapping");
        }
        return Ok(result);
    }
    if let EffectPayload::Known(LayerEffect::OuterGlow(style)) = payload {
        let mut keyed = Vec::new();
        for entry in dynamics.entries() {
            let PropertyTarget::EffectProperty(target) = &entry.target else {
                continue;
            };
            if Some(target.effect_id()) != id {
                continue;
            }
            let reason = if target.param_name() != "size" {
                Some("native export has only a size animation mapping")
            } else if let Some(reason) = host.static_parameters_reason {
                Some(reason)
            } else if host.video_keys.is_some_and(|clock| {
                entry
                    .animator
                    .keyframe_track()
                    .is_some_and(|track| !clock.exact(track))
            }) {
                Some(HELD_CLOCK_REASON)
            } else {
                None
            };
            if let Some(reason) = reason {
                approximate(
                    omissions,
                    record,
                    format!(
                        "Alpha Glow {} animation flattened to current static control: {reason}",
                        target.param_name()
                    ),
                );
            } else {
                keyed.push((&ALPHA_GLOW_SIZE, entry));
            }
        }
        if style.spread != 0.0
            || style.range != 0.5
            || style.blend_mode != fx_schema::BlendMode::Normal
        {
            approximate(omissions, record, format!("Alpha Glow replaces current OuterGlow spread={}, range={}, blend={:?} with the native single-color fading form; dilation, falloff and blend differences are not reproduced", style.spread, style.range, style.blend_mode));
        }
        let [r, g, b, a] = style.color;
        let color =
            PrColour::from_fx([r, g, b]).ok_or("Alpha Glow color must be finite in 0..1")?;
        if !(0.0..=1.0).contains(&a) || !style.spread.is_finite() || !style.range.is_finite() {
            return Err("Alpha Glow requires finite style controls and opacity in 0..1".to_owned());
        }
        let static_size = style.size.value().min(100.0).round();
        if style.size.value() > 100.0 {
            approximate(
                omissions,
                record,
                format!(
                    "Alpha Glow size {} limited to the native maximum 100; halo extent is reduced",
                    style.size.value()
                ),
            );
        }
        let converted = export_scalar_params(
            [&ALPHA_GLOW_SIZE],
            [static_size],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )
        .and_then(|(values, animations)| {
            alpha_glow_size(values[0], &animations)?;
            Ok((values, animations))
        });
        let ([size], animations) = match converted {
            Ok(converted) => converted,
            Err(reason) => {
                approximate(omissions, record, format!("Alpha Glow size animation flattened to current static size {static_size}: {reason}"));
                ([static_size], Vec::new())
            }
        };
        return Ok(PrEffect {
            mask: None,
            enabled: enabled && style.enabled,
            params: PrEffectParams::AlphaGlow {
                size,
                brightness: (a * 255.0).round(),
                color,
            },
            animations,
        });
    }
    let spec = effect_spec(payload);
    let grain = matches!(payload, EffectPayload::Known(LayerEffect::Grain { .. }));
    let mut grain_strength_seen = false;
    let mut keyed = Vec::new();
    for entry in dynamics.entries() {
        let PropertyTarget::EffectProperty(target) = &entry.target else {
            continue;
        };
        if Some(target.effect_id()) != id {
            continue;
        }
        let parameter = target.param_name();
        // Grain's authorable intensity and persisted amount address the same
        // runtime field. Never choose between competing alias tracks.
        let parameter = if grain && matches!(parameter, "intensity" | "amount") {
            if grain_strength_seen {
                return Err("Grain has multiple amount/intensity animation tracks for the same strength parameter".to_owned());
            }
            grain_strength_seen = true;
            "intensity"
        } else {
            parameter
        };
        if matches!(
            payload,
            EffectPayload::Known(LayerEffect::PosterizeTime { .. })
        ) {
            omit_field(
                omissions,
                host.layer,
                ExportField::Effects,
                record,
                format!("Posterize Time {parameter} animation was not exported: FX frameRate is not animatable; the current static runtime rate was kept"),
            );
            continue;
        }
        if matches!(payload, EffectPayload::Known(LayerEffect::LumaKey { .. }))
            && parameter == "invert"
        {
            return Err(
                "Legacy Luma has no native invert control; animated FX invert is not representable"
                    .to_owned(),
            );
        }
        let Some(param) = spec.and_then(|spec| spec.bound_param(parameter)) else {
            return Err(format!(
                "animated {parameter} has no static Premiere value; only static effect parameters export"
            ));
        };
        if let Some(reason) = host.static_parameters_reason {
            if grain {
                return Err(format!(
                    "Grain strength animation has an unsafe clock: {reason}"
                ));
            }
            omit_field(
                omissions,
                host.layer,
                ExportField::Effects,
                record,
                format!("effect {parameter} animation was not exported: {reason}; static values were kept"),
            );
            continue;
        }
        if host.video_keys.is_some_and(|clock| {
            entry
                .animator
                .keyframe_track()
                .is_some_and(|track| !clock.exact(track))
        }) {
            if grain {
                return Err(format!(
                    "Grain strength animation has an unsafe clock: {HELD_CLOCK_REASON}"
                ));
            }
            omit_field(
                omissions,
                host.layer,
                ExportField::Effects,
                record,
                format!(
                    "effect {parameter} animation was not exported: {HELD_CLOCK_REASON}; static values were kept"
                ),
            );
            continue;
        }
        keyed.push((param, entry));
    }
    if let EffectPayload::Known(levels @ LayerEffect::Levels { .. }) = payload {
        if let Some(form) = invert_form(levels, &keyed) {
            return export_invert(enabled, form, host.source_in, record, omissions);
        }
    }
    if let EffectPayload::Known(LayerEffect::CornerPin {
        upper_left_x,
        upper_left_y,
        upper_right_x,
        upper_right_y,
        lower_left_x,
        lower_left_y,
        lower_right_x,
        lower_right_y,
    }) = payload
    {
        let corners = [
            [*upper_left_x, *upper_left_y],
            [*upper_right_x, *upper_right_y],
            [*lower_left_x, *lower_left_y],
            [*lower_right_x, *lower_right_y],
        ];
        return export_corner_pin(enabled, corners, &keyed, host.source_in);
    }
    if let EffectPayload::Known(LayerEffect::FindEdges { invert }) = payload {
        host.frame_is_canvas(dynamics, STAGED_RAMP_REASON, FRAME_RULE)
            .map_err(|_| FIND_EDGES_FRAME_RULE.to_owned())?;
        let invert = invert.unwrap_or(1.0);
        if !invert.is_finite() {
            return Err("Find Edges invert must be finite".to_owned());
        }
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::FindEdges(PrFindEdges {
                // Preserve the shader's threshold, including absent/default 1.
                invert: invert <= 0.5,
                blend: 0.0,
                blend_animated: false,
            }),
            animations: Vec::new(),
        });
    }
    if let EffectPayload::Known(LayerEffect::GaussianBlur {
        blurriness,
        repeat_edge_pixels,
        layer_size,
    }) = payload
    {
        if layer_size.is_some() {
            return Err(
                "layerSize sets the Repeat Edge Pixels clamp from non-layer bounds, which Premiere cannot express"
                    .to_owned(),
            );
        }
        // The blur does not depend on the clip's frame.
        let ([amount], animations) = export_scalar_params(
            [&FILM_IMPACT_BLUR_AMOUNT],
            [blurriness.value()],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::FilmImpactBlur(PrFilmImpactBlur {
                amount,
                repeat_edge_pixels: repeat_edge_pixels.unwrap_or(false),
            }),
            animations,
        });
    }
    if let EffectPayload::Known(LayerEffect::DirectionalBlur {
        direction,
        blur_length,
    }) = payload
    {
        return export_directional_blur(
            enabled,
            [*direction, blur_length.value()],
            &keyed,
            dynamics,
            host,
            record,
            omissions,
        );
    }
    if let EffectPayload::Known(tint @ LayerEffect::TintTritone { .. }) = payload {
        return export_tint(enabled, tint, &keyed, host.source_in, record, omissions);
    }
    if let EffectPayload::Known(ramp @ LayerEffect::GradientRamp { .. }) = payload {
        return export_ramp(enabled, ramp, &keyed, dynamics, host, record, omissions);
    }
    if let EffectPayload::Known(LayerEffect::Mosaic {
        horizontal_blocks,
        vertical_blocks,
        sharp_colors,
    }) = payload
    {
        // The grid is a fraction of the frame at any Motion; only a
        // stage group or a nest is no host.
        if host.staged || host.nested {
            return Err(STAGED_MOSAIC_REASON.to_owned());
        }
        let ([horizontal, vertical], animations) = export_scalar_params(
            [&MOSAIC_HORIZONTAL_BLOCKS, &MOSAIC_VERTICAL_BLOCKS],
            [horizontal_blocks.value(), vertical_blocks.value()],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        let mosaic = PrMosaic {
            horizontal: PrMosaic::count(&MOSAIC.params[0], "", horizontal)?,
            vertical: PrMosaic::count(&MOSAIC.params[1], "", vertical)?,
            sharp_colors: *sharp_colors,
        };
        mosaic.ensure_convertible(&animations)?;
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::Mosaic(mosaic),
            animations,
        });
    }
    if let EffectPayload::Known(tile @ LayerEffect::MotionTile { .. }) = payload {
        if host.still {
            return Err(STILL_REPLICATE_REASON.to_owned());
        }
        return export_replicate(enabled, tile, &keyed, dynamics, host, record, omissions);
    }
    if let EffectPayload::Known(LayerEffect::Sharpen { amount }) = payload {
        if host.still {
            return Err(STILL_SHARPEN_REASON.to_owned());
        }
        // The shared frame check covers 2D Motion only. Sharpen must not
        // inherit admission after export diagnoses and drops skew or depth.
        let transform = host.transform;
        if transform.skew != 0.0
            || transform.skew_axis != 0.0
            || transform.rotation_x != 0.0
            || transform.rotation_y != 0.0
            || transform.orientation != [0.0; 3]
            || transform.position.z().is_some_and(|z| z != 0.0)
        {
            return Err(SHARPEN_HOST_RULE.to_owned());
        }
        host.frame_is_canvas(dynamics, STAGED_RAMP_REASON, FRAME_RULE)
            .map_err(|_| SHARPEN_HOST_RULE.to_owned())?;
        let ([amount], animations) = export_scalar_params(
            [&SHARPEN_AMOUNT],
            [amount.unwrap_or(40.0)],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::Sharpen(PrSharpen::new(amount, &animations)?),
            animations,
        });
    }
    if let EffectPayload::Known(LayerEffect::LensDistortion {
        amount,
        center_x,
        center_y,
    }) = payload
    {
        if !enabled {
            return Err(
                "bypassed native Lens Distortion export has no established wire form".to_owned(),
            );
        }
        // Motion export may drop these components while passing the original
        // transform here. Do not admit that lossy plane as an identity Lens host.
        let transform = host.transform;
        if transform.skew != 0.0
            || transform.skew_axis != 0.0
            || transform.rotation_x != 0.0
            || transform.rotation_y != 0.0
            || transform.orientation != [0.0; 3]
            || transform.position.z().is_some_and(|z| z != 0.0)
        {
            return Err("Lens Distortion requires an identity input plane without static skew, skew axis, 3D rotation, orientation or nonzero Z position".to_owned());
        }
        host.frame_is_canvas(dynamics, STAGED_RAMP_REASON, FRAME_RULE)
            .map_err(|_| {
                "Lens Distortion requires an unstaged identity canvas-sized input plane".to_owned()
            })?;
        if center_x.unwrap_or(0.5) != 0.5 || center_y.unwrap_or(0.5) != 0.5 {
            return Err("Lens Distortion exports only a centered radial effect; decentering is not a UV-centre translation".to_owned());
        }
        let ([curvature], animations) = export_scalar_params(
            [&LENS_CURVATURE],
            [amount.ok_or("Lens Distortion requires an explicit amount")?],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::LensDistortion(lens_curvature(curvature, &animations)?),
            animations,
        });
    }
    if let EffectPayload::Known(LayerEffect::Grain {
        amount,
        size,
        softness,
        aspect_ratio,
        seed,
    }) = payload
    {
        if [*size, *softness, *aspect_ratio, *seed]
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            return Err("Grain spatial/seed controls must be finite".to_owned());
        }
        if size.unwrap_or(1.0) != 1.0
            || softness.unwrap_or(1.0) != 0.0
            || aspect_ratio.unwrap_or(1.0) != 1.0
            || seed.unwrap_or(0.0) != 0.0
        {
            approximate(omissions, record, "Grain size/softness/aspectRatio/seed edits are lost in canonical Legacy Noise; current strength remains editable".to_owned());
        }
        let ([amount], animations) = export_scalar_params(
            [&NOISE_AMOUNT],
            [amount.unwrap_or(1.0)],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        approximate(
            omissions,
            record,
            format!("Grain exports as Legacy Noise: {NOISE_APPROXIMATION}"),
        );
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::Noise { amount },
            animations,
        });
    }
    if let EffectPayload::Known(LayerEffect::LumaKey {
        threshold,
        softness,
        invert,
    }) = payload
    {
        let invert = invert.unwrap_or(0.0);
        // The existing shader reverses coverage only when invert > 0.5.
        if !(0.0..=0.5).contains(&invert) {
            return Err(format!("Legacy Luma has no native invert control; current FX invert={invert} is unsupported"));
        }
        let ([threshold, cutoff], animations) = export_scalar_params(
            [&LEGACY_LUMA_THRESHOLD, &LEGACY_LUMA_CUTOFF],
            [threshold.unwrap_or(0.3), softness.unwrap_or(0.1)],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        validate_legacy_luma(threshold, cutoff, &animations)?;
        let mut effect = PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::LegacyLuma { threshold, cutoff },
            animations,
        };
        for note in normalize_legacy_luma(&mut effect) {
            approximate(
                omissions,
                record,
                format!(
                    "Legacy Luma effect {}: {note}",
                    id.map_or_else(|| "without id".to_owned(), |id| id.value().to_string())
                ),
            );
        }
        return Ok(effect);
    }
    if let EffectPayload::Known(LayerEffect::Posterize { levels }) = payload {
        // FX renders an absent `levels` at its default, which the converter
        // does not assume. The Level does not depend on the clip's frame.
        let levels =
            levels.ok_or("levels has no value; only a posterize with its levels exports")?;
        let ([level], animations) = export_scalar_params(
            [&POSTERIZE_LEVEL],
            [levels],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::Posterize(PrPosterize::new(level, &animations)?),
            animations,
        });
    }
    if let EffectPayload::Known(LayerEffect::BrightnessContrast {
        brightness,
        contrast,
    }) = payload
    {
        // Brightness and Contrast do not depend on the clip's frame.
        let ([brightness, contrast], animations) = export_scalar_params(
            [
                &BRIGHTNESS_CONTRAST_BRIGHTNESS,
                &BRIGHTNESS_CONTRAST_CONTRAST,
            ],
            [*brightness, *contrast],
            &keyed,
            |_, value| value,
            host.source_in,
            record,
            omissions,
        )?;
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
                brightness,
                contrast,
            }),
            animations,
        });
    }
    if let EffectPayload::Known(LayerEffect::PosterizeTime { frame_rate }) = payload {
        if host.staged || host.in_nest || host.video_keys.is_none() {
            return Err(POSTERIZE_TIME_OWNER_REASON.to_owned());
        }
        let resolved = resolved_posterize_time_rate(*frame_rate);
        if let Some(rate) = frame_rate.filter(|rate| *rate != resolved) {
            approximate(
                omissions,
                record,
                format!("Posterize Time frameRate {rate} exports as its FX runtime rate {resolved} fps (default 8, range 1 to 60)"),
            );
        }
        if enabled {
            approximate(omissions, record, POSTERIZE_TIME_CLOCK_APPROXIMATION);
        }
        return Ok(PrEffect {
            mask: None,
            enabled,
            params: PrEffectParams::PosterizeTime {
                frame_rate: resolved,
            },
            animations: Vec::new(),
        });
    }
    let source_animations = keyed
        .into_iter()
        .map(|(param, entry)| {
            export_param_keys(
                param,
                entry,
                std::convert::identity,
                host.source_in,
                record,
                omissions,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let animations: Vec<_> = source_animations.iter().map(written).collect();
    let params = match payload {
        // Premiere 26.5.1's form of a bypassed Levels is unverified.
        EffectPayload::Known(LayerEffect::Levels { .. } | LayerEffect::ShiftChannels { .. })
            if !enabled =>
        {
            return Err("a disabled Levels has no verified Premiere bypass form".to_owned())
        }
        EffectPayload::Known(LayerEffect::ShiftChannels {
            take_red_from,
            take_green_from,
            take_blue_from,
        }) => {
            let mut channels = [PrLevelChannel::Keep; 3];
            for ((channel, value), own) in channels
                .iter_mut()
                .zip([take_red_from, take_green_from, take_blue_from])
                .zip([
                    ChannelSource::Red,
                    ChannelSource::Green,
                    ChannelSource::Blue,
                ])
            {
                *channel = match *value {
                    ChannelSource::FullOff => PrLevelChannel::Off,
                    ChannelSource::FullOn => PrLevelChannel::On,
                    value if value == own => PrLevelChannel::Keep,
                    _ => {
                        return Err(
                            "ShiftChannels cross-channel routing has no native Levels mapping"
                                .to_owned(),
                        )
                    }
                };
            }
            PrEffectParams::Levels(PrLevels::Channels(channels))
        }
        EffectPayload::Known(LayerEffect::Levels {
            input_black,
            input_white,
            gamma,
            output_black,
            output_white,
        }) => {
            let fx = [
                *input_black,
                *input_white,
                *output_black,
                *output_white,
                *gamma,
            ];
            let mut rgb = [0.0; 5];
            for ((native, param), value) in rgb.iter_mut().zip(LEVELS.params).zip(fx) {
                // A keyed level's static value is its first key's (see `PrEffect`).
                *native = match source_animations
                    .iter()
                    .find(|animation| animation.param.id == param.id)
                    .and_then(|animation| animation.keys.scalar()?.first())
                {
                    Some(key) => key.value,
                    None => native_value(param, "", value)?,
                };
            }
            let source = PrLevels::Master { rgb };
            let levels = PrLevels::Master {
                rgb: std::array::from_fn(|index| LEVELS.params[index].written_value(rgb[index])),
            };
            // Validate level ordering before and after rounding to native integers:
            // rounding can hide a crossing or create one.
            source.ensure_rendered_form(&source_animations)?;
            levels.ensure_rendered_form(&animations)?;
            PrEffectParams::Levels(levels)
        }
        _ => return Err("it has no Premiere effect mapping".to_owned()),
    };
    Ok(PrEffect {
        mask: None,
        enabled,
        params,
        animations,
    })
}

/// A Corner Pin with the FX `corners` in native order and its keyed corners.
/// Both normalize each corner to the clip's own frame.
fn export_corner_pin(
    enabled: bool,
    mut corners: [[f64; 2]; 4],
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    source_in: i64,
) -> Result<PrEffect, String> {
    let mut animations = Vec::new();
    for (corner, param) in corners.iter_mut().zip(CORNER_PIN.params) {
        let Some(EffectParamBinding::Point { x, y }) = param.binding else {
            continue;
        };
        let track = |name: &str| {
            keyed.iter().map(|&(_, entry)| entry).find(|entry| {
                matches!(&entry.target, PropertyTarget::EffectProperty(target) if target.param_name() == name)
            })
        };
        let Some(keys) = export_point_keys(param, [track(x), track(y)], *corner, source_in)? else {
            continue;
        };
        // A keyed corner's static value is its first key's (see `PrEffect`).
        if let Some(first) = keys.first() {
            *corner = first.value;
        }
        animations.push(PrEffectParamAnimation {
            param,
            keys: PrEffectParamKeys::Point(keys),
        });
    }
    let pin = PrCornerPin { corners };
    pin.ensure_convex(&animations)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::CornerPin(pin),
        animations,
    })
}

/// The current Directional Blur with the FX `direction` and `blur_length` and
/// its keyed parameters, mapped into the clip's own frame through its layer's
/// static similarity (see [`ClipToComposition`]) and to Angle and Amount by
/// their bindings. Every native value, static or keyed, must lie in its
/// parameter's Premiere range.
fn export_directional_blur(
    enabled: bool,
    fx_values: [f64; 2],
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    dynamics: &AnimationGraph,
    host: EffectHost<'_>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let similarity = ClipToComposition::of_layer(host, dynamics)?;
    let ([angle, amount], animations) = export_scalar_params(
        [
            &FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE,
            &FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
        ],
        fx_values,
        keyed,
        |param, value| similarity.clip_value(param, value),
        host.source_in,
        record,
        omissions,
    )?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::FilmImpactDirectionalBlur(PrFilmImpactDirectionalBlur {
            angle,
            amount,
        }),
        animations,
    })
}

/// A Levels in Invert's form ([`invert_levels`]): its static output white
/// and, when its outputs are keyed, the output white track, whose keys the
/// output black track complements ([`complementary_keys`]).
#[derive(Debug, Clone, Copy)]
struct InvertForm<'a> {
    output_white: f64,
    keys: Option<&'a AnimationGraphEntry>,
}

/// The Invert's form of `levels` with the `keyed` parameters of its FX
/// record, or `None` for any other Levels: statics that differ from
/// [`invert_levels`], a keyed input or Gamma, outputs keyed other than as
/// complements of each other, or the static identity (output white
/// [`LEVELS_WHITE`]), which keeps its Levels export rather than becoming an
/// Invert that shows the original.
fn invert_form<'a>(
    levels: &LayerEffect,
    keyed: &[(&'static EffectParamSpec, &'a AnimationGraphEntry)],
) -> Option<InvertForm<'a>> {
    let LayerEffect::Levels { output_white, .. } = levels else {
        return None;
    };
    if *levels != invert_levels(*output_white) {
        return None;
    }
    // The (RGB) Black and White Output Levels in native order.
    let [black, white] = [&LEVELS.params[2], &LEVELS.params[3]].map(|output| {
        keyed
            .iter()
            .find(|(param, _)| param.id == output.id)
            .map(|(_, entry)| *entry)
    });
    let keys = match (black, white) {
        (None, None) if keyed.is_empty() && *output_white != LEVELS_WHITE => None,
        (Some(black), Some(white)) if keyed.len() == 2 && complementary_keys(black, white) => {
            Some(white)
        }
        _ => return None,
    };
    Some(InvertForm {
        output_white: *output_white,
        keys,
    })
}

/// Whether the output `black` and `white` tracks are enabled keyframe tracks
/// with keys at the same layer times, with the same easing, whose values are
/// complements ([`invert_output_black`]), so that one Blend With Original
/// track expresses both.
fn complementary_keys(black: &AnimationGraphEntry, white: &AnimationGraphEntry) -> bool {
    fn keys(entry: &AnimationGraphEntry) -> Option<&[PropertyKeyframe]> {
        match entry.animator.data() {
            AnimatorData::Keyframes {
                track,
                enabled: true,
                ..
            } => Some(track.keyframes()),
            _ => None,
        }
    }
    let (Some(black), Some(white)) = (keys(black), keys(white)) else {
        return false;
    };
    black.len() == white.len()
        && black.iter().zip(white).all(|(black, white)| {
            black.layer_time() == white.layer_time()
                && black.easing() == white.easing()
                && matches!(
                    (black.value(), white.value()),
                    (PropertyValue::Float(black), PropertyValue::Float(white))
                        if *black == invert_output_black(*white)
                )
        })
}

/// An Invert of every channel from a Levels in its `form`: its Blend With
/// Original, static or keyed, is the output white's share of 255
/// ([`invert_blend`]), within Premiere's 0 to 100 ([`native_value`]).
fn export_invert(
    enabled: bool,
    form: InvertForm<'_>,
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let keyed = form.keys.map(|entry| (&INVERT_BLEND, entry));
    // The Channel stays RGB; the Blend does not depend on the clip's frame.
    let ([blend], animations) = export_scalar_params(
        [&INVERT_BLEND],
        [form.output_white],
        keyed.as_slice(),
        |_, value| invert_blend(value),
        source_in,
        record,
        omissions,
    )?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Invert(PrInvert { blend, channel: 0 }),
        animations,
    })
}

/// A Tint from a `tintTritone` and its keyed parameters: every FX
/// `tintTritone` exports as a Tint, a Black & White included, because a
/// Black & White is Tint's defaults (E7) and has no parameters to edit.
/// Each colour's channels round to 8 bits ([`PrColour::from_fx`]); a keyed
/// colour needs all three channel tracks at the same times with the same
/// easing ([`export_colour_keys`]). The animations follow the native `Params`
/// order, as the reader returns them.
fn export_tint(
    enabled: bool,
    tint: &LayerEffect,
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let LayerEffect::TintTritone {
        black_r,
        black_g,
        black_b,
        white_r,
        white_g,
        white_b,
        amount,
    } = tint
    else {
        return Err("it is not a tintTritone".to_owned());
    };
    // FX renders an absent field at the shader's default, which the converter
    // does not assume.
    let field = |name: &str, value: &Option<f64>| {
        value.ok_or_else(|| {
            format!("{name} has no value; only a tintTritone with all seven values exports")
        })
    };
    let black = [
        field("blackR", black_r)?,
        field("blackG", black_g)?,
        field("blackB", black_b)?,
    ];
    let white = [
        field("whiteR", white_r)?,
        field("whiteG", white_g)?,
        field("whiteB", white_b)?,
    ];
    let amount = field("amount", amount)?;
    let mut animations = Vec::new();
    let mut colours = [PrColour::BLACK; 2];
    for ((colour, param), fx) in colours
        .iter_mut()
        .zip([&TINT_MAP_BLACK_TO, &TINT_MAP_WHITE_TO])
        .zip([black, white])
    {
        let keys = export_colour_keys(param, keyed, source_in, record, omissions)?;
        // A keyed colour's static value is its first key's (see `PrEffect`).
        *colour = match keys.first() {
            Some(first) => first.value,
            None => native_colour(param, fx)?,
        };
        if !keys.is_empty() {
            animations.push(PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Colour(keys),
            });
        }
    }
    let [black, white] = colours;
    // The Amount does not depend on the clip's frame.
    let amount = match keyed.iter().find(|(param, _)| param.id == TINT_AMOUNT.id) {
        Some(&(param, entry)) => {
            let animation = export_param_keys(
                param,
                entry,
                std::convert::identity,
                source_in,
                record,
                omissions,
            )?;
            let first = animation
                .keys
                .scalar()
                .and_then(<[PrScalarKeyframe]>::first)
                .map(|key| key.value)
                .ok_or_else(|| format!("{} has no keys", param.label))?;
            animations.push(animation);
            first
        }
        None => native_value(&TINT_AMOUNT, "", amount)?,
    };
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Tint(PrTint {
            black,
            white,
            amount,
        }),
        animations,
    })
}

/// A Ramp with the FX `gradientRamp`'s endpoints, colours and blend and its
/// keyed parameters, on a host whose frame is the canvas
/// ([`EffectHost::frame_is_canvas`]). Only the linear shape exports, and the
/// axis rule of the reader holds for the written values and keys
/// ([`PrRamp::ensure_aligned`]); export writes Ramp Scatter 0.
fn export_ramp(
    enabled: bool,
    ramp: &LayerEffect,
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    dynamics: &AnimationGraph,
    host: EffectHost<'_>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let LayerEffect::GradientRamp {
        start_x,
        start_y,
        end_x,
        end_y,
        start_r,
        start_g,
        start_b,
        end_r,
        end_g,
        end_b,
        blend,
        shape,
    } = ramp
    else {
        return Err("it is not a gradientRamp".to_owned());
    };
    host.frame_is_canvas(dynamics, STAGED_RAMP_REASON, FRAME_RULE)?;
    // FX renders an absent field at the shader's default, which the converter
    // does not assume.
    let field = |name: &str, value: &Option<f64>| {
        value.ok_or_else(|| {
            format!("{name} has no value; only a gradientRamp with all twelve values exports")
        })
    };
    let shape = field("shape", shape)?;
    if shape != 0.0 {
        return Err(format!(
            "shape {shape} is not 0 (linear); a radial ramp is not converted, because Premiere measures its radius in clip pixels and the FX gradientRamp in frame UV"
        ));
    }
    let mut points = [
        [field("startX", start_x)?, field("startY", start_y)?],
        [field("endX", end_x)?, field("endY", end_y)?],
    ];
    let colours = [
        [
            field("startR", start_r)?,
            field("startG", start_g)?,
            field("startB", start_b)?,
        ],
        [
            field("endR", end_r)?,
            field("endG", end_g)?,
            field("endB", end_b)?,
        ],
    ];
    let blend = field("blend", blend)?;
    let track = |name: &str| {
        keyed.iter().map(|&(_, entry)| entry).find(|entry| {
            matches!(&entry.target, PropertyTarget::EffectProperty(target) if target.param_name() == name)
        })
    };
    let mut animations = Vec::new();
    for (point, param) in points.iter_mut().zip([&RAMP_START, &RAMP_END]) {
        let Some(EffectParamBinding::Point { x, y }) = param.binding else {
            continue;
        };
        let Some(keys) = export_point_keys(param, [track(x), track(y)], *point, host.source_in)?
        else {
            continue;
        };
        // A keyed point's static value is its first key's (see `PrEffect`).
        if let Some(first) = keys.first() {
            *point = first.value;
        }
        animations.push(PrEffectParamAnimation {
            param,
            keys: PrEffectParamKeys::Point(keys),
        });
    }
    let mut native_colours = [PrColour::BLACK; 2];
    for ((colour, param), fx) in native_colours
        .iter_mut()
        .zip([&RAMP_START_COLOR, &RAMP_END_COLOR])
        .zip(colours)
    {
        let keys = export_colour_keys(param, keyed, host.source_in, record, omissions)?;
        *colour = match keys.first() {
            Some(first) => first.value,
            None => native_colour(param, fx)?,
        };
        if !keys.is_empty() {
            animations.push(PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Colour(keys),
            });
        }
    }
    // Blend With Original is one minus the FX blend, an affine map.
    let blend = match keyed.iter().find(|(param, _)| param.id == RAMP_BLEND.id) {
        Some(&(param, entry)) => {
            let animation = export_param_keys(
                param,
                entry,
                PrRamp::fx_blend,
                host.source_in,
                record,
                omissions,
            )?;
            let first = animation
                .keys
                .scalar()
                .and_then(<[PrScalarKeyframe]>::first)
                .map(|key| key.value)
                .ok_or_else(|| format!("{} has no keys", param.label))?;
            animations.push(animation);
            first
        }
        None => native_value(&RAMP_BLEND, "", PrRamp::fx_blend(blend))?,
    };
    // Native `Params` order, as the reader stores them.
    animations.sort_by_key(|animation| animation.param.id);
    let [start, end] = points;
    let [start_colour, end_colour] = native_colours;
    let ramp = PrRamp {
        start,
        start_colour,
        end,
        end_colour,
        blend,
    };
    ramp.ensure_aligned(&animations)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Ramp(ramp),
        animations,
    })
}

/// Why FX tiles that draw no Replicate grid are not exported, after their
/// values.
const REPLICATE_GRID_RULE: &str = "a Replicate draws Count × Count whole copies, tiles of 100/Count percent whose first is centred at 1/(2·Count) of the frame, for a whole Count from 2 to 16, and nothing is rounded";

/// A Replicate from an FX `motionTile` that draws a Count × Count grid of
/// whole copies ([`PrReplicate`]) on a host whose frame is the canvas
/// ([`EffectHost::frame_is_canvas`]): the whole frame as its output, neither
/// mirrored edges nor phase, and the tiles of one whole Count, static or on
/// keys ([`PrReplicate::grid_count`], [`export_tile_count_keys`]). The Count
/// and its keys pass the reader's rule ([`PrReplicate::new`]). Any other
/// `motionTile` is omitted with its reason; nothing is rounded.
fn export_replicate(
    enabled: bool,
    tile: &LayerEffect,
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    dynamics: &AnimationGraph,
    host: EffectHost<'_>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let LayerEffect::MotionTile {
        tile_center_x,
        tile_center_y,
        tile_width,
        tile_height,
        output_width,
        output_height,
        mirror_edges,
        phase,
    } = *tile
    else {
        return Err("it is not a motionTile".to_owned());
    };
    host.frame_is_canvas(dynamics, STAGED_REPLICATE_REASON, REPLICATE_FRAME_RULE)?;
    if [output_width, output_height] != [PrReplicate::FULL_FRAME_PERCENT; 2] {
        return Err(format!(
            "outputWidth {output_width} and outputHeight {output_height} are not 100; a Replicate's copies fill the whole frame"
        ));
    }
    if mirror_edges {
        return Err("mirrorEdges is on; a Replicate does not mirror its copies".to_owned());
    }
    if phase != 0.0 {
        return Err(format!(
            "phase {phase} is not 0; a Replicate does not offset its copies"
        ));
    }
    let animations = export_tile_count_keys(keyed, host.source_in, record, omissions)?
        .map(|keys| PrEffectParamAnimation {
            param: &REPLICATE_COUNT,
            keys: PrEffectParamKeys::Scalar(keys),
        })
        .into_iter()
        .collect::<Vec<_>>();
    // A keyed Count's static value is its first key's (see `PrEffect`).
    let count = match animations
        .first()
        .and_then(|animation| animation.keys.scalar()?.first())
    {
        Some(first) => first.value,
        None => PrReplicate::grid_count([tile_width, tile_height], [tile_center_x, tile_center_y])
            .map(f64::from)
            .ok_or_else(|| {
                format!(
                    "tileWidth {tile_width}, tileHeight {tile_height}, tileCenterX {tile_center_x} and tileCenterY {tile_center_y} draw no Replicate grid: {REPLICATE_GRID_RULE}"
                )
            })?,
    };
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Replicate(PrReplicate::new(count, &animations)?),
        animations,
    })
}

/// The Count keys of a `motionTile`'s keyed tile fields in `keyed`, or `None`
/// when none is keyed. Premiere keys one Count, so the four fields of its
/// binding ([`EffectParamBinding::TileCount`]) must all be enabled keyframe
/// tracks with keys at the same times and with the same easing, converted as
/// Motion keys are, whose values at every key are one Count's grid
/// ([`PrReplicate::grid_count`]).
fn export_tile_count_keys(
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<Vec<PrScalarKeyframe>>, String> {
    let Some(EffectParamBinding::TileCount {
        width,
        height,
        center_x,
        center_y,
    }) = REPLICATE_COUNT.binding
    else {
        return Err("Replicate Count has no tile fields".to_owned());
    };
    let names = [width, height, center_x, center_y];
    let entries = names.map(|name| {
        keyed.iter().map(|&(_, entry)| entry).find(|entry| {
            matches!(&entry.target, PropertyTarget::EffectProperty(target) if target.param_name() == name)
        })
    });
    if entries.iter().all(Option::is_none) {
        return Ok(None);
    }
    let mut tracks: [Vec<PrScalarKeyframe>; 4] = Default::default();
    for ((keys, entry), name) in tracks.iter_mut().zip(entries).zip(names) {
        let Some(entry) = entry else {
            return Err(format!(
                "its {width}, {height}, {center_x} and {center_y} must all be keyed, but {name} is not; Premiere keys them as one Count"
            ));
        };
        let AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } = entry.animator.data()
        else {
            return Err(format!("{name} animation is disabled or not keyframed"));
        };
        *keys = super::tesseract_to_premiere::export_scalar_keys(
            track, source_in, name, omissions, record,
        )
        .map_err(|error| error.to_string())?;
    }
    let [widths, heights, xs, ys] = tracks;
    let paired = [&heights, &xs, &ys].into_iter().all(|other| {
        other.len() == widths.len()
            && other.iter().zip(&widths).all(|(key, width)| {
                key.source_ticks == width.source_ticks && key.easing == width.easing
            })
    });
    if !paired {
        return Err(format!(
            "its {width}, {height}, {center_x} and {center_y} keys differ in time or easing, but Premiere keys them as one Count"
        ));
    }
    widths
        .iter()
        .zip(&heights)
        .zip(&xs)
        .zip(&ys)
        .map(|(((w, h), x), y)| {
            let count = PrReplicate::grid_count([w.value, h.value], [x.value, y.value])
                .ok_or_else(|| {
                    let time = keyframes::layer_millis(w.source_ticks, source_in)
                        .map_or_else(|error| error.to_string(), |millis| format!("{millis} ms"));
                    format!(
                        "its tile keys at {time} ({width} {}, {height} {}, {center_x} {} and {center_y} {}) draw no Replicate grid: {REPLICATE_GRID_RULE}",
                        w.value, h.value, x.value, y.value
                    )
                })?;
            Ok(PrScalarKeyframe {
                source_ticks: w.source_ticks,
                value: f64::from(count),
                easing: w.easing,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
}

/// The 8-bit native colour of the FX channel values of the colour `param`,
/// or why it has none: a channel outside 0 to 1 is never clamped.
fn native_colour(param: &EffectParamSpec, rgb: [f64; 3]) -> Result<PrColour, String> {
    PrColour::from_fx(rgb)
        .ok_or_else(|| format!("{} {:?} has a channel outside 0 to 1", param.label, rgb))
}

/// The native colour keys of the colour `param` from its three FX channel
/// entries in `keyed`, or none when no channel is keyed. Each channel's keys
/// convert as scalar keys do ([`export_param_keys`], the Motion key rules);
/// Premiere keys a colour as one, so the three tracks must be keyed together,
/// with the same times and easing, and each key's channels round to one 8-bit
/// colour. A Bezier segment between colours has no verified Premiere form
/// (`PrEffect::validate`).
fn export_colour_keys(
    param: &'static EffectParamSpec,
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<Vec<PrColourKeyframe>, String> {
    let Some(EffectParamBinding::Colour { red, green, blue }) = param.binding else {
        return Err(format!("{} is not a colour", param.label));
    };
    let label = param.label;
    let channels = [red, green, blue].map(|name| {
        keyed.iter().map(|&(_, entry)| entry).find(|entry| {
            matches!(&entry.target, PropertyTarget::EffectProperty(target) if target.param_name() == name)
        })
    });
    if channels.iter().all(Option::is_none) {
        return Ok(Vec::new());
    }
    let mut tracks = Vec::with_capacity(3);
    for (entry, name) in channels.into_iter().zip([red, green, blue]) {
        let Some(entry) = entry else {
            return Err(format!(
                "its {label} {red}, {green} and {blue} keys must all be keyed, but {name} is not; Premiere keys a colour as one track"
            ));
        };
        // The channel's FX value is its native value: the colour rounds below.
        let animation = export_param_keys(
            param,
            entry,
            std::convert::identity,
            source_in,
            record,
            omissions,
        )?;
        tracks.push(
            animation
                .keys
                .scalar()
                .map(<[PrScalarKeyframe]>::to_vec)
                .unwrap_or_default(),
        );
    }
    let [red_keys, green_keys, blue_keys] = <[Vec<PrScalarKeyframe>; 3]>::try_from(tracks)
        .unwrap_or_else(|_| unreachable!("three channels"));
    let paired = red_keys.len() == green_keys.len()
        && red_keys.len() == blue_keys.len()
        && red_keys
            .iter()
            .zip(&green_keys)
            .zip(&blue_keys)
            .all(|((r, g), b)| {
                r.source_ticks == g.source_ticks
                    && r.source_ticks == b.source_ticks
                    && r.easing == g.easing
                    && r.easing == b.easing
            });
    if !paired {
        return Err(format!(
            "its {label} {red}, {green} and {blue} keys differ in time or easing, but Premiere keys a colour as one track"
        ));
    }
    red_keys
        .iter()
        .zip(&green_keys)
        .zip(&blue_keys)
        .map(|((r, g), b)| {
            let millis = keyframes::layer_millis(r.source_ticks, source_in).map_err(|error| error.to_string())?;
            if matches!(r.easing, PrKeyframeEasing::CubicBezier { .. }) {
                return Err(format!(
                    "{label} key at {millis} ms has Bezier easing; Premiere's Bezier interpolation between colours is unverified"
                ));
            }
            Ok(PrColourKeyframe {
                source_ticks: r.source_ticks,
                value: native_colour(param, [r.value, g.value, b.value])
                    .map_err(|reason| format!("{reason} at {millis} ms"))?,
                easing: r.easing,
            })
        })
        .collect()
}

/// The native static values and keyed parameters of the scalar `params`, in
/// their native `Params` order as the reader returns them, from their FX
/// static `values` in that order and the effect's `keyed` parameters. Each FX
/// value, static or keyed, maps into the clip's frame by `clip_value` and to
/// native units by its parameter's binding, within the parameter's Premiere
/// range ([`native_value`]); a keyed parameter's static value is its first
/// key's (see `PrEffect`).
fn export_scalar_params<const N: usize>(
    params: [&'static EffectParamSpec; N],
    mut values: [f64; N],
    keyed: &[(&'static EffectParamSpec, &AnimationGraphEntry)],
    clip_value: impl Fn(&EffectParamSpec, f64) -> f64,
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<([f64; N], Vec<PrEffectParamAnimation>), String> {
    let animations = params
        .iter()
        .filter_map(|param| keyed.iter().find(|(keyed, _)| keyed.id == param.id))
        .map(|&(param, entry)| {
            export_param_keys(
                param,
                entry,
                |value| clip_value(param, value),
                source_in,
                record,
                omissions,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (value, param) in values.iter_mut().zip(params) {
        *value = match animations
            .iter()
            .find(|animation| animation.param.id == param.id)
            .and_then(|animation| animation.keys.scalar()?.first())
        {
            Some(key) => key.value,
            None => native_value(param, "", clip_value(param, *value))?,
        };
    }
    Ok((values, animations))
}

/// The native point keys of the point parameter `param` from its FX x and y
/// entries, converted as Motion Position keys are. Premiere keys a point as
/// one, so the two tracks must share key times and easing. When only one axis
/// has a track, the other keeps its static value at every key: the point then
/// moves on a straight line along the keyed axis, which one native point track
/// reproduces exactly.
fn export_point_keys(
    param: &EffectParamSpec,
    [x, y]: [Option<&AnimationGraphEntry>; 2],
    static_value: [f64; 2],
    source_in: i64,
) -> Result<Option<Vec<PrPointKeyframe>>, String> {
    fn keys<'a>(
        entry: &'a AnimationGraphEntry,
        label: &str,
    ) -> Result<&'a [PropertyKeyframe], String> {
        match entry.animator.data() {
            AnimatorData::Keyframes {
                track,
                enabled: true,
                ..
            } => Ok(track.keyframes()),
            _ => Err(format!("{label} animation is disabled or not keyframed")),
        }
    }
    let label = param.label;
    let float = |key: &PropertyKeyframe| match key.value() {
        PropertyValue::Float(value) => Ok(*value),
        _ => Err(format!("{label} keys must have float values")),
    };
    let (x, y) = (
        x.map(|entry| keys(entry, label)).transpose()?,
        y.map(|entry| keys(entry, label)).transpose()?,
    );
    let points = match (x, y) {
        (None, None) => return Ok(None),
        (Some(x), Some(y)) => {
            let paired = x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|(x, y)| x.layer_time() == y.layer_time() && x.easing() == y.easing());
            if !paired {
                return Err(format!(
                    "its {label} x and y keys differ in time or easing, but Premiere keys each point as one track"
                ));
            }
            x.iter()
                .zip(y)
                .map(|(x, y)| Ok((x, [float(x)?, float(y)?])))
                .collect::<Result<Vec<_>, String>>()?
        }
        (Some(x), None) => x
            .iter()
            .map(|key| Ok((key, [float(key)?, static_value[1]])))
            .collect::<Result<Vec<_>, String>>()?,
        (None, Some(y)) => y
            .iter()
            .map(|key| Ok((key, [static_value[0], float(key)?])))
            .collect::<Result<Vec<_>, String>>()?,
    };
    let keys = points
        .into_iter()
        .map(|(key, value)| {
            Ok(PrPointKeyframe {
                source_ticks: keyframes::source_ticks(source_in, key.layer_time().as_millis())
                    .map_err(|error| error.to_string())?,
                value,
                easing: keyframes::native_easing(key.easing())
                    .map_err(|error| error.to_string())?,
                spatial_in_tangent: None,
                spatial_out_tangent: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    // Premiere measures Bezier speed along the point's path.
    if keys.windows(2).any(|pair| {
        matches!(pair[1].easing, PrKeyframeEasing::CubicBezier { .. })
            && pair[0].value == pair[1].value
    }) {
        return Err(format!(
            "{label} cubic easing on a stationary point cannot preserve Premiere velocity"
        ));
    }
    Ok(Some(keys))
}

/// The unrounded native keys of one keyed parameter: converted and checked as
/// Motion keys are, their FX values mapped into the clip's frame by
/// `clip_value` and to native units by the parameter's binding, and within the
/// parameter's Premiere range.
fn export_param_keys(
    param: &'static EffectParamSpec,
    entry: &AnimationGraphEntry,
    clip_value: impl Fn(f64) -> f64,
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffectParamAnimation, String> {
    let label = export_label(param);
    let AnimatorData::Keyframes {
        track,
        enabled: true,
        ..
    } = entry.animator.data()
    else {
        return Err(format!("{label} animation is disabled or not keyframed"));
    };
    let keys = if [LEGACY_LUMA_THRESHOLD, LEGACY_LUMA_CUTOFF].contains(param) {
        track
            .keyframes()
            .iter()
            .map(|key| {
                let PropertyValue::Float(value) = key.value() else {
                    return Err(format!("{label} keys must have float values"));
                };
                Ok(PrScalarKeyframe {
                    source_ticks: keyframes::source_ticks(source_in, key.layer_time().as_millis())
                        .map_err(|e| e.to_string())?,
                    value: *value,
                    easing: keyframes::native_easing(key.easing()).map_err(|e| e.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?
    } else {
        super::tesseract_to_premiere::export_scalar_keys(track, source_in, label, omissions, record)
            .map_err(|error| error.to_string())?
    };
    let keys = keys
        .into_iter()
        .map(|key| {
            Ok(PrScalarKeyframe {
                value: native_value(param, " key value", clip_value(key.value))?,
                ..key
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(PrEffectParamAnimation {
        param,
        keys: PrEffectParamKeys::Scalar(keys),
    })
}

/// The Transform record that the video of a mask-less stage group exports as,
/// last in its clip's chain, or why the video is no Transform stage (a nest
/// carries it then). The video's transform is the effect's values: both
/// points normalized to the `frame`, Uniform Scale on when the two axes are
/// equal statically and share one track, Skew Axis 90° more than the skew
/// axis under a skew ([`PrTransform::native_skew_axis`]), the opacity as
/// Opacity, bilinear sampling, and the
/// composition's shutter angle unless `shutter_angle` names the Shutter Angle
/// that carries the video's motion blur. Its `PositionX`/`PositionY`,
/// `ScaleX`/`ScaleY`, `Rotation` and `Opacity` tracks become the parameters'
/// keys from `source_in`, the video's key origin, as Motion keys are written;
/// keys on any other property, keys that the video's key clock cannot place
/// ([`VideoKeyClock::exact`]), 3D fields and the identity without keys have
/// no Transform. The values and keys pass
/// [`PrTransform::ensure_convertible`], the reader's rule, and the
/// approximated ones are reported as import reports them
/// ([`PrTransform::approximations`]).
pub(super) fn export_transform_stage(
    video: &VideoLayer,
    dynamics: &AnimationGraph,
    frame: [u32; 2],
    source_in: i64,
    shutter_angle: Option<f64>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let tracks = super::tesseract_to_premiere::layer_tracks(dynamics, video.id, |_| true)
        .ok_or("the video's animations are not all enabled keyframe tracks without dependencies")?;
    if let Some(clock) = VideoKeyClock::of(video) {
        if let Some(property) = tracks
            .iter()
            .find_map(|(property, track)| (!clock.exact(track)).then_some(property))
        {
            return Err(format!(
                "the video's {property:?} keys are not exported: {HELD_CLOCK_REASON}"
            ));
        }
    }
    if video.transform == super::background::identity_transform() && tracks.is_empty() {
        return Err("the video is at the identity without keys; a nest carries it".to_owned());
    }
    export_layer_transform(
        super::tesseract_to_premiere::MotionHost {
            id: video.id,
            transform: &video.transform,
        },
        dynamics,
        frame,
        source_in,
        shutter_angle,
        record,
        omissions,
    )
}

/// Shared affine/key lowering for a video or a validated nested-picture stage.
/// Video-only stage admission, including the identity restriction, stays above.
pub(super) fn export_layer_transform(
    host: super::tesseract_to_premiere::MotionHost<'_>,
    dynamics: &AnimationGraph,
    frame: [u32; 2],
    source_in: i64,
    shutter_angle: Option<f64>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    export_affine(
        host,
        dynamics,
        frame,
        source_in,
        shutter_angle,
        record,
        omissions,
        false,
    )
}

/// Adjustment Geometry2 keeps Anchor keys as native point controls, not Motion.
pub(super) fn export_adjustment_geometry2(
    host: super::tesseract_to_premiere::MotionHost<'_>,
    dynamics: &AnimationGraph,
    frame: [u32; 2],
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let mut effect = export_affine(
        host, dynamics, frame, source_in, None, record, omissions, true,
    )?;
    let PrEffectParams::Transform(transform) = effect.params else {
        return Err("adjustment Geometry2 requires affine controls".to_owned());
    };
    effect.params = PrEffectParams::AdjustmentGeometry2(transform);
    Ok(effect)
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared affine lowering with bounded Anchor-key admission"
)]
fn export_affine(
    host: super::tesseract_to_premiere::MotionHost<'_>,
    dynamics: &AnimationGraph,
    frame: [u32; 2],
    source_in: i64,
    shutter_angle: Option<f64>,
    record: &str,
    omissions: &mut dyn OmissionSink,
    anchor_keys: bool,
) -> Result<PrEffect, String> {
    use super::tesseract_to_premiere::{
        export_position_keys, export_scalar_keys, layer_tracks, scale_tracks_match,
    };
    let t = host.transform;
    let tracks = layer_tracks(dynamics, host.id, |_| true)
        .ok_or("the layer's animations are not all enabled keyframe tracks without dependencies")?;
    if t.position.z().is_some_and(|z| z != 0.0)
        || t.rotation_x != 0.0
        || t.rotation_y != 0.0
        || t.orientation != [0.0; 3]
    {
        return Err("the video has 3D transform fields, which a Transform cannot carry".to_owned());
    }
    for &property in tracks.keys() {
        match property {
            PropType::PositionX
            | PropType::PositionY
            | PropType::ScaleX
            | PropType::ScaleY
            | PropType::Rotation
            | PropType::Opacity => {}
            PropType::AnchorPointX | PropType::AnchorPointY if anchor_keys => {}
            PropType::Skew | PropType::SkewAxis => return Err(PrTransform::KEYED_SKEW.to_owned()),
            other => {
                return Err(format!(
                    "the video has {other:?} keys, which a Transform has no parameter for"
                ))
            }
        }
    }
    let [width, height] = frame.map(f64::from);
    let mut position = [t.position.x() / width, t.position.y() / height];
    let mut animations = Vec::new();
    match (
        tracks.get(&PropType::PositionX),
        tracks.get(&PropType::PositionY),
    ) {
        (Some(x), Some(y)) => {
            let keys =
                export_position_keys(x, y, source_in, frame).map_err(|error| error.to_string())?;
            if let Some(first) = keys.first() {
                position = first.value;
            }
            animations.push(PrEffectParamAnimation {
                param: &TRANSFORM_POSITION,
                keys: PrEffectParamKeys::Point(keys),
            });
        }
        (None, None) => {}
        _ => return Err("the video's Position keys are not paired".to_owned()),
    }
    let mut anchor_point = [t.anchor_point[0] / width, t.anchor_point[1] / height];
    match (
        tracks.get(&PropType::AnchorPointX),
        tracks.get(&PropType::AnchorPointY),
    ) {
        (Some(x), Some(y)) => {
            let keys =
                export_position_keys(x, y, source_in, frame).map_err(|error| error.to_string())?;
            if let Some(first) = keys.first() {
                anchor_point = first.value;
            }
            animations.push(PrEffectParamAnimation {
                param: crate::schema::ADJUSTMENT_GEOMETRY2
                    .bound_param("anchorPointX")
                    .ok_or("Geometry2 Anchor Point has no keyed binding")?,
                keys: PrEffectParamKeys::Point(keys),
            });
        }
        (None, None) => {}
        _ => return Err("the layer's Anchor Point keys are not paired".to_owned()),
    }
    // A keyed parameter's static value is its first key's (see `PrEffect`).
    let mut scalar = |param: &'static EffectParamSpec,
                      track: Option<&&PropertyKeyframeTrack>,
                      value: f64|
     -> Result<f64, String> {
        let Some(track) = track else {
            return native_value(param, "", value);
        };
        let keys = export_scalar_keys(track, source_in, param.label, omissions, record)
            .map_err(|error| error.to_string())?;
        for key in &keys {
            native_value(param, " key value", key.value)?;
        }
        let first = keys.first().map_or(value, |key| key.value);
        animations.push(PrEffectParamAnimation {
            param,
            keys: PrEffectParamKeys::Scalar(keys),
        });
        Ok(first)
    };
    let (scale_x, scale_y) = (tracks.get(&PropType::ScaleX), tracks.get(&PropType::ScaleY));
    let uniform_scale = t.scale[0] == t.scale[1]
        && match (scale_x, scale_y) {
            (Some(x), Some(y)) => scale_tracks_match(x, y),
            (None, None) => true,
            _ => false,
        };
    let (scale_height, scale_width) = if uniform_scale {
        let height = scalar(&TRANSFORM_SCALE_HEIGHT, scale_y, t.scale[1])?;
        (height, height)
    } else {
        let height = scalar(&TRANSFORM_SCALE_HEIGHT, scale_y, t.scale[1])?;
        let width = scalar(&TRANSFORM_SCALE_WIDTH, scale_x, t.scale[0])?;
        (height, width)
    };
    let rotation = scalar(
        &TRANSFORM_ROTATION,
        tracks.get(&PropType::Rotation),
        t.rotation,
    )?;
    let opacity = scalar(
        &TRANSFORM_OPACITY,
        tracks.get(&PropType::Opacity),
        t.opacity.value(),
    )?;
    let transform = PrTransform {
        anchor_point,
        position,
        uniform_scale,
        scale_height,
        scale_width,
        skew: native_value(&TRANSFORM_SKEW, "", t.skew)?,
        skew_axis: native_value(
            &TRANSFORM_SKEW_AXIS,
            "",
            PrTransform::native_skew_axis(t.skew, t.skew_axis),
        )?,
        rotation,
        opacity,
        composition_shutter_angle: shutter_angle.is_none(),
        shutter_angle: shutter_angle.unwrap_or(0.0),
        bicubic_sampling: false,
    };
    transform.ensure_convertible(&animations)?;
    for warning in transform.approximations(&animations) {
        approximate(omissions, record, warning);
    }
    Ok(PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Transform(transform),
        animations,
    })
}

/// `animation` as export writes it: its scalar key values rounded by
/// [`EffectParamSpec::written_value`].
fn written(animation: &PrEffectParamAnimation) -> PrEffectParamAnimation {
    let mut animation = animation.clone();
    if let PrEffectParamKeys::Scalar(keys) = &mut animation.keys {
        for key in keys {
            key.value = animation.param.written_value(key.value);
        }
    }
    animation
}

/// The native value of the FX `value` of `param` ([`EffectParamSpec::native_value`]),
/// or why it has none, naming the FX value and range: `{label}{what} {value}
/// is outside Premiere's {low} to {high} range` ([`export_label`]).
fn native_value(param: &EffectParamSpec, what: &str, value: f64) -> Result<f64, String> {
    param.native_value(value).ok_or_else(|| {
        let [low, high] = param
            .value_range()
            .map_or([f64::NAN; 2], |range| [*range.start(), *range.end()])
            .map(|bound| param.fx_value(bound));
        format!(
            "{}{what} {value} is outside Premiere's {low} to {high} range",
            export_label(param)
        )
    })
}

/// The name of `param` in export reasons, which quote FX values: a scaled
/// parameter is named by its FX parameter, because its native name has other
/// units.
fn export_label(param: &EffectParamSpec) -> &'static str {
    match param.binding {
        Some(EffectParamBinding::ScaledScalar { name, .. }) => name,
        _ => param.label,
    }
}

/// The effect's wire `type`, for messages.
pub(super) fn effect_type(payload: &EffectPayload) -> String {
    serde_json::to_value(payload)
        .ok()
        .and_then(|value| value.get("type")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
#[path = "tests/effects.rs"]
mod tests;
