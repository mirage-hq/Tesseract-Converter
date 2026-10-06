use super::{
    adjustment, color_matte,
    graph::MediaIds,
    still,
    tracks::audio::{default_chain, secondary_content},
    BoundMedia,
};
use crate::schema::{
    native::*, records, AudioChannels, ColorSpace, PrAudioOccurrence, PrAudioStream, PrMedia,
    PrMediaKind, PrVideoOccurrence, VideoCodec, TICKS,
};
use base64::{engine::general_purpose::STANDARD, Engine};

fn clip<S>(
    source: ObjectId<S>,
    ids: &MediaIds,
    range: Option<std::ops::Range<i64>>,
    clip_id: String,
) -> Clip {
    Clip {
        version: Some(records::CLIP_VERSION.to_owned()),
        node: Node {
            version: records::NODE_VERSION,
            properties: ClipProperties::Media(MediaClipProperties {
                version: records::PROPERTIES_VERSION,
                default_is_drop_frame: None,
                label_color: records::MEDIA_CLIP_LABEL_COLOR,
                label_name: records::MEDIA_CLIP_LABEL_NAME,
            }),
            id: None,
        }
        .into(),
        marker_owner: Some(MarkerOwner {
            version: Some("1".into()),
            markers: Some(Reference::object(ids.markers)),
        }),
        time_remapping: None,
        maintain_audio_pitch: None,
        playback_speed: None,
        is_multicam: None,
        selected_track_index: None,
        play_backwards: None,
        source: Some(Reference::object(source)),
        out_point: range.as_ref().map(|range| range.end.to_string()),
        in_point: range.as_ref().map(|range| range.start.to_string()),
        clip_id: Some(clip_id),
        in_use: range.is_none().then_some(records::IN_USE.into()),
    }
}

/// The source template clip (`occurrence` absent) or one placed video clip.
pub(super) fn media_clip(
    media: &PrMedia,
    occurrence: Option<&PrVideoOccurrence>,
    ids: &MediaIds,
    clip_id: String,
) -> Clip {
    let mut clip = Clip {
        playback_speed: occurrence
            .filter(|clip| clip.playback_rate.abs() != 1.0)
            .map(|clip| clip.playback_rate.abs().to_string()),
        play_backwards: occurrence
            .is_some_and(|clip| clip.playback_rate.is_sign_negative())
            .then(|| "true".to_owned()),
        ..clip(
            ids.source,
            ids,
            occurrence.map(PrVideoOccurrence::source_ticks),
            clip_id,
        )
    };
    // Adobe generator clips carry a drop-frame preference and no markers.
    if media.is_generator() {
        clip.marker_owner = None;
        if let RetainedOrSkipped::Retained(node) = &mut clip.node {
            if let ClipProperties::Media(properties) = &mut node.properties {
                properties.default_is_drop_frame = Some(color_matte::DEFAULT_IS_DROP_FRAME);
            }
        }
    }
    clip
}

/// The source template clip (`occurrence` absent) or one placed audio clip, with
/// its linear Clip Gain when that is not unity.
pub(super) fn audio_clip(
    object_id: ObjectId<AudioClip>,
    ids: &MediaIds,
    occurrence: Option<&PrAudioOccurrence>,
    secondary: &[ObjectId<SecondaryContent>],
    channels: AudioChannels,
    gain: Option<f64>,
) -> Record {
    let source = ids.audio.as_ref().expect("allocated source audio IDs");
    Record::AudioClip(AudioClip {
        object_id,
        class_id: Some(records::AUDIO_CLIP.class_id.into()),
        version: Some(records::AUDIO_CLIP.version.into()),
        clip: Clip {
            maintain_audio_pitch: occurrence
                .filter(|sound| sound.preserve_audio_pitch)
                .map(|_| "true".to_owned()),
            playback_speed: occurrence
                .filter(|sound| sound.playback_rate.abs() != 1.0)
                .map(|sound| sound.playback_rate.abs().to_string()),
            play_backwards: occurrence
                .filter(|sound| sound.playback_rate < 0.0)
                .map(|_| "true".to_owned()),
            ..clip(
                source.source,
                ids,
                occurrence.map(|sound| sound.in_ticks..sound.out_ticks),
                super::graph::uuid(),
            )
        },
        secondary_contents: SecondaryContents::from_ids(secondary.iter().copied()),
        audio_channel_layout: channels.layout().into(),
        gain: gain.map(|gain| gain.to_string()),
        audio_time_scaler_settings: occurrence
            .filter(|sound| sound.preserve_audio_pitch)
            .map(|_| AUDIO_PITCH_ON_SCALER_SETTINGS.to_owned()),
    })
}

fn utf16_base64(value: &str) -> String {
    STANDARD.encode(
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

fn media_source(media: Uid<Media>) -> MediaSource {
    MediaSource {
        version: Some("4".to_owned()),
        content: Some(Content::versioned(records::CONTENT_VERSION.to_owned())),
        media: Some(Reference::uid(media)),
    }
}

/// The picture source record `source` of `media`, whose picture lasts
/// `original_duration_ticks`.
pub(super) fn video_media_source(
    source: ObjectId<VideoMediaSource>,
    media: Uid<Media>,
    original_duration_ticks: i64,
) -> Record {
    Record::VideoMediaSource(VideoMediaSource {
        object_id: Some(source.as_native_string()),
        object_uid: None,
        class_id: Some(records::VIDEO_MEDIA_SOURCE.class_id.to_owned()),
        version: Some(records::VIDEO_MEDIA_SOURCE.version.to_owned()),
        media_source: Some(media_source(media)),
        original_duration: Some(original_duration_ticks.to_string()),
    })
}

fn audio_records(audio: &PrAudioStream, ids: &MediaIds) -> Vec<Record> {
    let source = ids.audio.as_ref().expect("allocated source audio IDs");
    let mut output = vec![
        Record::AudioStream(AudioStream {
            object_id: source.stream,
            class_id: Some(records::AUDIO_STREAM.class_id.into()),
            version: Some(records::AUDIO_STREAM.version.into()),
            audio_channel_layout: audio.channels.layout().into(),
            duration: audio.intrinsic_ticks.to_string(),
            frame_rate: (TICKS / i64::from(audio.sample_rate)).to_string(),
            sample_type: Some("3".into()),
        }),
        Record::AudioMediaSource(AudioMediaSource {
            object_id: source.source,
            class_id: Some(records::AUDIO_MEDIA_SOURCE.class_id.into()),
            version: Some(records::AUDIO_MEDIA_SOURCE.version.into()),
            media_source: Some(media_source(ids.media)),
            original_duration: Some(audio.intrinsic_ticks.to_string()),
        }),
        audio_clip(
            source.template_clip,
            ids,
            None,
            &source.secondary,
            audio.channels,
            None,
        ),
        default_chain(source.components, audio.channels),
        Record::ClipChannelVectorSerializer(ClipChannelVectorSerializer {
            object_id: source.channel_vector,
            class_id: records::CLIP_CHANNEL_VECTOR_SERIALIZER.class_id,
            version: records::CLIP_CHANNEL_VECTOR_SERIALIZER.version,
            channels: ClipChannels {
                version: "1",
                items: IndexedRef::list(source.channels.iter().copied()),
            },
            channel_type: audio.channels.channel_type(),
        }),
    ];
    output.extend(
        source
            .secondary
            .iter()
            .enumerate()
            .map(|(index, id)| secondary_content(*id, Reference::object(source.source), index)),
    );
    output.extend(source.channels.iter().enumerate().map(|(index, id)| {
        Record::ClipChannelSerializer(ClipChannelSerializer {
            object_id: *id,
            class_id: records::CLIP_CHANNEL_SERIALIZER.class_id,
            version: records::CLIP_CHANNEL_SERIALIZER.version,
            source_clip_index: "0",
            source_channel_index: index,
        })
    }));
    output
}

pub(super) fn records(media: &BoundMedia<'_>, ids: &MediaIds) -> Vec<Record> {
    let spec = media.media();
    let video = spec.video.as_ref();
    let mut records: Vec<_> = [video.map(|video| {
        Record::VideoStream(match video.kind {
            PrMediaKind::Still { alpha } | PrMediaKind::NumberedStills { alpha } => {
                still::video_stream(video, alpha, ids)
            }
            PrMediaKind::AfterEffectsComposition(_) => {
                super::after_effects::video_stream(video, ids)
            }
            // Every corpus matte stream has the still fields and no alpha.
            PrMediaKind::ColorMatte(_) => still::video_stream(video, false, ids),
            // Every corpus Black Video stream also carries `IsContinuousTime`.
            PrMediaKind::Adjustment => VideoStream {
                is_continuous_time: Some("true".to_owned()),
                ..still::video_stream(video, false, ids)
            },
            PrMediaKind::Video { codec, hdr_profile } => VideoStream {
                is_numbered_stills: None,
                is_still: None,
                is_continuous_time: None,
                alpha_info_is_uncertain: None,
                is_overriden_image_orientation_type: None,
                object_id: ids.stream,
                class_id: Some(records::VIDEO_STREAM.class_id.to_owned()),
                version: Some(records::VIDEO_STREAM.version.to_owned()),
                frame_rate: Some(video.frame_rate.ticks_per_frame().to_string()),
                is_frame_rate_overridden: None,
                overidden_frame_rate: None,
                duration: Some(video.intrinsic_ticks.to_string()),
                // An alpha master (ProRes 4444 with a 32-bit entry) keeps its
                // alpha readable, as the corpus `ap4h` masters are saved
                // without `IgnoreAlpha` and with straight `AlphaType`.
                ignore_alpha: (!codec.is_some_and(VideoCodec::has_alpha))
                    .then(|| "true".to_owned()),
                frame_rect: Some(format!("0,0,{},{}", video.width, video.height)),
                pixel_aspect_ratio: None,
                original_par: None,
                is_par_overridden: Some("true".to_owned()),
                overridden_par: Some(video.pixel_aspect.native()),
                codec_type: codec.map(|codec| codec.codec_type().to_owned()),
                original_color_space: Some(
                    serde_json::to_string(&match hdr_profile {
                        Some(profile) => ColorSpace::source_hdr(profile),
                        None => ColorSpace::source_sdr(),
                    })
                    .expect("native source color fields serialize"),
                ),
                alpha_type: Some(
                    if codec.is_some_and(VideoCodec::has_alpha) {
                        records::VIDEO_STRAIGHT_ALPHA_TYPE
                    } else {
                        records::VIDEO_NO_ALPHA_TYPE
                    }
                    .to_owned(),
                ),
                field_type_is_uncertain: Some("true".to_owned()),
                original_field_type: None,
                original_image_orientation_type: Some(video.orientation.native().to_owned()),
            },
        })
    })]
    .into_iter()
    .flatten()
    .collect();
    let template_clip = match media {
        BoundMedia::File {
            media,
            relative_path,
            absolute_path,
        } => {
            records.extend(file_source_records(
                media,
                relative_path,
                absolute_path,
                ids,
            ));
            media_clip(media, None, ids, ids.template_clip_uid.clone())
        }
        BoundMedia::ColorMatte { media, matte } => {
            records.extend(color_matte::source_records(media, *matte, ids));
            color_matte::template_clip(media, ids)
        }
        // Adobe gives every generator's template clip the same explicit range.
        BoundMedia::Adjustment { media } => {
            records.extend(adjustment::source_records(media, ids));
            color_matte::template_clip(media, ids)
        }
    };
    records.extend(project_item_records(spec, ids, template_clip));
    if let Some(audio) = &spec.audio {
        records.extend(audio_records(audio, ids));
    }
    records
}

/// The media, source and markers records of one file on disk; only file clips
/// own a `MarkerOwner`.
fn file_source_records(
    spec: &PrMedia,
    relative_path: &str,
    absolute_path: &str,
    ids: &MediaIds,
) -> Vec<Record> {
    let video = spec.video.as_ref();
    [
        Some(Record::Media(Media {
            object_id: None,
            object_uid: Some(ids.media.as_native_string()),
            class_id: Some(records::MEDIA.class_id.to_owned()),
            version: Some(records::MEDIA.version.to_owned()),
            video_stream: video.map(|_| Reference::object(ids.stream)),
            importer_prefs: spec
                .after_effects_composition()
                .map(|composition| ImporterPrefs {
                    encoding: records::ENCODING.to_owned(),
                    binary_hash: super::graph::uuid(),
                    value: utf16_base64(&composition.dynamic_link_guid()),
                }),
            modification_state: Some(ModificationState {
                encoding: records::ENCODING.to_owned(),
                binary_hash: ids.media_binary_hash.clone(),
                value: if spec.after_effects_composition().is_some() {
                    // Premiere stores this state as UUID bytes, unlike the
                    // UTF-16 composition GUID in ImporterPrefs.
                    STANDARD.encode(ids.media_state.as_bytes())
                } else {
                    utf16_base64(&ids.media_state.to_string())
                },
            }),
            relative_paths: vec![relative_path.to_owned()],
            file_path: Some(absolute_path.to_owned()),
            infinite: spec.is_still().then(|| "true".to_owned()),
            implementation_id: Some(
                if spec.after_effects_composition().is_some() {
                    crate::schema::after_effects::IMPORTER_ID
                } else {
                    records::MEDIA_IMPLEMENTATION_ID
                }
                .to_owned(),
            ),
            title: Some(spec.name.clone()),
            file_key: Some(ids.media_file_key.clone()),
            content_and_metadata_state: Some(ids.media_state.to_string()),
            actual_media_file_path: Some(absolute_path.to_owned()),
            conformed_audio_rate: None,
            audio_stream: ids
                .audio
                .as_ref()
                .map(|audio| Reference::object(audio.stream)),
        })),
        video.map(|video| video_media_source(ids.source, ids.media, video.intrinsic_ticks)),
        Some(Record::Markers(Markers {
            object_id: ids.markers,
            class_id: Some(records::MARKERS.class_id.to_owned()),
            version: Some(records::MARKERS.version.to_owned()),
            by_guid: Some(records::BY_GUID.to_owned()),
            last_metadata_state: Some(records::ZERO_GUID.to_owned()),
            last_content_state: Some(ids.media_state.to_string()),
        })),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Logging, the template clip, channel groups, the master clip and the project
/// item shared by file-backed and generator media.
fn project_item_records(spec: &PrMedia, ids: &MediaIds, template_clip: Clip) -> Vec<Record> {
    let video = spec.video.as_ref();
    // Logging follows the picture; sound-only media has no frame rate.
    let (intrinsic_ticks, frame_rate_ticks) = match (video, &spec.audio) {
        (Some(video), _) => (
            video.intrinsic_ticks,
            Some(video.frame_rate.ticks_per_frame()),
        ),
        (None, Some(audio)) => (audio.intrinsic_ticks, None),
        (None, None) => unreachable!("validated media has a stream"),
    };
    [
        Some(Record::ClipLoggingInfo(ClipLoggingInfo {
            object_id: ids.logging,
            class_id: records::CLIP_LOGGING_INFO.class_id,
            version: records::CLIP_LOGGING_INFO.version,
            capture_mode: Some("2"),
            clip_name: Some(spec.name.clone()),
            timecode_format: Some("104"),
            media_in_point: (!spec.is_still()).then_some("0"),
            media_out_point: (!spec.is_still()).then_some(intrinsic_ticks),
            media_frame_rate: frame_rate_ticks,
        })),
        video.map(|_| {
            Record::VideoClip(VideoClip {
                object_id: Some(ids.template_clip.as_native_string()),
                class_id: Some(records::VIDEO_CLIP.class_id.into()),
                version: Some(records::VIDEO_CLIP.version.into()),
                clip: Some(template_clip),
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
            })
        }),
        Some(Record::ClipChannelGroupVectorSerializer(
            ClipChannelGroupVectorSerializer {
                object_id: ids.channels,
                class_id: records::CLIP_CHANNEL_GROUP_VECTOR_SERIALIZER.class_id,
                version: records::CLIP_CHANNEL_GROUP_VECTOR_SERIALIZER.version,
                vectors: ids.audio.as_ref().map(|audio| ClipChannelVectors {
                    version: "1",
                    items: vec![IndexedRef::new(0, audio.channel_vector)],
                }),
            },
        )),
        Some(Record::MasterClip(MasterClip {
            object_uid: Some(ids.master.as_native_string()),
            class_id: Some(records::MASTER_CLIP.class_id.into()),
            version: Some(records::MASTER_CLIP.version.into()),
            node: None,
            logging_info: Some(Ref::from(ids.logging).into()),
            audio_component_chains: ids
                .audio
                .as_ref()
                .map(|audio| AudioComponentChains::single(audio.components)),
            video_component_chain: None,
            clips: Some(Clips::from_ids(
                ids.audio.as_ref().map(|audio| audio.template_clip),
                video.map(|_| ids.template_clip),
            )),
            audio_clip_channel_groups: Some(Ref::from(ids.channels).into()),
            name: Some(spec.name.clone().into()),
            is_adjustment_layer: spec.is_adjustment().then(|| "true".to_owned()),
            change_version: Some("0".to_owned().into()),
        })),
        Some(Record::ClipProjectItem(ClipProjectItem {
            object_uid: ids.item,
            class_id: records::CLIP_PROJECT_ITEM.class_id,
            version: records::CLIP_PROJECT_ITEM.version,
            project_item: ProjectItem {
                version: records::PROJECT_ITEM_VERSION,
                node: Node {
                    version: records::NODE_VERSION,
                    properties: ClipProjectItemProperties {
                        version: records::PROPERTIES_VERSION,
                        icon_view_grid_order: None,
                        label: records::MEDIA_ITEM_LABEL_NAME,
                    },
                    id: None,
                },
                name: spec.name.clone(),
            },
            master_clip: ids.master.into(),
        })),
    ]
    .into_iter()
    .flatten()
    .collect()
}
