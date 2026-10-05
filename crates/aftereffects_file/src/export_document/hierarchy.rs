//! Strict native Null-parent and finite-canvas precomposition lowering.
//!
//! This helper classifies current FX Groups only. Registration and dispatch
//! remain in the shared export module so sibling-local rollback stays central.

mod animated_bounds;
mod blur_bounds;
#[cfg(test)]
mod blur_bounds_tests;
mod collapsed;
#[cfg(test)]
mod collapsed_tests;
mod demand;
mod effect_support;
mod root_viewport;
pub(super) mod skew;

#[cfg(test)]
mod demand_tests;

#[cfg(test)]
mod skew_tests;

#[cfg(test)]
mod nested_viewport_tests;
#[cfg(test)]
mod root_viewport_tests;

use std::collections::BTreeMap;

use fx_schema::{
    GroupLayer, Layer, LayerData, LayerId, Position, ShapeLineJoin, Time, Transform,
    layer::ShapePathCommand,
};

use crate::{
    timing::Duration24,
    writer::{
        CompositionOptions, LayerSpec, NativeLayerOptions, NullLayerSpec, PrecompositionSpec,
        SolidTransform, TransformAnimations,
    },
};

use super::media;

pub(super) use demand::Demand;

#[derive(Clone, Copy)]
pub(super) struct SourceInterval {
    range: fx_schema::TimeRangeProperty,
    clock: crate::writer::PropertyClock,
}

impl SourceInterval {
    pub(super) fn new(
        range: fx_schema::TimeRangeProperty,
        rate: crate::timing::FrameRate,
    ) -> Result<Self, crate::rifx::RifxError> {
        Ok(Self {
            range,
            clock: crate::writer::PropertyClock::for_rate(rate)?,
        })
    }
}

/// A bounded output canvas, never permission to crop child effect inputs.
/// Consumer viewports are a diagnosed approximation for otherwise oversized 3D sources.
pub(super) struct CertifiedCanvas {
    bounds: Bounds,
    root_output: bool,
    consumer_3d: bool,
    /// Declared native input domain, following all-child support validation.
    source_mask: bool,
    /// Input support bounds intersect validated content instead of replacing it.
    intersect_content: bool,
}

pub(super) struct ChildDemand {
    propagated: Demand,
    finite_canvas: Option<CertifiedCanvas>,
}

impl ChildDemand {
    /// A native hard mask or supported logical effect plane bounds source input
    /// independently of any unknown glyph extent or occurrence projection.
    pub(super) fn use_source_mask_viewport(&mut self, bounds: Bounds, duration_millis: u64) {
        self.propagated = Demand::root(bounds, duration_millis);
        self.finite_canvas = Some(CertifiedCanvas {
            bounds,
            root_output: false,
            consumer_3d: false,
            source_mask: true,
            intersect_content: false,
        });
    }

    pub(super) fn use_root_output_viewport(
        &mut self,
        group: &GroupLayer,
        dynamics: &crate::export_document::AnimationIndex<'_>,
        siblings: &[Layer],
        canvas: fx_schema::Dimensions,
    ) -> bool {
        if let Some(viewport) = root_viewport::canvas(group, dynamics, siblings, canvas) {
            self.finite_canvas = Some(viewport);
            return true;
        }
        false
    }

    /// An identity 2D source can share the final output plane only when its
    /// consumers have already proved that exact plane as their entire demand.
    /// Do not substitute the root canvas for an unknown or transformed domain.
    pub(super) fn use_nested_output_viewport(
        &mut self,
        group: &GroupLayer,
        dynamics: &crate::export_document::AnimationIndex<'_>,
        siblings: &[Layer],
        canvas: fx_schema::Dimensions,
    ) -> bool {
        if !demand_clock_is_unit(&group.playback)
            || subtree_needs_projection(&group.layers, dynamics)
        {
            return false;
        }
        let Some(viewport) = root_viewport::nested_canvas(group, dynamics, siblings, canvas) else {
            return false;
        };
        let Ok(demand) = self.propagated.finite_union() else {
            return false;
        };
        if demand.min != viewport.bounds.min || demand.max != viewport.bounds.max {
            return false;
        }
        self.finite_canvas = Some(viewport);
        true
    }

    /// The source has already inverted its identity occurrence and expanded the
    /// consumer demand by every supported effect kernel. Preserve finite content
    /// smaller than that demand rather than replacing it with a nominal canvas.
    pub(super) fn use_nested_input_viewport(
        &mut self,
        group: &GroupLayer,
        dynamics: &crate::export_document::AnimationIndex<'_>,
        siblings: &[Layer],
        canvas: fx_schema::Dimensions,
    ) -> bool {
        if !demand_clock_is_unit(&group.playback)
            || subtree_needs_projection(&group.layers, dynamics)
        {
            return false;
        }
        let Some(mut certificate) = root_viewport::input_canvas(group, dynamics, siblings, canvas)
        else {
            return false;
        };
        let Ok(bounds) = self.propagated.finite_union() else {
            return false;
        };
        certificate.bounds = bounds;
        self.finite_canvas = Some(certificate);
        true
    }

    /// Certify finite geometric output support, not pixel-exact native rasterization.
    pub(super) fn use_3d_consumer_viewport(
        &mut self,
        group: &GroupLayer,
        siblings: &[Layer],
        dynamics: &crate::export_document::AnimationIndex<'_>,
    ) -> Result<(), &'static str> {
        if group.motion_blur
            || !group.masks.is_empty()
            || group.track_matte.is_some()
            || !group.fills.is_empty()
            || !demand_clock_is_unit(&group.playback)
            || effect_support::stack(&group.effects).is_err()
        {
            return Err("3D consumer has unsupported masks, clock, blur or owner controls");
        }
        if group
            .layers
            .iter()
            .any(|child| child.parent_id().is_some_and(|parent| parent != group.id))
        {
            return Err("Consumer viewport cannot shift a mixed native parent chain");
        }
        if !subtree_needs_projection(&group.layers, dynamics) {
            return Err("Consumer viewport applies only to 3D-source precompositions");
        }
        for sibling in siblings {
            if sibling.id() == group.id {
                continue;
            }
            if root_viewport::references(sibling, group.id) != Some(false) {
                return Err("External or unknown consumer references this source");
            }
            if let LayerData::Adjustment(adjustment) = sibling.data()
                && effect_support::stack(&adjustment.effects).is_err()
            {
                return Err("Nonpointwise sibling Adjustment samples this source");
            }
        }
        let bounds = self.propagated.finite_union()?;
        if bounds
            .min
            .iter()
            .chain(bounds.max.iter())
            .any(|v| !v.is_finite())
        {
            return Err("Consumer viewport has non-finite bounds");
        }
        self.finite_canvas = Some(CertifiedCanvas {
            bounds,
            root_output: false,
            consumer_3d: true,
            source_mask: false,
            intersect_content: false,
        });
        Ok(())
    }

    /// A mixed source with unknown Text glyph bounds may use only the pixels
    /// that can reach its native consumer. Every ancestor's pointwise effects,
    /// clocks and inverse planar transforms have already shaped this demand;
    /// unsafe masks and non-invertible controls mark it full instead.
    /// An exact root clock may request any moment in its native source. The
    /// final output clip is still the same untransformed canvas at every such
    /// moment; replacing the clock's unknown phase with all source times only
    /// enlarges demand, never drops a visible pixel.
    pub(super) fn use_clocked_root_output_viewport(&mut self, source_duration_millis: u64) {
        if self
            .finite_canvas
            .as_ref()
            .is_some_and(|canvas| canvas.root_output)
            && let Some(canvas) = &self.finite_canvas
        {
            self.propagated = Demand::root(canvas.bounds, source_duration_millis);
        }
    }

    /// The caller has already checked native clock representability. Widen
    /// temporal coverage after spatial inversion; no source phase is assumed.
    pub(super) fn use_checked_source_domain(&mut self, checked: Self) {
        self.propagated = checked.propagated;
    }

    pub(super) fn text_canvas(
        &self,
        group: &GroupLayer,
        siblings: &[Layer],
    ) -> Result<Bounds, &'static str> {
        // Null parenting and native collapse are selected before this fallback.
        // Effectful or opacity-keyed Text-only groups may instead need a real
        // precomposition; they obey the same finite-demand checks as mixed ones.
        if group.motion_blur || !group.fills.is_empty() {
            return Err("Text consumer has motion blur or authored fills");
        }
        // Finite spatial demand does not certify compositing isolation or the
        // additional shutter-time samples of a nested Text source.
        fn text_compositing_is_supported(layer: &Layer) -> bool {
            match layer.data() {
                LayerData::Text(text) => text.blend_mode == Default::default() && !text.motion_blur,
                LayerData::Group(group) => {
                    group.blend_mode == Default::default()
                        && !group.motion_blur
                        && group.layers.iter().all(text_compositing_is_supported)
                }
                _ => true,
            }
        }
        if !group.layers.iter().all(text_compositing_is_supported) {
            return Err("Text source has unsupported nested blending or motion blur");
        }
        if group
            .layers
            .iter()
            .any(|child| child.parent_id().is_some_and(|parent| parent != group.id))
        {
            return Err("Text source has external native parent references");
        }
        for sibling in siblings {
            if sibling.id() == group.id {
                continue;
            }
            if root_viewport::references(sibling, group.id) != Some(false) {
                return Err("Text source has external or unknown sibling consumers");
            }
            if let LayerData::Adjustment(adjustment) = sibling.data()
                && effect_support::stack(&adjustment.effects).is_err()
            {
                return Err("Nonpointwise sibling Adjustment samples Text source");
            }
        }
        self.propagated.finite_union()
    }

    pub(super) const fn finite_canvas(&self) -> Option<&CertifiedCanvas> {
        self.finite_canvas.as_ref()
    }

    pub(super) const fn propagated(&self) -> &Demand {
        &self.propagated
    }
}

pub(super) fn validate_masked_source(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<(), &'static str> {
    animated_bounds::validate_masked_source(group, dynamics, resolved_media, canvas)
}

pub(super) fn has_active_shadow(records: &[fx_schema::EffectRecord]) -> bool {
    records.iter().any(|record| {
        let payload = match record.data() {
            fx_schema::EffectData::Identified { enabled: false, .. } => return false,
            fx_schema::EffectData::Identified { effect, .. }
            | fx_schema::EffectData::Legacy(effect) => effect,
        };
        matches!(payload, fx_schema::EffectPayload::Known(fx_schema::LayerEffect::DropShadow(shadow)) if shadow.enabled)
    })
}

pub(super) fn static_shadow_stack(
    records: &[fx_schema::EffectRecord],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    effect_support::shadow_bounds(
        Bounds {
            min: [0.0; 2],
            max: [0.0; 2],
        },
        records,
        dynamics,
    )
    .is_ok()
}

fn group_effect_bounds(
    bounds: Bounds,
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<Bounds, &'static str> {
    // Other profiles retain their existing content-only policy and diagnostics.
    // This exception does not certify an inverse sampling demand or a crop.
    if static_shadow_stack(&group.effects, dynamics) {
        effect_support::shadow_bounds(bounds, &group.effects, dynamics)
    } else {
        Ok(bounds)
    }
}

pub(super) fn pointwise_effect_stack(
    records: &[fx_schema::EffectRecord],
) -> Result<(), &'static str> {
    effect_support::stack(records)
}

pub(super) fn root_demand(canvas: fx_schema::Dimensions, duration_ms: u64) -> Demand {
    Demand::root(
        Bounds {
            min: [0.0; 2],
            max: [f64::from(canvas.width), f64::from(canvas.height)],
        },
        duration_ms,
    )
}

/// The zero-origin, full-window unit mapping is exactly `t - active.start`,
/// matching `Demand::map_clock(start, 1)`. Keep existing strict identity behavior;
/// independent mapping windows, offsets and TimeRemap need separate phase proof.
fn demand_clock_is_unit(playback: &fx_schema::layer::LayerPlayback) -> bool {
    super::playback_is_identity(playback)
        || (playback.input_offset_ms() == 0
            && matches!(
                playback.mapping(),
                fx_schema::layer::LayerPlaybackMapping::Linear { input, output }
                    if *input == playback.input_range()
                        && output.start == Time::ZERO
                        && input.duration == output.duration
            ))
}

/// Reverse occurrence operators while retaining root-time/local-time coupling.
/// Native precomposition sampling at a new crop boundary has no independent
/// support proof yet, so inferred demand never replaces full bounds.
pub(super) fn child_demand(
    source: &GroupLayer,
    _geometry: &GroupLayer,
    masks: &[fx_schema::layer::PathMask],
    dynamics: &crate::export_document::AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    inherited: &Demand,
    native_pointwise_mask_crop: bool,
) -> ChildDemand {
    child_demand_inner(
        source,
        masks,
        dynamics,
        canvas,
        inherited,
        native_pointwise_mask_crop,
        None,
        false,
    )
}

/// Call only after checking native clock representation and its source domain.
pub(super) fn checked_source_demand(
    source: &GroupLayer,
    masks: &[fx_schema::layer::PathMask],
    dynamics: &crate::export_document::AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    inherited: &Demand,
    native_pointwise_mask_crop: bool,
    source_duration_millis: u64,
) -> ChildDemand {
    child_demand_inner(
        source,
        masks,
        dynamics,
        canvas,
        inherited,
        native_pointwise_mask_crop,
        Some(source_duration_millis),
        false,
    )
}

// Mask crop, checked source duration and blur approximation are independent proof inputs.
#[allow(clippy::too_many_arguments)]
fn child_demand_inner(
    source: &GroupLayer,
    masks: &[fx_schema::layer::PathMask],
    dynamics: &crate::export_document::AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    inherited: &Demand,
    native_pointwise_mask_crop: bool,
    checked_source_duration: Option<u64>,
    approximate_blur_reach: bool,
) -> ChildDemand {
    let mut propagated = inherited.clone();
    let active_range = source.playback.input_range();
    propagated.active(
        active_range.start.as_millis(),
        active_range.end().as_millis(),
    );
    let finite_input_profile = demand_clock_is_unit(&source.playback)
        && !subtree_needs_projection(&source.layers, dynamics)
        && root_viewport::input_canvas(source, dynamics, &[], canvas).is_some();
    let reach = if approximate_blur_reach {
        effect_support::viewport_approximation_reach(&source.effects, dynamics)
    } else if finite_input_profile {
        effect_support::input_reach(&source.effects, dynamics)
    } else {
        effect_support::stack(&source.effects).map(|()| 0.0)
    };
    if let Err(reason) = reach {
        propagated.full(reason);
    }
    if !masks.is_empty() && !native_pointwise_mask_crop {
        match finite_mask_gate(masks, dynamics) {
            Ok(_gate) => propagated.full("Static Group Add mask is structurally finite, but native crop-boundary support is not independently proved"),
            Err(reason) => propagated.full(reason),
        }
    }
    if source.track_matte.is_some() && !finite_input_profile {
        propagated.full("Group matte source has no proved crop support");
    }
    if demand_clock_is_unit(&source.playback) {
        propagated.map_clock(active_range.start.as_millis(), 1.0);
    } else if checked_source_duration.is_none() {
        propagated.full("Nonidentity Group source clock requires coupled native phase proof");
    }
    if let Err(reason) = animated_bounds::inverse_planar_demand(
        &mut propagated,
        source.id,
        // The geometry carrier may have been reset to identity for native
        // separated X/Y tracks. Invert the actual FX occurrence, not that carrier.
        &source.transform,
        dynamics,
        canvas,
    ) {
        propagated.full(reason);
    }
    if let Ok(radius) = reach {
        propagated.inverse_regions(|bounds| bounds.expand(radius));
    }
    if let Some(duration) = checked_source_duration {
        // Spatial inversion used the actual occurrence and all-time transform
        // hulls. Its union is valid at every checked native source time, even
        // when a nonlinear/remapped clock visits them out of order.
        if let Ok(bounds) = propagated.finite_union() {
            propagated = Demand::root(bounds, duration);
        }
    }
    ChildDemand {
        propagated,
        finite_canvas: None,
    }
}

/// Only the precise finite-geometry overflow rejection can trigger a retry.
pub(super) const NATIVE_CANVAS_OVERFLOW: &str = "Proven precomposition bounds exceed the native canvas; subtree is not a certified collapsed 2D vector source";

/// A deliberately approximate source working plane, not a final-output or
/// native sampling certificate. Original geometry validation must run first.
pub(super) fn approximate_child_demand(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    inherited: &Demand,
    siblings: &[Layer],
    checked_source_duration: Option<u64>,
) -> Option<ChildDemand> {
    if !demand_clock_is_unit(&group.playback)
        || !group.masks.is_empty()
        || group.track_matte.is_some()
        || !group.fills.is_empty()
        || subtree_has_temporal_effects(&group.layers)
    {
        return None;
    }
    for sibling in siblings {
        if sibling.id() != group.id
            && (root_viewport::references(sibling, group.id)?
                || matches!(sibling.data(), LayerData::Adjustment(adjustment)
                    if !adjustment.effects.iter().all(root_viewport::pointwise_adjustment)))
        {
            return None;
        }
    }
    // Full inherited demand remains Full. No nominal root-canvas substitution.
    inherited.finite_union().ok()?;
    // Keep the spatial union in the already validated native source-time
    // domain, just as checked_source_demand does on the ordinary path.
    let mut candidate = child_demand_inner(
        group,
        &[],
        dynamics,
        canvas,
        inherited,
        false,
        checked_source_duration,
        true,
    );
    let bounds = candidate.propagated.finite_union().ok()?;
    candidate.finite_canvas = Some(CertifiedCanvas {
        bounds,
        root_output: false,
        consumer_3d: false,
        source_mask: false,
        intersect_content: false,
    });
    Some(candidate)
}

fn subtree_has_temporal_effects(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| {
        animated_bounds::temporal_effects(layer.data().effects())
            || matches!(layer.data(), LayerData::Group(group)
                if subtree_has_temporal_effects(&group.layers))
    })
}

/// Necessary, not sufficient, proof of a finite source-local mask enclosure.
/// The actual native/source support comparison must authorize use as a crop.
fn finite_mask_gate(
    masks: &[fx_schema::layer::PathMask],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<Bounds, &'static str> {
    let [mask] = masks else {
        return Err("Only one static Add mask has a candidate finite support gate");
    };
    if mask.mode != fx_schema::layer::MaskMode::Add
        || mask.inverted
        || mask.opacity.value().min(1.0) != 1.0
        || mask.feather != [0.0; 2]
        || mask.expansion != 0.0
        || dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return Err(
            "Mask mode, inversion, soft edge, opacity or animation has no finite support gate",
        );
    }
    let Some(path) = &mask.legacy_path else {
        return Err("Referenced mask guide transform/path has no proved static local hull");
    };
    path_bounds(&path.commands)
}

#[expect(
    clippy::large_enum_variant,
    reason = "each Group builds and consumes one plan on the stack; boxing would add an allocation per Group"
)]
pub(super) enum HierarchyPlan {
    Parent(ParentPlan),
    Precomposition(PrecompositionPlan),
}

pub(super) struct ParentPlan {
    parent: NullLayerSpec,
    parent_id: LayerId,
}

/// Why a precomposition uses native collapse transformations. Each source has
/// its own eligibility and occurrence rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CollapsedSource {
    /// Certified 2D vectors whose proven bounds exceed the native canvas.
    OversizedVector,
    /// Plain 2D Text, which has no FX glyph bounds to size a source canvas.
    Text,
}

pub(super) struct PrecompositionPlan {
    name: String,
    collapsed: Option<CollapsedSource>,
    width: u16,
    height: u16,
    duration: Duration24,
    origin: [f64; 2],
    camera: Option<crate::writer::NativeCameraSpec>,
    consumer_3d: bool,
    transform: SolidTransform,
    transform_animations: TransformAnimations,
    inner_parent: Option<skew::Lowering>,
}

/// Selects the narrow exact hierarchy representation. Native parenting is
/// preferred because it introduces no canvas clipping; precomposition is used
/// only when isolation is required and every visual bound is established.
#[cfg(test)]
pub(super) fn classify(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<HierarchyPlan, &'static str> {
    classify_with_demand(
        group,
        composition_end,
        duration,
        dynamics,
        resolved_media,
        canvas,
        None,
        None,
    )
}

// Keep the established classifier's clock/media/canvas inputs explicit.
#[allow(clippy::too_many_arguments)]
pub(super) fn classify_with_demand(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    finite_canvas: Option<&CertifiedCanvas>,
    text_canvas: Option<Bounds>,
) -> Result<HierarchyPlan, &'static str> {
    classify_inner(
        group,
        composition_end,
        duration,
        dynamics,
        resolved_media,
        canvas,
        false,
        None,
        finite_canvas,
        text_canvas,
        None,
    )
}

/// Selects the exact two-transform representation for one static planar skew.
/// The caller owns collision-free allocation of `helper_id`.
#[cfg(test)]
pub(super) fn classify_with_skew_helper(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    helper_id: LayerId,
) -> Result<HierarchyPlan, &'static str> {
    classify_with_skew_helper_and_demand(
        group,
        composition_end,
        duration,
        dynamics,
        resolved_media,
        canvas,
        None,
        None,
        helper_id,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the demand-aware classify inputs plus the caller-allocated skew helper id"
)]
pub(super) fn classify_with_skew_helper_and_demand(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    finite_canvas: Option<&CertifiedCanvas>,
    text_canvas: Option<Bounds>,
    helper_id: LayerId,
) -> Result<HierarchyPlan, &'static str> {
    classify_inner(
        group,
        composition_end,
        duration,
        dynamics,
        resolved_media,
        canvas,
        true,
        Some(helper_id),
        finite_canvas,
        text_canvas,
        None,
    )
}

pub(super) fn has_skew(group: &GroupLayer) -> bool {
    skew::is_present(&group.transform)
}

pub(super) fn validate_skew_source(group: &GroupLayer) -> Result<(), &'static str> {
    skew::validate_source(group)
}

#[cfg(test)]
pub(super) fn classify_precomposition(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<HierarchyPlan, &'static str> {
    classify_precomposition_with_demand(
        group,
        composition_end,
        duration,
        dynamics,
        resolved_media,
        canvas,
        None,
        None,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "source visibility and spatial demand are independently certified bounds inputs"
)]
pub(super) fn classify_precomposition_with_demand(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    finite_canvas: Option<&CertifiedCanvas>,
    text_canvas: Option<Bounds>,
    source_interval: Option<SourceInterval>,
) -> Result<HierarchyPlan, &'static str> {
    classify_inner(
        group,
        composition_end,
        duration,
        dynamics,
        resolved_media,
        canvas,
        true,
        None,
        finite_canvas,
        text_canvas,
        source_interval,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared core of the classify entry points; each extra argument is one entry point's policy"
)]
fn classify_inner(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    force_precomposition: bool,
    skew_helper_id: Option<LayerId>,
    finite_canvas: Option<&CertifiedCanvas>,
    text_canvas: Option<Bounds>,
    source_interval: Option<SourceInterval>,
) -> Result<HierarchyPlan, &'static str> {
    check_shared_group(group, composition_end)?;
    let inner_parent = skew_helper_id
        .map(|helper_id| skew::lower(group, dynamics, helper_id))
        .transpose()?;
    let (transform, transform_animations) = if let Some(lowering) = &inner_parent {
        (lowering.outer.clone(), lowering.outer_animations.clone())
    } else {
        native_transform(group, dynamics)?
    };
    let spatial = subtree_needs_projection(&group.layers, dynamics);
    let parenting_eligible = inner_parent.is_none()
        && !force_precomposition
        && !spatial
        && parenting_eligible(group)
        && transform_animations.opacity.is_none();
    // Childless controls have no painted extent, even when their occurrence
    // clock requires the forced classifier. Keep a native non-rendering Null
    // rather than inventing a source canvas; caller validates its clock/mattes.
    let empty_controls = inner_parent.is_none() && empty_controls_eligible(group);
    if empty_controls
        || (parenting_eligible
            && (group.layers.len() > 1
                || group.layers.first().is_some_and(|child| {
                    text_only_branch(child)
                        || (child.active_range().start != group.playback.input_range().start
                            && !subtree_has_dynamics(&group.layers, dynamics))
                })))
    {
        return Ok(HierarchyPlan::Parent(ParentPlan {
            parent: NullLayerSpec {
                name: group.name.clone(),
                transform,
                transform_animations,
            },
            parent_id: group.id,
        }));
    }
    // Audio has no visual extent. A minimal transparent source canvas is exact
    // for an audio-only subtree; do not turn missing/unknown visual bounds into
    // a guessed canvas for mixed content.
    let bounds = if audio_only(&group.layers) {
        Ok(Some(Bounds {
            min: [0.0; 2],
            max: [1.0; 2],
        }))
    } else if group.fills.is_empty()
        && group.effects.is_empty()
        && !spatial
        && transparent_hidden_content(&group.layers)
    {
        // Keep the containing canvas for known disabled artwork, rather than
        // shrinking its editable source to the audio-only one-pixel convention.
        Ok(Some(Bounds {
            min: [0.0; 2],
            max: [f64::from(canvas.width), f64::from(canvas.height)],
        }))
    } else if spatial
        || subtree_has_masks(&group.layers)
        || subtree_has_dynamics(&group.layers, dynamics)
    {
        animated_bounds::child_union_in_interval(
            group,
            dynamics,
            resolved_media,
            canvas,
            source_interval,
        )
    } else {
        child_union(group, resolved_media, canvas)
    };
    let spatial_text_canvas = spatial
        && inner_parent.is_none()
        && matches!(
            bounds,
            Err("Text/font glyph bounds are not known from the FX text box")
        )
        && finite_canvas
            .is_some_and(|canvas| canvas.root_output || canvas.consumer_3d || canvas.source_mask);
    let bounds = match bounds {
        Ok(Some(bounds)) => bounds,
        Ok(None) if parenting_eligible => {
            // Preserve the established single-child precomposition shape when
            // bounds are known, but use a Null when content such as point Text
            // has no finite geometry bounds. Parenting is exact in that case.
            return Ok(HierarchyPlan::Parent(ParentPlan {
                parent: NullLayerSpec {
                    name: group.name.clone(),
                    transform,
                    transform_animations,
                },
                parent_id: group.id,
            }));
        }
        Ok(None) => return Err("Precomposition has no finite visual child render bounds"),
        Err(_) if !spatial && inner_parent.is_none() && collapsed::text_only(group, dynamics) => {
            // Only Text lacks bounds here; the root canvas is a nominal source
            // size that collapse does not clip, never a guessed glyph extent.
            return Ok(HierarchyPlan::Precomposition(PrecompositionPlan {
                name: group.name.clone(),
                collapsed: Some(CollapsedSource::Text),
                width: u16::try_from(canvas.width).map_err(|_| "Native canvas width overflow")?,
                height: u16::try_from(canvas.height)
                    .map_err(|_| "Native canvas height overflow")?,
                duration,
                origin: [0.0; 2],
                camera: None,
                consumer_3d: false,
                transform,
                transform_animations,
                inner_parent: None,
            }));
        }
        Err("Text/font glyph bounds are not known from the FX text box")
            if spatial
                && inner_parent.is_none()
                && finite_canvas.is_some_and(|canvas| {
                    canvas.root_output || canvas.consumer_3d || canvas.source_mask
                }) =>
        {
            animated_bounds::validate_spatial_source_with_planar_text(
                group,
                dynamics,
                resolved_media,
                canvas,
            )?;
            finite_canvas
                .expect("guarded final-output, planar-consumer, or native-mask certificate")
                .bounds
        }
        Err("Text/font glyph bounds are not known from the FX text box")
            if !spatial
                && inner_parent.is_none()
                && (text_canvas.is_some()
                    || finite_canvas.is_some_and(|canvas| canvas.root_output)) =>
        {
            // This is the consumer's all-time visible preimage, not a guess
            // from the FX text box. A certified identity root clips to the
            // output even when its source clock is nonidentity.
            text_canvas
                .or_else(|| finite_canvas.map(|canvas| canvas.bounds))
                .expect("guarded by the viewport certificate")
        }
        Err(error) => return Err(error),
    };
    // Retain the original support/near-plane validation above. A final output
    // viewport is not permission to accept unsafe or unknown child geometry.
    let root_camera = crate::writer::NativeCameraSpec::root(canvas.width, canvas.height);
    // Do not change an already representable source's raster domain. Only rescue
    // sources whose full symmetric 3D canvas would otherwise be omitted.
    let finite_canvas = finite_canvas.filter(|certificate| {
        !certificate.consumer_3d
            || spatial_text_canvas
            || (spatial
                && (0..2).any(|axis| {
                    let radius = (root_camera.center[axis] - bounds.min[axis])
                        .max(bounds.max[axis] - root_camera.center[axis])
                        .ceil()
                        .max(1.0);
                    radius * 2.0 > f64::from(u16::MAX)
                }))
    });
    let bounds = finite_canvas.map_or(bounds, |certified| {
        if certified.intersect_content {
            Bounds {
                min: std::array::from_fn(|axis| bounds.min[axis].max(certified.bounds.min[axis])),
                max: std::array::from_fn(|axis| bounds.max[axis].min(certified.bounds.max[axis])),
            }
        } else {
            certified.bounds
        }
    });
    let bounds = group_effect_bounds(bounds, group, dynamics)?;
    let bounds = if let Some(lowering) = &inner_parent {
        let inner = &lowering.inner.transform;
        affine_bounds(
            bounds,
            inner.anchor,
            inner.position,
            inner.scale,
            inner.rotation,
            0.0,
            0.0,
        )?
    } else {
        bounds
    };
    // AE's principal point is the source canvas center, not camera world XY.
    // A symmetric enclosure preserves the root lens without clipping content.
    let bounds = if spatial && !finite_canvas.is_some_and(|canvas| canvas.root_output) {
        let radius = std::array::from_fn::<_, 2, _>(|axis| {
            (root_camera.center[axis] - bounds.min[axis])
                .max(bounds.max[axis] - root_camera.center[axis])
                .ceil()
                .max(1.0)
        });
        Bounds {
            min: std::array::from_fn(|axis| root_camera.center[axis] - radius[axis]),
            max: std::array::from_fn(|axis| root_camera.center[axis] + radius[axis]),
        }
    } else {
        Bounds {
            min: bounds.min.map(f64::floor),
            max: bounds.max.map(f64::ceil),
        }
    };
    let [mut left, mut top] = bounds.min;
    let [mut right, mut bottom] = bounds.max;
    if ![left, top, right, bottom].into_iter().all(f64::is_finite) || right <= left || bottom <= top
    {
        return Err("Proven precomposition bounds are non-finite or empty");
    }
    let consumer_3d = finite_canvas.is_some_and(|canvas| canvas.consumer_3d) && spatial;
    if consumer_3d {
        // Keep world/lens values unchanged and use dyadic raster axes. Native
        // controls still show 1-LSB edge/alpha differences; this is not exact.
        let extent = [right - left, bottom - top];
        let sides = extent.map(|value| {
            if !value.is_finite() || value > f64::from(u16::MAX) {
                return None;
            }
            let required = value.ceil().max(1.0) as u32;
            required
                .checked_next_power_of_two()
                .filter(|side| *side <= u32::from(u16::MAX))
        });
        let [Some(width), Some(height)] = sides else {
            return Err("3D consumer viewport cannot fit power-of-two native canvas");
        };
        left = root_camera.center[0] - f64::from(width) * 0.5;
        top = root_camera.center[1] - f64::from(height) * 0.5;
        right = left + f64::from(width);
        bottom = top + f64::from(height);
    }
    let collapse_transformations =
        right - left > f64::from(u16::MAX) || bottom - top > f64::from(u16::MAX);
    if collapse_transformations {
        if spatial || inner_parent.is_some() || !collapsed::eligible(group, dynamics) {
            return Err(NATIVE_CANVAS_OVERFLOW);
        }
        // Collapse retains vector geometry beyond this source viewport. Unlike
        // cropping, no child geometry or effect input is shortened or shifted.
        left = 0.0;
        top = 0.0;
        right = f64::from(u16::try_from(canvas.width).map_err(|_| "Native canvas width overflow")?);
        bottom =
            f64::from(u16::try_from(canvas.height).map_err(|_| "Native canvas height overflow")?);
        if right == 0.0 || bottom == 0.0 {
            return Err("Collapsed source canvas must be nonempty");
        }
    }
    let origin = [left, top];
    // Consumer-only variant keeps the original 3D world and camera coordinates.
    // The new canvas center replaces the old principal point; 2D children and
    // the occurrence anchor shift together to cancel that change at output.
    let mut transform = transform;
    for (anchor, origin) in transform.anchor.iter_mut().zip(origin) {
        *anchor -= origin;
    }
    let mut transform_animations = transform_animations;
    translate_track(
        transform_animations.anchor.as_mut(),
        [-origin[0], -origin[1]],
    )?;
    Ok(HierarchyPlan::Precomposition(PrecompositionPlan {
        name: group.name.clone(),
        collapsed: collapse_transformations.then_some(CollapsedSource::OversizedVector),
        width: (right - left) as u16,
        height: (bottom - top) as u16,
        duration,
        origin,
        camera: spatial.then(|| crate::writer::NativeCameraSpec {
            center: if consumer_3d {
                root_camera.center
            } else {
                [root_camera.center[0] - left, root_camera.center[1] - top]
            },
            distance: root_camera.distance,
        }),
        consumer_3d,
        transform,
        transform_animations,
        inner_parent,
    }))
}

impl ParentPlan {
    /// Parents direct emitted roots and appends the non-rendering Null after
    /// them, preserving AE stacking and child-owned switches/clocks.
    pub(super) fn finish(
        self,
        mut children: Vec<LayerSpec>,
        options: NativeLayerOptions,
    ) -> Result<Vec<LayerSpec>, crate::writer::AepWriteError> {
        for child in &mut children {
            child.assign_root_parent(self.parent_id)?;
        }
        children.push(LayerSpec::Options(
            Box::new(LayerSpec::Null(self.parent)),
            options,
        ));
        Ok(children)
    }
}

impl PrecompositionPlan {
    pub(super) const fn consumer_viewport(&self) -> bool {
        self.consumer_3d
    }

    pub(super) const fn collapsed_source(&self) -> Option<CollapsedSource> {
        self.collapsed
    }

    pub(super) const fn mask_space(&self) -> ([u32; 2], [f64; 2]) {
        ([self.width as u32, self.height as u32], self.origin)
    }

    /// Shifts only nested composition roots and conjugates the occurrence
    /// anchor by the same origin, so the world transform remains unchanged.
    pub(super) fn finish(
        self,
        mut children: Vec<LayerSpec>,
        mut options: NativeLayerOptions,
        composition_options: Option<CompositionOptions>,
    ) -> Result<LayerSpec, crate::writer::AepWriteError> {
        let offset = [-self.origin[0], -self.origin[1]];
        if let Some(lowering) = self.inner_parent {
            for child in &mut children {
                child.assign_root_parent(lowering.helper_id)?;
            }
            children.push(LayerSpec::Options(
                Box::new(LayerSpec::Null(lowering.inner)),
                skew::helper_options(lowering.helper_id),
            ));
        }
        for child in &mut children {
            if self.consumer_3d {
                child.translate_planar_composition_root(offset)?;
            } else {
                child.translate_composition_root(offset)?;
            }
        }
        if let Some(camera) = self.camera {
            crate::writer::append_root_camera(&mut children, camera)?;
        }
        if let Some((transform, animations)) = &mut options.transform_3d {
            for (anchor, delta) in transform.anchor.iter_mut().zip(offset) {
                *anchor += delta;
            }
            if let Some(track) = &mut animations.anchor {
                crate::writer::translate_numeric_track(track, &[offset[0], offset[1], 0.0])?;
            }
        }
        let mut composition_record =
            crate::schema::CompositionRecord::empty_ae26(self.width, self.height, self.duration)?;
        if let Some(value) = composition_options {
            crate::writer::apply_composition_options(&mut composition_record, value)?;
        }
        Ok(LayerSpec::Options(
            Box::new(LayerSpec::Precomposition(PrecompositionSpec {
                name: self.name,
                collapse_transformations: self.collapsed.is_some(),
                width: self.width,
                height: self.height,
                duration: self.duration,
                transform: self.transform,
                transform_animations: self.transform_animations,
                layers: children,
                composition_record: Some(composition_record),
            })),
            options,
        ))
    }
}

pub(super) fn audio_only(layers: &[Layer]) -> bool {
    fn audio_count_without_visuals(layers: &[Layer]) -> Option<usize> {
        layers.iter().try_fold(0usize, |count, layer| {
            let child_count = match layer.data() {
                LayerData::Audio(_) => 1,
                LayerData::Group(group)
                    if group.fills.is_empty()
                        && group.effects.is_empty()
                        && group.masks.is_empty() =>
                {
                    audio_count_without_visuals(&group.layers)?
                }
                _ => return None,
            };
            Some(count + child_count)
        })
    }

    audio_count_without_visuals(layers).is_some_and(|count| count > 0)
}

fn check_shared_group(group: &GroupLayer, composition_end: Time) -> Result<(), &'static str> {
    let active_range = group.playback.input_range();
    if active_range.start != Time::ZERO || active_range.end() < composition_end {
        return Err(
            "Native hierarchy requires a full-composition Group span; parent visibility and lifetime do not inherit",
        );
    }
    if !identity_clock(group, composition_end) {
        return Err("Group source clock is not the established full-span identity mapping");
    }
    if !group.masks.is_empty() {
        return Err("Group masks need occurrence records outside this hierarchy helper");
    }
    if !group.fills.is_empty()
        || [
            group.padding_top.value(),
            group.padding_right.value(),
            group.padding_bottom.value(),
            group.padding_left.value(),
            group.corner_radius_top_left.value(),
            group.corner_radius_top_right.value(),
            group.corner_radius_bottom_right.value(),
            group.corner_radius_bottom_left.value(),
        ]
        .into_iter()
        .any(|value| value != 0.0)
    {
        return Err("Group background, padding, or corners require isolated authored content");
    }
    Ok(())
}

fn identity_clock(group: &GroupLayer, composition_end: Time) -> bool {
    super::group_has_root_identity_clock(group, composition_end)
}

/// Text, or a nonempty Group of such branches, such as an imported text layer's
/// clock Group holding one Text per held Source Text value. FX Text has no glyph
/// bounds to size a precomposition; each nested Group is classified on its own.
pub(super) fn text_only_branch(layer: &Layer) -> bool {
    match layer.data() {
        LayerData::Text(_) => true,
        LayerData::Group(group) => {
            !group.layers.is_empty() && group.layers.iter().all(text_only_branch)
        }
        _ => false,
    }
}

// Known disabled artwork is transparent, not unknown geometry. Keep a real
// transparent source for a live masked/matted owner rather than dropping the
// owner and leaving its canvas guide as ordinary paint. Never guess for live
// unsupported content or for effects/fills that could generate coverage.
fn transparent_hidden_content(layers: &[Layer]) -> bool {
    layers.iter().all(|layer| {
        if super::layer_is_hidden(layer) {
            return true;
        }
        match layer.data() {
            LayerData::Audio(_) => true,
            LayerData::Group(group) => {
                group.fills.is_empty()
                    && group.effects.is_empty()
                    && group.blend_mode == Default::default()
                    && transparent_hidden_content(&group.layers)
            }
            _ => false,
        }
    })
}

pub(super) fn empty_controls_eligible(group: &GroupLayer) -> bool {
    group.layers.is_empty()
        && group.fills.is_empty()
        && group.effects.is_empty()
        && group.masks.is_empty()
        && group.track_matte.is_none()
}

fn parenting_eligible(group: &GroupLayer) -> bool {
    // Null parenting also preserves a single child's affine transform. Requiring
    // multiple children needlessly forces Text through unknown glyph bounds.
    // Opacity animation is checked separately: a Null's opacity is not inherited.
    !group.layers.is_empty()
        && !group.is_hidden
        && group.blend_mode == Default::default()
        && group.track_matte.is_none()
        && !group.motion_blur
        && group.transform.opacity.value() == 100.0
}

fn native_transform(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<(SolidTransform, TransformAnimations), &'static str> {
    let Position::TwoD(position) = group.transform.position else {
        return Err("Native hierarchy does not invent 3D parent/precomposition records");
    };
    if group.transform.skew != 0.0
        || group.transform.skew_axis != 0.0
        || group.transform.rotation_x != 0.0
        || group.transform.rotation_y != 0.0
        || group.transform.orientation != [0.0; 3]
    {
        return Err("Native AV hierarchy Transform has no established skew/3D mapping");
    }
    let transform = SolidTransform {
        anchor: group.transform.anchor_point,
        position,
        scale: group.transform.scale,
        rotation: group.transform.rotation,
        opacity: group.transform.opacity.value(),
    };
    let animations = super::transform_animations(dynamics, group.id, &group.transform, group.id)?;
    Ok((transform, animations))
}

fn subtree_needs_projection(
    layers: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    layers.iter().any(|layer| {
        super::mask_and_transform(layer).is_some_and(|(transform, _, _)| {
            super::transform3d::requires_native_3d(dynamics, transform, layer.id())
        }) || matches!(layer.data(), LayerData::Group(group) if subtree_needs_projection(&group.layers, dynamics))
    })
}

fn subtree_has_masks(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| {
        super::mask_and_transform(layer).is_some_and(|(_, masks, _)| !masks.is_empty())
            || matches!(layer.data(), LayerData::Group(group) if subtree_has_masks(&group.layers))
    })
}

fn subtree_has_dynamics(
    layers: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    layers.iter().any(|layer| {
        dynamics.for_layer(layer.id()).next().is_some()
            || matches!(layer.data(), LayerData::Shape(shape) if blur_bounds::has_dynamics(&shape.effects, dynamics))
            || layer
                .child_layers()
                .is_some_and(|children| subtree_has_dynamics(children, dynamics))
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    pub(super) min: [f64; 2],
    pub(super) max: [f64; 2],
}

impl Bounds {
    pub(super) fn include(&mut self, other: Self) {
        for axis in 0..2 {
            self.min[axis] = self.min[axis].min(other.min[axis]);
            self.max[axis] = self.max[axis].max(other.max[axis]);
        }
    }

    fn expand(self, amount: f64) -> Result<Self, &'static str> {
        if !amount.is_finite() || amount < 0.0 {
            return Err("Static shape expansion is not finite and nonnegative");
        }
        Ok(Self {
            min: [self.min[0] - amount, self.min[1] - amount],
            max: [self.max[0] + amount, self.max[1] + amount],
        })
    }
}

pub(super) fn static_child_union(
    group: &GroupLayer,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<Option<Bounds>, &'static str> {
    child_union(group, resolved_media, canvas)
}

pub(super) fn all_time_layer_bounds(
    layer: &Layer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<Option<Bounds>, &'static str> {
    if subtree_has_masks(std::slice::from_ref(layer))
        || subtree_has_dynamics(std::slice::from_ref(layer), dynamics)
    {
        animated_bounds::layer_bounds(layer, dynamics, resolved_media, canvas)
    } else {
        layer_bounds(layer, resolved_media, canvas)
    }
}

fn child_union(
    group: &GroupLayer,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<Option<Bounds>, &'static str> {
    let mut bounds: Option<Bounds> = None;
    for layer in &group.layers {
        if let Some(next) = layer_bounds(layer, resolved_media, canvas)? {
            match &mut bounds {
                Some(bounds) => bounds.include(next),
                None => bounds = Some(next),
            }
        }
    }
    Ok(bounds)
}

fn layer_bounds(
    layer: &Layer,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<Option<Bounds>, &'static str> {
    match layer.data() {
        LayerData::Rect(rect) => {
            if rect.is_hidden {
                return Ok(None);
            }
            if !rect.masks.is_empty() {
                return Err("Rect masks make precomposition render bounds unproved");
            }
            let mut bounds = Bounds {
                min: rect.rect.position,
                max: [
                    rect.rect.position[0] + rect.rect.size[0],
                    rect.rect.position[1] + rect.rect.size[1],
                ],
            };
            if rect.rect.stroke_enabled {
                let half = rect.rect.stroke_width.value() / 2.0;
                let reach = if rect.rect.stroke_join == ShapeLineJoin::Miter {
                    half * rect.rect.stroke_miter_limit.max(1.0)
                } else {
                    half
                };
                bounds = bounds.expand(reach)?;
            }
            transform_bounds(bounds, &rect.transform).map(Some)
        }
        LayerData::Shape(shape) => {
            if shape.is_hidden {
                return Ok(None);
            }
            if !shape.masks.is_empty() {
                return Err("Path masks make precomposition render bounds unproved");
            }
            // Integer, unrounded radial primitives have a source-derived
            // circumradius. Reuse the existing checked enclosure arithmetic;
            // do not infer a hull for rounded/fractional native semantics.
            if shape.shape.ellipse.is_none()
                && shape.shape.path.commands.is_empty()
                && let Some(star) = &shape.shape.poly_star
                && star.points.is_finite()
                && (3.0..=1000.0).contains(&star.points)
                && star.points.fract() == 0.0
                && star.outer_radius.is_finite()
                && star.outer_radius >= 0.0
                && star.inner_radius.is_finite()
                && star.inner_radius >= 0.0
                && star.outer_roundness == 0.0
                && star.inner_roundness == 0.0
            {
                return animated_bounds::layer_bounds(
                    layer,
                    &crate::export_document::AnimationIndex::new(&[]),
                    resolved_media,
                    canvas,
                );
            }
            if shape.shape.ellipse.is_some()
                && shape.shape.poly_star.is_none()
                && shape.shape.path.commands.is_empty()
            {
                // The all-time analyzer already derives the checked centered
                // ellipse hull and its modifier/transform reach. An empty index
                // retains this branch's static-only contract.
                return animated_bounds::layer_bounds(
                    layer,
                    &crate::export_document::AnimationIndex::new(&[]),
                    resolved_media,
                    canvas,
                );
            }
            if shape.shape.ellipse.is_some()
                || shape.shape.poly_star.is_some()
                || shape.shape.path.commands.is_empty()
            {
                return Err(
                    "Only checked static Path geometry has a proved Shape render enclosure",
                );
            }
            let mut bounds = path_bounds(&shape.shape.path.commands)?;
            // Mitered Offset Paths corners protrude up to the miter limit times
            // the amount, as the animated analyzer already accounts for.
            let offset_reach = match shape.shape.offset_paths {
                Some(offset) => {
                    let multiplier = if offset.line_join == ShapeLineJoin::Miter {
                        if !offset.miter_limit.is_finite() {
                            return Err("Shape miter limit is non-finite");
                        }
                        offset.miter_limit.abs().max(1.0)
                    } else {
                        1.0
                    };
                    offset.amount.abs() * multiplier
                }
                None => 0.0,
            };
            let mut stroke_reach: f64 = 0.0;
            for stroke in shape.shape.strokes.iter().filter(|stroke| stroke.enabled) {
                let half = stroke.width.value() / 2.0;
                let reach = if stroke.join == ShapeLineJoin::Miter {
                    half * stroke.miter_limit.max(1.0)
                } else {
                    half
                };
                stroke_reach = stroke_reach.max(reach);
            }
            bounds = bounds.expand(
                offset_reach
                    + stroke_reach
                    + blur_bounds::shape_reach(
                        shape,
                        &crate::export_document::AnimationIndex::new(&[]),
                    ),
            )?;
            transform_bounds(bounds, &shape.transform).map(Some)
        }
        LayerData::Group(group) => {
            if group.is_hidden {
                return Ok(None);
            }
            check_static_nested_group(group)?;
            let dynamics = crate::export_document::AnimationIndex::new(&[]);
            if super::logical_bulge_source_bounds(group, &dynamics, canvas).is_some() {
                return animated_bounds::layer_bounds(layer, &dynamics, resolved_media, canvas);
            }
            let Some(bounds) = child_union(group, resolved_media, canvas)? else {
                return Ok(None);
            };
            let bounds =
                collapsed::mask_output(group, &crate::export_document::AnimationIndex::new(&[]))
                    .unwrap_or(bounds);
            let bounds = group_effect_bounds(bounds, group, &dynamics)?;
            transform_bounds(bounds, &group.transform).map(Some)
        }
        LayerData::Image(_) | LayerData::Video(_) => {
            // Match animated bounds: disabled picture has no paint enclosure.
            // In particular, linked sources can retain disabled video beside
            // live independent audio; it must not invalidate visible siblings.
            if super::layer_is_hidden(layer) {
                return Ok(None);
            }
            let request = media::request(layer).ok_or("Visual media has no archive request")?;
            let source = resolved_media
                .get(request.asset_id.as_str())
                .ok_or("Visual media archive source was not resolved for bounds")?;
            let content = super::media_matte_content_view(layer)
                .map_err(|_| "Media matte content view could not be constructed")?;
            let spec = media::lower(&content, source, canvas)?;
            let size = spec.source.dimensions.map(f64::from);
            let geometry = spec.source_geometry;
            let bounds = Bounds {
                min: geometry.origin,
                max: [
                    geometry.origin[0] + size[0] * geometry.scale[0],
                    geometry.origin[1] + size[1] * geometry.scale[1],
                ],
            };
            transform_solid_bounds(bounds, &spec.transform.transform).map(Some)
        }
        // An Adjustment consumes the composition stack, not a finite content
        // rectangle. Preserve that canvas when enclosing its sibling scope.
        LayerData::Adjustment(_) => Ok(Some(Bounds {
            min: [0.0; 2],
            max: [f64::from(canvas.width), f64::from(canvas.height)],
        })),
        LayerData::Audio(_) => Ok(None),
        LayerData::Text(_) => Err("Text/font glyph bounds are not known from the FX text box"),
        _ => Err("Layer kind has no proven finite native precomposition render bounds"),
    }
}

fn check_static_nested_group(group: &GroupLayer) -> Result<(), &'static str> {
    if group.is_hidden {
        return Err("Hidden nested Group requires explicit descendant visibility normalization");
    }
    // Path masks multiply content coverage; use the unmasked enclosure.
    // Effect conversion retains its separately diagnosed source-canvas policy.
    if !group.fills.is_empty() {
        return Err("Nested Group backgrounds make bounds unproved");
    }
    Ok(())
}

pub(super) fn path_bounds(commands: &[ShapePathCommand]) -> Result<Bounds, &'static str> {
    if commands.is_empty() {
        // Empty held samples paint nothing. Including the origin is a
        // conservative finite enclosure; other keys still expand the union.
        return Ok(Bounds {
            min: [0.0; 2],
            max: [0.0; 2],
        });
    }
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    let mut include = |point: [f64; 2]| {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    };
    for command in commands {
        match *command {
            ShapePathCommand::MoveTo { x, y, .. } | ShapePathCommand::LineTo { x, y, .. } => {
                include([x, y])
            }
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                ..
            } => {
                include([c1x, c1y]);
                include([c2x, c2y]);
                include([x, y]);
            }
            ShapePathCommand::Close => {}
        }
    }
    if !min.into_iter().chain(max).all(f64::is_finite) {
        return Err("Static Path has no finite control-point enclosure");
    }
    Ok(Bounds { min, max })
}

fn transform_bounds(bounds: Bounds, transform: &Transform) -> Result<Bounds, &'static str> {
    let Position::TwoD(position) = transform.position else {
        return Err("3D descendant bounds are not proven for precomposition");
    };
    if transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
    {
        return Err("3D descendant bounds are not proven for precomposition");
    }
    affine_bounds(
        bounds,
        transform.anchor_point,
        position,
        transform.scale,
        transform.rotation,
        transform.skew,
        transform.skew_axis,
    )
}

fn transform_solid_bounds(
    bounds: Bounds,
    transform: &SolidTransform,
) -> Result<Bounds, &'static str> {
    affine_bounds(
        bounds,
        transform.anchor,
        transform.position,
        transform.scale,
        transform.rotation,
        0.0,
        0.0,
    )
}

fn affine_bounds(
    bounds: Bounds,
    anchor: [f64; 2],
    position: [f64; 2],
    scale: [f64; 2],
    rotation: f64,
    skew: f64,
    skew_axis: f64,
) -> Result<Bounds, &'static str> {
    if anchor
        .into_iter()
        .chain(position)
        .chain(scale)
        .chain([rotation, skew, skew_axis])
        .any(|value| !value.is_finite())
    {
        return Err("Descendant Transform is non-finite");
    }

    let matrix = skew::matrix_components(scale, rotation, skew, skew_axis)
        .map_err(|_| "Descendant Transform matrix is non-finite")?;
    let map = |point: [f64; 2]| {
        let x = point[0] - anchor[0];
        let y = point[1] - anchor[1];
        [
            position[0] + matrix[0] * x + matrix[1] * y,
            position[1] + matrix[2] * x + matrix[3] * y,
        ]
    };
    let corners = [
        map(bounds.min),
        map([bounds.max[0], bounds.min[1]]),
        map([bounds.min[0], bounds.max[1]]),
        map(bounds.max),
    ];
    if corners
        .iter()
        .flatten()
        .any(|coordinate| !coordinate.is_finite())
    {
        return Err("Descendant Transform bounds are non-finite");
    }
    let mut output = Bounds {
        min: corners[0],
        max: corners[0],
    };
    for corner in corners.into_iter().skip(1) {
        output.include(Bounds {
            min: corner,
            max: corner,
        });
    }
    Ok(output)
}

fn translate_track(
    track: Option<&mut crate::writer::NumericTrack>,
    offset: [f64; 2],
) -> Result<(), &'static str> {
    let Some(track) = track else {
        return Ok(());
    };
    for key in &mut track.keys {
        if key.values.len() < 2 {
            return Err("Native hierarchy anchor key has fewer than two dimensions");
        }
        for (value, offset) in key.values.iter_mut().zip(offset) {
            *value += offset;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "hierarchy/masked_skew_bounds_tests.rs"]
mod masked_skew_bounds_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn static_ellipse_value(size: [f64; 2]) -> serde_json::Value {
        let source = crate::export_document::tests::imported();
        let mut shape = crate::export_document::tests::rect(&source, 210);
        shape["type"] = serde_json::json!("Shape");
        shape.as_object_mut().unwrap().remove("rect");
        shape["transform"]["anchorPoint"] = serde_json::json!([0.0, 0.0]);
        shape["transform"]["position"] = serde_json::json!([0.0, 0.0]);
        shape["transform"]["scale"] = serde_json::json!([100.0, 100.0]);
        shape["transform"]["rotation"] = serde_json::json!(0.0);
        shape["shape"] = serde_json::json!({
            "path": {"commands": []},
            "ellipse": {"size": size, "position": [11.0, -8.0]},
            "fills": [{"paint": {"type": "solid", "color": [0.0, 1.0, 1.0, 1.0]}}]
        });
        shape
    }

    fn static_bounds(value: serde_json::Value) -> Result<Option<Bounds>, &'static str> {
        let layer: Layer = serde_json::from_value(value).unwrap();
        layer_bounds(
            &layer,
            &BTreeMap::new(),
            fx_schema::Dimensions {
                width: 320,
                height: 240,
            },
        )
    }

    #[test]
    fn static_ellipse_bounds_follow_center_and_edited_extent() {
        for (size, min, max) in [
            ([82.0, 54.0], [-30.0, -35.0], [52.0, 19.0]),
            ([126.0, 70.0], [-52.0, -43.0], [74.0, 27.0]),
        ] {
            let bounds = static_bounds(static_ellipse_value(size)).unwrap().unwrap();
            assert_eq!(bounds.min, min);
            assert_eq!(bounds.max, max);
        }
    }

    #[test]
    fn static_ellipse_bounds_keep_masks_ambiguous_geometry_and_overflow_guarded() {
        let base = static_ellipse_value([82.0, 54.0]);
        let mut masked = base.clone();
        masked["masks"] = serde_json::json!([{"id": 9001, "mode": "add", "layer": 211}]);
        assert_eq!(
            static_bounds(masked).unwrap_err(),
            "Path masks make precomposition render bounds unproved"
        );
        let mut mixed_path = base.clone();
        mixed_path["shape"]["path"]["commands"] = serde_json::json!([
            {"type": "moveTo", "x": 1000.0, "y": 1000.0},
            {"type": "lineTo", "x": 1100.0, "y": 1000.0},
            {"type": "lineTo", "x": 1100.0, "y": 1100.0},
            {"type": "close"}
        ]);
        assert!(static_bounds(mixed_path).is_err());
        let mut ambiguous = base.clone();
        ambiguous["shape"]["polyStar"] = serde_json::json!({});
        assert!(static_bounds(ambiguous.clone()).is_err());
        ambiguous["shape"]
            .as_object_mut()
            .unwrap()
            .remove("ellipse");
        // A lone PolyStar now has independently bounded geometry.
        assert!(static_bounds(ambiguous).is_ok());
        let mut overflow = base;
        overflow["shape"]["ellipse"]["size"] = serde_json::json!([f64::MAX, 54.0]);
        overflow["shape"]["ellipse"]["position"] = serde_json::json!([f64::MAX, -8.0]);
        assert!(static_bounds(overflow).is_err());
    }

    #[test]
    fn origin_conjugation_preserves_world_mapping() {
        let bounds = Bounds {
            min: [-20.2, 10.1],
            max: [80.1, 60.9],
        };
        let shifted = affine_bounds(
            bounds,
            [5.0, 7.0],
            [30.0, 40.0],
            [120.0, 80.0],
            17.0,
            0.0,
            0.0,
        )
        .unwrap();
        assert!(shifted.min[0].is_finite());
        assert!(shifted.max[1].is_finite());
    }

    #[test]
    fn cubic_control_hull_is_a_finite_enclosure() {
        let commands = vec![
            ShapePathCommand::MoveTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: -10.0,
                c1y: 20.0,
                c2x: 30.0,
                c2y: 40.0,
                x: 50.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
        ];
        let bounds = path_bounds(&commands).unwrap();
        assert_eq!(bounds.min, [-10.0, 0.0]);
        assert_eq!(bounds.max, [50.0, 40.0]);
    }
}
