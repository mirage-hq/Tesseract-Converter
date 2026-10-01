//! Manually specified edited-FX inputs. Native readback here is supplementary,
//! not Adobe open/inspection, reference measurement or an import assertion.
use super::*;
use crate::{properties, rifx::Chunk};

fn numeric(chunks: &[Chunk], name: &str) -> Option<properties::NumericProperty> {
    if let Ok(runs) = properties::runs(chunks) {
        for (run_name, run) in runs {
            if run_name == name {
                return properties::read_numeric(properties::unique_list(run, *b"tdbs").ok()?).ok();
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| numeric(children, name))
}

fn collect_text<'a>(layers: &'a [fx_schema::Layer], out: &mut Vec<&'a fx_schema::TextLayer>) {
    for layer in layers {
        match layer.data() {
            fx_schema::LayerData::Text(text) => out.push(text),
            fx_schema::LayerData::Group(group) => collect_text(&group.layers, out),
            _ => {}
        }
    }
}

fn check_case(name: &str, input_json: &str, expected_json: &str) {
    let input: Value = serde_json::from_str(input_json).expect("committed explicit FX input");
    let expected: Value = serde_json::from_str(expected_json).expect("manual native expectation");
    assert_eq!(input["composition"]["name"], name);
    assert_eq!(expected["status"], "AUTHORED_UNRUN");
    assert!(!input_json.contains("jsScript"));
    if let Some(dir) = std::env::var_os("AEP_EFFECTS_FX_PANEL_DIR") {
        std::fs::create_dir_all(&dir).expect("panel artifact directory");
        let base = std::path::Path::new(&dir).join(name);
        std::fs::write(base.with_extension("fx.json"), input_json).expect("explicit FX artifact");
        // AEP is written below after the fresh export, not borrowed from the authoring source.
    }
    let output = export(input);
    if let Some(dir) = std::env::var_os("AEP_EFFECTS_FX_PANEL_DIR") {
        let base = std::path::Path::new(&dir).join(name);
        std::fs::write(base.with_extension("aep"), &output.bytes).expect("fresh AEP artifact");
        std::fs::write(base.with_extension("expected.json"), expected_json)
            .expect("manual oracle artifact");
    }
    let project = read_project(&output.bytes).expect("supplementary own-reader native parse");
    let native = layers(&project);
    match name {
        "panel-text-document" => {
            let [layer] = native else {
                panic!("expected exactly one editable Text layer")
            };
            assert_eq!(layer.name.as_ref(), expected["layerName"].as_str().unwrap());
            assert_eq!(layer.record.layer_type(), 3);
            assert_eq!(layer.record.source_id(), 0, "no flattened media");
            let converted = to_structural_fx_document(&project, Some(1)).unwrap();
            let root = converted.document.to_json_value().unwrap();
            let serialized = root.to_string();
            assert!(serialized.contains(expected["text"].as_str().unwrap()));
            assert!(serialized.contains(expected["fontFamily"].as_str().unwrap()));
            assert!(!serialized.contains("jsScript"));
        }
        "panel-text-animation" => {
            let [layer] = native else {
                panic!("expected one editable Text animator owner")
            };
            assert_eq!(layer.name.as_ref(), expected["layerName"].as_str().unwrap());
            assert_eq!(layer.record.layer_type(), 3);
            assert_eq!(layer.record.source_id(), 0);
            let converted = to_structural_fx_document(&project, Some(1)).unwrap();
            let mut texts = Vec::new();
            collect_text(converted.document.composition().layers(), &mut texts);
            let [text] = texts.as_slice() else {
                panic!("editable Text must survive fresh export")
            };
            assert_eq!(text.source_text.text, expected["text"].as_str().unwrap());
            let [animator] = text.animators.as_slice() else {
                panic!("one editable Text animator")
            };
            assert_eq!(animator.name, expected["animatorName"].as_str().unwrap());
            assert!(
                animator.position.is_some(),
                "Animator position must remain editable"
            );
            assert_eq!(animator.selectors.len(), 1);
            let selector = &animator.selectors[0];
            let entry = converted
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .find(|entry| {
                    matches!(&entry.target, fx_schema::PropertyTarget::FxItemProperty(target)
                    if target.item_id() == selector.id && target.property_name() == "start")
                })
                .expect("selector Start must be a targeted editable key track");
            let keys = entry
                .animator
                .keyframe_track()
                .expect("selector keys")
                .keyframes();
            assert_eq!(
                keys.len(),
                expected["selectorStartKeys"].as_array().unwrap().len()
            );
            for (key, oracle) in keys
                .iter()
                .zip(expected["selectorStartKeys"].as_array().unwrap())
            {
                assert_eq!(
                    key.layer_time().as_millis(),
                    oracle[0].as_i64().unwrap() * 1000
                );
                assert_eq!(
                    serde_json::to_value(key.value()).unwrap()["value"].as_f64(),
                    oracle[1].as_f64()
                );
            }
        }
        "panel-gradient-stroke" => {
            let [layer] = native else {
                panic!("Gradient and Stroke must share one native Shape")
            };
            assert_eq!(layer.name.as_ref(), expected["layerName"].as_str().unwrap());
            assert_eq!(layer.record.layer_type(), 4);
            assert_eq!(layer.record.source_id(), 0);
            for property in expected["properties"].as_array().unwrap() {
                let name = property["name"].as_str().unwrap();
                let actual = numeric(&layer.content, name)
                    .unwrap_or_else(|| panic!("missing editable {name}"));
                assert_eq!(
                    actual.values,
                    property["value"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|value| value.as_f64().unwrap())
                        .collect::<Vec<_>>(),
                    "{name}"
                );
            }
        }
        "panel-text-path" => {
            let [layer] = native else {
                panic!("text guide must become one editable Text mask")
            };
            assert_eq!(layer.name.as_ref(), expected["layerName"].as_str().unwrap());
            assert_eq!(layer.record.layer_type(), 3);
            assert_eq!(layer.record.source_id(), 0);
            assert_eq!(
                numeric(&layer.content, "ADBE Text First Margin")
                    .expect("editable native Text Path First Margin")
                    .values,
                vec![expected["firstMargin"].as_f64().unwrap()]
            );
            let converted = to_structural_fx_document(&project, Some(1)).unwrap();
            let mut texts = Vec::new();
            collect_text(converted.document.composition().layers(), &mut texts);
            let [text] = texts.as_slice() else {
                panic!("editable Text path owner")
            };
            assert_eq!(text.source_text.text, expected["text"].as_str().unwrap());
            let path = text
                .path_options
                .as_ref()
                .expect("editable Text Path Options");
            assert_ne!(path.path_layer, text.id);
            assert_eq!(path.first_margin, expected["firstMargin"].as_f64().unwrap());
        }
        "panel-mask-matte" => {
            assert_eq!(
                native.len(),
                2,
                "mask and matte must be separate editable leaves"
            );
            let target = native
                .iter()
                .find(|layer| layer.name.as_ref() == expected["targetName"].as_str().unwrap())
                .expect("mask target");
            let matte = native
                .iter()
                .find(|layer| layer.name.as_ref() == expected["matteName"].as_str().unwrap())
                .expect("matte provider");
            assert_eq!(
                u64::from(target.record.track_matte_type()),
                expected["matteType"].as_u64().unwrap()
            );
            assert_eq!(target.record.matte_layer_id(), Some(matte.record.id()));
            assert_eq!(
                numeric(&target.content, "ADBE Mask Opacity")
                    .expect("editable native Mask Opacity")
                    .values,
                vec![expected["maskOpacity"].as_f64().unwrap() * 100.0]
            );
            assert_eq!(
                numeric(&target.content, "ADBE Mask Feather")
                    .expect("editable native Mask Feather")
                    .values,
                expected["maskFeather"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_f64().unwrap())
                    .collect::<Vec<_>>()
            );
        }
        "panel-motion-3d" => {
            let layer = native
                .iter()
                .find(|layer| layer.name.as_ref() == expected["layerName"].as_str().unwrap())
                .expect("3D owner survives camera insertion");
            assert!(layer.record.flags().three_d_layer);
            assert!(layer.record.flags().motion_blur);
            let orientation =
                numeric(&layer.content, "ADBE Orientation").expect("editable 3D Orientation");
            assert_eq!(
                orientation.values,
                expected["orientation"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect::<Vec<_>>()
            );
            let position = numeric(&layer.content, "ADBE Position").expect("editable 3D Position");
            assert_eq!(position.values[2], expected["positionZ"].as_f64().unwrap());
            let rotation =
                numeric(&layer.content, "ADBE Rotate X").expect("editable 3D Rotation X keys");
            assert_eq!(rotation.keyframes.len(), 2);
            for (key, oracle) in rotation
                .keyframes
                .iter()
                .zip(expected["rotationXKeys"].as_array().unwrap())
            {
                assert!((key.time_secs - oracle[0].as_f64().unwrap()).abs() < 1e-6);
                assert!((key.values[0] - oracle[1].as_f64().unwrap()).abs() < 1e-6);
            }
        }
        "panel-modifier-boolean" => {
            let [layer] = native else {
                panic!("Merge and Trim must remain one editable native vector owner")
            };
            assert_eq!(layer.name.as_ref(), expected["layerName"].as_str().unwrap());
            assert_eq!(layer.record.layer_type(), 4);
            assert_eq!(layer.record.source_id(), 0);
            assert_eq!(
                numeric(&layer.content, "ADBE Vector Merge Type")
                    .expect("editable Union operation")
                    .values,
                vec![expected["mergeOrdinal"].as_f64().unwrap()]
            );
            assert_eq!(
                numeric(&layer.content, "ADBE Vector Trim End")
                    .expect("editable Trim End")
                    .values,
                vec![expected["trimEnd"].as_f64().unwrap()]
            );
            assert!(
                numeric(&layer.content, "ADBE Vector Rect Size").is_some(),
                "native Rectangle operand"
            );
            assert!(
                numeric(&layer.content, "ADBE Vector Ellipse Size").is_some(),
                "native Ellipse operand"
            );
            let color =
                numeric(&layer.content, "ADBE Vector Fill Color").expect("owned editable paint");
            for (actual, expected) in color
                .values
                .iter()
                .zip(expected["fillColor"].as_array().unwrap())
            {
                assert!((actual - expected.as_f64().unwrap()).abs() < 1e-6);
            }
        }
        "panel-nested-effects" => {
            let root = native
                .iter()
                .find(|layer| layer.name.as_ref() == expected["rootName"].as_str().unwrap())
                .expect("native group occurrence");
            let ItemKind::Composition(source) = &project
                .item(root.record.source_id())
                .expect("fresh generated source")
                .kind
            else {
                panic!("group must generate a native precomposition")
            };
            let child = source
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == expected["childName"].as_str().unwrap())
                .expect("nested original FX effect owner");
            assert!(child.record.flags().effects_active);
            assert_eq!(
                numeric(&child.content, "ADBE Gaussian Blur 2-0001")
                    .expect("editable Gaussian Blur on nested owner")
                    .values,
                vec![expected["blurriness"].as_f64().unwrap()]
            );
            assert!(
                output
                    .diagnostics
                    .iter()
                    .all(|item| !item.message.contains("Effect stack omitted"))
            );
        }
        "panel-scene-stack" => {
            assert_eq!(
                native.len(),
                2,
                "two independent native leaves, not flattened"
            );
            for (actual, oracle) in native.iter().zip(expected["layers"].as_array().unwrap()) {
                assert_eq!(actual.name.as_ref(), oracle["name"].as_str().unwrap());
                assert_eq!(
                    u64::from(actual.record.blend_mode()),
                    oracle["blendCode"].as_u64().unwrap()
                );
            }
            let opacity =
                numeric(&native[0].content, "ADBE Opacity").expect("native editable opacity");
            assert_eq!(
                opacity.values,
                vec![expected["layers"][0]["opacity"].as_f64().unwrap()]
            );
            let position =
                numeric(&native[0].content, "ADBE Position").expect("editable keyed position");
            assert_eq!(position.keyframes.len(), 2);
            for (key, oracle) in position
                .keyframes
                .iter()
                .zip(expected["layers"][0]["positionXKeys"].as_array().unwrap())
            {
                assert!((key.time_secs - oracle[0].as_f64().unwrap()).abs() < 1e-6);
                assert!((key.values[0] - oracle[1].as_f64().unwrap()).abs() < 1e-6);
                assert_eq!((key.in_interpolation, key.out_interpolation), (1, 1));
            }
        }
        _ => panic!("undeclared export case: {name}"),
    }
}

macro_rules! panel_case {
    ($test:ident, $name:literal, $input:expr, $expected:expr) => {
        #[test]
        #[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
        fn $test() {
            crate::adobe_test_support::export_case($name, || check_case($name, $input, $expected));
        }
    };
}

panel_case!(
    panel_text_document,
    "panel-text-document",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-text-document.fx.json"),
    include_str!(
        "../../../tests/fixtures/non_audio_native_panel/panel-text-document.expected.json"
    )
);
panel_case!(
    panel_scene_stack,
    "panel-scene-stack",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-scene-stack.fx.json"),
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-scene-stack.expected.json")
);
panel_case!(
    panel_text_animation,
    "panel-text-animation",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-text-animation.fx.json"),
    include_str!(
        "../../../tests/fixtures/non_audio_native_panel/panel-text-animation.expected.json"
    )
);
panel_case!(
    panel_gradient_stroke,
    "panel-gradient-stroke",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-gradient-stroke.fx.json"),
    include_str!(
        "../../../tests/fixtures/non_audio_native_panel/panel-gradient-stroke.expected.json"
    )
);
panel_case!(
    panel_text_path,
    "panel-text-path",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-text-path.fx.json"),
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-text-path.expected.json")
);
panel_case!(
    panel_mask_matte,
    "panel-mask-matte",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-mask-matte.fx.json"),
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-mask-matte.expected.json")
);
panel_case!(
    panel_motion_3d,
    "panel-motion-3d",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-motion-3d.fx.json"),
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-motion-3d.expected.json")
);
panel_case!(
    panel_modifier_boolean,
    "panel-modifier-boolean",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-modifier-boolean.fx.json"),
    include_str!(
        "../../../tests/fixtures/non_audio_native_panel/panel-modifier-boolean.expected.json"
    )
);
panel_case!(
    panel_nested_effects,
    "panel-nested-effects",
    include_str!("../../../tests/fixtures/non_audio_native_panel/panel-nested-effects.fx.json"),
    include_str!(
        "../../../tests/fixtures/non_audio_native_panel/panel-nested-effects.expected.json"
    )
);
