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

#[test]
fn sparse_invert_catalog_preserves_explicit_controls_and_rejects_bad_records() {
    let size = [1920.0, 1080.0];
    let mut effect = effects::new_effect("ADBE Invert", true, size).unwrap();
    set(&mut effect, "-0001", &[2.0]);
    set(&mut effect, "-0002", &[25.0]);
    let mut parade = effects::effect_parade(&[effect], 7, size).unwrap();
    let plugin = parade
        .children_mut()
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"sspc"))
        .unwrap();
    let children = plugin.children_mut().unwrap();
    children
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"parT"))
        .unwrap()
        .children_mut()
        .unwrap()
        .clear();
    let content = |parade| {
        vec![Chunk::list(
            *b"tdgp",
            vec![named("ADBE Effect Parade"), parade, named("ADBE Group End")],
        )]
    };
    let (decoded, _) = read_effects(&content(parade.clone()), size);
    assert_eq!(value(&decoded[0], "-0001"), vec![2.0]);
    assert_eq!(value(&decoded[0], "-0002"), vec![25.0]);
    assert!(crate::effects::special::validate_import(&decoded[0]).is_err());

    for duplicate in [false, true] {
        let mut broken = parade.clone();
        let plugin = broken
            .children_mut()
            .unwrap()
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"sspc"))
            .unwrap();
        let body = plugin
            .children_mut()
            .unwrap()
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let index = body
            .iter()
            .position(|chunk| chunk == &named("ADBE Invert-0001"))
            .unwrap();
        if duplicate {
            let repeated = body[index..index + 2].to_vec();
            body.splice(index..index, repeated);
        } else {
            body[index + 1] = Chunk::list(*b"tdbs", vec![]);
        }
        let (decoded, _) = read_effects(&content(broken), size);
        let channel = decoded[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Invert-0001")
            .unwrap();
        assert!(
            channel.numeric.is_err(),
            "malformed/duplicate control must not use RGB default"
        );
        assert!(crate::effects::special::validate_import(&decoded[0]).is_err());
    }

    let plugin = parade
        .children_mut()
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"sspc"))
        .unwrap();
    let body = plugin
        .children_mut()
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    for name in ["ADBE Invert-0002", "ADBE Invert-0001"] {
        let index = body.iter().position(|chunk| chunk == &named(name)).unwrap();
        body.drain(index..index + 2);
    }
    let (decoded, _) = read_effects(&content(parade), size);
    assert_eq!(value(&decoded[0], "-0001"), vec![1.0]);
    assert_eq!(value(&decoded[0], "-0002"), vec![0.0]);
    assert!(crate::effects::special::validate_import(&decoded[0]).is_ok());
    for unreadable in [false, true] {
        let mut invalid = decoded[0].clone();
        if unreadable {
            invalid.declarations = super::Declarations::Unreadable;
        } else {
            invalid
                .parameters
                .iter_mut()
                .find(|parameter| parameter.match_name == "ADBE Invert-0001")
                .unwrap()
                .declared_kind = Ok(Some(2));
        }
        assert!(crate::effects::special::validate_import(&invalid).is_err());
    }
}

#[test]
fn integer_slider_default_is_signed_while_checkbox_and_popup_are_unsigned() {
    let pard = |kind: u32, default: [u8; 4]| {
        let mut bytes = vec![0; 148];
        bytes[12..16].copy_from_slice(&kind.to_be_bytes());
        bytes[56..60].copy_from_slice(&default);
        vec![Chunk::data(*b"pard", bytes).unwrap()]
    };
    let slider = super::default_numeric(&pard(1, (-5_i32).to_be_bytes()), [1.0, 1.0]).unwrap();
    assert_eq!(slider.values, [-5.0]);
    for kind in [4, 7] {
        let unsigned = super::default_numeric(&pard(kind, [0, 0, 0, 3]), [1.0, 1.0]).unwrap();
        assert_eq!(unsigned.values, [3.0]);
    }
}
