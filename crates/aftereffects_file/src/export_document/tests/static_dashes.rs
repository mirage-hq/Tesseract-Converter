use super::*;
use crate::{properties, rifx::Chunk};

fn dash_records(chunks: &[Chunk]) -> Option<Vec<(String, f64)>> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == "ADBE Vector Stroke Dashes" {
                let group = properties::unique_list(run, *b"tdgp").unwrap();
                return Some(
                    properties::runs(group)
                        .unwrap()
                        .into_iter()
                        .map(|(name, run)| {
                            let storage = properties::unique_list(run, *b"tdbs").unwrap();
                            let value = properties::read_numeric(storage).unwrap();
                            assert!(!value.animated);
                            (name.to_owned(), value.values[0])
                        })
                        .collect(),
                );
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(dash_records)
}

fn document(kind: &str, dashes: &[f64], offset: f64) -> Value {
    // The unchanged pinned Solid supplies metadata, not a native dash oracle.
    // All content below is explicit edited FX input for supplemental assertions.
    let mut value = imported();
    let mut layer = rect(&value, 210);
    layer["name"] = json!("Edited dashes");
    layer["transform"] = value["composition"]["layers"][0]["transform"].clone();
    let strokes = json!([{
        "paint":{"type":"solid","color":[0.2,0.4,0.6,1.0]},
        "width":6.0, "dashes":dashes, "dashOffset":offset
    }]);
    if kind == "Rect" {
        layer["rect"]["fillEnabled"] = json!(false);
        layer["rect"]["strokeEnabled"] = json!(true);
        layer["rect"]["strokeColor"] = json!([0.2, 0.4, 0.6, 1.0]);
        layer["rect"]["strokeWidth"] = json!(6.0);
        layer["rect"]["strokeDashes"] = json!(dashes);
        layer["rect"]["strokeDashOffset"] = json!(offset);
    } else {
        layer["type"] = json!("Shape");
        layer.as_object_mut().unwrap().remove("rect");
        layer["shape"] = json!({
            "path":{"commands":[
                {"type":"moveTo","x":0.0,"y":0.0},
                {"type":"lineTo","x":100.0,"y":0.0},
                {"type":"lineTo","x":100.0,"y":80.0},
                {"type":"close"}
            ]}, "fills":[], "strokes":strokes
        });
        if kind == "Boolean" {
            let mut operand = layer.clone();
            operand["id"] = json!(211);
            operand["parent"] = json!(210);
            operand["shape"]["strokes"] = json!([]);
            let mut other = operand.clone();
            other["id"] = json!(212);
            layer = json!({
                "type":"BooleanOperation", "id":210, "name":"Edited dashes",
                "parent":null, "activeRange":layer["activeRange"],
                "transform":layer["transform"], "op":"union", "layers":[operand,other],
                "fills":[], "strokes":strokes
            });
        }
    }
    value["composition"]["layers"] = json!([layer]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    value
}

fn imported_pattern(layers: &[Layer]) -> Option<(Vec<f64>, f64)> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Group(group) => imported_pattern(&group.layers),
        LayerData::Rect(rect) if rect.rect.stroke_enabled => Some((
            rect.rect
                .stroke_dashes
                .iter()
                .map(|dash| dash.value())
                .collect(),
            rect.rect.stroke_dash_offset,
        )),
        LayerData::Shape(shape) => shape.shape.strokes.first().map(|stroke| {
            (
                stroke.dashes.iter().map(|dash| dash.value()).collect(),
                stroke.dash_offset,
            )
        }),
        LayerData::BooleanOperation(boolean) => boolean.strokes.first().map(|stroke| {
            (
                stroke.dashes.iter().map(|dash| dash.value()).collect(),
                stroke.dash_offset,
            )
        }),
        _ => None,
    })
}

#[test]
fn fresh_rect_shape_and_boolean_keep_static_dash_pairs_and_phase() {
    for kind in ["Rect", "Shape", "Boolean"] {
        for dashes in [
            vec![12.0, 6.0],
            vec![12.0, 6.0, 3.0, 2.0],
            vec![12.0, 6.0, 3.0, 2.0, 8.0, 4.0],
        ] {
            for offset in [-3.5, 0.0, 7.25] {
                let output = export(document(kind, &dashes, offset));
                let native = read_project(&output.bytes).unwrap();
                assert_eq!(layers(&native).len(), 1, "{kind}: {:?}", output.diagnostics);
                let records = dash_records(&layers(&native)[0].content).unwrap();
                let mut expected: Vec<_> = dashes
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        let role = if index % 2 == 0 { "Dash" } else { "Gap" };
                        (
                            format!("ADBE Vector Stroke {role} {}", index / 2 + 1),
                            *value,
                        )
                    })
                    .collect();
                if offset != 0.0 {
                    expected.push(("ADBE Vector Stroke Offset".into(), offset));
                }
                assert_eq!(records, expected);
                let roundtrip = to_structural_fx_document(&native, Some(1)).unwrap();
                assert_eq!(
                    imported_pattern(roundtrip.document.composition().layers()),
                    Some((dashes.clone(), offset))
                );
            }
        }
        let output = export(document(kind, &[], 0.0));
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
        assert!(dash_records(&layers(&native)[0].content).is_none());
    }
}

#[test]
fn unmapped_patterns_and_phase_motion_keep_supported_siblings() {
    for kind in ["Rect", "Shape", "Boolean"] {
        for (pattern, offset, animated) in [
            (vec![4.0], 0.0, false),
            (vec![4.0, 0.0], 0.0, false),
            (vec![4.0; 8], 0.0, false),
            (vec![], 3.0, false),
            (vec![4.0, 2.0], 0.0, true),
        ] {
            let mut value = document(kind, &pattern, offset);
            let sibling = rect(&imported(), 220);
            value["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .push(sibling);
            if animated {
                value["composition"]["dynamics"] = json!({"entries":[keyed_entry(
                    LayerId::new(210), PropType::StrokeDashOffset,
                    [(0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(3.0))],
                )]});
            }
            let output = export(value);
            let native = read_project(&output.bytes).unwrap();
            if animated {
                assert_eq!(layers(&native).len(), 2, "{kind}: {:?}", output.diagnostics);
                let dashed = layers(&native)
                    .iter()
                    .find(|layer| layer.name.as_ref() == "Edited dashes")
                    .expect("animated dash phase remains editable");
                let offset =
                    super::stroke_keys::numeric(&dashed.content, "ADBE Vector Stroke Offset")
                        .unwrap();
                assert!(offset.animated);
                assert_eq!(offset.keyframes.len(), 2);
                assert_eq!(offset.keyframes[0].values, [0.0]);
                assert_eq!(offset.keyframes[1].values, [3.0]);
            } else {
                assert_eq!(layers(&native).len(), 1, "{kind}: {:?}", output.diagnostics);
                assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 220");
                assert!(
                    output
                        .diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(210)))
                );
            }
        }
    }
}
