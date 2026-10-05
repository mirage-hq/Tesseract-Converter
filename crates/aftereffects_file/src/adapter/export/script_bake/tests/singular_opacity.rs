use super::*;

// Numeric tracks extracted from Wave owner 2200063, remapped to the existing
// public fixture's Rect. No source archive, media or private metadata is shipped.
// This is source-derived CPU regression evidence, not an Adobe-native oracle.
fn source() -> EditableFxCompositionDocument {
    modify(&document("return 0;"), |raw| {
        raw["composition"]["layers"][0]["activeRange"] = json!({"start": 0, "duration": 1000});
        raw["composition"]["dynamics"] =
            serde_json::from_str(include_str!("singular_opacity_tracks.json")).unwrap();
    })
}

fn constant_tracks(durations: &[i64]) -> EditableFxCompositionDocument {
    modify(&source(), |raw| {
        let layer = raw["composition"]["layers"][0].clone();
        let entry = raw["composition"]["dynamics"]["entries"][2].clone();
        let mut layers = Vec::new();
        let mut entries = Vec::new();
        for (index, duration) in durations.iter().enumerate() {
            let mut layer = layer.clone();
            layer["id"] = json!(7 + index);
            layers.push(layer);
            let mut entry = entry.clone();
            entry["target"]["layerId"] = json!(7 + index);
            entry["animator"]["keyframes"] = json!([
                {"id":format!("constant-{index}-0"),"layerTime":0,"value":{"type":"float","value":50.0},"easing":{"type":"linear"}},
                {"id":format!("constant-{index}-1"),"layerTime":duration,"value":{"type":"float","value":50.0},"easing":{"type":"cubicBezier","x1":0.55,"y1":0.0,"x2":1.0,"y2":0.45}}
            ]);
            entries.push(entry);
        }
        raw["composition"]["layers"] = json!(layers);
        raw["composition"]["dynamics"]["entries"] = json!(entries);
    })
}

#[test]
fn singular_opacity_early_expansion_reserves_large_linear_suffix() {
    let original = modify(&constant_tracks(&[2]), |raw| {
        let keys = raw["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
            .as_array_mut()
            .unwrap();
        keys[1]["value"]["value"] = json!(0.0);
        keys.extend((3..=65_535).map(|time| {
            json!({"id":format!("suffix-{time}"),"layerTime":time,
                "value":{"type":"float","value":0.0},"easing":{"type":"linear"}})
        }));
        assert_eq!(keys.len(), 65_535);
    });
    let prepared = prepare(&original).unwrap();
    assert!(
        prepared.document.to_json_value().unwrap() == original.to_json_value().unwrap(),
        "over-limit expansion must retain the whole original track"
    );
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("native key count exceeded"))
    );
}

#[test]
fn singular_opacity_long_constant_track_is_declined_before_sampling() {
    let original = constant_tracks(&[65_535]);
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        prepared.document.to_json_value().unwrap(),
        original.to_json_value().unwrap()
    );
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sample limit"))
    );
}

#[test]
fn singular_opacity_huge_decline_preserves_smaller_sibling() {
    let original = constant_tracks(&[1_000_000_000, 66]);
    let saved = original.to_json_value().unwrap();
    let prepared = prepare(&original).unwrap();
    let raw = prepared.document.to_json_value().unwrap();
    assert_eq!(
        raw["composition"]["dynamics"]["entries"][0],
        saved["composition"]["dynamics"]["entries"][0]
    );
    assert_eq!(
        track(&prepared, 1).keyframes()[1].easing(),
        PropertyKeyframeEasing::Linear
    );
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(7)) && d.message.contains("sample limit"))
    );
    assert!(prepared.diagnostics.iter().any(
        |d| d.layer_id == Some(LayerId::new(8)) && d.message.contains("sampled original curve")
    ));
    assert_eq!(original.to_json_value().unwrap(), saved);
}

#[test]
fn singular_opacity_preparation_wide_sample_accounting() {
    let original = constant_tracks(&[30_000, 30_000, 30_000, 66]);
    let prepared = prepare(&original).unwrap();
    for index in [0, 1, 3] {
        assert_eq!(
            track(&prepared, index).keyframes()[1].easing(),
            PropertyKeyframeEasing::Linear
        );
    }
    let raw = prepared.document.to_json_value().unwrap();
    let saved = original.to_json_value().unwrap();
    assert_eq!(
        raw["composition"]["dynamics"]["entries"][2],
        saved["composition"]["dynamics"]["entries"][2]
    );
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(9)) && d.message.contains("5533 remaining"))
    );
}

#[test]
fn singular_opacity_whole_track_admission_preserves_early_segment() {
    let original = modify(&constant_tracks(&[1_000_000_000]), |raw| {
        let keys = &mut raw["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"];
        let mut middle = keys[1].clone();
        middle["id"] = json!("middle");
        middle["layerTime"] = json!(66);
        keys.as_array_mut().unwrap().insert(1, middle);
    });
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        prepared.document.to_json_value().unwrap(),
        original.to_json_value().unwrap()
    );
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sample limit"))
    );
}

#[test]
fn singular_opacity_wave_retains_owner_and_xy_keys() {
    let original = source();
    let original_json = original.to_json_value().unwrap();
    let prepared = prepare(&original).unwrap();
    let native = crate::export_document::to_aep(&prepared.document).unwrap();
    assert!(
        !native
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(7))),
        "Opacity must not omit its owner: {:?}",
        native.diagnostics
    );
    let keys = track(&prepared, 2).keyframes();
    assert!(keys.len() > 4);
    assert!(keys.len() < 70);
    let raw = prepared.document.to_json_value().unwrap();
    for index in 0..2 {
        assert_eq!(
            raw["composition"]["dynamics"]["entries"][index],
            original_json["composition"]["dynamics"]["entries"][index]
        );
        assert_eq!(track(&prepared, index).keyframes().len(), 14);
    }
    assert_eq!(
        raw["composition"]["layers"],
        original_json["composition"]["layers"]
    );
    assert_eq!(
        &keys[..3],
        &original.composition().dynamics().entries()[2]
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()[..3]
    );
    assert_eq!(keys.last().unwrap().layer_time().as_millis(), 466);
    assert_eq!(keys.last().unwrap().value(), &PropertyValue::Float(0.0));
    assert_eq!(original.to_json_value().unwrap(), original_json);
}

#[test]
fn singular_opacity_tunnel_preserves_preceding_hold_and_endpoints() {
    let original = modify(&source(), |raw| {
        let keys = &mut raw["composition"]["dynamics"]["entries"][2]["animator"]["keyframes"];
        *keys = json!([
            {"id":"t0", "layerTime":0,"value":{"type":"float","value":22.0},"easing":{"type":"hold"}},
            {"id":"t1", "layerTime":66,"value":{"type":"float","value":34.0},"easing":{"type":"linear"}},
            {"id":"t2", "layerTime":266,"value":{"type":"float","value":0.0},"easing":{"type":"cubicBezier","x1":0.55,"y1":0.0,"x2":1.0,"y2":0.45}}
        ]);
    });
    let prepared = prepare(&original).unwrap();
    let keys = track(&prepared, 2).keyframes();
    let previous = original.composition().dynamics().entries()[2]
        .animator
        .keyframe_track()
        .unwrap();
    assert_eq!(&keys[..2], &previous.keyframes()[..2]);
    assert_eq!(keys.last().unwrap().layer_time().as_millis(), 266);
    assert_eq!(keys.last().unwrap().value(), &PropertyValue::Float(0.0));
    assert!(keys.len() > 3 && keys.len() < 202);
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.message.contains("66..266ms"))
    );
}

// Fresh native readback is supplementary evidence: the Linear evaluator below
// does not use the fitter, but our reader is not independent Adobe inspection.
fn native_opacity(chunks: &[crate::rifx::Chunk]) -> Option<crate::properties::NumericProperty> {
    if let Ok(runs) = crate::properties::runs(chunks) {
        for (name, run) in runs {
            if name == "ADBE Opacity" {
                let storage = crate::properties::unique_list(run, *b"tdbs").unwrap();
                let property = crate::properties::read_numeric(storage).unwrap();
                if property.animated {
                    return Some(property);
                }
            }
        }
    }
    chunks
        .iter()
        .filter_map(crate::rifx::Chunk::children)
        .find_map(native_opacity)
}

#[test]
fn singular_opacity_fresh_native_grid_reports_quantized_error() {
    for (owner, start, end, from) in [(2200063, 400, 466, 100.0), (200270, 66, 266, 34.0)] {
        let original = modify(&source(), |raw| {
            if owner == 200270 {
                raw["composition"]["dynamics"]["entries"][2]["animator"]["keyframes"] = json!([
                    {"id":"t0", "layerTime":0,"value":{"type":"float","value":22.0},"easing":{"type":"hold"}},
                    {"id":"t1", "layerTime":66,"value":{"type":"float","value":34.0},"easing":{"type":"linear"}},
                    {"id":"t2", "layerTime":266,"value":{"type":"float","value":0.0},"easing":{"type":"cubicBezier","x1":0.55,"y1":0.0,"x2":1.0,"y2":0.45}}
                ]);
            }
        });
        let prepared = prepare(&original).unwrap();
        let exported = crate::export_document::to_aep(&prepared.document).unwrap();
        let fresh = crate::structure::read_project(&exported.bytes).unwrap();
        let crate::structure::ItemKind::Composition(root) = &fresh.item(1).unwrap().kind else {
            panic!("fresh root composition");
        };
        let native = root
            .layers
            .iter()
            .find_map(|layer| native_opacity(&layer.content))
            .unwrap();
        assert_eq!(
            native.keyframes.len(),
            track(&prepared, 2).keyframes().len()
        );
        let mut maximum_error: f64 = 0.0;
        for time in start..=end {
            let seconds = time as f64 / 1000.0;
            let interval = native
                .keyframes
                .partition_point(|key| key.time_secs < seconds);
            let saved = if interval == 0 {
                native.keyframes[0].values[0]
            } else if interval == native.keyframes.len() {
                native.keyframes.last().unwrap().values[0]
            } else {
                let a = &native.keyframes[interval - 1];
                let b = &native.keyframes[interval];
                assert_eq!(b.in_interpolation, 1, "generated segment must be Linear");
                a.values[0]
                    + (b.values[0] - a.values[0]) * (seconds - a.time_secs)
                        / (b.time_secs - a.time_secs)
            };
            let progress = (time - start) as f64 / (end - start) as f64;
            let u = crate::export_document::effects::invert_bezier(progress, 0.55, 1.0);
            let expected = from * (1.0 - crate::export_document::effects::bezier(u, 0.0, 0.45));
            maximum_error = maximum_error.max((saved * 100.0 - expected).abs());
        }
        assert!(maximum_error.is_finite());
        // The pre-native 0.01pp cap does not include PropertyClock rounding.
        // Report actual native error rather than silently relaxing that cap.
        println!(
            "owner={owner} segment={start}..{end}ms samples={} native_keys={} native_max_error_pp={maximum_error:.12}",
            end - start + 1,
            native.keyframes.len()
        );
    }
}

#[test]
fn singular_opacity_malformed_x_controls_keep_schema_rejection() {
    for (control, value) in [("x1", -0.1), ("x2", 1.1)] {
        let mut raw = source().to_json_value().unwrap();
        raw["composition"]["dynamics"]["entries"][2]["animator"]["keyframes"][3]["easing"]
            [control] = json!(value);
        assert!(EditableFxCompositionDocument::from_json_value(raw).is_err());
    }
}

#[test]
fn singular_opacity_ordinary_bezier_and_disabled_tracks_unchanged() {
    for disabled in [false, true] {
        let original = modify(&source(), |raw| {
            let animator = &mut raw["composition"]["dynamics"]["entries"][2]["animator"];
            if disabled {
                animator["enabled"] = json!(false);
                animator["disabledValue"] = json!({"type":"float","value":50.0});
            } else {
                animator["keyframes"][3]["easing"]["x2"] = json!(0.8);
            }
        });
        let prepared = prepare(&original).unwrap();
        assert_eq!(
            prepared.document.to_json_value().unwrap(),
            original.to_json_value().unwrap()
        );
        assert!(prepared.diagnostics.is_empty());
    }
}
