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
//! identity; FX ignores its geometry. Masked adjustment export remains omitted.

use super::{
    background::{black_shape, identity_transform},
    effects::{export_effects, import_effects, EffectHost},
    nested::{LayerExport, LayerScope},
    premiere_to_tesseract::{
        clip_transform, guide_layer, guide_mask, map_animation_graph_error, scalar_keys,
        set_tracks, tick_range, validate_time_range,
    },
    tesseract_to_premiere::export_scalar_keys,
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
    AdjustmentLayer, FxItemId, Layer, LayerData, LayerId, PercentageProperty, PropType, Property,
    PropertyAnimator, Transform,
};

/// The FX adjustment layer of one adjustment occurrence, with its Opacity keys
/// and keyed effect parameters set on `dynamics`. Its transform stays identity
/// at the clip's Opacity. Admitted static Motion coverage is carried by a
/// sibling rectangle guide; effects take the canvas as their frame.
pub(super) fn import_adjustment(
    clip: &PrVideoOccurrence,
    layer_id: LayerId,
    index: usize,
    canvas: [u32; 2],
    scope: &mut LayerScope<'_, '_, '_>,
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
    let (effects, effect_tracks) = import_effects(
        clip,
        layer_id,
        MaskBoundary::Flat,
        canvas,
        canvas,
        scope.effect_ids,
        omissions,
    );
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
    layers.insert(0, layer);
    Ok(layers)
}

/// Why an FX adjustment layer has no adjustment clip, if it has none: FX
/// gates a masked or matted adjustment's effect by that gate, and Premiere
/// has no such gate on an adjustment clip. Its blend mode exports as the
/// clip's ([`PrBlendMode::from_fx_mode`]).
pub(super) fn unexported_reason(adjustment: &AdjustmentLayer) -> Option<&'static str> {
    [
        (
            !adjustment.masks.is_empty(),
            "masks on an adjustment layer are not exported",
        ),
        (
            adjustment.track_matte.is_some(),
            "a track matte on an adjustment layer is not exported",
        ),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then_some(reason))
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
/// because the written Motion is Premiere's default whatever the layer's
/// geometric transform.
pub(super) fn export_adjustment_layer(
    adjustment: &AdjustmentLayer,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Option<(PrVideoOccurrence, PrMedia)>> {
    if let Some(reason) = unexported_reason(adjustment) {
        omit(
            omissions,
            OmissionScope::Occurrence,
            record,
            format!("adjustment layer was not exported: {reason}"),
        );
        return Ok(None);
    }
    let (width, height, frame_rate) = (context.width, context.height, context.frame_rate);
    let media = MediaId("adjustment-layer".to_owned());
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
            "adjustment layer Motion was not exported: FX ignores an adjustment layer's geometric transform, which has no Premiere counterpart; default Motion was written",
        );
    }
    let active_end = adjustment
        .active_range
        .start
        .checked_add_duration(adjustment.active_range.duration)
        .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
    let start_ticks = context.frame_ticks(adjustment.active_range.start, "activeRange.start")?;
    let end_ticks = context.frame_ticks(active_end, "activeRange.end")?;
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
            staged: false,
            nested: context.in_moved_nest,
            transform: &host_transform,
            source_in: in_ticks,
            frame: [width, height],
            canvas: [width, height],
        },
        context.written,
        record,
        omissions,
    );
    // Its Motion is Premiere's default whatever the layer's transform.
    let occurrence = PrVideoOccurrence {
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
