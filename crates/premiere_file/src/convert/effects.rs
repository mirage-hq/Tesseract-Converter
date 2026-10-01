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
//! [`tint_tritone`]), and a Ramp is a `gradientRamp` on a clip whose frame is
//! the canvas (see [`gradient_ramp`] and [`ImportHost::frame`]).

use super::{keyframes, tesseract_to_premiere::WrittenAnimation};
use crate::{
    approximate,
    export_loss::OmissionSink,
    omit,
    schema::{
        EffectParamBinding, EffectParamSpec, EffectSpec, MaskBoundary, PrAnimatedProperty,
        PrBrightnessContrast, PrColour, PrColourKeyframe, PrCornerPin, PrDirectionalBlur, PrEffect,
        PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, PrFilmImpactBlur,
        PrFilmImpactDirectionalBlur, PrGaussianBlur, PrInvert, PrKeyframeEasing, PrLevels,
        PrMosaic, PrPointKeyframe, PrRamp, PrScalarKeyframe, PrStaticTransform, PrTint,
        PrTransform, PrVideoOccurrence, BRIGHTNESS_CONTRAST, BRIGHTNESS_CONTRAST_BRIGHTNESS,
        BRIGHTNESS_CONTRAST_CONTRAST, CORNER_PIN, DIRECTIONAL_BLUR_DIRECTION,
        DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_BLUR, FILM_IMPACT_BLUR_AMOUNT,
        FILM_IMPACT_DIRECTIONAL_BLUR, FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
        FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, INVERT_BLEND, LEVELS, MOSAIC, MOSAIC_HORIZONTAL_BLOCKS,
        MOSAIC_VERTICAL_BLOCKS, RAMP, RAMP_BLEND, RAMP_END, RAMP_END_COLOR, RAMP_START,
        RAMP_START_COLOR, TINT, TINT_AMOUNT, TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO,
        TRANSFORM_OPACITY, TRANSFORM_POSITION, TRANSFORM_ROTATION, TRANSFORM_SCALE_HEIGHT,
        TRANSFORM_SCALE_WIDTH, TRANSFORM_SKEW, TRANSFORM_SKEW_AXIS,
    },
    Omission, OmissionScope,
};
use fx_schema::{
    animator::{
        AnimationGraph, AnimationGraphEntry, AnimatorData, PropertyKeyframe, PropertyKeyframeTrack,
    },
    EffectData, EffectId, EffectPayload, EffectRecord, FXComposition, LayerData, LayerEffect,
    LayerId, MediaSourceKind, NonNegativeProperty, PositiveProperty, PropType, PropertyTarget,
    PropertyValue, TimeOffset, Transform, VideoLayer,
};
use std::collections::BTreeSet;

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

/// Why a Ramp needs a host whose frame is the canvas: the FX shader measures
/// the ramp in the layer frame's UV, Premiere in clip pixels (Oracle run E10).
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

/// Why a Transform converts on no host but a media clip's stage group: FX has
/// no transform effect, the staged video's transform carries it, and an
/// adjustment layer's geometric transform does not render in FX.
const TRANSFORM_HOST_REASON: &str = "a Transform converts only as the transform of a media clip's stage group; FX has no transform effect, and an adjustment layer's geometric transform does not render";

/// The facts of an imported clip that some effects' values depend on.
struct ImportHost {
    /// The clip's static similarity, for a Directional Blur.
    similarity: Result<ClipToComposition, String>,
    /// Whether the clip's frame is the canvas at identity Motion, for a Ramp,
    /// or why not ([`FRAME_RULE`]).
    frame: Result<(), String>,
    /// Whether the clip converts under a stage group, on which a Mosaic is
    /// omitted ([`STAGED_MOSAIC_REASON`]).
    staged: bool,
}

impl ImportHost {
    /// The host facts of `clip`, whose media frame is `source` pixels on a
    /// `canvas`, converting under `boundary`.
    fn of_clip(
        clip: &PrVideoOccurrence,
        boundary: MaskBoundary,
        source: [u32; 2],
        canvas: [u32; 2],
    ) -> Self {
        // Only a Directional Blur maps through the clip's Motion (see `fx_value`).
        let similarity = match boundary {
            MaskBoundary::Flat => ClipToComposition::of_clip(clip),
            MaskBoundary::Staged => Err(STAGED_DIRECTIONAL_BLUR_REASON.to_owned()),
        };
        let frame = match boundary {
            MaskBoundary::Staged => Err(STAGED_RAMP_REASON.to_owned()),
            MaskBoundary::Flat => {
                let transform = clip.transform;
                let motion_animated = clip
                    .animations
                    .iter()
                    .any(|animation| animation.property() != PrAnimatedProperty::Opacity);
                match super::premiere_to_tesseract::wipe_frame_reason(
                    source,
                    canvas,
                    transform.scale,
                    transform.rotation,
                    transform.anchor_point == transform.position,
                    motion_animated,
                ) {
                    None => Ok(()),
                    Some(reason) => Err(format!("{reason}; {FRAME_RULE}")),
                }
            }
        };
        Self {
            similarity,
            frame,
            staged: boundary == MaskBoundary::Staged,
        }
    }
}

/// A host clip's static similarity from its own frame into composition space.
/// Premiere applies a Directional Blur in the clip's frame, before Motion
/// (Oracle run E2), while FX `directionalBlur` blurs composition pixels along
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
/// quotient rounds 16 of them). Oracle run E5 fits Premiere's render of an
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
fn invert_levels(output_white: f64) -> LayerEffect {
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
/// with the original by the Amount (Oracle runs E6 and E7, fitted on AME
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
    /// Whether the clip is a stage group, on which a Directional Blur is
    /// omitted rather than mapped through `layer`'s Motion.
    pub(super) staged: bool,
    /// Whether the clip is a nest placement, or lies in a nest whose placement
    /// writes Motion, so that a Directional Blur is omitted rather than mapped
    /// through `layer`'s Motion alone.
    pub(super) nested: bool,
    /// The layer's FX static transform.
    pub(super) transform: &'a Transform,
    /// The clip's native source In, from which key times count, as for Motion
    /// keys.
    pub(super) source_in: i64,
    /// The clip's media frame in pixels (a nest's canvas), which a Ramp needs
    /// to equal `canvas` ([`FRAME_RULE`]).
    pub(super) frame: [u32; 2],
    /// The sequence canvas in pixels.
    pub(super) canvas: [u32; 2],
}

impl EffectHost<'_> {
    /// Whether this layer's frame is the canvas at identity Motion, as a Ramp
    /// needs ([`FRAME_RULE`]), or why not. The Motion export writes the static
    /// transform unchanged when it is the identity, so `written` needs no check.
    fn frame_is_canvas(&self, dynamics: &AnimationGraph) -> Result<(), String> {
        if self.staged || self.nested {
            return Err(STAGED_RAMP_REASON.to_owned());
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
            Some(reason) => Err(format!("{reason}; {FRAME_RULE}")),
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

/// The effects of an occurrence's video layer in stack order, with
/// composition-unique ids, and the keyframe tracks of their keyed parameters on
/// layer `layer_id`. A staged clip's group carries its Crop or Linear Wipe but
/// no effects, so its video layer takes only the effects that apply before the
/// mask, and the ones that apply after it are reported. A Directional Blur on
/// a staged clip is omitted ([`STAGED_DIRECTIONAL_BLUR_REASON`]), and so is a
/// Ramp ([`STAGED_RAMP_REASON`]); a Ramp also needs the clip's `source` frame
/// to be the `canvas` ([`FRAME_RULE`]). A clip staged for its one Transform
/// ([`PrVideoOccurrence::transform_stage`]) takes the effects that apply
/// before it; the Transform itself is the staged video's transform, not an
/// effect, and the effects that apply after it are reported. A Transform that
/// stages nothing is reported with the stage's reason, and one on a flat host
/// (an adjustment layer) with the host reason.
pub(super) fn import_effects(
    clip: &PrVideoOccurrence,
    layer_id: LayerId,
    boundary: MaskBoundary,
    source: [u32; 2],
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
    let host = ImportHost::of_clip(clip, boundary, source, canvas);
    for (position, effect) in (1..).zip(&clip.effects) {
        if boundary == MaskBoundary::Staged
            && matches!(stage, Ok(Some((index, _, _))) if index + 1 == position)
        {
            continue;
        }
        if position > carried {
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
            (PrEffectParams::Transform(_), Err(reason)) => Err(reason.to_owned()),
            _ => layer_effect(effect, &host),
        }
        .and_then(|layer_effect| {
            let id = ids.take();
            let record = EffectRecord::from_data(&EffectData::Identified {
                id,
                enabled: effect.enabled,
                effect: EffectPayload::Known(layer_effect),
            })
            .map_err(|error| error.to_string())?;
            let mut effect_tracks = Vec::new();
            for animation in &effect.animations {
                effect_tracks.extend(param_tracks(animation, id, clip.in_ticks, layer_id, &host)?);
            }
            Ok((record, effect_tracks))
        });
        match imported {
            Ok((record, effect_tracks)) => {
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
        // The reader keeps only straight spatial paths, along which both
        // coordinates follow the point key's temporal easing.
        (PrEffectParamKeys::Point(keys), Some(EffectParamBinding::Point { x, y })) => [x, y]
            .into_iter()
            .enumerate()
            .map(|(axis, fx_param)| {
                let keys = keys
                    .iter()
                    .map(|key| (key.source_ticks, key.value[axis], key.easing))
                    .collect();
                track(fx_param, keys)
            })
            .collect(),
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
        _ => Err(format!("keyframed {label} has no FX parameter")),
    }
}

/// Report each effect of a still image or Color Matte occurrence, whose Image
/// or Rect layer imports none; the layer itself still converts.
pub(super) fn omit_effects(clip: &PrVideoOccurrence, kind: &str, omissions: &mut Vec<Omission>) {
    if clip.stroke.is_some() {
        omit(
            omissions,
            OmissionScope::Feature,
            clip.id.as_deref().unwrap_or(clip.media.as_str()),
            format!("Film Impact Stroke was not imported: {kind} is not an opaque physical video"),
        );
    }
    for (position, effect) in (1..).zip(&clip.effects) {
        let state = if effect.enabled { "" } else { "bypassed " };
        omit(
            omissions,
            OmissionScope::Feature,
            clip.record(),
            format!(
                "{state}{} effect at stack position {position} was not imported: effects on a {kind} are not converted",
                effect.spec().display_name
            ),
        );
    }
}

fn layer_effect(effect: &PrEffect, host: &ImportHost) -> Result<LayerEffect, String> {
    match effect.params {
        PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels,
        }) => Ok(LayerEffect::GaussianBlur {
            blurriness: NonNegativeProperty::new(blurriness)
                .ok_or("Blurriness must be finite and nonnegative")?,
            // FX reads an absent flag as unchecked; import keeps that canonical form.
            repeat_edge_pixels: repeat_edge_pixels.then_some(true),
            layer_size: None,
        }),
        PrEffectParams::FilmImpactBlur(PrFilmImpactBlur {
            amount,
            repeat_edge_pixels,
        }) => Ok(LayerEffect::GaussianBlur {
            blurriness: NonNegativeProperty::new(FILM_IMPACT_BLUR_AMOUNT.fx_value(amount))
                .ok_or("Amount must be finite and nonnegative")?,
            repeat_edge_pixels: repeat_edge_pixels.then_some(true),
            layer_size: None,
        }),
        // Both normalize each corner to the clip's own frame.
        PrEffectParams::CornerPin(PrCornerPin {
            corners: [upper_left, upper_right, lower_left, lower_right],
        }) => Ok(LayerEffect::CornerPin {
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
        PrEffectParams::Levels(PrLevels { rgb }) => {
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
        PrEffectParams::Invert(PrInvert { blend }) => {
            Ok(invert_levels(fx_value(&INVERT_BLEND, blend, host)?))
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
        // The reader keeps only aligned linear ramps; the host frame is the
        // converter's rule.
        PrEffectParams::Ramp(ramp) => {
            host.frame.as_ref().map_err(String::clone)?;
            Ok(gradient_ramp(ramp))
        }
        // The reader keeps only Sharp Colors on with whole, held counts; the
        // grid is the same fraction of the frame at any Motion.
        PrEffectParams::Mosaic(mosaic) => {
            if host.staged {
                return Err(STAGED_MOSAIC_REASON.to_owned());
            }
            fx_mosaic(mosaic)
        }
        // A Transform is no effect record: the one Transform of a media clip
        // is its staged video's transform (`import_effects`), and every other
        // host fails closed here.
        PrEffectParams::Transform(_) => Err(TRANSFORM_HOST_REASON.to_owned()),
    }
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
/// that export as nests, on those groups, and on exported adjustment layers.
/// [`export_effects`] reports an animated parameter of one of them with its
/// effect, so the generic animation-target omission skips those targets.
/// `canvas` is the sequence size.
pub(super) fn video_effect_ids(
    composition: &FXComposition,
    canvas: [u32; 2],
) -> BTreeSet<EffectId> {
    super::nested::exported_video_layers(composition.layers(), composition.dynamics(), canvas)
        .into_iter()
        .flat_map(|layer| match layer.data() {
            LayerData::Video(video) => video.effects.as_slice(),
            LayerData::Media(media) if media.source.kind == MediaSourceKind::Video => {
                media.effects.as_slice()
            }
            LayerData::Group(group) => group.effects.as_slice(),
            LayerData::Adjustment(adjustment) => adjustment.effects.as_slice(),
            _ => &[],
        })
        .filter_map(|record| match record.data() {
            EffectData::Identified { id, .. } => Some(*id),
            EffectData::Legacy(_) => None,
        })
        .collect()
}

/// Export a video layer's effect stack in its current order onto the clip
/// `host`, which export places, recording each exported effect in `written`.
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
        match export_effect(id, enabled, payload, dynamics, host, record, omissions) {
            Ok(effect) => {
                if let Some(id) = id {
                    written.record_effect(id);
                }
                exported.push(effect);
            }
            Err(reason) => {
                let name = match id {
                    Some(id) => format!("effect {}", id.value()),
                    None => format!("effect at stack position {position}"),
                };
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!(
                        "effects: {} {name} was not exported: {reason}",
                        effect_type(payload)
                    ),
                );
            }
        }
    }
    exported
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
    let spec = effect_spec(payload);
    let mut keyed = Vec::new();
    for entry in dynamics.entries() {
        let PropertyTarget::EffectProperty(target) = &entry.target else {
            continue;
        };
        if Some(target.effect_id()) != id {
            continue;
        }
        let parameter = target.param_name();
        let Some(param) = spec.and_then(|spec| spec.bound_param(parameter)) else {
            return Err(format!(
                "animated {parameter} has no static Premiere value; only static effect parameters export"
            ));
        };
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
            enabled,
            params: PrEffectParams::Mosaic(mosaic),
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
            enabled,
            params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
                brightness,
                contrast,
            }),
            animations,
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
        EffectPayload::Known(LayerEffect::Levels { .. }) if !enabled => {
            return Err("a disabled Levels has no verified Premiere bypass form".to_owned())
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
            let source = PrLevels { rgb };
            let levels = PrLevels {
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
        enabled,
        params: PrEffectParams::Invert(PrInvert { blend }),
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
    host.frame_is_canvas(dynamics)?;
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
            "shape {shape} is not 0 (linear); a radial ramp is not converted, because Premiere measures its radius in clip pixels and the FX gradientRamp in frame UV (Oracle run E10 probe)"
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
        enabled,
        params: PrEffectParams::Ramp(ramp),
        animations,
    })
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
    let keys = super::tesseract_to_premiere::export_scalar_keys(
        track, source_in, label, omissions, record,
    )
    .map_err(|error| error.to_string())?;
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
/// keys from `source_in`, as Motion keys are written; keys on any other
/// property, 3D fields and the identity without keys have no Transform.
/// The values and keys pass
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
    use super::tesseract_to_premiere::{
        export_position_keys, export_scalar_keys, layer_tracks, scale_tracks_match,
    };
    let t = &video.transform;
    let tracks = layer_tracks(dynamics, video.id, |_| true)
        .ok_or("the video's animations are not all enabled keyframe tracks without dependencies")?;
    if *t == super::background::identity_transform() && tracks.is_empty() {
        return Err(
            "the video is at the identity without keys; a nest carries it (supervisor decision D22-14)"
                .to_owned(),
        );
    }
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
        anchor_point: [t.anchor_point[0] / width, t.anchor_point[1] / height],
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
