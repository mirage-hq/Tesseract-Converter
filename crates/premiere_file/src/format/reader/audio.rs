//! Audio-track and sound-placement parsing.
//!
//! Static source, clip, track, and master levels and mutes fold into each
//! placement, with the clip's Clip Gain and intrinsic Volume, whose Level may be
//! keyed. Other automation, pan, solo, inserts, transitions, and routing are
//! reported as omissions.

use super::{
    animation, integer, nested::NestSound, require_zero_subclip_time_offset, required,
    required_integer, video,
};
use crate::error::{ensure, unsupported, BuildError, Result};
use crate::format::{graph, Graph, Located, Record};
use crate::schema::{
    native::{
        AudioClip, AudioClipTrack, AudioClipTrackItem, AudioComponentChain, AudioComponentParam,
        AudioFader, AudioFilterComponent, AudioMediaSource, AudioMixTrack, AudioStream,
        AudioTrackGroup, MasterClip, Reference, SecondaryContent, StereoToStereoPanProcessor,
        SubClip, Track,
    },
    records, AudioChannels, MediaId, PrAudioOccurrence, PrAudioStream, PrKeyframeEasing, PrMedia,
    PrScalarKeyframe, PrVolumeKeys, PrVolumeLayout, TICKS,
};
use crate::{omit, Omission, OmissionScope};
use fx_schema::LinearGain;
use std::collections::BTreeMap;

pub(super) fn read_stream(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
) -> Result<PrAudioStream> {
    let stream = graph.follow::<AudioStream>(reference, from)?;
    let ticks_per_sample = integer(
        &stream.value.frame_rate,
        &format!("{}: invalid FrameRate", stream.identity),
    )?;
    // Check syntax before support, so an unsupported rate cannot hide corrupt
    // duration or channel-layout data from the picture's media read.
    let intrinsic_ticks = integer(
        &stream.value.duration,
        &format!("{}: invalid Duration", stream.identity),
    )?;
    let channels = AudioChannels::parse(&stream.value.audio_channel_layout)?;
    ensure!(
        ticks_per_sample > 0 && TICKS % ticks_per_sample == 0,
        "{}: unsupported sample rate",
        stream.identity
    );
    Ok(PrAudioStream {
        intrinsic_ticks,
        channels,
        sample_rate: u32::try_from(TICKS / ticks_per_sample)
            .map_err(|_| unsupported("audio sample rate overflows"))?,
    })
}

/// A parameter's static value: the `StartKeyframe` value, else `CurrentValue`, else `default`.
fn static_value(param: &Located<AudioComponentParam>, name: &str, default: f64) -> Result<f64> {
    let text = match (&param.value.start_keyframe, &param.value.current_value) {
        (Some(key), _) => key.split(',').nth(1),
        (None, Some(value)) => Some(value.as_str()),
        (None, None) => return Ok(default),
    };
    text.and_then(|text| text.parse().ok())
        .filter(|value: &f64| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| unsupported(format!("{}: invalid {name} value", param.identity)))
}

/// A parameter's static value; keys are reported and the static value is used.
fn static_parameter(
    param: &Located<AudioComponentParam>,
    name: &str,
    default: f64,
    omissions: &mut Vec<Omission>,
) -> Result<f64> {
    if param.value.keyframes.is_some() {
        omit(
            omissions,
            OmissionScope::Feature,
            &param.identity,
            format!("{name} automation not converted; static value used"),
        );
    }
    static_value(param, name, default)
}

/// A named parameter's static value; see [`static_parameter`].
fn parameter(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    name: &str,
    default: f64,
    omissions: &mut Vec<Omission>,
) -> Result<f64> {
    let param = graph.follow::<AudioComponentParam>(reference, from)?;
    ensure!(
        param.value.name.as_deref() == Some(name),
        "{}: expected {name} parameter",
        param.identity
    );
    static_parameter(&param, name, default, omissions)
}

/// Where a chain sits. Only a placement's own chain holds Premiere's intrinsic
/// clip Volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChainOwner {
    Placement,
    Other,
}

/// The static gain of one processing chain, and the clip Volume Level it holds.
#[derive(Debug)]
struct ChainGain {
    /// Every stage except the clip Level.
    gain: f64,
    /// The clip Level's static gain, which is its value before any keys.
    level: f64,
    /// The clip Level's gains keyed on the source clock; empty when static.
    keys: Vec<PrScalarKeyframe>,
    /// Whether the chain or its clip Volume was not read in full, which was
    /// reported: what could not be read plays at unity, and a keyed switch
    /// at its static value.
    unread: bool,
}

impl ChainGain {
    const UNITY: Self = Self {
        gain: 1.0,
        level: 1.0,
        keys: Vec::new(),
        unread: false,
    };
}

/// Reads one processing chain. A chain without components is unity.
fn chain_gain(
    graph: &Graph<'_>,
    reference: Option<&Reference>,
    from: &str,
    owner: ChainOwner,
    omissions: &mut Vec<Omission>,
) -> Result<ChainGain> {
    let mut result = ChainGain::UNITY;
    let Some(reference) = reference else {
        return Ok(result);
    };
    let chain = graph.follow::<AudioComponentChain>(reference, from)?;
    let mut has_volume = false;
    for component in chain
        .value
        .component_chain
        .components
        .iter()
        .flat_map(|components| &components.items)
    {
        let record = graph.locate(component, &chain.identity)?;
        match record.tag() {
            "AudioFader" => {
                let fader = graph.decode::<AudioFader>(record)?;
                let params = required(
                    fader.value.audio_component.component.params.as_ref(),
                    &fader.identity,
                    "Params",
                )?;
                let [volume, mute] = params.params.as_slice() else {
                    return Err(unsupported(format!(
                        "{}: expected Volume and Mute parameters",
                        fader.identity
                    )));
                };
                result.gain *= parameter(
                    graph,
                    volume,
                    &fader.identity,
                    records::VOLUME_NAME,
                    1.0,
                    omissions,
                )?;
                if parameter(
                    graph,
                    mute,
                    &fader.identity,
                    records::MUTE_NAME,
                    0.0,
                    omissions,
                )? != 0.0
                {
                    result.gain = 0.0;
                }
            }
            "AudioMeter" => {}
            "AudioFilterComponent" if owner == ChainOwner::Placement => {
                match clip_filter(graph, record, omissions) {
                    Some(ClipFilter::Volume(volume)) => {
                        ensure!(!has_volume, "{}: duplicate clip Volume", chain.identity);
                        has_volume = true;
                        let Some(volume) = volume else {
                            result.unread = true;
                            continue;
                        };
                        if volume.muted {
                            result.gain = 0.0;
                        }
                        result.level = volume.level;
                        result.keys = volume.keys;
                        result.unread |= volume.keyed_switch;
                    }
                    Some(ClipFilter::ChannelVolume) => {}
                    None => omit(
                        omissions,
                        OmissionScope::Feature,
                        record.identity(),
                        "AudioFilterComponent audio processing not converted",
                    ),
                }
            }
            other => omit(
                omissions,
                OmissionScope::Feature,
                record.identity(),
                format!("{other} audio processing not converted"),
            ),
        }
    }
    Ok(result)
}

/// A chain that fails to read is reported and treated as unity.
fn chain_or_unity(
    graph: &Graph<'_>,
    reference: Option<&Reference>,
    from: &str,
    owner: ChainOwner,
    omissions: &mut Vec<Omission>,
) -> ChainGain {
    chain_gain(graph, reference, from, owner, omissions).unwrap_or_else(|error| {
        omit(
            omissions,
            OmissionScope::Feature,
            from,
            format!("audio level not converted: {error}"),
        );
        ChainGain {
            unread: true,
            ..ChainGain::UNITY
        }
    })
}

/// The static gain of a source, track, or master chain; see [`chain_or_unity`].
fn gain_or_unity(
    graph: &Graph<'_>,
    reference: Option<&Reference>,
    from: &str,
    omissions: &mut Vec<Omission>,
) -> f64 {
    chain_or_unity(graph, reference, from, ChainOwner::Other, omissions).gain
}

/// Premiere's intrinsic clip filters, which follow the placement's source clock.
enum ClipFilter {
    /// The clip Volume, or `None` when it could not be read.
    Volume(Option<ClipVolume>),
    /// A Channel Volume; one that is not at unity has been reported.
    ChannelVolume,
}

/// The clip Volume: Level gains relative to 0 dB, and the Mute switch.
struct ClipVolume {
    level: f64,
    keys: Vec<PrScalarKeyframe>,
    muted: bool,
    /// Whether the switch is keyed. Its keys were reported, and `muted` is
    /// its static value.
    keyed_switch: bool,
}

/// Reads a clip Volume or Channel Volume in either project layout. Returns
/// `None` for any other filter. A Volume or Channel Volume that cannot be
/// converted is reported and plays at unity. The filter is identified before
/// it is decoded, so a Volume whose parameters cannot be read is still the
/// clip Volume.
fn clip_filter(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<Omission>,
) -> Option<ClipFilter> {
    let element = record.element();
    let intrinsic = element
        .child("AudioComponent")
        .and_then(|audio| audio.child("Component"))
        .and_then(|component| component.child("Intrinsic"))
        .and_then(graph::Element::text);
    if intrinsic != Some("true") {
        return None;
    }
    let match_name = element
        .child("FilterMatchName")
        .and_then(graph::Element::text)?;
    if [AudioChannels::Mono, AudioChannels::Stereo]
        .iter()
        .any(|channels| channels.volume_match_name() == match_name)
    {
        return Some(ClipFilter::Volume(
            clip_volume(graph, record, omissions)
                .inspect_err(|error| {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        record.identity(),
                        format!("clip Volume not converted: {error}"),
                    );
                })
                .ok(),
        ));
    }
    if match_name != records::CHANNEL_VOLUME_MATCH_NAME {
        return None;
    }
    let filter = graph.decode::<AudioFilterComponent>(record).ok()?;
    let params = filter_params(graph, &filter).ok()?;
    let names: Vec<_> = params
        .iter()
        .map(|param| param.value.name.as_deref())
        .collect();
    let layout = match names.as_slice() {
        [Some(records::BYPASS_NAME), Some(left), Some(right), extra @ ..]
            if [*left, *right] == records::CHANNEL_VOLUME_NAMES
                && extra.len() == records::CHANNEL_VOLUME_EXTRA_PARAMS
                && extra.iter().all(Option::is_none) =>
        {
            PrVolumeLayout::Current
        }
        [Some(records::BYPASS_NAME), Some(left), Some(right)]
            if [*left, *right] == records::LEGACY_CHANNEL_VOLUME_NAMES =>
        {
            PrVolumeLayout::Legacy
        }
        _ => return None,
    };
    // Bypass has no effect while every channel plays at unity.
    let unity = params[1..].iter().all(|param| {
        param.value.keyframes.is_none()
            && param.value.is_time_varying.as_deref() != Some("true")
            && static_value(param, "channel volume", 1.0)
                .is_ok_and(|value| (value - layout.unity()).abs() <= 1e-9)
    });
    if !unity {
        omit(
            omissions,
            OmissionScope::Feature,
            &filter.identity,
            "clip Channel Volume not converted",
        );
    }
    Some(ClipFilter::ChannelVolume)
}

/// A filter's parameters, in order.
fn filter_params(
    graph: &Graph<'_>,
    filter: &Located<AudioFilterComponent>,
) -> crate::format::Result<Vec<Located<AudioComponentParam>>> {
    filter
        .value
        .audio_component
        .component
        .params
        .iter()
        .flat_map(|params| &params.params)
        .map(|reference| graph.follow::<AudioComponentParam>(reference, &filter.identity))
        .collect()
}

/// Reads the clip Volume of `record`. Its Mute (or, in the legacy layout,
/// Bypass) switch plays its static value; the Level may be keyed.
fn clip_volume(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<ClipVolume> {
    let filter = graph.decode::<AudioFilterComponent>(record)?;
    let params = filter_params(graph, &filter)?;
    let [switch, level] = params.as_slice() else {
        return Err(unsupported("expected two clip Volume parameters"));
    };
    let layout = match (switch.value.name.as_deref(), level.value.name.as_deref()) {
        (Some(records::MUTE_NAME), Some(records::LEVEL_NAME)) => PrVolumeLayout::Current,
        (Some(records::BYPASS_NAME), Some(records::LEVEL_NAME)) => PrVolumeLayout::Legacy,
        _ => return Err(unsupported("unknown clip Volume parameters")),
    };
    ensure!(
        filter
            .value
            .audio_component
            .component
            .bypass
            .as_deref()
            .is_none_or(|bypass| bypass == "false"),
        "bypassed Volume effect"
    );
    let switch_name = switch.value.name.as_deref().unwrap_or_default();
    let on = static_parameter(switch, switch_name, 0.0, omissions)? != 0.0;
    ensure!(
        !(on && layout == PrVolumeLayout::Legacy),
        "{}: bypassed Level",
        switch.identity
    );
    let gain = |value: f64| value / layout.unity();
    let static_level = gain(static_value(level, records::LEVEL_NAME, 1.0)?);
    let is_time_varying = level.value.is_time_varying.as_deref();
    ensure!(
        matches!(is_time_varying, None | Some("true") | Some("false")),
        "{}: invalid IsTimeVarying",
        level.identity
    );
    let wire = level.value.keyframes.as_deref().unwrap_or_default();
    // As for Motion: keys need IsTimeVarying absent or true, a static Level absent or false.
    ensure!(
        is_time_varying != Some(if wire.is_empty() { "true" } else { "false" }),
        "{}: Level keyframes and IsTimeVarying disagree",
        level.identity
    );
    let keys = animation::scalar_keys(wire, &level.identity)?
        .into_iter()
        .map(|key| {
            ensure!(
                !matches!(key.easing, PrKeyframeEasing::CubicBezier { .. }),
                "{}: Bezier Volume keyframes are not converted",
                level.identity
            );
            if key.value < 0.0 {
                return Err(unsupported(format!(
                    "{}: negative Level keyframe",
                    level.identity
                )));
            }
            Ok(PrScalarKeyframe {
                value: gain(key.value),
                ..key
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ClipVolume {
        level: static_level,
        keys,
        muted: on,
        keyed_switch: switch.value.keyframes.is_some(),
    })
}

/// Reports the existing pan omissions and establishes only the measured
/// static centered stereo panner; a static fallback for automation is not proof.
fn report_pan(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    omissions: &mut Vec<Omission>,
) -> bool {
    let balance = graph
        .follow::<StereoToStereoPanProcessor>(reference, from)
        .map_err(BuildError::from)
        .and_then(|panner| {
            let audio = &panner.value.pan_processor.audio_component;
            let params = required(audio.component.params.as_ref(), &panner.identity, "Params")?;
            let [balance] = params.params.as_slice() else {
                return Err(unsupported(format!(
                    "{}: expected one Balance parameter",
                    panner.identity
                )));
            };
            let param = graph.follow::<AudioComponentParam>(balance, &panner.identity)?;
            ensure!(
                param.value.name.as_deref() == Some(records::BALANCE_NAME),
                "{}: expected Balance parameter",
                param.identity
            );
            let value = static_parameter(&param, records::BALANCE_NAME, 0.5, omissions)?;
            let verified = audio.channel_type.as_deref() == Some("1")
                && audio.audio_channel_layout.as_deref().is_some_and(|layout| {
                    AudioChannels::parse(layout).ok() == Some(AudioChannels::Stereo)
                })
                && param.value.keyframes.is_none()
                && param
                    .value
                    .is_time_varying
                    .as_deref()
                    .is_none_or(|value| value == "false");
            Ok((value, verified))
        });
    if !matches!(balance, Ok((0.5, _))) {
        omit(
            omissions,
            OmissionScope::Feature,
            from,
            "audio pan not converted",
        );
    }
    matches!(balance, Ok((0.5, true)))
}

/// The unchanged stereo master/inlet form in the native gain reference.
/// Keep the inlet record so each track must also be among its actual sources.
fn centered_stereo_master<'g>(
    graph: &'g Graph<'_>,
    master: &Located<AudioMixTrack>,
) -> Option<Record<'g>> {
    let panner = graph
        .locate(&master.value.audio_track.panner, &master.identity)
        .ok()?;
    if panner.tag() != records::DEFAULT_PAN_PROCESSOR.tag
        || master.value.audio_track.sub_type.as_deref() != Some("3")
        || master.value.audio_track.assign.as_deref() != Some("0")
    {
        return None;
    }
    let root = panner.element();
    let text = |tag| root.child(tag).and_then(graph::Element::text);
    if text("DefaultPannerInputChannelType") != Some("1")
        || text("DefaultPannerOutputChannelType") != Some("1")
    {
        return None;
    }
    let audio = root.child("PanProcessor")?.child("AudioComponent")?;
    if audio.child("ChannelType").and_then(graph::Element::text) != Some("1")
        || audio
            .child("AudioChannelLayout")
            .and_then(graph::Element::text)
            .and_then(|layout| AudioChannels::parse(layout).ok())
            != Some(AudioChannels::Stereo)
        || audio
            .child("Component")?
            .child("Params")
            .is_some_and(|params| params.children().next().is_some())
    {
        return None;
    }
    let inlet = graph.locate(&master.value.inlet, &master.identity).ok()?;
    if inlet.tag() != records::AUDIO_TRACK_INLET.tag
        || inlet
            .element()
            .child("AudioChannelLayout")
            .and_then(graph::Element::text)
            .and_then(|layout| AudioChannels::parse(layout).ok())
            != Some(AudioChannels::Stereo)
    {
        return None;
    }
    Some(inlet)
}

/// The timeline mute of an audio track; the same field is a video track's output toggle.
fn is_muted<N>(track: &Track<N>) -> bool {
    track.is_muted.as_deref() == Some("true")
}

/// Reads the sound of one audio track group: its sound placements, and the
/// audio items of nested sequences, which the sequence pairs with their nests
/// or plays alone once its video tracks are read (`nested::pair_sounds`,
/// `nested::play_alone`).
pub(super) fn read_tracks(
    graph: &Graph<'_>,
    group: Located<AudioTrackGroup>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    _nesting: &mut super::nested::Nesting<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<(Vec<PrAudioOccurrence>, Vec<NestSound>)> {
    let track_group = required(group.value.track_group, &group.identity, "TrackGroup")?;
    let mut stereo_master = None;
    let master_gain = match &group.value.master_track {
        Some(reference) => match graph.follow::<AudioMixTrack>(reference, &group.identity) {
            Ok(master) => {
                stereo_master = centered_stereo_master(graph, &master);
                if is_muted(&master.value.track) {
                    0.0
                } else {
                    gain_or_unity(
                        graph,
                        master.value.audio_track.component_owner.components.as_ref(),
                        &master.identity,
                        omissions,
                    )
                }
            }
            Err(error) => {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    &group.identity,
                    format!("master audio level not converted: {error}"),
                );
                1.0
            }
        },
        None => 1.0,
    };
    let mut occurrences = Vec::new();
    let mut sounds = Vec::new();
    for reference in track_group.tracks.iter().flat_map(|tracks| &tracks.tracks) {
        let record = match graph.locate(reference, &group.identity) {
            Ok(record) => record,
            Err(error) => {
                omit(
                    omissions,
                    OmissionScope::Track,
                    &group.identity,
                    error.to_string(),
                );
                continue;
            }
        };
        let track = match graph.decode_as::<AudioClipTrack>(record, &group.identity) {
            Ok(track) => track,
            Err(error) => {
                omit(
                    omissions,
                    OmissionScope::Track,
                    record.identity(),
                    error.to_string(),
                );
                continue;
            }
        };
        if track
            .value
            .clip_track
            .transition_items
            .as_ref()
            .and_then(|transitions| transitions.track_items.as_ref())
            .is_some_and(|transitions| !transitions.items.is_empty())
        {
            omit(
                omissions,
                OmissionScope::Feature,
                &track.identity,
                "audio transitions not converted",
            );
        }
        let items = track
            .value
            .clip_track
            .clip_items
            .and_then(|items| items.track_items)
            .map_or_else(Vec::new, |items| items.items);
        if items.is_empty() {
            continue;
        }
        let centered_pan = report_pan(
            graph,
            &track.value.audio_track.panner,
            &track.identity,
            omissions,
        );
        let centered_stereo_route = centered_pan
            && track.value.audio_track.assign.is_none()
            && stereo_master.is_some_and(|inlet| {
                inlet.element().child("Sources").is_some_and(|sources| {
                    track.value.object_uid.as_deref().is_some_and(|uid| {
                        sources.children().any(|source| {
                            source.tag() == "Source" && source.attribute("ObjectURef") == Some(uid)
                        })
                    })
                })
            });
        if track.value.audio_track.solo.as_deref() == Some("1") {
            omit(
                omissions,
                OmissionScope::Feature,
                &track.identity,
                "audio solo not converted",
            );
        }
        let track_gain = if track.value.clip_track.track.as_ref().is_some_and(is_muted) {
            0.0
        } else {
            master_gain
                * gain_or_unity(
                    graph,
                    track.value.audio_track.component_owner.components.as_ref(),
                    &track.identity,
                    omissions,
                )
        };
        for item in items {
            let identity = item
                .id
                .as_deref()
                .or(item.uid.as_deref())
                .unwrap_or("unidentified occurrence")
                .to_owned();
            match read_occurrence(
                graph,
                &item,
                &track.identity,
                track_gain,
                centered_stereo_route,
                media,
                omissions,
            ) {
                Ok(AudioItem::Media(clip)) => occurrences.push(clip),
                Ok(AudioItem::Nest(sound)) => sounds.push(sound),
                Err(error) => omit(
                    omissions,
                    OmissionScope::Occurrence,
                    identity,
                    error.to_string(),
                ),
            }
        }
    }
    occurrences.sort_by_key(|clip| clip.start_ticks);
    Ok((occurrences, sounds))
}

/// What one audio track item plays.
enum AudioItem {
    Media(PrAudioOccurrence),
    /// The mix of a nested sequence.
    Nest(NestSound),
}

/// Each clip channel must play the same channel of `source`, in order.
fn ensure_channel_order(
    graph: &Graph<'_>,
    clip: &Located<AudioClip>,
    channels: AudioChannels,
    source: Record<'_>,
) -> Result<()> {
    ensure!(
        clip.value.secondary_contents.items.len() == channels.count(),
        "{}: channel mapping does not match the clip layout",
        clip.identity
    );
    for (index, reference) in clip.value.secondary_contents.items.iter().enumerate() {
        let secondary = graph.follow::<SecondaryContent>(reference, &clip.identity)?;
        ensure!(
            secondary.value.channel_index == index
                && graph.locate(&secondary.value.content, &secondary.identity)? == source,
            "{}: channel remapping is unsupported",
            clip.identity
        );
    }
    Ok(())
}

/// The source-level gain, which lives on the master clip of `sub`.
fn source_gain(
    graph: &Graph<'_>,
    sub: &Located<SubClip>,
    omissions: &mut Vec<Omission>,
) -> Result<f64> {
    let Some(master_reference) = &sub.value.master_clip else {
        return Ok(1.0);
    };
    let master = graph.follow::<MasterClip>(master_reference, &sub.identity)?;
    let chains = master
        .value
        .audio_component_chains
        .as_ref()
        .map_or(&[][..], |chains| chains.chains.as_slice());
    ensure!(
        chains.len() <= 1,
        "{}: multiple source channel groups are unsupported",
        master.identity
    );
    Ok(gain_or_unity(
        graph,
        chains.first(),
        &master.identity,
        omissions,
    ))
}

fn read_occurrence(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    track_gain: f64,
    centered_stereo_route: bool,
    media: &mut BTreeMap<MediaId, PrMedia>,
    omissions: &mut Vec<Omission>,
) -> Result<AudioItem> {
    let item = graph.follow::<AudioClipTrackItem>(reference, from)?;
    let body = item.value.clip_track_item;
    require_zero_subclip_time_offset(&body, &item.identity)?;
    let range = required(body.track_item.as_ref(), &item.identity, "TrackItem")?;
    let start = match range.start.as_deref() {
        Some(value) => integer(value, &format!("{}: invalid Start", item.identity))?,
        None => 0,
    };
    let end = integer(&range.end, &format!("{}: invalid End", item.identity))?;
    let clip_chain = chain_or_unity(
        graph,
        body.component_owner
            .as_ref()
            .and_then(|owner| owner.components.as_ref()),
        &item.identity,
        ChainOwner::Placement,
        omissions,
    );
    let mut gain = track_gain * clip_chain.gain;
    // A placement's clip Enable; a nest's sound compares it with its video item's.
    let muted = body.is_muted.as_deref() == Some("true");

    let sub_reference = required(
        body.sub_clip.as_ref(),
        &item.identity,
        records::SUB_CLIP.tag,
    )?;
    let sub = graph.follow::<SubClip>(sub_reference, &item.identity)?;
    let clip = graph.follow::<AudioClip>(&sub.value.clip, &sub.identity)?;
    let native_clip = &clip.value.clip;
    ensure!(
        video::playback_rate(native_clip, &clip.identity)? == 1.0
            && native_clip.time_remapping.is_none(),
        "{}: only unit, forward audio playback is supported",
        clip.identity
    );
    video::report_markers(graph, native_clip, &clip.identity, omissions);
    let source_in = required_integer(native_clip.in_point.as_deref(), &clip.identity, "InPoint")?;
    let source_out =
        required_integer(native_clip.out_point.as_deref(), &clip.identity, "OutPoint")?;
    let channels = AudioChannels::parse(&clip.value.audio_channel_layout)?;
    if let Some(text) = &clip.value.gain {
        match text.parse::<f64>() {
            Ok(clip_gain) if clip_gain.is_finite() && clip_gain >= 0.0 => gain *= clip_gain,
            _ => omit(
                omissions,
                OmissionScope::Feature,
                &clip.identity,
                "invalid clip Gain not converted",
            ),
        }
    }
    let source_reference = required(native_clip.source.as_ref(), &clip.identity, "Source")?;
    let source_record = graph.locate(source_reference, &clip.identity)?;
    if source_record.tag() == records::AUDIO_SEQUENCE_SOURCE.tag {
        // The stereo mix of the placed sequence, as Premiere 26.5.1 saves a
        // nest's audio item (`premiere_isolated_images_nests_26_5`, G6).
        ensure!(
            channels == AudioChannels::Stereo,
            "{}: only a stereo audio item of a nested sequence is supported",
            clip.identity
        );
        ensure_channel_order(graph, &clip, channels, source_record)?;
        gain *= source_gain(graph, &sub, omissions)?;
        // Its Volume scales every inner sound: a Volume that could not be read
        // would play them at unity, and a keyed Mute at its static value.
        ensure!(
            !clip_chain.unread,
            "{}: the audio item of a nested sequence with an unreadable Volume or a keyed Mute is not converted",
            item.identity
        );
        ensure!(
            start >= 0 && end > start && source_in >= 0,
            "{}: the audio item of a nested sequence has invalid timeline/source ranges",
            item.identity
        );
        // It plays its sequence from In at normal speed until End. Premiere
        // 26.5.1 saved an item shortened at its tail with its untrimmed Out
        // (`premiere_isolated_nest_audio_outer_keys_26_5`: End 7 s, In 1.5 s,
        // Out 8.5 s), and AME stops the sound at End. An item longer than
        // Out - In would play past its Out.
        let played_out = source_in
            .checked_add(end - start)
            .filter(|&played_out| played_out <= source_out)
            .ok_or_else(|| {
                unsupported(format!(
                    "{}: the audio item of a nested sequence must play its sequence at normal speed",
                    item.identity
                ))
            })?;
        let placement = graph.locate(reference, from)?;
        let sequence = graph::nested_sequence(graph, placement)?
            .ok_or_else(|| unsupported(format!("{}: missing nested sequence", item.identity)))?;
        return Ok(AudioItem::Nest(NestSound {
            id: item.identity,
            sequence,
            timeline: start..end,
            source: source_in..played_out,
            // Every sound that the item plays shares its gain, so a gain
            // that overflows omits the item.
            gain: LinearGain::new(gain * clip_chain.level)
                .map_err(|_| unsupported("audio gain overflows"))?
                .as_f64(),
            volume_keys: (!clip_chain.keys.is_empty()).then_some(PrVolumeKeys {
                keys: clip_chain.keys,
                gain,
            }),
            enabled: !muted,
        }));
    }
    let source = graph.decode_as::<AudioMediaSource>(source_record, &clip.identity)?;
    ensure_channel_order(graph, &clip, channels, source_record)?;
    gain *= source_gain(graph, &sub, omissions)?;
    if muted {
        gain = 0.0;
    }

    let media_source = required(
        source.value.media_source.as_ref(),
        &source.identity,
        "MediaSource",
    )?;
    if let Some(content) = &media_source.content {
        content.require_unbounded(&source.identity)?;
    }
    let media_reference = required(media_source.media.as_ref(), &source.identity, "Media")?;
    let media_id = video::read_source_media(
        graph,
        media_reference,
        &source.identity,
        media,
        false,
        omissions,
    )?;
    // The composition's video items import its picture without sound, so
    // this item's sound has no other owner and is reported, not doubled.
    ensure!(
        media[&media_id].after_effects_composition().is_none(),
        "{}: {}",
        source.identity,
        crate::schema::after_effects::LINKED_AUDIO_REASON
    );
    let stream = media[&media_id]
        .audio
        .as_ref()
        .ok_or_else(|| unsupported(format!("{}: source has no audio stream", source.identity)))?;
    ensure!(
        stream.channels == channels,
        "{}: clip channel layout differs from the source",
        clip.identity
    );
    // Only direct media reaches this point. A nested sequence is already a
    // stereo mix; its inner mono placements establish their own route once.
    if channels == AudioChannels::Mono {
        if centered_stereo_route {
            gain *= channels.centered_stereo_gain();
        } else {
            omit(omissions, OmissionScope::Feature, &item.identity,
                "mono centered-stereo gain not normalized: static centered pan and default stereo master routing are not established");
        }
    }
    let occurrence = PrAudioOccurrence {
        id: Some(item.identity),
        media: media_id,
        start_ticks: start,
        end_ticks: end,
        in_ticks: source_in,
        out_ticks: source_out,
        volume: LinearGain::new(gain * clip_chain.level)
            .map_err(|_| unsupported("audio gain overflows"))?,
        volume_keys: (!clip_chain.keys.is_empty()).then_some(PrVolumeKeys {
            keys: clip_chain.keys,
            gain,
        }),
    };
    occurrence.validate(stream)?;
    Ok(AudioItem::Media(occurrence))
}
