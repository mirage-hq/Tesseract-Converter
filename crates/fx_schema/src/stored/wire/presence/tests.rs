use super::*;
use crate::stored::wire::Wire;
use serde::ser::{SerializeMap, SerializeSeq};
use serde_json::json;

fn reference(raw: &Value, known: &Value) -> bool {
    match (raw, known) {
        (Value::Object(raw), Value::Object(known)) => raw
            .iter()
            .any(|(key, value)| known.get(key).is_none_or(|known| reference(value, known))),
        (Value::Array(raw), Value::Array(known)) => raw
            .iter()
            .zip(known)
            .any(|(raw, known)| reference(raw, known)),
        _ => false,
    }
}

fn compare(raw: &Value, known: &impl Serialize) {
    let wire = Wire::new(raw.clone());
    let expected = reference(raw, &serde_json::to_value(known).unwrap());
    assert_eq!(wire.has_unknown_fields(known, &[]), expected, "raw={raw}");
    assert!(!wire.is_materialized());
}

#[test]
fn all_json_kinds_match_object_membership_and_array_zip() {
    let values = [
        Value::Null,
        json!(false),
        json!(42),
        json!("text"),
        json!([]),
        json!({}),
        json!({"a": null}),
        json!({"a": {}, "b": 1}),
        json!({"a": {"future": [1,2]}}),
        json!([{}, {"extra": true}]),
        json!([{"future": null}]),
        json!([{"a": {"future": true}}, null]),
        json!({"a": [{"x": 1}, {"y": 2}]}),
    ];
    for raw in &values {
        for known in &values {
            compare(raw, known);
        }
    }
}

#[derive(Serialize)]
enum Tagged {
    Unit,
    Newtype(Value),
    Tuple(Value, u32),
    Record { value: Value },
}

#[test]
fn enum_envelopes_match_value_serialization() {
    for known in [
        Tagged::Unit,
        Tagged::Newtype(json!({"a": 1})),
        Tagged::Tuple(json!({"a": 1}), 2),
        Tagged::Record {
            value: json!({"a": 1}),
        },
    ] {
        for raw in [
            json!(null),
            json!({}),
            json!({"future": 1}),
            json!({"Newtype": {"a": 1, "future": 2}}),
            json!({"Tuple": [{"a": 1, "future": true}, 0]}),
            json!({"Record": {"value": {"a": 1}}}),
            json!({"Record": {"value": {"a": 1, "future": true}}, "extra": null}),
        ] {
            compare(&raw, &known);
        }
        compare(&serde_json::to_value(&known).unwrap(), &known);
    }
}

#[test]
fn duplicate_known_keys_keep_the_last_value() {
    struct Duplicates;
    impl Serialize for Duplicates {
        fn serialize<S: ser::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("a", &json!({}))?;
            map.serialize_entry("a", &json!({"x": 1}))?;
            map.end()
        }
    }
    compare(&json!({"a": {"x": 2}}), &Duplicates);
    compare(&json!({"a": {"x": 2}, "b": null}), &Duplicates);
    compare(&json!({"a": {"x": 2, "y": 3}}), &Duplicates);
}

#[test]
fn raw_duplicate_keys_match_materialized_last_value() {
    for count in [1, 20] {
        let prefix = (0..count)
            .map(|i| format!("\"field{i}\":null"))
            .collect::<Vec<_>>()
            .join(",");
        for text in [
            format!("{{{prefix},\"name\":{{\"future\":1}},\"name\":{{}}}}"),
            format!("{{{prefix},\"name\":{{}},\"name\":{{\"future\":1}}}}"),
        ] {
            let wire: Wire = serde_json::from_str(&text).unwrap();
            let raw: Value = serde_json::from_str(&text).unwrap();
            let mut known = raw.clone();
            known["name"] = json!({});
            assert_eq!(
                wire.has_unknown_fields(&known, &[]),
                reference(&raw, &known)
            );
            assert!(!wire.is_materialized());
        }
    }
}

#[test]
fn large_maps_match_independently_of_wire_order_without_materialization() {
    let entries: Vec<_> = (0..64)
        .rev()
        .map(|index| format!("\"field{index}\":{{\"x\":1}}"))
        .collect();
    let text = format!("{{{}}}", entries.join(","));
    let wire: Wire = serde_json::from_str(&text).unwrap();
    let mut known: Value = serde_json::from_str(&text).unwrap();
    assert!(!wire.has_unknown_fields(&known, &[]));
    known.as_object_mut().unwrap().remove("field32");
    assert!(wire.has_unknown_fields(&known, &[]));
    known["field32"] = json!({});
    assert!(wire.has_unknown_fields(&known, &[]));
    known["field32"] = json!({"x": 2});
    assert!(!wire.has_unknown_fields(&known, &[]));
    assert!(!wire.is_materialized());
}

#[test]
fn numeric_map_keys_match_json() {
    compare(
        &json!({"1": {}, "2": null}),
        &std::collections::BTreeMap::from([(1, json!({})), (2, Value::Null)]),
    );
    compare(
        &json!({"1": {"future": true}}),
        &std::collections::BTreeMap::from([(1, json!({}))]),
    );
}

#[test]
fn nested_record_skips_only_apply_at_the_requested_depth() {
    let raw =
        json!({"entries": [{"animator": {"future": 1}, "nested": {"animator": {"future": 1}}}]});
    let known = json!({"entries": [{"animator": {}, "nested": {"animator": {"future": 1}}}]});
    let wire = Wire::new(raw);
    assert!(wire.has_unknown_fields(&known, &[]));
    assert!(!wire.has_unknown_fields(&known, &[(2, "animator")]));
    assert!(wire.has_unknown_fields(
        &json!({"entries": [{"animator": {}, "nested": {"animator": {}}}]}),
        &[(2, "animator")]
    ));
    assert!(!wire.is_materialized());
}

#[test]
fn skipped_records_and_unmatched_array_tail_are_not_serialized() {
    struct MustNotSerialize;
    impl Serialize for MustNotSerialize {
        fn serialize<S: ser::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
            panic!("must not visit skipped records or unmatched array entries")
        }
    }
    #[derive(Serialize)]
    struct Record {
        nested: MustNotSerialize,
    }
    let wire = Wire::new(json!({"nested": {"large": [1,2,3]}}));
    assert!(!wire.has_unknown_fields(
        &Record {
            nested: MustNotSerialize
        },
        &[(0, "nested")]
    ));
    assert!(!wire.is_materialized());
    let empty = Wire::new(json!([]));
    assert!(!empty.has_unknown_fields(&vec![MustNotSerialize], &[]));

    struct StopAfterUnknown;
    impl Serialize for StopAfterUnknown {
        fn serialize<S: ser::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let mut seq = serializer.serialize_seq(Some(2))?;
            seq.serialize_element(&json!({}))?;
            seq.serialize_element(&MustNotSerialize)?;
            seq.end()
        }
    }
    assert!(Wire::new(json!([{"future": true}, {}])).has_unknown_fields(&StopAfterUnknown, &[]));
}
