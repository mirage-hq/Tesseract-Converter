//! Native-derived guide inputs exercise root Adjustment failure isolation.
//! Fresh AEP readback proves structure, not Adobe acceptance or RGB fidelity.

use super::*;
use crate::properties::{root_runs, runs, unique_list};
use sha2::{Digest, Sha256};

const GUIDE: u64 = 36_010;
const OWNER: u64 = 36_000;

fn named<'a>(layers: &'a [Layer], name: &str) -> Option<&'a Layer> {
    layers.iter().find_map(|layer| {
        if layer.name() == name {
            Some(layer)
        } else if let LayerData::Group(group) = layer.data() {
            named(&group.layers, name)
        } else {
            None
        }
    })
}

fn input(animated: bool) -> Value {
    let bytes = include_bytes!("../../../tests/fixtures/masks/import_mask_controls.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "01380c1f8c5ebe486cd068e5dee50447870b86e4864fc842590a99386dd9b417"
    );
    let native = read_project(bytes).unwrap();
    let imported = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document;
    let source = named(imported.composition().layers(), "target — Mask 1 guide").unwrap();
    assert!(matches!(source.data(), LayerData::Shape(_)));
    let mut guide = serde_json::to_value(source).unwrap();
    guide["id"] = json!(GUIDE);
    guide["name"] = json!("Source-only mask guide");
    guide["parent"] = Value::Null;
    guide["activeRange"] = json!({"start":0,"duration":2_000});
    guide["transform"] = json!({"position":[0.0,0.0],"anchorPoint":[0.0,0.0],
        "scale":[100.0,100.0],"rotation":0.0,"opacity":100.0});
    guide["shape"]["fills"] = json!([{
        "paint":{"type":"solid","color":[1.0,1.0,1.0,1.0]},"opacity":1.0
    }]);
    let mut value = super::imported();
    value["duration"] = json!(2.0);
    let mut picture = rect(&value, 40_000);
    picture["name"] = json!("Retained editable picture");
    picture["activeRange"] = json!({"start":250,"duration":1_500});
    picture["rect"]["size"] = json!([80.0, 60.0]);
    picture["transform"]["position"] = json!([120.0, 90.0]);
    let mut ordinary = guide.clone();
    ordinary["id"] = json!(40_001);
    ordinary["name"] = json!("Ordinary guide-shaped paint");
    value["composition"]["layers"] = json!([
        guide,
        {"type":"Adjustment","id":OWNER,"name":"Soft masked grade",
         "activeRange":{"start":0,"duration":2_000},
         "transform":{"position":[0.0,0.0],"anchorPoint":[0.0,0.0],
                      "scale":[100.0,100.0],"rotation":0.0,"opacity":100.0},
         "masks":[{"id":36_001,"layer":GUIDE,"mode":"add",
                   "inverted":false,"feather":[8.0,8.0],"opacity":1.0,"expansion":0.0}],
         "effects":[{"id":36_002,"enabled":true,"effect":{"type":"hueSaturation",
                      "hue":-7.0,"saturation":12.0,"lightness":1.0}}]},
        ordinary,picture
    ]);
    let mut entries = vec![keyed_entry(
        LayerId::new(OWNER),
        PropType::Opacity,
        [
            (0, PropertyValue::Float(0.0)),
            (500, PropertyValue::Float(22.0)),
            (1_750, PropertyValue::Float(0.0)),
        ],
    )];
    if animated {
        entries.push(keyed_entry(
            LayerId::new(GUIDE),
            PropType::PositionX,
            [
                (500, PropertyValue::Float(146.0)),
                (1_500, PropertyValue::Float(101.0)),
            ],
        ));
    }
    value["composition"]["dynamics"] = json!({"entries":entries});
    value
}

fn names(native: &StructuralProject) -> Vec<&str> {
    layers(native)
        .iter()
        .map(|layer| layer.name.as_ref())
        .collect()
}

#[test]
fn refused_root_adjustment_mask_omits_grade_and_source_only_guide() {
    for guide_first in [true, false] {
        let mut value = input(true);
        if !guide_first {
            value["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .swap(0, 1);
        }
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(
            names(&native),
            ["Ordinary guide-shaped paint", "Retained editable picture"]
        );
        assert!(output.omitted_layer_ids.contains(&LayerId::new(OWNER)));
        assert!(!output.omitted_layer_ids.contains(&LayerId::new(40_000)));
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(OWNER))
                && diagnostic.message.contains("Mask 1 omitted:")
                && diagnostic
                    .message
                    .contains("coordinate or guide geometry animation")
        }));
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(OWNER))
                && diagnostic.message.contains("unlowerable coverage mask")
                && diagnostic
                    .message
                    .contains("source-only guides remain consumed")
        }));
        let retained = to_structural_fx_document(&native, Some(1))
            .unwrap()
            .document;
        assert!(named(retained.composition().layers(), "Retained editable picture").is_some());
        let picture = layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == "Retained editable picture")
            .unwrap();
        assert!((picture.record.start_time().unwrap() - 0.25).abs() < 0.001);
        assert_eq!(picture.record.in_point(), Some(0.0));
        assert!((picture.record.out_point().unwrap() - 1.5).abs() < 0.001);
    }
}

#[test]
fn supported_root_adjustment_mask_keeps_native_gate_and_source_consumption() {
    let output = export(input(false));
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        names(&native),
        [
            "Soft masked grade",
            "Ordinary guide-shaped paint",
            "Retained editable picture"
        ]
    );
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(OWNER)));
    let owner = &layers(&native)[0];
    assert!(owner.record.flags().adjustment_layer);
    let parade = root_runs(&owner.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Mask Parade")
        .unwrap()
        .1;
    let parade = unique_list(parade, *b"tdgp").unwrap();
    assert_eq!(
        runs(parade)
            .unwrap()
            .into_iter()
            .filter(|(name, _)| *name == "ADBE Mask Atom")
            .count(),
        1
    );
    let retained = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document;
    let LayerData::Adjustment(adjustment) =
        named(retained.composition().layers(), "Soft masked grade")
            .unwrap()
            .data()
    else {
        panic!("editable native Adjustment")
    };
    assert_eq!(adjustment.masks.len(), 1);
    assert_eq!(adjustment.masks[0].feather, [8.0, 8.0]);
    assert!(
        retained
            .composition()
            .dynamics()
            .entries()
            .iter()
            .any(|entry| {
                entry.target.as_property().is_some_and(|property| {
                    property.layer_id() == adjustment.id
                        && property.property_type() == PropType::Opacity
                })
            })
    );
}

#[test]
fn nongating_mask_none_does_not_omit_root_adjustment_effects() {
    let mut value = input(true);
    value["composition"]["layers"][1]["masks"][0]["mode"] = json!("none");
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert!(names(&native).contains(&"Soft masked grade"));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(OWNER)));
}

#[test]
fn failed_text_owner_does_not_restore_a_failed_adjustments_consumed_guide() {
    let mut value = input(true);
    let mut title = super::review_regressions::review_text_layer(
        &super::imported(),
        40_002,
        "Invalid Text owner",
        false,
        Some(GUIDE),
    );
    title["sourceText"]["fontSize"] = json!(f64::MAX);
    title["sourceText"]["leading"] = Value::Null;
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(title);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        names(&native),
        ["Ordinary guide-shaped paint", "Retained editable picture"]
    );
    assert!(output.omitted_layer_ids.contains(&LayerId::new(OWNER)));
    assert!(output.omitted_layer_ids.contains(&LayerId::new(40_002)));
}

#[test]
fn hidden_root_adjustment_does_not_consume_its_refused_guide() {
    let mut value = input(true);
    value["composition"]["layers"][1]["isHidden"] = json!(true);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        names(&native),
        [
            "Source-only mask guide",
            "Soft masked grade",
            "Ordinary guide-shaped paint",
            "Retained editable picture"
        ]
    );
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(OWNER)));
    assert!(!layers(&native)[1].record.flags().enabled);
}

#[test]
fn root_adjustment_fallback_does_not_remove_native_matte_dependencies() {
    for provider in [GUIDE, OWNER] {
        let mut value = input(true);
        value["composition"]["layers"][3]["trackMatte"] = json!({"layer":provider,"mode":"alpha"});
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        assert!(names(&native).contains(&"Source-only mask guide"));
        assert!(names(&native).contains(&"Soft masked grade"));
        assert!(names(&native).contains(&"Retained editable picture"));
        assert!(!output.omitted_layer_ids.contains(&LayerId::new(OWNER)));
        let provider_name = if provider == GUIDE {
            "Source-only mask guide"
        } else {
            "Soft masked grade"
        };
        assert!(
            !layers(&native)
                .iter()
                .find(|layer| layer.name.as_ref() == provider_name)
                .unwrap()
                .record
                .flags()
                .enabled
        );
    }
}
