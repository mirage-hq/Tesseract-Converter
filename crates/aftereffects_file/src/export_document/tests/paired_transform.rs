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
fn separated_planar_group_keeps_null_parent_and_child_blends() {
    let mut value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(8100);
    group["name"] = json!("Planar controls");
    group["parent"] = Value::Null;
    group["effects"] = json!([]);
    group["masks"] = json!([]);
    group["fills"] = json!([]);
    group["trackMatte"] = Value::Null;
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    group["playback"] = fixture_linear_playback(
        json!({"start":0,"duration":30000}),
        json!({"start":0,"duration":30000}),
    );
    let mut screen = rect(&value, 8101);
    screen["parent"] = json!(8100);
    screen["blendMode"] = json!("screen");
    let mut normal = rect(&value, 8102);
    normal["parent"] = json!(8100);
    group["layers"] = json!([screen, normal]);
    value["duration"] = json!(30.0);
    value["composition"]["layers"] = json!([group]);
    // Independent axis knots and different interpolation require separated
    // Position followers, but do not make this a projected 3D Group.
    let mut y = keyed_entry(
        LayerId::new(8100),
        PropType::PositionY,
        [
            (0, PropertyValue::Float(20.0)),
            (700, PropertyValue::Float(40.0)),
        ],
    );
    y.animator = fx_schema::animator::PropertyAnimator::keyframes(scalar_keys(&[
        (0, 20.0, PropertyKeyframeEasing::Linear),
        (700, 40.0, PropertyKeyframeEasing::Hold),
    ]));
    value["composition"]["dynamics"]["entries"] = json!([
        keyed_entry(
            LayerId::new(8100),
            PropType::PositionX,
            [
                (0, PropertyValue::Float(10.0)),
                (500, PropertyValue::Float(30.0))
            ],
        ),
        y,
    ]);
    let output = export(value);
    assert!(
        output.omitted_layer_ids.is_empty(),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let controls = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Planar controls")
        .unwrap();
    assert!(
        controls.record.flags().null_layer,
        "2D keys must not isolate child blending"
    );
    assert!(!controls.record.flags().three_d_layer);
    for (id, blend) in [(8101, 6), (8102, 2)] {
        let child = layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == format!("Current solid {id}"))
            .unwrap();
        assert_eq!(child.record.parent_id(), controls.record.id());
        assert_eq!(child.record.blend_mode(), blend);
    }
    for (name, times, values) in [
        ("ADBE Position_0", [0.0, 0.5], [10.0, 30.0]),
        ("ADBE Position_1", [0.0, 0.7], [20.0, 40.0]),
    ] {
        let property = super::stroke_keys::numeric(&controls.content, name).unwrap();
        assert_eq!(property.keyframes.len(), 2, "{name}");
        for ((key, time), value) in property.keyframes.iter().zip(times).zip(values) {
            assert!((key.time_secs - time).abs() < 1e-4);
            assert_eq!(key.values, [value]);
        }
    }
}

#[test]
fn separated_planar_group_opacity_requires_a_composite_source() {
    for animated in [false, true] {
        let mut value = imported();
        let mut group = value["composition"]["layers"][0].clone();
        group["id"] = json!(8100);
        group["name"] = json!("Composite controls");
        group["parent"] = Value::Null;
        group["effects"] = json!([]);
        group["masks"] = json!([]);
        group["fills"] = json!([]);
        group["trackMatte"] = Value::Null;
        group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
        group["transform"]["opacity"] = json!(if animated { 100.0 } else { 37.5 });
        group["playback"] = fixture_linear_playback(
            json!({"start":0,"duration":30000}),
            json!({"start":0,"duration":30000}),
        );
        let mut first = rect(&value, 8101);
        first["parent"] = json!(8100);
        let mut second = rect(&value, 8102);
        second["parent"] = json!(8100);
        group["layers"] = json!([first, second]);
        value["duration"] = json!(30.0);
        value["composition"]["layers"] = json!([group]);
        let mut entries = vec![
            keyed_entry(
                LayerId::new(8100),
                PropType::PositionX,
                [
                    (0, PropertyValue::Float(10.0)),
                    (500, PropertyValue::Float(30.0)),
                ],
            ),
            keyed_entry(
                LayerId::new(8100),
                PropType::PositionY,
                [
                    (0, PropertyValue::Float(20.0)),
                    (700, PropertyValue::Float(40.0)),
                ],
            ),
        ];
        if animated {
            entries.push(keyed_entry(
                LayerId::new(8100),
                PropType::Opacity,
                [
                    (0, PropertyValue::Float(0.0)),
                    (1000, PropertyValue::Float(100.0)),
                ],
            ));
        }
        value["composition"]["dynamics"]["entries"] = json!(entries);
        let output = export(value);
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let controls = layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == "Composite controls")
            .unwrap();
        assert!(
            !controls.record.flags().null_layer,
            "Null opacity cannot gate children"
        );
        let ItemKind::Composition(source) = &native.item(controls.record.source_id()).unwrap().kind
        else {
            panic!("composite source");
        };
        assert_eq!(source.layers.len(), 2);
        let opacity = super::stroke_keys::numeric(&controls.content, "ADBE Opacity").unwrap();
        if animated {
            assert_eq!(opacity.keyframes.len(), 2);
            for ((key, time), value) in opacity.keyframes.iter().zip([0.0, 1.0]).zip([0.0, 1.0]) {
                assert!((key.time_secs - time).abs() < 1e-4);
                assert_eq!(key.values, [value]);
            }
        } else {
            assert_eq!(opacity.values, [0.375]);
        }
        for name in ["ADBE Position_0", "ADBE Position_1"] {
            assert_eq!(
                super::stroke_keys::numeric(&controls.content, name)
                    .unwrap()
                    .keyframes
                    .len(),
                2
            );
        }
    }
}

#[test]
fn root_image_composite_applies_geometry_before_rasterization() {
    assert_root_image_composite(30000, false);
}

#[test]
fn shorter_root_image_composite_applies_geometry_before_rasterization() {
    assert_root_image_composite(15000, false);
}

#[test]
fn shorter_explicitly_parented_root_image_composite_applies_geometry_before_rasterization() {
    assert_root_image_composite(15000, true);
}

fn assert_root_image_composite(duration_ms: u64, explicit_parent: bool) {
    use crate::writer::footage::{NativeFrameRate, NativeSourceFormat, RelativeMediaPath};
    use std::collections::BTreeMap;

    let mut value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(8100);
    group["name"] = json!("Image composite");
    group["parent"] = Value::Null;
    group["effects"] = json!([]);
    group["masks"] = json!([]);
    group["fills"] = json!([]);
    group["trackMatte"] = Value::Null;
    group["motionBlur"] = json!(false);
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    group["transform"]["scale"] = json!([53.0, 128.0]);
    group["transform"]["opacity"] = json!(37.5);
    group["playback"] = fixture_linear_playback(
        json!({"start":0,"duration":duration_ms}),
        json!({"start":0,"duration":duration_ms}),
    );
    group["layers"] = json!([{
        "type":"Image", "id":8101, "name":"Editable image",
        "parent": explicit_parent.then_some(8100),
        "activeRange":{"start":0,"duration":duration_ms},
        "source":{"assetId":"image", "fit":"contain"},
        "transform":identity_fx_transform()
    }]);
    value["duration"] = json!(30.0);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"]["entries"] = json!([
        keyed_entry(
            LayerId::new(8100),
            PropType::PositionX,
            [
                (0, PropertyValue::Float(10.0)),
                (500, PropertyValue::Float(30.0))
            ]
        ),
        keyed_entry(
            LayerId::new(8100),
            PropType::PositionY,
            [
                (0, PropertyValue::Float(20.0)),
                (700, PropertyValue::Float(40.0))
            ]
        ),
    ]);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let source = media::ResolvedMediaSource {
        asset_id: fx_schema::AssetId::new("image").unwrap(),
        path: RelativeMediaPath::new("media/image.png").unwrap(),
        native_duration: None,
        format: NativeSourceFormat::PngRgba,
        dimensions: [1024, 1024],
        duration_millis: 0,
        duration_millis_floor: 0,
        duration_native_ticks: None,
        frame_rate: NativeFrameRate::integer(0),
        audio_sample_rate: 0.0,
        wave_metadata: None,
    };
    let resolved = BTreeMap::from([("image".into(), source)]);
    let output = to_aep_with_media(&document, &resolved).unwrap();
    assert!(
        output.omitted_layer_ids.is_empty(),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let owner = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Image composite")
        .unwrap();
    let scale = super::stroke_keys::numeric(&owner.content, "ADBE Scale").unwrap();
    assert_eq!(
        scale.values,
        [1.0, 1.0, 1.0],
        "the composite must not resample transformed pixels"
    );
    let opacity = super::stroke_keys::numeric(&owner.content, "ADBE Opacity").unwrap();
    assert_eq!(opacity.values, [0.375]);
    let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind else {
        panic!("image composite source");
    };
    assert_eq!(source.record.dimensions(), (1920, 1080));
    let parent = source
        .layers
        .iter()
        .find(|layer| layer.record.flags().null_layer)
        .unwrap();
    let image = source
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Editable image")
        .unwrap();
    assert_eq!(image.record.parent_id(), parent.record.id());
    let scale = super::stroke_keys::numeric(&parent.content, "ADBE Scale").unwrap();
    assert_eq!(scale.values, [0.53, 1.28, 1.0]);
    for name in ["ADBE Position_0", "ADBE Position_1"] {
        assert_eq!(
            super::stroke_keys::numeric(&parent.content, name)
                .unwrap()
                .keyframes
                .len(),
            2
        );
    }
    let oracle = native_scale_oracle();
    let ItemKind::Composition(controls) = &oracle.item(17).unwrap().kind else {
        panic!("native scale oracle");
    };
    assert!(controls.layers.iter().any(|layer| {
        super::stroke_keys::numeric(&layer.content, "ADBE Scale").is_some_and(|scale| {
            scale
                .keyframes
                .iter()
                .any(|key| key.values.starts_with(&[0.53, 1.28]))
        })
    }));

    let mut adjusted = document.to_json_value().unwrap();
    adjusted["composition"]["layers"].as_array_mut().unwrap().insert(0, json!({
        "type":"Adjustment", "id":8200, "name":"Spatial consumer", "parent":null,
        "activeRange":{"start":0,"duration":30000},
        "effects":[{"id":8201,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":7}}],
        "transform":identity_fx_transform()
    }));
    let mut blurred = document.to_json_value().unwrap();
    blurred["composition"]["layers"][0]["layers"][0]["motionBlur"] = json!(true);
    let mut external = document.to_json_value().unwrap();
    let mut consumer = external["composition"]["layers"][0]["layers"][0].clone();
    consumer["id"] = json!(8400);
    consumer["parent"] = json!(8100);
    external["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(consumer);
    for input in [adjusted, blurred, external] {
        let input = EditableFxCompositionDocument::from_json_value(input).unwrap();
        let output = to_aep_with_media(&input, &resolved).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let owner = layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == "Image composite")
            .unwrap();
        assert_eq!(
            super::stroke_keys::numeric(&owner.content, "ADBE Scale")
                .unwrap()
                .values,
            [0.53, 1.28, 1.0],
            "spatial consumers and child motion blur must retain the source domain"
        );
    }
    let mut unrelated = document.to_json_value().unwrap();
    let mut sibling = unrelated["composition"]["layers"][0]["layers"][0].clone();
    sibling["id"] = json!(8300);
    sibling["parent"] = Value::Null;
    sibling["motionBlur"] = json!(true);
    unrelated["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(sibling);
    let unrelated = EditableFxCompositionDocument::from_json_value(unrelated).unwrap();
    let output = to_aep_with_media(&unrelated, &resolved).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let owner = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Image composite")
        .unwrap();
    assert_eq!(
        super::stroke_keys::numeric(&owner.content, "ADBE Scale")
            .unwrap()
            .values,
        [1.0, 1.0, 1.0],
        "unrelated sibling motion blur does not inherit the geometry parent"
    );
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
    // The oracle's final Hold boundary is below 1650ms on its native tick clock.
    for (property, times, values, ease) in [
        (
            PropType::ScaleX,
            [250, 1100, 1649],
            [38.0, 151.0, 82.0],
            "hold",
        ),
        (
            PropType::ScaleY,
            [250, 1100, 1649],
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
