use std::{fs, path::Path};

use fx_conv::{ConversionMode, ExportFromTesseract};
use sha2::{Digest, Sha256};
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::*;
use crate::{AfterEffects, AfterEffectsExportOptions};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/bframe_presentation"
);

#[test]
fn b_frame_mov_exports_unchanged_and_responds_to_authored_rate_edit() {
    let movie = Path::new(FIXTURES).join("movie.mov");
    let source_bytes = fs::read(&movie).unwrap();
    // Expected controls were independently authored/read in AE, not inferred
    // from the converter. The original and edited inputs own the same footage.
    let native: Value =
        serde_json::from_slice(&fs::read(Path::new(FIXTURES).join("provenance.json")).unwrap())
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&source_bytes)),
        native["media_sha256"].as_str().unwrap()
    );
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(fs::read(Path::new(FIXTURES).join("native-control.aep")).unwrap())
        ),
        native["source_sha256"].as_str().unwrap()
    );
    assert_eq!(native["readback"]["footage"]["frameRate"], 24);
    assert_eq!(native["readback"]["footage"]["duration"], 1);

    for (index, (name, duration_ms, stretch)) in [("original", 1_000, 1.0), ("edited", 500, 0.5)]
        .into_iter()
        .enumerate()
    {
        let root = tempfile::tempdir().unwrap();
        let input_range = json!({"start": 0, "duration": duration_ms});
        let source_range = json!({"start": 0, "duration": 1000});
        let mut value = imported();
        value["duration"] = json!(f64::from(duration_ms) / 1000.0);
        value["dimensions"] = json!({"width": 160, "height": 90});
        value["composition"]["name"] = json!(name);
        value["composition"]["layers"] = json!([{
            "type": "Video", "id": 884, "name": "B-frame moving control",
            "sourceRange": source_range,
            "sourceIntrinsicDuration": 1000,
            "playback": fixture_linear_playback(input_range, source_range),
            "volume": 0,
            "transform": {
                // FX media is centered in the viewport before owner transforms.
                // Identity placement matches the independently authored full canvas.
                "position": [0, 0], "anchorPoint": [0, 0], "scale": [100, 100],
                "rotation": 0, "opacity": 100
            },
            "source": {"assetId": "movie", "fit": "contain"}
        }]);
        value["composition"]["dynamics"] = json!({"entries": []});
        let archive_path = root.path().join("input.tsrct");
        drop(
            TesseractFileBuilder::try_new(
                EditableFxCompositionDocument::from_json_value(value).unwrap(),
            )
            .unwrap()
            .add_asset("movie", &movie, AssetKind::Video)
            .unwrap()
            .write(&archive_path)
            .unwrap(),
        );
        let output = root.path().join("output");
        let report = AfterEffects
            .export_from_tesseract(
                &archive_path,
                &output,
                &AfterEffectsExportOptions { fps: 30.0 },
                ConversionMode::Write,
            )
            .unwrap();
        assert!(
            report.diagnostics.iter().all(|entry| {
                entry.layer_id.is_none()
                    || entry.message.starts_with("Audio gain at/below -192 dB")
                    || entry
                        .message
                        .starts_with("Video rounded-millisecond visibility cannot be represented:")
            }),
            "unexpected media omission/approximation: {:?}",
            report.diagnostics
        );
        let project = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("expected root composition");
        };
        assert_eq!(comp.layers.len(), 1);
        assert_eq!(comp.duration_secs, f64::from(duration_ms) / 1000.0);
        let layer = &comp.layers[0];
        assert_eq!(layer.record.start_time(), Some(0.0));
        assert_eq!(layer.record.in_point(), Some(0.0));
        assert_eq!(layer.record.out_point(), Some(1.0));
        assert_eq!(layer.record.stretch(), Some(stretch));
        assert_eq!(
            native["readback"]["controls"][index]["stretch"]
                .as_f64()
                .unwrap(),
            stretch * 100.0
        );
        let sources = project
            .items
            .iter()
            .filter_map(|item| item.media.as_ref())
            .map(|source| source.as_ref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].native_frame_rate.integer, 24);
        assert_eq!(sources[0].duration.seconds(), 1.0);
        assert_eq!(
            fs::read(output.join(&sources[0].authored_path)).unwrap(),
            source_bytes
        );
    }
}
