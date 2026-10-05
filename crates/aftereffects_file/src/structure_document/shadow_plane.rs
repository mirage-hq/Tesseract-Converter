//! Native hard Shadow source-plane controls versus FX screen-pixel offsets.

use fx_schema::{EffectData, EffectPayload, EffectRecord, LayerEffect, Transform};

use crate::structure::Layer;

/// Only the isolated hard-shadow profile has an exact affine offset mapping here.
/// Soft kernels and nested/dynamic affine transforms retain the existing approximation.
pub(super) fn compensate(
    layer: &Layer,
    size: [u16; 2],
    transform: &Transform,
    effects: &mut [EffectRecord],
) -> Result<bool, String> {
    let (native, _) = crate::effects::native::read_effects(&layer.content, size.map(f64::from));
    let enabled: Vec<_> = native.iter().filter(|effect| effect.enabled).collect();
    let [native] = enabled.as_slice() else {
        return Ok(false);
    };
    if native.match_name != "ADBE Drop Shadow" || effects.len() != 1 {
        return Ok(false);
    }
    for suffix in ["-0003", "-0004", "-0005"] {
        let property = native
            .parameters
            .iter()
            .find(|property| property.match_name == format!("ADBE Drop Shadow{suffix}"));
        let Some(property) = property else {
            return Ok(false);
        };
        let Ok(value) = &property.numeric else {
            return Ok(false);
        };
        if value.animated || value.expression_enabled {
            return Ok(false);
        }
        if suffix == "-0005" && value.values.first() != Some(&0.0) {
            return Ok(false);
        }
    }
    // Eligibility is based on authored controls, before pure-expression aliases
    // can be lowered into apparently expression-free destination values.
    let properties =
        crate::properties::read_transform(&layer.content).map_err(|error| error.to_string())?;
    for name in ["ADBE Scale", "ADBE Rotate Z"] {
        let Some(property) = properties
            .iter()
            .find(|property| property.match_name == name)
        else {
            return Ok(false);
        };
        let Ok(value) = &property.numeric else {
            return Ok(false);
        };
        if value.animated || value.expression_enabled {
            return Ok(false);
        }
    }
    let mut data = effects[0].data().clone();
    let EffectData::Identified {
        effect: EffectPayload::Known(LayerEffect::DropShadow(shadow)),
        ..
    } = &mut data
    else {
        return Ok(false);
    };
    let Some(offset) = world_offset(shadow.offset, transform.scale, transform.rotation) else {
        return Ok(false);
    };
    if offset == shadow.offset {
        return Ok(false);
    }
    shadow.offset = offset;
    let record = EffectRecord::from_data(&data).map_err(|error| error.to_string())?;
    effects[0] = record;
    Ok(true)
}

fn world_offset(offset: [f64; 2], scale: [f64; 2], rotation: f64) -> Option<[f64; 2]> {
    if scale[0] != scale[1]
        || scale[0] <= 0.0
        || !scale[0].is_finite()
        || !rotation.is_finite()
        || offset.iter().any(|value| !value.is_finite())
    {
        return None;
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let factor = scale[0] / 100.0;
    let result = [
        factor * (offset[0] * cos - offset[1] * sin),
        factor * (offset[0] * sin + offset[1] * cos),
    ];
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    fn shadow_offsets(value: &Value, offsets: &mut Vec<[f64; 2]>) {
        match value {
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("dropShadow") {
                    let offset = object["offset"].as_array().expect("editable offset");
                    offsets.push([offset[0].as_f64().unwrap(), offset[1].as_f64().unwrap()]);
                }
                for value in object.values() {
                    shadow_offsets(value, offsets);
                }
            }
            Value::Array(values) => {
                for value in values {
                    shadow_offsets(value, offsets);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn hard_shadow_offset_responds_to_static_plane_input_edits() {
        let [x, y] = super::world_offset([20.0, 0.0], [300.0; 2], 90.0).unwrap();
        assert!(x.abs() < 1e-9 && (y - 60.0).abs() < 1e-9);
        assert_eq!(
            super::world_offset([20.0, 0.0], [300.0; 2], 0.0),
            Some([60.0, 0.0])
        );
        assert_eq!(
            super::world_offset([20.0, 0.0], [100.0; 2], 0.0),
            Some([20.0, 0.0])
        );
        for scale in [[200.0, 100.0], [-100.0; 2], [0.0; 2], [f64::INFINITY; 2]] {
            assert_eq!(super::world_offset([20.0, 0.0], scale, 90.0), None);
        }
        assert_eq!(super::world_offset([f64::NAN, 0.0], [100.0; 2], 90.0), None);
        assert_eq!(super::world_offset([20.0, 0.0], [100.0; 2], f64::NAN), None);
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
    fn fresh_shadow_import_responds_to_native_transform_edits_and_nonuniform_guard() {
        for (name, values, expected) in [
            // Native Scale cdat is a fraction; the importer exposes percentages.
            ("ADBE Scale", vec![3.0, 3.0, 1.0], [0.0, 60.0]),
            ("ADBE Rotate Z", vec![0.0], [40.0, 0.0]),
            ("ADBE Scale", vec![2.0, 1.0, 1.0], [20.0, 0.0]),
        ] {
            let mut project = crate::structure::read_project(&crate::test_fixtures::read(
                "effects/shadow-static-source-plane.aep",
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
            let mut offsets = Vec::new();
            shadow_offsets(
                &serde_json::to_value(converted.document).unwrap(),
                &mut offsets,
            );
            assert_eq!(offsets.len(), 1);
            for (actual, expected) in offsets[0].iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-9,
                    "edited {name}: {offsets:?}"
                );
            }
        }
    }

    #[test]
    fn auto_oriented_shadow_retains_the_existing_unverified_offset() {
        let mut project = crate::structure::read_project(&crate::test_fixtures::read(
            "effects/shadow-static-source-plane.aep",
        ))
        .unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let crate::structure::ItemKind::Composition(composition) = &mut item.kind else {
            panic!("pinned composition");
        };
        let layer = &mut composition.layers[0];
        let mut bytes = layer.record.encode();
        bytes[38] |= 1; // Established native auto-orient-along-path switch.
        layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
        assert_eq!(layer.record.auto_orient(), 1);
        let converted = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
        let mut offsets = Vec::new();
        shadow_offsets(
            &serde_json::to_value(converted.document).unwrap(),
            &mut offsets,
        );
        assert_eq!(offsets.len(), 1);
        assert!((offsets[0][0] - 20.0).abs() < 1e-9 && offsets[0][1].abs() < 1e-9);
        assert!(converted.diagnostics.iter().all(|diagnostic| {
            !diagnostic
                .message
                .contains("Isolated static hard Drop Shadow offset transformed")
        }));
    }

    #[test]
    fn native_static_hard_shadow_uses_the_transformed_source_plane() {
        let project = crate::structure::read_project(&crate::test_fixtures::read(
            "effects/shadow-static-source-plane.aep",
        ))
        .expect("independently authored native source");
        let converted = super::super::to_structural_fx_document(&project, Some(1))
            .expect("supported native Solid and Shadow");
        let mut offsets = Vec::new();
        shadow_offsets(
            &serde_json::to_value(converted.document).unwrap(),
            &mut offsets,
        );
        assert_eq!(offsets.len(), 1, "one editable native Shadow occurrence");
        let [x, y] = offsets[0];
        assert!(
            x.abs() < 1e-9 && (y - 40.0).abs() < 1e-9,
            "native Distance20 Direction90 under Scale200 Rotation90 is screen [0,40], got [{x},{y}]"
        );
    }
}
