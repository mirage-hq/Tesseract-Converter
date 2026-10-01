//! Still-image media detection from native `VideoStream`/`Media` records.

use crate::error::{ensure, unsupported, Result};
use crate::format::Located;
use crate::schema::{
    adjustment,
    native::{Media, VideoStream},
    EditKind, OccurrenceEdit, PrMediaKind, PrVideoOccurrence, STILL_STRAIGHT_ALPHA_TYPE,
};
use crate::{omit, Omission, OmissionScope};

/// Classify one media record as video or a file-backed still and read its
/// alpha declaration.
///
/// Observed Premiere stills declare `AlphaType` `1` (straight) for RGBA
/// colour-type-6 PNGs (`cinemagraph`, `phone_title`) and omit it for JPEGs
/// (all 20 corpus JPEG stills). The declaration for grey+alpha, `tRNS`, and
/// opaque PNGs is inferred, so import cross-checks it against the file
/// (`ValidatedImage::declaration_mismatch`). Other alpha modes, and an
/// `IgnoreAlpha` override that would flatten a transparent still, reject so
/// that the converted image cannot silently change appearance.
pub(super) fn media_kind(
    stream: &Located<VideoStream>,
    media: &Located<Media>,
) -> Result<PrMediaKind> {
    let Some(is_still) = stream.value.is_still.as_deref() else {
        return Ok(PrMediaKind::Video {
            codec: None,
            hdr_profile: None,
        });
    };
    ensure!(
        is_still == "true",
        "{}: unsupported IsStill value {is_still:?}",
        stream.identity
    );
    if let Some(code) = generator_code(&media.value) {
        return Err(unsupported(format!(
            "{}: synthetic still media {code} (a generated matte, title, or graphic rather than an image file) is unsupported; only PNG/JPEG file stills convert",
            media.identity
        )));
    }
    if let Some(infinite) = media.value.infinite.as_deref() {
        ensure!(
            infinite == "true",
            "{}: still media must be Infinite",
            media.identity
        );
    }
    // Observed `true` on 76 corpus still streams, never on video; its effect
    // is unknown and neither direction maps it, so it is read but not kept.
    if let Some(overridden) = stream.value.is_overriden_image_orientation_type.as_deref() {
        ensure!(
            matches!(overridden, "true" | "false"),
            "{}: unsupported IsOverridenImageOrientationType value {overridden:?}",
            stream.identity
        );
    }
    let alpha = match stream.value.alpha_type.as_deref() {
        None => false,
        Some(STILL_STRAIGHT_ALPHA_TYPE) => true,
        Some(other) => {
            return Err(unsupported(format!(
                "{}: still AlphaType {other:?} is unsupported; only absent (opaque) or 1 (straight alpha) stills convert",
                stream.identity
            )))
        }
    };
    ensure!(
        !(alpha && stream.value.ignore_alpha.as_deref() == Some("true")),
        "{}: still with IgnoreAlpha would flatten its transparency",
        stream.identity
    );
    Ok(PrMediaKind::Still { alpha })
}

/// Whether one converted occurrence of `kind` media is kept, recording why not.
///
/// Video is always kept. A still's Motion, Opacity and their keys convert as a
/// video's do, but a Crop, Linear Wipe, Opacity mask or Track Matte Key on a
/// still does not (a still exports none either), and the Color Matte mapping
/// carries only a static Opacity, as its rectangle's, of the picture and
/// transition edits: the first other such edit omits the occurrence. Neither
/// has a time-varying picture: its placement survives a clock edit, which is
/// reported as lost. An adjustment layer keeps Opacity
/// and its keys ([`adjustment::retains_edit`]), plus measured static Motion
/// coverage ([`adjustment::supports_motion_coverage`]); other edits omit it.
///
/// Import gives an image layer no Track Matte Key, so it would draw a keyed
/// still unkeyed: the key omits the still, and `consume_claimed_mattes` drops
/// its matte clips, which Premiere does not draw while the key names them
/// (fixture G1b).
pub(super) fn keep_occurrence(
    clip: &PrVideoOccurrence,
    kind: PrMediaKind,
    track_index: i64,
    omissions: &mut Vec<Omission>,
) -> bool {
    let (media, picture) = match kind {
        PrMediaKind::Video { .. } | PrMediaKind::AfterEffectsComposition(_) => return true,
        PrMediaKind::Still { .. } => ("a still image", Some("a still")),
        PrMediaKind::ColorMatte(_) => ("a Color Matte", Some("a solid")),
        PrMediaKind::Adjustment => ("an adjustment layer", None),
    };
    let record = clip.id.clone().unwrap_or_default();
    let context = format!(
        "track {track_index}, range {}..{} ticks",
        clip.start_ticks, clip.end_ticks
    );
    let edits = clip.edits();
    let omitting = edits.iter().find(|edit| match kind {
        PrMediaKind::Adjustment => {
            !(adjustment::retains_edit(**edit)
                || matches!(**edit, OccurrenceEdit::Position | OccurrenceEdit::Scale)
                    && adjustment::supports_motion_coverage(clip))
        }
        PrMediaKind::Still { .. } => matches!(
            **edit,
            OccurrenceEdit::Crop
                | OccurrenceEdit::LinearWipe
                | OccurrenceEdit::OpacityMask
                | OccurrenceEdit::TrackMatte
        ),
        // A Color Matte: its rectangle carries a static Opacity.
        _ => **edit != OccurrenceEdit::Opacity && edit.kind() != EditKind::Clock,
    });
    if let Some(edit) = omitting {
        let feature = edit.label();
        omit(
            omissions,
            OmissionScope::Occurrence,
            record,
            format!("{context}: {feature} on {media} is unsupported; occurrence omitted"),
        );
        return false;
    }
    // Every surviving edit of an adjustment layer converts, and so do a still's
    // Motion, Opacity and keys; a still's or solid's surviving clock edits are
    // lost.
    let Some(picture) = picture else {
        return true;
    };
    for edit in edits.iter().filter(|edit| edit.kind() == EditKind::Clock) {
        let property = edit.label();
        omit(
            omissions,
            OmissionScope::Feature,
            record.clone(),
            format!("{context}: {property} on {media} is not retained; {picture} has no time-varying picture"),
        );
    }
    true
}

/// The four-character code of Premiere generator media, if `media` is one.
///
/// Color Matte (`COLR`), Black Video (`BLAK`), Transparent Video (`TRNV`),
/// titles (`TITL`), and graphics (`GRFV`) are `IsStill` media in the corpus
/// whose `FilePath` holds that code in decimal and which have no
/// `RelativePath`, so they never name an image file.
fn generator_code(media: &Media) -> Option<String> {
    if !media.relative_paths.is_empty() {
        return None;
    }
    let code: u32 = media.file_path.as_deref()?.parse().ok()?;
    let bytes = code.to_be_bytes();
    Some(
        if bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            bytes.iter().copied().map(char::from).collect()
        } else {
            code.to_string()
        },
    )
}
