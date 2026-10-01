//! Canonical persisted rectangle and shape-layer schema.
use super::{
    model::{default_hundred, default_miter_limit},
    BlendMode, PathMask, RectShape, ShapeFillStyle, ShapeLineJoin, ShapePath, ShapeStrokeStyle,
    TrackMatte, Transform,
};
use crate::{
    effect::EffectRecord as EffectInstance, LayerId, NonNegativeProperty, ScalarProperty,
    TimeRangeProperty, Vector2Property,
};
#[path = "shape_declaration.rs"]
mod declaration;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

crate::define_shape_layer_schema!();
