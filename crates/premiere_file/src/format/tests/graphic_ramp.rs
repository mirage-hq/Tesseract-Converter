//! Graphic-host regressions using controls from feature_ramp_strict.prproj.
use super::*;
use serde_json::Value;

/// Relocate one native Ramp and its parameters into the native Text/Shape
/// scaffold. Key intervals/values/easings are unchanged, on the generator In.
fn native_ramp(xml: &str, native_id: &str) -> String {
    native_effect(xml, "feature_ramp_strict.prproj", native_id)
}

fn native_effect(xml: &str, fixture: &str, native_id: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let source = crate::format::read_xml(&path).unwrap();
    let parsed = roxmltree::Document::parse(&source).unwrap();
    let records: Vec<_> = parsed
        .root_element()
        .children()
        .filter(|node| node.is_element())
        .collect();
    let component = records
        .iter()
        .find(|node| node.attribute("ObjectID") == Some(native_id))
        .unwrap();
    let params = component
        .children()
        .find(|node| node.has_tag_name("Component"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("Params"))
        .unwrap();
    let mut relocation = vec![(native_id.to_owned(), "100".to_owned())];
    let mut values = Vec::new();
    for (index, reference) in params
        .children()
        .filter(|node| node.has_tag_name("Param"))
        .enumerate()
    {
        let id = reference.attribute("ObjectRef").unwrap();
        relocation.push((id.to_owned(), (101 + index).to_string()));
        let parameter = records
            .iter()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        let mut value = source[parameter.range()].to_owned();
        if let Some(keys) = parameter
            .children()
            .find(|node| node.has_tag_name("Keyframes"))
        {
            let original = keys.text().unwrap();
            let generator_in = graphic(xml).in_ticks;
            let shifted = original
                .split(';')
                .filter(|key| !key.is_empty())
                .map(|key| {
                    let (time, fields) = key.split_once(',').unwrap();
                    format!(
                        "{},{};",
                        time.parse::<i64>().unwrap() + generator_in,
                        fields
                    )
                })
                .collect::<String>();
            value = value.replace(original, &shifted);
        }
        values.push(value);
    }
    let relocate = |mut value: String| {
        for (old, new) in &relocation {
            value = value
                .replace(
                    &format!("ObjectID=\"{old}\""),
                    &format!("ObjectID=\"{new}\""),
                )
                .replace(
                    &format!("ObjectRef=\"{old}\""),
                    &format!("ObjectRef=\"{new}\""),
                );
        }
        value
    };
    let output = with_graphic_ramp(xml, "");
    let output_parsed = roxmltree::Document::parse(&output).unwrap();
    let placeholder = output_parsed
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some("100"))
        .unwrap();
    let mut effect = source[component.range()].to_owned();
    // Static blur fixtures use ID 4, already owned by the scaffold's Text.
    if fixture == "feature_gaussian_blur_strict.prproj" {
        effect = effect.replace("<ID>4</ID>", "<ID>10</ID>");
    }
    output
        .replace(&output[placeholder.range()], &relocate(effect))
        .replace(
            "</PremiereData>",
            &format!(
                "{}</PremiereData>",
                values.into_iter().map(relocate).collect::<String>()
            ),
        )
}

fn converted(xml: &str) -> (Value, Vec<crate::Omission>) {
    let (project, mut omissions) = inspect_project_with_omissions(xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    let document = crate::convert::premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    (document, omissions)
}

#[test]
fn legacy_json_appearance_stroke_matches_editable_modern_shape_and_keeps_text() {
    use crate::{
        format::shape_payload::encode_appearance,
        schema::text::PrShapeStroke,
        tests::support::{legacy_appearance, legacy_json},
    };
    let appearance = crate::schema::text::PrAppearance {
        fill: Some(crate::schema::text::PrFill::Solid(
            crate::schema::text::PrRgb([128; 3]),
        )),
        stroke: Some(PrShapeStroke {
            color: crate::schema::text::PrRgb([48; 3]),
            width: 25.0,
            join: None,
        }),
        shadow: None,
        mask_source: None,
    };
    let modern = shape_xml(
        TEXT_THEN_SHAPE,
        &STANDARD.encode(encode_appearance(&appearance).unwrap()),
    );
    let legacy = shape_xml(
        TEXT_THEN_SHAPE,
        &STANDARD.encode(legacy_appearance(&legacy_json(&[
            ("mStrokeVisible", Some("true")),
            ("mStrokeColor", Some("3158064")),
            ("mStrokeWidth", Some("12.5")),
        ]))),
    );
    let read = graphic(&legacy);
    let [PrGraphicObject::Text(_), PrGraphicObject::Shape(shape)] = read.objects.as_slice() else {
        panic!("{:?}", read.objects);
    };
    assert_eq!(shape.appearance, appearance);
    let (expected, _) = converted(&modern);
    let (actual, omissions) = converted(&legacy);
    assert_eq!(actual, expected);
    assert!(
        omissions
            .iter()
            .all(|report| report.scope != crate::OmissionScope::Occurrence),
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .any(|report| report.reason.contains("centred editable stroke")),
        "{omissions:?}"
    );
}

#[test]
fn legacy_rgb_shape_paints_reach_editable_fill_and_stroke() {
    use crate::tests::support::{legacy_appearance, legacy_json};

    // Match the independently calibrated C6 blue fill and green centred stroke.
    let modern = shape_xml(TEXT_THEN_SHAPE, CENTRED);
    let legacy = shape_xml(
        TEXT_THEN_SHAPE,
        &STANDARD.encode(legacy_appearance(&legacy_json(&[
            ("mFillColor", Some("24831")),
            ("mStrokeVisible", Some("true")),
            ("mStrokeColor", Some("65344")),
            ("mStrokeWidth", Some("12")),
        ]))),
    );
    let (expected, _) = converted(&modern);
    let (actual, omissions) = converted(&legacy);
    assert_eq!(actual, expected);
    let layers = actual["composition"]["layers"][0]["layers"]
        .as_array()
        .unwrap();
    let shape = layers
        .iter()
        .find(|layer| layer["type"] == "Shape")
        .unwrap();
    assert_eq!(
        shape["shape"]["fills"][0]["paint"]["color"],
        serde_json::json!([0.0, 96.0 / 255.0, 1.0, 1.0])
    );
    assert_eq!(
        shape["shape"]["strokes"][0]["paint"]["color"],
        serde_json::json!([0.0, 1.0, 64.0 / 255.0, 1.0])
    );
    assert_eq!(shape["shape"]["strokes"][0]["width"], 24.0);
    assert!(layers.iter().any(|layer| layer["type"] == "Text"));
    assert!(omissions
        .iter()
        .all(|report| report.scope != crate::OmissionScope::Occurrence));
}

#[test]
fn legacy_shape_stroke_extent_maps_to_full_editable_width() {
    use crate::tests::support::{legacy_appearance, legacy_json};
    for (native, editable) in [("0", 0.0), ("0.5", 1.0), ("100", 200.0)] {
        let xml = shape_xml(
            TEXT_THEN_SHAPE,
            &STANDARD.encode(legacy_appearance(&legacy_json(&[
                ("mStrokeVisible", Some("true")),
                ("mStrokeColor", Some("3158064")),
                ("mStrokeWidth", Some(native)),
                ("mLineJoinType", Some("1")),
            ]))),
        );
        let (document, omissions) = converted(&xml);
        let layers = document["composition"]["layers"][0]["layers"]
            .as_array()
            .unwrap();
        let shape = layers
            .iter()
            .find(|layer| layer["type"] == "Shape")
            .unwrap();
        let stroke = &shape["shape"]["strokes"][0];
        assert_eq!(stroke["width"], editable, "native extent {native}");
        assert_eq!(stroke["join"], "round");
        assert_eq!(shape["shape"]["fills"].as_array().unwrap().len(), 1);
        assert!(layers.iter().any(|layer| layer["type"] == "Text"));
        assert!(omissions
            .iter()
            .all(|report| report.scope != crate::OmissionScope::Occurrence));
    }
}

#[test]
fn legacy_round_stroke_join_remains_editable_and_keeps_fill_and_text() {
    use crate::tests::support::{legacy_appearance, legacy_json};
    let xml = shape_xml(
        TEXT_THEN_SHAPE,
        &STANDARD.encode(legacy_appearance(&legacy_json(&[
            ("mStrokeVisible", Some("true")),
            ("mStrokeColor", Some("3158064")),
            ("mStrokeWidth", Some("12.5")),
            ("mLineJoinType", Some("1")),
        ]))),
    );
    let (document, omissions) = converted(&xml);
    let layers = document["composition"]["layers"][0]["layers"]
        .as_array()
        .unwrap();
    assert_eq!(layers.len(), 2);
    let shape = layers
        .iter()
        .find(|layer| layer["type"] == "Shape")
        .unwrap();
    assert_eq!(shape["shape"]["strokes"][0]["join"], "round");
    assert_eq!(shape["shape"]["strokes"][0]["width"], 25.0);
    assert_eq!(shape["shape"]["fills"].as_array().unwrap().len(), 1);
    assert!(layers.iter().any(|layer| layer["type"] == "Text"));
    assert!(omissions
        .iter()
        .all(|report| report.scope != crate::OmissionScope::Occurrence));
}

fn ramp_group(document: &Value) -> &Value {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer["effects"]
                .as_array()
                .is_some_and(|effects| !effects.is_empty())
        })
        .unwrap()
}

/// Supplementary rehosting of published Transform controls, not a native
/// graphic-host oracle. Geometry2 must not discard editable paints or keys.
#[test]
fn graphic_geometry2_retains_editable_affine_and_generator_keys() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    let xml = native_effect(&baseline, "feature_transform_strict.prproj", "146")
        .replace("<ID>3</ID>", "<ID>10</ID>")
        .replace(
            "AE.ADBE Geometry</MatchName>",
            "AE.ADBE Geometry2</MatchName>",
        );
    let actual = graphic(&xml);
    assert_eq!(actual.objects, graphic(&baseline).objects);
    let (document, omissions) = converted(&xml);
    let outer = &document["composition"]["layers"][0];
    let stage = &outer["layers"][0];
    assert_eq!(stage["type"], "Group");
    assert_eq!(
        stage["transform"]["anchorPoint"],
        serde_json::json!([960.0, 540.0])
    );
    assert_eq!(
        stage["transform"]["position"],
        serde_json::json!([960.0, 540.0])
    );
    assert_eq!(stage["layers"].as_array().unwrap().len(), 2);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let tracks: Vec<_> = entries
        .iter()
        .filter(|entry| entry["target"]["layerId"] == stage["id"])
        .collect();
    assert_eq!(tracks.len(), 3, "{entries:?}");
    for property in ["scaleX", "scaleY", "rotation"] {
        let track = tracks
            .iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap();
        let keys = track["animator"]["keyframes"].as_array().unwrap();
        let actual: Vec<_> = keys
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            actual,
            if property == "rotation" {
                vec![(1000, 0.0), (2500, 90.0)]
            } else {
                vec![(1000, 100.0), (1500, 200.0), (2500, 50.0)]
            }
        );
    }
    assert_eq!(stage["parent"], outer["id"]);
    assert!(stage["layers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|child| child["parent"] == stage["id"]));
    assert!(omissions
        .iter()
        .any(|report| report.reason.contains("graphic Geometry2 retained")));
}

#[test]
fn graphic_geometry2_stack_preserves_order_and_static_affine_controls() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    let affine = native_effect(&baseline, "feature_transform_strict.prproj", "137")
        .replace("<ID>3</ID>", "<ID>11</ID>")
        .replace(
            "AE.ADBE Geometry</MatchName>",
            "AE.ADBE Geometry2</MatchName>",
        );
    let parsed = roxmltree::Document::parse(&affine).unwrap();
    let mut records = parsed
        .root_element()
        .children()
        .filter(|node| {
            node.attribute("ObjectID")
                .and_then(|id| id.parse::<u32>().ok())
                .is_some_and(|id| (100..113).contains(&id))
        })
        .map(|node| affine[node.range()].to_owned())
        .collect::<String>();
    for id in 100..113 {
        records = records
            .replace(
                &format!("ObjectID=\"{id}\""),
                &format!("ObjectID=\"{}\"", id + 300),
            )
            .replace(
                &format!("ObjectRef=\"{id}\""),
                &format!("ObjectRef=\"{}\"", id + 300),
            );
    }
    let xml = native_ramp(&baseline, "130")
        .replace(
            r#"<Component Index="1" ObjectRef="40"/>"#,
            r#"<Component Index="2" ObjectRef="40"/>"#,
        )
        .replace(
            r#"<Component Index="0" ObjectRef="100"/>"#,
            r#"<Component Index="0" ObjectRef="100"/><Component Index="1" ObjectRef="400"/>"#,
        )
        .replace("</PremiereData>", &format!("{records}</PremiereData>"));
    for ramp_last in [true, false] {
        let xml = if ramp_last {
            xml.clone()
        } else {
            xml.replace(
                r#"<Component Index="0" ObjectRef="100"/><Component Index="1" ObjectRef="400"/>"#,
                r#"<Component Index="0" ObjectRef="400"/><Component Index="1" ObjectRef="100"/>"#,
            )
        };
        assert_eq!(graphic(&xml).objects, graphic(&baseline).objects);
        let (document, _) = converted(&xml);
        let outside = &document["composition"]["layers"][0]["layers"][0];
        let inside = &outside["layers"][0];
        let (ramp, affine) = if ramp_last {
            (outside, inside)
        } else {
            (inside, outside)
        };
        assert_eq!(ramp["effects"][0]["effect"]["type"], "gradientRamp");
        assert_eq!(
            affine["transform"]["anchorPoint"],
            serde_json::json!([1440.0, 540.0])
        );
        assert_eq!(
            affine["transform"]["position"],
            serde_json::json!([960.0, 540.0])
        );
        assert_eq!(
            affine["transform"]["scale"],
            serde_json::json!([50.0, 50.0])
        );
        assert_eq!(affine["transform"]["rotation"], 30.0);
        assert_eq!(inside["layers"].as_array().unwrap().len(), 2);
        assert_eq!(inside["parent"], outside["id"]);
    }
}

#[test]
fn graphic_geometry2_bypass_keeps_base_paints_without_affine_stages_or_keys() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    let (expected, _) = converted(&baseline);
    for native_id in ["137", "146"] {
        let xml = native_effect(&baseline, "feature_transform_strict.prproj", native_id)
            .replace("<ID>3</ID>", "<ID>10</ID>")
            .replace(
                "AE.ADBE Geometry</MatchName>",
                "AE.ADBE Geometry2</MatchName>",
            );
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        let component = parsed
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some("100"))
            .unwrap();
        let original = &xml[component.range()];
        let bypassed = original.replace("</Component>", "<Bypass>true</Bypass></Component>");
        assert_ne!(original, bypassed);
        let xml = xml.replace(original, &bypassed);
        assert!(graphic(&xml).effect_loss.is_none());
        let (actual, omissions) = converted(&xml);
        assert_eq!(actual, expected);
        assert!(omissions
            .iter()
            .any(|report| report.reason.contains("bypassed graphic Geometry2 omitted")));
    }
}

#[test]
fn graphic_geometry2_unmapped_controls_keep_independent_paints() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    // The scaffold lacks native affine parameters: omission is local, not
    // a reason to discard otherwise convertible Text and Shape siblings.
    let xml = with_graphic_ramp(&baseline, "").replace("AE.ADBE Ramp", "AE.ADBE Geometry2");
    assert_eq!(graphic(&xml).objects, graphic(&baseline).objects);
    assert!(graphic(&xml).effect_loss.unwrap().has_unmapped_effects);
    let (_, omissions) = converted(&xml);
    assert!(omissions.iter().any(|report| report
        .reason
        .contains("graphic Geometry2 is unsupported; saved base paints retained")));
}

/// Rehost public native Tint controls, not an independent graphic-host oracle.
#[test]
fn graphic_tint_retains_editable_paints_controls_and_generator_keys() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    for native_id in ["124", "127"] {
        let xml = native_effect(&baseline, "feature_tint_strict.prproj", native_id)
            .replace("<ID>3</ID>", "<ID>10</ID>");
        let actual = graphic(&xml);
        assert_eq!(actual.objects, graphic(&baseline).objects);
        let (document, omissions) = converted(&xml);
        let group = ramp_group(&document);
        let effect = &group["effects"][0]["effect"];
        assert_eq!(effect["type"], "tintTritone");
        for channel in ["blackR", "blackG", "blackB", "whiteB"] {
            assert_eq!(effect[channel], 0.0, "{effect:?}");
        }
        // Native donor's Map White To is orange RGB(255, 128, 0).
        assert_eq!(effect["whiteR"], 1.0);
        assert_eq!(effect["whiteG"], 128.0 / 255.0);
        assert_eq!(
            effect["amount"],
            if native_id == "124" { 50.0 } else { 0.0 }
        );
        assert!(omissions
            .iter()
            .any(|report| report.reason.contains("graphic Tint retained")));
        assert!(omissions
            .iter()
            .all(|report| report.scope != crate::OmissionScope::Occurrence));
        if native_id == "127" {
            let entries = document["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap();
            let track = entries
                .iter()
                .find(|entry| entry["target"]["paramName"] == "amount")
                .unwrap();
            assert_eq!(track["target"]["effectId"], group["effects"][0]["id"]);
            let keys = track["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 3);
            for (key, (time, value)) in keys.iter().zip([(1000, 0.0), (1500, 100.0), (2500, 50.0)])
            {
                assert_eq!(key["layerTime"], time);
                assert_eq!(key["value"]["value"], value);
            }
        }
    }
}

#[test]
fn graphic_tint_unmapped_controls_keep_independent_paints() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    let xml = with_graphic_ramp(&baseline, "").replace("AE.ADBE Ramp", "AE.ADBE Tint");
    assert_eq!(graphic(&xml).objects, graphic(&baseline).objects);
    let (_, omissions) = converted(&xml);
    assert!(omissions.iter().any(|report| report
        .reason
        .contains("graphic Tint is unsupported; saved base paints retained")));
}

#[test]
fn graphic_gaussian_blur_retains_editable_objects_and_controls() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    for (native_id, amount, repeat) in [("160", 25.0, false), ("164", 80.0, true)] {
        let mut xml = native_effect(&baseline, "feature_gaussian_blur_strict.prproj", native_id);
        if repeat {
            // Supplementary toggle mutation; both native donors save it off.
            let parsed = roxmltree::Document::parse(&xml).unwrap();
            let parameter = parsed
                .root_element()
                .children()
                .find(|node| node.attribute("ObjectID") == Some("103"))
                .unwrap();
            let original = &xml[parameter.range()];
            let toggled = original.replace(
                "-91445760000000000,false,0,0,0,0,0,0",
                "-91445760000000000,true,0,0,0,0,0,0",
            );
            xml = xml.replace(original, &toggled);
        }
        let actual = graphic(&xml);
        assert_eq!(actual.objects, graphic(&baseline).objects);
        let (document, omissions) = converted(&xml);
        let group = ramp_group(&document);
        assert_eq!(group["type"], "Group");
        assert_eq!(group["effects"][0]["effect"]["type"], "gaussianBlur");
        assert_eq!(group["effects"][0]["effect"]["blurriness"], amount);
        assert_eq!(
            group["effects"][0]["effect"]["repeatEdgePixels"]
                .as_bool()
                .unwrap_or(false),
            repeat
        );
        assert!(!group["layers"].as_array().unwrap().is_empty());
        assert!(omissions
            .iter()
            .any(|omission| omission.reason.contains("graphic Gaussian Blur retained")));
    }
}

#[test]
fn graphic_gaussian_blur_keys_use_the_generator_clock() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    let xml = native_effect(
        &baseline,
        "feature_gaussian_blur_keys_26_5_strict.prproj",
        "104",
    );
    let (document, _) = converted(&xml);
    let group = ramp_group(&document);
    assert_eq!(group["effects"][0]["effect"]["blurriness"], 80.0);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let track = entries
        .iter()
        .find(|entry| entry["target"]["kind"] == "effectProperty")
        .unwrap();
    assert_eq!(track["target"]["paramName"], "blurriness");
    assert_eq!(track["target"]["effectId"], group["effects"][0]["id"]);
    let keys = track["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["layerTime"], 500);
    assert_eq!(keys[1]["layerTime"], 1500);
    assert_eq!(keys[0]["value"]["value"], 80.0);
    assert_eq!(keys[1]["value"]["value"], 0.0);
}

#[test]
fn graphic_gaussian_blur_unmapped_controls_keep_independent_content() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    // Missing saved parameters cannot supply a blur; keep the editable base.
    let xml = with_graphic_ramp(&baseline, "").replace("AE.ADBE Ramp", "AE.ADBE Gaussian Blur 2");
    let actual = graphic(&xml);
    assert_eq!(actual.objects, graphic(&baseline).objects);
    let (_, omissions) = converted(&xml);
    assert!(omissions.iter().any(|omission| omission
        .reason
        .contains("graphic Gaussian Blur is unsupported; saved base paints retained")));
}

#[test]
fn native_graphic_ramp_retains_colors_controls_text_shape_and_neighbor() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    let xml = native_ramp(&baseline, "130");
    let actual = graphic(&xml);
    assert_eq!(actual.objects, graphic(&baseline).objects);
    assert_eq!(actual.effect_loss.as_ref().unwrap().mapped_effects.len(), 1);
    let (document, omissions) = converted(&xml);
    let group = ramp_group(&document);
    assert_eq!(group["type"], "Group");
    let children = group["layers"].as_array().unwrap();
    assert!(children.iter().any(|layer| layer["type"] == "Text"));
    fn editable_shape(items: &[Value]) -> bool {
        items.iter().any(|layer| {
            layer["type"] == "Rect"
                || layer["type"] == "Shape"
                || layer["layers"]
                    .as_array()
                    .is_some_and(|children| editable_shape(children))
        })
    }
    assert!(editable_shape(children), "{children:?}");
    assert!(document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Video"));
    let effect = &group["effects"][0]["effect"];
    assert_eq!(effect["type"], "gradientRamp");
    assert_eq!(
        (effect["startX"].as_f64(), effect["startY"].as_f64()),
        (Some(0.5), Some(0.9))
    );
    assert_eq!(
        (effect["endX"].as_f64(), effect["endY"].as_f64()),
        (Some(0.5), Some(0.1))
    );
    assert_eq!(
        (
            effect["startR"].as_f64(),
            effect["startG"].as_f64(),
            effect["startB"].as_f64()
        ),
        (Some(1.0), Some(1.0), Some(0.0))
    );
    assert_eq!(
        (
            effect["endR"].as_f64(),
            effect["endG"].as_f64(),
            effect["endB"].as_f64()
        ),
        (Some(0.0), Some(0.0), Some(1.0))
    );
    assert_eq!(effect["blend"].as_f64(), Some(1.0));
    assert!(omissions
        .iter()
        .any(|omission| omission.kind == OmissionKind::Approximated
            && omission
                .reason
                .contains("graphic Ramp retained as editable gradientRamp")));
    assert!(
        !omissions.iter().any(|omission| omission
            .reason
            .contains("saved base paints retained without the effect")),
        "{omissions:?}"
    );
}

#[test]
fn native_graphic_ramp_stays_before_static_vector_motion() {
    let xml = native_ramp(&graphic_xml(BEFORE), "130");
    let actual = graphic(&xml);
    let motion = actual.vector_motion.as_ref().unwrap();
    assert_eq!((motion.scale, motion.rotation), (50.0, 90.0));
    assert_eq!(actual.objects, graphic(&text_only_xml()).objects);
    let (document, _) = converted(&xml);
    let group = ramp_group(&document);
    assert_eq!(group["transform"]["scale"], serde_json::json!([50.0, 50.0]));
    assert_eq!(group["transform"]["rotation"], 90.0);
    assert_eq!(group["layers"][0]["type"], "Text");
    assert_eq!(
        group["layers"][0]["transform"]["scale"],
        serde_json::json!([100.0, 100.0])
    );
    assert_eq!(group["effects"][0]["effect"]["type"], "gradientRamp");
}

#[test]
fn native_graphic_ramp_stack_keeps_native_application_order() {
    let baseline = text_only_xml();
    let bottom = native_ramp(&baseline, "118");
    let parsed = roxmltree::Document::parse(&bottom).unwrap();
    let mut records = parsed
        .root_element()
        .children()
        .filter(|node| {
            node.attribute("ObjectID")
                .and_then(|id| id.parse::<u32>().ok())
                .is_some_and(|id| (100..108).contains(&id))
        })
        .map(|node| bottom[node.range()].to_owned())
        .collect::<String>()
        .replace("<ID>3</ID>", "<ID>6</ID>");
    for id in 100..108 {
        records = records
            .replace(
                &format!("ObjectID=\"{id}\""),
                &format!("ObjectID=\"{}\"", id + 300),
            )
            .replace(
                &format!("ObjectRef=\"{id}\""),
                &format!("ObjectRef=\"{}\"", id + 300),
            );
    }
    let xml = native_ramp(&baseline, "130")
        .replace(
            r#"<Component Index="1" ObjectRef="40"/>"#,
            r#"<Component Index="2" ObjectRef="40"/>"#,
        )
        .replace(
            r#"<Component Index="0" ObjectRef="100"/>"#,
            r#"<Component Index="0" ObjectRef="100"/><Component Index="1" ObjectRef="400"/>"#,
        )
        .replace("</PremiereData>", &format!("{records}</PremiereData>"));
    assert_eq!(
        graphic(&xml)
            .effect_loss
            .as_ref()
            .unwrap()
            .mapped_effects
            .len(),
        2
    );
    assert_eq!(graphic(&xml).objects, graphic(&baseline).objects);
    let (document, _) = converted(&xml);
    let effects = ramp_group(&document)["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[0]["effect"]["startR"], 0.0);
    assert_eq!(effects[0]["effect"]["endR"], 1.0);
    assert_eq!(effects[1]["effect"]["startR"], 1.0);
    assert_eq!(effects[1]["effect"]["startG"], 1.0);
    assert_ne!(effects[0]["id"], effects[1]["id"]);
    let partial = xml.replace("0.5:0.90000000000000002", "0.2:0.9");
    assert_eq!(graphic(&partial).objects, graphic(&baseline).objects);
    let (document, omissions) = converted(&partial);
    let effects = ramp_group(&document)["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0]["effect"]["startR"], 0.0);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("not aligned with the frame")),
        "{omissions:?}"
    );
}

#[test]
fn native_graphic_ramp_blend_and_point_keys_use_generator_clock() {
    for (native_id, params) in [("124", vec!["blend"]), ("127", vec!["endX", "endY"])] {
        let xml = native_ramp(&text_only_xml(), native_id);
        let actual = graphic(&xml);
        let mapped = &actual.effect_loss.as_ref().unwrap().mapped_effects[0];
        assert_eq!(mapped.animations.len(), 1);
        let (document, omissions) = converted(&xml);
        let group = ramp_group(&document);
        let effect_id = group["effects"][0]["id"].clone();
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        for parameter in params {
            let track = entries
                .iter()
                .find(|entry| {
                    entry["target"]["effectId"] == effect_id
                        && entry["target"]["paramName"] == parameter
                })
                .unwrap_or_else(|| panic!("{native_id}/{parameter}: {entries:?}; {omissions:?}"));
            let keys = track["animator"]["keyframes"].as_array().unwrap();
            let actual: Vec<_> = keys
                .iter()
                .map(|key| {
                    (
                        key["layerTime"].as_i64().unwrap(),
                        key["value"]["value"].as_f64().unwrap(),
                        key["easing"]["type"].as_str().unwrap(),
                    )
                })
                .collect();
            let expected = match parameter {
                "blend" => vec![
                    (1000, 0.0, "linear"),
                    (1500, 1.0, "linear"),
                    (2500, 0.5, "hold"),
                ],
                "endX" => vec![(500, 0.5, "linear"), (1500, 0.5, "linear")],
                "endY" => vec![(500, 1.0, "linear"), (1500, 0.6, "linear")],
                _ => unreachable!(),
            };
            assert_eq!(actual, expected, "{native_id}/{parameter}: {omissions:?}");
        }
    }
}

#[test]
fn diagonal_graphic_ramp_localizes_loss_and_retains_editable_objects() {
    let baseline = text_only_xml();
    let xml = native_ramp(&baseline, "130").replace("0.5:0.90000000000000002", "0.2:0.9");
    assert!(graphic(&xml)
        .effect_loss
        .as_ref()
        .unwrap()
        .mapped_effects
        .is_empty());
    assert_eq!(graphic(&xml).objects, graphic(&baseline).objects);
    let (document, omissions) = converted(&xml);
    assert!(document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Text"));
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("Ramp")
                && omission.reason.contains("not aligned with the frame")),
        "{omissions:?}"
    );
}

#[test]
fn mapped_graphic_geometry2_retains_track_matte_provider_and_consumer() {
    use super::super::{
        animation::animation_fixture::track_matte_key_xml,
        effects::{with_chain, with_second_clip, DEFAULT_FLAGS},
    };
    let source = native_effect(&text_only_xml(), "feature_transform_strict.prproj", "137")
        .replace("<ID>3</ID>", "<ID>10</ID>")
        .replace(
            "AE.ADBE Geometry</MatchName>",
            "AE.ADBE Geometry2</MatchName>",
        )
        .replace(
            "<Start>254016000000</Start><End>762048000000</End>",
            "<Start>0</Start><End>1270080000000</End>",
        );
    for channel in [0, 1] {
        let xml = with_chain(
            &with_second_clip(&source),
            DEFAULT_FLAGS,
            &[(200, track_matte_key_xml(200, 2, channel, false))],
        );
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert!(
            sequence.video_occurrences().any(|clip| {
                clip.id.as_deref() == Some("VideoClipTrackItem:3") && clip.track_matte.is_some()
            }),
            "{omissions:?}"
        );
        let provider = sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .find(|graphic| graphic.id() == Some("VideoClipTrackItem:20"))
            .unwrap();
        let loss = provider.effect_loss.as_ref().unwrap();
        assert_eq!(loss.mapped_effects.len(), 1);
        assert!(!loss.has_unmapped_effects);
        let (document, omissions) = converted(&xml);
        let layers = document["composition"]["layers"].as_array().unwrap();
        let consumer = layers
            .iter()
            .find(|layer| !layer["trackMatte"].is_null())
            .unwrap();
        let provider_id = &consumer["trackMatte"]["layer"];
        let provider = layers
            .iter()
            .find(|layer| &layer["id"] == provider_id)
            .unwrap();
        assert_eq!(provider["type"], "Group", "{document}");
        assert_eq!(provider["layers"][0]["type"], "Group");
        assert!(omissions
            .iter()
            .any(|report| report.reason.contains("graphic Geometry2 retained")));
    }
}

#[test]
fn mapped_graphic_ramp_does_not_admit_or_expose_concealed_matte_provider() {
    use super::super::{
        animation::animation_fixture::track_matte_key_xml,
        effects::{with_chain, with_second_clip, DEFAULT_FLAGS},
    };
    let source = with_second_clip(&native_ramp(&text_only_xml(), "130").replace(
        "<Start>254016000000</Start><End>762048000000</End>",
        "<Start>0</Start><End>1270080000000</End>",
    ))
    .replace(
        r#"<TrackItem ObjectRef="20"/>"#,
        r#"<TrackItem ObjectRef="20"/><TrackItem ObjectRef="220"/>"#,
    )
    .replace(
        "</PremiereData>",
        &format!("{}</PremiereData>", graphic_item_record(220, 5 * TICKS)),
    );
    for channel in [0, 1] {
        let xml = with_chain(
            &source,
            DEFAULT_FLAGS,
            &[(200, track_matte_key_xml(200, 2, channel, false))],
        );
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert!(!sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .any(|graphic| graphic.id() == Some("VideoClipTrackItem:20")));
        let sibling = sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .find(|graphic| graphic.id() == Some("VideoClipTrackItem:220"))
            .unwrap();
        assert_eq!(
            sibling.effect_loss.as_ref().unwrap().mapped_effects.len(),
            1
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains("alpha coverage is unverified")),
            "{omissions:?}"
        );
    }
}
