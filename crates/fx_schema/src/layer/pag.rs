//! Canonical persisted PAG-layer schema.
use std::collections::BTreeSet;

use crate::{AssetId, Color, Duration, LayerId, TimeRangeProperty, TimeRemapProperty};
use serde::{Deserialize, Serialize, Serializer};
use ts_rs::TS;

#[path = "pag_declaration.rs"]
mod declaration;
crate::define_pag_layer_schema!();

impl PagLayer {
    pub(crate) fn playback_is_valid(&self, playback: &TimeRemapProperty) -> bool {
        playback.is_unit_rate_trim()
            && playback.keyframes().iter().all(|key| {
                self.active_range.start <= key.value && key.value <= self.active_range.end()
            })
    }

    pub(crate) fn insert_slot_links_are_valid(&self) -> bool {
        fn unique_non_empty<'a>(ids: impl Iterator<Item = &'a str>) -> Option<BTreeSet<&'a str>> {
            let mut unique = BTreeSet::new();
            for id in ids {
                if id.is_empty() || !unique.insert(id) {
                    return None;
                }
            }
            Some(unique)
        }

        let Some(color_ids) =
            unique_non_empty(self.color_inserts.iter().map(|item| item.id.as_str()))
        else {
            return false;
        };
        let Some(image_ids) =
            unique_non_empty(self.image_inserts.iter().map(|item| item.id.as_str()))
        else {
            return false;
        };
        let Some(text_ids) =
            unique_non_empty(self.text_inserts.iter().map(|item| item.id.as_str()))
        else {
            return false;
        };
        self.items
            .iter()
            .filter_map(|item| item.slots.as_ref())
            .all(|slots| {
                slots
                    .color_slots
                    .iter()
                    .all(|slot| color_ids.contains(slot.insert_id.as_str()))
                    && slots
                        .image_slots
                        .iter()
                        .all(|slot| image_ids.contains(slot.insert_id.as_str()))
                    && slots
                        .text_slots
                        .iter()
                        .all(|slot| text_ids.contains(slot.insert_id.as_str()))
            })
    }

    /// Composition-local media references, in configuration, insert, then transition order.
    pub fn media_source_layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        let configurations = self.items.iter().flat_map(|item| {
            item.configuration
                .iter()
                .flat_map(|entry| entry.pag_layer_config.source_layer_ids.iter().copied())
        });
        let inserts = self
            .image_inserts
            .iter()
            .flat_map(|item| item.source_layer_ids.iter().copied());
        let transitions = self.transition.iter().flat_map(|transition| {
            [
                transition.outgoing_source_layer_id,
                transition.incoming_source_layer_id,
            ]
            .into_iter()
            .flatten()
        });
        configurations.chain(inserts).chain(transitions)
    }
}

/// Source bindings and semantic metadata for a transition PAG layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagTransition {
    /// Source held at the outgoing boundary. Absent for a project-head transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outgoing_source_layer_id: Option<LayerId>,
    /// Source held at the incoming boundary. Absent for a project-tail transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incoming_source_layer_id: Option<LayerId>,
    /// Cut position measured from `PagLayer.active_range.start`.
    pub cut_offset: Duration,
    /// Stable legacy transition identity, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_id: Option<String>,
    /// Logical transition PAG id before aspect-ratio asset resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_asset_id: Option<AssetId>,
    /// Template or transition kind selected by the authoring system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_type: Option<String>,
    /// Optional FX audio layer paired with the visual transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_layer_id: Option<LayerId>,
}

/// Container animation understood by the established PAG renderer.
///
/// The serialized names match the legacy `AnimationType` protobuf so a
/// migrated PAG keeps the same wire representation without accepting an
/// arbitrary renderer instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "project_types.d.ts")]
pub enum PagAnimationType {
    #[serde(rename = "ANIMATION_TYPE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "ANIMATION_TYPE_INSTANT")]
    Instant,
    #[serde(rename = "ANIMATION_TYPE_OPACITY")]
    Opacity,
    #[serde(rename = "ANIMATION_TYPE_POP_IN")]
    PopIn,
    #[serde(rename = "ANIMATION_TYPE_SCALE_IN")]
    ScaleIn,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_LEFT")]
    SlideLeft,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_UP")]
    SlideUp,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_IN_RIGHT")]
    SlideInRight,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_IN_DOWN")]
    SlideInDown,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_OUT_LEFT")]
    SlideOutLeft,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_OUT_UP")]
    SlideOutUp,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_OUT_RIGHT")]
    SlideOutRight,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_OUT_DOWN")]
    SlideOutDown,
    #[serde(rename = "ANIMATION_TYPE_POP_OUT")]
    PopOut,
    #[serde(rename = "ANIMATION_TYPE_SCALE_OUT")]
    ScaleOut,
    #[serde(rename = "ANIMATION_TYPE_ZOOM_IN")]
    ZoomIn,
    #[serde(rename = "ANIMATION_TYPE_ZOOM_OUT")]
    ZoomOut,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_IN_LEFT")]
    SlideInLeft,
    #[serde(rename = "ANIMATION_TYPE_SLIDE_IN_UP")]
    SlideInUp,
    #[serde(rename = "ANIMATION_TYPE_FLIP_IN")]
    FlipIn,
    #[serde(rename = "ANIMATION_TYPE_TRANSFORM")]
    Transform,
    #[serde(rename = "ANIMATION_TYPE_JUMP")]
    Jump,
    #[serde(rename = "ANIMATION_TYPE_RANDOM")]
    Random,
    #[serde(rename = "ANIMATION_TYPE_SOFT_RISE")]
    SoftRise,
    #[serde(rename = "ANIMATION_TYPE_IMPACT_SCALE")]
    ImpactScale,
    #[serde(rename = "ANIMATION_TYPE_STAGGERED_POP")]
    StaggeredPop,
    #[serde(rename = "ANIMATION_TYPE_RAPID_STREAM")]
    RapidStream,
    #[serde(rename = "ANIMATION_TYPE_LATERAL_SLIDE_LEFT")]
    LateralSlideLeft,
    #[serde(rename = "ANIMATION_TYPE_LATERAL_SLIDE_RIGHT")]
    LateralSlideRight,
}

impl PagAnimationType {
    #[must_use]
    pub const fn legacy_name(self) -> &'static str {
        match self {
            Self::Unspecified => "ANIMATION_TYPE_UNSPECIFIED",
            Self::Instant => "ANIMATION_TYPE_INSTANT",
            Self::Opacity => "ANIMATION_TYPE_OPACITY",
            Self::PopIn => "ANIMATION_TYPE_POP_IN",
            Self::ScaleIn => "ANIMATION_TYPE_SCALE_IN",
            Self::SlideLeft => "ANIMATION_TYPE_SLIDE_LEFT",
            Self::SlideUp => "ANIMATION_TYPE_SLIDE_UP",
            Self::SlideInRight => "ANIMATION_TYPE_SLIDE_IN_RIGHT",
            Self::SlideInDown => "ANIMATION_TYPE_SLIDE_IN_DOWN",
            Self::SlideOutLeft => "ANIMATION_TYPE_SLIDE_OUT_LEFT",
            Self::SlideOutUp => "ANIMATION_TYPE_SLIDE_OUT_UP",
            Self::SlideOutRight => "ANIMATION_TYPE_SLIDE_OUT_RIGHT",
            Self::SlideOutDown => "ANIMATION_TYPE_SLIDE_OUT_DOWN",
            Self::PopOut => "ANIMATION_TYPE_POP_OUT",
            Self::ScaleOut => "ANIMATION_TYPE_SCALE_OUT",
            Self::ZoomIn => "ANIMATION_TYPE_ZOOM_IN",
            Self::ZoomOut => "ANIMATION_TYPE_ZOOM_OUT",
            Self::SlideInLeft => "ANIMATION_TYPE_SLIDE_IN_LEFT",
            Self::SlideInUp => "ANIMATION_TYPE_SLIDE_IN_UP",
            Self::FlipIn => "ANIMATION_TYPE_FLIP_IN",
            Self::Transform => "ANIMATION_TYPE_TRANSFORM",
            Self::Jump => "ANIMATION_TYPE_JUMP",
            Self::Random => "ANIMATION_TYPE_RANDOM",
            Self::SoftRise => "ANIMATION_TYPE_SOFT_RISE",
            Self::ImpactScale => "ANIMATION_TYPE_IMPACT_SCALE",
            Self::StaggeredPop => "ANIMATION_TYPE_STAGGERED_POP",
            Self::RapidStream => "ANIMATION_TYPE_RAPID_STREAM",
            Self::LateralSlideLeft => "ANIMATION_TYPE_LATERAL_SLIDE_LEFT",
            Self::LateralSlideRight => "ANIMATION_TYPE_LATERAL_SLIDE_RIGHT",
        }
    }
}

/// An animation name that cannot be represented by [`PagAnimationType`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported PAG animation type: {value}")]
pub struct PagAnimationTypeParseError {
    value: String,
}

impl TryFrom<&str> for PagAnimationType {
    type Error = PagAnimationTypeParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "ANIMATION_TYPE_UNSPECIFIED" => Ok(Self::Unspecified),
            "ANIMATION_TYPE_INSTANT" => Ok(Self::Instant),
            "ANIMATION_TYPE_OPACITY" => Ok(Self::Opacity),
            "ANIMATION_TYPE_POP_IN" => Ok(Self::PopIn),
            "ANIMATION_TYPE_SCALE_IN" => Ok(Self::ScaleIn),
            "ANIMATION_TYPE_SLIDE_LEFT" => Ok(Self::SlideLeft),
            "ANIMATION_TYPE_SLIDE_UP" => Ok(Self::SlideUp),
            "ANIMATION_TYPE_SLIDE_IN_RIGHT" => Ok(Self::SlideInRight),
            "ANIMATION_TYPE_SLIDE_IN_DOWN" => Ok(Self::SlideInDown),
            "ANIMATION_TYPE_SLIDE_OUT_LEFT" => Ok(Self::SlideOutLeft),
            "ANIMATION_TYPE_SLIDE_OUT_UP" => Ok(Self::SlideOutUp),
            "ANIMATION_TYPE_SLIDE_OUT_RIGHT" => Ok(Self::SlideOutRight),
            "ANIMATION_TYPE_SLIDE_OUT_DOWN" => Ok(Self::SlideOutDown),
            "ANIMATION_TYPE_POP_OUT" => Ok(Self::PopOut),
            "ANIMATION_TYPE_SCALE_OUT" => Ok(Self::ScaleOut),
            "ANIMATION_TYPE_ZOOM_IN" => Ok(Self::ZoomIn),
            "ANIMATION_TYPE_ZOOM_OUT" => Ok(Self::ZoomOut),
            "ANIMATION_TYPE_SLIDE_IN_LEFT" => Ok(Self::SlideInLeft),
            "ANIMATION_TYPE_SLIDE_IN_UP" => Ok(Self::SlideInUp),
            "ANIMATION_TYPE_FLIP_IN" => Ok(Self::FlipIn),
            "ANIMATION_TYPE_TRANSFORM" => Ok(Self::Transform),
            "ANIMATION_TYPE_JUMP" => Ok(Self::Jump),
            "ANIMATION_TYPE_RANDOM" => Ok(Self::Random),
            "ANIMATION_TYPE_SOFT_RISE" => Ok(Self::SoftRise),
            "ANIMATION_TYPE_IMPACT_SCALE" => Ok(Self::ImpactScale),
            "ANIMATION_TYPE_STAGGERED_POP" => Ok(Self::StaggeredPop),
            "ANIMATION_TYPE_RAPID_STREAM" => Ok(Self::RapidStream),
            "ANIMATION_TYPE_LATERAL_SLIDE_LEFT" => Ok(Self::LateralSlideLeft),
            "ANIMATION_TYPE_LATERAL_SLIDE_RIGHT" => Ok(Self::LateralSlideRight),
            _ => Err(PagAnimationTypeParseError {
                value: value.to_owned(),
            }),
        }
    }
}

fn default_pag_opacity() -> f64 {
    1.0
}

/// Placement modes supported by the current PAG overlay renderer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "project_types.d.ts")]
pub enum PagPosition {
    #[serde(rename = "OVERLAY_POSITION_KEYFRAME")]
    Keyframe,
    #[serde(rename = "OVERLAY_POSITION_TOP_HALF")]
    TopHalf,
    #[serde(rename = "OVERLAY_POSITION_BOTTOM_HALF")]
    BottomHalf,
    #[serde(rename = "OVERLAY_POSITION_CAPTIONS_AWARE")]
    CaptionsAware,
    #[serde(rename = "OVERLAY_POSITION_FULL_SCREEN")]
    FullScreen,
    #[serde(rename = "OVERLAY_POSITION_FILL")]
    Fill,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagKeyframeTrack {
    #[serde(default)]
    pub keyframes: Vec<PagKeyframe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagKeyframe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<PagKeyframePosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_curve: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagKeyframePosition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<PagPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_factor: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "project_types.d.ts")]
pub struct PagPoint {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagZoomPoint {
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub zoom_factor: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focal_point: Option<PagPoint>,
}

/// One PAG file in a PAG sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagSequenceItem {
    /// Asset-service id for the PAG file.
    pub asset_id: AssetId,
    /// Optional fixed, looped, or custom duration policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<FxPagItemDuration>,
    /// Direct PAG layer configuration used by templates without slot metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configuration: Vec<FxPagLayerConfigEntry>,
    /// Slot-to-PAG-layer mappings for editable inserts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<FxPagSlots>,
    /// Whether this item uses the intro timing rule.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_intro: bool,
}

/// One direct PAG layer configuration entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagLayerConfigEntry {
    pub key: String,
    pub pag_layer_config: FxPagLayerConfig,
}

/// Optional replacement content for one PAG layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagLayerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<FxPagImageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<FxPagVideoRef>,
    /// Ordered FX media sources used where the legacy schema referenced base
    /// footage. The renderer selects the source active at the current
    /// composition time, so one PAG can span several semantic segments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_layer_ids: Vec<LayerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<FxPagLayerConfigText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid: Option<FxPagLayerConfigSolid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagLayerConfigSolid {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagLayerConfigText {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagImageRef {
    pub asset_id: AssetId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagVideoRef {
    pub asset_id: AssetId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_mode: Option<PagImageScaleMode>,
}

/// PAG sequence item duration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export_to = "project_types.d.ts")]
pub enum FxPagItemDuration {
    Custom {
        #[serde(rename = "customSeconds")]
        custom_seconds: f64,
    },
    Standard {
        #[serde(rename = "durationType", default)]
        duration_type: FxPagItemDurationType,
    },
}

/// Standard PAG duration modes. Unknown proto values remain readable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[ts(export_to = "project_types.d.ts")]
pub enum FxPagItemDurationType {
    #[default]
    #[serde(rename = "PAG_ITEM_DURATION_TYPE_DURATION")]
    Duration,
    #[serde(rename = "PAG_ITEM_DURATION_TYPE_DURATION_LOOP")]
    DurationLoop,
    #[serde(rename = "PAG_ITEM_DURATION_TYPE_CUSTOM")]
    Custom,
    #[serde(untagged)]
    Other(String),
}

impl Serialize for FxPagItemDurationType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = match self {
            Self::Duration => "PAG_ITEM_DURATION_TYPE_DURATION",
            Self::DurationLoop => "PAG_ITEM_DURATION_TYPE_DURATION_LOOP",
            Self::Custom => "PAG_ITEM_DURATION_TYPE_CUSTOM",
            Self::Other(value) => value,
        };
        serializer.serialize_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagColorInsert {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagImageInsert {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<FxPagImageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<FxPagVideoRef>,
    /// Ordered FX media sources used where the legacy schema referenced base
    /// footage. The renderer selects the source active at the current
    /// composition time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_layer_ids: Vec<LayerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_mode: Option<PagImageScaleMode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagTextInsert {
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxPagSlots {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_slots: Vec<PagColorSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_slots: Vec<PagImageSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_slots: Vec<PagTextSlot>,
}

macro_rules! pag_slot {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct $name {
            pub insert_id: String,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            pub layer_references: Vec<PagLayerReference>,
        }
    };
}

pag_slot!(PagColorSlot);

pag_slot!(PagImageSlot);

pag_slot!(PagTextSlot);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PagLayerReference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

/// Existing PAG renderer scale modes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "project_types.d.ts")]
pub enum PagImageScaleMode {
    #[default]
    #[serde(rename = "PAG_IMAGE_INSERT_SCALE_MODE_STRETCH")]
    Stretch,
    #[serde(rename = "PAG_IMAGE_INSERT_SCALE_MODE_LETTERBOX")]
    Letterbox,
    #[serde(rename = "PAG_IMAGE_INSERT_SCALE_MODE_ZOOM")]
    Zoom,
}
