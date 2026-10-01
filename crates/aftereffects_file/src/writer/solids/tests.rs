use std::collections::HashSet;

use fx_schema::{GroupLayer, LayerData, PropType, PropertyValue};
use sha2::{Digest, Sha256};

use super::*;
use crate::{
    aep::Project,
    properties::{read_transform, root_runs, runs, unique_list},
    rifx::Chunk,
    structure::{ItemKind, StructuralProject, read_project},
    structure_document::to_structural_fx_document,
    writer::{KeyframeEasing, NumericKeyframe},
};

fn composition() -> CompositionSpec {
    CompositionSpec {
        name: "Solid roundtrip".into(),
        width: 640,
        height: 360,
        duration_frames: 72,
    }
}

fn solid() -> SolidLayerSpec {
    SolidLayerSpec {
        name: "Edited solid Ω".into(),
        width: 128,
        height: 64,
        color: [0.25, 0.5, 0.75],
        transform: SolidTransform {
            anchor: [13.0, 21.0],
            position: [91.0, -20.0],
            scale: [-50.0, 125.0],
            rotation: 17.5,
            opacity: 62.0,
        },
    }
}

fn layers(project: &StructuralProject) -> &[crate::structure::Layer] {
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("composition")
    };
    &comp.layers
}

fn group(layer: &fx_schema::Layer) -> &GroupLayer {
    let LayerData::Group(group) = layer.data() else {
        panic!("group")
    };
    group
}

fn property_chunks<'a>(content: &'a [Chunk], name: &str) -> &'a [Chunk] {
    let roots = root_runs(content).unwrap();
    let transform_run = roots
        .into_iter()
        .find(|(name, _)| *name == "ADBE Transform Group")
        .unwrap()
        .1;
    let transform = unique_list(transform_run, *b"tdgp").unwrap();
    let leaves = runs(transform).unwrap();
    unique_list(
        leaves.into_iter().find(|(key, _)| *key == name).unwrap().1,
        *b"tdbs",
    )
    .unwrap()
}

fn record(chunks: &[Chunk], id: [u8; 4]) -> &[u8] {
    chunks
        .iter()
        .find(|chunk| chunk.id() == id)
        .unwrap()
        .data_payload()
        .unwrap()
}

fn find_item(chunks: &[Chunk], id: u32) -> Option<&Chunk> {
    chunks.iter().find_map(|chunk| {
        if chunk.list_kind() == Some(*b"Item")
            && chunk
                .children()
                .is_some_and(|children| record(children, *b"iide") == id.to_le_bytes())
        {
            return Some(chunk);
        }
        chunk
            .children()
            .and_then(|children| find_item(children, id))
    })
}

#[test]
fn static_solid_records_preserve_native_fields_and_editable_transform() {
    let input = solid();
    let bytes = write_solid_composition(&composition(), std::slice::from_ref(&input)).unwrap();
    let native = read_project(&bytes).unwrap();
    assert_eq!(layers(&native).len(), 1);
    let layer = &layers(&native)[0];
    assert_eq!(layer.name.as_ref(), input.name);
    assert_eq!(layer.record.in_point(), Some(0.0));
    assert_eq!(layer.record.out_point(), Some(3.0));
    assert_eq!(layer.record.stretch(), Some(1.0));
    assert_eq!(layer.record.blend_mode(), 2);
    let source = native
        .item(layer.record.source_id())
        .unwrap()
        .solid
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!((source.width, source.height), (128, 64));
    assert_eq!(source.color, input.color);
    assert_eq!(source.pixel_aspect, (1, 1));
    let properties = read_transform(&layer.content).unwrap();
    for (name, values) in [
        ("ADBE Anchor Point", vec![13.0 / 128.0, 21.0 / 64.0, 0.0]),
        ("ADBE Position", vec![91.0, -20.0, 0.0]),
        ("ADBE Scale", vec![-0.5, 1.25, 1.0]),
        ("ADBE Rotate Z", vec![17.5]),
        ("ADBE Opacity", vec![0.62]),
    ] {
        let value = properties
            .iter()
            .find(|p| p.match_name == name)
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(!value.animated);
        assert!(!value.expression_present);
        assert_eq!(value.values, values);
    }
    let imported = to_structural_fx_document(&native, Some(1)).unwrap();
    let root = group(&imported.document.composition().layers()[0]);
    let occurrence = group(&root.layers[0]);
    assert_eq!(occurrence.transform.anchor_point, input.transform.anchor);
    assert_eq!(
        occurrence.transform.position,
        fx_schema::Position::TwoD(input.transform.position)
    );
    assert_eq!(occurrence.transform.scale, input.transform.scale);
    assert_eq!(occurrence.transform.rotation, input.transform.rotation);
    assert_eq!(
        occurrence.transform.opacity.value(),
        input.transform.opacity
    );
    let content = group(&occurrence.layers[0]);
    let LayerData::Rect(rect) = content.layers[0].data() else {
        panic!("editable solid")
    };
    assert_eq!(rect.rect.size, [128.0, 64.0]);
    assert_eq!(rect.rect.fill_color, [0.25, 0.5, 0.75, 1.0]);
}

#[test]
fn native_static_transform_probe_imports_pixel_anchor_and_values() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/effects/transform_probe.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/effects/transform_probe.aep");
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                "d669d0ca3d505ebaca545d6866185f6f6e69fe00256024bd0cb9ee22ba1a67d7"
            );
            let native = read_project(bytes).unwrap();
            let layer = &layers(&native)[0];
            assert_eq!(layer.record.id(), 15);
            assert_eq!(layer.record.source_id(), 14);
            let source = native
                .item(14)
                .unwrap()
                .solid
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            assert_eq!((source.width, source.height), (120, 80));
            let properties = read_transform(&layer.content).unwrap();
            for (name, expected) in [
                ("ADBE Anchor Point", vec![61.0 / 120.0, 41.0 / 80.0, 0.0]),
                ("ADBE Position", vec![159.0, 89.0, 0.0]),
                ("ADBE Scale", vec![0.9, 1.1, 1.0]),
                ("ADBE Rotate Z", vec![10.0]),
                ("ADBE Opacity", vec![0.9]),
            ] {
                let numeric = properties
                    .iter()
                    .find(|p| p.match_name == name)
                    .unwrap()
                    .numeric
                    .as_ref()
                    .unwrap();
                assert!(!numeric.animated, "{name}");
                for (actual, expected) in numeric.values.iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() < 1e-9,
                        "{name}: {actual} != {expected}"
                    );
                }
            }
            let imported = to_structural_fx_document(&native, Some(1)).unwrap();
            let root = group(&imported.document.composition().layers()[0]);
            let occurrence = group(&root.layers[0]);
            assert_eq!(occurrence.transform.anchor_point, [61.0, 41.0]);
            assert_eq!(
                occurrence.transform.position,
                fx_schema::Position::TwoD([159.0, 89.0])
            );
            for (actual, expected) in occurrence.transform.scale.iter().zip([90.0, 110.0]) {
                assert!((actual - expected).abs() < 1e-9);
            }
            assert_eq!(occurrence.transform.rotation, 10.0);
            assert_eq!(occurrence.transform.opacity.value(), 90.0);
        },
    );
    cases.finish();
}

#[test]
fn animated_solid_anchor_roundtrips_source_relative_keys_to_pixel_graph() {
    // Supplementary generated-project evidence; not an independent Adobe render.
    let spec = composition();
    let layer = solid();
    let duration = checked_duration(&spec).unwrap();
    let anchor = NumericTrack {
        keys: [([12.0, 20.0, 0.0], 0), ([60.0, 40.0, 0.0], 1000)]
            .into_iter()
            .map(|(values, time_millis)| NumericKeyframe {
                time_millis,
                values: values.to_vec(),
                easing: vec![KeyframeEasing::Linear],
                spatial_in: vec![0.0; 3],
                spatial_out: vec![0.0; 3],
            })
            .collect(),
    };
    let mut timeline = root::Timeline {
        next_id: 15,
        ..Default::default()
    };
    timeline.sources.push(source_item(&layer, 13).unwrap());
    timeline.layers.push(
        timeline_layer(
            &layer,
            14,
            13,
            duration,
            Some(&TransformAnimations {
                anchor: Some(anchor),
                ..Default::default()
            }),
        )
        .unwrap(),
    );
    let views = views::build_views(spec.width, spec.height, duration).unwrap();
    let bytes = root::build_project_with_timeline(
        &spec.name,
        spec.width,
        spec.height,
        duration,
        views,
        timeline,
    )
    .unwrap()
    .encode()
    .unwrap();
    let native = read_project(&bytes).unwrap();
    let property = read_transform(&layers(&native)[0].content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == "ADBE Anchor Point")
        .unwrap()
        .numeric
        .unwrap();
    assert_eq!(property.keyframes.len(), 2);
    for (key, expected) in property
        .keyframes
        .iter()
        .zip([[12.0 / 128.0, 20.0 / 64.0], [60.0 / 128.0, 40.0 / 64.0]])
    {
        for (actual, expected) in key.values.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-9);
        }
    }
    let imported = to_structural_fx_document(&native, Some(1)).unwrap();
    let entries = imported.document.composition().dynamics().entries();
    for (property, expected) in [
        (PropType::AnchorPointX, [12.0, 60.0]),
        (PropType::AnchorPointY, [20.0, 40.0]),
    ] {
        let entry = entries
            .iter()
            .find(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|target| target.property_type() == property)
            })
            .unwrap();
        let keys = entry.animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.len(), 2);
        for (key, expected) in keys.iter().zip(expected) {
            assert_eq!(key.value(), &PropertyValue::Float(expected));
        }
    }
}

#[test]
fn fresh_solid_source_registration_matches_native_grammar() {
    let native_bytes = include_bytes!("../../../tests/fixtures/effects/transform_probe.aep");
    let native = Project::parse(native_bytes).unwrap();
    let native_structural = read_project(native_bytes).unwrap();
    let native_layer = &layers(&native_structural)[0];
    let native_scale = property_chunks(&native_layer.content, "ADBE Scale");
    for bound in [*b"tdum", *b"tduM"] {
        assert_eq!(record(native_scale, bound), 0.0_f64.to_be_bytes());
    }

    let generated_bytes = write_solid_composition(&composition(), &[solid()]).unwrap();
    let generated = Project::parse(&generated_bytes).unwrap();
    let generated_structural = read_project(&generated_bytes).unwrap();
    let generated_layer = &layers(&generated_structural)[0];
    let generated_scale = property_chunks(&generated_layer.content, "ADBE Scale");
    for bound in [*b"tdum", *b"tduM"] {
        assert_eq!(record(generated_scale, bound), record(native_scale, bound));
    }

    let folder = generated
        .chunks
        .iter()
        .find(|c| c.list_kind() == Some(*b"Fold"))
        .unwrap();
    let children = folder.children().unwrap();
    let kinds: Vec<_> = children
        .iter()
        .map(|chunk| chunk.list_kind().unwrap_or(chunk.id()))
        .collect();
    assert_eq!(
        kinds,
        [
            *b"fdta", *b"Item", *b"FEE ", *b"fvdv", *b"fiop", *b"ftts", *b"foac", *b"fiac",
            *b"fipc", *b"fifl", *b"Item", *b"fvdv", *b"fiop", *b"ftts", *b"foac", *b"fiac",
            *b"fipc", *b"fifl"
        ]
    );
    let source = find_item(&generated.chunks, 13)
        .unwrap()
        .children()
        .unwrap();
    let native_source = find_item(&native.chunks, 14).unwrap().children().unwrap();
    assert_eq!(
        record(source, *b"ftgi"),
        [0_u32, 1, u32::MAX, 600]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(record(source, *b"ftgi"), record(native_source, *b"ftgi"));
    let pin = source
        .iter()
        .find(|c| c.list_kind() == Some(*b"Pin "))
        .unwrap()
        .children()
        .unwrap();
    let native_pin = native_source
        .iter()
        .find(|c| c.list_kind() == Some(*b"Pin "))
        .unwrap()
        .children()
        .unwrap();
    // Adobe assigns a source-specific GUID; fresh unassigned sources use zeros.
    assert_eq!(record(pin, *b"pgui"), &[0; 16]);
    assert_eq!(record(native_pin, *b"pgui").len(), 16);
    let clrs = pin
        .iter()
        .find(|c| c.list_kind() == Some(*b"CLRS"))
        .unwrap()
        .children()
        .unwrap();
    let native_clrs = native_pin
        .iter()
        .find(|c| c.list_kind() == Some(*b"CLRS"))
        .unwrap()
        .children()
        .unwrap();
    let kinds: Vec<_> = clrs.iter().map(Chunk::id).collect();
    assert_eq!(
        kinds,
        [
            *b"epid", *b"apid", *b"linl", *b"embp", *b"ipws", *b"dcui", *b"prgb", *b"Mcsp",
            *b"Utf8", *b"ocsp", *b"Utf8", *b"hdrm", *b"Utf8"
        ]
    );
    assert_eq!(kinds, native_clrs.iter().map(Chunk::id).collect::<Vec<_>>());
    for id in [
        *b"epid", *b"apid", *b"linl", *b"embp", *b"ipws", *b"dcui", *b"prgb", *b"Mcsp", *b"ocsp",
        *b"hdrm",
    ] {
        assert_eq!(record(clrs, id), record(native_clrs, id), "{id:?}");
    }
    assert_eq!(record(clrs, *b"epid"), &[255; 16]);
    assert_eq!(record(clrs, *b"linl"), 2_u32.to_le_bytes());
}

#[test]
fn timeline_order_and_source_ids_are_independent_and_deterministic() {
    let mut second = solid();
    second.name = "Second".into();
    second.width = 32;
    let inputs = [solid(), second];
    let bytes = write_solid_composition(&composition(), &inputs).unwrap();
    assert_eq!(
        bytes,
        write_solid_composition(&composition(), &inputs).unwrap()
    );
    assert_eq!(Project::parse(&bytes).unwrap().encode().unwrap(), bytes);
    let project = read_project(&bytes).unwrap();
    let mut ids: HashSet<u32> = (1..=12).collect();
    for (layer, input) in layers(&project).iter().zip(&inputs) {
        assert!(ids.insert(layer.record.id()));
        assert!(ids.insert(layer.record.source_id()));
        assert_eq!(layer.name.as_ref(), input.name);
        let source = project
            .item(layer.record.source_id())
            .unwrap()
            .solid
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!(source.width, input.width);
    }
    assert_eq!(layers(&project).len(), inputs.len());
}

#[test]
fn native_fixture_source_descriptor_is_reconstructed_without_runtime_templates() {
    let bytes = include_bytes!("../../../tests/fixtures/properties/transform_unseparated.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "1004b3ee82efd5e24b90ff67d8dd8c89a92c9a5ae554d69d96dc8847cdc61537"
    );
    let native = read_project(bytes).unwrap();
    let source = native
        .item(14)
        .unwrap()
        .solid
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    let input = SolidLayerSpec {
        name: "Independent solid".into(),
        width: source.width,
        height: source.height,
        color: source.color,
        transform: SolidTransform {
            anchor: [960.0, 540.0],
            position: [960.0, 540.0],
            scale: [100.0; 2],
            rotation: 0.0,
            opacity: 100.0,
        },
    };
    let spec = CompositionSpec {
        name: "Fresh".into(),
        width: 1920,
        height: 1080,
        duration_frames: 720,
    };
    let generated = read_project(&write_solid_composition(&spec, &[input]).unwrap()).unwrap();
    let generated_layer = &layers(&generated)[0];
    let generated_source = generated
        .item(generated_layer.record.source_id())
        .unwrap()
        .solid
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(generated_source, source);
    let original_layer = &layers(&native)[0];
    assert_eq!(
        generated_layer.record.quality(),
        original_layer.record.quality()
    );
    assert_eq!(
        generated_layer.record.blend_mode(),
        original_layer.record.blend_mode()
    );
    assert_eq!(
        generated_layer.record.stretch_fraction(),
        original_layer.record.stretch_fraction()
    );
    // The fixture establishes source fields, not that Adobe accepts the output.
}

fn large_timeline_inputs(count: usize) -> Vec<SolidLayerSpec> {
    (0..count)
        .map(|index| SolidLayerSpec {
            name: format!("Solid {index}"),
            ..solid()
        })
        .collect()
}

fn large_timeline_output(inputs: &[SolidLayerSpec], mixed: bool) -> Result<Vec<u8>, AepWriteError> {
    if mixed {
        let specs: Vec<_> = inputs
            .iter()
            .cloned()
            .map(super::super::rects::LayerSpec::Solid)
            .collect();
        super::super::rects::write_composition(&composition(), &specs)
    } else {
        write_solid_composition(&composition(), inputs)
    }
}

fn assert_large_timeline(count: usize, mixed: bool) {
    let inputs = large_timeline_inputs(count);
    let bytes = large_timeline_output(&inputs, mixed).unwrap();
    let project = read_project(&bytes).unwrap();
    assert_eq!(layers(&project).len(), count);
    let mut ids: HashSet<u32> = (1..=12).collect();
    for (layer, input) in layers(&project).iter().zip(&inputs) {
        assert!(ids.insert(layer.record.id()));
        assert!(ids.insert(layer.record.source_id()));
        assert_eq!(layer.name.as_ref(), input.name);
        let source = project.item(layer.record.source_id()).unwrap();
        assert_eq!(
            source.solid.as_ref().unwrap().as_ref().unwrap().width,
            input.width
        );
    }
}

#[test]
fn solid_id_allocation_rejects_overflow_without_wrapping() {
    assert_eq!(
        checked_solid_ids(u32::MAX - 2).unwrap(),
        (u32::MAX - 1, u32::MAX)
    );
    for source_id in [u32::MAX - 1, u32::MAX] {
        assert!(matches!(
            checked_solid_ids(source_id),
            Err(AepWriteError::Invalid("native ID overflow"))
        ));
    }
}

#[test]
fn large_timeline_solid_writer_preserves_513_layers() {
    assert_large_timeline(513, false);
}

#[test]
fn large_timeline_mixed_writer_preserves_513_layers() {
    assert_large_timeline(513, true);
}

#[test]
fn large_timeline_solid_writer_preserves_1657_layers() {
    assert_large_timeline(1657, false);
}

#[test]
fn large_timeline_mixed_writer_preserves_1657_layers() {
    assert_large_timeline(1657, true);
}

#[test]
fn no_layers_keeps_the_empty_writer_byte_identical() {
    assert_eq!(
        write_solid_composition(&composition(), &[]).unwrap(),
        super::super::write_empty_composition(&composition()).unwrap()
    );
}

#[test]
fn invalid_solids_are_rejected_before_any_project_is_published() {
    for input in [
        SolidLayerSpec {
            name: "bad\0name".into(),
            ..solid()
        },
        SolidLayerSpec {
            name: "é".repeat(128),
            ..solid()
        },
        SolidLayerSpec {
            width: 0,
            ..solid()
        },
        SolidLayerSpec {
            color: [f32::NAN, 0.0, 0.0],
            ..solid()
        },
        SolidLayerSpec {
            color: [1.1, 0.0, 0.0],
            ..solid()
        },
        SolidLayerSpec {
            transform: SolidTransform {
                rotation: f64::INFINITY,
                ..solid().transform
            },
            ..solid()
        },
        SolidLayerSpec {
            transform: SolidTransform {
                opacity: -1.0,
                ..solid().transform
            },
            ..solid()
        },
    ] {
        assert!(write_solid_composition(&composition(), &[input]).is_err());
    }
}
