//! Portable canonical schema for persisted FX composition documents.
//!
//! This crate owns lossless wire data and structural validation. Runtime
//! evaluation, compatibility migrations, and editor state are private.

use serde::{de, Deserialize, Deserializer, Serialize};
use ts_rs::TS;

pub mod animator;
pub mod asset_metadata;
pub mod asset_refs;
mod color_management;
pub mod composed_edit;
pub use color_management::{
    reject_nested_color_management, reject_scoped_color_management, validate_render_domain_nesting,
    validate_render_domains, AcesPolicy, ColorManagement, ColorManagementError, Compositing,
    OutputTransform, RenderDomain, RenderDomainDiagnostic, WorkingSpace,
};
pub mod editable_document;
pub mod effect;
mod snapshot;
mod stored;
pub mod text_animator;

pub use animator::{AnimationGraph, KeyframeId, PropertyAnimator, PropertyKeyframeEasing};
pub use composed_edit::{FXComposition, ValidationError};
pub use editable_document::{
    EditableFxCompositionDocument, EditableFxDocument, EditableFxDocumentError,
    EDITABLE_FX_COMPOSITION_SCHEMA, EDITABLE_FX_DOCUMENT_FORMAT_VERSION,
    EDITABLE_FX_DOCUMENT_SCHEMA_URL, HDR_GRAPHICS_WHITE_NITS_RANGE,
};
pub use snapshot::{
    Composition, CompositionData as SnapshotCompositionData, FxComposition, FxCompositionDocument,
    SnapshotData,
};
pub use text_animator::{RangeSelector, TextAnimator, WigglySelector};
mod canvas;
pub use canvas::{Dimensions, MotionBlurSettings};
mod caption_presentation;
pub mod color;
pub mod color_tools;
pub mod curves;
pub use color_tools::{ChannelMixer, HueCurves, SelectiveColor};
pub mod font_metadata;
pub mod id;
pub mod layer;
pub mod pag_validation;
pub mod property;
pub mod time;
pub mod time_remap;
mod time_remap_declaration;
pub use effect::{
    EffectData, EffectParam, EffectPayload, EffectRecord, EffectTextureInput, LayerEffect,
};
pub use time_remap::{
    TimeRemapError, TimeRemapExtrapolation, TimeRemapKeyframe, TimeRemapProperty,
};

pub use asset_metadata::{
    AssetMetadata, AssetMetadataMap, AssetMetadataSource, LayerRef, LayerRefMap,
    MAX_ANIMATOR_LAYER_REFS, MAX_ASSET_METADATA_ENTRIES, MAX_ASSET_METADATA_NAME_BYTES,
};
pub use caption_presentation::{
    DonorCaptionColor, DonorCaptionColorOverride, DonorCaptionEmojiOverride,
    DonorCaptionJsonOverride, DonorCaptionPhrasePresentationOverride, DonorCaptionPositionMode,
    DonorCaptionPresentation, DonorCaptionWordPresentationOverride,
};
pub use color::{
    ColorEncodingId, ColorEncodingIdError, ColorLutInterpolation, ColorTransform,
    ColorTransformSemanticVersion, InputColorTransform, InputTransform, NormalizedColorMix,
    NormalizedColorMixError, PersistedInputTransform, PrimaryGrade, PrimaryGradeError,
    PrimaryGradeSemanticVersion, TonalColor, TonalColorError, TonalColorSemanticVersion,
    ACESCCT_ENCODING_ID, INPUT_TRANSFORM_SEMANTIC_ID, LOOK_TRANSFORM_SEMANTIC_ID,
    PRIMARY_GRADE_SEMANTIC_ID, SDR_REC709_DISPLAY_ENCODING_ID, TONAL_COLOR_SEMANTIC_ID,
};
pub use curves::{
    ColorCurve, ColorCurveError, ColorCurvePoint, ColorCurves, ColorCurvesSemanticVersion,
    MAX_COLOR_CURVE_POINTS,
};
pub use font_metadata::{
    custom_font_selection_name, validate_embedded_font_registry, validate_font_asset_properties,
    FontAssetProperties, FontFaceMetadata, FontRegistryValidationError, FontVariationAxisMetadata,
    FontVariationCoordinateMetadata, FontVariationInstanceMetadata, MAX_CUSTOM_FONT_AXES_PER_FACE,
    MAX_CUSTOM_FONT_FACES, MAX_CUSTOM_FONT_INSTANCES_PER_FACE,
    MAX_CUSTOM_FONT_SELECTION_NAMES_PER_FACE, MAX_CUSTOM_FONT_SELECTION_NAME_BYTES,
};
pub use id::{AssetId, CompositionId, EffectId, FxItemId, InvalidAssetId, LayerId};
pub use layer::{
    default_audio_volume, default_audio_window_ms, default_wire_audio_volume, AdjustmentLayer,
    AiEditBackground, AiEditLayer, AiEditSticker, AiEditStickerContent, AiEditStickerLocation,
    AiEditStickerPoint, AnchorPointGrouping, AudioLayer, AudioLayerMetadata, AudioLayerModality,
    AudioLayerSource, AudioSource, AudioSourceEnhancement, BevelDirection, BevelEmbossStyle,
    BevelStyle, BevelTechnique, BlendMode, BooleanOp, BooleanOperationLayer, DropShadow,
    FiniteVec2, FontVariations, FrameBlendingMode, FxPagImageRef, FxPagItemDuration,
    FxPagItemDurationType, FxPagLayerConfig, FxPagLayerConfigEntry, FxPagLayerConfigSolid,
    FxPagLayerConfigText, FxPagSequenceItem, FxPagSlots, FxPagVideoRef, GlowSource,
    GradientOverlayStyle, GroupLayer, ImageAssetSource, ImageLayer, ImageSource, InnerGlowStyle,
    InnerShadowStyle, Justification, Layer, LayerData, LayerPlayback, LayerPlaybackMapping,
    LayerStrokePosition, LayerStyle, LayerStyleEntry, LinearGain, LinearGainValidationError,
    MaskMode, MediaFit, MediaFrame, MediaLayerSource, MediaPlacement, MediaSource, MediaSourceKind,
    MediaSourceRef, OuterGlowStyle, PagAnimationType, PagAnimationTypeParseError, PagColorInsert,
    PagColorSlot, PagImageInsert, PagImageScaleMode, PagImageSlot, PagKeyframe,
    PagKeyframePosition, PagKeyframeTrack, PagLayer, PagLayerReference, PagMetadata, PagPoint,
    PagPosition, PagTextInsert, PagTextSlot, PagTransition, PagZoomPoint, PathMask, Playback,
    Position, PositiveRect, PositiveVec2, RectBounds, RectLayer, RectShape, SatinStyle,
    ShapeContent, ShapeEllipse, ShapeFillRule, ShapeFillStyle, ShapeGradientStop,
    ShapeGradientType, ShapeHandleMirror, ShapeLayer, ShapeLineCap, ShapeLineJoin,
    ShapeOffsetPaths, ShapePaint, ShapePath, ShapePathCommand, ShapePolyStar, ShapePolyStarType,
    ShapeRoundCorners, ShapeStrokeStyle, ShapeTrimMode, ShapeTrimPaths, StrokeOutlineStyle,
    TextAnchorOptions, TextDocument, TextLayer, TextPathAlign, TextPathOptions, TrackMatte,
    TrackMatteType, Transform, VerticalAlign, VideoLayer, VideoLayerMetadata, VideoSource,
    VideoSourceEyeContact, HIGH_COMPOSED_GROUP_PLAYBACK_RATE, MAX_FONT_VARIATION_AXES,
    MAX_GROUP_PLAYBACK_RATE, MIN_GROUP_PLAYBACK_RATE,
};
pub use property::{
    AudioChannel, EffectParamTarget, FxItemTarget, PropBounds, PropType, Property, PropertyTarget,
    PropertyValue, TimeRangeProperty,
};
pub use time::{Duration, Time, TimeOffset, TimeRange};

/// Canonical public authoring schema, shared by readers and documentation APIs.
pub const FX_COMPOSITION_SCHEMA: &str = include_str!("../fx_composition.schema.json");

/// FX composition schema revision.
pub const FX_SCHEMA_REVISION: u8 = 77;

/// Backward-compatible schema revision name.
pub const SCHEMA_VERSION: u8 = FX_SCHEMA_REVISION;

/// Extended-sRGB RGBA paint: finite RGB 0..64, alpha 0..1.
pub type Color = [f64; 4];
/// Deserialize bounded extended-sRGB paint while keeping alpha as coverage.
pub fn deserialize_extended_color<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Color, D::Error> {
    let color = Color::deserialize(deserializer)?;
    if !has_valid_extended_color(&color) {
        return Err(de::Error::custom(
            "RGB must be finite in 0..=64 and alpha in 0..=1",
        ));
    }
    Ok(color)
}

/// Deserialize an optional authored paint without changing absent/null semantics.
pub fn deserialize_optional_extended_color<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Color>, D::Error> {
    let color = Option::<Color>::deserialize(deserializer)?;
    if color
        .as_ref()
        .is_some_and(|color| !has_valid_extended_color(color))
    {
        return Err(de::Error::custom(
            "RGB must be finite in 0..=64 and alpha in 0..=1",
        ));
    }
    Ok(color)
}

/// Two-dimensional `[x, y]` vector.
pub type Vector2 = [f64; 2];
/// Three-dimensional `[x, y, z]` vector.
pub type Vector3 = [f64; 3];
/// AE/PAG-style scalar property with no domain bound.
pub type ScalarProperty = f64;

/// Implicit AE camera distance for 2.5D layers, in composition widths.
pub const DEFAULT_CAMERA_DISTANCE_PER_WIDTH: f64 = 1.388;
/// AE/PAG-style 2D vector property.
pub type Vector2Property = Vector2;
/// AE/PAG-style 3D vector property.
pub type Vector3Property = Vector3;
/// AE/PAG-style RGBA color property.
pub type ColorProperty = Color;

/// Maximum encoded extended-sRGB channel for editable shape and solid paint.
/// This keeps continued-EOTF values representable in the HDR float target.
pub const MAX_EXTENDED_COLOR_CHANNEL: f64 = 64.0;

/// Validates extended-sRGB shape/solid RGB and normalized coverage alpha.
#[must_use]
pub fn has_valid_extended_color(color: &ColorProperty) -> bool {
    color.iter().enumerate().all(|(index, channel)| {
        let maximum = if index == 3 {
            1.0
        } else {
            MAX_EXTENDED_COLOR_CHANNEL
        };
        channel.is_finite() && (0.0..=maximum).contains(channel)
    })
}

/// Finite scalar greater than or equal to zero.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct NonNegativeProperty(f64);

/// Finite scalar greater than zero.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct PositiveProperty(f64);

/// Finite scalar in the inclusive range `0..=100`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct PercentageProperty(f64);

impl NonNegativeProperty {
    /// Creates a non-negative scalar property.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self(value))
    }

    /// Returns the wrapped scalar.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.0
    }
}

impl Default for NonNegativeProperty {
    fn default() -> Self {
        Self(0.0)
    }
}

impl<'de> Deserialize<'de> for NonNegativeProperty {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value)
            .ok_or_else(|| de::Error::custom("expected non-negative finite property value"))
    }
}

impl PositiveProperty {
    /// Creates a positive scalar property.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    /// Returns the wrapped scalar.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PositiveProperty {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| de::Error::custom("expected positive finite property value"))
    }
}

impl PercentageProperty {
    /// Creates a percentage property.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=100.0).contains(&value)).then_some(Self(value))
    }

    /// Returns the wrapped scalar.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PercentageProperty {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value)
            .ok_or_else(|| de::Error::custom("expected finite percentage property value"))
    }
}
