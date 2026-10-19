use super::*;

fn ramp(values: [f64; 2], easing: KeyframeEasing) -> NumericTrack {
    NumericTrack {
        keys: values
            .into_iter()
            .enumerate()
            .map(|(index, value)| NumericKeyframe {
                time_millis: index as i64 * 1000,
                values: vec![value],
                easing: vec![easing],
                spatial_in: vec![],
                spatial_out: vec![],
            })
            .collect(),
    }
}

#[test]
fn linear_crossings_keep_unsaturated_ramp_and_saturated_plateau() {
    for values in [[0.0, 1.0], [1.0, 0.0]] {
        let output = clipped(&ramp(values, KeyframeEasing::Linear), 2.0).unwrap();
        assert_eq!(output.keys.len(), 3);
        assert_eq!(output.keys[1].time_millis, 500);
        assert_eq!(output.keys[1].values, [1.0]);
        let plateau = if values[0] == 0.0 { 2 } else { 1 };
        assert_eq!(output.keys[plateau].easing, [KeyframeEasing::Hold]);
    }
}

#[test]
fn hold_steps_do_not_grow_a_continuous_crossing() {
    let output = clipped(&ramp([0.0, 1.0], KeyframeEasing::Hold), 100.0).unwrap();
    assert_eq!(output.keys.len(), 2);
    assert_eq!(output.keys[1].values, [1.0]);
    assert_eq!(output.keys[1].easing, [KeyframeEasing::Hold]);
}

#[test]
fn unsupported_curves_and_unrepresentable_crossings_retain_fallback() {
    let nonmonotone = KeyframeEasing::CubicBezier {
        x1: 0.3,
        y1: 1.5,
        x2: 0.6,
        y2: 0.5,
    };
    assert!(clipped(&ramp([0.0, 1.0], nonmonotone), 100.0).is_none());
    assert!(clipped(&ramp([0.0, 1.0], KeyframeEasing::Linear), 10000.0).is_none());
    assert!(clipped(&ramp([-1.0, 1.0], KeyframeEasing::Linear), 100.0).is_none());
}
