//! Canonical boolean layer fields, shared by stored and normalized readers.

/// Declare the layer using the caller's existing nested-record readers.
#[doc(hidden)]
#[macro_export]
macro_rules! define_boolean_layer_schema {
    () => {
        /// Figma-style non-destructive boolean group (JRB-1214).
        ///
        /// Combines the **vector geometry** of its child layers with a [`BooleanOp`]
        /// *before* any paint is applied, then fills/strokes the single combined
        /// path with this layer's own [`Self::fills`] / [`Self::strokes`] — exactly
        /// once. This is fundamentally different from [`GroupLayer`], which
        /// rasterizes each child (with the child's own paint) and blends the pixels:
        /// a boolean group ignores the children's fills, strokes, effects, and
        /// masks entirely — only their outlines and transforms matter.
        ///
        /// Like Figma's `BOOLEAN_OPERATION` node the children stay in the tree as
        /// ordinary layers (non-destructive editing) and boolean groups may nest:
        /// a child that is itself a `BooleanOperationLayer` contributes its combined
        /// path. Supported geometry children are `Shape`, `Rect`, and nested
        /// `BooleanOperation` layers; anything else (media, text, audio, plain
        /// groups) has no well-defined outline and is skipped with a warn-once log.
        /// A child whose `active_range` excludes the evaluation time is likewise
        /// excluded from the combine.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct BooleanOperationLayer {
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
            /// Stacked vector-path masks drawn directly on this layer, each carrying
            /// a stable [`FxItemId`] (JRB-1200).
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub masks: Vec<PathMask>,
            /// Immediate-parent-local span during which this layer is active. Root
            /// layers use composition time.
            pub active_range: TimeRangeProperty,
            /// AE-style Effects stack applied to the rasterized combined shape (same
            /// contract as [`ShapeLayer::effects`]).
            #[serde(
                default,
                skip_serializing_if = "Vec::is_empty",
                deserialize_with = "crate::effect::deserialize_effects"
            )]
            pub effects: Vec<EffectInstance>,
            /// Whether this layer participates in the composition's standard motion blur.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub motion_blur: bool,
            /// The layer's transform (anchor point, position, scale, rotation,
            /// opacity).
            pub transform: Transform,
            /// Boolean formula combining the children's geometry.
            #[serde(default)]
            pub op: BooleanOp,
            /// Child layers contributing geometry. The first item renders above
            /// later items (same paint order as [`GroupLayer::layers`]); for
            /// [`BooleanOp::Subtract`] the **last** item is the subject being cut.
            pub layers: Vec<Layer>,
            /// Fill styles painted once on the combined path.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub fills: Vec<ShapeFillStyle>,
            /// Stroke styles painted once on the combined path.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub strokes: Vec<ShapeStrokeStyle>,
            /// Optional TrimPaths modifier applied to the *combined* path before it
            /// is filled or stroked, matching [`ShapeContent::trim`]'s model
            /// (JRB-1549). `None` == the whole combined outline is drawn (no
            /// modifier).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub trim: Option<ShapeTrimPaths>,
        }
    };
}
