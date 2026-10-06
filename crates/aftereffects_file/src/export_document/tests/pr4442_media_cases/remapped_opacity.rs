//! Source-owned controls must remain inside the editable source clock, not be
//! moved onto the occurrence's Time Remap clock. Native proof is separate.
//!
//! The explicit ignored generator emits fresh public original/edited candidates
//! for a pinned independent Adobe fixture; it never renders or runs Adobe.
use super::*;

#[test]
fn remapped_opacity_independent_native_source_import_keeps_editable_controls() {
    let bytes = include_bytes!("../../../../tests/fixtures/source-clock-opacity/independent.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "f612d3b48b93c3c17956a288f394348b9dca8be13dab8d0c57f0010f96cb8ea5"
    );
    let native = read_project(bytes).unwrap();
    let imported = to_structural_fx_document(&native, Some(15)).unwrap();
    let entries = imported.document.composition().dynamics().entries();
    let opacity = entries
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(LayerId::new(5), PropType::Opacity))
        .expect("independent source-owned Opacity owner");
    let AnimatorData::Keyframes { track, .. } = opacity.animator.data() else {
        panic!("editable Opacity keys")
    };
    assert_eq!(
        track
            .keyframes()
            .iter()
            .map(|key| (key.layer_time().as_millis(), key.value().clone()))
            .collect::<Vec<_>>(),
        vec![
            (0, PropertyValue::Float(0.0)),
            (40, PropertyValue::Float(50.0)),
            (80, PropertyValue::Float(100.0))
        ]
    );
    assert!(
        track
            .keyframes()
            .iter()
            .all(|key| key.easing() == PropertyKeyframeEasing::Linear)
    );
    for parameter in ["horizontalBlocks", "verticalBlocks"] {
        let target: PropertyTarget = serde_json::from_value(
            json!({"kind":"effectProperty","effectId":7,"paramName":parameter}),
        )
        .unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.target == target)
            .expect("independent editable Mosaic control");
        let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
            panic!("editable Mosaic keys")
        };
        assert_eq!(
            track
                .keyframes()
                .iter()
                .map(|key| key.layer_time().as_millis())
                .collect::<Vec<_>>(),
            vec![0, 2500, 5500, 16000]
        );
    }
}

fn case(
    extra: Option<PropType>,
) -> (
    EditableFxCompositionDocument,
    BTreeMap<String, media::ResolvedMediaSource>,
) {
    let mut owner = video(8990, "source-movie");
    owner.as_object_mut().unwrap().remove("activeRange");
    owner["sourceRange"] = json!({"start":0,"duration":4000});
    owner["playback"] = fixture_remapped_playback(
        json!({"start":500,"duration":1500}),
        json!({"keyframes":[
        {"id":"start","time":500,"value":0,"easing":{"type":"linear"}},
        {"id":"middle","time":1000,"value":1500,"easing":{"type":"linear"}},
        {"id":"last","time":1900,"value":3000,"easing":{"type":"linear"}}
    ],"before":"inactive","after":"hold"}),
    );
    let mut entries = vec![keyed_entry(
        LayerId::new(8990),
        PropType::Opacity,
        [
            (0, PropertyValue::Float(0.0)),
            (40, PropertyValue::Float(50.0)),
            (80, PropertyValue::Float(100.0)),
        ],
    )];
    if let Some(property) = extra {
        entries.push(keyed_entry(
            LayerId::new(8990),
            property,
            [
                (0, PropertyValue::Float(0.0)),
                (100, PropertyValue::Float(10.0)),
            ],
        ));
    }
    let doc = document(vec![owner, rect(&imported(), 9000)], entries);
    let size = doc.dimensions();
    let sources = BTreeMap::from([(
        "source-movie".into(),
        resolved(
            "source-movie",
            "media/source.mov",
            NativeSourceFormat::QuickTime,
            [size.width as u16, size.height as u16],
            4000,
            NativeFrameRate::integer(30),
            0.0,
        ),
    )]);
    (doc, sources)
}

#[test]
fn remapped_opacity_retains_source_movie_and_source_owned_keys() {
    let (doc, sources) = case(None);
    let output = to_aep_with_media_and_fps(&doc, &sources, 30.0).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "video-8990"),
        "{:?}",
        output.diagnostics
    );
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 9000")
    );
    let wrapper = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "video-8990")
        .unwrap();
    assert_eq!(property_values(wrapper, "ADBE Opacity"), vec![1.0]);
    assert!(
        root_runs(&wrapper.content)
            .unwrap()
            .iter()
            .any(|(name, keys)| *name == "ADBE Time Remapping" && !keys.is_empty())
    );
    let inner = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.as_ref()),
            _ => None,
        })
        .flat_map(|comp| comp.layers.iter())
        .find(|layer| layer.name.as_ref() == "video-8990 source controls")
        .expect("retained editable movie source plane");
    let opacity = read_transform(&inner.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == "ADBE Opacity")
        .unwrap()
        .numeric
        .unwrap();
    assert_eq!(
        opacity
            .keyframes
            .iter()
            .map(|key| key.values.clone())
            .collect::<Vec<_>>(),
        vec![vec![0.0], vec![0.5], vec![1.0]]
    );
    assert!(
        !root_runs(&inner.content)
            .unwrap()
            .iter()
            .any(|(name, _)| *name == "ADBE Time Remapping")
    );
    assert!(native.items.iter().any(|item| item.media.is_some()));
}

#[test]
fn remapped_opacity_keeps_source_seed_and_incoming_parent() {
    let (doc, sources) = case(None);
    let mut value = doc.to_json_value().unwrap();
    value["composition"]["layers"][0]["transform"]["opacity"] = json!(0.0);
    value["composition"]["layers"][0]["parent"] = json!(9000);
    let mut parent = group(
        9000,
        "source-owner-parent",
        vec![value["composition"]["layers"][0].clone()],
    );
    parent.as_object_mut().unwrap().remove("activeRange");
    parent["transform"]["position"] = json!([10.0, 20.0]);
    parent["playback"] = fixture_linear_playback(
        json!({"start":0,"duration":30000}),
        json!({"start":0,"duration":30000}),
    );
    value["composition"]["layers"] = json!([parent, rect(&imported(), 9001)]);
    let doc = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = to_aep_with_media_and_fps(&doc, &sources, 30.0).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let parent = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "source-owner-parent")
        .expect("containing source is retained");
    let ItemKind::Composition(source) = &native.item(parent.record.source_id()).unwrap().kind
    else {
        panic!("containing source remains an editable precomposition");
    };
    let movie = source
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "video-8990")
        .unwrap_or_else(|| panic!("{:?}", output.diagnostics));
    assert_eq!(
        movie.record.parent_id(),
        0,
        "occurrence is local to its containing source, not reparented into its own clock plane"
    );
    assert_eq!(
        property_values(parent, "ADBE Position"),
        vec![10.0, 20.0, 0.0]
    );
}

#[test]
fn remapped_opacity_rejects_unproved_audio_matte_blend_and_geometry() {
    for mutation in [
        "audio", "matte", "blend", "geometry", "trim", "selector", "easing", "parent",
    ] {
        let (doc, mut sources) = case(None);
        let mut value = doc.to_json_value().unwrap();
        let owner = &mut value["composition"]["layers"][0];
        match mutation {
            "audio" => {
                owner["volume"] = json!(1.0);
                sources.get_mut("source-movie").unwrap().audio_sample_rate = 48000.0;
            }
            "matte" => owner["trackMatte"] = json!({"layer":9000,"mode":"alpha"}),
            "blend" => owner["blendMode"] = json!("multiply"),
            "geometry" => owner["transform"]["position"] = json!([1.0, 0.0]),
            "parent" => owner["parent"] = json!(9000),
            "trim" => {
                owner["sourceRange"] = json!({"start":500,"duration":3500});
                owner["playback"]["mapping"]["property"]["keyframes"][0]["value"] = json!(500);
            }
            "selector" => value["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(
                    serde_json::to_value(constant_entry(
                        LayerId::new(8990),
                        PropType::MediaSourceAssetId,
                        PropertyValue::String("source-movie".into()),
                    ))
                    .unwrap(),
                ),
            "easing" => {
                value["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1]["easing"] =
                    json!({"type":"cubicBezier","x1":0.3,"x2":0.7,"y1":0.2,"y2":0.8})
            }
            _ => unreachable!(),
        }
        let doc = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = to_aep_with_media_and_fps(&doc, &sources, 30.0).unwrap();
        let native = read_project(&output.bytes).unwrap();
        assert!(
            !layers(&native)
                .iter()
                .any(|layer| layer.name.as_ref() == "video-8990"),
            "{mutation}"
        );
        let expected = if mutation == "selector" {
            "source switches require a shared exact clock plan"
        } else if mutation == "parent" {
            "unproved non-containment transform parent"
        } else {
            "unproved geometry, audio, effects or dependencies"
        };
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains(expected)),
            "{mutation}: {:?}",
            output.diagnostics
        );
    }
}

#[test]
fn remapped_opacity_retains_non_frame_aligned_source_endpoint() {
    let (doc, mut sources) = case(None);
    let mut value = doc.to_json_value().unwrap();
    value["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(15042);
    value["composition"]["layers"][0]["sourceRange"]["duration"] = json!(15042);
    sources.get_mut("source-movie").unwrap().duration_millis = 15042;
    sources
        .get_mut("source-movie")
        .unwrap()
        .duration_millis_floor = 15042;
    let doc = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = to_aep_with_media_and_fps(&doc, &sources, 30.0).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "video-8990"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn remapped_opacity_rejects_source_tail_outside_native_container() {
    let (doc, mut sources) = case(None);
    let mut value = doc.to_json_value().unwrap();
    let owner = &mut value["composition"]["layers"][0];
    owner["sourceIntrinsicDuration"] = json!(4008);
    owner["sourceRange"]["duration"] = json!(4008);
    owner["playback"]["mapping"]["property"]["keyframes"][2]["value"] = json!(4007);
    let source = sources.get_mut("source-movie").unwrap();
    source.duration_millis = 4008;
    source.duration_millis_floor = 4008;
    let doc = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = to_aep_with_media_and_fps(&doc, &sources, 30.0).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert!(
        !layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "video-8990")
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("frame-aligned container")),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn remapped_opacity_keeps_dynamic_geometry_guard() {
    let (doc, sources) = case(Some(PropType::PositionX));
    let output = to_aep_with_media_and_fps(&doc, &sources, 30.0).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert!(
        !layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "video-8990")
    );
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 9000")
    );
}
