use super::*;
use sha2::{Digest, Sha256};

#[test]
fn native_hold_endpoint_flags_public_minimal_fx_keys_preserve_segment_ownership() {
    use fx_schema::animator::{
        AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    };
    for middle in [1.0, 0.4] {
        let id = LayerId::new(880);
        let mut value = imported();
        value["composition"]["layers"] = json!([rect(&value, 880)]);
        let keys = [-500, 500, 1500]
            .into_iter()
            .zip([0.0, middle, 0.4])
            .enumerate()
            .map(|(index, (time, value))| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("hold-endpoint-{index}")),
                    fx_schema::TimeOffset::from_millis(time),
                    PropertyValue::Float(value * 100.0),
                    if index == 1 {
                        PropertyKeyframeEasing::Hold
                    } else {
                        PropertyKeyframeEasing::Linear
                    },
                )
            })
            .collect();
        let entry = AnimationGraphEntry {
            target: fx_schema::PropertyTarget::layer(id, PropType::Opacity),
            animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        value["composition"]["dynamics"] = json!({"entries": [entry]});
        let output = export(value);
        let generated = read_project(&output.bytes).unwrap();
        let curves = opacity_curves(&generated);
        assert_eq!(curves.len(), 1, "{:?}", output.diagnostics);
        assert_curve(&curves[0], middle, true);
    }
}

fn oracle() -> StructuralProject {
    let bytes = include_bytes!("../../../tests/fixtures/properties/hold_endpoint_flags.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "70b9804ecc4f95f6ad8fb4d95a976168d4b6184181a763aabbf2778ba6a14f4d"
    );
    read_project(bytes).unwrap()
}

fn opacity_curves(project: &StructuralProject) -> Vec<crate::properties::NumericProperty> {
    project
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp),
            _ => None,
        })
        .flat_map(|comp| comp.layers.iter())
        .filter_map(|layer| {
            Some(
                crate::properties::read_transform(&layer.content)
                    .unwrap()
                    .into_iter()
                    .find(|property| property.match_name == "ADBE Opacity")?
                    .numeric
                    .unwrap(),
            )
        })
        .filter(|curve| curve.keyframes.len() == 3)
        .collect()
}

fn assert_curve(curve: &crate::properties::NumericProperty, middle: f64, linear_after: bool) {
    assert_eq!(
        curve
            .keyframes
            .iter()
            .map(|key| key.time_secs)
            .collect::<Vec<_>>(),
        [-0.5, 0.5, 1.5]
    );
    for (key, value) in curve.keyframes.iter().zip([0.0, middle, 0.4]) {
        assert!((key.values[0] - value).abs() < 1e-6, "{key:?} != {value}");
    }
    assert_eq!(curve.keyframes[0].out_interpolation, 3);
    assert_eq!(
        curve.keyframes[1].out_interpolation,
        if linear_after { 1 } else { 3 }
    );
    assert_eq!(
        curve.keyframes[2].in_interpolation,
        if linear_after { 1 } else { 3 }
    );
    // Independent native readback: .25 is held at zero; .75 is 85 percent
    // for control-2, or 100 percent for control-0/1. The edit changes those
    // responses to 40 percent (Linear) or 40 percent (Hold), respectively.
    assert_eq!(curve.keyframes[0].values[0], 0.0);
    let after = if linear_after {
        middle + (0.4 - middle) * 0.25
    } else {
        middle
    };
    assert!(
        (after
            - if middle == 1.0 && linear_after {
                0.85
            } else {
                middle
            })
        .abs()
            < 1e-6
    );
}

#[test]
fn native_hold_endpoint_flags_survive_editable_import_and_fresh_export() {
    let source = oracle();
    let document = to_structural_fx_document(&source, Some(1))
        .unwrap()
        .document;
    let tracks: Vec<_> = document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|target| target.property_type() == PropType::Opacity)
        })
        .filter_map(|entry| entry.animator.keyframe_track())
        .collect();
    assert_eq!(tracks.len(), 3);
    for track in tracks {
        let keys = track.keyframes();
        assert_eq!(
            keys.iter()
                .map(|key| key.layer_time().as_millis())
                .collect::<Vec<_>>(),
            [-500, 500, 1500]
        );
        assert_eq!(keys[1].easing(), PropertyKeyframeEasing::Hold);
        assert_eq!(keys[1].value(), &PropertyValue::Float(100.0));
    }
    let output = to_aep(&document).unwrap();
    let generated = read_project(&output.bytes).unwrap();
    let curves = opacity_curves(&generated);
    assert_eq!(curves.len(), 3, "{:?}", output.diagnostics);
    for curve in &curves {
        assert_curve(curve, 1.0, curve.keyframes[1].out_interpolation == 1);
    }
    assert_eq!(
        curves
            .iter()
            .filter(|curve| curve.keyframes[1].out_interpolation == 1)
            .count(),
        1
    );
    let mut edited = document.to_json_value().unwrap();
    let entries = edited["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    let mut edits = 0;
    for entry in entries {
        if entry["target"]["propertyType"] == "opacity" {
            entry["animator"]["keyframes"][1]["value"] =
                serde_json::to_value(PropertyValue::Float(40.0)).unwrap();
            edits += 1;
        }
    }
    assert_eq!(edits, 3);
    let edited_output = export(edited);
    let edited_project = read_project(&edited_output.bytes).unwrap();
    let edited_curves = opacity_curves(&edited_project);
    assert_eq!(edited_curves.len(), 3, "{:?}", edited_output.diagnostics);
    for curve in &edited_curves {
        assert_curve(curve, 0.4, curve.keyframes[1].out_interpolation == 1);
    }
}
