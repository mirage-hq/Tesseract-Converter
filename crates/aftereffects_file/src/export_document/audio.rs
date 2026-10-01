//! Editable gain keys, with an explicit bounded approximation in native dB space.

use super::{
    AnimationGraphEntry, KeyframeEasing, LayerId, NativeTrack, NumericKeyframe, NumericTrack,
    PropType, PropertyKeyframeEasing, constant_track, float_value, media, track,
};

pub(super) fn exactly_silent(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    base_gain: f64,
) -> Result<bool, &'static str> {
    match track(entries, id, PropType::AudioVolume)? {
        Some(NativeTrack::Constant(value)) => Ok(float_value(value)? == 0.0),
        Some(NativeTrack::Keyframes(keys)) => {
            let keys = keys.keyframes();
            if keys.is_empty() {
                return Ok(false);
            }
            for key in keys {
                if float_value(key.value())? != 0.0 {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        None => Ok(base_gain == 0.0),
    }
}

pub(super) fn levels_animation(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    allow_audio: bool,
) -> Result<Option<NumericTrack>, &'static str> {
    let Some(source) = track(entries, id, PropType::AudioVolume)? else {
        return Ok(None);
    };
    if !allow_audio {
        return Err("AudioVolume keys target media without native audio");
    }
    match source {
        NativeTrack::Constant(value) => {
            let db = media::gain_to_db(float_value(value)?)?;
            Ok(Some(constant_track(vec![db, db], 2, false)))
        }
        NativeTrack::Keyframes(track) => {
            let mut keys = Vec::with_capacity(track.keyframes().len());
            let mut previous_gain = None;
            for key in track.keyframes() {
                let gain = float_value(key.value())?;
                let db = media::gain_to_db(gain)?;
                let easing = match previous_gain {
                    Some(previous) => gain_easing(previous, gain, key.easing())?,
                    None => KeyframeEasing::Hold,
                };
                keys.push(NumericKeyframe {
                    time_millis: key.layer_time().as_millis(),
                    values: vec![db; 2],
                    easing: vec![easing; 2],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                });
                previous_gain = Some(gain);
            }
            Ok(Some(NumericTrack { keys }))
        }
    }
}

/// Map the two authored gain control values into dB, retaining temporal handles.
/// The logarithm of a cubic is not cubic: endpoints/Hold remain exact (apart
/// from the native silence floor), but continuous segments are approximations.
/// No sampled/baked keys are added. A negative control hull is rejected rather
/// than guessing the runtime's clamped gain between keys.
fn gain_easing(
    from: f64,
    to: f64,
    easing: PropertyKeyframeEasing,
) -> Result<KeyframeEasing, &'static str> {
    let (x1, y1, x2, y2) = match easing {
        PropertyKeyframeEasing::Hold => return Ok(KeyframeEasing::Hold),
        PropertyKeyframeEasing::Linear => (1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0),
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => (x1, y1, x2, y2),
    };
    let from_db = media::gain_to_db(from)?;
    let to_db = media::gain_to_db(to)?;
    let control = |y| media::gain_to_db(from + (to - from) * y);
    let control1 =
        control(y1).map_err(|_| "AudioVolume cubic gain control hull is negative or non-finite")?;
    let control2 =
        control(y2).map_err(|_| "AudioVolume cubic gain control hull is negative or non-finite")?;
    if from_db == to_db {
        if control1 != from_db || control2 != from_db {
            return Err(
                "AudioVolume equal floor endpoints have an unrepresentable above-floor excursion",
            );
        }
        return Ok(KeyframeEasing::Linear);
    }
    Ok(KeyframeEasing::CubicBezier {
        x1,
        y1: (control1 - from_db) / (to_db - from_db),
        x2,
        y2: (control2 - from_db) / (to_db - from_db),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_gain_floor_and_continuous_control_hulls_remain_finite_and_ordered() {
        let gains = [0.0, 1e-30, 0.001, 0.5, 1.0, 2.0, 100.0];
        assert_eq!(media::gain_to_db(0.0).unwrap(), -192.0);
        assert_eq!(media::gain_to_db(1e-30).unwrap(), -192.0);
        for from in gains {
            for to in gains {
                let easing = gain_easing(from, to, PropertyKeyframeEasing::Linear).unwrap();
                if let KeyframeEasing::CubicBezier { x1, y1, x2, y2 } = easing {
                    assert!([x1, y1, x2, y2].into_iter().all(f64::is_finite));
                    assert!((0.0..=1.0).contains(&y1));
                    assert!((y1..=1.0).contains(&y2));
                }
            }
        }
    }

    #[test]
    fn audio_gain_rejects_invalid_control_hulls_and_above_floor_excursions() {
        let invalid = PropertyKeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: -3.0,
            x2: 0.7,
            y2: 1.0,
        };
        assert!(gain_easing(0.5, 1.0, invalid).is_err());
        let excursion = PropertyKeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: 1e30,
            x2: 0.7,
            y2: 1.0,
        };
        assert!(gain_easing(0.0, 1e-30, excursion).is_err());
        assert!(media::gain_to_db(f64::INFINITY).is_err());
        assert!(media::gain_to_db(-0.1).is_err());
    }
}
