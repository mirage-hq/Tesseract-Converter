use super::*;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/expression_samples/expression_paths.aep");
const READBACK: &str =
    include_str!("../../tests/fixtures/expression_samples/expression_paths.readback.json");
const SOURCE_SHA256: &str = "f06e790dd67d78920dc72b8edcafb29f10e0bdb04567da5669b7c82411d539f6";

fn source() -> crate::structure::StructuralProject {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    crate::structure::read_project(SOURCE).unwrap()
}

fn named<'a>(layers: &'a [fx_schema::Layer], name: &str) -> Option<&'a fx_schema::Layer> {
    layers.iter().find_map(|layer| {
        if layer.name() == name {
            Some(layer)
        } else if let fx_schema::LayerData::Group(group) = layer.data() {
            named(&group.layers, name)
        } else {
            None
        }
    })
}

fn descendant_ids(layer: &fx_schema::Layer, ids: &mut Vec<fx_schema::LayerId>) {
    ids.push(layer.id());
    if let fx_schema::LayerData::Group(group) = layer.data() {
        for child in &group.layers {
            descendant_ids(child, ids);
        }
    }
}

#[test]
fn native_path_created_and_timed_consumers_are_sparse_editable_paths() {
    use fx_schema::{PropType, PropertyValue, ShapePathCommand};
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    assert_eq!(readback["source_sha256"], SOURCE_SHA256);
    let converted =
        crate::structure_document::to_structural_fx_document(&source(), Some(1)).unwrap();
    let json = converted.document.to_json_value().unwrap().to_string();
    assert!(!json.to_ascii_lowercase().contains("jsscript"));
    let entries = converted.document.composition().dynamics().entries();
    for (name, count, first_x, last_x, closed) in [
        ("CreatedClosed", 2, -105.0, -75.0, true),
        ("TimedCopy", 1, -100.0, -100.0, false),
    ] {
        let owner = named(converted.document.composition().layers(), name).unwrap();
        let mut ids = Vec::new();
        descendant_ids(owner, &mut ids);
        let matches: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry.target.layer_id().is_some_and(|id| ids.contains(&id))
                    && entry
                        .target
                        .as_property()
                        .is_some_and(|p| p.property_type() == PropType::ShapePath)
            })
            .collect();
        let [entry] = matches.as_slice() else {
            panic!(
                "{name}: missing editable expression Path track; found {}",
                matches.len()
            );
        };
        let track = entry.animator.keyframe_track().unwrap();
        track.validate_for_target(&entry.target).unwrap();
        assert_eq!(
            track.keyframes().len(),
            count,
            "{name}: sparse keys, not frame observations"
        );
        for (key, expected_x) in [
            (track.keyframes().first().unwrap(), first_x),
            (track.keyframes().last().unwrap(), last_x),
        ] {
            let PropertyValue::Path(path) = key.value() else {
                panic!("{name}: non-Path key")
            };
            let Some(ShapePathCommand::MoveTo { x, .. }) = path.commands.first() else {
                panic!("{name}: no start vertex")
            };
            assert!(
                (*x - expected_x).abs() < 1e-3,
                "{name}: native start vertex mismatch"
            );
            assert_eq!(
                matches!(path.commands.last(), Some(ShapePathCommand::Close)),
                closed
            );
        }
        if count == 2 {
            assert_eq!(track.keyframes()[0].layer_time().as_millis(), 0);
            assert_eq!(track.keyframes()[1].layer_time().as_millis(), 2000);
        }
    }
}

#[test]
fn native_copied_open_arrays_match_independent_adobe_geometry() {
    let project = source();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("missing native composition")
    };
    let actual = evaluate_occurrence(&items, 1, comp, &ExpressionSamples::default(), false);
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    let case = readback["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["layer"] == "CopiedOpen")
        .unwrap();
    let layer_id = u32::try_from(case["layer_id"].as_u64().unwrap()).unwrap();
    let evaluated = actual
        .paths
        .iter()
        .find(|path| path.layer_id == layer_id)
        .unwrap_or_else(|| panic!("missing copied Path: {:?}", actual.errors()));
    let rows = case["samples"].as_array().unwrap();
    assert_eq!(evaluated.values.len(), rows.len());
    for (value, row) in evaluated.values.iter().zip(rows) {
        let expected: crate::properties::NativePathValue =
            serde_json::from_value(row["value"].clone()).unwrap();
        assert_eq!(value.closed, expected.closed);
        for (values, expected) in [
            (&value.vertices, &expected.vertices),
            (&value.in_tangents, &expected.in_tangents),
            (&value.out_tangents, &expected.out_tangents),
        ] {
            assert_eq!(values.len(), expected.len());
            for (value, expected) in values.iter().flatten().zip(expected.iter().flatten()) {
                assert!(
                    (value - expected).abs() < 1e-3,
                    "native copied geometry mismatch at {}",
                    row["time"]
                );
            }
        }
    }
}

#[test]
fn native_path_numeric_consumers_match_independent_adobe_samples() {
    let project = source();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("missing native composition")
    };
    let actual = evaluate_occurrence(&items, 1, comp, &ExpressionSamples::default(), false);
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    for case in readback["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "numeric")
    {
        let id = u32::try_from(case["layer_id"].as_u64().unwrap()).unwrap();
        let identity = PropertyIdentity::Transform {
            match_name: "ADBE Position".into(),
        };
        let values = actual.lookup(1, id, &identity).unwrap_or_else(|| {
            panic!(
                "{}: missing evaluated native Path consumer; {:?}",
                case["layer"],
                actual.errors()
            )
        });
        assert_eq!(values.values().len(), 61);
        for (row, expected) in values
            .values()
            .iter()
            .zip(case["samples"].as_array().unwrap())
        {
            let expected = expected["value"].as_array().unwrap();
            assert_eq!(row.len(), expected.len());
            for (value, expected) in row.iter().zip(expected) {
                assert!(
                    (value - expected.as_f64().unwrap()).abs() < 1e-3,
                    "{}: native Path API value mismatch at {}",
                    case["layer"],
                    expected
                );
            }
        }
    }
}
