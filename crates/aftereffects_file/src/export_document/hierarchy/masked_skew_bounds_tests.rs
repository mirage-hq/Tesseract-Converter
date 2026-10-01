use super::*;
use crate::structure::read_project;
use crate::structure_document::to_structural_fx_document;
use serde_json::json;
use sha2::{Digest, Sha256};

fn source_group() -> GroupLayer {
    let bytes =
        include_bytes!("../../../tests/fixtures/pr4442_native/sources/transform2d_solid.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "4899cd41a07e3a101809d3ccd0ea9c053082b01510f68719a94064283f23281e"
    );
    let project = read_project(bytes).unwrap();
    let imported = to_structural_fx_document(&project, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let rect = &imported["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0];
    assert_eq!(rect["rect"]["size"], json!([220.0, 140.0]));
    serde_json::from_value(json!({
        "id": 1000,
        "name": "Bounds wrapper",
        "playback": super::super::tests::fixture_linear_playback(json!({"start":0,"duration":2000}), json!({"start":0,"duration":2000})),
        "transform": {
            "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
            "rotation": 0, "opacity": 100
        },
        "layers": [rect]
    }))
    .unwrap()
}

#[test]
fn masked_native_derived_group_retains_its_unmasked_enclosure() {
    // This mutation is supplementary bounds evidence, not independent native
    // mask-render proof. Mask conversion remains a separate lowering step.
    let mut group = source_group();
    let canvas = fx_schema::Dimensions {
        width: 640,
        height: 360,
    };
    let sources = BTreeMap::new();
    let before = static_child_union(&group, &sources, canvas)
        .unwrap()
        .unwrap();
    group.masks.push(
        serde_json::from_value(json!({
            "id": 1001, "inverted": true, "feather": [40, 50],
            "expansion": 20,
            "path": {"commands": [
                {"type": "moveTo", "x": 0, "y": 0},
                {"type": "lineTo", "x": 20, "y": 0},
                {"type": "lineTo", "x": 20, "y": 20},
                {"type": "close"}
            ]}
        }))
        .unwrap(),
    );
    check_static_nested_group(&group).unwrap();
    let after = static_child_union(&group, &sources, canvas)
        .unwrap()
        .unwrap();
    assert_eq!(before.min, after.min);
    assert_eq!(before.max, after.max);
    group.is_hidden = true;
    assert!(check_static_nested_group(&group).is_err());
    group.is_hidden = false;
    group.fills.push(
        serde_json::from_value(json!({
            "paint": {"type": "solid", "color": [1, 0, 0, 1]}, "opacity": 1
        }))
        .unwrap(),
    );
    assert!(check_static_nested_group(&group).is_err());
}

fn assert_bounds(bounds: Bounds, min: [f64; 2], max: [f64; 2]) {
    for axis in 0..2 {
        assert!((bounds.min[axis] - min[axis]).abs() < 1e-9);
        assert!((bounds.max[axis] - max[axis]).abs() < 1e-9);
    }
}

#[test]
fn static_skew_encloses_all_corners_with_axis_and_negative_scale() {
    let bounds = Bounds {
        min: [0.0, 0.0],
        max: [10.0, 20.0],
    };
    let mut transform = source_group().transform;
    transform.skew = 45.0;
    assert_bounds(
        transform_bounds(bounds, &transform).unwrap(),
        [-20.0, 0.0],
        [10.0, 20.0],
    );
    transform.skew_axis = 90.0;
    transform.scale = [-100.0, 200.0];
    assert_bounds(
        transform_bounds(bounds, &transform).unwrap(),
        [-10.0, -10.0],
        [0.0, 40.0],
    );
}

#[test]
fn skew_clamp_preserves_finite_and_three_d_guards() {
    let bounds = Bounds {
        min: [0.0, 0.0],
        max: [10.0, 20.0],
    };
    let mut transform = source_group().transform;
    transform.skew = 90.0;
    let clamped = transform_bounds(bounds, &transform).unwrap();
    transform.skew = 89.9;
    let expected = transform_bounds(bounds, &transform).unwrap();
    assert_eq!(clamped.min, expected.min);
    assert_eq!(clamped.max, expected.max);
    transform.skew = f64::NAN;
    assert!(transform_bounds(bounds, &transform).is_err());
    transform.skew = 0.0;
    transform.position = Position::ThreeD([0.0, 0.0, 1.0]);
    assert!(transform_bounds(bounds, &transform).is_err());
    transform.position = Position::TwoD([0.0, 0.0]);
    transform.scale = [f64::MAX, f64::MAX];
    let overflowing = Bounds {
        min: [0.0, 0.0],
        max: [1e10, 1e10],
    };
    assert!(transform_bounds(overflowing, &transform).is_err());
}
