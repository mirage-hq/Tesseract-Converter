//! Reduced native format96 sources through the public Premiere admission path.
//! Derived link XML and editable assertions are not independent render proof.

use super::*;
use fx_conv::{ConversionMode, ImportToTesseract, MediaKind, MediaStatus};

#[test]
fn native_format96_links_keep_all_five_editable_pictures_with_missing_footage() {
    let provenance: Value = serde_json::from_slice(
        &fs::read(fixture(
            "../aftereffects_file/tests/fixtures/hybrid/format96/provenance.json",
        ))
        .unwrap(),
    )
    .unwrap();
    let occurrences = provenance["occurrences"].as_array().unwrap();
    assert_eq!(occurrences.len(), 5);
    for occurrence in occurrences {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::copy(
            fixture("../aftereffects_file/tests/fixtures/hybrid/format96/coeditor-format96.rifx"),
            root.join("linked.aep"),
        )
        .unwrap();
        let guid = occurrence["guid"].as_str().unwrap();
        let item = occurrence["item_id"].as_u64().unwrap();
        let canvas = if item == 2165 {
            [1080, 1920]
        } else {
            [2160, 3840]
        };
        let xml = linked_av_declaring("linked.aep", guid, &TICKS.to_string()).replacen(
            "0,0,1920,1080",
            &format!("0,0,{},{}", canvas[0], canvas[1]),
            1,
        );
        let input = root.join("source.prproj");
        crate::test_support::write_prproj(&input, &xml);
        let options = crate::PremiereImportOptions::default();
        if item == 41 {
            let inspection = crate::Premiere
                .inspect_media(&input, &options, None)
                .unwrap();
            let video = inspection
                .media
                .iter()
                .find(|media| media.id == "53")
                .unwrap();
            assert_eq!(video.kind, MediaKind::Video);
            assert_eq!(video.status, MediaStatus::Missing);
            assert_eq!(video.name, "C20250915_0989.MP4");
        }
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            let output = root.join(format!("converted-{mode:?}"));
            let report = crate::Premiere
                .import_to_tesseract(&input, &output, &options, mode)
                .unwrap();
            if item == 41 {
                assert!(
                    report.diagnostics.iter().any(|note| {
                        note.reason.contains("C20250915_0989.MP4")
                            && note.reason.contains("missing")
                    }),
                    "{report:?}"
                );
            }
            if mode == ConversionMode::Check {
                assert!(!output.exists());
                continue;
            }
            let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
            assert!(archive.metadata().assets.is_empty());
            let document = archive.project_json().unwrap();
            let groups = linked_groups(&document);
            assert_eq!(groups.len(), 1, "occurrence {}", occurrence["occurrence"]);
            assert_canvas_clip(groups[0], canvas.map(f64::from));
            let content = all_layers(groups[0]);
            assert!(content.iter().any(|layer| {
                layer["name"] == occurrence["item_name"] && layer["type"] == "Group"
            }));
            if item == 121 {
                let text: Vec<_> = content
                    .iter()
                    .filter(|layer| layer["type"] == "Text")
                    .map(|layer| layer["sourceText"]["text"].as_str().unwrap())
                    .collect();
                assert_eq!(text, ["RESULTS", "FINAL"]);
            } else {
                assert!(!rect_fills(groups[0]).is_empty());
            }
            assert!(content.iter().all(|layer| {
                !matches!(layer["type"].as_str(), Some("Video" | "Image" | "Audio"))
            }));
            assert_unique_identities(&document);
        }
    }
}
