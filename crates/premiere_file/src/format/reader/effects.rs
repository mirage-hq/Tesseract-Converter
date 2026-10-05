//! Standard clip effects. `split_chain` divides an occurrence chain once:
//! intrinsic Motion/Opacity and active Crop, Linear Wipe or Track Matte Key go
//! to `animation`, other effects stay here.
//!
//! An active effect that changes what its clip covers or its transparency omits
//! the occurrence (`COVERAGE_EFFECTS`, masks). Gaussian Blur converts, with
//! keyed Blurriness, Corner Pin, with keyed corners, Directional Blur, with
//! keyed Direction and Blur Length, Levels, with keyed master levels,
//! Brightness & Contrast, with keyed Brightness and Contrast, Invert of
//! every channel, with keyed Blend With Original, Tint, with keyed colours and
//! Amount to Tint, and Black & White, which has no parameters. Every other
//! standard effect is omitted with a precise reason that names the effect, its
//! native versions and the clip that owns it, while the occurrence and its
//! other effects keep stack order, the reverse of the chain's `Index` order
//! (`chain_render_order`). Converted effects keep their side of an active Crop
//! or Linear Wipe, unless they are on both sides.

use crate::schema::{lens_curvature, LENS_CURVATURE, LENS_DISTORTION};
use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{graph::Element, Graph, Record},
    omit,
    schema::{
        chain_render_order, is_coverage_effect, mask_match_name,
        native::{Reference, VideoComponentParam, VideoFilterComponent},
        records, scalar_range, seconds,
        spatial::curved_segment,
        EffectParamBinding, EffectParamSpec, EffectSpec, PrBrightnessContrast, PrColour,
        PrColourKeyframe, PrCornerPin, PrDirectionalBlur, PrEffect, PrEffectParamAnimation,
        PrEffectParamKeys, PrEffectParams, PrFilmImpactBlur, PrFilmImpactDirectionalBlur,
        PrFindEdges, PrGaussianBlur, PrInvert, PrLevels, PrMatteChannel, PrMosaic, PrPointKeyframe,
        PrPosterize, PrRamp, PrReplicate, PrSharpen, PrTint, PrTransform, BLACK_WHITE,
        BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL, BRIGHTNESS_CONTRAST,
        BRIGHTNESS_CONTRAST_BRIGHTNESS, BRIGHTNESS_CONTRAST_CONTRAST, CORNER_PIN, DIRECTIONAL_BLUR,
        DIRECTIONAL_BLUR_DIRECTION, DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_BLUR,
        FILM_IMPACT_BLUR_26_2, FILM_IMPACT_BLUR_26_2_DEFAULTS, FILM_IMPACT_BLUR_AMOUNT,
        FILM_IMPACT_BLUR_CHROMATIC, FILM_IMPACT_BLUR_CONTROLS, FILM_IMPACT_BLUR_DEFAULTS,
        FILM_IMPACT_BLUR_EDGE, FILM_IMPACT_BLUR_THICKNESS, FILM_IMPACT_BLUR_UNIFORM,
        FILM_IMPACT_DIRECTIONAL_BLUR, FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
        FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, FILM_IMPACT_DIRECTIONAL_BLUR_DEFAULTS, FIND_EDGES,
        FIND_EDGES_BLEND, FIND_EDGES_INVERT, GAUSSIAN_BLUR, GAUSSIAN_BLUR_BLURRINESS,
        GAUSSIAN_BLUR_DIMENSIONS, GAUSSIAN_BLUR_MAX_BLURRINESS, GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS,
        INVERT, INVERT_BLEND, INVERT_CHANNEL, INVERT_CHANNEL_RGB, LEGACY_LUMA_KEY_MATCH_NAME,
        LEVELS, LEVELS_NEUTRAL, MASK_EFFECT_ORDER_REASON, MOSAIC, MOSAIC_HORIZONTAL_BLOCKS,
        MOSAIC_SHARP_COLORS, MOSAIC_VERTICAL_BLOCKS, NOISE, NOISE_AMOUNT, NOISE_CLIPPING,
        NOISE_COLOR, POSTERIZE, POSTERIZE_LEVEL, POSTERIZE_TIME, POSTERIZE_TIME_FRAME_RATE,
        PREMIERE_NATIVE_PARAMETER_ID, RAMP, RAMP_BLEND, RAMP_END, RAMP_END_COLOR, RAMP_SCATTER,
        RAMP_SHAPE, RAMP_SHAPE_LINEAR, RAMP_START, RAMP_START_COLOR, REPLICATE, REPLICATE_COUNT,
        SHARPEN, SHARPEN_AMOUNT, TINT, TINT_AMOUNT, TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO,
        TRACK_MATTE_KEY, TRACK_MATTE_KEY_COMPOSITE, TRACK_MATTE_KEY_MATTE, TRACK_MATTE_KEY_REVERSE,
        TRANSFORM, TRANSFORM_ANCHOR_POINT, TRANSFORM_COMPOSITION_SHUTTER_ANGLE, TRANSFORM_OPACITY,
        TRANSFORM_POSITION, TRANSFORM_ROTATION, TRANSFORM_SAMPLING, TRANSFORM_SAMPLING_BICUBIC,
        TRANSFORM_SAMPLING_BILINEAR, TRANSFORM_SCALE_HEIGHT, TRANSFORM_SCALE_WIDTH,
        TRANSFORM_SHUTTER_ANGLE, TRANSFORM_SKEW, TRANSFORM_SKEW_AXIS, TRANSFORM_UNIFORM_SCALE,
    },
    Omission, OmissionScope,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

mod lumetri;
mod offset;

const COMPONENT: &str = "Component";
const GEOMETRY2: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Geometry2",
    ..TRANSFORM
};

/// Children of a supported effect's parameter record. `Node` holds UI state.
const PARAM_CHILDREN: [&str; 13] = [
    "Node",
    "Name",
    "IsTimeVarying",
    "DiscontinuousInterpolate",
    "ParameterControlType",
    "StartKeyframe",
    "Keyframes",
    "CurrentValue",
    "LowerBound",
    "UpperBound",
    "ParameterID",
    "LowerUIBound",
    "UpperUIBound",
];

/// Effect Controls UI state. Other component properties, such as the
/// `ParentPinID` of Essential Graphics layers, can change what an effect
/// applies to and are rejected.
const UI_PROPERTIES: [&str; 2] = ["ECP.Filter.Expanded", "BE.VideoComponentChain.ChildPinID"];

/// An occurrence chain divided once into intrinsic components and standard
/// effects.
pub(super) struct SplitChain<'g, 'c> {
    /// Intrinsic components and active Crop, Linear Wipe or Track Matte Key in
    /// document order, for the Motion reader.
    pub(super) motion_and_masks: Vec<&'c Reference>,
    /// Standard effects with any active mask, in reverse document order (stack
    /// order when `ordered`); stack positions count in this order.
    standard: Vec<Record<'g>>,
    /// Whether every component `Index` agrees with its document order.
    ordered: bool,
}

/// Divide a chain's components. A `VideoFilterComponent` whose
/// `Component/Intrinsic` is not `true` is a standard effect. A missing
/// `Intrinsic` therefore reads as false; that shape is inferred, because every
/// real project in the corpus writes the flag.
pub(super) fn split_chain<'g, 'c>(
    graph: &'g Graph<'_>,
    components: &'c [Reference],
    chain: &str,
) -> Result<SplitChain<'g, 'c>> {
    let ordered = components.iter().enumerate().all(|(index, reference)| {
        reference
            .index
            .as_deref()
            .is_none_or(|value| value.parse::<usize>().ok() == Some(index))
    });
    let mut motion_and_masks = Vec::new();
    let mut standard = Vec::new();
    for reference in components {
        let record = graph.locate(reference, chain)?;
        let is_standard = record.tag() == records::VIDEO_FILTER_COMPONENT.tag
            && record.element().child(COMPONENT).is_some_and(|component| {
                component.child("Intrinsic").and_then(Element::text) != Some("true")
            });
        let native = NativeEffect::inspect(record);
        let unselected_matte = native.is_active_track_matte_key()
            && static_matte(graph, record).as_deref() == Some(TRACK_MATTE_NONE);
        if is_standard && !unselected_matte {
            standard.push(record);
        }
        if !is_standard || native.is_active_mask() {
            motion_and_masks.push(reference);
        }
    }
    Ok(SplitChain {
        motion_and_masks,
        // Premiere renders the chain in descending `Index`, the reverse of the
        // document order when `ordered`; otherwise no effect converts.
        standard: chain_render_order(standard).collect(),
        ordered,
    })
}

/// The occurrence that owns an effect stack, for omission context.
pub(super) struct EffectOwner<'a> {
    pub(super) occurrence: &'a str,
    /// The master clip whose own chain holds the stack, for the occurrence's
    /// source effects; `None` for the occurrence's own chain.
    pub(super) source: Option<&'a str>,
    /// Whether this occurrence has a separate Stroke geometry reader. Nested
    /// owners do not, so the shared effect reader must report their Stroke.
    pub(super) stroke_geometry: bool,
    pub(super) clip_name: Option<&'a str>,
    /// Native video track `Index`. Premiere labels index 0 as V1.
    pub(super) track_index: i64,
    pub(super) timeline_ticks: Range<i64>,
    /// Whether the clip's source frame is the sequence frame, unrotated: the
    /// only frame in which a Transform's Anchor Point and Position are
    /// measured.
    pub(super) source_is_canvas: bool,
    pub(super) adjustment: bool,
}

impl SplitChain<'_, '_> {
    /// Reject the occurrence when an active standard effect changes what its
    /// clip covers or its transparency. Converting the clip without that
    /// effect would leave it opaque over everything below. An active Track
    /// Matte Key is the clip's mask, which the Motion reader reads. Premiere
    /// renders a clip without its bypassed effects, so a bypassed one keeps
    /// the clip and `read_effects` reports it; a missing or invalid `Bypass`
    /// counts as active.
    pub(super) fn reject_coverage_effects(
        &self,
        graph: &Graph<'_>,
        effect_masks: bool,
    ) -> Result<()> {
        for (position, &record) in (1..).zip(&self.standard) {
            let native = NativeEffect::inspect(record);
            if native.bypass == Some("true")
                || native.match_name == Some(TRACK_MATTE_KEY.match_name)
            {
                continue;
            }
            if effect_masks && carries_mask(graph, record)?.is_some() {
                ensure!(
                    self.ordered,
                    "masked effect requires an unambiguous stack order"
                );
                // Admit mask and effect together; the converter still checks the
                // physical owner and the complete isolated-scope construction.
                let read = standard_effect_reader(&native, false, true)
                    .ok_or_else(|| unsupported("masked effect has no editable mapping"))?;
                let effect = read(graph, record, &native)?;
                ensure!(
                    !matches!(
                        effect.params,
                        PrEffectParams::Transform(_) | PrEffectParams::PosterizeTime { .. }
                    ) && !effect.requires_coverage(),
                    "masked geometry, temporal and coverage-changing effects are not converted"
                );
                read_effect_mask(graph, record, &mut Vec::new())?;
                continue;
            }
            let reason = if native.match_name == Some(LEGACY_LUMA_KEY_MATCH_NAME) {
                let admission = if self.ordered {
                    read_legacy_luma(graph, record, &native).map(|_| ())
                } else {
                    Err(unsupported(
                        "Legacy Luma requires an unambiguous effect stack order",
                    ))
                };
                if let Err(error) = admission {
                    reason(error)
                } else if let Some(mask) = carries_mask(graph, record)? {
                    format!("carries a mask ({mask} sub-component)")
                } else {
                    continue;
                }
            } else if is_coverage_effect(native.match_name, native.display_name) {
                "changes what the clip covers or its transparency".to_owned()
            } else if let Some(mask) = carries_mask(graph, record)? {
                format!("carries a mask ({mask} sub-component)")
            } else {
                continue;
            };
            return Err(unsupported(format!(
                "{} at stack position {position}: {reason}; the clip is not converted without it",
                native.identify()
            )));
        }
        Ok(())
    }

    /// Legacy Luma remains unadmitted on nests and multicam placements: the
    /// ordinary effect host does not establish its native coverage mapping.
    pub(super) fn reject_unconverted_coverage(&self, owner: &str) -> Result<()> {
        for (position, &record) in (1..).zip(&self.standard) {
            let native = NativeEffect::inspect(record);
            if native.match_name == Some(LEGACY_LUMA_KEY_MATCH_NAME)
                && native.bypass != Some("true")
            {
                return Err(unsupported(format!("{} at {owner} stack position {position}: the nest/multicam placement has no established Legacy Luma coverage mapping; coverage would change, so the occurrence is omitted", native.identify())));
            }
        }
        Ok(())
    }

    /// Reject a master clip's chain whose active Transform or Geometry2 can
    /// hide the clip where import leaves it out: import converts no source
    /// Transform (`convert::effects::import_source_effects`) and a Geometry2
    /// only as a centered positive zoom ([`read_centred_geometry2`]), so without one
    /// the clip would show where it hides the clip. Its Opacity must be
    /// readable and above 0 at every source time, its Bezier easing included
    /// ([`scalar_range`]), and its other values, read with its own record
    /// class's parameters (a Geometry2's saved Rotation marker included),
    /// must be readable and keep part of the picture in the clip's frame at
    /// every source time ([`PrTransform::hiding_geometry`]).
    ///
    /// That check holds for the whole picture alone in its frame, so a
    /// left-out one that moves, scales or turns the picture
    /// ([`PrTransform::changes_geometry`]) also rejects the chain beside
    /// another active effect that changes which part of the picture shows: a
    /// Transform or Geometry2 that does so, a Corner Pin, or a Mosaic, which
    /// samples each block at its centre, or a nondefault placement Crop.
    /// Their combined geometry is not evaluated. Whether a trim shows the
    /// hidden time is not evaluated, nor whether the placement's Motion moves
    /// a picture that stays in its frame
    /// off the canvas. A bypassed effect keeps the clip, since Premiere
    /// renders the clip without it, and an invalid `Bypass` counts as active
    /// and, for a Transform or Geometry2, unreadable. Checked on the native
    /// records, before `read_effects` reports an effect that it leaves out.
    pub(super) fn reject_hiding_transforms(
        &self,
        graph: &Graph<'_>,
        placement_crop: bool,
    ) -> Result<()> {
        // Each active effect that changes which part of the picture shows, at
        // its stack position, with why import leaves it out where it does.
        let mut reshaping = Vec::new();
        for (position, &record) in (1..).zip(&self.standard) {
            let native = NativeEffect::inspect(record);
            let (spec, left_out) = match native.match_name {
                _ if native.bypass == Some("true") => continue,
                Some(name) if name == TRANSFORM.match_name => {
                    (&TRANSFORM, "no source Transform converts")
                }
                // A Geometry2 that converts keeps its zoom.
                Some(name) if name == GEOMETRY2.match_name => {
                    if self.ordered && read_centred_geometry2(graph, record, &native).is_ok() {
                        reshaping.push((position, native, None));
                        continue;
                    }
                    (&GEOMETRY2, "it does not convert")
                }
                Some(name) if name == CORNER_PIN.match_name || name == MOSAIC.match_name => {
                    reshaping.push((position, native, None));
                    continue;
                }
                _ => continue,
            };
            let values = native
                .enabled()
                .and_then(|_| param_values(graph, record, spec))
                .and_then(|values| Ok((lowest_opacity(&values)?, values)));
            let hides = match values {
                Ok((lowest, values)) if lowest > 0.0 => match transform_params(&values) {
                    Ok(transform) => match transform.hiding_geometry(&values.animations) {
                        Some(geometry) => format!("can hide the clip: {geometry}"),
                        None => {
                            if transform.changes_geometry(&values.animations) {
                                reshaping.push((position, native, Some(left_out)));
                            }
                            continue;
                        }
                    },
                    Err(error) => {
                        format!("has parameters that cannot be read ({})", reason(error))
                    }
                },
                Ok((lowest, _)) => format!("can hide the clip: its Opacity reaches {lowest}"),
                Err(error) => format!("has an Opacity that cannot be read ({})", reason(error)),
            };
            return Err(unsupported(format!(
                "{} at stack position {position} {hides}, and {left_out}; the clip is not converted without it",
                native.identify()
            )));
        }
        let dropped = reshaping
            .iter()
            .find_map(|(position, native, left_out)| Some((*position, native, (*left_out)?)));
        if let Some((position, native, left_out)) = dropped.filter(|_| placement_crop) {
            return Err(unsupported(format!(
                "{} at stack position {position} can hide the clip: its geometry combined with the active nondefault placement Crop is not evaluated, and {left_out}; the clip is not converted without it",
                native.identify()
            )));
        }
        let beside = dropped
            .and_then(|(position, ..)| reshaping.iter().find(|(other, ..)| *other != position));
        match (dropped, beside) {
            (Some((position, native, left_out)), Some((other, beside, _))) => {
                Err(unsupported(format!(
                    "{} at stack position {position} can hide the clip: its geometry combined with {} at stack position {other} is not evaluated, and {left_out}; the clip is not converted without it",
                    native.identify(),
                    beside.identify()
                )))
            }
            _ => Ok(()),
        }
    }

    /// Moved/scaled adjustments have measured coverage only with no active
    /// standard effect or static full RGB Invert. Check native records before
    /// ordinary effect omissions erase an unsupported active effect.
    pub(super) fn reject_unmeasured_adjustment_coverage(&self, graph: &Graph<'_>) -> Result<()> {
        for &record in &self.standard {
            let native = NativeEffect::inspect(record);
            if native.bypass == Some("true") {
                continue;
            }
            ensure!(
                self.ordered && native.match_name == Some(INVERT.match_name),
                "{}: nondefault adjustment Motion supports only static RGB Invert or no active effect",
                native.identify()
            );
            let effect = read_invert(graph, record, &native)?;
            ensure!(
                matches!(effect.params, PrEffectParams::Invert(invert) if invert.channel == 0 && invert.blend == 0.0)
                    && effect.animations.is_empty(),
                "{}: nondefault adjustment Motion requires static full RGB Invert",
                native.identify()
            );
        }
        Ok(())
    }

    /// Check raw active records before best-effort parsing can discard an
    /// unknown effect or its failed keys. An interpreted source has no proven
    /// property clock for any of those losses.
    pub(super) fn require_static_interpreted_effects(
        &self,
        graph: &Graph<'_>,
        source: bool,
        source_is_canvas: bool,
    ) -> Result<()> {
        for &record in &self.standard {
            let native = NativeEffect::inspect(record);
            if native.bypass == Some("true") {
                continue;
            }
            // Intrinsic mask readers separately validate these records and
            // the occurrence guard rejects unsupported coverage clocks.
            if native.is_active_mask() {
                continue;
            }
            ensure!(
                self.ordered,
                "{}: interpreted picture requires an ordered static effect chain",
                native.identify()
            );
            let read = standard_effect_reader(&native, source, source_is_canvas)
                .ok_or_else(|| unsupported(format!("{}: interpreted picture has an unknown active effect; static semantics are unverified", native.identify())))?;
            let effect = read(graph, record, &native).map_err(|error| {
                unsupported(format!(
                    "{}: interpreted picture effect cannot be proved static: {error}",
                    native.identify()
                ))
            })?;
            ensure!(
                effect.animations.is_empty(),
                "{}: interpreted picture requires static occurrence and source effects",
                native.identify()
            );
        }
        Ok(())
    }

    /// Active Transform records in the chain, whether or not `read_effects`
    /// converts them: a second one omits the first even when it does not
    /// convert itself (`PrVideoOccurrence::transform_stage`). A bypassed
    /// Transform is not counted; Premiere renders the clip without it.
    pub(super) fn active_transforms(&self) -> usize {
        self.standard
            .iter()
            .filter(|&&record| NativeEffect::inspect(record).is_active_transform())
            .count()
    }

    /// Includes unknown native effects that `read_effects` will omit.
    pub(super) fn has_active_standard_effects(&self) -> bool {
        self.standard
            .iter()
            .any(|&record| NativeEffect::inspect(record).bypass != Some("true"))
    }

    /// Active standard effects not handled by the Motion/mask reader.
    /// Serialized bypassed effects do not change the nested picture. Missing
    /// or malformed bypass flags remain active, including on unknown effects.
    pub(super) fn has_active_effects_outside_motion(&self) -> bool {
        self.standard.iter().any(|&record| {
            let native = NativeEffect::inspect(record);
            native.bypass != Some("true") && !native.is_active_mask()
        })
    }

    /// A keyed Color Matte may carry its key, but no other active standard
    /// effect. Inspect native records before unsupported effects disappear.
    pub(super) fn has_active_non_matte_effects(&self) -> bool {
        self.standard.iter().any(|&record| {
            let native = NativeEffect::inspect(record);
            native.bypass != Some("true") && !native.is_active_track_matte_key()
        })
    }

    /// Raw Transform/Geometry2 presence for nest admission only. Geometry2
    /// must keep the existing separate inferred nest path; counting it as an
    /// ordinary clip Transform would change source/stage admission elsewhere.
    pub(super) fn has_active_nest_transform(&self) -> bool {
        self.standard.iter().any(|&record| {
            let native = NativeEffect::inspect(record);
            native.bypass != Some("true")
                && (native.match_name == Some(TRANSFORM.match_name)
                    || native.match_name == Some(GEOMETRY2.match_name))
        })
    }

    /// The additional import-only keyed-translation envelope has Geometry2
    /// evidence, not ordinary Transform evidence. Stack/mask admission remains
    /// the responsibility of `read_nest_transform`.
    pub(super) fn has_active_nest_geometry2(&self) -> bool {
        self.standard.iter().any(|&record| {
            let native = NativeEffect::inspect(record);
            native.bypass != Some("true") && native.match_name == Some(GEOMETRY2.match_name)
        })
    }

    /// Dropping an active Corner Pin would expose a full-frame nest over
    /// underlying content; the picture Group has no established pin frame.
    pub(super) fn has_active_nest_corner_pin(&self) -> bool {
        self.standard.iter().any(|&record| {
            let native = NativeEffect::inspect(record);
            native.bypass != Some("true") && native.match_name == Some(CORNER_PIN.match_name)
        })
    }

    /// Inspect the native stack before any lossy effect conversion. This is
    /// a separate inferred nest mapping; source-chain Geometry2 stays centered.
    pub(super) fn read_nest_transform(&self, graph: &Graph<'_>) -> Result<PrEffect> {
        let active: Vec<_> = self
            .standard
            .iter()
            .copied()
            .filter(|&record| NativeEffect::inspect(record).bypass != Some("true"))
            .collect();
        let is_transform = |record| {
            let name = NativeEffect::inspect(record).match_name;
            name == Some(TRANSFORM.match_name) || name == Some(GEOMETRY2.match_name)
        };
        ensure!(
            active.iter().copied().any(is_transform),
            "effects on a nested sequence occurrence are not converted"
        );
        ensure!(
            self.ordered && active.len() == 1 && is_transform(active[0]),
            "nested Transform requires exactly one active unmasked Transform"
        );
        let effect = read_transform(graph, active[0], &NativeEffect::inspect(active[0]))?;
        let PrEffectParams::Transform(transform) = &effect.params else {
            return Err(unsupported("nested Transform has no affine parameters"));
        };
        ensure!(transform.motion_blur_shutter_angle().is_none() && !transform.bicubic_sampling
            && effect.animations.iter().all(|animation|
                animation.param.id != TRANSFORM_SHUTTER_ANGLE.id
                && !(transform.uniform_scale && animation.param.id == TRANSFORM_SCALE_WIDTH.id)),
            "nested Transform requires fully retained controls and keys without motion blur or bicubic sampling");
        Ok(effect)
    }

    /// Only the measured sole standard Stroke can own occurrence geometry.
    pub(super) fn read_stroke(
        &self,
        graph: &Graph<'_>,
        omissions: &mut Vec<Omission>,
    ) -> Option<crate::schema::PrFilmImpactStroke> {
        let records: Vec<_> = self
            .standard
            .iter()
            .filter(|record| {
                NativeEffect::inspect(**record).match_name == Some("AE.Impact_Stroke_FX")
            })
            .collect();
        let first = records.first()?;
        let converted = if self.ordered && self.standard.len() == 1 && records.len() == 1 {
            super::stroke::read(graph, **first)
        } else {
            Err(unsupported("Stroke geometry requires one ordered standard effect without other effects or masks"))
        };
        match converted {
            Ok(profile) => Some(profile),
            Err(reason) => {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    first.identity(),
                    reason.to_string(),
                );
                None
            }
        }
    }

    /// Read the standard effects in stack order, and count those that apply
    /// before the Crop, Linear Wipe or Track Matte Key when the occurrence
    /// converts one (`mask`). An effect that cannot be
    /// represented is reported in `omissions` and left out; the others keep
    /// their order. Converted effects on both sides of the mask are all
    /// omitted, because one mask boundary keeps only one side in order.
    pub(super) fn read_effects(
        &self,
        graph: &Graph<'_>,
        owner: &EffectOwner<'_>,
        mask: bool,
        omissions: &mut Vec<Omission>,
    ) -> (Vec<PrEffect>, usize) {
        let mut effects = Vec::new();
        // The identity and description of each converted effect.
        let mut converted_effects = Vec::new();
        let mut above_mask = None;
        // A4's admission sees the native stack before unsupported records
        // disappear from the typed effects. An extra active effect must
        // never turn into the measured Transform + Alpha-key pair.
        let has_matte = self
            .standard
            .iter()
            .any(|&record| NativeEffect::inspect(record).is_active_track_matte_key());
        let active: Vec<_> = self
            .standard
            .iter()
            .map(|&record| NativeEffect::inspect(record))
            .filter(|effect| effect.bypass != Some("true"))
            .collect();
        let measured_pair = active.len() == 2
            && active
                .iter()
                .all(|effect| effect.is_active_transform() || effect.is_active_track_matte_key());
        for (position, &record) in (1..).zip(&self.standard) {
            let native = NativeEffect::inspect(record);
            if native.is_active_mask() {
                // The Motion owner reads the mask itself.
                if mask {
                    above_mask.get_or_insert(effects.len());
                }
                continue;
            }
            // Media occurrences read Stroke geometry separately. Nests have
            // no such host; source Strokes retain the ordinary unknown report.
            if native.match_name == Some("AE.Impact_Stroke_FX") && owner.source.is_none() {
                if !owner.stroke_geometry {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        &native.identity,
                        format!("{}: Film Impact Stroke was not imported: a nested sequence occurrence is not an opaque physical video", native.describe(position, owner)),
                    );
                }
                continue;
            }
            let description = native.describe(position, owner);
            // A clock effect on a masked host is unsupported regardless of side
            // or bypass. Drop it before mask accounting and typed A4 admission;
            // the native active-stack check above remains strict.
            if native.match_name == Some(POSTERIZE_TIME.match_name) && mask {
                omit(omissions, OmissionScope::Feature, &native.identity,
                    format!("{description}: Posterize Time on a masked host is not converted; supported sibling effects were kept"));
                continue;
            }

            let Some(read) =
                standard_effect_reader(&native, owner.source.is_some(), owner.source_is_canvas)
            else {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    &native.identity,
                    format!("unknown {description}: no Tesseract effect mapping"),
                );
                continue;
            };
            let mut omitted_controls = Vec::new();
            let mut approximations = Vec::new();
            let converted = if self.ordered
                && owner.adjustment
                && owner.source.is_none()
                && native.match_name == Some(GEOMETRY2.match_name)
            {
                // Keep the existing scale-only Corner Pin mapping (including
                // crossing lower windows); the composed stage fills its missing
                // Position/Rotation/Anchor control class, not a new zoom policy.
                let zoom = if owner.source_is_canvas {
                    read_geometry2(graph, record, &native)
                } else {
                    read_centred_geometry2(graph, record, &native)
                };
                zoom.or_else(|error| {
                    // Composite staging cannot cross an intrinsic mask boundary.
                    // Keep unsupported Geometry2 out of sibling mask accounting.
                    if mask {
                        Err(unsupported(format!("adjustment Geometry2 composite staging cannot cross a Crop/Wipe/Matte mask boundary; unsupported Geometry2 omitted without discarding supported sibling effects; scale-only mapping unavailable: {}", reason(error))))
                    } else {
                        read_adjustment_geometry2(graph, record, &native, &mut approximations)
                    }
                })
                .map(|effect| vec![effect])
            } else if native.is_active_transform() && has_matte && !measured_pair {
                Err(unsupported("Transform with Track Matte Key requires exactly those two active native effects; another active or unconverted effect is outside measured A4"))
            } else if self.ordered && native.match_name == Some(LEGACY_LUMA_KEY_MATCH_NAME) {
                read_legacy_luma_with_notes(graph, record, &native, &mut approximations)
                    .map(|e| vec![e])
            } else if self.ordered && native.match_name == Some(lumetri::MATCH_NAME) {
                lumetri::read(graph, record, &native, &mut omitted_controls)
            } else if self.ordered && native.match_name == Some(crate::schema::OFFSET.match_name) {
                offset::read(graph, record, &native).map(|effect| vec![effect])
            } else if self.ordered && native.match_name == Some(LEVELS.match_name) {
                read_levels_with_channels(graph, record, &mut omitted_controls)
            } else if self.ordered {
                (if native.match_name == Some("AE.ADBE Alpha Glow") {
                    read_alpha_glow_preserving_controls(
                        graph,
                        record,
                        &native,
                        &mut omitted_controls,
                    )
                } else {
                    read(graph, record, &native)
                })
                .map(|effect| vec![effect])
            } else {
                Err(unsupported("component Index values disagree with their order, so the stack order is ambiguous"))
            };
            let converted = converted.and_then(|mut effects| {
                if record.element().child("SubComponents").is_some() {
                    let mask = read_effect_mask(graph, record, omissions)?;
                    ensure!(
                        mask.is_none() || (owner.source.is_none() && effects.len() == 1),
                        "an effect mask requires one mapped occurrence effect"
                    );
                    if let [effect] = effects.as_mut_slice() {
                        // Alpha uses occurrence lowering, not an atomic masked
                        // effect record. Keep a bypassed parent out of that route
                        // so its independently live mask cannot discard the video.
                        ensure!(
                            mask.is_none()
                                || effect.enabled
                                || !matches!(
                                    effect.params,
                                    PrEffectParams::Invert(PrInvert { channel: 15, .. })
                                ),
                            "bypassed Invert Alpha with an effect mask is not mapped; original picture and supported siblings retained"
                        );
                        effect.mask = mask;
                    }
                }
                Ok(effects)
            });
            match converted {
                Ok(mut effect) => {
                    for note in approximations {
                        crate::approximate(
                            omissions,
                            &native.identity,
                            format!("{description}: {note}"),
                        );
                    }
                    for mapped in &mut effect {
                        for note in crate::schema::normalize_legacy_luma(mapped) {
                            crate::approximate(
                                omissions,
                                &native.identity,
                                format!("{description}: {note}"),
                            );
                        }
                    }
                    for reason in omitted_controls {
                        omit(
                            omissions,
                            OmissionScope::Feature,
                            &native.identity,
                            format!("{description}: {reason}"),
                        );
                    }
                    if native.match_name == Some(lumetri::MATCH_NAME) {
                        crate::approximate(
                            omissions,
                            &native.identity,
                            format!("{description}: {}", lumetri::REPLACEMENT),
                        );
                    }
                    if native.match_name == Some(crate::schema::OFFSET.match_name) {
                        crate::approximate(
                            omissions,
                            &native.identity,
                            format!("{description}: {}", offset::REPLACEMENT),
                        );
                    }
                    effects.extend(effect);
                    converted_effects.push((native.identity, description));
                }
                Err(error) => omit(
                    omissions,
                    OmissionScope::Feature,
                    &native.identity,
                    format!("{description}: {}", reason(error)),
                ),
            }
        }
        let above_mask = above_mask.unwrap_or(0);
        if above_mask == 0 || above_mask == effects.len() {
            return (effects, above_mask);
        }
        if effects.iter().any(PrEffect::requires_coverage) {
            for (identity, description) in converted_effects {
                crate::approximate(omissions, identity, format!("{description}: effects straddle a mask; retain the editable stack after the mask to preserve Legacy Luma coverage, approximating native processing order"));
            }
            return (effects, 0);
        }
        for (identity, description) in converted_effects {
            omit(
                omissions,
                OmissionScope::Feature,
                identity,
                format!("{description}: {MASK_EFFECT_ORDER_REASON}"),
            );
        }
        (Vec::new(), 0)
    }
}

type EffectReader = fn(&Graph<'_>, Record<'_>, &NativeEffect<'_>) -> Result<PrEffect>;

fn standard_effect_reader(
    native: &NativeEffect<'_>,
    source: bool,
    source_is_canvas: bool,
) -> Option<EffectReader> {
    let read = match native.match_name {
        Some(name) if name == GAUSSIAN_BLUR.match_name => read_gaussian_blur,
        Some(name) if name == FILM_IMPACT_BLUR.match_name => read_film_impact_blur,
        Some(name) if name == CORNER_PIN.match_name => match source {
            true => read_source_corner_pin,
            false => read_corner_pin,
        },
        Some(name) if name == DIRECTIONAL_BLUR.match_name => read_directional_blur,
        Some(name) if name == FILM_IMPACT_DIRECTIONAL_BLUR.match_name => {
            read_film_impact_directional_blur
        }
        Some(name) if name == LEVELS.match_name => read_levels,
        Some(lumetri::MATCH_NAME) => lumetri::reject_interpretation,
        Some("AE.ADBE Offset") => offset::reject_interpretation,
        Some(name) if name == BRIGHTNESS_CONTRAST.match_name => read_brightness_contrast,
        Some(name) if name == INVERT.match_name => read_invert,
        Some(name) if name == FIND_EDGES.match_name => read_find_edges,
        Some(name) if name == TINT.match_name => read_tint,
        Some(name) if name == BLACK_WHITE.match_name => read_black_white,
        Some(name) if name == RAMP.match_name => read_ramp,
        Some(name) if name == MOSAIC.match_name => read_mosaic,
        Some(name) if name == REPLICATE.match_name => read_replicate,
        Some(name) if name == POSTERIZE.match_name => read_posterize,
        Some(name) if name == SHARPEN.match_name => read_sharpen,
        Some(LEGACY_LUMA_KEY_MATCH_NAME) => read_legacy_luma,
        Some(name) if name == NOISE.match_name => read_noise,
        Some("AE.ADBE_Noise_FX") => read_modern_noise,
        Some(name) if name == POSTERIZE_TIME.match_name => read_posterize_time,
        Some("AE.ADBE Alpha Glow") => read_alpha_glow,
        Some(name) if name == TRANSFORM.match_name => read_transform,
        Some(name) if name == GEOMETRY2.match_name && source_is_canvas && !source => read_geometry2,
        Some(name) if name == GEOMETRY2.match_name => read_centred_geometry2,
        Some(name) if name == LENS_DISTORTION.match_name => read_lens_distortion,
        _ => return None,
    };
    Some(read)
}

/// Preserve a single-colour fading silhouette surrogate for all valid modes.
fn read_alpha_glow(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    read_alpha_glow_preserving_controls(graph, record, native, &mut Vec::new())
}

fn read_alpha_glow_preserving_controls(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
    losses: &mut Vec<String>,
) -> Result<PrEffect> {
    use crate::schema::{
        alpha_glow_size, ALPHA_GLOW, ALPHA_GLOW_BRIGHTNESS, ALPHA_GLOW_END, ALPHA_GLOW_FADE,
        ALPHA_GLOW_SIZE, ALPHA_GLOW_START, ALPHA_GLOW_USE_END,
    };
    let values = param_values(graph, record, &ALPHA_GLOW)?;
    match values.get(&ALPHA_GLOW_USE_END)? {
        "true" => losses.push("Alpha Glow Use End Color=true: Start Color retained; two-color interpolation and End Color contribution are replaced by a single-color halo".to_owned()),
        "false" => {},
        _ => return Err(unsupported("invalid Alpha Glow Use End Color boolean")),
    }
    match values.get(&ALPHA_GLOW_FADE)? {
        "false" => losses.push("Alpha Glow Fade Out=false: solid falloff replaced by soft OuterGlow falloff; size/color/opacity remain editable".to_owned()),
        "true" => {},
        _ => return Err(unsupported("invalid Alpha Glow Fade Out boolean")),
    }
    let size = values.scalar(&ALPHA_GLOW_SIZE)?;
    alpha_glow_size(size, &values.animations).map_err(unsupported)?;
    let brightness = values.scalar(&ALPHA_GLOW_BRIGHTNESS)?;
    if brightness.fract() != 0.0 {
        return Err(unsupported("Alpha Glow Brightness must be whole"));
    }
    let color = values.colour(&ALPHA_GLOW_START)?;
    values.colour(&ALPHA_GLOW_END)?;
    losses.extend(values.flattened_controls);
    Ok(PrEffect {
        mask: None,
        enabled: native.enabled()?,
        params: PrEffectParams::AlphaGlow {
            size,
            brightness,
            color,
        },
        animations: values.animations,
    })
}

fn read_effect_mask(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<Option<crate::schema::PrMask>> {
    let filter = graph.decode::<VideoFilterComponent>(record)?;
    filter
        .value
        .sub_components
        .as_ref()
        .map(|sub_components| {
            super::mask::read_opacity_mask(graph, &filter.identity, sub_components, omissions)
        })
        .transpose()
        .map(Option::flatten)
}

/// The match name of the first mask record among an effect's
/// `SubComponents`, in any saved form ([`mask_match_name`]).
fn carries_mask(graph: &Graph<'_>, record: Record<'_>) -> Result<Option<&'static str>> {
    let identity = record.identity();
    for sub_component in record
        .element()
        .child("SubComponents")
        .into_iter()
        .flat_map(Element::children)
    {
        let target = graph.locate(&sub_component.reference(), &identity)?;
        if let Some(mask) =
            mask_match_name(target.element().child("MatchName").and_then(Element::text))
        {
            return Ok(Some(mask));
        }
    }
    Ok(None)
}

/// An omission reason without the generic "unsupported conversion" prefix.
pub(super) fn reason(error: BuildError) -> String {
    match error {
        BuildError::Unsupported(reason) => reason,
        other => other.to_string(),
    }
}

/// Native identity read leniently, so that even an undecodable effect is named.
struct NativeEffect<'a> {
    identity: String,
    match_name: Option<&'a str>,
    display_name: Option<&'a str>,
    version: Option<&'a str>,
    component_version: Option<&'a str>,
    bypass: Option<&'a str>,
}

impl<'a> NativeEffect<'a> {
    fn inspect(record: Record<'a>) -> Self {
        let element = record.element();
        let component = element.child(COMPONENT);
        let component_text = |tag: &str| {
            component
                .and_then(|component| component.child(tag))
                .and_then(Element::text)
        };
        Self {
            identity: record.identity(),
            match_name: element.child("MatchName").and_then(Element::text),
            display_name: component_text("DisplayName"),
            version: element.attribute("Version"),
            component_version: component.and_then(|component| component.attribute("Version")),
            bypass: component_text("Bypass"),
        }
    }

    /// Whether this is an active Crop, Linear Wipe or Track Matte Key, which
    /// the Motion owner reads. A missing or invalid `Bypass` counts as active.
    fn is_active_mask(&self) -> bool {
        (matches!(
            self.match_name,
            Some("AE.ADBE AECrop" | "AE.ADBE Linear Wipe")
        ) || self.match_name == Some(TRACK_MATTE_KEY.match_name))
            && self.bypass != Some("true")
    }

    /// Whether this is an active Transform, convertible or not. A missing or
    /// invalid `Bypass` counts as active, as for a mask.
    fn is_active_transform(&self) -> bool {
        self.match_name == Some(TRANSFORM.match_name) && self.bypass != Some("true")
    }

    /// Whether this is a Track Matte Key that Premiere applies. A missing or
    /// invalid `Bypass` counts as active.
    fn is_active_track_matte_key(&self) -> bool {
        self.match_name == Some(TRACK_MATTE_KEY.match_name) && self.bypass != Some("true")
    }

    /// A missing `Bypass` reads as false. That shape is inferred: every real
    /// project in the corpus writes the flag.
    fn enabled(&self) -> Result<bool> {
        match self.bypass {
            None | Some("false") => Ok(true),
            Some("true") => Ok(false),
            Some(other) => Err(unsupported(format!("invalid Bypass {other:?}"))),
        }
    }

    /// Bypass state, names and record versions.
    fn identify(&self) -> String {
        let state = match self.bypass {
            None | Some("false") => "active",
            Some("true") => "bypassed",
            Some(_) => "invalid-Bypass",
        };
        format!(
            "{state} effect {:?} (match name {:?}, VideoFilterComponent version {}, Component version {})",
            self.display_name.unwrap_or("<no DisplayName>"),
            self.match_name.unwrap_or("<no MatchName>"),
            self.version.unwrap_or("<none>"),
            self.component_version.unwrap_or("<none>"),
        )
    }

    fn describe(&self, position: usize, owner: &EffectOwner<'_>) -> String {
        let stack = match owner.source {
            Some(master) => format!("source stack position {position} of {master}"),
            None => format!("stack position {position}"),
        };
        format!(
            "{} at {stack} on clip {:?} ({}, V{}, {} s to {} s)",
            self.identify(),
            owner.clip_name.unwrap_or("<unnamed>"),
            owner.occurrence,
            i128::from(owner.track_index) + 1,
            seconds(owner.timeline_ticks.start),
            seconds(owner.timeline_ticks.end),
        )
    }
}

fn read_gaussian_blur(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &GAUSSIAN_BLUR)?;
    let first_key = values
        .animations
        .iter()
        .find(|animation| animation.param.id == GAUSSIAN_BLUR_BLURRINESS.id)
        .and_then(|animation| animation.keys.scalar()?.first());
    // A keyed Blurriness starts at its first key, the value AME renders before it.
    let blurriness = match first_key {
        Some(key) => key.value,
        None => {
            let blurriness = values.get(&GAUSSIAN_BLUR_BLURRINESS)?;
            blurriness
                .parse::<f64>()
                .ok()
                .filter(|value| {
                    value.is_finite() && (0.0..=GAUSSIAN_BLUR_MAX_BLURRINESS).contains(value)
                })
                .ok_or_else(|| {
                    unsupported(format!(
                        "Blurriness {blurriness:?} is not a number from 0 to {GAUSSIAN_BLUR_MAX_BLURRINESS}"
                    ))
                })?
        }
    };
    let dimensions = values.get(&GAUSSIAN_BLUR_DIMENSIONS)?;
    ensure!(
        dimensions == BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL,
        "Blur Dimensions {dimensions:?} is not Horizontal and Vertical ({BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL}); FX gaussianBlur always blurs both axes"
    );
    let repeat_edge_pixels = match values.get(&GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS)? {
        "true" => true,
        "false" => false,
        other => {
            return Err(unsupported(format!(
                "invalid Repeat Edge Pixels value {other:?}"
            )))
        }
    };
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels,
        }),
        animations: values.animations,
    })
}

/// The current Gaussian Blur when FX `gaussianBlur` can express it: a uniform
/// blur (Uniform Blur on, or a static Amount equal to Thickness) without
/// Chromatic Aberration, with repeated edges or a transparent exterior, and
/// with every hidden parameter at its default.
fn read_film_impact_blur(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = film_impact_values(
        graph,
        record,
        &[
            (&FILM_IMPACT_BLUR, &FILM_IMPACT_BLUR_DEFAULTS),
            (&FILM_IMPACT_BLUR_26_2, &FILM_IMPACT_BLUR_26_2_DEFAULTS),
        ],
        "gaussianBlur",
    )?;
    let amount = values.scalar(&FILM_IMPACT_BLUR_AMOUNT)?;
    ensure!(
        values.get(&FILM_IMPACT_BLUR_UNIFORM)? == "true"
            || (values.animations.is_empty()
                && amount == values.scalar(&FILM_IMPACT_BLUR_THICKNESS)?),
        "Uniform Blur is off and Amount is not a static value equal to Thickness; FX gaussianBlur cannot represent a directional blur"
    );
    let repeat_edge_pixels = match values.get(&FILM_IMPACT_BLUR_EDGE)? {
        "1" => true,
        "2" => false,
        other => {
            return Err(unsupported(format!(
                "Edge Behavior {other} is not supported; FX gaussianBlur repeats edges (1) or leaves the exterior transparent (2)"
            )))
        }
    };
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::FilmImpactBlur(PrFilmImpactBlur {
            amount,
            repeat_edge_pixels,
        }),
        animations: values.animations,
    })
}

/// The current Directional Blur when FX `directionalBlur` can express it:
/// without Chromatic Aberration, with every hidden parameter at its default,
/// and with the Legacy blur's transparent exterior (Edge Behavior 2) or
/// Premiere's default repeated edges (1). Repeated edges differ from the
/// transparent exterior only near the frame edges, so both import as the same
/// blur. Seed is not kept.
fn read_film_impact_directional_blur(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = film_impact_values(
        graph,
        record,
        &[(
            &FILM_IMPACT_DIRECTIONAL_BLUR,
            &FILM_IMPACT_DIRECTIONAL_BLUR_DEFAULTS,
        )],
        "directionalBlur",
    )?;
    let edge = values.get(&FILM_IMPACT_BLUR_EDGE)?;
    ensure!(
        matches!(edge, "1" | "2"),
        "Edge Behavior {edge} is not supported; only repeated edges (1) and a transparent exterior (2) import as FX directionalBlur"
    );
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::FilmImpactDirectionalBlur(PrFilmImpactDirectionalBlur {
            angle: values.scalar(&FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE)?,
            amount: values.scalar(&FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT)?,
        }),
        animations: values.animations,
    })
}

/// The values of a Film Impact effect saved in one of its `layouts` when it
/// has no Chromatic Aberration and every hidden parameter has Premiere
/// 26.5.1's value in the layout's defaults, its static values in `Params`
/// order. The parameter count selects the layout, whose identities
/// [`param_values`] then checks; any other count is checked against the
/// first, current layout.
fn film_impact_values(
    graph: &Graph<'_>,
    record: Record<'_>,
    layouts: &[(&EffectSpec, &[&str])],
    fx_effect: &str,
) -> Result<ParamValues> {
    let count = graph
        .decode::<VideoFilterComponent>(record)?
        .value
        .component
        .and_then(|body| body.params)
        .map_or(0, |params| params.items.len());
    let &(spec, defaults) = layouts
        .iter()
        .find(|(spec, _)| spec.params.len() == count)
        .unwrap_or(&layouts[0]);
    let values = param_values(graph, record, spec)?;
    ensure!(
        values.scalar(&FILM_IMPACT_BLUR_CHROMATIC)? == 0.0,
        "Chromatic Aberration changes the colour channels independently; FX {fx_effect} cannot represent it"
    );
    for (param, default) in spec.params.iter().zip(defaults) {
        if FILM_IMPACT_BLUR_CONTROLS
            .iter()
            .any(|control| control.id == param.id)
        {
            continue;
        }
        let value = values.get(param)?;
        ensure!(
            value == *default
                || matches!((value.parse::<f64>(), default.parse::<f64>()),
                    (Ok(value), Ok(default)) if value == default),
            "hidden ParameterID {} ({:?}) is {value:?}, not Premiere 26.5.1's {default:?}; its effect on the picture is unverified",
            param.id,
            param.name
        );
    }
    Ok(values)
}

fn read_corner_pin(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    corner_pin(graph, record, native, false)
}

/// A master clip's Corner Pin, whose curved corner paths keep their saved
/// tangents for import to approximate with straight keys or report
/// (`convert::corner_path`), when their keys have the form that Premiere
/// saved (`animation::ensure_saved_curve_form`). Only its static corners are
/// checked here: the quad checks between keys assume straight paths.
fn read_source_corner_pin(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    corner_pin(graph, record, native, true)
}

/// A Corner Pin, whose corners may move on curved paths only when
/// `keep_curves` holds.
fn corner_pin(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
    keep_curves: bool,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values_with(graph, record, &CORNER_PIN, keep_curves)?;
    let mut corners = [[0.0; 2]; 4];
    for (corner, param) in corners.iter_mut().zip(CORNER_PIN.params) {
        *corner = values.point(param)?;
    }
    let pin = PrCornerPin { corners };
    let curved = values.animations.iter().any(|animation| {
        animation
            .keys
            .point()
            .is_some_and(|keys| curved_segment(keys).is_some())
    });
    let checked = if curved {
        &[][..]
    } else {
        &values.animations[..]
    };
    pin.ensure_convex(checked).map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::CornerPin(pin),
        animations: values.animations,
    })
}

fn read_directional_blur(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &DIRECTIONAL_BLUR)?;
    let blur = PrDirectionalBlur {
        direction: values.scalar(&DIRECTIONAL_BLUR_DIRECTION)?,
        blur_length: values.scalar(&DIRECTIONAL_BLUR_LENGTH)?,
    };
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::DirectionalBlur(blur),
        animations: values.animations,
    })
}

/// Interpretation admission must prove the whole native effect before the
/// ordinary reader can replace any unsupported channel correction.
fn read_levels(
    graph: &Graph<'_>,
    record: Record<'_>,
    _native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let mut omitted_controls = Vec::new();
    let effect = read_levels_preserving_master(graph, record, &mut omitted_controls)?;
    ensure!(
        omitted_controls.is_empty(),
        "Levels has unsupported channel corrections; the whole effect is not proved static"
    );
    Ok(effect)
}

/// Levels keeps its measured master or the static neutral-master selector
/// mapping. Other static RGB rows are omitted with their values, rather than
/// dropping the master. Private data must still repeat every StartKeyframe;
/// unrepresentable master forms, channel keys and invalid records still reject.
fn read_levels_preserving_master(
    graph: &Graph<'_>,
    record: Record<'_>,
    omitted_controls: &mut Vec<String>,
) -> Result<PrEffect> {
    let values = param_values(graph, record, &LEVELS)?;
    let stored = private_values(graph, record)?;
    let mut rgb = [0.0; 5];
    let mut channels = [LEVELS_NEUTRAL; 3];
    for ((index, param), stored) in LEVELS.params.iter().enumerate().zip(stored) {
        let start = values.start_keyframes[&param.id]
            .split(',')
            .nth(1)
            .unwrap_or_default();
        let value = f64::from(stored);
        ensure!(
            start.parse().ok() == Some(value),
            "PremiereFilterPrivateData stores {stored} for {}, not its StartKeyframe value {start:?}",
            param.label
        );
        let Some(master) = rgb.get_mut(index) else {
            ensure!(
                param
                    .value_range()
                    .is_some_and(|range| range.contains(&value)),
                "{} {value} is outside Premiere's {} to {} range",
                param.label,
                param.lower_bound,
                param.upper_bound
            );
            channels[index / 5 - 1][index % 5] = value;
            continue;
        };
        // A keyed level starts at its first key, as a keyed Blurriness does.
        *master = match values
            .animations
            .iter()
            .find(|animation| animation.param.id == param.id)
            .and_then(|animation| animation.keys.scalar()?.first())
        {
            Some(key) => key.value,
            None => {
                ensure!(
                    param
                        .value_range()
                        .is_some_and(|range| range.contains(&value)),
                    "{} {value} is outside Premiere's {} to {} range",
                    param.label,
                    param.lower_bound,
                    param.upper_bound
                );
                value
            }
        };
    }
    let selectors = match channels.map(crate::schema::PrLevelChannel::from_values) {
        [Some(red), Some(green), Some(blue)] => Some([red, green, blue]),
        _ => None,
    };
    let levels = if channels == [LEVELS_NEUTRAL; 3] {
        PrLevels::Master { rgb }
    } else if let Some(selectors) =
        selectors.filter(|_| rgb == LEVELS_NEUTRAL && values.animations.is_empty())
    {
        PrLevels::Channels(selectors)
    } else {
        // Keep the measured master mapping rather than losing the whole effect.
        // Persisted FX Levels has no individual rows; do not assume that native
        // per-channel transfer is equivalent to the SDR ColorCurves operation.
        for (name, row) in ["R", "G", "B"].into_iter().zip(channels) {
            if row != LEVELS_NEUTRAL {
                omitted_controls.push(format!(
                    "Levels ({name}) row {row:?} was omitted (Black Input, White Input, Black Output, White Output, Gamma); retained the master values and keys with a neutral {name} row because this per-channel correction has no established editable mapping"
                ));
            }
        }
        PrLevels::Master { rgb }
    };
    levels
        .ensure_rendered_form(&values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Levels(levels),
        animations: values.animations,
    })
}

/// The little-endian u16 values of a Levels record's
/// `PremiereFilterPrivateData`, one per parameter. Premiere
/// stores each distinct blob once; a copy is empty and names it by
/// `BinaryHash`.
fn private_values(graph: &Graph<'_>, record: Record<'_>) -> Result<Vec<u16>> {
    let data = record
        .element()
        .child("PremiereFilterPrivateData")
        .ok_or_else(|| unsupported("missing PremiereFilterPrivateData"))?;
    ensure!(
        data.attribute("Encoding") == Some(records::ENCODING),
        "PremiereFilterPrivateData is not base64"
    );
    let text = data.text().unwrap_or_default();
    let stored = if text.trim().is_empty() {
        let hash = data.attribute("BinaryHash").unwrap_or_default();
        graph
            .binary_value(hash, &record.identity())?
            .ok_or_else(|| {
                unsupported(format!(
                    "PremiereFilterPrivateData names missing binary {hash:?}"
                ))
            })?
    } else {
        text
    };
    let bytes = STANDARD
        .decode(stored.split_whitespace().collect::<String>())
        .map_err(|error| {
            unsupported(format!("invalid PremiereFilterPrivateData base64: {error}"))
        })?;
    ensure!(
        bytes.len() == 2 * LEVELS.params.len(),
        "PremiereFilterPrivateData has {} bytes, not 2 for each of the {} parameters",
        bytes.len(),
        LEVELS.params.len()
    );
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect())
}

fn read_brightness_contrast(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &BRIGHTNESS_CONTRAST)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness: values.scalar(&BRIGHTNESS_CONTRAST_BRIGHTNESS)?,
            contrast: values.scalar(&BRIGHTNESS_CONTRAST_CONTRAST)?,
        }),
        animations: values.animations,
    })
}

/// RGB0 lowers to Levels; Alpha15 is retained for occurrence-level lowering.
/// Other or keyed selections omit only the effect. Its
/// `PremiereFilterPrivateData` holds no parameter data and is ignored
/// (`EffectSpec::opaque_private_data`).
fn read_invert(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &INVERT)?;
    let channel = values.get(&INVERT_CHANNEL)?;
    ensure!(
        channel == INVERT_CHANNEL_RGB || channel == "15",
        "Channel {channel:?} is not RGB ({INVERT_CHANNEL_RGB}); FX levels inverts every channel"
    );
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Invert(PrInvert {
            channel: if channel == "15" { 15 } else { 0 },
            blend: values.scalar(&INVERT_BLEND)?,
        }),
        animations: values.animations,
    })
}

fn read_find_edges(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &FIND_EDGES)?;
    let invert = match values.get(&FIND_EDGES_INVERT)? {
        "true" => true,
        "false" => false,
        other => return Err(unsupported(format!("invalid Invert value {other:?}"))),
    };
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::FindEdges(PrFindEdges {
            invert,
            blend: values.scalar(&FIND_EDGES_BLEND)?,
            blend_animated: !values.animations.is_empty(),
        }),
        // Blend keys were parsed and range-checked, but have no FX target.
        animations: Vec::new(),
    })
}

fn read_tint(graph: &Graph<'_>, record: Record<'_>, native: &NativeEffect<'_>) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &TINT)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Tint(PrTint {
            black: values.colour(&TINT_MAP_BLACK_TO)?,
            white: values.colour(&TINT_MAP_WHITE_TO)?,
            amount: values.scalar(&TINT_AMOUNT)?,
        }),
        animations: values.animations,
    })
}

/// A Black & White has no parameters; `param_values` still checks the record's
/// shape (Premiere 26.5.1 saves it without a `Params` element).
fn read_black_white(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &BLACK_WHITE)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::BlackWhite,
        animations: values.animations,
    })
}

/// A Ramp converts only as a linear ramp without scatter whose axis stays
/// aligned with the frame ([`PrRamp::ensure_aligned`]); the converter adds the
/// host rule (a frame-size clip at identity Motion).
fn read_ramp(graph: &Graph<'_>, record: Record<'_>, native: &NativeEffect<'_>) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &RAMP)?;
    let shape = values.get(&RAMP_SHAPE)?;
    ensure!(
        shape == RAMP_SHAPE_LINEAR,
        "Ramp Shape {shape:?} is not linear ({RAMP_SHAPE_LINEAR}); a radial ramp is not converted, because Premiere measures its radius in clip pixels and the FX gradientRamp in frame UV"
    );
    let scatter = values.scalar(&RAMP_SCATTER)?;
    ensure!(
        scatter == 0.0,
        "Ramp Scatter {scatter} is not converted; FX has no scatter"
    );
    let ramp = PrRamp {
        start: values.point(&RAMP_START)?,
        start_colour: values.colour(&RAMP_START_COLOR)?,
        end: values.point(&RAMP_END)?,
        end_colour: values.colour(&RAMP_END_COLOR)?,
        blend: values.scalar(&RAMP_BLEND)?,
    };
    ramp.ensure_aligned(&values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Ramp(ramp),
        animations: values.animations,
    })
}

/// The static values of one active Track Matte Key: the matte track's
/// persistent `ID` and the channel that keys the clip.
pub(super) struct TrackMatteKey {
    pub(super) matte_track_id: usize,
    pub(super) channel: PrMatteChannel,
}

/// The Matte value of a Track Matte Key with no matte track selected, as
/// Premiere 26.5.1 saves a fresh key (fixture G6).
const TRACK_MATTE_NONE: &str = "4294967295";

/// The matte track's persistent `Track/ID` from an active Track Matte Key's
/// static `matte`, or `None` at [`TRACK_MATTE_NONE`]. Matte 0 is not a
/// measured encoding and names no track.
fn matte_track_id(native: &NativeEffect<'_>, matte: &str) -> Result<Option<usize>> {
    if matte == TRACK_MATTE_NONE {
        return Ok(None);
    }
    matte
        .parse::<usize>()
        .ok()
        .filter(|id| *id > 0)
        .map(Some)
        .ok_or_else(|| {
            unsupported(format!(
                "{}: Matte {matte:?} names no video track",
                native.identity
            ))
        })
}

/// The static Track Matte Key values of `record`, an active key in the corpus
/// and Premiere 26.5.1 record shapes ([`TRACK_MATTE_KEY`], `Bypass` false or
/// absent): three static parameters, no private data.
fn track_matte_values<'g>(
    graph: &Graph<'_>,
    record: Record<'g>,
) -> Result<(NativeEffect<'g>, ParamValues)> {
    let native = NativeEffect::inspect(record);
    let identify = |error| unsupported(format!("{}: {}", native.identity, reason(error)));
    ensure!(
        native.enabled().map_err(identify)?,
        "{}: bypassed Track Matte Key",
        native.identity
    );
    let values = param_values(graph, record, &TRACK_MATTE_KEY).map_err(identify)?;
    Ok((native, values))
}

/// The matte track ID that each active Track Matte Key among a chain's
/// `components` names, whether or not the key converts. Premiere does not
/// draw a matte track's clip while an active key names its track, even a
/// key that this converter rejects (fixture G1b: clip D's matte hides while
/// its Reverse was unconverted), so the reader consumes the matte range of an
/// omitted clip too. Only the key's static Matte is read ([`static_matte`]):
/// a key whose Composite Using or Reverse is keyed, or whose record has
/// another shape, still names its track. A key whose Matte cannot be read,
/// and a Matte None, name no track; the placement's own read omits a key
/// that it cannot convert, with the reason.
pub(super) fn claimed_matte_track_ids(
    graph: &Graph<'_>,
    components: &[Reference],
    chain: &str,
    omissions: &mut Vec<Omission>,
) -> Vec<usize> {
    components
        .iter()
        .filter_map(|reference| {
            let record = graph.locate(reference, chain).ok()?;
            let native = NativeEffect::inspect(record);
            if !native.is_active_track_matte_key() {
                return None;
            }
            let matte = static_matte(graph, record)?;
            if matte == TRACK_MATTE_NONE {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    &native.identity,
                    "Matte None selects no matte track; inactive Track Matte Key omitted without consuming another track",
                );
                return None;
            }
            matte_track_id(&native, &matte).ok().flatten()
        })
        .collect()
}

/// The static Matte of the Track Matte Key `record`, its `ParameterID` 1 read
/// alone through [`param_value`], so that the key's other parameters and its
/// record shape do not decide which track it consumes; `None` when that
/// parameter is missing or not one static value.
fn static_matte(graph: &Graph<'_>, record: Record<'_>) -> Option<String> {
    let filter = graph.decode::<VideoFilterComponent>(record).ok()?;
    let params = filter
        .value
        .component
        .and_then(|body| body.params)
        .map_or_else(Vec::new, |params| params.items);
    params.iter().find_map(|reference| {
        let param_record = graph.locate(reference, &filter.identity).ok()?;
        let param = graph.decode::<VideoComponentParam>(param_record).ok()?;
        if param.value.parameter_id != TRACK_MATTE_KEY_MATTE.id.to_string() {
            return None;
        }
        match param_value(
            &param.value,
            param_record.element(),
            &TRACK_MATTE_KEY_MATTE,
            &param.identity,
            false,
            false,
        )
        .ok()?
        {
            ParamValue::Static(matte) => Some(matte),
            ParamValue::Keyed(_) => None,
        }
    })
}

/// Read an active Track Matte Key ([`track_matte_values`]). Reverse with
/// Matte Alpha is one minus the matte's alpha (fixture G3a); Reverse with
/// Matte Luma fails closed ([`PrMatteChannel`]). A statically unselected matte
/// keeps the unkeyed placement; Composite Using and Reverse cannot affect it.
pub(super) fn read_track_matte(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
) -> Result<Option<TrackMatteKey>> {
    let record = graph.locate(reference, from)?;
    if static_matte(graph, record).as_deref() == Some(TRACK_MATTE_NONE) {
        return Ok(None);
    }
    let (native, values) = track_matte_values(graph, record)?;
    let Some(matte_track_id) = matte_track_id(&native, values.get(&TRACK_MATTE_KEY_MATTE)?)? else {
        return Ok(None);
    };
    let reverse = match values.get(&TRACK_MATTE_KEY_REVERSE)? {
        "false" => false,
        "true" => true,
        other => {
            return Err(unsupported(format!(
                "{}: invalid Reverse value {other:?}",
                native.identity
            )))
        }
    };
    let channel = match (values.get(&TRACK_MATTE_KEY_COMPOSITE)?, reverse) {
        ("0", false) => PrMatteChannel::Alpha,
        ("0", true) => PrMatteChannel::AlphaInverted,
        ("1", false) => PrMatteChannel::Luma,
        ("1", true) => {
            return Err(unsupported(format!(
                "{}: Reverse with Matte Luma is not converted; Premiere gives the matte clip's zero-luma exterior full coverage where FX's inverted luma matte gives none",
                native.identity
            )))
        }
        (other, _) => {
            return Err(unsupported(format!(
                "{}: Composite Using {other:?} is neither Matte Alpha (0) nor Matte Luma (1)",
                native.identity
            )))
        }
    };
    Ok(Some(TrackMatteKey {
        matte_track_id,
        channel,
    }))
}

/// One effect's parameter values: the static value of each parameter without
/// keys, by native `ParameterID`, and the keys of each keyed parameter.
struct ParamValues {
    /// Valid native controls reduced to their first saved value by a surrogate.
    flattened_controls: Vec<String>,
    statics: BTreeMap<usize, String>,
    animations: Vec<PrEffectParamAnimation>,
    /// Every parameter's `StartKeyframe` as written, keyed or not.
    start_keyframes: BTreeMap<usize, String>,
}

impl ParamValues {
    /// The static value of `param`. `param_values` reads every parameter of
    /// its spec, so only a keyed parameter or one of another effect has none.
    fn get(&self, param: &EffectParamSpec) -> Result<&str> {
        self.statics
            .get(&param.id)
            .map(String::as_str)
            .ok_or_else(|| unsupported(format!("no static {} value was read", param.label)))
    }

    /// The static value of the scalar `param`: a keyed parameter starts at its
    /// first key, as a keyed Blurriness does, and a static one must be a number
    /// in its native range.
    fn scalar(&self, param: &EffectParamSpec) -> Result<f64> {
        let first_key = self
            .animations
            .iter()
            .find(|animation| animation.param.id == param.id)
            .and_then(|animation| animation.keys.scalar()?.first());
        if let Some(key) = first_key {
            return Ok(key.value);
        }
        let value = self.get(param)?;
        value
            .parse::<f64>()
            .ok()
            .filter(|number| {
                param
                    .value_range()
                    .is_some_and(|range| range.contains(number))
            })
            .ok_or_else(|| {
                unsupported(format!(
                    "{} {value:?} is not a number from {} to {}",
                    param.label, param.lower_bound, param.upper_bound
                ))
            })
    }
}

impl ParamValues {
    /// The static value of the point `param`: a keyed parameter starts at its
    /// first key, as a keyed Blurriness does (for points this is
    /// inferred), and a static one must be a finite `x:y` point.
    fn point(&self, param: &EffectParamSpec) -> Result<[f64; 2]> {
        let first_key = self
            .animations
            .iter()
            .find(|animation| animation.param.id == param.id)
            .and_then(|animation| animation.keys.point()?.first());
        if let Some(key) = first_key {
            return Ok(key.value);
        }
        let value = self.get(param)?;
        value
            .split_once(':')
            .and_then(|(x, y)| Some([x.parse::<f64>().ok()?, y.parse::<f64>().ok()?]))
            .filter(|point| point.iter().all(|coordinate| coordinate.is_finite()))
            .ok_or_else(|| unsupported(format!("{} {value:?} is not a finite point", param.label)))
    }

    /// The static colour of the colour `param`: a keyed parameter starts at
    /// its first key, and a static one must be a native ARGB value with 8-bit
    /// channels ([`PrColour::from_native`]).
    fn colour(&self, param: &EffectParamSpec) -> Result<PrColour> {
        let first_key = self
            .animations
            .iter()
            .find(|animation| animation.param.id == param.id)
            .and_then(|animation| animation.keys.colour()?.first());
        if let Some(key) = first_key {
            return Ok(key.value);
        }
        let value = self.get(param)?;
        native_colour(value).map_err(|reason| unsupported(format!("{} {reason}", param.label)))
    }
}

/// The 8-bit colour of a native colour value as written, or why it has none.
fn native_colour(value: &str) -> std::result::Result<PrColour, String> {
    let native = value
        .parse::<u64>()
        .map_err(|_| format!("{value:?} is not a native colour value"))?;
    PrColour::from_native(native)
}

/// The keys of a colour parameter: the 8-field scalar key form whose value is
/// a native colour. The times, order and count are checked as scalar keys
/// are; the values are parsed as integers, because a double cannot hold every
/// 64-bit colour exactly. Premiere interpolates a Linear
/// segment per channel; a Bezier segment's velocity has no
/// meaning on a colour value, so a Bezier key is rejected.
fn colour_keys(wire: &str, context: &str) -> Result<Vec<PrColourKeyframe>> {
    let mut colours = Vec::new();
    for item in wire.split_terminator(';') {
        let mut fields = item.split(',');
        let (Some(_), Some(value), Some(mode)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(unsupported(format!(
                "{context}: unexpected Premiere keyframe shape"
            )));
        };
        ensure!(
            mode != "5",
            "{context}: Bezier keys are not supported; Premiere's Bezier interpolation between colours is unverified"
        );
        colours.push(
            native_colour(value)
                .map_err(|reason| unsupported(format!("{context}: key {reason}")))?,
        );
    }
    let times = super::animation::scalar_keys(wire, context)?;
    ensure!(
        times.len() == colours.len(),
        "{context}: unexpected Premiere keyframe shape"
    );
    Ok(times
        .into_iter()
        .zip(colours)
        .map(|(key, value)| PrColourKeyframe {
            source_ticks: key.source_ticks,
            value,
            easing: key.easing,
        })
        .collect())
}

/// The value of one parameter.
enum ParamValue {
    Static(String),
    Keyed(PrEffectParamKeys),
}

/// Check a supported effect's native shape and return its parameter values.
/// A Mosaic converts with Sharp Colors on, whole counts and Hold keys only
/// ([`PrMosaic::ensure_convertible`]); its host's Motion does not enter.
fn read_mosaic(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &MOSAIC)?;
    let count = |param: &EffectParamSpec| {
        PrMosaic::count(param, "", values.scalar(param)?).map_err(unsupported)
    };
    let sharp_colors = match values.get(&MOSAIC_SHARP_COLORS)? {
        "true" => true,
        "false" => false,
        other => return Err(unsupported(format!("invalid Sharp Colors value {other:?}"))),
    };
    let mosaic = PrMosaic {
        horizontal: count(&MOSAIC_HORIZONTAL_BLOCKS)?,
        vertical: count(&MOSAIC_VERTICAL_BLOCKS)?,
        sharp_colors,
    };
    mosaic
        .ensure_convertible(&values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Mosaic(mosaic),
        animations: values.animations,
    })
}

/// A Replicate whose Count, static or on Hold keys, is a whole number from 2
/// to 16 ([`PrReplicate::new`]); the converter adds its host rule and reports
/// its approximation.
fn read_replicate(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &REPLICATE)?;
    let replicate = PrReplicate::new(values.scalar(&REPLICATE_COUNT)?, &values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Replicate(replicate),
        animations: values.animations,
    })
}

fn read_sharpen(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &SHARPEN)?;
    let sharpen =
        PrSharpen::new(values.scalar(&SHARPEN_AMOUNT)?, &values.animations).map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Sharpen(sharpen),
        animations: values.animations,
    })
}

fn read_lens_distortion(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &LENS_DISTORTION)?;
    for param in &LENS_DISTORTION.params[1..5] {
        ensure!(
            values.scalar(param)? == 0.0,
            "Lens Distortion {} must be zero; decentering and prism are not converted",
            param.label
        );
    }
    ensure!(
        values.get(&LENS_DISTORTION.params[5])? == "true",
        "Lens Distortion requires Fill Alpha on; opaque Fill Color is not converted"
    );
    // Validate the otherwise inactive colour without guessing an alpha encoding.
    values.colour(&LENS_DISTORTION.params[6])?;
    let curvature =
        lens_curvature(values.scalar(&LENS_CURVATURE)?, &values.animations).map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::LensDistortion(curvature),
        animations: values.animations,
    })
}

fn read_noise(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &NOISE)?;
    ensure!(
        matches!(values.get(&NOISE_COLOR)?, "true" | "false"),
        "invalid Noise Type boolean"
    );
    ensure!(
        matches!(values.get(&NOISE_CLIPPING)?, "true" | "false"),
        "invalid Clipping boolean"
    );
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Noise {
            amount: values.scalar(&NOISE_AMOUNT)?,
        },
        animations: values.animations,
    })
}

fn read_modern_noise(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let spec = &crate::schema::MODERN_NOISE;
    let values = param_values(graph, record, spec)?;
    // Validate even discarded controls: approximation is not malformed-input recovery.
    for param in spec.params {
        if param.value_range().is_some() {
            let value = values.scalar(param)?;
            ensure!(
                !param.discontinuous_interpolate || value.fract() == 0.0,
                "invalid Noise {} ordinal",
                param.label
            );
        } else {
            ensure!(
                matches!(values.get(param)?, "true" | "false"),
                "invalid Noise {} boolean",
                param.label
            );
        }
    }
    Ok(PrEffect {
        mask: None,
        enabled: native.enabled()?,
        params: PrEffectParams::ModernNoise {
            amount: values.scalar(&spec.params[4])?,
            seed: values.scalar(&spec.params[3])?,
        },
        animations: values.animations,
    })
}

/// A Posterize converts with a whole Level and Hold Level keys only
/// ([`PrPosterize::new`]); the converter reports each as an approximation
/// ([`PrPosterize::QUANTIZER_APPROXIMATION`]).
fn read_posterize(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &POSTERIZE)?;
    let posterize = PrPosterize::new(values.scalar(&POSTERIZE_LEVEL)?, &values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Posterize(posterize),
        animations: values.animations,
    })
}

/// Keep the initial native rate and its keys for contextual loss reporting,
/// never the UI cache. FX cannot animate this parameter.
fn read_posterize_time(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &POSTERIZE_TIME)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::PosterizeTime {
            frame_rate: values.scalar(&POSTERIZE_TIME_FRAME_RATE)?,
        },
        animations: values.animations,
    })
}

/// A Transform whose skew keys [`PrTransform::ensure_convertible`] accepts;
/// the converter reports its approximated parameters
/// ([`PrTransform::approximations`]). A bypassed Transform is omitted:
/// Premiere renders the clip without it, and the stage group's video
/// carries an active one only. The converter
/// adds the host rules (a media clip whose source frame is the canvas,
/// without a Crop or Linear Wipe).
fn read_transform(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    ensure!(
        native.enabled()?,
        "a bypassed Transform is not converted: Premiere renders the clip without it, and only an active Transform becomes the staged video's transform"
    );
    let spec = if native.match_name == Some(GEOMETRY2.match_name) {
        &GEOMETRY2
    } else {
        &TRANSFORM
    };
    let values = param_values(graph, record, spec)?;
    let transform = transform_params(&values)?;
    transform
        .ensure_convertible(&values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Transform(transform),
        animations: values.animations,
    })
}

/// The Transform values of `values`, a Transform's or a Geometry2's
/// ([`param_values`]); a keyed parameter takes its first key.
fn transform_params(values: &ParamValues) -> Result<PrTransform> {
    let checkbox = |param: &EffectParamSpec| match values.get(param)? {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(unsupported(format!(
            "invalid {} value {other:?}",
            param.label
        ))),
    };
    let bicubic_sampling = match values.get(&TRANSFORM_SAMPLING)? {
        TRANSFORM_SAMPLING_BILINEAR => false,
        TRANSFORM_SAMPLING_BICUBIC => true,
        other => return Err(unsupported(format!("invalid Sampling value {other:?}"))),
    };
    Ok(PrTransform {
        anchor_point: values.point(&TRANSFORM_ANCHOR_POINT)?,
        position: values.point(&TRANSFORM_POSITION)?,
        uniform_scale: checkbox(&TRANSFORM_UNIFORM_SCALE)?,
        scale_height: values.scalar(&TRANSFORM_SCALE_HEIGHT)?,
        scale_width: values.scalar(&TRANSFORM_SCALE_WIDTH)?,
        skew: values.scalar(&TRANSFORM_SKEW)?,
        skew_axis: values.scalar(&TRANSFORM_SKEW_AXIS)?,
        rotation: values.scalar(&TRANSFORM_ROTATION)?,
        opacity: values.scalar(&TRANSFORM_OPACITY)?,
        composition_shutter_angle: checkbox(&TRANSFORM_COMPOSITION_SHUTTER_ANGLE)?,
        shutter_angle: values.scalar(&TRANSFORM_SHUTTER_ANGLE)?,
        bicubic_sampling,
    })
}

/// The least Opacity of a Transform at any time: its static value, or the
/// least value that its keys reach ([`scalar_range`]; a key's easing is that
/// of the interval ending at it).
fn lowest_opacity(values: &ParamValues) -> Result<f64> {
    let keys = values
        .animations
        .iter()
        .find(|animation| animation.param.id == TRANSFORM_OPACITY.id);
    let Some(animation) = keys else {
        return values.scalar(&TRANSFORM_OPACITY);
    };
    let keys = animation
        .keys
        .scalar()
        .ok_or_else(|| unsupported("Opacity keys are not scalar"))?;
    let values: Vec<_> = keys
        .iter()
        .map(|key| (key.value, key.easing.bezier()))
        .collect();
    Ok(scalar_range(&values)[0])
}

/// [`read_geometry2`] on a clip whose source frame is rotated or is not the
/// sequence frame: only the centered zoom, which lands the same whichever
/// frame Premiere normalizes it to.
fn read_centred_geometry2(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let effect = read_transform(graph, record, native)?;
    let PrEffectParams::Transform(transform) = effect.params else {
        return Err(unsupported("Geometry2 has no Transform parameters"));
    };
    ensure!(
        transform.anchor_point == [0.5; 2] && transform.position == [0.5; 2],
        "an off-centre Geometry2 zoom on media that is rotated or not sequence-sized is not converted: the frame that Premiere normalizes its Anchor Point and Position to is unmeasured there"
    );
    geometry2_zoom(effect, transform)
}

/// The native Geometry2 positive uniform zoom, represented by an affine
/// Corner Pin. As the Transform effect does, the zoom lands
/// the Anchor Point on the Position, both normalized to the clip frame, so
/// each frame corner `c` lands at `Position + Scale / 100 × (c − Anchor)`.
/// Each corner is linear in Scale Height, so its keys keep the authored
/// temporal easing exactly. Other Geometry2 forms remain unmeasured.
fn read_geometry2(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let effect = read_transform(graph, record, native)?;
    let PrEffectParams::Transform(transform) = effect.params else {
        return Err(unsupported("Geometry2 has no Transform parameters"));
    };
    geometry2_zoom(effect, transform)
}

/// Keep the authored adjustment controls before composite-stage admission.
/// Curved paths retain their editable tangents, with the shared clock diagnostic.
fn read_adjustment_geometry2(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
    approximations: &mut Vec<String>,
) -> Result<PrEffect> {
    ensure!(
        native.enabled()?,
        "bypassed adjustment Geometry2 is not staged"
    );
    let values = param_values_with_notes(
        graph,
        record,
        &crate::schema::ADJUSTMENT_GEOMETRY2,
        true,
        approximations,
    )?;
    let transform = transform_params(&values)?;
    transform
        .ensure_convertible(&values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::AdjustmentGeometry2(transform),
        animations: values.animations,
    })
}

fn geometry2_zoom(effect: PrEffect, transform: PrTransform) -> Result<PrEffect> {
    ensure!(
        transform.uniform_scale
            && transform.scale_height > 0.0
            && transform.skew == 0.0
            && transform.skew_axis == 0.0
            && transform.rotation == 0.0
            && transform.opacity == 100.0
            && transform.composition_shutter_angle
            && transform.shutter_angle == 0.0
            && !transform.bicubic_sampling
            && effect.animations.iter().all(|animation| animation.param.id == TRANSFORM_SCALE_HEIGHT.id),
        "Geometry2 converts only a positive uniform scale with static Anchor Point and Position, full Opacity, no Skew, Rotation or motion blur and bilinear Sampling"
    );
    let ([anchor_x, anchor_y], [position_x, position_y]) =
        (transform.anchor_point, transform.position);
    let corners = |scale: f64| {
        let factor = scale / 100.0;
        let corner = |x: f64, y: f64| {
            [
                position_x + factor * (x - anchor_x),
                position_y + factor * (y - anchor_y),
            ]
        };
        [
            corner(0.0, 0.0),
            corner(1.0, 0.0),
            corner(0.0, 1.0),
            corner(1.0, 1.0),
        ]
    };
    let mut animations = Vec::new();
    for animation in &effect.animations {
        let keys = animation
            .keys
            .scalar()
            .ok_or_else(|| unsupported("Geometry2 Scale Height has no scalar keys"))?;
        ensure!(
            keys.iter().all(|key| key.value > 0.0),
            "Geometry2 Scale Height keys must be positive"
        );
        for (index, param) in CORNER_PIN.params.iter().enumerate() {
            animations.push(PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Point(
                    keys.iter()
                        .map(|key| PrPointKeyframe {
                            source_ticks: key.source_ticks,
                            value: corners(key.value)[index],
                            easing: key.easing,
                            spatial_in_tangent: None,
                            spatial_out_tangent: None,
                        })
                        .collect(),
                ),
            });
        }
    }
    let pin = PrCornerPin {
        corners: corners(transform.scale_height),
    };
    pin.ensure_convex(&animations).map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled: effect.enabled,
        params: PrEffectParams::CornerPin(pin),
        animations,
    })
}

fn param_values(graph: &Graph<'_>, record: Record<'_>, spec: &EffectSpec) -> Result<ParamValues> {
    param_values_with(graph, record, spec, false)
}

/// [`param_values`], keeping curved spatial paths of point keys when
/// `keep_curves` holds ([`param_value`]).
fn param_values_with(
    graph: &Graph<'_>,
    record: Record<'_>,
    spec: &EffectSpec,
    keep_curves: bool,
) -> Result<ParamValues> {
    param_values_with_notes(graph, record, spec, keep_curves, &mut Vec::new())
}

fn param_values_with_notes(
    graph: &Graph<'_>,
    record: Record<'_>,
    spec: &EffectSpec,
    keep_curves: bool,
    approximations: &mut Vec<String>,
) -> Result<ParamValues> {
    let element = record.element();
    let children: &[&str] = if spec.premiere_native || spec.opaque_private_data {
        &[
            "Component",
            "PremiereFilterPrivateData",
            "MatchName",
            "VideoFilterType",
            "SubComponents",
        ]
    } else {
        &["Component", "MatchName", "VideoFilterType", "SubComponents"]
    };
    only_children(element, children, "")?;
    let component = element
        .child(COMPONENT)
        .ok_or_else(|| unsupported("missing Component"))?;
    let component_children: &[&str] = if spec.match_name == LENS_DISTORTION.match_name {
        &["Node", "Params", "ID", "DisplayName", "Bypass"]
    } else if spec.premiere_native {
        &["Node", "Params", "ID", "DisplayName"]
    } else {
        &[
            "Node",
            "Params",
            "ID",
            "DisplayName",
            "Bypass",
            "Intrinsic",
            "ArchivedType",
            "Unique",
        ]
    };
    only_children(component, component_children, "Component/")?;
    if let Some(node) = component.child("Node") {
        only_children(node, &["Properties"], "Component/Node/")?;
        if let Some(properties) = node.child("Properties") {
            only_children(properties, &UI_PROPERTIES, "Component/Node/Properties/")?;
        }
    }
    // `Unique` occurs once in the corpus, `false`, on a component saved by
    // Premiere 25.5 (`practice_files_transcription_magic`). Its meaning is
    // unknown, so any other value omits the effect.
    for (field, accepted) in [
        ("Intrinsic", "false"),
        ("ArchivedType", "0"),
        ("Unique", "false"),
    ] {
        let value = component.child(field).and_then(Element::text);
        ensure!(
            value.is_none_or(|value| value == accepted),
            "unsupported {field} {value:?}"
        );
    }
    let filter = graph.decode::<VideoFilterComponent>(record)?;
    ensure!(
        filter.value.video_filter_type.as_deref() == Some(spec.filter_type),
        "unsupported VideoFilterType {:?}",
        filter.value.video_filter_type
    );
    let params = filter
        .value
        .component
        .and_then(|body| body.params)
        .map_or_else(Vec::new, |params| params.items);
    ensure!(
        params.len() == spec.params.len(),
        "expected {} parameters, found {}",
        spec.params.len(),
        params.len()
    );
    let mut values = ParamValues {
        flattened_controls: Vec::new(),
        statics: BTreeMap::new(),
        animations: Vec::new(),
        start_keyframes: BTreeMap::new(),
    };
    let mut ids = BTreeSet::new();
    for reference in &params {
        let param_record = graph.locate(reference, &filter.identity)?;
        only_children(param_record.element(), &PARAM_CHILDREN, "parameter ")?;
        let param = graph.decode::<VideoComponentParam>(param_record)?;
        let param_spec = if spec.match_name == LENS_DISTORTION.match_name {
            ensure!(
                param.value.parameter_id == PREMIERE_NATIVE_PARAMETER_ID,
                "Lens Distortion requires ParameterID -1"
            );
            let index = reference
                .index
                .as_deref()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| unsupported("Lens Distortion requires parameter Index"))?;
            spec.params
                .get(index)
                .ok_or_else(|| unsupported("Lens Distortion parameter Index outside 0 to 6"))?
        } else if spec.premiere_native {
            ensure!(
                param.value.parameter_id == PREMIERE_NATIVE_PARAMETER_ID,
                "{}: ParameterID {} is not {PREMIERE_NATIVE_PARAMETER_ID}",
                param.identity,
                param.value.parameter_id
            );
            let name = param.value.name.as_deref();
            spec.params
                .iter()
                .find(|param_spec| name == Some(param_spec.name))
                .ok_or_else(|| {
                    unsupported(format!(
                        "{}: unknown parameter {:?}",
                        param.identity,
                        name.unwrap_or("<no Name>")
                    ))
                })?
        } else {
            spec.params
                .iter()
                .find(|param_spec| param_spec.id.to_string() == param.value.parameter_id)
                .ok_or_else(|| {
                    unsupported(format!(
                        "{}: unknown ParameterID {}",
                        param.identity, param.value.parameter_id
                    ))
                })?
        };
        if spec.match_name == crate::schema::MODERN_NOISE.match_name {
            ensure!(
                param.value.class_id.as_deref() == Some(param_spec.record.class_id),
                "unsupported modern Noise parameter class"
            );
        }
        if spec.match_name == FIND_EDGES.match_name {
            ensure!(
                param.value.class_id.as_deref() == Some(param_spec.record.class_id)
                    && param.value.lower_bound.as_deref().unwrap_or("") == param_spec.lower_bound
                    && param.value.upper_bound.as_deref().unwrap_or("") == param_spec.upper_bound
                    && param.value.parameter_control_type.as_deref().unwrap_or("")
                        == param_spec.control
                    && param.value.lower_ui_bound.as_deref() == param_spec.lower_ui_bound
                    && param.value.upper_ui_bound.as_deref() == param_spec.upper_ui_bound
                    && param.value.bypass.is_none()
                    && param.value.discontinuous_interpolate.is_none(),
                "{}: unsupported Find Edges {} parameter layout",
                param.identity,
                param_spec.label
            );
        }
        if spec.match_name == POSTERIZE_TIME.match_name {
            ensure!(
                param.value.class_id.as_deref() == Some(param_spec.record.class_id)
                    && param.value.discontinuous_interpolate.as_deref() == Some("true")
                    && param.value.lower_bound.as_deref() == Some(param_spec.lower_bound)
                    && param.value.upper_bound.as_deref() == Some(param_spec.upper_bound)
                    && param.value.lower_ui_bound.as_deref() == param_spec.lower_ui_bound
                    && param.value.upper_ui_bound.as_deref() == param_spec.upper_ui_bound
                    && param.value.parameter_control_type.is_none(),
                "unsupported Posterize Time Frame Rate layout"
            );
        }
        if spec.match_name == "AE.ADBE Alpha Glow" {
            ensure!(
                param.value.class_id.as_deref() == Some(param_spec.record.class_id)
                    && param.value.bypass.is_none()
                    && param.value.discontinuous_interpolate.is_none(),
                "{}: unsupported Alpha Glow {} parameter layout",
                param.identity,
                param_spec.label
            );
            if param_spec.id <= 2 {
                ensure!(
                    param.value.parameter_control_type.as_deref() == Some("1")
                        && param.value.lower_bound.as_deref() == Some(param_spec.lower_bound)
                        && param.value.upper_bound.as_deref() == Some(param_spec.upper_bound),
                    "{}: unsupported Alpha Glow integer slider bounds/control",
                    param.identity
                );
            }
        }
        values
            .start_keyframes
            .insert(param_spec.id, param.value.start_keyframe.clone());
        ensure!(
            param_record.tag() == param_spec.record.tag
                && param_spec.accepts_name(param.value.name.as_deref()),
            "{}: unexpected parameter {:?} or record type",
            param.identity,
            param.value.name.as_deref().unwrap_or_default()
        );
        ensure!(
            ids.insert(param_spec.id),
            "{}: duplicate {}",
            param.identity,
            if spec.premiere_native {
                format!("parameter {:?}", param_spec.name)
            } else {
                format!("ParameterID {}", param_spec.id)
            }
        );
        if spec.match_name == "AE.ADBE Alpha Glow" && param_spec.id != 1 {
            let (value, flattened) = alpha_glow_static_control(
                &param.value,
                param_record.element(),
                param_spec,
                &param.identity,
            )?;
            values.statics.insert(param_spec.id, value);
            if flattened {
                values.flattened_controls.push(format!("Alpha Glow {} animation flattened to its first saved key; the single-color surrogate keeps this control static", param_spec.label));
            }
            continue;
        }
        // Validate Alpha size wire independently before allowing a lossy easing
        // fallback. Malformed keys must not be mistaken for unrepresentable FX.
        let validated_alpha_size = if spec.match_name == "AE.ADBE Alpha Glow"
            && param_spec.id == 1
            && param
                .value
                .keyframes
                .as_deref()
                .is_some_and(|keys| !keys.is_empty())
        {
            Some(
                alpha_glow_static_control(
                    &param.value,
                    param_record.element(),
                    param_spec,
                    &param.identity,
                )?
                .0,
            )
        } else {
            None
        };
        let parsed = param_value_with_notes(
            &param.value,
            param_record.element(),
            param_spec,
            &param.identity,
            keep_curves,
            // Geometry2 saves a time-varying Rotation marker without keys;
            // its authored static zero still renders in the measured zoom.
            spec.match_name == GEOMETRY2.match_name && param_spec.id == TRANSFORM_ROTATION.id,
            approximations,
        );
        let parsed = match (parsed, validated_alpha_size) {
            (Ok(value), _) => value,
            (Err(error), Some(first)) => {
                values.flattened_controls.push(format!(
                    "Alpha Glow size animation flattened to its first validated value {first}: {}",
                    reason(error)
                ));
                ParamValue::Static(first)
            }
            (Err(error), None) => return Err(error),
        };
        match parsed {
            ParamValue::Static(value) => {
                values.statics.insert(param_spec.id, value);
            }
            ParamValue::Keyed(keys) => values.animations.push(PrEffectParamAnimation {
                param: param_spec,
                keys,
            }),
        }
    }
    // The counts match and no ParameterID repeats, so every parameter has a
    // static value or keys.
    Ok(values)
}

pub(super) fn only_children(element: Element<'_>, allowed: &[&str], path: &str) -> Result<()> {
    match element
        .children()
        .find(|child| !allowed.contains(&child.tag()))
    {
        Some(child) => Err(unsupported(format!(
            "{path}{} is not supported",
            child.tag()
        ))),
        None => Ok(()),
    }
}

/// Validate unsupported animated controls before keeping the first saved value.
/// This is a lossy static replacement, not replay of a hidden native payload.
fn alpha_glow_static_control(
    param: &VideoComponentParam,
    element: Element<'_>,
    spec: &EffectParamSpec,
    identity: &str,
) -> Result<(String, bool)> {
    let wire = param.keyframes.as_deref().unwrap_or("");
    if wire.is_empty() {
        let ParamValue::Static(value) = param_value(param, element, spec, identity, false, false)?
        else {
            return Err(unsupported("unexpected Alpha Glow static control"));
        };
        return Ok((value, false));
    }
    ensure!(
        param.is_time_varying.as_deref().is_none_or(|v| v == "true"),
        "conflicting Alpha Glow animation flag"
    );
    ensure!(wire.ends_with(';'), "unterminated Alpha Glow keyframe list");
    let mut previous = None;
    let mut first = None;
    for key in wire.split_terminator(';') {
        let fields: Vec<_> = key.split(',').collect();
        ensure!(fields.len() == 8, "invalid Alpha Glow key shape");
        let time = fields[0]
            .parse::<i64>()
            .map_err(|_| unsupported("invalid Alpha Glow key time"))?;
        ensure!(
            previous.is_none_or(|previous| time > previous),
            "unordered Alpha Glow keys"
        );
        match spec.id {
            1 | 2 => {
                let value = fields[1]
                    .parse::<f64>()
                    .map_err(|_| unsupported("invalid Alpha Glow slider key"))?;
                let maximum = if spec.id == 1 { 100.0 } else { 255.0 };
                ensure!(
                    (0.0..=maximum).contains(&value) && value.fract() == 0.0,
                    "Alpha Glow {} key must be whole in 0..{maximum}",
                    spec.label
                );
                if spec.id == 1 {
                    ensure!(
                        matches!(fields[2], "0" | "4" | "5"),
                        "invalid Alpha Glow size interpolation mode"
                    );
                    for field in [fields[5], fields[7]] {
                        ensure!(
                            field
                                .parse::<f64>()
                                .is_ok_and(|value| (0.0..=1.0).contains(&value)),
                            "invalid Alpha Glow size influence"
                        );
                    }
                }
            }
            3 | 4 => {
                native_colour(fields[1]).map_err(unsupported)?;
            }
            5 | 6 => {
                ensure!(
                    matches!(fields[1], "true" | "false"),
                    "invalid Alpha Glow boolean key value"
                );
            }
            _ => return Err(unsupported("invalid Alpha Glow static control")),
        }
        ensure!(
            fields[2].parse::<u8>().is_ok() && fields[3].parse::<u8>().is_ok(),
            "invalid Alpha Glow key mode/flags"
        );
        ensure!(
            fields[4..]
                .iter()
                .all(|field| field.parse::<f64>().is_ok_and(f64::is_finite)),
            "invalid Alpha Glow key handles"
        );
        first.get_or_insert(fields[1]);
        previous = Some(time);
    }
    Ok((
        first
            .ok_or_else(|| unsupported("empty Alpha Glow keys"))?
            .to_owned(),
        true,
    ))
}

/// The keys of a keyed parameter that has an FX binding, or else the static
/// value of one parameter. Other keyed parameters are unsupported, and so are
/// point keys on a curved spatial path unless `keep_curves` holds.
fn param_value(
    param: &VideoComponentParam,
    element: Element<'_>,
    spec: &EffectParamSpec,
    identity: &str,
    keep_curves: bool,
    empty_keys_are_static: bool,
) -> Result<ParamValue> {
    param_value_with_notes(
        param,
        element,
        spec,
        identity,
        keep_curves,
        empty_keys_are_static,
        &mut Vec::new(),
    )
}

fn param_value_with_notes(
    param: &VideoComponentParam,
    element: Element<'_>,
    spec: &EffectParamSpec,
    identity: &str,
    keep_curves: bool,
    empty_keys_are_static: bool,
    approximations: &mut Vec<String>,
) -> Result<ParamValue> {
    let label = spec.label;
    let wire = param.keyframes.as_deref().unwrap_or("");
    let is_time_varying = param.is_time_varying.as_deref();
    // Find Edges Blend is read only to report its loss, not as an FX binding.
    if spec.binding.is_some() || spec == &FIND_EDGES_BLEND {
        // Motion's key readers and `IsTimeVarying` rules.
        ensure!(
            matches!(is_time_varying, None | Some("true") | Some("false")),
            "invalid {label} IsTimeVarying"
        );
        let context = format!("{label} ({identity})");
        let keys = match spec.binding {
            Some(
                EffectParamBinding::Scalar(_)
                | EffectParamBinding::ScaledScalar { .. }
                | EffectParamBinding::Integer { .. }
                | EffectParamBinding::TileCount { .. },
            )
            | None => {
                let keys = if [
                    crate::schema::LEGACY_LUMA_THRESHOLD,
                    crate::schema::LEGACY_LUMA_CUTOFF,
                ]
                .contains(spec)
                {
                    let (keys, linearized) =
                        super::animation::linearized_scalar_keys(wire, &context)?;
                    if linearized {
                        approximations.push(format!("{context}: Bezier keys approximated as Linear with values/times retained; bounded interpolation prevents falloff overshoot"));
                    }
                    keys
                } else {
                    super::animation::scalar_keys(wire, &context)?
                };
                (!keys.is_empty()).then_some(PrEffectParamKeys::Scalar(keys))
            }
            Some(EffectParamBinding::Point { .. }) => {
                let keys = super::animation::point_keys(wire, &context)?;
                (!keys.is_empty()).then_some(PrEffectParamKeys::Point(keys))
            }
            Some(EffectParamBinding::Colour { .. }) => {
                let keys = colour_keys(wire, &context)?;
                (!keys.is_empty()).then_some(PrEffectParamKeys::Colour(keys))
            }
        };
        if let Some(keys) = keys {
            ensure!(
                is_time_varying != Some("false"),
                "{label} keyframes conflict with disabled IsTimeVarying"
            );
            match &keys {
                PrEffectParamKeys::Scalar(keys) => {
                    let range = spec.value_range();
                    if let Some(key) = keys.iter().find(|key| {
                        !range
                            .as_ref()
                            .is_some_and(|range| range.contains(&key.value))
                    }) {
                        return Err(unsupported(format!(
                            "{label} key value {} is outside Premiere's {} to {} range",
                            key.value, spec.lower_bound, spec.upper_bound
                        )));
                    }
                }
                PrEffectParamKeys::Point(keys) if !keep_curves => {
                    if let Some([start, end]) = curved_segment(keys) {
                        return Err(unsupported(format!(
                            "{label} moves on a curved spatial path between its keys at source times {} s and {} s; only a straight path converts, because FX keys each coordinate separately",
                            seconds(start.source_ticks),
                            seconds(end.source_ticks)
                        )));
                    }
                }
                // A kept curved path converts from the key form that
                // Premiere saved only, whose flags `point_keys` does not
                // keep; its reader checks the rest.
                PrEffectParamKeys::Point(keys) => {
                    if curved_segment(keys).is_some() {
                        super::animation::ensure_saved_curve_form(wire, &context)?;
                    }
                }
                // Each colour key is an 8-bit colour by construction.
                PrEffectParamKeys::Colour(_) => {}
            }
            // A keyed parameter ignores `StartKeyframe` and the cached
            // `CurrentValue`: before a later first key, AME renders that key's value.
            return Ok(ParamValue::Keyed(keys));
        }
        ensure!(
            is_time_varying != Some("true") || empty_keys_are_static,
            "empty time-varying {label}"
        );
    } else {
        ensure!(
            wire.is_empty() && is_time_varying != Some("true"),
            "keyframed {label} is not supported; only static values convert"
        );
        ensure!(
            is_time_varying.is_none_or(|value| value == "false"),
            "invalid {label} IsTimeVarying"
        );
    }
    let mut fields = param.start_keyframe.split(',');
    let (Some(time), Some(value)) = (fields.next(), fields.next()) else {
        return Err(unsupported(format!(
            "unexpected {label} StartKeyframe shape"
        )));
    };
    // A point's `StartKeyframe` has the 14 fields of a point key.
    let point = spec.record.tag == records::POINT_COMPONENT_PARAM.tag;
    ensure!(
        time == records::STATIC_KEYFRAME_TIME && fields.count() == if point { 12 } else { 6 },
        "unexpected {label} StartKeyframe shape"
    );
    // A static parameter's cached UI value can be stale; reject a conflict
    // instead of guessing which value Premiere renders.
    if let Some(current) = element.child("CurrentValue").and_then(Element::text) {
        let same = current == value
            || matches!(
                (current.parse::<f64>(), value.parse::<f64>()),
                (Ok(current), Ok(value)) if current == value
            );
        ensure!(
            same,
            "{label} CurrentValue {current:?} conflicts with its static value {value:?}"
        );
    }
    Ok(ParamValue::Static(value.to_owned()))
}

fn read_legacy_luma(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    read_legacy_luma_with_notes(graph, record, native, &mut Vec::new())
}

fn read_legacy_luma_with_notes(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
    approximations: &mut Vec<String>,
) -> Result<PrEffect> {
    use crate::schema::{
        validate_legacy_luma, LEGACY_LUMA_CUTOFF, LEGACY_LUMA_KEY, LEGACY_LUMA_THRESHOLD,
    };
    let enabled = native.enabled()?;
    let values = param_values_with_notes(graph, record, &LEGACY_LUMA_KEY, false, approximations)?;
    let threshold = values.scalar(&LEGACY_LUMA_THRESHOLD)?;
    let cutoff = values.scalar(&LEGACY_LUMA_CUTOFF)?;
    validate_legacy_luma(threshold, cutoff, &values.animations).map_err(unsupported)?;
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::LegacyLuma { threshold, cutoff },
        animations: values.animations,
    })
}

/// Ordinary occurrences may expand static rows; interpreted-source admission
/// continues to use the stricter scalar `read_levels` unchanged.
fn read_levels_with_channels(
    graph: &Graph<'_>,
    record: Record<'_>,
    omitted: &mut Vec<String>,
) -> Result<Vec<PrEffect>> {
    let mut notes = Vec::new();
    let master = read_levels_preserving_master(graph, record, &mut notes)?;
    if notes.is_empty() || record.element().child("SubComponents").is_some() {
        omitted.extend(notes);
        return Ok(vec![master]);
    }
    let stored = private_values(graph, record)?;
    let mut rows = [[0.0; 5]; 3];
    for (channel, row) in rows.iter_mut().enumerate() {
        *row = std::array::from_fn(|i| f64::from(stored[5 + channel * 5 + i]));
        if row[0] >= row[1] || row[2] > row[3] || row[4] < 1.0 {
            omitted.push(format!("Levels channel {} row {row:?} omitted: editable branch requires ascending input/output bounds and Gamma at least1; master and other channels retained",["R","G","B"][channel]));
            *row = LEVELS_NEUTRAL;
        }
    }
    if rows == [LEVELS_NEUTRAL; 3] {
        return Ok(vec![master]);
    }
    Ok(vec![
        master,
        PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Levels(PrLevels::Corrections(rows)),
            animations: Vec::new(),
        },
    ])
}
