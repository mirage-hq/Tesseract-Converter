//! Premiere numbered stills: a finite consecutive filename span on a source clock.
pub(crate) mod sampling;
use crate::{
    error::{ensure, unsupported, Result},
    schema::{PrVideoOccurrence, PrVideoStream, TICKS_PER_MILLISECOND},
};
use fx_schema::AssetId;
use std::path::{Path, PathBuf};

/// Bound native metadata before allocating paths or editable frame layers.
pub(crate) const MAX_FRAMES: i64 = 100_000;

pub(crate) fn frame_count(source: &PrVideoStream) -> Result<usize> {
    let step = source.frame_rate.ticks_per_frame();
    ensure!(
        step >= TICKS_PER_MILLISECOND,
        "numbered-image source cadence is faster than editable millisecond timing"
    );
    ensure!(
        source.intrinsic_ticks > 0 && source.intrinsic_ticks % step == 0,
        "numbered-image Duration must be a positive whole number of source frames"
    );
    let count = source.intrinsic_ticks / step;
    ensure!(
        count <= MAX_FRAMES,
        "numbered-image source span exceeds {MAX_FRAMES} frames"
    );
    usize::try_from(count)
        .map_err(|_| unsupported("numbered-image frame count exceeds index range"))
}

/// Saved order is consecutive numeric order, starting at the linked filename,
/// never a directory listing or a lexicographic sort. No gap is compressed.
pub(crate) fn paths(first: &Path, source: &PrVideoStream) -> Result<Vec<PathBuf>> {
    let count = frame_count(source)?;
    let stem = first
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| unsupported("numbered-image filename must be UTF-8"))?;
    ensure!(
        !stem.contains(['/', '\\', '\0']),
        "unsafe numbered-image filename"
    );
    let digit_start = stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (prefix, digits) = stem.split_at(digit_start);
    let start: u64 = digits.parse().map_err(|_| {
        unsupported("numbered-image filename needs a bounded trailing frame number")
    })?;
    let end = start
        .checked_add(count as u64 - 1)
        .ok_or_else(|| unsupported("numbered-image filename span overflows its frame number"))?;
    let extension = first
        .extension()
        .and_then(|value| value.to_str())
        .filter(|extension| crate::image_media::ImageFormat::from_extension(extension).is_some())
        .ok_or_else(|| unsupported("numbered-image frames must be PNG or JPEG files"))?;
    Ok((start..=end)
        .map(|number| {
            first.with_file_name(format!(
                "{prefix}{number:0width$}.{extension}",
                width = digits.len()
            ))
        })
        .collect())
}

pub(crate) fn frame_asset(base: &AssetId, index: usize) -> AssetId {
    AssetId::from_trusted(format!("{}-frame-{index}", base.as_str()))
}

/// Keep the source clock independent of sequence/output cadence. Clock edits
/// not proved by this mapping omit only their occurrence, never sibling media.
pub(crate) fn unsupported_occurrence(clip: &PrVideoOccurrence) -> Option<&'static str> {
    if clip.playback_rate != 1.0 || clip.time_remap.is_some() {
        Some("only unit forward playback without Time Remapping is supported")
    } else if clip.frame_blending.is_some() {
        Some("numbered-image frame blending is unsupported")
    } else if !clip.effects.is_empty() || clip.active_transforms != 0 {
        Some("effects on numbered images are unsupported")
    } else if !clip.crop.is_default()
        || clip.linear_wipe.is_some()
        || clip.opacity_mask.is_some()
        || clip.track_matte.is_some()
    {
        Some("masks or track mattes on numbered images are unsupported")
    } else if clip.stroke.is_some() {
        Some("Film Impact Stroke on numbered images is unsupported")
    } else {
        None
    }
}
