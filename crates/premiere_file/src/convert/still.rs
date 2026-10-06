//! Map Premiere still occurrences to editable image layers and back.
//!
//! A still occurrence is a placement, not a media time range: Premiere starts
//! every still one hour into a synthetic twelve-hour clock and gives it the
//! placement's duration. Import therefore keeps only the timeline range, and
//! export rebuilds the synthetic source range from the layer's active range.

use super::{
    effects::{export_effects, omit_unexported_effects, EffectHost},
    nested::LayerExport,
    tesseract_to_premiere::{
        crop_and_opacity_mask, export_motion_keys, export_transform, layer_animations,
        unexported_layer_fields, CanonicalMask, ClipLayer, MotionHost,
    },
};
use crate::{
    approximate,
    error::{ensure, unsupported, Result},
    export_loss::{omit_field, with_context, ExportContext, ExportField, OmissionSink},
    format::{PrMedia, PrVideoOccurrence},
    image_media::ValidatedImage,
    media::MediaFacts,
    omit,
    schema::{MediaId, PrBlendMode, PrMediaKind, STILL_INTRINSIC_TICKS},
    ExportLossDomain, ExportLossSource, OmissionScope,
};
use fx_schema::{
    animator::AnimationGraph, AssetId, BlendMode, ImageAssetSource, ImageLayer, ImageSource,
    LayerId, MediaFit, Position, PositiveRect, RectBounds, TimeRangeProperty, Transform,
};
use std::collections::BTreeMap;

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
/// transform, time remap, effects, corner radius, keys, masks or track matte.
/// A written still carries its Opacity, Motion, their keys, its effects and a
/// Crop or an Opacity mask ([`export_image_layer`]), but a Track Matte Key over
/// a moved, faded, keyed, effected, cropped or masked still is unmeasured, and
/// a matte's coverage must export whole, or the written key gates its clip by a
/// different picture. `canvas` is the sequence size.
pub(super) fn unsupported_matte_still(
    image: &ImageLayer,
    canvas: (u32, u32),
    dynamics: &AnimationGraph,
) -> Option<String> {
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
        (
            !image.masks.is_empty() || image.track_matte.is_some(),
            "masks or track matte",
        ),
    ]
    .into_iter()
    .find_map(|(changed, detail)| changed.then_some(detail))?;
    Some(format!(
        "the track matte source is a still whose {dropped} a matte still does not carry; the exported matte would gate the clip by a different picture"
    ))
}

/// The inspected facts of the still that `source` shows, or why export cannot
/// write it. An empty asset ID and missing or conflicting media facts reject
/// the export. A structurally valid EXR that Premiere's importer cannot expose
/// returns a local picture loss for the editable After Effects fallback. Frame
/// and fit differences do not discard an otherwise writable still; its writer
/// retains the full image and reports the framing approximation. A nest's
/// collapse check takes the same placement decision before writing the nest.
pub(super) fn still_facts<'f>(
    source: &ImageAssetSource,
    media_facts: &'f BTreeMap<String, MediaFacts>,
) -> Result<std::result::Result<&'f ValidatedImage, String>> {
    let asset_id = source.asset_id.as_str();
    if asset_id.is_empty() {
        return Err(unsupported("source.assetId must be nonempty"));
    }
    let facts = match media_facts.get(asset_id) {
        Some(MediaFacts::Still(facts)) => facts,
        Some(MediaFacts::UnsupportedStill(facts)) => return Ok(Err(facts.reason.to_owned())),
        Some(MediaFacts::Video(_) | MediaFacts::UnsupportedVideo(_)) => {
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
    Ok(Ok(facts))
}

/// Map one editable image layer to a still occurrence on Premiere's synthetic
/// still clock, which starts at the generator in-point of the sequence rate
/// ([`FrameRate::generator_in_ticks`](crate::format::FrameRate::generator_in_ticks)).
///
/// The one written form is the still at its pixel size, placed by Motion and
/// Opacity with their keys from the layer's transform, through the video
/// clip's Motion export; Scale to Frame Size is never written. `mask` is what
/// the image's mask exports as
/// ([`image_mask`](super::tesseract_to_premiere::image_mask)), `None`
/// without one, written as a video clip's is: a Crop as the clip's Crop
/// effect, which crops the still's own frame before Motion moves it, as a
/// still's Motion Crop does on import, and an Opacity mask on the clip's
/// Opacity, in unit fractions of the still's frame. The image's effects export
/// as a flat video clip's, in its frame and with their keys on the still's
/// clock, after the Crop, which FX also applies first; Premiere applies an
/// Opacity mask after every effect and FX the image's mask before them, so a
/// still with an Opacity mask writes none of its effects, which are reported.
/// A clip has no frame of its own. When `sourceRect` is not the packaged image
/// at the origin, or fit is Cover, Stretch or Custom, export retains the full
/// packaged image with the existing Motion and reports that framing can differ.
/// Like a video layer, a hidden layer exports disabled, and each property that
/// a clip cannot carry is reported. The dimensions and alpha come from the
/// inspected packaged image, because the document records no alpha fact.
pub(super) fn export_image_layer(
    image: &ImageLayer,
    parent: Option<LayerId>,
    mask: Option<&CanonicalMask>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Option<(PrVideoOccurrence, PrMedia)>> {
    let (width, height, frame_rate) = (context.width, context.height, context.frame_rate);
    let ImageSource::Asset(source) = &image.source;
    let asset_id = source.asset_id.as_str();
    let facts = match still_facts(source, context.media_facts)? {
        Ok(facts) => facts,
        Err(reason) => {
            let reason = format!("still was not exported: {reason}");
            if matches!(
                context.media_facts.get(asset_id),
                Some(MediaFacts::UnsupportedStill(_))
            ) {
                with_context(
                    omissions,
                    ExportContext {
                        source: ExportLossSource::Layer(image.id),
                        domain: ExportLossDomain::Picture,
                    },
                    |sink| omit(sink, OmissionScope::Occurrence, record, reason),
                );
            } else {
                omit(omissions, OmissionScope::Occurrence, record, reason);
            }
            return Ok(None);
        }
    };
    let image_rect = RectBounds::from_size(facts.width.into(), facts.height.into());
    if source
        .frame_rect
        .is_some_and(|rect| rect.get() != image_rect)
    {
        approximate(
            omissions,
            record,
            format!(
                "sourceRect was not the {}x{} image at the origin; the full packaged image was retained, so crop and placement can differ",
                facts.width, facts.height
            ),
        );
    }
    if !matches!(source.fit, MediaFit::Contain | MediaFit::None) {
        approximate(
            omissions,
            record,
            format!(
                "media fit {:?} was approximated by retaining the full packaged image and existing Motion; framing can differ",
                source.fit
            ),
        );
    }
    for (changed, field) in unexported_layer_fields(ClipLayer::Image(image), parent, mask.is_some())
    {
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
    let end_ticks = context.picture_end_ticks(active_end, None)?;
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
    let (crop, opacity_mask) = crop_and_opacity_mask(mask, record, omissions);
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
    let effects = if opacity_mask.is_some() {
        omit_unexported_effects(
            &image.effects,
            "FX applies it after the still's mask and Premiere before an Opacity mask, and a still exports no stage group",
            record,
            omissions,
        );
        Vec::new()
    } else {
        export_effects(
            &image.effects,
            context.dynamics,
            EffectHost {
                layer: image.id,
                still: true,
                staged: false,
                nested: context.in_moved_nest,
                in_nest: context.depth > 0,
                transform: &image.transform,
                source_in: in_ticks,
                video_keys: None,
                static_parameters_reason: None,
                frame: [facts.width, facts.height],
                canvas: [width, height],
            },
            context.written,
            record,
            omissions,
        )
    };
    if let Some(warning) = PrBlendMode::export_approximation(image.blend_mode) {
        approximate(omissions, record, warning);
    }
    Ok(Some((
        PrVideoOccurrence {
            opacity: image.transform.opacity.value(),
            blend_mode: PrBlendMode::from_fx_mode(image.blend_mode),
            transform,
            crop,
            opacity_mask,
            animations,
            // After the Crop, as FX applies the image's mask before them.
            effects,
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
                pixel_aspect: facts.pixel_aspect,
                interpretation: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                intrinsic_ticks: STILL_INTRINSIC_TICKS,
                frame_rate: frame_rate.into(),
                width: facts.width,
                height: facts.height,
                kind: if facts.format == crate::image_media::ImageFormat::OpenExr {
                    PrMediaKind::OpenExr {
                        alpha: facts.alpha,
                        numbered: false,
                        channels: facts
                            .open_exr_channels
                            .unwrap_or(crate::schema::OpenExrChannels::Unspecified),
                    }
                } else {
                    PrMediaKind::Still { alpha: facts.alpha }
                },
            }),
            audio: None,
        },
    )))
}

#[cfg(test)]
#[path = "tests/still.rs"]
mod tests;
