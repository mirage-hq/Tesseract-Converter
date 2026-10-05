//! Editable graphic text and shapes shared by both conversion directions.
//!
//! A Premiere Type-tool graphic is a clip over synthetic generator media. Its
//! component chain holds one or more `AE.ADBE Text` and `AE.ADBE Shape`
//! objects, optionally preceded by the graphic's intrinsic Vector Motion. This
//! module owns the model and the component parameter layouts;
//! `format::text_payload` owns the Source Text value and
//! `format::shape_payload` the Shape Path and Appearance values.
//!
//! Units follow Premiere: sizes and positions are sequence pixels, tracking is
//! 1/1000 em, and leading is added to Premiere's automatic 120% line spacing.
//! Semantics were measured by rendering generated graphics in Premiere 26.

use super::records::{self, XmlRecordDefinition};
use super::text_shadow::PrTextShadow;
use super::{PrAnimatedProperty, PrGraphic, PrMask, PrPropertyAnimation};
use std::collections::BTreeSet;

/// An 8-bit RGB paint as stored by Premiere text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrRgb(pub(crate) [u8; 3]);

/// A glyph stroke that Premiere draws `width` pixels outside the outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrTextStroke {
    pub(crate) color: PrRgb,
    pub(crate) width: f32,
}

/// Paragraph justification. `Justify` stretches every line but the last,
/// which stays left-aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrJustification {
    Left,
    Right,
    Center,
    Justify,
}

/// Vertical placement of a text block around its point origin or inside its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrVerticalAlign {
    Top,
    Center,
    Bottom,
}

/// How a text layer is laid out around its local origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PrTextFrame {
    /// Alignment selects the first, middle or last baseline at the origin.
    Point { vertical: PrVerticalAlign },
    /// Text wraps inside a box whose top-left corner is the origin.
    Box {
        width: f32,
        height: f32,
        vertical: PrVerticalAlign,
    },
}

/// The box that Premiere draws behind a text block when its Background is
/// on: document slots 17 (color), 18 (on), 19 (opacity), 20 (size) and 34
/// (corner radius). AME renders of 48 px caption cues measured one box
/// around the whole block, `size` px beyond the glyph ink on every side,
/// centred on the ink, with all four corners rounded by `radius` px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrTextBackground {
    pub(crate) color: PrRgb,
    /// Percent.
    pub(crate) opacity: f32,
    /// Pixels added on every side of the ink at [`Self::CALIBRATED_SIZE`].
    pub(crate) size: f32,
    /// Pixels.
    pub(crate) radius: f32,
}

impl PrTextBackground {
    /// The one text size whose background box AME renders measured; whether
    /// `size` scales with the text size is unverified.
    pub(crate) const CALIBRATED_SIZE: f32 = 48.0;
    /// The only opacity rendered; Premiere blends a shadow's opacity in
    /// linear light, so another value is not a linear alpha.
    const CALIBRATED_OPACITY: f32 = 100.0;
    /// A lower bound of one line's glyph ink height at the calibrated size,
    /// about an x-height (0.5 em): the renders measured 32.6 px for
    /// capitals (INFERRED for other glyphs).
    const LEAST_LINE_INK: f32 = 24.0;
    /// Premiere's line pitch at the calibrated size: 120% of the size (a
    /// two-line cue measured 58 px).
    const LINE_PITCH: f32 = 1.2 * Self::CALIBRATED_SIZE;

    /// Why the box Premiere draws for this background behind `document` is
    /// unverified, if it is, in either direction: the size is calibrated at
    /// 48 px only, only opacity 100 was rendered, and how Premiere clamps a
    /// radius above half the box height is unknown. The box height is bounded
    /// below by the block's least ink plus the padding: a lowercase- or
    /// punctuation-only line can be shorter than [`Self::LEAST_LINE_INK`], so
    /// a radius between the true and this half height stays unverified.
    pub(crate) fn unverified_reason(&self, document: &PrTextDocument) -> Option<String> {
        if document.size != Self::CALIBRATED_SIZE {
            return Some(format!(
                "the box is calibrated at {} px text only, not {} px",
                Self::CALIBRATED_SIZE,
                document.size
            ));
        }
        if self.opacity != Self::CALIBRATED_OPACITY {
            return Some(format!(
                "only opacity {} was rendered, not {}",
                Self::CALIBRATED_OPACITY,
                self.opacity
            ));
        }
        let lines = document.text.lines().count().max(1) as f32;
        let least_height =
            2.0 * self.size + Self::LEAST_LINE_INK + Self::LINE_PITCH * (lines - 1.0);
        (self.radius > least_height / 2.0).then(|| {
            format!(
                "how Premiere clamps a corner radius ({}) above half the box height (at least {}) is unknown",
                self.radius,
                least_height / 2.0
            )
        })
    }
}

/// The Source Text document of one single-style text layer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrTextDocument {
    /// Text with `\n` line breaks.
    pub(crate) text: String,
    /// PostScript font name.
    pub(crate) font: String,
    pub(crate) size: f32,
    /// `None` when the fill is disabled.
    pub(crate) fill: Option<PrRgb>,
    pub(crate) stroke: Option<PrTextStroke>,
    /// `None` when the shadow is disabled. [`PrText::validate`] leaves its
    /// ranges to the conversion, which omits only an out-of-range shadow.
    pub(crate) shadow: Option<PrTextShadow>,
    pub(crate) all_caps: bool,
    pub(crate) tracking: f32,
    /// Pixels added to the automatic line spacing of 1.2 × `size`.
    pub(crate) leading: f32,
    pub(crate) justification: PrJustification,
    pub(crate) frame: PrTextFrame,
    /// `None` when the background is off. [`PrText::validate`] checks its
    /// ranges; [`PrTextBackground::unverified_reason`] is left to the
    /// conversion, which omits only such a background.
    pub(crate) background: Option<PrTextBackground>,
}

/// A text layer transform in sequence pixels, with any graphic-level Vector
/// Motion already composed into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrTextTransform {
    pub(crate) position: [f64; 2],
    /// Layer-local pivot, applied before scale and rotation.
    pub(crate) anchor: [f64; 2],
    /// Uniform scale in percent.
    pub(crate) scale: f64,
    /// Clockwise degrees.
    pub(crate) rotation: f64,
    /// Percent.
    pub(crate) opacity: f64,
}

/// One Source Text key: the complete document that Premiere shows from
/// `source_ticks` on the generator clock until the next key. Premiere holds
/// Source Text between keys; a key stores no interpolation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrSourceTextKey {
    pub(crate) source_ticks: i64,
    pub(crate) document: PrTextDocument,
}

/// A Source Text document field that an FX text layer animates, so that
/// keys may differ in it. Every other field is the same in every key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SourceTextField {
    Text,
    Size,
    FillEnabled,
    FillColor,
    Tracking,
    Leading,
    StrokeEnabled,
    /// Carried by an all-character text animator's additive width, not the
    /// vector-only FX layer StrokeWidth property.
    StrokeWidth,
    AllCaps,
}

impl std::fmt::Display for SourceTextField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Text => "text",
            Self::Size => "size",
            Self::FillEnabled => "fill switch",
            Self::FillColor => "fill color",
            Self::Tracking => "tracking",
            Self::Leading => "leading",
            Self::StrokeEnabled => "stroke switch",
            Self::StrokeWidth => "stroke width",
            Self::AllCaps => "all caps",
        })
    }
}

/// One editable text layer: its Essential Graphics name, document, and transform.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrText {
    /// Static Horizontal Scale when Uniform Scale is off. `transform.scale`
    /// and its Scale keys then affect only the vertical axis.
    pub(crate) horizontal_scale: Option<f64>,
    pub(crate) name: String,
    /// The document shown before the first Source Text key, which is the
    /// first key's document when there are keys (Premiere 26.5.1 renders it
    /// there, not the saved start value), or the static Source Text.
    pub(crate) document: PrTextDocument,
    pub(crate) transform: PrTextTransform,
    /// Position, Scale, Rotation and Opacity keys of the text object. Scale
    /// is vertical only when `horizontal_scale` is present, otherwise uniform;
    /// on the generator clock, composed like `transform`. Position keys are
    /// normalized to the sequence frame.
    pub(crate) animations: Vec<PrPropertyAnimation>,
    /// Source Text keys on the generator clock, in time order; empty for
    /// static text. Their documents differ from `document` only in
    /// [`SourceTextField`]s ([`PrText::keyed_fields`]).
    pub(crate) source_text_keys: Vec<PrSourceTextKey>,
    pub(crate) mask_source: Option<PrMaskSource>,
}

/// A graphic object's Mask with Shape (Shape Appearance slots 12 and 13) or
/// Mask with Text (Source Text document slots 21 and 22), as AME renders of
/// Premiere 26.5.1 saves measured it: the object is not
/// drawn, and the composite of every object below it in its group keeps only
/// where the object's rendered alpha covers it (its fill, stroke, shadow and
/// opacity), or with `inverted` only where it does not. Objects above it are
/// untouched, a SubGroup bounds it, and several masks of one group multiply
/// in chain order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrMaskSource {
    pub(crate) inverted: bool,
}

/// Static point text whose styles cover complete lines. Its transform and
/// transform keys belong to the whole block; Source Text keys are unsupported.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrTextLines {
    pub(crate) name: String,
    pub(crate) documents: Vec<PrTextDocument>,
    pub(crate) transform: PrTextTransform,
    pub(crate) animations: Vec<PrPropertyAnimation>,
}

impl PrTextLines {
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        crate::format::ensure_valid!(
            self.documents.len() >= 2
                && self.documents.iter().all(|document| {
                    matches!(document.frame, PrTextFrame::Point { .. })
                        && !document.text.contains('\n')
                }),
            "mixed point text must consist of two or more complete lines"
        );
        let first = &self.documents[0];
        crate::format::ensure_valid!(
            first.justification != PrJustification::Justify
                && self.documents.iter().all(|document| {
                    document.leading == 0.0
                        && document.frame == first.frame
                        && document.justification == first.justification
                        && document.shadow == first.shadow
                        && document.background.is_none()
                }),
            "mixed point text requires automatic leading, common alignment and shadow, and no background"
        );
        for document in &self.documents {
            document.validate()?;
        }
        validate_transform(&self.transform, "text block")?;
        validate_animations(&self.animations, &TEXT_PARAMS, "text block")
    }
}

/// A graphic's Vector Motion: one transform for all of its objects, applied
/// after each object's own transform, in sequence pixels like
/// [`PrTextTransform`]. Vector Motion has no opacity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrVectorMotion {
    pub(crate) position: [f64; 2],
    pub(crate) anchor: [f64; 2],
    /// Uniform scale in percent.
    pub(crate) scale: f64,
    /// Clockwise degrees.
    pub(crate) rotation: f64,
    /// Position, uniform Scale and Rotation keys on the generator clock.
    /// Position keys are normalized to the sequence frame.
    pub(crate) animations: Vec<PrPropertyAnimation>,
}

/// One vertex of a Shape path, in the Shape's layer pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrPathVertex {
    /// Premiere's smooth flag (Path flag word 1); a corner (0) otherwise.
    /// Premiere 26.5.1 and AME build 85 draw a smooth vertex's tangents and
    /// ignore a corner's, drawing straight segments, as measured on native
    /// smooth vertices and the rounded bar; every corner that Premiere saved
    /// has both tangents on its point.
    pub(crate) smooth: bool,
    pub(crate) point: [f32; 2],
    /// Absolute control-point coordinates for the segment ending at this vertex,
    /// in the same coordinate space as `point`, not a vector from it.
    pub(crate) in_tangent: [f32; 2],
    /// Absolute control-point coordinates for the segment starting at this vertex,
    /// in the same coordinate space as `point`, not a vector from it.
    pub(crate) out_tangent: [f32; 2],
}

impl PrPathVertex {
    /// Whether a stroke draws no join at this vertex: both tangent vectors
    /// are nonzero and point in exactly opposite directions, so the two
    /// segments meet in one direction. Compared exactly in f64, where the
    /// differences and products of f32 coordinates of similar size are exact.
    fn joins_smoothly(&self) -> bool {
        let vector = |tangent: [f32; 2]| {
            [
                f64::from(tangent[0]) - f64::from(self.point[0]),
                f64::from(tangent[1]) - f64::from(self.point[1]),
            ]
        };
        let (incoming, outgoing) = (vector(self.in_tangent), vector(self.out_tangent));
        incoming != [0.0; 2]
            && outgoing != [0.0; 2]
            && incoming[0] * outgoing[1] == incoming[1] * outgoing[0]
            && incoming[0] * outgoing[0] + incoming[1] * outgoing[1] < 0.0
    }
}

/// The outline of a Shape: one contour, because a Premiere Path has no
/// contour separator. Each segment runs from a vertex's out tangent through
/// the next vertex's in tangent; a closed path also joins the last vertex to
/// the first.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrShapePath {
    pub(crate) vertices: Vec<PrPathVertex>,
    pub(crate) closed: bool,
}

/// A stroke join as the FX renderer draws it: a miter up to an FX miter limit, a
/// bevel, or a round join.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StrokeJoin {
    Miter(f64),
    Bevel,
    Round,
}

/// The triangle that calibration-2 stroked, as Premiere saved it, in layer
/// pixels: Premiere mitered its base corners (71.996°) and beveled its apex
/// (36.008°). It also mitered the 90° corners of an open path.
const CALIBRATION_TRIANGLE: [[f32; 2]; 3] = [[300.0, 0.0], [-300.0, 195.0], [-300.0, -195.0]];
/// FX's default miter limit, which import writes for mitered corners.
pub(crate) const IMPORTED_MITER_LIMIT: f64 = 4.0;

/// The miter class bound and the bevel class bound of the miter ratio
/// 1/sin(θ/2): the ratio of the calibration triangle's base corners, the
/// largest that Premiere was seen to miter, and of its apex, the smallest
/// that it was seen to bevel. Premiere's miter limit lies between the two,
/// assumed standard and monotonic, so the classes of other angles are
/// inferred.
fn measured_ratios() -> (f64, f64) {
    let corner = |point| PrPathVertex {
        smooth: false,
        point,
        in_tangent: point,
        out_tangent: point,
    };
    let triangle = PrShapePath {
        vertices: CALIBRATION_TRIANGLE.map(corner).to_vec(),
        closed: true,
    };
    let ratios = corner_ratios(&triangle).expect("the triangle's three corners have directions");
    (ratios[1], ratios[0])
}

/// The FX join that draws a stroke on `path` as Premiere does, or why
/// that is unverified: with `requested` `None` (import), FX's default miter
/// when every corner is in the miter class and a bevel when every corner is
/// in the bevel class; with a requested join (export), that join if it draws
/// every corner so. A stroke with corners of both classes converts in
/// neither direction. A vertex with nonzero, exactly opposite tangents is no
/// corner, and an open path's ends take butt caps. FX renderers apply
/// a miter limit L differently: the tessellation backend (lyon) bevels a
/// corner whose ratio exceeds 2L, the SVG preview backend one that exceeds
/// L. So a mitered corner needs a ratio of at most min(L, miter class
/// bound), and a beveled corner one above max(2L, bevel class bound), in
/// both ([`measured_ratios`]).
pub(crate) fn stroke_join(
    path: &PrShapePath,
    requested: Option<StrokeJoin>,
) -> Result<StrokeJoin, &'static str> {
    const UNVERIFIED: &str = "stroke joins are unverified";
    let ratios = corner_ratios(path).ok_or(UNVERIFIED)?;
    let (mitered_ratio, beveled_ratio) = measured_ratios();
    let mitered = |ratio: &f64| *ratio <= mitered_ratio;
    let beveled = |ratio: &f64| *ratio >= beveled_ratio;
    if !ratios.iter().all(|ratio| mitered(ratio) || beveled(ratio)) {
        return Err(UNVERIFIED);
    }
    let all_mitered = ratios.iter().all(mitered);
    if !all_mitered && !ratios.iter().all(beveled) {
        return Err("strokes with both mitered and beveled corners are unsupported");
    }
    let proven = match requested {
        None if all_mitered => Some(StrokeJoin::Miter(IMPORTED_MITER_LIMIT)),
        None => Some(StrokeJoin::Bevel),
        Some(StrokeJoin::Miter(limit)) => (limit.is_finite()
            && limit >= 1.0
            && ratios.iter().all(|&ratio| {
                ratio <= limit.min(mitered_ratio) || ratio > (2.0 * limit).max(beveled_ratio)
            }))
        .then_some(StrokeJoin::Miter(limit)),
        Some(StrokeJoin::Bevel) => ratios.iter().all(beveled).then_some(StrokeJoin::Bevel),
        Some(StrokeJoin::Round) => return Err("round stroke joins are unverified"),
    };
    proven.ok_or(UNVERIFIED)
}

/// The miter ratio 1/sin(θ/2) of each corner of `path`, θ the angle between
/// its two segments, or `None` when a corner's direction cannot be found (a
/// segment of zero length). A segment's direction at a vertex is its tangent
/// there, or the chord toward its far control point or vertex when the
/// tangent rests on the vertex.
fn corner_ratios(path: &PrShapePath) -> Option<Vec<f64>> {
    let vertices = &path.vertices;
    let count = vertices.len();
    let vector = |from: [f32; 2], to: [f32; 2]| {
        [
            f64::from(to[0]) - f64::from(from[0]),
            f64::from(to[1]) - f64::from(from[1]),
        ]
    };
    let mut ratios = Vec::new();
    for (index, vertex) in vertices.iter().enumerate() {
        let open_end = !path.closed && (index == 0 || index + 1 == count);
        if open_end || vertex.joins_smoothly() {
            continue;
        }
        let previous = &vertices[(index + count - 1) % count];
        let next = &vertices[(index + 1) % count];
        let incoming = [
            vector(vertex.in_tangent, vertex.point),
            vector(previous.out_tangent, vertex.point),
            vector(previous.point, vertex.point),
        ]
        .into_iter()
        .find(|direction| *direction != [0.0; 2])?;
        let outgoing = [
            vector(vertex.point, vertex.out_tangent),
            vector(vertex.point, next.in_tangent),
            vector(vertex.point, next.point),
        ]
        .into_iter()
        .find(|direction| *direction != [0.0; 2])?;
        let cos = (incoming[0] * outgoing[0] + incoming[1] * outgoing[1])
            / (incoming[0].hypot(incoming[1]) * outgoing[0].hypot(outgoing[1]));
        ratios.push((2.0 / (1.0 + cos)).sqrt());
    }
    Some(ratios)
}

/// A stroke that Premiere centres on the Shape outline (Appearance slot 31
/// `0`) and draws over the fill, `width` layer pixels wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrShapeStroke {
    pub(crate) color: PrRgb,
    pub(crate) width: f32,
}

/// What a Shape's Appearance fills its path with.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrFill {
    Solid(PrRgb),
    Gradient(PrGradient),
}

/// A gradient fill in the form that Premiere 26.5.1 saved and AME drew for
/// fixture `premiere_isolated_gradient_fills_26_5`:
/// every stop's midpoint at 50 %, on the shape's x axis. Premiere
/// interpolates the color stops component-wise on encoded RGB (G4) and the
/// opacity stops apart from them (G5).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrGradient {
    pub(crate) kind: PrGradientKind,
    /// Start and end x in layer pixels, at y 0: a linear gradient runs from
    /// start to end, a radial one is centred on start with radius
    /// |end − start| (G3).
    pub(crate) start_x: f32,
    pub(crate) end_x: f32,
    /// The color stops by position, 0 at start and 1 at end.
    pub(crate) stops: Vec<PrGradientStop>,
    /// The opacity stops by position; an opaque gradient's are
    /// [`OPAQUE_OPACITY_STOPS`].
    pub(crate) opacity_stops: Vec<PrGradientOpacityStop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrGradientKind {
    Linear,
    Radial,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrGradientStop {
    pub(crate) position: f32,
    pub(crate) color: PrRgb,
}

/// One opacity stop, 0 to 1 (G5: Premiere 26.5.1 saves full opacity as an
/// absent value and the fixture's transparent end as 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrGradientOpacityStop {
    pub(crate) position: f32,
    pub(crate) opacity: f32,
}

/// The opacity stops of every opaque gradient that Premiere 26.5.1 saved for
/// the gradient fixture (G5): full at 0 and at 1.
pub(crate) const OPAQUE_OPACITY_STOPS: [PrGradientOpacityStop; 2] = [
    PrGradientOpacityStop {
        position: 0.0,
        opacity: 1.0,
    },
    PrGradientOpacityStop {
        position: 1.0,
        opacity: 1.0,
    },
];

/// Reported in both directions for a gradient whose opacity stops are not
/// all full: Premiere composites them in about gamma-2.4 light (G5: the
/// fixture's D fits that model within 0.9 levels RMSE), and FX composites a
/// stop's alpha in encoded values, which is darker. The fresh conversion of
/// D differs from its render by 10.1 levels RMSE on G5's quiet backdrop and
/// by up to 89 over the white timecode behind it.
pub(crate) const GRADIENT_OPACITY_APPROXIMATION: &str = "gradient opacity stops composite in about gamma-2.4 light in Premiere; converted as FX stop alpha, which composites in encoded RGB, so partly transparent areas render darker (on the fixture's ramp 10 levels RMSE over a dark backdrop, up to 89 over bright content)";

/// Why a gradient off the shape's x axis converts in neither direction: every
/// rendered gradient had y 0.
pub(crate) const GRADIENT_Y_UNCONVERTED: &str =
    "gradient geometry with a nonzero y is not converted";

/// Reported in both directions for a gradient shape that keeps a shadow:
/// Premiere draws a shape's shadow under its fill and stroke (calibration
/// run 1, solid shapes), and no rendered gradient shape had one.
pub(crate) const GRADIENT_SHADOW_APPROXIMATION: &str = "paint order of a gradient fill under a shadow is unmeasured against Premiere (the shadow under fill and stroke was measured on solid shapes)";

/// The most color stops of a rendered gradient: three.
const MEASURED_GRADIENT_STOPS: usize = 3;

/// The shortest gradient axis that converts, in layer pixels: FX's gradient
/// shader draws only the first stop on a linear axis shorter than 1e-5 px or
/// a radial one below 1e-10 px (`AXIS_EPSILON` in the renderer's
/// `gradient.wgsl`); ten times that keeps clear of its f32 rounding.
const MIN_GRADIENT_AXIS: f32 = 1e-4;

impl PrGradient {
    /// Checks what FX needs to draw the gradient as Premiere does, for both
    /// conversion directions: finite geometry with an axis of at least
    /// [`MIN_GRADIENT_AXIS`], two or more color stops in order within 0..=1
    /// (FX's paint contract, `ShapePaint::has_valid_values`), and one or more
    /// opacity stops in order within 0..=1, each opacity within 0..=1.
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        crate::format::ensure_valid!(
            self.start_x.is_finite()
                && self.end_x.is_finite()
                && (self.end_x - self.start_x).abs() >= MIN_GRADIENT_AXIS
                && self.stops.len() >= 2
                && self
                    .stops
                    .iter()
                    .all(|stop| (0.0..=1.0).contains(&stop.position))
                && self
                    .stops
                    .windows(2)
                    .all(|pair| pair[0].position <= pair[1].position),
            "a gradient needs a finite axis of at least {MIN_GRADIENT_AXIS} px and two or more stops in order within 0..=1"
        );
        crate::format::ensure_valid!(
            !self.opacity_stops.is_empty()
                && self.opacity_stops.iter().all(|stop| {
                    (0.0..=1.0).contains(&stop.position) && (0.0..=1.0).contains(&stop.opacity)
                })
                && self
                    .opacity_stops
                    .windows(2)
                    .all(|pair| pair[0].position <= pair[1].position),
            "a gradient needs one or more opacity stops within 0..=1, in order within 0..=1"
        );
        Ok(())
    }
}

/// What a Shape's Appearance draws, in the forms that calibration renders and
/// the gradient fixture measured.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrAppearance {
    /// Slot 0's color, or [`DEFAULT_SHAPE_FILL`] without it, or the gradient
    /// that slot 19 selects; `None` when slot 1 is `0`, which draws no fill
    /// and keeps the stroke (calibration-2). Premiere fills an open path as
    /// if it were closed.
    pub(crate) fill: Option<PrFill>,
    pub(crate) stroke: Option<PrShapeStroke>,
    /// The shadow under the fill and stroke, cast by the stroked outline.
    /// Appearance stores no angle: the shadow falls down and to the right,
    /// [`SHAPE_SHADOW_ANGLE`] in the text shadow's units. Like the text
    /// shadow, the conversion checks its ranges and form.
    pub(crate) shadow: Option<PrTextShadow>,
    pub(crate) mask_source: Option<PrMaskSource>,
}

/// The fill that Premiere draws when an Appearance has no fill color
/// (calibration run 1: rendered (128, 128, 128)).
pub(crate) const DEFAULT_SHAPE_FILL: PrRgb = PrRgb([128; 3]);

/// The direction of every Shape shadow, in degrees clockwise from up, as the
/// text shadow stores its angle: calibration run 1 offsets a 40 px shadow by
/// (+28, +28) px.
pub(crate) const SHAPE_SHADOW_ANGLE: f32 = 135.0;

/// One graphic Shape object: a path with its Appearance, placed by its own
/// transform like text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrShape {
    /// The Essential Graphics layer name (`InstanceName`).
    pub(crate) name: String,
    pub(crate) path: PrShapePath,
    pub(crate) appearance: PrAppearance,
    /// Position and anchor in sequence pixels; the path is drawn at
    /// `position + R(rotation) · S · (point − anchor)` (calibration run 1: a
    /// 600×300 rectangle at Position 0.5:0.5 covers x 660-1259, y 390-689).
    pub(crate) transform: PrTextTransform,
    /// Horizontal Scale in percent while Uniform Scale is off: `S` then
    /// scales x by it and y by `transform.scale`, before the rotation
    /// (fixture render G7). `None` scales both axes by `transform.scale`.
    pub(crate) horizontal_scale: Option<f64>,
    /// Static owner-only mask in graphic-frame coordinates, after the object transform.
    pub(crate) mask: Option<PrMask>,
}

impl PrShape {
    /// Checks the value ranges that the encodings cannot express, for both
    /// conversion directions. Like the text, the shadow's ranges are left to
    /// the conversion, and so is the stroke join, which [`stroke_join`]
    /// proves for each direction.
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        crate::format::ensure_valid!(
            !self.path.vertices.is_empty()
                && self.path.vertices.iter().all(|vertex| {
                    [vertex.point, vertex.in_tangent, vertex.out_tangent]
                        .iter()
                        .flatten()
                        .all(|coordinate| coordinate.is_finite())
                }),
            "a shape path must have finite vertices"
        );
        if let Some(stroke) = &self.appearance.stroke {
            validate_stroke_width(stroke.width, "shape")?;
        }
        if let Some(PrFill::Gradient(gradient)) = &self.appearance.fill {
            gradient.validate()?;
        }
        crate::format::ensure_valid!(
            self.horizontal_scale.is_none_or(|scale| SHAPE_PARAMS
                .iter()
                .any(|spec| spec.role == HorizontalScale && spec.holds(scale))),
            "shape Horizontal Scale must be within Premiere's bounds"
        );
        validate_transform(&self.transform, "shape")
    }

    /// The warnings, one per approximation, for converting this shape's
    /// gradient fill in either direction: what no Premiere render measured
    /// beside a gradient. That is geometry under the shape's
    /// Scale, Horizontal Scale or Rotation or `graphic`'s kept Vector Motion
    /// (FX draws the gradient in layer space, so it moves with the shape as a
    /// solid fill does), a shadow that the converted shape keeps
    /// (`shadowed`), opacity stops, a shape or graphic Opacity below 100, and
    /// more than [`MEASURED_GRADIENT_STOPS`] color stops. None for another
    /// fill.
    pub(crate) fn gradient_approximations(
        &self,
        graphic: &PrGraphic,
        shadowed: bool,
    ) -> Vec<String> {
        let Some(PrFill::Gradient(gradient)) = &self.appearance.fill else {
            return Vec::new();
        };
        let transform = &self.transform;
        let motion = graphic.vector_motion.as_ref();
        let transforms: Vec<_> = [
            (transform.scale != 100.0).then(|| format!("Scale {}", transform.scale)),
            self.horizontal_scale
                .filter(|scale| *scale != 100.0)
                .map(|scale| format!("Horizontal Scale {scale}")),
            (transform.rotation != 0.0).then(|| format!("Rotation {}", transform.rotation)),
            motion
                .filter(|motion| motion.scale != 100.0)
                .map(|motion| format!("Vector Motion Scale {}", motion.scale)),
            motion
                .filter(|motion| motion.rotation != 0.0)
                .map(|motion| format!("Vector Motion Rotation {}", motion.rotation)),
            motion
                .filter(|motion| !motion.animations.is_empty())
                .map(|_| "Vector Motion keys".to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect();
        let opacities: Vec<_> = [
            (transform.opacity != 100.0).then(|| format!("Opacity {}", transform.opacity)),
            (graphic.opacity != 100.0).then(|| format!("graphic Opacity {}", graphic.opacity)),
            (!graphic.animations.is_empty()).then(|| "graphic Opacity keys".to_owned()),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut warnings = Vec::new();
        if !transforms.is_empty() {
            warnings.push(format!(
                "gradient geometry under a shape transform ({}) is unmeasured against Premiere; converted in layer space",
                transforms.join(" / ")
            ));
        }
        if shadowed {
            warnings.push(GRADIENT_SHADOW_APPROXIMATION.to_owned());
        }
        if gradient
            .opacity_stops
            .iter()
            .any(|stop| stop.opacity != 1.0)
        {
            warnings.push(GRADIENT_OPACITY_APPROXIMATION.to_owned());
        }
        if !opacities.is_empty() {
            warnings.push(format!(
                "gradient fill under an Opacity below 100 ({}) is unmeasured against Premiere; converted as FX opacity",
                opacities.join(" / ")
            ));
        }
        if gradient.stops.len() > MEASURED_GRADIENT_STOPS {
            warnings.push(format!(
                "gradient with {} color stops is unmeasured against Premiere, which rendered at most {MEASURED_GRADIENT_STOPS}; converted stop for stop",
                gradient.stops.len()
            ));
        }
        warnings
    }

    /// Folds a static Vector Motion into this shape's transform, as
    /// [`PrText::compose_static_vector_motion`] does for text.
    pub(crate) fn compose_static_vector_motion(&mut self, motion: &PrVectorMotion) {
        debug_assert!(
            motion.animations.is_empty(),
            "only static Vector Motion composes"
        );
        let similarity = Similarity::of(motion);
        similarity.compose(&mut self.transform);
        if let Some(scale) = &mut self.horizontal_scale {
            *scale *= similarity.scale;
        }
    }
}

/// One object of a graphic, in the order its component chain lists it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrGraphicObject {
    Text(PrText),
    TextLines(PrTextLines),
    Shape(PrShape),
    Group(PrGraphicGroup),
}

/// A graphic SubGroup (`AE.ADBE Graphic SubGroup`) at its identity
/// transform, and its objects in chain order. The chain lists it before its
/// objects, which its `ComponentGroupMap` pins to it. Its boundary bounds
/// the Mask with Shape and Text of the objects inside it; Premiere stores no opacity for it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrGraphicGroup {
    /// The Essential Graphics group name (`InstanceName`).
    pub(crate) name: String,
    pub(crate) objects: Vec<PrGraphicObject>,
}

/// Why a Mask with Shape or Text is outside the forms that
/// the native mask controls covered, named in omissions.
pub(crate) const MASK_OVER_SUBGROUP_UNVERIFIED: &str =
    "a Mask with Shape or Text over a lower SubGroup is unverified against Premiere";

impl PrGraphicObject {
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        match self {
            Self::Text(text) => text.validate(),
            Self::TextLines(text) => text.validate(),
            Self::Shape(shape) => {
                shape.validate()?;
                if let Some(mask) = &shape.mask {
                    crate::format::ensure_valid!(
                        mask.path_keys.is_empty(),
                        "Shape-attached Mask Path keys are unsupported"
                    );
                    mask.validate()?;
                }
                Ok(())
            }
            Self::Group(group) => group.objects.iter().try_for_each(Self::validate),
        }
    }

    /// This object's Mask with Shape or Text; a SubGroup has none.
    pub(crate) fn mask_source(&self) -> Option<PrMaskSource> {
        match self {
            Self::Text(text) => text.mask_source,
            Self::Shape(shape) => shape.appearance.mask_source,
            Self::TextLines(_) | Self::Group(_) => None,
        }
    }

    /// Folds a static Vector Motion into this object, as
    /// [`PrText::compose_static_vector_motion`] and
    /// [`PrShape::compose_static_vector_motion`] do, unless the folded object
    /// would fail its `validate`, for example a text Scale above 4000%; then
    /// the object stays unchanged and the Vector Motion must stay separate.
    /// Returns whether it folded.
    pub(crate) fn compose_static_vector_motion_in_range(
        &mut self,
        motion: &PrVectorMotion,
        frame: [u32; 2],
    ) -> bool {
        let mut composed = self.clone();
        match &mut composed {
            Self::Text(text) => text.compose_static_vector_motion(motion, frame),
            // Keep the graphic motion as the common parent of the line block.
            Self::TextLines(_) | Self::Group(_) => return false,
            Self::Shape(shape) if shape.mask.is_none() => {
                shape.compose_static_vector_motion(motion)
            }
            Self::Shape(_) => return false,
        }
        let folds = composed.validate().is_ok();
        if folds {
            *self = composed;
        }
        folds
    }
}

/// The omission reason of a graphic part that does not convert from
/// Premiere, the reader's and the importer's: `reason`, and the `below`
/// objects under a mask that go with it.
pub(crate) fn omitted_part(reason: &str, below: usize) -> String {
    match below {
        0 => format!("{reason}; the object is not converted"),
        1 => format!("{reason}; the mask and the 1 object below it are not converted"),
        below => format!("{reason}; the mask and the {below} objects below it are not converted"),
    }
}

/// The first object of `objects`, one group's objects in chain order at
/// SubGroup `depth` (0 for the graphic's own objects), whose Mask with Shape
/// or Text composite is outside the forms covered by the native mask controls,
/// and why. That composite is the mask object and every object below it in
/// its group; the objects above it are unaffected. Rendered: Shapes of one
/// closed path with a solid fill or none, a stroke, a shadow and any opacity,
/// and Texts without stroke, shadow or background, as masks of the graphic's
/// objects or of one SubGroup's, over Texts, Shapes and masks of the same
/// polarity. A lower SubGroup, a nested SubGroup, masks of both polarities,
/// a gradient fill, an open path and a mask attached to the mask object were
/// not, so both directions keep such a composite out.
pub(crate) fn unverified_mask_composite(
    objects: &[PrGraphicObject],
    depth: usize,
) -> Option<(usize, &'static str)> {
    let mut polarity = None;
    for (index, object) in objects.iter().enumerate() {
        let Some(mask) = object.mask_source() else {
            continue;
        };
        let lower = &objects[index + 1..];
        let reason = if depth > 1 {
            Some(
                "a Mask with Shape or Text inside a nested SubGroup is unverified against Premiere",
            )
        } else if lower
            .iter()
            .any(|object| matches!(object, PrGraphicObject::Group(_)))
        {
            Some(MASK_OVER_SUBGROUP_UNVERIFIED)
        } else if polarity.is_some_and(|inverted| inverted != mask.inverted) {
            Some("Masks with Shape or Text of both polarities in one group are unverified against Premiere")
        } else {
            match object {
                PrGraphicObject::Shape(shape) => [
                    (
                        shape.mask.is_some(),
                        "a Mask with Shape with an attached mask is unverified against Premiere",
                    ),
                    (
                        matches!(shape.appearance.fill, Some(PrFill::Gradient(_))),
                        "a Mask with Shape with a gradient fill is unverified against Premiere",
                    ),
                    (
                        !shape.path.closed,
                        "a Mask with Shape of an open path is unverified against Premiere",
                    ),
                ]
                .into_iter()
                .find_map(|(unverified, reason)| unverified.then_some(reason)),
                PrGraphicObject::Text(text) => {
                    let document = &text.document;
                    (document.stroke.is_some()
                        || document.shadow.is_some()
                        || document.background.is_some())
                    .then_some("a Mask with Text with a stroke, shadow or background is unverified against Premiere")
                }
                PrGraphicObject::TextLines(_) | PrGraphicObject::Group(_) => None,
            }
        };
        if let Some(reason) = reason {
            return Some((index, reason));
        }
        polarity = Some(mask.inverted);
    }
    None
}

/// A static Vector Motion as the similarity that it applies to its objects,
/// `x -> position + scale * R(rotation) * (x - anchor)`.
struct Similarity<'a> {
    motion: &'a PrVectorMotion,
    scale: f64,
    sin: f64,
    cos: f64,
}

impl<'a> Similarity<'a> {
    fn of(motion: &'a PrVectorMotion) -> Self {
        let (sin, cos) = motion.rotation.to_radians().sin_cos();
        Self {
            motion,
            scale: motion.scale / 100.0,
            sin,
            cos,
        }
    }

    /// Scales and rotates a vector.
    fn turn(&self, vector: [f64; 2]) -> [f64; 2] {
        [
            self.scale * (vector[0] * self.cos - vector[1] * self.sin),
            self.scale * (vector[0] * self.sin + vector[1] * self.cos),
        ]
    }

    /// Moves a point.
    fn place(&self, point: [f64; 2]) -> [f64; 2] {
        let motion = self.motion;
        let offset = self.turn([point[0] - motion.anchor[0], point[1] - motion.anchor[1]]);
        [
            motion.position[0] + offset[0],
            motion.position[1] + offset[1],
        ]
    }

    /// Composes an object transform: its position moves, its scale
    /// multiplies and its rotation adds.
    fn compose(&self, transform: &mut PrTextTransform) {
        transform.position = self.place(transform.position);
        transform.scale *= self.scale;
        transform.rotation += self.motion.rotation;
    }
}

impl PrVectorMotion {
    /// Checks the values that Vector Motion parameters can hold.
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        crate::format::ensure_valid!(
            self.position
                .iter()
                .chain(&self.anchor)
                .chain([&self.scale, &self.rotation])
                .all(|value| value.is_finite())
                && (0.0..=10000.0).contains(&self.scale)
                && (-32768.0..=32767.0).contains(&self.rotation),
            "Vector Motion must be finite, with Premiere's scale and rotation bounds"
        );
        validate_animations(&self.animations, &VECTOR_MOTION_PARAMS, "Vector Motion")
    }
}

/// Checks that each property is keyed once, on a parameter of `specs` that
/// can hold keys, with finite values inside that parameter's native bounds
/// and the keys that the native readers accept.
fn validate_animations(
    animations: &[PrPropertyAnimation],
    specs: &[GraphicParamSpec],
    owner: &str,
) -> crate::format::Result<()> {
    for (index, animation) in animations.iter().enumerate() {
        let property = animation.property();
        crate::format::ensure_valid!(
            animations[..index]
                .iter()
                .all(|earlier| earlier.property() != property),
            "{owner} has more than one {property:?} animation"
        );
        let spec = specs
            .iter()
            .find(|spec| spec.role.animation() == Some(property))
            .ok_or_else(|| {
                crate::format::invalid(format!("{owner} has no keyable {property:?}"))
            })?;
        crate::format::ensure_valid!(
            animation
                .scalar_keys()
                .is_none_or(|keys| keys.iter().all(|key| spec.holds(key.value))),
            "{owner} {property:?} keys must be finite and inside Premiere's bounds"
        );
        animation.validate_keys()?;
    }
    Ok(())
}

/// Checks that an object transform is finite and inside Premiere's Scale,
/// Rotation and Opacity bounds.
fn validate_transform(transform: &PrTextTransform, owner: &str) -> crate::format::Result<()> {
    crate::format::ensure_valid!(
        transform
            .position
            .iter()
            .chain(&transform.anchor)
            .chain([&transform.scale, &transform.rotation])
            .all(|value| value.is_finite())
            && (0.0..=4000.0).contains(&transform.scale)
            && (-32768.0..=32767.0).contains(&transform.rotation)
            && (0.0..=100.0).contains(&transform.opacity),
        "{owner} transform must be finite, with Premiere's scale, rotation and opacity bounds"
    );
    Ok(())
}

/// Checks that a text or shape stroke `width` is finite and nonnegative,
/// which neither payload encoding bounds.
pub(crate) fn validate_stroke_width(width: f32, owner: &str) -> crate::format::Result<()> {
    crate::format::ensure_valid!(
        width.is_finite() && width >= 0.0,
        "{owner} stroke width must be finite and nonnegative"
    );
    Ok(())
}

/// The editable FX `(font_family, font_style)` of an empty Text object that
/// Premiere saved without a font (a Source Text without runs): FX's font
/// for new text, so that the object takes text without a font edit. Premiere
/// saved no font for it; the reader reports the substitution.
pub(crate) const EMPTY_TEXT_FONT: [&str; 2] = ["Inter", "Regular"];

/// Maps CR and CRLF paragraph breaks to the model's LF.
pub(crate) fn normalize_line_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

impl PrTextDocument {
    /// Checks value ranges that the encodings cannot express and the limits of
    /// the supported subset, for both conversion directions.
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        let document = self;
        crate::format::ensure_valid!(
            !document
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
            "text contains unsupported control characters"
        );
        crate::format::ensure_valid!(
            document.size.is_finite() && document.size > 0.0,
            "text size must be positive and finite"
        );
        crate::format::ensure_valid!(
            document.tracking.is_finite() && document.leading.is_finite(),
            "text tracking and leading must be finite"
        );
        // The FX renderer centers its stroke on the outline, which differs from
        // Premiere's outside stroke once no fill covers the inner half.
        crate::format::ensure_valid!(
            document.fill.is_some() || document.stroke.is_none(),
            "outline-only text (stroke without fill) is unsupported"
        );
        // The FX renderer does not space lines closer than 0.8 em (1.2 em auto - 0.4 em).
        let line_spacing_supported = f64::from(document.leading) >= -0.4 * f64::from(document.size);
        crate::format::ensure_valid!(
            line_spacing_supported,
            "text line spacing below 0.8 em is unsupported"
        );
        // Zero characters draw no glyph: an empty Text object that Premiere
        // saves without runs names no font.
        if !(document.text.is_empty() && document.font.is_empty()) {
            document.validate_font()?;
        }
        if let Some(stroke) = &document.stroke {
            validate_stroke_width(stroke.width, "text")?;
        }
        if let Some(background) = &document.background {
            crate::format::ensure_valid!(
                (0.0..=100.0).contains(&background.opacity)
                    && background.size.is_finite()
                    && background.size >= 0.0
                    && background.radius.is_finite()
                    && background.radius >= 0.0,
                "text background opacity must be a percentage and its size and radius finite and nonnegative"
            );
        }
        if let PrTextFrame::Box { width, height, .. } = document.frame {
            crate::format::ensure_valid!(
                width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0,
                "text box size must be positive and finite"
            );
        }
        Ok(())
    }

    /// Checks the font as a PostScript name. The writer stores every text as
    /// one run that names its font, so an exported text needs one even when
    /// it has no characters.
    pub(crate) fn validate_font(&self) -> crate::format::Result<()> {
        crate::format::ensure_valid!(
            !self.font.is_empty() && !self.font.contains('\0'),
            "text font must be a nonempty PostScript name"
        );
        // FX font keys join family and style with '/', and a PostScript
        // name (OpenType name ID 6) cannot contain '/'.
        crate::format::ensure_valid!(
            !self.font.contains('/'),
            "text font {:?} contains '/', so it is not a PostScript name and cannot form a family/style key",
            self.font
        );
        Ok(())
    }
}

impl PrText {
    /// The fields in which the Source Text key documents differ, when an FX
    /// text layer animates each of them; otherwise the first differing field
    /// that none does. Fixed fields are compared with the document; fills
    /// and strokes are compared among the keys that have one, wherever the
    /// switch is off in between. Enabled strokes must share their color;
    /// width changes use an all-character text animator.
    pub(crate) fn keyed_fields(&self) -> crate::format::Result<BTreeSet<SourceTextField>> {
        let first = &self.document;
        let keys = self.source_text_keys.iter().map(|key| &key.document);
        let first_stroke = keys.clone().find_map(|key| key.stroke);
        let fixed = [
            (keys.clone().any(|key| key.font != first.font), "font"),
            (
                keys.clone()
                    .any(|key| key.justification != first.justification),
                "justification",
            ),
            (keys.clone().any(|key| key.frame != first.frame), "box"),
            (keys.clone().any(|key| key.shadow != first.shadow), "shadow"),
            (
                keys.clone()
                    .filter_map(|key| key.stroke)
                    .any(|stroke| Some(stroke.color) != first_stroke.map(|stroke| stroke.color)),
                "stroke color",
            ),
        ];
        if let Some((_, field)) = fixed.iter().find(|(differs, _)| *differs) {
            return Err(crate::format::invalid(format!(
                "Source Text keys change the {field}, which no FX text track animates"
            )));
        }
        use SourceTextField::*;
        let first_fill = keys.clone().find_map(|key| key.fill);
        Ok([
            (keys.clone().any(|key| key.text != first.text), Text),
            (keys.clone().any(|key| key.size != first.size), Size),
            (
                keys.clone()
                    .any(|key| key.fill.is_some() != first.fill.is_some()),
                FillEnabled,
            ),
            (
                keys.clone()
                    .filter_map(|key| key.fill)
                    .any(|fill| Some(fill) != first_fill),
                FillColor,
            ),
            (
                keys.clone().any(|key| key.tracking != first.tracking),
                Tracking,
            ),
            (
                keys.clone().any(|key| key.leading != first.leading),
                Leading,
            ),
            (
                keys.clone()
                    .any(|key| key.stroke.is_some() != first.stroke.is_some()),
                StrokeEnabled,
            ),
            (
                keys.clone()
                    .filter_map(|key| key.stroke)
                    .any(|stroke| Some(stroke.width) != first_stroke.map(|stroke| stroke.width)),
                StrokeWidth,
            ),
            (
                keys.clone().any(|key| key.all_caps != first.all_caps),
                AllCaps,
            ),
        ]
        .into_iter()
        .filter_map(|(differs, field)| differs.then_some(field))
        .collect())
    }

    /// Checks the document, each Source Text key, the transform and the
    /// keys of the transform, for both conversion directions.
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        self.document.validate()?;
        let keys = &self.source_text_keys;
        crate::format::ensure_valid!(
            keys.windows(2)
                .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
            "Source Text keys must have strictly increasing source times"
        );
        crate::format::ensure_valid!(
            keys.first()
                .is_none_or(|first| first.document == self.document),
            "the Source Text before the first key must be the first key's document"
        );
        for key in keys {
            key.document.validate()?;
        }
        self.keyed_fields()?;
        crate::format::ensure_valid!(
            self.horizontal_scale
                .is_none_or(|scale| TEXT_PARAMS[3].holds(scale)),
            "text Horizontal Scale must be within Premiere's bounds"
        );
        validate_transform(&self.transform, "text")?;
        validate_animations(&self.animations, &TEXT_PARAMS, "text")
    }

    /// Folds a static Vector Motion into this text layer's transform and keys.
    ///
    /// Vector Motion is a similarity transform, with clockwise rotation in
    /// y-down frame coordinates. Its uniform scale multiplies both text axes,
    /// including a separately held Horizontal Scale. So the text draws the same: its position, the
    /// position keys and their spatial tangents move through the Vector
    /// Motion, scale and its keys multiply, and rotation and its keys add.
    /// Key times and easing do not change. `motion` must have no keys.
    pub(crate) fn compose_static_vector_motion(
        &mut self,
        motion: &PrVectorMotion,
        frame: [u32; 2],
    ) {
        debug_assert!(
            motion.animations.is_empty(),
            "only static Vector Motion composes"
        );
        let similarity = Similarity::of(motion);
        similarity.compose(&mut self.transform);
        if let Some(scale) = &mut self.horizontal_scale {
            *scale *= similarity.scale;
        }
        let size = frame.map(f64::from);
        for animation in &mut self.animations {
            match animation {
                PrPropertyAnimation::Position(keys) => {
                    for key in keys {
                        let point =
                            similarity.place([key.value[0] * size[0], key.value[1] * size[1]]);
                        key.value = [point[0] / size[0], point[1] / size[1]];
                        for tangent in [&mut key.spatial_in_tangent, &mut key.spatial_out_tangent]
                            .into_iter()
                            .flatten()
                        {
                            let vector =
                                similarity.turn([tangent[0] * size[0], tangent[1] * size[1]]);
                            *tangent = [vector[0] / size[0], vector[1] / size[1]];
                        }
                    }
                }
                PrPropertyAnimation::UniformScale(keys) => {
                    keys.iter_mut()
                        .for_each(|key| key.value *= similarity.scale);
                }
                PrPropertyAnimation::Rotation(keys) => {
                    keys.iter_mut().for_each(|key| key.value += motion.rotation);
                }
                PrPropertyAnimation::Opacity(_) => {}
                // No text keys these (`validate_animations`).
                PrPropertyAnimation::AnchorPoint(_) | PrPropertyAnimation::ScaleWidth(_) => {}
            }
        }
    }
}

/// Synthetic media used by every Type-tool graphic.
pub(crate) const GRAPHIC_IMPLEMENTATION_ID: &str = "42008e7a-de6f-4270-96de-7e287abb9b4b";
/// The generator token Premiere stores where a file path would go.
pub(crate) const GRAPHIC_MEDIA_TOKEN: &str = "1196574294";
/// The generator's intrinsic duration: 12 hours of synthetic time.
pub(crate) const GRAPHIC_MEDIA_TICKS: i64 = 10_973_491_200_000_000;
pub(crate) const GRAPHIC_CODEC_TYPE: &str = "1431194446";
pub(crate) const GRAPHIC_NAME: &str = "Graphic";
pub(crate) const TEXT_MATCH_NAME: &str = "AE.ADBE Text";
pub(crate) const VECTOR_MOTION_MATCH_NAME: &str = "AE.ADBE Graphic Group";
pub(crate) const SHAPE_MATCH_NAME: &str = "AE.ADBE Shape";
/// A group of graphic objects, which the model does not have.
pub(crate) const SUBGROUP_MATCH_NAME: &str = "AE.ADBE Graphic SubGroup";
/// The Text component's private data, identical in every observed graphic.
pub(crate) const TEXT_PRIVATE_DATA: (&str, &str) = ("c40c6399-6b26-8c2c-feaf-d01b0000000d", "AA==");
/// The Shape component's private data: 16 zero bytes, as the calibration
/// projects wrote it, under the content hash that Premiere 26.5.1 saved for
/// it. Saved templates hold other values that look uninitialized.
pub(crate) const SHAPE_PRIVATE_DATA: (&str, &str) = (
    "036b9652-ba57-8e23-9514-59270000001c",
    "AAAAAAAAAAAAAAAAAAAAAA==",
);

// Premiere 26.3 and 26.5 write graphic component records with these versions,
// which differ from the older intrinsic Motion records.
pub(crate) const TEXT_FILTER_COMPONENT: XmlRecordDefinition = XmlRecordDefinition::new(
    records::VIDEO_FILTER_COMPONENT.tag,
    records::VIDEO_FILTER_COMPONENT.class_id,
    "9",
);
pub(crate) const TEXT_FILTER_BODY_VERSION: &str = "7";
pub(crate) const SOURCE_TEXT_PARAM: XmlRecordDefinition = XmlRecordDefinition::new(
    "ArbVideoComponentParam",
    "313e54d4-6903-49ad-b0bf-8262cdd10f4e",
    "3",
);
const SCALAR: XmlRecordDefinition = XmlRecordDefinition::new(
    records::VIDEO_COMPONENT_PARAM.tag,
    records::VIDEO_COMPONENT_PARAM.class_id,
    "10",
);
const BOOL: XmlRecordDefinition = XmlRecordDefinition::new(
    records::VIDEO_BOOL_COMPONENT_PARAM.tag,
    records::VIDEO_BOOL_COMPONENT_PARAM.class_id,
    "10",
);
const SLIDER: XmlRecordDefinition = XmlRecordDefinition::new(
    records::VIDEO_FILTER_AMOUNT_PARAM.tag,
    records::VIDEO_FILTER_AMOUNT_PARAM.class_id,
    "10",
);
const POINT: XmlRecordDefinition = XmlRecordDefinition::new(
    records::POINT_COMPONENT_PARAM.tag,
    records::POINT_COMPONENT_PARAM.class_id,
    "4",
);

/// What the converter does with one graphic transform parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphicParamRole {
    Position,
    Scale,
    Rotation,
    Opacity,
    Anchor,
    /// A Shape's Horizontal Scale, which applies while Uniform Scale is off.
    HorizontalScale,
    /// A Shape's Uniform Scale switch.
    Uniform,
    /// The Essential Graphics text selection; editor state, not rendering.
    Selection,
    /// Must keep its default value.
    Fixed,
}

impl GraphicParamRole {
    /// The property that a parameter of this role can key. Keys on any other
    /// parameter omit the graphic.
    pub(crate) fn animation(self) -> Option<PrAnimatedProperty> {
        match self {
            Self::Position => Some(PrAnimatedProperty::Position),
            Self::Scale => Some(PrAnimatedProperty::UniformScale),
            Self::Rotation => Some(PrAnimatedProperty::Rotation),
            Self::Opacity => Some(PrAnimatedProperty::Opacity),
            Self::Anchor
            | Self::HorizontalScale
            | Self::Uniform
            | Self::Selection
            | Self::Fixed => None,
        }
    }
}

pub(crate) struct GraphicParamSpec {
    pub(crate) id: usize,
    pub(crate) name: Option<&'static str>,
    pub(crate) record: XmlRecordDefinition,
    pub(crate) control: Option<&'static str>,
    pub(crate) initial: &'static str,
    pub(crate) lower: Option<&'static str>,
    pub(crate) upper: Option<&'static str>,
    /// The slider range that Premiere stores with Vector Motion scale.
    pub(crate) upper_ui: Option<&'static str>,
    pub(crate) role: GraphicParamRole,
    /// Whether Bezier keys of this parameter convert. A Bezier key stores its
    /// handles as speeds, and a Premiere 26.5.1 probe measured them in value
    /// per second, the clip Motion unit, for Text Scale and Opacity and
    /// Vector Motion Scale and Rotation only.
    pub(crate) bezier_speeds_verified: bool,
}

impl GraphicParamSpec {
    pub(crate) fn is_point(&self) -> bool {
        self.record.tag == records::POINT_COMPONENT_PARAM.tag
    }

    /// Whether `value` is finite and inside this parameter's native bounds.
    pub(crate) fn holds(&self, value: f64) -> bool {
        let bound = |bound: Option<&str>| bound.and_then(|bound| bound.parse::<f64>().ok());
        value.is_finite()
            && bound(self.lower).is_none_or(|lower| value >= lower)
            && bound(self.upper).is_none_or(|upper| value <= upper)
    }

    const fn with_upper_ui(mut self, bound: &'static str) -> Self {
        self.upper_ui = Some(bound);
        self
    }

    const fn with_verified_bezier_speeds(mut self) -> Self {
        self.bezier_speeds_verified = true;
        self
    }

    /// Match a default without imposing the writer's number formatting.
    pub(crate) fn accepts_default(&self, value: &str) -> bool {
        match (value.parse::<f64>(), self.initial.parse::<f64>()) {
            (Ok(actual), Ok(expected)) => actual == expected,
            _ => value == self.initial,
        }
    }
}

const fn param(
    id: usize,
    name: Option<&'static str>,
    record: XmlRecordDefinition,
    control: Option<&'static str>,
    initial: &'static str,
    bounds: (Option<&'static str>, Option<&'static str>),
    role: GraphicParamRole,
) -> GraphicParamSpec {
    GraphicParamSpec {
        id,
        name,
        record,
        control,
        initial,
        lower: bounds.0,
        upper: bounds.1,
        upper_ui: None,
        role,
        bezier_speeds_verified: false,
    }
}

use GraphicParamRole::{
    Anchor, Fixed, HorizontalScale, Opacity, Position, Rotation, Scale, Selection, Uniform,
};

pub(crate) const TEXT_PARAM_COUNT: usize = 21;

/// Legacy static Text ends at Parent Rotation (ID 21). The later unnamed
/// false Boolean ID 22 has no control in that layout; all earlier masking,
/// edge and parent controls remain present and must keep their defaults.
pub(crate) const LEGACY_TEXT_PARAM_COUNT: usize = 20;

/// Text component parameters after Source Text (ParameterID 1), in native order.
/// Unnamed booleans and sliders are responsive-design and masking controls.
pub(crate) const TEXT_PARAMS: [GraphicParamSpec; TEXT_PARAM_COUNT] = [
    param(
        2,
        Some("Transform"),
        BOOL,
        Some("11"),
        "false",
        (None, Some("false")),
        Fixed,
    ),
    param(
        3,
        Some("Position"),
        POINT,
        None,
        "0.5:0.5",
        (None, None),
        Position,
    ),
    param(
        4,
        Some("Scale"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("4000")),
        Scale,
    )
    .with_verified_bezier_speeds(),
    param(
        5,
        Some("Horizontal Scale"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("4000")),
        HorizontalScale,
    ),
    param(6, Some(" "), BOOL, None, "true", (None, None), Uniform),
    param(
        7,
        Some("Rotation"),
        SCALAR,
        Some("3"),
        "0.",
        (Some("-32768"), Some("32767")),
        Rotation,
    ),
    param(
        8,
        Some("Opacity"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("100")),
        Opacity,
    )
    .with_verified_bezier_speeds(),
    param(
        9,
        Some("Anchor Point"),
        POINT,
        None,
        "0:0",
        (None, None),
        Anchor,
    ),
    param(
        10,
        None,
        BOOL,
        Some("12"),
        "false",
        (None, Some("false")),
        Fixed,
    ),
    param(
        11,
        Some(" "),
        SLIDER,
        None,
        "0.",
        (Some("0"), Some("32768")),
        Fixed,
    ),
    param(
        12,
        Some(" "),
        SLIDER,
        None,
        "0.",
        (Some("0"), Some("32768")),
        Fixed,
    ),
    param(
        13,
        Some("start"),
        SLIDER,
        None,
        "-1.",
        (Some("-100"), Some("1000000000")),
        Selection,
    ),
    param(
        14,
        Some("end"),
        SLIDER,
        None,
        "-1.",
        (Some("-100"), Some("1000000000")),
        Selection,
    ),
    param(15, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(16, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(17, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(18, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(
        19,
        Some("Parent Width"),
        SCALAR,
        None,
        "0.",
        (Some("0"), Some("20000")),
        Fixed,
    ),
    param(
        20,
        Some("Parent Height"),
        SCALAR,
        None,
        "0.",
        (Some("0"), Some("20000")),
        Fixed,
    ),
    param(
        21,
        Some("Parent Rotation"),
        SCALAR,
        Some("3"),
        "0.",
        (Some("-32768"), Some("32767")),
        Fixed,
    ),
    param(22, Some(" "), BOOL, None, "false", (None, None), Fixed),
];

pub(crate) const VECTOR_MOTION_PARAM_COUNT: usize = 6;

/// The graphic's intrinsic Vector Motion. Position and anchor are normalized
/// to the sequence frame; the reader composes a static one into the text
/// transform.
pub(crate) const VECTOR_MOTION_PARAMS: [GraphicParamSpec; VECTOR_MOTION_PARAM_COUNT] = [
    param(
        1,
        Some("Position"),
        POINT,
        None,
        "0.5:0.5",
        (None, None),
        Position,
    ),
    param(
        2,
        Some("Scale"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("10000")),
        Scale,
    )
    .with_upper_ui("200")
    .with_verified_bezier_speeds(),
    param(
        3,
        Some("Scale Width"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("10000")),
        Fixed,
    )
    .with_upper_ui("200"),
    param(4, Some(" "), BOOL, None, "true", (None, None), Fixed),
    param(
        5,
        Some("Rotation"),
        SCALAR,
        Some("3"),
        "0.",
        (Some("-32768"), Some("32767")),
        Rotation,
    )
    .with_verified_bezier_speeds(),
    param(
        6,
        Some("Anchor Point"),
        POINT,
        None,
        "0.5:0.5",
        (None, None),
        Anchor,
    ),
];

pub(crate) const SHAPE_PARAM_COUNT: usize = 16;

/// Shape component parameters after Path (ParameterID 1) and Appearance (2),
/// in native order, as Premiere 26.5.1 saves a Shape.
pub(crate) const SHAPE_PARAMS: [GraphicParamSpec; SHAPE_PARAM_COUNT] = [
    param(
        3,
        Some("Transform"),
        BOOL,
        Some("11"),
        "false",
        (None, Some("false")),
        Fixed,
    ),
    param(
        4,
        Some("Position"),
        POINT,
        None,
        "0.5:0.5",
        (None, None),
        Position,
    ),
    param(
        5,
        Some("Scale"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("4000")),
        Scale,
    ),
    param(
        6,
        Some("Horizontal Scale"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("4000")),
        HorizontalScale,
    ),
    param(7, Some(" "), BOOL, None, "true", (None, None), Uniform),
    param(
        8,
        Some("Rotation"),
        SCALAR,
        Some("3"),
        "0.",
        (Some("-32768"), Some("32767")),
        Rotation,
    ),
    param(
        9,
        Some("Opacity"),
        SCALAR,
        None,
        "100.",
        (Some("0"), Some("100")),
        Opacity,
    ),
    param(
        10,
        Some("Anchor Point"),
        POINT,
        None,
        "0:0",
        (None, None),
        Anchor,
    ),
    param(
        11,
        None,
        BOOL,
        Some("12"),
        "false",
        (None, Some("false")),
        Fixed,
    ),
    param(12, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(13, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(14, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(15, Some(" "), BOOL, None, "false", (None, None), Fixed),
    param(
        16,
        Some("Parent Width"),
        SCALAR,
        None,
        "0.",
        (Some("0"), Some("20000")),
        Fixed,
    ),
    param(
        17,
        Some("Parent Height"),
        SCALAR,
        None,
        "0.",
        (Some("0"), Some("20000")),
        Fixed,
    ),
    param(
        18,
        Some("Parent Rotation"),
        SCALAR,
        Some("3"),
        "0.",
        (Some("-32768"), Some("32767")),
        Fixed,
    ),
];

#[cfg(test)]
mod tests {
    use super::{GraphicParamRole, SHAPE_PARAMS, TEXT_PARAMS, VECTOR_MOTION_PARAMS};

    #[test]
    fn graphic_parameter_layouts_are_dense_and_defaults_are_compatible() {
        for (layout, first) in [
            (&TEXT_PARAMS[..], 2),
            (&VECTOR_MOTION_PARAMS[..], 1),
            (&SHAPE_PARAMS[..], 3),
        ] {
            for (offset, spec) in layout.iter().enumerate() {
                assert_eq!(spec.id, first + offset);
                assert!(spec.accepts_default(spec.initial));
                assert!(!spec.accepts_default("not a default"));
            }
        }
        let roles = |layout: &[super::GraphicParamSpec]| -> Vec<usize> {
            layout
                .iter()
                .filter(|spec| spec.role != GraphicParamRole::Fixed)
                .map(|spec| spec.id)
                .collect()
        };
        assert_eq!(roles(&TEXT_PARAMS), [3, 4, 5, 6, 7, 8, 9, 13, 14]);
        // Shape Position, Scale, Horizontal Scale, Uniform Scale, Rotation,
        // Opacity and Anchor Point.
        assert_eq!(roles(&SHAPE_PARAMS), [4, 5, 6, 7, 8, 9, 10]);
        assert!(TEXT_PARAMS[2].accepts_default("100"));
        // Keys: Text Position, Scale, Rotation and Opacity; Vector Motion
        // Position, Scale and Rotation. Anchors and Scale Width stay static.
        let keyable = |layout: &[super::GraphicParamSpec]| -> Vec<usize> {
            layout
                .iter()
                .filter(|spec| spec.role.animation().is_some())
                .map(|spec| spec.id)
                .collect()
        };
        assert_eq!(keyable(&TEXT_PARAMS), [3, 4, 7, 8]);
        assert_eq!(keyable(&VECTOR_MOTION_PARAMS), [1, 2, 5]);
        // Bezier keys: the parameters whose speed unit the Premiere 26.5.1
        // probe measured, and no Position or Text Rotation.
        let bezier = |layout: &[super::GraphicParamSpec]| -> Vec<usize> {
            layout
                .iter()
                .filter(|spec| spec.bezier_speeds_verified)
                .map(|spec| spec.id)
                .collect()
        };
        assert_eq!(bezier(&TEXT_PARAMS), [4, 8]);
        assert_eq!(bezier(&VECTOR_MOTION_PARAMS), [2, 5]);
        let scale = &TEXT_PARAMS[2];
        assert!(scale.holds(4000.0) && !scale.holds(4000.5) && !scale.holds(-1.0));
        assert!(!scale.holds(f64::NAN));
    }

    #[test]
    fn shapes_keep_stroke_widths_and_horizontal_scales_inside_their_bounds() {
        use super::{PrGraphicObject, PrRgb, PrShapeStroke};
        let validated = |width: f32, horizontal_scale: f64| {
            let mut graphic = crate::tests::support::shape_graphic();
            let [PrGraphicObject::Shape(shape)] = graphic.objects.as_mut_slice() else {
                unreachable!("the test graphic holds one shape");
            };
            let color = PrRgb([0, 255, 64]);
            shape.appearance.stroke = Some(PrShapeStroke { color, width });
            shape.horizontal_scale = Some(horizontal_scale);
            shape.validate().map_err(|error| error.to_string())
        };
        assert!(validated(0.0, 0.0).is_ok() && validated(32.0, 4000.0).is_ok());
        for (width, horizontal_scale, reason) in [
            (
                -1.0,
                100.0,
                "shape stroke width must be finite and nonnegative",
            ),
            (
                f32::NAN,
                100.0,
                "shape stroke width must be finite and nonnegative",
            ),
            (
                32.0,
                4000.5,
                "shape Horizontal Scale must be within Premiere's bounds",
            ),
            (
                32.0,
                f64::NAN,
                "shape Horizontal Scale must be within Premiere's bounds",
            ),
        ] {
            let error = validated(width, horizontal_scale).unwrap_err();
            assert!(error.contains(reason), "{error}");
        }
    }
}
