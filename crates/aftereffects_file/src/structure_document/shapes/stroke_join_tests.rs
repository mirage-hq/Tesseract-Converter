//! Modified native records are supplementary routing tests, not Adobe Join evidence.
use super::*;
use crate::structure::{ItemKind, read_project};

fn join_storage() -> Chunk {
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    meta[12..16].copy_from_slice(&1000_u32.to_be_bytes());
    meta[59] = 4;
    meta[68] = 1;
    let mut header = vec![0; 24];
    header[10..12].copy_from_slice(&3_u16.to_be_bytes());
    header[18..20].copy_from_slice(&48_u16.to_be_bytes());
    header[23] = 4;
    let mut data = Vec::new();
    for (time, value) in [(-1000_i32, 1.0_f64), (500, 2.0), (2000, 3.0)] {
        let mut key = vec![0; 48];
        key[..4].copy_from_slice(&time.to_be_bytes());
        key[4] = 3;
        key[5] = 3;
        key[8..16].copy_from_slice(&value.to_be_bytes());
        data.extend(key);
    }
    Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdb4", meta).unwrap(),
            Chunk::data(*b"tdsb", vec![0; 4]).unwrap(),
            Chunk::list(
                *b"list",
                vec![
                    Chunk::data(*b"lhd3", header).unwrap(),
                    Chunk::data(*b"ldat", data).unwrap(),
                ],
            ),
        ],
    )
}

fn match_name(name: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn named(chunk: &Chunk, name: &str) -> bool {
    chunk.id() == *b"tdmn"
        && chunk.data_payload().is_some_and(|bytes| {
            bytes.starts_with(name.as_bytes()) && bytes[name.len()..].iter().all(|byte| *byte == 0)
        })
}

fn replace_stroke(chunks: &mut [Chunk], paint: &str) -> usize {
    let mut count = 0;
    for index in 0..chunks.len() {
        if named(&chunks[index], "ADBE Vector Graphic - G-Stroke") {
            chunks[index] = match_name(paint);
            let group = chunks[index + 1..]
                .iter_mut()
                .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                .expect("native gradient stroke group");
            let leaves = group.children_mut().unwrap();
            let insertion = leaves
                .iter()
                .position(|chunk| named(chunk, "ADBE Vector Stroke Line Join"))
                .unwrap_or_else(|| {
                    leaves
                        .iter()
                        .position(|chunk| named(chunk, "ADBE Group End"))
                        .unwrap_or(leaves.len())
                });
            if insertion < leaves.len() && named(&leaves[insertion], "ADBE Vector Stroke Line Join")
            {
                let end = leaves[insertion + 1..]
                    .iter()
                    .position(|chunk| chunk.id() == *b"tdmn")
                    .map_or(leaves.len(), |offset| insertion + 1 + offset);
                leaves.drain(insertion..end);
            }
            leaves.splice(
                insertion..insertion,
                [match_name("ADBE Vector Stroke Line Join"), join_storage()],
            );
            count += 1;
        }
        if let Some(children) = chunks[index].children_mut() {
            count += replace_stroke(children, paint);
        }
    }
    count
}

#[test]
fn solid_and_gradient_strokes_route_join_keys_to_editable_targets() {
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/shapes/gradient.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("pinned gradient composition")
    };
    let source = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 13)
        .unwrap();
    for paint in [
        "ADBE Vector Graphic - Stroke",
        "ADBE Vector Graphic - G-Stroke",
    ] {
        let mut layer = source.clone();
        assert!(replace_stroke(&mut layer.content, paint) > 0);
        let mut parent =
            super::super::group(LayerId::new(1), "source".into(), None, full_active_range());
        let mut output_budget = OutputBudget::default();
        let mut animation_budget = AnimationBudget::default();
        let imported = import(
            &layer,
            &parent,
            24,
            &mut 2,
            &mut output_budget,
            &mut animation_budget,
        )
        .unwrap();
        let joins: Vec<_> = imported
            .animations
            .iter()
            .filter(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|property| property.property_type() == PropType::StrokeJoin)
            })
            .collect();
        assert!(!joins.is_empty(), "{paint}: {:?}", imported.warnings);
        for entry in joins {
            let keys = entry.animator.keyframe_track().unwrap().keyframes();
            assert_eq!(keys.len(), 3);
            for (key, join) in keys.iter().zip(["miter", "round", "bevel"]) {
                assert_eq!(key.value(), &fx_schema::PropertyValue::String(join.into()));
                assert_eq!(
                    key.easing(),
                    fx_schema::animator::PropertyKeyframeEasing::Hold
                );
            }
        }
        assert!(
            imported
                .animations
                .iter()
                .all(|entry| !entry.animator.is_js_script())
        );
        parent.layers = super::super::stored_layers(imported.layers).unwrap();
        fx_schema::FXComposition::try_from_parts(
            fx_schema::CompositionId::new("stroke-join"),
            "Stroke Join",
            fx_schema::AnimationGraph::from_entries(imported.animations).unwrap(),
            super::super::stored_layers(vec![FxLayer::Group(parent)]).unwrap(),
        )
        .unwrap();
    }
}
