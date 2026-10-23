#![cfg(feature = "ffmpeg-library")]

//! `color_lumetri_look_cube.prproj`: human-authored in the Premiere 26.5.2
//! UI, one Lumetri Color on `a1_bg_chart.png` whose Basic Correction > Input
//! LUT and Creative > Look were each browsed to `color_warm_tint.cube`
//! (Intensity 100, every other control at its default). The saved paths were
//! replaced at equal length by `/fixture-source/<padding>/color_warm_tint.cube`
//! (`color_fixtures.provenance.json`), so import finds the `.cube` beside the
//! project.

use super::support::*;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tesseract_file::TesseractFile;

const PROJECT: &str = "color_lumetri_look_cube.prproj";
const PROJECT_SHA256: &str = "031a4ac3e1bdc507818e199f873dab1f5809a773029393e0fa78cd035347cfd6";
const SEQUENCE: &str = "af04973b-c53f-4d20-aeb3-bf0e65b12dd6";
const CUBE: &str = "color_warm_tint.cube";
const MEDIA: &str = "a1_bg_chart.png";
const SDR: &str = "jerboa:sdr-rec709-display:v1";

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn images(document: &Value) -> Vec<Value> {
    fn visit(layers: &Value, found: &mut Vec<Value>) {
        for layer in layers.as_array().into_iter().flatten() {
            if layer["type"] == "Image" {
                found.push(layer.clone());
            }
            visit(&layer["layers"], found);
        }
    }
    let mut found = Vec::new();
    visit(&document["composition"]["layers"], &mut found);
    found
}

fn asset_bytes(file: &TesseractFile, asset_id: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut file.asset(asset_id).unwrap().open().unwrap(),
        &mut bytes,
    )
    .unwrap();
    bytes
}

/// A fresh import of `project` into `output`: the archive, its one still
/// image layer and the import's reports.
fn import(project: &Path, output: &Path) -> (TesseractFile, Value, Vec<premiere_file::Omission>) {
    import_sequence(project, output, SEQUENCE)
}

fn import_sequence(
    project: &Path,
    output: &Path,
    sequence: &str,
) -> (TesseractFile, Value, Vec<premiere_file::Omission>) {
    let omissions = premiere_to_tesseract(project, output, Some(sequence), false).unwrap();
    let file = TesseractFile::open(first_project(output)).unwrap();
    let document = file.project_json().unwrap();
    let images = images(&document);
    assert_eq!(images.len(), 1, "{document}");
    (file, images[0].clone(), omissions)
}

/// Reports about the LUTs other than the interpolation note every imported
/// custom LUT carries ([`interpolation_notes`]).
fn lut_reports(omissions: &[premiere_file::Omission]) -> Vec<&premiere_file::Omission> {
    omissions
        .iter()
        .filter(|omission| {
            (omission.reason.contains("LUT")
                || omission.reason.contains("Look")
                || omission.reason.contains("ParameterID 98"))
                && !omission.reason.contains(INTERPOLATION)
        })
        .collect()
}

/// Premiere interpolates LUTs tetrahedrally, FX trilinearly (measured).
const INTERPOLATION: &str = "Premiere interpolates the LUT tetrahedrally";

fn interpolation_notes(omissions: &[premiere_file::Omission]) -> usize {
    omissions
        .iter()
        .filter(|omission| {
            omission.reason.starts_with("Lumetri Color custom LUT: ")
                && omission.reason.contains(INTERPOLATION)
                && omission.kind == premiere_file::OmissionKind::Approximated
        })
        .count()
}

#[test]
fn native_lumetri_input_lut_and_look_import_as_input_transform_and_look_transform() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(fs::read(fixture_path(PROJECT)).unwrap())
        ),
        PROJECT_SHA256
    );
    let provenance: Value =
        serde_json::from_slice(&fs::read(fixture_path("color_fixtures.provenance.json")).unwrap())
            .unwrap();
    let case = provenance["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case"] == "color_lumetri_look_cube")
        .unwrap();
    assert_eq!(case["project"]["sha256"], PROJECT_SHA256);
    assert_eq!(case["sequence_uid"], SEQUENCE);
    let dir = tempfile::tempdir().unwrap();
    let (file, image, omissions) = import(&fixture_path(PROJECT), &dir.path().join("converted"));
    // The Input LUT is the first operation of the clip: the source's Input
    // Transform, from Rec. 709 display values to Rec. 709 display values.
    let input = &image["source"]["inputTransform"];
    let asset_id = input["transform"]["assetId"].as_str().unwrap().to_owned();
    assert_eq!(
        input,
        &json!({
            "semanticVersion": 1,
            "transform": {
                "type": "lut3d",
                "assetId": asset_id,
                "inputEncoding": SDR,
                "outputEncoding": SDR,
                "interpolation": "trilinear",
            },
        })
    );
    // The Look, after the neutral Basic Correction (no primaryGrade) and
    // before the default Vignette (no vignette), at Intensity 100.
    let effects = image["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(effects[0]["enabled"], true);
    assert_eq!(
        effects[0]["effect"],
        json!({
            "type": "lookTransform",
            "semanticVersion": 1,
            "transform": {
                "type": "lut3d",
                "assetId": asset_id,
                "inputEncoding": SDR,
                "outputEncoding": SDR,
                "interpolation": "trilinear",
            },
            "mix": 1.0,
        })
    );
    // One packaged LUT for both slots, byte-equal to the committed .cube.
    assert_eq!(
        asset_bytes(&file, &asset_id),
        fs::read(fixture_path(CUBE)).unwrap()
    );
    let image_asset = image["source"]["assetId"].as_str().unwrap();
    assert_eq!(
        asset_bytes(&file, image_asset),
        fs::read(fixture_path(MEDIA)).unwrap()
    );
    assert_eq!(file.metadata().assets.len(), 2, "{:?}", file.metadata());
    // Neither slot is reported lost, and the embedded samples are not
    // reported separately.
    assert!(lut_reports(&omissions).is_empty(), "{omissions:?}");
    // The interpolation approximation of both LUTs (identical reports on one
    // clip merge into one).
    assert_eq!(interpolation_notes(&omissions), 1, "{omissions:?}");
}

/// The fixture XML with `edit` applied, saved beside copies of its media and,
/// when `with_cube`, its `.cube`.
fn edited_fixture(directory: &Path, with_cube: bool, edit: impl Fn(String) -> String) -> PathBuf {
    let xml = premiere_file_xml(&fixture_path(PROJECT));
    fs::copy(fixture_path(MEDIA), directory.join(MEDIA)).unwrap();
    if with_cube {
        fs::copy(fixture_path(CUBE), directory.join(CUBE)).unwrap();
    }
    let project = directory.join(PROJECT);
    write_prproj(&project, &edit(xml));
    project
}

fn premiere_file_xml(path: &Path) -> String {
    let mut xml = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::GzDecoder::new(fs::File::open(path).unwrap()),
        &mut xml,
    )
    .unwrap();
    xml
}

/// The RGB samples of a `.cube` text, in file order.
fn cube_samples(text: &str) -> Vec<[f32; 3]> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.'))
        .map(|line| {
            let values: Vec<f32> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            [values[0], values[1], values[2]]
        })
        .collect()
}

#[test]
fn native_lumetri_luts_without_their_cube_import_the_copy_premiere_embedded() {
    // The fixture's sanitized paths name no existing file, and Premiere
    // 26.5.2 still renders both LUTs: from the copy in ParameterID 98 that
    // its Blob graph references.
    let dir = tempfile::tempdir().unwrap();
    let project = edited_fixture(dir.path(), false, |xml| xml);
    let (file, image, omissions) = import(&project, &dir.path().join("converted"));
    let asset_id = image["source"]["inputTransform"]["transform"]["assetId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        image["effects"][0]["effect"]["transform"]["assetId"],
        asset_id.as_str()
    );
    let packaged = String::from_utf8(asset_bytes(&file, &asset_id)).unwrap();
    assert!(
        packaged.starts_with("LUT_3D_SIZE 17\n"),
        "{}",
        &packaged[..40]
    );
    let committed = fs::read_to_string(fixture_path(CUBE)).unwrap();
    let samples = cube_samples(&packaged);
    assert_eq!(samples.len(), 17 * 17 * 17);
    assert_eq!(samples, cube_samples(&committed));
    assert!(
        file.metadata().assets[asset_id.as_str()]
            .path
            .ends_with("/color_warm_tint.cube"),
        "{:?}",
        file.metadata()
    );
    assert!(lut_reports(&omissions).is_empty(), "{omissions:?}");
}

#[test]
fn native_lumetri_luts_without_their_cube_or_blob_graph_are_reported_and_the_clip_is_kept() {
    // With a sentinel Blob the embedded copy is not referenced, and Premiere
    // 26.5.2 renders the fixture without either LUT when the file is missing.
    let dir = tempfile::tempdir().unwrap();
    let project = edited_fixture(dir.path(), false, sentinel_blob);
    let (file, image, omissions) = import(&project, &dir.path().join("converted"));
    assert!(image["source"].get("inputTransform").is_none(), "{image}");
    assert!(image["effects"]
        .as_array()
        .is_none_or(|effects| effects.is_empty()));
    assert_eq!(file.metadata().assets.len(), 1);
    for control in [
        "Lumetri Color > Basic Correction > Input LUT",
        "Lumetri Color > Creative > Look",
    ] {
        let report = omissions
            .iter()
            .find(|omission| omission.reason.contains(control))
            .unwrap_or_else(|| panic!("{control}: {omissions:?}"));
        assert!(
            report.reason.contains("color_warm_tint.cube")
                && report.reason.contains("beside the project"),
            "{report:?}"
        );
    }
}

/// `xml` with the text of its Lumetri Blob (ParameterID 1) shader graph edited.
fn edit_blob(xml: &str, edit: impl Fn(&str) -> String) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let start = xml.find("<Name>Blob</Name>").unwrap();
    let value = start + xml[start..].find("<StartKeyframeValue").unwrap();
    let body = value + xml[value..].find('>').unwrap() + 1;
    let end = value + xml[value..].find("</StartKeyframeValue>").unwrap();
    let text = String::from_utf8(STANDARD.decode(xml[body..end].trim()).unwrap()).unwrap();
    format!(
        "{}{}{}",
        &xml[..body],
        STANDARD.encode(edit(&text)),
        &xml[end..]
    )
}

/// The Lumetri Blob (ParameterID 1) replaced by the constant `FE FE`
/// sentinel that a scripted save writes.
fn sentinel_blob(xml: String) -> String {
    let start = xml.find("<Name>Blob</Name>").unwrap();
    let value = start + xml[start..].find("<StartKeyframeValue").unwrap();
    let end = value + xml[value..].find("</StartKeyframeValue>").unwrap();
    format!(
        "{}<StartKeyframeValue Encoding=\"base64\" BinaryHash=\"63866a15-9327-e4cb-ff31-c9ea0000000e\">/v4={}",
        &xml[..value],
        &xml[end..]
    )
}

/// The value of binary ParameterID `id` replaced by `value` (base64).
fn with_binary(xml: &str, id: u32, value: &str) -> String {
    let at = xml
        .find(&format!("<ParameterID>{id}</ParameterID>"))
        .unwrap();
    let start = at + xml[at..].find("<StartKeyframeValue").unwrap();
    let close = start + xml[start..].find('>').unwrap();
    let end = if xml[..close].ends_with('/') {
        close + 1
    } else {
        let closing = "</StartKeyframeValue>";
        close + xml[close..].find(closing).unwrap() + closing.len()
    };
    format!(
        "{}<StartKeyframeValue Encoding=\"base64\" BinaryHash=\"{}\">{value}</StartKeyframeValue>{}",
        &xml[..start],
        if value.is_empty() {
            "63866a15-9327-e4cb-ff31-c9ea0000000e".to_owned()
        } else {
            format!("4c757431-0000-0000-0000-{:012x}", value.len())
        },
        &xml[end..]
    )
}

#[test]
fn a_lumetri_path_slot_is_imported_when_its_blob_is_the_constant_sentinel() {
    // Premiere 26.5.2 renders this edit of the fixture (Blob replaced by the
    // `FE FE` sentinel, everything else unchanged) identically to the
    // UI-authored project: both LUTs apply, so the path slots decide.
    let dir = tempfile::tempdir().unwrap();
    let project = edited_fixture(dir.path(), true, sentinel_blob);
    let (file, image, omissions) = import(&project, &dir.path().join("converted"));
    let asset_id = image["source"]["inputTransform"]["transform"]["assetId"]
        .as_str()
        .unwrap();
    assert_eq!(image["effects"][0]["effect"]["type"], "lookTransform");
    assert_eq!(
        image["effects"][0]["effect"]["transform"]["assetId"],
        asset_id
    );
    assert_eq!(
        asset_bytes(&file, asset_id),
        fs::read(fixture_path(CUBE)).unwrap()
    );
    assert!(lut_reports(&omissions).is_empty(), "{omissions:?}");
}

#[test]
fn a_lumetri_path_slot_that_its_blob_graph_contradicts_is_not_imported() {
    let dir = tempfile::tempdir().unwrap();
    let project = edited_fixture(dir.path(), true, |xml| {
        edit_blob(&xml, |text| {
            assert!(text.contains(CUBE));
            text.replace(CUBE, "color_cold_tint.cube")
        })
    });
    let (_, image, omissions) = import(&project, &dir.path().join("converted"));
    assert!(image["source"].get("inputTransform").is_none(), "{image}");
    assert!(image["effects"]
        .as_array()
        .is_none_or(|effects| effects.is_empty()));
    let reports: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.reason.contains("Lumetri custom LUT omitted"))
        .collect();
    assert_eq!(reports.len(), 2, "{omissions:?}");
    for report in reports {
        assert!(
            report.reason.contains("color_cold_tint.cube")
                && report
                    .reason
                    .contains("which LUT Premiere renders is unverified"),
            "{report:?}"
        );
    }
    // The embedded samples that no imported LUT uses are reported too.
    assert!(
        lut_reports(&omissions)
            .iter()
            .any(|omission| omission.reason.contains("ParameterID 98")),
        "{omissions:?}"
    );
}

/// The Lumetri Look path slot (ParameterID 24) and Intensity (26) of the one
/// Lumetri Color in an exported project, decoded.
fn exported_look_slot(project: &Path) -> (String, String) {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let xml = premiere_file_xml(project);
    let param = |id: u32| {
        let tag = format!("<ParameterID>{id}</ParameterID>");
        let at = xml.find(&tag).unwrap_or_else(|| panic!("ParameterID {id}"));
        let open = xml[..at].rfind("ComponentParam ObjectID").unwrap();
        let close = at + xml[at..].find("ComponentParam>").unwrap();
        xml[open..close].to_owned()
    };
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Lumetri</MatchName>")
            .count(),
        1
    );
    let look = param(24);
    let value = look.split("<StartKeyframeValue").nth(1).unwrap();
    let encoded =
        &value[value.find('>').unwrap() + 1..value.find("</StartKeyframeValue>").unwrap()];
    let bytes = STANDARD.decode(encoded.trim()).unwrap();
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    assert_eq!(units.last(), Some(&0), "one NUL terminator");
    let path = String::from_utf16(&units[..units.len() - 1]).unwrap();
    let intensity = param(26);
    let start = intensity.split("<StartKeyframe>").nth(1).unwrap();
    (path, start[..start.find('<').unwrap()].to_owned())
}

#[test]
fn native_lumetri_look_exports_as_a_lumetri_creative_look_with_its_cube_and_reimports() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("look.tsrct");
    build_tesseract_file(&fixture_path(PROJECT), &archive, Some(SEQUENCE)).unwrap();
    let native = dir.path().join("premiere");
    let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
    // The Input LUT, now the still's source inputTransform, exports as the
    // clip Interpret Footage Input LUT, its native equal; it is not reported.
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.reason.contains("input transform was not exported")),
        "{omissions:?}"
    );
    assert_eq!(
        premiere_file_xml(&native.join("project.prproj"))
            .matches("<InputLUTSpecified>true</InputLUTSpecified>")
            .count(),
        1
    );
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("lookTransform exports as a native Lumetri Color > Creative > Look")),
        "{omissions:?}"
    );
    // The .cube is written once beside the media; the Look path slot holds
    // its absolute path, NUL-terminated, at Intensity 100.
    let cube = native.join("media").join(CUBE);
    assert_eq!(
        fs::read(&cube).unwrap(),
        fs::read(fixture_path(CUBE)).unwrap()
    );
    // The clip Input LUT's file beside the project takes another name, as
    // import looks for the Look's saved name there first.
    assert!(!native.join(CUBE).exists());
    assert!(native.join("color_warm_tint-2.cube").is_file());
    let project = native.join("project.prproj");
    let (path, intensity) = exported_look_slot(&project);
    assert!(Path::new(&path).is_absolute(), "{path}");
    assert_eq!(
        Path::new(&path).canonicalize().unwrap(),
        cube.canonicalize().unwrap()
    );
    let intensity: f64 = intensity
        .split(',')
        .nth(1)
        .unwrap()
        .trim_end_matches('.')
        .parse()
        .unwrap();
    assert_eq!(intensity, 100.0);
    // Imported again, the Look is the same lookTransform with the same LUT.
    let (file, image, reimport) = import_sequence(
        &project,
        &dir.path().join("reimported"),
        &exported_root_sequence(&project),
    );
    let effects = image["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(effects[0]["effect"]["type"], "lookTransform");
    assert_eq!(effects[0]["effect"]["mix"], 1.0);
    let asset_id = effects[0]["effect"]["transform"]["assetId"]
        .as_str()
        .unwrap();
    assert_eq!(
        asset_bytes(&file, asset_id),
        fs::read(fixture_path(CUBE)).unwrap()
    );
    // The clip Input LUT imports back as the source inputTransform; with no
    // Color Space Override its input encoding is the reported assumption.
    assert_eq!(
        image["source"]["inputTransform"]["transform"]["type"], "lut3d",
        "{image}"
    );
    assert!(
        lut_reports(&reimport).iter().all(|omission| omission
            .reason
            .contains("Input LUT source encoding is unknown")),
        "{reimport:?}"
    );
}

#[test]
fn a_lumetri_path_slot_saved_without_its_terminator_is_not_imported() {
    // Premiere 26.5.2 renders the fixture with a sentinel Blob, no embedded
    // samples and its path slots saved without the NUL terminator (the form
    // a scripted path write saves) without either LUT, though the .cube
    // exists; with the terminator it renders both.
    use base64::{engine::general_purpose::STANDARD, Engine};
    let dir = tempfile::tempdir().unwrap();
    let project = edited_fixture(dir.path(), true, |xml| {
        let xml = sentinel_blob(xml);
        let at = xml.find("<ParameterID>4</ParameterID>").unwrap();
        let value = at + xml[at..].find("<StartKeyframeValue").unwrap();
        let body = value + xml[value..].find('>').unwrap() + 1;
        let end = value + xml[value..].find("</StartKeyframeValue>").unwrap();
        let mut bytes = STANDARD.decode(xml[body..end].trim()).unwrap();
        assert_eq!(bytes.split_off(bytes.len() - 2), [0, 0]);
        let unterminated = STANDARD.encode(bytes);
        // The Look slot keeps its own copy; the embedded samples are cleared.
        let xml = with_binary(&xml, 24, &unterminated);
        let xml = with_binary(&xml, 4, &unterminated);
        with_binary(&xml, 98, "")
    });
    let (_, image, omissions) = import(&project, &dir.path().join("converted"));
    assert!(image["source"].get("inputTransform").is_none(), "{image}");
    assert!(image["effects"]
        .as_array()
        .is_none_or(|effects| effects.is_empty()));
    let reports: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.reason.contains("Lumetri custom LUT omitted"))
        .collect();
    assert_eq!(reports.len(), 2, "{omissions:?}");
    for report in reports {
        assert!(
            report.reason.contains("without the NUL terminator"),
            "{report:?}"
        );
    }
}
