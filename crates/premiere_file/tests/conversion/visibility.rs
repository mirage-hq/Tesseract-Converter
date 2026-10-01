//! Clip Enable and track video output through the public conversion API.
use super::support::*;
use premiere_file::PrProjectFile;
use serde_json::{json, Value};
use std::path::Path;
use tesseract_file::TesseractFile;

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const RED: &str = "video-30fps-10s.mp4";
const BLUE: &str = "feature_two_tracks_gap_clip_b.mp4";

// Derived from the Adobe-authored `feature_two_tracks_gap_strict.prproj`. V1 shows the red
// source 0–10 s. Every hidden placement uses the blue source, so showing one would change
// the picture: V2 at 3–7 s and 10–11 s (Enable off; the second ends after V1) and V3 at
// 1–2 s (track output off). Its AME render (`premiere_isolated_clip_disabled`) shows no
// blue and lasts 11.000 s.
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/premiere_isolated_clip_disabled.prproj")
}

fn media_count(project: &PrProjectFile) -> usize {
    project
        .sequences()
        .flat_map(|sequence| sequence.video_occurrences())
        .map(|clip| clip.media_id())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

/// File name of the packaged asset a layer plays.
fn asset_file_name(file: &TesseractFile, layer: &Value) -> String {
    let asset_id = layer["source"]["assetId"].as_str().unwrap();
    Path::new(&file.metadata().assets[asset_id].path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap()
        .to_owned()
}

/// Sorted `(timeline ticks, source ticks)` rows, independent of track placement.
fn occurrence_rows(project: &PrProjectFile) -> Vec<(std::ops::Range<i64>, std::ops::Range<i64>)> {
    let mut rows: Vec<_> = project
        .sequences()
        .next()
        .unwrap()
        .video_occurrences()
        .map(|clip| (clip.timeline_ticks(), clip.source_ticks()))
        .collect();
    rows.sort_by_key(|row| row.0.start);
    rows
}

#[test]
fn disabled_clip_and_muted_track_import_hidden_and_export_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let native = PrProjectFile::load(fixture()).unwrap().0;
    assert_eq!(media_count(&native), 2);
    assert_eq!(
        occurrence_rows(&native),
        [
            (0..10 * TICKS, 0..10 * TICKS),
            (TICKS..2 * TICKS, 5 * TICKS..6 * TICKS),
            (3 * TICKS..7 * TICKS, 0..4 * TICKS),
            (10 * TICKS..11 * TICKS, 8 * TICKS..9 * TICKS),
        ]
    );
    let per_track: Vec<usize> = native
        .sequences()
        .next()
        .unwrap()
        .video_tracks()
        .map(<[_]>::len)
        .collect();
    assert_eq!(per_track, [1, 2, 1]);

    let output = dir.path().join("converted");
    premiere_to_tesseract(fixture(), &output, Some(SEQUENCE), false).unwrap();
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let file = TesseractFile::open(&projects[0]).unwrap();
    assert_eq!(file.metadata().assets.len(), 2);
    let document = file.project_json().unwrap();
    // Disabled clips count toward the end, so the 10–11 s clip lengthens the document.
    assert_eq!(document["duration"], 11.0);
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 4);
    let by_start = |start: i64| {
        layers
            .iter()
            .find(|layer| (*crate::test_support::layer_range(layer))["start"] == json!(start))
            .unwrap()
    };
    let visible = by_start(0);
    let disabled = by_start(3000);
    let disabled_tail = by_start(10000);
    let muted_track = by_start(1000);
    assert_ne!(visible["isHidden"], json!(true));
    assert_eq!(asset_file_name(&file, visible), RED);
    for hidden in [disabled, disabled_tail, muted_track] {
        assert_eq!(hidden["isHidden"], json!(true));
        assert_eq!(asset_file_name(&file, hidden), BLUE);
        assert_eq!(hidden["source"]["assetId"], disabled["source"]["assetId"]);
        assert_eq!(hidden["transform"], visible["transform"]);
        assert_eq!(hidden["source"]["fit"], visible["source"]["fit"]);
    }
    assert_eq!(
        disabled["sourceRange"],
        json!({"start": 0, "duration": 4000})
    );
    assert_eq!(
        disabled_tail["sourceRange"],
        json!({"start": 8000, "duration": 1000})
    );
    assert_eq!(
        muted_track["sourceRange"],
        json!({"start": 5000, "duration": 1000})
    );
    let canvas = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(canvas["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(canvas)),
        json!({"start": 0, "duration": 11000})
    );

    let rebuilt_dir = dir.path().join("rebuilt");
    tesseract_to_premiere(&projects[0], &rebuilt_dir, false).unwrap();
    let xml = read_xml(&rebuilt_dir.join("project.prproj"));
    let document = roxmltree::Document::parse(&xml).unwrap();
    let muted_owners: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("IsMuted"))
        .map(|node| node.parent().unwrap().tag_name().name())
        .collect();
    // Hidden layers become disabled clips; track output is not reconstructed.
    assert_eq!(muted_owners, ["ClipTrackItem"; 3]);
    // The disabled clips are the three hidden placements, read from the XML by start.
    let muted = document
        .descendants()
        .filter(|node| node.has_tag_name("ClipTrackItem"))
        .map(|item| item.children().any(|child| child.has_tag_name("IsMuted")));
    let mut starts: Vec<_> = track_item_ticks(&document, "Start")
        .into_iter()
        .zip(muted)
        .collect();
    starts.sort_unstable();
    assert_eq!(
        starts,
        [
            (0, false),
            (TICKS, true),
            (3 * TICKS, true),
            (10 * TICKS, true)
        ]
    );
    let rebuilt = PrProjectFile::load(rebuilt_dir.join("project.prproj"))
        .unwrap()
        .0;
    assert_eq!(occurrence_rows(&rebuilt), occurrence_rows(&native));
    assert_eq!(media_count(&rebuilt), 2);
    // Export re-lanes clips onto the lowest free track: three native tracks become two.
    let rebuilt_tracks = rebuilt.sequences().next().unwrap().video_tracks().len();
    assert_eq!(rebuilt_tracks, 2);
}
