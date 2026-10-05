use std::collections::HashMap;

use super::super::{
    point_control::tests::{controller, point},
    read_transform,
    tests::{data, list, numeric},
};
use super::*;
use crate::structure::{Composition, ItemKind, ProjectItem, read_project};

const EXPRESSION: &str =
    "comp(\"Source\").layer(\"Controller\").effect(\"Center\")(\"ADBE Point Control-0001\")";

fn item(composition: Composition) -> ProjectItem {
    let mut item = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap()
    .items
    .into_iter()
    .find(|item| matches!(item.kind, ItemKind::Composition(_)))
    .unwrap();
    item.id = 17;
    item.name = "Source".into();
    item.kind = ItemKind::Composition(Box::new(composition));
    item
}

fn receiver(composition: &Composition) -> (Layer, NumericProperty) {
    let mut layer = composition.layers[0].clone();
    layer.record = layer.record.clone().with_three_d_layer(true).unwrap();
    layer.name = "Destination".into();
    layer.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Position"),
                    numeric(&[10000.0, -10000.0, -55.0], Some(EXPRESSION)),
                ],
            ),
        ],
    )];
    let base = read_transform(&layer.content)
        .unwrap()
        .0
        .remove(0)
        .numeric
        .unwrap();
    (layer, base)
}

#[test]
fn point_position_policy_uses_producer_plane_and_independent_zero_z() {
    let source = controller(Some(point(&[0.25, 0.75], None)));
    let (receiver, base) = receiver(&source);
    let source = item(source);
    let mut destination = controller(None);
    destination.width = 100;
    destination.height = 200;
    let items = HashMap::from([(source.id, &source)]);
    let context = cross_comp::Context {
        composition_id: 18,
        composition: &destination,
        items: &items,
    };
    let original = source.clone();
    let lowered = lower(&receiver, context, &base).unwrap().unwrap();
    assert_eq!(lowered.values, [815.0, 399.75, 0.0]);
    assert!(!lowered.expression_enabled && !lowered.expression_present);
    assert!(!lowered.animated && lowered.keyframes.is_empty());
    assert_eq!(base.values, [10000.0, -10000.0, -55.0]);
    assert_eq!(source, original);
    // Equal numeric layer IDs in DIFFERENT compositions are not a self-link.
    let ItemKind::Composition(composition) = &source.kind else {
        panic!()
    };
    assert_eq!(receiver.record.id(), composition.layers[0].record.id());
}

#[test]
fn point_position_policy_keeps_occurrence_overrides_and_explicit_precedence() {
    let definition = controller(Some(point(&[0.25, 0.75], None)));
    let (mut receiver, base) = receiver(&definition);
    let mut record = receiver.record.encode();
    record[..4].copy_from_slice(&368_u32.to_be_bytes());
    receiver.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
    let source = item(definition);
    let items = HashMap::from([(source.id, &source)]);
    let occurrence = controller(Some(point(&[0.0, 0.0], None)));
    let context = cross_comp::Context {
        composition_id: source.id,
        composition: &occurrence,
        items: &items,
    };
    assert_eq!(
        lower(&receiver, context, &base).unwrap().unwrap().values,
        [0.0; 3]
    );
    let defaults = controller(None);
    assert_eq!(
        lower(
            &receiver,
            cross_comp::Context {
                composition: &defaults,
                ..context
            },
            &base
        )
        .unwrap()
        .unwrap()
        .values,
        [1630.0, 266.5, 0.0]
    );
    assert_eq!(
        lower(
            &receiver,
            cross_comp::Context {
                composition_id: 18,
                ..context
            },
            &base
        )
        .unwrap()
        .unwrap()
        .values,
        [815.0, 399.75, 0.0]
    );
}

#[test]
fn point_position_policy_declines_keyed_separated_non3d_and_invalid_sources() {
    let valid = controller(Some(point(&[0.25, 0.75], None)));
    let (receiver, base) = receiver(&valid);
    let source = item(valid);
    let items = HashMap::from([(source.id, &source)]);
    let destination = controller(None);
    let context = cross_comp::Context {
        composition_id: 18,
        composition: &destination,
        items: &items,
    };
    let mut invalid = base.clone();
    invalid.animated = true;
    assert!(lower(&receiver, context, &invalid).unwrap().is_err());
    invalid = base.clone();
    invalid.dimensions_separated = true;
    assert!(lower(&receiver, context, &invalid).unwrap().is_err());
    invalid = base.clone();
    invalid.values = vec![1.0, 2.0];
    assert!(lower(&receiver, context, &invalid).unwrap().is_err());
    let mut non3d = receiver.clone();
    non3d.record = non3d.record.with_three_d_layer(false).unwrap();
    assert!(lower(&non3d, context, &base).unwrap().is_err());
    for explicit in [
        point(&[0.25, 0.75], Some("value + [1,2]")),
        point(&[f64::NAN, 0.75], None),
        point(&[0.25], None),
        numeric(&[0.25, 0.75], None),
    ] {
        let source = item(controller(Some(explicit)));
        let items = HashMap::from([(source.id, &source)]);
        assert!(
            lower(
                &receiver,
                cross_comp::Context {
                    items: &items,
                    ..context
                },
                &base
            )
            .unwrap()
            .is_err(),
            "invalid explicit data must not use valid defaults"
        );
    }
    let duplicate = source.clone();
    let items = HashMap::from([(17, &source), (99, &duplicate)]);
    assert!(
        lower(
            &receiver,
            cross_comp::Context {
                items: &items,
                ..context
            },
            &base
        )
        .unwrap()
        .is_err()
    );
    let mut duplicate_layers = controller(None);
    duplicate_layers
        .layers
        .push(duplicate_layers.layers[0].clone());
    let source = item(duplicate_layers);
    let items = HashMap::from([(17, &source)]);
    assert!(
        lower(
            &receiver,
            cross_comp::Context {
                items: &items,
                ..context
            },
            &base
        )
        .unwrap()
        .is_err()
    );
}

#[test]
fn point_position_policy_sparse_explicit_never_infers_defaults_or_relaxes_legacy_reader() {
    fn omit_declaration(composition: &mut Composition) {
        composition.layers[0].content[0].children_mut().unwrap()[1]
            .children_mut()
            .unwrap()[1]
            .children_mut()
            .unwrap()[0]
            .children_mut()
            .unwrap()
            .clear();
    }
    let mut sparse = controller(Some(point(&[0.25, 0.75], None)));
    omit_declaration(&mut sparse);
    let (receiver, base) = receiver(&sparse);
    assert!(super::super::point_control::selected(&sparse, &sparse.layers[0], "Center").is_err());
    let source = item(sparse);
    let items = HashMap::from([(17, &source)]);
    let destination = controller(None);
    let context = cross_comp::Context {
        composition_id: 18,
        composition: &destination,
        items: &items,
    };
    assert_eq!(
        lower(&receiver, context, &base).unwrap().unwrap().values,
        [815.0, 399.75, 0.0]
    );
    for explicit in [
        None,
        Some(numeric(&[0.25, 0.75], None)),
        Some(point(&[0.25, 0.75], Some("value"))),
    ] {
        let mut sparse = controller(explicit);
        omit_declaration(&mut sparse);
        let source = item(sparse);
        let items = HashMap::from([(17, &source)]);
        assert!(
            lower(
                &receiver,
                cross_comp::Context {
                    items: &items,
                    ..context
                },
                &base
            )
            .unwrap()
            .is_err()
        );
    }
    let mut malformed = controller(Some(point(&[0.25, 0.75], None)));
    malformed.layers[0].content[0].children_mut().unwrap()[1]
        .children_mut()
        .unwrap()[1]
        .children_mut()
        .unwrap()[0]
        .children_mut()
        .unwrap()[1] = data(b"pard", vec![0; 148]);
    let source = item(malformed);
    let items = HashMap::from([(17, &source)]);
    assert!(
        lower(
            &receiver,
            cross_comp::Context {
                items: &items,
                ..context
            },
            &base
        )
        .unwrap()
        .is_err()
    );
}

#[test]
fn point_position_policy_grammar_accepts_only_complete_native_point_alias() {
    assert!(parse(EXPRESSION).is_some());
    assert!(parse(&format!("  {EXPRESSION}; ")).is_some());
    for text in [
        format!("{EXPRESSION} + value"),
        format!("{EXPRESSION}[0]"),
        format!("{EXPRESSION}.valueAtTime(0)"),
        format!("{EXPRESSION}; value"),
        EXPRESSION.replace("ADBE Point Control-0001", "ADBE Point3D Control-0001"),
        EXPRESSION.replace("comp(\"Source\")", "thisComp"),
    ] {
        assert!(parse(&text).is_none(), "{text}");
    }
}
