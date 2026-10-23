//! Native AE colour-correction effects lowered to canonical FX colour grades.
//!
//! Each mapping is an editable approximation: Adobe's colour math is not the
//! FX v1 equations, so only control identities and declared units survive.
//!
//! Control identities, labels and native ranges come from the independently
//! authored AE 26.5 colour catalog (`tests/fixtures/effects/color_catalog.aep`
//! and its fresh-host readback receipt, mirrored in `definitions.json`). Each
//! labelled control also requires the instance's own `pard` label to match, so
//! a different plugin build omits that control with a diagnostic, never a guess.

mod color_balance;
pub(crate) mod lumetri_luts;

use fx_schema::{ColorCurvesSemanticVersion, LayerEffect, color_tools::SelectiveColorMethod};
use serde_json::{Value, json};

use super::{
    mapping::{Field, Mapping, Scale},
    native::DecodedEffect,
};
use crate::{
    properties::NumericProperty,
    writer::effects::{NativeArbitraryData, NativeEffect},
};

pub(crate) const LUMETRI: &str = "ADBE Lumetri";
pub(crate) const COLOR_BALANCE: &str = "ADBE Color Balance 2";
pub(crate) const CHANNEL_MIXER: &str = "ADBE CHANNEL MIXER";
pub(crate) const SELECTIVE_COLOR: &str = "ADBE SelectiveColor";

/// One native scalar control with an affine map `fx = native * scale + offset`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Control {
    pub native: &'static str,
    /// Expected `pard` label; `None` for checkboxes whose label lives in `pdnm`.
    pub label: Option<&'static str>,
    pub param: &'static str,
    pub scale: f64,
    pub offset: f64,
    pub min: f64,
    pub max: f64,
}

const fn control(
    native: &'static str,
    label: &'static str,
    param: &'static str,
    scale: f64,
    offset: f64,
    [min, max]: [f64; 2],
) -> Control {
    Control {
        native,
        label: Some(label),
        param,
        scale,
        offset,
        min,
        max,
    }
}

impl Control {
    /// The export mapping rows of `controls`, each with the same affine map as
    /// its import (`native = (fx - offset) / scale`).
    const fn fields<const N: usize>(controls: &[Control; N], animated: bool) -> [Field; N] {
        let mut fields = [controls[0].field(animated); N];
        let mut index = 1;
        while index < N {
            fields[index] = controls[index].field(animated);
            index += 1;
        }
        fields
    }

    /// The same affine row as an export mapping field (`native = (fx - offset) / scale`).
    const fn field(self, animated: bool) -> Field {
        Field {
            native: self.native,
            field: self.param,
            param: self.param,
            component: 0,
            scale: Scale::Factor(self.scale),
            offset: self.offset,
            boolean: false,
            animated,
        }
    }
}

/// Authored native keys bound to one animatable FX field through the same
/// affine map as the static value.
#[derive(Clone, Debug)]
pub(crate) struct KeyedControl {
    pub control: Control,
    pub numeric: NumericProperty,
}

/// A lowered grade, its key bindings and contextual diagnostics.
#[derive(Debug)]
pub(crate) struct LoweredGrade {
    pub effect: LayerEffect,
    pub keyed: Vec<KeyedControl>,
    pub warnings: Vec<String>,
    /// Further static FX effects the same native instance applies before
    /// `effect`, in order (a Lumetri Input LUT).
    pub before: Vec<LayerEffect>,
    /// Further static FX effects the same native instance applies after
    /// `effect`, in order (Lumetri Look, RGB Curves, Hue Saturation Curves,
    /// Color Wheels, Vignette).
    pub after: Vec<LayerEffect>,
}

// Native Temperature/Tint span ±300 (three times the ±100 UI), Exposure ±7,
// tone windows ±150, Saturation 0..300. Lumetri's positive Tint is magenta,
// FX positive tint is green. The scales are the Premiere importer's, fitted to
// Premiere 26.5.2 renders of single-control probes (After Effects 26.5 renders
// the same Lumetri scalars within 0.32/255 of Premiere): see
// `LUMETRI_APPROXIMATION` for the measured residuals.
const LUMETRI_BASIC: [Control; 9] = [
    control(
        "ADBE Lumetri-0007",
        "Temperature",
        "temperature",
        1.0 / 116.0,
        0.0,
        [-1.0, 1.0],
    ),
    control(
        "ADBE Lumetri-0008",
        "Tint",
        "tint",
        -1.0 / 100.0,
        0.0,
        [-1.0, 1.0],
    ),
    // Linear-light stops: a negative Exposure is a gamma-2.4 code gain, a
    // positive one adds a highlight shoulder; /2.6 is the fitted compromise.
    control(
        LUMETRI_EXPOSURE,
        "Exposure",
        "exposure",
        1.0 / 2.6,
        0.0,
        [-5.0, 5.0],
    ),
    control(
        "ADBE Lumetri-0012",
        "Contrast",
        "contrast",
        1.0 / 360.0,
        0.0,
        [-2.0, 2.0],
    ),
    control(
        "ADBE Lumetri-0013",
        "Highlights",
        "highlights",
        1.0 / 240.0,
        0.0,
        [-1.0, 1.0],
    ),
    control(
        "ADBE Lumetri-0014",
        "Shadows",
        "shadows",
        1.0 / 510.0,
        0.0,
        [-1.0, 1.0],
    ),
    control(
        "ADBE Lumetri-0015",
        "Whites",
        "whites",
        1.0 / 54.0,
        0.0,
        [-1.0, 1.0],
    ),
    control(
        "ADBE Lumetri-0016",
        "Blacks",
        "blacks",
        1.0 / 124.0,
        0.0,
        [-1.0, 1.0],
    ),
    control(
        "ADBE Lumetri-0020",
        "Saturation",
        "saturation",
        LUMETRI_SATURATION_SCALE,
        -100.0 * LUMETRI_SATURATION_SCALE,
        [-1.0, 1.0],
    ),
];
/// FX `saturation` per Lumetri Saturation percent above 100 (Basic and Creative).
const LUMETRI_SATURATION_SCALE: f64 = 1.0 / 110.0;

/// The project colour domain a Lumetri grade is converted in. Only the
/// Exposure scale differs: the fitted legacy scale (stops/2.6) comes from
/// display-referred renders and does not describe an OCIO ACES project, which
/// keeps the earlier stops/2.2 scale (see [`LUMETRI_ACES_EXPOSURE_APPROXIMATION`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColorGradeDomain {
    Legacy,
    /// An OCIO colour-managed ACES project (FX ACES document).
    Aces,
}

/// Why a Lumetri Exposure in an OCIO ACES project keeps the stops/2.2 scale.
pub(crate) const LUMETRI_ACES_EXPOSURE_APPROXIMATION: &str = "OCIO ACES project: Lumetri Exposure maps as stops/2.2 (export x2.2), not the display-referred fitted stops/2.6, because AE grades ACEScg scene light (one in-house chart sample: Exposure +1 = x1.36 scene light); this ACES scale is an unaccepted approximation: the FX render of the ACES fixture differs from AE's own ACES 2.0 output by 2.7 8-bit codes (above the 2-code gate)";
/// Why an imported Lumetri grade is an approximation: the fitted scales and
/// the measured residual of each control (Premiere 26.5.2 single-control
/// probes on an in-house ramp; the same text as the Premiere importer's).
pub(crate) const LUMETRI_APPROXIMATION: &str = "Lumetri Basic/Creative scalars approximated by canonical primaryGrade v1 with scales fitted to Lumetri renders (Temperature/116, -Tint/100, Exposure stops/2.6, Contrast/360, Highlights/240, Shadows/510, Whites/54, Blacks/124, (Saturation-100)/110, Vibrance/167); Lumetri grades in gamma-2.4 linear light with a highlight shoulder, a luminance-keeping white balance, a white-point Whites gain and tone curves that keep black and white fixed, which primaryGrade's code-value equations do not reproduce: measured mean/max 8-bit error per control on an in-house ramp at UI -50/+50 (Temperature/Tint -100/+100, Exposure -0.5/+0.5, Saturation 50/150) is Exposure 1.4/3 and 5.7/32, Contrast 4.0/12 and 2.3/12, Highlights 6.2/21 and 6.1/19, Shadows 6.7/20 and 6.8/22, Whites 16.5/34 and 14.9/47, Blacks 2.6/37 and 5.2/37, Temperature 1.7/5 and 6.9/19, Tint 22.7/59 and 11.6/34, Saturation 13.3/83 and 5.0/61, Vibrance 20.1/118 and 2.0/15; After Effects 26.5 renders the same Lumetri scalars within 0.32/255 of Premiere";
const LUMETRI_EXPOSURE: &str = "ADBE Lumetri-0011";
const LUMETRI_EXPOSURE_ACES: Control = control(
    LUMETRI_EXPOSURE,
    "Exposure",
    "exposure",
    1.0 / 2.2,
    0.0,
    [-5.0, 5.0],
);
/// [`LUMETRI_BASIC`] with the ACES Exposure row.
const LUMETRI_BASIC_ACES: [Control; 9] = {
    let mut controls = LUMETRI_BASIC;
    assert!(controls[2].scale == 1.0 / 2.6 && controls[2].min == LUMETRI_EXPOSURE_ACES.min);
    controls[2] = LUMETRI_EXPOSURE_ACES;
    controls
};
const LUMETRI_VIBRANCE: Control = control(
    "ADBE Lumetri-0030",
    "Vibrance",
    "vibrance",
    1.0 / 167.0,
    0.0,
    [-1.0, 1.0],
);
const LUMETRI_CREATIVE_SATURATION: &str = "ADBE Lumetri-0031";
const LUMETRI_BASIC_ENABLE: &str = "ADBE Lumetri-0003";
const LUMETRI_CREATIVE_ENABLE: &str = "ADBE Lumetri-0023";
/// Scalar controls without an FX counterpart and their neutral native values.
const LUMETRI_UNMAPPED: [(&str, &str, f64); 8] = [
    ("ADBE Lumetri-0026", "Creative Look Intensity", 100.0),
    ("ADBE Lumetri-0028", "Faded Film", 0.0),
    ("ADBE Lumetri-0029", "Creative Sharpen", 0.0),
    ("ADBE Lumetri-0033", "Tint Balance", 0.0),
    ("ADBE Lumetri-0081", "HSL Secondary Denoise", 0.0),
    ("ADBE Lumetri-0086", "HSL Secondary Temperature", 0.0),
    ("ADBE Lumetri-0088", "HSL Secondary Contrast", 0.0),
    ("ADBE Lumetri-0064", "HDR Specular", 0.0),
];

const fn balance(native: &'static str, label: &'static str, param: &'static str) -> Control {
    control(native, label, param, 0.01, 0.0, [-1.0, 1.0])
}
const COLOR_BALANCE_CONTROLS: [Control; 9] = [
    balance(
        "ADBE Color Balance 2-0001",
        "Shadow Red Balance",
        "shadowsRed",
    ),
    balance(
        "ADBE Color Balance 2-0002",
        "Shadow Green Balance",
        "shadowsGreen",
    ),
    balance(
        "ADBE Color Balance 2-0003",
        "Shadow Blue Balance",
        "shadowsBlue",
    ),
    balance(
        "ADBE Color Balance 2-0004",
        "Midtone Red Balance",
        "midtonesRed",
    ),
    balance(
        "ADBE Color Balance 2-0005",
        "Midtone Green Balance",
        "midtonesGreen",
    ),
    balance(
        "ADBE Color Balance 2-0006",
        "Midtone Blue Balance",
        "midtonesBlue",
    ),
    balance(
        "ADBE Color Balance 2-0007",
        "Highlight Red Balance",
        "highlightsRed",
    ),
    balance(
        "ADBE Color Balance 2-0008",
        "Highlight Green Balance",
        "highlightsGreen",
    ),
    balance(
        "ADBE Color Balance 2-0009",
        "Highlight Blue Balance",
        "highlightsBlue",
    ),
];
const COLOR_BALANCE_PRESERVE_LUMINOSITY: &str = "ADBE Color Balance 2-0010";

const fn weight(native: &'static str, label: &'static str, param: &'static str) -> Control {
    control(native, label, param, 0.01, 0.0, [-2.0, 2.0])
}
const fn constant(native: &'static str, label: &'static str, param: &'static str) -> Control {
    control(native, label, param, 0.01, 0.0, [-1.0, 1.0])
}
/// `param` is `row.column` in the FX payload.
const CHANNEL_MIXER_CONTROLS: [Control; 12] = [
    weight("ADBE CHANNEL MIXER-0001", "Red-Red", "red.red"),
    weight("ADBE CHANNEL MIXER-0002", "Red-Green", "red.green"),
    weight("ADBE CHANNEL MIXER-0003", "Red-Blue", "red.blue"),
    constant("ADBE CHANNEL MIXER-0004", "Red-Const", "red.constant"),
    weight("ADBE CHANNEL MIXER-0005", "Green-Red", "green.red"),
    weight("ADBE CHANNEL MIXER-0006", "Green-Green", "green.green"),
    weight("ADBE CHANNEL MIXER-0007", "Green-Blue", "green.blue"),
    constant("ADBE CHANNEL MIXER-0008", "Green-Const", "green.constant"),
    weight("ADBE CHANNEL MIXER-0009", "Blue-Red", "blue.red"),
    weight("ADBE CHANNEL MIXER-0010", "Blue-Green", "blue.green"),
    weight("ADBE CHANNEL MIXER-0011", "Blue-Blue", "blue.blue"),
    constant("ADBE CHANNEL MIXER-0012", "Blue-Const", "blue.constant"),
];
const CHANNEL_MIXER_MONOCHROME: &str = "ADBE CHANNEL MIXER-0013";

/// Nine Details groups in native order (`-0008 + 6n` group starts), each
/// holding Cyan/Magenta/Yellow/Black. `param` is `range.ink` in the FX payload.
pub(crate) const SELECTIVE_COLOR_CONTROLS: [Control; 36] = [
    ink("ADBE SelectiveColor-0009", "Cyan", "reds.cyan"),
    ink("ADBE SelectiveColor-0010", "Magenta", "reds.magenta"),
    ink("ADBE SelectiveColor-0011", "Yellow", "reds.yellow"),
    ink("ADBE SelectiveColor-0012", "Black", "reds.black"),
    ink("ADBE SelectiveColor-0015", "Cyan", "yellows.cyan"),
    ink("ADBE SelectiveColor-0016", "Magenta", "yellows.magenta"),
    ink("ADBE SelectiveColor-0017", "Yellow", "yellows.yellow"),
    ink("ADBE SelectiveColor-0018", "Black", "yellows.black"),
    ink("ADBE SelectiveColor-0021", "Cyan", "greens.cyan"),
    ink("ADBE SelectiveColor-0022", "Magenta", "greens.magenta"),
    ink("ADBE SelectiveColor-0023", "Yellow", "greens.yellow"),
    ink("ADBE SelectiveColor-0024", "Black", "greens.black"),
    ink("ADBE SelectiveColor-0027", "Cyan", "cyans.cyan"),
    ink("ADBE SelectiveColor-0028", "Magenta", "cyans.magenta"),
    ink("ADBE SelectiveColor-0029", "Yellow", "cyans.yellow"),
    ink("ADBE SelectiveColor-0030", "Black", "cyans.black"),
    ink("ADBE SelectiveColor-0033", "Cyan", "blues.cyan"),
    ink("ADBE SelectiveColor-0034", "Magenta", "blues.magenta"),
    ink("ADBE SelectiveColor-0035", "Yellow", "blues.yellow"),
    ink("ADBE SelectiveColor-0036", "Black", "blues.black"),
    ink("ADBE SelectiveColor-0039", "Cyan", "magentas.cyan"),
    ink("ADBE SelectiveColor-0040", "Magenta", "magentas.magenta"),
    ink("ADBE SelectiveColor-0041", "Yellow", "magentas.yellow"),
    ink("ADBE SelectiveColor-0042", "Black", "magentas.black"),
    ink("ADBE SelectiveColor-0045", "Cyan", "whites.cyan"),
    ink("ADBE SelectiveColor-0046", "Magenta", "whites.magenta"),
    ink("ADBE SelectiveColor-0047", "Yellow", "whites.yellow"),
    ink("ADBE SelectiveColor-0048", "Black", "whites.black"),
    ink("ADBE SelectiveColor-0051", "Cyan", "neutrals.cyan"),
    ink("ADBE SelectiveColor-0052", "Magenta", "neutrals.magenta"),
    ink("ADBE SelectiveColor-0053", "Yellow", "neutrals.yellow"),
    ink("ADBE SelectiveColor-0054", "Black", "neutrals.black"),
    ink("ADBE SelectiveColor-0057", "Cyan", "blacks.cyan"),
    ink("ADBE SelectiveColor-0058", "Magenta", "blacks.magenta"),
    ink("ADBE SelectiveColor-0059", "Yellow", "blacks.yellow"),
    ink("ADBE SelectiveColor-0060", "Black", "blacks.black"),
];
/// The visible sliders mirroring the range selected by the Colors popup.
pub(crate) const SELECTIVE_COLOR_CURRENT: [&str; 4] = [
    "ADBE SelectiveColor-0003",
    "ADBE SelectiveColor-0004",
    "ADBE SelectiveColor-0005",
    "ADBE SelectiveColor-0006",
];
pub(crate) const SELECTIVE_COLOR_METHOD: &str = "ADBE SelectiveColor-0001";
pub(crate) const SELECTIVE_COLOR_SELECTED_RANGE: &str = "ADBE SelectiveColor-0002";

const fn ink(native: &'static str, label: &'static str, param: &'static str) -> Control {
    control(native, label, param, 0.01, 0.0, [-1.0, 1.0])
}

/// Whether the native effect is lowered here (successfully or with a diagnosed
/// omission), rather than by the generic scalar mapping table.
pub(crate) fn handles(match_name: &str) -> bool {
    matches!(
        match_name,
        LUMETRI | COLOR_BALANCE | CHANNEL_MIXER | SELECTIVE_COLOR
    )
}

/// Lower one native colour effect. `Err` omits the effect with its reason.
pub(crate) fn lower(
    source: &DecodedEffect,
    domain: ColorGradeDomain,
) -> Result<LoweredGrade, String> {
    match source.match_name.as_str() {
        LUMETRI => lumetri(source, domain),
        COLOR_BALANCE => color_balance(source, domain),
        CHANNEL_MIXER => channel_mixer(source),
        SELECTIVE_COLOR => selective_color(source),
        other => Err(format!("{other} is not a native colour-grade effect")),
    }
}

fn numeric<'a>(
    source: &'a DecodedEffect,
    native: &str,
    label: Option<&str>,
) -> Result<&'a NumericProperty, String> {
    let mut matches = source.parameters.iter().filter(|p| p.match_name == native);
    let parameter = matches
        .next()
        .ok_or_else(|| format!("{native}: control missing"))?;
    if matches.next().is_some() {
        return Err(format!("{native}: duplicate control"));
    }
    if let (Some(expected), Some(declared)) = (label, parameter.declared_label.as_deref())
        && declared != expected
    {
        return Err(format!(
            "{native}: declared label {declared:?} differs from expected {expected:?}; control identity unconfirmed"
        ));
    }
    parameter
        .numeric
        .as_ref()
        .map_err(|error| format!("{native}: {error}"))
}

fn initial(numeric: &NumericProperty) -> Option<f64> {
    match numeric
        .keyframes
        .first()
        .map(|key| key.values.as_slice())
        .unwrap_or(&numeric.values)
    {
        [value] if value.is_finite() => Some(*value),
        _ => None,
    }
}

/// A static checkbox state; animation and expressions keep the initial state.
fn checkbox(
    source: &DecodedEffect,
    native: &str,
    warnings: &mut Vec<String>,
) -> Result<bool, String> {
    let numeric = numeric(source, native, None)?;
    let value = initial(numeric).ok_or_else(|| format!("{native}: scalar checkbox required"))?;
    if numeric.animated || numeric.expression_enabled {
        warnings.push(format!(
            "{native}: checkbox animation/expression omitted; initial state retained"
        ));
    }
    Ok(value != 0.0)
}

/// Lower each control into `payload`; omitted controls keep their neutral FX
/// value. Keys are returned only when `animatable` and every key is in range.
fn scalar_controls(
    source: &DecodedEffect,
    controls: &[Control],
    payload: &mut Value,
    animatable: bool,
    warnings: &mut Vec<String>,
) -> Vec<KeyedControl> {
    let mut keyed = Vec::new();
    for control in controls {
        let numeric = match numeric(source, control.native, control.label) {
            Ok(numeric) => numeric,
            Err(reason) => {
                warnings.push(format!("{reason}; FX neutral value retained"));
                continue;
            }
        };
        let Some(value) = initial(numeric) else {
            warnings.push(format!(
                "{}: finite scalar required; FX neutral value retained",
                control.native
            ));
            continue;
        };
        let mapped = value * control.scale + control.offset;
        let clamped = mapped.clamp(control.min, control.max);
        if clamped != mapped {
            warnings.push(format!(
                "{} {value}: maps to {} {mapped}, outside FX {}..={}; clamped",
                control.native, control.param, control.min, control.max
            ));
        }
        set(payload, control.param, clamped);
        if numeric.expression_enabled {
            warnings.push(format!(
                "{}: AE expression not executed; initial value retained",
                control.native
            ));
        }
        if !numeric.animated || numeric.keyframes.is_empty() {
            continue;
        }
        if !animatable {
            warnings.push(format!(
                "{}: FX control is static; animation omitted, initial value retained",
                control.native
            ));
            continue;
        }
        let in_range = numeric.keyframes.iter().all(|key| {
            key.values.len() == 1
                && (control.min..=control.max)
                    .contains(&(key.values[0] * control.scale + control.offset))
        });
        if !in_range {
            warnings.push(format!("{}: keyed values leave the FX range; animation omitted, clamped initial value retained", control.native));
            continue;
        }
        keyed.push(KeyedControl {
            control: *control,
            numeric: numeric.clone(),
        });
    }
    keyed
}

fn set(payload: &mut Value, path: &str, value: f64) {
    let mut slot = payload;
    for part in path.split('.') {
        slot = &mut slot[part];
    }
    *slot = json!(value);
}

fn finish(
    payload: Value,
    keyed: Vec<KeyedControl>,
    warnings: Vec<String>,
) -> Result<LoweredGrade, String> {
    let effect = serde_json::from_value::<LayerEffect>(payload)
        .map_err(|error| format!("mapped values are outside the FX representation ({error})"))?;
    Ok(LoweredGrade {
        effect,
        keyed,
        warnings,
        before: Vec::new(),
        after: Vec::new(),
    })
}

fn lumetri(source: &DecodedEffect, domain: ColorGradeDomain) -> Result<LoweredGrade, String> {
    let mut warnings = Vec::new();
    let mut payload = json!({"type":"primaryGrade","semanticVersion":1,"temperature":0,"tint":0,"exposure":0,"contrast":0,"highlights":0,"shadows":0,"whites":0,"blacks":0,"saturation":0,"vibrance":0});
    let section = |native, name, warnings: &mut Vec<String>| {
        let enabled = lumetri_section(source, native, name, warnings);
        if !enabled {
            warnings.push(format!("{name} section disabled; its controls are omitted"));
        }
        enabled
    };
    let mut keyed = Vec::new();
    if section(LUMETRI_BASIC_ENABLE, "Basic Correction", &mut warnings) {
        let basic = match domain {
            ColorGradeDomain::Legacy => &LUMETRI_BASIC,
            ColorGradeDomain::Aces => &LUMETRI_BASIC_ACES,
        };
        keyed = scalar_controls(source, basic, &mut payload, true, &mut warnings);
    }
    if section(LUMETRI_CREATIVE_ENABLE, "Creative", &mut warnings) {
        keyed.extend(scalar_controls(
            source,
            &[LUMETRI_VIBRANCE],
            &mut payload,
            true,
            &mut warnings,
        ));
        creative_saturation(source, &mut payload, &mut keyed, &mut warnings);
    }
    for (native, name, neutral) in LUMETRI_UNMAPPED {
        if let Ok(numeric) = numeric(source, native, None)
            && (numeric.animated || initial(numeric).is_some_and(|value| value != neutral))
        {
            warnings.push(format!(
                "{name} ({native}): no FX primaryGrade counterpart; omitted"
            ));
        }
    }
    let [input_lut, look] = lumetri_luts::browsed(source);
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (lut, sections) in [(input_lut, &mut before), (look, &mut after)] {
        match lut.map(|lut| lut.and_then(|lut| Ok((lut.effect()?, lut.notes)))) {
            Some(Ok((effect, notes))) => {
                sections.push(effect);
                warnings.extend(notes);
                warnings.push(lumetri_luts::LUMETRI_LUT_APPROXIMATION.into());
            }
            Some(Err(reason)) => warnings.push(reason),
            None => {}
        }
    }
    if !after.is_empty()
        && (payload["vibrance"].as_f64() != Some(0.0)
            || payload["saturation"].as_f64() != Some(0.0))
    {
        warnings.push("Lumetri Creative Vibrance and Saturation apply after the Look in Lumetri, but the primaryGrade that carries them applies before the lookTransform".into());
    }
    after.extend(lumetri_curves_and_wheels(source, &mut warnings));
    after.extend(lumetri_vignette(source, &mut warnings));
    diagnose_lumetri_unmapped_data(source, &mut warnings);
    warnings.push(LUMETRI_APPROXIMATION.into());
    if domain == ColorGradeDomain::Aces {
        warnings.push(LUMETRI_ACES_EXPOSURE_APPROXIMATION.into());
        if !after.is_empty() {
            warnings.push("OCIO ACES project: Lumetri RGB Curves, Color Wheels and Vignette are measured on display-referred renders only; their FX effects are unmeasured in an ACES document".into());
        }
    }
    let mut lowered = finish(payload, keyed, warnings)?;
    // As the Premiere importer: a neutral Basic/Creative grade beside other
    // sections adds no FX effect.
    if lowered.keyed.is_empty()
        && !(before.is_empty() && after.is_empty())
        && matches!(&lowered.effect, LayerEffect::PrimaryGrade(grade) if grade.is_neutral())
    {
        let mut sections = before.into_iter().chain(after);
        lowered.effect = sections.next().expect("a section beside the grade");
        lowered.after = sections.collect();
        lowered.warnings.retain(|warning| {
            warning != LUMETRI_APPROXIMATION && warning != LUMETRI_ACES_EXPOSURE_APPROXIMATION
        });
    } else {
        lowered.before = before;
        lowered.after = after;
    }
    Ok(lowered)
}

/// AE's `ADBE Lumetri-00NN` is the Lumetri `ParameterID` N that Premiere saves.
fn lumetri_control(id: usize) -> String {
    format!("ADBE Lumetri-{id:04}")
}

const LUMETRI_CURVES_ENABLE: &str = "ADBE Lumetri-0037";
const LUMETRI_WHEELS_ENABLE: &str = "ADBE Lumetri-0046";
const LUMETRI_VIGNETTE_ENABLE: &str = "ADBE Lumetri-0050";
/// Vignette Amount, Midpoint, Roundness and Feather.
const LUMETRI_VIGNETTE: [(&str, &str); 4] = [
    ("ADBE Lumetri-0051", "Amount"),
    ("ADBE Lumetri-0052", "Midpoint"),
    ("ADBE Lumetri-0053", "Roundness"),
    ("ADBE Lumetri-0054", "Feather"),
];
/// After Effects runs the same Lumetri plug-in as Premiere Pro; the curve and
/// wheel conversions are Premiere-measured (see `crate::lumetri`).
const LUMETRI_AE_SHARED_MEASUREMENT: &str = "measured on Premiere 26.5.2 renders of the shared Lumetri plug-in; After Effects 26.5 saves the same payload under the same ParameterID and renders the same RGB Curves within 1.16 mean 8-bit codes of Premiere; export writes the FX curves as a native Curves effect, not back into Lumetri";

/// The static payload of the explicit Lumetri arbitrary-data control `id`,
/// `None` when AE saved none (the control is at its default).
fn lumetri_payload(source: &DecodedEffect, id: usize) -> Option<Result<&[u8], String>> {
    let name = lumetri_control(id);
    let mut matches = source
        .arbitrary
        .iter()
        .filter(|control| control.match_name == name);
    let control = matches.next()?;
    if matches.next().is_some() {
        return Some(Err(format!("{name}: duplicate arbitrary-data control")));
    }
    Some(
        control
            .payload
            .as_deref()
            .map_err(|error| format!("{name}: {error}")),
    )
}

/// Whether a Lumetri section checkbox is on; an unreadable one counts as on.
fn lumetri_section(
    source: &DecodedEffect,
    native: &str,
    name: &str,
    warnings: &mut Vec<String>,
) -> bool {
    checkbox(source, native, warnings).unwrap_or_else(|reason| {
        warnings.push(format!(
            "{name} section enable unreadable ({reason}); section treated as enabled"
        ));
        true
    })
}

/// Lumetri RGB Curves and Color Wheels as FX `colorCurves`, in Lumetri order.
fn lumetri_curves_and_wheels(
    source: &DecodedEffect,
    warnings: &mut Vec<String>,
) -> Vec<LayerEffect> {
    use crate::lumetri::{self, RgbCurves, Wheels};
    let mut effects = Vec::new();
    match lumetri_payload(source, lumetri::RGB_CURVES_ID)
        .map(|bytes| bytes.and_then(RgbCurves::decode))
    {
        None => {}
        Some(Err(reason)) => warnings.push(format!("RGB Curves not imported: {reason}")),
        Some(Ok(curves)) if curves.is_identity() => {}
        Some(Ok(_)) if !lumetri_section(source, LUMETRI_CURVES_ENABLE, "Curves", warnings) => {
            warnings.push("RGB Curves not imported: its Lumetri Curves section is off".into());
        }
        Some(Ok(curves)) => match lumetri::rgb_curves::color_curves(&curves) {
            Ok(effect) => {
                effects.push(effect);
                warnings.push(format!(
                    "{} ({LUMETRI_AE_SHARED_MEASUREMENT})",
                    lumetri::rgb_curves::LUMETRI_CURVES_APPROXIMATION
                ));
            }
            Err(reason) => warnings.push(format!("RGB Curves not imported: {reason}")),
        },
    }
    let saved: [Option<Result<&[u8], String>>; 5] =
        lumetri::HUE_CURVE_IDS.map(|id| lumetri_payload(source, id));
    if saved.iter().any(Option::is_some) {
        let bytes: Result<[Option<&[u8]>; 5], String> = saved
            .into_iter()
            .map(Option::transpose)
            .collect::<Result<Vec<_>, _>>()
            .map(|bytes| std::array::from_fn(|index| bytes[index]));
        match bytes.and_then(lumetri::hue_curves::HueSatCurves::decode) {
            Err(reason) => {
                warnings.push(format!("Hue Saturation Curves not imported: {reason}"));
            }
            Ok(curves) if curves.is_identity() => {}
            Ok(_) if !lumetri_section(source, LUMETRI_CURVES_ENABLE, "Curves", warnings) => {
                warnings.push(
                    "Hue Saturation Curves not imported: its Lumetri Curves section is off".into(),
                );
            }
            Ok(curves) => match lumetri::hue_curves::to_fx(&curves) {
                Ok(fx) => {
                    effects.push(LayerEffect::HueCurves(fx));
                    warnings.push(format!(
                        "{} ({LUMETRI_AE_SHARED_MEASUREMENT})",
                        lumetri::hue_curves::HUE_CURVES_APPROXIMATION
                    ));
                }
                Err(reason) => {
                    warnings.push(format!("Hue Saturation Curves not imported: {reason}"));
                }
            },
        }
    }
    match lumetri_payload(source, lumetri::COLOR_WHEELS_ID)
        .map(|bytes| bytes.and_then(Wheels::decode))
    {
        None => {}
        Some(Err(reason)) => warnings.push(format!("Color Wheels not imported: {reason}")),
        Some(Ok(wheels)) if wheels.is_neutral() => {}
        Some(Ok(_))
            if !lumetri_section(source, LUMETRI_WHEELS_ENABLE, "Color Wheels", warnings) =>
        {
            warnings
                .push("Color Wheels not imported: its Lumetri Color Wheels section is off".into());
        }
        Some(Ok(wheels)) => match lumetri::wheels::color_curves(&wheels) {
            Ok(effect) => {
                effects.push(effect);
                warnings.push(format!(
                    "{} ({LUMETRI_AE_SHARED_MEASUREMENT})",
                    lumetri::wheels::LUMETRI_WHEELS_APPROXIMATION
                ));
            }
            Err(reason) => warnings.push(format!("Color Wheels not imported: {reason}")),
        },
    }
    effects
}

/// The Lumetri Vignette section as FX `vignette` ([`crate::lumetri::vignette`]).
fn lumetri_vignette(source: &DecodedEffect, warnings: &mut Vec<String>) -> Option<LayerEffect> {
    let mut values = [0.0, 50.0, 0.0, 50.0];
    for (value, (native, label)) in values.iter_mut().zip(LUMETRI_VIGNETTE) {
        let Ok(numeric) = numeric(source, native, Some(label)) else {
            continue;
        };
        if let Some(initial) = initial(numeric) {
            *value = initial;
        }
        if numeric.animated || numeric.expression_enabled {
            warnings.push(format!(
                "Vignette {label} ({native}): keys/expression omitted; the static FX vignette keeps the initial value"
            ));
        }
    }
    let [amount, midpoint, roundness, feather] = values;
    if amount == 0.0 {
        return None;
    }
    if !lumetri_section(source, LUMETRI_VIGNETTE_ENABLE, "Vignette", warnings) {
        warnings.push(format!(
            "Vignette Amount {amount} not imported: its Lumetri Vignette section is off"
        ));
        return None;
    }
    if roundness != 0.0 {
        warnings.push(format!(
            "Vignette Roundness {roundness}: no FX vignette counterpart; the vignette keeps its frame-aspect shape"
        ));
    }
    warnings.push(crate::lumetri::vignette::VIGNETTE_APPROXIMATION.into());
    let [amount, radius, feather] = crate::lumetri::vignette::to_fx([amount, midpoint, feather]);
    Some(LayerEffect::Vignette {
        amount: Some(amount),
        radius: Some(radius),
        feather: Some(feather),
    })
}

/// Look / Input LUT popups (1 None, 2 custom file, 3 Browse, 4 separator,
/// 5 and above built-in), and the control in which AE 26.5 saves the selected
/// built-in LUT's name (UTF-16, scripted probe); AE saves no LUT samples for
/// a built-in LUT (they ship with the application).
const LUMETRI_LUT_POPUPS: [(&str, &str, usize); 2] = [
    ("ADBE Lumetri-0005", "Input LUT", 95),
    ("ADBE Lumetri-0025", "Look", 96),
];

/// Diagnose a Look / Input LUT popup that selects no browsed file
/// ([`lumetri_luts`] imports a browsed file, popup 2).
fn diagnose_lumetri_unmapped_data(source: &DecodedEffect, warnings: &mut Vec<String>) {
    for (native, name, label_id) in LUMETRI_LUT_POPUPS {
        let Some(selected) = numeric(source, native, None).ok().and_then(initial) else {
            continue;
        };
        if selected == 1.0 || selected == 2.0 {
            continue;
        }
        let label = lumetri_payload(source, label_id)
            .and_then(Result::ok)
            .filter(|bytes| bytes.len() > 2 && bytes.len() % 2 == 0)
            .map(|bytes| {
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .take_while(|unit| *unit != 0)
                    .collect();
                String::from_utf16_lossy(&units)
            });
        warnings.push(if selected >= 5.0 {
            format!(
                "{name} ({native}): built-in LUT {} (popup {selected}) omitted: After Effects saves only its name, the LUT ships with the application and its samples are not in the project; add the LUT to FX as a lookTransform",
                label.map_or_else(|| "(name not saved)".to_owned(), |label| format!("{label:?}"))
            )
        } else {
            format!(
                "{name} ({native}): popup {selected} selects no LUT that After Effects 26.5 saves (1 None, 2 a browsed file, 5 and above built-in); omitted"
            )
        });
    }
}

/// Creative Saturation multiplies the Basic chroma scale. Fold a static value
/// into FX saturation; keys survive only when Basic Saturation is neutral.
fn creative_saturation(
    source: &DecodedEffect,
    payload: &mut Value,
    keyed: &mut Vec<KeyedControl>,
    warnings: &mut Vec<String>,
) {
    let creative = control(
        LUMETRI_CREATIVE_SATURATION,
        "Saturation",
        "saturation",
        LUMETRI_SATURATION_SCALE,
        -100.0 * LUMETRI_SATURATION_SCALE,
        [-1.0, 1.0],
    );
    let Ok(numeric) = numeric(source, creative.native, creative.label) else {
        warnings.push(format!(
            "{}: Creative Saturation unreadable; omitted",
            creative.native
        ));
        return;
    };
    let Some(value) = initial(numeric) else {
        return;
    };
    if value == 100.0 && !numeric.animated {
        return;
    }
    let basic = payload["saturation"].as_f64().unwrap_or(0.0);
    let basic_keyed = keyed.iter().any(|key| key.control.param == "saturation");
    if basic == 0.0 && !basic_keyed {
        keyed.extend(scalar_controls(
            source,
            &[creative],
            payload,
            true,
            warnings,
        ));
        return;
    }
    let basic_percent = 100.0 + basic / LUMETRI_SATURATION_SCALE;
    let combined =
        ((basic_percent * value / 100.0 - 100.0) * LUMETRI_SATURATION_SCALE).clamp(-1.0, 1.0);
    set(payload, "saturation", combined);
    warnings.push(format!(
        "{}: Creative Saturation folded multiplicatively into the Basic Saturation value{}",
        creative.native,
        if numeric.animated || basic_keyed {
            "; Creative Saturation keys omitted, Basic keys keep their own scale"
        } else {
            ""
        }
    ));
}

/// Why an imported Color Balance is an approximation (see [`color_balance`]).
pub(crate) const COLOR_BALANCE_APPROXIMATION: &str = "Color Balance imported as editable per-channel FX colorCurves (Master identity) fitted within one code to After Effects 26.5's measured transfer: per channel a Levels map (Preserve Luminosity off: input black max(0,-shadow), input white 255-max(0,highlight), gamma 2^-((2 midtone + shadow + highlight)/200); on: shadows raise black points by max(shadows)-shadow, highlights lower white points by highlight-min(highlights), midtone gamma 2^-((midtone - (max+min of midtones)/2)/100)) with an empirically fitted toe for brightening gammas and a step at the white point when the black point reaches it; against 156 measured 8 bpc AE tables (24 random held-out looks) the mean error is 0.12 codes and 99.9% of table entries are within 2 codes (worst 10 codes on a steep ramp of fractional balances). The nine balances and Preserve Luminosity are not separately editable in FX (edit the curves; export writes a native Curves effect, not Color Balance); 16/32 bpc and OCIO projects are unmeasured";

/// Lower Color Balance to per-channel curves; its balances and keys do not
/// survive as controls (FX `colorCurves` is static).
fn color_balance(source: &DecodedEffect, domain: ColorGradeDomain) -> Result<LoweredGrade, String> {
    let mut warnings = Vec::new();
    let mut values = [0.0; 9];
    let mut keyed = Vec::new();
    for (slot, control) in values.iter_mut().zip(&COLOR_BALANCE_CONTROLS) {
        let numeric = match numeric(source, control.native, control.label) {
            Ok(numeric) => numeric,
            Err(reason) => {
                warnings.push(format!("{reason}; balance 0 used"));
                continue;
            }
        };
        let Some(value) = initial(numeric) else {
            warnings.push(format!(
                "{}: finite scalar required; balance 0 used",
                control.native
            ));
            continue;
        };
        *slot = value.clamp(-100.0, 100.0);
        if *slot != value {
            warnings.push(format!(
                "{} {value}: outside AE's valid range -100..=100; clamped",
                control.native
            ));
        }
        if numeric.expression_enabled {
            warnings.push(format!(
                "{}: AE expression not executed; initial value retained",
                control.native
            ));
        }
        if numeric.animated && !numeric.keyframes.is_empty() {
            keyed.push(control.native);
        }
    }
    if !keyed.is_empty() {
        warnings.push(format!(
            "{}: keys omitted; FX colorCurves are static, so the initial balances are converted",
            keyed.join(", ")
        ));
    }
    let preserve_luminosity =
        match checkbox(source, COLOR_BALANCE_PRESERVE_LUMINOSITY, &mut warnings) {
            Ok(preserve) => preserve,
            Err(reason) => {
                warnings.push(format!(
                    "Preserve Luminosity unreadable ({reason}); AE's default (off) used"
                ));
                false
            }
        };
    let balances = color_balance::Balances {
        shadows: [values[0], values[1], values[2]],
        midtones: [values[3], values[4], values[5]],
        highlights: [values[6], values[7], values[8]],
        preserve_luminosity,
    };
    warnings.push(COLOR_BALANCE_APPROXIMATION.into());
    if domain == ColorGradeDomain::Aces {
        warnings.push("OCIO ACES project: AE applies Color Balance to ACEScg working values; the eight-bit display transfer used here is unmeasured there".into());
    }
    Ok(LoweredGrade {
        effect: LayerEffect::ColorCurves {
            semantic_version: ColorCurvesSemanticVersion::V1,
            curves: balances.curves()?,
        },
        keyed: Vec::new(),
        warnings,
        before: Vec::new(),
        after: Vec::new(),
    })
}

fn channel_mixer(source: &DecodedEffect) -> Result<LoweredGrade, String> {
    let mut warnings = Vec::new();
    let row =
        |red: f64, green: f64, blue: f64| json!({"red":red,"green":green,"blue":blue,"constant":0});
    let mut payload = json!({"type":"channelMixer","semanticVersion":1,"red":row(1.,0.,0.),"green":row(0.,1.,0.),"blue":row(0.,0.,1.),"monochrome":false});
    let keyed = scalar_controls(
        source,
        &CHANNEL_MIXER_CONTROLS,
        &mut payload,
        false,
        &mut warnings,
    );
    debug_assert!(
        keyed.is_empty(),
        "static FX channel mixer has no key targets"
    );
    match checkbox(source, CHANNEL_MIXER_MONOCHROME, &mut warnings) {
        Ok(monochrome) => payload["monochrome"] = json!(monochrome),
        Err(reason) => warnings.push(format!(
            "Monochrome unreadable ({reason}); colour output retained"
        )),
    }
    warnings.push("Channel Mixer weights/constants mapped as percent/100 onto static canonical channelMixer v1; native clipping and alpha handling are unverified".into());
    finish(payload, Vec::new(), warnings)
}

fn selective_color(source: &DecodedEffect) -> Result<LoweredGrade, String> {
    let mut warnings = Vec::new();
    let mut payload = json!({"type":"selectiveColor","semanticVersion":1,"method":"relative"});
    for control in &SELECTIVE_COLOR_CONTROLS {
        set(&mut payload, control.param, 0.0);
    }
    let keyed = scalar_controls(
        source,
        &SELECTIVE_COLOR_CONTROLS,
        &mut payload,
        false,
        &mut warnings,
    );
    debug_assert!(
        keyed.is_empty(),
        "static FX selective colour has no key targets"
    );
    match numeric(source, SELECTIVE_COLOR_METHOD, Some("Method:")).map(initial) {
        Ok(Some(1.0)) => {}
        Ok(Some(2.0)) => payload["method"] = json!("absolute"),
        Ok(other) => warnings.push(format!(
            "Method {other:?}: expected popup 1 (Relative) or 2 (Absolute); relative used"
        )),
        Err(reason) => warnings.push(format!("{reason}; relative method used")),
    }
    // The visible sliders are panel-only views of the selected range: AE reads
    // them back as 0 after open while the Details groups drive the render, so
    // the groups are authoritative and a disagreement is only diagnosed.
    if let Ok(Some(selected)) =
        numeric(source, SELECTIVE_COLOR_SELECTED_RANGE, Some("Colors:")).map(initial)
        && (1.0..=9.0).contains(&selected)
        && selected.fract() == 0.0
    {
        let range = selected as usize - 1;
        for (ink, current) in SELECTIVE_COLOR_CURRENT.iter().enumerate() {
            let group = &SELECTIVE_COLOR_CONTROLS[range * 4 + ink];
            let visible = numeric(source, current, group.label).ok().and_then(initial);
            let stored = numeric(source, group.native, group.label)
                .ok()
                .and_then(initial);
            if visible.is_some() && stored.is_some() && visible != stored {
                warnings.push(format!(
                    "{current}: visible slider {visible:?} differs from {} {stored:?}; Details group value used",
                    group.param
                ));
            }
        }
    }
    warnings.push("Selective Color inks mapped as percent/100 onto static canonical selectiveColor v1; FX range weights and ink math approximate the native effect".into());
    finish(payload, Vec::new(), warnings)
}

const PRIMARY_GRADE_FIELDS: [Field; 10] = [
    LUMETRI_BASIC[0].field(true),
    LUMETRI_BASIC[1].field(true),
    LUMETRI_BASIC[2].field(true),
    LUMETRI_BASIC[3].field(true),
    LUMETRI_BASIC[4].field(true),
    LUMETRI_BASIC[5].field(true),
    LUMETRI_BASIC[6].field(true),
    LUMETRI_BASIC[7].field(true),
    LUMETRI_BASIC[8].field(true),
    LUMETRI_VIBRANCE.field(true),
];
const PRIMARY_GRADE_FIELDS_ACES: [Field; 10] = {
    let mut fields = PRIMARY_GRADE_FIELDS;
    fields[2] = LUMETRI_EXPOSURE_ACES.field(true);
    fields
};
const TONAL_COLOR_FIELDS: [Field; 9] = Control::fields(&COLOR_BALANCE_CONTROLS, true);
const CHANNEL_MIXER_FIELDS: [Field; 13] = [
    CHANNEL_MIXER_CONTROLS[0].field(false),
    CHANNEL_MIXER_CONTROLS[1].field(false),
    CHANNEL_MIXER_CONTROLS[2].field(false),
    CHANNEL_MIXER_CONTROLS[3].field(false),
    CHANNEL_MIXER_CONTROLS[4].field(false),
    CHANNEL_MIXER_CONTROLS[5].field(false),
    CHANNEL_MIXER_CONTROLS[6].field(false),
    CHANNEL_MIXER_CONTROLS[7].field(false),
    CHANNEL_MIXER_CONTROLS[8].field(false),
    CHANNEL_MIXER_CONTROLS[9].field(false),
    CHANNEL_MIXER_CONTROLS[10].field(false),
    CHANNEL_MIXER_CONTROLS[11].field(false),
    Field {
        native: CHANNEL_MIXER_MONOCHROME,
        field: "monochrome",
        param: "monochrome",
        component: 0,
        scale: Scale::Identity,
        offset: 0.0,
        boolean: true,
        animated: false,
    },
];
const SELECTIVE_COLOR_FIELDS: [Field; 36] = Control::fields(&SELECTIVE_COLOR_CONTROLS, false);

/// FX `vignette` as Lumetri's Vignette section. The mapping is not affine
/// ([`crate::lumetri::vignette::from_fx`] sets the values in [`export`]), so
/// these rows only name the controls and keys are not exported.
const VIGNETTE_FIELDS: [Field; 3] = [
    vignette_field("ADBE Lumetri-0051", "amount", -0.2),
    vignette_field("ADBE Lumetri-0052", "radius", 0.01),
    vignette_field("ADBE Lumetri-0054", "feather", 0.01),
];
const fn vignette_field(native: &'static str, param: &'static str, scale: f64) -> Field {
    Field {
        native,
        field: param,
        param,
        component: 0,
        scale: Scale::Factor(scale),
        offset: 0.0,
        boolean: false,
        animated: false,
    }
}
/// The FX renderer's values for absent `vignette` fields.
const FX_VIGNETTE_DEFAULTS: [f64; 3] = [0.5, 0.75, 0.35];

static EXPORT_MAPPINGS: [Mapping; 7] = [
    Mapping {
        native: LUMETRI,
        fx_type: "primaryGrade",
        fields: &PRIMARY_GRADE_FIELDS,
        note: "primaryGrade exported as fresh Lumetri Basic Correction/Creative scalars by the inverse of the fitted import scales (Temperature x116, Tint x-100, Exposure x2.6, Contrast x360, Highlights x240, Shadows x510, Whites x54, Blacks x124, Saturation 100+110s, Vibrance x167), clamped to each control's declared native valid range. The 24 Lumetri arbitrary-data controls (Blob sentinel, curves, wheels, HSL, LUT slots, Auto Tone analytics) carry AE 26.5's native default payloads, byte-equal to the pinned native instances, so that state stays at default. Adobe 26.5 opened, read back and rendered this export identical to the native color_ae_lumetri_basic reference; the scales' measured residual against Lumetri's own math is in the import diagnostic.",
    },
    Mapping {
        native: COLOR_BALANCE,
        fx_type: "tonalColor",
        fields: &TONAL_COLOR_FIELDS,
        note: "tonalColor exported as fresh Color Balance range balances x100 with Preserve Luminosity on; native range weights and colour math differ and are unmeasured.",
    },
    Mapping {
        native: CHANNEL_MIXER,
        fx_type: "channelMixer",
        fields: &CHANNEL_MIXER_FIELDS,
        note: "channelMixer exported as fresh Channel Mixer weights/constants x100 and Monochrome; native clipping/alpha handling is unmeasured.",
    },
    Mapping {
        native: SELECTIVE_COLOR,
        fx_type: "selectiveColor",
        fields: &SELECTIVE_COLOR_FIELDS,
        note: "selectiveColor exported as fresh Selective Color Details-group inks x100 and Method; the visible Cyan/Magenta/Yellow/Black sliders are written with the Reds values, but AE reads them back as 0 after open (panel-only; the Details groups drive the render). FX and native ink math differ and are unmeasured.",
    },
    Mapping {
        native: LUMETRI,
        fx_type: "vignette",
        fields: &VIGNETTE_FIELDS,
        note: "vignette exported as a fresh Lumetri Color whose Vignette section carries it by the inverse of the fitted Lumetri import (Roundness 0), every other Lumetri section at AE 26.5's defaults; keys are not exported (the mapping is not affine).",
    },
    Mapping {
        native: LUMETRI,
        fx_type: "hueCurves",
        fields: &[],
        note: "hueCurves exported as a fresh Lumetri Color whose Hue Saturation Curves (Hue vs Sat, Hue vs Hue, Hue vs Luma, Luma vs Sat, Sat vs Sat) carry it, matched at reference colours (crate::lumetri::hue_curves); every other Lumetri section at AE 26.5's defaults. Lumetri's curve math was measured in Premiere 26.5.2, whose Lumetri renders the same payloads as AE 26.5.",
    },
    Mapping {
        native: super::curves::MATCH_NAME,
        fx_type: "colorCurves",
        fields: &[],
        note: "colorCurves exported as a fresh Curves effect whose eight-bit RGB/Red/Green/Blue tables sample the piecewise-linear FX curves (alpha identity); AE renders the tables. FX knots are also written as AE editor points when they fit its 16-point editor, so the curve panel shows them as AE's spline; a Master beside channel curves is folded into the channel tables because AE applies each channel before the RGB curve. HDR behaviour is unmeasured.",
    },
];

/// The `primaryGrade` export mapping in an OCIO ACES project: the legacy
/// mapping with the stops/2.2 Exposure row.
static PRIMARY_GRADE_ACES_MAPPING: Mapping = Mapping {
    native: LUMETRI,
    fx_type: "primaryGrade",
    fields: &PRIMARY_GRADE_FIELDS_ACES,
    note: "primaryGrade exported in an OCIO ACES project as fresh Lumetri Basic Correction/Creative scalars by the inverse of the import scales (Temperature x116, Tint x-100, Exposure x2.2 (ACES), Contrast x360, Highlights x240, Shadows x510, Whites x54, Blacks x124, Saturation 100+110s, Vibrance x167), clamped to each control's declared native valid range; the 24 Lumetri arbitrary-data controls carry AE 26.5's native default payloads. The ACES Exposure scale is an unaccepted approximation (see the import diagnostic).",
};

/// The export mapping for one canonical colour grade in `domain`.
pub(crate) fn export_mapping_in(
    fx_type: &str,
    domain: ColorGradeDomain,
) -> Option<&'static Mapping> {
    if domain == ColorGradeDomain::Aces && fx_type == PRIMARY_GRADE_ACES_MAPPING.fx_type {
        return Some(&PRIMARY_GRADE_ACES_MAPPING);
    }
    export_mapping(fx_type)
}

/// Export-only mapping for one canonical colour grade.
pub(crate) fn export_mapping(fx_type: &str) -> Option<&'static Mapping> {
    EXPORT_MAPPINGS
        .iter()
        .find(|mapping| mapping.fx_type == fx_type)
}

fn set_native(effect: &mut NativeEffect, match_name: &str, value: f64) -> bool {
    effect
        .properties
        .iter_mut()
        .find(|property| property.match_name == match_name)
        .and_then(|property| property.values.first_mut())
        .map(|slot| *slot = value)
        .is_some()
}

fn native_value(effect: &NativeEffect, match_name: &str) -> Option<f64> {
    effect
        .properties
        .iter()
        .find(|property| property.match_name == match_name)
        .and_then(|property| property.values.first().copied())
}

/// Controls outside the affine rows: Color Balance folding and luminosity,
/// Selective Color method and visible sliders, and the Curves table payload.
///
/// `mapping` is the mapping `native` was lowered with; its rows bound the
/// native valid-range clamp.
pub(crate) fn export(
    effect: &LayerEffect,
    mapping: &Mapping,
    native: &mut NativeEffect,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if EXPORT_MAPPINGS
        .iter()
        .any(|grade| grade.native == mapping.native)
    {
        clamp_to_native_valid_range(mapping, native, &mut warnings);
    }
    match effect {
        LayerEffect::ColorCurves { curves, .. } => {
            let (payload, approximations) = super::curves::encode(curves);
            native.arbitrary.push(NativeArbitraryData {
                match_name: super::curves::CONTROL.into(),
                payload,
            });
            warnings.extend(approximations);
        }
        LayerEffect::TonalColor(tonal) => {
            if !set_native(native, COLOR_BALANCE_PRESERVE_LUMINOSITY, 1.0) {
                warnings.push("Preserve Luminosity control missing from the native catalog".into());
            }
            let globals = [tonal.global_red, tonal.global_green, tonal.global_blue];
            if globals.iter().any(|value| *value != 0.0) {
                for (index, control) in COLOR_BALANCE_CONTROLS.iter().enumerate() {
                    let global = globals[index % 3] * 100.0;
                    if let Some(range) = native_value(native, control.native) {
                        let folded = (range + global).clamp(-100.0, 100.0);
                        if folded != range + global {
                            warnings.push(format!(
                                "{}: global plus range balance exceeds +/-100; clamped",
                                control.native
                            ));
                        }
                        set_native(native, control.native, folded);
                    }
                }
                warnings.push("global offsets folded equally into the shadow/midtone/highlight balances (AE has no global balance); global keys are not folded".into());
            }
        }
        LayerEffect::HueCurves(curves) => {
            match crate::lumetri::hue_curves::from_fx(curves).encode() {
                Some(saved) => native.arbitrary.extend(saved.into_iter().map(|(id, payload)| {
                    NativeArbitraryData {
                        match_name: lumetri_control(id),
                        payload,
                    }
                })),
                None => warnings.push(
                    "a fitted Lumetri hue curve needs more than 32 points; the curves stay at their defaults".into(),
                ),
            }
            warnings.push(crate::lumetri::hue_curves::HUE_CURVES_APPROXIMATION.into());
        }
        LayerEffect::Vignette {
            amount,
            radius,
            feather,
        } => {
            let fx: [f64; 3] = std::array::from_fn(|index| {
                [amount, radius, feather][index].unwrap_or(FX_VIGNETTE_DEFAULTS[index])
            });
            let values = crate::lumetri::vignette::from_fx(fx);
            for ((field, value), (min, max)) in
                VIGNETTE_FIELDS
                    .iter()
                    .zip(values)
                    .zip([(-5.0, 5.0), (0.0, 100.0), (0.0, 100.0)])
            {
                let clamped = value.clamp(min, max);
                if clamped != value {
                    warnings.push(format!(
                        "vignette {} maps to {} {value}, outside its saved range {min}..={max}; written as {clamped}",
                        field.param, field.native
                    ));
                }
                set_native(native, field.native, clamped);
            }
            warnings.push(crate::lumetri::vignette::VIGNETTE_APPROXIMATION.into());
        }
        LayerEffect::SelectiveColor(selective) => {
            let method = match selective.method {
                SelectiveColorMethod::Relative => 1.0,
                SelectiveColorMethod::Absolute => 2.0,
            };
            set_native(native, SELECTIVE_COLOR_METHOD, method);
            set_native(native, SELECTIVE_COLOR_SELECTED_RANGE, 1.0);
            for (ink, current) in SELECTIVE_COLOR_CURRENT.iter().enumerate() {
                if let Some(value) = native_value(native, SELECTIVE_COLOR_CONTROLS[ink].native) {
                    set_native(native, current, value);
                }
            }
        }
        _ => {}
    }
    warnings
}

/// Clamp every affine-mapped static value and key to the native control's
/// declared valid range. With the fitted Lumetri scales several FX ranges map
/// beyond it: Exposure (FX ±5 stops x2.6 = ±13, native valid ±7), Contrast
/// (±2 x360), Highlights (±1 x240), Shadows (±1 x510) and Saturation
/// (100+110s below 0); such values are written at the native limit.
fn clamp_to_native_valid_range(
    mapping: &Mapping,
    native: &mut NativeEffect,
    warnings: &mut Vec<String>,
) {
    let Some(definition) = super::definitions::definition(mapping.native) else {
        return;
    };
    for field in mapping.fields.iter().filter(|field| !field.boolean) {
        let Scale::Factor(scale) = field.scale else {
            continue;
        };
        let Some((min, max)) = definition
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == field.native)
            .and_then(super::definitions::ParameterDefinition::valid_range)
        else {
            continue;
        };
        let Some(property) = native
            .properties
            .iter_mut()
            .find(|property| property.match_name == field.native)
        else {
            continue;
        };
        let fx = |value: f64| value * scale + field.offset;
        let label = &property.display_name;
        if let Some(slot) = property.values.first_mut()
            && !(min..=max).contains(slot)
        {
            let clamped = slot.clamp(min, max);
            warnings.push(format!(
                "{label} ({}) {} (FX {} {}) is outside AE's declared valid range {min}..={max}; written as {clamped} (FX {} {})",
                field.native,
                *slot,
                field.param,
                fx(*slot),
                field.param,
                fx(clamped)
            ));
            *slot = clamped;
        }
        let mut clamped_keys = Vec::new();
        for key in property
            .animation
            .iter_mut()
            .flat_map(|track| track.keys.iter_mut())
        {
            if let Some(value) = key.values.first_mut()
                && !(min..=max).contains(value)
            {
                clamped_keys.push(*value);
                *value = value.clamp(min, max);
            }
        }
        if !clamped_keys.is_empty() {
            warnings.push(format!(
                "{label} ({}) keys {clamped_keys:?} (FX {} {:?}) are outside AE's declared valid range {min}..={max}; written at the nearest limit, key times and easing unchanged",
                field.native,
                field.param,
                clamped_keys.iter().map(|value| fx(*value)).collect::<Vec<_>>()
            ));
        }
    }
}

#[cfg(test)]
mod tests;
