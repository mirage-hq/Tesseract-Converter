//! Supplemental dispatch regressions; not independent Adobe feature proof.
use super::*;
use crate::structure::{ItemKind, read_project};

#[test]
fn shape_adjustment_retains_editable_alpha_coverage() {
    // Supplemental switch mutation of an existing public native Shape fixture.
    // This is not independent Adobe-authored Adjustment or render evidence.
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/shapes/import_rectangle_controls.aep"
    ))
    .unwrap();
    let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
    let id = item.id;
    let ItemKind::Composition(comp) = &mut item.kind else {
        unreachable!()
    };
    let shape = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.layer_type() == 4)
        .unwrap();
    shape.record = shape.record.clone().with_adjustment_layer(true).unwrap();
    let imported =
        crate::structure_document::to_structural_fx_document(&project, Some(id)).unwrap();
    let json = imported.document.to_json_value().unwrap();
    let layers = json["composition"]["layers"][0]["layers"]
        .as_array()
        .unwrap();
    let adjustment = layers
        .iter()
        .find(|layer| layer["type"] == "Adjustment")
        .unwrap();
    let matte_id = adjustment["trackMatte"]["layer"]
        .as_u64()
        .expect("Shape adjustment needs an alpha coverage guide, not full-canvas effects");
    assert_eq!(adjustment["trackMatte"]["mode"], "alpha");
    let guide = layers
        .iter()
        .find(|layer| layer["id"].as_u64() == Some(matte_id))
        .unwrap();
    assert_eq!(guide["type"], "Group");
    fn contains_rect(layer: &serde_json::Value) -> bool {
        layer["type"] == "Rect"
            || layer["layers"]
                .as_array()
                .is_some_and(|children| children.iter().any(contains_rect))
    }
    assert!(contains_rect(guide), "guide retains an editable Rect node");
    assert!(!guide.to_string().contains("JsScript"));
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|note| note.message.contains("Shape Adjustment coverage"))
    );
}

fn numeric_property_mut<'a>(chunks: &'a mut [Chunk], name: &str) -> Option<&'a mut Chunk> {
    let start = chunks.iter().position(|chunk| {
        chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|bytes| bytes.split(|byte| *byte == 0).next() == Some(name.as_bytes()))
    });
    if let Some(start) = start {
        let end = chunks[start + 1..]
            .iter()
            .position(|chunk| chunk.id() == *b"tdmn")
            .map_or(chunks.len(), |offset| start + 1 + offset);
        return chunks[start + 1..end]
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdbs"));
    }
    for chunk in chunks {
        if let Some(children) = chunk.children_mut()
            && let Some(property) = numeric_property_mut(children, name)
        {
            return Some(property);
        }
    }
    None
}

fn feather(valid: bool) -> Chunk {
    let mut bytes = vec![0_u8; if valid { 124 } else { 12 }];
    bytes[..4].copy_from_slice(&[0xdb, 0x99, 0, 2]);
    if valid {
        bytes[59] = 4;
        bytes[60] = 6;
    }
    Chunk::list(
        *b"tdgp",
        vec![
            effect_match_name("ADBE Mask Feather").unwrap(),
            Chunk::list(*b"tdbs", vec![Chunk::data(*b"tdb4", bytes).unwrap()]),
        ],
    )
}

fn descriptor(chunk: &Chunk) -> &[u8] {
    chunk.children().unwrap()[1].children().unwrap()[0]
        .data_payload()
        .unwrap()
}

#[test]
fn animated_adjustment_guide_is_not_claimed_exact_after_track_pruning() {
    // Supplementary mutation of the pinned native source, not Adobe render proof.
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/adjustment/native_controls.aep"
    ))
    .unwrap();
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 205)
        .unwrap();
    let ItemKind::Composition(composition) = &mut item.kind else {
        panic!("parented Adjustment composition")
    };
    let adjustment = composition
        .layers
        .iter_mut()
        .find(|layer| layer.record.flags().adjustment_layer)
        .unwrap();
    let mut donor = read_project(include_bytes!(
        "../../../tests/fixtures/properties/property_2D_position.aep"
    ))
    .unwrap();
    let ItemKind::Composition(donor_comp) = &mut donor
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!("animated Position donor composition")
    };
    let position = numeric_property_mut(&mut donor_comp.layers[0].content, "ADBE Position")
        .expect("native animated Position")
        .clone();
    *numeric_property_mut(&mut adjustment.content, "ADBE Position").expect("Adjustment Position") =
        position;
    assert!(
        crate::properties::read_transform(&adjustment.content)
            .unwrap()
            .iter()
            .any(|property| property.match_name == "ADBE Position"
                && property
                    .numeric
                    .as_ref()
                    .is_ok_and(|value| value.animated && !value.keyframes.is_empty()))
    );

    let converted = super::super::to_structural_fx_document(&project, Some(205)).unwrap();
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Animated Adjustment/parent gate transforms")
    }));
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Adjustment parent/gate geometry is only partially mapped")
    }));
    assert!(!converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Adjustment parent/static affine is carried exactly")
    }));
}

#[test]
fn adjustment_normalizes_all_feathers_after_malformed_siblings() {
    let mut chunks = vec![feather(false), feather(true), feather(true)];
    normalize_adjustment_mask_feather(&mut chunks);
    assert_eq!(descriptor(&chunks[0]).len(), 12);
    assert_eq!(descriptor(&chunks[1])[59], 0);
    assert_eq!(descriptor(&chunks[2])[59], 0);
}

#[test]
fn adjustment_sparse_definitions_preserve_unknown_table_records() {
    let table = Chunk::list(
        *b"parT",
        vec![Chunk::data(*b"VEND", vec![1, 2, 3, 4]).unwrap()],
    );
    let mut chunks = vec![Chunk::list(*b"sspc", vec![table.clone()])];
    restore_sparse_effect_definitions(&mut chunks, Some("ADBE Pro Levels2")).unwrap();
    assert_eq!(chunks[0].children().unwrap()[0], table);
}

#[test]
fn adjustment_sparse_definitions_only_fill_proven_empty_layouts() {
    for name in ["ADBE Gaussian Blur 2", "ADBE Pro Levels2"] {
        let mut chunks = vec![Chunk::list(*b"sspc", vec![Chunk::list(*b"parT", vec![])])];
        restore_sparse_effect_definitions(&mut chunks, Some(name)).unwrap();
        assert!(
            chunks[0].children().unwrap()[0]
                .children()
                .unwrap()
                .iter()
                .any(|chunk| chunk.id() == *b"pard")
        );
    }
}
