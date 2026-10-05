use crate::{
    convert::tesseract_to_premiere,
    format::FrameRate,
    media::{MediaFacts, VideoMedia},
    schema::{PrMediaKind, VideoCodec},
    test_support::editable_document,
    Omission,
};
use fx_schema::EditableFxCompositionDocument;
use serde_json::json;
use std::collections::BTreeMap;

const ORIGINAL: &str = "premiere-video-1";
const EYE_CONTACT: &str = "eye-contact-output";

fn one_second(codec: VideoCodec) -> VideoMedia {
    VideoMedia {
        pixel_aspect: Default::default(),
        orientation: crate::schema::VideoOrientation::Identity,
        codec,
        bit_depth: 8,
        colour: None,
        width: 1920,
        height: 1080,
        timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, crate::schema::TICKS),
    }
}

/// Converts the one-clip document with Eye Contact recorded on its layer.
fn export(
    enabled: bool,
    facts: BTreeMap<String, VideoMedia>,
) -> crate::error::Result<(crate::format::PrProjectFile, Vec<Omission>)> {
    let mut wire = editable_document();
    // The schema writes `enabled` only when true; an explicit `false` would
    // not round-trip and would be reported as an unknown field.
    let mut eye_contact = json!({"eyeContactAssetId": EYE_CONTACT});
    if enabled {
        eye_contact["enabled"] = json!(true);
    }
    wire["composition"]["layers"][0]["source"]["eyeContact"] = eye_contact;
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let facts: BTreeMap<_, _> = facts
        .into_iter()
        .map(|(asset_id, video)| (asset_id, MediaFacts::Video(video)))
        .collect();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &facts,
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )?;
    Ok((project, omissions))
}

fn both_assets() -> BTreeMap<String, VideoMedia> {
    BTreeMap::from([
        (ORIGINAL.to_owned(), one_second(VideoCodec::H264)),
        (EYE_CONTACT.to_owned(), one_second(VideoCodec::HevcMain)),
    ])
}

fn exported_media(project: &crate::format::PrProjectFile) -> Vec<&str> {
    project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .map(|clip| clip.media_id().as_str())
        .collect()
}

#[test]
fn enabled_eye_contact_exports_the_active_replacement_with_its_own_codec() {
    let (project, omissions) = export(true, both_assets()).unwrap();
    assert_eq!(exported_media(&project), [EYE_CONTACT]);
    assert_eq!(
        project
            .media
            .keys()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        [EYE_CONTACT]
    );
    assert_eq!(
        project
            .media
            .values()
            .next()
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .kind,
        PrMediaKind::Video {
            codec: Some(VideoCodec::HevcMain),
            hdr_profile: None,
        }
    );
    let reasons: Vec<_> = omissions.iter().map(|item| item.reason.as_str()).collect();
    assert_eq!(
        reasons,
        [
            "Eye Contact exported as its active output \"eye-contact-output\"; the original picture lineage \"premiere-video-1\" and the Eye Contact toggle were not exported"
        ]
    );
}

#[test]
fn disabled_eye_contact_exports_the_original_and_reports_the_unused_output() {
    let (project, omissions) = export(false, both_assets()).unwrap();
    assert_eq!(exported_media(&project), [ORIGINAL]);
    assert_eq!(
        project
            .media
            .values()
            .next()
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .kind,
        PrMediaKind::Video {
            codec: Some(VideoCodec::H264),
            hdr_profile: None,
        }
    );
    let reasons: Vec<_> = omissions.iter().map(|item| item.reason.as_str()).collect();
    assert_eq!(
        reasons,
        ["inactive Eye Contact output \"eye-contact-output\" was not exported"]
    );
}

#[test]
fn eye_contact_duration_mismatch_names_the_active_output() {
    // `sourceIntrinsicDuration` (1000 ms) describes the original upload.
    let two_second_output = || {
        let mut facts = both_assets();
        facts.get_mut(EYE_CONTACT).unwrap().timing =
            crate::media::VideoTiming::for_test(FrameRate::Fps30, 2 * crate::schema::TICKS);
        facts
    };
    let error = export(true, two_second_output()).unwrap_err().to_string();
    assert!(
        error.ends_with(
            "sourceIntrinsicDuration 1000 ms differs from the packaged MP4 duration 2000 ms of the active Eye Contact output \"eye-contact-output\""
        ),
        "{error}"
    );
    // Disabled, the unused output's duration does not matter.
    let (project, _) = export(false, two_second_output()).unwrap();
    assert_eq!(exported_media(&project), [ORIGINAL]);
}

#[test]
fn active_replacement_requires_its_own_inspected_media() {
    // Only the original was inspected: export must not fall back to it.
    let original_only = BTreeMap::from([(ORIGINAL.to_owned(), one_second(VideoCodec::H264))]);
    let error = export(true, original_only).unwrap_err().to_string();
    assert!(
        error.contains("missing inspected media for asset \"eye-contact-output\""),
        "{error}"
    );
}

#[test]
fn active_replacement_keeps_playback_and_wipe_export_and_rejects_retimed_duration_mismatch() {
    // Synthetic replacements on pinned native fixtures prove structural
    // independence, not Adobe support for an Eye Contact/retiming combination.
    for name in [
        "feature_constant_reverse_0_905_strict.prproj",
        "feature_frame_blending_half_speed_strict.prproj",
        "feature_linear_wipe_strict.prproj",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let (native, omissions) = crate::format::PrProjectFile::load(path).unwrap();
        assert!(omissions.is_empty(), "{name}: {omissions:?}");
        let sequence = native.single_sequence().unwrap();
        let ids = crate::tesseract_output::asset_ids_in_order(sequence, &native.media);
        let mut omissions = Vec::new();
        let document =
            crate::convert::premiere_to_tesseract(sequence, &native.media, &ids, &mut omissions)
                .unwrap();
        assert!(omissions.is_empty(), "{name}: {omissions:?}");
        let mut facts = BTreeMap::new();
        for (id, asset) in &ids {
            let media = native.media[id].video.as_ref().unwrap();
            let video = VideoMedia {
                pixel_aspect: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                codec: VideoCodec::H264,
                bit_depth: 8,
                colour: None,
                width: media.width,
                height: media.height,
                timing: crate::media::VideoTiming::for_test(
                    media.frame_rate.supported().unwrap(),
                    media.intrinsic_ticks,
                ),
            };
            facts.insert(asset.as_str().to_owned(), MediaFacts::Video(video));
        }
        let mut expected_omissions = Vec::new();
        let expected = tesseract_to_premiere(
            &document,
            &facts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            crate::format::FrameRate::Fps30,
            &mut expected_omissions,
        )
        .unwrap();
        let mut wire = document.to_json_value().unwrap();
        for layer in wire["composition"]["layers"].as_array_mut().unwrap() {
            if layer["type"] != "Video" {
                continue;
            }
            let original = layer["source"]["assetId"].as_str().unwrap();
            let replacement = format!("{original}-eye-contact");
            let video = facts.remove(original).unwrap();
            facts.insert(replacement.clone(), video);
            layer["source"]["eyeContact"] =
                json!({"enabled": true, "eyeContactAssetId": replacement});
        }
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let mut omissions = Vec::new();
        let actual = tesseract_to_premiere(
            &document,
            &facts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            crate::format::FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        let expected_clips: Vec<_> = expected
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .collect();
        let actual_clips: Vec<_> = actual
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .collect();
        assert_eq!(actual_clips.len(), expected_clips.len(), "{name}");
        for (actual, expected) in actual_clips.iter().zip(&expected_clips) {
            assert_eq!(
                actual.media.as_str(),
                format!("{}-eye-contact", expected.media.as_str())
            );
            assert_eq!(actual.timeline_ticks(), expected.timeline_ticks(), "{name}");
            assert_eq!(actual.source_ticks(), expected.source_ticks(), "{name}");
            assert_eq!(actual.playback_rate, expected.playback_rate, "{name}");
            assert_eq!(actual.frame_blending, expected.frame_blending, "{name}");
            let wipe = |clip: &crate::schema::PrVideoOccurrence| {
                clip.linear_wipe.as_ref().map(|wipe| {
                    (
                        wipe.initial_completion,
                        wipe.angle_degrees,
                        wipe.feather,
                        wipe.completion
                            .iter()
                            .map(|key| (key.source_ticks, key.value, key.easing))
                            .collect::<Vec<_>>(),
                    )
                })
            };
            assert_eq!(wipe(actual), wipe(expected), "{name}");
        }
        assert_eq!(
            omissions.len(),
            // Replacement loss belongs to each authored clip, not each
            // speed/hold segment emitted from that clip.
            expected_omissions.len() + sequence.video_occurrences().count(),
            "{name}: {omissions:?}"
        );
        omissions.retain(|item| !item.reason.starts_with("Eye Contact exported as"));
        assert_eq!(omissions, expected_omissions, "{name}");

        let replacement = actual_clips[0].media.as_str();
        let MediaFacts::Video(video) = facts.get_mut(replacement).unwrap() else {
            panic!("replacement must contain video");
        };
        assert_eq!(
            video.timing.supported().unwrap().1,
            10 * crate::schema::TICKS
        );
        video.timing.sample_count += 30;
        let error = tesseract_to_premiere(
            &document,
            &facts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            crate::format::FrameRate::Fps30,
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.ends_with(&format!(
                "sourceIntrinsicDuration 10000 ms differs from the packaged MP4 duration 11000 ms of the active Eye Contact output {replacement:?}"
            )),
            "{name}: {error}"
        );
    }
}
