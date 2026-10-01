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
            .start_file(entry.name(), zip::write::SimpleFileOptions::default())
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
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut document: Value = serde_json::from_str(EDITABLE).unwrap();
    document["composition"]["layers"][0]["transform"]["opacity"] = json!(99);
    document["composition"]["layers"][0]["effects"] = json!([
        {"id": 1, "effect": {"type": "mosaic", "horizontalBlocks": 10.0, "verticalBlocks": 20.0}}
    ]);
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
    assert_eq!(fs::read(root.join("out/media/source.mp4")).unwrap(), MEDIA);
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
                "JS animator baking approximated {after_effects_baked} scalar/Path tracks"
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

    // `--fps 24` snaps the cut back to frame 37 of a 24 fps sequence.
    success(run(
        root,
        &[
            "convert", document, "--to", "premiere", "-o", "premiere", "--fps", "24",
        ],
    ));
    let project = root.join("premiere/project.prproj");
    let (native, _) = premiere_file::PrProjectFile::load(&project).unwrap();
    let clips: Vec<_> = native
        .sequences()
        .next()
        .unwrap()
        .video_occurrences()
        .map(|clip| (clip.timeline_ticks(), clip.source_ticks()))
        .collect();
    let frame = 10_584_000_000;
    assert_eq!(
        clips,
        [
            (0..37 * frame, 0..37 * frame),
            (37 * frame..72 * frame, 0..35 * frame)
        ]
    );
    let mut xml = String::new();
    flate2::read::GzDecoder::new(fs::File::open(&project).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml
        .contains("<MZ.Sequence.VideoTimeDisplayFormat>100</MZ.Sequence.VideoTimeDisplayFormat>"));

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
fn premiere_hybrid_cli_rejects_unsupported_clock_without_publication() {
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
        assert!(!output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("integral 24, 25 or 30 fps"));
        assert!(!root.join("package").exists());
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
