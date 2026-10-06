use crate::{properties, rifx::Chunk};

/// Independently AE-authored, saved/reopened scalar endpoint controls.
pub(super) fn native_hold_endpoint_curve(layer_name: &str) -> properties::NumericProperty {
    let project = crate::structure::read_project(include_bytes!(
        "../../../tests/fixtures/properties/hold_endpoint_flags.aep"
    ))
    .unwrap();
    let composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            crate::structure::ItemKind::Composition(composition) => Some(composition),
            _ => None,
        })
        .unwrap();
    let layer = composition
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == layer_name)
        .unwrap();
    properties::read_transform(&layer.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == "ADBE Opacity")
        .unwrap()
        .numeric
        .unwrap()
}

pub(super) fn data(id: &[u8; 4], value: impl Into<Vec<u8>>) -> Chunk {
    let mut value = value.into();
    if id == b"tdmn" {
        value.resize(40, 0);
    }
    Chunk::data(*id, value).unwrap()
}

pub(super) fn list(kind: &[u8; 4], children: Vec<Chunk>) -> Chunk {
    Chunk::list(*kind, children)
}

pub(super) fn name(value: &str) -> Chunk {
    let mut bytes = b"Utf8".to_vec();
    bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    bytes.extend(value.as_bytes());
    data(b"tdsn", bytes)
}

pub(super) fn numeric(values: &[f64], expression: Option<&str>) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = u8::try_from(values.len()).unwrap();
    let mut chunks = vec![
        data(b"tdb4", meta),
        data(b"tdsb", vec![0, 0, 0, 1]),
        data(
            b"cdat",
            values
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
        name("Slider"),
    ];
    if let Some(expression) = expression {
        chunks.push(data(b"Utf8", expression.as_bytes()));
    }
    list(b"tdbs", chunks)
}

fn slider(label: &str, value: f64, expression: Option<&str>) -> Vec<Chunk> {
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

fn content(scale_expression: &str, controls: Vec<Chunk>) -> Vec<Chunk> {
    vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Scale"),
                    numeric(&[1.0, 1.0, 1.0], Some(scale_expression)),
                ],
            ),
            data(b"tdmn", b"ADBE Effect Parade"),
            list(b"tdgp", controls),
        ],
    )]
}

#[test]
fn dimension_scale_reaches_editable_static_transform() {
    use crate::structure::{ItemKind, read_project};
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    let mut comp = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.clone()),
            _ => None,
        })
        .unwrap();
    comp.width = 1920;
    comp.height = 1080;
    let mut layer = comp.layers[0].clone();
    for (expression, expected) in [
        (
            "x = thisComp.width;y = thisComp.height;[x, y]",
            [1920.0, 1080.0],
        ),
        (
            "w = thisComp.width; h = thisComp.height; aspect = 1920/1080; if(w / h >= aspect){[w, w]}else{[h*aspect, h*aspect]}",
            [1920.0, 1920.0],
        ),
    ] {
        layer.content = content(expression, Vec::new());
        let (properties, warnings) = super::read_layer_transform(&layer, &comp).unwrap();
        let scale = properties
            .iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(!scale.expression_enabled, "{warnings:?}");
        assert!(!scale.animated);
        assert!(scale.keyframes.is_empty());
        let (transform, _) =
            crate::structure_document::transform::static_transform(&layer, [100, 100], &comp);
        assert_eq!(transform.scale, expected);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("live composition-resize linkage is not retained"))
        );
    }
}

#[test]
fn indexed_static_slider_position_preserves_native_layer_depth() {
    use crate::structure::{ItemKind, read_project};
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    let mut comp = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some((**comp).clone()),
            _ => None,
        })
        .unwrap();
    let mut owner = comp.layers[0].clone();
    owner.record = owner.record.with_three_d_layer(true).unwrap();
    owner.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Position"),
                    numeric(
                        &[1920.0, 1080.0, 0.0],
                        Some(
                            "[1920,1080,(index-2)*thisComp.layer(\"Extrude CTRL\").effect(\"Slider Control 1\")(\"ADBE Slider Control-0001\")]",
                        ),
                    ),
                ],
            ),
        ],
    )];
    let mut controller = owner.clone();
    let mut record = controller.record.encode();
    record[..4].copy_from_slice(&999_u32.to_be_bytes());
    controller.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
    controller.name = "Extrude CTRL".into();
    controller.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Effect Parade"),
            list(b"tdgp", slider("Slider Control 1", -2.0, None)),
        ],
    )];
    // The native 1-based index includes non-rendering controllers as well.
    comp.layers = vec![controller.clone(), owner.clone()];
    for (index, expected_z) in [(1, 0.0), (2, -2.0)] {
        if index == 2 {
            let mut preceding = controller.clone();
            let mut record = preceding.record.encode();
            record[..4].copy_from_slice(&998_u32.to_be_bytes());
            preceding.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
            preceding.name = "Unrelated null".into();
            comp.layers.insert(0, preceding);
        }
        let (properties, warnings) = super::read_layer_transform(&owner, &comp).unwrap();
        let position = properties
            .iter()
            .find(|p| p.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(!position.expression_enabled, "{warnings:?}");
        assert_eq!(position.values, [1920.0, 1080.0, expected_z]);
        assert!(!position.animated);
        assert!(position.keyframes.is_empty());
        assert!(warnings.iter().any(|warning| {
            warning.contains("layer-order/controller edit linkage is not retained")
        }));
    }
    for failure in [
        "duplicate",
        "missing",
        "expression",
        "nonfinite",
        "2d",
        "animated",
    ] {
        let mut rejected = comp.clone();
        let mut rejected_owner = owner.clone();
        match failure {
            "duplicate" => rejected.layers.push(controller.clone()),
            "missing" => rejected
                .layers
                .retain(|layer| layer.name.as_ref() != "Extrude CTRL"),
            "2d" => {
                rejected_owner.record = rejected_owner.record.with_three_d_layer(false).unwrap()
            }
            _ => {
                let value = if failure == "nonfinite" {
                    f64::INFINITY
                } else {
                    -2.0
                };
                let expression = (failure == "expression").then_some("time");
                let mut controls = slider("Slider Control 1", value, expression);
                if failure == "animated" {
                    // An animated flag without a supported track must never be
                    // certified static, even when cdat carries a cached value.
                    let chunk = &mut controls[1].children_mut().unwrap()[0]
                        .children_mut()
                        .unwrap()[2]
                        .children_mut()
                        .unwrap()[0];
                    let mut meta = chunk.data_payload().unwrap().to_vec();
                    meta[68] = 1;
                    *chunk = data(b"tdb4", meta);
                }
                let controller = rejected
                    .layers
                    .iter_mut()
                    .find(|layer| layer.name.as_ref() == "Extrude CTRL")
                    .unwrap();
                controller.content = vec![list(
                    b"tdgp",
                    vec![
                        data(b"tdmn", b"ADBE Effect Parade"),
                        list(b"tdgp", controls),
                    ],
                )];
            }
        }
        let (properties, warnings) =
            super::read_layer_transform(&rejected_owner, &rejected).unwrap();
        let position = properties
            .iter()
            .find(|p| p.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(position.expression_enabled, "{failure}: {warnings:?}");
        assert_eq!(position.values, [1920.0, 1080.0, 0.0]);
    }
}

const SCALE: &str = "[effect(\"Scale X\")(\"ADBE Slider Control-0001\"), effect(\"Scale Y\")(\"ADBE Slider Control-0001\")]";

#[test]
fn pure_slider_alias_scale_is_lowered_without_changing_raw_reader() {
    // Synthetic storage-level regression; independent Intro evidence stays local.
    let mut controls = slider("Scale X", 13.32547551601051, None);
    controls.extend(slider(
        "Scale Y",
        100.0,
        Some("effect(\"Scale X\")(\"Slider\")"),
    ));
    let content = content(SCALE, controls);
    let raw = properties::read_transform(&content).unwrap();
    assert!(raw[0].numeric.as_ref().unwrap().expression_enabled);
    let (lowered, warnings) = super::read_transform(&content).unwrap();
    let scale = lowered[0].numeric.as_ref().unwrap();
    assert!(!scale.expression_enabled);
    assert_eq!(scale.values, vec![0.1332547551601051; 2]);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("independent editable"))
    );
}

#[test]
fn unsupported_or_ambiguous_links_keep_the_original_expression() {
    let cases = [
        slider("Scale X", 10.0, Some("effect('Scale X')('Slider')")),
        [slider("Scale X", 10.0, None), slider("Scale X", 10.0, None)].concat(),
        [slider("Scale X", 10.0, None), slider("Scale Y", 11.0, None)].concat(),
        [
            slider("Scale X", 10.0, None),
            slider(
                "Scale Y",
                10.0,
                Some("effect('Scale X')('Slider').valueAtTime(1)"),
            ),
        ]
        .concat(),
        [
            slider("Scale X", 10.0, Some("effect('Scale Y')('Slider')")),
            slider("Scale Y", 10.0, Some("effect('Scale X')('Slider')")),
        ]
        .concat(),
    ];
    for controls in cases {
        let (properties, warnings) = super::read_transform(&content(SCALE, controls)).unwrap();
        assert!(properties[0].numeric.as_ref().unwrap().expression_enabled);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("not lowered"))
        );
    }
}

#[test]
fn reference_grammar_never_accepts_executable_suffixes_or_time_access() {
    for expression in [
        format!("{SCALE}; doSomething()"),
        format!("{SCALE} + value"),
        "[thisLayer.effect('X')('Slider'), effect('X')('Slider')]".into(),
        "[effect('X')('Slider').valueAtTime(time), effect('X')('Slider')]".into(),
        "[effect('X')('Slider')]".into(),
        "[effect('X')('Slider'), effect('X')('Slider'), effect('X')('Slider'), effect('X')('Slider')]".into(),
    ] {
        assert!(super::vector_references(&expression).is_none(), "{expression}");
    }
    assert_eq!(
        super::vector_references(" [ effect ( 'X' ) ( 'Slider' ), effect('X')('Slider') ] ; ")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn slider_key_units_and_temporal_easing_are_preserved_for_both_axes() {
    use crate::properties::{NumericKeyframe, NumericProperty, NumericValueKind};
    let key = NumericKeyframe {
        time_secs: 2.0,
        values: vec![13.32547551601051],
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![201.14342869180186],
        out_speed: vec![197.8776440084868],
        in_influence: vec![1.7170262369487377],
        out_influence: vec![43.549846017216765],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    };
    let scalar = NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![key.clone()],
        value_kind: NumericValueKind::Continuous,
    };
    let scale = super::scale_vector(scalar, 2).unwrap();
    let actual = &scale.keyframes[0];
    assert_eq!(actual.time_secs, key.time_secs);
    for axis in 0..2 {
        assert!((actual.values[axis] * 100.0 - key.values[0]).abs() < 1e-12);
        assert!((actual.in_speed[axis] * 100.0 - key.in_speed[0]).abs() < 1e-12);
        assert!((actual.out_speed[axis] * 100.0 - key.out_speed[0]).abs() < 1e-12);
        assert_eq!(actual.in_influence[axis], key.in_influence[0]);
        assert_eq!(actual.out_influence[axis], key.out_influence[0]);
    }
    assert_eq!(actual.in_interpolation, key.in_interpolation);
    assert_eq!(actual.out_interpolation, key.out_interpolation);
}

#[test]
fn numeric_slider_index_one_selects_only_the_value_parameter() {
    let scale = |index: &str| {
        let expression = format!("[effect('Scale X')({index}), effect('Scale X')({index})]");
        super::read_transform(&content(&expression, slider("Scale X", 25.0, None))).unwrap()
    };
    let (lowered, _) = scale("1");
    let lowered = lowered[0].numeric.as_ref().unwrap();
    assert!(!lowered.expression_enabled);
    assert_eq!(lowered.values, vec![0.25; 2]);
    for index in ["0", "2", "10", "1.0", "01"] {
        let (properties, warnings) = scale(index);
        assert!(
            properties[0].numeric.as_ref().unwrap().expression_enabled,
            "{index}"
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("not lowered")),
            "{index}"
        );
    }
}

#[test]
fn slider_index_one_reaches_source_text_percent_and_default_named_sliders() {
    let percent = "s = effect(\"Progress\")(1);\rMath.round(s).toLocaleString() + \"%\";";
    let value = super::Reference {
        effect: "Progress",
        parameter: super::SLIDER_VALUE,
    };
    assert_eq!(super::slider::percent_reference(percent), Some(value));

    let indexed = "[effect(\"Slider Control\")(1), effect(\"Slider Control\")(1)]";
    let slider = named_effect(
        "ADBE Slider Control",
        "-_0_/-",
        Some("Slider Control"),
        40.0,
    );
    let (lowered, warnings) = super::read_transform(&content(indexed, slider)).unwrap();
    let scale = lowered[0].numeric.as_ref().unwrap();
    assert!(!scale.expression_enabled, "{warnings:?}");
    assert_eq!(scale.values, vec![0.4; 2]);
}

#[test]
fn invalid_display_name_length_or_padding_is_rejected() {
    assert_eq!(super::display_name(&[name("Scale X")]), Some("Scale X"));
    assert_eq!(
        super::display_name(&[data(b"tdsn", b"Utf8\0\0\0\x7fshort")]),
        None
    );
    assert_eq!(
        super::display_name(&[data(b"tdsn", b"Utf8\0\0\0\x01Xevil")]),
        None
    );
    assert_eq!(super::display_name(&[name("X"), name("X")]), None);
}

/// A plugin descriptor's native `fnam` default effect name.
fn default_name(value: &str) -> Chunk {
    let mut bytes = b"Utf8".to_vec();
    bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    bytes.extend(value.as_bytes());
    data(b"fnam", bytes)
}

/// An effect of `kind` whose display name is `display` (AE stores `-_0_/-`
/// for a default name) and whose descriptor's `fnam` is `fnam`.
fn named_effect(kind: &str, display: &str, fnam: Option<&str>, value: f64) -> Vec<Chunk> {
    let mut descriptor: Vec<Chunk> = fnam.map(default_name).into_iter().collect();
    descriptor.push(list(
        b"tdgp",
        vec![
            name(display),
            data(b"tdmn", format!("{kind}-0001")),
            numeric(&[value], None),
        ],
    ));
    vec![data(b"tdmn", kind), list(b"sspc", descriptor)]
}

#[test]
fn effect_name_consults_fnam_only_for_the_default_name_placeholder() {
    let descriptor = [default_name("Slider Control")];
    assert_eq!(
        super::effect_name(&descriptor, &[name("-_0_/-")]),
        Some("Slider Control")
    );
    assert_eq!(
        super::effect_name(&descriptor, &[name("Progress")]),
        Some("Progress"),
        "a custom display name stays authoritative"
    );
    for (descriptor, controls) in [
        (Vec::new(), vec![name("-_0_/-")]),
        (
            vec![data(b"fnam", b"Utf8\0\0\0\x7fshort")],
            vec![name("-_0_/-")],
        ),
        (
            vec![default_name("A"), default_name("B")],
            vec![name("-_0_/-")],
        ),
        (descriptor.to_vec(), Vec::new()),
    ] {
        assert_eq!(super::effect_name(&descriptor, &controls), None);
    }
}

#[test]
fn default_named_sliders_resolve_by_fnam_without_weakening_identity_checks() {
    const DEFAULT: &str =
        "[effect(\"Slider Control\")(\"Slider\"), effect(\"Slider Control\")(\"Slider\")]";
    let slider = |display, fnam| named_effect("ADBE Slider Control", display, fnam, 40.0);
    let (lowered, warnings) =
        super::read_transform(&content(DEFAULT, slider("-_0_/-", Some("Slider Control")))).unwrap();
    let scale = lowered[0].numeric.as_ref().unwrap();
    assert!(!scale.expression_enabled, "{warnings:?}");
    assert_eq!(scale.values, vec![0.4; 2]);

    for controls in [
        // A custom display name is not the default name it replaced.
        slider("Progress", Some("Slider Control")),
        // The placeholder alone names nothing.
        slider("-_0_/-", None),
        // A default and a custom "Slider Control" collide.
        [
            slider("-_0_/-", Some("Slider Control")),
            slider("Slider Control", Some("Slider Control")),
        ]
        .concat(),
        // The default name must still belong to a Slider Control.
        named_effect("ADBE Angle Control", "-_0_/-", Some("Slider Control"), 40.0),
    ] {
        let (properties, warnings) = super::read_transform(&content(DEFAULT, controls)).unwrap();
        assert!(properties[0].numeric.as_ref().unwrap().expression_enabled);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("not lowered")),
            "{warnings:?}"
        );
    }
    let wrong_parameter = DEFAULT.replace("(\"Slider\")", "(\"Angle\")");
    let (properties, _) = super::read_transform(&content(
        &wrong_parameter,
        slider("-_0_/-", Some("Slider Control")),
    ))
    .unwrap();
    assert!(properties[0].numeric.as_ref().unwrap().expression_enabled);
}

#[test]
fn independent_and_sibling_scale_curves_keep_separate_editable_axes() {
    use crate::structure::{ItemKind, read_project};
    use crate::structure_document::{
        animation, animation_budget::AnimationBudget, camera_normalization, transform,
    };
    use fx_schema::{LayerId, PropType, PropertyTarget};
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    let mut comp = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(c) => Some(c.clone()),
            _ => None,
        })
        .unwrap();
    let mut layer = comp.layers[0].clone();
    let root = properties::root_runs(&layer.content).unwrap();
    let leaves = properties::runs(
        properties::unique_list(
            super::unique_run(&root, "ADBE Transform Group").unwrap(),
            *b"tdgp",
        )
        .unwrap(),
    )
    .unwrap();
    let mut keys = properties::unique_list(
        super::unique_run(&leaves, "ADBE Opacity").unwrap(),
        *b"tdbs",
    )
    .unwrap()
    .to_vec();
    keys.retain(|chunk| chunk.id() != *b"tdsn");
    keys.push(name("Slider"));
    let native = properties::read_numeric(&keys).unwrap();
    let mut controls = slider("Scale X", 150., None);
    controls.extend([
        data(b"tdmn", b"ADBE Slider Control"),
        list(
            b"sspc",
            vec![list(
                b"tdgp",
                vec![
                    name("Scale Y"),
                    data(b"tdmn", b"ADBE Slider Control-0001"),
                    list(b"tdbs", keys),
                ],
            )],
        ),
    ]);
    layer.content = content(SCALE, controls);
    layer.name = "Bottom".into();
    for sibling in [false, true] {
        comp.layers = vec![layer.clone()];
        let mut owner = layer.clone();
        owner.name = "Top".into();
        if sibling {
            let mut controls = slider("Scale X", 150., None);
            controls.extend(slider(
                "Scale Y",
                100.,
                Some("thisComp.layer('Bottom').effect('Scale Y')('Slider')"),
            ));
            owner.content = content(SCALE, controls);
        }
        let (props, warnings) = super::read_layer_transform(&owner, &comp).unwrap();
        assert!(
            !props
                .iter()
                .find(|p| p.match_name == "ADBE Scale")
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
                .expression_enabled,
            "{warnings:?}"
        );
        let y = props
            .iter()
            .find(|p| p.match_name == super::SCALE_Y)
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(y.keyframes.len(), native.keyframes.len());
        for (key, expected) in y.keyframes.iter().zip(&native.keyframes) {
            assert_eq!(key.time_secs, expected.time_secs);
            assert_eq!(key.values[0], expected.values[0] * 0.01);
            assert_eq!(key.in_influence, expected.in_influence);
        }
        let (static_value, _) = transform::static_transform(&owner, [1920, 1080], &comp);
        assert_eq!(static_value.scale[0], 150.);
        let id = LayerId::new(91);
        let (entries, _) = animation::transform_entries(
            &owner,
            &comp,
            id,
            animation::AnimationTargetClock::ParentIdentity,
            [1.; 2],
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].target,
            PropertyTarget::layer(id, PropType::ScaleY)
        );
        let (parent, _) = animation::transform_parent_entries(
            &owner,
            &comp,
            id,
            [1.; 2],
            &mut AnimationBudget::default(),
        );
        let (camera, _) = camera_normalization::corrected_transform_entries(
            &owner,
            &comp,
            id,
            camera_normalization::LayerCorrection {
                position: Some([1., 2.]),
                anchor: None,
            },
            [1.; 2],
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries, parent);
        assert_eq!(entries, camera);
        if sibling {
            comp.layers.push(layer.clone());
            let (props, _) = super::read_layer_transform(&owner, &comp).unwrap();
            assert!(!props.iter().any(|p| p.match_name == super::SCALE_Y));
            assert!(
                props
                    .iter()
                    .find(|p| p.match_name == "ADBE Scale")
                    .unwrap()
                    .numeric
                    .as_ref()
                    .unwrap()
                    .expression_enabled
            );
        }
    }
}

#[test]
fn animated_links_reach_owner_parent_and_camera_normalized_tracks() {
    use crate::structure::{ItemKind, read_project};
    use crate::structure_document::{
        animation, animation_budget::AnimationBudget, camera_normalization,
    };
    use fx_schema::LayerId;
    // Mutated native storage is supplemental integration coverage, not Adobe proof.
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    let composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) if !comp.layers.is_empty() => Some(comp),
            _ => None,
        })
        .unwrap();
    let mut layer = composition.layers[0].clone();
    let root = properties::root_runs(&layer.content).unwrap();
    let transform = super::unique_run(&root, "ADBE Transform Group").unwrap();
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").unwrap()).unwrap();
    let numeric = properties::unique_list(
        super::unique_run(&leaves, "ADBE Opacity").unwrap(),
        *b"tdbs",
    )
    .unwrap()
    .to_vec();
    assert!(
        !properties::read_numeric(&numeric)
            .unwrap()
            .keyframes
            .is_empty()
    );
    let mut controls = vec![
        data(b"tdmn", b"ADBE Slider Control"),
        list(
            b"sspc",
            vec![list(
                b"tdgp",
                vec![
                    name("Scale X"),
                    data(b"tdmn", b"ADBE Slider Control-0001"),
                    list(b"tdbs", numeric),
                ],
            )],
        ),
    ];
    controls.extend(slider(
        "Scale Y",
        100.0,
        Some("effect('Scale X')('ADBE Slider Control-0001')"),
    ));
    layer.content = content(SCALE, controls);
    let id = LayerId::new(77);
    let (owner, _) = animation::transform_entries(
        &layer,
        composition,
        id,
        animation::AnimationTargetClock::ParentIdentity,
        [1.0; 2],
        &mut AnimationBudget::default(),
    );
    let (parent, _) = animation::transform_parent_entries(
        &layer,
        composition,
        id,
        [1.0; 2],
        &mut AnimationBudget::default(),
    );
    let (camera, _) = camera_normalization::corrected_transform_entries(
        &layer,
        composition,
        id,
        camera_normalization::LayerCorrection {
            position: Some([10.0, 20.0]),
            anchor: None,
        },
        [1.0; 2],
        &mut AnimationBudget::default(),
    );
    assert_eq!(owner.len(), 2);
    assert_eq!(owner, parent);
    assert_eq!(owner, camera);
}

#[test]
fn keyed_position_wiggle_keeps_authored_base_motion_with_diagnostics() {
    use crate::structure::{ItemKind, read_project};
    use crate::structure_document::{animation, animation_budget::AnimationBudget};
    use fx_schema::LayerId;

    // Adding the Intro expression to native key storage is supplementary coverage,
    // not an independently Adobe-authored wiggle fixture or render oracle.
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/properties/property_2D_position.aep"
    ))
    .unwrap();
    let composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) if !comp.layers.is_empty() => Some(comp),
            _ => None,
        })
        .unwrap();
    let mut layer = composition.layers[0].clone();
    let root = properties::root_runs(&layer.content).unwrap();
    let transform = super::unique_run(&root, "ADBE Transform Group").unwrap();
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").unwrap()).unwrap();
    let mut storage = properties::unique_list(
        super::unique_run(&leaves, "ADBE Position").unwrap(),
        *b"tdbs",
    )
    .unwrap()
    .to_vec();
    let native = properties::read_numeric(&storage).unwrap();
    assert!(native.animated && !native.keyframes.is_empty());
    storage.push(data(b"Utf8", b"posterizeTime(5);\rwiggle(3, 5);"));
    layer.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![data(b"tdmn", b"ADBE Position"), list(b"tdbs", storage)],
            ),
        ],
    )];
    let (resolved, warnings) = super::read_layer_transform(&layer, composition).unwrap();
    let position = resolved
        .iter()
        .find(|property| property.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert!(!position.expression_enabled);
    assert_eq!(position.keyframes, native.keyframes);
    assert!(warnings.iter().any(|warning| {
        warning.contains("native base Position keys retained")
            && warning.contains("jitter and posterized sampling omitted")
    }));
    let (entries, _) = animation::transform_entries(
        &layer,
        composition,
        LayerId::new(77),
        animation::AnimationTargetClock::ParentIdentity,
        [1.0; 2],
        &mut AnimationBudget::default(),
    );
    assert!(!entries.is_empty());
}
