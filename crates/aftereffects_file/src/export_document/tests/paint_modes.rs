use super::*;
use crate::rifx::Chunk;

fn native_numeric(chunks: &[Chunk], name: &str) -> Option<crate::properties::NumericProperty> {
    if let Ok(runs) = crate::properties::runs(chunks) {
        for (candidate, run) in runs {
            if candidate == name {
                let storage = crate::properties::unique_list(run, *b"tdbs").unwrap();
                return Some(crate::properties::read_numeric(storage).unwrap());
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| native_numeric(children, name))
}

fn native_scalar(chunks: &[Chunk], name: &str) -> Option<f64> {
    native_numeric(chunks, name).and_then(|value| {
        assert!(!value.animated);
        value.values.first().copied()
    })
}

fn painted_layer(value: &Value, boolean: bool, fills: Value, strokes: Value) -> Value {
    let mut shape = rect(value, 210);
    shape["type"] = json!("Shape");
    shape["name"] = json!("Paint mode");
    shape["transform"] = value["composition"]["layers"][0]["transform"].clone();
    shape.as_object_mut().unwrap().remove("rect");
    shape["shape"] = json!({
        "path": {"commands": [
            {"type":"moveTo","x":0.0,"y":0.0},
            {"type":"lineTo","x":50.0,"y":0.0},
            {"type":"lineTo","x":50.0,"y":40.0}
        ]},
        "fills":fills, "strokes":strokes
    });
    if !boolean {
        return shape;
    }
    let mut other = shape.clone();
    shape["id"] = json!(211);
    other["id"] = json!(212);
    for child in [&mut shape, &mut other] {
        child["parent"] = json!(210);
        child["shape"]["fills"] = json!([]);
        child["shape"]["strokes"] = json!([]);
    }
    json!({
        "type":"BooleanOperation", "id":210, "name":"Paint mode",
        "parent":null, "activeRange":shape["activeRange"],
        "transform":shape["transform"], "op":"union", "layers":[shape,other],
        "fills":fills, "strokes":strokes
    })
}

#[test]
fn edited_shape_and_boolean_export_native_static_paint_modes() {
    // Explicit edited FX inputs and our native reader are supplemental structure
    // assertions, not independently authored AE fixtures or Adobe render proof.
    for boolean in [false, true] {
        for (field, mode, source_name, ordinal) in [
            ("fillRule", "nonZeroWinding", "ADBE Vector Fill Rule", 1.0),
            ("fillRule", "evenOdd", "ADBE Vector Fill Rule", 2.0),
            ("cap", "butt", "ADBE Vector Stroke Line Cap", 1.0),
            ("cap", "round", "ADBE Vector Stroke Line Cap", 2.0),
            ("cap", "square", "ADBE Vector Stroke Line Cap", 3.0),
        ] {
            let mut value = imported();
            let mut paint = json!({
                "paint":{"type":"solid","color":[0.3,0.6,0.2,1.0]}
            });
            paint[field] = json!(mode);
            let (fills, strokes) = if field == "fillRule" {
                (json!([paint]), json!([]))
            } else {
                paint["width"] = json!(7.0);
                (json!([]), json!([paint]))
            };
            let layer = painted_layer(&value, boolean, fills, strokes);
            value["composition"]["layers"] = json!([layer]);
            value["composition"]["dynamics"] = json!({"entries":[]});
            let output = export(value);
            let project = read_project(&output.bytes).unwrap();
            assert_eq!(
                layers(&project).len(),
                1,
                "{mode}: {:?}",
                output.diagnostics
            );
            assert_eq!(layers(&project)[0].name.as_ref(), "Paint mode");
            assert_eq!(
                native_scalar(&layers(&project)[0].content, source_name),
                Some(ordinal),
                "boolean={boolean}, mode={mode}"
            );
        }
    }
}

#[test]
fn integrated_caps_dashes_paint_opacity_and_layer_keys_keep_separate_records() {
    // Cross-slice regression code only; no execution or independent Adobe proof.
    for boolean in [false, true] {
        for (cap, ordinal) in [("round", 2.0), ("square", 3.0)] {
            let mut value = imported();
            let strokes = json!([{
                "paint":{"type":"solid","color":[0.3,0.6,0.2,0.5]},
                "width":7.0, "cap":cap, "dashes":[4.0,2.0],
                "dashOffset":-3.0, "opacity":0.25
            }]);
            let layer = painted_layer(&value, boolean, json!([]), strokes);
            value["composition"]["layers"] = json!([layer]);
            value["composition"]["dynamics"] = json!({"entries":[
                keyed_entry(LayerId::new(210), PropType::StrokeWidth,
                    [(0, PropertyValue::Float(2.0)), (500, PropertyValue::Float(10.0))]),
                keyed_entry(LayerId::new(210), PropType::StrokeMiterLimit,
                    [(0, PropertyValue::Float(4.0)), (500, PropertyValue::Float(12.0))]),
                keyed_entry(LayerId::new(210), PropType::Opacity,
                    [(0, PropertyValue::Float(20.0)), (500, PropertyValue::Float(70.0))])
            ]});
            let output = export(value);
            let native = read_project(&output.bytes).unwrap();
            assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
            let content = &layers(&native)[0].content;
            for (name, expected) in [
                ("ADBE Vector Stroke Line Cap", ordinal),
                ("ADBE Vector Stroke Dash 1", 4.0),
                ("ADBE Vector Stroke Gap 1", 2.0),
                ("ADBE Vector Stroke Offset", -3.0),
                ("ADBE Vector Stroke Opacity", 25.0),
            ] {
                assert_eq!(native_scalar(content, name), Some(expected));
            }
            for (name, expected) in [
                ("ADBE Vector Stroke Width", [2.0, 10.0]),
                ("ADBE Vector Stroke Miter Limit", [4.0, 12.0]),
                ("ADBE Opacity", [0.2, 0.7]),
            ] {
                let keys = native_numeric(content, name).unwrap();
                assert_eq!(keys.keyframes.len(), 2);
                for (key, (time, value)) in keys
                    .keyframes
                    .iter()
                    .zip([0.0, 0.5].into_iter().zip(expected))
                {
                    assert_eq!(key.time_secs, time);
                    assert_eq!(key.values, vec![value]);
                }
            }
            assert_eq!(
                native_numeric(content, "ADBE Vector Stroke Color")
                    .unwrap()
                    .values,
                vec![0.3, 0.6, 0.2, 0.5]
            );
        }
    }
}

#[test]
fn accepting_cap_modes_does_not_silently_drop_odd_dash_patterns() {
    for boolean in [false, true] {
        let mut value = imported();
        let strokes = json!([{
            "paint":{"type":"solid","color":[0.3,0.6,0.2,1.0]},
            "width":7.0, "cap":"round", "dashes":[4.0,2.0,1.0]
        }]);
        let unsupported = painted_layer(&value, boolean, json!([]), strokes);
        let sibling = rect(&value, 220);
        value["composition"]["layers"] = json!([unsupported, sibling]);
        value["composition"]["dynamics"] = json!({"entries":[]});
        let output = export(value);
        let project = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&project).len(), 1);
        assert_eq!(layers(&project)[0].name.as_ref(), "Current solid 220");
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(210))
                && diagnostic
                    .message
                    .contains("complete positive finite Dash/Gap pairs")
        }));
    }
}
