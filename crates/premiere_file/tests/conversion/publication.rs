use super::support::*;
use premiere_file::OmissionScope;
use std::{fs, path::Path};
use tesseract_file::TesseractFile;

#[cfg(unix)]
#[test]
fn non_utf8_input_and_output_paths_fail_without_writes() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let non_utf8 = root.join(OsStr::from_bytes(b"path-\xff"));
    let valid_path = root.join("source.prproj");
    fs::write(&valid_path, b"not a project").unwrap();

    for (input, output) in [
        (non_utf8.clone(), root.join("output")),
        (valid_path, non_utf8),
    ] {
        for check in [true, false] {
            for result in [
                premiere_to_tesseract(&input, &output, None, check).map(|_| ()),
                tesseract_to_premiere(&input, &output, check).map(|_| ()),
            ] {
                let error = result.unwrap_err();
                assert!(error.to_string().contains("UTF-8"), "{error}");
                assert!(!output.exists());
            }
        }
    }
}

#[test]
fn ambiguous_timelines_require_selection_before_ignoring_unsupported_content() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let xml = two_timelines();
    // Move only the second timeline's frames off the origin, which no reader
    // accepts; the first stays valid.
    let split = xml
        .find("<Sequence ObjectUID=\"z-second-sequence-1\">")
        .unwrap();
    let xml = format!(
        "{}{}",
        &xml[..split],
        xml[split..].replace("0,0,1920,1080", "8,0,1928,1080")
    );
    let input = fixture(root, &xml);
    let ambiguous = root.join("ambiguous");
    let error = premiere_to_tesseract(&input, &ambiguous, None, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("--sequence") && error.contains("tsrct-conv inspect"));
    assert!(!ambiguous.exists());

    let selected = root.join("selected");
    let omissions = premiere_to_tesseract(&input, &selected, Some("sequence-1"), false).unwrap();
    assert!(!omissions
        .iter()
        .any(|item| item.reason.contains("invalid FrameRect")));
    assert_eq!(project_files(&selected), [selected.join("project.tsrct")]);
}

#[test]
fn explicit_selection_ignores_malformed_unrelated_sequence_and_publishes() {
    for (field, target, replacement, expected_reason) in [
        (
            "guid",
            "<Sequence ObjectUID=\"z-second-sequence-1\">",
            "<Sequence>",
            "missing GUID",
        ),
        ("name", "<Name>Main</Name>", "<Name></Name>", "missing name"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let xml = two_timelines();
        let split = xml
            .find("<Sequence ObjectUID=\"z-second-sequence-1\">")
            .unwrap();
        let malformed = format!(
            "{}{}",
            &xml[..split],
            xml[split..].replacen(target, replacement, 1)
        );
        assert_ne!(xml, malformed, "fixture mutation for {field}");
        fixture(root, &malformed);

        let output = root.join("out");
        let omissions = premiere_to_tesseract(
            root.join("project.prproj"),
            &output,
            Some("sequence-1"),
            false,
        )
        .unwrap();

        assert!(
            !omissions
                .iter()
                .any(|item| item.reason.contains(expected_reason)),
            "{field}: {omissions:?}"
        );
        assert_eq!(
            project_files(&output),
            [output.join("project.tsrct")],
            "{field}"
        );
    }
}

#[test]
fn explicit_selection_does_not_convert_an_unrelated_cyclic_timeline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let xml = two_timelines();
    let source = "<VideoMediaSource ObjectID=\"27\">";
    assert!(xml.contains(source));
    let split = xml.find(source).unwrap();
    let xml = format!("{}{}", &xml[..split], xml[split..]
        .replacen(source, "<VideoSequenceSource ObjectID=\"27\"><SequenceSource><Sequence ObjectURef=\"z-second-sequence-1\"/></SequenceSource>", 1)
        .replacen("</VideoMediaSource>", "</VideoSequenceSource>", 1));
    let input = fixture(root, &xml);
    let output = root.join("out");
    let omissions = premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    assert!(!omissions.iter().any(|item| item.reason.contains("cyclic")));
    assert_eq!(project_files(&output), [output.join("project.tsrct")]);
}

#[test]
fn explicit_selection_preserves_valid_clip_in_a_cyclic_timeline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // The timeline keeps a valid clip (3) and adds a clip (9) that nests itself.
    let xml = one_second()
        .replace(
            "<TrackItem ObjectRef=\"3\"/>",
            "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"9\"/>",
        )
        .replace(
            "</PremiereData>",
            r#"
  <VideoClipTrackItem ObjectID="9"><ClipTrackItem><ComponentOwner><Components ObjectRef="10"/></ComponentOwner><TrackItem><Start>254016000000</Start><End>508032000000</End></TrackItem><SubClip ObjectRef="11"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
  <VideoComponentChain ObjectID="10"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>
  <SubClip ObjectID="11"><Clip ObjectRef="12"/><Name>nested</Name></SubClip>
  <VideoClip ObjectID="12"><Clip><Source ObjectRef="13"/><InPoint>0</InPoint><OutPoint>254016000000</OutPoint></Clip></VideoClip>
  <VideoSequenceSource ObjectID="13"><SequenceSource><Sequence ObjectURef="sequence-1"/></SequenceSource></VideoSequenceSource>
</PremiereData>"#,
        );
    fixture(root, &xml);
    let input = root.join("project.prproj");
    let output = root.join("explicit");
    let checked = premiere_to_tesseract(&input, &output, Some("sequence-1"), true).unwrap();
    assert!(!output.exists());
    let omissions = premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    assert_eq!(checked, omissions);
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "9" && item.scope == OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let files = project_files(&output);
    assert_eq!(files.len(), 1);
    assert_eq!(
        TesseractFile::open(&files[0])
            .unwrap()
            .metadata()
            .assets
            .len(),
        1
    );
}

#[test]
fn selected_timeline_ignores_unrelated_missing_media_without_staging_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let xml = two_timelines();
    let split = xml
        .find("<Sequence ObjectUID=\"z-second-sequence-1\">")
        .unwrap();
    fixture(
        root,
        &format!(
            "{}{}",
            &xml[..split],
            xml[split..].replace("media/source.mp4", "media/missing.mp4")
        ),
    );
    let output = root.join("out");
    let omissions = premiere_to_tesseract(
        root.join("project.prproj"),
        &output,
        Some("sequence-1"),
        false,
    )
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.contains("identity cannot be verified")),
        "{omissions:?}"
    );
    assert_eq!(project_files(&output), [output.join("project.tsrct")]);
    assert!(!fs::read_dir(root).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".conversion-tesseract-")));
}

#[test]
fn broken_occurrence_link_requires_selection_and_preserves_other_clips() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir(root.join("media")).unwrap();
    for name in ["clip-a.mp4", "clip-b.mp4"] {
        fs::copy(
            fixtures.join("video-30fps-10s.mp4"),
            root.join("media").join(name),
        )
        .unwrap();
    }
    let xml = read_xml(&fixtures.join("two-video-tracks.prproj"));
    let changed = xml.replace(
        "<TrackItem Index=\"1\" ObjectRef=\"149\"/>",
        "<TrackItem Index=\"1\" ObjectRef=\"999\"/>",
    );
    assert_ne!(xml, changed);
    let input = root.join("project.prproj");
    write_prproj(&input, &changed);
    // Two sequences need an explicit selection, whatever their topology.
    let error = premiere_to_tesseract(&input, root.join("all"), None, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("project has 2 selectable sequences; select one with --sequence <GUID>"),
        "{error}"
    );
    assert!(!root.join("all").exists());
    let output = root.join("out");
    let omissions = premiere_to_tesseract(
        &input,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    // Explicit selection can preserve the other clips without diagnosing the
    // unrelated full-project nesting graph.
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Occurrence);
    assert_eq!(omissions[0].record, "999");
    assert!(omissions[0].reason.contains("missing reference"));
    let files = project_files(&output);
    assert_eq!(files.len(), 1);
    let converted = TesseractFile::open(&files[0])
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        converted["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        2
    );
}

#[test]
fn no_convertible_occurrences_fail_without_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let xml = one_second().replace(
        "<Clip><Source",
        "<Clip><PlaybackSpeed>2</PlaybackSpeed><Source",
    );
    fixture(root, &xml);
    let output = root.join("out");
    let error = premiere_to_tesseract(root.join("project.prproj"), &output, None, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("no convertible video or audio occurrences")
            && error.contains("source span does not match the constant playback rate"),
        "{error}"
    );
    assert!(!output.exists());
}

#[test]
fn bad_used_media_rejects_check_and_execution_without_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir(root.join("media")).unwrap();
    fs::copy(
        fixtures.join("two-video-tracks.prproj"),
        root.join("project.prproj"),
    )
    .unwrap();
    fs::copy(
        fixtures.join("video-30fps-10s.mp4"),
        root.join("media/clip-a.mp4"),
    )
    .unwrap();
    fs::write(root.join("media/clip-b.mp4"), b"not a video").unwrap();
    let input = root.join("project.prproj");
    let output = root.join("out");
    let checked = premiere_to_tesseract(
        &input,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(!output.exists());
    let saved = premiere_to_tesseract(
        &input,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(checked, saved);
    assert!(
        saved.contains("clip-b.mp4")
            && saved.contains("failed admission")
            && saved.contains("MP4 metadata box exceeds its parent"),
        "{saved}"
    );
    assert!(!output.exists());
}

#[test]
fn premiere_to_tesseract_never_replaces_an_existing_directory_or_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(root, &one_second());
    fs::create_dir(root.join("existing")).unwrap();
    fs::write(root.join("existing/keep"), b"keep").unwrap();
    assert!(premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("existing"),
        None,
        false
    )
    .unwrap_err()
    .to_string()
    .contains("already exists"));
    assert_eq!(fs::read(root.join("existing/keep")).unwrap(), b"keep");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("absent", root.join("link")).unwrap();
        assert!(
            premiere_to_tesseract(root.join("project.prproj"), root.join("link"), None, false)
                .unwrap_err()
                .to_string()
                .contains("already exists")
        );
        assert!(root.join("link").is_symlink());
    }
}

#[test]
fn checks_validate_real_media_without_writes_and_share_execution_failures() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(root, &two_timelines());
    let before = tree_bytes(root);
    let error = premiere_to_tesseract(root.join("project.prproj"), root.join("out"), None, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("--sequence") && error.contains("tsrct-conv inspect"));
    assert_eq!(tree_bytes(root), before);
    assert!(!root.join("out").exists());

    premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("out"),
        Some("sequence-1"),
        false,
    )
    .unwrap();
    let projects = project_files(&root.join("out"));
    assert_eq!(projects, [root.join("out/project.tsrct")]);
    let input = &projects[0];
    assert!(input.is_file());

    let before = tree_bytes(root);
    tesseract_to_premiere(input, root.join("native"), true).unwrap();
    assert_eq!(tree_bytes(root), before);
    assert!(!root.join("native").exists());

    tesseract_to_premiere(input, root.join("native"), false).unwrap();
    assert!(root.join("native/project.prproj").is_file());
    assert!(root.join("native/media/source.mp4").is_file());

    fs::write(root.join("media/source.mp4"), b"invalid mp4 payload").unwrap();
    let before = tree_bytes(root);
    let checked_error = premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("bad"),
        Some("sequence-1"),
        true,
    )
    .unwrap_err()
    .to_string();
    let write_error = premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("bad"),
        Some("sequence-1"),
        false,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(checked_error, write_error);
    assert_eq!(tree_bytes(root), before);
    assert!(!root.join("bad").exists());
}

fn tree_bytes(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(
        root: &Path,
        path: &Path,
        out: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    visit(root, root, &mut out);
    out
}
