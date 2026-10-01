// Fresh imports of immutable Adobe-authored shape sources. The expected values
// come from independently authored JSX whose historical identities are pinned in
// tests/fixtures/aep_authoring_provenance.json; neither this importer nor its writer
// produced those expectations.
use super::*;

macro_rules! source {
    ($file:literal, $data:expr, $sha:literal, $bytes:literal) => {
        NativeSource {
            path: concat!("crates/aftereffects_file/tests/fixtures/shapes/", $file),
            bytes: $data,
            sha256: $sha,
            byte_count: $bytes,
        }
    };
}

struct NativeSource {
    path: &'static str,
    bytes: &'static [u8],
    sha256: &'static str,
    byte_count: usize,
}

const SOURCES: &[NativeSource] = &[
    source!(
        "import_boolean_paint_blend.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_boolean_paint_blend.aep"),
        "cd215299543670e4a6b1e81c7453ee18b4af8beff3bd59a056aadf3e19663e94",
        165_903
    ),
    source!(
        "import_gradient_controls.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_gradient_controls.aep"),
        "5406fe7f92d697e0882d11a8804c436785b51f533d813419364826b5f35ffcce",
        625_839
    ),
    source!(
        "import_gradient_stroke_details.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_gradient_stroke_details.aep"),
        "17fc50911102dec5cde381ab8ef234c62d315e5b9d3d128dd6d5bc3fe43fe919",
        270_693
    ),
    source!(
        "import_group_transform_controls.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_group_transform_controls.aep"),
        "ea4ba57dc9d672d4e321208298541018464b88a2fce11c25a4ff9ff35d371ed5",
        1_083_591
    ),
    source!(
        "import_isolated_native_gradients.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_isolated_native_gradients.aep"),
        "1cb781c461114a4044e635c24cae6b0802c386ed13277eae8cfdec8d2d1c4d63",
        142_405
    ),
    source!(
        "import_modifier_controls.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_modifier_controls.aep"),
        "5bc36fb8210de9bde79d2313b07c4c677e1c040e1d7764ca3a49ba9ed9fcb7e2",
        866_707
    ),
    source!(
        "import_modifier_order_cases.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_modifier_order_cases.aep"),
        "c52c7ff4a2635e2011ba53fc418dcc1363bc32dd20b99138ae928ce1a61dc303",
        635_723
    ),
    source!(
        "import_parametric_shape_controls.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_parametric_shape_controls.aep"),
        "828179aee6c82b68c5a866c7dff28aca7a75ab17083a753242396192c243fd6d",
        1_086_989
    ),
    source!(
        "import_path_direction_cases.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_path_direction_cases.aep"),
        "59946f45a301fd5530522f46e109e38827d69484efdc57f8107a8fda92edf3c6",
        548_289
    ),
    source!(
        "import_polygon_animation.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_polygon_animation.aep"),
        "f78b9bc9e43cd2402da9e6c0c2710b4c7e2786a60c05d6268ac563daa0fd330f",
        330_913
    ),
    source!(
        "import_rectangle_controls.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_rectangle_controls.aep"),
        "694bb9df366bc23a9a72aaf078497058a65b197eaa19c558cfe69f410b6fd2f6",
        468_843
    ),
    source!(
        "import_remaining_mapped_controls.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_remaining_mapped_controls.aep"),
        "aa080cbb30a474f579c7acfa7ba30bb173afef5b6eb73b7e513c5cdf17bd1c29",
        319_471
    ),
    source!(
        "import_shape_blend_ownership.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_shape_blend_ownership.aep"),
        "938cc1f441213b28af9ce1d703fc6787536b55692e2bcee377ffe4d86deed8b1",
        238_521
    ),
    source!(
        "import_shape_control_animation.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_shape_control_animation.aep"),
        "6031e00e3d65852c1a049635da302b5f9ac3cfc3a2fcd5952fbc6eba1f6c9591",
        1_539_855
    ),
    source!(
        "import_shape_flags_order.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_shape_flags_order.aep"),
        "8946ba3422a3fb93a43583e4aa379dd8b874c6e5885450b234ebcf8f15726727",
        788_663
    ),
    source!(
        "import_solid_paint_controls.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_solid_paint_controls.aep"),
        "4c7115020ff2a81da03f205a025688dc1dddf03f963c8fdbaa2dece6cd2650bf",
        935_211
    ),
    source!(
        "import_stroke_dash_caps.aep",
        include_bytes!("../../../tests/fixtures/shapes/import_stroke_dash_caps.aep"),
        "293603b08abae9bf0eb553e9a3849dd0f11040345cc3ecf791f09b3453388ec2",
        863_673
    ),
];

fn pinned_source(file: &str) -> &'static NativeSource {
    SOURCES
        .iter()
        .find(|source| source.path.ends_with(file))
        .unwrap_or_else(|| panic!("missing pinned native source {file}"))
}

fn source_bytes(file: &str) -> &'static [u8] {
    pinned_source(file).bytes
}

fn fresh_import(file: &str, comp_id: u32, comp_name: &str) -> StructuralConversion {
    let project = read_project(source_bytes(file)).unwrap();
    let source = composition(&project, comp_id);
    assert_eq!(source.frame_rate, 24.0);
    let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
    assert_imported_canvas_matches_source(source, &converted, &format!("{file}:{comp_id}"));
    assert_eq!(root(&converted).name, comp_name);
    assert_eq!(root(&converted).layers.len(), 2, "{file}:{comp_id}");
    converted
}

fn foreground(converted: &StructuralConversion) -> &GroupLayer {
    as_group(&root(converted).layers[0])
}

fn collect_geometry<'a>(layers: &'a [fx_schema::Layer], output: &mut Vec<&'a FxLayer>) {
    for layer in layers {
        match layer.data() {
            FxLayer::Rect(_) | FxLayer::Shape(_) | FxLayer::BooleanOperation(_) => {
                output.push(layer.data())
            }
            FxLayer::Group(group) => collect_geometry(&group.layers, output),
            _ => {}
        }
    }
}

fn only_geometry(converted: &StructuralConversion) -> &FxLayer {
    let mut layers = Vec::new();
    collect_geometry(&foreground(converted).layers, &mut layers);
    assert_eq!(layers.len(), 1);
    layers[0]
}

fn assert_track(converted: &StructuralConversion, property: &str, first: Value, last: Value) {
    let json: Value = serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap();
    let entries = json["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let matching: Vec<_> = entries
        .iter()
        .filter(|entry| entry["target"]["propertyType"] == property)
        .collect();
    assert_eq!(matching.len(), 1, "expected one editable {property} track");
    let keys = matching[0]["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["value"], first);
    assert_eq!(keys[1]["value"], last);
    assert_eq!(keys[0]["layerTime"], 0);
    assert!(keys[1]["layerTime"].as_u64().unwrap() > 0);
    assert!(keys.iter().all(|key| key["easing"]["type"] == "linear"));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_manifest_targets_are_sha_pinned_and_enumerated() {
    for source in SOURCES {
        assert_eq!(source.bytes.len(), source.byte_count, "{}", source.path);
        assert_eq!(format!("{:x}", Sha256::digest(source.bytes)), source.sha256);
    }
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/aep_video_references.json"
    ))
    .unwrap();
    let shape_sources: Vec<_> = manifest["sources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|source| {
            source["source_path"]
                .as_str()
                .unwrap()
                .starts_with("crates/aftereffects_file/tests/fixtures/shapes/import")
        })
        .collect();
    assert_eq!(shape_sources.len(), 17);
    assert_eq!(
        shape_sources
            .iter()
            .map(|source| source["compositions"].as_array().unwrap().len())
            .sum::<usize>(),
        141
    );
    for manifest_source in shape_sources {
        let path = manifest_source["source_path"].as_str().unwrap();
        let pinned = SOURCES.iter().find(|source| source.path == path).unwrap();
        assert_eq!(manifest_source["source_sha256"], pinned.sha256);
        assert_eq!(manifest_source["source_bytes"], pinned.byte_count);
        for comp in manifest_source["compositions"].as_array().unwrap() {
            assert!(comp["composition_id"].as_u64().unwrap() > 0);
            assert!(!comp["composition_name"].as_str().unwrap().is_empty());
        }
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pinned_square_canvas_survives_import_with_editable_geometry_unchanged() {
    let file = "import_isolated_native_gradients.aep";
    let pinned = pinned_source(file);
    assert_eq!(pinned.bytes.len(), pinned.byte_count);
    assert_eq!(format!("{:x}", Sha256::digest(pinned.bytes)), pinned.sha256);
    let project = read_project(pinned.bytes).unwrap();

    for (composition_id, composition_name) in [
        (14, "NATIVE_STOPS_FILL_ONLY"),
        (27, "NATIVE_STOPS_STROKE_ONLY"),
    ] {
        let source = composition(&project, composition_id);
        assert_eq!(
            (source.width, source.height, source.frame_rate),
            (1000, 1000, 24.0),
            "{composition_name} source canvas"
        );
        let converted = to_structural_fx_document(&project, Some(composition_id)).unwrap();
        assert_imported_canvas_matches_source(source, &converted, composition_name);
        assert_eq!(root(&converted).name, composition_name);

        let mut imported_geometry = Vec::new();
        collect_geometry(&root(&converted).layers, &mut imported_geometry);
        assert!(!imported_geometry.is_empty(), "{composition_name}");
        let imported_geometry = serde_json::to_value(imported_geometry).unwrap();

        let archived = converted.document.to_json_vec().unwrap();
        let reopened = EditableFxCompositionDocument::from_json_slice(&archived).unwrap();
        assert_eq!(reopened.dimensions(), converted.document.dimensions());
        let reopened_root = as_group(&reopened.composition().layers()[0]);
        let mut reopened_geometry = Vec::new();
        collect_geometry(&reopened_root.layers, &mut reopened_geometry);
        assert_eq!(
            serde_json::to_value(reopened_geometry).unwrap(),
            imported_geometry,
            "{composition_name} editable geometry"
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_parametric_controls_import_as_editable_geometry() {
    struct Case {
        id: u32,
        name: &'static str,
        expected: Expected,
    }
    enum Expected {
        Ellipse {
            size: [f64; 2],
            position: [f64; 2],
        },
        Star {
            polygon: bool,
            points: f64,
            position: [f64; 2],
            rotation: f64,
            inner_radius: f64,
            outer_radius: f64,
            inner_roundness: f64,
            outer_roundness: f64,
        },
    }
    let cases = [
        Case {
            id: 1,
            name: "IMPORT_ELLIPSE_SIZE",
            expected: Expected::Ellipse {
                size: [560.0, 310.0],
                position: [0.0, 0.0],
            },
        },
        Case {
            id: 17,
            name: "IMPORT_ELLIPSE_POSITION",
            expected: Expected::Ellipse {
                size: [400.0, 240.0],
                position: [70.0, -45.0],
            },
        },
        Case {
            id: 32,
            name: "IMPORT_STAR_POINTS",
            expected: Expected::Star {
                polygon: false,
                points: 7.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 47,
            name: "IMPORT_STAR_POSITION",
            expected: Expected::Star {
                polygon: false,
                points: 5.0,
                position: [90.0, -40.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 62,
            name: "IMPORT_STAR_ROTATION",
            expected: Expected::Star {
                polygon: false,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 35.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 77,
            name: "IMPORT_STAR_INNER_RADIUS",
            expected: Expected::Star {
                polygon: false,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 35.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 92,
            name: "IMPORT_STAR_OUTER_RADIUS",
            expected: Expected::Star {
                polygon: false,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 210.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 107,
            name: "IMPORT_STAR_INNER_ROUNDNESS",
            expected: Expected::Star {
                polygon: false,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 20.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 122,
            name: "IMPORT_STAR_OUTER_ROUNDNESS",
            expected: Expected::Star {
                polygon: false,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 30.0,
            },
        },
        Case {
            id: 137,
            name: "IMPORT_POLYGON_POINTS",
            expected: Expected::Star {
                polygon: true,
                points: 7.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 152,
            name: "IMPORT_POLYGON_POSITION",
            expected: Expected::Star {
                polygon: true,
                points: 5.0,
                position: [90.0, -40.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 167,
            name: "IMPORT_POLYGON_ROTATION",
            expected: Expected::Star {
                polygon: true,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 35.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 182,
            name: "IMPORT_POLYGON_OUTER_RADIUS",
            expected: Expected::Star {
                polygon: true,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 210.0,
                inner_roundness: 0.0,
                outer_roundness: 0.0,
            },
        },
        Case {
            id: 197,
            name: "IMPORT_POLYGON_OUTER_ROUNDNESS",
            expected: Expected::Star {
                polygon: true,
                points: 5.0,
                position: [0.0, 0.0],
                rotation: 0.0,
                inner_radius: 75.0,
                outer_radius: 150.0,
                inner_roundness: 0.0,
                outer_roundness: 30.0,
            },
        },
    ];
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    for case in cases {
        batch.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_parametric_shape_controls.aep",
            case.id,
            || {
                let converted =
                    fresh_import("import_parametric_shape_controls.aep", case.id, case.name);
                let FxLayer::Shape(shape) = only_geometry(&converted) else {
                    panic!("{} must remain editable Shape geometry", case.name)
                };
                assert_eq!(shape.shape.fills.len(), 1);
                match case.expected {
                    Expected::Ellipse { size, position } => {
                        let ellipse = shape.shape.ellipse.as_ref().unwrap();
                        assert_eq!((ellipse.size, ellipse.position), (size, position));
                    }
                    Expected::Star {
                        polygon,
                        points,
                        position,
                        rotation,
                        inner_radius,
                        outer_radius,
                        inner_roundness,
                        outer_roundness,
                    } => {
                        let star = shape.shape.poly_star.as_ref().unwrap();
                        assert_eq!(
                            matches!(star.star_type, fx_composition::ShapePolyStarType::Polygon),
                            polygon
                        );
                        assert_eq!(
                            (star.points, star.position, star.rotation),
                            (points, position, rotation)
                        );
                        assert_eq!(
                            (star.inner_radius, star.outer_radius),
                            (inner_radius, outer_radius)
                        );
                        assert_eq!(
                            (star.inner_roundness, star.outer_roundness),
                            (inner_roundness, outer_roundness)
                        );
                    }
                }
            },
        );
    }
    batch.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_rectangle_controls_import_static_values_and_key_tracks() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, size, position, roundness) in [
        (
            1,
            "IMPORT_RECT_SIZE_STATIC",
            [600.0, 200.0],
            [0.0, 0.0],
            0.0,
        ),
        (
            17,
            "IMPORT_RECT_POSITION_STATIC",
            [480.0, 270.0],
            [95.0, -65.0],
            0.0,
        ),
        (
            32,
            "IMPORT_RECT_ROUNDNESS_STATIC",
            [480.0, 270.0],
            [0.0, 0.0],
            64.0,
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_rectangle_controls.aep",
            id,
            || {
                let converted = fresh_import("import_rectangle_controls.aep", id, name);
                let FxLayer::Rect(rect) = only_geometry(&converted) else {
                    panic!("{name} must be an editable Rect")
                };
                assert_eq!(
                    (rect.rect.size, rect.transform.position, rect.rect.roundness),
                    (size, fx_composition::Position::TwoD(position), roundness)
                );
                assert_eq!(rect.rect.fill_color, [0.75, 0.125, 0.25, 1.0]);
            },
        );
    }
    for (id, name, property, first, last) in [
        (
            47,
            "IMPORT_RECT_SIZE_KEYED",
            "rectSize",
            serde_json::json!({"type":"vector2","value":[320.0,180.0]}),
            serde_json::json!({"type":"vector2","value":[560.0,300.0]}),
        ),
        (
            62,
            "IMPORT_RECT_POSITION_KEYED",
            "positionX",
            serde_json::json!({"type":"float","value":-120.0}),
            serde_json::json!({"type":"float","value":120.0}),
        ),
        (
            77,
            "IMPORT_RECT_ROUNDNESS_KEYED",
            "rectRoundness",
            serde_json::json!({"type":"float","value":0.0}),
            serde_json::json!({"type":"float","value":70.0}),
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_rectangle_controls.aep",
            id,
            || {
                let converted = fresh_import("import_rectangle_controls.aep", id, name);
                assert!(matches!(only_geometry(&converted), FxLayer::Rect(_)));
                assert_track(&converted, property, first, last);
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_solid_paint_controls_remain_editable() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, check) in [
        (1, "IMPORT_PAINT_FILL_COLOR", 0),
        (17, "IMPORT_PAINT_FILL_OPACITY", 1),
        (32, "IMPORT_PAINT_STROKE_COLOR", 2),
        (47, "IMPORT_PAINT_STROKE_WIDTH", 3),
        (62, "IMPORT_PAINT_STROKE_OPACITY", 4),
        (122, "IMPORT_PAINT_JOIN_MITER", 5),
        (137, "IMPORT_PAINT_JOIN_ROUND", 6),
        (152, "IMPORT_PAINT_JOIN_BEVEL", 7),
        (167, "IMPORT_PAINT_MITER_LIMIT", 8),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_solid_paint_controls.aep",
            id,
            || {
                let converted = fresh_import("import_solid_paint_controls.aep", id, name);
                let FxLayer::Rect(rect) = only_geometry(&converted) else {
                    panic!("{name} must remain an editable Rect")
                };
                match check {
                    0 => assert_eq!(rect.rect.fill_color, [0.125, 0.75, 0.25, 1.0]),
                    1 => assert_eq!(rect.transform.opacity.value(), 60.0),
                    2 => assert_eq!(rect.rect.stroke_color, Some([0.25, 0.875, 0.125, 1.0])),
                    3 => assert_eq!(rect.rect.stroke_width.value(), 22.0),
                    4 => assert_eq!(rect.transform.opacity.value(), 55.0),
                    5 => assert_eq!(rect.rect.stroke_join, fx_composition::ShapeLineJoin::Miter),
                    6 => assert_eq!(rect.rect.stroke_join, fx_composition::ShapeLineJoin::Round),
                    7 => assert_eq!(rect.rect.stroke_join, fx_composition::ShapeLineJoin::Bevel),
                    8 => assert_eq!(rect.rect.stroke_miter_limit, 8.0),
                    _ => unreachable!(),
                }
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_path_direction_and_stroke_geometry_are_editable() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, endpoints, closed) in [
        (
            1,
            "PATH_OPEN_FORWARD",
            vec![(-180.0, 120.0), (0.0, -150.0), (180.0, 120.0)],
            false,
        ),
        (
            17,
            "PATH_OPEN_REVERSED",
            vec![(180.0, 120.0), (0.0, -150.0), (-180.0, 120.0)],
            false,
        ),
        (
            32,
            "PATH_CLOSED_FORWARD",
            vec![(-180.0, 120.0), (0.0, -150.0), (180.0, 120.0)],
            true,
        ),
        (
            47,
            "PATH_CLOSED_REVERSED",
            vec![(180.0, 120.0), (0.0, -150.0), (-180.0, 120.0)],
            true,
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_path_direction_cases.aep",
            id,
            || {
                let converted = fresh_import("import_path_direction_cases.aep", id, name);
                let FxLayer::Shape(shape) = only_geometry(&converted) else {
                    panic!("{name} must remain an editable path")
                };
                let actual: Vec<_> = shape
                    .shape
                    .path
                    .commands
                    .iter()
                    .filter_map(|command| command.endpoint())
                    .collect();
                assert_eq!(actual, endpoints);
                assert_eq!(
                    shape
                        .shape
                        .path
                        .commands
                        .iter()
                        .any(|command| matches!(command, fx_composition::ShapePathCommand::Close)),
                    closed
                );
            },
        );
    }
    for (id, name, cap, join, miter) in [
        (
            1,
            "CAP_BUTT",
            fx_composition::ShapeLineCap::Butt,
            fx_composition::ShapeLineJoin::Miter,
            10.0,
        ),
        (
            17,
            "CAP_ROUND",
            fx_composition::ShapeLineCap::Round,
            fx_composition::ShapeLineJoin::Miter,
            10.0,
        ),
        (
            32,
            "CAP_SQUARE",
            fx_composition::ShapeLineCap::Square,
            fx_composition::ShapeLineJoin::Miter,
            10.0,
        ),
        (
            47,
            "JOIN_MITER",
            fx_composition::ShapeLineCap::Butt,
            fx_composition::ShapeLineJoin::Miter,
            10.0,
        ),
        (
            62,
            "JOIN_ROUND",
            fx_composition::ShapeLineCap::Butt,
            fx_composition::ShapeLineJoin::Round,
            10.0,
        ),
        (
            77,
            "JOIN_BEVEL",
            fx_composition::ShapeLineCap::Butt,
            fx_composition::ShapeLineJoin::Bevel,
            10.0,
        ),
        (
            92,
            "MITER_LOW",
            fx_composition::ShapeLineCap::Butt,
            fx_composition::ShapeLineJoin::Miter,
            2.0,
        ),
        (
            107,
            "MITER_HIGH",
            fx_composition::ShapeLineCap::Butt,
            fx_composition::ShapeLineJoin::Miter,
            10.0,
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_stroke_dash_caps.aep",
            id,
            || {
                let converted = fresh_import("import_stroke_dash_caps.aep", id, name);
                let FxLayer::Shape(shape) = only_geometry(&converted) else {
                    panic!("{name} must remain editable")
                };
                let stroke = &shape.shape.strokes[0];
                assert_eq!(
                    (
                        stroke.cap,
                        stroke.join,
                        stroke.miter_limit,
                        stroke.width.value()
                    ),
                    (cap, join, miter, 24.0)
                );
            },
        );
    }
    for (id, name, dashes, offset) in [
        (122, "DASH_GAP", vec![30.0, 18.0], 0.0),
        (137, "DASH_OFFSET", vec![30.0, 18.0], 22.0),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_stroke_dash_caps.aep",
            id,
            || {
                let converted = fresh_import("import_stroke_dash_caps.aep", id, name);
                let FxLayer::Shape(shape) = only_geometry(&converted) else {
                    panic!("{name} must remain editable")
                };
                let stroke = &shape.shape.strokes[0];
                assert_eq!(
                    stroke
                        .dashes
                        .iter()
                        .map(|value| value.value())
                        .collect::<Vec<_>>(),
                    dashes
                );
                assert_eq!(stroke.dash_offset, offset);
            },
        );
    }
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_stroke_dash_caps.aep",
        152,
        || {
            let keyed = fresh_import("import_stroke_dash_caps.aep", 152, "DASH_OFFSET_KEYED");
            assert_track(
                &keyed,
                "strokeDashOffset",
                serde_json::json!({"type":"float","value":0.0}),
                serde_json::json!({"type":"float","value":80.0}),
            );
        },
    );
    cases.finish();
}

fn native_color(channels: [f32; 4]) -> Value {
    // Native keys preserve the authored channels at f32 precision, widened to f64.
    serde_json::json!({"type":"color","value":channels.map(f64::from)})
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_color_animation_imports_normalized_editable_tracks() {
    for (id, name, property) in [
        (1, "KEYED_FILL_COLOR", "fillColor"),
        (32, "KEYED_STROKE_COLOR", "strokeColor"),
    ] {
        let converted = fresh_import("import_shape_control_animation.aep", id, name);
        let mut geometry = Vec::new();
        collect_geometry(&foreground(&converted).layers, &mut geometry);
        assert!(
            geometry
                .iter()
                .any(|layer| matches!(layer, FxLayer::Shape(_) | FxLayer::Rect(_)))
        );
        let json: Value =
            serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap();
        let tracks: Vec<_> = json["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["target"]["propertyType"] == property)
            .collect();
        assert_eq!(tracks.len(), 1, "one editable {property} track");
        let keys = tracks[0]["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["value"], native_color([0.8, 0.2, 0.1, 1.0]));
        assert_eq!(keys[1]["value"], native_color([0.1, 0.8, 0.3, 1.0]));
        // Pinned ldat times are 12288 and 49152 on the 24576-tick clock.
        assert_eq!(keys[0]["layerTime"], 500);
        assert_eq!(keys[1]["layerTime"], 2000);
        assert!(keys.iter().all(|key| key["easing"]["type"] == "linear"));
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_control_animation_imports_typed_editable_tracks() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, property, first, last) in [
        (
            1,
            "KEYED_FILL_COLOR",
            "fillColor",
            native_color([0.8, 0.2, 0.1, 1.0]),
            native_color([0.1, 0.8, 0.3, 1.0]),
        ),
        (
            17,
            "KEYED_FILL_OPACITY",
            "opacity",
            serde_json::json!({"type":"float","value":20.0}),
            serde_json::json!({"type":"float","value":90.0}),
        ),
        (
            32,
            "KEYED_STROKE_COLOR",
            "strokeColor",
            native_color([0.8, 0.2, 0.1, 1.0]),
            native_color([0.1, 0.8, 0.3, 1.0]),
        ),
        (
            47,
            "KEYED_STROKE_WIDTH",
            "strokeWidth",
            serde_json::json!({"type":"float","value":4.0}),
            serde_json::json!({"type":"float","value":30.0}),
        ),
        (
            62,
            "KEYED_STROKE_OPACITY",
            "opacity",
            serde_json::json!({"type":"float","value":20.0}),
            serde_json::json!({"type":"float","value":90.0}),
        ),
        (
            77,
            "KEYED_MITER_LIMIT",
            "strokeMiterLimit",
            serde_json::json!({"type":"float","value":2.0}),
            serde_json::json!({"type":"float","value":10.0}),
        ),
        (
            92,
            "KEYED_ROUND",
            "roundCornersRadius",
            serde_json::json!({"type":"float","value":0.0}),
            serde_json::json!({"type":"float","value":45.0}),
        ),
        (
            107,
            "KEYED_OFFSET",
            "offsetPathsAmount",
            serde_json::json!({"type":"float","value":-15.0}),
            serde_json::json!({"type":"float","value":40.0}),
        ),
        (
            122,
            "KEYED_TRIM_START",
            "trimStart",
            serde_json::json!({"type":"float","value":0.0}),
            serde_json::json!({"type":"float","value":60.0}),
        ),
        (
            137,
            "KEYED_TRIM_END",
            "trimEnd",
            serde_json::json!({"type":"float","value":20.0}),
            serde_json::json!({"type":"float","value":100.0}),
        ),
        (
            152,
            "KEYED_TRIM_OFFSET",
            "trimOffset",
            serde_json::json!({"type":"float","value":0.0}),
            serde_json::json!({"type":"float","value":180.0}),
        ),
        (
            167,
            "KEYED_ELLIPSE_SIZE",
            "ellipseSize",
            serde_json::json!({"type":"vector2","value":[200.0,120.0]}),
            serde_json::json!({"type":"vector2","value":[450.0,300.0]}),
        ),
        (
            182,
            "KEYED_ELLIPSE_POSITION",
            "ellipsePosition",
            serde_json::json!({"type":"vector2","value":[-100.0,0.0]}),
            serde_json::json!({"type":"vector2","value":[100.0,0.0]}),
        ),
        (
            197,
            "KEYED_STAR_POINTS",
            "polyStarPoints",
            serde_json::json!({"type":"float","value":5.0}),
            serde_json::json!({"type":"float","value":8.0}),
        ),
        (
            212,
            "KEYED_STAR_POSITION",
            "polyStarPosition",
            serde_json::json!({"type":"vector2","value":[-100.0,0.0]}),
            serde_json::json!({"type":"vector2","value":[100.0,0.0]}),
        ),
        (
            227,
            "KEYED_STAR_ROTATION",
            "polyStarRotation",
            serde_json::json!({"type":"float","value":0.0}),
            serde_json::json!({"type":"float","value":70.0}),
        ),
        (
            242,
            "KEYED_STAR_INNER_RADIUS",
            "polyStarInnerRadius",
            serde_json::json!({"type":"float","value":35.0}),
            serde_json::json!({"type":"float","value":90.0}),
        ),
        (
            257,
            "KEYED_STAR_OUTER_RADIUS",
            "polyStarOuterRadius",
            serde_json::json!({"type":"float","value":100.0}),
            serde_json::json!({"type":"float","value":200.0}),
        ),
        (
            272,
            "KEYED_STAR_INNER_ROUNDNESS",
            "polyStarInnerRoundness",
            serde_json::json!({"type":"float","value":0.0}),
            serde_json::json!({"type":"float","value":30.0}),
        ),
        (
            287,
            "KEYED_STAR_OUTER_ROUNDNESS",
            "polyStarOuterRoundness",
            serde_json::json!({"type":"float","value":0.0}),
            serde_json::json!({"type":"float","value":30.0}),
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_shape_control_animation.aep",
            id,
            || {
                let converted = fresh_import("import_shape_control_animation.aep", id, name);
                assert!(matches!(only_geometry(&converted), FxLayer::Shape(_)));
                assert_track(&converted, property, first, last);
                if name == "KEYED_STAR_POINTS" {
                    assert!(
                        converted.diagnostics.iter().any(|diagnostic| diagnostic
                            .message
                            .contains("point counts are floored"))
                    );
                }
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_boolean_paint_blend_stays_on_editable_boolean_geometry() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, stroke) in [
        (1, "BOOLEAN_FILL_MULTIPLY", false),
        (17, "BOOLEAN_STROKE_MULTIPLY", true),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_boolean_paint_blend.aep",
            id,
            || {
                let converted = fresh_import("import_boolean_paint_blend.aep", id, name);
                let FxLayer::BooleanOperation(boolean) = only_geometry(&converted) else {
                    panic!("{name} must remain an editable Boolean operation")
                };
                assert_eq!(boolean.op, fx_composition::BooleanOp::Union);
                assert_eq!(boolean.blend_mode, fx_composition::BlendMode::Multiply);
                assert_eq!(boolean.layers.len(), 2);
                if stroke {
                    assert!(boolean.fills.is_empty());
                    assert_eq!(boolean.strokes[0].width.value(), 30.0);
                } else {
                    assert!(boolean.strokes.is_empty());
                    assert_eq!(boolean.fills.len(), 1);
                }
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn reversed_parametric_controls_retain_direction_and_trim() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, kind) in [
        (1, "ELLIPSE_REVERSED", 0),
        (17, "STAR_REVERSED", 1),
        (32, "POLYGON_REVERSED", 2),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_remaining_mapped_controls.aep",
            id,
            || {
                let converted = fresh_import("import_remaining_mapped_controls.aep", id, name);
                let FxLayer::Shape(shape) = only_geometry(&converted) else {
                    panic!("{name} must remain editable")
                };
                assert_eq!(shape.shape.trim.as_ref().unwrap().end, 60.0);
                assert_eq!(shape.shape.strokes[0].width.value(), 16.0);
                match kind {
                    0 => assert!(shape.shape.ellipse.as_ref().unwrap().reversed),
                    1 => assert!(shape.shape.poly_star.as_ref().unwrap().reversed),
                    2 => {
                        let star = shape.shape.poly_star.as_ref().unwrap();
                        assert!(star.reversed);
                        assert_eq!(star.star_type, fx_composition::ShapePolyStarType::Polygon);
                    }
                    _ => unreachable!(),
                }
            },
        );
    }
    cases.finish();
}
