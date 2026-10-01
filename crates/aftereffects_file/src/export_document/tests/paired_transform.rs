use super::*;
use fx_schema::animator::{KeyframeId, PropertyKeyframe, PropertyKeyframeTrack};
use sha2::{Digest, Sha256};

fn native_scale_oracle() -> StructuralProject {
    let bytes = include_bytes!("../../../tests/fixtures/export_repairs/native-controls.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "f62f72bf6ec281a63be63ec22b35e7552ee1ef48e4b8db97bb19773f372450a3"
    );
    read_project(bytes).unwrap()
}

fn scalar_keys(points: &[(i64, f64, PropertyKeyframeEasing)]) -> PropertyKeyframeTrack {
    PropertyKeyframeTrack::new(
        points
            .iter()
            .enumerate()
            .map(|(index, &(time, value, easing))| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("paired-{index}")),
                    fx_schema::TimeOffset::from_millis(time),
                    PropertyValue::Float(value),
                    easing,
                )
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn paired_scale_native_hold_and_cubic_owners_survive_fresh_export() {
    let native = native_scale_oracle();
    let document = to_structural_fx_document(&native, Some(17))
        .unwrap()
        .document;
    let output = to_aep(&document).unwrap();
    assert!(
        !output.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("invalid native numeric keyframe")),
        "native Hold/Cubic Scale owners must survive: {:?}",
        output.diagnostics
    );
    let exported = read_project(&output.bytes).unwrap();
    let restored = to_structural_fx_document(&exported, Some(1))
        .unwrap()
        .document;
    for (property, times, values, ease) in [
        (
            PropType::ScaleX,
            [250, 1100, 1650],
            [38.0, 151.0, 82.0],
            "hold",
        ),
        (
            PropType::ScaleY,
            [250, 1100, 1650],
            [142.0, 64.0, 123.0],
            "hold",
        ),
        (
            PropType::ScaleX,
            [200, 950, 1700],
            [53.0, 142.0, 76.0],
            "cubic",
        ),
        (
            PropType::ScaleY,
            [200, 950, 1700],
            [128.0, 57.0, 151.0],
            "cubic",
        ),
    ] {
        for checked in [&document, &restored] {
            let track = checked
                .composition()
                .dynamics()
                .entries()
                .iter()
                .find_map(|entry| {
                    let target = entry.target.as_property()?;
                    if target.property_type() != property {
                        return None;
                    }
                    let track = entry.animator.keyframe_track()?;
                    let keys = track.keyframes();
                    (keys.len() == 3
                        && keys.iter().zip(values).all(|(key, expected)| {
                            float_value(key.value())
                                .is_ok_and(|value| (value - expected).abs() < 1e-4)
                        }))
                    .then_some(track)
                })
                .unwrap_or_else(|| {
                    panic!(
                        "native {property:?} values {values:?} must remain editable: {:?}",
                        output.diagnostics
                    )
                });
            for (key, time) in track.keyframes().iter().zip(times) {
                assert_eq!(key.layer_time().as_millis(), time);
            }
            for key in &track.keyframes()[1..] {
                assert!(matches!(
                    (ease, key.easing()),
                    ("hold", PropertyKeyframeEasing::Hold)
                        | ("cubic", PropertyKeyframeEasing::CubicBezier { .. })
                ));
            }
        }
    }
}

#[test]
fn paired_scale_constant_depth_shares_hold_and_cubic_interpolation() {
    // Supplementary construction regression. The native Scale panel separately
    // proves Adobe controls; a constant Z axis must not reject editable XY keys.
    for easing in [
        PropertyKeyframeEasing::Hold,
        PropertyKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 0.7,
            y2: 0.9,
        },
    ] {
        let x = scalar_keys(&[
            (0, 40.0, PropertyKeyframeEasing::Linear),
            (400, 80.0, easing),
        ]);
        let track = paired_track(
            Some(NativeTrack::Keyframes(&x)),
            None,
            [100.0, 75.0],
            100.0,
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(track.keys.len(), 2);
        assert_eq!(track.keys[0].values, [0.4, 0.75, 1.0]);
        assert_eq!(track.keys[1].values, [0.8, 0.75, 1.0]);
        let expected = std::mem::discriminant(&native_easing(easing));
        assert!(
            track.keys[1]
                .easing
                .iter()
                .all(|ease| std::mem::discriminant(ease) == expected),
            "native Scale interpolation is shared by XYZ: {:?}",
            track.keys[1].easing
        );
    }
}

#[test]
fn paired_scale_native_independent_knots_preserve_separated_position() {
    let source = native_scale_oracle();
    let mut input = to_structural_fx_document(&source, Some(34))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    // The independent Adobe source contains the exact union of these Scale
    // curves. Remove only redundant component knots to exercise editable FX's
    // independent axes, retaining the native separated Position animation.
    for entry in input["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
    {
        let keep: &[i64] = match entry["target"]["propertyType"].as_str() {
            Some("scaleX") => &[200, 1100],
            Some("scaleY") => &[550, 1550],
            _ => continue,
        };
        if let Some(keys) = entry["animator"]
            .get_mut("keyframes")
            .and_then(Value::as_array_mut)
        {
            keys.retain(|key| keep.contains(&key["layerTime"].as_i64().unwrap()));
        }
    }
    let output = export(input);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("omitted, convertible siblings")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let restored = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document;
    for (property, expected) in [
        (PropType::ScaleX, [63.0, 97.22222222222223, 151.0, 151.0]),
        (PropType::ScaleY, [139.0, 139.0, 94.45, 58.0]),
    ] {
        let track = restored
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find_map(|entry| {
                let target = entry.target.as_property()?;
                if target.property_type() != property {
                    return None;
                }
                let track = entry.animator.keyframe_track()?;
                (track.keyframes().len() == 4).then_some(track)
            })
            .expect("independent native Scale stays editable on separated Position owner");
        for ((key, expected), time) in track
            .keyframes()
            .iter()
            .zip(expected)
            .zip([200, 550, 1100, 1550])
        {
            assert_eq!(key.layer_time().as_millis(), time);
            assert!((float_value(key.value()).unwrap() - expected).abs() < 1e-4);
        }
    }
}

#[test]
fn paired_scale_rejects_simultaneous_hold_and_continuous_motion() {
    let x = scalar_keys(&[
        (0, 40.0, PropertyKeyframeEasing::Linear),
        (400, 80.0, PropertyKeyframeEasing::Hold),
    ]);
    let y = scalar_keys(&[
        (0, 100.0, PropertyKeyframeEasing::Linear),
        (400, 50.0, PropertyKeyframeEasing::Linear),
    ]);
    let error = paired_track(
        Some(NativeTrack::Keyframes(&x)),
        Some(NativeTrack::Keyframes(&y)),
        [100.0; 2],
        100.0,
        false,
    )
    .unwrap_err();
    assert!(error.contains("mixed simultaneous Hold/continuous"));
}

#[test]
fn paired_scale_splits_cubic_at_other_axis_knots() {
    let x = scalar_keys(&[
        (0, 0.0, PropertyKeyframeEasing::Linear),
        (
            1000,
            100.0,
            PropertyKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: 0.0,
                x2: 0.75,
                y2: 1.0,
            },
        ),
    ]);
    let y = scalar_keys(&[
        (0, 100.0, PropertyKeyframeEasing::Linear),
        (500, 150.0, PropertyKeyframeEasing::Linear),
        (1000, 100.0, PropertyKeyframeEasing::Linear),
    ]);
    let merged = paired_track(
        Some(NativeTrack::Keyframes(&x)),
        Some(NativeTrack::Keyframes(&y)),
        [100.0; 2],
        100.0,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(merged.keys.len(), 3);
    assert!((merged.keys[1].values[0] - 0.5).abs() < 1e-12);
    assert_eq!(merged.keys[1].values[1..], [1.5, 1.0]);
    for key in &merged.keys[1..] {
        assert!(
            key.easing
                .iter()
                .all(|easing| matches!(easing, KeyframeEasing::CubicBezier { .. }))
        );
        assert_eq!(
            key.easing[1],
            KeyframeEasing::CubicBezier {
                x1: 1.0 / 3.0,
                y1: 1.0 / 3.0,
                x2: 2.0 / 3.0,
                y2: 2.0 / 3.0
            }
        );
    }
}

#[test]
fn paired_scale_unions_independent_knots_without_frame_sampling() {
    let x = scalar_keys(&[
        (0, 40.0, PropertyKeyframeEasing::Linear),
        (400, 80.0, PropertyKeyframeEasing::Linear),
    ]);
    let y = scalar_keys(&[
        (0, 100.0, PropertyKeyframeEasing::Linear),
        (300, 100.0, PropertyKeyframeEasing::Linear),
        (700, 0.0, PropertyKeyframeEasing::Linear),
    ]);
    let track = paired_track(
        Some(NativeTrack::Keyframes(&x)),
        Some(NativeTrack::Keyframes(&y)),
        [100.0; 2],
        100.0,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        track
            .keys
            .iter()
            .map(|key| key.time_millis)
            .collect::<Vec<_>>(),
        [0, 300, 400, 700]
    );
    for (key, expected) in track.keys.iter().zip([
        [0.4, 1.0, 1.0],
        [0.7, 1.0, 1.0],
        [0.8, 0.75, 1.0],
        [0.8, 0.0, 1.0],
    ]) {
        for (actual, expected) in key.values.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-12);
        }
    }
}
