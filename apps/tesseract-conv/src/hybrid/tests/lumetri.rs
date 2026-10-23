//! Saved native Lumetri controls through the CLI: import to one editable
//! primaryGrade, then edited export. At 30 and 29.97 fps the automatic hybrid
//! keeps the native Premiere picture: the Lumetri grade's declared scale
//! approximation is not a routing loss, as the linked After Effects route
//! would write the same Lumetri controls. The written Lumetri Color is
//! inspected and reimported. Structural only; not Adobe acceptance or colour
//! fidelity.
use super::*;

#[test]
fn human_lumetri_import_keeps_primary_grade_keys_and_edited_export_writes_native_lumetri() {
    let parent = tempfile::tempdir().unwrap();
    let source = parent.path().join("source.prproj");
    let bytes = fs::read(fixture("human_lumetri_contrast.xml")).unwrap();
    fs::write(&source, &bytes).unwrap();
    fs::create_dir(parent.path().join("media")).unwrap();
    // The host declares 1920x1080/30fps/10s; supply matching synthetic footage,
    // not a Lumetri oracle or a reauthored native component.
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        parent.path().join("media/source.mp4"),
    )
    .unwrap();
    let imported = parent.path().join("imported");
    let report = import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("one editable primaryGrade")),
        "{report:?}"
    );
    let archive = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let mut document = archive.project_json().unwrap();
    let effects = document["composition"]["layers"][0]["effects"]
        .as_array_mut()
        .unwrap();
    // The pinned Vignette section is at its defaults: no vignette.
    assert_eq!(effects.len(), 1);
    let grade = &mut effects[0]["effect"];
    assert_eq!(grade["type"], "primaryGrade");
    assert_eq!(grade["semanticVersion"], 1);
    // 1 stop divided by 2.6 (the fitted Lumetri scale), as stored.
    assert_eq!(grade["exposure"], 0.384615385);
    assert_eq!(grade["contrast"], 0.069444444);
    assert_eq!(grade["saturation"], 0.272727273);
    grade["contrast"] = json!(0.25);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["target"]["paramName"], "saturation");
    let keys = entries[0]["animator"]["keyframes"].as_array_mut().unwrap();
    assert_eq!(keys[0]["value"]["value"], 0.272727273);
    assert_eq!(keys[1]["value"]["value"], -0.363636364);
    assert_eq!(keys[1]["layerTime"], 2513);
    keys[1]["value"]["value"] = json!(-0.65);

    let input = parent.path().join("edited.tsrct");
    archive_with_video(&document, &input, "video-30fps-10s.mp4");
    for rate in [
        premiere_file::FrameRate::Fps30,
        premiere_file::FrameRate::Fps30000Over1001,
    ] {
        let output = parent
            .path()
            .join(format!("exported-{}", rate.ticks_per_frame()));
        let report = export(
            &request(&input, &output, ConversionMode::Write),
            &PremiereExportOptions {
                hdr: None,
                frame_rate: Some(rate),
            },
        )
        .unwrap();
        assert!(
            report.diagnostics.iter().any(|note| note
                .message
                .contains("primaryGrade exports as a native Lumetri Color")),
            "{rate}: {report:?}"
        );
        assert!(
            !report.diagnostics.iter().any(
                |note| note.message.contains("not exported") || note.code.starts_with("HYBRID")
            ),
            "{rate}: {report:?}"
        );
        let prproj = output.join("project.prproj");
        let text = xml(&prproj);
        // One native Lumetri Color; no linked After Effects scope.
        assert_eq!(
            text.matches("<MatchName>AE.ADBE Lumetri</MatchName>")
                .count(),
            1,
            "{rate}"
        );
        assert!(!text.contains(".aep"), "{rate}");
        assert!(!output.join("media/ae-0001").exists(), "{rate}");
        assert!(!text.contains("JsScript"));
        // The edited Contrast 0.25 is saved as Contrast 90.
        assert!(text.contains("<StartKeyframe>-91445760000000000,90.,0,0,0,0,0,0</StartKeyframe>"));

        let reimported = parent
            .path()
            .join(format!("reimported-{}", rate.ticks_per_frame()));
        import_premiere(&request(&prproj, &reimported, ConversionMode::Write)).unwrap();
        let again = TesseractFile::open(reimported.join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap();
        let effects = again["composition"]["layers"][0]["effects"]
            .as_array()
            .unwrap();
        // The written Lumetri's neutral Vignette section adds no effect.
        assert_eq!(effects.len(), 1, "{rate}: {effects:?}");
        let grade = &effects[0]["effect"];
        assert_eq!(grade["type"], "primaryGrade");
        assert!(
            (grade["contrast"].as_f64().unwrap() - 0.25).abs() < 1e-9,
            "{grade}"
        );
        assert!(
            (grade["exposure"].as_f64().unwrap() - 0.384615385).abs() < 1e-8,
            "{grade}"
        );
        assert!(
            (grade["saturation"].as_f64().unwrap() - 0.272727273).abs() < 1e-9,
            "{grade}"
        );
        let entries = again["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["target"]["paramName"], "saturation");
        let keys = entries[0]["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert!((keys[0]["value"]["value"].as_f64().unwrap() - 0.272727273).abs() < 1e-9);
        assert!((keys[1]["value"]["value"].as_f64().unwrap() + 0.65).abs() < 1e-9);
        assert!((keys[1]["layerTime"].as_i64().unwrap() - 2513).abs() <= 1);
    }
    assert_eq!(fs::read(source).unwrap(), bytes);
}
