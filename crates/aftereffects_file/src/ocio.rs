//! OCIO colour management ↔ FX ACES `colorManagement`.
//!
//! After Effects 26.5 saves the project colour engine as the `pcms` JSON
//! (`colorManagementSystem` 1 = OCIO, `ocioConfigurationFile` = the built-in
//! config's display name), the working colour space as the `PwCs` JSON (`{}`:
//! the config's `scene_linear` role) and each footage item's OCIO input
//! colourspace as the `Pin `/`CLRS` `ocsp` JSON. An OCIO declaration is a
//! `baseProfileType` 3 profile whose `colorProfileName` is
//! `<family>/<colourspace>` and whose `colorProfileData` is base64 of
//! `{"colorSpace1":<colourspace>}` (`color_aces_ae_project.aep`).
//!
//! The built-in ACES 2.0 configs ("ACES 2.0 Studio v4.0", "ACES 2.0 CG v4.0")
//! with an ACEScg working space are the native equal of an FX ACES document
//! with `wholeProgram` compositing (AE composites every layer in ACEScg), the
//! ACES 2.0 output transforms, and camera input transforms with the names of
//! the FX ACES pipeline ([`fx_conv::aces`]).
//! Neither direction converts AE's display or output-module colour spaces.

use crate::{
    aep::Project, hdr::SettingsRecord, rifx::Chunk, structure::ProjectColorContext,
    writer::AepWriteError,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

/// The built-in config written for an FX ACES document: its camera input
/// colourspaces cover every FX ACES `cameraInput`.
pub(crate) const ACES_STUDIO_CONFIG: &str = "ACES 2.0 Studio v4.0";
/// AE 26.5's built-in ACES 2.0 OCIO configs.
const ACES_2_CONFIGS: [&str; 2] = [ACES_STUDIO_CONFIG, "ACES 2.0 CG v4.0"];
/// The ACES 2.0 configs' `scene_linear` role, FX ACES's compositing space.
pub(crate) const ACESCG: &str = "ACEScg";

/// The colourspace an OCIO profile declaration names, or `None` for any
/// other declaration (`{}`, ICC or Adobe built-in profiles).
fn colorspace(value: &Value) -> Option<String> {
    if value["baseProfileType"].as_u64()? != 3 {
        return None;
    }
    let data = STANDARD
        .decode(value["baseColorProfile"]["colorProfileData"].as_str()?)
        .ok()?;
    let data: Value = serde_json::from_slice(&data).ok()?;
    data["colorSpace1"].as_str().map(str::to_owned)
}

/// The OCIO profile declaration of `colorspace` in OCIO family `family`.
fn declaration(family: &str, colorspace: &str) -> String {
    let data = STANDARD.encode(json!({ "colorSpace1": colorspace }).to_string());
    json!({
        "baseColorProfile": {"colorProfileData": data, "colorProfileName": format!("{family}/{colorspace}")},
        "baseProfileType": 3
    })
    .to_string()
}

/// The `Utf8` JSON that follows the single top-level declaration `id`.
fn declared(project: &Project, id: [u8; 4]) -> Option<Value> {
    let mut found = project
        .chunks
        .iter()
        .enumerate()
        .filter(|(_, chunk)| chunk.id() == id);
    let (index, _) = found.next()?;
    if found.next().is_some() {
        return None;
    }
    let value = project
        .chunks
        .get(index + 1)
        .filter(|chunk| chunk.id() == *b"Utf8")?;
    serde_json::from_slice(value.data_payload()?).ok()
}

/// The colour context of an OCIO colour-managed project; `None` for the
/// Adobe colour engine (left to [`crate::hdr::import_context`]).
pub(crate) fn import_context(project: &Project) -> Option<ProjectColorContext> {
    let engine = declared(project, *b"pcms")?;
    if engine["colorManagementSystem"].as_u64() != Some(1) {
        return None;
    }
    let config = engine["ocioConfigurationFile"]
        .as_str()
        .unwrap_or("unnamed OCIO config")
        .chars()
        .take(160)
        .collect::<String>();
    let working = match declared(project, *b"PwCs") {
        Some(value) if value.as_object().is_some_and(serde_json::Map::is_empty) => {
            Some(ACESCG.to_owned())
        }
        Some(value) => colorspace(&value),
        None => None,
    };
    let unsupported = |reason| ProjectColorContext::Unsupported {
        profile: format!("OCIO config {config:?}, working colour space {working:?}"),
        reason,
    };
    if !ACES_2_CONFIGS.contains(&config.as_str()) {
        return Some(unsupported(
            "only AE's built-in ACES 2.0 OCIO configs (ACES 2.0 Studio v4.0, ACES 2.0 CG v4.0) map to the FX ACES 2.0 output transform; other OCIO configs have no FX colour management",
        ));
    }
    if working.as_deref() != Some(ACESCG) {
        return Some(unsupported(
            "only the ACEScg working colour space (the ACES 2.0 configs' scene_linear role) equals FX ACES compositing",
        ));
    }
    Some(ProjectColorContext::Aces { config })
}

/// The OCIO input colourspace that footage source `pin` (a `Pin ` list)
/// declares in its `CLRS` `ocsp`, if any.
pub(crate) fn footage_input(pin: &Chunk) -> Option<String> {
    let clrs = pin
        .children()?
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"CLRS"))?
        .children()?;
    let index = clrs.iter().position(|chunk| chunk.id() == *b"ocsp")?;
    let value = clrs.get(index + 1).filter(|chunk| chunk.id() == *b"Utf8")?;
    colorspace(&serde_json::from_slice(value.data_payload()?).ok()?)
}

/// The FX `cameraInput` encoding of OCIO input colourspace `colorspace`, or
/// why the footage keeps FX's default interpretation (`Err`), or `Ok(None)`
/// for a display colourspace, which FX's footage interpretation equals.
pub(crate) fn camera_input(colorspace: &str) -> Result<Option<&'static str>, String> {
    if let Some(camera) = fx_conv::aces::camera_for_aces_ocio_colorspace(colorspace) {
        return Ok(Some(camera));
    }
    if colorspace.ends_with(" - Display") {
        return Ok(None);
    }
    Err(format!(
        "OCIO input colourspace {colorspace:?} has no FX equivalent (FX ACES declares only camera inputs); footage decoded with FX's default display interpretation"
    ))
}

/// The OCIO footage declaration of FX camera encoding `encoding`, written in
/// the footage's `CLRS` `ocsp`.
pub(crate) fn footage_declaration(encoding: &str) -> Option<String> {
    let colorspace = fx_conv::aces::aces_ocio_colorspace(encoding)?;
    Some(declaration(camera_family(colorspace), colorspace))
}

/// The OCIO footage declaration of SDR footage without a camera input: the
/// display colourspace the ACES 2.0 configs' file rules assign on import
/// (`sRGB - Display` for stills, `Rec.1886 Rec.709 - Display` for movies), the
/// interpretation FX gives SDR images and video in ACES. An empty `ocsp`
/// would make AE read the footage values as ACEScg.
pub(crate) fn display_footage_declaration(still: bool) -> String {
    let colorspace = if still {
        "sRGB - Display"
    } else {
        "Rec.1886 Rec.709 - Display"
    };
    declaration("Display", colorspace)
}

/// The ACES studio config family of a camera input colourspace.
fn camera_family(colorspace: &str) -> &'static str {
    match colorspace {
        "Apple Log" => "Input/Apple",
        "ARRI LogC3 (EI800)" | "ARRI LogC4" => "Input/ARRI",
        "BMDFilm WideGamut Gen5" => "Input/BlackmagicDesign",
        "CanonLog3 CinemaGamut D55" => "Input/Canon",
        "V-Log V-Gamut" => "Input/Panasonic",
        "Log3G10 REDWideGamutRGB" => "Input/RED",
        _ => "Input/Sony",
    }
}

/// Turn a fresh generated project into an OCIO ACES project: 32 bpc, the
/// ACES 2.0 Studio config and the ACEScg working colour space.
pub(crate) fn configure_project(bytes: &mut Vec<u8>) -> Result<(), AepWriteError> {
    let mut project = Project::parse(bytes)?;
    let engine = json!({
        "colorManagementSystem": 1, "lutInterpolationMethod": 1,
        "ocioConfigurationFile": ACES_STUDIO_CONFIG
    })
    .to_string();
    let working = declaration("ACES", ACESCG);
    let (mut compact, mut expanded, mut engine_slot, mut working_slot) =
        (false, false, false, false);
    for index in 0..project.chunks.len() {
        match crate::hdr::set_32bpc(&mut project.chunks[index])? {
            Some(SettingsRecord::Compact) => {
                compact = true;
                continue;
            }
            Some(SettingsRecord::Expanded) => {
                expanded = true;
                continue;
            }
            None => {}
        }
        let value = match project.chunks[index].id() {
            [b'p', b'c', b'm', b's'] => {
                engine_slot = true;
                &engine
            }
            [b'P', b'w', b'C', b's'] => {
                working_slot = true;
                &working
            }
            _ => continue,
        };
        let next = project
            .chunks
            .get_mut(index + 1)
            .filter(|chunk| chunk.id() == *b"Utf8")
            .ok_or(AepWriteError::Invalid(
                "invalid native colour-management slot",
            ))?;
        *next = Chunk::data(*b"Utf8", value.as_bytes().to_vec())?;
    }
    if !(compact && expanded && engine_slot && working_slot) {
        return Err(AepWriteError::Invalid(
            "fresh OCIO project settings are incomplete",
        ));
    }
    *bytes = project.encode()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocio_declarations_name_their_colourspace() {
        let working = declaration("ACES", ACESCG);
        assert_eq!(
            working,
            r#"{"baseColorProfile":{"colorProfileData":"eyJjb2xvclNwYWNlMSI6IkFDRVNjZyJ9","colorProfileName":"ACES/ACEScg"},"baseProfileType":3}"#
        );
        assert_eq!(
            colorspace(&serde_json::from_str(&working).unwrap()).as_deref(),
            Some(ACESCG)
        );
        let footage = footage_declaration("camera:sony-slog3-sgamut3-cine:v1").unwrap();
        assert!(footage.contains(r#""colorProfileName":"Input/Sony/S-Log3 S-Gamut3.Cine""#));
        assert_eq!(
            camera_input(&colorspace(&serde_json::from_str(&footage).unwrap()).unwrap()),
            Ok(Some("camera:sony-slog3-sgamut3-cine:v1"))
        );
        assert_eq!(camera_input("Rec.1886 Rec.709 - Display"), Ok(None));
        assert!(camera_input("ACES2065-1").is_err());
        assert_eq!(footage_declaration("camera:nikon-nlog:v1"), None);
        // The native fixture's file-rule declarations, byte for byte.
        assert_eq!(
            display_footage_declaration(true),
            r#"{"baseColorProfile":{"colorProfileData":"eyJjb2xvclNwYWNlMSI6InNSR0IgLSBEaXNwbGF5In0=","colorProfileName":"Display/sRGB - Display"},"baseProfileType":3}"#
        );
        assert_eq!(
            display_footage_declaration(false),
            r#"{"baseColorProfile":{"colorProfileData":"eyJjb2xvclNwYWNlMSI6IlJlYy4xODg2IFJlYy43MDkgLSBEaXNwbGF5In0=","colorProfileName":"Display/Rec.1886 Rec.709 - Display"},"baseProfileType":3}"#
        );
    }
}

/// Import → export → import of the natively saved OCIO ACES fixture
/// `color_aces_ae_project.aep` through the public adapters.
#[cfg(test)]
mod fixture_tests {
    use std::fs;

    use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
    use serde_json::{Value, json};
    use tesseract_file::TesseractFile;

    use crate::{
        AfterEffectsExportOptions, AfterEffectsImportOptions,
        adapter::AfterEffects,
        structure::{ProjectColorContext, read_project},
    };

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/color_aces_ae_project.aep");
    const FIXTURE_SHA256: &str = "c71997e15c0fa990d8b87ac0e7923f11fdf7583c2fa61f7a52a2c74bfc207183";
    const CHART: &[u8] = include_bytes!("../../premiere_file/tests/fixtures/a1_bg_chart.png");
    const CLIP: &[u8] =
        include_bytes!("../../premiere_file/tests/fixtures/color_aces_slog3_clip.mp4");
    const SLOG3_CINE: &str = "camera:sony-slog3-sgamut3-cine:v1";
    const OUTPUT_NAME: &str = "project.tsrct";

    fn layers_of_type<'a>(layers: &'a Value, kind: &str, found: &mut Vec<&'a Value>) {
        for layer in layers.as_array().into_iter().flatten() {
            if layer["type"] == kind {
                found.push(layer);
            }
            layers_of_type(&layer["layers"], kind, found);
        }
    }

    fn primary_grades<'a>(value: &'a Value, grades: &mut Vec<&'a Value>) {
        match value {
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("primaryGrade") {
                    grades.push(value);
                }
                object
                    .values()
                    .for_each(|child| primary_grades(child, grades));
            }
            Value::Array(items) => items.iter().for_each(|child| primary_grades(child, grades)),
            _ => {}
        }
    }

    fn assert_aces_document(document: &Value) {
        assert_eq!(
            document["colorManagement"],
            json!({"workingSpace": "acescct", "outputTransform": "aces2.0", "compositing": "wholeProgram"})
        );
        let mut videos = Vec::new();
        layers_of_type(&document["composition"]["layers"], "Video", &mut videos);
        let [video] = videos[..] else {
            panic!("{document}");
        };
        assert_eq!(
            video["source"]["inputTransform"],
            json!({"semanticVersion": 1, "transform": {"type": "cameraInput", "inputEncoding": SLOG3_CINE}})
        );
        let mut images = Vec::new();
        layers_of_type(&document["composition"]["layers"], "Image", &mut images);
        let [image] = images[..] else {
            panic!("{document}");
        };
        assert!(image["source"].get("inputTransform").is_none(), "{image}");
    }

    #[test]
    fn native_ocio_aces_project_imports_as_an_fx_aces_document_with_a_camera_input_and_exports_the_project_policy()
     {
        use sha2::{Digest, Sha256};
        assert_eq!(format!("{:x}", Sha256::digest(FIXTURE)), FIXTURE_SHA256);
        let native = read_project(FIXTURE).unwrap();
        assert_eq!(
            native.color,
            ProjectColorContext::Aces {
                config: "ACES 2.0 Studio v4.0".into()
            }
        );
        // The native save's relative alias hint (base two levels above the
        // project directory, three trailing components) finds the footage in
        // `<job>/inputs/` for a project in `<job>/native/`.
        let root = tempfile::tempdir().unwrap();
        let job = root.path().join("job");
        fs::create_dir_all(job.join("native")).unwrap();
        fs::create_dir_all(job.join("inputs")).unwrap();
        let input = job.join("native/color_aces_ae_project.aep");
        fs::write(&input, FIXTURE).unwrap();
        fs::write(job.join("inputs/chart.png"), CHART).unwrap();
        fs::write(job.join("inputs/color_aces_slog3_clip.mp4"), CLIP).unwrap();
        let imported = root.path().join("imported");
        let report = AfterEffects
            .import_to_tesseract(
                &input,
                &imported,
                &AfterEffectsImportOptions {
                    composition: Some(1),
                    ..AfterEffectsImportOptions::default()
                },
                ConversionMode::Write,
            )
            .unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(
                    "OCIO colour management with the built-in \"ACES 2.0 Studio v4.0\" config"
                ) && diagnostic.message.contains("above the 2-code render gate")),
            "{:?}",
            report.diagnostics
        );
        let document = TesseractFile::open(imported.join(OUTPUT_NAME))
            .unwrap()
            .project_json()
            .unwrap();
        assert_aces_document(&document);
        // The fixture's Lumetri Exposure +1 imports by the ACES stops/2.2
        // scale, not the display-referred fitted stops/2.6.
        let mut grades = Vec::new();
        primary_grades(&document, &mut grades);
        let [grade] = grades[..] else {
            panic!("one primaryGrade expected: {grades:?}");
        };
        let exposure = grade["exposure"].as_f64().unwrap();
        assert!((exposure - 1.0 / 2.2).abs() < 1e-9, "{grade}");
        assert!(
            report.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains(crate::effects::color_grades::LUMETRI_ACES_EXPOSURE_APPROXIMATION)),
            "{:?}",
            report.diagnostics
        );

        let exported = root.path().join("exported");
        AfterEffects
            .export_from_tesseract(
                &imported.join(OUTPUT_NAME),
                &exported,
                &AfterEffectsExportOptions {
                    fps: 30.0,
                    hdr: None,
                },
                ConversionMode::Write,
            )
            .unwrap();
        // The fresh writer has no HEVC footage profile, so the clip is a
        // diagnosed omission here; `aces_camera_input_writes_the_footage_ocio_input_colourspace`
        // covers the camera declaration on supported footage.
        let written = read_project(&fs::read(exported.join("project.aep")).unwrap()).unwrap();
        assert_eq!(written.color, native.color);
        // The chart keeps the native file-rule interpretation, not raw ACEScg.
        let inputs: Vec<_> = written
            .items
            .iter()
            .filter_map(|item| item.ocio_input.as_deref())
            .collect();
        assert_eq!(inputs, ["sRGB - Display"]);
    }
}
