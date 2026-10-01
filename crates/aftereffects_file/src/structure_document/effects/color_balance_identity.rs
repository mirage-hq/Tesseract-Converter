//! Prove neutral Color Balance (HLS) controls without evaluating expressions.

use std::collections::HashMap;

use super::super::control_links::{display_name, expression, finished, quoted, token, unique_run};
use crate::{
    effects::native::{self, DecodedEffect},
    properties::{self, NumericProperty},
    rifx::Chunk,
    structure::{ItemKind, Layer, ProjectItem},
};

pub(super) const MATCH_NAME: &str = "ADBE Color Balance (HLS)";
const PARAMETERS: [&str; 3] = [
    "ADBE Color Balance (HLS)-0001",
    "ADBE Color Balance (HLS)-0002",
    "ADBE Color Balance (HLS)-0003",
];

fn neutral(value: &NumericProperty) -> bool {
    !value.animated
        && !value.expression_enabled
        && !value.expression_present
        && !value.dimensions_separated
        && value.keyframes.is_empty()
        && value.values == [0.0]
}

fn controls(layer: &Layer, index: usize) -> Result<&[Chunk], String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = unique_run(&roots, "ADBE Effect Parade").map_err(|e| e.to_string())?;
    let effects =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let (kind, run) = effects
        .get(index.checked_sub(1).ok_or("invalid effect index")?)
        .ok_or("effect missing")?;
    if *kind != MATCH_NAME {
        return Err("effect identity mismatch".into());
    }
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())
}

fn source_parameter(
    mut text: &str,
    parameter: &str,
    items: &HashMap<u32, &ProjectItem>,
) -> Result<NumericProperty, String> {
    if text.len() > 16 * 1024 {
        return Err("effect alias exceeds the bounded expression size".into());
    }
    let mut parse = || -> Option<(&str, &str, &str, &str)> {
        token(&mut text, "comp")?;
        token(&mut text, "(")?;
        let comp = quoted(&mut text)?;
        token(&mut text, ")")?;
        token(&mut text, ".layer")?;
        token(&mut text, "(")?;
        let layer = quoted(&mut text)?;
        token(&mut text, ")")?;
        token(&mut text, ".effect")?;
        token(&mut text, "(")?;
        let effect = quoted(&mut text)?;
        token(&mut text, ")")?;
        token(&mut text, "(")?;
        let property = quoted(&mut text)?;
        token(&mut text, ")")?;
        finished(text).then_some((comp, layer, effect, property))
    };
    let (comp_name, layer_name, effect_name, property_name) =
        parse().ok_or("not a pure cross-composition effect property alias")?;
    if property_name != parameter {
        return Err("alias changes HLS parameter identity".into());
    }
    let mut candidates = items.values().filter_map(|item| match &item.kind {
        ItemKind::Composition(comp) if item.name == comp_name => Some(comp),
        _ => None,
    });
    let comp = candidates.next().ok_or("source composition missing")?;
    if candidates.next().is_some() {
        return Err("source composition name ambiguous".into());
    }
    let mut layers = comp
        .layers
        .iter()
        .filter(|layer| layer.name.as_ref() == layer_name);
    let layer = layers.next().ok_or("source layer missing")?;
    if layers.next().is_some() {
        return Err("source layer name ambiguous".into());
    }
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = unique_run(&roots, "ADBE Effect Parade").map_err(|e| e.to_string())?;
    let runs =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut matches = Vec::new();
    for (index, (kind, run)) in runs.iter().enumerate() {
        let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
        let body = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
        if display_name(body) == Some(effect_name) {
            matches.push((index + 1, *kind));
        }
    }
    let [(index, MATCH_NAME)] = matches.as_slice() else {
        return Err("source effect name ambiguous, missing or not HLS".into());
    };
    let (effects, warnings) = native::read_effects(
        &layer.content,
        [f64::from(comp.width), f64::from(comp.height)],
    );
    if !warnings.is_empty() {
        return Err(format!(
            "source effect controls malformed: {}",
            warnings.join("; ")
        ));
    }
    let effect = effects
        .iter()
        .find(|e| e.index == *index)
        .ok_or("source effect undecodable")?;
    if effect
        .parameters
        .iter()
        .any(|p| !PARAMETERS.contains(&p.match_name.as_str()))
    {
        return Err("source has unknown HLS controls".into());
    }
    let mut values = effect
        .parameters
        .iter()
        .filter(|p| p.match_name == parameter);
    if let Some(value) = values.next() {
        if values.next().is_some() {
            return Err("source parameter ambiguous".into());
        }
        return value.numeric.clone().map_err(|e| e.to_string());
    }
    // Canonical HLS Hue/Lightness/Saturation defaults are all zero. These are
    // native control defaults, not a guess from the destination's cached value.
    // Explicit malformed/dynamic leaves above always take precedence.
    Ok(NumericProperty {
        values: vec![0.0],
        value_kind: crate::properties::NumericValueKind::Continuous,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
    })
}

pub(super) fn is_identity(
    layer: &Layer,
    effect: &DecodedEffect,
    items: Option<&HashMap<u32, &ProjectItem>>,
) -> Result<bool, String> {
    if effect.match_name != MATCH_NAME {
        return Ok(false);
    }
    let body = controls(layer, effect.index)?;
    let runs = properties::runs(body).map_err(|e| e.to_string())?;
    for parameter in PARAMETERS {
        let mut explicit = runs.iter().filter(|(name, _)| *name == parameter);
        let numeric = if let Some((_, run)) = explicit.next() {
            if explicit.next().is_some() {
                return Err("duplicate HLS control".into());
            }
            let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
            let numeric = properties::read_numeric(leaf).map_err(|e| e.to_string())?;
            if numeric.expression_enabled {
                source_parameter(
                    expression(leaf).map_err(|e| e.to_string())?,
                    parameter,
                    items.ok_or("HLS alias source context unavailable")?,
                )?
            } else {
                numeric
            }
        } else {
            let mut decoded = effect
                .parameters
                .iter()
                .filter(|p| p.match_name == parameter);
            if let Some(value) = decoded.next() {
                if decoded.next().is_some() {
                    return Err("duplicate HLS control".into());
                }
                value.numeric.clone().map_err(|e| e.to_string())?
            } else {
                NumericProperty {
                    values: vec![0.0],
                    value_kind: crate::properties::NumericValueKind::Continuous,
                    animated: false,
                    expression_enabled: false,
                    expression_present: false,
                    dimensions_separated: false,
                    keyframes: Vec::new(),
                }
            }
        };
        if !neutral(&numeric) {
            return Ok(false);
        }
    }
    if effect
        .parameters
        .iter()
        .any(|p| !PARAMETERS.contains(&p.match_name.as_str()))
    {
        return Err("unknown HLS controls".into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(id: &[u8; 4], value: impl Into<Vec<u8>>) -> Chunk {
        let mut value = value.into();
        if id == b"tdmn" {
            value.resize(40, 0);
        }
        Chunk::data(*id, value).unwrap()
    }

    fn list(kind: &[u8; 4], children: Vec<Chunk>) -> Chunk {
        Chunk::list(*kind, children)
    }

    fn name(value: &str) -> Chunk {
        let mut bytes = b"Utf8".to_vec();
        bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
        bytes.extend(value.as_bytes());
        data(b"tdsn", bytes)
    }

    fn numeric(values: &[f64], expression: Option<&str>) -> Chunk {
        let mut meta = vec![0; 124];
        meta[..2].copy_from_slice(&[0xdb, 0x99]);
        meta[3] = u8::try_from(values.len()).unwrap();
        let mut chunks = vec![
            data(b"tdb4", meta),
            data(b"tdsb", vec![0, 0, 0, 1]),
            data(
                b"cdat",
                values
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<_>>(),
            ),
            name("Slider"),
        ];
        if let Some(expression) = expression {
            chunks.push(data(b"Utf8", expression.as_bytes()));
        }
        list(b"tdbs", chunks)
    }

    use crate::structure::read_project;

    fn content(label: &str, values: Option<[f64; 3]>, aliases: bool) -> Vec<Chunk> {
        let mut leaves = vec![name(label)];
        if let Some(values) = values {
            for (parameter, value) in PARAMETERS.into_iter().zip(values) {
                leaves.push(data(b"tdmn", parameter.as_bytes()));
                let alias = format!(
                    "comp('Renamed control comp').layer('Renamed controller').effect('Renamed HLS')('{parameter}')"
                );
                leaves.push(numeric(&[value], aliases.then_some(alias.as_str())));
            }
        }
        vec![list(
            b"tdgp",
            vec![
                data(b"tdmn", b"ADBE Effect Parade"),
                list(
                    b"tdgp",
                    vec![
                        data(b"tdmn", MATCH_NAME.as_bytes()),
                        list(b"sspc", vec![list(b"parT", vec![]), list(b"tdgp", leaves)]),
                    ],
                ),
            ],
        )]
    }
    fn source() -> (Layer, crate::structure::StructuralProject) {
        let mut project = read_project(include_bytes!(
            "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let item = project
            .items
            .iter_mut()
            .find(|item| matches!(item.kind, ItemKind::Composition(_)))
            .unwrap();
        item.name = "Renamed control comp".into();
        let ItemKind::Composition(comp) = &mut item.kind else {
            panic!()
        };
        let mut layer = comp.layers[0].clone();
        layer.name = "Renamed controller".into();
        layer.content = content("Renamed HLS", None, false);
        comp.layers = vec![layer.clone()];
        let mut destination = layer;
        destination.content = content("Local HLS", Some([20.0, 30.0, 40.0]), true);
        (destination, project)
    }
    #[test]
    fn neutral_hls_aliases_use_unique_native_controls_not_cached_destination_values() {
        let (destination, project) = source();
        let items = project.items.iter().map(|item| (item.id, item)).collect();
        let effect = &native::read_effects(&destination.content, [320.0, 180.0]).0[0];
        assert!(is_identity(&destination, effect, Some(&items)).unwrap());
        assert!(is_identity(&destination, effect, None).is_err());
        for (values, aliases) in [(Some([0.0, 0.0, 1.0]), false), (Some([0.0; 3]), true)] {
            let mut altered = project.clone();
            let ItemKind::Composition(comp) = &mut altered
                .items
                .iter_mut()
                .find(|i| i.name == "Renamed control comp")
                .unwrap()
                .kind
            else {
                panic!()
            };
            comp.layers[0].content = content("Renamed HLS", values, aliases);
            let items = altered.items.iter().map(|item| (item.id, item)).collect();
            assert!(!is_identity(&destination, effect, Some(&items)).unwrap());
        }
        let mut ambiguous = project.clone();
        let ItemKind::Composition(comp) = &mut ambiguous
            .items
            .iter_mut()
            .find(|i| i.name == "Renamed control comp")
            .unwrap()
            .kind
        else {
            panic!()
        };
        comp.layers.push(comp.layers[0].clone());
        let items = ambiguous.items.iter().map(|item| (item.id, item)).collect();
        assert!(is_identity(&destination, effect, Some(&items)).is_err());
    }
    #[test]
    fn one_nonzero_hls_control_is_not_identity_and_wrong_effect_never_gets_defaults() {
        let (mut layer, _) = source();
        for values in [[0.0; 3], [0.0, 2.0, 0.0]] {
            layer.content = content("Local", Some(values), false);
            let effect = &native::read_effects(&layer.content, [320.0, 180.0]).0[0];
            assert_eq!(
                is_identity(&layer, effect, None).unwrap(),
                values == [0.0; 3]
            );
        }
        layer.content = content("Local", None, false);
        let mut effect = native::read_effects(&layer.content, [320.0, 180.0])
            .0
            .remove(0);
        effect.match_name = "Pseudo/controller with Hue labels".into();
        assert!(!is_identity(&layer, &effect, None).unwrap());
    }
}
