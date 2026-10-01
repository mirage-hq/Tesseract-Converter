use super::stroke_keys::{document, numeric, numeric_layer};
use super::*;
use fx_schema::animator::{KeyframeId, PropertyAnimator, PropertyKeyframe};

fn join_entry() -> AnimationGraphEntry {
    let keys = [(0, "miter"), (250, "round"), (500, "bevel")]
        .into_iter()
        .enumerate()
        .map(|(index, (time, join))| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("join-{index}")),
                fx_schema::TimeOffset::from_millis(time),
                PropertyValue::String(join.into()),
                if index == 0 {
                    PropertyKeyframeEasing::Linear // No incoming segment at the first key.
                } else {
                    PropertyKeyframeEasing::Hold
                },
            )
        })
        .collect();
    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(LayerId::new(400), PropType::StrokeJoin),
        animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

#[test]
fn edited_join_keys_keep_native_enums_hold_segments_and_leaf_clocks() {
    // Explicit editable FX input. The shared Transform fixture provides archive
    // metadata only, not independent Adobe-native Join animation evidence.
    for kind in ["Rect", "Shape", "BooleanOperation"] {
        for inherited in [false, true] {
            for delayed in [false, true] {
                let mut value = document(kind, PropertyKeyframeEasing::Linear, inherited, delayed);
                value["composition"]["dynamics"]["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(join_entry()));
                let output = export(value);
                let native = read_project(&output.bytes).unwrap();
                assert_eq!(layers(&native).len(), 1, "{kind}: {:?}", output.diagnostics);
                let (layer, join) =
                    numeric_layer(&native, layers(&native), "ADBE Vector Stroke Line Join")
                        .expect("Join owner survives in its generated composition");
                if delayed {
                    assert_eq!(layer.record.start_time(), Some(0.25));
                }
                if inherited {
                    assert_eq!(
                        numeric_layer(&native, layers(&native), "ADBE Rotate Z")
                            .expect("inherited Group rotation survives")
                            .1
                            .values,
                        vec![30.0]
                    );
                }
                assert!(join.animated);
                assert_eq!(join.keyframes.len(), 3);
                for (key, (time, value)) in
                    join.keyframes
                        .iter()
                        .zip([(0.0, 1.0), (0.25, 2.0), (0.5, 3.0)])
                {
                    assert_eq!(key.time_secs, time);
                    assert_eq!(key.values, vec![value]);
                }
                for pair in join.keyframes.windows(2) {
                    assert_eq!(pair[0].out_interpolation, 3);
                    assert_eq!(pair[1].in_interpolation, 3);
                }
                for (name, values) in [
                    ("ADBE Vector Stroke Width", [2.0, 10.0]),
                    ("ADBE Vector Stroke Miter Limit", [4.0, 12.0]),
                ] {
                    let property = numeric(&layer.content, name).unwrap();
                    assert_eq!(property.keyframes.len(), 2);
                    assert_eq!(property.keyframes[0].values, vec![values[0]]);
                    assert_eq!(property.keyframes[1].values, vec![values[1]]);
                }
            }
        }
    }
}

#[test]
fn unsupported_join_animators_keep_a_convertible_sibling() {
    for kind in ["Rect", "Shape", "BooleanOperation"] {
        for case in ["constant", "disabled", "absent_stroke"] {
            let mut value = document(kind, PropertyKeyframeEasing::Linear, false, false);
            let mut join = join_entry();
            if case == "constant" {
                join.animator =
                    PropertyAnimator::constant(PropertyValue::String("round".into())).unwrap();
            }
            if case == "disabled" {
                let mut data = join.animator.data().clone();
                let AnimatorData::Keyframes {
                    enabled,
                    disabled_value,
                    ..
                } = &mut data
                else {
                    panic!("keyed join");
                };
                *enabled = false;
                *disabled_value = Some(PropertyValue::String("round".into()));
                join.animator = PropertyAnimator::from_data(&data).unwrap();
            }
            value["composition"]["dynamics"] = json!({"entries":[join]});
            if case == "disabled" {
                // The current portable schema rejects disabled enum values.
                // Do not change FX validation to manufacture converter support.
                let error = EditableFxCompositionDocument::from_json_value(value).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("disabled property keyframe value is invalid")
                );
                continue;
            }
            if case == "absent_stroke" {
                let leaf = &mut value["composition"]["layers"][0];
                if kind == "Rect" {
                    leaf["rect"]["strokeEnabled"] = json!(false);
                    leaf["rect"]["fillEnabled"] = json!(true);
                } else {
                    let paint = if kind == "Shape" {
                        &mut leaf["shape"]
                    } else {
                        leaf
                    };
                    paint["strokes"] = json!([]);
                    paint["fills"] = json!([{"paint":{"type":"solid","color":[1.0,1.0,1.0,1.0]}}]);
                }
            }
            value["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .push(rect(&imported(), 900));
            let output = export(value);
            let native = read_project(&output.bytes).unwrap();
            if case == "constant" {
                assert_eq!(
                    layers(&native).len(),
                    2,
                    "{kind}/{case}: {:?}",
                    output.diagnostics
                );
                let stroke_layer = layers(&native)
                    .iter()
                    .find(|layer| layer.name.as_ref() != "Current solid 900")
                    .expect("supported constant Join remains editable");
                let (_, join) = numeric_layer(
                    &native,
                    std::slice::from_ref(stroke_layer),
                    "ADBE Vector Stroke Line Join",
                )
                .expect("supported constant Join has a native property");
                assert!(join.animated);
                assert_eq!(join.keyframes.len(), 1);
                assert_eq!(join.keyframes[0].time_secs, 0.0);
                assert_eq!(join.keyframes[0].values, [2.0]);
            } else {
                assert_eq!(
                    layers(&native).len(),
                    1,
                    "{kind}/{case}: {:?}",
                    output.diagnostics
                );
                assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 900");
                assert!(output.diagnostics.iter().any(|diagnostic| {
                    diagnostic.layer_id == Some(LayerId::new(400))
                        && diagnostic.message.contains("omitted")
                }));
            }
        }
    }
}

#[test]
fn join_export_does_not_coerce_unknown_or_numeric_values() {
    assert!(stroke_join_track(None).unwrap().is_none());
    for value in [
        PropertyValue::String("Round".into()),
        PropertyValue::Float(2.0),
    ] {
        let track = PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
            KeyframeId::new("invalid-join"),
            fx_schema::TimeOffset::from_millis(0),
            value,
            PropertyKeyframeEasing::Hold,
        )])
        .unwrap();
        assert!(stroke_join_track(Some(NativeTrack::Keyframes(&track))).is_err());
    }
}
