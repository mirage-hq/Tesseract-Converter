use super::*;
use crate::rifx::Chunk;

#[test]
fn sparse_fill_imports_native_red_and_full_amount() {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/fill_isolated.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!("pinned composition")
    };
    let owner = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 15)
        .unwrap();
    fn make_sparse(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"sspc") {
                let plugin = chunk.children_mut().unwrap();
                let table = crate::properties::unique_list(plugin, *b"parT").unwrap();
                for (name, expected) in [
                    ("ADBE Fill-0002", vec![1.0, 0.0, 0.0, 1.0]),
                    ("ADBE Fill-0005", vec![1.0]),
                ] {
                    let runs = crate::properties::runs(table).unwrap();
                    let (_, declaration) = runs.iter().find(|(key, _)| *key == name).unwrap();
                    assert_eq!(
                        native::default_numeric(declaration, [120.0, 80.0])
                            .unwrap()
                            .values,
                        expected,
                        "pinned native declaration establishes the omitted-control default"
                    );
                }
                for child in plugin {
                    if matches!(child.list_kind(), Some(kind) if kind == *b"parT" || kind == *b"tdgp")
                    {
                        child.children_mut().unwrap().clear();
                    }
                }
            } else if let Some(children) = chunk.children_mut() {
                make_sparse(children);
            }
        }
    }
    make_sparse(&mut owner.content);
    let (document, diagnostics) = imported_case(&project, 1);
    let record = effect_payload(&document, "tintTritone")
        .unwrap_or_else(|| panic!("editable Fill approximation missing: {diagnostics:?}"));
    for (field, expected) in [
        ("blackR", 1.0),
        ("whiteR", 1.0),
        ("blackG", 0.0),
        ("whiteG", 0.0),
        ("blackB", 0.0),
        ("whiteB", 0.0),
        ("amount", 100.0),
    ] {
        assert_eq!(record["effect"][field], expected, "{field}");
    }
}
