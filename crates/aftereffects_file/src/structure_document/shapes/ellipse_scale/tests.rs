use super::*;
use fx_schema::TimeOffset;

fn shape() -> ShapeLayer {
    let mut shape: ShapeLayer = serde_json::from_value(serde_json::json!({
        "id": 3, "name": "outer stroke",
        "activeRange": super::super::full_active_range(),
        "transform": super::super::identity_transform(),
        "shape": {"path": {"commands": []}, "ellipse": {"size": [100.0, 80.0]},
            "strokes": [{"paint": {"type": "solid", "color": [1, 1, 1, 1]}, "width": 10}]}
    }))
    .unwrap();
    shape.transform.scale = [400.0, 400.0];
    shape
}

fn keys(property: PropType, easing: PropertyKeyframeEasing) -> AnimationGraphEntry {
    bindings::entry(
        PropertyTarget::layer(LayerId::new(2), property),
        PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    KeyframeId::new(format!("{property}-0")),
                    TimeOffset::from_millis(-100),
                    PropertyValue::Float(0.0),
                    PropertyKeyframeEasing::Linear,
                ),
                PropertyKeyframe::new(
                    KeyframeId::new(format!("{property}-1")),
                    TimeOffset::from_millis(900),
                    PropertyValue::Float(400.0),
                    easing,
                ),
            ])
            .unwrap(),
        ),
        Vec::new(),
    )
}

#[test]
fn ellipse_scale_preserves_easing_and_rejects_incompatible_controls_atomically() {
    for easing in [
        PropertyKeyframeEasing::Hold,
        PropertyKeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: 0.0,
            x2: 0.8,
            y2: 1.0,
        },
    ] {
        let original = vec![
            keys(PropType::ScaleX, easing),
            keys(PropType::ScaleY, easing),
        ];
        let mut entries = original.clone();
        let mut shape = shape();
        assert!(
            normalize(
                &mut shape,
                LayerId::new(1),
                LayerId::new(2),
                &mut entries,
                &mut AnimationBudget::default()
            )
            .unwrap()
        );
        let track = entries[2].animator.keyframe_track().unwrap();
        assert_eq!(track.keyframes()[0].layer_time().as_millis(), -100);
        assert_eq!(track.keyframes()[1].layer_time().as_millis(), 900);
        assert_eq!(track.keyframes()[1].easing(), easing);
        assert_eq!(
            track.keyframes()[1].value(),
            &PropertyValue::Vector2([400.0, 320.0])
        );
        assert_ne!(
            track.keyframes()[0].id(),
            original[0].animator.keyframe_track().unwrap().keyframes()[0].id()
        );
        assert_eq!(&entries[..2], original);
    }
    for variant in 0..5 {
        let mut entries = vec![
            keys(PropType::ScaleX, PropertyKeyframeEasing::Linear),
            keys(PropType::ScaleY, PropertyKeyframeEasing::Linear),
        ];
        match variant {
            0 => entries[1] = keys(PropType::ScaleY, PropertyKeyframeEasing::Hold),
            1 => entries[0]
                .dependencies
                .push(PropertyTarget::layer(LayerId::new(4), PropType::ScaleX)),
            2 => entries.push(entries[0].clone()),
            3 => {
                let easing = PropertyKeyframeEasing::CubicBezier {
                    x1: 0.3,
                    y1: -1.0,
                    x2: 0.7,
                    y2: 1.0,
                };
                entries = vec![
                    keys(PropType::ScaleX, easing),
                    keys(PropType::ScaleY, easing),
                ];
            }
            _ => {
                entries.pop();
            }
        }
        let original = entries.clone();
        let mut shape = shape();
        let original_shape = shape.clone();
        let mut budget = AnimationBudget::with_limit(100_000);
        let remaining = budget.remaining();
        assert!(
            !normalize(
                &mut shape,
                LayerId::new(1),
                LayerId::new(2),
                &mut entries,
                &mut budget
            )
            .unwrap()
        );
        assert_eq!(shape, original_shape);
        assert_eq!(entries, original);
        assert_eq!(budget.remaining(), remaining);
    }
}

#[test]
fn ellipse_scale_budget_and_unsupported_geometry_keep_original_paint() {
    for variant in 0..6 {
        let mut shape = shape();
        match variant {
            1 => shape.transform.scale = [400.0, 300.0],
            2 => shape.transform.anchor_point = [10.0, 0.0],
            3 => shape.transform.scale = [-400.0, -400.0],
            4 => shape.shape.trim = Some(Default::default()),
            5 => shape.transform.skew = 20.0,
            _ => {}
        }
        let original = shape.clone();
        let mut entries = Vec::new();
        let mut budget = AnimationBudget::with_limit(if variant == 0 { 1 } else { 100_000 });
        let remaining = budget.remaining();
        assert!(
            !normalize(
                &mut shape,
                LayerId::new(1),
                LayerId::new(2),
                &mut entries,
                &mut budget
            )
            .unwrap()
        );
        assert_eq!(shape, original);
        assert!(entries.is_empty());
        assert_eq!(budget.remaining(), remaining);
    }
}
