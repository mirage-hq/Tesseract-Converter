//! Small wire-format enums shared across layer kinds (blend modes, text
//! justification, matte sampling, and media fit/placement).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Vector2Property;

/// Blend operation used when compositing a layer into its parent stack.
///
/// Mirrors [`scene::BlendMode`] one-to-one — every variant has a direct
/// scene counterpart of the same name, so the `From<BlendMode>` conversion
/// to `scene::BlendMode` is a pure 1:1 match with no fallback path.
/// (`impl` blocks aren't intra-doc link targets, so the conversion is
/// referenced as a code span rather than a link.)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Add,
    // Extended After Effects blend modes (JRB-1460). Names/wire forms mirror
    // AE's `BlendingMode` enum (camelCase of the AE UI label).
    ClassicColorBurn,
    ClassicColorDodge,
    ClassicDifference,
    LinearBurn,
    DarkerColor,
    LighterColor,
    LinearLight,
    VividLight,
    PinLight,
    HardMix,
    Subtract,
    Divide,
}

/// Paragraph justification for source text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum Justification {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Vertical alignment of box text within its box (Figma `textAlignVertical`
/// TOP/CENTER/BOTTOM; AE paragraph-text vertical justification).
///
/// Only meaningful when [`TextDocument::box_text`](super::TextDocument::box_text) is set — point text has no
/// box to align within. Absent (`None` on the document) keeps the legacy
/// libpag *AdjustToFitBox* behavior: centered when an explicit `leading` is
/// set and the text has fewer lines than the box holds, top otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum VerticalAlign {
    Top,
    Center,
    Bottom,
}

/// Which part of the type meets the guide curve on a text-on-path layer
/// (JRB-1819).
///
/// All three are measured on the cap band, so [`Self::Bottom`] *is*
/// baseline-on-curve — which is why there is no separate `Baseline` variant.
///
/// Deliberately NOT [`VerticalAlign`]: that field means "distribute lines
/// within the text box" and only exists on box text, while a path guide is
/// most often attached to *point* text. Overloading it would leave point text
/// with no control and give one stored value two meanings depending on whether
/// `pathOptions` happens to be set.
///
/// [`Self::Bottom`] is the serde default and resolves to a zero shift, so
/// every project authored before this field existed renders unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum TextPathAlign {
    /// Cap top on the curve; the type hangs below the guide.
    Top,
    /// Mid cap band on the curve.
    Center,
    /// Baseline on the curve — the pre-JRB-1819 placement, and the default.
    ///
    /// Declared last to match [`VerticalAlign`], which shares this member set —
    /// see the note on `scene::TextPathAlign` for why the order matters to the
    /// generated schema.
    #[default]
    Bottom,
}

impl TextPathAlign {
    /// Whether this is the default placement, for `skip_serializing_if` — see
    /// [`crate::TextPathOptions::align`] for why omitting the default matters
    /// to downstream readers.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Bottom)
    }
}

/// Sampling mode used by a track matte (which channel of the matte source
/// layer drives the masked layer's alpha).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum TrackMatteType {
    #[default]
    Alpha,
    AlphaInverted,
    Luma,
    LumaInverted,
}

/// A finite two-dimensional vector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(into = "Vector2Property", try_from = "Vector2Property")]
#[ts(type = "[number, number]")]
pub struct FiniteVec2(Vector2Property);

impl FiniteVec2 {
    /// Construct a vector when both components are finite.
    #[must_use]
    pub fn new(value: Vector2Property) -> Option<Self> {
        value
            .iter()
            .all(|component| component.is_finite())
            .then_some(Self(value))
    }

    /// Return the validated vector payload.
    #[must_use]
    pub const fn get(self) -> Vector2Property {
        self.0
    }
}

impl TryFrom<Vector2Property> for FiniteVec2 {
    type Error = &'static str;

    fn try_from(value: Vector2Property) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("components must be finite")
    }
}

impl From<FiniteVec2> for Vector2Property {
    fn from(value: FiniteVec2) -> Self {
        value.get()
    }
}

/// A finite two-dimensional vector whose components are strictly positive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(into = "Vector2Property", try_from = "Vector2Property")]
#[ts(type = "[number, number]")]
pub struct PositiveVec2(Vector2Property);

impl PositiveVec2 {
    /// Construct a vector when both components are finite and positive.
    #[must_use]
    pub fn new(value: Vector2Property) -> Option<Self> {
        value
            .iter()
            .all(|component| component.is_finite() && *component > 0.0)
            .then_some(Self(value))
    }

    /// Return the validated vector payload.
    #[must_use]
    pub const fn get(self) -> Vector2Property {
        self.0
    }
}

impl TryFrom<Vector2Property> for PositiveVec2 {
    type Error = &'static str;

    fn try_from(value: Vector2Property) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("components must be finite and strictly positive")
    }
}

impl From<PositiveVec2> for Vector2Property {
    fn from(value: PositiveVec2) -> Self {
        value.get()
    }
}

/// Declare the media-fit variants once while allowing the product reader to migrate legacy `none`.
#[doc(hidden)]
#[macro_export]
macro_rules! define_media_fit_schema {
    (none: [$($none_variant:tt)*], contain: [$($contain_attr:tt)*], reader: [$($reader:tt)*]) => {
    /// Responsive media layout policy or explicit authored content geometry.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
    #[serde(rename_all = "camelCase" $($reader)*)]
    #[ts(export_to = "project_types.d.ts")]
    pub enum MediaFit {
        Stretch,
        $($none_variant)*
        /// Scale the source to fit entirely inside the target bounds.
        #[default]
        $($contain_attr)*
        Contain,
        /// Scale the source to cover the target bounds.
        Cover,
        /// Explicit layer-local content geometry. `contentCenter` is absolute,
        /// not relative to the frame, so changing the frame does not move Custom
        /// content.
        Custom {
            scale: PositiveVec2,
            #[serde(rename = "contentCenter")]
            content_center: FiniteVec2,
        },
    }

    impl MediaFit {
        /// Construct validated explicit media geometry.
        pub fn custom(
            scale: Vector2Property,
            content_center: Vector2Property,
        ) -> Result<Self, &'static str> {
            let scale = PositiveVec2::new(scale)
                .ok_or("Custom scale components must be finite and strictly positive")?;
            let content_center =
                FiniteVec2::new(content_center).ok_or("Custom content center must be finite")?;
            Ok(Self::Custom {
                scale,
                content_center,
            })
        }
    }

    };
}

define_media_fit_schema! {
    none: [/// Historical fit value, retained without interpretation.
        None,],
    contain: [],
    reader: []
}

/// Half-canvas placement of a media layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts", rename_all = "camelCase")]
pub enum MediaPlacement {
    /// The overlay occupies the top half-canvas region `(W, H/2)` at `y = 0`;
    /// the base video parks in the bottom half.
    TopHalf,
    /// The overlay occupies the bottom half-canvas region `(W, H/2)` at
    /// `y = H/2`; the base video parks in the top half.
    BottomHalf,
}

#[cfg(test)]
mod tests {
    use super::MediaFit;
    use crate::VideoSource;

    #[test]
    fn omitted_media_fit_defaults_to_contain() {
        let source: VideoSource = serde_json::from_value(serde_json::json!({
            "assetId": "video-asset"
        }))
        .expect("source without fit should deserialize");

        assert_eq!(source.fit, MediaFit::Contain);
    }

    #[test]
    fn legacy_none_media_fit_is_preserved() {
        let fit: MediaFit =
            serde_json::from_str(r#""none""#).expect("legacy fit should deserialize");

        assert_eq!(fit, MediaFit::None);
        assert_ne!(fit, MediaFit::default());
        assert_eq!(
            serde_json::to_string(&fit).expect("fit should serialize"),
            r#""none""#
        );
    }
}
