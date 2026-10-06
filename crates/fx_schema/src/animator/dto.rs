//! Stored animator data. Script text is never inspected or rewritten here.

use super::{wire::WirePropertyAnimator, PropertyKeyframeTrack};
use crate::{stored::Stored, PropertyValue};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A structurally checked animator and its unmodified JSON representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct PropertyAnimator(Stored<AnimatorData>);

crate::define_stored_animator_data_schema!();

impl PropertyAnimator {
    pub fn constant(value: PropertyValue) -> Result<Self, serde_json::Error> {
        Self::from_data(&AnimatorData::Constant { value })
    }

    pub fn keyframes(track: PropertyKeyframeTrack) -> Self {
        Self::from_data(&AnimatorData::Keyframes {
            track,
            enabled: true,
            disabled_value: None,
        })
        .expect("checked track is valid stored data")
    }

    pub fn keyframe_track(&self) -> Option<&PropertyKeyframeTrack> {
        match self.data() {
            AnimatorData::Keyframes { track, .. } => Some(track),
            _ => None,
        }
    }

    pub(crate) fn has_unknown_fields(&self) -> bool {
        self.0.has_unknown_fields(&[])
    }

    pub fn known_value(&self) -> Value {
        serde_json::to_value(self.data()).expect("checked animator data serializes")
    }

    pub fn from_data(data: &AnimatorData) -> Result<Self, serde_json::Error> {
        Stored::from_data(data).map(Self)
    }

    pub fn data(&self) -> &AnimatorData {
        self.0.data()
    }

    pub fn wire_value(&self) -> &Value {
        self.0.wire_value()
    }

    pub fn finite_value_range(&self) -> Option<Vec<&PropertyValue>> {
        match self.data() {
            AnimatorData::Constant { value } => Some(vec![value]),
            AnimatorData::Keyframes {
                track,
                disabled_value,
                ..
            } => Some(
                track
                    .keyframes()
                    .iter()
                    .map(super::PropertyKeyframe::value)
                    .chain(disabled_value.iter())
                    .collect(),
            ),
            AnimatorData::JsScript { .. } => None,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self.data() {
            AnimatorData::Constant { .. } => "constant",
            AnimatorData::JsScript { .. } => "jsScript",
            AnimatorData::Keyframes { .. } => "keyframes",
        }
    }

    pub fn is_js_script(&self) -> bool {
        matches!(self.data(), AnimatorData::JsScript { .. })
    }
}

impl<'de> Deserialize<'de> for AnimatorData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        match WirePropertyAnimator::deserialize(deserializer)? {
            WirePropertyAnimator::Constant { value, .. } => Ok(Self::Constant { value }),
            WirePropertyAnimator::JsScript {
                code,
                layer_time_js_code,
                ..
            } => {
                if code.is_none() && layer_time_js_code.is_none() {
                    return Err(D::Error::custom(
                        "jsScript requires code or layerTimeJsCode",
                    ));
                }
                Ok(Self::JsScript {
                    code,
                    layer_time_js_code,
                })
            }
            WirePropertyAnimator::Keyframes {
                enabled,
                keyframes,
                disabled_value,
                ..
            } => {
                let track =
                    PropertyKeyframeTrack::from_wire(keyframes).map_err(D::Error::custom)?;
                if enabled && disabled_value.is_some() {
                    return Err(D::Error::custom(
                        "enabled keyframes cannot have disabledValue",
                    ));
                }
                if !enabled && disabled_value.is_none() {
                    return Err(D::Error::custom("disabled keyframes require disabledValue"));
                }
                Ok(Self::Keyframes {
                    track,
                    enabled,
                    disabled_value,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::PropertyKeyframe;
    use super::*;
    use crate::{KeyframeId, PropertyKeyframeEasing, TimeOffset};
    use serde_json::json;

    #[test]
    fn legacy_script_is_preserved_without_rewriting_or_adding_fields() {
        let wire = json!({"type": "jsScript", "code": "return input.time;"});
        let animator: PropertyAnimator = serde_json::from_value(wire.clone()).unwrap();
        let AnimatorData::JsScript {
            code,
            layer_time_js_code,
        } = animator.data()
        else {
            panic!("expected stored script");
        };
        assert_eq!(code.as_deref(), Some("return input.time;"));
        assert_eq!(layer_time_js_code, &None);
        assert_eq!(serde_json::to_value(animator).unwrap(), wire);
    }

    #[test]
    fn both_script_fields_are_data_not_a_precedence_decision() {
        let wire = json!({
            "type": "jsScript", "code": "legacy text", "layerTimeJsCode": "different text",
            "future": {"nested": [null, 7]}
        });
        let animator: PropertyAnimator = serde_json::from_value(wire.clone()).unwrap();
        let AnimatorData::JsScript {
            code,
            layer_time_js_code,
        } = animator.data()
        else {
            panic!("expected stored script");
        };
        assert_eq!(code.as_deref(), Some("legacy text"));
        assert_eq!(layer_time_js_code.as_deref(), Some("different text"));
        assert_eq!(animator.wire_value(), &wire);
        assert_eq!(serde_json::to_value(animator).unwrap(), wire);
    }

    #[test]
    fn canonical_script_preserves_explicit_null_and_unknown_fields() {
        let wire = json!({
            "type": "jsScript", "code": null, "layerTimeJsCode": "return 1;", "future": true
        });
        let animator: PropertyAnimator = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(animator).unwrap(), wire);
    }

    #[test]
    fn malformed_supported_fields_do_not_become_opaque_records() {
        for wire in [
            json!({"type": "jsScript"}),
            json!({"type": "jsScript", "code": 1}),
            json!({"type": "jsScript", "code": "x", "enabled": null}),
            json!({"type": "constant", "value": {"type": "float", "value": 1}, "code": null}),
        ] {
            assert!(serde_json::from_value::<PropertyAnimator>(wire).is_err());
        }
    }

    fn key(id: &str, milliseconds: i64) -> Value {
        serde_json::to_value(PropertyKeyframe::new(
            KeyframeId::new(id),
            TimeOffset::from_millis(milliseconds),
            PropertyValue::Float(1.0),
            PropertyKeyframeEasing::Linear,
        ))
        .unwrap()
    }

    #[test]
    fn keyframe_unknown_fields_and_order_survive() {
        let mut first = key("a", 0);
        first["futureKey"] = json!({"value": 3});
        first["easing"]["futureEasing"] = json!(true);
        let wire = json!({
            "type": "keyframes", "enabled": true,
            "keyframes": [first, key("b", 1000)], "futureTrack": null
        });
        let animator: PropertyAnimator = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(animator).unwrap(), wire);
    }

    #[test]
    fn unordered_or_duplicate_keys_are_rejected_not_normalized() {
        let mut malformed = key("a", 0);
        malformed["layerTime"] = json!("invalid");
        malformed["futureKey"] = json!(true);
        for keys in [
            vec![key("a", 1000), key("b", 0)],
            vec![key("a", 0), key("a", 1000)],
            vec![malformed, key("b", 1000)],
        ] {
            let wire = json!({"type": "keyframes", "enabled": true, "keyframes": keys});
            assert!(serde_json::from_value::<PropertyAnimator>(wire).is_err());
        }
    }

    #[test]
    fn authored_script_reenters_structural_checks() {
        assert!(PropertyAnimator::from_data(&AnimatorData::JsScript {
            code: None,
            layer_time_js_code: None,
        })
        .is_err());
        let animator = PropertyAnimator::from_data(&AnimatorData::JsScript {
            code: None,
            layer_time_js_code: Some("return 1;".to_owned()),
        })
        .unwrap();
        assert_eq!(
            animator.wire_value(),
            &json!({"type": "jsScript", "layerTimeJsCode": "return 1;"})
        );
    }
}
