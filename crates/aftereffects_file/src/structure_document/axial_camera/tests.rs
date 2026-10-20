use super::*;
use crate::properties::NumericKeyframe;

fn translation() -> NumericProperty {
    let key = |time_secs, z| NumericKeyframe {
        time_secs,
        values: vec![320.0, 180.0, z],
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![80.0],
        out_speed: vec![80.0],
        in_influence: vec![33.0],
        out_influence: vec![33.0],
        spatial_in: vec![0.0; 3],
        spatial_out: vec![0.0; 3],
    };
    NumericProperty {
        animated: true,
        keyframes: vec![key(8.0, 100.0), key(10.0, 0.0)],
        ..constant(0.0)
    }
}

#[test]
fn axial_camera_parent_keys_keep_signed_motion_endpoints_and_clock() {
    let source = translation();
    let mut scale = axial_translation(&source, [320.0, 180.0]).unwrap();
    assert_eq!(scale.keyframes[0].out_speed, [-80.0]);
    assert_eq!(scale.keyframes[1].in_speed, [-80.0]);
    assert_eq!(source.keyframes[0].out_speed, [80.0]);
    project_magnification(&mut scale, -800.0, 800.0).unwrap();
    assert!((scale.keyframes[0].values[0] - 80000.0 / 700.0).abs() < 1e-10);
    assert_eq!(scale.keyframes[1].values, [100.0]);
    assert!((scale.keyframes[1].in_speed[0] + 10.0).abs() < 1e-10);
    let plan = Projection {
        center: [320.0, 180.0],
        scale,
        samples: None,
        clock: NumericAnimationClock::ParentIdentity {
            start: -8.0,
            stretch: 1.0,
        },
    };
    let (stage, entries) = plan
        .stage(None, LayerId::new(99), &mut AnimationBudget::default())
        .unwrap();
    assert_eq!(stage.transform.position, Position::TwoD([320.0, 180.0]));
    assert_eq!(entries.len(), 2);
    let value = serde_json::to_value(&entries).unwrap();
    for entry in value.as_array().unwrap() {
        assert_eq!(
            entry["animator"]["keyframes"][0]["layerTime"].as_f64(),
            Some(0.0)
        );
        assert_eq!(
            entry["animator"]["keyframes"][1]["layerTime"].as_f64(),
            Some(2000.0)
        );
        assert_eq!(entry["animator"]["keyframes"][1]["value"]["value"], 100.0);
    }
}

#[test]
fn axial_camera_declines_off_axis_curves_expressions_and_near_plane() {
    let mut source = translation();
    source.keyframes[0].spatial_out[0] = 1.0;
    assert!(axial_translation(&source, [320.0, 180.0]).is_err());
    let mut source = translation();
    source.keyframes[0].spatial_out[2] = 500.0;
    assert!(axial_translation(&source, [320.0, 180.0]).is_err());
    let mut source = translation();
    source.expression_enabled = true;
    assert!(axial_translation(&source, [320.0, 180.0]).is_err());
    assert!(project_magnification(&mut constant(800.0), -800.0, 800.0).is_err());
    let mut scale = axial_translation(&translation(), [320.0, 180.0]).unwrap();
    project_magnification(&mut scale, -800.0, 800.0).unwrap();
    let plan = Projection {
        center: [320.0, 180.0],
        scale,
        clock: NumericAnimationClock::source_local(),
        samples: None,
    };
    let mut budget = AnimationBudget::with_limit(1);
    assert!(plan.stage(None, LayerId::new(99), &mut budget).is_err());
    assert_eq!(budget.used(), 0);
}
