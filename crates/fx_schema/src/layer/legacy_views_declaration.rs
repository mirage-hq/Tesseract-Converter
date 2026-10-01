//! Borrowed projections of the historical Media wire contract.

/// Declare the historical serializer envelopes without moving migration behavior.
#[doc(hidden)]
#[allow(clippy::crate_in_macro_def)] // Bind nested values to the invoking reader.
#[macro_export]
macro_rules! define_legacy_media_views_schema {
    ($layer:ident, $source:ident, $asset:ident) => {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct $layer<'a> {
            id: LayerId,
            name: &'a str,
            #[serde(skip_serializing_if = "str::is_empty")]
            description: &'a str,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            is_hidden: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            parent: Option<LayerId>,
            blend_mode: BlendMode,
            #[serde(skip_serializing_if = "Option::is_none")]
            track_matte: Option<&'a TrackMatte>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            masks: &'a Vec<PathMask>,
            #[serde(skip_serializing_if = "Option::is_none")]
            corner_radius: Option<f64>,
            active_range: TimeRangeProperty,
            #[serde(skip_serializing_if = "Option::is_none")]
            source_range: Option<TimeRangeProperty>,
            #[serde(skip_serializing_if = "Option::is_none")]
            playback: Option<crate::time::TimeRemapProperty>,
            #[serde(skip_serializing_if = "Option::is_none")]
            source_intrinsic_duration: Option<Duration>,
            #[serde(skip_serializing_if = "Option::is_none")]
            volume: Option<&'a LinearGain>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            effects: &'a Vec<EffectInstance>,
            #[serde(skip_serializing_if = "Option::is_none")]
            placement: Option<MediaPlacement>,
            #[serde(skip_serializing_if = "Option::is_none")]
            captions_enabled: Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            caption_presentation: Option<&'a DonorCaptionPresentation>,
            #[serde(
                serialize_with = "serialize_frame_blending",
                skip_serializing_if = "Option::is_none"
            )]
            frame_blending: Option<FrameBlendingMode>,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            motion_blur: bool,
            transform: &'a Transform,
            source: $source<'a>,
        }

        #[derive(Serialize)]
        #[serde(untagged)]
        enum $source<'a> {
            Asset($asset<'a>),
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct $asset<'a> {
            asset_id: &'a AssetId,
            kind: MediaSourceKind,
            #[serde(skip_serializing_if = "Option::is_none")]
            input_transform: Option<&'a crate::PersistedInputTransform>,
            #[serde(rename = "sourceRect", skip_serializing_if = "Option::is_none")]
            frame_rect: Option<PositiveRect>,
            fit: MediaFit,
            #[serde(skip_serializing_if = "Option::is_none")]
            time_remap: Option<&'a ScalarProperty>,
        }
    };
}
