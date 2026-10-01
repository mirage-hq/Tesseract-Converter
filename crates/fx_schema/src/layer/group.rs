//! Canonical persisted group and adjustment-layer schema.
use super::{BlendMode, Layer, PathMask, ShapeFillStyle, TrackMatte, Transform};
use crate::{
    effect::EffectRecord as EffectInstance, LayerId, LayerPlayback, NonNegativeProperty,
    TimeRangeProperty,
};
#[path = "group_declaration.rs"]
mod declaration;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MIN_GROUP_PLAYBACK_RATE: f64 = 0.1;

pub const MAX_GROUP_PLAYBACK_RATE: f64 = 10.0;

pub const HIGH_COMPOSED_GROUP_PLAYBACK_RATE: f64 = 16.0;

crate::define_group_playback_schema!();

crate::define_group_layer_schema! { reader: [] }

fn is_zero_non_negative(value: &NonNegativeProperty) -> bool {
    value.value() == 0.0
}
