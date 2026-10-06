//! Supplementary CPU coverage; independent native/Adobe proof is separate.
use std::{fs, io::Read};
use tesseract_file::TesseractFile;

fn numbered_project(root: &std::path::Path) -> std::path::PathBuf {
    let source = include_str!("../../tests/fixtures/one-clip.xml")
        .replace("8467200000", "10160640000")
        .replace("1270080000000", "609638400000")
        .replace("2540160000000", "609638400000")
        .replace("media/source.mp4", "media/frame_00042.png")
        .replace("<VideoStream ObjectID=\"8\">", "<VideoStream ObjectID=\"8\"><IsNumberedStills>true</IsNumberedStills><AlphaType>1</AlphaType>")
        .replace("1920,1080", "2,1");
    fs::create_dir(root.join("media")).unwrap();
    for number in 42..102 {
        let image =
            image::RgbaImage::from_raw(2, 1, vec![number as u8, 0, 0, 128, 0, number as u8, 0, 0])
                .unwrap();
        image
            .save(root.join(format!("media/frame_{number:05}.png")))
            .unwrap();
    }
    let project = root.join("project.prproj");
    fs::write(&project, source).unwrap();
    project
}

#[test]
fn numbered_images_keep_sixty_distinct_frames_and_source_cadence() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    let archive_path = dir.path().join("project.tsrct");
    crate::tests::support::convert_tesseract_file(&project, None)
        .unwrap()
        .write_to_staging(&archive_path)
        .unwrap();
    let archive = TesseractFile::open(archive_path).unwrap();
    let value = archive.project_json().unwrap();
    let group = &value["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    let frames = group["layers"].as_array().unwrap();
    assert_eq!(frames.len(), 60);
    for (index, frame) in frames.iter().enumerate() {
        assert_eq!(frame["type"], "Image");
        assert_eq!(crate::test_support::layer_range(frame)["start"], index * 40);
        assert_eq!(crate::test_support::layer_range(frame)["duration"], 40);
        let asset = frame["source"]["assetId"].as_str().unwrap();
        let mut bytes = Vec::new();
        archive
            .asset(asset)
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(
            bytes,
            fs::read(
                dir.path()
                    .join(format!("media/frame_{:05}.png", index + 42))
            )
            .unwrap()
        );
        let pixels = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(
            pixels.as_raw(),
            &[index as u8 + 42, 0, 0, 128, 0, index as u8 + 42, 0, 0]
        );
    }
}

fn ticks(ms: i64) -> i64 {
    ms * crate::schema::TICKS_PER_MILLISECOND
}

fn trim_project(project: &std::path::Path) {
    let xml = fs::read_to_string(project)
        .unwrap()
        .replace(
            "<End>609638400000</End>",
            &format!("<Start>{}</Start><End>{}</End>", ticks(400), ticks(800)),
        )
        .replace(
            "<InPoint>0</InPoint><OutPoint>609638400000</OutPoint>",
            &format!(
                "<InPoint>{}</InPoint><OutPoint>{}</OutPoint>",
                ticks(80),
                ticks(480)
            ),
        );
    fs::write(project, xml).unwrap();
}

fn add_repeated_placement(project: &std::path::Path, reverse: bool) {
    let mut xml = fs::read_to_string(project).unwrap();
    let mut records = String::new();
    for (tag, id) in [
        ("VideoClipTrackItem", 3),
        ("VideoComponentChain", 4),
        ("SubClip", 5),
        ("VideoClip", 6),
    ] {
        let start = xml.find(&format!("<{tag} ObjectID=\"{id}\">")).unwrap();
        let end = start + xml[start..].find(&format!("</{tag}>")).unwrap() + tag.len() + 3;
        records.push_str(&xml[start..end]);
    }
    for id in 3..=6 {
        records = records
            .replace(
                &format!("ObjectID=\"{id}\""),
                &format!("ObjectID=\"{}\"", id + 10),
            )
            .replace(
                &format!("ObjectRef=\"{id}\""),
                &format!("ObjectRef=\"{}\"", id + 10),
            );
    }
    records = records.replace(
        &format!("<Start>{}</Start><End>{}</End>", ticks(400), ticks(800)),
        &format!("<Start>{}</Start><End>{}</End>", ticks(1000), ticks(1400)),
    );
    if reverse {
        records = records.replace(
            "<VideoClip ObjectID=\"16\"><Clip>",
            "<VideoClip ObjectID=\"16\"><Clip><PlayBackwards>true</PlayBackwards>",
        );
    }
    xml = xml
        .replace(
            "<TrackItem ObjectRef=\"3\"/>",
            "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"13\"/>",
        )
        .replace("</PremiereData>", &format!("{records}</PremiereData>"));
    fs::write(project, xml).unwrap();
}

#[test]
fn numbered_images_trim_repeated_placements_share_complete_relocated_assets() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    fs::create_dir(&original).unwrap();
    let project = numbered_project(&original);
    trim_project(&project);
    add_repeated_placement(&project, false);
    let relocated = dir.path().join("relocated");
    fs::rename(original, &relocated).unwrap();
    let output = dir.path().join("trimmed.tsrct");
    crate::tests::support::build_tesseract_file(&relocated.join("project.prproj"), &output, None)
        .unwrap();
    let archive = TesseractFile::open(output).unwrap();
    assert_eq!(archive.metadata().assets.len(), 60);
    let value = archive.project_json().unwrap();
    let groups: Vec<_> = value["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .collect();
    assert_eq!(groups.len(), 2);
    for (group, start) in groups.iter().zip([400, 1000]) {
        assert_eq!(crate::test_support::layer_range(group)["start"], start);
        assert_eq!(crate::test_support::layer_range(group)["duration"], 400);
        let frames = group["layers"].as_array().unwrap();
        assert_eq!(frames.len(), 10);
        for (index, frame) in frames.iter().enumerate() {
            assert_eq!(crate::test_support::layer_range(frame)["start"], index * 40);
            assert_eq!(crate::test_support::layer_range(frame)["duration"], 40);
            assert_eq!(
                frame["source"]["assetId"],
                format!("premiere-video-1-frame-{}", index + 2)
            );
        }
    }
    assert_eq!(
        groups[0]["layers"][0]["source"],
        groups[1]["layers"][0]["source"]
    );
}

#[test]
fn numbered_images_missing_frame_and_late_change_are_io_or_identity_failures() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    let frame = dir.path().join("media/frame_00099.png");
    let bytes = fs::read(&frame).unwrap();
    fs::remove_file(&frame).unwrap();
    let error = crate::tests::support::convert_tesseract_file(&project, None).unwrap_err();
    assert!(
        matches!(
            error,
            crate::error::BuildError::Io(_) | crate::error::BuildError::IoAt { .. }
        ),
        "{error}"
    );
    fs::write(&frame, bytes).unwrap();
    let pending = crate::tests::support::convert_tesseract_file(&project, None).unwrap();
    fs::copy(dir.path().join("media/frame_00042.png"), &frame).unwrap();
    let error = pending
        .write_to_staging(&dir.path().join("changed.tsrct"))
        .unwrap_err();
    assert!(error.to_string().contains("frame bytes changed"), "{error}");
}

#[test]
fn numbered_images_unsupported_playback_omits_only_its_occurrence() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    trim_project(&project);
    add_repeated_placement(&project, true);
    let (parsed, omissions) = crate::format::PrProjectFile::load_selected(&project, None).unwrap();
    let (sequences, _) = parsed.into_parts();
    assert_eq!(sequences[0].video_items().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.to_string().contains("unit forward playback")),
        "{omissions:?}"
    );
    let output = dir.path().join("supported.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    let value = TesseractFile::open(output).unwrap().project_json().unwrap();
    assert_eq!(
        value["composition"]["layers"][0]["layers"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
}

#[test]
fn numbered_images_edited_choices_and_timing_export_as_ordinary_stills() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    let output = dir.path().join("edited.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    let mut archive = TesseractFile::open(&output).unwrap();
    let mut value = archive.project_json().unwrap();
    let frames = value["composition"]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap();
    frames[0]["source"]["assetId"] = serde_json::json!("premiere-video-1-frame-59");
    // Extend the edited first choice to 80ms and remove the now-covered second.
    let range_key = if frames[0].get("activeRange").is_some() {
        "activeRange"
    } else {
        "timeRange"
    };
    frames[0][range_key]["duration"] = serde_json::json!(80);
    frames.remove(1);
    let json = dir.path().join("edited.json");
    fs::write(&json, serde_json::to_vec(&value).unwrap()).unwrap();
    archive.commit_project_json(&json).unwrap();
    archive.save().unwrap();
    let destination = dir.path().join("premiere");
    crate::premiere_package::save_tesseract_as_premiere(
        &output,
        &destination,
        crate::schema::FrameRate::Fps25,
        false,
    )
    .unwrap();
    let native_path = destination.join("project.prproj");
    let (native, _) = crate::format::PrProjectFile::load_selected(&native_path, None).unwrap();
    let (sequences, media) = native.into_parts();
    let clips: Vec<_> = sequences[0].video_occurrences().collect();
    assert_eq!(clips.len(), 59);
    assert_eq!((clips[0].start_ticks, clips[0].end_ticks), (0, ticks(80)));
    assert!(media[&clips[0].media].is_still());
    assert!(media[&clips[0].media]
        .relative_path
        .as_ref()
        .unwrap()
        .ends_with("frame_00101.png"));
    assert_eq!(
        (clips[1].start_ticks, clips[1].end_ticks),
        (ticks(80), ticks(120))
    );
}

#[test]
fn numbered_images_corrupt_later_frame_is_not_a_first_frame_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    fs::write(dir.path().join("media/frame_00072.png"), b"not an image").unwrap();
    let error = crate::tests::support::convert_tesseract_file(&project, None).unwrap_err();
    assert!(
        error.to_string().contains("unrecognized image data"),
        "{error}"
    );
}

#[test]
fn numbered_images_live_aliases_must_agree_beyond_the_first_frame() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    fs::create_dir(dir.path().join("alias")).unwrap();
    for number in 42..102 {
        fs::copy(
            dir.path().join(format!("media/frame_{number:05}.png")),
            dir.path().join(format!("alias/frame_{number:05}.png")),
        )
        .unwrap();
    }
    fs::copy(
        dir.path().join("media/frame_00042.png"),
        dir.path().join("alias/frame_00043.png"),
    )
    .unwrap();
    let xml = fs::read_to_string(&project).unwrap().replace(
        "<RelativePath>",
        &format!(
            "<FilePath>{}</FilePath><RelativePath>",
            dir.path().join("alias/frame_00042.png").display()
        ),
    );
    fs::write(&project, xml).unwrap();
    let error = crate::tests::support::convert_tesseract_file(&project, None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("aliases identify different frame bytes"),
        "{error}"
    );
}

#[test]
fn numbered_images_opacity_keys_stay_on_trimmed_group_input_clock() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let dir = tempfile::tempdir().unwrap();
    let path = numbered_project(dir.path());
    trim_project(&path);
    let (native, _) = crate::format::PrProjectFile::load_selected(&path, None).unwrap();
    let (mut sequences, media) = native.into_parts();
    let clip = sequences[0].video_tracks[0].clip_mut(0);
    clip.opacity = 25.0;
    clip.enabled = false;
    clip.animations = vec![PrPropertyAnimation::Opacity(vec![
        PrScalarKeyframe {
            source_ticks: ticks(80),
            value: 25.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: ticks(280),
            value: 75.0,
            easing: PrKeyframeEasing::Linear,
        },
    ])];
    let value = crate::tests::support::project_document_with_media(&sequences[0], &media);
    let group = &value["composition"]["layers"][0];
    assert_eq!(group["transform"]["opacity"], 25.0);
    assert_eq!(group["isHidden"], true);
    assert_eq!(crate::test_support::layer_range(group)["start"], 400);
    let entries = value["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["target"]["layerId"], group["id"]);
    assert_eq!(entries[0]["target"]["propertyType"], "opacity");
    let keys = &entries[0]["animator"]["keyframes"];
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[1]["layerTime"], 200);
    assert_eq!(
        keys[1]["value"],
        serde_json::json!({"type":"float", "value":75.0})
    );
}

#[test]
fn numbered_images_source_cadence_selects_sequence_sampled_frames() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    let xml = fs::read_to_string(&project)
        .unwrap()
        .replace("609638400000", "304819200000");
    let (before, stream) = xml.split_once("<VideoStream ObjectID=\"8\">").unwrap();
    fs::write(
        &project,
        format!(
            "{before}<VideoStream ObjectID=\"8\">{}",
            stream.replace("10160640000", "5080320000")
        ),
    )
    .unwrap();
    let output = dir.path().join("mixed.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    let value = TesseractFile::open(output).unwrap().project_json().unwrap();
    let group = &value["composition"]["layers"][0];
    assert_eq!(crate::test_support::layer_range(group)["duration"], 1200);
    let frames = group["layers"].as_array().unwrap();
    assert_eq!(frames.len(), 30);
    assert_eq!(crate::test_support::layer_range(&frames[1])["start"], 40);
    assert_eq!(crate::test_support::layer_range(&frames[1])["duration"], 40);
    assert!(frames[1]["source"]["assetId"]
        .as_str()
        .unwrap()
        .ends_with("-frame-2"));
}

fn commit_edits(archive: &mut TesseractFile, value: &serde_json::Value, path: &std::path::Path) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    archive.commit_project_json(path).unwrap();
    archive.save().unwrap();
}

#[test]
fn numbered_images_mixed_rate_export_keeps_selected_frames_and_short_edit_sibling() {
    for shorten in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let project = numbered_project(dir.path());
        let xml = fs::read_to_string(&project)
            .unwrap()
            .replace("609638400000", "304819200000");
        let (before, stream) = xml.split_once("<VideoStream ObjectID=\"8\">").unwrap();
        fs::write(
            &project,
            format!(
                "{before}<VideoStream ObjectID=\"8\">{}",
                stream.replace("10160640000", "5080320000")
            ),
        )
        .unwrap();
        let output = dir.path().join("mixed.tsrct");
        crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
        let mut archive = TesseractFile::open(&output).unwrap();
        let mut value = archive.project_json().unwrap();
        // A nonzero off-grid group origin must still snap on the absolute grid.
        let group = &mut value["composition"]["layers"][0];
        group["playback"]["inputRange"]["start"] = serde_json::json!(10);
        group["playback"]["mapping"]["input"]["start"] = serde_json::json!(10);
        // Shorten one already-collapsing interval without covering its neighbor.
        if shorten {
            group["layers"][0]["activeRange"]["duration"] = serde_json::json!(5);
        }
        let mut sibling = group["layers"].as_array().unwrap().last().unwrap().clone();
        sibling["id"] = serde_json::json!(10000);
        sibling.as_object_mut().unwrap().remove("parent");
        sibling["activeRange"] = serde_json::json!({"start":1240,"duration":80});
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "type":"Rect", "id":10001, "name":"Explicit black canvas",
                "activeRange":{"start":0,"duration":1320},
                "transform":crate::convert::identity_transform(),
                "rect":{"size":[2,1],"fillColor":[0,0,0,1]}
            }));
        commit_edits(&mut archive, &value, &dir.path().join("edit.json"));
        let destination = dir.path().join("native");
        let report = crate::premiere_package::save_tesseract_as_premiere(
            &output,
            &destination,
            crate::schema::FrameRate::Fps25,
            false,
        )
        .unwrap();
        let (native, _) =
            crate::format::PrProjectFile::load_selected(&destination.join("project.prproj"), None)
                .unwrap();
        let (sequences, media) = native.into_parts();
        let mut clips: Vec<_> = sequences[0]
            .video_occurrences()
            .filter(|clip| media[&clip.media].is_still())
            .collect();
        clips.sort_by_key(|clip| clip.start_ticks);
        let retained = if shorten { 29 } else { 30 };
        assert_eq!(clips.len(), retained + 1, "{report:?}");
        for (index, clip) in clips[..retained].iter().enumerate() {
            assert_eq!(
                (clip.start_ticks, clip.end_ticks),
                (
                    ticks((index + usize::from(shorten)) as i64 * 40),
                    ticks((index + usize::from(shorten)) as i64 * 40 + 40)
                )
            );
            assert!(media[&clip.media]
                .relative_path
                .as_ref()
                .unwrap()
                .ends_with(&format!(
                    "frame_{:05}.png",
                    42 + (index + usize::from(shorten)) * 2
                )));
        }
        assert_eq!(
            (clips[retained].start_ticks, clips[retained].end_ticks),
            (ticks(1240), ticks(1320))
        );
        if shorten {
            assert!(format!("{report:?}").contains("collapses"));
        }
    }
}

#[test]
fn numbered_images_large_source_exports_motion_before_canvas_clipping() {
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    for number in 42..102 {
        let pixels = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 0, 0,
            255, 128, 255, 255, 0, 0, 255, 255, 0, 0,
        ];
        image::RgbaImage::from_raw(4, 2, pixels)
            .unwrap()
            .save(dir.path().join(format!("media/frame_{number:05}.png")))
            .unwrap();
    }
    let xml = fs::read_to_string(&project).unwrap();
    let (before, stream) = xml.split_once("<VideoStream ObjectID=\"8\">").unwrap();
    fs::write(
        &project,
        format!(
            "{before}<VideoStream ObjectID=\"8\">{}",
            stream.replace("0,0,2,1", "0,0,4,2")
        ),
    )
    .unwrap();
    let output = dir.path().join("large.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    let mut archive = TesseractFile::open(&output).unwrap();
    let mut value = archive.project_json().unwrap();
    let group = &mut value["composition"]["layers"][0];
    group["transform"]["scale"] = serde_json::json!([50, 50]);
    group["transform"]["anchorPoint"] = serde_json::json!([2, 1]);
    group["transform"]["position"] = serde_json::json!([0.5, 0.25]);
    commit_edits(&mut archive, &value, &dir.path().join("edit.json"));
    let destination = dir.path().join("native");
    crate::premiere_package::save_tesseract_as_premiere(
        &output,
        &destination,
        crate::schema::FrameRate::Fps25,
        false,
    )
    .unwrap();
    let (native, _) =
        crate::format::PrProjectFile::load_selected(&destination.join("project.prproj"), None)
            .unwrap();
    let (sequences, media) = native.into_parts();
    assert_eq!(
        sequences[0].nest_occurrences().count(),
        0,
        "must not clip a source-sized image in a parent-canvas nest before Motion"
    );
    let clips: Vec<_> = sequences[0]
        .video_occurrences()
        .filter(|clip| media[&clip.media].is_still())
        .collect();
    assert_eq!(clips.len(), 60);
    for clip in clips {
        let source = &media[&clip.media];
        let stream = source.video.as_ref().unwrap();
        assert_eq!((stream.width, stream.height), (4, 2));
        assert_eq!(clip.transform.scale, [50.0, 50.0]);
        assert_eq!(clip.transform.anchor_point, [0.5, 0.5]);
        assert_eq!(clip.transform.position, [0.25, 0.25]);
        let bytes = fs::read(destination.join(source.relative_path.as_ref().unwrap())).unwrap();
        assert_eq!(
            bytes,
            fs::read(dir.path().join("media/frame_00042.png")).unwrap()
        );
    }
    // A large source scaled and placed wholly inside the nest remains safe.
    // Keep that existing nested path when child Motion prevents direct lowering.
    let mut contained = value.clone();
    for image in contained["composition"]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
    {
        image["transform"]["scale"] = serde_json::json!([25, 25]);
        image["transform"]["position"] = serde_json::json!([0.25, 0.125]);
    }
    commit_edits(&mut archive, &contained, &dir.path().join("contained.json"));
    let destination = dir.path().join("contained-native");
    crate::premiere_package::save_tesseract_as_premiere(
        &output,
        &destination,
        crate::schema::FrameRate::Fps25,
        false,
    )
    .unwrap();
    let (native, _) =
        crate::format::PrProjectFile::load_selected(&destination.join("project.prproj"), None)
            .unwrap();
    let sequence = native.single_sequence().unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 1);
    let nest = sequence.nest_occurrences().next().unwrap();
    let children: Vec<_> = nest.sequence.video_occurrences().collect();
    assert_eq!(children.len(), 60);
    for child in children {
        assert_eq!(child.transform.scale, [25.0, 25.0]);
        assert_eq!(child.transform.position, [0.125, 0.125]);
    }
    // A child-local transform outside the canvas is still unsafe. Keep a clean
    // sibling but never fall back to the clipping canvas-sized nest.
    value["composition"]["layers"][0]["layers"][0]["transform"]["position"] =
        serde_json::json!([1, 0]);
    let mut sibling = value["composition"]["layers"][0]["layers"][1].clone();
    sibling["id"] = serde_json::json!(10000);
    sibling.as_object_mut().unwrap().remove("parent");
    sibling["activeRange"] = serde_json::json!({"start":0,"duration":2400});
    sibling["transform"] = value["composition"]["layers"][0]["transform"].clone();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(sibling);
    commit_edits(
        &mut archive,
        &value,
        &dir.path().join("unsupported-child.json"),
    );
    let destination = dir.path().join("bounded-native");
    let report = crate::premiere_package::save_tesseract_as_premiere(
        &output,
        &destination,
        crate::schema::FrameRate::Fps25,
        false,
    )
    .unwrap();
    assert!(format!("{report:?}").contains("rather than clipping pixels"));
    let (native, _) =
        crate::format::PrProjectFile::load_selected(&destination.join("project.prproj"), None)
            .unwrap();
    let (sequences, media) = native.into_parts();
    assert_eq!(sequences[0].nest_occurrences().count(), 0);
    assert_eq!(
        sequences[0]
            .video_occurrences()
            .filter(|clip| media[&clip.media].is_still())
            .count(),
        1
    );
}

#[test]
fn numbered_images_active_native_effects_omit_only_unsafe_placement() {
    for name in ["PR.ADBE Black &amp; White", "AE.Impact_Stroke_FX"] {
        for bypass in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let project = numbered_project(dir.path());
            trim_project(&project);
            add_repeated_placement(&project, false);
            let xml = fs::read_to_string(&project).unwrap().replace(
                "<VideoComponentChain ObjectID=\"14\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/>",
                "<VideoComponentChain ObjectID=\"14\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"20\"/></Components></ComponentChain>")
                .replace("</PremiereData>",&format!("<VideoFilterComponent ObjectID=\"20\"><Component><ID>3</ID><DisplayName>Review effect</DisplayName><Bypass>{bypass}</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>{name}</MatchName><VideoFilterType>1</VideoFilterType></VideoFilterComponent></PremiereData>"));
            fs::write(&project, xml).unwrap();
            let (native, omissions) =
                crate::format::PrProjectFile::load_selected(&project, None).unwrap();
            let (sequences, _) = native.into_parts();
            assert_eq!(
                sequences[0].video_occurrences().count(),
                if bypass { 2 } else { 1 },
                "{name}, bypass={bypass}: {omissions:?}"
            );
            assert_eq!(
                sequences[0].video_occurrences().next().unwrap().start_ticks,
                ticks(400)
            );
            if !bypass {
                assert!(
                    omissions
                        .iter()
                        .any(|o| o.to_string().contains("numbered-image")
                            && o.to_string().contains("active standard effects")),
                    "{omissions:?}"
                );
            }
        }
    }
}

#[test]
fn numbered_images_direct_stills_preserve_group_key_clock_and_visibility() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let dir = tempfile::tempdir().unwrap();
    let project = numbered_project(dir.path());
    trim_project(&project);
    let output = dir.path().join("keyed.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    let (native, _) = crate::format::PrProjectFile::load_selected(&project, None).unwrap();
    let (mut sequences, media) = native.into_parts();
    let clip = sequences[0].video_tracks[0].clip_mut(0);
    clip.enabled = false;
    clip.opacity = 25.0;
    clip.animations = vec![PrPropertyAnimation::Opacity(vec![
        PrScalarKeyframe {
            source_ticks: ticks(80),
            value: 25.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: ticks(280),
            value: 75.0,
            easing: PrKeyframeEasing::Linear,
        },
    ])];
    let value = crate::tests::support::project_document_with_media(&sequences[0], &media);
    let mut archive = TesseractFile::open(&output).unwrap();
    commit_edits(&mut archive, &value, &dir.path().join("keys.json"));
    let destination = dir.path().join("native");
    crate::premiere_package::save_tesseract_as_premiere(
        &output,
        &destination,
        crate::schema::FrameRate::Fps25,
        false,
    )
    .unwrap();
    let (native, _) =
        crate::format::PrProjectFile::load_selected(&destination.join("project.prproj"), None)
            .unwrap();
    let (sequences, media) = native.into_parts();
    let mut clips: Vec<_> = sequences[0]
        .video_occurrences()
        .filter(|clip| media[&clip.media].is_still())
        .collect();
    clips.sort_by_key(|clip| clip.start_ticks);
    assert_eq!(clips.len(), 10);
    let generator = crate::schema::FrameRate::Fps25.generator_in_ticks();
    for (index, clip) in clips.into_iter().enumerate() {
        assert!(!clip.enabled);
        assert_eq!(clip.opacity, 25.0);
        assert_eq!(clip.start_ticks, ticks(400 + index as i64 * 40));
        let PrPropertyAnimation::Opacity(keys) = &clip.animations[0] else {
            panic!("expected opacity keys")
        };
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].source_ticks, generator - ticks(index as i64 * 40));
        assert_eq!(
            keys[1].source_ticks,
            generator + ticks(200 - index as i64 * 40)
        );
        assert_eq!((keys[0].value, keys[1].value), (25.0, 75.0));
    }
}
