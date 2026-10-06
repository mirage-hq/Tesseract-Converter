//! A deliberately selected saved control, not an interpretation of Lumetri's blobs.
use super::*;
use crate::schema::{
    LUMETRI_EXPOSURE, LUMETRI_SATURATION, LUMETRI_TEMPERATURE, LUMETRI_TINT,
    LUMETRI_VIGNETTE_AMOUNT, LUMETRI_VIGNETTE_FEATHER, LUMETRI_VIGNETTE_MIDPOINT,
};

pub(super) const MATCH_NAME: &str = "AE.ADBE Lumetri";
pub(super) const REPLACEMENT: &str = "Lumetri replacement selects saved Exposure as FX Exposure (offset 0, gamma 1), Contrast as Brightness & Contrast (Brightness 0), then Saturation as Hue/Saturation (saved percentage minus 100, other controls neutral), followed by separate Temperature/Tint replacements (Temperature divided by 3, Tint sign reversed and divided by 3 to match the existing shader rather than its description), then a static centered Vignette (Amount=-saved/5, radius=Midpoint/100, feather=Feather/100, radius/feather floored to0.001), with its own section enable. Roundness is replaced by a circular falloff; these are declared range-normalized controls, not matched native tone/shape algorithms, in that replacement order; this is not an equivalent Lumetri transfer, and scalar authority over private/blob state is unproved. All other Lumetri controls and keys (including Highlights, Shadows, Whites, Blacks, LUT and other sections) and opaque state are omitted; edited export writes the replacement, never Lumetri or its original blobs";

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

pub(super) fn read(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
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
    let mut ids = BTreeSet::new();
    for reference in &references.items {
        let record = graph.locate(reference, &native.identity)?;
        let id = record
            .element()
            .child("ParameterID")
            .and_then(Element::text)
            .ok_or_else(|| unsupported("missing Lumetri ParameterID"))?;
        // Shared identity ambiguity is not safe to salvage per control.
        ensure!(ids.insert(id), "duplicate Lumetri ParameterID {id}");
        if matches!(
            id,
            "7" | "8" | "3" | "11" | "12" | "20" | "50" | "51" | "52" | "54"
        ) {
            controls.insert(id, record);
        }
    }
    let mut effects = match read_basic(graph, &mut controls, enabled, omissions) {
        Ok(effects) => effects,
        Err(error) => {
            omissions.push(format!(
                "Lumetri Basic Correction replacements omitted: {}",
                reason(error)
            ));
            Vec::new()
        }
    };
    match read_vignette(graph, &mut controls, enabled) {
        Ok(effect) => effects.push(effect),
        Err(error) => omissions.push(format!(
            "Lumetri Vignette replacement omitted: {}; other sections retained",
            reason(error)
        )),
    }
    Ok(effects)
}

fn read_basic(
    graph: &Graph<'_>,
    controls: &mut BTreeMap<&str, Record<'_>>,
    enabled: bool,
    omissions: &mut Vec<String>,
) -> Result<Vec<PrEffect>> {
    let section = controls
        .remove("3")
        .ok_or_else(|| unsupported("missing Lumetri Basic Correction enable"))?;
    let param = selected_param(graph, section)?;
    // The pinned section-enable checkbox is named one literal space, not the
    // separate "Basic Correction" disclosure header (ParameterID 2).
    ensure!(
        param.name.as_deref() == Some(" "),
        "unexpected Lumetri Basic Correction enable name"
    );
    require_static(&param, "Basic Correction enable")?;
    let ParamValue::Static(value) = param_value(
        &param,
        section.element(),
        &GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS,
        &section.identity(),
        false,
        false,
    )?
    else {
        return Err(unsupported("keyed Lumetri Basic Correction enable"));
    };
    let section_enabled = match value.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err(unsupported("invalid Lumetri Basic Correction enable")),
    };
    let enabled = enabled && section_enabled;
    let mut effects = Vec::new();
    for (id, spec) in [
        ("11", &LUMETRI_EXPOSURE),
        ("12", &BRIGHTNESS_CONTRAST_CONTRAST),
        ("20", &LUMETRI_SATURATION),
        ("7", &LUMETRI_TEMPERATURE),
        ("8", &LUMETRI_TINT),
    ] {
        let replacement = controls
            .remove(id)
            .ok_or_else(|| unsupported(format!("missing Lumetri {}", spec.name)))
            .and_then(|record| read_control(graph, record, id, spec, enabled));
        match replacement {
            Ok(effect) => effects.push(effect),
            Err(error) => omissions.push(format!(
                "Lumetri {} replacement omitted: {}; other admitted replacements retained",
                spec.name,
                reason(error)
            )),
        }
    }
    Ok(effects)
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

fn read_control(
    graph: &Graph<'_>,
    record: Record<'_>,
    id: &str,
    spec: &'static EffectParamSpec,
    enabled: bool,
) -> Result<PrEffect> {
    let param = selected_param(graph, record)?;
    ensure!(
        param.name.as_deref() == Some(spec.name),
        "unexpected Lumetri {} name",
        spec.name
    );
    // Retain the reviewed static-Contrast boundary, without discarding
    // independent Exposure/Saturation when that boundary is exceeded.
    if id == "12" {
        require_static(&param, "Contrast")?;
    }
    let (value, animations) = match param_value(
        &param,
        record.element(),
        spec,
        &record.identity(),
        false,
        false,
    )? {
        ParamValue::Static(value) => (
            value
                .parse::<f64>()
                .map_err(|_| unsupported("invalid Lumetri numeric control"))?,
            vec![],
        ),
        ParamValue::Keyed(keys) => {
            let value = keys
                .scalar()
                .and_then(|keys| keys.first())
                .ok_or_else(|| unsupported("missing Lumetri scalar keys"))?
                .value;
            (value, vec![PrEffectParamAnimation { param: spec, keys }])
        }
    };
    ensure!(
        spec.value_range()
            .is_some_and(|range| range.contains(&value)),
        "Lumetri {} outside replacement range",
        spec.name
    );
    Ok(PrEffect {
        mask: None,
        enabled,
        params: match id {
            "7" => PrEffectParams::LumetriTemperature(value),
            "8" => PrEffectParams::LumetriTint(value),
            "11" => PrEffectParams::LumetriExposure(value),
            "12" => PrEffectParams::BrightnessContrast(PrBrightnessContrast {
                brightness: 0.0,
                contrast: value,
            }),
            _ => PrEffectParams::LumetriSaturation(value),
        },
        animations,
    })
}

fn read_vignette(
    graph: &Graph<'_>,
    controls: &mut BTreeMap<&str, Record<'_>>,
    enabled: bool,
) -> Result<PrEffect> {
    let section = controls
        .remove("50")
        .ok_or_else(|| unsupported("missing Vignette enable"))?;
    let param = selected_param(graph, section)?;
    ensure!(
        param.name.as_deref() == Some(" "),
        "unexpected Vignette enable name"
    );
    require_static(&param, "Vignette enable")?;
    let ParamValue::Static(value) = param_value(
        &param,
        section.element(),
        &GAUSSIAN_BLUR_REPEAT_EDGE_PIXELS,
        &section.identity(),
        false,
        false,
    )?
    else {
        return Err(unsupported("keyed Vignette enable"));
    };
    let section_enabled = match value.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err(unsupported("invalid Vignette enable")),
    };
    let mut values = [0.0; 3];
    for (value, (id, spec)) in values.iter_mut().zip([
        ("51", &LUMETRI_VIGNETTE_AMOUNT),
        ("52", &LUMETRI_VIGNETTE_MIDPOINT),
        ("54", &LUMETRI_VIGNETTE_FEATHER),
    ]) {
        let record = controls
            .remove(id)
            .ok_or_else(|| unsupported(format!("missing {}", spec.label)))?;
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
    Ok(PrEffect {
        mask: None,
        enabled: enabled && section_enabled,
        params: PrEffectParams::LumetriVignette(values),
        animations: Vec::new(),
    })
}
