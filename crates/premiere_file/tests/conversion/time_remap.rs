#![cfg(feature = "ffmpeg-library")]

//! Time Remapping at another speed and with trimmed In/Out, from the unchanged
//! Premiere 26.5.1 save `feature_time_remap_trimmed_speed_26_5_strict.prproj`.
//!
//! Sequence `9a10a3b7-a83b-47d9-a68c-91d06d937738` (1920 x 1080, 30 fps)
//! places `feature_timecoded_source.mp4` twice on V1, both at PlaybackSpeed
//! 0.8 over the same nine-key Speed curve: `VideoClipTrackItem:63` at 0-2 s
//! with In 0.4 s and Out 2 s, and `VideoClipTrackItem:64` at 2-4 s with In 0
//! and Out 1.6 s. One AME render of the sequence showed the source frames of
//! [`NATIVE_FRAMES`] in the media's burned-in timestamps; they match curve
//! input `In + speed × elapsed`, not a scaled In, a dropped speed, a scaled
//! output or In added after the curve. The render covers the curve interior
//! only, not its appended media-end key.

use super::support::*;
use serde_json::Value;
use std::{fs, path::Path};
use tesseract_file::TesseractFile;

const FIXTURE: &str = "feature_time_remap_trimmed_speed_26_5_strict.prproj";
const SEQUENCE: &str = "9a10a3b7-a83b-47d9-a68c-91d06d937738";

/// Timeline frame and the 30 fps source frame that the native render shows there.
const NATIVE_FRAMES: [(u32, u32); 8] = [
    (0, 19),
    (15, 30),
    (30, 34),
    (59, 41),
    (85, 28),
    (100, 33),
    (110, 35),
    (119, 37),
];

/// Progress of an FX cubic Bezier at linear progress `x`, solving its x by bisection.
fn cubic_bezier_progress(easing: &Value, x: f64) -> f64 {
    let handle = |name: &str| easing[name].as_f64().unwrap();
    let curve = |first: f64, second: f64, t: f64| {
        3.0 * (1.0 - t).powi(2) * t * first + 3.0 * (1.0 - t) * t * t * second + t.powi(3)
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..100 {
        let middle = (low + high) / 2.0;
        if curve(handle("x1"), handle("x2"), middle) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    curve(handle("y1"), handle("y2"), (low + high) / 2.0)
}

/// The 30 fps source frame that a time-remapped FX playback shows at parent
/// time `parent_ms`, sampled as `fx_model` samples it: moved by
/// `inputOffsetMs`, then between the keys around it with the later key's easing.
fn source_frame(playback: &Value, parent_ms: f64) -> u32 {
    let input = parent_ms + playback["inputOffsetMs"].as_f64().unwrap();
    let keys = playback["mapping"]["property"]["keyframes"]
        .as_array()
        .unwrap();
    let ms = |key: &Value, field: &str| key[field].as_f64().unwrap();
    let next = keys.partition_point(|key| ms(key, "time") <= input);
    assert!(
        (1..keys.len()).contains(&next),
        "{input} ms is outside the keys"
    );
    let (from, to) = (&keys[next - 1], &keys[next]);
    let linear = (input - ms(from, "time")) / (ms(to, "time") - ms(from, "time"));
    let progress = match to["easing"]["type"].as_str().unwrap() {
        "linear" => linear,
        "cubicBezier" => cubic_bezier_progress(&to["easing"], linear),
        other => panic!("unexpected easing {other}"),
    };
    let source_ms = ms(from, "value") + (ms(to, "value") - ms(from, "value")) * progress;
    (source_ms * 30.0 / 1000.0).floor() as u32
}

#[test]
fn native_trimmed_speed_time_remap_keeps_both_placements_on_the_native_clock() {
    use sha2::{Digest, Sha256};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (name, digest) in [
        (
            FIXTURE,
            "b370546ee11c8cf81395a0fb9acbe7bb0c469aae8733a9f85de96fa6729cd58a",
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
    let omissions =
        premiere_to_tesseract(fixtures.join(FIXTURE), &output, Some(SEQUENCE), false).unwrap();
    // Only the track items' UI nodes are reported; both placements convert.
    assert!(
        omissions
            .iter()
            .all(|omission| omission.reason.contains("ClipTrackItem/TrackItem/Node")),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 2);
    let placement = |start: u64| {
        *layers
            .iter()
            .find(|layer| layer["playback"]["inputRange"]["start"] == start)
            .unwrap()
    };
    let (in_trimmed, out_trimmed) = (placement(0), placement(2000));

    // Each key keeps its native source time and easing at the parent time
    // where the clip reaches its curve input: (input - In) / 0.8 after the
    // placement start. The In-trimmed clip's first four keys precede its
    // start, so its keys and playback input move 500 ms later together.
    let curve_keys: [u64; 9] = [0, 174, 254, 547, 776, 900, 1040, 1393, 10000];
    let placements: [(&Value, u64, i64, [u64; 9]); 2] = [
        (
            in_trimmed,
            0,
            500,
            [0, 217, 277, 434, 605, 761, 1029, 2500, 13259],
        ),
        (
            out_trimmed,
            2000,
            0,
            [2000, 2217, 2277, 2434, 2605, 2761, 3029, 4500, 15259],
        ),
    ];
    for (layer, start, offset, times) in placements {
        assert_eq!(
            layer["sourceRange"],
            serde_json::json!({"start": 0, "duration": 10000})
        );
        assert_eq!(layer["sourceIntrinsicDuration"], 10000);
        let playback = &layer["playback"];
        assert_eq!(
            playback["inputRange"],
            serde_json::json!({"start": start, "duration": 2000})
        );
        assert_eq!(playback["inputOffsetMs"], offset);
        assert_eq!(playback["mapping"]["type"], "timeRemap");
        let property = &playback["mapping"]["property"];
        assert_eq!(property["before"], "continue");
        assert_eq!(property["after"], "continue");
        let keys = property["keyframes"].as_array().unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| (
                    key["time"].as_u64().unwrap(),
                    key["value"].as_u64().unwrap()
                ))
                .collect::<Vec<_>>(),
            times.into_iter().zip(curve_keys).collect::<Vec<_>>()
        );
        // The native ramps (mode 7 to 8) end at keys 2, 4 and 6.
        for (index, key) in keys.iter().enumerate() {
            let easing = &key["easing"];
            match index {
                2 | 4 | 6 => {
                    assert_eq!(easing["type"], "cubicBezier", "{index}");
                    assert!((easing["x1"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-12);
                    assert!((easing["x2"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-12);
                }
                _ => assert_eq!(easing["type"], "linear", "{index}"),
            }
        }
    }
    // The bounded easing that the reader derives from each native ramp and
    // its neighboring slopes, the same on both placements.
    for (index, (y1, y2)) in [
        (2, (0.199_600_798_4, 0.532_934_131_8)),
        (4, (0.467_065_868_3, 0.800_399_201_6)),
        (6, (0.512_820_512_8, 0.846_153_846_2)),
    ] {
        for layer in [in_trimmed, out_trimmed] {
            let easing = &layer["playback"]["mapping"]["property"]["keyframes"][index]["easing"];
            assert!(
                (easing["y1"].as_f64().unwrap() - y1).abs() < 1e-9,
                "{index}"
            );
            assert!(
                (easing["y2"].as_f64().unwrap() - y2).abs() < 1e-9,
                "{index}"
            );
        }
    }

    // The editable clocks select the source frames of the native render.
    for (timeline_frame, native_frame) in NATIVE_FRAMES {
        let parent_ms = f64::from(timeline_frame) * 1000.0 / 30.0;
        let [layer] = layers
            .iter()
            .filter(|layer| {
                let window = &layer["playback"]["inputRange"];
                let start = window["start"].as_f64().unwrap();
                (start..start + window["duration"].as_f64().unwrap()).contains(&parent_ms)
            })
            .collect::<Vec<_>>()[..]
        else {
            panic!("frame {timeline_frame} is not in exactly one placement");
        };
        assert_eq!(
            source_frame(&layer["playback"], parent_ms),
            native_frame,
            "timeline frame {timeline_frame}"
        );
    }
}

#[test]
fn native_retimed_rotation_keeps_keys_on_the_media_clock() {
    use sha2::{Digest, Sha256};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = fixtures.join("feature_time_remap_rotation_26_5_strict.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        "d62c617652a42fb2c81968e9a78efccb102214f251155e0c8f5817136703429c"
    );
    let dir = tempfile::tempdir().unwrap();
    // Relocate only media paths; keep the pinned source and timing records intact.
    let xml = read_xml(&source);
    let native = roxmltree::Document::parse(&xml).unwrap();
    let mut relocated = xml.clone();
    for node in native
        .descendants()
        .filter(|node| node.has_tag_name("FilePath") || node.has_tag_name("ActualMediaFilePath"))
    {
        relocated = relocated.replace(&xml[node.range()], "");
    }
    relocated = relocated.replace("../inputs/dependency-0.mp4", "dependency-0.mp4");
    fs::copy(
        fixtures.join("feature_timecoded_source.mp4"),
        dir.path().join("dependency-0.mp4"),
    )
    .unwrap();
    let input = dir.path().join("project.prproj");
    write_prproj(&input, &relocated);
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(input, &output, Some(SEQUENCE), false).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.reason.contains("ClipTrackItem/TrackItem/Node")),
        "{omissions:?}"
    );
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 2);
    let keyed = layers
        .iter()
        .find(|layer| layer["playback"]["inputRange"]["start"] == 0)
        .unwrap();
    let control = layers
        .iter()
        .find(|layer| layer["playback"]["inputRange"]["start"] == 2000)
        .unwrap();
    for layer in [keyed, control] {
        assert_eq!(layer["playback"]["inputRange"]["duration"], 2000);
        assert_eq!(
            layer["sourceRange"],
            serde_json::json!({"start": 0, "duration": 10000})
        );
        assert_eq!(
            layer["playback"]["mapping"]["property"]["keyframes"]
                .as_array()
                .unwrap()
                .len(),
            9
        );
    }
    assert_eq!(keyed["playback"]["inputOffsetMs"], 500);
    assert_eq!(control["playback"]["inputOffsetMs"], 0);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let rotation = &entries[0];
    assert_eq!(rotation["target"]["propertyType"], "rotation");
    assert_eq!(rotation["target"]["layerId"], keyed["id"]);
    assert_eq!(rotation["animator"]["type"], "keyframes");
    let keys = rotation["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
                key["easing"]["type"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    // Explicit video playback evaluates property keys in media time. The
    // native trim and speed belong only to playback, not these key times.
    assert_eq!(keys, [(0, 0.0, "linear"), (2000, 90.0, "linear")]);
    assert_eq!(control["transform"]["rotation"], 0.0);
}

#[test]
fn native_trimmed_speed_time_remap_edits_recover_picture_selection_and_sibling() {
    use premiere_file::{OmissionKind, OmissionScope};
    use serde_json::json;
    use sha2::{Digest, Sha256};

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = fixtures.join(FIXTURE);
    let media = fs::read(fixtures.join("feature_timecoded_source.mp4")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        "b370546ee11c8cf81395a0fb9acbe7bb0c469aae8733a9f85de96fa6729cd58a"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&media)),
        "4256ae026cb923ee0498374a1def4dd8c0e51078099a9f415726935e198ac0fe"
    );
    let native = read_xml(&source);
    let control_dir = tempfile::tempdir().unwrap();
    let control_output = control_dir.path().join("converted");
    premiere_to_tesseract(&source, &control_output, Some(SEQUENCE), false).unwrap();
    let control = TesseractFile::open(first_project(&control_output))
        .unwrap()
        .project_json()
        .unwrap();
    let controls = video_layers(&control);
    assert_eq!(controls.len(), 2);
    // Pin the healthy sibling's actual key clock and easing, then require its
    // complete playback to survive each source-derived edit unchanged.
    for (start, offset, times) in [
        (0, 500, [0, 217, 277, 434, 605, 761, 1029, 2500, 13259]),
        (
            2000,
            0,
            [2000, 2217, 2277, 2434, 2605, 2761, 3029, 4500, 15259],
        ),
    ] {
        let layer = controls
            .iter()
            .find(|layer| layer["playback"]["inputRange"]["start"] == start)
            .unwrap();
        let playback = &layer["playback"];
        assert_eq!(
            playback["inputRange"],
            json!({"start": start, "duration": 2000})
        );
        assert_eq!(playback["inputOffsetMs"], offset);
        assert_eq!(playback["mapping"]["type"], "timeRemap");
        let property = &playback["mapping"]["property"];
        assert_eq!(property["before"], "continue");
        assert_eq!(property["after"], "continue");
        let keys = property["keyframes"].as_array().unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| (
                    key["time"].as_u64().unwrap(),
                    key["value"].as_u64().unwrap()
                ))
                .collect::<Vec<_>>(),
            times
                .into_iter()
                .zip([0, 174, 254, 547, 776, 900, 1040, 1393, 10000])
                .collect::<Vec<_>>()
        );
        for (index, key) in keys.iter().enumerate() {
            let expected = match index {
                2 => Some((0.199_600_798_4, 0.532_934_131_8)),
                4 => Some((0.467_065_868_3, 0.800_399_201_6)),
                6 => Some((0.512_820_512_8, 0.846_153_846_2)),
                _ => None,
            };
            let easing = &key["easing"];
            if let Some((y1, y2)) = expected {
                assert_eq!(easing["type"], "cubicBezier");
                assert!((easing["x1"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-12);
                assert!((easing["x2"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-12);
                assert!((easing["y1"].as_f64().unwrap() - y1).abs() < 1e-9);
                assert!((easing["y2"].as_f64().unwrap() - y2).abs() < 1e-9);
            } else {
                assert_eq!(easing, &json!({"type": "linear"}));
            }
        }
    }

    // Valid saved reverse and media-end-segment clocks retain diagnosed
    // constant playback. Only the mismatched 0.5x source span loses a placement.
    for (clip, from, to, reason, kept, fallback, rejected_curve) in [
        (
            "94",
            "<PlaybackSpeed>0.8</PlaybackSpeed>",
            "<PlaybackSpeed>0.8</PlaybackSpeed><PlayBackwards>true</PlayBackwards>",
            "reverse playback combined with TimeRemapping is unsupported",
            2000,
            Some((0, 8000, 9600, 8000)),
            false,
        ),
        (
            "95",
            "<PlaybackSpeed>0.8</PlaybackSpeed>",
            "<PlaybackSpeed>0.5</PlaybackSpeed>",
            "source span does not match",
            0,
            Some((2000, 0, 0, 1600)),
            false,
        ),
        (
            "95",
            "<InPoint>0</InPoint>\n\t\t\t<OutPoint>406425600000</OutPoint>",
            "<InPoint>609638400000</InPoint>\n\t\t\t<OutPoint>1016064000000</OutPoint>",
            "TimeRemapping from a source In or at another speed plays past the key before the curve's media-end key",
            0,
            Some((2000, 2400, 2400, 4000)),
            false,
        ),
        (
            "94",
            "<PlaybackSpeed>0.8</PlaybackSpeed>\n\t\t\t<InPoint>101606400000</InPoint>\n\t\t\t<OutPoint>508032000000</OutPoint>",
            "<PlaybackSpeed>0.0001</PlaybackSpeed>\n\t\t\t<InPoint>0</InPoint>\n\t\t\t<OutPoint>50803200</OutPoint>",
            "saved source span rounds to an empty editable range",
            2000,
            None,
            true,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut xml = native.clone();
        edit_record(
            &mut xml,
            &format!("<VideoClip ObjectID=\"{clip}\""),
            "</VideoClip>",
            |record| {
                assert_eq!(record.matches(from).count(), 1);
                record.replace(from, to)
            },
        );
        if rejected_curve {
            // Native 0.2 ms is a valid saved span for 2 s at 0.0001x.
            // Reject only this decoded curve, before the mapper sees it.
            edit_record(
                &mut xml,
                "<TimeComponentParam ObjectID=\"108\"",
                "</TimeComponentParam>",
                |record| {
                    let start=record.find("<Keyframes>").unwrap();
                    let end=start+record[start..].find("</Keyframes>").unwrap()+"</Keyframes>".len();
                    let mut changed=record.to_owned();
                    changed.replace_range(start..end,"<Keyframes></Keyframes>");
                    changed
                },
            );
        }
        let source = dir.path().join(FIXTURE);
        write_prproj(&source, &xml);
        fs::write(dir.path().join("feature_timecoded_source.mp4"), &media).unwrap();
        if rejected_curve {
            let (_, reader_omissions) = premiere_file::PrProjectFile::load(&source).unwrap();
            assert!(reader_omissions.iter().any(|omission| {
                omission.kind == OmissionKind::Approximated
                    && omission.reason.contains("fewer than two usable keys")
            }));
            assert!(!reader_omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence));
        }
        let output = dir.path().join("converted");
        let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE), false).unwrap();
        let occurrences: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.scope == OmissionScope::Occurrence)
            .collect();
        let approximations: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.kind == OmissionKind::Approximated)
            .collect();
        let affected = if clip == "94" {
            "VideoClipTrackItem:63"
        } else {
            "VideoClipTrackItem:64"
        };
        if fallback.is_some() {
            assert!(occurrences.is_empty(), "{reason}: {omissions:?}");
            assert!(
                approximations.iter().any(|omission|omission.scope==OmissionScope::Feature && omission.record==affected && omission.reason.contains(reason))
                    && approximations.iter().all(|omission|omission.scope==OmissionScope::Feature && omission.record==affected && (omission.reason.contains("saved constant-rate playback") || omission.reason.contains("bounded authored source trim") || omission.reason.contains("bounded constant-speed recovery"))),
                "{reason}: {omissions:?}"
            );
        } else {
            if rejected_curve {
                assert!(
                    matches!(approximations[..], [omission]
                        if omission.record == affected
                            && omission.reason.contains("fewer than two usable keys")),
                    "{reason}: {omissions:?}"
                );
            } else {
                assert!(approximations.is_empty(), "{reason}: {omissions:?}");
            }
            assert!(
                matches!(occurrences[..], [omission]
                    if omission.kind == OmissionKind::Omitted
                        && omission.record == if clip == "94" { "VideoClipTrackItem:63" } else { "64" }
                        && omission.reason.contains(reason)),
                "{reason}: {omissions:?}"
            );
        }
        let archive = TesseractFile::open(first_project(&output)).unwrap();
        let document = archive.project_json().unwrap();
        let layers = video_layers(&document);
        assert_eq!(
            layers.len(),
            if fallback.is_some() { 2 } else { 1 },
            "{reason}"
        );
        let sibling = layers
            .iter()
            .find(|layer| layer["playback"]["inputRange"]["start"] == kept)
            .unwrap();
        let control = controls
            .iter()
            .find(|layer| layer["playback"]["inputRange"]["start"] == kept)
            .unwrap();
        assert_eq!(sibling["playback"], control["playback"], "{reason}");
        assert_eq!(sibling["source"], control["source"], "{reason}");
        assert_eq!(sibling["sourceRange"], json!({"start": 0, "duration": 10000}));
        assert_eq!(sibling["sourceIntrinsicDuration"], 10000);
        assert_eq!(archive.metadata().assets.len(), 1);
        for layer in &layers {
            let asset_id = layer["source"]["assetId"].as_str().unwrap();
            assert_eq!(
                archive
                    .asset(asset_id)
                    .unwrap()
                    .read_verified_bytes(media.len() as u64)
                    .unwrap(),
                media,
                "{reason}"
            );
        }
        if let Some((start, source_start, first, last)) = fallback {
            let retained = layers
                .iter()
                .find(|layer| layer["playback"]["inputRange"]["start"] == start)
                .unwrap();
            assert_eq!(retained["source"], sibling["source"]);
            assert_eq!(retained["sourceIntrinsicDuration"], 10000);
            assert_eq!(
                retained["sourceRange"],
                json!({"start": source_start, "duration": 1600})
            );
            let playback = &retained["playback"];
            assert_eq!(
                playback["inputRange"],
                json!({"start": start, "duration": 2000})
            );
            assert_eq!(playback["inputOffsetMs"], 0);
            assert_eq!(playback["mapping"]["type"], "timeRemap");
            let property = &playback["mapping"]["property"];
            assert_eq!(property["before"], "inactive");
            assert_eq!(property["after"], "inactive");
            let keys = property["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2);
            for (key, (time, value)) in keys.iter().zip([(start, first), (start + 2000, last)]) {
                assert_eq!(key["time"], time);
                assert_eq!(key["value"], value);
                assert_eq!(key["easing"], json!({"type": "linear"}));
            }
        }
    }
}
