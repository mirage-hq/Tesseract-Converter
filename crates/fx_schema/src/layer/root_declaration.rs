//! One layer-variant inventory for stored records and product projections.

/// The flattened boolean compatibility reader rejects an unsupported playback key.
#[doc(hidden)]
#[macro_export]
macro_rules! define_boolean_wire_projection {
    ($emit:ident) => {
        $emit! {
            struct WireBooleanOperationLayer {
                #[serde(flatten)]
                #[ts(flatten)]
                layer: BooleanOperationLayer,
                #[serde(rename = "playback", default, deserialize_with = "deserialize_field_presence")]
                #[ts(skip)]
                playback_present: bool,
            }
        }
    };
}

/// Rebind the same layer variants to stored, editable and wire payloads.
#[doc(hidden)]
#[macro_export]
macro_rules! define_layer_root_schema {
    ($emit:ident, media_before: [$($media_before:tt)*], media_after: [$($media_after:tt)*], boolean_variant: [$($boolean_variant:meta),*], boolean_field: [$($boolean_field:meta),*], boolean_type: $boolean_type:ty) => {
        $emit! {
            /// A renderable layer in the composition tree.
            pub enum Layer {
            $($media_before)*
            /// Text layer: a styled string (point or wrapping box text), with
            /// optional per-character animators and path options.
            Text(TextLayer),
            $($media_after)*
            /// Time-based video content.
            Video(VideoLayer),
            /// Still-image content or a bounded window of the host's base footage.
            Image(ImageLayer),
            /// Standalone PAG sequence, including background and transition PAGs.
            Pag(PagLayer),
            /// Rectangle layer: the simplest filled/stroked primitive (optionally
            /// rounded).
            Rect(RectLayer),
            /// Vector shape layer: bezier paths / parametric generators with fills,
            /// strokes, and path modifiers.
            Shape(ShapeLayer),
            /// Audio-only layer (no visual output); contributes an audible clip.
            Audio(AudioLayer),
            /// Nested group layer containing its own layer stack.
            Group(GroupLayer),
            /// Semantic AI Edit segment containing its source video and style-owned FX
            /// children.
            AiEdit(AiEditLayer),
            $(#[$boolean_variant])*
            /// Figma-style boolean group: combines child layer geometry with a
            /// boolean formula before painting the result once (JRB-1214).
            BooleanOperation($(#[$boolean_field])* $boolean_type),
            /// AE-style adjustment layer: a contentless layer whose effects stack
            /// applies to the composite of every sibling layer BELOW it in the
            /// stack (ENG-1505).
            Adjustment(AdjustmentLayer),
            }
        }
    };
}
