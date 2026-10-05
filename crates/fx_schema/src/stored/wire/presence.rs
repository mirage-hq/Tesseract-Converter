//! Compare serialized known fields with compact wire data without building JSON trees.
//!
//! Like `collect_unknown`, only object membership matters; arrays are zipped and
//! scalar values (including mismatched JSON kinds) are not compared.
use super::CompactValue;
use serde::{ser, Serialize};
use serde_json::{Error, Value};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

type Result<T = bool> = std::result::Result<T, Error>;

pub(super) struct Presence<'a> {
    pub(super) raw: Option<&'a CompactValue>,
    // Nested Stored records are checked separately against their typed data.
    pub(super) skip: &'a [(usize, &'static str)],
    pub(super) depth: usize,
}

impl<'a> Presence<'a> {
    fn child(&self, raw: Option<&'a CompactValue>) -> Self {
        Self {
            raw,
            skip: self.skip,
            depth: self.depth + 1,
        }
    }

    fn variant(self, name: &str) -> (Self, bool) {
        let fields = match self.raw {
            Some(CompactValue::Object(fields)) => &**fields,
            _ => &[],
        };
        let raw = fields
            .iter()
            .rfind(|(key, _)| key == name)
            .map(|(_, value)| value);
        (self.child(raw), fields.iter().any(|(key, _)| key != name))
    }
}

macro_rules! scalar {
    ($($method:ident($ty:ty)),* $(,)?) => {$(
        fn $method(self, _: $ty) -> Result { Ok(false) }
    )*};
}

impl<'a> ser::Serializer for Presence<'a> {
    type Ok = bool;
    type Error = Error;
    type SerializeSeq = Sequence<'a>;
    type SerializeTuple = Sequence<'a>;
    type SerializeTupleStruct = Sequence<'a>;
    type SerializeTupleVariant = Sequence<'a>;
    type SerializeMap = Object<'a>;
    type SerializeStruct = Object<'a>;
    type SerializeStructVariant = Object<'a>;

    scalar!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8])
    );

    fn serialize_none(self) -> Result {
        Ok(false)
    }
    fn serialize_unit(self) -> Result {
        Ok(false)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result {
        Ok(false)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result {
        Ok(false)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result {
        value.serialize(self)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result {
        let (child, unknown) = self.variant(variant);
        Ok(unknown || value.serialize(child)?)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Sequence<'a>> {
        Ok(Sequence {
            presence: self,
            index: 0,
            unknown: false,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<Sequence<'a>> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Sequence<'a>> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Sequence<'a>> {
        let (presence, unknown) = self.variant(variant);
        Ok(Sequence {
            presence,
            index: 0,
            unknown,
        })
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Object<'a>> {
        Ok(Object::new(self, false))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Object<'a>> {
        self.serialize_map(None)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Object<'a>> {
        let (presence, unknown) = self.variant(variant);
        Ok(Object::new(presence, unknown))
    }
}

pub(super) struct Sequence<'a> {
    presence: Presence<'a>,
    index: usize,
    unknown: bool,
}

impl Sequence<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        if !self.unknown {
            if let Some(CompactValue::Array(raw)) = self.presence.raw {
                if let Some(raw) = raw.get(self.index) {
                    self.unknown = value.serialize(self.presence.child(Some(raw)))?;
                }
            }
        }
        self.index += 1;
        Ok(())
    }
}

macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for Sequence<'_> {
            type Ok = bool;
            type Error = Error;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
                self.element(value)
            }
            fn end(self) -> Result {
                Ok(self.unknown)
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);

pub(super) struct Object<'a> {
    presence: Presence<'a>,
    fields: &'a [(String, CompactValue)],
    // Most records have only a handful of fields. Index larger maps (e.g.
    // layer references or opaque payloads) to avoid quadratic comparisons.
    index: Option<HashMap<&'a str, usize>>,
    // Per-key results also preserve serde_json's last-wins map semantics.
    matched: Vec<Option<bool>>,
    pending: Option<String>,
    unknown: bool,
}

impl<'a> Object<'a> {
    fn new(presence: Presence<'a>, unknown: bool) -> Self {
        let fields = match presence.raw {
            Some(CompactValue::Object(fields)) => &**fields,
            _ => &[],
        };
        Self {
            presence,
            fields,
            index: (fields.len() > 16).then(|| {
                fields
                    .iter()
                    .enumerate()
                    .map(|(index, (key, _))| (key.as_str(), index))
                    .collect()
            }),
            matched: vec![None; fields.len()],
            pending: None,
            unknown,
        }
    }

    fn field<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> Result<()> {
        if self.unknown {
            return Ok(());
        }
        let index = match &self.index {
            Some(index) => index.get(key).copied(),
            None => self.fields.iter().rposition(|(name, _)| name == key),
        };
        if let Some(index) = index {
            let skip = self
                .presence
                .skip
                .iter()
                .any(|&(depth, name)| depth == self.presence.depth && name == key);
            self.matched[index] =
                Some(!skip && value.serialize(self.presence.child(Some(&self.fields[index].1)))?);
        }
        Ok(())
    }

    fn finish(self) -> Result {
        Ok(self.unknown
            || self.fields.iter().enumerate().any(|(index, (key, _))| {
                self.matched[index] != Some(false)
                    && match &self.index {
                        Some(last) => last.get(key.as_str()) == Some(&index),
                        None => !self.fields[index + 1..]
                            .iter()
                            .any(|(later, _)| later == key),
                    }
            }))
    }
}

impl ser::SerializeMap for Object<'_> {
    type Ok = bool;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<()> {
        // Use serde_json's map-key serializer, including numeric/enum key rules.
        let mut map = ser::Serializer::serialize_map(serde_json::value::Serializer, Some(1))?;
        map.serialize_entry(key, &())?;
        let Value::Object(value) = map.end()? else {
            unreachable!("serde_json map serializer always produces an object")
        };
        self.pending = value.into_iter().next().map(|(key, _)| key);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        let key = self
            .pending
            .take()
            .expect("checked map serialization pairs keys and values");
        self.field(&key, value)
    }

    fn end(self) -> Result {
        self.finish()
    }
}

macro_rules! record {
    ($trait:ident) => {
        impl ser::$trait for Object<'_> {
            type Ok = bool;
            type Error = Error;
            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> Result<()> {
                self.field(key, value)
            }
            fn end(self) -> Result {
                self.finish()
            }
        }
    };
}
record!(SerializeStruct);
record!(SerializeStructVariant);
