//! Native Speed curves use plateau slopes rather than independent handles.

use super::graph::TimeRemapIds;
use crate::{
    format::{invalid, Result},
    schema::{native::*, records, PrMedia, PrVideoOccurrence, TICKS},
};
use std::fmt::Write;

pub(super) fn records(
    clip: &PrVideoOccurrence,
    media: &PrMedia,
    ids: &TimeRemapIds,
) -> Result<[Record; 2]> {
    let curve = clip
        .time_remap
        .as_ref()
        .ok_or_else(|| invalid("missing native ramp"))?;
    let modes = curve.ramp_modes().ok_or_else(|| {
        invalid("TimeRemapping requires forward ramps with plateau-constrained handles")
    })?;
    let intrinsic = media
        .video
        .as_ref()
        .ok_or_else(|| invalid("ramp has no video stream"))?
        .intrinsic_ticks;
    let mut keys = String::new();
    for (key, mode) in curve.keys.iter().zip(modes) {
        let time = key
            .timeline_ticks
            .checked_add(clip.in_ticks)
            .filter(|time| *time >= 0)
            .ok_or_else(|| invalid("TimeRemapping input key exceeds the native clock"))?;
        // Native Speed values are seconds, whereas key positions are ticks.
        // Use the saved 24-decimal form, not the unrelated Motion key units.
        write!(
            keys,
            "{time},{:.24},{mode},0,0,0,0,0;",
            key.source_ticks as f64 / TICKS as f64
        )
        .expect("writing a String cannot fail");
    }
    Ok([
        Record::TimeRemapping(TimeRemapping {
            _object_id: ids.mapping,
            class_id: records::TIME_REMAPPING.class_id.into(),
            version: records::TIME_REMAPPING.version.into(),
            keyframes: Reference::object(ids.parameter),
        }),
        Record::TimeComponentParam(TimeComponentParam {
            _object_id: ids.parameter,
            class_id: records::TIME_COMPONENT_PARAM.class_id.into(),
            // The pinned Premiere 26.5.1 save omits these five flags in v9.
            version: "9".into(),
            name: "Speed".into(),
            is_time_varying: None,
            is_locked: None,
            discontinuous_interpolate: None,
            parameter_control_type: None,
            range_locked: None,
            start_keyframe: super::tracks::scalar_start_keyframe(0),
            keyframes: keys,
            _current_value: "0".into(),
            parameter_id: "-1".into(),
            lower_bound: "0".into(),
            _upper_bound: (intrinsic as f64 / TICKS as f64).to_string(),
        }),
    ])
}
