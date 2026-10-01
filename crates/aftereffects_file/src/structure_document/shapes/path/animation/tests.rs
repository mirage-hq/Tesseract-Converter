use super::*;
use crate::structure::{ItemKind, read_project};
use fx_schema::{Layer, LayerData, PropType, PropertyTarget};
use sha2::{Digest, Sha256};

fn native_run() -> Vec<Chunk> {
    fn find(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"om-s") {
                let children = chunk.children().unwrap();
                if let Ok(meta) = unique_list(children, *b"tdbs").and_then(read_path_metadata)
                    && meta.keyframes.len() == 2
                {
                    return Some(vec![chunk.clone()]);
                }
            }
            if let Some(found) = chunk.children().and_then(find) {
                return Some(found);
            }
        }
        None
    }
    let project = read_project(include_bytes!(
        "../../../../../tests/fixtures/shapes/shape_misc.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("native composition missing")
    };
    find(&comp.layers[0].content).expect("native two-key mask path")
}

#[test]
fn native_mask_keys_import_as_editable_path_without_a_script() {
    let project = read_project(include_bytes!(
        "../../../../../tests/fixtures/shapes/shape_misc.aep"
    ))
    .unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    fn guide(layer: &Layer) -> Option<fx_schema::LayerId> {
        match layer.data() {
            LayerData::Group(group) => group
                .masks
                .first()
                .and_then(|mask| mask.layer)
                .or_else(|| group.layers.iter().find_map(guide)),
            _ => None,
        }
    }
    let guide = converted
        .document
        .composition()
        .layers()
        .iter()
        .find_map(guide)
        .unwrap();
    let entry = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(guide, PropType::ShapePath))
        .unwrap_or_else(|| panic!("{:?}", converted.diagnostics));
    assert!(matches!(
        entry.animator.data(),
        fx_schema::animator::AnimatorData::Keyframes { .. }
    ));
    fx_schema::EditableFxCompositionDocument::from_json_slice(
        &converted.document.to_json_vec().unwrap(),
    )
    .unwrap();
}

#[test]
fn pinned_native_paths_import_keys_and_match_independent_adobe_values() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/path-animation");
    let provenance: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("provenance.json")).unwrap()).unwrap();
    for fixture in provenance["files"].as_array().unwrap() {
        let bytes = crate::test_fixtures::read(dir.join(fixture["path"].as_str().unwrap()));
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            fixture["sha256"].as_str().unwrap()
        );
    }
    let mut checked = 0;
    for suffix in ["pin", "ease"] {
        let stem = format!("property_path_orientation_{suffix}");
        let project =
            read_project(&std::fs::read(dir.join(format!("{stem}.aep"))).unwrap()).unwrap();
        let oracle: serde_json::Value = serde_json::from_slice(&crate::test_fixtures::read(
            dir.join(format!("{stem}_value_at_time.json.gz")),
        ))
        .unwrap();
        for comp in oracle["comps"].as_array().unwrap() {
            let Some(item) = project
                .items
                .iter()
                .find(|item| item.name == comp["name"].as_str().unwrap())
            else {
                continue;
            };
            let converted =
                crate::structure_document::to_structural_fx_document(&project, Some(item.id))
                    .unwrap();
            // This is a public converter output contract, not a runtime check.
            let entries = converted.document.composition().dynamics().entries();
            assert!(entries.iter().all(|entry| !entry.animator.is_js_script()));
            for layer in comp["layers"].as_array().unwrap() {
                for property in layer["properties"].as_array().unwrap() {
                    if !matches!(
                        property["matchName"].as_str(),
                        Some("ADBE Vector Shape" | "ADBE Mask Shape")
                    ) {
                        continue;
                    }
                    let group = find_group(
                        converted.document.composition().layers(),
                        layer["name"].as_str().unwrap(),
                    )
                    .unwrap();
                    let mut ids = Vec::new();
                    // This upstream control-only probe has no Fill or Stroke.
                    // It proves hidden-producer numeric values, not visible motion.
                    shape_ids(&group.layers, &mut ids, false);
                    assert!(
                        converted
                            .document
                            .composition()
                            .dynamics()
                            .entries()
                            .iter()
                            .all(|entry| {
                                !ids.iter().any(|id| {
                                    entry.target == PropertyTarget::layer(*id, PropType::ShapePath)
                                        && matches!(entry.animator.data(), fx_schema::animator::AnimatorData::JsScript { layer_time_js_code: Some(code), .. } if code.starts_with("var K="))
                                })
                            })
                    );
                    ids.extend(group.masks.iter().filter_map(|mask| mask.layer));
                    let entry = entries
                        .iter()
                        .find(|entry| {
                            ids.iter().any(|id| {
                                entry.target == PropertyTarget::layer(*id, PropType::ShapePath)
                            })
                        })
                        .unwrap_or_else(|| {
                            panic!("missing {}: {:?}", property["path"], converted.diagnostics)
                        });
                    let fx_schema::animator::AnimatorData::Keyframes { track, .. } =
                        entry.animator.data()
                    else {
                        panic!("not editable keys")
                    };
                    let native_keys = property["keyframes"].as_array().unwrap();
                    assert_eq!(track.keyframes().len(), native_keys.len());
                    for (key, native) in track.keyframes().iter().zip(native_keys) {
                        assert_eq!(
                            key.layer_time().as_millis(),
                            (native["time"].as_f64().unwrap() * 1000.0).round() as i64
                        );
                        let PropertyValue::Path(path) = key.value() else {
                            panic!("not Path")
                        };
                        assert_native_outline(path, &native["value"]);
                        let expected = &native["value"]["vertices"][0];
                        let fx_schema::ShapePathCommand::MoveTo { x, y, .. } = path.commands[0]
                        else {
                            panic!("not MoveTo")
                        };
                        assert!((x - expected[0].as_f64().unwrap()).abs() < 0.001);
                        assert!((y - expected[1].as_f64().unwrap()).abs() < 0.001);
                    }
                    for frame in property["frames"].as_array().unwrap() {
                        let t = frame["time"].as_f64().unwrap() / 2.0;
                        let progress = progress(track.keyframes()[1].easing(), t.clamp(0.0, 1.0));
                        for axis in 0..2 {
                            let from = native_keys[0]["value"]["vertices"][0][axis]
                                .as_f64()
                                .unwrap();
                            let to = native_keys[1]["value"]["vertices"][0][axis]
                                .as_f64()
                                .unwrap();
                            let expected = frame["value"]["vertices"][0][axis].as_f64().unwrap();
                            assert!(
                                (from + (to - from) * progress - expected).abs() < 0.002,
                                "{} t={t} progress={progress}",
                                property["path"]
                            );
                        }
                    }
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 4, "one native shape and three native masks");
}

fn assert_native_outline(path: &fx_schema::ShapePath, native: &serde_json::Value) {
    let vertices = native["vertices"].as_array().unwrap();
    let closed = native["closed"].as_bool().unwrap();
    let segments = vertices.len() - usize::from(!closed);
    assert_eq!(path.commands.len(), 1 + segments + usize::from(closed));
    assert_eq!(
        matches!(
            path.commands.last(),
            Some(fx_schema::ShapePathCommand::Close)
        ),
        closed
    );
    for segment in 0..segments {
        let next = (segment + 1) % vertices.len();
        let fx_schema::ShapePathCommand::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
            ..
        } = path.commands[segment + 1]
        else {
            panic!("lost cubic command")
        };
        for (axis, actual) in [[c1x, c2x, x], [c1y, c2y, y]].into_iter().enumerate() {
            let from = vertices[segment][axis].as_f64().unwrap();
            let to = vertices[next][axis].as_f64().unwrap();
            let expected = [
                from + native["outTangents"][segment][axis].as_f64().unwrap(),
                to + native["inTangents"][next][axis].as_f64().unwrap(),
                to,
            ];
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 0.001,
                    "segment {segment}: {actual} != {expected}"
                );
            }
        }
    }
}

fn progress(easing: PropertyKeyframeEasing, x: f64) -> f64 {
    let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = easing else {
        return if easing == PropertyKeyframeEasing::Hold && x < 1.0 {
            0.0
        } else {
            x
        };
    };
    let cubic = |a: f64, b: f64, t: f64| {
        3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..50 {
        let t = (lo + hi) / 2.0;
        if cubic(x1, x2, t) < x { lo = t } else { hi = t }
    }
    cubic(y1, y2, (lo + hi) / 2.0)
}

fn find_group<'a>(layers: &'a [Layer], name: &str) -> Option<&'a fx_schema::GroupLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Group(group) if group.name == name => Some(group),
        LayerData::Group(group) => find_group(&group.layers, name),
        _ => None,
    })
}

fn shape_ids(layers: &[Layer], ids: &mut Vec<fx_schema::LayerId>, visible_only: bool) {
    for layer in layers {
        match layer.data() {
            LayerData::Shape(shape) if !visible_only || !shape.is_hidden => ids.push(shape.id),
            LayerData::Group(group) => shape_ids(&group.layers, ids, visible_only),
            _ => (),
        }
    }
}

#[test]
fn native_path_keys_budget_is_atomic_and_owner_clock_is_applied() {
    let run = native_run();
    let target = PropertyTarget::layer(fx_schema::LayerId::new(700), PropType::ShapePath);
    let mut budget = AnimationBudget::with_limit(1);
    let (output, warnings) = entries(
        &run,
        [400.0, 400.0],
        target.clone(),
        NumericAnimationClock::source_local(),
        &mut budget,
    );
    assert!(output.is_empty() && warnings.iter().any(|message| message.contains("budget")));
    assert_eq!(budget.used(), 0);
    let mut budget = AnimationBudget::default();
    let (output, warnings) = entries(
        &run,
        [400.0, 400.0],
        target,
        NumericAnimationClock::source_local_rebased(0.5),
        &mut budget,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(output.len(), 1);
    let fx_schema::animator::AnimatorData::Keyframes { track, .. } = output[0].animator.data()
    else {
        panic!("not keys")
    };
    let value = unique_list(super::super::property_run(&run).unwrap(), *b"om-s").unwrap();
    let metadata = read_path_metadata(unique_list(value, *b"tdbs").unwrap()).unwrap();
    for (key, native) in track.keyframes().iter().zip(&metadata.keyframes) {
        assert_eq!(
            key.layer_time().as_millis(),
            ((native.time_secs - 0.5) * 1000.0).round() as i64
        );
    }
    assert_eq!(
        budget.used(),
        committed_entry_reservation_bytes(&output[0]).unwrap()
    );
}

#[test]
fn native_mask_path_key_case_226_keeps_two_distinct_editable_keys() {
    let project = read_project(include_bytes!(
        "../../../../../tests/fixtures/masks/import_mask_controls.aep"
    ))
    .unwrap();
    let output = crate::structure_document::to_structural_fx_document(&project, Some(226)).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| !format!("{diagnostic:?}").contains("variable-width feather")),
        "{:?}",
        output.diagnostics
    );
    let target = find_group(output.document.composition().layers(), "target").unwrap();
    let guide = target.masks[0].layer.unwrap();
    let entry = output
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(guide, PropType::ShapePath))
        .unwrap();
    let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
        panic!("not editable keys")
    };
    assert_eq!(track.keyframes().len(), 2);
    assert_eq!(track.keyframes()[0].layer_time().as_millis(), 500);
    assert_eq!(track.keyframes()[1].layer_time().as_millis(), 2000);
    assert_ne!(track.keyframes()[0].value(), track.keyframes()[1].value());
}

#[test]
fn native_path_key_panel_keeps_linear_hold_and_bezier_authored_keys() {
    let project = read_project(include_bytes!(
        "../../../../../tests/fixtures/path-animation/import_path_key_cases.aep"
    ))
    .unwrap();
    for (comp, expected) in [
        (1, PropertyKeyframeEasing::Linear),
        (17, PropertyKeyframeEasing::Hold),
        (
            32,
            PropertyKeyframeEasing::CubicBezier {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            },
        ),
    ] {
        let output =
            crate::structure_document::to_structural_fx_document(&project, Some(comp)).unwrap();
        let mut visible_ids = Vec::new();
        shape_ids(
            output.document.composition().layers(),
            &mut visible_ids,
            true,
        );
        let tracks: Vec<_> = output
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .filter_map(|entry| {
                let target = entry.target.as_property()?;
                if target.property_type() != PropType::ShapePath
                    || !visible_ids.contains(&target.layer_id())
                {
                    return None;
                }
                let fx_schema::animator::AnimatorData::Keyframes { track, .. } =
                    entry.animator.data()
                else {
                    return None;
                };
                Some(track)
            })
            .collect();
        assert!(!tracks.is_empty(), "comp {comp}: {:?}", output.diagnostics);
        for track in tracks {
            assert_eq!(track.keyframes().len(), 2);
            assert_eq!(track.keyframes()[0].layer_time().as_millis(), 500);
            assert_eq!(track.keyframes()[1].layer_time().as_millis(), 2000);
            assert_eq!(track.keyframes()[1].easing(), expected);
            assert_ne!(track.keyframes()[0].value(), track.keyframes()[1].value());
        }
    }
}

#[test]
#[ignore = "requires local licensed AEP_INTRO_IMPORT_SOURCE; source cannot be redistributed"]
fn pinned_intro_mask_paths_keep_all_seven_editable_keys() {
    let bytes = std::fs::read(
        std::env::var_os("AEP_INTRO_IMPORT_SOURCE").expect("local licensed Intro source path"),
    )
    .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "75bb7d70238e23ffaafdeacf952217de1fcded8f86875bee39c2e91bda7804d9"
    );
    let project = read_project(&bytes).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(3)).unwrap();
    fn owner(layers: &[Layer], native_id: u32) -> Option<&fx_schema::GroupLayer> {
        layers.iter().find_map(|layer| {
            let LayerData::Group(group) = layer.data() else {
                return None;
            };
            if group
                .description
                .contains(&format!("comp=3 layer={native_id} kind="))
            {
                Some(group)
            } else {
                owner(&group.layers, native_id)
            }
        })
    }
    for native_id in [2103, 2105] {
        let group = owner(converted.document.composition().layers(), native_id).unwrap();
        assert_eq!(group.masks.len(), 1);
        let guide = group.masks[0].layer.expect("editable mask guide");
        let entry = converted
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(guide, PropType::ShapePath))
            .unwrap_or_else(|| panic!("native layer {native_id} mask animation was omitted"));
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
        else {
            panic!("native mask must remain editable keys");
        };
        assert_eq!(
            track
                .keyframes()
                .iter()
                .map(|key| key.layer_time().as_millis())
                .collect::<Vec<_>>(),
            [3542, 4167, 4292, 4333, 4375, 4417, 4458]
        );
        assert_ne!(track.keyframes()[0].value(), track.keyframes()[6].value());
        let expected = PropertyKeyframeEasing::CubicBezier {
            x1: 0.016,
            y1: 24.212053999690262 * 0.016,
            x2: 1.0 - 70.18391339768115 / 100.0,
            y2: 1.0,
        };
        assert_eq!(track.keyframes()[1].easing(), expected);
    }
}

#[test]
fn native_path_nonzero_speed_matches_independent_adobe_progress() {
    let run = native_run();
    let value = unique_list(super::super::property_run(&run).unwrap(), *b"om-s").unwrap();
    let metadata = read_path_metadata(unique_list(value, *b"tdbs").unwrap()).unwrap();
    // AE 26.5 valueAtTime readback of the source-pinned Intro masks, not
    // converter-generated expectations. Speeds are normalized Path slopes;
    // unlike numeric properties, neither duration nor geometry distance scales them.
    for (duration, out_speed, out_influence, in_speed, in_influence, distance, expected) in [
        (
            0.625,
            24.2120539996903,
            1.6,
            0.0,
            70.1839133976812,
            221.0,
            [0.6542114149051799, 0.8709216689940857, 0.971454045672673],
        ),
        (
            0.125,
            0.0,
            59.2610677083333,
            1.0,
            16.666666667,
            20.125,
            [0.05391716809003952, 0.23688919351706944, 0.5816764120729927],
        ),
        (
            1.0 / 24.0,
            1.0,
            16.666666667,
            22.764008523487,
            4.0,
            364.0,
            [0.10748056265024164, 0.19801632388607424, 0.3632720276549726],
        ),
    ] {
        let mut from = metadata.keyframes[0].clone();
        let mut to = metadata.keyframes[1].clone();
        from.time_secs = 0.0;
        to.time_secs = duration;
        from.out_interpolation = 2;
        to.in_interpolation = 2;
        from.out_speed = vec![out_speed];
        from.out_influence = vec![out_influence];
        to.in_speed = vec![in_speed];
        to.in_influence = vec![in_influence];
        let converted = easing(&from, &to).expect("native nonzero Path speed is representable");
        for (time, expected) in [0.25, 0.5, 0.75].into_iter().zip(expected) {
            // AE returns float32-quantized vertices (observed full-panel max
            // error 0.000102 px). Bound geometry error, not relative progress
            // on short moves: two float32 ULPs at the native 1600px extent.
            let error_pixels = (progress(converted, time) - expected).abs() * distance;
            assert!(error_pixels < 1.0 / 4096.0, "error {error_pixels} px");
        }
        // A slope of one is linear even when the native segment is very short.
        from.out_speed = vec![1.0];
        to.in_speed = vec![1.0];
        let converted = easing(&from, &to).unwrap();
        assert!((progress(converted, 0.37) - 0.37).abs() < 1e-12);
    }
}

#[test]
fn native_path_unproved_or_nonfinite_speed_retains_diagnostic() {
    let run = native_run();
    let value = unique_list(super::super::property_run(&run).unwrap(), *b"om-s").unwrap();
    let metadata = read_path_metadata(unique_list(value, *b"tdbs").unwrap()).unwrap();
    let mut from = metadata.keyframes[0].clone();
    let mut to = metadata.keyframes[1].clone();
    from.out_interpolation = 2;
    to.in_interpolation = 2;
    from.out_influence = vec![50.0];
    to.in_speed = vec![0.0];
    to.in_influence = vec![50.0];
    for speed in [f64::NAN, f64::INFINITY, -1.0, 3.0] {
        from.out_speed = vec![speed];
        assert!(easing(&from, &to).is_err(), "speed {speed}");
    }
}

#[test]
fn malformed_path_headers_keep_the_static_outline() {
    let mut run = native_run();
    fn corrupt(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"tdb4" {
                *chunk = Chunk::data(*b"tdb4", vec![0]).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                corrupt(children);
            }
        }
    }
    corrupt(&mut run);
    assert!(super::super::decode_first(&run, [400.0, 400.0]).is_ok());
    assert!(
        entries(
            &run,
            [400.0, 400.0],
            PropertyTarget::layer(fx_schema::LayerId::new(1), PropType::ShapePath),
            NumericAnimationClock::source_local(),
            &mut AnimationBudget::default()
        )
        .1
        .iter()
        .any(|warning| warning.contains("static editable path retained"))
    );
}
