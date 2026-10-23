//! Commit a native Apply Color LUT instance as an FX `lookTransform` whose
//! `.cube` asset the import adapter packages from the same sequence data
//! (`crate::adapter::apply_color_lut`).

use fx_schema::{EffectData, EffectId, EffectPayload, EffectRecord};

use super::super::reserve_ids;
use super::ImportedEffects;
use crate::{
    effects::{apply_color_lut, native::DecodedEffect},
    structure::Layer,
};

/// The import note every converted instance carries.
pub(crate) const APPROXIMATION: &str = "the LUT embedded in the effect (AE stores the applied .cube as a 16-bit device-link CLUT, not the file or its path) is packaged as a .cube asset and applied as an FX lookTransform (trilinear, display-referred SDR, mix 1); a non-None working space is not reproduced";

pub(super) fn import(
    source: &DecodedEffect,
    layer: &Layer,
    next_id: &mut u64,
    result: &mut ImportedEffects,
) {
    let name = apply_color_lut::MATCH_NAME;
    let (effect, embedded) = match apply_color_lut::lower(&layer.content, source.index) {
        Ok(lowered) => lowered,
        Err(reason) => {
            result.warnings.push(format!(
                "Effect {name}: {reason}; no lookTransform was created; effect omitted, owner and other effects retained"
            ));
            return;
        }
    };
    let mut candidate_id = *next_id;
    let Some(raw_id) = reserve_ids(&mut candidate_id, 1) else {
        result.warnings.push(format!(
            "Effect {name}: generated layer identifier space exhausted; effect omitted"
        ));
        return;
    };
    let record = match EffectRecord::from_data(&EffectData::Identified {
        id: EffectId::new(raw_id),
        enabled: source.enabled && layer.record.flags().effects_active,
        effect: EffectPayload::Known(effect),
        compositing_options: None,
        extensions: Default::default(),
    }) {
        Ok(record) => record,
        Err(error) => {
            result.warnings.push(format!(
                "Effect {name}: lookTransform is outside current FX representation ({error}); effect omitted, owner retained"
            ));
            return;
        }
    };
    *next_id = candidate_id;
    result.native_ordinals.push(source.index);
    result.effects.push(record);
    result.warnings.push(format!(
        "Effect {name} ({:?}, {}³): {APPROXIMATION}; {}",
        embedded.file_name,
        embedded.lut.size,
        apply_color_lut::LUT_INTERPOLATION
    ));
}
