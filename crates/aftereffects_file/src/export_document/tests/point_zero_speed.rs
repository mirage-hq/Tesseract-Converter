//! Pinned native zero-speed Point timing, fresh export and editable input response.
//! Native render/alpha/audio fidelity is not established by these CPU checks.

use super::*;

const SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/properties/point_zero_speed.aep");

fn imported_point() -> Value {
    let native = read_project(SOURCE).expect("pinned Adobe source");
    let imported = to_structural_fx_document(&native, Some(1)).expect("fresh Point import");
    let value = imported.document.to_json_value().expect("editable FX");
    let entries = value["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("imported dynamics");
    for (param, expected) in [("centerX", [0.5, 0.6]), ("centerY", [0.5, 0.4125])] {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["paramName"] == param)
            .expect("native Point scalar component");
        let keys = entry["animator"]["keyframes"]
            .as_array()
            .expect("editable keys");
        assert_eq!(keys.len(), 2);
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(key["layerTime"], [500, 1500][index]);
            assert!((key["value"]["value"].as_f64().unwrap() - expected[index]).abs() < 1e-12);
        }
        assert_eq!(keys[1]["easing"]["type"], "cubicBezier");
        assert_eq!(keys[1]["easing"]["y1"], 0.0);
        assert_eq!(keys[1]["easing"]["y2"], 1.0);
    }
    value
}

#[test]
fn point_zero_speed_unsupported_ease_retains_static_owner() {
    let mut value = imported_point();
    let entries = value["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    entries[0]["animator"]["keyframes"][1]["easing"]["y1"] = json!(0.2);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = to_aep(&document).expect("unsupported animation retains static owner");
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("unsupported cubic Point animation omitted")
    }));
    let native = read_project(&output.bytes).unwrap();
    let owners: Vec<_> = native
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return None;
            };
            Some(&comp.layers)
        })
        .flatten()
        .collect();
    // Imported hierarchy helpers are valid owners too; count the actual
    // Point control, not every AV layer across generated precompositions.
    let points: Vec<_> = owners
        .iter()
        .flat_map(|owner| crate::effects::native::read_effects(&owner.content, [120.0, 80.0]).0)
        .flat_map(|effect| effect.parameters)
        .filter_map(|parameter| {
            (parameter.match_name == "ADBE Radial Blur-0002").then_some(parameter.numeric)
        })
        .flatten()
        .collect();
    assert_eq!(points.len(), 1);
    assert!(points[0].keyframes.is_empty());
}

#[test]
fn point_zero_speed_native_source_exports_and_edits_without_static_fallback() {
    let input = imported_point();
    for (case, edited) in [("original", false), ("edited", true)] {
        let mut value = input.clone();
        if edited {
            for entry in value["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
            {
                let end = match entry["target"]["paramName"].as_str() {
                    Some("centerX") => 0.8,
                    Some("centerY") => 0.7,
                    _ => continue,
                };
                entry["animator"]["keyframes"][1]["value"]["value"] = json!(end);
            }
        }
        let document = EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
        let output = to_aep(&document).expect("fresh full Point export");
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("cubic Point animation omitted")),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).expect("fresh AEP own-reader");
        let points: Vec<_> = native
            .items
            .iter()
            .filter_map(|item| {
                let ItemKind::Composition(comp) = &item.kind else {
                    return None;
                };
                Some(
                    comp.layers
                        .iter()
                        .flat_map(|layer| {
                            crate::effects::native::read_effects(&layer.content, [120.0, 80.0]).0
                        })
                        .filter_map(|effect| {
                            effect
                                .parameters
                                .into_iter()
                                .find(|p| p.match_name == "ADBE Radial Blur-0002")
                                .map(|point| point.numeric.expect("numeric Point"))
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .flatten()
            .collect();
        assert_eq!(points.len(), 1, "one fresh Point owner, no donor replay");
        let keys = &points[0].keyframes;
        assert_eq!(keys.len(), 2, "native Point must not fall back to static");
        let end = if edited { [96.0, 56.0] } else { [72.0, 33.0] };
        for (index, expected) in [[60.0, 40.0], end].into_iter().enumerate() {
            assert!((keys[index].time_secs - [0.5, 1.5][index]).abs() < 1e-12);
            for (actual, expected) in keys[index].values.iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "{case}: {actual} != {expected}"
                );
            }
            assert!(
                keys[index]
                    .in_speed
                    .iter()
                    .chain(&keys[index].out_speed)
                    .all(|speed| *speed == 0.0)
            );
            assert!(
                keys[index]
                    .spatial_in
                    .iter()
                    .chain(&keys[index].spatial_out)
                    .all(|value| *value == 0.0)
            );
        }
        assert_eq!(keys[0].out_interpolation, 2);
        assert_eq!(keys[1].in_interpolation, 2);
    }
}
