//! Stored animation data and structural validation only.
use crate::PropertyTarget;
mod dto;
mod dto_graph;
mod dto_validation;
mod graph_data_declaration;
mod graph_declaration;
mod keyframe_declaration;
mod keyframes;
mod wire;
mod wire_declaration;
pub use dto::{AnimatorData, PropertyAnimator};
pub use dto_graph::{AnimationGraph, AnimationGraphEntry, GraphData};
pub use keyframes::{
    KeyframeId, PropertyKeyframe, PropertyKeyframeEasing, PropertyKeyframeError,
    PropertyKeyframeTrack, MAX_KEYFRAME_ID_BYTES,
};
/// Errors returned while building or evaluating an animation graph.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnimationGraphError {
    #[error("invalid graph data: {0}")]
    Wire(String),
    /// The serialized graph has the same property more than once.
    #[error("animation graph contains duplicate property {0}")]
    DuplicateProperty(PropertyTarget),
    /// A dependency references a property that has not been added.
    #[error("animation graph is missing property {0}")]
    UnknownProperty(PropertyTarget),
    /// One animator declared too many named layer references.
    #[error("animator for {property} contains {actual} layer references; maximum is {maximum}")]
    TooManyLayerRefs {
        property: PropertyTarget,
        actual: usize,
        maximum: usize,
    },
    /// Layer references are meaningful only for JavaScript animators.
    #[error("animator for {property} must be JavaScript to consume layer references")]
    LayerRefsRequireScript { property: PropertyTarget },
    /// A layer reference points at a layer whose source asset is animated.
    #[error("layer {layer_id} cannot provide stable metadata while its source asset is animated")]
    AnimatedLayerRefSource { layer_id: crate::LayerId },
    /// A dependency would create a cycle involving `property`.
    #[error("animation graph contains a cycle involving {property}")]
    Cycle {
        /// Property reported by the graph cycle detector.
        property: PropertyTarget,
    },
    /// An animator with an **unbounded** output range was attached to a
    /// finite-range property — one whose reachable value set must be
    /// statically enumerable before the first frame
    /// ([`PropType::finite_range_value_kind`]). Today that means a
    /// a JavaScript animator on any of the gated string properties:
    /// the asset ids ([`PropType::MediaSourceAssetId`] /
    /// [`PropType::AudioSourceAssetId`]) and fonts ([`PropType::FontFamily`]
    /// / [`PropType::FontStyle`]), where a script could return a value the
    /// resource preload never saw, plus the [`PropType::StrokeJoin`] string
    /// enum. Rejected at graph-construction time so the precomputed set
    /// can never be surprised by a frame.
    #[error(
        "property {property} requires a finite, statically-enumerable animator \
         range (got an unbounded {animator_kind} animator); {property} gates an \
         ahead-of-time precomputation and must stay enumerable"
    )]
    UnboundedAnimator {
        /// Property that rejected the animator.
        property: PropertyTarget,
        /// Kind of the rejected animator ([`PropertyAnimator::kind_label`]).
        animator_kind: &'static str,
    },
    /// An animator targeted a read-only property
    /// ([`PropType::is_read_only`]: the derived sources `AudioGain*`,
    /// `MediaColor` / `MediaLuminance`, and the timeline facts `ActiveRange` /
    /// `SourceRange`). Read-only properties are seeded at evaluation time and
    /// have no producing node, so an animator writing one is meaningless and
    /// rejected at graph-construction time.
    #[error("property {0} is read-only and cannot be animated")]
    ReadOnlyProperty(PropertyTarget),
    /// A finite-range property ([`PropType::finite_range_value_kind`]) was
    /// given a constant whose value kind it cannot enumerate as a key — e.g. a
    /// numeric constant on an asset-id property, which needs a
    /// [`PropertyValue::String`] asset id. The range is finite but carries no
    /// usable value, so it is rejected at construction rather than deferring
    /// to a per-frame apply-time `LayerPropertyError::TypeMismatch`.
    #[error(
        "property {property} requires a {expected} animator value but got {found}; \
         {property} gates an ahead-of-time precomputation and must stay enumerable"
    )]
    NonEnumerableValue {
        /// Property that rejected the value.
        property: PropertyTarget,
        /// Value kind the property requires ([`PropertyValueKind::label`]).
        expected: &'static str,
        /// Value kind that was supplied.
        found: &'static str,
    },
    /// A keyframe track is malformed or cannot drive its target through the
    /// legacy JavaScript contract.
    #[error("invalid keyframe animator for {property}: {source}")]
    InvalidKeyframes {
        /// Property that rejected the track.
        property: PropertyTarget,
        /// Track validation failure.
        #[source]
        source: PropertyKeyframeError,
    },
    /// Stable keyframe identities are unique across one composition graph.
    #[error(
        "property keyframe id {keyframe_id:?} is shared by {first_property} and {second_property}"
    )]
    DuplicateKeyframeId {
        /// Duplicated identity.
        keyframe_id: String,
        /// First target carrying the identity.
        first_property: PropertyTarget,
        /// Second target carrying the identity.
        second_property: PropertyTarget,
    },
}
