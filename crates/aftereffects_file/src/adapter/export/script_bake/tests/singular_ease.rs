use super::*;

fn source() -> EditableFxCompositionDocument {
    modify(&document("0"), |raw| {
        raw["composition"]["layers"][0]["activeRange"] = json!({"start": 0, "duration": 200});
        raw["composition"]["dynamics"]["entries"] = json!([{
            "target": {"kind": "layer", "layerId": 7, "propertyType": "scaleY"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "scale-start", "layerTime": 0, "value": {"type": "float", "value": 100},
                 "easing": {"type": "linear"}},
                {"id": "scale-end", "layerTime": 200, "value": {"type": "float", "value": 0},
                 "easing": {"type": "cubicBezier", "x1": 0.55, "y1": 0, "x2": 1, "y2": 0.45}}
            ]}
        }]);
    })
}

#[test]
fn singular_scale_finite_temporal_ease_retains_owner_and_authored_keys() {
    let original = source();
    let before = original.to_json_value().unwrap();
    let prepared = prepare(&original).unwrap();
    let native = crate::export_document::to_aep(&prepared.document).unwrap();
    assert!(
        !native
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(7))),
        "Scale must not omit its owner: {:?}",
        native.diagnostics
    );
    let original_keys = original.composition().dynamics().entries()[0]
        .animator
        .keyframe_track()
        .unwrap()
        .keyframes();
    let keys = track(&prepared, 0).keyframes();
    assert_eq!(keys.len(), 2, "no key resampling");
    for (key, original) in keys.iter().zip(original_keys) {
        assert_eq!(key.id(), original.id());
        assert_eq!(key.layer_time(), original.layer_time());
        assert_eq!(key.value(), original.value());
    }
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(7))
                && d.message.contains("finite temporal-ease approximation"))
    );
    assert_eq!(original.to_json_value().unwrap(), before);
    let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = keys[1].easing() else {
        panic!("finite native cubic expected");
    };
    assert_eq!((x1, y1, x2), (0.55, 0.0, 0.999));
    assert!((y2 - 0.44499078).abs() < 1e-6);
    assert!(prepared.diagnostics[0].message.contains("0.158"));
}

#[test]
fn singular_scale_disabled_ordinary_and_spatial_profiles_remain_unchanged() {
    for (property, enabled, x2, y2) in [
        ("scaleY", false, 1.0, 0.45),
        ("scaleY", true, 0.8, 0.45),
        ("scaleY", true, 1.0, 1.0),
        ("positionY", true, 1.0, 0.45),
    ] {
        let original = modify(&source(), |raw| {
            let entry = &mut raw["composition"]["dynamics"]["entries"][0];
            entry["target"]["propertyType"] = json!(property);
            entry["animator"]["enabled"] = json!(enabled);
            if !enabled {
                entry["animator"]["disabledValue"] = json!({"type":"float", "value":100});
            }
            entry["animator"]["keyframes"][1]["easing"]["x2"] = json!(x2);
            entry["animator"]["keyframes"][1]["easing"]["y2"] = json!(y2);
        });
        let prepared = prepare(&original).unwrap();
        assert!(prepared.diagnostics.is_empty());
        assert_eq!(
            prepared.document.to_json_value().unwrap(),
            original.to_json_value().unwrap()
        );
    }
}

#[test]
fn singular_scale_selected_scope_keeps_unselected_track() {
    let original = source();
    let prepared = super::super::prepare_layers(&original, &[], true).unwrap();
    assert!(prepared.diagnostics.is_empty());
    assert_eq!(
        prepared.document.to_json_value().unwrap(),
        original.to_json_value().unwrap()
    );
}

#[test]
fn singular_scale_unsupported_script_dependency_remains_diagnosed() {
    let original = modify(&source(), |raw| {
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "target": {"kind":"layer", "layerId":7, "propertyType":"rotation"},
                "dependencies": [{"kind":"layer", "layerId":7, "propertyType":"scaleY"}],
                "animator": {"type":"jsScript", "layerTimeJsCode":"return input.deps[0].value;"}
            }));
    });
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        prepared.document.to_json_value().unwrap()["composition"]["dynamics"]["entries"][1],
        original.to_json_value().unwrap()["composition"]["dynamics"]["entries"][1],
    );
    assert!(prepared.diagnostics.iter().any(|d| {
        d.message
            .contains("dependency is not an independent layer-time scalar script")
    }));
}
