//! Caption tracks: timed caption cues as editable text graphics.
//!
//! Each caption track becomes one lane of timed [`PrGraphic`]s appended above
//! the video tracks, a higher track Index above a lower one; overlapping cues
//! on different caption tracks stay separate layers and gaps stay gaps. An AME
//! render draws both caption tracks over the video and superimposes their
//! overlapping cues at one position, the higher track Index on top
//! (Adobe-verified: the `feature_caption_styles_26_5_strict` fixture's C7
//! stroke and shadow cover C1's white). A cue converts when the single-style
//! text path represents it (wording, timing, font, size, fill, stroke, shadow
//! and background box, as for Type-tool text). Premiere draws each cue from
//! its own `FormattedTextData`; the track's `CaptionDataTemplateStyle` is only
//! a default (Adobe-verified: a cue with its own fill draws that fill, and a
//! cue without a background over a background template draws no box), so a
//! cue's style is read from its payload without decoding the unused template.
//! Missing or unsupported template metadata is diagnosed. Caption content never
//! fails its sequence; a track or cue that cannot convert is omitted with its reason.
//!
//! `IsMuted=true` on a caption track or cue is read as track output or cue
//! Enable off, as for video clips, so the cue imports as a hidden layer. This
//! reading is inferred: no corpus caption track or cue is muted.
//!
//! The default-position corpus cues store no geometry: a cue records only
//! center justification and bottom alignment. Premiere draws that
//! bottom-centre default with the last line's baseline at 95% of the frame
//! height, centred, and stacks earlier lines above it at 120% of the size,
//! whatever the size, the line count and the project's title-safe and
//! action-safe margins (Adobe-verified by AME renders; see the
//! conversion-accuracy ledger in `apps/tesseract-conv/README.md`). The reader
//! writes each cue as bottom-aligned box text that lands there, with the
//! cue's center or left justification; a left cue starts 12.5% of the frame
//! width in (inferred from two AME renders, x 240 on 1920). A positioned cue
//! (document slot 33) fails closed in the payload decoder.
//!
//! Premiere also scales caption size with the frame, by a rule measured only
//! as 2/3 on 720x1280, so on a canvas other than 1920x1080 each cue keeps its
//! stored size and its sequence reports that approximation once.

use super::{
    integer, required, required_integer,
    video::{playback_rate, report_markers, report_unknown_children},
    visibility,
};
use crate::error::{ensure, unsupported, BuildError, Result};
use crate::format::{
    graph::Element,
    text_payload::{self, DecodedText, OmittedTextFeature},
    FrameRate, Graph, Located, Record,
};
use crate::schema::{
    caption::{self, CAPTION_MEDIA_TOKEN},
    native::{
        Block, CaptionDataClipTrackItem, DataComponentChain, DataMediaSource, DataStream,
        EncodedValue, MasterClip, Media, Reference, SubClip, TranscriptClip,
    },
    records,
    text::{
        self, PrGraphicObject, PrJustification, PrTextDocument, PrTextFrame, PrTextTransform,
        PrVerticalAlign,
    },
    PrBlendMode, PrGraphic, PrSequence, PrStaticTransform, PrText, PrVideoItem, PrVideoTrack,
};
use crate::{approximate, omit, Omission, OmissionScope};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::BTreeSet;

/// The frame on which Premiere draws a caption at its stored size (the pinned
/// AME renders); on 720x1280 it drew the 48 px cues at 2/3 size.
const STORED_SIZE_FRAME: [u32; 2] = [1920, 1080];

/// Append each caption track of `data_groups` above `sequence`'s video tracks.
///
/// A cue is labelled by its track (`C1` for track Index 0) and its position
/// on the track, so an omission names `C1 caption 12` as well as its record.
pub(super) fn read_caption_tracks(
    graph: &Graph<'_>,
    data_groups: &[Record<'_>],
    sequence: &mut PrSequence,
    _nesting: &mut super::nested::Nesting<'_>,
    omissions: &mut Vec<Omission>,
) {
    // Malformed groups and non-caption data tracks are reported with the
    // other non-video tracks.
    let track_records: Vec<_> = data_groups
        .iter()
        .filter_map(|group| Some((group.identity(), group.track_references().ok()?)))
        .flat_map(|(identity, references)| {
            references
                .into_iter()
                .filter_map(move |reference| graph.locate(&reference, &identity).ok())
        })
        .filter(|record| record.tag() == caption::CAPTION_DATA_CLIP_TRACK.tag)
        .collect();
    if track_records.is_empty() {
        return;
    }
    let mut tracks = Vec::with_capacity(track_records.len());
    let mut seen_indices = BTreeSet::new();
    for record in track_records {
        match read_track(graph, record, &seen_indices, omissions) {
            Ok(track) => {
                seen_indices.insert(track.index);
                tracks.push(track);
            }
            Err(error) => omit(
                omissions,
                OmissionScope::Track,
                record.identity(),
                error.to_string(),
            ),
        }
    }
    tracks.sort_by_key(|track| track.index);
    let mut seen_cues = BTreeSet::new();
    let video_lanes = sequence.video_tracks.len();
    for track in tracks {
        let mut graphics = Vec::with_capacity(track.cues.len());
        for (position, reference) in track.cues.iter().enumerate() {
            let label = format!("C{} caption {}", i128::from(track.index) + 1, position + 1);
            let cue = match graph.locate(reference, &track.identity) {
                Ok(cue) => cue,
                Err(error) => {
                    let identity = reference.id.as_deref().or(reference.uid.as_deref());
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        identity.unwrap_or("unidentified caption"),
                        format!("{label}: {error}"),
                    );
                    continue;
                }
            };
            let converted = if seen_cues.insert(cue.identity()) {
                read_cue(graph, cue, &track, &label, sequence, omissions)
            } else {
                Err(unsupported("duplicate caption reference"))
            };
            match converted {
                Ok(graphic) => graphics.push(graphic),
                Err(error) => omit(
                    omissions,
                    OmissionScope::Occurrence,
                    cue.identity(),
                    format!("{label}: {error}"),
                ),
            }
        }
        graphics.sort_by_key(|graphic| graphic.start_ticks);
        let mut items: Vec<PrVideoItem> = Vec::with_capacity(graphics.len());
        for graphic in graphics {
            if items
                .last()
                .is_some_and(|last| last.timeline_ticks().end > graphic.start_ticks)
            {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    graphic.id().unwrap_or_default(),
                    format!(
                        "{}: overlaps an earlier caption on this track",
                        graphic.texts().next().map_or("", |text| &text.name)
                    ),
                );
            } else {
                items.push(PrVideoItem::Graphic(graphic));
            }
        }
        // A converted cue extends the sequence end, as converted audio does; an
        // omitted cue does not. Whether AME renders a caption past the last video
        // item is unverified: the sequence-end probes covered video items only.
        if let Some(end) = items.last().map(|item| item.timeline_ticks().end) {
            sequence.timeline_end_ticks = sequence.timeline_end_ticks.max(end);
            sequence.video_tracks.push(PrVideoTrack {
                items,
                nests: Vec::new(),
                transitions: Vec::new(),
            });
        }
    }
    let [width, height] = sequence.dimensions();
    if sequence.video_tracks.len() > video_lanes && [width, height] != STORED_SIZE_FRAME {
        approximate(
            omissions,
            format!(
                "{} ({:?})",
                sequence.id.as_deref().unwrap_or_default(),
                sequence.name
            ),
            format!(
                "caption size on a {width}x{height} canvas: every converted cue keeps its stored pixel size; Premiere draws a caption at that size on 1920x1080 and scales it with another frame (48 px cues drew at 2/3 size on 720x1280) by an unmeasured rule, so a cue's text, line spacing, stroke, shadow and background box can draw larger or smaller than in Premiere"
            ),
        );
    }
}

/// The bottom caption default as box text, bottom-aligned, with its pivot on
/// the last line's baseline at the frame's horizontal centre so that edits
/// move and scale the caption from there.
struct CaptionRegion {
    frame: PrTextFrame,
    transform: PrTextTransform,
}

/// Place a bottom cue of `size` px where Premiere draws it: the last line's
/// baseline at 95% of the frame height, centred, or starting 12.5% of the
/// frame width in when left-justified.
///
/// The FX renderer's bottom-aligned box text without an authored first baseline puts
/// the first baseline `size` below the box top
/// (`fx_schema::TextDocument::box_first_baseline`) and moves the text
/// down by whole line slots of 120% of the size
/// (`scene::box_vertical_align_offset`), so the last line lands in the lowest
/// slot that fits. The box therefore holds as many slots as fit above the
/// baseline, plus about half a slot, and its pivot is that lowest slot's
/// baseline: any line count up to the box's capacity ends on the same
/// baseline. A centred box keeps 80% of the frame width as its wrap width;
/// no fixture cue wraps, so Premiere's wrap width is unverified. A
/// left-justified box spans the middle 75%: one left cue's ink started at x
/// 242 and one right cue's ended at x 1677 on 1920, so both ends sit 12.5% in
/// (INFERRED from those two cues).
fn bottom_centre_region(
    frame: [u32; 2],
    size: f32,
    justification: PrJustification,
) -> CaptionRegion {
    let [width, height] = frame.map(f64::from);
    let size = f64::from(size);
    let baseline = height - height / 20.0;
    let slots = ((baseline - size) * 5.0 / (6.0 * size)).floor().max(0.0);
    // `size + slots * 1.2 * size` and half a slot below it, each rounded once.
    let last_baseline = size * (5.0 + 6.0 * slots) / 5.0;
    let box_height = (size * (8.0 + 6.0 * slots) / 5.0).round();
    let box_width = match justification {
        PrJustification::Left => width * 3.0 / 4.0,
        _ => width * 4.0 / 5.0,
    };
    CaptionRegion {
        // Premiere stores box sizes as 32-bit floats; whole-pixel sizes are exact.
        frame: PrTextFrame::Box {
            width: box_width as f32,
            height: box_height as f32,
            vertical: PrVerticalAlign::Bottom,
        },
        transform: PrTextTransform {
            position: [width / 2.0, baseline],
            anchor: [box_width / 2.0, last_baseline],
            scale: 100.0,
            rotation: 0.0,
            opacity: 100.0,
        },
    }
}

/// One caption track and what each of its cues is checked against.
struct CaptionTrack {
    identity: String,
    index: i64,
    cues: Vec<Reference>,
    /// False when the track's `IsMuted` is `true`: its output is off (inferred).
    visible: bool,
}

/// Read one caption track. An error omits the track.
fn read_track(
    graph: &Graph<'_>,
    record: Record<'_>,
    seen_indices: &BTreeSet<i64>,
    omissions: &mut Vec<Omission>,
) -> Result<CaptionTrack> {
    let identity = record.identity();
    let root = record.element();
    report_unknown_children(
        root,
        &["DataClipTrack", "CaptionDataTemplateStyle"],
        &identity,
        "",
        omissions,
    );
    let clip_track_element = root
        .child("DataClipTrack")
        .and_then(|data| data.child("ClipTrack"));
    if let Some(clip_track) = clip_track_element {
        let path = "DataClipTrack/ClipTrack/";
        for (child, allowed) in [
            (
                "Track",
                &[
                    "Node",
                    "ID",
                    "IsLocked",
                    "MediaType",
                    "Index",
                    "IsMuted",
                    "IsSyncLocked",
                    "Name",
                ][..],
            ),
            ("ClipItems", &["MediaType", "Index", "TrackItems"][..]),
            ("TransitionItems", &["MediaType", "Index"][..]),
        ] {
            if let Some(element) = clip_track.child(child) {
                report_unknown_children(
                    element,
                    allowed,
                    &identity,
                    &format!("{path}{child}/"),
                    omissions,
                );
            }
        }
    }
    // Decode only the consumed track fields: even the template's encoding is
    // optional metadata, not a dependency of a cue's FormattedTextData.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct TrackFields {
        data_clip_track: Option<crate::schema::native::DataClipTrack>,
    }
    let track = graph.decode::<TrackFields>(record)?;
    let clip_track = required(
        track.value.data_clip_track.and_then(|data| data.clip_track),
        &identity,
        "DataClipTrack/ClipTrack",
    )?;
    let fields = required(clip_track.track.as_ref(), &identity, "Track")?;
    let visible = !visibility::is_muted(fields.is_muted.as_deref(), &identity)?;
    let index = match fields.index.as_deref() {
        Some(value) => integer(value, &format!("{identity}: invalid track Index"))?,
        None => 0,
    };
    ensure!(index >= 0, "caption track Index must be nonnegative");
    ensure!(
        !seen_indices.contains(&index),
        "duplicate caption track Index {index}"
    );
    let clip_items = required(clip_track.clip_items.as_ref(), &identity, "ClipItems")?;
    for (holder, holder_index, media_type) in [
        (
            "Track",
            fields.index.as_deref(),
            fields.media_type.as_deref(),
        ),
        (
            "ClipItems",
            clip_items.index.as_deref(),
            clip_items.media_type.as_deref(),
        ),
        (
            "TransitionItems",
            clip_track
                .transition_items
                .as_ref()
                .and_then(|items| items.index.as_deref()),
            clip_track
                .transition_items
                .as_ref()
                .and_then(|items| items.media_type.as_deref()),
        ),
    ] {
        ensure!(
            holder_index.is_none_or(|value| value.parse::<i64>().ok() == Some(index))
                && media_type.is_none_or(|value| value == records::DATA_MEDIA),
            "conflicting caption {holder} Index or MediaType"
        );
    }
    let cues = clip_track
        .clip_items
        .and_then(|items| items.track_items)
        .map_or_else(Vec::new, |items| items.items);
    let template_reason = match root.child("CaptionDataTemplateStyle") {
        None => Some("missing CaptionDataTemplateStyle"),
        Some(style) => match style.attribute("Encoding") {
            None => Some("missing CaptionDataTemplateStyle encoding"),
            Some(records::ENCODING) => None,
            Some(_) => Some("unsupported CaptionDataTemplateStyle encoding"),
        },
    };
    if let Some(reason) = template_reason {
        omit(
            omissions,
            OmissionScope::Feature,
            &identity,
            format!("caption default template not used: {reason}; each admitted cue uses its own FormattedTextData"),
        );
    }
    Ok(CaptionTrack {
        identity,
        index,
        cues,
        visible,
    })
}

/// Read one caption cue. An error omits the cue.
fn read_cue(
    graph: &Graph<'_>,
    record: Record<'_>,
    track: &CaptionTrack,
    label: &str,
    sequence: &PrSequence,
    omissions: &mut Vec<Omission>,
) -> Result<PrGraphic> {
    let mut cue_notes = Vec::new();
    let identity = record.identity();
    let root = record.element();
    report_unknown_children(
        root,
        &["DataClipTrackItem", "BlockVector", "LanguageCodeISO"],
        &identity,
        "",
        &mut cue_notes,
    );
    let clip_track_item = root
        .child("DataClipTrackItem")
        .and_then(|data| data.child("ClipTrackItem"));
    if let Some(item) = clip_track_item {
        let path = "DataClipTrackItem/ClipTrackItem/";
        report_unknown_children(
            item,
            &["ComponentOwner", "TrackItem", "SubClip", "IsMuted"],
            &identity,
            path,
            &mut cue_notes,
        );
        if let Some(owner) = item.child("ComponentOwner") {
            report_unknown_children(
                owner,
                &["Components"],
                &identity,
                &format!("{path}ComponentOwner/"),
                &mut cue_notes,
            );
        }
    }
    let item = graph.decode_as::<CaptionDataClipTrackItem>(record, &track.identity)?;
    let clip_track_item = required(
        item.value
            .data_clip_track_item
            .and_then(|data| data.clip_track_item),
        &identity,
        "DataClipTrackItem/ClipTrackItem",
    )?;
    let enabled = !visibility::is_muted(clip_track_item.is_muted.as_deref(), &identity)?;
    let range = required(clip_track_item.track_item.as_ref(), &identity, "TrackItem")?;
    let start = match range.start.as_deref() {
        Some(value) => integer(value, &format!("{identity}: invalid Start"))?,
        None => 0,
    };
    let end = integer(&range.end, &format!("{identity}: invalid End"))?;
    let components = required(
        clip_track_item
            .component_owner
            .as_ref()
            .and_then(|owner| owner.components.as_ref()),
        &identity,
        "caption component chain",
    )?;
    let chain = graph.follow::<DataComponentChain>(components, &identity)?;
    ensure!(
        chain
            .value
            .component_chain
            .and_then(|chain| chain.components)
            .is_none_or(|components| components.items.is_empty()),
        "{}: caption effects are unsupported",
        chain.identity
    );
    let sub_clip = required(clip_track_item.sub_clip.as_ref(), &identity, "SubClip")?;
    validate_source(
        graph,
        sub_clip,
        &identity,
        end.checked_sub(start),
        sequence.native_frame_rate(),
        &mut cue_notes,
    )?;
    let blocks = required(item.value.block_vector.as_ref(), &identity, "BlockVector")?;
    let [block] = blocks.items.as_slice() else {
        return Err(unsupported(format!(
            "captions with {} text blocks are unsupported",
            blocks.items.len()
        )));
    };
    let mut decoded = read_block(graph, block, &identity)?;
    let unconverted = unconverted_attributes(&decoded);
    ensure!(
        unconverted.is_empty(),
        "{} not converted",
        unconverted.join(", ")
    );
    // Run metadata is not a background and does not invalidate supported cues.
    // Report it separately so it cannot hide an independent background warning.
    let background_reason = {
        for feature in &decoded.omitted {
            if matches!(feature, OmittedTextFeature::RunStyleMetadata { .. }) {
                approximate(&mut cue_notes, &identity, feature.to_string());
            }
        }
        match decoded
            .omitted
            .iter()
            .find(|feature| !matches!(feature, OmittedTextFeature::RunStyleMetadata { .. }))
        {
            Some(OmittedTextFeature::UnknownRootData(_) | OmittedTextFeature::AlternateDocumentMarkers | OmittedTextFeature::DefaultRunStyle) => return Err(unsupported("unidentified caption Source Text root data")),
            Some(OmittedTextFeature::Background) => Some(
                "its payload stores no opacity or corner radius, and what Premiere draws then is unverified"
                    .to_owned(),
            ),
            Some(OmittedTextFeature::MissingRunMarker) => {
                return Err(unsupported("caption text requires an explicit run marker"));
            }
            // Caption decoding does not admit legacy graphic Source Text.
            Some(OmittedTextFeature::LegacyControl(_)) => {
                return Err(unsupported("legacy graphic text controls do not apply to captions"));
            }
            Some(OmittedTextFeature::RunStyleMetadata { .. }) | None => decoded
                .document
                .background
                .and_then(|background| background.unverified_reason(&decoded.document)),
        }
    };
    if let Some(reason) = background_reason {
        omit(
            &mut cue_notes,
            OmissionScope::Feature,
            &identity,
            format!("{label}: text background not converted: {reason}"),
        );
        decoded.document.background = None;
    }
    let region = bottom_centre_region(
        [sequence.width, sequence.height],
        decoded.document.size,
        decoded.document.justification,
    );
    let graphic = PrGraphic {
        id: Some(identity),
        start_ticks: start,
        end_ticks: end,
        in_ticks: sequence.native_frame_rate().generator_in_ticks(),
        vector_motion: None,
        clip_motion: PrStaticTransform::default(),
        opacity: 100.0,
        blend_mode: PrBlendMode::Normal,
        animations: Vec::new(),
        opacity_mask: None,
        effect_loss: None,
        objects: vec![PrGraphicObject::Text(PrText {
            horizontal_scale: None,
            mask_source: None,
            name: label.to_owned(),
            document: PrTextDocument {
                frame: region.frame,
                ..decoded.document
            },
            transform: region.transform,
            animations: Vec::new(),
            source_text_keys: Vec::new(),
        })],
        enabled: enabled && track.visible,
    };
    graphic.validate(sequence.native_frame_rate())?;
    omissions.extend(cue_notes);
    Ok(graphic)
}

/// Caption attributes that the single-style bottom text path cannot
/// represent: no AME render shows a caption with them.
fn unconverted_attributes(decoded: &DecodedText) -> Vec<&'static str> {
    let document = &decoded.document;
    let mut attributes = Vec::new();
    for (present, attribute) in [
        (document.fill.is_none(), "disabled text fill"),
        (document.all_caps, "all caps"),
        (document.tracking != 0.0, "tracking"),
        (document.leading != 0.0, "leading"),
        (
            !matches!(
                document.justification,
                PrJustification::Center | PrJustification::Left
            ),
            "justification other than center or left",
        ),
        (
            decoded.box_alignment != PrVerticalAlign::Bottom,
            "alignment other than bottom",
        ),
        (
            !matches!(document.frame, PrTextFrame::Point { .. }),
            "text box",
        ),
    ] {
        if present {
            attributes.push(attribute);
        }
    }
    attributes
}

/// Decode the cue's one text block.
fn read_block(graph: &Graph<'_>, reference: &Reference, from: &str) -> Result<DecodedText> {
    let block = graph.follow::<Block>(reference, from)?;
    let encoded = required(
        block.value.formatted_text_data.as_ref(),
        &block.identity,
        "FormattedTextData",
    )?;
    decode_payload(graph, encoded, &block.identity, "FormattedTextData")
}

/// Decode `owner`'s caption Source Text `field`, resolving a deduplicated
/// value by its `BinaryHash`.
fn decode_payload(
    graph: &Graph<'_>,
    encoded: &EncodedValue,
    owner: &str,
    field: &str,
) -> Result<DecodedText> {
    ensure!(
        encoded.encoding == records::ENCODING,
        "{owner}: unsupported {field} encoding"
    );
    let stored = if encoded.value.trim().is_empty() {
        let hash = required(encoded.binary_hash.as_deref(), owner, "BinaryHash")?;
        graph
            .binary_value(hash, owner)?
            .ok_or_else(|| unsupported(format!("{owner}: {field} names missing binary {hash}")))?
    } else {
        encoded.value.as_str()
    };
    let compact: String = stored.split_whitespace().collect();
    let payload = STANDARD
        .decode(compact)
        .map_err(|error| unsupported(format!("{owner}: invalid {field} base64: {error}")))?;
    text_payload::decode_caption(&payload).map_err(|error| match error {
        BuildError::Unsupported(message) => unsupported(format!("{owner}: {field}: {message}")),
        other => other,
    })
}

/// Check that the cue plays its synthetic caption source at unit speed.
fn validate_source(
    graph: &Graph<'_>,
    sub_clip: &Reference,
    from: &str,
    duration: Option<i64>,
    frame_rate: FrameRate,
    omissions: &mut Vec<Omission>,
) -> Result<()> {
    let sub = graph.follow::<SubClip>(sub_clip, from)?;
    let clip = graph.follow::<TranscriptClip>(&sub.value.clip, &sub.identity)?;
    let placed = required(
        clip.value
            .data_clip
            .as_ref()
            .and_then(|data| data.clip.as_ref()),
        &clip.identity,
        "DataClip/Clip",
    )?;
    ensure!(
        playback_rate(placed, &clip.identity)? == 1.0 && placed.time_remapping.is_none(),
        "{}: caption retiming is unsupported",
        clip.identity
    );
    report_markers(graph, placed, &clip.identity, omissions);
    let source_in = required_integer(placed.in_point.as_deref(), &clip.identity, "InPoint")?;
    let source_out = required_integer(placed.out_point.as_deref(), &clip.identity, "OutPoint")?;
    ensure!(
        source_in >= 0 && source_out.checked_sub(source_in) == duration,
        "{}: caption retiming is unsupported",
        clip.identity
    );
    let source_reference = required(placed.source.as_ref(), &clip.identity, "Source")?;
    let source = graph.follow::<DataMediaSource>(source_reference, &clip.identity)?;
    let generator_ticks = text::GRAPHIC_MEDIA_TICKS.to_string();
    ensure!(
        source.value.original_duration.as_deref() == Some(generator_ticks.as_str()),
        "{}: unexpected caption source duration",
        source.identity
    );
    let media_reference = required(
        source
            .value
            .media_source
            .as_ref()
            .and_then(|media_source| media_source.media.as_ref()),
        &source.identity,
        "MediaSource/Media",
    )?;
    let media_record = graph.locate(media_reference, &source.identity)?;
    let media = graph.decode_as::<Media>(media_record, &source.identity)?;
    let value = &media.value;
    ensure!(
        value.implementation_id.as_deref() == Some(text::GRAPHIC_IMPLEMENTATION_ID)
            && [&value.file_path, &value.actual_media_file_path]
                .iter()
                .all(|path| path
                    .as_deref()
                    .is_none_or(|path| path == CAPTION_MEDIA_TOKEN))
            && value.relative_paths.is_empty()
            && value.infinite.as_deref() == Some("true")
            && value.video_stream.is_none()
            && value.audio_stream.is_none(),
        "{}: unexpected caption generator media",
        media.identity
    );
    let stream_reference = required(
        media_record
            .element()
            .child(caption::DATA_STREAM.tag)
            .map(Element::reference),
        &media.identity,
        caption::DATA_STREAM.tag,
    )?;
    let stream = graph.follow::<DataStream>(&stream_reference, &media.identity)?;
    ensure!(
        stream.value.duration.as_deref() == Some(generator_ticks.as_str())
            && stream.value.frame_rate.as_deref()
                == Some(frame_rate.ticks_per_frame().to_string().as_str()),
        "{}: caption stream duration or rate differs from its sequence",
        stream.identity
    );
    if let Some(master) = &sub.value.master_clip {
        validate_master(graph, master, &sub.identity, &source)?;
    }
    Ok(())
}

/// The cue's master clip must hold one template clip of the same caption source.
fn validate_master(
    graph: &Graph<'_>,
    master: &Reference,
    from: &str,
    source: &Located<DataMediaSource>,
) -> Result<()> {
    let master = graph.follow::<MasterClip>(master, from)?;
    let clips = required(master.value.clips.as_ref(), &master.identity, "Clips")?;
    ensure!(
        clips.items.len() == 1,
        "{}: multiple caption source clips are unsupported",
        master.identity
    );
    let template = graph.follow::<TranscriptClip>(&clips.items[0], &master.identity)?;
    let template_source = required(
        template
            .value
            .data_clip
            .as_ref()
            .and_then(|data| data.clip.as_ref())
            .and_then(|clip| clip.source.as_ref()),
        &template.identity,
        "DataClip/Clip/Source",
    )?;
    ensure!(
        graph
            .locate(template_source, &template.identity)?
            .identity()
            == source.identity,
        "{}: caption source identity mismatch",
        master.identity
    );
    Ok(())
}
