//! Copies complete static native color-controller aliases, not an expression runtime.

use std::collections::HashMap;

use crate::{
    properties::{self, NumericValueKind, PropertyError},
    rifx::Chunk,
    structure::{Composition, ItemKind, Layer, ProjectItem},
};

use super::{finished, quoted, token, unique_run};

pub(super) fn resolve(
    property: &[Chunk],
    composition: &Composition,
    source_items: Option<&HashMap<u32, &ProjectItem>>,
) -> Result<[f64; 4], PropertyError> {
    let mut text = super::expression(property)?;
    let target_composition = if token(&mut text, "thisComp").is_some() {
        composition
    } else {
        token(&mut text, "comp")
            .and_then(|_| token(&mut text, "("))
            .ok_or(PropertyError::Layout(
                "not a direct Color Control composition alias",
            ))?;
        let name = quoted(&mut text).ok_or(PropertyError::Layout(
            "invalid Color Control composition name",
        ))?;
        token(&mut text, ")").ok_or(PropertyError::Layout(
            "invalid Color Control composition access",
        ))?;
        let items =
            source_items.ok_or(PropertyError::Layout("Color Control project scope missing"))?;
        let mut matches = items.values().filter_map(|item| match &item.kind {
            ItemKind::Composition(composition) if item.name == name => Some(composition.as_ref()),
            _ => None,
        });
        let composition = matches
            .next()
            .ok_or(PropertyError::Layout("Color Control composition missing"))?;
        if matches.next().is_some() {
            return Err(PropertyError::Layout("ambiguous Color Control composition"));
        }
        composition
    };
    token(&mut text, ".")
        .and_then(|_| token(&mut text, "layer"))
        .and_then(|_| token(&mut text, "("))
        .ok_or(PropertyError::Layout(
            "not a direct Color Control layer alias",
        ))?;
    let name =
        quoted(&mut text).ok_or(PropertyError::Layout("invalid Color Control layer name"))?;
    token(&mut text, ")")
        .and_then(|_| token(&mut text, "."))
        .ok_or(PropertyError::Layout("invalid Color Control access"))?;
    token(&mut text, "effect")
        .and_then(|_| token(&mut text, "("))
        .ok_or(PropertyError::Layout("invalid Color Control effect access"))?;
    let effect =
        quoted(&mut text).ok_or(PropertyError::Layout("invalid Color Control effect name"))?;
    token(&mut text, ")")
        .and_then(|_| token(&mut text, "("))
        .ok_or(PropertyError::Layout(
            "invalid Color Control parameter access",
        ))?;
    // A regular Color Control exposes its sole value at index 1. Keep numeric
    // access distinct from quoted pseudo-control labels: their index layouts
    // are not the regular Color Control contract.
    let indexed = token(&mut text, "1").is_some();
    let parameter = if indexed {
        "ADBE Color Control-0001"
    } else {
        quoted(&mut text).ok_or(PropertyError::Layout("invalid Color Control parameter"))?
    };
    if token(&mut text, ")").is_none() || !finished(text) {
        return Err(PropertyError::Layout("not a complete Color Control alias"));
    }
    let source = unique_layer(target_composition, name)?;
    let roots = properties::root_runs(&source.content)?;
    let parade = unique_run(&roots, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let mut matches = effects.iter().filter_map(|(kind, run)| {
        let plugin = properties::unique_list(run, *b"sspc").ok()?;
        let body = properties::unique_list(plugin, *b"tdgp").ok()?;
        (super::effect_name(plugin, body) == Some(effect)).then_some((*kind, body))
    });
    let (kind, body) = matches
        .next()
        .ok_or(PropertyError::Layout("Color Control effect missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous Color Control effect"));
    }
    let parameters = properties::runs(body)?;
    let storage = if kind == "ADBE Color Control"
        && matches!(parameter, "Color" | "ADBE Color Control-0001")
    {
        properties::unique_list(
            unique_run(&parameters, "ADBE Color Control-0001")?,
            *b"tdbs",
        )?
    } else if !indexed && kind.starts_with("Pseudo/") {
        // Native pseudo effects carry display names on each parameter's storage,
        // not on the surrounding run. Admit only a unique explicit color leaf;
        // never infer plugin defaults or evaluate its controller program.
        let mut matched = None;
        for (name, run) in &parameters {
            // Compositing controls are not custom effect-value parameters.
            if *name == "ADBE Effect Built In Params" {
                continue;
            }
            let Some(suffix) = name
                .strip_prefix(kind)
                .and_then(|suffix| suffix.strip_prefix('-'))
            else {
                return Err(PropertyError::Layout(
                    "invalid pseudo Color Control parameter namespace",
                ));
            };
            if suffix.len() != 4 || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(PropertyError::Layout(
                    "invalid pseudo Color Control parameter namespace",
                ));
            }
            let storage = properties::unique_list(run, *b"tdbs")?;
            if super::display_name(storage) == Some(parameter) && matched.replace(storage).is_some()
            {
                return Err(PropertyError::Layout(
                    "ambiguous pseudo Color Control parameter",
                ));
            }
        }
        matched.ok_or(PropertyError::Layout(
            "pseudo Color Control parameter missing",
        ))?
    } else {
        return Err(PropertyError::Layout("not a native color-controller alias"));
    };
    let numeric = properties::read_numeric(storage)?;
    if numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.expression_enabled
        || numeric.dimensions_separated
    {
        return Err(PropertyError::Layout(
            "Color Control must be static and expression-free",
        ));
    }
    if numeric.value_kind != NumericValueKind::Color {
        return Err(PropertyError::Layout(
            "Color Control must have native color storage",
        ));
    }
    let color: [f64; 4] = numeric
        .values
        .try_into()
        .map_err(|_| PropertyError::Layout("Color Control requires four components"))?;
    if color
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(PropertyError::Layout(
            "Color Control requires finite unit color components",
        ));
    }
    // Static controls have no clock dependency. Animated controls are rejected,
    // rather than assuming the sibling's source clock equals the consumer's.
    Ok(color)
}

fn unique_layer<'a>(composition: &'a Composition, name: &str) -> Result<&'a Layer, PropertyError> {
    let mut matches = composition
        .layers
        .iter()
        .filter(|layer| layer.name.as_ref() == name);
    let source = matches
        .next()
        .ok_or(PropertyError::Layout("Color Control layer missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous Color Control layer"));
    }
    Ok(source)
}
