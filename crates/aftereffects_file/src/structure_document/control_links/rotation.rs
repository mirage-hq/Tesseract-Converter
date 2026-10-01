//! Bounded equal-stretch scalar offsets for Rotation and separated X Position.
//! No expression execution; degree/pixel values stay in their native units.
use super::{display_name, expression, finished, quoted, token, unique_run};
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

#[derive(Debug, PartialEq)]
struct Link<'a> {
    layer: &'a str,
    effect: &'a str,
    parameter: &'a str,
    sign: f64,
    target: &'static str,
}

fn identifier<'a>(text: &mut &'a str) -> Option<&'a str> {
    *text = text.trim_start();
    let length = text
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    let name = text.get(..length)?;
    if name.is_empty()
        || name.as_bytes()[0].is_ascii_digit()
        || matches!(name, "thisComp" | "transform" | "value" | "var")
    {
        return None;
    }
    *text = &text[length..];
    Some(name)
}

fn parse(mut text: &str) -> Option<Link<'_>> {
    text = text.trim_start();
    if let Some(rest) = text.strip_prefix("var")
        && rest.starts_with(char::is_whitespace)
    {
        text = rest;
    }
    let binding = identifier(&mut text)?;
    token(&mut text, "=")?;
    token(&mut text, "thisComp.layer")?;
    token(&mut text, "(")?;
    let layer = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, ".effect")?;
    token(&mut text, "(")?;
    let effect = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, "(")?;
    let parameter = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, ";")?;
    token(&mut text, "transform.")?;
    let target = if token(&mut text, "rotation").is_some() {
        "ADBE Rotate Z"
    } else {
        token(&mut text, "xPosition")?;
        "ADBE Position_0"
    };
    let sign = if token(&mut text, "+").is_some() {
        1.0
    } else {
        token(&mut text, "-")?;
        -1.0
    };
    if identifier(&mut text)? != binding || !finished(text) {
        return None;
    }
    Some(Link {
        layer,
        effect,
        parameter,
        sign,
        target,
    })
}

pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Result<NumericProperty, PropertyError> {
    lower_property(
        layer,
        composition,
        base,
        "ADBE Rotate Z",
        "ADBE Angle Control",
        "ADBE Angle Control-0001",
    )
}

pub(super) fn lower_position_x(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> (Result<NumericProperty, PropertyError>, &'static str) {
    if let Ok(value) = lower_property(
        layer,
        composition,
        base,
        "ADBE Position_0",
        "ADBE Slider Control",
        "ADBE Slider Control-0001",
    ) {
        return (Ok(value), "scalar offset");
    }
    let resolved = (|| {
        let root = properties::root_runs(&layer.content)?;
        let transform = unique_run(&root, "ADBE Transform Group")?;
        let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
        let leaf = properties::unique_list(unique_run(&leaves, "ADBE Position_0")?, *b"tdbs")?;
        super::slider::lower_signed_scalar(layer, composition, expression(leaf)?)
    })();
    (resolved, "direct Slider reference")
}

fn lower_property(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
    property: &str,
    control_kind: &str,
    parameter: &str,
) -> Result<NumericProperty, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let leaf = properties::unique_list(unique_run(&leaves, property)?, *b"tdbs")?;
    let link = parse(expression(leaf)?)
        .ok_or(PropertyError::Layout("not a direct scalar offset binding"))?;
    if link.target != property {
        return Err(PropertyError::Layout(
            "scalar offset targets another property",
        ));
    }
    let mut matches = composition
        .layers
        .iter()
        .filter(|candidate| candidate.name.as_ref() == link.layer);
    let controller = matches
        .next()
        .ok_or(PropertyError::Layout("scalar controller layer missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous scalar controller layer"));
    }
    let (Some(start), Some(stretch)) = (layer.record.start_time(), layer.record.stretch()) else {
        return Err(PropertyError::Layout("invalid scalar binding clock"));
    };
    let source_start = controller
        .record
        .start_time()
        .ok_or(PropertyError::Layout("invalid scalar controller clock"))?;
    if !start.is_finite()
        || !source_start.is_finite()
        || !stretch.is_finite()
        || stretch <= 0.0
        || controller.record.stretch() != Some(stretch)
    {
        return Err(PropertyError::Layout(
            "scalar binding requires equal positive source stretch",
        ));
    }
    let root = properties::root_runs(&controller.content)?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let mut matches = effects.iter().filter_map(|(kind, run)| {
        let plugin = properties::unique_list(run, *b"sspc").ok()?;
        let body = properties::unique_list(plugin, *b"tdgp").ok()?;
        (display_name(body) == Some(link.effect)).then_some((*kind, body))
    });
    let (kind, body) = matches
        .next()
        .ok_or(PropertyError::Layout("scalar control missing"))?;
    if matches.next().is_some() || kind != control_kind {
        return Err(PropertyError::Layout(
            "ambiguous or mismatched scalar control kind",
        ));
    }
    let runs = properties::runs(body)?;
    let leaf = properties::unique_list(unique_run(&runs, parameter)?, *b"tdbs")?;
    if link.parameter != parameter && display_name(leaf) != Some(link.parameter) {
        return Err(PropertyError::Layout(
            "scalar parameter name does not match",
        ));
    }
    let numeric = properties::read_numeric(leaf)?;
    let mut curve = offset_curve(numeric, base, link.sign)?;
    // Native keys are layer-local. Equal stretch permits a pure time translation;
    // the existing target clock then restores their original composition times.
    let offset = (source_start - start) / stretch;
    if !offset.is_finite() {
        return Err(PropertyError::Layout("invalid scalar clock offset"));
    }
    for key in &mut curve.keyframes {
        key.time_secs += offset;
        if !key.time_secs.is_finite() {
            return Err(PropertyError::Layout("invalid rebased scalar key"));
        }
    }
    Ok(curve)
}

fn offset_curve(
    mut curve: NumericProperty,
    base: &NumericProperty,
    sign: f64,
) -> Result<NumericProperty, PropertyError> {
    if base.animated
        || !base.keyframes.is_empty()
        || base.values.len() != 1
        || !base.values[0].is_finite()
        || curve.expression_enabled
        || curve.dimensions_separated
        || curve.value_kind == NumericValueKind::Color
        || (curve.animated && curve.keyframes.is_empty())
        || (!curve.animated && curve.values.len() != 1)
        || (!curve.values.is_empty() && curve.values.len() != 1)
        || curve.keyframes.iter().any(|key| {
            key.values.len() != 1 || !key.spatial_in.is_empty() || !key.spatial_out.is_empty()
        })
    {
        return Err(PropertyError::Layout(
            "unsupported scalar curve or animated destination base",
        ));
    }
    for value in curve.values.iter_mut().chain(
        curve
            .keyframes
            .iter_mut()
            .flat_map(|key| key.values.iter_mut()),
    ) {
        *value = base.values[0] + sign * *value;
        if !value.is_finite() {
            return Err(PropertyError::Layout("nonfinite scalar offset"));
        }
    }
    for key in &mut curve.keyframes {
        for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
            *speed *= sign;
        }
    }
    // Angle controls use the native scalar flag also seen on integer controls,
    // but their authored temporal easing must remain continuous, not quantized.
    curve.value_kind = NumericValueKind::Continuous;
    curve.expression_enabled = false;
    curve.expression_present = false;
    Ok(curve)
}

#[cfg(test)]
mod tests;
