//! Graphic-host regressions using controls from feature_ramp_strict.prproj.
use super::*;
use serde_json::Value;

/// Relocate one native Ramp and its parameters into the native Text/Shape
/// scaffold. Key intervals/values/easings are unchanged, on the generator In.
fn native_ramp(xml: &str, native_id: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_ramp_strict.prproj");
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
    output
        .replace(
            &output[placeholder.range()],
            &relocate(source[component.range()].to_owned()),
        )
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

#[test]
fn native_graphic_ramp_retains_colors_controls_text_shape_and_neighbor() {
    let baseline = shape_xml(TEXT_THEN_SHAPE, FILL);
    let xml = native_ramp(&baseline, "130");
    let actual = graphic(&xml);
    assert_eq!(actual.objects, graphic(&baseline).objects);
    assert_eq!(actual.effect_loss.as_ref().unwrap().mapped_ramps.len(), 1);
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
            .mapped_ramps
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
        let mapped = &actual.effect_loss.as_ref().unwrap().mapped_ramps[0];
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
        .mapped_ramps
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
        assert_eq!(sibling.effect_loss.as_ref().unwrap().mapped_ramps.len(), 1);
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains("alpha coverage is unverified")),
            "{omissions:?}"
        );
    }
}
