//! Subclip placements through the public conversion API.
use super::support::*;
use premiere_file::PrProjectFile;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    ops::Range,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";

// Derived from the Adobe-authored `feature_repeated_adjacent_source_strict.prproj`.
// The subclip is inferred as a second master clip on the same Media whose clip
// record carries In/Out 2-8 s; no corpus project contains a Make Subclip item,
// so this shape is NOT Adobe-verified. V1 places it trimmed to 3-6 s at 0-3 s,
// whole (2-8 s) at 4-10 s, and past its Out (8-9.5 s) at 10-11.5 s. Placements
// store absolute media In/Out and share the placed master's Markers, as Adobe
// saves placements of marked master clips.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_subclip_strict.prproj")
}

type Rows = Vec<(Range<i64>, Range<i64>)>;

/// `(timeline, source)` tick ranges in timeline order, and the number of media.
fn occurrence_rows(project: &PrProjectFile) -> (Rows, usize) {
    assert_eq!(project.sequences().len(), 1);
    let sequence = project.sequences().next().unwrap();
    let mut rows: Rows = sequence
        .video_occurrences()
        .map(|clip| (clip.timeline_ticks(), clip.source_ticks()))
        .collect();
    rows.sort_by_key(|(timeline, _)| timeline.start);
    let media: BTreeSet<_> = sequence
        .video_occurrences()
        .map(|clip| clip.media_id())
        .collect();
    (rows, media.len())
}

fn fixture_rows() -> Rows {
    vec![
        (0..3 * TICKS, 3 * TICKS..6 * TICKS),
        (4 * TICKS..10 * TICKS, 2 * TICKS..8 * TICKS),
        (
            10 * TICKS..11 * TICKS + TICKS / 2,
            8 * TICKS..9 * TICKS + TICKS / 2,
        ),
    ]
}

/// Video layers in timeline order.
fn video_layers(document: &Value) -> Vec<&Value> {
    let mut layers: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    layers.sort_by_key(|layer| {
        (*crate::test_support::layer_range(layer))["start"]
            .as_i64()
            .unwrap()
    });
    layers
}

#[test]
fn marked_master_bounds_do_not_change_playback_or_wipe_conversion() {
    // Synthetic master marks on pinned feature fixtures: structural interaction
    // proof only, not an Adobe-authored retimed/wiped subclip claim.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (name, sequence, masters) in [
        (
            "feature_constant_reverse_0_905_strict.prproj",
            SEQUENCE,
            &["22"][..],
        ),
        (
            "feature_frame_blending_half_speed_strict.prproj",
            SEQUENCE,
            &["22"][..],
        ),
        (
            "feature_linear_wipe_strict.prproj",
            "49b892d1-dfc3-4be2-a83a-93789626be7c",
            &["22", "28"][..],
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        for media in [
            "feature_timecoded_source.mp4",
            "feature_two_tracks_gap_clip_a.mp4",
            "feature_two_tracks_gap_clip_b.mp4",
        ] {
            std::fs::copy(fixtures.join(media), dir.path().join(media)).unwrap();
        }
        let plain = read_xml(&fixtures.join(name));
        let parsed = roxmltree::Document::parse(&plain).unwrap();
        let mut marked = plain.clone();
        for id in masters.iter().rev() {
            let record = parsed
                .root_element()
                .children()
                .find(|node| {
                    node.has_tag_name("VideoClip") && node.attribute("ObjectID") == Some(id)
                })
                .unwrap();
            let clip = record
                .children()
                .find(|node| node.has_tag_name("Clip"))
                .unwrap();
            assert!(!clip
                .children()
                .any(|node| matches!(node.tag_name().name(), "InPoint" | "OutPoint")));
            let end = clip.range().end - "</Clip>".len();
            marked.insert_str(
                end,
                &format!(
                    "<InPoint>{}</InPoint><OutPoint>{}</OutPoint>",
                    TICKS,
                    2 * TICKS
                ),
            );
        }
        let mut outputs = Vec::new();
        for (label, xml) in [("plain", plain), ("marked", marked)] {
            let input = dir.path().join(format!("{label}.prproj"));
            write_prproj(&input, &xml);
            let output = dir.path().join(label);
            let import_omissions =
                premiere_to_tesseract(&input, &output, Some(sequence), false).unwrap();
            let archive = first_project(&output);
            let document = TesseractFile::open(&archive)
                .unwrap()
                .project_json()
                .unwrap();
            let exported = dir.path().join(format!("{label}-back"));
            let export_omissions = tesseract_to_premiere(&archive, &exported, false).unwrap();
            let exported = exported.join("project.prproj");
            let native = read_xml(&exported);
            let parsed = roxmltree::Document::parse(&native).unwrap();
            let master_ids: BTreeSet<_> = parsed
                .descendants()
                .filter(|node| {
                    node.has_tag_name("Clip")
                        && node.parent().is_some_and(|clips| {
                            clips.has_tag_name("Clips")
                                && clips
                                    .parent()
                                    .is_some_and(|master| master.has_tag_name("MasterClip"))
                        })
                })
                .filter_map(|node| node.attribute("ObjectRef"))
                .collect();
            assert!(!master_ids.is_empty());
            for master in parsed.root_element().children().filter(|node| {
                node.attribute("ObjectID")
                    .is_some_and(|id| master_ids.contains(id))
            }) {
                assert!(!master
                    .descendants()
                    .any(|node| matches!(node.tag_name().name(), "InPoint" | "OutPoint")));
            }
            let records: Vec<_> = parsed
                .descendants()
                .filter(|node| {
                    matches!(
                        node.tag_name().name(),
                        "PlaybackSpeed"
                            | "PlayBackwards"
                            | "TimeRemapping"
                            | "TimeInterpolationType"
                            | "InPoint"
                            | "OutPoint"
                            | "MatchName"
                            | "Name"
                            | "StartKeyframe"
                            | "Keyframes"
                    )
                })
                .map(|node| {
                    (
                        node.tag_name().name().to_owned(),
                        node.text().unwrap_or_default().to_owned(),
                    )
                })
                .collect();
            let rows = occurrence_rows(&PrProjectFile::load(&exported).unwrap().0);
            outputs.push((document, records, rows, import_omissions, export_omissions));
        }
        assert_eq!(
            outputs[0], outputs[1],
            "{name}: marked bounds changed conversion"
        );
    }
}

#[test]
fn subclip_placements_keep_absolute_source_ranges_through_an_edited_export() {
    let native = PrProjectFile::load(fixture()).unwrap().0;
    assert_eq!(occurrence_rows(&native), (fixture_rows(), 1));

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(fixture(), &output, Some(SEQUENCE), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert_eq!(file.metadata().assets.len(), 1);
    let mut document = file.project_json().unwrap();
    let layers: Vec<_> = video_layers(&document)
        .into_iter()
        .map(|layer| {
            (
                (*crate::test_support::layer_range(layer)).clone(),
                layer["sourceRange"].clone(),
                layer["source"]["assetId"].clone(),
                // The whole media stays available as trim handles around the subclip.
                layer["sourceIntrinsicDuration"].clone(),
            )
        })
        .collect();
    assert_eq!(
        layers,
        [
            (
                json!({"start": 0, "duration": 3000}),
                json!({"start": 3000, "duration": 3000}),
                json!("premiere-video-1"),
                json!(10000),
            ),
            (
                json!({"start": 4000, "duration": 6000}),
                json!({"start": 2000, "duration": 6000}),
                json!("premiere-video-1"),
                json!(10000),
            ),
            // Wholly past the subclip Out (8 s), inside the media.
            (
                json!({"start": 10000, "duration": 1500}),
                json!({"start": 8000, "duration": 1500}),
                json!("premiere-video-1"),
                json!(10000),
            ),
        ]
    );

    for layer in document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|layer| layer["type"] == "Video")
    {
        match (*crate::test_support::layer_range(layer))["start"].as_i64() {
            // Slip 1 s before the subclip In (2 s), into the media's head handle.
            Some(0) => {
                layer["sourceRange"] = json!({"start": 1000, "duration": 3000});
                layer["playback"]["mapping"]["output"]["start"] = json!(1000);
            }
            // Slip 1 s past the subclip Out (8 s), into the media's tail handle.
            Some(4000) => {
                layer["sourceRange"] = json!({"start": 3000, "duration": 6000});
                layer["playback"]["mapping"]["output"]["start"] = json!(3000);
            }
            _ => {}
        }
    }
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture().with_file_name("feature_timecoded_source.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("edited-native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let rebuilt = PrProjectFile::load(native.join("project.prproj"))
        .unwrap()
        .0;
    let slipped = vec![
        (0..3 * TICKS, TICKS..4 * TICKS),
        (4 * TICKS..10 * TICKS, 3 * TICKS..9 * TICKS),
        fixture_rows()[2].clone(),
    ];
    assert_eq!(occurrence_rows(&rebuilt), (slipped, 1));

    let reimported = root.join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let sources: Vec<_> = video_layers(&document)
        .into_iter()
        .map(|layer| layer["sourceRange"].clone())
        .collect();
    assert_eq!(
        sources,
        [
            json!({"start": 1000, "duration": 3000}),
            json!({"start": 3000, "duration": 6000}),
            json!({"start": 8000, "duration": 1500}),
        ]
    );
}
