//! Static extrusion positions authored from the native timeline index and a Slider.
//! No expression is executed; reordering layers or editing the controller in FX
//! does not update the independently editable imported Position.

use super::{
    expression, finished, finite_number, properties, quoted, reference, resolve, token, unique_run,
};
use crate::{
    properties::{NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

struct Profile<'a> {
    xy: [f64; 2],
    offset: f64,
    layer: &'a str,
    slider: super::Reference<'a>,
}

fn parse(mut text: &str) -> Option<Profile<'_>> {
    token(&mut text, "[")?;
    let x = finite_number(&mut text)?;
    token(&mut text, ",")?;
    let y = finite_number(&mut text)?;
    token(&mut text, ",")?;
    token(&mut text, "(")?;
    token(&mut text, "index")?;
    token(&mut text, "-")?;
    // Reject `index--2` (including `index--0`): JavaScript tokenizes `--`
    // as decrement. This bounded profile does not admit signed offsets.
    if text.trim_start().starts_with(['-', '+']) {
        return None;
    }
    let offset = finite_number(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, "*")?;
    token(&mut text, "thisComp")?;
    token(&mut text, ".")?;
    token(&mut text, "layer")?;
    token(&mut text, "(")?;
    let layer = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, ".")?;
    let slider = reference(&mut text)?;
    token(&mut text, "]")?;
    finished(text).then_some(Profile {
        xy: [x, y],
        offset,
        layer,
        slider,
    })
}

pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let root = properties::root_runs(&layer.content).ok()?;
    let transform = unique_run(&root, "ADBE Transform Group").ok()?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").ok()?).ok()?;
    let body =
        properties::unique_list(unique_run(&leaves, "ADBE Position").ok()?, *b"tdbs").ok()?;
    let profile = parse(expression(body).ok()?)?;
    Some(lower_profile(layer, composition, base, profile))
}

fn lower_profile(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
    profile: Profile<'_>,
) -> Result<NumericProperty, PropertyError> {
    if !layer.record.flags().three_d_layer || base.values.len() != 3 || base.dimensions_separated {
        return Err(PropertyError::Layout(
            "indexed Position requires unified 3D Position",
        ));
    }
    // Use original real Layr order, including hidden/null/controller layers,
    // not the generated FX layer IDs or the expanded render-layer count.
    let index = composition
        .layers
        .iter()
        .position(|candidate| candidate.record.id() == layer.record.id())
        .ok_or(PropertyError::Layout("indexed Position owner missing"))?;
    let index = u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or(PropertyError::Layout("indexed Position index overflow"))?;
    let mut candidates = composition
        .layers
        .iter()
        .filter(|candidate| candidate.name.as_ref() == profile.layer);
    let controller = candidates
        .next()
        .ok_or(PropertyError::Layout("indexed Position controller missing"))?;
    if candidates.next().is_some() {
        return Err(PropertyError::Layout(
            "ambiguous indexed Position controller",
        ));
    }
    let root = properties::root_runs(&controller.content)?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let slider = resolve(&effects, profile.slider)?;
    if slider.animated
        || !slider.keyframes.is_empty()
        || slider.expression_enabled
        || slider.values.len() != 1
    {
        return Err(PropertyError::Layout(
            "indexed Position requires a static scalar Slider",
        ));
    }
    let z = (f64::from(index) - profile.offset) * slider.values[0];
    let values = vec![profile.xy[0], profile.xy[1], z];
    if values.iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::Layout("indexed Position nonfinite result"));
    }
    Ok(NumericProperty {
        values,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind: NumericValueKind::Continuous,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "[1920,1080,(index-2)*thisComp.layer(\"Extrude CTRL\").effect(\"Slider Control 1\")(\"ADBE Slider Control-0001\")]";

    #[test]
    fn indexed_position_complete_grammar_rejects_different_semantics() {
        let profile = parse(SOURCE).unwrap();
        assert_eq!(profile.xy, [1920.0, 1080.0]);
        assert_eq!(profile.offset, 2.0);
        assert_eq!(profile.layer, "Extrude CTRL");
        for text in [
            format!("{SOURCE};evil()"),
            SOURCE.replace("index-2", "index+2"),
            SOURCE.replace("index-2", "index--2"),
            SOURCE.replace("index-2", "index--0"),
            SOURCE.replace("index-2", "index-time"),
            SOURCE.replace("1920,1080", "value[0],value[1]"),
            SOURCE.replace("thisComp.layer", "comp(\"Other\").layer"),
            SOURCE.replace("*thisComp", "/thisComp"),
        ] {
            assert!(parse(&text).is_none(), "{text}");
        }
    }
}
