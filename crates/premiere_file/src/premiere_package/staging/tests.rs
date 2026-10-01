//! Supplementary package/typed-graph checks, not generated-project Adobe proof.

use std::{fs, io::Read, path::PathBuf};

use serde_json::{json, Value};
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::*;
use crate::premiere_package::{inspect_audio, inspect_media};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn input_value() -> Value {
    let mut value = crate::test_support::editable_document();
    let layers = value["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": layers[0]["playback"]["inputRange"]["start"], "duration": 500}),
        json!({"start": layers[0]["sourceRange"]["start"], "duration": 500}),
    );
    layers[0]["sourceRange"]["duration"] = json!(500);
    let mut second = layers[0].clone();
    second["id"] = json!(3);
    second["playback"]["inputRange"]["start"] = json!(500);
    second["playback"]["mapping"]["input"]["start"] = json!(500);
    second["sourceRange"]["start"] = json!(500);
    second["playback"]["mapping"]["output"]["start"] = json!(500);
    layers.insert(0, second);
    layers.insert(
        0,
        json!({
            "type":"Audio", "id":4, "name":"Independent music",
            "playback": crate::test_support::linear_playback(json!({"start":100,"duration":200}), json!({"start":0,"duration":200})),
            "sourceRange":{"start":0,"duration":200}, "sourceIntrinsicDuration":200,
            "volume":0.5, "source":{"assetId":"music"}
        }),
    );
    value
}

fn builder(value: &Value) -> TesseractFileBuilder {
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
}

fn overlay() -> AfterEffectsOverlay {
    AfterEffectsOverlay {
        composition_guid: "00000001-0000-0000-0000-000000000000".into(),
        dimensions: [1920, 1080],
        frame_rate: FrameRate::Fps30,
        intrinsic_duration_secs: 1.0,
        timeline_secs: 0.1..0.9,
    }
}

fn xml(staged: &StagedPremiereExport) -> String {
    let bytes = fs::read(staged.directory().join("project.prproj")).unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[test]
fn staged_link_preserves_native_cuts_audio_and_final_paths() {
    let root = tempfile::tempdir().unwrap();
    let archive = builder(&input_value())
        .write(root.path().join("input.tsrct"))
        .unwrap();
    let output = root.path().join("final-package");
    let staged = Premiere
        .stage_with_after_effects_overlay(
            &archive,
            archive.project(),
            root.path(),
            &output,
            &Default::default(),
            &overlay(),
        )
        .unwrap();
    assert!(!output.exists());
    let xml = xml(&staged);
    assert!(!xml.contains(staged.directory().to_str().unwrap()));
    assert!(xml.contains(output.join(AEP_PATH).to_str().unwrap()));
    let (project, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let mut native_ranges = Vec::new();
    let mut linked = None;
    for occurrence in sequence.video_occurrences() {
        let source = project.media(occurrence).unwrap();
        if let Some(identity) = source.after_effects_composition() {
            assert_eq!(identity.dynamic_link_guid(), overlay().composition_guid);
            linked = Some(occurrence);
        } else {
            native_ranges.push((occurrence.timeline_ticks(), occurrence.source_ticks()));
        }
    }
    native_ranges.sort_by_key(|ranges| ranges.0.start);
    let half = TICKS_PER_SECOND / 2;
    assert_eq!(
        native_ranges,
        vec![(0..half, 0..half), (half..2 * half, half..2 * half)]
    );
    let linked = linked.unwrap();
    assert_eq!(
        linked.timeline_ticks(),
        TICKS_PER_SECOND / 10..9 * TICKS_PER_SECOND / 10
    );
    assert_eq!(linked.source_ticks(), linked.timeline_ticks());
    let top = sequence
        .video_tracks
        .last()
        .unwrap()
        .items
        .last()
        .unwrap()
        .media()
        .unwrap();
    assert!(project
        .media(top)
        .unwrap()
        .after_effects_composition()
        .is_some());
    assert_eq!(sequence.audio.len(), 1);
    let sound = &sequence.audio[0];
    assert_eq!(sound.start_ticks, TICKS_PER_SECOND / 10);
    assert_eq!(sound.end_ticks, 3 * TICKS_PER_SECOND / 10);
    assert_eq!(sound.in_ticks, 0);
    assert_eq!(sound.out_ticks, TICKS_PER_SECOND / 5);
    assert!((sound.volume.as_f64() - 0.5).abs() < 1e-6);
    assert_eq!(staged.after_effects_path(), Path::new(AEP_PATH));
    assert_eq!(staged.report().artifacts.len(), 3);
    assert!(!staged.directory().join(AEP_PATH).exists());
    for artifact in &staged.report().artifacts {
        assert!(staged.directory().join(&artifact.path).is_file());
        assert!(!artifact.path.is_absolute());
    }
    for name in ["video-30fps.mp4", "audio-mono.wav"] {
        assert_eq!(
            fs::read(staged.directory().join("media").join(name)).unwrap(),
            fs::read(fixture(name)).unwrap()
        );
    }
    let private = staged.directory().to_owned();
    drop(staged);
    assert!(!private.exists());
    assert!(root.path().join("input.tsrct").exists());
}

#[test]
fn selected_document_alone_controls_native_video_and_audio_inspection() {
    let root = tempfile::tempdir().unwrap();
    let selected = input_value();
    let mut full = selected.clone();
    let layers = full["composition"]["layers"].as_array_mut().unwrap();
    let mut bad_audio = layers[0].clone();
    bad_audio["id"] = json!(90);
    bad_audio["source"]["assetId"] = json!("bad-audio");
    let mut bad_video = layers[1].clone();
    bad_video["id"] = json!(91);
    bad_video["source"]["assetId"] = json!("bad-video");
    layers.extend([bad_audio, bad_video]);
    let audio = root.path().join("broken.wav");
    let video = root.path().join("broken.mp4");
    fs::write(&audio, b"not wave").unwrap();
    fs::write(&video, b"not mp4").unwrap();
    let archive = builder(&full)
        .add_asset("bad-audio", &audio, AssetKind::Audio)
        .unwrap()
        .add_asset("bad-video", &video, AssetKind::Video)
        .unwrap()
        .write(root.path().join("input.tsrct"))
        .unwrap();
    assert!(inspect_media(&archive, archive.project()).is_err());
    let full_audio = inspect_audio(&archive, archive.project(), &Default::default()).unwrap();
    assert!(matches!(
        full_audio.get("bad-audio"),
        Some(crate::audio_media::SourceSound::Unsupported(_))
    ));
    let document = EditableFxCompositionDocument::from_json_value(selected).unwrap();
    let selected_audio = inspect_audio(&archive, &document, &Default::default()).unwrap();
    assert!(!selected_audio.contains_key("bad-audio"));
    let staged = Premiere
        .stage_with_after_effects_overlay(
            &archive,
            &document,
            root.path(),
            &root.path().join("final"),
            &Default::default(),
            &overlay(),
        )
        .unwrap();
    assert_eq!(staged.report().artifacts.len(), 3);
}

#[test]
fn invalid_links_clean_staging_without_touching_unrelated_files() {
    let root = tempfile::tempdir().unwrap();
    let archive = builder(&input_value())
        .write(root.path().join("input.tsrct"))
        .unwrap();
    fs::write(root.path().join("keep.txt"), b"keep").unwrap();
    let mut invalid = Vec::new();
    for guid in ["", "1", "00000000-0000-0000-0000-000000000000"] {
        let mut link = overlay();
        link.composition_guid = guid.into();
        invalid.push(link);
    }
    let mut link = overlay();
    link.dimensions = [320, 180];
    invalid.push(link);
    let mut link = overlay();
    link.frame_rate = FrameRate::Fps25;
    invalid.push(link);
    for range in [
        -0.1..0.9,
        0.9..0.1,
        0.0..0.0,
        0.0..1.1,
        0.001..0.9,
        f64::NAN..0.9,
    ] {
        let mut link = overlay();
        link.timeline_secs = range;
        invalid.push(link);
    }
    for duration in [0.5, f64::INFINITY, f64::MAX] {
        let mut link = overlay();
        link.intrinsic_duration_secs = duration;
        invalid.push(link);
    }
    for link in invalid {
        assert!(Premiere
            .stage_with_after_effects_overlay(
                &archive,
                archive.project(),
                root.path(),
                &root.path().join("final"),
                &Default::default(),
                &link,
            )
            .is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
    assert_eq!(fs::read(root.path().join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn a_custom_canvas_stages_its_matching_overlay_and_rejects_another_size() {
    let root = tempfile::tempdir().unwrap();
    let mut value = input_value();
    value["dimensions"] = json!({"width": 1080, "height": 1920});
    let canvas = value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    assert_eq!(canvas["name"], "Black canvas");
    canvas["rect"]["size"] = json!([1080, 1920]);
    let archive = builder(&value)
        .write(root.path().join("input.tsrct"))
        .unwrap();
    let portrait = AfterEffectsOverlay {
        dimensions: [1080, 1920],
        ..overlay()
    };
    let staged = Premiere
        .stage_with_after_effects_overlay(
            &archive,
            archive.project(),
            root.path(),
            &root.path().join("final-package"),
            &Default::default(),
            &portrait,
        )
        .unwrap();
    let (project, omissions) =
        crate::format::inspect_project_with_omissions(&xml(&staged), None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.dimensions(), [1080, 1920]);
    let linked = sequence
        .video_occurrences()
        .find_map(|clip| {
            let media = project.media(clip).unwrap();
            media.after_effects_composition().and(media.video.as_ref())
        })
        .unwrap();
    assert_eq!([linked.width, linked.height], [1080, 1920]);
    drop(staged);
    // The 1920x1080 overlay does not align with the portrait sequence.
    assert!(Premiere
        .stage_with_after_effects_overlay(
            &archive,
            archive.project(),
            root.path(),
            &root.path().join("final"),
            &Default::default(),
            &overlay(),
        )
        .is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn staging_rejects_existing_final_output_and_missing_staging_parent() {
    let root = tempfile::tempdir().unwrap();
    let archive = builder(&input_value())
        .write(root.path().join("input.tsrct"))
        .unwrap();
    let output = root.path().join("final");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("sentinel"), b"untouched").unwrap();
    assert!(Premiere
        .stage_with_after_effects_overlay(
            &archive,
            archive.project(),
            root.path(),
            &output,
            &Default::default(),
            &overlay(),
        )
        .is_err());
    assert!(Premiere
        .stage_with_after_effects_overlay(
            &archive,
            archive.project(),
            &root.path().join("absent"),
            &root.path().join("fresh"),
            &Default::default(),
            &overlay(),
        )
        .is_err());
    assert!(!root.path().join("absent").exists());
    assert!(!root.path().join("fresh").exists());
    assert_eq!(fs::read(output.join("sentinel")).unwrap(), b"untouched");
}
