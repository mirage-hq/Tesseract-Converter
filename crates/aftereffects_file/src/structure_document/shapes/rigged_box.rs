//! Bounded import of expression-driven Rectangle controls.
//!
//! This recognizes only two complete forms and never evaluates JavaScript: the
//! Rigged Box pseudo-effect's stock Size, Position, and Roundness formulas, and
//! plain Slider references (`control_links::slider_components`).

use crate::{
    effects::native,
    properties::{self, NumericKeyframe, NumericProperty, NumericValueKind, PropertyError},
    rifx::Chunk,
    structure::{Composition, Layer},
    structure_document::control_links,
};

#[cfg(test)]
#[path = "rigged_box/tests.rs"]
mod tests;

const EFFECT: &str = "Pseudo/PS Rigged Box";
const X_SIZE: &str = "Pseudo/PS Rigged Box-0001";
const Y_SIZE: &str = "Pseudo/PS Rigged Box-0002";
const ROUNDNESS: &str = "Pseudo/PS Rigged Box-0003";
const X_ANCHOR: &str = "Pseudo/PS Rigged Box-0004";
const Y_ANCHOR: &str = "Pseudo/PS Rigged Box-0005";

const SIZE_EXPRESSION: &str = r#"
e = effect("Rigged Box");
xSize = e(1);
ySize = e(2);
[xSize,ySize];
"#;
const POSITION_EXPRESSION: &str = r#"
e = effect("Rigged Box");
size = thisProperty.propertyGroup(1).size;
xAnchor = e(4) / -200;
yAnchor = e(5) / -200;
value + [xAnchor*size[0],yAnchor*size[1]];
"#;
const ROUNDNESS_EXPRESSION: &str = r#"
e = effect("Rigged Box");
e(3);
"#;

/// The recognized expression form that produced a Rectangle's [`Controls`].
pub(super) enum Profile {
    /// The stock Rigged Box formulas. `defaulted_x_anchor`: the sparse effect
    /// omitted its X Anchor slot, which is assumed to be 0.
    RiggedBox { defaulted_x_anchor: bool },
    /// Size, Position or Roundness expressions that are complete Slider
    /// references; see [`resolve_sliders`].
    Sliders,
}

/// Resolved native curves for Rectangle Size, Position and Roundness.
pub(super) struct Controls {
    pub(super) size: NumericProperty,
    pub(super) position: NumericProperty,
    pub(super) roundness: NumericProperty,
    pub(super) profile: Profile,
}

impl Controls {
    /// The resolved curve of one native Rectangle property.
    pub(super) fn curve(&self, property: &str) -> &NumericProperty {
        match property {
            "ADBE Vector Rect Size" => &self.size,
            "ADBE Vector Rect Position" => &self.position,
            _ => &self.roundness,
        }
    }
}

pub(super) fn resolve(
    layer: &Layer,
    leaves: &[(&str, &[Chunk])],
) -> Result<Controls, PropertyError> {
    require_expression(leaves, "ADBE Vector Rect Size", SIZE_EXPRESSION)?;
    require_expression(leaves, "ADBE Vector Rect Position", POSITION_EXPRESSION)?;
    require_expression(leaves, "ADBE Vector Rect Roundness", ROUNDNESS_EXPRESSION)?;
    validate_effect_binding(layer)?;

    let (effects, _) = native::read_effects(&layer.content, [0.0, 0.0]);
    let mut matches = effects.iter().filter(|effect| effect.match_name == EFFECT);
    let effect = matches
        .next()
        .filter(|effect| effect.enabled)
        .ok_or(PropertyError::Layout("enabled Rigged Box effect missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous Rigged Box effect"));
    }
    let x = control(effect, X_SIZE)?;
    let y = control(effect, Y_SIZE)?;
    let roundness = control(effect, ROUNDNESS)?;
    let (x_anchor, defaulted_x_anchor) = match optional_control(effect, X_ANCHOR)? {
        Some(numeric) => (static_scalar(numeric)?, false),
        None => {
            // Rigged Box 3.0 omits its unchanged X Anchor leaf from the layer-side
            // sparse pseudo-effect table. Assume zero only for this recognized
            // profile; the caller explicitly diagnoses this unverified default.
            (0.0, true)
        }
    };
    let y_anchor = static_scalar(control(effect, Y_ANCHOR)?)?;
    let size = merge_axes(x, y)?;
    if values(&size).any(|value| !value.is_finite() || value <= 0.0)
        || values(roundness).any(|value| !value.is_finite() || value < 0.0)
    {
        return Err(PropertyError::Layout("invalid Rigged Box geometry value"));
    }

    let base = property(leaves, "ADBE Vector Rect Position")?;
    if base.animated || base.values.len() != 2 || !base.keyframes.is_empty() {
        return Err(PropertyError::Layout(
            "Rigged Box Position requires one static base value",
        ));
    }
    let factors = [-x_anchor / 200.0, -y_anchor / 200.0];
    let mut position = size.clone();
    position.values = size
        .values
        .iter()
        .zip(base.values.iter())
        .zip(factors)
        .map(|((&value, &base), factor)| base + factor * value)
        .collect();
    for key in &mut position.keyframes {
        for ((value, &base), factor) in key.values.iter_mut().zip(&base.values).zip(factors) {
            *value = base + factor * *value;
        }
        for (speed, factor) in key
            .in_speed
            .iter_mut()
            .zip(factors)
            .chain(key.out_speed.iter_mut().zip(factors))
        {
            *speed *= factor;
        }
    }
    position.expression_enabled = false;
    position.expression_present = false;
    Ok(Controls {
        size,
        position,
        roundness: without_expression(roundness.clone()),
        profile: Profile::RiggedBox { defaulted_x_anchor },
    })
}

/// Resolves a Rectangle whose enabled Size, Position or Roundness expressions
/// are each a complete Slider reference, or for Size and Position an array of
/// two. Each component keeps the Slider's pixel values, keys and temporal
/// speeds, and two components merge only where their keys are compatible. A
/// property without an enabled expression keeps its native leaf or default.
/// Geometry validity stays with the typed Rect lowering that uses the curves.
pub(super) fn resolve_sliders(
    layer: &Layer,
    composition: &Composition,
    leaves: &[(&str, &[Chunk])],
) -> Result<Controls, PropertyError> {
    let mut linked = false;
    let mut curve = |name: &str, dimensions: usize| -> Result<NumericProperty, PropertyError> {
        let Some((_, run)) = leaves.iter().find(|(candidate, _)| *candidate == name) else {
            let values = super::super::defaults::numeric(name).ok_or(PropertyError::Layout(
                "Rectangle property has no native default",
            ))?;
            return Ok(NumericProperty {
                values: values.to_vec(),
                animated: false,
                expression_enabled: false,
                expression_present: false,
                dimensions_separated: false,
                keyframes: Vec::new(),
                value_kind: NumericValueKind::Continuous,
            });
        };
        let body = properties::unique_list(run, *b"tdbs")?;
        let native = properties::read_numeric(body)?;
        if !native.expression_enabled {
            return Ok(native);
        }
        linked = true;
        match control_links::slider_components(layer, composition, body, dimensions)?.as_slice() {
            [x, y] => merge_axes(x, y),
            [scalar] => {
                validate_axis(scalar)?;
                Ok(without_expression(scalar.clone()))
            }
            _ => Err(PropertyError::Layout(
                "unsupported Rectangle control dimensions",
            )),
        }
    };
    let size = curve("ADBE Vector Rect Size", 2)?;
    let position = curve("ADBE Vector Rect Position", 2)?;
    let roundness = curve("ADBE Vector Rect Roundness", 1)?;
    if !linked {
        return Err(PropertyError::Layout(
            "no Rectangle property has an enabled expression",
        ));
    }
    Ok(Controls {
        size,
        position,
        roundness,
        profile: Profile::Sliders,
    })
}

pub(super) fn initial(numeric: &NumericProperty, dimensions: usize) -> Option<Vec<f64>> {
    let values = numeric
        .keyframes
        .first()
        .map_or(numeric.values.as_slice(), |key| key.values.as_slice());
    (values.len() == dimensions).then(|| values.to_vec())
}

fn validate_effect_binding(layer: &Layer) -> Result<(), PropertyError> {
    let roots = properties::root_runs(&layer.content)?;
    let parade = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Parade")
        .ok_or(PropertyError::Layout("Rigged Box Effect Parade missing"))?
        .1;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let mut rigged_boxes = 0;
    for (match_name, run) in effects {
        let plugin = properties::unique_list(run, *b"sspc")?;
        let body = properties::unique_list(plugin, *b"tdgp")?;
        let display = effect_display_name(body)?;
        if match_name == EFFECT {
            rigged_boxes += 1;
            // `-_0_/-` is AE's native sentinel for this unrenamed pseudo effect;
            // an explicit different display name cannot satisfy effect("Rigged Box").
            if !matches!(display, "-_0_/-" | "Rigged Box") {
                return Err(PropertyError::Layout("Rigged Box effect was renamed"));
            }
        } else if display == "Rigged Box" {
            return Err(PropertyError::Layout(
                "Rigged Box expression display name is ambiguous",
            ));
        }
    }
    if rigged_boxes != 1 {
        return Err(PropertyError::Layout("ambiguous Rigged Box effect"));
    }
    Ok(())
}

fn effect_display_name(chunks: &[Chunk]) -> Result<&str, PropertyError> {
    let bytes = properties::data(chunks, *b"tdsn")?;
    if bytes.get(..4) != Some(b"Utf8") {
        return Err(PropertyError::Layout("Rigged Box display name encoding"));
    }
    let length = usize::try_from(u32::from_be_bytes(
        bytes
            .get(4..8)
            .ok_or(PropertyError::Layout("Rigged Box display name length"))?
            .try_into()
            .map_err(|_| PropertyError::Layout("Rigged Box display name length"))?,
    ))
    .map_err(|_| PropertyError::Layout("Rigged Box display name length"))?;
    let end = 8_usize
        .checked_add(length)
        .ok_or(PropertyError::Layout("Rigged Box display name length"))?;
    let padding = bytes
        .get(end..)
        .ok_or(PropertyError::Layout("Rigged Box display name payload"))?;
    if padding.len() > 3 || padding.iter().any(|byte| *byte != 0) {
        return Err(PropertyError::Layout("Rigged Box display name padding"));
    }
    std::str::from_utf8(
        bytes
            .get(8..end)
            .ok_or(PropertyError::Layout("Rigged Box display name payload"))?,
    )
    .map_err(|_| PropertyError::Layout("Rigged Box display name UTF-8"))
}

fn require_expression(
    leaves: &[(&str, &[Chunk])],
    name: &str,
    expected: &str,
) -> Result<(), PropertyError> {
    let run = leaves
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .ok_or(PropertyError::Layout(
            "Rigged Box Rectangle property missing",
        ))?
        .1;
    let body = properties::unique_list(run, *b"tdbs")?;
    let numeric = properties::read_numeric(body)?;
    if !numeric.expression_enabled {
        return Err(PropertyError::Layout(
            "Rigged Box expression is not enabled",
        ));
    }
    let expression = std::str::from_utf8(properties::data(body, *b"Utf8")?)
        .map_err(|_| PropertyError::Layout("invalid Rigged Box expression text"))?;
    if canonical(expression) != canonical(expected) {
        return Err(PropertyError::Layout("unrecognized Rigged Box expression"));
    }
    Ok(())
}

fn canonical(expression: &str) -> String {
    expression
        .split(['\r', '\n'])
        .map(|line| line.split("//").next().unwrap_or_default().trim())
        .collect()
}

fn property(leaves: &[(&str, &[Chunk])], name: &str) -> Result<NumericProperty, PropertyError> {
    let run = leaves
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .ok_or(PropertyError::Layout(
            "Rigged Box Rectangle property missing",
        ))?
        .1;
    properties::read_numeric(properties::unique_list(run, *b"tdbs")?)
}

fn control<'a>(
    effect: &'a native::DecodedEffect,
    name: &str,
) -> Result<&'a NumericProperty, PropertyError> {
    optional_control(effect, name)?.ok_or(PropertyError::Layout("Rigged Box control missing"))
}

fn optional_control<'a>(
    effect: &'a native::DecodedEffect,
    name: &str,
) -> Result<Option<&'a NumericProperty>, PropertyError> {
    let mut matches = effect
        .parameters
        .iter()
        .filter(|parameter| parameter.match_name == name);
    let Some(parameter) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous Rigged Box control"));
    }
    parameter.numeric.as_ref().map(Some).map_err(Clone::clone)
}

fn static_scalar(numeric: &NumericProperty) -> Result<f64, PropertyError> {
    if numeric.expression_enabled
        || numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.values.len() != 1
        || numeric.value_kind != NumericValueKind::Continuous
    {
        return Err(PropertyError::Layout(
            "Rigged Box anchor must be one static scalar",
        ));
    }
    Ok(numeric.values[0])
}

fn merge_axes(x: &NumericProperty, y: &NumericProperty) -> Result<NumericProperty, PropertyError> {
    validate_axis(x)?;
    validate_axis(y)?;
    let template = if is_time_superset(x, y) {
        x
    } else if is_time_superset(y, x) {
        y
    } else {
        return Err(PropertyError::Layout(
            "Rectangle control axes have incompatible key times",
        ));
    };
    let values = match (static_numeric(x), static_numeric(y)) {
        (Some(x), Some(y)) => vec![x.values[0], y.values[0]],
        _ => Vec::new(),
    };
    let mut keys = Vec::with_capacity(template.keyframes.len());
    for key in &template.keyframes {
        keys.push(merge_key(key.time_secs, x, y)?);
    }
    for index in 1..keys.len() {
        let x_moves = keys[index - 1].values[0] != keys[index].values[0];
        let y_moves = keys[index - 1].values[1] != keys[index].values[1];
        let from_x = scalar_key_at(x, keys[index - 1].time_secs)?;
        let to_x = scalar_key_at(x, keys[index].time_secs)?;
        let from_y = scalar_key_at(y, keys[index - 1].time_secs)?;
        let to_y = scalar_key_at(y, keys[index].time_secs)?;
        if x_moves
            && y_moves
            && (from_x.out_interpolation != from_y.out_interpolation
                || to_x.in_interpolation != to_y.in_interpolation)
        {
            return Err(PropertyError::Layout(
                "Rectangle control axes have incompatible interpolation modes",
            ));
        }
        let (from, to) = if x_moves {
            (&from_x, &to_x)
        } else {
            (&from_y, &to_y)
        };
        keys[index - 1].out_interpolation = from.out_interpolation;
        keys[index].in_interpolation = to.in_interpolation;
    }
    Ok(NumericProperty {
        values,
        animated: !keys.is_empty(),
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: keys,
        value_kind: NumericValueKind::Continuous,
    })
}

fn static_numeric(numeric: &NumericProperty) -> Option<&NumericProperty> {
    (!numeric.animated && numeric.keyframes.is_empty() && numeric.values.len() == 1)
        .then_some(numeric)
}

fn validate_axis(numeric: &NumericProperty) -> Result<(), PropertyError> {
    if numeric.expression_enabled
        || numeric.dimensions_separated
        || numeric.value_kind != NumericValueKind::Continuous
        || (!numeric.values.is_empty() && numeric.values.len() != 1)
        || numeric.keyframes.len() > 4096
        || numeric.keyframes.iter().any(|key| {
            key.values.len() != 1
                || !key.time_secs.is_finite()
                || key.in_speed.len() != 1
                || key.out_speed.len() != 1
                || key.in_influence.len() != 1
                || key.out_influence.len() != 1
                || !key.spatial_in.is_empty()
                || !key.spatial_out.is_empty()
        })
        || numeric.keyframes.windows(2).any(|pair| {
            pair[0].time_secs >= pair[1].time_secs
                || (pair[0].values == pair[1].values
                    && ((pair[0].out_interpolation == 2 && pair[0].out_speed[0] != 0.0)
                        || (pair[1].in_interpolation == 2 && pair[1].in_speed[0] != 0.0)))
        })
    {
        return Err(PropertyError::Layout(
            "unsupported Rectangle control scalar curve",
        ));
    }
    Ok(())
}

fn is_time_superset(candidate: &NumericProperty, other: &NumericProperty) -> bool {
    other.keyframes.iter().all(|key| {
        candidate
            .keyframes
            .iter()
            .any(|candidate| candidate.time_secs == key.time_secs)
    })
}

fn merge_key(
    time: f64,
    x: &NumericProperty,
    y: &NumericProperty,
) -> Result<NumericKeyframe, PropertyError> {
    let x = scalar_key_at(x, time)?;
    let y = scalar_key_at(y, time)?;
    Ok(NumericKeyframe {
        time_secs: time,
        values: vec![x.values[0], y.values[0]],
        in_interpolation: y.in_interpolation,
        out_interpolation: y.out_interpolation,
        in_speed: vec![x.in_speed[0], y.in_speed[0]],
        in_influence: vec![x.in_influence[0], y.in_influence[0]],
        out_speed: vec![x.out_speed[0], y.out_speed[0]],
        out_influence: vec![x.out_influence[0], y.out_influence[0]],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    })
}

fn scalar_key_at(numeric: &NumericProperty, time: f64) -> Result<NumericKeyframe, PropertyError> {
    if let Some(key) = numeric.keyframes.iter().find(|key| key.time_secs == time) {
        return Ok(key.clone());
    }
    if numeric.keyframes.is_empty() && numeric.values.len() == 1 {
        return Ok(constant_key(time, numeric.values[0]));
    }
    let before = numeric
        .keyframes
        .iter()
        .rev()
        .find(|key| key.time_secs < time);
    let after = numeric.keyframes.iter().find(|key| key.time_secs > time);
    let value = match (before, after) {
        (Some(before), Some(after)) if before.values == after.values => before.values[0],
        (Some(before), None) => before.values[0],
        _ => {
            return Err(PropertyError::Layout(
                "Rectangle control axes require curve resampling",
            ));
        }
    };
    Ok(constant_key(time, value))
}

fn constant_key(time: f64, value: f64) -> NumericKeyframe {
    NumericKeyframe {
        time_secs: time,
        values: vec![value],
        in_interpolation: 1,
        out_interpolation: 1,
        in_speed: vec![0.0],
        in_influence: vec![0.0],
        out_speed: vec![0.0],
        out_influence: vec![0.0],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    }
}

fn without_expression(mut numeric: NumericProperty) -> NumericProperty {
    numeric.expression_enabled = false;
    numeric.expression_present = false;
    numeric
}

fn values(numeric: &NumericProperty) -> impl Iterator<Item = f64> + '_ {
    numeric.values.iter().copied().chain(
        numeric
            .keyframes
            .iter()
            .flat_map(|key| key.values.iter().copied()),
    )
}
