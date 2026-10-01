//! Color Matte generator records: the media and source of one colour. Its
//! stream is the still stream without alpha (`writer/still.rs`).
//!
//! Where every corpus matte agrees, output follows it: `IsStill`, the `COLR`
//! paths, `Infinite`, the 8-byte `ImporterPrefs`, a zero
//! `ContentAndMetadataState`, `ConformedAudioRate`, and generator clips that
//! carry `BE.Prefs.SyntheticMedia.DefaultIsDropFrame` but no `MarkerOwner` (so
//! no `Markers` record). Invented or unverified, because the corpus is not
//! unanimous or its values are not understood:
//! - `ImporterPrefs` `BinaryHash`: Adobe's is derived from the blob (every
//!   black matte in `countdown_title`, `lower_third` and `credits` carries
//!   `8faeedf7-eb02-d2a5-c178-492000000014`) by an unidentified digest; this
//!   writer emits a random UUID.
//! - `ModificationState`: copied from the most common corpus value; see
//!   [`MODIFICATION_STATE`].
//! - `VideoStream`: the older `corporate_slideshow` (Version 16, the only
//!   30 fps mattes) also writes `IsOverridenImageOrientationType=true`, and
//!   Version 19 mattes write colour-space and input-LUT fields; the writer's
//!   Version 22 stream emits none of them.
//! - Project-item and clip labels and the `ClipLoggingInfo` media range reuse
//!   the file-media writer defaults, unlike corpus mattes.
//!
//! A Premiere 26.x reopen of generated output is not yet verified.

use super::{
    graph::MediaIds,
    media::{media_clip, video_media_source},
};
use crate::schema::{
    color_matte::{COLOR_MATTE_FILE_PATH, GENERATOR_IMPLEMENTATION_ID},
    native::*,
    records, PrColorMatte, PrMedia,
};
use base64::{engine::general_purpose::STANDARD, Engine};

/// The most common corpus `ModificationState` of a matte (`lower_third`,
/// `credits`, `countdown_title`, `phone_title`, `horror_title`, among others).
/// Adobe's value is not constant: `corporate_slideshow` writes
/// `miNxGBctgDz7m/zIAAAAQA==` and `cinematic_vertical` writes
/// `5Cq1BiG2Dv/WPLr0AAAAQA==`. Its meaning is unknown, so this copy is
/// unverified.
const MODIFICATION_STATE: &str = "GHEjmi0XPID7m/zIAAAAQA==";
const MODIFICATION_STATE_HASH: &str = "e36a7dbe-b1a9-af63-b399-e5190000001c";
/// Adobe writes `i64::MAX` for generator media with no audio to conform.
const NO_CONFORMED_AUDIO_RATE: &str = "9223372036854775807";
/// The default still duration of a new matte's master clip, in seconds; the
/// clip ends on the last whole frame of the matte's rate within it
/// ([`FrameRate::whole_frame_ticks`](crate::schema::FrameRate::whole_frame_ticks)).
///
/// `corporate_slideshow` (the only 30 fps corpus mattes) has a
/// `0..1270080000000` template, which is 150 frames; 29.97 fps corpus mattes
/// have 149 frames (`1262874412800`), and the 24 fps matte of `horror_title`
/// has 120.
const TEMPLATE_CLIP_SECONDS: i64 = 5;
/// `BE.Prefs.SyntheticMedia.DefaultIsDropFrame` on every generator clip.
///
/// Every corpus matte clip carries the preference. `corporate_slideshow`, the
/// only 30 fps evidence, writes `false`; 30 fps has no drop-frame timecode.
pub(super) const DEFAULT_IS_DROP_FRAME: &str = "false";

pub(super) fn source_records(spec: &PrMedia, matte: PrColorMatte, ids: &MediaIds) -> Vec<Record> {
    vec![
        Record::Media(Media {
            object_id: None,
            object_uid: Some(ids.media.as_native_string()),
            class_id: Some(records::MEDIA.class_id.to_owned()),
            version: Some(records::MEDIA.version.to_owned()),
            video_stream: Some(Reference::object(ids.stream)),
            importer_prefs: Some(ImporterPrefs {
                encoding: records::ENCODING.to_owned(),
                binary_hash: ids.media_binary_hash.clone(),
                value: STANDARD.encode(matte.importer_prefs()),
            }),
            modification_state: Some(ModificationState {
                encoding: records::ENCODING.to_owned(),
                binary_hash: MODIFICATION_STATE_HASH.to_owned(),
                value: MODIFICATION_STATE.to_owned(),
            }),
            relative_paths: Vec::new(),
            file_path: Some(COLOR_MATTE_FILE_PATH.to_owned()),
            infinite: Some("true".to_owned()),
            implementation_id: Some(GENERATOR_IMPLEMENTATION_ID.to_owned()),
            title: Some(spec.name.clone()),
            file_key: None,
            content_and_metadata_state: Some(records::ZERO_GUID.to_owned()),
            actual_media_file_path: Some(COLOR_MATTE_FILE_PATH.to_owned()),
            conformed_audio_rate: Some(NO_CONFORMED_AUDIO_RATE.to_owned()),
            audio_stream: None,
        }),
        video_media_source(
            ids.source,
            ids.media,
            spec.video
                .as_ref()
                .expect("matte has a picture stream")
                .intrinsic_ticks,
        ),
    ]
}

/// The unused master clip; unlike file media, Adobe gives it an explicit range.
pub(super) fn template_clip(spec: &PrMedia, ids: &MediaIds) -> Clip {
    let mut clip = media_clip(spec, None, ids, ids.template_clip_uid.clone());
    clip.in_point = Some(0.to_string());
    clip.out_point = Some(
        spec.video
            .as_ref()
            .expect("matte has a picture stream")
            .frame_rate
            .supported()
            .expect("matte has a supported synthetic clock")
            .whole_frame_ticks(TEMPLATE_CLIP_SECONDS)
            .to_string(),
    );
    clip
}
