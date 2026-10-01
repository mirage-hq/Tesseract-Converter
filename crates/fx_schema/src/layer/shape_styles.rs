//! Canonical persisted vector paint and stroke schema.
use super::{
    model::{default_miter_limit, default_one},
    BlendMode,
};
use crate::{ColorProperty, NonNegativeProperty, ScalarProperty, Vector2Property};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Define canonical vector paint and stroke records for a reader's gradient policy.
#[doc(hidden)]
#[macro_export]
macro_rules! define_shape_styles_schema {
    (historical: [$($historical:tt)*], migration: [$($migration:tt)*]) => {
        /// Fill style matching [`FillStyle`].
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeFillStyle {
            /// Fill paint.
            pub paint: ShapePaint,
            /// Fill winding rule.
            #[serde(default)]
            pub fill_rule: ShapeFillRule,
            /// Fill compositing mode.
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Fill opacity in normalized 0..=1 units.
            #[serde(default = "default_one")]
            pub opacity: ScalarProperty,
        }

        /// Stroke style matching [`StrokeStyle`].
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ShapeStrokeStyle {
            /// Whether this authored stroke participates in rendering. Disabled
            /// strokes retain their complete style so editor visibility toggles are
            /// non-destructive.
            #[serde(default = "default_true", skip_serializing_if = "is_true")]
            pub enabled: bool,
            /// Stroke paint.
            pub paint: ShapePaint,
            /// Stroke width in layer pixels.
            pub width: NonNegativeProperty,
            /// Stroke cap style.
            #[serde(default)]
            pub cap: ShapeLineCap,
            /// Stroke join style.
            #[serde(default)]
            pub join: ShapeLineJoin,
            /// Miter limit for miter joins.
            #[serde(default = "default_miter_limit")]
            pub miter_limit: ScalarProperty,
            /// Stroke compositing mode.
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Stroke opacity in normalized 0..=1 units.
            #[serde(default = "default_one")]
            pub opacity: ScalarProperty,
            /// Alternating dash/gap lengths in layer pixels; empty means solid
            /// stroke.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub dashes: Vec<NonNegativeProperty>,
            /// Phase offset into the dash pattern, in layer pixels.
            #[serde(default)]
            pub dash_offset: ScalarProperty,
        }

        /// Paint matching [`Paint`].
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(
            tag = "type",
            rename_all = "camelCase",
            rename_all_fields = "camelCase"
        )]
        #[ts(export_to = "project_types.d.ts")]
        pub enum ShapePaint {
            /// Solid RGBA paint.
            Solid {
                /// Solid color.
                color: ColorProperty,
            },
            /// Gradient paint.
            Gradient {
                /// Gradient algorithm.
                gradient_type: ShapeGradientType,
                /// Gradient start point in layer pixels.
                start: Vector2Property,
                /// Gradient end point in layer pixels.
                end: Vector2Property,
                /// Ordered gradient stops.
                stops: Vec<ShapeGradientStop>,
            },
        }

        /// Gradient type matching [`GradientType`].
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum ShapeGradientType {
            /// Linear gradient.
            #[default]
            $($migration)*
            Linear,
            Radial,
            Conic,
            $($historical)*
        }
    };
}

define_shape_styles_schema! {
    historical: [/// Historical gradient kind, retained without reinterpretation.
        Reflected,],
    migration: []
}

impl ShapeFillStyle {
    /// Constructs a solid fill with canonical wire defaults.
    #[must_use]
    pub fn solid(color: ColorProperty) -> Self {
        Self {
            paint: ShapePaint::Solid { color },
            fill_rule: ShapeFillRule::default(),
            blend_mode: BlendMode::default(),
            opacity: default_one(),
        }
    }
}

impl ShapeStrokeStyle {
    /// Constructs a solid enabled stroke with canonical wire defaults.
    #[must_use]
    pub fn solid(color: ColorProperty, width: NonNegativeProperty) -> Self {
        Self {
            enabled: true,
            paint: ShapePaint::Solid { color },
            width,
            cap: ShapeLineCap::default(),
            join: ShapeLineJoin::default(),
            miter_limit: default_miter_limit(),
            blend_mode: BlendMode::default(),
            opacity: default_one(),
            dashes: Vec::new(),
            dash_offset: 0.0,
        }
    }
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// Gradient stop matching [`GradientStop`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct ShapeGradientStop {
    /// Stop offset along the gradient axis, normalized 0..1.
    pub offset: ScalarProperty,
    /// Stop RGBA color.
    pub color: ColorProperty,
}

/// Shape fill rule matching [`FillRule`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum ShapeFillRule {
    #[default]
    NonZeroWinding,
    EvenOdd,
}

/// Stroke line cap matching [`LineCap`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum ShapeLineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// Stroke line join matching [`LineJoin`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum ShapeLineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

impl ShapePaint {
    /// Returns whether every authored paint component satisfies the persisted finite/range contract.
    #[must_use]
    pub fn has_valid_values(&self) -> bool {
        let valid_color = |color: &ColorProperty| {
            color
                .iter()
                .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
        };
        match self {
            Self::Solid { color } => valid_color(color),
            Self::Gradient {
                start, end, stops, ..
            } => {
                start.iter().chain(end).all(|value| value.is_finite())
                    && stops.len() >= 2
                    && stops.iter().all(|stop| {
                        stop.offset.is_finite()
                            && (0.0..=1.0).contains(&stop.offset)
                            && valid_color(&stop.color)
                    })
                    && stops
                        .windows(2)
                        .all(|pair| pair[0].offset <= pair[1].offset)
            }
        }
    }
}
