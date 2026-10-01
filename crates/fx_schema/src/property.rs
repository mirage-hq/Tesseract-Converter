use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    ColorProperty, Duration, EffectId, FxItemId, LayerId, ShapePath, Time, TimeRange,
    Vector2Property,
};

/// Time interval value carried by the read-only range properties
/// ([`PropType::ActiveRange`] / [`PropType::SourceRange`]): a start point plus
/// a duration, both in the integer-millisecond wire format of
/// `time_types::Time` / `time_types::Duration`.
///
/// Deliberately distinct from [`crate::TimeRange`] (`{start, end}`): the
/// `{start, duration}` shape is the wire/product contract for animator inputs —
/// it matches how JS animators consume the value (`input.deps[i].value =
/// { start, duration }`, see the script input docs) and how the project model
/// expresses spans (`Timing` is start + duration, not start/end).
/// Hosts that want interval arithmetic should convert via the
/// [`From<TimeRangeProperty> for crate::TimeRange`] impl and use that type's
/// `contains` / `intersection` / `duration` helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct TimeRangeProperty {
    /// Interval start.
    pub start: Time,
    /// Interval length.
    pub duration: Duration,
}

impl TimeRangeProperty {
    /// Creates a range from a start point and a duration.
    #[must_use]
    pub const fn new(start: Time, duration: Duration) -> Self {
        Self { start, duration }
    }

    /// Exclusive interval end (`start + duration`), saturating.
    #[must_use]
    pub fn end(&self) -> Time {
        self.start.saturating_add(self.duration)
    }
}

impl From<TimeRangeProperty> for TimeRange {
    /// Converts to the half-open `{start, end}` interval type so hosts get
    /// `contains` / `intersection` / `duration` without hand-rolling the
    /// `start + duration` arithmetic (saturating, like [`TimeRangeProperty::end`]).
    fn from(range: TimeRangeProperty) -> Self {
        Self::new(range.start, range.end())
    }
}

/// Channel selector for audio-gain sampling.
///
/// Mirrors the three sliders After Effects' *Convert Audio to Keyframes*
/// produces (`Left Channel`, `Right Channel`, `Both Channels`). Returned by
/// [`PropType::audio_gain_channel`]; a derived-source host bridge can use it to
/// reduce the selected channel(s) to a scalar gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum AudioChannel {
    /// Left channel only.
    Left,
    /// Right channel only.
    Right,
    /// Both channels mixed (AE "Both Channels").
    #[default]
    Both,
}

/// Animatable layer property kinds understood by the composition animator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum PropType {
    /// Layer x-position in composition pixels.
    PositionX,
    /// Layer y-position in composition pixels.
    PositionY,
    /// Layer z-position in composition pixels. Only valid on 3D layers
    /// (layers whose `transform.position` is declared as `[x, y, z]`).
    /// Animating `PositionZ` on a 2D layer is rejected with
    /// [`LayerPropertyError::UnsupportedProperty`]: 3D-ness is a
    /// structural property of the document, not a per-frame value, so
    /// callers must opt the layer into 3D space by writing a
    /// 3-component `position` in the source document before attaching a
    /// `PositionZ` animator.
    PositionZ,
    /// Layer opacity as a percentage, `0..=100` (100 = fully opaque; values
    /// outside the range are rejected at apply time).
    Opacity,
    /// Layer rotation in degrees. This is the in-plane (Z-axis) rotation —
    /// AE's plain "Rotation" for a 2D layer / "Z Rotation" for a 3D layer.
    Rotation,
    /// Layer shear angle in degrees (AE "Skew"). Positive values shear along
    /// the axis selected by [`Self::SkewAxis`].
    Skew,
    /// Orientation of the layer shear axis in degrees (AE "Skew Axis").
    /// Has no visible effect while [`Self::Skew`] is zero.
    SkewAxis,
    /// Layer out-of-plane rotation about the X axis, in degrees (AE "X
    /// Rotation"). Tilts the layer plane forward/back. Valid on any layer
    /// (unlike `PositionZ`, out-of-plane tilt is not gated on a 3D
    /// `position`): a plain 2D layer with a non-zero `rotationX` renders as
    /// a card tilted through the AE-default implicit camera.
    RotationX,
    /// Layer out-of-plane rotation about the Y axis, in degrees (AE "Y
    /// Rotation"). Swings the layer plane like a card flip. See
    /// [`Self::RotationX`] for the (lack of) 3D gate.
    RotationY,
    /// Layer orientation X component, in degrees (AE "Orientation", first
    /// axis). Orientation is the absolute-pose companion to the per-axis
    /// `Rotation*` deltas; both compose into the layer's 3D rotation.
    OrientationX,
    /// Layer orientation Y component, in degrees (AE "Orientation", second
    /// axis). See [`Self::OrientationX`].
    OrientationY,
    /// Layer orientation Z component, in degrees (AE "Orientation", third
    /// axis). In-plane like [`Self::Rotation`]; both fold into the layer's
    /// Z rotation. See [`Self::OrientationX`].
    OrientationZ,
    /// Layer x-scale as a percentage (100 = unscaled).
    ScaleX,
    /// Layer y-scale as a percentage (100 = unscaled).
    ScaleY,
    /// Layer anchor-point x-coordinate in layer-local pixels (AE "Anchor
    /// Point", first component). The transform pivot: rotation, skew, and scale
    /// compose about it (`position + R·H·S·(p − anchor)`, see
    /// [`crate::Transform::projected_affine`]), so writing it moves the
    /// pivot without touching `position`. Layer-local and pre-scale like AE,
    /// which is why a `ScaleX`/`ScaleY` write never invalidates it
    /// (ENG-1503).
    AnchorPointX,
    /// Layer anchor-point y-coordinate in layer-local pixels. See
    /// [`Self::AnchorPointX`].
    AnchorPointY,
    /// Whether the layer's fill is painted — `Text.sourceText.applyFill`, the
    /// Rect `fillEnabled` flag, or the installed fill(s) of a Shape /
    /// BooleanOperation layer. The fill counterpart of
    /// [`Self::StrokeEnabled`], with the same presence semantics on the
    /// array-backed layer kinds: a `ShapeFillStyle` carries no enabled flag,
    /// so disabling clears `fills` and re-enabling installs a default
    /// solid-white fill (WEB-3146).
    FillEnabled,
    /// Primary fill color: `Text.sourceText.fillColor`, `Rect.rect.fillColor`,
    /// or the first solid fill of a Shape / BooleanOperation layer. Requires a
    /// fill to restyle on the array-backed layer kinds — install one first via
    /// [`Self::FillEnabled`], the same contract [`Self::StrokeColor`] has
    /// against [`Self::StrokeEnabled`].
    FillColor,
    /// Actual source text for text layers.
    TextContent,
    /// Text layer font family (`Text.sourceText.fontFamily`). Finite-range
    /// gated: only a constant `String` animator is allowed (a time-varying
    /// JS animator is rejected at graph construction) so font resources can
    /// be preloaded ahead of the first frame.
    FontFamily,
    /// Text layer font style / weight (`Text.sourceText.fontStyle`).
    /// Finite-range gated like [`Self::FontFamily`]: constant `String`
    /// animators only.
    FontStyle,
    /// Text layer font size in pixels (`Text.sourceText.fontSize`).
    FontSize,
    /// Text layer letter spacing in `1/1000 em` (AE Character-panel
    /// *Tracking*; `Text.sourceText.tracking`). Signed — negative values
    /// tighten. Figma `letterSpacing` in pixels converts as
    /// `tracking = letter_spacing_px / font_size_px * 1000`.
    Tracking,
    /// Text layer line height in pixels (AE Character-panel *Leading*;
    /// `Text.sourceText.leading`). Writing it replaces auto-leading with the
    /// explicit value (must be `> 0`). Figma `lineHeight` (PIXELS unit) maps
    /// directly.
    Leading,
    /// Whether the text is underlined (`Text.sourceText.underline`; Figma
    /// `textDecoration: UNDERLINE`). Boolean-animatable like
    /// [`Self::StrokeEnabled`].
    Underline,
    /// Whether the text is struck through (`Text.sourceText.strikethrough`;
    /// Figma `textDecoration: STRIKETHROUGH`). Boolean-animatable like
    /// [`Self::StrokeEnabled`].
    Strikethrough,
    /// Whether the text is rendered in all uppercase
    /// (`Text.sourceText.all_caps`; AE Character-panel *All Caps* / Figma
    /// *uppercase*, JRB-1528). Boolean-animatable like [`Self::Underline`] /
    /// [`Self::Strikethrough`]; the render lowering applies the Unicode-aware
    /// uppercase transform when set (see
    /// [`crate::layer::text::rendered_text`]).
    AllCaps,
    /// Live left-channel audio gain (`0.0..=1.0`) of the addressed
    /// [`crate::AudioLayer`]. Read-only derived property: never applied to a
    /// layer; sampled by the evaluator from the layer's source audio so other
    /// properties can depend on it (AE "Convert Audio to Keyframes → Left
    /// Channel" slider, made live).
    AudioGainLeft,
    /// Live right-channel audio gain of the addressed [`crate::AudioLayer`].
    AudioGainRight,
    /// Live both-channel (mixed) audio gain of the addressed [`crate::AudioLayer`].
    AudioGainBoth,
    /// Live average colour of the addressed [`crate::VideoLayer`]'s complete
    /// decoded frame. The authored media frame is layer geometry, not a source
    /// crop. This read-only derived property is never applied to a layer; the
    /// evaluator samples it so other properties can depend on it, such as an
    /// auto-contrast text colour over video. AE analogue: `sampleImage`
    /// averaged over the layer.
    MediaColor,
    /// Live perceptual luminance (`0.0..=1.0`, Rec.709 over linear light) of the
    /// addressed [`crate::VideoLayer`]'s current frame. Read-only derived
    /// property; the ergonomic scalar for "light background → dark text".
    MediaLuminance,
    /// Time span (`{start, duration}`, integer milliseconds —
    /// [`PropertyValue::TimeRange`] / [`TimeRangeProperty`]) on the
    /// composition timeline during which the addressed layer is visible /
    /// active. Read-only timeline fact ([`PropClass::ReadOnly`]): never
    /// applied to a layer by an animator; seeded by the evaluator from the
    /// layer model's persisted `activeRange` field so other properties can
    /// depend on a layer's lifetime (e.g. fade an overlay relative to a clip's
    /// in/out points). Valid on every layer type.
    ActiveRange,
    /// Time span (`{start, duration}`, integer milliseconds —
    /// [`PropertyValue::TimeRange`] / [`TimeRangeProperty`]) into the
    /// addressed layer's source media that backs its active span. Read-only
    /// timeline fact ([`PropClass::ReadOnly`]), seeded from the persisted
    /// `sourceRange` field like [`Self::ActiveRange`]. Only layers with
    /// time-based source media carry one: video media layers and audio layers
    /// ([`crate::FXComposition`] rejects a `SourceRange` dependency on any
    /// other layer type — on both the mutation path and deserialize — the
    /// same way `PositionZ` is rejected on 2D layers).
    SourceRange,
    /// Captions asset id backing a media layer's source
    /// (`Media.source.asset_id`). The animated value is a
    /// [`PropertyValue::String`] carrying the asset id to swap in.
    ///
    /// # Finite-range contract
    ///
    /// Unlike the numeric / color properties, this property gates project
    /// resource preloading: `Project::required_resources` (which feeds
    /// `ResourceProvider::load_all_required_resources`) must be able to
    /// statically enumerate every asset id any frame could request *before*
    /// the first frame renders. It therefore declares a finite value range
    /// ([`PropType::finite_range_value_kind`] →
    /// [`PropertyValueKind::String`]): the animation graph rejects any
    /// animator whose reachable output set it cannot enumerate ahead of time
    /// — an unbounded JavaScript animator
    /// ([`crate::AnimationGraphError::UnboundedAnimator`]) or a finite animator
    /// containing a value that is not a `String` asset id
    /// ([`crate::AnimationGraphError::NonEnumerableValue`]). Constant strings
    /// and finite string keyframe tracks survive, which lets
    /// [`crate::FXComposition::asset_refs`] union the static
    /// `Media.source.asset_id` with every reachable override.
    MediaSourceAssetId,
    /// Captions asset id backing an audio layer's source
    /// (`Audio.source.asset_id`). The animated value is a
    /// [`PropertyValue::String`] carrying the asset id to swap in.
    ///
    /// The audio counterpart of [`Self::MediaSourceAssetId`]: it overrides an
    /// [`crate::AudioLayer`] source (never a media/video source — a
    /// `MediaSourceAssetId` on an audio layer, or this on a media layer, is
    /// rejected at apply time). It is finite-range gated for the same reason
    /// (see [`PropType::finite_range_value_kind`] →
    /// [`PropertyValueKind::String`]): every reachable `String` override must
    /// be statically enumerable so [`crate::FXComposition::asset_refs`] can
    /// preload it as an [`crate::AssetKind::Audio`] asset ahead of the first
    /// frame.
    AudioSourceAssetId,
    /// Group padding above its child-derived content bounds, in pixels.
    PaddingTop,
    /// Group padding to the right of its child-derived content bounds, in pixels.
    PaddingRight,
    /// Group padding below its child-derived content bounds, in pixels.
    PaddingBottom,
    /// Group padding to the left of its child-derived content bounds, in pixels.
    PaddingLeft,
    /// Group top-left background corner radius in pixels. Does not clip children.
    CornerRadiusTopLeft,
    /// Group top-right background corner radius in pixels. Does not clip children.
    CornerRadiusTopRight,
    /// Group bottom-right background corner radius in pixels. Does not clip children.
    CornerRadiusBottomRight,
    /// Group bottom-left background corner radius in pixels. Does not clip children.
    CornerRadiusBottomLeft,
    /// Rectangle corner roundness in pixels (non-negative).
    RectRoundness,
    /// Rectangle `[width, height]` size, in layer pixels. Both components must
    /// be non-negative. Valid only on a [`crate::RectLayer`].
    RectSize,
    /// Whether the layer's stroke is rendered — Rect strokes, or the
    /// installed stroke(s) of a Shape / BooleanOperation layer.
    StrokeEnabled,
    /// Stroke color of a Rect, Shape, or BooleanOperation layer (RGBA,
    /// channels 0..1). Distinct from a text layer's glyph stroke
    /// (`sourceText.strokeColor`, a static field) and from the `stroke`
    /// layer-style entry (an `fxItemProperty` target).
    StrokeColor,
    /// Stroke width in pixels (non-negative) of a Rect, Shape, or
    /// BooleanOperation layer.
    StrokeWidth,
    /// Shape-layer stroke dash phase offset, in layer pixels (AE's *Stroke >
    /// Dashes > Offset*). Animating it slides the dash pattern along the path
    /// (the classic "marching ants" crawl). Valid only on a layer whose
    /// stroke carries a non-empty dash array — the array itself is a static
    /// field installed via the `strokeDashes` layer-field write, like the
    /// TrimPaths modifier that gates [`Self::TrimStart`] (JRB-1526).
    StrokeDashOffset,
    /// Shape / boolean-operation stroke line-join style, carried by
    /// [`crate::ShapeStrokeStyle::join`] as one of `"miter"` / `"round"` /
    /// `"bevel"`. A **string enum**, not a scalar: it is written through
    /// `updateLayerField` like the other static enum fields and is not driven
    /// by a time-varying animator (finite-range gated to
    /// [`PropertyValueKind::String`] exactly like the other string properties,
    /// so only a constant override is enumerable). Broadcasts to the installed
    /// stroke(s) of a Shape / BooleanOperation layer and rejects when the
    /// layer carries none. Unlike [`Self::StrokeColor`] /
    /// [`Self::StrokeWidth`] it does NOT apply to Rect layers — a Rect
    /// stroke has no join field (JRB-1622).
    StrokeJoin,
    /// Shape / boolean-operation stroke miter limit
    /// ([`crate::ShapeStrokeStyle::miter_limit`]), the sharp-corner cutoff for
    /// miter joins. Broadcasts to the installed stroke(s) and rejects when the
    /// layer carries none; like [`Self::StrokeJoin`] (and unlike
    /// [`Self::StrokeWidth`]) it does not apply to Rect layers (JRB-1622).
    StrokeMiterLimit,
    /// Shape-layer TrimPaths start, as a percentage (`0..=100`). Animating it
    /// (together with `TrimEnd`) produces the stroke "draw-on". Valid only on a
    /// [`crate::ShapeLayer`] whose `shape.trim` modifier is present.
    TrimStart,
    /// Shape-layer TrimPaths end, as a percentage (`0..=100`). See
    /// [`Self::TrimStart`].
    TrimEnd,
    /// Shape-layer TrimPaths offset, in degrees (`360` == one full loop). See
    /// [`Self::TrimStart`].
    TrimOffset,
    /// Shape-layer Round Corners radius, in layer pixels (`>= 0`, AE's `Round
    /// Corners` modifier). Writable and animatable like the other shape
    /// scalars. Unlike `TrimStart` (which requires the `shape.trim` modifier to
    /// already exist), applying this on a shape layer creates the
    /// `shape.round_corners` modifier on demand and a `0` radius clears it —
    /// absent and a `0` radius render identically (JRB-1479 / JRB-1216).
    RoundCornersRadius,
    /// Shape-layer Offset Paths amount, in layer pixels (positive = expand,
    /// negative = contract; AE's `Offset Paths` modifier, JRB-1529). Like
    /// `TrimStart` (and unlike `RoundCornersRadius`), valid only on a
    /// [`crate::ShapeLayer`] whose `shape.offset_paths` modifier is present —
    /// the modifier's `line_join` / `miter_limit` live on the installed
    /// struct, so an amount write cannot create it on demand without
    /// silently defaulting them.
    OffsetPathsAmount,
    /// Whether a layer drop shadow is rendered.
    DropShadowEnabled,
    /// Layer drop shadow color.
    DropShadowColor,
    /// Layer drop shadow offset in pixels.
    DropShadowOffset,
    /// Layer drop shadow blur radius in pixels.
    DropShadowBlurRadius,
    /// Layer drop shadow spread radius in pixels.
    DropShadowSpreadRadius,
    /// Linear audio gain ("AE Audio Levels", `1.0` = unity) of the addressed
    /// [`crate::AudioLayer`] or asset-backed video [`crate::MediaLayer`].
    /// Writable scalar: unlike the read-only
    /// `AudioGain*` derived sources (which *sample* the waveform for visual
    /// reactivity), this is the user-set output gain applied to the layer's
    /// audio. Persisted as [`crate::AudioLayer::volume`]; animating it (e.g. a
    /// fade-in/out) drives a time-varying gain in the audio mixer. A media
    /// layer persists it as optional `MediaLayer.volume`; omission disables
    /// embedded audio for backward compatibility, unless an `AudioVolume`
    /// animator targets the layer — authoring one opts the layer in with a
    /// silent base gain. Non-negative
    /// ([`PropClass::Writable`]); valid only on audio layers and video media
    /// layers.
    ///
    /// Named `AudioVolume` for parity with the other audio-only properties
    /// (`AudioGain*`, `AudioSourceAssetId`), but serialized as `"volume"`: the
    /// editor's volume sliders submit `updateLayerField` with that name
    /// (JRB-1287), so the wire string stays `volume`.
    #[serde(rename = "volume")]
    AudioVolume,
    /// Shape-layer PolyStar vertex count (AE's `Polystar Path` "Points", JRB-1215).
    /// Animating it grows / shrinks the star or polygon; clamped to `3..=1000`
    /// at lowering. Valid only on a [`crate::ShapeLayer`] whose
    /// `shape.poly_star` generator is present.
    PolyStarPoints,
    /// Shape-layer PolyStar center, in layer-local pixels. See
    /// [`Self::PolyStarPoints`].
    PolyStarPosition,
    /// Shape-layer PolyStar rotation in degrees. See [`Self::PolyStarPoints`].
    PolyStarRotation,
    /// Shape-layer PolyStar outer radius in layer pixels. See
    /// [`Self::PolyStarPoints`].
    PolyStarOuterRadius,
    /// Shape-layer PolyStar inner radius in layer pixels (ignored for the
    /// polygon type). See [`Self::PolyStarPoints`].
    PolyStarInnerRadius,
    /// Shape-layer PolyStar outer-corner roundness as a percentage (`0..=100`).
    /// See [`Self::PolyStarPoints`].
    PolyStarOuterRoundness,
    /// Shape-layer PolyStar inner-corner roundness as a percentage (`0..=100`,
    /// ignored for the polygon type). See [`Self::PolyStarPoints`].
    PolyStarInnerRoundness,
    /// Shape-layer Ellipse `[width, height]` size, in layer pixels (AE's
    /// `Ellipse Path` "Size", JRB-1530). Valid only on a
    /// [`crate::ShapeLayer`] whose `shape.ellipse` generator is present,
    /// mirroring the [`Self::PolyStarPoints`] pre-existence rule.
    EllipseSize,
    /// Shape-layer Ellipse center, in layer-local pixels. See
    /// [`Self::EllipseSize`].
    EllipsePosition,
    /// Shape-layer outline geometry (`Shape.shape.path`), as a
    /// [`PropertyValue::Path`]. Native path keyframes interpolate the outline
    /// per frame; scripts can also return a complete path at each time.
    /// Because path masks and
    /// text path options resolve their geometry through a shape-layer
    /// reference (ENG-1471), this is also how a mask outline or a
    /// text-on-path guide is animated. Valid only on a [`crate::ShapeLayer`].
    ShapePath,
}

/// How a [`PropType`] participates in evaluation: a normal *writable* layer
/// property, a read-only *derived source*, or a plain *read-only* timeline
/// fact — each non-writable class with its neutral default.
///
/// This is the **single source of truth** for the writable / non-writable
/// split and the neutral default — the facts the evaluation core needs and
/// the ones whose hand-synced duplication used to drift. `is_read_only`,
/// `is_derived_source`, `read_only_default`, and the `apply_*` reject are all
/// thin views over [`PropType::classify`]'s exhaustive match, so adding a new
/// `PropType` variant fails to compile until it is classified here.
/// `LAYER_PROPERTY_DEPS` (the hand-maintained writable list in `layer.rs`) is
/// not derived from `classify` directly, but the
/// `layer_property_deps_matches_classify` test asserts it equals the set of
/// `Writable` variants, so the two cannot drift.
///
/// `DerivedSource` vs `ReadOnly`: both are non-animatable ambient inputs the
/// evaluator seeds before the topological walk (no producing node, no graph
/// edge). The split is *what the value is*: a `DerivedSource` is **sampled from
/// media content** (PCM gain, frame colour), while a `ReadOnly` property is a
/// **structural timeline fact** (a layer's active span, its source-media span)
/// stored on the layer model.
///
/// Deliberately *not* carried here: the audio channel an `AudioGain*` property
/// samples. That is a host-audio-domain detail consumed only by the audio
/// resolver, so it stays in [`PropType::audio_gain_channel`] rather than
/// leaking into the kind-agnostic classification.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PropClass {
    /// A property written onto a layer by `apply_animated_property`.
    Writable,
    /// A read-only value **sampled from media content** with no producing
    /// animator node, seeded at evaluation time by a host
    /// [`crate::DerivedSourceProvider`].
    DerivedSource {
        /// Neutral value used when the host cannot resolve the source.
        default: PropertyValue,
    },
    /// A read-only **structural timeline fact** (no media sampling involved)
    /// with no producing animator node, seeded from persisted layer fields.
    ReadOnly,
}

/// The range a **static** (authored) write to one [`PropType`] must satisfy.
///
/// Returned by [`PropType::static_write_bounds`]; the whole table is shipped to
/// hosts by [`PropType::static_write_bounds_table`].
///
/// For a `Vector2`-valued property (e.g. [`PropType::EllipseSize`]) the bounds
/// apply **per component** — there is no whole-vector magnitude bound.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct PropBounds {
    /// Lower bound, inclusive unless [`Self::exclusive_min`] is set. `None`
    /// means the engine imposes no floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub min: Option<f64>,
    /// Inclusive upper bound. `None` means the engine imposes no ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max: Option<f64>,
    /// When set, [`Self::min`] is a *strict* floor (`value > min`) rather than
    /// `value >= min`.
    pub exclusive_min: bool,
    /// When set, the value must be non-zero with its **sign left free** — the
    /// bound a scale factor carries, since JRB-1670 flips a layer by writing a
    /// negative scale. Not expressible as a `min`/`max` interval, hence its own
    /// flag.
    pub exclude_zero: bool,
}

impl PropBounds {
    /// Non-zero, sign free.
    const NON_ZERO: Self = Self {
        min: None,
        max: None,
        exclusive_min: false,
        exclude_zero: true,
    };

    /// Non-negative (`value >= 0`), no ceiling.
    const NON_NEGATIVE: Self = Self {
        min: Some(0.0),
        max: None,
        exclusive_min: false,
        exclude_zero: false,
    };

    /// Strictly positive (`value > 0`), no ceiling.
    const POSITIVE: Self = Self {
        min: Some(0.0),
        max: None,
        exclusive_min: true,
        exclude_zero: false,
    };
}

/// The one authoritative list of static-write bounds, keyed by property.
///
/// Every consumer reads it through [`PropType::static_write_bounds`] (one
/// property) or [`PropType::static_write_bounds_table`] (the whole map, shipped
/// to the editor over the `fxPropertyBounds` wasm export) so a panel input can
/// never offer a value the engine will reject.
///
/// Deliberately sparse — a property absent from this list has no static-write
/// bound. In particular [`PropType::PolyStarInnerRadius`] is **not** listed: a
/// zero inner radius still renders visible spikes, so it is a legitimate
/// authored value, unlike a zero outer radius which collapses the star to a
/// point. Likewise the scale axes carry no magnitude cap — the engine happily
/// renders an arbitrarily large scale; the ±1000% the editor offers is an
/// advisory UI range, not an engine bound.
const STATIC_WRITE_BOUNDS: &[(PropType, PropBounds)] = &[
    (PropType::ScaleX, PropBounds::NON_ZERO),
    (PropType::PaddingTop, PropBounds::NON_NEGATIVE),
    (PropType::PaddingRight, PropBounds::NON_NEGATIVE),
    (PropType::PaddingBottom, PropBounds::NON_NEGATIVE),
    (PropType::PaddingLeft, PropBounds::NON_NEGATIVE),
    (PropType::CornerRadiusTopLeft, PropBounds::NON_NEGATIVE),
    (PropType::CornerRadiusTopRight, PropBounds::NON_NEGATIVE),
    (PropType::CornerRadiusBottomRight, PropBounds::NON_NEGATIVE),
    (PropType::CornerRadiusBottomLeft, PropBounds::NON_NEGATIVE),
    (PropType::ScaleY, PropBounds::NON_ZERO),
    (PropType::EllipseSize, PropBounds::POSITIVE),
    (PropType::PolyStarOuterRadius, PropBounds::POSITIVE),
];

impl PropType {
    /// Parse a wire property name into its [`PropType`] without an
    /// owned-`Value` round-trip — the serde rename IS the wire spelling
    /// (pinned by the `display_matches_serde_wire_name` test below).
    #[must_use]
    pub fn from_wire(property_name: &str) -> Option<Self> {
        use serde::Deserialize as _;
        Self::deserialize(
            serde::de::value::StrDeserializer::<serde::de::value::Error>::new(property_name),
        )
        .ok()
    }

    /// Classifies this property as writable, derived-source, or read-only.
    /// The exhaustive match is the one place the split and each derived-source
    /// neutral default live; the predicates below are thin views over it.
    pub(crate) fn classify(self) -> PropClass {
        match self {
            // Silence for audio gain; for the media sources a dark, fully
            // transparent colour / zero luminance, so an unloaded video reads
            // as "no contribution" and luminance-driven text defaults to its
            // light variant rather than failing evaluation.
            Self::AudioGainLeft | Self::AudioGainRight | Self::AudioGainBoth => {
                PropClass::DerivedSource {
                    default: PropertyValue::Float(0.0),
                }
            }
            Self::MediaLuminance => PropClass::DerivedSource {
                default: PropertyValue::Float(0.0),
            },
            Self::MediaColor => PropClass::DerivedSource {
                default: PropertyValue::Color([0.0, 0.0, 0.0, 0.0]),
            },
            // Structural timeline facts, not media samples: read-only. They
            // must resolve from the persisted layer model.
            Self::ActiveRange | Self::SourceRange => PropClass::ReadOnly,
            // Writable layer properties. Listed explicitly (no wildcard) so a
            // new variant must be classified here before it compiles.
            Self::PositionX
            | Self::PositionY
            | Self::PositionZ
            | Self::Opacity
            | Self::Rotation
            | Self::Skew
            | Self::SkewAxis
            | Self::RotationX
            | Self::RotationY
            | Self::OrientationX
            | Self::OrientationY
            | Self::OrientationZ
            | Self::ScaleX
            | Self::ScaleY
            | Self::AnchorPointX
            | Self::AnchorPointY
            | Self::FillEnabled
            | Self::FillColor
            | Self::TextContent
            | Self::FontFamily
            | Self::FontStyle
            | Self::FontSize
            | Self::Tracking
            | Self::Leading
            | Self::Underline
            | Self::Strikethrough
            | Self::AllCaps
            | Self::MediaSourceAssetId
            | Self::AudioSourceAssetId
            | Self::PaddingTop
            | Self::PaddingRight
            | Self::PaddingBottom
            | Self::PaddingLeft
            | Self::CornerRadiusTopLeft
            | Self::CornerRadiusTopRight
            | Self::CornerRadiusBottomRight
            | Self::CornerRadiusBottomLeft
            | Self::RectRoundness
            | Self::RectSize
            | Self::StrokeEnabled
            | Self::StrokeColor
            | Self::StrokeWidth
            | Self::StrokeDashOffset
            | Self::StrokeJoin
            | Self::StrokeMiterLimit
            | Self::TrimStart
            | Self::TrimEnd
            | Self::TrimOffset
            | Self::RoundCornersRadius
            | Self::OffsetPathsAmount
            | Self::DropShadowEnabled
            | Self::DropShadowColor
            | Self::DropShadowOffset
            | Self::DropShadowBlurRadius
            | Self::DropShadowSpreadRadius
            | Self::AudioVolume
            | Self::PolyStarPoints
            | Self::PolyStarPosition
            | Self::PolyStarRotation
            | Self::PolyStarOuterRadius
            | Self::PolyStarInnerRadius
            | Self::PolyStarOuterRoundness
            | Self::PolyStarInnerRoundness
            | Self::EllipseSize
            | Self::EllipsePosition
            | Self::ShapePath => PropClass::Writable,
        }
    }

    /// The smallest kind in sort order — the first declared variant, since
    /// `Ord` is derived. The lower bound of one layer's contiguous target
    /// range in a value map keyed by [`PropertyTarget`]. Pinned against every
    /// variant by `layer_property_deps_matches_classify`.
    pub const MIN: Self = Self::PositionX;

    /// True for every non-writable property — both *derived source*
    /// ([`PropClass::DerivedSource`]: media-sampled `AudioGain*`,
    /// `Media{Color,Luminance}`) and plain *read-only*
    /// ([`PropClass::ReadOnly`]: timeline facts `ActiveRange` /
    /// `SourceRange`). These have no producing animator node; the evaluator
    /// seeds any dependency on one as an ambient input (no graph node, no
    /// edge, seeded before the topological walk), and animator write paths
    /// reject them. Static layer-field mutations may still persist model-backed
    /// timeline facts (`ActiveRange` / `SourceRange`).
    #[must_use]
    pub fn is_read_only(self) -> bool {
        match self.classify() {
            PropClass::DerivedSource { .. } | PropClass::ReadOnly => true,
            PropClass::Writable => false,
        }
    }

    /// True for read-only *derived source* properties specifically: values
    /// **sampled from media content** by the host (today the
    /// `AudioGain{Left,Right,Both}` properties sampled from a layer's waveform,
    /// and the `Media{Color,Luminance}` properties sampled from a frame). A
    /// subset of [`Self::is_read_only`]; the structural timeline facts
    /// (`ActiveRange` / `SourceRange`) are read-only but not derived sources.
    #[must_use]
    pub fn is_derived_source(self) -> bool {
        matches!(self.classify(), PropClass::DerivedSource { .. })
    }

    /// The value a host-sampled derived source takes when it cannot be
    /// resolved. Persisted timeline facts do not have a synthetic fallback.
    /// Returns `None` for writable properties and model-backed read-only
    /// properties.
    #[must_use]
    pub fn read_only_default(self) -> Option<PropertyValue> {
        match self.classify() {
            PropClass::DerivedSource { default } => Some(default),
            PropClass::ReadOnly | PropClass::Writable => None,
        }
    }

    /// The [`AudioChannel`] an `AudioGain*` property samples, or `None` for
    /// every other property.
    ///
    /// Kept separate from [`Self::classify`] on purpose: the channel is an
    /// audio-domain fact consumed only by the host audio resolver, not by the
    /// kind-agnostic evaluation core.
    #[must_use]
    pub fn audio_gain_channel(self) -> Option<AudioChannel> {
        match self {
            Self::AudioGainLeft => Some(AudioChannel::Left),
            Self::AudioGainRight => Some(AudioChannel::Right),
            Self::AudioGainBoth => Some(AudioChannel::Both),
            _ => None,
        }
    }

    /// The range a **static** (authored) write to this property must satisfy, or
    /// `None` when the engine imposes none beyond finiteness.
    ///
    /// This is the **single source of truth** for those bounds. The
    /// authoring-boundary validators in `project_mutation` (the
    /// `updateLayerField` and `setFxLayerTransform` routes) enforce them, and
    /// the editor's property panel derives its input min/max from the same
    /// table via [`Self::static_write_bounds_table`], so a panel input can
    /// never offer a value the engine will reject (JRB-1731).
    ///
    /// Bounds deliberately do **not** apply to per-frame animator evaluation:
    /// an animated value legitimately passes through zero (a scale pop-in, an
    /// ellipse that animates shut), so the shared apply path
    /// (`Layer::apply_animated_property`) only requires finiteness. It is the
    /// *static* write that persists a degenerate layer — invisible on canvas
    /// with no handle left to grab it back.
    ///
    /// For a `Vector2` property the returned bounds apply per component.
    #[must_use]
    pub fn static_write_bounds(self) -> Option<PropBounds> {
        // A linear scan over this small table beats any map here, and it keeps the
        // table a plain `const` slice that both views read. If the list ever
        // grows past a handful, switch to an exhaustive `match self` (compiled
        // to a jump table) rather than adding a lazily-built map.
        STATIC_WRITE_BOUNDS
            .iter()
            .find(|(property_type, _)| *property_type == self)
            .map(|(_, bounds)| *bounds)
    }

    /// Every property that carries static-write bounds, keyed by property.
    ///
    /// The host-facing view of [`Self::static_write_bounds`] — both read the one
    /// authoritative list, so shipping the table to the editor cannot drift
    /// from what the mutation layer enforces.
    #[must_use]
    pub fn static_write_bounds_table() -> BTreeMap<Self, PropBounds> {
        STATIC_WRITE_BOUNDS.iter().copied().collect()
    }

    /// If this property gates an ahead-of-time precomputation — its reachable
    /// value set must be *statically enumerable* before the first frame — the
    /// [`PropertyValueKind`] every reachable value must carry; otherwise
    /// `None`.
    ///
    /// This is the **single source of truth** for which properties are
    /// finite-range gated and the value kind they enumerate. The animation
    /// graph (`ensure_enumerable_animator`) reads it to reject animators whose
    /// output range it cannot enumerate ahead of time (an unbounded JavaScript
    /// animator) or whose enumerated values are
    /// the wrong kind, so the precomputed set can never be surprised by a
    /// frame.
    ///
    /// Today five properties are gated, all to
    /// [`PropertyValueKind::String`]: [`Self::MediaSourceAssetId`] and
    /// [`Self::AudioSourceAssetId`] drive resource preloading through
    /// [`crate::FXComposition::asset_refs`], which must see every asset id
    /// any frame could request ahead of
    /// `ResourceProvider::load_all_required_resources`; [`Self::FontFamily`]
    /// and [`Self::FontStyle`] likewise feed font preloading via
    /// `text_font_refs`; and [`Self::StrokeJoin`] is a string enum gated
    /// purely to forbid time-varying animators. A future property that
    /// feeds any other finite precomputation opts in by adding an arm here —
    /// no change to the graph gate is needed.
    #[must_use]
    pub const fn finite_range_value_kind(self) -> Option<PropertyValueKind> {
        match self {
            Self::FontFamily
            | Self::FontStyle
            | Self::MediaSourceAssetId
            | Self::AudioSourceAssetId
            // A string enum ("miter"/"round"/"bevel"): finite-range gated so
            // only a statically-enumerable constant override is allowed (no
            // time-varying JS animator), exactly like the string font fields.
            // It does NOT feed resource preloading — `asset_refs` /
            // `text_font_refs` match their own explicit `PropType`s, not the
            // finite-range set — so gating it here only constrains its
            // animators (JRB-1622).
            | Self::StrokeJoin => Some(PropertyValueKind::String),
            _ => None,
        }
    }
}

impl fmt::Display for PropType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::PositionX => "positionX",
            Self::PositionY => "positionY",
            Self::PositionZ => "positionZ",
            Self::Opacity => "opacity",
            Self::Rotation => "rotation",
            Self::Skew => "skew",
            Self::SkewAxis => "skewAxis",
            Self::RotationX => "rotationX",
            Self::RotationY => "rotationY",
            Self::OrientationX => "orientationX",
            Self::OrientationY => "orientationY",
            Self::OrientationZ => "orientationZ",
            Self::ScaleX => "scaleX",
            Self::ScaleY => "scaleY",
            Self::AnchorPointX => "anchorPointX",
            Self::AnchorPointY => "anchorPointY",
            Self::FillEnabled => "fillEnabled",
            Self::FillColor => "fillColor",
            Self::TextContent => "textContent",
            Self::FontFamily => "fontFamily",
            Self::FontStyle => "fontStyle",
            Self::FontSize => "fontSize",
            Self::Tracking => "tracking",
            Self::Leading => "leading",
            Self::Underline => "underline",
            Self::Strikethrough => "strikethrough",
            Self::AllCaps => "allCaps",
            Self::AudioGainLeft => "audioGainLeft",
            Self::AudioGainRight => "audioGainRight",
            Self::AudioGainBoth => "audioGainBoth",
            Self::MediaColor => "mediaColor",
            Self::MediaLuminance => "mediaLuminance",
            Self::ActiveRange => "activeRange",
            Self::SourceRange => "sourceRange",
            Self::MediaSourceAssetId => "mediaSourceAssetId",
            Self::AudioSourceAssetId => "audioSourceAssetId",
            Self::PaddingTop => "paddingTop",
            Self::PaddingRight => "paddingRight",
            Self::PaddingBottom => "paddingBottom",
            Self::PaddingLeft => "paddingLeft",
            Self::CornerRadiusTopLeft => "cornerRadiusTopLeft",
            Self::CornerRadiusTopRight => "cornerRadiusTopRight",
            Self::CornerRadiusBottomRight => "cornerRadiusBottomRight",
            Self::CornerRadiusBottomLeft => "cornerRadiusBottomLeft",
            Self::RectRoundness => "rectRoundness",
            Self::RectSize => "rectSize",
            Self::StrokeEnabled => "strokeEnabled",
            Self::StrokeColor => "strokeColor",
            Self::StrokeWidth => "strokeWidth",
            Self::StrokeDashOffset => "strokeDashOffset",
            Self::StrokeJoin => "strokeJoin",
            Self::StrokeMiterLimit => "strokeMiterLimit",
            Self::TrimStart => "trimStart",
            Self::TrimEnd => "trimEnd",
            Self::TrimOffset => "trimOffset",
            Self::RoundCornersRadius => "roundCornersRadius",
            Self::OffsetPathsAmount => "offsetPathsAmount",
            Self::DropShadowEnabled => "dropShadowEnabled",
            Self::DropShadowColor => "dropShadowColor",
            Self::DropShadowOffset => "dropShadowOffset",
            Self::DropShadowBlurRadius => "dropShadowBlurRadius",
            Self::DropShadowSpreadRadius => "dropShadowSpreadRadius",
            Self::AudioVolume => "volume",
            Self::PolyStarPoints => "polyStarPoints",
            Self::PolyStarPosition => "polyStarPosition",
            Self::PolyStarRotation => "polyStarRotation",
            Self::PolyStarOuterRadius => "polyStarOuterRadius",
            Self::PolyStarInnerRadius => "polyStarInnerRadius",
            Self::PolyStarOuterRoundness => "polyStarOuterRoundness",
            Self::PolyStarInnerRoundness => "polyStarInnerRoundness",
            Self::EllipseSize => "ellipseSize",
            Self::EllipsePosition => "ellipsePosition",
            Self::ShapePath => "shapePath",
        };
        f.write_str(name)
    }
}

/// Stable address for a **fixed** animatable layer property — one of the closed
/// [`PropType`] set (position, opacity, stroke, …). The dynamic effect-param
/// namespace is addressed separately by [`EffectParamTarget`]; both are
/// unified as graph keys by [`PropertyTarget`]. `Property` stays `Copy` so the
/// fixed path (the overwhelming majority of animated values) keeps a cheap key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct Property {
    /// Id of the layer that owns the property, anywhere in the composition
    /// tree.
    layer_id: LayerId,
    /// Which fixed layer property is addressed (see [`PropType`] for the
    /// closed set and each property's units).
    property_type: PropType,
}

impl Property {
    /// Creates a property address for a fixed `property_type` on `layer_id`.
    #[must_use]
    pub fn new(layer_id: LayerId, property_type: PropType) -> Self {
        Self {
            layer_id,
            property_type,
        }
    }

    /// Returns the layer that owns this property.
    #[must_use]
    pub fn layer_id(&self) -> LayerId {
        self.layer_id
    }

    /// Returns the animated property kind.
    #[must_use]
    pub fn property_type(&self) -> PropType {
        self.property_type
    }
}

impl fmt::Display for Property {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "layer {} {}", self.layer_id, self.property_type)
    }
}

/// Address of one param of an effect, keyed by the effect's stable
/// [`EffectId`] and the param **name**.
///
/// No `layer_id` and no stack index: the [`EffectId`] is unique within the
/// composition, so the owning layer and the effect are both derivable from it,
/// and the address survives stack reorders / removals / cross-layer moves with
/// no fixup. The param stays name-keyed so a reordered/edited param list
/// re-resolves at apply time. Always drives a writable `Float` scalar — it
/// carries no `PropType` and never participates in the read-only / finite-range
/// classification (ENG-1280 / JRB-1211).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct EffectParamTarget {
    /// Stable id of the owning `EffectInstance` in the layer's `effects`
    /// stack.
    effect_id: EffectId,
    /// The effect's parameter name — the camelCase field name on the effect
    /// variant (e.g. `blurriness`, `glowRadius`), or a custom shader's
    /// declared param name.
    param_name: String,
}

impl EffectParamTarget {
    /// Creates an effect-param address from a stable effect id + param name.
    #[must_use]
    pub fn new(effect_id: EffectId, param_name: impl Into<String>) -> Self {
        Self {
            effect_id,
            param_name: param_name.into(),
        }
    }

    /// The stable id of the addressed effect.
    #[must_use]
    pub fn effect_id(&self) -> EffectId {
        self.effect_id
    }

    /// The addressed param's name.
    #[must_use]
    pub fn param_name(&self) -> &str {
        &self.param_name
    }
}

/// Address of one property of a stacked FX sub-item (a path mask; future
/// stacked layer styles), keyed by the item's stable [`FxItemId`] and the
/// property **name**.
///
/// Mirrors [`EffectParamTarget`]'s discipline: no `layer_id` and no kind tag —
/// the [`FxItemId`] is unique within the composition, so the owning layer and
/// the item kind are both derivable from it at apply time, and the address
/// survives stack reorders / removals / cross-layer moves with no fixup. The
/// property stays name-keyed so the per-kind property namespace can grow
/// without touching this address type (JRB-1373).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct FxItemTarget {
    /// Stable fx-item id of the stacked sub-item: a `layerStyles` entry, a
    /// path mask, a text animator or selector, or a text `pathOptions`.
    item_id: FxItemId,
    /// The sub-item's property name — the camelCase field name on the item
    /// (e.g. `size` on a layer style, `start` on a range selector,
    /// `firstMargin` on path options).
    property_name: String,
}

impl FxItemTarget {
    /// Creates an fx-item property address from a stable item id + property
    /// name.
    #[must_use]
    pub fn new(item_id: FxItemId, property_name: impl Into<String>) -> Self {
        Self {
            item_id,
            property_name: property_name.into(),
        }
    }

    /// The stable id of the addressed FX sub-item.
    #[must_use]
    pub fn item_id(&self) -> FxItemId {
        self.item_id
    }

    /// The addressed property's name.
    #[must_use]
    pub fn property_name(&self) -> &str {
        &self.property_name
    }
}

/// What the animation graph drives: a fixed layer [`Property`] or a name-keyed
/// effect param ([`EffectParamTarget`]).
///
/// The two variants are distinct *property targets*: a fixed, closed-set
/// [`PropType`] property of a layer, and a param of an effect (addressed by the
/// effect's stable [`EffectId`] — an effect is **not** a layer, it is a member
/// of a layer's effect stack). This enum is the **extension point**: future
/// dynamic namespaces (mask params, shape-toolkit params, text-selector params)
/// become new variants, each with its own typed address — mirroring After
/// Effects' per-layer property groups (ENG-1280 / JRB-1211).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum PropertyTarget {
    /// A fixed, closed-set layer property (transform, fill, stroke, …).
    /// Serializes as `kind: "layer"`.
    #[serde(rename = "layer")]
    LayerProperty(Property),
    /// A param of an effect, keyed by the effect's stable id.
    EffectProperty(EffectParamTarget),
    /// A property of a stacked FX sub-item (path mask / stacked layer style),
    /// keyed by the item's stable [`FxItemId`] (JRB-1373).
    FxItemProperty(FxItemTarget),
}

impl PropertyTarget {
    /// A fixed-property target: `property_type` on `layer_id`.
    #[must_use]
    pub fn layer(layer_id: LayerId, property_type: PropType) -> Self {
        Self::LayerProperty(Property::new(layer_id, property_type))
    }

    /// An effect-param target: the param named `param_name` of the effect with
    /// stable id `effect_id`.
    #[must_use]
    pub fn effect_param(effect_id: EffectId, param_name: impl Into<String>) -> Self {
        Self::EffectProperty(EffectParamTarget::new(effect_id, param_name))
    }

    /// An fx-item property target: the property named `property_name` of the
    /// stacked sub-item with stable id `item_id`.
    #[must_use]
    pub fn fx_item(item_id: FxItemId, property_name: impl Into<String>) -> Self {
        Self::FxItemProperty(FxItemTarget::new(item_id, property_name))
    }

    /// The owning layer for a fixed-property target, or `None` for an effect
    /// param / fx-item property (whose layer is derived from its stable id at
    /// apply time, not stored on the address).
    #[must_use]
    pub fn layer_id(&self) -> Option<LayerId> {
        match self {
            Self::LayerProperty(property) => Some(property.layer_id()),
            Self::EffectProperty(_) | Self::FxItemProperty(_) => None,
        }
    }

    /// The addressed effect's id, for an effect-param target.
    #[must_use]
    pub fn effect_id(&self) -> Option<EffectId> {
        match self {
            Self::EffectProperty(target) => Some(target.effect_id()),
            Self::LayerProperty(_) | Self::FxItemProperty(_) => None,
        }
    }

    /// The addressed FX sub-item's id, for an fx-item property target.
    #[must_use]
    pub fn fx_item_id(&self) -> Option<FxItemId> {
        match self {
            Self::FxItemProperty(target) => Some(target.item_id()),
            Self::LayerProperty(_) | Self::EffectProperty(_) => None,
        }
    }

    /// The fixed [`Property`] this target addresses, or `None` for an effect
    /// param. The classification gates ([`PropType::is_read_only`],
    /// [`PropType::finite_range_value_kind`]) only ever apply to a fixed
    /// property, so they route through this.
    #[must_use]
    pub fn as_property(&self) -> Option<Property> {
        match self {
            Self::LayerProperty(property) => Some(*property),
            Self::EffectProperty(_) | Self::FxItemProperty(_) => None,
        }
    }
}

impl From<Property> for PropertyTarget {
    fn from(property: Property) -> Self {
        Self::LayerProperty(property)
    }
}

impl From<EffectParamTarget> for PropertyTarget {
    fn from(target: EffectParamTarget) -> Self {
        Self::EffectProperty(target)
    }
}

impl From<FxItemTarget> for PropertyTarget {
    fn from(target: FxItemTarget) -> Self {
        Self::FxItemProperty(target)
    }
}

impl fmt::Display for PropertyTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LayerProperty(property) => write!(f, "{property}"),
            Self::EffectProperty(target) => {
                write!(f, "effect {} {}", target.effect_id(), target.param_name())
            }
            Self::FxItemProperty(target) => {
                write!(f, "fxItem {} {}", target.item_id(), target.property_name())
            }
        }
    }
}

/// Evaluated value produced by an animator node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum PropertyValue {
    /// Portable signed integer value for serialized graph data.
    Integer(i64),
    /// Floating-point value for continuous animation data.
    Float(f64),
    /// Two-dimensional value for vector properties such as position.
    Vector2(Vector2Property),
    /// RGBA color value for color properties.
    Color(ColorProperty),
    /// Text value for string properties.
    String(String),
    /// Boolean value for toggle properties.
    Bool(bool),
    /// Time interval (start + duration) for the read-only range properties
    /// ([`PropType::ActiveRange`] / [`PropType::SourceRange`]).
    TimeRange(TimeRangeProperty),
    /// Bezier path geometry for path-valued fx-item properties (a mask path
    /// morph). Emitted per frame by a JS animator — deliberately **not**
    /// keyframed, so there is no vertex-count-matching constraint (JRB-1373).
    Path(ShapePath),
}

/// The discriminant of a [`PropertyValue`], without its payload.
///
/// Lets a finite-range property declare the value kind it enumerates
/// ([`PropType::finite_range_value_kind`]) and lets the animation graph check
/// an animator's reachable values against it without naming a concrete value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyValueKind {
    /// [`PropertyValue::Integer`].
    Integer,
    /// [`PropertyValue::Float`].
    Float,
    /// [`PropertyValue::Vector2`].
    Vector2,
    /// [`PropertyValue::Color`].
    Color,
    /// [`PropertyValue::String`].
    String,
    /// [`PropertyValue::Bool`].
    Bool,
    /// [`PropertyValue::TimeRange`].
    TimeRange,
    /// [`PropertyValue::Path`].
    Path,
}

impl PropertyValueKind {
    /// camelCase label for diagnostics (matches the serde tag of
    /// [`PropertyValue`]).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Vector2 => "vector2",
            Self::Color => "color",
            Self::String => "string",
            Self::Bool => "bool",
            Self::TimeRange => "timeRange",
            Self::Path => "path",
        }
    }
}

impl PropertyValue {
    /// This value's [`PropertyValueKind`] discriminant.
    #[must_use]
    pub const fn kind(&self) -> PropertyValueKind {
        match self {
            Self::Integer(_) => PropertyValueKind::Integer,
            Self::Float(_) => PropertyValueKind::Float,
            Self::Vector2(_) => PropertyValueKind::Vector2,
            Self::Color(_) => PropertyValueKind::Color,
            Self::String(_) => PropertyValueKind::String,
            Self::Bool(_) => PropertyValueKind::Bool,
            Self::TimeRange(_) => PropertyValueKind::TimeRange,
            Self::Path(_) => PropertyValueKind::Path,
        }
    }

    /// True unless this value carries a non-finite (NaN / ±∞) number in **any**
    /// component. Covers `Float`, `Vector2`, and `Color`; non-numeric variants
    /// (`Integer`, `String`, `Bool`) are always finite.
    ///
    /// Used to reject a non-finite derived-source value before it is seeded
    /// into the evaluation map. Checking every numeric component (not just
    /// `Float`) matters now that derived sources can be `Color` (`MediaColor`):
    /// a NaN channel must fall back to the neutral default, not propagate.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        match self {
            Self::Float(value) => value.is_finite(),
            Self::Vector2(components) => components.iter().all(|c| c.is_finite()),
            Self::Color(channels) => channels.iter().all(|c| c.is_finite()),
            Self::Path(path) => path.is_finite(),
            // Integer-millisecond intervals can never carry NaN / ±∞.
            Self::Integer(_) | Self::String(_) | Self::Bool(_) | Self::TimeRange(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table and the per-property lookup must never diverge — both read
    /// `STATIC_WRITE_BOUNDS`, and this pins that they stay two views of one
    /// list (JRB-1731). Also pins the enrolled set, so removing a bound the
    /// editor's panel inputs depend on is a deliberate, visible change.
    #[test]
    fn static_write_bounds_table_matches_the_per_property_lookup() {
        let table = PropType::static_write_bounds_table();

        // Pinned by sorted wire name, not by iteration order: `BTreeMap` keys on
        // the derived `Ord` (the discriminant), so asserting the map's own order
        // would couple this test to `PropType`'s declaration order and break on
        // any unrelated variant reshuffle.
        let mut enrolled = table.keys().map(ToString::to_string).collect::<Vec<_>>();
        enrolled.sort();
        assert_eq!(
            enrolled,
            [
                "cornerRadiusBottomLeft",
                "cornerRadiusBottomRight",
                "cornerRadiusTopLeft",
                "cornerRadiusTopRight",
                "ellipseSize",
                "paddingBottom",
                "paddingLeft",
                "paddingRight",
                "paddingTop",
                "polyStarOuterRadius",
                "scaleX",
                "scaleY",
            ]
        );
        for (property_type, bounds) in &table {
            assert_eq!(
                property_type.static_write_bounds().as_ref(),
                Some(bounds),
                "table and lookup diverged for {property_type:?}",
            );
        }

        // A zero inner radius still renders spikes, so it is deliberately
        // unbounded — the panel must keep offering it.
        assert_eq!(PropType::PolyStarInnerRadius.static_write_bounds(), None);
        // No magnitude cap on scale: the ±1000% the editor offers is advisory.
        let scale = PropType::ScaleX
            .static_write_bounds()
            .expect("scaleX carries bounds");
        assert!(scale.exclude_zero);
        assert_eq!((scale.min, scale.max), (None, None));
    }

    #[test]
    fn property_display_names_layer_and_camel_case_property() {
        let property = Property::new(LayerId::new(42), PropType::DropShadowBlurRadius);

        assert_eq!(property.to_string(), "layer 42 dropShadowBlurRadius");
        assert_eq!(PropType::PositionX.to_string(), "positionX");
    }

    /// `Display` doubles as the wire-name producer for hosts that build
    /// `updateLayerField` writes (the ENG-1474 tilt gesture emits
    /// `PropType::RotationX.to_string()`), while the parser side round-trips
    /// the string through serde. Pin the two hand-maintained mappings to
    /// each other so a rename can't silently split them.
    #[test]
    fn display_matches_serde_wire_name() {
        for prop in [
            PropType::PositionX,
            PropType::PositionY,
            PropType::Rotation,
            PropType::Skew,
            PropType::SkewAxis,
            PropType::RotationX,
            PropType::RotationY,
            PropType::OrientationX,
            PropType::OrientationY,
            PropType::OrientationZ,
            PropType::ScaleX,
            PropType::ScaleY,
            PropType::AnchorPointX,
            PropType::AnchorPointY,
            PropType::Opacity,
            PropType::StrokeJoin,
            PropType::StrokeMiterLimit,
        ] {
            assert_eq!(
                serde_json::to_value(prop).unwrap(),
                serde_json::Value::String(prop.to_string()),
                "Display and serde wire name diverged for {prop:?}",
            );
        }
    }

    #[test]
    fn fx_item_target_round_trips_with_camel_case_kind_tag() {
        let target = PropertyTarget::fx_item(FxItemId::new(9), "feather");

        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "fxItemProperty",
                "itemId": 9,
                "propertyName": "feather",
            }),
        );
        assert_eq!(
            serde_json::from_value::<PropertyTarget>(json).unwrap(),
            target,
        );
        assert_eq!(target.fx_item_id(), Some(FxItemId::new(9)));
        assert_eq!(target.layer_id(), None);
        assert_eq!(target.effect_id(), None);
        assert_eq!(target.as_property(), None);
        assert_eq!(target.to_string(), "fxItem 9 feather");
    }

    #[test]
    fn path_value_round_trips_with_shape_path_wire_shape() {
        let path: ShapePath = serde_json::from_value(serde_json::json!({
            "commands": [
                { "type": "moveTo", "x": 0.0, "y": 0.0 },
                { "type": "cubicTo", "c1x": 1.0, "c1y": 2.0, "c2x": 3.0, "c2y": 4.0, "x": 5.0, "y": 6.0 },
                { "type": "close" },
            ],
        }))
        .unwrap();
        let value = PropertyValue::Path(path);

        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["type"], "path");
        assert_eq!(json["value"]["commands"][0]["type"], "moveTo");
        assert_eq!(
            serde_json::from_value::<PropertyValue>(json).unwrap(),
            value
        );
        assert!(value.is_finite());
    }

    #[test]
    fn classify_views_agree_for_derived_read_only_and_writable() {
        // The predicates are thin views over `classify`; check they stay
        // consistent so a future refactor can't let them drift apart.
        //
        // Media-sampled derived sources: read-only AND derived.
        for prop in [
            PropType::AudioGainLeft,
            PropType::AudioGainRight,
            PropType::AudioGainBoth,
            PropType::MediaColor,
            PropType::MediaLuminance,
        ] {
            assert!(
                prop.is_derived_source(),
                "{prop} should be a derived source"
            );
            assert!(prop.is_read_only(), "{prop} should be read-only");
            assert!(
                prop.read_only_default().is_some(),
                "{prop} must have a synthetic read-only default"
            );
        }
        // Structural timeline facts: read-only but NOT derived sources.
        for prop in [PropType::ActiveRange, PropType::SourceRange] {
            assert!(prop.is_read_only(), "{prop} should be read-only");
            assert!(
                !prop.is_derived_source(),
                "{prop} is a timeline fact, not a media-sampled derived source"
            );
            assert_eq!(
                prop.read_only_default(),
                None,
                "{prop} must not have a synthetic read-only default"
            );
        }
        for prop in [
            PropType::Opacity,
            PropType::FontFamily,
            PropType::FontStyle,
            PropType::FontSize,
            PropType::FillColor,
            PropType::TextContent,
        ] {
            assert!(!prop.is_read_only(), "{prop} should be writable");
            assert!(!prop.is_derived_source(), "{prop} should be writable");
            assert_eq!(
                prop.read_only_default(),
                None,
                "{prop} must not have a read-only default"
            );
            assert_eq!(prop.audio_gain_channel(), None);
        }
    }

    #[test]
    fn audio_gain_channel_only_for_audio_gain() {
        assert_eq!(
            PropType::AudioGainLeft.audio_gain_channel(),
            Some(AudioChannel::Left)
        );
        assert_eq!(
            PropType::AudioGainRight.audio_gain_channel(),
            Some(AudioChannel::Right)
        );
        assert_eq!(
            PropType::AudioGainBoth.audio_gain_channel(),
            Some(AudioChannel::Both)
        );
        // A non-audio derived source has a default but no channel — the channel
        // is an audio-only fact, not part of the generic classification.
        assert!(PropType::MediaColor.is_derived_source());
        assert_eq!(PropType::MediaColor.audio_gain_channel(), None);
    }

    #[test]
    fn finite_range_gate_is_keyed_on_property_not_hardcoded() {
        // Only properties that feed ahead-of-time resource preloading are
        // finite-range gated today. String font fields enumerate font refs;
        // asset-id fields enumerate asset refs. Every other property opts out
        // (`None`), so the graph gate never restricts their animators.
        assert_eq!(
            PropType::FontFamily.finite_range_value_kind(),
            Some(PropertyValueKind::String)
        );
        assert_eq!(
            PropType::FontStyle.finite_range_value_kind(),
            Some(PropertyValueKind::String)
        );
        assert_eq!(
            PropType::MediaSourceAssetId.finite_range_value_kind(),
            Some(PropertyValueKind::String)
        );
        assert_eq!(
            PropType::AudioSourceAssetId.finite_range_value_kind(),
            Some(PropertyValueKind::String)
        );
        for prop in [
            PropType::PositionX,
            PropType::Opacity,
            PropType::FillColor,
            PropType::TextContent,
            PropType::FontSize,
            PropType::AudioGainBoth,
            PropType::ActiveRange,
            PropType::SourceRange,
        ] {
            assert_eq!(
                prop.finite_range_value_kind(),
                None,
                "{prop} must not be finite-range gated"
            );
        }
    }

    #[test]
    fn property_value_kind_matches_variant() {
        assert_eq!(PropertyValue::Integer(1).kind(), PropertyValueKind::Integer);
        assert_eq!(PropertyValue::Float(1.0).kind(), PropertyValueKind::Float);
        assert_eq!(
            PropertyValue::Vector2([0.0, 0.0]).kind(),
            PropertyValueKind::Vector2
        );
        assert_eq!(
            PropertyValue::Color([0.0, 0.0, 0.0, 1.0]).kind(),
            PropertyValueKind::Color
        );
        assert_eq!(
            PropertyValue::String("x".into()).kind(),
            PropertyValueKind::String
        );
        assert_eq!(PropertyValue::Bool(true).kind(), PropertyValueKind::Bool);
        assert_eq!(
            PropertyValue::TimeRange(TimeRangeProperty::new(Time::ZERO, Duration::from_millis(1),))
                .kind(),
            PropertyValueKind::TimeRange
        );
        assert_eq!(
            PropertyValue::Path(ShapePath {
                commands: Vec::new()
            })
            .kind(),
            PropertyValueKind::Path
        );
    }

    #[test]
    fn time_range_value_round_trips_in_millisecond_wire_format() {
        let value = PropertyValue::TimeRange(TimeRangeProperty::new(
            Time::from_millis(1500),
            Duration::from_millis(2500),
        ));
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "timeRange",
                "value": { "start": 1500, "duration": 2500 }
            })
        );
        let reparsed: PropertyValue = serde_json::from_value(json).unwrap();
        assert_eq!(reparsed, value);

        let range = TimeRangeProperty::new(Time::from_millis(1500), Duration::from_millis(2500));
        assert_eq!(range.end(), Time::from_millis(4000));
    }

    #[test]
    fn is_finite_checks_every_numeric_component() {
        assert!(PropertyValue::Float(1.0).is_finite());
        assert!(!PropertyValue::Float(f64::NAN).is_finite());
        assert!(PropertyValue::Color([0.0, 0.5, 1.0, 1.0]).is_finite());
        // A single NaN channel makes the whole colour non-finite — the case the
        // old `Float`-only guard let through for `MediaColor`.
        assert!(!PropertyValue::Color([0.0, f64::NAN, 0.0, 1.0]).is_finite());
        assert!(!PropertyValue::Vector2([f64::INFINITY, 0.0]).is_finite());
        // Non-numeric variants are always finite.
        assert!(PropertyValue::String("x".into()).is_finite());
        assert!(PropertyValue::Bool(true).is_finite());
        assert!(PropertyValue::TimeRange(TimeRangeProperty::new(
            Time::ZERO,
            Duration::from_millis(1),
        ))
        .is_finite());
        // A single NaN coordinate in any command makes the whole path
        // non-finite — JSON cannot carry NaN, so this gate is what stops a
        // script-side NaN from seeding the evaluation map.
        assert!(PropertyValue::Path(ShapePath {
            commands: vec![crate::ShapePathCommand::MoveTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            }],
        })
        .is_finite());
        assert!(!PropertyValue::Path(ShapePath {
            commands: vec![
                crate::ShapePathCommand::MoveTo {
                    x: 0.0,
                    y: 0.0,
                    mirror: None,
                    corner_radius: None,
                },
                crate::ShapePathCommand::CubicTo {
                    c1x: 1.0,
                    c1y: f64::NAN,
                    c2x: 2.0,
                    c2y: 3.0,
                    x: 4.0,
                    y: 5.0,
                    mirror: None,
                    corner_radius: None,
                },
            ],
        })
        .is_finite());
    }
}
