#[cfg(feature = "ffmpeg-library")]
use crate::format::inspect_project;
#[cfg(feature = "ffmpeg-library")]
use crate::test_support::editable_document as editable_video_document;
#[cfg(feature = "ffmpeg-library")]
use crate::test_support::write_archive;
use crate::test_support::write_prproj;
#[cfg(feature = "ffmpeg-library")]
use crate::tests::support::inspect;
#[cfg(feature = "ffmpeg-library")]
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};
const SOURCE: &str = include_str!("../../tests/fixtures/one-clip.xml");
const MEDIA: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");

fn one_second_xml(relative_paths: &str) -> String {
    SOURCE
        .replace("1270080000000", "254016000000")
        .replace("2540160000000", "254016000000")
        .replace(
            "<RelativePath>media/source.mp4</RelativePath>",
            relative_paths,
        )
}

fn h264_bytes() -> Vec<u8> {
    MEDIA.to_vec()
}

#[test]
fn distinct_timeline_and_source_ranges_survive_tesseract_reopen() {
    let xml = SOURCE
        .replace(
            "<End>1270080000000</End>",
            "<Start>254016000000</Start><End>1016064000000</End>",
        )
        .replace("<InPoint>0</InPoint>", "<InPoint>508032000000</InPoint>");
    let dir = tempfile::tempdir().unwrap();
    let asset = dir.path().join("source.mp4");
    std::fs::write(&asset, b"opaque original bytes: no transcoding").unwrap();
    let output = dir.path().join("result.tsrct");
    let parsed = crate::format::inspect_project_with_media(&xml, None).unwrap();
    TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&crate::tests::support::project_document_with_media(
            parsed.single_sequence().unwrap(),
            &parsed.media,
        ))
        .unwrap(),
    )
    .unwrap()
    .add_asset("premiere-video-1", &asset, AssetKind::Video)
    .unwrap()
    .write(&output)
    .unwrap();
    std::fs::remove_file(asset).unwrap();
    let reopened = TesseractFile::open(output).unwrap();
    let doc = reopened.project_json().unwrap();
    let layer = &doc["composition"]["layers"][0];
    assert_eq!((*crate::test_support::layer_range(layer))["start"], 1000.0);
    assert_eq!(
        (*crate::test_support::layer_range(layer))["duration"],
        3000.0
    );
    assert_eq!(layer["sourceRange"]["start"], 2000.0);
    assert_eq!(layer["sourceRange"]["duration"], 3000.0);
    assert_eq!(doc["duration"], 4.0);
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut reopened.asset("premiere-video-1").unwrap().open().unwrap(),
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, b"opaque original bytes: no transcoding");
}

fn native_media_fixture(relative_paths: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    let media = package.join("media");
    let external = directory.path().join("external");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    let project = package.join("project.prproj");
    write_prproj(&project, &one_second_xml(relative_paths));
    (
        directory,
        project,
        media.join("source.mp4"),
        external.join("source.mp4"),
    )
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn media_dimension_mismatch_rejects_conversion_despite_a_healthy_sibling() {
    let (directory, project, packaged, _) =
        native_media_fixture("<RelativePath>media/source.mp4</RelativePath>");
    fs::write(&packaged, MEDIA).unwrap();
    let xml = one_second_xml("<RelativePath>media/source.mp4</RelativePath>");
    // Two independent Media records share bytes. Only the first declares the
    // wrong size; the healthy sibling must not permit partial publication.
    let document = roxmltree::Document::parse(&xml).unwrap();
    let sibling = document
        .root_element()
        .children()
        .filter(|node| {
            node.is_element()
                && (node
                    .attribute("ObjectID")
                    .is_some_and(|id| id.parse::<u32>().unwrap() >= 3)
                    || node.has_tag_name("Media"))
        })
        .map(|node| &xml[node.range()])
        .collect::<Vec<_>>()
        .join("");
    let sibling = (3..=8)
        .fold(sibling, |xml, id| {
            xml.replace(
                &format!("ObjectID=\"{id}\""),
                &format!("ObjectID=\"{}\"", id + 10),
            )
            .replace(
                &format!("ObjectRef=\"{id}\""),
                &format!("ObjectRef=\"{}\"", id + 10),
            )
        })
        .replace("media-1", "media-2")
        .replace(
            "<TrackItem><End>254016000000</End>",
            "<TrackItem><Start>254016000000</Start><End>508032000000</End>",
        );
    let xml = xml
        .replace(
            "<TrackItem ObjectRef=\"3\"/>",
            "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"13\"/>",
        )
        .replace("</PremiereData>", &format!("{sibling}</PremiereData>"));
    let stream = "<VideoStream ObjectID=\"8\"><Duration>254016000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect>";
    assert!(xml.contains(stream));
    write_prproj(
        &project,
        &xml.replace(stream, &stream.replace("1920,1080", "1280,720")),
    );
    let output = directory.path().join("size-isolation");
    for check in [true, false] {
        let error = crate::premiere_to_tesseract(&project, &output, None, check)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("video media Media:ObjectUID:media-1")
                && error.contains("failed admission")
                && error
                    .contains("source dimensions 1920x1080 differ from native dimensions 1280x720"),
            "{error}"
        );
        assert!(!output.exists());
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn absolute_only_media_requires_consistent_live_aliases() {
    for conflict in [false, true] {
        let (directory, project, packaged, external) = native_media_fixture("");
        fs::write(&packaged, MEDIA).unwrap();
        let mut other = MEDIA.to_vec();
        if conflict {
            *other.last_mut().unwrap() ^= 1;
        }
        fs::write(&external, other).unwrap();
        write_prproj(
            &project,
            &one_second_xml(&format!(
                "<FilePath>{}</FilePath><ActualMediaFilePath>{}</ActualMediaFilePath>",
                packaged.display(),
                external.display()
            )),
        );
        let output = directory.path().join("absolute-only");
        let result = crate::premiere_to_tesseract(&project, &output, None, false);
        if conflict {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("identify different bytes"));
            assert!(!output.exists());
        } else {
            assert!(result.unwrap().is_empty());
            let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
            assert_eq!(
                file.project_json().unwrap()["composition"]["layers"][0]["type"],
                "Video"
            );
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(
                &mut file.asset("premiere-video-1").unwrap().open().unwrap(),
                &mut bytes,
            )
            .unwrap();
            assert_eq!(bytes, MEDIA);
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unsupported_native_sound_keeps_adobe_derived_picture() {
    use std::io::Read;
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(
        fs::File::open(fixtures.join("feature_linked_av_strict.prproj")).unwrap(),
    )
    .read_to_string(&mut xml)
    .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let stream = document
        .descendants()
        .find(|node| node.has_tag_name("AudioStream") && node.attribute("ObjectID") == Some("111"))
        .unwrap();
    let stream_xml = &xml[stream.range()];
    for (from, to, reason) in [
        (
            "[{\"channellabel\":100},{\"channellabel\":101}]",
            "[{\"channellabel\":2}]",
            "only ordinary mono/stereo",
        ),
        (
            "<FrameRate>5292000</FrameRate>",
            "<FrameRate>11</FrameRate>",
            "unsupported sample rate",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project.prproj");
        fs::copy(
            fixtures.join("feature_linked_av_source.mp4"),
            directory.path().join("feature_linked_av_source.mp4"),
        )
        .unwrap();
        assert!(stream_xml.contains(from));
        write_prproj(
            &project,
            &xml.replace(stream_xml, &stream_xml.replace(from, to)),
        );
        let output = directory.path().join("picture");
        let omissions = crate::premiere_to_tesseract(
            &project,
            &output,
            Some("80acdd81-0a96-4677-b17f-b2ffe2dff738"),
            false,
        )
        .unwrap();
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == crate::OmissionScope::Feature
                    && item.reason.contains(reason)),
            "{omissions:?}"
        );
        let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
        let document = file.project_json().unwrap();
        let layers = document["composition"]["layers"].as_array().unwrap();
        assert!(layers.iter().all(|layer| layer["type"] != "Audio"));
        let pictures = layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .collect::<Vec<_>>();
        assert_eq!(pictures.len(), 1);
        assert_eq!(pictures[0]["volume"].as_f64(), Some(0.0));
        let whole = json!({"start": 0, "duration": 5000});
        assert_eq!((*crate::test_support::layer_range(pictures[0])), whole);
        assert_eq!(pictures[0]["sourceRange"], whole);
        assert_eq!(file.metadata().assets.len(), 1);
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == crate::OmissionScope::Occurrence
                    && item.reason.contains("source has no audio stream")),
            "{omissions:?}"
        );
        let mut bytes = Vec::new();
        file.asset(pictures[0]["source"]["assetId"].as_str().unwrap())
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(
            bytes,
            fs::read(fixtures.join("feature_linked_av_source.mp4")).unwrap()
        );
    }
}

#[test]
fn absent_relative_path_does_not_admit_missing_or_invalid_aliases() {
    for (paths, reason) in [
        ("", "missing RelativePath and absolute media aliases"),
        ("<FilePath/>", "FilePath must be a nonempty absolute path"),
        (
            "<FilePath>source.mp4</FilePath>",
            "FilePath must be a nonempty absolute path",
        ),
        (
            WINDOWS_ALIASES,
            "media saved on Windows has no package-local RelativePath",
        ),
    ] {
        let (directory, project, _, _) = native_media_fixture(paths);
        let output = directory.path().join("invalid-alias");
        let error = crate::premiere_to_tesseract(&project, &output, None, false).unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
        assert!(!output.exists());
    }
    let (directory, project, _, external) = native_media_fixture("");
    write_prproj(
        &project,
        &one_second_xml(&format!("<FilePath>{}</FilePath>", external.display())),
    );
    let output = directory.path().join("missing-alias");
    let error = crate::premiere_to_tesseract(&project, &output, None, false).unwrap_err();
    assert!(
        error.to_string().contains("identity cannot be verified"),
        "{error}"
    );
    assert!(!output.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_inspection_and_import_do_not_inspect_unconsumed_embedded_audio() {
    let source = one_second_xml("<RelativePath>media/source.mp4</RelativePath>")
        .replace("<VideoStream ObjectRef=\"8\"/>", "<VideoStream ObjectRef=\"8\"/><AudioStream ObjectRef=\"99\"/>")
        .replace("</PremiereData>", "<AudioStream ObjectID=\"99\"><AudioChannelLayout>[{\"channellabel\":100},{\"channellabel\":101}]</AudioChannelLayout><Duration>254016000000</Duration><FrameRate>5760000</FrameRate></AudioStream></PremiereData>");
    let (directory, project, packaged, _) = native_media_fixture("");
    fs::write(packaged, MEDIA).unwrap();
    write_prproj(&project, &source);
    let (_, sequence, _) = loaded_sequence(&project);
    let target = sequence.id.as_deref().unwrap();
    let selected_media = crate::Premiere
        .inspect_media(
            &project,
            &crate::PremiereImportOptions {
                sequence: Some(target.to_owned()),
            },
            None,
        )
        .unwrap();
    assert!(
        crate::tesseract_output::require_video_admission(&selected_media, &project).is_ok(),
        "{selected_media:?}"
    );
    let output = directory.path().join("picture-only");
    crate::premiere_to_tesseract(&project, &output, None, false).unwrap();
    let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let pictures: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(pictures.len(), 1);
    assert!(!layers.iter().any(|layer| layer["type"] == "Audio"));
    assert_eq!(pictures[0]["volume"], 0.0);
}

#[test]
fn malformed_native_sound_is_not_an_unsupported_audio_omission() {
    let source = one_second_xml("<RelativePath>media/source.mp4</RelativePath>").replace(
        "<VideoStream ObjectRef=\"8\"/>",
        "<VideoStream ObjectRef=\"8\"/><AudioStream ObjectRef=\"99\"/>",
    );
    for (fields, reason) in [
        ("<AudioChannelLayout>not-json</AudioChannelLayout><Duration>254016000000</Duration><FrameRate>11</FrameRate>", "invalid conversion JSON"),
        ("<AudioChannelLayout>[{\"channellabel\":100},{\"channellabel\":101}]</AudioChannelLayout><Duration>invalid</Duration><FrameRate>5292000</FrameRate>", "invalid Duration"),
        ("<AudioChannelLayout>[{\"channellabel\":100},{\"channellabel\":101}]</AudioChannelLayout><Duration>254016000000</Duration><FrameRate>invalid</FrameRate>", "invalid FrameRate"),
    ] {
        let xml = source.replace("</PremiereData>", &format!("<AudioStream ObjectID=\"99\">{fields}</AudioStream></PremiereData>"));
        let (directory, project, packaged, _) = native_media_fixture("");
        fs::write(packaged, MEDIA).unwrap();
        write_prproj(&project, &xml);
        let output = directory.path().join("malformed-audio");
        let error = crate::premiere_to_tesseract(&project, &output, None, false).unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
        assert!(!error.to_string().contains("native audio was not imported"));
        assert!(!output.exists());
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn premiere_to_tesseract_accepts_same_bytes_for_external_and_packaged_candidates() {
    let (directory, project, packaged, external) = native_media_fixture(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>",
    );
    let bytes = h264_bytes();
    std::fs::write(&packaged, &bytes).unwrap();
    std::fs::write(&external, &bytes).unwrap();
    let output = directory.path().join("same.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    assert!(output.is_file());
}

fn loaded_sequence(
    project: &Path,
) -> (
    PathBuf,
    crate::format::PrSequence,
    std::sync::Arc<std::collections::BTreeMap<crate::format::MediaId, crate::format::PrMedia>>,
) {
    let source = project.canonicalize().unwrap();
    let (project, omissions) = crate::format::PrProjectFile::load_selected(&source, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let (mut sequences, media) = project.into_parts();
    (source, sequences.remove(0), std::sync::Arc::new(media))
}

#[cfg(unix)]
#[test]
fn unpackageable_media_name_rejects_used_video_admission() {
    // A backslash is valid in a Unix file name but not in an archive path.
    let (_directory, project, packaged, _) =
        native_media_fixture(r"<RelativePath>media/source\clip.mp4</RelativePath>");
    fs::write(packaged.with_file_name(r"source\clip.mp4"), h264_bytes()).unwrap();
    let (source, sequence, media) = loaded_sequence(&project);
    let mut omissions = Vec::new();
    let error = match crate::tesseract_output::convert_premiere_sequence(
        &source,
        sequence,
        media,
        &mut omissions,
    ) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("unsafe used video must fail admission"),
    };
    assert!(
        error.contains("failed admission") && error.contains("unsafe archive path"),
        "{error}"
    );
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn project_build_failure_is_an_error_not_an_omission() {
    let (_directory, project, packaged, _) =
        native_media_fixture("<RelativePath>media/source.mp4</RelativePath>");
    fs::write(&packaged, h264_bytes()).unwrap();
    let (source, mut sequence, media) = loaded_sequence(&project);
    // Media inspection passes; the project build then rejects the canvas.
    sequence.width = 0;
    let mut omissions = Vec::new();
    let result = crate::tesseract_output::convert_premiere_sequence(
        &source,
        sequence,
        media,
        &mut omissions,
    );
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("width and height must be non-zero"),
        "{error}"
    );
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn external_only_relative_media_requires_matching_absolute_identity() {
    let (directory, project, _packaged, external) =
        native_media_fixture("<RelativePath>../external/source.mp4</RelativePath>");
    std::fs::write(&external, h264_bytes()).unwrap();
    let paths = format!(
        "<RelativePath>../external/source.mp4</RelativePath><FilePath>{}</FilePath>",
        external.display()
    );
    write_prproj(&project, &one_second_xml(&paths));
    let converted = crate::tests::support::convert_tesseract_file(&project, None).unwrap();
    converted
        .write_to_staging(&directory.path().join("external.tsrct"))
        .unwrap();

    write_prproj(
        &project,
        &one_second_xml("<RelativePath>../external/source.mp4</RelativePath>"),
    );
    let error = crate::tests::support::convert_tesseract_file(&project, None).unwrap_err();
    assert!(error.to_string().contains("matching absolute alias"));

    let conflicting = directory.path().join("different.mp4");
    std::fs::write(&conflicting, b"different source").unwrap();
    write_prproj(
        &project,
        &one_second_xml(&paths.replace(external.to_str().unwrap(), conflicting.to_str().unwrap())),
    );
    assert!(
        crate::tests::support::convert_tesseract_file(&project, None)
            .unwrap_err()
            .to_string()
            .contains("different bytes")
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn packaged_media_digest_must_match_the_inspected_source() {
    let (directory, project, packaged, _external) =
        native_media_fixture("<RelativePath>media/source.mp4</RelativePath>");
    std::fs::write(&packaged, h264_bytes()).unwrap();
    let output = directory.path().join("changed-import");
    let converted =
        crate::tesseract_import::TesseractImport::convert(&project, &output, None).unwrap();
    let mut changed = h264_bytes();
    *changed.last_mut().unwrap() ^= 1;
    std::fs::write(&packaged, changed).unwrap();
    assert!(converted
        .write()
        .unwrap_err()
        .to_string()
        .contains("packaged source bytes changed"));
    assert!(!output.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn premiere_to_tesseract_rejects_conflicting_native_candidates_before_output() {
    let (directory, project, packaged, external) = native_media_fixture(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>",
    );
    let bytes = h264_bytes();
    let mut different_valid_mp4 = bytes.clone();
    *different_valid_mp4.last_mut().unwrap() ^= 1;
    std::fs::write(&packaged, different_valid_mp4).unwrap();
    std::fs::write(&external, bytes).unwrap();
    for path in [&packaged, &external] {
        let input = std::fs::File::open(path).unwrap();
        let size = input.metadata().unwrap().len();
        media_transcode::inspect::inspect(input, size, false).unwrap();
    }
    let output = directory.path().join("conflict.tsrct");
    let error = crate::tests::support::build_tesseract_file(&project, &output, None).unwrap_err();
    assert!(error.to_string().contains("identify different bytes"));
    assert!(!output.exists());
}

/// A live package-local RelativePath identifies the media, so a missing `../`
/// hint beside it is stale history. Without that copy, a missing hint that no
/// live absolute alias vouches for omits the unavailable video's placements.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn missing_relative_hint_is_stale_beside_live_package_media_else_video_is_omitted() {
    // Whether the packaged and external copies exist, then the admission error.
    for (packaged_live, external_live, expected_error) in [
        (true, false, None),
        (
            false,
            true,
            Some("missing media: native media candidate is missing; identity cannot be verified without a matching absolute alias: [\"media/source.mp4\"]"),
        ),
        (
            false,
            false,
            Some("missing media: native media candidates are missing; identity cannot be verified"),
        ),
    ] {
        let (_directory, project, packaged, external) = native_media_fixture(
            "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>",
        );
        for (path, live) in [(&packaged, packaged_live), (&external, external_live)] {
            if live {
                fs::write(path, h264_bytes()).unwrap();
            }
        }
        let (source, sequence, media) = loaded_sequence(&project);
        let mut omissions = Vec::new();
        let converted = crate::tesseract_output::convert_premiere_sequence(
            &source,
            sequence,
            media,
            &mut omissions,
        );
        if let Some(reason) = expected_error {
            assert!(converted.unwrap().is_none());
            assert!(omissions.iter().any(|note| note.reason.contains(reason)), "{omissions:?}");
        } else {
            assert!(converted.unwrap().is_some());
            assert!(omissions.is_empty(), "{omissions:?}");
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn premiere_to_tesseract_accepts_adobe_save_as_stale_relative_hint_with_verified_absolute_alias() {
    let (directory, project, packaged, external) = native_media_fixture(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>",
    );
    let paths = format!(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath><ActualMediaFilePath>{}</ActualMediaFilePath><FilePath>{}</FilePath>",
        external.display(), external.display()
    );
    write_prproj(&project, &one_second_xml(&paths));
    std::fs::write(&external, h264_bytes()).unwrap();
    assert!(!packaged.exists());
    let output = directory.path().join("save-as.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    assert!(output.is_file());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn premiere_to_tesseract_accepts_stale_external_relative_hint_only_with_live_matching_alias() {
    let (directory, project, packaged, external) = native_media_fixture(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>",
    );
    let paths = format!(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath><FilePath>{}</FilePath>",
        packaged.display()
    );
    write_prproj(&project, &one_second_xml(&paths));
    std::fs::write(&packaged, h264_bytes()).unwrap();
    assert!(!external.exists());
    let output = directory.path().join("relocated.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    assert!(output.is_file());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn premiere_to_tesseract_rechecks_reappearing_relative_alias_before_writing() {
    let (directory, project, packaged, external) = native_media_fixture(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>",
    );
    let paths = format!(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath><FilePath>{}</FilePath>",
        external.display()
    );
    write_prproj(&project, &one_second_xml(&paths));
    let mut bytes = h264_bytes();
    std::fs::write(&external, &bytes).unwrap();
    let output = directory.path().join("changed-alias-import");
    let converted =
        crate::tesseract_import::TesseractImport::convert(&project, &output, None).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&packaged, bytes).unwrap();
    let error = converted.write().unwrap_err();
    assert!(error.to_string().contains("identify different bytes"));
    assert!(!output.exists());
}

#[test]
fn premiere_to_tesseract_rejects_conflicting_absolute_alias_with_missing_relative_hint() {
    let (directory, project, packaged, external) = native_media_fixture(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>",
    );
    let conflict = directory.path().join("conflict.mp4");
    let paths = format!(
        "<RelativePath>../external/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath><ActualMediaFilePath>{}</ActualMediaFilePath><FilePath>{}</FilePath>",
        external.display(), conflict.display()
    );
    write_prproj(&project, &one_second_xml(&paths));
    let mut bytes = h264_bytes();
    std::fs::write(&external, &bytes).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(conflict, bytes).unwrap();
    assert!(!packaged.exists());
    let output = directory.path().join("conflict.tsrct");
    let error = crate::tests::support::build_tesseract_file(&project, &output, None).unwrap_err();
    assert!(error.to_string().contains("identify different bytes"));
    assert!(!output.exists());
}

// Adobe Save As outside the package keeps the old package-local hint and adds
// one from the logical save path. A symlinked ancestor, such as macOS /tmp,
// can leave that hint stale too. Premiere still links the absolute path.
fn stale_relative_hints_with_aliases(actual: &Path, file_path: &Path) -> String {
    format!(
        "<RelativePath>../stale/source.mp4</RelativePath><RelativePath>./media/source.mp4</RelativePath><ActualMediaFilePath>{}</ActualMediaFilePath><FilePath>{}</FilePath>",
        actual.display(),
        file_path.display()
    )
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn premiere_to_tesseract_uses_agreeing_absolute_aliases_when_every_relative_hint_is_stale() {
    let (directory, project, packaged, external) =
        native_media_fixture("<RelativePath>./media/source.mp4</RelativePath>");
    std::fs::write(&external, h264_bytes()).unwrap();
    assert!(!packaged.exists());
    let paths = stale_relative_hints_with_aliases(&external, &external);
    write_prproj(&project, &one_second_xml(&paths));
    let output = directory.path().join("save-as-outside.tsrct");
    crate::tests::support::build_tesseract_file(&project, &output, None).unwrap();
    assert!(output.is_file());
}

#[test]
fn premiere_to_tesseract_rejects_conflicting_absolute_aliases_when_every_relative_hint_is_stale() {
    let (directory, project, packaged, external) =
        native_media_fixture("<RelativePath>./media/source.mp4</RelativePath>");
    let conflict = directory.path().join("conflict.mp4");
    let mut bytes = h264_bytes();
    std::fs::write(&external, &bytes).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&conflict, bytes).unwrap();
    assert!(!packaged.exists());
    let paths = stale_relative_hints_with_aliases(&external, &conflict);
    write_prproj(&project, &one_second_xml(&paths));
    let output = directory.path().join("conflict.tsrct");
    let error = crate::tests::support::build_tesseract_file(&project, &output, None).unwrap_err();
    assert!(
        error.to_string().contains("identify different bytes"),
        "{error}"
    );
    assert!(!output.exists());
}

#[test]
fn premiere_to_tesseract_rejects_empty_native_candidate_before_output() {
    let (directory, project, packaged, _external) =
        native_media_fixture("<RelativePath/><RelativePath>media/source.mp4</RelativePath>");
    std::fs::write(packaged, h264_bytes()).unwrap();
    let output = directory.path().join("empty.tsrct");
    let error = crate::tests::support::build_tesseract_file(&project, &output, None).unwrap_err();
    assert!(error.to_string().contains("empty or absolute RelativePath"));
    assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn premiere_to_tesseract_rejects_package_local_symlink_escape_before_output() {
    use std::os::unix::fs::symlink;

    for paths in [
        "<RelativePath>media/source.mp4</RelativePath>".to_owned(),
        format!(r"<RelativePath>.\media\source.mp4</RelativePath>{WINDOWS_ALIASES}"),
    ] {
        let (directory, project, packaged, external) = native_media_fixture(&paths);
        std::fs::write(&external, h264_bytes()).unwrap();
        symlink(&external, &packaged).unwrap();
        let output = directory.path().join("escape.tsrct");
        let error =
            crate::tests::support::build_tesseract_file(&project, &output, None).unwrap_err();
        assert!(
            error.to_string().contains("escapes the source package"),
            "{paths}: {error}"
        );
        assert!(!output.exists());
    }
}

/// Drive-absolute aliases as Premiere writes them on Windows; no host but the
/// saving one can open them.
const WINDOWS_ALIASES: &str = r"<FilePath>C:\Users\editor\package\media\source.mp4</FilePath><ActualMediaFilePath>C:\Users\editor\package\media\source.mp4</ActualMediaFilePath>";

#[cfg(unix)]
#[cfg(feature = "ffmpeg-library")]
#[test]
fn windows_saved_media_converts_only_from_its_verified_package_copy() {
    const PACKAGED: &str = r"<RelativePath>.\media\source.mp4</RelativePath>";
    const EXTERNAL: &str = r"<RelativePath>..\external\source.mp4</RelativePath>";
    let bytes = h264_bytes();
    let mut other = bytes.clone();
    *other.last_mut().unwrap() ^= 1;
    // Hints, packaged and external file bytes, and the rejection if any.
    for (hints, packaged_bytes, external_bytes, rejection) in [
        (PACKAGED.to_owned(), Some(&bytes), None, None),
        (
            PACKAGED.to_owned(),
            None,
            None,
            Some("identity cannot be verified"),
        ),
        (
            format!("{PACKAGED}{EXTERNAL}"),
            Some(&bytes),
            Some(&other),
            Some("identify different bytes"),
        ),
        // A Windows alias never vouches for media outside the package.
        (
            EXTERNAL.to_owned(),
            None,
            Some(&bytes),
            Some("media saved on Windows has no package-local RelativePath"),
        ),
    ] {
        let (directory, project, packaged, external) =
            native_media_fixture(&format!("{hints}{WINDOWS_ALIASES}"));
        for (path, contents) in [(&packaged, packaged_bytes), (&external, external_bytes)] {
            if let Some(contents) = contents {
                fs::write(path, contents).unwrap();
            }
        }
        let output = directory.path().join("windows.tsrct");
        let result = crate::tests::support::build_tesseract_file(&project, &output, None);
        match rejection {
            None => {
                result.unwrap();
                let archive = TesseractFile::open(&output).unwrap();
                let mut packaged_copy = Vec::new();
                std::io::Read::read_to_end(
                    &mut archive.asset("premiere-video-1").unwrap().open().unwrap(),
                    &mut packaged_copy,
                )
                .unwrap();
                assert_eq!(packaged_copy, bytes, "{hints}");
            }
            Some(reason) => {
                let error = result.unwrap_err().to_string();
                assert!(error.contains(reason), "{hints}: {error}");
                assert!(!output.exists());
            }
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
fn write_tesseract_document(path: &Path, document: serde_json::Value) {
    let media = path.parent().unwrap().join("black-1920-1s.mp4");
    std::fs::write(&media, MEDIA).unwrap();
    write_archive(path, &document, &media);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn tesseract_to_premiere_rebuilds_graph_and_preserves_exact_media() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.tsrct");
    write_tesseract_document(&source, editable_video_document());
    let package = directory.path().join("premiere-package");
    crate::tests::support::tesseract_to_premiere(&source, &package).unwrap();

    let media = package.join("media").join("black-1920-1s.mp4");
    assert_eq!(
        std::fs::read(&media).unwrap(),
        std::fs::read(source.parent().unwrap().join("black-1920-1s.mp4")).unwrap()
    );
    let xml = crate::format::read_xml(&package.join("project.prproj")).unwrap();
    let premiere_project = crate::format::inspect_project_with_media(&xml, None).unwrap();
    let premiere_clip = premiere_project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    let premiere_media = premiere_project.media(premiere_clip).unwrap();
    assert_eq!(inspect_project(&xml, None).unwrap().name, "Fresh exact 30");
    assert_eq!(premiere_clip.start_ticks, 0);
    assert_eq!(premiere_clip.end_ticks, 254_016_000_000);
    assert_eq!(premiere_clip.in_ticks, 0);
    assert_eq!(premiere_clip.out_ticks, 254_016_000_000);
    let video = premiere_media.video.as_ref().unwrap();
    assert_eq!(video.intrinsic_ticks, 254_016_000_000);
    assert_eq!(
        inspect_project(&xml, None)
            .unwrap()
            .frame_rate
            .ticks_per_frame(),
        8_467_200_000
    );
    assert_eq!(video.width, 1920);
    assert_eq!(video.height, 1080);
    assert_eq!(
        premiere_media.relative_path.as_deref(),
        Some("./media/black-1920-1s.mp4")
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn tesseract_to_premiere_snaps_persisted_millisecond_boundaries() {
    let directory = tempfile::tempdir().unwrap();

    for (label, stored_duration) in [
        ("exact-frame-float", 1000.0 / 30.0),
        ("integer-millisecond", 33.0),
    ] {
        let source = directory.path().join(format!("{label}.tsrct"));
        let mut document = editable_video_document();
        document["duration"] = json!(1.0 / 30.0);
        let video = &mut document["composition"]["layers"][0];
        video["activeRange"] = json!({"start":0,"duration":stored_duration});
        video.as_object_mut().unwrap().remove("playback");
        video["type"] = json!("Media");
        video["source"]["kind"] = json!("video");
        document["composition"]["layers"][0]["sourceRange"]["duration"] = json!(stored_duration);
        write_tesseract_document(&source, document);
        let output = directory.path().join(format!("{label}-output"));
        crate::tests::support::tesseract_to_premiere(&source, &output).unwrap();
        let premiere_clip = inspect(
            &crate::format::read_xml(&output.join("project.prproj")).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(premiere_clip.end_ticks, 8_467_200_000);
        assert_eq!(premiere_clip.out_ticks, 8_467_200_000);
    }

    let fractional_source = directory.path().join("fractional-near-miss.tsrct");
    let mut fractional = editable_video_document();
    fractional["duration"] = json!(0.0338);
    let video = &mut fractional["composition"]["layers"][0];
    video["activeRange"] = json!({"start":0,"duration":33.8});
    video.as_object_mut().unwrap().remove("playback");
    video["type"] = json!("Media");
    video["source"]["kind"] = json!("video");
    fractional["composition"]["layers"][0]["sourceRange"]["duration"] = json!(33.8);
    write_tesseract_document(&fractional_source, fractional);
    let reopened = TesseractFile::open(&fractional_source).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(reopened.project_json_bytes()).unwrap();
    assert_eq!(
        (*crate::test_support::layer_range(&raw["composition"]["layers"][0]))["duration"],
        33.8
    );
    // Storage retains the authored number; the checked millisecond data view
    // still rounds it exactly as the converter expects.
    assert_eq!(reopened.project_json().unwrap(), raw);
    assert_eq!(
        reopened.project().composition().layers()[0]
            .active_range()
            .duration
            .as_millis(),
        34
    );
    let fractional_output = directory.path().join("fractional-near-miss-output");
    crate::tests::support::tesseract_to_premiere(&fractional_source, &fractional_output).unwrap();
    let premiere_clip = inspect(
        &crate::format::read_xml(&fractional_output.join("project.prproj")).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(premiere_clip.end_ticks, 8_467_200_000);
    assert_eq!(premiere_clip.out_ticks, 8_467_200_000);
}

#[test]
fn ambiguous_import_fails_before_media_validation_or_publication() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let first = one_second_xml("<RelativePath>media/source.mp4</RelativePath>");
    let second = first
        .replace("ObjectID=\"", "ObjectID=\"2")
        .replace("ObjectRef=\"", "ObjectRef=\"2")
        .replace("ObjectUID=\"", "ObjectUID=\"z-second-")
        .replace("ObjectURef=\"", "ObjectURef=\"z-second-")
        .replace("<Name>Main</Name>", "<Name>Second</Name>")
        .replace("media/source.mp4", "media/missing.mp4");
    let xml = first.replace(
        "</PremiereData>",
        &second.replace("<PremiereData Version=\"3\">", ""),
    );
    let input = root.join("project.prproj");
    write_prproj(&input, &xml);
    let output = root.join("output");
    let error = crate::tesseract_import::TesseractImport::convert(&input, &output, None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("--sequence") && error.contains("tsrct-conv inspect"),
        "{error}"
    );
    assert!(!output.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn prepared_tesseract_import_rejects_changed_inputs() {
    let (dir, native, media, _) =
        native_media_fixture("<RelativePath>media/source.mp4</RelativePath>");
    fs::write(&media, h264_bytes()).unwrap();
    let tesseract_output = dir.path().join("source.tsrct");
    let import =
        crate::tesseract_import::TesseractImport::convert(&native, &tesseract_output, None)
            .unwrap();
    fs::write(&media, b"changed").unwrap();
    assert!(import.write().unwrap_err().to_string().contains("changed"));
    assert!(!tesseract_output.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn changed_input_blocks_a_prepared_batch_without_publication() {
    let (root, input, media, _) =
        native_media_fixture("<RelativePath>media/source.mp4</RelativePath>");
    fs::write(media, h264_bytes()).unwrap();
    let output = root.path().join("tesseract_output");
    let conversion =
        crate::tesseract_import::TesseractImport::convert(&input, &output, Some("sequence-1"))
            .unwrap();
    fs::write(&input, b"changed after conversion").unwrap();
    let error = conversion.write().unwrap_err();
    assert!(error.to_string().contains("source changed"), "{error}");
    assert!(!output.exists());
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".conversion-tesseract-")));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn invalid_selection_and_late_destination_do_not_publish_files() {
    let (root, input, media, _) =
        native_media_fixture("<RelativePath>media/source.mp4</RelativePath>");
    fs::write(media, h264_bytes()).unwrap();
    let output = root.path().join("tesseract_output");
    assert!(
        crate::tesseract_import::TesseractImport::convert(&input, &output, Some("missing"))
            .is_err()
    );
    assert!(!output.exists());

    let conversion =
        crate::tesseract_import::TesseractImport::convert(&input, &output, None).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep"), b"existing file").unwrap();
    assert!(conversion.write().is_err());
    assert_eq!(fs::read(output.join("keep")).unwrap(), b"existing file");
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".conversion-tesseract-")));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn late_absolute_alias_blocks_prepared_batch() {
    let (dir, input, media, alias) =
        native_media_fixture("<RelativePath>media/source.mp4</RelativePath>");
    fs::write(media, h264_bytes()).unwrap();
    write_prproj(
        &input,
        &one_second_xml(&format!(
            "<RelativePath>media/source.mp4</RelativePath><FilePath>{}</FilePath>",
            alias.display()
        )),
    );
    let output = dir.path().join("out");
    let import = crate::tesseract_import::TesseractImport::convert(&input, &output, None).unwrap();
    fs::write(alias, b"different source").unwrap();
    assert!(import
        .write()
        .unwrap_err()
        .to_string()
        .contains("different bytes"));
    assert!(!output.exists());
}
