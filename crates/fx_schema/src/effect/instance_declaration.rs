//! Shared effect-instance fields and compatibility provenance.

/// Supply one field list to the stored and normalized record builders.
#[doc(hidden)]
#[macro_export]
macro_rules! define_effect_instance_schema {
    ($emit:ident) => {
        $emit! {
            /// One effect in a layer's effect stack: a stable [`EffectId`] plus the
            /// [`LayerEffect`] it applies.
            ///
            /// The id is either caller-assigned or engine-minted at add time and is the
            /// **durable identity** an animator addresses — so an
            /// effect-param animation survives stack inserts, removals, reorders, and even
            /// moving the effect to another layer, with no index fixup. Unique within the
            /// composition, so the owning layer is derivable from the id.
            ///
            /// Serializes nested as `{ "id": <n>, "enabled": <bool>, "effect": { "type": …, … } }`:
            /// `id` and `enabled` are instance metadata, while `effect` is the kind + its
            /// params. Missing `enabled` defaults to `true` for backward compatibility. (A future
            /// project-level shared-effect model would change only the `effect` payload —
            /// a reference + per-layer values — leaving the `id` spine intact.)
            pub struct EffectInstance {
                /// Stable caller-minted identity of this effect instance, unique within
                /// the composition — the id `effectProperty` animator targets address.
                pub id: EffectId,
                /// Whether this effect participates in rendering. Disabled effects retain
                /// their payload, stack position, id, and parameter animators.
                #[serde(default = "default_true")]
                pub enabled: bool,
                /// AE Compositing Options; omission preserves the historical effect path.
                #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_persisted_compositing_options")]
                pub compositing_options: Option<EffectCompositingOptions>,
                /// Forward-compatible instance fields retained on writeback.
                #[serde(default, flatten)]
                pub extensions: EffectInstanceExtensions,
                /// The effect this instance applies.
                #[serde(
                    deserialize_with = "deserialize_persisted_effect",
                    serialize_with = "serialize_persisted_effect"
                )]
                pub effect: LayerEffect,
            }
            compatibility {
                /// Durable input-boundary alias; never a second rendering payload.
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(skip)]
                pub(crate) legacy_source: Option<LegacyEffectSource>,
                /// Original legacy ordinary-effect wrapper shape, when applicable.
                #[serde(default, skip_serializing_if = "Option::is_none")]
                #[ts(skip)]
                pub(crate) legacy_ordinary_wire: Option<LegacyOrdinaryEffectWire>,
                /// Runtime-only provenance for an id minted while reading a standalone
                /// layer boundary. Destination insertion may remint only these ids.
                #[serde(skip)]
                #[ts(skip)]
                pub(crate) boundary_generated_id: bool,
            }
        }
    };
}

/// Historical aliases are data; their migration remains reader-owned.
#[doc(hidden)]
#[macro_export]
macro_rules! define_effect_provenance_schema {
    () => {
        /// Address retained only for adapting operations authored against the old
        /// layer-style namespaces. The effect payload and ordering live exclusively in
        /// the owning `EffectInstance`.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "kind", rename_all = "camelCase")]
        pub(crate) enum LegacyEffectSource {
            LayerStyle {
                #[serde(rename = "itemId")]
                item_id: FxItemId,
                /// Unknown fields from the historical layer-style entry wrapper.
                #[serde(
                    default,
                    rename = "entryFields",
                    skip_serializing_if = "BTreeMap::is_empty"
                )]
                entry_fields: BTreeMap<String, serde_json::Value>,
            },
            InlineShadow {
                /// A colliding historical payload field moved out of the effect body.
                #[serde(
                    default,
                    rename = "entryFields",
                    skip_serializing_if = "BTreeMap::is_empty"
                )]
                entry_fields: BTreeMap<String, serde_json::Value>,
            },
        }

        /// Exact ordinary-effect representation read from a legacy composition.
        ///
        /// This is persistence provenance for standalone layer boundaries, not another
        /// effect category. Full composition serialization removes it after preserving
        /// any opaque metadata in the canonical effect wrapper.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub(crate) enum LegacyOrdinaryEffectWire {
            Bare,
            Instance,
        }
    };
}
