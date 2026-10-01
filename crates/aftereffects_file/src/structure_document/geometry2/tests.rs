use super::*;
use crate::structure::{ItemKind, read_project};
use fx_schema::{Duration, Time, TimeRangeProperty};
use sha2::{Digest, Sha256};

fn groups<'a>(layers: &'a [fx_schema::Layer], name: &str, output: &mut Vec<&'a GroupLayer>) {
    for layer in layers {
        if let LayerData::Group(group) = layer.data() {
            if group.name == name {
                output.push(group);
            }
            groups(&group.layers, name, output);
        }
    }
}

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

#[test]
#[ignore = "requires licensed local AEP_GEOMETRY2_SOURCE at the pinned SHA-256"]
fn local_external_source_restores_three_fold_geometry2_stages() {
    let bytes =
        std::fs::read(std::env::var_os("AEP_GEOMETRY2_SOURCE").expect("licensed source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = read_project(&bytes).expect("pinned source parses");
    let item = project.item(724).expect("fold composition");
    let ItemKind::Composition(native) = &item.kind else {
        panic!("composition")
    };
    let converted =
        super::super::to_structural_fx_document(&project, Some(724)).expect("isolated fold import");
    let mut wrappers = Vec::new();
    for (name, anchor, position, scale, rotation) in [
        (
            "Dark-Bottom",
            [1919.9, 800.5],
            [1920.0, 800.5],
            [100.0, 100.0],
            180.0,
        ),
        // -0003 is Scale Height; -0004 is Width (not a vector Scale).
        (
            "Light-Bottom",
            [1920.5, 801.0],
            [1920.0, 801.0],
            [100.0, -100.0],
            0.0,
        ),
    ] {
        wrappers.clear();
        groups(
            converted.document.composition().layers(),
            name,
            &mut wrappers,
        );
        let stage = wrappers
            .iter()
            .find(|group| group.description.contains("post-layer Geometry2"))
            .unwrap_or_else(|| {
                panic!(
                    "{name}: editable stage missing; {:?}",
                    converted.diagnostics
                )
            });
        assert_effect(&stage.transform, anchor, position, scale, rotation);
        let LayerData::Group(child) = stage.layers[0].data() else {
            panic!("native occurrence child")
        };
        assert!(child.description.contains("AEP comp=724 layer="));
        assert_eq!(child.parent, Some(stage.id));
        assert_eq!(child.transform.position, Position::TwoD([1920.0, 800.0]));
    }
    wrappers.clear();
    groups(
        converted.document.composition().layers(),
        "Light-Top",
        &mut wrappers,
    );
    let stage = wrappers
        .iter()
        .find(|group| group.description.contains("post-layer Geometry2"))
        .expect("Center-driven stage exists");
    close(stage.transform.scale[0], -100.0);
    close(stage.transform.scale[1], 100.1);
    let LayerData::Group(child) = stage.layers[0].data() else {
        panic!("copied native occurrence")
    };
    assert_eq!(child.transform.position, Position::TwoD([1920.0, 800.0]));
    let point_entries: Vec<_> = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|property| property.layer_id() == stage.id)
        })
        .collect();
    assert_eq!(point_entries.len(), 4);
    assert!(
        point_entries
            .iter()
            .all(|entry| !entry.animator.is_js_script())
    );
    assert!(!converted.diagnostics.iter().any(|diagnostic| {
        format!("{diagnostic:?}")
            .contains("Effect ADBE Geometry2: no current native FX counterpart")
    }));

    let owner = native
        .layers
        .iter()
        .find(|layer| layer.record.id() == 743)
        .unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let plan = prepare(owner, native, &items, &empty_group(), true)
        .unwrap()
        .unwrap();
    let Point::Curve(points) = &plan.position else {
        panic!("editable fitted point")
    };
    assert!(
        points
            .values()
            .all(|point| (point[0] - 1920.0).abs() < 1e-8)
    );
    // Before Center's first local1.125s key, its [60,60] Position cancels
    // the ancestors' explicit [60,60] pivots. At Scale/Offset's first key
    // (local1/3 + start5/6 = composition7/6s), its native Y is800.999997.
    // Center later animates away from60, so the cancellation is not global.
    let before = points.range(..=1167).next_back().unwrap();
    let after = points.range(1167..).next().unwrap();
    let value = if before.0 == after.0 {
        before.1[1]
    } else {
        let fraction = (1167 - before.0) as f64 / (after.0 - before.0) as f64;
        before.1[1] + (after.1[1] - before.1[1]) * fraction
    };
    assert!((value - 800.999997060301).abs() <= 0.011, "{value}");
    assert!(prepare(owner, native, &items, &empty_group(), false).is_err());
}
