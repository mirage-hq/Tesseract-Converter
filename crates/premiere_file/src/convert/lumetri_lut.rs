//! Lumetri custom LUTs to FX: a Creative Look is a `lookTransform` at
//! `mix = Intensity/100`; a Basic Correction Input LUT is a full-mix
//! `lookTransform` at its stack position, promoted to the layer's source
//! `inputTransform` when nothing applies before it
//! ([`promote_input_lut`]). Both LUTs read and write Rec. 709 display
//! values, which is what Lumetri applies them to in a Rec. 709 sequence: the
//! native render of `color_lumetri_look_cube` equals the chart with the
//! `.cube` applied twice to its 8-bit code values (MAD 0.95/255).
//!
//! Export writes a `lookTransform` whose LUT reads and writes Rec. 709
//! display values as a Lumetri Color with only its Creative Look set
//! ([`export_look`]).
use super::effects::EffectIdAllocator;
use super::input_transform::{encoding, lut3d_input_transform};
use crate::{
    approximate,
    export_loss::OmissionSink,
    lut_assets::lut_asset,
    schema::{LumetriLutSlot, LutFile, PrEffect, PrEffectParams, PrLumetriLook, PrLumetriLut},
    Omission,
};
use fx_schema::{
    ColorTransform, ColorTransformSemanticVersion, EffectData, EffectPayload, EffectRecord,
    LayerEffect, NormalizedColorMix, PersistedInputTransform, SDR_REC709_DISPLAY_ENCODING_ID,
};

fn look_transform(
    lut: &PrLumetriLut,
    slot: LumetriLutSlot,
    mix: f64,
) -> Result<LayerEffect, String> {
    let asset_id = lut_asset(lut)
        .map_err(|reason| format!("the Premiere control {}: {reason}", slot.control()))?
        .clone();
    Ok(LayerEffect::LookTransform {
        semantic_version: ColorTransformSemanticVersion::V1,
        transform: Some(ColorTransform::lut3d(
            asset_id,
            encoding(SDR_REC709_DISPLAY_ENCODING_ID)?,
            encoding(SDR_REC709_DISPLAY_ENCODING_ID)?,
        )),
        mix: NormalizedColorMix::new(mix).map_err(|error| error.to_string())?,
    })
}

/// A Creative Look; an Intensity above 100 is clamped to a full mix by the
/// caller's diagnostic ([`look_clamp_note`]).
pub(super) fn look_effect(look: &PrLumetriLook) -> Result<LayerEffect, String> {
    look_transform(
        &look.lut,
        LumetriLutSlot::Look,
        (look.intensity / 100.0).clamp(0.0, 1.0),
    )
}

/// Why a Look Intensity does not map exactly, if it does not.
pub(super) fn look_clamp_note(look: &PrLumetriLook) -> Option<String> {
    (look.intensity > 100.0).then(|| {
        format!(
            "Lumetri Creative Look Intensity {} exceeds 100: FX lookTransform mix is at most 1, so it was clamped to 1 (Lumetri extrapolates the LUT beyond a full application)",
            look.intensity
        )
    })
}

/// The interpolation approximation of an imported Lumetri custom LUT.
pub(super) fn interpolation_note() -> String {
    format!(
        "Lumetri Color custom LUT: {}",
        crate::cube_lut::PREMIERE_LUT_INTERPOLATION
    )
}

/// A Basic Correction Input LUT at its stack position.
pub(super) fn input_lut_effect(lut: &PrLumetriLut) -> Result<LayerEffect, String> {
    look_transform(lut, LumetriLutSlot::Input, 1.0)
}

/// Move a Lumetri Input LUT that is the first effect of a media layer, enabled
/// and without compositing options, into the layer's source `inputTransform`,
/// which applies before every effect (an Input LUT has no keys). Any other
/// Input LUT stays a full-mix `lookTransform` at its position, which applies
/// the same LUT to the same values; it is reported.
pub(super) fn promote_input_lut(
    effects: &mut Vec<EffectRecord>,
    input_transform: &mut Option<PersistedInputTransform>,
    ids: &EffectIdAllocator,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<(), String> {
    let marked = |effect: &EffectRecord| matches!(effect.data(), EffectData::Identified { id, .. } if ids.is_input_lut(*id));
    if let Some(first) = effects.first() {
        if marked(first) && input_transform.is_none() {
            if let EffectData::Identified {
                enabled: true,
                compositing_options: None,
                effect:
                    EffectPayload::Known(LayerEffect::LookTransform {
                        transform: Some(ColorTransform::Lut3d { asset_id, .. }),
                        ..
                    }),
                ..
            } = first.data()
            {
                *input_transform = Some(lut3d_input_transform(
                    asset_id.clone(),
                    Some(SDR_REC709_DISPLAY_ENCODING_ID),
                    record,
                    omissions,
                )?);
                effects.remove(0);
            }
        }
    }
    if effects.iter().any(marked) {
        approximate(
            omissions,
            record,
            "Lumetri Basic Correction Input LUT kept as a full-mix lookTransform at its stack position, not as the source inputTransform: it is bypassed, another effect applies before it, or the source already has an Input Transform",
        );
    }
    Ok(())
}

/// The Lumetri Color of an FX `lookTransform`: Creative > Look browsed to
/// the transform's `.cube` (bound to its package file by
/// [`crate::lut_assets::bind_luts`]) at Intensity `mix * 100`, every other
/// control at its default. Only a LUT from Rec. 709 display values to Rec.
/// 709 display values has this form; Premiere's tetrahedral LUT
/// interpolation ([`crate::cube_lut::PREMIERE_LUT_INTERPOLATION`]) is
/// reported as a retained approximation.
pub(super) fn export_look(
    enabled: bool,
    effect: &LayerEffect,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrEffect, String> {
    let LayerEffect::LookTransform { transform, mix, .. } = effect else {
        return Err("only a lookTransform exports as a Lumetri Creative Look".to_owned());
    };
    let Some(ColorTransform::Lut3d {
        asset_id,
        input_encoding,
        output_encoding,
        ..
    }) = transform
    else {
        return Err(
            "a lookTransform without a lut3d transform has no Lumetri Creative Look form"
                .to_owned(),
        );
    };
    let (input, output) = (input_encoding.to_string(), output_encoding.to_string());
    if input != SDR_REC709_DISPLAY_ENCODING_ID || output != SDR_REC709_DISPLAY_ENCODING_ID {
        return Err(format!(
            "Lumetri Color > Creative > Look applies its .cube to Rec. 709 display values, so a lut3d {input} -> {output} has no native form"
        ));
    }
    super::effects::retained_lumetri_approximation(
        omissions,
        record,
        format!(
            "effects: lookTransform exports as a native Lumetri Color > Creative > Look (Intensity = mix*100) with its .cube written beside the media; {}",
            crate::cube_lut::PREMIERE_LUT_INTERPOLATION
        ),
    );
    Ok(PrEffect {
        enabled,
        mask: None,
        params: PrEffectParams::LumetriLook(PrLumetriLook {
            lut: PrLumetriLut {
                native_path: String::new(),
                file: LutFile::Packaged(asset_id.clone()),
                embedded: None,
            },
            // Rounded as the grade's saved values are, so mix 0.3 saves 30.
            intensity: (mix.get() * 100.0 * 1e8).round() / 1e8,
        }),
        animations: Vec::new(),
    })
}
