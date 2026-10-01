//! Canonical persisted layer root and shared payload records.
use super::*;
use crate::{
    effect::EffectRecord, AssetId, ColorProperty, FxItemId, LayerId, NonNegativeProperty,
    PercentageProperty, PositiveProperty, ScalarProperty, TimeRangeProperty, Vector2Property,
    Vector3Property,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use ts_rs::TS;

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Serde helper for `Arc<str>`: deserialize via `String` then `Arc::from`,
/// serialize via `&str`. The wire format stays a plain JSON string; the
/// in-memory representation is shared via `Arc` so per-frame `evaluate_node`
/// calls clone the refcount instead of allocating a fresh `Arc<str>` for
/// every text layer.
mod arc_str {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<str>, D::Error> {
        String::deserialize(d).map(Arc::from)
    }

    pub fn serialize<S: Serializer>(value: &Arc<str>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(value)
    }
}

#[path = "root_declaration.rs"]
mod root_declaration;

macro_rules! emit_stored_layer_root {
    ($(#[$doc:meta])* pub enum Layer { $($variants:tt)* }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(tag = "type")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum LayerData { $($variants)* }
    };
}
crate::define_layer_root_schema!(emit_stored_layer_root,
    media_before: [Media(super::media_record::LegacyMediaData),], media_after: [],
    boolean_variant: [serde(deserialize_with = "wire::deserialize_boolean")],
    boolean_field: [], boolean_type: BooleanOperationLayer
);

impl LayerData {
    /// User-visible name shared by every layer variant.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Text(layer) => &layer.name,
            Self::Video(layer) => &layer.name,
            Self::Media(layer) => &layer.name,
            Self::Image(layer) => &layer.name,
            Self::Pag(layer) => &layer.name,
            Self::Rect(layer) => &layer.name,
            Self::Shape(layer) => &layer.name,
            Self::Audio(layer) => &layer.name,
            Self::Group(layer) => &layer.name,
            Self::AiEdit(layer) => &layer.name,
            Self::BooleanOperation(layer) => &layer.name,
            Self::Adjustment(layer) => &layer.name,
        }
    }

    /// Stable identifier shared by every layer variant.
    pub fn id(&self) -> LayerId {
        match self {
            Self::Text(layer) => layer.id,
            Self::Video(layer) => layer.id,
            Self::Media(layer) => layer.id,
            Self::Image(layer) => layer.id,
            Self::Pag(layer) => layer.id,
            Self::Rect(layer) => layer.id,
            Self::Shape(layer) => layer.id,
            Self::Audio(layer) => layer.id,
            Self::Group(layer) => layer.id,
            Self::AiEdit(layer) => layer.id,
            Self::BooleanOperation(layer) => layer.id,
            Self::Adjustment(layer) => layer.id,
        }
    }

    /// Immediate containing layer identifier, when this layer is nested.
    pub fn parent_id(&self) -> Option<LayerId> {
        match self {
            Self::Text(layer) => layer.parent,
            Self::Video(layer) => layer.parent,
            Self::Media(layer) => layer.parent,
            Self::Image(layer) => layer.parent,
            Self::Pag(layer) => layer.parent,
            Self::Rect(layer) => layer.parent,
            Self::Shape(layer) => layer.parent,
            Self::Audio(layer) => layer.parent,
            Self::Group(layer) => layer.parent,
            Self::AiEdit(layer) => layer.parent,
            Self::BooleanOperation(layer) => layer.parent,
            Self::Adjustment(layer) => layer.parent,
        }
    }

    /// Nested child stack for container layers.
    pub fn child_layers(&self) -> Option<&[Layer]> {
        match self {
            Self::Group(layer) => Some(&layer.layers),
            Self::AiEdit(layer) => Some(&layer.layers),
            Self::BooleanOperation(layer) => Some(&layer.layers),
            _ => None,
        }
    }

    /// Ordered effect stack, empty for layer kinds that cannot carry effects.
    pub fn effects(&self) -> &[EffectRecord] {
        match self {
            Self::Video(layer) => &layer.effects,
            Self::Media(layer) => &layer.effects,
            Self::Image(layer) => &layer.effects,
            Self::Group(layer) => &layer.effects,
            Self::Text(layer) => &layer.effects,
            Self::Rect(layer) => &layer.effects,
            Self::Shape(layer) => &layer.effects,
            Self::BooleanOperation(layer) => &layer.effects,
            Self::Adjustment(layer) => &layer.effects,
            Self::Pag(_) | Self::AiEdit(_) | Self::Audio(_) => &[],
        }
    }

    /// Persisted active interval for every layer variant.
    pub fn active_range(&self) -> TimeRangeProperty {
        match self {
            Self::Text(layer) => layer.active_range,
            Self::Video(layer) => layer.playback.input_range(),
            Self::Media(layer) => layer.active_range,
            Self::Image(layer) => layer.active_range,
            Self::Pag(layer) => layer.active_range,
            Self::Rect(layer) => layer.active_range,
            Self::Shape(layer) => layer.active_range,
            Self::Audio(layer) => layer.playback.input_range(),
            Self::Group(layer) => layer.playback.input_range(),
            Self::AiEdit(layer) => layer.active_range,
            Self::BooleanOperation(layer) => layer.active_range,
            Self::Adjustment(layer) => layer.active_range,
        }
    }

    /// Whether this layer carries a source-media time range.
    pub fn supports_source_range(&self) -> bool {
        matches!(self, Self::Video(_) | Self::Audio(_))
            || matches!(self, Self::Media(layer) if layer.source_range.is_some())
    }

    /// Stable diagnostic name for this layer variant.
    pub fn layer_type_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "Text",
            Self::Video(_) => "Video",
            Self::Media(_) => "Media",
            Self::Image(_) => "Image",
            Self::Pag(_) => "Pag",
            Self::Rect(_) => "Rect",
            Self::Shape(_) => "Shape",
            Self::Audio(_) => "Audio",
            Self::Group(_) => "Group",
            Self::AiEdit(_) => "AiEdit",
            Self::BooleanOperation(_) => "BooleanOperation",
            Self::Adjustment(_) => "Adjustment",
        }
    }

    /// Validate positive persisted timing ranges recursively.
    pub fn validate_timing_ranges(&self) -> Result<(), String> {
        fn positive(id: LayerId, field: &str, duration: crate::Duration) -> Result<(), String> {
            if duration.is_zero() {
                return Err(format!(
                    "FX composition layer {id} {field} duration must be positive"
                ));
            }
            Ok(())
        }

        positive(self.id(), "activeRange", self.active_range().duration)?;
        match self {
            Self::Media(layer) => {
                if let Some(range) = layer.source_range {
                    positive(layer.id, "sourceRange", range.duration)?;
                }
                if let Some(duration) = layer.source_intrinsic_duration {
                    positive(layer.id, "sourceIntrinsicDuration", duration)?;
                }
            }
            Self::Video(layer) => {
                positive(layer.id, "sourceRange", layer.source_range.duration)?;
                positive(
                    layer.id,
                    "sourceIntrinsicDuration",
                    layer.source_intrinsic_duration,
                )?;
            }
            Self::Audio(layer) => {
                positive(layer.id, "sourceRange", layer.source_range.duration)?;
                positive(
                    layer.id,
                    "sourceIntrinsicDuration",
                    layer.source_intrinsic_duration,
                )?;
            }
            Self::Group(layer) => layer
                .layers
                .iter()
                .try_for_each(Layer::validate_timing_ranges)?,
            Self::AiEdit(layer) => layer
                .layers
                .iter()
                .try_for_each(Layer::validate_timing_ranges)?,
            Self::BooleanOperation(layer) => {
                layer
                    .layers
                    .iter()
                    .try_for_each(Layer::validate_timing_ranges)?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[path = "source_declaration.rs"]
mod source_declaration;

crate::define_media_source_schema! {
    strict: [],
    runtime: [],
    rect_reader: "lenient_frame_rect",
    input_reader: "deserialize_persisted_input_transform"
}

// The public decoder retains unknown source fields in the stored record.
crate::define_media_source_wire_schema!(serde(rename_all = "camelCase"));

/// Decode supported fields without migrating historical source records.
fn parse_media_source_wire(value: serde_json::Value) -> Result<MediaSourceWire, serde_json::Error> {
    serde_json::from_value(value)
}

#[derive(Debug, Clone, Copy)]
enum ColorTransformReadMode {
    Strict,
    Persisted,
}

fn parse_input_transform(
    payload: Option<serde_json::Value>,
    mode: ColorTransformReadMode,
) -> Result<Option<crate::PersistedInputTransform>, serde_json::Error> {
    payload
        .map(|payload| match mode {
            ColorTransformReadMode::Strict => {
                serde_json::from_value::<crate::InputTransform>(payload)
                    .map(crate::PersistedInputTransform::from)
            }
            ColorTransformReadMode::Persisted => {
                crate::PersistedInputTransform::from_persisted_value(payload)
            }
        })
        .transpose()
}

fn video_source_from_wire(
    wire: MediaSourceWire,
    mode: ColorTransformReadMode,
) -> Result<VideoSource, serde_json::Error> {
    Ok(VideoSource {
        asset_id: wire.asset_id,
        eye_contact: wire.eye_contact,
        audio_enhancement: wire.audio_enhancement,
        input_transform: parse_input_transform(wire.input_transform, mode)?,
        frame_rect: wire.frame_rect,
        fit: wire.fit,
        time_remap: wire.time_remap,
    })
}

fn image_source_from_wire(
    wire: MediaSourceWire,
    mode: ColorTransformReadMode,
) -> Result<ImageAssetSource, String> {
    if wire.eye_contact.is_some() {
        return Err("eyeContact is only valid for video sources".to_owned());
    }
    if wire.audio_enhancement.is_some() {
        return Err("audioEnhancement is only valid for video sources".to_owned());
    }
    Ok(ImageAssetSource {
        asset_id: wire.asset_id,
        input_transform: parse_input_transform(wire.input_transform, mode)
            .map_err(|error| error.to_string())?,
        frame_rect: wire.frame_rect,
        fit: wire.fit,
        time_remap: wire.time_remap,
    })
}

impl<'de> Deserialize<'de> for VideoSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = serde_json::Value::deserialize(deserializer)?;
        parse_media_source_wire(value)
            .and_then(|wire| video_source_from_wire(wire, ColorTransformReadMode::Strict))
            .map_err(|error| D::Error::custom(format!("invalid video source: {error}")))
    }
}

pub(crate) fn deserialize_persisted_video_source<'de, D>(
    deserializer: D,
) -> Result<VideoSource, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value = serde_json::Value::deserialize(deserializer)?;
    parse_media_source_wire(value)
        .and_then(|wire| video_source_from_wire(wire, ColorTransformReadMode::Persisted))
        .map_err(|error| D::Error::custom(format!("invalid video source: {error}")))
}

impl<'de> Deserialize<'de> for ImageAssetSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = serde_json::Value::deserialize(deserializer)?;
        parse_media_source_wire(value)
            .map_err(D::Error::custom)
            .and_then(|wire| {
                image_source_from_wire(wire, ColorTransformReadMode::Strict)
                    .map_err(D::Error::custom)
            })
    }
}

impl<'de> Deserialize<'de> for ImageSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        ImageAssetSource::deserialize(deserializer)
            .map(Self::Asset)
            .map_err(|error| D::Error::custom(format!("invalid image source: {error}")))
    }
}

pub(crate) fn deserialize_persisted_image_source<'de, D>(
    deserializer: D,
) -> Result<ImageSource, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value = serde_json::Value::deserialize(deserializer)?;
    parse_media_source_wire(value)
        .map_err(|error| D::Error::custom(format!("invalid image source: {error}")))
        .and_then(|wire| {
            image_source_from_wire(wire, ColorTransformReadMode::Persisted)
                .map(ImageSource::Asset)
                .map_err(|error| D::Error::custom(format!("invalid image source: {error}")))
        })
}

/// Optional rectangles remain optional; malformed supported rectangles reject.
fn lenient_frame_rect<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PositiveRect>, D::Error> {
    Option::<PositiveRect>::deserialize(deserializer)
}

/// Borrowed source discriminator for APIs shared by video and image layers.
#[derive(Debug, Clone, Copy)]
pub enum MediaSourceRef<'a> {
    /// Video source.
    Video(&'a VideoSource),
    /// Still-image source.
    Image(&'a ImageAssetSource),
}

fn deserialize_persisted_input_transform<'de, D>(
    deserializer: D,
) -> Result<Option<crate::PersistedInputTransform>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    value
        .map(crate::PersistedInputTransform::from_persisted_value)
        .transpose()
        .map_err(serde::de::Error::custom)
}

impl MediaLayerSource {
    #[must_use]
    pub fn asset(&self) -> Option<&MediaSource> {
        let Self::Asset(source) = self;
        Some(source)
    }

    #[must_use]
    pub fn asset_mut(&mut self) -> Option<&mut MediaSource> {
        let Self::Asset(source) = self;
        Some(source)
    }

    #[must_use]
    pub fn kind(&self) -> Option<MediaSourceKind> {
        self.asset().map(|source| source.kind)
    }

    #[must_use]
    pub fn fit(&self) -> MediaFit {
        let Self::Asset(source) = self;
        source.fit
    }

    pub fn fit_mut(&mut self) -> &mut MediaFit {
        let Self::Asset(source) = self;
        &mut source.fit
    }

    #[must_use]
    pub fn frame_rect(&self) -> Option<RectBounds> {
        let Self::Asset(source) = self;
        source.frame_rect.map(PositiveRect::get)
    }

    pub fn frame_rect_mut(&mut self) -> &mut Option<PositiveRect> {
        let Self::Asset(source) = self;
        &mut source.frame_rect
    }

    #[must_use]
    pub fn frame(&self) -> MediaFrame {
        let Self::Asset(source) = self;
        source
            .frame_rect
            .map_or(MediaFrame::Natural, MediaFrame::Fixed)
    }
}

/// Rectangle bounds in layer-local pixels.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct RectBounds {
    /// Layer-local left edge.
    pub x: f64,
    /// Layer-local top edge.
    pub y: f64,
    /// Layer-local width.
    pub width: f64,
    /// Layer-local height.
    pub height: f64,
}

impl RectBounds {
    /// Rectangle of the given dimensions anchored at the origin.
    #[must_use]
    pub const fn from_size(width: f64, height: f64) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }
}

/// A finite rectangle with strictly positive dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(into = "RectBounds", try_from = "RectBounds")]
#[ts(type = "RectBounds")]
pub struct PositiveRect(RectBounds);

impl PositiveRect {
    /// Validate an authored media frame rectangle.
    #[must_use]
    pub fn new(rect: RectBounds) -> Option<Self> {
        [rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|value| value.is_finite())
            .then_some(rect)
            .filter(|rect| rect.width > 0.0 && rect.height > 0.0)
            .map(Self)
    }

    /// Return the validated rectangle payload.
    #[must_use]
    pub const fn get(self) -> RectBounds {
        self.0
    }
}

impl TryFrom<RectBounds> for PositiveRect {
    type Error = &'static str;

    fn try_from(rect: RectBounds) -> Result<Self, Self::Error> {
        Self::new(rect).ok_or("media frame rect must be finite with strictly positive dimensions")
    }
}

impl From<PositiveRect> for RectBounds {
    fn from(rect: PositiveRect) -> Self {
        rect.get()
    }
}

/// Authored media frame policy. Natural resolves from logical asset metadata
/// at evaluation and is never persisted as a rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaFrame {
    /// Resolve to `(0, 0, natural_display_width, natural_display_height)`.
    Natural,
    /// Use the complete authored frame, including its origin.
    Fixed(PositiveRect),
}

#[path = "rect_declaration.rs"]
mod rect_declaration;
crate::define_rect_shape_schema!();

crate::define_rect_shape_wire_schema!();

fn is_zero_scalar(value: &ScalarProperty) -> bool {
    *value == 0.0
}

fn is_default_shape_line_join(value: &ShapeLineJoin) -> bool {
    *value == ShapeLineJoin::default()
}

fn is_default_miter_limit(value: &ScalarProperty) -> bool {
    *value == default_miter_limit()
}

/// Wire-boundary error for an explicitly authored invalid rectangle paint.
const RECT_FILL_PAINT_WIRE_ERROR: &str = "rectangle fill paint must contain finite colors and coordinates, colors and ordered stop offsets between zero and one, and at least two gradient stops";

impl Serialize for RectShape {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RectShapeWire::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for RectShape {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let shape = RectShapeWire::deserialize(deserializer)?;
        if shape
            .fill_paint
            .as_ref()
            .is_some_and(|paint| !paint.has_valid_values())
            || !shape
                .fill_color
                .iter()
                .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
        {
            return Err(serde::de::Error::custom(RECT_FILL_PAINT_WIRE_ERROR));
        }
        Ok(shape)
    }
}

/// Maximum variable-font axes carried into one shaped glyph cache key.
///
/// OpenType permits more in theory, but production variable fonts (including
/// multi-axis designs such as Roboto Flex) fit within this bound. Keeping the
/// set inline makes cosmic-text glyph cache keys copyable and allocation-free.
pub const MAX_FONT_VARIATION_AXES: usize = 16;

/// Variable-font coordinates authored on one FX text layer.
///
/// The OpenType axis names are dynamic per font, so they live in `axes`
/// rather than a fixed Rust enum. The collection carries one stable
/// [`FxItemId`]; animation targets address an axis by that id plus its
/// four-character tag as the `fxItemProperty.propertyName`.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FontVariations {
    /// Stable, composition-unique identity of this variable-axis collection.
    pub(crate) id: FxItemId,
    /// OpenType four-character axis tag to its authored coordinate.
    #[ts(type = "Record<string, number>")]
    pub(crate) axes: BTreeMap<String, ScalarProperty>,
}

impl<'de> Deserialize<'de> for FontVariations {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            id: FxItemId,
            axes: BTreeMap<String, ScalarProperty>,
        }

        let Wire { id, axes } = Wire::deserialize(deserializer)?;
        if axes.len() > MAX_FONT_VARIATION_AXES {
            return Err(serde::de::Error::custom(format!(
                "fontVariations has {} axes; at most {MAX_FONT_VARIATION_AXES} are supported",
                axes.len()
            )));
        }
        if let Some(tag) = axes.keys().find(|tag| !FontVariations::is_valid_tag(tag)) {
            return Err(serde::de::Error::custom(format!(
                "font variation axis tag {tag:?} must contain exactly four printable ASCII bytes"
            )));
        }
        Ok(Self { id, axes })
    }
}

/// AE Source Text document data / PAG TextDocument subset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct TextDocument {
    /// The source string rendered by the layer.
    pub text: String,
    /// Font family name (resolved against the bundled font index).
    #[serde(with = "arc_str")]
    #[ts(type = "string")]
    pub font_family: Arc<str>,
    /// Font style / weight name (e.g. `"Regular"`, `"Bold"`; default
    /// `"Regular"`).
    #[serde(with = "arc_str", default = "default_font_style")]
    #[ts(type = "string")]
    pub font_style: Arc<str>,
    /// Font size in pixels.
    pub font_size: PositiveProperty,
    /// Optional variable-font axis coordinates. Axis values are graph-
    /// animatable through this collection's stable [`FxItemId`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub font_variations: Option<FontVariations>,
    /// Whether the glyph fill is painted (default true).
    #[serde(default = "default_true")]
    pub apply_fill: bool,
    /// Glyph fill color (RGBA, channels 0..1).
    pub fill_color: ColorProperty,
    /// Whether the glyph stroke is painted (default false).
    #[serde(default)]
    pub apply_stroke: bool,
    /// Glyph stroke color (RGBA, channels 0..1). This is the whole-string
    /// text stroke — distinct from the `stroke` layer-style entry (a
    /// silhouette outline) and from Rect/Shape stroke properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<ColorProperty>,
    /// Glyph stroke width in pixels (non-negative).
    #[serde(default)]
    pub stroke_width: NonNegativeProperty,
    /// Paint the stroke above the fill when true (AE "Stroke Over Fill").
    #[serde(default)]
    pub stroke_over_fill: bool,
    /// Paragraph justification (left / center / right / justified variants).
    #[serde(default)]
    pub justification: Justification,
    /// AE character tracking amount in `1/1000 em` (signed; whole-string —
    /// per-character tracking lives on text animators).
    #[serde(default)]
    pub tracking: ScalarProperty,
    /// Optional line height in pixels. `None` means auto-leading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leading: Option<PositiveProperty>,
    /// Baseline shift in pixels.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub baseline_shift: ScalarProperty,
    /// `true` = wrapping box/paragraph text (requires `boxSize` +
    /// `boxPosition`); `false` = point text placed at a baseline-like
    /// origin. Use box text for a known layout region that needs paragraph or
    /// vertical alignment. Use point text for free-standing intrinsic text.
    /// Box text wraps inside the box; it does not clip or shrink overflow.
    #[serde(default)]
    pub box_text: bool,
    /// Whether layer transform scale should resize box-text glyphs instead of
    /// only changing the paragraph layout frame. The editor opts a layer in
    /// when multi-selection resize writes transform scale; existing authored
    /// documents remain on the legacy rendering path.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scale_box_text_with_transform: bool,
    /// Text box `[width, height]` in layer pixels. Required when `boxText`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub box_size: Option<Vector2Property>,
    /// Text box origin `[x, y]` in layer pixels (the layer transform then
    /// places the box on the canvas). Required when `boxText`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub box_position: Option<Vector2Property>,
    /// Y-coordinate of the first text baseline in the layer's local
    /// coordinate space (AE/PAG `firstBaseLine`). When absent the renderer
    /// approximates it as `boxPosition.y + fontSize`, which keeps legacy
    /// documents rendering unchanged but sits up to a quarter font-size off
    /// AE's real ascent-based value — carriers of authored AE/PAG box text
    /// (e.g. the PAG→FX converter) should set it.
    ///
    /// NOT an AE-exposed parameter (deviation, PAG-import support): AE
    /// derives the box-text first baseline from font metrics internally and
    /// has no such property in its UI; the PAG file format bakes the derived
    /// value into `TextDocument.firstBaseLine` at export, and carrying it on
    /// this wire is what lets converted text reproduce AE's placement
    /// exactly instead of re-deriving it approximately.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub box_first_baseline: Option<f64>,
    /// Render the text uppercased (display transform only — `text` keeps
    /// its authored casing). Default false.
    #[serde(default)]
    pub all_caps: bool,
    /// Draw an underline beneath the text, in the fill color (Figma
    /// `textDecoration: UNDERLINE`; not an AE-native attribute). Kept off
    /// the wire when false so pre-v21 documents re-serialize byte-identical.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// Draw a strike line through the text, in the fill color (Figma
    /// `textDecoration: STRIKETHROUGH`; not an AE-native attribute). Kept off
    /// the wire when false so pre-v21 documents re-serialize byte-identical.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strikethrough: bool,
    /// Vertical alignment of box text within its box (Figma
    /// `textAlignVertical`). `None` keeps the legacy libpag *AdjustToFitBox*
    /// behavior — see [`VerticalAlign`]. Ignored for point text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub vertical_align: Option<VerticalAlign>,
}

/// AE-style layer position. Wire shape is `[x, y]` for 2D layers or
/// `[x, y, z]` for 3D layers — same dual arity AE uses for the layer
/// `Position` property.
///
/// # AE faithfulness
///
/// AE's `position` is a single property whose arity changes with the
/// layer's 3D switch:
///
///   - 3D switch **OFF** → `[x, y]`    (2D layer)
///   - 3D switch **ON**  → `[x, y, z]` (3D layer; `z = 0` is meaningful)
///
/// We encode both forms in one enum. The arity is the **structural**
/// signal: a layer is 3D iff [`Position::is_3d`] returns `true`. A layer
/// with `ThreeD([x, y, 0.0])` is a 3D layer at the camera plane,
/// distinct from `TwoD([x, y])`. This matters because depth-sort bin
/// membership and perspective projection must be deterministic
/// regardless of the current animated value, so the 3D-ness check is
/// **data-only** and never depends on time, animator graph state, or
/// the current evaluated `z`.
///
/// # Layer-index vs `position.z` — how AE reconciles them
///
/// AE renders a composition's children using a **bin** model. The rule
/// (faithful to AE Classic 3D) is:
///
/// 1. Walk siblings in timeline order, grouping consecutive **3D**
///    layers into one bin. A **2D** layer terminates the current bin
///    and starts a new (degenerate) one — 2D layers are bin breakers.
/// 2. Within a 3D bin, depth-sort by **camera distance**: layers with
///    larger `z` (farther from the camera) paint first, smaller `z`
///    (closer) paint last on top. The sort is **stable**, so ties
///    resolve back to timeline order. Consequence: a 3D bin in which
///    every layer has `z = 0` paints in the same order a pure 2D
///    stack would — they're visually indistinguishable in that case,
///    even though structurally the bin still exists.
/// 3. Between bins, paint order follows timeline order. A 2D layer
///    sitting between two 3D layers in the timeline cannot be
///    reordered by any `z` value in the other bin — the bin boundary
///    is hard.
///
/// Three useful corollaries for authors and the renderer:
///
///   - **Adding `z = 0` to every layer is a visual no-op.** Perspective
///     scale at `z = 0` is `D/(D+0) = 1`, depth-sort ties resolve by
///     timeline order, so an all-`z=0` 3D bin renders byte-identically
///     to the same layers as 2D. Useful for migrating documents
///     incrementally.
///   - **Inserting a 2D layer between two 3D layers changes the
///     visible stack**, because it breaks the bin. This is AE-documented
///     behavior and the reason "adjustment layers" / "2D layers" surface
///     as render-order surprises in user comps.
///   - **The bin structure is a property of the document**, not of any
///     particular frame. Per-frame animation only moves layers *within*
///     their bin's depth-sort, never across bins; this is what lets the
///     `is_3d` check be data-only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export_to = "project_types.d.ts")]
pub enum Position {
    /// 2D layer position. Opts the layer **out** of the 3D bin: the
    /// layer acts as a bin breaker between contiguous 3D siblings.
    TwoD([f64; 2]),
    /// 3D layer position. Opts the layer **into** the 3D bin: the
    /// layer is depth-sorted with its contiguous 3D siblings before
    /// compositing. `z = 0` is a meaningful state (the camera plane).
    ThreeD([f64; 3]),
}

impl Position {
    /// Construct a 2D position.
    #[must_use]
    pub const fn xy(x: f64, y: f64) -> Self {
        Self::TwoD([x, y])
    }

    /// Construct a 3D position.
    #[must_use]
    pub const fn xyz(x: f64, y: f64, z: f64) -> Self {
        Self::ThreeD([x, y, z])
    }

    #[must_use]
    pub fn x(&self) -> f64 {
        match self {
            Self::TwoD([x, _]) | Self::ThreeD([x, _, _]) => *x,
        }
    }

    #[must_use]
    pub fn y(&self) -> f64 {
        match self {
            Self::TwoD([_, y]) | Self::ThreeD([_, y, _]) => *y,
        }
    }

    #[must_use]
    pub fn z(&self) -> Option<f64> {
        match self {
            Self::TwoD(_) => None,
            Self::ThreeD([_, _, z]) => Some(*z),
        }
    }

    #[must_use]
    pub fn z_or_zero(&self) -> f64 {
        self.z().unwrap_or(0.0)
    }

    #[must_use]
    pub fn is_3d(&self) -> bool {
        matches!(self, Self::ThreeD(_))
    }

    #[must_use]
    pub fn xy_array(&self) -> [f64; 2] {
        [self.x(), self.y()]
    }

    pub fn set_x(&mut self, x: f64) {
        match self {
            Self::TwoD([value, _]) | Self::ThreeD([value, _, _]) => *value = x,
        }
    }

    pub fn set_y(&mut self, y: f64) {
        match self {
            Self::TwoD([_, value]) | Self::ThreeD([_, value, _]) => *value = y,
        }
    }

    /// Set `z` for a structurally 3D position and return the previous value.
    pub fn try_set_z(&mut self, z: f64) -> Option<f64> {
        match self {
            Self::ThreeD([_, _, value]) => Some(std::mem::replace(value, z)),
            Self::TwoD(_) => None,
        }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::TwoD([0.0, 0.0])
    }
}

/// AE basic transform group.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct Transform {
    /// Layer-local pivot in pixels, measured from this layer's own content
    /// origin — never in canvas or parent coordinates. Its visual meaning
    /// depends on the layer kind, so derive it from measured local bounds
    /// rather than copying a desired placement into it. For point text,
    /// `[0, 0]` is the baseline start; for a Rect the visual center is
    /// `rect.position + rect.size / 2` — do not assume the origin is
    /// `[0, 0]`, since a panel size edit moves it to keep this pivot's
    /// fraction of the rectangle (see
    /// [`crate::layer_bounds::anchored_resize_origin`]).
    /// Put the parent-space spot where this pivot should land in `position`.
    pub anchor_point: Vector2Property,
    /// Parent-space location where `anchorPoint` lands; this is the field to
    /// use for canvas/group placement. Wire shape is `[x, y]` (2D layer) or
    /// `[x, y, z]` (3D layer). See [`Position`] for the full layer-index vs
    /// `z` reconciliation rules — this is the field that determines whether a
    /// layer joins the 3D bin.
    #[ts(type = "Array<number>")]
    pub position: Position,
    /// Scale percentage for x and y axes.
    pub scale: Vector2Property,
    /// In-plane (Z-axis) rotation in degrees. AE's plain "Rotation" for a 2D
    /// layer / "Z Rotation" for a 3D layer.
    pub rotation: ScalarProperty,
    /// In-plane shear angle in degrees (AE "Skew"). The renderer clamps the
    /// angle short of ±90° so the affine remains finite.
    #[serde(default)]
    pub skew: ScalarProperty,
    /// Orientation of the shear axis in degrees (AE "Skew Axis").
    #[serde(default)]
    pub skew_axis: ScalarProperty,
    /// Out-of-plane rotation about the X axis, in degrees (AE "X Rotation").
    /// Tilts the layer plane forward/back. Defaults to `0.0` so a document
    /// authored before 3D rotation existed deserializes unchanged and
    /// projects identically (no corner-pin warp is emitted at zero tilt).
    #[serde(default)]
    pub rotation_x: ScalarProperty,
    /// Out-of-plane rotation about the Y axis, in degrees (AE "Y Rotation").
    /// Swings the layer plane like a card flip. Defaults to `0.0`.
    #[serde(default)]
    pub rotation_y: ScalarProperty,
    /// Absolute 3D orientation `[x, y, z]` in degrees (AE "Orientation").
    /// Composes with the per-axis `rotation` / `rotation_x` / `rotation_y`
    /// deltas into the layer's total 3D rotation. Defaults to `[0, 0, 0]`.
    #[serde(default)]
    #[ts(type = "[number, number, number]")]
    pub orientation: Vector3Property,
    /// Opacity percentage in the inclusive range 0 to 100.
    pub opacity: PercentageProperty,
}

// `impl From<&Transform> for Affine` was removed when the perspective
// pipeline (`Transform::projected_affine`) became the single source of
// truth for the layer affine. Keeping a 2D-only `From` impl alongside
// the perspective-aware path created the obvious footgun: any
// `(&transform).into()` would silently strip the 3D foreshortening
// without a type error. A perspective-free affine is still trivially
// available for 2D transforms: `project_position` short-circuits to
// identity whenever `position.is_3d()` is false, so any
// `EvalContext::new(&[], w, h)` yields the plain 2D affine for a 2D
// transform regardless of comp dimensions.

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_shadow_color() -> ColorProperty {
    [0.0, 0.0, 0.0, 1.0]
}

pub(super) fn default_one() -> ScalarProperty {
    1.0
}

pub(super) fn default_hundred() -> ScalarProperty {
    100.0
}

pub(super) fn default_miter_limit() -> ScalarProperty {
    4.0
}

fn default_font_style() -> Arc<str> {
    Arc::from("Regular")
}

impl VideoSource {
    /// Build an asset-backed video source with the requested frame policy.
    #[must_use]
    pub fn from_asset(asset_id: AssetId, frame_rect: Option<PositiveRect>, fit: MediaFit) -> Self {
        Self {
            asset_id,
            eye_contact: None,
            audio_enhancement: None,
            input_transform: None,
            frame_rect,
            fit,
            time_remap: None,
        }
    }

    #[must_use]
    pub fn frame(&self) -> MediaFrame {
        self.frame_rect
            .map_or(MediaFrame::Natural, MediaFrame::Fixed)
    }

    #[must_use]
    pub fn frame_rect(&self) -> Option<RectBounds> {
        self.frame_rect.map(PositiveRect::get)
    }

    #[must_use]
    pub fn input_transform(&self) -> Option<&crate::InputTransform> {
        self.input_transform
            .as_ref()
            .and_then(crate::PersistedInputTransform::as_supported)
    }
}

impl ImageSource {
    /// Build an asset-backed still-image source with the requested frame policy.
    #[must_use]
    pub fn from_asset(asset_id: AssetId, frame_rect: Option<PositiveRect>, fit: MediaFit) -> Self {
        Self::Asset(ImageAssetSource {
            asset_id,
            input_transform: None,
            frame_rect,
            fit,
            time_remap: None,
        })
    }

    #[must_use]
    pub fn asset(&self) -> Option<&ImageAssetSource> {
        let Self::Asset(source) = self;
        Some(source)
    }

    #[must_use]
    pub fn asset_mut(&mut self) -> Option<&mut ImageAssetSource> {
        let Self::Asset(source) = self;
        Some(source)
    }

    #[must_use]
    pub fn input_transform(&self) -> Option<&crate::InputTransform> {
        self.asset()
            .and_then(|source| source.input_transform.as_ref())
            .and_then(crate::PersistedInputTransform::as_supported)
    }

    #[must_use]
    pub fn fit(&self) -> MediaFit {
        let Self::Asset(source) = self;
        source.fit
    }

    pub fn fit_mut(&mut self) -> &mut MediaFit {
        let Self::Asset(source) = self;
        &mut source.fit
    }

    #[must_use]
    pub fn frame_rect(&self) -> Option<RectBounds> {
        let Self::Asset(source) = self;
        source.frame_rect.map(PositiveRect::get)
    }

    pub fn frame_rect_mut(&mut self) -> &mut Option<PositiveRect> {
        let Self::Asset(source) = self;
        &mut source.frame_rect
    }

    #[must_use]
    pub fn frame(&self) -> MediaFrame {
        let Self::Asset(source) = self;
        source
            .frame_rect
            .map_or(MediaFrame::Natural, MediaFrame::Fixed)
    }

    /// Mutable frame and fit slots used by media-layout mutations.
    pub fn layout_fields_mut(&mut self) -> (&mut Option<PositiveRect>, &mut MediaFit) {
        let Self::Asset(source) = self;
        (&mut source.frame_rect, &mut source.fit)
    }
}

impl FontVariations {
    #[must_use]
    pub fn is_valid_tag(tag: &str) -> bool {
        let bytes = tag.as_bytes();
        bytes.len() == 4 && bytes.iter().all(|byte| (0x20..=0x7e).contains(byte))
    }

    #[must_use]
    pub fn tag_bytes(tag: &str) -> Option<[u8; 4]> {
        Self::is_valid_tag(tag)
            .then(|| tag.as_bytes().try_into().ok())
            .flatten()
    }

    #[must_use]
    pub fn id(&self) -> FxItemId {
        self.id
    }

    /// Remap this item's identity while assembling a candidate layer tree.
    /// Composition validation checks uniqueness and references before publication.
    pub fn remap_id(&mut self, id: FxItemId) {
        self.id = id;
    }

    #[must_use]
    pub fn axes(&self) -> &BTreeMap<String, ScalarProperty> {
        &self.axes
    }

    /// Mutable authored coordinate for an existing OpenType axis tag.
    pub fn axis_mut(&mut self, tag: &str) -> Option<&mut ScalarProperty> {
        self.axes.get_mut(tag)
    }
}
