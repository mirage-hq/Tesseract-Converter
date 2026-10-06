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

pub(super) fn is_outline_only(document: &crate::schema::text::PrTextDocument) -> bool {
    document.fill.is_none() && document.stroke.is_some()
}

/// Premiere draws a stroke of width `w` outside the glyph outline. The FX renderer
/// centers its stroke on the outline and paints the fill over it, so the same
/// visible stroke has width `2w`.
pub(super) const STROKE_WIDTH_RATIO: f64 = 2.0;

/// A filled glyph guide cuts the interior out of the centered stroke. Both
/// documents remain editable; glyph-layout edits must keep them in agreement.
pub(super) const OUTLINE_ONLY_GLYPH_CUTOUT: &str = "outline-only text uses an editable filled-glyph cutout; text, font and layout edits must be applied to both Text children";

/// Fill only the documents whose stroke would otherwise paint inside the glyph.
/// The guide shares every authored glyph-layout field and Source Text clock,
/// but never contributes visible paint, a shadow or a stroke of its own.
/// [`super::graphic::import_graphic`] paints a supported text background on
/// the parent Group, outside the child Text's matte, so the guide must not
/// duplicate that background.
pub(super) fn glyph_interior(text: &crate::schema::text::PrText) -> crate::schema::text::PrText {
    let mut guide = text.clone();
    guide.animations.clear();
    guide.mask_source = None;
    for document in std::iter::once(&mut guide.document).chain(
        guide
            .source_text_keys
            .iter_mut()
            .map(|key| &mut key.document),
    ) {
        document.fill = if is_outline_only(document) {
            document.stroke.map(|stroke| stroke.color)
        } else {
            None
        };
        document.stroke = None;
        document.shadow = None;
        document.background = None;
    }
    guide
}
