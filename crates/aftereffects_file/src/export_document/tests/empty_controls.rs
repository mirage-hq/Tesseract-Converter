use super::*;

#[test]
fn empty_controls_with_group_fills_are_not_null_eligible() {
    let value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    group["layers"] = json!([]);
    group["effects"] = json!([]);
    group["masks"] = json!([]);
    group["fills"] = json!([]);
    group["trackMatte"] = Value::Null;
    let empty: Layer = serde_json::from_value(group.clone()).unwrap();
    let LayerData::Group(empty) = empty.data() else {
        panic!("group")
    };
    assert!(hierarchy::empty_controls_eligible(empty));

    group["fills"] = json!([
        {"paint":{"type":"solid","color":[0.2,0.3,0.4,1.0]},"opacity":1.0}
    ]);
    let painted: Layer = serde_json::from_value(group).unwrap();
    let LayerData::Group(painted) = painted.data() else {
        panic!("group")
    };
    assert!(!hierarchy::empty_controls_eligible(painted));
}

#[test]
fn empty_controls_affine_windows_keep_native_null_and_visible_sibling() {
    for (start, duration, source_start, source_duration, offset, hidden) in [
        (69_866, 500, 0, 500, 0, true),
        (500, 1_000, 250, 500, 100, false),
        (0, 30_000, 0, 30_000, 0, false),
    ] {
        let mut value = imported();
        value["duration"] = json!(72.0);
        let mut group = value["composition"]["layers"][0].clone();
        group["id"] = json!(2_900_000);
        group["name"] = json!("Empty controls");
        group["parent"] = Value::Null;
        group["layers"] = json!([]);
        group["effects"] = json!([]);
        group["masks"] = json!([]);
        group["fills"] = json!([]);
        group["trackMatte"] = Value::Null;
        group["isHidden"] = json!(hidden);
        let input = json!({"start": start, "duration": duration});
        group["playback"] = fixture_linear_playback(
            input,
            json!({"start": source_start, "duration": source_duration}),
        );
        group["playback"]["inputOffsetMs"] = json!(offset);
        let sibling = rect(&value, 9_901);
        value["composition"]["layers"] = json!([group, sibling]);
        value["composition"]["dynamics"]["entries"] = json!([
            keyed_entry(
                LayerId::new(2_900_000),
                PropType::Opacity,
                [
                    (0, PropertyValue::Float(25.0)),
                    (500, PropertyValue::Float(75.0)),
                ]
            ),
            keyed_entry(
                LayerId::new(2_900_000),
                PropType::PositionX,
                [
                    (0, PropertyValue::Float(10.0)),
                    (500, PropertyValue::Float(30.0)),
                ]
            ),
        ]);
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        let controls = layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == "Empty controls")
            .unwrap_or_else(|| panic!("empty controls omitted: {:?}", output.diagnostics));
        assert!(controls.record.flags().null_layer);
        for (name, values) in [
            ("ADBE Opacity", [0.25, 0.75]),
            ("ADBE Position", [10.0, 30.0]),
        ] {
            let property = super::stroke_keys::numeric(&controls.content, name).unwrap();
            assert_eq!(property.keyframes.len(), 2, "{name}");
            for (key, (time, value)) in property
                .keyframes
                .iter()
                .zip([0, 500].into_iter().zip(values))
            {
                let expected = source_start + (offset + time) * source_duration / duration;
                assert!(
                    (key.time_secs - expected as f64 / 1_000.0).abs() < 0.0001,
                    "{name}: {:?}",
                    key
                );
                assert_eq!(key.values[0], value, "{name}");
            }
        }
        assert_eq!(
            controls.record.in_point_fraction(),
            reduced(source_start + offset * source_duration / duration, 1_000)
        );
        assert_eq!(
            controls.record.out_point_fraction(),
            reduced(
                source_start + offset * source_duration / duration + source_duration,
                1_000
            )
        );
        assert_eq!(
            controls.record.stretch_fraction(),
            reduced(duration, source_duration)
        );
        // Offset is in the input domain; the native source starts earlier by
        // source_start / playback rate plus that input offset.
        assert_eq!(
            controls.record.start_time_fraction(),
            reduced(
                start - source_start * duration / source_duration - offset,
                1_000
            )
        );
        assert!(
            layers(&native)
                .iter()
                .any(|layer| layer.name.as_ref() == "Current solid 9901")
        );
    }
}

fn reduced(mut numerator: i64, mut denominator: i64) -> (i32, u32) {
    let (mut a, mut b) = (numerator.abs(), denominator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    numerator /= a;
    denominator /= a;
    (
        numerator.try_into().unwrap(),
        denominator.try_into().unwrap(),
    )
}

#[test]
fn empty_controls_cannot_replace_an_incoming_matte_source() {
    let mut value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(2_900_000);
    group["parent"] = Value::Null;
    group["layers"] = json!([]);
    group["effects"] = json!([]);
    group["masks"] = json!([]);
    group["fills"] = json!([]);
    group["trackMatte"] = Value::Null;
    let mut dependent = rect(&value, 9_900);
    dependent["trackMatte"] = json!({"mode":"alpha", "layer":2_900_000});
    let sibling = rect(&value, 9_901);
    value["composition"]["layers"] = json!([group, dependent, sibling]);
    value["composition"]["dynamics"]["entries"] = json!([]);
    let output = export(value);
    assert!(output.omitted_layer_ids.contains(&LayerId::new(2_900_000)));
    assert!(output.omitted_layer_ids.contains(&LayerId::new(9_900)));
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1);
    assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 9901");
}
