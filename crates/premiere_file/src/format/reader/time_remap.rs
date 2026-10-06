//! Fail-closed reader for Premiere's intrinsic source-time curve.

use super::required;
use crate::{
    error::{ensure, unsupported, Result},
    format::Graph,
    schema::{
        native::{Reference, TimeComponentParam, TimeRemapping, VideoClip},
        records, PrKeyframeEasing, PrTimeRemap, PrTimeRemapKeyframe, TICKS,
    },
};

/// The measured explicit-source native frame hold.
pub(super) fn read_frame_hold(
    clip: &VideoClip,
    duration: i64,
    context: &str,
) -> Result<Option<PrTimeRemap>> {
    if !clip.declares_frame_hold() {
        return Ok(None);
    }
    let (mode, source) = match (&clip.frame_hold, &clip.frame_hold_start) {
        (Some(mode), Some(source)) => (mode, source),
        _ => return Err(unsupported(format!("{context}: incomplete FrameHold"))),
    };
    ensure!(
        mode == VideoClip::EXPLICIT_FRAME_HOLD,
        "{context}: unsupported FrameHold mode {mode:?}"
    );
    ensure!(
        clip.clip
            .as_ref()
            .is_none_or(|clip| clip.time_remapping.is_none()),
        "{context}: FrameHold combined with TimeRemapping is unsupported"
    );
    let source_ticks = source
        .parse::<i64>()
        .map_err(|_| unsupported(format!("{context}: invalid FrameHoldStart")))?;
    ensure!(
        source_ticks >= 0 && duration > 0,
        "{context}: invalid FrameHold range"
    );
    Ok(Some(PrTimeRemap::frame_hold(source_ticks, duration)))
}

#[derive(Clone, Copy)]
struct NativeKey {
    timeline_ticks: i64,
    source_ticks: i64,
    mode: u8,
}

fn source_ticks(value: &str, context: &str) -> Result<i64> {
    let seconds = value
        .parse::<f64>()
        .map_err(|_| unsupported(format!("{context}: invalid source time")))?;
    let scaled = seconds * TICKS as f64;
    ensure!(
        seconds >= 0.0 && scaled.is_finite() && scaled <= i64::MAX as f64,
        "{context}: source time is outside Premiere's tick range"
    );
    let rounded = scaled.round();
    ensure!(
        matches!(
            (scaled - rounded).abs().partial_cmp(&1.0),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ),
        "{context}: source time is not representable in Premiere ticks"
    );
    Ok(rounded as i64)
}

fn slope(from: NativeKey, to: NativeKey, context: &str) -> Result<f64> {
    let input = (i128::from(to.timeline_ticks) - i128::from(from.timeline_ticks)) as f64;
    let output = (i128::from(to.source_ticks) - i128::from(from.source_ticks)) as f64;
    ensure!(
        input > 0.0 && output > 0.0,
        "{context}: stationary and reverse TimeRemapping segments are not supported"
    );
    Ok(output / input)
}

fn ramp_easing(keys: &[NativeKey], start: usize, context: &str) -> Result<PrKeyframeEasing> {
    ensure!(
        start > 0 && start + 2 < keys.len(),
        "{context}: speed ramp requires adjacent plateau segments"
    );
    let ramp_input = (i128::from(keys[start + 1].timeline_ticks)
        - i128::from(keys[start].timeline_ticks)) as f64;
    let ramp_output =
        (i128::from(keys[start + 1].source_ticks) - i128::from(keys[start].source_ticks)) as f64;
    ensure!(
        ramp_input > 0.0 && ramp_output > 0.0,
        "{context}: stationary and reverse speed ramps are not supported"
    );
    let scale = ramp_input / ramp_output;
    let y1 = slope(keys[start - 1], keys[start], context)? * scale / 3.0;
    let y2 = 1.0 - slope(keys[start + 1], keys[start + 2], context)? * scale / 3.0;
    ensure!(
        (0.0..=1.0).contains(&y1) && (0.0..=1.0).contains(&y2),
        "{context}: speed-ramp derivative cannot be represented by bounded FX easing"
    );
    Ok(PrKeyframeEasing::CubicBezier {
        x1: 1.0 / 3.0,
        y1,
        x2: 2.0 / 3.0,
        y2,
    })
}

/// `curve`, whose keys are at native input ticks, with its key times in input
/// ticks after the `source_in` from which a placement plays it at forward
/// `rate`. Premiere shows the curve at input `source_in + rate × elapsed`
/// (Adobe-measured on physical video), so the placement reaches a key
/// `timeline_ticks / rate` ticks after its start, a quotient that the
/// converter keeps exact until the millisecond. A key before `source_in`
/// becomes negative. Source values and easing are unchanged.
pub(super) fn after_source_in(
    mut curve: PrTimeRemap,
    source_in: i64,
    rate: f64,
    context: &str,
) -> Result<PrTimeRemap> {
    // A NaN speed compares with no number and is rejected too.
    ensure!(
        matches!(rate.partial_cmp(&0.0), Some(std::cmp::Ordering::Greater)),
        "{context}: reverse playback combined with TimeRemapping is unsupported"
    );
    for key in &mut curve.keys {
        key.timeline_ticks = key.timeline_ticks.checked_sub(source_in).ok_or_else(|| {
            unsupported(format!(
                "{context}: TimeRemapping key time exceeds Premiere's tick range"
            ))
        })?;
    }
    Ok(curve)
}

pub(in crate::format) fn read_time_remapping(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
) -> Result<PrTimeRemap> {
    let mapping = graph.follow::<TimeRemapping>(reference, from)?;
    ensure!(
        mapping.value.class_id == records::TIME_REMAPPING.class_id
            && mapping.value.version == records::TIME_REMAPPING.version,
        "{}: unsupported TimeRemapping record",
        mapping.identity
    );
    let param = graph.follow::<TimeComponentParam>(&mapping.value.keyframes, &mapping.identity)?;
    // Premiere 26.5.1 saves Version 9, which omits the five flags below.
    // Version 8 saves each of them.
    let omits_flags = param.value.version == "9";
    ensure!(
        param.value.class_id == records::TIME_COMPONENT_PARAM.class_id
            && (omits_flags || param.value.version == records::TIME_COMPONENT_PARAM.version)
            && param.value.name == "Speed"
            && param.value.parameter_id == "-1"
            && param.value.lower_bound == "0",
        "{}: unsupported TimeRemapping parameter",
        param.identity
    );
    // A saved flag must hold the value that Version 8 saves, in either version.
    for (field, value, saved) in [
        ("IsTimeVarying", &param.value.is_time_varying, "true"),
        ("IsLocked", &param.value.is_locked, "false"),
        (
            "DiscontinuousInterpolate",
            &param.value.discontinuous_interpolate,
            "false",
        ),
        (
            "ParameterControlType",
            &param.value.parameter_control_type,
            "21",
        ),
        ("RangeLocked", &param.value.range_locked, "false"),
    ] {
        if value.is_none() && omits_flags {
            continue;
        }
        let value = required(value.as_deref(), &param.identity, field)?;
        ensure!(
            value == saved,
            "{}: unsupported TimeRemapping parameter {field} {value:?}",
            param.identity
        );
    }
    let start: Vec<_> = param.value.start_keyframe.split(',').collect();
    ensure!(
        start.len() == 8
            && start[0] == records::STATIC_KEYFRAME_TIME
            && source_ticks(start[1], &param.identity)? == 0
            && start[2..]
                .iter()
                .all(|field| field.parse::<f64>().ok() == Some(0.0)),
        "{}: unsupported TimeRemapping initial key",
        param.identity
    );
    ensure!(
        param.value.keyframes.ends_with(';'),
        "{}: unterminated TimeRemapping keys",
        param.identity
    );
    let mut native = Vec::new();
    for item in param.value.keyframes.split_terminator(';') {
        let fields: Vec<_> = item.split(',').collect();
        ensure!(
            fields.len() == 8
                && fields[3..]
                    .iter()
                    .all(|field| field.parse::<f64>().ok() == Some(0.0)),
            "{}: unexpected TimeRemapping key shape",
            param.identity
        );
        let key = NativeKey {
            timeline_ticks: fields[0].parse().map_err(|_| {
                unsupported(format!(
                    "{}: invalid TimeRemapping key time",
                    param.identity
                ))
            })?,
            source_ticks: source_ticks(fields[1], &param.identity)?,
            mode: fields[2].parse().map_err(|_| {
                unsupported(format!("{}: invalid TimeRemapping mode", param.identity))
            })?,
        };
        ensure!(
            matches!(key.mode, 6..=8),
            "{}: unsupported TimeRemapping mode {}",
            param.identity,
            key.mode
        );
        if let Some(previous) = native.last().copied() {
            slope(previous, key, &param.identity)?;
        }
        native.push(key);
    }
    ensure!(
        native.len() >= 4 && native[0].mode == 6 && native.last().is_some_and(|key| key.mode == 6),
        "{}: incomplete TimeRemapping boundary keys",
        param.identity
    );
    let mut ramp_count = 0;
    let mut keys = Vec::with_capacity(native.len());
    for (index, key) in native.iter().copied().enumerate() {
        let easing = if index > 0 && native[index - 1].mode == 7 {
            ensure!(
                key.mode == 8,
                "{}: ramp start is not followed by a ramp end",
                param.identity
            );
            ramp_count += 1;
            ramp_easing(&native, index - 1, &param.identity)?
        } else {
            ensure!(
                key.mode != 8,
                "{}: ramp end has no ramp start",
                param.identity
            );
            PrKeyframeEasing::Linear
        };
        keys.push(PrTimeRemapKeyframe {
            timeline_ticks: key.timeline_ticks,
            source_ticks: key.source_ticks,
            easing,
        });
    }
    ensure!(
        ramp_count > 0,
        "{}: TimeRemapping has no variable-speed segment",
        param.identity
    );
    Ok(PrTimeRemap { keys })
}
