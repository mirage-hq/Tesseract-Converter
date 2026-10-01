//! Independent AE sourceRectAtTime probe, not a rendered-image comparison.
use super::*;
use crate::structure::{ItemKind, read_project};

fn named_group<'a>(
    layers: &'a [fx_schema::Layer],
    name: &str,
) -> Option<&'a fx_schema::GroupLayer> {
    for layer in layers {
        if let FxLayer::Group(group) = layer.data() {
            if group.name == name {
                return Some(group);
            }
            if let Some(found) = named_group(&group.layers, name) {
                return Some(found);
            }
        }
    }
    None
}

fn painted_rect(layers: &[fx_schema::Layer]) -> Option<&fx_schema::RectLayer> {
    for layer in layers {
        match layer.data() {
            FxLayer::Rect(rect) if !rect.is_hidden => return Some(rect),
            FxLayer::Group(group) => {
                if let Some(found) = painted_rect(&group.layers) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn pinned_complex_shape_sources_import_without_converter_js() {
    // This is a script-free invariant, not proof of fidelity for omitted
    // animated/compound shape semantics or an independent render comparison.
    for bytes in [
        include_bytes!("../../../tests/fixtures/geometry/geometry_probe.aep").as_slice(),
        include_bytes!("../../../tests/fixtures/shapes/gradient.aep").as_slice(),
        include_bytes!("../../../tests/fixtures/shapes/shape_misc.aep").as_slice(),
    ] {
        let project = read_project(bytes).unwrap();
        let item = project
            .items
            .iter()
            .find(|item| matches!(item.kind, ItemKind::Composition(_)))
            .unwrap();
        let imported =
            crate::structure_document::to_structural_fx_document(&project, Some(item.id)).unwrap();
        assert!(
            imported
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .all(|entry| { !entry.animator.is_js_script() }),
            "{} produced converter JS",
            item.name
        );
    }
}

#[test]
fn native_rectangle_matches_independent_source_rect_probe() {
    use sha2::{Digest, Sha256};
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/geometry");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("provenance.json")).unwrap()).unwrap();
    for file in manifest["files"].as_array().unwrap() {
        let bytes = std::fs::read(directory.join(file["path"].as_str().unwrap())).unwrap();
        assert_eq!(bytes.len() as u64, file["size"].as_u64().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            file["sha256"].as_str().unwrap()
        );
    }
    let project =
        read_project(&std::fs::read(directory.join("geometry_probe.aep")).unwrap()).unwrap();
    let item = project
        .items
        .iter()
        .find(|item| item.name == "PROBE_MAIN")
        .unwrap();
    let ItemKind::Composition(native) = &item.kind else {
        panic!("probe composition");
    };
    let source = native
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "shape_rect")
        .unwrap();
    assert_eq!((item.id, source.record.id()), (14, 40));
    assert_eq!(source.record.layer_type(), 4);
    let imported =
        crate::structure_document::to_structural_fx_document(&project, Some(item.id)).unwrap();
    let composition = imported.document.composition();
    assert!(imported.diagnostics.iter().any(|diagnostic| {
        format!("{diagnostic:?}").contains("identity vector-group wrapper normalized")
    }));
    let occurrence = named_group(composition.layers(), "shape_rect").unwrap();
    let paint = painted_rect(&occurrence.layers).expect("native editable Rect paint");
    assert!(paint.rect.stroke_enabled);
    assert!(paint.rect.stroke_color.is_some());
    assert!(!paint.rect.fill_enabled);
    assert!(composition.dynamics().entries().iter().all(|entry| {
        entry.target.layer_id() != Some(paint.id) || !entry.animator.is_js_script()
    }));
    let fx_schema::Position::TwoD(position) = paint.transform.position else {
        panic!("native Rectangle position must remain 2D")
    };
    let bounds = [
        paint.rect.position[0] - paint.transform.anchor_point[0] + position[0],
        paint.rect.position[1] - paint.transform.anchor_point[1] + position[1],
        paint.rect.size[0],
        paint.rect.size[1],
    ];
    let references: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.join("geometry_rects_probe.json")).unwrap(),
    )
    .unwrap();
    let expected = &references["shape_rect"]["t0_noext"]["value"];
    for (actual, name) in bounds.into_iter().zip(["left", "top", "width", "height"]) {
        assert!(
            (actual - expected[name].as_f64().unwrap()).abs() < 0.0002,
            "{name}: {actual} versus {}",
            expected[name]
        );
    }
    fx_schema::EditableFxCompositionDocument::from_json_slice(
        &imported.document.to_json_vec().unwrap(),
    )
    .unwrap();
}
