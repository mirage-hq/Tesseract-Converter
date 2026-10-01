//! The editable composition field inventory; readers own normalization and caches.

/// Bind shared composition fields to a stored, editable, or borrowed wire record.
#[doc(hidden)]
#[macro_export]
macro_rules! define_editable_composition_schema {
    ($emit:ident, $defaults:meta, $motion_write:meta, $dynamics_write:meta, $segments_write:meta) => {
        $emit! {
            /// Editable FX composition state plus the animation graph that drives it.
            ///
            /// `Deserialize` is implemented manually so the load path re-runs the same
            /// dependency layer-kind lint as [`FXComposition::set_property_animator`]
            /// (e.g. a [`PropType::SourceRange`] dependency on a layer without time-based
            /// source media is rejected at parse time, not silently defaulted).
            pub struct FXComposition {
                /// Stable composition id used by project timeline actions.
                id: CompositionId,
                /// User-visible name for the composition.
                name: String,
                /// AE composition-level standard motion-blur controls.
                #[ts(optional = nullable)]
                #[$defaults]
                #[$motion_write]
                motion_blur: MotionBlurSettings,
                /// The animation graph driving every animated layer / effect / fx-item
                /// property. All time-dependence lives here; the `layers` tree holds
                /// only static structure and base values.
                #[ts(optional = nullable)]
                #[$defaults]
                #[$dynamics_write]
                dynamics: AnimationGraph,
                /// Root layer stack, TOPMOST first (AE paint order): earlier layers
                /// render above later ones.
                #[$defaults]
                layers: Vec<Layer>,
                /// Explicitly marked root layers that own project-timeline segment
                /// semantics. `AiEdit` roots carry the same role intrinsically and do not
                /// need to be persisted here. Segment deletion may ripple project-timed
                /// content, so ordinary media geometry is never used to infer membership.
                #[ts(optional, as = "Option<_>")]
                #[$defaults]
                #[$segments_write]
                segment_layer_ids: Vec<LayerId>,
            }
            provenance {
                #[serde(default, deserialize_with = "deserialize_optional_schema_version", skip_serializing_if = "Option::is_none")]
                version: Option<u8>,
            }
        }
    };
}
