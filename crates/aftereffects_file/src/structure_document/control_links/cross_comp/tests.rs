use std::collections::HashMap;

use super::super::tests::{data, list, numeric};
use super::*;

#[test]
fn native_hold_endpoint_flags_are_admitted_without_weakening_curve_guards() {
    let curve = super::super::tests::native_hold_endpoint_curve("control-0");
    validate_curve(&curve, 1).unwrap();
    let mut invalid = curve.clone();
    invalid.keyframes[0].out_interpolation = 4;
    assert!(validate_curve(&invalid, 1).is_err());
    invalid = curve;
    invalid.keyframes[0].out_interpolation = 1;
    invalid.keyframes[1].in_interpolation = 3;
    assert!(validate_curve(&invalid, 1).is_err());
}
use crate::{
    properties::{NumericKeyframe, NumericValueKind},
    schema::layer_records::LayerRecord,
    structure::{ItemKind, ProjectItem, SolidSource, read_project},
};

#[test]
fn static_cross_comp_slider_scale_replaces_the_cached_transform() {
    let (template, mut source_comp) = template();
    let mut controls = template.clone();
    controls.name = "Controls".into();
    controls.record = record_with_id(&controls.record, 2);
    controls.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Effect Parade"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Slider Control"),
                    list(
                        b"sspc",
                        vec![list(
                            b"tdgp",
                            vec![
                                super::super::tests::name("Text Scale"),
                                data(b"tdmn", b"ADBE Slider Control-0001"),
                                numeric(&[129.8462], None),
                            ],
                        )],
                    ),
                ],
            ),
        ],
    )];
    source_comp.layers = vec![controls];
    let mut owner_comp = source_comp.clone();
    owner_comp.layers = vec![layer(
        &template,
        1,
        "Owner",
        vec![(
            "ADBE Scale",
            numeric(
                &[1.0, 1.0],
                Some(
                    "temp = comp(\"Render\").layer(\"Controls\").effect(\"Text Scale\")(\"ADBE Slider Control-0001\"); [temp, temp]",
                ),
            ),
        )],
    )];
    let source_item = item(3, "Render", source_comp);
    let owner_item = item(4, "Child", owner_comp.clone());
    let items = HashMap::from([(3, &source_item), (4, &owner_item)]);
    let (properties, warnings) = super::super::read_layer_transform_with_sources(
        &owner_comp.layers[0],
        4,
        &owner_comp,
        &items,
    )
    .unwrap();
    let scale = property(&properties, "ADBE Scale");
    assert_eq!(scale.values, vec![129.8462 * 0.01; 2]);
    assert!(!scale.expression_enabled);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("static cross-composition Slider"))
    );
    let duplicate = item(
        5,
        "Render",
        match &source_item.kind {
            ItemKind::Composition(composition) => (**composition).clone(),
            _ => unreachable!(),
        },
    );
    let ambiguous = HashMap::from([(3, &source_item), (4, &owner_item), (5, &duplicate)]);
    let (properties, _) = super::super::read_layer_transform_with_sources(
        &owner_comp.layers[0],
        4,
        &owner_comp,
        &ambiguous,
    )
    .unwrap();
    assert!(property(&properties, "ADBE Scale").expression_enabled);
}

fn template() -> (Layer, Composition) {
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/property_1D_opacity.aep"
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
    let layer = composition.layers[0].clone();
    composition.layers.clear();
    (layer, composition)
}

fn record_with_id(record: &LayerRecord, id: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[..4].copy_from_slice(&id.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn record_with_stretch(record: &LayerRecord, numerator: i32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[8..12].copy_from_slice(&numerator.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn record_with_source(record: &LayerRecord, source_id: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[40..44].copy_from_slice(&source_id.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn transform_content(properties: Vec<(&str, crate::rifx::Chunk)>) -> Vec<crate::rifx::Chunk> {
    let leaves = properties
        .into_iter()
        .flat_map(|(property, numeric)| [data(b"tdmn", property.as_bytes()), numeric])
        .collect();
    vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(b"tdgp", leaves),
        ],
    )]
}

fn layer(
    template: &Layer,
    id: u32,
    layer_name: &str,
    properties: Vec<(&str, crate::rifx::Chunk)>,
) -> Layer {
    let mut layer = template.clone();
    layer.name = layer_name.into();
    layer.record = record_with_id(&layer.record, id);
    layer.content = transform_content(properties);
    layer
}

fn item(id: u32, item_name: &str, composition: Composition) -> ProjectItem {
    ProjectItem {
        id,
        name: item_name.into(),
        parent_folder: None,
        kind: ItemKind::Composition(Box::new(composition)),
        footage: None,
        solid: None,
        media: None,
        native_media: None,
    }
}

fn solid_item(id: u32, dimensions: [u16; 2]) -> ProjectItem {
    ProjectItem {
        id,
        name: format!("Solid {id}"),
        parent_folder: None,
        kind: ItemKind::Footage,
        footage: None,
        solid: Some(Ok(SolidSource {
            width: dimensions[0],
            height: dimensions[1],
            pixel_aspect: (1, 1),
            color: [0.0; 3],
        })),
        media: None,
        native_media: None,
    }
}

fn property<'a>(
    properties: &'a [crate::properties::TransformProperty],
    name: &str,
) -> &'a NumericProperty {
    properties
        .iter()
        .find(|property| property.match_name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .numeric
        .as_ref()
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
fn grammar_accepts_only_exact_direct_cross_composition_transform_members() {
    assert_eq!(
        parse(" comp ( 'SH01' ).layer ( \"Scaler\" ).transform.scale; ").unwrap(),
        Reference {
            composition: "SH01",
            layer: "Scaler",
            member: Member::Scale,
        }
    );
    assert_eq!(
        parse("comp(\"SH01\").layer(\"Box_04\").transform.rotation").unwrap(),
        Reference {
            composition: "SH01",
            layer: "Box_04",
            member: Member::Rotation,
        }
    );
    for expression in [
        "thisComp.layer(\"Scaler\").transform.scale",
        "comp(\"\").layer(\"Scaler\").transform.scale",
        "comp(\"SH01\").layer(\"\").transform.scale",
        "comp(\"SH01\").layer(name).transform.scale",
        "comp(\"SH01\").layer(\"Scaler\").transform.scale + [1, 1]",
        "comp(\"SH01\").layer(\"Scaler\").transform.scale.valueAtTime(time)",
        "comp(\"SH01\").layer(\"Scaler\").content(\"Group 1\").transform.scale",
    ] {
        assert!(parse(expression).is_none(), "{expression}");
    }

    let (template, _) = template();
    let mismatch = layer(
        &template,
        1,
        "Mismatch",
        vec![(
            "ADBE Rotate Z",
            numeric(
                &[0.0],
                Some("comp(\"SH01\").layer(\"Scaler\").transform.scale"),
            ),
        )],
    );
    assert!(
        reference(&mismatch, Member::Rotation)
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("changes property member")
    );

    let identity = layer(
        &template,
        2,
        "Identity",
        vec![("ADBE Scale", numeric(&[1.0, 1.0], Some("transform.scale")))],
    );
    let (properties, _) = super::super::read_transform(&identity.content).unwrap();
    let base = property(&properties, "ADBE Scale");
    let lowered = same_member_identity(&identity, Member::Scale, base)
        .unwrap()
        .unwrap();
    assert!(!lowered.expression_enabled);
    assert_eq!(lowered.values, vec![1.0, 1.0]);
}

#[test]
fn positive_stretch_rebases_time_and_speed_but_reversed_clocks_are_rejected() {
    let (template, _) = template();
    let mut source = layer(&template, 2, "Source", Vec::new());
    source.record = record_with_stretch(&source.record, 2);
    let mut owner = layer(&template, 1, "Owner", Vec::new());
    owner.record = record_with_stretch(&owner.record, 1);
    let curve = NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![NumericKeyframe {
            time_secs: 1.0,
            values: vec![5.0],
            in_interpolation: 2,
            out_interpolation: 2,
            in_speed: vec![4.0],
            in_influence: vec![50.0],
            out_speed: vec![6.0],
            out_influence: vec![50.0],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        }],
        value_kind: NumericValueKind::Continuous,
    };
    let lowered = rebase(curve.clone(), &source, &owner, 1).unwrap();
    let scale = source.record.stretch().unwrap() / owner.record.stretch().unwrap();
    let offset = (source.record.start_time().unwrap() - owner.record.start_time().unwrap())
        / owner.record.stretch().unwrap();
    assert!((lowered.keyframes[0].time_secs - (offset + scale)).abs() < 1e-12);
    assert_eq!(lowered.keyframes[0].in_speed, vec![2.0]);
    assert_eq!(lowered.keyframes[0].out_speed, vec![3.0]);

    source.record = record_with_stretch(&source.record, -1);
    let error = rebase(curve.clone(), &source, &owner, 1).unwrap_err();
    assert!(error.to_string().contains("finite positive layer clocks"));
    owner.record = record_with_stretch(&owner.record, -1);
    source.record = record_with_stretch(&source.record, 1);
    let error = rebase(curve, &source, &owner, 1).unwrap_err();
    assert!(error.to_string().contains("finite positive layer clocks"));
}

#[test]
fn keyed_direct_alias_destination_accepts_native_layout_but_not_unrelated_layouts() {
    let destination = NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: true,
        expression_present: true,
        dimensions_separated: false,
        keyframes: vec![NumericKeyframe {
            time_secs: 0.875,
            values: vec![300.0],
            in_interpolation: 2,
            out_interpolation: 2,
            in_speed: vec![0.0],
            in_influence: vec![33.0],
            out_speed: vec![0.0],
            out_influence: vec![33.0],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        }],
        value_kind: NumericValueKind::Continuous,
    };
    validate_destination(&destination, 1).unwrap();
    let mut disabled = destination.clone();
    disabled.expression_enabled = false;
    assert!(validate_destination(&disabled, 1).is_err());
    let mut malformed = destination.clone();
    malformed.keyframes[0].values.clear();
    assert!(validate_destination(&malformed, 1).is_err());
    let mut invalid = destination;
    invalid.keyframes[0].values[0] = f64::NAN;
    assert!(validate_destination(&invalid, 1).is_err());
}

#[test]
fn absent_rotation_defaults_to_zero_but_malformed_source_group_is_rejected() {
    for malformed in [false, true] {
        let (template, mut source_comp) = template();
        let mut source = layer(&template, 2, "Source", Vec::new());
        if malformed {
            source.content = vec![list(b"tdgp", vec![data(b"tdmn", b"ADBE Transform Group")])];
        }
        source_comp.layers = vec![source];
        let mut owner_comp = source_comp.clone();
        owner_comp.layers = vec![layer(
            &template,
            1,
            "Owner",
            vec![(
                "ADBE Rotate Z",
                numeric(
                    &[42.0],
                    Some("comp(\"SH01\").layer(\"Source\").transform.rotation"),
                ),
            )],
        )];
        let source_item = item(3, "SH01", source_comp);
        let owner_item = item(4, "Child", owner_comp.clone());
        let items = HashMap::from([(3, &source_item), (4, &owner_item)]);
        let (properties, warnings) = super::super::read_layer_transform_with_sources(
            &owner_comp.layers[0],
            4,
            &owner_comp,
            &items,
        )
        .unwrap();
        let rotation = property(&properties, "ADBE Rotate Z");
        assert_eq!(rotation.expression_enabled, malformed, "{warnings:#?}");
        assert_eq!(rotation.values, vec![if malformed { 42.0 } else { 0.0 }]);
        assert!(!rotation.animated);
        assert!(rotation.keyframes.is_empty());
    }
}

#[test]
fn absent_planar_shape_position_reuses_source_canvas_not_destination_or_cached_position() {
    for (kind, flags, malformed, lowered) in [
        (4, 0, false, true),
        (0, 0, false, false),
        (4, 4, false, false),
        (4, 128, false, false),
        (4, 0, true, false),
    ] {
        let (template, mut source_comp) = template();
        source_comp.width = 3840;
        source_comp.height = 1600;
        let mut source = layer(&template, 2, "Source", Vec::new());
        let mut bytes = source.record.encode();
        bytes[131] = kind;
        bytes[38] = flags;
        source.record = LayerRecord::decode(&bytes).unwrap();
        if malformed {
            source.content = vec![list(b"tdgp", vec![data(b"tdmn", b"ADBE Transform Group")])];
        }
        source_comp.layers = vec![source];
        let mut owner_comp = source_comp.clone();
        owner_comp.width = 1280;
        owner_comp.height = 720;
        owner_comp.layers = vec![layer(
            &template,
            1,
            "Owner",
            vec![(
                "ADBE Position",
                numeric(
                    &[99.0, 23.0, 0.0],
                    Some("comp('SH01').layer('Source').transform.position"),
                ),
            )],
        )];
        let source_item = item(3, "SH01", source_comp);
        let owner_item = item(4, "Child", owner_comp.clone());
        let items = HashMap::from([(3, &source_item), (4, &owner_item)]);
        let (properties, warnings) = super::super::read_layer_transform_with_sources(
            &owner_comp.layers[0],
            4,
            &owner_comp,
            &items,
        )
        .unwrap();
        let position = property(&properties, "ADBE Position");
        assert_eq!(!position.expression_enabled, lowered, "{warnings:#?}");
        assert_eq!(
            position.values,
            if lowered {
                vec![1920.0, 800.0, 0.0]
            } else {
                vec![99.0, 23.0, 0.0]
            }
        );
    }
}

#[test]
fn source_relative_anchor_alias_rejects_different_solid_dimensions() {
    let (template, mut source_comp) = template();
    let mut source_layer = layer(
        &template,
        2,
        "Source",
        vec![("ADBE Anchor Point", numeric(&[0.5, 0.5], None))],
    );
    source_layer.record = record_with_source(&source_layer.record, 10);
    source_comp.layers = vec![source_layer];
    let mut owner_comp = source_comp.clone();
    let mut owner = layer(
        &template,
        1,
        "Owner",
        vec![(
            "ADBE Anchor Point",
            numeric(
                &[0.5, 0.5],
                Some("comp(\"SH01\").layer(\"Source\").transform.anchorPoint"),
            ),
        )],
    );
    owner.record = record_with_source(&owner.record, 11);
    owner_comp.layers = vec![owner];
    let source_item = item(3, "SH01", source_comp);
    let owner_item = item(4, "Child", owner_comp.clone());
    let source_solid = solid_item(10, [100, 100]);
    let owner_solid = solid_item(11, [200, 100]);
    let items = HashMap::from([
        (3, &source_item),
        (4, &owner_item),
        (10, &source_solid),
        (11, &owner_solid),
    ]);
    let (properties, warnings) = super::super::read_layer_transform_with_sources(
        &owner_comp.layers[0],
        4,
        &owner_comp,
        &items,
    )
    .unwrap();
    assert!(property(&properties, "ADBE Anchor Point").expression_enabled);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("incompatible source-relative dimensions"))
    );
}

#[test]
fn constant_anchor_aliases_use_pixels_in_occurrences_and_parent_copies() {
    for precomposition in [false, true] {
        for expression in [
            "comp(\"SH01\").layer(\"Source\").transform.anchorPoint",
            "transform.anchorPoint",
        ] {
            let (base, mut source_comp) = template();
            let mut source = layer(
                &base,
                2,
                "Source",
                vec![("ADBE Anchor Point", numeric(&[0.5, 0.25], None))],
            );
            source.record = record_with_source(&source.record, 10);
            source_comp.layers = vec![source];
            let mut owner_comp = source_comp.clone();
            let mut owner = layer(
                &base,
                1,
                "Owner",
                vec![("ADBE Anchor Point", numeric(&[0.5, 0.25], Some(expression)))],
            );
            owner.record = record_with_source(&owner.record, 10);
            let mut child = layer(&base, 3, "Child", Vec::new());
            let mut record = record_with_source(&child.record, 10).encode();
            record[132..136].copy_from_slice(&1_u32.to_be_bytes());
            child.record = LayerRecord::decode(&record).unwrap();
            owner_comp.layers = vec![owner, child];
            let source_item = if precomposition {
                let mut nested = source_comp.clone();
                nested.layers.clear();
                nested.width = 100;
                nested.height = 100;
                item(10, "Nested", nested)
            } else {
                solid_item(10, [100, 100])
            };
            let mut project = read_project(include_bytes!(
                "../../../../tests/fixtures/properties/property_1D_opacity.aep"
            ))
            .unwrap();
            project.items = vec![
                item(3, "SH01", source_comp),
                item(4, "Child", owner_comp),
                source_item,
            ];
            let converted =
                crate::structure_document::to_structural_fx_document(&project, Some(4)).unwrap();
            fn check(layers: &[fx_schema::Layer], found: &mut usize) {
                for layer in layers {
                    if let fx_schema::LayerData::Group(group) = layer.data() {
                        if group.description.contains("AEP comp=4 layer=1 kind=")
                            || group.description.contains("transform-only parent copy 1;")
                        {
                            assert_eq!(group.transform.anchor_point, [50.0, 25.0]);
                            *found += 1;
                        }
                        check(&group.layers, found);
                    }
                }
            }
            let mut found = 0;
            check(converted.document.composition().layers(), &mut found);
            assert_eq!(
                found, 2,
                "ordinary occurrence and transform-only parent copy"
            );
        }
    }
}

#[test]
fn direct_alias_lowers_and_ambiguous_or_cyclic_sources_remain_expressions() {
    let (template, mut source_comp) = template();
    source_comp.layers = vec![layer(
        &template,
        2,
        "Source",
        vec![
            ("ADBE Position", numeric(&[10.0, 20.0], None)),
            ("ADBE Scale", numeric(&[0.5, 0.5], None)),
            ("ADBE Rotate Z", numeric(&[45.0], None)),
            (
                "ADBE Opacity",
                numeric(
                    &[1.0],
                    Some("comp(\"Child\").layer(\"Owner\").transform.opacity"),
                ),
            ),
        ],
    )];
    let mut owner_comp = source_comp.clone();
    owner_comp.layers = vec![layer(
        &template,
        1,
        "Owner",
        vec![
            (
                "ADBE Position",
                numeric(
                    &[0.0, 0.0],
                    Some("comp(\"SH01\").layer(\"Source\").transform.position"),
                ),
            ),
            (
                "ADBE Scale",
                numeric(
                    &[1.0, 1.0, 1.0],
                    Some("comp(\"SH01\").layer(\"Source\").transform.scale"),
                ),
            ),
            (
                "ADBE Rotate Z",
                numeric(
                    &[0.0],
                    Some("comp(\"SH01\").layer(\"Source\").transform.rotation"),
                ),
            ),
            (
                "ADBE Opacity",
                numeric(
                    &[1.0],
                    Some("comp(\"SH01\").layer(\"Source\").transform.opacity"),
                ),
            ),
        ],
    )];
    let source_item = item(3, "SH01", source_comp.clone());
    let owner_item = item(4, "Child", owner_comp.clone());
    let items = HashMap::from([(3, &source_item), (4, &owner_item)]);
    let owner = &owner_comp.layers[0];

    let (properties, warnings) =
        super::super::read_layer_transform_with_sources(owner, 4, &owner_comp, &items).unwrap();
    assert_eq!(
        property(&properties, "ADBE Position").values,
        vec![10.0, 20.0]
    );
    assert_eq!(property(&properties, "ADBE Scale").values, vec![0.5, 0.5]);
    assert_eq!(property(&properties, "ADBE Rotate Z").values, vec![45.0]);
    assert!(property(&properties, "ADBE Opacity").expression_enabled);
    assert!(warnings.iter().any(|warning| warning.contains("cyclic")));
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("native 2D source mapped to 3D destination"))
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("cross-composition"))
    );

    let duplicate_item = item(5, "SH01", source_comp.clone());
    let ambiguous = HashMap::from([(3, &source_item), (4, &owner_item), (5, &duplicate_item)]);
    let (properties, warnings) =
        super::super::read_layer_transform_with_sources(owner, 4, &owner_comp, &ambiguous).unwrap();
    assert!(property(&properties, "ADBE Rotate Z").expression_enabled);
    assert!(warnings.iter().any(|warning| warning.contains("ambiguous")));

    let mut incompatible_comp = owner_comp.clone();
    incompatible_comp.layers[0].content = transform_content(vec![(
        "ADBE Rotate Z",
        numeric(
            &[0.0, 1.0],
            Some("comp(\"SH01\").layer(\"Source\").transform.rotation"),
        ),
    )]);
    let incompatible_item = item(6, "Incompatible", incompatible_comp.clone());
    let incompatible_items = HashMap::from([(3, &source_item), (6, &incompatible_item)]);
    let (properties, warnings) = super::super::read_layer_transform_with_sources(
        &incompatible_comp.layers[0],
        6,
        &incompatible_comp,
        &incompatible_items,
    )
    .unwrap();
    assert!(property(&properties, "ADBE Rotate Z").expression_enabled);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("dimensions are unsupported"))
    );

    source_comp.layers[0].content = transform_content(vec![(
        "ADBE Rotate Z",
        numeric(
            &[0.0],
            Some("comp(\"Child\").layer(\"Owner\").transform.rotation"),
        ),
    )]);
    let cyclic_source = item(3, "SH01", source_comp);
    let cyclic = HashMap::from([(3, &cyclic_source), (4, &owner_item)]);
    let (properties, warnings) =
        super::super::read_layer_transform_with_sources(owner, 4, &owner_comp, &cyclic).unwrap();
    assert!(property(&properties, "ADBE Rotate Z").expression_enabled);
    assert!(warnings.iter().any(|warning| warning.contains("cyclic")));
}
