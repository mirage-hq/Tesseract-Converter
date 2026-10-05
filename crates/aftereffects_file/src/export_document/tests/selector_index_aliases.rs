//! Pinned native Index Range aliases and fresh editable FX export.
//! Native author/readback is independent; generated-project checks below are
//! structural, not Adobe acceptance or render-fidelity evidence.

use std::collections::{BTreeMap, BTreeSet};

use fx_schema::{FxItemId, PropertyTarget, PropertyValue, TextLayer, TimeOffset};
use sha2::{Digest, Sha256};

use super::*;
use crate::{
    properties::{NumericProperty, read_numeric, runs, unique_list},
    rifx::Chunk,
};

const SOURCE: &[u8] =
    include_bytes!("../../../tests/fixtures/text/import_selector_index_aliases.aep");

fn source_document() -> EditableFxCompositionDocument {
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE)),
        "7b24d1ec81e4bed7dbf3c0fe12590b31f251d195fa2f304c5bd7684a50e4f247"
    );
    let native = read_project(SOURCE).unwrap();
    to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
}

fn texts(layers: &[fx_schema::Layer]) -> Vec<&TextLayer> {
    layers
        .iter()
        .flat_map(|layer| match layer.data() {
            LayerData::Text(text) => vec![text],
            LayerData::Group(group) => texts(&group.layers),
            _ => Vec::new(),
        })
        .collect()
}

fn assert_offset_keys(document: &EditableFxCompositionDocument, id: FxItemId, values: [f64; 2]) {
    let matches = document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter(|entry| {
            matches!(&entry.target, PropertyTarget::FxItemProperty(target)
                if target.item_id() == id && target.property_name() == "offset")
        })
        .collect::<Vec<_>>();
    let [entry] = matches.as_slice() else {
        panic!(
            "one editable offset track for {id:?}, got {}",
            matches.len()
        )
    };
    let keys = entry.animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 2);
    for ((key, time), value) in keys.iter().zip([0, 500]).zip(values) {
        assert_eq!(key.layer_time(), TimeOffset::from_millis(time));
        assert_eq!(key.value(), &PropertyValue::Float(value));
        assert_eq!(key.easing(), PropertyKeyframeEasing::Linear);
    }
}

#[test]
fn native_index_selector_aliases_import_active_units_basis_and_editable_keys() {
    let document = source_document();
    let imported = texts(document.composition().layers());
    assert_eq!(imported.len(), 4);
    for (name, basis) in [
        ("Characters", "characters"),
        ("ExcludingSpaces", "charactersExcludingSpaces"),
        ("Words", "words"),
        ("Lines", "lines"),
    ] {
        let text = imported
            .iter()
            .find(|text| {
                serde_json::to_value(text.animators[0].selectors[0].based_on).unwrap()
                    == json!(basis)
            })
            .unwrap_or_else(|| panic!("native basis for {name}"));
        assert_eq!(text.animators.len(), 1);
        assert_eq!(text.animators[0].position, Some([0.0, 10.0]));
        let selectors = &text.animators[0].selectors;
        assert_eq!(selectors.len(), 2);
        let lead = &selectors[0];
        let copy = &selectors[1];
        assert_eq!(serde_json::to_value(lead.based_on).unwrap(), json!(basis));
        assert_eq!(serde_json::to_value(lead.units).unwrap(), json!("index"));
        assert_eq!([lead.start, lead.end], [1.25, 3.5]);
        assert_offset_keys(&document, lead.id, [-0.5, 1.5]);
        assert_eq!(
            serde_json::to_value(copy.units).unwrap(),
            json!("percentage")
        );
        // Native aliases return Index numbers. Only the receiving Percentage
        // control is normalized for FX, exactly once.
        assert_eq!(
            serde_json::to_value(copy.based_on).unwrap(),
            json!("characters")
        );
        assert_eq!([copy.start, copy.end], [0.0125, 0.035]);
        assert_offset_keys(&document, copy.id, [-0.005, 0.015]);
    }
    let value = document.to_json_value().unwrap().to_string();
    assert!(!value.contains("jsScript"));
}

fn native_selectors(content: &[Chunk]) -> Vec<BTreeMap<String, NumericProperty>> {
    let mut result = Vec::new();
    for chunk in content {
        let Some(children) = chunk.children() else {
            continue;
        };
        if chunk.list_kind() == Some(*b"tdgp") {
            for (name, run) in runs(children).unwrap() {
                if name == "ADBE Text Selector" {
                    let mut fields = BTreeMap::new();
                    numeric_fields(run, &mut fields);
                    result.push(fields);
                } else {
                    result.extend(native_selectors(run));
                }
            }
        } else {
            result.extend(native_selectors(children));
        }
    }
    result
}

fn numeric_fields(content: &[Chunk], fields: &mut BTreeMap<String, NumericProperty>) {
    for chunk in content {
        let Some(children) = chunk.children() else {
            continue;
        };
        if chunk.list_kind() == Some(*b"tdgp") {
            for (name, run) in runs(children).unwrap() {
                if let Ok(storage) = unique_list(run, *b"tdbs") {
                    assert!(
                        fields
                            .insert(name.into(), read_numeric(storage).unwrap())
                            .is_none()
                    );
                } else {
                    numeric_fields(run, fields);
                }
            }
        } else {
            numeric_fields(children, fields);
        }
    }
}

fn edit_start(value: &mut Value, id: u64) -> bool {
    if value.get("id") == Some(&json!(id)) && value.get("units").is_some() {
        value["start"] = json!(0.0225);
        return true;
    }
    match value {
        Value::Object(object) => object.values_mut().any(|value| edit_start(value, id)),
        Value::Array(array) => array.iter_mut().any(|value| edit_start(value, id)),
        _ => false,
    }
}

#[test]
fn native_index_selector_aliases_full_export_retains_active_fields_and_fx_edit() {
    let document = source_document();
    let imported = texts(document.composition().layers());
    let words = imported
        .iter()
        .find(|text| {
            text.animators[0].selectors[0].based_on
                == fx_schema::text_animator::SelectorBasis::Words
        })
        .unwrap();
    let id = words.animators[0].selectors[1].id.value();
    for edited in [false, true] {
        let mut value = document.to_json_value().unwrap();
        if edited {
            assert!(edit_start(&mut value, id));
        }
        let input = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = to_aep(&input).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let native_texts = native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(&comp.layers),
                _ => None,
            })
            .flatten()
            .filter(|layer| layer.record.layer_type() == 3)
            .collect::<Vec<_>>();
        assert_eq!(native_texts.len(), 4, "{:?}", output.diagnostics);
        let mut bases = BTreeSet::new();
        for layer in native_texts {
            let selectors = native_selectors(&layer.content);
            let [lead, copy] = selectors.as_slice() else {
                panic!("two native selectors for {}", layer.name)
            };
            bases.insert(lead["ADBE Text Range Type2"].values[0].to_bits());
            assert_eq!(lead["ADBE Text Range Units"].values, [2.0]);
            assert_eq!(copy["ADBE Text Range Units"].values, [1.0]);
            assert_eq!(copy["ADBE Text Range Type2"].values, [1.0]);
            assert!(!copy.contains_key("ADBE Text Index Start"));
            for (fields, names) in [
                (lead, ["ADBE Text Index Start", "ADBE Text Index End"]),
                (copy, ["ADBE Text Percent Start", "ADBE Text Percent End"]),
            ] {
                for name in names {
                    assert!(!fields[name].expression_enabled);
                    assert!(!fields[name].animated);
                }
            }
            assert_eq!(lead["ADBE Text Index Start"].values, [1.25]);
            assert_eq!(lead["ADBE Text Index End"].values, [3.5]);
            assert!(!lead.contains_key("ADBE Text Percent Start"));
            let start = if edited && lead["ADBE Text Range Type2"].values == [3.0] {
                2.25
            } else {
                1.25
            };
            assert_eq!(copy["ADBE Text Percent Start"].values, [start]);
            // Receiving FX fractions return to native percentages with only
            // the expected binary floating-point roundoff.
            let [end] = copy["ADBE Text Percent End"].values.as_slice() else {
                panic!("one native Percent End value")
            };
            assert!((end - 3.5).abs() < 1e-12);
            for (fields, name) in [
                (lead, "ADBE Text Index Offset"),
                (copy, "ADBE Text Percent Offset"),
            ] {
                let keys = &fields[name].keyframes;
                assert_eq!(keys.len(), 2);
                for ((key, time), value) in keys.iter().zip([0.0, 0.5]).zip([-0.5, 1.5]) {
                    assert_eq!(key.time_secs, time);
                    assert_eq!(key.values, [value]);
                }
                assert!(!fields[name].expression_enabled);
            }
        }
        assert_eq!(bases, [1.0_f64, 2.0, 3.0, 4.0].map(f64::to_bits).into());
    }
}
