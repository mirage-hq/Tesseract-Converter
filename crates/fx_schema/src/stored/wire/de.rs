//! Borrowed deserialization from compact stored JSON.
//!
//! The adapter structure follows serde_json 1.0.149's borrowed `Value`
//! deserializer (`src/value/de.rs`), adapted under its MIT license below.
//! Compact storage differs only in its array and object containers.

/*
From https://github.com/serde-rs/json/tree/v1.0.149 (LICENSE-MIT):

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
*/

use std::slice;

use serde::de::{
    self, Deserialize, DeserializeSeed, EnumAccess, Expected, IntoDeserializer, MapAccess,
    SeqAccess, Unexpected, VariantAccess, Visitor,
};
use serde::{forward_to_deserialize_any, Deserializer};
use serde_json::Error;

use super::CompactValue;

const RAW_VALUE_TOKEN: &str = "$serde_json::private::RawValue";

macro_rules! deserialize_number {
    ($method:ident) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Error>
        where
            V: Visitor<'de>,
        {
            match self {
                CompactValue::Number(value) => value.$method(visitor),
                _ => Err(self.invalid_type(&visitor)),
            }
        }
    };
}

macro_rules! deserialize_numeric_key {
    ($method:ident) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Error>
        where
            V: Visitor<'de>,
        {
            if !matches!(self.key.as_bytes().first(), Some(b'0'..=b'9' | b'-')) {
                return Err(numeric_key_error());
            }
            let mut parser = serde_json::Deserializer::from_str(self.key);
            let value = parser.$method(visitor)?;
            // Unlike JSON documents, Value's numeric keys permit no trailing
            // whitespace. Run the visitor first to preserve error precedence.
            if self.key.ends_with([' ', '\t', '\r', '\n']) || parser.end().is_err() {
                return Err(numeric_key_error());
            }
            Ok(value)
        }
    };
}

#[cold]
fn numeric_key_error() -> Error {
    // serde_json exposes no constructor for ExpectedNumericKey. Error::custom
    // has the same text but the wrong category (Data rather than Syntax).
    // Ask its Value key reader for the exact error using a tiny constant map;
    // no input subtree is materialized on this failure-only path.
    serde_json::from_value::<std::collections::BTreeMap<u8, ()>>(serde_json::json!({"": null}))
        .expect_err("an empty string cannot be a numeric JSON key")
}

impl<'de> Deserializer<'de> for &'de CompactValue {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Null => visitor.visit_unit(),
            CompactValue::Bool(value) => visitor.visit_bool(*value),
            CompactValue::Number(value) => value.deserialize_any(visitor),
            CompactValue::String(value) => visitor.visit_borrowed_str(value),
            CompactValue::Array(values) => visit_array(values, visitor),
            CompactValue::Object(values) => visit_object(values, visitor),
        }
    }

    deserialize_number!(deserialize_i8);
    deserialize_number!(deserialize_i16);
    deserialize_number!(deserialize_i32);
    deserialize_number!(deserialize_i64);
    deserialize_number!(deserialize_i128);
    deserialize_number!(deserialize_u8);
    deserialize_number!(deserialize_u16);
    deserialize_number!(deserialize_u32);
    deserialize_number!(deserialize_u64);
    deserialize_number!(deserialize_u128);
    deserialize_number!(deserialize_f32);
    deserialize_number!(deserialize_f64);

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Object(values) => deserialize_object_enum(values, visitor),
            CompactValue::String(variant) => visitor.visit_enum(EnumDeserializer {
                variant,
                value: None,
            }),
            other => Err(de::Error::invalid_type(
                other.unexpected(),
                &"string or map",
            )),
        }
    }

    fn deserialize_newtype_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        if name == RAW_VALUE_TOKEN {
            // serde_json's borrowed Value reader also serializes the Value into
            // an owned string for this opt-in protocol. Keep ordinary decoding
            // allocation-free and limit the fallback to RawValue requests.
            return self.to_value().deserialize_newtype_struct(name, visitor);
        }

        visitor.visit_newtype_struct(self)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Bool(value) => visitor.visit_bool(*value),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::String(value) => visitor.visit_borrowed_str(value),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::String(value) => visitor.visit_borrowed_str(value),
            CompactValue::Array(values) => visit_array(values, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Null => visitor.visit_unit(),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Array(values) => visit_array(values, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Object(values) => visit_object(values, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self {
            CompactValue::Array(values) => visit_array(values, visitor),
            CompactValue::Object(values) => visit_object(values, visitor),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

impl<'de> IntoDeserializer<'de, Error> for &'de CompactValue {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self::Deserializer {
        self
    }
}

fn visit_array<'de, V>(values: &'de [CompactValue], visitor: V) -> Result<V::Value, Error>
where
    V: Visitor<'de>,
{
    let len = values.len();
    let mut deserializer = SeqDeserializer::new(values);
    let value = visitor.visit_seq(&mut deserializer)?;
    if deserializer.iter.len() == 0 {
        Ok(value)
    } else {
        Err(de::Error::invalid_length(len, &"fewer elements in array"))
    }
}

fn visit_object<'de, V>(
    values: &'de [(String, CompactValue)],
    visitor: V,
) -> Result<V::Value, Error>
where
    V: Visitor<'de>,
{
    let len = values.len();
    let mut deserializer = MapDeserializer::new(values);
    let value = visitor.visit_map(&mut deserializer)?;
    if deserializer.iter.len() == 0 {
        Ok(value)
    } else {
        Err(de::Error::invalid_length(len, &"fewer elements in map"))
    }
}

fn deserialize_object_enum<'de, V>(
    values: &'de [(String, CompactValue)],
    visitor: V,
) -> Result<V::Value, Error>
where
    V: Visitor<'de>,
{
    let [(variant, value)] = values else {
        return Err(de::Error::invalid_value(
            Unexpected::Map,
            &"map with a single key",
        ));
    };
    visitor.visit_enum(EnumDeserializer {
        variant,
        value: Some(value),
    })
}

struct SeqDeserializer<'de> {
    iter: slice::Iter<'de, CompactValue>,
}

impl<'de> SeqDeserializer<'de> {
    fn new(values: &'de [CompactValue]) -> Self {
        Self {
            iter: values.iter(),
        }
    }
}

impl<'de> SeqAccess<'de> for SeqDeserializer<'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.iter
            .next()
            .map(|value| seed.deserialize(value))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct MapDeserializer<'de> {
    iter: slice::Iter<'de, (String, CompactValue)>,
    value: Option<&'de CompactValue>,
}

impl<'de> MapDeserializer<'de> {
    fn new(values: &'de [(String, CompactValue)]) -> Self {
        Self {
            iter: values.iter(),
            value: None,
        }
    }
}

impl<'de> MapAccess<'de> for MapDeserializer<'de> {
    type Error = Error;

    fn next_key_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(MapKeyDeserializer { key }).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Error>
    where
        T: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .take()
            .ok_or_else(|| de::Error::custom("value is missing"))?;
        seed.deserialize(value)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct MapKeyDeserializer<'de> {
    key: &'de str,
}

impl<'de> Deserializer<'de> for MapKeyDeserializer<'de> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_borrowed_str(self.key)
    }

    deserialize_numeric_key!(deserialize_i8);
    deserialize_numeric_key!(deserialize_i16);
    deserialize_numeric_key!(deserialize_i32);
    deserialize_numeric_key!(deserialize_i64);
    deserialize_numeric_key!(deserialize_i128);
    deserialize_numeric_key!(deserialize_u8);
    deserialize_numeric_key!(deserialize_u16);
    deserialize_numeric_key!(deserialize_u32);
    deserialize_numeric_key!(deserialize_u64);
    deserialize_numeric_key!(deserialize_u128);
    deserialize_numeric_key!(deserialize_f32);
    deserialize_numeric_key!(deserialize_f64);

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self.key {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(de::Error::invalid_type(Unexpected::Str(self.key), &visitor)),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        self.key
            .into_deserializer()
            .deserialize_enum(name, variants, visitor)
    }

    forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct
        map struct identifier ignored_any
    }
}

struct EnumDeserializer<'de> {
    variant: &'de str,
    value: Option<&'de CompactValue>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer<'de> {
    type Error = Error;
    type Variant = VariantDeserializer<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(self.variant.into_deserializer())?;
        Ok((variant, VariantDeserializer { value: self.value }))
    }
}

struct VariantDeserializer<'de> {
    value: Option<&'de CompactValue>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.value {
            Some(value) => Deserialize::deserialize(value),
            None => Ok(()),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.value {
            Some(value) => seed.deserialize(value),
            None => Err(de::Error::invalid_type(
                Unexpected::UnitVariant,
                &"newtype variant",
            )),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(CompactValue::Array(values)) if values.is_empty() => visitor.visit_unit(),
            Some(CompactValue::Array(values)) => visit_array(values, visitor),
            Some(other) => Err(de::Error::invalid_type(
                other.unexpected(),
                &"tuple variant",
            )),
            None => Err(de::Error::invalid_type(
                Unexpected::UnitVariant,
                &"tuple variant",
            )),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(CompactValue::Object(values)) => visit_object(values, visitor),
            Some(other) => Err(de::Error::invalid_type(
                other.unexpected(),
                &"struct variant",
            )),
            None => Err(de::Error::invalid_type(
                Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }
}

impl CompactValue {
    fn invalid_type<E>(&self, expected: &dyn Expected) -> E
    where
        E: de::Error,
    {
        de::Error::invalid_type(self.unexpected(), expected)
    }

    fn unexpected(&self) -> Unexpected<'_> {
        match self {
            Self::Null => Unexpected::Unit,
            Self::Bool(value) => Unexpected::Bool(*value),
            Self::Number(value) => {
                if let Some(value) = value.as_u64() {
                    Unexpected::Unsigned(value)
                } else if let Some(value) = value.as_i64() {
                    Unexpected::Signed(value)
                } else if let Some(value) = value.as_f64() {
                    Unexpected::Float(value)
                } else {
                    Unexpected::Other("number")
                }
            }
            Self::String(value) => Unexpected::Str(value),
            Self::Array(_) => Unexpected::Seq,
            Self::Object(_) => Unexpected::Map,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt;

    use serde::de::{DeserializeOwned, IgnoredAny};
    use serde::Deserialize;
    use serde_json::{json, value::RawValue, Value};

    use super::*;

    fn assert_same<T>(value: Value)
    where
        T: DeserializeOwned + fmt::Debug + PartialEq,
    {
        let compact = CompactValue::from(value.clone());
        let expected = T::deserialize(&value);
        let actual = T::deserialize(&compact);
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => assert_eq!(actual, expected),
            (Err(expected), Err(actual)) => {
                assert_eq!(actual.to_string(), expected.to_string());
                assert_eq!(actual.classify(), expected.classify());
            }
            (expected, actual) => {
                panic!("different results: Value={expected:?}, compact={actual:?}");
            }
        }
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Record {
        name: String,
        enabled: bool,
        count: u64,
        optional: Option<i32>,
        ignored: IgnoredAny,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    enum Choice {
        Unit,
        Newtype(u64),
        Tuple(i16, String),
        Struct { active: bool },
    }

    #[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    struct NumericKey(i16);

    #[derive(Debug, PartialEq)]
    struct Bytes(Vec<u8>);

    impl<'de> Deserialize<'de> for Bytes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct BytesVisitor;

            impl<'de> Visitor<'de> for BytesVisitor {
                type Value = Bytes;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("a string or byte array")
                }

                fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
                where
                    E: de::Error,
                {
                    Ok(Bytes(value.as_bytes().to_vec()))
                }

                fn visit_seq<A>(self, mut values: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    let mut bytes = Vec::with_capacity(values.size_hint().unwrap_or(0));
                    while let Some(value) = values.next_element()? {
                        bytes.push(value);
                    }
                    Ok(Bytes(bytes))
                }
            }

            deserializer.deserialize_bytes(BytesVisitor)
        }
    }

    #[derive(Debug, PartialEq)]
    enum NumberKind {
        Signed(i64),
        Unsigned(u64),
        Float(u64),
    }

    impl<'de> Deserialize<'de> for NumberKind {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct NumberVisitor;

            impl Visitor<'_> for NumberVisitor {
                type Value = NumberKind;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("a JSON number")
                }

                fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                    Ok(NumberKind::Signed(value))
                }

                fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                    Ok(NumberKind::Unsigned(value))
                }

                fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                    Ok(NumberKind::Float(value.to_bits()))
                }
            }

            deserializer.deserialize_any(NumberVisitor)
        }
    }

    #[derive(Debug, PartialEq)]
    struct FirstMapEntry;

    impl<'de> Deserialize<'de> for FirstMapEntry {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct FirstMapEntryVisitor;

            impl<'de> Visitor<'de> for FirstMapEntryVisitor {
                type Value = FirstMapEntry;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("a nonempty map")
                }

                fn visit_map<A>(self, mut values: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    let _: Option<(IgnoredAny, IgnoredAny)> = values.next_entry()?;
                    Ok(FirstMapEntry)
                }
            }

            deserializer.deserialize_map(FirstMapEntryVisitor)
        }
    }

    #[test]
    fn scalars_options_structs_and_borrowed_strings_match_value() {
        assert_same::<bool>(json!(true));
        assert_same::<i64>(json!(i64::MIN));
        assert_same::<u64>(json!(u64::MAX));
        assert_same::<f64>(json!(-0.0));
        assert_same::<f64>(json!(f64::from_bits(1)));
        assert_same::<i8>(json!(128));
        assert_same::<u8>(json!(-1));
        assert_same::<u128>(json!(-1));
        assert_same::<String>(json!("hello"));
        assert_same::<Bytes>(json!("hello"));
        assert_same::<Bytes>(json!([0, 1, 127, 255]));
        assert_same::<NumberKind>(json!(-1));
        assert_same::<NumberKind>(json!(u64::MAX));
        assert_same::<NumberKind>(json!(-0.0));
        assert_same::<NumberKind>(json!(f64::from_bits(1)));
        assert_same::<Option<u8>>(Value::Null);
        assert_same::<Option<u8>>(json!(7));
        assert_same::<Record>(json!({
            "name": "stored",
            "enabled": true,
            "count": u64::MAX,
            "optional": null,
            "ignored": {"deep": [1, 2, 3]},
        }));

        let value = json!("borrowed");
        let compact = CompactValue::from(value.clone());
        assert_eq!(
            <&str>::deserialize(&compact).unwrap(),
            <&str>::deserialize(&value).unwrap()
        );
    }

    #[test]
    fn enums_and_struct_sequence_forms_match_value() {
        assert_same::<Choice>(json!("Unit"));
        assert_same::<Choice>(json!({"Newtype": u64::MAX}));
        assert_same::<Choice>(json!({"Tuple": [-12, "value"]}));
        assert_same::<Choice>(json!({"Struct": {"active": true}}));
        assert_same::<Record>(json!(["stored", true, 3, null, [1, 2]]));
    }

    #[test]
    fn numeric_bool_char_and_newtype_map_keys_match_value() {
        assert_same::<BTreeMap<i64, String>>(json!({"-2": "a", "7": "b"}));
        assert_same::<BTreeMap<bool, u8>>(json!({"false": 0, "true": 1}));
        assert_same::<BTreeMap<char, u8>>(json!({"x": 1, "日": 2}));
        assert_same::<BTreeMap<NumericKey, bool>>(json!({"-9": true, "12": false}));
    }

    #[test]
    fn enum_map_key_borrowing_matches_value() {
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
        struct Key(String);
        impl<'de> Deserialize<'de> for Key {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct KeyVisitor;
                impl<'de> Visitor<'de> for KeyVisitor {
                    type Value = Key;
                    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        f.write_str("an enum key")
                    }
                    fn visit_enum<A: EnumAccess<'de>>(self, input: A) -> Result<Key, A::Error> {
                        let (name, variant) = input.variant::<&str>()?;
                        variant.unit_variant()?;
                        Ok(Key(name.to_owned()))
                    }
                }
                deserializer.deserialize_enum("Key", &[], KeyVisitor)
            }
        }
        assert_same::<BTreeMap<Key, ()>>(json!({"Unit": null}));
    }

    #[test]
    fn numeric_map_keys_match_value_for_trailing_input_and_ranges() {
        for key in [
            "1 ", "1\n", "1x", " 1", "01", "+1", "1.0", "1e2", "128", "-129", "-1",
        ] {
            assert_same::<BTreeMap<i8, ()>>(json!({key: null}));
            assert_same::<BTreeMap<u8, ()>>(json!({key: null}));
            assert_same::<BTreeMap<i128, ()>>(json!({key: null}));
        }
    }

    #[test]
    fn numeric_map_key_visitor_dispatch_matches_value() {
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
        struct Key(u64);
        impl<'de> Deserialize<'de> for Key {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct KeyVisitor;
                impl Visitor<'_> for KeyVisitor {
                    type Value = Key;
                    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        f.write_str("an unsigned JSON number")
                    }
                    fn visit_u64<E: de::Error>(self, n: u64) -> Result<Key, E> {
                        Ok(Key(n))
                    }
                }
                // A method hint is not permission to change the visitor method
                // or narrow the value before the user's visitor sees it.
                deserializer.deserialize_i8(KeyVisitor)
            }
        }
        assert_same::<BTreeMap<Key, ()>>(json!({"1": null, "255": null}));
    }

    #[test]
    fn mismatches_and_trailing_container_entries_match_value_errors() {
        assert_same::<u64>(json!("12"));
        assert_same::<bool>(json!(0));
        assert_same::<Record>(json!(false));
        assert_same::<(u8, u8)>(json!([1, 2, 3]));
        assert_same::<FirstMapEntry>(json!({"a": 1, "b": 2}));
        assert_same::<Choice>(json!({"Unit": null, "Newtype": 1}));
        assert_same::<BTreeMap<i64, ()>>(json!({"not-a-number": null}));
    }

    #[test]
    fn raw_value_protocol_matches_value_with_owned_fallback() {
        for value in [
            Value::Null,
            json!("text"),
            json!(-0.0),
            json!([1, true, {"nested": "value"}]),
            json!({"$serde_json::private::RawValue": "ordinary object field"}),
        ] {
            let compact = CompactValue::from(value.clone());
            let expected = Box::<RawValue>::deserialize(&value).unwrap();
            let actual = Box::<RawValue>::deserialize(&compact).unwrap();
            assert_eq!(actual.get(), expected.get());

            let expected = <&RawValue>::deserialize(&value).unwrap_err();
            let actual = <&RawValue>::deserialize(&compact).unwrap_err();
            assert_eq!(actual.to_string(), expected.to_string());
        }
    }
}
