//! An edited FX vignette on the pinned Lumetri host: at 30 and 29.97 fps the
//! automatic hybrid keeps the native Premiere picture (the Lumetri Vignette's
//! declared approximation is not a routing loss), writes a native Lumetri
//! Color Vignette and rereads it without a neutral grade. No native pixel
//! proof or Adobe acceptance.
use super::{lumetri_round_trip::human_lumetri_document, *};

#[test]
fn human_lumetri_vignette_native_retained_export_writes_current_lumetri_controls() {
    let parent = tempfile::tempdir().unwrap();
    let mut document = human_lumetri_document(parent.path());
    document["composition"]["layers"][0]["effects"] = json!([{
        "id": 1, "enabled": true,
        "effect": {"type": "vignette", "amount": 0.444, "radius": 0.71625, "feather": 0.3845}
    }]);
    let archive = parent.path().join("edited.tsrct");
    archive_with_video(&document, &archive, "video-30fps-10s.mp4");
    for rate in [
        premiere_file::FrameRate::Fps30,
        premiere_file::FrameRate::Fps30000Over1001,
    ] {
        let output = parent
            .path()
            .join(format!("output-{}", rate.ticks_per_frame()));
        let report = export(
            &request(&archive, &output, ConversionMode::Write),
            &PremiereExportOptions {
                hdr: None,
                frame_rate: Some(rate),
            },
        )
        .unwrap();
        assert!(
            report.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains("vignette exports as a native Lumetri Color Vignette")),
            "{report:?}"
        );
        let prproj = output.join("project.prproj");
        let text = xml(&prproj);
        assert_eq!(
            text.matches("<MatchName>AE.ADBE Lumetri</MatchName>")
                .count(),
            1,
            "{rate}"
        );
        assert!(!text.contains(".aep"), "{rate}");
        // The fitted inverse of the import: Lumetri (-2, 50, 50) is FX
        // (0.444, 0.71625, 0.3845).
        for (name, saved) in [("Amount", "-2."), ("Midpoint", "50."), ("Feather", "50.")] {
            let document = roxmltree::Document::parse(&text).unwrap();
            let record = document
                .root_element()
                .children()
                .find(|node| {
                    node.has_tag_name("VideoComponentParam")
                        && node
                            .children()
                            .any(|child| child.has_tag_name("Name") && child.text() == Some(name))
                })
                .unwrap();
            let start = record
                .children()
                .find(|child| child.has_tag_name("StartKeyframe"))
                .and_then(|child| child.text())
                .unwrap();
            assert_eq!(start.split(',').nth(1), Some(saved), "{rate} {name}");
        }
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
        // The written Lumetri's neutral Basic Correction adds no grade.
        assert_eq!(effects.len(), 1, "{rate}: {effects:?}");
        let vignette = &effects[0]["effect"];
        assert_eq!(vignette["type"], "vignette");
        for (field, expected) in [("amount", 0.444), ("radius", 0.71625), ("feather", 0.3845)] {
            assert!(
                (vignette[field].as_f64().unwrap() - expected).abs() < 1e-9,
                "{rate} {field}: {vignette}"
            );
        }
    }
}
