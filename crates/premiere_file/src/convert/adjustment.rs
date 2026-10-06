//! Adjustment layers ↔ FX adjustment layers.
//!
//! Premiere applies an adjustment clip's effects to the composite of every
//! lower video track and replaces that composite with the result, mixed by the
//! clip's Opacity; where an effect leaves the result transparent (a blur edge
//! without Repeat Edge Pixels, a Crop), the sequence shows black, not the
//! untouched composite (measured on the 26.5.1 fixture, G2). The FX adjustment
//! layer folds its effects over the layers below it the same way
//! (`fx_composition::AdjustmentLayer::apply_to_stack`).
//! Default Motion keeps the existing effects and Opacity mapping. Measured
//! static axis-aligned Motion with no effect or full RGB Invert adds a
//! nonpainting rectangle guide and PathMask: Motion changes effect coverage,
//! not the underlying composite's coordinates. The adjustment itself stays at
//! identity; FX ignores its geometry. Export derives that bounded Motion from
//! the current guide, never from cached imported placement values.

use super::{
    background::{black_shape, identity_transform},
    effects::{export_effects, import_effects, invert_levels, report_source_effects, EffectHost},
    nested::{LayerExport, LayerScope},
    premiere_to_tesseract::{
        clip_transform, guide_layer, guide_mask, map_animation_graph_error, scalar_keys,
        set_tracks, tick_range, validate_time_range,
    },
    tesseract_to_premiere::{export_scalar_keys, layer_animations, Guide},
};
use crate::{
    approximate,
    error::{ensure, unsupported, Result},
    export_loss::{omit_field, ExportField, OmissionSink},
    format::{MediaId, PrMedia, PrVideoOccurrence},
    omit,
    schema::{
        adjustment::ADJUSTMENT_LAYER_NAME, MaskBoundary, PrAnimatedProperty, PrBlendMode,
        PrMediaKind, PrPropertyAnimation, PrStaticTransform, PrVideoStream, STILL_INTRINSIC_TICKS,
    },
    Omission, OmissionScope,
};
use fx_schema::{
    animator::AnimationGraph, AdjustmentLayer, BlendMode, EffectData, EffectPayload, FxItemId,
    Layer, LayerData, LayerId, MaskMode, PercentageProperty, PropType, Property, PropertyAnimator,
    Transform,
};

/// The FX adjustment layer of one adjustment occurrence, with its Opacity keys
/// and keyed effect parameters set on `dynamics`. Its transform stays identity
/// at the clip's Opacity. Admitted static Motion coverage is carried by a
/// sibling rectangle guide; effects take the `canvas` of the clip's sequence
/// as their frame, while FX draws them in the document's
/// ([`LayerScope::document_canvas`]), so a Corner Pin converts only where the
/// two are one size ([`import_effects`]).
pub(super) fn import_adjustment(
    clip: &PrVideoOccurrence,
    layer_id: LayerId,
    index: usize,
    canvas: [u32; 2],
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut fx_schema::AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Vec<Layer>> {
    let record = clip.record();
    if clip.stroke.is_some() {
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            "Film Impact Stroke was not imported: adjustment layer is not an opaque physical video",
        );
    }
    let active_range = tick_range(clip.start_ticks, clip.end_ticks)?;
    validate_time_range("active_range", active_range)?;
    let mut transform = identity_transform();
    transform.opacity = PercentageProperty::new(clip.opacity)
        .ok_or_else(|| unsupported("Premiere opacity must be between 0 and 100"))?;
    let mut tracks = Vec::new();
    for animation in &clip.animations {
        ensure!(
            animation.property() == PrAnimatedProperty::Opacity,
            "{record}: an adjustment layer carries Opacity keys only"
        );
        match scalar_keys(animation, clip.in_ticks, layer_id, PropType::Opacity) {
            Ok(keys) => tracks.push((Property::new(layer_id, PropType::Opacity), keys)),
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("Opacity animation was not imported: {error}"),
            ),
        }
    }
    // Premiere uses the sequence canvas; FX draws adjustment effects in
    // the document canvas even inside a nest.
    // Geometry2 acts on the lower picture, not on FX Adjustment geometry.
    // Composite admission below owns its diagnostic and editable group.
    let mut effect_clip = clip.clone();
    for index in (0..effect_clip.effects.len()).rev() {
        if matches!(
            effect_clip.effects[index].params,
            crate::schema::PrEffectParams::AdjustmentGeometry2(_)
        ) {
            effect_clip.remove_effect(index);
        }
    }
    let (effects, effect_tracks) = import_effects(
        &effect_clip,
        layer_id,
        false,
        MaskBoundary::Flat,
        scope.parent.is_some(),
        false,
        crate::schema::PrMediaKind::Adjustment,
        canvas,
        scope.document_canvas,
        canvas,
        scope.effect_ids,
        omissions,
    );
    if !super::effects::retains_coverage(clip, &effects, omissions) {
        return Ok(Vec::new());
    }
    let mut layers = Vec::new();
    let mut masks = Vec::new();
    if clip.transform != PrStaticTransform::default() {
        ensure!(
            crate::schema::adjustment::supports_motion_coverage(clip),
            "{record}: adjustment Motion coverage is unsupported"
        );
        let guide_id = LayerId::new(*scope.next_index as u64 + 1);
        let mask_id = FxItemId::new(*scope.next_index as u64 + 2);
        *scope.next_index += 2;
        let mut rect = black_shape(canvas[0], canvas[1]);
        rect.fill_enabled = false;
        layers.push(Layer::from_data(&LayerData::Rect(guide_layer(
            guide_id,
            format!("Premiere adjustment {} coverage", index + 1),
            scope.parent,
            active_range,
            clip_transform(&clip.transform, clip.opacity, canvas, canvas)?,
            rect,
        )))?);
        masks.push(guide_mask(mask_id, guide_id, 0.0));
    }
    let layer = Layer::from_data(&LayerData::Adjustment(AdjustmentLayer {
        id: layer_id,
        name: format!("Premiere adjustment {}", index + 1),
        description: String::new(),
        is_hidden: !clip.enabled,
        parent: scope.parent,
        // FX composites the effected copy over the untouched composite with
        // this mode, as it does with the Opacity (for Premiere measured for
        // Screen over one Levels adjustment, inferred for other modes).
        blend_mode: clip.blend_mode.fx_mode(),
        track_matte: None,
        masks,
        active_range,
        effects,
        transform,
    }))?;
    set_tracks(dynamics, tracks)?;
    for (target, track) in effect_tracks {
        dynamics
            .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
            .map_err(map_animation_graph_error)?;
    }
    if let Some(warning) = clip.blend_mode.approximation() {
        approximate(omissions, record, warning);
    }
    // Premiere's order for an adjustment's source effects is unmeasured.
    report_source_effects(clip, false, omissions);
    layers.insert(0, layer);
    Ok(layers)
}

/// Shared admission for placement, media inventory and script ownership.
pub(super) fn unexported_reason(
    adjustment: &AdjustmentLayer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> Option<String> {
    coverage_transform(adjustment, layers, dynamics, canvas).err()
}

/// Native Motion changes the effect's coverage, not the picture beneath it.
/// Keep the measured static uniform rectangle + no effect/full RGB Invert form.
fn coverage_transform(
    adjustment: &AdjustmentLayer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> std::result::Result<PrStaticTransform, String> {
    if adjustment.track_matte.is_some() {
        return Err("a track matte on an adjustment layer is not exported".to_owned());
    }
    let mask = match adjustment.masks.as_slice() {
        [] => return Ok(PrStaticTransform::default()),
        [mask] => mask,
        _ => return Err("masks on an adjustment layer are not exported".to_owned()),
    };
    if mask.mode != MaskMode::Add
        || mask.inverted
        || mask.opacity.value() != 1.0
        || mask.expansion != 0.0
        || mask.feather != [0.0; 2]
        || mask.legacy_path.is_some()
        || dynamics
            .entries()
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return Err("masks on an adjustment layer are not exported".to_owned());
    }
    let guide = mask
        .layer
        .and_then(|id| layers.iter().find(|layer| layer.id() == id))
        .and_then(|layer| match layer.data() {
            LayerData::Rect(rect) => Some(rect),
            _ => None,
        })
        .ok_or_else(|| "masks on an adjustment layer are not exported".to_owned())?;
    if let Some(reason) =
        Guide::Rect(guide).unsupported("adjustment", adjustment.parent, adjustment.active_range)
    {
        return Err(reason);
    }
    let t = &guide.transform;
    if guide.rect.roundness != 0.0
        || t.position.z().is_some_and(|z| z != 0.0)
        || t.rotation != 0.0
        || t.skew != 0.0
        || t.rotation_x != 0.0
        || t.rotation_y != 0.0
        || t.orientation != [0.0; 3]
        || t.opacity.value() != 100.0
        || layer_animations(dynamics, guide.id).next().is_some()
    {
        return Err("adjustment coverage requires a static, unrounded 2D rectangle without rotation, skew or opacity".to_owned());
    }
    if adjustment.blend_mode != BlendMode::Normal
        || adjustment.transform.opacity.value() != 100.0
        || layer_animations(dynamics, adjustment.id)
            .any(|(property, _)| property == PropType::Opacity)
        || adjustment.effects.iter().any(|effect| {
            let (id, enabled, payload) = match effect.data() {
                EffectData::Identified {
                    id,
                    enabled,
                    effect,
                    ..
                } => (Some(*id), *enabled, effect),
                EffectData::Legacy(effect) => (None, true, effect),
            };
            enabled
                && (payload != &EffectPayload::Known(invert_levels(0.0))
                    || id.is_some_and(|id| {
                        dynamics
                            .entries()
                            .iter()
                            .any(|entry| entry.target.effect_id() == Some(id))
                    }))
        })
    {
        return Err("adjustment coverage requires static Opacity 100, Normal blend, and only static full RGB Invert or no active effect".to_owned());
    }
    let frame = canvas.map(f64::from);
    let scale = [0, 1].map(|axis| guide.rect.size[axis] * t.scale[axis] / frame[axis]);
    if frame.contains(&0.0)
        || guide.rect.size.iter().any(|size| *size <= 0.0)
        || t.scale.iter().any(|scale| *scale <= 0.0)
        || scale.iter().any(|scale| !scale.is_finite())
        || (scale[0] - scale[1]).abs() > f64::EPSILON * 16.0 * scale[0].max(scale[1])
    {
        return Err(
            "adjustment coverage requires positive uniform canvas-relative scale".to_owned(),
        );
    }
    let position = t.position.xy_array();
    let center = [0, 1].map(|axis| {
        (position[axis]
            + (guide.rect.position[axis] + guide.rect.size[axis] / 2.0 - t.anchor_point[axis])
                * t.scale[axis]
                / 100.0)
            / frame[axis]
    });
    if center.iter().any(|value| !value.is_finite()) {
        return Err("adjustment coverage has a nonfinite position".to_owned());
    }
    Ok(PrStaticTransform {
        position: center,
        scale: [scale[0]; 2],
        ..PrStaticTransform::default()
    })
}

/// Whether `transform` moves no pixel: FX ignores an adjustment layer's
/// geometry, and only such a transform has Premiere's default Motion as its
/// exact counterpart.
fn moves_nothing(transform: &Transform) -> bool {
    transform.position.xy_array() == transform.anchor_point
        && transform.position.z().is_none_or(|z| z == 0.0)
        && transform.scale == [100.0; 2]
        && transform.rotation == 0.0
        && transform.skew == 0.0
        && transform.rotation_x == 0.0
        && transform.rotation_y == 0.0
        && transform.orientation == [0.0; 3]
}

/// Map an FX adjustment layer to an adjustment clip of the shared Black Video
/// generator media on the export grid, or `None` when it has no adjustment
/// clip; that layer is then omitted with the precise reason. The placement
/// starts at the generator in-point of the sequence rate, as a Color Matte
/// does. Effects export through the clip path with the identity as their host,
/// because the layer's own geometry never moves the composite. A supported
/// coverage guide supplies Motion independently of that geometry.
pub(super) fn export_adjustment_layer(
    adjustment: &AdjustmentLayer,
    layers: &[Layer],
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Option<(PrVideoOccurrence, PrMedia)>> {
    let (width, height, frame_rate) = (context.width, context.height, context.frame_rate);
    let transform = match coverage_transform(adjustment, layers, context.dynamics, [width, height])
    {
        Ok(transform) => transform,
        Err(reason) => {
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                format!("adjustment layer was not exported: {reason}"),
            );
            return Ok(None);
        }
    };
    if adjustment.parent.is_none()
        && layers.windows(2).any(|pair| {
            pair[0].id() == adjustment.id
                && matches!(pair[1].data(), LayerData::Group(group)
                    if super::adjustment_geometry::is_stage(group)
                        && group.parent.is_none()
                        && pair[1].active_range() == adjustment.active_range)
        })
    {
        approximate(
            omissions,
            record,
            super::adjustment_geometry::ROOT_ADJUSTMENT_APPROXIMATION,
        );
    }
    let media = MediaId(format!("adjustment-layer:{width}x{height}"));
    if context.media_facts.contains_key(media.as_str()) {
        return Err(unsupported(format!(
            "asset {media} names both a packaged asset and an adjustment layer"
        )));
    }
    if !adjustment.description.is_empty() {
        omit_field(
            omissions,
            adjustment.id,
            ExportField::Description,
            record,
            "description was not exported",
        );
    }
    if !moves_nothing(&adjustment.transform) {
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            if adjustment.masks.is_empty() {
                "adjustment layer Motion was not exported: FX ignores an adjustment layer's geometric transform, which has no Premiere counterpart; default Motion was written"
            } else {
                "adjustment layer Motion was not exported: FX ignores its geometric transform; the current coverage guide supplied native Motion"
            },
        );
    }
    let active_end = adjustment
        .active_range
        .start
        .checked_add_duration(adjustment.active_range.duration)
        .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
    let start_ticks = context.frame_ticks(adjustment.active_range.start, "activeRange.start")?;
    let end_ticks = context.picture_end_ticks(active_end, None)?;
    ensure!(
        end_ticks > start_ticks,
        "activeRange {}..{} ms collapses to zero duration on the {frame_rate} sequence grid",
        adjustment.active_range.start.as_millis(),
        active_end.as_millis()
    );
    let in_ticks = frame_rate.generator_in_ticks();
    let out_ticks = in_ticks
        .checked_add(end_ticks - start_ticks)
        .filter(|out| *out <= STILL_INTRINSIC_TICKS)
        .ok_or_else(|| unsupported("adjustment layer outlasts the Black Video generator"))?;
    let mut animations = Vec::new();
    let mut tracks = context
        .property_tracks
        .remove(&adjustment.id)
        .unwrap_or_default();
    if let Some(opacity) = tracks.remove(&PropType::Opacity) {
        match export_scalar_keys(opacity, in_ticks, "Opacity", omissions, record) {
            Ok(keys) => animations.push(PrPropertyAnimation::Opacity(keys)),
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("Opacity animation was not exported: {error}"),
            ),
        }
    }
    for property in tracks.into_keys() {
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!(
                "{property:?} animation was not exported: FX ignores an adjustment layer's geometric transform, which has no Premiere counterpart"
            ),
        );
    }
    // The caller places the adjustment clip, which writes these keys.
    context
        .written
        .record_animations(adjustment.id, &animations);
    let host_transform = identity_transform();
    let effects = export_effects(
        &adjustment.effects,
        context.dynamics,
        EffectHost {
            layer: adjustment.id,
            still: false,
            staged: false,
            nested: context.in_moved_nest,
            in_nest: context.depth > 0,
            transform: &host_transform,
            source_in: in_ticks,
            video_keys: None,
            static_parameters_reason: None,
            frame: [width, height],
            canvas: [width, height],
        },
        context.written,
        record,
        omissions,
    );
    // Only the coverage guide supplies Motion; adjustment geometry is ignored.
    let occurrence = PrVideoOccurrence {
        transform,
        opacity: adjustment.transform.opacity.value(),
        blend_mode: PrBlendMode::from_fx_mode(adjustment.blend_mode),
        animations,
        enabled: !adjustment.is_hidden,
        effects,
        ..PrVideoOccurrence::unedited(media, start_ticks..end_ticks, in_ticks..out_ticks)
    };
    let media = PrMedia {
        name: ADJUSTMENT_LAYER_NAME.to_owned(),
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: Some(PrVideoStream {
            pixel_aspect: Default::default(),
            interpretation: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks: STILL_INTRINSIC_TICKS,
            frame_rate: frame_rate.into(),
            width,
            height,
            kind: PrMediaKind::Adjustment,
        }),
        audio: None,
    };
    if let Some(warning) = PrBlendMode::export_approximation(adjustment.blend_mode) {
        approximate(omissions, record, warning);
    }
    Ok(Some((occurrence, media)))
}

#[cfg(test)]
#[path = "tests/adjustment.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/adjustment_geometry.rs"]
mod geometry_tests;
