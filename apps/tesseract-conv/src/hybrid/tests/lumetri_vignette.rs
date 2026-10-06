//! Native saved neutral Vignette and edited export; no native pixel proof.
use super::*;

#[test]
fn human_lumetri_vignette_edited_linked_export_writes_current_native_controls() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("native.prproj");
    let bytes = fs::read(fixture("human_lumetri_contrast.xml")).unwrap();
    fs::write(&input, &bytes).unwrap();
    fs::create_dir(parent.path().join("media")).unwrap();
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        parent.path().join("media/source.mp4"),
    )
    .unwrap();
    let imported = parent.path().join("imported");
    import_premiere(&request(&input, &imported, ConversionMode::Write)).unwrap();
    let original = TesseractFile::open(imported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        original["composition"]["layers"][0]["effects"][5]["effect"]["type"],
        "vignette"
    );
    for edited in [false, true] {
        let mut document = original.clone();
        if edited {
            document["composition"]["layers"][0]["effects"][5]["effect"] =
                json!({"type":"vignette", "amount":0.6,"radius":0.8,"feather":0.2});
        }
        let archive = parent.path().join(format!("edited-{edited}.tsrct"));
        archive_with_video(&document, &archive, "video-30fps-10s.mp4");
        let output = parent.path().join(format!("output-{edited}"));
        let report = export(
            &request(&archive, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("feather")
                    && diagnostic.message.contains("Pin Highlights")),
            "{report:?}"
        );
        assert!(xml(&output.join("project.prproj")).contains("./media/ae-0001/compositions.aep"));
        let native = aftereffects_file::aep::Project::parse(
            &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            numeric_properties(&native.chunks, "CS Vignette-0001")[0].values,
            [if edited { 60.0 } else { 0.0 }]
        );
        assert_eq!(
            numeric_properties(&native.chunks, "CS Vignette-0002")[0].values,
            [if edited { 48.0 } else { 30.0 }]
        );
    }
    assert_eq!(fs::read(input).unwrap(), bytes);
}
