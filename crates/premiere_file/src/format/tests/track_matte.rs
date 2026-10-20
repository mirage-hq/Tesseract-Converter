//! Reader and writer rules for the Track Matte Key.

use super::{
    animation::animation_fixture::{track_matte_key_xml, with_matte_track, MATTE_TRACK, SOURCE},
    effects::{blur, invert, tint, top_crop, track_matte_key, with_second_clip},
    nested::{
        corpus_motion_component, placement_chain, placement_records, read_motion, sequence_records,
        sequence_source, with_records, Placement, MOTION,
    },
};
use crate::{
    format::{inspect_project_with_omissions, writer::project_xml, Graph},
    schema::{
        check_track_matte, native::VideoComponentParam, MediaId, PrColorMatte, PrKeyframeEasing,
        PrMatteChannel, PrMediaKind, PrNestOccurrence, PrPropertyAnimation, PrScalarKeyframe,
        PrStaticTransform, PrTrackMatte, PrVideoOccurrence, PrVideoTrack, TICKS,
    },
    tests::support::{clip_of, nest_of, sequence_of, video_media},
    OmissionScope,
};

#[derive(Clone, Copy)]
enum NativeMatteCoverage {
    Full,
    ProviderExtends,
    EarlyEnd,
    LateStart,
    Missing,
}

#[derive(Clone, Copy)]
enum NativeMatteConsumer {
    Video,
    Group,
    Rect,
}

fn append_opacity_to_chain(chain: &str, chain_id: u32, component_id: u32, keys: &str) -> String {
    let opacity = super::nested::opacity_chain(chain_id, component_id, 100.0, keys);
    let (_, records) = opacity.split_once("</VideoComponentChain>").unwrap();
    let chain = chain
        .replace("<DefaultOpacity>true</DefaultOpacity>", "")
        .replace(
            "</Components>",
            &format!("<Component Index=\"1\" ObjectRef=\"{component_id}\"/></Components>"),
        );
    format!("{chain}{records}")
}

/// Reduced native-derived controls, not new Adobe oracles. Keep the authored
/// Alpha pair129/138, source media and key record forms; alter only placement
/// windows and add independent source-clock Opacity keys.
fn native_matte_interval_control(coverage: NativeMatteCoverage) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_track_matte_key_26_5_strict.prproj");
    let mut xml = crate::format::read_xml(&path).unwrap();
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let mut removed: Vec<_> = parsed
        .descendants()
        .filter(|node| {
            node.has_tag_name("TrackItem")
                && node.attribute("ObjectRef").is_some()
                && node
                    .ancestors()
                    .any(|parent| parent.has_tag_name("VideoClipTrack"))
                && !matches!(node.attribute("ObjectRef"), Some("129" | "138"))
        })
        .map(|node| node.range())
        .collect();
    removed.sort_by_key(|range| std::cmp::Reverse(range.start));
    for range in removed {
        xml.replace_range(range, "");
    }
    let still_in = 914_449_132_800_000_i64;
    for id in ["129", "221", "138", "240", "164", "182"] {
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        let node = parsed
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        let before = &xml[node.range()];
        let after = match (id, coverage) {
            ("129", NativeMatteCoverage::ProviderExtends) => before.replace(
                "<End>508032000000</End>",
                &format!("<Start>{TICKS}</Start><End>{}</End>", 2 * TICKS),
            ),
            ("221", NativeMatteCoverage::ProviderExtends) => before.replace(
                "<OutPoint>508032000000</OutPoint>",
                &format!("<OutPoint>{TICKS}</OutPoint>"),
            ),
            ("138", NativeMatteCoverage::EarlyEnd) => before.replace(
                "<End>508032000000</End>",
                &format!("<End>{}</End>", 3 * TICKS / 2),
            ),
            ("240", NativeMatteCoverage::EarlyEnd) => before.replace(
                "<OutPoint>914957164800000</OutPoint>",
                &format!("<OutPoint>{}</OutPoint>", still_in + 3 * TICKS / 2),
            ),
            ("138", NativeMatteCoverage::LateStart) => before.replace(
                "<End>508032000000</End>",
                &format!("<Start>{}</Start><End>{}</End>", TICKS / 2, 2 * TICKS),
            ),
            ("240", NativeMatteCoverage::LateStart) => before.replace(
                "<OutPoint>914957164800000</OutPoint>",
                &format!("<OutPoint>{}</OutPoint>", still_in + 3 * TICKS / 2),
            ),
            ("138", NativeMatteCoverage::Missing) => before.replace(
                "<End>508032000000</End>",
                &format!("<Start>{}</Start><End>{}</End>", 2 * TICKS, 3 * TICKS),
            ),
            ("164", NativeMatteCoverage::LateStart | NativeMatteCoverage::Missing) => {
                append_opacity_to_chain(
                    before,
                    164,
                    10_100,
                    &format!("<Keyframes>0,100.,0,0,0,0,0,0;{TICKS},50.,0,0,0,0,0,0;</Keyframes>"),
                )
            }
            ("182", _) => super::nested::opacity_chain(
                182,
                10_000,
                100.0,
                &format!(
                    "<Keyframes>{still_in},100.,0,0,0,0,0,0;{},50.,0,0,0,0,0,0;</Keyframes>",
                    still_in + TICKS / 2
                ),
            ),
            _ => before.to_owned(),
        };
        xml.replace_range(node.range(), &after);
    }
    xml
}

fn with_nested_matte_consumer(mut xml: String) -> String {
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let clip = parsed
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some("221"))
        .unwrap();
    let range = clip.range();
    let before = &xml[range.clone()];
    let after = before.replace("<Source ObjectRef=\"80\"/>", "<Source ObjectRef=\"9002\"/>");
    assert_ne!(before, after);
    xml.replace_range(range, &after);
    let range = {
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        parsed
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some("165"))
            .unwrap()
            .range()
    };
    let before = &xml[range.clone()];
    let after = before.replace(
        "<MasterClip ObjectURef=\"f0c217c5-dfb2-475c-a45e-e377882616b0\"/>",
        "",
    );
    assert_ne!(before, after);
    xml.replace_range(range, &after);

    let placement = Placement {
        start: 0,
        end: 2 * TICKS,
        source_in: 0,
    };
    let records = sequence_records("matte-consumer-nest", "Matte consumer nest", 9000, &[9010])
        + &sequence_source(9002, "matte-consumer-nest")
        + &placement_records(9010, 80, &placement);
    with_records(&xml, &records)
}

fn native_matte_interval_wire(
    coverage: NativeMatteCoverage,
    consumer: NativeMatteConsumer,
) -> (serde_json::Value, Vec<crate::Omission>) {
    let mut xml = native_matte_interval_control(coverage);
    if matches!(consumer, NativeMatteConsumer::Group) {
        xml = with_nested_matte_consumer(xml);
    }
    let (mut project, mut omissions) =
        inspect_project_with_omissions(&xml, Some("3776e3eb-791f-4e6a-a2bb-7e77eff235ef")).unwrap();
    if matches!(consumer, NativeMatteConsumer::Rect) {
        let media = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .find(|clip| clip.track_matte.is_some())
            .unwrap()
            .media
            .clone();
        project
            .media
            .get_mut(&media)
            .unwrap()
            .video
            .as_mut()
            .unwrap()
            .kind = PrMediaKind::ColorMatte(PrColorMatte { rgb: [12, 34, 56] });
    }
    let sequence = project.single_sequence().unwrap();
    if !matches!(
        coverage,
        NativeMatteCoverage::Missing | NativeMatteCoverage::ProviderExtends
    ) {
        assert!(
            sequence
                .video_occurrences()
                .any(|clip| clip.track_matte.is_some())
                || sequence
                    .nest_occurrences()
                    .any(|nest| nest.track_matte.is_some()),
            "{omissions:?}"
        );
    }
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    fx_schema::EditableFxCompositionDocument::from_json_value(document.clone()).unwrap();
    (document, omissions)
}

fn consecutive_native_matte_xml() -> String {
    let mut xml =
        with_nested_matte_consumer(native_matte_interval_control(NativeMatteCoverage::Full));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let record = |id| {
        let node = parsed
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        xml[node.range()].to_owned()
    };
    let provider = record("138");
    let subclip = record("183");
    let source = record("240");
    let still_in = 914_449_132_800_000_i64;
    let second = provider
        .replace("ObjectID=\"138\"", "ObjectID=\"10138\"")
        .replace("ObjectRef=\"183\"", "ObjectRef=\"10183\"")
        .replace("<End>", &format!("<Start>{TICKS}</Start><End>"))
        + &subclip
            .replace("ObjectID=\"183\"", "ObjectID=\"10183\"")
            .replace("ObjectRef=\"240\"", "ObjectRef=\"10240\"")
        + &source
            .replace("ObjectID=\"240\"", "ObjectID=\"10240\"")
            .replace(
                &format!("<InPoint>{still_in}</InPoint>"),
                &format!("<InPoint>{}</InPoint>", still_in + TICKS),
            );
    xml = xml.replace(&provider, &provider.replace("<End>508032000000</End>", &format!("<End>{TICKS}</End>")))
        .replace(&source, &source.replace("<OutPoint>914957164800000</OutPoint>",
                   &format!("<OutPoint>{}</OutPoint>", still_in + TICKS)))
        .replace("<TrackItem Index=\"0\" ObjectRef=\"138\"/>",
                 "<TrackItem Index=\"0\" ObjectRef=\"138\"/><TrackItem Index=\"1\" ObjectRef=\"10138\"/>");
    with_records(&xml, &second)
}

#[test]
fn consecutive_track_mattes_keep_nested_picture_and_source_clock() {
    let xml = consecutive_native_matte_xml();
    let (project, mut omissions) =
        inspect_project_with_omissions(&xml, Some("3776e3eb-791f-4e6a-a2bb-7e77eff235ef")).unwrap();
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let mut consumers: Vec<_> = layers
        .iter()
        .filter(|layer| layer["trackMatte"].is_object())
        .collect();
    consumers.sort_by_key(|layer| layer["playback"]["inputRange"]["start"].as_i64().unwrap());
    assert_eq!(consumers.len(), 2, "{omissions:?}");
    for (index, consumer) in consumers.iter().enumerate() {
        assert_eq!(consumer["type"], "Group");
        assert_eq!(
            consumer["playback"]["inputRange"],
            serde_json::json!({"start": index * 1000, "duration": 1000})
        );
        let child = consumer["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(
            child["sourceRange"],
            serde_json::json!({"start": index * 1000, "duration": 1000})
        );
        let provider = layers
            .iter()
            .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(provider["activeRange"], consumer["playback"]["inputRange"]);
    }
    assert_ne!(
        consumers[0]["trackMatte"]["layer"],
        consumers[1]["trackMatte"]["layer"]
    );
    fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
}

#[test]
fn consecutive_track_mattes_reject_gaps_and_overlaps() {
    let xml = consecutive_native_matte_xml();
    for start in [TICKS / 2, 3 * TICKS / 2] {
        let xml = xml.replace(
            &format!("<Start>{TICKS}</Start>"),
            &format!("<Start>{start}</Start>"),
        );
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("3776e3eb-791f-4e6a-a2bb-7e77eff235ef"))
                .unwrap();
        let keyed = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .filter(|nest| nest.track_matte.is_some())
            .count();
        if start < TICKS {
            assert_eq!(keyed, 1);
            assert!(
                omissions
                    .iter()
                    .any(|loss| loss.reason.contains("overlaps another occurrence")),
                "{omissions:?}"
            );
        } else {
            assert_eq!(keyed, 0);
            assert!(
                omissions
                    .iter()
                    .any(|loss| loss.reason.contains("only one unambiguous provider")),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn track_matte_provider_interval_full_coverage_keeps_native_source_clock() {
    let (document, _) =
        native_matte_interval_wire(NativeMatteCoverage::Full, NativeMatteConsumer::Video);
    let layers = document["composition"]["layers"].as_array().unwrap();
    let consumer = layers
        .iter()
        .find(|layer| layer["trackMatte"].is_object())
        .unwrap();
    let provider = layers
        .iter()
        .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
        .unwrap();
    assert_eq!(consumer["type"], "Video");
    assert_eq!(
        consumer["playback"]["inputRange"],
        serde_json::json!({"start":0,"duration":2000})
    );
    assert_eq!(
        consumer["sourceRange"],
        serde_json::json!({"start":0,"duration":2000})
    );
    assert_eq!(provider["type"], "Image");
    assert_eq!(
        provider["activeRange"],
        serde_json::json!({"start":0,"duration":2000})
    );
    let keys = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == provider["id"]
                && entry["target"]["propertyType"] == "opacity"
        })
        .unwrap();
    assert_eq!(keys["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(keys["animator"]["keyframes"][1]["layerTime"], 500);
    assert_eq!(keys["animator"]["keyframes"][1]["value"]["value"], 50.0);
}

#[test]
fn track_matte_provider_outside_consumer_is_rejected_instead_of_silently_consumed() {
    let (document, omissions) = native_matte_interval_wire(
        NativeMatteCoverage::ProviderExtends,
        NativeMatteConsumer::Video,
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert!(
        layers.iter().all(|layer| !layer["trackMatte"].is_object()),
        "{document:#}"
    );
    assert!(
        !layers.iter().any(|layer| layer["type"] == "Image"),
        "{document:#}"
    );
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("independently painting interval cannot be consumed")),
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("matte source of the omitted clip")),
        "{omissions:?}"
    );
}

#[test]
fn track_matte_provider_interval_partial_coverage_keeps_covered_editable_video() {
    let (document, omissions) =
        native_matte_interval_wire(NativeMatteCoverage::EarlyEnd, NativeMatteConsumer::Video);
    let layers = document["composition"]["layers"].as_array().unwrap();
    let consumer = layers
        .iter()
        .find(|layer| layer["trackMatte"].is_object())
        .unwrap();
    let provider = layers
        .iter()
        .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
        .unwrap();
    assert_eq!(consumer["type"], "Video");
    assert_eq!(
        consumer["playback"]["inputRange"],
        serde_json::json!({"start":0,"duration":1500})
    );
    assert_eq!(
        consumer["playback"]["mapping"]["input"],
        serde_json::json!({"start":0,"duration":2000})
    );
    assert_eq!(
        consumer["playback"]["mapping"]["output"],
        serde_json::json!({"start":0,"duration":2000})
    );
    assert_eq!(
        provider["activeRange"],
        serde_json::json!({"start":0,"duration":1500})
    );
    let keys = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == provider["id"]
                && entry["target"]["propertyType"] == "opacity"
        })
        .unwrap();
    assert_eq!(keys["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(keys["animator"]["keyframes"][1]["layerTime"], 500);
    assert_eq!(keys["animator"]["keyframes"][1]["value"]["value"], 50.0);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("uncovered interval")
                && omission.reason.contains("omitted")),
        "{omissions:?}"
    );
    fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
}

#[test]
fn track_matte_provider_late_start_preserves_video_source_and_animation_clocks() {
    let (document, omissions) =
        native_matte_interval_wire(NativeMatteCoverage::LateStart, NativeMatteConsumer::Video);
    let layers = document["composition"]["layers"].as_array().unwrap();
    let consumer = layers
        .iter()
        .find(|layer| layer["trackMatte"].is_object())
        .unwrap();
    assert_eq!(consumer["type"], "Video");
    assert_eq!(
        consumer["playback"]["inputRange"],
        serde_json::json!({"start":500,"duration":1500})
    );
    assert_eq!(consumer["playback"]["inputOffsetMs"], 0);
    assert_eq!(
        consumer["playback"]["mapping"]["input"],
        serde_json::json!({"start":0,"duration":2000})
    );
    assert_eq!(
        consumer["playback"]["mapping"]["output"],
        serde_json::json!({"start":0,"duration":2000})
    );
    // LayerPlayback samples parent time directly, so the retained 500 ms
    // boundary still selects source time 500 ms; it never restarts at zero.
    let playback = &consumer["playback"];
    let parent_ms = 500_i64;
    let authored_ms = parent_ms + playback["inputOffsetMs"].as_i64().unwrap();
    let input = &playback["mapping"]["input"];
    let output = &playback["mapping"]["output"];
    let source_ms = output["start"].as_i64().unwrap()
        + (authored_ms - input["start"].as_i64().unwrap()) * output["duration"].as_i64().unwrap()
            / input["duration"].as_i64().unwrap();
    assert_eq!(source_ms, 500);
    let keys = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == consumer["id"]
                && entry["target"]["propertyType"] == "opacity"
        })
        .unwrap();
    assert_eq!(keys["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(keys["animator"]["keyframes"][1]["layerTime"], 1000);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("uncovered interval")),
        "{omissions:?}"
    );
}

#[test]
fn track_matte_provider_late_start_preserves_group_content_and_animation_clocks() {
    let (document, omissions) =
        native_matte_interval_wire(NativeMatteCoverage::LateStart, NativeMatteConsumer::Group);
    let consumer = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["trackMatte"].is_object())
        .unwrap();
    assert_eq!(consumer["type"], "Group");
    assert_eq!(
        consumer["playback"]["inputRange"],
        serde_json::json!({"start":500,"duration":1500})
    );
    assert_eq!(consumer["playback"]["inputOffsetMs"], 0);
    assert_eq!(
        consumer["playback"]["mapping"]["input"],
        serde_json::json!({"start":0,"duration":2000})
    );
    assert_eq!(
        consumer["playback"]["mapping"]["output"],
        serde_json::json!({"start":0,"duration":2000})
    );
    let keys = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == consumer["id"]
                && entry["target"]["propertyType"] == "opacity"
        })
        .unwrap();
    assert_eq!(keys["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(keys["animator"]["keyframes"][1]["layerTime"], 1000);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("uncovered interval")),
        "{omissions:?}"
    );
}

#[test]
fn track_matte_provider_late_start_rebases_rect_animation_clock() {
    let (document, omissions) =
        native_matte_interval_wire(NativeMatteCoverage::LateStart, NativeMatteConsumer::Rect);
    let consumer = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["trackMatte"].is_object())
        .unwrap();
    assert_eq!(consumer["type"], "Rect");
    assert_eq!(
        consumer["activeRange"],
        serde_json::json!({"start":500,"duration":1500})
    );
    let keys = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == consumer["id"]
                && entry["target"]["propertyType"] == "opacity"
        })
        .unwrap();
    assert_eq!(keys["animator"]["keyframes"][0]["layerTime"], -500);
    assert_eq!(keys["animator"]["keyframes"][1]["layerTime"], 500);
    assert_eq!(keys["animator"]["keyframes"][1]["value"]["value"], 50.0);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("uncovered interval")),
        "{omissions:?}"
    );
}

#[test]
fn track_matte_missing_coverage_publishes_no_orphaned_animation() {
    let (document, omissions) =
        native_matte_interval_wire(NativeMatteCoverage::Missing, NativeMatteConsumer::Video);
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert!(
        layers.iter().all(|layer| !layer["trackMatte"].is_object()),
        "{document:#}"
    );
    let layer_ids: std::collections::BTreeSet<_> = layers
        .iter()
        .filter_map(|layer| layer["id"].as_str())
        .collect();
    for entry in document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
    {
        if let Some(layer_id) = entry["target"]["layerId"].as_str() {
            assert!(layer_ids.contains(layer_id), "{entry:#}");
        }
    }
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("holds no clip over")),
        "{omissions:?}"
    );
    fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
}

#[test]
fn non_canvas_transform_matte_keeps_the_existing_source_size_rejection() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_transform_track_matte_26_5_strict.prproj");
    let original = crate::format::read_xml(&path).unwrap();
    // Both saved orders remain outside the Track Matte source-size boundary,
    // even when their matte carries an effect without a mapping.
    for chain in ["122", "124"] {
        let mut xml = original.clone();
        for id in ["77", chain] {
            let parsed = roxmltree::Document::parse(&xml).unwrap();
            let record = parsed
                .root_element()
                .children()
                .find(|node| node.attribute("ObjectID") == Some(id))
                .unwrap();
            let before = &xml[record.range()];
            let after = if id == "77" {
                before.replace("0,0,1920,1080", "0,0,1280,720")
            } else {
                before.replace("</ComponentChain>", "<Components Version=\"1\"><Component Index=\"0\" ObjectRef=\"900\"/></Components></ComponentChain>")
            };
            assert_ne!(before, after);
            let range = record.range();
            xml.replace_range(range, &after);
        }
        let end = xml.rfind("</").unwrap();
        xml.insert_str(end, "<VideoFilterComponent ObjectID=\"900\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"><Component Version=\"7\"><ID>99</ID><DisplayName>Own unsupported probe</DisplayName><Bypass>false</Bypass></Component><VideoFilterType>2</VideoFilterType><MatchName>Own.Unsupported.Probe</MatchName></VideoFilterComponent>");
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("18832324-570e-4e73-8460-84b8c8150813"))
                .unwrap();
        assert_eq!(
            omissions
                .iter()
                .filter(|omission| omission
                    .reason
                    .contains("Track Matte Key on media that is not sequence-sized"))
                .count(),
            2,
            "{chain}: {omissions:#?}"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains("Own.Unsupported.Probe")),
            "{omissions:#?}"
        );
        assert!(
            !omissions
                .iter()
                .any(|omission| omission.reason.contains("measured A4")),
            "{omissions:#?}"
        );
        assert!(!project.sequences[0]
            .video_tracks
            .iter()
            .flat_map(|track| &track.items)
            .filter_map(crate::schema::PrVideoItem::media)
            .any(|clip| clip.track_matte.is_some()));
    }
}

/// `food_promo` `VideoClipTrackItem:86` (Premiere 14.4): its Opacity
/// (`VideoFilterComponent:126`, 23 %, Normal) and Track Matte Key
/// (`:127`, Matte 3 = the track ID of `Index` 2, Matte Alpha), verbatim.
const FOOD_PROMO_OPACITY: &str = "<VideoFilterComponent ObjectID=\"126\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"148\"/><Param Index=\"1\" ObjectRef=\"149\"/><Param Index=\"2\" ObjectRef=\"150\"/></Params><ID>2</ID><DisplayName>Opacity</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Opacity</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"148\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>Opacity</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,23.,0,0,0,0,0,0</StartKeyframe><CurrentValue>23</CurrentValue><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"149\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>10</ParameterControlType><StartKeyframe>-91445760000000000,18,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>26</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"150\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>31</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>";
const FOOD_PROMO_TRACK_MATTE_KEY: &str = "<VideoFilterComponent ObjectID=\"127\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"151\"/><Param Index=\"1\" ObjectRef=\"152\"/><Param Index=\"2\" ObjectRef=\"153\"/></Params><ID>3</ID><DisplayName>Track Matte Key</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Legacy Key Track Matte</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"151\" ClassID=\"2f2eb0a3-318c-4a93-99fc-f1d319edc864\" Version=\"9\"><Name>Matte:</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4294967295</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"152\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Composite Using:</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"153\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name>Reverse</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>";

/// `corporate_slideshow` `VideoClipTrackItem:1176` (Premiere 12.x): its
/// Track Matte Key `VideoFilterComponent:1799`, `Version` 7 with `Component`
/// 5 and a `Node`, Matte 13 (the track ID of `Index` 12), Matte Luma, verbatim.
const CORPORATE_TRACK_MATTE_KEY: &str = "<VideoFilterComponent ObjectID=\"1799\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"><Component Version=\"5\"><Node Version=\"1\"><Properties Version=\"1\"><ECP.Filter.Expanded>false</ECP.Filter.Expanded></Properties></Node><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"2486\"/><Param Index=\"1\" ObjectRef=\"2487\"/><Param Index=\"2\" ObjectRef=\"2488\"/></Params><ID>3</ID><DisplayName>Track Matte Key</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Legacy Key Track Matte</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>\
<VideoComponentParam ObjectID=\"2486\" ClassID=\"2f2eb0a3-318c-4a93-99fc-f1d319edc864\" Version=\"9\"><Name>Matte:</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,13,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4294967295</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"2487\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Composite Using:</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,1,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>\
<VideoComponentParam ObjectID=\"2488\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name>Reverse</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>";

#[test]
fn track_matte_keys_read_their_matte_track_by_id_channel_and_chain_position() {
    // The matte track carries `Track/ID` 7 at `Index` 1, so the key's Matte 7
    // resolves to track 1, not to a seventh track.
    // Each corpus record names its own matte track ID; the matte track takes it.
    for (case, matte_track_id, components, channel, opacity, effects, above) in [
        // `horror_title` `VideoClipTrackItem:89`: Invert at Index 0 and the
        // key at Index 1, which applies first; the Invert follows it.
        (
            "horror_title, Invert after the key",
            7,
            vec![(20, invert(20)), (30, track_matte_key(30))],
            PrMatteChannel::Alpha,
            100.0,
            1,
            0,
        ),
        (
            "food_promo, Opacity 23 and the key",
            3,
            vec![
                (126, FOOD_PROMO_OPACITY.to_owned()),
                (127, FOOD_PROMO_TRACK_MATTE_KEY.to_owned()),
            ],
            PrMatteChannel::Alpha,
            23.0,
            0,
            0,
        ),
        (
            "corporate_slideshow 12.x record, Matte Luma",
            13,
            vec![(1799, CORPORATE_TRACK_MATTE_KEY.to_owned())],
            PrMatteChannel::Luma,
            100.0,
            0,
            0,
        ),
        // The blur at Index 1 applies before the key at Index 0.
        (
            "Gaussian Blur before the key",
            7,
            vec![(20, track_matte_key(20)), (30, blur(30))],
            PrMatteChannel::Alpha,
            100.0,
            1,
            1,
        ),
        // Fixture clip D: Reverse with Matte Alpha is one minus the alpha (G3a).
        (
            "Reverse with Matte Alpha",
            7,
            vec![(20, track_matte_key_xml(20, 7, 0, true))],
            PrMatteChannel::AlphaInverted,
            100.0,
            0,
            0,
        ),
    ] {
        let xml = with_matte_track(SOURCE, &components).replace(
            "<ID>7</ID><Index>1</Index>",
            &format!("<ID>{matte_track_id}</ID><Index>1</Index>"),
        );
        // An explicit Opacity component replaces the chain's default flag.
        let xml = if opacity == 100.0 {
            xml
        } else {
            xml.replacen("<DefaultOpacity>true</DefaultOpacity>", "", 1)
        };
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        assert!(
            omissions
                .iter()
                .all(|omission| omission.scope != OmissionScope::Occurrence),
            "{case}: {omissions:?}"
        );
        let tracks = &project.sequences[0].video_tracks;
        let clip = tracks[0].clip(0);
        assert_eq!(
            clip.track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel
            }),
            "{case}"
        );
        assert_eq!(
            (clip.effects.len(), clip.effects_above_mask, clip.opacity),
            (effects, above, opacity),
            "{case}"
        );
        let matte = tracks[1].clip(0);
        assert_eq!(matte.id.as_deref(), Some("VideoClipTrackItem:93"), "{case}");
        assert!(matte.enabled && matte.track_matte.is_none(), "{case}");
    }
}

#[test]
fn an_omitted_track_below_the_matte_track_does_not_shift_the_matte() {
    // Native tracks 0-3: the keyed clip, a track whose record is missing
    // (omitted whole), the matte track (`Track/ID` 7, `Index` 2, item 93)
    // and an unrelated track (ID 8, `Index` 3, item 95 over the same range).
    // The kept tracks are 0, 2 and 3, so Matte 7 is kept track 1: item 93,
    // not item 95 at native `Index` 2's position.
    let unrelated = Placement {
        start: 0,
        end: 5 * TICKS,
        source_in: 0,
    };
    let xml = with_matte_track(SOURCE, &[(20, track_matte_key(20))])
        .replace(
            "<Track Index=\"1\" ObjectURef=\"track-2\"/></Tracks>",
            "<Track Index=\"1\" ObjectURef=\"track-missing\"/><Track Index=\"2\" ObjectURef=\"track-2\"/><Track Index=\"3\" ObjectURef=\"track-4\"/></Tracks>",
        )
        .replace(
            "<Track><ID>7</ID><Index>1</Index></Track><ClipItems><Index>1</Index>",
            "<Track><ID>7</ID><Index>2</Index></Track><ClipItems><Index>2</Index>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                "<VideoClipTrack ObjectUID=\"track-4\"><ClipTrack><Track><ID>8</ID><Index>3</Index></Track><ClipItems><Index>3</Index><TrackItems><TrackItem ObjectRef=\"95\"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>{}</PremiereData>",
                placement_records(95, 7, &unrelated)
            ),
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(|omission| omission.scope)
            .collect::<Vec<_>>(),
        [OmissionScope::Track],
        "{omissions:?}"
    );
    let tracks = &project.sequences[0].video_tracks;
    let fill = tracks[0].clip(0);
    let matte = fill.track_matte.expect("the key converts");
    assert_eq!(matte.track_index, 1);
    assert_eq!(
        tracks[matte.track_index].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:93")
    );
    assert_eq!(tracks.len(), 3);
    assert_eq!(
        tracks[2].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:95")
    );
}

/// "Outer" places `one-clip.xml`'s sequence on V1 (item 110) with the key
/// (130), after `motion` in its chain when there is one, and its media source
/// on V2 (item 120), both over 0-2 s; the key names V2 by its `Track/ID` 2. A
/// clip at 3-4 s on V2 (item 125) keeps Outer convertible without them.
fn keyed_nest_xml(motion: Option<&str>) -> String {
    let placement = Placement {
        start: 0,
        end: 2 * TICKS,
        source_in: 0,
    };
    let key = track_matte_key_xml(130, 2, 1, false);
    let chain = match motion {
        Some(motion) => placement_chain(
            111,
            "<DefaultOpacity>true</DefaultOpacity>",
            &[(300, motion.to_owned()), (130, key)],
        ),
        None => placement_chain(
            111,
            "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity>",
            &[(130, key)],
        ),
    };
    let mut records = sequence_records("outer", "Outer", 100, &[110]).replace(
        "<Track ObjectURef=\"outer-track\"/></Tracks>",
        "<Track ObjectURef=\"outer-track\"/><Track Index=\"1\" ObjectURef=\"outer-track-2\"/></Tracks>",
    );
    records.push_str(&sequence_source(102, "sequence-1"));
    records.push_str(&placement_records(110, 102, &placement).replace(
        "<VideoComponentChain ObjectID=\"111\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
        &chain,
    ));
    records.push_str("<VideoClipTrack ObjectUID=\"outer-track-2\"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef=\"120\"/><TrackItem ObjectRef=\"125\"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>");
    records.push_str(&placement_records(120, 7, &placement));
    records.push_str(&placement_records(
        125,
        7,
        &Placement {
            start: 3 * TICKS,
            end: 4 * TICKS,
            source_in: 0,
        },
    ));
    with_records(SOURCE, &records)
}

fn empty_keyed_nest_xml() -> String {
    with_records(
        &keyed_nest_xml(None).replace(
            "<Sequence ObjectURef=\"sequence-1\"/>",
            "<Sequence ObjectURef=\"empty\"/>",
        ),
        &sequence_records("empty", "Empty", 200, &[]),
    )
}

#[test]
fn empty_native_nest_does_not_claim_its_matte_picture() {
    let xml = empty_keyed_nest_xml();
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 0);
    let pictures: Vec<_> = sequence.video_occurrences().collect();
    assert_eq!(pictures.len(), 2, "{omissions:?}");
    assert_eq!(pictures[0].id.as_deref(), Some("VideoClipTrackItem:120"));
    assert_eq!(pictures[0].timeline_ticks(), 0..2 * TICKS);
    assert!(pictures[0].track_matte.is_none());
    assert!(omissions
        .iter()
        .any(|item| item.reason.contains("empty native sequence")));
    let document = crate::tests::support::project_document_with_media(sequence, &project.media);
    let layers = document["composition"]["layers"].as_array().unwrap();
    let videos: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(videos.len(), 2);
    assert!(videos
        .iter()
        .all(|layer| layer["isHidden"] != true && layer.get("trackMatte").is_none()));
}

#[test]
fn empty_native_nest_with_disabled_consumer_retains_matte_concealment() {
    for (before, after) in [
        (
            "<VideoClipTrackItem ObjectID=\"110\"><ClipTrackItem>",
            "<VideoClipTrackItem ObjectID=\"110\"><ClipTrackItem><IsMuted>true</IsMuted>",
        ),
        (
            "<VideoClipTrack ObjectUID=\"outer-track\"><ClipTrack><Track><ID>1</ID>",
            "<VideoClipTrack ObjectUID=\"outer-track\"><ClipTrack><Track><ID>1</ID><IsMuted>true</IsMuted>",
        ),
    ] {
        let xml = empty_keyed_nest_xml().replace(before, after);
        let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
        let pictures: Vec<_> = project.single_sequence().unwrap().video_occurrences().collect();
        assert_eq!(pictures.len(), 1, "{omissions:?}");
        assert_eq!(pictures[0].id.as_deref(), Some("VideoClipTrackItem:125"));
        assert!(omissions.iter().any(|item| item.reason.contains("matte source of the omitted clip")));
    }
}

#[test]
fn empty_native_nest_with_group_processing_retains_matte_concealment() {
    let xml = empty_keyed_nest_xml().replace(
        "<VideoComponentChain ObjectID=\"201\"><ComponentChain/></VideoComponentChain>",
        "<VideoComponentChain ObjectID=\"201\"><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"130\"/></Components></ComponentChain></VideoComponentChain>",
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    let pictures: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    assert_eq!(pictures.len(), 1);
    assert_eq!(pictures[0].id.as_deref(), Some("VideoClipTrackItem:125"));
    assert!(omissions
        .iter()
        .any(|item| item.reason.contains("matte source of the omitted clip")));
}

#[test]
fn nested_geometry2_stack_keeps_curved_position_tint_picture_and_matte_consumers() {
    const CURVED_POSITION_WARNING: &str = "Transform Position curved spatial path retains editable tangents but FX traverses parametrically rather than native constant-speed distance";

    // The host is the existing native-derived nested Geometry2 fixture. The
    // Position control is a publishable native-derived control that uses the
    // saved curved-key form established by the pinned source-effects fixture;
    // it is not an independently Adobe-rendered oracle.
    let native = include_str!("../../../tests/fixtures/nested-transform-geometry2.xml");
    let dom = roxmltree::Document::parse(native).unwrap();
    let first: String = dom
        .root_element()
        .children()
        .filter(|node| node.is_element())
        .map(|node| &native[node.range()])
        .collect();
    const STATIC_POSITION: &str = "<Name>Position</Name>\n\t\t<StartKeyframe>-91445760000000000,0.3645833432674408:1.6666666269302368,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>";
    let curved_position = format!(
        "<Name>Position</Name>\n\t\t<IsTimeVarying>true</IsTimeVarying>\n\t\t<StartKeyframe>-91445760000000000,0.3645833432674408:1.6666666269302368,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>\n\t\t<Keyframes>0,0.25:0.5,0,0,0,0.16666666666666666,0.1,0.16666666666666666,5,4,0,0,0.05,0.04;{TICKS},0.5:0.25,0,0,0,0.16666666666666666,0.1,0.16666666666666666,5,4,-0.04,-0.03,0.03,0.05;{},0.75:0.5,0,0,0,0.16666666666666666,0.1,0.16666666666666666,5,4,-0.05,0.02,0,0;</Keyframes>",
        2 * TICKS
    );
    assert_eq!(first.matches(STATIC_POSITION).count(), 1);
    let first = first.replace(STATIC_POSITION, &curved_position);
    let mut second = first.clone();
    for id in (407..=840).rev() {
        second = second
            .replace(
                &format!("ObjectID=\"{id}\""),
                &format!("ObjectID=\"{}\"", id + 1000),
            )
            .replace(
                &format!("ObjectRef=\"{id}\""),
                &format!("ObjectRef=\"{}\"", id + 1000),
            );
    }
    second = second
        .replace("0.3645833432674408:1.6666666269302368", "0.5:0.5")
        .replace("762048000000,-90.", "762048000000,45.");
    let tint = tint(3000).replace("<Name>Amount to Tint</Name><IsTimeVarying>false</IsTimeVarying>", "<Name>Amount to Tint</Name><IsTimeVarying>true</IsTimeVarying>").replace("<StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>3</ParameterID>",
        &format!("<StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><Keyframes>{TICKS},25.,0,0,0,0,0,0;{},75.,0,0,0,0,0,0;</Keyframes><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>3</ParameterID>", 3 * TICKS));
    for (components, expected, expected_report_suffixes) in [
        (
            vec![
                (407, first.clone()),
                (1407, second.clone()),
                (3000, tint.clone()),
            ],
            vec!["tint", "second", "first"],
            vec!["; retained nested effect 2", "; retained nested effect 3"],
        ),
        (
            vec![(3000, tint.clone()), (407, first.clone())],
            vec!["first", "tint"],
            vec!["; retained nested effect 1"],
        ),
    ] {
        let provider = placement_records(
            93,
            999,
            &Placement {
                start: 0,
                end: 5 * TICKS,
                source_in: 0,
            },
        );
        let provider = provider.replace(r#"<VideoComponentChain ObjectID="94"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#,
            &placement_chain(94, "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity>", &components));
        let track = r#"<VideoClipTrack ObjectUID="track-2"><ClipTrack><Track><ID>7</ID><Index>1</Index></Track><ClipItems><TrackItems><TrackItem ObjectRef="93"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>"#;
        let child = format!(
            r#"<VideoClipTrackItem ObjectID="120"><ClipTrackItem><ComponentOwner><Components ObjectRef="121"/></ComponentOwner><TrackItem><End>{}</End></TrackItem><SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem><VideoComponentChain ObjectID="121"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>"#,
            5 * TICKS
        );
        let xml = with_matte_track(SOURCE, &[(500, track_matte_key(500))]).replace(
            MATTE_TRACK,
            &format!(
                "{track}{provider}{}{}{child}",
                sequence_source(999, "provider"),
                sequence_records("provider", "Provider picture", 200, &[120])
            ),
        );
        let (project, mut notes) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let outer = project.single_sequence().unwrap();
        let provider = outer.video_tracks[1]
            .nests
            .first()
            .unwrap_or_else(|| panic!("provider and consumer retained: {notes:?}"));
        assert_eq!(provider.id.as_deref(), Some("VideoClipTrackItem:93"));
        assert_eq!(provider.effects.len(), expected.len());
        assert_eq!(provider.sequence.video_occurrences().count(), 1);
        assert!(outer.video_tracks[0].clip(0).track_matte.is_some());
        assert!(
            notes
                .iter()
                .all(|note| note.scope != OmissionScope::Occurrence),
            "{notes:?}"
        );
        let ids = crate::tesseract_output::asset_ids_in_order(outer, &project.media);
        let document =
            crate::convert::premiere_to_tesseract(outer, &project.media, &ids, &mut notes)
                .unwrap()
                .to_json_value()
                .unwrap();
        let curved_position_report_suffixes = notes
            .iter()
            .filter(|note| note.kind == crate::OmissionKind::Approximated)
            .filter_map(|note| note.reason.strip_prefix(CURVED_POSITION_WARNING))
            .collect::<Vec<_>>();
        assert_eq!(
            curved_position_report_suffixes, expected_report_suffixes,
            "{notes:?}"
        );
        let roots = document["composition"]["layers"].as_array().unwrap();
        let consumer = roots
            .iter()
            .find(|layer| !layer["trackMatte"].is_null())
            .unwrap();
        let provider = roots
            .iter()
            .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(consumer["trackMatte"]["mode"], "alpha");
        let mut stage = &provider["layers"][0];
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        for kind in expected.iter().rev() {
            match *kind {
                "tint" => {
                    assert_eq!(stage["effects"][0]["effect"]["type"], "tintTritone");
                    let entry = entries
                        .iter()
                        .find(|entry| entry["target"]["effectId"] == stage["effects"][0]["id"])
                        .unwrap();
                    assert_eq!(entry["target"]["paramName"], "amount");
                    assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 1000);
                    assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 3000);
                }
                affine => {
                    let entry = entries
                        .iter()
                        .find(|entry| {
                            entry["target"]["layerId"] == stage["id"]
                                && entry["target"]["propertyType"] == "rotation"
                        })
                        .unwrap();
                    assert_eq!(
                        entry["animator"]["keyframes"][1]["value"]["value"],
                        if affine == "first" { -90.0 } else { 45.0 }
                    );
                    assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 1000);
                    assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 3000);
                    for (property, values, incoming, outgoing) in [
                        (
                            "positionX",
                            [480.0, 960.0, 1440.0],
                            [None, Some(-76.8), Some(-96.0)],
                            [Some(96.0), Some(57.6), None],
                        ),
                        (
                            "positionY",
                            [540.0, 270.0, 540.0],
                            [None, Some(-32.4), Some(21.6)],
                            [Some(43.2), Some(54.0), None],
                        ),
                    ] {
                        let entry = entries
                            .iter()
                            .find(|entry| {
                                entry["target"]["layerId"] == stage["id"]
                                    && entry["target"]["propertyType"] == property
                            })
                            .unwrap();
                        let keys = entry["animator"]["keyframes"].as_array().unwrap();
                        assert_eq!(keys.len(), 3);
                        for (index, key) in keys.iter().enumerate() {
                            assert_eq!(key["layerTime"], i64::try_from(index).unwrap() * 1000);
                            assert_eq!(key["value"]["value"], values[index]);
                            for (actual, expected) in [
                                (
                                    key.get("spatialInTangent").and_then(|value| value.as_f64()),
                                    incoming[index],
                                ),
                                (
                                    key.get("spatialOutTangent")
                                        .and_then(|value| value.as_f64()),
                                    outgoing[index],
                                ),
                            ] {
                                match (actual, expected) {
                                    (Some(actual), Some(expected)) => {
                                        assert!((actual - expected).abs() < 1e-9)
                                    }
                                    (None, None) => {}
                                    pair => panic!("unpaired spatial tangent {pair:?}"),
                                }
                            }
                        }
                    }
                }
            }
            if !stage["masks"].is_null() && !stage["masks"].as_array().unwrap().is_empty() {
                break;
            }
            stage = &stage["layers"][0];
        }
        let guide = stage["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == stage["masks"][0]["layer"])
            .unwrap();
        assert_eq!(guide["rect"]["size"], serde_json::json!([1920.0, 1080.0]));
        assert!(stage["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["type"] == "Video"));
    }
}

#[test]
fn a_nested_track_matte_keeps_its_base_without_optional_posterize_time() {
    let effect = r#"<VideoFilterComponent ObjectID="300"><Component><Params><Param Index="0" ObjectRef="301"/></Params><ID>3</ID><DisplayName>Posterize Time</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>AE.ADBE Posterize Time</MatchName></VideoFilterComponent><VideoComponentParam ObjectID="301"><Name>Frame Rate</Name><ParameterControlType>2</ParameterControlType><StartKeyframe>0,8.,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>"#;
    for components in [
        r#"<Component Index="0" ObjectRef="130"/><Component Index="1" ObjectRef="300"/>"#,
        r#"<Component Index="0" ObjectRef="300"/><Component Index="1" ObjectRef="130"/>"#,
    ] {
        let xml =
            keyed_nest_xml(None).replace(r#"<Component Index="0" ObjectRef="130"/>"#, components);
        let xml = with_records(&xml, effect);
        let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
        assert!(
            omissions
                .iter()
                .all(|item| item.scope != OmissionScope::Occurrence),
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == OmissionScope::Feature
                    && item.reason.contains("Posterize Time")),
            "{omissions:?}"
        );
        let outer = &project.sequences[0];
        let nest = outer.nest_occurrences().next().unwrap();
        assert_eq!(nest.id.as_deref(), Some("VideoClipTrackItem:110"));
        assert_eq!(nest.timeline_ticks(), 0..2 * TICKS);
        assert_eq!(
            nest.track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Luma
            })
        );
        assert!(nest.effects.is_empty());
        assert_eq!(nest.sequence.video_occurrences().count(), 1);
        assert_eq!(
            outer.video_tracks[1].clip(0).id.as_deref(),
            Some("VideoClipTrackItem:120")
        );
    }
}

#[test]
fn a_masked_nest_keeps_mapped_effect_keys_children_and_concealed_provider() {
    let blur = blur(400).replace(
        "<IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,25.,0,0,0,0,0,0</StartKeyframe>",
        &format!("<IsTimeVarying>true</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,25.,0,0,0,0,0,0</StartKeyframe><Keyframes>0,25.,0,0,0,0,0,0;{TICKS},50.,0,0,0,0,0,0;</Keyframes>"),
    );
    let unknown = r#"<VideoFilterComponent ObjectID="500"><Component><DisplayName>Unknown</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic></Component><MatchName>Own.Unsupported.Detail</MatchName></VideoFilterComponent>"#;
    for components in [
        r#"<Component Index="0" ObjectRef="130"/><Component Index="1" ObjectRef="400"/><Component Index="2" ObjectRef="500"/>"#,
        r#"<Component Index="0" ObjectRef="500"/><Component Index="1" ObjectRef="400"/><Component Index="2" ObjectRef="130"/>"#,
    ] {
        let xml =
            keyed_nest_xml(None).replace(r#"<Component Index="0" ObjectRef="130"/>"#, components);
        let xml = with_records(&xml, &format!("{blur}{unknown}"));
        let (project, mut omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
        let outer = &project.sequences[0];
        let nest = outer
            .nest_occurrences()
            .next()
            .unwrap_or_else(|| panic!("masked base retained: {omissions:?}"));
        assert_eq!(nest.id.as_deref(), Some("VideoClipTrackItem:110"));
        assert_eq!(nest.timeline_ticks(), 0..2 * TICKS);
        assert_eq!(
            nest.track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Luma
            })
        );
        assert_eq!(nest.sequence.video_occurrences().count(), 1);
        assert_eq!(nest.effects.len(), 1);
        assert_eq!(nest.effects[0].animations.len(), 1);
        assert_eq!(
            outer.video_tracks[1].items.len(),
            2,
            "provider and independent sibling"
        );
        assert!(
            omissions
                .iter()
                .all(|note| note.scope != OmissionScope::Occurrence),
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.record == "VideoFilterComponent:500"
                    && note.reason.contains("Own.Unsupported.Detail")),
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.record == "VideoClipTrackItem:110"
                    && note.reason.contains("before outer coverage")),
            "{omissions:?}"
        );
        let ids = crate::tesseract_output::asset_ids_in_order(outer, &project.media);
        let document =
            crate::convert::premiere_to_tesseract(outer, &project.media, &ids, &mut omissions)
                .unwrap()
                .to_json_value()
                .unwrap();
        let roots = document["composition"]["layers"].as_array().unwrap();
        let consumer = roots
            .iter()
            .find(|layer| !layer["trackMatte"].is_null())
            .unwrap();
        assert_eq!(consumer["trackMatte"]["mode"], "luma");
        let provider = roots
            .iter()
            .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(provider["type"], "Video");
        assert_eq!(
            provider["playback"]["inputRange"],
            serde_json::json!({"start": 0, "duration": 2000})
        );
        assert_eq!(
            roots
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .count(),
            2
        );
        let stage = &consumer["layers"][0];
        assert_eq!(stage["effects"][0]["effect"]["type"], "gaussianBlur");
        assert_eq!(stage["effects"][0]["effect"]["blurriness"], 25.0);
        assert!(stage["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["type"] == "Video"));
        let guide = stage["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == stage["masks"][0]["layer"])
            .unwrap();
        assert_eq!(guide["rect"]["size"], serde_json::json!([1920.0, 1080.0]));
        let entry = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["effectId"] == stage["effects"][0]["id"])
            .unwrap();
        assert_eq!(entry["target"]["paramName"], "blurriness");
        assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 0);
        assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 1000);
        assert_eq!(entry["animator"]["keyframes"][1]["value"]["value"], 50.0);
    }
}

#[test]
fn fixed_pivot_scale_keeps_nested_picture_and_moves_its_matte() {
    let keys = format!("<Keyframes>0,50.,0,0,0,0,0,0;{TICKS},100.,0,0,0,0,0,0;</Keyframes>");
    // Native Track Matte saves 50% Scale; native static Motion saves Anchor 0.25:0.75.
    let scale_keys = [("Scale", keys.as_str())];
    for (pivot, pixels, animated, start) in [
        ("0.5:0.5", [960.0, 540.0], false, 0),
        ("0.5:0.5", [960.0, 540.0], true, TICKS),
        ("0.25:0.75", [480.0, 810.0], false, 0),
        ("0.25:0.75", [480.0, 810.0], true, TICKS),
        ("0.5:1", [960.0, 1080.0], true, 0),
        ("1:0.5", [1920.0, 540.0], true, 0),
    ] {
        let motion = corpus_motion_component(
            300,
            &[
                ("Scale", "50."),
                ("Scale Width", "50."),
                ("Position", pivot),
                ("Anchor Point", pivot),
            ],
            if animated { &scale_keys } else { &[] },
        );
        let mut xml = keyed_nest_xml(Some(&motion));
        if start != 0 {
            xml = xml.replace(
                &format!("<Start>0</Start><End>{}</End>", 2 * TICKS),
                &format!("<Start>{start}</Start><End>{}</End>", start + 2 * TICKS),
            );
        }
        let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
        let outer = project.single_sequence().unwrap();
        assert_eq!(outer.nest_occurrences().count(), 1, "{omissions:?}");
        let document = crate::tests::support::project_document_with_media(outer, &project.media);
        let roots = document["composition"]["layers"].as_array().unwrap();
        let consumer = roots.iter().find(|layer| layer["type"] == "Group").unwrap();
        assert_eq!(
            consumer["transform"]["scale"],
            serde_json::json!([50.0, 50.0])
        );
        assert_eq!(consumer["transform"]["position"], serde_json::json!(pixels));
        assert_eq!(
            consumer["transform"]["anchorPoint"],
            serde_json::json!(pixels)
        );
        assert_eq!(consumer["trackMatte"]["mode"], "luma");
        assert_eq!(
            consumer["playback"]["inputRange"]["start"],
            start / (TICKS / 1000)
        );
        let children = consumer["layers"].as_array().unwrap();
        let provider = children
            .iter()
            .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(provider["type"], "Video");
        assert_eq!(provider["parent"], consumer["id"]);
        assert_eq!(
            provider["playback"]["inputRange"],
            serde_json::json!({"start": 0, "duration": 2000})
        );
        assert_eq!(
            children
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .count(),
            2
        );
        assert_eq!(
            roots
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .count(),
            1
        );
        if animated {
            let entries = document["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap();
            for property in ["scaleX", "scaleY"] {
                let entry = entries
                    .iter()
                    .find(|entry| {
                        entry["target"]["layerId"] == consumer["id"]
                            && entry["target"]["propertyType"] == property
                    })
                    .unwrap();
                let keys = entry["animator"]["keyframes"].as_array().unwrap();
                assert_eq!(keys.len(), 2);
                for (key, (time, value)) in keys.iter().zip([(0, 50.0), (1000, 100.0)]) {
                    assert_eq!(key["layerTime"], time);
                    assert_eq!(key["value"]["value"], value);
                }
            }
        }
    }
}

#[test]
fn translated_nested_matte_keeps_picture_coverage_and_static_pivots() {
    for values in [
        [
            ("Scale", "50."),
            ("Position", "0.5:0.5"),
            ("Anchor Point", "0.25:0.75"),
        ],
        [
            ("Scale", "50."),
            ("Position", "0.25:0.75"),
            ("Anchor Point", "0.5:0.5"),
        ],
    ] {
        let motion = corpus_motion_component(300, &values, &[]);
        let (project, omissions) =
            inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
        let outer = project.single_sequence().unwrap();
        assert_eq!(outer.nest_occurrences().count(), 1, "{omissions:?}");
        let document = crate::tests::support::project_document_with_media(outer, &project.media);
        let roots = document["composition"]["layers"].as_array().unwrap();
        let consumer = roots.iter().find(|layer| layer["type"] == "Group").unwrap();
        let (position, anchor) = if values[1].1 == "0.5:0.5" {
            ([960.0, 540.0], [480.0, 810.0])
        } else {
            ([480.0, 810.0], [960.0, 540.0])
        };
        assert_eq!(
            consumer["transform"]["position"],
            serde_json::json!(position)
        );
        assert_eq!(
            consumer["transform"]["anchorPoint"],
            serde_json::json!(anchor)
        );
        assert_eq!(
            consumer["transform"]["scale"],
            serde_json::json!([50.0, 50.0])
        );
        let children = consumer["layers"].as_array().unwrap();
        let provider = children
            .iter()
            .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(provider["parent"], consumer["id"]);
        assert_eq!(provider["type"], "Video");
        assert_eq!(consumer["trackMatte"]["mode"], "luma");
        assert_eq!(
            children
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .count(),
            2
        );
        assert_eq!(
            roots
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .count(),
            1
        );
    }
}

#[test]
fn translated_nested_matte_retains_an_editable_solid_provider() {
    let motion = corpus_motion_component(300, &[("Scale", "50."), ("Position", "0.25:0.75")], &[]);
    let (mut project, mut omissions) =
        inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
    for media in project.media.values_mut() {
        if let Some(video) = &mut media.video {
            video.kind = PrMediaKind::ColorMatte(PrColorMatte { rgb: [12, 34, 56] });
        }
    }
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    let consumer = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap_or_else(|| panic!("{omissions:?}"));
    let provider = consumer["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
        .unwrap();
    assert_eq!(provider["type"], "Rect");
    assert_eq!(provider["parent"], consumer["id"]);
    assert_eq!(
        consumer["transform"]["position"],
        serde_json::json!([480.0, 810.0])
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn translated_short_canvas_matte_preserves_public_import_and_provider_tail() {
    let motion = corpus_motion_component(
        300,
        &[
            ("Scale", "50."),
            ("Position", "0.25:0.75"),
            ("Anchor Point", "0.5:0.5"),
        ],
        &[],
    );
    short_canvas_matte_preserves_public_import_and_provider_tail(
        Some(&motion),
        [480.0, 810.0],
        50.0,
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn default_motion_short_canvas_matte_preserves_public_import_and_provider_tail() {
    short_canvas_matte_preserves_public_import_and_provider_tail(None, [960.0, 540.0], 100.0);
}

#[cfg(feature = "ffmpeg-library")]
fn short_canvas_matte_preserves_public_import_and_provider_tail(
    motion: Option<&str>,
    position: [f64; 2],
    scale: f64,
) {
    let mut xml = keyed_nest_xml(motion);
    let canvas = "</TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef=\"2\"/>";
    assert_eq!(xml.matches(canvas).count(), 1);
    xml = xml.replace(canvas, &canvas.replace("1920,1080", "1920,200"));
    let occurrence_frame =
        "<SubClip ObjectRef=\"5\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect>";
    assert_eq!(xml.matches(occurrence_frame).count(), 1);
    xml = xml.replace(
        occurrence_frame,
        &occurrence_frame.replace("1920,1080", "1920,200"),
    );
    let original = placement_records(
        120,
        7,
        &Placement {
            start: 0,
            end: 2 * TICKS,
            source_in: 0,
        },
    );
    let extended = placement_records(
        120,
        7,
        &Placement {
            start: 0,
            end: 3 * TICKS,
            source_in: 0,
        },
    );
    assert_eq!(xml.matches(&original).count(), 1);
    xml = xml.replace(&original, &extended);
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("media")).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/feature_timecoded_source.mp4"),
        directory.path().join("media/source.mp4"),
    )
    .unwrap();
    let source = directory.path().join("source.prproj");
    std::fs::write(&source, xml).unwrap();
    let output = directory.path().join("converted");
    let omissions = crate::premiere_to_tesseract(&source, &output, Some("outer"), false).unwrap();
    let file = tesseract_file::TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document = file.project_json().unwrap();
    let roots = document["composition"]["layers"].as_array().unwrap();
    let consumer = roots
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap_or_else(|| panic!("{omissions:?}"));
    assert_eq!(
        consumer["transform"]["position"],
        serde_json::json!(position)
    );
    assert_eq!(
        consumer["transform"]["anchorPoint"],
        serde_json::json!([960.0, 100.0])
    );
    assert_eq!(
        consumer["transform"]["scale"],
        serde_json::json!([scale, scale])
    );
    let children = consumer["layers"].as_array().unwrap();
    let matte = children
        .iter()
        .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
        .unwrap();
    assert_eq!(matte["type"], "Group");
    assert_eq!(matte["parent"], consumer["id"]);
    assert_eq!(
        matte["transform"]["position"],
        serde_json::json!([0.0, 0.0])
    );
    assert_eq!(matte["transform"]["scale"][0], 100.0);
    assert!(
        (matte["transform"]["scale"][1].as_f64().unwrap() - 100.0 * 200.0 / 1080.0).abs() < 1e-12
    );
    let provider = &matte["layers"][0];
    assert_eq!(provider["type"], "Video");
    assert_eq!(provider["parent"], matte["id"]);
    assert_eq!(
        provider["playback"]["inputRange"],
        serde_json::json!({"start": 0, "duration": 2000})
    );
    let tails: Vec<_> = roots
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(tails.len(), 2);
    let tail = tails
        .iter()
        .find(|layer| layer["playback"]["inputRange"]["start"] == 2000)
        .unwrap();
    assert_eq!(tail["playback"]["inputRange"]["duration"], 1000);
    assert_eq!(
        tail["playback"]["mapping"]["output"],
        serde_json::json!({"start": 2000, "duration": 1000})
    );
    assert_eq!(provider["source"]["assetId"], tail["source"]["assetId"]);
    assert_eq!(
        children
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
}

#[test]
fn scaled_nested_matte_keeps_unsafe_provider_omissions() {
    let motion = corpus_motion_component(300, &[("Scale", "50.")], &[]);
    let original = placement_records(
        120,
        7,
        &Placement {
            start: 0,
            end: 2 * TICKS,
            source_in: 0,
        },
    );
    let retimed = original.replace(
        &format!("<VideoClip ObjectID=\"123\"><Clip><Source ObjectRef=\"7\"/><InPoint>0</InPoint><OutPoint>{}</OutPoint>", 2 * TICKS),
        &format!("<VideoClip ObjectID=\"123\"><Clip><Source ObjectRef=\"7\"/><PlaybackSpeed>0.5</PlaybackSpeed><InPoint>0</InPoint><OutPoint>{TICKS}</OutPoint>"),
    );
    assert_ne!(retimed, original);
    let cropped = original.replace(
        "<VideoComponentChain ObjectID=\"121\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
        &placement_chain(121, "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity>", &[(500, top_crop(500))]),
    );
    assert_ne!(cropped, original);
    for (replacement, reason, nests) in [
        (retimed, "independent linear Video or Group provider", 1),
        (cropped, "the matte clip has its own Crop", 0),
    ] {
        let xml = keyed_nest_xml(Some(&motion)).replace(&original, &replacement);
        let (project, mut omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
        let outer = project.single_sequence().unwrap();
        assert_eq!(outer.nest_occurrences().count(), nests, "{omissions:?}");
        let ids = crate::tesseract_output::asset_ids_in_order(outer, &project.media);
        let document =
            crate::convert::premiere_to_tesseract(outer, &project.media, &ids, &mut omissions)
                .unwrap()
                .to_json_value()
                .unwrap();
        assert!(
            omissions.iter().any(|note| note.reason.contains(reason)),
            "{omissions:?}"
        );
        let roots = document["composition"]["layers"].as_array().unwrap();
        assert!(roots.iter().all(|layer| layer["type"] != "Group"));
        assert_eq!(
            roots
                .iter()
                .filter(|layer| layer["type"] == "Video")
                .count(),
            1
        );
    }
}

#[test]
fn neutral_position_keys_keep_nested_track_matte_picture() {
    let keys = format!(
        "<Keyframes>0,0.5:0.5,0,0,0,0.16666666666666666,0,0.16666666666666666,0,2,0,0,0,0;{TICKS},0.5:0.5,5,0,0,0.33333333333333331,0,0.16666666666666666,0,4,0,0,0,0;</Keyframes>"
    );
    let motion = corpus_motion_component(300, &[], &[("Position", &keys)]);
    let (project, omissions) =
        inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.nest_occurrences().count(), 1, "{omissions:?}");
    let document = crate::tests::support::project_document_with_media(outer, &project.media);
    let roots = document["composition"]["layers"].as_array().unwrap();
    let consumer = roots.iter().find(|layer| layer["type"] == "Group").unwrap();
    assert_eq!(consumer["trackMatte"]["mode"], "luma");
    let provider = roots
        .iter()
        .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
        .unwrap();
    assert_eq!(provider["type"], "Video");
    assert!(consumer["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Video"));
    assert_eq!(
        consumer["transform"]["position"],
        serde_json::json!([960.0, 540.0])
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (property, expected) in [("positionX", 960.0), ("positionY", 540.0)] {
        let entry = entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == consumer["id"]
                    && entry["target"]["propertyType"] == property
            })
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(key["layerTime"], index * 1000);
            assert_eq!(key["value"]["value"], expected);
        }
    }

    for changed in [
        keys.replace(&format!("{TICKS},0.5:0.5"), &format!("{TICKS},0.6:0.5")),
        keys.replace("0,4,0,0,0,0;", "5,4,0.1,0,0,0;"),
    ] {
        let motion = corpus_motion_component(300, &[], &[("Position", &changed)]);
        let (project, _) =
            inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .nest_occurrences()
                .count(),
            0
        );
    }
}

#[test]
fn a_nested_placement_keeps_its_track_matte_key() {
    let xml = keyed_nest_xml(None);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let outer = &project.sequences[0];
    let nest = outer.nest_occurrences().next().expect("the nest");
    assert_eq!(
        (nest.id.as_deref(), nest.track_matte),
        (
            Some("VideoClipTrackItem:110"),
            Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Luma,
            })
        )
    );
    assert_eq!(
        outer.video_tracks[1].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:120")
    );

    // Its Opacity fades the keyed picture: Opacity and the key both scale its
    // alpha, so in either order, and the nest keeps both.
    let chain = super::nested::opacity_chain(111, 410, 60.0, "").replace(
        "<Component Index=\"0\" ObjectRef=\"410\"/>",
        "<Component Index=\"0\" ObjectRef=\"410\"/><Component Index=\"1\" ObjectRef=\"130\"/>",
    );
    let start = xml.find(r#"<VideoComponentChain ObjectID="111""#).unwrap();
    let end = start
        + xml[start..].find("</VideoComponentChain>").unwrap()
        + "</VideoComponentChain>".len();
    let mut faded = xml.clone();
    faded.replace_range(start..end, &chain);
    assert_ne!(faded, xml);
    let (project, omissions) = inspect_project_with_omissions(&faded, Some("outer")).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let faded_sequence = &project.sequences[0];
    let nest = faded_sequence.nest_occurrences().next().expect("the nest");
    assert_eq!((nest.opacity, nest.track_matte.is_some()), (60.0, true));

    // Centred Scale keys retain both picture and matte for common staging.
    let motion = super::nested::keyed_motion(300);
    let (project, omissions) =
        inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = &project.sequences[0];
    assert_eq!(outer.nest_occurrences().count(), 1);
    let kept: Vec<_> = outer
        .video_items()
        .filter_map(crate::schema::PrVideoItem::id)
        .collect();
    assert_eq!(kept, ["VideoClipTrackItem:120", "VideoClipTrackItem:125"]);

    // Another canvas retains its source-frame matte owner at default Motion.
    let sized = xml
        .replacen(
            r#"</TrackGroup><FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#,
            r#"</TrackGroup><FrameRect>0,0,1080,1920</FrameRect><ComponentOwner><Components ObjectRef="2"/>"#,
            1,
        )
        .replacen(
            r#"<SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect>"#,
            r#"<SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1080,1920</FrameRect>"#,
            1,
        );
    assert_ne!(sized, xml);
    let mut omissions = Vec::new();
    let read = crate::format::reader::read_sequence(
        &Graph::parse(&sized).unwrap(),
        Some("outer"),
        &std::collections::BTreeSet::new(),
        &mut std::collections::BTreeMap::new(),
        &mut omissions,
    );
    let sequence = read.unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 1);
    assert_eq!(sequence.video_occurrences().count(), 2);
    assert!(omissions.is_empty(), "{omissions:?}");
}

/// Rotated matte-keyed Motion stays outside the admitted Scale form;
/// its otherwise hidden provider must not become ordinary visible content.
#[test]
fn a_keyed_nest_with_static_motion_is_omitted_with_its_matte_clip() {
    let motion = corpus_motion_component(300, MOTION, &[]);
    let (project, omissions) =
        inspect_project_with_omissions(&keyed_nest_xml(Some(&motion)), Some("outer")).unwrap();
    let reported: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
        .collect();
    assert_eq!(
        reported,
        [
            (
                "110",
                "unsupported conversion: VideoClipTrackItem:110: Motion with a Track Matte Key on a nested sequence occurrence is not converted"
            ),
            (
                "VideoClipTrackItem:120",
                "matte source of the omitted clip VideoClipTrackItem:110 was not converted: Premiere does not draw a track-matte source"
            ),
        ]
    );
    let outer = &project.sequences[0];
    assert_eq!(outer.nest_occurrences().count(), 0);
    let kept: Vec<_> = outer
        .video_items()
        .filter_map(crate::schema::PrVideoItem::id)
        .collect();
    assert_eq!(kept, ["VideoClipTrackItem:125"]);
}

/// The matte names the rendered nested picture, including its Motion. Its
/// editable group clips to the source canvas before applying that Motion.
#[test]
fn a_moved_nest_keeps_its_track_matte_source_identity() {
    let keyed = PrVideoOccurrence {
        track_matte: Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        }),
        ..clip_of("source", 0..2 * TICKS, 0)
    };
    // Rotation keys from the default 0 degrees: the static Motion stays the
    // default, but the keys move the nest.
    let rotation = PrPropertyAnimation::Rotation(
        [(0, 0.0), (TICKS, 90.0)]
            .map(|(source_ticks, value)| PrScalarKeyframe {
                source_ticks,
                value,
                easing: PrKeyframeEasing::Linear,
            })
            .to_vec(),
    );
    for (transform, animations) in [
        (PrStaticTransform::default(), Vec::new()),
        (read_motion(), Vec::new()),
        (PrStaticTransform::default(), vec![rotation]),
    ] {
        let keys = animations.len();
        let matte = PrNestOccurrence {
            transform,
            animations,
            ..nest_of(sequence_of("Titles", Vec::new()), 0..2 * TICKS, 0)
        };
        let tracks = [
            PrVideoTrack::media([keyed.clone()]),
            PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests: vec![matte],
            },
        ];
        assert_eq!(
            check_track_matte(&tracks, 0, 0..2 * TICKS, keyed.track_matte.unwrap()),
            Ok(()),
            "{transform:?}, {keys} keyed properties"
        );
    }
}

/// The matte track of [`MATTE_TRACK`] holding `items` instead: each an item
/// `id` over `start..end` ticks playing source 7 from 0, with the records of
/// `placement_records`.
fn matte_track_of(items: &[(u32, i64, i64)]) -> String {
    let references: String = items
        .iter()
        .map(|(id, _, _)| format!("<TrackItem ObjectRef=\"{id}\"/>"))
        .collect();
    let mut records = format!(
        "<VideoClipTrack ObjectUID=\"track-2\"><ClipTrack><Track><ID>7</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems>{references}</TrackItems></ClipItems></ClipTrack></VideoClipTrack>"
    );
    for &(id, start, end) in items {
        records.push_str(&placement_records(
            id,
            7,
            &Placement {
                start,
                end,
                source_in: 0,
            },
        ));
    }
    records
}

#[test]
fn track_matte_keys_outside_the_supported_form_omit_the_occurrence() {
    let key = |id| track_matte_key(id);
    let keyed_matte = key(20).replace(
        "<IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,7,0,0,0,0,0,0</StartKeyframe>",
        "<IsTimeVarying>true</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,7,0,0,0,0,0,0</StartKeyframe><Keyframes>0,7,0,0,0,0,0,0;254016000000,1,0,0,0,0,0,0;</Keyframes>",
    );
    assert_ne!(keyed_matte, key(20));
    let with_private_data = key(20).replace(
        "</Component><MatchName>",
        "</Component><PremiereFilterPrivateData Encoding=\"base64\">AAAA</PremiereFilterPrivateData><MatchName>",
    );
    // A second clip (`VideoClipTrackItem:9`, 5-10 s) keeps the sequence
    // convertible when the keyed clip and its matte are both omitted.
    let source = with_second_clip(SOURCE);
    let base = with_matte_track(&source, &[(20, key(20))]);
    let portrait_source = source.replace(
        "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
        "<FrameRect>0,0,1080,1920</FrameRect></VideoStream>",
    );
    assert_ne!(portrait_source, source);
    for (case, xml, reason) in [
        (
            "keyed Matte",
            with_matte_track(&source, &[(20, keyed_matte)]),
            "VideoFilterComponent:20: keyframed Matte is not supported; only static values convert",
        ),
        // Fixture clip E: Premiere shows the clip outside the matte item, where
        // the matte has no luma (G3b).
        (
            "Reverse with Matte Luma",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 7, 1, true))]),
            "VideoFilterComponent:20: Reverse with Matte Luma is not converted; Premiere gives the matte clip's zero-luma exterior full coverage where FX's inverted luma matte gives none",
        ),
        (
            "unknown Composite Using",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 7, 2, false))]),
            "VideoFilterComponent:20: Composite Using \"2\" is neither Matte Alpha (0) nor Matte Luma (1)",
        ),
        (
            "Matte 0",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 0, 0, false))]),
            "VideoFilterComponent:20: Matte \"0\" names no video track",
        ),
        (
            "Matte names no track",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 9, 0, false))]),
            "VideoClipTrackItem:3: Track Matte Key Matte 9 names no video track of the sequence",
        ),
        (
            "Matte names the clip's own track",
            with_matte_track(&source, &[(20, track_matte_key_xml(20, 1, 0, false))]),
            "Track Matte Key names video track 0, which is not above the clip's track 0",
        ),
        (
            "private data",
            with_matte_track(&source, &[(20, with_private_data)]),
            "VideoFilterComponent:20: PremiereFilterPrivateData is not supported",
        ),
        (
            "two keys",
            with_matte_track(&source, &[(20, key(20)), (30, key(30))]),
            "VideoComponentChain:4: duplicate Track Matte Key component",
        ),
        (
            "key with a Crop",
            with_matte_track(&source, &[(20, key(20)), (30, top_crop(30))]),
            "a Track Matte Key with a Crop, Linear Wipe or Opacity mask on one clip is not converted",
        ),
        (
            "portrait fill source",
            with_matte_track(&portrait_source, &[(20, key(20))]),
            "Track Matte Key on media that is not sequence-sized is not converted",
        ),
        (
            "two matte items over the range",
            base.replace(
                MATTE_TRACK,
                &matte_track_of(&[(40, 0, 2 * TICKS), (50, 2 * TICKS, 5 * TICKS)]),
            ),
            "the matte track 1 holds more than one clip over the clip's range; only one unambiguous provider converts",
        ),
        (
            "disabled matte item",
            base.replace(
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem>",
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem><IsMuted>true</IsMuted>",
            ),
            "the matte clip is disabled or on a muted track; FX drops a hidden matte source and would show the clip whole",
        ),
        (
            "muted matte track",
            base.replace(
                "<Track><ID>7</ID><Index>1</Index></Track>",
                "<Track><ID>7</ID><Index>1</Index><IsMuted>true</IsMuted></Track>",
            ),
            "the matte clip is disabled or on a muted track",
        ),
        // FX would draw a hidden clip's matte; Premiere's is unmeasured.
        (
            "disabled keyed clip",
            base.replace(
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem>",
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem><IsMuted>true</IsMuted>",
            ),
            "a Track Matte Key on a disabled clip or on a muted track is not converted: whether Premiere draws its matte clip is unmeasured, and FX draws a hidden clip's matte source as content",
        ),
        (
            "matte item with a Crop",
            base.replace(
                "<VideoComponentChain ObjectID=\"94\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
                &format!("<VideoComponentChain ObjectID=\"94\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"40\"/></Components></ComponentChain></VideoComponentChain>{}", top_crop(40)),
            ),
            "the matte clip has its own Crop, Linear Wipe, Opacity mask or Track Matte Key",
        ),
        // A matte item that its own rules omit leaves the matte track empty.
        (
            "omitted matte item",
            base.replace(
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"94\"/></ComponentOwner>",
                "<VideoClipTrackItem ObjectID=\"93\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"94\"/></ComponentOwner><OriginalSubClipTimeOffset>1</OriginalSubClipTimeOffset>",
            ),
            "the matte track 1 holds no clip over the clip's range",
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let kept: Vec<_> = project.sequences[0].video_tracks[0]
            .items
            .iter()
            .filter_map(crate::schema::PrVideoItem::id)
            .collect();
        assert_eq!(kept, ["VideoClipTrackItem:9"], "{case}");
        let omission = omissions
            .iter()
            .find(|omission| {
                omission.scope == OmissionScope::Occurrence
                    && (omission.record == "3" || omission.record == "VideoClipTrackItem:3")
            })
            .unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert!(omission.reason.contains(reason), "{case}: {}", omission.reason);
    }
}

#[test]
fn an_omitted_keyed_clip_consumes_its_matte_clips() {
    // Premiere does not draw a matte clip while an active key names its
    // track, whether or not the keyed clip converts (fixture G1b: clip D's
    // matte hid while its Reverse was unconverted), so the matte clips over
    // an omitted clip's range go too, wherever the clip was omitted: in the
    // key's own read (E), before it (a keyer beside the key) or after every
    // item is read (two matte items over the range; a disabled keyed clip).
    // A key whose Composite Using is keyed still names its matte by
    // its static Matte. A second clip (`VideoClipTrackItem:9`, 5-10 s) keeps
    // the sequence convertible.
    let source = with_second_clip(SOURCE);
    let reverse_luma = |id| track_matte_key_xml(id, 7, 1, true);
    let keyed_composite = track_matte_key(20).replace(
        "<IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>",
        "<IsTimeVarying>true</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><Keyframes>0,0,0,0,0,0,0,0;254016000000,1,0,0,0,0,0,0;</Keyframes>",
    );
    assert_ne!(keyed_composite, track_matte_key(20));
    let ultra_key = tint(30)
        .replace(
            "<DisplayName>Tint</DisplayName>",
            "<DisplayName>Ultra Key</DisplayName>",
        )
        .replace("AE.ADBE Tint", "AE.ADBE Ultra Key");
    let consumed = "matte source of the omitted clip VideoClipTrackItem:3 was not converted: Premiere does not draw a track-matte source";
    for (case, xml, omitted_mattes) in [
        (
            "Reverse with Matte Luma",
            with_matte_track(&source, &[(20, reverse_luma(20))]),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "a keyer beside the key",
            with_matte_track(&source, &[(20, track_matte_key(20)), (30, ultra_key)]),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "keyed Composite Using",
            with_matte_track(&source, &[(20, keyed_composite)]),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "disabled keyed clip",
            with_matte_track(&source, &[(20, track_matte_key(20))]).replace(
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem>",
                "<VideoClipTrackItem ObjectID=\"3\"><ClipTrackItem><IsMuted>true</IsMuted>",
            ),
            vec!["VideoClipTrackItem:93"],
        ),
        (
            "two matte items over the range",
            with_matte_track(&source, &[(20, track_matte_key(20))]).replace(
                MATTE_TRACK,
                &matte_track_of(&[(40, 0, 2 * TICKS), (50, 2 * TICKS, 5 * TICKS)]),
            ),
            vec!["VideoClipTrackItem:40", "VideoClipTrackItem:50"],
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let tracks = &project.sequences[0].video_tracks;
        let kept: Vec<_> = tracks
            .iter()
            .flat_map(|track| {
                track
                    .items
                    .iter()
                    .filter_map(crate::schema::PrVideoItem::id)
            })
            .collect();
        assert_eq!(kept, ["VideoClipTrackItem:9"], "{case}");
        let consumed_records: Vec<_> = omissions
            .iter()
            .filter(|omission| {
                omission.scope == OmissionScope::Occurrence && omission.reason == consumed
            })
            .map(|omission| omission.record.as_str())
            .collect();
        assert_eq!(consumed_records, omitted_mattes, "{case}: {omissions:?}");
    }
    // A matte that a kept clip keys stays: track 1's clip 95 keys the matte
    // (track 2, `Track/ID` 7) by Matte Alpha over the same range as the
    // omitted clip 3.
    let kept_consumer = Placement {
        start: 0,
        end: 5 * TICKS,
        source_in: 0,
    };
    let xml = with_matte_track(&source, &[(20, reverse_luma(20))])
        .replace(
            "<Track Index=\"1\" ObjectURef=\"track-2\"/></Tracks>",
            "<Track Index=\"1\" ObjectURef=\"track-mid\"/><Track Index=\"2\" ObjectURef=\"track-2\"/></Tracks>",
        )
        .replace(
            "<Track><ID>7</ID><Index>1</Index></Track><ClipItems><Index>1</Index>",
            "<Track><ID>7</ID><Index>2</Index></Track><ClipItems><Index>2</Index>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                "<VideoClipTrack ObjectUID=\"track-mid\"><ClipTrack><Track><ID>8</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef=\"95\"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>{}{}</PremiereData>",
                placement_records(95, 7, &kept_consumer).replace(
                    "<VideoComponentChain ObjectID=\"96\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
                    "<VideoComponentChain ObjectID=\"96\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"60\"/></Components></ComponentChain></VideoComponentChain>",
                ),
                track_matte_key_xml(60, 7, 0, false)
            ),
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let tracks = &project.sequences[0].video_tracks;
    assert_eq!(
        tracks[1].clip(0).track_matte,
        Some(PrTrackMatte {
            track_index: 2,
            channel: PrMatteChannel::Alpha,
        })
    );
    assert_eq!(
        tracks[2].clip(0).id.as_deref(),
        Some("VideoClipTrackItem:93"),
        "{omissions:?}"
    );
    assert!(
        omissions.iter().all(|omission| omission.reason != consumed),
        "{omissions:?}"
    );
}

/// Premiere 26.5.1's Track Matte Key records, verbatim from the
/// second save of `feature_track_matte_key_26_5_strict.prproj`:
/// `VideoFilterComponent` 9 / `Component` 7 with `VideoFilterType` before
/// `MatchName`, no `Bypass`, `Intrinsic` or `ArchivedType`, and three v10
/// parameters without `ParameterControlType` or `IsTimeVarying`; only
/// Composite Using carries bounds and `DiscontinuousInterpolate`. Clip A
/// (`:220`): Matte 3 (the V3 track's `Track/ID`), Matte Alpha, Reverse false.
const PREMIERE_26_5_KEY_A: &str = r#"<VideoFilterComponent ObjectID="220" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="259"/>
				<Param Index="1" ObjectRef="260"/>
				<Param Index="2" ObjectRef="261"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="259" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="260" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="261" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip D (`:233`): Matte 3, Matte Alpha, Reverse true.
const PREMIERE_26_5_KEY_D: &str = r#"<VideoFilterComponent ObjectID="233" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="288"/>
				<Param Index="1" ObjectRef="289"/>
				<Param Index="2" ObjectRef="290"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="288" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="289" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="290" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip E (`:238`): Matte 3, Matte Luma, Reverse true.
const PREMIERE_26_5_KEY_E: &str = r#"<VideoFilterComponent ObjectID="238" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="297"/>
				<Param Index="1" ObjectRef="298"/>
				<Param Index="2" ObjectRef="299"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="297" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="298" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,1,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="299" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip D's key in the first save, before any matte was selected: Matte None
/// is 4294967295.
const PREMIERE_26_5_KEY_NONE: &str = r#"<VideoFilterComponent ObjectID="233" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="288"/>
				<Param Index="1" ObjectRef="289"/>
				<Param Index="2" ObjectRef="290"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="288" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,4294967295,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="289" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="290" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;
/// Clip F (`VideoClipTrackItem:136`): its chain `:178` holds the key `:235`
/// at `Index` 0 (`ID` 4) above the Gaussian Blur (Legacy) `:236` at `Index`
/// 1 (`ID` 3), Blurriness 25, so the blur applies before the key (G7).
const PREMIERE_26_5_CHAIN_F: &str = r#"<VideoComponentChain ObjectID="178" ClassID="0970e08a-f58f-4108-b29a-1a717b8e12e2" Version="3">
		<DefaultMotion>true</DefaultMotion>
		<DefaultOpacity>true</DefaultOpacity>
		<DefaultMotionComponentID>1</DefaultMotionComponentID>
		<DefaultOpacityComponentID>2</DefaultOpacityComponentID>
		<ComponentChain Version="3">
			<Node Version="1">
				<Properties Version="1">
					<MZ.ComponentChain.ActiveComponentID>2</MZ.ComponentChain.ActiveComponentID>
					<MZ.ComponentChain.ActiveComponentParamIndex>4294967295</MZ.ComponentChain.ActiveComponentParamIndex>
				</Properties>
			</Node>
			<Components Version="1">
				<Component Index="0" ObjectRef="235"/>
				<Component Index="1" ObjectRef="236"/>
			</Components>
		</ComponentChain>
	</VideoComponentChain>
<VideoFilterComponent ObjectID="235" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="291"/>
				<Param Index="1" ObjectRef="292"/>
				<Param Index="2" ObjectRef="293"/>
			</Params>
			<ID>4</ID>
			<DisplayName>Track Matte Key</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Legacy Key Track Matte</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="291" ClassID="2f2eb0a3-318c-4a93-99fc-f1d319edc864" Version="10">
		<Name>Matte:</Name>
		<ParameterID>1</ParameterID>
		<StartKeyframe>-91445760000000000,3,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoComponentParam ObjectID="292" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Composite Using:</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>1</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="293" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name>Reverse</Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>
<VideoFilterComponent ObjectID="236" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9">
		<Component Version="7">
			<Params Version="1">
				<Param Index="0" ObjectRef="294"/>
				<Param Index="1" ObjectRef="295"/>
				<Param Index="2" ObjectRef="296"/>
			</Params>
			<ID>3</ID>
			<DisplayName>Gaussian Blur (Legacy)</DisplayName>
		</Component>
		<VideoFilterType>2</VideoFilterType>
		<MatchName>AE.ADBE Gaussian Blur 2</MatchName>
	</VideoFilterComponent>
<VideoComponentParam ObjectID="294" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="10">
		<Name>Blurriness</Name>
		<ParameterID>1</ParameterID>
		<UpperUIBound>50</UpperUIBound>
		<StartKeyframe>-91445760000000000,25.,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>30000</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="295" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10">
		<Name>Blur Dimensions</Name>
		<DiscontinuousInterpolate>true</DiscontinuousInterpolate>
		<ParameterID>2</ParameterID>
		<StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe>
		<LowerBound>0</LowerBound>
		<UpperBound>2</UpperBound>
	</VideoComponentParam>
<VideoComponentParam ObjectID="296" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10">
		<Name> </Name>
		<ParameterID>3</ParameterID>
		<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>
	</VideoComponentParam>"#;

#[test]
fn numbered_images_native_track_matte_survives_public_import() {
    use fx_conv::{ConversionMode, ImportToTesseract};

    for (records, id, mode) in [
        (PREMIERE_26_5_KEY_A, 220, "alpha"),
        (PREMIERE_26_5_KEY_D, 233, "alphaInverted"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let xml = with_matte_track(SOURCE, &[(id, records.to_owned())])
            .replace("<ID>7</ID><Index>1</Index>", "<ID>3</ID><Index>1</Index>")
            .replace("8467200000", "10160640000")
            .replace("1270080000000", "20321280000")
            .replace("2540160000000", "20321280000")
            .replace("1920,1080", "2,1")
            .replace("media/source.mp4", "media/frame_00000.png")
            .replace(
                "<VideoStream ObjectID=\"8\">",
                "<VideoStream ObjectID=\"8\"><IsNumberedStills>true</IsNumberedStills><AlphaType>1</AlphaType>",
            );
        std::fs::create_dir(dir.path().join("media")).unwrap();
        for index in 0..2 {
            image::RgbaImage::from_pixel(2, 1, image::Rgba([255, 0, 0, 128]))
                .save(dir.path().join(format!("media/frame_{index:05}.png")))
                .unwrap();
        }
        let source = dir.path().join("source.prproj");
        std::fs::write(&source, xml).unwrap();
        let output = dir.path().join("converted");
        let report = crate::Premiere
            .import_to_tesseract(
                &source,
                &output,
                &crate::PremiereImportOptions::default(),
                ConversionMode::Write,
            )
            .unwrap();
        let archive = tesseract_file::TesseractFile::open(output.join("project.tsrct")).unwrap();
        let value = archive.project_json().unwrap();
        let layers = value["composition"]["layers"].as_array().unwrap();
        let consumer = layers
            .iter()
            .find(|layer| layer["trackMatte"].is_object())
            .unwrap_or_else(|| panic!("missing numbered-image consumer: {:?}", report.diagnostics));
        let provider = layers
            .iter()
            .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(consumer["trackMatte"]["mode"], mode);
        for group in [consumer, provider] {
            assert_eq!(group["type"], "Group");
            let frames = group["layers"].as_array().unwrap();
            assert_eq!(frames.len(), 2);
            for (index, frame) in frames.iter().enumerate() {
                assert_eq!(frame["type"], "Image");
                assert_eq!(frame["parent"], group["id"]);
                assert_eq!(
                    frame["source"]["assetId"],
                    format!("premiere-video-1-frame-{index}")
                );
                assert_eq!(crate::test_support::layer_range(frame)["start"], index * 40);
                assert_eq!(crate::test_support::layer_range(frame)["duration"], 40);
            }
        }
    }
}

#[test]
fn premiere_26_5_track_matte_keys_read_as_saved() {
    // The matte track takes the fixture's V3 `Track/ID` 3; the records are
    // otherwise as saved.
    let fixture =
        |xml: String| xml.replace("<ID>7</ID><Index>1</Index>", "<ID>3</ID><Index>1</Index>");
    for (case, records, id, channel) in [
        (
            "A: Matte Alpha",
            PREMIERE_26_5_KEY_A,
            220,
            PrMatteChannel::Alpha,
        ),
        (
            "D: Matte Alpha, Reverse",
            PREMIERE_26_5_KEY_D,
            233,
            PrMatteChannel::AlphaInverted,
        ),
    ] {
        let xml = fixture(with_matte_track(SOURCE, &[(id, records.to_owned())]));
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        let clip = project.sequences[0].video_tracks[0].clip(0);
        assert_eq!(
            clip.track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel
            }),
            "{case}"
        );
        assert!(clip.effects.is_empty(), "{case}");
    }
    // F: the chain as saved, the key at Index 0 above the blur at Index 1.
    let xml = fixture(
        with_matte_track(SOURCE, &[])
            .replace(
                "<Components ObjectRef=\"4\"/>",
                "<Components ObjectRef=\"178\"/>",
            )
            .replace(
                "</PremiereData>",
                &format!("{PREMIERE_26_5_CHAIN_F}</PremiereData>"),
            ),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project.sequences[0].video_tracks[0].clip(0);
    assert_eq!(
        clip.track_matte,
        Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        })
    );
    assert_eq!((clip.effects.len(), clip.effects_above_mask), (1, 1));
    assert!(
        matches!(
            &clip.effects[0].params,
            crate::schema::PrEffectParams::GaussianBlur(blur) if blur.blurriness == 25.0
        ),
        "{:?}",
        clip.effects[0].params
    );
    // E fails closed by name; a second clip keeps the sequence convertible.
    // The unselected Matte instead retains unkeyed content (the next test).
    {
        let (case, records, id, reason, kept) = (
            "E: Matte Luma, Reverse",
            PREMIERE_26_5_KEY_E,
            238,
            "VideoFilterComponent:238: Reverse with Matte Luma is not converted",
            vec!["VideoClipTrackItem:9"],
        );
        let xml = fixture(with_matte_track(
            &with_second_clip(SOURCE),
            &[(id, records.to_owned())],
        ));
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let items: Vec<_> = project.sequences[0]
            .video_tracks
            .iter()
            .flat_map(|track| {
                track
                    .items
                    .iter()
                    .filter_map(crate::schema::PrVideoItem::id)
            })
            .collect();
        assert_eq!(items, kept, "{case}: {omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.record == "3" && omission.reason.contains(reason)),
            "{case}: {omissions:?}"
        );
    }
}

#[test]
fn track_matte_none_keeps_unkeyed_picture_and_animated_nest_children() {
    // G6's independently saved None value names no track. Reverse and
    // Composite Using cannot select coverage when there is no matte.
    for records in [
        PREMIERE_26_5_KEY_NONE.to_owned(),
        PREMIERE_26_5_KEY_NONE.replace(
            "-91445760000000000,true,0,0,0,0,0,0",
            "-91445760000000000,unknown,0,0,0,0,0,0",
        ),
    ] {
        let xml = with_matte_track(SOURCE, &[(233, records), (400, blur(400))]);
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let sequence = &project.sequences[0];
        let picture = sequence.video_tracks[0].clip(0);
        assert_eq!(picture.id.as_deref(), Some("VideoClipTrackItem:3"));
        assert_eq!(picture.timeline_ticks(), 0..5 * TICKS);
        assert_eq!((picture.in_ticks, picture.out_ticks), (0, 5 * TICKS));
        assert!(picture.track_matte.is_none());
        assert_eq!(picture.effects.len(), 1);
        assert!(matches!(&picture.effects[0].params,
            crate::schema::PrEffectParams::GaussianBlur(blur) if blur.blurriness == 25.0));
        assert_eq!(
            sequence.video_tracks[1].clip(0).id.as_deref(),
            Some("VideoClipTrackItem:93")
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.scope == OmissionScope::Feature
                    && note.record == "VideoFilterComponent:233"
                    && note.reason.contains("Matte None")),
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .all(|note| note.scope != OmissionScope::Occurrence),
            "{omissions:?}"
        );
    }

    // An inert key cannot erase or duplicate the independent active key,
    // regardless of stack order, and does not count as an extra colour effect.
    for keys in [
        vec![
            (20, track_matte_key(20)),
            (30, track_matte_key_xml(30, 4294967295, 0, false)),
        ],
        vec![
            (30, track_matte_key_xml(30, 4294967295, 0, false)),
            (20, track_matte_key(20)),
        ],
    ] {
        let xml = with_matte_track(SOURCE, &keys);
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        assert_eq!(
            project.sequences[0].video_tracks[0].clip(0).track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Alpha,
            })
        );
        assert!(
            omissions
                .iter()
                .all(|note| note.scope != OmissionScope::Occurrence),
            "{omissions:?}"
        );
    }

    // Parameter identity and selected coverage, not the number of inactive
    // controls, decide admission. No fixed component-count limit is needed.
    let mut keys: Vec<_> = [20, 30, 40, 50, 60, 70]
        .into_iter()
        .map(|id| (id, track_matte_key_xml(id, 4294967295, 0, false)))
        .collect();
    keys.push((80, track_matte_key(80)));
    let xml = with_matte_track(SOURCE, &keys);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    let picture = project.sequences[0].video_tracks[0].clip(0);
    assert_eq!(picture.timeline_ticks(), 0..5 * TICKS);
    assert_eq!(
        picture.track_matte,
        Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha
        })
    );
    assert!(
        omissions
            .iter()
            .all(|note| note.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );

    let motion = super::nested::keyed_motion(300);
    let xml = keyed_nest_xml(Some(&motion)).replace(
        &track_matte_key_xml(130, 2, 1, false),
        &track_matte_key_xml(130, 4294967295, 1, true),
    );
    let (project, mut omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    let outer = &project.sequences[0];
    let nest = outer
        .nest_occurrences()
        .next()
        .expect("unkeyed animated nest");
    assert_eq!(nest.id.as_deref(), Some("VideoClipTrackItem:110"));
    assert!(nest.track_matte.is_none());
    assert!(!nest.animations.is_empty());
    assert_eq!(nest.sequence.video_occurrences().count(), 1);
    assert_eq!(outer.video_tracks[1].items.len(), 2);
    let ids = crate::tesseract_output::asset_ids_in_order(outer, &project.media);
    let document =
        crate::convert::premiere_to_tesseract(outer, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    let picture = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["name"] == nest.sequence.name)
        .unwrap();
    assert_eq!(picture["type"], "Group");
    assert!(picture["trackMatte"].is_null());
    let frame_guide = picture["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["id"] == picture["masks"][0]["layer"])
        .unwrap();
    assert_eq!(
        frame_guide["rect"]["size"],
        serde_json::json!([1920.0, 1080.0])
    );
    assert!(picture["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|child| child["type"] == "Video"));
    assert!(document["composition"]["dynamics"]["entries"]
        .as_array()
        .is_some_and(|tracks| !tracks.is_empty()));
    assert!(
        omissions
            .iter()
            .all(|note| note.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
}

#[test]
fn a_graphic_matte_clip_with_a_clip_opacity_mask_is_a_masked_matte() {
    // A graphic matte keys its clip, but one with a clip Opacity mask imports
    // as a group and the mask's guide beside it, which a stage group that
    // takes its matte would leave behind: the key does not convert, as for a
    // media matte with its own Opacity mask.
    let matte = PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    };
    let mut graphic = crate::tests::support::text_graphic();
    (graphic.start_ticks, graphic.end_ticks) = (0, 5 * TICKS);
    let mut sequence = keyed_sequence(PrMatteChannel::Alpha);
    sequence.video_tracks[1].items = vec![crate::schema::PrVideoItem::Graphic(graphic.clone())];
    assert_eq!(
        crate::schema::check_track_matte(&sequence.video_tracks, 0, 0..5 * TICKS, matte),
        Ok(())
    );
    graphic.opacity_mask = Some(crate::tests::support::opacity_mask());
    sequence.video_tracks[1].items = vec![crate::schema::PrVideoItem::Graphic(graphic)];
    assert_eq!(
        crate::schema::check_track_matte(&sequence.video_tracks, 0, 0..5 * TICKS, matte),
        Err(
            "the matte clip has its own Crop, Linear Wipe, Opacity mask or Track Matte Key"
                .to_owned()
        )
    );
}

#[test]
fn chained_track_mattes_keep_each_editable_coverage_owner() {
    for nested in [false, true] {
        let mut sequence = keyed_sequence(PrMatteChannel::Alpha);
        let key = PrTrackMatte {
            track_index: 2,
            channel: PrMatteChannel::AlphaInverted,
        };
        if nested {
            let inner = sequence_of(
                "Provider content",
                vec![PrVideoTrack::media([clip_of("source", 0..5 * TICKS, 0)])],
            );
            let mut provider = nest_of(inner, 0..5 * TICKS, 0);
            provider.track_matte = Some(key);
            sequence.video_tracks[1] = PrVideoTrack {
                items: Vec::new(),
                nests: vec![provider],
                transitions: Vec::new(),
            };
        } else {
            sequence.video_tracks[1].clip_mut(0).track_matte = Some(key);
        }
        sequence.video_tracks.push(PrVideoTrack::media([clip_of(
            "source",
            0..5 * TICKS,
            TICKS,
        )]));
        let media = video_media();
        assert_eq!(
            check_track_matte(
                &sequence.video_tracks,
                0,
                0..5 * TICKS,
                PrTrackMatte {
                    track_index: 1,
                    channel: PrMatteChannel::Alpha
                },
            ),
            Ok(()),
            "nested={nested}"
        );
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let document =
            crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
                .unwrap()
                .to_json_value()
                .unwrap();
        fx_schema::EditableFxCompositionDocument::from_json_value(document.clone()).unwrap();
        let layers = document["composition"]["layers"].as_array().unwrap();
        let consumer = layers
            .iter()
            .find(|layer| layer["trackMatte"]["mode"] == "alpha")
            .unwrap();
        let provider = layers
            .iter()
            .find(|layer| layer["id"] == consumer["trackMatte"]["layer"])
            .unwrap();
        assert_eq!(provider["trackMatte"]["mode"], "alphaInverted");
        let guide = layers
            .iter()
            .find(|layer| layer["id"] == provider["trackMatte"]["layer"])
            .unwrap();
        assert!(guide["trackMatte"].is_null());
        assert_eq!(provider["type"], if nested { "Group" } else { "Video" });
        assert_eq!(guide["type"], "Video");
        assert_eq!(consumer["playback"]["inputRange"]["duration"], 5000);
        assert_eq!(guide["playback"]["mapping"]["output"]["start"], 1000);
        assert!(
            omissions
                .iter()
                .all(|note| note.scope != OmissionScope::Occurrence),
            "{omissions:?}"
        );

        sequence.video_tracks[0].clip_mut(0).transform.position[0] = 0.6;
        let mut omissions = Vec::new();
        crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions).unwrap();
        assert!(
            omissions.iter().any(|note| note
                .reason
                .contains("cannot move its provider without its coverage dependencies")),
            "nested={nested}: {omissions:?}"
        );
    }
}

/// A 30 fps 1080p sequence: the 0-5 s source on track 0 keyed by `channel`
/// from the same source on track 1.
fn keyed_sequence(channel: PrMatteChannel) -> crate::schema::PrSequence {
    let mut fill = clip_of("source", 0..5 * TICKS, 0);
    fill.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel,
    });
    sequence_of(
        "Main",
        vec![
            PrVideoTrack::media([fill]),
            PrVideoTrack::media([clip_of("source", 0..5 * TICKS, 0)]),
        ],
    )
}

#[test]
fn written_track_matte_keys_name_the_matte_track_id_and_read_back() {
    let mut media = video_media();
    let source = media.get_mut(&MediaId("source".into())).unwrap();
    source.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Video {
        codec: Some(crate::schema::VideoCodec::H264),
        hdr_profile: None,
    };
    source.name = "source.mp4".into();
    source.relative_path = Some("./media/source.mp4".into());
    source.relative_paths = vec!["./media/source.mp4".into()];
    source.absolute_paths = vec![(
        crate::schema::records::MediaPathField::FilePath,
        "/media/source.mp4".into(),
    )];
    for (channel, composite, reverse) in [
        (PrMatteChannel::Alpha, "0", "false"),
        (PrMatteChannel::AlphaInverted, "0", "true"),
        (PrMatteChannel::Luma, "1", "false"),
    ] {
        let project = crate::schema::PrProjectFile::from_sequences(
            vec![keyed_sequence(channel)],
            media.clone(),
        );
        let xml = project_xml(&project).unwrap();
        let graph = Graph::parse(&xml).unwrap();
        let key = graph
            .records()
            .find(|record| {
                record
                    .element()
                    .child("MatchName")
                    .and_then(crate::format::graph::Element::text)
                    == Some("AE.ADBE Legacy Key Track Matte")
            })
            .expect("a Track Matte Key component");
        // The corpus 12.x form: the written track at index 1 has ID 2.
        assert_eq!(key.element().attribute("Version"), Some("7"));
        let body = key.element().child("Component").expect("Component");
        assert_eq!(body.attribute("Version"), Some("5"));
        let values: Vec<_> = body
            .child("Params")
            .expect("Params")
            .children()
            .map(|param| {
                let record = graph.locate(&param.reference(), "test").unwrap();
                let param = graph.decode::<VideoComponentParam>(record).unwrap();
                (
                    param.value.name.unwrap(),
                    param.value.parameter_control_type.unwrap(),
                    param.value.start_keyframe,
                )
            })
            .collect();
        assert_eq!(
            values,
            [
                (
                    "Matte:".to_owned(),
                    "13".to_owned(),
                    "-91445760000000000,2,0,0,0,0,0,0".to_owned()
                ),
                (
                    "Composite Using:".to_owned(),
                    "7".to_owned(),
                    format!("-91445760000000000,{composite},0,0,0,0,0,0")
                ),
                (
                    "Reverse".to_owned(),
                    "4".to_owned(),
                    format!("-91445760000000000,{reverse},0,0,0,0,0,0")
                ),
            ],
            "{channel:?}"
        );
        let (reread, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let tracks = &reread.sequences[0].video_tracks;
        assert_eq!(
            tracks[0].clip(0).track_matte,
            Some(PrTrackMatte {
                track_index: 1,
                channel
            })
        );
        assert_eq!(tracks[1].clip(0).track_matte, None);
    }
    // The model rejects a key whose matte the sequence lacks, so no clip is
    // written without its matte.
    let mut sequence = keyed_sequence(PrMatteChannel::Alpha);
    sequence.video_tracks.pop();
    sequence.timeline_end_ticks = sequence.occurrence_end_ticks();
    let project = crate::schema::PrProjectFile::from_sequences(vec![sequence], media);
    let error = project_xml(&project).unwrap_err().to_string();
    assert!(
        error.contains("Track Matte Key names video track 1, which the sequence does not have"),
        "{error}"
    );
}
