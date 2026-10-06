use fx_schema::{Position, PropType, PropertyValue};
use sha2::{Digest, Sha256};

use super::super::*;

const SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/effects/radial_wipe_half_plane.aep");

fn find_owner<'a>(group: &'a GroupLayer, identity: &str) -> Option<&'a GroupLayer> {
    if group.description.contains(identity) {
        return Some(group);
    }
    group
        .layers
        .iter()
        .filter_map(|layer| match layer.data() {
            FxLayer::Group(child) => find_owner(child, identity),
            _ => None,
        })
        .next()
}

#[test]
fn radial_wipe_native_fresh_import_has_editable_half_plane_not_omission() {
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE)),
        "672afb582fa532bff7c4a3c610891da762d962bd66bd22845f147b45cfeb9f6a"
    );
    assert_public_half_plane(SOURCE, [35.0, 24.0], [0.0, 90.0]);
}

fn assert_public_half_plane(source: &[u8], center: [f64; 2], angles: [f64; 2]) {
    let project = crate::structure::read_project(source).unwrap();
    let converted = to_structural_fx_document(&project, Some(22)).unwrap();
    let FxLayer::Group(root) = converted.document.composition().layers()[0].data() else {
        panic!("composition root")
    };
    let owner = root
        .layers
        .iter()
        .filter_map(|layer| match layer.data() {
            FxLayer::Group(group) => find_owner(group, "comp=22 layer=34"),
            _ => None,
        })
        .next()
        .expect("native owner occurrence");
    assert_eq!(
        owner.masks.len(),
        1,
        "native effect was omitted: {:?}",
        converted.diagnostics
    );
    let mask = &owner.masks[0];
    assert_eq!(mask.mode, fx_schema::layer::MaskMode::Add);
    assert!(!mask.inverted);
    assert_eq!(mask.opacity.value(), 1.0);
    assert_eq!(mask.feather, [0.0, 0.0]);
    let guide_id = mask.layer.unwrap();
    let guide = owner
        .layers
        .iter()
        .find(|layer| layer.id() == guide_id)
        .unwrap();
    let FxLayer::Shape(shape) = guide.data() else {
        panic!("editable Shape guide")
    };
    assert_eq!(shape.transform.position, Position::TwoD(center));
    assert_eq!(shape.shape.path.commands.len(), 5);
    assert!(shape.shape.fills.is_empty() && shape.shape.strokes.is_empty());
    let entry = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| {
            entry.target.as_property().is_some_and(|p| {
                p.layer_id() == guide_id && p.property_type() == PropType::Rotation
            })
        })
        .unwrap();
    let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
        panic!("editable Rotation keys")
    };
    assert_eq!(track.keyframes().len(), 2);
    for ((key, angle), time) in track.keyframes().iter().zip(angles).zip([0, 1000]) {
        assert_eq!(key.layer_time().as_millis(), time);
        assert_eq!(key.value(), &PropertyValue::Float(angle));
        // FX easing belongs to the arriving key, unlike AE's outgoing flag.
        let easing = if time == 0 {
            fx_schema::PropertyKeyframeEasing::Linear
        } else {
            fx_schema::PropertyKeyframeEasing::Hold
        };
        assert_eq!(key.easing(), easing, "arriving key at {time}ms");
    }
}

#[test]
fn radial_wipe_native_direct_angle_keys_import_as_editable_hold_rotation() {
    let source = include_bytes!("../../../tests/fixtures/effects/radial_wipe_keyed.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        "7df3d2c575ec1d67fb5ea6076e3133b03c1d4fbafbf2bb0cd404854fd8fd32d7"
    );
    assert_public_half_plane(source, [41.0, 17.0], [30.0, 120.0]);
}

#[test]
fn radial_wipe_edited_adobe_source_imports_current_center_and_hold_angles() {
    let source = include_bytes!("../../../tests/fixtures/effects/radial_wipe_edited.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        "8bb3d334d53dbff0c9fd75cf376582d1ee5a99c11ea1915f9db2f9407c9b9f26"
    );
    assert_public_half_plane(source, [41.0, 17.0], [30.0, 120.0]);
}
