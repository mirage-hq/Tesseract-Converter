//! Sampling-domain proof and bounded native Mosaic approximation.
use std::collections::BTreeMap;

use fx_schema::{
    EffectData, EffectPayload, Layer, LayerData, LayerEffect, LayerId, Position, PropType,
    Transform,
};

use super::{AnimationIndex, hierarchy, media};

/// A checked source sampling-domain decision for one Mosaic owner.
pub(super) struct MosaicDomain(MosaicDomainKind);

enum MosaicDomainKind {
    /// The renderer and native effect both use the unchanged root canvas.
    RootCanvas,
    /// The renderer uses one finite, constant escaped raw-geometry domain.
    Escaped(hierarchy::Bounds),
    /// The source domain cannot be represented without changing authored counts.
    Unsupported(&'static str),
}

/// The structural scope whose backdrop and coordinate ownership were checked.
pub(super) struct MosaicScope {
    structural_parent: Option<LayerId>,
    root: bool,
    cross_layer_inputs: bool,
}

/// Keeps root and nested admission at one call boundary. Root Adjustments need
/// the caller's no-backdrop certificate; nested source stacks do not receive a
/// host backdrop. A flattened inherited transform has a different coordinate
/// owner and therefore cannot use either profile.
pub(super) fn scope(
    inside_precomposition: bool,
    structural_parent: Option<LayerId>,
    inherited_transform: bool,
    root_has_no_backdrop: bool,
    dimensions_match: bool,
    cross_layer_inputs: bool,
) -> Option<MosaicScope> {
    if inherited_transform || !dimensions_match {
        return None;
    }
    match structural_parent {
        Some(parent) => Some(MosaicScope {
            structural_parent: Some(parent),
            root: false,
            cross_layer_inputs,
        }),
        None if !inside_precomposition && root_has_no_backdrop => Some(MosaicScope {
            structural_parent: None,
            root: true,
            cross_layer_inputs,
        }),
        None => None,
    }
}

pub(super) fn classify(
    owner: &Layer,
    siblings: &[Layer],
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    rate: crate::timing::FrameRate,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    scope: &MosaicScope,
) -> MosaicDomain {
    if let Err(reason) = owner_is_eligible(owner, dynamics, canvas, scope) {
        return MosaicDomain(MosaicDomainKind::Unsupported(reason));
    }
    // FX collects consumed sources composition-wide, including references from
    // outside a nested owner's siblings. Do not infer that index here.
    if scope.cross_layer_inputs {
        return MosaicDomain(MosaicDomainKind::Unsupported(
            "composition has cross-layer sampling inputs",
        ));
    }
    if scope.root {
        match prove_root_canvas(owner, siblings, dynamics, canvas, resolved_media) {
            Ok(Some(domain)) => return domain,
            Ok(None) => {}
            Err(reason) => return MosaicDomain(MosaicDomainKind::Unsupported(reason)),
        }
    }
    approximate_escaped_domain(owner, siblings, dynamics, canvas, rate, scope)
}

fn owner_is_eligible(
    owner: &Layer,
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    scope: &MosaicScope,
) -> Result<(), &'static str> {
    let LayerData::Adjustment(adjustment) = owner.data() else {
        return Err("owner is not an Adjustment");
    };
    if owner
        .parent_id()
        .is_some_and(|parent| Some(parent) != scope.structural_parent)
    {
        return Err("owner has an external coordinate parent");
    }
    if adjustment.is_hidden {
        return Err("owner is hidden");
    }
    if adjustment.blend_mode != Default::default()
        || adjustment.transform.opacity.value() != 100.0
        || !matches!(adjustment.transform.position, Position::TwoD(_))
        || !adjustment.masks.is_empty()
        || adjustment.track_matte.is_some()
    {
        return Err("owner has a blend, opacity, 3D, mask, or matte gate");
    }
    if dynamics.for_layer(owner.id()).next().is_some() {
        return Err("owner gate or transform is animated");
    }
    if canvas.width == 0 || canvas.height == 0 || canvas.width > 16_384 || canvas.height > 16_384 {
        return Err("native canvas dimensions are outside the Mosaic domain profile");
    }
    if adjustment.effects.iter().any(|record| match record.data() {
        EffectData::Identified { enabled: false, .. } => false,
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => {
            !matches!(effect, EffectPayload::Known(LayerEffect::Mosaic { .. }))
        }
    }) {
        return Err("owner has another enabled effect stage");
    }
    Ok(())
}

fn prove_root_canvas(
    owner: &Layer,
    siblings: &[Layer],
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
) -> Result<Option<MosaicDomain>, &'static str> {
    let index = siblings
        .iter()
        .position(|layer| layer.id() == owner.id())
        .ok_or("Mosaic owner is absent from its sibling stack")?;
    let mut measurable = false;
    for layer in &siblings[index + 1..] {
        if not_drawn(layer, dynamics) {
            continue;
        }
        // This deliberately excludes clocks/parents/unknown or projected
        // geometry. Native masked support must never substitute for raw paths.
        if layer.parent_id().is_some()
            || dynamics.for_layer(layer.id()).next().is_some()
            || !layer.data().effects().is_empty()
        {
            return Ok(None);
        }
        let transform = match layer.data() {
            LayerData::Rect(rect)
                if !rect.is_hidden
                    && rect.masks.is_empty()
                    && rect.track_matte.is_none()
                    && rect.rect.fill_enabled
                    && !rect.rect.stroke_enabled
                    && rect.rect.roundness == 0.0
                    // Unrounded integer Rect arithmetic is exact at this
                    // canvas range in the renderer's f32 path lowering.
                    && rect.rect.position.into_iter().chain(rect.rect.size)
                        .all(|value| value.is_finite() && value.fract() == 0.0) =>
            {
                &rect.transform
            }
            LayerData::Shape(shape)
                if !shape.is_hidden
                    && shape.masks.is_empty()
                    && shape.track_matte.is_none()
                    && shape.shape.ellipse.is_none()
                    && shape.shape.poly_star.is_none()
                    && shape.shape.round_corners.is_none()
                    && shape.shape.offset_paths.is_none()
                    && shape.shape.trim.is_none()
                    && shape.shape.strokes.is_empty()
                    && !shape.shape.fills.is_empty()
                    && shape
                        .shape
                        .path
                        .commands
                        .iter()
                        .all(|command| command.corner_radius().is_none()) =>
            {
                &shape.transform
            }
            _ => return Ok(None),
        };
        if !identity_geometry(transform) {
            return Ok(None);
        }
        let Some(bounds) =
            hierarchy::all_time_layer_bounds(layer, dynamics, resolved_media, canvas)?
        else {
            continue;
        };
        if bounds
            .min
            .into_iter()
            .chain(bounds.max)
            .any(|value| !value.is_finite())
            || bounds.min[0] < 0.0
            || bounds.min[1] < 0.0
            || bounds.max[0] > f64::from(canvas.width)
            || bounds.max[1] > f64::from(canvas.height)
            || bounds.max[0] <= bounds.min[0]
            || bounds.max[1] <= bounds.min[1]
        {
            return Ok(None);
        }
        measurable = true;
    }
    Ok(measurable.then_some(MosaicDomain(MosaicDomainKind::RootCanvas)))
}

fn approximate_escaped_domain(
    owner: &Layer,
    siblings: &[Layer],
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    rate: crate::timing::FrameRate,
    scope: &MosaicScope,
) -> MosaicDomain {
    let unsupported = |reason| MosaicDomain(MosaicDomainKind::Unsupported(reason));
    let Some(index) = siblings.iter().position(|layer| layer.id() == owner.id()) else {
        return unsupported("owner is absent from its sibling stack");
    };
    let bounds = match hierarchy::mosaic_constant_raw_geometry_in_interval(
        &siblings[index + 1..],
        owner.active_range(),
        dynamics,
        canvas,
        rate,
        scope.structural_parent,
    ) {
        Ok(Some(bounds)) => bounds,
        Ok(None) => return unsupported("lower stack has no drawn measurable raw geometry"),
        Err(reason) => return unsupported(reason),
    };
    let extent = [bounds.max[0] - bounds.min[0], bounds.max[1] - bounds.min[1]];
    if extent
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return unsupported("raw-geometry domain has no finite positive extent");
    }
    if bounds.min[0] >= 0.0
        && bounds.min[1] >= 0.0
        && bounds.max[0] <= f64::from(canvas.width)
        && bounds.max[1] <= f64::from(canvas.height)
    {
        return unsupported("raw-geometry domain stays inside the native canvas");
    }
    if extent == [f64::from(canvas.width), f64::from(canvas.height)] {
        return unsupported(
            "escaped domain differs only by grid origin, which native Mosaic cannot represent",
        );
    }
    MosaicDomain(MosaicDomainKind::Escaped(bounds))
}

/// Reject uncertain source consumption once for the whole composition, even
/// when a referencer is hidden or its shader cannot render.
pub(super) fn has_cross_layer_inputs(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| {
        let matte = match layer.data() {
            LayerData::Media(value) => value.track_matte.is_some(),
            LayerData::Text(value) => value.track_matte.is_some(),
            LayerData::Video(value) => value.track_matte.is_some(),
            LayerData::Image(value) => value.track_matte.is_some(),
            LayerData::Rect(value) => value.track_matte.is_some(),
            LayerData::Shape(value) => value.track_matte.is_some(),
            LayerData::Group(value) => value.track_matte.is_some(),
            LayerData::BooleanOperation(value) => value.track_matte.is_some(),
            LayerData::Adjustment(value) => value.track_matte.is_some(),
            LayerData::Pag(_) | LayerData::Audio(_) | LayerData::AiEdit(_) => false,
        };
        matte
            || super::mask_and_transform(layer).is_some_and(|(_, masks, text_path)| {
                text_path.is_some() || masks.iter().any(|mask| mask.layer.is_some())
            })
            || layer.data().effects().iter().any(|record| {
                let effect = match record.data() {
                    EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
                };
                matches!(
                    effect,
                    EffectPayload::Known(LayerEffect::CustomShader { .. })
                )
            })
            || layer.child_layers().is_some_and(has_cross_layer_inputs)
    })
}

/// Identifies hidden, unpainted or provably empty static inputs.
/// Empty paths with generators/modifiers or ShapePath animators need analysis;
/// so do animated Rect strokes whose static base width is zero.
pub(super) fn not_drawn(layer: &Layer, dynamics: &AnimationIndex<'_>) -> bool {
    match layer.data() {
        LayerData::Rect(rect) => {
            rect.is_hidden
                || (!rect.rect.fill_enabled
                    && (!rect.rect.stroke_enabled
                        || rect.rect.stroke_color.is_none()
                        || (rect.rect.stroke_width.value() <= 0.0
                            && !dynamics.for_layer(rect.id).any(|entry| {
                                entry.target.as_property().is_some_and(|target| {
                                    target.property_type() == PropType::StrokeWidth
                                })
                            }))))
        }
        LayerData::Shape(shape) => {
            shape.is_hidden
                || (shape.shape.path.commands.is_empty()
                    && shape.shape.ellipse.is_none()
                    && shape.shape.poly_star.is_none()
                    && shape.shape.round_corners.is_none()
                    && shape.shape.offset_paths.is_none()
                    && shape.shape.trim.is_none()
                    && !dynamics.for_layer(shape.id).any(|entry| {
                        entry
                            .target
                            .as_property()
                            .is_some_and(|target| target.property_type() == PropType::ShapePath)
                    }))
                || (shape.shape.fills.is_empty()
                    && !shape.shape.strokes.iter().any(|stroke| stroke.enabled))
        }
        LayerData::Group(group) => group.is_hidden,
        _ => false,
    }
}

fn identity_geometry(transform: &Transform) -> bool {
    transform.anchor_point == [0.0; 2]
        && transform.position == fx_schema::Position::TwoD([0.0; 2])
        && transform.scale == [100.0; 2]
        && transform.rotation == 0.0
        && transform.rotation_x == 0.0
        && transform.rotation_y == 0.0
        && transform.orientation == [0.0; 3]
        && transform.skew == 0.0
        && transform.skew_axis == 0.0
}

pub(super) fn lower(
    effect: &LayerEffect,
    native: &mut crate::writer::effects::NativeEffect,
    native_size: [f64; 2],
    domain: Option<&MosaicDomain>,
) -> Option<String> {
    let LayerEffect::Mosaic { sharp_colors, .. } = effect else {
        return None;
    };
    match domain.map(|domain| &domain.0) {
        Some(MosaicDomainKind::RootCanvas) if !sharp_colors => {
            let checkbox = native
                .properties
                .iter_mut()
                .find(|property| property.match_name == "ADBE Mosaic-0003")?;
            checkbox.values[0] = 1.0;
            Some("Sharp Colors set to on for proven raw root canvas-domain FX block-center sampling; counts, keys, bypass and order retained. The saved false checkbox is normalized; native render fidelity remains unmeasured.".into())
        }
        Some(MosaicDomainKind::RootCanvas) => None,
        Some(MosaicDomainKind::Escaped(bounds)) => {
            let extent = [bounds.max[0] - bounds.min[0], bounds.max[1] - bounds.min[1]];
            let scale = [native_size[0] / extent[0], native_size[1] / extent[1]];
            if remap_counts(native, scale) {
                Some(format!(
                    "Mosaic integer block counts scaled by [{:.6}, {:.6}] to approximate cell size over the constant escaped raw-geometry sampling domain [{:.3}, {:.3}]..[{:.3}, {:.3}]. Native Mosaic has no grid-origin control, so negative phase remains unmatched; authored Sharp Colors retained and source center sampling remains unsupported.",
                    scale[0], scale[1], bounds.min[0], bounds.min[1], bounds.max[0], bounds.max[1]
                ))
            } else {
                Some("Mosaic constant escaped raw-geometry domain is finite, but its block-size approximation exceeds the native count controls; authored counts and Sharp Colors retained. Native grid origin and source center sampling remain unsupported.".into())
            }
        }
        Some(MosaicDomainKind::Unsupported(reason)) => Some(format!(
            "Mosaic escaped-domain count approximation not applied: {reason}; authored counts and Sharp Colors retained. Native grid origin and source center sampling remain unsupported."
        )),
        None => Some("Mosaic sampling-domain equivalence is unproved in this native hierarchy/backdrop scope; authored counts and Sharp Colors retained. FX still samples centers even with false; native averaging/grid semantics remain unsupported, not faithful preservation.".into()),
    }
}

fn remap_counts(native: &mut crate::writer::effects::NativeEffect, scale: [f64; 2]) -> bool {
    if scale
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return false;
    }
    let mut adjusted = native.clone();
    for (match_name, scale) in [
        ("ADBE Mosaic-0001", scale[0]),
        ("ADBE Mosaic-0002", scale[1]),
    ] {
        let Some(property) = adjusted
            .properties
            .iter_mut()
            .find(|property| property.match_name == match_name)
        else {
            return false;
        };
        let Some(bounds) = property.bounds else {
            return false;
        };
        let map = |value: f64| {
            let value = (value * scale).round();
            (value.is_finite() && value >= bounds.0 && value <= bounds.1).then_some(value)
        };
        for value in &mut property.values {
            let Some(mapped) = map(*value) else {
                return false;
            };
            *value = mapped;
        }
        if let Some(track) = &mut property.animation {
            for key in &mut track.keys {
                for value in &mut key.values {
                    let Some(mapped) = map(*value) else {
                        return false;
                    };
                    *value = mapped;
                }
            }
        }
    }
    *native = adjusted;
    true
}
