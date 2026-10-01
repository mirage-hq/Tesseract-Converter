//! Lossless JSON records with immutable, structurally checked data views.
//!
//! The data reader must only decode and validate storage fields. This wrapper
//! does not make a normalizing or migrating reader suitable for storage use.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

mod finite;
mod wire;

use wire::Wire;

/// Serialize authored data without allowing JSON's non-finite-to-null coercion.
pub(crate) fn checked_value(data: &(impl Serialize + ?Sized)) -> Result<Value, serde_json::Error> {
    data.serialize(finite::Finite)?;
    serde_json::to_value(data)
}

/// Keeps the original JSON separate from its read-only typed view.
///
/// The immutable wire tree is compact and shared by clones. A full JSON value
/// is materialized only when explicitly requested, not for reads or serialization.
/// This avoids retaining a full map-based subtree at every nested record.
///
/// No mutable view is exposed: modifying the decoded fields alone would leave
/// serialization stale. An edited record must be constructed and checked anew.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stored<T> {
    wire: Arc<Wire>,
    data: T,
}

impl<T> Stored<T> {
    pub(crate) fn data(&self) -> &T {
        &self.data
    }

    pub(crate) fn wire_value(&self) -> &Value {
        self.wire.value()
    }

    /// Checks field presence without materializing a nested JSON tree.
    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.wire.contains_key(key)
    }
}

impl<T: DeserializeOwned> Stored<T> {
    pub(crate) fn from_value(wire: Value) -> Result<Self, serde_json::Error> {
        Self::from_wire(Wire::new(wire))
    }

    fn from_wire(wire: Wire) -> Result<Self, serde_json::Error> {
        let data = wire.decode()?;
        Ok(Self {
            wire: Arc::new(wire),
            data,
        })
    }
}

impl<T: DeserializeOwned + Serialize> Stored<T> {
    /// Applies the same checks to authored data as to a JSON input.
    pub(crate) fn from_data(data: &T) -> Result<Self, serde_json::Error> {
        Self::from_value(checked_value(data)?)
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Stored<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_wire(Wire::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl<T> Serialize for Stored<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wire.as_ref().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Record {
        count: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(default)]
        items: Vec<u64>,
    }

    #[test]
    fn retains_unknown_nested_data() {
        let wire = json!({"count": 3, "future": {"nested": [null, {"x": true}]}});
        let record: Stored<Record> = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(record.data().count, 3);
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn does_not_materialize_absent_defaults() {
        let wire = json!({"count": 3});
        let record = Stored::<Record>::from_value(wire.clone()).unwrap();
        assert_eq!(record.data().note, None);
        assert!(record.data().items.is_empty());
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn retains_explicit_null() {
        let wire = json!({"count": 3, "note": null});
        let record = Stored::<Record>::from_value(wire.clone()).unwrap();
        assert_eq!(record.data().note, None);
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }

    #[test]
    fn retains_array_order_and_duplicates() {
        let wire = json!({"count": 3, "items": [8, 2, 8, 1]});
        let record = Stored::<Record>::from_value(wire.clone()).unwrap();
        assert_eq!(record.data().items, [8, 2, 8, 1]);
        assert_eq!(record.wire_value(), &wire);
    }

    #[test]
    fn malformed_known_fields_never_fall_back_to_opaque_data() {
        for wire in [
            json!({"count": "3", "future": true}),
            json!({"count": -1}),
            json!({"count": 3, "items": ["bad"]}),
            json!({"future": {"count": 3}}),
        ] {
            assert!(Stored::<Record>::from_value(wire).is_err());
        }
    }

    #[test]
    fn authored_data_uses_the_checked_reader() {
        #[derive(Debug, Serialize, Deserialize)]
        #[serde(try_from = "u64", into = "u64")]
        #[derive(Clone)]
        struct Positive(u64);

        impl TryFrom<u64> for Positive {
            type Error = &'static str;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                if value == 0 {
                    Err("must be positive")
                } else {
                    Ok(Self(value))
                }
            }
        }

        impl From<Positive> for u64 {
            fn from(value: Positive) -> Self {
                value.0
            }
        }

        assert!(Stored::from_data(&Positive(0)).is_err());
        let valid = Stored::from_data(&Positive(2)).unwrap();
        assert_eq!(valid.data().0, 2);
        assert_eq!(valid.wire_value(), &json!(2));
    }

    #[test]
    fn a_failed_candidate_does_not_change_an_existing_record() {
        let record = Stored::<Record>::from_value(json!({"count": 3})).unwrap();
        let before = record.clone();
        let mut candidate = record.wire_value().clone();
        candidate["count"] = json!("invalid");
        assert!(Stored::<Record>::from_value(candidate).is_err());
        assert_eq!(record, before);
    }

    #[test]
    fn optional_non_finite_numbers_cannot_silently_become_null() {
        #[derive(Serialize, Deserialize)]
        struct OptionalNumber {
            values: Vec<Option<f64>>,
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Stored::from_data(&OptionalNumber {
                values: vec![Some(value)]
            })
            .is_err());
        }
        let stored = Stored::from_data(&OptionalNumber {
            values: vec![None, Some(1.5)],
        })
        .unwrap();
        assert_eq!(
            stored.wire_value(),
            &serde_json::json!({"values": [null, 1.5]})
        );
    }

    #[test]
    fn clones_share_the_immutable_wire_view() {
        let record = Stored::<Record>::from_value(json!({"count": 3})).unwrap();
        let cloned = record.clone();
        assert!(std::ptr::eq(record.wire_value(), cloned.wire_value()));
    }

    #[test]
    fn clones_share_lazy_wire_without_changing_equality() {
        let input = json!({"count": 3, "unknown": {"path": [1, 2, 1]}});
        let record = Stored::<Record>::from_value(input.clone()).unwrap();
        let cloned = record.clone();
        let independent = Stored::<Record>::from_value(input.clone()).unwrap();
        assert!(Arc::ptr_eq(&record.wire, &cloned.wire));
        assert_eq!(record, independent);
        assert_eq!(serde_json::to_value(&record).unwrap(), input);
        // The finite checker is a non-JSON serializer used for authored data.
        assert_eq!(checked_value(&record).unwrap(), input);
        assert!(!record.wire.is_materialized());
        assert!(!independent.wire.is_materialized());
        assert_eq!(cloned.wire_value(), &input);
        assert!(std::ptr::eq(record.wire_value(), cloned.wire_value()));
        assert_eq!(record, independent);
        assert!(!independent.wire.is_materialized());

        let mut edited = cloned.wire_value().clone();
        edited["count"] = json!(4);
        let edited = Stored::<Record>::from_value(edited).unwrap();
        assert_eq!(record.wire_value(), &input);
        assert_ne!(record, edited);
    }

    #[test]
    fn nested_geometry_stays_compact_through_clone_and_serialization() {
        #[derive(Debug, Clone, PartialEq, Deserialize)]
        struct Node {
            #[serde(default)]
            children: Vec<Stored<Node>>,
        }
        fn assert_compact(node: &Stored<Node>) {
            assert!(!node.wire.is_materialized());
            for child in &node.data().children {
                assert_compact(child);
            }
        }
        let commands: Vec<_> = (0..1000)
            .map(|i| json!({"type": "lineTo", "x": i, "y": i}))
            .collect();
        let mut input = json!({"path": commands});
        for _ in 0..8 {
            input = json!({"children": [input], "future": null});
        }
        let record: Stored<Node> =
            serde_json::from_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let from_value = Stored::<Node>::from_value(input.clone()).unwrap();
        assert_eq!(record, from_value);
        assert_compact(&from_value);
        let cloned = record.clone();
        assert_eq!(serde_json::to_value(&cloned).unwrap(), input);
        assert_eq!(record, cloned);
        assert_compact(&record);
        assert_compact(&cloned);
        let mut original = &record;
        let mut copy = &cloned;
        while let Some(child) = original.data().children.first() {
            copy = &copy.data().children[0];
            assert!(Arc::ptr_eq(&child.wire, &copy.wire));
            original = child;
        }
    }

    #[test]
    fn streaming_records_keep_duplicate_last_wins_and_checked_nested_fields() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Parent {
            child: Stored<Record>,
        }
        let input = r#"{"child":{"count":"bad","count":3,"future":null},"unknown":[-0.0]}"#;
        let record: Stored<Parent> = serde_json::from_str(input).unwrap();
        let value: Value = serde_json::from_str(input).unwrap();
        assert_eq!(record, Stored::from_value(value.clone()).unwrap());
        assert_eq!(record.data().child.data().count, 3);
        assert!(!record.wire.is_materialized());
        assert!(!record.data().child.wire.is_materialized());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&value).unwrap()
        );
        for invalid in [
            r#"{"child":{"count":3,"count":"bad"}}"#,
            r#"{"child":{"count":null}}"#,
            r#"{"child":{}}"#,
        ] {
            assert!(serde_json::from_str::<Stored<Parent>>(invalid).is_err());
        }
    }

    #[test]
    fn nested_stored_records_keep_their_wire_data() {
        #[derive(Debug, Deserialize)]
        struct Parent {
            child: Stored<Record>,
        }

        let wire = json!({"child": {"count": 3, "unknown": 7}, "outer": true});
        let record = Stored::<Parent>::from_value(wire.clone()).unwrap();
        assert_eq!(record.data().child.data().count, 3);
        assert_eq!(record.data().child.wire_value()["unknown"], 7);
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
}
