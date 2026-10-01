//! Canonical PAG layer fields; clock interpretation belongs to each reader.

/// Declare the PAG layer using the caller's playback reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_pag_layer_schema {
    () => {
        /// A standalone PAG sequence in the FX layer tree.
        ///
        /// The layer owns its full authored timeline range. It is a root for
        /// a background or transition that crosses semantic segment boundaries.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct PagLayer {
            /// Stable composition-unique layer id.
            pub id: LayerId,
            /// User-visible name for the layer.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this PAG layer is excluded from rendering.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing semantic folder, or `null` at the root.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// Authored span that owns PAG sequence timing and presentation progress.
            /// With no playback mapping this is also the immediate-parent-local slot.
            pub active_range: TimeRangeProperty,
            /// Optional 1x trim from immediate-parent time to original project time.
            /// With this field, activeRange also uses original project time.
            /// Its input interval owns visibility; activeRange stays unchanged
            /// so trimming does not restart the intro or move the outro.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub playback: Option<TimeRemapProperty>,
            /// Whether the PAG renders behind the source video instead of in front of it.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_background: bool,
            /// Normalized overlay opacity.
            #[serde(default = "default_pag_opacity")]
            pub opacity: f64,
            /// Legacy PAG placement mode.
            #[serde(default)]
            pub position: PagPosition,
            /// Placement keyframes used by the existing PAG renderer.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub keyframe_track: Option<PagKeyframeTrack>,
            /// Authored PAG layout dimensions.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub metadata: Option<PagMetadata>,
            /// Authored PAG zoom track.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub zooms: Vec<PagZoomPoint>,
            /// Legacy container animation metadata, retained but not currently rendered.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub in_animation: Option<PagAnimationType>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub active_animation: Option<PagAnimationType>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub out_animation: Option<PagAnimationType>,
            /// Legacy cutout intent. The current PAG renderer stores this value but
            /// does not yet apply cutout compositing.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_cutout: bool,
            /// Ordered PAG files in the sequence.
            pub items: Vec<FxPagSequenceItem>,
            /// Editable color slot values.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub color_inserts: Vec<PagColorInsert>,
            /// Editable image and video slot values.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub image_inserts: Vec<PagImageInsert>,
            /// Editable text slot values.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub text_inserts: Vec<PagTextInsert>,
            /// Optional two-source transition semantics for a PAG that spans a cut.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub transition: Option<PagTransition>,
        }
    };
}
