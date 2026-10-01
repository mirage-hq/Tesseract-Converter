use super::*;
use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
};
use sha2::{Digest, Sha256};

const HORIZONTAL: &str = r#"
    a = thisComp.layer("Left").toComp([0,0,0]);
    b = thisComp.layer("Right").toComp([0,0,0]);
    s = thisComp.layer("Controls").effect("Horiz")("Slider");
    value + linear(s, 0, 100, a, b)
"#;

#[test]
fn exact_stock_grammar_accepts_names_but_not_executable_or_algebraic_variants() {
    let parsed = parse(HORIZONTAL).expect("stock Position blend parses");
    assert_eq!(parsed.endpoints, ["Left", "Right"]);
    assert_eq!(parsed.controller, "Controls");
    assert_eq!(parsed.effect, "Horiz");

    for rejected in [
        HORIZONTAL.replace("value +", "wiggle(2) + value +"),
        HORIZONTAL.replace("linear(s, 0, 100, a, b)", "linear(s, 0, 1, a, b)"),
        HORIZONTAL.replace("toComp([0,0,0])", "toComp(anchorPoint)"),
        HORIZONTAL.replace("effect(\"Horiz\")", "effect(\"Horiz\") + effect(\"Other\")"),
        HORIZONTAL.replace("\"Left\"", "\"Le\\ft\""),
    ] {
        assert!(parse(&rejected).is_none(), "accepted {rejected:?}");
    }
}

#[test]
fn evaluates_base_plus_clamped_linear_endpoints_and_specific_slider_alias() {
    let (owner, composition) = fixture(150.0, None);
    let blend = resolve(&owner, &composition)
        .expect("profile recognized")
        .expect("profile resolves");
    assert_eq!(blend.dependencies(), [2, 3]);
    assert_eq!(
        blend.evaluate(0.0, [[0.0, 100.0], [100.0, 200.0]]).unwrap(),
        [110.0, 220.0]
    );

    let (owner, composition) = fixture(75.0, Some(("Vert-Offset", 25.0)));
    let blend = resolve(&owner, &composition)
        .expect("alias profile recognized")
        .expect("alias profile resolves");
    assert_eq!(
        blend.evaluate(0.0, [[0.0, 100.0], [100.0, 200.0]]).unwrap(),
        [60.0, 170.0]
    );

    let (owner, composition) = fixture(-10.0, None);
    let blend = resolve(&owner, &composition).unwrap().unwrap();
    assert_eq!(
        blend.evaluate(0.0, [[0.0, 100.0], [100.0, 200.0]]).unwrap(),
        [10.0, 120.0]
    );
}

#[test]
fn recognized_profile_rejects_parented_owners_ambiguous_references_and_slider_js() {
    let (mut owner, composition) = fixture(50.0, None);
    owner.record = record_with_identity(&owner.record, 1, 2);
    assert!(
        resolve(&owner, &composition)
            .unwrap()
            .err()
            .unwrap()
            .contains("unparented")
    );

    let (owner, mut composition) = fixture(50.0, None);
    composition.layers.push(composition.layers[0].clone());
    assert!(
        resolve(&owner, &composition)
            .unwrap()
            .err()
            .unwrap()
            .contains("ambiguous")
    );

    let (owner, mut composition) = fixture(50.0, None);
    let controller = composition
        .layers
        .iter_mut()
        .find(|layer| layer.name.as_ref() == "Controls")
        .unwrap();
    controller.content = controller_content(
        50.0,
        Some("value-effect(\"Offset\")(\"Slider\");time"),
        Some(("Offset", 10.0)),
    );
    assert!(
        resolve(&owner, &composition)
            .unwrap()
            .err()
            .unwrap()
            .contains("exact same-layer")
    );
}

#[test]
#[ignore = "requires licensed local Intro source at the pinned SHA-256"]
fn local_external_source_resolves_fold_position_blends_and_transitive_rig() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../tmp/ordinary-intro/native-input/source.aep");
    let bytes = std::fs::read(&source).expect("set up the licensed Intro source locally");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = read_project(&bytes).expect("pinned Intro source parses");
    let ItemKind::Composition(composition) = &project.item(724).unwrap().kind else {
        panic!("item 724 must be a composition")
    };
    let source_items = project.items.iter().map(|item| (item.id, item)).collect();
    // Historical supplementary CPU snapshot with zeroed null anchors, not an
    // Adobe oracle. Below, subtract the independently derived displacement of
    // the two explicit 60px parent anchors; sparse null anchors remain zero.
    let expected = [
        (
            737,
            [
                (
                    738,
                    "CenterShape-Bottom",
                    [1945.9408271119844, 832.7867052413611],
                ),
                (736, "Diamond-Edge", [1989.0625140711986, 815.8701546333343]),
            ],
        ),
        (
            732,
            [
                (734, "Diamond-Top", [1945.884, 798.9575889419528]),
                (736, "Diamond-Edge", [1989.0625140711986, 815.8701546333343]),
            ],
        ),
        (
            731,
            [
                (733, "Center", [1945.884, 815.8701546333343]),
                (734, "Diamond-Top", [1945.884, 798.9575889419528]),
            ],
        ),
    ];
    let mut rig = super::super::TransformRig::new(composition, &source_items);
    for (owner_id, endpoints) in expected {
        let owner = layer_by_id(composition, owner_id);
        let blend = resolve(owner, composition)
            .expect("native Position profile recognized")
            .expect("native Position profile resolves");
        let expected_ids = endpoints.map(|(id, name, _)| {
            assert_eq!(unique_named_layer(composition, name).record.id(), id);
            id
        });
        assert_eq!(blend.dependencies(), expected_ids);
        rig.add(owner_id)
            .expect("transitive transform rig resolves");
    }

    let transitive_blends: Vec<_> = rig
        .nodes
        .values()
        .filter(|node| resolve(node.layer, composition).is_some())
        .map(|node| (node.layer.record.id(), node.layer.name.as_ref()))
        .collect();
    assert_eq!(
        transitive_blends,
        [
            (731, "Middle-Edge"),
            (732, "TopRight"),
            (737, "CenterShape-Right")
        ]
    );

    assert_eq!(layer_by_id(composition, 730).record.parent_id(), 729);
    assert!(resolve(layer_by_id(composition, 730), composition).is_none());
    for endpoint_id in [733, 734, 736, 738] {
        assert_eq!(
            layer_by_id(composition, endpoint_id).record.parent_id(),
            730
        );
    }

    // The native expressions call toComp([0,0,0]): pin the endpoint layer
    // origins after the real Shape Ctrls -> Scale/Offset parent chain, rather
    // than inventing additional "Origin" Position-blend nodes.
    let native_start = 5.0 / 6.0;
    // Native source 808 is 120x120 and both parent anchors store [.5,.5].
    // At this boundary Scale/Offset is 20%, and Shape Ctrls begins at native
    // scale [1.157, .2391797977527753]. Each parent's anchor contributes:
    // outer_scale * (outer_anchor + inner_scale * inner_anchor).
    let anchor_correction = [
        0.2 * (60.0 + 1.157 * 60.0),
        0.2 * (60.0 + 0.2391797977527753 * 60.0),
    ];
    for (_, endpoints) in expected {
        for (endpoint_id, _, expected_origin) in endpoints {
            let actual = rig
                .matrix(endpoint_id, native_start)
                .unwrap()
                .apply([0.0, 0.0]);
            assert_point_close(
                actual,
                [
                    expected_origin[0] - anchor_correction[0],
                    expected_origin[1] - anchor_correction[1],
                ],
            );
        }
    }

    let critical_time = 44.0 / 24.0;
    for owner_id in [737, 732, 731] {
        let owner = layer_by_id(composition, owner_id);
        let blend = resolve(owner, composition).unwrap().unwrap();
        let endpoints = blend.dependencies().map(|dependency| {
            rig.matrix(dependency, critical_time)
                .unwrap()
                .apply([0.0, 0.0])
        });
        let evaluated = blend.evaluate(critical_time, endpoints).unwrap();
        assert!(evaluated.into_iter().all(f64::is_finite));
        assert!(
            rig.matrix(owner_id, critical_time)
                .unwrap()
                .apply([0.0, 0.0])
                .into_iter()
                .all(f64::is_finite)
        );
    }

    let source = layer_by_id(composition, 729);
    let (properties, warnings) =
        crate::structure_document::control_links::read_layer_transform(source, composition)
            .unwrap();
    let scale = properties
        .iter()
        .find(|property| property.match_name == "ADBE Scale")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert!(!scale.expression_enabled, "{warnings:?}");
    assert!(!scale.expression_present, "{warnings:?}");
    assert!((scale.keyframes[0].values[0] - 0.2).abs() < 0.0001);
    assert!(
        scale
            .keyframes
            .iter()
            .all(|key| key.values.len() == 2 && key.values[0] == key.values[1])
    );
}

fn fixture(slider: f64, alias: Option<(&str, f64)>) -> (Layer, Composition) {
    let project = read_project(include_bytes!(
        "../../../../../tests/fixtures/properties/property_2D_position.aep"
    ))
    .unwrap();
    let mut composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some((**composition).clone()),
            _ => None,
        })
        .unwrap();
    let template = composition.layers[0].clone();
    let owner = layer(
        &template,
        1,
        "Owner",
        transform_content(vec![(
            "ADBE Position",
            numeric(&[10.0, 20.0, 0.0], Some(HORIZONTAL)),
        )]),
    );
    let left = layer(&template, 2, "Left", Vec::new());
    let right = layer(&template, 3, "Right", Vec::new());
    let controller = layer(
        &template,
        4,
        "Controls",
        controller_content(
            slider,
            alias.map(|_| "value-effect(\"Vert-Offset\")(\"Slider\")"),
            alias,
        ),
    );
    composition.layers = vec![left, right, controller];
    (owner, composition)
}

fn layer(template: &Layer, id: u32, name: &str, content: Vec<Chunk>) -> Layer {
    let mut layer = template.clone();
    layer.name = name.into();
    layer.record = record_with_identity(&layer.record, id, 0);
    layer.content = content;
    layer
}

fn record_with_identity(record: &LayerRecord, id: u32, parent_id: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[..4].copy_from_slice(&id.to_be_bytes());
    bytes[132..136].copy_from_slice(&parent_id.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn transform_content(leaves: Vec<(&str, Chunk)>) -> Vec<Chunk> {
    let leaves = leaves
        .into_iter()
        .flat_map(|(name, body)| [data(b"tdmn", name.as_bytes()), body])
        .collect();
    vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(b"tdgp", leaves),
        ],
    )]
}

fn controller_content(
    slider: f64,
    expression: Option<&str>,
    alias: Option<(&str, f64)>,
) -> Vec<Chunk> {
    let mut effects = slider_effect("Horiz", slider, expression);
    if let Some((name, value)) = alias {
        effects.extend(slider_effect(name, value, None));
    }
    vec![list(
        b"tdgp",
        vec![data(b"tdmn", b"ADBE Effect Parade"), list(b"tdgp", effects)],
    )]
}

fn slider_effect(label: &str, value: f64, expression: Option<&str>) -> Vec<Chunk> {
    vec![
        data(b"tdmn", b"ADBE Slider Control"),
        list(
            b"sspc",
            vec![list(
                b"tdgp",
                vec![
                    name(label),
                    data(b"tdmn", b"ADBE Slider Control-0001"),
                    numeric(&[value], expression),
                ],
            )],
        ),
    ]
}

fn numeric(values: &[f64], expression: Option<&str>) -> Chunk {
    let mut metadata = vec![0; 124];
    metadata[..2].copy_from_slice(&[0xdb, 0x99]);
    metadata[3] = u8::try_from(values.len()).unwrap();
    let mut children = vec![
        data(b"tdb4", metadata),
        data(b"tdsb", vec![0, 0, 0, 1]),
        data(
            b"cdat",
            values
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
        name("Slider"),
    ];
    if let Some(expression) = expression {
        children.push(data(b"Utf8", expression.as_bytes()));
    }
    list(b"tdbs", children)
}

fn name(value: &str) -> Chunk {
    let mut bytes = b"Utf8".to_vec();
    bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    bytes.extend(value.as_bytes());
    data(b"tdsn", bytes)
}

fn data(id: &[u8; 4], value: impl Into<Vec<u8>>) -> Chunk {
    let mut value = value.into();
    if id == b"tdmn" {
        value.resize(40, 0);
    }
    Chunk::data(*id, value).unwrap()
}

fn list(kind: &[u8; 4], children: Vec<Chunk>) -> Chunk {
    Chunk::list(*kind, children)
}

fn layer_by_id(composition: &Composition, id: u32) -> &Layer {
    composition
        .layers
        .iter()
        .find(|layer| layer.record.id() == id)
        .unwrap_or_else(|| panic!("layer {id} must exist"))
}

fn assert_point_close(actual: [f64; 2], expected: [f64; 2]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }
}

fn unique_named_layer<'a>(composition: &'a Composition, name: &str) -> &'a Layer {
    let mut matches = composition
        .layers
        .iter()
        .filter(|layer| layer.name.as_ref() == name);
    let layer = matches
        .next()
        .unwrap_or_else(|| panic!("layer {name:?} must exist"));
    assert!(matches.next().is_none(), "layer {name:?} must be unique");
    layer
}
