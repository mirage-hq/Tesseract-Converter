use super::*;

const NATIVE: &[u8] =
    include_bytes!("../../../tests/fixtures/media-replacement/media_replacement.aep");

fn contains_description(group: &GroupLayer, text: &str) -> bool {
    group.description.contains(text)
        || group.layers.iter().any(|layer| match layer.data() {
            FxLayer::Group(group) => contains_description(group, text),
            _ => false,
        })
}

#[test]
fn native_media_replacement_expands_only_the_selected_occurrence() {
    let project = read_project(NATIVE).unwrap();
    let source = composition(&project, 2).layers[0].clone();
    assert_eq!(source.record.id(), 14);
    let converted = to_structural_fx_document(&project, Some(15)).unwrap();
    assert!(contains_description(root(&converted), "source=30 "));
    assert!(contains_description(
        root(&converted),
        "AEP comp=30 layer=42 "
    ));
    assert_eq!(composition(&project, 2).layers[0], source);
    let direct = to_structural_fx_document(&project, Some(2)).unwrap();
    assert!(!contains_description(root(&direct), "source=30 "));
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
}

#[test]
fn missing_essential_media_replacement_retains_original_source() {
    let mut project = read_project(NATIVE).unwrap();
    let source_id = composition(&project, 2).layers[0].record.source_id();
    project.items.retain(|item| item.id != 30);
    let converted = to_structural_fx_document(&project, Some(15)).unwrap();
    assert!(!contains_description(root(&converted), "source=30 "));
    assert!(contains_description(
        root(&converted),
        &format!("source={source_id} ")
    ));
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains(
            "replacement source 30 is missing or not an AV item; original source retained",
        )
    }));
}
