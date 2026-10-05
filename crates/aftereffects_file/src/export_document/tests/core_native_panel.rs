//! Explicit edited-FX inputs and manually specified native-control oracles.
//! These tests are UNRUN. Own-reader inspection is supplemental, not Adobe
//! acceptance, independent rendering, import fidelity, or a visual pass.
use super::*;
use crate::{properties, rifx::Chunk};

fn numeric(chunks: &[Chunk], target: &str) -> Option<properties::NumericProperty> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                return properties::read_numeric(properties::unique_list(run, *b"tdbs").ok()?).ok();
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| numeric(children, target))
}

fn near(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < 1e-5,
        "{label}: expected {expected}, got {actual}"
    );
}

fn values(actual: &[f64], expected: &[Value], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: component count");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        near(
            *actual,
            expected.as_f64().expect("manual numeric oracle"),
            &format!("{label}[{index}]"),
        );
    }
}

fn check_case(name: &str, input_json: &str, expected_json: &str) {
    let input: Value = serde_json::from_str(input_json).expect("committed edited FX input");
    let expected: Value =
        serde_json::from_str(expected_json).expect("manual native-control oracle");
    assert_eq!(input["composition"]["name"], name);
    assert_eq!(expected["status"], "AUTHORED_UNRUN");
    assert!(!input_json.contains("jsScript"));
    let dir = crate::adobe_test_support::artifact_directory();
    std::fs::create_dir_all(&dir).expect("artifact directory");
    let output = export(input);
    {
        let base = std::path::Path::new(&dir).join(name);
        std::fs::write(base.with_extension("fx.json"), input_json).expect("fresh FX artifact");
        std::fs::write(base.with_extension("aep"), &output.bytes).expect("fresh AEP artifact");
        std::fs::write(base.with_extension("expected.json"), expected_json)
            .expect("manual oracle artifact");
    }
    let project = read_project(&output.bytes).expect("supplementary own-reader native structure");
    let ItemKind::Composition(comp) = &project.item(1).expect("root composition").kind else {
        panic!("{name}: missing native composition");
    };
    assert_eq!((comp.width, comp.height), (320, 180));
    near(comp.duration_secs, 2.0, "composition duration");
    near(comp.frame_rate, 24.0, "composition source FPS");
    assert_eq!(comp.pixel_aspect, (1, 1));
    let oracle_layers = expected["layers"]
        .as_array()
        .expect("ordered native layers");
    assert_eq!(
        comp.layers.len(),
        oracle_layers.len(),
        "{name}: {:?}",
        output.diagnostics
    );
    for (layer, oracle) in comp.layers.iter().zip(oracle_layers) {
        let layer_name = oracle["name"].as_str().expect("layer name");
        assert_eq!(layer.name.as_ref(), layer_name, "native stacking order");
        match oracle["kind"].as_str().expect("native kind") {
            "solid" => {
                let source_id = layer.record.source_id();
                assert_ne!(source_id, 0, "{layer_name}: not flattened or source-less");
                let source = project
                    .item(source_id)
                    .expect("fresh source item")
                    .solid
                    .as_ref()
                    .expect("Solid source")
                    .as_ref()
                    .expect("valid Solid source");
                if let Some(size) = oracle["sourceSize"].as_array() {
                    assert_eq!(
                        (source.width as u64, source.height as u64),
                        (size[0].as_u64().unwrap(), size[1].as_u64().unwrap())
                    );
                }
                if let Some(color) = oracle["sourceColor"].as_array() {
                    values(&source.color.map(f64::from), color, layer_name);
                }
            }
            "shape" => {
                assert_eq!(layer.record.layer_type(), 4, "{layer_name}: native Shape");
                assert_eq!(
                    layer.record.source_id(),
                    0,
                    "{layer_name}: no flattened media"
                );
            }
            other => panic!("{layer_name}: undeclared native kind {other}"),
        }
        if let Some(start) = oracle["startTime"].as_f64() {
            near(
                layer.record.start_time().expect("native start"),
                start,
                "start time",
            );
        }
        if let Some(end) = oracle["outPoint"].as_f64() {
            near(
                layer.record.out_point().expect("native out point"),
                end,
                "out point",
            );
        }
        if let Some(properties) = oracle["properties"].as_array() {
            for property in properties {
                let property_name = property["name"].as_str().expect("property match name");
                let label = format!("{layer_name}/{property_name}");
                let actual = numeric(&layer.content, property_name)
                    .unwrap_or_else(|| panic!("{label}: editable native numeric property missing"));
                if let Some(value) = property["value"].as_array() {
                    assert!(!actual.animated, "{label}: unexpected keys");
                    values(&actual.values, value, &label);
                }
                if let Some(keys) = property["keys"].as_array() {
                    assert!(actual.animated, "{label}: native animation missing");
                    assert_eq!(actual.keyframes.len(), keys.len(), "{label}: key count");
                    for (index, (key, oracle_key)) in actual.keyframes.iter().zip(keys).enumerate()
                    {
                        let fields = oracle_key.as_array().expect("[seconds, components...]");
                        near(
                            key.time_secs,
                            fields[0].as_f64().unwrap(),
                            &format!("{label} key {index} time"),
                        );
                        values(&key.values, &fields[1..], &format!("{label} key {index}"));
                    }
                    if let Some(interpolation) = property["lastInInterpolation"].as_u64() {
                        assert_eq!(
                            actual.keyframes.last().unwrap().in_interpolation as u64,
                            interpolation,
                            "{label}: native key easing"
                        );
                    }
                }
            }
        }
        if oracle["path"].is_object() {
            let imported = to_structural_fx_document(&project, Some(1))
                .expect("supplementary editable path readback");
            let shape = imported
                .document
                .composition()
                .layers()
                .iter()
                .find_map(|item| match item.data() {
                    fx_schema::LayerData::Shape(shape) if shape.name == layer_name => Some(shape),
                    _ => None,
                })
                .expect("independent editable Path layer");
            assert!(
                shape.shape.path.commands.iter().any(|command| matches!(
                    command,
                    fx_schema::layer::ShapePathCommand::CubicTo { .. }
                )),
                "{layer_name}: cubic handles must remain editable"
            );
            assert_eq!(
                shape.shape.path.commands.last(),
                Some(&fx_schema::layer::ShapePathCommand::Close),
                "{layer_name}: closed cubic contour"
            );
        }
        if let Some(op) = oracle["booleanOp"].as_str() {
            let imported = to_structural_fx_document(&project, Some(1))
                .expect("supplementary own-reader Boolean readback");
            fn find<'a>(
                layers: &'a [fx_schema::Layer],
                name: &str,
            ) -> Option<&'a fx_schema::BooleanOperationLayer> {
                layers.iter().find_map(|layer| match layer.data() {
                    fx_schema::LayerData::BooleanOperation(boolean) if boolean.name == name => {
                        Some(boolean)
                    }
                    fx_schema::LayerData::Group(group) => find(&group.layers, name),
                    _ => None,
                })
            }
            let boolean = find(imported.document.composition().layers(), layer_name)
                .unwrap_or_else(|| panic!("{layer_name}: editable Boolean missing"));
            let expected_op = match op {
                "union" => fx_schema::BooleanOp::Union,
                "subtract" => fx_schema::BooleanOp::Subtract,
                "intersect" => fx_schema::BooleanOp::Intersect,
                "exclude" => fx_schema::BooleanOp::Exclude,
                _ => panic!("unknown manual Boolean operation"),
            };
            assert_eq!(boolean.op, expected_op, "{layer_name}: operation");
            let kinds = oracle["operandKinds"]
                .as_array()
                .expect("ordered operand oracle");
            assert_eq!(
                boolean.layers.len(),
                kinds.len(),
                "{layer_name}: operand count"
            );
            for (operand, kind) in boolean.layers.iter().zip(kinds) {
                assert!(
                    match kind.as_str().expect("operand kind") {
                        "shape" => matches!(operand.data(), fx_schema::LayerData::Shape(_)),
                        "boolean" =>
                            matches!(operand.data(), fx_schema::LayerData::BooleanOperation(_)),
                        _ => false,
                    },
                    "{layer_name}: wrong operand order/kind"
                );
            }
        }
    }
    if let Some(omitted) = expected["omitted"].as_array() {
        for omission in omitted {
            let id = fx_schema::LayerId::new(omission["id"].as_u64().unwrap());
            let reason = omission["message"].as_str().expect("diagnostic fragment");
            assert!(
                output
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.layer_id == Some(id)
                        && diagnostic.message.contains(reason)),
                "{name}: omission {id:?} must retain a contextual diagnostic: {:?}",
                output.diagnostics
            );
            assert!(
                !comp
                    .layers
                    .iter()
                    .any(|layer| layer.name.as_ref() == omission["name"].as_str().unwrap()),
                "{name}: rejected leaf must not appear as native render content"
            );
        }
    }
}

macro_rules! panel_case {
    ($test:ident, $name:literal, $input:expr, $expected:expr) => {
        #[test]
        #[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
        fn $test() {
            crate::adobe_test_support::export_case($name, || {
                check_case($name, $input, $expected);
            });
        }
    };
}
panel_case!(
    core_solid,
    "core-solid",
    include_str!("../../../tests/fixtures/core_native_panel/core-solid.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-solid.expected.json")
);
panel_case!(
    core_rect_paint,
    "core-rect-paint",
    include_str!("../../../tests/fixtures/core_native_panel/core-rect-paint.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-rect-paint.expected.json")
);
panel_case!(
    core_path_ellipse,
    "core-path-ellipse",
    include_str!("../../../tests/fixtures/core_native_panel/core-path-ellipse.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-path-ellipse.expected.json")
);
panel_case!(
    core_star_polygon,
    "core-star-polygon",
    include_str!("../../../tests/fixtures/core_native_panel/core-star-polygon.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-star-polygon.expected.json")
);
panel_case!(
    core_static_transform,
    "core-static-transform",
    include_str!("../../../tests/fixtures/core_native_panel/core-static-transform.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-static-transform.expected.json")
);
panel_case!(
    core_transform_keys,
    "core-transform-keys",
    include_str!("../../../tests/fixtures/core_native_panel/core-transform-keys.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-transform-keys.expected.json")
);
panel_case!(
    core_rect_keys,
    "core-rect-keys",
    include_str!("../../../tests/fixtures/core_native_panel/core-rect-keys.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-rect-keys.expected.json")
);
panel_case!(
    core_parametric_keys,
    "core-parametric-keys",
    include_str!("../../../tests/fixtures/core_native_panel/core-parametric-keys.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-parametric-keys.expected.json")
);
panel_case!(
    core_boolean,
    "core-boolean",
    include_str!("../../../tests/fixtures/core_native_panel/core-boolean.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-boolean.expected.json")
);
panel_case!(
    core_boolean_nested_keys,
    "core-boolean-nested-keys",
    include_str!("../../../tests/fixtures/core_native_panel/core-boolean-nested-keys.fx.json"),
    include_str!(
        "../../../tests/fixtures/core_native_panel/core-boolean-nested-keys.expected.json"
    )
);
panel_case!(
    core_mixed_order,
    "core-mixed-order",
    include_str!("../../../tests/fixtures/core_native_panel/core-mixed-order.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-mixed-order.expected.json")
);
panel_case!(
    core_group_ranges,
    "core-group-ranges",
    include_str!("../../../tests/fixtures/core_native_panel/core-group-ranges.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-group-ranges.expected.json")
);
panel_case!(
    core_limits,
    "core-limits",
    include_str!("../../../tests/fixtures/core_native_panel/core-limits.fx.json"),
    include_str!("../../../tests/fixtures/core_native_panel/core-limits.expected.json")
);
