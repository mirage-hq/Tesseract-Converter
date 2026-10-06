//! Canonical persisted effect schema.
mod compositing;
mod declaration;
pub use compositing::*;
mod instance;
mod instance_declaration;
pub(crate) mod record;
pub use record::{EffectData, EffectPayload, EffectRecord};
mod params_support;

pub(crate) fn deserialize_effects<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<EffectRecord>, D::Error> {
    <Vec<EffectRecord> as serde::Deserialize>::deserialize(deserializer)
}

pub use instance::{
    is_reserved_effect_param, EffectParam, EffectTextureInput, ANIMATION_TIME_PARAM,
};
pub use params_support::{
    ChannelSource, FractalType, NoiseType, SegmentationEmptyFallback, ShutterControl,
};

use crate::{
    layer::{
        BevelEmbossStyle, DropShadow, GradientOverlayStyle, InnerGlowStyle, InnerShadowStyle,
        OuterGlowStyle, SatinStyle, StrokeOutlineStyle,
    },
    NonNegativeProperty, PercentageProperty, PositiveProperty,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

crate::define_effect_payload_schema!();
