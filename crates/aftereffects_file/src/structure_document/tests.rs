use std::{collections::HashSet, fs, path::PathBuf};

use fx_schema as fx_composition;
use fx_schema::LayerData as FxLayer;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;
use crate::{schema::layer_records::LayerRecord, structure::read_project};

mod adjustment;
mod adobe_feature_additions;
mod adobe_vector_panel;
mod essential;
mod footage_anchor;
mod implemented_core_additions;
mod implemented_shape_additions;
mod implemented_text_additions;
mod media;
mod mosaic_derived;
mod mosaic_effect_alias;
mod native_general_cases;
mod native_layer_styles;
mod native_shape_cases;
mod native_sources;
mod native_text_cases;
mod parent_lookup;
mod pr4442_general_cases;
mod pr4442_text_cases;
mod pr4442_vector_cases;
mod still_layers_and_frame_fades;
mod text_control_links;

#[test]
fn generated_id_reservation_crosses_one_million_and_rejects_counter_overflow_atomically() {
    let mut cursor = 999_999;
    assert_eq!(reserve_ids(&mut cursor, 3), Some(999_999));
    assert_eq!(cursor, 1_000_002);

    cursor = u64::MAX - 2;
    assert_eq!(reserve_ids(&mut cursor, 2), Some(u64::MAX - 2));
    assert_eq!(cursor, u64::MAX);
    assert_eq!(reserve_ids(&mut cursor, 1), None);
    assert_eq!(cursor, u64::MAX);

    cursor = 17;
    assert_eq!(reserve_ids(&mut cursor, 0), None);
    assert_eq!(cursor, 17);
}

fn fixture(name: &str) -> StructuralProject {
    let path = fixture_dir().join(name);
    read_project(&fs::read(&path).unwrap())
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layers")
}

fn composition(project: &StructuralProject, id: u32) -> &Composition {
    let ItemKind::Composition(comp) = &project.item(id).unwrap().kind else {
        panic!("not a comp");
    };
    comp
}

fn composition_mut(project: &mut StructuralProject, id: u32) -> &mut Composition {
    let item = project.items.iter_mut().find(|item| item.id == id).unwrap();
    let ItemKind::Composition(comp) = &mut item.kind else {
        panic!("not a comp");
    };
    comp
}

fn root(converted: &StructuralConversion) -> &GroupLayer {
    as_group(&converted.document.composition().layers()[0])
}

fn assert_imported_canvas_matches_source(
    source: &Composition,
    converted: &StructuralConversion,
    context: &str,
) {
    assert_ne!(source.width, 0, "{context} source width");
    assert_ne!(source.height, 0, "{context} source height");
    let dimensions = converted.document.dimensions();
    assert_eq!(
        (dimensions.width, dimensions.height),
        (u32::from(source.width), u32::from(source.height)),
        "{context} imported canvas"
    );
}

fn as_group(layer: &fx_schema::Layer) -> &GroupLayer {
    let FxLayer::Group(group) = layer.data() else {
        panic!("not a Group");
    };
    group
}

fn has(converted: &StructuralConversion, limitation: Limitation) -> bool {
    converted
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.limitation == limitation)
}

fn patch(layer: &mut Layer, offset: usize, bytes: &[u8]) {
    let mut raw = layer.record.encode();
    raw[offset..offset + bytes.len()].copy_from_slice(bytes);
    layer.record = LayerRecord::decode(&raw).unwrap();
}

#[test]
fn native_solid_has_editable_pixels_and_source_transform() {
    let bytes = include_bytes!("../../tests/fixtures/properties/transform_unseparated.aep");
    let project = read_project(bytes).unwrap();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    assert_eq!(
        occurrence.layers.len(),
        1,
        "native solid must not remain an empty placeholder"
    );
    let content = as_group(&occurrence.layers[0]);
    let FxLayer::Rect(rect) = content.layers[0].data() else {
        panic!("solid must be an editable rectangle")
    };
    assert_eq!(rect.parent, Some(content.id));
    assert_eq!(
        rect.transform.position,
        fx_composition::Position::TwoD([0.0, 0.0])
    );
    assert_eq!(occurrence.transform.anchor_point, [960.0, 540.0]);
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([960.0, 540.0])
    );
    assert_eq!(occurrence.transform.scale, [100.0, 100.0]);
    assert_eq!(occurrence.transform.opacity.value(), 100.0);
    assert_eq!(
        occurrence.playback,
        identity_playback(occurrence.playback.input_range())
    );
    let bytes = converted.document.to_json_vec().unwrap();
    EditableFxCompositionDocument::from_json_slice(&bytes).unwrap();
}

// Native/runtime solid bounds are checked in fx_composition/tests/aep_solid_bounds.rs
// so this crate's dependency graph remains portable, including in tests.

// Synthetic property changes supplement (not replace) the immutable native cases.
fn set_static_transform(layer: &mut Layer, values: &[(&str, &[f64])]) {
    use crate::rifx::Chunk;
    fn name(value: &str) -> Chunk {
        let mut bytes = vec![0; 40];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        Chunk::data(*b"tdmn", bytes).unwrap()
    }
    let mut leaves = Vec::new();
    for (key, values) in values {
        let mut meta = vec![0; 124];
        meta[..2].copy_from_slice(&[0xdb, 0x99]);
        meta[3] = values.len() as u8;
        leaves.push(name(key));
        leaves.push(Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", meta).unwrap(),
                Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                Chunk::data(
                    *b"cdat",
                    values
                        .iter()
                        .enumerate()
                        .flat_map(|(index, value)| {
                            // The pinned native Solid stores Anchor fractions of its
                            // 1920×1080 source, unlike Position's pixel cdat.
                            let source_value = if *key == "ADBE Anchor Point" && index < 2 {
                                value / [1920.0, 1080.0][index]
                            } else {
                                *value
                            };
                            source_value.to_be_bytes()
                        })
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            ],
        ));
    }
    let root = layer
        .content
        .iter_mut()
        .find(|c| c.list_kind() == Some(*b"tdgp"))
        .unwrap();
    *root = Chunk::list(
        *b"tdgp",
        vec![name("ADBE Transform Group"), Chunk::list(*b"tdgp", leaves)],
    );
}

#[test]
fn null_transform_uses_physical_solid_anchor_without_rendering_solid_bounds() {
    fn convert_null(dimensions: [u16; 2], explicit_anchor: bool) -> GroupLayer {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/properties/transform_unseparated.aep"
        ))
        .unwrap();
        let layer = &mut composition_mut(&mut project, 1).layers[0];
        let source_id = layer.record.source_id();
        let mut record = layer.record.encode();
        record[38] |= 0x80;
        layer.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
        if explicit_anchor {
            // `set_static_transform` writes source-relative Solid storage using
            // this fixture's original 1920×1080 dimensions, so these values
            // encode the native [.5, .5] anchor used by 120×120 Intro nulls.
            set_static_transform(
                layer,
                &[
                    ("ADBE Anchor Point", &[960.0, 540.0, 0.0]),
                    ("ADBE Position", &[60.0, 60.0, 0.0]),
                ],
            );
        } else {
            set_static_transform(layer, &[("ADBE Position", &[60.0, 60.0, 0.0])]);
        }
        let layer = composition(&project, 1).layers[0].clone();
        let source = project
            .items
            .iter_mut()
            .find(|item| item.id == source_id)
            .expect("solid source");
        let solid = source
            .solid
            .as_mut()
            .expect("solid metadata")
            .as_mut()
            .expect("decoded solid metadata");
        solid.width = dimensions[0];
        solid.height = dimensions[1];
        assert_eq!(source_dimensions(Some(source), &layer), [0, 0]);
        assert_eq!(source_anchor_dimensions(Some(source), &layer), dimensions);

        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        as_group(&root(&converted).layers[0]).clone()
    }

    let explicit = convert_null([120, 120], true);
    assert_eq!(explicit.transform.anchor_point, [60.0, 60.0]);
    assert_eq!(
        explicit.transform.position,
        fx_composition::Position::TwoD([60.0, 60.0])
    );
    assert!(
        as_group(&explicit.layers[0]).layers.is_empty(),
        "physical anchor dimensions must not make a null render as a solid"
    );

    let implicit = convert_null([100, 100], false);
    assert_eq!(
        implicit.transform.anchor_point,
        [0.0, 0.0],
        "physical dimensions scale explicit null anchors only; the existing sparse-null zero default remains until independent native evidence proves otherwise"
    );
    assert_eq!(
        implicit.transform.position,
        fx_composition::Position::TwoD([60.0, 60.0])
    );
    assert!(as_group(&implicit.layers[0]).layers.is_empty());
}

#[test]
fn precomposition_anchor_storage_is_normalized_to_source_pixels() {
    let mut project = read_project(include_bytes!(
        "../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    let source_id = composition(&project, 1).layers[0].record.source_id();
    set_static_transform(
        &mut composition_mut(&mut project, 1).layers[0],
        &[("ADBE Anchor Point", &[23.0, 17.0, 0.0])],
    );
    let mut nested = composition(&project, 1).clone();
    nested.layers.clear();
    let source = project
        .items
        .iter_mut()
        .find(|item| item.id == source_id)
        .unwrap();
    source.kind = ItemKind::Composition(Box::new(nested));
    source.solid = None;
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    assert_eq!(occurrence.transform.anchor_point, [23.0, 17.0]);
    assert_eq!(
        solid_anchor_scale(Some(project.item(source_id).unwrap()), [1920, 1080]),
        [1920.0, 1080.0]
    );
}

#[test]
fn static_values_use_pixels_percent_and_degrees_without_baking() {
    let mut project = read_project(include_bytes!(
        "../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    set_static_transform(
        &mut composition_mut(&mut project, 1).layers[0],
        &[
            ("ADBE Anchor Point", &[23.0, 17.0, 0.0]),
            ("ADBE Position", &[160.0, 120.0, 0.0]),
            ("ADBE Scale", &[-1.25, 0.7, 1.0]),
            ("ADBE Rotate Z", &[22.5]),
            ("ADBE Opacity", &[0.65]),
        ],
    );
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    let content = as_group(&occurrence.layers[0]);
    let FxLayer::Rect(rect) = content.layers[0].data() else {
        panic!("not an editable solid")
    };
    assert_eq!(occurrence.transform.anchor_point, [23.0, 17.0]);
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([160.0, 120.0])
    );
    assert_eq!(occurrence.transform.scale, [-125.0, 70.0]);
    assert_eq!(occurrence.transform.rotation, 22.5);
    assert_eq!(occurrence.transform.opacity.value(), 65.0);
    assert_eq!(rect.rect.size, [1920.0, 1080.0]);
    assert_eq!(rect.parent, Some(content.id));
    assert_ne!(rect.id, occurrence.id);
    assert!(!has(&converted, Limitation::Placeholder));
    let json = converted.document.to_json_vec().unwrap();
    EditableFxCompositionDocument::from_json_slice(&json).unwrap();
}

#[test]
fn unsupported_static_position_expression_preserves_authored_base() {
    use crate::rifx::Chunk;
    for separated in [false, true] {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/properties/transform_unseparated.aep"
        ))
        .unwrap();
        let layer = &mut composition_mut(&mut project, 1).layers[0];
        set_static_transform(
            layer,
            &[
                ("ADBE Position", &[799.875, -800.0, 0.0]),
                ("ADBE Position_0", &[799.875]),
                ("ADBE Position_1", &[-800.0]),
                ("ADBE Rotate Z", &[22.5]),
            ],
        );
        let property_root = layer
            .content
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let leaves = property_root[1].children_mut().unwrap();
        for pair in leaves.chunks_exact_mut(2) {
            let is_position = pair[0]
                .data_payload()
                .unwrap()
                .starts_with(b"ADBE Position\0");
            let is_x = pair[0]
                .data_payload()
                .unwrap()
                .starts_with(b"ADBE Position_0\0");
            let leaf = pair[1].children_mut().unwrap();
            if is_position && separated {
                leaf[1] = Chunk::data(*b"tdsb", [0, 0, 8, 1]).unwrap();
            }
            if (is_position && !separated) || (is_x && separated) {
                leaf.push(
                    Chunk::data(*b"Utf8", b"unsupportedPositionExpression()".to_vec()).unwrap(),
                );
            }
        }
        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        let occurrence = as_group(&root(&converted).layers[0]);
        assert_eq!(
            occurrence.transform.position,
            fx_composition::Position::TwoD([799.875, -800.0])
        );
        assert_eq!(occurrence.transform.rotation, 22.5);
        assert!(
            converted
                .diagnostics
                .iter()
                .any(|d| d.message.contains("stored pre-expression position")
                    && d.message.contains("not evaluated"))
        );
    }
}

#[test]
fn malformed_property_defaults_only_that_component_and_keeps_sibling_content() {
    let mut project = read_project(include_bytes!(
        "../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    set_static_transform(
        &mut composition_mut(&mut project, 1).layers[0],
        &[
            ("ADBE Position", &[12.0, 34.0, 0.0]),
            ("ADBE Scale", &[f64::NAN, 1.0, 1.0]),
        ],
    );
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    assert!(matches!(
        as_group(&occurrence.layers[0]).layers[0].data(),
        FxLayer::Rect(_)
    ));
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([12.0, 34.0])
    );
    assert_eq!(occurrence.transform.scale, [100.0, 100.0]);
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|d| d.message.contains("ADBE Scale: non-finite"))
    );
}

#[test]
fn native_animation_targets_occurrence_transform_and_separated_position_is_static() {
    use fx_schema::{PropType, PropertyTarget};
    for (file, property) in [
        ("property_2D_position.aep", PropType::PositionX),
        ("property_scale.aep", PropType::ScaleX),
        ("property_rotation.aep", PropType::Rotation),
        ("property_1D_opacity.aep", PropType::Opacity),
    ] {
        let bytes = fs::read(
            fixture_dir()
                .parent()
                .unwrap()
                .join("properties")
                .join(file),
        )
        .unwrap();
        let converted = to_structural_fx_document(&read_project(&bytes).unwrap(), Some(1)).unwrap();
        let occurrence = as_group(&root(&converted).layers[0]);
        assert!(
            converted
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .any(|entry| entry.target == PropertyTarget::layer(occurrence.id, property)),
            "{file}: missing editable occurrence track"
        );
        assert_eq!(
            occurrence.playback,
            identity_playback(occurrence.playback.input_range())
        );
        assert!(matches!(
            as_group(&occurrence.layers[0]).layers[0].data(),
            FxLayer::Rect(_)
        ));
    }
    let project = read_project(include_bytes!(
        "../../tests/fixtures/properties/transform_separated.aep"
    ))
    .unwrap();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    assert!(matches!(
        as_group(&occurrence.layers[0]).layers[0].data(),
        FxLayer::Rect(_)
    ));
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([960.0, 540.0])
    );
    assert!(
        !converted
            .diagnostics
            .iter()
            .any(|d| d.message.starts_with("ADBE Position"))
    );
}

#[test]
fn spatial_position_axis_with_noise_equal_endpoints_stays_put_on_occurrence_and_parent_copy() {
    use crate::rifx::Chunk;
    use fx_schema::{PropType, PropertyKeyframeEasing, PropertyTarget};

    fn named_list<'a>(children: &'a mut [Chunk], name: &str, kind: [u8; 4]) -> &'a mut Vec<Chunk> {
        let start = children
            .iter()
            .position(|chunk| {
                chunk.id() == *b"tdmn"
                    && chunk.data_payload().is_some_and(|bytes| {
                        bytes
                            .iter()
                            .copied()
                            .take_while(|byte| *byte != 0)
                            .eq(name.bytes())
                    })
            })
            .unwrap_or_else(|| panic!("native {name}"));
        let end = children[start + 1..]
            .iter()
            .position(|chunk| chunk.id() == *b"tdmn")
            .map_or(children.len(), |offset| start + 1 + offset);
        children[start + 1..end]
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(kind))
            .and_then(Chunk::children_mut)
            .unwrap_or_else(|| panic!("native {name} body"))
    }

    // Rewrites only the values and incoming path speed of the two native
    // Position keys; AE's spatial flag, times, interpolation, influences and
    // zero tangents are kept.
    fn with_position_keys(values: [[f64; 2]; 2], incoming_speed: f64) -> StructuralProject {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/pr4442_native/sources/hierarchy_animated_bounds_precomp.aep"
        ))
        .unwrap();
        let composition = composition_mut(&mut project, 16);
        let child = composition
            .layers
            .iter_mut()
            .find(|layer| layer.record.id() == 31)
            .expect("native sibling");
        patch(child, 132, &29_u32.to_be_bytes());
        let layer = composition
            .layers
            .iter_mut()
            .find(|layer| layer.record.id() == 29)
            .expect("native spatial Position owner");
        let root = layer
            .content
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .and_then(Chunk::children_mut)
            .expect("native property root");
        let transform = named_list(root, "ADBE Transform Group", *b"tdgp");
        let position = named_list(transform, "ADBE Position", *b"tdbs");
        let keys = position
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"list"))
            .and_then(Chunk::children_mut)
            .expect("native Position keys");
        let ldat = keys
            .iter_mut()
            .find(|chunk| chunk.id() == *b"ldat")
            .expect("native key data");
        let mut bytes = ldat.data_payload().unwrap().to_vec();
        // Native 3D spatial layout: 128-byte items; in speed at 24, X/Y at 56/64.
        assert_eq!(bytes.len(), 256);
        for (index, [x, y]) in values.into_iter().enumerate() {
            let item = &mut bytes[index * 128..(index + 1) * 128];
            item[56..64].copy_from_slice(&x.to_be_bytes());
            item[64..72].copy_from_slice(&y.to_be_bytes());
        }
        bytes[128 + 24..128 + 32].copy_from_slice(&incoming_speed.to_be_bytes());
        *ldat = Chunk::data(*b"ldat", bytes).unwrap();
        project
    }

    // The arriving easing of each Position axis on native layer 29's occurrence
    // and on the transform-only parent copy that carries its motion for 31.
    fn arriving_easings(project: &StructuralProject) -> Vec<(PropType, PropertyKeyframeEasing)> {
        fn owners(layers: &[fx_schema::Layer], found: &mut Vec<LayerId>) {
            for layer in layers {
                if let FxLayer::Group(group) = layer.data() {
                    if group.description.contains("AEP comp=16 layer=29 ")
                        || group.description.contains("transform-only parent copy 29;")
                    {
                        found.push(group.id);
                    }
                    owners(&group.layers, found);
                }
            }
        }
        let converted = to_structural_fx_document(project, Some(16)).unwrap();
        let mut found = Vec::new();
        owners(converted.document.composition().layers(), &mut found);
        assert_eq!(found.len(), 2, "occurrence and parent copy");
        let entries = converted.document.composition().dynamics().entries();
        let mut easings = Vec::new();
        for id in found {
            for axis in [PropType::PositionX, PropType::PositionY] {
                let keys = entries
                    .iter()
                    .find(|entry| entry.target == PropertyTarget::layer(id, axis))
                    .and_then(|entry| entry.animator.keyframe_track())
                    .unwrap_or_else(|| panic!("{axis:?} keys on {id:?}"))
                    .keyframes();
                assert_eq!(keys.len(), 2);
                assert_eq!(
                    keys[1].spatial_in_tangent(),
                    None,
                    "straight native geometry must not reintroduce FX spatial double easing"
                );
                easings.push((axis, keys[1].easing()));
            }
        }
        easings
    }

    // AE keeps a duplicated key's incoming path speed although the copy sits
    // 2 ULPs from the original: this segment has no path to traverse.
    let noise = with_position_keys([[220.0, 350.000_000_000_000_1], [220.0, 350.0]], 6.25);
    for (axis, easing) in arriving_easings(&noise) {
        assert_eq!(easing, PropertyKeyframeEasing::Linear, "{axis:?}");
    }

    // Real motion keeps its native ease: the 1.25 s segment has 60% influences
    // and no outgoing speed, so only the arriving handle carries the speed.
    let arriving_y = |easings: Vec<(PropType, PropertyKeyframeEasing)>| {
        easings
            .into_iter()
            .filter(|(axis, _)| *axis == PropType::PositionY)
            .map(|(_, easing)| easing)
            .collect::<Vec<_>>()
    };
    let tiny = 0.0009765625; // 2^-10 px: sub-pixel, yet ~1.7e10 ULPs at 350.
    // The second pair overshoots its arriving handle below zero, unclamped.
    for (to, speed, y2) in [(350.0 + tiny, 0.000244140625, 0.8125), (650.0, 600.0, -0.5)] {
        for easing in arriving_y(arriving_easings(&with_position_keys(
            [[220.0, 350.0], [220.0, to]],
            speed,
        ))) {
            let PropertyKeyframeEasing::CubicBezier {
                x1,
                y1,
                x2,
                y2: actual,
            } = easing
            else {
                panic!("real motion to {to} lost its native ease: {easing:?}");
            };
            assert!((x1 - 0.6).abs() < 1e-12 && y1 == 0.0 && (x2 - 0.4).abs() < 1e-12);
            assert!((actual - y2).abs() < 1e-9, "to {to}: y2 {actual}");
        }
    }
}

fn assert_pruned_adjustment_budget(adjustment_first: bool) {
    let mut project = read_project(include_bytes!(
        "../../tests/fixtures/properties/property_2D_position.aep"
    ))
    .unwrap();
    let comp = composition_mut(&mut project, 1);
    let mut adjustment = comp.layers[0].clone();
    adjustment.name = "Discarded adjustment motion".into();
    let adjustment_flags = adjustment.record.raw_bytes()[38] | 2;
    patch(&mut adjustment, 38, &[adjustment_flags]);
    let mut retained = comp.layers[0].clone();
    retained.name = "Retained ordinary motion".into();
    patch(&mut retained, 0, &999_u32.to_be_bytes());
    comp.layers = if adjustment_first {
        vec![adjustment, retained]
    } else {
        vec![retained, adjustment]
    };

    let full = to_structural_fx_document(&project, Some(1)).unwrap();
    let full_root = root(&full);
    let adjustment_id = full_root
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Adjustment(layer) => Some(layer.id),
            _ => None,
        })
        .expect("adjustment layer");
    let retained_id = full_root
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Group(layer) if layer.name == "Retained ordinary motion" => Some(layer.id),
            _ => None,
        })
        .expect("ordinary animated layer");
    let full_entries = full.document.composition().dynamics().entries();
    assert!(
        !full_entries.is_empty(),
        "ordinary motion must be reachable"
    );
    assert!(full_entries.iter().all(|entry| {
        entry.target.as_property().is_some_and(|property| {
            property.layer_id() == retained_id && property.layer_id() != adjustment_id
        })
    }));

    // Use the same native tracks without the discarded Adjustment as the
    // budget oracle, including the preflight estimator's conservative overhead.
    let mut reachable_only = project.clone();
    composition_mut(&mut reachable_only, 1)
        .layers
        .retain(|layer| !layer.record.flags().adjustment_layer);
    let reachable_reservation = to_structural_fx_document(&reachable_only, Some(1))
        .unwrap()
        .animation_budget_used;
    assert!(
        reachable_reservation >= full.committed_animation_bytes,
        "ordinary charged {reachable_reservation}, full charged {}, full committed {}",
        full.animation_budget_used,
        full.committed_animation_bytes
    );
    let limited =
        to_structural_fx_document_with_animation_limit(&project, Some(1), reachable_reservation)
            .unwrap();
    assert_eq!(
        serde_json::to_value(limited.document.composition().dynamics().entries()).unwrap(),
        serde_json::to_value(full_entries).unwrap(),
        "discarded Adjustment motion must not deny reachable sibling animation"
    );
    assert_eq!(full.animation_budget_used, reachable_reservation);
    assert_eq!(limited.animation_budget_used, reachable_reservation);
}

#[test]
fn pruned_adjustment_transform_tracks_do_not_consume_reachable_animation_budget() {
    assert_pruned_adjustment_budget(true);
}

#[test]
fn pruned_adjustment_transform_accounting_is_layer_order_independent() {
    assert_pruned_adjustment_budget(false);
}

#[test]
fn duplicate_leaf_does_not_discard_other_static_components() {
    let mut project = read_project(include_bytes!(
        "../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    set_static_transform(
        &mut composition_mut(&mut project, 1).layers[0],
        &[
            ("ADBE Position", &[12.0, 34.0, 0.0]),
            ("ADBE Scale", &[1.25, 0.7, 1.0]),
            ("ADBE Scale", &[2.0, 2.0, 1.0]),
        ],
    );
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    assert!(matches!(
        as_group(&occurrence.layers[0]).layers[0].data(),
        FxLayer::Rect(_)
    ));
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([12.0, 34.0])
    );
    assert_eq!(occurrence.transform.scale, [100.0, 100.0]);
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|d| d.message.contains("duplicate Transform leaf"))
    );
}

#[test]
fn native_sources_are_pinned_and_every_sidecar_structure_is_checked() {
    let provenance: Value = serde_json::from_slice(include_bytes!(
        "../../tests/fixtures/layers/provenance.json"
    ))
    .unwrap();
    for case in provenance["cases"].as_array().unwrap() {
        let bytes = fs::read(fixture_dir().join(case["file"].as_str().unwrap())).unwrap();
        assert_eq!(bytes.len() as u64, case["bytes"].as_u64().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            case["sha256"].as_str().unwrap()
        );
    }
    let expected: Value =
        serde_json::from_slice(include_bytes!("../../tests/fixtures/layers/expected.json"))
            .unwrap();
    for case in expected.as_array().unwrap() {
        let file = case["file"].as_str().unwrap();
        let project = fixture(file);
        let items = case["items"].as_array().unwrap();
        assert_eq!(project.items.len(), items.len(), "{file}: item count");
        for item in items {
            let id = u32::try_from(item["id"].as_u64().unwrap()).unwrap();
            let actual = project
                .item(id)
                .unwrap_or_else(|| panic!("{file}: missing item {id}"));
            assert_eq!(
                actual.name,
                item["name"].as_str().unwrap(),
                "{file}: item {id}"
            );
            assert_eq!(
                actual.parent_folder.map(u64::from),
                item["parentFolderId"].as_u64(),
                "{file}: folder of {id}"
            );
            match item["itemType"].as_str().unwrap() {
                "FolderItem" => assert!(matches!(actual.kind, ItemKind::Folder)),
                "FootageItem" => {
                    use crate::structure::FootageSourceKind;
                    assert!(matches!(actual.kind, ItemKind::Footage));
                    let expected_kind = match item["sourceType"].as_str().unwrap() {
                        "SolidSource" => FootageSourceKind::Solid,
                        "FileSource" => FootageSourceKind::File,
                        "PlaceholderSource" => FootageSourceKind::Placeholder,
                        other => panic!("unexpected sidecar source type {other}"),
                    };
                    assert_eq!(
                        actual.footage.unwrap().main_source,
                        expected_kind,
                        "{file}: source class of {id}"
                    );
                }
                "CompItem" => {
                    let comp = composition(&project, id);
                    assert_eq!(u64::from(comp.width), item["width"].as_u64().unwrap());
                    assert_eq!(u64::from(comp.height), item["height"].as_u64().unwrap());
                    assert!(
                        (comp.duration_secs - item["duration"].as_f64().unwrap()).abs() < 0.0001
                    );
                    assert!((comp.frame_rate - item["frameRate"].as_f64().unwrap()).abs() < 0.0001);
                    let aspect = f64::from(comp.pixel_aspect.0) / f64::from(comp.pixel_aspect.1);
                    assert!((aspect - item["pixelAspect"].as_f64().unwrap()).abs() < 1e-10);
                    assert!(
                        (comp.display_start_secs - item["displayStartTime"].as_f64().unwrap())
                            .abs()
                            < 0.0001
                    );
                    let layers = item["layers"].as_array().unwrap();
                    assert_eq!(comp.layers.len(), layers.len(), "{file} comp {id}");
                    for (actual, expected) in comp.layers.iter().zip(layers) {
                        check_layer(file, id, actual, expected);
                    }
                }
                kind => panic!("unexpected sidecar item kind {kind}"),
            }
        }
    }
}

fn check_layer(file: &str, comp_id: u32, actual: &Layer, expected: &Value) {
    let context = format!("{file} comp {comp_id} layer {}", expected["id"]);
    let record = &actual.record;
    assert_eq!(
        u64::from(record.id()),
        expected["id"].as_u64().unwrap(),
        "{context}: order/id"
    );
    assert_eq!(
        actual.name.as_ref(),
        expected["name"].as_str().unwrap(),
        "{context}: name"
    );
    if let Some(source_id) = expected["sourceId"].as_u64() {
        assert_eq!(
            u64::from(record.source_id()),
            source_id,
            "{context}: source"
        );
    }
    assert_eq!(
        u64::from(record.parent_id()),
        expected["parentId"].as_u64().unwrap(),
        "{context}: parent"
    );
    let layer_type = match expected["layerType"].as_str().unwrap() {
        "AVLayer" => 0,
        "LightLayer" => 1,
        "CameraLayer" => 2,
        "TextLayer" => 3,
        "ShapeLayer" => 4,
        "ThreeDModelLayer" => 5,
        "ParametricMeshLayer" => 7,
        // Older upstream exporters used a generic class for these AE types.
        "Layer" => match expected["matchName"].as_str().unwrap() {
            "ADBE Text Layer" => 3,
            "ADBE Vector Layer" => 4,
            "ADBE 3D Model Layer" => 5,
            other => panic!("unexpected independent match name {other}"),
        },
        other => panic!("unexpected independent layer type {other}"),
    };
    assert_eq!(record.layer_type(), layer_type, "{context}: type");
    assert_eq!(
        u64::from(record.label()),
        expected["label"].as_u64().unwrap(),
        "{context}: label"
    );
    if let Some(mode) = expected["blendingMode"].as_u64() {
        // Explicit mapping from py-aep enums/general.py; Adobe API enums are
        // not wire ordinals. Native cases here contain Normal (wire 0 or 2).
        assert_eq!(
            mode, 5212,
            "extend the independent mode mapping for new fixtures"
        );
        assert!(
            matches!(record.blend_mode(), 0 | 2),
            "{context}: Normal blend mode"
        );
    }
    if let Some(mode) = expected["trackMatteType"].as_u64() {
        assert_eq!(
            u64::from(record.track_matte_type()) + 5012,
            mode,
            "{context}: matte mode"
        );
    }
    if let Some(mode) = expected["autoOrient"].as_u64() {
        assert_eq!(
            u64::from(record.auto_orient()) + 4212,
            mode,
            "{context}: auto-orient"
        );
    }
    let start = record.start_time().unwrap();
    let stretch = record.stretch().unwrap();
    for (key, value) in [
        ("startTime", start),
        ("stretch", stretch * 100.0),
        ("inPoint", start + record.in_point().unwrap() * stretch),
        ("outPoint", start + record.out_point().unwrap() * stretch),
    ] {
        let expected_value = expected[key].as_f64().unwrap();
        if file == "layer_timing.aep"
            && comp_id == 30
            && record.id() == 43
            && matches!(key, "inPoint" | "outPoint")
        {
            // Pinned native API sidecar differs from the raw stored reverse
            // endpoints by one 1/3000s AE tick. Preserve both pieces of evidence;
            // this is a documented mismatch, not a widened global tolerance.
            assert!(
                (value - expected_value - 1.0 / 3000.0).abs() < 1e-10,
                "{context}: known reverse boundary discrepancy"
            );
        } else {
            assert!(
                (value - expected_value).abs() < 0.0001,
                "{context}: {key}, got {value}, expected {}",
                expected[key]
            );
        }
    }
    let flags = record.flags();
    for (key, value) in [
        ("enabled", flags.enabled),
        ("audioEnabled", flags.audio_enabled),
        ("effectsActive", flags.effects_active),
        ("solo", flags.solo),
        ("guideLayer", flags.guide_layer),
        ("nullLayer", flags.null_layer),
        ("adjustmentLayer", flags.adjustment_layer),
        ("threeDLayer", flags.three_d_layer),
        ("shy", flags.shy),
        ("locked", flags.locked),
        ("collapseTransformation", flags.collapse_transformation),
        ("motionBlur", flags.motion_blur),
        ("frameBlending", flags.frame_blending),
        ("threeDPerChar", flags.three_d_per_char),
        ("environmentLayer", flags.environment_layer),
        ("preserveTransparency", flags.preserve_transparency),
    ] {
        if let Some(expected) = expected[key].as_bool() {
            assert_eq!(value, expected, "{context}: {key}");
        }
    }
    assert!(
        !actual.content.is_empty(),
        "{context}: raw properties retained"
    );
}

#[test]
fn all_native_compositions_convert_with_explicit_structural_only_diagnostics() {
    let expected: Value =
        serde_json::from_slice(include_bytes!("../../tests/fixtures/layers/expected.json"))
            .unwrap();
    for case in expected.as_array().unwrap() {
        let project = fixture(case["file"].as_str().unwrap());
        for item in &project.items {
            let ItemKind::Composition(comp) = &item.kind else {
                continue;
            };
            let converted = to_structural_fx_document(&project, Some(item.id)).unwrap();
            assert_eq!(root(&converted).name, item.name);
            assert!(root(&converted).layers.len() >= comp.layers.len());
            for helper in &root(&converted).layers[comp.layers.len()..] {
                assert!(
                    root(&converted).layers.iter().any(|target| {
                        let matte = match target.data() {
                            FxLayer::Group(group) => group.track_matte.as_ref(),
                            FxLayer::Adjustment(adjustment) => adjustment.track_matte.as_ref(),
                            _ => None,
                        };
                        matte.is_some_and(|matte| matte.layer == as_group(helper).id)
                    }),
                    "appended layer must be a referenced matte helper"
                );
            }
            assert!(has(&converted, Limitation::CompositionSettings));
            for (source, layer) in comp.layers.iter().zip(&root(&converted).layers) {
                let (name, parent, description) = if source.record.flags().adjustment_layer {
                    let FxLayer::Adjustment(adjustment) = layer.data() else {
                        panic!("native Adjustment must remain an editable Adjustment");
                    };
                    (&adjustment.name, adjustment.parent, &adjustment.description)
                } else {
                    let group = as_group(layer);
                    (&group.name, group.parent, &group.description)
                };
                assert_eq!(name, source.name.as_ref());
                assert_eq!(parent, Some(root(&converted).id));
                assert!(description.contains(&format!("layer={}", source.record.id())));
            }
            // Reopening exercises persisted JSON limits/validation, not just construction.
            let bytes = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&bytes).unwrap();
        }
    }
}

#[test]
fn native_parenting_and_mattes_are_not_confused_with_group_containment() {
    let oracle_bytes = crate::test_fixtures::read("parenting/layer_misc.json.gz");
    assert_eq!(
        format!("{:x}", Sha256::digest(&oracle_bytes)),
        "b0ad48f49cf97bd410b3acbe67c1e86c5bf5179e53915ccf4bdc23fdda229d35"
    );
    let oracle: Value = serde_json::from_slice(&oracle_bytes).unwrap();
    let native_comp = oracle["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == 44)
        .unwrap();
    let native_parent = native_comp["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == 57)
        .unwrap();
    let transform = native_parent["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["matchName"] == "ADBE Transform Group")
        .unwrap();
    let opacity = transform["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["matchName"] == "ADBE Opacity")
        .unwrap();
    assert_eq!(opacity["value"], 0);
    let converted = to_structural_fx_document(&fixture("layer_misc.aep"), Some(44)).unwrap();
    let parent = root(&converted);
    assert_eq!(parent.layers.len(), 2);
    let child = as_group(&parent.layers[0]);
    assert_eq!(child.name, "ChildLayer");
    assert_eq!(child.parent, Some(parent.id));
    assert!(child.description.contains("transform-only parent copy 57"));
    assert_eq!(child.transform.opacity.value(), 100.0);
    assert_eq!(child.transform.anchor_point, [0.0, 0.0]);
    assert_eq!(
        child.transform.position,
        fx_schema::Position::TwoD([50.0, 50.0])
    );
    assert_eq!(
        child.playback,
        identity_playback(child.playback.input_range())
    );
    let occurrence = as_group(&child.layers[0]);
    assert_eq!(occurrence.parent, Some(child.id));
    assert!(occurrence.description.contains("transformParent=57"));
    assert_eq!(occurrence.transform.anchor_point, [50.0, 50.0]);
    assert_eq!(
        occurrence.transform.position,
        fx_schema::Position::TwoD([0.0, 0.0])
    );
    assert_eq!(
        as_group(&parent.layers[1]).transform.opacity.value(),
        0.0,
        "the source null's own opacity must not leak into its child's wrapper"
    );
    assert!(has(&converted, Limitation::Parenting));
    let matte = fixture("track_matte_yes.aep");
    assert_eq!(
        composition(&matte, 1).layers[0].record.matte_layer_id(),
        Some(15)
    );
    let converted = to_structural_fx_document(&matte, Some(1)).unwrap();
    assert!(has(&converted, Limitation::TrackMatte));
    let target = as_group(&root(&converted).layers[0]);
    let matte = target.track_matte.as_ref().expect("native matte mapped");
    assert!(
        root(&converted)
            .layers
            .iter()
            .any(|layer| as_group(layer).id == matte.layer)
    );
    assert_ne!(
        matte.layer,
        as_group(&root(&converted).layers[1]).id,
        "matte sampling must not consume the independent paint occurrence"
    );
}

#[test]
fn native_precomp_trim_and_stretch_use_content_clock_not_active_range_offset() {
    let project = fixture("outPoint_clamp.aep");
    for (id, start, end, source_end) in [
        (13, 0.0, 5.0, 5.0),
        (26, 0.0, 10.0, 5.0),
        (39, 0.0, 20.0, 5.0),
        (52, 3.0, 8.0, 5.0),
    ] {
        let converted = to_structural_fx_document(&project, Some(id)).unwrap();
        let occurrence = as_group(&root(&converted).layers[0]);
        assert_eq!(
            occurrence.playback,
            identity_playback(occurrence.playback.input_range())
        );
        assert_eq!(occurrence.playback.input_range().start, Time::ZERO);
        let group = as_group(&occurrence.layers[0]);
        assert_eq!(group.parent, Some(occurrence.id));
        assert_eq!(group.playback.input_range().start.as_secs(), start);
        assert_eq!(group.playback.input_range().end().as_secs(), end);
        let playback = group
            .playback
            .time_remap()
            .expect("expected affine keyframes");
        let keys = playback.keyframes();
        assert_eq!(keys[0].time.as_secs(), start);
        assert_eq!(keys[0].value.as_secs(), 0.0);
        assert_eq!(keys[1].time.as_secs(), end);
        assert_eq!(keys[1].value.as_secs(), source_end);
        assert!(!has(&converted, Limitation::Placeholder));
    }
}

/// Sets a layer's native start, in and out points, in tenths of a
/// millisecond, and its stretch fraction.
fn set_native_clock(layer: &mut Layer, [start, input, output]: [i32; 3], stretch: (i32, u32)) {
    for (offset, value) in [(12, start), (20, input), (28, output)] {
        patch(layer, offset, &value.to_be_bytes());
        patch(layer, offset + 4, &10_000_u32.to_be_bytes());
    }
    patch(layer, 8, &stretch.0.to_be_bytes());
    patch(layer, 108, &stretch.1.to_be_bytes());
}

/// The visible source-clock window and its two linear source keys, in ms.
fn source_clock(document: &EditableFxCompositionDocument) -> ((u64, u64), [(u64, u64); 2]) {
    let root = as_group(&document.composition().layers()[0]);
    let occurrence = as_group(&root.layers[0]);
    let clock = as_group(&occurrence.layers[0]);
    assert_eq!(clock.name, "Source content clock");
    assert!(!clock.is_hidden);
    let window = clock.playback.input_range();
    let remap = clock
        .playback
        .time_remap()
        .expect("affine source clock keys");
    let keys = remap.keyframes();
    assert_eq!(keys.len(), 2);
    assert!(
        keys.iter()
            .all(|key| key.easing == PropertyKeyframeEasing::Linear)
    );
    (
        (window.start.as_millis(), window.end().as_millis()),
        [0, 1].map(|index| (keys[index].time.as_millis(), keys[index].value.as_millis())),
    )
}

fn converted_source_clock(stretch: (i32, u32), timing: [i32; 3]) -> ((u64, u64), [(u64, u64); 2]) {
    let mut project = fixture("outPoint_clamp.aep");
    set_native_clock(
        &mut composition_mut(&mut project, 13).layers[0],
        timing,
        stretch,
    );
    let converted = to_structural_fx_document(&project, Some(13)).unwrap();
    let clock = source_clock(&converted.document);
    let readback =
        EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
            .expect("source clock import remains valid editable FX");
    assert_eq!(source_clock(&readback), clock, "editable JSON readback");
    clock
}

#[test]
fn authored_unit_stretch_keeps_an_exact_1x_source_clock_after_rounding() {
    // Supplemental mutations of a native precomposition layer record; native
    // composition time is start + source * stretch. Times: 0.1 ms units.
    let cases = [
        // Parent 1.3..101.9 ms reads source 1.6..102.2 ms. Rounding both
        // source ends gave 2..102 over the window 1..102, a 100/101x clock.
        ((1, 1), [-3, 16, 1022], ((1, 102), [(1, 2), (102, 103)])),
        // An unreduced unit fraction is the same authored 1x.
        ((2, 2), [-3, 16, 1022], ((1, 102), [(1, 2), (102, 103)])),
        // The opposite rounding direction gave a 100/99x clock.
        ((1, 1), [4, 12, 1010], ((2, 101), [(2, 1), (101, 100)])),
        // A negative start clips the parent at zero and keeps the positive
        // source offset. Rounding both source ends gave a 51/50x clock.
        ((1, 1), [-504, 0, 1008], ((0, 50), [(0, 50), (50, 100)])),
        // A negative in point clips the parent 5.4..100.7 ms at source zero,
        // 10.4 ms. Rounding both source ends gave a 90/91x clock.
        ((1, 1), [104, -50, 903], ((10, 101), [(10, 0), (101, 91)])),
    ];
    let actual: Vec<_> = cases
        .iter()
        .map(|&(stretch, timing, _)| converted_source_clock(stretch, timing))
        .collect();
    let expected: Vec<_> = cases.iter().map(|&(_, _, expected)| expected).collect();
    assert_eq!(actual, expected);
    for (_, keys) in actual {
        assert_eq!(
            keys[1].0 - keys[0].0,
            keys[1].1 - keys[0].1,
            "equal parent and source spans: exact 1x"
        );
    }
}

#[test]
fn nonunit_near_unit_and_reverse_stretches_keep_independently_rounded_source_keys() {
    for (stretch, timing, expected) in [
        // A genuine 2/1 stretch plays at half speed: parent 1.3..101.9 ms
        // reads source 0.8..51.1 ms.
        ((2, 1), [-3, 8, 511], ((1, 102), [(1, 1), (102, 51)])),
        // A near-unit rational stretch is authored as such, not snapped to 1x.
        (
            (1_000_001, 1_000_000),
            [-3, 16, 1022],
            ((1, 102), [(1, 2), (102, 102)]),
        ),
        // Reverse 1x: parent 1.3..101.9 ms reads source 102.2..1.6 ms.
        ((-1, 1), [1035, 16, 1022], ((1, 102), [(1, 102), (102, 2)])),
    ] {
        assert_eq!(
            converted_source_clock(stretch, timing),
            expected,
            "{stretch:?} {timing:?}"
        );
    }
}

fn time_remap_property(layer: &mut Layer) -> &mut Vec<crate::rifx::Chunk> {
    let root = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .expect("native property root");
    let children = root.children_mut().expect("property root children");
    let start = children
        .iter()
        .position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk.data_payload().is_some_and(|bytes| {
                    bytes
                        .iter()
                        .copied()
                        .take_while(|byte| *byte != 0)
                        .eq(b"ADBE Time Remapping".iter().copied())
                })
        })
        .expect("native Time Remapping property");
    let end = children[start + 1..]
        .iter()
        .position(|chunk| chunk.id() == *b"tdmn")
        .map_or(children.len(), |offset| start + 1 + offset);
    children[start + 1..end]
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .and_then(crate::rifx::Chunk::children_mut)
        .expect("Time Remapping numeric property")
}

#[test]
fn authored_time_remap_uses_parent_visibility_when_affine_source_is_negative() {
    // Supplemental mutation of a native Time Remapping property: parent 1..2s
    // maps to positive source 1..2s while the unused affine clock is -2..-1s.
    let mut project = fixture("avlayer_flags.aep");
    let layer = &mut composition_mut(&mut project, 125).layers[0];
    let property = time_remap_property(layer);
    let timebase = property
        .iter()
        .find(|chunk| chunk.id() == *b"tdb4")
        .and_then(crate::rifx::Chunk::data_payload)
        .map(|bytes| u32::from_be_bytes(bytes[12..16].try_into().unwrap()))
        .expect("Time Remapping timebase");
    let key_list = property
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"list"))
        .and_then(crate::rifx::Chunk::children_mut)
        .expect("Time Remapping key list");
    let (count, stride) = key_list
        .iter()
        .find(|chunk| chunk.id() == *b"lhd3")
        .and_then(crate::rifx::Chunk::data_payload)
        .map(|header| {
            (
                usize::from(u16::from_be_bytes(header[10..12].try_into().unwrap())),
                usize::from(u16::from_be_bytes(header[18..20].try_into().unwrap())),
            )
        })
        .expect("Time Remapping key header");
    assert_eq!((count, stride), (2, 48));
    let ldat = key_list
        .iter_mut()
        .find(|chunk| chunk.id() == *b"ldat")
        .expect("Time Remapping key data");
    let mut key_bytes = ldat.data_payload().expect("key data payload").to_vec();
    let timebase = i32::try_from(timebase).expect("fixture timebase fits i32");
    for (index, (time, value)) in [(-2, 1.0_f64), (-1, 2.0)].into_iter().enumerate() {
        let offset = index * stride;
        key_bytes[offset..offset + 4].copy_from_slice(&(time * timebase).to_be_bytes());
        key_bytes[offset + 8..offset + 16].copy_from_slice(&value.to_be_bytes());
    }
    *ldat = crate::rifx::Chunk::data(*b"ldat", key_bytes).unwrap();

    let (_, start_denominator) = layer.record.start_time_fraction();
    let (_, in_denominator) = layer.record.in_point_fraction();
    let (_, out_denominator) = layer.record.out_point_fraction();
    patch(
        layer,
        12,
        &(3 * i32::try_from(start_denominator).unwrap()).to_be_bytes(),
    );
    patch(
        layer,
        20,
        &(-2 * i32::try_from(in_denominator).unwrap()).to_be_bytes(),
    );
    patch(
        layer,
        28,
        &(-i32::try_from(out_denominator).unwrap()).to_be_bytes(),
    );

    let converted = to_structural_fx_document(&project, Some(125)).unwrap();
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .expect("authored-remap import remains valid editable FX");
    let occurrence = as_group(&root(&converted).layers[0]);
    let gate = as_group(&occurrence.layers[0]);
    assert_eq!(gate.name, "Source content clock");
    assert!(!gate.is_hidden);
    assert_eq!(gate.playback.input_range().start, Time::from_secs(1.0));
    assert_eq!(gate.playback.input_range().end(), Time::from_secs(2.0));
    let gate_playback = gate
        .playback
        .time_remap()
        .expect("parent visibility gate must use keyframes");
    let gate_keys = gate_playback.keyframes();
    assert_eq!(
        (gate_keys[0].time, gate_keys[0].value),
        (Time::from_secs(1.0), Time::from_secs(1.0))
    );
    assert_eq!(
        (gate_keys[1].time, gate_keys[1].value),
        (Time::from_secs(2.0), Time::from_secs(2.0))
    );

    let remap = as_group(&gate.layers[0]);
    assert_eq!(remap.name, "Authored source remap");
    assert!(!remap.is_hidden);
    let remap_playback = remap
        .playback
        .time_remap()
        .expect("authored source remap must use keyframes");
    let remap_keys = remap_playback.keyframes();
    assert_eq!(
        (remap_keys[0].time, remap_keys[0].value),
        (Time::from_secs(1.0), Time::from_secs(1.0))
    );
    assert_eq!(
        (remap_keys[1].time, remap_keys[1].value),
        (Time::from_secs(2.0), Time::from_secs(2.0))
    );
    assert!(!converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("nonnegative source clock has no positive-duration")
    }));

    let limited = to_structural_fx_document_with_animation_limit(&project, Some(125), 1)
        .expect("tiny animation allowance must preserve static conversion");
    let occurrence = as_group(&root(&limited).layers[0]);
    let content = as_group(&occurrence.layers[0]);
    assert!(
        content.playback == identity_playback(content.playback.input_range()),
        "authored remap, gate, and affine fallback must all be omitted before allocation"
    );
    assert!(
        !content.is_hidden,
        "static source carrier must remain visible"
    );
    assert_eq!(content.playback.input_range().start, Time::from_secs(1.0));
    assert_eq!(content.playback.input_range().end(), Time::from_secs(2.0));
    assert!(limited.diagnostics.iter().any(|diagnostic| {
        diagnostic.limitation == Limitation::ExpansionLimit
            && diagnostic.message.contains("static content")
    }));
}

/// Rewrites the pinned native remap keys as (layer-local seconds, source
/// seconds) items. The fixture's first 48-byte item supplies interpolation.
fn set_time_remap_keys(layer: &mut Layer, keys: &[(i32, f64)]) {
    let property = time_remap_property(layer);
    let timebase = property
        .iter()
        .find(|chunk| chunk.id() == *b"tdb4")
        .and_then(crate::rifx::Chunk::data_payload)
        .map(|bytes| u32::from_be_bytes(bytes[12..16].try_into().unwrap()))
        .expect("Time Remapping timebase");
    let timebase = i32::try_from(timebase).unwrap();
    let list = property
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"list"))
        .and_then(crate::rifx::Chunk::children_mut)
        .expect("Time Remapping key list");
    let header = list
        .iter_mut()
        .find(|chunk| chunk.id() == *b"lhd3")
        .expect("Time Remapping key header");
    let mut header_bytes = header.data_payload().unwrap().to_vec();
    header_bytes[10..12].copy_from_slice(&u16::try_from(keys.len()).unwrap().to_be_bytes());
    *header = crate::rifx::Chunk::data(*b"lhd3", header_bytes).unwrap();
    let ldat = list
        .iter_mut()
        .find(|chunk| chunk.id() == *b"ldat")
        .expect("Time Remapping key data");
    let template = ldat.data_payload().unwrap()[..48].to_vec();
    let mut items = Vec::with_capacity(keys.len() * 48);
    for (time, value) in keys {
        let mut item = template.clone();
        item[0..4].copy_from_slice(&(time * timebase).to_be_bytes());
        item[8..16].copy_from_slice(&value.to_be_bytes());
        items.extend(item);
    }
    *ldat = crate::rifx::Chunk::data(*b"ldat", items).unwrap();
}

fn set_time_remap_expression(layer: &mut Layer, text: &str, enabled: bool) {
    let property = time_remap_property(layer);
    property.retain(|chunk| chunk.id() != *b"Utf8");
    property.push(crate::rifx::Chunk::data(*b"Utf8", text.as_bytes().to_vec()).unwrap());
    let meta = property
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdb4")
        .expect("Time Remapping metadata");
    let mut bytes = meta.data_payload().unwrap().to_vec();
    bytes[119] = if enabled {
        bytes[119] & !1
    } else {
        bytes[119] | 1
    };
    *meta = crate::rifx::Chunk::data(*b"tdb4", bytes).unwrap();
}

/// Layer clock as exact native rationals: start, in point and out point.
fn set_layer_clock(layer: &mut Layer, clock: [(i32, u32); 3]) {
    for (offset, (numerator, denominator)) in [12, 20, 28].into_iter().zip(clock) {
        patch(layer, offset, &numerator.to_be_bytes());
        patch(layer, offset + 4, &denominator.to_be_bytes());
    }
    assert_eq!(layer.record.stretch(), Some(1.0));
}

fn remap_clocks(converted: &StructuralConversion) -> (&GroupLayer, Option<&GroupLayer>) {
    let occurrence = as_group(&root(converted).layers[0]);
    let gate = as_group(&occurrence.layers[0]);
    assert_eq!(gate.name, "Source content clock");
    let remap = gate
        .layers
        .iter()
        .map(as_group)
        .find(|group| group.name == "Authored source remap");
    (gate, remap)
}

fn playback(group: &GroupLayer) -> &TimeRemapProperty {
    group.playback.time_remap().unwrap_or_else(|| {
        panic!(
            "{} needs time-remap playback, got {:?}",
            group.name, group.playback
        )
    })
}

fn key_millis(remap: &TimeRemapProperty) -> Vec<(u64, u64)> {
    remap
        .keyframes()
        .iter()
        .map(|key| (key.time.as_millis(), key.value.as_millis()))
        .collect()
}

fn remap_diagnostics(converted: &StructuralConversion) -> Vec<&str> {
    converted
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("Time Remapping"))
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

/// Native CAP2 comp 813/layer 828 shape: the layer lifetime starts before the
/// first authored key. AE holds the first source time until that key.
#[test]
fn authored_remap_holds_end_values_inside_its_lifetime_gate() {
    let mut project = fixture("avlayer_flags.aep");
    let layer = &mut composition_mut(&mut project, 125).layers[0];
    set_layer_clock(layer, [(1, 1), (0, 1), (4, 1)]);
    set_time_remap_keys(layer, &[(1, 0.0), (3, 2.0)]);
    let converted = to_structural_fx_document(&project, Some(125)).unwrap();
    let (gate, remap) = remap_clocks(&converted);
    let gate_playback = playback(gate);
    assert_eq!(key_millis(gate_playback), [(1_000, 1_000), (5_000, 5_000)]);
    assert_eq!(
        (gate_playback.before(), gate_playback.after()),
        (
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive
        ),
        "the lifetime gate keeps the layer inactive outside its native span"
    );
    let remap = playback(remap.expect("authored remap"));
    assert_eq!(key_millis(remap), [(2_000, 0), (4_000, 2_000)]);
    assert_eq!(
        (remap.before(), remap.after()),
        (TimeRemapExtrapolation::Hold, TimeRemapExtrapolation::Hold)
    );
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
}

/// Native CAP2 comp 281/layer 313 record: one authored key at source
/// 1.833333s, lifetime 55/24..98/24s. Zero keys keep the affine clock.
#[test]
fn one_authored_remap_key_is_constant_over_the_layer_lifetime() {
    let one_key = |keys: &[(i32, f64)]| {
        let mut project = fixture("avlayer_flags.aep");
        let layer = &mut composition_mut(&mut project, 125).layers[0];
        set_layer_clock(layer, [(11, 24), (44, 24), (87, 24)]);
        set_time_remap_keys(layer, keys);
        project
    };
    let project = one_key(&[(2, 1.833_333_333_333_333_3)]);
    let converted = to_structural_fx_document(&project, Some(125)).unwrap();
    let (gate, remap) = remap_clocks(&converted);
    assert_eq!(key_millis(playback(gate)), [(2_292, 2_292), (4_083, 4_083)]);
    let remap = playback(remap.expect("constant authored remap"));
    assert_eq!(key_millis(remap), [(2_292, 1_833), (4_083, 1_833)]);
    assert_eq!(
        (remap.before(), remap.after()),
        (TimeRemapExtrapolation::Hold, TimeRemapExtrapolation::Hold)
    );
    let diagnostics = remap_diagnostics(&converted);
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("one authored key"))
    );
    assert!(
        !diagnostics
            .iter()
            .any(|message| message.contains("at least two")),
        "{diagnostics:?}"
    );
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("fractional-millisecond endpoints rounded")
    }));

    // An allowance that admits only the gate rolls it back with the constant.
    let gate_bytes =
        super::animation_budget::committed_remap_serialized_bytes(playback(gate)).unwrap() + 1;
    let limited =
        to_structural_fx_document_with_animation_limit(&project, Some(125), gate_bytes).unwrap();
    let (content, remap) = remap_clocks(&limited);
    assert!(remap.is_none() && content.playback.time_remap().is_none());
    assert!(limited.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("over-budget remap visibility gate")
            && diagnostic.message.contains("one-key constant remap")
    }));

    // No authored key is not a constant: the affine source clock remains.
    // Its unit stretch spans the rounded 1791 ms window from the rounded
    // source origin; the rounded out point 3625 ms would be 1792/1791x.
    let converted = to_structural_fx_document(&one_key(&[]), Some(125)).unwrap();
    let (content, remap) = remap_clocks(&converted);
    assert!(remap.is_none());
    assert_eq!(
        key_millis(playback(content)),
        [(2_292, 1_833), (4_083, 3_624)]
    );
    assert!(
        remap_diagnostics(&converted)
            .iter()
            .any(|message| message.contains("no supported authored keys"))
    );
}

/// Native CAP2 comp 625/layer 653 shape: local keys -1s->0s and 0s->1s,
/// parent keys 0..1s, lifetime 0..1.75s and the exact cycle expression.
fn cycled_remap_project(expression: &str, enabled: bool) -> StructuralProject {
    let mut project = fixture("avlayer_flags.aep");
    let layer = &mut composition_mut(&mut project, 125).layers[0];
    set_layer_clock(layer, [(4, 4), (-4, 4), (3, 4)]);
    set_time_remap_keys(layer, &[(-1, 0.0), (0, 1.0)]);
    set_time_remap_expression(layer, expression, enabled);
    project
}

#[test]
fn exact_two_sided_cycle_expression_loops_the_authored_keys() {
    for expression in [
        "loopIn() + loopOut() - value;",
        " loopIn()+loopOut()-value \n",
    ] {
        let converted =
            to_structural_fx_document(&cycled_remap_project(expression, true), Some(125)).unwrap();
        let (gate, remap) = remap_clocks(&converted);
        let gate_playback = playback(gate);
        assert_eq!(key_millis(gate_playback), [(0, 0), (1_750, 1_750)]);
        assert_eq!(gate_playback.after(), TimeRemapExtrapolation::Inactive);
        let remap = playback(remap.expect("cycled authored remap"));
        assert_eq!(
            key_millis(remap),
            [(0, 0), (1_000, 1_000)],
            "{expression:?}"
        );
        assert_eq!(
            (remap.before(), remap.after()),
            (TimeRemapExtrapolation::Loop, TimeRemapExtrapolation::Loop),
            "{expression:?}"
        );
        let layer_id = composition(&cycled_remap_project(expression, true), 125).layers[0]
            .record
            .id();
        let loop_notes: Vec<_> = converted
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("approximated by FX Loop"))
            .collect();
        assert_eq!(loop_notes.len(), 1, "{expression:?}");
        let note = loop_notes[0];
        assert_eq!(note.limitation, Limitation::Timing);
        assert_eq!(
            (note.composition_id, note.layer_id),
            (Some(125), Some(layer_id))
        );
        assert!(note.message.contains("final authored key"));
        assert!(note.message.contains("not established"));
    }

    // A disabled expression does not run: AE holds the authored end values.
    let disabled = to_structural_fx_document(
        &cycled_remap_project("loopIn() + loopOut() - value;", false),
        Some(125),
    )
    .unwrap();
    let remap = playback(remap_clocks(&disabled).1.expect("authored remap"));
    assert_eq!(remap.after(), TimeRemapExtrapolation::Hold);
    assert!(
        !remap_diagnostics(&disabled)
            .iter()
            .any(|message| message.contains("approximated by FX Loop"))
    );
}

#[test]
fn other_remap_expressions_stay_diagnosed_without_authored_keys() {
    for expression in [
        "loopOut();",
        "loopOut() + loopIn() - value;",
        "loopIn('cycle') + loopOut() - value;",
        "loopIn() + loopOut('pingpong') - value;",
        "loopIn() + loopOut() - valueAtTime(0);",
        "loopIn() + loopOut() - value; // cycle",
        "loopIn() + loopOut() - value;;",
        "loopIn() + loopOut() - value * 2;",
    ] {
        let converted =
            to_structural_fx_document(&cycled_remap_project(expression, true), Some(125)).unwrap();
        let (content, remap) = remap_clocks(&converted);
        assert!(remap.is_none(), "{expression:?}");
        assert!(
            content.playback.time_remap().is_some(),
            "{expression:?}: affine source clock retained"
        );
        assert!(
            remap_diagnostics(&converted)
                .iter()
                .any(|message| message.contains("not the exact")),
            "{expression:?}"
        );
    }
}

#[test]
fn repeated_and_nested_sources_get_distinct_editable_occurrences() {
    // Supplemental graph stress derived from a pinned native precomp, not a new Adobe fidelity case.
    let mut project = fixture("outPoint_clamp.aep");
    let mut first = composition(&project, 13).layers[0].clone();
    let mut second = first.clone();
    patch(&mut second, 0, &999_u32.to_be_bytes());
    let mut nested = first.clone();
    patch(&mut nested, 40, &26_u32.to_be_bytes());
    patch(&mut nested, 0, &1000_u32.to_be_bytes());
    // A source layer ID may repeat in distinct compositions/instances; FX IDs may not.
    patch(&mut first, 0, &25_u32.to_be_bytes());
    composition_mut(&mut project, 13).layers = vec![first, second, nested];
    let converted = to_structural_fx_document(&project, Some(13)).unwrap();
    let mut ids = HashSet::new();
    fn walk(group: &GroupLayer, ids: &mut HashSet<LayerId>) {
        assert!(ids.insert(group.id));
        for layer in &group.layers {
            let child = as_group(layer);
            assert_eq!(child.parent, Some(group.id));
            walk(child, ids);
        }
    }
    walk(root(&converted), &mut ids);
    assert_eq!(
        ids.len(),
        9,
        "four occurrences each have a distinct content clock"
    );
    assert!(has(&converted, Limitation::IndependentCopies));

    let limited = to_structural_fx_document_with_animation_limit(&project, Some(13), 1)
        .expect("repeated precomps must survive animation exhaustion");
    let mut limited_ids = HashSet::new();
    walk(root(&limited), &mut limited_ids);
    assert_eq!(limited_ids.len(), 9);
    assert!(has(&limited, Limitation::ExpansionLimit));
}

#[test]
fn cyclic_and_missing_references_do_not_abort_supported_siblings() {
    let mut project = fixture("outPoint_clamp.aep");
    let mut cycle = composition(&project, 13).layers[0].clone();
    patch(&mut cycle, 40, &13_u32.to_be_bytes());
    let mut missing = cycle.clone();
    patch(&mut missing, 0, &999_u32.to_be_bytes());
    patch(&mut missing, 40, &9999_u32.to_be_bytes());
    let good = composition(&project, 26).layers[0].clone();
    composition_mut(&mut project, 13).layers = vec![cycle, missing, good];
    let converted = to_structural_fx_document(&project, Some(13)).unwrap();
    assert_eq!(root(&converted).layers.len(), 3);
    assert!(has(&converted, Limitation::Cycle));
    assert!(has(&converted, Limitation::MissingReference));
    assert!(has(&converted, Limitation::Placeholder));
    assert!(converted.document.to_json_vec().is_ok());
}

#[test]
fn invalid_timing_is_warned_and_retained_as_non_rendering_content() {
    let mut project = fixture("outPoint_clamp.aep");
    let layer = &mut composition_mut(&mut project, 13).layers[0];
    patch(layer, 24, &0_u32.to_be_bytes());
    let converted = to_structural_fx_document(&project, Some(13)).unwrap();
    assert_eq!(root(&converted).layers.len(), 1);
    assert!(has(&converted, Limitation::Timing));
    let occurrence = as_group(&root(&converted).layers[0]);
    assert_eq!(occurrence.name, "Precomp_5s");
    assert_ne!(occurrence.playback.input_range().duration, Duration::ZERO);
    let content = as_group(&occurrence.layers[0]);
    assert!(content.is_hidden);
    assert_ne!(content.playback.input_range().duration, Duration::ZERO);
    assert_eq!(
        content.playback,
        identity_playback(content.playback.input_range())
    );
}

#[test]
fn wholly_before_zero_reverse_occurrence_stays_non_rendering() {
    let project = fixture("layer_timing.aep");
    let layer = &composition(&project, 30).layers[0];
    assert_eq!(layer.record.id(), 43);
    assert!(layer.record.stretch().is_some_and(|stretch| stretch < 0.0));

    let converted = to_structural_fx_document(&project, Some(30)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    let content = as_group(&occurrence.layers[0]);
    assert!(content.is_hidden);
    assert_eq!(content.playback.input_range().start, Time::ZERO);
    assert_ne!(content.playback.input_range().duration, Duration::ZERO);
    assert_eq!(
        content.playback,
        identity_playback(content.playback.input_range())
    );
    assert!(has(&converted, Limitation::Timing));
}

#[test]
fn complex_native_source_imports_its_ordered_layer_stack() {
    let project = fixture("complex_comp.aep");
    let converted = to_structural_fx_document(&project, Some(56)).unwrap();
    assert_eq!(root(&converted).name, "complex");
    let names: Vec<_> = root(&converted)
        .layers
        .iter()
        .map(|layer| as_group(layer).name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Empty Layer",
            "Artboard 2 Layer",
            "Parent Layer",
            "Effects Layer",
            "Text Layer",
            "Shapes",
            "Background",
            "Calque 1"
        ]
    );
    assert!(has(&converted, Limitation::Placeholder));
    // This source has no companion AE JSON. This is a structural smoke check,
    // not an independently validated property/render fidelity case.
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
}

#[test]
fn recursive_depth_is_bounded_and_truncation_is_reported() {
    let mut project = fixture("outPoint_clamp.aep");
    let template = project.item(13).unwrap().clone();
    for index in 0..30_u32 {
        let mut item = template.clone();
        item.id = 1000 + index;
        let ItemKind::Composition(comp) = &mut item.kind else {
            unreachable!();
        };
        patch(&mut comp.layers[0], 40, &(1001 + index).to_be_bytes());
        project.items.push(item);
    }
    let deep = to_structural_fx_document(&project, Some(1000)).unwrap();
    assert!(has(&deep, Limitation::ExpansionLimit));
    EditableFxCompositionDocument::from_json_slice(&deep.document.to_json_vec().unwrap()).unwrap();
}

#[test]
fn wide_native_graph_keeps_siblings_beyond_former_occurrence_limit() {
    // Stress a pinned native graph without duplicating its property blobs.
    // This is structural stress coverage, not independent Adobe render proof.
    const LAYER_COUNT: u32 = 10_001;
    let mut project = fixture("outPoint_clamp.aep");
    let mut first = composition(&project, 13).layers[0].clone();
    first.content.clear();
    composition_mut(&mut project, 13).layers = (0..LAYER_COUNT)
        .map(|index| {
            let mut layer = first.clone();
            patch(&mut layer, 0, &(index + 1).to_be_bytes());
            layer
        })
        .collect();
    let wide = to_structural_fx_document(&project, Some(13)).unwrap();
    assert_eq!(
        root(&wide).layers.len(),
        usize::try_from(LAYER_COUNT).unwrap()
    );
    let tail = as_group(root(&wide).layers.last().unwrap());
    assert!(tail.description.contains("layer=10001 "));
    assert!(
        !tail.layers.is_empty(),
        "the final source clock must survive"
    );
    assert!(!has(&wide, Limitation::ExpansionLimit));
    EditableFxCompositionDocument::from_json_slice(&wide.document.to_json_vec().unwrap()).unwrap();
}

#[test]
fn unknown_kinds_and_missing_links_preserve_identity_and_visibility() {
    let mut project = fixture("layer_misc.aep");
    let layer = &mut composition_mut(&mut project, 44).layers[0];
    patch(layer, 131, &[255]);
    patch(layer, 132, &99999_u32.to_be_bytes());
    // Solo the first layer, preserving the second layer's place but hiding it.
    let flag = layer.record.raw_bytes()[38] | (1 << 3);
    patch(layer, 38, &[flag]);
    let converted = to_structural_fx_document(&project, Some(44)).unwrap();
    assert_eq!(root(&converted).layers.len(), 2);
    assert!(!as_group(&root(&converted).layers[0]).is_hidden);
    assert!(as_group(&root(&converted).layers[1]).is_hidden);
    assert!(has(&converted, Limitation::MissingReference));
    assert!(has(&converted, Limitation::Placeholder));
    assert!(
        as_group(&root(&converted).layers[0])
            .description
            .contains("kind=255")
    );
}

#[test]
fn omitted_selection_requires_exactly_one_composition_even_with_a_unique_root() {
    let mut project = fixture("outPoint_clamp.aep");
    let error = to_structural_fx_document(&project, None)
        .err()
        .expect("multiple compositions must be ambiguous");
    assert!(matches!(
        error,
        DocumentError::AmbiguousCompositionSelection { count: 5 }
    ));

    // Item 1 is a nested composition referenced by every root in this fixture.
    let nested = to_structural_fx_document(&project, Some(1)).unwrap();
    assert_eq!(root(&nested).name, "Precomp_5s");
    assert!(!has(&nested, Limitation::Selection));

    project.items.retain(|item| [13, 26].contains(&item.id));
    patch(
        &mut composition_mut(&mut project, 13).layers[0],
        40,
        &26_u32.to_be_bytes(),
    );
    patch(
        &mut composition_mut(&mut project, 26).layers[0],
        40,
        &13_u32.to_be_bytes(),
    );
    assert!(matches!(
        to_structural_fx_document(&project, None),
        Err(DocumentError::AmbiguousCompositionSelection { count: 2 })
    ));
    let converted = to_structural_fx_document(&project, Some(13)).unwrap();
    assert_eq!(root(&converted).name, "outPoint_clamp_precomp");
    assert!(has(&converted, Limitation::Cycle));
    assert!(!has(&converted, Limitation::Selection));
}

#[test]
fn omitted_selection_handles_zero_or_one_composition_and_explicit_ids_stay_strict() {
    let one = read_project(include_bytes!("../../tests/fixtures/ae26_one_comp.aep")).unwrap();
    let converted = to_structural_fx_document(&one, None).unwrap();
    assert_eq!(root(&converted).name, "classic-3d");

    assert!(matches!(
        to_structural_fx_document(&one, Some(u32::MAX)),
        Err(DocumentError::CompositionSelection(u32::MAX))
    ));

    let mut zero = one;
    zero.items
        .retain(|item| !matches!(&item.kind, ItemKind::Composition(_)));
    assert!(matches!(
        to_structural_fx_document(&zero, None),
        Err(DocumentError::NoComposition)
    ));
}

#[test]
fn non_av_comp_inputs_are_placeholders_not_visible_precomp_occurrences() {
    let mut project = fixture("outPoint_clamp.aep");
    patch(&mut composition_mut(&mut project, 13).layers[0], 131, &[1]);
    let converted = to_structural_fx_document(&project, Some(13)).unwrap();
    assert!(has(&converted, Limitation::Placeholder));
    assert_eq!(
        converted
            .diagnostics
            .iter()
            .filter(|warning| warning.limitation == Limitation::GroupBounds)
            .count(),
        1
    );
}

#[test]
fn long_unicode_names_are_retained_exactly_for_composition_and_layers() {
    let mut project = fixture("outPoint_clamp.aep");
    let shared: std::sync::Arc<str> = "名前".repeat(2000).into();
    let item = project.items.iter_mut().find(|item| item.id == 13).unwrap();
    item.name = shared.to_string();
    let ItemKind::Composition(composition) = &mut item.kind else {
        panic!("item 13 must be a composition")
    };
    let mut layer = composition.layers[0].clone();
    layer.name = shared.clone();
    let mut second = layer.clone();
    patch(&mut second, 0, &999_u32.to_be_bytes());
    assert!(std::sync::Arc::ptr_eq(&layer.name, &second.name));
    composition.layers = vec![layer, second];

    let converted = to_structural_fx_document(&project, Some(13)).unwrap();
    assert_eq!(converted.document.composition().name(), shared.as_ref());
    assert_eq!(root(&converted).name, shared.as_ref());
    let mut key_ids = HashSet::new();
    for layer in &root(&converted).layers {
        let group = as_group(layer);
        assert_eq!(group.name, shared.as_ref());
        assert_eq!(
            group.playback,
            identity_playback(group.playback.input_range())
        );
        let playback = as_group(&group.layers[0])
            .playback
            .time_remap()
            .expect("expected source remap keyframes");
        for key in playback.keyframes() {
            assert!(key_ids.insert(key.id.as_str()));
        }
    }
    assert!(
        converted
            .diagnostics
            .iter()
            .all(|warning| !warning.message.contains("display name truncated"))
    );
}

#[test]
fn essential_overrides_are_applied_during_fresh_occurrence_conversion() {
    let mut project = read_project(include_bytes!(
        "../../tests/fixtures/essential/multiple_controllers.aep"
    ))
    .unwrap();
    // Supplemental perturbation distinguishes a native explicit 100% override
    // from a missing override that happens to equal AE's default opacity.
    let source = &mut composition_mut(&mut project, 1).layers[0];
    assert_eq!(source.record.id(), 15);
    set_static_transform(source, &[("ADBE Opacity", &[0.35])]);
    let original = source.clone();
    let converted = to_structural_fx_document(&project, Some(16)).unwrap();
    let occurrence = as_group(&root(&converted).layers[0]);
    let content = as_group(&occurrence.layers[0]);
    let overridden = as_group(&content.layers[0]);
    assert_eq!(overridden.transform.opacity.value(), 100.0);
    assert_eq!(composition(&project, 1).layers[0], original);
    let direct = to_structural_fx_document(&project, Some(1)).unwrap();
    assert_eq!(
        as_group(&root(&direct).layers[0]).transform.opacity.value(),
        35.0
    );
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
}

// Repository-only ledger coverage runs in scripts/test-aep-support-ledger.py.
// Keep private docs out of this portable crate's compile-time source inputs.
