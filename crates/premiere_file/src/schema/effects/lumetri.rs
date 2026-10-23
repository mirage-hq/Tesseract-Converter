//! Native `AE.ADBE Lumetri` ("Lumetri Color") layout and the scalar FX
//! mappings: Basic Correction and Creative scalars to one canonical
//! `primaryGrade`, and the Vignette section to the FX `vignette`.
//!
//! The layout is the 130-parameter record list of the pinned
//! `human_lumetri_contrast.xml` (copied from a Premiere save), in its native
//! `Params` order, with that save's values as defaults except the three
//! controls its author edited (Exposure 1, Contrast 25, keyed Saturation),
//! which are reset to Lumetri's neutral 0, 0 and 100. The reader compares
//! every unmapped section against these defaults to diagnose lost grading;
//! the writer writes them for every section it does not map. The Premiere
//! 26.5.2 save `color_lumetri_basic.prproj` has the same 130 records, record
//! for record, apart from its authored Basic Correction values.
//!
//! The arbitrary-data sections that both Premiere and After Effects save in
//! the same layout (RGB Curves, Hue Saturation Curves, Color Wheels) are
//! decoded by [`aftereffects_file::lumetri`], and browsed Input LUT / Look
//! files by [`super::lumetri_lut`]. HSL Secondary and the remaining blobs are
//! compared against the defaults and diagnosed, never interpreted.

use super::{EffectParamBinding, EffectParamSpec, EffectSpec};
use crate::schema::records::XmlRecordDefinition;

/// Native match name of Lumetri Color.
pub(crate) const LUMETRI_MATCH_NAME: &str = "AE.ADBE Lumetri";

const FLOAT: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542",
    "10",
);
const CHECKBOX: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "cc12343e-f113-4d3b-ae05-b287db77d461",
    "10",
);
const POPUP: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
    "10",
);
const COLOUR: XmlRecordDefinition = XmlRecordDefinition::new(
    "VideoComponentParam",
    "0fde4e9f-f895-4ba3-b0fe-9a6feafda583",
    "10",
);
/// A Lumetri arbitrary-data (curve, wheel, key, path) parameter.
const BINARY: XmlRecordDefinition = XmlRecordDefinition::new(
    "ArbVideoComponentParam",
    "313e54d4-6903-49ad-b0bf-8262cdd10f4e",
    "3",
);

/// One pinned layout slot. Popups alone interpolate discontinuously, as in
/// the pinned save; an empty `name` is a record saved without `Name`.
const fn param(
    id: usize,
    name: &'static str,
    record: XmlRecordDefinition,
    control: &'static str,
    [lower_bound, upper_bound]: [&'static str; 2],
    [lower_ui_bound, upper_ui_bound]: [Option<&'static str>; 2],
) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record,
        control,
        lower_bound,
        upper_bound,
        lower_ui_bound,
        upper_ui_bound,
        // Const string comparison: the class of `POPUP`.
        discontinuous_interpolate: matches!(
            record.class_id.as_bytes(),
            b"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8"
        ),
        binding: None,
    }
}

/// A saved default: a scalar's complete `StartKeyframe`, or a binary
/// parameter's `BinaryHash` and base64 value. An empty `value` is how
/// Premiere saves a value stored earlier in the component under the same
/// hash; the hash is content-derived (its last eight hex digits are the
/// byte length plus 12), so equal hashes are equal values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LumetriDefault {
    Static(&'static str),
    Binary {
        hash: &'static str,
        value: &'static str,
    },
}

/// The `PremiereFilterPrivateData` (`BinaryHash`, base64 value) of a Lumetri
/// Color as Premiere 26.5.2 saves it: 140 bytes starting `tmul`, identical in
/// the differently graded native saves `color_lumetri_basic.prproj` and a
/// Premiere 26.5.2 Lumetri authority probe, so it carries no grade. That probe
/// proved that a component with this private data, the [`LUMETRI_BLOB_SENTINEL`] Blob and
/// edited scalars renders by its scalars; a component without the private
/// data was not tested, so the writer keeps it.
pub(crate) const LUMETRI_PRIVATE_DATA: (&str, &str) = (
    "198708d4-e5af-9ffd-2378-58c00000008c",
    "dG11bAMAAAD/////////////////////AAAAAP//////////AAAAAAAAAAD/////AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeOz//3js//947P//AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAA=",
);

/// Lumetri's `Blob` (ParameterID 1) as every pinned native save stores it: the
/// constant two bytes `FE FE` (base64 `/v4=`), also for a fully graded Basic,
/// Creative and Vignette. Premiere renders by the scalars, but refuses to open
/// a project whose Lumetri `Blob` has no value (authority probe, Premiere 26.5.2), so
/// every written Lumetri carries it ([`LUMETRI_DEFAULTS`]'s first entry).
pub(crate) const LUMETRI_BLOB_SENTINEL: LumetriDefault = LumetriDefault::Binary {
    hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
    value: "/v4=",
};

/// Basic Correction section enable (a checkbox named one space).
pub(crate) const LUMETRI_BASIC_ENABLE: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Basic Correction enable",
    ..param(3, " ", CHECKBOX, "", ["", ""], [None, None])
};
/// Creative section enable.
pub(crate) const LUMETRI_CREATIVE_ENABLE: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Creative enable",
    ..param(23, " ", CHECKBOX, "", ["", ""], [None, None])
};
/// Vignette section enable.
pub(crate) const LUMETRI_VIGNETTE_ENABLE: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Vignette enable",
    ..param(50, " ", CHECKBOX, "", ["", ""], [None, None])
};

/// Saved Temperature (UI -100..100, saved -300..300); FX `temperature`
/// is the saved value divided by 116, warmer positive in both. The scale is
/// fitted to Premiere 26.5.2 renders of single-control probes on an in-house
/// ramp ([`LUMETRI_GRADE_APPROXIMATION`]): Lumetri's white balance is a
/// per-channel code-value gain that also changes green and keeps Rec.709 luma,
/// which FX's red-versus-blue gain does not.
pub(crate) const LUMETRI_TEMPERATURE: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Temperature",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "temperature",
        multiplier: 1.0 / 116.0,
    }),
    ..param(
        7,
        "Temperature",
        FLOAT,
        "",
        ["-300", "300"],
        [Some("-100"), Some("100")],
    )
};
/// Saved Tint (UI -100..100, saved -300..300). Lumetri's positive Tint is
/// magenta and FX's positive `tint` green, so FX is minus the saved value
/// divided by 100 (fitted like Temperature).
pub(crate) const LUMETRI_TINT: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Tint",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "tint",
        multiplier: -1.0 / 100.0,
    }),
    ..param(
        8,
        "Tint",
        FLOAT,
        "",
        ["-300", "300"],
        [Some("-100"), Some("100")],
    )
};
/// Saved Exposure in linear-light stops. Measured in Premiere 26.5.2, a
/// negative Exposure is exactly a gamma-2.4 code-value gain (stops/2.4), and a
/// positive one adds a highlight shoulder that keeps pixels with a channel at
/// white unchanged; FX `exposure` multiplies code values and clips, so the
/// fitted compromise divides by 2.6.
pub(crate) const LUMETRI_EXPOSURE: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Exposure",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "exposure",
        multiplier: 1.0 / 2.6,
    }),
    ..param(
        11,
        "Exposure",
        FLOAT,
        "",
        ["-7", "7"],
        [Some("-5"), Some("5")],
    )
};
/// Saved Contrast; FX `contrast` stops are the saved value divided by 360,
/// fitted to Premiere 26.5.2 renders: Lumetri's contrast is an S-curve about
/// code value 0.5 that keeps black and white fixed, FX's a straight slope.
pub(crate) const LUMETRI_CONTRAST: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Contrast",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "contrast",
        multiplier: 1.0 / 360.0,
    }),
    ..param(
        12,
        "Contrast",
        FLOAT,
        "",
        ["-150", "150"],
        [Some("-100"), Some("100")],
    )
};
/// Saved Highlights; FX `highlights` is the saved value divided by 240
/// (fitted; Lumetri's highlight curve also moves midtones, FX's window does not).
pub(crate) const LUMETRI_HIGHLIGHTS: EffectParamSpec = tonal(13, "Highlights", "highlights", 240.0);
/// Saved Shadows; FX `shadows` is the saved value divided by 510 (fitted;
/// Lumetri's shadow curve keeps black fixed, FX's window offsets it).
pub(crate) const LUMETRI_SHADOWS: EffectParamSpec = tonal(14, "Shadows", "shadows", 510.0);
/// Saved Whites; FX `whites` is the saved value divided by 54 (fitted).
/// Lumetri's Whites is a white-point gain (±100 is ±1 linear stop), which FX's
/// top-quarter window cannot express at any scale.
pub(crate) const LUMETRI_WHITES: EffectParamSpec = tonal(15, "Whites", "whites", 54.0);
/// Saved Blacks; FX `blacks` is the saved value divided by 124 (fitted).
pub(crate) const LUMETRI_BLACKS: EffectParamSpec = tonal(16, "Blacks", "blacks", 124.0);
/// FX `saturation` per saved Basic Saturation percent above 100 (fitted;
/// Lumetri scales chroma in gamma-2.4 linear light, FX in code values).
const LUMETRI_SATURATION_SCALE: f64 = 1.0 / 110.0;
/// Basic Saturation in percent; FX `saturation` is the saved value minus
/// 100, divided by 110 ([`lumetri_fx_value`]): 100 neutral.
pub(crate) const LUMETRI_SATURATION: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Saturation",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "saturation",
        multiplier: LUMETRI_SATURATION_SCALE,
    }),
    ..param(
        20,
        "Saturation",
        FLOAT,
        "",
        ["0", "300"],
        [None, Some("200")],
    )
};
/// Creative Vibrance; FX `vibrance` is the saved value divided by 167
/// (fitted; a negative Lumetri Vibrance only partly desaturates, FX's fully).
pub(crate) const LUMETRI_VIBRANCE: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Vibrance",
    binding: Some(EffectParamBinding::ScaledScalar {
        name: "vibrance",
        multiplier: 1.0 / 167.0,
    }),
    ..param(30, "Vibrance", FLOAT, "", ["-100", "100"], [None, None])
};
/// Creative Saturation in percent. FX has one saturation: the reader folds
/// a static Creative Saturation multiplicatively into Basic Saturation.
pub(crate) const LUMETRI_CREATIVE_SATURATION: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Creative Saturation",
    ..param(
        31,
        "Saturation",
        FLOAT,
        "",
        ["0", "300"],
        [None, Some("200")],
    )
};
pub(crate) const LUMETRI_VIGNETTE_AMOUNT: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Vignette Amount",
    ..param(
        51,
        "Amount",
        FLOAT,
        "",
        ["-5", "5"],
        [Some("-3"), Some("3")],
    )
};
pub(crate) const LUMETRI_VIGNETTE_MIDPOINT: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Vignette Midpoint",
    ..param(52, "Midpoint", FLOAT, "", ["0", "100"], [None, None])
};
pub(crate) const LUMETRI_VIGNETTE_ROUNDNESS: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Vignette Roundness",
    ..param(53, "Roundness", FLOAT, "", ["-100", "100"], [None, None])
};
pub(crate) const LUMETRI_VIGNETTE_FEATHER: EffectParamSpec = EffectParamSpec {
    label: "Lumetri Vignette Feather",
    ..param(54, "Feather", FLOAT, "", ["0", "100"], [None, None])
};

/// The Vignette controls that map to FX `vignette`, in its amount, radius,
/// feather order: FX `amount` is minus the saved Amount divided by 5,
/// `radius` Midpoint/100 and `feather` Feather/100; Roundness is not mapped.
pub(crate) const LUMETRI_VIGNETTE_PARAMS: [&EffectParamSpec; 3] = [
    &LUMETRI_VIGNETTE_AMOUNT,
    &LUMETRI_VIGNETTE_MIDPOINT,
    &LUMETRI_VIGNETTE_FEATHER,
];

const fn tonal(id: usize, name: &'static str, fx: &'static str, divisor: f64) -> EffectParamSpec {
    EffectParamSpec {
        binding: Some(EffectParamBinding::ScaledScalar {
            name: fx,
            multiplier: 1.0 / divisor,
        }),
        ..param(
            id,
            name,
            FLOAT,
            "",
            ["-150", "150"],
            [Some("-100"), Some("100")],
        )
    }
}

/// Lumetri Color, written with every [`LUMETRI_PARAMS`] record. Both the
/// grade and the vignette replacement use it; each written Lumetri carries
/// one of them, its other sections at [`LUMETRI_DEFAULTS`].
pub(crate) const LUMETRI: EffectSpec = EffectSpec {
    match_name: LUMETRI_MATCH_NAME,
    display_name: "Lumetri Color",
    filter_type: "2",
    params: LUMETRI_PARAMS,
    premiere_native: false,
    opaque_private_data: false,
};

/// The Basic Correction and Creative controls that map to `primaryGrade`,
/// in its control order.
pub(crate) const LUMETRI_GRADE_PARAMS: [&EffectParamSpec; 10] = [
    &LUMETRI_TEMPERATURE,
    &LUMETRI_TINT,
    &LUMETRI_EXPOSURE,
    &LUMETRI_CONTRAST,
    &LUMETRI_HIGHLIGHTS,
    &LUMETRI_SHADOWS,
    &LUMETRI_WHITES,
    &LUMETRI_BLACKS,
    &LUMETRI_SATURATION,
    &LUMETRI_VIBRANCE,
];

/// Index of [`LUMETRI_SATURATION`] in [`LUMETRI_GRADE_PARAMS`], the control a
/// static Creative Saturation folds into.
pub(crate) const LUMETRI_GRADE_SATURATION: usize = 8;
const _: () = assert!(LUMETRI_GRADE_PARAMS[LUMETRI_GRADE_SATURATION].id == LUMETRI_SATURATION.id);

/// Saved values of [`LUMETRI_GRADE_PARAMS`] that are both savable and have
/// FX images within the primaryGrade v1 ranges. Import clamps saved values
/// beyond them (Temperature beyond ±116, Tint beyond ±100, Whites beyond ±54,
/// Blacks beyond ±124, Saturation above 210) with a diagnostic; export clamps
/// FX values whose saved image lies beyond them (Exposure beyond ±7/2.6,
/// Contrast, Highlights and Shadows beyond ±150 saved) with a diagnostic.
pub(crate) const LUMETRI_GRADE_RANGES: [[f64; 2]; 10] = [
    [-116.0, 116.0],
    [-100.0, 100.0],
    [-7.0, 7.0],
    [-150.0, 150.0],
    [-150.0, 150.0],
    [-150.0, 150.0],
    [-54.0, 54.0],
    [-124.0, 124.0],
    [0.0, 210.0],
    [-100.0, 100.0],
];

/// Neutral saved values of [`LUMETRI_GRADE_PARAMS`].
pub(crate) const LUMETRI_GRADE_NEUTRAL: [f64; 10] =
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 100.0, 0.0];

/// Why a Lumetri grade is an editable approximation in either direction.
pub(crate) const LUMETRI_GRADE_APPROXIMATION: &str = "Lumetri Basic Correction and Creative scalars map to one canonical primaryGrade by scales fitted to Premiere 26.5.2 renders (Temperature/116, -Tint/100, Exposure stops/2.6, Contrast/360, Highlights/240, Shadows/510, Whites/54, Blacks/124, (Saturation-100)/110, Vibrance/167); Lumetri grades in gamma-2.4 linear light with a highlight shoulder, a luminance-keeping white balance, a white-point Whites gain and tone curves that keep black and white fixed, which primaryGrade's code-value equations do not reproduce: measured mean/max 8-bit error per control on an in-house ramp at UI -50/+50 (Temperature/Tint -100/+100, Exposure -0.5/+0.5, Saturation 50/150) is Exposure 1.4/3 and 5.7/32, Contrast 4.0/12 and 2.3/12, Highlights 6.2/21 and 6.1/19, Shadows 6.7/20 and 6.8/22, Whites 16.5/34 and 14.9/47, Blacks 2.6/37 and 5.2/37, Temperature 1.7/5 and 6.9/19, Tint 22.7/59 and 11.6/34, Saturation 13.3/83 and 5.0/61, Vibrance 20.1/118 and 2.0/15, so the picture is not equivalent";

/// The FX value of a grade control's saved `value`: its binding's scale, and
/// for Saturation of the percent above 100, so that 100 percent is neutral 0.
pub(crate) fn lumetri_fx_value(param: &EffectParamSpec, value: f64) -> f64 {
    if *param == LUMETRI_SATURATION {
        // Rounded like the scale (`EffectParamSpec::fx_value`).
        ((value - 100.0) * LUMETRI_SATURATION_SCALE * 1e9).round() / 1e9
    } else {
        param.fx_value(value)
    }
}

/// The FX value that [`EffectParamSpec::native_value`] scales to a grade
/// control's saved value: [`lumetri_fx_value`]'s inverse before scaling.
pub(crate) fn lumetri_scaled_fx_value(param: &EffectParamSpec, fx: f64) -> f64 {
    if *param == LUMETRI_SATURATION {
        fx + 100.0 * LUMETRI_SATURATION_SCALE
    } else {
        fx
    }
}

/// A Lumetri Basic Correction and Creative grade: the saved values of
/// [`LUMETRI_GRADE_PARAMS`], each within [`LUMETRI_GRADE_RANGES`], with a
/// static Creative Saturation folded into Basic Saturation. Keyed controls
/// keep their keys in the effect's animations.
///
/// `vignette` is export-only: the static saved Vignette Amount, Midpoint and
/// Feather of an FX `vignette` that directly follows this grade on the same
/// layer, written into the same Lumetri Color instead of a second one. The
/// reader always returns the Vignette section as its own effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrLumetriGrade {
    pub(crate) values: [f64; 10],
    pub(crate) vignette: Option<[f64; 3]>,
}

impl PrLumetriGrade {
    /// The primaryGrade controls, in its order.
    pub(crate) fn fx_controls(self) -> [f64; 10] {
        std::array::from_fn(|index| {
            lumetri_fx_value(LUMETRI_GRADE_PARAMS[index], self.values[index])
        })
    }

    /// Why this grade cannot be written: a value outside its range.
    pub(crate) fn invalid_reason(self) -> Option<String> {
        let grade = (0..LUMETRI_GRADE_PARAMS.len()).find_map(|index| {
            let (value, [low, high]) = (self.values[index], LUMETRI_GRADE_RANGES[index]);
            (!value.is_finite() || !(low..=high).contains(&value)).then(|| {
                format!(
                    "{} {value} is outside {low} to {high}",
                    LUMETRI_GRADE_PARAMS[index].label
                )
            })
        });
        grade.or_else(|| {
            self.vignette?
                .into_iter()
                .zip(LUMETRI_VIGNETTE_PARAMS)
                .find(|(value, param)| {
                    !param
                        .value_range()
                        .is_some_and(|range| range.contains(value))
                })
                .map(|(value, param)| format!("{} {value} is outside its saved range", param.label))
        })
    }
}

/// The pinned default of the layout parameter `id`.
pub(crate) fn lumetri_default(id: usize) -> Option<LumetriDefault> {
    LUMETRI_PARAMS
        .iter()
        .zip(LUMETRI_DEFAULTS)
        .find(|(param, _)| param.id == id)
        .map(|(_, default)| default)
}

pub(crate) const LUMETRI_PARAMS: &[EffectParamSpec] = &[
    param(1, "Blob", BINARY, "", ["", ""], [None, None]),
    param(62, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(
        2,
        "Basic Correction",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    LUMETRI_BASIC_ENABLE,
    param(4, " ", BINARY, "", ["", ""], [None, None]),
    param(95, " ", BINARY, "", ["", ""], [None, None]),
    param(5, "Input LUT", POPUP, "", ["0", "11"], [None, None]),
    param(127, "Input LUT", POPUP, "10", ["0", "11"], [None, None]),
    param(63, "HDR White", FLOAT, "", ["100", "1000"], [None, None]),
    param(120, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(119, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(123, "Intensity", FLOAT, "", ["0", "100"], [None, None]),
    param(6, "Color", CHECKBOX, "11", ["", "false"], [None, None]),
    param(94, "White Balance", COLOUR, "", ["", ""], [None, None]),
    LUMETRI_TEMPERATURE,
    LUMETRI_TINT,
    LUMETRI_SATURATION,
    param(9, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(10, "Light", CHECKBOX, "11", ["", "false"], [None, None]),
    LUMETRI_EXPOSURE,
    LUMETRI_CONTRAST,
    LUMETRI_HIGHLIGHTS,
    LUMETRI_SHADOWS,
    LUMETRI_WHITES,
    LUMETRI_BLACKS,
    param(
        64,
        "HDR Specular",
        FLOAT,
        "",
        ["-150", "150"],
        [Some("-100"), Some("100")],
    ),
    param(17, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(18, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(19, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(21, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(22, "Creative", CHECKBOX, "11", ["", "false"], [None, None]),
    LUMETRI_CREATIVE_ENABLE,
    param(24, " ", BINARY, "", ["", ""], [None, None]),
    param(96, " ", BINARY, "", ["", ""], [None, None]),
    param(25, "Look", POPUP, "", ["0", "68"], [None, None]),
    param(128, "Look", POPUP, "10", ["0", "68"], [None, None]),
    param(129, " ", BINARY, "", ["", ""], [None, None]),
    param(130, "Color Space", POPUP, "", ["0", "34"], [None, None]),
    param(26, "Intensity", FLOAT, "", ["0", "200"], [None, None]),
    param(
        27,
        "Adjustments",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    param(
        28,
        "Faded Film",
        FLOAT,
        "",
        ["0", "150"],
        [None, Some("100")],
    ),
    param(29, "Sharpen", FLOAT, "", ["-100", "100"], [None, None]),
    LUMETRI_VIBRANCE,
    LUMETRI_CREATIVE_SATURATION,
    param(32, "", BINARY, "9", ["", ""], [None, None]),
    param(
        33,
        "Tint Balance",
        FLOAT,
        "",
        ["-150", "150"],
        [Some("-100"), Some("100")],
    ),
    param(34, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(35, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(36, "Curves", CHECKBOX, "11", ["", "false"], [None, None]),
    param(37, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(
        38,
        "RGB Curves",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    param(118, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(65, "HDR Range", FLOAT, "", ["100", "10000"], [None, None]),
    param(39, " ", BINARY, "9", ["", ""], [None, None]),
    param(40, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(
        41,
        "Hue Saturation Curves",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    param(104, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(
        113,
        "Hue (vs Sat) Selector",
        COLOUR,
        "",
        ["", ""],
        [None, None],
    ),
    param(42, "Hue vs Sat", BINARY, "9", ["", ""], [None, None]),
    param(105, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(
        114,
        "Hue (vs Hue) Selector",
        COLOUR,
        "",
        ["", ""],
        [None, None],
    ),
    param(106, "Hue vs Hue", BINARY, "9", ["", ""], [None, None]),
    param(107, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(
        115,
        "Hue (vs Luma) Selector",
        COLOUR,
        "",
        ["", ""],
        [None, None],
    ),
    param(108, "Hue vs Luma", BINARY, "9", ["", ""], [None, None]),
    param(109, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(
        116,
        "Luma (vs Sat) Selector",
        COLOUR,
        "",
        ["", ""],
        [None, None],
    ),
    param(110, "Luma vs Sat", BINARY, "9", ["", ""], [None, None]),
    param(111, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(
        117,
        "Sat (vs Sat) Selector",
        COLOUR,
        "",
        ["", ""],
        [None, None],
    ),
    param(112, "Sat vs Sat", BINARY, "9", ["", ""], [None, None]),
    param(43, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(44, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(
        45,
        "Color Wheels & Match",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    param(46, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(101, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(102, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(103, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(66, "HDR White", FLOAT, "", ["100", "1000"], [None, None]),
    param(47, " ", BINARY, "9", ["", ""], [None, None]),
    param(48, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(
        67,
        "HSL Secondary",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    param(68, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(69, "Key", CHECKBOX, "11", ["", "false"], [None, None]),
    param(70, "Set color", COLOUR, "", ["", ""], [None, None]),
    param(71, "Add color", COLOUR, "", ["", ""], [None, None]),
    param(72, "Remove color", COLOUR, "", ["", ""], [None, None]),
    param(73, " ", BINARY, "9", ["", ""], [None, None]),
    param(74, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(75, " ", POPUP, "", ["0", "2"], [None, None]),
    param(76, " ", CHECKBOX, "", ["", ""], [None, None]),
    param(77, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(78, " ", CHECKBOX, "16", ["", ""], [None, None]),
    param(79, " ", CHECKBOX, "12", ["", "false"], [None, None]),
    param(80, "Refine", CHECKBOX, "11", ["", "false"], [None, None]),
    param(81, "Denoise", FLOAT, "", ["0", "100"], [None, None]),
    param(82, "Blur", FLOAT, "", ["0", "1000"], [None, Some("30")]),
    param(83, "Blur", CHECKBOX, "12", ["", "false"], [None, None]),
    param(
        84,
        "Correction",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    param(85, " ", BINARY, "9", ["", ""], [None, None]),
    param(93, " ", BINARY, "", ["", ""], [None, None]),
    param(
        86,
        "Temperature",
        FLOAT,
        "",
        ["-300", "300"],
        [Some("-100"), Some("100")],
    ),
    param(
        87,
        "Tint",
        FLOAT,
        "",
        ["-300", "300"],
        [Some("-100"), Some("100")],
    ),
    param(
        88,
        "Contrast",
        FLOAT,
        "",
        ["-150", "150"],
        [Some("-100"), Some("100")],
    ),
    param(89, "Sharpen", FLOAT, "", ["-100", "100"], [None, None]),
    param(
        90,
        "Saturation",
        FLOAT,
        "",
        ["0", "300"],
        [None, Some("200")],
    ),
    param(
        91,
        "Saturation",
        CHECKBOX,
        "12",
        ["", "false"],
        [None, None],
    ),
    param(92, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(49, "Vignette", CHECKBOX, "11", ["", "false"], [None, None]),
    LUMETRI_VIGNETTE_ENABLE,
    LUMETRI_VIGNETTE_AMOUNT,
    LUMETRI_VIGNETTE_MIDPOINT,
    LUMETRI_VIGNETTE_ROUNDNESS,
    LUMETRI_VIGNETTE_FEATHER,
    param(55, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(
        56,
        "SpeedGrade Custom",
        CHECKBOX,
        "11",
        ["", "false"],
        [None, None],
    ),
    param(57, "Custom Layer", CHECKBOX, "", ["", ""], [None, None]),
    param(58, "unused", CHECKBOX, "16", ["", ""], [None, None]),
    param(59, "unused", CHECKBOX, "16", ["", ""], [None, None]),
    param(60, " ", BINARY, "", ["", ""], [None, None]),
    param(61, "", CHECKBOX, "12", ["", "false"], [None, None]),
    param(97, "", CHECKBOX, "", ["", ""], [None, None]),
    param(98, "Embedded LUTs", BINARY, "", ["", ""], [None, None]),
    param(
        99,
        "Media Embedded LUT type",
        BINARY,
        "",
        ["", ""],
        [None, None],
    ),
    param(
        100,
        "Media Embedded Lut blob",
        BINARY,
        "",
        ["", ""],
        [None, None],
    ),
    param(
        121,
        "SemanticAutoTone Progress",
        FLOAT,
        "10",
        ["-1", "1"],
        [Some("0"), None],
    ),
    param(
        122,
        "Auto Tone Analytics Data",
        BINARY,
        "",
        ["", ""],
        [None, None],
    ),
    param(124, " ", CHECKBOX, "10", ["", ""], [None, None]),
    param(125, "LUTAsset", BINARY, "", ["", ""], [None, None]),
    param(126, "LookAsset", BINARY, "", ["", ""], [None, None]),
];

// Each layout parameter has exactly one pinned default, paired by position.
const _: () = assert!(LUMETRI_PARAMS.len() == LUMETRI_DEFAULTS.len());

pub(crate) const LUMETRI_DEFAULTS: [LumetriDefault; 130] = [
    LUMETRI_BLOB_SENTINEL,
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Static("0,0,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,100.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,50.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374897589125431296,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,100.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Static("0,0,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "6fd80a96-3763-05c0-8975-91180000001c",
        value: "NJHEzR4hKvCa8KThGXaBcw==",
    },
    LumetriDefault::Static("-91445760000000000,28,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,100.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,100.,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "49d2fca5-ddd1-a393-0544-f6d80000002c",
        value: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    },
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,100.,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "261f3840-e8cf-a8cd-fa5e-bac10000082c",
        value: "AAAAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8D8AAAAAAADwPwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAPA/AAAAAAAA8D8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADwPwAAAAAAAPA/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8D8AAAAAAADwPwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
    },
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623936,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "f146cafe-eab3-19ed-0ee8-de0e00000214",
        value: "AwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8D8AAAAAAADwPwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
    },
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623936,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "f146cafe-eab3-19ed-0ee8-de0e00000214",
        value: "",
    },
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623936,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "f146cafe-eab3-19ed-0ee8-de0e00000214",
        value: "",
    },
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623936,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "dc782da7-2ab3-2bea-9cbe-726600000214",
        value: "AgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA8D8AAAAAAADwPwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
    },
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623936,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "dc782da7-2ab3-2bea-9cbe-726600000214",
        value: "",
    },
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,100.,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "0b5436c4-bae0-bb64-a2f6-c8c40000006c",
        value: "AAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAA",
    },
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623680,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623680,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,18374686479671623680,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "39a419ab-5a1c-7d5c-3db0-76fc00000034",
        value: "AAAAAD8AAAAAAAAAAAEAAAA/AAAAAAAAAAABAAAAPwAAAAAAAAAAAQ==",
    },
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "2e2bc4a0-c0c0-3af7-27b5-a326000000cc",
        value: "AAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAAAAAAAAAAAAAAAAAAAADgPwAAAAAAAAAA",
    },
    LumetriDefault::Binary {
        hash: "3c355f83-deb1-a1e5-225c-571c00000010",
        value: "AAAAAA==",
    },
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,100.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,50.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,0.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,50.,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,true,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Binary {
        hash: "3c355f83-deb1-a1e5-225c-571c00000010",
        value: "",
    },
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Static("-91445760000000000,-1.,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "4abe37c1-2b16-787e-3562-1564000001a2",
        value: "ewAiAG0AQQB1AHQAbwBUAG8AbgBlAFAAcgBlAHMAcwBlAGQAIgA6AGYAYQBsAHMAZQAsACIAbQBCAGwAYQBjAGsAcwAiADoAMAAsACIAbQBDAG8AbgB0AHIAYQBzAHQAIgA6ADAALAAiAG0ARQB4AHAAbwBzAHUAcgBlACIAOgAwACwAIgBtAEgAaQBnAGgAbABpAGcAaAB0AHMAIgA6ADAALAAiAG0ATABhAHMAdABQAHIAZQBzAHMAZQBkAEEAdQB0AG8AVABvAG4AZQBCAHUAdAB0AG8AbgAiADoAMAAsACIAbQBSAGUAcwBlAHQAUAByAGUAcwBzAGUAZAAiADoAZgBhAGwAcwBlACwAIgBtAFMAYQB0AHUAcgBhAHQAaQBvAG4AIgA6ADAALAAiAG0AUwBoAGEAZABvAHcAcwAiADoAMAAsACIAbQBUAGUAbQBwAGUAcgBhAHQAdQByAGUAIgA6ADAALAAiAG0AVABpAG4AdAAiADoAMAAsACIAbQBXAGgAaQB0AGUAcwAiADoAMAB9AA==",
    },
    LumetriDefault::Static("-91445760000000000,false,0,0,0,0,0,0"),
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
    LumetriDefault::Binary {
        hash: "63866a15-9327-e4cb-ff31-c9ea0000000e",
        value: "",
    },
];
