use fx_schema::animator::{
    AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
};
use fx_schema::{Layer, PropertyTarget, TimeOffset};
use serde_json::json;

use super::*;

fn key(name: &str, millis: i64, value: f64, easing: PropertyKeyframeEasing) -> PropertyKeyframe {
    PropertyKeyframe::new(
        KeyframeId::new(name),
        TimeOffset::from_millis(millis),
        PropertyValue::Float(value),
        easing,
    )
}

fn track_entry(property: PropType, keys: Vec<PropertyKeyframe>) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(LayerId::new(7), property),
        animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn transform(position: Position) -> Transform {
    let mut transform: Transform = serde_json::from_value(json!({
        "anchorPoint": [0.0, 0.0],
        "position": [0.0, 0.0],
        "scale": [100.0, 100.0],
        "rotation": 0.0,
        "opacity": 100.0
    }))
    .unwrap();
    transform.position = position;
    transform
}

fn assert_contains(bounds: Bounds, point: [f64; 2]) {
    for axis in 0..2 {
        assert!(
            point[axis] >= bounds.min[axis] - 1e-8 && point[axis] <= bounds.max[axis] + 1e-8,
            "axis {axis}: {point:?} not in {:?}..{:?}",
            bounds.min,
            bounds.max
        );
    }
}

fn project_corner(transform: &Transform, corner: [f64; 2], canvas: Dimensions) -> [f64; 2] {
    let [anchor_x, anchor_y] = transform.anchor_point;
    let [scale_x, scale_y] = transform.scale;
    let mut x = (corner[0] - anchor_x) * scale_x / 100.0;
    let mut y = (corner[1] - anchor_y) * scale_y / 100.0;
    let axis = transform.skew_axis.to_radians();
    let (sin_axis, cos_axis) = axis.sin_cos();
    let axis_x = cos_axis * x - sin_axis * y;
    let axis_y = sin_axis * x + cos_axis * y;
    let sheared_x = axis_x - transform.skew.clamp(-89.9, 89.9).to_radians().tan() * axis_y;
    x = cos_axis * sheared_x + sin_axis * axis_y;
    y = -sin_axis * sheared_x + cos_axis * axis_y;

    let (sin_x, cos_x) = transform.rotation_x.to_radians().sin_cos();
    let (sin_y, cos_y) = transform.rotation_y.to_radians().sin_cos();
    let (sin_z, cos_z) = transform.rotation.to_radians().sin_cos();
    let (x1, y1, z1) = (x, y * cos_x, y * sin_x);
    let (x2, y2, z2) = (x1 * cos_y + z1 * sin_y, y1, -x1 * sin_y + z1 * cos_y);
    let [position_x, position_y] = transform.position.xy_array();
    let world = [
        position_x + x2 * cos_z - y2 * sin_z,
        position_y + x2 * sin_z + y2 * cos_z,
        transform.position.z_or_zero() + z2,
    ];
    let distance = f64::from(canvas.width) * 1.388;
    let factor = distance / (distance + world[2]);
    let center = [
        f64::from(canvas.width) / 2.0,
        f64::from(canvas.height) / 2.0,
    ];
    [
        center[0] + (world[0] - center[0]) * factor,
        center[1] + (world[1] - center[1]) * factor,
    ]
}

#[test]
fn constant_three_d_transform_projects_through_the_root_camera() {
    let local = Bounds {
        min: [0.0, 0.0],
        max: [10.0, 10.0],
    };
    let transform = transform(Position::ThreeD([50.0, 50.0, -50.0]));
    let media = BTreeMap::new();
    let canvas = Dimensions::new(100, 100);
    let analyzer = Analyzer {
        source_interval: None,
        dynamics: &crate::export_document::AnimationIndex::new(&[]),
        resolved_media: &media,
        canvas,
    };
    let bounds = analyzer
        .transform_bounds(local, LayerId::new(7), &transform)
        .unwrap();
    let factor = 138.8 / 88.8;
    assert!((bounds.min[0] - 50.0).abs() < 1e-9);
    assert!((bounds.min[1] - 50.0).abs() < 1e-9);
    assert!((bounds.max[0] - (50.0 + 10.0 * factor)).abs() < 1e-9);
    assert!((bounds.max[1] - (50.0 + 10.0 * factor)).abs() < 1e-9);
}

#[test]
fn trigonometric_ranges_include_interior_minimum_and_maximum() {
    let (sin, _) = degree_sin_cos(Interval::new(30.0, 150.0).unwrap()).unwrap();
    assert_eq!(sin.max, 1.0);
    let (_, cos) = degree_sin_cos(Interval::new(-200.0, 20.0).unwrap()).unwrap();
    assert_eq!(cos.min, -1.0);
    assert_eq!(cos.max, 1.0);
}

#[test]
fn animated_skew_cubic_overshoot_and_negative_scale_stay_enclosed() {
    let local = Bounds {
        min: [-20.0, -10.0],
        max: [30.0, 40.0],
    };
    let mut transform = transform(Position::ThreeD([50.0, 50.0, 20.0]));
    transform.scale = [-120.0, 80.0];
    transform.rotation_x = 20.0;
    transform.rotation_y = -15.0;
    let entries = vec![
        track_entry(
            PropType::Skew,
            vec![
                key("skew-a", 0, 0.0, PropertyKeyframeEasing::Linear),
                key(
                    "skew-b",
                    1000,
                    30.0,
                    PropertyKeyframeEasing::CubicBezier {
                        x1: 0.2,
                        y1: 2.0,
                        x2: 0.8,
                        y2: 2.0,
                    },
                ),
            ],
        ),
        track_entry(
            PropType::SkewAxis,
            vec![
                key("axis-a", 0, -45.0, PropertyKeyframeEasing::Linear),
                key("axis-b", 1000, 120.0, PropertyKeyframeEasing::Linear),
            ],
        ),
        track_entry(
            PropType::Rotation,
            vec![
                key("rotation-a", 0, -30.0, PropertyKeyframeEasing::Linear),
                key("rotation-b", 1000, 150.0, PropertyKeyframeEasing::Linear),
            ],
        ),
    ];
    let media = BTreeMap::new();
    let canvas = Dimensions::new(200, 120);
    let analyzer = Analyzer {
        source_interval: None,
        dynamics: &crate::export_document::AnimationIndex::new(&entries),
        resolved_media: &media,
        canvas,
    };
    let bounds = analyzer
        .transform_bounds(local, LayerId::new(7), &transform)
        .unwrap();

    // Sampling is only a sanity check of the proved interval enclosure. The
    // implementation itself never samples and includes the cubic control hull.
    for skew in [0.0, 30.0, 49.0] {
        for axis in [-45.0, 20.0, 120.0] {
            for rotation in [-30.0, 60.0, 150.0] {
                let mut sampled = transform;
                sampled.skew = skew;
                sampled.skew_axis = axis;
                sampled.rotation = rotation;
                for corner in [
                    local.min,
                    [local.max[0], local.min[1]],
                    [local.min[0], local.max[1]],
                    local.max,
                ] {
                    assert_contains(bounds, project_corner(&sampled, corner, canvas));
                }
            }
        }
    }
}

#[test]
fn interior_position_z_overshoot_that_crosses_the_camera_is_rejected() {
    let entries = vec![track_entry(
        PropType::PositionZ,
        vec![
            key("z-a", 0, 0.0, PropertyKeyframeEasing::Linear),
            key(
                "z-b",
                1000,
                10.0,
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: -20.0,
                    x2: 0.8,
                    y2: -20.0,
                },
            ),
        ],
    )];
    let media = BTreeMap::new();
    let analyzer = Analyzer {
        source_interval: None,
        dynamics: &crate::export_document::AnimationIndex::new(&entries),
        resolved_media: &media,
        canvas: Dimensions::new(100, 100),
    };
    let result = analyzer.transform_bounds(
        Bounds {
            min: [0.0, 0.0],
            max: [10.0, 10.0],
        },
        LayerId::new(7),
        &transform(Position::ThreeD([50.0, 50.0, 0.0])),
    );
    match result {
        Err(error) => assert_eq!(error, "3D descendant reaches the root camera near plane"),
        Ok(_) => panic!("the continuous Position Z hull crosses the root camera"),
    }
}

#[test]
fn tilted_corner_crossing_is_rejected_even_when_the_layer_origin_is_in_front() {
    let media = BTreeMap::new();
    let analyzer = Analyzer {
        source_interval: None,
        dynamics: &crate::export_document::AnimationIndex::new(&[]),
        resolved_media: &media,
        canvas: Dimensions::new(100, 100),
    };
    let mut transform = transform(Position::ThreeD([50.0, 50.0, -100.0]));
    transform.rotation_x = -90.0;
    assert!(
        analyzer
            .transform_bounds(
                Bounds {
                    min: [0.0, 0.0],
                    max: [10.0, 100.0],
                },
                LayerId::new(7),
                &transform,
            )
            .is_err()
    );
}

#[test]
fn nonfinite_transform_values_are_rejected() {
    let media = BTreeMap::new();
    let analyzer = Analyzer {
        source_interval: None,
        dynamics: &crate::export_document::AnimationIndex::new(&[]),
        resolved_media: &media,
        canvas: Dimensions::new(100, 100),
    };
    let mut transform = transform(Position::ThreeD([50.0, 50.0, 0.0]));
    transform.skew = f64::NAN;
    assert!(
        analyzer
            .transform_bounds(
                Bounds {
                    min: [0.0, 0.0],
                    max: [10.0, 10.0],
                },
                LayerId::new(7),
                &transform,
            )
            .is_err()
    );
}

fn correlated_entries(simultaneous: bool) -> Vec<AnimationGraphEntry> {
    let linear = PropertyKeyframeEasing::Linear;
    vec![
        track_entry(
            PropType::RotationY,
            vec![
                key("r0", 0, 0.0, linear),
                key("r1", 100, 90.0, linear),
                key("r2", 200, 0.0, linear),
                key("r3", 300, 0.0, linear),
            ],
        ),
        track_entry(
            PropType::ScaleX,
            vec![
                key("s0", 0, 100.0, linear),
                key("s1", 100, if simultaneous { 190.0 } else { 103.5 }, linear),
                key("s2", 200, 103.5, linear),
                key("s3", 300, 190.0, linear),
            ],
        ),
    ]
}

fn correlated_bounds(entries: &[AnimationGraphEntry]) -> Result<Bounds, &'static str> {
    let media = BTreeMap::new();
    let index = crate::export_document::AnimationIndex::new(entries);
    let analyzer = Analyzer {
        dynamics: &index,
        resolved_media: &media,
        canvas: Dimensions::new(100, 100),
        source_interval: None,
    };
    analyzer.transform_bounds(
        Bounds {
            min: [-100.0, -10.0],
            max: [100.0, 10.0],
        },
        LayerId::new(7),
        &transform(Position::ThreeD([50.0, 50.0, 0.0])),
    )
}

#[test]
fn disjoint_scale_peak_and_quarter_turn_are_continuously_enclosed() {
    let entries = correlated_entries(false);
    let media = BTreeMap::new();
    let index = crate::export_document::AnimationIndex::new(&entries);
    let analyzer = Analyzer {
        dynamics: &index,
        resolved_media: &media,
        canvas: Dimensions::new(100, 100),
        source_interval: None,
    };
    assert_eq!(
        analyzer
            .transform_bounds_window(
                Bounds {
                    min: [-100.0, -10.0],
                    max: [100.0, 10.0]
                },
                LayerId::new(7),
                &transform(Position::ThreeD([50.0, 50.0, 0.0])),
                None,
            )
            .unwrap_err(),
        "3D descendant reaches the root camera near plane"
    );
    assert!(correlated_bounds(&entries).is_ok());
    assert_eq!(
        correlated_bounds(&correlated_entries(true)).unwrap_err(),
        "3D descendant reaches the root camera near plane"
    );
}

#[test]
fn correlated_windows_retain_temporal_and_spatial_overshoot() {
    let mut entries = correlated_entries(false);
    entries.push(track_entry(
        PropType::PositionZ,
        vec![
            key("z0", 0, 0.0, PropertyKeyframeEasing::Linear),
            key(
                "z1",
                100,
                10.0,
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: -100.0,
                    x2: 0.8,
                    y2: -100.0,
                },
            ),
        ],
    ));
    assert!(correlated_bounds(&entries).is_err());
    entries.pop();
    entries.push(track_entry(
        PropType::PositionX,
        vec![
            key("x0", 0, 50.0, PropertyKeyframeEasing::Linear)
                .with_spatial_tangents(None, Some(1000.0)),
            key("x1", 100, 50.0, PropertyKeyframeEasing::Linear)
                .with_spatial_tangents(Some(1000.0), None),
        ],
    ));
    // Translation is not a depth change; nevertheless its complete spatial hull
    // must survive the same windowed scalar enclosure used for projection.
    let AnimatorData::Keyframes { track, .. } = entries.last().unwrap().animator.data() else {
        panic!("expected position keys")
    };
    let range = track_component_range_window(
        track,
        Component::Scalar,
        Some((
            Some(TimeOffset::from_millis(25)),
            Some(TimeOffset::from_millis(75)),
        )),
    )
    .unwrap();
    assert!(range.max >= 1050.0);
}

#[test]
fn correlated_proof_cannot_receive_a_nonfinite_track() {
    // Invalid key values are rejected before the analyzer can retry a proof.
    assert!(
        PropertyKeyframeTrack::new(vec![key(
            "bad",
            0,
            f64::NAN,
            PropertyKeyframeEasing::Linear,
        )])
        .is_err()
    );
}

#[test]
fn correlated_window_budget_exhaustion_keeps_near_plane_rejection() {
    let mut entries = correlated_entries(false);
    entries.push(track_entry(
        PropType::Rotation,
        (0..256)
            .map(|time| {
                key(
                    &format!("extra-{time}"),
                    time,
                    0.0,
                    PropertyKeyframeEasing::Linear,
                )
            })
            .collect(),
    ));
    assert_eq!(
        correlated_bounds(&entries).unwrap_err(),
        "3D descendant reaches the root camera near plane"
    );
}

fn rect_layer(masked: bool) -> Layer {
    let masks = if masked {
        json!([{
            "id": 8,
            "inverted": true,
            "feather": [40.0, 50.0],
            "expansion": 20.0,
            "path": {"commands": [
                {"type": "moveTo", "x": 0.0, "y": 0.0},
                {"type": "lineTo", "x": 20.0, "y": 0.0},
                {"type": "lineTo", "x": 20.0, "y": 20.0},
                {"type": "close"}
            ]}
        }])
    } else {
        json!([])
    };
    serde_json::from_value(json!({
        "id": 7,
        "type": "Rect",
        "name": "Masked bounds rect",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {
            "anchorPoint": [0.0, 0.0], "position": [0.0, 0.0],
            "scale": [100.0, 100.0], "rotation": 0.0, "opacity": 100.0
        },
        "rect": {"size": [100.0, 50.0], "fillColor": [1.0, 1.0, 1.0, 1.0]},
        "masks": masks
    }))
    .unwrap()
}

#[test]
fn multiplicative_masks_retain_the_unmasked_animated_enclosure() {
    let media = BTreeMap::new();
    let analyzer = Analyzer {
        source_interval: None,
        dynamics: &crate::export_document::AnimationIndex::new(&[]),
        resolved_media: &media,
        canvas: Dimensions::new(640, 360),
    };
    let unmasked = analyzer.layer_bounds(&rect_layer(false)).unwrap().unwrap();
    let masked = analyzer.layer_bounds(&rect_layer(true)).unwrap().unwrap();
    assert_eq!(masked.min, unmasked.min);
    assert_eq!(masked.max, unmasked.max);
}
