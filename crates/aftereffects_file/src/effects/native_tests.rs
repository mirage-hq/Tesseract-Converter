use super::read_effects;
use crate::{
    rifx::Chunk,
    writer::effects::{self, NativeEffect},
};

fn named(value: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    Chunk::data(*b"tdmn", bytes).unwrap()
}

#[test]
fn review_audit_duplicate_effect_parades_are_diagnosed_not_selected() {
    let content = vec![Chunk::list(
        *b"tdgp",
        vec![
            named("ADBE Effect Parade"),
            Chunk::list(*b"tdgp", vec![]),
            named("ADBE Effect Parade"),
            Chunk::list(*b"tdgp", vec![]),
        ],
    )];
    let (effects, warnings) = read_effects(&content, [100.0, 100.0]);
    assert!(effects.is_empty());
    assert!(warnings.iter().any(|warning| warning.contains("duplicate")));
}

fn set(effect: &mut NativeEffect, suffix: &str, values: &[f64]) {
    let parameter = effect
        .properties
        .iter_mut()
        .find(|parameter| parameter.match_name.ends_with(suffix))
        .unwrap();
    parameter.values = values.to_vec();
}

fn value(effect: &super::DecodedEffect, suffix: &str) -> Vec<f64> {
    effect
        .parameters
        .iter()
        .find(|parameter| parameter.match_name.ends_with(suffix))
        .unwrap()
        .numeric
        .as_ref()
        .unwrap()
        .values
        .clone()
}

#[test]
#[ignore = "requires pinned AEP_EXPRESSION_SOURCE; see support ledger"]
fn mixkit_native_sparse_shadow_regression() {
    use crate::{
        properties,
        structure::{ItemKind, read_project},
    };
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(std::env::var("AEP_EXPRESSION_SOURCE").unwrap()).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "cb8139e4a82f17dbaf121a1a7313e5a5990c96e243f5a62a51d570515eba7a84"
    );
    let project = read_project(&bytes).unwrap();
    let item = project.items.iter().find(|item| item.id == 20219).unwrap();
    let ItemKind::Composition(comp) = &item.kind else {
        panic!("Main composition missing")
    };
    let mut definition_counts = Vec::new();
    for id in [20261, 21018] {
        let layer = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == id)
            .unwrap();
        let root = properties::root_runs(&layer.content).unwrap();
        let (_, parade) = root
            .iter()
            .find(|(name, _)| *name == "ADBE Effect Parade")
            .unwrap();
        let runs = properties::runs(properties::unique_list(parade, *b"tdgp").unwrap()).unwrap();
        let (_, shadow) = runs
            .iter()
            .find(|(name, _)| *name == "ADBE Drop Shadow")
            .unwrap();
        let sspc = properties::unique_list(shadow, *b"sspc").unwrap();
        definition_counts.push(
            properties::runs(properties::unique_list(sspc, *b"parT").unwrap())
                .unwrap()
                .len(),
        );
        let (effects, warnings) = super::read_effects(&layer.content, [1920.0, 1080.0]);
        let shadow = effects
            .iter()
            .find(|effect| effect.match_name == "ADBE Drop Shadow")
            .unwrap();
        assert!(shadow.enabled, "{warnings:?}");
        assert!((value(shadow, "-0001")[0] - 0.17730103433132).abs() < 1e-8);
        assert_eq!(value(shadow, "-0002"), vec![25.5]);
        assert_eq!(value(shadow, "-0003"), vec![117.0]);
        assert_eq!(value(shadow, "-0004"), vec![40.0]);
        assert_eq!(value(shadow, "-0005"), vec![0.0]);
    }
    assert!(
        definition_counts[0] > definition_counts[1],
        "{definition_counts:?}"
    );
    eprintln!("native per-instance parameter definition counts: {definition_counts:?}");
}

/// `fill_isolated.readback.json` is Adobe's own readback: Fill Mask = 0 (none).
#[test]
fn adobe_native_unset_mask_path_declaration_is_decoded() {
    use crate::structure::{ItemKind, read_project};
    let project = read_project(include_bytes!(
        "../../tests/fixtures/effects/fill_isolated.aep"
    ))
    .expect("independently Adobe-authored Fill source");
    let ItemKind::Composition(comp) = &project.item(1).expect("pinned composition").kind else {
        panic!("composition 1")
    };
    let subject = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 15)
        .expect("pinned Fill owner");
    let (effects, warnings) = read_effects(&subject.content, [120.0, 80.0]);
    let parameter = |name: &str| {
        effects[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == name)
            .expect("declared Fill control")
    };
    let mask = parameter("ADBE Fill-0001");
    assert_eq!(mask.declared_kind, Ok(Some(12)));
    assert!(
        mask.unset_path,
        "an all-zero path declaration selects no mask"
    );
    assert!(mask.numeric.is_err(), "a path selector is not a number");
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("ADBE Fill-0001")),
        "a decoded unset path is not an unsupported default: {warnings:?}"
    );
    let color = parameter("ADBE Fill-0002");
    assert_eq!(color.declared_kind, Ok(Some(5)));
    assert!(!color.unset_path);
    assert_eq!(
        value(&effects[0], "-0002")[..3],
        [0.8, 0.2, 0.1].map(|v| f64::from(v as f32))
    );
}

/// Other effects keep their explicit values and catalog fallback when their
/// declarations are unreadable; the decoder only reports that state.
#[test]
fn unreadable_declarations_keep_the_catalog_fallback_for_other_effects() {
    use super::Declarations;
    let size = [320.0, 180.0];
    let mut shadow = effects::new_effect("ADBE Drop Shadow", true, size).unwrap();
    set(&mut shadow, "-0002", &[25.5]);
    let parade = effects::effect_parade(&[shadow], 7, size).unwrap();
    let decode = |edit: fn(&mut Vec<Chunk>)| {
        let mut parade = parade.clone();
        let plugin = parade
            .children_mut()
            .unwrap()
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"sspc"))
            .and_then(Chunk::children_mut)
            .unwrap();
        edit(plugin);
        let content = vec![Chunk::list(
            *b"tdgp",
            vec![named("ADBE Effect Parade"), parade, named("ADBE Group End")],
        )];
        read_effects(&content, size)
    };
    fn table(plugin: &mut [Chunk]) -> &mut Vec<Chunk> {
        plugin
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"parT"))
            .and_then(Chunk::children_mut)
            .unwrap()
    }
    for (edit, declarations, warning) in [
        (
            (|_| {}) as fn(&mut Vec<Chunk>),
            Declarations::Readable,
            None,
        ),
        (
            |plugin| plugin.retain(|chunk| chunk.list_kind() != Some(*b"parT")),
            Declarations::Missing,
            Some("native parameter definitions missing"),
        ),
        (
            |plugin| {
                let copy = table(plugin).clone();
                plugin.push(Chunk::list(*b"parT", copy));
            },
            Declarations::Unreadable,
            Some("duplicate native parameter definition tables"),
        ),
        (
            |plugin| {
                let table = table(plugin);
                let name = table
                    .iter()
                    .position(|chunk| chunk.id() == *b"tdmn")
                    .unwrap();
                table[name] = Chunk::data(*b"tdmn", vec![0xff, 0xff]).unwrap();
            },
            Declarations::Unreadable,
            Some("native parameter definitions malformed"),
        ),
    ] {
        let (decoded, warnings) = decode(edit);
        assert_eq!(decoded.len(), 1, "{warnings:?}");
        assert_eq!(decoded[0].declarations, declarations);
        assert_eq!(
            value(&decoded[0], "-0002"),
            vec![25.5],
            "explicit value kept"
        );
        match warning {
            None => assert!(warnings.is_empty(), "{warnings:?}"),
            Some(text) => assert!(warnings.iter().any(|w| w.contains(text)), "{warnings:?}"),
        }
    }
    // A malformed declaration is reported but still yields to the explicit value.
    let (decoded, _) = decode(|plugin| {
        let table = table(plugin);
        let name = table
            .iter()
            .position(|chunk| {
                chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(b"ADBE Drop Shadow-0002\0"))
            })
            .unwrap();
        table[name + 1] = Chunk::data(*b"pard", vec![0; 12]).unwrap();
    });
    let distance = decoded[0]
        .parameters
        .iter()
        .find(|parameter| parameter.match_name.ends_with("-0002"))
        .unwrap();
    assert!(distance.declared_kind.is_err());
    assert_eq!(value(&decoded[0], "-0002"), vec![25.5]);
}

#[test]
fn sparse_duplicate_effect_definitions_use_each_instances_own_controls() {
    let size = [320.0, 180.0];
    let mut first = effects::new_effect("ADBE Drop Shadow", true, size).unwrap();
    first.name = "First Shadow".into();
    set(&mut first, "-0001", &[0.1, 0.2, 0.3, 1.0]);
    set(&mut first, "-0002", &[25.5]);
    set(&mut first, "-0003", &[117.0]);
    set(&mut first, "-0004", &[40.0]);

    let mut second = effects::new_effect("ADBE Drop Shadow", true, size).unwrap();
    second.name = "Second Shadow".into();
    set(&mut second, "-0001", &[0.8, 0.7, 0.6, 1.0]);
    set(&mut second, "-0002", &[51.0]);
    set(&mut second, "-0003", &[33.0]);
    set(&mut second, "-0004", &[12.0]);

    let mut parade = effects::effect_parade(&[first, second], 7, size).unwrap();
    let second_plugin = parade
        .children_mut()
        .unwrap()
        .iter_mut()
        .filter(|chunk| chunk.list_kind() == Some(*b"sspc"))
        .nth(1)
        .unwrap();
    second_plugin
        .children_mut()
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"parT"))
        .unwrap()
        .children_mut()
        .unwrap()
        .clear();

    let content = vec![Chunk::list(
        *b"tdgp",
        vec![named("ADBE Effect Parade"), parade, named("ADBE Group End")],
    )];
    let (decoded, warnings) = read_effects(&content, size);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].index, 1);
    assert_eq!(decoded[1].index, 2);
    assert_eq!(value(&decoded[0], "-0001"), vec![0.1, 0.2, 0.3, 1.0]);
    assert_eq!(value(&decoded[0], "-0002"), vec![25.5]);
    assert_eq!(value(&decoded[0], "-0003"), vec![117.0]);
    assert_eq!(value(&decoded[0], "-0004"), vec![40.0]);
    assert_eq!(value(&decoded[1], "-0001"), vec![0.8, 0.7, 0.6, 1.0]);
    assert_eq!(value(&decoded[1], "-0002"), vec![51.0]);
    assert_eq!(value(&decoded[1], "-0003"), vec![33.0]);
    assert_eq!(value(&decoded[1], "-0004"), vec![12.0]);
}
