//! The Premiere 26.5.2 save `template_placeholder_nest.prproj` (provenance in
//! `template_fixtures.provenance.json`, SHA-256 `FIXTURE_SHA256`) through the
//! CLI: a fresh import of sequence "Template Placeholder Nest", whose V2 holds
//! the empty nested sequence "Media _ Placeholder" (a template drop zone)
//! above a Lumetri-graded still, export back to Premiere at 29.97 fps, and
//! reimport. The placeholder stays an empty nested sequence and the grade a
//! native Lumetri Color. Adobe opened, saved, reopened and rendered an export
//! of this route; that evidence is recorded in the provenance, not here.
use super::*;
use sha2::{Digest, Sha256};

const FIXTURE_SHA256: &str = "32015e4ec047699992ba00a569f97edd9e819fdc05f5b15ac16fe6ee4299e9a8";

const SEQUENCE: &str = "55542fdc-3560-47b5-9a93-54eef4f5d673";

const PLACEHOLDER: &str =
    "empty template placeholder sequence \"Media _ Placeholder\" has no source occurrences";

fn placeholder_reports(report: &fx_conv::ConversionReport) -> usize {
    report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains(PLACEHOLDER))
        .count()
}

#[test]
fn template_placeholder_nest_cli_round_trip_keeps_an_empty_nested_sequence_and_the_grade() {
    let source = fixture("template_placeholder_nest.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        FIXTURE_SHA256
    );
    let (_source_dir, source) = relinked_native_fixture(&source);
    let parent = tempfile::tempdir().unwrap();
    let imported = parent.path().join("imported");
    let mut import = request(&source, &imported, ConversionMode::Write);
    import.sequence = Some(SEQUENCE);
    let report = import_premiere(&import).unwrap();
    // The sequence itself and the audio item that plays it.
    assert_eq!(placeholder_reports(&report), 2, "{report:?}");

    let output = parent.path().join("exported");
    let report = export(
        &request(
            &imported.join("project.tsrct"),
            &output,
            ConversionMode::Write,
        ),
        &PremiereExportOptions {
            hdr: None,
            frame_rate: Some(premiere_file::FrameRate::Fps30000Over1001),
        },
    )
    .unwrap();
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|note| note.message.contains("not exported")
                || note.message.contains("no exportable video")
                || note.code.starts_with("HYBRID")),
        "{report:?}"
    );
    let prproj = output.join("project.prproj");
    let text = xml(&prproj);
    assert!(!text.contains(".aep"));
    assert_eq!(
        text.matches("<MatchName>AE.ADBE Lumetri</MatchName>")
            .count(),
        1
    );
    let parsed = roxmltree::Document::parse(&text).unwrap();
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

    let reimported = parent.path().join("reimported");
    let root = parsed
        .descendants()
        .filter(|node| node.has_tag_name("Sequence"))
        .find(|node| {
            node.children().any(|child| {
                child.has_tag_name("Name") && child.text() == Some("Template Placeholder Nest")
            })
        })
        .and_then(|node| node.attribute("ObjectUID"))
        .unwrap()
        .to_owned();
    let mut again = request(&prproj, &reimported, ConversionMode::Write);
    again.sequence = Some(&root);
    let report = import_premiere(&again).unwrap();
    // Export writes no audio item for a placeholder.
    assert_eq!(placeholder_reports(&report), 1, "{report:?}");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("one editable primaryGrade")),
        "{report:?}"
    );
}
