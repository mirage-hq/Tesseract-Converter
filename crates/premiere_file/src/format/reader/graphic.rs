//! Type-tool graphics: one editable text layer over synthetic generator media.
//!
//! The reader classifies a track item as a graphic by its generator media,
//! checks the fixed native parameters fail-closed, and composes a static
//! graphic Vector Motion into the text layer transform while the text stays
//! inside its native ranges. Keyed Vector Motion, and a static one that
//! would take the text outside them, stay separate, as the transform of the
//! whole graphic, and so does the clip's own Opacity, which the video Opacity
//! reader reads. Bezier keys convert only on parameters whose Bezier speeds
//! were measured, in the measured form; other Bezier keys omit the graphic.

use super::{
    animation::{point_keys, read_video_compositing, scalar_keys},
    color_matte, integer, require_zero_subclip_time_offset, required, required_integer,
    stream_dimensions,
    video::{default_chain, master_video_clip, playback_rate, report_markers, scale_to_frame_size},
    visibility,
};
use crate::error::{ensure, unsupported, BuildError, Result};
use crate::format::{shape_payload, text_payload, FrameRate, Graph, Located};
use crate::schema::{
    native::{
        ArbVideoComponentParam, MasterClip, Media, Reference, SubClip, VideoClip,
        VideoClipTrackItem, VideoComponentChain, VideoComponentParam, VideoFilterComponent,
        VideoMediaSource, VideoStream,
    },
    records,
    text::{
        self, GraphicParamRole, GraphicParamSpec, PrGraphicObject, PrShape, PrSourceTextKey,
        PrTextTransform, PrVectorMotion, SHAPE_PARAMS, TEXT_PARAMS, VECTOR_MOTION_PARAMS,
    },
    PrAnimatedProperty, PrGraphic, PrPropertyAnimation, PrText, ToneMapSettings, TICKS,
};
use crate::{omit, Omission, OmissionScope};
use base64::{engine::general_purpose::STANDARD, Engine};

/// The source chain of a track item whose media is the graphic generator.
pub(super) struct GraphicClip {
    sub: Located<SubClip>,
    clip: Located<VideoClip>,
    source: Located<VideoMediaSource>,
    media: Located<Media>,
}

/// Returns the source chain when the item plays synthetic graphic media.
///
/// Any traversal failure returns `None`, so media clips keep the media reader's errors.
pub(super) fn graphic_clip(
    graph: &Graph<'_>,
    item: &Located<VideoClipTrackItem>,
) -> Option<GraphicClip> {
    let sub_reference = item.value.clip_track_item.as_ref()?.sub_clip.as_ref()?;
    let sub = graph
        .follow::<SubClip>(sub_reference, &item.identity)
        .ok()?;
    let clip = graph
        .follow::<VideoClip>(&sub.value.clip, &sub.identity)
        .ok()?;
    let source_reference = clip.value.clip.as_ref()?.source.as_ref()?;
    let source = graph
        .follow::<VideoMediaSource>(source_reference, &clip.identity)
        .ok()?;
    let media_reference = source.value.media_source.as_ref()?.media.as_ref()?;
    if color_matte::is_color_matte_media(graph.locate(media_reference, &source.identity).ok()?) {
        return None;
    }
    let media = graph
        .follow::<Media>(media_reference, &source.identity)
        .ok()?;
    (media.value.implementation_id.as_deref() == Some(text::GRAPHIC_IMPLEMENTATION_ID)).then_some(
        GraphicClip {
            sub,
            clip,
            source,
            media,
        },
    )
}

/// Read one graphic occurrence. An error omits the occurrence.
pub(super) fn read_graphic(
    graph: &Graph<'_>,
    item: Located<VideoClipTrackItem>,
    graphic: GraphicClip,
    frame: [u32; 2],
    frame_rate: FrameRate,
    omissions: &mut Vec<Omission>,
) -> Result<PrGraphic> {
    let identity = item.identity.clone();
    let frame_rect = format!("0,0,{},{}", frame[0], frame[1]);
    if let Some(tone_map_settings) = &item.value.tone_map_settings {
        let value: ToneMapSettings = serde_json::from_str(tone_map_settings)?;
        if value != ToneMapSettings::DEFAULT {
            omit(
                omissions,
                OmissionScope::Feature,
                &identity,
                "nondefault tone mapping not converted",
            );
        }
    }
    ensure!(
        item.value.frame_rect.as_deref() == Some(frame_rect.as_str()),
        "{identity}: unsupported graphic geometry"
    );
    if let Some(pixel_aspect_ratio) = item.value.pixel_aspect_ratio.as_deref() {
        let ratio = records::PixelAspectRatio::parse(pixel_aspect_ratio, &identity)?;
        ensure!(
            ratio.is_square(),
            "{identity}: unsupported graphic geometry: non-square pixels ({ratio})"
        );
    }
    let track_item = required(
        item.value.clip_track_item.as_ref(),
        &identity,
        "ClipTrackItem",
    )?;
    require_zero_subclip_time_offset(track_item, &identity)?;
    let enabled = !visibility::is_muted(track_item.is_muted.as_deref(), &identity)?;
    let range = required(track_item.track_item.as_ref(), &identity, "TrackItem")?;
    let start = match range.start.as_deref() {
        Some(value) => integer(value, &format!("{identity}: invalid Start"))?,
        None => 0,
    };
    let end = integer(&range.end, &format!("{identity}: invalid End"))?;

    let GraphicClip {
        sub,
        clip,
        source,
        media,
    } = graphic;
    let placed = required(clip.value.clip.as_ref(), &clip.identity, "Clip")?;
    ensure!(
        !scale_to_frame_size(&clip.value, &clip.identity)?,
        "{}: Scale to Frame Size on a graphic is not converted",
        clip.identity
    );
    // Speed, reverse playback, time remapping and holds all retime the graphic.
    ensure!(
        playback_rate(placed, &clip.identity)? == 1.0
            && placed.time_remapping.is_none()
            && !clip.value.declares_frame_hold(),
        "{}: graphic retiming is unsupported",
        clip.identity
    );
    report_markers(graph, placed, &clip.identity, omissions);
    let source_in = required_integer(placed.in_point.as_deref(), &clip.identity, "InPoint")?;
    let source_out = required_integer(placed.out_point.as_deref(), &clip.identity, "OutPoint")?;
    ensure!(
        source_in >= 0 && source_out.checked_sub(source_in) == end.checked_sub(start),
        "{}: graphic retiming is unsupported",
        clip.identity
    );
    validate_generator(graph, &source, &media, &frame_rect, frame_rate)?;
    if let Some(master) = &sub.value.master_clip {
        validate_master(graph, master, &sub.identity, &source)?;
    }

    let components = required(
        track_item
            .component_owner
            .as_ref()
            .and_then(|owner| owner.components.as_ref()),
        &identity,
        "graphic component chain",
    )?;
    let chain = graph.follow::<VideoComponentChain>(components, &identity)?;
    // Check this before `default_chain`, which would also report it as a
    // feature. Clip Motion keys are not converted and keep this message.
    ensure!(
        chain.value.default_motion.as_deref() == Some("true"),
        "{}: graphic clip Motion and Opacity must keep their defaults",
        chain.identity
    );
    default_chain(graph, components, &identity, omissions);
    let layers = chain
        .value
        .component_chain
        .as_ref()
        .and_then(|content| content.components.as_ref())
        .map_or(&[][..], |components| components.items.as_slice());
    // Premiere 26.5.1 lists a kept clip Opacity first. A chain with neither a `DefaultOpacity`
    // nor an Opacity component reads 100: inferred (video reader's fallback), not Adobe-observed.
    let (clip_opacity, layers) = match layers.split_first() {
        Some((first, rest)) if is_opacity(graph, first, &chain.identity) => (Some(first), rest),
        _ => (None, layers),
    };
    let (opacity, blend_mode, opacity_animation, opacity_mask) =
        read_video_compositing(graph, &chain, clip_opacity.as_slice(), omissions)?;
    // Graphic masks are JRB-2083: the mask frame of a generator clip is
    // unmeasured.
    ensure!(
        opacity_mask.is_none(),
        "{}: a mask on a graphic clip Opacity is not converted (JRB-2083)",
        chain.identity
    );
    if let Some(reference) = clip_opacity {
        // Its Bezier speeds were measured in value per second, as on the
        // probed graphic parameters, with the same unmeasured form.
        let (param, wire) = opacity_keys(graph, reference, &chain.identity)?;
        ensure!(
            !bent_bezier_after_linear_or_hold(&wire, None),
            "{param}: Bezier key after a Linear or Hold key on graphic clip Opacity is unsupported until its interpolation is verified"
        );
    }
    // The Vector Motion comes first, after a clip Opacity: an order
    // inferred, not seen in an Adobe save.
    let (group, object_references) = match layers.split_first() {
        Some((first, rest))
            if match_name(graph, first, &chain.identity).as_deref()
                == Some(text::VECTOR_MOTION_MATCH_NAME) =>
        {
            (
                Some(read_vector_motion(graph, first, &chain.identity, frame)?),
                rest,
            )
        }
        _ => (None, layers),
    };
    ensure!(
        !object_references.is_empty(),
        "{}: a graphic without text or shape objects is unsupported",
        chain.identity
    );
    let mut objects = Vec::with_capacity(object_references.len());
    for reference in object_references {
        objects.push(read_object(
            graph,
            reference,
            &chain.identity,
            frame,
            &identity,
            omissions,
        )?);
    }
    let vector_motion = match group {
        Some(motion)
            if motion.animations.is_empty()
                && objects.len() == 1
                && objects[0].compose_static_vector_motion_in_range(&motion, frame) =>
        {
            None
        }
        motion => motion,
    };
    let graphic = PrGraphic {
        id: Some(identity),
        start_ticks: start,
        end_ticks: end,
        in_ticks: source_in,
        vector_motion,
        opacity,
        blend_mode,
        animations: opacity_animation.into_iter().collect(),
        objects,
        enabled,
    };
    graphic.validate(frame_rate)?;
    Ok(graphic)
}

/// The match name of a chain component that resolves.
fn match_name(graph: &Graph<'_>, reference: &Reference, from: &str) -> Option<String> {
    graph
        .follow::<VideoFilterComponent>(reference, from)
        .ok()?
        .value
        .match_name
}

/// Read one graphic object: a Text or a Shape. Every other component
/// between or after the objects omits the graphic.
fn read_object(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    frame: [u32; 2],
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<PrGraphicObject> {
    let component = graph.follow::<VideoFilterComponent>(reference, from)?;
    ensure!(
        component.value.sub_components.is_none(),
        "{}: a mask on a graphic object is not converted (JRB-2083)",
        component.identity
    );
    let pixels = frame_pixels(frame);
    match component.value.match_name.as_deref() {
        Some(text::TEXT_MATCH_NAME) => {
            let layer = read_text_component(graph, component)?;
            for feature in &layer.omitted {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!("{feature} not converted"),
                );
            }
            let transform = object_transform(&layer.transform, pixels);
            Ok(match layer.document {
                text_payload::TextDocuments::Uniform(document) => PrGraphicObject::Text(PrText {
                    name: layer.name,
                    document,
                    transform,
                    animations: layer.animations,
                    source_text_keys: layer.source_text_keys,
                }),
                text_payload::TextDocuments::Lines(documents) => {
                    PrGraphicObject::TextLines(text::PrTextLines {
                        name: layer.name,
                        documents,
                        transform,
                        animations: layer.animations,
                    })
                }
            })
        }
        Some(text::SHAPE_MATCH_NAME) => {
            read_shape_component(graph, component, pixels).map(PrGraphicObject::Shape)
        }
        Some(text::SUBGROUP_MATCH_NAME) => Err(unsupported(format!(
            "{}: graphic SubGroups are unsupported",
            component.identity
        ))),
        Some(text::VECTOR_MOTION_MATCH_NAME) | None => Err(unsupported(format!(
            "{}: unsupported graphic component",
            component.identity
        ))),
        Some(effect) => Err(unsupported(format!(
            "{}: effect {effect:?} in a graphic is unsupported",
            component.identity
        ))),
    }
}

/// An object transform in sequence pixels.
fn object_transform(
    transform: &NormalizedTransform,
    pixels: impl Fn([f64; 2]) -> [f64; 2],
) -> PrTextTransform {
    PrTextTransform {
        position: pixels(transform.position),
        anchor: pixels(transform.anchor),
        scale: transform.scale,
        rotation: transform.rotation,
        opacity: transform.opacity,
    }
}

/// Whether a chain component is the clip's intrinsic Opacity.
fn is_opacity(graph: &Graph<'_>, reference: &Reference, from: &str) -> bool {
    graph
        .follow::<VideoFilterComponent>(reference, from)
        .is_ok_and(|component| component.value.match_name.as_deref() == Some("AE.ADBE Opacity"))
}

/// The record identity and key list of the Opacity parameter of a clip
/// Opacity component that the video Opacity reader has accepted.
fn opacity_keys(graph: &Graph<'_>, reference: &Reference, from: &str) -> Result<(String, String)> {
    let component = graph.follow::<VideoFilterComponent>(reference, from)?;
    let params = component
        .value
        .component
        .and_then(|body| body.params)
        .ok_or_else(|| unsupported(format!("{}: missing Opacity Params", component.identity)))?;
    for reference in &params.items {
        let record = graph.locate(reference, &component.identity)?;
        let input = graph.decode::<VideoComponentParam>(record)?;
        if input.value.parameter_id == "1" {
            return Ok((input.identity, input.value.keyframes.unwrap_or_default()));
        }
    }
    Err(unsupported(format!(
        "{}: missing Opacity parameter",
        component.identity
    )))
}

/// Converts a point normalized to the frame into frame pixels.
fn frame_pixels(frame: [u32; 2]) -> impl Fn([f64; 2]) -> [f64; 2] {
    let size = frame.map(f64::from);
    move |point| [point[0] * size[0], point[1] * size[1]]
}

fn validate_generator(
    graph: &Graph<'_>,
    source: &Located<VideoMediaSource>,
    media: &Located<Media>,
    frame_rect: &str,
    frame_rate: FrameRate,
) -> Result<()> {
    ensure!(
        source.value.original_duration.as_deref() == Some(&text::GRAPHIC_MEDIA_TICKS.to_string()),
        "{}: unexpected graphic source duration",
        source.identity
    );
    let value = &media.value;
    ensure!(
        [&value.file_path, &value.actual_media_file_path]
            .iter()
            .all(|path| path
                .as_deref()
                .is_none_or(|path| path == text::GRAPHIC_MEDIA_TOKEN))
            && value.relative_paths.is_empty()
            && value.infinite.as_deref() == Some("true")
            && value.audio_stream.is_none(),
        "{}: unexpected graphic generator media",
        media.identity
    );
    let stream_reference = required(
        value.video_stream.as_ref(),
        &media.identity,
        records::VIDEO_STREAM.tag,
    )?;
    let stream = graph.follow::<VideoStream>(stream_reference, &media.identity)?;
    // Object sizes are stream pixels, so the stream needs the strict frame and
    // square pixels of every other stream before it is matched to its sequence.
    stream_dimensions(&stream)?;
    ensure!(
        stream.value.frame_rect.as_deref() == Some(frame_rect)
            && stream.value.frame_rate.as_deref()
                == Some(&frame_rate.ticks_per_frame().to_string()),
        "{}: graphic frame size or rate differs from its sequence",
        stream.identity
    );
    Ok(())
}

fn validate_master(
    graph: &Graph<'_>,
    master: &Reference,
    from: &str,
    source: &Located<VideoMediaSource>,
) -> Result<()> {
    let master = graph.follow::<MasterClip>(master, from)?;
    let template = master_video_clip(graph, &master)?;
    let template_clip = required(template.value.clip.as_ref(), &template.identity, "Clip")?;
    let template_source = required(template_clip.source.as_ref(), &template.identity, "Source")?;
    ensure!(
        graph
            .locate(template_source, &template.identity)?
            .identity()
            == source.identity,
        "{}: graphic source identity mismatch",
        master.identity
    );
    Ok(())
}

/// A static transform whose position and anchor are normalized to the frame.
struct NormalizedTransform {
    position: [f64; 2],
    anchor: [f64; 2],
    scale: f64,
    /// A Shape's Horizontal Scale and Uniform Scale switch.
    horizontal_scale: f64,
    uniform: bool,
    rotation: f64,
    opacity: f64,
}

struct TextComponent {
    name: String,
    document: text_payload::TextDocuments,
    transform: NormalizedTransform,
    animations: Vec<PrPropertyAnimation>,
    source_text_keys: Vec<PrSourceTextKey>,
    omitted: Vec<text_payload::OmittedTextFeature>,
}

fn read_vector_motion(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    frame: [u32; 2],
) -> Result<PrVectorMotion> {
    let component = graph.follow::<VideoFilterComponent>(reference, from)?;
    ensure!(
        component.value.sub_components.is_none(),
        "{}: a mask on a graphic Vector Motion is not converted (JRB-2083)",
        component.identity
    );
    let body = required(
        component.value.component.as_ref(),
        &component.identity,
        "Component",
    )?;
    ensure!(
        component.value.match_name.as_deref() == Some(text::VECTOR_MOTION_MATCH_NAME)
            && body.display_name.as_deref() == Some("Vector Motion")
            && body.intrinsic.as_deref() == Some("true")
            && body.bypass.as_deref().is_none_or(|value| value == "false"),
        "{}: unsupported graphic component",
        component.identity
    );
    let params = required(body.params.as_ref(), &component.identity, "Params")?;
    let (transform, animations) = read_params(
        graph,
        &params.items,
        &VECTOR_MOTION_PARAMS,
        GraphicComponent::VectorMotion,
        &component.identity,
    )?;
    let pixels = frame_pixels(frame);
    Ok(PrVectorMotion {
        position: pixels(transform.position),
        anchor: pixels(transform.anchor),
        scale: transform.scale,
        rotation: transform.rotation,
        animations,
    })
}

fn read_text_component(
    graph: &Graph<'_>,
    component: Located<VideoFilterComponent>,
) -> Result<TextComponent> {
    let body = required(
        component.value.component.as_ref(),
        &component.identity,
        "Component",
    )?;
    ensure!(
        body.bypass.as_deref() != Some("true"),
        "{}: a bypassed graphic object is unsupported",
        component.identity
    );
    ensure!(
        component.value.match_name.as_deref() == Some(text::TEXT_MATCH_NAME)
            && body.display_name.as_deref() == Some("Text")
            && body.bypass.as_deref().is_none_or(|value| value == "false"),
        "{}: unsupported graphic component",
        component.identity
    );
    let params = required(body.params.as_ref(), &component.identity, "Params")?;
    let (source_text, transform_params) = params
        .items
        .split_first()
        .ok_or_else(|| unsupported(format!("{}: missing Source Text", component.identity)))?;
    let source_text = read_source_text(graph, source_text, &component.identity)?;

    let (transform, animations) = read_params(
        graph,
        transform_params,
        &TEXT_PARAMS,
        GraphicComponent::Text,
        &component.identity,
    )?;
    Ok(TextComponent {
        name: body.instance_name.clone().unwrap_or_default(),
        document: source_text.document,
        transform,
        animations,
        source_text_keys: source_text.keys,
        omitted: source_text.omitted,
    })
}

/// A Text's Source Text as read: the document shown before the first key,
/// the keys, and the features of any of them that the model does not keep.
struct SourceText {
    document: text_payload::TextDocuments,
    keys: Vec<PrSourceTextKey>,
    omitted: Vec<text_payload::OmittedTextFeature>,
}

/// Read a Text's Source Text: its static value and its keys, each one
/// complete document. Premiere stores Source Text keys as
/// `ticks,base64;…;` with no interpolation fields (three Premiere 14 corpus
/// projects and the Premiere 26.5.1 save), so the text holds between keys.
/// Premiere 26.5.1 renders the first key's document before the first key
/// and never the stored `StartKeyframeValue` (measured on the Source Text
/// keys fixture, whose saved start values differ from the first keys), so
/// that value is decoded, so a malformed one still fails closed, and
/// otherwise unused with keys.
fn read_source_text(graph: &Graph<'_>, reference: &Reference, from: &str) -> Result<SourceText> {
    let source = arb_param(graph, reference, from, ("1", "Source Text"))?;
    let wire = source.value.keyframes.as_deref().unwrap_or_default();
    // Keys need an absent or true flag; a flag without keys must not be
    // true. Premiere 14 wrote keys without the flag.
    ensure!(
        match source.value.is_time_varying.as_deref() {
            None => true,
            Some("true") => !wire.is_empty(),
            Some("false") => wire.is_empty(),
            Some(_) => false,
        },
        "{}: animated or unknown Source Text is unsupported",
        source.identity
    );
    let stored = static_value(graph, &source, "Source Text", text_payload::decode_graphic)?;
    ensure!(
        wire.is_empty() || matches!(stored.documents, text_payload::TextDocuments::Uniform(_)),
        "{}: keyed mixed text styles are unsupported",
        source.identity
    );
    let mut omitted = stored.omitted;
    let mut keys = Vec::new();
    if !wire.is_empty() {
        ensure!(
            wire.ends_with(';'),
            "{}: unterminated Source Text key list",
            source.identity
        );
        for key in wire.split_terminator(';') {
            let Some((ticks, encoded)) = key.split_once(',') else {
                return Err(unsupported(format!(
                    "{}: unexpected Source Text key shape",
                    source.identity
                )));
            };
            let source_ticks = ticks.parse::<i64>().map_err(|_| {
                unsupported(format!("{}: invalid Source Text key time", source.identity))
            })?;
            ensure!(
                keys.last()
                    .is_none_or(|last: &PrSourceTextKey| last.source_ticks < source_ticks),
                "{}: Source Text keys must have strictly increasing source times",
                source.identity
            );
            let decoded = decode_stored(
                encoded,
                &source.identity,
                "Source Text",
                text_payload::decode,
            )?;
            for feature in decoded.omitted {
                if !omitted.contains(&feature) {
                    omitted.push(feature);
                }
            }
            keys.push(PrSourceTextKey {
                source_ticks,
                document: decoded.document,
            });
        }
    }
    let document = keys.first().map_or(stored.documents, |first| {
        text_payload::TextDocuments::Uniform(first.document.clone())
    });
    Ok(SourceText {
        document,
        keys,
        omitted,
    })
}

/// Read one static Shape object: its Path and Appearance, and a transform
/// without keys.
fn read_shape_component(
    graph: &Graph<'_>,
    component: Located<VideoFilterComponent>,
    pixels: impl Fn([f64; 2]) -> [f64; 2],
) -> Result<PrShape> {
    let body = required(
        component.value.component.as_ref(),
        &component.identity,
        "Component",
    )?;
    ensure!(
        body.bypass.as_deref() != Some("true"),
        "{}: a bypassed graphic object is unsupported",
        component.identity
    );
    ensure!(
        body.display_name.as_deref() == Some("Shape")
            && body.intrinsic.is_none()
            && body.bypass.as_deref().is_none_or(|value| value == "false"),
        "{}: unsupported graphic component",
        component.identity
    );
    let params = required(body.params.as_ref(), &component.identity, "Params")?;
    let [path, appearance, transform_params @ ..] = params.items.as_slice() else {
        return Err(unsupported(format!(
            "{}: unsupported graphic parameter layout",
            component.identity
        )));
    };
    let path = binary_param(
        graph,
        path,
        &component.identity,
        ("1", "Path"),
        shape_payload::decode_path,
    )?;
    let appearance = binary_param(
        graph,
        appearance,
        &component.identity,
        ("2", "Appearance"),
        shape_payload::decode_appearance,
    )?;
    let (transform, animations) = read_params(
        graph,
        transform_params,
        &SHAPE_PARAMS,
        GraphicComponent::Shape,
        &component.identity,
    )?;
    ensure!(
        animations.is_empty(),
        "{}: keyed graphic Shape parameters are unsupported",
        component.identity
    );
    // Under Uniform Scale, a Horizontal Scale other than 100 was not rendered.
    let horizontal_scale = match (transform.uniform, transform.horizontal_scale) {
        (true, 100.0) => None,
        (true, _) => {
            return Err(unsupported(format!(
                "{}: Horizontal Scale under Uniform Scale is unverified",
                component.identity
            )))
        }
        (false, scale) => Some(scale),
    };
    if appearance.stroke.is_some() {
        text::stroke_join(&path, None)
            .map_err(|reason| unsupported(format!("{}: {reason}", component.identity)))?;
    }
    Ok(PrShape {
        name: body.instance_name.clone().unwrap_or_default(),
        path,
        appearance,
        transform: object_transform(&transform, pixels),
        horizontal_scale,
    })
}

/// Decode the static binary value of an object parameter `(ParameterID,
/// Name)` that is stored like Source Text: inline base64, or a `BinaryHash`
/// that names an earlier copy. Keys fail closed.
pub(super) fn binary_param<T>(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    (id, name): (&str, &str),
    decode: impl Fn(&[u8]) -> Result<T>,
) -> Result<T> {
    let source = arb_param(graph, reference, from, (id, name))?;
    ensure!(
        source.value.keyframes.is_none()
            && source
                .value
                .is_time_varying
                .as_deref()
                .is_none_or(|flag| flag == "false"),
        "{}: animated or unknown {name} is unsupported",
        source.identity
    );
    static_value(graph, &source, name, decode)
}

/// The binary parameter record `(ParameterID, Name)` of an object.
fn arb_param(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    (id, name): (&str, &str),
) -> Result<Located<ArbVideoComponentParam>> {
    let source = graph.follow::<ArbVideoComponentParam>(reference, from)?;
    ensure!(
        source.value.parameter_id == id
            && source.value.name.as_deref() == Some(name)
            && source
                .value
                .start_keyframe_position
                .as_deref()
                .is_none_or(|time| time == records::STATIC_KEYFRAME_TIME),
        "{}: animated or unknown {name} is unsupported",
        source.identity
    );
    Ok(source)
}

/// Decode the `StartKeyframeValue` of a binary parameter: inline base64, or
/// a `BinaryHash` that names an earlier copy.
fn static_value<T>(
    graph: &Graph<'_>,
    source: &Located<ArbVideoComponentParam>,
    name: &str,
    decode: impl Fn(&[u8]) -> Result<T>,
) -> Result<T> {
    let encoded = required(
        source.value.start_keyframe_value.as_ref(),
        &source.identity,
        "StartKeyframeValue",
    )?;
    ensure!(
        encoded.encoding == records::ENCODING,
        "{}: unsupported {name} encoding",
        source.identity
    );
    let stored = if encoded.value.trim().is_empty() {
        let hash = required(
            encoded.binary_hash.as_deref(),
            &source.identity,
            "BinaryHash",
        )?;
        graph.binary_value(hash, &source.identity)?.ok_or_else(|| {
            unsupported(format!(
                "{}: {name} names missing binary {hash}",
                source.identity
            ))
        })?
    } else {
        encoded.value.as_str()
    };
    decode_stored(stored, &source.identity, name, decode)
}

/// Decode one base64 binary value of the parameter `name` of `record`.
fn decode_stored<T>(
    stored: &str,
    record: &str,
    name: &str,
    decode: impl Fn(&[u8]) -> Result<T>,
) -> Result<T> {
    let compact: String = stored.split_whitespace().collect();
    let payload = STANDARD
        .decode(compact)
        .map_err(|error| unsupported(format!("{record}: invalid {name} base64: {error}")))?;
    decode(&payload).map_err(|error| match error {
        BuildError::Unsupported(message) => unsupported(format!("{record}: {message}")),
        other => other,
    })
}

/// Decode parameters in native order, check fixed ones fail-closed, and read
/// the transform and the keys of the parameters that can hold them from the
/// others. Keys keep their generator-clock times, and Bezier timing needs a
/// parameter with verified Bezier speeds.
/// The native owner matters for measured interpolation. These reader-only
/// admissions must not change the parameter specs also used by export.
#[derive(Clone, Copy)]
enum GraphicComponent {
    Text,
    Shape,
    VectorMotion,
}

fn read_params(
    graph: &Graph<'_>,
    references: &[Reference],
    specs: &[GraphicParamSpec],
    component: GraphicComponent,
    owner: &str,
) -> Result<(NormalizedTransform, Vec<PrPropertyAnimation>)> {
    ensure!(
        references.len() == specs.len(),
        "{owner}: unsupported graphic parameter layout"
    );
    // Each layout sets every field that it has; Vector Motion has no opacity.
    let mut transform = NormalizedTransform {
        position: [0.5; 2],
        anchor: [0.0; 2],
        scale: 100.0,
        horizontal_scale: 100.0,
        uniform: true,
        rotation: 0.0,
        opacity: 100.0,
    };
    let mut animations = Vec::new();
    for (reference, spec) in references.iter().zip(specs) {
        let record = graph.locate(reference, owner)?;
        ensure!(
            record.tag() == spec.record.tag,
            "{}: unexpected graphic parameter record",
            record.identity()
        );
        let input = graph.decode::<VideoComponentParam>(record)?;
        let param = &input.value;
        ensure!(
            param.parameter_id == spec.id.to_string() && param.name.as_deref() == spec.name,
            "{}: unexpected graphic parameter {:?}",
            input.identity,
            param.name
        );
        let wire = param.keyframes.as_deref().unwrap_or_default();
        // Keys need an absent or true flag; a flag without keys must not be true.
        ensure!(
            match param.is_time_varying.as_deref() {
                None => true,
                Some("true") => !wire.is_empty(),
                Some("false") => wire.is_empty(),
                Some(_) => false,
            },
            "{}: animated or unknown graphic parameters are unsupported",
            input.identity
        );
        if !wire.is_empty() {
            let name = spec.name.unwrap_or("(unnamed)");
            let property = spec.role.animation().ok_or_else(|| {
                unsupported(format!(
                    "{}: animated graphic {name:?} is unsupported",
                    input.identity
                ))
            })?;
            let animation = match property {
                PrAnimatedProperty::Position => {
                    PrPropertyAnimation::Position(point_keys(wire, &input.identity)?)
                }
                PrAnimatedProperty::UniformScale => {
                    PrPropertyAnimation::UniformScale(scalar_keys(wire, &input.identity)?)
                }
                PrAnimatedProperty::Rotation => {
                    PrPropertyAnimation::Rotation(scalar_keys(wire, &input.identity)?)
                }
                PrAnimatedProperty::Opacity => {
                    PrPropertyAnimation::Opacity(scalar_keys(wire, &input.identity)?)
                }
                // No graphic role keys these clip Motion properties.
                PrAnimatedProperty::AnchorPoint | PrAnimatedProperty::ScaleWidth => {
                    return Err(unsupported(format!(
                        "{}: animated graphic {name:?} is unsupported",
                        input.identity
                    )));
                }
            };
            // C1 measured straight Bezier→Bezier Position on Text/Vector
            // Motion, Text Rotation in stored degrees/second, Linear→Bezier
            // Text Scale and Hold→Bezier Text Opacity. Keep every other
            // unmeasured combination behind the original guards.
            if spec.bezier_speeds_verified {
                let measured_start = match (component, property) {
                    (GraphicComponent::Text, PrAnimatedProperty::UniformScale) => Some(0),
                    (GraphicComponent::Text, PrAnimatedProperty::Opacity) => Some(4),
                    _ => None,
                };
                ensure!(
                    !bent_bezier_after_linear_or_hold(wire, measured_start),
                    "{}: Bezier key after a Linear or Hold key on graphic {name:?} is unsupported until its interpolation is verified",
                    input.identity
                );
            } else {
                let measured = match (component, property) {
                    (
                        GraphicComponent::Text | GraphicComponent::VectorMotion,
                        PrAnimatedProperty::Position,
                    ) => bezier_keys_only(wire) && animation_is_straight(&animation),
                    (GraphicComponent::Text, PrAnimatedProperty::Rotation) => {
                        bezier_keys_only(wire)
                    }
                    _ => false,
                };
                ensure!(
                    !has_temporal_bezier(wire) || measured,
                    "{}: Bezier keys on graphic {name:?} are unsupported until their speed unit is verified",
                    input.identity
                );
            }
            animations.push(animation);
        }
        let fields: Vec<_> = param.start_keyframe.split(',').collect();
        ensure!(
            fields.len() == if spec.is_point() { 14 } else { 8 }
                && fields[0] == records::STATIC_KEYFRAME_TIME,
            "{}: unexpected graphic parameter keyframe shape",
            input.identity
        );
        let value = fields[1];
        match spec.role {
            GraphicParamRole::Position => transform.position = point(value, owner)?,
            GraphicParamRole::Anchor => transform.anchor = point(value, owner)?,
            GraphicParamRole::Scale => transform.scale = number(value, owner)?,
            GraphicParamRole::HorizontalScale => {
                transform.horizontal_scale = number(value, owner)?;
            }
            GraphicParamRole::Uniform => {
                transform.uniform = match value {
                    "true" => true,
                    "false" => false,
                    _ => {
                        return Err(unsupported(format!(
                            "{owner}: invalid graphic Uniform Scale value"
                        )))
                    }
                };
            }
            GraphicParamRole::Rotation => transform.rotation = number(value, owner)?,
            GraphicParamRole::Opacity => transform.opacity = number(value, owner)?,
            GraphicParamRole::Selection => {}
            GraphicParamRole::Fixed => ensure!(
                spec.accepts_default(value),
                "{}: nondefault graphic parameter {:?} is unsupported",
                input.identity,
                spec.name.unwrap_or("(unnamed)")
            ),
        }
    }
    Ok((transform, animations))
}

/// C1 measured segments with Bezier on both keys, not mixed point/Rotation
/// modes or a lone Bezier key. The key reader already validated this wire.
fn bezier_keys_only(wire: &str) -> bool {
    let mut keys = wire.split_terminator(';').peekable();
    let Some(first) = keys.next() else {
        return false;
    };
    let is_bezier = |key: &str| {
        key.split(',')
            .nth(2)
            .and_then(|mode| mode.parse::<u8>().ok())
            == Some(5)
    };
    keys.peek().is_some() && is_bezier(first) && keys.all(is_bezier)
}

/// Temporal Position speed was measured only on straight, tangent-free paths.
fn animation_is_straight(animation: &PrPropertyAnimation) -> bool {
    let PrPropertyAnimation::Position(keys) = animation else {
        return false;
    };
    keys.iter()
        .all(|key| key.spatial_in_tangent.is_none() && key.spatial_out_tangent.is_none())
}

/// Whether a key list that the Motion key readers accepted has a key with
/// temporal Bezier interpolation (mode `5` in its third field), wherever it
/// is: Premiere stores one mode per key. A point key's spatial mode is a
/// later field.
fn has_temporal_bezier(wire: &str) -> bool {
    wire.split_terminator(';').any(|key| {
        key.split(',')
            .nth(2)
            .and_then(|mode| mode.parse::<u8>().ok())
            == Some(5)
    })
}

/// Whether a scalar key list that the Motion key reader accepted has a
/// temporal Bezier key right after a Linear or Hold key whose incoming handle
/// bends the segment into it, or right after a Linear key whose stored
/// outgoing handle bends it. The Motion reader eases a Linear key into a
/// Bezier key with both handles (F23, probed on clip Motion Scale only) and
/// holds after a Hold key; C1 measured Text Scale after Linear and Text
/// Opacity after Hold, passed as `measured_start`. For other owners only
/// neutral handles leave the segment straight either way: an influence of
/// exactly zero, or a speed exactly equal to the segment's average value per
/// second. Any other handle counts as bent, however close.
fn bent_bezier_after_linear_or_hold(wire: &str, measured_start: Option<u8>) -> bool {
    struct Key {
        ticks: i64,
        value: f64,
        mode: u8,
        incoming_speed: f64,
        incoming_influence: f64,
        outgoing_speed: f64,
        outgoing_influence: f64,
    }
    let keys: Option<Vec<Key>> = wire
        .split_terminator(';')
        .map(|key| {
            let fields: Vec<&str> = key.split(',').collect();
            Some(Key {
                ticks: fields.first()?.parse().ok()?,
                value: fields.get(1)?.parse().ok()?,
                mode: fields.get(2)?.parse().ok()?,
                incoming_speed: fields.get(4)?.parse().ok()?,
                incoming_influence: fields.get(5)?.parse().ok()?,
                outgoing_speed: fields.get(6)?.parse().ok()?,
                outgoing_influence: fields.get(7)?.parse().ok()?,
            })
        })
        .collect();
    // The Motion reader has checked every field; anything else fails closed.
    let Some(keys) = keys else {
        return true;
    };
    keys.windows(2).any(|pair| {
        let [start, end] = pair else {
            return false;
        };
        // Increasing key times may span the whole i64 range: widen, as the
        // Motion Bezier reader does, before subtracting.
        let duration_secs = (i128::from(end.ticks) - i128::from(start.ticks)) as f64 / TICKS as f64;
        let average_speed = (end.value - start.value) / duration_secs;
        let bends = |speed: f64, influence: f64| influence != 0.0 && speed != average_speed;
        matches!(start.mode, 0 | 4)
            && Some(start.mode) != measured_start
            && end.mode == 5
            && (bends(end.incoming_speed, end.incoming_influence)
                || start.mode == 0 && bends(start.outgoing_speed, start.outgoing_influence))
    })
}

fn number(value: &str, context: &str) -> Result<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| unsupported(format!("{context}: invalid graphic parameter value")))
}

fn point(value: &str, context: &str) -> Result<[f64; 2]> {
    let (x, y) = value
        .split_once(':')
        .ok_or_else(|| unsupported(format!("{context}: invalid graphic point value")))?;
    Ok([number(x, context)?, number(y, context)?])
}
