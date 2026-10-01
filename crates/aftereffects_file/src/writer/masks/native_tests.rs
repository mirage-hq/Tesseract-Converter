//! Independent native mask grammar, not a writer/reader round-trip oracle.
use super::*;
use crate::properties::{data, root_runs, runs, unique_list};
use crate::structure::ItemKind;
use sha2::{Digest, Sha256};

fn native_and_fresh() -> (Vec<Chunk>, Chunk) {
    let source = include_bytes!("../../../tests/fixtures/adjustment/native_controls.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        "ed45319ed014ba979a9e0b4868aa635775f288fe851c39d9242aab097020f26f"
    );
    let project = crate::structure::read_project(source).unwrap();
    let ItemKind::Composition(comp) = &project.item(163).unwrap().kind else {
        panic!("native comp163")
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 180)
        .unwrap();
    let roots = root_runs(&layer.content).unwrap();
    let parade = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Mask Parade")
        .unwrap()
        .1;
    let atoms = runs(unique_list(parade, *b"tdgp").unwrap()).unwrap();
    let atom = atoms
        .iter()
        .find(|(name, _)| *name == "ADBE Mask Atom")
        .unwrap()
        .1;
    let native = unique_list(atom, *b"tdgp").unwrap().to_vec();
    let mask = NativeMaskSpec {
        name: "asymmetric-static-gate".into(),
        path: serde_json::from_value(serde_json::json!({"commands": [
            {"type":"moveTo","x":24,"y":18}, {"type":"lineTo","x":269,"y":37},
            {"type":"lineTo","x":244,"y":151}, {"type":"lineTo","x":61,"y":132},
            {"type":"lineTo","x":17,"y":81}, {"type":"close"}
        ]}))
        .unwrap(),
        path_track: None,
        source_size: [320, 180],
        mode: NativeMaskMode::Add,
        inverted: false,
        feather: [9.0, 4.0],
        opacity: 0.68,
        expansion: 6.0,
        feather_track: None,
        opacity_track: None,
        expansion_track: None,
    };
    (native, mask_properties(&mask).unwrap())
}

fn leaf<'a>(children: &'a [Chunk], name: &str) -> &'a [Chunk] {
    runs(children)
        .unwrap()
        .into_iter()
        .find(|(candidate, _)| *candidate == name)
        .unwrap()
        .1
}

fn descendant_data(children: &[Chunk], tag: [u8; 4]) -> Option<&[u8]> {
    for chunk in children {
        if chunk.id() == tag {
            return chunk.data_payload();
        }
        if let Some(value) = chunk
            .children()
            .and_then(|children| descendant_data(children, tag))
        {
            return Some(value);
        }
    }
    None
}

#[test]
fn native_mask_path_list_allocation_matches_independent_five_point_outline() {
    let (native, fresh) = native_and_fresh();
    let native_path = leaf(&native, "ADBE Mask Shape");
    let fresh_path = leaf(fresh.children().unwrap(), "ADBE Mask Shape");
    assert_eq!(
        descendant_data(fresh_path, *b"lhd3"),
        descendant_data(native_path, *b"lhd3")
    );
    assert_eq!(
        descendant_data(fresh_path, *b"shph"),
        descendant_data(native_path, *b"shph")
    );
    let expected = descendant_data(native_path, *b"ldat").unwrap();
    let actual = descendant_data(fresh_path, *b"ldat").unwrap();
    assert_eq!(actual.len(), expected.len());
    for (slot, (actual, expected)) in actual
        .chunks_exact(4)
        .zip(expected.chunks_exact(4))
        .enumerate()
    {
        let actual = f32::from_be_bytes(actual.try_into().unwrap());
        let expected = f32::from_be_bytes(expected.try_into().unwrap());
        assert!(
            (actual - expected).abs() < 0.000_001,
            "native path coordinate slot {slot}: {actual} != {expected}"
        );
    }
}

#[test]
fn mask_opacity_keys_use_normalized_storage_without_mutating_authoring_values() {
    let mut mask =
        NativeMaskSpec::crop_rectangle("Keyed mask", [320, 180], [0.0, 0.0, 320.0, 180.0]).unwrap();
    mask.opacity_track = Some(super::super::keyframes::Track {
        keys: vec![super::super::keyframes::Keyframe {
            time_millis: 250,
            values: vec![68.0],
            easing: vec![super::super::keyframes::Easing::Linear],
            spatial_in: vec![],
            spatial_out: vec![],
        }],
    });
    let fresh = mask_properties(&mask).unwrap();
    let property = unique_list(
        leaf(fresh.children().unwrap(), "ADBE Mask Opacity"),
        *b"tdbs",
    )
    .unwrap();
    let numeric = crate::properties::read_numeric(property).unwrap();
    assert_eq!(numeric.keyframes[0].time_secs, 0.25);
    assert_eq!(numeric.keyframes[0].values, [0.68]);
    let descriptor = data(property, *b"tdb4").unwrap();
    assert_eq!(&descriptor[56..61], &[0, 0, 0, 4, 6]);
    assert_eq!(mask.opacity_track.unwrap().keys[0].values, [68.0]);
}

#[test]
fn native_mask_numeric_records_match_independent_adobe_controls() {
    let (native, fresh) = native_and_fresh();
    for name in ["ADBE Mask Feather", "ADBE Mask Opacity", "ADBE Mask Offset"] {
        let expected = unique_list(leaf(&native, name), *b"tdbs").unwrap();
        let actual = unique_list(leaf(fresh.children().unwrap(), name), *b"tdbs").unwrap();
        for tag in [*b"tdsb", *b"tdb4", *b"cdat", *b"tdum", *b"tduM"] {
            assert_eq!(
                data(actual, tag).unwrap(),
                data(expected, tag).unwrap(),
                "{name} {tag:?}"
            );
        }
    }
}
