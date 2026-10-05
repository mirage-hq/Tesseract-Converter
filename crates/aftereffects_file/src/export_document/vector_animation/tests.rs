//! Supplemental FX → AEP structural tests. The native group-property contract
//! is independently visible in shapes/import_group_transform_controls.aep
//! (compositions 137 and 167); our own reader of a fresh export is not Adobe
//! acceptance or an independent render comparison.

use super::*;
use crate::{
    properties,
    rifx::Chunk,
    structure::{ItemKind, read_project},
    structure_document::to_structural_fx_document,
};
use fx_schema::{EditableFxCompositionDocument, LayerData};
use serde_json::{Value, json};

fn source_shape() -> Value {
    // Synthetic FX regression for the omitted owner ID, not an import of the
    // showreel's 40105 Shape or independent proof of its rendered output.
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    let mut value = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut shape =
        value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    shape["type"] = json!("Shape");
    shape["id"] = json!(40_105);
    shape["name"] = json!("Editable keyed skew and painted geometry");
    shape["parent"] = Value::Null;
    shape.as_object_mut().unwrap().remove("rect");
    shape["shape"] = json!({
        "path":{"commands":[
            {"type":"moveTo","x":0.0,"y":0.0},
            {"type":"lineTo","x":90.0,"y":0.0},
            {"type":"lineTo","x":90.0,"y":80.0},
            {"type":"close"}
        ]},
        "fills":[{"paint":{"type":"solid","color":[1.0,0.5,0.2,1.0]}}],
        "strokes":[]
    });
    shape["transform"]["skew"] = json!(0.0);
    shape["transform"]["skewAxis"] = json!(0.0);
    value["composition"]["layers"] = json!([shape]);
    value["composition"]["dynamics"] = json!({"entries":[
        {"target":{"kind":"layer","layerId":40105,"propertyType":"skew"},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                {"id":"sk0","layerTime":0,"value":{"type":"float","value":-12.0},"easing":{"type":"linear"}},
                {"id":"sk1","layerTime":240,"value":{"type":"float","value":20.0},"easing":{"type":"cubicBezier","x1":0.16,"y1":1.0,"x2":0.3,"y2":1.0}}
            ]}},
        {"target":{"kind":"layer","layerId":40105,"propertyType":"skewAxis"},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                {"id":"sa0","layerTime":0,"value":{"type":"float","value":5.0},"easing":{"type":"linear"}},
                {"id":"sa1","layerTime":240,"value":{"type":"float","value":30.0},"easing":{"type":"linear"}}
            ]}},
        {"target":{"kind":"layer","layerId":40105,"propertyType":"fillColor"},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                {"id":"fc0","layerTime":0,"value":{"type":"color","value":[1.0,0.5,0.2,1.0]},"easing":{"type":"linear"}},
                {"id":"fc1","layerTime":240,"value":{"type":"color","value":[0.2,0.5,1.0,1.0]},"easing":{"type":"linear"}}
            ]}}
    ]});
    value
}

fn numeric(chunks: &[Chunk], target: &str) -> Option<properties::NumericProperty> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                let storage = properties::unique_list(run, *b"tdbs").unwrap();
                let property = properties::read_numeric(storage).unwrap();
                // Boolean operands also have static Transform controls. The
                // regression concerns the source owner's keyed control.
                if property.animated {
                    return Some(property);
                }
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| numeric(children, target))
}

#[test]
fn source_owned_skew_keys_survive_fresh_vector_writer_with_zero_static_skew() {
    let value = source_shape();
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let entries = document.composition().dynamics().entries();
    let LayerData::Shape(shape) = document.composition().layers()[0].data() else {
        panic!("source Shape");
    };
    let owner = LayerId::new(40_105);
    assert!(has_skew_tracks(
        &crate::export_document::AnimationIndex::new(entries),
        owner
    ));
    let base = &shape.transform;
    let keys = program_transform_animations(
        &crate::export_document::AnimationIndex::new(entries),
        owner,
        base,
    )
    .unwrap();
    assert_eq!(keys.skew.as_ref().unwrap().keys[0].values, [-12.0]);
    assert_eq!(keys.skew_axis.as_ref().unwrap().keys[1].values, [30.0]);
    assert_eq!(keys.rotation, None);

    let exported = super::super::to_aep(&document).unwrap();
    assert!(
        exported
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.layer_id != Some(owner)),
        "{:?}",
        exported.diagnostics
    );
    let fresh = read_project(&exported.bytes).unwrap();
    let ItemKind::Composition(root) = &fresh.item(1).unwrap().kind else {
        panic!("fresh native composition");
    };
    assert_eq!(root.layers.len(), 1, "the Shape must not be omitted");
    let content = &root.layers[0].content;
    for (property, expected) in [
        ("ADBE Vector Skew", [-12.0, 20.0]),
        ("ADBE Vector Skew Axis", [5.0, 30.0]),
    ] {
        let track = numeric(content, property).expect("source-owned native vector property");
        assert!(track.animated, "{property}");
        assert_eq!(track.keyframes.len(), 2, "{property}");
        for (key, (time, value)) in track
            .keyframes
            .iter()
            .zip([(0.0, expected[0]), (0.24, expected[1])])
        {
            assert!((key.time_secs - time).abs() < 1.0 / 24_576.0, "{property}");
            assert!((key.values[0] - value).abs() < 1e-6, "{property}");
        }
    }
    assert!(String::from_utf8_lossy(&exported.bytes).contains("ADBE Vector Shape - Group"));
    assert_eq!(
        numeric(content, "ADBE Vector Fill Color")
            .unwrap()
            .keyframes
            .len(),
        2
    );
}

#[test]
fn rectangle_solid_hint_and_boolean_keep_zero_base_skew_keys() {
    for boolean in [false, true] {
        let mut value = source_shape();
        let owner = &mut value["composition"]["layers"][0];
        if boolean {
            let mut first = owner.clone();
            first["id"] = json!(40106);
            first["parent"] = json!(40105);
            let mut second = first.clone();
            second["id"] = json!(40107);
            second["transform"]["position"] = json!([20.0, 0.0]);
            owner["type"] = json!("BooleanOperation");
            owner["op"] = json!("union");
            owner["layers"] = json!([first, second]);
            owner["fills"] = owner["shape"]["fills"].clone();
            owner["strokes"] = json!([]);
        } else {
            let native = read_project(include_bytes!(
                "../../../tests/fixtures/properties/transform_unseparated.aep"
            ))
            .unwrap();
            let original = to_structural_fx_document(&native, Some(1))
                .unwrap()
                .document
                .to_json_value()
                .unwrap();
            owner["type"] = json!("Rect");
            owner["description"] = json!("Editable AE solid; keyed skew needs a vector program");
            owner["rect"] =
                original["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0]["rect"]
                    .clone();
        }
        owner.as_object_mut().unwrap().remove("shape");
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let exported = super::super::to_aep(&document).unwrap();
        let fresh = read_project(&exported.bytes).unwrap();
        let ItemKind::Composition(root) = &fresh.item(1).unwrap().kind else {
            panic!("fresh native composition");
        };
        assert_eq!(
            root.layers.len(),
            1,
            "boolean={boolean}: {:?}",
            exported.diagnostics
        );
        let content = &root.layers[0].content;
        let skew = numeric(content, "ADBE Vector Skew").unwrap();
        assert_eq!(skew.keyframes.len(), 2);
        assert_eq!(skew.keyframes[0].values, [-12.0]);
        assert_eq!(skew.keyframes[1].values, [20.0]);
        assert_eq!(
            numeric(content, "ADBE Vector Skew Axis")
                .unwrap()
                .keyframes
                .len(),
            2
        );
    }
}

#[test]
fn keyed_skew_keeps_native_3d_and_position_partition_guards() {
    let mut value = source_shape();
    let document = EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let LayerData::Shape(shape) = document.composition().layers()[0].data() else {
        panic!("source Shape");
    };
    assert!(
        super::super::transform_animations_partitioned(
            &crate::export_document::AnimationIndex::new(
                document.composition().dynamics().entries()
            ),
            shape.id,
            &shape.transform,
            shape.id,
            true,
            true,
        )
        .is_err(),
        "native 3D must not silently consume vector Skew keys"
    );
    let mut position = value["composition"]["dynamics"]["entries"][0].clone();
    position["target"]["propertyType"] = json!("positionX");
    for (index, key) in position["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        key["id"] = json!(format!("position-{index}"));
    }
    value["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(position);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let exported = super::super::to_aep(&document).unwrap();
    assert!(exported.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(40105))
            && diagnostic
                .message
                .contains("Animated Anchor/Position/Rotation combined with Skew")
    }));
    let fresh = read_project(&exported.bytes).unwrap();
    let ItemKind::Composition(root) = &fresh.item(1).unwrap().kind else {
        panic!("root")
    };
    assert!(
        root.layers.is_empty(),
        "unsupported keys must not become static output"
    );
}

#[test]
fn pinned_native_vector_group_skew_keys_identify_editable_property() {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/shapes/import_group_transform_controls.aep"
    ))
    .unwrap();
    for (composition_id, property) in [(137, "ADBE Vector Skew"), (167, "ADBE Vector Skew Axis")] {
        let ItemKind::Composition(composition) = &native.item(composition_id).unwrap().kind else {
            panic!("pinned native composition {composition_id}");
        };
        assert!(
            composition.layers.iter().any(|layer| {
                numeric(&layer.content, property)
                    .is_some_and(|value| value.animated && value.keyframes.len() >= 2)
            }),
            "{composition_id}/{property}: native group must own its keyed control"
        );
    }
}

#[test]
fn keyed_skew_helper_writes_native_vector_properties_without_baking() {
    use crate::writer::{
        CompositionSpec, GeometryAnimations, LayerSpec, SolidTransform, TransformAnimations,
        VectorContent, VectorGeometry, VectorGroupSpec, VectorGroupTransform, VectorLayerSpec,
        write_composition,
    };
    let document = EditableFxCompositionDocument::from_json_value(source_shape()).unwrap();
    let LayerData::Shape(shape) = document.composition().layers()[0].data() else {
        panic!("source Shape");
    };
    let entries = document.composition().dynamics().entries();
    let keys = program_transform_animations(
        &crate::export_document::AnimationIndex::new(entries),
        shape.id,
        &shape.transform,
    )
    .unwrap();
    let group = VectorGroupSpec {
        name: "Source-owned Transform".into(),
        blend_mode: Default::default(),
        transform: VectorGroupTransform {
            anchor: shape.transform.anchor_point,
            position: match shape.transform.position {
                Position::TwoD(value) => value,
                Position::ThreeD(_) => panic!("fixture is 2D"),
            },
            scale: shape.transform.scale,
            skew: shape.transform.skew,
            skew_axis: shape.transform.skew_axis,
            rotation: shape.transform.rotation,
            opacity: shape.transform.opacity.value(),
        },
        contents: vec![VectorContent::Geometry {
            geometry: VectorGeometry::Ellipse(fx_schema::ShapeEllipse::default()),
            animations: GeometryAnimations::default(),
        }],
    };
    let bytes = write_composition(
        &CompositionSpec {
            name: "Keyed skew".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[LayerSpec::VectorProgram(VectorLayerSpec {
            name: "Editable source".into(),
            transform: SolidTransform {
                anchor: [0.0; 2],
                position: [0.0; 2],
                scale: [100.0; 2],
                rotation: 0.0,
                opacity: 100.0,
            },
            transform_animations: TransformAnimations::default(),
            contents: vec![VectorContent::AnimatedGroup(group, keys)],
        })],
    )
    .unwrap();
    let fresh = read_project(&bytes).unwrap();
    let ItemKind::Composition(composition) = &fresh.item(1).unwrap().kind else {
        panic!("fresh composition");
    };
    for (property, values) in [
        ("ADBE Vector Skew", [-12.0, 20.0]),
        ("ADBE Vector Skew Axis", [5.0, 30.0]),
    ] {
        let track = numeric(&composition.layers[0].content, property).unwrap();
        assert_eq!(track.keyframes.len(), 2, "{property}: no dense frame bake");
        assert_eq!(track.keyframes[0].values, [values[0]]);
        assert_eq!(track.keyframes[1].values, [values[1]]);
        assert!((track.keyframes[1].time_secs - 0.24).abs() < 1.0 / 24_576.0);
    }
}

#[test]
fn owned_transform_keys_do_not_consume_geometry_or_foreign_skew() {
    let value = source_shape();
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let owner = LayerId::new(40_105);
    let mut entries = document.composition().dynamics().entries().to_vec();
    let mut foreign = entries[0].clone();
    foreign.target = fx_schema::PropertyTarget::layer(LayerId::new(40_106), PropType::Skew);
    entries.push(foreign);
    let LayerData::Shape(shape) = document.composition().layers()[0].data() else {
        panic!("source Shape");
    };
    let base = &shape.transform;
    let keys = program_transform_animations(
        &crate::export_document::AnimationIndex::new(&entries),
        owner,
        base,
    )
    .unwrap();
    assert_eq!(keys.skew.as_ref().unwrap().keys.len(), 2);
    assert!(!has_skew_tracks(
        &crate::export_document::AnimationIndex::new(&entries),
        LayerId::new(40_107)
    ));
    let duplicate = entries[0].clone();
    entries.push(duplicate);
    assert_eq!(
        program_transform_animations(
            &crate::export_document::AnimationIndex::new(&entries),
            owner,
            base
        ),
        Err("Duplicate source-owned Transform animator cannot be written natively")
    );
}
