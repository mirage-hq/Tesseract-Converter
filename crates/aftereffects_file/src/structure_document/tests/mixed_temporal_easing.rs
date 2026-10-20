use super::*;
use fx_schema::{PropType, PropertyKeyframeEasing, PropertyTarget};

fn find_group<'a>(layers: &'a [fx_schema::Layer], name: &str) -> Option<&'a fx_schema::GroupLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        FxLayer::Group(group) if group.name == name => Some(group),
        FxLayer::Group(group) => find_group(&group.layers, name),
        _ => None,
    })
}

fn orientation_half_angle(value: &Value) -> f64 {
    let angles: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| (value.as_f64().unwrap() / 2.0).to_radians())
        .collect();
    let [x, y, z] = angles.as_slice() else {
        panic!("native XYZ orientation")
    };
    (x.cos() * y.cos() * z.cos() - x.sin() * y.sin() * z.sin()).acos()
}

#[test]
fn native_mixed_numeric_easing_matches_independent_adobe_progress() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/path-animation");
    let source = fs::read(dir.join("property_path_orientation_ease.aep")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&source)),
        "cf2506e440cdd11cbba6224e6d146f96df390238ead2f641983d4b551ca08d07"
    );
    let projection = crate::test_fixtures::read(
        dir.join("property_path_orientation_ease_value_at_time.json.gz"),
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&projection)),
        "8c220ba7b51a1c3c97c657d74ae7302603acb8963f82f332f284c490479503b1"
    );
    let oracle: Value = serde_json::from_slice(&projection).unwrap();
    let property = oracle["comps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|comp| comp["name"] == "vat_ease")
        .unwrap()["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["name"] == "orient_mixed")
        .unwrap()["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["matchName"] == "ADBE Orientation")
        .unwrap();
    let project = read_project(&source).unwrap();
    let imported = to_structural_fx_document(&project, Some(16)).unwrap();
    let group = find_group(imported.document.composition().layers(), "orient_mixed").unwrap();
    let entries = imported.document.composition().dynamics().entries();
    for axis in [
        PropType::OrientationX,
        PropType::OrientationY,
        PropType::OrientationZ,
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(group.id, axis))
            .unwrap();
        let track = entry.animator.keyframe_track().unwrap();
        assert_eq!(track.keyframes().len(), 2);
        let easing = track.keyframes()[1].easing();
        assert_eq!(
            easing,
            PropertyKeyframeEasing::CubicBezier {
                x1: 1.0 / 6.0,
                y1: 1.0 / 6.0,
                x2: 1.0 - 0.9,
                y2: 1.0,
            }
        );
        // Native Orientation uses quaternion interpolation; its angle from
        // identity isolates temporal progress without asserting Euler fidelity.
        let total_angle = orientation_half_angle(&property["keyframes"][1]["value"]);
        for frame in property["frames"].as_array().unwrap() {
            let time = frame["time"].as_f64().unwrap();
            if !(0.0..2.0).contains(&time) {
                continue;
            }
            let progress = match easing {
                PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                    let cubic = |a: f64, b: f64, t: f64| {
                        3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t.powi(3)
                    };
                    let (mut lo, mut hi) = (0.0, 1.0);
                    for _ in 0..50 {
                        let t = (lo + hi) / 2.0;
                        if cubic(x1, x2, t) < time / 2.0 {
                            lo = t;
                        } else {
                            hi = t;
                        }
                    }
                    cubic(y1, y2, (lo + hi) / 2.0)
                }
                _ => panic!("native mixed cubic easing"),
            };
            let expected = orientation_half_angle(&frame["value"]) / total_angle;
            assert!(
                (progress - expected).abs() < 1e-7,
                "t={time}: {progress} != {expected}"
            );
        }
    }
}
