//! Public Film Impact Pop consent through the real converter process.
//! Native control fragments use existing public harnesses, not new Adobe proof.

use fx_conv::{sha256_file, MediaMap, MediaMapSource, MediaReplacement};
use premiere_file::{MediaRelink, MediaRelinkBinding};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Output},
};
use tesseract_file::TesseractFile;

const VIDEO: &[u8] =
    include_bytes!("../../../crates/premiere_file/tests/fixtures/video-30fps-10s.mp4");
const CLIP: &str = include_str!("../../../crates/premiere_file/tests/fixtures/one-clip.xml");

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tsrct-conv"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn convert(root: &Path, output: &str, extra: &[&str], allow: bool, check: bool) -> Value {
    let mut args = vec![
        "convert",
        "source.prproj",
        "--to",
        "tesseract",
        "-o",
        output,
        "--json",
    ];
    args.extend_from_slice(extra);
    if allow {
        args.push("--allow-film-impact-pop");
    }
    if check {
        args.push("--check");
    }
    let result = run(root, &args);
    assert!(result.status.success(), "{result:?}");
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["mode"], if check { "check" } else { "write" });
    assert_eq!(
        report["artifactStatus"],
        if check { "planned" } else { "published" }
    );
    assert_eq!(root.join(output).exists(), !check);
    report
}

fn assert_pop_diagnostics(report: &Value, count: usize, allow: bool) {
    let diagnostics: Vec<_> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| {
            diagnostic["message"]
                .as_str()
                .unwrap()
                .contains("Film Impact")
        })
        .collect();
    assert_eq!(diagnostics.len(), count, "{report}");
    for diagnostic in diagnostics {
        let message = diagnostic["message"].as_str().unwrap();
        if allow {
            assert_eq!(diagnostic["kind"], "approximation", "{diagnostic}");
            assert!(message.contains("remain approximate"), "{message}");
            assert!(!message.contains("(not converted)"), "{message}");
        } else {
            assert_ne!(diagnostic["kind"], "approximation", "{diagnostic}");
            assert!(message.contains("Pop emulation is disabled"), "{message}");
            assert!(message.contains("--allow-film-impact-pop"), "{message}");
            assert!(message.ends_with("(not converted)"), "{message}");
        }
    }
}

fn document(root: &Path, output: &str) -> Value {
    TesseractFile::open(root.join(output).join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap()
}

fn geometry_entries(doc: &Value) -> Vec<&Value> {
    doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            matches!(
                entry["target"]["propertyType"].as_str(),
                Some("scaleX" | "scaleY" | "positionX" | "positionY")
            )
        })
        .collect()
}

// Keep the native 43-control profile intact; relocate only its one-second
// transition range onto the existing public 30fps physical-picture harness.
fn physical_pop_xml() -> String {
    let records =
        include_str!("../../../crates/premiere_file/tests/fixtures/film-impact-pop-profile.xml")
            .replace("<?xml version='1.0' encoding='utf-8'?>", "")
            .replace("<PremiereData>", "")
            .replace("</PremiereData>", "")
            .replace("50854003200", "0")
            .replace("305124019200", "254016000000");
    CLIP.replace("</ClipItems></ClipTrack>", "</ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef=\"1006\"/></TrackItems></TransitionItems></ClipTrack>")
        .replace("<SubClip ObjectRef=\"5\"/></ClipTrackItem>", "<SubClip ObjectRef=\"5\"/><HeadTransition ObjectRef=\"1006\"/></ClipTrackItem>")
        .replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

#[test]
fn film_impact_pop_cli_help_scopes_consent_to_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let help = run(dir.path(), &["convert", "--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("--allow-film-impact-pop"));
    assert!(text.contains("Premiere") && text.contains("approximation"));
    for args in [
        vec!["inspect", "missing.prproj", "--allow-film-impact-pop"],
        vec![
            "transcode",
            "missing.mp4",
            "-o",
            "out.mp4",
            "--allow-film-impact-pop",
        ],
        vec![
            "convert",
            "missing.prproj",
            "--to",
            "tesseract",
            "-o",
            "out",
            "--allow-film-impact-pop=false",
        ],
    ] {
        let result = run(dir.path(), &args);
        assert!(!result.status.success(), "{result:?}");
        assert!(result.stdout.is_empty());
        assert!(!dir.path().join("out").exists());
    }
}

#[test]
fn film_impact_pop_cli_rejects_unused_consent_on_other_routes_before_io() {
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in [
        ("after-effects", "tesseract"),
        ("tesseract", "premiere"),
        ("tesseract", "after-effects"),
    ] {
        for check in [false, true] {
            let mut args = vec![
                "convert",
                "missing.unknown",
                "--from",
                from,
                "--to",
                to,
                "-o",
                "missing-parent/out",
                "--allow-film-impact-pop",
            ];
            if check {
                args.push("--check");
            }
            let result = run(dir.path(), &args);
            assert!(!result.status.success(), "{result:?}");
            assert_eq!(
                String::from_utf8(result.stderr).unwrap().trim(),
                "--allow-film-impact-pop is only supported for Premiere to Tesseract conversion"
            );
            assert!(result.stdout.is_empty());
        }
    }
    for (from, to, reason) in [
        (
            "premiere",
            "premiere",
            "source and destination formats must differ",
        ),
        (
            "premiere",
            "after-effects",
            "direct conversion from Premiere to After Effects is not supported",
        ),
        (
            "after-effects",
            "premiere",
            "direct conversion from After Effects to Premiere is not supported",
        ),
    ] {
        let result = run(
            dir.path(),
            &[
                "convert",
                "missing.unknown",
                "--from",
                from,
                "--to",
                to,
                "-o",
                "out",
                "--allow-film-impact-pop",
            ],
        );
        assert!(!result.status.success());
        assert!(String::from_utf8(result.stderr).unwrap().contains(reason));
        assert!(result.stdout.is_empty());
    }
    assert!(!dir.path().join("out").exists());
}

#[test]
fn film_impact_pop_cli_native_graphics_default_deny_and_explicit_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let records =
        include_str!("../../../crates/premiere_file/tests/fixtures/graphic_pop/native.xml")
            .replace("<PremiereData>", "")
            .replace("</PremiereData>", "");
    // The same minimal wrapper as the public native-fragment library test.
    let xml = format!(
        r#"<PremiereData Version="3">
        <Sequence ObjectUID="pop-test-sequence"><Name>Graphic Pop</Name><TrackGroups><TrackGroup><Second ObjectRef="1"/></TrackGroup></TrackGroups></Sequence>
        <VideoTrackGroup ObjectID="1"><TrackGroup><Tracks><Track ObjectURef="pop-track"/></Tracks><FrameRate>8467200000</FrameRate></TrackGroup><FrameRect>0,0,1080,1920</FrameRect><ComponentOwner><Components ObjectRef="2"/></ComponentOwner></VideoTrackGroup>
        <VideoComponentChain ObjectID="2"><ComponentChain/></VideoComponentChain>
        <VideoClipTrack ObjectUID="pop-track"><ClipTrack><Track><ID>1</ID></Track><ClipItems><TrackItems><TrackItem ObjectRef="421"/><TrackItem ObjectRef="422"/><TrackItem ObjectRef="423"/><TrackItem ObjectRef="424"/></TrackItems></ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef="425"/><TrackItem ObjectRef="426"/><TrackItem ObjectRef="427"/></TrackItems></TransitionItems></ClipTrack></VideoClipTrack>
        {records}</PremiereData>"#
    );
    fs::write(root.join("source.prproj"), xml).unwrap();
    for allow in [false, true] {
        let output = if allow { "allowed" } else { "default" };
        let extra = ["--from", "premiere", "--sequence", "pop-test-sequence"];
        let checked = convert(root, output, &extra, allow, true);
        let written = convert(root, output, &extra, allow, false);
        assert_eq!(checked["diagnostics"], written["diagnostics"]);
        assert_pop_diagnostics(&written, 3, allow);
        let doc = document(root, output);
        assert_eq!(doc["dimensions"], json!({"width": 1080, "height": 1920}));
        let entries = geometry_entries(&doc);
        assert_eq!(entries.len(), if allow { 16 } else { 0 });
        let incoming = doc["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["activeRange"]["start"] == 1267)
            .unwrap();
        assert_eq!(incoming["type"], "Text");
        if allow {
            let keys = &entries
                .iter()
                .find(|entry| {
                    entry["target"]["layerId"] == incoming["id"]
                        && entry["target"]["propertyType"] == "scaleX"
                })
                .unwrap()["animator"]["keyframes"];
            assert_eq!(keys.as_array().unwrap().len(), 32);
            assert_eq!(keys[0]["layerTime"], 0);
            assert_eq!(keys[0]["value"]["value"], 0.0);
            assert_eq!(keys[31]["value"]["value"], 0.0);
        }
    }
}

#[test]
fn film_impact_pop_cli_forwards_consent_through_plain_map_and_relink_imports() {
    for route in ["plain", "map", "relink"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("media")).unwrap();
        let media = root.join("media/source.mp4");
        fs::write(&media, VIDEO).unwrap();
        let mut xml = physical_pop_xml();
        let authored = r"C:\authored\source.mp4";
        if route == "relink" {
            xml = xml.replace(
                "<RelativePath>media/source.mp4</RelativePath>",
                &format!("<FilePath>{authored}</FilePath>"),
            );
        }
        let input = root.join("source.prproj");
        fs::write(&input, xml).unwrap();
        let source_hash = sha256_file(&input).unwrap();
        let media_hash = sha256_file(&media).unwrap();
        let source = MediaMapSource {
            format: "premiere".into(),
            sha256: source_hash.clone(),
            target: "sequence-1".into(),
        };
        let extra = match route {
            "map" => {
                let replacement = root.join("replacement.mp4");
                fs::write(&replacement, VIDEO).unwrap();
                let map = MediaMap {
                    version: 1,
                    source,
                    replacements: vec![MediaReplacement {
                        original: media.canonicalize().unwrap(),
                        original_sha256: media_hash.clone(),
                        replacement: "replacement.mp4".into(),
                        replacement_sha256: sha256_file(&replacement).unwrap(),
                    }],
                };
                fs::write(root.join("map.json"), serde_json::to_vec(&map).unwrap()).unwrap();
                vec!["--media-map", "map.json"]
            }
            "relink" => {
                let relink = MediaRelink {
                    version: 1,
                    source,
                    bindings: vec![MediaRelinkBinding {
                        media_uid: "media-1".into(),
                        authored_path: authored.into(),
                        local_path: media.canonicalize().unwrap(),
                        sha256: media_hash.clone(),
                    }],
                };
                fs::write(
                    root.join("relink.json"),
                    serde_json::to_vec(&relink).unwrap(),
                )
                .unwrap();
                vec!["--media-relink", "relink.json"]
            }
            _ => vec![],
        };
        for allow in [false, true] {
            let output = if allow { "allowed" } else { "default" };
            let checked = convert(root, output, &extra, allow, true);
            let written = convert(root, output, &extra, allow, false);
            assert_eq!(checked["diagnostics"], written["diagnostics"], "{route}");
            assert_pop_diagnostics(&written, 1, allow);
            let doc = document(root, output);
            let layer = &doc["composition"]["layers"][0];
            assert_eq!(layer["type"], "Video");
            assert_eq!(
                layer["playback"]["inputRange"],
                json!({"start": 0, "duration": 5000})
            );
            assert_eq!(layer["sourceRange"], json!({"start": 0, "duration": 5000}));
            let entries = geometry_entries(&doc);
            assert_eq!(entries.len(), if allow { 4 } else { 0 }, "{route}");
            if allow {
                let keys = &entries
                    .iter()
                    .find(|entry| entry["target"]["propertyType"] == "scaleX")
                    .unwrap()["animator"]["keyframes"];
                assert_eq!(keys.as_array().unwrap().len(), 16);
                assert_eq!(keys[0]["layerTime"], 0);
                assert_eq!(keys[0]["value"]["value"], 0.0);
                assert_eq!(keys[15]["layerTime"], 1000);
                assert_eq!(keys[15]["value"]["value"], 100.0);
            }
            let archive = TesseractFile::open(root.join(output).join("project.tsrct")).unwrap();
            assert_eq!(archive.metadata().assets.len(), 1);
            for id in archive.metadata().assets.keys() {
                let mut bytes = Vec::new();
                archive
                    .asset(id)
                    .unwrap()
                    .open()
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                assert_eq!(bytes, VIDEO);
            }
        }
        assert_eq!(sha256_file(&input).unwrap(), source_hash);
        assert_eq!(sha256_file(&media).unwrap(), media_hash);
    }
}

#[test]
fn film_impact_pop_cli_consent_does_not_bypass_physical_media_admission() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("source.prproj"), physical_pop_xml()).unwrap();
    fs::create_dir(root.join("media")).unwrap();
    fs::write(root.join("media/source.mp4"), b"not a video").unwrap();
    for allow in [false, true] {
        for check in [false, true] {
            let mut args = vec!["convert", "source.prproj", "--to", "tesseract", "-o", "out"];
            if allow {
                args.push("--allow-film-impact-pop");
            }
            if check {
                args.push("--check");
            }
            let result = run(root, &args);
            assert!(!result.status.success(), "{result:?}");
            assert!(result.stdout.is_empty());
            assert!(!root.join("out").exists());
            let error = String::from_utf8(result.stderr).unwrap();
            assert!(
                error.contains("media") || error.contains("video"),
                "{error}"
            );
            assert!(!error.contains("Pop emulation is disabled"), "{error}");
        }
    }
}
