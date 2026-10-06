//! Still-image media detection from native `VideoStream`/`Media` records.

use crate::error::{ensure, unsupported, Result};
use crate::format::Located;
use crate::schema::{
    adjustment,
    native::{Media, VideoStream},
    EditKind, OccurrenceEdit, PrMediaKind, PrVideoOccurrence, OPENEXR_ALPHA_TYPE,
    OPENEXR_CODEC_TYPE, STILL_STRAIGHT_ALPHA_TYPE,
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
    let numbered = match stream.value.is_numbered_stills.as_deref() {
        None | Some("false") => false,
        Some("true") => true,
        Some(value) => {
            return Err(unsupported(format!(
                "{}: unsupported IsNumberedStills value {value:?}",
                stream.identity
            )))
        }
    };
    if !numbered && stream.value.is_still.is_none() {
        return Ok(PrMediaKind::Video {
            codec: None,
            hdr_profile: None,
        });
    }
    if numbered {
        ensure!(
            stream
                .value
                .is_still
                .as_deref()
                .is_none_or(|value| value == "false"),
            "{}: conflicting IsStill/IsNumberedStills declarations",
            stream.identity
        );
    } else {
        ensure!(
            stream.value.is_still.as_deref() == Some("true"),
            "{}: unsupported IsStill value {:?}",
            stream.identity,
            stream.value.is_still
        );
    }
    if let Some(code) = generator_code(&media.value) {
        return Err(unsupported(format!(
            "{}: synthetic still media {code} (a generated matte, title, or graphic rather than an image file) is unsupported; only PNG/JPEG/OpenEXR file stills convert",
            media.identity
        )));
    }
    if numbered {
        ensure!(
            media
                .value
                .infinite
                .as_deref()
                .is_none_or(|value| value == "false"),
            "{}: numbered stills must have a finite source clock",
            media.identity
        );
    } else if let Some(infinite) = media.value.infinite.as_deref() {
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
    let open_exr = stream.value.codec_type.as_deref() == Some(OPENEXR_CODEC_TYPE);
    let alpha = match stream.value.alpha_type.as_deref() {
        None => false,
        Some(STILL_STRAIGHT_ALPHA_TYPE) if !open_exr => true,
        Some(OPENEXR_ALPHA_TYPE) if open_exr => true,
        Some(other) => {
            return Err(unsupported(format!(
                "{}: still AlphaType {other:?} is unsupported for its codec",
                stream.identity
            )))
        }
    };
    ensure!(
        !(alpha && stream.value.ignore_alpha.as_deref() == Some("true")),
        "{}: still with IgnoreAlpha would flatten its transparency",
        stream.identity
    );
    Ok(if open_exr {
        PrMediaKind::OpenExr {
            alpha,
            numbered,
            channels: crate::schema::OpenExrChannels::Unspecified,
        }
    } else if numbered {
        PrMediaKind::NumberedStills { alpha }
    } else {
        PrMediaKind::Still { alpha }
    })
}

/// Whether one converted occurrence of `kind` media is kept, recording why not.
///
/// Video is always kept. A still's Motion, Opacity and their keys convert as a
/// video's do, and so do its static Crop (a Crop effect or the Motion Crop,
/// either of which crops the still's own frame before Motion) and its Opacity
/// mask: import draws each as the image's Crop or mask guide, as for a flat
/// video, and imports the still's other effects as a flat video's after it,
/// reporting those that apply before it (`effects::import_still_effects`).
/// `mask_boundary` omits a still with both, as it omits any clip with two
/// masks. A Linear Wipe or Track
/// Matte Key on a still does not convert (a still exports neither, nor its
/// Opacity mask), and the Color Matte mapping carries only a static Opacity,
/// as its rectangle's, of the picture and transition edits: the first other
/// such edit omits the occurrence. Neither has a time-varying picture: its
/// placement survives a clock edit, which is reported as lost. An adjustment
/// layer keeps Opacity and its keys ([`adjustment::retains_edit`]), plus
/// measured static Motion coverage ([`adjustment::supports_motion_coverage`]);
/// other edits omit it.
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
        PrMediaKind::NumberedStills { .. } | PrMediaKind::OpenExr { numbered: true, .. } => {
            if let Some(reason) = crate::numbered_images::unsupported_occurrence(clip) {
                omit(omissions, OmissionScope::Occurrence, clip.record(),
                    format!("track {track_index}, range {}..{} ticks: {reason}; numbered-image occurrence omitted", clip.start_ticks, clip.end_ticks));
                return false;
            }
            return true;
        }
        PrMediaKind::Still { .. }
        | PrMediaKind::OpenExr {
            numbered: false, ..
        } => ("a still image", Some("a still")),
        PrMediaKind::ColorMatte(_) => ("a Color Matte", Some("a solid")),
        PrMediaKind::Adjustment => ("an adjustment layer", None),
    };
    let record = clip.id.clone().unwrap_or_default();
    let context = format!(
        "track {track_index}, range {}..{} ticks",
        clip.start_ticks, clip.end_ticks
    );
    let edits = clip.edits();
    let sharp_matte_crop = matches!(kind, PrMediaKind::ColorMatte(_))
        && clip.crop.edge_feather == 0.0
        && clip.effects.is_empty()
        && clip
            .source_effects
            .as_ref()
            .is_none_or(|source| source.effects.is_empty() && source.active_transforms == 0)
        && clip.track_matte.is_none()
        && clip.opacity_mask.is_none();
    // One ordinary vector mask uses the rectangle's source-frame guide. Mixed
    // coverage/effect stages retain their existing admission restrictions.
    let ordinary_matte_mask = clip.opacity_mask.is_some()
        && clip.crop.is_default()
        && clip.track_matte.is_none()
        && clip.linear_wipe.is_none()
        && clip.effects.is_empty()
        && clip
            .source_effects
            .as_ref()
            .is_none_or(|source| source.effects.is_empty() && source.active_transforms == 0);
    let omitting = edits.iter().find(|edit| match kind {
        PrMediaKind::Adjustment => {
            !(adjustment::retains_edit(**edit)
                || matches!(**edit, OccurrenceEdit::Position | OccurrenceEdit::Scale)
                    && adjustment::supports_motion_coverage(clip)
                || **edit == OccurrenceEdit::LinearWipe && adjustment::supports_wipe_coverage(clip))
        }
        PrMediaKind::Still { .. }
        | PrMediaKind::OpenExr {
            numbered: false, ..
        } => matches!(
            **edit,
            OccurrenceEdit::LinearWipe | OccurrenceEdit::TrackMatte
        ),
        // A Color Matte's rectangle and sharp Crop guide share Motion. A
        // Track Matte consumer still has no staged geometric Motion owner.
        PrMediaKind::ColorMatte(_) => {
            !(matches!(
                **edit,
                OccurrenceEdit::Opacity | OccurrenceEdit::OpacityKeys | OccurrenceEdit::TrackMatte
            ) || clip.track_matte.is_none()
                && matches!(
                    **edit,
                    OccurrenceEdit::MotionKeys
                        | OccurrenceEdit::Position
                        | OccurrenceEdit::AnchorPoint
                        | OccurrenceEdit::Scale
                        | OccurrenceEdit::Rotation
                )
                || **edit == OccurrenceEdit::Crop && sharp_matte_crop
                || **edit == OccurrenceEdit::OpacityMask && ordinary_matte_mask
                || clip.track_matte.is_none() && edit.kind() == EditKind::Clock)
        }
        PrMediaKind::Video { .. }
        | PrMediaKind::AfterEffectsComposition(_)
        | PrMediaKind::NumberedStills { .. }
        | PrMediaKind::OpenExr { numbered: true, .. } => unreachable!("returned above"),
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
    // Motion, Crop, Opacity mask, Opacity and keys; a still's or solid's
    // surviving clock edits are lost.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        schema::{
            PrColorMatte, PrEffect, PrEffectParams, PrGaussianBlur, PrKeyframeEasing, PrLinearWipe,
            PrMatteChannel, PrPropertyAnimation, PrScalarKeyframe, PrSourceEffects, PrTrackMatte,
            TICKS,
        },
        tests::support::{clip_of, opacity_mask},
    };

    fn blur() -> PrEffect {
        PrEffect {
            enabled: true,
            mask: None,
            params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                blurriness: 5.0,
                repeat_edge_pixels: false,
            }),
            animations: Vec::new(),
        }
    }

    #[test]
    fn sharp_crop_color_matte_admission_preserves_other_picture_and_effect_guards() {
        let matte = PrMediaKind::ColorMatte(PrColorMatte {
            rgb: [243, 215, 169],
        });
        let mut plain = clip_of("matte", 0..TICKS, 0);
        plain.crop.left = 50.0;
        plain.opacity = 47.0;
        let mut omissions = Vec::new();
        assert!(keep_occurrence(&plain, matte, 0, &mut omissions));
        assert!(omissions.is_empty());
        type EditOccurrence = fn(&mut PrVideoOccurrence);
        let supported: [EditOccurrence; 3] = [
            |clip| clip.transform.position = [0.25, 0.5],
            |clip| {
                clip.animations = vec![PrPropertyAnimation::Rotation(vec![PrScalarKeyframe {
                    source_ticks: 0,
                    value: 15.0,
                    easing: PrKeyframeEasing::Linear,
                }])]
            },
            |clip| {
                clip.animations = vec![PrPropertyAnimation::Opacity(vec![PrScalarKeyframe {
                    source_ticks: 0,
                    value: 50.0,
                    easing: PrKeyframeEasing::Linear,
                }])]
            },
        ];
        for edit in supported {
            let mut clip = plain.clone();
            edit(&mut clip);
            let mut omissions = Vec::new();
            assert!(keep_occurrence(&clip, matte, 0, &mut omissions));
            assert!(omissions.is_empty());
        }
        let cases: [(&str, EditOccurrence); 7] = [
            ("Crop", |clip| clip.crop.edge_feather = 12.0),
            ("Linear Wipe", |clip| {
                clip.linear_wipe = Some(PrLinearWipe {
                    initial_completion: 50.0,
                    completion: Vec::new(),
                    angle_degrees: 90,
                    feather: 0.0,
                })
            }),
            ("Crop", |clip| clip.opacity_mask = Some(opacity_mask())),
            ("Crop", |clip| {
                clip.track_matte = Some(PrTrackMatte {
                    track_index: 1,
                    channel: PrMatteChannel::Alpha,
                })
            }),
            ("Crop", |clip| clip.effects = vec![blur()]),
            ("Crop", |clip| {
                clip.source_effects = Some(PrSourceEffects {
                    master: "MasterClip:source".into(),
                    effects: vec![blur()],
                    active_transforms: 0,
                })
            }),
            ("Crop", |clip| {
                clip.source_effects = Some(PrSourceEffects {
                    master: "MasterClip:source".into(),
                    effects: Vec::new(),
                    active_transforms: 1,
                })
            }),
        ];
        for (reason, edit) in cases {
            let mut clip = plain.clone();
            edit(&mut clip);
            let mut omissions = Vec::new();
            assert!(
                !keep_occurrence(&clip, matte, 0, &mut omissions),
                "{reason}"
            );
            assert_eq!(omissions.len(), 1);
            assert!(omissions[0].reason.contains(reason), "{omissions:?}");
            assert_eq!(omissions[0].scope, OmissionScope::Occurrence);
        }
    }
}
