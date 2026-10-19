//! Static inward Border envelope for Roughen Edges; not its procedural edge band.

use std::{borrow::Cow, collections::HashSet};

use fx_schema::LayerEffect;

use super::native::{Declarations, DecodedEffect};
use crate::{properties, rifx::Chunk, structure::Layer};

fn scalar(source: &DecodedEffect, name: &str, shared: Option<&[Chunk]>) -> Result<f64, String> {
    let numeric = if let Some(parameter) = source.parameters.iter().find(|p| p.match_name == name) {
        parameter
            .declared_kind
            .as_ref()
            .map_err(|e| e.to_string())?;
        Cow::Borrowed(parameter.numeric.as_ref().map_err(|e| e.to_string())?)
    } else {
        let declarations =
            properties::runs(shared.ok_or_else(|| format!("{name}: missing control/default"))?)
                .map_err(|e| e.to_string())?;
        let matches: Vec<_> = declarations.iter().filter(|(id, _)| *id == name).collect();
        let [(_, declaration)] = matches.as_slice() else {
            return Err(format!("{name}: unique shared declaration required"));
        };
        Cow::Owned(
            super::native::default_numeric(declaration, [1., 1.]).map_err(|e| e.to_string())?,
        )
    };
    if numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.expression_present
        || numeric.expression_enabled
        || numeric.dimensions_separated
    {
        return Err(format!(
            "{name}: requires a static control without expressions"
        ));
    }
    match numeric.values.as_slice() {
        [value] if value.is_finite() => Ok(*value),
        _ => Err(format!("{name}: requires a finite scalar")),
    }
}

fn controls(source: &DecodedEffect, layer: &Layer) -> Result<bool, String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let mut parades = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade");
    let (_, parade) = parades.next().ok_or("missing Effect Parade")?;
    if parades.next().is_some() {
        return Err("duplicate Effect Parade".into());
    }
    let instances =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let (name, run) = instances
        .get(
            source
                .index
                .checked_sub(1)
                .ok_or("invalid effect ordinal")?,
        )
        .ok_or("missing effect ordinal")?;
    if *name != "ADBE Roughen Edges" {
        return Err("effect ordinal/name mismatch".into());
    }
    let plugin = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let body =
        properties::runs(properties::unique_list(plugin, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in body {
        if !seen.insert(name) {
            return Err(format!("duplicate Roughen Edges control {name}"));
        }
        if name == "ADBE Effect Built In Params" {
            let options = properties::runs(
                properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                return Err("nonempty Roughen Edges compositing options".into());
            }
        }
    }
    Ok(properties::unique_list(plugin, *b"parT")
        .map_err(|e| e.to_string())?
        .is_empty())
}

pub(crate) fn lower(
    source: &DecodedEffect,
    layer: &Layer,
    definitions: &[Chunk],
) -> Result<Vec<LayerEffect>, String> {
    if source.declarations == Declarations::Unreadable {
        return Err("unreadable Roughen Edges declarations".into());
    }
    let sparse = controls(source, layer)?;
    let missing = ["ADBE Roughen Edges-0001", "ADBE Roughen Edges-0002"]
        .iter()
        .any(|name| !source.parameters.iter().any(|p| p.match_name == *name));
    let shared = if sparse && missing {
        Some(
            crate::essential::file_effect_declarations(definitions, "ADBE Roughen Edges")
                .map_err(|warning| warning.message)?,
        )
    } else {
        None
    };
    if scalar(source, "ADBE Roughen Edges-0001", shared)? != 1. {
        return Err("only Roughen Edge Type is approximated; colored/cut/spiky/rusty/photocopy modes are unsupported".into());
    }
    let border = scalar(source, "ADBE Roughen Edges-0002", shared)?;
    if !(0. ..=500.).contains(&border) {
        return Err("Border must be within the native 0..500 pixel range".into());
    }
    // Each existing renderer pass clamps choke to ten pixels. Separate editable
    // passes retain the whole inward envelope instead of silently clipping it.
    let mut remaining = border;
    let mut effects = Vec::new();
    loop {
        let choke = remaining.min(10.);
        effects.push(LayerEffect::SimpleChoker { choke: Some(choke) });
        remaining -= choke;
        if remaining <= 0. {
            return Ok(effects);
        }
    }
}
