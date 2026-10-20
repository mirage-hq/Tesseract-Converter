//! A coverage-changing adjustment Wipe or Crop masks the lower composite,
//! not just the mix between an effected and an untouched copy.
use super::{
    background::{black_shape, identity_transform, plain_group},
    nested::LayerScope,
    premiere_to_tesseract::{
        guide_layer, guide_mask, keyframe_id, linear_wipe_guide, set_tracks, tick_range,
    },
};
use crate::{
    error::Result,
    schema::{PrSequence, PrVideoOccurrence, MAX_NEST_DEPTH},
    {omit, Omission, OmissionScope},
};
use fx_schema::{
    animator::{PropertyKeyframe, PropertyKeyframeTrack},
    AnimationGraph, FxItemId, Layer, LayerData, LayerId, Property, PropertyKeyframeEasing,
    PropertyValue, TimeOffset,
};

#[derive(Default)]
struct Dependencies {
    owned: std::collections::BTreeSet<LayerId>,
    referenced: std::collections::BTreeSet<LayerId>,
    nesting_depth: usize,
}

// Descendant guides belong to their subtree, not to the outer sibling list.
fn dependencies(layers: &[Layer]) -> Dependencies {
    let mut result = Dependencies::default();
    for layer in layers {
        result.owned.insert(layer.id());
        let (matte, masks, children): (
            Option<&fx_schema::TrackMatte>,
            &[fx_schema::PathMask],
            &[Layer],
        ) = match layer.data() {
            LayerData::Video(v) => (v.track_matte.as_ref(), &v.masks, &[]),
            LayerData::Image(v) => (v.track_matte.as_ref(), &v.masks, &[]),
            LayerData::Media(v) => (v.track_matte.as_ref(), &v.masks, &[]),
            LayerData::Rect(v) => (v.track_matte.as_ref(), &v.masks, &[]),
            LayerData::Shape(v) => (v.track_matte.as_ref(), &v.masks, &[]),
            LayerData::Text(v) => (v.track_matte.as_ref(), &v.masks, &[]),
            LayerData::Group(v) => (v.track_matte.as_ref(), &v.masks, &v.layers),
            LayerData::Adjustment(v) => (v.track_matte.as_ref(), &v.masks, &[]),
            LayerData::BooleanOperation(v) => (v.track_matte.as_ref(), &v.masks, &v.layers),
            LayerData::AiEdit(v) => (None, &[], &v.layers),
            LayerData::Audio(_) | LayerData::Pag(_) => (None, &[], &[]),
        };
        result.referenced.extend(matte.map(|matte| matte.layer));
        result
            .referenced
            .extend(masks.iter().filter_map(|mask| mask.layer));
        let child = dependencies(children);
        // Conservatively include staging groups too; adding a Wipe must not
        // make native export discard an existing picture subtree at its limit.
        result.nesting_depth = result
            .nesting_depth
            .max(child.nesting_depth + usize::from(matches!(layer.data(), LayerData::Group(_))));
        result.owned.extend(child.owned);
        result.referenced.extend(child.referenced);
    }
    result
}

/// Why wrapping would separate a matte/mask dependency or exceed native nest depth.
pub(super) fn boundary_reason(
    lower: &[Layer],
    upper: &[Layer],
    depth: usize,
) -> Option<&'static str> {
    let lower = dependencies(lower);
    let upper = dependencies(upper);
    if !lower.referenced.is_disjoint(&upper.owned) || !upper.referenced.is_disjoint(&lower.owned) {
        Some("a mask crosses its lower-composite boundary")
    } else if depth + 1 + lower.nesting_depth > MAX_NEST_DEPTH {
        Some("the wrapper would exceed the native nesting limit")
    } else {
        None
    }
}

/// Dispatch bottom-up Geometry2 staging first, then wrap a supported Wipe while
/// keeping existing layer clocks under a full-sequence group. Its guide opens
/// outside the adjustment interval; within it, the wipe clips the composite.
/// Export uses the ordinary editable nest-and-Wipe route, not an opaque replay.
pub(super) fn wrap(
    project: &PrSequence,
    pending: &[(LayerId, &PrVideoOccurrence)],
    scope: &mut LayerScope<'_, '_>,
    layers: &mut Vec<Layer>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<()> {
    for &(adjustment_id, clip) in pending.iter().rev() {
        super::adjustment_geometry::wrap(
            project,
            adjustment_id,
            clip,
            scope,
            layers,
            dynamics,
            omissions,
        )?;
        let Some(position) = layers.iter().position(|layer| layer.id() == adjustment_id) else {
            continue;
        };
        let crop =
            !clip.crop.is_default() && crate::schema::adjustment::supports_crop_coverage(clip);
        if clip.linear_wipe.is_none() && !crop {
            continue;
        }
        let feature = if crop { "Crop" } else { "Wipe" };
        // A following effect stays above the masked lower composite; preceding
        // effects stay inside it. Mixed stacks require a separate effect split.
        let boundary =
            position + usize::from(clip.effects_above_mask == 0 && !clip.effects.is_empty());
        let reason = boundary_reason(
            &layers[boundary..],
            &layers[..boundary],
            scope.nesting_depth,
        );
        if let Some(reason) = reason {
            omit(omissions, OmissionScope::Feature, clip.record(), format!("adjustment {feature} not converted: {reason}; the existing layer stack is preserved"));
            continue;
        }
        let range = tick_range(0, project.end_ticks())?;
        let window = tick_range(clip.start_ticks, clip.end_ticks)?;
        let start = window.start.as_millis();
        let end = start + window.duration.as_millis();
        let at_sequence_end = end == range.start.as_millis() + range.duration.as_millis();
        let group_id = LayerId::new(*scope.next_index as u64 + 1);
        let guide_id = LayerId::new(*scope.next_index as u64 + 2);
        let mask_id = FxItemId::new(*scope.next_index as u64 + 3);
        let canvas = [project.width, project.height];
        let prepared = if let Some(wipe) = &clip.linear_wipe {
            (|| {
                let (mut transform, property, native) =
                    linear_wipe_guide(wipe, clip.in_ticks, guide_id, canvas)?;
                let mut open = wipe.clone();
                open.initial_completion = 0.0;
                open.completion.clear();
                let (full_transform, _, _) =
                    linear_wipe_guide(&open, clip.in_ticks, guide_id, canvas)?;
                let axis = usize::from(property == fx_schema::PropType::ScaleY);
                let full = full_transform.scale[axis];
                let mut keys = Vec::with_capacity(native.keyframes().len() + 3);
                if start != 0 {
                    keys.push(PropertyKeyframe::new(
                        keyframe_id(guide_id, "adjustment-wipe-boundary", 0),
                        TimeOffset::ZERO,
                        PropertyValue::Float(full),
                        PropertyKeyframeEasing::Hold,
                    ));
                    transform.scale[axis] = full;
                }
                // Native keyed controls hold the first value back to In.
                if let Some(first) = native
                    .keyframes()
                    .first()
                    .filter(|key| key.layer_time().as_millis() > 0)
                {
                    keys.push(PropertyKeyframe::new(
                        keyframe_id(guide_id, "adjustment-wipe-start", 0),
                        TimeOffset::from_millis(start as i64),
                        first.value().clone(),
                        PropertyKeyframeEasing::Hold,
                    ));
                }
                for (index, key) in native.keyframes().iter().enumerate() {
                    let time = start as i64 + key.layer_time().as_millis();
                    crate::error::ensure!(
                        time >= start as i64 && (at_sequence_end || time < end as i64),
                        "adjustment Wipe keys collide with the placement boundary"
                    );
                    keys.push(PropertyKeyframe::new(
                        key.id().clone(),
                        TimeOffset::from_millis(time),
                        key.value().clone(),
                        if index == 0 {
                            PropertyKeyframeEasing::Hold
                        } else {
                            key.easing()
                        },
                    ));
                }
                // Terminal support keys retain the incoming curve beyond Out.
                let supporting_tail = native
                    .keyframes()
                    .last()
                    .is_some_and(|key| start as i64 + key.layer_time().as_millis() >= end as i64);
                if !at_sequence_end || !supporting_tail {
                    keys.push(PropertyKeyframe::new(
                        keyframe_id(guide_id, "adjustment-wipe-boundary", 1),
                        TimeOffset::from_millis(end as i64),
                        PropertyValue::Float(full),
                        PropertyKeyframeEasing::Hold,
                    ));
                }
                let track = PropertyKeyframeTrack::new(keys).map_err(|error| {
                    crate::error::unsupported(format!("adjustment Wipe guide keys: {error}"))
                })?;
                Ok(CoverageGuide {
                    transform,
                    tracks: vec![(Property::new(guide_id, property), track)],
                    feather: wipe.feather,
                })
            })()
        } else {
            crop_guide(clip, guide_id, canvas, start, end)
        };
        let CoverageGuide {
            transform,
            tracks,
            feather,
        } = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                omit(omissions, OmissionScope::Feature, clip.record(), format!("adjustment {feature} not converted: {error}; the existing layer stack is preserved"));
                continue;
            }
        };
        *scope.next_index += 3;
        let mut children = layers.split_off(boundary);
        // Preserve existing guide IDs and child ownership. The wrapper has
        // identity geometry and clock, so reference coordinates do not change;
        // a lower group's guide must remain that group's child.
        for child in &mut children {
            let mut data = child.data().clone();
            match &mut data {
                LayerData::Video(v) => v.parent = Some(group_id),
                LayerData::Image(v) => v.parent = Some(group_id),
                LayerData::Rect(v) => v.parent = Some(group_id),
                LayerData::Shape(v) => v.parent = Some(group_id),
                LayerData::Text(v) => v.parent = Some(group_id),
                LayerData::Group(v) => v.parent = Some(group_id),
                LayerData::Adjustment(v) => v.parent = Some(group_id),
                LayerData::Media(v) => v.parent = Some(group_id),
                LayerData::Pag(v) => v.parent = Some(group_id),
                LayerData::Audio(v) => v.parent = Some(group_id),
                LayerData::AiEdit(v) => v.parent = Some(group_id),
                LayerData::BooleanOperation(v) => v.parent = Some(group_id),
            }
            *child = Layer::from_data(&data)?;
        }
        let mut group = plain_group(
            group_id,
            format!("Premiere adjustment {feature} composite"),
            range,
            identity_transform(),
            children,
        )?;
        group.parent = scope.parent;
        group.masks.push(guide_mask(mask_id, guide_id, feather));
        let rect = black_shape(canvas[0], canvas[1]);
        let guide = guide_layer(
            guide_id,
            format!("Premiere adjustment {feature} guide"),
            Some(group_id),
            range,
            transform,
            rect,
        );
        group
            .layers
            .push(Layer::from_data(&LayerData::Rect(guide))?);
        layers.push(Layer::from_data(&LayerData::Group(group))?);
        set_tracks(dynamics, tracks)?;
    }
    Ok(())
}

struct CoverageGuide {
    transform: fx_schema::Transform,
    tracks: Vec<(Property, PropertyKeyframeTrack)>,
    feather: f64,
}

// The full-canvas guide opens outside the adjustment's half-open interval.
fn crop_guide(
    clip: &PrVideoOccurrence,
    guide: LayerId,
    canvas: [u32; 2],
    start: u64,
    end: u64,
) -> Result<CoverageGuide> {
    use fx_schema::PropType;
    let crop = &clip.crop;
    let mut tracks = Vec::with_capacity(4);
    for (property, value, full) in [
        (
            PropType::PositionX,
            f64::from(canvas[0]) * crop.left / 100.0,
            0.0,
        ),
        (
            PropType::PositionY,
            f64::from(canvas[1]) * crop.top / 100.0,
            0.0,
        ),
        (PropType::ScaleX, 100.0 - crop.left - crop.right, 100.0),
        (PropType::ScaleY, 100.0 - crop.top - crop.bottom, 100.0),
    ] {
        let mut keys = Vec::with_capacity(3);
        for (index, (time, value)) in [(0, full), (start, value), (end, full)]
            .into_iter()
            .enumerate()
        {
            if start == 0 && index == 0 {
                continue;
            }
            keys.push(PropertyKeyframe::new(
                keyframe_id(guide, &format!("adjustment-crop-{property:?}"), index),
                TimeOffset::from_millis(time as i64),
                PropertyValue::Float(value),
                PropertyKeyframeEasing::Hold,
            ));
        }
        let track = PropertyKeyframeTrack::new(keys).map_err(|error| {
            crate::error::unsupported(format!("adjustment Crop guide keys: {error}"))
        })?;
        tracks.push((Property::new(guide, property), track));
    }
    Ok(CoverageGuide {
        transform: identity_transform(),
        tracks,
        feather: 0.0,
    })
}
