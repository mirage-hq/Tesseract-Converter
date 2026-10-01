//! Numeric keys belonging to the single native stroke, not the layer Transform.

use super::{AepWriteError, KeyframeEasing, NumericTrack};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StrokeAnimations {
    pub width: Option<NumericTrack>,
    pub miter_limit: Option<NumericTrack>,
    pub join: Option<NumericTrack>,
}

impl StrokeAnimations {
    pub(super) fn validate(&self, has_stroke: bool) -> Result<(), AepWriteError> {
        if !has_stroke
            && (self.width.is_some() || self.miter_limit.is_some() || self.join.is_some())
        {
            return Err(AepWriteError::Invalid(
                "stroke animation requires an exported owned Stroke",
            ));
        }
        if let Some(track) = &self.join {
            for key in &track.keys {
                if !matches!(key.values.as_slice(), [1.0 | 2.0 | 3.0])
                    || key.easing.as_slice() != [KeyframeEasing::Hold]
                    || !key.spatial_in.is_empty()
                    || !key.spatial_out.is_empty()
                {
                    return Err(AepWriteError::Invalid(
                        "stroke join keys require scalar enums 1/2/3 with Hold easing and no spatial tangents",
                    ));
                }
            }
        }
        for (track, maximum) in [(&self.width, 100_000.0), (&self.miter_limit, f64::MAX)] {
            let Some(track) = track else {
                continue;
            };
            for key in &track.keys {
                let [value] = key.values.as_slice() else {
                    return Err(AepWriteError::Invalid("stroke key must be scalar"));
                };
                if !value.is_finite() || !(0.0..=maximum).contains(value) {
                    return Err(AepWriteError::Invalid("stroke key exceeds native bounds"));
                }
                // FX rejects evaluated negative widths; native clamping of
                // overshooting curves is not established. Retain bounded easing
                // only instead of silently changing behavior between keys.
                if key.easing.iter().any(|easing| {
                    matches!(easing, KeyframeEasing::CubicBezier { y1, y2, .. }
                        if !(0.0..=1.0).contains(y1) || !(0.0..=1.0).contains(y2))
                }) {
                    return Err(AepWriteError::Invalid(
                        "overshooting stroke easing is not native-exportable",
                    ));
                }
            }
            // The shared encoder derives temporal speeds from endpoint delta.
            // Finite endpoints alone do not guarantee finite native speeds.
            for pair in track.keys.windows(2) {
                // Native time bounds (checked by the shared encoder) are below
                // f64's exact integer range; convert first to avoid i64 overflow.
                let duration = (pair[1].time_millis as f64 - pair[0].time_millis as f64) / 1000.0;
                if duration <= 0.0 {
                    continue; // The shared encoder rejects unordered times.
                }
                let delta = pair[1].values[0] - pair[0].values[0];
                for easing in &pair[1].easing {
                    if let KeyframeEasing::CubicBezier { x1, y1, x2, y2 } = easing {
                        let outgoing = if *x1 == 0.0 {
                            if *y1 != 0.0 {
                                return Err(AepWriteError::Invalid(
                                    "stroke temporal speed is nonfinite",
                                ));
                            }
                            0.0
                        } else {
                            y1 / x1 * delta / duration
                        };
                        let incoming = if *x2 == 1.0 {
                            if *y2 != 1.0 {
                                return Err(AepWriteError::Invalid(
                                    "stroke temporal speed is nonfinite",
                                ));
                            }
                            0.0
                        } else {
                            (1.0 - y2) / (1.0 - x2) * delta / duration
                        };
                        if !outgoing.is_finite() || !incoming.is_finite() {
                            return Err(AepWriteError::Invalid(
                                "stroke temporal speed is nonfinite",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::NumericKeyframe;

    fn keys(value: f64) -> StrokeAnimations {
        StrokeAnimations {
            width: Some(NumericTrack {
                keys: vec![NumericKeyframe {
                    time_millis: 0,
                    values: vec![value],
                    easing: vec![KeyframeEasing::Linear],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                }],
            }),
            miter_limit: None,
            join: None,
        }
    }

    #[test]
    fn stroke_keys_require_a_paint_and_bounded_values() {
        assert!(StrokeAnimations::default().validate(false).is_ok());
        for value in [0.0, 6.0, 100_000.0] {
            assert!(keys(value).validate(true).is_ok());
            assert!(keys(value).validate(false).is_err());
        }
        for value in [-1.0, 100_001.0, f64::NAN, f64::INFINITY] {
            assert!(keys(value).validate(true).is_err());
        }
        let mut miter = keys(-1.0);
        miter.miter_limit = miter.width.take();
        assert!(miter.validate(true).is_err());
    }

    #[test]
    fn join_tracks_require_owned_stroke_and_discrete_native_values() {
        for value in [1.0, 2.0, 3.0, 0.0, 1.5, 4.0, f64::NAN, f64::INFINITY] {
            let mut animations = keys(value);
            animations.join = animations.width.take();
            let track = animations.join.as_mut().unwrap();
            track.keys[0].easing = vec![KeyframeEasing::Hold];
            let valid = matches!(value, 1.0 | 2.0 | 3.0);
            assert_eq!(animations.validate(true).is_ok(), valid);
            assert!(animations.validate(false).is_err());
            animations.join.as_mut().unwrap().keys[0].easing = vec![KeyframeEasing::Linear];
            assert!(animations.validate(true).is_err());
        }
        let mut animations = keys(1.0);
        animations.join = animations.width.take();
        let key = &mut animations.join.as_mut().unwrap().keys[0];
        key.easing = vec![KeyframeEasing::Hold];
        key.spatial_out = vec![0.0];
        assert!(animations.validate(true).is_err());
    }

    #[test]
    fn finite_endpoints_cannot_emit_infinite_temporal_speed() {
        let mut animations = keys(0.0);
        let track = animations.width.as_mut().unwrap();
        let mut next = track.keys[0].clone();
        next.time_millis = 1;
        next.values = vec![100_000.0];
        next.easing = vec![KeyframeEasing::CubicBezier {
            x1: f64::MIN_POSITIVE,
            y1: 0.5,
            x2: 0.75,
            y2: 0.8,
        }];
        track.keys.push(next);
        assert!(animations.validate(true).is_err());
    }

    #[test]
    fn vertical_endpoint_tangents_are_rejected() {
        let mut animations = keys(0.0);
        let mut next = animations.width.as_ref().unwrap().keys[0].clone();
        next.time_millis = 1000;
        next.values = vec![10.0];
        for (x1, y1, x2, y2, valid) in [
            (0.0, 0.0, 1.0, 1.0, true),
            (0.0, 0.5, 1.0, 1.0, false),
            (0.0, 0.0, 1.0, 0.5, false),
        ] {
            next.easing = vec![KeyframeEasing::CubicBezier { x1, y1, x2, y2 }];
            animations.width.as_mut().unwrap().keys.push(next.clone());
            assert_eq!(animations.validate(true).is_ok(), valid);
            animations.width.as_mut().unwrap().keys.pop();
        }
    }

    #[test]
    fn overshooting_stroke_ease_is_not_silently_clamped() {
        let mut animations = keys(6.0);
        animations.width.as_mut().unwrap().keys[0].easing = vec![KeyframeEasing::CubicBezier {
            x1: 0.25,
            y1: -0.5,
            x2: 0.75,
            y2: 1.0,
        }];
        assert!(animations.validate(true).is_err());
    }
}
