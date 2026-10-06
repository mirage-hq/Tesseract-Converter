//! Reuses occurrence-local Essential Properties paths; no name-based binding.

#[cfg(test)]
mod tests;

use super::*;
use crate::{essential, properties, rifx::Chunk, structure::Layer};
use std::collections::BTreeSet;

/// Current static value in native property units. Colour alpha and a point's
/// unedited third coordinate remain the known template values.
#[derive(Debug, Clone, PartialEq)]
pub enum SavedGraphicNumeric {
    /// Scalar/boolean/angle values keep native units.
    Scalar(f64),
    /// Normalized RGB; saved Capsule alpha has no verified mapping here.
    ColourRgb([f64; 3]),
    /// Template-pixel position, not Premiere's normalized parameter storage.
    Point([f64; 2]),
}

fn error(message: impl Into<String>) -> GraphicTemplateError {
    GraphicTemplateError::Controller(message.into())
}

pub(super) fn instantiate(
    template: &SavedGraphicTemplate,
    uuids: &[&str],
) -> Result<(SavedGraphicTemplate, u32, Vec<String>), GraphicTemplateError> {
    let mut seen = BTreeSet::new();
    let mut selected = BTreeSet::new();
    let mut declared = BTreeSet::new();
    let mut warnings = Vec::new();
    for item in &template.project.items {
        let ItemKind::Composition(comp) = &item.kind else {
            continue;
        };
        for warning in &comp.essential_properties.warnings {
            warnings.push(format!(
                "template composition {}: {}",
                item.id, warning.message
            ));
        }
        if !comp.essential_properties.values.is_empty() {
            declared.insert(item.id);
        }
        for controller in &comp.essential_properties.values {
            if !seen.insert(controller.uuid.as_str()) {
                return Err(error(format!(
                    "ambiguous template controller UUID {:?}",
                    controller.uuid
                )));
            }
            if uuids.contains(&controller.uuid.as_str()) {
                selected.insert(item.id);
            }
        }
    }
    if selected.is_empty() {
        selected = declared;
    }
    if selected.is_empty() {
        selected.extend(
            template.project.items.iter().filter_map(|item| {
                matches!(item.kind, ItemKind::Composition(_)).then_some(item.id)
            }),
        );
    }
    let mut ids = selected.into_iter();
    let id = ids
        .next()
        .ok_or_else(|| error("template has no composition"))?;
    if ids.next().is_some() {
        return Err(error(
            "template controller declarations do not select one composition",
        ));
    }
    Ok((template.clone(), id, warnings))
}

fn controller(
    template: &SavedGraphicTemplate,
    uuid: &str,
) -> Result<essential::Controller, GraphicTemplateError> {
    let mut found = template
        .project
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return None;
            };
            Some(comp.essential_properties.values.iter())
        })
        .flatten()
        .filter(|controller| controller.uuid == uuid);
    let value = found.next().ok_or_else(|| {
        error(format!(
            "controller {uuid:?} absent; template property retained"
        ))
    })?;
    if found.next().is_some() {
        return Err(error("ambiguous controller UUID"));
    }
    Ok(value.clone())
}

// Exactly the same indexed/by-name selection as essential::apply. A failed read
// never chooses a sibling or invents an implicit source value for partial edits.
fn storage<'a>(
    layer: &'a Layer,
    path: &[essential::SourcePropertyRef],
) -> Result<&'a [Chunk], GraphicTemplateError> {
    let mut group = properties::unique_list(&layer.content, *b"tdgp")
        .map_err(|error| super::GraphicTemplateError::Controller(error.to_string()))?;
    let mut effect_control = false;
    for (position, node) in path.iter().enumerate() {
        let runs = properties::runs(group)
            .map_err(|error| super::GraphicTemplateError::Controller(error.to_string()))?;
        let index = if effect_control {
            None
        } else {
            node.child_index
        };
        let selected = if let Some(index) = index {
            runs.get(usize::try_from(index).map_err(|_| error("source index does not fit usize"))?)
                .filter(|(name, _)| *name == node.match_name)
                .map(|(_, storage)| *storage)
        } else {
            let mut matches = runs.iter().filter(|(name, _)| *name == node.match_name);
            let selected = matches.next().map(|(_, storage)| *storage);
            if matches.next().is_some() {
                return Err(error("ambiguous source property"));
            }
            selected
        }
        .ok_or_else(|| error(format!("source path {:?} is absent", node.match_name)))?;
        if position + 1 == path.len() {
            return Ok(selected);
        }
        effect_control = position > 0 && path[position - 1].match_name == "ADBE Effect Parade";
        let selected = if effect_control {
            properties::unique_list(selected, *b"sspc")
                .map_err(|error| super::GraphicTemplateError::Controller(error.to_string()))?
        } else {
            selected
        };
        group = properties::unique_list(selected, *b"tdgp")
            .map_err(|error| super::GraphicTemplateError::Controller(error.to_string()))?;
    }
    Err(error("empty source property path"))
}

struct NativeNumericSource {
    numeric: properties::NumericProperty,
    effect_point: bool,
}

fn effect_default(
    layer: &Layer,
    path: &[essential::SourcePropertyRef],
    size: [f64; 2],
) -> Result<Option<NativeNumericSource>, GraphicTemplateError> {
    let [parade, instance, property] = path else {
        return Ok(None);
    };
    if parade.match_name != "ADBE Effect Parade" {
        return Ok(None);
    }
    let (effects, _) = crate::effects::native::read_effects(&layer.content, size);
    let mut candidates = effects.iter().filter(|effect| {
        effect.match_name == instance.match_name
            && instance.child_index.is_none_or(|index| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                    == Some(effect.index)
            })
    });
    let Some(effect) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Err(error("ambiguous source effect"));
    }
    let mut candidates = effect
        .parameters
        .iter()
        .filter(|parameter| parameter.match_name == property.match_name);
    let Some(parameter) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Err(error("ambiguous source effect parameter"));
    }
    let kind = parameter
        .declared_kind
        .clone()
        .map_err(|error| {
            super::GraphicTemplateError::Controller(format!(
                "source effect declaration not decoded: {error}"
            ))
        })?
        .or_else(|| {
            crate::effects::definitions::definition(&effect.match_name)
                .and_then(|definition| {
                    definition
                        .parameters
                        .iter()
                        .find(|definition| definition.match_name == parameter.match_name)
                })
                .map(|definition| definition.kind)
        });
    parameter
        .numeric
        .clone()
        .map(|numeric| {
            Some(NativeNumericSource {
                numeric,
                effect_point: kind == Some(6),
            })
        })
        .map_err(|error| {
            super::GraphicTemplateError::Controller(format!(
                "source effect default not decoded: {error}"
            ))
        })
}

pub(super) fn apply_numeric(
    template: &mut SavedGraphicTemplate,
    uuid: &str,
    value: &SavedGraphicNumeric,
) -> Result<Vec<String>, GraphicTemplateError> {
    let controller = controller(template, uuid)?;
    let comp_id = controller
        .source_comp_id
        .ok_or_else(|| error("controller has no source composition"))?;
    let layer_id = controller
        .source_layer_id
        .ok_or_else(|| error("controller has no source layer"))?;
    let comp = template
        .project
        .items
        .iter_mut()
        .find_map(|item| {
            if item.id != comp_id {
                return None;
            }
            let ItemKind::Composition(comp) = &mut item.kind else {
                return None;
            };
            Some(comp)
        })
        .ok_or_else(|| error("source composition is absent"))?;
    let layer = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == layer_id)
        .ok_or_else(|| error("source layer is absent"))?;
    let original = match storage(layer, &controller.path) {
        Ok(storage) => Some(properties::unique_list(storage, *b"tdbs").map_err(|error| {
            super::GraphicTemplateError::Controller(format!(
                "source property is not numeric: {error}"
            ))
        })?),
        // Essential Properties admits only its existing declared/implicit numeric
        // leaves when storage is absent. Partial colour/point edits still require
        // a successfully decoded source value below.
        Err(_) => None,
    };
    let effect_point =
        original.is_some_and(|storage| properties::read_effect_point(storage).is_ok());
    let numeric = original
        .map(|storage| {
            properties::read_numeric(storage)
                .or_else(|_| properties::read_effect_point(storage))
                .map_err(|error| {
                    super::GraphicTemplateError::Controller(format!(
                        "source numeric storage not decoded: {error}; source retained"
                    ))
                })
        })
        .transpose()?;
    let size = [f64::from(comp.width), f64::from(comp.height)];
    let source = match numeric {
        Some(numeric) => Some(NativeNumericSource {
            numeric,
            effect_point,
        }),
        None => effect_default(layer, &controller.path, size)?,
    };
    let effect_point = source.as_ref().is_some_and(|source| source.effect_point);
    let numeric = source.as_ref().map(|source| &source.numeric);
    let (values, colour) = match value {
        SavedGraphicNumeric::Scalar(value) => {
            if numeric.as_ref().is_some_and(|source| {
                source.value_kind == properties::NumericValueKind::Color || source.values.len() > 1
            }) {
                return Err(error(
                    "scalar override is incompatible with source property",
                ));
            }
            (vec![*value], false)
        }
        SavedGraphicNumeric::ColourRgb(rgb) => {
            let source = numeric
                .as_ref()
                .filter(|source| {
                    source.value_kind == properties::NumericValueKind::Color
                        && source.values.len() == 4
                })
                .ok_or_else(|| error("partial colour requires known template alpha"))?;
            (vec![rgb[0], rgb[1], rgb[2], source.values[3]], true)
        }
        SavedGraphicNumeric::Point(point) => {
            let source = numeric
                .as_ref()
                .filter(|source| matches!(source.values.len(), 2 | 3))
                .ok_or_else(|| error("point override requires a known source point"))?;
            let mut values = source.values.clone();
            values[..2].copy_from_slice(point);
            (values, false)
        }
    };
    if values.iter().any(|value| !value.is_finite())
        || (colour && values.iter().any(|value| !(0.0..=1.0).contains(value)))
    {
        return Err(error("invalid/nonfinite numeric override"));
    }
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[2..4].copy_from_slice(
        &u16::try_from(values.len())
            .map_err(|_| error("invalid dimensions"))?
            .to_be_bytes(),
    );
    meta[59] = if colour {
        1
    } else if effect_point {
        4
    } else {
        0
    };
    let encoded = if colour {
        vec![
            values[3] * 255.0,
            values[0] * 255.0,
            values[1] * 255.0,
            values[2] * 255.0,
        ]
    } else if effect_point {
        if values.len() != 2 || size.iter().any(|size| *size <= 0.0) {
            return Err(error(
                "native effect point requires two finite source bounds",
            ));
        }
        // Native plugin type-4 points store fractions; the effect reader scales
        // them back into the same source-pixel units used by this override API.
        vec![values[0] / size[0], values[1] / size[1]]
    } else {
        values
    };
    let bytes = encoded
        .into_iter()
        .flat_map(f64::to_be_bytes)
        .collect::<Vec<_>>();
    let chunk = |id, bytes: Vec<u8>| {
        Chunk::data(id, bytes)
            .map_err(|error| super::GraphicTemplateError::Controller(error.to_string()))
    };
    let replacement = essential::Override {
        source_comp_id: comp_id,
        source_layer_id: layer_id,
        value: essential::OverrideValue::Property {
            path: controller.path,
            chunks: vec![Chunk::list(
                *b"tdbs",
                vec![
                    chunk(*b"tdb4", meta)?,
                    chunk(*b"tdsb", vec![0; 4])?,
                    chunk(*b"cdat", bytes)?,
                ],
            )],
        },
    };
    let mut warnings = essential::apply(layer, &replacement)
        .map(|warnings| {
            warnings
                .into_iter()
                .map(|warning| warning.message)
                .collect::<Vec<_>>()
        })
        .map_err(|warning| error(warning.message))?;
    if colour {
        warnings.push(
            "saved Capsule colour alpha override is not mapped; known template alpha retained"
                .into(),
        );
    }
    Ok(warnings)
}
