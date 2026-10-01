//! Historical Media layer fields, with independent normalization by each reader.

#[path = "legacy_views_declaration.rs"]
mod legacy_views_declaration;

/// Bind the historical field inventory to stored and product migration records.
#[doc(hidden)]
// Historical records use the invoking reader's clock and nested payload types.
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! define_legacy_media_schema {
    ($emit:ident,
     description: [$($description:meta),*], hidden: [$($hidden:meta),*],
     start: $start:ty, start_doc: [$($start_doc:meta),*], start_attrs: [$($start_attr:meta),*],
     corner: $corner:ty, playback_attrs: [$($playback_attr:meta),*],
     volume_attrs: [$($volume_attr:meta),*], effects_attrs: [$($effect_attr:meta),*],
     caption_attrs: [$($caption_attr:meta),*],
     frame: $frame:ty, frame_attrs: [$($frame_attr:meta),*],
     motion_attrs: [$($motion_attr:meta),*], source: $source:ty) => {
        $emit! {
            #[ts(rename = "MediaLayer")]
            pub struct LegacyMediaData {
                pub id: LayerId,
                pub name: String,
                $(#[$description])*
                #[serde(default)]
                pub description: String,
                $(#[$hidden])*
                #[serde(default)]
                pub is_hidden: bool,
                pub parent: Option<LayerId>,
                $(#[$start_doc])*
                $(#[$start_attr])*
                pub start_time: $start,
                #[serde(default)]
                pub blend_mode: BlendMode,
                #[serde(alias = "layerMask")]
                pub track_matte: Option<TrackMatte>,
                #[serde(default)]
                pub masks: Vec<PathMask>,
                pub corner_radius: $corner,
                pub active_range: TimeRangeProperty,
                pub source_range: Option<TimeRangeProperty>,
                $(#[$playback_attr])*
                pub playback: Option<crate::time::TimeRemapProperty>,
                pub source_intrinsic_duration: Option<Duration>,
                $(#[$volume_attr])*
                pub volume: Option<LinearGain>,
                $(#[$effect_attr])*
                #[serde(default)]
                pub effects: Vec<EffectInstance>,
                pub placement: Option<MediaPlacement>,
                pub captions_enabled: Option<bool>,
                $(#[$caption_attr])*
                pub caption_presentation: Option<DonorCaptionPresentation>,
                $(#[$frame_attr])*
                pub frame_blending: $frame,
                $(#[$motion_attr])*
                #[serde(default)]
                pub motion_blur: bool,
                pub transform: Transform,
                pub source: $source,
            }
        }
    };
}
