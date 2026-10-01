//! Audio tracks, the stereo mixer, and sound placements.
//!
//! Each placement gets its own track, so overlapping sounds need no mixing.
//! Every fader stays at unity: a placement stores its level in Premiere's
//! intrinsic clip Volume, as Premiere 26.5.1 writes it, with Clip Gain for the
//! part above the Volume's +15 dB range.

use super::{animation, clip_track, scalar_start_keyframe, MediaKind};
use crate::format::{
    writer::graph::{
        AudioPlacementIds, ClipVolumeIds, MediaIds, MixerComponentIds, MixerStripIds,
        SequenceGraphIds,
    },
    Result,
};
use crate::schema::{
    native::*, records, AudioChannels, PrAudioOccurrence, PrMedia, PrScalarKeyframe, PrVolumeLayout,
};

/// A chain without components: Premiere's serialization of untouched volume.
pub(in crate::format::writer) fn default_chain(
    object_id: ObjectId<AudioComponentChain>,
    channels: AudioChannels,
) -> Record {
    Record::AudioComponentChain(AudioComponentChain {
        object_id,
        class_id: Some(records::AUDIO_COMPONENT_CHAIN.class_id.into()),
        version: Some(records::AUDIO_COMPONENT_CHAIN.version.into()),
        default_volume: Some("true".into()),
        default_volume_component_id: Some("1".into()),
        default_channel_volume_component_id: Some("2".into()),
        component_chain: AudioChain {
            version: Some(records::COMPONENT_CHAIN_VERSION.into()),
            components: None,
        },
        audio_channel_layout: Some(channels.layout().into()),
        channel_type: Some(channels.channel_type().into()),
    })
}

pub(in crate::format::writer) fn secondary_content(
    object_id: ObjectId<SecondaryContent>,
    content: Reference,
    channel_index: usize,
) -> Record {
    Record::SecondaryContent(SecondaryContent {
        object_id,
        class_id: Some(records::SECONDARY_CONTENT.class_id.into()),
        version: Some(records::SECONDARY_CONTENT.version.into()),
        content,
        channel_index,
    })
}

fn audio_clip_track(
    uid: Uid<AudioClipTrack>,
    index: usize,
    strip: &MixerStripIds,
    track_uid: String,
    placement: Option<ObjectId<AudioClipTrackItem>>,
) -> Record {
    Record::AudioClipTrack(Box::new(AudioClipTrack {
        object_uid: Some(uid.as_native_string()),
        class_id: Some(records::AUDIO_CLIP_TRACK.class_id.into()),
        version: Some(records::AUDIO_CLIP_TRACK.version.into()),
        clip_track: clip_track(
            MediaKind::Audio,
            index + 2,
            index,
            IndexedRef::list(placement),
        ),
        audio_track: AudioTrack {
            version: Some(records::AUDIO_TRACK_VERSION.into()),
            component_owner: ComponentOwner::audio(strip.chain),
            panner: Reference::object(strip.panner),
            id: Some(track_uid),
            sub_type: None,
            assign: None,
            next_panner_id: Some(records::NEXT_PANNER_ID.into()),
            solo: None,
        },
    }))
}

fn audio_component(
    component_id: &str,
    kind: &str,
    params: Vec<ObjectId<AudioComponentParam>>,
) -> AudioComponent {
    AudioComponent {
        version: Some("3".into()),
        component: Component {
            version: Some("7".into()),
            params: (!params.is_empty()).then(|| Params {
                version: Some("1".into()),
                params: IndexedRef::list(params)
                    .into_iter()
                    .map(IndexedRef::into_reference)
                    .collect(),
            }),
            id: component_id.into(),
            bypass: None,
            intrinsic: None,
        },
        frame_rate: Some(records::AUDIO_TICKS_PER_SAMPLE.into()),
        audio_channel_layout: Some(records::STEREO.into()),
        channel_type: Some(records::CHANNEL_TYPE.into()),
        component_type: Some(kind.into()),
    }
}

fn strip_chain(object_id: ObjectId<AudioComponentChain>, components: &MixerComponentIds) -> Record {
    Record::AudioComponentChain(AudioComponentChain {
        object_id,
        class_id: Some(records::AUDIO_COMPONENT_CHAIN.class_id.into()),
        version: Some(records::AUDIO_COMPONENT_CHAIN.version.into()),
        default_volume: None,
        default_volume_component_id: None,
        default_channel_volume_component_id: None,
        component_chain: AudioChain {
            version: Some(records::COMPONENT_CHAIN_VERSION.into()),
            components: Some(Components::fader_meter(components.fader, components.meter)),
        },
        audio_channel_layout: Some(records::STEREO.into()),
        channel_type: Some(records::CHANNEL_TYPE.into()),
    })
}

fn pan_processor(
    object_id: ObjectId<StereoToStereoPanProcessor>,
    balance: ObjectId<AudioComponentParam>,
) -> Record {
    Record::StereoToStereoPanProcessor(StereoToStereoPanProcessor {
        object_id,
        class_id: Some(records::STEREO_TO_STEREO_PAN_PROCESSOR.class_id.into()),
        version: Some(records::STEREO_TO_STEREO_PAN_PROCESSOR.version.into()),
        pan_processor: PanProcessor {
            version: Some(records::PAN_PROCESSOR_VERSION.into()),
            audio_component: audio_component("4294967280", "0", vec![balance]),
        },
    })
}

fn scalar_param(
    object_id: ObjectId<AudioComponentParam>,
    name: Option<&str>,
    value: Option<f64>,
) -> AudioComponentParam {
    AudioComponentParam {
        object_id,
        class_id: Some(records::SCALAR_PARAM.class_id.into()),
        version: Some(records::SCALAR_PARAM.version.into()),
        start_keyframe: value.map(scalar_start_keyframe),
        current_value: value.map(|value| value.to_string()),
        keyframes: None,
        is_time_varying: None,
        name: name.map(Into::into),
        is_inverted: None,
        upper_bound: None,
        range_locked: None,
        units_string: None,
    }
}

/// A static switch of an intrinsic clip filter, off.
fn clip_switch_param(object_id: ObjectId<AudioComponentParam>, name: &str) -> Record {
    Record::AudioComponentParam(AudioComponentParam {
        class_id: Some(records::BOOL_PARAM.class_id.into()),
        is_time_varying: Some("false".into()),
        range_locked: Some(records::RANGE_LOCKED.into()),
        ..scalar_param(object_id, Some(name), None)
    })
}

/// A static level of an intrinsic clip filter. Premiere writes the value 1.0
/// (+15 dB) with neither `StartKeyframe` nor `CurrentValue`.
fn clip_level_param(
    object_id: ObjectId<AudioComponentParam>,
    name: Option<&str>,
    value: f64,
) -> AudioComponentParam {
    AudioComponentParam {
        is_time_varying: Some("false".into()),
        units_string: Some(records::UNITS_STRING.into()),
        ..scalar_param(object_id, name, (value != 1.0).then_some(value))
    }
}

fn clip_filter(
    object_id: ObjectId<AudioFilterComponent>,
    component_id: &str,
    match_name: &str,
    channels: AudioChannels,
    params: Vec<ObjectId<AudioComponentParam>>,
) -> Record {
    let mut audio_component = audio_component(component_id, "0", params);
    audio_component.component.intrinsic = Some("true".into());
    audio_component.audio_channel_layout = Some(channels.layout().into());
    audio_component.channel_type = Some(channels.channel_type().into());
    Record::AudioFilterComponent(AudioFilterComponent {
        object_id,
        class_id: Some(records::AUDIO_FILTER_COMPONENT.class_id.into()),
        version: Some(records::AUDIO_FILTER_COMPONENT.version.into()),
        audio_component,
        filter_preset: Some("0".into()),
        channel_config_data: Some(channels.filter_channel_config().into()),
        filter_match_name: match_name.into(),
        filter_index: Some("-1".into()),
    })
}

/// The clip Volume chain of one placement, and the Clip Gain that multiplies
/// its `Level`. A static Level holds at most 1.0 (+15 dB); Clip Gain takes the
/// rest of a louder volume. A keyed volume whose loudest key or static value is
/// above 0 dB moves that peak into Clip Gain, so every written Level is at or
/// below 0 dB, where Premiere's fader curve scales with the gain: the curve
/// between keys then stays the one imported from Levels at or below 0 dB.
/// `PrAudioOccurrence::validate` has already limited the keys to Linear and
/// Hold.
fn clip_volume_records(
    occurrence: &PrAudioOccurrence,
    chain: ObjectId<AudioComponentChain>,
    channels: AudioChannels,
    ids: &ClipVolumeIds,
) -> Result<(Option<f64>, Vec<Record>)> {
    let unity = PrVolumeLayout::Current.unity();
    let mix_gain = channels.centered_stereo_gain();
    let native_volume = occurrence.volume.as_f64() / mix_gain;
    let keys: Vec<PrScalarKeyframe> = occurrence
        .volume_keys
        .iter()
        .flat_map(|keys| {
            keys.keys.iter().map(|key| PrScalarKeyframe {
                value: key.value * keys.gain / mix_gain,
                ..key.clone()
            })
        })
        .collect();
    let peak = keys
        .iter()
        .map(|key| key.value)
        .fold(native_volume, f64::max);
    let clip_gain = if keys.is_empty() {
        (peak * unity > 1.0).then_some(peak * unity)
    } else {
        (peak > 1.0).then_some(peak)
    };
    let level = |gain: f64| gain * unity / clip_gain.unwrap_or(1.0);
    let level_keys: Vec<_> = keys
        .into_iter()
        .map(|key| PrScalarKeyframe {
            value: level(key.value),
            ..key
        })
        .collect();
    let static_level = level(native_volume);
    let level_param = if level_keys.is_empty() {
        clip_level_param(ids.level, Some(records::LEVEL_NAME), static_level)
    } else {
        AudioComponentParam {
            keyframes: Some(animation::scalar_keyframes(&level_keys)?),
            is_time_varying: None,
            ..clip_level_param(ids.level, Some(records::LEVEL_NAME), static_level)
        }
    };
    let components = std::iter::once(ids.component)
        .chain(ids.channel_volume.as_ref().map(|channel| channel.component));
    let mut output = vec![
        Record::AudioComponentChain(AudioComponentChain {
            object_id: chain,
            class_id: Some(records::AUDIO_COMPONENT_CHAIN.class_id.into()),
            version: Some(records::AUDIO_COMPONENT_CHAIN.version.into()),
            default_volume: None,
            default_volume_component_id: None,
            default_channel_volume_component_id: None,
            component_chain: AudioChain {
                version: Some(records::COMPONENT_CHAIN_VERSION.into()),
                components: Some(Components::clip_volume(components)),
            },
            audio_channel_layout: Some(channels.layout().into()),
            channel_type: Some(channels.channel_type().into()),
        }),
        clip_filter(
            ids.component,
            "1",
            channels.volume_match_name(),
            channels,
            vec![ids.mute, ids.level],
        ),
        clip_switch_param(ids.mute, records::MUTE_NAME),
        Record::AudioComponentParam(level_param),
    ];
    if let Some(channel) = &ids.channel_volume {
        output.push(clip_filter(
            channel.component,
            "2",
            records::CHANNEL_VOLUME_MATCH_NAME,
            channels,
            std::iter::once(channel.bypass)
                .chain(channel.levels.iter().copied())
                .collect(),
        ));
        output.push(clip_switch_param(channel.bypass, records::BYPASS_NAME));
        output.extend(channel.levels.iter().enumerate().map(|(index, id)| {
            Record::AudioComponentParam(clip_level_param(
                *id,
                records::CHANNEL_VOLUME_NAMES.get(index).copied(),
                unity,
            ))
        }));
    }
    Ok((clip_gain, output))
}

pub(in crate::format::writer) fn records(ids: &SequenceGraphIds) -> Vec<Record> {
    let mut output = Vec::new();
    let items = ids.audio_items();
    for (index, strip) in ids.mixer.strips.iter().enumerate() {
        output.push(audio_clip_track(
            ids.sequence.audio_tracks[index],
            index,
            strip,
            ids.sequence.audio_track_uids[index].clone(),
            items.get(index).copied(),
        ));
    }

    output.push(Record::AudioMixTrack(AudioMixTrack {
        object_id: ids.mixer.mix_track,
        class_id: Some(records::AUDIO_MIX_TRACK.class_id.into()),
        version: Some(records::AUDIO_MIX_TRACK.version.into()),
        audio_track: AudioTrack {
            version: Some(records::AUDIO_TRACK_VERSION.into()),
            component_owner: ComponentOwner::audio(ids.mixer.master.chain),
            panner: Reference::object(ids.mixer.master.panner),
            id: Some(ids.sequence.mix_track_uid.clone()),
            sub_type: Some("3".into()),
            assign: Some("0".into()),
            next_panner_id: Some(records::NEXT_PANNER_ID.into()),
            solo: None,
        },
        track: Track {
            version: Some(records::TRACK_VERSION.to_owned()),
            node: Node {
                version: records::NODE_VERSION,
                properties: MixTrackProperties {
                    version: records::PROPERTIES_VERSION,
                    expanded: records::TL_SQ_TRACK_EXPANDED,
                    expanded_height: records::TL_SQ_TRACK_EXPANDED_HEIGHT,
                },
                id: None,
            }
            .into(),
            id: Some("1".to_owned()),
            media_type: Some(records::AUDIO_MEDIA.to_owned()),
            index: Some("0".to_owned()),
            is_muted: None,
            _name: None,
        },
        inlet: Reference::object(ids.mixer.inlet),
    }));

    for strip in &ids.mixer.strips {
        output.push(strip_chain(strip.chain, &strip.components));
        output.push(pan_processor(strip.panner, strip.balance));
    }
    output.push(strip_chain(
        ids.mixer.master.chain,
        &ids.mixer.master.components,
    ));
    output.push(Record::DefaultPanProcessor(DefaultPanProcessor {
        object_id: ids.mixer.master.panner,
        class_id: records::DEFAULT_PAN_PROCESSOR.class_id,
        version: records::DEFAULT_PAN_PROCESSOR.version,
        pan_processor: PanProcessor {
            version: Some(records::PAN_PROCESSOR_VERSION.into()),
            audio_component: audio_component("4294967280", "0", Vec::new()),
        },
        input_channel_type: "1",
        output_channel_type: "1",
    }));
    output.push(Record::AudioTrackInlet(AudioTrackInlet {
        object_id: ids.mixer.inlet,
        class_id: records::AUDIO_TRACK_INLET.class_id,
        version: records::AUDIO_TRACK_INLET.version,
        sources: Sources {
            version: "1",
            sources: IndexedURef::list(ids.sequence.audio_tracks.iter().copied()),
        },
        audio_channel_layout: records::STEREO,
    }));

    for (components, balance) in ids
        .mixer
        .strips
        .iter()
        .map(|strip| (&strip.components, Some(strip.balance)))
        .chain(std::iter::once((&ids.mixer.master.components, None)))
    {
        output.push(Record::AudioFader(AudioFader {
            object_id: components.fader,
            class_id: Some(records::AUDIO_FADER.class_id.into()),
            version: Some(records::AUDIO_FADER.version.into()),
            audio_component: audio_component("1", "1", vec![components.volume, components.mute]),
        }));
        output.push(Record::AudioMeter(AudioMeter {
            object_id: components.meter,
            class_id: records::AUDIO_METER.class_id,
            version: records::AUDIO_METER.version,
            audio_component: audio_component("2", "2", Vec::new()),
        }));
        if let Some(balance) = balance {
            output.push(Record::AudioComponentParam(AudioComponentParam {
                is_inverted: Some("true".into()),
                ..scalar_param(balance, Some(records::BALANCE_NAME), Some(0.5))
            }));
        }
    }
    for params in ids
        .mixer
        .strips
        .iter()
        .map(|strip| &strip.components)
        .chain(std::iter::once(&ids.mixer.master.components))
    {
        output.push(Record::AudioComponentParam(AudioComponentParam {
            upper_bound: Some(records::UPPER_BOUND.into()),
            range_locked: Some(records::RANGE_LOCKED.into()),
            units_string: Some(records::UNITS_STRING.into()),
            ..scalar_param(params.volume, Some(records::VOLUME_NAME), None)
        }));
        output.push(Record::AudioComponentParam(AudioComponentParam {
            class_id: Some(records::BOOL_PARAM.class_id.into()),
            range_locked: Some(records::RANGE_LOCKED.into()),
            ..scalar_param(params.mute, Some(records::MUTE_NAME), None)
        }));
    }
    output
}

pub(in crate::format::writer) fn placement_records(
    occurrence: &PrAudioOccurrence,
    media: &PrMedia,
    ids: &MediaIds,
    placement: &AudioPlacementIds,
) -> Result<Vec<Record>> {
    let channels = media
        .audio
        .as_ref()
        .expect("sound placement media has an audio stream")
        .channels;
    let source = ids.audio.as_ref().expect("allocated source audio IDs");
    let (clip_gain, chain) = match &placement.volume {
        Some(volume) => clip_volume_records(occurrence, placement.components, channels, volume)?,
        None => (None, vec![default_chain(placement.components, channels)]),
    };
    let mut output = vec![super::super::media::audio_clip(
        placement.clip,
        ids,
        Some(occurrence.in_ticks..occurrence.out_ticks),
        &placement.secondary,
        channels,
        clip_gain,
    )];
    output.extend(chain);
    output.extend([
        Record::SubClip(SubClip {
            object_id: placement.subclip,
            class_id: Some(records::SUB_CLIP.class_id.to_owned()),
            version: Some(records::SUB_CLIP.version.to_owned()),
            clip: Reference::object(placement.clip),
            master_clip: Some(Reference::uid(ids.master)),
            name: Some(media.name.clone()),
            original_channel_group: Some("0".to_owned()),
        }),
        Record::AudioClipTrackItem(AudioClipTrackItem {
            object_id: placement.track_item,
            class_id: Some(records::AUDIO_CLIP_TRACK_ITEM.class_id.into()),
            version: Some(records::AUDIO_CLIP_TRACK_ITEM.version.into()),
            clip_track_item: ClipTrackItem {
                version: Some("8".into()),
                component_owner: Some(ComponentOwner::audio(placement.components)),
                track_item: Some(TrackItemRange {
                    version: Some("4".into()),
                    _node: None,
                    _item_type: None,
                    _media_type: None,
                    _track_index: None,
                    _track_ref_count: None,
                    start: (occurrence.start_ticks != 0)
                        .then(|| occurrence.start_ticks.to_string()),
                    end: occurrence.end_ticks.to_string(),
                }),
                sub_clip: Some(Reference::object(placement.subclip)),
                head_transition: None,
                tail_transition: None,
                is_muted: None,
                original_sub_clip_time_offset: None,
            },
        }),
    ]);
    output.extend(
        placement
            .secondary
            .iter()
            .enumerate()
            .map(|(index, id)| secondary_content(*id, Reference::object(source.source), index)),
    );
    Ok(output)
}
