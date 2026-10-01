//! Canonical persisted layer-style schema.
use super::{
    model::{default_shadow_color, default_true},
    BlendMode, ShapeGradientStop, ShapeGradientType,
};
use crate::{ColorProperty, FxItemId, NonNegativeProperty, ScalarProperty, Vector2Property};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Canonical legacy layer-style field and variant declarations shared by readers.
#[doc(hidden)]
#[macro_export]
macro_rules! define_layer_styles_schema {
    () => {
        /// Drop-shadow payload shared by [`crate::LayerEffect::DropShadow`] and the
        /// legacy inline/style input DTOs. Runtime storage is always the ordered
        /// effect stack; legacy inputs migrate once at the composition boundary.
        ///
        /// Inline `dropShadow*` layer addresses and stacked `fxItemProperty` addresses
        /// remain compatibility aliases. Legacy writeback preserves their original
        /// ordering and gates; it cannot represent an animated stacked `enabled` gate.
        /// Ordered documents address the same payload through a stable effect id.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct DropShadow {
            /// Whether the shadow is rendered.
            #[serde(default = "default_true")]
            pub enabled: bool,
            /// RGBA shadow color.
            #[serde(default = "default_shadow_color")]
            pub color: ColorProperty,
            /// Screen-space shadow offset in pixels.
            #[serde(default)]
            pub offset: Vector2Property,
            /// Blur radius in pixels.
            #[serde(default)]
            pub blur_radius: NonNegativeProperty,
            /// Hard silhouette dilation radius in pixels before blur.
            #[serde(default)]
            pub spread_radius: NonNegativeProperty,
            /// Shadow compositing mode.
            #[serde(default)]
            pub blend_mode: BlendMode,
        }

        /// Identity-bearing entry in the legacy `layerStyles` wire projection.
        /// The composition-unique [`FxItemId`] remains a stable compatibility alias
        /// after migration into [`crate::EffectInstance`]. Historical animator
        /// addresses therefore survive inserts, removals, and reorders.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct LayerStyleEntry {
            /// Stable identity of this style instance (composition-unique).
            pub id: FxItemId,
            /// The style payload.
            pub style: LayerStyle,
        }

        /// Historical tagged style data, retained without conversion to ordered effects.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(tag = "type", rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum LayerStyle {
            /// Outer glow around the layer's rendered alpha silhouette.
            OuterGlow(OuterGlowStyle),
            /// Stroke outline around the layer's rendered alpha silhouette.
            Stroke(StrokeOutlineStyle),
            /// Gradient fill composited over the layer's content.
            GradientOverlay(GradientOverlayStyle),
            /// Inner shadow cast onto the *inside* of the layer's alpha silhouette —
            /// the inverted sibling of Drop Shadow (JRB-1468). Lowers to
            /// [`scene::NodeEffect::InnerShadow`].
            InnerShadow(InnerShadowStyle),
            /// Inner glow radiating inward from the layer's alpha edge — the inverted
            /// sibling of Outer Glow (JRB-1468). Lowers to
            /// [`scene::NodeEffect::InnerGlow`].
            InnerGlow(InnerGlowStyle),
            /// Satin: soft folded sheen from contour interference of the alpha
            /// (JRB-1468). Lowers to [`scene::NodeEffect::Satin`].
            Satin(SatinStyle),
            /// Bevel & Emboss: fake directional lighting of the alpha edge slope
            /// (JRB-1468). Lowers to [`scene::NodeEffect::BevelEmboss`].
            BevelEmboss(BevelEmbossStyle),
            /// Drop shadow cast behind the layer's alpha silhouette — the stacked
            /// representation of the layer's inline [`DropShadow`] field, sharing its
            /// payload and lowering (JRB-1781 P2). Lowers to
            /// [`scene::NodeEffect::DropShadow`].
            DropShadow(DropShadow),
        }

        /// Persisted outer-glow controls.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct OuterGlowStyle {
            /// Whether the glow is rendered.
            #[serde(default = "default_true")]
            pub enabled: bool,
            /// RGBA glow color (alpha carries the glow opacity, like [`DropShadow`]).
            #[serde(default = "default_glow_color")]
            pub color: ColorProperty,
            /// Glow size in pixels at full intrinsic scale (libpag's authored `size`).
            #[serde(default)]
            pub size: NonNegativeProperty,
            /// Hard-dilation share of `size` (0..=1) before the blur.
            #[serde(default)]
            pub spread: ScalarProperty,
            /// libpag's authored `range` parameter (typically 0.5..=1.0); the
            /// renderer owns the divide-by-zero clamp.
            #[serde(default = "default_glow_range")]
            pub range: ScalarProperty,
            /// Glow compositing mode.
            #[serde(default)]
            pub blend_mode: BlendMode,
        }

        /// Stroke outline layer style. Named to avoid a clash with the shape-toolkit
        /// [`ShapeStrokeStyle`] (a vector-path stroke, not a silhouette outline).
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct StrokeOutlineStyle {
            /// Whether the stroke is rendered.
            #[serde(default = "default_true")]
            enabled: bool,
            /// RGBA stroke color (alpha carries the stroke opacity).
            #[serde(default = "default_stroke_outline_color")]
            pub(crate) color: ColorProperty,
            /// Stroke width in pixels.
            #[serde(default)]
            pub(crate) width: NonNegativeProperty,
            /// Where the stroke sits relative to the silhouette edge.
            #[serde(default)]
            position: LayerStrokePosition,
            /// Stroke compositing mode.
            #[serde(default)]
            blend_mode: BlendMode,
        }

        /// Where a stroke-outline style sits relative to the silhouette edge.
        /// Mirrors [`scene::StrokePosition`] one-to-one.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum LayerStrokePosition {
            /// Stroke grows outward from the edge (AE default).
            #[default]
            Outside,
            /// Stroke straddles the edge.
            Center,
            /// Stroke grows inward from the edge.
            Inside,
        }

        /// Gradient overlay layer style: a gradient fill composited over the layer's
        /// content between two explicit points in layer-local pixels (the PAG
        /// evaluator derives the same two points from angle/scale/offset).
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct GradientOverlayStyle {
            /// Whether the overlay is rendered.
            #[serde(default = "default_true")]
            enabled: bool,
            /// Overlay opacity (0..=1).
            #[serde(default = "default_gradient_overlay_opacity")]
            pub(crate) opacity: ScalarProperty,
            /// Gradient shape.
            #[serde(default)]
            gradient_type: ShapeGradientType,
            /// Gradient start point in layer-local pixels.
            #[serde(default)]
            start: Vector2Property,
            /// Gradient end point in layer-local pixels.
            #[serde(default)]
            end: Vector2Property,
            /// Ordered color stops. An empty list renders nothing.
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            stops: Vec<ShapeGradientStop>,
            /// Overlay compositing mode.
            #[serde(default)]
            blend_mode: BlendMode,
        }

        /// Inner shadow layer style: the inverted sibling of [`DropShadow`]. Where
        /// Drop Shadow casts a blurred silhouette *behind* the layer, Inner Shadow
        /// casts one onto the *inside* of the alpha edge (AE "Inner Shadow"). The
        /// authoring vocabulary mirrors [`DropShadow`] (an animatable `offset` in
        /// layer-local pixels, computed by the author from AE's angle/distance) plus a
        /// blur `size`; `choke` is AE's inward-spread share (0..=1), the interior twin
        /// of Drop Shadow's `spread_radius`.
        ///
        /// JRB-1468: lowers to [`scene::NodeEffect::InnerShadow`], a single-pass
        /// post-process the renderer draws over the layer's rasterized content.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct InnerShadowStyle {
            /// Whether the shadow is rendered.
            #[serde(default = "default_true")]
            pub(crate) enabled: bool,
            /// RGBA shadow color (alpha carries the shadow opacity, like [`DropShadow`]).
            #[serde(default = "default_shadow_color")]
            pub(crate) color: ColorProperty,
            /// Shadow offset in layer-local pixels (author derives it from AE's
            /// angle + distance, matching the PAG Drop Shadow convention).
            #[serde(default)]
            pub(crate) offset: Vector2Property,
            /// Blur size in pixels (AE "Size").
            #[serde(default)]
            pub(crate) size: NonNegativeProperty,
            /// Inward hard-erosion share of `size` (0..=1) before the blur (AE
            /// "Choke"); the interior twin of Drop Shadow's spread.
            #[serde(default)]
            pub(crate) choke: ScalarProperty,
            /// Shadow compositing mode. **Authored & persisted but not yet rendered:**
            /// [`scene::NodeEffect::InnerShadow`] is a single-pass post-process that
            /// recolors pixels already inside the alpha (unlike the additive
            /// [`OuterGlowStyle`] / [`DropShadow`] paths that composite alongside the
            /// content and forward `blend_mode`), so a per-effect blend mode would have
            /// to be applied *inside* the shader pass. Wiring that through the scene
            /// variant + WGSL post-process is deferred to the follow-up tracked in the
            /// PR body next to Satin / Bevel & Emboss; until then the renderer always
            /// composites the shadow as Normal. The field is kept (and round-trips
            /// through serde) so authored documents survive that follow-up unchanged.
            #[serde(default)]
            pub(crate) blend_mode: BlendMode,
        }

        /// Inner glow layer style: the inverted sibling of [`OuterGlowStyle`]. Where
        /// Outer Glow radiates outward from the alpha edge, Inner Glow radiates inward
        /// (AE "Inner Glow"). Mirrors the Outer Glow authoring vocabulary (`color`,
        /// `size`, `range`) with AE's inward-spread `choke` in place of `spread`, plus
        /// AE's [`GlowSource`] (glow from the `Edge` inward vs. from the `Center`
        /// outward-to-edge).
        ///
        /// JRB-1468: lowers to [`scene::NodeEffect::InnerGlow`], a single-pass
        /// post-process the renderer draws over the layer's rasterized content.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct InnerGlowStyle {
            /// Whether the glow is rendered.
            #[serde(default = "default_true")]
            pub(crate) enabled: bool,
            /// RGBA glow color (alpha carries the glow opacity, like [`OuterGlowStyle`]).
            #[serde(default = "default_glow_color")]
            pub(crate) color: ColorProperty,
            /// Glow size in pixels (AE "Size").
            #[serde(default)]
            pub(crate) size: NonNegativeProperty,
            /// Inward hard-erosion share of `size` (0..=1) before the blur (AE
            /// "Choke"); the interior twin of Outer Glow's `spread`.
            #[serde(default)]
            pub(crate) choke: ScalarProperty,
            /// AE "Range" (typically 0.5..=1.0); scales the glow reach, matching
            /// [`OuterGlowStyle::range`]. The renderer owns the divide-by-zero clamp.
            #[serde(default = "default_glow_range")]
            pub(crate) range: ScalarProperty,
            /// Where the glow originates (AE "Source"): from the alpha `Edge` inward
            /// (AE default) or from the `Center` outward to the edge.
            #[serde(default)]
            pub(crate) source: GlowSource,
            /// Glow compositing mode. **Authored & persisted but not yet rendered:**
            /// like [`InnerShadowStyle::blend_mode`], [`scene::NodeEffect::InnerGlow`]
            /// is a single-pass post-process with no blend-mode input, so the renderer
            /// always composites the glow as Normal. Wiring a per-effect blend mode
            /// through the scene variant + WGSL pass is deferred to the follow-up
            /// tracked in the PR body next to Satin / Bevel & Emboss. The field is kept
            /// (and round-trips through serde) so authored documents survive unchanged.
            #[serde(default)]
            pub(crate) blend_mode: BlendMode,
        }

        /// Where an [`InnerGlowStyle`] originates, matching AE's Inner Glow "Source".
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum GlowSource {
            /// Glow grows inward from the alpha edge (AE default).
            #[default]
            Edge,
            /// Glow grows from the layer center outward to the edge.
            Center,
        }

        /// Satin layer style (JRB-1468): a soft folded sheen produced by the contour
        /// interference of two offset, blurred copies of the layer's alpha, tinted and
        /// masked to the inside (AE "Satin"). Lowers to [`scene::NodeEffect::Satin`], a
        /// single-pass post-process over the layer's rasterized content.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct SatinStyle {
            /// Whether the satin is rendered.
            #[serde(default = "default_true")]
            pub(crate) enabled: bool,
            /// RGBA satin color (alpha carries the satin opacity). AE default: black.
            #[serde(default = "default_shadow_color")]
            pub(crate) color: ColorProperty,
            /// Contour offset in layer-local pixels (author derives it from AE's
            /// angle + distance).
            #[serde(default)]
            pub(crate) offset: Vector2Property,
            /// Blur size in pixels (AE "Size").
            #[serde(default)]
            pub(crate) size: NonNegativeProperty,
            /// AE "Invert": flips the contour interference (AE default on).
            #[serde(default = "default_true")]
            pub(crate) invert: bool,
            /// Satin compositing mode. **Authored & persisted but not yet rendered:**
            /// like [`InnerShadowStyle::blend_mode`] / [`InnerGlowStyle::blend_mode`],
            /// [`scene::NodeEffect::Satin`] is a single-pass post-process with no
            /// blend-mode input, so the renderer always composites the sheen as Normal.
            /// Wiring a per-effect blend mode through the scene variant + WGSL pass is
            /// deferred to the follow-up tracked in the PR body next to Bevel & Emboss.
            /// The field is kept (and round-trips through serde) so authored documents
            /// survive that follow-up unchanged.
            #[serde(default)]
            pub(crate) blend_mode: BlendMode,
        }

        /// Bevel & Emboss layer style (JRB-1468): fake directional lighting of the
        /// alpha edge slope, adding an AE-style highlight + shadow, masked to the
        /// inside. Lowers to [`scene::NodeEffect::BevelEmboss`], a single-pass
        /// post-process. `technique` and the outer-halo styles are carried for AE
        /// interface fidelity; the renderer currently lights the inside with the
        /// Smooth normal for every style (outer-halo compositing is a follow-up).
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct BevelEmbossStyle {
            /// Whether the bevel is rendered.
            #[serde(default = "default_true")]
            pub(crate) enabled: bool,
            /// AE "Style".
            #[serde(default)]
            pub(crate) style: BevelStyle,
            /// AE "Technique".
            #[serde(default)]
            pub(crate) technique: BevelTechnique,
            /// AE "Depth" (1.0 = 100%); scales the slope steepness.
            #[serde(default = "default_bevel_depth")]
            pub(crate) depth: ScalarProperty,
            /// AE "Direction".
            #[serde(default)]
            pub(crate) direction: BevelDirection,
            /// Bevel width in pixels (AE "Size").
            #[serde(default = "default_bevel_size")]
            pub(crate) size: NonNegativeProperty,
            /// Extra edge blur in pixels (AE "Soften").
            #[serde(default)]
            pub(crate) soften: NonNegativeProperty,
            /// Light azimuth in degrees (AE "Angle"; default 120).
            #[serde(default = "default_bevel_angle")]
            pub(crate) angle: ScalarProperty,
            /// Light elevation in degrees (AE "Altitude"; default 30).
            #[serde(default = "default_bevel_altitude")]
            pub(crate) altitude: ScalarProperty,
            /// RGBA highlight (alpha carries the highlight opacity; AE default white at
            /// 75%, composited Screen-like).
            #[serde(default = "default_bevel_highlight_color")]
            pub(crate) highlight_color: ColorProperty,
            /// RGBA shadow (alpha carries the shadow opacity; AE default black at 75%,
            /// composited Multiply-like).
            #[serde(default = "default_shadow_color_75")]
            pub(crate) shadow_color: ColorProperty,
        }

        /// AE Bevel & Emboss "Style".
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum BevelStyle {
            /// Bevel on the inside of the alpha (AE default).
            #[default]
            InnerBevel,
            OuterBevel,
            Emboss,
            PillowEmboss,
            StrokeEmboss,
        }

        /// AE Bevel & Emboss "Technique". Carried for interface fidelity; the renderer
        /// currently uses the Smooth normal for all three.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum BevelTechnique {
            #[default]
            Smooth,
            ChiselHard,
            ChiselSoft,
        }

        /// AE Bevel & Emboss "Direction".
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub enum BevelDirection {
            /// Light raises the surface (AE default).
            #[default]
            Up,
            /// Light sinks the surface (highlight/shadow swap).
            Down,
        }
    };
}

define_layer_styles_schema! {}

impl Default for DropShadow {
    fn default() -> Self {
        Self {
            enabled: true,
            color: default_shadow_color(),
            offset: [0.0, 0.0],
            blur_radius: NonNegativeProperty::default(),
            spread_radius: NonNegativeProperty::default(),
            blend_mode: BlendMode::Normal,
        }
    }
}

impl Default for OuterGlowStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: default_glow_color(),
            size: NonNegativeProperty::default(),
            spread: 0.0,
            range: default_glow_range(),
            blend_mode: BlendMode::Normal,
        }
    }
}

impl Default for StrokeOutlineStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: default_stroke_outline_color(),
            width: NonNegativeProperty::default(),
            position: LayerStrokePosition::default(),
            blend_mode: BlendMode::Normal,
        }
    }
}

impl StrokeOutlineStyle {
    /// Constructs an enabled normal-blend silhouette stroke.
    #[must_use]
    pub fn new(
        color: ColorProperty,
        width: NonNegativeProperty,
        position: LayerStrokePosition,
    ) -> Self {
        Self {
            enabled: true,
            color,
            width,
            position,
            blend_mode: BlendMode::Normal,
        }
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[must_use]
    pub const fn color(&self) -> ColorProperty {
        self.color
    }

    pub fn set_color(&mut self, color: ColorProperty) {
        self.color = color;
    }

    #[must_use]
    pub const fn width(&self) -> NonNegativeProperty {
        self.width
    }

    pub fn set_width(&mut self, width: NonNegativeProperty) {
        self.width = width;
    }

    #[must_use]
    pub const fn position(&self) -> LayerStrokePosition {
        self.position
    }

    #[must_use]
    pub const fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }
}

impl Default for GradientOverlayStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            opacity: default_gradient_overlay_opacity(),
            gradient_type: ShapeGradientType::default(),
            start: [0.0, 0.0],
            end: [0.0, 0.0],
            stops: Vec::new(),
            blend_mode: BlendMode::Normal,
        }
    }
}

impl GradientOverlayStyle {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[must_use]
    pub const fn opacity(&self) -> ScalarProperty {
        self.opacity
    }

    pub fn set_opacity(&mut self, opacity: ScalarProperty) {
        self.opacity = opacity;
    }

    #[must_use]
    pub const fn gradient_type(&self) -> ShapeGradientType {
        self.gradient_type
    }

    #[must_use]
    pub const fn start(&self) -> Vector2Property {
        self.start
    }

    pub fn set_start(&mut self, start: Vector2Property) {
        self.start = start;
    }

    #[must_use]
    pub const fn end(&self) -> Vector2Property {
        self.end
    }

    pub fn set_end(&mut self, end: Vector2Property) {
        self.end = end;
    }

    #[must_use]
    pub fn stops(&self) -> &[ShapeGradientStop] {
        &self.stops
    }

    #[must_use]
    pub const fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }
}

impl Default for InnerShadowStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: default_shadow_color(),
            offset: [0.0, 0.0],
            size: NonNegativeProperty::default(),
            choke: 0.0,
            blend_mode: BlendMode::Normal,
        }
    }
}

impl InnerShadowStyle {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    #[must_use]
    pub const fn color(&self) -> ColorProperty {
        self.color
    }
    pub fn set_color(&mut self, color: ColorProperty) {
        self.color = color;
    }
    #[must_use]
    pub const fn offset(&self) -> Vector2Property {
        self.offset
    }
    pub fn set_offset(&mut self, offset: Vector2Property) {
        self.offset = offset;
    }
    #[must_use]
    pub const fn size(&self) -> NonNegativeProperty {
        self.size
    }
    pub fn set_size(&mut self, size: NonNegativeProperty) {
        self.size = size;
    }
    #[must_use]
    pub const fn choke(&self) -> ScalarProperty {
        self.choke
    }
    pub fn set_choke(&mut self, choke: ScalarProperty) {
        self.choke = choke;
    }
}

/// Black at 75% opacity — AE's default shadow/bevel-shadow color.
fn default_shadow_color_75() -> ColorProperty {
    [0.0, 0.0, 0.0, 0.75]
}

/// AE's outer-glow default: a pale-yellow glow at 75% opacity.
fn default_glow_color() -> ColorProperty {
    [1.0, 1.0, 0.75, 0.75]
}

/// libpag's typical authored glow `range` (AE default).
fn default_glow_range() -> ScalarProperty {
    0.5
}

/// AE's stroke-style default color: opaque red.
fn default_stroke_outline_color() -> ColorProperty {
    [1.0, 0.0, 0.0, 1.0]
}

fn default_gradient_overlay_opacity() -> ScalarProperty {
    1.0
}

impl Default for InnerGlowStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: default_glow_color(),
            size: NonNegativeProperty::default(),
            choke: 0.0,
            range: default_glow_range(),
            source: GlowSource::default(),
            blend_mode: BlendMode::Normal,
        }
    }
}

impl InnerGlowStyle {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    #[must_use]
    pub const fn color(&self) -> ColorProperty {
        self.color
    }
    pub fn set_color(&mut self, color: ColorProperty) {
        self.color = color;
    }
    #[must_use]
    pub const fn size(&self) -> NonNegativeProperty {
        self.size
    }
    pub fn set_size(&mut self, size: NonNegativeProperty) {
        self.size = size;
    }
    #[must_use]
    pub const fn choke(&self) -> ScalarProperty {
        self.choke
    }
    pub fn set_choke(&mut self, choke: ScalarProperty) {
        self.choke = choke;
    }
    #[must_use]
    pub const fn range(&self) -> ScalarProperty {
        self.range
    }
    pub fn set_range(&mut self, range: ScalarProperty) {
        self.range = range;
    }
    #[must_use]
    pub const fn source(&self) -> GlowSource {
        self.source
    }
}

impl Default for SatinStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: default_shadow_color(),
            offset: [0.0, 0.0],
            size: NonNegativeProperty::default(),
            invert: true,
            blend_mode: BlendMode::Normal,
        }
    }
}

impl SatinStyle {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    #[must_use]
    pub const fn color(&self) -> ColorProperty {
        self.color
    }
    pub fn set_color(&mut self, color: ColorProperty) {
        self.color = color;
    }
    #[must_use]
    pub const fn offset(&self) -> Vector2Property {
        self.offset
    }
    pub fn set_offset(&mut self, offset: Vector2Property) {
        self.offset = offset;
    }
    #[must_use]
    pub const fn size(&self) -> NonNegativeProperty {
        self.size
    }
    pub fn set_size(&mut self, size: NonNegativeProperty) {
        self.size = size;
    }
    #[must_use]
    pub const fn invert(&self) -> bool {
        self.invert
    }
    pub fn set_invert(&mut self, invert: bool) {
        self.invert = invert;
    }
}

impl Default for BevelEmbossStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            style: BevelStyle::default(),
            technique: BevelTechnique::default(),
            depth: default_bevel_depth(),
            direction: BevelDirection::default(),
            size: default_bevel_size(),
            soften: NonNegativeProperty::default(),
            angle: default_bevel_angle(),
            altitude: default_bevel_altitude(),
            highlight_color: default_bevel_highlight_color(),
            shadow_color: default_shadow_color_75(),
        }
    }
}

impl BevelEmbossStyle {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    #[must_use]
    pub const fn style(&self) -> BevelStyle {
        self.style
    }
    #[must_use]
    pub const fn depth(&self) -> ScalarProperty {
        self.depth
    }
    pub fn set_depth(&mut self, depth: ScalarProperty) {
        self.depth = depth;
    }
    #[must_use]
    pub const fn direction(&self) -> BevelDirection {
        self.direction
    }
    #[must_use]
    pub const fn size(&self) -> NonNegativeProperty {
        self.size
    }
    pub fn set_size(&mut self, size: NonNegativeProperty) {
        self.size = size;
    }
    #[must_use]
    pub const fn soften(&self) -> NonNegativeProperty {
        self.soften
    }
    pub fn set_soften(&mut self, soften: NonNegativeProperty) {
        self.soften = soften;
    }
    #[must_use]
    pub const fn angle(&self) -> ScalarProperty {
        self.angle
    }
    pub fn set_angle(&mut self, angle: ScalarProperty) {
        self.angle = angle;
    }
    #[must_use]
    pub const fn altitude(&self) -> ScalarProperty {
        self.altitude
    }
    pub fn set_altitude(&mut self, altitude: ScalarProperty) {
        self.altitude = altitude;
    }
    #[must_use]
    pub const fn highlight_color(&self) -> ColorProperty {
        self.highlight_color
    }
    pub fn set_highlight_color(&mut self, color: ColorProperty) {
        self.highlight_color = color;
    }
    #[must_use]
    pub const fn shadow_color(&self) -> ColorProperty {
        self.shadow_color
    }
    pub fn set_shadow_color(&mut self, color: ColorProperty) {
        self.shadow_color = color;
    }
}

/// AE Bevel defaults.
fn default_bevel_depth() -> ScalarProperty {
    1.0
}

fn default_bevel_size() -> NonNegativeProperty {
    NonNegativeProperty::new(5.0).unwrap_or_default()
}

fn default_bevel_angle() -> ScalarProperty {
    120.0
}

fn default_bevel_altitude() -> ScalarProperty {
    30.0
}

fn default_bevel_highlight_color() -> ColorProperty {
    [1.0, 1.0, 1.0, 0.75]
}
