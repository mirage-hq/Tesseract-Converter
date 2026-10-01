//! One canonical graph-entry field inventory for stored, runtime, and TypeScript records.

/// Supply graph fields to each reader's record builder. Reader-specific attributes
/// control the wire spelling and TS representation without duplicating fields.
#[doc(hidden)]
#[macro_export]
macro_rules! define_animation_graph_entry_fields_schema {
    ($emit:ident, $name:ident, $derive:ident, $record_attr:meta,
     $dependencies_attr:meta, $random_seed_attr:meta, $visibility:vis) => {
        $emit! {
            name: $name, visibility: [$visibility];
            /// One serializable entry in an [`AnimationGraph`].
            ///
            /// Deserialization is manual: documents predating [`PropertyTarget`]
            /// addressed the animated property under `property` rather than `target`.
            #[derive(Debug, Clone, PartialEq, $derive)]
            #[$record_attr]
            pub struct AnimationGraphEntry {
                /// Target produced by this graph entry — a fixed layer property or a
                /// name-keyed effect param ([`PropertyTarget`]).
                target: PropertyTarget,
                /// Animator used to produce the target's value.
                animator: PropertyAnimator,
                /// Property targets that must evaluate before this entry, exposed to the
                /// animator as `input.deps`. Read-only layer properties are ambient
                /// sources; every other dependency must have a producing graph entry.
                /// Order is semantic and retained after canonical target migration.
                #[$dependencies_attr]
                dependencies: Vec<PropertyTarget>,
                /// Original address used to derive `input.randomSeed` when this entry's
                /// canonical target was migrated to another namespace.
                #[$random_seed_attr]
                random_seed_target: Option<PropertyTarget>,
                /// Named, remappable layer references exposed as `input.refs.<alias>`.
                /// Read metadata with the free function
                /// `getMetadata(input.refs.<alias>, "<name>")`; a ref has no
                /// `getMetadata` method. Read source-local time from
                /// `input.refs.<alias>.sourceTime.milliseconds`.
                layer_refs: LayerRefMap,
            }
        }
    };
}

/// Supply the borrowed graph-entry output fields to the runtime writer.
/// The writer chooses when to omit defaults; the schema owns field names/types.
#[doc(hidden)]
#[macro_export]
macro_rules! define_animation_graph_entry_output_schema {
    ($emit:ident) => {
        $emit! {
            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            struct Wire<'a> {
                target: &'a PropertyTarget,
                animator: &'a PropertyAnimator,
                #[serde(skip_serializing_if = "Vec::is_empty")]
                dependencies: &'a Vec<PropertyTarget>,
                #[serde(skip_serializing_if = "Option::is_none")]
                random_seed_target: &'a Option<PropertyTarget>,
                #[serde(skip_serializing_if = "BTreeMap::is_empty")]
                layer_refs: &'a LayerRefMap,
            }
        }
    };
}

/// Supply the shared graph-entry input fields to either migration reader.
/// The dependency address discriminant remains reader-owned: product code
/// normalizes retired script indexes while the stored reader retains JSON.
#[doc(hidden)]
#[macro_export]
macro_rules! define_animation_graph_entry_input_schema {
    ($emit:ident, $dependency:ty) => {
        $emit! {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Wire {
                #[serde(default)]
                target: Option<PropertyTarget>,
                /// Legacy fixed-layer address accepted only on read.
                #[serde(default)]
                property: Option<Property>,
                animator: PropertyAnimator,
                #[serde(default)]
                dependencies: Vec<$dependency>,
                #[serde(default)]
                random_seed_target: Option<PropertyTarget>,
                #[serde(default)]
                layer_refs: LayerRefMap,
            }
        }
    };
}

/// Declare the compatibility metadata embedded in legacy layer-time code.
#[doc(hidden)]
#[macro_export]
macro_rules! define_layer_time_compat_metadata_schema {
    ($emit:ident) => {
        $emit! {
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct LayerTimeCompatMetadata {
                range_dependency_index: usize,
                authored_dependency_indices: Vec<usize>,
                authored_active_range_indices: Vec<usize>,
                random_seed_prefix: u64,
            }
        }
    };
}

/// Declare the strict runtime reader's canonical/legacy dependency alternatives.
#[doc(hidden)]
#[macro_export]
macro_rules! define_runtime_graph_dependency_wire_schema {
    ($emit:ident) => {
        $emit! {
            #[serde(untagged)]
            enum DependencyWire {
                Canonical(PropertyTarget),
                Legacy(Property),
            }
        }
    };
}

/// Declare the union of current and legacy dependency addresses for TS clients.
#[doc(hidden)]
#[macro_export]
macro_rules! define_animation_graph_dependency_schema {
    () => {
        /// Dependency inputs accepted at the project-action boundary.
        #[allow(dead_code)]
        #[derive(Deserialize, Serialize, TS)]
        #[serde(untagged)]
        #[ts(export_to = "project_types.d.ts")]
        pub(super) enum AnimationGraphDependency {
            Canonical(PropertyTarget),
            Legacy(Property),
        }
    };
}
