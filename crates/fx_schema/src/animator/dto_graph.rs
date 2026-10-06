//! Lossless graph records and structural dependency validation.

use super::dto::PropertyAnimator;
use super::AnimationGraphError;
use crate::{stored::Stored, LayerRefMap, Property, PropertyTarget};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct AnimationGraph(Stored<GraphData>);

macro_rules! stored_animation_graph {
    ($(#[$docs:meta])* pub struct $name:ident {
        $(#[doc = $field_doc:expr])* pub $field:ident: $type:ty,
    } reader_attributes { #[$entries_attr:meta] }) => {
        #[derive(Debug, Clone, PartialEq, Serialize)]
        pub struct GraphData {
            pub $field: $type,
        }
    };
}
crate::define_animation_graph_data_schema!(stored_animation_graph, allow(dead_code));

macro_rules! stored_graph_entry {
    (name: $name:ident, visibility: [$visibility:vis];
     $(#[$attrs:meta])* pub struct AnimationGraphEntry {
        $( $(#[$field_attr:meta])* $field:ident: $type:ty, )*
    }) => {
        $(#[$attrs])*
        $visibility struct $name {
            $( $(#[$field_attr])* pub $field: $type, )*
        }
    };
}
crate::define_animation_graph_entry_fields_schema!(
    stored_graph_entry,
    AnimationGraphEntry,
    Serialize,
    serde(rename_all = "camelCase"),
    allow(dead_code),
    serde(skip_serializing_if = "Option::is_none"),
    pub
);

impl Default for AnimationGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationGraph {
    pub fn new() -> Self {
        Self::from_data(&GraphData {
            entries: Vec::new(),
        })
        .expect("empty graph is structurally valid")
    }

    pub fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    pub fn from_entries(entries: Vec<AnimationGraphEntry>) -> Result<Self, AnimationGraphError> {
        super::dto_validation::validate_entries(&entries)?;
        validate_dependencies(&entries)?;
        Self::from_data(&GraphData { entries })
            .map_err(|error| AnimationGraphError::Wire(error.to_string()))
    }

    pub fn property_animator(&self, property: Property) -> Option<&PropertyAnimator> {
        let target = PropertyTarget::from(property);
        self.entries()
            .iter()
            .find(|entry| entry.target == target)
            .map(|entry| &entry.animator)
    }

    pub fn set_property(
        &mut self,
        target: impl Into<PropertyTarget>,
        animator: impl Into<PropertyAnimator>,
        dependencies: Vec<Property>,
    ) -> Result<(), AnimationGraphError> {
        let target = target.into();
        let animator = animator.into();
        let entry = AnimationGraphEntry {
            target: target.clone(),
            animator,
            dependencies: dependencies.into_iter().map(PropertyTarget::from).collect(),
            random_seed_target: None,
            layer_refs: LayerRefMap::default(),
        };
        let mut raw = self.wire_value().clone();
        if raw.get("entries").is_none() {
            raw["entries"] = Value::Array(Vec::new());
        }
        let value = serde_json::to_value(&entry)
            .map_err(|error| AnimationGraphError::Wire(error.to_string()))?;
        let entries = raw["entries"]
            .as_array_mut()
            .expect("checked graph entries are an array");
        if let Some(index) = self
            .entries()
            .iter()
            .position(|entry| entry.target == target)
        {
            entries[index] = value;
        } else {
            entries.push(value);
        }
        // Validate typed entries first to retain structured construction errors.
        let candidate_entries: Vec<AnimationGraphEntry> =
            serde_json::from_value(raw["entries"].clone())
                .map_err(|error| AnimationGraphError::Wire(error.to_string()))?;
        super::dto_validation::validate_entries(&candidate_entries)?;
        validate_dependencies(&candidate_entries)?;
        let candidate = serde_json::from_value(raw)
            .map_err(|error| AnimationGraphError::Wire(error.to_string()))?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn has_unknown_fields(&self) -> bool {
        // Match known_value's replacement of entries[*].animator, retaining
        // checks for unknown graph/entry fields and omitted optional fields.
        self.0.has_unknown_fields(&[(2, "animator")])
            || self
                .entries()
                .iter()
                .any(|entry| entry.animator.has_unknown_fields())
    }

    pub fn known_value(&self) -> Value {
        let mut value = serde_json::to_value(self.0.data()).expect("checked graph data serializes");
        for (index, entry) in self.entries().iter().enumerate() {
            value["entries"][index]["animator"] = entry.animator.known_value();
        }
        value
    }

    pub fn from_data(data: &GraphData) -> Result<Self, serde_json::Error> {
        Stored::from_data(data).map(Self)
    }

    pub fn entries(&self) -> &[AnimationGraphEntry] {
        &self.0.data().entries
    }

    pub fn wire_value(&self) -> &Value {
        self.0.wire_value()
    }
}

impl<'de> Deserialize<'de> for AnimationGraphEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        macro_rules! stored_graph_entry_input {
            ($(#[$attrs:meta])* struct $name:ident {
                $( $(#[$field_attr:meta])* $field:ident: $type:ty, )*
            }) => {
                $(#[$attrs])*
                struct $name {
                    $( $(#[$field_attr])* $field: $type, )*
                }
            };
        }
        crate::define_animation_graph_entry_input_schema!(stored_graph_entry_input, Dependency);

        enum Dependency {
            Target(PropertyTarget),
            Property(Property),
        }
        impl<'de> Deserialize<'de> for Dependency {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = Value::deserialize(deserializer)?;
                if value.get("kind").is_some() {
                    serde_json::from_value(value)
                        .map(Self::Target)
                        .map_err(serde::de::Error::custom)
                } else {
                    serde_json::from_value(value)
                        .map(Self::Property)
                        .map_err(serde::de::Error::custom)
                }
            }
        }

        let wire = Wire::deserialize(deserializer)?;
        let target = wire
            .target
            .or_else(|| wire.property.map(PropertyTarget::from))
            .ok_or_else(|| serde::de::Error::missing_field("target"))?;
        Ok(Self {
            target,
            animator: wire.animator,
            dependencies: wire
                .dependencies
                .into_iter()
                .map(|dependency| match dependency {
                    Dependency::Target(target) => target,
                    Dependency::Property(property) => property.into(),
                })
                .collect(),
            random_seed_target: wire.random_seed_target,
            layer_refs: wire.layer_refs,
        })
    }
}

impl<'de> Deserialize<'de> for GraphData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(default)]
            entries: Vec<AnimationGraphEntry>,
        }
        let wire = Wire::deserialize(deserializer)?;
        super::dto_validation::validate_entries(&wire.entries).map_err(serde::de::Error::custom)?;
        validate_dependencies(&wire.entries).map_err(serde::de::Error::custom)?;
        Ok(Self {
            entries: wire.entries,
        })
    }
}

/// Checks references and acyclicity without deriving or caching an execution order.
fn validate_dependencies(entries: &[AnimationGraphEntry]) -> Result<(), AnimationGraphError> {
    let mut indices = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if indices.insert(&entry.target, index).is_some() {
            return Err(AnimationGraphError::DuplicateProperty(entry.target.clone()));
        }
    }
    let mut indegrees = vec![0_usize; entries.len()];
    let mut dependents = vec![Vec::new(); entries.len()];
    for (index, entry) in entries.iter().enumerate() {
        for dependency in &entry.dependencies {
            if dependency
                .as_property()
                .is_some_and(|property| property.property_type().is_read_only())
            {
                continue;
            }
            let source = indices
                .get(dependency)
                .ok_or_else(|| AnimationGraphError::UnknownProperty(dependency.clone()))?;
            indegrees[index] += 1;
            dependents[*source].push(index);
        }
    }
    let mut ready = indegrees
        .iter()
        .enumerate()
        .filter_map(|(index, &degree)| (degree == 0).then_some(index))
        .collect::<Vec<_>>();
    while let Some(index) = ready.pop() {
        for &dependent in &dependents[index] {
            indegrees[dependent] -= 1;
            if indegrees[dependent] == 0 {
                ready.push(dependent);
            }
        }
    }
    if let Some(index) = indegrees.iter().position(|&degree| degree != 0) {
        return Err(AnimationGraphError::Cycle {
            property: entries[index].target.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayerId, PropType};
    use serde_json::json;

    fn property(id: u64) -> Property {
        Property::new(LayerId::new(id), PropType::Rotation)
    }

    fn entry(id: u64, dependencies: &[u64]) -> Value {
        json!({
            "property": property(id),
            "animator": {"type": "jsScript", "code": "legacy source text"},
            "dependencies": dependencies.iter().map(|&id| property(id)).collect::<Vec<_>>()
        })
    }

    #[test]
    fn legacy_dependency_order_duplicates_and_script_text_are_unchanged() {
        let wire = json!({"entries": [entry(3, &[2, 1, 2]), entry(1, &[]), entry(2, &[])]});
        let graph: AnimationGraph = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            graph.entries()[0].dependencies,
            vec![property(2).into(), property(1).into(), property(2).into()]
        );
        assert_eq!(
            graph.entries()[0].animator.wire_value()["code"],
            "legacy source text"
        );
        assert_eq!(serde_json::to_value(graph).unwrap(), wire);
    }

    #[test]
    fn unknown_fields_at_graph_entry_and_animator_levels_survive() {
        let mut node = entry(1, &[]);
        node["futureEntry"] = json!([null, 2]);
        node["animator"]["futureAnimator"] = json!(true);
        let wire = json!({"entries": [node], "futureGraph": {"version": 99}});
        let graph: AnimationGraph = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(graph.wire_value(), &wire);
        assert_eq!(serde_json::to_value(graph).unwrap(), wire);
    }

    #[test]
    fn empty_input_does_not_gain_default_fields() {
        let graph: AnimationGraph = serde_json::from_value(json!({})).unwrap();
        assert!(graph.entries().is_empty());
        assert_eq!(serde_json::to_value(graph).unwrap(), json!({}));
    }

    #[test]
    fn duplicate_targets_missing_dependencies_and_cycles_are_rejected() {
        for entries in [
            vec![entry(1, &[]), entry(1, &[])],
            vec![entry(1, &[2])],
            vec![entry(1, &[2]), entry(2, &[1])],
            vec![entry(1, &[1])],
        ] {
            assert!(serde_json::from_value::<AnimationGraph>(json!({"entries": entries})).is_err());
        }
    }

    #[test]
    fn asset_targets_require_enumerable_values_of_the_right_type() {
        let mut node = entry(1, &[]);
        node["property"] =
            serde_json::to_value(Property::new(LayerId::new(1), PropType::MediaSourceAssetId))
                .unwrap();
        assert!(
            serde_json::from_value::<AnimationGraph>(json!({"entries": [node.clone()]})).is_err()
        );
        node["animator"] = json!({"type": "constant", "value": crate::PropertyValue::Float(1.0)});
        assert!(serde_json::from_value::<AnimationGraph>(json!({"entries": [node]})).is_err());
    }

    #[test]
    fn layer_references_require_script_records() {
        let mut node = entry(1, &[]);
        node["layerRefs"] = json!({"source": {"layerId": 2}});
        assert!(
            serde_json::from_value::<AnimationGraph>(json!({"entries": [node.clone()]})).is_ok()
        );
        node["animator"] = json!({"type": "constant", "value": crate::PropertyValue::Float(1.0)});
        assert!(serde_json::from_value::<AnimationGraph>(json!({"entries": [node]})).is_err());
    }

    #[test]
    fn keyframe_ids_must_be_unique_across_entries() {
        let key = super::super::PropertyKeyframe::new(
            crate::KeyframeId::new("shared"),
            crate::TimeOffset::from_millis(0),
            crate::PropertyValue::Float(1.0),
            crate::PropertyKeyframeEasing::Linear,
        );
        let mut first = entry(1, &[]);
        first["animator"] = json!({"type": "keyframes", "enabled": true, "keyframes": [key]});
        assert!(
            serde_json::from_value::<AnimationGraph>(json!({"entries": [first.clone()]})).is_ok()
        );
        let mut second = first.clone();
        second["property"] = serde_json::to_value(property(2)).unwrap();
        assert!(
            serde_json::from_value::<AnimationGraph>(json!({"entries": [first, second]})).is_err()
        );
    }

    #[test]
    fn authored_graph_uses_the_same_structural_checks() {
        let graph = AnimationGraph::from_data(&GraphData {
            entries: Vec::new(),
        })
        .unwrap();
        assert!(graph.entries().is_empty());
        let node: AnimationGraphEntry = serde_json::from_value(entry(1, &[1])).unwrap();
        assert!(AnimationGraph::from_data(&GraphData {
            entries: vec![node]
        })
        .is_err());
    }
}
