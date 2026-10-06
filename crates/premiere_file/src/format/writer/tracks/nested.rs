//! Nested-sequence placements and the inner sequences they play.
//!
//! A placement is a VideoClip whose source is the inner sequence's
//! VideoSequenceSource, a SubClip whose master is the inner sequence's master
//! clip, the component chain of a media placement, and a VideoClipTrackItem.
//! A placement whose sequence has sound also has an audio item of that
//! sequence's AudioSequenceSource, linked to the video item. Every placement
//! has its own inner sequence, written with the ordinary sequence records.

use super::{audio, graphic, video};
use crate::format::writer::{
    graph::{ItemIds, MediaIds, NestIds, NestSoundIds, SequenceGraphIds},
    sequence, valid_xml_text, BoundMedia,
};
use crate::format::{invalid, Result};
use crate::schema::{
    native::*, records, text::PrGraphicObject, AudioChannels, MediaId, PrNestOccurrence,
    PrSequence, PrVideoItem, PrVideoTrack, ToneMapSettings,
};
use std::collections::BTreeMap;

/// Checks writer-only restrictions throughout the nested sequence tree.
pub(in crate::format::writer) fn validate_sequences(spec: &PrSequence) -> Result<()> {
    if spec.audio.iter().any(|clip| clip.source_channel.is_some()) {
        return Err(invalid(
            "selected audio channels must be extracted before native writing",
        ));
    }
    if spec.video_tracks.iter().any(|track| {
        track.transitions.iter().any(|transition| {
            transition.kind != crate::schema::PrVideoTransitionKind::CrossDissolve
        })
    }) {
        return Err(invalid(
            "writer supports only native Cross Dissolve New video transitions",
        ));
    }
    // A text or shape layer name becomes its component's InstanceName, which
    // the model leaves unconstrained.
    for (index, track) in spec.video_tracks.iter().enumerate() {
        for graphic in track.items.iter().filter_map(PrVideoItem::graphic) {
            let mut remaining: Vec<_> = graphic.objects.iter().collect();
            while let Some(object) = remaining.pop() {
                let (kind, name) = match object {
                    PrGraphicObject::Text(text) => ("text", &text.name),
                    PrGraphicObject::Shape(shape) => ("shape", &shape.name),
                    PrGraphicObject::Group(group) => {
                        remaining.extend(&group.objects);
                        ("SubGroup", &group.name)
                    }
                    PrGraphicObject::TextLines(_) => {
                        return Err(invalid(
                            "native mixed text must be converted to editable line objects before writing",
                        ))
                    }
                };
                if !valid_xml_text(name) {
                    return Err(invalid(format!(
                        "sequence {:?}, video track {index}, {}: {kind} layer name must contain only valid XML characters",
                        spec.name,
                        graphic.id.as_deref().unwrap_or("graphic")
                    )));
                }
            }
        }
    }
    for nest in spec.nest_occurrences() {
        let name = &nest.sequence.name;
        if name.is_empty() || name.chars().count() > 255 || !valid_xml_text(name) {
            return Err(invalid(
                "nested sequence name must contain 1 to 255 valid XML characters",
            ));
        }
        validate_sequences(&nest.sequence)?;
    }
    Ok(())
}

/// Track item references of one track, its items and nested placements in timeline order.
pub(in crate::format::writer) fn track_items(
    track: &PrVideoTrack,
    placements: &[ItemIds],
    nests: &[NestIds],
) -> Vec<ObjectId<VideoClipTrackItemId>> {
    let mut items: Vec<_> = track
        .items
        .iter()
        .zip(placements)
        .map(|(item, ids)| (item.timeline_ticks().start, ids.track_item()))
        .chain(
            track
                .nests
                .iter()
                .zip(nests)
                .map(|(nest, ids)| (nest.start_ticks, ids.placement.track_item)),
        )
        .collect();
    items.sort_by_key(|(start, _)| *start);
    items.into_iter().map(|(_, id)| id).collect()
}

/// Records of every nested placement in `spec` and of its audio item, each
/// followed by its inner sequence, that sequence's media, graphic and sound
/// placements, and its own nests.
pub(in crate::format::writer) fn records(
    spec: &PrSequence,
    ids: &SequenceGraphIds,
    bound_media: &BTreeMap<&MediaId, BoundMedia<'_>>,
    media_ids: &BTreeMap<&MediaId, &MediaIds>,
) -> Result<Vec<Record>> {
    let mut output = Vec::new();
    for (track, nest_ids) in spec.video_tracks.iter().zip(&ids.nests) {
        for (nest, ids) in track.nests.iter().zip(nest_ids) {
            output.extend(placement_records(spec, nest, ids)?);
            if let Some(sound) = &ids.sound {
                output.extend(sound_records(nest, ids, sound));
            }
            let inner = &nest.sequence;
            output.extend(sequence::records(inner, &ids.inner)?);
            for (item, item_ids) in inner
                .video_items()
                .zip(ids.inner.placements.iter().flatten())
            {
                match (item, item_ids) {
                    (PrVideoItem::Media(occurrence), ItemIds::Media(placement)) => {
                        output.extend(video::placement_records(
                            inner,
                            occurrence,
                            bound_media[&occurrence.media].media(),
                            media_ids[&occurrence.media],
                            placement,
                        )?);
                    }
                    (PrVideoItem::Graphic(item), ItemIds::Graphic(graphic_ids)) => {
                        output.extend(graphic::records(inner, item, graphic_ids)?);
                    }
                    (PrVideoItem::Capsule(_), _) => return Err(crate::format::invalid("native Capsule replay/export is unsupported; import as editable FX instead")),
                    _ => unreachable!("ProjectIds allocates identities of each item's own kind"),
                }
            }
            for (occurrence, placement) in inner.audio.iter().zip(&ids.inner.audio_placements) {
                output.extend(audio::placement_records(
                    occurrence,
                    bound_media[&occurrence.media].media(),
                    media_ids[&occurrence.media],
                    placement,
                )?);
            }
            output.extend(records(inner, &ids.inner, bound_media, media_ids)?);
        }
    }
    Ok(output)
}

/// The audio item that plays the stereo mix of `nest`'s sequence over the
/// placement's ranges, with the video item's Enable, and the link of the two,
/// as Premiere 26.5.1 saves them (`premiere_isolated_images_nests_26_5`: item
/// 115, Link 107). The item keeps Premiere's default Volume: import folds an
/// item's gain into the inner sounds, which export writes instead.
fn sound_records(nest: &PrNestOccurrence, ids: &NestIds, sound: &NestSoundIds) -> Vec<Record> {
    let source = ids.inner.sequence.audio_source;
    let mut clip = sequence::sequence_clip(source, sound.clip_uid.clone());
    clip.in_point = Some(nest.in_ticks.to_string());
    clip.out_point = Some(nest.out_ticks.to_string());
    clip.in_use = None;
    let mut output = vec![
        Record::AudioClip(AudioClip {
            object_id: sound.clip,
            class_id: Some(records::AUDIO_CLIP.class_id.into()),
            version: Some(records::AUDIO_CLIP.version.into()),
            clip,
            secondary_contents: SecondaryContents::from_ids(sound.secondary),
            audio_channel_layout: records::STEREO.into(),
            gain: None,
            audio_time_scaler_settings: None,
        }),
        audio::default_chain(sound.components, AudioChannels::Stereo),
        Record::SubClip(SubClip {
            object_id: sound.subclip,
            class_id: Some(records::SUB_CLIP.class_id.to_owned()),
            version: Some(records::SUB_CLIP.version.to_owned()),
            clip: Reference::object(sound.clip),
            master_clip: Some(Reference::uid(ids.inner.sequence.master)),
            name: Some(nest.sequence.name.clone()),
            original_channel_group: Some("0".to_owned()),
        }),
        Record::AudioClipTrackItem(AudioClipTrackItem {
            object_id: sound.track_item,
            class_id: Some(records::AUDIO_CLIP_TRACK_ITEM.class_id.into()),
            version: Some(records::AUDIO_CLIP_TRACK_ITEM.version.into()),
            clip_track_item: ClipTrackItem {
                version: Some("8".into()),
                component_owner: Some(ComponentOwner::audio(sound.components)),
                track_item: Some(TrackItemRange {
                    version: Some("4".into()),
                    _node: None,
                    _item_type: None,
                    _media_type: None,
                    _track_index: None,
                    _track_ref_count: None,
                    start: (nest.start_ticks != 0).then(|| nest.start_ticks.to_string()),
                    end: nest.end_ticks.to_string(),
                }),
                sub_clip: Some(Reference::object(sound.subclip)),
                head_transition: None,
                tail_transition: None,
                is_muted: (!nest.enabled).then(|| "true".to_owned()),
                original_sub_clip_time_offset: None,
            },
        }),
    ];
    output.extend(
        sound
            .secondary
            .iter()
            .enumerate()
            .map(|(index, id)| audio::secondary_content(*id, Reference::object(source), index)),
    );
    output.push(Record::Link(Link {
        object_id: sound.link,
        class_id: records::LINK.class_id,
        version: records::LINK.version,
        group: LinkedItems {
            version: "1",
            items: LinkedItemList {
                version: "1",
                items: vec![
                    Reference::indexed_object(0, ids.placement.track_item),
                    Reference::indexed_object(1, sound.track_item),
                ],
            },
        },
    }));
    output
}

fn placement_records(
    outer: &PrSequence,
    nest: &PrNestOccurrence,
    ids: &NestIds,
) -> Result<Vec<Record>> {
    let placement = &ids.placement;
    let tone_map =
        serde_json::to_string(&ToneMapSettings::DEFAULT).expect("native tone-map fields serialize");
    let mut clip =
        sequence::sequence_clip(ids.inner.sequence.video_source, placement.clip_uid.clone());
    clip.in_point = Some(nest.in_ticks.to_string());
    clip.out_point = Some(nest.out_ticks.to_string());
    clip.in_use = None;
    let mut output = vec![
        Record::VideoClip(VideoClip {
            object_id: Some(placement.placed_clip.as_native_string()),
            class_id: Some(records::VIDEO_CLIP.class_id.into()),
            version: Some(records::VIDEO_CLIP.version.into()),
            clip: Some(clip),
            adjustment_layer: None,
            time_interpolation_type: None,
            scale_to_frame_policy: None,
            _poster_frame: None,
            frame_blend: None,
            field_processing: None,
            hold_filters: None,
            deinterlace_on_hold: None,
            reverse_field_dominance: None,
            scale_to_frame_size: None,
            frame_hold: None,
            frame_hold_start: None,
        }),
        Record::SubClip(SubClip {
            object_id: placement.subclip,
            class_id: Some(records::SUB_CLIP.class_id.to_owned()),
            version: Some(records::SUB_CLIP.version.to_owned()),
            clip: Reference::object(placement.placed_clip),
            master_clip: Some(Reference::uid(ids.inner.sequence.master)),
            name: Some(nest.sequence.name.clone()),
            original_channel_group: Some("0".to_owned()),
        }),
        video::component_chain(nest.into(), placement),
        Record::VideoClipTrackItem(VideoClipTrackItem {
            object_id: Some(placement.track_item.as_native_string()),
            class_id: Some(records::VIDEO_CLIP_TRACK_ITEM.class_id.into()),
            version: Some(records::VIDEO_CLIP_TRACK_ITEM.version.into()),
            clip_track_item: Some(ClipTrackItem {
                version: Some("8".into()),
                is_muted: (!nest.enabled).then(|| "true".to_owned()),
                component_owner: Some(ComponentOwner::video(placement.components)),
                track_item: Some(TrackItemRange {
                    version: Some("4".into()),
                    _node: None,
                    _item_type: None,
                    _media_type: None,
                    _track_index: None,
                    _track_ref_count: None,
                    start: (nest.start_ticks != 0).then(|| nest.start_ticks.to_string()),
                    end: nest.end_ticks.to_string(),
                }),
                sub_clip: Some(Reference::object(placement.subclip)),
                head_transition: None,
                tail_transition: None,
                original_sub_clip_time_offset: None,
            }),
            pixel_aspect_ratio: Some(records::PIXEL_ASPECT_RATIO.into()),
            tone_map_settings: Some(tone_map),
            frame_rect: Some(format!("0,0,{},{}", outer.width, outer.height)),
        }),
    ];
    output.extend(video::component_records(nest.into(), placement)?);
    Ok(output)
}
