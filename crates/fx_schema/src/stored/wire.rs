//! Immutable JSON storage without the spare capacity of a map per tiny object.
//!
//! Keep `Number` directly: a JSON text round trip can change floating-point
//! values, and a raw JSON serializer would not support every Serde serializer.
use std::{collections::BTreeMap, sync::OnceLock};

use serde::{
    de::DeserializeOwned, ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer,
};
use serde_json::{Number, Value};

mod capture;
mod de;
mod presence;

#[derive(Debug)]
pub(super) struct Wire {
    compact: CompactValue,
    value: OnceLock<Value>,
}

impl Wire {
    pub(super) fn new(value: Value) -> Self {
        Self {
            compact: value.into(),
            value: OnceLock::new(),
        }
    }

    pub(super) fn decode<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        T::deserialize(&self.compact)
    }

    pub(super) fn has_unknown_fields<T: Serialize>(
        &self,
        data: &T,
        skip: &[(usize, &'static str)],
    ) -> bool {
        data.serialize(presence::Presence {
            raw: Some(&self.compact),
            skip,
            depth: 0,
        })
        .expect("checked stored data serializes")
    }

    pub(super) fn value(&self) -> &Value {
        self.value.get_or_init(|| self.compact.to_value())
    }

    pub(super) fn contains_key(&self, key: &str) -> bool {
        matches!(&self.compact, CompactValue::Object(fields)
            if fields.iter().any(|(name, _)| name == key))
    }

    #[cfg(test)]
    pub(super) fn is_materialized(&self) -> bool {
        self.value.get().is_some()
    }
}

impl<'de> Deserialize<'de> for Wire {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            compact: CompactValue::deserialize(deserializer)?,
            value: OnceLock::new(),
        })
    }
}

impl PartialEq for Wire {
    fn eq(&self, other: &Self) -> bool {
        // The optional cache is not part of the stored document's identity.
        self.compact == other.compact
    }
}

impl Serialize for Wire {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.compact.serialize(serializer)
    }
}

#[derive(Debug)]
enum CompactValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Box<[Self]>),
    Object(Box<[(String, Self)]>),
}

impl From<Value> for CompactValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(value),
            Value::Number(value) => Self::Number(value),
            Value::String(value) => Self::String(value),
            Value::Array(values) => Self::Array(values.into_iter().map(Self::from).collect()),
            Value::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from(value)))
                    .collect(),
            ),
        }
    }
}

impl CompactValue {
    fn to_value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(*value),
            Self::Number(value) => Value::Number(value.clone()),
            Self::String(value) => Value::String(value.clone()),
            Self::Array(values) => Value::Array(values.iter().map(Self::to_value).collect()),
            Self::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_value()))
                    .collect(),
            ),
        }
    }
}

impl PartialEq for CompactValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (Self::Number(left), Self::Number(right)) => left == right,
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Array(left), Self::Array(right)) => left == right,
            (Self::Object(left), Self::Object(right)) => {
                if left.len() != right.len() {
                    return false;
                }
                if left
                    .iter()
                    .map(|(key, _)| key)
                    .eq(right.iter().map(|(key, _)| key))
                {
                    return left
                        .iter()
                        .zip(right.iter())
                        .all(|((_, left), (_, right))| left == right);
                }
                // Value's object equality ignores order, even when a downstream
                // crate enables serde_json/preserve_order. Keep serialization in
                // its original order, without building full Value trees here.
                let left: BTreeMap<_, _> = left.iter().map(|(key, value)| (key, value)).collect();
                let right: BTreeMap<_, _> = right.iter().map(|(key, value)| (key, value)).collect();
                left == right
            }
            _ => false,
        }
    }
}

impl Serialize for CompactValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(value) => value.serialize(serializer),
            Self::String(value) => serializer.serialize_str(value),
            Self::Array(values) => values.serialize(serializer),
            Self::Object(values) => {
                let mut map = serializer.serialize_map(Some(values.len()))?;
                for (key, value) in values {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_all_value_kinds_without_reparsing_numbers() {
        let original = json!({
            "null": null,
            "bool": true,
            "string": "escaped\n\"text\" 日本語",
            "array": [3, 1, 3, {}, []],
            "numbers": [i64::MIN, u64::MAX, -0.0, 0.0, 1.0, f64::MIN_POSITIVE, f64::MAX,
                f64::from_bits(1), f64::from_bits(0x3ff0000000000001)],
        });
        let wire = Wire::new(original.clone());
        assert_eq!(serde_json::to_value(&wire).unwrap(), original);
        assert_eq!(
            serde_json::to_vec(&wire).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );
        assert!(!wire.is_materialized());
        assert_eq!(wire.value(), &original);
        // Value equality alone cannot distinguish the sign of zero.
        assert_eq!(
            wire.value()["numbers"][2].as_f64().unwrap().to_bits(),
            (-0.0_f64).to_bits()
        );
    }

    #[test]
    fn equality_matches_value_including_numeric_representation() {
        let values = [
            json!(null),
            json!(false),
            json!("1"),
            json!(0),
            json!(0.0),
            json!(-0.0),
            json!(1),
            json!(1.0),
            json!(u64::MAX),
            json!([1, 2]),
            json!([2, 1]),
            json!({"x": 1}),
            json!({"x": 1.0}),
        ];
        for left in &values {
            for right in &values {
                let left_wire = Wire::new(left.clone());
                let right_wire = Wire::new(right.clone());
                assert_eq!(
                    left_wire == right_wire,
                    left == right,
                    "{left:?} vs {right:?}"
                );
                assert!(!left_wire.is_materialized());
                assert!(!right_wire.is_materialized());
            }
        }
    }

    #[test]
    fn object_equality_ignores_order_but_serialization_preserves_it() {
        let left = CompactValue::Object(
            vec![("b".into(), json!(1).into()), ("a".into(), json!(2).into())].into(),
        );
        let right = CompactValue::Object(
            vec![("a".into(), json!(2).into()), ("b".into(), json!(1).into())].into(),
        );
        assert_eq!(left, right);
        assert_eq!(serde_json::to_string(&left).unwrap(), r#"{"b":1,"a":2}"#);
        assert_eq!(serde_json::to_string(&right).unwrap(), r#"{"a":2,"b":1}"#);
    }
}
