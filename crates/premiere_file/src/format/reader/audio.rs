//! Audio-track and sound-placement parsing.
//!
//! Static source, clip, track, and master levels and mutes fold into each
//! placement, with the clip's Clip Gain and intrinsic Volume, whose Level may be
//! keyed, and the audio transitions at its edges. Matching stereo Amplify
//! channels (saved ducking) use the same volume path. Static Fill Right with Left
//! uses the existing full-source mono path. Other automation, pan, solo, inserts,
//! and routing are reported as omissions.

mod amplify;
mod nested_mono;

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
        AudioTrackGroup, AudioTransitionTrackItem, MasterClip, Reference, SecondaryContent,
        StereoToStereoPanProcessor, SubClip, Track,
    },
    records, AudioChannels, CustomFadeShape, FrameRate, MediaId, PrAudioFade, PrAudioOccurrence,
    PrAudioSourceChannel, PrAudioStream, PrFadeCurve, PrKeyframeEasing, PrMedia, PrScalarKeyframe,
    PrVolumeKeys, PrVolumeLayout, TICKS, TICKS_PER_MILLISECOND,
};
use crate::{approximate, omit, Omission, OmissionKind, OmissionScope};
use fx_schema::LinearGain;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Range, RangeInclusive},
};

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
        prepared_clock: None,
        intrinsic_ticks,
        channels,
        sample_rate: u32::try_from(TICKS / ticks_per_sample)
            .map_err(|_| unsupported("audio sample rate overflows"))?,
    })
}

/// The text of a parameter's static value: the `StartKeyframe` value, else
/// `CurrentValue`, else `None`. A `StartKeyframe` without a value field has
/// empty text, which no reading accepts.
fn static_text(param: &AudioComponentParam) -> Option<&str> {
    match (&param.start_keyframe, &param.current_value) {
        (Some(key), _) => Some(key.split(',').nth(1).unwrap_or_default()),
        (None, value) => value.as_deref(),
    }
}

/// A parameter's static value: the `StartKeyframe` value, else `CurrentValue`, else `default`.
fn static_value(param: &Located<AudioComponentParam>, name: &str, default: f64) -> Result<f64> {
    let Some(text) = static_text(&param.value) else {
        return Ok(default);
    };
    text.parse()
        .ok()
        .filter(|value: &f64| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| unsupported(format!("{}: invalid {name} value", param.identity)))
}

/// A switch's static state, from the text that [`static_value`] reads:
/// Premiere saves a fader Mute as `true` or `false`, and a number is on
/// unless it is zero. A switch without a value is off.
fn static_switch(param: &Located<AudioComponentParam>, name: &str) -> Result<bool> {
    match static_text(&param.value) {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Ok(static_value(param, name, 0.0)? != 0.0),
    }
}

/// Reports the keys of `param`, whose static value is used.
fn report_keys(param: &Located<AudioComponentParam>, name: &str, omissions: &mut Vec<Omission>) {
    if param.value.keyframes.is_some() {
        omit(
            omissions,
            OmissionScope::Feature,
            &param.identity,
            format!("{name} automation not converted; static value used"),
        );
    }
}

/// A parameter's static value; keys are reported and the static value is used.
fn static_parameter(
    param: &Located<AudioComponentParam>,
    name: &str,
    default: f64,
    omissions: &mut Vec<Omission>,
) -> Result<f64> {
    report_keys(param, name, omissions);
    static_value(param, name, default)
}

/// The parameter `name` that `reference` names. Its keys are reported: its
/// static value, read as a number or a switch, is used.
fn parameter(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    name: &str,
    omissions: &mut Vec<Omission>,
) -> Result<Located<AudioComponentParam>> {
    let param = graph.follow::<AudioComponentParam>(reference, from)?;
    ensure!(
        param.value.name.as_deref() == Some(name),
        "{}: expected {name} parameter",
        param.identity
    );
    report_keys(&param, name, omissions);
    Ok(param)
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
    /// Native identity of a verified clip insert that copies left to both outputs.
    fill_right: Option<String>,
}

impl ChainGain {
    const UNITY: Self = Self {
        gain: 1.0,
        level: 1.0,
        keys: Vec::new(),
        unread: false,
        fill_right: None,
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
    let mut amplify = None;
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
                let volume = parameter(
                    graph,
                    volume,
                    &fader.identity,
                    records::VOLUME_NAME,
                    omissions,
                )?;
                result.gain *= static_value(&volume, records::VOLUME_NAME, 1.0)?;
                let mute = parameter(graph, mute, &fader.identity, records::MUTE_NAME, omissions)?;
                if static_switch(&mute, records::MUTE_NAME)? {
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
                        result.unread |= volume.unread;
                    }
                    Some(ClipFilter::ChannelVolume | ClipFilter::Skipped) => {}
                    Some(ClipFilter::FillRight) => {
                        result.fill_right = Some(record.identity());
                    }
                    Some(ClipFilter::Amplify(volume)) => {
                        amplify = Some(if amplify.is_some() {
                            Err(unsupported("multiple Amplify effects are not combined"))
                        } else {
                            volume
                        });
                    }
                    None => report_filter_omission(
                        record,
                        "no editable processing equivalent",
                        omissions,
                    ),
                }
            }
            "AudioFilterComponent" => {
                insert_filter(graph, record, owner, omissions);
            }
            other => omit(
                omissions,
                OmissionScope::Feature,
                record.identity(),
                format!("{other} audio processing not converted"),
            ),
        }
    }
    if let Some(amplify) = amplify {
        if let Err(error) = amplify.and_then(|volume| apply_amplify(&mut result, volume)) {
            result.gain = 0.0;
            result.unread = true;
            omit(
                omissions,
                OmissionScope::Feature,
                &chain.identity,
                format!("Amplify not converted: {error}; the sound was kept at zero gain"),
            );
        }
    }
    Ok(result)
}

/// Combine only one changing stage; multiplying two curves needs a separate fit.
fn apply_amplify(chain: &mut ChainGain, amplify: ClipVolume) -> Result<()> {
    ensure!(
        chain.keys.is_empty() || amplify.keys.is_empty(),
        "keyed clip Volume and Amplify cannot be combined"
    );
    if amplify.keys.is_empty() {
        chain.gain *= amplify.level;
    } else {
        chain.gain *= chain.level;
        chain.level = amplify.level;
        chain.keys = amplify.keys;
    }
    Ok(())
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

/// Clip gain controls and static routing inserts. Gain keys follow the source clock.
enum ClipFilter {
    /// The clip Volume, or `None` when it could not be read.
    Volume(Option<ClipVolume>),
    /// A Channel Volume; one that is not at unity has been reported.
    ChannelVolume,
    /// Equal stereo Amplify channels, including saved Essential Sound ducking.
    Amplify(Result<ClipVolume>),
    /// A static Fill Right with Left insert on a stereo placement.
    FillRight,
    /// A bypassed insert, or one whose omission was already reported.
    Skipped,
}

/// The clip Volume: Level gains relative to 0 dB, and the Mute switch.
struct ClipVolume {
    level: f64,
    keys: Vec<PrScalarKeyframe>,
    muted: bool,
    /// Whether either control failed or the switch is keyed. Reported even
    /// when an independent control establishes silence (nests remain unread).
    unread: bool,
}

/// Identifies intrinsic gain controls before decoding them; other inserts use
/// static bypass and the measured Fill Right mapping, or report their identity.
/// Amplify failures stay explicit so a saved
/// ducking envelope cannot turn into an unattenuated sound. Unread Volume
/// plays at unity unless an independent current Mute or static Level
/// establishes silence.
fn clip_filter(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<Omission>,
) -> Option<ClipFilter> {
    let element = record.element();
    let match_name = element
        .child("FilterMatchName")
        .and_then(graph::Element::text)?;
    if match_name == amplify::MATCH_NAME {
        return Some(ClipFilter::Amplify(amplify::read(graph, record, omissions)));
    }
    let intrinsic = element
        .child("AudioComponent")
        .and_then(|audio| audio.child("Component"))
        .and_then(|component| component.child("Intrinsic"))
        .and_then(graph::Element::text);
    if intrinsic != Some("true") {
        return Some(insert_filter(
            graph,
            record,
            ChainOwner::Placement,
            omissions,
        ));
    }
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

/// Stereo input/output configuration saved by Premiere 26.x for Fill Right.
const FILL_RIGHT_STEREO_CONFIG: &str = r#"{"in":[{"layout":[100,101],"name":"Stereo In","type":0}],"out":[{"layout":[100,101],"name":"Stereo Out","type":0}]}"#;
/// Premiere 26.x saves a disconnected secondary input as u64::MAX, without Content.
const DISCONNECTED_CHANNEL_INDEX: &str = "18446744073709551615";

/// Preserve native identity even when a plugin has no audible FX equivalent.
fn report_filter_omission(
    record: Record<'_>,
    reason: impl std::fmt::Display,
    omissions: &mut Vec<Omission>,
) {
    let name = record
        .element()
        .child("FilterMatchName")
        .and_then(graph::Element::text)
        .unwrap_or("<missing FilterMatchName>");
    omit(
        omissions,
        OmissionScope::Feature,
        record.identity(),
        format!("audio filter {name:?} not converted: {reason}"),
    );
}

/// Static bypass is a no-op even for plugins whose opaque processing cannot decode.
/// Active Fill Right is admitted only on its measured stereo clip configuration.
fn insert_filter(
    graph: &Graph<'_>,
    record: Record<'_>,
    owner: ChainOwner,
    omissions: &mut Vec<Omission>,
) -> ClipFilter {
    let read = || -> Result<ClipFilter> {
        let root = record.element();
        let audio = root
            .child("AudioComponent")
            .ok_or_else(|| unsupported("missing AudioComponent"))?;
        let component = audio
            .child("Component")
            .ok_or_else(|| unsupported("missing Component"))?;
        let mut bypass_fields = component.children().filter(|child| child.tag() == "Bypass");
        let bypass_field = bypass_fields.next();
        ensure!(
            bypass_fields.next().is_none() && bypass_field.is_none_or(graph::Element::is_text_only),
            "malformed component Bypass"
        );
        let bypass = bypass_field.map(|value| value.text().unwrap_or_default());
        ensure!(
            matches!(bypass, None | Some("true" | "false")),
            "invalid component Bypass"
        );
        if bypass == Some("true") {
            return Ok(ClipFilter::Skipped);
        }
        if let Some(reference) = component
            .child("Params")
            .and_then(|params| params.children().next())
        {
            let param =
                graph.follow::<AudioComponentParam>(&reference.reference(), &record.identity())?;
            if param.value.name.as_deref() == Some(records::BYPASS_NAME) {
                ensure!(
                    param.value.keyframes.is_none()
                        && matches!(param.value.is_time_varying.as_deref(), None | Some("false")),
                    "automated or malformed Bypass"
                );
                if static_switch(&param, records::BYPASS_NAME)? {
                    return Ok(ClipFilter::Skipped);
                }
            }
        }
        ensure!(
            owner == ChainOwner::Placement
                && root.child("FilterMatchName").and_then(graph::Element::text)
                    == Some(records::FILL_RIGHT_MATCH_NAME),
            "no editable processing equivalent"
        );
        let filter = graph.decode::<AudioFilterComponent>(record)?;
        let params = filter_params(graph, &filter)?;
        ensure!(
            params.len() == 1 && params[0].value.name.as_deref() == Some(records::BYPASS_NAME),
            "unknown Fill Right parameter layout"
        );
        ensure!(
            filter
                .value
                .audio_component
                .audio_channel_layout
                .as_deref()
                .map(AudioChannels::parse)
                .transpose()?
                == Some(AudioChannels::Stereo)
                && filter.value.audio_component.channel_type.as_deref() == Some("1")
                && filter.value.channel_config_data.as_deref() == Some(FILL_RIGHT_STEREO_CONFIG),
            "unverified Fill Right routing"
        );
        Ok(ClipFilter::FillRight)
    };
    match read() {
        Ok(filter) => filter,
        Err(error) => {
            report_filter_omission(record, error, omissions);
            ClipFilter::Skipped
        }
    }
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
    let switch_value = static_parameter(switch, switch_name, 0.0, omissions);
    let level_value = clip_level(level, layout);
    let on = match switch_value {
        Ok(value) => value != 0.0,
        Err(error) => {
            // Current Mute cannot bypass Level. Only a fully read static zero
            // proves silence; a zero base below keys may become audible later.
            if layout == PrVolumeLayout::Current
                && matches!(&level_value, Ok((0.0, keys)) if keys.is_empty())
            {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record.identity(),
                    format!("clip Volume not converted: {error}"),
                );
                return Ok(ClipVolume {
                    level: 0.0,
                    keys: Vec::new(),
                    muted: false,
                    unread: true,
                });
            }
            return Err(error);
        }
    };
    ensure!(
        !(on && layout == PrVolumeLayout::Legacy),
        "{}: bypassed Level",
        switch.identity
    );
    let mut volume = ClipVolume {
        level: 1.0,
        keys: Vec::new(),
        muted: on,
        unread: switch.value.keyframes.is_some(),
    };
    match level_value {
        Ok((level, keys)) => {
            volume.level = level;
            volume.keys = keys;
        }
        Err(error) => {
            volume.unread = true;
            omit(
                omissions,
                OmissionScope::Feature,
                record.identity(),
                format!("clip Volume not converted: {error}"),
            );
        }
    }
    Ok(volume)
}

/// Read Level independently so a failure cannot discard an already known Mute.
fn clip_level(
    level: &Located<AudioComponentParam>,
    layout: PrVolumeLayout,
) -> Result<(f64, Vec<PrScalarKeyframe>)> {
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
    Ok((static_level, keys))
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
            let param = parameter(
                graph,
                balance,
                &panner.identity,
                records::BALANCE_NAME,
                omissions,
            )?;
            let value = static_value(&param, records::BALANCE_NAME, 0.5)?;
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
fn is_muted<N>(track: &Track<N>, identity: &str) -> Result<bool> {
    super::visibility::is_muted(track.is_muted.as_deref(), identity)
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
                let muted = match is_muted(&master.value.track, &master.identity) {
                    Ok(muted) => muted,
                    Err(error) => {
                        omit(
                            omissions,
                            OmissionScope::Track,
                            &master.identity,
                            error.to_string(),
                        );
                        return Ok((Vec::new(), Vec::new()));
                    }
                };
                if muted {
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
        let mut track = match graph.decode_as::<AudioClipTrack>(record, &group.identity) {
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
        let transitions = track
            .value
            .clip_track
            .transition_items
            .take()
            .and_then(|items| items.track_items)
            .map_or_else(Vec::new, |items| items.items);
        let items = track
            .value
            .clip_track
            .clip_items
            .take()
            .and_then(|items| items.track_items)
            .map_or_else(Vec::new, |items| items.items);
        if items.is_empty() {
            // No clip links these transitions: each is reported.
            read_transitions(
                graph,
                &transitions,
                &track.identity,
                &[],
                &mut [],
                media,
                omissions,
            );
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
        let muted = match track
            .value
            .clip_track
            .track
            .as_ref()
            .map_or(Ok(false), |native| is_muted(native, &track.identity))
        {
            Ok(muted) => muted,
            Err(error) => {
                omit(
                    omissions,
                    OmissionScope::Track,
                    &track.identity,
                    error.to_string(),
                );
                continue;
            }
        };
        let track_gain = if muted {
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
        let mut placements = Vec::with_capacity(items.len());
        let mut links = Vec::with_capacity(items.len());
        for item in items {
            let identity = item
                .id
                .as_deref()
                .or(item.uid.as_deref())
                .unwrap_or("unidentified occurrence")
                .to_owned();
            let mut link = transition_link(graph, &item, &track.identity);
            // Global de-duplication must not hide omitted processing when
            // another occurrence reuses the same native clip or source stage.
            let mut item_notes = Vec::new();
            let read = read_occurrence(
                graph,
                &item,
                &track.identity,
                track_gain,
                centered_stereo_route,
                media,
                &mut item_notes,
            );
            for note in item_notes {
                crate::export_loss::OmissionSink::emit(omissions, note);
            }
            let linked = match read {
                Ok(AudioItem::Media(clip)) => {
                    placements.push(clip);
                    LinkedItem::Placement(placements.len() - 1)
                }
                Ok(AudioItem::Nest(sound)) => {
                    sounds.push(sound);
                    LinkedItem::Nest
                }
                Ok(AudioItem::MonoNest(sound)) => {
                    match nested_mono::read(
                        graph,
                        &sound,
                        &track,
                        group.value.master_track.as_ref(),
                        media,
                        omissions,
                    ) {
                        Ok(clip) => {
                            placements.push(clip);
                            LinkedItem::Placement(placements.len() - 1)
                        }
                        Err(error) => {
                            omit(
                                omissions,
                                OmissionScope::Occurrence,
                                identity,
                                error.to_string(),
                            );
                            LinkedItem::Omitted
                        }
                    }
                }
                Err(error) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        identity,
                        error.to_string(),
                    );
                    LinkedItem::Omitted
                }
            };
            if let Some(link) = &mut link {
                link.item = linked;
            }
            links.extend(link);
        }
        read_transitions(
            graph,
            &transitions,
            &track.identity,
            &links,
            &mut placements,
            media,
            omissions,
        );
        occurrences.extend(placements);
    }
    occurrences.sort_by_key(|clip| clip.start_ticks);
    Ok((occurrences, sounds))
}

/// What a track item that may link a transition became.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkedItem {
    /// An omitted item: its transitions have nothing to fade on its side.
    Omitted,
    /// The converted placement of this index among its track's placements.
    Placement(usize),
    /// The audio item of a nested sequence, which carries no fade.
    Nest,
}

/// A track item's range and transition links, which its transitions are
/// checked against even when the item itself is omitted.
struct TransitionClipLink {
    start_ticks: i64,
    end_ticks: i64,
    head_transition: Option<String>,
    tail_transition: Option<String>,
    item: LinkedItem,
}

/// Reads a track item's links leniently: an item that cannot be read is
/// omitted by [`read_occurrence`] with its own error.
fn transition_link(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
) -> Option<TransitionClipLink> {
    let item = graph.follow::<AudioClipTrackItem>(reference, from).ok()?;
    let body = &item.value.clip_track_item;
    let range = body.track_item.as_ref()?;
    let transition = |reference: Option<&Reference>| {
        reference
            .and_then(|reference| graph.locate(reference, &item.identity).ok())
            .map(|record| record.identity())
    };
    Some(TransitionClipLink {
        start_ticks: match range.start.as_deref() {
            Some(value) => value.parse().ok()?,
            None => 0,
        },
        end_ticks: range.end.parse().ok()?,
        head_transition: transition(body.head_transition.as_ref()),
        tail_transition: transition(body.tail_transition.as_ref()),
        item: LinkedItem::Omitted,
    })
}

/// A checked audio transition and the converted placements it fades.
struct AudioTransition {
    id: String,
    curve: PrFadeCurve,
    start_ticks: i64,
    end_ticks: i64,
    outgoing: Option<usize>,
    incoming: Option<usize>,
}

/// Reads the transitions of one track onto its placements. A transition that
/// cannot convert is reported, and its clips keep their cuts and levels.
fn read_transitions(
    graph: &Graph<'_>,
    references: &[Reference],
    track: &str,
    links: &[TransitionClipLink],
    placements: &mut [PrAudioOccurrence],
    media: &BTreeMap<MediaId, PrMedia>,
    omissions: &mut Vec<Omission>,
) {
    let mut listed = BTreeSet::new();
    for reference in references {
        if let Ok(record) = graph.locate(reference, track) {
            listed.insert(record.identity());
        }
    }
    let mut unlisted = BTreeSet::new();
    for identity in links.iter().flat_map(|link| {
        [
            link.head_transition.as_deref(),
            link.tail_transition.as_deref(),
        ]
        .into_iter()
        .flatten()
    }) {
        if !listed.contains(identity) && unlisted.insert(identity) {
            omit(
                omissions,
                OmissionScope::Feature,
                identity,
                "clip-linked audio transition is missing from TransitionItems; transition not converted",
            );
        }
    }
    let mut seen = BTreeSet::new();
    let mut transitions = Vec::with_capacity(references.len());
    for reference in references {
        let identity = reference
            .id
            .as_deref()
            .or(reference.uid.as_deref())
            .unwrap_or("unidentified transition")
            .to_owned();
        match read_transition(graph, reference, track, links, placements, media) {
            Ok(transition) if !seen.insert(transition.id.clone()) => omit(
                omissions,
                OmissionScope::Feature,
                identity,
                "duplicate audio transition reference",
            ),
            Ok(transition) => transitions.push(transition),
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                identity,
                error.to_string(),
            ),
        }
    }
    // The two fades of one placement must not overlap; both are omitted.
    let mut overlapping = BTreeSet::new();
    for (index, placement) in placements.iter().enumerate() {
        let head = transitions.iter().find(|item| item.incoming == Some(index));
        let tail = transitions.iter().find(|item| item.outgoing == Some(index));
        if let (Some(head), Some(tail)) = (head, tail) {
            if head.end_ticks > tail.start_ticks {
                for transition in [head, tail] {
                    overlapping.insert(transition.id.clone());
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        &transition.id,
                        format!(
                            "audio fades overlap on {}; transition not converted",
                            placement.id.as_deref().unwrap_or("a placement")
                        ),
                    );
                }
            }
        }
    }
    for transition in transitions {
        if overlapping.contains(&transition.id) {
            continue;
        }
        let fade = PrAudioFade {
            curve: transition.curve,
            duration_ticks: transition.end_ticks - transition.start_ticks,
            id: Some(transition.id),
        };
        // A crossfade plays the outgoing source past its Out to the transition
        // end, and the incoming source from its In minus the Alignment.
        if let Some(index) = transition.outgoing {
            let placement = &mut placements[index];
            let handle = transition.end_ticks - placement.end_ticks;
            let source_end = placement
                .source_at(transition.end_ticks)
                .expect("transition source handle checked before applying it");
            placement.out_ticks = source_end;
            placement.end_ticks += handle;
            placement.fade_out = Some(fade.clone());
        }
        if let Some(index) = transition.incoming {
            let placement = &mut placements[index];
            let handle = placement.start_ticks - transition.start_ticks;
            let source_start = placement
                .source_at(transition.start_ticks)
                .expect("transition source handle checked before applying it");
            placement.in_ticks = source_start;
            placement.start_ticks -= handle;
            placement.fade_in = Some(fade);
        }
    }
}

/// Short fades keep their native silence edge, clip cuts and source handles.
/// Move only the full-level edge inward to the sequence grid, far enough for
/// the existing fit's keys. Confine it to the clip and the opposite fade; if
/// Level changes in the enlarged span, retain the native span and coarsen its
/// inner keys instead. Neither approximation alters the native Level curve.
pub(super) fn clamp_short_fades(
    clips: &mut [PrAudioOccurrence],
    frame_rate: FrameRate,
    omissions: &mut Vec<Omission>,
) {
    let frame = i128::from(frame_rate.ticks_per_frame());
    for clip in clips {
        for fade_in in [true, false] {
            let (fade, opposite) = if fade_in {
                (&clip.fade_in, &clip.fade_out)
            } else {
                (&clip.fade_out, &clip.fade_in)
            };
            let Some(fade) = fade else { continue };
            let edge = if fade_in {
                clip.start_ticks
            } else {
                clip.end_ticks
            };
            let minimum =
                i128::from(fade.curve.shortest_millis()) * i128::from(TICKS_PER_MILLISECOND);
            if i128::from(fade.duration_ticks) >= minimum {
                continue;
            }
            let native_duration = fade.duration_ticks;
            let inner = if fade_in {
                (i128::from(edge) + minimum + frame - 1).div_euclid(frame) * frame
            } else {
                (i128::from(edge) - minimum).div_euclid(frame) * frame
            };
            let available = clip.end_ticks
                - clip.start_ticks
                - opposite.as_ref().map_or(0, |fade| fade.duration_ticks);
            // Bounded by the validated placement, so the resulting tick span
            // fits i64 even for a sequence grid beyond that clock's endpoint.
            let duration = (inner - i128::from(edge)).abs().min(i128::from(available)) as i64;
            let timeline_span = if fade_in {
                clip.start_ticks..clip.start_ticks + duration
            } else {
                clip.end_ticks - duration..clip.end_ticks
            };
            let source_span = clip
                .source_part(&timeline_span)
                .expect("validated placement bounds contain the clamped fade source window");
            let level_holds = clip
                .volume_keys
                .as_ref()
                .is_none_or(|keys| level_holds_over_fade(&keys.keys, source_span, fade_in));
            let duration = if level_holds {
                duration
            } else {
                native_duration
            };
            approximate(
                omissions,
                fade.id.as_deref().unwrap_or("audio transition"),
                format!("short {} fade ({native_duration} ticks) approximated on the sequence frame grid: silence edge, clip cuts and source handles unchanged; full-level edge moved inward where the clip, opposite fade and held Level permit ({duration} ticks); colliding inner keys coarsened on the millisecond grid", fade.curve.match_name()),
            );
            let fade = if fade_in {
                &mut clip.fade_in
            } else {
                &mut clip.fade_out
            };
            fade.as_mut()
                .expect("clamped fade remains attached")
                .duration_ticks = duration;
        }
    }
}

/// Whether the clip Level holds one value over a fade's `span` on the source
/// clock: no Level key inside it, and the keys around it hold or keep one
/// value. The one exception is a key exactly at the fade's full-level `edge`
/// (its end for a fade-in) that holds that value over the fade, as export
/// writes where the Level changes beyond the fade.
fn level_is_constant(
    keys: &[PrScalarKeyframe],
    span: RangeInclusive<i64>,
    edge: i64,
    fade_in: bool,
) -> bool {
    let first = keys.partition_point(|key| key.source_ticks < *span.start());
    let last = keys.partition_point(|key| key.source_ticks <= *span.end());
    let previous = first.checked_sub(1).map(|index| &keys[index]);
    let next = keys.get(last);
    match &keys[first..last] {
        [] => match (previous, next) {
            (Some(previous), Some(next)) => {
                next.easing == PrKeyframeEasing::Hold || previous.value == next.value
            }
            _ => true,
        },
        [key] if key.source_ticks == edge => {
            if fade_in {
                previous.is_none_or(|previous| previous.value == key.value)
            } else {
                next.is_none_or(|next| {
                    next.easing == PrKeyframeEasing::Hold || next.value == key.value
                })
            }
        }
        _ => false,
    }
}

/// Whether the clip Level `keys` hold one value over a fade-in or fade-out
/// that plays the source range `fade`, with no key within
/// [`PrAudioFade::LEVEL_KEY_MARGIN_MILLIS`] of it but one at its full-level
/// edge ([`level_is_constant`]). The fades of a placement convert only where
/// this holds.
pub(super) fn level_holds_over_fade(
    keys: &[PrScalarKeyframe],
    fade: Range<i64>,
    fade_in: bool,
) -> bool {
    let margin = PrAudioFade::LEVEL_KEY_MARGIN_MILLIS * TICKS_PER_MILLISECOND;
    let edge = if fade_in { fade.end } else { fade.start };
    level_is_constant(
        keys,
        fade.start.saturating_sub(margin)..=fade.end.saturating_add(margin),
        edge,
        fade_in,
    )
}

/// Fails on a child element that the transition reader does not model.
fn ensure_known_children(
    element: graph::Element<'_>,
    allowed: &[&str],
    identity: &str,
) -> Result<()> {
    match element
        .children()
        .find(|child| !allowed.contains(&child.tag()))
    {
        Some(child) => Err(unsupported(format!(
            "{identity}: audio transition {} not converted",
            child.tag()
        ))),
        None => Ok(()),
    }
}

/// The curve of a transition. A record converts only when its curve-defining
/// fields equal a form that AME rendered (run A5): the fade shape pair that
/// Premiere 26.5.1 writes, or Constant Power's (1, 19) of Premiere 25 (project
/// version 43), which Premiere 26.5.1 re-saves without the pair.
fn fade_curve(transition: &Located<AudioTransitionTrackItem>) -> Result<PrFadeCurve> {
    let item = &transition.value;
    let body = &item.transition_track_item;
    let name = required(
        body.match_name.as_deref(),
        &transition.identity,
        "MatchName",
    )?;
    // Native 26.5.2 saved/reopened the four calibration controls with the
    // type absent. Do not infer explicit type 0/1 or an unknown shape family.
    if name == "Custom Fade" {
        let shape = required_integer(
            item.fade_shape_value.as_deref(),
            &transition.identity,
            "FadeShapeValue",
        )?;
        let shape = CustomFadeShape::new(shape).ok_or_else(|| {
            unsupported(format!(
                "{}: unmeasured Custom Fade shape not converted",
                transition.identity
            ))
        })?;
        ensure!(
            item.fade_shape_type.is_none(),
            "{}: unmeasured Custom Fade type not converted",
            transition.identity
        );
        ensure!(
            matches!(
                (body.has_incoming_clip.as_deref(), body.has_outgoing_clip.as_deref()),
                (Some("true"), Some("false")) | (Some("false"), Some("true"))
            ),
            "{}: Custom Fade crossfade has no accepted native render evidence; transition not converted, sound preserved",
            transition.identity
        );
        ensure!(
            item.crossfade_symmetry
                .as_deref()
                .is_none_or(|value| value == "0"),
            "{}: nondefault CrossfadeSymmetry not converted",
            transition.identity
        );
        return Ok(PrFadeCurve::Custom(shape));
    }
    let curve = PrFadeCurve::ALL
        .into_iter()
        .find(|curve| curve.match_name() == name)
        .ok_or_else(|| {
            unsupported(format!(
                "{} ({name}) audio transition not converted",
                body.display_name.as_deref().unwrap_or("unnamed")
            ))
        })?;
    let shape = match (&item.fade_shape_type, &item.fade_shape_value) {
        (Some(kind), Some(value)) => Some((
            integer(
                kind,
                &format!("{}: invalid FadeShapeType", transition.identity),
            )?,
            integer(
                value,
                &format!("{}: invalid FadeShapeValue", transition.identity),
            )?,
        )),
        (None, None) => None,
        _ => {
            return Err(unsupported(format!(
                "{}: incomplete audio fade shape",
                transition.identity
            )))
        }
    };
    ensure!(
        shape == curve.fade_shape()
            || (curve == PrFadeCurve::ConstantPower && shape == Some((1, 19))),
        "{}: {name} with fade shape {shape:?} not converted",
        transition.identity
    );
    ensure!(
        item.crossfade_symmetry
            .as_deref()
            .is_none_or(|value| value == "0"),
        "{}: nondefault CrossfadeSymmetry not converted",
        transition.identity
    );
    Ok(curve)
}

/// Reads one audio transition and checks it against the clips that link it
/// and the converted placements among them.
fn read_transition(
    graph: &Graph<'_>,
    reference: &Reference,
    owner: &str,
    clips: &[TransitionClipLink],
    placements: &[PrAudioOccurrence],
    media: &BTreeMap<MediaId, PrMedia>,
) -> Result<AudioTransition> {
    let record = graph.locate(reference, owner)?;
    let element = record.element();
    let identity = record.identity();
    ensure_known_children(
        element,
        &[
            "TransitionTrackItem",
            "AudioChannelLayout",
            "ChannelType",
            "FrameRate",
            "FadeShapeType",
            "FadeShapeValue",
            "CrossfadeSymmetry",
        ],
        &identity,
    )?;
    if let Some(body) = element.child("TransitionTrackItem") {
        ensure_known_children(
            body,
            &[
                "TrackItem",
                "HasOutgoingClip",
                "HasIncomingClip",
                "DisplayName",
                "MatchName",
                "Alignment",
            ],
            &identity,
        )?;
        if let Some(range) = body.child("TrackItem") {
            ensure_known_children(range, &["Start", "End"], &identity)?;
        }
    }
    let transition = graph.decode_as::<AudioTransitionTrackItem>(record, owner)?;
    let item = &transition.value.transition_track_item;
    let range = required(item.track_item.as_ref(), &identity, "TrackItem")?;
    // Premiere 26.5.1 leaves out a Start at 0.
    let start = match range.start.as_deref() {
        Some(value) => integer(value, &format!("{identity}: invalid Start"))?,
        None => 0,
    };
    let end = integer(&range.end, &format!("{identity}: invalid End"))?;
    ensure!(
        0 <= start && start < end,
        "{identity}: invalid audio transition range"
    );
    let duration = end - start;
    let alignment = integer(
        required(item.alignment.as_deref(), &identity, "Alignment")?,
        &format!("{identity}: invalid Alignment"),
    )?;
    ensure!(
        (0..=duration).contains(&alignment),
        "{identity}: transition Alignment lies outside its range"
    );
    let cut = start + alignment;
    let has_outgoing_clip = video::native_bool(
        required(
            item.has_outgoing_clip.as_deref(),
            &identity,
            "HasOutgoingClip",
        )?,
        &identity,
        "HasOutgoingClip",
    )?;
    let has_incoming_clip = video::native_bool(
        required(
            item.has_incoming_clip.as_deref(),
            &identity,
            "HasIncomingClip",
        )?,
        &identity,
        "HasIncomingClip",
    )?;
    ensure!(
        has_outgoing_clip || has_incoming_clip,
        "{identity}: transition has no adjacent clip"
    );
    let curve = fade_curve(&transition)?;
    // Every corpus record is stereo, over mono clips too. Older records also
    // name the sample rate (48 or 44.1 kHz), which leaves the curve over the
    // transition's span unchanged; Premiere 26.5.1 writes neither field.
    ensure!(
        AudioChannels::parse(&transition.value.audio_channel_layout)
            .is_ok_and(|channels| channels == AudioChannels::Stereo)
            && transition
                .value
                .channel_type
                .as_deref()
                .is_none_or(|value| value == AudioChannels::Stereo.channel_type())
            && transition.value.frame_rate.as_deref().is_none_or(|value| {
                value
                    .parse::<i64>()
                    .is_ok_and(|ticks| ticks > 0 && TICKS % ticks == 0)
            }),
        "{identity}: unsupported audio transition layout"
    );
    // A one-sided fade lies inside its clip: a fade-out ends and a fade-in
    // starts at the cut.
    ensure!(
        has_incoming_clip || alignment == duration,
        "{identity}: a fade-out must end at its clip's end"
    );
    ensure!(
        has_outgoing_clip || alignment == 0,
        "{identity}: a fade-in must start at its clip's start"
    );

    let outgoing: Vec<_> = clips
        .iter()
        .filter(|clip| clip.tail_transition.as_deref() == Some(&identity))
        .collect();
    let incoming: Vec<_> = clips
        .iter()
        .filter(|clip| clip.head_transition.as_deref() == Some(&identity))
        .collect();
    ensure!(
        outgoing.len() == usize::from(has_outgoing_clip)
            && incoming.len() == usize::from(has_incoming_clip),
        "{identity}: transition clip links conflict with HasOutgoingClip/HasIncomingClip"
    );
    if let Some(clip) = outgoing.first() {
        ensure!(
            clip.end_ticks == cut,
            "{identity}: outgoing clip does not end at the transition cut"
        );
        ensure!(
            clip.start_ticks <= start,
            "{identity}: transition starts before its outgoing clip"
        );
    }
    if let Some(clip) = incoming.first() {
        ensure!(
            clip.start_ticks == cut,
            "{identity}: incoming clip does not start at the transition cut"
        );
        ensure!(
            end <= clip.end_ticks,
            "{identity}: transition ends after its incoming clip"
        );
    }
    // The mix of a nested sequence keeps its cut: its audio item has no fade.
    ensure!(
        outgoing
            .iter()
            .chain(&incoming)
            .all(|clip| clip.item != LinkedItem::Nest),
        "{identity}: audio transition of the audio item of a nested sequence not converted"
    );
    // A crossfade plays source past each clip's cut. A side whose placement
    // was omitted has nothing to check; the other side still converts.
    let placement_index = |clip: Option<&&TransitionClipLink>| match clip.map(|clip| clip.item) {
        Some(LinkedItem::Placement(index)) => Some(index),
        _ => None,
    };
    let outgoing = placement_index(outgoing.first());
    let incoming = placement_index(incoming.first());
    if let Some(placement) = outgoing
        .filter(|_| end > cut)
        .map(|index| &placements[index])
    {
        let intrinsic_ticks = media
            .get(&placement.media)
            .and_then(|media| media.audio.as_ref())
            .map_or(0, |stream| stream.intrinsic_ticks);
        ensure!(
            placement
                .source_at(end)
                .is_ok_and(|source| (0..=intrinsic_ticks).contains(&source)),
            "{identity}: outgoing clip lacks the required source handle"
        );
    }
    if let Some(placement) = incoming.map(|index| &placements[index]) {
        let intrinsic_ticks = media
            .get(&placement.media)
            .and_then(|media| media.audio.as_ref())
            .map_or(0, |stream| stream.intrinsic_ticks);
        ensure!(
            placement
                .source_at(start)
                .is_ok_and(|source| (0..=intrinsic_ticks).contains(&source)),
            "{identity}: incoming clip lacks the required source handle"
        );
    }
    // Short spans are retained here. Once the sequence frame grid is known,
    // clamp_short_fades changes only their editable full-level edge.
    // The fade scales the clip Level; its keys carry that Level only while it
    // holds one value over the fade, with no Level key within 2 ms of it but
    // one of that value exactly at the fade's full-level edge.
    for (placement, fade_in) in [(outgoing, false), (incoming, true)]
        .into_iter()
        .filter_map(|(index, fade_in)| Some((&placements[index?], fade_in)))
    {
        if let Some(keys) = &placement.volume_keys {
            let span = placement.source_part(&(start..end))?;
            ensure!(
                level_holds_over_fade(&keys.keys, span, fade_in),
                "{identity}: keyed clip Level changes during the audio fade or within 2 ms of it"
            );
        }
    }
    Ok(AudioTransition {
        outgoing,
        incoming,
        id: identity,
        curve,
        start_ticks: start,
        end_ticks: end,
    })
}

/// What one audio track item plays.
enum AudioItem {
    Media(PrAudioOccurrence),
    /// The mix of a nested sequence.
    Nest(NestSound),
    /// Channel 0 still needs proof that its bus is one centered mono leaf.
    MonoNest(NestSound),
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

/// The measured Fill Right replaces a disconnected right input or the ordinary
/// right source channel with left channel 0, at unity on both outputs. Reuse the
/// full-source mono asset path; do not bake clip gain, trims or source clocks.
fn fill_right_source_channel(
    graph: &Graph<'_>,
    clip: &Located<AudioClip>,
    channels: AudioChannels,
    source_channels: AudioChannels,
    source: Record<'_>,
    centered_stereo_route: bool,
) -> Result<Option<usize>> {
    ensure!(
        channels == AudioChannels::Stereo && centered_stereo_route,
        "{}: Fill Right requires a stereo clip and verified centered stereo routing",
        clip.identity
    );
    match source_channels {
        AudioChannels::Stereo => {
            ensure_channel_order(graph, clip, channels, source)?;
            Ok(Some(0))
        }
        AudioChannels::Mono => {
            let [left, right] = clip.value.secondary_contents.items.as_slice() else {
                return Err(unsupported(format!(
                    "{}: Fill Right requires two clip inputs",
                    clip.identity
                )));
            };
            let left = graph.follow::<SecondaryContent>(left, &clip.identity)?;
            ensure!(
                left.value.channel_index == 0
                    && graph.locate(&left.value.content, &left.identity)? == source,
                "{}: Fill Right left input must be mono source channel 0",
                clip.identity
            );
            // The native disconnected secondary has no Content, so it cannot
            // decode as an ordinary source-channel reference.
            let right = graph.locate(right, &clip.identity)?;
            let mut fields = right.element().children();
            let index = fields.next();
            ensure!(
                right.tag() == "SecondaryContent"
                    && fields.next().is_none()
                    && index.is_some_and(|index| {
                        index.tag() == "ChannelIndex"
                            && index.is_text_only()
                            && index.text() == Some(DISCONNECTED_CHANNEL_INDEX)
                    }),
                "{}: Fill Right requires the saved disconnected right input",
                clip.identity
            );
            Ok(None)
        }
    }
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
    let chain = if chains.len() <= 1 {
        chains.first()
    } else {
        let group = required_integer(
            sub.value.original_channel_group.as_deref(),
            &sub.identity,
            "OrigChGrp",
        )?;
        let matching: Vec<_> = chains
            .iter()
            .filter(|reference| {
                reference
                    .index
                    .as_deref()
                    .and_then(|index| index.parse::<i64>().ok())
                    == Some(group)
            })
            .collect();
        ensure!(
            group >= 0 && matching.len() == 1,
            "{}: OrigChGrp must identify one source audio component chain",
            sub.identity
        );
        Some(matching[0])
    };
    Ok(gain_or_unity(graph, chain, &master.identity, omissions))
}

/// Read the canonical flag without interpreting or replaying opaque scaler data.
fn native_pitch_state(
    clip: &AudioClip,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<bool> {
    let pitch = match clip.clip.maintain_audio_pitch.as_deref() {
        None => false,
        Some("true") => true,
        Some("false") => {
            crate::approximate(omissions, record, "unverified literal MaintainAudioPitch=false imported as editable OFF; native canonical OFF omits the flag and scaler");
            false
        }
        Some(_) => {
            return Err(unsupported(format!(
                "{record}: invalid MaintainAudioPitch flag"
            )))
        }
    };
    let scaler = clip.audio_time_scaler_settings.as_deref();
    let expected = pitch.then_some(crate::schema::native::AUDIO_PITCH_ON_SCALER_SETTINGS);
    if (pitch && scaler.is_none()) || scaler != expected {
        crate::approximate(omissions, record, format!("unverified AudioTimeScalerSettings {scaler:?}; native pitch flag {pitch} retained as editable state, scaler members are not interpreted or replayed"));
    }
    Ok(pitch)
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
    let first_omission = omissions.len();
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
    let muted = super::visibility::is_muted(body.is_muted.as_deref(), &item.identity)?;

    let sub_reference = required(
        body.sub_clip.as_ref(),
        &item.identity,
        records::SUB_CLIP.tag,
    )?;
    let sub = graph.follow::<SubClip>(sub_reference, &item.identity)?;
    let clip = graph.follow::<AudioClip>(&sub.value.clip, &sub.identity)?;
    let native_clip = &clip.value.clip;
    ensure!(
        native_clip.is_multicam != Some(true) && native_clip.selected_track_index.is_none(),
        "{}: multicam audio channel selection is not converted",
        clip.identity
    );
    let playback_rate = video::playback_rate(native_clip, &clip.identity)?;
    let preserve_audio_pitch = native_pitch_state(&clip.value, &clip.identity, omissions)?;
    ensure!(
        native_clip.time_remapping.is_none(),
        "{}: audio TimeRemapping is not converted",
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
        if let Some(record) = &clip_chain.fill_right {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "audio filter {:?} not converted: {}: nested stereo mix plays unchanged",
                    records::FILL_RIGHT_MATCH_NAME,
                    item.identity
                ),
            );
        }
        if preserve_audio_pitch && channels == AudioChannels::Stereo {
            crate::approximate(omissions, &clip.identity, "nested-sequence audio item's pitch control is not represented separately; supported unit-speed sequence sound is retained without that control");
        }
        ensure!(
            playback_rate == 1.0,
            "{}: retimed nested-sequence audio items are not converted",
            clip.identity
        );
        // Stereo retains the ordinary nested mix. Mono retains channel 0 here;
        // read_tracks admits it only through the bounded mono-bus decision.
        if channels == AudioChannels::Mono {
            ensure!(
                !preserve_audio_pitch,
                "{}: nested mono selection requires unit-forward pitch-OFF audio without TimeRemapping",
                clip.identity
            );
            if let [reference] = clip.value.secondary_contents.items.as_slice() {
                let secondary = graph.follow::<SecondaryContent>(reference, &clip.identity)?;
                ensure!(
                    secondary.value.channel_index != 1
                        || graph.locate(&secondary.value.content, &secondary.identity)?
                            != source_record,
                    "{}: nested mono channel 1 selection is not yet mapped (converter follow-up)",
                    clip.identity
                );
            }
        }
        ensure_channel_order(graph, &clip, channels, source_record)?;
        gain *= source_gain(graph, &sub, omissions)?;
        if channels == AudioChannels::Mono {
            ensure!(
                centered_stereo_route
                    && clip_chain.keys.is_empty()
                    && clip_chain.fill_right.is_none()
                    && body.head_transition.is_none()
                    && body.tail_transition.is_none()
                    && omissions[first_omission..]
                        .iter()
                        .all(|note| note.kind == OmissionKind::Approximated),
                "{}: nested mono selection requires static gain and verified centered routing without omitted processing",
                item.identity
            );
            // Selecting the bus does not undo its leaf's center attenuation.
            // The selected mono clip is then centered once on its parent track.
            gain *= AudioChannels::Mono.centered_stereo_gain();
        }
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
        let sound = NestSound {
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
        };
        return Ok(match channels {
            AudioChannels::Stereo => AudioItem::Nest(sound),
            AudioChannels::Mono => AudioItem::MonoNest(sound),
        });
    }
    let source = graph.decode_as::<AudioMediaSource>(source_record, &clip.identity)?;
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
    let stream = media[&media_id]
        .audio
        .as_ref()
        .ok_or_else(|| unsupported(format!("{}: source has no audio stream", source.identity)))?;
    let base_source_channel = || -> Result<Option<PrAudioSourceChannel>> {
        if channels == AudioChannels::Mono && stream.channels == AudioChannels::Stereo {
            let [reference] = clip.value.secondary_contents.items.as_slice() else {
                return Err(unsupported(format!(
                    "{}: mono selection requires one source channel",
                    clip.identity
                )));
            };
            let channel = graph.follow::<SecondaryContent>(reference, &clip.identity)?;
            ensure!(
                channel.value.channel_index < 2
                    && graph.locate(&channel.value.content, &channel.identity)? == source_record,
                "{}: mono selection must name channel 0 or 1 of its stereo source",
                clip.identity
            );
            ensure!(
                centered_stereo_route,
                "{}: mono source-channel selection requires verified centered stereo routing",
                item.identity
            );
            Ok(Some(PrAudioSourceChannel::Mono(
                channel.value.channel_index,
            )))
        } else {
            ensure!(
                stream.channels == channels,
                "{}: clip channel layout differs from the source",
                clip.identity
            );
            ensure_channel_order(graph, &clip, channels, source_record)?;
            Ok(None)
        }
    };
    let source_channel = if let Some(record) = &clip_chain.fill_right {
        match fill_right_source_channel(
            graph,
            &clip,
            channels,
            stream.channels,
            source_record,
            centered_stereo_route,
        ) {
            Ok(Some(_)) => Some(PrAudioSourceChannel::FillRight(record.clone())),
            Ok(None) => None,
            Err(error) => {
                // Only a route that was independently playable may survive an
                // unsupported insert. Genuine selectors keep their safety checks.
                let channel = match base_source_channel() {
                    Ok(channel) => channel,
                    Err(_) => return Err(error),
                };
                let retained = if channels == AudioChannels::Stereo
                    && stream.channels == AudioChannels::Stereo
                {
                    "stereo source plays unchanged"
                } else {
                    "base source routing plays unchanged"
                };
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!(
                        "audio filter {:?} not converted: {}: {error}; {retained}",
                        records::FILL_RIGHT_MATCH_NAME,
                        item.identity
                    ),
                );
                channel
            }
        }
    } else {
        base_source_channel()?
    };
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
        source_channel,
        preserve_audio_pitch,
        playback_rate,
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
        fade_in: None,
        fade_out: None,
    };
    occurrence.validate(stream)?;
    if media_source
        .content
        .as_ref()
        .and_then(|content| content.audio_proxies.as_ref())
        .is_some_and(|proxies| !proxies.items.is_empty())
    {
        omit(
            omissions,
            OmissionScope::Feature,
            &source.identity,
            "AudioProxies attachments and preview preference are not retained or exported; sound selection uses only primary Media and its original channel selection, subject to media admission",
        );
    }
    Ok(AudioItem::Media(occurrence))
}

#[cfg(test)]
mod short_fade_tests {
    use super::*;

    fn clip(duration_ms: i64) -> PrAudioOccurrence {
        PrAudioOccurrence {
            source_channel: None,
            id: Some("short-clip".into()),
            media: MediaId("tone".into()),
            start_ticks: 0,
            end_ticks: duration_ms * TICKS_PER_MILLISECOND,
            in_ticks: 0,
            out_ticks: duration_ms * TICKS_PER_MILLISECOND,
            playback_rate: 1.0,
            preserve_audio_pitch: false,
            volume: LinearGain::new(0.25).unwrap(),
            volume_keys: None,
            fade_in: Some(PrAudioFade {
                id: Some(format!("short-head-{duration_ms}")),
                curve: PrFadeCurve::ConstantPower,
                duration_ticks: 16 * TICKS_PER_MILLISECOND,
            }),
            fade_out: None,
        }
    }

    #[test]
    fn short_threshold_uses_ticks_not_absolute_millisecond_rounding() {
        let mut placement = clip(1000);
        placement.start_ticks = 1003 * TICKS_PER_MILLISECOND / 10;
        placement.end_ticks += placement.start_ticks;
        placement.fade_in.as_mut().unwrap().duration_ticks = 384 * TICKS_PER_MILLISECOND / 10;
        let mut omissions = Vec::new();
        clamp_short_fades(
            std::slice::from_mut(&mut placement),
            FrameRate::Fps30,
            &mut omissions,
        );
        assert_eq!(
            placement.fade_in.as_ref().unwrap().duration_ticks,
            TICKS / 6 - placement.start_ticks
        );
        assert_eq!(
            placement.end_ticks - placement.start_ticks,
            1000 * TICKS_PER_MILLISECOND
        );
        assert_eq!(
            (placement.in_ticks, placement.out_ticks),
            (0, 1000 * TICKS_PER_MILLISECOND)
        );
        assert_eq!(omissions.len(), 1);
        assert_eq!(omissions[0].kind, crate::OmissionKind::Approximated);
    }

    #[test]
    fn short_grid_clamping_preserves_levels_and_confines_both_fades() {
        let mut narrow = clip(16);
        let mut two = clip(80);
        two.fade_out = two.fade_in.clone();
        let mut keyed = clip(1000);
        keyed.volume_keys = Some(PrVolumeKeys {
            keys: vec![
                PrScalarKeyframe {
                    source_ticks: 50 * TICKS_PER_MILLISECOND,
                    value: 1.0,
                    easing: PrKeyframeEasing::Linear,
                },
                PrScalarKeyframe {
                    source_ticks: 100 * TICKS_PER_MILLISECOND,
                    value: 0.5,
                    easing: PrKeyframeEasing::Linear,
                },
            ],
            gain: 0.25,
        });
        let mut omissions = Vec::new();
        clamp_short_fades(
            std::slice::from_mut(&mut narrow),
            FrameRate::Fps30,
            &mut omissions,
        );
        assert_eq!(
            narrow.fade_in.unwrap().duration_ticks,
            16 * TICKS_PER_MILLISECOND
        );
        clamp_short_fades(
            std::slice::from_mut(&mut two),
            FrameRate::Fps30,
            &mut omissions,
        );
        assert_eq!(
            two.fade_in.unwrap().duration_ticks,
            64 * TICKS_PER_MILLISECOND
        );
        assert_eq!(
            two.fade_out.unwrap().duration_ticks,
            16 * TICKS_PER_MILLISECOND
        );
        clamp_short_fades(
            std::slice::from_mut(&mut keyed),
            FrameRate::Fps30,
            &mut omissions,
        );
        assert_eq!(
            keyed.fade_in.unwrap().duration_ticks,
            16 * TICKS_PER_MILLISECOND
        );
        assert_eq!(keyed.volume.as_f64(), 0.25);
        let keys = keyed.volume_keys.unwrap();
        assert_eq!(keys.gain, 0.25);
        assert_eq!(
            (keys.keys[0].source_ticks, keys.keys[0].value),
            (50 * TICKS_PER_MILLISECOND, 1.0)
        );
        assert_eq!(
            (keys.keys[1].source_ticks, keys.keys[1].value),
            (100 * TICKS_PER_MILLISECOND, 0.5)
        );
        assert_eq!(omissions.len(), 4);
        assert!(omissions
            .iter()
            .all(|note| note.kind == crate::OmissionKind::Approximated));
    }
}
