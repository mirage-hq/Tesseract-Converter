//! PR #4442 general import contracts.
//!
//! Native inputs are independently authored and hash-pinned. Mutated records are
//! explicitly supplemental fault injection, not independent Adobe evidence.

use super::*;
use crate::{
    properties::{NumericKeyframe, NumericProperty, NumericValueKind},
    rifx::Chunk,
};
use fx_schema::{LayerId, PropType, PropertyTarget, PropertyValue};
use sha2::{Digest, Sha256};

const ESSENTIAL: &[u8] =
    include_bytes!("../../../tests/fixtures/essential/multiple_controllers.aep");
const MASKS: &[u8] = include_bytes!("../../../tests/fixtures/masks/import_mask_controls.aep");
const PATH_KEYS: &[u8] =
    include_bytes!("../../../tests/fixtures/path-animation/import_path_key_cases.aep");
const CLOCKS: &[u8] =
    include_bytes!("../../../tests/fixtures/properties/import_temporal_clock_cases.aep");
const MATTES: &[u8] =
    include_bytes!("../../../tests/fixtures/compositing/import_track_matte_cases.aep");
const AUDIO: &[u8] =
    include_bytes!("../../../tests/fixtures/media/import_audio_media_controls.aep");

fn pinned(bytes: &[u8], len: usize, sha: &str) -> StructuralProject {
    assert_eq!(bytes.len(), len);
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), sha);
    read_project(bytes).unwrap()
}

fn groups(group: &GroupLayer) -> Vec<&GroupLayer> {
    fn visit<'a>(group: &'a GroupLayer, output: &mut Vec<&'a GroupLayer>) {
        output.push(group);
        for layer in &group.layers {
            if let FxLayer::Group(child) = layer.data() {
                visit(child, output);
            }
        }
    }
    let mut output = Vec::new();
    visit(group, &mut output);
    output
}

fn remove_one_override_uuid(chunks: &mut [Chunk]) -> bool {
    for chunk in chunks {
        if chunk.list_kind() == Some(*b"OvG2") {
            let children = chunk.children_mut().unwrap();
            if let Some(index) = children
                .iter()
                .position(|child| child.list_kind() == Some(*b"CPrp"))
            {
                children.remove(index);
                return true;
            }
        }
        if let Some(children) = chunk.children_mut()
            && remove_one_override_uuid(children)
        {
            return true;
        }
    }
    false
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_essential_uuid_cardinality_mismatch_fails_closed_and_keeps_source_values() {
    let mut project = pinned(
        ESSENTIAL,
        141_003,
        "df08145c4be5d3547b1bb5650b48be777f8663d0d88dc472f65990f0ac37c6d3",
    );
    set_static_transform(
        &mut composition_mut(&mut project, 1).layers[0],
        &[("ADBE Opacity", &[0.35])],
    );
    let occurrence = &mut composition_mut(&mut project, 16).layers[0];
    assert!(remove_one_override_uuid(&mut occurrence.content));

    let converted = to_structural_fx_document(&project, Some(16)).unwrap();
    assert!(
        groups(root(&converted))
            .iter()
            .any(|group| group.transform.opacity.value() == 35.0)
    );
    assert!(
        !groups(root(&converted))
            .iter()
            .any(|group| group.transform.opacity.value() == 100.0 && group.name == "Controller 1")
    );
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("UUIDs")
            && diagnostic.message.contains("preorder nodes")
            && diagnostic
                .message
                .contains("original source values retained")
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_inactive_reverse_occurrence_keeps_identity_but_never_resurrects_pixels() {
    let project = fixture("layer_timing.aep");
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(include_bytes!(
                "../../../tests/fixtures/layers/layer_timing.aep"
            ))
        ),
        "8f7595ddca0a1aee842f7e35c79e00d605c2142ef13df041506dd29f4b32b784"
    );
    let converted = to_structural_fx_document(&project, Some(30)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    let content = as_group(&occurrence.layers[0]);
    assert_eq!(occurrence.name, "Time-Reverse");
    assert!(content.is_hidden);
    assert_ne!(content.playback.input_range().duration, Duration::ZERO);
    assert_eq!(
        content.playback,
        identity_playback(content.playback.input_range())
    );
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.limitation == Limitation::Timing)
    );
    assert!(
        !converted
            .document
            .to_json_value()
            .unwrap()
            .to_string()
            .contains("jsScript")
    );
}

fn force_before_zero(layer: &mut Layer) {
    let mut bytes = layer.record.encode();
    let denominator = u32::from_be_bytes(bytes[16..20].try_into().unwrap()).max(1);
    bytes[12..16].copy_from_slice(
        &(-20_i32.saturating_mul(i32::try_from(denominator).unwrap())).to_be_bytes(),
    );
    layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_before_zero_audio_and_matte_occurrences_keep_relationship_identity_while_inactive() {
    let mut mattes = pinned(
        MATTES,
        368_321,
        "7f597f51312021b3948472b56b137b9eeb7c7056086a2e95d150508607cb366a",
    );
    for layer in &mut composition_mut(&mut mattes, 1).layers {
        force_before_zero(layer);
    }
    let converted = to_structural_fx_document(&mattes, Some(1)).unwrap();
    let target = groups(root(&converted))
        .into_iter()
        .find(|group| group.name == "matted_foreground")
        .unwrap();
    let matte = target
        .track_matte
        .as_ref()
        .expect("inactive target retains matte identity");
    assert!(
        groups(root(&converted))
            .iter()
            .any(|group| group.id == matte.layer)
    );
    assert!(
        target
            .layers
            .iter()
            .filter_map(|layer| match layer.data() {
                FxLayer::Group(group) => Some(group),
                _ => None,
            })
            .any(|group| group.is_hidden && group.playback.input_range().duration != Duration::ZERO)
    );

    let mut audio = pinned(
        AUDIO,
        776_439,
        "36920d6bdc07dbb6292cc305327ace44efbacacb182dc18da3c979439f5473a8",
    );
    force_before_zero(&mut composition_mut(&mut audio, 2).layers[0]);
    let converted = to_structural_fx_document_with_assets(&audio, Some(2), &mut |_| true).unwrap();
    let wire = converted.document.to_json_value().unwrap().to_string();
    assert!(
        wire.contains("Audio"),
        "inactive occurrence retains its audio identity"
    );
    assert!(
        groups(root(&converted))
            .iter()
            .any(|group| group.is_hidden && group.playback.input_range().duration != Duration::ZERO)
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_animated_mask_storage_uses_defaults_not_stale_cdat_and_path_motion_stays_diagnosed() {
    let project = pinned(
        MASKS,
        1_178_491,
        "01380c1f8c5ebe486cd068e5dee50447870b86e4864fc842590a99386dd9b417",
    );
    for (comp_id, name, expected) in [
        (178, "MASK_FEATHER_KEYED", ([0.0, 0.0], 1.0, 0.0)),
        (194, "MASK_OPACITY_KEYED", ([0.0, 0.0], 1.0, 0.0)),
        (210, "MASK_EXPANSION_KEYED", ([0.0, 0.0], 1.0, 0.0)),
    ] {
        assert_eq!(project.item(comp_id).unwrap().name.as_str(), name);
        let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
        let mask = groups(root(&converted))
            .into_iter()
            .find_map(|group| group.masks.first())
            .unwrap();
        assert_eq!(
            (mask.feather, mask.opacity.value(), mask.expansion),
            expected,
            "{name}"
        );
        let wire = converted.document.to_json_value().unwrap();
        assert!(
            wire["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| {
                    entry["target"]["propertyName"]
                        == match comp_id {
                            178 => "feather",
                            194 => "opacity",
                            _ => "expansion",
                        }
                })
        );
    }

    let keyed_path = to_structural_fx_document(&project, Some(226)).unwrap();
    assert!(
        groups(root(&keyed_path))
            .iter()
            .any(|group| !group.masks.is_empty())
    );
    assert!(
        keyed_path
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Path")
                && diagnostic.message.contains("animation"))
    );
}

fn numeric_key(time_secs: f64, value: f64) -> NumericKeyframe {
    NumericKeyframe {
        time_secs,
        values: vec![value],
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![1.0],
        in_influence: vec![50.0],
        out_speed: vec![1.0],
        out_influence: vec![50.0],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_numeric_clocks_preserve_signed_owner_time_and_nonfinite_vertical_ease_fails_safe() {
    use crate::structure_document::animation::{
        NumericAnimationClock, NumericAnimationTarget, easing_for_key, numeric_entries,
    };

    let mut keys = vec![numeric_key(0.0, 0.0), numeric_key(2.0, 100.0)];
    keys[0].out_speed[0] = f64::MAX;
    let mut warnings = Vec::new();
    let easing = easing_for_key(&keys, 1, 0, f64::MAX, &mut warnings, "Opacity");
    assert_eq!(easing, fx_schema::PropertyKeyframeEasing::Linear);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("non-finite temporal ease"))
    );

    let numeric = NumericProperty {
        values: vec![0.0],
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        value_kind: NumericValueKind::Continuous,
        keyframes: vec![numeric_key(0.0, 10.0), numeric_key(2.0, 20.0)],
    };
    let target = NumericAnimationTarget::float(
        PropertyTarget::layer(LayerId::new(4442), PropType::Opacity),
        0,
        1.0,
    );
    let (entries, warnings) = numeric_entries(
        "signed owner clock",
        &numeric,
        &[target],
        NumericAnimationClock::source_local_rebased(1.0),
        &mut crate::structure_document::animation_budget::AnimationBudget::default(),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let track = entries[0].animator.keyframe_track().unwrap();
    assert_eq!(
        track
            .keyframes()
            .iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        [-1_000, 1_000]
    );
    assert_eq!(track.keyframes()[0].value(), &PropertyValue::Float(10.0));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_native_clock_targets_and_path_exclusion_are_pinned_not_inferred_from_roundtrip() {
    let clocks = pinned(
        CLOCKS,
        332_969,
        "d80216eacadb2ba319ea6434184ef6253f3337317e03d67720137edf92ac9cf9",
    );
    for (id, name) in [
        (1, "CLOCK_IDENTITY"),
        (16, "CLOCK_START_STRETCH"),
        (30, "CLOCK_REVERSE_BEZIER"),
        (44, "CLOCK_SHAPE_START_STRETCH"),
        (57, "CLOCK_MASK_START_STRETCH"),
    ] {
        assert_eq!(clocks.item(id).unwrap().name.as_str(), name);
        let converted = to_structural_fx_document(&clocks, Some(id)).unwrap();
        assert!(
            !converted
                .document
                .composition()
                .dynamics()
                .entries()
                .is_empty(),
            "{name}"
        );
    }

    let paths = pinned(
        PATH_KEYS,
        239_485,
        "363b3f616d2375a92e0f52e8e026dee28a2b8ce3b7c4b63ae8c2dfade6b50f5d",
    );
    for (id, name) in [(1, "PATH_LINEAR"), (17, "PATH_HOLD"), (32, "PATH_BEZIER")] {
        assert_eq!(paths.item(id).unwrap().name.as_str(), name);
        let converted = to_structural_fx_document(&paths, Some(id)).unwrap();
        assert!(
            converted
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Path")
                    && diagnostic.message.contains("animation"))
        );
        assert!(
            !converted
                .document
                .to_json_value()
                .unwrap()
                .to_string()
                .contains("jsScript")
        );
    }
}
