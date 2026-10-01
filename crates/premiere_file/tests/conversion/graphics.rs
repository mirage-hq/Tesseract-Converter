//! Graphics on Premiere 26.5.1 fixtures. Case
//! `premiere_isolated_graphic_transform_keys_26_5`: keyed Vector Motion and
//! text object keys import as an editable graphic group. Case
//! `premiere_isolated_graphic_clip_opacity_keys_26_5`: the keyed clip Opacity
//! imports as the opacity of a graphic group. Case
//! `premiere_isolated_graphic_shapes_26_5`: graphics with several objects
//! and static vector shapes import as groups and shape layers. Case
//! `premiere_isolated_source_text_hold_keys_26_5`: Source Text keys import
//! as Hold tracks of the text layer. Case
//! `premiere_isolated_gradient_fills_26_5`: linear and radial gradient fills
//! and an opacity ramp import as FX gradient paints. In each, an edit in the
//! Tesseract document exports, and the crate reader reads it back.

use super::support::*;
use premiere_file::PrProjectFile;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const FONT_NOT_PACKAGED: &str = "font \"Arial-BoldMT\" is not packaged in this document; import it with tsrct project import-font before preview or export.";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// One key as (layer ms, value, easing type).
type Key = (i64, f64, String);

/// The key tracks of `layer`, by property type.
fn tracks(document: &Value, layer: &Value) -> Vec<(String, Vec<Key>)> {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == layer["id"])
        .map(|entry| {
            let keys = entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| {
                    (
                        key["layerTime"].as_i64().unwrap(),
                        key["value"]["value"].as_f64().unwrap(),
                        key["easing"]["type"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            (
                entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                keys,
            )
        })
        .collect()
}

fn keys(list: &[(i64, f64, &str)]) -> Vec<Key> {
    list.iter()
        .map(|&(time, value, easing)| (time, value, easing.to_owned()))
        .collect()
}

/// The graphic group and its text layer.
fn graphic(document: &Value) -> (&Value, &Value) {
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .expect("the graphic imports as a graphic group");
    let [text] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("a graphic group holds one text layer: {group}");
    };
    (group, text)
}

/// The key track of `property` in `document` on `layer`, mutably.
fn track_mut<'a>(document: &'a mut Value, layer: &Value, property: &str) -> &'a mut Value {
    document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| {
            entry["target"]["layerId"] == layer["id"] && entry["target"]["propertyType"] == property
        })
        .unwrap_or_else(|| panic!("{property} track"))
}

#[test]
fn adobe_measured_graphic_bezier_keys_import_on_the_generator_clock() {
    // C1: unchanged Premiere 26.5.1 save, independently rendered in AME.
    // Assert the saved speeds: Premiere rewrote Rotation's out speed to 15.
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_graphic_bezier_position_rotation_26_5_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.scope == premiere_file::OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let (group, text) = graphic(&document);
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start": 0, "duration": 9000})
    );
    assert_eq!(text["activeRange"], json!({"start": 0, "duration": 9000}));
    assert_eq!(text["parent"], group["id"]);
    let check_curve = |layer: &Value,
                       property: &str,
                       key_index: usize,
                       time: i64,
                       value: f64,
                       handles: [f64; 4]| {
        let entry = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == layer["id"]
                    && entry["target"]["propertyType"] == property
            })
            .unwrap();
        let key = &entry["animator"]["keyframes"][key_index];
        assert_eq!(key["layerTime"], time);
        assert!((key["value"]["value"].as_f64().unwrap() - value).abs() < 1e-9);
        assert_eq!(key["easing"]["type"], "cubicBezier");
        for (field, expected) in ["x1", "y1", "x2", "y2"].into_iter().zip(handles) {
            let actual = key["easing"][field].as_f64().unwrap();
            assert!(
                (actual - expected).abs() < 1e-9,
                "{property}/{field}: {actual} != {expected}"
            );
        }
    };
    check_curve(text, "positionX", 1, 1700, 1344.0, [0.5, 0.9, 0.75, 0.85]);
    check_curve(text, "positionY", 2, 2900, 864.0, [0.5, 1.0, 0.75, 0.875]);
    check_curve(group, "positionX", 1, 4700, 1190.4, [0.5, 1.0, 0.75, 0.9]);
    check_curve(group, "positionY", 1, 4700, 637.2, [0.5, 1.0, 0.75, 0.9]);
    check_curve(text, "rotation", 1, 6500, 90.0, [0.5, 0.1, 0.75, 0.9]);
    for axis in ["scaleX", "scaleY"] {
        check_curve(text, axis, 1, 8200, 150.0, [1.0 / 6.0, 0.2, 0.6, 0.904]);
    }
    let opacity = valued_tracks(&document, text)
        .into_iter()
        .find(|(property, _)| property == "opacity")
        .unwrap()
        .1;
    assert_eq!(
        opacity,
        vec![
            (7000, json!(100.0), "linear".to_owned()),
            (8200, json!(40.0), "hold".to_owned())
        ]
    );
}

#[test]
fn adobe_derived_multi_text_keys_target_only_their_own_layer() {
    // Native-derived G-probe P3b source, removing the unrelated last graphic.
    // The tested records are unchanged from its Premiere 26.5.1 save. The graphic
    // (2–4 s) contains keyed Vector Motion, keyed Text "py" and static Text
    // "ok". This is editable import evidence, not an Adobe render comparison.
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_multi_text_transform_keys_26_5_derived.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.record == "58" && omission.reason.contains("keyed objects")),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group" && layer["playback"]["inputRange"]["start"] == 2000)
        .expect("the keyed multi-object graphic must be imported");
    // A plain group clock: layer time is sequence time minus the 2 s start.
    let window = json!({"start": 2000, "duration": 2000});
    assert_eq!(
        group["playback"],
        json!({
            "type": "windowed",
            "inputRange": window,
            "mapping": {
                "type": "linear",
                "input": window,
                "output": {"start": 0, "duration": 2000}
            },
            "inputOffsetMs": 0
        })
    );
    assert_eq!(group["transform"]["anchorPoint"], json!([768.0, 486.0]));
    let children = group["layers"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    let (text, sibling) = (&children[0], &children[1]);
    assert_eq!(text["sourceText"]["text"], "py");
    assert_eq!(sibling["sourceText"]["text"], "ok");
    assert_ne!(text["id"], sibling["id"]);
    for child in children {
        assert_eq!(child["parent"], group["id"]);
        assert_eq!(child["activeRange"], json!({"start": 0, "duration": 2000}));
    }
    let scale = keys(&[(500, 100.0, "linear"), (1500, 120.0, "linear")]);
    assert_eq!(
        tracks(&document, text),
        [
            (
                "positionX".to_owned(),
                keys(&[(500, 480.0, "linear"), (1500, 576.0, "linear")])
            ),
            (
                "positionY".to_owned(),
                keys(&[(500, 540.0, "linear"), (1500, 594.0, "linear")])
            ),
            ("scaleX".to_owned(), scale.clone()),
            ("scaleY".to_owned(), scale),
            (
                "opacity".to_owned(),
                keys(&[(500, 100.0, "linear"), (1500, 40.0, "linear")])
            ),
        ]
    );
    assert!(tracks(&document, sibling).is_empty());
    assert_eq!(sibling["transform"]["position"], json!([1440.0, 540.0]));
    let vm_scale = keys(&[(500, 100.0, "linear"), (1500, 70.0, "linear")]);
    assert_eq!(
        tracks(&document, group),
        [
            (
                "positionX".to_owned(),
                keys(&[
                    (500, 960.0, "linear"),
                    (1000, 1056.0, "linear"),
                    (1500, 1152.0, "hold")
                ])
            ),
            (
                "positionY".to_owned(),
                keys(&[
                    (500, 540.0, "linear"),
                    (1000, 486.0, "linear"),
                    (1500, 432.0, "hold")
                ])
            ),
            ("scaleX".to_owned(), vm_scale.clone()),
            ("scaleY".to_owned(), vm_scale),
            (
                "rotation".to_owned(),
                keys(&[(500, 0.0, "linear"), (1500, 20.0, "hold")])
            ),
        ]
    );
}

#[test]
fn derived_multi_text_source_keys_keep_independent_tracks_and_trimmed_clocks() {
    // Supplementary XML combination of native payloads, never Adobe-saved:
    // two independently keyed Texts and a static Shape under shared Vector
    // Motion, at 2–4 s with source In 0.5 s. It proves routing, not fidelity.
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_multi_text_source_keys_26_5_derived.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    assert!(
        !omissions.iter().any(|omission| omission.record == "58"
            && omission.scope == premiere_file::OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group" && layer["playback"]["inputRange"]["start"] == 2000)
        .expect("the derived graphic imports");
    // A plain group clock: layer time is sequence time minus the 2 s start.
    let window = json!({"start": 2000, "duration": 2000});
    assert_eq!(
        group["playback"],
        json!({
            "type": "windowed",
            "inputRange": window,
            "mapping": {
                "type": "linear",
                "input": window,
                "output": {"start": 0, "duration": 2000}
            },
            "inputOffsetMs": 0
        })
    );
    let children = group["layers"].as_array().unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(
        children
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Text", "Text", "Shape"]
    );
    for child in children {
        assert_eq!(child["parent"], group["id"]);
        assert_eq!(child["activeRange"], json!({"start": 0, "duration": 2000}));
    }
    assert_ne!(children[0]["id"], children[1]["id"]);
    assert_ne!(children[0]["id"], children[2]["id"]);
    assert_ne!(children[1]["id"], children[2]["id"]);
    let expected = [
        (
            "TWO",
            "THREE",
            [480.0, 576.0],
            [100.0, 120.0],
            [100.0, 40.0],
        ),
        (
            "SIZE",
            "SIZE",
            [1152.0, 1248.0],
            [90.0, 110.0],
            [80.0, 20.0],
        ),
    ];
    for (text, (first, last, position, scale, opacity)) in children.iter().zip(expected) {
        assert_eq!(text["sourceText"]["text"], first);
        let tracks = valued_tracks(&document, text);
        let track = |property: &str| &tracks.iter().find(|(name, _)| name == property).unwrap().1;
        assert_eq!(
            track("positionX"),
            &vec![
                (0, json!(position[0]), "linear".to_owned()),
                (1000, json!(position[1]), "linear".to_owned())
            ]
        );
        for axis in ["scaleX", "scaleY"] {
            assert_eq!(
                track(axis),
                &vec![
                    (0, json!(scale[0]), "linear".to_owned()),
                    (1000, json!(scale[1]), "linear".to_owned())
                ]
            );
        }
        assert_eq!(
            track("opacity"),
            &vec![
                (0, json!(opacity[0]), "linear".to_owned()),
                (1000, json!(opacity[1]), "linear".to_owned())
            ]
        );
        assert_eq!(
            track("textContent"),
            &held(&[(0, json!(first)), (1000, json!(last))])
        );
    }
    assert!(valued_tracks(&document, &children[2]).is_empty());
    assert_eq!(children[2]["name"], "Rectangle");
    let group_tracks = valued_tracks(&document, group);
    assert_eq!(group_tracks[0].0, "positionX");
    assert_eq!(
        group_tracks[0].1,
        vec![
            (0, json!(960.0), "linear".to_owned()),
            (500, json!(1056.0), "linear".to_owned()),
            (1000, json!(1152.0), "hold".to_owned())
        ]
    );
    assert_eq!(
        document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Group")
            .count(),
        2,
        "the first, unrelated static graphic is preserved"
    );
}

#[test]
fn adobe_graphic_transform_keys_stay_editable_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_graphic_transform_keys_26_5_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        [
            "ClipTrackItem/TrackItem/Node not converted",
            "ClipTrackItem/TrackItem/Node not converted",
            FONT_NOT_PACKAGED,
        ]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    let types: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["Group", "Video", "Rect"]);

    // The graphic on V2 over 1.0-4.0 s: its Vector Motion is the group
    // transform, with the Anchor Point 0.4:0.45 in pixels, and the text
    // starts with the group, so both run on the graphic's clock.
    let (group, text) = graphic(&document);
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start": 1000, "duration": 3000})
    );
    assert_eq!(group["transform"]["anchorPoint"], json!([768.0, 486.0]));
    assert_eq!(text["parent"], group["id"]);
    assert_eq!(text["activeRange"], json!({"start": 0, "duration": 3000}));
    assert_eq!(text["sourceText"]["text"], "py");
    // Layer time is key time minus the InPoint 3601 s. The Scale key before
    // the In and the Opacity key after the Out are kept, and a Hold key
    // holds the value before it.
    let vm_scale = keys(&[(-500, 100.0, "linear"), (1500, 70.0, "linear")]);
    assert_eq!(
        tracks(&document, group),
        [
            (
                "positionX".to_owned(),
                keys(&[
                    (500, 960.0, "linear"),
                    (1500, 1056.0, "linear"),
                    (2500, 1152.0, "hold"),
                ]),
            ),
            (
                "positionY".to_owned(),
                keys(&[
                    (500, 540.0, "linear"),
                    (1500, 486.0, "linear"),
                    (2500, 432.0, "hold"),
                ]),
            ),
            ("scaleX".to_owned(), vm_scale.clone()),
            ("scaleY".to_owned(), vm_scale),
            (
                "rotation".to_owned(),
                keys(&[(1000, 0.0, "linear"), (2000, 20.0, "hold")]),
            ),
        ]
    );
    let text_scale = keys(&[(500, 100.0, "linear"), (2500, 120.0, "linear")]);
    assert_eq!(
        tracks(&document, text),
        [
            (
                "positionX".to_owned(),
                keys(&[(-500, 480.0, "linear"), (2500, 576.0, "linear")]),
            ),
            (
                "positionY".to_owned(),
                keys(&[(-500, 540.0, "linear"), (2500, 594.0, "linear")]),
            ),
            ("scaleX".to_owned(), text_scale.clone()),
            ("scaleY".to_owned(), text_scale),
            (
                "opacity".to_owned(),
                keys(&[(500, 100.0, "linear"), (3500, 40.0, "linear")]),
            ),
        ]
    );

    // Edit the current document: move the Rotation Hold key earlier, change
    // the last Text Scale key and give the Opacity fade a Bezier curve, which
    // converts on Text Opacity.
    let (group, text) = graphic(&document);
    let (group, text) = (group.clone(), text.clone());
    track_mut(&mut document, &group, "rotation")["animator"]["keyframes"][1]["layerTime"] =
        json!(1700);
    for axis in ["scaleX", "scaleY"] {
        track_mut(&mut document, &text, axis)["animator"]["keyframes"][1]["value"]["value"] =
            json!(130.0);
    }
    let curve = json!({"type": "cubicBezier", "x1": 0.4, "y1": 0.1, "x2": 0.75, "y2": 0.95});
    track_mut(&mut document, &text, "opacity")["animator"]["keyframes"][1]["easing"] =
        curve.clone();
    let asset_id = document["composition"]["layers"][1]["source"]["assetId"]
        .as_str()
        .unwrap()
        .to_owned();
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(&asset_id, fixture("video-30fps-10s.mp4"), AssetKind::Video)
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("native");
    tesseract_to_premiere(&edited, &native, false).unwrap();
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(project.sequences().count(), 1);

    // Importing the export again recovers the edited keys on the same
    // clock; the other tracks come back as imported.
    let reimported = root.join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(reasons, [FONT_NOT_PACKAGED]);
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let (group, text) = graphic(&document);
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start": 1000, "duration": 3000})
    );
    let group_tracks = tracks(&document, group);
    let rotation = &group_tracks
        .iter()
        .find(|(name, _)| name == "rotation")
        .unwrap()
        .1;
    assert_eq!(
        rotation,
        &keys(&[(1000, 0.0, "linear"), (1700, 20.0, "hold")])
    );
    let text_tracks = tracks(&document, text);
    let scale = &text_tracks
        .iter()
        .find(|(name, _)| name == "scaleX")
        .unwrap()
        .1;
    assert_eq!(
        scale,
        &keys(&[(500, 100.0, "linear"), (2500, 130.0, "linear")])
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let opacity = entries
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == text["id"] && entry["target"]["propertyType"] == "opacity"
        })
        .unwrap();
    let easing = &opacity["animator"]["keyframes"][1]["easing"];
    assert_eq!(easing["type"], "cubicBezier");
    for handle in ["x1", "y1", "x2", "y2"] {
        let (read, written) = (
            easing[handle].as_f64().unwrap(),
            curve[handle].as_f64().unwrap(),
        );
        assert!(
            (read - written).abs() < 1e-9,
            "{handle}: {read} != {written}"
        );
    }
}

/// The chain of the exported graphic: its `Default*` fields, and the match
/// names of its components in order.
fn graphic_chain(xml: &str) -> (Vec<String>, Vec<String>) {
    let document = roxmltree::Document::parse(xml).unwrap();
    let records: Vec<_> = document.root_element().children().collect();
    let by_id = |id: &str| {
        records
            .iter()
            .find(|record| record.attribute("ObjectID") == Some(id))
            .unwrap_or_else(|| panic!("record {id}"))
    };
    let match_name = |component: &roxmltree::Node<'_, '_>| {
        component
            .children()
            .find(|node| node.has_tag_name("MatchName"))
            .and_then(|node| node.text())
            .unwrap_or_default()
            .to_owned()
    };
    for chain in records
        .iter()
        .filter(|record| record.has_tag_name("VideoComponentChain"))
    {
        let components: Vec<_> = chain
            .descendants()
            .filter(|node| node.has_tag_name("Component"))
            .filter_map(|node| node.attribute("ObjectRef"))
            .map(|id| match_name(by_id(id)))
            .collect();
        if components.iter().any(|name| name == "AE.ADBE Text") {
            let defaults = chain
                .children()
                .filter(|node| node.tag_name().name().starts_with("Default"))
                .map(|node| node.tag_name().name().to_owned())
                .collect();
            return (defaults, components);
        }
    }
    panic!("no graphic chain")
}

#[test]
fn adobe_graphic_clip_opacity_keys_stay_editable_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_graphic_clip_opacity_keys_26_5_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        [
            "ClipTrackItem/TrackItem/Node not converted",
            "ClipTrackItem/TrackItem/Node not converted",
            FONT_NOT_PACKAGED,
        ]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    let types: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["Group", "Video", "Rect"]);

    // The graphic on V2 over 1.0-4.0 s, with its InPoint at 0: its keyed clip
    // Opacity fades the whole graphic, so it is the opacity of a group whose
    // transform is otherwise the identity; the text keeps its own transform.
    let (group, text) = graphic(&document);
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start": 1000, "duration": 3000})
    );
    assert_eq!(
        (
            &group["transform"]["position"],
            &group["transform"]["anchorPoint"],
            &group["transform"]["scale"],
            &group["transform"]["opacity"],
        ),
        (
            &json!([0.0, 0.0]),
            &json!([0.0, 0.0]),
            &json!([100.0, 100.0]),
            &json!(100.0)
        )
    );
    assert_eq!(text["parent"], group["id"]);
    assert_eq!(text["activeRange"], json!({"start": 0, "duration": 3000}));
    assert_eq!(text["transform"]["opacity"], json!(100.0));
    // Premiere's keys: 100 Linear at 0.5 s, 0 Hold at 1.5 s, 70 at 2.5 s.
    assert_eq!(
        tracks(&document, group),
        [(
            "opacity".to_owned(),
            keys(&[
                (500, 100.0, "linear"),
                (1500, 0.0, "linear"),
                (2500, 70.0, "hold"),
            ]),
        )]
    );
    assert!(tracks(&document, text).is_empty());

    let group = group.clone();
    let asset_id = document["composition"]["layers"][1]["source"]["assetId"]
        .as_str()
        .unwrap()
        .to_owned();
    let export = |document: &Value, name: &str| {
        let edited = root.join(format!("{name}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(document).unwrap())
            .unwrap()
            .add_asset(&asset_id, fixture("video-30fps-10s.mp4"), AssetKind::Video)
            .unwrap()
            .write(&edited)
            .unwrap();
        let native = root.join(name);
        let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
        (native, omissions)
    };
    let curve = json!({"type": "cubicBezier", "x1": 0.4, "y1": 0.1, "x2": 0.75, "y2": 0.95});

    // A Bezier curve into a key that starts a Hold cannot be written: Premiere
    // ignores that key's in-handle. So moving the Hold key and easing into it
    // exports the clip Opacity's static value and reports its keys.
    let mut into_hold = document.clone();
    let opacity = track_mut(&mut into_hold, &group, "opacity");
    opacity["animator"]["keyframes"][1]["layerTime"] = json!(1250);
    opacity["animator"]["keyframes"][1]["easing"] = curve.clone();
    let (native, omissions) = export(&into_hold, "into-hold");
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        ["Opacity animation was not exported: unsupported conversion: Opacity cubic easing into a key that starts a Hold cannot keep its in-handle, which Premiere ignores"]
    );
    let (defaults, components) = graphic_chain(&read_xml(&native.join("project.prproj")));
    assert_eq!(
        defaults,
        [
            "DefaultMotion",
            "DefaultOpacity",
            "DefaultMotionComponentID",
            "DefaultOpacityComponentID"
        ]
    );
    assert_eq!(components, ["AE.ADBE Text"]);

    // Edit the current document: move the Hold key earlier, change the last
    // value and turn the Hold after it into a Bezier curve that ends on the
    // last key, which is written Linear and converts on the clip Opacity.
    let opacity = track_mut(&mut document, &group, "opacity");
    opacity["animator"]["keyframes"][1]["layerTime"] = json!(1250);
    opacity["animator"]["keyframes"][2]["value"]["value"] = json!(60.0);
    opacity["animator"]["keyframes"][2]["easing"] = curve.clone();
    let (native, omissions) = export(&document, "native");
    assert!(omissions.is_empty(), "{omissions:?}");
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(project.sequences().count(), 1);
    // The export keeps the clip Opacity as Premiere 26.5.1 saved it: first in
    // the graphic's chain, without `DefaultOpacity`.
    let (defaults, components) = graphic_chain(&read_xml(&native.join("project.prproj")));
    assert_eq!(defaults, ["DefaultMotion", "DefaultMotionComponentID"]);
    assert_eq!(components, ["AE.ADBE Opacity", "AE.ADBE Text"]);

    // Importing the export again recovers the edited keys on the same clock.
    let reimported = root.join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(reasons, [FONT_NOT_PACKAGED]);
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let (group, _) = graphic(&document);
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start": 1000, "duration": 3000})
    );
    assert_eq!(
        tracks(&document, group),
        [(
            "opacity".to_owned(),
            keys(&[
                (500, 100.0, "linear"),
                (1250, 0.0, "linear"),
                (2500, 60.0, "cubicBezier"),
            ]),
        )]
    );
    let opacity = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["layerId"] == group["id"])
        .unwrap();
    let easing = &opacity["animator"]["keyframes"][2]["easing"];
    for handle in ["x1", "y1", "x2", "y2"] {
        let (read, written) = (
            easing[handle].as_f64().unwrap(),
            curve[handle].as_f64().unwrap(),
        );
        assert!(
            (read - written).abs() < 1e-9,
            "{handle}: {read} != {written}"
        );
    }
}

/// A layer as (type, name, hidden) with its layers, for structure checks.
fn outline(layer: &Value) -> (String, String, bool, Vec<(String, String)>) {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let children = layer["layers"].as_array().map_or_else(Vec::new, |layers| {
        layers
            .iter()
            .map(|child| (text(&child["type"]), text(&child["name"])))
            .collect()
    });
    (
        text(&layer["type"]),
        text(&layer["name"]),
        layer["isHidden"] == true,
        children,
    )
}

#[test]
fn adobe_graphic_shapes_and_objects_stay_editable_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_graphic_shapes_26_5_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    // G9's ID-only TrackItem Node comes from the Premiere 26.5.1 save.
    assert_eq!(
        reasons,
        [
            "ClipTrackItem/TrackItem/Node not converted",
            FONT_NOT_PACKAGED
        ]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .clone();
    let owned = |(kind, name): (&str, &str)| (kind.to_owned(), name.to_owned());
    let group = |children: &[(&str, &str)], hidden: bool, index: usize| {
        (
            "Group".to_owned(),
            format!("Premiere graphic {index}"),
            hidden,
            children.iter().copied().map(owned).collect::<Vec<_>>(),
        )
    };
    let shape = |name: &str| ("Shape".to_owned(), name.to_owned(), false, Vec::new());
    let text_and_rectangle = [("Text", "py"), ("Shape", "Rectangle")];
    // G1-G9 on V2 at 0-9 s: several objects make a group whose layers are in
    // paint order (the first listed in front); one Shape is a shape layer.
    assert_eq!(
        layers.iter().take(9).map(outline).collect::<Vec<_>>(),
        [
            group(&text_and_rectangle, false, 2),
            group(&[("Shape", "Rectangle"), ("Text", "py")], false, 3),
            group(&[("Text", "py"), ("Shape", "Ellipse")], false, 4),
            group(&[("Text", "py"), ("Text", "ok")], false, 5),
            shape("Outline"),
            group(&[("Shape", "Box"), ("Text", "py")], false, 7),
            shape("Scaled"),
            shape("Shadowed"),
            group(&text_and_rectangle, true, 10),
        ]
    );
    let rectangle = &layers[0]["layers"][1];
    assert_eq!(rectangle["transform"]["position"], json!([672.0, 540.0]));
    assert_eq!(
        rectangle["shape"]["path"]["commands"][1],
        json!({"type": "lineTo", "x": 100.0, "y": -90.0})
    );
    assert_eq!(
        rectangle["shape"]["fills"][0]["paint"]["color"],
        json!([0.0, 96.0 / 255.0, 1.0, 1.0])
    );
    let ellipse = &layers[2]["layers"][1]["shape"];
    let commands: Vec<_> = ellipse["path"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        commands,
        ["moveTo", "cubicTo", "cubicTo", "cubicTo", "cubicTo", "close"]
    );
    let stroke = &ellipse["strokes"][0];
    assert_eq!(
        (&stroke["width"], &stroke["join"], &stroke["cap"]),
        (&json!(24.0), &json!("miter"), &json!("butt"))
    );
    // G5 draws no fill (Appearance slot 1 = 0), only its centred stroke.
    let outline_shape = &layers[4]["shape"];
    assert_eq!(outline_shape.get("fills"), None);
    assert_eq!(outline_shape["strokes"][0]["width"], json!(24.0));
    assert_eq!(
        (
            &layers[4]["transform"]["position"],
            &layers[4]["transform"]["anchorPoint"]
        ),
        (&json!([768.0, 378.0]), &json!([96.0, 108.0]))
    );
    // G6's static Vector Motion is the group transform.
    let motion = &layers[5]["transform"];
    assert_eq!(
        (
            &motion["position"],
            &motion["anchorPoint"],
            &motion["scale"],
            &motion["rotation"]
        ),
        (
            &json!([960.0, 540.0]),
            &json!([864.0, 594.0]),
            &json!([80.0, 80.0]),
            &json!(15.0)
        )
    );
    // G7 scales x by Horizontal Scale and y by Scale, before rotating.
    assert_eq!(
        (
            &layers[6]["transform"]["scale"],
            &layers[6]["transform"]["rotation"]
        ),
        (&json!([150.0, 60.0]), &json!(30.0))
    );
    // G8's shadow: distance 50 at 45° down and to the right, size 20.
    let shadow = &layers[7]["effects"][0]["effect"];
    assert_eq!(shadow["type"], "dropShadow");
    let offset = shadow["offset"].as_array().unwrap();
    assert!(offset
        .iter()
        .all(|axis| (axis.as_f64().unwrap() - 50.0 / 2_f64.sqrt()).abs() < 1e-9));
    assert_eq!(
        (&shadow["spreadRadius"], &shadow["blurRadius"]),
        (&json!(9.78), &json!(0.0))
    );

    // Edit: move a rectangle vertex in G1, change the ellipse fill in G3 and
    // swap the objects of G2.
    let composition = &mut document["composition"]["layers"];
    composition[0]["layers"][1]["shape"]["path"]["commands"][1]["x"] = json!(140.0);
    composition[2]["layers"][1]["shape"]["fills"][0]["paint"]["color"] =
        json!([1.0, 0.0, 0.0, 1.0]);
    composition[1]["layers"].as_array_mut().unwrap().reverse();
    let asset_id = layers[9]["source"]["assetId"].as_str().unwrap().to_owned();
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(&asset_id, fixture("video-30fps-10s.mp4"), AssetKind::Video)
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(project.sequences().count(), 1);

    // The reimported export keeps the edits and every other graphic.
    let reimported = root.join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(reasons, [FONT_NOT_PACKAGED]);
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let reread = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        reread.iter().take(9).map(outline).collect::<Vec<_>>()[1],
        group(&[("Text", "py"), ("Shape", "Rectangle")], false, 3)
    );
    assert_eq!(
        reread[0]["layers"][1]["shape"]["path"]["commands"][1],
        json!({"type": "lineTo", "x": 140.0, "y": -90.0})
    );
    assert_eq!(
        reread[2]["layers"][1]["shape"]["fills"][0]["paint"]["color"],
        json!([1.0, 0.0, 0.0, 1.0])
    );
    for index in [3, 4, 5, 6, 7, 8] {
        assert_eq!(
            outline(&reread[index]),
            outline(&layers[index]),
            "G{}",
            index + 1
        );
    }
}

/// One key of a track whose values are not all numbers: (layer ms, value,
/// easing type).
type ValuedKey = (i64, Value, String);

/// The key tracks of `layer`, by property type, with values of any kind.
fn valued_tracks(document: &Value, layer: &Value) -> Vec<(String, Vec<ValuedKey>)> {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == layer["id"])
        .map(|entry| {
            let keys = entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| {
                    (
                        key["layerTime"].as_i64().unwrap(),
                        key["value"]["value"].clone(),
                        key["easing"]["type"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            (
                entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                keys,
            )
        })
        .collect()
}

/// The key times of every keyed Source Text record of an exported project,
/// in record order, checking the record's form as Premiere 26.5.1 saves it.
fn source_text_key_times(xml: &str) -> Vec<Vec<i64>> {
    let document = roxmltree::Document::parse(xml).unwrap();
    document
        .root_element()
        .children()
        .filter(|record| {
            record.has_tag_name("ArbVideoComponentParam")
                && record
                    .children()
                    .any(|node| node.has_tag_name("Name") && node.text() == Some("Source Text"))
        })
        .filter_map(|record| {
            let child = |name: &str| {
                record
                    .children()
                    .find(|node| node.has_tag_name(name))
                    .and_then(|node| node.text())
            };
            let keys = child("Keyframes")?;
            assert_eq!(record.attribute("Version"), Some("3"));
            assert_eq!(child("IsTimeVarying"), Some("true"));
            assert!(child("StartKeyframeValue").is_some());
            Some(
                keys.split_terminator(';')
                    .map(|key| {
                        let [ticks, payload] = key.split(',').collect::<Vec<_>>()[..] else {
                            panic!("two fields per key: {key}");
                        };
                        assert!(!payload.is_empty());
                        ticks.parse().unwrap()
                    })
                    .collect(),
            )
        })
        .collect()
}

fn held(list: &[(i64, Value)]) -> Vec<ValuedKey> {
    list.iter()
        .map(|(time, value)| (*time, value.clone(), "hold".to_owned()))
        .collect()
}

#[test]
fn adobe_source_text_hold_keys_stay_editable_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_text_hold_keys_gradient_26_5_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    // Three ID-only TrackItem Nodes come from the Premiere 26.5.1 saves. C
    // and D are the gradient probe's shapes, saved with the unmeasured
    // Appearance slot 8 (JRB-2015); each omits its own occurrence.
    assert_eq!(
        reasons,
        [
            "ClipTrackItem/TrackItem/Node not converted",
            "ClipTrackItem/TrackItem/Node not converted",
            "ClipTrackItem/TrackItem/Node not converted",
            "unsupported conversion: ArbVideoComponentParam:171: unsupported Appearance slot 8",
            "unsupported conversion: ArbVideoComponentParam:189: unsupported Appearance slot 8",
            FONT_NOT_PACKAGED,
        ]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .clone();
    let names: Vec<_> = layers
        .iter()
        .map(|layer| {
            (
                layer["type"].as_str().unwrap(),
                layer["name"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("Text", "A"),
            ("Text", "B"),
            ("Video", "Premiere video 1"),
            ("Rect", "Premiere black canvas"),
        ]
    );
    let (a, b) = (&layers[0], &layers[1]);
    // A on V2 at 1-4 s from In 3601 s: keys TWO at 3601.5 s and THREE at
    // 3602.5 s hold the text; the first key's document is the text before
    // it (Premiere renders TWO from the first frame, not the saved
    // StartKeyframeValue ONE).
    assert_eq!(a["activeRange"], json!({"start": 1000, "duration": 3000}));
    assert_eq!(a["sourceText"]["text"], "TWO");
    assert_eq!(a["sourceText"]["fontSize"], 100.0);
    assert_eq!(
        valued_tracks(&document, a),
        [(
            "textContent".to_owned(),
            held(&[(500, json!("TWO")), (1500, json!("THREE"))])
        )]
    );
    // B at 4.5-7.5 s: the same text SIZE in red 140 px, then with a black
    // stroke (native width 1.5, FX 3 px) at 3602.5 s. Only the stroke switch
    // differs, so it keys the stroke switch beside the text, and the stroke
    // is the layer's; the Position keys authored beside the Source Text
    // keys are the same Linear tracks as without them.
    assert_eq!(b["activeRange"], json!({"start": 4500, "duration": 3000}));
    assert_eq!(b["sourceText"]["text"], "SIZE");
    assert_eq!(b["sourceText"]["fontSize"], 140.0);
    assert_eq!(b["sourceText"]["fillColor"], json!([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(b["sourceText"]["applyStroke"], json!(false));
    assert_eq!(b["sourceText"]["strokeColor"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(b["sourceText"]["strokeWidth"], 3.0);
    assert_eq!(
        valued_tracks(&document, b),
        [
            (
                "positionX".to_owned(),
                vec![
                    (1000, json!(768.0), "linear".to_owned()),
                    (2000, json!(1152.0), "linear".to_owned()),
                ]
            ),
            (
                "positionY".to_owned(),
                vec![
                    (1000, json!(540.0), "linear".to_owned()),
                    (2000, json!(540.0), "linear".to_owned()),
                ]
            ),
            (
                "textContent".to_owned(),
                held(&[(500, json!("SIZE")), (1500, json!("SIZE"))])
            ),
            (
                "strokeEnabled".to_owned(),
                held(&[(500, json!(false)), (1500, json!(true))])
            ),
        ]
    );

    // Edit: A's second text becomes FOUR, and B's stroke switches on 500 ms
    // earlier, at a time its text track lacks. B's static position becomes
    // its first Position key, which FX shows before that key anyway: the
    // Position keys export only from a static value equal to the first key.
    track_mut(&mut document, a, "textContent")["animator"]["keyframes"][1]["value"]["value"] =
        json!("FOUR");
    track_mut(&mut document, b, "strokeEnabled")["animator"]["keyframes"][1]["layerTime"] =
        json!(1000);
    document["composition"]["layers"][1]["transform"]["position"] = json!([768.0, 540.0]);
    let asset_id = layers[2]["source"]["assetId"].as_str().unwrap().to_owned();
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(&asset_id, fixture("video-30fps-10s.mp4"), AssetKind::Video)
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // The export writes the Source Text records in the saved form: version
    // 3, IsTimeVarying, and one `ticks,base64;` key per key time of every
    // Source Text track, on the generator clock of the placement at 3600 s.
    assert_eq!(project.sequences().count(), 1);
    let key_times = source_text_key_times(&read_xml(&native.join("project.prproj")));
    const HOUR: i64 = 3600 * 254_016_000_000;
    const HALF_SECOND: i64 = 254_016_000_000 / 2;
    assert_eq!(
        key_times,
        [
            vec![HOUR + HALF_SECOND, HOUR + 3 * HALF_SECOND],
            vec![
                HOUR + HALF_SECOND,
                HOUR + 2 * HALF_SECOND,
                HOUR + 3 * HALF_SECOND
            ],
        ]
    );

    // Importing the export again recovers the edited keys: B's stroke
    // switch now at 1000 ms, and its text held over three keys.
    let reimported = root.join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(reasons, [FONT_NOT_PACKAGED]);
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let text_layer = |name: &str| {
        layers
            .iter()
            .find(|layer| layer["name"] == name)
            .unwrap_or_else(|| panic!("text layer {name}"))
    };
    let (a, b) = (text_layer("A"), text_layer("B"));
    assert_eq!(a["activeRange"], json!({"start": 1000, "duration": 3000}));
    assert_eq!(
        valued_tracks(&document, a),
        [(
            "textContent".to_owned(),
            held(&[(500, json!("TWO")), (1500, json!("FOUR"))])
        )]
    );
    // B's Position keys come back beside its Source Text keys.
    let b_tracks = valued_tracks(&document, b);
    assert_eq!(
        b_tracks
            .iter()
            .map(|(name, keys)| (name.as_str(), keys.len()))
            .collect::<Vec<_>>(),
        [
            ("positionX", 2),
            ("positionY", 2),
            ("textContent", 3),
            ("strokeEnabled", 3),
        ]
    );
    assert_eq!(
        b_tracks[2..],
        [
            (
                "textContent".to_owned(),
                held(&[
                    (500, json!("SIZE")),
                    (1000, json!("SIZE")),
                    (1500, json!("SIZE"))
                ])
            ),
            (
                "strokeEnabled".to_owned(),
                held(&[
                    (500, json!(false)),
                    (1000, json!(true)),
                    (1500, json!(true))
                ])
            ),
        ]
    );
    assert_eq!(b["sourceText"]["strokeWidth"], 3.0);
}

/// A's keyed text with a drop shadow exports beside its siblings B and the
/// video, and the shadow and keys read back. Before the fix the shadow reached
/// only the static document, so the project failed validation and no sibling
/// exported.
#[test]
fn adobe_source_text_hold_keys_export_with_a_shadow_beside_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    premiere_to_tesseract(
        fixture("feature_text_hold_keys_gradient_26_5_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let a = layers
        .iter_mut()
        .find(|layer| layer["name"] == "A")
        .unwrap();
    a["effects"] = json!([{"id": 90, "effect": {"type": "dropShadow", "color": [0, 0, 0, 1], "offset": [4, 4], "blurRadius": 0}}]);
    let b = layers
        .iter_mut()
        .find(|layer| layer["name"] == "B")
        .unwrap();
    b["transform"]["position"] = json!([768.0, 540.0]);
    let asset_id = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap()["source"]["assetId"]
        .as_str()
        .unwrap()
        .to_owned();
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(&asset_id, fixture("video-30fps-10s.mp4"), AssetKind::Video)
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(project.sequences().count(), 1);
    assert_eq!(
        source_text_key_times(&read_xml(&native.join("project.prproj")))
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        [2, 2]
    );

    let reimported = root.join("reimported");
    premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let names: Vec<_> = layers
        .iter()
        .map(|layer| layer["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["A", "B", "Premiere video 1", "Premiere black canvas"]
    );
    let a = &layers[0];
    let [effect] = a["effects"].as_array().unwrap().as_slice() else {
        panic!("A keeps one effect: {a}");
    };
    assert_eq!(effect["effect"]["type"], "dropShadow");
    // The offset comes back through Premiere's angle and distance.
    let offset: Vec<f64> = effect["effect"]["offset"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_f64().unwrap())
        .collect();
    assert!(
        offset.iter().all(|axis| (axis - 4.0).abs() < 1e-6),
        "{offset:?}"
    );
    assert_eq!(
        valued_tracks(&document, a),
        [(
            "textContent".to_owned(),
            held(&[(500, json!("TWO")), (1500, json!("THREE"))])
        )]
    );
    assert!(layers[1].get("effects").is_none(), "B has no shadow");
}

/// The warning of a gradient with opacity stops, in both directions.
const OPACITY_STOPS_WARNING: &str = "gradient opacity stops composite in about gamma-2.4 light in Premiere; converted as FX stop alpha, which composites in encoded RGB, so partly transparent areas render darker (on the fixture's ramp 10 levels RMSE over a dark backdrop, up to 89 over bright content)";

/// The gradient paint of the shape layer named `name`.
fn gradient_paint<'a>(layers: &'a [Value], name: &str) -> &'a Value {
    let layer = layers
        .iter()
        .find(|layer| layer["name"] == name)
        .unwrap_or_else(|| panic!("shape {name}"));
    &layer["shape"]["fills"][0]["paint"]
}

#[test]
fn adobe_gradient_fills_stay_editable_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_gradient_fills_26_5_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    // D's opacity ramp converts with its warning; the ID-only TrackItem
    // Nodes come from the Premiere 26.5.1 save.
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .filter(|reason| *reason != "ClipTrackItem/TrackItem/Node not converted")
        .collect();
    assert_eq!(reasons, [OPACITY_STOPS_WARNING]);
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .clone();
    let blue = json!([0.0, 96.0 / 255.0, 254.0 / 255.0, 1.0]);
    // A stays the solid control; B and C keep their saved geometry, stops
    // and C's middle stop at 0.49922094 (Oracle run 23, G1-G3).
    assert_eq!(
        layers[0]["shape"]["fills"][0]["paint"],
        json!({"type": "solid", "color": blue})
    );
    assert_eq!(
        *gradient_paint(&layers, "B"),
        json!({
            "type": "gradient", "gradientType": "linear", "start": [-150.0, 0.0], "end": [150.0, 0.0],
            "stops": [
                {"offset": 0.0, "color": blue},
                {"offset": 1.0, "color": [0.0, 200.0 / 255.0, 0.0, 1.0]}
            ]
        })
    );
    assert_eq!(
        *gradient_paint(&layers, "C"),
        json!({
            "type": "gradient", "gradientType": "radial", "start": [-150.0, 0.0], "end": [150.0, 0.0],
            "stops": [
                {"offset": 0.0, "color": [1.0, 1.0, 1.0, 1.0]},
                {"offset": f64::from(f32::from_bits(0x3eff_99e3)), "color": blue},
                {"offset": 1.0, "color": [0.0, 0.0, 0.0, 1.0]}
            ]
        })
    );
    // D is B's ramp fading out under its centred black 12 px stroke (G5).
    let d = layers.iter().find(|layer| layer["name"] == "D").unwrap();
    assert_eq!(
        d["shape"]["fills"][0]["paint"]["stops"],
        json!([
            {"offset": 0.0, "color": blue},
            {"offset": 1.0, "color": [0.0, 200.0 / 255.0, 0.0, 0.0]}
        ])
    );
    assert_eq!(
        (
            &d["shape"]["strokes"][0]["paint"]["color"],
            &d["shape"]["strokes"][0]["width"]
        ),
        (&json!([0.0, 0.0, 0.0, 1.0]), &json!(12.0))
    );

    // Edit: B's end color, C's middle stop, D's end opacity, and a new
    // linear gradient on a copy of A beside D.
    let composition = document["composition"]["layers"].as_array_mut().unwrap();
    let edited_linear = json!({
        "type": "gradient", "gradientType": "linear", "start": [-300.0, 0.0], "end": [300.0, 0.0],
        "stops": [
            {"offset": 0.0, "color": [1.0, 1.0, 0.0, 1.0]},
            {"offset": 1.0, "color": [1.0, 0.0, 1.0, 1.0]}
        ]
    });
    let mut added = composition[0].clone();
    added["id"] = json!(6);
    added["name"] = json!("E");
    added["activeRange"] = json!({"start": 7500, "duration": 2500});
    added["shape"]["fills"][0]["paint"] = edited_linear.clone();
    for layer in composition.iter_mut() {
        match layer["name"].as_str() {
            Some("B") => {
                layer["shape"]["fills"][0]["paint"]["stops"][1]["color"] =
                    json!([1.0, 0.0, 0.0, 1.0]);
            }
            Some("C") => layer["shape"]["fills"][0]["paint"]["stops"][1]["offset"] = json!(0.3),
            Some("D") => layer["shape"]["fills"][0]["paint"]["stops"][1]["color"][3] = json!(0.5),
            _ => {}
        }
    }
    composition.insert(4, added);
    let asset_id = layers[4]["source"]["assetId"].as_str().unwrap().to_owned();
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            &asset_id,
            fixture("feature_timecoded_source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
        .collect();
    assert_eq!(reasons, [("layer 5 (\"D\")", OPACITY_STOPS_WARNING)]);

    // The reimported export keeps the edits.
    let reimported = root.join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(reasons, [OPACITY_STOPS_WARNING]);
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let reread = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        gradient_paint(reread, "B")["stops"][1]["color"],
        json!([1.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(
        gradient_paint(reread, "C")["stops"][1]["offset"],
        json!(f64::from(0.3_f32))
    );
    assert_eq!(
        gradient_paint(reread, "D")["stops"][1]["color"],
        json!([0.0, 200.0 / 255.0, 0.0, 0.5])
    );
    assert_eq!(*gradient_paint(reread, "E"), edited_linear);
    let a = reread.iter().find(|layer| layer["name"] == "A").unwrap();
    assert_eq!(
        a["shape"]["fills"][0]["paint"],
        json!({"type": "solid", "color": blue})
    );
}
