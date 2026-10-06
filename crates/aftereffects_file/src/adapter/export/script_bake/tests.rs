mod encoded_document;
mod parallel;
mod playback;
mod sampled;
mod singular_ease;
mod singular_opacity;
mod source_text_retention;

use super::*;
use crate::{AfterEffects, AfterEffectsExportOptions, structure::read_project};
use fx_conv::{ConversionMode, Progress};
use serde_json::{Value, json};
use tesseract_file::TesseractFileBuilder;

// An edited pinned native fixture exercises preparation/publication only. These
// tests are supplementary CPU contracts, NOT an independent Adobe render oracle.
fn document(code: &str) -> EditableFxCompositionDocument {
    let source = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    let mut raw = crate::structure_document::to_structural_fx_document(&source, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut rect = raw["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    rect["id"] = json!(7);
    rect["parent"] = Value::Null;
    rect["name"] = json!("Baked scalar solid");
    rect["activeRange"] = json!({"start": 500, "duration": 100});
    raw["composition"]["layers"] = json!([rect]);
    raw["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "layer", "layerId": 7, "propertyType": "positionX"},
        "animator": {"type": "jsScript", "layerTimeJsCode": code}
    }]});
    EditableFxCompositionDocument::from_json_value(raw).unwrap()
}

fn modify(
    doc: &EditableFxCompositionDocument,
    edit: impl FnOnce(&mut Value),
) -> EditableFxCompositionDocument {
    let mut raw = doc.to_json_value().unwrap();
    edit(&mut raw);
    EditableFxCompositionDocument::from_json_value(raw).unwrap()
}

fn track<'a>(prepared: &'a Prepared<'_>, index: usize) -> &'a PropertyKeyframeTrack {
    prepared.document.composition().dynamics().entries()[index]
        .animator
        .keyframe_track()
        .unwrap()
}

#[test]
fn script_progress_counts_each_processed_track() {
    let original = modify(&document("return 20 + input.time.milliseconds;"), |raw| {
        let mut second = raw["composition"]["dynamics"]["entries"][0].clone();
        second["target"]["propertyType"] = json!("positionY");
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(second);
    });
    let events = std::sync::Mutex::new(Vec::new());
    let callback = |event| events.lock().unwrap().push(event);

    prepare_with_progress(
        &original,
        &AfterEffectsExportOptions::default(),
        Progress::new(&callback),
    )
    .unwrap();

    let events = events.into_inner().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[3].phase, "validate baked AE document");
    assert!(events[3].total.is_none());
    assert_eq!(events[0].phase, "bake FX scripts for AEP");
    assert_eq!(events[0].total, Some(2));
    assert_eq!(events[1].completed, Some(1));
    assert_eq!(events[2].completed, Some(2));
}

fn text_document(code: &str) -> EditableFxCompositionDocument {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/text_controls_native_panel/panel-text-style-hold.fx.json"
    ))
    .unwrap();
    modify(
        &EditableFxCompositionDocument::from_json_value(raw).unwrap(),
        |raw| {
            let entry = raw["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|entry| entry["target"]["propertyType"] == "textContent")
                .unwrap();
            entry["animator"] = json!({"type": "jsScript", "layerTimeJsCode": code});
        },
    )
}

#[test]
fn source_derived_typed_caption_script_becomes_editable_hold_text() {
    let original = text_document(
        "const events=[[0.0, 'I'], [0.1, 'IB'], [0.2, 'IBM']];let text='';for(const e of events){if(input.time.seconds+0.00001>=e[0])text=e[1];}return text;",
    );
    let baked = prepare(&original).unwrap();
    let entry = baked
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| matches!(&entry.target, PropertyTarget::LayerProperty(target) if target.property_type() == PropType::TextContent))
        .unwrap();
    let keys = entry.animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0].value(), &PropertyValue::String("I".into()));
    assert_eq!(keys[1].value(), &PropertyValue::String("IB".into()));
    assert_eq!(keys[2].value(), &PropertyValue::String("IBM".into()));
    assert!(
        keys.iter()
            .all(|key| key.easing() == PropertyKeyframeEasing::Hold)
    );
    assert!((100..=111).contains(&keys[1].layer_time().as_millis()));
    assert!((200..=211).contains(&keys[2].layer_time().as_millis()));
    assert!(
        baked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("sub-grid"))
    );
    let native = crate::export_document::to_aep(&baked.document).unwrap();
    assert!(!native.omitted_layer_ids.contains(&LayerId::new(6100)));
}

#[test]
fn source_text_script_returning_a_number_is_diagnosed_not_coerced() {
    let original = text_document("return 42;");
    let baked = prepare(&original).unwrap();
    let entry = baked
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| matches!(&entry.target, PropertyTarget::LayerProperty(target) if target.property_type() == PropType::TextContent))
        .unwrap();
    assert!(entry.animator.is_js_script());
    assert!(baked.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("must return a valid Unicode string")
    }));
}

#[test]
fn source_text_script_depending_on_call_history_is_not_baked() {
    let original = text_document("globalThis.n=(globalThis.n||0)+1; return String(globalThis.n);");
    let baked = prepare(&original).unwrap();
    assert!(
        baked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("history-dependent"))
    );
    assert!(baked.document.composition().dynamics().entries().iter().any(|entry| {
        matches!(&entry.target, PropertyTarget::LayerProperty(target) if target.property_type() == PropType::TextContent)
            && entry.animator.is_js_script()
    }));
}

#[test]
fn scalar_bake_is_local_deterministic_and_preserves_unrelated_data() {
    let original = modify(&document("return 20 + input.time.milliseconds;"), |raw| {
        raw["futureEnvelope"] = json!({"keep": [null, 1.5]});
        raw["composition"]["layers"][0]["futureLayer"] = json!({"keep": "exact"});
        raw["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({
            "target": {"kind": "layer", "layerId": 7, "propertyType": "positionY"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "authored-y", "layerTime": 0, "value": {"type": "float", "value": 25.0}, "easing": {"type": "hold"}}
            ]}
        }));
    });
    let before = original.to_json_value().unwrap();
    let baked = prepare(&original).unwrap();
    let after = baked.document.to_json_value().unwrap();
    let keys = track(&baked, 0).keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].layer_time().as_millis(), 0);
    assert_eq!(keys[1].layer_time().as_millis(), 100);
    assert_eq!(keys[0].value(), &PropertyValue::Float(20.0));
    assert_eq!(keys[1].value(), &PropertyValue::Float(120.0));
    assert_eq!(
        after["composition"]["layers"],
        before["composition"]["layers"]
    );
    assert_eq!(after["futureEnvelope"], before["futureEnvelope"]);
    assert_eq!(
        after["composition"]["dynamics"]["entries"][1],
        before["composition"]["dynamics"]["entries"][1]
    );
    assert_eq!(before, original.to_json_value().unwrap());
    assert_eq!(
        after,
        prepare(&original)
            .unwrap()
            .document
            .to_json_value()
            .unwrap()
    );
}

#[test]
fn path_script_bake_quantizes_steps_and_variable_contours_to_owner_grid() {
    let original = modify(
        &document(
            "var t=Math.floor(input.time.milliseconds/17); var c=[]; for(var i=0;i<t;i++){c.push({type:'moveTo',x:i,y:t},{type:'lineTo',x:i+1,y:t},{type:'lineTo',x:i,y:t+1},{type:'close'});}return {commands:c};",
        ),
        |raw| {
            raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                json!("shapePath");
        },
    );
    let baked = prepare(&original).unwrap();
    let keys = track(&baked, 0).keyframes();
    assert_eq!(
        keys.iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        // Each 17ms topology change moves to the next sample on the 96Hz grid.
        // This is a sampled approximation, not millisecond-exact step timing.
        [0, 21, 42, 52, 73, 94]
    );
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(key.easing(), PropertyKeyframeEasing::Hold);
        let PropertyValue::Path(path) = key.value() else {
            panic!("Path keys required")
        };
        assert_eq!(path.commands.len(), index * 4);
    }
    assert!(
        original.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
}

#[test]
fn path_script_bake_matches_pinned_disappearance_controls() {
    let original = EditableFxCompositionDocument::from_json_value(
        serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/path-keys-proof/hold-disappearance-v1.fx.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let baked = prepare(&original).unwrap();
    let keys = track(&baked, 0).keyframes();
    assert_eq!(
        keys.iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        [0, 500, 1000, 1500]
    );
    let counts: Vec<_> = keys
        .iter()
        .map(|key| {
            assert_eq!(key.easing(), PropertyKeyframeEasing::Hold);
            let PropertyValue::Path(path) = key.value() else {
                panic!("editable Path required")
            };
            path.commands.len()
        })
        .collect();
    assert_eq!(counts, [4, 0, 4, 1]);
    let output = crate::export_document::to_aep(&baked.document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|entry| !entry.message.contains("omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let imported = crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    assert!(
        imported
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .any(|entry| {
                entry.animator.keyframe_track().is_some_and(|track| {
                    track.keyframes().iter().map(|key| key.value()).eq([
                        &PropertyValue::Float(100.0),
                        &PropertyValue::Float(0.0),
                        &PropertyValue::Float(100.0),
                        &PropertyValue::Float(0.0),
                    ])
                })
            }),
        "empty stroked geometry must carry editable visibility keys"
    );
}

#[test]
fn path_script_bake_rejects_history_dependent_geometry() {
    let original = modify(
        &document(
            "globalThis.n=(globalThis.n||0)+1; return {commands:[{type:'moveTo',x:globalThis.n,y:0},{type:'lineTo',x:20,y:20}]};",
        ),
        |raw| {
            raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                json!("shapePath");
        },
    );
    let baked = prepare(&original).unwrap();
    assert!(
        baked.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert!(
        baked
            .diagnostics
            .iter()
            .any(|entry| entry.message.contains("history-dependent"))
    );
}

#[test]
fn path_script_bake_reduces_linear_coordinates_together() {
    let original = modify(
        &document(
            "var t=input.time.milliseconds; return {commands:[{type:'moveTo',x:t,y:0},{type:'lineTo',x:t+10,y:20}]};",
        ),
        |raw| {
            raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                json!("shapePath");
        },
    );
    let baked = prepare(&original).unwrap();
    let keys = track(&baked, 0).keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[1].layer_time().as_millis(), 100);
    assert_eq!(keys[1].easing(), PropertyKeyframeEasing::Linear);
}

#[test]
fn no_script_document_is_borrowed_and_unchanged() {
    let original = modify(&document("return 1;"), |raw| {
        raw["composition"]["dynamics"]["entries"] = json!([])
    });
    let prepared = prepare(&original).unwrap();
    assert!(matches!(prepared.document, Cow::Borrowed(_)));
    assert!(prepared.diagnostics.is_empty());
}

#[test]
fn effect_script_uses_its_owner_local_clock() {
    let original = modify(&document("return input.time.milliseconds;"), |raw| {
        raw["composition"]["layers"][0]["effects"] = json!([{
            "id": 101, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 0.0, "dimensions": "both", "repeatEdgePixels": false}
        }]);
        raw["composition"]["dynamics"]["entries"][0]["target"] =
            json!({"kind": "effectProperty", "effectId": 101, "paramName": "blurriness"});
    });
    let prepared = prepare(&original).unwrap();
    let keys = track(&prepared, 0).keyframes();
    assert_eq!(keys.last().unwrap().layer_time().as_millis(), 100);
    assert_eq!(keys.last().unwrap().value(), &PropertyValue::Float(100.0));
}

#[test]
fn corner_pin_invisible_extremes_do_not_erase_visible_point_samples() {
    let original = modify(
        &document(
            "const t = input.time.milliseconds; return t < 92 ? -1000000 + t : -10.8 / (1 + (t - 100) / 9.6);",
        ),
        |raw| {
            raw["composition"]["layers"][0]["activeRange"] = json!({"start": 0, "duration": 1500});
            raw["composition"]["layers"][0]["effects"] = json!([{
                "id": 101, "enabled": true, "effect": {
                    "type": "cornerPin", "upperLeftX": 0.0, "upperLeftY": 0.0,
                    "upperRightX": 1.0, "upperRightY": 0.0,
                    "lowerLeftX": 0.0, "lowerLeftY": 1.0,
                    "lowerRightX": 1.0, "lowerRightY": 1.0
                }
            }]);
            let entry = raw["composition"]["dynamics"]["entries"][0].clone();
            raw["composition"]["dynamics"]["entries"] = json!(
                [
                    "upperLeftX",
                    "upperLeftY",
                    "upperRightX",
                    "upperRightY",
                    "lowerLeftX",
                    "lowerLeftY",
                    "lowerRightX",
                    "lowerRightY",
                ]
                .map(|param| {
                    let mut entry = entry.clone();
                    entry["target"] =
                        json!({"kind": "effectProperty", "effectId": 101, "paramName": param});
                    entry
                })
            );
        },
    );
    let options = AfterEffectsExportOptions { fps: 30.0 };
    let prepared = prepare_with_progress(&original, &options, Progress::default()).unwrap();
    for index in 0..8 {
        let keys = track(&prepared, index).keyframes();
        assert_eq!(keys[0].value(), &PropertyValue::Float(-1000000.0));
        let visible = keys
            .iter()
            .find(|key| key.layer_time().as_millis() == 100)
            .expect("the first visible point cannot be fitted away by invisible extremes");
        assert_eq!(visible.value(), &PropertyValue::Float(-10.8));
    }
    assert!(
        original.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    let edited = modify(&original, |raw| {
        for entry in raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
        {
            entry["animator"]["layerTimeJsCode"] = json!(
                "const t = input.time.milliseconds; return t < 92 ? -2000000 + t : -21.6 / (1 + (t - 100) / 9.6);"
            );
        }
    });
    let prepared_edit = prepare_with_progress(&edited, &options, Progress::default()).unwrap();
    for index in 0..8 {
        let keys = track(&prepared_edit, index).keyframes();
        assert_eq!(keys[0].value(), &PropertyValue::Float(-2000000.0));
        assert_eq!(
            keys.iter()
                .find(|key| key.layer_time().as_millis() == 100)
                .unwrap()
                .value(),
            &PropertyValue::Float(-21.6)
        );
    }
}

#[test]
fn selected_mask_script_keeps_unsupported_diagnostic_and_bakes_sibling() {
    let original = modify(&document("return 2;"), |raw| {
        raw["composition"]["layers"][0]["masks"] = json!([{"id": 901, "mode": "add", "layer": 7}]);
        raw["composition"]["dynamics"]["entries"][0]["target"] =
            json!({"kind": "fxItemProperty", "itemId": 901, "propertyName": "opacity"});
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "target": {"kind": "layer", "layerId": 7, "propertyType": "positionX"},
                "animator": {"type": "jsScript", "layerTimeJsCode": "return 4;"}
            }));
    });
    let prepared = prepare_layers(&original, original.composition().layers(), true).unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert_eq!(
        track(&prepared, 1).keyframes()[0].value(),
        &PropertyValue::Float(4.0)
    );
    assert!(prepared.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(7))
            && diagnostic
                .message
                .contains("fxItem 901 opacity was not baked")
            && diagnostic
                .message
                .contains("FX-item scripts (including mask-item properties) are not supported")
    }));
}

#[test]
fn selected_text_item_scripts_are_diagnosed_only_for_selected_root() {
    let original =
        modify(&document("return 2;"), |raw| {
            let text = json!({
                "type": "Text", "id": 8, "name": "Selected text", "parent": null,
                "activeRange": {"start": 500, "duration": 100},
                "transform": raw["composition"]["layers"][0]["transform"],
                "sourceText": {"text": "Selected", "fontFamily": "Inter-Regular", "fontSize": 42.0,
                    "fillColor": [1.0, 1.0, 1.0, 1.0],
                    "fontVariations": {"id": 806, "axes": {"wght": 700.0}}},
                "animators": [{"id": 801, "selectors": [{"id": 802}],
                    "wigglySelectors": [{"id": 803}]}],
                "pathOptions": {"id": 804, "pathLayer": 7}, "anchorOptions": {"id": 805}
            });
            raw["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .insert(0, text);
            raw["composition"]["dynamics"]["entries"] = json!([]);
            for id in [801, 802, 803, 804, 805, 806] {
                raw["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({
                "target": {"kind": "fxItemProperty", "itemId": id, "propertyName": "opacity"},
                "animator": {"type": "jsScript", "layerTimeJsCode": "return 2;"}
            }));
            }
            raw["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "target": {"kind": "layer", "layerId": 7, "propertyType": "positionX"},
                    "animator": {"type": "jsScript", "layerTimeJsCode": "return 4;"}
                }));
        });
    let prepared = prepare_layers(&original, &original.composition().layers()[..1], true).unwrap();
    assert!(matches!(prepared.document, Cow::Borrowed(_)));
    assert_eq!(prepared.diagnostics.len(), 6);
    for id in [801, 802, 803, 804, 805, 806] {
        assert!(prepared.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(8))
                && diagnostic
                    .message
                    .contains(&format!("fxItem {id} opacity was not baked"))
                && diagnostic
                    .message
                    .contains("FX-item scripts (including mask-item properties) are not supported")
        }));
    }
    assert!(
        prepared.document.composition().dynamics().entries()[6]
            .animator
            .is_js_script()
    );
}

#[test]
fn failures_remain_scripts_with_context_instead_of_static_success() {
    for source in [
        "return NaN;",
        "return input.time.milliseconds < 50 ? -1e308 : 1e308;",
        "throw 'bad';",
        "throw new SyntaxError('runtime');",
        "return [1,2];",
        "globalThis.n = (globalThis.n || 0) + 1; return globalThis.n;",
    ] {
        let original = document(source);
        let prepared = prepare(&original).unwrap();
        assert_eq!(
            prepared.document.to_json_value().unwrap(),
            original.to_json_value().unwrap()
        );
        assert!(
            prepared
                .diagnostics
                .iter()
                .any(|warning| warning.layer_id == Some(LayerId::new(7))
                    && warning.message.contains("was not baked")),
            "{source}"
        );
    }
}

#[test]
fn parse_invalid_transform_uses_authored_static_for_both_video_group_owners() {
    // Minimal equivalent of the original malformed `1700--0.066` expression.
    // The native archive and its scripts are never repaired by preparation.
    for (root, owner) in [(4000, 4526), (5000, 5535)] {
        let original = modify(&document("return 1700--0.066;"), |raw| {
            let child = json!({
                "type":"Video", "id":owner + 1, "parent":owner, "name":"Actual footage",
                "playback":{"type":"windowed","inputRange":{"start":0,"duration":100},
                    "mapping":{"type":"linear","input":{"start":0,"duration":100},
                        "output":{"start":0,"duration":100}},"inputOffsetMs":0},
                "sourceRange":{"start":0,"duration":100}, "sourceIntrinsicDuration":100,
                "source":{"assetId":"test-video","fit":"contain"},
                "transform":{"anchorPoint":[0,0],"position":[0,0],
                    "scale":[100,100],"rotation":0,"opacity":100}
            });
            let child = json!({
                "type":"Group", "id":owner, "name":"Source footage group", "parent":root,
                "playback":{"type":"windowed","inputRange":{"start":0,"duration":100},
                    "mapping":{"type":"linear","input":{"start":0,"duration":100},
                        "output":{"start":0,"duration":100}},"inputOffsetMs":0},
                "transform":{"anchorPoint":[600,1700],"position":[600,1700],
                    "scale":[100,100],"rotation":0,"opacity":100},
                "layers":[child]
            });
            raw["composition"]["layers"] = json!([{
                "type": "Group", "id": root, "name": "Source root",
                "playback":{"type":"windowed","inputRange":{"start":0,"duration":100},
                    "mapping":{"type":"linear","input":{"start":0,"duration":100},
                        "output":{"start":0,"duration":100}},"inputOffsetMs":0},
                "transform": {"anchorPoint": [0, 0], "position": [0, 0],
                    "scale": [100, 100], "rotation": 0, "opacity": 100},
                "layers": [child]
            }]);
            raw["composition"]["dynamics"]["entries"] = json!([
                {"target": {"kind": "layer", "layerId": owner, "propertyType": "positionY"},
                 "animator": {"type": "jsScript", "layerTimeJsCode": "return 1700--0.066;"}},
                {"target": {"kind": "layer", "layerId": owner, "propertyType": "positionX"},
                 "animator": {"type": "jsScript", "layerTimeJsCode": "return input.time.milliseconds;"}}
            ]);
        });
        let before = original.to_json_value().unwrap();
        let prepared = prepare(&original).unwrap();
        let entries = prepared.document.composition().dynamics().entries();
        assert!(
            matches!(entries[0].animator.data(), AnimatorData::Constant { value: PropertyValue::Float(value) } if *value == 1700.0)
        );
        assert!(entries[1].animator.keyframe_track().is_some());
        assert_eq!(original.to_json_value().unwrap(), before);
        assert_eq!(
            prepared.document.to_json_value().unwrap()["composition"]["layers"],
            before["composition"]["layers"]
        );
        assert!(prepared.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(owner))
                && diagnostic.message.contains(&format!("Root {root}"))
                && diagnostic.message.contains("positionY")
                && diagnostic.message.contains("parse failure")
                && diagnostic.message.contains("1700")
                && diagnostic.message.contains("motion is lost")
        }));
    }
}

fn plain_footage_clock() -> Value {
    json!({"type":"windowed","inputRange":{"start":0,"duration":100},
        "mapping":{"type":"linear","input":{"start":0,"duration":100},
            "output":{"start":0,"duration":100}},"inputOffsetMs":0})
}

#[test]
fn parse_invalid_transform_preserves_authored_base_for_mixed_groups() {
    let solid = document("return 1700--0.066;").to_json_value().unwrap()["composition"]["layers"]
        [0]
    .clone();
    let text = text_document("return 'text';").to_json_value().unwrap()["composition"]["layers"][0]
        .clone();
    let video = media_on_clock("Video", media_clock(0, 0))
        .to_json_value()
        .unwrap()["composition"]["layers"][0]
        .clone();
    let audio = media_on_clock("Audio", media_clock(0, 0))
        .to_json_value()
        .unwrap()["composition"]["layers"][0]
        .clone();
    fn identify(children: &mut [Value], parent: u64, next: &mut u64) {
        for child in children {
            let id = *next;
            *next += 1;
            child["id"] = json!(id);
            child["parent"] = json!(parent);
            if let Some(layers) = child["layers"].as_array_mut() {
                identify(layers, id, next);
            }
        }
    }
    for mut children in [
        vec![solid.clone()],
        vec![text.clone()],
        vec![],
        vec![audio],
        vec![video.clone(), text.clone()],
        vec![video.clone(), solid],
        vec![
            json!({"type":"Group","id":10,"name":"Nested mixed", "playback":plain_footage_clock(), "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
            "layers":[video, text]}),
        ],
    ] {
        let mut next = 8;
        identify(&mut children, 7, &mut next);
        let original = modify(&document("return 1700--0.066;"), |raw| {
            raw["composition"]["layers"] = json!([{
                "type":"Group", "id":7, "name":"Mixed descendant owner", "playback":plain_footage_clock(),
                "transform":{"anchorPoint":[0,0],"position":[600,1700],
                    "scale":[100,100],"rotation":0,"opacity":100},
                "layers":children
            }]);
            raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                json!("positionY");
        });
        let before = original.to_json_value().unwrap();
        let prepared = prepare(&original).unwrap();
        let empty = children.is_empty();
        let animator = prepared.document.composition().dynamics().entries()[0]
            .animator
            .data();
        if empty {
            assert!(matches!(animator, AnimatorData::JsScript { .. }));
        } else {
            assert!(
                matches!(animator, AnimatorData::Constant { value: PropertyValue::Float(value) } if *value == 1700.0)
            );
            assert!(
                prepared
                    .diagnostics
                    .iter()
                    .any(|d| d.message.contains("motion is lost"))
            );
        }
        assert_eq!(original.to_json_value().unwrap(), before);
        assert_eq!(
            prepared.document.to_json_value().unwrap()["composition"]["layers"],
            before["composition"]["layers"]
        );
    }
}

#[test]
fn parse_invalid_transform_checks_owner_not_descendant_effect_profiles() {
    for level in [0, 1, 2] {
        for field in [
            "effects",
            "motionBlur",
            "blendMode",
            "masks",
            "trackMatte",
            "fills",
        ] {
            // Video has no Group paint; the two Group levels exercise that guard.
            if level == 2 && field == "fills" {
                continue;
            }
            let original = modify(&document("return 1700--0.066;"), |raw| {
                let mut video = media_on_clock("Video", plain_footage_clock())
                    .to_json_value()
                    .unwrap()["composition"]["layers"][0]
                    .clone();
                video["id"] = json!(9);
                video["parent"] = json!(8);
                raw["composition"]["layers"] = json!([{
                    "type":"Group","id":7,"name":"Footage owner", "playback":plain_footage_clock(),
                    "transform":{"anchorPoint":[0,0],"position":[600,1700],"scale":[100,100],"rotation":0,"opacity":100},
                    "layers":[{"type":"Group","id":8,"name":"Structural wrapper", "parent":7,
                        "playback":plain_footage_clock(), "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100}, "layers":[video]}]
                }]);
                let mut node = &mut raw["composition"]["layers"][0];
                for _ in 0..level {
                    node = &mut node["layers"][0];
                }
                node[field] = match field {
                    "effects" => json!([{"id":101,"enabled":true,"effect":{"type":"gaussianBlur",
                        "blurriness":1.0,"dimensions":"both","repeatEdgePixels":false}}]),
                    "motionBlur" => json!(true),
                    "masks" => json!([{"id":901,"mode":"add","layer":9}]),
                    "trackMatte" => json!({"mode":"alpha","layer":9}),
                    "fills" => json!([{"blendMode":"normal","fillRule":"nonZeroWinding",
                        "opacity":1.0,"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]}}]),
                    _ => json!("multiply"),
                };
                raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                    json!("positionY");
            });
            let prepared = prepare(&original).unwrap();
            let animator = prepared.document.composition().dynamics().entries()[0]
                .animator
                .data();
            if level == 0 {
                assert!(matches!(animator, AnimatorData::JsScript { .. }));
                assert!(
                    !prepared
                        .diagnostics
                        .iter()
                        .any(|d| d.message.contains("motion is lost"))
                );
            } else {
                assert!(
                    matches!(animator, AnimatorData::Constant { value: PropertyValue::Float(value) } if *value == 1700.0)
                );
                assert!(
                    prepared
                        .diagnostics
                        .iter()
                        .any(|d| d.message.contains("motion is lost"))
                );
            }
        }
    }
}

#[test]
fn subtraction_of_negative_number_is_normally_baked() {
    let original = document("return 1700 - -0.066;");
    let prepared = prepare(&original).unwrap();
    assert!(
        matches!(track(&prepared, 0).keyframes()[0].value(), PropertyValue::Float(value) if (*value - 1700.066).abs() < 1e-9)
    );
}

#[test]
fn malformed_dependency_or_reference_and_runtime_syntax_errors_remain_scripts() {
    for source in ["return 1700--0.066;", "throw new SyntaxError('runtime');"] {
        let original = modify(&document(source), |raw| {
            let mut child = raw["composition"]["layers"][0].clone();
            child["id"] = json!(8);
            child["parent"] = json!(7);
            raw["composition"]["layers"] = json!([{
                "type":"Group","id":7,"name":"Untrusted Group control",
                "playback":{"type":"windowed","inputRange":{"start":500,"duration":100},
                    "mapping":{"type":"linear","input":{"start":500,"duration":100},
                        "output":{"start":0,"duration":100}},"inputOffsetMs":0},
                "transform":{"anchorPoint":[0,0],"position":[600,1700],
                    "scale":[100,100],"rotation":0,"opacity":100},
                "layers":[child]
            }]);
        });
        if source.starts_with("throw") {
            let prepared = prepare(&original).unwrap();
            assert_eq!(
                prepared.document.to_json_value().unwrap(),
                original.to_json_value().unwrap()
            );
        } else {
            for field in ["dependencies", "layerRefs"] {
                let dependent = modify(&original, |raw| {
                    raw["composition"]["dynamics"]["entries"][0][field] =
                        if field == "dependencies" {
                            json!([{"kind": "layer", "layerId": 7, "propertyType": "positionY"}])
                        } else {
                            raw["composition"]["layers"].as_array_mut().unwrap().push(json!({
                            "type":"Image","id":9,"name":"Reference-only asset","parent":null,
                            "activeRange":{"start":0,"duration":2000},
                            "transform":{"anchorPoint":[0,0],"position":[0,0],
                                "scale":[100,100],"rotation":0,"opacity":100},
                            "source":{"assetId":"parse-reference","fit":"contain"}
                        }));
                            json!({"source": {"layerId": 9}})
                        };
                    raw["composition"]["dynamics"]["entries"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({
                            "target":{"kind":"layer","layerId":7,"propertyType":"positionY"},
                            "animator":{"type":"constant","value":{"type":"float","value":1700.0}}
                        }));
                });
                let prepared = prepare(&dependent).unwrap();
                assert_eq!(
                    prepared.document.to_json_value().unwrap(),
                    dependent.to_json_value().unwrap()
                );
            }
        }
    }
}

#[test]
fn a_source_at_the_former_bound_parses_on_the_evaluation_thread() {
    // Retain the previously exercised recursive-parser case. Stack sizing now
    // grows past the former source bound instead of clamping every longer source.
    assert_eq!(stack_bytes(32 * 1024).unwrap(), 1 << 30);
    assert_eq!(stack_bytes(32 * 1024 + 1).unwrap(), (1 << 30) + (32 << 10));
    assert!(stack_bytes(usize::MAX).is_err());
    let unclosed = format!("return {}1;", "(".repeat(32 * 1024 - "return 1;".len()));
    let nested = format!("return {}50{};", "(".repeat(1000), ")".repeat(1000));
    let original = modify(&document(&unclosed), |raw| {
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "target": {"kind": "layer", "layerId": 7, "propertyType": "positionY"},
                "animator": {"type": "jsScript", "layerTimeJsCode": nested}
            }));
    });
    for selected in [false, true] {
        let prepared =
            prepare_layers(&original, original.composition().layers(), selected).unwrap();
        assert!(
            prepared.document.composition().dynamics().entries()[0]
                .animator
                .is_js_script()
        );
        assert_eq!(
            track(&prepared, 1).keyframes()[0].value(),
            &PropertyValue::Float(50.0)
        );
        assert!(
            prepared.diagnostics.iter().any(|warning| {
                warning.layer_id == Some(LayerId::new(7))
                    && warning
                        .message
                        .starts_with("JS animator layer 7 positionX was not baked: SyntaxError")
            }),
            "selected {selected}: {:?}",
            prepared.diagnostics
        );
    }
}

#[test]
fn selected_and_ordinary_scopes_bake_sources_above_the_former_byte_quota() {
    let unclosed = format!("return +; //{}", " ".repeat(32 * 1024));
    let long = format!("return 2; //{}", " ".repeat(32 * 1024));
    let original = modify(&document(&unclosed), |raw| {
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "target": {"kind": "layer", "layerId": 7, "propertyType": "positionY"},
                "animator": {"type": "jsScript", "layerTimeJsCode": long}
            }));
    });
    let kept = |prepared: &Prepared<'_>, property: &str, reason: &str| {
        let message = format!("JS animator layer 7 {property} was not baked: {reason}");
        prepared.diagnostics.iter().any(|warning| {
            warning.layer_id == Some(LayerId::new(7)) && warning.message.starts_with(&message)
        })
    };
    let bound = "script source exceeds";

    let ordinary = prepare(&original).unwrap();
    assert!(
        ordinary.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert!(
        kept(&ordinary, "positionX", "SyntaxError"),
        "{:?}",
        ordinary.diagnostics
    );
    assert_eq!(
        track(&ordinary, 1).keyframes()[0].value(),
        &PropertyValue::Float(2.0)
    );
    assert!(
        !ordinary
            .diagnostics
            .iter()
            .any(|warning| warning.message.contains(bound)),
        "{:?}",
        ordinary.diagnostics
    );

    let scope = prepare_layers(&original, original.composition().layers(), true).unwrap();
    assert!(kept(&scope, "positionX", "SyntaxError"));
    assert_eq!(
        track(&scope, 1).keyframes()[0].value(),
        &PropertyValue::Float(2.0)
    );
    assert!(
        !scope
            .diagnostics
            .iter()
            .any(|warning| warning.message.contains(bound))
    );
}

#[test]
fn selected_scope_bakes_its_script_and_preserves_an_unselected_large_source() {
    let large = format!("return 3; //{}", " ".repeat(128 * 1024));
    let original = modify(&document("return 2;"), |raw| {
        let mut other = raw["composition"]["layers"][0].clone();
        other["id"] = json!(8);
        raw["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(other);
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .insert(
                0,
                json!({
                    "target": {"kind": "layer", "layerId": 8, "propertyType": "positionX"},
                    "animator": {"type": "jsScript", "layerTimeJsCode": large}
                }),
            );
    });
    let prepared = prepare_layers(&original, &original.composition().layers()[..1], true).unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert_eq!(
        track(&prepared, 1).keyframes()[0].value(),
        &PropertyValue::Float(2.0)
    );
    assert_eq!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .known_value(),
        original.composition().dynamics().entries()[0]
            .animator
            .known_value()
    );
}

#[test]
fn legacy_clocks_and_playback_are_not_guessed() {
    for original in [
        modify(&document("return 1;"), |raw| {
            raw["composition"]["dynamics"]["entries"][0]["animator"] =
                json!({"type": "jsScript", "code": "return input.time.seconds;"});
        }),
        modify(&document("return 1;"), |raw| {
            // Unknown playback on a non-media layer is retained losslessly; do
            // not interpret it or silently use the active duration as its clock.
            raw["composition"]["layers"][0]["playback"] = json!({"rate": 2.0});
        }),
    ] {
        let prepared = prepare(&original).unwrap();
        assert!(
            prepared.document.composition().dynamics().entries()[0]
                .animator
                .is_js_script()
        );
        assert_eq!(prepared.diagnostics.len(), 1);
    }
}

fn media_on_clock(kind: &str, playback: Value) -> EditableFxCompositionDocument {
    modify(
        &document("return 0.5 + input.time.milliseconds / 1000;"),
        |raw| {
            let transform = raw["composition"]["layers"][0]["transform"].clone();
            let mut media = json!({
                "type":kind, "id":7, "name":"Canonical media", "playback":playback,
                "sourceRange":{"start":2000,"duration":200}, "sourceIntrinsicDuration":5000,
                "source":{"assetId":"test-media"}
            });
            let property = if kind == "Audio" {
                "volume"
            } else {
                assert_eq!(kind, "Video");
                media["transform"] = transform;
                media["source"]["fit"] = json!("contain");
                media["source"]["sourceRect"] = json!({"x":0,"y":0,"width":32,"height":32});
                "opacity"
            };
            raw["composition"]["layers"] = json!([media]);
            raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                json!(property);
        },
    )
}

fn media_clock(offset: i64, mismatch: i64) -> Value {
    json!({
        "type":"windowed", "inputRange":{"start":1000,"duration":100},
        "mapping":{"type":"linear", "input":{"start":1000+offset+mismatch,"duration":100},
            "output":{"start":2000,"duration":200}}, "inputOffsetMs":offset
    })
}

#[test]
fn canonical_media_input_clocks_bake_signed_anchor_cancellation_independently_of_source_rate() {
    for kind in ["Video", "Audio"] {
        for offset in [-500, 0, 500] {
            let original = media_on_clock(kind, media_clock(offset, 0));
            let before = original.to_json_value().unwrap();
            let prepared = prepare(&original).unwrap();
            let keys = track(&prepared, 0).keyframes();
            assert_eq!(keys.len(), 2, "{kind} {offset}");
            assert_eq!(keys[0].layer_time().as_millis(), 0);
            assert_eq!(keys[1].layer_time().as_millis(), 100);
            assert_eq!(keys[0].value(), &PropertyValue::Float(0.5));
            assert_eq!(keys[1].value(), &PropertyValue::Float(0.6));
            assert_eq!(
                prepared.document.to_json_value().unwrap()["composition"]["layers"],
                before["composition"]["layers"]
            );
            assert_eq!(original.to_json_value().unwrap(), before);
        }
    }
}

#[test]
fn canonical_media_shifted_anchors_and_time_remaps_bake_in_owner_domain() {
    let remap = json!({
        "type":"windowed", "inputRange":{"start":1000,"duration":100},
        "mapping":{"type":"timeRemap", "property":{"keyframes":[
            {"id":"a","time":1000,"value":2000,"easing":{"type":"linear"}},
            {"id":"b","time":1100,"value":2200,"easing":{"type":"linear"}}
        ],"before":"inactive","after":"inactive"}}, "inputOffsetMs":0
    });
    for kind in ["Video", "Audio"] {
        for playback in [media_clock(-500, 1), media_clock(500, 1), remap.clone()] {
            let original = media_on_clock(kind, playback);
            let prepared = prepare(&original).unwrap();
            let keys = track(&prepared, 0).keyframes();
            let start = if original.composition().layers()[0].wire_value()["playback"]["mapping"]["type"]
                == "timeRemap"
            {
                2000
            } else {
                0
            };
            assert_eq!(keys[0].layer_time().as_millis(), start);
            assert_eq!(
                keys[0].value(),
                &PropertyValue::Float(0.5 + start as f64 / 1000.0)
            );
        }
    }
}

#[test]
fn delayed_affine_group_clocks_bake_owner_domains_and_child_windows() {
    for (source_start, source_duration) in [(0, 100), (1, 100), (0, 200)] {
        let original = modify(
            &document("return 0.5 + input.time.milliseconds / 1000;"),
            |raw| {
                let mut child = raw["composition"]["layers"][0].clone();
                child["activeRange"] = json!({"start":0,"duration":100});
                child["parent"] = json!(8);
                let transform = child["transform"].clone();
                raw["composition"]["layers"] = json!([{
                    "type":"Group", "id":8, "name":"Delayed owner",
                    "playback":{"type":"windowed", "inputRange":{"start":1000,"duration":100},
                        "mapping":{"type":"linear", "input":{"start":1000,"duration":100},
                            "output":{"start":source_start,"duration":source_duration}}, "inputOffsetMs":0},
                    "transform":transform, "layers":[child]
                }]);
                let mut owner = raw["composition"]["dynamics"]["entries"][0].clone();
                owner["target"]["layerId"] = json!(8);
                owner["target"]["propertyType"] = json!("opacity");
                raw["composition"]["dynamics"]["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(owner);
            },
        );
        let prepared = prepare(&original).unwrap();
        for (index, entry) in prepared
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .enumerate()
        {
            assert!(!entry.animator.is_js_script());
            {
                let keys = entry.animator.keyframe_track().unwrap().keyframes();
                assert_eq!(
                    (
                        keys[0].layer_time().as_millis(),
                        keys[1].layer_time().as_millis()
                    ),
                    if index == 0 {
                        (0, 100)
                    } else {
                        (source_start, source_start + source_duration)
                    }
                );
                let start = if index == 0 { 0 } else { source_start };
                let end = if index == 0 {
                    100
                } else {
                    source_start + source_duration
                };
                assert_eq!(
                    keys[0].value(),
                    &PropertyValue::Float(0.5 + start as f64 / 1000.0)
                );
                assert_eq!(
                    keys[1].value(),
                    &PropertyValue::Float(0.5 + end as f64 / 1000.0)
                );
            }
        }
    }
}

#[test]
fn accumulated_evaluations_do_not_reject_the_next_script() {
    let mut budget = Budget {
        calls: 20_000_000,
        keys: 250_000,
    };
    let mut runtime = ScriptRuntime::new().unwrap();
    assert_eq!(
        evaluate(&mut runtime, "return 1;", 0, 0, &mut budget).unwrap(),
        1.0
    );
    assert_eq!(budget.calls, 20_000_001);
    let original = document("return 1;");
    let entry = &original.composition().dynamics().entries()[0];
    let owner = Owner {
        id: original.composition().layers()[0].id(),
        duration_ms: 2,
        start_ms: 0,
        clock_id: 0,
        unsupported_clock: false,
    };
    // Production derives the consumer's owner through the same lookup, and the
    // dependency-domain program resolves every member through it.
    let animator = bake_entry(
        entry,
        Some(owner),
        original.composition().dynamics().entries(),
        &|_| Some(owner),
        &mut BTreeSet::new(),
        &mut budget,
        Sampling::new(&AfterEffectsExportOptions::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(animator.keyframe_track().unwrap().keyframes().len(), 1);
    assert_eq!(budget.keys, 250_001);
}

#[test]
fn script_window_longer_than_one_minute_retains_its_constant_value() {
    let original = modify(&document("return 1;"), |raw| {
        raw["composition"]["layers"][0]["activeRange"]["duration"] = json!(60_001);
    });
    let prepared = prepare(&original).unwrap();
    assert_eq!(track(&prepared, 0).keyframes().len(), 1);
    assert_eq!(
        track(&prepared, 0).keyframes()[0].value(),
        &PropertyValue::Float(1.0)
    );
}

#[test]
fn evaluations_count_work_and_reject_counter_overflow() {
    let mut budget = Budget::default();
    let mut runtime = ScriptRuntime::new().unwrap();
    let code = "return input.time.milliseconds;";
    assert_eq!(
        evaluate(&mut runtime, code, 0, 2, &mut budget).unwrap(),
        2.0
    );
    assert_eq!(budget.calls, 1);
    budget.calls = usize::MAX;
    assert!(matches!(
        evaluate(&mut runtime, code, 0, 3, &mut budget),
        Err(BakeError::Budget("evaluation counter overflow"))
    ));
}

#[test]
fn fifteen_second_stepped_opacity_has_compact_validated_keys() {
    let original = modify(
        &document(
            "return Math.floor((Math.round(input.time.seconds * 30) % 30) / 10) === 0 ? 1 : 0;",
        ),
        |raw| {
            raw["composition"]["layers"][0]["activeRange"] =
                json!({"start": 0, "duration": 15_000});
            raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] =
                json!("opacity");
        },
    );
    let prepared = prepare(&original).unwrap();
    let keys = track(&prepared, 0).keyframes();
    assert!(keys.len() > 20 && keys.len() < 100);
    assert!(
        keys.iter()
            .all(|key| key.easing() == PropertyKeyframeEasing::Hold)
    );
}

#[test]
fn direct_input_matches_json_builder_and_is_fresh_at_extreme_times() {
    let probe = r#"
        const keys = o => Object.keys(o).join(',');
        const descriptor = (o, k) => {
          const d = Object.getOwnPropertyDescriptor(o, k);
          return [d.writable, d.enumerable, d.configurable].join(',');
        };
        const result = [keys(input), keys(input.time), keys(input.refs),
          keys(input.__jerboaAssetMetadata), descriptor(input, 'time'),
          descriptor(input.time, 'milliseconds'), descriptor(input, 'refs'),
          Object.getPrototypeOf(input) === Object.prototype,
          Object.getPrototypeOf(input.time) === Object.prototype,
          Object.getPrototypeOf(input.refs) === Object.prototype,
          Object.getPrototypeOf(input.__jerboaAssetMetadata) === Object.prototype,
          Object.getPrototypeOf(input.deps) === Array.prototype,
          input.time.seconds, input.time.milliseconds, input.randomSeed,
          input.deps.length].join('|');
        input.time.milliseconds = -1;
        input.deps.push(5);
        input.refs.changed = 1;
        input.__jerboaAssetMetadata.changed = 1;
        return result;
    "#;
    for time_ms in [
        0,
        1,
        i32::MAX as u64,
        i32::MAX as u64 + 1,
        MAX_EXACT_SCRIPT_MILLIS,
    ] {
        for seed in [0, i32::MAX as u32, u32::MAX] {
            let mut direct = ScriptRuntime::new().unwrap();
            let mut json_runtime = ScriptRuntime::new().unwrap();
            for _ in 0..2 {
                let actual = input::build(direct.context_mut(), time_ms, seed);
                install_reference_tables(
                    &actual,
                    input::empty_object(direct.context_mut()),
                    input::empty_object(direct.context_mut()),
                    direct.context_mut(),
                )
                .unwrap();
                let old = json!({
                    "time": {"seconds": time_ms as f64 / 1000.0, "milliseconds": time_ms},
                    "randomSeed": seed, "deps": [],
                });
                let expected = JsValue::from_json(&old, json_runtime.context_mut()).unwrap();
                let refs = JsValue::from_json(&json!({}), json_runtime.context_mut()).unwrap();
                let metadata = JsValue::from_json(&json!({}), json_runtime.context_mut()).unwrap();
                install_reference_tables(&expected, refs, metadata, json_runtime.context_mut())
                    .unwrap();
                assert_eq!(
                    direct
                        .call(probe, actual)
                        .unwrap()
                        .as_string()
                        .unwrap()
                        .to_std_string_escaped(),
                    json_runtime
                        .call(probe, expected)
                        .unwrap()
                        .as_string()
                        .unwrap()
                        .to_std_string_escaped(),
                    "time={time_ms}, seed={seed}",
                );
            }
        }
    }
}

#[test]
fn unsupported_only_borrows_original_and_mixed_bakes_preserve_it() {
    let original = modify(&document("return input.time.milliseconds;"), |raw| {
        raw["composition"]["dynamics"]["entries"][0]["animator"] =
            json!({"type": "jsScript", "code": "return 1;"});
    });
    let before = original.to_json_value().unwrap();
    let skipped = prepare(&original).unwrap();
    assert!(matches!(skipped.document, Cow::Borrowed(_)));
    assert_eq!(skipped.diagnostics.len(), 1);
    assert_eq!(original.to_json_value().unwrap(), before);
    let mixed = modify(&original, |raw| {
        let mut second = raw["composition"]["dynamics"]["entries"][0].clone();
        second["animator"] = json!({
            "type": "jsScript", "layerTimeJsCode": "return input.time.milliseconds;"
        });
        second["target"]["propertyType"] = json!("positionY");
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(second);
    });
    let mixed_before = mixed.to_json_value().unwrap();
    let result = prepare(&mixed).unwrap();
    assert!(matches!(result.document, Cow::Owned(_)));
    assert!(
        result.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert_eq!(track(&result, 1).keyframes().len(), 2);
    assert_eq!(mixed.to_json_value().unwrap(), mixed_before);
    assert!(
        result.diagnostics[0]
            .message
            .contains("legacy/mixed script clocks")
    );
}

#[test]
fn input_reference_table_shape_matches_playback() {
    let original = document(
        "if (!input.refs || Object.keys(input).includes('__jerboaAssetMetadata')) throw 'wrong ABI'; return getMetadata({layerId:7}, 'missing') === null ? 2 : 3;",
    );
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0).keyframes()[0].value(),
        &PropertyValue::Float(2.0)
    );
}

#[test]
fn seed_provenance_matches_the_existing_runtime_wire_contract() {
    let target = PropertyTarget::layer(LayerId::new(7), PropType::PositionX);
    assert_eq!(random_seed(seed::prefix(&target), 0), 3_792_147_118);
    assert_eq!(random_seed(seed::prefix(&target), 100), 3_792_097_914);
    let effect = PropertyTarget::effect_param(fx_schema::EffectId::new(101), "blurriness");
    assert_eq!(random_seed(seed::prefix(&effect), 0), 3_639_593_753);
    let original = modify(&document("return input.randomSeed;"), |raw| {
        raw["composition"]["dynamics"]["entries"][0]["randomSeedTarget"] =
            json!({"kind": "effectProperty", "effectId": 101, "paramName": "blurriness"});
    });
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0).keyframes()[0].value(),
        &PropertyValue::Float(3_639_593_753.0)
    );
}

#[test]
fn same_owner_dependency_bakes_original_script_and_convertible_sibling() {
    let original = modify(&document("return input.deps[0].value;"), |raw| {
        raw["composition"]["dynamics"]["entries"][0]["dependencies"] = json!([
            {"kind": "layer", "layerId": 7, "propertyType": "positionY"}
        ]);
        raw["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({
            "target": {"kind": "layer", "layerId": 7, "propertyType": "positionY"},
            "animator": {"type": "jsScript", "layerTimeJsCode": "return input.time.milliseconds;"}
        }));
    });
    let prepared = prepare(&original).unwrap();
    assert_eq!(track(&prepared, 0).keyframes().len(), 2);
    assert_eq!(track(&prepared, 1).keyframes().len(), 2);
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .dependencies
            .is_empty()
    );
}

#[test]
fn unrepresentable_script_time_publishes_neither_check_nor_write_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tsrct");
    let original = modify(&document("return 1;"), |raw| {
        raw["composition"]["layers"][0]["activeRange"]["duration"] =
            json!(MAX_EXACT_SCRIPT_MILLIS + 1);
    });
    TesseractFileBuilder::new(original).write(&input).unwrap();
    let bytes = std::fs::read(&input).unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = dir.path().join("output");
        let result = AfterEffects.export_from_tesseract_with_options(
            &input,
            &output,
            &AfterEffectsExportOptions::default(),
            mode,
        );
        assert!(result.is_err());
        assert!(!output.exists());
        assert_eq!(std::fs::read(&input).unwrap(), bytes);
    }
}

#[test]
fn check_and_write_bake_the_same_tracks_and_leave_the_archive_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.tsrct");
    let original = document("return 20 + input.time.milliseconds;");
    TesseractFileBuilder::from_project_json(&original.to_json_vec().unwrap())
        .unwrap()
        .write(&input)
        .unwrap();
    let before = std::fs::read(&input).unwrap();
    let check_dir = dir.path().join("check");
    let output = dir.path().join("write");
    let options = AfterEffectsExportOptions::default();
    let check = AfterEffects
        .export_from_tesseract_with_options(&input, &check_dir, &options, ConversionMode::Check)
        .unwrap();
    let write = AfterEffects
        .export_from_tesseract_with_options(&input, &output, &options, ConversionMode::Write)
        .unwrap();
    assert_eq!(check.diagnostics, write.diagnostics);
    assert!(!check_dir.exists());
    assert_eq!(std::fs::read(&input).unwrap(), before);
    let native = read_project(&std::fs::read(output.join("project.aep")).unwrap()).unwrap();
    let imported = crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    assert!(!imported.document.composition().layers().is_empty());
    assert!(
        imported
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .any(|entry| {
                entry
                    .animator
                    .keyframe_track()
                    .is_some_and(|track| track.keyframes().len() >= 2)
            })
    );
}
