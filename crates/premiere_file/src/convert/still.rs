//! Map Premiere still occurrences to editable image layers and back.
//!
//! A still occurrence is a placement, not a media time range: Premiere starts
//! every still one hour into a synthetic twelve-hour clock and gives it the
//! placement's duration. Import therefore keeps only the timeline range, and
//! export rebuilds the synthetic source range from the layer's active range.

use super::{
    nested::LayerExport,
    tesseract_to_premiere::{
        export_motion_keys, export_transform, layer_animations, unexported_layer_fields,
        unsupported_image_masks, ClipLayer, MotionHost,
    },
};
use crate::{
    approximate,
    error::{ensure, unsupported, Result},
    export_loss::{omit_field, ExportField, OmissionSink},
    format::{PrMedia, PrVideoOccurrence},
    media::MediaFacts,
    omit,
    schema::{MediaId, PrBlendMode, PrMediaKind, STILL_INTRINSIC_TICKS},
    OmissionScope,
};
use fx_schema::{
    animator::AnimationGraph, AssetId, BlendMode, ImageLayer, ImageSource, LayerId, MediaFit,
    Position, PositiveRect, RectBounds, TimeRangeProperty, Transform,
};

/// Build an editable image layer for one still placement: the still at its
/// pixel size, moved by `transform`, the clip's Motion and Opacity mapped as a
/// video clip's are. A frame of the natural size with `Contain` draws the
/// pixels unscaled, so the geometry is Motion's alone.
pub(super) fn image_layer(
    source: &crate::schema::PrVideoStream,
    asset_id: &AssetId,
    layer_id: LayerId,
    name: String,
    active_range: TimeRangeProperty,
    is_hidden: bool,
    transform: Transform,
) -> Result<ImageLayer> {
    let frame = PositiveRect::new(RectBounds::from_size(
        source.width.into(),
        source.height.into(),
    ))
    .ok_or_else(|| unsupported("still dimensions must be positive"))?;
    Ok(ImageLayer {
        id: layer_id,
        name,
        description: String::new(),
        is_hidden,
        parent: None,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        corner_radius: None,
        active_range,
        effects: Vec::new(),
        placement: None,
        captions_enabled: None,
        motion_blur: false,
        transform,
        source: ImageSource::from_asset(asset_id.clone(), Some(frame), MediaFit::Contain),
    })
}

/// Why the still `image` is no track matte source, if it is not. A matte
/// still exports only at its defaults: default Motion, Opacity 100, a
/// contain fit of the canvas-sized frame, and no blend mode, input
/// transform, time remap, effects, corner radius or keys. A written still
/// carries its Opacity, Motion and their keys ([`export_image_layer`]), but
/// a Track Matte Key over a moved, faded or keyed still is unmeasured, and a
/// matte's coverage must export whole, or the written key gates its clip by
/// a different picture. `canvas` is the sequence size.
pub(super) fn unsupported_matte_still(
    image: &ImageLayer,
    canvas: (u32, u32),
    dynamics: &AnimationGraph,
) -> Option<String> {
    if let Some(reason) = unsupported_image_masks(image) {
        return Some(reason.to_owned());
    }
    let t = &image.transform;
    let ImageSource::Asset(source) = &image.source;
    let (width, height) = (f64::from(canvas.0), f64::from(canvas.1));
    // Both pivots draw a canvas-sized still identically at neutral Motion.
    let origin_pivot = t.anchor_point == [0.0, 0.0] && t.position == Position::default();
    let centered_pivot = t.anchor_point == [width / 2.0, height / 2.0]
        && t.position == Position::xy(width / 2.0, height / 2.0);
    let dropped = [
        (
            image.blend_mode != BlendMode::Normal,
            "blend mode (using normal)",
        ),
        (t.opacity.value() != 100.0, "opacity (using 100%)"),
        (source.fit != MediaFit::Contain, "media fit (using contain)"),
        (source.input_transform.is_some(), "input transform"),
        // An image ignores its legacy time remap, but the field is still lost.
        (source.time_remap.is_some(), "time remap"),
        (
            !(origin_pivot || centered_pivot),
            "position/anchor (using center)",
        ),
        (t.scale != [100.0, 100.0], "static scale (using 100%)"),
        (t.rotation != 0.0, "static rotation (using 0 degrees)"),
        (t.skew != 0.0 || t.skew_axis != 0.0, "skew"),
        (
            t.rotation_x != 0.0 || t.rotation_y != 0.0 || t.orientation != [0.0, 0.0, 0.0],
            "3D rotation",
        ),
        (!image.effects.is_empty(), "effects"),
        (image.corner_radius.is_some(), "corner radius"),
        (
            layer_animations(dynamics, image.id).next().is_some(),
            "keys",
        ),
    ]
    .into_iter()
    .find_map(|(changed, detail)| changed.then_some(detail))?;
    Some(format!(
        "the track matte source is a still whose {dropped} a matte still does not carry; the exported matte would gate the clip by a different picture"
    ))
}

/// Map one editable image layer to a still occurrence on Premiere's synthetic
/// still clock, which starts at the generator in-point of the sequence rate
/// ([`FrameRate::generator_in_ticks`](crate::format::FrameRate::generator_in_ticks)).
///
/// The one written form is the still at its pixel size, placed by Motion and
/// Opacity with their keys from the layer's transform, through the video
/// clip's Motion export; Scale to Frame Size is never written. A clip has no
/// frame of its own, so an image whose `sourceRect` is not the packaged image
/// at the origin, or whose fit is Cover, Stretch or Custom, is omitted. Like
/// a video layer, a hidden layer exports disabled, and each property that a
/// clip cannot carry is reported. The dimensions and alpha come from the
/// inspected packaged image, because the document records no alpha fact.
pub(super) fn export_image_layer(
    image: &ImageLayer,
    parent: Option<LayerId>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Option<(PrVideoOccurrence, PrMedia)>> {
    let (width, height, frame_rate) = (context.width, context.height, context.frame_rate);
    let ImageSource::Asset(source) = &image.source;
    let asset_id = source.asset_id.as_str();
    if asset_id.is_empty() {
        return Err(unsupported("source.assetId must be nonempty"));
    }
    let facts = match context.media_facts.get(asset_id) {
        Some(MediaFacts::Still(facts)) => facts,
        Some(MediaFacts::Video(_)) => {
            return Err(unsupported(format!(
                "asset {asset_id:?} is used with conflicting media kinds across layers"
            )))
        }
        None => {
            return Err(unsupported(format!(
                "missing inspected media for asset {asset_id:?}"
            )))
        }
    };
    let image_rect = RectBounds::from_size(facts.width.into(), facts.height.into());
    let geometry = if source
        .frame_rect
        .is_some_and(|rect| rect.get() != image_rect)
    {
        Some(format!(
            "its sourceRect is not the {}x{} image at the origin",
            facts.width, facts.height
        ))
    } else if !matches!(source.fit, MediaFit::Contain | MediaFit::None) {
        Some("its media fit is not Contain".to_owned())
    } else {
        None
    };
    if let Some(reason) = geometry {
        omit(
            omissions,
            OmissionScope::Occurrence,
            record,
            format!("still was not exported: {reason}"),
        );
        return Ok(None);
    }
    for (changed, field) in unexported_layer_fields(ClipLayer::Image(image), parent, false) {
        if changed {
            omit_field(
                omissions,
                image.id,
                field,
                record,
                format!("{field} was not exported"),
            );
        }
    }
    for (changed, field, reason) in [
        (
            source.input_transform.is_some(),
            ExportField::InputTransform,
            "input transform was not exported",
        ),
        // An image ignores its legacy time remap, but the field is still lost.
        (
            source.time_remap.is_some(),
            ExportField::TimeRemap,
            "time remap was not exported",
        ),
    ] {
        if changed {
            omit_field(omissions, image.id, field, record, reason);
        }
    }
    if let Some(note) = facts.colour_note() {
        omit(omissions, OmissionScope::Feature, record, note);
    }
    let active_end = image
        .active_range
        .start
        .checked_add_duration(image.active_range.duration)
        .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
    let start_ticks = context.frame_ticks(image.active_range.start, "activeRange.start")?;
    let end_ticks = context.frame_ticks(active_end, "activeRange.end")?;
    ensure!(
        end_ticks > start_ticks,
        "activeRange {}..{} ms collapses to zero duration on the {frame_rate} sequence grid",
        image.active_range.start.as_millis(),
        active_end.as_millis()
    );
    let in_ticks = frame_rate.generator_in_ticks();
    let out_ticks = in_ticks
        .checked_add(end_ticks - start_ticks)
        .filter(|out| *out <= STILL_INTRINSIC_TICKS)
        .ok_or_else(|| {
            unsupported("still placement exceeds Premiere's twelve-hour still duration")
        })?;
    let motion = MotionHost::image(image);
    let mut transform = export_transform(
        motion,
        [facts.width, facts.height],
        [width, height],
        context.dynamics,
        record,
        omissions,
    );
    // Keys are on the still's synthetic clock, from the in-point.
    let mut tracks = context
        .property_tracks
        .remove(&image.id)
        .unwrap_or_default();
    let (animations, _) = export_motion_keys(
        &mut tracks,
        in_ticks,
        &mut transform,
        [facts.width, facts.height],
        [width, height],
        record,
        omissions,
    );
    if let Some(warning) = PrBlendMode::export_approximation(image.blend_mode) {
        approximate(omissions, record, warning);
    }
    Ok(Some((
        PrVideoOccurrence {
            opacity: image.transform.opacity.value(),
            blend_mode: PrBlendMode::from_fx_mode(image.blend_mode),
            transform,
            animations,
            enabled: !image.is_hidden,
            ..PrVideoOccurrence::unedited(
                MediaId(asset_id.to_owned()),
                start_ticks..end_ticks,
                in_ticks..out_ticks,
            )
        },
        PrMedia {
            name: String::new(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: Some(crate::schema::PrVideoStream {
                orientation: crate::schema::VideoOrientation::Identity,
                intrinsic_ticks: STILL_INTRINSIC_TICKS,
                frame_rate: frame_rate.into(),
                width: facts.width,
                height: facts.height,
                kind: PrMediaKind::Still { alpha: facts.alpha },
            }),
            audio: None,
        },
    )))
}

#[cfg(test)]
#[path = "tests/still.rs"]
mod tests;
