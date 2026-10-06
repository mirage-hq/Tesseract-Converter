use crate::{
    convert::premiere_to_tesseract::premiere_to_tesseract,
    format::{MediaId, PrMedia},
    schema::{
        PrColorMatte, PrKeyframeEasing, PrMatteChannel, PrMediaKind, PrPropertyAnimation,
        PrScalarKeyframe, PrTrackMatte, PrVideoTrack, TICKS, TICKS_PER_MILLISECOND,
    },
    tests::support::{clip_of, video_media, video_sequence},
    OmissionScope,
};

#[test]
fn color_matte_failed_key_keeps_ordinary_content_but_never_freezes_provider_coverage() {
    let mut sequence = video_sequence();
    let mut media = video_media();
    let mut stream = media.values().next().unwrap().video.clone().unwrap();
    stream.kind = PrMediaKind::ColorMatte(PrColorMatte { rgb: [255, 0, 0] });
    media.insert(
        MediaId("matte".into()),
        PrMedia {
            name: "Matte".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: Some(stream),
            audio: None,
        },
    );
    let mut matte = clip_of("matte", 0..5 * TICKS, 0);
    matte.id = Some("provider".into());
    matte.animations = vec![PrPropertyAnimation::Rotation(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: TICKS_PER_MILLISECOND / 4,
            value: 20.0,
            easing: PrKeyframeEasing::Linear,
        },
    ])];
    sequence.video_tracks = vec![PrVideoTrack::media([matte])];
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert_eq!(document["composition"]["layers"][0]["type"], "Rect");
    assert!(omissions.iter().any(|o| o.scope == OmissionScope::Feature
        && o.reason.contains("Rotation animation was not imported")));

    let mut consumer = clip_of("source", 0..5 * TICKS, 0);
    consumer.id = Some("consumer".into());
    consumer.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    let independent = clip_of("source", 5 * TICKS..10 * TICKS, 0);
    let provider = sequence.video_tracks.remove(0);
    sequence.video_tracks = vec![PrVideoTrack::media([consumer, independent]), provider];
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
    assert!(!layers
        .iter()
        .any(|layer| layer["rect"]["fillColor"] == serde_json::json!([1.0, 0.0, 0.0, 1.0])));
    assert!(omissions.iter().any(|o| o.record == "provider"
        && o.scope == OmissionScope::Occurrence
        && o.reason
            .contains("coverage and dependent consumers omitted")));
    assert!(omissions
        .iter()
        .any(|o| o.record == "consumer" && o.scope == OmissionScope::Occurrence));
}
