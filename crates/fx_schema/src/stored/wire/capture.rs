//! Capture JSON-shaped Serde input without an intermediate recursive `Value`.
//!
//! The small, temporary object index uses serde_json's own map so dependency
//! feature unification (notably `preserve_order`) cannot change our key policy.
use std::{fmt, mem};

use serde::{
    de::{self, value, DeserializeSeed, IntoDeserializer, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::{Map, Number, Value};

use super::CompactValue;

impl<'de> Deserialize<'de> for CompactValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Capture)
    }
}

struct Capture;

impl<'de> Visitor<'de> for Capture {
    type Value = CompactValue;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(CompactValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(CompactValue::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(CompactValue::Number(value.into()))
    }

    fn visit_i128<E: de::Error>(self, number: i128) -> Result<Self::Value, E> {
        Number::deserialize(value::I128Deserializer::new(number)).map(CompactValue::Number)
    }

    fn visit_u128<E: de::Error>(self, number: u128) -> Result<Self::Value, E> {
        Number::deserialize(value::U128Deserializer::new(number)).map(CompactValue::Number)
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        // Value's generic Serde capture represents non-finite numbers as null.
        Ok(Number::from_f64(value).map_or(CompactValue::Null, CompactValue::Number))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(CompactValue::String(value))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(CompactValue::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(CompactValue::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        CompactValue::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = input.next_element()? {
            values.push(value);
        }
        Ok(CompactValue::Array(values.into_boxed_slice()))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
        let Some(first) = input.next_key_seed(FirstKey)? else {
            return Ok(CompactValue::Object(Box::default()));
        };
        // These are Serde transport protocols, not ordinary JSON field names.
        // Delegate their interpretation (including feature-dependent behavior,
        // validation and consumption) to Value itself. Only these rare inputs
        // use the old intermediate tree; ordinary objects stay compact.
        if matches!(
            first.as_str(),
            "$serde_json::private::RawValue" | "$serde_json::private::Number"
        ) {
            return Value::deserialize(value::MapAccessDeserializer::new(ReplayKey {
                first: Some(first),
                input,
            }))
            .map(CompactValue::from);
        }

        let mut order = Map::new();
        let mut values = Vec::new();
        let mut key = first;
        loop {
            let value = input.next_value()?;
            match order.entry(key) {
                serde_json::map::Entry::Occupied(entry) => {
                    // All index values are created below from this Vec's length.
                    let index = entry.get().as_u64().expect("object indices are unsigned") as usize;
                    values[index] = value;
                }
                serde_json::map::Entry::Vacant(entry) => {
                    entry.insert(Value::from(values.len()));
                    values.push(value);
                }
            }
            match input.next_key()? {
                Some(next) => key = next,
                None => break,
            }
        }
        Ok(CompactValue::Object(
            order
                .into_iter()
                .map(|(key, index)| {
                    // Entries never escape this function or contain input data.
                    let index = index.as_u64().expect("object indices are unsigned") as usize;
                    (key, mem::replace(&mut values[index], CompactValue::Null))
                })
                .collect(),
        ))
    }
}

// Value classifies its first key using deserialize_str, then uses ordinary
// String deserialization for subsequent keys. Keep that generic Serde contract.
struct FirstKey;

impl<'de> DeserializeSeed<'de> for FirstKey {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for FirstKey {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        Ok(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<String, E> {
        Ok(value)
    }
}

struct ReplayKey<A> {
    first: Option<String>,
    input: A,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for ReplayKey<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        match self.first.take() {
            Some(key) => seed.deserialize(key.into_deserializer()).map(Some),
            None => self.input.next_key_seed(seed),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        self.input.next_value_seed(seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Error};

    fn assert_capture(input: &str) {
        let expected = serde_json::from_str::<Value>(input);
        let actual = serde_json::from_str::<CompactValue>(input);
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => {
                assert_eq!(actual.to_value(), expected, "{input}");
                assert_eq!(
                    serde_json::to_string(&actual).unwrap(),
                    serde_json::to_string(&expected).unwrap(),
                    "{input}"
                );
            }
            (Err(expected), Err(actual)) => {
                assert_eq!(actual.to_string(), expected.to_string());
                assert_eq!(actual.classify(), expected.classify());
            }
            (expected, actual) => {
                panic!("capture mismatch for {input}: {expected:?} vs {actual:?}")
            }
        }
    }

    #[test]
    fn streaming_capture_matches_value_order_duplicates_and_number_kinds() {
        for input in [
            "null",
            "true",
            "false",
            "0",
            "-0",
            "0.0",
            "-0.0",
            "1",
            "1.0",
            "1e0",
            "18446744073709551615",
            "-9223372036854775808",
            "18446744073709551616",
            "5e-324",
            "1.7976931348623157e308",
            "1e309",
            r#""a\u0000\"😀""#,
            "[]",
            "{}",
            "[0,-0.0,null,{},[]]",
            r#"{"z":0,"a":1,"z":{"z":2,"a":null},"b":[1,2,1]}"#,
            r#"{"future":{"z":1,"a":2,"z":3},"known":null}"#,
        ] {
            assert_capture(input);
        }
    }

    #[test]
    fn reserved_protocol_keys_keep_values_capture_behavior() {
        for input in [
            r#"{"$serde_json::private::RawValue":"[1,-0.0,{}]"}"#,
            r#"{"$serde_json::private::RawValue":"{\"a\":1,\"a\":2}"}"#,
            r#"{"$serde_json::private::RawValue":"broken"}"#,
            r#"{"$serde_json::private::RawValue":null}"#,
            r#"{"$serde_json::private::RawValue":"{}","extra":1}"#,
            r#"{"ordinary":1,"$serde_json::private::RawValue":"{}"}"#,
            r#"{"$serde_json::private::Number":"1.000"}"#,
            r#"{"$serde_json::private::Number":12,"extra":[1]}"#,
            r#"{"nested":{"$serde_json::private::RawValue":"-0.0"}}"#,
        ] {
            assert_capture(input);
        }
    }

    #[test]
    fn generic_capture_preserves_number_bits_and_nonfinite_policy() {
        for number in [
            0.0,
            -0.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::MAX,
            1.2345678901234567,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let expected =
                Value::deserialize(value::F64Deserializer::<Error>::new(number)).unwrap();
            let actual =
                CompactValue::deserialize(value::F64Deserializer::<Error>::new(number)).unwrap();
            assert_eq!(
                serde_json::to_vec(&actual).unwrap(),
                serde_json::to_vec(&expected).unwrap()
            );
        }
        for number in [
            i128::MIN,
            i64::MIN as i128,
            -1,
            0,
            i64::MAX as i128,
            i128::MAX,
        ] {
            let expected = Value::deserialize(value::I128Deserializer::<Error>::new(number));
            let actual = CompactValue::deserialize(value::I128Deserializer::<Error>::new(number));
            assert_eq!(
                actual.map(|v| v.to_value()).map_err(|e| e.to_string()),
                expected.map_err(|e| e.to_string())
            );
        }
        for number in [0, u64::MAX as u128, u128::MAX] {
            let expected = Value::deserialize(value::U128Deserializer::<Error>::new(number));
            let actual = CompactValue::deserialize(value::U128Deserializer::<Error>::new(number));
            assert_eq!(
                actual.map(|v| v.to_value()).map_err(|e| e.to_string()),
                expected.map_err(|e| e.to_string())
            );
        }
    }

    #[test]
    fn nested_generic_capture_matches_owned_and_borrowed_values() {
        let mut value = json!({"empty":{},"array":[],"number":-0.0,"null":null});
        for depth in 0..12 {
            value = json!({"z":value,"a":depth,"array":[true,depth,"😀"]});
            let borrowed = CompactValue::deserialize(&value).unwrap();
            let owned = CompactValue::deserialize(value.clone()).unwrap();
            assert_eq!(
                serde_json::to_vec(&borrowed).unwrap(),
                serde_json::to_vec(&value).unwrap()
            );
            assert_eq!(borrowed, owned);
        }
    }
}
