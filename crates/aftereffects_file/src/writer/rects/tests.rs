use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::to_structural_fx_document;
use fx_schema::LayerData;

fn rect() -> VectorRectSpec {
    VectorRectSpec {
        name: "Edited Rectangle".into(),
        stroke_dashes: StrokeDashes::default(),
        size: [100.0, 50.0],
        position: [12.0, -9.0],
        roundness: 4.0,
        fill_color: None,
        stroke_color: Some([0.1, 0.5, 0.9, 1.0]),
        stroke_width: 6.0,
        stroke_join: ShapeLineJoin::Miter,
        stroke_miter_limit: 4.0,
        transform: SolidTransform {
            anchor: [50.0, 25.0],
            position: [320.0, 240.0],
            scale: [100.0, 100.0],
            rotation: 0.0,
            opacity: 100.0,
        },
    }
}

fn spec() -> CompositionSpec {
    CompositionSpec {
        name: "Fresh Vector".into(),
        width: 640,
        height: 480,
        duration_frames: 48,
    }
}

#[test]
fn writes_fresh_source_less_editable_rectangle_and_stroke() {
    let bytes = write_composition(&spec(), &[LayerSpec::Rect(rect())]).unwrap();
    assert_eq!(
        bytes,
        write_composition(&spec(), &[LayerSpec::Rect(rect())]).unwrap()
    );
    let parsed = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &parsed.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    assert_eq!(comp.layers.len(), 1);
    let layer = &comp.layers[0];
    assert_eq!(layer.record.layer_type(), 4);
    assert_eq!(layer.record.source_id(), 0);
    assert_eq!(layer.name.as_ref(), "Edited Rectangle");
    let imported = to_structural_fx_document(&parsed, Some(1)).unwrap();
    fn find_rect(layers: &[fx_schema::Layer]) -> Option<&fx_schema::RectLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::Rect(rect) if !rect.is_hidden => Some(rect),
            LayerData::Group(group) => find_rect(&group.layers),
            _ => None,
        })
    }
    let result =
        find_rect(imported.document.composition().layers()).expect("native Rect and Stroke");
    assert_eq!(result.rect.size, [100.0, 50.0]);
    assert_eq!(result.rect.roundness, 4.0);
    assert_eq!(
        result.transform.position,
        fx_schema::Position::TwoD([12.0, -9.0])
    );
    assert_eq!(result.rect.stroke_width.value(), 6.0);
    assert!(!result.rect.fill_enabled);
    assert!(result.rect.stroke_enabled);
    assert!(
        imported
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .all(|entry| {
                entry.target.layer_id() != Some(result.id) || !entry.animator.is_js_script()
            })
    );
}

#[test]
fn vector_collections_keep_native_indexed_group_discriminators() {
    fn collect(chunks: &[Chunk], flags: &mut Vec<u32>) {
        for pair in chunks.windows(2) {
            if pair[0].id() != *b"tdmn" {
                continue;
            }
            let name = pair[0]
                .data_payload()
                .unwrap()
                .split(|byte| *byte == 0)
                .next();
            if name != Some(b"ADBE Root Vectors Group".as_slice())
                && name != Some(b"ADBE Vectors Group".as_slice())
            {
                continue;
            }
            let discriminator = pair[1]
                .children()
                .unwrap()
                .iter()
                .find(|chunk| chunk.id() == *b"tdsb")
                .unwrap()
                .data_payload()
                .unwrap();
            flags.push(u32::from_be_bytes(discriminator.try_into().unwrap()));
        }
        for children in chunks.iter().filter_map(Chunk::children) {
            collect(children, flags);
        }
    }
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/geometry_rect_size.aep"
    ))
    .unwrap();
    let mut native_flags = Vec::new();
    for item in &native.items {
        if let ItemKind::Composition(comp) = &item.kind {
            for layer in &comp.layers {
                collect(&layer.content, &mut native_flags);
            }
        }
    }
    assert_eq!(native_flags, [0x401, 0x401]);

    let bytes = write_composition(&spec(), &[LayerSpec::Rect(rect())]).unwrap();
    let generated = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &generated.item(1).unwrap().kind else {
        panic!("fresh composition");
    };
    let mut generated_flags = Vec::new();
    collect(&comp.layers[0].content, &mut generated_flags);
    assert_eq!(generated_flags, native_flags);
}

#[test]
fn rejects_bad_rect_before_writing_and_preserves_mixed_order() {
    let mut invalid = rect();
    invalid.size[0] = -1.0;
    assert!(write_composition(&spec(), &[LayerSpec::Rect(invalid)]).is_err());
    let solid = SolidLayerSpec {
        name: "Second solid".into(),
        width: 32,
        height: 16,
        color: [1.0, 0.0, 0.0],
        transform: rect().transform,
    };
    let bytes =
        write_composition(&spec(), &[LayerSpec::Rect(rect()), LayerSpec::Solid(solid)]).unwrap();
    let parsed = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &parsed.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    assert_eq!(
        comp.layers
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>(),
        ["Edited Rectangle", "Second solid"]
    );
    assert_eq!(comp.layers[0].record.source_id(), 0);
    assert_ne!(comp.layers[1].record.source_id(), 0);
}
