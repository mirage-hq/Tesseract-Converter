use fx_schema::{LayerId, PropType, PropertyKeyframeEasing, PropertyTarget, PropertyValue};

use crate::structure_document::animation_budget::AnimationBudget;

// Entirely synthetic controls: a curved path with motion on every axis.
#[test]
fn synthetic_spatial_xyz_emits_all_three_editable_tracks() {
    use super::{NumericAnimationClock, NumericAnimationTarget, numeric_entries};
    use crate::properties::{NumericKeyframe, NumericProperty, NumericValueKind};

    let key = |time_secs, values, spatial_in, spatial_out| NumericKeyframe {
        time_secs,
        values,
        spatial_in,
        spatial_out,
        in_interpolation: 1,
        out_interpolation: 1,
        in_speed: vec![0.],
        out_speed: vec![0.],
        in_influence: vec![33.333333],
        out_influence: vec![33.333333],
    };
    let numeric = NumericProperty {
        values: vec![],
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        value_kind: NumericValueKind::Continuous,
        keyframes: vec![
            key(0., vec![0., 0., 0.], vec![0.; 3], vec![0., 50., 40.]),
            key(1., vec![100., 80., -60.], vec![-30., 0., -40.], vec![0.; 3]),
        ],
    };
    let id = LayerId::new(100);
    let targets = [
        PropType::PositionX,
        PropType::PositionY,
        PropType::PositionZ,
    ]
    .into_iter()
    .enumerate()
    .map(|(axis, property)| {
        NumericAnimationTarget::float(PropertyTarget::layer(id, property), axis, 1.)
    })
    .collect::<Vec<_>>();
    let (entries, warnings) = numeric_entries(
        "ADBE Position",
        &numeric,
        &targets,
        NumericAnimationClock::source_local(),
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 3, "{warnings:?}");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("shared path-speed"))
    );
    for (axis, target) in targets.iter().enumerate() {
        let entry = entries
            .iter()
            .find(|entry| entry.target == target.target)
            .unwrap();
        let track = entry.animator.keyframe_track().unwrap();
        assert!(track.keyframes().len() > 2);
        assert!(!track.has_spatial_tangents());
        for source in &numeric.keyframes {
            let key = track
                .keyframes()
                .iter()
                .find(|key| key.layer_time().as_millis() == (source.time_secs * 1000.) as i64)
                .unwrap();
            assert_eq!(key.value(), &PropertyValue::Float(source.values[axis]));
            assert_eq!(key.easing(), PropertyKeyframeEasing::Linear);
        }
    }
    fx_schema::AnimationGraph::from_entries(entries).unwrap();
}
