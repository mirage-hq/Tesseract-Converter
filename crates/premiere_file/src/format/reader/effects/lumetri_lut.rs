//! Lumetri Color custom `.cube` LUTs: the Basic Correction Input LUT and the
//! Creative Look ([`crate::schema::PrLumetriLut`]).
//!
//! Premiere 26.5.2 saves a browsed LUT twice: its path in the control's path
//! slot, and the path in the shader graph of the `Blob` (ParameterID 1),
//! whose `LUT` shaders name the Input LUT (before `BasicCorrection3`) and the
//! Look (beside `LookColorSpace`, the Look group ending with
//! `__Intensity`). The path slot decides. Premiere 26.5.2 renders both LUTs
//! of the UI-authored fixture unchanged when its `Blob` is replaced by the
//! constant `FE FE` sentinel and its embedded samples (ParameterID 98) are
//! cleared, as long as the slot keeps its NUL terminator and the file
//! exists; without the terminator, or without the file, it renders no LUT.
//! A `Blob` shader graph that names another file in the same slot is a
//! conflict whose rendered LUT is unverified; that slot is not imported.
//! With the native `Blob` graph, which references the embedded copy by
//! `<embeddedlut>`, Premiere renders both LUTs although the saved paths name
//! no existing file (the committed fixture's sanitized paths), so that copy
//! is kept for import to package when the file is missing
//! ([`crate::schema::EmbeddedLut`]).
use super::*;
use crate::schema::{
    lumetri_default, lumetri_param, native::ArbVideoComponentParam, LumetriDefault, LumetriLutSlot,
    LutFile, PrLumetriLook, PrLumetriLut, LUMETRI_BASIC_ENABLE, LUMETRI_CREATIVE_ENABLE,
    LUMETRI_CREATIVE_SATURATION, LUMETRI_INPUT_LUT_POPUP, LUMETRI_LOOK_COLOR_SPACE,
    LUMETRI_LOOK_COLOR_SPACE_REC709, LUMETRI_LOOK_POPUP, LUMETRI_VIBRANCE,
};
use aftereffects_file::lumetri::luts::{self, BlobLuts};

/// The custom LUTs of one Lumetri Color: its Input LUT effect, which applies
/// before the grade, and its Look effect, which applies after it.
#[derive(Default)]
pub(super) struct LumetriLuts {
    pub(super) input: Option<PrEffect>,
    pub(super) look: Option<PrEffect>,
}

/// Read both custom-LUT slots of one Lumetri Color. A slot that cannot be
/// imported is diagnosed in `omissions` naming its Premiere control; the
/// other slot and the rest of the Lumetri are kept.
pub(super) fn read(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enabled: bool,
    approximations: &mut Vec<String>,
    omissions: &mut Vec<String>,
) -> LumetriLuts {
    let blob = blob_luts(graph, controls);
    let embedded = embedded_entries(graph, controls);
    let mut luts = LumetriLuts::default();
    let input = read_slot(
        graph,
        controls,
        LumetriLutSlot::Input,
        blob.as_ref(),
        &embedded,
    );
    match input {
        Ok(Some(lut)) => {
            luts.input = Some(effect(enabled, PrEffectParams::LumetriInputLut(lut)));
        }
        Ok(None) => {}
        Err(error) => omissions.push(slot_omission(LumetriLutSlot::Input, error)),
    }
    match read_slot(
        graph,
        controls,
        LumetriLutSlot::Look,
        blob.as_ref(),
        &embedded,
    )
    .and_then(|lut| {
        lut.map(|lut| read_look(graph, controls, lut, blob.as_ref(), approximations))
            .transpose()
    }) {
        Ok(Some(look)) => {
            // Lumetri applies Creative Vibrance and Saturation after its
            // Look; the grade that carries them applies before the
            // lookTransform.
            let vibrance = static_value(graph, controls, LUMETRI_VIBRANCE.id).unwrap_or(0.0);
            let saturation =
                static_value(graph, controls, LUMETRI_CREATIVE_SATURATION.id).unwrap_or(100.0);
            if vibrance != 0.0 || saturation != 100.0 {
                approximations.push(format!(
                    "Lumetri Creative Vibrance {vibrance} and Saturation {saturation} apply after the Look in Lumetri, but the primaryGrade that carries them applies before the lookTransform"
                ));
            }
            luts.look = Some(effect(enabled, PrEffectParams::LumetriLook(look)));
        }
        Ok(None) => {}
        Err(error) => omissions.push(slot_omission(LumetriLutSlot::Look, error)),
    }
    if luts.input.is_none()
        && luts.look.is_none()
        && controls
            .get(luts::EMBEDDED_LUTS.to_string().as_str())
            .is_some_and(|record| binary_differs(graph, *record, luts::EMBEDDED_LUTS))
    {
        omissions.push("Lumetri embedded LUT samples omitted: saved ParameterID 98 holds LUT samples that no imported Input LUT or Look uses".to_owned());
    }
    luts
}

fn effect(enabled: bool, params: PrEffectParams) -> PrEffect {
    PrEffect {
        enabled,
        mask: None,
        params,
        animations: Vec::new(),
    }
}

fn slot_omission(slot: LumetriLutSlot, error: BuildError) -> String {
    let fx = match slot {
        LumetriLutSlot::Input => "no source inputTransform was set",
        LumetriLutSlot::Look => "no lookTransform was created",
    };
    format!(
        "Lumetri custom LUT omitted: the Premiere control {} {}; {fx}; other sections retained",
        slot.control(),
        reason(error)
    )
}

/// The `.cube` path of one slot, `None` when the slot holds no browsed file.
fn read_slot(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    slot: LumetriLutSlot,
    blob: Option<&BlobLuts>,
    embedded: &[([u8; 16], Vec<u8>)],
) -> Result<Option<PrLumetriLut>> {
    let (path_id, popup_id, enable) = match slot {
        LumetriLutSlot::Input => (
            luts::INPUT_LUT_PATH,
            LUMETRI_INPUT_LUT_POPUP,
            &LUMETRI_BASIC_ENABLE,
        ),
        LumetriLutSlot::Look => (
            luts::LOOK_PATH,
            LUMETRI_LOOK_POPUP,
            &LUMETRI_CREATIVE_ENABLE,
        ),
    };
    let Some(native_path) = path_slot(graph, controls, path_id)? else {
        return Ok(None);
    };
    ensure!(
        static_value(graph, controls, popup_id)? == 0.0,
        "selects a built-in LUT (popup ParameterID {popup_id} is not 0) beside the browsed file {native_path:?}, so which LUT applies is unverified"
    );
    if let Some(blob) = blob {
        let blob_path = match slot {
            LumetriLutSlot::Input => blob.input.as_deref(),
            LumetriLutSlot::Look => blob.look.as_ref().map(|(path, _)| path.as_str()),
        };
        ensure!(
            blob_path == Some(native_path.as_str()),
            "names {native_path:?} in its path slot (ParameterID {path_id}) but the Lumetri Blob shader graph applies {blob_path:?} there, so which LUT Premiere renders is unverified"
        );
    }
    ensure!(
        section_on(graph, controls, enable)?,
        "is in a section that is off"
    );
    // Premiere renders a missing file from its embedded copy only when the
    // Blob graph references that copy (a sentinel Blob renders no LUT).
    let embedded = blob.and_then(|blob| luts::embedded_copy(blob, embedded, &native_path));
    Ok(Some(PrLumetriLut {
        native_path,
        file: LutFile::Unresolved,
        embedded,
    }))
}

/// The Look's Intensity and colour space; a Look Color Space other than the
/// value a Rec. 709 sequence saves is imported as Rec. 709 with a note.
fn read_look(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    lut: PrLumetriLut,
    blob: Option<&BlobLuts>,
    approximations: &mut Vec<String>,
) -> Result<PrLumetriLook> {
    let intensity = static_value(graph, controls, luts::LOOK_INTENSITY)?;
    ensure!(
        (0.0..=200.0).contains(&intensity),
        "Intensity {intensity} is outside 0 to 200"
    );
    if let Some(saved) = blob.and_then(|blob| blob.look.as_ref().and_then(|(_, value)| *value)) {
        ensure!(
            (saved - intensity).abs().total_cmp(&1e-6).is_lt(),
            "Intensity {intensity} differs from the Blob's __Intensity {saved}"
        );
    }
    let space = static_value(graph, controls, LUMETRI_LOOK_COLOR_SPACE)?;
    if space != LUMETRI_LOOK_COLOR_SPACE_REC709 {
        approximations.push(format!(
            "Lumetri Creative Look Color Space popup {space} is not the Rec. 709 value {LUMETRI_LOOK_COLOR_SPACE_REC709}: the LUT is imported as a jerboa:sdr-rec709-display:v1 lookTransform, which applies it to Rec. 709 display values"
        ));
    }
    Ok(PrLumetriLook { lut, intensity })
}

/// The UTF-16LE path of a binary path slot, `None` for the empty `FE FE`
/// default. Keys are unsupported.
fn path_slot(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    id: usize,
) -> Result<Option<String>> {
    let record = controls
        .get(id.to_string().as_str())
        .copied()
        .ok_or_else(|| unsupported(format!("misses its path slot ParameterID {id}")))?;
    let bytes = binary_value(graph, record)?;
    luts::decode_path_slot(&bytes, id).map_err(unsupported)
}

/// The static bytes of a binary parameter: inline base64, or the value of an
/// earlier parameter saved under the same `BinaryHash`.
fn binary_value(graph: &Graph<'_>, record: Record<'_>) -> Result<Vec<u8>> {
    let param = graph.decode::<ArbVideoComponentParam>(record)?.value;
    ensure!(
        param.keyframes.as_deref().is_none_or(str::is_empty)
            && param
                .is_time_varying
                .as_deref()
                .is_none_or(|flag| flag == "false"),
        "has a keyed binary ParameterID {}",
        param.parameter_id
    );
    let Some(value) = param.start_keyframe_value else {
        return Ok(Vec::new());
    };
    let text = if value.value.trim().is_empty() {
        match value.binary_hash.as_deref() {
            Some(hash) => graph
                .binary_value(hash, &record.identity())?
                .unwrap_or_default(),
            None => "",
        }
    } else {
        value.value.as_str()
    };
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(compact).map_err(|_| {
        unsupported(format!(
            "has an invalid base64 value in ParameterID {}",
            param.parameter_id
        ))
    })
}

/// Whether binary parameter `id` differs from its pinned default value.
fn binary_differs(graph: &Graph<'_>, record: Record<'_>, id: usize) -> bool {
    let Some(LumetriDefault::Binary { hash, .. }) = lumetri_default(id) else {
        return true;
    };
    match graph.decode::<ArbVideoComponentParam>(record) {
        Ok(param) => param
            .value
            .start_keyframe_value
            .is_none_or(|value| value.binary_hash.as_deref() != Some(hash)),
        Err(_) => true,
    }
}

/// The static numeric value of a scalar or popup parameter.
fn static_value(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    id: usize,
) -> Result<f64> {
    let spec = lumetri_param(id).ok_or_else(|| unsupported(format!("unknown ParameterID {id}")))?;
    let record = controls
        .get(id.to_string().as_str())
        .copied()
        .ok_or_else(|| unsupported(format!("misses {} (ParameterID {id})", spec.label)))?;
    let param = graph.decode::<VideoComponentParam>(record)?.value;
    ensure!(
        param.keyframes.as_deref().is_none_or(str::is_empty)
            && param
                .is_time_varying
                .as_deref()
                .is_none_or(|flag| flag == "false"),
        "has keyed {} (ParameterID {id}), which is not converted",
        spec.label
    );
    // `-91445760000000000,<value>,…`, or `<value>,…` for a popup saved
    // without its time.
    let fields: Vec<&str> = param.start_keyframe.split(',').collect();
    let value = match fields.as_slice() {
        [time, value, ..] if time.starts_with('-') && time.len() > 10 => value,
        [value, ..] => value,
        [] => "",
    };
    value
        .trim_end_matches('.')
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            unsupported(format!(
                "has an unreadable {} (ParameterID {id})",
                spec.label
            ))
        })
}

/// Whether a section's enable checkbox is on.
fn section_on(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
    enable: &EffectParamSpec,
) -> Result<bool> {
    let record = controls
        .get(enable.id.to_string().as_str())
        .copied()
        .ok_or_else(|| unsupported(format!("misses {}", enable.label)))?;
    let param = graph.decode::<VideoComponentParam>(record)?.value;
    match param.start_keyframe.split(',').nth(1) {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err(unsupported(format!("has an unreadable {}", enable.label))),
    }
}

fn blob_luts(graph: &Graph<'_>, controls: &BTreeMap<&str, Record<'_>>) -> Option<BlobLuts> {
    let record = controls.get("1").copied()?;
    let bytes = binary_value(graph, record).ok()?;
    luts::parse_blob_luts(std::str::from_utf8(&bytes).ok()?)
}

/// The entries of `Embedded LUTs` (ParameterID 98): 16-byte id and payload.
/// Empty when the parameter is absent, keyed, or not a whole entry list.
fn embedded_entries(
    graph: &Graph<'_>,
    controls: &BTreeMap<&str, Record<'_>>,
) -> Vec<([u8; 16], Vec<u8>)> {
    let Some(bytes) = controls
        .get(luts::EMBEDDED_LUTS.to_string().as_str())
        .and_then(|record| binary_value(graph, *record).ok())
    else {
        return Vec::new();
    };
    luts::embedded_entries(&bytes)
}
