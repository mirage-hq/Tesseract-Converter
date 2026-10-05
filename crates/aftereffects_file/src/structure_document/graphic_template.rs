//! Static editable capsule contents use the ordinary Text/Shape decoders.
//! Responsive width is an explicit source-average glyph approximation, not an
//! expression runtime, native readback cache or generated destination script.

use super::{animation_budget::AnimationBudget, control_links, shapes, text, transform};
use crate::{
    graphic_template::{GraphicTemplateError, SavedGraphicText},
    properties,
    rifx::Chunk,
    structure::{Composition, ItemKind, StructuralProject},
};
use fx_schema::{Duration, GroupLayer, Layer, LayerData, LayerId, Time, TimeRangeProperty};
use std::collections::BTreeMap;

pub(crate) fn layers(
    project: &StructuralProject,
    id: u32,
    values: &[SavedGraphicText],
) -> Result<(Vec<GroupLayer>, Vec<String>), GraphicTemplateError> {
    let Some(ItemKind::Composition(comp)) = project.item(id).map(|item| &item.kind) else {
        return Err(error("composition missing"));
    };
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let mut next_id = comp
        .layers
        .iter()
        .map(|layer| u64::from(layer.record.id()))
        .max()
        .unwrap_or(0)
        + 1;
    let mut budget = shapes::OutputBudget::default();
    let mut animation_budget = AnimationBudget::default();
    let mut groups = Vec::new();
    let mut warnings = values
        .iter()
        .flat_map(|value| {
            value
                .diagnostics
                .iter()
                .map(move |warning| format!("template layer {}: {warning}", value.layer_id))
        })
        .collect::<Vec<_>>();
    for source in &comp.layers {
        let mut source = source.clone();
        let label = format!("template layer {} ({:?})", source.record.id(), source.name);
        let mut occurrence = super::group(
            LayerId::new(u64::from(source.record.id())),
            source.name.to_string(),
            (source.record.parent_id() != 0)
                .then(|| LayerId::new(u64::from(source.record.parent_id()))),
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(comp.duration_secs)),
        );
        let identity = occurrence.transform;
        let solid = project
            .item(source.record.source_id())
            .and_then(|item| item.solid.as_ref())
            .and_then(|solid| solid.as_ref().ok());
        let size = solid.map_or([0, 0], |solid| [solid.width, solid.height]);
        let (matrix, messages) = transform::static_transform(&source, size, comp);
        occurrence.transform = matrix;
        occurrence.is_hidden = !source.record.flags().enabled || source.record.flags().guide_layer;
        warnings.extend(
            messages
                .into_iter()
                .map(|message| format!("{label}: {message}")),
        );
        if has_mask_coverage(&source)
            .map_err(|error| {
                warnings.push(format!(
                    "{label}: mask coverage not decoded: {error}; consumer retained hidden"
                ))
            })
            .unwrap_or(true)
        {
            occurrence.is_hidden = true;
            warnings.push(format!("{label}: unsupported template mask coverage; concealed consumer retained hidden, never exposed unmasked"));
        }
        if source.record.track_matte_type() != 0 {
            occurrence.is_hidden = true;
            warnings.push(format!("{label}: unsupported template Track Matte coverage; concealed consumer retained hidden, never exposed unmasked"));
        }
        if source.record.in_point() != Some(0.0)
            || source.record.out_point() != Some(comp.duration_secs)
        {
            warnings.push(format!("{label}: template-layer lifetime normalized to the occurrence; placement source clocks are retained"));
        }
        let layers = match source.record.layer_type() {
            0 if solid.is_some()
                && !source.record.flags().null_layer
                && !source.record.flags().adjustment_layer =>
            {
                let solid = solid.ok_or_else(|| error("solid source is absent"))?;
                let rect =
                    transform::solid_rect(solid, &occurrence, LayerId::new(next_id), identity);
                next_id += 1;
                vec![LayerData::Rect(rect)]
            }
            3 => {
                let imported = text::import_in_composition(
                    &source,
                    comp,
                    None,
                    &occurrence,
                    &[],
                    &mut next_id,
                    &mut animation_budget,
                );
                warnings.extend(imported.warnings);
                let mut layers = imported.layers;
                if let Some(value) = values.iter().find(|value| {
                    value.composition_id == id && value.layer_id == source.record.id()
                }) {
                    let mut count = 0;
                    for layer in &mut layers {
                        if let LayerData::Text(layer) = layer {
                            layer.source_text = value.document.clone();
                            // Default AE character grouping is not a live animator.
                            // Its authoring controls have no ordinary graphic counterpart.
                            if layer.anchor_options.as_ref().is_some_and(|options| {
                                options.anchor_point_grouping != Default::default()
                                    || options.grouping_alignment != [0.0, 0.0]
                            }) {
                                warnings.push(format!("{label}: nondefault Text anchor grouping approximated by ordinary text"));
                            }
                            if layer.path_options.is_some() {
                                warnings.push(format!("{label}: Text Path Options omitted; ordinary editable text retained"));
                            }
                            layer.anchor_options = None;
                            layer.path_options = None;
                            layer.track_matte = None;
                            count += 1;
                        }
                    }
                    if count != 1 {
                        warnings.push(format!("{label}: resolved uniform Text did not yield one editable Text; convertible siblings retained"));
                    }
                }
                if !imported.animations.is_empty() {
                    warnings.push(format!(
                        "{label}: Text animation controls reduced to current static editable values"
                    ));
                }
                layers
            }
            4 => {
                let original = source.clone();
                rewrite(&mut source.content, &original, comp, values, &mut warnings);
                let imported = match shapes::import_with_evaluations(
                    &source,
                    comp,
                    (id, &crate::ExpressionSamples::default()),
                    &items,
                    true,
                    &occurrence,
                    super::MAX_GROUP_DEPTH,
                    &mut next_id,
                    &mut budget,
                    &mut animation_budget,
                ) {
                    Ok(imported) => imported,
                    Err(error) => {
                        warnings.push(format!(
                            "{label}: Shape omitted: {error}; convertible siblings retained"
                        ));
                        groups.push(occurrence);
                        continue;
                    }
                };
                warnings.extend(imported.warnings);
                if imported.animations.iter().any(|entry| {
                    !matches!(
                        entry.animator.data(),
                        fx_schema::animator::AnimatorData::Constant { .. }
                    )
                }) {
                    warnings.push(format!(
                        "{label}: keyed template Shape controls reduced to initial editable values"
                    ));
                }
                imported.layers
            }
            kind => {
                warnings.push(format!(
                    "{label}: template layer kind {kind} omitted; Text/Shape siblings retained"
                ));
                Vec::new()
            }
        };
        for layer in &layers {
            match Layer::from_data(layer) {
                Ok(layer) => occurrence.layers.push(layer),
                Err(error) => warnings.push(format!(
                    "{label}: child {:?} omitted: {error}; convertible siblings retained",
                    layer.name()
                )),
            }
        }
        groups.push(occurrence);
    }
    Ok((groups, warnings))
}
fn has_mask_coverage(source: &crate::structure::Layer) -> Result<bool, properties::PropertyError> {
    for (_, parade) in properties::root_runs(&source.content)?
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Parade")
    {
        for (_, atom) in properties::runs(properties::unique_list(parade, *b"tdgp")?)?
            .into_iter()
            .filter(|(name, _)| *name == "ADBE Mask Atom")
        {
            let info = properties::data(atom, *b"mkif")?;
            if info.len() != 48 {
                return Ok(true);
            }
            if u16::from_be_bytes([info[6], info[7]]) != 0 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn error(message: &str) -> GraphicTemplateError {
    GraphicTemplateError::Controller(message.into())
}

fn rewrite(
    chunks: &mut [Chunk],
    owner: &crate::structure::Layer,
    comp: &Composition,
    values: &[SavedGraphicText],
    warnings: &mut Vec<String>,
) {
    for index in 0..chunks.len().saturating_sub(1) {
        if chunks[index].id() != *b"tdmn"
            || chunks[index]
                .data_payload()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .map(|name| name.trim_end_matches('\0'))
                != Some("ADBE Vector Rect Size")
        {
            continue;
        }
        let Some(storage) = chunks[index + 1].children_mut() else {
            continue;
        };
        let Ok(numeric) = properties::read_numeric(storage) else {
            continue;
        };
        if !numeric.expression_enabled || numeric.animated {
            continue;
        }
        let Ok(expression) = control_links::expression(storage) else {
            continue;
        };
        let Some((size, missing_slider)) = rectangle_size(expression, owner, comp, values) else {
            warnings.push("responsive Rectangle expression outside static linear-width subset; editable authored outline retained, not resolved layout".into());
            continue;
        };
        if missing_slider {
            warnings.push("responsive Rectangle has a declared Slider whose value record is absent; the additive adjustment is approximated by neutral zero, not a decoded current value; verify the controller before relying on its geometry".into());
        }
        for chunk in storage {
            if chunk.id() == *b"cdat" {
                let mut bytes = chunk.data_payload().unwrap_or_default().to_vec();
                if bytes.len() < 16 {
                    continue;
                }
                bytes[..8].copy_from_slice(&size[0].to_be_bytes());
                bytes[8..16].copy_from_slice(&size[1].to_be_bytes());
                *chunk = Chunk::data(*b"cdat", bytes).expect("cdat is a data chunk");
            } else if chunk.id() == *b"tdb4" {
                let mut bytes = chunk.data_payload().unwrap_or_default().to_vec();
                // read_numeric already validated this metadata's 124-byte layout.
                bytes[properties::EXPRESSION_DISABLED_OFFSET] |=
                    properties::EXPRESSION_DISABLED_MASK;
                *chunk = Chunk::data(*b"tdb4", bytes).expect("tdb4 is a data chunk");
            }
        }
        warnings.push("responsive Rectangle width approximated from current character count/font size and source-average cached glyph extent; kerning, glyph differences, font changes and tracking are not measured; height/padding/paints/transforms retained as editable native-derived geometry; no live controller after edits".into());
    }
    for chunk in chunks {
        if let Some(children) = chunk.children_mut() {
            rewrite(children, owner, comp, values, warnings);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Scalar(f64),
    Text(u32),
    EstimatedSlider(f64),
}

// A complete static assignment-and-addition form, using the existing bounded
// control-link lexer/resolver. No arbitrary code, names, constants or time grid
// are evaluated. Unknown syntax fails as a whole; comments are not statements.
fn rectangle_size(
    source: &str,
    owner: &crate::structure::Layer,
    comp: &Composition,
    texts: &[SavedGraphicText],
) -> Option<([f64; 2], bool)> {
    let mut clean = String::new();
    let mut quote = None;
    let mut comment = false;
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if comment {
            if matches!(c, '\r' | '\n') {
                comment = false;
                clean.push(' ');
            }
            continue;
        }
        if quote.is_none() && c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            comment = true;
            continue;
        }
        if matches!(c, '\'' | '"') {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(c);
            }
        }
        clean.push(c);
    }
    let mut variables = BTreeMap::new();
    let mut statements: Vec<_> = clean
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let array = statements.pop()?;
    for statement in statements {
        let (name, expression) = statement.split_once('=')?;
        let mut name = name.trim().strip_prefix("var ").unwrap_or(name.trim());
        let variable = control_links::identifier(&mut name)?;
        if !name.trim().is_empty() || variables.contains_key(variable) {
            return None;
        }
        let value = term(expression.trim(), &variables, owner, comp, texts)?;
        variables.insert(variable.to_owned(), value);
    }
    let array = array.strip_prefix('[')?.strip_suffix(']')?;
    let (width, height) = array.split_once(',')?;
    let width = term(width.trim(), &variables, owner, comp, texts)?;
    let height = term(height.trim(), &variables, owner, comp, texts)?;
    let missing =
        matches!(width, Value::EstimatedSlider(_)) || matches!(height, Value::EstimatedSlider(_));
    let (Value::Scalar(width) | Value::EstimatedSlider(width)) = width else {
        return None;
    };
    let (Value::Scalar(height) | Value::EstimatedSlider(height)) = height else {
        return None;
    };
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then_some(([width, height], missing))
}
fn term(
    expression: &str,
    vars: &BTreeMap<String, Value>,
    owner: &crate::structure::Layer,
    comp: &Composition,
    texts: &[SavedGraphicText],
) -> Option<Value> {
    let mut rest = expression;
    if control_links::token(&mut rest, "thisComp.layer(").is_some() {
        let name = control_links::quoted(&mut rest)?;
        control_links::token(&mut rest, ")")?;
        if !rest.trim().is_empty() {
            return None;
        }
        let mut found = comp.layers.iter().filter(|layer| &*layer.name == name);
        let layer = found.next()?;
        if found.next().is_some() {
            return None;
        }
        return Some(Value::Text(layer.record.id()));
    }
    if let Some(reference) = control_links::reference(&mut rest) {
        if !rest.trim().is_empty() {
            return None;
        }
        let roots = properties::root_runs(&owner.content).ok()?;
        let parade = control_links::unique_run(&roots, "ADBE Effect Parade").ok()?;
        let effects = properties::runs(properties::unique_list(parade, *b"tdgp").ok()?).ok()?;
        let numeric = match control_links::resolve(&effects, reference) {
            Ok(numeric) => numeric,
            Err(crate::properties::PropertyError::Layout("Slider parameter not found"))
                if reference.selects_slider_value() =>
            {
                // Only a unique, explicitly declared Slider reaches this error.
                // This is an estimated neutral additive control, never a getter
                // result, a hidden -0000 value or a silent native default.
                return Some(Value::EstimatedSlider(0.0));
            }
            Err(_) => return None,
        };
        if numeric.animated || numeric.expression_enabled || numeric.values.len() != 1 {
            return None;
        }
        return Some(Value::Scalar(numeric.values[0]));
    }
    let mut total = 0.0;
    let mut estimated = false;
    for part in expression.split('+') {
        let mut part = part.trim();
        let value = if let Some(number) = control_links::finite_number(&mut part) {
            if !part.trim().is_empty() {
                return None;
            }
            number
        } else {
            let name = control_links::identifier(&mut part)?;
            let value = *vars.get(name)?;
            match value {
                Value::Scalar(value) if part.trim().is_empty() => value,
                Value::EstimatedSlider(value) if part.trim().is_empty() => {
                    estimated = true;
                    value
                }
                Value::Text(id) => {
                    control_links::token(&mut part, ".sourceRectAtTime(")?;
                    control_links::token(&mut part, "time")?;
                    control_links::token(&mut part, ",")?;
                    control_links::token(&mut part, "false")?;
                    control_links::token(&mut part, ")")?;
                    control_links::token(&mut part, ".width")?;
                    if !part.trim().is_empty() {
                        return None;
                    }
                    let layer = comp.layers.iter().find(|layer| layer.record.id() == id)?;
                    let current = texts.iter().find(|text| text.layer_id == id)?;
                    text::saved_graphic_width(layer, &current.document).ok()?
                }
                _ => return None,
            }
        };
        total += value;
    }
    total.is_finite().then_some(if estimated {
        Value::EstimatedSlider(total)
    } else {
        Value::Scalar(total)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_synthetic_scalar_rectangle_recipe_fails_closed() {
        let owner = crate::structure::Layer {
            name: "synthetic".into(),
            record: crate::schema::layer_records::LayerRecord::decode(&[0; 164]).unwrap(),
            content: Vec::new(),
        };
        let comp = Composition {
            width: 320,
            height: 100,
            duration_secs: 1.0,
            frame_rate: 24.0,
            pixel_aspect: (1, 1),
            display_start_secs: 0.0,
            layers: Vec::new(),
            record: crate::schema::CompositionRecord::empty_ae26(
                320,
                100,
                crate::timing::Duration24::from_frames(24).unwrap(),
            )
            .unwrap(),
            essential_properties: crate::essential::Parsed {
                values: Vec::new(),
                warnings: Vec::new(),
            },
        };
        assert_eq!(
            rectangle_size(
                "var width = 40 + 2; height = 10; [width, height]",
                &owner,
                &comp,
                &[]
            ),
            Some(([42.0, 10.0], false))
        );
        for recipe in [
            "[time, 10]",
            "[missing, 10]",
            "[40, -10]",
            "width = 40; width = 50; [width, 10]",
            "[0, 10]",
            "[40, 10, 2]",
        ] {
            assert!(
                rectangle_size(recipe, &owner, &comp, &[]).is_none(),
                "{recipe}"
            );
        }
    }
}
