//! Type-tool graphics: one editable text layer over synthetic generator media.
//!
//! The reader classifies a track item as a graphic by its generator media,
//! checks the fixed native parameters fail-closed, and composes a static
//! graphic Vector Motion into the text layer transform while the text stays
//! inside its native ranges. Keyed Vector Motion, and a static one that
//! would take the text outside them, stay separate, as the transform of the
//! whole graphic, and so does the clip's own Opacity, which the video Opacity
//! reader reads with its one clip mask and bounded numeric controls. Bezier keys convert only on
//! parameters whose Bezier speeds were measured, in the measured form; other
//! Bezier keys omit the graphic, except Shape Scale's diagnosed scalar approximation.
//!
//! A Source Graphic keeps its objects on its master clip's own chain, which
//! every placement of that master shows; each placement reads them afresh,
//! beside its own timing, static clip Motion and clip Opacity. Keys in that
//! shared chain omit the placement: their clock is unmeasured.

mod capsule;
mod legacy_title;

use super::{
    animation::{
        point_keys, point_start, read_video_animations, read_video_compositing, scalar_keys,
        MotionAndMasks,
    },
    color_matte, integer, require_zero_subclip_time_offset, required, required_integer,
    stream_dimensions,
    video::{default_chain, master_video_clip, playback_rate, report_markers, scale_to_frame_size},
    visibility,
};
use crate::error::{ensure, unsupported, BuildError, Result};
use crate::format::{shape_payload, text_payload, FrameRate, Graph, Located};
use crate::schema::{
    native::{
        ArbVideoComponentParam, ComponentPinSerializer, ComponentPinVectorSerializer, MasterClip,
        Media, Reference, SubClip, VideoClip, VideoClipTrackItem, VideoComponentChain,
        VideoComponentParam, VideoFilterComponent, VideoMediaSource, VideoStream,
    },
    records,
    text::{
        self, omitted_part, unverified_mask_composite, GraphicParamRole, GraphicParamSpec,
        PrGraphicGroup, PrGraphicObject, PrMaskSource, PrShape, PrSourceTextKey, PrTextTransform,
        PrVectorMotion, LEGACY_TEXT_PARAM_COUNT, SHAPE_PARAMS, TEXT_PARAMS, VECTOR_MOTION_PARAMS,
    },
    PrAnimatedProperty, PrGraphic, PrGraphicEffectLoss, PrPropertyAnimation, PrStaticTransform,
    PrText, ToneMapSettings, TICKS,
};
use crate::{approximate, omit, Omission, OmissionScope};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::{BTreeMap, BTreeSet};

/// The source chain of a track item whose media is the graphic generator.
pub(super) struct GraphicClip {
    sub: Located<SubClip>,
    clip: Located<VideoClip>,
    source: Located<VideoMediaSource>,
    media: Located<Media>,
    capsule: bool,
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
    let media_record = graph.locate(media_reference, &source.identity).ok()?;
    if color_matte::is_color_matte_media(media_record)
        || color_matte::is_black_video_media(media_record)
    {
        return None;
    }
    let media = graph
        .follow::<Media>(media_reference, &source.identity)
        .ok()?;
    // Dynamic Link and saved graphic containers share an importer. Only an
    // explicit capsule component distinguishes this occurrence's content.
    let capsule = media.value.implementation_id.as_deref()
        == Some(crate::schema::after_effects::IMPORTER_ID)
        && capsule::has_saved_component(graph, item);
    (capsule
        || legacy_title::is_title(&media.value)
        || media.value.implementation_id.as_deref() == Some(text::GRAPHIC_IMPLEMENTATION_ID))
    .then_some(GraphicClip {
        sub,
        clip,
        source,
        media,
        capsule,
    })
}

pub(super) fn is_capsule(source: &GraphicClip) -> bool {
    source.capsule
}

pub(super) fn read_capsule(
    graph: &Graph<'_>,
    item: Located<VideoClipTrackItem>,
    source: GraphicClip,
    frame: [u32; 2],
    frame_rate: FrameRate,
    omissions: &mut Vec<Omission>,
) -> Result<crate::schema::PrCapsule> {
    capsule::read(graph, item, source, frame, frame_rate, omissions)
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
        capsule: _,
    } = graphic;
    let placed = required(clip.value.clip.as_ref(), &clip.identity, "Clip")?;
    ensure!(
        placed.is_multicam != Some(true) && placed.selected_track_index.is_none(),
        "{}: multicam selection on a graphic is unsupported",
        clip.identity
    );
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
        source_in >= 0 && source_out > source_in,
        "{}: graphic retiming is unsupported",
        clip.identity
    );
    let legacy = legacy_title::is_title(&media.value);
    let hdr = if legacy {
        let stream = graph.follow::<VideoStream>(
            required(
                media.value.video_stream.as_ref(),
                &media.identity,
                "VideoStream",
            )?,
            &media.identity,
        )?;
        ensure!(
            stream_dimensions(&stream)? == frame,
            "{}: legacy title stream differs from sequence canvas",
            media.identity
        );
        None
    } else {
        validate_generator(graph, &source, &media, &frame_rect, frame_rate, omissions)?
    };
    let master = sub
        .value
        .master_clip
        .as_ref()
        .map(|master| validate_master(graph, master, &sub.identity, &source))
        .transpose()?;
    let shared = match &master {
        _ if legacy => None,
        Some(master) => shared_content(graph, master, omissions)?,
        None => None,
    };

    let components = required(
        track_item
            .component_owner
            .as_ref()
            .and_then(|owner| owner.components.as_ref()),
        &identity,
        "graphic component chain",
    )?;
    let chain = graph.follow::<VideoComponentChain>(components, &identity)?;
    if shared.is_none() {
        // An explicit Motion component replaces the default flag. Its controls
        // and keys are checked below by the shared media-Motion reader.
        ensure!(
            chain.value.default_motion.as_deref() == Some("true")
                || chain
                    .value
                    .component_chain
                    .as_ref()
                    .and_then(|content| content.components.as_ref())
                    .is_some_and(
                        |components| components.items.iter().any(|reference| match_name(
                            graph,
                            reference,
                            &chain.identity
                        )
                        .as_deref()
                            == Some("AE.ADBE Motion"))
                    ),
            "{}: graphic clip Motion and Opacity must keep their defaults",
            chain.identity
        );
    }
    ensure!(
        shared.is_none() || chain.value.component_group_map.is_none(),
        "{}: SubGroups on a Source Graphic placement are unverified",
        chain.identity
    );
    default_chain(
        graph,
        components,
        &identity,
        &["ComponentGroupMap"],
        omissions,
    );
    let placed = chain
        .value
        .component_chain
        .as_ref()
        .and_then(|content| content.components.as_ref())
        .map_or(&[][..], |components| components.items.as_slice());
    // Source Graphic objects belong to the master; placement transforms
    // must surround those objects without changing the shared source.
    let mut placement_vector_motion = None;
    let (content, clip_motion, clip_motion_animations, clip_opacity, layers) = match &shared {
        Some(content) => {
            let (motion, opacity, vector_motion) =
                read_placement_motion(graph, &chain, placed, frame, omissions)?;
            placement_vector_motion = vector_motion;
            let layers = content
                .value
                .component_chain
                .as_ref()
                .and_then(|chain| chain.components.as_ref())
                .map_or(&[][..], |components| components.items.as_slice());
            (
                content,
                motion.transform,
                motion.animations,
                opacity,
                layers,
            )
        }
        None => {
            // Premiere 26.5.1 lists a kept clip Opacity first. A chain with neither a
            // `DefaultOpacity` nor an Opacity component reads 100: inferred (video
            // reader's fallback), not Adobe-observed.
            let (clip_opacity, layers) = match placed.split_first() {
                Some((first, rest)) if is_opacity(graph, first, &chain.identity) => {
                    (Some(first), rest)
                }
                _ => (None, placed),
            };
            let (clip_motion, clip_motion_animations, layers) = match layers.split_first() {
                Some((first, rest))
                    if match_name(graph, first, &chain.identity).as_deref()
                        == Some("AE.ADBE Motion") =>
                {
                    let motion = read_video_animations(graph, &chain, &[first], true, omissions)?;
                    ensure!(
                        motion.crop.is_default()
                            && motion.linear_wipe.is_none()
                            && motion.track_matte.is_none(),
                        "{}: graphic Motion coverage effects are not converted",
                        chain.identity
                    );
                    (motion.transform, motion.animations, rest)
                }
                _ => (PrStaticTransform::default(), Vec::new(), layers),
            };
            (
                &chain,
                clip_motion,
                clip_motion_animations,
                clip_opacity,
                layers,
            )
        }
    };
    // A mask on the clip Opacity masks the whole graphic after its Vector
    // Motion, in the sequence frame (fixture `feature_graphic_masks_d_26_5`,
    // probe d1); one that the mask reader rejects omits the graphic, and so
    // does a keyed Mask Path, which only a video clip's mask converts.
    let (opacity, blend_mode, opacity_animation, opacity_mask) =
        read_video_compositing(graph, &chain, clip_opacity.as_slice(), omissions)?;
    ensure!(
        opacity_mask
            .as_ref()
            .is_none_or(|mask| mask.raster.is_none()),
        "{}: Object Mask saved raster requires a physical video source; graphic occurrence omitted",
        chain.identity
    );
    ensure!(
        shared.is_none() || opacity_mask.is_none(),
        "{}: a clip Opacity mask on a Source Graphic is not converted: its mask frame is unmeasured",
        chain.identity
    );
    ensure!(
        opacity_mask
            .as_ref()
            .is_none_or(|mask| mask.path_keys.is_empty()),
        "{}: Mask Path keys on a graphic clip Opacity are not converted; only a video clip's Opacity mask converts keyed",
        chain.identity
    );
    if let Some(reference) = clip_opacity {
        // Keep the shared scalar reader's editable curve rather than drop
        // the whole graphic for an unmeasured mode pair. Native graphic
        // readback has not established parity for these mixed-mode handles.
        let (param, wire) = opacity_keys(graph, reference, &chain.identity)?;
        if bent_bezier_after_linear_or_hold(&wire, None) {
            approximate(
                omissions,
                param,
                "graphic clip Opacity mixed-mode interpolation uses the shared scalar Motion mapping: Linear into Bezier uses both stored handles, Hold into Bezier stays Hold; native graphic interpolation and RGB/alpha fidelity are unverified, so fades may differ; editable objects, key values and generator-clock times retained",
            );
        }
    }
    // The Vector Motion comes first, after a clip Opacity: an order
    // inferred, not seen in an Adobe save.
    let (mut group, object_references) = match layers.split_first() {
        Some((first, rest))
            if match_name(graph, first, &content.identity).as_deref()
                == Some(text::VECTOR_MOTION_MATCH_NAME) =>
        {
            (
                Some(read_vector_motion(graph, first, &content.identity, frame)?),
                rest,
            )
        }
        _ => (None, layers),
    };
    let group_is_placement = placement_vector_motion.is_some();
    if let Some(motion) = placement_vector_motion {
        ensure!(
            group.is_none(),
            "{}: simultaneous shared and placement Vector Motion is unsupported",
            chain.identity
        );
        group = Some(motion);
    }
    ensure!(
        legacy || !object_references.is_empty(),
        "{}: a graphic without text or shape objects is unsupported",
        content.identity
    );
    ensure!(
        shared.is_none() || content.value.component_group_map.is_none(),
        "{}: SubGroups in a shared Source Graphic are unverified",
        content.identity
    );
    let (mut objects, mut effect_loss) = if legacy {
        let title = legacy_title::objects(graph, &media, frame, omissions)?;
        let split = super::effects::split_chain(graph, placed, &content.identity)?;
        approximate(omissions, &identity, "legacy title placement effects use the existing canvas-sized graphic group approximation; source/placement effect staging and native coverage fidelity are unverified; Track Matte Key/Linear Wipe are not mapped on this graphic host");
        let mut effect_omissions = Vec::new();
        let (mapped_effects, _) = split.read_effects(
            graph,
            &super::effects::EffectOwner {
                occurrence: &identity,
                source: None,
                stroke_geometry: false,
                clip_name: media.value.title.as_deref(),
                track_index: 0,
                timeline_ticks: start..end,
                source_is_canvas: true,
                adjustment: false,
                solid_crop: false,
            },
            false,
            &mut effect_omissions,
        );
        let plain_title_alpha = title.plain_alpha
            && shared.is_none()
            && split.motion_and_masks.is_empty()
            && effect_omissions
                .iter()
                .all(|loss| loss.kind != crate::OmissionKind::Omitted)
            && mapped_effects
                .iter()
                .all(|effect| matches!(effect.params, crate::schema::PrEffectParams::Transform(_)));
        omissions.extend(effect_omissions);
        // RGB paint losses do not change an opaque single-face glyph's alpha.
        let loss = Some(PrGraphicEffectLoss {
            effect_component: media.identity.clone(),
            effect_name: "legacy title styling/placement effects",
            mapped_effects,
            has_unmapped_effects: true,
            plain_title_alpha,
        });
        (title.objects, loss)
    } else {
        read_objects(
            graph,
            object_references,
            content,
            frame,
            shared.is_none(),
            &identity,
            omissions,
        )?
    };
    ensure!(
        shared.is_none() || objects.iter().all(|object| object.mask_source().is_none()),
        "{}: Mask with Shape or Text in a shared Source Graphic is unverified",
        content.identity
    );
    ensure!(
        effect_loss.is_none() || opacity_mask.is_none(),
        "{}: graphic effect beside a clip Opacity mask has unverified alpha coverage; graphic occurrence omitted",
        content.identity
    );
    // Shared graphics remain flat; the guard above rejects any SubGroup map.
    let fontless: Vec<_> = object_references.iter().filter_map(|reference| {
        let component = graph.follow::<VideoFilterComponent>(reference, &content.identity).ok()?;
        if component.value.match_name.as_deref() != Some(text::TEXT_MATCH_NAME) { return None; }
        let identity = component.identity.clone();
        let text = read_text_component(graph, component).ok()?;
        matches!(text.document, text_payload::TextDocuments::Uniform(document) if document.font.is_empty()).then_some(identity)
    }).collect();
    // Only static shared content was saved: the native pass showed one
    // static text at two source clocks, which fixes no clock for keys.
    ensure!(
        shared.is_none()
            || (group_is_placement
                || group
                    .as_ref()
                    .is_none_or(|motion| motion.animations.is_empty()))
                && objects.iter().all(|object| match object {
                    PrGraphicObject::Text(text) => {
                        text.animations.is_empty() && text.source_text_keys.is_empty()
                    }
                    PrGraphicObject::TextLines(lines) => lines.animations.is_empty(),
                    PrGraphicObject::Shape(shape) => shape.animations.is_empty(),
                    PrGraphicObject::Group(_) => false,
                }),
        "{}: keys in Source Graphic shared content are not converted: their clock is unmeasured",
        content.identity
    );
    if let (Some(_), Some(loss)) = (&shared, &mut effect_loss) {
        for effect in &mut loss.mapped_effects {
            if !effect.animations.is_empty() {
                effect.animations.clear();
                approximate(omissions, &identity,
                    "graphic effect keys in shared Source Graphic content have an unmeasured clock; retained saved static controls and independent content");
            }
        }
    }
    let effect_keys = effect_loss.as_ref().is_some_and(|loss| {
        loss.mapped_effects
            .iter()
            .any(|effect| !effect.animations.is_empty())
    });
    let content_is_static = group
        .as_ref()
        .is_none_or(|motion| motion.animations.is_empty())
        && objects.iter().all(object_is_static)
        && !effect_keys;
    // Premiere can save a still graphic with a source span different from its
    // displayed placement (100% speed, no reverse). With no keys anywhere in
    // the accepted graphic, generator time cannot change its content. Keep the
    // timeline range and source InPoint; export may canonicalize the unused
    // OutPoint. Keyed graphics still require the measured one-to-one clock.
    ensure!(
        source_out.checked_sub(source_in) == end.checked_sub(start)
            || opacity_animation.is_none()
                && clip_motion_animations.is_empty()
                && opacity_mask.as_ref().is_none_or(mask_is_static)
                && content_is_static,
        "{}: graphic retiming is unsupported",
        clip.identity
    );
    let vector_motion = match group {
        Some(motion)
            if effect_loss
                .as_ref()
                .is_none_or(|loss| loss.mapped_effects.is_empty())
                && motion.animations.is_empty()
                && objects.len() == 1
                && objects[0].compose_static_vector_motion_in_range(&motion, frame) =>
        {
            None
        }
        motion => motion,
    };
    let graphic = PrGraphic {
        hdr,
        id: Some(identity),
        start_ticks: start,
        end_ticks: end,
        in_ticks: source_in,
        vector_motion,
        clip_motion,
        clip_motion_animations,
        opacity,
        blend_mode,
        animations: opacity_animation.into_iter().collect(),
        opacity_mask,
        objects,
        effect_loss,
        enabled,
    };
    graphic.validate(frame_rate)?;
    // Keyed by the saved object: the placements of a Source Graphic read
    // the same one and report it once.
    for object in fontless {
        approximate(omissions, object, empty_text_font());
    }
    if let (Some(master), Some(_)) = (&master, &shared) {
        // Reported once per master: the list keeps one copy of a report.
        omit(
            omissions,
            OmissionScope::Feature,
            &master.identity,
            SHARED_EDITING_NOT_CONVERTED,
        );
    }
    Ok(graphic)
}

// Mask-source objects are still time-dependent when their Text content or
// transform is keyed. SubGroups must check their children, not just their own
// identity transform. Attached and clip masks carry separate numeric clocks.
fn object_is_static(object: &PrGraphicObject) -> bool {
    match object {
        PrGraphicObject::Text(text) => {
            text.animations.is_empty() && text.source_text_keys.is_empty()
        }
        PrGraphicObject::TextLines(lines) => lines.animations.is_empty(),
        PrGraphicObject::Shape(shape) => {
            shape.animations.is_empty() && shape.mask.as_ref().is_none_or(mask_is_static)
        }
        PrGraphicObject::Group(group) => group.objects.iter().all(object_is_static),
    }
}

fn mask_is_static(mask: &crate::schema::PrMask) -> bool {
    mask.path_keys.is_empty() && !mask.has_numeric_keys()
}

/// Why a Source Graphic converts without its link: every placement of its
/// master clip imports its own copy of the shared objects.
pub(super) const SHARED_EDITING_NOT_CONVERTED: &str = "Source Graphic shared editing is not converted: each placement imports its own copy of the shared objects, so editing one copy leaves the others unchanged";

/// The approximation reported for a Text saved without a font, which is
/// empty (one with characters needs a font: `PrTextDocument::validate`): its
/// import font, in which it draws nothing.
pub(super) fn empty_text_font() -> String {
    format!(
        "empty Text saved without a font: it imports with {}, FX's font for new text; Premiere saved no font for it",
        text::EMPTY_TEXT_FONT.join(" ")
    )
}

/// The Source Graphic content of `master`: its own component chain, which
/// holds the objects that every placement of the master shows (Premiere
/// 26.5.1 save `feature_source_graphic_26_5`). `None` for an ordinary
/// graphic, whose placement holds its objects.
fn shared_content(
    graph: &Graph<'_>,
    master: &Located<MasterClip>,
    omissions: &mut Vec<Omission>,
) -> Result<Option<Located<VideoComponentChain>>> {
    let Some(reference) = &master.value.video_component_chain else {
        return Ok(None);
    };
    default_chain(
        graph,
        reference,
        &master.identity,
        &["ComponentGroupMap"],
        omissions,
    );
    let content = graph.follow::<VideoComponentChain>(reference, &master.identity)?;
    // The saved content chain has no intrinsic Motion or Opacity of its own.
    ensure!(
        content.value.default_motion.is_none() && content.value.default_opacity.is_none(),
        "{}: a Source Graphic chain with its own Motion or Opacity defaults is unsupported",
        content.identity
    );
    Ok(Some(content))
}

/// Read placement-owned Motion and Vector Motion on the generator source
/// clock, independently of the shared objects and the clip Opacity component.
fn read_placement_motion<'r>(
    graph: &Graph<'_>,
    chain: &Located<VideoComponentChain>,
    placed: &'r [Reference],
    frame: [u32; 2],
    omissions: &mut Vec<Omission>,
) -> Result<(
    MotionAndMasks,
    Option<&'r Reference>,
    Option<PrVectorMotion>,
)> {
    let mut clip_opacity = None;
    let mut vector_motion = None;
    let mut components = Vec::new();
    for reference in placed {
        let component = graph.follow::<VideoFilterComponent>(reference, &chain.identity)?;
        match component.value.match_name.as_deref() {
            Some(text::VECTOR_MOTION_MATCH_NAME) => {
                ensure!(vector_motion.is_none(), "{}: duplicate placement Vector Motion", chain.identity);
                let motion = read_vector_motion(graph, reference, &chain.identity, frame)?;
                vector_motion = Some(motion);
                continue;
            }
            Some("AE.ADBE Motion") => {}
            // The compositing reader checks only the component returned
            // here, so a second Opacity would escape its checks.
            Some("AE.ADBE Opacity") => ensure!(
                clip_opacity.replace(reference).is_none(),
                "{}: duplicate intrinsic Opacity",
                chain.identity
            ),
            name => {
                return Err(unsupported(format!(
                    "{}: a Source Graphic placement's chain holds only its clip Motion and Opacity, not {name:?}",
                    chain.identity
                )))
            }
        }
        components.push(reference);
    }
    let motion = read_video_animations(graph, chain, &components, true, omissions)?;
    ensure!(
        motion.crop.is_default() && motion.linear_wipe.is_none() && motion.track_matte.is_none(),
        "{}: Motion Crop, Linear Wipe or Track Matte Key on a Source Graphic placement is not converted",
        chain.identity
    );
    Ok((motion, clip_opacity, vector_motion))
}

/// The match name of a chain component that resolves.
fn match_name(graph: &Graph<'_>, reference: &Reference, from: &str) -> Option<String> {
    graph
        .follow::<VideoFilterComponent>(reference, from)
        .ok()?
        .value
        .match_name
}

enum ReadObject {
    Kept(Box<PrGraphicObject>),
    MappedEffect,
    Omitted { reason: String, masks_below: bool },
}

/// A SubGroup whose members are still being read.
struct OpenGroup {
    /// Its component `ID`, which its members' pins name.
    id: String,
    identity: String,
    name: String,
    objects: Vec<(String, ReadObject)>,
    /// Why the whole group is omitted.
    omitted: Option<String>,
}

/// Read the objects of a graphic's chain after its Vector Motion into their
/// SubGroup tree. The chain is flat: it lists each SubGroup before its
/// members, which the chain's `ComponentGroupMap` pins to it by component
/// `ID` in Adobe's 24.3 Social Media Template. A pin
/// must name a member and a SubGroup of this chain, once, and a member must
/// follow its SubGroup inside the SubGroup's run of members. A component
/// that cannot convert still omits the graphic, except Ramp, Gaussian Blur, Geometry2,
/// Tint and Basic 3D, which retain saved paints and supported controls without certifying
/// native coverage. A mask or SubGroup form that no render covers omits only
/// its part ([`admit`]).
fn read_objects(
    graph: &Graph<'_>,
    references: &[Reference],
    chain: &Located<VideoComponentChain>,
    frame: [u32; 2],
    ordinary: bool,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<(Vec<PrGraphicObject>, Option<PrGraphicEffectLoss>)> {
    let pins = match &chain.value.component_group_map {
        Some(reference) => read_group_map(graph, reference, &chain.identity)?,
        None => BTreeMap::new(),
    };
    let mut top = Vec::new();
    let mut open: Vec<OpenGroup> = Vec::new();
    let mut ids = BTreeSet::new();
    let mut groups = BTreeSet::new();
    let mut effect_loss = None;
    let mut leading_effects = true;
    for reference in references {
        let native_record = graph.locate(reference, &chain.identity)?;
        let component = graph.decode::<VideoFilterComponent>(native_record)?;
        let is_effect = matches!(
            component.value.match_name.as_deref(),
            Some(
                "AE.ADBE Ramp"
                    | "AE.ADBE Gaussian Blur 2"
                    | "AE.ADBE Geometry2"
                    | "AE.ADBE Tint"
                    | "AE.ADBE Basic 3D"
            )
        );
        let mapped_effect = (leading_effects
            && open.is_empty()
            && is_effect
            && component.value.match_name.as_deref() != Some("AE.ADBE Basic 3D"))
        .then(|| super::effects::read_graphic_effect(graph, native_record));
        leading_effects &= is_effect;
        let id = component
            .value
            .component
            .as_ref()
            .and_then(|body| body.id.clone());
        if let Some(id) = &id {
            ensure!(
                ids.insert(id.clone()),
                "{}: duplicate graphic component ID {id}",
                component.identity
            );
        }
        let parent = id.as_ref().and_then(|id| pins.get(id));
        ensure!(
            pins.is_empty() || id.is_some(),
            "{}: a graphic component without an ID beside a SubGroup map",
            component.identity
        );
        // Members follow their SubGroup, so reaching a component closes every
        // SubGroup that does not enclose it.
        while open.last().is_some_and(|group| Some(&group.id) != parent) {
            close_group(&mut open, &mut top, record, omissions);
        }
        if let Some(parent) = parent {
            ensure!(
                open.last().is_some(),
                "{}: pinned to SubGroup {parent}, which does not enclose it in the chain",
                component.identity
            );
        }
        let identity = component.identity.clone();
        if component.value.match_name.as_deref() == Some(text::SUBGROUP_MATCH_NAME) {
            ensure!(
                ordinary,
                "{identity}: SubGroups in a shared Source Graphic are unverified"
            );
            let id = id.ok_or_else(|| {
                unsupported(format!("{identity}: a graphic SubGroup without an ID"))
            })?;
            let (name, omitted) = read_subgroup(graph, component)?;
            groups.insert(id.clone());
            open.push(OpenGroup {
                id,
                identity,
                name,
                objects: Vec::new(),
                omitted,
            });
            continue;
        }
        ensure!(
            ordinary || component.value.sub_components.is_none(),
            "{identity}: a mask on a graphic object is not converted"
        );
        let object = read_object(
            graph,
            component,
            frame,
            record,
            omissions,
            &mut effect_loss,
            mapped_effect,
        )?;
        let mask = match &object {
            ReadObject::Kept(object) => object.mask_source().is_some(),
            ReadObject::MappedEffect => false,
            ReadObject::Omitted { masks_below, .. } => *masks_below,
        };
        ensure!(
            ordinary || !mask,
            "{identity}: Mask with Shape or Text in a shared Source Graphic is unverified"
        );
        match open.last_mut() {
            Some(group) => group.objects.push((identity, object)),
            None => top.push((identity, object)),
        }
    }
    while !open.is_empty() {
        close_group(&mut open, &mut top, record, omissions);
    }
    for (child, parent) in &pins {
        ensure!(
            ids.contains(child) && groups.contains(parent),
            "{}: its SubGroup map pins component {child} to {parent}, which are not a member and a SubGroup of the chain",
            chain.identity
        );
    }
    let objects = admit(top, 0, record, omissions);
    let objects = match &effect_loss {
        Some(loss) => omit_changed_mask_dependents(objects, loss, record, omissions),
        None => objects,
    };
    Ok((objects, effect_loss))
}

/// Loss is known only after reading the entire chain, including effects after
/// a mask-source object. Remove that dependent lower composite, not its siblings
/// above, and apply the same rule inside already assembled SubGroups.
fn omit_changed_mask_dependents(
    objects: Vec<PrGraphicObject>,
    loss: &PrGraphicEffectLoss,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Vec<PrGraphicObject> {
    let mut kept = Vec::with_capacity(objects.len());
    let mut objects = objects.into_iter();
    while let Some(mut object) = objects.next() {
        let masks_below = object.mask_source().is_some();
        let attached = matches!(&object, PrGraphicObject::Shape(shape) if shape.mask.is_some())
            || matches!(&object, PrGraphicObject::Text(text) if text.mask.is_some());
        if masks_below || attached {
            let below = if masks_below { objects.len() } else { 0 };
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                omitted_part(
                    &format!("{}: graphic {} changes luma and has unverified alpha coverage for Mask with Shape/Text or an attached mask", loss.effect_component, loss.effect_name),
                    below,
                ),
            );
            if masks_below {
                break;
            }
            continue;
        }
        if let PrGraphicObject::Group(group) = &mut object {
            group.objects = omit_changed_mask_dependents(
                std::mem::take(&mut group.objects),
                loss,
                record,
                omissions,
            );
            if group.objects.is_empty() {
                continue;
            }
        }
        kept.push(object);
    }
    kept
}

/// Close the innermost open SubGroup into its parent's objects, admitting
/// its members at its depth.
fn close_group(
    open: &mut Vec<OpenGroup>,
    top: &mut Vec<(String, ReadObject)>,
    record: &str,
    omissions: &mut Vec<Omission>,
) {
    let Some(group) = open.pop() else {
        return;
    };
    let object = match group.omitted {
        Some(reason) => ReadObject::Omitted {
            reason: format!("{}: {reason}", group.identity),
            masks_below: false,
        },
        None => ReadObject::Kept(Box::new(PrGraphicObject::Group(PrGraphicGroup {
            name: group.name,
            objects: admit(group.objects, open.len() + 1, record, omissions),
        }))),
    };
    match open.last_mut() {
        Some(parent) => parent.objects.push((group.identity, object)),
        None => top.push((group.identity, object)),
    }
}

/// The objects of one group level at SubGroup `depth` that convert, reporting
/// the others. An omitted Mask with Shape or Text takes the objects below it
/// in the group, and so does a mask composite outside the rendered forms
/// ([`unverified_mask_composite`]); the objects above it stay.
fn admit(
    objects: Vec<(String, ReadObject)>,
    depth: usize,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Vec<PrGraphicObject> {
    let mut kept = Vec::with_capacity(objects.len());
    let mut identities = Vec::with_capacity(objects.len());
    let mut objects = objects.into_iter();
    while let Some((identity, object)) = objects.next() {
        match object {
            ReadObject::MappedEffect => continue,
            ReadObject::Kept(object) => {
                if matches!(object.as_ref(), PrGraphicObject::Group(group) if group.objects.is_empty())
                {
                    omit(omissions, OmissionScope::Feature, record,
                        format!("{identity}: empty graphic SubGroup omitted; none observed in measured saves"));
                    continue;
                }
                kept.push(*object);
                identities.push(identity);
            }
            ReadObject::Omitted {
                reason,
                masks_below,
            } => {
                let below = if masks_below { objects.len() } else { 0 };
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    omitted_part(&reason, below),
                );
                if masks_below {
                    break;
                }
            }
        }
    }
    if let Some((index, reason)) = unverified_mask_composite(&kept, depth) {
        let below = kept.len() - index - 1;
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            omitted_part(&format!("{}: {reason}", identities[index]), below),
        );
        kept.truncate(index);
    }
    kept
}

/// The SubGroup of each member component `ID`, from a chain's
/// `ComponentGroupMap`.
fn read_group_map(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
) -> Result<BTreeMap<String, String>> {
    let record = graph.locate_as(
        reference,
        records::COMPONENT_PIN_VECTOR_SERIALIZER.tag,
        from,
    )?;
    let map = graph.decode::<ComponentPinVectorSerializer>(record)?;
    let mut pins = BTreeMap::new();
    for item in &map.value.pin_vector.items {
        let record = graph.locate_as(item, records::COMPONENT_PIN_SERIALIZER.tag, &map.identity)?;
        let pin = graph.decode::<ComponentPinSerializer>(record)?.value;
        ensure!(
            pins.insert(pin.child_pin_id.clone(), pin.parent_pin_id)
                .is_none(),
            "{}: component {} is pinned to more than one SubGroup",
            map.identity,
            pin.child_pin_id
        );
    }
    Ok(pins)
}

/// Read a SubGroup component: its name, and why the group is omitted when
/// its transform is keyed or not the identity, or a mask is attached to it,
/// which no render covers.
fn read_subgroup(
    graph: &Graph<'_>,
    component: Located<VideoFilterComponent>,
) -> Result<(String, Option<String>)> {
    let body = required(
        component.value.component.as_ref(),
        &component.identity,
        "Component",
    )?;
    ensure!(
        body.display_name.as_deref() == Some("Group")
            && body.intrinsic.is_none()
            && body.bypass.as_deref().is_none_or(|value| value == "false")
            && component.value.premiere_filter_private_data.is_none(),
        "{}: unsupported graphic SubGroup component",
        component.identity
    );
    let params = required(body.params.as_ref(), &component.identity, "Params")?;
    // A SubGroup has the Vector Motion's parameter layout in the 24.3 template.
    let (transform, animations) = read_params(
        graph,
        &params.items,
        &VECTOR_MOTION_PARAMS,
        GraphicComponent::VectorMotion,
        &component.identity,
    )?;
    let identity = animations.is_empty()
        && transform.position == transform.anchor
        && transform.scale == 100.0
        && transform.rotation == 0.0;
    let omitted = if component.value.sub_components.is_some() {
        Some("a mask on a graphic SubGroup is unverified against Premiere")
    } else if !identity {
        Some("a keyed or moved graphic SubGroup is unverified against Premiere")
    } else {
        None
    };
    Ok((
        body.instance_name.clone().unwrap_or_default(),
        omitted.map(str::to_owned),
    ))
}

/// Read one graphic object or carry its supported leading effect controls.
/// Native graphic-host coverage stays unproved for masks/Track Matte consumers.
/// Unknown effects retain their existing guards; misplaced Geometry2 is diagnosed
/// without discarding independent base paints.
fn read_object(
    graph: &Graph<'_>,
    mut component: Located<VideoFilterComponent>,
    frame: [u32; 2],
    record: &str,
    omissions: &mut Vec<Omission>,
    effect_loss: &mut Option<PrGraphicEffectLoss>,
    mapped_effect: Option<Result<crate::schema::PrEffect>>,
) -> Result<ReadObject> {
    let pixels = frame_pixels(frame);
    let identity = component.identity.clone();
    if component.value.match_name.as_deref() == Some(text::SHAPE_MATCH_NAME)
        && component.value.component.as_ref().is_some_and(|body| {
            body.display_name.as_deref() == Some("Shape") && body.bypass.as_deref() == Some("true")
        })
    {
        // Bypass removes the whole object, including any coverage it supplied.
        return Ok(ReadObject::Omitted {
            reason: format!("{identity}: bypassed graphic Shape omitted; disabled content is not retained for editing"),
            masks_below: false,
        });
    }
    let attached = component.value.sub_components.take();
    let object = match component.value.match_name.as_deref() {
        Some(text::TEXT_MATCH_NAME) => {
            let mut layer = read_text_component(graph, component)?;
            if let text_payload::TextDocuments::Lines(lines) = &layer.document {
                if lines.iter().any(|line| line.leading != 0.0) {
                    if !layer.transform.uniform || attached.is_some() || layer.mask_source.is_some()
                    {
                        let mut document = lines[0].clone();
                        document.text = lines
                            .iter()
                            .map(|line| line.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        document.leading = 0.0;
                        layer.document = text_payload::TextDocuments::Uniform(document);
                        approximate(omissions, &identity, "tight point text with nonuniform scale or object coverage retains automatic leading; independent line placement is unsupported for this host");
                    } else {
                        approximate(omissions, &identity, "tight point text retained as independently editable lines; text edits do not reflow between lines");
                    }
                }
            }
            for reason in &layer.transform.approximations {
                approximate(omissions, &identity, reason);
            }
            let masks_below = layer.mask_source.is_some();
            let mask = if !masks_below {
                let mask = attached
                    .as_ref()
                    .map(|attached| {
                        super::mask::read_opacity_mask(graph, &identity, attached, omissions)
                    })
                    .transpose()?
                    .flatten();
                ensure!(
                    mask.as_ref().is_none_or(|mask| mask.raster.is_none()
                        && mask.path_keys.is_empty()
                        && !mask.has_numeric_keys()
                        && mask.feather == 0.0
                        && mask.expansion == 0.0
                        && mask.opacity == 100.0)
                        && (mask.is_none()
                            || matches!(&layer.document,
                            text_payload::TextDocuments::Uniform(document) if document.fill.is_some())
                                && layer
                                    .source_text_keys
                                    .iter()
                                    .all(|key| key.document.fill.is_some())),
                    "{identity}: only static hard masks on uniform graphic Text are converted"
                );
                mask
            } else {
                None
            };
            let unsupported = if attached.is_some() && masks_below {
                Some("a mask on a graphic object is not converted".to_owned())
            } else if masks_below && matches!(layer.document, text_payload::TextDocuments::Lines(_))
            {
                Some("Mask with Text on mixed-line text is unverified".to_owned())
            } else if masks_below && !layer.omitted.is_empty() {
                Some(format!(
                    "a Mask with Text whose {} is not converted would mask differently",
                    layer.omitted[0]
                ))
            } else {
                None
            };
            if let Some(reason) = unsupported {
                return Ok(ReadObject::Omitted {
                    reason: format!("{identity}: {reason}"),
                    masks_below,
                });
            }
            for feature in &layer.omitted {
                match feature {
                    text_payload::OmittedTextFeature::Background => omit(
                        omissions,
                        OmissionScope::Feature,
                        record,
                        format!("{feature} not converted"),
                    ),
                    _ => approximate(omissions, &identity, feature.to_string()),
                }
            }
            let transform = object_transform(&layer.transform, pixels);
            match layer.document {
                text_payload::TextDocuments::Uniform(document) => PrGraphicObject::Text(PrText {
                    mask,
                    horizontal_scale: (!layer.transform.uniform)
                        .then_some(layer.transform.horizontal_scale),
                    name: layer.name,
                    document,
                    transform,
                    animations: layer.animations,
                    source_text_keys: layer.source_text_keys,
                    mask_source: layer.mask_source,
                }),
                text_payload::TextDocuments::Lines(documents) => {
                    ensure!(
                        layer.transform.uniform,
                        "{identity}: nonuniform mixed-line Text scale is unsupported"
                    );
                    PrGraphicObject::TextLines(text::PrTextLines {
                        name: layer.name,
                        documents,
                        transform,
                        animations: layer.animations,
                    })
                }
            }
        }
        Some(text::SHAPE_MATCH_NAME) => {
            let mut shape = read_shape_component(graph, component, pixels, omissions)?;
            if let Some(attached) = attached {
                match super::mask::read_opacity_mask(graph, &identity, &attached, omissions) {
                    Ok(mask) if mask.as_ref().is_none_or(|mask| mask.path_keys.is_empty()) => {
                        shape.mask = mask
                    }
                    Ok(_) => {
                        return Ok(ReadObject::Omitted {
                            reason: format!(
                                "{identity}: Shape-attached Mask Path keys are unsupported"
                            ),
                            masks_below: shape.appearance.mask_source.is_some(),
                        })
                    }
                    Err(error) => {
                        return Ok(ReadObject::Omitted {
                            reason: format!(
                                "{identity}: its attached mask cannot convert: {error}"
                            ),
                            masks_below: shape.appearance.mask_source.is_some(),
                        })
                    }
                }
            }
            PrGraphicObject::Shape(shape)
        }
        Some(
            name @ ("AE.ADBE Ramp"
            | "AE.ADBE Gaussian Blur 2"
            | "AE.ADBE Geometry2"
            | "AE.ADBE Tint"
            | "AE.ADBE Basic 3D"),
        ) => {
            let label = match name {
                "AE.ADBE Ramp" => "Ramp",
                "AE.ADBE Geometry2" => "Geometry2",
                "AE.ADBE Tint" => "Tint",
                "AE.ADBE Basic 3D" => "Basic 3D",
                _ => "Gaussian Blur",
            };
            let body = required(component.value.component.as_ref(), &identity, "Component")?;
            let bypassed = body.bypass.as_deref() == Some("true");
            ensure!(
                bypassed || attached.is_none(),
                "{identity}: graphic {label} with an attached mask is not converted"
            );
            let mut limitation = None;
            if !bypassed {
                let loss = effect_loss.get_or_insert_with(|| PrGraphicEffectLoss {
                    effect_component: identity.clone(),
                    effect_name: label,
                    mapped_effects: Vec::new(),
                    has_unmapped_effects: false,
                    plain_title_alpha: false,
                });
                match mapped_effect {
                    Some(Ok(effect)) => {
                        loss.mapped_effects.push(effect);
                        return Ok(ReadObject::MappedEffect);
                    }
                    Some(Err(error)) => limitation = Some(super::effects::reason(error)),
                    None if name == "AE.ADBE Basic 3D" => limitation = Some("Swivel, Tilt, Distance to Image, Specular Highlight, Preview and their animation are omitted; retained content is flat, not a perspective or lighting approximation".to_owned()),
                    None => limitation = Some(format!("the {label} is inside or between graphic objects; its effect scope cannot be replaced by the whole graphic")),
                }
                loss.has_unmapped_effects = true;
            }
            return Ok(ReadObject::Omitted {
                reason: if bypassed {
                    format!("{identity}: bypassed graphic {label} omitted")
                } else {
                    format!("{identity}: graphic {label} is unsupported; saved base paints retained without the effect: {}", limitation.unwrap_or_default())
                },
                masks_below: false,
            });
        }
        Some(text::VECTOR_MOTION_MATCH_NAME) | None => {
            return Err(unsupported(format!(
                "{identity}: unsupported graphic component"
            )))
        }
        Some(effect) => {
            return Err(unsupported(format!(
                "{identity}: effect {effect:?} in a graphic is unsupported"
            )))
        }
    };
    Ok(ReadObject::Kept(Box::new(object)))
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
    omissions: &mut Vec<Omission>,
) -> Result<Option<fx_conv::HdrTransfer>> {
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
    let Some(encoded) = stream.value.original_color_space.as_deref() else {
        return Ok(None); // Legacy missing declaration is not a root-HDR inference.
    };
    match serde_json::from_str::<crate::schema::ColorSpace>(encoded) {
        Ok(color) if color == crate::schema::ColorSpace::sequence_sdr() => Ok(None),
        Ok(color) if color.rgb_hdr_transfer().is_some() => Ok(color.rgb_hdr_transfer()),
        _ => {
            approximate(omissions, &stream.identity, "unknown Graphic OriginalColorSpace; source RGB retained as legacy numeric approximation, independently convertible objects preserved; no root HDR inheritance or colour-fidelity claim");
            Ok(None)
        }
    }
}

/// The graphic's master clip, which must play the placement's generator
/// source.
fn validate_master(
    graph: &Graph<'_>,
    master: &Reference,
    from: &str,
    source: &Located<VideoMediaSource>,
) -> Result<Located<MasterClip>> {
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
    Ok(master)
}

/// A static transform whose position and anchor are normalized to the frame.
struct NormalizedTransform {
    approximations: Vec<String>,
    position: [f64; 2],
    anchor: [f64; 2],
    scale: f64,
    /// Independent horizontal scale and the native Uniform Scale switch.
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
    mask_source: Option<PrMaskSource>,
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
        "{}: a mask on a graphic Vector Motion is not converted",
        component.identity
    );
    let body = required(
        component.value.component.as_ref(),
        &component.identity,
        "Component",
    )?;
    ensure!(
        component.value.match_name.as_deref() == Some(text::VECTOR_MOTION_MATCH_NAME)
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

    let specs = if source_text.legacy_static && transform_params.len() == LEGACY_TEXT_PARAM_COUNT {
        &TEXT_PARAMS[..LEGACY_TEXT_PARAM_COUNT]
    } else {
        &TEXT_PARAMS[..]
    };
    let profile = if source_text.legacy_static {
        GraphicComponent::LegacyText
    } else {
        GraphicComponent::Text
    };
    let (transform, animations) =
        read_params(graph, transform_params, specs, profile, &component.identity)?;
    Ok(TextComponent {
        name: body.instance_name.clone().unwrap_or_default(),
        document: source_text.document,
        transform,
        animations,
        source_text_keys: source_text.keys,
        mask_source: source_text.mask_source,
        omitted: source_text.omitted,
    })
}

/// A Text's Source Text as read: the document shown before the first key,
/// the keys, and the features of any of them that the model does not keep.
struct SourceText {
    document: text_payload::TextDocuments,
    keys: Vec<PrSourceTextKey>,
    omitted: Vec<text_payload::OmittedTextFeature>,
    legacy_static: bool,
    mask_source: Option<PrMaskSource>,
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
    let legacy_static = stored.legacy_json && wire.is_empty();
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
            ensure!(
                decoded.mask_source == stored.mask_source,
                "{}: Source Text keys that change Mask with Text are unverified",
                source.identity
            );
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
        legacy_static,
        mask_source: stored.mask_source,
    })
}

/// Read one static Shape object: its Path and Appearance, and a transform
/// without keys.
fn read_shape_component(
    graph: &Graph<'_>,
    component: Located<VideoFilterComponent>,
    pixels: impl Fn([f64; 2]) -> [f64; 2],
    omissions: &mut Vec<Omission>,
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
        component.value.match_name.as_deref() == Some(text::SHAPE_MATCH_NAME)
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
    let (appearance, appearance_notes) = binary_param(
        graph,
        appearance,
        &component.identity,
        ("2", "Appearance"),
        shape_payload::decode_appearance_with_notes,
    )?;
    let (transform, animations) = read_params(
        graph,
        transform_params,
        &SHAPE_PARAMS,
        GraphicComponent::Shape,
        &component.identity,
    )?;
    ensure!(
        animations
            .iter()
            .all(|animation| matches!(animation, PrPropertyAnimation::UniformScale(_))),
        "{}: keyed graphic Shape parameters are unsupported",
        component.identity
    );
    for reason in &transform.approximations {
        approximate(omissions, &component.identity, reason);
    }
    // Equal static axes are unambiguous without inferring inactive-width semantics.
    let horizontal_scale = match (transform.uniform, transform.horizontal_scale) {
        (true, width) if width == 100.0 || (width == transform.scale && animations.is_empty()) => {
            None
        }
        (true, _) => {
            return Err(unsupported(format!(
                "{}: Horizontal Scale under Uniform Scale is unverified",
                component.identity
            )))
        }
        (false, scale) => Some(scale),
    };
    if appearance
        .stroke
        .is_some_and(|stroke| stroke.join.is_none())
    {
        text::stroke_join(&path, None)
            .map_err(|reason| unsupported(format!("{}: {reason}", component.identity)))?;
    }
    for reason in appearance_notes {
        approximate(omissions, &component.identity, &reason);
    }
    Ok(PrShape {
        name: body.instance_name.clone().unwrap_or_default(),
        path,
        appearance,
        transform: object_transform(&transform, pixels),
        horizontal_scale,
        animations,
        mask: None,
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

/// Decode the binary value of a parameter `(ParameterID, Name)` stored like
/// Source Text, and its keys: `ticks,base64;` per key, each a complete value
/// on the owner's source clock, in strictly increasing time. Keys need an
/// absent or true `IsTimeVarying` and a flag without keys must not be true, as
/// for Source Text ([`read_source_text`]). The static value is decoded even
/// with keys, so that a malformed one still fails closed.
pub(super) fn keyed_binary_param<T>(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    (id, name): (&str, &str),
    decode: impl Fn(&[u8]) -> Result<T>,
) -> Result<(T, Vec<(i64, T)>)> {
    let source = arb_param(graph, reference, from, (id, name))?;
    let wire = source.value.keyframes.as_deref().unwrap_or_default();
    ensure!(
        match source.value.is_time_varying.as_deref() {
            None => true,
            Some("true") => !wire.is_empty(),
            Some("false") => wire.is_empty(),
            Some(_) => false,
        },
        "{}: animated or unknown {name} is unsupported",
        source.identity
    );
    let value = static_value(graph, &source, name, &decode)?;
    if wire.is_empty() {
        return Ok((value, Vec::new()));
    }
    ensure!(
        wire.ends_with(';'),
        "{}: unterminated {name} key list",
        source.identity
    );
    let mut keys: Vec<(i64, T)> = Vec::new();
    for key in wire.split_terminator(';') {
        let Some((ticks, encoded)) = key.split_once(',') else {
            return Err(unsupported(format!(
                "{}: unexpected {name} key shape",
                source.identity
            )));
        };
        let ticks = ticks
            .parse::<i64>()
            .map_err(|_| unsupported(format!("{}: invalid {name} key time", source.identity)))?;
        ensure!(
            keys.last().is_none_or(|(last, _)| *last < ticks),
            "{}: {name} keys must have strictly increasing source times",
            source.identity
        );
        keys.push((
            ticks,
            decode_stored(encoded, &source.identity, name, &decode)?,
        ));
    }
    Ok((value, keys))
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
    LegacyText,
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
        approximations: Vec::new(),
        position: [0.5; 2],
        anchor: [0.0; 2],
        scale: 100.0,
        horizontal_scale: 100.0,
        uniform: true,
        rotation: 0.0,
        opacity: 100.0,
    };
    let mut animations = Vec::new();
    let mut current_text_width = None;
    let mut vertical_name = false;
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
            !matches!(component, GraphicComponent::LegacyText)
                || param.class_id.as_deref() == Some(spec.record.class_id),
            "{}: unexpected legacy graphic parameter class",
            input.identity
        );
        let named_vertical = matches!(component, GraphicComponent::Text)
            && spec.id == 4
            && param.name.as_deref() == Some("Vertical Scale");
        vertical_name |= named_vertical;
        // Older Text saves leave the seven inactive tail controls unassigned.
        // Their ordered layout, class and name still identify the default slot;
        // do not extend this admission to transforms or modern graphic profiles.
        let unassigned_legacy_tail = matches!(component, GraphicComponent::LegacyText)
            && (15..=21).contains(&spec.id)
            && param.parameter_id == "-1";
        // Current controls bind by ParameterID, not localized UI labels.
        ensure!(
            (param.parameter_id == spec.id.to_string() || unassigned_legacy_tail)
                && (!matches!(component, GraphicComponent::LegacyText)
                    || param.name.as_deref() == spec.name),
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
        ensure!(
            !matches!(component, GraphicComponent::LegacyText) || wire.is_empty(),
            "{}: animated legacy graphic parameters are unsupported",
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
                PrAnimatedProperty::AnchorPoint
                | PrAnimatedProperty::ScaleWidth
                | PrAnimatedProperty::ScaleHeight => {
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
            if matches!(
                (component, property),
                (GraphicComponent::Shape, PrAnimatedProperty::UniformScale)
            ) {
                if has_temporal_bezier(wire) {
                    transform.approximations.push(format!(
                        "{}: Shape Scale interpolation uses the shared scalar Motion mapping; native speed units and mixed modes are unverified, so intermediate size may differ; editable values and generator-clock times retained",
                        input.identity
                    ));
                }
            } else if spec.bezier_speeds_verified {
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
        let expected_fields = if spec.is_point() {
            if fields.get(1).is_some_and(|value| value.contains(':')) {
                14
            } else {
                15
            }
        } else {
            8
        };
        ensure!(
            fields.len() == expected_fields && fields[0] == records::STATIC_KEYFRAME_TIME,
            "{}: unexpected graphic parameter keyframe shape",
            input.identity
        );
        let value = fields[1];
        if unassigned_legacy_tail {
            ensure!(
                spec.accepts_default(value),
                "{}: nondefault unassigned legacy Text control is unsupported",
                input.identity
            );
            transform.approximations.push(format!(
                "{}: unassigned legacy Text control at slot {} ({:?}) retains its static default; responsive authoring control is not exported",
                input.identity, spec.id, spec.name.unwrap_or("unnamed")
            ));
        }
        // Legacy Text uses the same Scale/Width/Uniform IDs as current Text.
        // Read the inactive width too: its type, bounds and static clock remain
        // validated even when Uniform selects Scale for both axes.
        let role = match (component, spec.id) {
            (GraphicComponent::LegacyText, 5) => GraphicParamRole::HorizontalScale,
            (GraphicComponent::LegacyText, 6) => GraphicParamRole::Uniform,
            (GraphicComponent::VectorMotion, 3) => GraphicParamRole::HorizontalScale,
            (GraphicComponent::VectorMotion, 4) => GraphicParamRole::Uniform,
            _ => spec.role,
        };
        if matches!(
            component,
            GraphicComponent::LegacyText | GraphicComponent::VectorMotion
        ) && matches!(
            role,
            GraphicParamRole::Scale | GraphicParamRole::HorizontalScale
        ) {
            ensure!(
                spec.holds(number(value, owner)?),
                "{}: graphic scale is outside its native bounds",
                input.identity
            );
        }
        let approximated_controller = matches!(
            component,
            GraphicComponent::Text | GraphicComponent::LegacyText
        ) && matches!(spec.id, 11 | 12)
            || matches!(
                component,
                GraphicComponent::Text | GraphicComponent::LegacyText | GraphicComponent::Shape
            ) && matches!(
                spec.name,
                Some("Parent Width" | "Parent Height" | "Parent Rotation")
            );
        if approximated_controller && !spec.accepts_default(value) {
            ensure!(
                spec.holds(number(value, owner)?),
                "{}: graphic controller value outside native bounds",
                input.identity
            );
            transform.approximations.push(format!(
                "{}: parameter {} ({:?}) value {value}: authoring semantics are unknown; saved object geometry/text is used as the editable approximation instead; the parameter itself is not exported and responsive layout after edits may differ",
                input.identity, spec.id, spec.name.unwrap_or("unnamed")
            ));
            continue;
        }
        match role {
            GraphicParamRole::Position => {
                transform.position = point_start(&param.start_keyframe, &input.identity)?;
            }
            GraphicParamRole::Anchor => {
                transform.anchor = point_start(&param.start_keyframe, &input.identity)?;
            }
            GraphicParamRole::Scale => transform.scale = number(value, owner)?,
            GraphicParamRole::HorizontalScale => {
                let width = number(value, owner)?;
                if matches!(component, GraphicComponent::Text) {
                    current_text_width = Some((input.identity.clone(), width, spec.holds(width)));
                }
                transform.horizontal_scale = width;
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
    if matches!(component, GraphicComponent::VectorMotion) && !transform.uniform {
        // PrVectorMotion has one scale axis; equal static independent axes are
        // the same similarity, but keyed Scale would leave Scale Width behind.
        ensure!(
            transform.horizontal_scale == transform.scale
                && !animations
                    .iter()
                    .any(|animation| matches!(animation, PrPropertyAnimation::UniformScale(_))),
            "{owner}: independent Vector Motion scale requires equal static Scale and Scale Width"
        );
    }
    if matches!(component, GraphicComponent::LegacyText) {
        // PrText's uniform transform uses Scale for both axes; Horizontal
        // Scale is active only with Uniform off. Legacy saves can retain a
        // different static width from that inactive control. Do not apply it
        // as a second axis or require it to mirror the active Scale value.
        ensure!(
            transform.uniform,
            "{owner}: nonuniform legacy Text scale is unsupported"
        );
    }
    if matches!(component, GraphicComponent::Text) {
        if let Some((identity, width, in_native_bounds)) = current_text_width {
            if transform.uniform {
                if !in_native_bounds {
                    transform.approximations.push(format!(
                        "{identity}: inactive Horizontal Scale value {width} is outside native bounds and is discarded because Uniform Scale uses Scale for both axes"
                    ));
                }
            } else {
                ensure!(
                    in_native_bounds,
                    "{identity}: graphic scale is outside its native bounds"
                );
            }
        }
        ensure!(
            !vertical_name || !transform.uniform,
            "{owner}: Vertical Scale requires disabled Uniform Scale"
        );
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

/// Temporal Position speed was measured on straight paths. Collinear automatic
/// handles describe the same path; the editable mapper discards them too.
fn animation_is_straight(animation: &PrPropertyAnimation) -> bool {
    let PrPropertyAnimation::Position(keys) = animation else {
        return false;
    };
    crate::schema::spatial::curved_segment(keys).is_none()
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

#[cfg(test)]
mod static_content_tests {
    use super::*;
    use crate::schema::text::{PrGraphicGroup, PrMaskSource};

    #[test]
    fn subgroups_inspect_mask_source_child_clocks() {
        let mut graphic = crate::tests::support::text_graphic();
        graphic.text_mut().mask_source = Some(PrMaskSource { inverted: false });
        let grouped = |objects| {
            PrGraphicObject::Group(PrGraphicGroup {
                name: String::new(),
                objects,
            })
        };
        assert!(object_is_static(&grouped(graphic.objects.clone())));
        graphic
            .text_mut()
            .animations
            .push(PrPropertyAnimation::UniformScale(vec![
                crate::schema::PrScalarKeyframe {
                    source_ticks: 0,
                    value: 100.0,
                    easing: crate::schema::PrKeyframeEasing::Linear,
                },
            ]));
        assert!(!object_is_static(&grouped(graphic.objects.clone())));
        graphic.text_mut().animations.clear();
        let document = graphic.text().document.clone();
        graphic.text_mut().source_text_keys.push(PrSourceTextKey {
            source_ticks: 0,
            document,
        });
        assert!(!object_is_static(&grouped(graphic.objects)));
    }
}
