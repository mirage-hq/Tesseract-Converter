//! Intrinsic Opacity layouts: the Premiere 26.3 layout that native XML conversion
//! shares, and the Premiere 26.5 layout that only the reader accepts.

use super::PrAnimatedProperty;
use fx_schema::BlendMode;

pub(crate) const OPACITY_PARAM_COUNT: usize = 3;

/// The Blend Mode of an intrinsic Opacity, from the values of its two Blend
/// Mode parameters (2, 3), the same in both layouts. Every placement that
/// carries an Opacity reads it: a media or nest occurrence and a graphic clip.
///
/// Measured on an AME render of a Premiere 26.5.1 save (VFC Version 9,
/// Component Version 7; `premiere_isolated_blend_codes_26_5`): each mode's
/// parameter 2 code renders as that mode's formula on encoded values, at
/// Opacity 100 over opaque chart patches and for Screen at Opacity 50 too,
/// and parameter 2 alone selects the mode: (22, 0) and (1, 0) render as
/// (22, 10) and (1, 5). Export writes each mode's canonical pair, the
/// chart's; that the Blend Mode menu writes the same parameter 3 is
/// inferred. An export gate measured the written pairs on stills, and Screen
/// on a Color Matte, on a graphic of one shape at Opacity 50 and on an
/// adjustment layer, and Multiply on a nest. Inferred, not measured: older
/// layouts (the pinned 26.3-layout (22, 10) and (4, 7) renders agree), other
/// Opacity values, semi-transparent pixels, other modes on those hosts,
/// graphics of several objects, and other sequence colour settings.
/// Premiere's Dissolve (6) and code 27 are [`Self::Unmeasured`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PrBlendMode {
    #[default]
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    /// Premiere's Linear Dodge (Add).
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
    /// A parameter 2 code that no Adobe evidence names. It converts as
    /// Normal with its warning ([`Self::approximation`]); export never writes
    /// it.
    Unmeasured {
        primary: u8,
        legacy: u8,
    },
}

impl PrBlendMode {
    /// Every mode with a code, which `from_native_values` searches.
    const PAIRED: [Self; 26] = [
        Self::Normal,
        Self::Darken,
        Self::Multiply,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::DarkerColor,
        Self::Lighten,
        Self::Screen,
        Self::ColorDodge,
        Self::LinearDodge,
        Self::LighterColor,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Difference,
        Self::Exclusion,
        Self::Subtract,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    /// The values of the two Blend Mode parameters (2, 3) that export writes.
    pub(crate) const fn native_values(self) -> (u8, u8) {
        match self {
            Self::Normal => (18, 0),
            Self::Darken => (3, 3),
            Self::Multiply => (17, 4),
            Self::ColorBurn => (1, 5),
            Self::LinearBurn => (13, 6),
            Self::DarkerColor => (4, 7),
            Self::Lighten => (11, 9),
            Self::Screen => (22, 10),
            Self::ColorDodge => (2, 11),
            Self::LinearDodge => (14, 12),
            Self::LighterColor => (12, 13),
            Self::Overlay => (19, 15),
            Self::SoftLight => (23, 16),
            Self::HardLight => (8, 17),
            Self::VividLight => (24, 18),
            Self::LinearLight => (15, 19),
            Self::PinLight => (20, 20),
            Self::HardMix => (9, 21),
            Self::Difference => (5, 23),
            Self::Exclusion => (7, 24),
            Self::Subtract => (25, 25),
            Self::Divide => (26, 26),
            Self::Hue => (10, 28),
            Self::Saturation => (21, 29),
            Self::Color => (0, 30),
            Self::Luminosity => (16, 31),
            Self::Unmeasured { primary, legacy } => (primary, legacy),
        }
    }

    /// The mode of parameter 2's code, whatever parameter 3 holds.
    pub(crate) fn from_native_values(primary: u8, legacy: u8) -> Self {
        Self::PAIRED
            .into_iter()
            .find(|mode| mode.native_values().0 == primary)
            .unwrap_or(Self::Unmeasured { primary, legacy })
    }

    /// The FX blend mode that this converts as: Normal for an unmeasured code.
    pub(crate) const fn fx_mode(self) -> BlendMode {
        match self {
            Self::Normal | Self::Unmeasured { .. } => BlendMode::Normal,
            Self::Darken => BlendMode::Darken,
            Self::Multiply => BlendMode::Multiply,
            Self::ColorBurn => BlendMode::ColorBurn,
            Self::LinearBurn => BlendMode::LinearBurn,
            Self::DarkerColor => BlendMode::DarkerColor,
            Self::Lighten => BlendMode::Lighten,
            Self::Screen => BlendMode::Screen,
            Self::ColorDodge => BlendMode::ColorDodge,
            Self::LinearDodge => BlendMode::Add,
            Self::LighterColor => BlendMode::LighterColor,
            Self::Overlay => BlendMode::Overlay,
            Self::SoftLight => BlendMode::SoftLight,
            Self::HardLight => BlendMode::HardLight,
            Self::VividLight => BlendMode::VividLight,
            Self::LinearLight => BlendMode::LinearLight,
            Self::PinLight => BlendMode::PinLight,
            Self::HardMix => BlendMode::HardMix,
            Self::Difference => BlendMode::Difference,
            Self::Exclusion => BlendMode::Exclusion,
            Self::Subtract => BlendMode::Subtract,
            Self::Divide => BlendMode::Divide,
            Self::Hue => BlendMode::Hue,
            Self::Saturation => BlendMode::Saturation,
            Self::Color => BlendMode::Color,
            Self::Luminosity => BlendMode::Luminosity,
        }
    }

    /// The blend that writes the FX `mode`: its own mode, or for one of FX's
    /// classic After Effects modes the nearest Premiere mode, whose export
    /// reports the difference ([`Self::export_approximation`]).
    pub(crate) const fn from_fx_mode(mode: BlendMode) -> Self {
        match mode {
            BlendMode::Normal => Self::Normal,
            BlendMode::Darken => Self::Darken,
            BlendMode::Multiply => Self::Multiply,
            BlendMode::ColorBurn | BlendMode::ClassicColorBurn => Self::ColorBurn,
            BlendMode::LinearBurn => Self::LinearBurn,
            BlendMode::DarkerColor => Self::DarkerColor,
            BlendMode::Lighten => Self::Lighten,
            BlendMode::Screen => Self::Screen,
            BlendMode::ColorDodge | BlendMode::ClassicColorDodge => Self::ColorDodge,
            BlendMode::Add => Self::LinearDodge,
            BlendMode::LighterColor => Self::LighterColor,
            BlendMode::Overlay => Self::Overlay,
            BlendMode::SoftLight => Self::SoftLight,
            BlendMode::HardLight => Self::HardLight,
            BlendMode::VividLight => Self::VividLight,
            BlendMode::LinearLight => Self::LinearLight,
            BlendMode::PinLight => Self::PinLight,
            BlendMode::HardMix => Self::HardMix,
            // FX draws both as the absolute difference.
            BlendMode::Difference | BlendMode::ClassicDifference => Self::Difference,
            BlendMode::Exclusion => Self::Exclusion,
            BlendMode::Subtract => Self::Subtract,
            BlendMode::Divide => Self::Divide,
            BlendMode::Hue => Self::Hue,
            BlendMode::Saturation => Self::Saturation,
            BlendMode::Color => Self::Color,
            BlendMode::Luminosity => Self::Luminosity,
        }
    }

    /// The one warning of exporting the FX `mode` ([`Self::from_fx_mode`]):
    /// the written mode's own, or where FX's classic formula differs from the
    /// nearest Premiere mode's.
    pub(crate) fn export_approximation(mode: BlendMode) -> Option<String> {
        let edge = match mode {
            BlendMode::ClassicColorBurn => {
                "a black pixel over white renders white instead of black"
            }
            BlendMode::ClassicColorDodge => {
                "a white pixel over black renders black instead of white"
            }
            mode => return Self::from_fx_mode(mode).approximation(),
        };
        let (primary, legacy) = Self::from_fx_mode(mode).native_values();
        Some(format!(
            "FX {mode:?} exports as the nearest Premiere Blend Mode ({primary}, {legacy}), whose formula differs only where {edge}"
        ))
    }

    /// The one warning of a placement whose blend converts, in either
    /// direction, other than as its measured formula; `None` otherwise.
    ///
    /// FX's other formulas are the measured ones on encoded values, with
    /// W3C Soft Light (the chart fits it and Photoshop's alike) and W3C
    /// luminosity for Hue, Saturation, Color and Luminosity.
    pub(crate) fn approximation(self) -> Option<String> {
        let (primary, legacy) = self.native_values();
        let pick = match self {
            Self::Unmeasured { .. } => {
                return Some(format!(
                    "Blend Mode ({primary}, {legacy}) has no measured Premiere mode; converted as Normal, so the layer covers the tracks below instead of blending with them (content-dependent error)"
                ))
            }
            Self::DarkerColor => "Darker",
            Self::LighterColor => "Lighter",
            _ => return None,
        };
        Some(format!(
            "Blend Mode ({primary}, {legacy}) {pick} Color picks each pixel's layer by BT.709 luma in Premiere and by channel sum in FX, so pixels whose two orders differ show the other layer (9.9 levels mean error on the measured colour chart)"
        ))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrOpacityParamSpec {
    pub(crate) id: usize,
    pub(crate) name: &'static str,
    pub(crate) class_id: &'static str,
    pub(crate) control: Option<&'static str>,
    pub(crate) lower_bound: &'static str,
    pub(crate) upper_bound: &'static str,
    /// A second `UpperBound` the reader accepts: Premiere 26.5.1 re-saves an
    /// older Opacity record with its existing bound 26 on the primary Blend
    /// Mode (fixture `feature_opacity_masks_26_5_strict` clips A to D,
    /// `oracle/17/facts.md`); the bound is the enumeration length, not a value.
    /// The writer emits `upper_bound`.
    pub(crate) older_upper_bound: Option<&'static str>,
    pub(crate) animation: Option<PrAnimatedProperty>,
}

impl PrOpacityParamSpec {
    /// Whether a saved `UpperBound` is this parameter's.
    pub(crate) fn accepts_upper_bound(&self, bound: Option<&str>) -> bool {
        bound == Some(self.upper_bound) || (bound.is_some() && bound == self.older_upper_bound)
    }
}

pub(crate) const OPACITY_PARAMS: [PrOpacityParamSpec; OPACITY_PARAM_COUNT] = [
    PrOpacityParamSpec {
        id: 1,
        name: "Opacity",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: Some("2"),
        lower_bound: "0",
        upper_bound: "100",
        older_upper_bound: None,
        animation: Some(PrAnimatedProperty::Opacity),
    },
    PrOpacityParamSpec {
        id: 2,
        name: "Blend Mode",
        class_id: "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
        control: Some("10"),
        lower_bound: "0",
        upper_bound: "26",
        older_upper_bound: None,
        animation: None,
    },
    PrOpacityParamSpec {
        id: 3,
        name: "Blend Mode",
        class_id: "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
        control: Some("7"),
        lower_bound: "0",
        upper_bound: "31",
        older_upper_bound: None,
        animation: None,
    },
];

/// The Premiere 26.5 layout: no control type on Opacity and the legacy Blend
/// Mode, and one more primary Blend Mode (a re-saved older record keeps 26).
pub(crate) const OPACITY_PARAMS_26_5: [PrOpacityParamSpec; OPACITY_PARAM_COUNT] = [
    PrOpacityParamSpec {
        id: 1,
        name: "Opacity",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: None,
        lower_bound: "0",
        upper_bound: "100",
        older_upper_bound: None,
        animation: Some(PrAnimatedProperty::Opacity),
    },
    PrOpacityParamSpec {
        id: 2,
        name: "Blend Mode",
        class_id: "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
        control: Some("10"),
        lower_bound: "0",
        upper_bound: "27",
        older_upper_bound: Some("26"),
        animation: None,
    },
    PrOpacityParamSpec {
        id: 3,
        name: "Blend Mode",
        class_id: "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
        control: None,
        lower_bound: "0",
        upper_bound: "31",
        older_upper_bound: None,
        animation: None,
    },
];

#[cfg(test)]
mod tests {
    use super::PrBlendMode;
    use fx_schema::BlendMode;

    #[test]
    fn each_native_code_reads_writes_and_converts_as_its_mode() {
        use PrBlendMode as Pr;
        // The pairs of the `premiere_isolated_blend_codes_26_5` chart, each
        // with its measured mode, and whether FX's formula differs.
        for (pair, mode, fx, approximated) in [
            ((18, 0), Pr::Normal, BlendMode::Normal, false),
            ((3, 3), Pr::Darken, BlendMode::Darken, false),
            ((17, 4), Pr::Multiply, BlendMode::Multiply, false),
            ((1, 5), Pr::ColorBurn, BlendMode::ColorBurn, false),
            ((13, 6), Pr::LinearBurn, BlendMode::LinearBurn, false),
            ((4, 7), Pr::DarkerColor, BlendMode::DarkerColor, true),
            ((11, 9), Pr::Lighten, BlendMode::Lighten, false),
            ((22, 10), Pr::Screen, BlendMode::Screen, false),
            ((2, 11), Pr::ColorDodge, BlendMode::ColorDodge, false),
            ((14, 12), Pr::LinearDodge, BlendMode::Add, false),
            ((12, 13), Pr::LighterColor, BlendMode::LighterColor, true),
            ((19, 15), Pr::Overlay, BlendMode::Overlay, false),
            ((23, 16), Pr::SoftLight, BlendMode::SoftLight, false),
            ((8, 17), Pr::HardLight, BlendMode::HardLight, false),
            ((24, 18), Pr::VividLight, BlendMode::VividLight, false),
            ((15, 19), Pr::LinearLight, BlendMode::LinearLight, false),
            ((20, 20), Pr::PinLight, BlendMode::PinLight, false),
            ((9, 21), Pr::HardMix, BlendMode::HardMix, false),
            ((5, 23), Pr::Difference, BlendMode::Difference, false),
            ((7, 24), Pr::Exclusion, BlendMode::Exclusion, false),
            ((25, 25), Pr::Subtract, BlendMode::Subtract, false),
            ((26, 26), Pr::Divide, BlendMode::Divide, false),
            ((10, 28), Pr::Hue, BlendMode::Hue, false),
            ((21, 29), Pr::Saturation, BlendMode::Saturation, false),
            ((0, 30), Pr::Color, BlendMode::Color, false),
            ((16, 31), Pr::Luminosity, BlendMode::Luminosity, false),
        ] {
            assert_eq!(Pr::from_native_values(pair.0, pair.1), mode);
            assert_eq!(mode.native_values(), pair);
            assert_eq!(mode.fx_mode(), fx);
            assert_eq!(Pr::from_fx_mode(fx), mode, "{pair:?}");
            assert_eq!(Pr::export_approximation(fx), mode.approximation());
            assert_eq!(mode.approximation().is_some(), approximated, "{pair:?}");
        }
        // Parameter 2 alone selects the mode: the chart's (22, 0) and (1, 0)
        // render as Screen and Color Burn.
        assert_eq!(Pr::from_native_values(22, 0), Pr::Screen);
        assert_eq!(Pr::from_native_values(1, 0), Pr::ColorBurn);
        // Dissolve and code 27 are unmeasured: they keep their values, convert
        // as Normal with a report and never export.
        for pair in [(6, 1), (27, 0)] {
            let mode = Pr::from_native_values(pair.0, pair.1);
            assert_eq!(
                mode,
                Pr::Unmeasured {
                    primary: pair.0,
                    legacy: pair.1
                }
            );
            assert_eq!(mode.native_values(), pair);
            assert_eq!(mode.fx_mode(), BlendMode::Normal);
            assert!(mode.approximation().is_some());
        }
        // FX's classic After Effects modes export as the nearest Premiere mode,
        // reporting where their formulas differ.
        for (classic, nearest, approximated) in [
            (BlendMode::ClassicColorBurn, Pr::ColorBurn, true),
            (BlendMode::ClassicColorDodge, Pr::ColorDodge, true),
            (BlendMode::ClassicDifference, Pr::Difference, false),
        ] {
            assert_eq!(Pr::from_fx_mode(classic), nearest);
            assert_eq!(Pr::export_approximation(classic).is_some(), approximated);
        }
    }
}
