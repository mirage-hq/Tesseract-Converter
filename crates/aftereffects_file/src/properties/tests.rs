use super::*;
use crate::structure::{ItemKind, read_project};
use sha2::{Digest, Sha256};

/// Key values from the SHA-pinned Adobe JSON sidecar (see fixture provenance),
/// not from the native writer or a round trip. AE's spatial header has an extra
/// double before its temporal ease, unlike ordinary scalar/vector records.
#[test]
fn native_spatial_position_keys_match_independent_adobe_values() {
    let bytes = include_bytes!("../../tests/fixtures/properties/property_2D_position.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "afd303bec0c345010c15869e1e85ceeced05ee710cbe039914200b60d11b93de"
    );
    let project = read_project(bytes).unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("pinned composition 1")
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 15)
        .unwrap();
    let properties = read_transform(&layer.content).unwrap();
    let numeric = properties
        .iter()
        .find(|p| p.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(numeric.keyframes.len(), 2);
    assert_eq!(numeric.keyframes[0].time_secs, 0.0);
    assert_eq!(numeric.keyframes[0].values, [0.0, 0.0, 0.0]);
    assert_eq!(numeric.keyframes[1].time_secs, 5.0);
    assert_eq!(numeric.keyframes[1].values, [100.0, 100.0, 0.0]);
    for key in &numeric.keyframes {
        assert_eq!(key.spatial_in, [0.0, 0.0, 0.0]);
        assert_eq!(key.spatial_out, [0.0, 0.0, 0.0]);
    }
}

#[test]
fn native_properties_match_independent_adobe_sidecar_values_and_flags() {
    let manifest: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../tests/fixtures/properties/provenance.json"
    ))
    .unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        let file = case["file"].as_str().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/properties")
            .join(file);
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            case["sha256"].as_str().unwrap()
        );
        assert_eq!(bytes.len() as u64, case["bytes"].as_u64().unwrap());
        let project = read_project(&bytes).unwrap();
        for comp in case["compositions"].as_array().unwrap() {
            let ItemKind::Composition(actual) = &project
                .item(comp["id"].as_u64().unwrap() as u32)
                .unwrap()
                .kind
            else {
                panic!("not comp")
            };
            for expected in comp["layers"].as_array().unwrap() {
                let layer = actual
                    .layers
                    .iter()
                    .find(|l| u64::from(l.record.id()) == expected["id"].as_u64().unwrap())
                    .unwrap();
                let decoded = read_transform(&layer.content).unwrap();
                for p in expected["properties"].as_array().unwrap() {
                    let name = p["matchName"].as_str().unwrap();
                    let Some(property) = decoded.iter().find(|d| d.match_name == name) else {
                        continue;
                    }; // Native default properties may be omitted.
                    let numeric = property.numeric.as_ref().unwrap();
                    assert_eq!(
                        numeric.animated,
                        p["numKeys"].as_u64().unwrap_or(0) != 0,
                        "{file}: {name}"
                    );
                    assert_eq!(
                        numeric.expression_enabled,
                        p["expressionEnabled"].as_bool().unwrap_or(false),
                        "{file}: {name}"
                    );
                    if let Some(separated) = p["dimensionsSeparated"].as_bool() {
                        assert_eq!(numeric.dimensions_separated, separated, "{file}: {name}");
                    }
                    if numeric.animated {
                        continue;
                    } // Stored cdat is not an animation sample.
                    let values: Vec<f64> = match &p["value"] {
                        serde_json::Value::Array(a) => {
                            a.iter().map(|v| v.as_f64().unwrap()).collect()
                        }
                        v => vec![v.as_f64().unwrap()],
                    };
                    let factor = if matches!(name, "ADBE Scale" | "ADBE Opacity") {
                        100.0
                    } else {
                        1.0
                    };
                    assert_eq!(
                        numeric.values.len(),
                        values.len(),
                        "{file}: {name}: dimensions"
                    );
                    for (raw, expected) in numeric.values.iter().zip(values) {
                        assert!(
                            (raw * factor - expected).abs() < 1e-6,
                            "{file}: {name}: {raw} != {expected}"
                        );
                    }
                }
                if file == "transform_unseparated.aep" {
                    assert!(
                        decoded.iter().all(|p| !matches!(
                            p.match_name.as_str(),
                            "ADBE Anchor Point"
                                | "ADBE Position"
                                | "ADBE Scale"
                                | "ADBE Rotate Z"
                                | "ADBE Opacity"
                                | "ADBE Position_0"
                                | "ADBE Position_1"
                        )),
                        "native default 2D values are omitted"
                    );
                } else {
                    assert!(
                        !decoded.is_empty(),
                        "{file}: expected stored Transform leaves"
                    );
                }
                let animated_count = expected["properties"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|p| p["numKeys"].as_u64().unwrap_or(0) != 0)
                    .count();
                assert_eq!(
                    decoded
                        .iter()
                        .filter(|p| p.numeric.as_ref().is_ok_and(|n| n.animated))
                        .count(),
                    animated_count,
                    "{file}: animated leaf coverage"
                );
            }
        }
    }
}

fn numeric(values: &[f64]) -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = values.len() as u8;
    vec![
        Chunk::data(*b"tdb4", meta).unwrap(),
        Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
        Chunk::data(
            *b"cdat",
            values
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    ]
}

#[test]
fn legacy_expression_chunk_prevents_static_evaluation_without_marker() {
    let mut chunks = numeric(&[1.0]);
    chunks.push(Chunk::data(*b"expr", b"arbitraryNativeExpression();").unwrap());
    let decoded = read_numeric(&chunks).unwrap();
    assert!(decoded.expression_present);
    assert!(decoded.expression_enabled);
}

#[test]
fn group_enabled_reads_only_its_own_header_and_not_collapse_state() {
    for flags in 0..4 {
        let run = vec![Chunk::list(
            *b"tdgp",
            vec![Chunk::data(*b"tdsb", vec![0, 0, 0, flags]).unwrap()],
        )];
        assert_eq!(group_enabled(&run).unwrap(), flags & 1 != 0);
    }
    let nested = Chunk::list(
        *b"tdbs",
        vec![Chunk::data(*b"tdsb", vec![0, 0, 0, 0]).unwrap()],
    );
    assert!(group_enabled(&[Chunk::list(*b"tdgp", vec![nested])]).unwrap());
    for flags in [vec![], vec![0; 3], vec![0; 5]] {
        let run = vec![Chunk::list(
            *b"tdgp",
            vec![Chunk::data(*b"tdsb", flags).unwrap()],
        )];
        let mut warnings = Vec::new();
        assert!(group_enabled_or_warn(&run, "test", &mut warnings));
        assert_eq!(warnings.len(), 1);
    }
    let flag = Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap();
    assert!(group_enabled(&[Chunk::list(*b"tdgp", vec![flag.clone(), flag])]).is_err());
}

#[test]
fn rejects_nonfinite_truncated_and_duplicate_numeric_records() {
    assert_eq!(
        read_numeric(&numeric(&[f64::NAN])),
        Err(PropertyError::NonFinite)
    );
    let mut chunks = numeric(&[1.0]);
    chunks[2] = Chunk::data(*b"cdat", vec![0; 7]).unwrap();
    assert!(read_numeric(&chunks).is_err());
    let mut chunks = numeric(&[1.0]);
    chunks.push(chunks[0].clone());
    assert!(read_numeric(&chunks).is_err());
}

#[test]
fn expression_and_animation_markers_prevent_static_evaluation() {
    let mut chunks = numeric(&[1.0]);
    let mut meta = chunks[0].data_payload().unwrap().to_vec();
    meta[120] = 1;
    chunks[0] = Chunk::data(*b"tdb4", meta.clone()).unwrap();
    assert!(read_numeric(&chunks).unwrap().expression_enabled);
    meta[119] = 1;
    chunks[0] = Chunk::data(*b"tdb4", meta).unwrap();
    let decoded = read_numeric(&chunks).unwrap();
    assert!(decoded.expression_present);
    assert!(!decoded.expression_enabled);
    chunks.push(Chunk::list(*b"list", vec![]));
    assert_eq!(
        read_numeric(&chunks),
        Err(PropertyError::Layout("missing numeric record"))
    );
}

#[test]
fn native_orientation_wrapper_decodes_static_and_keyed_values() {
    for (bytes, expected_static, expected_keys) in [
        (
            include_bytes!("../../tests/fixtures/properties/orientation_5_0_0.aep").as_slice(),
            vec![5.0, 0.0, 0.0],
            Vec::new(),
        ),
        (
            include_bytes!("../../tests/fixtures/properties/orientation_with_keyframes.aep")
                .as_slice(),
            Vec::new(),
            vec![[5.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
        ),
    ] {
        let project = read_project(bytes).expect("pinned native Orientation AEP must parse");
        let orientation = project
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(composition) => Some(composition),
                _ => None,
            })
            .flat_map(|composition| &composition.layers)
            .find_map(|layer| {
                read_transform(&layer.content)
                    .ok()?
                    .into_iter()
                    .find(|property| property.match_name == "ADBE Orientation")
            })
            .expect("native fixture must contain Orientation")
            .numeric
            .expect("native Orientation layout must decode");
        assert_eq!(orientation.values, expected_static);
        assert_eq!(
            orientation
                .keyframes
                .iter()
                .map(|key| key.values.as_slice())
                .collect::<Vec<_>>(),
            expected_keys
                .iter()
                .map(<[f64; 3]>::as_slice)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn animated_colors_decode_native_argb_units_without_clamping() {
    let mut meta = vec![0; 124];
    meta[12..16].copy_from_slice(&24_576_u32.to_be_bytes());
    let mut header = vec![0; 24];
    header[10..12].copy_from_slice(&1_u16.to_be_bytes());
    header[18..20].copy_from_slice(&152_u16.to_be_bytes());
    header[23] = 4;
    for (native, expected) in [
        ([255.0_f64, 51.0, 102.0, 153.0], [0.2, 0.4, 0.6, 1.0]),
        ([63.75, -63.75, 382.5, 127.5], [-0.25, 1.5, 0.5, 0.25]),
    ] {
        let mut item = vec![0; 152];
        item[..4].copy_from_slice(&12_288_i32.to_be_bytes());
        item[4] = 1;
        item[5] = 2;
        for (slot, value) in item[24..56]
            .chunks_exact_mut(8)
            .zip([2.0_f64, 0.25, 3.0, 0.75])
        {
            slot.copy_from_slice(&value.to_be_bytes());
        }
        for (slot, value) in item[56..88].chunks_exact_mut(8).zip(native) {
            slot.copy_from_slice(&value.to_be_bytes());
        }
        let chunks = [Chunk::list(
            *b"list",
            vec![
                Chunk::data(*b"lhd3", header.clone()).unwrap(),
                Chunk::data(*b"ldat", item).unwrap(),
            ],
        )];
        let keys =
            keyframes::read_keyframes(&chunks, &meta, 4, NumericValueKind::Color, None).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].values, expected);
        assert_eq!(keys[0].time_secs, 0.5);
        assert_eq!(keys[0].in_interpolation, 1);
        assert_eq!(keys[0].out_interpolation, 2);
        assert_eq!(keys[0].in_speed, [2.0]);
        assert_eq!(keys[0].out_speed, [3.0]);
        assert_eq!(keys[0].in_influence, [25.0]);
        assert_eq!(keys[0].out_influence, [75.0]);
    }
}

#[test]
fn color_and_integer_numeric_types_use_pinned_native_layouts() {
    let mut color = numeric(&[255.0, 51.0, 102.0, 153.0]);
    let mut meta = color[0].data_payload().unwrap().to_vec();
    meta[3] = 4;
    meta[59] = 1;
    color[0] = Chunk::data(*b"tdb4", meta).unwrap();
    let decoded = read_numeric(&color).unwrap();
    assert_eq!(decoded.value_kind, NumericValueKind::Color);
    assert_eq!(decoded.values, [0.2, 0.4, 0.6, 1.0]);

    let mut integer = numeric(&[2.0]);
    let mut meta = integer[0].data_payload().unwrap().to_vec();
    meta[59] = 4;
    integer[0] = Chunk::data(*b"tdb4", meta).unwrap();
    let decoded = read_numeric(&integer).unwrap();
    assert_eq!(decoded.value_kind, NumericValueKind::Integer);
    assert_eq!(decoded.values, [2.0]);

    let bytes = include_bytes!("../../tests/fixtures/shapes/shape_basic.aep");
    let project = read_project(bytes).expect("pinned native shape AEP must parse");
    let mut native_colors = Vec::new();
    for item in &project.items {
        let ItemKind::Composition(composition) = &item.kind else {
            continue;
        };
        for layer in &composition.layers {
            collect_named_numeric(&layer.content, "ADBE Shadow Color", &mut native_colors);
        }
    }
    assert!(
        !native_colors.is_empty(),
        "native shape fixture must contain colors"
    );
    assert!(native_colors.iter().all(|color| {
        color.value_kind == NumericValueKind::Color
            && color.values.len() == 4
            && color
                .values
                .iter()
                .all(|channel| (0.0..=1.0).contains(channel))
    }));
    assert!(
        native_colors
            .iter()
            .any(|color| color.values == [0.0, 0.0, 0.0, 1.0]),
        "native ARGB storage must resolve to the sidecar's opaque RGBA black"
    );
}

fn collect_named_numeric(chunks: &[Chunk], name: &str, output: &mut Vec<NumericProperty>) {
    if let Ok(named_runs) = runs(chunks) {
        for (candidate, run) in named_runs {
            if candidate == name
                && let Ok(list) = unique_list(run, *b"tdbs")
                && let Ok(value) = read_numeric(list)
            {
                output.push(value);
            }
        }
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        collect_named_numeric(children, name, output);
    }
}
