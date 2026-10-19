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

fn property(chunks: &[crate::rifx::Chunk], name: &str) -> Option<crate::rifx::Chunk> {
    for (index, chunk) in chunks.iter().enumerate() {
        if chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|v| v.starts_with(name.as_bytes()) && v.get(name.len()) == Some(&0))
            && let Some(found) = chunks[index + 1..]
                .iter()
                .take_while(|c| c.id() != *b"tdmn")
                .find(|c| c.list_kind() == Some(*b"tdbs"))
        {
            return Some(found.clone());
        }
        if let Some(found) = chunk.children().and_then(|c| property(c, name)) {
            return Some(found);
        }
    }
    None
}

fn replace_property(
    chunks: &mut [crate::rifx::Chunk],
    name: &str,
    replacement: &crate::rifx::Chunk,
) -> bool {
    for index in 0..chunks.len() {
        if chunks[index].id() == *b"tdmn"
            && chunks[index]
                .data_payload()
                .is_some_and(|v| v.starts_with(name.as_bytes()) && v.get(name.len()) == Some(&0))
            && let Some(target) = chunks[index + 1..]
                .iter_mut()
                .take_while(|c| c.id() != *b"tdmn")
                .find(|c| c.list_kind() == Some(*b"tdbs"))
        {
            *target = replacement.clone();
            return true;
        }
        if let Some(children) = chunks[index].children_mut()
            && replace_property(children, name, replacement)
        {
            return true;
        }
    }
    false
}

fn insert_wipe(
    chunks: &mut [crate::rifx::Chunk],
    completion: &crate::rifx::Chunk,
    first: bool,
) -> bool {
    for chunk in chunks {
        let Some(children) = chunk.children_mut() else {
            continue;
        };
        if let Some(start) = children.iter().position(|c| {
            c.id() == *b"tdmn"
                && c.data_payload()
                    .is_some_and(|v| v.starts_with(b"ADBE Radial Wipe\0"))
        }) {
            let end = children[start + 1..]
                .iter()
                .position(|c| c.id() == *b"tdmn")
                .map_or(children.len(), |i| start + 1 + i);
            let mut sibling = children[start..end].to_vec();
            assert!(replace_property(
                &mut sibling,
                "ADBE Radial Wipe-0001",
                completion
            ));
            let mut direction = property(&sibling, "ADBE Radial Wipe-0004").unwrap();
            let data = direction
                .children_mut()
                .unwrap()
                .iter_mut()
                .find(|c| c.id() == *b"cdat")
                .unwrap();
            let mut bytes = data.data_payload().unwrap().to_vec();
            let original = f64::from_be_bytes(bytes[..8].try_into().unwrap());
            bytes[..8].copy_from_slice(&(3.0 - original).to_be_bytes());
            *data = crate::rifx::Chunk::data(*b"cdat", bytes).unwrap();
            assert!(replace_property(
                &mut sibling,
                "ADBE Radial Wipe-0004",
                &direction
            ));
            children.splice(if first { start..start } else { end..end }, sibling);
            return true;
        }
        if insert_wipe(children, completion, first) {
            return true;
        }
    }
    false
}

fn sector_groups(group: &GroupLayer) -> Vec<&GroupLayer> {
    let mut groups = Vec::new();
    // A raster precomposition's own source-canvas mask is not a wipe sector.
    let canvas = group
        .layers
        .iter()
        .any(|layer| matches!(layer.data(), FxLayer::Rect(rect) if rect.name == "Composition canvas guide"));
    if !group.masks.is_empty() && !canvas {
        groups.push(group);
    }
    for layer in &group.layers {
        if let FxLayer::Group(child) = layer.data() {
            groups.extend(sector_groups(child));
        }
    }
    groups
}

#[test]
fn radial_wipe_completion_sector_keeps_editable_boundaries_in_both_directions() {
    for (direction, first_completion, expected, narrow_first) in [
        (1.0_f64, 100.0_f64, [210.0, 30.0], None),
        (2.0, 100.0, [30.0, 210.0], None),
        (1.0, 0.0, [-150.0, 30.0], None),
        (2.0, 0.0, [390.0, 210.0], None),
        (1.0, 0.0, [-150.0, 30.0], Some(true)),
        (1.0, 0.0, [-150.0, 30.0], Some(false)),
        (2.0, 0.0, [390.0, 210.0], Some(true)),
        (2.0, 0.0, [390.0, 210.0], Some(false)),
    ] {
        let union = first_completion == 0.0;
        let mut project = crate::structure::read_project(include_bytes!(
            "../../../tests/fixtures/effects/radial_wipe_keyed.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 22).unwrap().kind
        else {
            panic!("native composition")
        };
        let layer = comp
            .layers
            .iter_mut()
            .find(|l| l.record.id() == 34)
            .unwrap();
        let mut completion = property(&layer.content, "ADBE Radial Wipe-0002").unwrap();
        fn keys(chunks: &mut [crate::rifx::Chunk], first: f64) {
            for chunk in chunks {
                if chunk.id() == *b"ldat" {
                    let mut bytes = chunk.data_payload().unwrap().to_vec();
                    assert_eq!(bytes.len(), 96);
                    bytes[8..16].copy_from_slice(&first.to_be_bytes());
                    bytes[56..64].copy_from_slice(&50.0_f64.to_be_bytes());
                    *chunk = crate::rifx::Chunk::data(*b"ldat", bytes).unwrap();
                } else if let Some(children) = chunk.children_mut() {
                    keys(children, first);
                }
            }
        }
        keys(completion.children_mut().unwrap(), first_completion);
        let mut angle = property(&layer.content, "ADBE Radial Wipe-0001").unwrap();
        let children = angle.children_mut().unwrap();
        let cdat = children.iter_mut().find(|c| c.id() == *b"cdat").unwrap();
        let mut bytes = cdat.data_payload().unwrap().to_vec();
        bytes[..8].copy_from_slice(&30.0_f64.to_be_bytes());
        *cdat = crate::rifx::Chunk::data(*b"cdat", bytes).unwrap();
        assert!(replace_property(
            &mut layer.content,
            "ADBE Radial Wipe-0001",
            &completion
        ));
        assert!(replace_property(
            &mut layer.content,
            "ADBE Radial Wipe-0002",
            &angle
        ));
        let mut dir = angle.clone();
        let cdat = dir
            .children_mut()
            .unwrap()
            .iter_mut()
            .find(|c| c.id() == *b"cdat")
            .unwrap();
        let mut bytes = cdat.data_payload().unwrap().to_vec();
        bytes[..8].copy_from_slice(&direction.to_be_bytes());
        *cdat = crate::rifx::Chunk::data(*b"cdat", bytes).unwrap();
        fn insert_direction(chunks: &mut [crate::rifx::Chunk], dir: &crate::rifx::Chunk) -> bool {
            for chunk in chunks {
                let Some(children) = chunk.children_mut() else {
                    continue;
                };
                if children.iter().any(|c| {
                    c.id() == *b"tdmn"
                        && c.data_payload()
                            .is_some_and(|v| v.starts_with(b"ADBE Radial Wipe-0001\0"))
                }) && children.iter().any(|c| c.list_kind() == Some(*b"tdbs"))
                {
                    let mut name = vec![0; 40];
                    let native_name = b"ADBE Radial Wipe-0004\0";
                    name[..native_name.len()].copy_from_slice(native_name);
                    children.insert(0, dir.clone());
                    children.insert(0, crate::rifx::Chunk::data(*b"tdmn", name).unwrap());
                    return true;
                }
                if insert_direction(children, dir) {
                    return true;
                }
            }
            false
        }
        assert!(insert_direction(&mut layer.content, &dir));
        fn remove_center(chunks: &mut Vec<crate::rifx::Chunk>) {
            if let Some(start) = chunks.iter().position(|c| {
                c.id() == *b"tdmn"
                    && c.data_payload()
                        .is_some_and(|v| v.starts_with(b"ADBE Radial Wipe-0003\0"))
            }) {
                let end = chunks[start + 1..]
                    .iter()
                    .position(|c| c.id() == *b"tdmn")
                    .map_or(chunks.len(), |i| start + 1 + i);
                chunks.drain(start..end);
            }
            for chunk in chunks {
                if let Some(children) = chunk.children_mut() {
                    remove_center(children);
                }
            }
        }
        remove_center(&mut layer.content);
        if let Some(first) = narrow_first {
            let mut narrow = completion.clone();
            keys(narrow.children_mut().unwrap(), 100.0);
            assert!(insert_wipe(&mut layer.content, &narrow, first));
        }
        let converted = to_structural_fx_document(&project, Some(22)).unwrap();
        let FxLayer::Group(root) = converted.document.composition().layers()[0].data() else {
            panic!("root")
        };
        let owner = find_owner(root, "comp=22 layer=34").unwrap();
        use fx_schema::layer::MaskMode;
        assert!(owner.masks.is_empty());
        assert_eq!(
            owner.layers.len(),
            1,
            "source paint must be inside the wipe stages"
        );
        let groups = sector_groups(owner);
        assert_eq!(
            groups.len(),
            if narrow_first.is_some() { 2 } else { 1 },
            "{:?}",
            converted.diagnostics
        );
        for group in &groups {
            assert_eq!(group.masks.len(), 2);
            assert_eq!(group.layers.len(), 3, "one source Group and two guides");
            assert!(matches!(group.layers[0].data(), FxLayer::Group(_)));
            assert_eq!(group.masks[0].mode, MaskMode::Add);
            assert_eq!(group.transform.position, Position::TwoD([0.0, 0.0]));
            assert_eq!(group.transform.scale, [100.0, 100.0]);
            assert_eq!(group.transform.rotation, 0.0);
            assert_eq!(group.playback, owner.playback);
        }
        let owner = groups
            .into_iter()
            .find(|g| (g.masks[1].mode == MaskMode::Add) == union)
            .unwrap();
        for (index, mask) in owner.masks.iter().take(2).enumerate() {
            let guide = owner
                .layers
                .iter()
                .find(|l| Some(l.id()) == mask.layer)
                .unwrap();
            let FxLayer::Shape(shape) = guide.data() else {
                panic!("editable boundary")
            };
            assert_eq!(shape.parent, Some(owner.id));
            assert_eq!(shape.transform.position, Position::TwoD([48.0, 32.0]));
            assert_eq!(shape.shape.path.commands.len(), 5);
            assert!(shape.shape.fills.is_empty() && shape.shape.strokes.is_empty());
            if index == 0 {
                assert_eq!(
                    shape.transform.rotation,
                    if direction == 1.0 { 30.0 } else { 210.0 }
                );
            }
        }
        let moving = owner.masks[1].layer.unwrap();
        let entry = converted
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|e| {
                e.target.as_property().is_some_and(|p| {
                    p.layer_id() == moving && p.property_type() == PropType::Rotation
                })
            })
            .unwrap();
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
        else {
            panic!("editable angle keys")
        };
        assert_eq!(track.keyframes().len(), 2);
        for ((key, value), millis) in track.keyframes().iter().zip(expected).zip([0, 1000]) {
            assert_eq!(key.layer_time().as_millis(), millis);
            let PropertyValue::Float(actual) = key.value() else {
                panic!("scalar rotation")
            };
            assert!((actual - value).abs() < 1e-8);
            assert_eq!(
                key.easing(),
                if millis == 0 {
                    fx_schema::PropertyKeyframeEasing::Linear
                } else {
                    fx_schema::PropertyKeyframeEasing::Hold
                }
            );
        }
        assert!(
            !converted
                .document
                .to_json_value()
                .unwrap()
                .to_string()
                .contains("JsScript")
        );
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 22).unwrap().kind
        else {
            panic!("composition")
        };
        let layer = comp
            .layers
            .iter_mut()
            .find(|l| l.record.id() == 34)
            .unwrap();
        assert!(replace_property(
            &mut layer.content,
            "ADBE Radial Wipe-0001",
            &angle
        ));
        let mut invalid = completion.clone();
        keys(invalid.children_mut().unwrap(), -1.0);
        assert!(replace_property(
            &mut layer.content,
            "ADBE Radial Wipe-0001",
            &invalid
        ));
        let rejected = to_structural_fx_document(&project, Some(22)).unwrap();
        let FxLayer::Group(root) = rejected.document.composition().layers()[0].data() else {
            panic!("root")
        };
        let owner = find_owner(root, "comp=22 layer=34").unwrap();
        assert_eq!(
            sector_groups(owner).len(),
            if narrow_first.is_some() { 1 } else { 0 }
        );
        assert!(
            !owner.layers.is_empty(),
            "unsupported wipe retains source content"
        );
        assert!(rejected.diagnostics.iter().any(|d| {
            d.message
                .contains("Completion must be authored, finite and confined")
        }));
    }
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
