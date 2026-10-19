//! Independent AE26.5x89 native probes; own-writer sources are not the oracle.
use super::*;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/expression_samples/expression_compatibility.aep");
const READBACK: &str =
    include_str!("../../tests/fixtures/expression_samples/expression_compatibility.readback.json");
const SOURCE_SHA256: &str = "82db8b7462a8784dcd7ba74ab1a5a777e4e0314e171f2614f0b44188958a1b6d";

fn source() -> crate::structure::StructuralProject {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    crate::structure::read_project(SOURCE).unwrap()
}

#[test]
fn native_compatibility_values_match_adobe_and_cached_bounds_are_reported() {
    let project = source();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("native composition1 missing")
    };
    let model = Model::new(&items, 1, comp, true);
    assert_eq!(
        format!("{:x}", Sha256::digest(READBACK.as_bytes())),
        "37efc25788493c1a8762c0cadaddbd2cb960b1a1bea1bdd03a92ff7a755104c0"
    );
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    assert_eq!(readback["composition_id"], 1);
    assert_eq!(readback["cases"].as_array().unwrap().len(), 10);
    for case in readback["cases"].as_array().unwrap() {
        let layer_id = u32::try_from(case["layer_id"].as_u64().unwrap()).unwrap();
        let name = case["layer"].as_str().unwrap();
        let identity = if case["match_name"] == "ADBE Text Document" {
            PropertyIdentity::SourceText {}
        } else {
            PropertyIdentity::Transform {
                match_name: case["match_name"].as_str().unwrap().into(),
            }
        };
        let slot = model
            .properties
            .iter()
            .position(|p| p.layer_id == layer_id && p.identity == identity)
            .unwrap_or_else(|| panic!("{name}: no modelled target"));
        let samples = case["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 31, "{name}");
        let times: Vec<_> = samples
            .iter()
            .map(|s| s["time"].as_f64().unwrap())
            .collect();
        let mut prepared = prepare(&model).unwrap();
        if matches!(identity, PropertyIdentity::SourceText {}) {
            let actual = evaluate_text_grid(&mut prepared, slot, &times)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            for (sample, actual) in samples.iter().zip(actual) {
                assert_eq!(sample["value"].as_str().unwrap(), actual, "{name}");
            }
            continue;
        }
        let actual =
            evaluate_grid(&mut prepared, slot, &times).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut maximum_error = 0.0_f64;
        for (sample, actual) in samples.iter().zip(actual) {
            let expected: Vec<_> = match &sample["value"] {
                serde_json::Value::Array(values) => {
                    values.iter().map(|v| v.as_f64().unwrap()).collect()
                }
                value => vec![value.as_f64().unwrap()],
            };
            assert_eq!(actual.len(), expected.len(), "{name}");
            for (a, e) in actual.iter().zip(expected) {
                maximum_error = maximum_error.max((a - e).abs());
            }
        }
        eprintln!("native compatibility {name}: maximum native-unit error {maximum_error:e}");
        if name == "TextBounds" {
            assert!(
                prepared
                    .approximated
                    .contains("sourceRectAtTime:text-cache")
            );
            // A cached bound is an explicitly measured approximation, not an
            // independent font-layout equality assertion.
            assert!(model.properties[slot].expression_enabled);
        } else {
            assert!(maximum_error <= 1e-3, "{name}: {maximum_error:e}");
        }
    }
}

#[test]
fn cached_bounds_data_bindings_keep_editable_numeric_keys() {
    use crate::{aep::Project, rifx::Chunk};
    use fx_schema::{LayerData, PropType, PropertyValue};

    fn replace(chunks: &mut [Chunk]) -> usize {
        let mut count = 0;
        for chunk in chunks {
            if chunk.id() == *b"Utf8"
                && chunk.data_payload().is_some_and(|bytes| {
                    bytes.starts_with(b"thisComp.layer('Title').sourceRectAtTime(time,false).width")
                })
            {
                *chunk = Chunk::data(
                    *b"Utf8",
                    b"s=thisComp.layer('Title').sourceRectAtTime(time,false); h=s.height; w=s.width; w+0*h".to_vec(),
                )
                .unwrap();
                count += 1;
            } else if let Some(children) = chunk.children_mut() {
                count += replace(children);
            }
        }
        count
    }
    fn owner(layers: &[fx_schema::Layer]) -> Option<fx_schema::LayerId> {
        layers.iter().find_map(|layer| {
            if layer.name() == "TextBounds" {
                Some(layer.id())
            } else if let LayerData::Group(group) = layer.data() {
                owner(&group.layers)
            } else {
                None
            }
        })
    }

    // A code-mutated public source tests admission/lowering, not a new native oracle.
    let mut file = Project::parse(SOURCE).unwrap();
    assert_eq!(replace(&mut file.chunks), 1);
    let project = crate::structure::read_project(&file.encode().unwrap()).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    let owner = owner(converted.document.composition().layers()).unwrap();
    let entries: Vec<_> = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter(|entry| {
            entry.target.layer_id() == Some(owner)
                && entry
                    .target
                    .as_property()
                    .is_some_and(|property| property.property_type() == PropType::Rotation)
        })
        .collect();
    let [entry] = entries.as_slice() else {
        panic!(
            "expected editable TextBounds Rotation keys: {:?}",
            converted.diagnostics
        )
    };
    let track = entry.animator.keyframe_track().unwrap();
    track.validate_for_target(&entry.target).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("native composition missing")
    };
    let model = Model::new(&items, 1, comp, true);
    let title = model.comps[0]
        .layers
        .iter()
        .find(|layer| layer.name == "Title")
        .unwrap();
    // Cached layout is an approximation, not equality to Adobe's text rasterizer.
    let expected = model.properties[title.source_text.unwrap()]
        .text_bounds
        .unwrap()[2];
    assert!(!track.keyframes().is_empty());
    assert!(track.keyframes().iter().all(|key| {
        matches!(key.value(), PropertyValue::Float(value) if (*value-expected).abs()<1e-3)
    }));
    assert!(
        !converted
            .document
            .to_json_value()
            .unwrap()
            .to_string()
            .contains("jsScript")
    );
}

#[test]
fn native_world_vector_basis_uses_reciprocal_scale_not_point_differences() {
    let project = source();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("native composition1 missing")
    };
    let mut model = Model::new(&items, 1, comp, true);
    let slot = model
        .properties
        .iter()
        .position(|p| {
            p.layer_id == 20
                && p.identity
                    == PropertyIdentity::Transform {
                        match_name: "ADBE Position".into(),
                    }
        })
        .unwrap();
    let readback: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/expression_samples/expression_compatibility.vector-readback.json"
    ))
    .unwrap();
    assert_eq!(readback["source_sha256"], SOURCE_SHA256);
    let pinned = include_bytes!(
        "../../tests/fixtures/expression_samples/expression_compatibility.vector-readback.json"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(pinned)),
        "c9471c831fbc0258a26271530a513828051ee675dd4e96dacd73b3209430cbe2"
    );
    assert_eq!(
        readback["author_script_sha256"].as_str().unwrap(),
        format!(
            "{:x}",
            Sha256::digest(include_bytes!(
                "../../tests/fixtures/expression_samples/expression_compatibility.vector-readback.jsx"
            ))
        )
    );
    let subject = "thisComp.layer('Subject')";
    for (name, expression) in [
        ("vector", format!("{subject}.toWorldVec([1,2,3])")),
        ("basisX", format!("{subject}.toWorldVec([1,0,0])")),
        ("basisY", format!("{subject}.toWorldVec([0,1,0])")),
        ("basisZ", format!("{subject}.toWorldVec([0,0,1])")),
        (
            "difference",
            format!(
                "var l={subject}; var a=l.toWorld([1,2,3]); var b=l.toWorld([0,0,0]); [a[0]-b[0],a[1]-b[1],a[2]-b[2]]"
            ),
        ),
    ] {
        model.properties[slot].expression = Some(expression);
        let actual = evaluate_grid(&mut prepare(&model).unwrap(), slot, &[0.0]).unwrap();
        let expected = readback["values"][name].as_array().unwrap();
        assert_eq!(actual[0].len(), expected.len());
        for (actual, expected) in actual[0].iter().zip(expected) {
            let expected = expected.as_f64().unwrap();
            assert!(
                (actual - expected).abs() < 1e-8,
                "native {name} component: {actual} != {expected}"
            );
        }
    }
    model.properties[slot].expression = Some(format!(
        "{subject}.fromWorldVec({})",
        readback["values"]["vector"]
    ));
    let recovered = evaluate_grid(&mut prepare(&model).unwrap(), slot, &[0.0]).unwrap();
    assert_eq!(recovered[0].len(), 3);
    for (actual, expected) in recovered[0].iter().zip([1.0, 2.0, 3.0]) {
        assert!((actual - expected).abs() < 1e-8);
    }
}

#[test]
fn native_compatibility_import_retains_editable_consumers_and_held_echo() {
    use fx_schema::{PropType, PropertyValue};
    let converted =
        crate::structure_document::to_structural_fx_document(&source(), Some(1)).unwrap();
    let json = converted.document.to_json_value().unwrap().to_string();
    assert!(!json.to_ascii_lowercase().contains("jsscript"));
    assert!(
        json.contains("\"text\":\"Lottie!\""),
        "missing editable Echo Source Text"
    );
    fn owner_id(layers: &[fx_schema::Layer], name: &str) -> Option<fx_schema::LayerId> {
        layers.iter().find_map(|layer| {
            if layer.name() == name {
                return Some(layer.id());
            }
            if let fx_schema::LayerData::Group(group) = layer.data() {
                return owner_id(&group.layers, name);
            }
            None
        })
    }
    let entries = converted.document.composition().dynamics().entries();
    for (native_owner, name) in [
        (22, "WorldRoundtrip"),
        (24, "WorldVector"),
        (26, "VectorRoundtrip"),
        (28, "PropertyLoop"),
        (30, "ScalarLength"),
        (32, "ZeroPosterize"),
        (34, "SourceStyle"),
        (36, "TextBounds"),
    ] {
        let owner = owner_id(converted.document.composition().layers(), name).unwrap();
        let matches: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry.target.layer_id().is_some_and(|id| id == owner)
                    && entry
                        .target
                        .as_property()
                        .is_some_and(|p| p.property_type() == PropType::Rotation)
            })
            .collect();
        let [entry] = matches.as_slice() else {
            panic!(
                "native owner {native_owner} / FX {owner:?}: expected one editable Rotation track, found {}",
                matches.len()
            )
        };
        let track = entry.animator.keyframe_track().unwrap();
        track.validate_for_target(&entry.target).unwrap();
        assert!(
            track
                .keyframes()
                .iter()
                .all(|key| matches!(key.value(), PropertyValue::Float(value) if value.is_finite()))
        );
        if native_owner == 28 {
            assert!(
                track
                    .keyframes()
                    .windows(2)
                    .any(|pair| pair[0].value() != pair[1].value()),
                "PropertyLoop must vary, not retain its static original"
            );
        }
        if native_owner == 30 {
            assert!(track.keyframes().iter().all(
                |key| matches!(key.value(), PropertyValue::Float(value) if (*value-50.0).abs()<1e-8)
            ));
        }
    }
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sourceRectAtTime:text-cache"))
    );
}
