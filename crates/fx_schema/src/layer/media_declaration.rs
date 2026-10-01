//! Shared persisted video/image layer declarations. Reader-specific bindings are
//! supplied by the consuming crate without duplicating the field inventory.

/// Declare the compatibility video reader's field projection in the canonical owner.
/// Its optional time and raw volume are normalized before becoming `VideoLayer`.
#[doc(hidden)]
// Nested types must bind to the invoking reader, not the stored projection.
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! define_wire_video_layer_schema {
    ($emit:ident) => {
        $emit! {
            struct WireVideoLayer {
                id: LayerId,
                name: String,
                #[serde(default)]
                description: String,
                #[serde(default)]
                metadata: Option<VideoLayerMetadata>,
                #[serde(default)]
                is_hidden: bool,
                parent: Option<LayerId>,
                #[serde(default, deserialize_with = "deserialize_optional_time_secs")]
                start_time: Option<Time>,
                #[serde(default)]
                blend_mode: BlendMode,
                #[serde(alias = "layerMask")]
                track_matte: Option<TrackMatte>,
                #[serde(default)]
                masks: Vec<PathMask>,
                corner_radius: Option<f64>,
                #[serde(default, deserialize_with = "super::wire::deserialize_legacy_range_presence")]
                active_range: Option<TimeRangeProperty>,
                #[serde(default, deserialize_with = "super::wire::deserialize_legacy_range_presence")]
                source_range: Option<TimeRangeProperty>,
                #[serde(default, deserialize_with = "super::wire::deserialize_playback_presence")]
                playback: Option<serde_json::Value>,
                #[serde(default)]
                preserve_audio_pitch: bool,
                source_intrinsic_duration: Duration,
                volume: Option<f64>,
                #[serde(default, deserialize_with = "crate::effect::deserialize_effects")]
                effects: Vec<EffectInstance>,
                placement: Option<MediaPlacement>,
                captions_enabled: Option<bool>,
                #[serde(default)]
                caption_presentation: Option<DonorCaptionPresentation>,
                #[serde(default, deserialize_with = "deserialize_frame_blending")]
                frame_blending: Option<FrameBlendingMode>,
                #[serde(default)]
                motion_blur: bool,
                transform: Transform,
                #[serde(deserialize_with = "deserialize_persisted_video_source")]
                source: VideoSource,
            }
        }
    };
}

/// Define media layer payloads with the caller's effect and frame-blending readers.
#[doc(hidden)]
// Preserve caller-specific time-remap types and their source metadata spelling.
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! define_media_layer_schema {
    (
        metadata_serde: [$($metadata_serde:meta),*],
        video_serde: [$($video_serde:meta),*],
        image_serde: [$($image_serde:meta),*],
        start_time: $start_time:ty, start_attrs: [$($start_attr:meta),*],
        frame_blending: $frame_blending:ty, frame_attrs: [$($frame_attr:meta),*],
        video_source_attrs: [$($video_source_attr:meta),*],
        image_source_attrs: [$($image_source_attr:meta),*],
        image_effect_attrs: [$($image_effect_attr:meta),*]
    ) => {
        /// Semantic editor state for AI features applied to one video layer.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $(, $metadata_serde)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct VideoLayerMetadata {
            /// Whether AI Zoom has been applied to this video layer.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub ai_zoom_enabled: bool,
            /// Whether AI Trim has been applied to this video layer.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub ai_trim_enabled: bool,
        }

        /// Algorithm used to synthesize an in-between frame for slow TimeRemap samples.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum FrameBlendingMode { Simple, OpticalFlow }

        /// Video layer data with structurally required source timing.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $(, $video_serde)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct VideoLayer {
            /// Stable composition-unique layer id — the id space referenced by
            /// `parent`, track mattes, path masks, and text `pathOptions`.
            pub id: LayerId,
            /// User-visible name for the layer. This is distinct from `description`.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Semantic AI feature state for this specific video layer.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub metadata: Option<VideoLayerMetadata>,
            /// Whether this layer is excluded from all rendered output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing Group layer, or `null` for a root
            /// layer — mirrors the layer's position in the tree.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// Legacy AE source time offset. New documents should use
            /// [`Self::source_range`], which can represent both offset and stretch.
            /// Still accepted on input for backwards compatibility, but omitted from
            /// canonical JSON because `sourceRange` owns media source timing.
            $(#[$start_attr])*
            pub start_time: $start_time,
            /// How this layer composites over the stack below (see [`BlendMode`]).
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Optional inline track-matte source applied before blending.
            #[serde(alias = "layerMask", skip_serializing_if = "Option::is_none")]
            pub track_matte: Option<TrackMatte>,
            /// Stacked vector-path masks drawn directly on this layer, each carrying a
            /// stable [`FxItemId`] (JRB-1200). Distinct from the [`Self::track_matte`]
            /// track matte: these carry their own path geometry rather than
            /// referencing another layer's rendered alpha.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub masks: Vec<PathMask>,
            /// Normalized rounded-corner clip for the media frame (`0.0..=1.0`;
            /// radius px = value × min(w, h) / 2 of the frame, `1.0` = pill).
            /// `None`/`0` = square corners.
            ///
            /// For `fit: contain`, the clip follows the fitted media rectangle inside
            /// the frame, using the source's logical display dimensions. Cover,
            /// stretch, and custom fits round the frame.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub corner_radius: Option<f64>,
            /// Source-video span, independent of the visible playback window.
            pub source_range: TimeRangeProperty,
            /// Parent-clock window and editable source mapping.
            pub playback: LayerPlayback,
            /// Preserve perceived pitch for embedded audio during supported TimeRemap
            /// playback and export. Unsupported mappings use ordinary resampling.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub preserve_audio_pitch: bool,
            /// Intrinsic duration of the backing video source asset.
            #[ts(type = "number")]
            pub source_intrinsic_duration: Duration,
            /// Optional linear gain for the embedded audio track of an asset-backed
            /// video (`0.0` = mute, `1.0` = unity). Omission disables embedded audio
            /// so documents authored before schema v30 remain silent — unless an
            /// `AudioVolume` animator targets the layer, which opts it in with the
            /// animator driving the gain.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub volume: Option<LinearGain>,
            /// AE-style Effects stack applied to this layer's source frame in order.
            /// See [`LayerEffect`] for the available variants. The first matte-source
            /// effect (today only [`LayerEffect::PersonMatte`]) replaces the layer's
            /// rendered pixels with an alpha matte; later matte effects in the same
            /// stack are currently ignored.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub effects: Vec<EffectInstance>,
            /// Half-canvas takeover placement marker (schema v22). `None` — the
            /// default and the shape of every pre-v22 document — renders the layer
            /// from its ordinary fields alone. See [`MediaPlacement`] for the
            /// dynamics the marker drives on top of the persisted geometry.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub placement: Option<MediaPlacement>,
            /// Explicit participation in the project's effective caption track.
            /// Unset preserves pre-v26 behavior (video media layers default off).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub captions_enabled: Option<bool>,
            /// Sparse occurrence-local caption presentation keyed by canonical
            /// transcription coordinates. Projected effective-caption ids are never
            /// persisted here.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub caption_presentation: Option<DonorCaptionPresentation>,
            $(#[$frame_attr])*
            /// How slow TimeRemap samples combine their neighboring source frames.
            ///
            /// Legacy `true` decodes as [`FrameBlendingMode::Simple`]; `false` and an
            /// omitted field disable synthesis. Optical Flow is persisted as the
            /// `"opticalFlow"` string.
            pub frame_blending: $frame_blending,
            /// Whether this layer participates in the composition's standard motion blur.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub motion_blur: bool,
            /// The layer's transform (anchor point, position, scale, rotation,
            /// opacity).
            pub transform: Transform,
            $(#[$video_source_attr])*
            /// Captions video asset rendered by this layer.
            pub source: VideoSource,
        }

        /// Still-image layer data.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $(, $image_serde)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct ImageLayer {
            /// Stable composition-unique layer id.
            pub id: LayerId,
            /// User-visible name for the layer.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this layer is excluded from all rendered output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing group, or `None` for a root layer.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// How this layer composites over the stack below.
            #[serde(default)]
            pub blend_mode: BlendMode,
            /// Optional inline track-matte source applied before blending.
            #[serde(alias = "layerMask", skip_serializing_if = "Option::is_none")]
            pub track_matte: Option<TrackMatte>,
            /// Stacked vector-path masks.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub masks: Vec<PathMask>,
            /// Normalized rounded-corner clip for the media frame.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub corner_radius: Option<f64>,
            /// Immediate-parent-local span during which this layer is active.
            pub active_range: TimeRangeProperty,
            $(#[$image_effect_attr])*
            /// AE-style effects stack applied to the source frame.
            pub effects: Vec<EffectInstance>,
            /// Half-canvas takeover placement marker.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub placement: Option<MediaPlacement>,
            /// Preserved legacy metadata; image layers never contribute captions.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub captions_enabled: Option<bool>,
            /// Whether this layer participates in standard motion blur.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub motion_blur: bool,
            /// Layer transform.
            pub transform: Transform,
            $(#[$image_source_attr])*
            /// Still-image asset source.
            pub source: FxImageSource,
        }
    };
}
