//! Isolated, weighted pictures for an ordinary two-sided Cross Dissolve.
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
    animator::{PropertyKeyframe, PropertyKeyframeTrack},
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
        handled.insert(transition_index);
        // Prepare both pictures and their complete graph before changing the
        // stack. Unsupported admission must retain the original cut pictures.
        let prepared = (|| -> Result<_> {
            if scope.parent.is_some()
                || !(transition.start_ticks <= transition.cut_ticks
                    && transition.cut_ticks <= transition.end_ticks
                    && transition.start_ticks < transition.end_ticks)
            {
                return Err(unsupported(
                    "requires a bounded dissolve on the document clock",
                ));
            }
            let group_id = LayerId::new(*scope.next_index as u64 + 1);
            let guide_id = LayerId::new(*scope.next_index as u64 + 4);
            let mask_id = FxItemId::new(*scope.next_index as u64 + 5);
            let pictures = [outgoing, incoming].map(|id| {
                track
                    .items
                    .iter()
                    .filter_map(PrVideoItem::media)
                    .find(|clip| clip.id.as_deref() == Some(id))
                    .ok_or_else(|| unsupported("a linked physical picture was not retained"))
            });
            let [outgoing_picture, incoming_picture] = pictures;
            let outgoing_picture = outgoing_picture?;
            let incoming_picture = incoming_picture?;
            // Keep the document clock inside the group, but isolate only the
            // union of the pictures, not unrelated parts of the sequence.
            let range = tick_range(
                outgoing_picture
                    .start_ticks
                    .min(incoming_picture.start_ticks),
                outgoing_picture.end_ticks.max(incoming_picture.end_ticks),
            )?;
            let playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
            let origin = range.start.as_millis();
            let start = time_from_ticks(transition.start_ticks)?.as_millis();
            let cut = time_from_ticks(transition.cut_ticks)?.as_millis();
            let end = time_from_ticks(transition.end_ticks)?.as_millis();
            if start < origin
                || end > range.end().as_millis()
                || start >= end
                || (transition.start_ticks < transition.cut_ticks
                    && transition.cut_ticks < transition.end_ticks
                    && !(start < cut && cut < end))
            {
                return Err(unsupported(
                    "dissolve keys collapse after millisecond rounding",
                ));
            }
            let mut children = Vec::new();
            let mut positions = Vec::new();
            let mut tracks = Vec::new();
            for (side, id) in [outgoing, incoming].into_iter().enumerate() {
                let clip = track
                    .items
                    .iter()
                    .filter_map(PrVideoItem::media)
                    .find(|clip| clip.id.as_deref() == Some(id))
                    .ok_or_else(|| unsupported("a linked physical picture was not retained"))?;
                if clip.blend_mode != PrBlendMode::Normal
                    || clip.playback_rate != 1.0
                    || clip.time_remap.is_some()
                    || !clip.animations.is_empty()
                    || !clip.effects.is_empty()
                    || clip.source_effects.is_some()
                    || !clip.crop.is_default()
                    || clip.linear_wipe.is_some()
                    || clip.opacity_mask.is_some()
                    || clip.track_matte.is_some()
                    || clip.active_transforms != 0
                    || clip.stroke.is_some()
                    || matte_consumers.contains_key(&(track_index, clip.start_ticks))
                    || track
                        .transitions
                        .iter()
                        .filter(|other| {
                            other.outgoing_clip.as_deref() == Some(id)
                                || other.incoming_clip.as_deref() == Some(id)
                        })
                        .count()
                        != 1
                {
                    return Err(unsupported("requires unkeyed, unstaged Normal pictures without effects, masks, matte bindings or another transition"));
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
                let tail = side == 0;
                if (tail
                    && (clip.end_ticks != transition.cut_ticks
                        || clip.start_ticks > transition.start_ticks))
                    || (!tail
                        && (clip.start_ticks != transition.cut_ticks
                            || clip.end_ticks < transition.end_ticks))
                {
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
                if dynamics
                    .entries()
                    .iter()
                    .any(|entry| entry.target.layer_id() == Some(layer_id))
                {
                    return Err(unsupported(
                        "the picture already has an animated property clock",
                    ));
                }
                let lead = if tail {
                    0
                } else {
                    transition.cut_ticks - transition.start_ticks
                };
                let trail = if tail {
                    transition.end_ticks - transition.cut_ticks
                } else {
                    0
                };
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
                    if tail {
                        clip.start_ticks
                    } else {
                        transition.start_ticks
                    },
                    if tail {
                        transition.end_ticks
                    } else {
                        clip.end_ticks
                    },
                )?;
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
                let weight = |progress: f64| 100.0 * if tail { 1.0 - progress } else { progress };
                let mut values = vec![(start, weight(0.0))];
                if start < cut && cut < end {
                    // Keep the saved cut's fraction before clock quantization;
                    // a small native offset must not be treated as malformed.
                    let progress = (transition.cut_ticks - transition.start_ticks) as f64
                        / (transition.end_ticks - transition.start_ticks) as f64;
                    values.push((cut, weight(progress)));
                }
                values.push((end, weight(1.0)));
                let keys = values
                    .into_iter()
                    .enumerate()
                    .map(|(index, (time, value))| {
                        let local = time
                            .checked_sub(origin)
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
            if positions[0] == positions[1] {
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
                layers[positions[0].min(positions[1])] = group;
                layers.remove(positions[0].max(positions[1]));
                *scope.next_index += 5;
                approximate(omissions, &transition.id,
                    "Cross Dissolve uses an isolated linear weighted sum of editable pictures; native edited-export and alpha equivalence are unverified");
            }
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                &transition.id,
                format!("Cross Dissolve detected; editable transition not converted: {error}"),
            ),
        }
    }
    Ok(handled)
}
