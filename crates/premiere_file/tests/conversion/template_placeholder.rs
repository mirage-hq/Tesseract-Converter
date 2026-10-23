#![cfg(feature = "ffmpeg-library")]

//! The Premiere 26.5.2-authored template fixture
//! `template_placeholder_nest.prproj` (`template_fixtures.provenance.json`):
//! sequence "Template Placeholder Nest" holds, on V1, the chart still under
//! Lumetri Exposure +0.5 (0–2 s) and, on V2, a placement of "Media _
//! Placeholder", a nested sequence whose clips were removed (a template drop
//! zone), with that nest's linked audio item on A1 (0–4.97 s). The native
//! project is the oracle for the saved structure; the reimport of our export
//! is a supplementary round trip only.

use super::support::*;
use premiere_file::OmissionScope;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tesseract_file::TesseractFile;

const PROJECT: &str = "template_placeholder_nest.prproj";
const PROJECT_SHA256: &str = "32015e4ec047699992ba00a569f97edd9e819fdc05f5b15ac16fe6ee4299e9a8";
const SEQUENCE: &str = "55542fdc-3560-47b5-9a93-54eef4f5d673";
const PLACEHOLDER_RECORD: &str = "Sequence:5abbc670-bbf9-4f16-b1cf-fc4022e248b2";
const PLACEHOLDER_AUDIO_RECORD: &str = "AudioClipTrackItem:80";

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Every layer of `layers` and its descendants, depth first.
fn all_layers<'a>(layers: &'a Value, found: &mut Vec<&'a Value>) {
    for layer in layers.as_array().into_iter().flatten() {
        found.push(layer);
        all_layers(&layer["layers"], found);
    }
}

/// The one report on `record` whose reason starts with `prefix`.
fn report<'a>(
    omissions: &'a [premiere_file::Omission],
    record: &str,
    prefix: &str,
) -> &'a premiere_file::Omission {
    let reports: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.record == record && omission.reason.starts_with(prefix))
        .collect();
    let [report] = reports[..] else {
        panic!("{record}: {omissions:?}");
    };
    report
}

/// The imported placeholder group, shown for `shown_ms`, its graded chart
/// and the document.
fn assert_imported(omissions: &[premiere_file::Omission], output: &Path, shown_ms: u64) -> Value {
    let placeholder = report(
        omissions,
        PLACEHOLDER_RECORD,
        "empty template placeholder sequence \"Media _ Placeholder\" has no source occurrences; Premiere renders it as nothing",
    );
    assert_eq!(placeholder.scope, OmissionScope::Feature);
    assert!(
        !omissions.iter().any(|omission| omission
            .reason
            .contains("not converted: ClipTrack/Track/")
            || omission.reason.contains("no convertible video")),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(output))
        .unwrap()
        .project_json()
        .unwrap();
    let mut layers = Vec::new();
    all_layers(&document["composition"]["layers"], &mut layers);
    let groups: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Group" && layer["name"] == "Media _ Placeholder")
        .collect();
    let [group] = groups[..] else {
        panic!("{document}");
    };
    // Nothing visible: the group has no picture or sound below it.
    let mut inside = Vec::new();
    all_layers(&group["layers"], &mut inside);
    assert!(
        inside.iter().all(|layer| layer["type"] == "Group"),
        "{group}"
    );
    assert_eq!(group["playback"]["inputRange"]["start"], 0, "{group}");
    assert_eq!(
        group["playback"]["inputRange"]["duration"], shown_ms,
        "{group}"
    );
    let graded: Vec<_> = layers
        .iter()
        .filter(|layer| {
            layer["effects"].as_array().is_some_and(|effects| {
                effects
                    .iter()
                    .any(|effect| effect["effect"]["type"] == "primaryGrade")
            })
        })
        .collect();
    let [chart] = graded[..] else {
        panic!("{document}");
    };
    assert_eq!(chart["type"], "Image", "{chart}");
    let exposure = chart["effects"][0]["effect"]["exposure"].as_f64().unwrap();
    // Lumetri Exposure +0.5 stops over the fitted 2.6 scale.
    assert!((exposure - 0.5 / 2.6).abs() < 1e-9, "{chart}");
    document
}

#[test]
fn template_placeholder_fixture_matches_its_pinned_sha256() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(fs::read(fixture_path(PROJECT)).unwrap())
        ),
        PROJECT_SHA256
    );
    let provenance: Value = serde_json::from_slice(
        &fs::read(fixture_path("template_fixtures.provenance.json")).unwrap(),
    )
    .unwrap();
    let recorded: Vec<_> = provenance["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            case["project"]["repo_path"]
                .as_str()
                .is_some_and(|path| path.ends_with(&format!("/{PROJECT}")))
        })
        .map(|case| {
            (
                case["project"]["sha256"].as_str().unwrap(),
                case["sequence_uid"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(recorded, [(PROJECT_SHA256, SEQUENCE)]);
}

#[test]
fn native_empty_placeholder_nest_imports_as_an_empty_group_beside_the_graded_chart() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions =
        premiere_to_tesseract(fixture_path(PROJECT), &output, Some(SEQUENCE), false).unwrap();
    let audio = report(
        &omissions,
        PLACEHOLDER_AUDIO_RECORD,
        "empty template placeholder sequence \"Media _ Placeholder\" has no source occurrences; its audio item plays nothing and is dropped",
    );
    assert_eq!(audio.scope, OmissionScope::Feature);
    // 60 frames at 29.97 fps.
    let document = assert_imported(&omissions, &output, 2002);
    let mut layers = Vec::new();
    all_layers(&document["composition"]["layers"], &mut layers);
    assert!(
        layers.iter().all(|layer| layer["type"] != "Audio"),
        "{document}"
    );
    // The sequence end that the dropped audio item set is kept: Premiere
    // plays black after the picture ends.
    assert_eq!(document["duration"], 4.972, "{document}");
}

#[test]
fn template_placeholder_export_writes_an_empty_nested_sequence_that_reimports_as_the_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let imported = dir.path().join("converted");
    premiere_to_tesseract(fixture_path(PROJECT), &imported, Some(SEQUENCE), false).unwrap();
    let package = dir.path().join("native");
    let omissions = tesseract_to_premiere(first_project(&imported), &package, false).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.contains("no exportable video")),
        "{omissions:?}"
    );
    let xml = read_xml(&package.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let names: Vec<_> = parsed
        .descendants()
        .filter(|node| node.has_tag_name("Sequence") && node.attribute("ObjectUID").is_some())
        .filter_map(|node| {
            node.children()
                .find(|child| child.has_tag_name("Name"))
                .and_then(|name| name.text())
        })
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"Media _ Placeholder"), "{names:?}");

    let root = exported_root_sequence(&package.join("project.prproj"));
    let reimported = dir.path().join("reimported");
    let omissions = premiere_to_tesseract(
        package.join("project.prproj"),
        &reimported,
        Some(&root),
        false,
    )
    .unwrap();
    let placeholders: Vec<_> = omissions
        .iter()
        .filter(|omission| {
            omission.reason.starts_with(
                "empty template placeholder sequence \"Media _ Placeholder\" has no source occurrences",
            )
        })
        .collect();
    assert_eq!(placeholders.len(), 1, "{omissions:?}");
    let mut assert_reimport = omissions.clone();
    for omission in &mut assert_reimport {
        if omission
            .reason
            .starts_with("empty template placeholder sequence")
        {
            omission.record = PLACEHOLDER_RECORD.to_owned();
        }
    }
    // The export runs at the default 30 fps: 60 frames.
    assert_imported(&assert_reimport, &reimported, 2000);
}

const LOCKS_PROJECT: &str = "template_track_locks.prproj";
const LOCKS_SHA256: &str = "317f960d950a9d4fb72564ab3b1943013d5aa3f06580bad2cbd5bbc10ee66759";
const LOCKS_SEQUENCE: &str = "cfc74e38-7b60-44de-a0bb-e38d152a1494";

/// `template_track_locks.prproj` saves V1 with `IsSyncLocked false` and V2
/// with `IsLocked true`, the only lock elements Premiere 26.5.2 writes.
#[test]
fn native_track_locks_import_without_reports_and_export_premiere_defaults() {
    use sha2::{Digest, Sha256};
    let source = fixture_path(LOCKS_PROJECT);
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        LOCKS_SHA256
    );
    let saved = read_xml(&source);
    assert_eq!(
        saved.matches("<IsSyncLocked>false</IsSyncLocked>").count(),
        1
    );
    assert_eq!(saved.matches("<IsLocked>true</IsLocked>").count(), 1);
    let dir = tempfile::tempdir().unwrap();
    let imported = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &imported, Some(LOCKS_SEQUENCE), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = TesseractFile::open(first_project(&imported))
        .unwrap()
        .project_json()
        .unwrap();
    let mut layers = Vec::new();
    all_layers(&document["composition"]["layers"], &mut layers);
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Image")
            .count(),
        2,
        "{document}"
    );

    let package = dir.path().join("native");
    tesseract_to_premiere(first_project(&imported), &package, false).unwrap();
    let written = read_xml(&package.join("project.prproj"));
    assert!(!written.contains("<IsSyncLocked>"));
    assert!(!written.contains("<IsLocked>"));
}
