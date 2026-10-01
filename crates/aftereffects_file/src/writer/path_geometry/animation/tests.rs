use super::*;
use crate::properties::{data, read_path_metadata, unique_list};

fn path(x: f64, closed: bool) -> ShapePath {
    let mut commands = vec![
        ShapePathCommand::MoveTo {
            x,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::LineTo {
            x: x + 20.0,
            y: 30.0,
            mirror: None,
            corner_radius: None,
        },
    ];
    if closed {
        commands.push(ShapePathCommand::Close);
    }
    ShapePath { commands }
}

fn track(easing: KeyframeEasing) -> PathTrack {
    PathTrack {
        keyframes: vec![
            PathKeyframe {
                time_millis: -500,
                path: path(0.0, true),
                easing: KeyframeEasing::Linear,
            },
            PathKeyframe {
                time_millis: 1500,
                path: path(15.0, true),
                easing,
            },
        ],
    }
}

#[test]
fn native_path_keys_write_signed_times_parallel_shapes_and_ease_records() {
    for (easing, kinds, influences) in [
        (KeyframeEasing::Linear, [1, 1], [0.0, 0.0]),
        (KeyframeEasing::Hold, [3, 3], [0.0, 0.0]),
        (
            KeyframeEasing::CubicBezier {
                x1: 0.9,
                y1: 0.0,
                x2: 0.8,
                y2: 1.0,
            },
            [2, 2],
            [90.0, 20.0],
        ),
        (
            KeyframeEasing::CubicBezier {
                x1: 1.0 / 6.0,
                y1: 1.0 / 6.0,
                x2: 0.1,
                y2: 1.0,
            },
            [1, 2],
            [0.0, 90.0],
        ),
        (
            KeyframeEasing::CubicBezier {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            },
            [2, 2],
            [0.0, 0.0],
        ),
    ] {
        let property = animated_property(&track(easing)).unwrap();
        let storage = unique_list(property.children().unwrap(), *b"tdbs").unwrap();
        let meta = read_path_metadata(storage).unwrap();
        assert_eq!(meta.keyframes.len(), 2);
        assert_eq!(meta.keyframes[0].time_secs, -0.5);
        assert_eq!(meta.keyframes[1].time_secs, 1.5);
        assert_eq!(
            [
                meta.keyframes[0].out_interpolation,
                meta.keyframes[1].in_interpolation
            ],
            kinds
        );
        assert!((meta.keyframes[0].out_influence[0] - influences[0]).abs() < 1e-10);
        assert!((meta.keyframes[1].in_influence[0] - influences[1]).abs() < 1e-10);
        let records = data(unique_list(storage, *b"list").unwrap(), *b"ldat").unwrap();
        assert_eq!(records.len(), 128);
        assert!(records[56..64].iter().all(|v| *v == 0));
        assert_eq!(f64::from_be_bytes(records[16..24].try_into().unwrap()), 2.0);
        assert_eq!(
            unique_list(property.children().unwrap(), *b"omks")
                .unwrap()
                .len(),
            2
        );
    }
}

#[test]
fn native_path_keys_exceed_old_policy_boundary() {
    let value = PathTrack {
        keyframes: (0..=10_000)
            .map(|index| PathKeyframe {
                time_millis: index,
                path: path(index as f64, false),
                easing: KeyframeEasing::Hold,
            })
            .collect(),
    };
    let property = animated_property(&value).expect("Path keys above the old policy limit");
    assert_eq!(
        unique_list(property.children().unwrap(), *b"omks")
            .unwrap()
            .len(),
        value.keyframes.len()
    );
}

#[test]
fn native_path_keys_reject_unproved_speed_and_morph_but_allow_hold_topology() {
    assert!(
        validate_path_track(&track(KeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.5,
            x2: 0.8,
            y2: 1.0
        }))
        .is_err()
    );
    let mut value = track(KeyframeEasing::Linear);
    value.keyframes[1].path = path(15.0, false);
    assert!(validate_path_track(&value).is_err());
    value.keyframes[1].easing = KeyframeEasing::Hold;
    assert!(validate_path_track(&value).is_ok());
    value.keyframes[1].time_millis = -500;
    assert!(validate_path_track(&value).is_err());
    let mut value = track(KeyframeEasing::Linear);
    for key in &mut value.keyframes {
        key.path = path(15.0, false);
    }
    let native = animated_property(&value).unwrap();
    for shape in unique_list(native.children().unwrap(), *b"omks").unwrap() {
        assert_ne!(data(shape.children().unwrap(), *b"shph").unwrap()[3] & 8, 0);
    }
    let mut value = track(KeyframeEasing::Linear);
    for key in &mut value.keyframes {
        key.path.commands.insert(
            2,
            ShapePathCommand::LineTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
        );
    }
    assert!(
        validate_path_track(&value).is_err(),
        "only the first key folds its closing seam"
    );
}
