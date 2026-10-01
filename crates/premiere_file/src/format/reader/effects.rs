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

use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{graph::Element, Graph, Record},
    omit,
    schema::{
        chain_render_order, is_coverage_effect, mask_match_name,
        native::{Reference, VideoComponentParam, VideoFilterComponent},
        records, seconds, EffectParamBinding, EffectParamSpec, EffectSpec, PrBrightnessContrast,
        PrColour, PrColourKeyframe, PrCornerPin, PrDirectionalBlur, PrEffect,
        PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, PrFilmImpactBlur,
        PrFilmImpactDirectionalBlur, PrGaussianBlur, PrInvert, PrLevels, PrMatteChannel, PrMosaic,
        PrPointKeyframe, PrRamp, PrTint, PrTransform, BLACK_WHITE,
        BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL, BRIGHTNESS_CONTRAST,
        BRIGHTNESS_CONTRAST_BRIGHTNESS, BRIGHTNESS_CONTRAST_CONTRAST, CORNER_PIN, DIRECTIONAL_BLUR,
        DIRECTIONAL_BLUR_DIRECTION, DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_BLUR,
        FILM_IMPACT_BLUR_26_2, FILM_IMPACT_BLUR_26_2_DEFAULTS, FILM_IMPACT_BLUR_AMOUNT,
        FILM_IMPACT_BLUR_CHROMATIC, FILM_IMPACT_BLUR_CONTROLS, FILM_IMPACT_BLUR_DEFAULTS,
        FILM_IMPACT_BLUR_EDGE, FILM_IMPACT_BLUR_THICKNESS, FILM_IMPACT_BLUR_UNIFORM,
        FILM_IMPACT_DIRECTIONAL_BLUR, FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
        FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, FILM_IMPACT_DIRECTIONAL_BLUR_DEFAULTS, GAUSSIAN_BLUR,
        GAUSSIAN_BLUR_BLURRINESS, GAUSSIAN_BLUR_DIMENSIONS, GAUSSIAN_BLUR_MAX_BLURRINESS,
        GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS, INVERT, INVERT_BLEND, INVERT_CHANNEL, INVERT_CHANNEL_RGB,
        LEVELS, LEVELS_NEUTRAL, MASK_EFFECT_ORDER_REASON, MOSAIC, MOSAIC_HORIZONTAL_BLOCKS,
        MOSAIC_SHARP_COLORS, MOSAIC_VERTICAL_BLOCKS, PREMIERE_NATIVE_PARAMETER_ID, RAMP,
        RAMP_BLEND, RAMP_END, RAMP_END_COLOR, RAMP_SCATTER, RAMP_SHAPE, RAMP_SHAPE_LINEAR,
        RAMP_START, RAMP_START_COLOR, TINT, TINT_AMOUNT, TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO,
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
        if is_standard {
            standard.push(record);
        }
        if !is_standard || NativeEffect::inspect(record).is_active_mask() {
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
    pub(super) clip_name: Option<&'a str>,
    /// Native video track `Index`. Premiere labels index 0 as V1.
    pub(super) track_index: i64,
    pub(super) timeline_ticks: Range<i64>,
}

impl SplitChain<'_, '_> {
    /// Reject the occurrence when an active standard effect changes what its
    /// clip covers or its transparency. Converting the clip without that
    /// effect would leave it opaque over everything below. An active Track
    /// Matte Key is the clip's mask, which the Motion reader reads. Premiere
    /// renders a clip without its bypassed effects, so a bypassed one keeps
    /// the clip and `read_effects` reports it; a missing or invalid `Bypass`
    /// counts as active.
    pub(super) fn reject_coverage_effects(&self, graph: &Graph<'_>) -> Result<()> {
        for (position, &record) in (1..).zip(&self.standard) {
            let native = NativeEffect::inspect(record);
            if native.bypass == Some("true")
                || native.match_name == Some(TRACK_MATTE_KEY.match_name)
            {
                continue;
            }
            let reason = if is_coverage_effect(native.match_name, native.display_name) {
                "changes what the clip covers or its transparency".to_owned()
            } else if let Some(mask) = carries_mask(graph, record)? {
                format!("carries a mask ({mask} sub-component; JRB-2028)")
            } else {
                continue;
            };
            return Err(unsupported(format!(
                "{} at stack position {position} {reason}; the clip is not converted without it",
                native.identify()
            )));
        }
        Ok(())
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
                matches!(effect.params, PrEffectParams::Invert(invert) if invert.blend == 0.0)
                    && effect.animations.is_empty(),
                "{}: nondefault adjustment Motion requires static full RGB Invert",
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
            // The occurrence owns Stroke geometry; read_stroke validates and reports it once.
            if native.match_name == Some("AE.Impact_Stroke_FX") {
                continue;
            }
            let description = native.describe(position, owner);
            let read = match native.match_name {
                Some(name) if name == GAUSSIAN_BLUR.match_name => read_gaussian_blur,
                Some(name) if name == FILM_IMPACT_BLUR.match_name => read_film_impact_blur,
                Some(name) if name == CORNER_PIN.match_name => read_corner_pin,
                Some(name) if name == DIRECTIONAL_BLUR.match_name => read_directional_blur,
                Some(name) if name == FILM_IMPACT_DIRECTIONAL_BLUR.match_name => {
                    read_film_impact_directional_blur
                }
                Some(name) if name == LEVELS.match_name => read_levels,
                Some(name) if name == BRIGHTNESS_CONTRAST.match_name => read_brightness_contrast,
                Some(name) if name == INVERT.match_name => read_invert,
                Some(name) if name == TINT.match_name => read_tint,
                Some(name) if name == BLACK_WHITE.match_name => read_black_white,
                Some(name) if name == RAMP.match_name => read_ramp,
                Some(name) if name == MOSAIC.match_name => read_mosaic,
                Some(name) if name == TRANSFORM.match_name => read_transform,
                Some(name) if name == GEOMETRY2.match_name => read_geometry2,
                _ => {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        &native.identity,
                        format!("unknown {description}: no Tesseract effect mapping"),
                    );
                    continue;
                }
            };
            let converted = if native.is_active_transform() && has_matte && !measured_pair {
                Err(unsupported("Transform with Track Matte Key requires exactly those two active native effects; another active or unconverted effect is outside measured A4"))
            } else if self.ordered {
                read(graph, record, &native)
            } else {
                Err(unsupported(
                    "component Index values disagree with their order, so the stack order is ambiguous",
                ))
            };
            match converted {
                Ok(effect) => {
                    effects.push(effect);
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
fn reason(error: BuildError) -> String {
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
        format!(
            "{} at stack position {position} on clip {:?} ({}, V{}, {} s to {} s)",
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
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &CORNER_PIN)?;
    let mut corners = [[0.0; 2]; 4];
    for (corner, param) in corners.iter_mut().zip(CORNER_PIN.params) {
        *corner = values.point(param)?;
    }
    let pin = PrCornerPin { corners };
    pin.ensure_convex(&values.animations).map_err(unsupported)?;
    Ok(PrEffect {
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
        enabled,
        params: PrEffectParams::DirectionalBlur(blur),
        animations: values.animations,
    })
}

/// A Levels whose private data repeats its `StartKeyframe` values, whose
/// (R), (G) and (B) rows are neutral and unkeyed, and whose master is a form
/// that an Adobe render measured. `param_values` rejects `Bypass` and
/// `Intrinsic`, so the effect is active.
fn read_levels(
    graph: &Graph<'_>,
    record: Record<'_>,
    _native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let values = param_values(graph, record, &LEVELS)?;
    let stored = private_values(graph, record)?;
    let mut rgb = [0.0; 5];
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
            let neutral = LEVELS_NEUTRAL[index % 5];
            ensure!(
                value == neutral,
                "{} {value} is not neutral ({neutral}); FX levels has only the master (RGB) row",
                param.label
            );
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
    let levels = PrLevels { rgb };
    levels
        .ensure_rendered_form(&values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        enabled: true,
        params: PrEffectParams::Levels(levels),
        animations: values.animations,
    })
}

/// The little-endian u16 values of a Levels record's
/// `PremiereFilterPrivateData`, one per parameter (Oracle run E4). Premiere
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
        enabled,
        params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness: values.scalar(&BRIGHTNESS_CONTRAST_BRIGHTNESS)?,
            contrast: values.scalar(&BRIGHTNESS_CONTRAST_CONTRAST)?,
        }),
        animations: values.animations,
    })
}

/// An Invert of every channel (Channel 0, RGB), the only Channel that FX
/// `levels` expresses; another or a keyed Channel omits the effect. Its
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
        channel == INVERT_CHANNEL_RGB,
        "Channel {channel:?} is not RGB ({INVERT_CHANNEL_RGB}); FX levels inverts every channel"
    );
    Ok(PrEffect {
        enabled,
        params: PrEffectParams::Invert(PrInvert {
            blend: values.scalar(&INVERT_BLEND)?,
        }),
        animations: values.animations,
    })
}

fn read_tint(graph: &Graph<'_>, record: Record<'_>, native: &NativeEffect<'_>) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let values = param_values(graph, record, &TINT)?;
    Ok(PrEffect {
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
        "Ramp Shape {shape:?} is not linear ({RAMP_SHAPE_LINEAR}); a radial ramp is not converted, because Premiere measures its radius in clip pixels and the FX gradientRamp in frame UV (Oracle run E10 probe)"
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
/// Matte Luma fails closed ([`PrMatteChannel`]).
pub(super) fn read_track_matte(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
) -> Result<TrackMatteKey> {
    let record = graph.locate(reference, from)?;
    let (native, values) = track_matte_values(graph, record)?;
    let matte_track_id =
        matte_track_id(&native, values.get(&TRACK_MATTE_KEY_MATTE)?)?.ok_or_else(|| {
            unsupported(format!(
                "{}: Matte None selects no matte track; a key without a matte is not converted",
                native.identity
            ))
        })?;
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
    Ok(TrackMatteKey {
        matte_track_id,
        channel,
    })
}

/// One effect's parameter values: the static value of each parameter without
/// keys, by native `ParameterID`, and the keys of each keyed parameter.
struct ParamValues {
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
    /// first key, as a keyed Blurriness does (Oracle run D; for points this is
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
/// 64-bit colour exactly (Oracle run E6). Premiere interpolates a Linear
/// segment per channel (E6, Tint clip E); a Bezier segment's velocity has no
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
        enabled,
        params: PrEffectParams::Mosaic(mosaic),
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
        "a bypassed Transform is not converted: Premiere renders the clip without it, and only an active Transform becomes the staged video's transform (supervisor decision D22-5)"
    );
    let spec = if native.match_name == Some(GEOMETRY2.match_name) {
        &GEOMETRY2
    } else {
        &TRANSFORM
    };
    let values = param_values(graph, record, spec)?;
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
    let transform = PrTransform {
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
    };
    transform
        .ensure_convertible(&values.animations)
        .map_err(unsupported)?;
    Ok(PrEffect {
        enabled: true,
        params: PrEffectParams::Transform(transform),
        animations: values.animations,
    })
}

/// The native Geometry2 centered uniform zoom, represented by an affine
/// Corner Pin. Each corner is linear in Scale Height, so its keys keep the
/// authored temporal easing exactly. Other Geometry2 forms remain unmeasured.
fn read_geometry2(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let effect = read_transform(graph, record, native)?;
    let PrEffectParams::Transform(transform) = effect.params else {
        return Err(unsupported("Geometry2 has no Transform parameters"));
    };
    ensure!(
        transform.anchor_point == [0.5; 2]
            && transform.position == [0.5; 2]
            && transform.uniform_scale
            && transform.scale_height > 0.0
            && transform.skew == 0.0
            && transform.skew_axis == 0.0
            && transform.rotation == 0.0
            && transform.opacity == 100.0
            && transform.composition_shutter_angle
            && transform.shutter_angle == 0.0
            && !transform.bicubic_sampling
            && effect.animations.iter().all(|animation| animation.param.id == TRANSFORM_SCALE_HEIGHT.id),
        "Geometry2 converts only a centered positive uniform scale with full Opacity, no Skew, Rotation or motion blur and bilinear Sampling"
    );
    let corners = |scale: f64| {
        let inset = (1.0 - scale / 100.0) / 2.0;
        [
            [inset, inset],
            [1.0 - inset, inset],
            [inset, 1.0 - inset],
            [1.0 - inset; 2],
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
        enabled: effect.enabled,
        params: PrEffectParams::CornerPin(pin),
        animations,
    })
}

fn param_values(graph: &Graph<'_>, record: Record<'_>, spec: &EffectSpec) -> Result<ParamValues> {
    let element = record.element();
    let children: &[&str] = if spec.premiere_native || spec.opaque_private_data {
        &[
            "Component",
            "PremiereFilterPrivateData",
            "MatchName",
            "VideoFilterType",
        ]
    } else {
        &["Component", "MatchName", "VideoFilterType"]
    };
    only_children(element, children, "")?;
    let component = element
        .child(COMPONENT)
        .ok_or_else(|| unsupported("missing Component"))?;
    let component_children: &[&str] = if spec.premiere_native {
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
        statics: BTreeMap::new(),
        animations: Vec::new(),
        start_keyframes: BTreeMap::new(),
    };
    let mut ids = BTreeSet::new();
    for reference in &params {
        let param_record = graph.locate(reference, &filter.identity)?;
        only_children(param_record.element(), &PARAM_CHILDREN, "parameter ")?;
        let param = graph.decode::<VideoComponentParam>(param_record)?;
        let param_spec = if spec.premiere_native {
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
        match param_value(
            &param.value,
            param_record.element(),
            param_spec,
            &param.identity,
            // Geometry2 saves a time-varying Rotation marker without keys;
            // its authored static zero still renders in the measured zoom.
            spec.match_name == GEOMETRY2.match_name && param_spec.id == TRANSFORM_ROTATION.id,
        )? {
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

/// Largest distance, in units of the clip frame, at which a spatial control
/// point still lies on its straight segment. Premiere writes float noise such
/// as 1.4e-17 into the automatic tangents of the straight corpus paths.
const STRAIGHT_PATH_TOLERANCE: f64 = 1e-9;

/// The first two consecutive keys whose spatial path is curved: a control
/// point (a key plus its tangent) lies farther than
/// [`STRAIGHT_PATH_TOLERANCE`] from the segment between them. On a straight
/// path a point moves by its temporal easing alone.
fn curved_segment(keys: &[PrPointKeyframe]) -> Option<[&PrPointKeyframe; 2]> {
    keys.windows(2).find_map(|pair| {
        let [start, end] = [&pair[0], &pair[1]];
        let curved = [
            (start, start.spatial_out_tangent),
            (end, end.spatial_in_tangent),
        ]
        .into_iter()
        .any(|(key, tangent)| {
            tangent.is_some_and(|tangent| {
                let control = [key.value[0] + tangent[0], key.value[1] + tangent[1]];
                !on_segment(control, start.value, end.value)
            })
        });
        curved.then_some([start, end])
    })
}

/// Whether `point` lies within [`STRAIGHT_PATH_TOLERANCE`] of the closed
/// segment from `start` to `end`.
fn on_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> bool {
    let chord = [end[0] - start[0], end[1] - start[1]];
    let offset = [point[0] - start[0], point[1] - start[1]];
    let length_squared = chord[0] * chord[0] + chord[1] * chord[1];
    let along = if length_squared > 0.0 {
        ((offset[0] * chord[0] + offset[1] * chord[1]) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (offset[0] - along * chord[0]).hypot(offset[1] - along * chord[1]) <= STRAIGHT_PATH_TOLERANCE
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

/// The keys of a keyed parameter that has an FX binding, or else the static
/// value of one parameter. Other keyed parameters are unsupported.
fn param_value(
    param: &VideoComponentParam,
    element: Element<'_>,
    spec: &EffectParamSpec,
    identity: &str,
    empty_keys_are_static: bool,
) -> Result<ParamValue> {
    let label = spec.label;
    let wire = param.keyframes.as_deref().unwrap_or("");
    let is_time_varying = param.is_time_varying.as_deref();
    if let Some(binding) = spec.binding {
        // Motion's key readers and `IsTimeVarying` rules.
        ensure!(
            matches!(is_time_varying, None | Some("true") | Some("false")),
            "invalid {label} IsTimeVarying"
        );
        let context = format!("{label} ({identity})");
        let keys = match binding {
            EffectParamBinding::Scalar(_)
            | EffectParamBinding::ScaledScalar { .. }
            | EffectParamBinding::Integer { .. } => {
                let keys = super::animation::scalar_keys(wire, &context)?;
                (!keys.is_empty()).then_some(PrEffectParamKeys::Scalar(keys))
            }
            EffectParamBinding::Point { .. } => {
                let keys = super::animation::point_keys(wire, &context)?;
                (!keys.is_empty()).then_some(PrEffectParamKeys::Point(keys))
            }
            EffectParamBinding::Colour { .. } => {
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
                PrEffectParamKeys::Point(keys) => {
                    if let Some([start, end]) = curved_segment(keys) {
                        return Err(unsupported(format!(
                            "{label} moves on a curved spatial path between its keys at source times {} s and {} s; only a straight path converts, because FX keys each coordinate separately",
                            seconds(start.source_ticks),
                            seconds(end.source_ticks)
                        )));
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
