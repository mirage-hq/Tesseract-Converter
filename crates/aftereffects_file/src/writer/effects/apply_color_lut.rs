//! Apply Color LUT (`ADBE Apply Color LUT2`) instance state.
//!
//! The human-authored AE 26.5 fixture `color_ae_apply_color_lut.aep` pins the
//! layout: the plugin's `sspc` carries the LUT as sequence data (`sdat`,
//! [`crate::effects::apply_color_lut::encode_sequence_data`]) between the
//! control group and `pgui`, and the `-0001` group declaration's `pard` label
//! names the file (`Using: <name>`). AE stores the same state in the
//! project-level definition (`EfDf`), so the definition copies the first
//! instance's LUT.

use super::{NativeEffect, NativeLut};
use crate::{
    effects::{apply_color_lut, definitions::ParameterDefinition},
    rifx::{Chunk, RifxError},
};

pub(super) use apply_color_lut::MATCH_NAME;

/// The embedded LUT of an Apply Color LUT instance. Export binds it on the
/// final layers (`export_document::apply_color_lut::bind`) and omits every
/// instance it cannot bind, so only the payload validation that runs during
/// lowering writes an unbound instance (no `sdat`, registry label).
fn bound(effect: &NativeEffect) -> Option<(&NativeLut, &[u8])> {
    effect
        .lut
        .as_ref()
        .and_then(|lut| Some((lut, lut.sequence_data.as_deref()?)))
}

/// Name the file in the instance's `-0001` declaration label.
pub(super) fn label_instance(effect: &NativeEffect, instance: &mut ParameterDefinition) {
    if effect.match_name == MATCH_NAME
        && instance.match_name == apply_color_lut::FILE_LABEL_CONTROL
        && let Some((lut, _)) = bound(effect)
    {
        instance.label = apply_color_lut::file_label(&lut.file_name);
    }
}

/// The instance's `sdat` record, for a bound Apply Color LUT only.
pub(super) fn sequence_data(effect: &NativeEffect) -> Result<Option<Chunk>, RifxError> {
    if effect.match_name != MATCH_NAME {
        return Ok(None);
    }
    bound(effect)
        .map(|(_, data)| Chunk::data(*b"sdat", data.to_vec()))
        .transpose()
}

/// The LUT of the first Apply Color LUT instance in `chunks`, for the
/// project-level definition.
pub(super) fn first_instance_lut(chunks: &[Chunk]) -> Option<NativeLut> {
    for pair in chunks.windows(2) {
        if pair[0].id() == *b"tdmn"
            && pair[0].data_payload().is_some_and(|name| {
                name.split(|byte| *byte == 0).next() == Some(MATCH_NAME.as_bytes())
            })
            && let Some(plugin) = pair[1].children()
            && pair[1].list_kind() == Some(*b"sspc")
            && let Some(data) = plugin
                .iter()
                .find(|chunk| chunk.id() == *b"sdat")
                .and_then(Chunk::data_payload)
            && let Ok(embedded) = apply_color_lut::decode_sequence_data(data)
        {
            return Some(NativeLut {
                asset_id: embedded.asset_id(),
                file_name: embedded.file_name,
                sequence_data: Some(data.to_vec()),
            });
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(first_instance_lut)
}
