//! Nested sequences in the `feature_nested_sequence_strict` fixture. Its nest
//! records are XML edits of `feature_nested_second_sequence_strict` (whose own
//! nest was an edit too), so it is XML-edited: Premiere has never saved this
//! nest shape. Its AME render is the `premiere_isolated_nested_sequence`
//! reference (`proof: video_reference`).

#[cfg(feature = "ffmpeg-library")]
use super::support::*;
use premiere_file::PrProjectFile;
#[cfg(feature = "ffmpeg-library")]
use serde_json::{json, Value};
#[cfg(feature = "ffmpeg-library")]
use std::path::PathBuf;
use std::{fs, path::Path};
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::{AssetKind, TesseractFileBuilder};

#[cfg(feature = "ffmpeg-library")]
const INNER: &str = "07beb511-806b-48cb-b0f5-92293e47d8f4";
#[cfg(feature = "ffmpeg-library")]
const OUTER: &str = "dab91e14-ca76-47e7-93fc-99bf6bcc94be";
#[cfg(feature = "ffmpeg-library")]
const HIDDEN_NEST_SEQUENCE: &str = "a8b57c46-9429-47b1-915f-e2b6e25ed337";

/// Stages the fixture and its two sources at their package paths.
#[cfg(feature = "ffmpeg-library")]
fn nested_fixture(root: &Path) -> PathBuf {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(root.join("media")).unwrap();
    for (name, source) in [
        ("nested-timecoded.mp4", "feature_timecoded_source.mp4"),
        ("outer-red.mp4", "feature_multi_sequence_red_10s.mp4"),
    ] {
        fs::copy(fixtures.join(source), root.join("media").join(name)).unwrap();
    }
    let project = root.join("project.prproj");
    fs::copy(
        fixtures.join("feature_nested_sequence_strict.prproj"),
        &project,
    )
    .unwrap();
    project
}

#[cfg(feature = "ffmpeg-library")]
type LayerRow = (usize, String, Option<String>, Value, Value, Option<String>);

/// Depth, type, group name, active and source ranges, and packaged file of every layer.
#[cfg(feature = "ffmpeg-library")]
fn layer_tree(file: &TesseractFile) -> Vec<LayerRow> {
    fn walk(file: &TesseractFile, layers: &Value, depth: usize, rows: &mut Vec<LayerRow>) {
        for layer in layers.as_array().unwrap() {
            let asset = layer["source"]["assetId"].as_str().map(|id| {
                Path::new(&file.metadata().assets[id].path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            });
            let group = layer["type"] == "Group";
            rows.push((
                depth,
                layer["type"].as_str().unwrap().to_owned(),
                group.then(|| layer["name"].as_str().unwrap().to_owned()),
                (*crate::test_support::layer_range(layer)).clone(),
                layer["sourceRange"].clone(),
                asset,
            ));
            if group {
                walk(file, &layer["layers"], depth + 1, rows);
            }
        }
    }
    let mut rows = Vec::new();
    walk(
        file,
        &file.project_json().unwrap()["composition"]["layers"],
        0,
        &mut rows,
    );
    rows
}

#[cfg(feature = "ffmpeg-library")]
fn only_project(directory: &Path) -> TesseractFile {
    let projects = project_files(directory);
    assert_eq!(projects.len(), 1);
    TesseractFile::open(&projects[0]).unwrap()
}

#[cfg(feature = "ffmpeg-library")]
fn range(start: i64, duration: i64) -> Value {
    json!({"start": start, "duration": duration})
}

#[cfg(feature = "ffmpeg-library")]
fn row(
    depth: usize,
    kind: &str,
    group: Option<&str>,
    active: Value,
    source: Value,
    asset: Option<&str>,
) -> LayerRow {
    (
        depth,
        kind.into(),
        group.map(str::to_owned),
        active,
        source,
        asset.map(str::to_owned),
    )
}

#[cfg(feature = "ffmpeg-library")]
fn outer_rows() -> Vec<LayerRow> {
    let timecoded = Some("nested-timecoded.mp4");
    vec![
        row(0, "Group", Some("Inner"), range(0, 3000), Value::Null, None),
        row(
            1,
            "Video",
            None,
            range(0, 3000),
            range(1000, 3000),
            timecoded,
        ),
        row(
            0,
            "Group",
            Some("Inner"),
            range(5000, 6000),
            Value::Null,
            None,
        ),
        row(1, "Video", None, range(0, 4000), range(0, 4000), timecoded),
        row(
            1,
            "Video",
            None,
            range(5000, 1000),
            range(7000, 1000),
            timecoded,
        ),
        row(
            0,
            "Video",
            None,
            range(0, 10000),
            range(0, 10000),
            Some("outer-red.mp4"),
        ),
        row(0, "Rect", None, range(0, 11000), Value::Null, None),
    ]
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn xml_edited_nests_import_as_independent_groups_over_the_outer_clip() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions =
        premiere_to_tesseract(nested_fixture(dir.path()), &output, Some(OUTER), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Inner is placed by Outer, so only Outer is a top-level timeline.
    let file = only_project(&output);
    assert_eq!(file.project().composition().name(), "Outer");
    // Three inline copies share the timecoded source asset.
    assert_eq!(file.metadata().assets.len(), 2);
    // The 0-3 s nest shows Inner 1-4 s; the 5-11 s nest shows all of Inner,
    // whose 4-5 s gap (9-10 s outer) leaves the red outer clip visible.
    assert_eq!(layer_tree(&file), outer_rows());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn reverse_nested_public_import_publishes_child_assets_and_decreasing_playback() {
    // Supplemental mutation of the existing native-derived nest scaffold;
    // it does not claim independently Adobe-authored reverse-nest proof.
    let dir = tempfile::tempdir().unwrap();
    let source = nested_fixture(dir.path());
    let mut xml = read_xml(&source);
    for id in [86, 150] {
        edit_record(
            &mut xml,
            &format!("<VideoClip ObjectID=\"{id}\""),
            "</VideoClip>",
            |record| record.replace("</Clip>", "<PlayBackwards>true</PlayBackwards></Clip>"),
        );
    }
    write_prproj(&source, &xml);
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, Some(OUTER), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let file = only_project(&output);
    assert_eq!(file.metadata().assets.len(), 2);
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    for (owner, first, last) in [(&layers[0], 5000, 2000), (&layers[1], 6000, 0)] {
        let picture = &owner["layers"][0];
        let keys = picture["playback"]["mapping"]["property"]["keyframes"]
            .as_array()
            .unwrap();
        assert_eq!(keys[0]["value"], first);
        assert_eq!(keys[1]["value"], last);
        let videos = picture["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .collect::<Vec<_>>();
        assert_eq!(videos.len(), 2);
        assert_eq!(*crate::test_support::layer_range(videos[0]), range(0, 4000));
        assert_eq!(
            *crate::test_support::layer_range(videos[1]),
            range(5000, 1000)
        );
        assert!(videos.iter().all(|video| file
            .metadata()
            .assets
            .contains_key(video["source"]["assetId"].as_str().unwrap())));
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn nests_round_trip_through_premiere_as_one_sequence_per_group() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(nested_fixture(root), root.join("first"), Some(OUTER), false).unwrap();
    let first = project_files(&root.join("first")).remove(0);
    let omissions = tesseract_to_premiere(&first, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let native = root.join("native/project.prproj");
    assert_eq!(read_xml(&native).matches("<Sequence ObjectUID=").count(), 3);
    let (project, omissions) = PrProjectFile::load(&native).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project
            .sequences()
            .map(|sequence| sequence.name())
            .collect::<Vec<_>>(),
        ["Outer"]
    );
    let root_sequence = exported_root_sequence(&native);
    premiere_to_tesseract(&native, root.join("again"), Some(&root_sequence), false).unwrap();
    assert_eq!(layer_tree(&only_project(&root.join("again"))), outer_rows());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn missing_nested_media_preserves_healthy_outer_media() {
    let dir = tempfile::tempdir().unwrap();
    let project = nested_fixture(dir.path());
    fs::remove_file(dir.path().join("media/nested-timecoded.mp4")).unwrap();
    let output = dir.path().join("converted");
    for check in [true, false] {
        let omissions = premiere_to_tesseract(&project, &output, Some(OUTER), check).unwrap();
        assert!(
            omissions
                .iter()
                .any(|note| note.record == "VideoClipTrackItem:145"
                    && note.reason.contains("missing media")),
            "{omissions:?}"
        );
        if check {
            assert!(!output.exists());
        } else {
            let file = only_project(&output);
            assert_eq!(file.metadata().assets.len(), 1);
            assert_eq!(
                layer_tree(&file),
                [
                    row(
                        0,
                        "Video",
                        None,
                        range(0, 10000),
                        range(0, 10000),
                        Some("outer-red.mp4")
                    ),
                    row(0, "Rect", None, range(0, 11000), Value::Null, None),
                ]
            );
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn explicit_inner_selection_converts_the_inner_timeline_alone() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("inner");
    premiere_to_tesseract(nested_fixture(dir.path()), &output, Some(INNER), false).unwrap();
    let file = only_project(&output);
    assert_eq!(file.project().composition().name(), "Inner");
    let timecoded = Some("nested-timecoded.mp4");
    // As a top-level timeline, Inner's gap is covered by its own black canvas.
    assert_eq!(
        layer_tree(&file),
        [
            row(0, "Video", None, range(0, 4000), range(0, 4000), timecoded),
            row(
                0,
                "Video",
                None,
                range(5000, 1000),
                range(7000, 1000),
                timecoded
            ),
            row(0, "Rect", None, range(0, 6000), Value::Null, None),
        ]
    );
}

#[test]
fn a_nest_whose_master_clip_plays_media_is_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(root.join("media")).unwrap();
    fs::copy(
        fixtures.join("feature_nested_second_sequence_strict.prproj"),
        root.join("project.prproj"),
    )
    .unwrap();
    for (name, source) in [
        ("clip-a.mp4", "feature_multi_sequence_red_10s.mp4"),
        ("clip-b.mp4", "feature_multi_sequence_blue_10s.mp4"),
    ] {
        fs::copy(fixtures.join(source), root.join("media").join(name)).unwrap();
    }
    // This older fixture only swapped the placed clip's source to a sequence;
    // its SubClip still names the media master clip.
    let (project, omissions) = PrProjectFile::load(root.join("project.prproj")).unwrap();
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "85" && item.reason.contains("source identity mismatch")),
        "{omissions:?}"
    );
    let outer = project.sequences().next().unwrap();
    assert_eq!(outer.name(), "Two tracks overlap");
    assert_eq!(outer.video_occurrences().count(), 1);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn recovered_group_properties_do_not_skip_media_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    premiere_to_tesseract(nested_fixture(root), root.join("first"), Some(OUTER), false).unwrap();
    let file = only_project(&root.join("first"));
    let imported = file.project_json().unwrap();
    let group_id = &imported["composition"]["layers"][0]["id"];
    // Keys on the first group's `property` at 0 and 1 s.
    let track = |property: &str, [first, second]: [f64; 2], easing: Value| {
        let key = |id: &str, time: i64, value: f64, easing: Value| json!({"id": id, "layerTime": time, "value": {"type": "float", "value": value}, "easing": easing});
        json!({
            "target": {"kind": "layer", "layerId": group_id, "propertyType": property},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                key("first", 0, first, json!({"type": "linear"})),
                key("second", 1000, second, easing)
            ]}
        })
    };
    let linear = || json!({"type": "linear"});
    let broken = root.join("broken.mp4");
    fs::write(&broken, b"not a video").unwrap();
    // Property recovery keeps this group's child a used asset. Malformed media
    // must therefore reject atomically rather than bypass physical inspection.
    for (index, (tracks, _reason)) in [
        (
            vec![track("anchorPointX", [0.0, 960.0], linear())],
            "unpaired Anchor Point keyframes were not exported",
        ),
        (
            vec![track("scaleY", [100.0, 0.0], linear())],
            "nonuniform or unpaired Scale keyframes were not exported",
        ),
        (
            vec![track(
                "opacity",
                [100.0, 100.0],
                json!({"type": "cubicBezier", "x1": 0.4, "y1": 0.0, "x2": 0.6, "y2": 1.0}),
            )],
            "Opacity animation was not exported: unsupported conversion: Opacity cubic easing between equal values cannot preserve Premiere velocity",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut document = imported.clone();
        let group = &mut document["composition"]["layers"][0];
        assert_eq!(group["type"], "Group");
        let original_asset=group["layers"][0]["source"]["assetId"].as_str().unwrap().to_owned();
        group["layers"][0]["source"]["assetId"] = json!("broken-video");
        document["composition"]["dynamics"] = json!({ "entries": tracks });
        let mut builder =
            TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
                .unwrap();
        for (id, asset) in &file.metadata().assets {
            let name = Path::new(&asset.path).file_name().unwrap();
            builder = builder
                .add_asset(id, root.join("media").join(name), AssetKind::Video)
                .unwrap();
        }
        let edited = root.join(format!("edited-{index}.tsrct"));
        builder
            .add_asset("broken-video", &broken, AssetKind::Video)
            .unwrap()
            .write(&edited)
            .unwrap();
        let native = root.join(format!("native-{index}"));
        for check in [true,false] {
            let error=tesseract_to_premiere(&edited,&native,check).unwrap_err();
            assert!(error.to_string().contains("broken-video") && error.to_string().contains("MP4 metadata box exceeds its parent"),"{error}");
            assert!(!native.exists());
        }
        // The same recovery against real, already-pinned media retains children
        // and independent siblings instead of using omission to skip inspection.
        let mut builder=TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
        for (id,asset) in &file.metadata().assets {
            let name=Path::new(&asset.path).file_name().unwrap();
            builder=builder.add_asset(id,root.join("media").join(name),AssetKind::Video).unwrap();
        }
        let original=&file.metadata().assets[&original_asset];
        let valid_media=root.join("media").join(Path::new(&original.path).file_name().unwrap());
        let valid=root.join(format!("valid-{index}.tsrct"));
        builder.add_asset("broken-video",valid_media,AssetKind::Video).unwrap().write(&valid).unwrap();
        let losses=tesseract_to_premiere(valid,&native,false).unwrap();
        assert!(losses.iter().any(|loss|loss.scope==premiere_file::OmissionScope::Feature),"{losses:?}");
        let reimported=root.join(format!("recovered-{index}"));
        let (project,_) = PrProjectFile::load(native.join("project.prproj")).unwrap();
        let outer=project.sequences().find(|sequence|Some(sequence.name())==document["composition"]["name"].as_str()).unwrap();
        premiere_to_tesseract(native.join("project.prproj"),&reimported,outer.id(),false).unwrap();
        let recovered=only_project(&reimported).project_json().unwrap();
        let groups=|wire:&Value|->Vec<Value> {wire["composition"]["layers"].as_array().unwrap().iter().filter(|layer|layer["type"]=="Group").map(|group|json!({
            "range":crate::test_support::layer_range(group),
            "source":group["layers"][0]["sourceRange"],
            "childRange":crate::test_support::layer_range(&group["layers"][0]),
            "children":group["layers"].as_array().unwrap().iter().filter(|child|child["type"]=="Video").count()
        })).collect()};
        assert_eq!(groups(&recovered),groups(&document));
        assert_eq!(recovered["composition"]["layers"].as_array().unwrap().iter().filter(|layer|layer["type"]=="Video").count(),1);

    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn adobe_hidden_nests_import_as_hidden_groups_and_export_as_disabled_nests() {
    /// Start, hidden state and child count of each top-level group, by start.
    fn groups(document: &Value) -> Vec<(i64, bool, usize)> {
        let mut groups: Vec<_> = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Group")
            .map(|group| {
                (
                    (*crate::test_support::layer_range(group))["start"]
                        .as_i64()
                        .unwrap(),
                    group["isHidden"] == true,
                    group["layers"].as_array().unwrap().len(),
                )
            })
            .collect();
        groups.sort();
        groups
    }
    // `premiere_isolated_hidden_nest_26_5`, Premiere
    // 26.5.1's save: AME renders the nest at 0-2 s, and neither the disabled
    // nest at 2-4 s nor the nest at 4-6 s on V3, whose track output is off.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_hidden_nest_26_5_strict.prproj");
    let converted = root.join("converted");
    let omissions =
        premiere_to_tesseract(&source, &converted, Some(HIDDEN_NEST_SEQUENCE), false).unwrap();
    // The inner audio of each nest stays omitted, as before.
    assert!(
        omissions.iter().all(
            |omission| omission.reason.contains("found AudioSequenceSource")
                || omission.reason.contains("ClipTrackItem/TrackItem/Node")
        ),
        "{omissions:?}"
    );
    let document = only_project(&converted).project_json().unwrap();
    let hidden = [(0, false, 1), (2000, true, 1), (4000, true, 1)];
    assert_eq!(groups(&document), hidden);
    // Each hidden group exports as a disabled nest and reimports hidden.
    let first = project_files(&converted).remove(0);
    let omissions = tesseract_to_premiere(&first, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let native = root.join("native/project.prproj");
    let xml = read_xml(&native);
    assert_eq!(xml.matches("<IsMuted>true</IsMuted>").count(), 2);
    let root_sequence = exported_root_sequence(&native);
    premiere_to_tesseract(&native, root.join("again"), Some(&root_sequence), false).unwrap();
    let again = only_project(&root.join("again")).project_json().unwrap();
    assert_eq!(groups(&again), hidden);
}

/// The G2 gate document: over a black canvas, a hidden plain group at
/// 0-1.5 s and an edited group at 1.5-6 s (Scale 70, Rotation 10, moved by
/// (100, 50) px, Opacity keys 100 to 40, a Gaussian Blur 20 and a Crop of 10%
/// per edge) that plays the timecoded source from 1 s at group 0-2 s and from
/// 5 s at 2.5-4.5 s.
#[cfg(feature = "ffmpeg-library")]
fn edited_group_document() -> Value {
    let transform = |anchor: [f64; 2], position: [f64; 2], scale: f64, rotation: f64| {
        json!({
            "anchorPoint": anchor, "position": position, "scale": [scale, scale],
            "rotation": rotation, "opacity": 100
        })
    };
    let identity = transform([0.0, 0.0], [0.0, 0.0], 100.0, 0.0);
    let video = |id: u64, parent: u64, start: i64, duration: i64, source_start: i64| {
        json!({
            "type": "Video", "id": id, "parent": parent, "name": format!("Timecoded {id}"),
            "playback": crate::test_support::linear_playback(json!({"start": start, "duration": duration}), json!({"start": source_start, "duration": duration})),
            "sourceRange": {"start": source_start, "duration": duration},
            "sourceIntrinsicDuration": 10000, "volume": 0,
            "transform": transform([960.0, 540.0], [960.0, 540.0], 100.0, 0.0),
            "source": {
                "assetId": "premiere-video-1", "fit": "contain",
                "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}
            }
        })
    };
    let key = |id: &str, time: i64, value: f64| {
        json!({
            "id": id, "layerTime": time, "value": {"type": "float", "value": value},
            "easing": {"type": "linear"}
        })
    };
    json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1,
        "dimensions": {"width": 1920, "height": 1080},
        "duration": 6.0,
        "composition": {
            "id": "main",
            "name": "Edited group",
            "layers": [
                {
                    "type": "Group", "id": 10, "name": "Edited", "blendMode": "normal",
                    "playback": crate::test_support::linear_playback(json!({"start": 1500, "duration": 4500}), json!({"start": 0, "duration": 4500})),
                    "transform": transform([960.0, 540.0], [1060.0, 590.0], 70.0, 10.0),
                    "effects": [{"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 20.0}}],
                    "masks": [{"id": 2, "mode": "add", "layer": 13}],
                    "layers": [
                        video(11, 10, 0, 2000, 1000),
                        video(12, 10, 2500, 2000, 5000),
                        {
                            "type": "Rect", "id": 13, "parent": 10, "name": "Crop guide",
                            "activeRange": {"start": 0, "duration": 4500}, "transform": identity,
                            "rect": {"size": [1536, 864], "position": [192, 108], "fillColor": [0, 0, 0, 1]}
                        }
                    ]
                },
                {
                    "type": "Group", "id": 20, "name": "Hidden", "blendMode": "normal",
                    "isHidden": true, "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1500}), json!({"start": 0, "duration": 1500})),
                    "transform": identity, "layers": [video(21, 20, 0, 1500, 0)]
                },
                {
                    "type": "Rect", "id": 30, "name": "Black canvas",
                    "activeRange": {"start": 0, "duration": 6000}, "transform": identity,
                    "rect": {"size": [1920, 1080], "fillColor": [0, 0, 0, 1]}
                }
            ],
            "dynamics": {"entries": [{
                "target": {"kind": "layer", "layerId": 10, "propertyType": "opacity"},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [key("o0", 0, 100.0), key("o1", 4000, 40.0)]}
            }]}
        }
    })
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn an_edited_group_writes_one_nest_placement_that_carries_its_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let media =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_timecoded_source.mp4");
    let archive = root.join("edited-group.tsrct");
    write_archive(&archive, &edited_group_document(), &media);
    let omissions = tesseract_to_premiere(&archive, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let xml = read_xml(&root.join("native/project.prproj"));
    assert_edited_group_native_controls(&xml);
}

/// Current native placement, child order, source trims, bypass and editable keys.
#[cfg(feature = "ffmpeg-library")]
fn assert_edited_group_native_controls(xml: &str) {
    let document = roxmltree::Document::parse(xml).unwrap();
    fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, tag: &str) -> roxmltree::Node<'a, 'i> {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .unwrap()
    }
    fn text(node: roxmltree::Node<'_, '_>, tag: &str) -> Option<String> {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    }
    let ids: Vec<_> = document
        .descendants()
        .filter_map(|node| node.attribute("ObjectID"))
        .collect();
    let records: std::collections::BTreeMap<_, _> = document
        .root_element()
        .children()
        .filter_map(|node| Some((node.attribute("ObjectID")?, node)))
        .collect();
    assert_eq!(records.len(), ids.len(), "ObjectIDs are unique");
    let follow = |node: roxmltree::Node<'_, '_>, tag: &str| {
        records[child(node, tag).attribute("ObjectRef").unwrap()]
    };
    let by_uid = |uid: &str| {
        document
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectUID") == Some(uid))
            .unwrap()
    };
    // The video track items of a sequence, each with its ClipTrackItem,
    // range, In/Out, IsMuted and the sequence that it plays, if any.
    let items = |sequence: roxmltree::Node<'_, '_>| {
        let group = follow(
            child(child(sequence, "TrackGroups"), "TrackGroup"),
            "Second",
        );
        child(child(group, "TrackGroup"), "Tracks")
            .children()
            .filter(|node| node.has_tag_name("Track"))
            .flat_map(|track| {
                let track = by_uid(track.attribute("ObjectURef").unwrap());
                child(child(child(track, "ClipTrack"), "ClipItems"), "TrackItems")
                    .children()
                    .filter(|node| node.has_tag_name("TrackItem"))
                    .map(|item| {
                        child(
                            records[item.attribute("ObjectRef").unwrap()],
                            "ClipTrackItem",
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .map(|clip_item| {
                let range = child(clip_item, "TrackItem");
                let clip = child(follow(follow(clip_item, "SubClip"), "Clip"), "Clip");
                let source = follow(clip, "Source");
                let nested = source.has_tag_name("VideoSequenceSource").then(|| {
                    by_uid(
                        child(child(source, "SequenceSource"), "Sequence")
                            .attribute("ObjectURef")
                            .unwrap(),
                    )
                });
                (
                    clip_item,
                    (text(range, "Start").map_or(0, |start| start.parse().unwrap())
                        ..text(range, "End").unwrap().parse::<i64>().unwrap()),
                    (
                        text(clip, "InPoint").unwrap(),
                        text(clip, "OutPoint").unwrap(),
                    ),
                    text(clip_item, "IsMuted"),
                    nested,
                )
            })
            .collect::<Vec<_>>()
    };
    let ticks = |seconds: f64| (seconds * TICKS as f64) as i64;
    let source = |seconds: f64| ticks(seconds).to_string();
    let outer = document
        .root_element()
        .children()
        .find(|node| {
            node.has_tag_name("Sequence") && text(*node, "Name").as_deref() == Some("Edited group")
        })
        .unwrap();
    // The guide is no clip: the outer sequence places the two groups.
    let [hidden, edited] = items(outer).try_into().unwrap();
    let name = |item: &(_, _, _, _, Option<roxmltree::Node<'_, '_>>)| {
        text(item.4.unwrap(), "Name").unwrap()
    };
    assert_eq!(
        (name(&hidden), name(&edited)),
        ("Hidden".to_owned(), "Edited".to_owned())
    );
    assert_eq!(
        (&hidden.1, &hidden.2, hidden.3.as_deref()),
        (&(0..ticks(1.5)), &(source(0.0), source(1.5)), Some("true"))
    );
    assert_eq!(
        (&edited.1, &edited.2, edited.3.as_deref()),
        (&(ticks(1.5)..ticks(6.0)), &(source(0.0), source(4.5)), None)
    );
    // The edited placement's chain in Index order, as (match name,
    // parameter values). Premiere applies it from the highest Index down: the
    // Crop, the blur, then Motion and Opacity.
    let mut components: Vec<_> = follow(child(edited.0, "ComponentOwner"), "Components")
        .descendants()
        .filter(|node| node.has_tag_name("Component") && node.has_attribute("Index"))
        .map(|component| {
            let filter = records[component.attribute("ObjectRef").unwrap()];
            let params: Vec<_> = child(child(filter, "Component"), "Params")
                .children()
                .filter(|node| node.has_tag_name("Param"))
                .map(|param| {
                    let param = records[param.attribute("ObjectRef").unwrap()];
                    (
                        text(param, "Name").unwrap_or_default(),
                        text(param, "StartKeyframe")
                            .unwrap()
                            .split(',')
                            .nth(1)
                            .unwrap()
                            .to_owned(),
                        text(param, "Keyframes"),
                    )
                })
                .collect();
            (
                component
                    .attribute("Index")
                    .unwrap()
                    .parse::<usize>()
                    .unwrap(),
                text(filter, "MatchName").unwrap(),
                params,
            )
        })
        .collect();
    components.sort();
    let chain: Vec<_> = components
        .iter()
        .map(|(index, name, _)| (*index, name.as_str()))
        .collect();
    assert_eq!(
        chain,
        [
            (0, "AE.ADBE Opacity"),
            (1, "AE.ADBE Motion"),
            (2, "AE.Impact_Blur_FX"),
            (3, "AE.ADBE AECrop"),
        ]
    );
    let param = |component: usize, name: &str| {
        components[component]
            .2
            .iter()
            .find(|(param, _, _)| param == name)
            .map(|(_, value, keys)| (value.as_str(), keys.as_deref()))
            .unwrap()
    };
    assert_eq!(
        param(0, "Opacity"),
        (
            "100",
            Some(format!("0,100,0,0,0,0,0,0;{},40,0,0,0,0,0,0;", ticks(4.0)).as_str())
        )
    );
    assert_eq!(
        param(1, "Position").0,
        format!("{}:{}", 1060.0 / 1920.0, 590.0 / 1080.0)
    );
    for (name, value) in [
        ("Scale", "70"),
        ("Scale Width", "70"),
        (" ", "true"),
        ("Rotation", "10"),
        ("Anchor Point", "0.5:0.5"),
    ] {
        assert_eq!(param(1, name), (value, None), "{name}");
    }
    // Blurriness 20 is Amount 20 / 5.7.
    assert_eq!(param(2, "Amount"), ("3.508771929824561", None));
    for edge in ["Left", "Top", "Right", "Bottom"] {
        assert_eq!(param(3, edge), ("10", None), "{edge}");
    }
    // Each placement's own sequence holds its group's children, with their
    // timing and source trims.
    let timing = |placement: &(_, _, _, _, Option<roxmltree::Node<'_, '_>>)| {
        items(placement.4.unwrap())
            .into_iter()
            .map(|item| (item.1, item.2))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        timing(&hidden),
        [(0..ticks(1.5), (source(0.0), source(1.5)))]
    );
    assert_eq!(
        timing(&edited),
        [
            (0..ticks(2.0), (source(1.0), source(3.0))),
            (ticks(2.5)..ticks(4.5), (source(5.0), source(7.0))),
        ]
    );
}

/// Explicit edits of the existing native-backed G2 export input, not an Adobe
/// motion-blur oracle. The lost optional flag must not erase its current edits.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn group_motion_blur_preserves_native_content_controls_order_and_disabled_state() {
    use premiere_file::{
        ExportField, ExportLossDomain, ExportLossKind, ExportLossSource, Premiere,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let media =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_timecoded_source.mp4");
    let mut document = edited_group_document();
    for index in [0, 1] {
        document["composition"]["layers"][index]["motionBlur"] = json!(true);
    }
    let archive = root.join("group-motion-blur.tsrct");
    write_archive(&archive, &document, &media);
    let file = TesseractFile::open(&archive).unwrap();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let report = operation.losses();
    assert!(report.has_native_content);
    assert_eq!(report.losses.len(), 2, "{report:?}");
    for id in [10, 20] {
        let loss = report
            .losses
            .iter()
            .find(|loss| loss.source == ExportLossSource::Layer(fx_schema::LayerId::new(id)))
            .unwrap();
        assert_eq!(loss.domain, ExportLossDomain::Picture);
        assert_eq!(loss.kind, ExportLossKind::Field(ExportField::MotionBlur));
        assert_eq!(loss.omission.scope, premiere_file::OmissionScope::Feature);
        assert_eq!(loss.omission.kind, premiere_file::OmissionKind::Omitted);
        assert!(loss
            .omission
            .reason
            .contains("group motion blur was not exported"));
    }
    let staged = operation
        .stage_with_picture_replacements(root, &root.join("native"), &[])
        .unwrap();
    assert_eq!(staged.report().diagnostics.len(), 2);
    let xml = read_xml(&staged.directory().join("project.prproj"));
    assert_eq!(xml.matches("<Sequence ObjectUID=").count(), 3);
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Geometry2</MatchName>")
            .count(),
        0
    );
    assert_edited_group_native_controls(&xml);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn group_motion_blur_reports_repaired_labels_but_not_empty_owners() {
    use premiere_file::{ExportField, ExportLossKind, ExportLossSource, Premiere};

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let media =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_timecoded_source.mp4");
    for empty in [false, true] {
        let mut document = edited_group_document();
        for index in [0, 1] {
            document["composition"]["layers"][index]["motionBlur"] = json!(true);
        }
        if empty {
            document["composition"]["layers"][0]["layers"] = json!([]);
            document["composition"]["layers"][0]["masks"] = json!([]);
        } else {
            // An invalid label is generated without losing the current child
            // picture, while a genuinely empty owner remains unretained.
            document["composition"]["layers"][0]["name"] = json!("");
        }
        let archive = root.join(format!("invalid-group-{empty}.tsrct"));
        write_archive(&archive, &document, &media);
        let file = TesseractFile::open(&archive).unwrap();
        let operation = Premiere
            .prepare_export(&file, file.project(), &Default::default())
            .unwrap();
        let report = operation.losses();
        let blur_losses: Vec<_> = report
            .losses
            .iter()
            .filter(|loss| loss.kind == ExportLossKind::Field(ExportField::MotionBlur))
            .collect();
        let mut owners = blur_losses
            .iter()
            .map(|loss| match loss.source {
                ExportLossSource::Layer(id) => id.value(),
                _ => panic!("Group field loss must identify its layer"),
            })
            .collect::<Vec<_>>();
        owners.sort_unstable();
        assert_eq!(
            owners,
            if empty { vec![20] } else { vec![10, 20] },
            "{report:?}"
        );
        if empty {
            assert!(
                report.diagnostics.iter().any(|note| note.scope
                    == premiere_file::OmissionScope::Occurrence
                    && note.record.starts_with("layer 10 (")
                    && note
                        .reason
                        .contains("group with no exportable video was not exported")),
                "{report:?}"
            );
        } else {
            assert!(
                report.losses.iter().any(|loss| loss.source
                    == ExportLossSource::Layer(fx_schema::LayerId::new(10))
                    && loss.kind == ExportLossKind::Field(ExportField::Metadata)),
                "{report:?}"
            );
        }
        let staged = operation
            .stage_with_picture_replacements(root, &root.join(format!("native-{empty}")), &[])
            .unwrap();
        let xml = read_xml(&staged.directory().join("project.prproj"));
        assert_eq!(
            xml.matches("<Sequence ObjectUID=").count(),
            if empty { 2 } else { 3 }
        );
        if !empty {
            let parsed = roxmltree::Document::parse(&xml).unwrap();
            let generated = parsed
                .descendants()
                .filter(|node| node.has_tag_name("Sequence"))
                .filter_map(|node| {
                    node.children()
                        .find(|child| child.has_tag_name("Name"))
                        .and_then(|name| name.text())
                })
                .find(|name| *name != "Hidden" && *name != "Edited group")
                .unwrap();
            assert!(!generated.is_empty() && generated.chars().count() <= 255);
            // Compare all editable native controls using the existing helper;
            // only its expected display label is normalized for this assertion.
            assert_edited_group_native_controls(
                &xml.replace(&format!("<Name>{generated}</Name>"), "<Name>Edited</Name>"),
            );
        }
        assert!(xml.contains("<Name>Hidden</Name>"));
        assert!(xml.contains("<IsMuted>true</IsMuted>"));
    }
}
