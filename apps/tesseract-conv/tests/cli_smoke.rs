//! Process-level checks for command arguments, output, and error presentation.
use premiere_file::{premiere_to_tesseract, tesseract_to_premiere};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Output},
};
const AEP: &[u8] =
    include_bytes!("../../../crates/aftereffects_file/tests/fixtures/ae26_one_comp.aep");
const XML: &str = include_str!("../../../crates/premiere_file/tests/fixtures/one-clip.xml");
const MEDIA: &[u8] = include_bytes!("../../../crates/premiere_file/tests/fixtures/video-30fps.mp4");
const EDITABLE: &str =
    include_str!("../../../crates/premiere_file/tests/fixtures/editable-video.json");

fn premiere_relocated_alpha_fixture(
    root: &Path,
) -> (premiere_file::MediaRelink, fx_conv::MediaMap, String) {
    use fx_conv::{sha256_file, MediaMapSource, MediaReplacement};
    let authored = r"\\?\E:\collected\source.mov";
    let xml = XML
        .replace("1270080000000", "254016000000")
        .replace("2540160000000", "254016000000")
        .replace("1920,1080", "16,16")
        .replace(
            "<RelativePath>media/source.mp4</RelativePath>",
            &format!("<FilePath>{authored}</FilePath>"),
        );
    write_prproj(&root.join("source.prproj"), &xml);
    fs::write(
        root.join("original.mov"),
        include_bytes!("../../../crates/premiere_file/tests/fixtures/alpha-media/animation.mov"),
    )
    .unwrap();
    fs::write(
        root.join("prepared.mov"),
        include_bytes!("../../../crates/premiere_file/tests/fixtures/alpha-media/prores4444.mov"),
    )
    .unwrap();
    let source = MediaMapSource {
        format: "premiere".into(),
        sha256: sha256_file(&root.join("source.prproj")).unwrap(),
        target: "sequence-1".into(),
    };
    let original = root.join("original.mov").canonicalize().unwrap();
    let original_hash = sha256_file(&original).unwrap();
    let relink = premiere_file::MediaRelink {
        version: 1,
        source: source.clone(),
        bindings: vec![premiere_file::MediaRelinkBinding {
            media_uid: "media-1".into(),
            authored_path: authored.into(),
            local_path: original.clone(),
            sha256: original_hash.clone(),
        }],
    };
    let map = fx_conv::MediaMap {
        version: 1,
        source,
        replacements: vec![MediaReplacement {
            original,
            original_sha256: original_hash,
            replacement: "prepared.mov".into(),
            replacement_sha256: sha256_file(&root.join("prepared.mov")).unwrap(),
        }],
    };
    (relink, map, xml)
}

#[test]
fn premiere_media_composition_publishes_prepared_alpha_after_authenticated_relocation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (relink, map, _) = premiere_relocated_alpha_fixture(root);
    fs::write(
        root.join("relink.json"),
        serde_json::to_vec(&relink).unwrap(),
    )
    .unwrap();
    fs::write(root.join("map.json"), serde_json::to_vec(&map).unwrap()).unwrap();
    for check in [true, false] {
        let mut args = vec![
            "convert",
            "source.prproj",
            "--to",
            "tesseract",
            "--sequence",
            "sequence-1",
            "--output",
            "converted",
            "--media-relink",
            "relink.json",
            "--media-map",
            "map.json",
            "--json",
        ];
        if check {
            args.push("--check");
        }
        let result = run(root, &args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| {
                note["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("native FFmpeg playback"))
            }));
        assert_eq!(
            report["artifactStatus"],
            if check { "planned" } else { "published" }
        );
        assert_eq!(root.join("converted").exists(), !check);
    }
    let archive =
        tesseract_file::TesseractFile::open(root.join("converted/project.tsrct")).unwrap();
    let document = archive.project_json().unwrap();
    let pictures: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(pictures.len(), 1);
    assert_eq!(
        pictures[0]["sourceRange"],
        json!({"start": 0, "duration": 1000})
    );
    assert_eq!(pictures[0]["source"]["assetId"], "premiere-video-1");
    let prepared = fs::read(root.join("prepared.mov")).unwrap();
    assert_eq!(
        archive
            .asset("premiere-video-1")
            .unwrap()
            .read_verified_bytes(prepared.len() as u64)
            .unwrap(),
        prepared
    );
    assert_eq!(
        fx_conv::sha256_file(&root.join("source.prproj")).unwrap(),
        map.source.sha256
    );
    assert_eq!(
        fx_conv::sha256_file(&root.join("original.mov")).unwrap(),
        relink.bindings[0].sha256
    );
    assert_eq!(
        fx_conv::sha256_file(&root.join("prepared.mov")).unwrap(),
        map.replacements[0].replacement_sha256
    );
}

#[test]
fn premiere_media_composition_rejects_identity_errors_and_native_candidate_conflicts() {
    for case in [
        "uid",
        "authored",
        "relink-project",
        "relink-target",
        "disguised-original",
        "map-project",
        "map-target",
        "map-original-hash",
        "map-prepared-hash",
        "other-original-path",
        "native-conflict",
        "map-escape",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (mut relink, mut map, xml) = premiere_relocated_alpha_fixture(root);
        match case {
            "uid" => relink.bindings[0].media_uid = "absent-media".into(),
            "authored" => relink.bindings[0].authored_path = r"E:\other\source.mov".into(),
            "relink-project" => relink.source.sha256 = "0".repeat(64),
            "relink-target" => relink.source.target = "other".into(),
            "disguised-original" => {
                relink.bindings[0].sha256 = map.replacements[0].replacement_sha256.clone()
            }
            "map-project" => map.source.sha256 = "0".repeat(64),
            "map-target" => map.source.target = "other".into(),
            "map-original-hash" => map.replacements[0].original_sha256 = "0".repeat(64),
            "map-prepared-hash" => map.replacements[0].replacement_sha256 = "0".repeat(64),
            "other-original-path" => {
                fs::create_dir(root.join("other")).unwrap();
                fs::copy(root.join("original.mov"), root.join("other/original.mov")).unwrap();
                map.replacements[0].original =
                    root.join("other/original.mov").canonicalize().unwrap();
            }
            "native-conflict" => {
                fs::write(root.join("conflict.mov"), b"different original candidate").unwrap();
                let edited = xml.replace(
                    "<FilePath>",
                    "<RelativePath>conflict.mov</RelativePath><FilePath>",
                );
                write_prproj(&root.join("source.prproj"), &edited);
                let hash = fx_conv::sha256_file(&root.join("source.prproj")).unwrap();
                relink.source.sha256 = hash.clone();
                map.source.sha256 = hash;
            }
            "map-escape" => map.replacements[0].replacement = "../prepared.mov".into(),
            _ => unreachable!(),
        }
        fs::write(
            root.join("relink.json"),
            serde_json::to_vec(&relink).unwrap(),
        )
        .unwrap();
        fs::write(root.join("map.json"), serde_json::to_vec(&map).unwrap()).unwrap();
        let result = run(
            root,
            &[
                "convert",
                "source.prproj",
                "--to",
                "tesseract",
                "--sequence",
                "sequence-1",
                "--output",
                "converted",
                "--media-relink",
                "relink.json",
                "--media-map",
                "map.json",
            ],
        );
        assert!(!result.status.success(), "accepted {case}");
        assert!(!root.join("converted").exists(), "published {case}");
        if case == "native-conflict" {
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("different bytes"),
                "{:?}",
                result
            );
        }
    }
}

#[test]
fn media_map_import_packages_replacement_bytes_without_modifying_sources() {
    use aftereffects_file::{aep, rifx::Chunk, AfterEffects};
    use fx_conv::{sha256_file, ImportToTesseract, MediaMap, MediaMapSource, MediaReplacement};

    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "original.swf".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut native = aep::Project::parse(include_bytes!(
        "../../../crates/aftereffects_file/tests/fixtures/pr4442_native/sources/media_video.aep"
    ))
    .unwrap();
    relink(&mut native.chunks);
    let input = root.join("source.aep");
    let original = root.join("original.swf");
    fs::write(&input, native.encode().unwrap()).unwrap();
    fs::write(&original, b"FWS unsupported test source").unwrap();
    fs::create_dir(root.join("prepared")).unwrap();
    let video =
        include_bytes!("../../../crates/aftereffects_file/tests/fixtures/audio_e2e/movie.mov");
    let replacement = root.join("prepared/replacement.mov");
    fs::write(&replacement, video).unwrap();
    let targets = AfterEffects.list_import_targets(&input).unwrap();
    assert_eq!(targets.len(), 1);
    let target = targets[0].id.as_str();
    let source_hash = sha256_file(&input).unwrap();
    let original_hash = sha256_file(&original).unwrap();
    let map = MediaMap {
        version: 1,
        source: MediaMapSource {
            format: "after-effects".into(),
            sha256: source_hash.clone(),
            target: target.into(),
        },
        replacements: vec![MediaReplacement {
            original: original.canonicalize().unwrap(),
            original_sha256: original_hash.clone(),
            replacement: "replacement.mov".into(),
            replacement_sha256: sha256_file(&replacement).unwrap(),
        }],
    };
    fs::write(
        root.join("prepared/media-map.json"),
        serde_json::to_vec(&map).unwrap(),
    )
    .unwrap();
    let unmapped = run(
        root,
        &[
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "--composition",
            target,
            "--output",
            "unmapped",
        ],
    );
    assert!(!unmapped.status.success());
    assert!(!root.join("unmapped").exists());
    let unprepared: Value = serde_json::from_str(&success(run(
        root,
        &["inspect", "source.aep", "--composition", target, "--json"],
    )))
    .unwrap();
    assert_eq!(unprepared["media_admission"], "blocked");
    let prepared: Value = serde_json::from_str(&success(run(
        root,
        &[
            "inspect",
            "source.aep",
            "--composition",
            target,
            "--media-map",
            "prepared/media-map.json",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(prepared["media_admission"], "ready");
    aep_success(run(
        root,
        &[
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "--composition",
            target,
            "--media-map",
            "prepared/media-map.json",
            "--output",
            "converted",
            "--check",
        ],
    ));
    assert!(!root.join("converted").exists());
    aep_success(run(
        root,
        &[
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "--composition",
            target,
            "--media-map",
            "prepared/media-map.json",
            "--output",
            "converted",
        ],
    ));
    let archive =
        tesseract_file::TesseractFile::open(root.join("converted/project.tsrct")).unwrap();
    assert!(!archive.metadata().assets.is_empty());
    for id in archive.metadata().assets.keys() {
        let mut bytes = Vec::new();
        archive
            .asset(id)
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, video);
    }
    assert_eq!(sha256_file(&input).unwrap(), source_hash);
    assert_eq!(sha256_file(&original).unwrap(), original_hash);
    fs::write(&replacement, b"changed prepared media").unwrap();
    let stale = run(
        root,
        &[
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "--composition",
            target,
            "--media-map",
            "prepared/media-map.json",
            "--output",
            "stale",
        ],
    );
    assert!(!stale.status.success());
    assert!(!root.join("stale").exists());
}

#[test]
fn transcode_never_overwrites_an_existing_media_file() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("input.mov"), b"original source").unwrap();
    fs::write(root.join("output.mov"), b"earlier successful output").unwrap();
    let output = run(root, &["transcode", "input.mov", "--output", "output.mov"]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read(root.join("input.mov")).unwrap(),
        b"original source"
    );
    assert_eq!(
        fs::read(root.join("output.mov")).unwrap(),
        b"earlier successful output"
    );
    assert!(!root.join("media-map.json").exists());
}

#[test]
fn transcode_failure_keeps_the_cause_without_a_success_message() {
    let directory = tempfile::tempdir().unwrap();
    let output = run(
        directory.path(),
        &[
            "transcode",
            "missing.mov",
            "--output",
            "converted.mov",
            "--backend",
            "external-ffmpeg-command",
        ],
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("missing.mov"), "{error}");
    assert!(error.contains("I/O"), "{error}");
    assert!(!error.contains("Complete"));
    assert!(!directory.path().join("converted.mov").exists());
}

#[test]
fn json_conversion_retains_diagnostics_and_distinguishes_planned_from_published() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fixture(
        root,
        &one_second().replacen(
            "<ComponentChain/>",
            "<ComponentChain><UnknownEffect/></ComponentChain>",
            1,
        ),
    );
    for check in [true, false] {
        let mut args = vec![
            "convert",
            "project.prproj",
            "--to",
            "tesseract",
            "-o",
            "out",
            "--json",
        ];
        if check {
            args.push("--check");
        }
        let output = run(root, &args);
        assert!(output.status.success(), "{output:?}");
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            result["artifactStatus"],
            if check { "planned" } else { "published" }
        );
        assert!(!result["artifacts"].as_array().unwrap().is_empty());
        assert!(!result["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(root.join("out/project.tsrct").exists(), !check);
        let events: Vec<serde_json::Value> = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(events.iter().any(|event| event["type"] == "diagnostic"));
        assert_eq!(events.last().unwrap()["type"], "complete");
        assert!(events.iter().all(|event| event["command"] == "convert"));
    }
}

#[test]
fn json_runtime_failures_preserve_causes_without_success_output() {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec![
            "convert",
            "missing.prproj",
            "--to",
            "tesseract",
            "-o",
            "out",
            "--json",
        ],
        vec![
            "transcode",
            "missing.mov",
            "-o",
            "out.mov",
            "--backend",
            "external-ffmpeg-command",
            "--json",
        ],
    ] {
        let output = run(directory.path(), &args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let events: Vec<serde_json::Value> = String::from_utf8(output.stderr)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.last().unwrap()["type"], "error");
        assert!(events.last().unwrap()["message"]
            .as_str()
            .unwrap()
            .contains("missing"));
        assert!(!events.last().unwrap()["causes"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(events.iter().all(|event| event["type"] != "complete"));
    }
    assert!(!directory.path().join("out").exists());
    assert!(!directory.path().join("out.mov").exists());
}

#[test]
fn inspect_aep_json_exposes_exact_composition_identity_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("source.aep"), AEP).unwrap();
    let inspected = success(run(root, &["inspect", "source.aep", "--json"]));
    let object: Value = serde_json::from_str(&inspected).unwrap();
    assert_eq!(object["schema_version"], 2);
    assert_eq!(object["media_admission"], "ready");
    assert_eq!(object["media_preflight"][0]["target"], "1");
    assert_eq!(object["source"]["bytes"], AEP.len());
    assert_eq!(object["source"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(object["source"]["format_version"], 97);
    assert!(object["source"]["producer_version_word"].as_u64().is_some());
    let comp = &object["compositions"][0];
    assert_eq!(comp["id"], 1);
    assert_eq!(comp["name"], "classic-3d");
    assert_eq!(comp["width"], 1920);
    assert_eq!(comp["height"], 1080);
    assert_eq!(comp["duration_secs"], 1.0);
    assert!(comp["frame_rate"].as_f64().unwrap() > 0.0);
    assert_eq!(comp["direct_layer_count"], 0);
    assert_eq!(comp["reachable_unique_layer_count"], 0);
    assert_eq!(object["font_inventory"], "unavailable");
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
    let text = success(run(root, &["inspect", "source.aep"]));
    assert!(text.contains("File: source.aep\nCompositions: 1\n\nID"));
    assert!(text.contains("MEDIA CHECK"));
    assert!(text.contains("WITH PRECOMPS"));
    let row = text.lines().find(|line| line.starts_with("1  ")).unwrap();
    assert!(row.contains("OK"));
    assert!(row.contains("1920x1080"));
    assert!(row.ends_with("classic-3d"));
    assert!(text.contains("classic-3d\n\nMedia check: 1/1 targets OK"));
    assert!(!text.contains("Full composition and footage details"));
    assert!(!text.contains("reachable_comps="));
    assert!(!text.contains("authored_path="));
    assert!(!text.contains("Saved"));
    let legacy = success(run(root, &["inspect", "source.aep", "--metadata-only"]));
    assert!(legacy.contains("comp id=1"));
}

#[test]
fn inspect_premiere_lists_native_metadata_without_media_or_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_prproj(&root.join("source.prproj"), XML);
    let inspected = success(run(
        root,
        &["inspect", "source.prproj", "--metadata-only", "--json"],
    ));
    let object: Value = serde_json::from_str(&inspected).unwrap();
    assert_eq!(object["schema_version"], 1);
    assert_eq!(object["format"], "premiere");
    assert_eq!(object["targets"].as_array().unwrap().len(), 1);
    let target = &object["targets"][0];
    assert_eq!(target["id"], "sequence-1");
    assert_eq!(target["name"], "Main");
    assert_eq!(target["width"], 1920);
    assert_eq!(target["height"], 1080);
    assert_eq!(target["fps"], 30.0);
    assert_eq!(target["duration_secs"], 5.0);
    assert_eq!(target["video_track_count"], 1);
    assert_eq!(target["audio_track_count"], 0);
    assert!(target["layer_count"].is_null());
    let text = success(run(root, &["inspect", "source.prproj"]));
    assert!(text.contains("id=sequence-1"));
    assert!(text.contains("1920x1080 fps=30"));
    assert!(text.contains("video_tracks=1 audio_tracks=0"));
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn inspect_premiere_reports_missing_used_media_without_claiming_readiness() {
    let dir = tempfile::tempdir().unwrap();
    write_prproj(&dir.path().join("source.prproj"), XML);
    let inspected: Value = serde_json::from_str(&success(run(
        dir.path(),
        &[
            "inspect",
            "source.prproj",
            "--sequence",
            "sequence-1",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(inspected["schema_version"], 2);
    assert_eq!(inspected["media_admission"], "blocked");
    assert_eq!(inspected["media_preflight"][0]["target"], "sequence-1");
    assert!(inspected["media_preflight"][0]["media"]
        .as_array()
        .unwrap()
        .iter()
        .any(|media| media["status"] == "missing"));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn inspect_subcommand_rejects_conversion_flags_without_io() {
    let dir = tempfile::tempdir().unwrap();
    for flag in [
        "--check",
        "--to",
        "--output",
        "--fps",
        "--expression-samples",
    ] {
        let result = run(dir.path(), &["inspect", "missing.prproj", flag]);
        assert!(!result.status.success(), "accepted {flag}");
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("unexpected argument"));
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn inspect_nested_and_missing_media_are_source_metadata_not_disk_probes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("nested.aep"),
        include_bytes!("../../../crates/aftereffects_file/tests/fixtures/layers/folder.aep"),
    )
    .unwrap();
    let nested: Value =
        serde_json::from_str(&success(run(root, &["inspect", "nested.aep", "--json"]))).unwrap();
    assert!(nested["compositions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|comp| comp["parent_folder"] == 10));
    fs::write(
        root.join("footage_not_missing.aep"),
        include_bytes!(
            "../../../crates/aftereffects_file/tests/fixtures/media/footage_not_missing.aep"
        ),
    )
    .unwrap();
    fs::write(
        root.join("footage_missing.aep"),
        include_bytes!(
            "../../../crates/aftereffects_file/tests/fixtures/media/footage_missing.aep"
        ),
    )
    .unwrap();
    let present: Value = serde_json::from_str(&success(run(
        root,
        &[
            "inspect",
            "footage_not_missing.aep",
            "--metadata-only",
            "--json",
        ],
    )))
    .unwrap();
    let missing: Value = serde_json::from_str(&success(run(
        root,
        &[
            "inspect",
            "footage_missing.aep",
            "--metadata-only",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(present["media"][0]["missing_at_save"], false);
    assert_eq!(missing["media"][0]["missing_at_save"], true);
    assert_eq!(present["media"][0]["current_status"], "not_checked");
    assert!(present["media"][0]["authored_path"]
        .as_str()
        .unwrap()
        .ends_with("sample_motionblur_transparency.exr"));
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);
}

#[test]
fn inspect_rejects_conversion_flags_and_other_formats_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("source.aep"), AEP).unwrap();
    for flags in [
        vec!["inspect", "source.aep", "--to", "tesseract"],
        vec!["inspect", "source.aep", "-o", "output"],
        vec!["inspect", "source.aep", "--check"],
        vec!["inspect", "source.aep", "--fps", "30"],
        vec!["source.aep", "--json"],
        vec!["source.aep", "--inspect"],
        vec!["inspect", "source.aep", "--from", "premiere"],
    ] {
        let result = run(root, &flags);
        assert!(!result.status.success(), "unexpected success: {flags:?}");
        assert!(result.stdout.is_empty());
    }
    assert!(!root.join("output").exists());
}

#[test]
fn inspect_malformed_source_fails_without_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("source.aep"), b"not an AEP").unwrap();
    let result = run(root, &["inspect", "source.aep", "--json"]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("read AEP structure"));
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn format_routes_run_checks_and_save_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(root, &one_second());
    let output = root.join("tesseract_output");
    let checked = success(run(
        root,
        &[
            "convert",
            "project.prproj",
            "--to",
            "tesseract",
            "-o",
            "tesseract_output",
            "--check",
        ],
    ));
    assert_eq!(checked, "Premiere to Tesseract conversion is valid.\n");
    assert!(!output.exists());

    let selected = success(run(
        root,
        &[
            "convert",
            "project.prproj",
            "--to",
            "tesseract",
            "-o",
            "tesseract_output",
            "--sequence",
            "sequence-1",
            "--check",
        ],
    ));
    assert_eq!(selected, "Premiere to Tesseract conversion is valid.\n");
    assert!(!output.exists());

    let saved = success(run(
        root,
        &[
            "convert",
            "project.prproj",
            "--to",
            "tesseract",
            "-o",
            "tesseract_output",
        ],
    ));
    assert_eq!(saved, "Saved project.tsrct.\n");
    let archive = output.join("project.tsrct");

    let checked = success(run(
        root,
        &[
            "convert",
            archive.to_str().unwrap(),
            "--to",
            "premiere",
            "-o",
            "premiere_output",
            "--check",
        ],
    ));
    assert_eq!(checked, "Tesseract to Premiere conversion is valid.\n");
    assert!(!root.join("premiere_output").exists());

    let saved = success(run(
        root,
        &[
            "convert",
            archive.to_str().unwrap(),
            "--to",
            "premiere",
            "-o",
            "premiere_output",
        ],
    ));
    assert_eq!(saved, "Saved Premiere project.\n");
    assert!(root.join("premiere_output/project.prproj").is_file());

    let help = run(root, &["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("tsrct-conv"));
    assert!(help.contains("convert"));
    assert!(help.contains("inspect"));
    assert!(help.contains("transcode"));
    assert!(!help.contains("--to"));
    assert!(!help.contains("--from"));
    assert!(help.contains(&format!(
        "fxSchemaVersion (this converter): {}",
        fx_schema::FX_SCHEMA_REVISION
    )));

    let bare = run(root, &[]);
    assert!(bare.status.success());
    assert!(String::from_utf8(bare.stdout).unwrap().contains("convert"));
    // Force Clap's terminal styling while capturing stdout so an unstyled
    // render_help() printed directly cannot silently replace its help printer.
    let styled = Command::new(env!("CARGO_BIN_EXE_tsrct-conv"))
        .env("CLICOLOR_FORCE", "1")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert!(styled.status.success());
    assert!(String::from_utf8(styled.stdout)
        .unwrap()
        .contains("\u{1b}[1m\u{1b}[4mUsage:"));
    let convert_help = success(run(root, &["convert", "--help"]));
    assert!(convert_help.contains("--to"));
    assert!(!convert_help.contains("--inspect"));

    let version = success(run(root, &["--version"]));
    assert!(version.starts_with("tsrct-conv "));
}

#[test]
fn export_reports_mismatched_and_missing_fx_schema_versions_even_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let project = json!({
        "$schema": fx_schema::EDITABLE_FX_DOCUMENT_SCHEMA_URL,
        "formatVersion": fx_schema::EDITABLE_FX_DOCUMENT_FORMAT_VERSION,
        "dimensions": {"width": 1080, "height": 1920},
        "duration": 6,
        "composition": {"id": "composition-1", "name": "Test", "layers": []}
    });
    tesseract_file::TesseractFileBuilder::from_project_json(&serde_json::to_vec(&project).unwrap())
        .unwrap()
        .write(root.join("current.tsrct"))
        .unwrap();

    for (name, version) in [
        ("older", Some(fx_schema::FX_SCHEMA_REVISION - 1)),
        ("legacy", None),
    ] {
        let path = root.join(format!("{name}.tsrct"));
        rewrite_fx_schema_version(&root.join("current.tsrct"), &path, version);
        for target in ["premiere", "after-effects"] {
            for mode in ["--check", "--fps"] {
                let mut args = vec![
                    "convert",
                    path.to_str().unwrap(),
                    "--to",
                    target,
                    "-o",
                    "output",
                    mode,
                ];
                if mode == "--fps" {
                    args.push("invalid");
                }
                let result = run(root, &args);
                let stderr = String::from_utf8_lossy(&result.stderr);
                assert!(
                    stderr.contains(&format!(
                        "Tesseract file fxSchemaVersion={}",
                        version.map_or_else(
                            || "unknown (missing)".to_owned(),
                            |value| value.to_string()
                        )
                    )),
                    "{target}: {stderr}"
                );
                assert!(stderr.contains(&format!(
                    "converter fxSchemaVersion={}",
                    fx_schema::FX_SCHEMA_REVISION
                )));
                if mode == "--fps" {
                    assert!(!result.status.success(), "{target}: invalid fps must fail");
                    assert!(stderr.contains("info: FX schema versions:"));
                }
            }
        }
    }

    let equal = run(
        root,
        &[
            "convert",
            "current.tsrct",
            "--to",
            "premiere",
            "-o",
            "output",
            "--check",
        ],
    );
    assert!(!String::from_utf8_lossy(&equal.stderr).contains("info: FX schema versions:"));
}

fn rewrite_fx_schema_version(source: &Path, destination: &Path, version: Option<u8>) {
    let mut source = zip::ZipArchive::new(fs::File::open(source).unwrap()).unwrap();
    let mut output = zip::ZipWriter::new(fs::File::create(destination).unwrap());
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name() == "metadata.json" {
            let mut metadata: Value = serde_json::from_slice(&bytes).unwrap();
            match version {
                Some(version) => metadata["fxSchemaVersion"] = json!(version),
                None => {
                    metadata.as_object_mut().unwrap().remove("fxSchemaVersion");
                }
            }
            bytes = serde_json::to_vec(&metadata).unwrap();
        }
        output
            .start_file(
                entry.name(),
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap();
}

#[test]
fn after_effects_import_checks_writes_and_reopens_editable_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("source.aep"), AEP).unwrap();

    let checked = aep_success(run(
        root,
        &[
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "-o",
            "checked",
            "--check",
        ],
    ));
    assert!(checked.contains("structure can be imported"));
    assert!(!root.join("checked").exists());

    let saved = aep_success(run(
        root,
        &[
            "convert",
            "source.aep",
            "--to",
            "tesseract",
            "-o",
            "written",
        ],
    ));
    assert!(saved.contains("project.tsrct"));
    let output = root.join("written/project.tsrct");
    let reopened = tesseract_file::TesseractFile::open(output).unwrap();
    let project = reopened.project_json().unwrap();
    assert_eq!(project["composition"]["name"], "classic-3d");
    assert_eq!(
        project["dimensions"],
        json!({ "width": 1920, "height": 1080 })
    );
    assert_eq!(project["duration"], 1.0);
    let layers = project["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0]["type"], "Group");
    assert_eq!(layers[0]["name"], "classic-3d");
    assert_eq!(layers[0]["layers"], json!([]));
    assert!(reopened.metadata().assets.is_empty());
}

#[test]
fn after_effects_accepts_uppercase_extension_and_explicit_format_override() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("source.AEP"), AEP).unwrap();
    aep_success(run(
        root,
        &[
            "convert",
            "source.AEP",
            "--to",
            "tesseract",
            "-o",
            "uppercase",
            "--check",
        ],
    ));
    assert!(!root.join("uppercase").exists());

    fs::write(root.join("source.data"), AEP).unwrap();
    aep_success(run(
        root,
        &[
            "convert",
            "source.data",
            "--from",
            "after-effects",
            "--to",
            "tesseract",
            "-o",
            "explicit",
            "--check",
        ],
    ));
    assert!(!root.join("explicit").exists());
}

#[test]
fn after_effects_selection_and_warning_parity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("layers.aep"),
        include_bytes!("../../../crates/aftereffects_file/tests/fixtures/layers/layer_misc.aep"),
    )
    .unwrap();
    let checked = run(
        root,
        &[
            "convert",
            "layers.aep",
            "--to",
            "tesseract",
            "--composition",
            "44",
            "-o",
            "checked",
            "--check",
        ],
    );
    let written = run(
        root,
        &[
            "convert",
            "layers.aep",
            "--to",
            "tesseract",
            "--composition",
            "44",
            "-o",
            "written",
        ],
    );
    assert!(checked.status.success());
    assert!(written.status.success());
    assert_eq!(checked.stderr, written.stderr);
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("[AE-PARENTING] composition 44 layer 59")
    );
    assert!(!root.join("checked").exists());
    let file = tesseract_file::TesseractFile::open(root.join("written/project.tsrct")).unwrap();
    assert_eq!(
        file.project_json().unwrap()["composition"]["name"],
        "parent"
    );
    let absent = run(
        root,
        &[
            "convert",
            "layers.aep",
            "--to",
            "tesseract",
            "--composition",
            "999999",
            "-o",
            "absent",
        ],
    );
    assert!(!absent.status.success());
    assert!(!root.join("absent").exists());
    let invalid = run(
        root,
        &[
            "convert",
            "missing.prproj",
            "--to",
            "tesseract",
            "--composition",
            "44",
            "-o",
            "invalid",
        ],
    );
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("--composition is only supported"));
    assert!(!root.join("invalid").exists());
}

#[test]
fn after_effects_multiple_compositions_require_selection_in_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("precomp.aep"),
        include_bytes!(
            "../../../crates/aftereffects_file/tests/fixtures/layers/outPoint_clamp.aep"
        ),
    )
    .unwrap();
    let checked = run(
        root,
        &[
            "convert",
            "precomp.aep",
            "--to",
            "tesseract",
            "-o",
            "checked",
            "--check",
        ],
    );
    let written = run(
        root,
        &[
            "convert",
            "precomp.aep",
            "--to",
            "tesseract",
            "-o",
            "written",
        ],
    );
    assert!(!checked.status.success());
    assert!(!written.status.success());
    assert_eq!(checked.stderr, written.stderr);
    assert!(String::from_utf8_lossy(&written.stderr).contains("--composition"));
    assert!(!root.join("checked").exists());
    assert!(!root.join("written").exists());
    aep_success(run(
        root,
        &[
            "convert",
            "precomp.aep",
            "--to",
            "tesseract",
            "--composition",
            "13",
            "-o",
            "written",
        ],
    ));
    let file = tesseract_file::TesseractFile::open(root.join("written/project.tsrct")).unwrap();
    assert_eq!(
        file.project_json().unwrap()["composition"]["name"],
        "outPoint_clamp_precomp"
    );
}

#[test]
fn after_effects_errors_do_not_publish_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("bad.aep"), b"not an AEP").unwrap();

    for args in [
        vec![
            "convert",
            "bad.aep",
            "--to",
            "tesseract",
            "-o",
            "bad-output",
        ],
        vec![
            "convert",
            "bad.aep",
            "--to",
            "premiere",
            "-o",
            "direction-output",
        ],
        vec![
            "convert",
            "bad.aep",
            "--to",
            "tesseract",
            "-o",
            "sequence-output",
            "--sequence",
            "id",
        ],
    ] {
        let output = run(root, &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    assert!(!root.join("bad-output").exists());
    assert!(!root.join("direction-output").exists());
    assert!(!root.join("sequence-output").exists());
    let reverse = run(
        root,
        &[
            "convert",
            "missing.tsrct",
            "--to",
            "after-effects",
            "-o",
            "reverse-output",
        ],
    );
    assert_eq!(reverse.status.code(), Some(1), "{reverse:?}");
    assert!(reverse.stdout.is_empty());
    assert!(String::from_utf8_lossy(&reverse.stderr).contains("inspect Tesseract input"));
    assert!(!root.join("reverse-output").exists());
}

#[test]
fn after_effects_static_export_checks_writes_and_reimports() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("solid.aep"),
        include_bytes!(
            "../../../crates/aftereffects_file/tests/fixtures/properties/transform_unseparated.aep"
        ),
    )
    .unwrap();
    aep_success(run(
        root,
        &[
            "convert",
            "solid.aep",
            "--to",
            "tesseract",
            "-o",
            "imported",
        ],
    ));
    let check = run(
        root,
        &[
            "convert",
            "imported/project.tsrct",
            "--to",
            "after-effects",
            "-o",
            "exported",
            "--check",
        ],
    );
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(!root.join("exported").exists());
    let write = run(
        root,
        &[
            "convert",
            "imported/project.tsrct",
            "--to",
            "after-effects",
            "-o",
            "exported",
        ],
    );
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
    assert_eq!(check.stderr, write.stderr);
    assert!(String::from_utf8_lossy(&write.stdout).contains("Adobe acceptance unverified"));
    let native = aftereffects_file::structure::read_project(
        &fs::read(root.join("exported/project.aep")).unwrap(),
    )
    .unwrap();
    let aftereffects_file::structure::ItemKind::Composition(comp) = &native.item(1).unwrap().kind
    else {
        panic!("composition")
    };
    assert_eq!(comp.layers.len(), 1);
    aep_success(run(
        root,
        &[
            "convert",
            "exported/project.aep",
            "--to",
            "tesseract",
            "-o",
            "reimported",
        ],
    ));
    assert!(root.join("reimported/project.tsrct").is_file());
    for flag in ["--sequence", "--composition"] {
        let failure = run(
            root,
            &[
                "imported/project.tsrct",
                "--to",
                "after-effects",
                "-o",
                "invalid",
                flag,
                "1",
            ],
        );
        assert!(!failure.status.success());
        assert!(!root.join("invalid").exists());
    }
}

#[test]
fn omitted_feature_is_reported_with_success_and_check_parity() {
    let dir = tempfile::tempdir().unwrap();
    for (name, xml) in [
        (
            "component",
            one_second().replacen(
                "<ComponentChain/>",
                "<ComponentChain><UnknownEffect/></ComponentChain>",
                1,
            ),
        ),
        (
            "group",
            one_second().replacen(
                "<VideoTrackGroup ObjectID=\"1\">",
                "<VideoTrackGroup ObjectID=\"1\"><UnknownEffect/>",
                1,
            ),
        ),
    ] {
        let root = dir.path().join(name);
        fs::create_dir(&root).unwrap();
        fixture(&root, &xml);
        let args = [
            "convert",
            "project.prproj",
            "--to",
            "tesseract",
            "-o",
            "out",
        ];
        let checked = run(&root, &[&args[..], &["--check"]].concat());
        assert!(checked.status.success(), "{checked:?}");
        assert!(!root.join("out").exists());
        let saved = run(&root, &args);
        assert!(saved.status.success(), "{saved:?}");
        assert!(root.join("out/project.tsrct").exists());
        assert_eq!(checked.stderr, saved.stderr);
        assert!(String::from_utf8_lossy(&saved.stderr).contains("UnknownEffect"));
    }
}

#[test]
fn reverse_omissions_are_reported_in_check_and_write_modes() {
    // SDR HEVC now prepares for editable AEP export. HDR/timecode remains
    // outside that destination policy and exercises native fallback.
    let unsupported_media =
        include_bytes!("../../../crates/premiere_file/tests/fixtures/feature_hdr_hlg_hvc1.mov");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
    document["composition"]["layers"][0]["transform"]["opacity"] = json!(99);
    document["composition"]["layers"][0]["effects"] = json!([
        {"id": 1, "effect": {"type": "mosaic", "horizontalBlocks": 10.0, "verticalBlocks": 20.0}}
    ]);
    document["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(2000);
    document["composition"]["layers"][0]["source"]["sourceRect"]["width"] = json!(320);
    document["composition"]["layers"][0]["source"]["sourceRect"]["height"] = json!(180);
    fs::write(root.join("source.mov"), unsupported_media).unwrap();
    tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&document).unwrap(),
    )
    .unwrap()
    .add_asset(
        "premiere-video-1",
        root.join("source.mov"),
        tesseract_file::AssetKind::Video,
    )
    .unwrap()
    .write(root.join("edited.tsrct"))
    .unwrap();

    let args = ["convert", "edited.tsrct", "--to", "premiere", "-o", "out"];
    let checked = run(root, &[&args[..], &["--check"]].concat());
    assert!(checked.status.success(), "{checked:?}");
    assert!(!root.join("out").exists());
    let saved = run(root, &args);
    assert!(saved.status.success(), "{saved:?}");
    assert!(root.join("out/project.prproj").is_file());
    assert_eq!(checked.stderr, saved.stderr);
    let stderr = String::from_utf8_lossy(&saved.stderr);
    assert!(stderr.contains("effects"), "{stderr}");
    assert!(!stderr.contains("opacity"), "{stderr}");
    assert!(
        stderr.contains("retained the native Premiere result"),
        "{stderr}"
    );
    assert!(!root.join("out/media/ae-0001").exists());
    assert_eq!(
        fs::read(root.join("out/media/source.mov")).unwrap(),
        unsupported_media
    );
}

/// Writes the editable video archive with `scripts` on its video layer 1.
fn scripted_archive(root: &Path, name: &str, scripts: &[(&str, String)]) {
    let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
    document["composition"]["dynamics"] = json!({"entries": scripts.iter().map(|(property, code)| json!({
        "target": {"kind": "layer", "layerId": 1, "propertyType": property},
        "animator": {"type": "jsScript", "layerTimeJsCode": code}
    })).collect::<Vec<_>>()});
    let media = root.join(format!("{name}.mp4"));
    fs::write(&media, MEDIA).unwrap();
    tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&document).unwrap(),
    )
    .unwrap()
    .add_asset("premiere-video-1", media, tesseract_file::AssetKind::Video)
    .unwrap()
    .write(root.join(format!("{name}.tsrct")))
    .unwrap();
}

#[test]
fn script_animation_exports_as_editable_keys_in_check_and_write_modes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    scripted_archive(
        root,
        "scripted",
        &[(
            "rotation",
            "return 90 * Math.sin(input.time.seconds * Math.PI);".to_owned(),
        )],
    );
    let args = ["convert", "scripted.tsrct", "--to", "premiere", "-o", "out"];
    let checked = run(root, &[&args[..], &["--check"]].concat());
    assert!(checked.status.success(), "{checked:?}");
    assert!(!root.join("out").exists());
    let saved = run(root, &args);
    assert!(saved.status.success(), "{saved:?}");
    assert_eq!(checked.stderr, saved.stderr);
    let stderr = String::from_utf8_lossy(&saved.stderr);
    assert!(
        stderr.contains("JS animation baking: 1 of 1 scripts became editable keys")
            && stderr.contains("1 of 1 baked JS animation tracks were written as native keys"),
        "{stderr}"
    );
    assert!(root.join("out/project.prproj").is_file());
}

/// Deeply nested scripts are baked, or kept with a diagnostic, on one or
/// several evaluation threads; the process never aborts on their parse.
/// Each case runs the converter in its own process, so an abort fails the
/// case rather than the test runner. Premiere export hands a script that it
/// keeps to an After Effects scope, which evaluates it again.
#[test]
fn nested_scripts_bake_or_keep_their_animator_without_aborting() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let nested = |depth: usize| format!("return {}50{};", "(".repeat(depth), ")".repeat(depth));
    let rotation = "return 90 * Math.sin(input.time.seconds * Math.PI);".to_owned();
    // A bounded 32 KiB malformed input, with one unclosed parenthesis per
    // byte, must keep its animator rather than abort export.
    let unclosed = format!("return {}1;", "(".repeat(32 * 1024 - "return 1;".len()));
    assert_eq!(unclosed.len(), 32 * 1024);
    // A valid source past the former 32 KiB quota bakes into editable keys
    // in both formats.
    let long = format!("return 50; //{}", " ".repeat(32 * 1024));
    // Archive name, scripts by Motion property, and the baked summaries of
    // Premiere and After Effects export.
    type Case<'a> = (&'a str, Vec<(&'a str, String)>, &'a str, &'a str);
    let cases: [Case<'_>; 4] = [
        (
            "single-1000",
            vec![("opacity", nested(1000))],
            "1 of 1 scripts",
            "1/1",
        ),
        (
            "several-200",
            vec![("opacity", nested(200)), ("rotation", rotation.clone())],
            "2 of 2 scripts",
            "2/2",
        ),
        (
            "unclosed",
            vec![("opacity", unclosed), ("rotation", rotation)],
            "1 of 2 scripts",
            "1/2",
        ),
        ("long", vec![("opacity", long)], "1 of 1 scripts", "1/1"),
    ];
    for (name, scripts, baked, after_effects_baked) in cases {
        scripted_archive(root, name, &scripts);
        let source = format!("{name}.tsrct");
        let output = run(
            root,
            &[
                "convert", &source, "--to", "premiere", "-o", name, "--check",
            ],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{name}: {:?} {stderr}",
            output.status
        );
        assert!(
            stderr.contains(&format!(
                "JS animation baking: {baked} became editable keys"
            )),
            "{name}: {stderr}"
        );
        if name == "unclosed" {
            assert!(
                stderr.contains("kept their animator, which export does not convert: the script failed: layer 1 opacity")
                    && stderr.contains("SyntaxError"),
                "{name}: {stderr}"
            );
        }
        // An invalid script remains in the After Effects scope, and its media
        // is no QuickTime movie, so export keeps the native Premiere result.
        let scope_reason = match name {
            "unclosed" => Some("SyntaxError"),
            _ => None,
        };
        if let Some(reason) = scope_reason {
            assert!(
                stderr.contains(&format!(
                    "JS animator layer 1 opacity was not baked: {reason}"
                )) && stderr.contains("retained the native Premiere result"),
                "{name}: {stderr}"
            );
        }
        assert!(!root.join(name).exists(), "{name}");
        let saved = run(root, &["convert", &source, "--to", "premiere", "-o", name]);
        assert!(saved.status.success(), "{name}: {saved:?}");
        assert_eq!(saved.stderr, output.stderr, "{name}");
        assert!(root.join(name).join("project.prproj").is_file(), "{name}");
        assert!(!root.join(name).join("media/ae-0001").exists(), "{name}");

        let aep = format!("{name}-aep");
        let checked = run(
            root,
            &[
                "convert",
                &source,
                "--to",
                "after-effects",
                "-o",
                &aep,
                "--check",
            ],
        );
        let aep_stderr = String::from_utf8_lossy(&checked.stderr);
        assert!(
            checked.status.success(),
            "{name}: {:?} {aep_stderr}",
            checked.status
        );
        assert!(
            aep_stderr.contains(&format!(
                "JS animator baking approximated {after_effects_baked} scalar/Path/Source Text tracks"
            )),
            "{name}: {aep_stderr}"
        );
        if name == "unclosed" {
            assert!(
                aep_stderr.contains("JS animator layer 1 opacity was not baked: SyntaxError"),
                "{name}: {aep_stderr}"
            );
        }
        assert!(!root.join(&aep).exists(), "{name}");
    }
}

#[test]
fn fps_sets_the_export_rate() {
    // The Adobe-derived 24 fps cut sits on frame 37; import rounds it to 1542 ms.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/premiere_file/tests/fixtures/feature_rate_24_cut_strict.prproj");
    success(run(
        root,
        &[
            "convert",
            fixture.to_str().unwrap(),
            "--to",
            "tesseract",
            "-o",
            "tesseract",
            "--sequence",
            "c8acf9c1-34b2-4086-9f55-d528950a7059",
        ],
    ));
    let document = "tesseract/project.tsrct";
    // The video clip ranges and the decompressed XML of a Premiere export at `fps`.
    let export = |fps: &str| {
        let output = format!("premiere-{fps}");
        success(run(
            root,
            &[
                "convert",
                document,
                "--to",
                "premiere",
                "-o",
                output.as_str(),
                "--fps",
                fps,
            ],
        ));
        let project = root.join(output).join("project.prproj");
        let (native, _) = premiere_file::PrProjectFile::load(&project).unwrap();
        let clips: Vec<_> = native
            .sequences()
            .next()
            .unwrap()
            .video_occurrences()
            .map(|clip| (clip.timeline_ticks(), clip.source_ticks()))
            .collect();
        let mut xml = String::new();
        flate2::read::GzDecoder::new(fs::File::open(&project).unwrap())
            .read_to_string(&mut xml)
            .unwrap();
        (clips, xml)
    };

    // `--fps 24` snaps the cut back to frame 37 of a 24 fps sequence.
    let (clips, xml) = export("24");
    let frame = 10_584_000_000;
    assert_eq!(
        clips,
        [
            (0..37 * frame, 0..37 * frame),
            (37 * frame..72 * frame, 0..35 * frame)
        ]
    );
    assert!(xml
        .contains("<MZ.Sequence.VideoTimeDisplayFormat>100</MZ.Sequence.VideoTimeDisplayFormat>"));

    // At 50 and 60 fps the 1542 ms cut snaps to frame 77 or 93 of the 3 s
    // timeline, the sequence stores the display code that Premiere saves for
    // its rate, and the 24 fps sources keep their own rate.
    for (fps, frame, cut, end, code) in [
        ("50", 5_080_320_000, 77, 150, "105"),
        ("60", 4_233_600_000, 93, 180, "108"),
    ] {
        let (clips, xml) = export(fps);
        assert_eq!(
            clips,
            [
                (0..cut * frame, 0..cut * frame),
                (cut * frame..end * frame, 0..(end - cut) * frame)
            ],
            "{fps} fps"
        );
        assert!(xml.contains(&format!(
            "<MZ.Sequence.VideoTimeDisplayFormat>{code}</MZ.Sequence.VideoTimeDisplayFormat>"
        )));
        assert!(xml.contains(&format!("<FrameRate>{frame}</FrameRate>")));
        assert!(xml.contains("<FrameRate>10584000000</FrameRate>"));
    }

    // Another rate rejects before any output is written.
    let output = run(
        root,
        &[
            "convert",
            document,
            "--to",
            "premiere",
            "-o",
            "premiere-48",
            "--fps",
            "48",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("--fps 48 is not supported for Premiere export"));
    assert!(!root.join("premiere-48").exists());

    // After Effects takes the decimal rate.
    let output = run(
        root,
        &[
            "convert",
            document,
            "--to",
            "after-effects",
            "-o",
            "after-effects",
            "--fps",
            "60",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let native = aftereffects_file::structure::read_project(
        &fs::read(root.join("after-effects/project.aep")).unwrap(),
    )
    .unwrap();
    let aftereffects_file::structure::ItemKind::Composition(composition) =
        &native.item(1).unwrap().kind
    else {
        panic!("composition 1");
    };
    assert_eq!(composition.frame_rate, 60.0);
}

#[test]
fn argument_and_runtime_errors_use_stderr_and_nonzero_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for args in [
        vec!["convert"],
        vec!["convert", "missing.prproj", "--to", "tesseract"],
        vec!["missing.prproj", "--to", "tesseract"],
        vec!["convert", "missing.prproj", "-o", "out"],
        vec!["convert", "missing.prproj", "--to", "svg", "-o", "out"],
        vec![
            "convert",
            "missing.prproj",
            "--to",
            "tesseract",
            "--from",
            "aep",
            "-o",
            "out",
        ],
    ] {
        let output = run(root, &args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    for (input, to) in [
        ("missing.prproj", "tesseract"),
        ("missing.tsrct", "premiere"),
    ] {
        for check in [false, true] {
            let mut args = vec!["convert", input, "--to", to, "-o", "out"];
            if check {
                args.push("--check");
            }
            let output = run(root, &args);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(!output.stderr.is_empty());
            assert!(!root.join("out").exists());
        }
    }
}

#[test]
fn explicit_source_format_handles_ambiguous_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root, &one_second());
    let ambiguous = root.join("project.dat");
    fs::rename(input, &ambiguous).unwrap();

    let unknown = run(
        root,
        &["convert", "project.dat", "--to", "tesseract", "-o", "out"],
    );
    assert_eq!(unknown.status.code(), Some(1), "{unknown:?}");
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("--from"));
    assert!(!root.join("out").exists());

    let checked = success(run(
        root,
        &[
            "convert",
            "project.dat",
            "--from",
            "premiere",
            "--to",
            "tesseract",
            "-o",
            "out",
            "--check",
        ],
    ));
    assert_eq!(checked, "Premiere to Tesseract conversion is valid.\n");
    assert!(!root.join("out").exists());

    fs::rename(ambiguous, root.join("project.PRPROJ")).unwrap();
    let inferred = success(run(
        root,
        &[
            "convert",
            "project.PRPROJ",
            "--to",
            "tesseract",
            "-o",
            "out",
            "--check",
        ],
    ));
    assert_eq!(inferred, checked);
    assert!(!root.join("out").exists());
}

#[test]
fn unsupported_direction_and_sequence_reject_before_publication() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (args, message) in [
        (
            vec!["convert", "missing.prproj", "--to", "premiere", "-o", "out"],
            "must differ",
        ),
        (
            vec![
                "convert",
                "missing.tsrct",
                "--to",
                "premiere",
                "-o",
                "out",
                "--sequence",
                "id",
            ],
            "--sequence is only supported",
        ),
    ] {
        let output = run(root, &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
        assert!(!root.join("out").exists());
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_return_errors_before_publication() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, process::Command};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root, &one_second());
    let imported = root.join("tesseract_output");
    premiere_to_tesseract(&input, &imported, None, false).unwrap();
    let saved = imported.join("project.tsrct");
    let native_dir = root.join("native");
    tesseract_to_premiere(&saved, &native_dir, false).unwrap();
    for (from, to, input) in [
        ("premiere", "tesseract", native_dir.join("project.prproj")),
        ("tesseract", "premiere", saved),
    ] {
        for invalid_input in [false, true] {
            let invalid = root.join(OsString::from_vec(
                format!("{from}-bad-")
                    .into_bytes()
                    .into_iter()
                    .chain([0xff])
                    .collect(),
            ));
            if invalid_input && cfg!(target_os = "linux") {
                fs::copy(&input, &invalid).unwrap();
            }
            for check in [false, true] {
                let valid_output = root.join(format!("{from}-out-{check}"));
                let output = if invalid_input {
                    &valid_output
                } else {
                    &invalid
                };
                let mut cli = Command::new(env!("CARGO_BIN_EXE_tsrct-conv"));
                cli.arg("convert")
                    .arg(if invalid_input { &invalid } else { &input })
                    .arg("--from")
                    .arg(from)
                    .arg("--to")
                    .arg(to)
                    .arg("-o")
                    .arg(output);
                if check {
                    cli.arg("--check");
                }
                let result = cli.output().unwrap();
                assert_eq!(result.status.code(), Some(1), "{result:?}");
                assert!(String::from_utf8_lossy(&result.stderr).contains("UTF-8"));
                assert!(result.stdout.is_empty());
                assert!(!output.exists());
            }
        }
    }
}

fn hybrid_archive(root: &Path) {
    let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
    let rect: Value = serde_json::from_str(include_str!(
        "../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
    ))
    .unwrap();
    let mut overlay = rect["composition"]["layers"][0].clone();
    overlay["id"] = json!(10);
    overlay["activeRange"] = json!({"start":200,"duration":600});
    overlay["rect"]["size"] = json!([120, 80]);
    overlay["rect"]["strokeEnabled"] = json!(true);
    overlay["rect"]["strokeWidth"] = json!(4);
    // Plain filled/stroked rectangles are now native Premiere graphics.
    // Glow still needs an editable AEP scope.
    overlay["effects"] = json!([{"id":100,"effect":{
        "type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5
    }}]);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, overlay);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({
                "type":"Audio", "id":20, "name":"Native sound",
                "playback": {
                    "type": "windowed", "inputRange": {"start": 100, "duration": 200},
                    "mapping": {"type": "linear", "input": {"start": 100, "duration": 200},
                        "output": {"start": 0, "duration": 200}},
                    "inputOffsetMs": 0
                },
                "sourceRange":{"start":0,"duration":200},
                "sourceIntrinsicDuration":200, "volume":0.5, "source":{"assetId":"music"}
            }),
        );
    fs::write(root.join("source.mp4"), MEDIA).unwrap();
    fs::write(
        root.join("sound.wav"),
        include_bytes!("../../../crates/premiere_file/tests/fixtures/audio-mono.wav"),
    )
    .unwrap();
    tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&document).unwrap(),
    )
    .unwrap()
    .add_asset(
        "premiere-video-1",
        root.join("source.mp4"),
        tesseract_file::AssetKind::Video,
    )
    .unwrap()
    .add_asset(
        "music",
        root.join("sound.wav"),
        tesseract_file::AssetKind::Audio,
    )
    .unwrap()
    .write(root.join("hybrid.tsrct"))
    .unwrap();
}

#[test]
fn premiere_hybrid_cli_creates_linked_aep_and_reimports_editable_content() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    hybrid_archive(root);
    let args = [
        "convert",
        "hybrid.tsrct",
        "--to",
        "premiere",
        "-o",
        "package",
    ];
    let checked = run(root, &[&args[..], &["--check"]].concat());
    assert!(checked.status.success(), "{checked:?}");
    assert!(!root.join("package").exists());
    let saved = run(root, &args);
    assert!(saved.status.success(), "{saved:?}");
    let aep = root.join("package/media/ae-0001/compositions.aep");
    assert!(aep.is_file(), "ordinary CLI must publish the linked AEP");
    assert_eq!(checked.stderr, saved.stderr);
    assert!(String::from_utf8_lossy(&saved.stderr).contains("Editable linked-AEP package"));
    let project = root.join("package/project.prproj");
    let bytes = fs::read(&project).unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("./media/ae-0001/compositions.aep"));
    assert!(!xml.contains(".conversion-"));
    assert!(!run(root, &args).status.success());
    assert_eq!(fs::read(&project).unwrap(), bytes);

    let import = [
        "convert",
        "package/project.prproj",
        "--to",
        "tesseract",
        "-o",
        "imported",
    ];
    let checked = run(root, &[&import[..], &["--check"]].concat());
    assert!(checked.status.success(), "{checked:?}");
    assert!(!root.join("imported").exists());
    let imported = run(root, &import);
    assert!(imported.status.success(), "{imported:?}");
    assert_eq!(checked.stderr, imported.stderr);
    let archive = tesseract_file::TesseractFile::open(root.join("imported/project.tsrct")).unwrap();
    fn has_editable_rect(layers: &[fx_schema::Layer]) -> bool {
        layers.iter().any(|layer| {
            matches!(layer.data(), fx_schema::LayerData::Rect(rect) if rect.rect.size == [120.0, 80.0])
                || layer.child_layers().is_some_and(has_editable_rect)
        })
    }
    assert!(has_editable_rect(archive.project().composition().layers()));
    fn audio_count(layers: &[fx_schema::Layer]) -> usize {
        layers
            .iter()
            .map(|layer| {
                let own = if let fx_schema::LayerData::Audio(audio) = layer.data() {
                    assert_eq!(audio.volume.as_f64(), 0.5);
                    1
                } else {
                    0
                };
                own + layer.child_layers().map(audio_count).unwrap_or(0)
            })
            .sum()
    }
    assert_eq!(audio_count(archive.project().composition().layers()), 1);
    assert_eq!(
        fs::read(root.join("package/media/sound.wav")).unwrap(),
        fs::read(root.join("sound.wav")).unwrap()
    );
    fs::remove_file(aep).unwrap();
    let missing = run(
        root,
        &[
            "convert",
            "package/project.prproj",
            "--to",
            "tesseract",
            "-o",
            "missing",
        ],
    );
    // A missing linked source omits its picture with a diagnostic, never
    // silently; the native sound still converts.
    assert!(missing.status.success(), "{missing:?}");
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("linked After Effects project: missing media"),
        "{stderr}"
    );
    let archive = tesseract_file::TesseractFile::open(root.join("missing/project.tsrct")).unwrap();
    assert!(!has_editable_rect(archive.project().composition().layers()));
    assert_eq!(audio_count(archive.project().composition().layers()), 1);
}

#[test]
fn premiere_hybrid_cli_retains_native_output_for_fractional_clock() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    hybrid_archive(root);
    let args = [
        "convert",
        "hybrid.tsrct",
        "--to",
        "premiere",
        "-o",
        "package",
        "--fps",
        "29.97",
    ];
    for check in [true, false] {
        let args = if check {
            [&args[..], &["--check"]].concat()
        } else {
            args.to_vec()
        };
        let output = run(root, &args);
        assert!(output.status.success(), "{output:?}");
        let warnings = String::from_utf8_lossy(&output.stderr);
        assert!(warnings.contains("not exact in linked AEP scopes"));
        assert!(warnings.contains("retained the native Premiere result"));
        assert_eq!(root.join("package/project.prproj").exists(), !check);
        assert!(!root.join("package/media/ae-0001").exists());
    }
}

fn one_second() -> String {
    XML.replace("1270080000000", "254016000000")
        .replace("2540160000000", "254016000000")
}

fn run(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tsrct-conv"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn aep_success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("[AE-COMPOSITION-SETTINGS]"));
    String::from_utf8(output.stdout).unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
}

fn fixture(directory: &Path, xml: &str) -> std::path::PathBuf {
    fs::create_dir_all(directory.join("media")).unwrap();
    fs::write(directory.join("media/source.mp4"), MEDIA).unwrap();
    let native = directory.join("project.prproj");
    write_prproj(&native, xml);
    native
}

fn write_prproj(path: &Path, xml: &str) {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(xml.as_bytes()).unwrap();
    fs::write(path, encoder.finish().unwrap()).unwrap();
}

/// Default public routing must preserve deliberately retained native effects,
/// not replace their picture solely because their approximation was reported.
#[test]
fn premiere_hybrid_retained_lens_alpha_glow_native_effects() {
    fn field<'a>(record: &'a str, tag: &str) -> &'a str {
        record
            .split_once(&format!("<{tag}>"))
            .unwrap()
            .1
            .split_once(&format!("</{tag}>"))
            .unwrap()
            .0
    }
    fn attribute<'a>(record: &'a str, name: &str) -> &'a str {
        record
            .split_once(&format!("{name}=\""))
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0
    }
    fn component<'a>(xml: &'a str, name: &str) -> &'a str {
        xml.split("<VideoFilterComponent ")
            .skip(1)
            .map(|part| part.split_once("</VideoFilterComponent>").unwrap().0)
            .find(|part| part.contains(&format!("<MatchName>{name}</MatchName>")))
            .unwrap()
    }
    fn parameter<'a>(xml: &'a str, component: &str, index: usize) -> &'a str {
        let reference = component.split("<Param ").nth(index + 1).unwrap();
        let id = attribute(reference, "ObjectRef");
        xml.split_once(&format!("<VideoComponentParam ObjectID=\"{id}\""))
            .unwrap()
            .1
            .split_once("</VideoComponentParam>")
            .unwrap()
            .0
    }
    for alpha in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
        document["composition"]["layers"][0]["effects"] = if alpha {
            json!([{"id":9,"effect":{"type":"outerGlow","color":[0.2,0.4,0.8,0.6],"size":42.4,"spread":0.2,"range":0.8,"blendMode":"screen"}}])
        } else {
            json!([{"id":9,"effect":{"type":"lensDistortion","amount":-0.6,"centerX":0.5,"centerY":0.5}}])
        };
        if alpha {
            document["composition"]["layers"][0]["masks"] =
                json!([{"id":1,"mode":"add","layer":2,"feather":[0.0,0.0],"opacity":1.0}]);
            let mut canvas = document["composition"]["layers"][1].clone();
            canvas["id"] = json!(3);
            document["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .push(canvas);
            document["composition"]["layers"][1]["rect"]["position"] = json!([480.0, 270.0]);
            document["composition"]["layers"][1]["rect"]["size"] = json!([1440.0, 810.0]);
        } else {
            document["composition"]["dynamics"] = json!({"entries":[{
                "target":{"kind":"effectProperty","effectId":9,"paramName":"amount"},
                "animator":{"type":"keyframes","enabled":true,"keyframes":[
                    {"id":"lens-cli-a","layerTime":0,"value":{"type":"float","value":-0.6},"easing":{"type":"linear"}},
                    {"id":"lens-cli-b","layerTime":500,"value":{"type":"float","value":0.2},"easing":{"type":"linear"}}
                ]}
            }]});
        }
        fs::write(root.join("source.mp4"), MEDIA).unwrap();
        tesseract_file::TesseractFileBuilder::from_project_json(
            &serde_json::to_vec(&document).unwrap(),
        )
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mp4"),
            tesseract_file::AssetKind::Video,
        )
        .unwrap()
        .write(root.join("edited.tsrct"))
        .unwrap();
        let saved = run(
            root,
            &[
                "convert",
                "edited.tsrct",
                "--to",
                "premiere",
                "-o",
                "native",
            ],
        );
        assert!(saved.status.success(), "{saved:?}");
        let diagnostics = String::from_utf8_lossy(&saved.stderr);
        assert!(
            diagnostics.contains(if alpha {
                "Alpha Glow"
            } else {
                "deliberate slider normalization"
            }),
            "{diagnostics}"
        );
        assert!(
            !diagnostics.contains("Editable linked-AEP package"),
            "{diagnostics}"
        );
        let mut xml = String::new();
        flate2::read::GzDecoder::new(fs::File::open(root.join("native/project.prproj")).unwrap())
            .read_to_string(&mut xml)
            .unwrap();
        assert!(!xml.contains(".aep"), "{xml}");
        let effect = component(
            &xml,
            if alpha {
                "AE.ADBE Alpha Glow"
            } else {
                "PR.ADBE Lens Distortion"
            },
        );
        let first = parameter(&xml, effect, 0);
        assert_eq!(
            field(first, "StartKeyframe")
                .split(',')
                .nth(1)
                .unwrap()
                .parse::<f64>()
                .unwrap(),
            if alpha { 42.0 } else { 60.0 }
        );
        if alpha {
            assert_eq!(
                field(parameter(&xml, effect, 1), "StartKeyframe")
                    .split(',')
                    .nth(1),
                Some("153")
            );
            let rgb = (0xff00_u64 << 48) | (0x3300_u64 << 32) | (0x6600_u64 << 16) | 0xcc00;
            for index in [2, 3] {
                assert_eq!(
                    field(parameter(&xml, effect, index), "StartKeyframe")
                        .split(',')
                        .nth(1)
                        .unwrap()
                        .parse::<u64>()
                        .unwrap(),
                    rgb
                );
            }
            assert_eq!(
                field(parameter(&xml, effect, 4), "StartKeyframe")
                    .split(',')
                    .nth(1),
                Some("false")
            );
            assert_eq!(
                field(parameter(&xml, effect, 5), "StartKeyframe")
                    .split(',')
                    .nth(1),
                Some("true")
            );
            let crop = component(&xml, "AE.ADBE AECrop");
            let index = |component: &str| -> usize {
                let id = attribute(component, "ObjectID");
                let reference = xml
                    .split("<Component ")
                    .skip(1)
                    .find(|part| {
                        part.split_once('>')
                            .unwrap()
                            .0
                            .contains(&format!("ObjectRef=\"{id}\""))
                    })
                    .unwrap();
                attribute(reference, "Index").parse().unwrap()
            };
            assert!(index(crop) > index(effect), "Crop must apply before Glow");
            assert_eq!(
                field(parameter(&xml, crop, 0), "StartKeyframe")
                    .split(',')
                    .nth(1)
                    .unwrap()
                    .parse::<f64>()
                    .unwrap(),
                25.0
            );
        } else {
            let keys: Vec<_> = field(first, "Keyframes")
                .split_terminator(';')
                .map(|key| {
                    let mut fields = key.split(',');
                    (
                        fields.next().unwrap().parse::<i64>().unwrap(),
                        fields.next().unwrap().parse::<f64>().unwrap(),
                    )
                })
                .collect();
            assert_eq!(keys, [(0, 60.0), (127_008_000_000, -20.0)]);
        }
    }
}

#[test]
fn levels_channel_selectors_use_the_native_premiere_cli_route() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
    let effects = json!([
        {"id": 1, "enabled": true, "effect": {"type": "shiftChannels", "takeRedFrom": "fullOff", "takeGreenFrom": "fullOn", "takeBlueFrom": "blue"}},
        {"id": 2, "enabled": true, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.5, "outputBlack": 0.0, "outputWhite": 255.0}}
    ]);
    document["composition"]["layers"][0]["effects"] = effects.clone();
    fs::write(root.join("source.mp4"), MEDIA).unwrap();
    tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&document).unwrap(),
    )
    .unwrap()
    .add_asset(
        "premiere-video-1",
        root.join("source.mp4"),
        tesseract_file::AssetKind::Video,
    )
    .unwrap()
    .write(root.join("edited.tsrct"))
    .unwrap();
    let args = [
        "convert",
        "edited.tsrct",
        "--to",
        "premiere",
        "-o",
        "native",
    ];
    let checked = run(root, &[&args[..], &["--check"]].concat());
    assert!(checked.status.success(), "{checked:?}");
    assert!(!root.join("native").exists());
    let saved = run(root, &args);
    assert!(saved.status.success(), "{saved:?}");
    assert_eq!(checked.stderr, saved.stderr);
    assert!(saved.stderr.is_empty(), "{saved:?}");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(fs::File::open(root.join("native/project.prproj")).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(
        xml.matches("<MatchName>PR.ADBE Levels</MatchName>").count(),
        2
    );
    assert!(!xml.contains("AfterEffects"));
    let imported = run(
        root,
        &[
            "convert",
            "native/project.prproj",
            "--to",
            "tesseract",
            "-o",
            "imported",
        ],
    );
    assert!(imported.status.success(), "{imported:?}");
    let archive = tesseract_file::TesseractFile::open(root.join("imported/project.tsrct")).unwrap();
    assert_eq!(
        archive.project_json().unwrap()["composition"]["layers"][0]["effects"],
        effects
    );
}

#[path = "../../../crates/premiere_file/tests/support/numbered_sequence_samples.rs"]
mod numbered_sequence_samples;

#[test]
fn numbered_sequence_samples_public_export_keeps_original_stills() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = numbered_sequence_samples::import(root);
    let mut archive = tesseract_file::TesseractFile::open(path).unwrap();
    assert_eq!(archive.metadata().assets.len(), 6);
    for index in 0..6 {
        let mut bytes = Vec::new();
        archive
            .asset(&format!("premiere-video-1-frame-{index}"))
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(
            bytes,
            fs::read(root.join(format!("frame{index:03}.png"))).unwrap()
        );
    }
    let mut document = archive.project_json().unwrap();
    numbered_sequence_samples::swap(&mut document);
    let edit = root.join("edited.json");
    fs::write(&edit, serde_json::to_vec(&document).unwrap()).unwrap();
    archive.commit_project_json(&edit).unwrap();
    archive.save_as(root.join("edited.tsrct")).unwrap();
    let output = run(
        root,
        &[
            "convert",
            "edited.tsrct",
            "--to",
            "premiere",
            "-o",
            "exported",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(fs::File::open(root.join("exported/project.prproj")).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    assert!(
        !xml.contains(".aep"),
        "ordinary stills must not seed AE replacement"
    );
    assert!(!xml.contains(".exr"));
    assert!(xml.contains(".png"));
    assert!(xml.contains("<FrameRect>0,0,640,360</FrameRect>"));
    fn field<'a>(record: &'a str, tag: &str) -> Option<&'a str> {
        let (_, value) = record.split_once(&format!("<{tag}>"))?;
        Some(value.split_once(&format!("</{tag}>"))?.0)
    }
    fn reference<'a>(record: &'a str, tag: &str, attribute: &str) -> &'a str {
        let element = record
            .split_once(&format!("<{tag} "))
            .unwrap()
            .1
            .split_once('>')
            .unwrap()
            .0;
        element
            .split_once(&format!("{attribute}=\""))
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0
    }
    fn record<'a>(xml: &'a str, tag: &str, attribute: &str, id: &str) -> &'a str {
        let start = xml.find(&format!("<{tag} {attribute}=\"{id}\"")).unwrap();
        let end = start + xml[start..].find(&format!("</{tag}>")).unwrap() + tag.len() + 3;
        &xml[start..end]
    }
    #[derive(Debug, PartialEq)]
    struct NativeStill {
        start: i64,
        end: i64,
        frame: usize,
        extent: String,
        motion: std::collections::BTreeMap<u32, String>,
        default_opacity: bool,
    }
    fn native_stills(xml: &str, root: &Path) -> Vec<NativeStill> {
        let mut stills = Vec::new();
        for item in xml.split("<VideoClipTrackItem ").skip(1) {
            let item = item.split_once("</VideoClipTrackItem>").unwrap().0;
            let subclip = record(
                xml,
                "SubClip",
                "ObjectID",
                reference(item, "SubClip", "ObjectRef"),
            );
            let clip = record(
                xml,
                "VideoClip",
                "ObjectID",
                reference(subclip, "Clip", "ObjectRef"),
            );
            let source = record(
                xml,
                "VideoMediaSource",
                "ObjectID",
                reference(clip, "Source", "ObjectRef"),
            );
            let media = record(
                xml,
                "Media",
                "ObjectUID",
                reference(source, "Media", "ObjectURef"),
            );
            let stream = record(
                xml,
                "VideoStream",
                "ObjectID",
                reference(media, "VideoStream", "ObjectRef"),
            );
            assert_eq!(field(stream, "IsStill"), Some("true"));
            let bytes = fs::read(
                root.join("exported")
                    .join(field(media, "RelativePath").unwrap()),
            )
            .unwrap();
            let frame = (0..6)
                .find(|index| bytes == fs::read(root.join(format!("frame{index:03}.png"))).unwrap())
                .unwrap();
            let chain = record(
                xml,
                "VideoComponentChain",
                "ObjectID",
                reference(item, "Components", "ObjectRef"),
            );
            assert_eq!(chain.matches("<Component ").count(), 1);
            let motion = record(
                xml,
                "VideoFilterComponent",
                "ObjectID",
                reference(chain, "Component", "ObjectRef"),
            );
            assert_eq!(field(motion, "MatchName"), Some("AE.ADBE Motion"));
            assert_eq!(field(motion, "Bypass"), Some("false"));
            let mut controls = std::collections::BTreeMap::new();
            for parameter in motion.split("<Param ").skip(1) {
                let id = parameter
                    .split_once("ObjectRef=\"")
                    .unwrap()
                    .1
                    .split_once('"')
                    .unwrap()
                    .0;
                let tag = if xml.contains(&format!("<PointComponentParam ObjectID=\"{id}\"")) {
                    "PointComponentParam"
                } else {
                    "VideoComponentParam"
                };
                let parameter = record(xml, tag, "ObjectID", id);
                assert!(field(parameter, "Keyframes").is_none());
                controls.insert(
                    field(parameter, "ParameterID").unwrap().parse().unwrap(),
                    field(parameter, "StartKeyframe")
                        .unwrap()
                        .split(',')
                        .nth(1)
                        .unwrap()
                        .to_owned(),
                );
            }
            stills.push(NativeStill {
                start: field(item, "Start").unwrap_or("0").parse().unwrap(),
                end: field(item, "End").unwrap().parse().unwrap(),
                frame,
                extent: field(stream, "FrameRect").unwrap().to_owned(),
                motion: controls,
                default_opacity: field(chain, "DefaultOpacity") == Some("true"),
            });
        }
        stills.sort_by_key(|still| still.start);
        stills
    }
    let frame_ticks = 8_467_200_000;
    let expected: Vec<_> = [(0, 2, 3), (2, 3, 4), (3, 5, 1), (5, 6, 2)]
        .into_iter()
        .map(|(start, end, frame)| NativeStill {
            start: start * frame_ticks,
            end: end * frame_ticks,
            frame,
            extent: "0,0,640,360".into(),
            motion: [
                (1, "0.5:0.5"),
                (2, "50"),
                (3, "50"),
                (4, "true"),
                (5, "0"),
                (6, "0.5:0.5"),
                (7, "0."),
            ]
            .into_iter()
            .map(|(key, value)| (key, value.to_owned()))
            .collect(),
            default_opacity: true,
        })
        .collect();
    assert_eq!(native_stills(&xml, root), expected);
    // Negative controls exercise the same reference traversal, not independent
    // range/asset sets: wrong published bytes or Motion must fail equality.
    let wrong_motion = xml.replacen(",50,", ",25,", 1);
    assert_ne!(wrong_motion, xml);
    assert_ne!(native_stills(&wrong_motion, root), expected);
    let first_media = xml
        .split("<RelativePath>")
        .skip(1)
        .find_map(|rest| {
            let path = rest.split_once("</RelativePath>").unwrap().0;
            path.ends_with(".png").then_some(path)
        })
        .unwrap();
    let wrong_image = xml.replace(
        &format!("<RelativePath>{first_media}</RelativePath>"),
        "<RelativePath>../frame000.png</RelativePath>",
    );
    assert_ne!(native_stills(&wrong_image, root), expected);

    // A genuine off-grid picture change must still enter conservative hybrid
    // routing; the new typed normalization is not a blanket warning bypass.
    let group = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Group" && layer["playback"]["inputRange"]["start"] == 100)
        .unwrap();
    group["playback"]["inputRange"]["start"] = 90.into();
    group["playback"]["mapping"]["input"]["start"] = 90.into();
    fs::write(&edit, serde_json::to_vec(&document).unwrap()).unwrap();
    archive.commit_project_json(&edit).unwrap();
    archive.save_as(root.join("off-grid.tsrct")).unwrap();
    let changed = run(
        root,
        &[
            "convert",
            "off-grid.tsrct",
            "--to",
            "premiere",
            "-o",
            "off-grid",
        ],
    );
    assert!(changed.status.success(), "{changed:?}");
    let diagnostics = String::from_utf8_lossy(&changed.stderr);
    assert!(
        diagnostics.contains("Editable linked-AEP package")
            || diagnostics.contains("HYBRID-NATIVE-RETAINED"),
        "{diagnostics}"
    );
}

#[test]
fn premiere_hybrid_noise_keeps_current_native_stack_and_failed_noise_falls_back() {
    fn field<'a>(record: &'a str, tag: &str) -> &'a str {
        record
            .split_once(&format!("<{tag}>"))
            .unwrap()
            .1
            .split_once(&format!("</{tag}>"))
            .unwrap()
            .0
    }
    fn attribute<'a>(record: &'a str, name: &str) -> &'a str {
        record
            .split_once(&format!("{name}=\""))
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0
    }
    fn parameter<'a>(xml: &'a str, component: &str, index: usize) -> &'a str {
        let reference = component.split("<Param ").nth(index + 1).unwrap();
        let id = attribute(reference, "ObjectRef");
        xml.split_once(&format!("<VideoComponentParam ObjectID=\"{id}\""))
            .unwrap()
            .1
            .split_once("</VideoComponentParam>")
            .unwrap()
            .0
    }

    for (unsupported, strength_keys) in [
        (false, None),
        (true, None),
        (false, Some([0.0, 4.0, 12.0])),
        (false, Some([2.0, 8.0, 16.0])),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
        document["composition"]["layers"][0]["effects"] = json!([
            {"id":9,"effect":{"type":"grain","amount":if unsupported {41.0} else {12.0},"size":2.0,"softness":0.7,"aspectRatio":1.3,"seed":17.0}},
            {"id":10,"enabled":false,"effect":{"type":"grain","amount":2.0,"size":1.0,"softness":0.0,"aspectRatio":1.0,"seed":0.0}}
        ]);
        if let Some(values) = strength_keys {
            document["composition"]["layers"][0]["effects"][0]["effect"]["amount"] =
                json!(values[0]);
            // Independently edited current strength, not cached source values.
            document["composition"]["dynamics"] = json!({"entries":[{
                "target":{"kind":"effectProperty","effectId":9,"paramName":"intensity"},
                "animator":{"type":"keyframes","enabled":true,"keyframes":[
                    {"id":"noise-a","layerTime":0,"value":{"type":"float","value":values[0]},"easing":{"type":"linear"}},
                    {"id":"noise-b","layerTime":500,"value":{"type":"float","value":values[1]},"easing":{"type":"hold"}},
                    {"id":"noise-c","layerTime":1000,"value":{"type":"float","value":values[2]},"easing":{"type":"hold"}}
                ]}
            }]});
        }
        fs::write(root.join("source.mp4"), MEDIA).unwrap();
        tesseract_file::TesseractFileBuilder::from_project_json(
            &serde_json::to_vec(&document).unwrap(),
        )
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mp4"),
            tesseract_file::AssetKind::Video,
        )
        .unwrap()
        .write(root.join("edited.tsrct"))
        .unwrap();
        let saved = run(
            root,
            &[
                "convert",
                "edited.tsrct",
                "--to",
                "premiere",
                "-o",
                "native",
            ],
        );
        assert!(saved.status.success(), "{saved:?}");
        let mut xml = String::new();
        flate2::read::GzDecoder::new(fs::File::open(root.join("native/project.prproj")).unwrap())
            .read_to_string(&mut xml)
            .unwrap();
        if unsupported {
            assert!(
                xml.contains(".aep"),
                "failed Grain must still seed hybrid fallback: {xml}"
            );
            continue;
        }
        assert!(!xml.contains(".aep"), "{xml}");
        let mut components: Vec<_> = xml
            .split("<VideoFilterComponent ")
            .skip(1)
            .map(|part| part.split_once("</VideoFilterComponent>").unwrap().0)
            .filter(|part| part.contains("<MatchName>AE.ADBE Noise2</MatchName>"))
            .collect();
        assert_eq!(components.len(), 2);
        let index = |component: &str| -> usize {
            let id = attribute(component, "ObjectID");
            let reference = xml
                .split("<Component ")
                .skip(1)
                .find(|part| {
                    part.split_once('>')
                        .unwrap()
                        .0
                        .contains(&format!("ObjectRef=\"{id}\""))
                })
                .unwrap();
            attribute(reference, "Index").parse().unwrap()
        };
        components.sort_by_key(|component| std::cmp::Reverse(index(component)));
        for (component, expected, bypass) in [
            (
                components[0],
                strength_keys.map_or(30.0, |values| values[0] / 0.4),
                "false",
            ),
            (components[1], 5.0, "true"),
        ] {
            let value: f64 = field(parameter(&xml, component, 0), "StartKeyframe")
                .split(',')
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(value, expected);
            assert_eq!(field(component, "Bypass"), bypass);
            let amount = parameter(&xml, component, 0);
            let active_keys = strength_keys.filter(|_| bypass == "false");
            assert_eq!(
                field(amount, "IsTimeVarying"),
                if active_keys.is_some() {
                    "true"
                } else {
                    "false"
                }
            );
            if let Some(values) = active_keys {
                let keys: Vec<_> = field(amount, "Keyframes")
                    .split_terminator(';')
                    .map(|key| {
                        let fields: Vec<_> = key.split(',').collect();
                        (
                            fields[0].parse::<i64>().unwrap(),
                            fields[1].parse::<f64>().unwrap(),
                            fields[2].parse::<u8>().unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(
                    keys,
                    [
                        (0, values[0] / 0.4, 4),
                        (127_008_000_000, values[1] / 0.4, 4),
                        (254_016_000_000, values[2] / 0.4, 0)
                    ]
                );
            } else {
                assert!(!amount.contains("<Keyframes>"));
            }
            for index in [1, 2] {
                assert_eq!(
                    field(parameter(&xml, component, index), "IsTimeVarying"),
                    "false"
                );
            }
        }
    }
}

/// The source-derived dual-key case must exercise real fallback and inspect its
/// native controls, not merely prove that some linked AEP was published.
#[test]
fn premiere_hybrid_noise_modern_strength_seed_keys_reach_linked_grain() {
    use aftereffects_file::{properties, rifx::Chunk, structure};

    fn keyed_record(xml: &str, id: u32, values: [f64; 2]) -> String {
        let start = xml
            .find(&format!("<VideoComponentParam ObjectID=\"{id}\""))
            .unwrap();
        let end = start + xml[start..].find("</VideoComponentParam>").unwrap();
        let record = &xml[start..end];
        assert!(!record.contains("<Keyframes>"));
        // Half-second knots within the one-second media harness. Only the exact
        // native Seed736 / Intensity737 records are changed; source bytes stay pinned.
        let keys = format!(
            "<Keyframes>0,{},0,0,0,0,0,0;127008000000,{},4,0,0,0,0,0;</Keyframes>",
            values[0], values[1]
        );
        format!("{}{}{}{}", &xml[..start], record, keys, &xml[end..])
    }
    fn named(chunk: &Chunk, name: &str) -> bool {
        chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .unwrap()
                .split(|byte| *byte == 0)
                .next()
                == Some(name.as_bytes())
    }
    fn grain_groups<'a>(chunks: &'a [Chunk], result: &mut Vec<&'a [Chunk]>) {
        for pair in chunks.windows(2) {
            if named(&pair[0], "VISINF Grain Implant") && pair[1].list_kind() == Some(*b"sspc") {
                let group = pair[1]
                    .children()
                    .unwrap()
                    .iter()
                    .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                    .unwrap();
                result.push(group.children().unwrap());
            }
        }
        for chunk in chunks {
            if let Some(children) = chunk.children() {
                grain_groups(children, result);
            }
        }
    }
    fn control(group: &[Chunk], name: &str) -> properties::NumericProperty {
        let pair = group.windows(2).find(|pair| named(&pair[0], name)).unwrap();
        assert_eq!(pair[1].list_kind(), Some(*b"tdbs"));
        properties::read_numeric(pair[1].children().unwrap()).unwrap()
    }
    fn enabled(group: &[Chunk]) -> bool {
        let flags = group
            .iter()
            .find(|chunk| chunk.id() == *b"tdsb")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(flags.len(), 4);
        flags[3] & 1 != 0
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let records =
        include_str!("../../../crates/premiere_file/tests/fixtures/noise-native-records.xml");
    let body = records
        .split_once("<PremiereData>")
        .unwrap()
        .1
        .split_once("</PremiereData>")
        .unwrap()
        .0;
    let body = keyed_record(&keyed_record(body, 737, [10.0, 30.0]), 736, [3.0, 7.0]);
    let xml = one_second().replace(
        "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
        &body.replacen("<VideoComponentChain ObjectID=\"388\"", "<VideoComponentChain ObjectID=\"4\"", 1),
    );
    fixture(root, &xml);
    let imported = run(
        root,
        &[
            "convert",
            "project.prproj",
            "--to",
            "tesseract",
            "--sequence",
            "sequence-1",
            "-o",
            "imported",
        ],
    );
    assert!(imported.status.success(), "{imported:?}");
    let mut archive =
        tesseract_file::TesseractFile::open(root.join("imported/project.tsrct")).unwrap();
    let original = archive.project_json().unwrap();
    let layers = original["composition"]["layers"].as_array().unwrap();
    let owner = layers
        .iter()
        .position(|layer| {
            layer["effects"]
                .as_array()
                .is_some_and(|effects| effects.len() == 2)
        })
        .unwrap();
    let effects = &layers[owner]["effects"];
    assert_eq!(effects[0]["effect"]["amount"], json!(4.0));
    assert_eq!(effects[0]["effect"]["seed"], json!(3.0));
    assert_eq!(effects[1]["effect"]["amount"], json!(2.0));
    let modern_id = effects[0]["id"].clone();
    for (parameter, expected) in [("intensity", [4.0, 12.0]), ("seed", [3.0, 7.0])] {
        let entries = original["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let entry = entries
            .iter()
            .find(|entry| {
                entry["target"]["effectId"] == modern_id
                    && entry["target"]["paramName"] == parameter
            })
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        for (index, expected) in expected.into_iter().enumerate() {
            assert_eq!(keys[index]["value"]["value"], json!(expected));
            assert_eq!(keys[index]["layerTime"], json!(index * 500));
        }
    }
    for edited in [false, true] {
        let mut document = original.clone();
        let (strength, seed, legacy) = if edited {
            ([6.0, 14.0], [13.0, 29.0], 9.0)
        } else {
            ([4.0, 12.0], [3.0, 7.0], 2.0)
        };
        if edited {
            let effects = &mut document["composition"]["layers"][owner]["effects"];
            effects[0]["effect"]["amount"] = json!(strength[0]);
            effects[0]["effect"]["seed"] = json!(seed[0]);
            effects[0]["effect"]["size"] = json!(2.5);
            effects[1]["effect"]["amount"] = json!(legacy);
            effects[1]["enabled"] = json!(false);
            for entry in document["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
            {
                if entry["target"]["effectId"] == modern_id {
                    let values = match entry["target"]["paramName"].as_str().unwrap() {
                        "intensity" => strength,
                        "seed" => seed,
                        other => panic!("unexpected modern track {other}"),
                    };
                    for (key, value) in entry["animator"]["keyframes"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .zip(values)
                    {
                        key["value"]["value"] = json!(value);
                    }
                }
            }
        }
        let edit = root.join("edited.json");
        fs::write(&edit, serde_json::to_vec(&document).unwrap()).unwrap();
        archive.commit_project_json(&edit).unwrap();
        let input = if edited {
            "edited.tsrct"
        } else {
            "source.tsrct"
        };
        archive.save_as(root.join(input)).unwrap();
        let out = if edited {
            "edited-native"
        } else {
            "source-native"
        };
        let exported = run(root, &["convert", input, "--to", "premiere", "-o", out]);
        assert!(exported.status.success(), "{exported:?}");
        let diagnostics = String::from_utf8_lossy(&exported.stderr);
        assert!(
            !diagnostics.contains("no native animated target"),
            "{diagnostics}"
        );
        let mut xml = String::new();
        flate2::read::GzDecoder::new(
            fs::File::open(root.join(out).join("project.prproj")).unwrap(),
        )
        .read_to_string(&mut xml)
        .unwrap();
        assert!(
            xml.contains("./media/ae-0001/compositions.aep"),
            "seed keys must trigger genuine fallback: {xml}"
        );
        let native = structure::read_project(
            &fs::read(root.join(out).join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        let mut groups = Vec::new();
        for item in &native.items {
            if let structure::ItemKind::Composition(composition) = &item.kind {
                for layer in &composition.layers {
                    grain_groups(&layer.content, &mut groups);
                }
            }
        }
        assert_eq!(
            groups.len(),
            2,
            "Modern and Legacy remain in native order on fallback"
        );
        assert!(enabled(groups[0]));
        assert_eq!(enabled(groups[1]), !edited);
        for (name, expected) in [
            ("VISINF Grain Implant-0008", strength),
            ("VISINF Grain Implant-0013", seed),
        ] {
            let property = control(groups[0], name);
            assert!(property.animated);
            assert_eq!(
                property
                    .keyframes
                    .iter()
                    .map(|key| (key.time_secs, key.values[0]))
                    .collect::<Vec<_>>(),
                [(0.0, expected[0]), (0.5, expected[1])]
            );
        }
        assert_eq!(
            control(groups[0], "VISINF Grain Implant-0007").values,
            [if edited { 2.5 } else { 1.0 }]
        );
        let legacy_strength = control(groups[1], "VISINF Grain Implant-0008");
        assert_eq!(legacy_strength.values, [legacy]);
        assert!(legacy_strength.keyframes.is_empty());
    }
}

#[test]
fn warp_public_native_graphs_keep_current_edits_through_aep_package() {
    fn children(node: &Value) -> impl Iterator<Item = &Value> {
        node.get("composition").into_iter().chain(
            node.get("layers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
    }
    fn named<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
        if node["name"] == name {
            return Some(node);
        }
        children(node).find_map(|child| named(child, name))
    }
    fn id_node(node: &Value, id: u64) -> Option<&Value> {
        if node["id"].as_u64() == Some(id) {
            return Some(node);
        }
        children(node).find_map(|child| id_node(child, id))
    }
    fn multiply(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
        [
            a[0] * b[0] + a[1] * b[2],
            a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2],
            a[2] * b[1] + a[3] * b[3],
        ]
    }
    // Follow exporter/importer container normalization down to the actual
    // pre-Mirror image rather than assuming the named outer Group owns rotation.
    fn source_matrix(node: &Value, parent: [f64; 4]) -> Option<[f64; 4]> {
        let t = &node["transform"];
        assert_eq!(t["skew"].as_f64().unwrap_or(0.), 0.);
        let angle = t["rotation"].as_f64().unwrap_or(0.).to_radians();
        let sx = t["scale"][0].as_f64().unwrap_or(100.) / 100.;
        let sy = t["scale"][1].as_f64().unwrap_or(100.) / 100.;
        let matrix = multiply(
            parent,
            [
                angle.cos() * sx,
                -angle.sin() * sy,
                angle.sin() * sx,
                angle.cos() * sy,
            ],
        );
        if node["name"] == "Source before Mirror" {
            return Some(matrix);
        }
        children(node).find_map(|child| source_matrix(child, matrix))
    }
    fn assert_mask_reference(root: &Value, branch: &Value) {
        let masks = branch["masks"]
            .as_array()
            .expect("branch-local clipping mask");
        assert!(!masks.is_empty());
        for mask in masks {
            let guide = id_node(
                root,
                mask["layer"].as_u64().expect("editable guide reference"),
            )
            .expect("mask guide is present");
            assert_eq!(guide["type"], "Shape");
            assert!(!guide["shape"]["path"]["commands"]
                .as_array()
                .expect("editable path")
                .is_empty());
        }
    }

    fn named_path<'a>(node: &'a Value, name: &str) -> Option<Vec<&'a Value>> {
        if node["name"] == name {
            return Some(vec![node]);
        }
        children(node).find_map(|child| {
            let mut path = named_path(child, name)?;
            path.insert(0, node);
            Some(path)
        })
    }
    fn transform_point(node: &Value, point: [f64; 2]) -> Option<[f64; 2]> {
        let t = &node["transform"];
        for key in ["skew", "rotationX", "rotationY"] {
            if t[key].as_f64().unwrap_or(0.) != 0. {
                return None;
            }
        }
        if t["orientation"]
            .as_array()
            .is_some_and(|axes| axes.iter().any(|v| v.as_f64() != Some(0.)))
        {
            return None;
        }
        let angle = t["rotation"].as_f64().unwrap_or(0.).to_radians();
        let p: [f64; 2] = std::array::from_fn(|axis| {
            (point[axis] - t["anchorPoint"][axis].as_f64().unwrap_or(0.))
                * t["scale"][axis].as_f64().unwrap_or(100.)
                / 100.
        });
        Some([
            angle.cos() * p[0] - angle.sin() * p[1] + t["position"][0].as_f64().unwrap_or(0.),
            angle.sin() * p[0] + angle.cos() * p[1] + t["position"][1].as_f64().unwrap_or(0.),
        ])
    }
    // Only a common ancestor can crop BOTH images. Resolve source-local native
    // mask coordinates through their carrier's normalized anchor/position.
    fn finite_canvas_mask(root: &Value) -> Option<(u64, u64)> {
        let reflected = named_path(root, "Mirror reflected half")?;
        let retained = named_path(root, "Mirror retained source half")?;
        for (index, (owner, _)) in reflected
            .iter()
            .zip(&retained)
            .take_while(|(a, b)| std::ptr::eq(**a, **b))
            .enumerate()
        {
            for mask in owner
                .get("masks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if mask["mode"] != "add"
                    || mask["inverted"] != false
                    || mask["opacity"] != 1.0
                    || mask["expansion"] != 0.0
                    || mask["feather"] != json!([0.0, 0.0])
                    || owner["isHidden"].as_bool().unwrap_or(false)
                {
                    continue;
                }
                let guide = id_node(root, mask["layer"].as_u64()?)?;
                if guide["type"] != "Shape"
                    || guide["parent"] != owner["id"]
                    || guide["isHidden"].as_bool().unwrap_or(false)
                    || guide["activeRange"]["start"].as_u64()? != 0
                    || guide["activeRange"]["duration"].as_u64()? < 2000
                    || guide["transform"]["opacity"] != 100.0
                {
                    continue;
                }
                let commands = guide["shape"]["path"]["commands"].as_array()?;
                if !matches!(commands.len(), 5 | 6) || commands.last()?["type"] != "close" {
                    continue;
                }
                let corners = [[0., 0.], [320., 0.], [320., 180.], [0., 180.]];
                let rectangle =
                    commands[..commands.len() - 1]
                        .iter()
                        .enumerate()
                        .all(|(i, command)| {
                            if command["type"] != if i == 0 { "moveTo" } else { "lineTo" } {
                                return false;
                            }
                            let Some(point) = command["x"].as_f64().zip(command["y"].as_f64())
                            else {
                                return false;
                            };
                            let point =
                                transform_point(guide, [point.0, point.1]).and_then(|point| {
                                    reflected[..=index]
                                        .iter()
                                        .rev()
                                        .try_fold(point, |point, node| transform_point(node, point))
                                });
                            // Native path storage quantizes coordinates to float32.
                            point.is_some_and(|point| {
                                (0..2).all(|axis| (point[axis] - corners[i % 4][axis]).abs() < 1e-4)
                            })
                        });
                if rectangle {
                    return Some((owner["id"].as_u64()?, mask["id"].as_u64()?));
                }
            }
        }
        None
    }
    fn remove_mask(node: &mut Value, owner: u64, mask: u64) {
        if node["id"].as_u64() == Some(owner) {
            node["masks"]
                .as_array_mut()
                .unwrap()
                .retain(|m| m["id"].as_u64() != Some(mask));
            return;
        }
        if let Some(comp) = node.get_mut("composition") {
            remove_mask(comp, owner, mask);
        }
        if let Some(layers) = node.get_mut("layers").and_then(Value::as_array_mut) {
            for child in layers {
                remove_mask(child, owner, mask);
            }
        }
    }

    fn edit(node: &mut Value, target: &str, count: &mut usize) {
        if target == "Mirror reflected half" && node["name"] == target {
            node["transform"]["rotation"] = json!(80.0);
            *count += 1;
        }
        if let Some(effects) = node.get_mut("effects").and_then(Value::as_array_mut) {
            for record in effects {
                if record["effect"]["type"] == target {
                    record["effect"][if target == "bulge" {
                        "bulgeHeight"
                    } else {
                        "phase"
                    }] = json!(-0.75);
                    *count += 1;
                }
            }
        }
        if let Some(comp) = node.get_mut("composition") {
            edit(comp, target, count);
        }
        if let Some(layers) = node.get_mut("layers").and_then(Value::as_array_mut) {
            for layer in layers {
                edit(layer, target, count);
            }
        }
    }
    for (composition, target) in [
        ("1", "Mirror reflected half"),
        ("14", "bulge"),
        ("27", "waveWarp"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("source.aep"),
            include_bytes!(
                "../../../crates/aftereffects_file/tests/fixtures/effects/warp_static.aep"
            ),
        )
        .unwrap();
        aep_success(run(
            root,
            &[
                "convert",
                "source.aep",
                "--composition",
                composition,
                "--to",
                "tesseract",
                "-o",
                "imported",
            ],
        ));
        let mut archive =
            tesseract_file::TesseractFile::open(root.join("imported/project.tsrct")).unwrap();
        let mut document = archive.project_json().unwrap();
        let mut count = 0;
        edit(&mut document, target, &mut count);
        assert_eq!(
            count, 1,
            "source {composition} must retain target before export"
        );
        let edit_path = root.join("edit.json");
        fs::write(&edit_path, serde_json::to_vec(&document).unwrap()).unwrap();
        archive.commit_project_json(&edit_path).unwrap();
        archive.save_as(root.join("edited.tsrct")).unwrap();
        let output = run(
            root,
            &[
                "convert",
                "edited.tsrct",
                "--to",
                "after-effects",
                "-o",
                "exported",
            ],
        );
        assert!(output.status.success(), "{output:?}");
        let native = aftereffects_file::structure::read_project(
            &fs::read(root.join("exported/project.aep")).unwrap(),
        )
        .unwrap();
        let imported =
            aftereffects_file::structure_document::to_structural_fx_document(&native, Some(1))
                .unwrap();
        let json = imported.document.to_json_value().unwrap();
        let text = json.to_string();
        assert!(!text.contains("JsScript"));
        if composition == "1" {
            let branch =
                named(&json, "Mirror reflected half").expect("current reflected branch retained");
            let actual = source_matrix(branch, [1., 0., 0., 1.])
                .expect("branch owns editable pre-Mirror content");
            let reflection = |degrees: f64| {
                let a = degrees.to_radians();
                [-a.cos(), -a.sin(), -a.sin(), a.cos()]
            };
            let expected = reflection(80.);
            for (a, e) in actual.into_iter().zip(expected) {
                assert!(
                    (a - e).abs() < 1e-6,
                    "edited branch matrix {actual:?}, expected {expected:?}"
                );
            }
            assert!(
                actual
                    .into_iter()
                    .zip(reflection(60.))
                    .any(|(a, old)| (a - old).abs() > 0.1),
                "unchanged native60 must fail the edited80 contract"
            );
            // Resolve clipping only along the actual pre-Mirror content path,
            // not an unrelated masked descendant or the outer canvas crop.
            fn source_path(node: &Value) -> Option<Vec<&Value>> {
                if node["name"] == "Source before Mirror" {
                    return Some(vec![node]);
                }
                children(node).find_map(|child| {
                    let mut path = source_path(child)?;
                    path.push(node);
                    Some(path)
                })
            }
            fn assert_half_masks(root: &Value) {
                for name in ["Mirror reflected half", "Mirror retained source half"] {
                    let clipped = named(root, name).expect("source branch survives");
                    let path = source_path(clipped).expect("branch owns pre-Mirror content");
                    let masked = path
                        .into_iter()
                        .find(|node| {
                            node.get("masks")
                                .and_then(Value::as_array)
                                .is_some_and(|m| !m.is_empty())
                        })
                        .expect("each source half remains clipped before reflection");
                    assert_mask_reference(root, masked);
                }
            }
            assert_half_masks(&json);
            let (owner, mask) = finite_canvas_mask(&json)
                .expect("common active mask must clip both images to the finite 320x180 canvas");
            let mut missing_canvas = json.clone();
            remove_mask(&mut missing_canvas, owner, mask);
            assert_half_masks(&missing_canvas);
            assert!(
                finite_canvas_mask(&missing_canvas).is_none(),
                "two intact half-plane masks must not substitute for the removed common canvas mask"
            );
        } else {
            assert!(text.contains(target), "{target} lost: {text}");
            assert!(
                text.contains("-0.75"),
                "current edited control lost: {text}"
            );
        }
    }
}

#[test]
fn warp_public_fisheye_keeps_native_lens_approximation_instead_of_empty_ae_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
    document["composition"]["layers"][0]["effects"] =
        json!([{ "id":9,"effect":{"type":"fisheye","amount":20,"centerX":0.5,"centerY":0.5}}]);
    fs::write(root.join("source.mp4"), MEDIA).unwrap();
    tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&document).unwrap(),
    )
    .unwrap()
    .add_asset(
        "premiere-video-1",
        root.join("source.mp4"),
        tesseract_file::AssetKind::Video,
    )
    .unwrap()
    .write(root.join("edited.tsrct"))
    .unwrap();
    let output = run(
        root,
        &[
            "convert",
            "edited.tsrct",
            "--to",
            "premiere",
            "-o",
            "exported",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostics.contains("quarter-frame"), "{diagnostics}");
    assert!(
        !diagnostics.contains("Editable linked-AEP package"),
        "{diagnostics}"
    );
    let mut xml = String::new();
    flate2::read::GzDecoder::new(fs::File::open(root.join("exported/project.prproj")).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("PR.ADBE Lens Distortion"));
    assert!(!xml.contains(".aep"));
}

#[test]
fn procedural_noise_native_keys_and_current_edits_export_canonical_turbulent() {
    use aftereffects_file::{properties, rifx::Chunk, structure};
    fn name(chunk: &Chunk) -> Option<&str> {
        (chunk.id() == *b"tdmn").then(|| {
            std::str::from_utf8(
                chunk
                    .data_payload()
                    .unwrap()
                    .split(|b| *b == 0)
                    .next()
                    .unwrap(),
            )
            .unwrap()
        })
    }
    fn effects<'a>(chunks: &'a [Chunk], result: &mut Vec<(&'a str, &'a [Chunk])>) {
        for pair in chunks.windows(2) {
            if let Some(name) = name(&pair[0]).filter(|_| pair[1].list_kind() == Some(*b"sspc")) {
                let group = pair[1]
                    .children()
                    .unwrap()
                    .iter()
                    .find(|c| c.list_kind() == Some(*b"tdgp"))
                    .unwrap();
                result.push((name, group.children().unwrap()));
            }
        }
        for chunk in chunks {
            if let Some(children) = chunk.children() {
                effects(children, result);
            }
        }
    }
    fn edit_owner(layers: &mut Value) -> usize {
        let mut count = 0;
        for layer in layers.as_array_mut().unwrap() {
            if let Some(list) = layer
                .get_mut("effects")
                .and_then(Value::as_array_mut)
                .filter(|list| {
                    list.iter()
                        .any(|effect| effect["effect"]["type"] == "turbulentNoise")
                })
            {
                let noise = list
                    .iter_mut()
                    .find(|effect| effect["effect"]["type"] == "turbulentNoise")
                    .unwrap();
                noise["enabled"] = json!(false);
                list.push(json!({"id":9900,"effect":{"type":"gaussianBlur","blurriness":7.0}}));
                count += 1;
            }
            if layer["layers"].is_array() {
                count += edit_owner(&mut layer["layers"]);
            }
        }
        count
    }
    let normal: &[u8] = include_bytes!("../../../crates/aftereffects_file/tests/fixtures/effects/native-fractal-turbulent-keys.aep");
    let multiply: &[u8] = include_bytes!(
        "../../../crates/aftereffects_file/tests/fixtures/effects/native-fractal-multiply-keys.aep"
    );
    for (target, source, is_multiply) in [
        ("1", normal, false),
        ("16", normal, false),
        ("1", multiply, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("source.aep"), source).unwrap();
        if is_multiply {
            let native = structure::read_project(source).unwrap();
            let structure::ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
                panic!()
            };
            assert_eq!(comp.layers[0].record.id(), 15);
            let solid = native
                .item(comp.layers[0].record.source_id())
                .unwrap()
                .solid
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            assert_eq!(solid.color, [0.25, 0.5, 0.75]);
            let mut found = Vec::new();
            effects(&comp.layers[0].content, &mut found);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].0, "ADBE Fractal Noise");
            let rows = found[0].1;
            let pair = rows
                .windows(2)
                .find(|pair| name(&pair[0]) == Some("ADBE Fractal Noise-0030"))
                .unwrap();
            assert_eq!(
                properties::read_numeric(pair[1].children().unwrap())
                    .unwrap()
                    .values,
                [5.]
            );
        }
        let imported = run(
            root,
            &[
                "convert",
                "source.aep",
                "--to",
                "tesseract",
                "--composition",
                target,
                "-o",
                "imported",
            ],
        );
        assert!(imported.status.success(), "{imported:?}");
        let mut archive =
            tesseract_file::TesseractFile::open(root.join("imported/project.tsrct")).unwrap();
        let mut document = archive.project_json().unwrap();
        if is_multiply {
            let mut pending: Vec<_> = document["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .collect();
            let mut generator = None;
            let mut colored_source = false;
            let mut multiply_groups = 0;
            while let Some(layer) = pending.pop() {
                if layer["blendMode"] == "multiply" {
                    multiply_groups += 1;
                    assert_eq!(layer["name"], "Independent Fractal generator");
                    generator = Some(layer["layers"][0]["effects"][0]["id"].clone());
                }
                colored_source |= layer["rect"]["fillColor"] == json!([0.25, 0.5, 0.75, 1.0]);
                if let Some(children) = layer.get("layers").and_then(Value::as_array) {
                    pending.extend(children);
                }
            }
            assert_eq!(multiply_groups, 1);
            assert!(colored_source, "nonneutral input retained below Multiply");
            let id = generator.unwrap();
            let entries = document["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap();
            assert_eq!(
                entries
                    .iter()
                    .filter(|entry| entry["target"]["effectId"] == id)
                    .count(),
                6
            );
        }
        assert_eq!(edit_owner(&mut document["composition"]["layers"]), 1);
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap();
        for (parameter, expected, edited) in [
            ("contrast", [100., 130.], 170.),
            ("brightness", [0., 10.], 20.),
            ("scale", [100., 140.], 180.),
            ("offsetX", [0., 5.], 10.),
            ("offsetY", [0., -2.5], -5.),
            ("evolution", [0., 45.], 90.),
        ] {
            let entry = entries
                .iter_mut()
                .find(|entry| entry["target"]["paramName"] == parameter)
                .unwrap();
            let keys = entry["animator"]["keyframes"].as_array_mut().unwrap();
            assert_eq!(keys.len(), 2);
            for (i, value) in expected.into_iter().enumerate() {
                assert!((keys[i]["value"]["value"].as_f64().unwrap() - value).abs() < 1e-8);
                assert_eq!(keys[i]["layerTime"], json!(i * 1000));
            }
            keys[1]["value"]["value"] = json!(edited);
        }
        let edit = root.join("edited.json");
        fs::write(&edit, serde_json::to_vec(&document).unwrap()).unwrap();
        archive.commit_project_json(&edit).unwrap();
        archive.save_as(root.join("edited.tsrct")).unwrap();
        let output = run(
            root,
            &[
                "convert",
                "edited.tsrct",
                "--to",
                "after-effects",
                "-o",
                "native",
            ],
        );
        assert!(output.status.success(), "{output:?}");
        let native =
            structure::read_project(&fs::read(root.join("native/project.aep")).unwrap()).unwrap();
        if is_multiply {
            // Current editable graph exports native Multiply compositing, not Fractal replay.
            assert_eq!(
                native
                    .items
                    .iter()
                    .filter_map(|item| match &item.kind {
                        structure::ItemKind::Composition(comp) => Some(comp),
                        _ => None,
                    })
                    .flat_map(|comp| &comp.layers)
                    .filter(|layer| layer.record.blend_mode() == 5)
                    .count(),
                1
            );
        }
        let mut found = Vec::new();
        for item in &native.items {
            if let structure::ItemKind::Composition(comp) = &item.kind {
                for layer in &comp.layers {
                    effects(&layer.content, &mut found);
                }
            }
        }
        assert_eq!(
            found.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            ["ADBE AIF Perlin Noise 3D", "ADBE Gaussian Blur 2"]
        );
        let group = found[0].1;
        assert_eq!(
            group
                .iter()
                .find(|c| c.id() == *b"tdsb")
                .unwrap()
                .data_payload()
                .unwrap()[3]
                & 1,
            0
        );
        for (slot, expected) in [
            ("0004", vec![100., 170.]),
            ("0005", vec![0., 20.]),
            ("0010", vec![100., 180.]),
            ("0020", vec![0., 90.]),
        ] {
            let native_name = format!("ADBE AIF Perlin Noise 3D-{slot}");
            let pair = group
                .windows(2)
                .find(|pair| name(&pair[0]) == Some(native_name.as_str()))
                .unwrap();
            let property = properties::read_numeric(pair[1].children().unwrap()).unwrap();
            assert_eq!(property.keyframes.len(), 2);
            for (index, key) in property.keyframes.iter().enumerate() {
                assert_eq!(key.time_secs, index as f64);
                assert!(
                    (key.values[0] - expected[index]).abs() < 1e-6,
                    "{slot}: {key:?}"
                );
            }
        }
    }
}
