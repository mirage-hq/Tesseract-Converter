//! A coverage-changing adjustment Wipe masks the effected lower composite,
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
        let Some(wipe) = &clip.linear_wipe else {
            continue;
        };
        let reason = boundary_reason(
            &layers[position..],
            &layers[..position],
            scope.nesting_depth,
        );
        if let Some(reason) = reason {
            omit(omissions, OmissionScope::Feature, clip.record(), format!("adjustment Linear Wipe not converted: {reason}; the existing layer stack is preserved"));
            continue;
        }
        let range = tick_range(0, project.end_ticks())?;
        let window = tick_range(clip.start_ticks, clip.end_ticks)?;
        let start = window.start.as_millis();
        let end = start + window.duration.as_millis();
        let group_id = LayerId::new(*scope.next_index as u64 + 1);
        let guide_id = LayerId::new(*scope.next_index as u64 + 2);
        let mask_id = FxItemId::new(*scope.next_index as u64 + 3);
        *scope.next_index += 3;
        let canvas = [project.width, project.height];
        let (mut transform, property, _) =
            linear_wipe_guide(wipe, clip.in_ticks, guide_id, canvas)?;
        // The constant key at the start is editable independently of the
        // boundary keys that keep the rest of the sequence unmasked.
        let mut values = Vec::new();
        if start != 0 {
            values.push((0, 100.0));
        }
        values.push((start, 100.0 - wipe.initial_completion));
        values.push((end, 100.0));
        let first = values[0].1;
        match property {
            fx_schema::PropType::ScaleX => transform.scale[0] = first,
            _ => transform.scale[1] = first,
        }
        let keys = values
            .into_iter()
            .enumerate()
            .map(|(index, (time, value))| {
                PropertyKeyframe::new(
                    keyframe_id(guide_id, "adjustment-wipe", index),
                    TimeOffset::from_millis(time as i64),
                    PropertyValue::Float(value),
                    PropertyKeyframeEasing::Hold,
                )
            })
            .collect();
        let track = PropertyKeyframeTrack::new(keys).map_err(|error| {
            crate::error::unsupported(format!("adjustment Wipe guide keys: {error}"))
        })?;
        let mut children = layers.split_off(position);
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
            "Premiere adjustment Wipe composite".to_owned(),
            range,
            identity_transform(),
            children,
        )?;
        group.parent = scope.parent;
        group
            .masks
            .push(guide_mask(mask_id, guide_id, wipe.feather));
        let rect = black_shape(canvas[0], canvas[1]);
        let guide = guide_layer(
            guide_id,
            "Premiere adjustment Wipe guide".to_owned(),
            Some(group_id),
            range,
            transform,
            rect,
        );
        group
            .layers
            .push(Layer::from_data(&LayerData::Rect(guide))?);
        layers.push(Layer::from_data(&LayerData::Group(group))?);
        set_tracks(dynamics, vec![(Property::new(guide_id, property), track)])?;
    }
    Ok(())
}
