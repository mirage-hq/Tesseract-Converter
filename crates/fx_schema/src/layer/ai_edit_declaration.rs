//! Canonical ai_edit layer fields, shared by stored and normalized readers.

/// Declare the layer using the caller's existing nested-record readers.
#[doc(hidden)]
#[macro_export]
macro_rules! define_ai_edit_layer_schema {
    () => {
        /// A semantic AI Edit segment.
        ///
        /// The nested stack contains the source media and all style layers.
        /// `source_layer_id` points to the source [`super::VideoLayer`] or
        /// [`super::ImageLayer`] in that stack. PAG content uses normal [`super::PagLayer`]
        /// children so it can also live at the composition root and cross AI Edit
        /// segment boundaries.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct AiEditLayer {
            /// Stable composition-unique layer id.
            pub id: LayerId,
            /// User-visible name, such as `Hook` or `B-Roll`.
            pub name: String,
            /// `LayerId` of the containing semantic container, or `null` at the root.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// Immediate-parent-local span during which this AI Edit segment is active.
            pub active_range: TimeRangeProperty,
            /// Required VideoStyleTemplate id.
            pub style_id: String,
            /// ShotStyleTemplate id within `style_id`.
            ///
            /// Legacy split shots preserve their rendered style while dropping the
            /// original template lineage, so migrated layers may omit this value.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub shot_style_id: Option<String>,
            /// The nested FX Video or Image layer that supplies this segment's source media.
            pub source_layer_id: LayerId,
            /// Initial source offset for shot-style media overlays, in milliseconds.
            /// Classic overlays use the shot's unscaled video time, which cannot be
            /// recovered exactly from the rounded, speed-scaled FX source range.
            /// Omission lets older/native FX documents derive it from their source.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub media_overlay_source_start: Option<Time>,
            /// Base painted below background PAG content when the source is cut out.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub background: Option<AiEditBackground>,
            /// Legacy shot stickers rendered with the segment's caption/emoji settings.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub stickers: Vec<AiEditSticker>,
            /// Source media, audio, matte, and PAG style layers. The first item
            /// renders above later items.
            pub layers: Vec<Layer>,
        }
    };
}
