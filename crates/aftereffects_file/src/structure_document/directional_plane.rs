//! Native Directional Blur source-plane controls versus FX screen-space controls.

use fx_schema::{
    EffectData, EffectPayload, EffectRecord, LayerEffect, NonNegativeProperty, Transform,
};

use crate::structure::{Composition, Layer};

/// Uniform static affine controls preserve a blur axis and its pixel units;
/// native and FX sampling kernels are still different approximations.
pub(super) fn compensate(
    layer: &Layer,
    composition: &Composition,
    size: [u16; 2],
    transform: &Transform,
    effects: &mut [EffectRecord],
) -> Result<bool, String> {
    if !layer.record.flags().effects_active || effects.len() != 1 {
        return Ok(false);
    }
    let roots = crate::properties::root_runs(&layer.content).map_err(|error| error.to_string())?;
    for (_, run) in roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Parade")
    {
        let parade =
            crate::properties::unique_list(run, *b"tdgp").map_err(|error| error.to_string())?;
        if !crate::properties::runs(parade)
            .map_err(|error| error.to_string())?
            .is_empty()
        {
            return Ok(false);
        }
    }
    let (native, _) = crate::effects::native::read_effects(&layer.content, size.map(f64::from));
    let mut enabled = native.iter().filter(|effect| effect.enabled);
    let Some(native) = enabled.next() else {
        return Ok(false);
    };
    if enabled.next().is_some() || native.match_name != "ADBE Motion Blur" {
        return Ok(false);
    }
    for name in ["ADBE Motion Blur-0001", "ADBE Motion Blur-0002"] {
        let Some(property) = native
            .parameters
            .iter()
            .find(|property| property.match_name == name)
        else {
            return Ok(false);
        };
        if !property
            .numeric
            .as_ref()
            .is_ok_and(|value| !value.animated && !value.expression_enabled)
        {
            return Ok(false);
        }
    }
    let (properties, _) = super::control_links::read_layer_transform(layer, composition)
        .map_err(|error| error.to_string())?;
    for name in ["ADBE Scale", "ADBE Rotate Z"] {
        let Some(property) = properties
            .iter()
            .find(|property| property.match_name == name)
        else {
            return Ok(false);
        };
        if !property
            .numeric
            .as_ref()
            .is_ok_and(|value| !value.animated && !value.expression_enabled)
        {
            return Ok(false);
        }
    }
    if transform.scale[0] != transform.scale[1]
        || transform.scale[0] <= 0.0
        || !transform.scale[0].is_finite()
        || !transform.rotation.is_finite()
    {
        return Ok(false);
    }
    let mut data = effects[0].data().clone();
    let EffectData::Identified {
        enabled: true,
        effect:
            EffectPayload::Known(LayerEffect::DirectionalBlur {
                direction,
                blur_length,
            }),
        ..
    } = &mut data
    else {
        return Ok(false);
    };
    let world_direction = *direction + transform.rotation;
    let world_length = blur_length.value() * (transform.scale[0] / 100.0);
    if !world_direction.is_finite() || !world_length.is_finite() {
        return Ok(false);
    }
    if world_direction == *direction && world_length == blur_length.value() {
        return Ok(false);
    }
    *direction = world_direction;
    *blur_length =
        NonNegativeProperty::new(world_length).ok_or("invalid screen-space blur length")?;
    effects[0] = EffectRecord::from_data(&data).map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    fn controls(value: &Value, output: &mut Vec<[f64; 2]>) {
        match value {
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("directionalBlur") {
                    output.push([
                        object["direction"].as_f64().unwrap(),
                        object["blurLength"].as_f64().unwrap(),
                    ]);
                }
                for value in object.values() {
                    controls(value, output);
                }
            }
            Value::Array(values) => {
                for value in values {
                    controls(value, output);
                }
            }
            _ => {}
        }
    }

    fn edit_cdat(chunks: &mut [crate::rifx::Chunk], values: &[f64]) -> bool {
        for chunk in chunks {
            if chunk.id() == *b"cdat" {
                let mut bytes = chunk.data_payload().unwrap().to_vec();
                assert!(bytes.len() >= values.len() * 8);
                for (slot, value) in bytes.chunks_exact_mut(8).zip(values) {
                    slot.copy_from_slice(&value.to_be_bytes());
                }
                *chunk = crate::rifx::Chunk::data(*b"cdat", bytes).unwrap();
                return true;
            }
            if let Some(children) = chunk.children_mut()
                && edit_cdat(children, values)
            {
                return true;
            }
        }
        false
    }

    fn edit_static(chunks: &mut [crate::rifx::Chunk], name: &str, values: &[f64]) -> bool {
        for index in 0..chunks.len() {
            if chunks[index].id() == *b"tdmn"
                && chunks[index].data_payload().is_some_and(|bytes| {
                    bytes.split(|byte| *byte == 0).next() == Some(name.as_bytes())
                })
            {
                let end = chunks[index + 1..]
                    .iter()
                    .position(|chunk| chunk.id() == *b"tdmn")
                    .map_or(chunks.len(), |offset| index + 1 + offset);
                return edit_cdat(&mut chunks[index + 1..end], values);
            }
            if let Some(children) = chunks[index].children_mut()
                && edit_static(children, name, values)
            {
                return true;
            }
        }
        false
    }

    #[test]
    fn directional_plane_import_responds_to_native_transform_input_edits() {
        for (name, values, expected) in [
            // Native Scale cdat stores fractions, not FX percentages.
            ("ADBE Scale", vec![3.0, 3.0, 1.0], [90.0, 60.0]),
            ("ADBE Rotate Z", vec![-45.0], [-45.0, 40.0]),
            ("ADBE Scale", vec![2.0, 1.0, 1.0], [0.0, 20.0]),
            ("ADBE Scale", vec![-2.0, -2.0, 1.0], [0.0, 20.0]),
        ] {
            let mut project = crate::structure::read_project(&crate::test_fixtures::read(
                "effects/directional-static-source-plane.aep",
            ))
            .unwrap();
            let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
            let crate::structure::ItemKind::Composition(composition) = &mut item.kind else {
                panic!("pinned composition");
            };
            assert!(edit_static(
                &mut composition.layers[0].content,
                name,
                &values
            ));
            let converted = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
            let mut output = Vec::new();
            controls(
                &serde_json::to_value(converted.document).unwrap(),
                &mut output,
            );
            assert_eq!(output, [expected], "edited native {name}");
        }
    }

    #[test]
    fn native_directional_blur_uses_rotated_scaled_source_plane() {
        let project = crate::structure::read_project(&crate::test_fixtures::read(
            "effects/directional-static-source-plane.aep",
        ))
        .expect("independently Adobe-authored source");
        let converted = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
        let document = serde_json::to_value(converted.document).unwrap();
        let mut output = Vec::new();
        controls(&document, &mut output);
        assert_eq!(
            output,
            [[90.0, 40.0]],
            "native Direction0/Length20 under Scale200/Rotation90 must become horizontal screen-space controls"
        );
    }
}
