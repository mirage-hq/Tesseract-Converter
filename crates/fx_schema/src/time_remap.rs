//! Persisted time-remap data and structural constraints only.
use crate::animator::{KeyframeId, PropertyKeyframeEasing, MAX_KEYFRAME_ID_BYTES};
use crate::time::Time;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;
const MAX_EXACT_KEYFRAME_MILLIS: u64 = (1_u64 << 53) - 1;

/// Behavior before or after a TimeRemap property's authored input interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum TimeRemapExtrapolation {
    /// The layer has no content outside the authored interval.
    Inactive,
    /// Hold the nearest endpoint value.
    Hold,
    /// Continue with the nearest endpoint tangent.
    Continue,
    /// Repeat the authored input interval.
    Loop,
    /// Repeat the authored input interval, alternating forward and reverse passes.
    PingPong,
}

crate::define_time_remap_keyframe_schema!();

macro_rules! stored_time_remap_property {
    ($(#[$docs:meta])* pub struct $name:ident { $($fields:tt)* }) => {
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[serde(rename_all = "camelCase")]
        struct TimeRemapData {
            $($fields)*
        }
    };
}
crate::define_time_remap_property_schema!(stored_time_remap_property);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct TimeRemapProperty(crate::stored::Stored<TimeRemapData>);

impl<'de> Deserialize<'de> for TimeRemapData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::define_time_remap_wire_schema!();
        let wire = Wire::deserialize(deserializer)?;
        validate_keyframes(&wire.keyframes).map_err(de::Error::custom)?;
        Ok(Self {
            keyframes: wire.keyframes,
            before: wire.before,
            after: wire.after,
        })
    }
}

impl TimeRemapProperty {
    pub fn new(
        keyframes: Vec<TimeRemapKeyframe>,
        before: TimeRemapExtrapolation,
        after: TimeRemapExtrapolation,
    ) -> Result<Self, TimeRemapError> {
        validate_keyframes(&keyframes)?;
        let data = TimeRemapData {
            keyframes,
            before,
            after,
        };
        crate::stored::Stored::from_data(&data)
            .map(Self)
            .map_err(|error| TimeRemapError::Serialization(error.to_string()))
    }
    pub fn keyframes(&self) -> &[TimeRemapKeyframe] {
        &self.0.data().keyframes
    }
    pub fn before(&self) -> TimeRemapExtrapolation {
        self.0.data().before
    }
    pub fn after(&self) -> TimeRemapExtrapolation {
        self.0.data().after
    }

    /// Structural restriction required by the persisted PAG playback field.
    pub(crate) fn is_unit_rate_trim(&self) -> bool {
        self.before() == TimeRemapExtrapolation::Inactive
            && self.after() == TimeRemapExtrapolation::Inactive
            && self.keyframes().windows(2).all(|pair| {
                pair[1].easing == PropertyKeyframeEasing::Linear
                    && pair[1].value.checked_sub(pair[0].value)
                        == pair[1].time.checked_sub(pair[0].time)
            })
    }
}

fn has_finite_endpoint_derivative(
    first_x: f64,
    first_y: f64,
    second_x: f64,
    second_y: f64,
) -> bool {
    const EPSILON: f64 = 1e-12;
    first_x.abs() >= EPSILON
        || (first_y.abs() < EPSILON && (second_x.abs() >= EPSILON || second_y.abs() < EPSILON))
}

/// Validation failure for a complete TimeRemap property.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimeRemapError {
    #[error("TimeRemap requires at least two keyframes")]
    TooFewKeyframes,
    #[error("TimeRemap keyframe id must be non-empty and at most {MAX_KEYFRAME_ID_BYTES} bytes")]
    InvalidKeyframeId,
    #[error("TimeRemap keyframe ids must be unique: {0:?}")]
    DuplicateKeyframeId(String),
    #[error("TimeRemap keyframe times must be strictly increasing")]
    UnorderedKeyframes,
    #[error("TimeRemap keyframe time and value must be exact non-sentinel milliseconds")]
    InexactCoordinate,
    #[error("TimeRemap cubic easing must be bounded and have a finite derivative")]
    InvalidEasing,
    #[error("TimeRemap coordinate transform is outside the exact non-negative millisecond domain")]
    CoordinateTransformOutOfRange,
    #[error("TimeRemap could not be serialized for validation: {0}")]
    Serialization(String),
}

fn validate_keyframes(keyframes: &[TimeRemapKeyframe]) -> Result<(), TimeRemapError> {
    if keyframes.len() < 2 {
        return Err(TimeRemapError::TooFewKeyframes);
    }
    let mut ids = BTreeSet::new();
    let mut previous_time = None;
    for keyframe in keyframes {
        let id = keyframe.id.as_str();
        if id.is_empty() || id.len() > MAX_KEYFRAME_ID_BYTES {
            return Err(TimeRemapError::InvalidKeyframeId);
        }
        if !ids.insert(id) {
            return Err(TimeRemapError::DuplicateKeyframeId(id.to_owned()));
        }
        let time = keyframe.time.as_millis();
        let value = keyframe.value.as_millis();
        if time > MAX_EXACT_KEYFRAME_MILLIS || value > MAX_EXACT_KEYFRAME_MILLIS {
            return Err(TimeRemapError::InexactCoordinate);
        }
        if previous_time.is_some_and(|previous| time <= previous) {
            return Err(TimeRemapError::UnorderedKeyframes);
        }
        previous_time = Some(time);
        validate_easing(keyframe.easing)?;
    }
    Ok(())
}

fn validate_easing(easing: PropertyKeyframeEasing) -> Result<(), TimeRemapError> {
    let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = easing else {
        return Ok(());
    };
    if ![x1, y1, x2, y2].into_iter().all(f64::is_finite)
        || !(0.0..=1.0).contains(&x1)
        || !(0.0..=1.0).contains(&x2)
        || !(0.0..=1.0).contains(&y1)
        || !(0.0..=1.0).contains(&y2)
        || x1 > x2
        || !has_finite_endpoint_derivative(x1, y1, x2, y2)
        || !has_finite_endpoint_derivative(1.0 - x2, 1.0 - y2, 1.0 - x1, 1.0 - y1)
    {
        return Err(TimeRemapError::InvalidEasing);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_remap_larger_than_the_former_one_mib_limit_round_trips() {
        let keyframes = (0..8_192)
            .map(|index| TimeRemapKeyframe {
                id: KeyframeId::new(format!("{index:04}-{}", "x".repeat(120))),
                time: Time::from_millis(index),
                value: Time::from_millis(index),
                easing: PropertyKeyframeEasing::Linear,
            })
            .collect();
        let property = TimeRemapProperty::new(
            keyframes,
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap();

        let wire = serde_json::to_vec(&property).unwrap();
        assert!(wire.len() > 1024 * 1024);
        let restored: TimeRemapProperty = serde_json::from_slice(&wire).unwrap();
        assert_eq!(restored, property);
    }
}
