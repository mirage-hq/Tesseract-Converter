use super::*;
use crate::properties::NumericValueKind;
use fx_schema::PropertyValue;

fn prepare(numeric: &NumericProperty) -> Result<Option<NumericProperty>, String> {
    super::prepare(numeric, NumericAnimationClock::source_local())
}

// Numeric controls read from vendor-original Bold02 SHA256
// 48dcb6d46c69977b63f5a62c1736017e5945a6b48567253c19c56296df9274f3,
// comp 1 / Circle 01 layer 92. The proprietary source/media are not fixtures.
// This is source-field/math regression evidence, not independent native readback.
fn circle() -> NumericProperty {
    let key = |time_secs, values, spatial_in, spatial_out, speed, influence| NumericKeyframe {
        time_secs,
        values,
        spatial_in,
        spatial_out,
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![0.],
        in_influence: vec![100.],
        out_speed: vec![speed],
        out_influence: vec![influence],
    };
    NumericProperty {
        values: vec![],
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        value_kind: NumericValueKind::Continuous,
        keyframes: vec![
            key(
                0.9342676009342676,
                vec![2031.9913330078125, 684.2991943359375, 0.],
                vec![0.; 3],
                vec![0.; 3],
                140.07258495294056,
                3.665670049075537,
            ),
            key(
                8.008008008008009,
                vec![2985.9913330078125, 444.2991943359375, 0.],
                vec![-671.9913330078125, -136.2991943359375, 0.],
                vec![671.9913330078125, 136.2991943359375, 0.],
                0.,
                33.333333,
            ),
        ],
    }
}

// Source fields decoded from AI SaaS SHA256
// 548a6b8849fcde4ea134140a212d7008722ddb832cbe13144fea29aa3ca71046,
// Scene04 comp11585 / Spark layer11591 / Position ldat at4294872.
// Native adjustment readback independently confirms values and composition
// times1001/1201/1502ms. Proprietary source/media are not published here.
fn spark() -> NumericProperty {
    let mut numeric = circle();
    let key = |time_secs, values, spatial_in, spatial_out| NumericKeyframe {
        time_secs,
        values,
        spatial_in,
        spatial_out,
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![0.],
        in_influence: vec![33.333332999999996],
        out_speed: vec![0.],
        out_influence: vec![33.333332999999996],
    };
    numeric.keyframes = vec![
        key(
            0.9676343009676343,
            vec![1917.2367544380443, 1148.0040867877206, 0.],
            vec![0.; 3],
            vec![0.; 3],
        ),
        key(
            1.1678345011678346,
            vec![1930.746797460159, 374.01424042084784, 0.],
            vec![3.070575444773567, -500.618299144009, 0.],
            vec![-3.8318760572201427, 624.7386878447886, 0.],
        ),
        key(
            1.4681348014681348,
            vec![1893.5298321702312, 2506.172753894125, 0.],
            vec![14.052139282226562, -778.7295227050781, 0.],
            vec![1594.2169647216797, 213.17420959472656, 0.],
        ),
    ];
    numeric.keyframes[2].out_influence = vec![35.330571970925284];
    numeric
}

#[test]
fn ai_spark_submillisecond_refinement_keeps_both_position_tracks() {
    use super::super::{NumericAnimationClock, NumericAnimationTarget, numeric_entries};
    use crate::structure_document::animation_budget::AnimationBudget;
    use fx_schema::{AnimationGraph, LayerId, PropType, PropertyTarget};
    let targets = [PropType::PositionX, PropType::PositionY]
        .into_iter()
        .enumerate()
        .map(|(axis, property)| {
            NumericAnimationTarget::float(
                PropertyTarget::layer(LayerId::new(11591), property),
                axis,
                1.,
            )
        })
        .collect::<Vec<_>>();
    for clock in [
        NumericAnimationClock::ParentIdentity {
            start: 0.033366700033366704,
            stretch: 1.,
        },
        NumericAnimationClock::ParentIdentity {
            start: -0.0123456789,
            stretch: 0.5,
        },
        NumericAnimationClock::ParentIdentity {
            start: 0.0123456789,
            stretch: -0.5,
        },
        NumericAnimationClock::source_local_rebased(2.0123456789),
    ] {
        let (entries, warnings) = numeric_entries(
            "ADBE Position",
            &spark(),
            &targets,
            clock,
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries.len(), 2, "{clock:?}: {warnings:?}");
        for (axis, entry) in entries.iter().enumerate() {
            let keys = entry.animator.keyframe_track().unwrap().keyframes();
            let (first, last) = if clock.reversed() { (2, 0) } else { (0, 2) };
            assert_eq!(
                keys[0].value(),
                &PropertyValue::Float(spark().keyframes[first].values[axis])
            );
            assert_eq!(
                keys.last().unwrap().value(),
                &PropertyValue::Float(spark().keyframes[last].values[axis])
            );
            assert!(
                keys.windows(2)
                    .all(|pair| pair[0].layer_time() < pair[1].layer_time())
            );
        }
        AnimationGraph::from_entries(entries).unwrap();
    }
}

fn evaluate_linear(keys: &[NumericKeyframe], time: f64) -> Vec<f64> {
    let pair = keys
        .windows(2)
        .find(|pair| pair[0].time_secs <= time && time <= pair[1].time_secs)
        .unwrap();
    let t = (time - pair[0].time_secs) / (pair[1].time_secs - pair[0].time_secs);
    pair[0]
        .values
        .iter()
        .zip(&pair[1].values)
        .map(|(a, b)| a + (b - a) * t)
        .collect()
}

#[test]
fn bold02_shared_path_speed_reaches_source_math_position_not_signed_axis_ease() {
    let numeric = circle();
    let prepared = prepare(&numeric).unwrap().unwrap();
    // Independent 10,000-chord numerical diagnostic; the unchanged native MP4
    // circle center is approximately (2440,444) at 1.75s, not old (2228,564).
    let value = evaluate_linear(&prepared.keyframes, 1.75);
    assert!(distance(&value, &[2436.21, 446.90, 0.]) < 0.3, "{value:?}");
    assert_eq!(prepared.keyframes[0].values, numeric.keyframes[0].values);
    assert_eq!(
        prepared.keyframes.last().unwrap().values,
        numeric.keyframes[1].values
    );
    assert!(prepared.keyframes.len() > 2 && prepared.keyframes.len() < 150);
    assert!(
        prepared
            .keyframes
            .iter()
            .all(|key| key.spatial_in.is_empty()
                && key.spatial_out.is_empty()
                && key.in_interpolation == 1
                && key.out_interpolation == 1)
    );
}

#[test]
fn bold02_import_emits_coupled_editable_adaptive_position_tracks() {
    use super::super::{NumericAnimationClock, NumericAnimationTarget, numeric_entries};
    use crate::structure_document::animation_budget::AnimationBudget;
    use fx_schema::{AnimationGraph, LayerId, PropType, PropertyTarget};
    let targets = [PropType::PositionX, PropType::PositionY]
        .into_iter()
        .enumerate()
        .map(|(axis, property)| {
            NumericAnimationTarget::float(
                PropertyTarget::layer(LayerId::new(92), property),
                axis,
                1.,
            )
        })
        .collect::<Vec<_>>();
    for clock in [
        NumericAnimationClock::source_local(),
        NumericAnimationClock::source_local_rebased(2.),
    ] {
        let (entries, warnings) = numeric_entries(
            "ADBE Position",
            &circle(),
            &targets,
            clock,
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries.len(), 2, "{warnings:?}");
        assert!(
            entries
                .iter()
                .all(|entry| entry.animator.keyframe_track().unwrap().keyframes().len() > 2)
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("shared path-speed"))
        );
        AnimationGraph::from_entries(entries).unwrap();
    }
}

#[test]
fn collinear_nonuniform_curve_uses_distance_not_parameter() {
    let mut numeric = circle();
    numeric.keyframes[0].time_secs = 0.;
    numeric.keyframes[0].values = vec![0., 0., 0.];
    numeric.keyframes[0].out_interpolation = 1;
    numeric.keyframes[1].time_secs = 1.;
    numeric.keyframes[1].values = vec![100., 0., 0.];
    numeric.keyframes[1].spatial_in = vec![-100., 0., 0.];
    numeric.keyframes[1].in_interpolation = 1;
    let prepared = prepare(&numeric).unwrap().unwrap();
    assert!(distance(&evaluate_linear(&prepared.keyframes, 0.5), &[50., 0., 0.]) < 0.01);
}

#[test]
fn unsupported_distance_ease_is_diagnosed_and_hold_remains_discrete() {
    let mut numeric = circle();
    numeric.keyframes[0].out_speed = vec![1e9];
    assert!(prepare(&numeric).unwrap_err().contains("nonmonotone"));
    numeric.keyframes[0].out_interpolation = 3;
    let prepared = prepare(&numeric).unwrap().unwrap();
    assert_eq!(prepared.keyframes.len(), 2);
    assert_eq!(prepared.keyframes[0].out_interpolation, 3);
}

#[test]
fn total_work_limit_is_shared_and_checked_before_generated_allocations() {
    let mut remaining = 8;
    let mut table = vec![];
    subdivide_curve(&[[0., 1., 2., 3.]], 0., 1., 0, &mut table, &mut remaining).unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(table.len(), 1);
    let numeric = circle();
    let mut output = vec![];
    let error = fit_interval(
        &numeric.keyframes[0],
        &numeric.keyframes[1],
        &|_| panic!("work exhausted before evaluation"),
        NumericAnimationClock::source_local(),
        0.,
        1.,
        vec![0.; 3],
        vec![1.; 3],
        0,
        &mut output,
        &mut remaining,
    )
    .unwrap_err();
    assert!(error.contains("total refinement work"));
    assert!(output.is_empty());
    let error = subdivide_curve(
        &[[0., 1e8, -1e8, 1.]],
        0.,
        1.,
        0,
        &mut table,
        &mut remaining,
    )
    .unwrap_err();
    assert!(error.contains("total refinement work"));
    assert_eq!(table.len(), 1);
}

#[test]
fn finite_extreme_curve_returns_refinement_diagnostic_without_unbounded_generation() {
    let mut numeric = circle();
    numeric.keyframes[0].spatial_out = vec![1e8, -1e8, 0.];
    let error = prepare(&numeric).unwrap_err();
    assert!(error.contains("refinement"), "{error}");
}

#[test]
fn adjacent_target_milliseconds_stop_without_inventing_submillisecond_keys() {
    let mut numeric = circle();
    numeric.keyframes[0].time_secs = 0.0001;
    numeric.keyframes[1].time_secs = 0.0007;
    let prepared = prepare(&numeric).unwrap().unwrap();
    assert_eq!(prepared.keyframes.len(), 2);
    assert_eq!(prepared.keyframes[0].values, numeric.keyframes[0].values);
    assert_eq!(prepared.keyframes[1].values, numeric.keyframes[1].values);
    for stretch in [0.5, -0.5] {
        let clock = NumericAnimationClock::ParentIdentity { start: 0., stretch };
        assert!(
            super::prepare(&numeric, clock)
                .unwrap_err()
                .contains("endpoints collide")
        );
    }
}

#[test]
fn reversed_hold_retains_authored_discrete_endpoints() {
    let mut numeric = spark();
    for key in &mut numeric.keyframes {
        key.out_interpolation = 3;
    }
    let prepared = super::prepare(
        &numeric,
        NumericAnimationClock::ParentIdentity {
            start: -2.12345,
            stretch: -0.5,
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(prepared.keyframes.len(), numeric.keyframes.len());
    for (actual, source) in prepared.keyframes.iter().zip(&numeric.keyframes) {
        assert_eq!(actual.values, source.values);
        assert_eq!(actual.time_secs, source.time_secs);
    }
    assert!(
        prepared.keyframes[..2]
            .iter()
            .all(|key| key.out_interpolation == 3)
    );
}

#[test]
fn target_clock_range_and_inverse_cancellation_are_diagnosed() {
    let clock = NumericAnimationClock::ParentIdentity {
        start: 1e100,
        stretch: 1.,
    };
    assert!(
        super::prepare(&spark(), clock)
            .unwrap_err()
            .contains("fitting range")
    );
    let numeric = spark();
    let clock = NumericAnimationClock::source_local_rebased(1e16);
    assert!(
        source_unit(1, &numeric.keyframes[0], &numeric.keyframes[1], clock)
            .unwrap_err()
            .contains("round-trip")
    );
}

#[test]
fn fitted_endpoints_preserve_authored_times_despite_cancellation() {
    let mut numeric = circle();
    for key in &mut numeric.keyframes {
        key.in_interpolation = 1;
        key.out_interpolation = 1;
        for value in key
            .values
            .iter_mut()
            .chain(&mut key.spatial_in)
            .chain(&mut key.spatial_out)
        {
            *value *= 0.0001;
        }
    }
    numeric.keyframes[0].time_secs = -1_000_000.;
    numeric.keyframes[1].time_secs = 0.00050000002;
    for clock in [
        NumericAnimationClock::source_local(),
        NumericAnimationClock::ParentIdentity {
            start: 0.00025,
            stretch: 0.5,
        },
        NumericAnimationClock::ParentIdentity {
            start: 0.00025,
            stretch: -0.5,
        },
    ] {
        let prepared = super::prepare(&numeric, clock).unwrap().unwrap();
        assert_eq!(
            prepared.keyframes.first().unwrap().time_secs,
            numeric.keyframes[0].time_secs
        );
        assert_eq!(
            prepared.keyframes.last().unwrap().time_secs,
            numeric.keyframes[1].time_secs
        );
        assert_eq!(
            super::target_millis(prepared.keyframes.last().unwrap(), clock).unwrap(),
            super::target_millis(&numeric.keyframes[1], clock).unwrap()
        );
    }
}

// Native Position key fields from selected Infinity source SHA256
// 1373d29f81469e8d5de5e0af38079814ef7f01a77d976cf9a1e9bf8734d43005,
// comp 1648 / layers 1652 and 1655. The two tracks have distinct geometry.
// Source-field/math evidence only; the proprietary AEP is not redistributed.
fn infinity_position(layer_id: u32) -> NumericProperty {
    let positions = match layer_id {
        1652 => [
            (0., 1925.1090000000002, 1143.0617064241842, 0.),
            (
                0.9333333333333333,
                1674.2003333333332,
                1345.3990491154477,
                0.,
            ),
            (
                1.2166666666666666,
                1674.2003333333332,
                1345.3990491154477,
                0.,
            ),
            (2.08046875, 3239.520318074795, 1611.1110478638236, 0.),
            (2.35, 2678.520333333584, 1611.1110478638236, 0.),
            (
                3.283333333333333,
                637.2958475349019,
                1006.3643604385038,
                0.8878000723556708,
            ),
            (
                3.552864583333333,
                637.2958475349019,
                1006.3643604385038,
                0.8878000723556708,
            ),
            (
                3.9833333333333334,
                1925.1090000000002,
                1143.0617064241842,
                0.,
            ),
        ],
        1655 => [
            (0., 1925.1090000000002, 1143.0617064241842, 0.),
            (0.9333333333333333, 2301.472, 772.0557157821145, 0.),
            (1.2166666666666666, 2301.472, 772.0557157821145, 0.),
            (2.08046875, 3272.049984741313, 550.621699271577, 0.),
            (2.35, 3272.049984741313, 943.6216840127879, 0.),
            (
                3.283333333333333,
                1496.1169548199473,
                466.37149583984296,
                0.8877976047338578,
            ),
            (
                3.552864583333333,
                1496.1169548199473,
                466.37149583984296,
                0.8877976047338578,
            ),
            (
                3.9833333333333334,
                1925.1090000000002,
                1143.0617064241842,
                0.,
            ),
        ],
        _ => panic!("unexpected native Infinity layer {layer_id}"),
    };
    NumericProperty {
        keyframes: positions
            .into_iter()
            .map(|(time_secs, x, y, incoming_x)| NumericKeyframe {
                time_secs,
                values: vec![x, y, 0.],
                spatial_in: vec![incoming_x, 0., 0.],
                spatial_out: vec![0.; 3],
                in_interpolation: 2,
                out_interpolation: 2,
                in_speed: vec![0.],
                out_speed: vec![0.],
                in_influence: vec![78.],
                out_influence: vec![78.],
            })
            .collect(),
        ..circle()
    }
}

fn assert_infinity_position_restored(layer_id: u32) {
    use super::super::{NumericAnimationTarget, numeric_entries};
    use crate::structure_document::animation_budget::AnimationBudget;
    use fx_schema::{AnimationGraph, LayerId, PropType, PropertyTarget};

    let source = infinity_position(layer_id);
    let prepared = prepare(&source)
        .expect("native mixed straight/curved track must fit without a larger work budget")
        .unwrap();
    // Independent scalar-distance Bezier inversion at addressable 233ms,
    // with native handles (0.78, 0, 0.22, 1). This detects cubic(progress)
    // double-easing on the straight span, including opposite signed axes.
    let expected = if layer_id == 1652 {
        [1912.5732592069041, 1153.170757316839, 0.]
    } else {
        [1943.912611189644, 1124.5257387624797, 0.]
    };
    assert!(distance(&evaluate_linear(&prepared.keyframes, 0.233), &expected) < 0.25);
    for authored in &source.keyframes {
        let retained = prepared
            .keyframes
            .iter()
            .find(|key| (key.time_secs - authored.time_secs).abs() < 1e-12)
            .expect("every authored endpoint must remain");
        assert_eq!(retained.values, authored.values);
    }
    // Same endpoints do not make this span straight: its incoming handle
    // creates a small out-and-back excursion that must survive the fast path.
    let loop_mid = (source.keyframes[5].time_secs + source.keyframes[6].time_secs) / 2.;
    assert!(
        evaluate_linear(&prepared.keyframes, loop_mid)[0] > source.keyframes[5].values[0] + 0.1
    );

    let targets = [PropType::PositionX, PropType::PositionY]
        .into_iter()
        .enumerate()
        .map(|(axis, property)| {
            NumericAnimationTarget::float(
                PropertyTarget::layer(LayerId::new(u64::from(layer_id)), property),
                axis,
                1.,
            )
        })
        .collect::<Vec<_>>();
    let (entries, warnings) = numeric_entries(
        "ADBE Position",
        &source,
        &targets,
        NumericAnimationClock::source_local(),
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 2, "native layer {layer_id}: {warnings:?}");
    AnimationGraph::from_entries(entries).unwrap();
}

#[test]
fn infinity_position_image03_mixed_segments_fit_without_budget_inflation() {
    assert_infinity_position_restored(1652);
}

#[test]
fn infinity_position_image06_mixed_segments_fit_without_budget_inflation() {
    assert_infinity_position_restored(1655);
}

#[test]
fn zero_tangent_tracks_keep_original_editable_keys() {
    let mut numeric = circle();
    for key in &mut numeric.keyframes {
        key.spatial_in = vec![0.; 3];
        key.spatial_out = vec![0.; 3];
    }
    assert!(prepare(&numeric).unwrap().is_none());
}
