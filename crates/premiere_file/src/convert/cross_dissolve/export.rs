//! Export current editable dissolve values, never a saved native transition blob.
use super::super::{
    background::{black_shape, identity_transform, plain_group},
    color_matte,
    nested::LayerExport,
    premiere_to_tesseract::{guide_layer, guide_mask},
    tesseract_to_premiere as export,
    timing::time_from_ticks,
};
use crate::{
    approximate,
    error::Result,
    export_loss::OmissionSink,
    schema::{
        PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe, PrVideoItem, PrVideoOccurrence,
        PrVideoTrack, PrVideoTransition, PrVideoTransitionKind, TICKS_PER_MILLISECOND,
    },
};
use fx_schema::{
    animator::PropertyKeyframeTrack, GroupLayer, LayerData, LayerId, PropType,
    PropertyKeyframeEasing, PropertyValue, RectLayer,
};
use std::collections::BTreeSet;

/// Lower only a current linear edge ramp on a flat picture. Names and key IDs
/// cannot distinguish an imported dissolve from an equivalent authored ramp.
pub(in crate::convert) fn place_picture(
    tracks: &mut Vec<PrVideoTrack>,
    mut item: PrVideoItem,
    min_track: usize,
    layer: LayerId,
    context: &mut LayerExport<'_, '_>,
) -> Result<usize> {
    let transition = if let PrVideoItem::Media(clip) = &mut item {
        one_sided(clip, layer, context)
    } else {
        None
    };
    let index = export::place_item(tracks, item, min_track, context)?;
    if let Some(transition) = transition {
        tracks[index].transitions.push(transition);
    }
    Ok(index)
}

fn one_sided(
    clip: &mut PrVideoOccurrence,
    layer: LayerId,
    context: &LayerExport<'_, '_>,
) -> Option<PrVideoTransition> {
    let tracks = export::layer_tracks(context.dynamics, layer, |property| {
        property == PropType::Opacity
    })?;
    let track = tracks.get(&PropType::Opacity)?;
    if ramp(track)?.len() != 2
        || clip.blend_mode != crate::schema::PrBlendMode::Normal
        || clip.playback_rate != 1.0
        || clip.time_remap.is_some()
        || clip.track_matte.is_some()
        || !clip.effects.is_empty()
        || clip.source_effects.is_some()
        || !clip.crop.is_default()
        || clip.linear_wipe.is_some()
        || clip.opacity_mask.is_some()
        || clip.active_transforms != 0
        || clip.stroke.is_some()
    {
        return None;
    }
    let animation = clip
        .animations
        .iter()
        .position(|animation| matches!(animation, PrPropertyAnimation::Opacity(_)))?;
    let PrPropertyAnimation::Opacity(keys) = &clip.animations[animation] else {
        return None;
    };
    let [first, last] = keys.as_slice() else {
        return None;
    };
    if first.easing != PrKeyframeEasing::Linear
        || last.easing != PrKeyframeEasing::Linear
        || first.source_ticks >= last.source_ticks
    {
        return None;
    }
    let head = first.value == 0.0
        && last.value > 0.0
        && first.source_ticks == clip.in_ticks
        && last.source_ticks <= clip.out_ticks;
    let tail = last.value == 0.0
        && first.value > 0.0
        && last.source_ticks == clip.out_ticks
        && first.source_ticks >= clip.in_ticks;
    if !head && !tail {
        return None;
    }
    let start = clip
        .start_ticks
        .checked_add(first.source_ticks.checked_sub(clip.in_ticks)?)?;
    let end = clip
        .start_ticks
        .checked_add(last.source_ticks.checked_sub(clip.in_ticks)?)?;
    let id = format!("dissolve-picture-{layer}");
    let transition = PrVideoTransition {
        id: format!("dissolve-{layer}"),
        kind: PrVideoTransitionKind::CrossDissolve,
        start_ticks: start,
        end_ticks: end,
        cut_ticks: if head { start } else { end },
        outgoing_clip: tail.then(|| id.clone()),
        incoming_clip: head.then(|| id.clone()),
    };
    clip.opacity = if head { last.value } else { first.value };
    clip.animations.remove(animation);
    clip.id = Some(id);
    Some(transition)
}

/// The only animated rectangle admitted as a native dissolve matte. Fill
/// alpha multiplies the current key values, not the overridden static opacity.
pub(in crate::convert) fn matte_animation(
    rect: &RectLayer,
    context: &LayerExport<'_, '_>,
) -> Option<Vec<PrScalarKeyframe>> {
    let tracks = export::layer_tracks(context.dynamics, rect.id, |_| true)?;
    if tracks.len() != 1 || !(0.0..=1.0).contains(&rect.rect.fill_color[3]) {
        return None;
    }
    let track = tracks.get(&PropType::Opacity)?;
    let keys = ramp(track)?;
    let [(first_time, first), (last_time, last)] = keys.as_slice() else {
        return None;
    };
    let duration = i64::try_from(rect.active_range.duration.as_millis()).ok()?;
    let head = *first_time == 0 && *first == 0.0 && *last > 0.0 && *last_time <= duration;
    let tail = *last_time == duration && *last == 0.0 && *first > 0.0 && *first_time >= 0;
    if !head && !tail {
        return None;
    }
    keys.into_iter()
        .map(|(time, value)| {
            Some(PrScalarKeyframe {
                source_ticks: context
                    .frame_rate
                    .generator_in_ticks()
                    .checked_add(time.checked_mul(TICKS_PER_MILLISECOND)?)?,
                value: value * rect.rect.fill_color[3],
                easing: PrKeyframeEasing::Linear,
            })
        })
        .collect()
}

fn ramp(track: &PropertyKeyframeTrack) -> Option<Vec<(i64, f64)>> {
    let keys = track.keyframes();
    if !(2..=3).contains(&keys.len()) {
        return None;
    }
    keys.iter()
        .map(|key| {
            let PropertyValue::Float(value) = key.value() else {
                return None;
            };
            ((0.0..=100.0).contains(value)
                && key.easing() == PropertyKeyframeEasing::Linear
                && key.spatial_in_tangent().is_none()
                && key.spatial_out_tangent().is_none())
            .then_some((key.layer_time().as_millis(), *value))
        })
        .collect()
}

/// Recover a frame-grid cut and duration only when their rounded coordinates
/// still equal the current controls. The middle weight retains native residual
/// ticks (for example, 1892 ticks); arbitrary edits use the current FX clock.
fn transition_ticks(keys: &[(i64, f64)], frame: i64) -> Option<(i64, i64, i64)> {
    let &(start, _) = keys.first()?;
    let &(end, _) = keys.last()?;
    if start < 0 || start >= end {
        return None;
    }
    let (cut, progress) = if keys.len() == 3 {
        (keys[1].0, keys[1].1 / 100.0)
    } else {
        ((start + end) / 2, 0.5)
    };
    if cut < start
        || cut > end
        || !(0.0..=1.0).contains(&progress)
        || ((cut - start) as f64 - progress * (end - start) as f64).abs() > 1.0
    {
        return None;
    }
    let start_ticks = start.checked_mul(TICKS_PER_MILLISECOND)?;
    let end_ticks = end.checked_mul(TICKS_PER_MILLISECOND)?;
    let cut_ticks = cut.checked_mul(TICKS_PER_MILLISECOND)?;
    let grid = |ticks: i64| ((ticks as f64 / frame as f64).round() as i64).checked_mul(frame);
    let aligned_cut = grid(cut_ticks)?;
    let duration = grid(end_ticks - start_ticks)?;
    let aligned_start = aligned_cut.checked_sub((progress * duration as f64).round() as i64)?;
    let aligned_end = aligned_start.checked_add(duration)?;
    if duration > 0
        && [aligned_start, aligned_cut, aligned_end]
            .into_iter()
            .zip([start, cut, end])
            .all(|(ticks, millis)| {
                time_from_ticks(ticks).is_ok_and(|time| time.as_millis() as i64 == millis)
            })
    {
        Some((aligned_start, aligned_cut, aligned_end))
    } else {
        Some((start_ticks, cut_ticks, end_ticks))
    }
}

pub(in crate::convert) fn export_group(
    group: &GroupLayer,
    tracks: &mut Vec<PrVideoTrack>,
    consumed: &BTreeSet<LayerId>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Result<bool> {
    if canonical_group(group, tracks, consumed, context, omissions)? {
        return Ok(true);
    }
    // Keep current pictures and coverage on the ordinary nested path when
    // only native-transition recognition fails. Existing native Linear Dodge
    // carries Add; do not replace it with Normal or flatten the isolation.
    let isolated_add = !group.masks.is_empty() && group.layers.iter().any(|layer| {
        matches!(layer.data(), LayerData::Group(child) if child.blend_mode == fx_schema::BlendMode::Add)
    });
    if isolated_add {
        approximate(omissions, format!("layer {}", group.id),
            "native Cross Dissolve was not reconstructed from the current topology, controls or clocks; retaining supported current pictures, masks, clocks and Add through ordinary nested export using native Linear Dodge, not Normal; nested rasterization/pass-through and native transition fidelity remain unmeasured; independently unsafe masks or clocks remain scoped omissions");
    }
    Ok(false)
}

fn canonical_group(
    group: &GroupLayer,
    tracks: &mut Vec<PrVideoTrack>,
    consumed: &BTreeSet<LayerId>,
    context: &mut LayerExport<'_, '_>,
    _omissions: &mut dyn OmissionSink,
) -> Result<bool> {
    if context.depth != 0
        || group.parent.is_some()
        || consumed.contains(&group.id)
        || group.layers.len() != 3
        || group.masks.len() != 1
        || group.masks.iter().any(|mask| {
            context
                .dynamics
                .entries()
                .iter()
                .any(|entry| entry.target.fx_item_id() == Some(mask.id))
        })
        || !export::layer_tracks(context.dynamics, group.id, |_| true)
            .is_some_and(|tracks| tracks.is_empty())
    {
        return Ok(false);
    }
    let [outgoing, incoming, guide] = group.layers.as_slice() else {
        return Ok(false);
    };
    let (LayerData::Group(outgoing), LayerData::Group(incoming), LayerData::Rect(guide)) =
        (outgoing.data(), incoming.data(), guide.data())
    else {
        return Ok(false);
    };
    let range = group.playback.input_range();
    let picture_ranges =
        [outgoing, incoming].map(|fade| fade.layers.first().map(|picture| picture.active_range()));
    let [Some(out_range), Some(in_range)] = picture_ranges else {
        return Ok(false);
    };
    let [Some(out_end), Some(in_end)] = [out_range, in_range].map(|range| {
        range
            .start
            .as_millis()
            .checked_add(range.duration.as_millis())
    }) else {
        return Ok(false);
    };
    if range.start != out_range.start.min(in_range.start)
        || range
            .start
            .as_millis()
            .checked_add(range.duration.as_millis())
            != Some(out_end.max(in_end))
    {
        return Ok(false);
    }
    let expected_guide = guide_layer(
        guide.id,
        guide.name.clone(),
        Some(group.id),
        range,
        identity_transform(),
        black_shape(context.width, context.height),
    );
    let mut expected = plain_group(
        group.id,
        group.name.clone(),
        range,
        identity_transform(),
        group.layers.clone(),
    )?;
    expected.description = group.description.clone();
    expected.playback = fx_schema::LayerPlayback::linear(range, range, range, 0)
        .map_err(crate::error::unsupported)?;
    expected
        .masks
        .push(guide_mask(group.masks[0].id, guide.id, 0.0));
    if group != &expected
        || guide != &expected_guide
        || !export::layer_tracks(context.dynamics, guide.id, |_| true)
            .is_some_and(|tracks| tracks.is_empty())
    {
        return Ok(false);
    }
    let mut curves = Vec::new();
    for fade in [outgoing, incoming] {
        if fade.layers.len() != 1
            || consumed.contains(&fade.id)
            || consumed.contains(&fade.layers[0].id())
        {
            return Ok(false);
        }
        let mut expected = plain_group(
            fade.id,
            fade.name.clone(),
            range,
            identity_transform(),
            fade.layers.clone(),
        )?;
        expected.parent = Some(group.id);
        expected.description = fade.description.clone();
        expected.playback = fx_schema::LayerPlayback::linear(range, range, range, 0)
            .map_err(crate::error::unsupported)?;
        expected.blend_mode = fx_schema::BlendMode::Add;
        if fade != &expected {
            return Ok(false);
        }
        let Some(properties) = export::layer_tracks(context.dynamics, fade.id, |_| true) else {
            return Ok(false);
        };
        if properties.len() != 1 {
            return Ok(false);
        }
        let Some(mut curve) = properties
            .get(&PropType::Opacity)
            .and_then(|track| ramp(track))
        else {
            return Ok(false);
        };
        // Fade keys use layer time. Pictures and native transitions use the
        // retained document clock, even when isolation starts after zero.
        let (Ok(origin), Ok(duration)) = (
            i64::try_from(range.start.as_millis()),
            i64::try_from(range.duration.as_millis()),
        ) else {
            return Ok(false);
        };
        if curve.iter().any(|(time, _)| !(0..=duration).contains(time)) {
            return Ok(false);
        }
        for (time, _) in &mut curve {
            let Some(absolute) = time.checked_add(origin) else {
                return Ok(false);
            };
            *time = absolute;
        }
        curves.push(curve);
        let layer = &fade.layers[0];
        if !export::layer_tracks(context.dynamics, layer.id(), |_| true)
            .is_some_and(|tracks| tracks.is_empty())
        {
            return Ok(false);
        }
        let plain = match layer.data() {
            LayerData::Video(v) => {
                v.blend_mode == fx_schema::BlendMode::Normal
                    && v.masks.is_empty()
                    && v.track_matte.is_none()
                    && v.effects.is_empty()
                    && v.volume.unwrap_or(fx_schema::LinearGain::ZERO)
                        == fx_schema::LinearGain::ZERO
            }
            LayerData::Image(v) => {
                v.blend_mode == fx_schema::BlendMode::Normal
                    && v.masks.is_empty()
                    && v.track_matte.is_none()
                    && v.effects.is_empty()
            }
            LayerData::Rect(v) => {
                v.blend_mode == fx_schema::BlendMode::Normal
                    && color_matte::exported_matte(
                        v,
                        Some(fade.id),
                        &BTreeSet::new(),
                        context,
                        [context.width, context.height],
                    )
                    .is_some()
            }
            _ => false,
        };
        if !plain {
            return Ok(false);
        }
    }
    if curves[0].len() != curves[1].len()
        || curves[1].first().map(|k| k.1) != Some(0.0)
        || curves[1].last().map(|k| k.1) != Some(100.0)
        || !curves[0]
            .iter()
            .zip(&curves[1])
            .all(|(a, b)| a.0 == b.0 && (a.1 + b.1 - 100.0).abs() < 1e-9)
    {
        return Ok(false);
    }
    let Some((start, cut, end)) =
        transition_ticks(&curves[1], context.frame_rate.ticks_per_frame())
    else {
        return Ok(false);
    };
    // Both editable pictures must remain visible for the entire ramp. Check
    // the editable clock before native frame rounding can restore a trim.
    let (Ok(first), Ok(last)) = (
        u64::try_from(curves[1][0].0),
        u64::try_from(curves[1].last().unwrap().0),
    ) else {
        return Ok(false);
    };
    for fade in [outgoing, incoming] {
        let range = fade.layers[0].active_range();
        if range.start.as_millis() > first
            || range
                .start
                .as_millis()
                .checked_add(range.duration.as_millis())
                .is_none_or(|end| end < last)
        {
            return Ok(false);
        }
    }
    let mut media = context.media.clone();
    let Ok(pictures) = super::pictures::prepare([outgoing, incoming], context, &mut media) else {
        return Ok(false);
    };
    // Allow only the existing sub-millisecond reconstruction of native ticks,
    // not a missing part of either picture or a changed playback mapping.
    if pictures.iter().any(|clip| {
        clip.playback_rate != 1.0
            || clip.time_remap.is_some()
            || clip.start_ticks > start.saturating_add(TICKS_PER_MILLISECOND)
            || clip.end_ticks < end.saturating_sub(TICKS_PER_MILLISECOND)
    }) {
        return Ok(false);
    }
    *context.media = media;
    let mut items = Vec::new();
    for (index, (mut clip, fade)) in pictures.into_iter().zip([outgoing, incoming]).enumerate() {
        // The two pictures are one replaceable source boundary: the current
        // isolated group. Replacing either half alone would lose its dissolve.
        clip.id = Some(format!("dissolve-{}-{index}", group.id));
        if index == 0 {
            clip.out_ticks -= clip.end_ticks - cut;
            clip.end_ticks = cut;
        } else {
            clip.in_ticks += cut - clip.start_ticks;
            clip.start_ticks = cut;
        }
        items.push(PrVideoItem::Media(clip));
        context
            .packer
            .record_item(context.container, context.boundary()?, tracks.len())?;
        context.written.record(fade.id, PropType::Opacity);
        context.property_tracks.remove(&fade.id);
    }
    let outgoing_clip = items[0].media().and_then(|clip| clip.id.clone());
    let incoming_clip = items[1].media().and_then(|clip| clip.id.clone());
    tracks.push(PrVideoTrack {
        items,
        nests: Vec::new(),
        transitions: vec![PrVideoTransition {
            id: format!("dissolve-{}", group.id),
            kind: PrVideoTransitionKind::CrossDissolve,
            start_ticks: start,
            cut_ticks: cut,
            end_ticks: end,
            outgoing_clip,
            incoming_clip,
        }],
    });
    Ok(true)
}
