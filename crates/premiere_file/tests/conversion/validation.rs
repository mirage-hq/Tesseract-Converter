use super::support::*;
use serde_json::json;
use std::fs;
#[cfg(feature = "ffmpeg-library")]
use std::path::Path;
use tesseract_file::TesseractFileBuilder;
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::{AssetKind, TesseractFile};

#[test]
fn malformed_input_and_nonframe_ranges_fail_without_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(
        root,
        &one_second()
            .replace("<TrackItem><End>", "<TrackItem><Start>1</Start><End>")
            .replace("<End>254016000000</End>", "<End>254016000001</End>")
            .replace(
                "<Duration>254016000000</Duration>",
                "<Duration>508032000000</Duration>",
            )
            .replace(
                "<OriginalDuration>254016000000</OriginalDuration>",
                "<OriginalDuration>508032000000</OriginalDuration>",
            ),
    );
    assert!(
        premiere_to_tesseract(root.join("project.prproj"), root.join("out"), None, false)
            .unwrap_err()
            .to_string()
            .contains("frame boundary")
    );
    assert!(!root.join("out").exists());
    fs::write(root.join("bad.prproj"), b"not a project").unwrap();
    premiere_to_tesseract(root.join("bad.prproj"), root.join("bad-out"), None, true)
        .unwrap_err()
        .to_string();
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn premiere_to_tesseract_rejects_duration_drift_and_invalid_sequence_or_source_clocks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // One frame of duration drift used to pass the premiere_to_tesseract's broad metadata tolerance.
    fixture(
        root,
        &one_second()
            .replace(
                "<Duration>254016000000</Duration>",
                "<Duration>262483200000</Duration>",
            )
            .replace(
                "<OriginalDuration>254016000000</OriginalDuration>",
                "<OriginalDuration>262483200000</OriginalDuration>",
            ),
    );
    // Used-video metadata must fail admission before any output is published.
    let error = premiere_to_tesseract(root.join("project.prproj"), root.join("drift"), None, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("failed admission")
            && error.contains("native VideoStream Duration")
            && error.contains("differs from the file duration"),
        "{error}"
    );
    assert!(!root.join("drift").exists());
    // Sequence clocks must be positive. Physical source clocks may be unlisted
    // but must still agree with the inspected frame count and duration.
    let xml = one_second();
    for (tag, ticks, output_name, reason) in [
        (
            "VideoTrackGroup",
            0,
            "sequence-fps",
            "unsupported video frame rate",
        ),
        (
            "VideoStream",
            123,
            "source-fps",
            "native VideoStream Duration does not match its constant frame duration and file sample count",
        ),
    ] {
        let document = roxmltree::Document::parse(&xml).unwrap();
        let record = document
            .descendants()
            .find(|node| node.has_tag_name(tag) && node.attribute("ObjectID").is_some())
            .unwrap();
        let rate = record
            .descendants()
            .find(|node| node.has_tag_name("FrameRate"))
            .unwrap();
        let mut invalid = xml.clone();
        invalid.replace_range(rate.range(), &format!("<FrameRate>{ticks}</FrameRate>"));
        fixture(root, &invalid);
        let output = root.join(output_name);
        let error = premiere_to_tesseract(root.join("project.prproj"), &output, None, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{tag}: {error}");
        if tag == "VideoStream" {
            assert!(error.contains("failed admission"), "{error}");
        }
        assert!(!output.exists());
    }
}

/// The direct `FrameRect` of every native record `tag` in `xml`.
#[cfg(feature = "ffmpeg-library")]
fn frames<'a>(xml: &'a roxmltree::Document<'_>, tag: &str) -> Vec<&'a str> {
    xml.descendants()
        .filter(|node| node.has_tag_name(tag))
        .filter_map(|node| {
            node.children()
                .find(|child| child.has_tag_name("FrameRect"))
        })
        .map(|frame| frame.text().unwrap())
        .collect()
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn custom_canvas_premiere_build_writes_the_document_size_and_reimports_it() {
    // A 1920x1080 source centred on its own frame at Scale 50, a quarter of
    // the way across and three quarters down a portrait canvas, under a
    // full-frame red solid, an adjustment layer and a small blue rectangle,
    // which export as a Color Matte, an adjustment clip and a graphic Shape.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    doc["dimensions"] = json!({"width": 1080, "height": 1920});
    let identity = json!({
        "anchorPoint": [0.0, 0.0], "position": [0.0, 0.0],
        "scale": [100.0, 100.0], "rotation": 0.0, "opacity": 100.0
    });
    let layers = doc["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["transform"] = json!({
        "anchorPoint": [960.0, 540.0], "position": [270.0, 1440.0],
        "scale": [50.0, 50.0], "rotation": 0.0, "opacity": 100.0
    });
    layers[1]["rect"]["size"] = json!([1080.0, 1920.0]);
    let range = json!({"start": 0, "duration": 1000});
    layers.splice(
        0..0,
        [
            json!({
                "type": "Rect", "id": 5, "name": "Blue box", "activeRange": range,
                "transform": identity,
                "rect": {"size": [200.0, 100.0], "position": [100.0, 1600.0], "fillColor": [0, 0, 1, 1]}
            }),
            json!({
                "type": "Adjustment", "id": 4, "name": "Adjust", "activeRange": range,
                "transform": identity
            }),
            json!({
                "type": "Rect", "id": 3, "name": "Red solid",
                "activeRange": {"start": 0, "duration": 500}, "transform": identity,
                "rect": {"size": [1080.0, 1920.0], "fillColor": [1, 0, 0, 1]}
            }),
        ],
    );
    let source = archive(root, &doc, &root.join("source.mp4"));
    let checked = tesseract_to_premiere(&source, root.join("checked"), true).unwrap();
    assert!(!root.join("checked").exists());
    let written = tesseract_to_premiere(&source, root.join("native"), false).unwrap();
    assert!(written.is_empty(), "{written:?}");
    assert_eq!(checked, written);

    let native = root.join("native/project.prproj");
    let xml = read_xml(&native);
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    // The sequence and its four clips carry the canvas, and so do the matte,
    // adjustment and graphic generators; the video keeps its own frame.
    assert_eq!(frames(&parsed, "VideoTrackGroup"), ["0,0,1080,1920"]);
    assert_eq!(frames(&parsed, "VideoClipTrackItem"), ["0,0,1080,1920"; 4]);
    let mut streams = frames(&parsed, "VideoStream");
    streams.sort_unstable();
    assert_eq!(
        streams,
        [
            "0,0,1080,1920",
            "0,0,1080,1920",
            "0,0,1080,1920",
            "0,0,1920,1080"
        ]
    );
    assert_eq!(
        fs::read(root.join("native/media/source.mp4")).unwrap(),
        MEDIA
    );
    let (project, omissions) = premiere_file::PrProjectFile::load(&native).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project.sequences().next().unwrap().dimensions(),
        [1080, 1920]
    );

    let omissions = premiere_to_tesseract(&native, root.join("reimported"), None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(reimported["dimensions"], doc["dimensions"]);
    let layers = reimported["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Shape", "Adjustment", "Rect", "Video", "Rect"]
    );
    // The box keeps its canvas pixels (y 1600 to 1700, off a 1920x1080 frame).
    assert_eq!(
        layers[0]["shape"]["path"]["commands"][0],
        json!({"type": "moveTo", "x": 100.0, "y": 1600.0})
    );
    assert_eq!(
        layers[0]["shape"]["path"]["commands"][2],
        json!({"type": "lineTo", "x": 300.0, "y": 1700.0})
    );
    assert_eq!(layers[2]["rect"]["size"], json!([1080.0, 1920.0]));
    assert_eq!(
        (*crate::test_support::layer_range(&layers[2])),
        json!({"start": 0, "duration": 500})
    );
    let video = &layers[3];
    assert_eq!(video["transform"]["anchorPoint"], json!([960.0, 540.0]));
    assert_eq!(video["transform"]["position"], json!([270.0, 1440.0]));
    assert_eq!(video["transform"]["scale"], json!([50.0, 50.0]));
    assert_eq!(
        video["source"]["sourceRect"],
        json!({"x": 0.0, "y": 0.0, "width": 1920.0, "height": 1080.0})
    );
    assert_eq!(layers[4]["rect"]["size"], json!([1080.0, 1920.0]));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn a_group_on_a_custom_canvas_exports_as_a_nest_of_that_size_and_reimports_it() {
    // The portrait document's 1920x1080 source, at Scale 50 a quarter of the
    // way across and three quarters down, inside a plain group: the group
    // exports as a nested sequence of the document's size, placed with that
    // frame, and reads back as a group whose video keeps its transform.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    doc["dimensions"] = json!({"width": 1080, "height": 1920});
    let layers = doc["composition"]["layers"].as_array_mut().unwrap();
    let identity = layers[1]["transform"].clone();
    let mut video = layers[0].take();
    video["parent"] = json!(10);
    video["transform"] = json!({
        "anchorPoint": [960.0, 540.0], "position": [270.0, 1440.0],
        "scale": [50.0, 50.0], "rotation": 0.0, "opacity": 100.0
    });
    layers[0] = json!({
        "type": "Group", "id": 10, "name": "Inner", "blendMode": "normal",
        "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
        "transform": identity, "layers": [video]
    });
    layers[1]["rect"]["size"] = json!([1080.0, 1920.0]);
    let source = archive(root, &doc, &root.join("source.mp4"));
    let omissions = tesseract_to_premiere(&source, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    let native = root.join("native/project.prproj");
    let xml = read_xml(&native);
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    // The root and nested sequences, the nest placement and the clip inside
    // the nest carry the canvas; the video keeps its own frame.
    assert_eq!(xml.matches("<Sequence ObjectUID=").count(), 2);
    assert_eq!(frames(&parsed, "VideoTrackGroup"), ["0,0,1080,1920"; 2]);
    assert_eq!(frames(&parsed, "VideoClipTrackItem"), ["0,0,1080,1920"; 2]);
    assert_eq!(frames(&parsed, "VideoStream"), ["0,0,1920,1080"]);

    let root_sequence = exported_root_sequence(&native);
    let omissions = premiere_to_tesseract(
        &native,
        root.join("reimported"),
        Some(&root_sequence),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(reimported["dimensions"], doc["dimensions"]);
    let layers = reimported["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Group", "Rect"]
    );
    let group = &layers[0];
    assert_eq!(group["transform"]["anchorPoint"], json!([0.0, 0.0]));
    assert_eq!(group["transform"]["position"], json!([0.0, 0.0]));
    assert_eq!(group["transform"]["scale"], json!([100.0, 100.0]));
    let [video] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("one layer in the group: {group}");
    };
    assert_eq!(video["type"], "Video");
    assert_eq!(video["transform"]["anchorPoint"], json!([960.0, 540.0]));
    assert_eq!(video["transform"]["position"], json!([270.0, 1440.0]));
    assert_eq!(video["transform"]["scale"], json!([50.0, 50.0]));
    assert_eq!(layers[1]["rect"]["size"], json!([1080.0, 1920.0]));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn tesseract_to_premiere_check_builds_native_records_and_rejects_corrupt_media_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    let source = archive(root, &doc, &root.join("source.mp4"));
    let mut bytes = fs::read(&source).unwrap();
    let media_start = bytes.windows(MEDIA.len()).position(|w| w == MEDIA).unwrap();
    // Change one byte in compressed picture data, leaving MP4 headers valid.
    let mdat = MEDIA.windows(4).position(|w| w == b"mdat").unwrap();
    bytes[media_start + mdat + 40] ^= 1;
    fs::write(&source, bytes).unwrap();
    let check_error = tesseract_to_premiere(&source, root.join("native"), true)
        .unwrap_err()
        .to_string();
    let write_error = tesseract_to_premiere(&source, root.join("native"), false)
        .unwrap_err()
        .to_string();
    assert_eq!(check_error, write_error);
    assert!(!root.join("native").exists());

    doc["composition"]["name"] = json!("x".repeat(5000));
    write_archive(
        &root.join("long-name.tsrct"),
        &doc,
        &root.join("source.mp4"),
    );
    let check_error =
        tesseract_to_premiere(root.join("long-name.tsrct"), root.join("long-out"), true)
            .unwrap_err()
            .to_string();
    assert!(check_error.contains("sequence name"), "{check_error}");
    assert!(!root.join("long-out").exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn omitted_video_source_rect_uses_inspected_media_dimensions() {
    for fit in [None, Some("cover"), Some("contain"), Some("stretch")] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut doc = document(root);
        doc["dimensions"] = json!({"width": 1080, "height": 1920});
        doc["composition"]["layers"][1]["rect"]["size"] = json!([1080, 1920]);
        let video = &mut doc["composition"]["layers"][0];
        let source = video["source"].as_object_mut().unwrap();
        source.remove("sourceRect");
        match fit {
            Some(fit) => {
                source.insert("fit".into(), json!(fit));
            }
            None => {
                source.remove("fit");
            }
        }
        video["transform"] = json!({
            "anchorPoint": [32, 18], "position": [270, 1440],
            "scale": [50, 50], "rotation": 10, "opacity": 100
        });
        let media = root.join("source.mp4");
        let media_bytes = include_bytes!("../fixtures/source_rect_64x36.mp4");
        fs::write(&media, media_bytes).unwrap();
        let archive_path = archive(root, &doc, &media);
        let original = fs::read(&archive_path).unwrap();
        let package = root.join("native");
        let checked = tesseract_to_premiere(&archive_path, &package, true).unwrap();
        assert!(!package.exists());
        let written = tesseract_to_premiere(&archive_path, &package, false).unwrap();
        assert_eq!(checked, written);
        assert!(written.is_empty(), "{fit:?}: {written:?}");
        let native = package.join("project.prproj");
        let xml = read_xml(&native);
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(frames(&parsed, "VideoTrackGroup"), ["0,0,1080,1920"]);
        assert_eq!(frames(&parsed, "VideoStream"), ["0,0,64,36"]);
        assert_eq!(fs::read(&archive_path).unwrap(), original);
        assert_eq!(
            fs::read(package.join("media/source.mp4")).unwrap(),
            media_bytes
        );

        premiere_to_tesseract(&native, root.join("reimported"), None, false).unwrap();
        let reimported = TesseractFile::open(first_project(&root.join("reimported")))
            .unwrap()
            .project_json()
            .unwrap();
        let video = &reimported["composition"]["layers"][0];
        assert_eq!(
            video["source"]["sourceRect"],
            json!({
                "x": 0.0, "y": 0.0, "width": 64.0, "height": 36.0
            })
        );
        assert_eq!(video["transform"]["anchorPoint"], json!([32.0, 18.0]));
        assert_eq!(video["transform"]["position"], json!([270.0, 1440.0]));
        assert_eq!(video["transform"]["scale"], json!([50.0, 50.0]));
        assert_eq!(video["transform"]["rotation"], json!(10.0));
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn reversing_video_without_source_rect_is_omitted_without_inspecting_its_media() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    let layers = doc["composition"]["layers"].as_array_mut().unwrap();
    let mut native = layers[0].take();
    native["parent"] = json!(10);
    let mut remapped = native.clone();
    remapped["id"] = json!(3);
    remapped["name"] = json!("Unsupported remapped footage");
    remapped["source"]
        .as_object_mut()
        .unwrap()
        .remove("sourceRect");
    remapped["source"]["assetId"] = json!("missing-omitted-media");
    remapped["playback"] = json!({
        "type": "windowed", "inputRange": {"start": 0, "duration": 1000},
        "mapping": {"type": "timeRemap", "property": {
            "before": "inactive", "after": "inactive", "keyframes": [
                {"id": "in", "time": 0, "value": 0, "easing": {"type": "linear"}},
                {"id": "middle", "time": 500, "value": 1100, "easing": {"type": "linear"}},
                {"id": "out", "time": 1000, "value": 1000, "easing": {"type": "linear"}}
            ]
        }}, "inputOffsetMs": 0
    });
    layers[0] = json!({
        "type": "Group", "id": 10, "name": "Native and remapped pictures",
        "playback": crate::test_support::linear_playback(
            json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})
        ),
        "transform": layers[1]["transform"], "layers": [remapped, native]
    });
    let source = root.join("source.tsrct");
    fs::write(
        root.join("omitted.mp4"),
        b"invalid media must remain uninspected",
    )
    .unwrap();
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&doc).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "missing-omitted-media",
            root.join("omitted.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(&source)
        .unwrap();
    let output = root.join("native");
    let checked = tesseract_to_premiere(&source, &output, true).unwrap();
    assert!(!output.exists());
    let written = tesseract_to_premiere(&source, &output, false).unwrap();
    assert_eq!(checked, written);
    assert!(
        written.iter().any(|item| {
            item.record.contains("layer 3") && item.reason.contains("time remapping")
        }),
        "{written:?}"
    );
    let xml = read_xml(&output.join("project.prproj"));
    assert_eq!(xml.matches("<VideoClipTrackItem ObjectID=").count(), 2);
    assert_eq!(fs::read(output.join("media/source.mp4")).unwrap(), MEDIA);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn source_rect_cannot_disagree_with_packaged_video_frame() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    doc["composition"]["layers"][0]["transform"]["anchorPoint"] = json!([960, 540]);
    doc["composition"]["layers"][0]["transform"]["position"] = json!([960, 540]);
    // A sourceRect alone does not prove the underlying video's natural size.
    // The publication boundary inspects the packaged bytes before writing XML.
    let media = root.join("source.mp4");
    fs::write(&media, include_bytes!("../fixtures/source_rect_64x36.mp4")).unwrap();
    let source = archive(root, &doc, &media);
    for check in [true, false] {
        let output = root.join(format!("native-{check}"));
        let error = tesseract_to_premiere(&source, &output, check)
            .unwrap_err()
            .to_string();
        assert!(error.contains("matching sourceRect dimensions"), "{error}");
        assert!(!output.exists());
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn tesseract_to_premiere_preserves_supported_opacity_and_output_safety() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut doc = document(root);
    doc["composition"]["layers"][0]["transform"]["opacity"] = json!(99);
    let source = archive(root, &doc, &root.join("source.mp4"));
    let package = root.join("exported");
    let checked = tesseract_to_premiere(&source, &package, true).unwrap();
    assert!(!package.exists());
    let omissions = tesseract_to_premiere(&source, &package, false).unwrap();
    assert_eq!(checked, omissions);
    assert!(
        !omissions.iter().any(|item| item.reason.contains("opacity")),
        "supported opacity must be written: {omissions:?}"
    );
    assert!(package.join("project.prproj").exists());
    let reimported = root.join("reimported");
    premiere_to_tesseract(package.join("project.prproj"), &reimported, None, false).unwrap();
    let reopened = TesseractFile::open(first_project(&reimported)).unwrap();
    assert_eq!(
        reopened.project_json().unwrap()["composition"]["layers"][0]["transform"]["opacity"],
        json!(99.0)
    );

    let existing = root.join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("sentinel"), b"keep").unwrap();
    let error = tesseract_to_premiere(&source, &existing, false).unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert_eq!(fs::read(existing.join("sentinel")).unwrap(), b"keep");
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn tesseract_to_premiere_rejects_inexact_media_duration_and_endpoints_before_output() {
    for (label, duration, intrinsic, start, length, expected) in [
        ("inflated", 1.0, 1100, 0, 1000, "sourceIntrinsicDuration"),
        ("missing-frame", 0.067, 1000, 969, 67, "frame past"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let mut doc = document(root);
        doc["duration"] = json!(duration);
        let video = &mut doc["composition"]["layers"][0];
        video["sourceIntrinsicDuration"] = json!(intrinsic);
        video["sourceRange"] = json!({"start":start,"duration":length});
        video["playback"] = crate::test_support::linear_playback(
            json!({"start": 0, "duration": length}),
            json!({"start": start, "duration": length}),
        );
        let source = archive(root, &doc, &root.join("source.mp4"));
        let output = root.join(label);
        let error = tesseract_to_premiere(source, &output, false).unwrap_err();
        assert!(error.to_string().contains(expected), "{label}: {error}");
        assert!(!output.exists());
    }
}

#[test]
fn text_the_premiere_writer_cannot_encode_fails_check_and_execution_alike() {
    // A media-free archive of the editable fixture with its video replaced by
    // one text layer whose name holds a control character.
    let mut doc = crate::test_support::editable_document();
    doc["duration"] = json!(1.0);
    let layers = &mut doc["composition"]["layers"];
    let transform = layers[1]["transform"].clone();
    layers[0] = json!({
        "type": "Text",
        "id": 1,
        "name": "bad\u{1}name",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": transform,
        "sourceText": {
            "text": "Title",
            "fontFamily": "Arial-BoldMT",
            "fontStyle": "",
            "fontSize": 80,
            "fillColor": [1, 1, 1, 1]
        }
    });
    layers[1]["activeRange"]["duration"] = json!(1000);
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("control-name.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&doc).unwrap())
        .unwrap()
        .write(&source)
        .unwrap();
    let [check, execute] = [true, false].map(|check| {
        let output = dir.path().join(format!("out-{check}"));
        let error = tesseract_to_premiere(&source, &output, check)
            .unwrap_err()
            .to_string();
        assert!(!output.exists());
        error
    });
    assert!(check.contains("text layer name"), "{check}");
    assert_eq!(check, execute);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_stream_rate_and_absolute_aliases_must_match_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let xml = one_second().replace("<VideoStream ObjectID=\"8\"><Duration>254016000000</Duration><FrameRate>8467200000</FrameRate>",
        "<VideoStream ObjectID=\"8\"><Duration>254016000000</Duration><FrameRate>10160640000</FrameRate>");
    fixture(root, &xml);
    assert!(premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("rate-out"),
        None,
        true
    )
    .unwrap_err()
    .to_string()
    .contains("native VideoStream FrameRate"));
    assert!(!root.join("rate-out").exists());
    let other = root.join("other.mp4");
    fs::write(&other, [MEDIA, b"different bytes"].concat()).unwrap();
    let xml = one_second().replace("<RelativePath>media/source.mp4</RelativePath>",
        &format!("<RelativePath>media/source.mp4</RelativePath><FilePath>{}</FilePath><ActualMediaFilePath>{}</ActualMediaFilePath>",other.display(),other.display()));
    fixture(root, &xml);
    let check = premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("alias-out"),
        None,
        true,
    )
    .unwrap_err()
    .to_string();
    let execute = premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("alias-out"),
        None,
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(check.contains("identify different bytes"));
    assert_eq!(check, execute);
    assert!(!root.join("alias-out").exists());
    fs::write(&other, MEDIA).unwrap();
    premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("same-out"),
        None,
        true,
    )
    .unwrap();
    fs::remove_file(&other).unwrap();
    premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("missing-out"),
        None,
        true,
    )
    .unwrap();
    assert!(!root.join("missing-out").exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn media_metadata_and_edit_lists_are_checked_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let native = fixture(root, &one_second());
    let good = root.join("good.tsrct");
    build_tesseract_file(&native, &good, None).unwrap();
    let document = TesseractFile::open(&good).unwrap().project_json().unwrap();
    let mut excessive_samples = MEDIA.to_vec();
    let stsz = excessive_samples
        .windows(4)
        .position(|bytes| bytes == b"stsz")
        .unwrap();
    excessive_samples[stsz + 8..stsz + 12].copy_from_slice(&1_u32.to_be_bytes());
    excessive_samples[stsz + 12..stsz + 16].copy_from_slice(&31_u32.to_be_bytes());
    fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = u32::try_from(payload.len() + 8).unwrap().to_be_bytes();
        [size.as_slice(), kind, payload].concat()
    }
    let mfhd = mp4_box(b"mfhd", &[0, 0, 0, 0, 0, 0, 0, 1]);
    let tfhd = mp4_box(b"tfhd", &[0, 0, 0, 0, 0, 0, 0, 1]);
    let trun = mp4_box(b"trun", &[0, 0, 0, 0, 255, 255, 255, 255]);
    let traf = mp4_box(b"traf", &[tfhd, trun].concat());
    let moof = mp4_box(b"moof", &[mfhd, traf].concat());
    // Two runs would overflow the dependency's sample_count() accumulator.
    let fragments = [MEDIA, moof.as_slice(), moof.as_slice()].concat();
    let mut shortened = MEDIA.to_vec();
    let edit = shortened
        .windows(4)
        .position(|bytes| bytes == b"elst")
        .unwrap();
    shortened[edit + 12..edit + 16].copy_from_slice(&500_u32.to_be_bytes());
    let mut shifted = MEDIA.to_vec();
    shifted[edit + 16..edit + 20].copy_from_slice(&512_u32.to_be_bytes());
    let mut speed = MEDIA.to_vec();
    speed[edit + 20..edit + 22].copy_from_slice(&2_u16.to_be_bytes());
    let mut translated = MEDIA.to_vec();
    let tkhd = translated
        .windows(4)
        .position(|bytes| bytes == b"tkhd")
        .unwrap();
    assert_eq!(translated[tkhd + 4], 0);
    translated[tkhd + 68..tkhd + 72].copy_from_slice(&(100_i32 << 16).to_be_bytes());
    let mut display_size = MEDIA.to_vec();
    display_size[tkhd + 80..tkhd + 84].copy_from_slice(&(1919_u32 << 16).to_be_bytes());
    let mut container_aspect = MEDIA.to_vec();
    let aspect = container_aspect
        .windows(4)
        .position(|bytes| bytes == b"pasp")
        .unwrap();
    container_aspect[aspect + 4..aspect + 8].copy_from_slice(&2_u32.to_be_bytes());
    let mut invalid_aspect = MEDIA.to_vec();
    invalid_aspect[aspect + 4..aspect + 8].copy_from_slice(&0_u32.to_be_bytes());
    for (name, bytes, message) in [
        (
            "zero-pixel-aspect",
            invalid_aspect.as_slice(),
            "pixel aspect ratio must be positive",
        ),
        (
            // The fixture's one chunk holds 30 samples, so the 31st one-byte
            // sample has no chunk offset.
            "sample-count",
            excessive_samples.as_slice(),
            "sample table does not cover every declared sample",
        ),
        ("fragments", fragments.as_slice(), "fragmented MP4"),
        ("translated", translated.as_slice(), "display transform"),
        (
            "display-size",
            display_size.as_slice(),
            "display dimensions",
        ),
        ("shortened", shortened.as_slice(), "edit list"),
        ("shifted", shifted.as_slice(), "edit list"),
        ("speed", speed.as_slice(), "edit list"),
        (
            "container-aspect",
            container_aspect.as_slice(),
            "pixel aspect ratio",
        ),
    ] {
        fs::write(root.join("media/source.mp4"), bytes).unwrap();
        let bad_archive = root.join(format!("{name}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset(
                "premiere-video-1",
                root.join("media/source.mp4"),
                AssetKind::Video,
            )
            .unwrap()
            .write(&bad_archive)
            .unwrap();
        for (command, source) in [
            ("premiere-to-tesseract", native.as_path()),
            ("tesseract-to-premiere", bad_archive.as_path()),
        ] {
            for check in [true, false] {
                let output = root.join(format!("{name}-{command}-{check}"));
                let error = match command {
                    "premiere-to-tesseract" => {
                        premiere_to_tesseract(source, &output, None, check).unwrap_err()
                    }
                    "tesseract-to-premiere" => {
                        tesseract_to_premiere(source, &output, check).unwrap_err()
                    }
                    _ => unreachable!("fixed test directions"),
                }
                .to_string();
                assert!(error.contains(message), "{name}/{command}: {error}");
                assert!(!output.exists());
            }
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn generated_media_paths_must_match_packaged_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let doc = document(root);
    let path = archive(root, &doc, &root.join("source.mp4"));
    tesseract_to_premiere(&path, root.join("carriage\rreturn"), false).unwrap();
    let native = read_xml(&root.join("carriage\rreturn/project.prproj"));
    let parsed = roxmltree::Document::parse(&native).unwrap();
    let paths: Vec<_> = parsed
        .descendants()
        .filter(|n| n.has_tag_name("FilePath"))
        .map(|n| n.text().unwrap())
        .collect();
    assert!(!paths.is_empty());
    for path in paths {
        assert!(
            Path::new(path).exists(),
            "successful tesseract_to_premiere references nonexistent path {path:?}"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn malformed_mp4_boxes_fail_check_and_execution_without_outputs() {
    for (box_type, field_offset, value) in [
        (b"stco", 12, MEDIA.len() as u32 + 1000),
        (b"stco", 12, MEDIA.len() as u32 - 1),
        (b"stsz", 16, 0),
        (b"stsz", 16, u32::MAX),
        (b"stsc", 12, 0),
        (b"stsc", 16, 0),
        (b"\xa9nam", 4, 8),
        (b"\xa9nam", 4, 15),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let doc = document(root);
        let mut bytes = MEDIA.to_vec();
        let title = bytes.windows(4).position(|s| s == b"\xa9too").unwrap();
        bytes[title..title + 4].copy_from_slice(b"\xa9nam");
        let position = bytes.windows(4).position(|s| s == box_type).unwrap() + field_offset;
        bytes[position..position + 4].copy_from_slice(&value.to_be_bytes());
        fs::create_dir(root.join("media")).unwrap();
        let media = root.join("media/source.mp4");
        fs::write(&media, bytes).unwrap();
        let tesseract_source = archive(root, &doc, &media);
        let native = root.join("project.prproj");
        let xml = XML
            .replace("1270080000000", "254016000000")
            .replace("2540160000000", "254016000000");
        write_prproj(&native, &xml);
        for (command, input) in [
            ("premiere-to-tesseract", &native),
            ("tesseract-to-premiere", &tesseract_source),
        ] {
            for check in [false, true] {
                let output = root.join(format!("{command}-{check}"));
                let failed = match command {
                    "premiere-to-tesseract" => {
                        premiere_to_tesseract(input, &output, None, check).is_err()
                    }
                    "tesseract-to-premiere" => {
                        tesseract_to_premiere(input, &output, check).is_err()
                    }
                    _ => unreachable!("fixed test directions"),
                };
                assert!(failed, "{command} {box_type:?} {field_offset} {value}");
                assert!(!output.exists());
            }
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn repeated_media_still_validates_each_occurrences_source_duration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    let mut first = doc["composition"]["layers"][0].clone();
    first["playback"] = crate::test_support::linear_playback(
        json!({"start": first["playback"]["inputRange"]["start"], "duration": 500}),
        json!({"start": first["sourceRange"]["start"], "duration": 500}),
    );
    first["sourceRange"]["duration"] = json!(500);
    let mut second = first.clone();
    second["id"] = json!(3);
    second["playback"]["inputRange"]["start"] = json!(500);
    second["playback"]["mapping"]["input"]["start"] = json!(500);
    second["sourceIntrinsicDuration"] = json!(1100);
    let canvas = doc["composition"]["layers"][1].clone();
    doc["composition"]["layers"] = json!([first, second, canvas]);
    let path = archive(root, &doc, &root.join("source.mp4"));
    let output = root.join("out");
    let error = tesseract_to_premiere(&path, &output, true).unwrap_err();
    assert!(
        error.to_string().contains("sourceIntrinsicDuration"),
        "{error}"
    );
    assert!(!output.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn archive_media_validation_still_rejects_wrong_kind_and_container() {
    for (name, kind, expected) in [
        (
            "source.mp4",
            AssetKind::Audio,
            "writer requires packaged media with a matching kind",
        ),
        (
            "source.mkv",
            AssetKind::Video,
            "video file extension \"mkv\" is unsupported; conversion accepts MP4 or QuickTime MOV video",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let doc = document(root);
        let media = root.join(name);
        fs::write(&media, MEDIA).unwrap();
        let input = root.join("input.tsrct");
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&doc).unwrap())
            .unwrap()
            .add_asset("premiere-video-1", &media, kind)
            .unwrap()
            .write(&input)
            .unwrap();
        let output = root.join("output");
        let error = tesseract_to_premiere(&input, &output, true).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert!(!output.exists());
    }
}

#[test]
fn web_project_file_is_not_an_editable_document_archive() {
    // A web Project is not the standalone editable-document envelope.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = root.join("web.json");
    fs::write(&input, br#"{"fx_compositions":[],"captions":[]}"#).unwrap();
    let output = root.join("out");
    assert!(tesseract_to_premiere(&input, &output, true).is_err());
    assert!(!output.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn container_color_profiles_are_validated_in_both_directions() {
    for (payload, error) in [
        (b"nclc\x00\x01\x00\x01\x00\x01".as_slice(), None),
        (b"nclx\x00\x01\x00\x01\x00\x01\x00", None),
        (b"nclx\x00\x01\x00\x01\x00\x01\x80", Some("full-range")),
        (b"nclx\x00\x01\x00\x01\x00\x01\x01", Some("reserved")),
        // BT.2020 PQ passes through; BT.601 has no pass-through table entry.
        (b"nclx\x00\x09\x00\x10\x00\x09\x00", None),
        (
            b"nclx\x00\x05\x00\x06\x00\x06\x00",
            Some("colour metadata 5/6/6 is unsupported"),
        ),
        (b"prof\x00\x01\x00\x01\x00\x01", Some("color profile")),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let doc = document(root);
        let media = root.join("source.mp4");
        fs::write(&media, with_container_color(payload)).unwrap();
        let path = archive(root, &doc, &media);
        let output = root.join("native");
        if let Some(expected) = error {
            let error = tesseract_to_premiere(&path, &output, false).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert!(!output.exists());
        } else {
            tesseract_to_premiere(&path, &output, false).unwrap();
            build_tesseract_file(
                &output.join("project.prproj"),
                &root.join("roundtrip.tsrct"),
                None,
            )
            .unwrap();
        }
        // Also check unsupported declarations on the native premiere_to_tesseract path.
        fs::create_dir(root.join("media")).unwrap();
        fs::copy(&media, root.join("media/source.mp4")).unwrap();
        let xml = XML
            .replace("1270080000000", "254016000000")
            .replace("2540160000000", "254016000000");
        let native = root.join("project.prproj");
        write_prproj(&native, &xml);
        let destination = root.join("tesseract_output.tsrct");
        let result = build_tesseract_file(&native, &destination, None);
        if let Some(expected) = error {
            assert!(result.unwrap_err().to_string().contains(expected));
            assert!(!destination.exists());
        } else {
            result.unwrap();
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
fn with_container_color(payload: &[u8]) -> Vec<u8> {
    let mut bytes = MEDIA.to_vec();
    let pasp = bytes.windows(4).position(|b| b == b"pasp").unwrap();
    let old_size = u32::from_be_bytes(bytes[pasp - 4..pasp].try_into().unwrap());
    assert_eq!(old_size, 16);
    let new_size = u32::try_from(payload.len() + 8).unwrap();
    let delta = i64::from(new_size) - i64::from(old_size);
    for kind in [
        b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd", b"avc1",
    ] {
        let pos = bytes.windows(4).rposition(|b| b == kind).unwrap();
        let size = u32::from_be_bytes(bytes[pos - 4..pos].try_into().unwrap());
        bytes[pos - 4..pos].copy_from_slice(
            &u32::try_from(i64::from(size) + delta)
                .unwrap()
                .to_be_bytes(),
        );
    }
    let replacement = [new_size.to_be_bytes().as_slice(), b"colr", payload].concat();
    bytes.splice(pasp - 4..pasp - 4 + old_size as usize, replacement);
    // moov precedes mdat, so growing metadata shifts every chunk offset.
    let stco = bytes.windows(4).position(|b| b == b"stco").unwrap();
    let count = u32::from_be_bytes(bytes[stco + 8..stco + 12].try_into().unwrap());
    for i in 0..count as usize {
        let pos = stco + 12 + i * 4;
        let offset = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap());
        bytes[pos..pos + 4].copy_from_slice(
            &u32::try_from(i64::from(offset) + delta)
                .unwrap()
                .to_be_bytes(),
        );
    }
    bytes
}
