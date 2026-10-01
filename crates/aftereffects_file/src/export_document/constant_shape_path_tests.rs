use super::*;
use fx_schema::{
    PropertyTarget, ShapePathCommand, TimeOffset,
    animator::{KeyframeId, PropertyKeyframe, PropertyKeyframeTrack},
};

fn path(x: f64) -> ShapePath {
    ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x,
                y: 2.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: x + 3.0,
                y: 4.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::Close,
        ],
    }
}

fn entry(id: LayerId, animator: PropertyAnimator) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(id, PropType::ShapePath),
        animator,
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

#[test]
fn constant_shape_path_materializes_effective_geometry_instead_of_stale_base() {
    let id = LayerId::new(41);
    let base = path(1.0);
    let current = path(9.0);
    let entries = [entry(
        id,
        PropertyAnimator::constant(PropertyValue::Path(current.clone())).unwrap(),
    )];

    assert_ne!(base, current);
    assert_eq!(
        effective_static_shape_path(&entries, id, &base).unwrap(),
        &current
    );
}

#[test]
fn keyframed_shape_path_remains_excluded_from_native_path_export() {
    let id = LayerId::new(42);
    let track = PropertyKeyframeTrack::new(vec![
        PropertyKeyframe::new(
            KeyframeId::new("shape-path-0"),
            TimeOffset::ZERO,
            PropertyValue::Float(1.0),
            PropertyKeyframeEasing::Linear,
        ),
        PropertyKeyframe::new(
            KeyframeId::new("shape-path-1"),
            TimeOffset::from_millis(100),
            PropertyValue::Float(2.0),
            PropertyKeyframeEasing::Linear,
        ),
    ])
    .unwrap();
    let entries = [entry(id, PropertyAnimator::keyframes(track))];

    assert_eq!(
        effective_static_shape_path(&entries, id, &path(0.0)).unwrap_err(),
        "Keyframed Shape Path export is excluded; only the current static editable path can be authored natively"
    );
}

#[test]
fn constant_shape_path_rejects_wrong_value_kind_contextually() {
    let id = LayerId::new(43);
    let entries = [entry(
        id,
        PropertyAnimator::constant(PropertyValue::Float(7.0)).unwrap(),
    )];

    assert_eq!(
        effective_static_shape_path(&entries, id, &path(0.0)).unwrap_err(),
        "Shape Path animator on a Shape layer must evaluate to a path value"
    );
}
