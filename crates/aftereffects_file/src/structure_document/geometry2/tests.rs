use super::*;
use crate::structure::{ItemKind, read_project};
use fx_schema::{Duration, Time, TimeRangeProperty};

fn close(value: f64, expected: f64) {
    assert!((value - expected).abs() < 0.001, "{value} != {expected}");
}

fn assert_effect(
    transform: &Transform,
    anchor: [f64; 2],
    position: [f64; 2],
    scale: [f64; 2],
    rotation: f64,
) {
    for (actual, expected) in transform.anchor_point.iter().zip(anchor) {
        close(*actual, expected);
    }
    let Position::TwoD(actual) = transform.position else {
        panic!("expected planar position")
    };
    for (actual, expected) in actual.iter().zip(position) {
        close(*actual, expected);
    }
    for (actual, expected) in transform.scale.iter().zip(scale) {
        close(*actual, expected);
    }
    close(transform.rotation, rotation);
}

fn empty_group() -> GroupLayer {
    super::super::group(
        LayerId::new(17),
        "Owner".into(),
        None,
        TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0)),
    )
}

#[test]
fn point_grammar_is_complete_and_never_executes_suffixes() {
    assert_eq!(
        parse_expression(" thisComp.layer('Center').toComp([0, 0, 0]); "),
        Some(PointExpression::Origin("Center"))
    );
    assert_eq!(
        parse_expression("effect(\"Transform\")(\"Position\")"),
        Some(PointExpression::PositionAlias("Transform"))
    );
    for expression in [
        "thisComp.layer('Center').toComp([1,0,0])",
        "thisComp.layer('Center').toComp([0,0,1])",
        "thisComp.layer('Center').toComp([0,0,0])+[1,0]",
        "thisComp.layer('Center').toComp([0,0,0], time-1)",
        "effect('Transform')('Anchor Point')",
        "effect('Transform')('Position');evil()",
    ] {
        assert!(parse_expression(expression).is_none(), "{expression}");
    }
}

#[test]
fn post_layer_stage_preserves_child_transform_and_admission_is_atomic() {
    let mut inner = empty_group();
    inner.transform.position = Position::TwoD([80.0, 50.0]);
    inner.transform.scale = [200.0, 100.0];
    let before = serde_json::to_value(&inner).unwrap();
    let point = Point::Curve(BTreeMap::from([(0, [10.0, 20.0]), (1000, [30.0, 40.0])]));
    let prepared = Prepared {
        anchor: point.clone(),
        position: point,
        transform: Transform {
            scale: [-100.0, 100.0],
            ..empty_group().transform
        },
    };
    let mut successful = inner.clone();
    let entries = prepared
        .apply(
            &mut successful,
            LayerId::new(99),
            &mut AnimationBudget::default(),
        )
        .unwrap();
    let LayerData::Group(child) = successful.layers[0].data() else {
        panic!("native layer retained")
    };
    assert_eq!(child.id, inner.id);
    assert_eq!(child.parent, Some(successful.id));
    assert_eq!(child.transform, inner.transform);
    assert_eq!(successful.transform.scale, [-100.0, 100.0]);
    assert_eq!(entries.len(), 4);
    let first = committed_entry_reservation_bytes(&entries[0]).unwrap();
    let mut budget = AnimationBudget::with_limit(first);
    assert!(
        prepared
            .apply(&mut inner, LayerId::new(99), &mut budget)
            .is_err()
    );
    assert_eq!(serde_json::to_value(&inner).unwrap(), before);
    assert_eq!(budget.used(), 0);
}

fn chunk(name: &[u8; 4], bytes: impl Into<Vec<u8>>) -> Chunk {
    let mut bytes = bytes.into();
    if name == b"tdmn" {
        bytes.resize(40, 0);
    }
    Chunk::data(*name, bytes).unwrap()
}

fn point(values: &[f64], expression: Option<&str>) -> Chunk {
    let mut header = vec![0; 124];
    header[..2].copy_from_slice(&[0xdb, 0x99]);
    header[3] = u8::try_from(values.len()).unwrap();
    let mut children = vec![
        chunk(b"tdb4", header),
        chunk(b"tdsb", vec![0, 0, 0, 1]),
        chunk(
            b"cdat",
            values
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
    ];
    if let Some(expression) = expression {
        children.push(chunk(b"Utf8", expression.as_bytes()));
    }
    Chunk::list(*b"tdbs", children)
}

fn synthetic_owner(
    extra_controls: Vec<Chunk>,
    position_expression: Option<&str>,
) -> (Layer, Composition) {
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    let mut composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some((**comp).clone()),
            _ => None,
        })
        .unwrap();
    let mut owner = composition.layers.remove(0);
    let mut record = owner.record.encode();
    record[131] = 4;
    record[38] = 0;
    record[39] |= 4;
    owner.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
    let mut controls = vec![
        chunk(b"tdsn", b"Utf8\0\0\0\x09Transform"),
        chunk(b"tdmn", b"ADBE Geometry2-0001"),
        point(&[10.0, 20.0], None),
        chunk(b"tdmn", b"ADBE Geometry2-0002"),
        point(&[30.0, 40.0], position_expression),
    ];
    controls.extend(extra_controls);
    owner.content = vec![Chunk::list(
        *b"tdgp",
        vec![
            chunk(b"tdmn", b"ADBE Effect Parade"),
            Chunk::list(
                *b"tdgp",
                vec![
                    chunk(b"tdmn", b"ADBE Geometry2"),
                    Chunk::list(
                        *b"sspc",
                        vec![
                            Chunk::list(*b"parT", Vec::new()),
                            Chunk::list(*b"tdgp", controls),
                        ],
                    ),
                ],
            ),
        ],
    )];
    (owner, composition)
}

#[test]
fn sparse_geometry2_defaults_and_rejected_controls_are_explicit() {
    let (owner, comp) = synthetic_owner(Vec::new(), None);
    let items = HashMap::new();
    let plan = prepare(&owner, &comp, &items, &empty_group(), true)
        .unwrap()
        .unwrap();
    assert_effect(
        &plan.transform,
        [10.0, 20.0],
        [30.0, 40.0],
        [100.0, 100.0],
        0.0,
    );
    for controls in [
        vec![chunk(b"tdmn", b"ADBE Geometry2-0005"), point(&[10.0], None)],
        vec![
            chunk(b"tdmn", b"ADBE Geometry2-0001"),
            point(&[10.0, 20.0], None),
        ],
        vec![
            chunk(b"tdmn", b"ADBE Effect Built In Params"),
            Chunk::list(
                *b"tdgp",
                vec![chunk(b"tdmn", b"ADBE Effect Opacity"), point(&[50.0], None)],
            ),
        ],
    ] {
        let (owner, comp) = synthetic_owner(controls, None);
        assert!(prepare(&owner, &comp, &items, &empty_group(), true).is_err());
    }
    let (owner, comp) = synthetic_owner(Vec::new(), Some("effect('Transform')('Position')"));
    assert!(
        prepare(&owner, &comp, &items, &empty_group(), true)
            .err()
            .unwrap()
            .contains("cyclic")
    );
}

#[test]
fn geometry2_ignores_disabled_effect_stack_and_rejects_other_owner_stages() {
    let (mut owner, comp) = synthetic_owner(Vec::new(), None);
    let items = HashMap::new();
    let mut record = owner.record.encode();
    record[39] &= !4;
    owner.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
    assert!(
        prepare(&owner, &comp, &items, &empty_group(), true)
            .unwrap()
            .is_none()
    );
    record[39] |= 4;
    for (kind, flags) in [(0, 0), (4, 4), (4, 2)] {
        record[131] = kind;
        record[38] = flags;
        owner.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
        assert!(prepare(&owner, &comp, &items, &empty_group(), true).is_err());
    }
}
