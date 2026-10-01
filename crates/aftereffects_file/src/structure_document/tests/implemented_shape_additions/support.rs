use super::*;

macro_rules! source {
    ($file:literal, $data:expr, $sha:literal, $bytes:literal, $oracle:literal) => {
        NativeSource {
            file: $file,
            bytes: $data,
            sha256: $sha,
            byte_count: $bytes,
            oracle: $oracle,
        }
    };
}

pub(super) struct NativeSource {
    pub(super) file: &'static str,
    pub(super) bytes: &'static [u8],
    pub(super) sha256: &'static str,
    pub(super) byte_count: usize,
    pub(super) oracle: &'static str,
}

pub(super) const SOURCES: &[NativeSource] = &[
    source!(
        "native_boolean_operations.aep",
        include_bytes!(
            "../../../../tests/fixtures/implemented_additions/native_boolean_operations.aep"
        ),
        "4d5f14663dbfbb20fa376d004472b773a14466c3c6ec5a1c0e8bd6a2935a014e",
        270_029,
        "historical Adobe authoring readback SHA 4620c06a6dff494fc0731f2ed9bf19bd28af42f33e00c40879dcf51ba9bb242c"
    ),
    source!(
        "native_static_paints_and_geometry.aep",
        include_bytes!(
            "../../../../tests/fixtures/implemented_additions/native_static_paints_and_geometry.aep"
        ),
        "14079d1d737c0c64921ff2b2c32acf455a2b0595e7be8b13c6805d892c865226",
        529_749,
        "historical Adobe authoring readback SHA b615b0685a7d0aa4aa43b9ee61db211d65099521b48db710e4f47a7b1fac895d"
    ),
    source!(
        "native_transform_keys.aep",
        include_bytes!(
            "../../../../tests/fixtures/implemented_additions/native_transform_keys.aep"
        ),
        "09674f30f1d4544885f355a210c89bb51c05c4cbb1e875fa545c955f1097d43e",
        462_845,
        "historical Adobe authoring readback SHA 5c38d82bb512d8e838dfce575b790e3c7214793a6fdba10d688f0c3e26d94a4f"
    ),
    source!(
        "native_boolean_operand_structures.aep",
        include_bytes!(
            "../../../../tests/fixtures/implemented_additions/native_boolean_operand_structures.aep"
        ),
        "ad7a9d76dbb06d855b1ac181c92ae4f8e8f1febd315711179d5fcf14ca9f42ca",
        667_027,
        "historical Adobe authoring readback SHA 3594347a26bbb58cbfcce9695c292daf26f2186e45ac04cf1cc431e4ca321179"
    ),
    source!(
        "native_parametric_key_channels.aep",
        include_bytes!(
            "../../../../tests/fixtures/implemented_additions/native_parametric_key_channels.aep"
        ),
        "7c10eecb3b40883418c60fe7760bd0641bd7f6eb244f933b2fc3c298c1c9b56f",
        913_575,
        "historical Adobe authoring readback retained before fixture-sidecar cleanup"
    ),
    source!(
        "import_gradient_controls.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_gradient_controls.aep"),
        "5406fe7f92d697e0882d11a8804c436785b51f533d813419364826b5f35ffcce",
        625_839,
        "/tmp/aep-author-batch2.jsx"
    ),
    source!(
        "import_gradient_stroke_details.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_gradient_stroke_details.aep"),
        "17fc50911102dec5cde381ab8ef234c62d315e5b9d3d128dd6d5bc3fe43fe919",
        270_693,
        "/tmp/aep-author-image-gradient-clocks.jsx"
    ),
    source!(
        "import_group_transform_controls.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_group_transform_controls.aep"),
        "ea4ba57dc9d672d4e321208298541018464b88a2fce11c25a4ff9ff35d371ed5",
        1_083_591,
        "/tmp/aep-author-batch2.jsx"
    ),
    source!(
        "import_isolated_native_gradients.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_isolated_native_gradients.aep"),
        "1cb781c461114a4044e635c24cae6b0802c386ed13277eae8cfdec8d2d1c4d63",
        142_405,
        "/tmp/aep-author-gradient-nested.jsx plus pinned py-aep gradient sidecar"
    ),
    source!(
        "import_modifier_controls.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_modifier_controls.aep"),
        "5bc36fb8210de9bde79d2313b07c4c677e1c040e1d7764ca3a49ba9ed9fcb7e2",
        866_707,
        "/tmp/aep-author-remaining-batch.jsx"
    ),
    source!(
        "import_modifier_order_cases.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_modifier_order_cases.aep"),
        "c52c7ff4a2635e2011ba53fc418dcc1363bc32dd20b99138ae928ce1a61dc303",
        635_723,
        "/tmp/aep-author-combinations.jsx"
    ),
    source!(
        "import_path_direction_cases.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_path_direction_cases.aep"),
        "59946f45a301fd5530522f46e109e38827d69484efdc57f8107a8fda92edf3c6",
        548_289,
        "/tmp/aep-author-shape-details.jsx"
    ),
    source!(
        "import_polygon_animation.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_polygon_animation.aep"),
        "f78b9bc9e43cd2402da9e6c0c2710b4c7e2786a60c05d6268ac563daa0fd330f",
        330_913,
        "/tmp/aep-author-structure-blend-keyed.jsx"
    ),
    source!(
        "import_remaining_mapped_controls.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_remaining_mapped_controls.aep"),
        "aa080cbb30a474f579c7acfa7ba30bb173afef5b6eb73b7e513c5cdf17bd1c29",
        319_471,
        "/tmp/aep-author-final-eleven.jsx"
    ),
    source!(
        "import_shape_blend_ownership.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_shape_blend_ownership.aep"),
        "938cc1f441213b28af9ce1d703fc6787536b55692e2bcee377ffe4d86deed8b1",
        238_521,
        "/tmp/aep-author-structure-blend-keyed.jsx"
    ),
    source!(
        "import_shape_flags_order.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_shape_flags_order.aep"),
        "8946ba3422a3fb93a43583e4aa379dd8b874c6e5885450b234ebcf8f15726727",
        788_663,
        "/tmp/aep-author-shape-details.jsx"
    ),
    source!(
        "import_solid_paint_controls.aep",
        include_bytes!("../../../../tests/fixtures/shapes/import_solid_paint_controls.aep"),
        "4c7115020ff2a81da03f205a025688dc1dddf03f963c8fdbaa2dece6cd2650bf",
        935_211,
        "/tmp/jerboa-aep-import-solid-paint-controls-20260926.jsx plus readback"
    ),
];

pub(super) fn pinned_source(file: &str) -> &'static NativeSource {
    SOURCES
        .iter()
        .find(|source| source.file == file)
        .unwrap_or_else(|| panic!("missing pinned shape-addition source {file}"))
}

pub(super) fn fresh_import(file: &str, comp_id: u32, comp_name: &str) -> StructuralConversion {
    let source = pinned_source(file);
    let project = read_project(source.bytes).unwrap();
    let composition = composition(&project, comp_id);
    assert_eq!(composition.frame_rate, 24.0, "{file}:{comp_id}");
    let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
    assert_imported_canvas_matches_source(composition, &converted, &format!("{file}:{comp_id}"));
    assert_eq!(root(&converted).name, comp_name);
    converted
}

pub(super) fn foreground(converted: &StructuralConversion) -> &GroupLayer {
    as_group(&root(converted).layers[0])
}

fn collect_geometry_from<'a>(layers: &'a [fx_schema::Layer], output: &mut Vec<&'a FxLayer>) {
    for layer in layers {
        match layer.data() {
            FxLayer::Rect(_) | FxLayer::Shape(_) | FxLayer::BooleanOperation(_) => {
                output.push(layer.data());
                if let FxLayer::BooleanOperation(boolean) = layer.data() {
                    collect_geometry_from(&boolean.layers, output);
                }
            }
            FxLayer::Group(group) => collect_geometry_from(&group.layers, output),
            _ => {}
        }
    }
}

pub(super) fn geometry(converted: &StructuralConversion) -> Vec<&FxLayer> {
    let mut output = Vec::new();
    collect_geometry_from(&foreground(converted).layers, &mut output);
    output
}

pub(super) fn diagnostic_contains(converted: &StructuralConversion, needle: &str) -> bool {
    converted
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains(needle))
}

pub(super) fn document_json(converted: &StructuralConversion) -> Value {
    serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap()
}

pub(super) fn contains_field(value: &Value, key: &str, expected: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.get(key) == Some(expected)
                || object
                    .values()
                    .any(|child| contains_field(child, key, expected))
        }
        Value::Array(values) => values
            .iter()
            .any(|child| contains_field(child, key, expected)),
        _ => false,
    }
}

pub(super) fn assert_track(
    converted: &StructuralConversion,
    property: &str,
    first: Value,
    last: Value,
) {
    let json = document_json(converted);
    let matching: Vec<_> = json["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["propertyType"] == property)
        .collect();
    assert_eq!(matching.len(), 1, "expected one editable {property} track");
    let keys = matching[0]["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2, "{property}");
    assert_eq!(keys[0]["value"], first, "{property} first value");
    assert_eq!(keys[1]["value"], last, "{property} last value");
    assert_eq!(keys[0]["layerTime"], 500, "{property} source clock");
    assert_eq!(keys[1]["layerTime"], 2000, "{property} source clock");
}

pub(super) fn paint_kinds(converted: &StructuralConversion) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    for layer in geometry(converted) {
        match layer {
            FxLayer::Rect(rect) => {
                if rect.rect.fill_enabled {
                    kinds.push("fill");
                }
                if rect.rect.stroke_enabled {
                    kinds.push("stroke");
                }
            }
            FxLayer::Shape(shape) => {
                kinds.extend(std::iter::repeat_n("fill", shape.shape.fills.len()));
                kinds.extend(std::iter::repeat_n("stroke", shape.shape.strokes.len()));
            }
            FxLayer::BooleanOperation(boolean) => {
                kinds.extend(std::iter::repeat_n("fill", boolean.fills.len()));
                kinds.extend(std::iter::repeat_n("stroke", boolean.strokes.len()));
            }
            _ => {}
        }
    }
    kinds
}

pub(super) fn paints(converted: &StructuralConversion) -> Vec<&fx_schema::ShapePaint> {
    let mut paints = Vec::new();
    for layer in geometry(converted) {
        match layer {
            FxLayer::Rect(rect) => {
                if let Some(paint) = rect.rect.fill_paint.as_ref() {
                    paints.push(paint);
                }
            }
            FxLayer::Shape(shape) => {
                paints.extend(shape.shape.fills.iter().map(|fill| &fill.paint));
                paints.extend(shape.shape.strokes.iter().map(|stroke| &stroke.paint));
            }
            FxLayer::BooleanOperation(boolean) => {
                paints.extend(boolean.fills.iter().map(|fill| &fill.paint));
                paints.extend(boolean.strokes.iter().map(|stroke| &stroke.paint));
            }
            _ => {}
        }
    }
    paints
}

pub(super) fn command_counts(converted: &StructuralConversion) -> Vec<usize> {
    geometry(converted)
        .into_iter()
        .filter_map(|layer| match layer {
            FxLayer::Shape(shape) => Some(shape.shape.path.commands.len()),
            _ => None,
        })
        .collect()
}
