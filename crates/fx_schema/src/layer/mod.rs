//! Canonical persisted layer schema.

mod ai_edit;
mod audio;
mod boolean;
mod enums;
mod group;
mod mask;
mod media;
mod media_record;
pub use media_record::{FrameBlendingData, LegacyMediaData, MediaFitData, PlaybackData};
pub use wire::Layer;
mod model;
mod pag;
mod playback;
mod shape;
mod shape_path;
mod shape_styles;
mod styles;
mod text;
mod wire;

pub use ai_edit::{
    AiEditBackground, AiEditLayer, AiEditSticker, AiEditStickerContent, AiEditStickerLocation,
    AiEditStickerPoint,
};
pub use audio::{
    default_audio_volume, default_audio_window_ms, default_wire_audio_volume, AudioAutoDucking,
    AudioLayer, AudioLayerMetadata, AudioLayerModality, AudioLayerSource, AudioSource,
    AudioSourceEnhancement, LinearGain, LinearGainValidationError, AUTO_DUCK_DEFAULT_GAIN,
    AUTO_DUCK_DEFAULT_MERGE_GAP,
};
pub use boolean::{BooleanOp, BooleanOperationLayer};
pub use enums::{
    BlendMode, FiniteVec2, Justification, MediaFit, MediaPlacement, PositiveVec2, TextPathAlign,
    TrackMatteType, VerticalAlign,
};
pub use group::{
    AdjustmentLayer, GroupLayer, Playback, HIGH_COMPOSED_GROUP_PLAYBACK_RATE,
    MAX_GROUP_PLAYBACK_RATE, MIN_GROUP_PLAYBACK_RATE,
};
pub use mask::{MaskMode, PathMask, TrackMatte};
pub use media::{FrameBlendingMode, ImageLayer, VideoLayer, VideoLayerMetadata};
pub use model::{
    FontVariations, ImageAssetSource, ImageSource, LayerData, MediaFrame, MediaLayerSource,
    MediaSource, MediaSourceKind, MediaSourceRef, Position, PositiveRect, RectBounds, RectShape,
    TextDocument, Transform, VideoSource, VideoSourceEyeContact, MAX_FONT_VARIATION_AXES,
};
pub use pag::{
    FxPagImageRef, FxPagItemDuration, FxPagItemDurationType, FxPagLayerConfig,
    FxPagLayerConfigEntry, FxPagLayerConfigSolid, FxPagLayerConfigText, FxPagSequenceItem,
    FxPagSlots, FxPagVideoRef, PagAnimationType, PagAnimationTypeParseError, PagColorInsert,
    PagColorSlot, PagImageInsert, PagImageScaleMode, PagImageSlot, PagKeyframe,
    PagKeyframePosition, PagKeyframeTrack, PagLayer, PagLayerReference, PagMetadata, PagPoint,
    PagPosition, PagTextInsert, PagTextSlot, PagTransition, PagZoomPoint,
};
pub use playback::{LayerPlayback, LayerPlaybackMapping};
pub use shape::{
    RectLayer, ShapeContent, ShapeEllipse, ShapeLayer, ShapeOffsetPaths, ShapePolyStar,
    ShapePolyStarType, ShapeRoundCorners, ShapeTrimMode, ShapeTrimPaths,
};
pub use shape_path::{ShapeHandleMirror, ShapePath, ShapePathCommand};
pub use shape_styles::{
    ShapeFillRule, ShapeFillStyle, ShapeGradientStop, ShapeGradientType, ShapeLineCap,
    ShapeLineJoin, ShapePaint, ShapeStrokeStyle,
};
pub use styles::{
    BevelDirection, BevelEmbossStyle, BevelStyle, BevelTechnique, DropShadow, GlowSource,
    GradientOverlayStyle, InnerGlowStyle, InnerShadowStyle, LayerStrokePosition, LayerStyle,
    LayerStyleEntry, OuterGlowStyle, SatinStyle, StrokeOutlineStyle,
};
pub use text::{AnchorPointGrouping, TextAnchorOptions, TextLayer, TextPathOptions};
