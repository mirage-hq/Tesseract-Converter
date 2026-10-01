// Independent Adobe-authored inputs; no writer output is used as an oracle.
use super::*;

fn assert_pinned_canvas(source: &Composition, converted: &StructuralConversion, evidence: &Value) {
    assert_eq!(u64::from(source.width), evidence["width"].as_u64().unwrap());
    assert_eq!(
        u64::from(source.height),
        evidence["height"].as_u64().unwrap()
    );
    assert_imported_canvas_matches_source(source, converted, "pinned Adobe source");
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_1080p_solid_proof_has_pinned_editable_pixels() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/solid_color_1080.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/solid_color_1080.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/solid_color_1080.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(u64::from(comp.width), evidence["width"].as_u64().unwrap());
            assert_eq!(u64::from(comp.height), evidence["height"].as_u64().unwrap());
            assert_eq!(comp.frame_rate, evidence["fps"].as_f64().unwrap());
            assert_eq!(comp.layers.len(), 1);
            assert_eq!(
                u64::from(comp.layers[0].record.id()),
                evidence["layer_id"].as_u64().unwrap()
            );

            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_SOLID");
            assert_eq!(root.layers.len(), 1);
            let occurrence = as_group(&root.layers[0]);
            assert_eq!(occurrence.name, "solid_color");
            assert_eq!(
                occurrence.transform.position,
                fx_composition::Position::TwoD([960.0, 540.0])
            );
            assert_eq!(occurrence.transform.anchor_point, [960.0, 540.0]);
            assert_eq!(occurrence.layers.len(), 1);
            let content = as_group(&occurrence.layers[0]);
            assert_eq!(content.layers.len(), 1);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("AE solid must remain an editable Rect, not flattened media")
            };
            assert_eq!(rect.rect.size, [1920.0, 1080.0]);
            assert_eq!(rect.rect.fill_color, [0.75, 0.125, 0.25, 1.0]);
            assert!(rect.rect.fill_enabled);
            assert!(!rect.rect.stroke_enabled);
            let json = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&json).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_static_position_imports_editable_2d_transform() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/static_position.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/static_position.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/static_position.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert!(!comp.layers[0].record.flags().three_d_layer);
            assert_eq!(
                u64::from(comp.layers[0].record.id()),
                evidence["layer_id"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"].as_u64().unwrap()
            );

            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_POSITION");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "position_target");
            let authored_position = [
                evidence["position"][0].as_f64().unwrap(),
                evidence["position"][1].as_f64().unwrap(),
            ];
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD(authored_position)
            );
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("position target must retain editable solid pixels")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.75, 0.125, 0.25, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let json = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&json).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_linear_opacity_imports_editable_keyframes() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/linear_opacity.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/linear_opacity.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/linear_opacity.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            assert_eq!(
                evidence["opacity_keys"],
                serde_json::json!([[0.5, 0], [1.5, 100]])
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(
                u64::from(comp.layers[0].record.id()),
                evidence["layer_id"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"].as_u64().unwrap()
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_OPACITY");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "opacity_target");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(foreground.transform.opacity.value(), 100.0);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("opacity target must retain editable solid pixels")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.75, 0.125, 0.25, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");

            let editable = converted.document.to_json_vec().unwrap();
            let json: Value = serde_json::from_slice(&editable).unwrap();
            let target_id = &json["composition"]["layers"][0]["layers"][0]["id"];
            let entries = json["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap();
            assert_eq!(entries.len(), 1);
            let entry = &entries[0];
            assert_eq!(entry["target"]["layerId"], *target_id);
            assert_eq!(entry["target"]["propertyType"], "opacity");
            let keys = &entry["animator"]["keyframes"];
            assert_eq!(keys.as_array().unwrap().len(), 2);
            for (index, (time, value)) in [(500, 0.0), (1500, 100.0)].into_iter().enumerate() {
                assert_eq!(keys[index]["layerTime"], time);
                assert_eq!(keys[index]["value"]["type"], "float");
                assert_eq!(keys[index]["value"]["value"], value);
                assert_eq!(keys[index]["easing"]["type"], "linear");
            }
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn distinct_adobe_opacity_source_imports_editable_linear_keys() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_linear_opacity.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_linear_opacity.aep");
            let source: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_linear_opacity.ae.json"
            ))
            .unwrap();
            assert_eq!(bytes.len() as u64, source["source_bytes"].as_u64().unwrap());
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                source["source_sha256"].as_str().unwrap()
            );
            assert_eq!(
                source["opacity_keys"],
                serde_json::json!([[0.5, 0], [1.5, 100]])
            );
            let comp_id = u32::try_from(source["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), source["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                source["background_layer_id"]
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &source);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_OPACITY_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "opacity_only_source");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(foreground.transform.opacity.value(), 100.0);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("new Adobe opacity source must remain an editable solid")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.1875, 0.625, 0.9375, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            let json: Value = serde_json::from_slice(&editable).unwrap();
            let entry = &json["composition"]["dynamics"]["entries"][0];
            assert_eq!(
                entry["target"]["layerId"],
                json["composition"]["layers"][0]["layers"][0]["id"]
            );
            assert_eq!(entry["target"]["propertyType"], "opacity");
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2);
            for (index, (time, value)) in [(500, 0.0), (1500, 100.0)].into_iter().enumerate() {
                assert_eq!(keys[index]["layerTime"], time);
                assert_eq!(keys[index]["value"]["type"], "float");
                assert_eq!(keys[index]["value"]["value"], value);
                assert_eq!(keys[index]["easing"]["type"], "linear");
            }
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_shape_fill_imports_editable_paint_and_geometry() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/shape_fill.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/shape_fill.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/shape_fill.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            assert_eq!(evidence["rectangle_size"], serde_json::json!([480, 270]));
            assert_eq!(
                evidence["fill_rgba"],
                serde_json::json!([0.75, 0.125, 0.25, 1])
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(
                u64::from(comp.layers[0].record.id()),
                evidence["layer_id"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"].as_u64().unwrap()
            );

            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_SHAPE_FILL");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "shape_fill_target");
            let source = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = source.layers[0].data() else {
                panic!("native Rectangle and Fill must retain editable Rect controls")
            };
            assert!(!rect.is_hidden);
            assert_eq!(rect.parent, Some(source.id));
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.position, [0.0, 0.0]);
            assert_eq!(rect.rect.roundness, 0.0);
            assert_eq!(rect.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(
                rect.transform.position,
                fx_composition::Position::TwoD([0.0, 0.0])
            );
            assert!(rect.rect.fill_enabled);
            assert!(!rect.rect.stroke_enabled);
            assert_eq!(rect.rect.fill_color, [0.75, 0.125, 0.25, 1.0]);
            assert_eq!(rect.transform.opacity.value(), 100.0);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn distinct_adobe_shape_fill_source_imports_editable_paint() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_shape_fill.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_shape_fill.aep");
            let source: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_shape_fill.ae.json"
            ))
            .unwrap();
            assert_eq!(bytes.len() as u64, source["source_bytes"].as_u64().unwrap());
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                source["source_sha256"].as_str().unwrap()
            );
            assert_eq!(source["rectangle_size"], serde_json::json!([360, 240]));
            assert_eq!(
                source["fill_rgba"],
                serde_json::json!([0.125, 0.6875, 0.875, 1])
            );
            let comp_id = u32::try_from(source["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), source["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                source["background_layer_id"]
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &source);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_SHAPE_FILL_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "export_shape_fill_target");
            let source_content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = source_content.layers[0].data() else {
                panic!("distinct Adobe Rectangle and Fill must remain editable Rect controls")
            };
            assert!(!rect.is_hidden);
            assert_eq!(rect.parent, Some(source_content.id));
            assert_eq!(rect.rect.size, [360.0, 240.0]);
            assert_eq!(rect.rect.position, [0.0, 0.0]);
            assert_eq!(rect.rect.roundness, 0.0);
            assert_eq!(rect.transform.anchor_point, [180.0, 120.0]);
            assert_eq!(
                rect.transform.position,
                fx_composition::Position::TwoD([0.0, 0.0])
            );
            assert!(rect.rect.fill_enabled);
            assert!(!rect.rect.stroke_enabled);
            assert_eq!(rect.rect.fill_color, [0.125, 0.6875, 0.875, 1.0]);
            assert_eq!(rect.transform.opacity.value(), 100.0);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_nonuniform_scale_imports_editable_2d_transform() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/static_scale.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/static_scale.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/static_scale.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            assert_eq!(evidence["scale"], serde_json::json!([150, 50, 100]));
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(
                u64::from(comp.layers[0].record.id()),
                evidence["layer_id"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"].as_u64().unwrap()
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_SCALE");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "scale_target");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([960.0, 540.0])
            );
            assert_eq!(foreground.transform.scale, [150.0, 50.0]);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("scaled solid must retain editable source pixels")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.75, 0.125, 0.25, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_combined_static_transform_imports_editable_controls() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/combined_transform.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/combined_transform.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/combined_transform.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "EDITED_FX_CYAN_SOURCE");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([1040.0, 480.0])
            );
            assert_eq!(foreground.transform.scale, [125.0, 80.0]);
            assert_eq!(foreground.transform.rotation, 11.0);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("combined solid must retain editable source pixels")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.1875, 0.625, 0.9375, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_isolated_position_source_has_editable_import_semantics() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_static_position.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_static_position.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_static_position.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_POSITION_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "position_only_source");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([1072.0, 464.0])
            );
            assert_eq!(foreground.transform.scale, [100.0, 100.0]);
            assert_eq!(foreground.transform.rotation, 0.0);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("isolated Position source must remain an editable solid")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.1875, 0.625, 0.9375, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn distinct_adobe_color_and_source_dimensions_import_as_editable_solid() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_static_color.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_static_color.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_static_color.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            assert_eq!(evidence["foreground_size"], serde_json::json!([448, 192]));
            assert_eq!(
                evidence["foreground_rgb"],
                serde_json::json!([0.875, 0.3125, 0.0625])
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            let source_id = comp.layers[0].record.source_id();
            let source = project
                .item(source_id)
                .unwrap()
                .solid
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            assert_eq!((source.width, source.height), (448, 192));
            assert_eq!(source.color, [0.875, 0.3125, 0.0625]);
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_COLOR_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "color_only_source");
            assert_eq!(foreground.transform.anchor_point, [224.0, 96.0]);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([960.0, 540.0])
            );
            assert_eq!(foreground.transform.scale, [100.0, 100.0]);
            assert_eq!(foreground.transform.rotation, 0.0);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("native color/size-only solid must remain editable, not flattened media")
            };
            assert_eq!(rect.rect.size, [448.0, 192.0]);
            assert_eq!(rect.rect.fill_color, [0.875, 0.3125, 0.0625, 1.0]);
            assert!(rect.rect.fill_enabled);
            assert!(!rect.rect.stroke_enabled);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn distinct_adobe_static_orientation_is_imported_as_editable_3d_not_z_rotation() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_static_orientation.aep",
        1,
        || {
            let bytes =
                include_bytes!("../../../tests/fixtures/render/export_static_orientation.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_static_orientation.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            assert_eq!(evidence["three_d"], true);
            assert_eq!(evidence["orientation"], serde_json::json!([0, 0, 22]));
            assert_eq!(evidence["rotation"], 0);
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert!(comp.layers[0].record.flags().three_d_layer);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_ORIENTATION_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "orientation_only_source");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::ThreeD([960.0, 540.0, 0.0])
            );
            assert_eq!(foreground.transform.scale, [100.0, 100.0]);
            assert_eq!(foreground.transform.rotation, 0.0);
            assert_eq!(foreground.transform.rotation_x, 0.0);
            assert_eq!(foreground.transform.rotation_y, 0.0);
            assert_eq!(foreground.transform.orientation, [0.0, 0.0, 22.0]);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("AE 3D orientation solid must remain editable, not flattened media")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.75, 0.375, 0.125, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn independently_adobe_authored_add_blend_over_gray_is_editable_not_flattened() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_add_blend.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_add_blend.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_add_blend.ae.json"
            ))
            .unwrap();
            assert_eq!(bytes.len() as u64, evidence["source_bytes"]);
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"]
            );
            assert_eq!(evidence["foreground_is_add"], true);
            assert_eq!(evidence["background_is_normal"], true);
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            assert!(matches!(comp.layers[0].record.blend_mode(), 4 | 29));
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(
                (root.name.as_str(), root.layers.len()),
                ("AEP_PROOF_ADD_BLEND_EXPORT", 2)
            );
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "add_blend_source");
            assert_eq!(foreground.blend_mode, fx_composition::BlendMode::Add);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([960.0, 540.0])
            );
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("Adobe Add foreground must remain an editable Rect")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.5, 0.125, 0.125, 1.0]);
            let background = as_group(&root.layers[1]);
            assert_eq!(background.name, "gray_backdrop");
            assert_eq!(background.blend_mode, fx_composition::BlendMode::Normal);
            let gray = as_group(&background.layers[0]);
            let FxLayer::Rect(rect) = gray.layers[0].data() else {
                panic!("gray sibling must remain editable, not flattened media")
            };
            assert_eq!(rect.rect.fill_color, [0.25, 0.25, 0.25, 1.0]);
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn independently_adobe_authored_eye_off_keeps_editable_hidden_foreground() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_eye_off.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_eye_off.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_eye_off.ae.json"
            ))
            .unwrap();
            assert_eq!(bytes.len() as u64, evidence["source_bytes"]);
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"]
            );
            assert_eq!(evidence["foreground_enabled"], false);
            assert_eq!(evidence["background_enabled"], true);
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            assert!(!comp.layers[0].record.flags().enabled);
            assert!(comp.layers[1].record.flags().enabled);
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(
                (root.name.as_str(), root.layers.len()),
                ("AEP_PROOF_EYE_OFF_EXPORT", 2)
            );
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "eye_off_source");
            assert!(
                foreground.is_hidden,
                "Eye-off must retain the editable hidden group"
            );
            assert_eq!(foreground.blend_mode, fx_composition::BlendMode::Normal);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("disabled Adobe Solid must remain an editable Rect")
            };
            assert!(rect.is_hidden, "the visual child must also be hidden");
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.5, 0.125, 0.125, 1.0]);
            let background = as_group(&root.layers[1]);
            assert_eq!(background.name, "gray_backdrop");
            assert!(!background.is_hidden);
            let gray = as_group(&background.layers[0]);
            let FxLayer::Rect(rect) = gray.layers[0].data() else {
                panic!("gray sibling must remain editable and visible")
            };
            assert!(!rect.is_hidden);
            assert_eq!(rect.rect.fill_color, [0.25, 0.25, 0.25, 1.0]);
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn independently_adobe_authored_start_in_out_stretch_retains_editable_content_clock() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_layer_timing.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_layer_timing.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_layer_timing.ae.json"
            ))
            .unwrap();
            assert_eq!(bytes.len() as u64, evidence["source_bytes"]);
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"]
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            let record = &comp.layers[0].record;
            assert_eq!(u64::from(record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            for (actual, key) in [
                (record.start_time().unwrap(), "start_time"),
                (record.stretch().unwrap() * 100.0, "stretch"),
                (
                    record.start_time().unwrap()
                        + record.in_point().unwrap() * record.stretch().unwrap(),
                    "in_point",
                ),
                (
                    record.start_time().unwrap()
                        + record.out_point().unwrap() * record.stretch().unwrap(),
                    "out_point",
                ),
            ] {
                let expected = evidence[key].as_f64().unwrap();
                assert!(
                    (actual - expected).abs() < 0.0001,
                    "{key}: native {actual} vs Adobe {expected}"
                );
            }
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_TIMING_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "timed_solid_source");
            assert_eq!(foreground.playback.input_range().start, Time::ZERO);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([960.0, 540.0])
            );
            let content = as_group(&foreground.layers[0]);
            assert_eq!(content.playback.input_range().start.as_secs(), 0.5);
            assert_eq!(content.playback.input_range().end().as_secs(), 1.7);
            let playback = content
                .playback
                .time_remap()
                .expect("authored AE stretch/start must retain an editable source clock");
            let keys = playback.keyframes();
            assert_eq!(
                (keys[0].time.as_secs(), keys[0].value.as_secs()),
                (0.5, 0.25)
            );
            assert_eq!(
                (keys[1].time.as_secs(), keys[1].value.as_secs()),
                (1.7, 1.75)
            );
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("timed solid must remain an editable Rect, not flattened media")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.25, 0.625, 0.875, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_isolated_scale_source_has_editable_import_semantics() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_static_scale.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_static_scale.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_static_scale.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_SCALE_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "scale_only_source");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([960.0, 540.0])
            );
            assert_eq!(foreground.transform.scale, [135.0, 70.0]);
            assert_eq!(foreground.transform.rotation, 0.0);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("isolated Scale source must remain an editable solid")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.1875, 0.625, 0.9375, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ae_authored_isolated_rotation_source_has_editable_import_semantics() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/render/export_static_rotation.aep",
        1,
        || {
            let bytes = include_bytes!("../../../tests/fixtures/render/export_static_rotation.aep");
            let evidence: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/render/export_static_rotation.ae.json"
            ))
            .unwrap();
            assert_eq!(
                bytes.len() as u64,
                evidence["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                evidence["source_sha256"].as_str().unwrap()
            );
            let comp_id = u32::try_from(evidence["composition_id"].as_u64().unwrap()).unwrap();
            let project = read_project(bytes).unwrap();
            let comp = composition(&project, comp_id);
            assert_eq!(comp.layers.len(), 2);
            assert_eq!(u64::from(comp.layers[0].record.id()), evidence["layer_id"]);
            assert_eq!(
                u64::from(comp.layers[1].record.id()),
                evidence["background_layer_id"]
            );
            let converted = to_structural_fx_document(&project, Some(comp_id)).unwrap();
            assert_pinned_canvas(comp, &converted, &evidence);
            let root = root(&converted);
            assert_eq!(root.name, "AEP_PROOF_ROTATION_EXPORT");
            assert_eq!(root.layers.len(), 2);
            let foreground = as_group(&root.layers[0]);
            assert_eq!(foreground.name, "rotation_only_source");
            assert_eq!(foreground.transform.anchor_point, [240.0, 135.0]);
            assert_eq!(
                foreground.transform.position,
                fx_composition::Position::TwoD([960.0, 540.0])
            );
            assert_eq!(foreground.transform.scale, [100.0, 100.0]);
            assert_eq!(foreground.transform.rotation, 17.0);
            let content = as_group(&foreground.layers[0]);
            let FxLayer::Rect(rect) = content.layers[0].data() else {
                panic!("isolated Z Rotation source must remain an editable solid")
            };
            assert_eq!(rect.rect.size, [480.0, 270.0]);
            assert_eq!(rect.rect.fill_color, [0.1875, 0.625, 0.9375, 1.0]);
            assert_eq!(as_group(&root.layers[1]).name, "black_backdrop");
            let editable = converted.document.to_json_vec().unwrap();
            EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adobe_authored_solid_source_edit_restores_pixel_anchor_units() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/implemented_additions/native_export_edit_controls.aep",
        1,
        || {
    let bytes = include_bytes!(
        "../../../tests/fixtures/implemented_additions/native_export_edit_controls.aep"
    );
    assert_eq!(bytes.len(), 527_923);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "c961d610151a74279f85b3fb54583c90379e870a375e01b65ad5ec0e2b5eb7ee"
    );

    let project = read_project(bytes).unwrap();
    let comp = composition(&project, 1);
    assert_eq!(project.item(1).unwrap().name, "EDIT_SOLID_SOURCE_EDIT");
    assert_eq!([comp.width, comp.height], [640, 360]);
    assert_eq!(comp.layers.len(), 1);
    let layer = &comp.layers[0];
    assert_eq!(layer.record.id(), 15);
    assert_eq!(layer.record.source_id(), 14);
    let source = project.item(14).unwrap();
    let solid = source.solid.as_ref().unwrap().as_ref().unwrap();
    assert_eq!([solid.width, solid.height], [180, 90]);
    assert_eq!(solid.color, [0.8, 0.15, 0.25]);

    // AE preserved the pre-edit pixel Anchor [35, 20] as source-relative
    // fractions when the Solid source dimensions changed to 180x90.
    let stored_anchor = crate::properties::read_static_source_relative_anchor(&layer.content)
        .unwrap()
        .unwrap();
    assert_eq!(stored_anchor, [35.0 / 180.0, 20.0 / 90.0]);

    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    assert_imported_canvas_matches_source(comp, &converted, "pinned Adobe source edit");
    let root = root(&converted);
    assert_eq!(root.name, "EDIT_SOLID_SOURCE_EDIT");
    let occurrence = as_group(&root.layers[0]);
    assert_eq!(occurrence.name, "Edited solid 180x90");
    assert_eq!(occurrence.transform.anchor_point, [35.0, 20.0]);
    assert_eq!(
        occurrence.transform.position,
        fx_composition::Position::TwoD([270.0, 160.0])
    );
    let content = as_group(&occurrence.layers[0]);
    let FxLayer::Rect(rect) = content.layers[0].data() else {
        panic!("edited Adobe Solid must remain native editable Rect content")
    };
    assert_eq!(rect.rect.size, [180.0, 90.0]);
    assert_eq!(rect.rect.position, [0.0, 0.0]);
    assert_eq!(
        rect.rect.fill_color,
        [
            f64::from(0.8_f32),
            f64::from(0.15_f32),
            f64::from(0.25_f32),
            1.0,
        ]
    );

    let editable = converted.document.to_json_vec().unwrap();
    EditableFxCompositionDocument::from_json_slice(&editable).unwrap();
        },
    );
    cases.finish();
}
