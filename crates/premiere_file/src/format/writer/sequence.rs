use super::{
    graph::SequenceGraphIds,
    tracks::{audio, video},
};
use crate::format::{invalid, Result};
use crate::schema::{native::*, records, AudioChannels, FrameRate, PrSequence};

pub(super) fn sequence_clip<S>(source: ObjectId<S>, clip_id: String) -> Clip {
    Clip {
        version: Some(records::CLIP_VERSION.to_owned()),
        node: Node {
            version: records::NODE_VERSION,
            properties: ClipProperties::Media(MediaClipProperties {
                version: records::PROPERTIES_VERSION,
                default_is_drop_frame: None,
                label_color: "19005",
                label_name: records::SEQUENCE_CLIP_LABEL_NAME,
            }),
            id: None,
        }
        .into(),
        marker_owner: None,
        time_remapping: None,
        playback_speed: None,
        play_backwards: None,
        source: Some(Reference::object(source)),
        out_point: None,
        in_point: None,
        clip_id: Some(clip_id),
        in_use: Some(records::IN_USE.into()),
    }
}

fn sequence_source(audio: bool, ids: &SequenceGraphIds, end: i64) -> Record {
    let source = SequenceSourceBody {
        version: "4",
        content: Content::versioned(records::CONTENT_VERSION.to_owned()),
        sequence: ids.sequence.sequence.into(),
    };
    if audio {
        Record::AudioSequenceSource(AudioSequenceSource {
            object_id: ids.sequence.audio_source,
            class_id: records::AUDIO_SEQUENCE_SOURCE.class_id,
            version: records::AUDIO_SEQUENCE_SOURCE.version,
            source,
            original_duration: end,
        })
    } else {
        Record::VideoSequenceSource(VideoSequenceSource {
            object_id: ids.sequence.video_source,
            class_id: records::VIDEO_SEQUENCE_SOURCE.class_id,
            version: records::VIDEO_SEQUENCE_SOURCE.version,
            source,
            original_duration: end,
        })
    }
}

/// `MZ.Sequence.VideoTimeDisplayFormat` of a sequence rate, as Premiere saves it in
/// the Adobe-saved corpus projects of `tests/manifest.json`:
///
/// | Rate | Code | Timecode | Corpus sequences |
/// | --- | --- | --- | --- |
/// | 23.976 | 110 | 23.976 fps | `copy_and_paste_effects` "MASTER SEQUENCE", `transition_countdown` |
/// | 24 | 100 | 24 fps | all 12 sequences of `visualizer_slideshow` |
/// | 25 | 101 | 25 fps | `cinemagraph` "Spanish Steps", `travel_days` "Travel Days - Begin" |
/// | 29.97 | 102 | drop-frame | 101 sequences in 13 projects, e.g. `phone_title`, `stills_and_panorama` |
/// | 30 | 104 | 30 fps | 71 in `corporate_slideshow`, one each in `adjust_the_anchor_point` and `explore_bezier_keyframes` |
/// | 59.94 | 106 | drop-frame | `color` "making_espresso_01" |
///
/// Five 29.97 fps sequences store 103 (non-drop-frame): "Color Matte" in
/// `credits`, `credits_lower_third`, `food_lower_third` and `lower_third`, and
/// "Nested Sequence 01" in `food_lower_third`. No corpus sequence runs at 50 or
/// 60 fps, so those rates have no code and reject.
fn video_time_display_format(frame_rate: FrameRate) -> Result<&'static str> {
    Ok(match frame_rate {
        FrameRate::Fps24000Over1001 => "110",
        FrameRate::Fps24 => "100",
        FrameRate::Fps25 => "101",
        FrameRate::Fps30000Over1001 => "102",
        FrameRate::Fps30 => "104",
        FrameRate::Fps60000Over1001 => "106",
        FrameRate::Fps50 | FrameRate::Fps60 => {
            return Err(invalid(format!(
                "writer has no Premiere-authored time display format for {frame_rate} sequences"
            )))
        }
    })
}

fn sequence_record(spec: &PrSequence, ids: &SequenceGraphIds) -> Result<Record> {
    // The work area carries the timeline end, which can outlast the last item as
    // in Adobe-saved projects (cinemagraph). AME's `exportSequence` rendered no
    // probed project past its last item, whatever its work area.
    let work_out = spec.end_ticks();
    Ok(Record::Sequence(Sequence {
        object_uid: Some(ids.sequence.sequence.as_native_string()),
        class_id: Some(records::SEQUENCE.class_id.into()),
        version: Some(records::SEQUENCE.version.into()),
        node: Node {
            version: records::NODE_VERSION,
            properties: SequenceProperties {
                version: records::PROPERTIES_VERSION,
                current_solo: records::AMM_CURRENT_SOLO,
                time_per_pixel: records::TL_SQ_TIME_PER_PIXEL,
                monitor_zoom_in: "0",
                monitor_zoom_out: "0",
                header_width: "180",
                visible_base_time: "0",
                video_visible_base: "0",
                audio_visible_base: "0",
                data_visible_base: "0",
                hide_shy_tracks: "0",
                av_divider_position: records::TL_SQAV_DIVIDER_POSITION,
                work_in_point: "0",
                work_out_point: work_out,
                edit_line: "0",
                video_time_display_format: video_time_display_format(spec.frame_rate)?,
                audio_time_display_format: "200",
                editing_mode_guid: records::MZ_SEQUENCE_EDITING_MODE_GUID,
                preview_use_max_bit_depth: "false",
                preview_use_max_render_quality: "false",
                preview_rendering_preset_path: records::MZ_SEQUENCE_PREVIEW_RENDERING_PRESET_PATH,
                preview_rendering_preset_codec: records::MZ_SEQUENCE_PREVIEW_RENDERING_PRESET_CODEC,
                preview_rendering_class_id: records::MZ_SEQUENCE_PREVIEW_RENDERING_CLASS_ID,
                preview_width: spec.width,
                preview_height: spec.height,
            },
            id: None,
        }
        .into(),
        persistent_group_container: PersistentGroupContainer {
            version: "1",
            link_container: LinkContainer {
                version: "1",
                links: links(ids),
            },
        }
        .into(),
        track_groups: Some(TrackGroups {
            version: Some("1".into()),
            groups: vec![
                TrackGroupLink::video(ids.sequence.video_group),
                TrackGroupLink::audio(ids.sequence.audio_group),
                TrackGroupLink::data(ids.sequence.data_group),
            ],
        }),
        local_id: None,
        name: Some(spec.name.clone()),
        preview_format_identifier: records::PREVIEW_FORMAT_IDENTIFIER.into(),
    }))
}

/// The link of each nest's video and audio item, as Premiere lists them.
fn links(ids: &SequenceGraphIds) -> Option<Links> {
    let links: Vec<_> = ids
        .nests
        .iter()
        .flatten()
        .filter_map(|nest| nest.sound.as_ref().map(|sound| sound.link))
        .collect();
    (!links.is_empty()).then(|| Links {
        version: "1",
        links: IndexedRef::list(links),
    })
}

pub(super) fn records(spec: &PrSequence, ids: &SequenceGraphIds) -> Result<Vec<Record>> {
    let sequence_end = spec.end_ticks();
    let mut output = vec![
        Record::ClipProjectItem(ClipProjectItem {
            object_uid: ids.sequence.item,
            class_id: records::CLIP_PROJECT_ITEM.class_id,
            version: records::CLIP_PROJECT_ITEM.version,
            project_item: ProjectItem {
                version: records::PROJECT_ITEM_VERSION,
                node: Node {
                    version: records::NODE_VERSION,
                    properties: ClipProjectItemProperties {
                        version: records::PROPERTIES_VERSION,
                        icon_view_grid_order: Some("0"),
                        label: records::SEQUENCE_ITEM_LABEL_NAME,
                    },
                    id: None,
                },
                name: spec.name.clone(),
            },
            master_clip: ids.sequence.master.into(),
        }),
        Record::MasterClip(MasterClip {
            object_uid: Some(ids.sequence.master.as_native_string()),
            class_id: Some(records::MASTER_CLIP.class_id.into()),
            version: Some(records::MASTER_CLIP.version.into()),
            node: None,
            logging_info: Some(Ref::from(ids.sequence.logging).into()),
            audio_component_chains: Some(AudioComponentChains::single(
                ids.sequence.audio_component_chain,
            )),
            clips: Some(Clips::from_ids(
                Some(ids.sequence.audio_clip),
                Some(ids.sequence.video_clip),
            )),
            audio_clip_channel_groups: Some(Ref::from(ids.sequence.channel_groups).into()),
            name: Some(spec.name.clone().into()),
            is_adjustment_layer: None,
            change_version: Some("3".to_owned().into()),
        }),
        Record::ClipLoggingInfo(ClipLoggingInfo {
            object_id: ids.sequence.logging,
            class_id: records::CLIP_LOGGING_INFO.class_id,
            version: records::CLIP_LOGGING_INFO.version,
            capture_mode: None,
            clip_name: None,
            timecode_format: None,
            media_in_point: None,
            media_out_point: None,
            media_frame_rate: None,
        }),
        audio::default_chain(ids.sequence.audio_component_chain, AudioChannels::Stereo),
        Record::AudioClip(AudioClip {
            object_id: ids.sequence.audio_clip,
            class_id: Some(records::AUDIO_CLIP.class_id.into()),
            version: Some(records::AUDIO_CLIP.version.into()),
            clip: sequence_clip(
                ids.sequence.audio_source,
                ids.sequence.audio_clip_uid.clone(),
            ),
            secondary_contents: SecondaryContents::from_ids(ids.sequence.secondary_content),
            audio_channel_layout: records::STEREO.into(),
            gain: None,
        }),
        Record::VideoClip(VideoClip {
            object_id: Some(ids.sequence.video_clip.as_native_string()),
            class_id: Some(records::VIDEO_CLIP.class_id.into()),
            version: Some(records::VIDEO_CLIP.version.into()),
            clip: Some(sequence_clip(
                ids.sequence.video_source,
                ids.sequence.video_clip_uid.clone(),
            )),
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
        Record::ClipChannelGroupVectorSerializer(ClipChannelGroupVectorSerializer {
            object_id: ids.sequence.channel_groups,
            class_id: records::CLIP_CHANNEL_GROUP_VECTOR_SERIALIZER.class_id,
            version: records::CLIP_CHANNEL_GROUP_VECTOR_SERIALIZER.version,
            vectors: Some(ClipChannelVectors {
                version: "1",
                items: vec![IndexedRef::new(0, ids.sequence.channel_vector)],
            }),
        }),
        sequence_source(true, ids, sequence_end),
    ];
    for (index, id) in ids.sequence.secondary_content.iter().copied().enumerate() {
        output.push(audio::secondary_content(
            id,
            Reference::object(ids.sequence.audio_source),
            index,
        ));
    }
    output.extend([
        sequence_source(false, ids, sequence_end),
        Record::ClipChannelVectorSerializer(ClipChannelVectorSerializer {
            object_id: ids.sequence.channel_vector,
            class_id: records::CLIP_CHANNEL_VECTOR_SERIALIZER.class_id,
            version: records::CLIP_CHANNEL_VECTOR_SERIALIZER.version,
            channels: ClipChannels {
                version: "1",
                items: IndexedRef::list(ids.sequence.channels),
            },
            channel_type: records::CHANNEL_TYPE,
        }),
        sequence_record(spec, ids)?,
    ]);
    for (index, id) in ids.sequence.channels.iter().copied().enumerate() {
        output.push(Record::ClipChannelSerializer(ClipChannelSerializer {
            object_id: id,
            class_id: records::CLIP_CHANNEL_SERIALIZER.class_id,
            version: records::CLIP_CHANNEL_SERIALIZER.version,
            source_clip_index: "0",
            source_channel_index: index,
        }));
    }
    output.push(video::group_record(spec, ids));
    output.push(Record::AudioTrackGroup(AudioTrackGroup {
        object_id: ids.sequence.audio_group,
        class_id: Some(records::AUDIO_TRACK_GROUP.class_id.into()),
        version: Some(records::AUDIO_TRACK_GROUP.version.into()),
        track_group: Some(TrackGroup {
            version: Some(records::TRACK_GROUP_VERSION.into()),
            tracks: Some(Tracks::from_uids(ids.sequence.audio_tracks.iter().copied())),
            frame_rate: Some(records::AUDIO_TICKS_PER_SAMPLE.to_owned()),
            next_track_id: (ids.sequence.audio_tracks.len() + 2).into(),
        }),
        master_track: Some(Reference::object(ids.mixer.mix_track)),
        id: Some(ids.sequence.audio_group_uid.clone()),
        automation_safe_flags: Some("0".into()),
        num_adaptive_channels: Some("2".into()),
    }));
    output.push(Record::DataTrackGroup(DataTrackGroup {
        object_id: ids.sequence.data_group,
        class_id: records::DATA_TRACK_GROUP.class_id,
        version: records::DATA_TRACK_GROUP.version,
        track_group: EmptyTrackGroup {
            version: records::TRACK_GROUP_VERSION,
            frame_rate: spec.frame_rate.ticks_per_frame().to_string(),
            next_track_id: 1,
        },
    }));
    output.extend(video::track_records(spec, ids));
    output.extend(audio::records(ids));
    Ok(output)
}
