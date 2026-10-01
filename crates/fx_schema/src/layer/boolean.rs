//! Canonical persisted Boolean-operation layer schema.
use super::{
    BlendMode, Layer, PathMask, ShapeFillStyle, ShapeStrokeStyle, ShapeTrimPaths, TrackMatte,
    Transform,
};
use crate::{effect::EffectRecord as EffectInstance, LayerId, TimeRangeProperty};
#[path = "boolean_declaration.rs"]
mod declaration;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Figma-style boolean formula combining child layer geometry (JRB-1214).
///
/// Semantics strictly follow Figma's boolean groups:
/// * `Union` — outer silhouette of all children merged.
/// * `Subtract` — the children *above* cut out of the **bottom-most** child
///   (Figma: "removes any areas overlapping the bottom layer of the current
///   selection"). In Tesseract's paint order the first child renders topmost,
///   so the subject is the **last** entry of `layers`.
/// * `Intersect` — only the area common to every child.
/// * `Exclude` — symmetric difference (XOR): overlaps removed, the rest kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum BooleanOp {
    #[default]
    Union,
    Subtract,
    Intersect,
    Exclude,
}

crate::define_boolean_layer_schema!();
