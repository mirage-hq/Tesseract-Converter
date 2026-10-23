//! Brightness & Contrast in both "Use Legacy (supports HDR)" modes from the
//! natively authored `color_ae_brightness_contrast_modes.aep` (AE 26.5, 8 bpc,
//! working space None; provenance in `color_ae_fixtures.provenance.json`):
//! ten 64 px bands of one horizontal Gradient Ramp, nine of them followed by
//! Brightness & Contrast with the values below. Import keeps the native units
//! and names the legacy mode; export writes Use Legacy off.
use super::*;

const SHA256: &str = "38dbbb12c8407c274650d7afcd54ce05dd97933a15a9b13ebb292b46e868cc56";

/// Band layer name, Brightness, Contrast, Use Legacy.
const BANDS: [(&str, f64, f64, f64); 9] = [
    ("B0 C0 Legacy", 0.0, 0.0, 1.0),
    ("B40 C0 Legacy", 40.0, 0.0, 1.0),
    ("B-40 C0 Legacy", -40.0, 0.0, 1.0),
    ("B0 C50 Legacy", 0.0, 50.0, 1.0),
    ("B0 C-50 Legacy", 0.0, -50.0, 1.0),
    ("B40 C30 Legacy", 40.0, 30.0, 1.0),
    ("B40 C30 Default", 40.0, 30.0, 0.0),
    ("B0 C50 Default", 0.0, 50.0, 0.0),
    ("B40 C0 Default", 40.0, 0.0, 0.0),
];

const LEGACY_WARNING: &str = "Use Legacy (supports HDR) is on: the native legacy mode adds Brightness/255, then scales about 0.5 by 100/(100 - Contrast) for positive Contrast or (100 + Contrast)/100 otherwise";

fn native_controls(layer: &Layer) -> Option<[f64; 3]> {
    let (effects, _) = crate::effects::native::read_effects(&layer.content, [1024.0, 64.0]);
    let effect = effects
        .iter()
        .find(|effect| effect.match_name == "ADBE Brightness & Contrast 2")?;
    let value = |suffix: &str| {
        let parameter = effect
            .parameters
            .iter()
            .find(|parameter| parameter.match_name.ends_with(suffix))
            .unwrap_or_else(|| panic!("{suffix} missing"));
        let numeric = parameter.numeric.as_ref().unwrap();
        assert!(!numeric.animated && !numeric.expression_enabled);
        numeric.values[0]
    };
    Some([value("-0001"), value("-0002"), value("-0003")])
}

fn layers(project: &StructuralProject) -> Vec<&Layer> {
    project
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.layers.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn imported() -> (StructuralProject, StructuralConversion) {
    let bytes = include_bytes!("../../../tests/fixtures/color_ae_brightness_contrast_modes.aep");
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), SHA256);
    let project = read_project(bytes).unwrap();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    (project, converted)
}

fn imported_effects(value: &Value) -> Vec<(String, f64, f64)> {
    fn walk(value: &Value, found: &mut Vec<(String, f64, f64)>) {
        for layer in value["layers"].as_array().into_iter().flatten() {
            for record in layer["effects"].as_array().into_iter().flatten() {
                if record["effect"]["type"] == "brightnessContrast" {
                    found.push((
                        layer["name"].as_str().unwrap().to_owned(),
                        record["effect"]["brightness"].as_f64().unwrap(),
                        record["effect"]["contrast"].as_f64().unwrap(),
                    ));
                }
            }
            walk(layer, found);
        }
    }
    let mut found = Vec::new();
    walk(&value["composition"], &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn expected_effects() -> Vec<(String, f64, f64)> {
    let mut expected: Vec<_> = BANDS
        .iter()
        .map(|(name, brightness, contrast, _)| ((*name).to_owned(), *brightness, *contrast))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    expected
}

#[test]
fn native_brightness_contrast_modes_import_with_the_legacy_mode_named() {
    let (project, converted) = imported();
    // The authored controls, read natively (provenance readback values).
    for (name, brightness, contrast, legacy) in BANDS {
        let layer = layers(&project)
            .into_iter()
            .find(|layer| &*layer.name == name)
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!(
            native_controls(layer),
            Some([brightness, contrast, legacy]),
            "{name}"
        );
    }
    let value = converted.document.to_json_value().unwrap();
    assert_eq!(imported_effects(&value), expected_effects());
    let messages: Vec<&str> = converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    let count = |needle: &str| messages.iter().filter(|m| m.contains(needle)).count();
    assert_eq!(count(LEGACY_WARNING), 6, "{messages:#?}");
    assert_eq!(
        count("FX brightnessContrast keeps the native Brightness/Contrast units"),
        9,
        "{messages:#?}"
    );
    assert_eq!(
        count("ADBE Brightness & Contrast 2-0003"),
        0,
        "{messages:#?}"
    );
    assert_eq!(count("Legacy/HDR mode is not modeled"), 0, "{messages:#?}");
}

#[test]
fn brightness_contrast_export_writes_use_legacy_off_with_the_fx_values() {
    let (_, converted) = imported();
    let exported = crate::export_document::to_aep(&converted.document).unwrap();
    assert!(
        exported
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Export writes Use Legacy off")),
        "{:#?}",
        exported.diagnostics
    );
    let project = read_project(&exported.bytes).unwrap();
    let mut written: Vec<_> = layers(&project)
        .into_iter()
        .filter_map(|layer| {
            native_controls(layer).map(|controls| (layer.name.to_string(), controls))
        })
        .collect();
    written.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(written.len(), BANDS.len(), "{written:?}");
    for ((name, controls), (expected_name, brightness, contrast)) in
        written.iter().zip(expected_effects())
    {
        assert_eq!(name, &expected_name);
        assert_eq!(controls, &[brightness, contrast, 0.0], "{name}");
    }
    // The fresh import of our export keeps the values and reports no legacy mode.
    let reimported = to_structural_fx_document(&project, Some(exported.root.item_id())).unwrap();
    assert_eq!(
        imported_effects(&reimported.document.to_json_value().unwrap()),
        expected_effects()
    );
    assert!(
        !reimported
            .diagnostics
            .iter()
            .any(|d| d.message.contains(LEGACY_WARNING))
    );
}
