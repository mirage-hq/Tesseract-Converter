//! Shared persisted asset-source declarations with reader-specific runtime hooks.

/// Bind the split-source decoder's supported fields to either reader's policy.
#[doc(hidden)]
#[macro_export]
macro_rules! define_media_source_wire_schema {
    ($strict:meta) => {
        #[derive(Deserialize)]
        #[$strict]
        struct MediaSourceWire {
            asset_id: AssetId,
            #[serde(default)]
            eye_contact: Option<VideoSourceEyeContact>,
            #[serde(default)]
            audio_enhancement: Option<AudioSourceEnhancement>,
            #[serde(default)]
            input_transform: Option<serde_json::Value>,
            #[serde(
                default,
                rename = "sourceRect",
                deserialize_with = "lenient_frame_rect"
            )]
            frame_rect: Option<PositiveRect>,
            #[serde(default)]
            fit: MediaFit,
            time_remap: Option<ScalarProperty>,
        }
    };
}

/// Declare video, image and legacy-media sources once for both consumers.
#[doc(hidden)]
// Bind persisted transform wrappers to each reader's strictness policy.
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! define_media_source_schema {
    (strict: [$($strict:meta),*], runtime: [$($runtime:tt)*],
     rect_reader: $rect_reader:literal, input_reader: $input_reader:literal) => {
        /// Source settings for a video layer.
        #[derive(Debug, Clone, PartialEq, Serialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct VideoSource {
            /// Captions asset identifier for the source video.
            pub asset_id: AssetId,
            /// Optional Eye Contact state and asset lineage for editor actions.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub eye_contact: Option<VideoSourceEyeContact>,
            /// Optional enhanced-audio (denoise) state and asset lineage for this
            /// layer's embedded audio. `asset_id` remains the original asset.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub audio_enhancement: Option<AudioSourceEnhancement>,
            /// Optional reader-only source conversion evaluated before ordinary effects.
            /// Unsupported future semantic payloads are retained as opaque no-ops.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional, type = "InputTransform")]
            pub input_transform: Option<crate::PersistedInputTransform>,
            /// Optional authored layer-local media frame. When omitted, the frame uses
            /// the media's logical natural display dimensions at the origin.
            /// Serialized as `sourceRect`, the field's historical wire name.
            #[serde(rename = "sourceRect", skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable, type = "RectBounds | null")]
            pub frame_rect: Option<PositiveRect>,
            /// Lays out media within `sourceRect` or the natural frame. It does not
            /// resize the layer transform.
            #[serde(default)]
            #[ts(optional, as = "Option<_>")]
            pub fit: MediaFit,
            /// Optional static AE-style source time remap in source seconds.
            #[serde(skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub time_remap: Option<ScalarProperty>,
            $($runtime)*
        }

        /// Eye Contact state and the generated asset needed to apply it.
        /// `VideoSource::asset_id` remains the original asset.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $(, $strict)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct VideoSourceEyeContact {
            /// Whether this layer currently uses the Eye Contact output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub enabled: bool,
            /// Generated Eye Contact video asset.
            pub eye_contact_asset_id: AssetId,
        }

        /// Asset-backed source settings for a still-image layer.
        #[derive(Debug, Clone, PartialEq, Serialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ImageAssetSource {
            /// Captions asset identifier for the source image.
            pub asset_id: AssetId,
            /// Optional reader-only source conversion evaluated before ordinary effects.
            /// Unsupported future semantic payloads are retained as opaque no-ops.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional, type = "InputTransform")]
            pub input_transform: Option<crate::PersistedInputTransform>,
            /// Optional authored layer-local media frame. When omitted, the frame uses
            /// the media's logical natural display dimensions at the origin.
            /// Serialized as `sourceRect`, the field's historical wire name.
            #[serde(rename = "sourceRect", skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable, type = "RectBounds | null")]
            pub frame_rect: Option<PositiveRect>,
            /// Lays out media within `sourceRect` or the natural frame. It does not
            /// resize the layer transform.
            #[serde(default)]
            #[ts(optional, as = "Option<_>")]
            pub fit: MediaFit,
            /// Preserved legacy source-time remap metadata. Image evaluation ignores
            /// it, but retaining it keeps legacy `Media` round trips lossless.
            #[serde(skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub time_remap: Option<ScalarProperty>,
            $($runtime)*
        }

        /// Asset-backed source for a still-image layer.
        #[derive(Debug, Clone, PartialEq, Serialize, TS)]
        #[serde(untagged)]
        #[ts(export_to = "project_types.d.ts")]
        pub enum ImageSource { Asset(ImageAssetSource) }

        /// Legacy source settings accepted by the backwards-compatible `Media` tag.
        /// Unknown retired crop fields remain tolerated by this deliberately
        /// non-strict compatibility shape.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct MediaSource {
            /// Captions asset identifier for the source media.
            pub asset_id: AssetId,
            /// Source asset kind.
            pub kind: MediaSourceKind,
            /// Optional source color transform, retained when reading legacy Media layers.
            #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = $input_reader)]
            #[ts(optional, type = "InputTransform")]
            pub input_transform: Option<crate::PersistedInputTransform>,
            /// Optional authored layer-local media frame under its historical wire name.
            /// When omitted, the frame uses the media's logical natural display dimensions
            /// at the origin.
            #[serde(default, rename = "sourceRect", skip_serializing_if = "Option::is_none", deserialize_with = $rect_reader)]
            #[ts(optional = nullable, type = "RectBounds | null")]
            pub frame_rect: Option<PositiveRect>,
            /// Lays out media within `sourceRect` or the natural frame. It does not resize
            /// the layer transform.
            #[serde(default)]
            #[ts(optional, as = "Option<_>")]
            pub fit: MediaFit,
            /// Optional static AE-style source time remap in source seconds.
            #[serde(skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub time_remap: Option<ScalarProperty>,
            $($runtime)*
        }

        /// Media source asset kind.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts", rename_all = "camelCase")]
        pub enum MediaSourceKind { Video, Image }

        /// Asset source accepted by the backwards-compatible `Media` tag.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(untagged)]
        #[ts(export_to = "project_types.d.ts")]
        pub enum MediaLayerSource { Asset(MediaSource) }
    };
}
