//! The explicit frame hold extracted from a native Crop placement.

use super::support::*;
#[cfg(feature = "ffmpeg-library")]
use serde_json::json;
#[cfg(feature = "ffmpeg-library")]
use std::fs;
use std::path::Path;
#[cfg(feature = "ffmpeg-library")]
use std::path::PathBuf;
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;

#[cfg(feature = "ffmpeg-library")]
const NATIVE_HOLD: &str = include_str!("../fixtures/cap2-native-frame-hold.xml");
#[cfg(feature = "ffmpeg-library")]
const TEN_SECONDS: &[u8] = include_bytes!("../fixtures/video-30fps-10s.mp4");

#[cfg(feature = "ffmpeg-library")]
fn hold_xml(record: &str) -> String {
    let clip = record
        .replace("ObjectID=\"404\"", "ObjectID=\"6\"")
        .replace("ObjectRef=\"111\"", "ObjectRef=\"7\"");
    let start = XML.find("  <VideoClip ObjectID=\"6\"").unwrap();
    let end = start + XML[start..].find("</VideoClip>").unwrap() + "</VideoClip>".len();
    let mut xml = XML.to_owned();
    xml.replace_range(start..end, &clip);
    xml.replace(
        "<End>1270080000000</End>",
        "<Start>3602158560000</Start><End>3814050240000</End>",
    )
    .replacen(
        "<FrameRate>8467200000</FrameRate>",
        "<FrameRate>10594584000</FrameRate>",
        1,
    )
}

/// Imports the pinned hold, on its ten-second media, into `dir`.
#[cfg(feature = "ffmpeg-library")]
fn imported_hold(dir: &Path) -> PathBuf {
    let source = fixture(dir, &hold_xml(NATIVE_HOLD));
    fs::write(dir.join("media/source.mp4"), TEN_SECONDS).unwrap();
    let output = dir.join("converted");
    let omissions = premiere_to_tesseract(source, &output, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    first_project(&output)
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_explicit_frame_hold_imports_as_editable_constant_playback() {
    let dir = tempfile::tempdir().unwrap();
    let archive = imported_hold(dir.path());
    let file = TesseractFile::open(&archive).unwrap();
    let document = file.project_json().unwrap();
    let video = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(video["sourceRange"], json!({"start":0,"duration":10000}));
    let playback = &video["playback"];
    assert_eq!(
        playback["inputRange"],
        json!({"start":14181,"duration":834})
    );
    assert_eq!(playback["inputOffsetMs"], 0);
    assert_eq!(playback["mapping"]["type"], "timeRemap");
    let keys = &playback["mapping"]["property"]["keyframes"];
    assert_eq!(keys[0]["time"], 14181);
    assert_eq!(keys[1]["time"], 15015);
    assert_eq!(keys[0]["value"], 3462);
    assert_eq!(keys[1]["value"], 3462);
    assert_eq!(keys.as_array().unwrap().len(), 2);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_frame_hold_exports_as_native_frame_hold() {
    let dir = tempfile::tempdir().unwrap();
    let archive = imported_hold(dir.path());
    let native = dir.path().join("native");
    let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    // The generated records, read without this crate's reader: the editable
    // curve's held 3462 ms is the explicit mode-4 Frame Hold start and the
    // source In, Out spans the placement, and neither a speed nor a remap
    // curve is written. The placement keeps its 14181-15015 ms window on the
    // 30 fps export frames 425-450.
    let project = native.join("project.prproj");
    let xml = read_xml(&project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let ticks = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(|text| text.parse::<i64>().unwrap())
    };
    let holds: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("VideoClip") && ticks(*node, "FrameHold").is_some())
        .collect();
    let [hold] = holds.as_slice() else {
        panic!("expected one held VideoClip, found {}", holds.len());
    };
    let clip = hold
        .children()
        .find(|child| child.has_tag_name("Clip"))
        .unwrap();
    let held = 3462 * crate::test_support::TICKS / 1000;
    let frame = crate::test_support::TICKS / 30;
    assert_eq!(
        [
            ticks(*hold, "FrameHold"),
            ticks(*hold, "FrameHoldStart"),
            ticks(clip, "InPoint"),
            ticks(clip, "OutPoint"),
        ],
        [Some(4), Some(held), Some(held), Some(held + 25 * frame)]
    );
    assert!(
        !xml.contains("<PlaybackSpeed") && !xml.contains("<TimeRemapping"),
        "{xml}"
    );
    let placements: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("VideoClipTrackItem"))
        .map(|item| {
            let first = |tag: &str| {
                item.descendants()
                    .find(|node| node.has_tag_name(tag))
                    .and_then(|node| node.text())
                    .map(|text| text.parse::<i64>().unwrap())
            };
            (first("Start"), first("End"))
        })
        .collect();
    assert_eq!(placements, [(Some(425 * frame), Some(450 * frame))]);

    // Supplementary: this crate reads the export back as the same hold.
    let root_sequence = exported_root_sequence(&project);
    let again = dir.path().join("again");
    let omissions = premiere_to_tesseract(&project, &again, Some(&root_sequence), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = TesseractFile::open(first_project(&again))
        .unwrap()
        .project_json()
        .unwrap();
    let video = reimported["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let keys = &video["playback"]["mapping"]["property"]["keyframes"];
    assert_eq!(keys.as_array().unwrap().len(), 2);
    assert_eq!(keys[0]["value"], 3462);
    assert_eq!(keys[1]["value"], 3462);
}

/// Derived nesting of the unchanged native hold record, not an Adobe-saved
/// composite-hold oracle. Both unit-forward windows trim its head and tail.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_frame_hold_child_survives_nested_windows_and_independent_export_edits() {
    const FRAME: i64 = 10_594_584_000; // The native hold's 24000/1001 sequence clock.
    let mut records = format!(
        r#"<Sequence ObjectUID="hold-outer"><Name>Held children</Name><TrackGroups><TrackGroup><Second ObjectRef="100"/></TrackGroup></TrackGroups></Sequence>
<VideoTrackGroup ObjectID="100"><TrackGroup><Tracks><Track ObjectURef="hold-outer-track"/></Tracks><FrameRate>{FRAME}</FrameRate></TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef="101"/></ComponentOwner></VideoTrackGroup>
<VideoComponentChain ObjectID="101"><ComponentChain/></VideoComponentChain>
<VideoClipTrack ObjectUID="hold-outer-track"><ClipTrack><Track><ID>1</ID></Track><ClipItems><TrackItems><TrackItem ObjectRef="110"/><TrackItem ObjectRef="120"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>
<VideoSequenceSource ObjectID="102"><SequenceSource><Sequence ObjectURef="sequence-1"/></SequenceSource><OriginalDuration>{}</OriginalDuration></VideoSequenceSource>"#,
        360 * FRAME
    );
    for (id, start, source_in, duration) in [(110, 0, 342, 16), (120, 120, 344, 14)] {
        let (chain, sub, clip) = (id + 1, id + 2, id + 3);
        records.push_str(&format!(
            r#"<VideoClipTrackItem ObjectID="{id}"><ClipTrackItem><ComponentOwner><Components ObjectRef="{chain}"/></ComponentOwner><TrackItem><Start>{}</Start><End>{}</End></TrackItem><SubClip ObjectRef="{sub}"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
<VideoComponentChain ObjectID="{chain}"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>
<SubClip ObjectID="{sub}"><Clip ObjectRef="{clip}"/><Name>Held copy</Name></SubClip>
<VideoClip ObjectID="{clip}"><Clip><Source ObjectRef="102"/><InPoint>{}</InPoint><OutPoint>{}</OutPoint></Clip></VideoClip>"#,
            start * FRAME,
            (start + duration) * FRAME,
            source_in * FRAME,
            (source_in + duration) * FRAME
        ));
    }
    let xml =
        hold_xml(NATIVE_HOLD).replace("</PremiereData>", &format!("{records}</PremiereData>"));
    let dir = tempfile::tempdir().unwrap();
    let source = fixture(dir.path(), &xml);
    fs::write(dir.path().join("media/source.mp4"), TEN_SECONDS).unwrap();
    let output = dir.path().join("converted");
    let losses = premiere_to_tesseract(source, &output, Some("hold-outer"), false).unwrap();
    assert!(losses.is_empty(), "{losses:?}");
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    assert_eq!(file.metadata().assets.len(), 1);
    let groups = document["composition"]["layers"].as_array_mut().unwrap();
    assert_eq!(
        groups
            .iter()
            .filter(|layer| layer["type"] == "Group")
            .count(),
        2
    );
    let mut identities = Vec::new();
    let mut assets = Vec::new();
    for (index, group) in groups
        .iter_mut()
        .filter(|layer| layer["type"] == "Group")
        .enumerate()
    {
        let (start, duration) = [(0, 667), (5005, 584)][index];
        assert_eq!(
            group["playback"]["inputRange"],
            json!({"start":start,"duration":duration})
        );
        let children = group["layers"].as_array_mut().unwrap();
        let [video] = children.as_mut_slice() else {
            panic!("expected one held child: {children:?}");
        };
        assert_eq!(video["type"], "Video");
        assert_eq!(video["sourceRange"], json!({"start":0,"duration":10000}));
        assert_eq!(
            video["playback"]["inputRange"],
            json!({"start":0,"duration":duration})
        );
        let keys = video["playback"]["mapping"]["property"]["keyframes"]
            .as_array_mut()
            .unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(
            (keys[0]["time"].as_u64(), keys[1]["time"].as_u64()),
            (Some(0), Some(duration))
        );
        assert!(keys.iter().all(|key| key["value"] == 3462));
        if index == 1 {
            for key in keys {
                key["value"] = json!(4000);
            }
        }
        identities.push(video["id"].clone());
        assets.push(video["source"]["assetId"].clone());
    }
    assert_ne!(identities[0], identities[1]);
    assert_eq!(assets[0], assets[1]);
    assert_eq!(assets[0], json!("premiere-video-1"));
    assert_eq!(document["duration"], json!(5.589));
    let archive = dir.path().join("edited.tsrct");
    write_archive(&archive, &document, &dir.path().join("media/source.mp4"));
    let native = dir.path().join("native");
    let losses = tesseract_to_premiere(&archive, &native, false).unwrap();
    // The 5589 ms document end and last occurrence both round to frame 168
    // (5600 ms) at 30 fps. Gap export no longer emits canvas-coverage reports;
    // matching endpoints must not trigger the last-occurrence diagnostic.
    assert!(losses.is_empty(), "{losses:?}");
    // Independent XML inspection: each edited child is an explicit native
    // hold, rather than an ordinary moving clip or an opaque retained source.
    let xml = read_xml(&native.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, tag: &str) -> roxmltree::Node<'a, 'i> {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .unwrap()
    }
    let records: std::collections::BTreeMap<_, _> = parsed
        .root_element()
        .children()
        .filter_map(|node| Some((node.attribute("ObjectID")?, node)))
        .collect();
    let follow = |node: roxmltree::Node<'_, '_>, tag: &str| {
        records[child(node, tag).attribute("ObjectRef").unwrap()]
    };
    let by_uid = |uid: &str| {
        parsed
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectUID") == Some(uid))
            .unwrap()
    };
    let value = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.descendants()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(|text| text.parse::<i64>().unwrap())
    };
    let items = |sequence: roxmltree::Node<'_, '_>| {
        let group = follow(
            child(child(sequence, "TrackGroups"), "TrackGroup"),
            "Second",
        );
        assert!(group.has_tag_name("VideoTrackGroup"));
        child(child(group, "TrackGroup"), "Tracks")
            .children()
            .filter(|node| node.has_tag_name("Track"))
            .flat_map(|track| {
                let track = by_uid(track.attribute("ObjectURef").unwrap());
                child(child(child(track, "ClipTrack"), "ClipItems"), "TrackItems")
                    .children()
                    .filter(|node| node.has_tag_name("TrackItem"))
                    .map(|item| records[item.attribute("ObjectRef").unwrap()])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let sequences: Vec<_> = parsed
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Sequence"))
        .collect();
    assert_eq!(
        sequences.len(),
        3,
        "one root and two independent child sequences"
    );
    let outer = sequences
        .into_iter()
        .find(|node| child(*node, "Name").text() == Some("Held children"))
        .unwrap();
    let mut placements = items(outer);
    placements.sort_by_key(|item| value(*item, "Start").unwrap_or(0));
    assert_eq!(
        placements.len(),
        2,
        "the holds must not flatten into root clips"
    );
    let ticks = crate::test_support::TICKS;
    let frame = ticks / 30;
    let mut sources = Vec::new();
    let mut nested_uids = Vec::new();
    for (index, item) in placements.into_iter().enumerate() {
        let (start, end, held) = [(0, 20, 3462 * ticks / 1000), (150, 168, 4 * ticks)][index];
        assert_eq!(
            (
                value(item, "Start").unwrap_or(0),
                value(item, "End").unwrap()
            ),
            (start * frame, end * frame)
        );
        let nest = follow(follow(child(item, "ClipTrackItem"), "SubClip"), "Clip");
        assert_eq!(
            (value(nest, "InPoint"), value(nest, "OutPoint")),
            (Some(0), Some((end - start) * frame))
        );
        let source = follow(child(nest, "Clip"), "Source");
        assert!(source.has_tag_name("VideoSequenceSource"));
        let uid = child(child(source, "SequenceSource"), "Sequence")
            .attribute("ObjectURef")
            .unwrap();
        nested_uids.push(uid);
        let children = items(by_uid(uid));
        let [item] = children.as_slice() else {
            panic!("each nest must own exactly one held child");
        };
        assert_eq!(
            (
                value(*item, "Start").unwrap_or(0),
                value(*item, "End").unwrap()
            ),
            (0, (end - start) * frame)
        );
        let hold = follow(follow(child(*item, "ClipTrackItem"), "SubClip"), "Clip");
        assert_eq!(
            (
                value(hold, "FrameHold"),
                value(hold, "FrameHoldStart"),
                value(hold, "InPoint"),
                value(hold, "OutPoint")
            ),
            (
                Some(4),
                Some(held),
                Some(held),
                Some(held + (end - start) * frame)
            )
        );
        sources.push(
            child(child(hold, "Clip"), "Source")
                .attribute("ObjectRef")
                .unwrap(),
        );
    }
    assert_ne!(
        nested_uids[0], nested_uids[1],
        "each placement owns a separate nest"
    );
    assert_eq!(
        sources[0], sources[1],
        "independent holds still share physical media"
    );
    assert!(!xml.contains("<PlaybackSpeed") && !xml.contains("<TimeRemapping"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn edited_frame_hold_keeps_valid_easing_but_rejects_ineligible_sources() {
    // Each curve's first key is at the window start; the second key's time
    // and easing vary.
    for (case, (start, duration), held, last_time, last_easing) in [
        (
            "held instant before the source selection",
            (4000, 6000),
            3462,
            1000,
            "linear",
        ),
        (
            "held instant at the exclusive end of the source selection",
            (0, 5000),
            5000,
            1000,
            "linear",
        ),
        (
            "curve ending inside the window",
            (0, 10000),
            3462,
            900,
            "linear",
        ),
        ("Hold-eased curve", (0, 10000), 3462, 1000, "hold"),
    ] {
        let keys = json!([
            {"id": "a", "time": 0, "value": held, "easing": {"type": "linear"}},
            {"id": "b", "time": last_time, "value": held, "easing": {"type": last_easing}}
        ]);
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("source.mp4");
        fs::write(&media, TEN_SECONDS).unwrap();
        let mut document = crate::test_support::editable_document();
        let video = &mut document["composition"]["layers"][0];
        video["sourceRange"] = json!({"start": start, "duration": duration});
        video["sourceIntrinsicDuration"] = json!(10_000);
        video["playback"] = crate::test_support::remapped_playback(
            json!({"start": 0, "duration": 1000}),
            json!({"keyframes": keys, "before": "inactive", "after": "inactive"}),
        );
        let mut sibling = video.clone();
        sibling["id"] = json!(3);
        sibling["sourceRange"] = json!({"start": 7000, "duration": 1000});
        sibling["playback"] = crate::test_support::linear_playback(
            json!({"start": 1000, "duration": 1000}),
            json!({"start": 7000, "duration": 1000}),
        );
        document["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
        document["duration"] = json!(2);
        let archive = dir.path().join("edited.tsrct");
        crate::test_support::write_archive(&archive, &document, &media);
        let native = dir.path().join("native");
        let losses = tesseract_to_premiere(&archive, &native, false).unwrap();
        let xml = read_xml(&native.join("project.prproj"));
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        let holds: Vec<i64> = parsed
            .descendants()
            .filter(|node| node.has_tag_name("FrameHoldStart"))
            .map(|node| node.text().unwrap().parse().unwrap())
            .collect();
        if last_easing == "hold" {
            assert_eq!(holds, [3462 * crate::test_support::TICKS / 1000]);
            assert_eq!(holds[0] / (crate::test_support::TICKS / 30), 103);
            assert!(losses.iter().any(|loss| loss
                .reason
                .contains("editable speed and Frame Hold segments")));
        } else {
            assert!(holds.is_empty(), "{case}: {holds:?}");
            assert!(
                losses.iter().any(|loss| loss.record.contains("layer 1")
                    && loss.reason.contains("time remapping was not exported")),
                "{case}: {losses:?}"
            );
        }
        let sibling_in = (7 * crate::test_support::TICKS).to_string();
        assert!(
            parsed
                .descendants()
                .any(|node| node.has_tag_name("VideoClip")
                    && node.descendants().any(|child| child.has_tag_name("InPoint")
                        && child.text() == Some(sibling_in.as_str()))),
            "{case}: lost unrelated video"
        );
    }
}

// Retained pictures now require physical media admission, unlike the former
// reader-only rejection case. Keep this publication proof in the FFmpeg lane.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_frame_hold_recovers_unknown_incomplete_out_of_bounds_and_combined_forms() {
    for (from, to, reason) in [
        (
            "<FrameHold>4</FrameHold>",
            "<FrameHold>3</FrameHold>",
            "FrameHold",
        ),
        (
            "<FrameHoldStart>879350472000</FrameHoldStart>",
            "",
            "FrameHold",
        ),
        ("<FrameHold>4</FrameHold>", "", "FrameHold"),
        (
            "879350472000</FrameHoldStart>",
            "-1</FrameHoldStart>",
            "FrameHold",
        ),
        (
            "879350472000</FrameHoldStart>",
            "2540160000000</FrameHoldStart>",
            "TimeRemapping",
        ),
        (
            "<InPoint>",
            "<PlaybackSpeed>2</PlaybackSpeed><InPoint>",
            "TimeRemapping",
        ),
        (
            "<InPoint>",
            "<TimeRemapping ObjectRef=\"4\"/><InPoint>",
            "FrameHold",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path(), &hold_xml(&NATIVE_HOLD.replace(from, to)));
        fs::write(dir.path().join("media/source.mp4"), TEN_SECONDS).unwrap();
        let output = dir.path().join("converted");
        let losses = premiere_to_tesseract(source, &output, None, false).unwrap();
        assert!(
            losses.iter().any(|loss| loss.reason.contains(reason)),
            "{losses:?}"
        );
        let project = fs::read_dir(&output)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "tsrct")
            })
            .unwrap();
        let file = tesseract_file::TesseractFile::open(project).unwrap();
        let document = file.project_json().unwrap();
        let video = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(
            video["sourceRange"],
            serde_json::json!({"start":3462,"duration":834})
        );
        assert_eq!(video["playback"]["mapping"]["output"], video["sourceRange"]);
        assert_eq!(
            video["playback"]["inputRange"],
            serde_json::json!({"start":14181,"duration":834})
        );
    }
}

#[test]
fn native_frame_hold_is_rejected_on_master_sources_graphics_and_adjustments() {
    // Inject holds into independently pinned existing native sources to ensure
    // the newly typed fields cannot bypass unsupported clock owners.
    for (name, master, reason) in [
        (
            "feature_constant_reverse_0_905_strict.prproj",
            true,
            "FrameHold on a master source",
        ),
        ("feature_text_point.prproj", false, "graphic retiming"),
        (
            "feature_adjustment_layer_26_5_strict.prproj",
            false,
            "FrameHold on an adjustment layer",
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let xml = read_xml(&path);
        let document = roxmltree::Document::parse(&xml).unwrap();
        let id = if name == "feature_adjustment_layer_26_5_strict.prproj" {
            document
                .descendants()
                .find(|node| {
                    node.has_tag_name("VideoClip")
                        && node.children().any(|child| {
                            child.has_tag_name("AdjustmentLayer") && child.text() == Some("true")
                        })
                })
                .unwrap()
                .attribute("ObjectID")
                .unwrap()
        } else if master {
            document
                .descendants()
                .find(|node| node.has_tag_name("MasterClip"))
                .unwrap()
                .descendants()
                .find(|node| node.has_tag_name("Clip") && node.attribute("ObjectRef").is_some())
                .unwrap()
                .attribute("ObjectRef")
                .unwrap()
        } else {
            let sub = document
                .descendants()
                .find(|node| node.has_tag_name("VideoClipTrackItem"))
                .unwrap()
                .descendants()
                .find(|node| node.has_tag_name("SubClip"))
                .unwrap()
                .attribute("ObjectRef")
                .unwrap();
            document
                .root_element()
                .children()
                .find(|node| {
                    node.has_tag_name("SubClip") && node.attribute("ObjectID") == Some(sub)
                })
                .unwrap()
                .children()
                .find(|node| node.has_tag_name("Clip"))
                .unwrap()
                .attribute("ObjectRef")
                .unwrap()
        };
        let record = document
            .root_element()
            .children()
            .find(|node| node.has_tag_name("VideoClip") && node.attribute("ObjectID") == Some(id))
            .unwrap();
        let position = record.range().end - "</VideoClip>".len();
        for fields in [
            "<FrameHold>4</FrameHold><FrameHoldStart>0</FrameHoldStart>",
            "<FrameHoldStart>0</FrameHoldStart>",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut edited = xml.clone();
            edited.insert_str(position, fields);
            let input = dir.path().join("held.prproj");
            write_prproj(&input, &edited);
            match premiere_file::PrProjectFile::load(&input) {
                Ok((_, omissions)) => assert!(
                    omissions
                        .iter()
                        .any(|omission| omission.to_string().contains(reason)),
                    "{omissions:?}"
                ),
                Err(error) => assert!(error.to_string().contains(reason), "{error}"),
            }
        }
    }
}
