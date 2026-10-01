//! One text-layer declaration shared by stored and normalized document projections.

/// Define the text-layer schema using the caller's existing effect representation.
#[doc(hidden)]
#[macro_export]
// Retain the caller-qualified source spelling used by schema metadata generation.
#[allow(clippy::crate_in_macro_def)]
macro_rules! define_text_layer_schema {
    () => {
        /// Text layer data.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct TextLayer {
            /// Stable composition-unique layer id — the id space referenced by
            /// `parent`, track mattes, path masks, and text `pathOptions`.
            pub id: LayerId,
            /// User-visible name for the layer. This is distinct from `description`.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this layer is excluded from all rendered output.
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
            /// Immediate-parent-local span during which this layer is active. Root
            /// layers use composition time.
            pub active_range: TimeRangeProperty,
            /// AE-style Effects stack applied to this layer's rasterized content
            /// (the renderer wraps each lowered [`scene::NodeEffect`] in an
            /// `EffectGroup` around the text raster, in authored stack order).
            /// [`LayerEffect::PersonMatte`] lowers to nothing here — it needs a
            /// source video frame to segment — so it is skipped on a text layer.
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
            /// Source text document rendered by this layer.
            pub source_text: TextDocument,
            /// After Effects *Text > Animators* stack (range selectors +
            /// per-character properties), applied in order to produce the per-character
            /// `char_animations` the renderer draws. Empty ⇒ plain text (JRB-1217).
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub animators: Vec<crate::text_animator::TextAnimator>,
            /// AE *Text > Path Options* (JRB-1219 / ENG-1471): when set, lay this
            /// layer's glyphs along the referenced shape-producing layer's outline
            /// instead of in a straight line.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub path_options: Option<TextPathOptions>,
            /// Anchor options (JRB-1791 item 4): which unit the animators' per-character
            /// transforms pivot about, and where inside that unit.
            ///
            /// Named for what it holds rather than for where AE keeps it: this is the
            /// **anchor** subset of AE's *Text > More Options* panel, and it deliberately
            /// omits that panel's unrelated members (`fillAndStroke`,
            /// `interCharacterBlending`), which no path in this repo consumes.
            ///
            /// Deliberately `Option`, and deliberately absent from every existing
            /// document: AE's grouping semantics put a Character pivot at the glyph's
            /// advance *center*, whereas every renderer in this repo has always pivoted
            /// at the glyph's pen *origin*. Adopting AE unconditionally would move
            /// rendered pixels on the whole corpus (a 90° rotation at font size 100
            /// displaces a glyph by ~52 px). `None` therefore means "keep the legacy
            /// pen-origin pivot, byte for byte"; `Some` opts a document into AE.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub anchor_options: Option<TextAnchorOptions>,
        }
    };
}
