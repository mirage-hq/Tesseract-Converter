//! Lower the native Speed ramp subset without baking or replacing its keys.

use super::{keyframes, timing::ticks_from_time};
use crate::schema::{PrTimeRemap, PrTimeRemapKeyframe, TICKS_PER_MILLISECOND};
use fx_schema::VideoLayer;

pub(super) struct NativeRamp {
    pub(super) in_ticks: i64,
    pub(super) out_ticks: i64,
    pub(super) curve: PrTimeRemap,
}

/// The selected input window on the saved Speed clock, with its full curve.
/// Unsupported curves return `None` so the caller retains its existing loss
/// handling. Covered windows never use the curve's extrapolation.
pub(super) fn native_ramp(video: &VideoLayer) -> Option<NativeRamp> {
    let property = video.playback.time_remap()?;
    let keys = property.keyframes();
    let first = keys.first()?;
    let last = keys.last()?;
    let range = video.playback.input_range();
    let start = i128::from(range.start.as_millis()) + i128::from(video.playback.input_offset_ms());
    let end = start + i128::from(range.duration.as_millis());
    // A rounded media-end key can exceed the packaged duration. Omit that
    // curve before lowering instead of aborting an otherwise usable export.
    if start < i128::from(first.time.as_millis())
        || end > i128::from(last.time.as_millis())
        || start >= end
        || keys.iter().any(|key| {
            key.value < video.source_range.start
                || key.value > video.source_range.end()
                || key.value.as_millis() >= video.source_intrinsic_duration.as_millis()
        })
    {
        return None;
    }
    let to_ticks =
        |millis: i128| i64::try_from(millis.checked_mul(i128::from(TICKS_PER_MILLISECOND))?).ok();
    // Normalize only the input origin. Source values and all authored keys
    // stay unchanged; the placement's In selects the visible part of them.
    let in_ticks = to_ticks(start - i128::from(first.time.as_millis()))?;
    let out_ticks = to_ticks(end - i128::from(first.time.as_millis()))?;
    if in_ticks >= to_ticks(i128::from(video.source_intrinsic_duration.as_millis()))? {
        return None;
    }
    let curve = PrTimeRemap {
        keys: keys
            .iter()
            .map(|key| {
                Some(PrTimeRemapKeyframe {
                    timeline_ticks: to_ticks(i128::from(key.time.as_millis()) - start)?,
                    source_ticks: ticks_from_time(key.value, "ramp source key").ok()?,
                    easing: keyframes::native_easing(key.easing).ok()?,
                })
            })
            .collect::<Option<Vec<_>>>()?,
    };
    curve.ramp_modes()?;
    Some(NativeRamp {
        in_ticks,
        out_ticks,
        curve,
    })
}
