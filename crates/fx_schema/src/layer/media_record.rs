//! Historical Media fields retained as data, without conversion to Video/Image.

use super::{
    BlendMode, FrameBlendingMode, LinearGain, MediaPlacement, MediaSourceKind, PathMask,
    PositiveRect, TrackMatte, Transform,
};
use crate::{
    effect::record::EffectRecord as EffectInstance, AssetId, DonorCaptionPresentation, Duration,
    LayerId, PersistedInputTransform, ScalarProperty, TimeRangeProperty,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[path = "legacy_media_declaration.rs"]
mod legacy_media_declaration;

macro_rules! emit_stored_legacy_media {
    ($(#[$type_attr:meta])* pub struct LegacyMediaData { $($(#[$attr:meta])* pub $name:ident: $ty:ty,)* }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
        #[serde(rename_all = "camelCase")]
        pub struct LegacyMediaData { $($(#[$attr])* pub $name: $ty,)* }
    };
}
crate::define_legacy_media_schema!(emit_stored_legacy_media,
    description: [], hidden: [],
    start: Option<ScalarProperty>, start_doc: [doc = " Historical startTime is in seconds. Absence does not imply sourceRange.start."], start_attrs: [],
    corner: Option<ScalarProperty>, playback_attrs: [], volume_attrs: [],
    effects_attrs: [], caption_attrs: [],
    frame: Option<FrameBlendingData>, frame_attrs: [],
    motion_attrs: [], source: LegacyMediaSourceData
);

/// Bind the historical frame-blending wire alternatives to either reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_frame_blending_data_schema {
    () => {
        /// Both historical booleans and explicit algorithm names are stored literally.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
        #[serde(untagged)]
        pub enum FrameBlendingData {
            Boolean(bool),
            Mode(FrameBlendingMode),
        }
    };
}

define_frame_blending_data_schema!();

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMediaSourceData {
    pub asset_id: AssetId,
    pub kind: MediaSourceKind,
    #[serde(default, deserialize_with = "read_input_transform")]
    #[ts(type = "InputTransform | null")]
    pub input_transform: Option<PersistedInputTransform>,
    pub source_rect: Option<PositiveRect>,
    /// Keep the retired `none` spelling distinct instead of applying a fit policy.
    #[serde(default)]
    pub fit: Option<MediaFitData>,
    pub time_remap: Option<ScalarProperty>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum MediaFitData {
    None,
    Contain,
    Cover,
    Stretch,
    Custom {
        scale: super::PositiveVec2,
        #[serde(rename = "contentCenter")]
        content_center: super::FiniteVec2,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stored::Stored;
    use serde_json::json;

    fn legacy() -> Value {
        json!({
            "type": "Media", "id": 1, "name": "legacy",
            "activeRange": {"start": 0, "duration": 1000},
            "source": {"assetId": "asset", "kind": "video", "fit": "none"},
            "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100}
        })
    }

    #[test]
    fn historical_fields_are_not_inferred_from_the_source_kind() {
        let wire = legacy();
        let stored: Stored<LegacyMediaData> = serde_json::from_value(wire.clone()).unwrap();
        assert!(stored.data().start_time.is_none());
        assert!(stored.data().source_range.is_none());
        assert!(stored.data().source_intrinsic_duration.is_none());
        assert_eq!(stored.data().source.fit, Some(MediaFitData::None));
        assert_eq!(serde_json::to_value(stored).unwrap(), wire);
    }

    #[test]
    fn legacy_effects_crop_extensions_and_boolean_frame_blending_survive() {
        let mut wire = legacy();
        wire["frameBlending"] = json!(true);
        wire["source"]["retiredCrop"] = json!({"x": 0.25});
        wire["effects"] = json!([{"type": "shaderPreset", "presetId": "historical"}]);
        let stored: Stored<LegacyMediaData> = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            stored.data().frame_blending,
            Some(FrameBlendingData::Boolean(true))
        );
        assert_eq!(serde_json::to_value(stored).unwrap(), wire);
    }

    #[test]
    fn malformed_supported_media_fields_are_not_opaque_fallbacks() {
        for (field, value) in [
            ("volume", json!(-1)),
            ("startTime", json!("invalid")),
            ("frameBlending", json!(3)),
        ] {
            let mut wire = legacy();
            wire[field] = value;
            assert!(serde_json::from_value::<Stored<LegacyMediaData>>(wire).is_err());
        }
        let mut wire = legacy();
        wire["source"]["sourceRect"] = json!({"x": 0, "y": 0, "width": 0, "height": 1});
        assert!(serde_json::from_value::<Stored<LegacyMediaData>>(wire).is_err());
    }

    #[test]
    fn explicit_legacy_timing_is_not_reconciled_or_rebased() {
        let mut wire = legacy();
        wire["startTime"] = json!(2.5);
        wire["sourceRange"] = json!({"start": 7000, "duration": 1000});
        let stored: Stored<LegacyMediaData> = serde_json::from_value(wire.clone()).unwrap();
        assert!(stored.data().start_time.is_some());
        assert_eq!(
            stored.data().source_range.unwrap().start,
            crate::Time::from_millis(7000)
        );
        assert_eq!(serde_json::to_value(stored).unwrap(), wire);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum PlaybackData {
    Keyframes(crate::TimeRemapProperty),
    Rate { rate: crate::PositiveProperty },
}

impl<'de> Deserialize<'de> for PlaybackData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.get("keyframes").is_some()
            || value.get("before").is_some()
            || value.get("after").is_some()
        {
            serde_json::from_value(value)
                .map(Self::Keyframes)
                .map_err(serde::de::Error::custom)
        } else {
            #[derive(Deserialize)]
            struct Rate {
                rate: crate::PositiveProperty,
            }
            let rate: Rate = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            Ok(Self::Rate { rate: rate.rate })
        }
    }
}

fn read_input_transform<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PersistedInputTransform>, D::Error> {
    Option::<Value>::deserialize(deserializer)?
        .map(PersistedInputTransform::from_persisted_value)
        .transpose()
        .map_err(serde::de::Error::custom)
}
