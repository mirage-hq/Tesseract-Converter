//! Shared graph entry collection, with runtime indexes kept outside the wire.

/// Declare the strict runtime reader's persisted graph envelope.
#[doc(hidden)]
#[macro_export]
macro_rules! define_runtime_graph_reader_schema {
    ($emit:ident) => {
        $emit! {
            #[serde(rename_all = "camelCase")]
            struct WireAnimationGraph {
                #[serde(default)]
                entries: Vec<AnimationGraphEntry>,
            }
        }
    };
}

/// Bind the graph's authored entries to runtime or lossless storage.
#[doc(hidden)]
#[macro_export]
macro_rules! define_animation_graph_data_schema {
    ($emit:ident, $entries_attr:meta) => {
        $emit! {
            /// Directed acyclic graph that evaluates animator nodes in dependency order.
            ///
            /// This type stores the graph; execution is provided by the runtime adapter.
            pub struct AnimationGraph {
                /// The graph's animator entries — one per animated property target.
                /// Order does not matter; evaluation order derives from dependencies.
                pub entries: Vec<AnimationGraphEntry>,
            }
            reader_attributes { #[$entries_attr] }
        }
    };
}
