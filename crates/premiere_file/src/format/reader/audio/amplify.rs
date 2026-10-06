//! Saved stereo ducking uses Amplify, not intrinsic clip Volume.

use super::{animation, filter_params, static_switch, static_value, ClipVolume};
use crate::{
    approximate,
    error::{ensure, unsupported, Result},
    format::{Graph, Record},
    schema::{native::AudioFilterComponent, AudioChannels, PrKeyframeEasing, PrScalarKeyframe},
    Omission,
};

pub(super) const MATCH_NAME: &str = "f14fee7d-fe5f-4202-b790-f53209e56411";
// Premiere's saved Amplify stereo routing, with no channel remap.
const STEREO_CONFIG: &str = r#"{"in":[{"layout":[100,101],"name":"","type":0}],"out":[{"layout":[100,101],"name":"","type":0}]}"#;
// Amplify interpolates normalized values linearly in dB. Intrinsic Volume
// interpolates in fader position instead. Small dB pieces retain the former
// through the existing Volume fit without adding a second animation model.
const PIECE_DB: f64 = 2.0;

pub(super) fn read(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<ClipVolume> {
    let filter = graph.decode::<AudioFilterComponent>(record)?;
    let component = &filter.value.audio_component.component;
    ensure!(
        matches!(
            component.bypass.as_deref(),
            None | Some("false") | Some("true")
        ),
        "invalid Amplify component bypass"
    );
    let unity = || ClipVolume {
        level: 1.0,
        keys: Vec::new(),
        muted: false,
        unread: false,
    };
    if component.bypass.as_deref() == Some("true") {
        return Ok(unity());
    }
    let params = filter_params(graph, &filter)?;
    ensure!(params.len() == 36, "unknown Amplify parameter layout");
    for (index, expected) in [
        (0, "Bypass"),
        (1, "Channel Count"),
        (2, "Link Sliders"),
        (3, "Left"),
        (4, "Right"),
        (35, "Link Keyframes"),
    ] {
        ensure!(
            params[index].value.name.as_deref() == Some(expected),
            "unknown Amplify parameter {index}"
        );
    }
    ensure!(
        params[5..35].iter().all(|param| param.value.name.is_none()),
        "unknown Amplify channel parameters"
    );
    ensure!(
        params[0].value.keyframes.is_none()
            && matches!(
                params[0].value.is_time_varying.as_deref(),
                None | Some("false")
            ),
        "keyed Amplify bypass"
    );
    if static_switch(&params[0], "Amplify bypass")? {
        return Ok(unity());
    }
    ensure!(
        params[1].value.keyframes.is_none()
            && static_value(&params[1], "Amplify channel count", 0.0)? == 2.0 / 32.0
            && filter.value.audio_component.audio_channel_layout.as_deref()
                == Some(AudioChannels::Stereo.layout())
            && filter.value.channel_config_data.as_deref() == Some(STEREO_CONFIG),
        "only unremapped stereo Amplify is converted"
    );
    let left = &params[3];
    let level = gain(static_value(left, "Amplify Left", 2.0 / 3.0)?)?;
    let wire = left.value.keyframes.as_deref().unwrap_or_default();
    for channel in &params[3..35] {
        ensure!(
            gain(static_value(channel, "Amplify channel", 2.0 / 3.0)?)? == level
                && channel.value.keyframes == left.value.keyframes,
            "unequal Amplify channel gains or keys"
        );
        let varying = channel.value.is_time_varying.as_deref();
        ensure!(
            matches!(varying, None | Some("true") | Some("false"))
                && varying != Some(if wire.is_empty() { "true" } else { "false" }),
            "Amplify keys and IsTimeVarying disagree"
        );
    }
    // The scalar decoder also admits Bezier forms; Amplify's handles have no
    // native calibration, so admit only its saved Linear/Hold modes here.
    ensure!(
        wire.split_terminator(';')
            .all(|key| matches!(key.split(',').nth(2), Some("0" | "4"))),
        "Bezier Amplify keys are not converted"
    );
    let native = animation::scalar_keys(wire, &left.identity)?;
    let mut keys: Vec<PrScalarKeyframe> = Vec::new();
    let mut fitted_ramp = false;
    for (index, key) in native.iter().enumerate() {
        let to_db = decibels(key.value)?;
        if let Some(previous) = index.checked_sub(1).map(|index| &native[index]) {
            let from_db = decibels(previous.value)?;
            if key.easing != PrKeyframeEasing::Hold && from_db != to_db {
                fitted_ramp = true;
                // Normalized values are in [0, 1], hence at most 72 pieces.
                let pieces = ((to_db - from_db).abs() / PIECE_DB).ceil() as i64;
                let span = key
                    .source_ticks
                    .checked_sub(previous.source_ticks)
                    .ok_or_else(|| unsupported("Amplify key interval overflows"))?;
                for part in 1..pieces {
                    // Quotient/remainder arithmetic avoids overflow for large
                    // source clocks and keeps each inserted tick in the interval.
                    let ticks = previous.source_ticks
                        + span / pieces * part
                        + span % pieces * part / pieces;
                    keys.push(PrScalarKeyframe {
                        source_ticks: ticks,
                        value: 10_f64.powf(
                            (from_db + (to_db - from_db) * part as f64 / pieces as f64) / 20.0,
                        ),
                        easing: PrKeyframeEasing::Linear,
                    });
                }
            }
        }
        keys.push(PrScalarKeyframe {
            value: gain(key.value)?,
            ..*key
        });
    }
    if fitted_ramp {
        approximate(omissions, &filter.identity,
            "Amplify linear-dB ramps use clip Volume pieces at most 2 dB apart; millisecond timing and Volume curve-fit precision apply");
    }
    Ok(ClipVolume {
        level,
        keys,
        muted: false,
        unread: false,
    })
}

fn decibels(value: f64) -> Result<f64> {
    ensure!(
        value.is_finite() && (0.0..=1.0).contains(&value),
        "invalid normalized Amplify gain"
    );
    // The native saved ducking keys read back as 0.666666686535 and
    // 0.541666686535. Human UI readback gives 0 and -18 dB; at 00;00;03;28
    // (29.97 DF), this scale predicts -16.8135 dB, displayed as -16.81.
    // Float32 storage accounts for the approximately 0.000003 dB residual.
    Ok(144.0 * value - 96.0)
}

fn gain(value: f64) -> Result<f64> {
    Ok(10_f64.powf(decibels(value)? / 20.0))
}
