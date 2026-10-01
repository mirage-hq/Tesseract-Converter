use super::support::*;
use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
use premiere_file::{
    Omission, OmissionKind, OmissionScope, PrProjectFile, PrSequence, PrVideoItem, Premiere,
    PremiereImportOptions,
};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const CLIP_A: &[u8] = include_bytes!("../fixtures/feature_two_tracks_gap_clip_a.mp4");
const CLIP_B: &[u8] = include_bytes!("../fixtures/feature_two_tracks_gap_clip_b.mp4");
/// The two root sequences of `two-video-tracks.prproj`.
const TWO_TRACKS_OVERLAP: &str = "1592feef-89df-4c40-ba9d-fe9088c8f4a5";
const TWO_TRACKS_GAP: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";

// This Adobe-authored two-track fixture has only its absolute media aliases removed.
// Rust tests stage pixel-distinct red and blue clips under those relative aliases.
// Adobe save/reopen and rendered-output checks remain separate evidence.
fn two_video_tracks_fixture(root: &Path) -> std::path::PathBuf {
    fs::create_dir_all(root.join("media")).unwrap();
    for (name, media) in [("clip-a.mp4", CLIP_A), ("clip-b.mp4", CLIP_B)] {
        fs::write(root.join("media").join(name), media).unwrap();
    }
    let path = root.join("two-video-tracks.prproj");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/two-video-tracks.prproj"),
        &path,
    )
    .unwrap();
    path
}

fn sequence_table(project: &PrProjectFile, sequence: &PrSequence) -> Value {
    let occurrences: Vec<_> = sequence
        .video_tracks()
        .enumerate()
        .flat_map(|(track_index, track)| {
            track
                .iter()
                .filter_map(|item| match item {
                    PrVideoItem::Media(clip) => Some(clip),
                    PrVideoItem::Graphic(_) => None,
                })
                .map(move |clip| {
                    let timeline = clip.timeline_ticks();
                    let source = clip.source_ticks();
                    json!({
                        "track_index": track_index,
                        "media_name": project.media(clip).unwrap().name(),
                        "timeline_ticks": [timeline.start, timeline.end],
                        "source_ticks": [source.start, source.end],
                    })
                })
        })
        .collect();
    json!({
        "name": sequence.name(),
        "video_track_count": sequence.video_tracks().len(),
        "occurrences": occurrences,
    })
}

fn converted_adobe_sequence(root: &Path, sequence: &str) -> TesseractFile {
    let output = root.join("converted");
    premiere_to_tesseract(
        two_video_tracks_fixture(root),
        &output,
        Some(sequence),
        false,
    )
    .unwrap();
    TesseractFile::open(first_project(&output)).unwrap()
}

fn layer_uses_media(file: &TesseractFile, layer: &Value, file_name: &str) -> bool {
    let asset_id = layer["source"]["assetId"].as_str().unwrap();
    Path::new(&file.metadata().assets[asset_id].path)
        .file_name()
        .and_then(|name| name.to_str())
        == Some(file_name)
}

#[test]
fn premiere_to_tesseract_preserves_trimmed_source_range() {
    let dir = tempfile::tempdir().unwrap();
    let file = converted_adobe_sequence(dir.path(), TWO_TRACKS_OVERLAP);
    let document = file.project_json().unwrap();
    let clip = video_layers(&document)
        .into_iter()
        .find(|layer| layer_uses_media(&file, layer, "clip-b.mp4"))
        .unwrap();

    assert_eq!(
        (*crate::test_support::layer_range(clip)),
        json!({"start": 2000, "duration": 2000})
    );
    assert_eq!(
        clip["sourceRange"],
        json!({"start": 3000, "duration": 2000})
    );
}

#[test]
fn premiere_to_tesseract_preserves_cut_gap_for_reused_media() {
    let dir = tempfile::tempdir().unwrap();
    let file = converted_adobe_sequence(dir.path(), TWO_TRACKS_GAP);
    let document = file.project_json().unwrap();
    let mut occurrences: Vec<_> = video_layers(&document)
        .into_iter()
        .filter(|layer| layer_uses_media(&file, layer, "clip-a.mp4"))
        .map(|layer| {
            (
                (*crate::test_support::layer_range(layer))["start"]
                    .as_i64()
                    .unwrap(),
                (*crate::test_support::layer_range(layer))["duration"]
                    .as_i64()
                    .unwrap(),
                layer["sourceRange"]["start"].as_i64().unwrap(),
                layer["sourceRange"]["duration"].as_i64().unwrap(),
                layer["source"]["assetId"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    occurrences.sort_by_key(|occurrence| occurrence.0);

    assert_eq!(occurrences.len(), 2);
    assert_eq!(
        occurrences[0].0..occurrences[0].0 + occurrences[0].1,
        0..2000
    );
    assert_eq!(
        occurrences[1].0..occurrences[1].0 + occurrences[1].1,
        4000..5000
    );
    assert_eq!((occurrences[0].2, occurrences[0].3), (0, 2000));
    assert_eq!((occurrences[1].2, occurrences[1].3), (4000, 1000));
    assert_eq!(occurrences[0].4, occurrences[1].4);
}

#[test]
fn isolated_red_blue_sources_preserve_the_editable_gap_without_overlap() {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_two_tracks_gap_strict.prproj");
    let native = PrProjectFile::load(&source).unwrap().0;
    assert_eq!(native.sequences().len(), 1);
    let track_clips: Vec<_> = native
        .sequences()
        .next()
        .unwrap()
        .video_tracks()
        .map(|track| track.len())
        .collect();
    assert_eq!(track_clips.iter().sum::<usize>(), 2);
    assert_eq!(track_clips.iter().filter(|&&count| count > 0).count(), 1);
    let output = dir.path().join("distinct-sources");
    premiere_to_tesseract(
        &source,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let file = TesseractFile::open(&projects[0]).unwrap();
    assert_eq!(file.metadata().assets.len(), 2);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 2);
    let first = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 0)
        .unwrap();
    let second = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 3000)
        .unwrap();
    assert!(layer_uses_media(
        &file,
        first,
        "feature_two_tracks_gap_clip_a.mp4"
    ));
    assert!(layer_uses_media(
        &file,
        second,
        "feature_two_tracks_gap_clip_b.mp4"
    ));
    assert_ne!(first["source"]["assetId"], second["source"]["assetId"]);
    assert_eq!(
        (*crate::test_support::layer_range(first)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(second)),
        json!({"start": 3000, "duration": 2000})
    );
    assert_eq!(first["sourceRange"], json!({"start": 0, "duration": 2000}));
    assert_eq!(second["sourceRange"], json!({"start": 0, "duration": 2000}));
    assert!(layers.iter().all(|layer| {
        let start = (*crate::test_support::layer_range(layer))["start"]
            .as_i64()
            .unwrap();
        let end = start
            + (*crate::test_support::layer_range(layer))["duration"]
                .as_i64()
                .unwrap();
        !(start <= 2500 && 2500 < end)
    }));
    let canvas = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(canvas["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(canvas)),
        json!({"start": 0, "duration": 5000})
    );
    assert_editable_feature_roundtrip(
        dir.path(),
        "feature_two_tracks_gap_strict.prproj",
        "Two tracks gap",
        &projects[0],
    );
}

#[test]
fn adobe_red_blue_video_tracks_preserve_overlap_and_layer_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = "feature_two_tracks_overlap_color_strict.prproj";
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let native = PrProjectFile::load(&source).unwrap().0;
    assert_eq!(native.sequences().len(), 1);
    let sequence = native.sequences().next().unwrap();
    assert_eq!(sequence.name(), "Two tracks overlap color");
    assert_eq!(
        sequence_table(&native, sequence)["occurrences"],
        json!([
            {"track_index": 0, "media_name": "feature_two_tracks_gap_clip_a.mp4",
             "timeline_ticks": [0, 2 * TICKS], "source_ticks": [0, 2 * TICKS]},
            {"track_index": 1, "media_name": "feature_two_tracks_gap_clip_b.mp4",
             "timeline_ticks": [TICKS, 3 * TICKS], "source_ticks": [0, 2 * TICKS]},
        ])
    );

    let (file, converted) = isolated_feature(dir.path(), fixture);
    assert_eq!(file.metadata().assets.len(), 2);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 2);
    // Higher-index Premiere track renders above the lower one throughout 1–2 s.
    assert!(layer_uses_media(
        &file,
        layers[0],
        "feature_two_tracks_gap_clip_b.mp4"
    ));
    assert!(layer_uses_media(
        &file,
        layers[1],
        "feature_two_tracks_gap_clip_a.mp4"
    ));
    assert_eq!(
        (*crate::test_support::layer_range(layers[0])),
        json!({"start": 1000, "duration": 2000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(layers[1])),
        json!({"start": 0, "duration": 2000})
    );
    assert!(layers
        .iter()
        .all(|layer| { layer["sourceRange"] == json!({"start": 0, "duration": 2000}) }));
    assert_ne!(
        layers[0]["source"]["assetId"],
        layers[1]["source"]["assetId"]
    );
    assert_editable_feature_roundtrip(dir.path(), fixture, "Two tracks overlap color", &converted);
}

#[test]
fn lightening_22_10_blend_pair_converts_as_screen_both_ways_and_reimports() {
    // AME renders this fixture's (22, 10) brighter than both layers, a lightening
    // blend that Screen fits (tests/README.md): both clips convert, the upper
    // one as Screen at its 50% Opacity, the measured formula, without a report.
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_opacity_normal_static_50_strict.prproj");
    let output = dir.path().join("feature-converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // The blend of each clip, upper (blue) first, as read from `output`.
    let blends = |output: &Path| -> Vec<(Value, Value)> {
        let file = TesseractFile::open(project_files(output).pop().unwrap()).unwrap();
        let document = file.project_json().unwrap();
        let layers = video_layers(&document);
        [
            "feature_two_tracks_gap_clip_b.mp4",
            "feature_two_tracks_gap_clip_a.mp4",
        ]
        .into_iter()
        .map(|media| {
            let layer = layers
                .iter()
                .find(|layer| layer_uses_media(&file, layer, media))
                .unwrap();
            (
                layer["blendMode"].clone(),
                layer["transform"]["opacity"].clone(),
            )
        })
        .collect()
    };
    let expected = [
        (json!("screen"), json!(50.0)),
        (json!("normal"), json!(100.0)),
    ];
    assert_eq!(blends(&output), expected);

    // Export writes (22, 10), which reads back as Screen.
    let converted = project_files(&output).pop().unwrap();
    let native = dir.path().join("native");
    let omissions = tesseract_to_premiere(&converted, &native, false).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| !omission.reason.to_lowercase().contains("blend mode")),
        "{omissions:?}"
    );
    // The two Blend Mode parameters hold the canonical pair (22, 10).
    let xml = read_xml(&native.join("project.prproj"));
    for value in [22, 10] {
        let start =
            format!("<StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>");
        assert!(xml.contains(&start), "{value}");
    }
    let reimported = dir.path().join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(blends(&reimported), expected);
}

#[test]
fn adobe_blend_code_chart_imports_each_measured_mode_and_writes_it_back() {
    // Tile q plays in five 1 s slots from 1 s, one pair each; the Oracle's AME
    // render measures every slot's mode (tests/README.md). Parameter 2 selects
    // the mode, so (22, 0) is Screen and (1, 0) Color Burn; Premiere saved the
    // (18, 10) slot with its default Opacity.
    let expected = [
        ["normal", "lighten", "softLight", "difference", "color"],
        ["darken", "screen", "hardLight", "exclusion", "luminosity"],
        ["multiply", "colorDodge", "vividLight", "subtract", "screen"],
        ["colorBurn", "add", "linearLight", "divide", "normal"],
        ["linearBurn", "lighterColor", "pinLight", "hue", "colorBurn"],
        ["darkerColor", "overlay", "hardMix", "saturation", "screen"],
    ];
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_blend_codes_26_5_strict.prproj");
    // Each tile slot's (tile, start ms, blend mode, opacity), from `output`.
    let slots = |output: &Path| -> Vec<(String, Value, Value, Value)> {
        let file = TesseractFile::open(project_files(output).pop().unwrap()).unwrap();
        let document = file.project_json().unwrap();
        let mut slots: Vec<_> = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Image")
            .map(|layer| {
                let asset = layer["source"]["assetId"].as_str().unwrap();
                let name = Path::new(&file.metadata().assets[asset].path)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                (
                    name,
                    (*crate::test_support::layer_range(layer))["start"].clone(),
                    layer["blendMode"].clone(),
                    layer["transform"]["opacity"].clone(),
                )
            })
            .filter(|(name, ..)| name != "a1_bg_chart.png")
            .collect();
        slots.sort_by_key(|(name, start, ..)| (name.clone(), start.as_i64()));
        slots
    };
    let chart: Vec<_> = expected
        .iter()
        .enumerate()
        .flat_map(|(tile, modes)| {
            modes.iter().enumerate().map(move |(slot, mode)| {
                // The last slot of tile 5 is (22, 10) at Opacity 50.
                let opacity = if (tile, slot) == (5, 4) { 50.0 } else { 100.0 };
                (
                    format!("a1_fg_tile{tile}.png"),
                    json!(1000 * (slot + 1)),
                    json!(mode),
                    json!(opacity),
                )
            })
        })
        .collect();
    // Only Darker and Lighter Color report their formula difference.
    let blend_reports = |omissions: &[Omission]| -> Vec<String> {
        omissions
            .iter()
            .filter(|omission| omission.reason.contains("Blend Mode"))
            .map(|omission| omission.reason.clone())
            .collect()
    };
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("c08fb09e-d4dc-47fc-ba80-7b39d691ebd8"),
        false,
    )
    .unwrap();
    assert_eq!(slots(&output), chart);
    let reports = blend_reports(&omissions);
    assert!(
        reports.len() == 2
            && reports[0].starts_with("Blend Mode (4, 7) Darker Color")
            && reports[1].starts_with("Blend Mode (12, 13) Lighter Color"),
        "{omissions:?}"
    );
    // Export writes each mode's pair, and a reimport reads every mode back.
    let native = dir.path().join("native");
    let omissions =
        tesseract_to_premiere(project_files(&output).pop().unwrap(), &native, false).unwrap();
    assert_eq!(blend_reports(&omissions).len(), 2, "{omissions:?}");
    let reimported = dir.path().join("reimported");
    premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    assert_eq!(slots(&reimported), chart);
}

#[test]
fn adobe_normal_pair_18_0_preserves_static_opacity_without_omission() {
    // AME renders this fixture's (18, 0) at 50% as a mix of the two layers.
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_opacity_screen_strict.prproj");
    let output = dir.path().join("feature-converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    let project = project_files(&output).pop().unwrap();
    let file = TesseractFile::open(project).unwrap();
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 2);

    let upper = layers
        .iter()
        .find(|layer| layer_uses_media(&file, layer, "feature_two_tracks_gap_clip_b.mp4"))
        .unwrap();
    assert_eq!(upper["transform"]["opacity"], json!(50.0));
    assert_eq!(upper["blendMode"], json!("normal"));
    assert_eq!(
        (*crate::test_support::layer_range(upper)),
        json!({"start": 0, "duration": 2000})
    );

    let lower = layers
        .iter()
        .find(|layer| layer_uses_media(&file, layer, "feature_two_tracks_gap_clip_a.mp4"))
        .unwrap();
    assert_eq!(lower["transform"]["opacity"], json!(100.0));
    assert_eq!(lower["blendMode"], json!("normal"));
    assert_eq!(
        (*crate::test_support::layer_range(lower)),
        json!({"start": 0, "duration": 2000})
    );
}

fn isolated_feature(root: &Path, fixture: &str) -> (TesseractFile, std::path::PathBuf) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    assert_eq!(PrProjectFile::load(&source).unwrap().0.sequences().len(), 1);
    let output = root.join("feature-converted");
    premiere_to_tesseract(
        &source,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    (
        TesseractFile::open(&projects[0]).unwrap(),
        projects[0].clone(),
    )
}

fn assert_editable_feature_roundtrip(
    root: &Path,
    fixture: &str,
    sequence_name: &str,
    converted: &Path,
) {
    let original = PrProjectFile::load(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture),
    )
    .unwrap()
    .0;
    let selected = original
        .sequences()
        .find(|sequence| sequence.name() == sequence_name)
        .unwrap();
    let before = sequence_table(&original, selected);
    let output = root.join("feature-reencoded");
    tesseract_to_premiere(converted, &output, false).unwrap();
    let rebuilt = PrProjectFile::load(output.join("project.prproj"))
        .unwrap()
        .0;
    let after = sequence_table(&rebuilt, rebuilt.sequences().next().unwrap());
    let editable_occurrences = |table: &Value| {
        let mut occurrences = table["occurrences"].as_array().unwrap().clone();
        occurrences.sort_by_key(|item| item["timeline_ticks"][0].as_i64().unwrap());
        let mut sources = Vec::<String>::new();
        occurrences
            .iter()
            .map(|item| {
                let name = item["media_name"].as_str().unwrap();
                let source = sources
                    .iter()
                    .position(|known| known == name)
                    .unwrap_or_else(|| {
                        sources.push(name.to_owned());
                        sources.len() - 1
                    });
                (
                    item["timeline_ticks"].clone(),
                    item["source_ticks"].clone(),
                    source,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(editable_occurrences(&after), editable_occurrences(&before));
    if fixture == "feature_two_tracks_overlap_color_strict.prproj" {
        let track_indices = |table: &Value| {
            table["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .map(|clip| clip["track_index"].as_u64().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(track_indices(&after), track_indices(&before));
    }
    if fixture == "feature_quicktime_container_strict.prproj" {
        assert!(after["occurrences"][0]["media_name"]
            .as_str()
            .unwrap()
            .ends_with(".mov"));
    }
}

#[test]
fn adobe_quicktime_media_keeps_original_mov_asset_editable() {
    let dir = tempfile::tempdir().unwrap();
    let (file, archive) = isolated_feature(dir.path(), "feature_quicktime_container_strict.prproj");
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(file.metadata().assets.len(), 1);
    assert_eq!(layers.len(), 1);
    let layer = layers[0];
    assert!(layer_uses_media(
        &file,
        layer,
        "feature_timecoded_source.mov"
    ));
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(layer["sourceRange"], json!({"start": 0, "duration": 2000}));
    assert_editable_feature_roundtrip(
        dir.path(),
        "feature_quicktime_container_strict.prproj",
        "QuickTime media",
        &archive,
    );
}

/// `feature_custom_canvas_vertical_strict.prproj` (manifest case
/// `premiere_isolated_vertical_canvas`) is XML-derived, not a Premiere save
/// (its portrait sequence keeps its scaffold's 1920x1080 preview size and
/// `clip-a.mp4` names): one default-Motion clip of its 1080x1920 source on a
/// 1080x1920 sequence, which AME rendered as the case's pinned reference. This
/// test checks the editable structure of a fresh conversion and export; the
/// render comparison and a Premiere reopen of the export are not run here.
#[test]
fn derived_vertical_canvas_converts_at_its_own_size_in_both_directions() {
    const FIXTURE: &str = "feature_custom_canvas_vertical_strict.prproj";
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE);
    let output = dir.path().join("feature-converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    let document = file.project_json().unwrap();
    assert_eq!(
        document["dimensions"],
        json!({"width": 1080, "height": 1920})
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2);
    let (video, canvas) = (&layers[0], &layers[1]);
    assert!(layer_uses_media(
        &file,
        video,
        "feature_custom_canvas_vertical.mov"
    ));
    assert_eq!(
        video["source"]["sourceRect"],
        json!({"x": 0.0, "y": 0.0, "width": 1080.0, "height": 1920.0})
    );
    // Default Motion: the source's centre on the canvas centre, unscaled.
    assert_eq!(video["transform"]["anchorPoint"], json!([540.0, 960.0]));
    assert_eq!(video["transform"]["position"], json!([540.0, 960.0]));
    assert_eq!(video["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(
        (*crate::test_support::layer_range(video)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(video["sourceRange"], json!({"start": 0, "duration": 2000}));
    assert_eq!(canvas["rect"]["size"], json!([1080.0, 1920.0]));

    assert_editable_feature_roundtrip(dir.path(), FIXTURE, "Vertical canvas", &archive);
    let native = dir.path().join("feature-reencoded/project.prproj");
    let (rebuilt, omissions) = PrProjectFile::load(&native).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        rebuilt.sequences().next().unwrap().dimensions(),
        [1080, 1920]
    );
    let xml = read_xml(&native);
    assert_eq!(
        xml.matches("<FrameRect>0,0,1080,1920</FrameRect>").count(),
        3
    );
    assert!(!xml.contains("<FrameRect>0,0,1920,1080</FrameRect>"));
    let again = dir.path().join("again");
    premiere_to_tesseract(&native, &again, None, false).unwrap();
    let reimported = TesseractFile::open(first_project(&again))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(reimported["dimensions"], document["dimensions"]);
    assert_eq!(
        reimported["composition"]["layers"][0]["transform"],
        video["transform"]
    );
}

/// Premiere 26.5.1 saved `feature_custom_canvas_vertical_26_5_strict.prproj`
/// (manifest case `premiere_isolated_vertical_canvas_26_5`, which the strict
/// gate scores against its AME render): sequence "Vertical canvas 26.5", a
/// Custom-mode 1080x1920 30 fps sequence with one default-Motion clip of the
/// 1080x1920 source on V1 for 2 s. The save keeps its absolute media paths;
/// its package-local `RelativePath` names the committed media. Its only report
/// is the 26.5 save-form track item `Node`, at feature scope: the clip converts.
#[test]
fn premiere_26_5_vertical_canvas_imports_at_its_own_size() {
    const FIXTURE: &str = "feature_custom_canvas_vertical_26_5_strict.prproj";
    const SOURCE_MEDIA: &[u8] = include_bytes!("../fixtures/feature_custom_canvas_vertical.mov");
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE);
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("716eb057-b99d-4972-a9eb-6054b62eaa7a"),
        false,
    )
    .unwrap();
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "VideoClipTrackItem:63".into(),
            reason: "ClipTrackItem/TrackItem/Node not converted".into(),
        }]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert_eq!(file.project().composition().name(), "Vertical canvas 26.5");
    let document = file.project_json().unwrap();
    assert_eq!(
        document["dimensions"],
        json!({"width": 1080, "height": 1920})
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Video", "Rect"]
    );
    let (video, canvas) = (&layers[0], &layers[1]);
    assert_eq!(
        video["source"]["sourceRect"],
        json!({"x": 0.0, "y": 0.0, "width": 1080.0, "height": 1920.0})
    );
    // Default Motion: the source's centre on the canvas centre, unscaled.
    assert_eq!(video["transform"]["anchorPoint"], json!([540.0, 960.0]));
    assert_eq!(video["transform"]["position"], json!([540.0, 960.0]));
    assert_eq!(video["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(
        (*crate::test_support::layer_range(video)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(video["sourceRange"], json!({"start": 0, "duration": 2000}));
    assert_eq!(canvas["rect"]["size"], json!([1080.0, 1920.0]));
    assert_eq!(canvas["rect"]["fillColor"], json!([0.0, 0.0, 0.0, 1.0]));
    // The packaged media is the committed source, byte for byte.
    let mut packaged = Vec::new();
    file.asset(video["source"]["assetId"].as_str().unwrap())
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut packaged)
        .unwrap();
    assert_eq!(packaged, SOURCE_MEDIA);
}

#[test]
fn adobe_adjacent_cut_keeps_distinct_editable_sources() {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_adjacent_cut_strict.prproj");
    let native = PrProjectFile::load(source).unwrap().0;
    assert_eq!(native.sequences().len(), 1);
    let track_clips: Vec<_> = native
        .sequences()
        .next()
        .unwrap()
        .video_tracks()
        .map(|track| track.len())
        .collect();
    assert_eq!(track_clips.iter().sum::<usize>(), 2);
    assert_eq!(track_clips.iter().filter(|&&count| count > 0).count(), 1);
    let (file, archive) = isolated_feature(dir.path(), "feature_adjacent_cut_strict.prproj");
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(file.metadata().assets.len(), 2);
    assert_eq!(layers.len(), 2);
    let red = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 0)
        .unwrap();
    let blue = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 2000)
        .unwrap();
    assert!(layer_uses_media(
        &file,
        red,
        "feature_two_tracks_gap_clip_a.mp4"
    ));
    assert!(layer_uses_media(
        &file,
        blue,
        "feature_two_tracks_gap_clip_b.mp4"
    ));
    assert_ne!(red["source"]["assetId"], blue["source"]["assetId"]);
    for (layer, start) in [(red, 0), (blue, 2000)] {
        assert_eq!(
            (*crate::test_support::layer_range(layer)),
            json!({"start": start, "duration": 2000})
        );
        assert_eq!(layer["sourceRange"], json!({"start": 0, "duration": 2000}));
    }
    assert_editable_feature_roundtrip(
        dir.path(),
        "feature_adjacent_cut_strict.prproj",
        "Adjacent cut",
        &archive,
    );
}

#[test]
fn adobe_linear_wipe_preserves_editable_overlap_mask_and_source_handles() {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_linear_wipe_strict.prproj");
    let output = dir.path().join("linear-wipe-converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("49b892d1-dfc3-4be2-a83a-93789626be7c"),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let file = TesseractFile::open(&projects[0]).unwrap();
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 2);
    let red = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 0)
        .unwrap();
    let blue = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 2000)
        .unwrap();
    assert_eq!(
        (*crate::test_support::layer_range(red)),
        json!({"start": 0, "duration": 3000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(blue)),
        json!({"start": 2000, "duration": 3000})
    );
    assert_eq!(red["sourceRange"], json!({"start": 0, "duration": 3000}));
    assert_eq!(blue["sourceRange"], json!({"start": 0, "duration": 3000}));
    let masks = blue["masks"].as_array().unwrap();
    assert_eq!(masks.len(), 1);
    assert_eq!(masks[0]["mode"], "add");
    assert_eq!(masks[0]["feather"], json!([5.0, 5.0]));
    let guide_id = masks[0]["layer"].as_u64().unwrap();
    let guide = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == guide_id)
        .unwrap();
    assert_eq!(guide["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(guide)),
        (*crate::test_support::layer_range(blue))
    );
    assert_eq!(guide["transform"]["anchorPoint"], json!([0.0, 0.0]));
    let dynamics = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let wipe = dynamics
        .iter()
        .find(|entry| entry["target"]["layerId"] == guide_id)
        .unwrap();
    assert_eq!(wipe["target"]["propertyType"], "scaleX");
    assert_eq!(
        wipe["animator"]["keyframes"],
        json!([
            {"id": "premiere-linear-wipe-3-0", "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "linear"}},
            {"id": "premiere-linear-wipe-3-1", "layerTime": 1000, "value": {"type": "float", "value": 100.0}, "easing": {"type": "linear"}}
        ])
    );
    assert_editable_feature_roundtrip(
        dir.path(),
        "feature_linear_wipe_strict.prproj",
        "Linear wipe red to blue",
        &projects[0],
    );
    let rebuilt = dir.path().join("feature-reencoded/project.prproj");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(rebuilt).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("<DisplayName>Linear Wipe</DisplayName>"));
    assert!(xml.contains("<MatchName>AE.ADBE Linear Wipe</MatchName>"));
    assert!(xml.contains("<Name>Transition Completion</Name>"));
    assert!(xml.contains("<Name>Wipe Angle</Name>"));
    assert!(xml.contains("<StartKeyframe>-91445760000000000,270,0,0,0,0,0,0</StartKeyframe>"));
    assert!(xml.contains("<Keyframes>0,100,0,0,0,0,0,0;254016000000,0,0,0,0,0,0,0;</Keyframes>"));
    assert!(xml.contains("<Name>Feather</Name>"));
}

#[test]
fn adobe_nonzero_source_trim_keeps_timecoded_source_offset() {
    let dir = tempfile::tempdir().unwrap();
    let (file, archive) = isolated_feature(dir.path(), "feature_nonzero_source_trim_strict.prproj");
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(file.metadata().assets.len(), 1);
    assert_eq!(layers.len(), 1);
    let layer = layers[0];
    assert!(layer_uses_media(
        &file,
        layer,
        "feature_timecoded_source.mp4"
    ));
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        layer["sourceRange"],
        json!({"start": 3000, "duration": 2000})
    );
    assert_editable_feature_roundtrip(
        dir.path(),
        "feature_nonzero_source_trim_strict.prproj",
        "Nonzero source trim",
        &archive,
    );
}

#[test]
fn adobe_exported_linear_rotation_keeps_editable_source_clock_keys() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = "feature_motion_rotation_linear_strict.prproj";
    let (file, archive) = isolated_feature(dir.path(), fixture);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(file.metadata().assets.len(), 1);
    assert_eq!(layers.len(), 1);
    let layer = layers[0];
    // Premiere's default Motion Position and Anchor Point are each 0.5:0.5.
    // A zero-degree key renders identically with an origin pivot, but later keys do not.
    assert_eq!(layer["transform"]["anchorPoint"], json!([960.0, 540.0]));
    assert_eq!(layer["transform"]["position"], json!([960.0, 540.0]));
    assert_eq!(layer["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(layer["transform"]["rotation"], json!(0.0));
    assert!(layer_uses_media(
        &file,
        layer,
        "feature_timecoded_source.mp4"
    ));
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(layer["sourceRange"], json!({"start": 0, "duration": 2000}));

    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let rotation = &entries[0];
    assert_eq!(rotation["target"]["propertyType"], "rotation");
    assert_eq!(rotation["target"]["layerId"], layer["id"]);
    assert_eq!(rotation["animator"]["type"], "keyframes");
    let keys = rotation["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_u64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
                key["easing"]["type"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(keys, [(500, 0.0, "linear"), (1500, 60.0, "linear")]);

    // A structural round trip does not assert visual agreement with Adobe's export.
    assert_editable_feature_roundtrip(
        dir.path(),
        fixture,
        "feature_motion_rotation_linear_strict",
        &archive,
    );
}

/// Stages `feature_keyframe_ids_strict.prproj` ("Keyframe ids"): blue V1 at
/// 0-10 s, Rotation-keyed red clips A (0-3 s) and B (3-6 s) on V2, and on V3
/// two placements (6-8 s, 8-10 s) of "Keyed inner", whose red clip has its own
/// Rotation keys. It is a structural derivative of the pinned
/// `feature_nested_second_sequence_strict` and
/// `feature_motion_rotation_linear_strict` projects, not saved by Premiere.
fn keyframe_ids_fixture(root: &Path) -> std::path::PathBuf {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(root.join("media")).unwrap();
    for (name, source) in [
        ("clip-a.mp4", "feature_multi_sequence_red_10s.mp4"),
        ("clip-b.mp4", "feature_multi_sequence_blue_10s.mp4"),
    ] {
        fs::copy(fixtures.join(source), root.join("media").join(name)).unwrap();
    }
    let project = root.join("project.prproj");
    fs::copy(
        fixtures.join("feature_keyframe_ids_strict.prproj"),
        &project,
    )
    .unwrap();
    project
}

type RotationRow = (u64, Option<String>, Value, Vec<(i64, f64, String)>);

/// A layer's id, document start, holding group and source range.
type LayerPlace = (Value, u64, Option<String>, Value);

/// Each Rotation track by the document start of its layer: the group holding
/// the layer, the layer's source range and the (layer time, value, easing) of
/// its keys. Every keyframe id must name its own layer and be unique.
fn rotation_rows(document: &Value) -> Vec<RotationRow> {
    fn place(layers: &Value, offset: u64, group: Option<&str>, places: &mut Vec<LayerPlace>) {
        for layer in layers.as_array().unwrap() {
            let start = offset
                + (*crate::test_support::layer_range(layer))["start"]
                    .as_u64()
                    .unwrap();
            let source = layer["sourceRange"].clone();
            places.push((layer["id"].clone(), start, group.map(str::to_owned), source));
            if layer["type"] == "Group" {
                place(&layer["layers"], start, layer["name"].as_str(), places);
            }
        }
    }
    let mut places = Vec::new();
    place(&document["composition"]["layers"], 0, None, &mut places);
    let mut ids = std::collections::BTreeSet::new();
    let mut rows: Vec<RotationRow> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(entry["target"]["propertyType"], "rotation");
            let target = &entry["target"]["layerId"];
            let (_, start, group, source) =
                places.iter().find(|(id, ..)| id == target).unwrap().clone();
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            for (index, key) in keys.iter().enumerate() {
                assert_eq!(key["id"], format!("premiere-rotation-{target}-{index}"));
                assert!(ids.insert(key["id"].as_str().unwrap().to_owned()));
            }
            let keys = keys.iter().map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                    key["easing"]["type"].as_str().unwrap().to_owned(),
                )
            });
            (start, group, source, keys.collect())
        })
        .collect();
    assert_eq!(ids.len(), 12, "{ids:?}");
    rows.sort_by_key(|row| row.0);
    rows
}

#[test]
fn adobe_derived_rotation_keys_stay_unique_across_layers_and_nest_copies() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        keyframe_ids_fixture(root),
        &output,
        Some("1592feef-89df-4c40-ba9d-fe9088c8f4a5"),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let archive = first_project(&output);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();

    // The native Linear keys on each clip's source clock: every In is 0.
    let row = |start, group: Option<&str>, duration, keys: [(i64, f64); 3]| -> RotationRow {
        let source = json!({"start": 0, "duration": duration});
        let keys = keys.map(|(time, value)| (time, value, "linear".to_owned()));
        (start, group.map(str::to_owned), source, keys.to_vec())
    };
    let inner = [(500, 0.0), (1000, 60.0), (1500, 20.0)];
    let expected = [
        row(0, None, 3000, [(500, 0.0), (1500, 30.0), (2500, -15.0)]),
        row(3000, None, 3000, [(500, 0.0), (1500, -30.0), (2500, 45.0)]),
        row(6000, Some("Keyed inner"), 2000, inner),
        row(8000, Some("Keyed inner"), 2000, inner),
    ];
    assert_eq!(rotation_rows(&document), expected);
    // Both placements are plain groups over one keyed copy each.
    let layers = document["composition"]["layers"].as_array().unwrap();
    let canvas = layers.last().unwrap();
    let groups: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .collect();
    assert_eq!(groups.len(), 2);
    for group in groups {
        assert_eq!(group["transform"], canvas["transform"], "identity");
        assert_eq!(group["blendMode"], "normal");
        assert_eq!(group["layers"].as_array().unwrap().len(), 1);
    }

    // The export writes native keys that read back as the same editable keys.
    let omissions = tesseract_to_premiere(&archive, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let native = root.join("native/project.prproj");
    let (project, omissions) = PrProjectFile::load(&native).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let names: Vec<_> = project
        .sequences()
        .map(|sequence| sequence.name())
        .collect();
    assert_eq!(names, ["Keyframe ids"]);
    let root_sequence = exported_root_sequence(&native);
    let omissions =
        premiere_to_tesseract(&native, root.join("again"), Some(&root_sequence), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let again = TesseractFile::open(first_project(&root.join("again")))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(rotation_rows(&again), expected);
}

/// Every video layer, root or group child, in document order: its absolute
/// range, holding group, media, static Motion and Opacity, blend mode and the
/// (layer time, value, easing) keys of each animated property. Every keyframe
/// id must be unique.
fn motion_opacity_layers(file: &TesseractFile) -> Vec<Value> {
    fn visit(layers: &Value, offset: u64, group: Option<&str>, found: &mut Vec<(Value, u64)>) {
        for layer in layers.as_array().unwrap() {
            let start = offset
                + (*crate::test_support::layer_range(layer))["start"]
                    .as_u64()
                    .unwrap();
            match layer["type"].as_str().unwrap() {
                "Group" => visit(&layer["layers"], start, layer["name"].as_str(), found),
                "Video" => {
                    let mut layer = layer.clone();
                    layer["group"] = json!(group);
                    found.push((layer, start));
                }
                _ => {}
            }
        }
    }
    let document = file.project_json().unwrap();
    let mut ids = std::collections::BTreeSet::new();
    let mut keys = serde_json::Map::new();
    for entry in document["composition"]["dynamics"]["entries"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let track: Vec<_> = entry["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| {
                assert!(
                    ids.insert(key["id"].to_string()),
                    "shared keyframe id {}",
                    key["id"]
                );
                json!([
                    key["layerTime"],
                    key["value"]["value"],
                    key["easing"]["type"]
                ])
            })
            .collect();
        let target = &entry["target"];
        keys.entry(target["layerId"].to_string())
            .or_insert_with(|| json!({}))[target["propertyType"].as_str().unwrap()] = json!(track);
    }
    let mut found = Vec::new();
    visit(&document["composition"]["layers"], 0, None, &mut found);
    found
        .into_iter()
        .map(|(layer, start)| {
            let transform = &layer["transform"];
            let asset = &file.metadata().assets[layer["source"]["assetId"].as_str().unwrap()];
            json!({
                "start": start,
                "duration": (*crate::test_support::layer_range(&layer))["duration"],
                "group": layer["group"],
                "media": Path::new(&asset.path).file_name().unwrap().to_str().unwrap(),
                "position": transform["position"],
                "anchorPoint": transform["anchorPoint"],
                "scale": transform["scale"],
                "rotation": transform["rotation"],
                "opacity": transform["opacity"],
                "blendMode": layer["blendMode"],
                "keys": keys.get(&layer["id"].to_string()).cloned().unwrap_or_else(|| json!({})),
            })
        })
        .collect()
}

#[test]
fn premiere_26_5_motion_and_opacity_stay_editable_in_both_directions() {
    // Premiere 26.5.1 saved `feature_motion_opacity_26_5_strict.prproj`
    // ("Motion opacity 26.5"): blue V1 at 0-10 s; on V2, red clips with static
    // Position (S1, 0-1 s), Scale (S2), Rotation (S3), Anchor Point (S4),
    // Opacity (S5) and nonuniform Scale (S6, 5-6 s), keyed Position, Scale,
    // Rotation and Opacity (K, 6-8 s), and two placements (8-9 s, 9-10 s) of
    // "Keyed inner", whose red clip has keyed Opacity and Rotation.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_motion_opacity_26_5_strict.prproj");
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("aad89517-456c-4cbf-8d97-c54bf5af8eb2"),
        false,
    )
    .unwrap();
    // Every omission is outside Motion and Opacity: the nest's audio parts and
    // the 26.5 track item `Node`.
    assert!(
        omissions.iter().all(
            |omission| omission.reason.contains("found AudioSequenceSource")
                || omission.reason == "ClipTrackItem/TrackItem/Node not converted"
        ),
        "{omissions:?}"
    );
    let archive = first_project(&output);
    let layers = motion_opacity_layers(&TesseractFile::open(&archive).unwrap());

    let red = "feature_multi_sequence_red_10s.mp4";
    let layer = |start: u64, duration: u64, group: Option<&str>, media: &str| {
        json!({
            "start": start,
            "duration": duration,
            "group": group,
            "media": media,
            "position": [960.0, 540.0],
            "anchorPoint": [960.0, 540.0],
            "scale": [100.0, 100.0],
            "rotation": 0.0,
            "opacity": 100.0,
            "blendMode": "normal",
            "keys": {},
        })
    };
    let with = |mut layer: Value, field: &str, value: Value| {
        layer[field] = value;
        layer
    };
    // Native keys on each clip's source clock; each outgoing mode becomes the
    // easing of the next key.
    let keyed = json!({
        "positionX": [[500, 960.0, "linear"], [1000, 1152.0, "linear"], [1500, 1344.0, "hold"]],
        "positionY": [[500, 540.0, "linear"], [1000, 486.0, "linear"], [1500, 432.0, "hold"]],
        "scaleX": [[500, 100.0, "linear"], [1000, 60.0, "linear"], [1500, 80.0, "hold"]],
        "scaleY": [[500, 100.0, "linear"], [1000, 60.0, "linear"], [1500, 80.0, "hold"]],
        "rotation": [[500, 0.0, "linear"], [1000, 20.0, "hold"], [1500, -10.0, "linear"]],
        "opacity": [[500, 100.0, "linear"], [1000, 50.0, "linear"], [1500, 75.0, "hold"]],
    });
    let inner = json!({
        "rotation": [[300, 0.0, "linear"], [700, 25.0, "hold"]],
        "opacity": [[300, 100.0, "linear"], [700, 40.0, "linear"]],
    });
    let expected = [
        with(
            layer(0, 1000, None, red),
            "position",
            json!([1200.0, 702.0]),
        ),
        // S2: uniform Scale 60 with Scale Width left at 100, as Premiere saves it.
        with(layer(1000, 1000, None, red), "scale", json!([60.0, 60.0])),
        with(layer(2000, 1000, None, red), "rotation", json!(20.0)),
        with(
            layer(3000, 1000, None, red),
            "anchorPoint",
            json!([1152.0, 702.0]),
        ),
        with(layer(4000, 1000, None, red), "opacity", json!(50.0)),
        with(layer(5000, 1000, None, red), "scale", json!([80.0, 50.0])),
        with(layer(6000, 2000, None, red), "keys", keyed),
        with(
            layer(8000, 1000, Some("Keyed inner"), red),
            "keys",
            inner.clone(),
        ),
        with(layer(9000, 1000, Some("Keyed inner"), red), "keys", inner),
        layer(0, 10000, None, "feature_multi_sequence_blue_10s.mp4"),
    ];
    assert_eq!(layers, expected);

    // Export writes the 26.3 layout, which reads back as the same editable values.
    let omissions = tesseract_to_premiere(&archive, root.join("native"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let native = root.join("native/project.prproj");
    let (_, omissions) = PrProjectFile::load(&native).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let root_sequence = exported_root_sequence(&native);
    let omissions =
        premiere_to_tesseract(&native, root.join("again"), Some(&root_sequence), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let again = TesseractFile::open(first_project(&root.join("again"))).unwrap();
    assert_eq!(motion_opacity_layers(&again), expected);
}

#[test]
fn adobe_derived_static_motion_keeps_independent_position_anchor_and_axis_scales() {
    // Adobe exported this source separately, but this structural assertion does
    // not claim that the Tesseract render has been scored against that export.
    let dir = tempfile::tempdir().unwrap();
    let fixture = "feature_motion_static_transform_strict.prproj";
    let (file, archive) = isolated_feature(dir.path(), fixture);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 1);
    let layer = layers[0];
    assert_eq!(layer["transform"]["anchorPoint"], json!([480.0, 810.0]));
    assert_eq!(layer["transform"]["position"], json!([1228.8, 464.4]));
    assert_eq!(layer["transform"]["scale"], json!([70.0, 135.0]));
    assert_eq!(layer["transform"]["rotation"], json!(27.0));
    assert!(layer_uses_media(
        &file,
        layer,
        "feature_timecoded_source.mp4"
    ));
    assert!(document["composition"]["dynamics"]["entries"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert_editable_feature_roundtrip(
        dir.path(),
        fixture,
        "feature_motion_static_transform_strict",
        &archive,
    );
}

/// Native probe `premiere_motion_anchor_scale_width_probe_20260930`, derived
/// from `feature_motion_static_transform_strict.prproj` by its Motion values
/// alone (Position 0.5:0.5, Scale 50, Uniform Scale off, Rotation 0): Linear
/// Anchor Point keys 0.25:0.25 at 0 s and 0.5:0.5 at 0.8 s, and Linear Scale
/// Width keys 50 at 1 s and 100 at 1.8 s. AME renders every frame as Position
/// plus each axis's Scale times the source pixel less the source-normalized
/// anchor, within 0.8 px (an independent measurement; no FX render is
/// compared here). Source and canvas are both 1920 x 1080; see the fixture's
/// provenance file.
const ANCHOR_SCALE_WIDTH_PROBE: &str = "feature_motion_anchor_scale_width_probe.prproj";

/// `[id, layer time, value, easing]` of every key of each property that
/// `document` animates on `layer`, by property.
fn layer_keys(document: &Value, layer: &Value) -> Value {
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let tracks: serde_json::Map<String, Value> = entries
        .iter()
        .filter(|entry| entry["target"]["layerId"] == layer["id"])
        .map(|entry| {
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            let keys = keys
                .iter()
                .map(|key| {
                    json!([
                        key["id"],
                        key["layerTime"],
                        key["value"]["value"],
                        key["easing"]["type"]
                    ])
                })
                .collect();
            let property = entry["target"]["propertyType"].as_str().unwrap();
            (property.to_owned(), Value::Array(keys))
        })
        .collect();
    Value::Object(tracks)
}

#[test]
fn native_probe_anchor_point_and_scale_width_keys_import_as_editable_axis_tracks() {
    use sha2::{Digest, Sha256};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (name, digest) in [
        (
            ANCHOR_SCALE_WIDTH_PROBE,
            "429745911a526d1d99f008f09c68b05853a63d2a8ddf0c100dc6e48b9690f809",
        ),
        (
            "feature_timecoded_source.mp4",
            "4256ae026cb923ee0498374a1def4dd8c0e51078099a9f415726935e198ac0fe",
        ),
    ] {
        let bytes = fs::read(fixtures.join(name)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest, "{name}");
    }
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixtures.join(ANCHOR_SCALE_WIDTH_PROBE),
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 1);
    let layer = layers[0];
    assert!(layer_uses_media(
        &file,
        layer,
        "feature_timecoded_source.mp4"
    ));
    // The clip shows over 0-2 s, and its playback maps that window linearly
    // onto 0-2 s of source, inside its source selection.
    let range = json!({"start": 0, "duration": 2000});
    assert_eq!(
        layer["playback"],
        json!({
            "type": "windowed", "inputRange": range,
            "mapping": {"type": "linear", "input": range, "output": range},
            "inputOffsetMs": 0,
        })
    );
    assert_eq!(layer["sourceRange"], range);
    // Anchor Point in source pixels, Position in canvas pixels; both axes
    // start at Scale 50, and only the X axis is keyed.
    assert_eq!(layer["transform"]["anchorPoint"], json!([480.0, 270.0]));
    assert_eq!(layer["transform"]["position"], json!([960.0, 540.0]));
    assert_eq!(layer["transform"]["scale"], json!([50.0, 50.0]));
    assert_eq!(layer["transform"]["rotation"], json!(0.0));
    let id = |name: &str, index: u32| format!("premiere-{name}-{}-{index}", layer["id"]);
    assert_eq!(
        layer_keys(&document, layer),
        json!({
            "anchorPointX": [
                [id("anchor-point-x", 0), 0, 480.0, "linear"],
                [id("anchor-point-x", 1), 800, 960.0, "linear"],
            ],
            "anchorPointY": [
                [id("anchor-point-y", 0), 0, 270.0, "linear"],
                [id("anchor-point-y", 1), 800, 540.0, "linear"],
            ],
            "scaleX": [
                [id("scale-x", 0), 1000, 50.0, "linear"],
                [id("scale-x", 1), 1800, 100.0, "linear"],
            ],
        })
    );
}

/// One intrinsic Motion parameter as a generated project writes it.
struct WrittenParam {
    name: String,
    /// The value of its `StartKeyframe`.
    start: String,
    /// Each `Keyframes` key's ticks, value and outgoing mode.
    keys: Vec<(i64, String, String)>,
}

/// The parameters of the one intrinsic Motion of the generated `project`,
/// read from its XML, by `ParameterID`.
fn written_motion(project: &Path) -> std::collections::BTreeMap<String, WrittenParam> {
    use std::collections::HashMap;
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let records: HashMap<&str, roxmltree::Node<'_, '_>> = document
        .root_element()
        .children()
        .filter_map(|node| Some((node.attribute("ObjectID")?, node)))
        .collect();
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let motions: Vec<_> = records
        .values()
        .filter(|node| text(**node, "MatchName").as_deref() == Some("AE.ADBE Motion"))
        .collect();
    let [motion] = motions.as_slice() else {
        panic!("expected one Motion component, found {}", motions.len());
    };
    motion
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
        .map(|param| {
            let record = records[param.attribute("ObjectRef").unwrap()];
            let start = text(record, "StartKeyframe").unwrap();
            let keys = text(record, "Keyframes")
                .unwrap_or_default()
                .split_terminator(';')
                .map(|key| {
                    let fields: Vec<_> = key.split(',').collect();
                    (
                        fields[0].parse().unwrap(),
                        fields[1].to_owned(),
                        fields[2].to_owned(),
                    )
                })
                .collect();
            let param = WrittenParam {
                name: text(record, "Name").unwrap(),
                start: start.split(',').nth(1).unwrap().to_owned(),
                keys,
            };
            (text(record, "ParameterID").unwrap(), param)
        })
        .collect()
}

#[test]
fn edited_anchor_point_and_scale_width_keys_export_as_native_motion_keys() {
    let dir = tempfile::tempdir().unwrap();
    // An explicit editable document on the probe's media and frame, which no
    // import produced: the anchor moves from 0.25:0.25 of the source at 0 s
    // to 0.75:0.75 at 1.2 s, and the X axis alone grows from Scale 50 to 80
    // over 1-1.5 s while the Y axis stays at its static 50, equal to X's at
    // the start.
    let key = |id: &str, time: u64, value: f64| {
        json!({
            "id": id, "layerTime": time,
            "value": {"type": "float", "value": value}, "easing": {"type": "linear"},
        })
    };
    let track = |property: &str, keys: [(u64, f64); 2]| {
        json!({
            "target": {"kind": "layer", "layerId": 1, "propertyType": property},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                key(&format!("{property}-0"), keys[0].0, keys[0].1),
                key(&format!("{property}-1"), keys[1].0, keys[1].1),
            ]},
        })
    };
    let mut document = crate::test_support::editable_document();
    document["duration"] = json!(2.0);
    let layers = &mut document["composition"]["layers"];
    let range = json!({"start": 0, "duration": 2000});
    // The playback window 0-2 s maps linearly onto 0-2 s of source, inside
    // the independent source selection.
    layers[0]["playback"] = json!({
        "type": "windowed", "inputRange": range,
        "mapping": {"type": "linear", "input": range, "output": range},
        "inputOffsetMs": 0,
    });
    layers[0]["sourceRange"] = range.clone();
    layers[0]["sourceIntrinsicDuration"] = json!(10_000);
    layers[0]["transform"] = json!({
        "anchorPoint": [480, 270], "position": [960, 540], "scale": [50, 50],
        "rotation": 0, "opacity": 100,
    });
    layers[1]["activeRange"] = range;
    document["composition"]["dynamics"] = json!({"entries": [
        track("anchorPointX", [(0, 480.0), (1200, 1440.0)]),
        track("anchorPointY", [(0, 270.0), (1200, 810.0)]),
        track("scaleX", [(1000, 50.0), (1500, 80.0)]),
    ]});
    let edited = dir.path().join("edited.tsrct");
    write_archive(
        &edited,
        &document,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_timecoded_source.mp4"),
    );
    let native = dir.path().join("native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    // The generated records, read without this crate's reader: Uniform Scale
    // is off although both static axes are 50, the Scale Width keys move the
    // X axis alone, and the Anchor Point keys are fractions of the source.
    let motion = written_motion(&native.join("project.prproj"));
    let value = |text: &str| text.parse::<f64>().unwrap();
    let point = |text: &str| {
        let (x, y) = text.split_once(':').unwrap();
        [value(x), value(y)]
    };
    let position = &motion["1"];
    assert_eq!(
        (position.name.as_str(), point(&position.start)),
        ("Position", [0.5, 0.5])
    );
    assert!(position.keys.is_empty());
    let height = &motion["2"];
    assert_eq!(
        (height.name.as_str(), value(&height.start)),
        ("Scale Height", 50.0)
    );
    assert!(height.keys.is_empty());
    let width = &motion["3"];
    assert_eq!(
        (width.name.as_str(), value(&width.start)),
        ("Scale Width", 50.0)
    );
    assert_eq!(
        width
            .keys
            .iter()
            .map(|(ticks, key, mode)| (*ticks, value(key), mode.as_str()))
            .collect::<Vec<_>>(),
        [(TICKS, 50.0, "0"), (3 * TICKS / 2, 80.0, "0")]
    );
    assert_eq!(motion["4"].start, "false");
    let anchor = &motion["6"];
    assert_eq!(
        (anchor.name.as_str(), point(&anchor.start)),
        ("Anchor Point", [0.25, 0.25])
    );
    assert_eq!(
        anchor
            .keys
            .iter()
            .map(|(ticks, key, mode)| (*ticks, point(key), mode.as_str()))
            .collect::<Vec<_>>(),
        [(0, [0.25, 0.25], "0"), (6 * TICKS / 5, [0.75, 0.75], "0")]
    );

    // Supplementary: this crate reads the export back as the edited tracks.
    let root_sequence = exported_root_sequence(&native.join("project.prproj"));
    let again = dir.path().join("again");
    let omissions = premiere_to_tesseract(
        native.join("project.prproj"),
        &again,
        Some(&root_sequence),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&again))
        .unwrap()
        .project_json()
        .unwrap();
    let without_ids = |keys: Value| -> Value {
        keys.as_object()
            .unwrap()
            .iter()
            .map(|(property, keys)| {
                let keys = keys.as_array().unwrap().iter();
                (
                    property.clone(),
                    keys.map(|key| json!(key.as_array().unwrap()[1..]))
                        .collect(),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    assert_eq!(
        without_ids(layer_keys(&reimported, video_layers(&reimported)[0])),
        without_ids(layer_keys(&document, video_layers(&document)[0]))
    );
    let transform = &video_layers(&reimported)[0]["transform"];
    assert_eq!(transform["scale"], json!([50.0, 50.0]));
    assert_eq!(transform["anchorPoint"], json!([480.0, 270.0]));
}

#[test]
fn adobe_derived_media_fit_crop_stays_editable_in_both_directions() {
    // The native Crop parameter IDs and record classes come from the pinned
    // Adobe-authored `corporate_slideshow` source. This focused fixture is a
    // structural derivative; visual evidence must remain unapproved until a
    // human reviews its independent AME export.
    let dir = tempfile::tempdir().unwrap();
    let fixture = "feature_media_fit_crop_strict.prproj";
    let (file, archive) = isolated_feature(dir.path(), fixture);
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3);
    let video = &layers[0];
    let guide = &layers[1];
    assert_eq!(video["source"]["fit"], json!("contain"));
    assert_eq!(
        video["source"]["sourceRect"],
        json!({"x": 0.0, "y": 0.0, "width": 1080.0, "height": 1920.0})
    );
    assert_eq!(video["masks"][0]["mode"], json!("add"));
    assert_eq!(video["masks"][0]["layer"], guide["id"]);
    assert_eq!(video["masks"][0]["feather"], json!([24.0, 24.0]));
    assert_eq!(guide["transform"], video["transform"]);
    assert_eq!(guide["rect"]["position"], json!([135.0, 144.0]));
    assert_eq!(guide["rect"]["size"], json!([702.0, 1440.0]));

    let rebuilt = dir.path().join("crop-reencoded");
    tesseract_to_premiere(&archive, &rebuilt, false).unwrap();
    let reconverted = dir.path().join("crop-reconverted");
    premiere_to_tesseract(rebuilt.join("project.prproj"), &reconverted, None, false).unwrap();
    let projects = project_files(&reconverted);
    assert_eq!(projects.len(), 1);
    let roundtripped = TesseractFile::open(&projects[0])
        .unwrap()
        .project_json()
        .unwrap();
    let layers = roundtripped["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers[0]["masks"][0]["feather"], json!([24.0, 24.0]));
    assert_eq!(layers[1]["transform"], layers[0]["transform"]);
    assert_eq!(layers[1]["rect"]["position"], json!([135.0, 144.0]));
    assert_eq!(layers[1]["rect"]["size"], json!([702.0, 1440.0]));
}

#[test]
fn unsupported_crop_mask_omits_its_video_and_keeps_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (crop_file, _) = isolated_feature(root, "feature_media_fit_crop_strict.prproj");
    let mask = crop_file.project_json().unwrap()["composition"]["layers"][0]["masks"][0].clone();
    let mut document = document(root);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut sibling = layers[0].clone();
    sibling["id"] = json!(5);
    let mut path_mask = mask.clone();
    path_mask["layer"] = Value::Null;
    layers[0]["masks"] = json!([path_mask]);
    layers.insert(0, sibling);
    let edited = archive(root, &document, &root.join("source.mp4"));
    let output = root.join("unsupported-crop");
    let omissions = tesseract_to_premiere(&edited, &output, false).unwrap();
    // Exporting the video without its mask would show what the mask hides.
    assert!(
        omissions.iter().any(|omission| omission.to_string()
            == "occurrence layer 1 (\"Source\"): masks cannot be exported: the mask guide is not a rectangle or shape beside the video; occurrence omitted"),
        "{omissions:?}"
    );
    let project = PrProjectFile::load(output.join("project.prproj"))
        .unwrap()
        .0;
    let sequence = project.sequences().next().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1, "the sibling");
}

const STAGE_ORDER_FIXTURE: &str = "feature_stage_order_26_5_strict.prproj";
const STAGE_ORDER_SEQUENCE: &str = "82b6b2a3-0b54-4e98-ba50-d874e9be10ce";
const STAGE_MOTION_FIXTURE: &str = "feature_stage_motion_26_5_strict.prproj";
const STAGE_MOTION_SEQUENCE: &str = "a6a1d3c8-6481-4e34-a8cb-132545d69b7b";

/// Imports an Oracle run C6 stage fixture, Premiere 26.5.1's save as saved,
/// and returns its path, the document and its archive. The only omissions are
/// UI nodes and a master clip's `TimeDisplay`.
fn import_stage_fixture(
    root: &Path,
    fixture: &str,
    sequence: &str,
) -> (std::path::PathBuf, Value, std::path::PathBuf) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let output = root.join("stage-converted");
    let omissions = premiere_to_tesseract(&source, &output, Some(sequence), false).unwrap();
    assert!(
        omissions.iter().all(|omission| {
            omission.reason.contains("ClipTrackItem/TrackItem/Node")
                || omission.reason.contains("TimeDisplay")
        }),
        "{omissions:?}"
    );
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let document = TesseractFile::open(&projects[0])
        .unwrap()
        .project_json()
        .unwrap();
    (source, document, projects[0].clone())
}

/// Each masked clip of a stage fixture document (a stage group, or a video
/// with a mask), by start: its type and the effects of its video.
fn stage_clips(document: &Value) -> Vec<(i64, String, Value)> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group" || layer.get("masks").is_some())
        .map(|layer| {
            let video = if layer["type"] == "Group" {
                &layer["layers"][0]
            } else {
                layer
            };
            let effects = video["effects"]
                .as_array()
                .map(|effects| {
                    effects
                        .iter()
                        .map(|effect| effect["effect"].clone())
                        .collect()
                })
                .unwrap_or_default();
            (
                (*crate::test_support::layer_range(layer))["start"]
                    .as_i64()
                    .unwrap(),
                layer["type"].as_str().unwrap().to_owned(),
                Value::Array(effects),
            )
        })
        .collect()
}

/// The layer of `document`, at the top level or in a group, with `id`.
fn layer_by_id(document: &Value, id: &Value) -> Value {
    fn find(layers: &Value, id: &Value) -> Option<Value> {
        layers.as_array()?.iter().find_map(|layer| {
            (&layer["id"] == id)
                .then(|| layer.clone())
                .or_else(|| find(&layer["layers"], id))
        })
    }
    find(&document["composition"]["layers"], id).unwrap()
}

/// The guide of the mask on the layer that starts at `start`.
fn stage_guide(document: &Value, start: i64) -> Value {
    let masked = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer.get("masks").is_some()
                && (*crate::test_support::layer_range(layer))["start"] == json!(start)
        })
        .unwrap();
    layer_by_id(document, &masked["masks"][0]["layer"])
}

/// The keyed properties of layer `id`: each property's layer times and values.
fn layer_tracks(document: &Value, id: &Value) -> Vec<(String, Vec<(i64, f64)>)> {
    let mut tracks: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| &entry["target"]["layerId"] == id)
        .map(|entry| {
            let keys = entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| {
                    (
                        key["layerTime"].as_i64().unwrap(),
                        key["value"]["value"].as_f64().unwrap(),
                    )
                })
                .collect();
            (
                entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                keys,
            )
        })
        .collect();
    tracks.sort_by(|a, b| a.0.cmp(&b.0));
    tracks
}

/// The standard effects of each video clip of a native project that has any,
/// by start ticks: their match names in chain `Index` order (Premiere applies
/// them from the highest Index to the lowest).
fn standard_chains(project: &Path) -> Vec<(i64, Vec<String>)> {
    use std::collections::HashMap;
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let records: HashMap<&str, roxmltree::Node<'_, '_>> = document
        .root_element()
        .children()
        .filter_map(|node| Some((node.attribute("ObjectID")?, node)))
        .collect();
    fn child<'a, 'input>(
        node: roxmltree::Node<'a, 'input>,
        tag: &str,
    ) -> Option<roxmltree::Node<'a, 'input>> {
        node.children().find(|child| child.has_tag_name(tag))
    }
    let mut chains: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("VideoClipTrackItem"))
        .filter_map(|item| {
            let clip = child(item, "ClipTrackItem")?;
            let start = child(clip, "TrackItem")
                .and_then(|track_item| child(track_item, "Start"))
                .map_or(0, |start| start.text().unwrap().parse().unwrap());
            let chain = records
                [child(child(clip, "ComponentOwner")?, "Components")?.attribute("ObjectRef")?];
            let mut components: Vec<(usize, String)> =
                child(child(chain, "ComponentChain")?, "Components")?
                    .children()
                    .filter(|node| node.has_tag_name("Component"))
                    .map(|component| {
                        let record = records[component.attribute("ObjectRef").unwrap()];
                        (
                            component.attribute("Index").unwrap().parse().unwrap(),
                            child(record, "MatchName")
                                .and_then(|name| name.text())
                                .unwrap_or_default()
                                .to_owned(),
                        )
                    })
                    .filter(|(_, name)| {
                        !matches!(name.as_str(), "AE.ADBE Motion" | "AE.ADBE Opacity")
                    })
                    .collect();
            components.sort();
            (!components.is_empty()).then(|| {
                (
                    start,
                    components.into_iter().map(|(_, name)| name).collect(),
                )
            })
        })
        .collect();
    chains.sort();
    chains
}

/// Exports `archive`, checks that the written chains are the fixture's, and
/// that the export reimports as `document`. Returns the export's omissions.
fn assert_stage_fixture_round_trip(
    root: &Path,
    source: &Path,
    document: &Value,
    archive: &Path,
    chains: &[(i64, &[&str])],
) -> Vec<Omission> {
    let rebuilt = root.join("stage-exported");
    let omissions = tesseract_to_premiere(archive, &rebuilt, false).unwrap();
    let written = standard_chains(&rebuilt.join("project.prproj"));
    let mut expected: Vec<(i64, Vec<String>)> = chains
        .iter()
        .map(|(start, names)| {
            (
                *start,
                names.iter().map(|name| (*name).to_owned()).collect(),
            )
        })
        .collect();
    assert_eq!(standard_chains(source), expected);
    // Source coverage remains Legacy; export now writes the current native blur.
    for (_, names) in &mut expected {
        for name in names {
            if name == "AE.ADBE Gaussian Blur 2" {
                *name = "AE.Impact_Blur_FX".to_owned();
            }
        }
    }
    assert_eq!(written, expected);
    let reconverted = root.join("stage-reconverted");
    premiere_to_tesseract(rebuilt.join("project.prproj"), &reconverted, None, false).unwrap();
    let reimported = TesseractFile::open(first_project(&reconverted))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(reimported["composition"], document["composition"]);
    omissions
}

#[test]
fn adobe_stage_order_fixture_converts_each_order_and_writes_it_back() {
    // `premiere_isolated_stage_order_26_5` (Oracle run C6, F1): four timecoded
    // clips from source In 1 s. AME renders A and C sharp (the blur at chain
    // Index 1 applies before the Crop or wipe at Index 0) and B and D soft.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (source, document, archive) =
        import_stage_fixture(root, STAGE_ORDER_FIXTURE, STAGE_ORDER_SEQUENCE);
    let blur = |blurriness, repeat_edge| {
        let mut blur = json!({"type": "gaussianBlur", "blurriness": blurriness});
        if repeat_edge {
            blur["repeatEdgePixels"] = json!(true);
        }
        json!([blur])
    };
    assert_eq!(
        stage_clips(&document),
        [
            (0, "Group".to_owned(), blur(40.0, false)),
            (2000, "Video".to_owned(), blur(40.0, false)),
            (4000, "Group".to_owned(), blur(25.0, true)),
            (6000, "Video".to_owned(), blur(25.0, true)),
        ]
    );
    // The Crop Left 20, Top 15, Right 0, Bottom 10 of the 1920x1080 source; the
    // 90-degree wipe's Completion 0 at 1.25 s to 60 at 2.5 s, as guide ScaleX.
    for start in [0, 2000] {
        let guide = stage_guide(&document, start);
        assert_eq!(guide["rect"]["position"], json!([384.0, 162.0]));
        assert_eq!(guide["rect"]["size"], json!([1536.0, 810.0]));
    }
    for start in [4000, 6000] {
        let guide = stage_guide(&document, start);
        assert_eq!(
            layer_tracks(&document, &guide["id"]),
            [("scaleX".to_owned(), vec![(250, 100.0), (1500, 40.0)])]
        );
    }
    let omissions = assert_stage_fixture_round_trip(
        root,
        &source,
        &document,
        &archive,
        &[
            (0, &["AE.ADBE AECrop", "AE.ADBE Gaussian Blur 2"]),
            (
                508_032_000_000,
                &["AE.ADBE Gaussian Blur 2", "AE.ADBE AECrop"],
            ),
            (
                1_016_064_000_000,
                &["AE.ADBE Linear Wipe", "AE.ADBE Gaussian Blur 2"],
            ),
            (
                1_524_096_000_000,
                &["AE.ADBE Gaussian Blur 2", "AE.ADBE Linear Wipe"],
            ),
        ],
    );
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[test]
fn adobe_stage_motion_fixture_keeps_motion_masks_and_keys_and_writes_them_back() {
    // `premiere_isolated_stage_motion_26_5` (Oracle run C6, F1m): Premiere
    // applies the effects in the clip's frame and then Motion (Index 0).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (source, document, archive) =
        import_stage_fixture(root, STAGE_MOTION_FIXTURE, STAGE_MOTION_SEQUENCE);
    assert_eq!(
        stage_clips(&document),
        [
            (
                0,
                "Video".to_owned(),
                json!([{"type": "gaussianBlur", "blurriness": 40.0}])
            ),
            (2000, "Video".to_owned(), json!([])),
            (4000, "Group".to_owned(), json!([])),
            (
                6000,
                "Video".to_owned(),
                json!([{"type": "gaussianBlur", "blurriness": 30.0, "repeatEdgePixels": true}])
            ),
        ]
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    let at = |start: i64| {
        layers
            .iter()
            .find(|layer| {
                (*crate::test_support::layer_range(layer))["start"] == json!(start)
                    && layer["type"] != "Rect"
            })
            .unwrap()
    };
    let transform = |layer: &Value| {
        json!([
            &layer["transform"]["position"],
            &layer["transform"]["rotation"],
            &layer["transform"]["scale"]
        ])
    };
    // E: static Scale 80, Rotation 15 and Position 0.45:0.55; the blur applies
    // after the Crop of A, so the clip is one video whose guide repeats its
    // transform.
    let e_guide = stage_guide(&document, 0);
    assert_eq!(
        transform(at(0)),
        json!([[864.0, 594.0], 15.0, [80.0, 80.0]])
    );
    assert_eq!(transform(&e_guide), transform(at(0)));
    assert_eq!(e_guide["rect"]["size"], json!([1536.0, 810.0]));
    // F: the Crop of A with Scale 100 to 80 and Rotation 0 to 20 from 1.25 s to
    // 2.75 s; the guide repeats the video's keys.
    let f_keys = [
        ("rotation".to_owned(), vec![(250, 0.0), (1750, 20.0)]),
        ("scaleX".to_owned(), vec![(250, 100.0), (1750, 80.0)]),
        ("scaleY".to_owned(), vec![(250, 100.0), (1750, 80.0)]),
    ];
    assert_eq!(layer_tracks(&document, &at(2000)["id"]), f_keys);
    assert_eq!(
        layer_tracks(&document, &stage_guide(&document, 2000)["id"]),
        f_keys
    );
    // G: the wipe of C on a clip with Scale 80 and Position 0.45:0.55 is
    // staged: the group carries the Motion, the video sits at the identity.
    let g = at(4000);
    assert_eq!(transform(g), json!([[864.0, 594.0], 0.0, [80.0, 80.0]]));
    assert_eq!(
        transform(&g["layers"][0]),
        json!([[0.0, 0.0], 0.0, [100.0, 100.0]])
    );
    assert_eq!(
        layer_tracks(&document, &g["layers"][1]["id"]),
        [("scaleX".to_owned(), vec![(250, 100.0), (1500, 40.0)])]
    );
    // H: the 1080x1920 source at Scale 50 with Crop Left 10, Top 20, Right 10,
    // Bottom 5 in source pixels.
    assert_eq!(
        at(6000)["source"]["sourceRect"],
        json!({"x": 0.0, "y": 0.0, "width": 1080.0, "height": 1920.0})
    );
    assert_eq!(
        transform(at(6000)),
        json!([[960.0, 540.0], 0.0, [50.0, 50.0]])
    );
    let h_guide = stage_guide(&document, 6000);
    assert_eq!(h_guide["rect"]["position"], json!([108.0, 384.0]));
    assert_eq!(h_guide["rect"]["size"], json!([864.0, 1440.0]));
    let omissions = assert_stage_fixture_round_trip(
        root,
        &source,
        &document,
        &archive,
        &[
            (0, &["AE.ADBE Gaussian Blur 2", "AE.ADBE AECrop"]),
            (508_032_000_000, &["AE.ADBE AECrop"]),
            (1_016_064_000_000, &["AE.ADBE Linear Wipe"]),
            (
                1_524_096_000_000,
                &["AE.ADBE Gaussian Blur 2", "AE.ADBE AECrop"],
            ),
        ],
    );
    assert!(omissions.is_empty(), "{omissions:?}");
}

/// Moves the first layer of `document`, a video, and its Crop guide, the
/// second, under a stage group 10 that takes the video's range, transform and
/// mask, as import stages a clip. The third layer is the black canvas, at the
/// identity.
fn stage_first_video(document: &mut Value) {
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let identity = layers[2]["transform"].clone();
    let mut video = layers.remove(0);
    let mut guide = layers.remove(0);
    let mut group = json!({
        "type": "Group",
        "id": 10,
        "name": "Premiere stage 1",
        "playback": crate::test_support::linear_playback(json!(*crate::test_support::layer_range(&video)), json!({"start": 0, "duration": (*crate::test_support::layer_range(&video))["duration"]})),
        "transform": video["transform"],
        "masks": video["masks"],
    });
    let local_range =
        json!({"start": 0, "duration": (*crate::test_support::layer_range(&video))["duration"]});
    for layer in [&mut video, &mut guide] {
        layer["transform"] = identity.clone();
        if layer["type"] == "Video" {
            layer["playback"] = crate::test_support::linear_playback(
                local_range.clone(),
                layer["sourceRange"].clone(),
            );
        } else {
            assert_eq!(layer["type"], "Rect");
            layer["activeRange"] = local_range.clone();
        }
        layer["parent"] = json!(10);
    }
    video.as_object_mut().unwrap().remove("masks");
    group["layers"] = json!([video, guide]);
    layers.insert(0, group);
}

#[test]
fn stage_group_round_trips_as_one_native_clip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // The flat Crop of the pinned portrait fixture, staged under a group with a
    // blur that applies before the Crop, as import stages such a clip.
    let (file, _) = isolated_feature(root, "feature_media_fit_crop_strict.prproj");
    let mut document = file.project_json().unwrap();
    stage_first_video(&mut document);
    let stage = &mut document["composition"]["layers"][0];
    stage["layers"][0]["effects"] =
        json!([{"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 30.0}}]);
    let guide = stage["layers"][1].clone();
    let media =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_crop_portrait.mp4");
    let edited = archive(root, &document, &media);
    let output = root.join("staged");
    let omissions = tesseract_to_premiere(&edited, &output, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // One native clip, which import stages again with the same blur and Crop.
    let native = output.join("project.prproj");
    let (project, _) = PrProjectFile::load(&native).unwrap();
    let sequence = project.sequences().next().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);
    let omissions = premiere_to_tesseract(&native, root.join("reimported"), None, false).unwrap();
    assert!(
        omissions.iter().all(|omission| omission
            .reason
            .contains("Crop Edge Feather is approximated")),
        "{omissions:?}"
    );
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    let stage = &reimported["composition"]["layers"][0];
    assert_eq!(stage["type"], "Group");
    assert_eq!(
        stage["layers"][0]["effects"][0]["effect"]["blurriness"],
        30.0
    );
    assert_eq!(stage["layers"][1]["rect"], guide["rect"]);
}

#[test]
fn media_of_a_clip_omitted_for_its_mask_is_not_inspected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (file, _) = isolated_feature(root, "feature_media_fit_crop_strict.prproj");
    let media =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_crop_portrait.mp4");
    let broken = root.join("broken.mp4");
    fs::write(&broken, b"not a video").unwrap();
    let music = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio-mono.wav");
    for (staged, reason) in [
        (
            false,
            "masks cannot be exported: the mask is inverted; occurrence omitted",
        ),
        (
            true,
            "group was not exported as a nested sequence: the mask is inverted",
        ),
    ] {
        // The fixture's cropped video, its mask inverted, plays an asset whose
        // bytes are not a video, at a positive volume; a copy without the mask
        // plays the fixture's. An audio layer plays an asset of its own.
        let mut document = file.project_json().unwrap();
        let layers = document["composition"]["layers"].as_array_mut().unwrap();
        let mut sibling = layers[0].clone();
        sibling["id"] = json!(20);
        sibling.as_object_mut().unwrap().remove("masks");
        layers[0]["masks"][0]["inverted"] = json!(true);
        layers[0]["source"]["assetId"] = json!("broken-video");
        layers[0]["volume"] = json!(1.0);
        if staged {
            stage_first_video(&mut document);
        }
        let layers = document["composition"]["layers"].as_array_mut().unwrap();
        layers.insert(0, sibling);
        layers.insert(
            0,
            json!({
                "type": "Audio",
                "id": 21,
                "name": "Music",
                "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 200}), json!({"start": 0, "duration": 200})),
                "sourceRange": {"start": 0, "duration": 200},
                "sourceIntrinsicDuration": 200,
                "volume": 1.0,
                "source": {"assetId": "music"},
            }),
        );
        let edited = root.join(format!("edited-{staged}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset("premiere-video-1", &media, AssetKind::Video)
            .unwrap()
            .add_asset("broken-video", &broken, AssetKind::Video)
            .unwrap()
            .add_asset("music", &music, AssetKind::Audio)
            .unwrap()
            .write(&edited)
            .unwrap();
        let output = root.join(format!("native-{staged}"));
        let omissions = tesseract_to_premiere(&edited, &output, false).unwrap();
        assert!(
            omissions.iter().any(|omission| omission.reason == reason),
            "{omissions:?}"
        );
        let project = PrProjectFile::load(output.join("project.prproj"))
            .unwrap()
            .0;
        let sequence = project.sequences().next().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1, "the sibling");
        // The audio layer's asset is inspected and its sound exports.
        let xml = read_xml(&output.join("project.prproj"));
        assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 1);
    }
}

#[test]
fn media_of_an_image_omitted_for_its_mask_is_not_inspected() {
    // A still exports no Crop or Linear Wipe, so an image with a mask is
    // omitted whole, and its asset, whose bytes are not an image, is never
    // opened. An unmasked image and the video beside it export.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut document = document(root);
    let broken = root.join("broken.png");
    fs::write(&broken, b"not an image").unwrap();
    let still =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_still_opaque.jpg");
    let identity = document["composition"]["layers"][0]["transform"].clone();
    let whole = json!({"start": 0, "duration": 1000});
    let image = |id: u64, name: &str, asset: &str| {
        json!({
            "type": "Image",
            "id": id,
            "name": name,
            "activeRange": whole,
            "transform": identity,
            "source": {
                "assetId": asset,
                "fit": "contain",
                "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080},
            },
        })
    };
    let mut masked = image(30, "Masked", "broken-image");
    masked["masks"] = json!([{"id": 32, "mode": "add", "layer": 31}]);
    let guide = json!({
        "type": "Rect",
        "id": 31,
        "name": "Guide",
        "activeRange": whole,
        "transform": identity,
        "rect": {"size": [1920.0, 1080.0], "fillColor": [1, 1, 1, 1]},
    });
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    for layer in [image(33, "Unmasked", "still"), guide, masked] {
        layers.insert(0, layer);
    }
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("broken-image", &broken, AssetKind::Image)
        .unwrap()
        .add_asset("still", &still, AssetKind::Image)
        .unwrap()
        .write(&edited)
        .unwrap();
    let output = root.join("native");
    let omissions = tesseract_to_premiere(&edited, &output, false).unwrap();
    assert!(
        omissions.iter().any(|omission| omission.record == "layer 30 (\"Masked\")"
            && omission.reason
                == "masks cannot be exported: a still image exports no Crop, Linear Wipe or Track Matte Key; occurrence omitted"),
        "{omissions:?}"
    );
    let project = PrProjectFile::load(output.join("project.prproj"))
        .unwrap()
        .0;
    let sequence = project.sequences().next().unwrap();
    let mut stills: Vec<bool> = sequence
        .video_occurrences()
        .map(|clip| project.media(clip).unwrap().is_still())
        .collect();
    stills.sort_unstable();
    assert_eq!(stills, [false, true], "the video and the unmasked image");
}

/// The 100 ms from 100 ms into the 200 ms `video-with-audio.mp4`, placed at
/// 500 ms at volume 1 with a Crop (Top 15%): the video, its Crop guide and the
/// black canvas.
fn audible_cropped_document() -> Value {
    let mut document = crate::test_support::editable_document();
    document["duration"] = json!(0.6);
    let placed = json!({"start": 500, "duration": 100});
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[1]["activeRange"] = json!({"start": 0, "duration": 600});
    let video = &mut layers[0];
    video["playback"] = crate::test_support::linear_playback(
        placed.clone(),
        json!({"start": 100, "duration": 100}),
    );
    video["sourceRange"] = json!({"start": 100, "duration": 100});
    video["sourceIntrinsicDuration"] = json!(200);
    video["volume"] = json!(1.0);
    video["masks"] = json!([{"id": 12, "mode": "add", "layer": 11, "feather": [0.0, 0.0]}]);
    let guide = json!({
        "type": "Rect",
        "id": 11,
        "name": "Premiere Crop guide 1",
        "activeRange": placed,
        "transform": video["transform"],
        "rect": {"position": [0.0, 162.0], "size": [1920.0, 918.0], "fillColor": [0, 0, 0, 1]},
    });
    layers.insert(1, guide);
    document
}

/// The timeline Start and End and the source In of each sound clip of a
/// native project: its `TrackItem`, and the `InPoint` of the `AudioClip` that
/// its `SubClip` references. Premiere leaves out a zero Start.
fn sound_clip_ticks(document: &roxmltree::Document<'_>) -> Vec<[i64; 3]> {
    fn child<'a, 'input>(
        node: roxmltree::Node<'a, 'input>,
        tag: &str,
    ) -> roxmltree::Node<'a, 'input> {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .unwrap()
    }
    fn referenced<'a, 'input>(
        document: &'a roxmltree::Document<'input>,
        reference: roxmltree::Node<'_, '_>,
    ) -> roxmltree::Node<'a, 'input> {
        let id = reference.attribute("ObjectRef").unwrap();
        document
            .descendants()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    }
    fn ticks(node: roxmltree::Node<'_, '_>, tag: &str) -> i64 {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .map_or(0, |child| child.text().unwrap().parse().unwrap())
    }
    document
        .descendants()
        .filter(|node| node.has_tag_name("AudioClipTrackItem"))
        .map(|item| {
            let clip_track_item = child(item, "ClipTrackItem");
            let range = child(clip_track_item, "TrackItem");
            let sub_clip = referenced(document, child(clip_track_item, "SubClip"));
            let clip = referenced(document, child(sub_clip, "Clip"));
            [
                ticks(range, "Start"),
                ticks(range, "End"),
                ticks(child(clip, "Clip"), "InPoint"),
            ]
        })
        .collect()
}

#[test]
fn stage_group_exports_the_sound_of_its_video_as_a_flat_clip_does() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let media = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-with-audio.mp4");
    let mut exports = Vec::new();
    for staged in [false, true] {
        let mut document = audible_cropped_document();
        if staged {
            stage_first_video(&mut document);
        }
        let edited = root.join(format!("audible-{staged}.tsrct"));
        write_archive(&edited, &document, &media);
        let output = root.join(format!("audible-native-{staged}"));
        tesseract_to_premiere(&edited, &output, false).unwrap();
        let xml = read_xml(&output.join("project.prproj"));
        let native = roxmltree::Document::parse(&xml).unwrap();
        exports.push((
            sound_clip_ticks(&native),
            track_item_ticks(&native, "Start"),
            track_item_ticks(&native, "End"),
        ));
    }
    // The staged clip's one sound plays over the group's range, from 500 to
    // 600 ms, from 100 ms into its source (254016000000 ticks per second).
    assert_eq!(
        exports[1].0,
        [[127_008_000_000, 152_409_600_000, 25_401_600_000]]
    );
    // The flat clip's picture and sound are at the same times.
    assert_eq!(exports[1], exports[0]);
}

/// Imports `fixture`, one 2 s clip of `feature_timecoded_source.mp4` at the
/// canvas center that shows its source from `source_start` ms, and checks
/// that its only keys are `keys` (layer time in ms, value and easing type) on
/// each of `properties`. The archive round-trips as `evidence` expects.
/// Returns each property's keys, for checks beyond their type.
fn assert_center_clip_motion_keys(
    fixture: &str,
    evidence: &str,
    source_start: u64,
    properties: &[&str],
    keys: &[(u64, f64, &str)],
) -> Vec<Vec<Value>> {
    let dir = tempfile::tempdir().unwrap();
    let (file, archive) = isolated_feature(dir.path(), fixture);
    assert_eq!(file.metadata().assets.len(), 1);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 1);
    let layer = layers[0];
    assert!(layer_uses_media(
        &file,
        layer,
        "feature_timecoded_source.mp4"
    ));
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        layer["sourceRange"],
        json!({"start": source_start, "duration": 2000})
    );
    assert_eq!(layer["transform"]["anchorPoint"], json!([960.0, 540.0]));
    assert_eq!(layer["transform"]["position"], json!([960.0, 540.0]));
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), properties.len());
    let per_property = properties
        .iter()
        .map(|property| {
            let entry = entries
                .iter()
                .find(|entry| entry["target"]["propertyType"] == *property)
                .unwrap();
            assert_eq!(entry["target"]["layerId"], layer["id"]);
            assert_eq!(entry["animator"]["type"], "keyframes");
            let actual = entry["animator"]["keyframes"].as_array().unwrap();
            let summary: Vec<_> = actual
                .iter()
                .map(|key| {
                    (
                        key["layerTime"].as_u64().unwrap(),
                        key["value"]["value"].as_f64().unwrap(),
                        key["easing"]["type"].as_str().unwrap(),
                    )
                })
                .collect();
            assert_eq!(summary, keys, "{property}");
            actual.clone()
        })
        .collect();
    assert_editable_feature_roundtrip(dir.path(), fixture, evidence, &archive);
    per_property
}

const SCALE_AXES: [&str; 2] = ["scaleX", "scaleY"];

#[test]
fn adobe_hold_rotation_keeps_editable_source_clock_keys() {
    assert_center_clip_motion_keys(
        "feature_motion_rotation_hold_strict.prproj",
        "feature_motion_rotation_hold_strict",
        0,
        &["rotation"],
        &[(500, 0.0, "linear"), (1500, 60.0, "hold")],
    );
}

#[test]
fn adobe_trimmed_source_rotation_retains_source_clock_keys() {
    // Native source keys at 3.5/4.5 s must follow the 3 s source trim.
    assert_center_clip_motion_keys(
        "feature_motion_rotation_trimmed_source_strict.prproj",
        "feature_motion_rotation_trimmed_source_strict",
        3000,
        &["rotation"],
        &[(500, 0.0, "linear"), (1500, 60.0, "linear")],
    );
}

#[test]
fn adobe_trimmed_source_scale_keeps_two_editable_source_clock_axes() {
    assert_center_clip_motion_keys(
        "feature_motion_scale_trimmed_source_strict.prproj",
        "feature_motion_scale_trimmed_source_strict",
        3000,
        &SCALE_AXES,
        &[(500, 100.0, "linear"), (1500, 150.0, "linear")],
    );
}

#[test]
fn adobe_hold_scale_keeps_two_editable_axis_keys() {
    assert_center_clip_motion_keys(
        "feature_motion_scale_hold_strict.prproj",
        "feature_motion_scale_hold_strict",
        0,
        &SCALE_AXES,
        &[(500, 100.0, "linear"), (1500, 150.0, "hold")],
    );
}

#[test]
fn adobe_exported_asymmetric_bezier_scale_keeps_editable_handles() {
    let axes = assert_center_clip_motion_keys(
        "feature_motion_scale_bezier_strict.prproj",
        "feature_motion_scale_bezier_strict",
        0,
        &SCALE_AXES,
        &[(500, 100.0, "linear"), (1500, 150.0, "cubicBezier")],
    );
    for keys in axes {
        let easing = &keys[1]["easing"];
        for (handle, expected) in [("x1", 0.4), ("y1", 0.024), ("x2", 1.0), ("y2", 1.0)] {
            let actual = easing[handle].as_f64().unwrap();
            assert!((actual - expected).abs() < 1e-12, "{handle}: {actual}");
        }
    }
}

#[test]
fn source_aligned_asymmetric_bezier_scale_imports_exact_native_controls() {
    const LEGACY_FIXTURE: &str = "feature_motion_scale_bezier_asymmetric_strict.prproj";
    const ALIGNED_FIXTURE: &str = "feature_motion_scale_bezier_asymmetric_aligned_strict.prproj";
    const LEGACY_KEYS: &str = "63504000000,0.,5,0,0,0.16666666666666666,51.79436120439027,0.75494403761504603;232848000000,100.,5,0,31.890558862255716,0.88142724889955981,0,0.16666666666666666;";
    const ALIGNED_KEYS: &str = "0,0.,5,0,0,0.16666666666666666,51.79436120439027,0.75494403761504603;169344000000,100.,5,0,31.890558862255716,0.88142724889955981,0,0.16666666666666666;";

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let legacy_xml = read_xml(&fixtures.join(LEGACY_FIXTURE));
    let aligned_xml = read_xml(&fixtures.join(ALIGNED_FIXTURE));
    assert_eq!(legacy_xml.matches(LEGACY_KEYS).count(), 1);
    assert_eq!(aligned_xml, legacy_xml.replace(LEGACY_KEYS, ALIGNED_KEYS));

    // The legacy keys start 250 ms after the In, where the static Scale is
    // 100. Premiere holds the first key (0) before it, as the FX animator
    // does, so the legacy fixture imports the aligned keys and Bezier
    // controls 250 ms later, with no key before the first.
    for (fixture, first_key_millis) in [(ALIGNED_FIXTURE, 0), (LEGACY_FIXTURE, 250)] {
        let dir = tempfile::tempdir().unwrap();
        let (file, _) = isolated_feature(dir.path(), fixture);
        let document = file.project_json().unwrap();
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        assert_eq!(entries.len(), 2, "{fixture}");
        for property in ["scaleX", "scaleY"] {
            let entry = entries
                .iter()
                .find(|entry| entry["target"]["propertyType"] == property)
                .unwrap();
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2, "{fixture}");
            assert_eq!(keys[0]["layerTime"], first_key_millis, "{fixture}");
            assert_eq!(keys[0]["value"]["value"], 0.0, "{fixture}");
            assert_eq!(keys[1]["layerTime"], first_key_millis + 667, "{fixture}");
            assert_eq!(keys[1]["value"]["value"], 100.0, "{fixture}");
            let easing = &keys[1]["easing"];
            assert_eq!(easing["type"], "cubicBezier", "{fixture}");
            for (handle, expected) in [
                ("x1", 0.754944037615046),
                ("y1", 0.26067896115556327),
                ("x2", 0.11857275110044019),
                ("y2", 0.8126052829078165),
            ] {
                let actual = easing[handle].as_f64().unwrap();
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "{fixture} {handle}: {actual}"
                );
            }
        }
    }
}

/// Uniform Scale keys import as one editable key track per axis. Both
/// fixtures round-trip as the `feature_motion_scale_linear_strict` evidence
/// expects; the derived graph failed Adobe export, so only its editable
/// structure is asserted.
#[test]
fn native_derived_uniform_scale_preserves_two_editable_axes_and_center_pivot() {
    assert_uniform_scale_keys("feature_motion_scale_linear_strict.prproj");
}

#[test]
fn adobe_exported_uniform_scale_preserves_two_editable_axes_and_center_pivot() {
    assert_uniform_scale_keys("feature_motion_scale_linear_private_strict.prproj");
}

fn assert_uniform_scale_keys(fixture: &str) {
    assert_center_clip_motion_keys(
        fixture,
        "feature_motion_scale_linear_strict",
        0,
        &SCALE_AXES,
        &[(500, 100.0, "linear"), (1500, 150.0, "linear")],
    );
}

#[test]
fn adobe_repeated_adjacent_source_keeps_one_asset_with_two_in_points() {
    let dir = tempfile::tempdir().unwrap();
    let (file, archive) =
        isolated_feature(dir.path(), "feature_repeated_adjacent_source_strict.prproj");
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(file.metadata().assets.len(), 1);
    assert_eq!(layers.len(), 2);
    let first = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 0)
        .unwrap();
    let second = layers
        .iter()
        .find(|layer| (*crate::test_support::layer_range(layer))["start"] == 2000)
        .unwrap();
    assert!(layer_uses_media(
        &file,
        first,
        "feature_timecoded_source.mp4"
    ));
    assert!(layer_uses_media(
        &file,
        second,
        "feature_timecoded_source.mp4"
    ));
    assert_eq!(first["source"]["assetId"], second["source"]["assetId"]);
    assert_eq!(
        (*crate::test_support::layer_range(first)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(second)),
        json!({"start": 2000, "duration": 2000})
    );
    assert_eq!(first["sourceRange"], json!({"start": 0, "duration": 2000}));
    assert_eq!(
        second["sourceRange"],
        json!({"start": 3000, "duration": 2000})
    );
    assert_editable_feature_roundtrip(
        dir.path(),
        "feature_repeated_adjacent_source_strict.prproj",
        "Repeated adjacent source",
        &archive,
    );
}

#[test]
fn premiere_to_tesseract_preserves_cross_track_overlap() {
    let dir = tempfile::tempdir().unwrap();
    let file = converted_adobe_sequence(dir.path(), TWO_TRACKS_OVERLAP);
    let document = file.project_json().unwrap();
    let mut ranges: Vec<_> = video_layers(&document)
        .into_iter()
        .map(|layer| {
            (
                (*crate::test_support::layer_range(layer))["start"]
                    .as_i64()
                    .unwrap(),
                (*crate::test_support::layer_range(layer))["duration"]
                    .as_i64()
                    .unwrap(),
            )
        })
        .collect();
    ranges.sort_unstable();

    assert_eq!(ranges, [(0, 5000), (2000, 2000)]);
    assert!(ranges[0].0 + ranges[0].1 > ranges[1].0);
}

#[test]
fn premiere_to_tesseract_selects_only_the_requested_sequence_guid() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("selected");
    premiere_to_tesseract(
        two_video_tracks_fixture(root),
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();

    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let selected = TesseractFile::open(&projects[0]).unwrap();
    assert_eq!(selected.project().composition().name(), "Two tracks gap");
    assert_eq!(video_layers(&selected.project_json().unwrap()).len(), 3);
}

#[test]
fn adobe_nested_second_sequence_is_selectable_by_guid() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(root.join("media")).unwrap();
    fs::copy(
        fixtures.join("feature_nested_second_sequence_strict.prproj"),
        root.join("project.prproj"),
    )
    .unwrap();
    for (name, source) in [
        ("clip-a.mp4", "feature_multi_sequence_red_10s.mp4"),
        ("clip-b.mp4", "feature_multi_sequence_blue_10s.mp4"),
    ] {
        fs::copy(fixtures.join(source), root.join("media").join(name)).unwrap();
    }
    let (default_project, _) = PrProjectFile::load(root.join("project.prproj")).unwrap();
    let default_roots = default_project.sequences().collect::<Vec<_>>();
    assert_eq!(default_roots.len(), 1);
    assert_eq!(default_roots[0].name(), "Two tracks overlap");

    let output = root.join("selected-nested-sequence");
    premiere_to_tesseract(
        root.join("project.prproj"),
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let file = TesseractFile::open(&projects[0]).unwrap();
    assert_eq!(file.project().composition().name(), "Two tracks gap");
    assert_eq!(file.metadata().assets.len(), 2);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 3);
    for (layer, asset, start, duration, source_start) in [
        (layers[0], "clip-b.mp4", 1000, 2000, 0),
        (layers[1], "clip-a.mp4", 0, 2000, 0),
        (layers[2], "clip-a.mp4", 4000, 1000, 4000),
    ] {
        assert!(layer_uses_media(&file, layer, asset));
        assert_eq!(
            (*crate::test_support::layer_range(layer)),
            json!({"start": start, "duration": duration})
        );
        assert_eq!(
            layer["sourceRange"],
            json!({"start": source_start, "duration": duration})
        );
    }
    let canvas = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(canvas["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(canvas)),
        json!({"start": 0, "duration": 5000})
    );
}

#[test]
fn adobe_second_sequence_selects_only_its_distinct_color_sources() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(root.join("media")).unwrap();
    fs::copy(
        fixture.join("two-video-tracks.prproj"),
        root.join("project.prproj"),
    )
    .unwrap();
    for (name, source) in [
        ("clip-a.mp4", "feature_multi_sequence_red_10s.mp4"),
        ("clip-b.mp4", "feature_multi_sequence_blue_10s.mp4"),
    ] {
        fs::copy(fixture.join(source), root.join("media").join(name)).unwrap();
    }
    let output = root.join("selected-color-sequence");
    premiere_to_tesseract(
        root.join("project.prproj"),
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    let projects = project_files(&output);
    assert_eq!(projects.len(), 1);
    let file = TesseractFile::open(&projects[0]).unwrap();
    assert_eq!(file.project().composition().name(), "Two tracks gap");
    assert_eq!(file.metadata().assets.len(), 2);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 3);
    // The second root has two red cuts and one blue overlay; the first root
    // instead has a single long red clip. Selected UID matters to the pixels.
    for (layer, asset, active, source) in [
        (
            layers[0],
            "clip-b.mp4",
            json!({"start": 1000, "duration": 2000}),
            json!({"start": 0, "duration": 2000}),
        ),
        (
            layers[1],
            "clip-a.mp4",
            json!({"start": 0, "duration": 2000}),
            json!({"start": 0, "duration": 2000}),
        ),
        (
            layers[2],
            "clip-a.mp4",
            json!({"start": 4000, "duration": 1000}),
            json!({"start": 4000, "duration": 1000}),
        ),
    ] {
        assert!(layer_uses_media(&file, layer, asset));
        assert_eq!((*crate::test_support::layer_range(layer)), active);
        assert_eq!(layer["sourceRange"], source);
    }
    let canvas = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(canvas["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(canvas)),
        json!({"start": 0, "duration": 5000})
    );
}

#[test]
fn tesseract_to_premiere_adds_and_moves_clips_with_exact_source_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut document = document(root);
    let mut moved = document["composition"]["layers"][0].clone();
    moved["playback"] = crate::test_support::linear_playback(
        json!({"start": 1000, "duration": 500}),
        json!({"start": 500, "duration": 500}),
    );
    moved["sourceRange"] = json!({"start": 500, "duration": 500});
    let mut added = document["composition"]["layers"][0].clone();
    added["id"] = json!(3);
    added["playback"] = crate::test_support::linear_playback(
        json!({"start": 2500, "duration": 500}),
        json!({"start": 0, "duration": 500}),
    );
    added["sourceRange"] = json!({"start": 0, "duration": 500});
    let mut canvas = document["composition"]["layers"][1].clone();
    canvas["activeRange"]["duration"] = json!(3000);
    document["composition"]["layers"] = json!([moved, added, canvas]);
    document["duration"] = json!(3.0);

    let input = archive(root, &document, &root.join("source.mp4"));
    tesseract_to_premiere(&input, root.join("native"), false).unwrap();
    let project = PrProjectFile::load(root.join("native/project.prproj"))
        .unwrap()
        .0;
    let sequence = project.sequences().next().unwrap();
    assert_eq!(
        sequence_table(&project, sequence),
        json!({
            "name": "Fresh exact 30",
            "video_track_count": 1,
            "occurrences": [
                {
                    "track_index": 0,
                    "media_name": "source.mp4",
                    "timeline_ticks": [TICKS, 3 * TICKS / 2],
                    "source_ticks": [TICKS / 2, TICKS],
                },
                {
                    "track_index": 0,
                    "media_name": "source.mp4",
                    "timeline_ticks": [5 * TICKS / 2, 3 * TICKS],
                    "source_ticks": [0, TICKS / 2],
                },
            ],
        })
    );
}

#[test]
fn tesseract_to_premiere_omits_a_deleted_middle_clip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut document = document(root);
    let base = document["composition"]["layers"][0].clone();
    let mut clips = Vec::new();
    for (id, start, source_start) in [(10, 0, 0), (11, 1000, 500), (12, 2000, 500)] {
        let mut clip = base.clone();
        clip["id"] = json!(id);
        clip["playback"] = crate::test_support::linear_playback(
            json!({"start": start, "duration": 500}),
            json!({"start": source_start, "duration": 500}),
        );
        clip["sourceRange"] = json!({"start": source_start, "duration": 500});
        clips.push(clip);
    }
    clips.remove(1);
    let mut canvas = document["composition"]["layers"][1].clone();
    canvas["activeRange"]["duration"] = json!(2500);
    clips.push(canvas);
    document["composition"]["layers"] = json!(clips);
    document["duration"] = json!(2.5);

    let input = archive(root, &document, &root.join("source.mp4"));
    tesseract_to_premiere(&input, root.join("native"), false).unwrap();
    let project = PrProjectFile::load(root.join("native/project.prproj"))
        .unwrap()
        .0;
    let sequence = project.sequences().next().unwrap();
    assert_eq!(
        sequence_table(&project, sequence),
        json!({
            "name": "Fresh exact 30",
            "video_track_count": 1,
            "occurrences": [
                {
                    "track_index": 0,
                    "media_name": "source.mp4",
                    "timeline_ticks": [0, TICKS / 2],
                    "source_ticks": [0, TICKS / 2],
                },
                {
                    "track_index": 0,
                    "media_name": "source.mp4",
                    "timeline_ticks": [2 * TICKS, 5 * TICKS / 2],
                    "source_ticks": [TICKS / 2, TICKS],
                },
            ],
        })
    );
}

#[test]
fn native_media_is_shared_across_project_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let project = PrProjectFile::load(two_video_tracks_fixture(dir.path()))
        .unwrap()
        .0;
    assert_eq!(project.sequences().len(), 2);
    let ids: Vec<_> = project
        .sequences()
        .flat_map(|sequence| sequence.video_occurrences())
        .map(|clip| clip.media_id().as_str().to_owned())
        .collect();
    assert_eq!(ids.len(), 5);
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        2
    );
    assert!(project
        .sequences()
        .flat_map(|sequence| sequence.video_occurrences())
        .all(|clip| project.media(clip).is_some()));
}

#[test]
fn adobe_video_tracks_round_trip_with_order_and_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let expected = json!([
        {
            "name": "Two tracks overlap",
            "video_track_count": 2,
            "occurrences": [
                {"track_index": 0, "timeline_ticks": [0, 5 * TICKS], "source_ticks": [0, 5 * TICKS], "media_name": "clip-a.mp4"},
                {"track_index": 1, "timeline_ticks": [2 * TICKS, 4 * TICKS], "source_ticks": [3 * TICKS, 5 * TICKS], "media_name": "clip-b.mp4"},
            ],
        },
        {
            "name": "Two tracks gap",
            "video_track_count": 2,
            "occurrences": [
                {"track_index": 0, "timeline_ticks": [0, 2 * TICKS], "source_ticks": [0, 2 * TICKS], "media_name": "clip-a.mp4"},
                {"track_index": 0, "timeline_ticks": [4 * TICKS, 5 * TICKS], "source_ticks": [4 * TICKS, 5 * TICKS], "media_name": "clip-a.mp4"},
                {"track_index": 1, "timeline_ticks": [TICKS, 3 * TICKS], "source_ticks": [0, 2 * TICKS], "media_name": "clip-b.mp4"},
            ],
        },
    ]);
    let fixture = two_video_tracks_fixture(root);
    for (index, (wanted, sequence)) in expected
        .as_array()
        .unwrap()
        .iter()
        .zip([TWO_TRACKS_OVERLAP, TWO_TRACKS_GAP])
        .enumerate()
    {
        // Each root sequence imports as its own selected project.
        let output = root.join(format!("tesseract-{index}"));
        premiere_to_tesseract(&fixture, &output, Some(sequence), false).unwrap();
        let file = TesseractFile::open(first_project(&output)).unwrap();
        assert_eq!(
            file.project().composition().name(),
            wanted["name"].as_str().unwrap()
        );
        assert_eq!(file.metadata().assets.len(), 2);
        for (asset_id, expected) in [("premiere-video-1", CLIP_A), ("premiere-video-2", CLIP_B)] {
            let mut packaged = Vec::new();
            file.asset(asset_id)
                .unwrap()
                .open()
                .unwrap()
                .read_to_end(&mut packaged)
                .unwrap();
            assert_eq!(packaged, expected);
        }
        assert_ne!(CLIP_A, CLIP_B);
        let document = file.project_json().unwrap();
        let layers = document["composition"]["layers"].as_array().unwrap();
        let mut occurrences = wanted["occurrences"].as_array().unwrap().clone();
        occurrences.sort_by_key(|clip| std::cmp::Reverse(clip["track_index"].as_u64().unwrap()));
        assert_eq!(layers.len(), occurrences.len() + 1);
        assert_eq!(layers.last().unwrap()["type"], "Rect");
        for (layer, occurrence) in layers.iter().zip(occurrences) {
            let asset = &file.metadata().assets[layer["source"]["assetId"].as_str().unwrap()];
            assert_eq!(
                Path::new(&asset.path)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                occurrence["media_name"]
            );
            for (field, ticks) in [
                ("activeRange", "timeline_ticks"),
                ("sourceRange", "source_ticks"),
            ] {
                let start = occurrence[ticks][0].as_i64().unwrap() * 1000 / TICKS;
                let end = occurrence[ticks][1].as_i64().unwrap() * 1000 / TICKS;
                assert_eq!(
                    if field == "activeRange" {
                        crate::test_support::layer_range(layer)
                    } else {
                        &layer[field]
                    },
                    &json!({"start": start, "duration": end - start})
                );
            }
        }
        let native = root.join(format!("native-{index}"));
        tesseract_to_premiere(first_project(&output), &native, false).unwrap();
        let rebuilt = PrProjectFile::load(native.join("project.prproj"))
            .unwrap()
            .0;
        assert_eq!(
            sequence_table(&rebuilt, rebuilt.sequences().next().unwrap()),
            *wanted
        );
    }
}

#[test]
fn edited_distinct_video_layers_export_overlap_and_reused_source_without_flattening() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = converted_adobe_sequence(root, TWO_TRACKS_OVERLAP);
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0]["source"]["assetId"], "premiere-video-2");
    assert_eq!(layers[1]["source"]["assetId"], "premiere-video-1");
    // Move/trim the upper blue source while retaining overlap with red below.
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 1000, "duration": 2000}),
        json!({"start": 2000, "duration": 2000}),
    );
    layers[0]["sourceRange"] = json!({"start": 2000, "duration": 2000});
    let mut reused = layers[1].clone();
    reused["id"] = json!(100);
    reused["playback"] = crate::test_support::linear_playback(
        json!({"start": 5000, "duration": 1000}),
        json!({"start": 6000, "duration": 1000}),
    );
    reused["sourceRange"] = json!({"start": 6000, "duration": 1000});
    layers.insert(2, reused);
    layers[3]["activeRange"]["duration"] = json!(6000);
    document["duration"] = json!(6.0);

    let edited = root.join("edited-layers.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("media/clip-a.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "premiere-video-2",
            root.join("media/clip-b.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("edited-native");
    tesseract_to_premiere(&edited, &native, false).unwrap();
    let rebuilt = PrProjectFile::load(native.join("project.prproj"))
        .unwrap()
        .0;
    assert_eq!(
        sequence_table(&rebuilt, rebuilt.sequences().next().unwrap()),
        json!({
            "name": "Two tracks overlap",
            "video_track_count": 2,
            "occurrences": [
                {"track_index": 0, "timeline_ticks": [0, 5 * TICKS], "source_ticks": [0, 5 * TICKS], "media_name": "clip-a.mp4"},
                {"track_index": 0, "timeline_ticks": [5 * TICKS, 6 * TICKS], "source_ticks": [6 * TICKS, 7 * TICKS], "media_name": "clip-a.mp4"},
                {"track_index": 1, "timeline_ticks": [TICKS, 3 * TICKS], "source_ticks": [2 * TICKS, 4 * TICKS], "media_name": "clip-b.mp4"},
            ]
        })
    );
    assert_eq!(fs::read(native.join("media/clip-a.mp4")).unwrap(), CLIP_A);
    assert_eq!(fs::read(native.join("media/clip-b.mp4")).unwrap(), CLIP_B);
}

#[test]
fn shared_premiere_model_loads_the_supported_subset() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = fixture(root, &one_second());
    let project = PrProjectFile::load(&source).unwrap().0;
    let sequence = project.sequences().next().unwrap();
    assert_eq!(sequence.id(), Some("sequence-1"));
    assert_eq!(sequence.name(), "Main");
    assert_eq!(sequence.video_occurrences().count(), 1);

    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(sequence.dimensions(), [1920, 1080]);
    assert_eq!(clip.timeline_ticks(), 0..TICKS);
    assert_eq!(project.media(clip).unwrap().name(), "source.mp4");
}

#[test]
fn premiere_lists_duplicate_names_and_requires_one_target() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = fixture(root, &two_timelines());
    fs::remove_file(root.join("media/source.mp4")).unwrap();

    let targets = Premiere.list_import_targets(&source).unwrap();
    assert_eq!(
        targets
            .iter()
            .map(|target| (target.id.as_str(), target.name.as_str()))
            .collect::<Vec<_>>(),
        [("sequence-1", "Main"), ("z-second-sequence-1", "Main")]
    );
    for target in &targets {
        assert_eq!((target.width, target.height), (Some(1920), Some(1080)));
        assert_eq!(target.fps, Some(30.0));
        assert_eq!(target.duration_secs, Some(1.0));
        assert_eq!(target.layer_count, None);
        assert_eq!(target.video_track_count, Some(1));
        assert_eq!(target.audio_track_count, Some(0));
    }
    let output = root.join("out");
    let error = premiere_to_tesseract(&source, &output, None, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("--sequence") && error.contains("tsrct-conv inspect"));
    assert!(!output.exists());

    fs::write(root.join("media/source.mp4"), MEDIA).unwrap();
    premiere_to_tesseract(&source, &output, Some("z-second-sequence-1"), false).unwrap();
    assert_eq!(project_files(&output), [output.join("project.tsrct")]);
}

#[test]
fn premiere_adapter_preserves_check_publication_and_legacy_reports() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = fixture(root, &one_second());
    let batch = root.join("tesseract_output");
    let options = PremiereImportOptions {
        sequence: Some("sequence-1".to_owned()),
    };
    let checked = Premiere
        .import_to_tesseract(&source, &batch, &options, ConversionMode::Check)
        .unwrap();
    assert_eq!(
        checked.diagnostics,
        premiere_to_tesseract(&source, root.join("legacy_check"), Some("sequence-1"), true)
            .unwrap()
    );
    assert!(!batch.exists());
    assert!(!root.join("legacy_check").exists());

    let written = Premiere
        .import_to_tesseract(&source, &batch, &options, ConversionMode::Write)
        .unwrap();
    assert_eq!(written, checked);
    let output = root.join("premiere_output");
    let document = first_project(&batch);
    let checked = Premiere
        .export_from_tesseract(
            &document,
            &output,
            &Default::default(),
            ConversionMode::Check,
        )
        .unwrap();
    assert!(checked.diagnostics.is_empty());
    assert!(!output.exists());
    let written = Premiere
        .export_from_tesseract(
            &document,
            &output,
            &Default::default(),
            ConversionMode::Write,
        )
        .unwrap();
    assert_eq!(written, checked);
    assert!(output.join("project.prproj").is_file());
}

#[test]
fn quicktime_mov_survives_checks_and_both_conversion_directions_without_transcoding() {
    assert_eq!(&MOV[4..12], b"ftypqt  ");
    for extension in ["mov", "MOV"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let name = format!("source.{extension}");
        fixture(root, &one_second().replace("source.mp4", &name));
        fs::write(root.join("media").join(&name), MOV).unwrap();
        for check in [true, false] {
            let result = premiere_to_tesseract(
                root.join("project.prproj"),
                root.join("tesseract_output"),
                None,
                check,
            );
            result.unwrap();
            if check {
                assert!(!root.join("tesseract_output").exists());
            }
        }
        let source = root.join("tesseract_output/project.tsrct");
        let file = TesseractFile::open(&source).unwrap();
        assert_eq!(
            file.metadata().assets["premiere-video-1"].content_type,
            "video/quicktime"
        );
        for check in [true, false] {
            let result = tesseract_to_premiere(
                root.join(source.to_str().unwrap()),
                root.join("premiere_output"),
                check,
            );
            result.unwrap();
            if check {
                assert!(!root.join("premiere_output").exists());
            }
        }
        assert_eq!(
            fs::read(root.join("premiere_output/media").join(&name)).unwrap(),
            MOV
        );
        premiere_to_tesseract(
            root.join("premiere_output/project.prproj"),
            root.join("rebuilt_tesseract"),
            None,
            false,
        )
        .unwrap();
        let roundtrip = TesseractFile::open(root.join("rebuilt_tesseract/project.tsrct")).unwrap();
        assert_eq!(
            file.project_json().unwrap(),
            roundtrip.project_json().unwrap()
        );
        let mut bytes = Vec::new();
        roundtrip
            .asset("premiere-video-1")
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, MOV);
    }
}

#[test]
fn explicit_guid_selection_and_hostile_names_cannot_escape_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(
        root,
        &two_timelines().replace("<Name>Main</Name>", "<Name>../../日本語 \\ test</Name>"),
    );
    premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("./out"),
        Some("sequence-1"),
        false,
    )
    .unwrap();
    let projects = project_files(&root.join("out"));
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].parent(), Some(root.join("out").as_path()));
    assert!(projects[0].is_file());
    // A name is not a selection key: only the GUID selects.
    let error = premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("wrong"),
        Some("Main"),
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("no sequence matches GUID \"Main\""),
        "{error}"
    );
    assert!(!root.join("wrong").exists());
}

#[test]
fn current_edited_document_builds_premiere_cuts_gaps_and_deleted_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    let base = doc["composition"]["layers"][0].clone();
    let mut layers = Vec::new();
    for index in 0..4 {
        let mut clip = base.clone();
        clip["id"] = json!(index + 10);
        clip["playback"] = crate::test_support::linear_playback(
            json!({"start":index * 1000,"duration":500}),
            json!({"start":if index % 2 == 0 { 0 } else { 500 },"duration":500}),
        );
        clip["sourceRange"] = json!({"start":if index % 2 == 0 { 0 } else { 500 },"duration":500});
        layers.push(clip);
    }
    // Delete a middle occurrence, keeping a gap and three independent source ranges.
    layers.remove(1);
    let mut canvas = doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    canvas["activeRange"]["duration"] = json!(3500);
    layers.push(canvas);
    doc["composition"]["layers"] = json!(layers);
    doc["duration"] = json!(3.5);
    let input = archive(root, &doc, &root.join("source.mp4"));
    let original = fs::read(&input).unwrap();
    tesseract_to_premiere(&input, root.join("native"), false).unwrap();
    let xml = read_xml(&root.join("native/project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        track_item_ticks(&parsed, "Start"),
        [0, 2 * TICKS, 3 * TICKS]
    );
    assert_eq!(
        track_item_ticks(&parsed, "End"),
        [TICKS / 2, 5 * TICKS / 2, 7 * TICKS / 2]
    );
    assert_eq!(fs::read_dir(root.join("native/media")).unwrap().count(), 1);
    assert_eq!(
        parsed
            .descendants()
            .filter(|n| n.has_tag_name("Sequence") && n.attribute("ObjectUID").is_some())
            .count(),
        1
    );
    premiere_to_tesseract(
        root.join("native/project.prproj"),
        root.join("again"),
        None,
        false,
    )
    .unwrap();
    let restored = TesseractFile::open(first_project(&root.join("again")))
        .unwrap()
        .project_json()
        .unwrap();
    let ranges: Vec<_> = restored["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|layer| {
            (
                (*crate::test_support::layer_range(layer))["start"]
                    .as_f64()
                    .unwrap(),
                (*crate::test_support::layer_range(layer))["duration"]
                    .as_f64()
                    .unwrap(),
                layer["sourceRange"]["start"].as_f64().unwrap(),
                layer["sourceRange"]["duration"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ranges,
        [
            (0.0, 500.0, 0.0, 500.0),
            (2000.0, 500.0, 0.0, 500.0),
            (3000.0, 500.0, 500.0, 500.0)
        ]
    );
    assert_eq!(fs::read(&input).unwrap(), original);
    assert!(tesseract_to_premiere(&input, root.join("native"), false)
        .unwrap_err()
        .to_string()
        .contains("already exists"));
}

#[test]
fn repeated_media_survives_premiere_tesseract_roundtrip_with_one_packaged_asset() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let native = fixture(root, &one_second());
    let input = root.join("first.tsrct");
    build_tesseract_file(&native, &input, None).unwrap();
    let mut document = TesseractFile::open(&input).unwrap().project_json().unwrap();
    let mut second = document["composition"]["layers"][0].clone();
    second["id"] = json!(3);
    second["playback"]["inputRange"]["start"] = json!(1000);
    second["playback"]["mapping"]["input"]["start"] = json!(1000);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, second);
    document["composition"]["layers"][2]["activeRange"]["duration"] = json!(2000);
    document["duration"] = json!(2);
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("media/source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(root.join("repeat.tsrct"))
        .unwrap();
    tesseract_to_premiere(root.join("repeat.tsrct"), root.join("first-native"), false).unwrap();
    premiere_to_tesseract(
        root.join("first-native/project.prproj"),
        root.join("again"),
        None,
        false,
    )
    .unwrap();
    let input = first_project(&root.join("again"));
    let file = TesseractFile::open(&input).unwrap();
    assert_eq!(file.metadata().assets.len(), 1);
    let doc = file.project_json().unwrap();
    assert_eq!(
        doc["composition"]["layers"][0]["source"]["assetId"],
        doc["composition"]["layers"][1]["source"]["assetId"]
    );
    tesseract_to_premiere(&input, root.join("second-native"), true).unwrap();
    tesseract_to_premiere(&input, root.join("second-native"), false).unwrap();
    let first_xml = read_xml(&root.join("first-native/project.prproj"));
    let second_xml = read_xml(&root.join("second-native/project.prproj"));
    let first_doc = roxmltree::Document::parse(&first_xml).unwrap();
    let second_doc = roxmltree::Document::parse(&second_xml).unwrap();
    assert_eq!(
        track_item_ticks(&first_doc, "Start"),
        track_item_ticks(&second_doc, "Start")
    );
    assert_eq!(
        fs::read(root.join("first-native/media/source.mp4")).unwrap(),
        fs::read(root.join("second-native/media/source.mp4")).unwrap()
    );
}

#[test]
fn distinct_original_media_with_colliding_names_receive_unique_package_paths() {
    check_media_name_collisions("mp4", MEDIA, ["source", "source"]);
    check_media_name_collisions("mov", MOV, ["source", "source"]);
    check_media_name_collisions("mp4", MEDIA, ["é", "e\u{301}"]);
}

#[test]
fn distinct_same_byte_files_keep_independent_identity_after_package_relocation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = fixture(root, &one_second());
    let input = root.join("source.tsrct");
    build_tesseract_file(&source, &input, None).unwrap();
    let mut doc = TesseractFile::open(&input).unwrap().project_json().unwrap();
    let mut second = doc["composition"]["layers"][0].clone();
    second["id"] = json!(3);
    second["source"]["assetId"] = json!("separate-source");
    second["playback"]["inputRange"]["start"] = json!(1000);
    second["playback"]["mapping"]["input"]["start"] = json!(1000);
    doc["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, second);
    doc["composition"]["layers"][2]["activeRange"]["duration"] = json!(2000);
    doc["duration"] = json!(2);
    let other = root.join("media/separate.mp4");
    fs::write(&other, MEDIA).unwrap();
    let distinct = root.join("distinct.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&doc).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("media/source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("separate-source", &other, AssetKind::Video)
        .unwrap()
        .write(&distinct)
        .unwrap();
    let native = root.join("native");
    tesseract_to_premiere(&distinct, &native, false).unwrap();
    let moved = root.join("relocated");
    fs::rename(&native, &moved).unwrap();
    premiere_to_tesseract(
        root.join("relocated/project.prproj"),
        root.join("again"),
        None,
        true,
    )
    .unwrap();
    assert!(!root.join("again").exists());
    let tesseract_output = root.join("again.tsrct");
    build_tesseract_file(&moved.join("project.prproj"), &tesseract_output, None).unwrap();
    let saved = TesseractFile::open(&tesseract_output).unwrap();
    let saved_doc = saved.project_json().unwrap();
    assert_ne!(
        saved_doc["composition"]["layers"][0]["source"]["assetId"],
        saved_doc["composition"]["layers"][1]["source"]["assetId"]
    );
    tesseract_to_premiere(&tesseract_output, root.join("second-native"), false).unwrap();
    let media: Vec<_> = fs::read_dir(root.join("second-native/media"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(media.len(), 2);
    assert_ne!(media[0], media[1]);
}

#[test]
fn explicit_selection_handles_uncertain_nesting() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let xml = two_timelines().replace(
        "<SubClip ObjectID=\"25\"><Clip ObjectRef=\"26\"/>",
        "<SubClip ObjectID=\"25\">",
    );
    assert_ne!(xml, two_timelines());
    fixture(root, &xml);
    // Uncertain nesting does not change the rule: two selectable sequences
    // need an explicit selection, and the unrelated broken link does not
    // block the selected one.
    let error = premiere_to_tesseract(root.join("project.prproj"), root.join("all"), None, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("project has 2 selectable sequences; select one with --sequence <GUID>"),
        "{error}"
    );
    assert!(!root.join("all").exists());
    for check in [true, false] {
        premiere_to_tesseract(
            root.join("project.prproj"),
            root.join("selected"),
            Some("sequence-1"),
            check,
        )
        .unwrap();
        if check {
            assert!(!root.join("selected").exists());
        } else {
            assert_eq!(project_files(&root.join("selected")).len(), 1);
        }
    }
    premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("invalid"),
        Some("z-second-sequence-1"),
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(!root.join("invalid").exists());
}

#[test]
fn explicit_selection_converts_a_nested_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let xml = one_second().replace("</PremiereData>", r#"
    <Sequence ObjectUID="parent"><Name>Parent</Name><TrackGroups><TrackGroup><Second ObjectRef="100"/></TrackGroup></TrackGroups></Sequence>
    <VideoTrackGroup ObjectID="100"><TrackGroup><Tracks><Track ObjectRef="101"/></Tracks></TrackGroup></VideoTrackGroup>
    <VideoClipTrack ObjectID="101"><ClipTrack><ClipItems><TrackItems><TrackItem ObjectRef="102"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>
    <VideoClipTrackItem ObjectID="102"><ClipTrackItem><SubClip ObjectRef="103"/></ClipTrackItem></VideoClipTrackItem>
    <SubClip ObjectID="103"><Clip ObjectRef="104"/></SubClip>
    <VideoClip ObjectID="104"><Clip><Source ObjectRef="105"/></Clip></VideoClip>
    <VideoSequenceSource ObjectID="105"><SequenceSource><Sequence ObjectURef="sequence-1"/></SequenceSource></VideoSequenceSource>
    </PremiereData>"#);
    fixture(root, &xml);
    premiere_to_tesseract(
        root.join("project.prproj"),
        root.join("child"),
        Some("sequence-1"),
        false,
    )
    .unwrap();
    let projects = project_files(&root.join("child"));
    assert_eq!(projects.len(), 1);
    let file = TesseractFile::open(&projects[0]).unwrap();
    assert_eq!(file.project().composition().name(), "Main");
}

fn check_media_name_collisions(extension: &str, bytes: &[u8], names: [&str; 2]) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let native = fixture(root, &one_second());
    let input = root.join("source.tsrct");
    build_tesseract_file(&native, &input, None).unwrap();
    let mut doc = TesseractFile::open(&input).unwrap().project_json().unwrap();
    let mut second = doc["composition"]["layers"][0].clone();
    second["id"] = json!(3);
    second["playback"]["inputRange"]["start"] = json!(1000);
    second["playback"]["mapping"]["input"]["start"] = json!(1000);
    second["source"]["assetId"] = json!("different-media");
    doc["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, second);
    doc["composition"]["layers"][2]["activeRange"]["duration"] = json!(2000);
    doc["duration"] = json!(2);
    fs::create_dir(root.join("other")).unwrap();
    let first = root.join(format!("media/{}.{extension}", names[0]));
    fs::write(&first, bytes).unwrap();
    let other = root.join(format!("other/{}.{extension}", names[1]));
    fs::write(&other, [bytes, b"\0\0\0\x0cfreeabcd"].concat()).unwrap();
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&doc).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &first, AssetKind::Video)
        .unwrap()
        .add_asset("different-media", &other, AssetKind::Video)
        .unwrap()
        .write(root.join("collision.tsrct"))
        .unwrap();
    tesseract_to_premiere(root.join("collision.tsrct"), root.join("native"), true).unwrap();
    tesseract_to_premiere(root.join("collision.tsrct"), root.join("native"), false).unwrap();
    let mut media: Vec<_> = fs::read_dir(root.join("native/media"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    media.sort();
    assert_eq!(media.len(), 2);
    assert_ne!(
        media[0].to_string_lossy().to_lowercase(),
        media[1].to_string_lossy().to_lowercase()
    );
    assert!(media
        .iter()
        .all(|path| path.extension().unwrap() == extension));
    let payloads: Vec<_> = media.iter().map(|path| fs::read(path).unwrap()).collect();
    assert!(payloads.contains(&bytes.to_vec()));
    assert!(payloads.contains(&fs::read(&other).unwrap()));
    premiere_to_tesseract(
        root.join("native/project.prproj"),
        root.join("again"),
        None,
        false,
    )
    .unwrap();
}

#[test]
fn adobe_native_point_text_stays_editable_without_media() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_text_point.prproj");
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        source,
        &output,
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
        false,
    )
    .unwrap();
    // The document packages no font file, so import reports the font once.
    let unpackaged = "font \"Arial-BoldMT\" is not packaged in this document; import it with \
                      tsrct project import-font before preview or export.";
    assert_eq!(
        omissions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [format!(
            "feature VideoClipTrackItem:10113 (\"py\"): {unpackaged}"
        )]
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert!(file.metadata().assets.is_empty());
    let mut document = file.project_json().unwrap();
    assert_eq!(document["duration"], json!(2.0));
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1]["type"], "Rect");
    let text = &mut layers[0];
    assert_eq!(text["type"], "Text");
    assert_eq!(
        (*crate::test_support::layer_range(text)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(text["sourceText"]["text"], "py");
    assert_eq!(text["sourceText"]["fontFamily"], "Arial-BoldMT");
    assert_eq!(text["sourceText"]["fontStyle"], "");
    assert_eq!(text["sourceText"]["fontSize"], 160.0);
    assert_eq!(text["sourceText"]["fillColor"], json!([1.0, 1.0, 1.0, 1.0]));
    assert_eq!(text["sourceText"]["boxText"], false);
    assert_eq!(text["transform"]["position"], json!([480.0, 540.0]));
    assert_eq!(text["transform"]["anchorPoint"], json!([0.0, 0.0]));

    // Reverse conversion must read the current editable text, not replay source XML.
    text["sourceText"]["text"] = json!("Edited");
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("native");
    tesseract_to_premiere(&edited, &native, false).unwrap();
    // A text-only document writes no media files.
    assert!(
        !native.join("media").exists() || fs::read_dir(native.join("media")).unwrap().count() == 0
    );
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.sequences().next().unwrap();
    let [PrVideoItem::Graphic(graphic)] = sequence.video_tracks().next().unwrap() else {
        panic!("expected one editable graphic");
    };
    assert_eq!(graphic.timeline_ticks(), 0..2 * TICKS);
    // Reopening the written project must read the edited Source Text.
    let again = root.join("again");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(|omission| omission.reason.as_str())
            .collect::<Vec<_>>(),
        [unpackaged]
    );
    let reopened = TesseractFile::open(first_project(&again))
        .unwrap()
        .project_json()
        .unwrap();
    let source_text = &reopened["composition"]["layers"][0]["sourceText"];
    assert_eq!(source_text["text"], "Edited");
    // Premiere -> .tsrct -> Premiere keeps the PostScript name byte for byte.
    assert_eq!(
        source_text["fontFamily"].as_str().map(str::as_bytes),
        Some(&b"Arial-BoldMT"[..])
    );
    assert_eq!(source_text["fontStyle"], "");
}

#[test]
fn single_style_text_survives_premiere_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut doc = document(root);
    doc["composition"]["layers"].as_array_mut().unwrap().insert(
        0,
        json!({
            "type": "Text",
            "id": 9,
            "name": "Title",
            "activeRange": {"start": 0, "duration": 1000},
            "transform": {"anchorPoint": [10, 20], "position": [960, 540], "scale": [80, 80], "rotation": -15, "opacity": 75},
            "sourceText": {
                "text": "Wraps inside\na box",
                "fontFamily": "Inter",
                "fontStyle": "Bold",
                "fontSize": 72,
                "fillColor": [1, 0, 0, 1],
                "applyStroke": true,
                "strokeColor": [0, 0, 1, 1],
                "strokeWidth": 8,
                "justification": "right",
                "tracking": 25,
                "leading": 100,
                "boxText": true,
                "boxSize": [700, 300],
                "boxPosition": [0, 0],
                "verticalAlign": "bottom"
            }
        }),
    );
    let normalized = fx_schema::EditableFxCompositionDocument::from_json_value(doc.clone())
        .unwrap()
        .to_json_value()
        .unwrap();
    // The document packages Inter Bold; its registry face (`metadata.json`
    // `fonts`) names the PostScript name. The converter never reads font bytes.
    let font = root.join("Inter-Bold.ttf");
    fs::write(&font, b"packaged font bytes").unwrap();
    let registry = serde_json::from_value(json!({
        "faces": [{
            "postscriptName": "Inter-Bold",
            "fullName": "Inter Bold",
            "familyName": "Inter",
            "styleName": "Bold",
            "weight": 700,
            "width": 5,
            "selectionNames": ["Inter/Bold"]
        }]
    }))
    .unwrap();
    let input = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&doc).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            root.join("source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_font_asset("inter-bold", &font, registry)
        .unwrap()
        .write(&input)
        .unwrap();
    tesseract_to_premiere(&input, root.join("native"), false).unwrap();
    let xml = read_xml(&root.join("native/project.prproj"));
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Text</MatchName>").count(),
        1
    );
    premiere_to_tesseract(
        root.join("native/project.prproj"),
        root.join("again"),
        None,
        false,
    )
    .unwrap();
    let restored = TesseractFile::open(first_project(&root.join("again")))
        .unwrap()
        .project_json()
        .unwrap();
    let text = |document: &Value| {
        document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Text")
            .unwrap()
            .clone()
    };
    let (mut expected, actual) = (text(&normalized), text(&restored));
    // Export names the packaged face by its PostScript name, which reimport stores.
    expected["sourceText"]["fontFamily"] = json!("Inter-Bold");
    expected["sourceText"]["fontStyle"] = json!("");
    // The native reimport spells out default transform/text fields and uses f64
    // numbers; the authored document omits those defaults and uses integers.
    let expected_transform: fx_schema::Transform =
        serde_json::from_value(expected["transform"].clone()).unwrap();
    let actual_transform: fx_schema::Transform =
        serde_json::from_value(actual["transform"].clone()).unwrap();
    assert_eq!(actual_transform, expected_transform, "transform");
    let expected_text: fx_schema::TextDocument =
        serde_json::from_value(expected["sourceText"].clone()).unwrap();
    let actual_text: fx_schema::TextDocument =
        serde_json::from_value(actual["sourceText"].clone()).unwrap();
    assert_eq!(actual_text, expected_text, "sourceText");
    for field in ["name", "activeRange"] {
        assert_eq!(actual[field], expected[field], "{field}");
    }
}
