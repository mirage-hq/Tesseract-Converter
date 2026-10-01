use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::animation_budget::AnimationBudget;
use crate::structure_document::shapes::{OutputBudget, program};
use fx_schema::BlendMode;

#[test]
fn paired_rect_miter_and_geometry_keys_keep_rect_and_group_ownership() {
    // Supplemental native-record construction with relabelled keys from the
    // pinned Scale fixture; this is not independent Adobe feature proof.
    fn numeric(name: &str, values: &[f64]) -> Vec<Chunk> {
        let mut marker = name.as_bytes().to_vec();
        marker.resize(40, 0);
        let mut header = vec![0; 124];
        header[..2].copy_from_slice(&[0xdb, 0x99]);
        header[3] = values.len() as u8;
        vec![
            Chunk::data(*b"tdmn", marker).unwrap(),
            Chunk::list(
                *b"tdbs",
                vec![
                    Chunk::data(*b"tdb4", header).unwrap(),
                    Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                    Chunk::data(
                        *b"cdat",
                        values
                            .iter()
                            .flat_map(|value| value.to_be_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                ],
            ),
        ]
    }
    fn keyed(name: &str) -> Vec<Chunk> {
        let mut marker = name.as_bytes().to_vec();
        marker.resize(40, 0);
        let mut run = vec![Chunk::data(*b"tdmn", marker).unwrap()];
        run.extend(animated_scale_run());
        run
    }
    fn group(name: &str, leaves: Vec<Vec<Chunk>>) -> Vec<Chunk> {
        let mut marker = name.as_bytes().to_vec();
        marker.resize(40, 0);
        vec![
            Chunk::data(*b"tdmn", marker).unwrap(),
            Chunk::list(*b"tdgp", leaves.into_iter().flatten().collect()),
        ]
    }
    let children: Vec<_> = [
        group(
            "ADBE Vector Shape - Rect",
            vec![
                numeric("ADBE Vector Rect Size", &[100.0, 50.0]),
                keyed("ADBE Vector Rect Position"),
                numeric("ADBE Vector Rect Roundness", &[0.0]),
            ],
        ),
        group(
            "ADBE Vector Graphic - Stroke",
            vec![
                numeric("ADBE Vector Stroke Color", &[0.0, 0.0, 1.0]),
                numeric("ADBE Vector Stroke Width", &[4.0]),
                numeric("ADBE Vector Stroke Opacity", &[100.0]),
                keyed("ADBE Vector Stroke Miter Limit"),
            ],
        ),
        group(
            "ADBE Vector Graphic - Fill",
            vec![
                numeric("ADBE Vector Fill Color", &[1.0, 0.0, 0.0]),
                numeric("ADBE Vector Fill Opacity", &[100.0]),
            ],
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut program = program::Program::parse(&children, 24);
    assert!(single_source(&program).is_none());
    for disabled in 0..2 {
        program.paints[disabled].enabled = false;
        let (_, paint, _) =
            single_source(&program).expect("disabled paint must not force path fallback");
        assert_eq!(paint.name, program.paints[1 - disabled].operation.name);
        program.paints[disabled].enabled = true;
    }
    for paint in &mut program.paints {
        paint.enabled = false;
    }
    assert!(single_source(&program).is_none());
    for paint in &mut program.paints {
        paint.enabled = true;
    }
    let mut next_id = 500;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let layers = lower(
        &mut collector,
        &program,
        "dual paint Rectangle",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .expect("one Rect preserves both simple paints");
    let FxLayer::Rect(rect) = &layers[0] else {
        panic!("editable FX Rect")
    };
    assert_eq!(rect.rect.size, [100.0, 50.0]);
    assert!(rect.rect.fill_enabled && rect.rect.stroke_enabled);
    assert_eq!(rect.rect.fill_color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(rect.rect.stroke_color, Some([0.0, 0.0, 1.0, 1.0]));
    for property in [
        PropType::PositionX,
        PropType::PositionY,
        PropType::StrokeMiterLimit,
    ] {
        let entry = collector
            .animations
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(rect.id, property))
            .expect("paired Rect keeps typed geometry and Miter keys");
        assert!(matches!(
            entry.animator.data(),
            fx_schema::animator::AnimatorData::Keyframes { .. }
        ));
        assert!(!entry.animator.is_js_script());
    }
    // A disabled containing group must not turn its paints into a visible Rect.
    let draws = std::mem::take(&mut program.scopes[0].draws);
    program.scopes[0]
        .draws
        .push(program::Draw::Group(program::ScopeId(1)));
    program.scopes.push(program::Scope {
        parent: Some(program::ScopeId(0)),
        enabled: false,
        operation: None,
        transform: None,
        draws,
        geometry: vec![program::GeometryId(0)],
    });
    program.geometry[0].owner = program::ScopeId(1);
    for paint in &mut program.paints {
        paint.owner = program::ScopeId(1);
    }
    assert!(dual_source(&program, &mut Vec::new()).is_none());
    // Consolidation: dual paints must also survive the separate Group route.
    let transform = vec![Chunk::list(
        *b"tdgp",
        [
            numeric("ADBE Vector Rotation", &[30.0]),
            keyed("ADBE Vector Scale"),
            numeric("ADBE Vector Group Opacity", &[50.0]),
        ]
        .into_iter()
        .flatten()
        .collect(),
    )];
    program.scopes[1].enabled = true;
    program.scopes[1].transform = Some(&transform);
    let grouped = lower(
        &mut collector,
        &program,
        "grouped dual paint",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .expect("paired paints retain their separate Group");
    let FxLayer::Group(group) = &grouped[0] else {
        panic!("separate Group Transform");
    };
    let FxLayer::Rect(child) = group.layers[0].data() else {
        panic!("editable dual-paint Rect");
    };
    assert_eq!(group.transform.rotation, 30.0);
    assert_eq!(group.transform.opacity.value(), 50.0);
    assert_eq!(child.parent, Some(group.id));
    assert_eq!(child.transform.opacity.value(), 100.0);
    assert_eq!(child.rect.fill_color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(child.rect.stroke_color, Some([0.0, 0.0, 1.0, 1.0]));
    for property in [PropType::ScaleX, PropType::ScaleY] {
        assert!(
            collector
                .animations
                .iter()
                .any(|entry| entry.target == PropertyTarget::layer(group.id, property))
        );
        assert!(
            collector
                .animations
                .iter()
                .all(|entry| entry.target != PropertyTarget::layer(child.id, property))
        );
    }
    for property in [
        PropType::PositionX,
        PropType::PositionY,
        PropType::StrokeMiterLimit,
    ] {
        assert!(
            collector
                .animations
                .iter()
                .any(|entry| entry.target == PropertyTarget::layer(child.id, property))
        );
        assert!(
            collector
                .animations
                .iter()
                .all(|entry| entry.target != PropertyTarget::layer(group.id, property))
        );
    }
    let mut layers = layers;
    layers.extend(grouped);
    let mut root =
        crate::structure_document::group(LayerId::new(1), "root".into(), None, full_active_range());
    root.layers = crate::structure_document::stored_layers(layers).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("dual-paint-rectangle"),
        "Dual Paint Rectangle",
        fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
        crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
    )
    .unwrap();
}

#[test]
fn animated_vector_group_preserves_native_rectangle_and_separate_targets() {
    // The Rectangle and Scale keys come from separate native fixtures. This is
    // supplemental combination evidence, not an Adobe-authored animated group.
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/geometry/geometry_probe.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(14).unwrap().kind else {
        panic!("probe composition")
    };
    let source = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 40)
        .unwrap();
    let (_, root) = crate::properties::root_runs(&source.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Root Vectors Group")
        .unwrap();
    let mut program =
        program::Program::parse(crate::properties::unique_list(root, *b"tdgp").unwrap(), 24);
    assert_eq!(program.scopes.len(), 2);
    let mut marker = b"ADBE Vector Scale".to_vec();
    marker.resize(40, 0);
    let mut scale = vec![Chunk::data(*b"tdmn", marker).unwrap()];
    scale.extend(animated_scale_run());
    let mut opacity_name = b"ADBE Vector Group Opacity".to_vec();
    opacity_name.resize(40, 0);
    let mut metadata = vec![0; 124];
    metadata[..2].copy_from_slice(&[0xdb, 0x99]);
    metadata[3] = 1;
    scale.push(Chunk::data(*b"tdmn", opacity_name).unwrap());
    scale.push(Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdb4", metadata).unwrap(),
            Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
            Chunk::data(*b"cdat", 50.0_f64.to_be_bytes().to_vec()).unwrap(),
        ],
    ));
    let transform = vec![Chunk::list(*b"tdgp", scale)];
    program.scopes[1].transform = Some(&transform);
    let mut next_id = 500;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let exhausted = lower(
        &mut collector,
        &program,
        "grouped Rectangle",
        LayerId::new(1),
        &mut OutputBudget::with_limit(0),
    )
    .unwrap()
    .expect("eligible native Rectangle");
    assert!(exhausted.is_empty());
    assert!(
        collector.animations.is_empty(),
        "budget rolls group keys back"
    );
    let layers = lower(
        &mut collector,
        &program,
        "grouped Rectangle",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .expect("existing FX Group and Rect retain both controls");
    let FxLayer::Group(group) = &layers[0] else {
        panic!("separate vector transform")
    };
    assert_eq!(group.parent, Some(LayerId::new(1)));
    let FxLayer::Rect(rect) = group.layers[0].data() else {
        panic!("editable parametric Rectangle, not a frozen Shape outline")
    };
    assert_eq!(rect.parent, Some(group.id));
    assert_eq!(rect.rect.size, [100.0, 50.0]);
    assert_eq!(rect.transform.anchor_point, [50.0, 25.0]);
    assert_eq!(rect.transform.scale, [100.0, 100.0]);
    assert_eq!(group.transform.opacity.value(), 50.0);
    assert_eq!(rect.transform.opacity.value(), 100.0);
    for property in [PropType::ScaleX, PropType::ScaleY] {
        let entry = collector
            .animations
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(group.id, property))
            .expect("typed group Scale keys");
        assert!(matches!(
            entry.animator.data(),
            fx_schema::animator::AnimatorData::Keyframes { .. }
        ));
        assert!(
            collector
                .animations
                .iter()
                .all(|entry| entry.target != PropertyTarget::layer(rect.id, property))
        );
    }
    assert!(
        collector
            .animations
            .iter()
            .all(|entry| !entry.animator.is_js_script())
    );
    let mut root =
        crate::structure_document::group(LayerId::new(1), "root".into(), None, full_active_range());
    root.layers = crate::structure_document::stored_layers(layers).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("grouped-rectangle"),
        "Grouped Rectangle",
        fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
        crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
    )
    .unwrap();
}

pub(super) fn animated_scale_run() -> Vec<Chunk> {
    fn find(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
        if let Ok(entries) = crate::properties::runs(chunks) {
            for (name, run) in entries {
                if name == "ADBE Scale"
                    && super::super::numeric_from_run(run)
                        .is_some_and(|value| !value.keyframes.is_empty())
                {
                    return Some(run.to_vec());
                }
            }
        }
        chunks.iter().filter_map(Chunk::children).find_map(find)
    }
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/property_scale.aep"
    ))
    .unwrap();
    project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) => {
                comp.layers.iter().find_map(|layer| find(&layer.content))
            }
            _ => None,
        })
        .expect("pinned native Scale keys")
}

#[test]
fn native_shapes_keep_identity_group_blend_boundaries() {
    fn named(name: &str, value: Chunk) -> Vec<Chunk> {
        let mut bytes = vec![0; 40];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        vec![Chunk::data(*b"tdmn", bytes).unwrap(), value]
    }
    fn scalar(value: f64) -> Chunk {
        let mut meta = vec![0; 124];
        meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
        Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", meta).unwrap(),
                Chunk::data(*b"tdsb", vec![0; 4]).unwrap(),
                Chunk::data(*b"cdat", value.to_be_bytes().to_vec()).unwrap(),
            ],
        )
    }
    // Synthetic programs isolate the optimization guard; not native Adobe proof.
    for source in ["ADBE Vector Shape - Ellipse", "ADBE Vector Shape - Star"] {
        for explicit_transform in [false, true] {
            for (ordinal, expected) in [
                (1.0, BlendMode::Normal),
                (4.0, BlendMode::Multiply),
                (99.0, BlendMode::Normal),
            ] {
                let source_leaves = if source == "ADBE Vector Shape - Ellipse" {
                    let storage = animated_scale_run()
                        .into_iter()
                        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
                        .unwrap();
                    named("ADBE Vector Ellipse Size", storage)
                } else {
                    Vec::new()
                };
                let mut contents = named(source, Chunk::list(*b"tdgp", source_leaves));
                contents.extend(named(
                    "ADBE Vector Graphic - Fill",
                    Chunk::list(*b"tdgp", named("ADBE Vector Fill Opacity", scalar(50.0))),
                ));
                let mut group = named("ADBE Vector Blend Mode", scalar(ordinal));
                group.extend(named("ADBE Vectors Group", Chunk::list(*b"tdgp", contents)));
                if explicit_transform {
                    group.extend(named(
                        "ADBE Vector Transform Group",
                        Chunk::list(*b"tdgp", Vec::new()),
                    ));
                }
                let root = named("ADBE Vector Group", Chunk::list(*b"tdgp", group));
                let program = program::Program::parse(&root, 8);
                assert_eq!(program.scopes.len(), 2);
                assert!(identity_group(Some(&program.scopes[1]), &mut Vec::new()));
                let mut next_id = 2;
                let mut animation_budget = AnimationBudget::default();
                let mut collector = Collector {
                    includes_occurrence_pipeline: true,
                    next_id: &mut next_id,
                    animation_budget: &mut animation_budget,
                    animations: Vec::new(),
                    warnings: Vec::new(),
                    frame_fade_lowered: false,
                };
                let layers = collector
                    .collect_contents_with_budget(
                        &root,
                        "blend probe",
                        LayerId::new(1),
                        8,
                        &mut OutputBudget::default(),
                    )
                    .unwrap();
                if expected == BlendMode::Multiply {
                    let group = layers
                        .iter()
                        .find_map(|layer| match layer {
                            FxLayer::Group(group) if !group.is_hidden => Some(group),
                            _ => None,
                        })
                        .expect("visible vector blend group must survive normalization");
                    assert_eq!(group.blend_mode, expected);
                    assert_eq!(group.transform.opacity.value(), 100.0);
                    assert!(
                        group.layers.iter().any(|layer| {
                            matches!(layer.data(), FxLayer::Shape(shape)
                                if !shape.is_hidden && shape.parent == Some(group.id)
                                    && shape.blend_mode == BlendMode::Normal
                                    && shape.transform.opacity.value() == 50.0
                                    && (shape.shape.ellipse.is_some() || shape.shape.poly_star.is_some()))
                        }),
                        "{:?}",
                        collector.warnings
                    );
                } else {
                    assert!(
                        layers
                            .iter()
                            .all(|layer| !matches!(layer, FxLayer::Group(_)))
                    );
                }
                if ordinal == 99.0 {
                    assert!(
                        collector
                            .warnings
                            .iter()
                            .any(|warning| { warning.contains("unrecognized shape blend mode") })
                    );
                }
                assert!(
                    collector
                        .animations
                        .iter()
                        .all(|entry| !entry.animator.is_js_script())
                );
                let animation_start = collector.animations.len();
                let omitted = super::super::native_shape::lower(
                    &mut collector,
                    &program,
                    "budget probe",
                    LayerId::new(1),
                    &mut OutputBudget::with_limit(0),
                )
                .unwrap()
                .unwrap();
                assert!(omitted.is_empty());
                assert_eq!(collector.animations.len(), animation_start);
                if source == "ADBE Vector Shape - Ellipse" {
                    assert!(collector.animations.iter().any(|entry| {
                        entry.target.as_property().is_some_and(|property| {
                            property.property_type() == PropType::EllipseSize
                        }) && entry.animator.keyframe_track().is_some()
                    }));
                }
                let mut root = crate::structure_document::group(
                    LayerId::new(1),
                    "root".into(),
                    None,
                    full_active_range(),
                );
                root.layers = crate::structure_document::stored_layers(layers).unwrap();
                fx_schema::FXComposition::try_from_parts(
                    fx_schema::CompositionId::new("shape-blend"),
                    "Shape blend",
                    fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
                    crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn native_rectangle_keys_drive_size_and_center_anchor_without_js() {
    // Supplemental native-record relabeling: the static Rectangle and the
    // animated Scale are separate pinned Adobe-native sources.
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/geometry/geometry_probe.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(14).unwrap().kind else {
        panic!("probe composition")
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 40)
        .unwrap();
    let (name, root) = crate::properties::root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Root Vectors Group")
        .unwrap();
    assert_eq!(name, "ADBE Root Vectors Group");
    let mut program =
        program::Program::parse(crate::properties::unique_list(root, *b"tdgp").unwrap(), 24);
    let GeometryKind::Source(source) = &program.geometry[0].kind else {
        panic!("expected native Rectangle source")
    };
    let mut rect_run = source.chunks.to_vec();
    let leaves = rect_run[0].children_mut().unwrap();
    let marker = b"ADBE Vector Rect Size";
    let offset = leaves
        .iter()
        .position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.windows(marker.len()).any(|window| window == marker))
        })
        .unwrap();
    let mut scale = animated_scale_run();
    scale.retain(|chunk| chunk.id() != *b"tdmn");
    assert_eq!(scale.len(), 1);
    leaves[offset + 1] = scale.pop().unwrap();
    program.geometry[0].kind = GeometryKind::Source(program::NativeRun {
        name: "ADBE Vector Shape - Rect",
        chunks: &rect_run,
    });
    let mut next_id = 500;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let layers = lower(
        &mut collector,
        &program,
        "animated Rectangle",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .expect("supported native FX Rect mapping");
    let FxLayer::Rect(rect) = &layers[0] else {
        panic!("native FX Rect")
    };
    for property in [
        PropType::RectSize,
        PropType::AnchorPointX,
        PropType::AnchorPointY,
    ] {
        let target = PropertyTarget::layer(rect.id, property);
        let entry = collector
            .animations
            .iter()
            .find(|entry| entry.target == target)
            .expect("native typed keyframe");
        assert!(!entry.animator.is_js_script());
    }
    let track = |property| {
        let entry = collector
            .animations
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(rect.id, property))
            .unwrap();
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
        else {
            panic!("Rectangle must have typed FX keyframes")
        };
        track
    };
    let size_keys = track(PropType::RectSize).keyframes();
    let x_keys = track(PropType::AnchorPointX).keyframes();
    let y_keys = track(PropType::AnchorPointY).keyframes();
    assert_eq!(size_keys.len(), 2);
    for ((size, x), y) in size_keys.iter().zip(x_keys).zip(y_keys) {
        let fx_schema::PropertyValue::Vector2([width, height]) = size.value() else {
            panic!("typed size")
        };
        let fx_schema::PropertyValue::Float(anchor_x) = x.value() else {
            panic!("typed x anchor")
        };
        let fx_schema::PropertyValue::Float(anchor_y) = y.value() else {
            panic!("typed y anchor")
        };
        assert_eq!(size.layer_time(), x.layer_time());
        assert_eq!(size.layer_time(), y.layer_time());
        assert!((anchor_x * 2.0 - width).abs() < 1e-9);
        assert!((anchor_y * 2.0 - height).abs() < 1e-9);
    }
    let mut root =
        crate::structure_document::group(LayerId::new(1), "root".into(), None, full_active_range());
    root.layers = crate::structure_document::stored_layers(layers).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("animated-rectangle"),
        "Animated Rectangle",
        fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
        crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires licensed AEP_INTRO_IMPORT_SOURCE; source cannot be redistributed"]
fn pinned_intro_rectangles_keep_zero_start_and_coupled_size_keys() {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(std::env::var_os("AEP_INTRO_IMPORT_SOURCE").expect("source path")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "75bb7d70238e23ffaafdeacf952217de1fcded8f86875bee39c2e91bda7804d9"
    );
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &project.item(3).unwrap().kind else {
        panic!("SH01 composition")
    };
    fn rect(layer: &FxLayer) -> Option<&fx_schema::RectLayer> {
        match layer {
            FxLayer::Rect(rect) => Some(rect),
            FxLayer::Group(group) => group.layers.iter().find_map(|child| rect(child.data())),
            _ => None,
        }
    }
    // Independent Adobe readback: keys at composition times 9+1/24,
    // 9+14/24 (and 10+13/24 for Dark_Masker). Both owners start at 9+1/24.
    for (native_id, expected) in [
        (2372, vec![(0, 0.0), (542, 138.0)]),
        (2375, vec![(0, 1600.0), (542, 900.0), (1500, 300.0)]),
    ] {
        let layer = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == native_id)
            .unwrap();
        let owner = crate::structure_document::group(
            LayerId::new(1),
            "owner".into(),
            None,
            full_active_range(),
        );
        let mut next_id = 2;
        let imported = super::super::import(
            layer,
            &owner,
            32,
            &mut next_id,
            &mut OutputBudget::default(),
            &mut AnimationBudget::default(),
        )
        .unwrap();
        let rect = imported.layers.iter().find_map(rect).unwrap_or_else(|| {
            panic!(
                "native {native_id} lost editable Rect: {:?}",
                imported.warnings
            )
        });
        for (property, factor) in [
            (PropType::RectSize, 1.0),
            (PropType::AnchorPointX, 0.5),
            (PropType::AnchorPointY, 0.5),
        ] {
            let entry = imported
                .animations
                .iter()
                .find(|entry| entry.target == PropertyTarget::layer(rect.id, property))
                .expect("editable native Size and coupled Anchor tracks");
            let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
            else {
                panic!("native controls must remain typed keyframes, not scripts")
            };
            assert_eq!(track.keyframes().len(), expected.len());
            for (key, &(time, value)) in track.keyframes().iter().zip(&expected) {
                assert_eq!(key.layer_time().as_millis(), time);
                match key.value() {
                    fx_schema::PropertyValue::Vector2(pair) => {
                        assert!(pair.iter().all(|v| (v - value).abs() < 1e-9))
                    }
                    fx_schema::PropertyValue::Float(actual) => {
                        assert!((actual - value * factor).abs() < 1e-9)
                    }
                    _ => panic!("wrong native Rectangle control type"),
                }
            }
        }
    }
}

#[test]
fn zero_start_animated_rectangle_size_is_valid_but_invalid_sizes_are_not() {
    let mut size = super::super::numeric_from_run(&animated_scale_run())
        .expect("pinned native animated numeric property");
    assert!(size.keyframes.len() >= 2);
    size.keyframes[0].values[..2].copy_from_slice(&[0.0, 0.0]);
    size.keyframes[1].values[..2].copy_from_slice(&[138.0, 138.0]);

    assert!(valid_rect_size_animation([0.0, 0.0], &size));

    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        let mut invalid_size = size.clone();
        invalid_size.keyframes[0].values[0] = invalid;
        assert!(!valid_rect_size_animation([invalid, 0.0], &invalid_size));
    }

    let mut all_zero = size;
    for key in &mut all_zero.keyframes {
        key.values[..2].copy_from_slice(&[0.0, 0.0]);
    }
    assert!(!valid_rect_size_animation([0.0, 0.0], &all_zero));
}

#[test]
fn size_components_are_constant_only_with_unchanged_zero_speed_keys() {
    use crate::properties::{NumericKeyframe, NumericProperty, NumericValueKind};
    // Decision cases for Slider-resolved anchor tracks; values are illustrative.
    fn key(time_secs: f64, values: [f64; 2], out_speed: [f64; 2]) -> NumericKeyframe {
        NumericKeyframe {
            time_secs,
            values: values.to_vec(),
            in_interpolation: 2,
            out_interpolation: 2,
            in_speed: vec![0.0; 2],
            in_influence: vec![33.0; 2],
            out_speed: out_speed.to_vec(),
            out_influence: vec![33.0; 2],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        }
    }
    fn size(keyframes: Vec<NumericKeyframe>) -> NumericProperty {
        NumericProperty {
            values: Vec::new(),
            animated: true,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes,
            value_kind: NumericValueKind::Continuous,
        }
    }
    let still = [0.0; 2];
    // Width moves; Height holds 50 with zero speed on every key.
    let one_axis = size(vec![
        key(0.0, [10.0, 50.0], still),
        key(1.0, [20.0, 50.0], still),
    ]);
    assert!(!constant_component(&one_axis, 0, 10.0));
    assert!(constant_component(&one_axis, 1, 50.0));
    // Equal Height values with a nonzero Bezier speed overshoot between them.
    let overshoot = size(vec![
        key(0.0, [10.0, 50.0], [0.0, 30.0]),
        key(1.0, [20.0, 50.0], still),
    ]);
    assert!(!constant_component(&overshoot, 1, 50.0));
    // Equal endpoints do not hide an interior Height excursion.
    let excursion = size(vec![
        key(0.0, [10.0, 50.0], still),
        key(0.5, [15.0, 70.0], still),
        key(1.0, [20.0, 50.0], still),
    ]);
    assert!(!constant_component(&excursion, 1, 50.0));
}

#[test]
fn native_gradient_fill_preserves_editable_rectangle_controls() {
    // Supplement: both records are independently Adobe-authored, but their
    // combination is not. It does not establish single-feature Adobe proof.
    fn gradient_fill_run(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
        if let Ok(runs) = crate::properties::runs(chunks) {
            for (name, run) in runs {
                if name == "ADBE Vector Graphic - G-Fill" {
                    return Some(run.to_vec());
                }
            }
        }
        chunks
            .iter()
            .filter_map(Chunk::children)
            .find_map(gradient_fill_run)
    }
    let gradients = read_project(include_bytes!(
        "../../../../tests/fixtures/shapes/gradient.aep"
    ))
    .unwrap();
    let ItemKind::Composition(gradient_comp) = &gradients.item(1).unwrap().kind else {
        panic!("pinned gradient composition")
    };
    let gradient_layer = gradient_comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 13)
        .expect("pinned gradient layer");
    let fill_run = gradient_fill_run(&gradient_layer.content)
        .expect("pinned ADBE Vector Graphic - G-Fill source property");
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/geometry/geometry_probe.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(14).unwrap().kind else {
        panic!("geometry composition")
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 40)
        .unwrap();
    let root = crate::properties::root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Root Vectors Group")
        .unwrap()
        .1;
    let mut program =
        program::Program::parse(crate::properties::unique_list(root, *b"tdgp").unwrap(), 24);
    assert_eq!(program.paints.len(), 1);
    program.paints[0].operation = program::NativeRun {
        name: "ADBE Vector Graphic - G-Fill",
        chunks: &fill_run,
    };
    let mut next_id = 500;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let layers = lower(
        &mut collector,
        &program,
        "gradient Rectangle",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .unwrap_or_else(|| {
        panic!(
            "one gradient fill has a native Rect mapping: {:?}",
            collector.warnings
        )
    });
    let FxLayer::Rect(rect) = &layers[0] else {
        panic!("editable FX Rect, not a static Shape contour")
    };
    assert_eq!(rect.rect.size, [100.0, 50.0]);
    assert!(rect.rect.fill_enabled);
    assert!(!rect.rect.stroke_enabled);
    let Some(ShapePaint::Gradient {
        gradient_type,
        start,
        end,
        stops,
    }) = &rect.rect.fill_paint
    else {
        panic!("editable gradient paint")
    };
    assert_eq!(*gradient_type, fx_schema::ShapeGradientType::Linear);
    assert_eq!(*start, [50.0, 25.0]);
    assert_eq!(*end, [150.0, 25.0]);
    assert!(stops.len() >= 2);
    assert!(stops.iter().all(|stop| {
        stop.offset.is_finite()
            && (0.0..=1.0).contains(&stop.offset)
            && stop.color.iter().all(|component| component.is_finite())
    }));
    assert!(
        stops
            .windows(2)
            .all(|pair| pair[0].offset <= pair[1].offset)
    );
    // A group Scale track does not require animating Rect-local gradient axes.
    assert_eq!(program.scopes.len(), 2);
    let mut marker = b"ADBE Vector Scale".to_vec();
    marker.resize(40, 0);
    let mut scale = vec![Chunk::data(*b"tdmn", marker).unwrap()];
    scale.extend(animated_scale_run());
    let transform = vec![Chunk::list(*b"tdgp", scale)];
    program.scopes[1].transform = Some(&transform);
    let grouped = lower(
        &mut collector,
        &program,
        "animated group gradient",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .expect("static Rect gradient inside an animated Group");
    let FxLayer::Group(group) = &grouped[0] else {
        panic!("separate animated Group");
    };
    let FxLayer::Rect(child) = group.layers[0].data() else {
        panic!("editable gradient Rect");
    };
    assert_eq!(child.parent, Some(group.id));
    assert_eq!(child.rect.fill_paint, rect.rect.fill_paint);
    for property in [PropType::ScaleX, PropType::ScaleY] {
        assert!(
            collector
                .animations
                .iter()
                .any(|entry| { entry.target == PropertyTarget::layer(group.id, property) })
        );
    }
    let mut layers = layers;
    layers.extend(grouped);
    assert!(
        collector
            .animations
            .iter()
            .all(|entry| !entry.animator.is_js_script())
    );
    let mut root = crate::structure_document::group(
        LayerId::new(1),
        "gradient root".into(),
        None,
        full_active_range(),
    );
    root.layers = crate::structure_document::stored_layers(layers).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("gradient-rectangle"),
        "Gradient Rectangle",
        fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
        crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
    )
    .unwrap();
}
