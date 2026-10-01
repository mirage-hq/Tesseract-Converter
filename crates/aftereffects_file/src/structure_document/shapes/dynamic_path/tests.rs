use super::*;
use crate::structure::{ItemKind, read_project};
use fx_schema::LayerId;
use fx_schema::{Layer as FxLayer, LayerData, PropType};
use sha2::{Digest, Sha256};

mod proof;
mod regression;
use proof::assert_feature_vertices;

#[test]
fn recognizes_only_complete_two_and_four_point_profiles() {
    let two = r#"
        var start = thisComp.layer(effect("Start")("Layer"));
        var end = thisComp.layer(effect("End")("Layer"));
        var a = fromCompToSurface(start.toComp(start.anchorPoint));
        var b = fromCompToSurface(end.toComp(end.anchorPoint));
        createPath(points = [a, b], inTangents = [], outTangents = [], isClosed = false);
    "#;
    // This superficially similar expression requests zero tangents, unlike
    // the stock profile. It must not silently inherit the stored handles.
    assert_eq!(parse_expression(two), None);

    let four = r#"
        var a = thisComp.layer(effect("A")("Layer"));
        var b = thisComp.layer(effect("B")("Layer"));
        var c = thisComp.layer(effect("C")("Layer"));
        var d = thisComp.layer(effect("D")("Layer"));
        createPath([
            fromCompToSurface(a.toComp(a.anchorPoint)),
            fromCompToSurface(b.toComp(b.anchorPoint)),
            fromCompToSurface(c.toComp(c.anchorPoint)),
            fromCompToSurface(d.toComp(d.anchorPoint))
        ], [], [], true);
    "#;
    assert_eq!(parse_expression(four), None);

    let stock = r#"
        var nullLayerNames = ["A: Path 1 [1.1.0]", "A: Path 1 [1.1.1]"];
        var origPath = thisProperty;
        var origPoints = origPath.points();
        var origInTang = origPath.inTangents();
        var origOutTang = origPath.outTangents();
        var getNullLayers = [];
        var getNullLayersIn = [];
        var getNullLayersOut = [];
        for (var i = 0; i < nullLayerNames.length; i++) {
            try { getNullLayers.push(effect(nullLayerNames[i])("ADBE Layer Control-0001")); }
            catch (err) { getNullLayers.push(null); }
        }
        for (var i = 0; i < getNullLayers.length; i++) {
            if (getNullLayers[i] != null && getNullLayers[i].index != thisLayer.index) {
                origPoints[i] = fromCompToSurface(getNullLayers[i].toComp(getNullLayers[i].anchorPoint));
            }
        }
        createPath(origPoints, origInTang, origOutTang, origPath.isClosed());
    "#;
    assert_eq!(
        parse_expression(stock),
        Some(DynamicExpression::CreatePath {
            controls: vec!["A: Path 1 [1.1.0]".into(), "A: Path 1 [1.1.1]".into()],
        })
    );

    let stock_four = stock.replace(
        "\"A: Path 1 [1.1.1]\"",
        "\"A: Path 1 [1.1.1]\", \"A: Path 1 [1.1.2]\", \"A: Path 1 [1.1.3]\"",
    );
    assert!(
        matches!(parse_expression(&stock_four), Some(DynamicExpression::CreatePath { controls, .. }) if controls.len() == 4)
    );
    let without_unused_arrays = stock
        .replace("var getNullLayersIn = [];", "")
        .replace("var getNullLayersOut = [];", "");
    assert_eq!(
        parse_expression(&without_unused_arrays),
        parse_expression(stock)
    );
    for unsupported in [
        stock.replace(
            "fromCompToSurface(getNullLayers[i].toComp(getNullLayers[i].anchorPoint))",
            "getNullLayers[i].toComp(fromCompToSurface(getNullLayers[i].anchorPoint))",
        ),
        format!("{stock}execute();"),
        two.replace("createPath", "unknown"),
        two.replace("start.anchorPoint", "start.position"),
        two.replace("[a, b]", "[a + [1, 0], b]"),
        two.replace("outTangents = []", "outTangents = [[1, 0], [0, 1]]"),
        two.replace("isClosed = false", "isClosed = controller.enabled"),
    ] {
        assert_eq!(parse_expression(&unsupported), None, "{unsupported}");
    }
}

#[test]
fn recognizes_exact_direct_sibling_path_copy() {
    assert_eq!(
        parse_expression(
            r#"thisComp.layer("Folding Path").content("Shape 1").content("Path 1").path;"#
        ),
        Some(DynamicExpression::CopySibling {
            layer: "Folding Path".into(),
            contents: vec!["Shape 1".into(), "Path 1".into()],
        })
    );
    assert_eq!(
        parse_expression(
            r#"comp("SH01").layer("Circle_connector").content("Group 1").content("Path 1").path"#
        ),
        Some(DynamicExpression::CopyComposition {
            composition: "SH01".into(),
            layer: "Circle_connector".into(),
            contents: vec!["Group 1".into(), "Path 1".into()],
        })
    );
    assert_eq!(
        parse_expression(
            r#"thisComp.layer("Folding Path").content("Shape 1").transform.position;"#
        ),
        None
    );
    assert_eq!(
        parse_expression(r#"thisComp.layer("Folding Path").content("A").path + value;"#),
        None
    );
}

#[test]
fn recognized_path_expression_is_not_limited_by_source_byte_count() {
    let expression = format!(
        "{}{}",
        " ".repeat(16 * 1024 + 1),
        r#"thisComp.layer("Folding Path").content("Shape 1").content("Path 1").path;"#
    );
    assert!(matches!(
        parse_expression(&expression),
        Some(DynamicExpression::CopySibling { .. })
    ));
}

#[test]
fn affine_surface_round_trip_preserves_points() {
    let local = Affine::layer([12.0, -4.0], [210.0, 95.0], [1.25, 0.75], 37.0);
    let parent = Affine::layer([0.0, 0.0], [-30.0, 18.0], [0.8, 1.4], -12.0);
    let matrix = local.then(parent);
    let point = [48.0, -19.0];
    let restored = matrix.inverse().unwrap().apply(matrix.apply(point));
    assert!((restored[0] - point[0]).abs() < 1e-9);
    assert!((restored[1] - point[1]).abs() < 1e-9);
}

#[test]
fn generated_paths_keep_order_closedness_and_relative_tangents() {
    let initial = ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: 1.0,
                c1y: 0.0,
                c2x: 9.0,
                c2y: 0.0,
                x: 10.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 10.0,
                y: 10.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::Close,
        ],
    };
    let geometry = PathGeometry::new(initial).unwrap();
    let path = geometry
        .deform(&[[5.0, 6.0], [20.0, 8.0], [17.0, 19.0]])
        .unwrap();
    let [
        ShapePathCommand::MoveTo { x: x0, y: y0, .. },
        ShapePathCommand::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x: x1,
            y: y1,
            ..
        },
        ShapePathCommand::LineTo { x: x2, y: y2, .. },
        ShapePathCommand::LineTo { x: x3, y: y3, .. },
        ShapePathCommand::Close,
    ] = path.commands.as_slice()
    else {
        panic!("deformed contour topology changed: {:?}", path.commands)
    };
    assert_eq!([*x0, *y0], [5.0, 6.0]);
    assert_eq!(
        [*c1x, *c1y, *c2x, *c2y, *x1, *y1],
        [6.0, 6.0, 19.0, 8.0, 20.0, 8.0]
    );
    assert_eq!([*x2, *y2], [17.0, 19.0]);
    assert_eq!([*x3, *y3], [5.0, 6.0]);

    let points = [[5.0, 6.0], [20.0, 8.0], [17.0, 19.0]];
    let clock = NumericAnimationClock::source_local_rebased(0.0);
    let key = path_keyframe(LayerId::new(1), 0, &points, clock, &geometry).unwrap();
    let PropertyValue::Path(compact) = key.value() else {
        panic!("generated key remains an editable Path")
    };
    assert_eq!(compact.commands.len(), path.commands.len() - 1);
    assert!(matches!(
        compact.commands.last(),
        Some(ShapePathCommand::Close)
    ));

    let mut curved = geometry.initial.clone();
    let closing_index = curved.commands.len() - 2;
    curved.commands[closing_index] = ShapePathCommand::CubicTo {
        c1x: 9.0,
        c1y: 7.0,
        c2x: 2.0,
        c2y: 3.0,
        x: 0.0,
        y: 0.0,
        mirror: None,
        corner_radius: None,
    };
    let curved = PathGeometry::new(curved).unwrap();
    let key = path_keyframe(LayerId::new(1), 0, &points, clock, &curved).unwrap();
    let PropertyValue::Path(compact) = key.value() else {
        panic!("generated key remains an editable Path")
    };
    assert_eq!(compact.commands.len(), curved.initial.commands.len());
    assert!(matches!(
        compact.commands[closing_index],
        ShapePathCommand::CubicTo { .. }
    ));
}

#[test]
#[ignore = "requires licensed local Intro source at the pinned SHA-256"]
fn local_external_source_restores_bounded_dynamic_path_families() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tmp/ordinary-intro/native-input/source.aep");
    let bytes = std::fs::read(&source).expect("set up the licensed Intro source locally");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = read_project(&bytes).expect("pinned Intro source parses");
    struct Case<'a> {
        composition_id: u32,
        layers: &'a [(u32, &'a [i64])],
    }
    let cases = [
        Case {
            composition_id: 3,
            layers: &[
                (2210, &[2_000]),
                (2137, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2146, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2147, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2148, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2149, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2150, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2155, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2156, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2157, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2158, &[42, 83, 125, 2_333, 2_375, 2_417]),
                (2199, &[750, 792, 833]),
                (2200, &[750, 792, 833]),
            ],
        },
        Case {
            composition_id: 2420,
            layers: &[
                (2459, &[417]),
                (2466, &[417]),
                (2468, &[417]),
                (2627, &[83]),
                (2636, &[83]),
            ],
        },
        Case {
            composition_id: 724,
            layers: &[
                (741, &[1_083, 1_125, 1_167, 1_833]),
                (747, &[1_083, 1_125, 1_167, 1_833]),
                (750, &[1_083, 1_125, 1_167, 1_833]),
                (742, &[1_083, 1_125, 1_167, 1_833]),
                (743, &[1_083, 1_125, 1_167, 1_833]),
                (746, &[1_083, 1_125, 1_167, 1_833]),
            ],
        },
    ];
    let mut failures = Vec::new();
    for case in cases {
        let composition_id = case.composition_id;
        let item = project
            .item(composition_id)
            .expect("requested composition exists");
        let ItemKind::Composition(native) = &item.kind else {
            panic!("item {composition_id} is not a composition")
        };
        let converted =
            crate::structure_document::to_structural_fx_document(&project, Some(composition_id))
                .expect("fresh bounded conversion succeeds");
        for diagnostic in &converted.diagnostics {
            let text = format!("{diagnostic:?}");
            if text.contains("Path expression not lowered") {
                eprintln!("{text}");
            }
        }
        assert!(
            converted
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .all(|entry| !entry.animator.is_js_script())
        );
        for (layer_id, critical_times_ms) in case.layers {
            let native_layer = native
                .layers
                .iter()
                .find(|layer| layer.record.id() == *layer_id)
                .unwrap_or_else(|| {
                    panic!("native layer {layer_id} exists in comp {composition_id}")
                });
            assert_native_dependency(native_layer, composition_id, *layer_id);
            let native_path = path::decode_first(path_runs(native_layer).unwrap()[0], [1.0, 1.0])
                .unwrap()
                .0;
            assert_eq!(
                native
                    .layers
                    .iter()
                    .filter(|candidate| candidate.name == native_layer.name)
                    .count(),
                1,
                "native ID must bind unambiguously to the imported group"
            );
            let groups = groups_named(
                converted.document.composition().layers(),
                native_layer.name.as_ref(),
            );
            let matched = groups.iter().any(|group| {
                    let mut shape_ids = Vec::new();
                    collect_shape_ids(&group.layers, &mut shape_ids);
                    converted
                        .document
                        .composition()
                        .dynamics()
                        .entries()
                        .iter()
                        .any(|entry| {
                            if !shape_ids.iter().any(|id| entry.target == PropertyTarget::layer(*id, PropType::ShapePath)) {
                                return false;
                            }
                            let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data() else { return false; };
                            assert!(track.keyframes().iter().all(|key| matches!(key.value(), PropertyValue::Path(path) if path.is_finite())));
                            for critical_time_ms in *critical_times_ms {
                                assert_feature_vertices(
                                    *layer_id,
                                    track,
                                    *critical_time_ms,
                                    &native_path,
                                );
                            }
                            true
                        })
                });
            if !matched {
                failures.push(format!(
                    "comp {composition_id} native layer {layer_id} ({}) lacks an editable ShapePath track",
                    native_layer.name
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "requires licensed local Intro source at the pinned SHA-256"]
fn local_external_source_restores_frame_108_cross_composition_connector() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tmp/ordinary-intro/native-input/source.aep");
    let bytes = std::fs::read(&source).expect("set up the licensed Intro source locally");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = read_project(&bytes).expect("pinned Intro source parses");
    let ItemKind::Composition(source_composition) = &project.item(3).unwrap().kind else {
        panic!("SH01 is a composition")
    };
    assert_eq!(
        generated_path_controls(
            source_composition,
            "Circle_connector",
            &["Group 1".into(), "Path 1".into()]
        ),
        Some(vec![
            "Circle_connector: Path 1 [1.1.0]".into(),
            "Circle_connector: Path 1 [1.1.1]".into(),
            "Circle_connector: Path 1 [1.1.2]".into(),
            "Circle_connector: Path 1 [1.1.3]".into(),
        ])
    );
    let converted = crate::structure_document::to_structural_fx_document(&project, Some(596))
        .expect("fresh frame-108 source composition conversion succeeds");
    assert!(converted.diagnostics.iter().all(|diagnostic| {
        let text = format!("{diagnostic:?}");
        !(text.contains("composition 596 layer 639")
            && text.contains("Path expression not lowered"))
    }));
    let groups = groups_named(
        converted.document.composition().layers(),
        "Circle_connector 2",
    );
    assert_eq!(groups.len(), 1);
    let mut shape_ids = Vec::new();
    collect_shape_ids(&groups[0].layers, &mut shape_ids);
    let track = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find_map(|entry| {
            if !shape_ids
                .iter()
                .any(|id| entry.target == PropertyTarget::layer(*id, PropType::ShapePath))
            {
                return None;
            }
            let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
            else {
                return None;
            };
            Some(track)
        })
        .unwrap_or_else(|| {
            let path_targets = converted
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .filter_map(|entry| {
                    let text = format!("{:?}", entry.target);
                    text.contains("ShapePath").then_some((text, entry.animator.data()))
                })
                .collect::<Vec<_>>();
            panic!(
                "cross-composition connector has editable ShapePath keys; shape IDs={shape_ids:?}; path targets={path_targets:#?}; diagnostics={:#?}",
                converted.diagnostics
            )
        });
    let points = proof::sample_vertices(track, 958);
    let (min_x, max_x) = points
        .iter()
        .map(|point| point[0])
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), x| {
            (min.min(x), max.max(x))
        });
    assert!(
        max_x - min_x > 500.0,
        "frame-108 connector must not retain the stale 500px square: {points:?}"
    );
}

fn assert_native_dependency(layer: &Layer, composition_id: u32, layer_id: u32) {
    let parsed: Vec<_> = path_runs(layer)
        .expect("native Path runs decode")
        .into_iter()
        .filter_map(|run| path_expression(run).expect("native Path metadata decodes"))
        .filter_map(parse_expression)
        .collect();
    let [expression] = parsed.as_slice() else {
        panic!("comp {composition_id} layer {layer_id} must have one authorized Path expression")
    };
    match (layer_id, expression) {
        (2210, DynamicExpression::CreatePath { controls }) => {
            assert_eq!(controls.len(), 4);
            let ids: Vec<_> = controls
                .iter()
                .map(|control| layer_control_id(layer, control).unwrap())
                .collect();
            assert_eq!(ids, [2131, 2130, 2126, 2127]);
        }
        (
            2137 | 2146 | 2147 | 2148 | 2149 | 2150 | 2155 | 2156 | 2157 | 2158 | 2199 | 2200,
            DynamicExpression::CreatePath { controls },
        ) => {
            assert_eq!(controls.len(), 4);
            let ids: Vec<_> = controls
                .iter()
                .map(|control| layer_control_id(layer, control).unwrap())
                .collect();
            let expected: &[u32] = match layer_id {
                2137 => &[2136, 2135, 2131, 2132],
                2146 => &[2134, 2135, 2131, 2130],
                2147 => &[2134, 2133, 2129, 2130],
                2148 => &[2128, 2127, 2117, 2118],
                2149 => &[2127, 2126, 2122, 2117],
                2150 => &[2126, 2125, 2121, 2122],
                2155 => &[2134, 2133, 2129, 2129],
                2156 => &[2128, 2127, 2117, 2128],
                2157 => &[2128, 2154, 2127, 2128],
                2158 => &[2130, 2129, 2153, 2153],
                2199 => &[2132, 2131, 2127, 2128],
                2200 => &[2130, 2129, 2125, 2126],
                _ => unreachable!(),
            };
            assert_eq!(ids, expected);
        }
        (2459 | 2466 | 2468, DynamicExpression::CreatePath { controls }) => {
            assert_eq!(controls.len(), 2);
            let ids: Vec<_> = controls
                .iter()
                .map(|control| layer_control_id(layer, control).unwrap())
                .collect();
            assert_eq!(ids, [2455, 2456]);
        }
        (2627 | 2636, DynamicExpression::CreatePath { controls }) => {
            let ids: Vec<_> = controls
                .iter()
                .map(|control| layer_control_id(layer, control).unwrap())
                .collect();
            assert_eq!(
                ids,
                if layer_id == 2627 {
                    vec![2622, 2619, 2607, 2610]
                } else {
                    vec![2624, 2600]
                }
            );
        }
        (741 | 747 | 750, DynamicExpression::CreatePath { controls }) => {
            assert_eq!(controls.len(), 4);
            let ids: Vec<_> = controls
                .iter()
                .map(|control| layer_control_id(layer, control).unwrap())
                .collect();
            let expected = match layer_id {
                741 => [733, 737, 737, 732],
                747 => [732, 732, 733, 731],
                750 => [731, 731, 750, 737],
                _ => unreachable!(),
            };
            assert_eq!(ids, expected);
        }
        (742 | 743 | 746, DynamicExpression::CopySibling { layer, .. }) => {
            assert_eq!(layer, "Dark-Top");
        }
        _ => panic!("unexpected comp {composition_id} layer {layer_id} expression: {expression:?}"),
    }
}

fn groups_named<'a>(layers: &'a [FxLayer], name: &str) -> Vec<&'a fx_schema::GroupLayer> {
    let mut result = Vec::new();
    for layer in layers {
        if let LayerData::Group(group) = layer.data() {
            if group.name == name {
                result.push(group);
            }
            result.extend(groups_named(&group.layers, name));
        }
    }
    result
}

fn collect_shape_ids(layers: &[FxLayer], output: &mut Vec<LayerId>) {
    for layer in layers {
        match layer.data() {
            LayerData::Shape(shape) => output.push(shape.id),
            LayerData::Group(group) => collect_shape_ids(&group.layers, output),
            _ => (),
        }
    }
}
