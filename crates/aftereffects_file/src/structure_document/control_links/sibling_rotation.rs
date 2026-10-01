//! Strict affine sibling Rotation lowering.
//! No expression execution; only bounded degree/sign mapping and clock rebasing.
use super::{expression, finished, finite_number, quoted, token, unique_run};
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

#[derive(Debug, PartialEq)]
struct Link<'a> {
    constant: f64,
    layer: &'a str,
    sign: f64,
}

fn parse(mut text: &str) -> Option<Link<'_>> {
    let original = text;
    if let Some(constant) = finite_number(&mut text)
        && token(&mut text, "+").is_some()
        && let Some(layer) = sibling_rotation(&mut text)
        && finished(text)
    {
        return Some(Link {
            constant,
            layer,
            sign: 1.0,
        });
    }

    text = original.trim_start();
    let sign = if token(&mut text, "-").is_some() {
        -1.0
    } else {
        1.0
    };
    let layer = sibling_rotation(&mut text)?;
    let constant_sign = if token(&mut text, "+").is_some() {
        1.0
    } else {
        token(&mut text, "-")?;
        -1.0
    };
    // Keep the pre-existing `reference + signed_constant` grammar. The new
    // subtraction form remains deliberately narrower (one unsigned operand).
    if constant_sign < 0.0 && matches!(text.trim_start().as_bytes().first(), Some(b'+' | b'-')) {
        return None;
    }
    let constant = constant_sign * finite_number(&mut text)?;
    finished(text).then_some(Link {
        constant,
        layer,
        sign,
    })
}

fn sibling_rotation<'a>(text: &mut &'a str) -> Option<&'a str> {
    token(text, "thisComp.layer")?;
    token(text, "(")?;
    let layer = quoted(text)?;
    if layer.is_empty() {
        return None;
    }
    token(text, ")")?;
    token(text, ".transform.rotation")?;
    Some(layer)
}

/// Replaces an enabled Rotation expression with an independent raw sibling curve.
///
/// Rejection is atomic: `property` remains unchanged on every error.
pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    property: &mut NumericProperty,
) -> Option<Result<(), PropertyError>> {
    if !property.expression_enabled {
        return Some(Err(PropertyError::Layout(
            "sibling Rotation destination has no enabled expression",
        )));
    }
    let text = match rotation_leaf(layer).and_then(expression) {
        Ok(text) => text,
        Err(error) => return Some(Err(error)),
    };
    lower_text(layer, composition, property, text)
}

/// Reuse the same bounded grammar for a Rotation-valued effect control.
pub(super) fn lower_text(
    layer: &Layer,
    composition: &Composition,
    property: &mut NumericProperty,
    text: &str,
) -> Option<Result<(), PropertyError>> {
    let link = parse(text)?;
    Some(lower_recognized(layer, composition, property, link))
}

fn lower_recognized(
    layer: &Layer,
    composition: &Composition,
    property: &mut NumericProperty,
    link: Link<'_>,
) -> Result<(), PropertyError> {
    let source = unique_sibling(layer, composition, link.layer)?;
    let (Some(owner_start), Some(owner_stretch)) =
        (layer.record.start_time(), layer.record.stretch())
    else {
        return Err(PropertyError::Layout(
            "invalid sibling Rotation owner clock",
        ));
    };
    let (Some(source_start), Some(source_stretch)) =
        (source.record.start_time(), source.record.stretch())
    else {
        return Err(PropertyError::Layout(
            "invalid sibling Rotation source clock",
        ));
    };
    if !owner_start.is_finite()
        || !source_start.is_finite()
        || !owner_stretch.is_finite()
        || !source_stretch.is_finite()
        || owner_stretch <= 0.0
        || source_stretch != owner_stretch
    {
        return Err(PropertyError::Layout(
            "sibling Rotation requires equal finite positive source stretch",
        ));
    }

    let mut curve = match rotation_leaf(source) {
        Ok(leaf) => {
            let curve = properties::read_numeric(leaf)?;
            validate_raw_rotation(&curve)?;
            curve
        }
        Err(PropertyError::Layout("missing named control")) => {
            property.values = vec![0.0];
            property.animated = false;
            property.expression_enabled = false;
            property.expression_present = false;
            property.dimensions_separated = false;
            property.keyframes.clear();
            property.clone()
        }
        Err(error) => return Err(error),
    };
    let offset = (source_start - owner_start) / owner_stretch;
    if !offset.is_finite() {
        return Err(PropertyError::Layout(
            "invalid sibling Rotation clock offset",
        ));
    }
    offset_curve(&mut curve, link.sign, link.constant, offset)?;
    curve.value_kind = NumericValueKind::Continuous;
    curve.expression_enabled = false;
    curve.expression_present = false;
    *property = curve;
    Ok(())
}

fn offset_curve(
    curve: &mut NumericProperty,
    sign: f64,
    constant: f64,
    offset: f64,
) -> Result<(), PropertyError> {
    for value in curve.values.iter_mut().chain(
        curve
            .keyframes
            .iter_mut()
            .flat_map(|key| key.values.iter_mut()),
    ) {
        *value = sign * *value + constant;
        if !value.is_finite() {
            return Err(PropertyError::Layout("nonfinite sibling Rotation offset"));
        }
    }
    for key in &mut curve.keyframes {
        key.time_secs += offset;
        if !key.time_secs.is_finite() {
            return Err(PropertyError::Layout(
                "invalid rebased sibling Rotation key",
            ));
        }
        for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
            *speed *= sign;
            if !speed.is_finite() {
                return Err(PropertyError::Layout("nonfinite sibling Rotation speed"));
            }
        }
    }
    Ok(())
}

fn rotation_leaf(layer: &Layer) -> Result<&[crate::rifx::Chunk], PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    properties::unique_list(unique_run(&leaves, "ADBE Rotate Z")?, *b"tdbs")
}

fn unique_sibling<'a>(
    owner: &Layer,
    composition: &'a Composition,
    name: &str,
) -> Result<&'a Layer, PropertyError> {
    let mut matches = composition
        .layers
        .iter()
        .filter(|candidate| candidate.name.as_ref() == name);
    let source = matches
        .next()
        .ok_or(PropertyError::Layout("sibling Rotation layer missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous sibling Rotation layer"));
    }
    let source_id = source.record.id();
    let owner_id = owner.record.id();
    if source_id == owner_id
        || composition
            .layers
            .iter()
            .filter(|candidate| candidate.record.id() == source_id)
            .count()
            != 1
        || composition
            .layers
            .iter()
            .filter(|candidate| candidate.record.id() == owner_id)
            .count()
            != 1
    {
        return Err(PropertyError::Layout(
            "sibling Rotation requires unique native layer identities",
        ));
    }
    Ok(source)
}

fn validate_raw_rotation(curve: &NumericProperty) -> Result<(), PropertyError> {
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
            && key.in_speed.len() == 1
            && key.in_speed[0].is_finite()
            && key.out_speed.len() == 1
            && key.out_speed[0].is_finite()
            && key.in_influence.len() == 1
            && (0.0..=100.0).contains(&key.in_influence[0])
            && key.out_influence.len() == 1
            && (0.0..=100.0).contains(&key.out_influence[0])
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
        || curve.value_kind == NumericValueKind::Color
        || !valid_values
        || !valid_key_order
        || !valid_keys
        || !valid_interpolation_pairs
        || !valid_shape
    {
        return Err(PropertyError::Layout(
            "sibling Rotation source is not a raw finite scalar curve",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
