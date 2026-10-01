//! Track Matte Key (JRB-2023) through the public conversion API: an FX video
//! keyed by a sibling video exports as a Premiere clip with a Track Matte Key
//! whose matte is the source's clip on the track above, and the written
//! project imports back to the same key; Premiere 26.5.1's own save of eight
//! keyed clips imports clip by clip and writes its keys back.

use super::support::*;
use premiere_file::{OmissionScope, PrProjectFile};
use serde_json::{json, Value};
use std::path::Path;
use tesseract_file::TesseractFile;

#[test]
fn adobe_transform_moves_the_alpha_keyed_picture_in_both_saved_orders() {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_transform_track_matte_26_5_strict.prproj");
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("18832324-570e-4e73-8460-84b8c8150813"),
        false,
    )
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.contains("Transform")),
        "{omissions:#?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let mut groups: Vec<_> = layers
        .iter()
        .filter(|layer| layer.get("trackMatte").is_some())
        .collect();
    groups.sort_by_key(|layer| {
        (*crate::test_support::layer_range(layer))["start"]
            .as_i64()
            .unwrap()
    });
    assert_eq!(groups.len(), 2);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 4);
    for (group, start) in groups.into_iter().zip([0, 2500]) {
        assert_eq!(group["type"], "Group");
        assert_eq!(
            (*crate::test_support::layer_range(group)),
            json!({"start": start, "duration": 2500})
        );
        assert_eq!(group["transform"]["position"], json!([960.0, 540.0]));
        assert_eq!(group["transform"]["anchorPoint"], json!([960.0, 540.0]));
        let children = group["layers"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        let video = children
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(
            (*crate::test_support::layer_range(video)),
            json!({"start": 0, "duration": 2500})
        );
        assert_eq!(video["transform"]["position"], json!([0.0, 0.0]));
        assert_eq!(video["transform"]["anchorPoint"], json!([0.0, 0.0]));
        assert!(video.get("trackMatte").is_none());
        let matte = children
            .iter()
            .find(|layer| layer["id"] == group["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(matte["type"], "Image");
        assert_eq!(
            (*crate::test_support::layer_range(matte)),
            json!({"start": 0, "duration": 2500})
        );
        assert_eq!(group["trackMatte"]["mode"], "alpha");
        for (property, values) in [
            ("positionX", [960.0, 1152.0]),
            ("positionY", [540.0, 594.0]),
        ] {
            let track = entries
                .iter()
                .find(|entry| {
                    entry["target"]["layerId"] == group["id"]
                        && entry["target"]["propertyType"] == property
                })
                .unwrap();
            let keys = track["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2);
            for ((key, time), value) in keys.iter().zip([500, 1500]).zip(values) {
                assert_eq!(key["layerTime"], time);
                assert_eq!(key["value"]["value"], value);
                assert_eq!(key["easing"]["type"], "linear");
            }
        }
    }
    assert!(unconsumed_images(&document).is_empty());
}

#[test]
fn unconverted_native_effects_cannot_enter_the_measured_transform_matte_pair() {
    use std::io::Write;
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let original = read_xml(&fixtures.join("feature_transform_track_matte_26_5_strict.prproj"));
    for (record, expected) in [
        (
            "92",
            "another active or unconverted effect is outside measured A4",
        ),
        ("122", "static matte with no active native effects"),
        ("114", "AE.ADBE Geometry2"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "feature_linked_av_source.mp4",
            "feature_timecoded_source.mp4",
            "tmk_alpha_rect.png",
        ] {
            std::fs::copy(fixtures.join(name), dir.path().join(name)).unwrap();
        }
        let parsed = roxmltree::Document::parse(&original).unwrap();
        let node = parsed
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(record))
            .unwrap();
        let before = &original[node.range()];
        let after = if record == "114" {
            before.replace("AE.ADBE Geometry</", "AE.ADBE Geometry2</")
        } else if before.contains("</Components>") {
            before.replace(
                "</Components>",
                "<Component Index=\"2\" ObjectRef=\"900\"/></Components>",
            )
        } else {
            before.replace("</ComponentChain>", "<Components Version=\"1\"><Component Index=\"0\" ObjectRef=\"900\"/></Components></ComponentChain>")
        };
        let mut xml = original.clone();
        xml.replace_range(node.range(), &after);
        if record != "114" {
            let end = xml.rfind("</").unwrap();
            xml.insert_str(end, "<VideoFilterComponent ObjectID=\"900\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><ID>99</ID><DisplayName>Own unsupported probe</DisplayName><Bypass>false</Bypass></Component><VideoFilterType>2</VideoFilterType><MatchName>Own.Unsupported.Probe</MatchName></VideoFilterComponent>");
        }
        let path = dir.path().join("negative.prproj");
        let mut writer = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        writer.write_all(xml.as_bytes()).unwrap();
        writer.finish().unwrap();
        let output = dir.path().join("converted");
        let omissions = premiere_to_tesseract(
            &path,
            &output,
            Some("18832324-570e-4e73-8460-84b8c8150813"),
            false,
        )
        .unwrap();
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(expected)),
            "{record}: {omissions:#?}"
        );
        let document = TesseractFile::open(first_project(&output))
            .unwrap()
            .project_json()
            .unwrap();
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        assert_eq!(
            entries.len(),
            2,
            "only the unchanged second pair retains its Position tracks"
        );
        let keyed: Vec<_> = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer.get("trackMatte").is_some())
            .collect();
        assert_eq!(
            keyed.len(),
            2,
            "{record}: both keyed fills must survive unsupported Transform admission"
        );
        if record == "122" {
            assert!(
                omissions
                    .iter()
                    .any(|omission| omission.scope == OmissionScope::Feature
                        && omission.reason.contains("Transform omitted")),
                "{omissions:#?}"
            );
        }
        assert!(unconsumed_images(&document).is_empty());
    }
}

/// The flat document: video 1 over 0 to 1 s at Premiere's default Motion,
/// keyed by the luma of video 3, the same source over the same range, listed
/// below it.
fn keyed_document(dir: &Path) -> Value {
    let mut document = document(dir);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["transform"] = json!({"anchorPoint": [960, 540], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100});
    layers[0]["trackMatte"] = json!({"mode": "luma", "layer": 3});
    let mut source = layers[0].clone();
    source["id"] = json!(3);
    source["name"] = json!("Matte");
    source.as_object_mut().unwrap().remove("trackMatte");
    layers.insert(1, source);
    document
}

/// The `(ParameterID, StartKeyframe)` of each Track Matte Key of `project`
/// in document order, each in `ParameterID` order, with the `Track/ID` of
/// every video track.
fn native_keys(project: &Path) -> (Vec<Vec<(String, String)>>, Vec<String>) {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let child_text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let keys: Vec<Vec<(String, String)>> = root
        .children()
        .filter(|node| {
            child_text(*node, "MatchName").as_deref() == Some("AE.ADBE Legacy Key Track Matte")
        })
        .map(|key| {
            assert_eq!(key.attribute("Version"), Some("7"));
            let mut params: Vec<(String, String)> = key
                .descendants()
                .filter(|node| node.has_tag_name("Param"))
                .map(|param| {
                    let param = root
                        .children()
                        .find(|node| node.attribute("ObjectID") == param.attribute("ObjectRef"))
                        .unwrap();
                    (
                        child_text(param, "ParameterID").unwrap(),
                        child_text(param, "StartKeyframe").unwrap(),
                    )
                })
                .collect();
            params.sort();
            params
        })
        .collect();
    assert!(!keys.is_empty(), "a Track Matte Key component");
    let track_ids = root
        .children()
        .filter(|node| node.has_tag_name("VideoClipTrack"))
        .map(|track| {
            track
                .descendants()
                .find(|node| node.has_tag_name("ID"))
                .and_then(|id| id.text())
                .unwrap()
                .to_owned()
        })
        .collect();
    (keys, track_ids)
}

/// The static Matte, Composite Using and Reverse of each written key.
fn key_values(keys: &[Vec<(String, String)>]) -> Vec<[String; 3]> {
    keys.iter()
        .map(|params| {
            let value = |id: &str| {
                let (_, start) = params.iter().find(|(param, _)| param == id).unwrap();
                start.split(',').nth(1).unwrap().to_owned()
            };
            [value("1"), value("2"), value("3")]
        })
        .collect()
}

#[test]
fn an_edited_track_matte_exports_as_the_key_and_reimports_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let document = keyed_document(root);
    let edited = archive(root, &document, &root.join("source.mp4"));
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    // The key names the matte track by its ID, the second track's 2, and
    // writes Matte Luma and Reverse off.
    let (keys, track_ids) = native_keys(&exported);
    assert_eq!(
        keys,
        [[
            (
                "1".to_owned(),
                "-91445760000000000,2,0,0,0,0,0,0".to_owned()
            ),
            (
                "2".to_owned(),
                "-91445760000000000,1,0,0,0,0,0,0".to_owned()
            ),
            (
                "3".to_owned(),
                "-91445760000000000,false,0,0,0,0,0,0".to_owned()
            ),
        ]]
    );
    assert_eq!(track_ids, ["1", "2"]);
    let (project, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let tracks: Vec<_> = project
        .sequences()
        .next()
        .unwrap()
        .video_tracks()
        .map(|track| track.len())
        .collect();
    assert_eq!(tracks, [1, 1]);

    // Reimport: the keyed video names the matte's layer, which stays a
    // sibling, and the matte is not keyed itself; Matte Luma is reported as
    // the Rec. 601 approximation (fixture G2).
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.scope, omission.record.as_str()))
            .collect::<Vec<_>>(),
        [(OmissionScope::Feature, "VideoClipTrackItem:79")],
        "{omissions:?}"
    );
    assert!(omissions[0].reason.starts_with(
        "Matte Luma is approximated: Premiere weights the matte's encoded RGB by Rec. 601"
    ));
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = reimported["composition"]["layers"].as_array().unwrap();
    let videos: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    let [matte, keyed] = videos.as_slice() else {
        panic!("{layers:?}");
    };
    assert_eq!(
        keyed["trackMatte"],
        json!({"mode": "luma", "layer": matte["id"]})
    );
    assert!(matte.get("trackMatte").is_none() && keyed.get("parent").is_none());
    assert_eq!(
        (*crate::test_support::layer_range(matte)),
        (*crate::test_support::layer_range(keyed))
    );
}

const TRACK_MATTE_FIXTURE: &str = "feature_track_matte_key_26_5_strict.prproj";
const TRACK_MATTE_SEQUENCE: &str = "3776e3eb-791f-4e6a-a2bb-7e77eff235ef";

/// One row of [`keyed_layers`].
type KeyedLayer = (i64, String, String, String, Option<Vec<f64>>, usize);

/// Each keyed root layer of `document` by start: its type, its track matte
/// mode, the type of the matte layer it names (a stage group's direct child),
/// the group's Scale and the keyed video's effect count.
fn keyed_layers(document: &Value) -> Vec<KeyedLayer> {
    let layers = document["composition"]["layers"].as_array().unwrap();
    let mut keyed: Vec<_> = layers
        .iter()
        .filter(|layer| layer.get("trackMatte").is_some())
        .map(|layer| {
            let matte_id = &layer["trackMatte"]["layer"];
            let (siblings, video) = if layer["type"] == "Group" {
                let children = layer["layers"].as_array().unwrap();
                (children, &children[0])
            } else {
                (layers, layer)
            };
            let matte = siblings
                .iter()
                .find(|candidate| &candidate["id"] == matte_id)
                .unwrap_or_else(|| panic!("matte {matte_id} of {}", layer["id"]));
            (
                (*crate::test_support::layer_range(layer))["start"]
                    .as_i64()
                    .unwrap(),
                layer["type"].as_str().unwrap().to_owned(),
                layer["trackMatte"]["mode"].as_str().unwrap().to_owned(),
                matte["type"].as_str().unwrap().to_owned(),
                (layer["type"] == "Group").then(|| {
                    layer["transform"]["scale"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|value| value.as_f64().unwrap())
                        .collect()
                }),
                video["effects"].as_array().map_or(0, Vec::len),
            )
        })
        .collect();
    keyed.sort_by_key(|layer| layer.0);
    keyed
}

/// The root image layers of `document` that no layer's track matte names.
fn unconsumed_images(document: &Value) -> Vec<Value> {
    let layers = document["composition"]["layers"].as_array().unwrap();
    let consumed: Vec<&Value> = layers
        .iter()
        .filter_map(|layer| layer.get("trackMatte").map(|matte| &matte["layer"]))
        .collect();
    layers
        .iter()
        .filter(|layer| layer["type"] == "Image" && !consumed.contains(&&layer["id"]))
        .map(|layer| layer["id"].clone())
        .collect()
}

#[test]
fn adobe_track_matte_key_fixture_imports_each_clip_and_writes_the_keys_back() {
    // `premiere_isolated_track_matte_key_26_5` (Oracle run 13): V1 base
    // clips, V2 the keyed fills and V3 their matte stills over the same
    // ranges. A (0-2 s) Matte Alpha; B (2-4 s) the same at Scale 50; C1-C4
    // (4-6 s) Matte Luma from red, green, blue and grey-128 stills; D (6-7 s)
    // Matte Alpha with Reverse; F (7-8.5 s) Matte Alpha after a Gaussian Blur
    // (Legacy) 25; E (8.5-10 s) Matte Luma with Reverse. E fails closed (G3b)
    // and its matte item 146 is a disclosed fixture limit (its source span
    // does not match its duration); the fill's own key consumed nothing there.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(TRACK_MATTE_FIXTURE);
    let output = root.join("converted");
    let omissions =
        premiere_to_tesseract(&source, &output, Some(TRACK_MATTE_SEQUENCE), false).unwrap();
    let luma_approximation = |record: &str| {
        (
            OmissionScope::Feature,
            record.to_owned(),
            "Matte Luma is approximated: Premiere weights the matte's encoded RGB by Rec. 601 (measured), FX by Rec. 709; exact for a neutral matte, up to about 11 levels apart on a saturated colour matte".to_owned(),
        )
    };
    let reported: Vec<_> = omissions
        .iter()
        .filter(|omission| {
            !omission.reason.contains("ClipTrackItem/TrackItem/Node")
                && !omission.reason.contains("DefMappingID")
        })
        .map(|omission| {
            (
                omission.scope,
                omission.record.clone(),
                omission.reason.clone(),
            )
        })
        .collect();
    assert_eq!(
        reported,
        [
            (
                OmissionScope::Occurrence,
                "137".to_owned(),
                "unsupported conversion: VideoFilterComponent:238: Reverse with Matte Luma is not converted; Premiere gives the matte clip's zero-luma exterior full coverage where FX's inverted luma matte gives none".to_owned(),
            ),
            (
                OmissionScope::Occurrence,
                "146".to_owned(),
                "invalid Premiere project: source span does not match the constant playback rate".to_owned(),
            ),
            luma_approximation("VideoClipTrackItem:131"),
            luma_approximation("VideoClipTrackItem:132"),
            luma_approximation("VideoClipTrackItem:133"),
            luma_approximation("VideoClipTrackItem:134"),
        ]
    );
    let archive = first_project(&output);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    let expected = [
        (0, "Video", "alpha", "Image", None, 0),
        (2000, "Group", "alpha", "Image", Some(vec![50.0, 50.0]), 0),
        (4000, "Video", "luma", "Image", None, 0),
        (4500, "Video", "luma", "Image", None, 0),
        (5000, "Video", "luma", "Image", None, 0),
        (5500, "Video", "luma", "Image", None, 0),
        (6000, "Video", "alphaInverted", "Image", None, 0),
        (7000, "Group", "alpha", "Image", Some(vec![100.0, 100.0]), 1),
    ]
    .map(|(start, kind, mode, matte, scale, effects)| {
        (
            start,
            kind.to_owned(),
            mode.to_owned(),
            matte.to_owned(),
            scale,
            effects,
        )
    });
    assert_eq!(keyed_layers(&document), expected);
    assert_eq!(unconsumed_images(&document), Vec::<Value>::new());

    // Every key writes back with its Matte (the V3 track's written ID 3),
    // Composite Using and Reverse, and the export imports to the same layers.
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = native.join("project.prproj");
    let (keys, track_ids) = native_keys(&exported);
    assert_eq!(track_ids, ["1", "2", "3"]);
    let value = |matte: &str, composite: &str, reverse: &str| {
        [matte.to_owned(), composite.to_owned(), reverse.to_owned()]
    };
    assert_eq!(
        key_values(&keys),
        [
            value("3", "0", "false"),
            value("3", "0", "false"),
            value("3", "1", "false"),
            value("3", "1", "false"),
            value("3", "1", "false"),
            value("3", "1", "false"),
            value("3", "0", "true"),
            value("3", "0", "false"),
        ]
    );
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert_eq!(
        omissions
            .iter()
            .filter(|omission| omission.scope == OmissionScope::Occurrence)
            .count(),
        0,
        "{omissions:?}"
    );
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(keyed_layers(&reimported), expected);
    assert_eq!(unconsumed_images(&reimported), Vec::<Value>::new());
}
