//! Shared rectangle geometry and paint; each reader owns its wire validation.

/// The remote serde projection for the shared rectangle fields.
#[doc(hidden)]
#[macro_export]
macro_rules! define_rect_shape_wire_schema {
    () => {
        #[derive(Serialize, Deserialize)]
        #[serde(remote = "RectShape", rename_all = "camelCase")]
        struct RectShapeWire {
            size: Vector2Property,
            #[serde(default)]
            position: Vector2Property,
            #[serde(default)]
            roundness: ScalarProperty,
            #[serde(default = "default_true")]
            fill_enabled: bool,
            fill_color: ColorProperty,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            fill_paint: Option<ShapePaint>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            fill_blend_mode: Option<BlendMode>,
            #[serde(default)]
            stroke_enabled: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            stroke_color: Option<ColorProperty>,
            #[serde(default)]
            stroke_width: NonNegativeProperty,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            stroke_dashes: Vec<NonNegativeProperty>,
            #[serde(default, skip_serializing_if = "is_zero_scalar")]
            stroke_dash_offset: ScalarProperty,
            #[serde(default, skip_serializing_if = "is_default_shape_line_join")]
            stroke_join: ShapeLineJoin,
            #[serde(
                default = "default_miter_limit",
                skip_serializing_if = "is_default_miter_limit"
            )]
            stroke_miter_limit: ScalarProperty,
        }
    };
}

/// Declare the editable rectangle without duplicating its fields.
#[doc(hidden)]
#[macro_export]
#[allow(clippy::crate_in_macro_def)] // Preserve product source spelling for schema generation.
macro_rules! define_rect_shape_schema {
    () => {
        /// AE rectangle shape/SolidLayer-compatible geometry and paint.
        #[derive(Debug, Clone, PartialEq, TS)]
        #[ts(rename_all = "camelCase", export_to = "project_types.d.ts")]
        pub struct RectShape {
            /// Rectangle `[width, height]` in layer pixels.
            pub size: Vector2Property,
            /// Local rectangle origin `[x, y]` in layer pixels, not a canvas-space
            /// placement. A freshly authored rect keeps this at `[0, 0]` with
            /// `transform.anchorPoint` at `size / 2` and the desired parent-space
            /// center in `transform.position`, but do NOT treat `[0, 0]` as an
            /// invariant: the origin is free, and a static size write moves it so
            /// that the layer anchor keeps its fraction of the rectangle — a
            /// centered anchor holds the center, a left-edge anchor holds the left
            /// edge (see [`crate::layer_bounds::anchored_resize_origin`]).
            /// Non-zero values are also valid when intentionally offsetting the path
            /// inside its layer.
            ///
            /// This generator is origin-authored where [`ShapeEllipse`] and
            /// [`ShapePolyStar`] are center-authored. Making it center-authored is
            /// the real fix for that asymmetry — it would also cover the keyframed
            /// and scripted size paths, which still grow from the origin — and is
            /// deferred for persisted-project migration cost (JRB-1886).
            pub position: Vector2Property,
            /// AE rectangle path roundness in pixels.
            pub roundness: ScalarProperty,
            /// Whether the solid fill is painted (default true).
            pub fill_enabled: bool,
            /// Fill color (RGBA, channels 0..1).
            pub fill_color: ColorProperty,
            /// Optional complete fill-paint override. Legacy rectangles omit this and
            /// derive a solid paint from [`Self::fill_color`]; gradients are stored
            /// here so the rectangle keeps its parametric size and roundness controls
            /// while sharing the vector-fill editor and renderer. `fill_color` stays
            /// populated as an intentional solid fallback for older readers.
            #[ts(optional)]
            pub fill_paint: Option<ShapePaint>,
            /// Controls rectangle fill compositing independently of the layer blend mode.
            /// Omission uses Normal. Unlike [`ShapeFillStyle::blend_mode`], this deliberately
            /// omits Normal so legacy rect documents stay byte-identical and strict validators
            /// see the field only when non-Normal blending is used.
            #[ts(optional)]
            pub fill_blend_mode: Option<BlendMode>,
            /// Whether the stroke is painted (default false).
            pub stroke_enabled: bool,
            /// Stroke color (RGBA, channels 0..1). The stroke renders only when
            /// `strokeEnabled` is true, `strokeWidth` > 0, AND a color is present.
            pub stroke_color: Option<ColorProperty>,
            /// Stroke width in layer pixels (non-negative).
            pub stroke_width: NonNegativeProperty,
            /// Alternating dash/gap lengths in layer pixels; empty means solid.
            pub stroke_dashes: Vec<NonNegativeProperty>,
            /// Phase offset into the dash pattern, in layer pixels.
            pub stroke_dash_offset: ScalarProperty,
            /// Stroke line-join style.
            pub stroke_join: ShapeLineJoin,
            /// Miter limit used when [`Self::stroke_join`] is [`ShapeLineJoin::Miter`].
            pub stroke_miter_limit: ScalarProperty,
        }
    };
}
