//! AE transfer modes and composition-level settings mapped to existing FX controls.

use fx_schema::{BlendMode, MotionBlurSettings, TrackMatteType};

use crate::{
    schema::{CompositionRecord, layer_records::LayerRecord},
    structure::Composition,
};

/// Adobe's binary PF_Xfer values are not the scripting enum or FX ordinals.
pub(super) fn blend_mode(value: u8) -> Option<BlendMode> {
    Some(match value {
        0 | 2 => BlendMode::Normal,
        4 | 29 => BlendMode::Add,
        5 => BlendMode::Multiply,
        6 => BlendMode::Screen,
        7 => BlendMode::Overlay,
        8 => BlendMode::SoftLight,
        9 => BlendMode::HardLight,
        10 => BlendMode::Darken,
        11 => BlendMode::Lighten,
        12 => BlendMode::ClassicDifference,
        13 => BlendMode::Hue,
        14 => BlendMode::Saturation,
        15 => BlendMode::Color,
        16 => BlendMode::Luminosity,
        23 => BlendMode::ClassicColorDodge,
        24 => BlendMode::ClassicColorBurn,
        25 => BlendMode::Exclusion,
        26 => BlendMode::Difference,
        27 => BlendMode::ColorDodge,
        28 => BlendMode::ColorBurn,
        30 => BlendMode::LinearBurn,
        31 => BlendMode::LinearLight,
        32 => BlendMode::VividLight,
        33 => BlendMode::PinLight,
        34 => BlendMode::HardMix,
        35 => BlendMode::LighterColor,
        36 => BlendMode::DarkerColor,
        37 => BlendMode::Subtract,
        38 => BlendMode::Divide,
        _ => return None,
    })
}

pub(super) fn motion_blur(
    record: &CompositionRecord,
) -> Result<MotionBlurSettings, serde_json::Error> {
    let (samples, adaptive_limit) = record.motion_blur_samples();
    // Reuse the persisted type's exact range validation rather than creating
    // a document that will fail to reopen after a permissive field assignment.
    serde_json::from_value(serde_json::json!({
        "enabled": record.flags()[1] & 8 != 0,
        "shutterAngle": record.shutter_angle(),
        "shutterPhase": record.shutter_phase(),
        "samplesPerFrame": samples,
        "adaptiveSampleLimit": adaptive_limit,
    }))
}

/// Native layer selected as a record's track matte, before index resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MatteLayer {
    /// Legacy records have no selector field: AE uses the preceding layer.
    Preceding,
    /// Modern records name their provider explicitly.
    Id(u32),
}

/// The selected matte layer, or `None` when the record has no active matte.
///
/// Modern AE keeps the last matte mode while its layer selector is None, so a
/// zero selector disables the stored mode instead of choosing the layer above.
pub(super) fn matte_layer(record: &LayerRecord) -> Option<MatteLayer> {
    if record.track_matte_type() == 0 {
        return None;
    }
    match record.matte_layer_id_raw() {
        None => Some(MatteLayer::Preceding),
        Some(0) => None,
        Some(id) => Some(MatteLayer::Id(id)),
    }
}

/// Resolve within this source composition, before assigning occurrence-local FX IDs.
pub(super) fn matte_source(
    comp: &Composition,
    index: usize,
) -> Result<Option<(usize, TrackMatteType)>, String> {
    let record = &comp.layers[index].record;
    let Some(selected) = matte_layer(record) else {
        return Ok(None);
    };
    let mode = match record.track_matte_type() {
        1 => TrackMatteType::Alpha,
        2 => TrackMatteType::AlphaInverted,
        3 => TrackMatteType::Luma,
        4 => TrackMatteType::LumaInverted,
        value => return Err(format!("unknown track matte type {value}; matte omitted")),
    };
    let source = match selected {
        MatteLayer::Id(id) => comp
            .layers
            .iter()
            .position(|layer| layer.record.id() == id)
            .ok_or_else(|| format!("missing matte layer {id}; matte omitted"))?,
        MatteLayer::Preceding => index
            .checked_sub(1)
            .ok_or_else(|| "legacy matte has no preceding source; matte omitted".to_owned())?,
    };
    if source == index {
        return Err("self-referencing matte omitted".into());
    }
    Ok(Some((source, mode)))
}

#[cfg(test)]
mod tests {
    use fx_schema::{BlendMode, GroupLayer, LayerData as FxLayer, TrackMatteType};

    use crate::{
        structure::{ItemKind, StructuralProject, read_project},
        structure_document::to_structural_fx_document,
        writer::{
            CompositionSpec, LayerSpec, NativeCameraSpec, SolidLayerSpec, SolidTransform,
            write_composition,
        },
    };

    fn root(layers: &[fx_schema::Layer]) -> &GroupLayer {
        let FxLayer::Group(group) = layers[0].data() else {
            panic!("expected composition Group");
        };
        group
    }

    #[test]
    fn fixture_hashes_match_pinned_native_sources() {
        use sha2::{Digest, Sha256};
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compositing");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("provenance.json")).unwrap()).unwrap();
        for entry in manifest["files"].as_array().unwrap() {
            let bytes = crate::test_fixtures::read(dir.join(entry["path"].as_str().unwrap()));
            assert_eq!(bytes.len() as u64, entry["size"].as_u64().unwrap());
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                entry["sha256"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn unknown_blend_modes_are_not_silently_cast() {
        use super::blend_mode;
        for unknown in [1, 3, 17, 18, 19, 20, 21, 22, 39, 255] {
            assert_eq!(blend_mode(unknown), None);
        }
        for (source, target) in [
            (12, BlendMode::ClassicDifference),
            (23, BlendMode::ClassicColorDodge),
            (24, BlendMode::ClassicColorBurn),
            (25, BlendMode::Exclusion),
            (26, BlendMode::Difference),
            (27, BlendMode::ColorDodge),
            (28, BlendMode::ColorBurn),
            (29, BlendMode::Add),
            (30, BlendMode::LinearBurn),
            (31, BlendMode::LinearLight),
            (32, BlendMode::VividLight),
            (33, BlendMode::PinLight),
            (34, BlendMode::HardMix),
            (35, BlendMode::LighterColor),
            (36, BlendMode::DarkerColor),
            (37, BlendMode::Subtract),
            (38, BlendMode::Divide),
        ] {
            assert_eq!(blend_mode(source), Some(target));
        }
    }

    fn patch(layer: &mut crate::structure::Layer, offset: usize, bytes: &[u8]) {
        let mut raw = layer.record.encode();
        raw[offset..offset + bytes.len()].copy_from_slice(bytes);
        layer.record = crate::schema::layer_records::LayerRecord::decode(&raw).unwrap();
    }

    /// The same record in the legacy 160-byte layout, which has no matte-layer field.
    fn legacy(layer: &mut crate::structure::Layer) {
        layer.record =
            crate::schema::layer_records::LayerRecord::decode(&layer.record.raw_bytes()[..160])
                .unwrap();
    }

    #[test]
    fn synthetic_visible_matte_keeps_its_paint_copy_and_uses_fresh_ids() {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/compositing/trackMatteType.aep"
        ))
        .unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
            panic!("comp")
        };
        let flags = comp.layers[1].record.raw_bytes()[39] | 1;
        patch(&mut comp.layers[1], 39, &[flags]);
        let result = to_structural_fx_document(&project, Some(1)).unwrap();
        let layers = &root(result.document.composition().layers()).layers;
        assert_eq!(layers.len(), 3);
        let FxLayer::Group(painted) = layers[1].data() else {
            panic!("Group")
        };
        assert!(!painted.is_hidden);
        let matte = root(layers).track_matte.as_ref().unwrap();
        assert_ne!(matte.layer, painted.id);
        assert_eq!(matte.layer, layers[2].id());
        result.document.to_json_vec().unwrap();
    }

    #[test]
    fn synthetic_matte_modes_and_invalid_edges_are_explicit() {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/compositing/trackMatteType.aep"
        ))
        .unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
            panic!("comp")
        };
        for (code, expected) in [
            (1, TrackMatteType::Alpha),
            (2, TrackMatteType::AlphaInverted),
            (3, TrackMatteType::Luma),
            (4, TrackMatteType::LumaInverted),
        ] {
            patch(&mut comp.layers[0], 107, &[code]);
            assert_eq!(super::matte_source(comp, 0).unwrap(), Some((1, expected)));
        }
        patch(&mut comp.layers[0], 160, &999_u32.to_be_bytes());
        assert!(
            super::matte_source(comp, 0)
                .unwrap_err()
                .contains("missing")
        );
        let id = comp.layers[0].record.id();
        patch(&mut comp.layers[0], 160, &id.to_be_bytes());
        assert!(
            super::matte_source(comp, 0)
                .unwrap_err()
                .contains("self-referencing")
        );
        // A modern zero selector is AE's "no matte layer" state. The stored
        // mode, including an unknown one, must not select the layer above.
        patch(&mut comp.layers[0], 160, &0_u32.to_be_bytes());
        comp.layers.swap(0, 1);
        assert_eq!(super::matte_source(comp, 1).unwrap(), None);
        patch(&mut comp.layers[1], 107, &[9]);
        assert_eq!(super::matte_source(comp, 1).unwrap(), None);
        patch(&mut comp.layers[1], 107, &[4]);
        // Only a record without the selector field uses the preceding layer.
        legacy(&mut comp.layers[1]);
        assert_eq!(
            super::matte_source(comp, 1).unwrap(),
            Some((0, TrackMatteType::LumaInverted))
        );
        comp.layers.swap(0, 1);
        assert!(
            super::matte_source(comp, 0)
                .unwrap_err()
                .contains("preceding")
        );
    }

    /// Native CAP2 comp 625 state: providers 650/640 keep AE's last Alpha mode
    /// with a zero selector while ball 646 and text 795 select them explicitly.
    #[test]
    fn modern_zero_selector_on_a_provider_keeps_its_explicit_consumer_matte() {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/compositing/trackMatteType.aep"
        ))
        .unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
            panic!("comp")
        };
        let provider_id = comp.layers[1].record.id();
        assert_eq!(comp.layers[0].record.matte_layer_id(), Some(provider_id));
        patch(&mut comp.layers[1], 107, &[1]);
        patch(&mut comp.layers[1], 160, &0_u32.to_be_bytes());
        assert_eq!(super::matte_source(comp, 1).unwrap(), None);

        let result = to_structural_fx_document(&project, Some(1)).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("cyclic")),
            "a zero selector must not form a false provider-consumer cycle"
        );
        let layers = &root(result.document.composition().layers()).layers;
        let consumer = root(layers);
        let matte = consumer
            .track_matte
            .as_ref()
            .expect("explicit consumer matte must survive");
        assert_eq!(matte.mode, TrackMatteType::Alpha);
        let helper = layers
            .iter()
            .find(|layer| layer.id() == matte.layer)
            .expect("provider helper");
        let FxLayer::Group(helper) = helper.data() else {
            panic!("provider helper Group")
        };
        assert!(helper.name.ends_with("(matte source)"));
        assert!(helper.track_matte.is_none(), "provider sample has no matte");
        let providers: Vec<_> = layers
            .iter()
            .filter_map(|layer| match layer.data() {
                FxLayer::Group(group) if group.id != consumer.id => Some(group),
                _ => None,
            })
            .collect();
        assert!(!providers.is_empty());
        assert!(
            providers.iter().all(|group| group.track_matte.is_none()),
            "the stored Alpha mode must not consume the layer above"
        );
        result.document.to_json_vec().unwrap();
    }

    fn solid(name: &str, color: [f32; 3]) -> LayerSpec {
        LayerSpec::Solid(SolidLayerSpec {
            name: name.into(),
            width: 64,
            height: 64,
            color,
            transform: SolidTransform {
                anchor: [32.0, 32.0],
                position: [320.0, 180.0],
                scale: [100.0, 100.0],
                rotation: 0.0,
                opacity: 100.0,
            },
        })
    }

    fn camera_matte_project(
        camera_between_participants: bool,
        legacy_camera_source: bool,
    ) -> StructuralProject {
        let spec = CompositionSpec {
            name: "Camera matte indices".into(),
            width: 640,
            height: 360,
            duration_frames: 24,
        };
        let camera = LayerSpec::Camera(NativeCameraSpec::root(640, 360));
        let target = solid("Target", [1.0, 0.0, 0.0]);
        let provider = solid("Provider", [0.0, 1.0, 0.0]);
        let unrelated = solid("Unrelated", [0.0, 0.0, 1.0]);
        let layers = if legacy_camera_source {
            vec![camera, target, unrelated]
        } else if camera_between_participants {
            vec![target, camera, provider, unrelated]
        } else {
            vec![camera, target, provider, unrelated]
        };
        let mut project = read_project(&write_composition(&spec, &layers).unwrap()).unwrap();
        let ItemKind::Composition(comp) = &mut project.items[0].kind else {
            panic!("composition")
        };
        let provider_id = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Provider")
            .map(|layer| layer.record.id());
        let target = comp
            .layers
            .iter_mut()
            .find(|layer| layer.name.as_ref() == "Target")
            .expect("target layer");
        patch(target, 107, &[1]);
        match provider_id {
            Some(id) => patch(target, 160, &id.to_be_bytes()),
            None => legacy(target),
        }
        project
    }

    fn assert_explicit_camera_matte(camera_between_participants: bool) {
        let project = camera_matte_project(camera_between_participants, false);
        let result = to_structural_fx_document(&project, Some(1)).unwrap();
        let layers = &root(result.document.composition().layers()).layers;
        assert!(
            layers
                .iter()
                .all(|layer| layer.data().name() != "FX root projection"),
            "canonical camera must be removed"
        );
        let target = layers
            .iter()
            .find_map(|layer| match layer.data() {
                FxLayer::Group(group) if group.name == "Target" => Some(group),
                _ => None,
            })
            .expect("target group");
        let matte = target.track_matte.as_ref().expect("target matte");
        let helper = layers
            .iter()
            .find(|layer| layer.id() == matte.layer)
            .expect("fresh provider helper");
        assert_eq!(helper.data().name(), "Provider (matte source)");
        let unrelated = layers
            .iter()
            .find_map(|layer| match layer.data() {
                FxLayer::Group(group) if group.name == "Unrelated" => Some(group),
                _ => None,
            })
            .expect("unrelated group");
        assert!(unrelated.track_matte.is_none());
        result.document.to_json_vec().unwrap();
    }

    #[test]
    fn generated_camera_before_explicit_matte_keeps_original_index_mapping() {
        assert_explicit_camera_matte(false);
    }

    #[test]
    fn generated_camera_between_explicit_matte_participants_keeps_original_index_mapping() {
        assert_explicit_camera_matte(true);
    }

    #[test]
    fn generated_camera_used_as_legacy_matte_provider_is_not_normalized_away() {
        let project = camera_matte_project(false, true);
        let result = to_structural_fx_document(&project, Some(1)).unwrap();
        let layers = &root(result.document.composition().layers()).layers;
        assert!(
            layers
                .iter()
                .any(|layer| layer.data().name() == "FX root projection"),
            "legacy matte provider camera must remain explicit"
        );
        let target = layers
            .iter()
            .find_map(|layer| match layer.data() {
                FxLayer::Group(group) if group.name == "Target" => Some(group),
                _ => None,
            })
            .expect("target group");
        let matte = target.track_matte.as_ref().expect("legacy matte relation");
        let helper = layers
            .iter()
            .find(|layer| layer.id() == matte.layer)
            .expect("legacy camera helper");
        assert_eq!(helper.data().name(), "FX root projection (matte source)");
        result.document.to_json_vec().unwrap();
    }

    #[test]
    fn native_blend_modes_are_editable() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/compositing/blendingMode.aep"
        ))
        .unwrap();
        for (id, expected) in [
            (30, BlendMode::Add),
            (1, BlendMode::Multiply),
            (16, BlendMode::Screen),
        ] {
            let result = to_structural_fx_document(&project, Some(id)).unwrap();
            let layer = root(&root(result.document.composition().layers()).layers);
            assert_eq!(layer.blend_mode, expected, "source comp {id}");
        }
    }

    #[test]
    fn native_track_mattes_reference_live_occurrence_sources() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/compositing/trackMatteType.aep"
        ))
        .unwrap();
        for (id, mode) in [(1, TrackMatteType::Alpha), (18, TrackMatteType::Luma)] {
            let result = to_structural_fx_document(&project, Some(id)).unwrap();
            let layers = &root(result.document.composition().layers()).layers;
            let target = root(layers);
            let matte = target
                .track_matte
                .as_ref()
                .expect("native matte was discarded");
            assert_eq!(matte.mode, mode);
            let source = layers
                .iter()
                .find(|layer| layer.id() == matte.layer)
                .expect("matte must resolve within its occurrence");
            let FxLayer::Group(source) = source.data() else {
                panic!("expected source Group")
            };
            assert!(
                !source.is_hidden,
                "AE-disabled matte must remain sampleable"
            );
            assert!(
                !source.layers.is_empty(),
                "matte content must be editable, not a placeholder"
            );
        }
    }

    #[test]
    fn native_shutter_settings_are_preserved_even_when_disabled() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/compositing/shutterAngle.aep"
        ))
        .unwrap();
        for (id, angle) in [(1, 180.0), (13, 360.0)] {
            let result = to_structural_fx_document(&project, Some(id)).unwrap();
            let settings = result.document.composition().motion_blur();
            assert!(!settings.enabled);
            assert_eq!(settings.shutter_angle.value(), angle);
            assert_eq!(settings.shutter_phase, 0.0);
            assert_eq!(settings.samples_per_frame.value(), 16.0);
            assert_eq!(settings.adaptive_sample_limit.value(), 128.0);
        }
    }
}
