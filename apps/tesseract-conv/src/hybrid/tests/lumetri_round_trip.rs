//! FX grade and vignette stacks through CLI export (default 30 fps) and a
//! fresh import of the written project. The automatic hybrid keeps every
//! stack in the native Premiere picture, with no linked After Effects scope;
//! a grade directly followed by a vignette shares one Lumetri Color, and
//! reimport adds no neutral companion section, so each stack rereads as
//! itself. Structural only; Premiere has not opened these exports.
use super::*;

/// The pinned `human_lumetri_contrast.xml` host imported through the CLI,
/// with its video layer's effects and every animation removed.
pub(super) fn human_lumetri_document(parent: &Path) -> Value {
    let source = parent.join("source.prproj");
    let bytes = fs::read(fixture("human_lumetri_contrast.xml")).unwrap();
    fs::write(&source, &bytes).unwrap();
    fs::create_dir(parent.join("media")).unwrap();
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        parent.join("media/source.mp4"),
    )
    .unwrap();
    let imported = parent.join("imported");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let mut document = TesseractFile::open(imported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    document["composition"]["layers"][0]["effects"] = json!([]);
    document["composition"]["dynamics"]["entries"] = json!([]);
    assert_eq!(fs::read(source).unwrap(), bytes);
    document
}

fn grade(id: u64) -> Value {
    json!({"id": id, "enabled": true, "effect": {
        "type": "primaryGrade", "semanticVersion": 1, "temperature": 0.1, "tint": -0.05,
        "exposure": 0.454545455, "contrast": 0.25, "highlights": -0.25, "shadows": 0.25,
        "whites": 0.15, "blacks": -0.15, "saturation": 0.3, "vibrance": 0.2
    }})
}

fn vignette(id: u64) -> Value {
    json!({"id": id, "enabled": true, "effect": {
        "type": "vignette", "amount": 0.444, "radius": 0.71625, "feather": 0.3845
    }})
}

/// [`grade`] whose static Contrast is its first key's, as Premiere saves a
/// keyed parameter.
fn keyed_grade(id: u64) -> Value {
    let mut grade = grade(id);
    grade["effect"]["contrast"] = json!(0.2);
    grade
}

/// A linear Contrast key track on effect `id`: 0.2 at 0 ms, -0.4 at 1000 ms.
fn contrast_keys(id: u64) -> Value {
    json!({
        "target": {"kind": "effectProperty", "effectId": id, "paramName": "contrast"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "contrast-start", "layerTime": 0, "value": {"type": "float", "value": 0.2}, "easing": {"type": "linear"}},
            {"id": "contrast-end", "layerTime": 1000, "value": {"type": "float", "value": -0.4}, "easing": {"type": "linear"}}
        ]}
    })
}

/// Every numeric control of `expected` is within 1e-8 of `actual`'s.
fn assert_controls(actual: &Value, expected: &Value, case: &str) {
    assert_eq!(actual["type"], expected["type"], "{case}: {actual}");
    for (name, value) in expected.as_object().unwrap() {
        if let Some(value) = value.as_f64() {
            let reread = actual[name]
                .as_f64()
                .unwrap_or_else(|| panic!("{case} {name}: {actual}"));
            assert!(
                (reread - value).abs() < 1e-8,
                "{case} {name}: {reread} != {value}"
            );
        }
    }
}

#[test]
fn lumetri_grade_and_vignette_stacks_export_natively_at_30_fps_and_reread_as_themselves() {
    let parent = tempfile::tempdir().unwrap();
    let base = human_lumetri_document(parent.path());
    // (case, FX stack, animation entries, written Lumetri Color count)
    let cases = [
        ("grade", json!([grade(1)]), json!([]), 1),
        ("vignette", json!([vignette(1)]), json!([]), 1),
        (
            "grade-vignette",
            json!([grade(1), vignette(2)]),
            json!([]),
            1,
        ),
        (
            "vignette-grade",
            json!([vignette(1), grade(2)]),
            json!([]),
            2,
        ),
        (
            "keyed-grade",
            json!([keyed_grade(1)]),
            json!([contrast_keys(1)]),
            1,
        ),
    ];
    for (case, effects, entries, components) in cases {
        let mut document = base.clone();
        document["composition"]["layers"][0]["effects"] = effects.clone();
        document["composition"]["dynamics"]["entries"] = entries.clone();
        let input = parent.path().join(format!("{case}.tsrct"));
        archive_with_video(&document, &input, "video-30fps-10s.mp4");
        let output = parent.path().join(format!("{case}-exported"));
        // Default options: a 30 fps sequence, where linked AE scopes are exact.
        let report = export(
            &request(&input, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        assert!(
            !report.diagnostics.iter().any(
                |note| note.message.contains("not exported") || note.code.starts_with("HYBRID")
            ),
            "{case}: {report:?}"
        );
        let prproj = output.join("project.prproj");
        let text = xml(&prproj);
        assert_eq!(
            text.matches("<MatchName>AE.ADBE Lumetri</MatchName>")
                .count(),
            components,
            "{case}"
        );
        assert!(!text.contains(".aep"), "{case}");
        assert!(!output.join("media/ae-0001").exists(), "{case}");

        let reimported = parent.path().join(format!("{case}-reimported"));
        import_premiere(&request(&prproj, &reimported, ConversionMode::Write)).unwrap();
        let again = TesseractFile::open(reimported.join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap();
        let reread = again["composition"]["layers"][0]["effects"]
            .as_array()
            .unwrap();
        let expected = effects.as_array().unwrap();
        assert_eq!(
            reread
                .iter()
                .map(|effect| effect["effect"]["type"].clone())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|effect| effect["effect"]["type"].clone())
                .collect::<Vec<_>>(),
            "{case}"
        );
        for (reread, expected) in reread.iter().zip(expected) {
            assert_eq!(reread["enabled"], true, "{case}");
            assert_controls(&reread["effect"], &expected["effect"], case);
        }
        let entries = again["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        if case == "keyed-grade" {
            assert_eq!(entries.len(), 1, "{entries:?}");
            assert_eq!(entries[0]["target"]["paramName"], "contrast");
            assert_eq!(entries[0]["target"]["effectId"], reread[0]["id"]);
            let keys = entries[0]["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2, "{keys:?}");
            for (key, (time, value)) in keys.iter().zip([(0, 0.2), (1000, -0.4)]) {
                assert!(
                    (key["layerTime"].as_i64().unwrap() - time).abs() <= 1,
                    "{key}"
                );
                assert!(
                    (key["value"]["value"].as_f64().unwrap() - value).abs() < 1e-8,
                    "{key}"
                );
            }
        } else {
            assert!(entries.is_empty(), "{case}: {entries:?}");
        }
    }
}
