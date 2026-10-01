//! Lowering for direct scalar transform-property aliases.
//! No expression execution; only strict cross-layer aliases and same-member identities are lowered.

use std::collections::HashSet;

use super::{expression, finished, quoted, token, unique_run};
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ScalarProperty {
    PositionX,
    PositionY,
    Rotation,
}

impl ScalarProperty {
    fn from_match_name(name: &str) -> Option<Self> {
        match name {
            "ADBE Position_0" => Some(Self::PositionX),
            "ADBE Position_1" => Some(Self::PositionY),
            "ADBE Rotate Z" => Some(Self::Rotation),
            _ => None,
        }
    }

    const fn match_name(self) -> &'static str {
        match self {
            Self::PositionX => "ADBE Position_0",
            Self::PositionY => "ADBE Position_1",
            Self::Rotation => "ADBE Rotate Z",
        }
    }

    const fn expression_name(self) -> &'static str {
        match self {
            Self::PositionX => "xPosition",
            Self::PositionY => "yPosition",
            Self::Rotation => "rotation",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Link<'a> {
    layer: &'a str,
    property: ScalarProperty,
    sign: f64,
}

fn parse(mut text: &str) -> Option<Link<'_>> {
    text = text.trim_start();
    let sign = if token(&mut text, "-").is_some() {
        -1.0
    } else {
        1.0
    };
    token(&mut text, "thisComp.layer")?;
    token(&mut text, "(")?;
    let layer = quoted(&mut text)?;
    if layer.is_empty() {
        return None;
    }
    token(&mut text, ")")?;
    token(&mut text, ".transform.")?;
    let property = if token(&mut text, "xPosition").is_some() {
        ScalarProperty::PositionX
    } else if token(&mut text, "yPosition").is_some() {
        ScalarProperty::PositionY
    } else {
        token(&mut text, "rotation")?;
        ScalarProperty::Rotation
    };
    finished(text).then_some(Link {
        layer,
        property,
        sign,
    })
}

fn is_same_member_identity(mut text: &str, property: ScalarProperty) -> bool {
    token(&mut text, "transform.").is_some()
        && token(&mut text, property.expression_name()).is_some()
        && finished(text)
}

/// Replaces a recognized direct transform alias with an independent scalar curve.
///
/// `None` means the destination expression is outside the deliberately strict grammar.
pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    name: &str,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let property = ScalarProperty::from_match_name(name)?;
    let text = property_expression(layer, property).ok()?;
    if is_same_member_identity(text, property) {
        return Some(resolve(layer, composition, property));
    }
    parse(text)?;
    Some(validate_destination(base).and_then(|()| resolve(layer, composition, property)))
}

fn resolve<'a>(
    mut layer: &'a Layer,
    composition: &'a Composition,
    mut property: ScalarProperty,
) -> Result<NumericProperty, PropertyError> {
    let mut visited = HashSet::new();
    let mut links = Vec::new();
    let mut curve = loop {
        if !visited.insert((unique_identity(layer, composition)?, property)) {
            return Err(PropertyError::Layout("cyclic transform property alias"));
        }
        let leaf = property_leaf(layer, property)?;
        let numeric = properties::read_numeric(leaf)?;
        if !numeric.expression_enabled {
            validate_scalar_curve(&numeric)?;
            break numeric;
        }
        let text = expression(leaf)?;
        let Some(link) = parse(text) else {
            break resolve_existing_control(layer, composition, property, &numeric, text)?;
        };
        let source = unique_source(composition, link.layer)?;
        links.push((source, layer, link.sign));
        layer = source;
        property = link.property;
    };
    // Preserve the original per-edge clock/sign conversion order.
    for (source, destination, sign) in links.into_iter().rev() {
        curve = rebase_and_sign(curve, source, destination, sign)?;
    }
    Ok(curve)
}

fn resolve_existing_control(
    layer: &Layer,
    composition: &Composition,
    property: ScalarProperty,
    numeric: &NumericProperty,
    text: &str,
) -> Result<NumericProperty, PropertyError> {
    if is_same_member_identity(text, property) {
        let mut curve = numeric.clone();
        curve.expression_enabled = false;
        curve.expression_present = false;
        validate_scalar_curve(&curve)?;
        return Ok(curve);
    }

    let curve = match property {
        ScalarProperty::Rotation => super::rotation::lower(layer, composition, numeric),
        ScalarProperty::PositionX => {
            super::rotation::lower_position_x(layer, composition, numeric).0
        }
        ScalarProperty::PositionY => super::slider::lower_signed_scalar(layer, composition, text),
    }?;
    validate_scalar_curve(&curve)?;
    Ok(curve)
}

fn property_leaf(
    layer: &Layer,
    property: ScalarProperty,
) -> Result<&[crate::rifx::Chunk], PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    properties::unique_list(unique_run(&leaves, property.match_name())?, *b"tdbs")
}

fn property_expression(layer: &Layer, property: ScalarProperty) -> Result<&str, PropertyError> {
    expression(property_leaf(layer, property)?)
}

fn unique_identity(layer: &Layer, composition: &Composition) -> Result<u32, PropertyError> {
    let id = layer.record.id();
    if id == 0
        || composition
            .layers
            .iter()
            .filter(|candidate| candidate.record.id() == id)
            .count()
            != 1
    {
        return Err(PropertyError::Layout(
            "transform property alias requires a unique native layer identity",
        ));
    }
    Ok(id)
}

fn unique_source<'a>(composition: &'a Composition, name: &str) -> Result<&'a Layer, PropertyError> {
    let mut matches = composition
        .layers
        .iter()
        .filter(|candidate| candidate.name.as_ref() == name);
    let source = matches.next().ok_or(PropertyError::Layout(
        "transform property alias source is missing",
    ))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout(
            "transform property alias source name is ambiguous",
        ));
    }
    let _ = unique_identity(source, composition)?;
    Ok(source)
}

fn validate_destination(base: &NumericProperty) -> Result<(), PropertyError> {
    if !base.expression_enabled
        || base.animated
        || !base.keyframes.is_empty()
        || base.values.len() != 1
        || !base.values[0].is_finite()
        || base.dimensions_separated
        || base.value_kind != NumericValueKind::Continuous
    {
        return Err(PropertyError::Layout(
            "transform property alias destination is not a finite scalar",
        ));
    }
    Ok(())
}

fn validate_scalar_curve(curve: &NumericProperty) -> Result<(), PropertyError> {
    let valid_values =
        curve.values.len() <= 1 && curve.values.iter().all(|value| value.is_finite());
    let valid_key_order = curve
        .keyframes
        .windows(2)
        .all(|pair| pair[0].time_secs < pair[1].time_secs);
    let valid_keys = curve.keyframes.iter().all(|key| {
        key.time_secs.is_finite()
            && key.values.len() == 1
            && key.values[0].is_finite()
            && valid_metadata(&key.in_speed, false)
            && valid_metadata(&key.out_speed, false)
            && valid_metadata(&key.in_influence, true)
            && valid_metadata(&key.out_influence, true)
            && matches!(key.in_interpolation, 1..=3)
            && matches!(key.out_interpolation, 1..=3)
            && key.spatial_in.is_empty()
            && key.spatial_out.is_empty()
    });
    let valid_interpolation_pairs = curve.keyframes.windows(2).all(|pair| {
        if pair[0].out_interpolation == 3 {
            pair[1].in_interpolation == 3
        } else {
            matches!(pair[0].out_interpolation, 1 | 2) && matches!(pair[1].in_interpolation, 1 | 2)
        }
    });
    let valid_shape = if curve.animated {
        !curve.keyframes.is_empty()
    } else {
        curve.values.len() == 1 && curve.keyframes.is_empty()
    };
    if curve.expression_enabled
        || curve.expression_present
        || curve.dimensions_separated
        || curve.value_kind != NumericValueKind::Continuous
        || !valid_values
        || !valid_key_order
        || !valid_keys
        || !valid_interpolation_pairs
        || !valid_shape
    {
        return Err(PropertyError::Layout(
            "transform property alias source is not a raw finite scalar curve",
        ));
    }
    Ok(())
}

fn valid_metadata(values: &[f64], influence: bool) -> bool {
    values.len() == 1
        && values
            .iter()
            .all(|value| value.is_finite() && (!influence || (0.0..=100.0).contains(value)))
}

fn rebase_and_sign(
    mut curve: NumericProperty,
    source: &Layer,
    owner: &Layer,
    sign: f64,
) -> Result<NumericProperty, PropertyError> {
    validate_scalar_curve(&curve)?;
    let (Some(source_start), Some(source_stretch)) =
        (source.record.start_time(), source.record.stretch())
    else {
        return Err(PropertyError::Layout(
            "invalid transform property alias source clock",
        ));
    };
    let (Some(owner_start), Some(owner_stretch)) =
        (owner.record.start_time(), owner.record.stretch())
    else {
        return Err(PropertyError::Layout(
            "invalid transform property alias owner clock",
        ));
    };
    if !source_start.is_finite()
        || !owner_start.is_finite()
        || !source_stretch.is_finite()
        || !owner_stretch.is_finite()
        || source_stretch <= 0.0
        || owner_stretch != source_stretch
    {
        return Err(PropertyError::Layout(
            "transform property alias requires equal finite positive source stretch",
        ));
    }
    let offset = (source_start - owner_start) / owner_stretch;
    if !offset.is_finite() {
        return Err(PropertyError::Layout(
            "invalid transform property alias clock offset",
        ));
    }
    for value in curve.values.iter_mut().chain(
        curve
            .keyframes
            .iter_mut()
            .flat_map(|key| key.values.iter_mut()),
    ) {
        *value *= sign;
        if !value.is_finite() {
            return Err(PropertyError::Layout(
                "nonfinite transform property alias value",
            ));
        }
    }
    for key in &mut curve.keyframes {
        key.time_secs += offset;
        if !key.time_secs.is_finite() {
            return Err(PropertyError::Layout(
                "invalid rebased transform property alias key",
            ));
        }
        for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
            *speed *= sign;
            if !speed.is_finite() {
                return Err(PropertyError::Layout(
                    "nonfinite transform property alias speed",
                ));
            }
        }
    }
    curve.expression_enabled = false;
    curve.expression_present = false;
    Ok(curve)
}

#[cfg(test)]
mod tests;
