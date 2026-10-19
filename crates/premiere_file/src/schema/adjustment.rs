//! Premiere's adjustment layer: a flagged clip of Black Video generator media
//! whose effects apply to the composite of every lower video track.
//!
//! The native shape is taken from the seven corpus projects that place one
//! (`transition_countdown`, `vhs_slideshow`, `vhsvertical`, `corporate_slideshow`,
//! `abstract_slideshow`, `food_promo`, `lyric_lower_third`; 70 timeline items):
//! the project item is a `MasterClip` with `IsAdjustmentLayer` true over one
//! `VideoClip` of Black Video media (the shared generator `ImplementationID`,
//! the `BLAK` pseudo path, `Title` "Black Video", `Infinite`, an `IsStill`
//! canvas-sized stream), and every placed `VideoClip` carries `AdjustmentLayer`
//! true. The media alone does not identify an adjustment layer: a plain Black
//! Video item is the same record. The two flags do.

use super::{OccurrenceEdit, PrBlendMode, PrEffectParams, PrStaticTransform, PrVideoOccurrence};

/// `FilePath`/`ActualMediaFilePath` of Black Video media: the `BLAK`
/// four-character code.
pub(crate) const BLACK_VIDEO_FILE_PATH: &str = "1112293707";
/// `Media/Title` of Black Video media on every corpus adjustment layer.
pub(crate) const BLACK_VIDEO_TITLE: &str = "Black Video";
/// Default project-item name; user renames are not round-tripped.
pub(crate) const ADJUSTMENT_LAYER_NAME: &str = "Adjustment Layer";

/// Whether an adjustment placement keeps `edit`.
///
/// Premiere applies an adjustment's Opacity as a mix of the effected composite
/// over the untouched one, which the FX adjustment layer's opacity gate also
/// does, so Opacity and its keys convert. Bounded static Motion coverage is
/// admitted separately by [`supports_motion_coverage`], and hard Wipe by
/// [`supports_wipe_coverage`]. Static Crop-only rectangles use
/// [`supports_crop_coverage`]; Motion keys and clock edits remain omitted.
/// Omitting them changes the picture: Premiere renders the cropped region of a
/// Crop-only adjustment black (26.5.1 fixture, G4), not the composite beneath.
pub(crate) fn retains_edit(edit: OccurrenceEdit) -> bool {
    matches!(edit, OccurrenceEdit::Opacity | OccurrenceEdit::OpacityKeys)
}

/// A hard-edge Wipe at unit coverage clips the effected lower
/// composite. Keep that order with the original adjustment in a masked group.
/// Feather would also soften the group's boundary outside the Wipe interval.
pub(crate) fn supports_wipe_coverage(clip: &PrVideoOccurrence, at_sequence_end: bool) -> bool {
    clip.enabled
        && clip.transform == PrStaticTransform::default()
        && clip.opacity == 100.0
        && clip.blend_mode == PrBlendMode::Normal
        && clip.animations.is_empty()
        && clip.playback_rate == 1.0
        && clip.time_remap.is_none()
        && clip.crop.is_default()
        && clip.linear_wipe.as_ref().is_some_and(|wipe| {
            wipe.feather == 0.0
                && (wipe.completion.is_empty()
                    || (wipe
                        .completion
                        .first()
                        .is_some_and(|key| key.source_ticks >= clip.in_ticks)
                        && wipe.completion.last().is_some_and(|key| {
                            at_sequence_end || key.source_ticks < clip.out_ticks
                        })))
        })
        && clip.opacity_mask.is_none()
        && clip.track_matte.is_none()
        && clip.active_transforms == 0
        && (clip.effects_above_mask == 0 || clip.effects_above_mask == clip.effects.len())
}

/// A static hard rectangle clips the lower composite, rather than limiting
/// where an adjustment mixes its effects. Keep mixed effect stacks separate.
pub(crate) fn supports_crop_coverage(clip: &PrVideoOccurrence) -> bool {
    let crop = &clip.crop;
    clip.enabled
        && clip.transform == PrStaticTransform::default()
        && clip.opacity == 100.0
        && clip.blend_mode == PrBlendMode::Normal
        && clip.animations.is_empty()
        && clip.playback_rate == 1.0
        && clip.time_remap.is_none()
        && clip.linear_wipe.is_none()
        && crop.edge_feather == 0.0
        && [crop.left, crop.top, crop.right, crop.bottom]
            .into_iter()
            .all(|edge| (0.0..=100.0).contains(&edge))
        && crop.left + crop.right <= 100.0
        && crop.top + crop.bottom <= 100.0
        && clip.opacity_mask.is_none()
        && clip.track_matte.is_none()
        && clip.active_transforms == 0
        && clip.effects.is_empty()
        && clip
            .source_effects
            .as_ref()
            .is_none_or(|source| source.effects.is_empty() && source.active_transforms == 0)
}

/// The measured axis-aligned Motion coverage of A3: moving or uniformly
/// scaling a canvas-sized adjustment changes which pixels its effects reach,
/// without moving those pixels. Only no effect or static full RGB Invert is
/// admitted. Offset also keeps this coverage as a diagnosed approximation;
/// other spatial effects, mixing and Motion animation remain unmeasured here.
pub(crate) fn supports_motion_coverage(clip: &PrVideoOccurrence) -> bool {
    clip.transform.anchor_point == PrStaticTransform::default().anchor_point
        && clip.transform.rotation == 0.0
        && clip.transform.scale[0] > 0.0
        && clip.transform.scale[0] == clip.transform.scale[1]
        && clip.opacity == 100.0
        && clip.blend_mode == PrBlendMode::Normal
        && clip.animations.is_empty()
        && clip.playback_rate == 1.0
        && clip.time_remap.is_none()
        && clip.crop.is_default()
        && clip.linear_wipe.is_none()
        && clip.opacity_mask.is_none()
        && clip.track_matte.is_none()
        && clip.active_transforms == 0
        && clip
            .effects
            .iter()
            .filter(|effect| effect.enabled)
            .all(|effect| {
                matches!(effect.params, PrEffectParams::Offset(_))
                    || matches!(effect.params, PrEffectParams::Invert(invert) if invert.blend == 0.0)
                        && effect.animations.is_empty()
            })
}
