use crate::test_support::write_prproj;
use crate::{
    MediaRelink, MediaRelinkBinding, Premiere, PremiereImportOptions, ValidatedMediaRelink,
};
use fx_conv::{ConversionMode, MediaMapSource};
#[cfg(feature = "ffmpeg-library")]
use std::io::Read;
use std::{
    fs,
    path::{Path, PathBuf},
};
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;

const UID: &str = "5f3a1c9e-7b2d-4e6f-9a80-c1d2e3f40501";
const TARGET: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const AUTHORED: &str = r"\\?\E:\collected\portrait.png";
const PNG: &[u8] = include_bytes!("../../tests/fixtures/feature_still_transparent.png");

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, String) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("source.prproj");
    let local = directory.path().join("explicit.png");
    let xml = crate::tests::support::prproj_xml(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/premiere_isolated_still_image.prproj"),
    );
    let xml = xml.replace("<RelativePath>feature_still_transparent.png</RelativePath>", &format!(r"<FilePath>{AUTHORED}</FilePath><ActualMediaFilePath>E:\old\portrait.png</ActualMediaFilePath>"));
    assert!(xml.contains(AUTHORED));
    write_prproj(&project, &xml);
    fs::write(&local, PNG).unwrap();
    for name in ["feature_still_opaque.jpg", "video-30fps-10s.mp4"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
            directory.path().join(name),
        )
        .unwrap();
    }
    (directory, project, local, xml)
}

fn manifest(project: &Path, local: &Path) -> MediaRelink {
    MediaRelink {
        version: 1,
        source: MediaMapSource {
            format: "premiere".into(),
            sha256: crate::hash::hash(project).unwrap(),
            target: TARGET.into(),
        },
        bindings: vec![MediaRelinkBinding {
            media_uid: UID.into(),
            authored_path: AUTHORED.into(),
            local_path: local.to_owned(),
            sha256: crate::hash::hash(local).unwrap(),
        }],
    }
}

fn options() -> PremiereImportOptions {
    PremiereImportOptions {
        sequence: Some(TARGET.into()),
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn media_relink_native_still_keeps_editable_image_and_exact_alpha_bytes() {
    // Only path fields of the pinned native still fixture change. Its authored
    // clips, transforms, timing and original PNG are not synthesized here.
    let (directory, project, local, _) = fixture();
    let original_hash = crate::hash::hash(&project).unwrap();
    let default_output = directory.path().join("default");
    let default_report = <Premiere as fx_conv::ImportToTesseract>::import_to_tesseract(
        &Premiere,
        &project,
        &default_output,
        &options(),
        ConversionMode::Check,
    )
    .unwrap();
    assert!(default_report
        .diagnostics
        .iter()
        .any(|omission| omission.reason.contains(UID)));
    assert!(!default_output.exists());
    let relink = ValidatedMediaRelink::new(manifest(&project, &local)).unwrap();
    let inspection = Premiere
        .inspect_media_with_relink(&project, &options(), &relink)
        .unwrap();
    // Time-based inspection does not list stills; editable packaging below
    // proves the picture. Its native record must no longer be unassessed.
    assert!(
        !inspection
            .unassessed
            .iter()
            .any(|reason| reason.contains(UID)),
        "{inspection:?}"
    );
    let check = directory.path().join("check");
    Premiere
        .import_with_media_relink(&project, &check, &options(), ConversionMode::Check, &relink)
        .unwrap();
    assert!(!check.exists());
    let output = directory.path().join("write");
    let report = Premiere
        .import_with_media_relink(
            &project,
            &output,
            &options(),
            ConversionMode::Write,
            &relink,
        )
        .unwrap();
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|omission| omission.reason.contains(UID)),
        "{:?}",
        report.diagnostics
    );
    let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
    let mut matched = None;
    for id in file.metadata().assets.keys() {
        let mut bytes = Vec::new();
        file.asset(id)
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        if bytes == PNG {
            matched = Some(id.clone());
        }
    }
    let id = matched.expect("the original alpha PNG must be packaged unchanged");
    let document = file.project_json().unwrap();
    fn has_image(value: &serde_json::Value, id: &str) -> bool {
        match value {
            serde_json::Value::Object(object) => {
                (value["type"] == "Image" && value["source"]["assetId"] == id)
                    || object.values().any(|child| has_image(child, id))
            }
            serde_json::Value::Array(array) => array.iter().any(|child| has_image(child, id)),
            _ => false,
        }
    }
    assert!(has_image(&document, &id), "{document}");
    assert_eq!(crate::hash::hash(&project).unwrap(), original_hash);
}

#[test]
fn media_relink_rejects_wrong_source_target_uid_path_and_duplicates_before_output() {
    let (directory, project, local, _) = fixture();
    for case in 0..5 {
        let mut map = manifest(&project, &local);
        match case {
            0 => map.source.sha256 = "0".repeat(64),
            1 => map.source.target = "other-sequence".into(),
            2 => map.bindings[0].media_uid = "absent-media".into(),
            3 => map.bindings[0].authored_path = r"E:\other\portrait.png".into(),
            _ => map.bindings.push(map.bindings[0].clone()),
        }
        let output = directory.path().join(format!("rejected-{case}"));
        match ValidatedMediaRelink::new(map) {
            Ok(relink) => assert!(Premiere
                .import_with_media_relink(
                    &project,
                    &output,
                    &options(),
                    ConversionMode::Write,
                    &relink
                )
                .is_err()),
            Err(_) => assert_eq!(case, 4),
        }
        assert!(!output.exists());
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn media_relink_rejects_changed_files_at_check_and_publication() {
    let (directory, project, local, _) = fixture();
    let relink = ValidatedMediaRelink::new(manifest(&project, &local)).unwrap();
    let output = directory.path().join("changed");
    let pending = crate::tesseract_import::TesseractImport::convert_with_media_relink(
        &project,
        &output,
        Some(TARGET),
        &relink,
        fx_conv::Progress::default(),
    )
    .unwrap();
    fs::write(&local, b"changed file").unwrap();
    assert!(pending.write().is_err());
    assert!(!output.exists());
    assert!(Premiere
        .import_with_media_relink(
            &project,
            &output,
            &options(),
            ConversionMode::Check,
            &relink
        )
        .is_err());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn media_relink_preserves_relative_candidate_conflicts_and_malformed_alias_rejection() {
    let (directory, project, local, xml) = fixture();
    for (name, edited) in [
        ("conflict", xml.replace(&format!("<FilePath>{AUTHORED}</FilePath>"), &format!("<RelativePath>conflict.png</RelativePath><FilePath>{AUTHORED}</FilePath>"))),
        ("malformed", xml.replace(r"E:\old\portrait.png", "relative.png")),
        ("drive-hint", xml.replace(&format!("<FilePath>{AUTHORED}</FilePath>"), &format!(r"<RelativePath>..\E:\old\portrait.png</RelativePath><FilePath>{AUTHORED}</FilePath>"))),
    ] {
        fs::write(directory.path().join("conflict.png"), b"different bytes").unwrap();
        write_prproj(&project, &edited);
        let relink = ValidatedMediaRelink::new(manifest(&project, &local)).unwrap();
        let output = directory.path().join(name);
        assert!(Premiere.import_with_media_relink(&project, &output, &options(), ConversionMode::Write, &relink).is_err());
        assert!(!output.exists());
    }
}

#[cfg(unix)]
#[cfg(feature = "ffmpeg-library")]
#[test]
fn media_relink_rejects_retargeted_local_symlink_and_package_hint_escape() {
    use std::os::unix::fs::symlink;
    let (directory, project, local, xml) = fixture();
    let alias = directory.path().join("alias.png");
    symlink(&local, &alias).unwrap();
    let relink = ValidatedMediaRelink::new(manifest(&project, &alias)).unwrap();
    let other = directory.path().join("other.png");
    fs::write(&other, PNG).unwrap();
    fs::remove_file(&alias).unwrap();
    symlink(&other, &alias).unwrap();
    let output = directory.path().join("retargeted");
    assert!(Premiere
        .import_with_media_relink(
            &project,
            &output,
            &options(),
            ConversionMode::Check,
            &relink
        )
        .is_err());
    assert!(!output.exists());

    let outside = tempfile::tempdir().unwrap();
    let image = outside.path().join("image.png");
    fs::write(&image, PNG).unwrap();
    symlink(&image, directory.path().join("escaped.png")).unwrap();
    write_prproj(
        &project,
        &xml.replace(
            &format!("<FilePath>{AUTHORED}</FilePath>"),
            &format!("<RelativePath>escaped.png</RelativePath><FilePath>{AUTHORED}</FilePath>"),
        ),
    );
    let relink = ValidatedMediaRelink::new(manifest(&project, &image)).unwrap();
    let output = directory.path().join("escape");
    let error = Premiere
        .import_with_media_relink(
            &project,
            &output,
            &options(),
            ConversionMode::Write,
            &relink,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("escapes the source package"),
        "{error}"
    );
    assert!(!output.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn media_relink_keeps_video_dimension_and_invalid_media_admission() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("video.prproj");
    let local = directory.path().join("video.mp4");
    let source = include_str!("../../tests/fixtures/one-clip.xml")
        .replace("1270080000000", "254016000000")
        .replace("2540160000000", "254016000000")
        .replace(
            "<RelativePath>media/source.mp4</RelativePath>",
            &format!("<FilePath>{AUTHORED}</FilePath>"),
        );
    for invalid in [false, true] {
        let xml = source.replace("<VideoStream ObjectID=\"8\"><Duration>254016000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect>", "<VideoStream ObjectID=\"8\"><Duration>254016000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1280,720</FrameRect>");
        write_prproj(&project, &xml);
        fs::write(
            &local,
            if invalid {
                b"not video"
            } else {
                include_bytes!("../../tests/fixtures/video-30fps.mp4").as_slice()
            },
        )
        .unwrap();
        let mut map = manifest(&project, &local);
        map.source.target = "sequence-1".into();
        map.bindings[0].media_uid = "media-1".into();
        let relink = ValidatedMediaRelink::new(map).unwrap();
        let output = directory
            .path()
            .join(if invalid { "invalid" } else { "dimensions" });
        let error = Premiere
            .import_with_media_relink(
                &project,
                &output,
                &PremiereImportOptions {
                    sequence: Some("sequence-1".into()),
                },
                ConversionMode::Write,
                &relink,
            )
            .unwrap_err();
        assert!(error.to_string().contains("failed admission"), "{error}");
        assert!(!output.exists());
    }
}
