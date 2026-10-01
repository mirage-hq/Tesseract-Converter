//! Signed keyframe time quantization and easing at the FX/Premiere boundary.

use crate::{
    error::{unsupported, Result},
    schema::{PrKeyframeEasing, TICKS_PER_MILLISECOND},
};
use fx_schema::PropertyKeyframeEasing;

/// Round a source-relative native tick time to the nearest signed millisecond.
/// Ties round away from zero; distinct keys that collide are rejected by the track.
pub(super) fn layer_millis(source_ticks: i64, source_in: i64) -> Result<i64> {
    let offset = i128::from(source_ticks) - i128::from(source_in);
    let millisecond = i128::from(TICKS_PER_MILLISECOND);
    let half = millisecond / 2;
    let rounded = if offset < 0 {
        -((-offset + half) / millisecond)
    } else {
        (offset + half) / millisecond
    };
    i64::try_from(rounded)
        .map_err(|_| unsupported("Premiere key time exceeds the FX signed millisecond range"))
}

pub(super) fn source_ticks(source_in: i64, layer_millis: i64) -> Result<i64> {
    let ticks =
        i128::from(source_in) + i128::from(layer_millis) * i128::from(TICKS_PER_MILLISECOND);
    i64::try_from(ticks).map_err(|_| unsupported("Rotation key time exceeds Premiere's tick range"))
}

pub(super) fn fx_easing(easing: PrKeyframeEasing) -> PropertyKeyframeEasing {
    match easing {
        PrKeyframeEasing::Linear => PropertyKeyframeEasing::Linear,
        PrKeyframeEasing::Hold => PropertyKeyframeEasing::Hold,
        PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 }
        }
    }
}

pub(super) fn native_easing(easing: PropertyKeyframeEasing) -> Result<PrKeyframeEasing> {
    match easing {
        PropertyKeyframeEasing::Hold => Ok(PrKeyframeEasing::Hold),
        PropertyKeyframeEasing::Linear => Ok(PrKeyframeEasing::Linear),
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            ensure_representable_tangent(x1, y1, "outgoing")?;
            ensure_representable_tangent(1.0 - x2, 1.0 - y2, "incoming")?;
            Ok(PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 })
        }
    }
}

/// Whether Premiere draws `easing`, the segment into a scalar key that starts
/// a Hold, as FX does. Premiere 26.5.1 ignores the in-handle of such a key and
/// arrives with a zero-length one (Adobe readbacks of a graphic clip Opacity
/// and a clip Motion Scale), so only a curve that already arrives that way, or
/// a straight one, is drawn the same.
pub(super) fn premiere_keeps_the_arrival_into_a_hold(easing: PrKeyframeEasing) -> bool {
    match easing {
        PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            (x2 == 1.0 && y2 == 1.0) || (x1 == y1 && x2 == y2)
        }
        PrKeyframeEasing::Linear | PrKeyframeEasing::Hold => true,
    }
}

fn ensure_representable_tangent(influence: f64, rise: f64, name: &str) -> Result<()> {
    if influence == 0.0 && rise != 0.0 {
        return Err(unsupported(format!(
            "vertical {name} cubic timing handle cannot be represented by Premiere velocity/influence"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{layer_millis, source_ticks};
    use crate::schema::TICKS_PER_MILLISECOND;

    #[test]
    fn signed_key_times_round_symmetrically_at_half_millisecond() {
        let half_millis = TICKS_PER_MILLISECOND / 2;
        let one_second = 1000 * TICKS_PER_MILLISECOND;
        assert_eq!(layer_millis(half_millis, 0).unwrap(), 1);
        assert_eq!(layer_millis(-half_millis, 0).unwrap(), -1);
        assert_eq!(layer_millis(half_millis - 1, 0).unwrap(), 0);
        assert_eq!(layer_millis(1 - half_millis, 0).unwrap(), 0);
        assert_eq!(
            layer_millis(one_second - half_millis, one_second).unwrap(),
            -1
        );
        assert_eq!(source_ticks(one_second, -1000).unwrap(), 0);
    }

    #[test]
    fn out_of_range_source_tick_conversion_rejects() {
        assert!(source_ticks(i64::MAX, 1).is_err());
        assert!(source_ticks(i64::MIN, -1).is_err());
        assert!(layer_millis(i64::MAX, i64::MIN).is_ok());
    }
}
