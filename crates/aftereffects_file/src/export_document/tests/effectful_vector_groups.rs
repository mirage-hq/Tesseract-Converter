//! Synthetic structural coverage for effectful Groups lowered as one native Shape.
//! These tests intentionally do not copy private source contours or establish Adobe fidelity.

use super::*;
use crate::{effects::native::read_effects, properties, rifx::Chunk};
use fx_schema::layer::ShapePathCommand;

const TEMPLATE: &str = include_str!("../../../tests/fixtures/effects_coverage/template.fx.json");
const GROUP_ID: u64 = 41_000;
const ROW_A_ID: u64 = 41_010;
const ROW_B_ID: u64 = 41_011;
const RAY_ID: u64 = 41_005;

fn identity_transform() -> Value {
    json!({
        "anchorPoint":[0.0,0.0], "position":[0.0,0.0], "scale":[100.0,100.0],
        "rotation":0.0, "rotationX":0.0, "rotationY":0.0,
        "orientation":[0.0,0.0,0.0], "skew":0.0, "skewAxis":0.0, "opacity":100.0
    })
}

fn shape(id: u64, name: &str, commands: Value) -> Value {
    json!({
        "type":"Shape", "id":id, "name":name, "parent":GROUP_ID,
        "blendMode":"normal", "trackMatte":null, "masks":[],
        "activeRange":{"start":0,"duration":2000}, "effects":[], "motionBlur":false,
        "transform":identity_transform(),
        "shape":{
            "path":{"commands":commands}, "fills":[],
            "strokes":[{"paint":{"type":"solid","color":[0.2,0.8,1.0,1.0]},"width":2.5}]
        }
    })
}

fn synthetic_group(animated_skew: bool, exposure: bool) -> Value {
    let mut document: Value = serde_json::from_str(TEMPLATE).expect("test document template");
    document["composition"]["name"] = json!(if animated_skew {
        "Synthetic effectful animated vector Group"
    } else {
        "Synthetic effectful fixed-skew vector Group"
    });
    let mut row_a = shape(
        ROW_A_ID,
        "Synthetic row A",
        json!([
            {"type":"moveTo","x":-240.0,"y":-45.0},
            {"type":"lineTo","x":280.0,"y":-12.0}
        ]),
    );
    row_a["shape"]["trim"] = json!({"start":5.0,"end":95.0,"offset":12.0,"mode":"simultaneously"});
    let row_b = shape(
        ROW_B_ID,
        "Synthetic row B",
        json!([
            {"type":"moveTo","x":-310.0,"y":35.0},
            {"type":"cubicTo","c1x":-80.0,"c1y":5.0,"c2x":130.0,"c2y":75.0,"x":340.0,"y":42.0}
        ]),
    );
    let mut ray = shape(
        RAY_ID,
        "Synthetic 65520 ray",
        json!([
            {"type":"moveTo","x":-32760.0,"y":610.0},
            {"type":"lineTo","x":-120.0,"y":20.0},
            {"type":"lineTo","x":32760.0,"y":-590.0}
        ]),
    );
    ray["blendMode"] = json!("add");
    ray["transform"]["skew"] = json!(if animated_skew { 0.0 } else { -6.5 });

    let mut effects = vec![json!({
        "id":40_004,"enabled":true,
        "effect":{"type":"glow","glowThreshold":20.0,"glowRadius":10.0,"glowIntensity":1.25}
    })];
    if exposure {
        effects.push(json!({
            "id":40_003,"enabled":true,
            "effect":{"type":"exposure","exposure":0.25,"offset":0.1,"gammaCorrection":0.9}
        }));
    }
    document["composition"]["layers"] = json!([{
        "type":"Group", "id":GROUP_ID, "name":"Synthetic effectful vector owner",
        "parent":null, "blendMode":"normal", "trackMatte":null, "masks":[],
        "playback":fixture_linear_playback(json!({"start":0,"duration":2000}), json!({"start":0,"duration":2000})),
        "effects":effects, "motionBlur":false, "transform":identity_transform(),
        "layers":[row_a,row_b,ray]
    }]);

    let mut entries = Vec::new();
    for (id, start_y, end_y, start_scale, end_scale, start_opacity, end_opacity) in [
        (ROW_A_ID, -45.0, -12.0, 82.0, 118.0, 25.0, 90.0),
        (ROW_B_ID, 35.0, 64.0, 76.0, 109.0, 15.0, 75.0),
    ] {
        entries.push(keyed_entry(
            LayerId::new(id),
            PropType::PositionY,
            [
                (0, PropertyValue::Float(start_y)),
                (1000, PropertyValue::Float(end_y)),
            ],
        ));
        entries.push(keyed_entry(
            LayerId::new(id),
            PropType::ScaleY,
            [
                (0, PropertyValue::Float(start_scale)),
                (1000, PropertyValue::Float(end_scale)),
            ],
        ));
        entries.push(keyed_entry(
            LayerId::new(id),
            PropType::Opacity,
            [
                (0, PropertyValue::Float(start_opacity)),
                (1000, PropertyValue::Float(end_opacity)),
            ],
        ));
    }
    entries.push(keyed_entry(
        LayerId::new(ROW_A_ID),
        PropType::StrokeWidth,
        [
            (0, PropertyValue::Float(2.5)),
            (1000, PropertyValue::Float(5.5)),
        ],
    ));
    entries.push(keyed_entry(
        LayerId::new(ROW_A_ID),
        PropType::TrimStart,
        [
            (0, PropertyValue::Float(5.0)),
            (1000, PropertyValue::Float(40.0)),
        ],
    ));
    if animated_skew {
        entries.push(keyed_entry(
            LayerId::new(RAY_ID),
            PropType::Skew,
            [
                (0, PropertyValue::Float(-4.0)),
                (1000, PropertyValue::Float(7.0)),
            ],
        ));
    }
    let mut entries = entries
        .into_iter()
        .map(|entry| serde_json::to_value(entry).expect("animation entry"))
        .collect::<Vec<_>>();
    if exposure {
        entries.push(json!({
            "target":{"kind":"effectProperty","effectId":40_003,"paramName":"exposure"},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                {"id":"exposure-start","layerTime":0,"value":{"type":"float","value":0.25},"easing":{"type":"linear"}},
                {"id":"exposure-end","layerTime":1000,"value":{"type":"float","value":1.5},"easing":{"type":"linear"}}
            ]}, "dependencies":[], "layerRefs":{}
        }));
    }
    document["composition"]["dynamics"]["entries"] = Value::Array(entries);
    document
}

fn native_output(value: Value) -> (ExportedDocument, StructuralProject) {
    assert!(!serde_json::to_string(&value).unwrap().contains("jsScript"));
    let output = export(value);
    let native = read_project(&output.bytes).expect("fresh native project");
    (output, native)
}

fn numeric_properties(
    chunks: &[Chunk],
    target: &str,
    output: &mut Vec<properties::NumericProperty>,
) {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target
                && let Ok(storage) = properties::unique_list(run, *b"tdbs")
                && let Ok(property) = properties::read_numeric(storage)
            {
                output.push(property);
            }
            numeric_properties(run, target, output);
        }
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        numeric_properties(children, target, output);
    }
}

fn numerics(chunks: &[Chunk], target: &str) -> Vec<properties::NumericProperty> {
    let mut output = Vec::new();
    numeric_properties(chunks, target, &mut output);
    output
}

fn shape_x_span(shape: &ShapeLayer) -> Option<(f64, f64)> {
    let values = shape
        .shape
        .path
        .commands
        .iter()
        .flat_map(|command| match command {
            ShapePathCommand::MoveTo { x, .. } | ShapePathCommand::LineTo { x, .. } => vec![*x],
            ShapePathCommand::CubicTo { c1x, c2x, x, .. } => vec![*c1x, *c2x, *x],
            ShapePathCommand::Close => Vec::new(),
        });
    values.fold(None, |bounds, x| {
        Some(bounds.map_or((x, x), |(min, max): (f64, f64)| (min.min(x), max.max(x))))
    })
}

fn find_widest_shape(layers: &[Layer]) -> Option<&ShapeLayer> {
    layers
        .iter()
        .filter_map(|layer| match layer.data() {
            LayerData::Shape(shape) => Some(shape),
            LayerData::Group(group) => find_widest_shape(&group.layers),
            _ => None,
        })
        .max_by(|left, right| {
            let left = shape_x_span(left).map_or(0.0, |(min, max)| max - min);
            let right = shape_x_span(right).map_or(0.0, |(min, max)| max - min);
            left.total_cmp(&right)
        })
}

fn find_add_group(layers: &[Layer]) -> Option<&GroupLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Group(group) if group.blend_mode == fx_schema::BlendMode::Add => Some(group),
        LayerData::Group(group) => find_add_group(&group.layers),
        _ => None,
    })
}

fn assert_native_vector_group(value: Value, exposure: bool) {
    let (output, native) = native_output(value);
    let [layer] = layers(&native) else {
        panic!(
            "expected one source-less native Shape: {:?}",
            output.diagnostics
        );
    };
    assert_eq!(layer.name.as_ref(), "Synthetic effectful vector owner");
    assert_eq!(layer.record.layer_type(), 4, "native Shape layer");
    assert_eq!(
        layer.record.source_id(),
        0,
        "no generated precomposition source"
    );
    assert!(
        output.diagnostics.iter().all(|diagnostic| {
            !diagnostic.message.contains("exceed the native canvas")
                && !diagnostic.message.contains("bounds cannot")
        }),
        "{:?}",
        output.diagnostics
    );

    let (effects, warnings) = read_effects(&layer.content, [320.0, 180.0]);
    assert!(
        warnings.iter().all(|warning| !warning.contains("missing")),
        "{warnings:?}"
    );
    let expected = if exposure {
        vec!["ADBE Glo2", "ADBE Exposure2"]
    } else {
        vec!["ADBE Glo2"]
    };
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    if exposure {
        let exposure = effects[1]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Exposure2-0003")
            .expect("native Exposure control")
            .numeric
            .as_ref()
            .expect("numeric Exposure control");
        assert!(exposure.animated);
        assert_eq!(exposure.keyframes.len(), 2);
        assert_eq!(exposure.keyframes[0].values, [0.25]);
        assert_eq!(exposure.keyframes[1].values, [1.5]);
    }

    let positions = numerics(&layer.content, "ADBE Vector Position");
    let scales = numerics(&layer.content, "ADBE Vector Scale");
    let opacities = numerics(&layer.content, "ADBE Vector Group Opacity");
    let animated_positions = positions
        .iter()
        .filter(|property| property.animated)
        .collect::<Vec<_>>();
    let animated_scales = scales
        .iter()
        .filter(|property| property.animated)
        .collect::<Vec<_>>();
    let animated_opacities = opacities
        .iter()
        .filter(|property| property.animated)
        .collect::<Vec<_>>();
    for property in animated_positions.iter().chain(&animated_scales) {
        assert!(
            property
                .keyframes
                .iter()
                .all(|key| { key.values.len() == 2 && [0.0, 1.0].contains(&key.time_secs) })
        );
    }
    for expected in [([0.0, -45.0], [0.0, -12.0]), ([0.0, 35.0], [0.0, 64.0])] {
        assert!(animated_positions.iter().any(|property| {
            property.keyframes[0].values == expected.0 && property.keyframes[1].values == expected.1
        }));
    }
    for expected in [
        ([100.0, 82.0], [100.0, 118.0]),
        ([100.0, 76.0], [100.0, 109.0]),
    ] {
        assert!(animated_scales.iter().any(|property| {
            property.keyframes[0].values == expected.0 && property.keyframes[1].values == expected.1
        }));
    }
    assert!(animated_opacities.iter().all(|property| {
        property
            .keyframes
            .iter()
            .all(|key| key.values.len() == 1 && [0.0, 1.0].contains(&key.time_secs))
    }));
    for expected in [([25.0], [90.0]), ([15.0], [75.0])] {
        assert!(animated_opacities.iter().any(|property| {
            property.keyframes[0].values == expected.0 && property.keyframes[1].values == expected.1
        }));
    }

    let stroke_width = numerics(&layer.content, "ADBE Vector Stroke Width");
    assert!(stroke_width.iter().any(|property| {
        property.animated
            && property.keyframes[0].values == [2.5]
            && property.keyframes[1].values == [5.5]
    }));
    let trim_start = numerics(&layer.content, "ADBE Vector Trim Start");
    assert!(trim_start.iter().any(|property| {
        property.animated
            && property.keyframes[0].values == [5.0]
            && property.keyframes[1].values == [40.0]
    }));

    let skew = numerics(&layer.content, "ADBE Vector Skew");
    if exposure {
        assert!(skew.iter().any(|property| {
            property.animated
                && property.keyframes[0].values == [-4.0]
                && property.keyframes[1].values == [7.0]
        }));
    } else {
        assert!(
            skew.iter()
                .any(|property| !property.animated && property.values == [-6.5])
        );
    }

    let encoded_name_positions =
        ["Synthetic row A", "Synthetic row B", "Synthetic 65520 ray"].map(|name| {
            output
                .bytes
                .windows(name.len())
                .position(|bytes| bytes == name.as_bytes())
                .unwrap_or_else(|| panic!("native vector Group name {name:?} missing"))
        });
    assert!(
        encoded_name_positions
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );

    let imported = to_structural_fx_document(&native, Some(1))
        .expect("own-reader structural inspection")
        .document;
    let ray = find_widest_shape(imported.composition().layers()).expect("editable synthetic ray");
    assert_eq!(shape_x_span(ray), Some((-32760.0, 32760.0)));
    let add = find_add_group(imported.composition().layers()).expect("child Add vector Group");
    assert_eq!(
        find_widest_shape(&add.layers).and_then(shape_x_span),
        Some((-32760.0, 32760.0))
    );
}

#[test]
fn effectful_vector_group_keeps_65520px_animated_skew_rails() {
    let value = synthetic_group(true, true);
    assert_native_vector_group(value, true);
}

#[test]
fn effectful_vector_group_keeps_full_fixed_skew_rails_before_glow() {
    let value = synthetic_group(false, false);
    assert_native_vector_group(value, false);
}

fn assert_effectful_inline_rejected(value: Value, label: &str) {
    let (output, native) = native_output(value);
    let selected = matches!(layers(&native), [layer]
        if layer.name.as_ref() == "Synthetic effectful vector owner"
            && layer.record.layer_type() == 4
            && layer.record.source_id() == 0);
    assert!(
        !selected,
        "{label} unexpectedly selected inline vector lowering: {:?}",
        output.diagnostics
    );
}

#[test]
fn effectful_vector_group_rejects_unscoped_child_semantics_transactionally() {
    let mut child_effect = synthetic_group(false, false);
    child_effect["composition"]["layers"][0]["layers"][0]["effects"] = json!([{
        "id":41_101,"enabled":true,
        "effect":{"type":"glow","glowThreshold":25.0,"glowRadius":4.0,"glowIntensity":1.0}
    }]);
    assert_effectful_inline_rejected(child_effect, "child effect");

    let mut child_mask = synthetic_group(false, false);
    child_mask["composition"]["layers"][0]["layers"][0]["masks"] = json!([{
        "id":41_102,"mode":"add","path":{"commands":[
            {"type":"moveTo","x":-10.0,"y":-10.0},
            {"type":"lineTo","x":10.0,"y":-10.0},
            {"type":"lineTo","x":0.0,"y":10.0},
            {"type":"close"}
        ]}
    }]);
    assert_effectful_inline_rejected(child_mask, "child mask");

    let mut child_matte = synthetic_group(false, false);
    child_matte["composition"]["layers"][0]["layers"][0]["trackMatte"] =
        json!({"mode":"alpha","layer":ROW_B_ID});
    assert_effectful_inline_rejected(child_matte, "child matte");

    let mut child_clock = synthetic_group(false, false);
    child_clock["composition"]["layers"][0]["layers"][0]["activeRange"]["duration"] = json!(1500);
    assert_effectful_inline_rejected(child_clock, "child source clock");

    let mut child_3d = synthetic_group(false, false);
    child_3d["composition"]["layers"][0]["layers"][0]["transform"]["position"] =
        json!([0.0, 0.0, 25.0]);
    assert_effectful_inline_rejected(child_3d, "child 3D Transform");

    let mut unsupported_blend = synthetic_group(false, false);
    unsupported_blend["composition"]["layers"][0]["layers"][2]["blendMode"] = json!("divide");
    assert_effectful_inline_rejected(unsupported_blend, "unsupported native vector blend");

    let mut external_reference = synthetic_group(false, false);
    let mut dependent = external_reference["composition"]["layers"][0]["layers"][1].clone();
    dependent["id"] = json!(41_200);
    dependent["name"] = json!("External dependent");
    dependent["parent"] = json!(RAY_ID);
    external_reference["composition"]["layers"]
        .as_array_mut()
        .expect("root layers")
        .push(dependent);
    assert_effectful_inline_rejected(external_reference, "externally referenced child identity");
}
