//! Sequence Color Setup with an ACEScct working colour space.
//!
//! Premiere 26.5.2 saves a sequence whose Color Setup working colour space is
//! ACEScct (`color_aces_pr_sequence.prproj`) with `WorkingColorSpace`
//! `ACEScct`, `WorkingColorSpaceConfiguration` id `ACEScct` (not linearized),
//! `AllowLinearCompositing` `false`, `AutoInputGamutCompressionEnabled` `true`,
//! the host-default `ColorManagementSettings` (auto tone mapping on) and no
//! `OutputColorSpace`, `ToneMappingAlgorithm` or `GamutCompressionAlgorithm`.
//! That working space is the native equal of an FX ACES document; the controls
//! FX cannot hold are reported here, and the document policy and its measured
//! output difference are reported by `convert::color_management`.

use super::super::Located;
use crate::{
    omit,
    schema::{native::VideoTrackGroup, ColorSpace, SequenceAces},
    Omission, OmissionScope,
};
use serde_json::Value;

/// The ACES Color Setup of a sequence whose working colour space is ACEScct;
/// `None` for every other working space (SDR and Rec.2100 stay with
/// `reader::hdr`).
pub(super) fn sequence(
    video: &Located<VideoTrackGroup>,
    omissions: &mut Vec<Omission>,
) -> Option<SequenceAces> {
    let fields = &video.value;
    let working: ColorSpace = serde_json::from_str(fields.working_color_space.as_deref()?).ok()?;
    if working != SequenceAces::working_color_space() {
        return None;
    }
    let mut report = |reason: String| {
        omit(
            omissions,
            OmissionScope::Feature,
            &video.identity,
            format!("{reason}; the ACEScct working space is still imported as an FX ACES document"),
        );
    };
    let configuration = fields
        .working_color_space_configuration
        .as_deref()
        .and_then(|encoded| serde_json::from_str::<Value>(encoded).ok());
    if configuration.as_ref().is_none_or(|configuration| {
        configuration["workingSpaceID"].as_str() != Some(SequenceAces::WORKING_NAME)
            || configuration["workingSpaceIsLinearized"].as_u64() != Some(0)
    }) {
        report(format!(
            "ACEScct WorkingColorSpaceConfiguration {:?} is not the pinned non-linearized ACEScct configuration",
            fields.working_color_space_configuration
        ));
    }
    if fields.allow_linear_compositing.as_deref() == Some("true") {
        report("Premiere linear compositing (AllowLinearCompositing) in the ACEScct working space not converted: FX footageOnly composites footage in linear ACEScg but graphics in display values".into());
    }
    if fields.auto_input_gamut_compression_enabled.as_deref() == Some("false") {
        report("Premiere Auto Input Gamut Compression off not converted: FX ACES has no input gamut-compression control".into());
    }
    if let Some(settings) = fields
        .color_management_settings
        .as_deref()
        .and_then(|encoded| serde_json::from_str::<Value>(encoded).ok())
    {
        if settings["autoToneMapEnabled"].as_bool() == Some(false) {
            report("Premiere Auto Tone Mapping off not converted: FX always renders ACES through the ACES 2.0 output transform".into());
        }
    }
    for (name, value) in [
        ("ToneMappingAlgorithm", &fields.tone_mapping_algorithm),
        (
            "GamutCompressionAlgorithm",
            &fields.gamut_compression_algorithm,
        ),
    ] {
        if let Some(value) = value {
            report(format!("Premiere {name} {value} not converted: FX renders ACES through the ACES 2.0 output transform only"));
        }
    }
    let output = fields
        .output_color_space
        .as_deref()
        .map(|encoded| serde_json::from_str::<ColorSpace>(encoded).map_err(|_| encoded));
    let hdr_output = match output {
        None => None,
        Some(Ok(output)) if output.is_sequence_sdr() => None,
        Some(Ok(output)) => {
            let transfer = output.output_hdr_transfer();
            if transfer.is_none() {
                report(format!(
                    "ACEScct sequence output colour space {:?} not converted: FX ACES output is the ACES 2.0 SDR Rec.709 (BT.1886) or, with `tsrct export --hdr`, HDR transform",
                    fields.output_color_space
                ));
            }
            transfer
        }
        Some(Err(encoded)) => {
            report(format!(
                "unrecognized ACEScct sequence OutputColorSpace {encoded:?} not converted"
            ));
            None
        }
    };
    Some(SequenceAces { hdr_output })
}
