use super::*;

fn owner(border: f64, mode: f64) -> Layer {
    let mut owner = catalog_owner(&[
        native_effect("ADBE Luma Key", &[("-0001", mode), ("-0002", border)]),
        native_effect("ADBE Gaussian Blur 2", &[]),
    ]);
    fn rename(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"tdmn"
                && let Some(name) = chunk_match_name(chunk)
                && let Some(suffix) = name.strip_prefix("ADBE Luma Key")
            {
                let mut bytes = format!("ADBE Roughen Edges{suffix}").into_bytes();
                bytes.resize(40, 0);
                *chunk = Chunk::data(*b"tdmn", bytes).unwrap();
            }
            if let Some(children) = chunk.children_mut() {
                rename(children);
            }
        }
    }
    rename(&mut owner.content);
    owner
}

#[test]
fn static_roughen_border_keeps_editable_erosion_and_effect_order() {
    let imported = import_cosmic(&owner(23., 1.));
    assert_eq!(imported.effects.len(), 4, "{:?}", imported.warnings);
    assert_eq!(imported.native_ordinals, [1, 1, 1, 2]);
    for (record, expected) in imported.effects.iter().zip([10., 10., 3.]) {
        let value = serde_json::to_value(record).unwrap();
        assert_eq!(value["effect"]["type"], "simpleChoker");
        assert_eq!(value["effect"]["choke"], expected);
    }
    let last = serde_json::to_value(imported.effects.last().unwrap()).unwrap();
    assert_eq!(last["effect"]["type"], "gaussianBlur");
    assert!(imported.animations.is_empty());
    assert!(imported.warnings.iter().any(|w| {
        w.contains("Roughen Edges") && w.contains("fractal") && w.contains("approximation")
    }));
}

#[test]
fn sparse_roughen_uses_only_same_file_declared_mode_default() {
    for mode in [1_u32, 2] {
        let mut owner = owner(23., 1.);
        assert!(patch_control(
            &mut owner.content,
            "ADBE Roughen Edges-0001",
            *b"pard",
            &mut |bytes| {
                bytes[56..60].copy_from_slice(&mode.to_be_bytes());
            }
        ));
        let definitions = vec![Chunk::list(
            *b"EfdG",
            crate::essential::instance_definitions(&owner.content),
        )];
        fn sparse(chunks: &mut [Chunk]) -> bool {
            for chunk in chunks {
                if chunk.list_kind() == Some(*b"sspc") {
                    let children = chunk.children_mut().unwrap();
                    let controls = children
                        .iter_mut()
                        .find(|c| c.list_kind() == Some(*b"tdgp"))
                        .unwrap()
                        .children_mut()
                        .unwrap();
                    if let Some(index) = controls
                        .iter()
                        .position(|c| chunk_match_name(c) == Some("ADBE Roughen Edges-0001"))
                    {
                        controls.drain(index..index + 2);
                        *children
                            .iter_mut()
                            .find(|c| c.list_kind() == Some(*b"parT"))
                            .unwrap() = Chunk::list(*b"parT", vec![]);
                        return true;
                    }
                }
                if let Some(children) = chunk.children_mut()
                    && sparse(children)
                {
                    return true;
                }
            }
            false
        }
        assert!(sparse(&mut owner.content));
        let imported = super::super::import_with_context(
            super::super::ImportContext {
                evaluations: &Default::default(),
                composition_id: 1,
                composition: None,
                items: None,
                effect_definitions: &definitions,
            },
            &owner,
            PLANE,
            PLANE,
            &mut 100,
            &mut Default::default(),
        );
        assert_eq!(
            imported.effects.len(),
            if mode == 1 { 4 } else { 1 },
            "{:?}",
            imported.warnings
        );
    }
}

#[test]
fn unsupported_roughen_mode_preserves_effect_sibling() {
    let imported = import_cosmic(&owner(23., 2.));
    assert_eq!(imported.effects.len(), 1);
    assert_eq!(imported.native_ordinals, [2]);
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("Roughen Edges") && w.contains("Edge Type"))
    );
}
