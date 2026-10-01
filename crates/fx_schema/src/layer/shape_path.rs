//! Authoring-space vector path wire types: the per-anchor
//! [`ShapePathCommand`] drawing commands (with handle-mirroring and
//! corner-radius anchor metadata) and the [`ShapeHandleMirror`] coupling
//! modes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{NonNegativeProperty, ScalarProperty};

/// How edits to one bezier handle of a path anchor couple to the opposite
/// handle (canvas shape-edit mode, JRB-1575). Stored per anchor on the
/// drawing command whose endpoint is that anchor; absent means [`Point`].
/// `Linear` is explicit so the editor can distinguish a linear anchor from a
/// free-handle anchor whose mode is absent.
///
/// [`Point`]: ShapeHandleMirror::Point
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum ShapeHandleMirror {
    /// Handles move independently.
    Point,
    /// The opposite handle mirrors the edited handle's angle; each handle
    /// keeps its own length. Retained for backwards compatibility with the
    /// original three-mode editor.
    Straight,
    /// The opposite handle mirrors both angle and length.
    Symmetrical,
    /// The anchor is linear and has no Bézier handles. The wire value remains
    /// `none` for compatibility with the editor bindings.
    #[serde(rename = "none")]
    #[ts(rename = "none")]
    Linear,
}

/// Single vector path drawing command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export_to = "project_types.d.ts")]
pub enum ShapePathCommand {
    /// Move the current point without drawing.
    MoveTo {
        /// Target x-coordinate.
        x: ScalarProperty,
        /// Target y-coordinate.
        y: ScalarProperty,
        /// Handle-mirroring mode of the anchor at this endpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        mirror: Option<ShapeHandleMirror>,
        /// Explicit corner-radius override in layer pixels. Absent inherits the
        /// shape's Round Corners modifier; explicit `0` keeps this anchor sharp.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        corner_radius: Option<NonNegativeProperty>,
    },
    /// Draw a straight segment from the current point.
    LineTo {
        /// Target x-coordinate.
        x: ScalarProperty,
        /// Target y-coordinate.
        y: ScalarProperty,
        /// Handle-mirroring mode of the anchor at this endpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        mirror: Option<ShapeHandleMirror>,
        /// Explicit corner-radius override in layer pixels. Absent inherits the
        /// shape's Round Corners modifier; explicit `0` keeps this anchor sharp.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        corner_radius: Option<NonNegativeProperty>,
    },
    /// Draw a cubic Bézier segment from the current point.
    CubicTo {
        /// First control point x-coordinate.
        c1x: ScalarProperty,
        /// First control point y-coordinate.
        c1y: ScalarProperty,
        /// Second control point x-coordinate.
        c2x: ScalarProperty,
        /// Second control point y-coordinate.
        c2y: ScalarProperty,
        /// Target x-coordinate.
        x: ScalarProperty,
        /// Target y-coordinate.
        y: ScalarProperty,
        /// Handle-mirroring mode of the anchor at this endpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        mirror: Option<ShapeHandleMirror>,
        /// Explicit corner-radius override in layer pixels. Absent inherits the
        /// shape's Round Corners modifier; explicit `0` keeps this anchor sharp.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        corner_radius: Option<NonNegativeProperty>,
    },
    /// Close the current contour.
    Close,
}

impl ShapePathCommand {
    /// The anchor's stored handle-mirroring mode; absent (and `close`, which
    /// has no anchor) reads as [`ShapeHandleMirror::Point`].
    #[must_use]
    pub fn mirror(&self) -> ShapeHandleMirror {
        match *self {
            Self::MoveTo { mirror, .. }
            | Self::LineTo { mirror, .. }
            | Self::CubicTo { mirror, .. } => mirror.unwrap_or(ShapeHandleMirror::Point),
            Self::Close => ShapeHandleMirror::Point,
        }
    }

    /// The anchor's explicit corner-radius override in layer pixels. `None`
    /// inherits the shape's layer-wide Round Corners modifier.
    #[must_use]
    pub fn corner_radius(&self) -> Option<f64> {
        match self {
            Self::MoveTo { corner_radius, .. }
            | Self::LineTo { corner_radius, .. }
            | Self::CubicTo { corner_radius, .. } => {
                corner_radius.as_ref().map(NonNegativeProperty::value)
            }
            Self::Close => None,
        }
    }

    /// The drawing command's endpoint — the anchor it contributes to the
    /// path. `None` for `close`, which has no endpoint of its own.
    #[must_use]
    pub fn endpoint(&self) -> Option<(f64, f64)> {
        match *self {
            Self::MoveTo { x, y, .. } | Self::LineTo { x, y, .. } | Self::CubicTo { x, y, .. } => {
                Some((x, y))
            }
            Self::Close => None,
        }
    }
}

/// Vector path command list matching `Path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct ShapePath {
    /// Ordered path commands.
    pub commands: Vec<ShapePathCommand>,
}

impl ShapePath {
    /// Endpoints of an authored straight Line: exactly one open `MoveTo →
    /// LineTo` segment. This structural identity is independent of its angle,
    /// so moving an endpoint diagonally does not turn a Line into a rectangle.
    #[must_use]
    pub fn line_endpoints(&self) -> Option<[[f64; 2]; 2]> {
        match self.commands.as_slice() {
            [ShapePathCommand::MoveTo { x: x0, y: y0, .. }, ShapePathCommand::LineTo { x: x1, y: y1, .. }]
                if [x0, y0, x1, y1]
                    .into_iter()
                    .all(|coordinate| coordinate.is_finite()) =>
            {
                Some([[*x0, *y0], [*x1, *y1]])
            }
            _ => None,
        }
    }

    /// True unless any coordinate in any command is non-finite (NaN / ±∞).
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.commands.iter().all(|command| match *command {
            ShapePathCommand::MoveTo { x, y, .. } | ShapePathCommand::LineTo { x, y, .. } => {
                x.is_finite() && y.is_finite()
            }
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                ..
            } => [c1x, c1y, c2x, c2y, x, y]
                .iter()
                .all(|coordinate| coordinate.is_finite()),
            ShapePathCommand::Close => true,
        })
    }
}
