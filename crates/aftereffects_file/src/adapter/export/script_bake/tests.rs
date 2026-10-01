use super::*;
use crate::{AfterEffects, AfterEffectsExportOptions, structure::read_project};
use fx_conv::{ConversionMode, Progress};
use serde_json::Value;
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

    prepare_with_progress(&original, Progress::new(&callback)).unwrap();

    let events = events.into_inner().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[3].phase, "validate baked AE document");
    assert!(events[3].total.is_none());
    assert_eq!(events[0].phase, "bake AE scripts");
    assert_eq!(events[0].total, Some(2));
    assert_eq!(events[1].completed, Some(1));
    assert_eq!(events[2].completed, Some(2));
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
fn path_script_bake_keeps_steps_and_variable_contours_in_owner_clock() {
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
        [0, 17, 34, 51, 68, 85]
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
fn canonical_media_shifted_anchors_and_time_remaps_keep_their_script_with_a_diagnostic() {
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
            assert!(
                prepared.document.composition().dynamics().entries()[0]
                    .animator
                    .is_js_script()
            );
            assert_eq!(prepared.diagnostics.len(), 1);
            assert!(
                prepared.diagnostics[0]
                    .message
                    .contains("owner or ancestor playback remapping is not supported")
            );
        }
    }
}

#[test]
fn delayed_plain_group_clocks_bake_owners_and_children_while_nonplain_clocks_propagate() {
    for (source_start, source_duration, supported) in
        [(0, 100, true), (1, 100, false), (0, 200, false)]
    {
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
        for entry in prepared.document.composition().dynamics().entries() {
            assert_eq!(entry.animator.is_js_script(), !supported);
            if supported {
                let keys = entry.animator.keyframe_track().unwrap().keyframes();
                assert_eq!(
                    (
                        keys[0].layer_time().as_millis(),
                        keys[1].layer_time().as_millis()
                    ),
                    (0, 100)
                );
                assert_eq!(keys[0].value(), &PropertyValue::Float(0.5));
                assert_eq!(keys[1].value(), &PropertyValue::Float(0.6));
            }
        }
        if !supported {
            assert_eq!(prepared.diagnostics.len(), 2);
            assert!(prepared.diagnostics.iter().all(|diagnostic| {
                diagnostic
                    .message
                    .contains("owner or ancestor playback remapping is not supported")
            }));
        }
    }
}

#[test]
fn accumulated_evaluations_do_not_reject_the_next_script() {
    let mut budget = Budget {
        calls: 20_000_000,
        probes: 80_000_000,
        keys: 250_000,
    };
    let mut runtime = ScriptRuntime::new().unwrap();
    assert_eq!(
        evaluate(&mut runtime, "return 1;", 0, 0, &mut budget).unwrap(),
        1.0
    );
    budget.probe().unwrap();
    assert_eq!((budget.calls, budget.probes), (20_000_001, 80_000_001));
    let original = document("return 1;");
    let entry = &original.composition().dynamics().entries()[0];
    let animator = bake_entry(
        entry,
        Some(Owner {
            id: original.composition().layers()[0].id(),
            duration_ms: 2,
            unsupported_clock: false,
        }),
        &mut BTreeSet::new(),
        &mut budget,
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
fn cache_hits_and_misses_count_work_and_reject_counter_overflow() {
    let mut budget = Budget::default();
    let mut runtime = ScriptRuntime::new().unwrap();
    let mut samples = BTreeMap::new();
    let code = "return input.time.milliseconds;";
    assert_eq!(
        cached_evaluate(&mut runtime, code, 0, 2, &mut samples, &mut budget).unwrap(),
        2.0
    );
    assert_eq!((budget.calls, budget.probes), (1, 1));
    cached_evaluate(&mut runtime, code, 0, 2, &mut samples, &mut budget).unwrap();
    assert_eq!((budget.calls, budget.probes), (1, 2));
    budget.probes = usize::MAX;
    assert!(matches!(
        cached_evaluate(&mut runtime, code, 0, 2, &mut samples, &mut budget),
        Err(BakeError::Budget("probe counter overflow"))
    ));
    budget.probes = 0;
    budget.calls = usize::MAX;
    assert!(matches!(
        cached_evaluate(&mut runtime, code, 0, 3, &mut samples, &mut budget),
        Err(BakeError::Budget("evaluation counter overflow"))
    ));
    // A cache hit does not evaluate JS, even when the call counter is exhausted.
    assert_eq!(
        cached_evaluate(&mut runtime, code, 0, 2, &mut samples, &mut budget).unwrap(),
        2.0
    );
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
fn unsupported_dependency_keeps_its_script_and_convertible_sibling() {
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
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert_eq!(track(&prepared, 1).keyframes().len(), 2);
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|item| item.message.contains("dependency or layer-reference"))
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
