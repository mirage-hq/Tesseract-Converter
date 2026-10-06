//! Source-backed structural coverage of the existing linked-AEP route, not
//! independent Adobe acceptance or temporal-render fidelity.
use super::*;
use crate::formats::import_after_effects;
use sha2::{Digest, Sha256};

fn pixel_motion_effects(layers: &[Value]) -> Vec<&Value> {
    let mut effects = Vec::new();
    for layer in layers {
        if let Some(records) = layer["effects"].as_array() {
            effects.extend(
                records
                    .iter()
                    .filter(|record| record["effect"]["type"] == "pixelMotionBlur")
                    .map(|record| &record["effect"]),
            );
        }
        if let Some(children) = layer["layers"].as_array() {
            effects.extend(pixel_motion_effects(children));
        }
    }
    effects
}

fn edit_pixel_motion(layers: &mut [Value], payload: &Value) -> usize {
    let mut count = 0;
    for layer in layers {
        if let Some(records) = layer.get_mut("effects").and_then(Value::as_array_mut) {
            for record in records {
                if record["effect"]["type"] == "pixelMotionBlur" {
                    record["effect"] = payload.clone();
                    count += 1;
                }
            }
        }
        if let Some(children) = layer.get_mut("layers").and_then(Value::as_array_mut) {
            count += edit_pixel_motion(children, payload);
        }
    }
    count
}

#[test]
fn native_pixel_motion_blur_import_and_edited_linked_export_keep_controls() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../crates/aftereffects_file/tests/fixtures/effects_coverage/native_static_controls.aep",
    );
    let bytes = fs::read(&source).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "7c65ebe724fc8403399979cc1354adca3abd737b6ad7a76ba8eafc5b61ccf95b"
    );
    let parent = tempfile::tempdir().unwrap();
    let imported = parent.path().join("native-import");
    let mut native_request = request(&source, &imported, ConversionMode::Write);
    native_request.composition = Some(391);
    import_after_effects(&native_request).unwrap();
    let archive = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let mut document = archive.project_json().unwrap();
    let original = json!({
        "type":"pixelMotionBlur", "shutterControl":"manual",
        "shutterAngle":120.0, "shutterSamples":8.0, "vectorDetail":20.0
    });
    assert_eq!(
        pixel_motion_effects(document["composition"]["layers"].as_array().unwrap()),
        vec![&original]
    );

    for (name, expected) in [
        ("original", original),
        (
            "edited",
            json!({
                "type":"pixelMotionBlur", "shutterControl":"manual",
                "shutterAngle":240.0, "shutterSamples":16.0, "vectorDetail":40.0
            }),
        ),
    ] {
        assert_eq!(
            edit_pixel_motion(
                document["composition"]["layers"].as_array_mut().unwrap(),
                &expected
            ),
            1
        );
        let input = parent.path().join(format!("{name}.tsrct"));
        // The selected native composition contains solids only. The new archive
        // carries editable content, not the original AEP or rendered media.
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .write(&input)
            .unwrap();
        let output = parent.path().join(name);
        let checked = export(
            &request(&input, &output, ConversionMode::Check),
            &Default::default(),
        )
        .unwrap();
        assert!(!output.exists());
        let written = export(
            &request(&input, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(checked, written);
        assert!(written
            .diagnostics
            .iter()
            .any(|d| d.code == "HYBRID-EXPERIMENTAL"));
        let prproj = output.join("project.prproj");
        assert!(xml(&prproj).contains("./media/ae-0001/compositions.aep"));
        let reimported = parent.path().join(format!("{name}-reimported"));
        import_premiere(&request(&prproj, &reimported, ConversionMode::Write)).unwrap();
        let result = TesseractFile::open(reimported.join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap();
        assert_eq!(
            pixel_motion_effects(result["composition"]["layers"].as_array().unwrap()),
            vec![&expected]
        );
        assert!(!result.to_string().contains("JsScript"));
    }
    assert_eq!(fs::read(source).unwrap(), bytes);
}
