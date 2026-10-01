//! Black Video generator records of an adjustment layer: the media and source
//! that every adjustment clip of the export shares. Its stream is the still
//! stream without alpha (`writer/still.rs`) plus `IsContinuousTime`, which
//! every corpus Black Video stream carries.
//!
//! Where the eight corpus records agree, output follows them: `IsStill`, the
//! `BLAK` paths, `Infinite`, `Title` "Black Video", a zero
//! `ContentAndMetadataState`, `ConformedAudioRate`, no `ImporterPrefs`, and
//! generator clips that carry `BE.Prefs.SyntheticMedia.DefaultIsDropFrame` but
//! no `MarkerOwner`. The project item carries `IsAdjustmentLayer` and each
//! placed clip `AdjustmentLayer` (`writer/media.rs`, `writer/tracks/video.rs`).
//! Invented or unverified, as for Color Matte records:
//! - `ModificationState`: copied from the most common corpus value (four of
//!   the seven records that carry one); its meaning is unknown.
//! - Project-item and clip labels and the `ClipLoggingInfo` timecode format
//!   reuse the file-media writer defaults, unlike corpus adjustment layers.
//!
//! Premiere 26.5.1 reopened one edited export of these records and read its
//! Levels, edge-transparent Gaussian Blur and Opacity key back as written
//! (the JRB-2030 export gate; case `premiere_isolated_adjustment_layer_26_5`).

use super::{graph::MediaIds, media::video_media_source};
use crate::schema::{
    adjustment::{BLACK_VIDEO_FILE_PATH, BLACK_VIDEO_TITLE},
    color_matte::GENERATOR_IMPLEMENTATION_ID,
    native::*,
    records, PrMedia,
};

/// The most common corpus `ModificationState` of Black Video media
/// (`transition_countdown`, `vhs_slideshow`, `vhsvertical`, `food_promo`);
/// `abstract_slideshow` and `corporate_slideshow` write
/// `Ytxu9M6On2lewoS+AAAAQA==`. Unverified, like the Color Matte copy.
const MODIFICATION_STATE: &str = "9G7cYo7OaZ9ewoS+AAAAQA==";
const MODIFICATION_STATE_HASH: &str = "a4f9f615-16ac-e2c1-b049-f6b00000001c";
/// Adobe writes `i64::MAX` for generator media with no audio to conform.
const NO_CONFORMED_AUDIO_RATE: &str = "9223372036854775807";

pub(super) fn source_records(spec: &PrMedia, ids: &MediaIds) -> Vec<Record> {
    vec![
        Record::Media(Media {
            object_id: None,
            object_uid: Some(ids.media.as_native_string()),
            class_id: Some(records::MEDIA.class_id.to_owned()),
            version: Some(records::MEDIA.version.to_owned()),
            video_stream: Some(Reference::object(ids.stream)),
            importer_prefs: None,
            modification_state: Some(ModificationState {
                encoding: records::ENCODING.to_owned(),
                binary_hash: MODIFICATION_STATE_HASH.to_owned(),
                value: MODIFICATION_STATE.to_owned(),
            }),
            relative_paths: Vec::new(),
            file_path: Some(BLACK_VIDEO_FILE_PATH.to_owned()),
            infinite: Some("true".to_owned()),
            implementation_id: Some(GENERATOR_IMPLEMENTATION_ID.to_owned()),
            title: Some(BLACK_VIDEO_TITLE.to_owned()),
            file_key: None,
            content_and_metadata_state: Some(records::ZERO_GUID.to_owned()),
            actual_media_file_path: Some(BLACK_VIDEO_FILE_PATH.to_owned()),
            conformed_audio_rate: Some(NO_CONFORMED_AUDIO_RATE.to_owned()),
            audio_stream: None,
        }),
        video_media_source(
            ids.source,
            ids.media,
            spec.video
                .as_ref()
                .expect("adjustment layer media has a picture stream")
                .intrinsic_ticks,
        ),
    ]
}
