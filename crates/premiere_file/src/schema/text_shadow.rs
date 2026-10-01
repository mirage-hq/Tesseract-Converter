//! The drop shadow of a single-style text layer, shared by both conversion
//! directions.
//!
//! Premiere 26 keeps the shadow in the Source Text document
//! (`format::text_payload`). Values use Premiere's units: opacity in percent,
//! angle in degrees, distance in sequence pixels, and size and blur in
//! Premiere's own units. The FX mapping, its inferred parts and the
//! texts that can keep a shadow live in `convert::text_shadow`.

use super::text::PrRgb;

/// An enabled Premiere text shadow. A disabled shadow is not modeled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrTextShadow {
    pub(crate) color: PrRgb,
    /// Percent.
    pub(crate) opacity: f32,
    /// Degrees.
    pub(crate) angle: f32,
    /// Offset from the text, in pixels.
    pub(crate) distance: f32,
    /// Growth before blurring, in Premiere's units rather than pixels.
    pub(crate) size: f32,
    /// Softness, in Premiere's units rather than pixels.
    pub(crate) blur: f32,
}

impl PrTextShadow {
    /// Checks the value ranges that the Source Text encoding cannot express.
    ///
    /// Both conversion directions omit only the shadow when this fails, and
    /// the encoder refuses to write it.
    pub(crate) fn validate(&self) -> crate::format::Result<()> {
        crate::format::ensure_valid!(
            (0.0..=100.0).contains(&self.opacity),
            "text shadow opacity must be 0 to 100 percent"
        );
        crate::format::ensure_valid!(self.angle.is_finite(), "text shadow angle must be finite");
        for (field, value) in [
            ("distance", self.distance),
            ("size", self.size),
            ("blur", self.blur),
        ] {
            crate::format::ensure_valid!(
                value.is_finite() && value >= 0.0,
                "text shadow {field} must be finite and nonnegative"
            );
        }
        Ok(())
    }
}
