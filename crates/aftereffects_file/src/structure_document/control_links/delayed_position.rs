//! Bounded lowering for one authored delayed-Master Position rig.
//!
//! The expression text is recognized exactly after removing whitespace and
//! line comments. Its referenced native curves are evaluated analytically;
//! only the final scalar movement is approximated with sparse editable keys.

use fx_keyframe_bake::curve_fit::{FittedCurve, FittedEasing, fit_scalar_curve};

use super::{display_name, expression, unique_run};
use crate::{
    properties::{self, NumericKeyframe, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

const CANONICAL_EXPRESSION: &str = "varp=thisComp.layer(\"Master\").transform.position;varfps=thisComp.frameDuration;vars=timeRemap;vard=thisComp.layer(\"Master\").effect(\"delay\")(\"Slider\")*fps;varamnt=easeOut(time,s.key(1).time,s.key(2).time,1,0);p.valueAtTime(time-amnt*d);";
const FIT_TOLERANCE_PIXELS: f64 = 0.01;

pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let text = position_expression(layer).ok()?;
    if normalized_expression(text).as_deref() != Some(CANONICAL_EXPRESSION) {
        return None;
    }
    Some(lower_recognized(layer, composition, base))
}

fn position_expression(layer: &Layer) -> Result<&str, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let position = properties::unique_list(unique_run(&leaves, "ADBE Position")?, *b"tdbs")?;
    expression(position)
}

fn normalized_expression(text: &str) -> Option<String> {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quote = None;
    while let Some(character) = chars.next() {
        if let Some(delimiter) = quote {
            normalized.push(character);
            if character == '\\' {
                normalized.push(chars.next()?);
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            normalized.push(character);
        } else if character == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for comment in chars.by_ref() {
                if matches!(comment, '\n' | '\r') {
                    break;
                }
            }
        } else if !character.is_whitespace() {
            normalized.push(character);
        }
    }
    quote.is_none().then_some(normalized)
}

fn lower_recognized(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Result<NumericProperty, PropertyError> {
    validate_base(base)?;
    let (owner_start, owner_stretch, active_start, active_end) = layer_clock(layer)?;
    let remap = time_remap(layer)?;
    let remap_start = owner_start + remap.keyframes[0].time_secs * owner_stretch;
    let remap_end = owner_start + remap.keyframes[1].time_secs * owner_stretch;
    if !remap_start.is_finite() || !remap_end.is_finite() || remap_end <= remap_start {
        return Err(PropertyError::Layout(
            "invalid delayed Position remap clock",
        ));
    }

    let controller = unique_layer(composition, "Master")?;
    if controller.record.id() == layer.record.id() {
        return Err(PropertyError::Layout("cyclic delayed Position controller"));
    }
    let (controller_start, controller_stretch) = source_clock(controller)?;
    let source = controller_position(controller)?;
    validate_source(&source, base.values.len())?;
    let delay_frames = delay_frames(controller)?;
    if !delay_frames.is_finite()
        || !composition.frame_rate.is_finite()
        || composition.frame_rate <= 0.0
    {
        return Err(PropertyError::Layout(
            "invalid delayed Position frame clock",
        ));
    }
    let delay_seconds = delay_frames / composition.frame_rate;
    if !delay_seconds.is_finite() {
        return Err(PropertyError::Layout("nonfinite delayed Position offset"));
    }

    let moving = moving_components(&source);
    let Some(&component) = moving.first() else {
        let values =
            evaluate_position(&source, active_start, controller_start, controller_stretch)?;
        return Ok(NumericProperty {
            values,
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        });
    };

    let span_ms = ((active_end - active_start) * 1_000.0).ceil();
    if !span_ms.is_finite() || span_ms <= 0.0 || span_ms >= u64::MAX as f64 {
        return Err(PropertyError::Layout(
            "delayed Position duration is not representable in milliseconds",
        ));
    }
    let duration_ms = span_ms as u64;
    let mut evaluate = |offset_ms: u64| {
        let time = active_start + offset_ms as f64 / 1_000.0;
        let amount = ease_out(time, remap_start, remap_end, 1.0, 0.0);
        let sample_time = time - amount * delay_seconds;
        evaluate_position(&source, sample_time, controller_start, controller_stretch)
            .map(|values| values[component])
    };
    // Fit with headroom: the fitter probes a finite set of integer-millisecond
    // intervals, while validation below checks every millisecond against the
    // unchanged acceptance tolerance.
    let curve = fit_curve(duration_ms, &mut evaluate)?;
    validate_fit(&curve, duration_ms, &mut evaluate)?;
    let mut evaluate_values = |offset_ms: u64| {
        let time = active_start + offset_ms as f64 / 1_000.0;
        let amount = ease_out(time, remap_start, remap_end, 1.0, 0.0);
        evaluate_position(
            &source,
            time - amount * delay_seconds,
            controller_start,
            controller_stretch,
        )
    };
    let output = fitted_property(
        &curve,
        component,
        active_start,
        owner_start,
        owner_stretch,
        &mut evaluate_values,
    )?;
    validate_output(
        &output,
        duration_ms,
        active_start,
        owner_start,
        owner_stretch,
        &mut evaluate_values,
    )?;
    Ok(output)
}

fn validate_base(base: &NumericProperty) -> Result<(), PropertyError> {
    if base.animated
        || !base.keyframes.is_empty()
        || !base.expression_enabled
        || base.dimensions_separated
        || base.value_kind != NumericValueKind::Continuous
        || !(2..=3).contains(&base.values.len())
        || base.values.iter().any(|value| !value.is_finite())
    {
        return Err(PropertyError::Layout(
            "unsupported delayed Position destination",
        ));
    }
    Ok(())
}

fn layer_clock(layer: &Layer) -> Result<(f64, f64, f64, f64), PropertyError> {
    let (start, stretch) = source_clock(layer)?;
    let (Some(in_point), Some(out_point)) = (layer.record.in_point(), layer.record.out_point())
    else {
        return Err(PropertyError::Layout(
            "invalid delayed Position active range",
        ));
    };
    let active_start = start + in_point * stretch;
    let active_end = start + out_point * stretch;
    if !in_point.is_finite()
        || !out_point.is_finite()
        || !active_start.is_finite()
        || !active_end.is_finite()
        || active_end <= active_start
    {
        return Err(PropertyError::Layout(
            "invalid delayed Position active range",
        ));
    }
    Ok((start, stretch, active_start, active_end))
}

fn source_clock(layer: &Layer) -> Result<(f64, f64), PropertyError> {
    let (Some(start), Some(stretch)) = (layer.record.start_time(), layer.record.stretch()) else {
        return Err(PropertyError::Layout(
            "invalid delayed Position source clock",
        ));
    };
    if !start.is_finite() || !stretch.is_finite() || stretch <= 0.0 {
        return Err(PropertyError::Layout(
            "delayed Position requires positive finite stretch",
        ));
    }
    Ok((start, stretch))
}

fn time_remap(layer: &Layer) -> Result<NumericProperty, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let remap = properties::unique_list(unique_run(&root, "ADBE Time Remapping")?, *b"tdbs")?;
    let numeric = properties::read_numeric(remap)?;
    if numeric.expression_enabled
        || !numeric.animated
        || numeric.keyframes.len() < 2
        || numeric.keyframes.iter().any(|key| {
            !key.time_secs.is_finite()
                || key.values.len() != 1
                || !key.spatial_in.is_empty()
                || !key.spatial_out.is_empty()
        })
    {
        return Err(PropertyError::Layout(
            "unsupported delayed Position time remap",
        ));
    }
    Ok(numeric)
}

fn unique_layer<'a>(composition: &'a Composition, name: &str) -> Result<&'a Layer, PropertyError> {
    let mut matches = composition
        .layers
        .iter()
        .filter(|layer| layer.name.as_ref() == name);
    let layer = matches
        .next()
        .ok_or(PropertyError::Layout("delayed Position controller missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout(
            "ambiguous delayed Position controller",
        ));
    }
    Ok(layer)
}

fn controller_position(layer: &Layer) -> Result<NumericProperty, PropertyError> {
    let properties = properties::read_transform(&layer.content)?;
    let mut matches = properties
        .into_iter()
        .filter(|property| property.match_name == "ADBE Position");
    let property = matches
        .next()
        .ok_or(PropertyError::Layout("delayed Position source missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous delayed Position source"));
    }
    property.numeric
}

fn delay_frames(layer: &Layer) -> Result<f64, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let mut matches = effects.iter().filter_map(|(kind, run)| {
        let plugin = properties::unique_list(run, *b"sspc").ok()?;
        let body = properties::unique_list(plugin, *b"tdgp").ok()?;
        (display_name(body) == Some("delay")).then_some((*kind, body))
    });
    let (kind, body) = matches.next().ok_or(PropertyError::Layout(
        "delayed Position delay control missing",
    ))?;
    if matches.next().is_some() || kind != "ADBE Slider Control" {
        return Err(PropertyError::Layout(
            "ambiguous delayed Position delay control",
        ));
    }
    let parameters = properties::runs(body)?;
    let value = properties::unique_list(
        unique_run(&parameters, "ADBE Slider Control-0001")?,
        *b"tdbs",
    )?;
    let numeric = properties::read_numeric(value)?;
    let [frames] = numeric.values.as_slice() else {
        return Err(PropertyError::Layout(
            "delayed Position delay must be static scalar",
        ));
    };
    if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
        return Err(PropertyError::Layout(
            "delayed Position delay must be static scalar",
        ));
    }
    Ok(*frames)
}

fn validate_source(source: &NumericProperty, dimensions: usize) -> Result<(), PropertyError> {
    if !source.animated
        || source.expression_enabled
        || source.dimensions_separated
        || source.value_kind != NumericValueKind::Continuous
        || source.keyframes.len() < 2
        || source.keyframes.iter().any(|key| {
            key.values.len() != dimensions
                || !key.time_secs.is_finite()
                || key.values.iter().any(|value| !value.is_finite())
                || key
                    .spatial_in
                    .iter()
                    .chain(&key.spatial_out)
                    .any(|value| !value.is_finite() || value.abs() > 1.0e-12)
        })
        || source
            .keyframes
            .windows(2)
            .any(|keys| keys[1].time_secs <= keys[0].time_secs)
    {
        return Err(PropertyError::Layout(
            "unsupported delayed Position source curve or spatial motion",
        ));
    }
    Ok(())
}

fn moving_components(source: &NumericProperty) -> Vec<usize> {
    let first = &source.keyframes[0].values;
    (0..first.len())
        .filter(|component| {
            source
                .keyframes
                .iter()
                .any(|key| key.values[*component] != first[*component])
        })
        .collect()
}

pub(super) fn evaluate_position(
    source: &NumericProperty,
    composition_time: f64,
    start: f64,
    stretch: f64,
) -> Result<Vec<f64>, PropertyError> {
    let local_time = (composition_time - start) / stretch;
    let keys = &source.keyframes;
    if local_time <= keys[0].time_secs {
        return Ok(keys[0].values.clone());
    }
    // Callers validate strictly increasing keys. Exact key times select the
    // new value, including Hold segments, without a linear scan per fit sample.
    let index = keys.partition_point(|key| key.time_secs <= local_time) - 1;
    if index + 1 == keys.len() {
        return Ok(keys[index].values.clone());
    }
    let from = &keys[index];
    let to = &keys[index + 1];
    let duration = to.time_secs - from.time_secs;
    let progress = (local_time - from.time_secs) / duration;
    (0..from.values.len())
        .map(|component| {
            let easing = native_progress(from, to, component, duration, progress)?;
            let value =
                from.values[component] + (to.values[component] - from.values[component]) * easing;
            value
                .is_finite()
                .then_some(value)
                .ok_or(PropertyError::Layout(
                    "nonfinite delayed Position source value",
                ))
        })
        .collect()
}

fn native_progress(
    from: &NumericKeyframe,
    to: &NumericKeyframe,
    component: usize,
    duration: f64,
    progress: f64,
) -> Result<f64, PropertyError> {
    if from.out_interpolation == 3 {
        return Ok(0.0);
    }
    if from.out_interpolation == 1 && to.in_interpolation == 1 {
        return Ok(progress);
    }
    if !matches!(from.out_interpolation, 1 | 2) || !matches!(to.in_interpolation, 1 | 2) {
        return Err(PropertyError::Layout(
            "unknown delayed Position interpolation",
        ));
    }
    let spatial = !from.spatial_in.is_empty()
        || !from.spatial_out.is_empty()
        || !to.spatial_in.is_empty()
        || !to.spatial_out.is_empty();
    let value = |values: &[f64]| {
        if spatial {
            values.first().copied()
        } else {
            values.get(component).or_else(|| values.first()).copied()
        }
    };
    let out_influence = value(&from.out_influence).ok_or(PropertyError::Layout(
        "incomplete delayed Position temporal ease",
    ))?;
    let in_influence = value(&to.in_influence).ok_or(PropertyError::Layout(
        "incomplete delayed Position temporal ease",
    ))?;
    let out_speed = value(&from.out_speed).ok_or(PropertyError::Layout(
        "incomplete delayed Position temporal ease",
    ))?;
    let in_speed = value(&to.in_speed).ok_or(PropertyError::Layout(
        "incomplete delayed Position temporal ease",
    ))?;
    let delta = to.values[component] - from.values[component];
    let distance = if spatial {
        to.values
            .iter()
            .zip(&from.values)
            .map(|(to, from)| (to - from).powi(2))
            .sum::<f64>()
            .sqrt()
    } else {
        delta.abs()
    };
    if distance <= f64::EPSILON {
        if out_speed.abs() > f64::EPSILON || in_speed.abs() > f64::EPSILON {
            return Err(PropertyError::Layout(
                "equal-endpoint delayed Position segment has nonzero temporal speed",
            ));
        }
        return Ok(progress);
    }
    let x1 = (out_influence / 100.0).clamp(0.0, 1.0);
    let x2 = 1.0 - (in_influence / 100.0).clamp(0.0, 1.0);
    let normalization = if spatial { distance } else { delta };
    let y1 = if from.out_interpolation == 1 {
        x1
    } else {
        out_speed * duration / normalization * x1
    };
    let y2 = if to.in_interpolation == 1 {
        x2
    } else {
        1.0 - in_speed * duration / normalization * (1.0 - x2)
    };
    if [x1, y1, x2, y2].into_iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::Layout(
            "nonfinite delayed Position temporal ease",
        ));
    }
    Ok(cubic_bezier_progress(progress, x1, y1, x2, y2))
}

fn ease_out(time: f64, start: f64, end: f64, from: f64, to: f64) -> f64 {
    if time <= start {
        return from;
    }
    if time >= end {
        return to;
    }
    let progress = (time - start) / (end - start);
    let eased = ae_ease_out_progress(progress);
    from + (to - from) * eased
}

/// AE's `easeOut` is a cubic Hermite with unit start slope and zero end slope.
/// Independently measured from AE 26.5 `easeOut()` readback: 0.109 at 0.1,
/// 0.232 at 0.2 and 0.625 at 0.5 (the `expression_apis` native fixture of #5063).
fn ae_ease_out_progress(progress: f64) -> f64 {
    let p = progress.clamp(0.0, 1.0);
    p * p * (3.0 - 2.0 * p) + p * (1.0 - p) * (1.0 - p)
}

fn cubic_bezier_progress(progress: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..48 {
        let parameter = (low + high) * 0.5;
        let x = cubic(parameter, x1, x2);
        if x < progress {
            low = parameter;
        } else {
            high = parameter;
        }
    }
    cubic((low + high) * 0.5, y1, y2)
}

fn cubic(parameter: f64, first: f64, second: f64) -> f64 {
    let inverse = 1.0 - parameter;
    3.0 * inverse * inverse * parameter * first
        + 3.0 * inverse * parameter * parameter * second
        + parameter * parameter * parameter
}

fn fit_curve(
    duration_ms: u64,
    evaluate: impl FnMut(u64) -> Result<f64, PropertyError>,
) -> Result<FittedCurve, PropertyError> {
    fit_scalar_curve::<PropertyError>(duration_ms, FIT_TOLERANCE_PIXELS * 0.25, evaluate)
}

fn validate_fit(
    curve: &FittedCurve,
    duration_ms: u64,
    evaluate: &mut impl FnMut(u64) -> Result<f64, PropertyError>,
) -> Result<(), PropertyError> {
    let mut segment = 0;
    for time in 0..=duration_ms {
        let expected = evaluate(time)?;
        let actual = fitted_value(curve, time, &mut segment);
        if !actual.is_finite() || (actual - expected).abs() > FIT_TOLERANCE_PIXELS + 1.0e-9 {
            return Err(PropertyError::Layout(
                "delayed Position fit exceeds 0.01 pixel tolerance",
            ));
        }
    }
    Ok(())
}

pub(super) fn fitted_value(curve: &FittedCurve, time: u64, segment: &mut usize) -> f64 {
    if time <= curve.keys[0].offset_ms {
        return curve.keys[0].value;
    }
    while *segment + 1 < curve.keys.len() && curve.keys[*segment + 1].offset_ms <= time {
        *segment += 1;
    }
    let first = &curve.keys[*segment];
    let Some(next) = curve.keys.get(*segment + 1) else {
        return first.value;
    };
    let progress = (time - first.offset_ms) as f64 / (next.offset_ms - first.offset_ms) as f64;
    first.value + (next.value - first.value) * next.easing.progress(progress.clamp(0.0, 1.0))
}

pub(super) fn fitted_property(
    curve: &FittedCurve,
    component: usize,
    active_start: f64,
    owner_start: f64,
    owner_stretch: f64,
    evaluate: &mut impl FnMut(u64) -> Result<Vec<f64>, PropertyError>,
) -> Result<NumericProperty, PropertyError> {
    let mut keyframes = Vec::with_capacity(curve.keys.len());
    for (index, key) in curve.keys.iter().enumerate() {
        let mut values = evaluate(key.offset_ms)?;
        let dimensions = values.len();
        values[component] = key.value;
        let time_secs =
            (active_start + key.offset_ms as f64 / 1_000.0 - owner_start) / owner_stretch;
        keyframes.push(NumericKeyframe {
            time_secs,
            values,
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: vec![0.0; dimensions],
            in_influence: vec![33.333_333_333; dimensions],
            out_speed: vec![0.0; dimensions],
            out_influence: vec![33.333_333_333; dimensions],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        });
        if index > 0 {
            apply_easing(&mut keyframes, index, key.easing)?;
        }
    }
    Ok(NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes,
        value_kind: NumericValueKind::Continuous,
    })
}

fn validate_output(
    output: &NumericProperty,
    duration_ms: u64,
    active_start: f64,
    owner_start: f64,
    owner_stretch: f64,
    evaluate: &mut impl FnMut(u64) -> Result<Vec<f64>, PropertyError>,
) -> Result<(), PropertyError> {
    for offset_ms in 0..=duration_ms {
        let expected = evaluate(offset_ms)?;
        let actual = evaluate_position(
            output,
            active_start + offset_ms as f64 / 1_000.0,
            owner_start,
            owner_stretch,
        )?;
        if actual.len() != expected.len()
            || actual
                .iter()
                .zip(expected)
                .any(|(actual, expected)| (actual - expected).abs() > FIT_TOLERANCE_PIXELS + 1.0e-9)
        {
            return Err(PropertyError::Layout(
                "delayed Position vector fit exceeds 0.01 pixel tolerance",
            ));
        }
    }
    Ok(())
}

fn apply_easing(
    keys: &mut [NumericKeyframe],
    index: usize,
    easing: FittedEasing,
) -> Result<(), PropertyError> {
    let (before, after) = keys.split_at_mut(index);
    let previous = &mut before[index - 1];
    let current = &mut after[0];
    match easing {
        FittedEasing::Hold => previous.out_interpolation = 3,
        FittedEasing::Linear => {}
        FittedEasing::Cubic { y1, y2 } => {
            previous.out_interpolation = 2;
            current.in_interpolation = 2;
            let duration = current.time_secs - previous.time_secs;
            if duration <= 0.0 || !duration.is_finite() {
                return Err(PropertyError::Layout(
                    "invalid delayed Position fitted segment",
                ));
            }
            for component in 0..previous.values.len() {
                let delta = current.values[component] - previous.values[component];
                if !delta.is_finite() {
                    return Err(PropertyError::Layout(
                        "invalid delayed Position fitted segment",
                    ));
                }
                previous.out_speed[component] = y1 * delta / (duration / 3.0);
                current.in_speed[component] = (1.0 - y2) * delta / (duration / 3.0);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
