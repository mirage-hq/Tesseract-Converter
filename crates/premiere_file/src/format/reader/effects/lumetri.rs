//! Lumetri Color: Basic Correction and Creative scalars as one editable
//! `primaryGrade`, RGB Curves as an FX `colorCurves`, the Hue Saturation
//! Curves as an FX `hueCurves`, the Color Wheels as a second `colorCurves`
//! following their measured per-channel transfer, the Vignette section as an
//! FX `vignette`, and a contextual diagnosis of every other section whose
//! saved state differs from the pinned defaults
//! ([`crate::schema::LUMETRI_DEFAULTS`]). Parameters are found by
//! `ParameterID`, so the Premiere 22/23 layout (103 parameters) reads like the
//! 26.5.2 one. Other saved blobs are compared, not interpreted.
use super::*;
use crate::schema::{
    lumetri_default, native::ArbVideoComponentParam, LumetriDefault, PrLumetriGrade,
    PrScalarKeyframe, LUMETRI_BASIC_ENABLE, LUMETRI_CREATIVE_ENABLE, LUMETRI_CREATIVE_SATURATION,
    LUMETRI_GRADE_APPROXIMATION, LUMETRI_GRADE_NEUTRAL, LUMETRI_GRADE_PARAMS, LUMETRI_GRADE_RANGES,
    LUMETRI_GRADE_SATURATION, LUMETRI_MATCH_NAME, LUMETRI_SATURATION, LUMETRI_VIBRANCE,
    LUMETRI_VIGNETTE_ENABLE, LUMETRI_VIGNETTE_PARAMS,
};
use aftereffects_file::lumetri::{
    hue_curves::HueSatCurves, LumetriCurve, RgbCurves, Wheels, COLOR_WHEELS_ID, HUE_CURVE_IDS,
    RGB_CURVES_ID,
};

pub(super) const MATCH_NAME: &str = LUMETRI_MATCH_NAME;
pub(super) const REPLACEMENT: &str = "Lumetri Color converts its Basic Correction and Creative scalars to one editable primaryGrade, its RGB Curves to a colorCurves, its Hue Saturation Curves to a hueCurves matched at reference colours, its Color Wheels to a colorCurves following their measured per-channel lift, power and gain, and its Vignette section to an FX vignette by a fit to After Effects 26.5 renders of the same plug-in (Roundness is replaced by a circular falloff); this is not an equivalent Lumetri transfer. Premiere renders by these saved scalars (the Blob is a constant sentinel). Other sections whose saved state differs from Lumetri's defaults are diagnosed separately; edited export writes current FX controls, never the original Lumetri blobs";

// Interpreted-source admission must not mistake a lossy replacement for proof
// that the original effect is static and commutes with source interpretation.
pub(super) fn reject_interpretation(
    _: &Graph<'_>,
    _: Record<'_>,
    _: &NativeEffect<'_>,
) -> Result<PrEffect> {
    Err(unsupported(
        "Lumetri saved-control replacement does not prove the original effect static",
    ))
}

/// A Lumetri section that no FX effect maps: its saved members, compared
/// with the pinned defaults, and why a difference is lost.
struct UnmappedSection {
    name: &'static str,
    /// The section's enable checkbox, reported when it is off.
    enable: Option<&'static EffectParamSpec>,
    /// `ParameterID`s of the saved controls and values. Disclosure headers,
    /// group ends and eyedropper selectors are not compared.
    members: &'static [usize],
    reason: &'static str,
}

const UNMAPPED_SECTIONS: [UnmappedSection; 11] = [
    UnmappedSection {
        name: "Basic Correction Input LUT",
        enable: Some(&LUMETRI_BASIC_ENABLE),
        members: &[5, 127, 95, 125],
        reason: "the Premiere control Lumetri Color > Basic Correction > Input LUT selects a built-in LUT (popup) or a LUTAsset; for a built-in LUT Premiere 26.5.2 saves only the popup index (scripted probe) and the LUT ships with Premiere, so its samples are not in the project (a browsed .cube is read from ParameterID 4); no source Input Transform was set: set the clip's source input transform to the LUT in FX instead",
    },
    UnmappedSection {
        name: "Basic Correction HDR controls",
        enable: Some(&LUMETRI_BASIC_ENABLE),
        members: &[63, 64, 119, 120],
        reason: "HDR White, HDR Specular and HDR tone mapping have no primaryGrade equivalent",
    },
    UnmappedSection {
        name: "Creative Look",
        enable: Some(&LUMETRI_CREATIVE_ENABLE),
        members: &[25, 128, 96, 126],
        reason: "the Premiere control Lumetri Color > Creative > Look selects a built-in Look (popup) or a LookAsset; for a built-in Look Premiere 26.5.2 saves only the popup index (scripted probe) and the LUT ships with Premiere, so its samples are not in the project (a browsed .cube is read from ParameterID 24); no lookTransform was created: add a lookTransform with the LUT in FX instead",
    },
    UnmappedSection {
        name: "Creative Faded Film",
        enable: Some(&LUMETRI_CREATIVE_ENABLE),
        members: &[28],
        reason: "Faded Film has no primaryGrade equivalent",
    },
    UnmappedSection {
        name: "Creative Sharpen",
        enable: Some(&LUMETRI_CREATIVE_ENABLE),
        members: &[29],
        reason: "Creative Sharpen is not part of primaryGrade, and no sharpen kernel measurement maps it",
    },
    UnmappedSection {
        name: "Creative Shadow and Highlight Tint",
        enable: Some(&LUMETRI_CREATIVE_ENABLE),
        members: &[32, 33],
        reason: "the tint wheels are an unverified binary value and Tint Balance has no primaryGrade equivalent",
    },
    UnmappedSection {
        name: "RGB Curves HDR Range",
        enable: Some(&CURVES_ENABLE),
        members: &[65],
        reason: "HDR Range scales the curves of HDR footage and has no colorCurves equivalent",
    },
    UnmappedSection {
        name: "Color Wheels & Match HDR White and wheel mode",
        enable: Some(&WHEELS_ENABLE),
        members: &[66, 101, 103],
        reason: "HDR White and the wheel-mode checkboxes were not measured; the wheels themselves import as measured in the default three-wheel mode on SDR footage",
    },
    UnmappedSection {
        name: "HSL Secondary",
        enable: Some(&HSL_ENABLE),
        members: &[
            70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 81, 82, 83, 85, 86, 87, 88, 89, 90, 93,
        ],
        reason: "Premiere 26.5.2 renders of in-house key values decode the 40-byte key (a leading byte that changed nothing, then Hue, Saturation and Lightness as f32 centre, range and softness plus an enable byte) but show it is not FX's qualifier: Saturation and Lightness select HLS saturation and lightness where FX selects HSV saturation and Rec. 709 luminance, the soft hue edge falls near-linearly from a cusp where FX's shoulder is a smoothstep (about 0.1 of the matte apart), and range 0.1 without softness selects nothing; the 192-byte correction was not decoded; no qualified compositingOptions correction was created",
    },
    UnmappedSection {
        name: "media-embedded LUT",
        enable: None,
        // 98 holds the samples of a browsed Input LUT or Look (`lumetri_lut`).
        members: &[97, 99, 100],
        reason: "embedded LUT selection and data are unverified binary values",
    },
    UnmappedSection {
        name: "Vignette Roundness",
        enable: Some(&LUMETRI_VIGNETTE_ENABLE),
        members: &[53],
        reason: "FX vignette has a circular falloff",
    },
];

/// Section enables named one space, like Basic Correction's.
const CURVES_ENABLE: EffectParamSpec = EffectParamSpec {
    id: 37,
    label: "Lumetri Curves enable",
    ..LUMETRI_BASIC_ENABLE
};
const WHEELS_ENABLE: EffectParamSpec = EffectParamSpec {
    id: 46,
    label: "Lumetri Color Wheels enable",
    ..LUMETRI_BASIC_ENABLE
};
/// `ParameterID`s of the Color Wheels mode checkboxes.
const WHEEL_MODE_IDS: [usize; 2] = [101, 103];
const HSL_ENABLE: EffectParamSpec = EffectParamSpec {
    id: 68,
    label: "Lumetri HSL Secondary enable",
    ..LUMETRI_BASIC_ENABLE
};

/// The effects of one Lumetri Color: its grade, its RGB Curves, its Hue
/// Saturation Curves, its Color Wheels, then its vignette, the order in which Lumetri applies those sections. A section at its neutral state
/// (every grade control neutral and unkeyed, or Vignette Amount 0) or whose
/// Lumetri section is off adds no effect, so a written grade or vignette
/// rereads without a no-op companion. A section that cannot be read is
/// diagnosed in `omissions` without discarding the other; approximations of
/// the read grade are noted in `approximations`.
pub(super) fn read(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
    approximations: &mut Vec<String>,
    omissions: &mut Vec<String>,
) -> Result<Vec<PrEffect>> {
    let enabled = native.enabled()?;
    let filter = graph.decode::<VideoFilterComponent>(record)?;
    ensure!(
        filter.value.video_filter_type.as_deref() == Some("2"),
        "unsupported Lumetri VideoFilterType"
    );
    let references = filter
        .value
        .component
        .and_then(|body| body.params)
        .ok_or_else(|| unsupported("missing Lumetri parameters"))?;
    let mut controls = BTreeMap::new();
    for reference in &references.items {
        let record = graph.locate(reference, &native.identity)?;
        let id = record
            .element()
            .child("ParameterID")
            .and_then(Element::text)
            .ok_or_else(|| unsupported("missing Lumetri ParameterID"))?;
        // Shared identity ambiguity is not safe to salvage per control.
        ensure!(
            controls.insert(id, record).is_none(),
            "duplicate Lumetri ParameterID {id}"
        );
    }
    let luts = super::lumetri_lut::read(graph, &controls, enabled, approximations, omissions);
    let mut effects: Vec<PrEffect> = luts.input.into_iter().collect();
    match read_grade(graph, &controls, enabled, approximations, omissions) {
        Ok(effect) if is_neutral_grade(&effect) => {}
        Ok(effect) => {
            approximations.push(LUMETRI_GRADE_APPROXIMATION.to_owned());
            effects.push(effect);
        }
        Err(error) => omissions.push(format!(
            "Lumetri Basic Correction and Creative grade omitted: {}; other sections retained",
            reason(error)
        )),
    }
    effects.extend(luts.look);
    match read_rgb_curves(graph, &controls, enabled, omissions) {
        Ok(effect) => effects.extend(effect),
        Err(error) => omissions.push(format!(
            "Lumetri RGB Curves omitted: {}; other sections retained",
            reason(error)
        )),
    }
    match read_hue_curves(graph, &controls, enabled, omissions) {
        Ok(effect) => effects.extend(effect),
        Err(error) => omissions.push(format!(
            "Lumetri Hue Saturation Curves omitted: {}; no hueCurves was created for them, other sections retained",
            reason(error)
        )),
    }
    match read_color_wheels(graph, &controls, enabled, omissions) {
        Ok(effect) => effects.extend(effect),
        Err(error) => omissions.push(format!(
            "Lumetri Color Wheels omitted: {}; no colorCurves was created for them, other sections retained",
            reason(error)
        )),
    }
    match read_vignette(graph, &controls, enabled, omissions) {
        Ok(effect) => effects.extend(effect),
        Err(error) => omissions.push(format!(
            "Lumetri Vignette replacement omitted: {}; other sections retained",
            reason(error)
        )),
    }
    diagnose_unmapped_sections(graph, &controls, omissions);
    Ok(effects)
}

/// Whether `grade` changes no picture: every control at its neutral saved
/// value and none keyed.
fn is_neutral_grade(grade: &PrEffect) -> bool {
    matches!(
        grade.params,
        PrEffectParams::LumetriGrade(PrLumetriGrade { values, .. })
            if values == LUMETRI_GRADE_NEUTRAL
    ) && grade.animations.is_empty()
}

/// The Basic Correction and Creative grade. A section that is off imports
/// its controls neutral; a control that cannot be read imports neutral with
/// a diagnostic, keeping its siblings. Values beyond what primaryGrade
/// represents clamp with a diagnostic ([`LUMETRI_GRADE_RANGES`]).
fn read_grade(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enabled: bool,
    approximations: &mut Vec<String>,
    omissions: &mut Vec<String>,
) -> Result<PrEffect> {
    let basic = section_enabled(graph, controls, &LUMETRI_BASIC_ENABLE)?;
    let creative = section_enabled(graph, controls, &LUMETRI_CREATIVE_ENABLE)?;
    let mut values = LUMETRI_GRADE_NEUTRAL;
    let mut keyed: Vec<(&'static EffectParamSpec, Vec<PrScalarKeyframe>)> = Vec::new();
    for (index, param) in LUMETRI_GRADE_PARAMS.into_iter().enumerate() {
        let section_on = if param.id == LUMETRI_VIBRANCE.id {
            creative
        } else {
            basic
        };
        match read_control(graph, controls, param) {
            Ok((value, keys)) if !section_on => {
                if value != LUMETRI_GRADE_NEUTRAL[index] || keys.is_some() {
                    omissions.push(format!(
                        "{} not imported: its Lumetri section is off, so the control imports neutral",
                        param.label
                    ));
                }
            }
            Ok((value, keys)) => {
                values[index] = value;
                if let Some(keys) = keys {
                    keyed.push((param, keys));
                }
            }
            Err(error) => omissions.push(format!(
                "{} omitted: {}; other Lumetri grade controls retained",
                param.label,
                reason(error)
            )),
        }
    }
    fold_creative_saturation(
        graph,
        controls,
        creative,
        &mut values,
        &mut keyed,
        approximations,
        omissions,
    );
    for (index, param) in LUMETRI_GRADE_PARAMS.into_iter().enumerate() {
        let [low, high] = LUMETRI_GRADE_RANGES[index];
        if !(low..=high).contains(&values[index]) {
            let clamped = values[index].clamp(low, high);
            approximations.push(format!(
                "{} {} clamped to {clamped}: primaryGrade represents saved values {low} to {high}",
                param.label, values[index]
            ));
            values[index] = clamped;
        }
        if let Some((_, keys)) = keyed.iter_mut().find(|(keyed, _)| keyed.id == param.id) {
            if keys.iter().any(|key| !(low..=high).contains(&key.value)) {
                for key in keys.iter_mut() {
                    key.value = key.value.clamp(low, high);
                }
                approximations.push(format!(
                    "{} key values clamped to {low} to {high}, the saved values primaryGrade represents",
                    param.label
                ));
            }
        }
    }
    Ok(PrEffect {
        enabled,
        mask: None,
        params: PrEffectParams::LumetriGrade(PrLumetriGrade {
            values,
            vignette: None,
        }),
        animations: keyed
            .into_iter()
            .map(|(param, keys)| PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Scalar(keys),
            })
            .collect(),
    })
}

/// Fold a static Creative Saturation multiplicatively into the Basic
/// Saturation value and keys: FX has one saturation control. A keyed or
/// unreadable Creative Saturation is diagnosed and left out.
fn fold_creative_saturation(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    creative: bool,
    values: &mut [f64; 10],
    keyed: &mut [(&'static EffectParamSpec, Vec<PrScalarKeyframe>)],
    approximations: &mut Vec<String>,
    omissions: &mut Vec<String>,
) {
    let label = LUMETRI_CREATIVE_SATURATION.label;
    let percent = match read_control(graph, controls, &LUMETRI_CREATIVE_SATURATION) {
        Ok((percent, _)) => percent,
        Err(error) => {
            omissions.push(format!(
                "{label} omitted: {}; other Lumetri grade controls retained",
                reason(error)
            ));
            return;
        }
    };
    if percent == 100.0 {
        return;
    }
    if !creative {
        omissions.push(format!(
            "{label} not imported: its Lumetri section is off, so the control imports neutral"
        ));
        return;
    }
    let factor = percent / 100.0;
    values[LUMETRI_GRADE_SATURATION] *= factor;
    if let Some((_, keys)) = keyed
        .iter_mut()
        .find(|(param, _)| param.id == LUMETRI_SATURATION.id)
    {
        for key in keys {
            key.value *= factor;
        }
    }
    approximations.push(format!(
        "{label} {percent} folded multiplicatively into Saturation: FX has one saturation control, so an edit no longer distinguishes the Basic and Creative controls"
    ));
}

/// The record of the layout parameter `param`.
fn control<'a>(
    controls: &BTreeMap<&str, Record<'a>>,
    param: &EffectParamSpec,
) -> Result<Record<'a>> {
    controls
        .get(param.id.to_string().as_str())
        .copied()
        .ok_or_else(|| unsupported(format!("missing {}", param.label)))
}

/// A grade control's static value, or its first key's value and its keys.
fn read_control(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    param: &'static EffectParamSpec,
) -> Result<(f64, Option<Vec<PrScalarKeyframe>>)> {
    let record = control(controls, param)?;
    let saved = selected_param(graph, record)?;
    ensure!(
        saved.name.as_deref() == Some(param.name),
        "unexpected {} name",
        param.label
    );
    match param_value(
        &saved,
        record.element(),
        param,
        &record.identity(),
        false,
        false,
    )? {
        ParamValue::Static(value) => Ok((
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| unsupported(format!("invalid {} value", param.label)))?,
            None,
        )),
        ParamValue::Keyed(keys) => {
            let keys = keys
                .scalar()
                .filter(|keys| !keys.is_empty())
                .ok_or_else(|| unsupported(format!("missing {} scalar keys", param.label)))?
                .to_vec();
            Ok((keys[0].value, Some(keys)))
        }
    }
}

/// A static section enable checkbox, named one literal space (not its
/// disclosure header, which carries the section name).
fn section_enabled(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enable: &EffectParamSpec,
) -> Result<bool> {
    let record = control(controls, enable)?;
    let param = selected_param(graph, record)?;
    ensure!(
        param.name.as_deref() == Some(" "),
        "unexpected {} name",
        enable.label
    );
    require_static(&param, enable.label)?;
    let ParamValue::Static(value) = param_value(
        &param,
        record.element(),
        enable,
        &record.identity(),
        false,
        false,
    )?
    else {
        return Err(unsupported(format!("keyed {}", enable.label)));
    };
    match value.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(unsupported(format!("invalid {}", enable.label))),
    }
}

fn selected_param(graph: &Graph<'_>, record: Record<'_>) -> Result<VideoComponentParam> {
    ensure!(
        record.tag() == "VideoComponentParam",
        "unexpected Lumetri parameter record"
    );
    only_children(record.element(), &PARAM_CHILDREN, "Lumetri parameter ")?;
    Ok(graph.decode::<VideoComponentParam>(record)?.value)
}

fn require_static(param: &VideoComponentParam, control: &str) -> Result<()> {
    ensure!(
        param
            .is_time_varying
            .as_deref()
            .is_none_or(|value| value == "false")
            && param.keyframes.as_deref().is_none_or(str::is_empty),
        "Lumetri replacement requires static {control}"
    );
    Ok(())
}

/// The RGB Curves of the Curves section, enabled with the effect: none
/// when every curve is the diagonal, or when the section is off, which is
/// diagnosed for changed curves. Undecodable or keyed curves are an error.
fn read_rgb_curves(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enabled: bool,
    omissions: &mut Vec<String>,
) -> Result<Option<PrEffect>> {
    let record = controls
        .get(RGB_CURVES_ID.to_string().as_str())
        .copied()
        .ok_or_else(|| unsupported("missing RGB Curves"))?;
    let curves = RgbCurves::decode(&arbitrary_bytes(graph, record)?).map_err(unsupported)?;
    if curves.is_identity() {
        return Ok(None);
    }
    if !section_enabled(graph, controls, &CURVES_ENABLE)? {
        omissions.push(
            "Lumetri RGB Curves not imported: its Lumetri Curves section is off, so no colorCurves was created"
                .to_owned(),
        );
        return Ok(None);
    }
    Ok(Some(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::LumetriCurves(curves),
        animations: Vec::new(),
    }))
}

/// The Hue Saturation Curves (ParameterIDs 42, 106, 108, 110, 112) of the
/// Curves section, enabled with the effect: none when every curve is
/// neutral, or when the section is off, which is diagnosed for changed
/// curves. Undecodable or keyed curves are an error.
fn read_hue_curves(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enabled: bool,
    omissions: &mut Vec<String>,
) -> Result<Option<PrEffect>> {
    // Premiere 22/23 saves only Hue vs Sat; an absent curve has no points.
    let mut saved: [Option<Vec<u8>>; 5] = Default::default();
    for (slot, id) in saved.iter_mut().zip(HUE_CURVE_IDS) {
        if let Some(record) = controls.get(id.to_string().as_str()) {
            *slot = Some(arbitrary_bytes(graph, *record)?);
        }
    }
    let curves =
        HueSatCurves::decode(saved.each_ref().map(Option::as_deref)).map_err(unsupported)?;
    if curves.is_identity() {
        return Ok(None);
    }
    if !section_enabled(graph, controls, &CURVES_ENABLE)? {
        omissions.push(
            "Lumetri Hue Saturation Curves not imported: its Lumetri Curves section is off, so no hueCurves was created"
                .to_owned(),
        );
        return Ok(None);
    }
    Ok(Some(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::LumetriHueCurves(curves),
        animations: Vec::new(),
    }))
}

/// The Color Wheels value (ParameterID 47), enabled with the effect; none
/// when its Shadows, Midtones and Highlights wheels are neutral, or when the
/// section is off, which is diagnosed for moved wheels. The wheels are
/// measured only in the default three-wheel mode, so a changed wheel-mode
/// checkbox (ParameterID 101 or 103) is not read.
fn read_color_wheels(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enabled: bool,
    omissions: &mut Vec<String>,
) -> Result<Option<PrEffect>> {
    let record = controls
        .get(COLOR_WHEELS_ID.to_string().as_str())
        .copied()
        .ok_or_else(|| unsupported("missing Color Wheels"))?;
    let wheels = Wheels::decode(&arbitrary_bytes(graph, record)?).map_err(unsupported)?;
    if wheels.is_neutral() {
        return Ok(None);
    }
    if !section_enabled(graph, controls, &WHEELS_ENABLE)? {
        omissions.push(
            "Lumetri Color Wheels not imported: its Lumetri Color Wheels section is off, so no colorCurves was created"
                .to_owned(),
        );
        return Ok(None);
    }
    for id in WHEEL_MODE_IDS {
        if controls
            .get(id.to_string().as_str())
            .is_none_or(|record| differs_from_default(graph, *record, id))
        {
            return Err(unsupported(format!(
                "the wheel-mode checkbox ParameterID {id} differs from the measured three-wheel default"
            )));
        }
    }
    Ok(Some(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::LumetriWheels(wheels),
        animations: Vec::new(),
    }))
}

/// The static Amount, Midpoint and Feather of the Vignette section, enabled
/// with the effect; none when Amount is 0, which darkens nothing, or when
/// the section is off, which is diagnosed for a nonzero Amount.
fn read_vignette(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enabled: bool,
    omissions: &mut Vec<String>,
) -> Result<Option<PrEffect>> {
    let section_enabled = section_enabled(graph, controls, &LUMETRI_VIGNETTE_ENABLE)?;
    let mut values = [0.0; 3];
    for (value, spec) in values.iter_mut().zip(LUMETRI_VIGNETTE_PARAMS) {
        let record = control(controls, spec)?;
        let param = selected_param(graph, record)?;
        ensure!(
            param.name.as_deref() == Some(spec.name),
            "unexpected {} name",
            spec.label
        );
        let ParamValue::Static(raw) = param_value(
            &param,
            record.element(),
            spec,
            &record.identity(),
            false,
            false,
        )?
        else {
            return Err(unsupported("keyed Vignette control"));
        };
        *value = raw
            .parse::<f64>()
            .map_err(|_| unsupported("invalid Vignette numeric value"))?;
        ensure!(
            spec.value_range()
                .is_some_and(|range| range.contains(value)),
            "{} outside saved range",
            spec.label
        );
    }
    if values[0] == 0.0 {
        return Ok(None);
    }
    if !section_enabled {
        omissions.push(format!(
            "{} {} not imported: its Lumetri section is off, so the vignette imports as none",
            LUMETRI_VIGNETTE_PARAMS[0].label, values[0]
        ));
        return Ok(None);
    }
    Ok(Some(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::LumetriVignette(values),
        animations: Vec::new(),
    }))
}

/// Report each unmapped section whose saved members differ from the pinned
/// defaults, naming the differing `ParameterID`s.
fn diagnose_unmapped_sections(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    omissions: &mut Vec<String>,
) {
    for section in &UNMAPPED_SECTIONS {
        let changed: Vec<String> = section
            .members
            .iter()
            .filter(|id| {
                controls
                    .get(id.to_string().as_str())
                    .is_some_and(|record| differs_from_default(graph, *record, **id))
            })
            .map(usize::to_string)
            .collect();
        if changed.is_empty() {
            continue;
        }
        let state = match section
            .enable
            .map(|enable| section_enabled(graph, controls, enable))
        {
            Some(Ok(false)) => " (section off)",
            _ => "",
        };
        omissions.push(format!(
            "Lumetri {} omitted{state}: saved ParameterID {} differ from Lumetri's defaults; {}",
            section.name,
            changed.join(", "),
            section.reason
        ));
    }
}

/// The bytes of the static arbitrary-data parameter `record`: its inline
/// base64 value, or the value that Premiere stored earlier in the project
/// under the same `BinaryHash` and leaves empty here.
fn arbitrary_bytes(graph: &Graph<'_>, record: Record<'_>) -> Result<Vec<u8>> {
    let saved = graph.decode::<ArbVideoComponentParam>(record)?.value;
    ensure!(
        saved.keyframes.as_deref().is_none_or(str::is_empty),
        "keyed Lumetri arbitrary data"
    );
    let encoded = saved
        .start_keyframe_value
        .ok_or_else(|| unsupported("missing Lumetri arbitrary-data value"))?;
    ensure!(
        encoded.encoding == records::ENCODING,
        "unsupported Lumetri arbitrary-data encoding"
    );
    let stored = if encoded.value.trim().is_empty() {
        let hash = encoded
            .binary_hash
            .as_deref()
            .ok_or_else(|| unsupported("missing Lumetri arbitrary-data BinaryHash"))?;
        graph
            .binary_value(hash, &record.identity())?
            .ok_or_else(|| {
                unsupported(format!(
                    "Lumetri arbitrary data names missing binary {hash}"
                ))
            })?
            .to_owned()
    } else {
        encoded.value
    };
    let compact: String = stored.split_whitespace().collect();
    STANDARD
        .decode(compact)
        .map_err(|_| unsupported("invalid Lumetri arbitrary-data base64"))
}

/// Whether the saved parameter `id` differs from its pinned default: a
/// keyed or undecodable record, a different static value, or binary data
/// with another content hash and value. A hue curve without points is the
/// identity whatever its saved kind (Premiere 22/23 saves Hue vs Sat as kind
/// 1, Premiere 26.5.2 as kind 3). An unknown `id` differs.
fn differs_from_default(graph: &Graph<'_>, record: Record<'_>, id: usize) -> bool {
    if HUE_CURVE_IDS.contains(&id) {
        if let Some(curve) = arbitrary_bytes(graph, record)
            .ok()
            .and_then(|bytes| LumetriCurve::decode(&bytes))
        {
            return !curve.points.is_empty();
        }
    }
    match lumetri_default(id) {
        None => true,
        Some(LumetriDefault::Static(start)) => {
            let Ok(saved) = graph.decode::<VideoComponentParam>(record) else {
                return true;
            };
            let saved = saved.value;
            if saved
                .keyframes
                .as_deref()
                .is_some_and(|keys| !keys.is_empty())
                || saved.is_time_varying.as_deref() == Some("true")
            {
                return true;
            }
            match (
                saved.start_keyframe.split(',').nth(1),
                start.split(',').nth(1),
            ) {
                (Some(saved), Some(default)) => {
                    match (saved.parse::<f64>(), default.parse::<f64>()) {
                        (Ok(saved), Ok(default)) => saved != default,
                        _ => saved != default,
                    }
                }
                _ => true,
            }
        }
        Some(LumetriDefault::Binary { hash, value }) => {
            let Ok(saved) = graph.decode::<ArbVideoComponentParam>(record) else {
                return true;
            };
            let saved = saved.value;
            if saved
                .keyframes
                .as_deref()
                .is_some_and(|keys| !keys.is_empty())
            {
                return true;
            }
            saved.start_keyframe_value.as_ref().is_none_or(|saved| {
                saved.binary_hash.as_deref() != Some(hash)
                    && (value.is_empty() || saved.value.trim() != value)
            })
        }
    }
}
