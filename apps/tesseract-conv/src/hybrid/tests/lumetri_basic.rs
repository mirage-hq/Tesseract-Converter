//! The Premiere 26.5.2 save `color_lumetri_basic.prproj` (provenance in
//! `color_fixtures.provenance.json`, SHA-256 `FIXTURE_SHA256`) through
//! the CLI: a fresh import of sequence "Lumetri Basic" to one editable
//! primaryGrade, export back to a native Lumetri Color at 30 fps, and
//! reimport. The automatic hybrid keeps the native Premiere picture: the
//! grade's declared scale approximation is not a routing loss, so no linked
//! AEP scope replaces it. Structural only: this 30 fps native route has no
//! Adobe open or render of its export.
use super::*;
use sha2::{Digest, Sha256};

const FIXTURE_SHA256: &str = "8b78978e027e9b1af0df8a5c06009edd7a8b9bce0c4a6948b94a8d48195ff4a1";

const SEQUENCE: &str = "5c919df6-dea9-4620-9eb3-611b6fc20602";

/// The authored Basic Correction values by the fitted scales: Temperature
/// 30/116, Tint -(-15)/100, Exposure 0.5/2.6, Contrast 30/360,
/// Highlights -25/240, Shadows 25/510, Whites 15/54, Blacks -15/124,
/// Saturation (130 - 100)/110, Vibrance 0.
const GRADE: [(&str, f64); 10] = [
    ("temperature", 30.0 / 116.0),
    ("tint", 0.15),
    ("exposure", 0.5 / 2.6),
    ("contrast", 30.0 / 360.0),
    ("highlights", -25.0 / 240.0),
    ("shadows", 25.0 / 510.0),
    ("whites", 15.0 / 54.0),
    ("blacks", -15.0 / 124.0),
    ("saturation", 30.0 / 110.0),
    ("vibrance", 0.0),
];

/// The effects of the one effected layer of the project at `archive`.
fn graded_effects(archive: &Path) -> Vec<Value> {
    let document = TesseractFile::open(archive)
        .unwrap()
        .project_json()
        .unwrap();
    let mut graded: Vec<Vec<Value>> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|layer| {
            layer["effects"]
                .as_array()
                .filter(|effects| !effects.is_empty())
        })
        .cloned()
        .collect();
    assert_eq!(graded.len(), 1);
    graded.remove(0)
}

fn assert_grade(effect: &Value, expected: &[(&str, f64)]) {
    assert_eq!(effect["type"], "primaryGrade");
    assert_eq!(effect["semanticVersion"], 1);
    for (name, value) in expected {
        let actual = effect[*name].as_f64().unwrap();
        assert!((actual - value).abs() < 1e-9, "{name}: {actual} != {value}");
    }
}

#[test]
fn color_lumetri_basic_cli_exports_native_lumetri_at_30_fps_and_reimports_the_grade() {
    let source = fixture("color_lumetri_basic.prproj");
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
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("one editable primaryGrade")),
        "{report:?}"
    );
    // No Lumetri section beyond the grade and vignette differs from its
    // defaults.
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Lumetri")
                && diagnostic.message.contains("omitted")),
        "{report:?}"
    );
    let effects = graded_effects(&imported.join("project.tsrct"));
    // The Vignette section is saved at its defaults: no vignette.
    assert_eq!(effects.len(), 1);
    assert_grade(&effects[0]["effect"], &GRADE);

    let output = parent.path().join("exported");
    let report = export(
        &request(
            &imported.join("project.tsrct"),
            &output,
            ConversionMode::Write,
        ),
        &PremiereExportOptions {
            hdr: None,
            frame_rate: Some(premiere_file::FrameRate::Fps30),
        },
    )
    .unwrap();
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|note| note.message.contains("not exported") || note.code.starts_with("HYBRID")),
        "{report:?}"
    );
    let prproj = output.join("project.prproj");
    let text = xml(&prproj);
    assert_eq!(
        text.matches("<MatchName>AE.ADBE Lumetri</MatchName>")
            .count(),
        1
    );
    assert!(!text.contains(".aep"));
    assert!(!output.join("media/ae-0001").exists());
    // The saved scalars and the Blob sentinel, as Premiere saved them.
    for saved in [
        "-91445760000000000,30.,0,0,0,0,0,0",
        "-91445760000000000,-15.,0,0,0,0,0,0",
        "-91445760000000000,0.5,0,0,0,0,0,0",
        "-91445760000000000,130.,0,0,0,0,0,0",
        "-91445760000000000,-25.,0,0,0,0,0,0",
    ] {
        assert!(
            text.contains(&format!("<StartKeyframe>{saved}</StartKeyframe>")),
            "{saved}"
        );
    }
    assert!(text.contains(">/v4=</StartKeyframeValue>"));

    let reimported = parent.path().join("reimported");
    import_premiere(&request(&prproj, &reimported, ConversionMode::Write)).unwrap();
    let again = graded_effects(&reimported.join("project.tsrct"));
    // The written Lumetri's neutral Vignette section adds no effect.
    assert_eq!(again.len(), 1, "{again:?}");
    assert_grade(&again[0]["effect"], &GRADE);
}
