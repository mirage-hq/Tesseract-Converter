//! Isolated, weighted pictures for nonoverlapping two-sided Cross Dissolves.
mod export;
mod pictures;
use super::{
    background::{black_shape, identity_transform, plain_group},
    nested::LayerScope,
    premiere_to_tesseract::{guide_layer, guide_mask, keyframe_id, set_tracks, tick_range},
    timing::time_from_ticks,
};
use crate::{
    approximate,
    error::{unsupported, Result},
    format::{MediaId, PrMedia},
    omit,
    schema::{PrBlendMode, PrMediaKind, PrSequence, PrVideoItem, PrVideoTransitionKind},
    Omission, OmissionScope,
};
pub(super) use export::{export_group, matte_animation, place_picture};
use fx_schema::{
    animator::{AnimatorData, PropertyKeyframe, PropertyKeyframeTrack},
    AnimationGraph, BlendMode, FxItemId, Layer, LayerData, LayerId, LayerPlayback, PropType,
    Property, PropertyKeyframeEasing, PropertyValue, TimeOffset,
};
use std::collections::{BTreeMap, BTreeSet};

#[expect(
    clippy::too_many_arguments,
    reason = "transition admission shares the track's picture and matte context"
)]
pub(super) fn import(
    project: &PrSequence,
    track_index: usize,
    scope: &mut LayerScope<'_, '_>,
    layers: &mut Vec<Layer>,
    media: &BTreeMap<MediaId, PrMedia>,
    matte_consumers: &BTreeMap<(usize, i64), usize>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<BTreeSet<usize>> {
    let track = &project.video_tracks[track_index];
    let mut handled = BTreeSet::new();
    for (transition_index, transition) in track.transitions.iter().enumerate() {
        let (PrVideoTransitionKind::CrossDissolve, Some(outgoing), Some(incoming)) = (
            transition.kind,
            transition.outgoing_clip.as_deref(),
            transition.incoming_clip.as_deref(),
        ) else {
            continue;
        };
        if handled.contains(&transition_index) {
            continue;
        }
        let mut members = BTreeSet::from([transition_index]);
        let mut picture_ids = BTreeSet::from([outgoing, incoming]);
        loop {
            let previous = members.len();
            for (index, other) in track.transitions.iter().enumerate() {
                if other.kind != PrVideoTransitionKind::CrossDissolve {
                    continue;
                }
                if let (Some(outgoing), Some(incoming)) = (
                    other.outgoing_clip.as_deref(),
                    other.incoming_clip.as_deref(),
                ) {
                    if picture_ids.contains(outgoing) || picture_ids.contains(incoming) {
                        members.insert(index);
                        picture_ids.extend([outgoing, incoming]);
                    }
                }
            }
            if members.len() == previous {
                break;
            }
        }
        handled.extend(&members);
        let mut chain: Vec<_> = members
            .iter()
            .map(|&index| &track.transitions[index])
            .collect();
        chain.sort_by_key(|item| item.cut_ticks);
        // Prepare the whole connected chain before changing the stack, so a
        // rejected ramp cannot leave only half of a shared picture converted.
        let prepared = (|| -> Result<_> {
            if scope.parent.is_some() {
                return Err(unsupported(
                    "requires a bounded dissolve on the document clock",
                ));
            }
            if chain.iter().any(|item| {
                !(item.start_ticks <= item.cut_ticks
                    && item.cut_ticks <= item.end_ticks
                    && item.start_ticks < item.end_ticks)
            }) || chain.windows(2).any(|pair| {
                pair[0].incoming_clip != pair[1].outgoing_clip
                    || pair[0].end_ticks >= pair[1].start_ticks
            }) {
                return Err(unsupported(
                    "requires a linear chain of separated dissolve ramps",
                ));
            }
            let ids: Vec<_> = std::iter::once(
                chain[0]
                    .outgoing_clip
                    .as_deref()
                    .ok_or_else(|| unsupported("missing outgoing picture"))?,
            )
            .chain(
                chain
                    .iter()
                    .filter_map(|item| item.incoming_clip.as_deref()),
            )
            .collect();
            if ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len() {
                return Err(unsupported("the dissolve chain repeats a picture"));
            }
            let group_id = LayerId::new(*scope.next_index as u64 + 1);
            let guide_id = LayerId::new(*scope.next_index as u64 + ids.len() as u64 + 2);
            let mask_id = FxItemId::new(*scope.next_index as u64 + ids.len() as u64 + 3);
            let pictures = ids
                .iter()
                .map(|id| {
                    track
                        .items
                        .iter()
                        .filter_map(PrVideoItem::media)
                        .find(|clip| clip.id.as_deref() == Some(*id))
                        .ok_or_else(|| unsupported("a linked physical picture was not retained"))
                })
                .collect::<Result<Vec<_>>>()?;
            // Keep the document clock inside the group, but isolate only the
            // union of the pictures, not unrelated parts of the sequence.
            let range = tick_range(
                pictures
                    .iter()
                    .map(|clip| clip.start_ticks)
                    .min()
                    .ok_or_else(|| unsupported("the dissolve chain has no pictures"))?,
                pictures
                    .iter()
                    .map(|clip| clip.end_ticks)
                    .max()
                    .ok_or_else(|| unsupported("the dissolve chain has no pictures"))?,
            )?;
            let playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
            let origin = range.start.as_millis();
            let mut previous_end = None;
            for item in &chain {
                let start = time_from_ticks(item.start_ticks)?.as_millis();
                let cut = time_from_ticks(item.cut_ticks)?.as_millis();
                let end = time_from_ticks(item.end_ticks)?.as_millis();
                if start < origin
                    || end > range.end().as_millis()
                    || start >= end
                    || previous_end.is_some_and(|previous| previous >= start)
                    || (item.start_ticks < item.cut_ticks
                        && item.cut_ticks < item.end_ticks
                        && !(start < cut && cut < end))
                {
                    return Err(unsupported(
                        "dissolve keys collapse after millisecond rounding",
                    ));
                }
                previous_end = Some(end);
            }
            let mut children = Vec::new();
            let mut positions = Vec::new();
            let mut tracks = Vec::new();
            for (side, (&id, &clip)) in ids.iter().zip(&pictures).enumerate() {
                let head = side.checked_sub(1).map(|index| chain[index]);
                let tail = chain.get(side).copied();
                if clip.blend_mode != PrBlendMode::Normal
                    || clip.playback_rate != 1.0
                    || clip.time_remap.is_some()
                    || !clip.effects.is_empty()
                    || clip.source_effects.is_some()
                    || !clip.crop.is_default()
                    || clip.linear_wipe.is_some()
                    || clip.opacity_mask.is_some()
                    || clip.track_matte.is_some()
                    || clip.active_transforms != 0
                    || clip.stroke.is_some()
                    || matte_consumers.contains_key(&(track_index, clip.start_ticks))
                    || track.transitions.iter().enumerate().any(|(index, other)| {
                        !members.contains(&index)
                            && (other.outgoing_clip.as_deref() == Some(id)
                                || other.incoming_clip.as_deref() == Some(id))
                    })
                {
                    return Err(unsupported("requires unstaged Normal pictures without effects, masks, matte bindings or another transition"));
                }
                let source = media
                    .get(&clip.media)
                    .and_then(|media| media.video.as_ref())
                    .ok_or_else(|| unsupported("the picture has no retained source"))?;
                if !matches!(
                    source.kind,
                    PrMediaKind::Video { .. }
                        | PrMediaKind::Still { .. }
                        | PrMediaKind::ColorMatte(_)
                ) || !matches!(
                    source.interpretation,
                    crate::schema::SourceInterpretation::Original
                ) {
                    return Err(unsupported(
                        "requires an uninterpreted flat video, still or Color Matte",
                    ));
                }
                if tail.is_some_and(|item| {
                    clip.end_ticks != item.cut_ticks || clip.start_ticks > item.start_ticks
                }) || head.is_some_and(|item| {
                    clip.start_ticks != item.cut_ticks || clip.end_ticks < item.end_ticks
                }) {
                    return Err(unsupported(
                        "the linked pictures do not meet at the saved cut",
                    ));
                }
                let layer_id = scope
                    .item_layers
                    .get(&(track_index, clip.start_ticks))
                    .ok_or_else(|| unsupported("a linked picture was omitted"))?
                    .id();
                let position = layers
                    .iter()
                    .position(|layer| layer.id() == layer_id)
                    .ok_or_else(|| unsupported("a linked picture is not beside the track"))?;
                let lead = head.map_or(0, |item| item.cut_ticks - item.start_ticks);
                let trail = tail.map_or(0, |item| item.end_ticks - item.cut_ticks);
                let source_in = clip
                    .in_ticks
                    .checked_sub(lead)
                    .filter(|value| *value >= 0)
                    .ok_or_else(|| unsupported("insufficient incoming source handles"))?;
                let source_out = clip
                    .out_ticks
                    .checked_add(trail)
                    .filter(|value| *value <= source.intrinsic_ticks)
                    .ok_or_else(|| unsupported("insufficient outgoing source handles"))?;
                let active = tick_range(
                    head.map_or(clip.start_ticks, |item| item.start_ticks),
                    tail.map_or(clip.end_ticks, |item| item.end_ticks),
                )?;
                // Extending the incoming placement moves its local zero, not
                // the document times of its existing Motion/Opacity keys.
                let shift =
                    time_from_ticks(clip.start_ticks)?.as_millis() - active.start.as_millis();
                for entry in dynamics
                    .entries()
                    .iter()
                    .filter(|entry| entry.target.layer_id() == Some(layer_id))
                {
                    let property = entry
                        .target
                        .as_property()
                        .ok_or_else(|| unsupported("dissolve picture requires a layer property"))?;
                    let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
                        return Err(unsupported("dissolve picture requires keyframe animation"));
                    };
                    let shift = i64::try_from(shift)
                        .map_err(|_| unsupported("dissolve picture key offset exceeds range"))?;
                    let keys = track
                        .keyframes()
                        .iter()
                        .map(|key| {
                            let time =
                                key.layer_time().as_millis().checked_add(shift).ok_or_else(
                                    || unsupported("dissolve picture key exceeds range"),
                                )?;
                            Ok(PropertyKeyframe::new(
                                key.id().clone(),
                                TimeOffset::from_millis(time),
                                key.value().clone(),
                                key.easing(),
                            )
                            .with_spatial_tangents(
                                key.spatial_in_tangent(),
                                key.spatial_out_tangent(),
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let track = PropertyKeyframeTrack::new(keys)
                        .map_err(|error| unsupported(format!("dissolve picture keys: {error}")))?;
                    tracks.push((property, track));
                }
                let mut source_range = tick_range(source_in, source_out)?;
                source_range.duration = active.duration;
                let fade_id = LayerId::new(*scope.next_index as u64 + 2 + side as u64);
                let mut picture = layers[position].data().clone();
                match &mut picture {
                    LayerData::Video(video) => {
                        video.parent = Some(fade_id);
                        video.source_range = source_range;
                        video.playback = LayerPlayback::linear(active, active, source_range, 0)
                            .map_err(unsupported)?;
                    }
                    LayerData::Image(image) => {
                        image.parent = Some(fade_id);
                        image.active_range = active;
                    }
                    LayerData::Rect(rect) => {
                        rect.parent = Some(fade_id);
                        rect.active_range = active;
                    }
                    _ => return Err(unsupported("requires a retained flat picture")),
                }
                let mut fade = plain_group(
                    fade_id,
                    "Premiere dissolve picture".to_owned(),
                    range,
                    identity_transform(),
                    vec![Layer::from_data(&picture)?],
                )?;
                fade.parent = Some(group_id);
                fade.playback = playback.clone();
                fade.blend_mode = BlendMode::Add;
                children.push(Layer::from_data(&LayerData::Group(fade))?);
                positions.push(position);
                let mut values = Vec::new();
                for (item, outgoing) in head
                    .map(|item| (item, false))
                    .into_iter()
                    .chain(tail.map(|item| (item, true)))
                {
                    let start = time_from_ticks(item.start_ticks)?.as_millis();
                    let cut = time_from_ticks(item.cut_ticks)?.as_millis();
                    let end = time_from_ticks(item.end_ticks)?.as_millis();
                    let weight =
                        |progress: f64| 100.0 * if outgoing { 1.0 - progress } else { progress };
                    values.push((start, weight(0.0)));
                    if start < cut && cut < end {
                        let progress = (item.cut_ticks - item.start_ticks) as f64
                            / (item.end_ticks - item.start_ticks) as f64;
                        values.push((cut, weight(progress)));
                    }
                    values.push((end, weight(1.0)));
                }
                let keys = values
                    .into_iter()
                    .enumerate()
                    .map(|(index, (time, value))| {
                        // Chains animate the shared document content clock;
                        // keep the existing single-pair export normalization.
                        let key_origin = if chain.len() == 1 { origin } else { 0 };
                        let local = time
                            .checked_sub(key_origin)
                            .and_then(|local| i64::try_from(local).ok())
                            .ok_or_else(|| {
                                unsupported("dissolve key exceeds its isolation clock")
                            })?;
                        Ok(PropertyKeyframe::new(
                            keyframe_id(fade_id, "cross-dissolve", index),
                            TimeOffset::from_millis(local),
                            PropertyValue::Float(value),
                            PropertyKeyframeEasing::Linear,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let keys = PropertyKeyframeTrack::new(keys).map_err(|error| {
                    unsupported(format!("Cross Dissolve opacity keys: {error}"))
                })?;
                tracks.push((Property::new(fade_id, PropType::Opacity), keys));
            }
            if positions.iter().copied().collect::<BTreeSet<_>>().len() != positions.len() {
                return Err(unsupported("the transition links the same picture twice"));
            }
            let mut group = plain_group(
                group_id,
                "Premiere Cross Dissolve".to_owned(),
                range,
                identity_transform(),
                children,
            )?;
            group.parent = scope.parent;
            group.playback = playback;
            // Normal groups can pass their children through to lower tracks.
            // A full-canvas mask isolates the sum before Normal compositing.
            group.masks.push(guide_mask(mask_id, guide_id, 0.0));
            let guide = guide_layer(
                guide_id,
                "Premiere dissolve isolation".to_owned(),
                Some(group_id),
                range,
                identity_transform(),
                black_shape(project.width, project.height),
            );
            group
                .layers
                .push(Layer::from_data(&LayerData::Rect(guide))?);
            Ok((
                positions,
                Layer::from_data(&LayerData::Group(group))?,
                tracks,
            ))
        })();
        match prepared {
            Ok((positions, group, tracks)) => {
                set_tracks(dynamics, tracks)?;
                let mut positions = positions;
                positions.sort_unstable();
                layers[positions[0]] = group;
                for &position in positions[1..].iter().rev() {
                    layers.remove(position);
                }
                *scope.next_index += chain.len() + 4;
                for item in &chain {
                    approximate(omissions, &item.id,
                        "Cross Dissolve uses an isolated linear weighted sum of editable pictures; Motion/Opacity keys retain their document times and hold endpoint values outside their key span; native handle extrapolation, edited-export and alpha equivalence are unverified");
                }
            }
            Err(error) => {
                for item in &chain {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        &item.id,
                        format!(
                            "Cross Dissolve detected; editable transition not converted: {error}"
                        ),
                    );
                }
            }
        }
    }
    Ok(handled)
}
