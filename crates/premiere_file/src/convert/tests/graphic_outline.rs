//! Native OUTLINE keys and explicit-color supported edits. The supplied native
//! source's absent stroke color is intentionally not assigned a guessed default.
use super::*;
use crate::format::{inspect_project_with_omissions, with_explicit_white_stroke};
use base64::{engine::general_purpose::STANDARD, Engine};

const NATIVE: &str = include_str!("../../../tests/fixtures/graphic_outline/native.xml");
const UID: &str = "a6eb3c4b-2c5b-4cef-b0fd-68b013bfb73d";
const SOURCE_IN: i64 = 914_455_911_965_987;
const SOURCE_TIMES: [i64; 3] = [SOURCE_IN, 915_102_766_046_919, 915_366_614_422_036];

/// Supplementary authored white paint, not an assertion about native defaults.
/// Keep the native frame clock, placement, generator range, key times and widths.
fn explicit_color_source() -> String {
    let xml = roxmltree::Document::parse(NATIVE).unwrap();
    let source = xml
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some("1264"))
        .unwrap();
    let wire = source
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap();
    let mut edited = NATIVE.to_owned();
    for key in wire.split_terminator(';') {
        let (_, encoded) = key.split_once(',').unwrap();
        let payload = STANDARD.decode(encoded).unwrap();
        let explicit = with_explicit_white_stroke(payload);
        edited = edited.replace(encoded, &STANDARD.encode(explicit));
    }
    assert_eq!(
        edited.matches("<FrameRate>8511237907</FrameRate>").count(),
        2
    );
    edited
}

fn imported_outline() -> Value {
    let (project, mut omissions) =
        inspect_project_with_omissions(&explicit_color_source(), Some(UID)).unwrap();
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("absent Source Text run marker")),
        "{omissions:?}"
    );
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.frame_rate.ticks_per_frame(), 8_511_237_907);
    let graphic = sequence
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert_eq!(graphic.in_ticks, SOURCE_IN);
    assert_eq!(graphic.start_ticks, 0);
    assert_eq!(graphic.end_ticks, 1_268_174_448_143);
    let keys = &graphic.text().source_text_keys;
    assert_eq!(
        keys.iter().map(|key| key.source_ticks).collect::<Vec<_>>(),
        SOURCE_TIMES
    );
    assert_eq!(
        keys.iter()
            .map(|key| key.document.stroke.unwrap().width)
            .collect::<Vec<_>>(),
        [5.0, 25.0, 25.0]
    );
    premiere_to_tesseract(sequence, &project.media, &BTreeMap::new(), &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap()
}

fn export_outline(document: Value, omissions: &mut Vec<Omission>) -> Result<PrProjectFile> {
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        omissions,
    )
}

#[test]
fn native_outline_missing_color_stays_diagnosed_without_guessing() {
    let error = inspect_project_with_omissions(NATIVE, Some(UID)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ArbVideoComponentParam:1264: enabled text stroke lacks a color or width"),
        "{error}"
    );
}

#[test]
fn explicit_color_native_outline_imports_held_deltas_and_exports_authored_edits() {
    let mut document = imported_outline();
    let text = &document["composition"]["layers"][0];
    assert_eq!(text["type"], "Text");
    assert_eq!(text["name"], "OUTLINE");
    assert_eq!(text["sourceText"]["text"], "OUTLINE");
    assert_eq!(text["sourceText"]["strokeWidth"], 10.0);
    assert_eq!(
        text["sourceText"]["strokeColor"],
        json!([1.0, 1.0, 1.0, 1.0])
    );
    let animator = text["animators"][0].clone();
    assert_eq!(animator["strokeWidth"], 0.0);
    assert!(animator.get("selectors").is_none());
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["itemId"] == animator["id"])
        .unwrap();
    assert_eq!(entry["target"]["propertyName"], "strokeWidth");
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(
        keys.iter()
            .map(|key| (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
                key["easing"]["type"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [(0, 0.0, "hold"), (2547, 40.0, "hold"), (3585, 40.0, "hold")]
    );
    // Authored base 14 plus edited delta 52 gives FX width 66 / native 33.
    // The invalid unused static delta -1000 must not reject enabled keyed totals.
    document["composition"]["layers"][0]["sourceText"]["strokeWidth"] = json!(14.0);
    document["composition"]["layers"][0]["animators"][0]["strokeWidth"] = json!(-1000.0);
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["itemId"] == animator["id"])
        .unwrap();
    entry["animator"]["keyframes"][1]["value"]["value"] = json!(52.0);
    entry["animator"]["keyframes"][1]["layerTime"] = json!(2000);
    let mut omissions = Vec::new();
    let project = export_outline(document, &mut omissions).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let text = project.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap()
        .text();
    assert_eq!(
        text.source_text_keys
            .iter()
            .map(|key| (key.source_ticks, key.document.stroke.unwrap().width))
            .collect::<Vec<_>>(),
        [
            (EXPORT_IN, 7.0),
            (EXPORT_IN + 2000 * TICKS / 1000, 33.0),
            // The untouched TextContent track retains this source clock.
            (EXPORT_IN + 2547 * TICKS / 1000, 33.0),
            (EXPORT_IN + 3585 * TICKS / 1000, 27.0)
        ]
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("edited.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (read, omissions) = PrProjectFile::load(path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let text = read.sequences[0]
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap()
        .text();
    assert_eq!(
        text.source_text_keys
            .iter()
            .map(|key| key.document.stroke.unwrap().width)
            .collect::<Vec<_>>(),
        [7.0, 33.0, 33.0, 27.0]
    );
}

#[test]
fn outline_export_rejects_selected_or_multi_property_animators_and_non_hold_widths() {
    let original = imported_outline();
    let animator_id = original["composition"]["layers"][0]["animators"][0]["id"].clone();
    let mut other_property = original.clone();
    other_property["composition"]["layers"][0]["animators"][0]["rotation"] = json!(30.0);
    let mut animated_other_property = original.clone();
    let entry = animated_other_property["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["itemId"] == animator_id)
        .unwrap();
    entry["target"]["propertyName"] = json!("rotation");
    let mut selected = original.clone();
    selected["composition"]["layers"][0]["animators"][0]["selectors"] =
        json!([{"id": 100, "start": 0, "end": 50}]);
    let mut non_hold = original.clone();
    let entry = non_hold["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["itemId"] == animator_id)
        .unwrap();
    entry["animator"]["keyframes"][1]["easing"]["type"] = json!("linear");
    let mut negative = original;
    let entry = negative["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["itemId"] == animator_id)
        .unwrap();
    entry["animator"]["keyframes"][1]["value"]["value"] = json!(-11.0);
    for (document, reason, keeps_base) in [
        (other_property, "all-character width-only", false),
        (
            animated_other_property,
            "only stroke width may be animated",
            false,
        ),
        (selected, "all-character width-only", false),
        (non_hold, "stroke width keys must hold", true),
        (
            negative,
            "total stroke width must be finite and nonnegative",
            true,
        ),
    ] {
        let mut omissions = Vec::new();
        let exported = export_outline(document, &mut omissions);
        if keeps_base {
            let project = exported.unwrap();
            let text = project
                .single_sequence()
                .unwrap()
                .video_items()
                .find_map(PrVideoItem::graphic)
                .unwrap()
                .text();
            assert_eq!(text.document.text, "OUTLINE");
            assert!(text.source_text_keys.is_empty());
        } else {
            assert!(exported.is_err());
        }
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
    }
}
