//! Shared Group/Adjustment field definitions; clock interpretation stays at the reader.

/// Declare the product compatibility reader's Group field projection.
///
/// The raw timing fields retain enough presence information for the product reader
/// to distinguish canonical windowed playback from historical Group clocks before
/// constructing the normalized [`LayerPlayback`] owned by that reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_wire_group_layer_schema {
    () => {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireGroupLayer {
            id: LayerId,
            name: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            is_hidden: bool,
            parent: Option<LayerId>,
            #[serde(default)]
            blend_mode: BlendMode,
            #[serde(alias = "layerMask")]
            track_matte: Option<TrackMatte>,
            #[serde(default)]
            masks: Vec<PathMask>,
            #[serde(
                default,
                deserialize_with = "super::wire::deserialize_legacy_range_presence"
            )]
            active_range: Option<TimeRangeProperty>,
            #[serde(default, deserialize_with = "super::wire::deserialize_field_presence")]
            source_range_present: bool,
            #[serde(
                default,
                deserialize_with = "super::wire::deserialize_playback_presence"
            )]
            playback: Option<serde_json::Value>,
            #[serde(default, deserialize_with = "crate::effect::deserialize_effects")]
            effects: Vec<EffectInstance>,
            #[serde(default)]
            motion_blur: bool,
            #[serde(default)]
            padding_top: NonNegativeProperty,
            #[serde(default)]
            padding_right: NonNegativeProperty,
            #[serde(default)]
            padding_bottom: NonNegativeProperty,
            #[serde(default)]
            padding_left: NonNegativeProperty,
            #[serde(default)]
            fills: Vec<ShapeFillStyle>,
            #[serde(default)]
            corner_radius_top_left: NonNegativeProperty,
            #[serde(default)]
            corner_radius_top_right: NonNegativeProperty,
            #[serde(default)]
            corner_radius_bottom_right: NonNegativeProperty,
            #[serde(default)]
            corner_radius_bottom_left: NonNegativeProperty,
            transform: Transform,
            layers: Vec<Layer>,
        }
    };
}

/// Retired playback fields shared by both reader contracts.
#[doc(hidden)]
#[macro_export]
macro_rules! define_group_playback_schema {
    ($($attrs:meta),*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        $(#[$attrs])*
        /// Retired positive-rate Group playback payload retained only in generated
        /// read/action schemas. Runtime deserialization accepts and drops this shape.
        #[allow(dead_code)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct Playback {
            /// Retired positive local-seconds-per-parent-second rate.
            pub rate: f64,
        }
    };
}

/// Define group and adjustment records with the caller's layer/effect/playback readers.
#[doc(hidden)]
#[macro_export]
macro_rules! define_group_layer_schema {
    (reader: [$($reader:tt)*]) => {
        /// Group layer data with a nested layer stack.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $($reader)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct GroupLayer {
            /// Stable composition-unique layer id — the id space referenced by
            /// `parent`, track mattes, path masks, and text `pathOptions`.
            pub id: LayerId,
            /// User-visible name for the layer. This is distinct from `description`.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this layer and its descendants are excluded from all output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing Group layer, or `null` for a root
            /// layer — mirrors the layer's position in the tree.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// How this layer composites over the stack below (see [`BlendMode`]).
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Optional inline track-matte source applied before blending.
            #[serde(alias = "layerMask", skip_serializing_if = "Option::is_none")]
            pub track_matte: Option<TrackMatte>,
            /// Stacked vector-path masks drawn directly on this layer, each carrying a
            /// stable [`FxItemId`] (JRB-1200). Distinct from the [`Self::track_matte`]
            /// track matte: these carry their own path geometry rather than
            /// referencing another layer's rendered alpha.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub masks: Vec<PathMask>,
            /// Parent-clock window and editable content mapping.
            pub playback: LayerPlayback,
            /// AE-style Effects stack applied to the group's composited output
            /// (children rendered and composed first, then effects applied to the
            /// result). This is Tesseract's AdjustmentLayer replacement — effects on a
            /// Group apply with explicit parent-child scope, not to siblings below
            /// in stack order.
            ///
            /// Effect compatibility on Group:
            /// - `LayerEffect::PersonMatte` is **not** valid on a Group: it requires
            ///   a single source video frame to segment, and a Group renders a
            ///   composited stack of arbitrary content. Listed `PersonMatte`
            ///   effects are skipped at `evaluate_node` time with a warn-once log.
            /// - Non-matte variants (blur, glow, mosaic, color grading, channel
            ///   shifts) apply to the group's composited output through the common
            ///   ordered layer stack.
            #[serde(
                default,
                skip_serializing_if = "Vec::is_empty",
                deserialize_with = "crate::effect::deserialize_effects"
            )]
            pub effects: Vec<EffectInstance>,
            /// Whether this layer participates in the composition's standard motion blur.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub motion_blur: bool,
            /// Space above the child-derived content bounds, in layer-local pixels.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub padding_top: NonNegativeProperty,
            /// Space to the right of the child-derived content bounds.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub padding_right: NonNegativeProperty,
            /// Space below the child-derived content bounds.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub padding_bottom: NonNegativeProperty,
            /// Space to the left of the child-derived content bounds.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub padding_left: NonNegativeProperty,
            /// Ordered paints drawn behind the padded child bounds.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub fills: Vec<ShapeFillStyle>,
            /// Top-left background corner radius. Children are not clipped.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub corner_radius_top_left: NonNegativeProperty,
            /// Top-right background corner radius. Children are not clipped.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub corner_radius_top_right: NonNegativeProperty,
            /// Bottom-right background corner radius. Children are not clipped.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub corner_radius_bottom_right: NonNegativeProperty,
            /// Bottom-left background corner radius. Children are not clipped.
            #[serde(default, skip_serializing_if = "is_zero_non_negative")]
            pub corner_radius_bottom_left: NonNegativeProperty,
            /// The layer's transform (anchor point, position, scale, rotation,
            /// opacity).
            pub transform: Transform,
            /// Nested layer stack. The first item renders above later items.
            pub layers: Vec<Layer>,
        }

        /// AE-style adjustment layer (ENG-1505): a contentless layer whose
        /// [`LayerEffect`] stack applies to the composite of every sibling layer
        /// **below** it in the stack (the AE "affects all layers below" rule),
        /// strictly ported from After Effects:
        ///
        /// * **Scope** — the sibling suffix below this layer within the same
        ///   composition / group; layers above are untouched. A bottom-most
        ///   adjustment (nothing beneath) is a no-op, as is one with no effects.
        /// * **Masks gate the effect spatially** — inside mask coverage the
        ///   effected composite shows, outside the untouched composite shows
        ///   (AE order: mask clips the backdrop copy first, then effects run —
        ///   this reproduces AE's characteristic masked-blur edge).
        /// * **Opacity is a wet/dry mix** — 100 = fully effected, 0 = untouched.
        /// * **Blend mode** blends the effected copy against the untouched
        ///   composite.
        /// * **Transform** — AE ignores an adjustment layer's geometric transform
        ///   for the layers beneath (it only moves the layer's own solid/mask).
        ///   Only `transform.opacity` participates here; mask geometry comes from
        ///   the referenced shape layers' own comp-space positions.
        ///
        /// Non-goals (deliberate, documented AE parity limits): Generate /
        /// Simulation / Time-category behavior (AE users are pointed at solids /
        /// precomps for those), 3D depth-sort interleaving (an adjustment layer is
        /// always a 2D bin breaker here), and layer styles (silhouette-based, so
        /// meaningless on a contentless layer — the field set omits them).
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct AdjustmentLayer {
            /// Stable composition-unique layer id — the id space referenced by
            /// `parent`, track mattes, path masks, and text `pathOptions`.
            pub id: LayerId,
            /// User-visible name for the layer. This is distinct from `description`.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this adjustment is excluded from all output processing.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing Group layer, or `null` for a root
            /// layer — mirrors the layer's position in the tree.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// Blends the effected composite against the untouched composite (see
            /// [`BlendMode`]).
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Optional track matte gating where the effected composite applies.
            #[serde(alias = "layerMask", skip_serializing_if = "Option::is_none")]
            pub track_matte: Option<TrackMatte>,
            /// Stacked vector-path masks gating where the effected composite
            /// applies (same [`PathMask`] shape-reference model as other layers).
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub masks: Vec<PathMask>,
            /// Immediate-parent-local span during which this layer is active.
            pub active_range: TimeRangeProperty,
            /// AE-style Effects stack applied to the composite of the sibling
            /// layers below. [`LayerEffect::PersonMatte`] is not valid here (it
            /// needs a single source video frame) and is skipped with a warn-once,
            /// same as on a Group.
            #[serde(
                default,
                skip_serializing_if = "Vec::is_empty",
                deserialize_with = "crate::effect::deserialize_effects"
            )]
            pub effects: Vec<EffectInstance>,
            /// Only `opacity` (the wet/dry mix) is honored; see the type docs for
            /// why the geometric components are ignored (AE parity).
            pub transform: Transform,
        }
    };
}
