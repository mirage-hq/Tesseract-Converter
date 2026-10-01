use super::*;

fn fixture() -> Composition {
    let project = read_project(include_bytes!(
        "../../../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some((**composition).clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn compact_generated_coordinates_remain_within_the_original_fit_tolerance() {
    let original = vec![[1234.123456789, -567.987654321], [0.000000001, f64::MAX]];
    let mut compact = original.clone();
    compact_fitted_coordinates(&mut compact);
    assert_eq!(compact[0], [1234.1235, -567.9877]);
    assert_eq!(compact[1], [0.0, f64::MAX]);
    assert!(point_error(&original, &compact) < FIT_TOLERANCE_PIXELS);
    assert!(
        serde_json::to_vec(&compact).unwrap().len() < serde_json::to_vec(&original).unwrap().len()
    );

    // Validation compares compact endpoints to the original analytical curve,
    // not to rounded oracle samples. It must still detect an interior excursion.
    let fitted = BTreeMap::from([(0, vec![[0.0, 0.0]]), (2, vec![[0.0, 0.0]])]);
    let mut evaluations = 0;
    let splits = validation_splits(
        &fitted,
        &|time, _| Ok(vec![[if time == 1 { 0.1 } else { 0.0 }, 0.0]]),
        &mut evaluations,
    )
    .unwrap();
    assert_eq!(splits, vec![(1, vec![[0.1, 0.0]])]);
}

#[test]
fn sparse_transform_keeps_center_and_requires_source_backed_anchor() {
    let composition = fixture();
    let mut source_layer = composition.layers[0].clone();
    let mut bytes = source_layer.record.encode();
    bytes[38] &= !0x80;
    bytes[40..44].copy_from_slice(&99_u32.to_be_bytes());
    bytes[131] = 0;
    source_layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    let layer = &source_layer;
    let zero = NumericProperty {
        values: vec![0.0],
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind: crate::properties::NumericValueKind::Continuous,
    };
    let mut node = TransformNode {
        layer,
        default_position: [1920.0, 800.0],
        default_anchor: None,
        anchor_scale: [1.0, 1.0],
        position_blend: None,
        properties: vec![
            crate::properties::TransformProperty {
                match_name: "ADBE Position_0".into(),
                numeric: Ok(zero.clone()),
            },
            crate::properties::TransformProperty {
                match_name: "ADBE Anchor Point".into(),
                numeric: Ok(zero),
            },
        ],
    };
    assert_eq!(
        node.matrix(0.0, None).unwrap().apply([0.0, 0.0]),
        [1920.0, 800.0]
    );

    node.properties
        .retain(|property| property.match_name != "ADBE Anchor Point");
    let error = node.matrix(0.0, None).unwrap_err();
    assert!(error.contains(&format!("layer {}", layer.record.id())));
    assert!(error.contains("lacks Anchor Point and physical source dimensions"));

    let mut shape_layer = layer.clone();
    let mut bytes = shape_layer.record.encode();
    bytes[131] = 4;
    shape_layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    node.layer = &shape_layer;
    assert_eq!(node.anchor(0.0).unwrap(), [0.0, 0.0]);

    node.properties.push(crate::properties::TransformProperty {
        match_name: "ADBE Anchor Point".into(),
        numeric: Err(PropertyError::Layout("malformed present anchor")),
    });
    assert!(node.matrix(0.0, None).is_err());
}

#[test]
fn source_relative_solid_anchors_share_static_keyed_and_null_units() {
    let composition = fixture();
    let mut solid_layer = composition.layers[0].clone();
    let mut bytes = solid_layer.record.encode();
    bytes[38] &= !0x80;
    bytes[40..44].copy_from_slice(&99_u32.to_be_bytes());
    bytes[131] = 0;
    solid_layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    let source = ProjectItem {
        id: 99,
        name: "3840 solid".into(),
        parent_folder: None,
        kind: ItemKind::Footage,
        footage: None,
        solid: Some(Ok(crate::structure::SolidSource {
            width: 3_840,
            height: 1_600,
            pixel_aspect: (1, 1),
            color: [0.0; 3],
        })),
        media: None,
        native_media: None,
    };
    let anchor_dimensions = source_anchor_dimensions(Some(&source), &solid_layer);
    let static_anchor = NumericProperty {
        values: vec![0.5, 0.25],
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind: crate::properties::NumericValueKind::Continuous,
    };
    let mut node = TransformNode {
        layer: &solid_layer,
        properties: vec![crate::properties::TransformProperty {
            match_name: "ADBE Anchor Point".into(),
            numeric: Ok(static_anchor.clone()),
        }],
        default_position: [1920.0, 800.0],
        default_anchor: Some([1_920.0, 800.0]),
        anchor_scale: solid_anchor_scale(Some(&source), anchor_dimensions),
        position_blend: None,
    };
    assert_eq!(node.anchor(0.0).unwrap(), [1_920.0, 400.0]);

    let key = |time_secs| NumericKeyframe {
        time_secs,
        values: static_anchor.values.clone(),
        in_interpolation: 1,
        out_interpolation: 1,
        in_speed: Vec::new(),
        in_influence: Vec::new(),
        out_speed: Vec::new(),
        out_influence: Vec::new(),
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    };
    node.properties[0].numeric = Ok(NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![key(0.0), key(1.0)],
        value_kind: crate::properties::NumericValueKind::Continuous,
    });
    let (start, stretch) = valid_clock(&solid_layer).unwrap();
    assert_eq!(
        node.anchor(start + 0.5 * stretch).unwrap(),
        [1_920.0, 400.0]
    );

    let mut null_layer = solid_layer.clone();
    let mut bytes = null_layer.record.encode();
    bytes[38] |= 0x80;
    null_layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    node.layer = &null_layer;
    let anchor_dimensions = source_anchor_dimensions(Some(&source), &null_layer);
    node.default_anchor = None;
    node.anchor_scale = solid_anchor_scale(Some(&source), anchor_dimensions);
    assert_eq!(
        node.anchor(start + 0.5 * stretch).unwrap(),
        [1_920.0, 400.0]
    );
    node.properties.clear();
    assert_eq!(node.anchor(start + 0.5 * stretch).unwrap(), [0.0, 0.0]);
}

#[test]
fn duplicate_native_transform_identity_is_rejected() {
    let mut composition = fixture();
    let layer = composition.layers[0].clone();
    let id = layer.record.id();
    composition.layers.push(layer);
    assert!(
        TransformRig::new(&composition, &HashMap::new())
            .add(id)
            .unwrap_err()
            .contains("ambiguous")
    );
}

#[test]
fn auto_orient_along_path_is_rejected_before_transform_lowering() {
    let mut composition = fixture();
    let id = composition.layers[0].record.id();
    let mut bytes = composition.layers[0].record.encode();
    bytes[38] |= 1;
    composition.layers[0].record =
        crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    let error = TransformRig::new(&composition, &HashMap::new())
        .add(id)
        .unwrap_err();
    assert!(error.contains(&format!("layer {id}")));
    assert!(error.contains("auto-orient along path"));
}

#[test]
fn denied_dynamic_path_entry_does_not_consume_budget() {
    let entry = AnimationGraphEntry {
        target: PropertyTarget::layer(LayerId::new(1), PropType::ShapePath),
        animator: PropertyAnimator::constant(PropertyValue::Path(ShapePath {
            commands: vec![
                ShapePathCommand::MoveTo {
                    x: 0.0,
                    y: 0.0,
                    mirror: None,
                    corner_radius: None,
                },
                ShapePathCommand::LineTo {
                    x: 1.0,
                    y: 1.0,
                    mirror: None,
                    corner_radius: None,
                },
            ],
        }))
        .unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let mut budget = AnimationBudget::with_limit(0);
    assert!(reserve_entry(&mut budget, &entry).is_err());
    assert_eq!(budget.used(), 0);
}

#[test]
fn zero_tangent_spatial_segments_use_linear_distance_and_curves_are_rejected() {
    let composition = fixture();
    let layer = &composition.layers[0];
    let key = |time_secs, values, spatial_in, spatial_out| NumericKeyframe {
        time_secs,
        values,
        in_interpolation: 1,
        out_interpolation: 1,
        in_speed: Vec::new(),
        in_influence: Vec::new(),
        out_speed: Vec::new(),
        out_influence: Vec::new(),
        spatial_in,
        spatial_out,
    };
    let mut property = NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![
            key(0.0, vec![0.0, 0.0], vec![0.0, 0.0], vec![0.0, 0.0]),
            key(1.0, vec![100.0, 0.0], vec![0.0, 0.0], vec![0.0, 0.0]),
        ],
        value_kind: crate::properties::NumericValueKind::Continuous,
    };
    let (start, stretch) = valid_clock(layer).unwrap();
    let quarter_time = start + 0.25 * stretch;
    assert_eq!(
        evaluate_numeric(&property, quarter_time, layer, 0).unwrap(),
        25.0
    );

    property.keyframes[0].spatial_out = vec![50.0, 25.0];
    let error = evaluate_numeric(&property, quarter_time, layer, 0).unwrap_err();
    assert!(error.contains(&format!("layer {}", layer.record.id())));
    assert!(error.contains("component 0"));
    assert!(error.contains("nonzero spatial tangents"));
}

#[test]
fn equal_scalar_endpoints_preserve_signed_temporal_excursions() {
    let composition = fixture();
    let properties = crate::properties::read_transform(&composition.layers[0].content).unwrap();
    let source = properties
        .iter()
        .find_map(|property| {
            property
                .numeric
                .as_ref()
                .ok()
                .filter(|numeric| !numeric.keyframes.is_empty())
        })
        .unwrap();
    let mut from = source.keyframes[0].clone();
    let mut to = from.clone();
    from.values = vec![10.0];
    to.values = vec![10.0];
    from.out_interpolation = 2;
    to.in_interpolation = 2;
    from.out_influence = vec![100.0 / 3.0];
    to.in_influence = vec![100.0 / 3.0];
    from.out_speed = vec![120.0];
    to.in_speed = vec![-120.0];
    assert!((equal_endpoint_value(&from, &to, 0, 1.0, 0.5).unwrap() - 40.0).abs() < 1e-6);
    from.out_speed[0] = -120.0;
    to.in_speed[0] = 120.0;
    assert!((equal_endpoint_value(&from, &to, 0, 1.0, 0.5).unwrap() + 20.0).abs() < 1e-6);
    from.out_interpolation = 3;
    assert_eq!(equal_endpoint_value(&from, &to, 0, 1.0, 0.5).unwrap(), 10.0);
}

#[test]
fn full_grid_validation_refines_errors_between_quarter_samples() {
    let fitted = BTreeMap::from([
        (0, vec![[0.0, 0.0], [1.0, 1.0]]),
        (10, vec![[0.0, 0.0], [1.0, 1.0]]),
    ]);
    let evaluate = |time, evaluations: &mut usize| {
        *evaluations += 1;
        let offset = if time == 4 { 0.02 } else { 0.0 };
        Ok(vec![[offset, 0.0], [1.0, 1.0]])
    };
    let mut evaluations = 0;
    assert_eq!(
        validation_splits(&fitted, &evaluate, &mut evaluations).unwrap(),
        vec![(4, vec![[0.02, 0.0], [1.0, 1.0]])]
    );
}

#[test]
fn dynamic_path_track_rejects_non_finite_fitted_vertices() {
    let geometry = PathGeometry::new(ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 1.0,
                y: 1.0,
                mirror: None,
                corner_radius: None,
            },
        ],
    })
    .unwrap();
    let fitted = BTreeMap::from([
        (0, vec![[0.0, 0.0], [1.0, 1.0]]),
        (1, vec![[f64::NAN, 0.0], [1.0, 1.0]]),
    ]);
    assert!(
        build_path_track(
            fx_schema::LayerId::new(1),
            fitted,
            NumericAnimationClock::source_local(),
            &geometry,
        )
        .is_err()
    );
}

#[test]
fn source_seed_count_uses_the_analytical_evaluation_boundary() {
    let mut seeds = BTreeSet::new();
    for time in 0..MAX_EVALUATIONS as i64 {
        insert_seed(&mut seeds, time).unwrap();
    }
    insert_seed(&mut seeds, 0).unwrap();
    assert!(insert_seed(&mut seeds, MAX_EVALUATIONS as i64).is_err());
}

#[test]
fn dynamic_path_tracks_cross_the_former_key_and_byte_quotas() {
    let geometry = PathGeometry::new(ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 1.0,
                y: 1.0,
                mirror: None,
                corner_radius: None,
            },
        ],
    })
    .unwrap();
    let points = vec![[0.0, 0.0], [1.0, 1.0]];
    let owner_id = fx_schema::LayerId::new(1);
    let clock = NumericAnimationClock::source_local();
    let fitted: BTreeMap<_, _> = (0..10_000).map(|time| (time, points.clone())).collect();
    let track = build_path_track(owner_id, fitted, clock, &geometry).unwrap();
    assert_eq!(track.keyframes().len(), 10_000);
    let wire = serde_json::to_vec(&track).unwrap();
    assert!(wire.len() > 1024 * 1024);
    let restored: PropertyKeyframeTrack = serde_json::from_slice(&wire).unwrap();
    assert_eq!(restored, track);
}
