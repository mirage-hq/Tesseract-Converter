use super::*;
use fx_schema::TimeOffset;
use fx_schema::animator::PropertyKeyframeEasing;

use crate::structure_document::animation_budget::committed_entry_serialized_bytes;

fn copy_animator(
    animator: &PropertyAnimator,
    target: &PropertyTarget,
) -> Result<PropertyAnimator, serde_json::Error> {
    super::copy_animator(animator, target, &mut AnimationBudget::default())
        .map(|animator| animator.expect("default test budget admits copied animator"))
}

fn mirror(
    entries: &mut Vec<AnimationGraphEntry>,
    target: PropertyTarget,
    source: PropertyTarget,
    value: PropertyValue,
    warnings: &mut Vec<String>,
) -> Result<(), serde_json::Error> {
    super::mirror(
        entries,
        target,
        source,
        value,
        warnings,
        &mut AnimationBudget::default(),
    )
    .map(|kept| assert!(kept, "default test budget admits mirrored animator"))
}

#[test]
fn path_copy_budget_failure_does_not_publish_static_visible_motion() {
    let source = PropertyTarget::layer(LayerId::new(1), PropType::ShapePath);
    let target = PropertyTarget::layer(LayerId::new(2), PropType::ShapePath);
    let path: fx_schema::ShapePath = serde_json::from_value(serde_json::json!({"commands":[
        {"type":"moveTo","x":0,"y":0},{"type":"lineTo","x":20,"y":10}
    ]}))
    .unwrap();
    let animator = PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
            KeyframeId::new("path-source"),
            TimeOffset::from_millis(0),
            PropertyValue::Path(path.clone()),
            PropertyKeyframeEasing::Linear,
        )])
        .unwrap(),
    );
    let mut entries = vec![entry(source.clone(), animator, Vec::new())];
    let mut warnings = Vec::new();
    let mut budget = AnimationBudget::with_limit(1);
    assert!(
        !super::mirror(
            &mut entries,
            target.clone(),
            source.clone(),
            PropertyValue::Path(path),
            &mut warnings,
            &mut budget
        )
        .unwrap()
    );
    assert_eq!(budget.remaining(), 1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].target, source);
    assert!(entries.iter().all(|entry| entry.target != target));
    assert!(warnings.iter().any(|warning| warning.contains("omitted")));
}

#[test]
fn copied_animator_is_rejected_before_clone_without_consuming_failed_budget() {
    let target = PropertyTarget::layer(LayerId::new(2), PropType::Rotation);
    let animator = PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
            KeyframeId::new("source-key"),
            TimeOffset::from_millis_f64(100.0),
            PropertyValue::Float(12.0),
            PropertyKeyframeEasing::Linear,
        )])
        .unwrap(),
    );
    let mut budget = AnimationBudget::with_limit(1);
    assert!(
        super::copy_animator(&animator, &target, &mut budget)
            .unwrap()
            .is_none()
    );
    assert_eq!(budget.remaining(), 1);
}

#[test]
fn disabled_copied_animator_uses_exact_complete_state_budget() {
    let target = PropertyTarget::layer(LayerId::new(2), PropType::Rotation);
    let track = PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
        KeyframeId::new("source-disabled"),
        TimeOffset::from_millis(250),
        PropertyValue::Float(12.0),
        PropertyKeyframeEasing::Linear,
    )])
    .unwrap();
    let animator = PropertyAnimator::from_data(&AnimatorData::Keyframes {
        track,
        enabled: false,
        disabled_value: Some(PropertyValue::Float(7.0)),
    })
    .unwrap();

    let mut probe = AnimationBudget::default();
    super::copy_animator(&animator, &target, &mut probe)
        .unwrap()
        .unwrap();
    let required = probe.used();

    let mut too_small = AnimationBudget::with_limit(required - 1);
    assert!(
        super::copy_animator(&animator, &target, &mut too_small)
            .unwrap()
            .is_none()
    );
    assert_eq!(too_small.remaining(), required - 1);

    let mut exact = AnimationBudget::with_limit(required);
    let copied = super::copy_animator(&animator, &target, &mut exact)
        .unwrap()
        .unwrap();
    assert_eq!(exact.remaining(), 0);
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = copied.data()
    else {
        panic!("copied animator must remain keyframed")
    };
    assert!(!*enabled);
    assert_eq!(disabled_value, &Some(PropertyValue::Float(7.0)));
    let committed = entry(target, copied, Vec::new());
    assert_eq!(
        committed_entry_serialized_bytes(&committed).unwrap() + 1,
        required,
        "the exact admitted copy must not serialize beyond its charged bytes"
    );
}

#[test]
fn copied_animator_batch_rolls_back_every_staged_copy_on_later_denial() {
    let first_target = PropertyTarget::layer(LayerId::new(2), PropType::PositionX);
    let second_target = PropertyTarget::layer(LayerId::new(2), PropType::PositionY);
    let first = PropertyAnimator::constant(PropertyValue::Float(1.0)).unwrap();
    let second = PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
            KeyframeId::new("source-y"),
            TimeOffset::from_millis(0),
            PropertyValue::Float(2.0),
            PropertyKeyframeEasing::Linear,
        )])
        .unwrap(),
    );
    let mut probe = AnimationBudget::default();
    super::copy_animator(&first, &first_target, &mut probe)
        .unwrap()
        .unwrap();
    let first_reservation = probe.used();

    let mut budget = AnimationBudget::with_limit(first_reservation);
    let copied = super::copy_animators_atomically(
        [(&first, first_target), (&second, second_target)],
        &mut budget,
    )
    .unwrap();
    assert!(copied.is_none());
    assert_eq!(budget.remaining(), first_reservation);
}

#[test]
fn mirrored_authored_animator_denial_does_not_fall_back_to_a_constant() {
    let source = PropertyTarget::layer(LayerId::new(1), PropType::PositionX);
    let target = PropertyTarget::layer(LayerId::new(2), PropType::PositionX);
    let animator = PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
            KeyframeId::new("source-x"),
            TimeOffset::from_millis(0),
            PropertyValue::Float(2.0),
            PropertyKeyframeEasing::Linear,
        )])
        .unwrap(),
    );
    let mut entries = vec![entry(source.clone(), animator, Vec::new())];
    let mut warnings = Vec::new();
    let mut budget = AnimationBudget::with_limit(0);
    let kept = super::mirror(
        &mut entries,
        target.clone(),
        source,
        PropertyValue::Float(2.0),
        &mut warnings,
        &mut budget,
    )
    .unwrap();
    assert!(!kept);
    assert_eq!(entries.len(), 1);
    assert!(entries.iter().all(|entry| entry.target != target));
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("required mirrored control set omitted"))
    );
}

#[test]
fn copied_keys_have_independent_ids_without_changing_authored_state() {
    for enabled in [true, false] {
        let mut entries = Vec::new();
        for property in [PropType::PositionX, PropType::PositionY] {
            let source = PropertyTarget::layer(LayerId::new(1), property);
            let track = PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    KeyframeId::new(format!("source-{property}-0")),
                    TimeOffset::from_millis_f64(-500.0),
                    PropertyValue::Float(10.0),
                    PropertyKeyframeEasing::Linear,
                )
                .with_spatial_tangents(Some(0.0), Some(5.0)),
                PropertyKeyframe::new(
                    KeyframeId::new(format!("source-{property}-1")),
                    TimeOffset::from_millis_f64(1250.0),
                    PropertyValue::Float(40.0),
                    PropertyKeyframeEasing::Linear,
                )
                .with_spatial_tangents(Some(-5.0), Some(0.0)),
            ])
            .unwrap();
            let animator = PropertyAnimator::from_data(&AnimatorData::Keyframes {
                track,
                enabled,
                disabled_value: (!enabled).then_some(PropertyValue::Float(7.0)),
            })
            .unwrap();
            let original_wire = animator.wire_value().clone();
            entries.push(entry(source.clone(), animator.clone(), Vec::new()));
            for id in [2, 3] {
                let target = PropertyTarget::layer(LayerId::new(id), property);
                let copied = copy_animator(&animator, &target).unwrap();
                assert_eq!(
                    copied.wire_value(),
                    copy_animator(&animator, &target).unwrap().wire_value(),
                    "copy IDs are deterministic"
                );
                let mut source_value = animator.known_value();
                let mut copied_value = copied.known_value();
                for (before, after) in source_value["keyframes"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .zip(copied_value["keyframes"].as_array_mut().unwrap())
                {
                    assert_ne!(before["id"], after["id"]);
                    before.as_object_mut().unwrap().remove("id");
                    after.as_object_mut().unwrap().remove("id");
                }
                assert_eq!(source_value, copied_value);
                entries.push(entry(target, copied, Vec::new()));
            }
            assert_eq!(animator.wire_value(), &original_wire);
        }
        fx_schema::AnimationGraph::from_entries(entries).unwrap();
    }
}

#[test]
fn mirror_rekeys_tracks_and_leaves_constants_unchanged() {
    let source = PropertyTarget::layer(LayerId::new(1), PropType::Rotation);
    let target = PropertyTarget::layer(LayerId::new(2), PropType::Rotation);
    for animator in [
        PropertyAnimator::constant(PropertyValue::Float(12.0)).unwrap(),
        PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
                KeyframeId::new("rotation-key"),
                TimeOffset::from_millis_f64(100.0),
                PropertyValue::Float(12.0),
                PropertyKeyframeEasing::Hold,
            )])
            .unwrap(),
        ),
    ] {
        let mut entries = vec![entry(source.clone(), animator.clone(), Vec::new())];
        let mut warnings = Vec::new();
        mirror(
            &mut entries,
            target.clone(),
            source.clone(),
            PropertyValue::Float(0.0),
            &mut warnings,
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        if animator.keyframe_track().is_none() {
            assert_eq!(entries[1].animator.wire_value(), animator.wire_value());
        }
        assert!(entries.iter().all(|entry| !entry.animator.is_js_script()));
        fx_schema::AnimationGraph::from_entries(entries).unwrap();
    }
}
