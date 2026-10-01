//! Text layout rules that relate Premiere and FX units in both directions.

/// Premiere spaces lines at 120% of the font size and adds its leading to
/// that. FX automatic leading may expand to the font's natural metrics, so
/// import always specifies the full baseline-to-baseline distance explicitly.
const AUTO_LINE_SPACING: f32 = 1.2;

/// Premiere's automatic line spacing, in its 32-bit precision. Both directions
/// add or subtract leading around this value in f64, so a round trip keeps
/// ordinary leading values exact.
pub(super) fn automatic_line_spacing(size: f32) -> f64 {
    f64::from(AUTO_LINE_SPACING * size)
}

pub(super) fn line_spacing(document: &crate::schema::text::PrTextDocument) -> f64 {
    automatic_line_spacing(document.size) + f64::from(document.leading)
}

/// The local anchor adjustment that puts a point block's aligned baseline at
/// its source origin. Every line break advances a line, a trailing one too.
pub(super) fn point_anchor_offset(document: &crate::schema::text::PrTextDocument) -> f64 {
    use crate::schema::text::PrTextFrame;
    match document.frame {
        PrTextFrame::Point { vertical } => {
            let advance = document.text.matches('\n').count() as f64 * line_spacing(document);
            vertical_offset(vertical, advance)
        }
        PrTextFrame::Box { .. } => 0.0,
    }
}

pub(super) fn vertical_offset(vertical: crate::schema::text::PrVerticalAlign, advance: f64) -> f64 {
    use crate::schema::text::PrVerticalAlign;
    match vertical {
        PrVerticalAlign::Top => 0.0,
        PrVerticalAlign::Center => advance / 2.0,
        PrVerticalAlign::Bottom => advance,
    }
}

/// Premiere draws a stroke of width `w` outside the glyph outline. The FX renderer
/// centers its stroke on the outline and paints the fill over it, so the same
/// visible stroke has width `2w`.
pub(super) const STROKE_WIDTH_RATIO: f64 = 2.0;
