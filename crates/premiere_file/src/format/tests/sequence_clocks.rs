//! Fractional placements stay editable; sampling cadence is not a tick-range restriction.

use super::nested::{
    placement_records, sequence_records, sequence_source, with_records, Placement,
};
use crate::{
    convert::premiere_to_tesseract,
    format::{inspect_project_with_omissions, reader::read_xml, FrameRate},
    schema::{
        text::PrGraphicObject, PrKeyframeEasing, PrMatteChannel, PrPropertyAnimation,
        PrScalarKeyframe, PrTrackMatte, PrVideoItem, PrVideoTrack, TICKS, TICKS_PER_MILLISECOND,
    },
    tesseract_output::asset_ids_in_order,
};
use serde_json::{json, Value};
use std::path::Path;

const TEXT_SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const FRAME: i64 = FrameRate::Fps30000Over1001.ticks_per_frame();

fn text_xml() -> String {
    // Independently authored public point-text source. The clock edits and
    // added nests below are structural controls, not Adobe endpoint proof.
    read_xml(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_text_point.prproj"),
    )
    .unwrap()
}

fn document(xml: &str, sequence_id: &str) -> Value {
    let (project, mut omissions) = inspect_project_with_omissions(xml, Some(sequence_id)).unwrap();
    let sequence = project.single_sequence().unwrap();
    premiere_to_tesseract(
        sequence,
        &project.media,
        &asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap()
}

#[test]
fn fractional_sequence_clock_keeps_known_matte_controls_but_not_failed_coverage() {
    // Native point-text content; paired nests and provider controls are
    // structural safety controls, not independently measured Adobe pixels.
    let (project, _) = inspect_project_with_omissions(&text_xml(), Some(TEXT_SEQUENCE)).unwrap();
    let child = project.single_sequence().unwrap();
    let start = 125 * TICKS_PER_MILLISECOND + 2 * TICKS_PER_MILLISECOND / 5;
    let range = start..start + 2 * TICKS;
    let key = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    let valid_rotation = PrPropertyAnimation::Rotation(vec![key(0, 0.0), key(TICKS, 90.0)]);
    // Real target failure: distinct native times collapse to one millisecond.
    // Holding these rotation keys at a default cannot establish matte coverage.
    let failed_rotation = PrPropertyAnimation::Rotation(vec![key(0, 0.0), key(1, 90.0)]);
    for (animations, failed) in [
        (Vec::new(), false),
        (vec![valid_rotation], false),
        (vec![failed_rotation], true),
    ] {
        let mut consumer = crate::tests::support::nest_of(child.clone(), range.clone(), 0);
        consumer.id = Some("fractional-consumer".into());
        consumer.track_matte = Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        });
        let mut provider = crate::tests::support::nest_of(child.clone(), range.clone(), 0);
        provider.id = Some("fractional-provider".into());
        provider.animations = animations;
        let sibling = crate::tests::support::nest_of(child.clone(), 3 * TICKS..5 * TICKS, 0);
        let outer = crate::tests::support::sequence_of(
            "Fractional coverage",
            vec![
                PrVideoTrack {
                    items: Vec::new(),
                    transitions: Vec::new(),
                    nests: vec![consumer],
                },
                PrVideoTrack {
                    items: Vec::new(),
                    transitions: Vec::new(),
                    nests: vec![provider, sibling],
                },
            ],
        );
        let mut omissions = Vec::new();
        let doc = premiere_to_tesseract(
            &outer,
            &project.media,
            &asset_ids_in_order(&outer, &project.media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        let roots = doc["composition"]["layers"].as_array().unwrap();
        if failed {
            let kept: Vec<_> = roots
                .iter()
                .filter(|layer| layer["type"] == "Group")
                .collect();
            assert_eq!(kept.len(), 1, "{roots:?}; {omissions:?}");
            assert_eq!(
                *crate::test_support::layer_range(kept[0]),
                json!({"start":3000,"duration":2000})
            );
            assert_eq!(kept[0]["layers"][0]["type"], "Text");
            assert!(
                omissions
                    .iter()
                    .any(|note| note.record == "fractional-consumer"
                        && note
                            .reason
                            .contains("native matte provider fractional-provider on track 1")
                        && note.reason.contains("was not converted")),
                "{omissions:?}"
            );
            assert!(
                omissions
                    .iter()
                    .any(|note| note.record == "fractional-provider"
                        && note
                            .reason
                            .contains("matte Motion keys could not be retained")),
                "{omissions:?}"
            );
        } else {
            assert_eq!(
                roots
                    .iter()
                    .filter(|layer| layer["type"] == "Group")
                    .count(),
                3,
                "{roots:?}; {omissions:?}"
            );
            let consumer = roots
                .iter()
                .find(|layer| !layer["trackMatte"].is_null())
                .unwrap();
            let provider = roots
                .iter()
                .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
                .unwrap();
            assert_eq!(
                *crate::test_support::layer_range(provider),
                json!({"start":125,"duration":2000})
            );
            assert_eq!(provider["layers"][0]["type"], "Text");
            assert_eq!(consumer["layers"][0]["type"], "Text");
            assert!(
                omissions
                    .iter()
                    .all(|note| note.scope != crate::OmissionScope::Occurrence),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn fractional_sequence_clock_retains_native_text_placement() {
    let start = 125 * TICKS_PER_MILLISECOND + 2 * TICKS_PER_MILLISECOND / 5;
    let xml = text_xml();
    assert_eq!(xml.matches("<Start>0</Start>").count(), 1);
    let xml = xml
        .replace("<Start>0</Start>", &format!("<Start>{start}</Start>"))
        .replace(
            &format!("<End>{}</End>", 2 * TICKS),
            &format!("<End>{}</End>", 2 * TICKS + start),
        );
    let document = document(&xml, TEXT_SEQUENCE);
    let text = &document["composition"]["layers"][0];
    assert_eq!(text["type"], "Text");
    assert!(!text["sourceText"]["text"].as_str().unwrap().is_empty());
    assert_eq!(
        *crate::test_support::layer_range(text),
        json!({"start":125,"duration":2000})
    );
}

#[test]
fn fractional_sequence_clock_retains_reverse_child_text_and_source_keys() {
    let length = 60 * FRAME;
    let mut xml = text_xml()
        .replace(
            "<FrameRate>8467200000</FrameRate>",
            &format!("<FrameRate>{FRAME}</FrameRate>"),
        )
        .replace(
            &format!("<End>{}</End>", 2 * TICKS),
            &format!("<End>{length}</End>"),
        )
        .replace(
            &format!("<OutPoint>{}</OutPoint>", 2 * TICKS),
            &format!("<OutPoint>{length}</OutPoint>"),
        );
    let mut records = sequence_records("clock-middle", "Clock middle", 600, &[610]);
    records.push_str(&sequence_source(602, TEXT_SEQUENCE).replace(
        &format!("<OriginalDuration>{}</OriginalDuration>", 5 * TICKS),
        &format!("<OriginalDuration>{length}</OriginalDuration>"),
    ));
    records.push_str(
        &placement_records(
            610,
            602,
            &Placement {
                start: 5 * FRAME,
                end: 65 * FRAME,
                source_in: 0,
            },
        )
        .replace(
            "<Clip><Source ObjectRef=\"602\"/>",
            "<Clip><PlayBackwards>true</PlayBackwards><Source ObjectRef=\"602\"/>",
        ),
    );
    records.push_str(&sequence_records("clock-outer", "Clock outer", 700, &[710]));
    records.push_str(&sequence_source(702, "clock-middle").replace(
        &format!("<OriginalDuration>{}</OriginalDuration>", 5 * TICKS),
        &format!("<OriginalDuration>{}</OriginalDuration>", 65 * FRAME),
    ));
    records.push_str(&placement_records(
        710,
        702,
        &Placement {
            start: FRAME,
            end: 66 * FRAME,
            source_in: 0,
        },
    ));
    xml = with_records(
        &xml,
        &records.replace(
            "<FrameRate>8467200000</FrameRate>",
            &format!("<FrameRate>{FRAME}</FrameRate>"),
        ),
    );
    let (project, mut omissions) =
        inspect_project_with_omissions(&xml, Some("clock-outer")).unwrap();
    let mut sequence = project.single_sequence().unwrap().clone();
    let graphic = &mut sequence.video_tracks[0].nests[0].sequence.video_tracks[0].nests[0]
        .sequence
        .video_tracks[0]
        .items[0];
    let PrVideoItem::Graphic(graphic) = graphic else {
        panic!("native text graphic")
    };
    let PrGraphicObject::Text(text) = &mut graphic.objects[0] else {
        panic!("native point text")
    };
    // Supplemental key control: descendant keys remain on their source clock,
    // not the increasing occurrence clock around the decreasing picture map.
    text.animations.push(PrPropertyAnimation::Rotation(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: length,
            value: 90.0,
            easing: PrKeyframeEasing::Linear,
        },
    ]));
    let document = premiere_to_tesseract(
        &sequence,
        &project.media,
        &asset_ids_in_order(&sequence, &project.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(
        omissions
            .iter()
            .any(|omission| omission.record == "VideoClipTrackItem:710"
                && omission
                    .reason
                    .contains("child tick bounds retain the residual phase")),
        "{omissions:?}"
    );
    let middle = &document["composition"]["layers"][0];
    assert_eq!(
        middle["playback"]["inputRange"],
        json!({"start":33,"duration":2169})
    );
    let occurrence = &middle["layers"][0];
    assert_eq!(
        occurrence["playback"]["inputRange"],
        json!({"start":167,"duration":2002})
    );
    let picture = &occurrence["layers"][0];
    let keys = crate::tests::support::playback_keys(picture);
    assert_eq!(
        keys.iter()
            .map(|key| key["value"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [2002, 0]
    );
    let text = &picture["layers"][0];
    assert_eq!(text["type"], "Text");
    assert!(!text["sourceText"]["text"].as_str().unwrap().is_empty());
    let rotation = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|track| {
            track["target"]["layerId"] == text["id"]
                && track["target"]["propertyType"] == "rotation"
        })
        .unwrap();
    assert_eq!(rotation["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(rotation["animator"]["keyframes"][1]["layerTime"], 2002);
    assert_eq!(rotation["animator"]["keyframes"][1]["value"]["value"], 90.0);
}
