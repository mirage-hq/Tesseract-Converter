#![cfg(feature = "ffmpeg-library")]

//! H.264 and HEVC media through both public conversion directions.
//!
//! `feature_video_formats_strict.prproj` places an H.264 MP4 (0-2 s, source
//! 2-4 s) and an HEVC Main MP4 (2-4 s) back to back on V1. It is derived from
//! the Adobe-authored adjacent-cut fixture: its HEVC record is an edited copy
//! of an H.264 record, with only CodecType, name, path, and Duration changed
//! (`OriginalColorSpace` and `AlphaType` are unchanged). Its pinned AME render
//! verifies import playback of that derived record, not a Premiere-authored
//! HEVC record or a reopen of generated Premiere output.
//!
//! `feature_hdr_passthrough_strict.prproj` is that project with the HEVC media
//! replaced by `feature_hdr_hlg_hvc1.mov`, a 320x180 VideoToolbox Main 10 HLG
//! QuickTime file with a timecode track: name, path and the stream's FrameRect
//! changed, and its `CodecType` and `OriginalColorSpace` set to the text that
//! Premiere 26.5.1 saved for 10-bit HLG `hvc1` sources.

mod avcc;

use super::support::*;
use fx_conv::{
    sha256_file, ConversionMode, MediaMap, MediaMapSource, MediaReplacement, MediaStatus,
    ValidatedMediaMap,
};
use premiere_file::{
    Omission, OmissionKind, OmissionScope, PrProjectFile, Premiere, PremiereImportOptions,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const PROJECT: &str = "feature_video_formats_strict.prproj";
const H264: &str = "video-30fps-10s.mp4";
const HEVC: &str = "feature_video_formats_hevc.mp4";
/// A one-second 1080p30 H.264 MOV that stands in for a generated Eye Contact output.
const EYE_CONTACT_OUTPUT: &str = "video-30fps.mov";
/// `VideoStream.CodecType` of the two media, in timeline order: the values
/// Premiere 24.3-26.5.1 store for `avc1` media and Premiere 26.5.1 for HEVC
/// masters.
const CODEC_TYPES: [&str; 2] = ["1635148593", "1212503619"];
/// The `OriginalColorSpace` that Premiere 26.5.1 saved for the 10-bit HLG
/// sources of that fixture, verbatim.
const HLG_PROFILE: &str = r#"{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"BT.2100 HLG,10-bit,Display-Referred"},"baseProfileType":1}"#;
const HDR_PROJECT: &str = "feature_hdr_passthrough_strict.prproj";
const HLG: &str = "feature_hdr_hlg_hvc1.mov";
/// The HLG media record of the HDR project, as import identifies its warning.
const HLG_MEDIA: &str =
    "Media:ObjectUID:93318ede-9c1e-4e0a-94ea-da42c59425c6 (\"feature_hdr_hlg_hvc1.mov\")";
const HLG_WARNING: &str =
    "video colour BT.2020/HLG/BT.2020nc passes through unchanged; display depends on the player";

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Copies the project and its two media files into `root`.
fn stage(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    for name in [PROJECT, H264, HEVC] {
        fs::copy(fixture_path(name), root.join(name)).unwrap();
    }
    root.join(PROJECT)
}

/// Converts one timeline; exported projects carry a writer-generated sequence UID.
fn convert(project: &Path, sequence: Option<&str>, output: &Path) -> TesseractFile {
    let omissions = premiere_to_tesseract(project, output, sequence, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let projects = project_files(output);
    assert_eq!(projects.len(), 1);
    TesseractFile::open(&projects[0]).unwrap()
}

fn asset_name(file: &TesseractFile, asset_id: &str) -> String {
    Path::new(&file.metadata().assets[asset_id].path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// (asset file name, activeRange, sourceRange) of each video layer in stack order.
fn video_layers(file: &TesseractFile) -> Vec<(String, Value, Value)> {
    let document = file.project_json().unwrap();
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|layer| {
            (
                asset_name(file, layer["source"]["assetId"].as_str().unwrap()),
                (*crate::test_support::layer_range(layer)).clone(),
                layer["sourceRange"].clone(),
            )
        })
        .collect()
}

fn expected_layers() -> Vec<(String, Value, Value)> {
    [
        (H264, (0, 2000), (2000, 2000)),
        (HEVC, (2000, 2000), (0, 2000)),
    ]
    .into_iter()
    .map(
        |(name, (start, duration), (source_start, source_duration))| {
            (
                name.to_owned(),
                json!({"start": start, "duration": duration}),
                json!({"start": source_start, "duration": source_duration}),
            )
        },
    )
    .collect()
}

/// `(media name, timeline ticks, source ticks)` of every occurrence.
fn occurrences(project: &PrProjectFile) -> Vec<(String, [i64; 2], [i64; 2])> {
    let sequence = project.sequences().next().unwrap();
    sequence
        .video_occurrences()
        .map(|clip| {
            let (timeline, source) = (clip.timeline_ticks(), clip.source_ticks());
            (
                project.media(clip).unwrap().name().to_owned(),
                [timeline.start, timeline.end],
                [source.start, source.end],
            )
        })
        .collect()
}

fn codec_types(xml: &str) -> Vec<String> {
    roxmltree::Document::parse(xml)
        .unwrap()
        .descendants()
        .filter(|node| node.has_tag_name("CodecType"))
        .map(|node| node.text().unwrap().to_owned())
        .collect()
}

#[test]
fn adobe_derived_video_formats_import_each_codec_as_an_editable_layer() {
    let dir = tempfile::tempdir().unwrap();
    let file = convert(
        &fixture_path(PROJECT),
        Some(SEQUENCE),
        &dir.path().join("converted"),
    );
    assert_eq!(video_layers(&file), expected_layers());
    for (asset_id, descriptor) in &file.metadata().assets {
        let name = asset_name(&file, asset_id);
        assert_eq!(descriptor.content_type, "video/mp4", "{name}");
        // Media is packaged without transcoding.
        let mut packaged = Vec::new();
        std::io::Read::read_to_end(
            &mut file.asset(asset_id).unwrap().open().unwrap(),
            &mut packaged,
        )
        .unwrap();
        assert_eq!(packaged, fs::read(fixture_path(&name)).unwrap(), "{name}");
    }
}

#[test]
fn authored_video_formats_export_with_each_codec_type_and_unchanged_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Author the reverse input independently, so a matching import mistake
    // cannot hide a codec or source-range error on export.
    let mut document = crate::test_support::editable_document();
    document["duration"] = json!(4);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 0, "duration": 2000}),
        json!({"start": 2000, "duration": 2000}),
    );
    layers[0]["sourceRange"] = json!({"start": 2000, "duration": 2000});
    layers[0]["sourceIntrinsicDuration"] = json!(10000);
    let mut hevc = layers[0].clone();
    hevc["id"] = json!(2);
    hevc["source"]["assetId"] = json!("hevc-source");
    hevc["playback"] = crate::test_support::linear_playback(
        json!({"start": 2000, "duration": 2000}),
        json!({"start": 0, "duration": 2000}),
    );
    hevc["sourceRange"] = json!({"start": 0, "duration": 2000});
    hevc["sourceIntrinsicDuration"] = json!(2000);
    layers[1]["id"] = json!(3);
    layers[1]["activeRange"]["duration"] = json!(4000);
    layers.insert(1, hevc);
    let archive = root.join("authored.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", fixture_path(H264), AssetKind::Video)
        .unwrap()
        .add_asset("hevc-source", fixture_path(HEVC), AssetKind::Video)
        .unwrap()
        .write(&archive)
        .unwrap();
    let omissions = tesseract_to_premiere(&archive, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = root.join("native/project.prproj");
    assert_eq!(codec_types(&read_xml(&exported)), CODEC_TYPES);
    let original = PrProjectFile::load(fixture_path(PROJECT)).unwrap().0;
    let (rebuilt, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrences(&rebuilt), occurrences(&original));
    for name in [H264, HEVC] {
        assert_eq!(
            fs::read(root.join("native/media").join(name)).unwrap(),
            fs::read(fixture_path(name)).unwrap(),
            "{name}"
        );
    }
    // The exported package converts again to the same editable layers.
    let again = convert(&exported, None, &root.join("again"));
    assert_eq!(video_layers(&again), expected_layers());
}

/// Imports the HDR project's timeline, whose one warning is the HLG record's.
fn convert_hdr(root: &Path) -> TesseractFile {
    let omissions = premiere_to_tesseract(
        fixture_path(HDR_PROJECT),
        root.join("converted"),
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Approximated,
            record: HLG_MEDIA.to_owned(),
            reason: HLG_WARNING.to_owned(),
        }]
    );
    TesseractFile::open(first_project(&root.join("converted"))).unwrap()
}

#[test]
fn hlg_hevc_passes_through_with_one_warning_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let warning = |record: &str| Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Approximated,
        record: record.to_owned(),
        reason: HLG_WARNING.to_owned(),
    };
    let file = convert_hdr(root);
    let mut expected = expected_layers();
    expected[1].0 = HLG.to_owned();
    assert_eq!(video_layers(&file), expected);
    let hlg = file
        .metadata()
        .assets
        .iter()
        .find(|(id, _)| asset_name(&file, id) == HLG)
        .map(|(id, descriptor)| (id.clone(), descriptor.content_type.clone()))
        .unwrap();
    assert_eq!(hlg.1, "video/quicktime");
    let mut packaged = Vec::new();
    std::io::Read::read_to_end(
        &mut file.asset(&hlg.0).unwrap().open().unwrap(),
        &mut packaged,
    )
    .unwrap();
    assert_eq!(packaged, fs::read(fixture_path(HLG)).unwrap());

    let omissions = tesseract_to_premiere(
        first_project(&root.join("converted")),
        root.join("native"),
        false,
    )
    .unwrap();
    assert_eq!(omissions, [warning(&format!("asset {:?}", hlg.0))]);
    let exported = root.join("native/project.prproj");
    let xml = read_xml(&exported);
    assert_eq!(codec_types(&xml), CODEC_TYPES);
    // The H.264 source keeps the BT.709 profile; the HLG source gets Premiere's.
    assert_eq!(
        xml.matches("<OriginalColorSpace>").count(),
        2,
        "one profile per media"
    );
    assert_eq!(
        xml.matches(&format!(
            "<OriginalColorSpace>{HLG_PROFILE}</OriginalColorSpace>"
        ))
        .count(),
        1
    );
    assert_eq!(
        fs::read(root.join("native/media").join(HLG)).unwrap(),
        fs::read(fixture_path(HLG)).unwrap()
    );
    let original = PrProjectFile::load(fixture_path(HDR_PROJECT)).unwrap().0;
    let (rebuilt, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrences(&rebuilt), occurrences(&original));
    let omissions = premiere_to_tesseract(&exported, root.join("again"), None, false).unwrap();
    // The writer assigns its own media identity; the readable name stays.
    let [again_warning] = &omissions[..] else {
        panic!("{omissions:?}");
    };
    assert_eq!(again_warning.reason, HLG_WARNING);
    assert!(
        again_warning.record.starts_with("Media:")
            && again_warning.record.ends_with(&format!(" ({HLG:?})")),
        "{again_warning:?}"
    );
    let again = TesseractFile::open(first_project(&root.join("again"))).unwrap();
    assert_eq!(video_layers(&again), expected);
}

#[test]
fn relocated_video_formats_package_resolves_media_by_its_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    let moved = dir.path().join("moved");
    let project = stage(&moved);
    // Stale absolute aliases from the pre-move location must not win or block.
    let xml = read_xml(&project);
    let mut relocated = xml.clone();
    for name in [H264, HEVC] {
        relocated = relocated.replace(
            &format!("<RelativePath>{name}</RelativePath>"),
            &format!(
                "<RelativePath>{name}</RelativePath><FilePath>/Volumes/Missing/original/{name}</FilePath>"
            ),
        );
    }
    assert_eq!(relocated.matches("/Volumes/Missing/original/").count(), 2);
    fs::remove_file(&project).unwrap();
    write_prproj(&project, &relocated);
    let file = convert(&project, Some(SEQUENCE), &dir.path().join("converted"));
    assert_eq!(video_layers(&file), expected_layers());
}

#[test]
fn repeated_and_separate_new_codec_media_keep_their_identities() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = convert(
        &fixture_path(PROJECT),
        Some(SEQUENCE),
        &root.join("converted"),
    );
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    // A second HEVC placement at 4-5 s reads source 0.5-1.5 s of the same asset.
    let mut repeated = layers[1].clone();
    repeated["id"] = json!(10);
    repeated["playback"] = crate::test_support::linear_playback(
        json!({"start": 4000, "duration": 1000}),
        json!({"start": 500, "duration": 1000}),
    );
    repeated["sourceRange"] = json!({"start": 500, "duration": 1000});
    // A separate HEVC asset with identical bytes and file name at 5-6 s.
    let mut separate = layers[1].clone();
    separate["id"] = json!(11);
    separate["source"]["assetId"] = json!("separate-hevc");
    separate["playback"] = crate::test_support::linear_playback(
        json!({"start": 5000, "duration": 1000}),
        json!({"start": 0, "duration": 1000}),
    );
    separate["sourceRange"] = json!({"start": 0, "duration": 1000});
    layers.insert(2, repeated);
    layers.insert(3, separate);
    layers[4]["activeRange"]["duration"] = json!(6000);
    document["duration"] = json!(6);
    let copy = root.join("copy");
    fs::create_dir(&copy).unwrap();
    fs::copy(fixture_path(HEVC), copy.join(HEVC)).unwrap();
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (asset_id, path) in [
        ("premiere-video-1", fixture_path(H264)),
        ("premiere-video-2", fixture_path(HEVC)),
        ("separate-hevc", copy.join(HEVC)),
    ] {
        builder = builder.add_asset(asset_id, path, AssetKind::Video).unwrap();
    }
    let edited = root.join("edited.tsrct");
    builder.write(&edited).unwrap();
    tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    let (native, _) = PrProjectFile::load(root.join("native/project.prproj")).unwrap();
    let names: Vec<_> = occurrences(&native)
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    // One HEVC media serves both placements; the separate identity stays apart
    // under a unique name.
    assert_eq!(names[1], names[2]);
    assert_ne!(names[1], names[3]);
    assert!(names[3].ends_with(".mp4"), "{names:?}");
    assert_eq!(fs::read_dir(root.join("native/media")).unwrap().count(), 3);
    let again = convert(
        &root.join("native/project.prproj"),
        None,
        &root.join("again"),
    );
    assert_eq!(again.metadata().assets.len(), 3);
    let reimported: Vec<_> = again.project_json().unwrap()["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|layer| layer["source"]["assetId"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(reimported[1], reimported[2]);
    assert_ne!(reimported[1], reimported[3]);
}

#[test]
fn m4v_media_rejects_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("package");
    let project = stage(&root);
    fs::rename(root.join(H264), root.join("video-30fps-10s.m4v")).unwrap();
    let xml = read_xml(&project).replace(
        &format!("<RelativePath>{H264}</RelativePath>"),
        "<RelativePath>video-30fps-10s.m4v</RelativePath>",
    );
    fs::remove_file(&project).unwrap();
    write_prproj(&project, &xml);
    for check in [true, false] {
        let output = dir.path().join(format!("converted-{check}"));
        let error = premiere_to_tesseract(&project, &output, Some(SEQUENCE), check)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("video file extension \"m4v\" is unsupported"),
            "{error}"
        );
        assert!(!output.exists());
    }

    let export_root = dir.path().join("export");
    fs::create_dir(&export_root).unwrap();
    let doc = document(&export_root);
    let media = export_root.join("source.m4v");
    fs::write(&media, MEDIA).unwrap();
    let edited = archive(&export_root, &doc, &media);
    let error = tesseract_to_premiere(&edited, export_root.join("native"), true)
        .unwrap_err()
        .to_string();
    assert!(
        error.ends_with(
            "video file extension \"m4v\" is unsupported; conversion accepts MP4 or QuickTime MOV video"
        ),
        "{error}"
    );
}

/// A one-clip archive whose layer records an Eye Contact replacement.
fn eye_contact_archive(root: &Path, enabled: bool) -> PathBuf {
    let original = document(root);
    let mut document = original;
    document["composition"]["layers"][0]["source"]["eyeContact"] =
        json!({"enabled": enabled, "eyeContactAssetId": "eye-contact-output"});
    let path = root.join("eye-contact.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "eye-contact-output",
            fixture_path(EYE_CONTACT_OUTPUT),
            AssetKind::Video,
        )
        .unwrap()
        .write(&path)
        .unwrap();
    path
}

#[test]
fn enabled_eye_contact_exports_only_the_active_replacement_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let archive = eye_contact_archive(root, true);
    let omissions = tesseract_to_premiere(&archive, root.join("native"), false).unwrap();
    assert!(
        omissions.iter().any(|item| item
            .reason
            .contains("Eye Contact exported as its active output")),
        "{omissions:?}"
    );
    let media: Vec<_> = fs::read_dir(root.join("native/media"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(media.len(), 1);
    assert_eq!(
        fs::read(&media[0]).unwrap(),
        fs::read(fixture_path(EYE_CONTACT_OUTPUT)).unwrap()
    );
    let exported = root.join("native/project.prproj");
    assert_eq!(codec_types(&read_xml(&exported)), [CODEC_TYPES[0]]);
    let (native, _) = PrProjectFile::load(&exported).unwrap();
    assert_eq!(
        occurrences(&native),
        [(EYE_CONTACT_OUTPUT.to_owned(), [0, TICKS], [0, TICKS])]
    );
}

#[test]
fn eye_contact_keeps_picture_when_original_sound_is_malformed() {
    for replacement in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut document = document(root);
        document["duration"] = json!(0.2);
        document["composition"]["layers"][1]["activeRange"]["duration"] = json!(200);
        let layer = &mut document["composition"]["layers"][0];
        layer["volume"] = json!(0.5);
        layer["playback"] = crate::test_support::linear_playback(
            json!({"start": 0, "duration": 200}),
            json!({"start": 0, "duration": 200}),
        );
        layer["sourceRange"]["duration"] = json!(200);
        layer["sourceIntrinsicDuration"] = json!(if replacement { 1000 } else { 200 });
        if replacement {
            layer["source"]["eyeContact"] =
                json!({"enabled": true, "eyeContactAssetId": "eye-contact-output"});
            // Eye Contact changes picture only; malformed original sound is diagnosed.
            fs::write(root.join("source.mp4"), b"original audio is malformed").unwrap();
        } else {
            fs::copy(
                fixture_path("video-with-audio.mp4"),
                root.join("source.mp4"),
            )
            .unwrap();
        }
        let archive = root.join("input.tsrct");
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset(
                "premiere-video-1",
                root.join("source.mp4"),
                AssetKind::Video,
            )
            .unwrap()
            .add_asset(
                "eye-contact-output",
                fixture_path(EYE_CONTACT_OUTPUT),
                AssetKind::Video,
            )
            .unwrap()
            .write(&archive)
            .unwrap();
        let omissions = tesseract_to_premiere(&archive, root.join("native"), false).unwrap();
        let xml = read_xml(&root.join("native/project.prproj"));
        assert_eq!(xml.matches("<VideoClipTrackItem ").count(), 1);
        assert_eq!(
            xml.matches("<AudioClipTrackItem ").count(),
            usize::from(!replacement)
        );
        let media: Vec<_> = fs::read_dir(root.join("native/media"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(media.len(), 1);
        assert_eq!(
            fs::read(&media[0]).unwrap(),
            fs::read(if replacement {
                fixture_path(EYE_CONTACT_OUTPUT)
            } else {
                root.join("source.mp4")
            })
            .unwrap()
        );
        if replacement {
            // Export plans a root clip's sound before its picture, so the two
            // reports are compared whole, in either order.
            let omission = |reason: &str| Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".to_owned(),
                reason: reason.to_owned(),
            };
            assert_eq!(omissions.len(), 2, "{omissions:?}");
            for expected in [
                omission("embedded audio was not exported: unsupported conversion: MP4 metadata box exceeds its parent"),
                omission("Eye Contact exported as its active output \"eye-contact-output\"; the original picture lineage \"premiere-video-1\" and the Eye Contact toggle were not exported"),
            ] {
                assert!(omissions.contains(&expected), "{omissions:?}");
            }
        } else {
            assert!(omissions.is_empty(), "{omissions:?}");
        }
    }
}

#[test]
fn disabled_eye_contact_exports_the_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let archive = eye_contact_archive(root, false);
    let omissions = tesseract_to_premiere(&archive, root.join("native"), false).unwrap();
    assert!(
        omissions.iter().any(|item| item.reason
            == "inactive Eye Contact output \"eye-contact-output\" was not exported"),
        "{omissions:?}"
    );
    let media: Vec<_> = fs::read_dir(root.join("native/media"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(media.len(), 1);
    assert_eq!(fs::read(&media[0]).unwrap(), MEDIA);
}

#[test]
fn subtitle_tracks_reject_in_both_directions() {
    // The HLG fixture's timecode track is ignored; the same track under a
    // QuickTime text handler would carry visible captions that conversion
    // drops, so the file rejects by its handler.
    let mut captioned = fs::read(fixture_path(HLG)).unwrap();
    let handler = captioned
        .windows(4)
        .enumerate()
        .filter(|(_, kind)| *kind == b"hdlr")
        .map(|(at, _)| at + 12)
        .find(|&at| &captioned[at..at + 4] == b"tmcd")
        .unwrap();
    captioned[handler..handler + 4].copy_from_slice(b"text");
    let expected = "subtitle or caption tracks (text) are unsupported";

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("package");
    fs::create_dir_all(&root).unwrap();
    for name in [HDR_PROJECT, H264] {
        fs::copy(fixture_path(name), root.join(name)).unwrap();
    }
    fs::write(root.join(HLG), &captioned).unwrap();
    for check in [true, false] {
        let output = dir.path().join(format!("converted-{check}"));
        let error = premiere_to_tesseract(root.join(HDR_PROJECT), &output, Some(SEQUENCE), check)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!output.exists());
    }

    let export_root = dir.path().join("export");
    fs::create_dir(&export_root).unwrap();
    let doc = document(&export_root);
    let source = export_root.join("captioned.mov");
    fs::write(&source, &captioned).unwrap();
    let archive = archive(&export_root, &doc, &source);
    let error = tesseract_to_premiere(&archive, export_root.join("native"), true)
        .unwrap_err()
        .to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn unsupported_codecs_are_named_in_both_directions() {
    // VP9 bytes under the HEVC file name.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("package");
    let project = stage(&root);
    fs::copy(fixture_path("video-vp9-64x64.mp4"), root.join(HEVC)).unwrap();
    for check in [true, false] {
        let output = dir.path().join(format!("converted-{check}"));
        let omissions = premiere_to_tesseract(&project, &output, Some(SEQUENCE), check).unwrap();
        assert!(
            omissions
                .iter()
                .any(|note| note.record == "VideoClipTrackItem:153"
                    && note.reason.contains("video codec \"vp09\" is unsupported")),
            "{omissions:?}"
        );
        if check {
            assert!(!output.exists());
        } else {
            let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
            assert_eq!(video_layers(&file), expected_layers()[..1]);
            assert_eq!(file.metadata().assets.len(), 1);
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let export_root = dir.path().join("export");
    fs::create_dir(&export_root).unwrap();
    let doc = document(&export_root);
    let archive = archive(&export_root, &doc, &fixture_path("video-vp9-64x64.mp4"));
    let error = tesseract_to_premiere(&archive, export_root.join("native"), true)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("video codec \"vp09\" is unsupported"),
        "{error}"
    );
}

#[test]
fn dolby_vision_entries_omit_source_and_preserve_supported_siblings() {
    // Supplementary sample-entry mutation of existing HEVC media, not a new
    // Adobe-native Dolby Vision fixture or proof of decoding/fidelity.
    for entry in [b"dvh1", b"dvhe"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("package");
        let project = stage(&root);
        let mut bytes = fs::read(root.join(HEVC)).unwrap();
        let offsets: Vec<_> = bytes
            .windows(4)
            .enumerate()
            .filter_map(|(offset, tag)| (tag == b"hvc1").then_some(offset))
            .collect();
        assert!(!offsets.is_empty());
        for offset in offsets {
            bytes[offset..offset + 4].copy_from_slice(entry);
        }
        fs::write(root.join(HEVC), bytes).unwrap();
        let expected = format!(
            "Dolby Vision {} sample entries are not decodable",
            String::from_utf8_lossy(entry)
        );
        for check in [true, false] {
            let output = directory.path().join(format!("converted-{check}"));
            let omissions =
                premiere_to_tesseract(&project, &output, Some(SEQUENCE), check).unwrap();
            assert!(
                omissions
                    .iter()
                    .any(|note| note.record == "VideoClipTrackItem:153"
                        && note.reason.contains(&expected)),
                "{omissions:?}"
            );
            if check {
                assert!(!output.exists());
            } else {
                let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
                assert_eq!(video_layers(&file), expected_layers()[..1]);
                assert_eq!(file.metadata().assets.len(), 1);
                let asset = file.metadata().assets.keys().next().unwrap();
                let healthy = fs::read(root.join(H264)).unwrap();
                assert_eq!(
                    file.asset(asset)
                        .unwrap()
                        .read_verified_bytes(healthy.len() as u64)
                        .unwrap(),
                    healthy
                );
            }
        }
    }
}

#[test]
fn native_media_of_an_omitted_placement_only_exempts_unavailable_video() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let project = stage(root);
    let mut xml = read_xml(&project);
    // Native occurrence 145 uses SubClip 143 -> VideoClip 142, not its master 22.
    let insertion = roxmltree::Document::parse(&xml)
        .unwrap()
        .descendants()
        .find(|node| node.has_tag_name("VideoClip") && node.attribute("ObjectID") == Some("142"))
        .unwrap()
        .range()
        .end
        - "</VideoClip>".len();
    xml.insert_str(insertion, "<ScaleToFramePolicy>1</ScaleToFramePolicy>");
    write_prproj(&project, &xml);
    let options = PremiereImportOptions {
        sequence: Some(SEQUENCE.into()),
    };
    let inspection = Premiere.inspect_media(&project, &options, None).unwrap();
    assert!(inspection.is_ready(), "{inspection:?}");
    assert_eq!(inspection.media.len(), 2);
    let omissions =
        premiere_to_tesseract(&project, root.join("valid"), Some(SEQUENCE), false).unwrap();
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("Scale to Frame Size")),
        "{omissions:?}"
    );
    fs::copy(fixture_path("video-vp9-64x64.mp4"), root.join(H264)).unwrap();
    let inspection = Premiere.inspect_media(&project, &options, None).unwrap();
    assert!(!inspection.is_ready());
    assert!(inspection
        .media
        .iter()
        .any(|media| media.status == MediaStatus::RequiresTranscode));
    for check in [true, false] {
        let output = root.join(format!("unsupported-{check}"));
        premiere_to_tesseract(&project, &output, Some(SEQUENCE), check).unwrap();
        if check {
            assert!(!output.exists());
        } else {
            let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
            assert_eq!(video_layers(&file), expected_layers()[1..]);
            assert_eq!(file.metadata().assets.len(), 1);
        }
    }
    // Even an occurrence already omitted by the reader cannot bypass native
    // path/container validation. Malformed present media is not a codec omission.
    fs::write(root.join(H264), b"invalid movie").unwrap();
    for check in [true, false] {
        let output = root.join(format!("malformed-{check}"));
        let error = premiere_to_tesseract(&project, &output, Some(SEQUENCE), check).unwrap_err();
        assert!(error.to_string().contains("failed admission"), "{error}");
        assert!(!output.exists());
    }
}

#[test]
fn prepared_media_map_admits_a_bound_replacement_and_rejects_stale_or_wrong_scope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("package");
    let project = stage(&root);
    let original = root.join(HEVC);
    fs::copy(fixture_path("video-vp9-64x64.mp4"), &original).unwrap();
    let replacement = root.join("prepared.mp4");
    fs::copy(fixture_path(HEVC), &replacement).unwrap();

    let ordinary_output = dir.path().join("ordinary");
    let ordinary = premiere_to_tesseract(&project, &ordinary_output, Some(SEQUENCE), true).unwrap();
    assert!(
        ordinary
            .iter()
            .any(|note| note.reason.contains("video codec \"vp09\" is unsupported")),
        "{ordinary:?}"
    );
    assert!(!ordinary_output.exists());

    let map_path = root.join("media-map.json");
    let map_dto = |source_sha256: String, target: &str| MediaMap {
        version: 1,
        source: MediaMapSource {
            format: "premiere".into(),
            sha256: source_sha256,
            target: target.into(),
        },
        replacements: vec![MediaReplacement {
            original: original.canonicalize().unwrap(),
            original_sha256: sha256_file(&original).unwrap(),
            replacement: PathBuf::from("prepared.mp4"),
            replacement_sha256: sha256_file(&replacement).unwrap(),
        }],
    };
    let load = |map: &MediaMap| {
        fs::write(&map_path, serde_json::to_vec(map).unwrap()).unwrap();
        ValidatedMediaMap::load(&map_path).unwrap()
    };
    let source_hash = sha256_file(&project).unwrap();
    let map = load(&map_dto(source_hash.clone(), SEQUENCE));
    let options = PremiereImportOptions {
        sequence: Some(SEQUENCE.into()),
    };
    let ordinary_preflight = Premiere.inspect_media(&project, &options, None).unwrap();
    let blocked = ordinary_preflight
        .media
        .iter()
        .find(|media| media.original.as_deref() == Some(original.canonicalize().unwrap().as_path()))
        .unwrap();
    assert_eq!(blocked.status, MediaStatus::RequiresTranscode);
    assert_eq!(blocked.selected, blocked.original);

    let mapped_preflight = Premiere
        .inspect_media(&project, &options, Some(&map))
        .unwrap();
    let prepared = mapped_preflight
        .media
        .iter()
        .find(|media| media.original.as_deref() == Some(original.canonicalize().unwrap().as_path()))
        .unwrap();
    assert_eq!(prepared.status, MediaStatus::Supported);
    assert_eq!(
        prepared.selected.as_deref(),
        Some(replacement.canonicalize().unwrap().as_path())
    );
    assert!(mapped_preflight.is_ready());

    let check_output = dir.path().join("mapped-check");
    let report = Premiere
        .import_with_media_map(
            &project,
            &check_output,
            &options,
            ConversionMode::Check,
            &map,
        )
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert!(!check_output.exists());

    let write_output = dir.path().join("mapped-write");
    let report = Premiere
        .import_with_media_map(
            &project,
            &write_output,
            &options,
            ConversionMode::Write,
            &map,
        )
        .unwrap();
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let file = TesseractFile::open(first_project(&write_output)).unwrap();
    let prepared_id = file
        .metadata()
        .assets
        .iter()
        .find_map(|(id, _)| (asset_name(&file, id) == "prepared.mp4").then_some(id))
        .unwrap();
    let mut packaged = Vec::new();
    std::io::Read::read_to_end(
        &mut file.asset(prepared_id).unwrap().open().unwrap(),
        &mut packaged,
    )
    .unwrap();
    assert_eq!(packaged, fs::read(&replacement).unwrap());

    let stale = load(&map_dto("0".repeat(64), SEQUENCE));
    let error = Premiere
        .import_with_media_map(
            &project,
            &dir.path().join("stale"),
            &options,
            ConversionMode::Check,
            &stale,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("file changed"), "{error}");

    let wrong_scope = load(&map_dto(source_hash, "another-sequence"));
    let error = Premiere
        .import_with_media_map(
            &project,
            &dir.path().join("wrong-scope"),
            &options,
            ConversionMode::Check,
            &wrong_scope,
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("source format or selected target differs"),
        "{error}"
    );
}

#[test]
fn same_named_hdr_media_and_repeated_placements_each_warn_once_per_record() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let warning = |record: String| Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Approximated,
        record,
        reason: HLG_WARNING.to_owned(),
    };
    let file = convert_hdr(root);
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    // A second HLG placement at 4-5 s of the same asset, and a separate asset
    // with identical bytes and file name at 5-6 s.
    let mut repeated = layers[1].clone();
    repeated["id"] = json!(10);
    repeated["playback"] = crate::test_support::linear_playback(
        json!({"start": 4000, "duration": 1000}),
        json!({"start": 500, "duration": 1000}),
    );
    repeated["sourceRange"] = json!({"start": 500, "duration": 1000});
    let mut separate = layers[1].clone();
    separate["id"] = json!(11);
    separate["source"]["assetId"] = json!("separate-hlg");
    separate["playback"] = crate::test_support::linear_playback(
        json!({"start": 5000, "duration": 1000}),
        json!({"start": 0, "duration": 1000}),
    );
    separate["sourceRange"] = json!({"start": 0, "duration": 1000});
    layers.insert(2, repeated);
    layers.insert(3, separate);
    layers[4]["activeRange"]["duration"] = json!(6000);
    document["duration"] = json!(6);
    let copy = root.join("copy");
    fs::create_dir(&copy).unwrap();
    fs::copy(fixture_path(HLG), copy.join(HLG)).unwrap();
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap()).unwrap();
    for (asset_id, path) in [
        ("premiere-video-1", fixture_path(H264)),
        ("premiere-video-2", fixture_path(HLG)),
        ("separate-hlg", copy.join(HLG)),
    ] {
        builder = builder.add_asset(asset_id, path, AssetKind::Video).unwrap();
    }
    let edited = root.join("edited.tsrct");
    builder.write(&edited).unwrap();
    // Export warns once per asset: the repeated placement adds nothing.
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert_eq!(
        omissions,
        [
            warning("asset \"separate-hlg\"".to_owned()),
            warning("asset \"premiere-video-2\"".to_owned()),
        ]
    );
    // The writer gave the separate file a unique name; a Premiere project may
    // instead hold two same-named files in different folders. Import must
    // then warn once per media record, not once per name.
    let exported = root.join("native/project.prproj");
    let xml = read_xml(&exported);
    let unique = "premiere-media-001.mov";
    assert!(xml.contains(unique), "{xml}");
    // Name elements hold the bare name; path elements get the folder.
    let same_name = xml
        .replace(&format!(">{unique}<"), &format!(">{HLG}<"))
        .replace(unique, &format!("copy/{HLG}"));
    fs::create_dir(root.join("native/media/copy")).unwrap();
    fs::rename(
        root.join("native/media").join(unique),
        root.join("native/media/copy").join(HLG),
    )
    .unwrap();
    write_prproj(&exported, &same_name);
    let omissions = premiere_to_tesseract(&exported, root.join("again"), None, false).unwrap();
    let records: Vec<_> = omissions.iter().map(|omission| &omission.record).collect();
    assert_eq!(omissions.len(), 2, "{omissions:?}");
    assert_ne!(records[0], records[1]);
    for omission in &omissions {
        assert_eq!(omission.reason, HLG_WARNING);
        assert!(
            omission.record.starts_with("Media:")
                && omission.record.ends_with(&format!(" ({HLG:?})")),
            "{omission:?}"
        );
    }
    let again = TesseractFile::open(first_project(&root.join("again"))).unwrap();
    assert_eq!(again.metadata().assets.len(), 3);
}
