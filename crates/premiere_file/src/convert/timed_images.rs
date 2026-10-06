//! Lower disjoint, untransformed Image children directly through their Group's Motion.
//!
//! Each instant shows at most one child, so applying the group's Motion and
//! opacity to that still preserves compositing without a canvas-sized nest
//! clipping source pixels first. This is structural, not import provenance.
use super::{
    background::identity_transform,
    nested::LayerExport,
    still::{export_image_layer, still_facts},
    tesseract_to_premiere::{export_motion_keys, export_transform, layer_animations, MotionHost},
};
use crate::{
    approximate,
    error::{unsupported, Result},
    export_loss::{omit_field, ExportField, OmissionSink},
    omit,
    schema::{PrMedia, PrVideoOccurrence},
    Omission, OmissionKind, OmissionScope,
};
use fx_schema::{BlendMode, Duration, GroupLayer, ImageSource, LayerData};

/// A supplied matte is a bounded set of neutral full-canvas stills. Group
/// Motion/Opacity remain editable; child geometry, clocks and effects do not.
pub(super) fn is_matte_source(
    group: &GroupLayer,
    dynamics: &fx_schema::AnimationGraph,
    canvas: [u32; 2],
) -> bool {
    if super::nested::unsupported_group_fields(group, dynamics).is_some()
        || group.blend_mode != BlendMode::Normal
        || !group.effects.is_empty()
        || !group.masks.is_empty()
        || group.track_matte.is_some()
        || layer_animations(dynamics, group.id).next().is_some()
        || group.layers.is_empty()
    {
        return false;
    }
    let mut ranges = Vec::with_capacity(group.layers.len());
    for layer in &group.layers {
        let fx_schema::LayerData::Image(image) = layer.data() else {
            return false;
        };
        let ImageSource::Asset(source) = &image.source;
        if image.parent != Some(group.id)
            || image.is_hidden
            || source.input_transform.is_some()
            || source.time_remap.is_some()
            || super::tesseract_to_premiere::source_frame(source.frame_rect).ok() != Some(canvas)
            || image.transform != identity_transform()
            || image.blend_mode != BlendMode::Normal
            || !image.effects.is_empty()
            || !image.masks.is_empty()
            || image.track_matte.is_some()
            || image.corner_radius.is_some()
            || image.motion_blur
            || layer_animations(dynamics, image.id).next().is_some()
        {
            return false;
        }
        let Some(end) = image
            .active_range
            .start
            .checked_add_duration(image.active_range.duration)
        else {
            return false;
        };
        if end.as_millis() > group.playback.input_range().duration.as_millis() {
            return false;
        }
        ranges.push(image.active_range.start..end);
    }
    ranges.sort_by_key(|range| range.start);
    ranges.last().is_some_and(|range| {
        range.end.as_millis() == group.playback.input_range().duration.as_millis()
    }) && ranges.windows(2).all(|pair| pair[0].end <= pair[1].start)
}

pub(super) fn export_group(
    group: &GroupLayer,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<Vec<(PrVideoOccurrence, PrMedia)>>> {
    Ok(
        export_frames(group, context, omissions, false)?.map(|frames| {
            frames
                .into_iter()
                .map(|(_, clip, media)| (clip, media))
                .collect()
        }),
    )
}

/// Disjoint stills carry current Group Motion inside a neutral matte nest.
/// This keeps native linked-matte admission unchanged and clocks nest-local.
pub(super) fn export_matte_frames(
    group: &GroupLayer,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<Vec<(fx_schema::LayerId, PrVideoOccurrence, PrMedia)>>> {
    export_frames(group, context, omissions, true)
}

fn export_frames(
    group: &GroupLayer,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    local_clock: bool,
) -> Result<Option<Vec<(fx_schema::LayerId, PrVideoOccurrence, PrMedia)>>> {
    if group.layers.is_empty()
        || !group.effects.is_empty()
        || !group.masks.is_empty()
        || group.track_matte.is_some()
        || group.blend_mode != BlendMode::Normal
    {
        return Ok(None);
    }
    let mut window = group.playback.input_range();
    if local_clock {
        window.start = fx_schema::Time::ZERO;
    }
    let mut images = Vec::with_capacity(group.layers.len());
    for layer in &group.layers {
        let LayerData::Image(image) = layer.data() else {
            return Ok(None);
        };
        let ImageSource::Asset(source) = &image.source;
        if source.input_transform.is_some()
            || source.time_remap.is_some()
            || image.transform != identity_transform()
            || image.blend_mode != BlendMode::Normal
            || !image.effects.is_empty()
            || !image.masks.is_empty()
            || image.track_matte.is_some()
            || image.corner_radius.is_some()
            || image.motion_blur
            || layer_animations(context.dynamics, image.id)
                .next()
                .is_some()
        {
            return Ok(None);
        }
        let end = image
            .active_range
            .start
            .checked_add_duration(image.active_range.duration)
            .ok_or_else(|| unsupported("timed Image range overflows"))?;
        if end.as_millis() > window.duration.as_millis() {
            return Ok(None);
        }
        images.push(image);
    }
    images.sort_by_key(|image| image.active_range.start);
    for pair in images.windows(2) {
        let end = pair[0]
            .active_range
            .start
            .checked_add_duration(pair[0].active_range.duration)
            .ok_or_else(|| unsupported("timed Image range overflows"))?;
        if end > pair[1].active_range.start {
            return Ok(None);
        }
    }
    if !group.description.is_empty() {
        omit_field(
            omissions,
            group.id,
            ExportField::Description,
            format!("layer {} ({:?})", group.id, group.name),
            "description was not exported",
        );
    }
    let group_start = context.frame_ticks(window.start, "timed Image group start")?;
    // A neutral supplied-matte nest adds no clock mapping. At an exact
    // integral-grid root placement, local sampled boundaries remain the same
    // document samples. Fractional-grid origins, deeper containing clocks and
    // animated owners retain conservative picture losses.
    let root_clock = context.depth == 0 && context.origin == fx_schema::Time::ZERO;
    let matte_clock = local_clock
        && context.depth == 1
        && super::timing::ticks_from_time(context.origin, "supplied matte origin")?
            % context.frame_rate.ticks_per_frame()
            == 0;
    let mut normalization_only = (root_clock || matte_clock)
        && layer_animations(context.dynamics, group.id)
            .next()
            .is_none()
        && sampled_boundary(context, window.start)?;
    let mut exported = Vec::with_capacity(images.len());
    for image in images {
        let record = format!("layer {} ({:?})", image.id, image.name);
        // Check media before grid omission; malformed facts must not be hidden
        // by a short interval. Ordinary still export owns its other admission.
        let ImageSource::Asset(source) = &image.source;
        if let Err(reason) = still_facts(source, context.media_facts)? {
            omit(omissions, OmissionScope::Occurrence, &record, reason);
            continue;
        }
        let mut placed = image.clone();
        placed.active_range.start = window
            .start
            .checked_add_duration(Duration::from_millis(image.active_range.start.as_millis()))
            .ok_or_else(|| unsupported("timed Image placement overflows"))?;
        let end = placed
            .active_range
            .start
            .checked_add_duration(placed.active_range.duration)
            .ok_or_else(|| unsupported("timed Image placement end overflows"))?;
        let final_matte_frame = local_clock && end.as_millis() == window.duration.as_millis();
        normalization_only &= sampled_boundary(context, placed.active_range.start)?
            && (sampled_boundary(context, end)? || final_matte_frame);
        if context.picture_end_ticks(end, None)?
            <= context.frame_ticks(placed.active_range.start, "timed Image start")?
        {
            omit(omissions, OmissionScope::Occurrence, &record,
                "timed Image interval collapses to zero duration on the output grid; only this frame placement was omitted");
            continue;
        }
        placed.is_hidden |= group.is_hidden;
        let Some((mut clip, media)) =
            export_image_layer(&placed, Some(group.id), None, context, omissions, &record)?
        else {
            continue;
        };
        let stream = media
            .video
            .as_ref()
            .expect("exported still has an image stream");
        let source_size = [stream.width, stream.height];
        let canvas = [context.width, context.height];
        clip.transform = export_transform(
            MotionHost {
                id: group.id,
                transform: &group.transform,
            },
            source_size,
            canvas,
            context.dynamics,
            &record,
            omissions,
        );
        clip.opacity = group.transform.opacity.value();
        let mut tracks = context
            .property_tracks
            .get(&group.id)
            .cloned()
            .unwrap_or_default();
        // Group keys are on its zero-based input clock. Keep that clock at
        // every snapped child placement, rather than restarting keys per frame.
        let source_in = clip
            .in_ticks
            .checked_sub(clip.start_ticks - group_start)
            .ok_or_else(|| unsupported("timed Image property clock overflows"))?;
        let (animations, dropped) = export_motion_keys(
            &mut tracks,
            source_in,
            &mut clip.transform,
            source_size,
            canvas,
            &record,
            omissions,
        );
        if dropped.is_some() || !tracks.is_empty() {
            omit(
                omissions,
                OmissionScope::Occurrence,
                &record,
                "timed Image group Motion keys cannot be preserved; frame placement omitted",
            );
            continue;
        }
        clip.animations = animations;
        context
            .written
            .record_placement(group.id, &clip.animations, None, None);
        exported.push((image.id, clip, media));
    }
    context.property_tracks.remove(&group.id);
    let record = format!("layer {} ({:?})", group.id, group.name);
    if normalization_only {
        omissions.emit_timed_image_normalization(Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Approximated,
            record,
            reason: "disjoint timed Images exported as ordinary still placements with current Group Motion; grouping normalized without changing root sequence-sampled picture".into(),
        });
    } else {
        approximate(omissions, record,
            "disjoint timed Images exported as ordinary still placements with current Group Motion; output-grid timing or animated/containing-clock picture equivalence is unproved");
    }
    Ok(Some(exported))
}

/// A fallback nest must not silently clip an image before its Group Motion.
/// An image that ordinary nested lowering omits before media lookup cannot
/// require inspected facts or discard otherwise exportable siblings here.
/// Other host/geometry families retain their existing admission and ownership.
pub(super) fn oversized_source(group: &GroupLayer, context: &LayerExport<'_, '_>) -> Result<bool> {
    if !group
        .layers
        .iter()
        .all(|layer| matches!(layer.data(), LayerData::Image(_)))
    {
        return Ok(false);
    }
    let canvas = [context.width, context.height];
    for layer in &group.layers {
        let LayerData::Image(image) = layer.data() else {
            continue;
        };
        if super::tesseract_to_premiere::image_mask(image, &group.layers, context.dynamics, canvas)
            .is_err()
        {
            continue;
        }
        let ImageSource::Asset(source) = &image.source;
        let Ok(facts) = still_facts(source, context.media_facts)? else {
            continue;
        };
        if facts.width <= context.width && facts.height <= context.height {
            continue;
        }
        // Source dimensions alone do not establish clipping: the child's own
        // static 2D Motion can put a large image wholly inside the nest canvas.
        // Effects, animation and 3D/skew need a separate bounds proof.
        let transform = &image.transform;
        if !image.effects.is_empty()
            || image.motion_blur
            || layer_animations(context.dynamics, image.id)
                .next()
                .is_some()
            || transform.skew != 0.0
            || transform.skew_axis != 0.0
            || transform.rotation_x != 0.0
            || transform.rotation_y != 0.0
            || transform.orientation != [0.0; 3]
            || transform.position.z().is_some_and(|z| z != 0.0)
        {
            return Ok(true);
        }
        let (width, height) = (f64::from(facts.width), f64::from(facts.height));
        let (sin, cos) = transform.rotation.to_radians().sin_cos();
        let fits = [[0.0, 0.0], [width, 0.0], [0.0, height], [width, height]]
            .into_iter()
            .all(|[x, y]| {
                let x = (x - transform.anchor_point[0]) * transform.scale[0] / 100.0;
                let y = (y - transform.anchor_point[1]) * transform.scale[1] / 100.0;
                let x_in_canvas = transform.position.x() + cos * x - sin * y;
                let y_in_canvas = transform.position.y() + sin * x + cos * y;
                (0.0..=f64::from(context.width)).contains(&x_in_canvas)
                    && (0.0..=f64::from(context.height)).contains(&y_in_canvas)
            });
        if !fits {
            return Ok(true);
        }
    }
    Ok(false)
}

/// First native boundary whose rounded FX sample is outside the half-open
/// owner window. A final 2ms image can own a real output sample; nearest-grid
/// end snapping must not discard it. Supplied matte consumers and their picture
/// children use the same membership rule as the admitted provider.
pub(super) fn sampled_end_ticks(
    context: &LayerExport<'_, '_>,
    end: fx_schema::Time,
) -> Result<i64> {
    sampled_end_ticks_at(context.origin, context.frame_rate, end)
}

pub(super) fn sampled_end_ticks_at(
    origin_time: fx_schema::Time,
    frame_rate: crate::schema::FrameRate,
    end: fx_schema::Time,
) -> Result<i64> {
    let origin = super::timing::frame_ticks_from_time(origin_time, frame_rate, "matte origin")?;
    let absolute = origin_time
        .checked_add_duration(Duration::from_millis(end.as_millis()))
        .ok_or_else(|| unsupported("supplied matte end overflows"))?;
    let ticks =
        super::timing::frame_ticks_from_time(absolute, frame_rate, "supplied matte end")? - origin;
    let sample = crate::numbered_images::sampling::sample_time(
        (origin + ticks) / frame_rate.ticks_per_frame(),
        frame_rate,
    )?;
    if sample < absolute {
        ticks
            .checked_add(frame_rate.ticks_per_frame())
            .ok_or_else(|| unsupported("supplied matte end overflows"))
    } else {
        Ok(ticks)
    }
}

fn sampled_boundary(context: &LayerExport<'_, '_>, time: fx_schema::Time) -> Result<bool> {
    let ticks = context.frame_ticks(time, "timed Image sampled boundary")?;
    let frame = ticks / context.frame_rate.ticks_per_frame();
    Ok(crate::numbered_images::sampling::sample_time(frame, context.frame_rate)? == time)
}

/// Shared numbered-frame picture construction for ordinary sequences and saved
/// supplied mattes. Ranges are already sampled on the owning sequence grid.
pub(super) fn import_frames(
    source: &crate::schema::PrVideoStream,
    asset: &fx_schema::AssetId,
    parent: fx_schema::LayerId,
    frames: Vec<(usize, fx_schema::TimeRangeProperty)>,
    next_index: &mut usize,
) -> Result<Vec<fx_schema::Layer>> {
    frames
        .into_iter()
        .map(|(frame_index, range)| {
            let id = fx_schema::LayerId::new(*next_index as u64 + 1);
            *next_index += 1;
            let mut image = super::still::image_layer(
                source,
                &crate::numbered_images::frame_asset(asset, frame_index),
                id,
                format!("Frame {}", frame_index + 1),
                range,
                false,
                identity_transform(),
            )?;
            image.parent = Some(parent);
            Ok(fx_schema::Layer::from_data(&LayerData::Image(image))?)
        })
        .collect()
}
