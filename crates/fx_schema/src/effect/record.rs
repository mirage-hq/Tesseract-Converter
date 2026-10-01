//! Stored effect records without identifier allocation or legacy conversion.

use crate::{stored::Stored, EffectId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use strum::VariantNames;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct EffectRecord(Stored<EffectData>);

// Bind the canonical instance's payload slot to the stored reader.
use super::LayerEffect as KnownLayerEffect;
type LayerEffect = EffectPayload;

macro_rules! stored_effect_instance {
    ($(#[$docs:meta])* pub struct $name:ident {
        $($(#[$field_attrs:meta])* $visibility:vis $field:ident: $field_type:ty,)*
    } compatibility { $($compatibility:tt)* }) => {
        #[derive(Debug, Clone, PartialEq, Serialize)]
        #[serde(untagged)]
        pub enum EffectData {
            Identified {
                $($(#[$field_attrs])* $field: $field_type,)*
            },
            Legacy(EffectPayload),
        }

        #[derive(Deserialize)]
        struct Instance {
            $($(#[$field_attrs])* $field: $field_type,)*
        }
    };
}
crate::define_effect_instance_schema!(stored_effect_instance);

fn deserialize_persisted_effect<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<EffectPayload, D::Error> {
    EffectPayload::deserialize(deserializer)
}

fn serialize_persisted_effect<S: Serializer>(
    effect: &EffectPayload,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    effect.serialize(serializer)
}

/// Unknown effect kinds remain opaque; malformed known kinds are errors.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectPayload {
    Known(KnownLayerEffect),
    Unknown(Value),
}

impl EffectRecord {
    pub fn from_data(data: &EffectData) -> Result<Self, serde_json::Error> {
        Stored::from_data(data).map(Self)
    }

    pub fn known_value(&self) -> Value {
        serde_json::to_value(self.data()).expect("checked effect data serializes")
    }

    pub fn data(&self) -> &EffectData {
        self.0.data()
    }

    pub fn wire_value(&self) -> &Value {
        self.0.wire_value()
    }
}

impl<'de> Deserialize<'de> for EffectData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = Value::deserialize(deserializer)?;
        let object = raw
            .as_object()
            .ok_or_else(|| D::Error::custom("effect record must be an object"))?;
        // Presence, not successful decoding, selects the instance format.
        if object.contains_key("id") || object.contains_key("effect") {
            let instance: Instance = serde_json::from_value(raw).map_err(D::Error::custom)?;
            Ok(Self::Identified {
                id: instance.id,
                enabled: instance.enabled,
                effect: instance.effect,
            })
        } else {
            serde_json::from_value(raw)
                .map(Self::Legacy)
                .map_err(D::Error::custom)
        }
    }
}

fn default_true() -> bool {
    true
}

impl<'de> Deserialize<'de> for EffectPayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = Value::deserialize(deserializer)?;
        let kind = raw
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::custom("effect requires a string type"))?;
        let future_contract = match kind {
            "lookTransform" => crate::color::has_unsupported_color_semantics(&raw),
            "primaryGrade" => crate::color::has_unsupported_primary_grade_semantics(&raw),
            "colorCurves" => crate::curves::has_unsupported_curves_semantics(&raw),
            _ => false,
        };
        if KnownLayerEffect::VARIANTS.contains(&kind) && kind != "unsupported" && !future_contract {
            serde_json::from_value(raw)
                .map(Self::Known)
                .map_err(D::Error::custom)
        } else {
            Ok(Self::Unknown(raw))
        }
    }
}

impl Serialize for EffectPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Known(effect) => effect.serialize(serializer),
            Self::Unknown(raw) => raw.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_effect_is_preserved_without_allocating_an_identifier() {
        let wire = json!({"type": "gaussianBlur", "blurriness": 3, "future": [null, 2]});
        let record: EffectRecord = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            record.data(),
            EffectData::Legacy(EffectPayload::Known(_))
        ));
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn legacy_shader_preset_is_not_resolved_or_rewritten() {
        let wire =
            json!({"type": "shaderPreset", "presetId": "historical", "params": {"amount": 3}});
        let record: EffectRecord = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            record.data(),
            EffectData::Legacy(EffectPayload::Unknown(_))
        ));
        assert_eq!(record.wire_value(), &wire);
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn identified_record_preserves_absent_defaults_and_nested_extensions() {
        let wire = json!({"id": 7, "effect": {"type": "gaussianBlur", "blurriness": 3, "future": true}, "futureInstance": null});
        let record: EffectRecord = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            record.data(),
            EffectData::Identified { enabled: true, .. }
        ));
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn malformed_identified_records_never_fall_back_to_legacy() {
        for wire in [
            json!({"id": null, "type": "futureEffect"}),
            json!({"effect": {"type": "futureEffect"}}),
            json!({"id": 7, "effect": {"type": "futureEffect"}, "enabled": null}),
            json!({"id": 7, "effect": {"type": "gaussianBlur", "blurriness": "invalid"}}),
            json!({"type": "gaussianBlur", "blurriness": "invalid"}),
            json!({"type": 7}),
        ] {
            assert!(serde_json::from_value::<EffectRecord>(wire).is_err());
        }
    }

    #[test]
    fn unknown_identified_effect_kinds_remain_data() {
        let wire = json!({"id": 7, "enabled": false, "effect": {"type": "futureEffect", "data": [2, 1, 2]}});
        let record: EffectRecord = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            record.data(),
            EffectData::Identified {
                effect: EffectPayload::Unknown(_),
                ..
            }
        ));
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn authored_unknown_payload_cannot_bypass_known_field_checks() {
        let data = EffectData::Legacy(EffectPayload::Unknown(
            json!({"type": "gaussianBlur", "blurriness": "invalid"}),
        ));
        assert!(EffectRecord::from_data(&data).is_err());
    }
}
