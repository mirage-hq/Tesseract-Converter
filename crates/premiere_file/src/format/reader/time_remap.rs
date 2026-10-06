//! Recover usable source-time keys without discarding their picture owner.

use crate::{
    error::{ensure, unsupported, Result},
    format::Graph,
    schema::{
        native::{Reference, TimeComponentParam, TimeRemapping, VideoClip},
        records, PrKeyframeEasing, PrTimeRemap, PrTimeRemapKeyframe, TICKS,
    },
    {approximate, Omission},
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

#[cfg(test)]
pub(in crate::format) fn read_time_remapping(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    omissions: &mut Vec<Omission>,
) -> Result<PrTimeRemap> {
    match read_optional_time_remapping(graph, reference, from, omissions)? {
        TimeRemapDisposition::Supported(curve) => Ok(curve),
        TimeRemapDisposition::Unsupported(error)
        | TimeRemapDisposition::UnsupportedBinding(error) => Err(error),
    }
}

/// Consumed graph identity and required known-record decoding stay outer errors.
/// Optional curve losses recover only after independent clock validation.
pub(super) enum TimeRemapDisposition {
    Supported(PrTimeRemap),
    Unsupported(crate::error::BuildError),
    /// Unknown optional layouts must not be decoded as the known curve. Only
    /// the physical-media caller can recover their independently saved base.
    UnsupportedBinding(crate::error::BuildError),
}

pub(super) fn read_optional_time_remapping(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    omissions: &mut Vec<Omission>,
) -> Result<TimeRemapDisposition> {
    let mapping = graph.locate_as(reference, records::TIME_REMAPPING.tag, from)?;
    if mapping.element().attribute("ClassID") != Some(records::TIME_REMAPPING.class_id) {
        return Ok(TimeRemapDisposition::UnsupportedBinding(unsupported(
            format!("{}: incompatible TimeRemapping binding", mapping.identity()),
        )));
    }
    let mapping = graph.decode::<TimeRemapping>(mapping)?;
    let param = graph.locate_as(
        &mapping.value.keyframes,
        records::TIME_COMPONENT_PARAM.tag,
        &mapping.identity,
    )?;
    if param.element().attribute("ClassID") != Some(records::TIME_COMPONENT_PARAM.class_id) {
        return Ok(TimeRemapDisposition::UnsupportedBinding(unsupported(
            format!("{}: incompatible TimeRemapping binding", param.identity()),
        )));
    }
    let param = graph.decode::<TimeComponentParam>(param)?;
    if param.value.parameter_id != "-1" {
        return Ok(TimeRemapDisposition::UnsupportedBinding(unsupported(
            format!("{}: incompatible TimeRemapping binding", param.identity),
        )));
    }
    Ok(match read_curve(&param, omissions) {
        Ok(curve) => TimeRemapDisposition::Supported(curve),
        Err(error) => TimeRemapDisposition::Unsupported(error),
    })
}

fn read_curve(
    param: &crate::format::Located<TimeComponentParam>,
    omissions: &mut Vec<Omission>,
) -> Result<PrTimeRemap> {
    let mut native = Vec::<NativeKey>::new();
    for item in param
        .value
        .keyframes
        .split(';')
        .filter(|item| !item.is_empty())
    {
        let fields: Vec<_> = item.split(',').collect();
        let parsed = (|| -> Result<NativeKey> {
            let time = fields
                .first()
                .ok_or_else(|| unsupported("missing key time"))?;
            let source = fields
                .get(1)
                .ok_or_else(|| unsupported("missing source time"))?;
            Ok(NativeKey {
                timeline_ticks: time.parse().map_err(|_| unsupported("invalid key time"))?,
                source_ticks: source_ticks(source, &param.identity)?,
                mode: fields
                    .get(2)
                    .and_then(|mode| mode.parse().ok())
                    .unwrap_or(6),
            })
        })();
        match parsed {
            Ok(key)
                if native
                    .last()
                    .is_none_or(|previous| key.timeline_ticks > previous.timeline_ticks) =>
            {
                if fields.len() != 8 || !matches!(key.mode, 6..=8) {
                    approximate(omissions, &param.identity,
                        "TimeRemapping key metadata was not reproduced; usable source/timeline values were retained");
                }
                native.push(key);
            }
            Ok(_) => approximate(
                omissions,
                &param.identity,
                "duplicate or out-of-order TimeRemapping key was skipped; other keys were retained",
            ),
            Err(error) => approximate(
                omissions,
                &param.identity,
                format!(
                    "unusable TimeRemapping key was skipped: {error}; other keys were retained"
                ),
            ),
        }
    }
    ensure!(
        native.len() >= 2,
        "{}: TimeRemapping has fewer than two usable keys",
        param.identity
    );
    let mut keys = Vec::with_capacity(native.len());
    for (index, key) in native.iter().copied().enumerate() {
        let easing = if index > 0 && native[index - 1].mode == 7 && key.mode == 8 {
            match ramp_easing(&native, index - 1, &param.identity) {
                Ok(easing) => easing,
                Err(error) => {
                    approximate(omissions, &param.identity,
                        format!("TimeRemapping easing approximated as linear: {error}; source/timeline keys were retained"));
                    PrKeyframeEasing::Linear
                }
            }
        } else {
            if key.mode != 6
                && !(key.mode == 7 && native.get(index + 1).is_some_and(|next| next.mode == 8))
            {
                approximate(omissions, &param.identity,
                    "unpaired or unknown TimeRemapping mode approximated as linear; source/timeline keys were retained");
            }
            PrKeyframeEasing::Linear
        };
        keys.push(PrTimeRemapKeyframe {
            timeline_ticks: key.timeline_ticks,
            source_ticks: key.source_ticks,
            easing,
        });
    }
    Ok(PrTimeRemap { keys })
}
