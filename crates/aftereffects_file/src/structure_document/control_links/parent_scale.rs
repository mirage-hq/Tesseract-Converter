//! Exact, bounded lowering of AE's stock inverse-parent-Scale expression.
//!
//! This module resolves only the owner's direct native parent. It never follows
//! parent expressions recursively and never removes the parent transform: the
//! independently keyed child Scale cancels visual scaling while preserving the
//! parent's translation and rotation in AE transform order.

use fx_keyframe_bake::curve_fit::{FittedCurve, fit_scalar_curve};

use super::{
    delayed_position::{evaluate_position, fitted_property},
    expression, properties, read_transform, split_scale, token, unique_run,
};
use crate::{
    properties::{NumericKeyframe, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

const EFFECTIVE_TOLERANCE: f64 = 0.0001;
const FIT_HEADROOM: f64 = 0.25;
const MIN_PARENT_MAGNITUDE: f64 = 1.0e-8;
const MAX_CHILD_MAGNITUDE: f64 = 1.0e8;
const MAX_PARENT_RANGE_PER_PARTITION: f64 = 2.0;

pub(super) struct LoweredParentScale {
    pub(super) axes: [NumericProperty; 2],
    pub(super) warnings: Vec<String>,
}

/// Lowers the exact stock parent-Scale cancellation expression, when present.
///
/// `None` means the expression is outside this grammar. A recognized expression
/// returns either complete X/Y numeric properties or one contextual rejection;
/// no partial axis is published.
pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Option<Result<LoweredParentScale, PropertyError>> {
    let text = scale_expression(layer).ok()?;
    if !parse(text) {
        return None;
    }
    Some(lower_recognized(layer, composition, base))
}

fn scale_expression(layer: &Layer) -> Result<&str, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let scale = properties::unique_list(unique_run(&leaves, "ADBE Scale")?, *b"tdbs")?;
    expression(scale)
}

fn optional_semicolon(text: &mut &str) {
    let _ = token(text, ";");
}

fn parse(mut text: &str) -> bool {
    let parsed = (|| {
        token(&mut text, "s")?;
        token(&mut text, "=")?;
        token(&mut text, "[")?;
        token(&mut text, "]")?;
        optional_semicolon(&mut text);

        token(&mut text, "ps")?;
        token(&mut text, "=")?;
        for part in ["parent", ".", "transform", ".", "scale", ".", "value"] {
            token(&mut text, part)?;
        }
        optional_semicolon(&mut text);

        token(&mut text, "for")?;
        token(&mut text, "(")?;
        token(&mut text, "i")?;
        token(&mut text, "=")?;
        token(&mut text, "0")?;
        token(&mut text, ";")?;
        token(&mut text, "i")?;
        token(&mut text, "<")?;
        token(&mut text, "ps")?;
        token(&mut text, ".")?;
        token(&mut text, "length")?;
        token(&mut text, ";")?;
        token(&mut text, "i")?;
        token(&mut text, "++")?;
        token(&mut text, ")")?;
        token(&mut text, "{")?;
        token(&mut text, "s")?;
        token(&mut text, "[")?;
        token(&mut text, "i")?;
        token(&mut text, "]")?;
        token(&mut text, "=")?;
        token(&mut text, "value")?;
        token(&mut text, "[")?;
        token(&mut text, "i")?;
        token(&mut text, "]")?;
        token(&mut text, "*")?;
        token(&mut text, "100")?;
        token(&mut text, "/")?;
        token(&mut text, "ps")?;
        token(&mut text, "[")?;
        token(&mut text, "i")?;
        token(&mut text, "]")?;
        optional_semicolon(&mut text);
        token(&mut text, "}")?;
        optional_semicolon(&mut text);
        token(&mut text, "s")?;
        optional_semicolon(&mut text);
        text.trim().is_empty().then_some(())
    })();
    parsed.is_some()
}

fn lower_recognized(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Result<LoweredParentScale, PropertyError> {
    validate_owner(layer, base)?;
    let parent = direct_parent(layer, composition)?;
    if parent.record.flags().three_d_layer {
        return Err(PropertyError::Layout(
            "parent Scale cancellation requires a 2D parent",
        ));
    }
    let (owner_start, owner_stretch, active_start, active_end) = owner_clock(layer)?;
    let (parent_start, parent_stretch) = source_clock(parent)?;
    if parent_stretch != owner_stretch {
        return Err(PropertyError::Layout(
            "parent Scale cancellation requires equal positive stretches",
        ));
    }

    let parent_axes = resolved_parent_axes(parent, composition)?;
    // Transform ancestors affect their children even outside their own paint
    // lifetime. Include the parent's complete animated range, not just this
    // guide's in/out points; endpoint extrapolation then remains valid too.
    let (active_start, active_end) = parent_axes
        .iter()
        .flat_map(|axis| &axis.keyframes)
        .map(|key| parent_start + key.time_secs * parent_stretch)
        .fold((active_start, active_end), |(start, end), time| {
            (start.min(time), end.max(time))
        });
    let child_values = [base.values[0], base.values[1]];
    let clock = AxisClock {
        duration: bounded_duration(active_start, active_end)?,
        active_start,
        owner_start,
        owner_stretch,
        parent_start,
        parent_stretch,
    };
    let axes = [0, 1].map(|axis| lower_axis(&parent_axes[axis], child_values[axis], clock));
    let [x, y] = axes;
    let axes = [x?, y?];
    Ok(LoweredParentScale {
        axes,
        warnings: vec![
            "ADBE Scale: exact parent-Scale cancellation lowered to bounded independent editable X/Y values/keys; the direct parent transform remains intact and live linkage is not retained; static cancellation is exact, while animated fits are checked for effective parent × child Scale error within 0.01 percentage points on a 1ms analytical grid before FX millisecond quantization"
                .into(),
        ],
    })
}

fn validate_owner(layer: &Layer, base: &NumericProperty) -> Result<(), PropertyError> {
    if layer.record.flags().three_d_layer
        || !base.expression_enabled
        || base.animated
        || !base.keyframes.is_empty()
        || base.dimensions_separated
        || base.value_kind != NumericValueKind::Continuous
        || !(2..=3).contains(&base.values.len())
        || base.values.iter().any(|value| !value.is_finite())
    {
        return Err(PropertyError::Layout(
            "parent Scale cancellation requires a finite static 2D child Scale",
        ));
    }
    Ok(())
}

fn direct_parent<'a>(
    layer: &Layer,
    composition: &'a Composition,
) -> Result<&'a Layer, PropertyError> {
    let parent_id = layer.record.parent_id();
    if parent_id == 0 || parent_id == layer.record.id() {
        return Err(PropertyError::Layout(
            "parent Scale cancellation requires a noncyclic direct parent ID",
        ));
    }
    let mut matches = composition
        .layers
        .iter()
        .filter(|candidate| candidate.record.id() == parent_id);
    let parent = matches
        .next()
        .ok_or(PropertyError::Layout("direct Scale parent is missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("direct Scale parent ID is ambiguous"));
    }
    Ok(parent)
}

fn source_clock(layer: &Layer) -> Result<(f64, f64), PropertyError> {
    let (Some(start), Some(stretch)) = (layer.record.start_time(), layer.record.stretch()) else {
        return Err(PropertyError::Layout("invalid parent Scale source clock"));
    };
    if !start.is_finite() || !stretch.is_finite() || stretch <= 0.0 {
        return Err(PropertyError::Layout("invalid parent Scale source clock"));
    }
    Ok((start, stretch))
}

fn owner_clock(layer: &Layer) -> Result<(f64, f64, f64, f64), PropertyError> {
    let (start, stretch) = source_clock(layer)?;
    let (Some(in_point), Some(out_point)) = (layer.record.in_point(), layer.record.out_point())
    else {
        return Err(PropertyError::Layout(
            "invalid parent Scale owner active range",
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
            "invalid parent Scale owner active range",
        ));
    }
    Ok((start, stretch, active_start, active_end))
}

fn bounded_duration(active_start: f64, active_end: f64) -> Result<u64, PropertyError> {
    let duration = ((active_end - active_start) * 1_000.0).ceil();
    if !duration.is_finite() || duration < 1.0 || duration >= u64::MAX as f64 {
        return Err(PropertyError::Layout(
            "parent Scale cancellation duration is not representable in milliseconds",
        ));
    }
    // The range check above proves this is a positive integer within `u64`.
    Ok(duration as u64)
}

fn resolved_parent_axes(
    parent: &Layer,
    composition: &Composition,
) -> Result<[NumericProperty; 2], PropertyError> {
    let (properties, _) = read_transform(&parent.content)?;
    let scale = properties
        .iter()
        .find(|property| property.match_name == "ADBE Scale")
        .ok_or(PropertyError::Layout("direct parent Scale is missing"))?
        .numeric
        .as_ref()
        .map_err(Clone::clone)?;
    if scale.expression_enabled {
        let axes = split_scale(parent, composition)?;
        for axis in &axes {
            validate_nonsingular_curve(axis)?;
        }
        return Ok(axes);
    }
    Ok([project_axis(scale, 0)?, project_axis(scale, 1)?])
}

fn project_axis(
    source: &NumericProperty,
    component: usize,
) -> Result<NumericProperty, PropertyError> {
    if source.expression_enabled
        || source.dimensions_separated
        || source.value_kind != NumericValueKind::Continuous
        || (!source.animated && !source.keyframes.is_empty())
        || (source.animated && source.keyframes.is_empty())
    {
        return Err(PropertyError::Layout(
            "unsupported direct parent Scale property",
        ));
    }
    let mut result = source.clone();
    result.values = project_required(&source.values, component, source.animated)?;
    for key in &mut result.keyframes {
        if !key.time_secs.is_finite() || !key.spatial_in.is_empty() || !key.spatial_out.is_empty() {
            return Err(PropertyError::Layout("invalid direct parent Scale key"));
        }
        key.values = project_required(&key.values, component, false)?;
        key.in_speed = project_optional(&key.in_speed, component)?;
        key.in_influence = project_optional(&key.in_influence, component)?;
        key.out_speed = project_optional(&key.out_speed, component)?;
        key.out_influence = project_optional(&key.out_influence, component)?;
    }
    if result
        .keyframes
        .windows(2)
        .any(|keys| keys[1].time_secs <= keys[0].time_secs)
    {
        return Err(PropertyError::Layout(
            "direct parent Scale keys are not strictly increasing",
        ));
    }
    validate_nonsingular_curve(&result)?;
    Ok(result)
}

fn project_required(
    values: &[f64],
    component: usize,
    allow_empty: bool,
) -> Result<Vec<f64>, PropertyError> {
    if values.is_empty() && allow_empty {
        return Ok(Vec::new());
    }
    if !(2..=3).contains(&values.len()) || values.iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::Layout(
            "direct parent Scale is not a finite 2D value",
        ));
    }
    Ok(vec![values[component]])
}

fn project_optional(values: &[f64], component: usize) -> Result<Vec<f64>, PropertyError> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let value = values
        .get(component)
        .or_else(|| values.first())
        .copied()
        .filter(|value| value.is_finite())
        .ok_or(PropertyError::Layout(
            "invalid direct parent Scale temporal ease",
        ))?;
    Ok(vec![value])
}

fn validate_nonsingular_curve(source: &NumericProperty) -> Result<(), PropertyError> {
    if source.expression_enabled
        || source.dimensions_separated
        || source.value_kind != NumericValueKind::Continuous
        || (!source.animated && !source.keyframes.is_empty())
        || (source.animated && source.keyframes.is_empty())
    {
        return Err(PropertyError::Layout(
            "unsupported direct parent Scale axis",
        ));
    }
    if !source.animated {
        let [value] = source.values.as_slice() else {
            return Err(PropertyError::Layout(
                "direct parent Scale axis is not scalar",
            ));
        };
        return ensure_safe_parent(*value);
    }
    if source.keyframes.iter().any(|key| {
        !key.time_secs.is_finite()
            || key.values.len() != 1
            || !key.spatial_in.is_empty()
            || !key.spatial_out.is_empty()
    }) || source
        .keyframes
        .windows(2)
        .any(|keys| keys[1].time_secs <= keys[0].time_secs)
    {
        return Err(PropertyError::Layout(
            "invalid direct parent Scale axis keys",
        ));
    }
    for key in &source.keyframes {
        ensure_safe_parent(key.values[0])?;
    }
    for pair in source.keyframes.windows(2) {
        validate_segment(&pair[0], &pair[1])?;
    }
    Ok(())
}

fn validate_segment(from: &NumericKeyframe, to: &NumericKeyframe) -> Result<(), PropertyError> {
    let from_value = from.values[0];
    let to_value = to.values[0];
    if from_value.signum() != to_value.signum() {
        return Err(PropertyError::Layout("direct parent Scale crosses zero"));
    }
    if from.out_interpolation == 3 || (from.out_interpolation == 1 && to.in_interpolation == 1) {
        return Ok(());
    }
    if !matches!(from.out_interpolation, 1 | 2) || !matches!(to.in_interpolation, 1 | 2) {
        return Err(PropertyError::Layout(
            "unknown direct parent Scale interpolation",
        ));
    }
    let duration = to.time_secs - from.time_secs;
    let delta = to_value - from_value;
    if delta.abs() <= f64::EPSILON {
        let speeds = [first(&from.out_speed)?, first(&to.in_speed)?];
        if speeds.into_iter().any(|speed| speed.abs() > f64::EPSILON) {
            return Err(PropertyError::Layout(
                "equal parent Scale endpoints have nonzero temporal speed",
            ));
        }
        return Ok(());
    }
    let out_influence = first(&from.out_influence)?;
    let in_influence = first(&to.in_influence)?;
    let out_speed = first(&from.out_speed)?;
    let in_speed = first(&to.in_speed)?;
    let x1 = (out_influence / 100.0).clamp(0.0, 1.0);
    let x2 = 1.0 - (in_influence / 100.0).clamp(0.0, 1.0);
    let y1 = if from.out_interpolation == 1 {
        x1
    } else {
        out_speed * duration / delta * x1
    };
    let y2 = if to.in_interpolation == 1 {
        x2
    } else {
        1.0 - in_speed * duration / delta * (1.0 - x2)
    };
    let controls = [
        from_value,
        from_value + delta * y1,
        from_value + delta * y2,
        to_value,
    ];
    if controls.iter().any(|value| {
        !value.is_finite()
            || value.abs() < MIN_PARENT_MAGNITUDE
            || value.signum() != from_value.signum()
    }) {
        return Err(PropertyError::Layout(
            "direct parent Scale easing can overshoot through a singular value",
        ));
    }
    Ok(())
}

fn first(values: &[f64]) -> Result<f64, PropertyError> {
    let [value] = values else {
        return Err(PropertyError::Layout(
            "incomplete direct parent Scale temporal ease",
        ));
    };
    value
        .is_finite()
        .then_some(*value)
        .ok_or(PropertyError::Layout(
            "nonfinite direct parent Scale temporal ease",
        ))
}

fn ensure_safe_parent(value: f64) -> Result<(), PropertyError> {
    if !value.is_finite() || value.abs() < MIN_PARENT_MAGNITUDE {
        return Err(PropertyError::Layout(
            "direct parent Scale is singular or nonfinite",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct AxisClock {
    duration: u64,
    active_start: f64,
    owner_start: f64,
    owner_stretch: f64,
    parent_start: f64,
    parent_stretch: f64,
}

fn lower_axis(
    parent: &NumericProperty,
    child_value: f64,
    clock: AxisClock,
) -> Result<NumericProperty, PropertyError> {
    if !parent.animated {
        let parent_value = parent
            .values
            .first()
            .copied()
            .ok_or(PropertyError::Layout("parent Scale axis is missing"))?;
        let value = reciprocal(child_value, parent_value)?;
        if (parent_value * value - child_value).abs() > EFFECTIVE_TOLERANCE {
            return Err(PropertyError::Layout(
                "static parent Scale cancellation exceeds the effective tolerance",
            ));
        }
        return Ok(static_axis(value));
    }

    let mut parent_samples = Vec::with_capacity(
        usize::try_from(clock.duration)
            .ok()
            .and_then(|duration| duration.checked_add(1))
            .ok_or(PropertyError::Layout("parent Scale sample bound overflow"))?,
    );
    let mut maximum_parent: f64 = 0.0;
    for offset in 0..=clock.duration {
        let composition_time = clock.active_start + offset as f64 / 1_000.0;
        let value = evaluate_position(
            parent,
            composition_time,
            clock.parent_start,
            clock.parent_stretch,
        )?
        .first()
        .copied()
        .ok_or(PropertyError::Layout("parent Scale axis is missing"))?;
        ensure_safe_parent(value)?;
        maximum_parent = maximum_parent.max(value.abs());
        parent_samples.push(value);
    }
    if !maximum_parent.is_finite() || maximum_parent <= 0.0 {
        return Err(PropertyError::Layout("invalid parent Scale sample range"));
    }
    let target = |offset: u64| -> Result<f64, PropertyError> {
        let index = usize::try_from(offset)
            .map_err(|_| PropertyError::Layout("parent Scale sample offset overflow"))?;
        reciprocal(child_value, parent_samples[index])
    };
    let curve = fit_weighted_reciprocal(&parent_samples, &target)?;
    let mut values = |offset| target(offset).map(|value| vec![value]);
    let output = fitted_property(
        &curve,
        0,
        clock.active_start,
        clock.owner_start,
        clock.owner_stretch,
        &mut values,
    )?;
    validate_effective_product(
        &output,
        &parent_samples,
        child_value,
        clock.active_start,
        clock.owner_start,
        clock.owner_stretch,
    )?;
    Ok(output)
}

// A single absolute reciprocal tolerance is pathological near the native
// 0.001% key: it would demand the same tiny child error at 100,000× Scale as
// at identity. Partition where parent magnitude changes by more than 2×, fit
// each bounded interval with a locally sufficient absolute tolerance, then
// verify the composed parent × child result over the unchanged full grid.
fn fit_weighted_reciprocal(
    parent_samples: &[f64],
    target: &impl Fn(u64) -> Result<f64, PropertyError>,
) -> Result<FittedCurve, PropertyError> {
    let boundaries = fitting_boundaries(parent_samples)?;
    let mut keys: Vec<fx_keyframe_bake::curve_fit::FittedKey> = Vec::new();
    let mut minimum_tolerance = f64::INFINITY;
    for boundary in boundaries.windows(2) {
        let start = boundary[0];
        let end = boundary[1];
        let maximum_parent = parent_samples[start..=end]
            .iter()
            .map(|value| value.abs())
            .fold(0.0_f64, f64::max);
        let tolerance = EFFECTIVE_TOLERANCE / maximum_parent * FIT_HEADROOM;
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(PropertyError::Layout("invalid parent Scale fit tolerance"));
        }
        minimum_tolerance = minimum_tolerance.min(tolerance);
        let duration = u64::try_from(end - start)
            .map_err(|_| PropertyError::Layout("parent Scale partition overflow"))?;
        let start_offset = u64::try_from(start)
            .map_err(|_| PropertyError::Layout("parent Scale partition overflow"))?;
        let curve = fit_scalar_curve(duration, tolerance, |offset| {
            let offset = start_offset
                .checked_add(offset)
                .ok_or(PropertyError::Layout("parent Scale sample offset overflow"))?;
            target(offset)
        })?;
        for mut key in curve.keys {
            key.offset_ms = key
                .offset_ms
                .checked_add(start_offset)
                .ok_or(PropertyError::Layout("parent Scale key offset overflow"))?;
            if let Some(previous) = keys.last()
                && previous.offset_ms == key.offset_ms
            {
                if previous.value != key.value {
                    return Err(PropertyError::Layout(
                        "parent Scale partitions disagree at their boundary",
                    ));
                }
                continue;
            }
            keys.push(key);
        }
    }
    if keys.is_empty() || !minimum_tolerance.is_finite() {
        return Err(PropertyError::Layout("parent Scale fit produced no keys"));
    }
    Ok(FittedCurve {
        keys,
        tolerance: minimum_tolerance,
    })
}

fn fitting_boundaries(parent_samples: &[f64]) -> Result<Vec<usize>, PropertyError> {
    let first = parent_samples
        .first()
        .copied()
        .ok_or(PropertyError::Layout("parent Scale fit has no samples"))?;
    let mut boundaries = vec![0];
    let mut minimum = first.abs();
    let mut maximum = minimum;
    for offset in 1..parent_samples.len() {
        let magnitude = parent_samples[offset].abs();
        let next_minimum = minimum.min(magnitude);
        let next_maximum = maximum.max(magnitude);
        if next_maximum / next_minimum > MAX_PARENT_RANGE_PER_PARTITION {
            let boundary = offset - 1;
            if boundaries.last().copied() != Some(boundary) {
                boundaries.push(boundary);
            }
            minimum = parent_samples[boundary].abs().min(magnitude);
            maximum = parent_samples[boundary].abs().max(magnitude);
        } else {
            minimum = next_minimum;
            maximum = next_maximum;
        }
    }
    let last = parent_samples
        .len()
        .checked_sub(1)
        .ok_or(PropertyError::Layout("parent Scale fit has no samples"))?;
    if boundaries.last().copied() != Some(last) {
        boundaries.push(last);
    }
    Ok(boundaries)
}

fn reciprocal(child: f64, parent: f64) -> Result<f64, PropertyError> {
    ensure_safe_parent(parent)?;
    let value = child / parent;
    if !value.is_finite() || value.abs() > MAX_CHILD_MAGNITUDE {
        return Err(PropertyError::Layout(
            "inverse parent Scale exceeds the finite child bound",
        ));
    }
    Ok(value)
}

fn static_axis(value: f64) -> NumericProperty {
    NumericProperty {
        values: vec![value],
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind: NumericValueKind::Continuous,
    }
}

fn validate_effective_product(
    output: &NumericProperty,
    parent_samples: &[f64],
    child_value: f64,
    active_start: f64,
    owner_start: f64,
    owner_stretch: f64,
) -> Result<(), PropertyError> {
    for (offset, parent) in parent_samples.iter().copied().enumerate() {
        let composition_time = active_start + offset as f64 / 1_000.0;
        let child = evaluate_position(output, composition_time, owner_start, owner_stretch)?
            .first()
            .copied()
            .ok_or(PropertyError::Layout("fitted child Scale axis is missing"))?;
        let error = (parent * child - child_value).abs();
        if !child.is_finite()
            || child.abs() > MAX_CHILD_MAGNITUDE
            || !error.is_finite()
            || error > EFFECTIVE_TOLERANCE + 1.0e-9
        {
            return Err(PropertyError::Layout(
                "effective parent × fitted child Scale exceeds 0.01 percentage-point tolerance",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
