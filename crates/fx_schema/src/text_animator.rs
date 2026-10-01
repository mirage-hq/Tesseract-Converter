//! Persisted text-animator and selector records.
//!
//! Records carry caller-assigned identities, selection parameters and base
//! property values. They describe data only; selector and text evaluation are
//! not part of this crate.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ColorProperty, FxItemId, ScalarProperty, Vector2Property};

/// Shape of a range selector's falloff curve.
///
/// Values match AE's *Shape* dropdown and libpag `TextRangeSelectorShape`
/// (0-indexed): Square, Ramp Up, Ramp Down, Triangle, Round, Smooth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SelectorShape {
    #[default]
    Square,
    RampUp,
    RampDown,
    Triangle,
    Round,
    Smooth,
}

/// Unit in which a range selector's `start` / `end` / `offset` are expressed.
///
/// AE's *Units* dropdown. `Percentage` reads them as a `[0, 1]` fraction of the
/// unit count; `Index` reads them as absolute unit indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SelectorUnits {
    #[default]
    Percentage,
    Index,
}

/// The unit a range selector partitions the string into (AE's *Based On*).
///
/// Every character inherits the selector weight of the unit it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SelectorBasis {
    #[default]
    Characters,
    CharactersExcludingSpaces,
    Words,
    Lines,
}

/// How a selector combines with the running per-character weight of the
/// selectors before it (AE's *Mode*; libpag `TextSelectorMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SelectorMode {
    #[default]
    Add,
    Subtract,
    Intersect,
    Min,
    Max,
    Difference,
}

/// A text range selector — picks which characters an animator affects and with
/// what per-character weight.
///
/// Field semantics mirror AE's *Range Selector* and libpag's `TextRangeSelector`
/// (tag 71). `start` / `end` / `offset` are read according to [`Self::units`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RangeSelector {
    /// Stable, caller-minted identity for graph addressing of this selector's
    /// animatable scalars (`start`/`end`/`offset`/`amount`/`easeHigh`/`easeLow`/
    /// `randomSeed`) via `PropertyTarget::FxItemProperty`.
    pub id: FxItemId,
    /// Range start. `[0, 1]` fraction of the unit count in `Percentage` units,
    /// absolute unit index in `Index` units.
    #[serde(default)]
    pub start: ScalarProperty,
    /// Range end. Defaults to `1.0` (the whole string in `Percentage` units).
    #[serde(default = "default_one")]
    pub end: ScalarProperty,
    /// Amount added to both `start` and `end` — the knob AE presets keyframe to
    /// produce a reveal.
    #[serde(default)]
    pub offset: ScalarProperty,
    #[serde(default)]
    pub units: SelectorUnits,
    #[serde(default)]
    pub based_on: SelectorBasis,
    #[serde(default)]
    pub mode: SelectorMode,
    /// Selector strength (`1.0` = 100%). Multiplies every per-character weight.
    #[serde(default = "default_one")]
    pub amount: ScalarProperty,
    #[serde(default)]
    pub shape: SelectorShape,
    /// Ease into the high (selected) end — Triangle shape only, `[-1, 1]`.
    #[serde(default)]
    pub ease_high: ScalarProperty,
    /// Ease into the low (deselected) end — Triangle shape only, `[-1, 1]`.
    #[serde(default)]
    pub ease_low: ScalarProperty,
    /// Randomize which unit each selector position maps to (AE *Randomize
    /// Order*), using a deterministic MT19937 permutation seeded by
    /// [`Self::random_seed`].
    #[serde(default)]
    pub randomize_order: bool,
    /// Seed for [`Self::randomize_order`]. libpag stores it as `uint16_t`.
    #[serde(default)]
    pub random_seed: ScalarProperty,
}

impl Default for RangeSelector {
    fn default() -> Self {
        Self {
            id: FxItemId::new(0),
            start: 0.0,
            end: 1.0,
            offset: 0.0,
            units: SelectorUnits::Percentage,
            based_on: SelectorBasis::Characters,
            mode: SelectorMode::Add,
            amount: 1.0,
            shape: SelectorShape::Square,
            ease_high: 0.0,
            ease_low: 0.0,
            randomize_order: false,
            random_seed: 0.0,
        }
    }
}

/// A text wiggly selector (AE *Wiggly Selector*) — adds random, time-varying
/// per-character variation on top of the animator's range selectors.
///
/// Field semantics mirror libpag's `TextWigglySelector`: an organic cosine
/// oscillation seeded per character, mapped to a `[0, 1]` weight scaled by
/// [`Self::amount`]. Unlike a [`RangeSelector`], its output depends on time, so
/// an animator carrying one re-evaluates every frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WigglySelector {
    /// Stable, caller-minted identity for graph addressing of this selector's
    /// animatable scalars (`speed` / `amount` / `seed`) via
    /// `PropertyTarget::FxItemProperty`.
    pub id: FxItemId,
    /// How this selector combines with the ones before it (AE *Mode*).
    #[serde(default)]
    pub mode: SelectorMode,
    /// Wiggles per second (AE *Wiggles/Second*; libpag default `2.0`).
    #[serde(default = "default_wiggle_speed")]
    pub speed: ScalarProperty,
    /// Selector strength in percent (`100` = full; AE *Max/Min Amount*).
    #[serde(default = "default_hundred")]
    pub amount: ScalarProperty,
    /// Random seed for the per-character phase offset (AE *Random Seed*).
    #[serde(default)]
    pub seed: ScalarProperty,
}

impl Default for WigglySelector {
    fn default() -> Self {
        Self {
            id: FxItemId::new(0),
            mode: SelectorMode::Add,
            speed: 2.0,
            amount: 100.0,
            seed: 0.0,
        }
    }
}

impl WigglySelector {
    /// The full set of graph-animatable scalar names on a [`WigglySelector`].
    pub const ANIMATABLE_PROPERTIES: &'static [&'static str] = &["speed", "amount", "seed"];
}

/// A text animator — combines range selectors with per-character properties.
///
/// The `Option` properties mirror AE's *Add Property* menu: a property is only
/// applied when present. Scale is a percentage (`[100, 100]` = identity),
/// rotation is in degrees, opacity is a percentage (`100` = opaque), position
/// is a pixel offset.
///
/// Anchor Point / Skew / Skew Axis (JRB-1461) are wired through the per-glyph
/// affine below. Tracking (JRB-1495), Stroke Width (JRB-1493), and Blur
/// (JRB-1494) are realized renderer-side and all three render: Tracking
/// reflows glyph advance, Stroke Width adjusts the per-glyph stroke, and Blur
/// runs each glyph through the separable-gaussian pipeline — all carried on
/// `scene::CharAnimation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TextAnimator {
    /// Stable, caller-minted identity for graph addressing of this animator's
    /// per-character properties via `PropertyTarget::FxItemProperty`.
    pub id: FxItemId,
    /// Optional user-visible name for the text animator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Range selectors, combined in order by each selector's
    /// [`RangeSelector::mode`]. No selectors ⇒ every character fully selected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selectors: Vec<RangeSelector>,
    /// Wiggly selectors (AE *Wiggly Selector*), folded into the combined
    /// per-character weight after the range selectors, in order by each
    /// selector's [`WigglySelector::mode`] (JRB-1218).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wiggly_selectors: Vec<WigglySelector>,
    /// Per-character position offset in layer pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Vector2Property>,
    /// Per-character anchor-point offset in layer pixels (AE *Anchor Point*).
    /// Shifts the pivot rotation / skew are applied around; `[0, 0]` = identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_point: Option<Vector2Property>,
    /// Per-character scale in percent (`[100, 100]` = identity). Blends
    /// multiplicatively from identity by selector weight (unlike the
    /// additive position/rotation/tracking accumulation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Vector2Property>,
    /// Per-character rotation in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<ScalarProperty>,
    /// Per-character shear angle in degrees (AE *Skew*).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skew: Option<ScalarProperty>,
    /// Axis the shear is applied along, in degrees (AE *Skew Axis*). Only has an
    /// effect together with a non-zero [`Self::skew`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skew_axis: Option<ScalarProperty>,
    /// Per-character tracking in `1/1000 em` (AE *Tracking*) — extra
    /// inter-character advance that reflows every following glyph on the line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking: Option<ScalarProperty>,
    /// Additive per-character stroke-width delta in pixels (AE *Stroke Width*).
    /// Only visible where the glyph has a stroke color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<ScalarProperty>,
    /// Per-character gaussian blur `(x, y)` radius in pixels (AE *Blur*).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur: Option<Vector2Property>,
    /// Per-character opacity in percent (`100` = fully opaque). Blends
    /// multiplicatively from opaque by selector weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<ScalarProperty>,
    /// Per-character fill color override (RGBA `[0, 1]`). A hard override,
    /// not a blend: applied only where the combined selector weight exceeds
    /// 0.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<ColorProperty>,
    /// Per-character stroke color override (RGBA `[0, 1]`). A hard override
    /// like [`Self::fill_color`]: applied only where the combined selector
    /// weight exceeds 0.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<ColorProperty>,
    /// Extra inter-line leading in pixels (AE *Line Spacing*).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<ScalarProperty>,
    /// Per-line horizontal anchor as a percentage (AE *Line Anchor*; `0` = left,
    /// `50` = center, `100` = right). Authorable / animatable; its render effect
    /// is coupled to per-character advance (Tracking, JRB-1495).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_anchor: Option<ScalarProperty>,
    /// Shift each character's value by this many code points within its class
    /// (AE *Character Offset*, "Preserve Case & Digits": A–Z, a–z, 0–9 cycle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_offset: Option<ScalarProperty>,
    /// Replace each character with this code point (AE *Character Value*),
    /// interpolated from the source by the selector weight and constrained to
    /// the source character's class ("Preserve Case & Digits").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_value: Option<ScalarProperty>,
}

impl Default for TextAnimator {
    /// An empty animator with a placeholder id — the same convention
    /// [`RangeSelector::default`] follows (`FxItemId` has no `Default`, so this
    /// is hand-written rather than derived), so tests and builders can spell out
    /// only the properties they care about. Real animators get a caller-minted
    /// [`FxItemId`].
    fn default() -> Self {
        Self {
            id: FxItemId::new(0),
            name: String::new(),
            selectors: Vec::new(),
            wiggly_selectors: Vec::new(),
            position: None,
            anchor_point: None,
            scale: None,
            rotation: None,
            skew: None,
            skew_axis: None,
            tracking: None,
            stroke_width: None,
            blur: None,
            opacity: None,
            fill_color: None,
            stroke_color: None,
            line_spacing: None,
            line_anchor: None,
            character_offset: None,
            character_value: None,
        }
    }
}

impl TextAnimator {
    /// The full set of graph-animatable property names on a [`TextAnimator`].
    pub const ANIMATABLE_PROPERTIES: &'static [&'static str] = &[
        "position",
        "anchorPoint",
        "scale",
        "rotation",
        "skew",
        "skewAxis",
        "tracking",
        "strokeWidth",
        "blur",
        "opacity",
        "fillColor",
        "strokeColor",
        "lineSpacing",
        "lineAnchor",
        "characterOffset",
        "characterValue",
    ];
}

impl RangeSelector {
    /// The full set of graph-animatable scalar names on a [`RangeSelector`].
    pub const ANIMATABLE_PROPERTIES: &'static [&'static str] = &[
        "start",
        "end",
        "offset",
        "amount",
        "easeHigh",
        "easeLow",
        "randomSeed",
    ];
}

fn default_one() -> ScalarProperty {
    1.0
}

fn default_hundred() -> ScalarProperty {
    100.0
}

fn default_wiggle_speed() -> ScalarProperty {
    2.0
}
