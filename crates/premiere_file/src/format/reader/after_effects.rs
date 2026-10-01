//! The bounded Dynamic Link media profile observed in the native hybrid fixture.
//!
//! A linked composition's audio stream is read like any media's; its audio
//! track items are omitted by the audio reader, because linked-audio
//! occurrence import is not implemented.

use super::required;
use crate::{
    error::{ensure, unsupported, Result},
    format::{Graph, Located},
    schema::{
        after_effects::{CODEC, IMPORTER_ID},
        native::{Media, VideoStream},
        ColorSpace, PrAfterEffectsComposition, PrMediaKind,
    },
};
use base64::{engine::general_purpose::STANDARD, Engine};

pub(super) fn media_kind(
    graph: &Graph<'_>,
    media: &Located<Media>,
    stream: &Located<VideoStream>,
) -> Result<Option<PrMediaKind>> {
    if media.value.implementation_id.as_deref() != Some(IMPORTER_ID) {
        ensure!(
            stream.value.codec_type.as_deref() != Some(CODEC),
            "{}: After Effects codec without its Dynamic Link importer",
            media.identity
        );
        return Ok(None);
    }
    let identity = &media.identity;
    ensure!(
        media
            .value
            .infinite
            .as_deref()
            .is_none_or(|value| value == "false"),
        "{identity}: infinite After Effects composition is unsupported"
    );
    let prefs = required(
        media.value.importer_prefs.as_ref(),
        identity,
        "ImporterPrefs",
    )?;
    ensure!(
        prefs.encoding == "base64",
        "{identity}: After Effects ImporterPrefs must be base64"
    );
    // Resolve even an inline value so conflicting definitions fail rather than
    // allowing the same BinaryHash to select two different compositions.
    let stored = graph.binary_value(&prefs.binary_hash, identity)?;
    let value = if prefs.value.trim().is_empty() {
        required(stored, identity, "ImporterPrefs binary value")?
    } else {
        &prefs.value
    };
    // 36 ASCII GUID characters encoded as UTF16LE: exactly 72 bytes / 96 base64
    // characters. Ignore native XML whitespace, never accept an opaque payload.
    let encoded: String = value
        .chars()
        .filter(|value| !value.is_ascii_whitespace())
        .take(97)
        .collect();
    ensure!(
        encoded.len() == 96,
        "{identity}: After Effects ImporterPrefs must encode one 36-character GUID"
    );
    let bytes = STANDARD.decode(encoded).map_err(|error| {
        unsupported(format!(
            "{identity}: invalid After Effects ImporterPrefs: {error}"
        ))
    })?;
    ensure!(
        bytes.len() == 72,
        "{identity}: invalid After Effects GUID byte count"
    );
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let guid = String::from_utf16(&units).map_err(|error| {
        unsupported(format!(
            "{identity}: invalid After Effects GUID UTF16LE: {error}"
        ))
    })?;
    let composition = PrAfterEffectsComposition::parse(&guid).ok_or_else(|| {
        unsupported(format!(
            "{identity}: invalid After Effects composition GUID"
        ))
    })?;
    let video = &stream.value;
    ensure!(
        video.codec_type.as_deref() == Some(CODEC),
        "{identity}: unsupported After Effects codec"
    );
    ensure!(
        video.alpha_type.as_deref() == Some("1")
            && video
                .ignore_alpha
                .as_deref()
                .is_none_or(|value| value == "false"),
        "{identity}: After Effects link requires straight alpha without IgnoreAlpha"
    );
    ensure!(
        video.original_field_type.as_deref() == Some("4"),
        "{identity}: After Effects link requires the observed progressive field type"
    );
    ensure!(
        video
            .is_still
            .as_deref()
            .is_none_or(|value| value == "false")
            && video
                .is_continuous_time
                .as_deref()
                .is_none_or(|value| value == "false"),
        "{identity}: still or continuous-time After Effects links are unsupported"
    );
    ensure!(
        video
            .alpha_info_is_uncertain
            .as_deref()
            .is_none_or(|value| value == "false")
            && video
                .field_type_is_uncertain
                .as_deref()
                .is_none_or(|value| value == "false"),
        "{identity}: uncertain After Effects alpha or field interpretation is unsupported"
    );
    let color: ColorSpace = serde_json::from_str(required(
        video.original_color_space.as_deref(),
        identity,
        "OriginalColorSpace",
    )?)
    .map_err(|error| {
        unsupported(format!(
            "{identity}: invalid After Effects color profile: {error}"
        ))
    })?;
    ensure!(
        color == ColorSpace::sequence_sdr(),
        "{identity}: unsupported After Effects color interpretation"
    );
    Ok(Some(PrMediaKind::AfterEffectsComposition(composition)))
}
