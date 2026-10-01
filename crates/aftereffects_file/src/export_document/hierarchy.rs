//! Strict native Null-parent and finite-canvas precomposition lowering.
//!
//! This helper classifies current FX Groups only. Registration and dispatch
//! remain in the shared export module so sibling-local rollback stays central.

mod animated_bounds;
mod collapsed;
#[cfg(test)]
mod collapsed_tests;
mod demand;
mod effect_support;
mod root_viewport;
mod skew;

#[cfg(test)]
mod demand_tests;

#[cfg(test)]
mod skew_tests;

#[cfg(test)]
mod root_viewport_tests;

use std::collections::BTreeMap;

use fx_schema::{
    GroupLayer, Layer, LayerData, LayerId, Position, ShapeLineJoin, Time, Transform,
    animator::AnimationGraphEntry, layer::ShapePathCommand,
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

/// A bounded output canvas, never permission to crop child effect inputs.
/// Consumer viewports are a diagnosed approximation for otherwise oversized 3D sources.
pub(super) struct CertifiedCanvas {
    bounds: Bounds,
    root_output: bool,
    consumer_3d: bool,
}

pub(super) struct ChildDemand {
    propagated: Demand,
    finite_canvas: Option<CertifiedCanvas>,
}

impl ChildDemand {
    pub(super) fn use_root_output_viewport(
        &mut self,
        group: &GroupLayer,
        dynamics: &[AnimationGraphEntry],
        siblings: &[Layer],
        canvas: fx_schema::Dimensions,
    ) -> bool {
        if let Some(viewport) = root_viewport::canvas(group, dynamics, siblings, canvas) {
            self.finite_canvas = Some(viewport);
            return true;
        }
        false
    }

    /// Certify finite geometric output support, not pixel-exact native rasterization.
    pub(super) fn use_3d_consumer_viewport(
        &mut self,
        group: &GroupLayer,
        siblings: &[Layer],
        dynamics: &[AnimationGraphEntry],
    ) -> Result<(), &'static str> {
        if group.motion_blur
            || !group.masks.is_empty()
            || group.track_matte.is_some()
            || !group.fills.is_empty()
            || !super::playback_is_identity(&group.playback)
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
        });
        Ok(())
    }

    pub(super) const fn finite_canvas(&self) -> Option<&CertifiedCanvas> {
        self.finite_canvas.as_ref()
    }

    pub(super) const fn propagated(&self) -> &Demand {
        &self.propagated
    }
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

/// Reverse occurrence operators while retaining root-time/local-time coupling.
/// Native precomposition sampling at a new crop boundary has no independent
/// support proof yet, so inferred demand never replaces full bounds.
pub(super) fn child_demand(
    source: &GroupLayer,
    _geometry: &GroupLayer,
    masks: &[fx_schema::layer::PathMask],
    dynamics: &[AnimationGraphEntry],
    canvas: fx_schema::Dimensions,
    inherited: &Demand,
) -> ChildDemand {
    let mut propagated = inherited.clone();
    let active_range = source.playback.input_range();
    propagated.active(
        active_range.start.as_millis(),
        active_range.end().as_millis(),
    );
    if let Err(reason) = effect_support::stack(&source.effects) {
        propagated.full(reason);
    }
    if !masks.is_empty() {
        match finite_mask_gate(masks, dynamics) {
            Ok(_gate) => propagated.full("Static Group Add mask is structurally finite, but native crop-boundary support is not independently proved"),
            Err(reason) => propagated.full(reason),
        }
    }
    if source.track_matte.is_some() {
        propagated.full("Group matte source has no proved crop support");
    }
    if super::playback_is_identity(&source.playback) {
        propagated.map_clock(active_range.start.as_millis(), 1.0);
    } else {
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
    ChildDemand {
        propagated,
        finite_canvas: None,
    }
}

/// Necessary, not sufficient, proof of a finite source-local mask enclosure.
/// The actual native/source support comparison must authorize use as a crop.
fn finite_mask_gate(
    masks: &[fx_schema::layer::PathMask],
    dynamics: &[AnimationGraphEntry],
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
    dynamics: &[AnimationGraphEntry],
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
    )
}

pub(super) fn classify_with_demand(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &[AnimationGraphEntry],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    finite_canvas: Option<&CertifiedCanvas>,
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
    )
}

/// Selects the exact two-transform representation for one static planar skew.
/// The caller owns collision-free allocation of `helper_id`.
#[cfg(test)]
pub(super) fn classify_with_skew_helper(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &[AnimationGraphEntry],
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
    dynamics: &[AnimationGraphEntry],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    finite_canvas: Option<&CertifiedCanvas>,
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
    dynamics: &[AnimationGraphEntry],
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
    )
}

pub(super) fn classify_precomposition_with_demand(
    group: &GroupLayer,
    composition_end: Time,
    duration: Duration24,
    dynamics: &[AnimationGraphEntry],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    finite_canvas: Option<&CertifiedCanvas>,
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
    dynamics: &[AnimationGraphEntry],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
    force_precomposition: bool,
    skew_helper_id: Option<LayerId>,
    finite_canvas: Option<&CertifiedCanvas>,
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
    // Imported vector control groups can be hidden and childless. A disabled
    // Null retains their editable controls without inventing a render canvas.
    let empty_hidden_controls = !force_precomposition
        && group.is_hidden
        && group.layers.is_empty()
        && group.effects.is_empty()
        && group.track_matte.is_none();
    if empty_hidden_controls
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
    } else if spatial
        || subtree_has_masks(&group.layers)
        || subtree_has_dynamics(&group.layers, dynamics)
    {
        animated_bounds::child_union(group, dynamics, resolved_media, canvas)
    } else {
        child_union(group, resolved_media, canvas)
    };
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
        Err(error) => return Err(error),
    };
    // Retain the original support/near-plane validation above. A final output
    // viewport is not permission to accept unsafe or unknown child geometry.
    let root_camera = crate::writer::NativeCameraSpec::root(canvas.width, canvas.height);
    // Do not change an already representable source's raster domain. Only rescue
    // sources whose full symmetric 3D canvas would otherwise be omitted.
    let finite_canvas = finite_canvas.filter(|certificate| {
        !certificate.consumer_3d
            || (spatial
                && (0..2).any(|axis| {
                    let radius = (root_camera.center[axis] - bounds.min[axis])
                        .max(bounds.max[axis] - root_camera.center[axis])
                        .ceil()
                        .max(1.0);
                    radius * 2.0 > f64::from(u16::MAX)
                }))
    });
    let bounds = finite_canvas.map_or(bounds, |certified| certified.bounds);
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
            return Err(
                "Proven precomposition bounds exceed the native canvas; subtree is not a certified collapsed 2D vector source",
            );
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
fn text_only_branch(layer: &Layer) -> bool {
    match layer.data() {
        LayerData::Text(_) => true,
        LayerData::Group(group) => {
            !group.layers.is_empty() && group.layers.iter().all(text_only_branch)
        }
        _ => false,
    }
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
    dynamics: &[AnimationGraphEntry],
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

fn subtree_needs_projection(layers: &[Layer], dynamics: &[AnimationGraphEntry]) -> bool {
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

fn subtree_has_dynamics(layers: &[Layer], dynamics: &[AnimationGraphEntry]) -> bool {
    layers.iter().any(|layer| {
        dynamics
            .iter()
            .any(|entry| entry.target.layer_id() == Some(layer.id()))
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
    dynamics: &[AnimationGraphEntry],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: fx_schema::Dimensions,
) -> Result<Option<Bounds>, &'static str> {
    if subtree_has_dynamics(std::slice::from_ref(layer), dynamics) {
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
            if shape.shape.ellipse.is_some()
                || shape.shape.poly_star.is_some()
                || shape.shape.path.commands.is_empty()
            {
                return Err(
                    "Only checked static Path geometry has a proved Shape render enclosure",
                );
            }
            let mut bounds = path_bounds(&shape.shape.path.commands)?;
            let offset_reach = shape
                .shape
                .offset_paths
                .map_or(0.0, |offset| offset.amount.abs());
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
            bounds = bounds.expand(offset_reach + stroke_reach)?;
            transform_bounds(bounds, &shape.transform).map(Some)
        }
        LayerData::Group(group) => {
            if group.is_hidden {
                return Ok(None);
            }
            check_static_nested_group(group)?;
            let Some(bounds) = child_union(group, resolved_media, canvas)? else {
                return Ok(None);
            };
            let bounds = collapsed::mask_output(group, &[]).unwrap_or(bounds);
            transform_bounds(bounds, &group.transform).map(Some)
        }
        LayerData::Image(_) | LayerData::Video(_) => {
            let request = media::request(layer).ok_or("Visual media has no archive request")?;
            let source = resolved_media
                .get(request.asset_id.as_str())
                .ok_or("Visual media archive source was not resolved for bounds")?;
            let spec = media::lower(layer, source, canvas)?;
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

fn path_bounds(commands: &[ShapePathCommand]) -> Result<Bounds, &'static str> {
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
