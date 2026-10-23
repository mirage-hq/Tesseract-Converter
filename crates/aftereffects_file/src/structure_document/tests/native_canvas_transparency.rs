use super::*;

// Independent Adobe author/save/control readback; these are import structure tests,
// not native render or alpha-fidelity proof.
fn native_canvas() -> (StructuralProject, Value) {
    let bytes = include_bytes!("../../../tests/fixtures/canvas/transparency.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "e43aa8b1c9b296adda1d35670d166eab89855ce3c4d2255061e13924c0c1202f"
    );
    let readback = serde_json::from_str(include_str!(
        "../../../tests/fixtures/canvas/transparency.readback.json"
    ))
    .unwrap();
    (read_project(bytes).unwrap(), readback)
}

fn assert_canvas(source: &Composition, converted: &StructuralConversion, evidence: &Value) {
    assert_eq!((source.width, source.height), (160, 90));
    assert_eq!(source.duration_secs, 1.0);
    assert_eq!(source.frame_rate, 24.0);
    assert_eq!(evidence["width"], 160);
    assert_eq!(evidence["height"], 90);
    assert_eq!(evidence["duration"], 1);
    assert_eq!(evidence["fps"], 24);
    for (actual, expected) in source.record.background_color()[..3]
        .iter()
        .zip(evidence["preview_background"].as_array().unwrap())
    {
        assert!((actual - expected.as_f64().unwrap()).abs() < 1e-6);
    }
    assert_eq!(source.record.background_color()[3], 1.0);
    assert_imported_canvas_matches_source(source, converted, "native transparent canvas");
    let json = String::from_utf8(converted.document.to_json_vec().unwrap()).unwrap();
    assert!(
        !json.contains("JsScript"),
        "canvas policy must not author scripts"
    );
    assert_eq!(converted.document.background_color(), None);
}

fn assert_solid(
    project: &StructuralProject,
    source: &Layer,
    occurrence: &GroupLayer,
    evidence: &Value,
    size: [f64; 2],
    color: [f64; 3],
) {
    assert_eq!(u64::from(source.record.id()), evidence["id"]);
    assert_eq!(u64::from(source.record.source_id()), evidence["source_id"]);
    let solid = project
        .item(source.record.source_id())
        .unwrap()
        .solid
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!([f64::from(solid.width), f64::from(solid.height)], size);
    for (actual, expected) in solid.color.iter().zip(color) {
        assert!((f64::from(*actual) - expected).abs() < 1e-6);
    }
    assert_eq!(evidence["width"].as_f64().unwrap(), size[0]);
    assert_eq!(evidence["height"].as_f64().unwrap(), size[1]);
    assert_eq!(evidence["position"], serde_json::json!([80, 45, 0]));
    for (actual, expected) in evidence["color"].as_array().unwrap().iter().zip(color) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-6);
    }
    assert_eq!(occurrence.name, evidence["name"].as_str().unwrap());
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([80.0, 45.0])
    );
    assert_eq!(
        occurrence.transform.anchor_point,
        [size[0] / 2.0, size[1] / 2.0]
    );
    assert_eq!(occurrence.layers.len(), 1);
    let content = as_group(&occurrence.layers[0]);
    assert_eq!(content.layers.len(), 1);
    let FxLayer::Rect(rect) = content.layers[0].data() else {
        panic!("authored SolidSource must remain an editable rectangle")
    };
    assert_eq!(rect.rect.size, size);
    assert!(rect.rect.fill_enabled);
    assert!(!rect.rect.stroke_enabled);
    for (actual, expected) in rect
        .rect
        .fill_color
        .value()
        .unwrap()
        .iter()
        .zip([color[0], color[1], color[2], 1.0])
    {
        assert!((actual - expected).abs() < 1e-6);
    }
}

#[test]
fn native_canvas_preview_background_does_not_replace_transparent_pixels() {
    let (project, readback) = native_canvas();
    let evidence = &readback["compositions"][0];
    assert_eq!(evidence["id"], 1);
    assert_eq!(
        evidence["preview_background"],
        serde_json::json!([0.2, 0.4, 0.6])
    );
    let source = composition(&project, 1);
    assert_eq!(source.layers.len(), 1);
    assert_eq!(source.layers[0].record.id(), 15);
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    assert_eq!(root(&converted).name, "CanvasTransparent");
    assert_eq!(root(&converted).layers.len(), 1);
    assert_solid(
        &project,
        &source.layers[0],
        as_group(&root(&converted).layers[0]),
        &evidence["layers"][0],
        [40.0, 20.0],
        [1.0, 0.2, 0.1],
    );
    assert_canvas(source, &converted, evidence);
}

#[test]
fn native_canvas_authored_background_remains_editable_below_foreground() {
    let (project, readback) = native_canvas();
    let evidence = &readback["compositions"][1];
    assert_eq!(evidence["id"], 16);
    let source = composition(&project, 16);
    assert_eq!(source.layers.len(), 2);
    assert_eq!(source.layers[0].record.id(), 31);
    assert_eq!(source.layers[1].record.id(), 29);
    assert_eq!(evidence["layers"][0]["index"], 1);
    assert_eq!(evidence["layers"][1]["index"], 2);
    let converted = to_structural_fx_document(&project, Some(16)).unwrap();
    assert_eq!(root(&converted).name, "CanvasAuthoredBackground");
    assert_eq!(root(&converted).layers.len(), 2);
    assert_solid(
        &project,
        &source.layers[0],
        as_group(&root(&converted).layers[0]),
        &evidence["layers"][0],
        [40.0, 20.0],
        [1.0, 0.2, 0.1],
    );
    assert_solid(
        &project,
        &source.layers[1],
        as_group(&root(&converted).layers[1]),
        &evidence["layers"][1],
        [160.0, 90.0],
        [0.1, 0.3, 0.5],
    );
    assert_canvas(source, &converted, evidence);
}

#[test]
fn native_canvas_nested_transparent_child_keeps_same_alpha_policy() {
    let (project, readback) = native_canvas();
    let evidence = &readback["compositions"][2];
    assert_eq!(evidence["id"], 32);
    let source = composition(&project, 32);
    assert_eq!(source.layers.len(), 1);
    assert_eq!(source.layers[0].record.id(), 44);
    assert_eq!(source.layers[0].record.source_id(), 1);
    assert_eq!(evidence["layers"][0]["source_id"], 1);
    assert!(evidence["layers"][0]["color"].is_null());
    let converted = to_structural_fx_document(&project, Some(32)).unwrap();
    assert_eq!(root(&converted).name, "CanvasNestedTransparent");
    assert_eq!(root(&converted).layers.len(), 1);
    let occurrence = as_group(&root(&converted).layers[0]);
    assert_eq!(occurrence.name, "Authored precomposition");
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([80.0, 45.0])
    );
    assert_eq!(occurrence.layers.len(), 1);
    let child = as_group(&occurrence.layers[0]);
    assert_eq!(child.name, "Source content clock");
    assert_eq!(
        child.layers.len(),
        2,
        "authored paint plus mask-only canvas guide, not preview background"
    );
    let child_source = composition(&project, 1);
    assert_solid(
        &project,
        &child_source.layers[0],
        as_group(&child.layers[0]),
        &readback["compositions"][0]["layers"][0],
        [40.0, 20.0],
        [1.0, 0.2, 0.1],
    );
    let child_converted = to_structural_fx_document(&project, Some(1)).unwrap();
    assert_canvas(child_source, &child_converted, &readback["compositions"][0]);
    assert_canvas(source, &converted, evidence);
}

#[test]
fn normal_raster_precomposition_retains_its_native_canvas() {
    let (mut project, readback) = native_canvas();
    let evidence = &readback["compositions"][0];
    assert_eq!(evidence["width"], 160);
    assert_eq!(evidence["height"], 90);
    composition_mut(&mut project, 32).width = 320;
    composition_mut(&mut project, 32).height = 180;
    let converted = to_structural_fx_document(&project, Some(32)).unwrap();
    let owner = as_group(&root(&converted).layers[0]);
    let content = as_group(&owner.layers[0]);
    assert_eq!(
        content.masks.len(),
        1,
        "raster source canvas must clip its content"
    );
    let mask = &content.masks[0];
    assert_eq!(mask.mode, fx_composition::layer::MaskMode::Add);
    assert!(!mask.inverted);
    assert_eq!(mask.feather, [0.0, 0.0]);
    assert_eq!(mask.expansion, 0.0);
    assert_eq!(mask.opacity.value(), 1.0);
    let guide = content
        .layers
        .iter()
        .find(|layer| Some(layer.id()) == mask.layer)
        .unwrap();
    let FxLayer::Rect(guide) = guide.data() else {
        panic!("editable source-canvas rectangle")
    };
    assert_eq!(guide.rect.position, [0.0, 0.0]);
    assert_eq!(guide.rect.size, [160.0, 90.0]);
    assert_eq!(guide.parent, Some(content.id));
    assert_eq!(guide.active_range.start.as_millis(), 0);
    assert_eq!(guide.active_range.duration.as_millis(), 1000);
    assert_eq!(guide.transform.anchor_point, [0.0, 0.0]);
    assert_eq!(
        guide.transform.position,
        fx_composition::Position::TwoD([0.0, 0.0])
    );
    assert!(
        owner.masks.is_empty(),
        "clip belongs before owner geometry/effects"
    );
    assert_solid(
        &project,
        &composition(&project, 1).layers[0],
        as_group(&content.layers[0]),
        &evidence["layers"][0],
        [40.0, 20.0],
        [1.0, 0.2, 0.1],
    );

    let source = &mut composition_mut(&mut project, 32).layers[0];
    source.record = source
        .record
        .clone()
        .with_collapse_transformations(true)
        .unwrap();
    let collapsed = to_structural_fx_document(&project, Some(32)).unwrap();
    let owner = as_group(&root(&collapsed).layers[0]);
    assert!(as_group(&owner.layers[0]).masks.is_empty());
}

#[test]
fn blended_compositions_keep_an_editable_transparent_canvas_boundary() {
    let (mut project, _) = native_canvas();
    composition_mut(&mut project, 32).width = 320;
    composition_mut(&mut project, 32).height = 180;
    let normal = to_structural_fx_document(&project, Some(32)).unwrap();
    let normal_owner = as_group(&root(&normal).layers[0]);
    let normal_clock = as_group(&normal_owner.layers[0]);
    assert_eq!(normal_clock.masks.len(), 1);
    let source = composition_mut(&mut project, 1);
    let mut bytes = source.layers[0].record.encode();
    bytes[99] = 5;
    source.layers[0].record = LayerRecord::decode(&bytes).unwrap();

    fn assert_boundary(group: &GroupLayer) {
        assert_eq!(
            group.masks.len(),
            1,
            "native composition isolation is missing"
        );
        let mask = &group.masks[0];
        assert_eq!(mask.mode, fx_composition::layer::MaskMode::Add);
        assert!(!mask.inverted);
        assert_eq!(mask.feather, [0.0, 0.0]);
        assert_eq!(mask.expansion, 0.0);
        assert_eq!(mask.opacity.value(), 1.0);
        assert!(mask.legacy_path.is_none());
        let guide = group
            .layers
            .iter()
            .find(|layer| Some(layer.id()) == mask.layer)
            .unwrap();
        let FxLayer::Rect(guide) = guide.data() else {
            panic!("editable canvas rectangle")
        };
        assert_eq!(guide.parent, Some(group.id));
        assert_eq!(guide.rect.position, [0.0, 0.0]);
        assert_eq!(guide.rect.size, [160.0, 90.0]);
        assert_eq!(
            guide.transform.position,
            fx_composition::Position::TwoD([0.0, 0.0])
        );
        assert_eq!(
            as_group(&group.layers[0]).blend_mode,
            fx_composition::BlendMode::Multiply
        );
    }

    let standalone = to_structural_fx_document(&project, Some(1)).unwrap();
    assert_boundary(root(&standalone));
    assert_eq!(standalone.document.background_color(), None);

    let nested = to_structural_fx_document(&project, Some(32)).unwrap();
    assert!(root(&nested).masks.is_empty());
    let owner = as_group(&root(&nested).layers[0]);
    assert!(owner.masks.is_empty());
    assert_eq!(owner.transform, normal_owner.transform);
    let content = as_group(&owner.layers[0]);
    assert_boundary(content);
    assert_eq!(content.playback, normal_clock.playback);
    assert_eq!(content.name, "Source content clock");
    assert!(content.effects.is_empty());

    let owner = &mut composition_mut(&mut project, 32).layers[0];
    owner.record = owner
        .record
        .clone()
        .with_collapse_transformations(true)
        .unwrap();
    let collapsed = to_structural_fx_document(&project, Some(32)).unwrap();
    let owner = as_group(&root(&collapsed).layers[0]);
    assert!(as_group(&owner.layers[0]).masks.is_empty());
}

#[test]
fn blend_canvas_declines_nonplanar_and_invalid_canvases_without_losing_children() {
    for profile in ["3d", "nonsquare", "empty"] {
        let (mut project, _) = native_canvas();
        let comp = composition_mut(&mut project, 1);
        let mut bytes = comp.layers[0].record.encode();
        bytes[99] = 5;
        comp.layers[0].record = LayerRecord::decode(&bytes).unwrap();
        match profile {
            "3d" => {
                comp.layers[0].record = comp.layers[0]
                    .record
                    .clone()
                    .with_three_d_layer(true)
                    .unwrap();
            }
            "nonsquare" => comp.pixel_aspect = (2, 1),
            "empty" => comp.width = 0,
            _ => unreachable!(),
        }
        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        assert!(root(&converted).masks.is_empty(), "{profile}");
        assert_eq!(root(&converted).layers.len(), 1, "{profile}");
        assert_eq!(
            as_group(&root(&converted).layers[0]).blend_mode,
            fx_composition::BlendMode::Multiply
        );
        assert!(
            converted.diagnostics.iter().any(|diagnostic| {
                diagnostic.limitation == Limitation::GroupBounds
                    && diagnostic
                        .message
                        .contains("blend-canvas isolation omitted")
            }),
            "{profile}"
        );
    }
}
