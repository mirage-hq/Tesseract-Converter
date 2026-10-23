//! Premiere sequence Color Setup ↔ FX document `colorManagement`.
//!
//! A sequence whose working colour space is ACEScct ([`SequenceAces`]) imports
//! as the FX ACES policy `acescct` / `aces2.0` / `footageOnly`, and an FX ACES
//! document exports as such a sequence. The two pipelines agree on the
//! grading domain (ACEScct) but not on the output: Premiere renders ACEScct to
//! its Rec.709 output through its own tone mapping (Auto Tone Mapping, Auto
//! Input Gamut Compression), FX through the ACES 2.0 output transform. That
//! difference is reported with every conversion; its measured size is in the
//! Premiere support ledger. A clip's camera-log Color Space Override is the
//! native equal of an FX `cameraInput` Input Transform for the cameras FX's
//! ACES pipeline converts ([`fx_conv::aces`]).

use crate::{
    approximate, omit,
    schema::{legacy_camera_override_reason, PrCameraOverride, SequenceAces},
    Omission, OmissionScope,
};
use fx_schema::{
    ColorEncodingId, ColorManagement, Compositing, InputColorTransform, InputTransform,
    OutputTransform, PersistedInputTransform,
};

/// The FX policy of an imported ACEScct sequence.
pub(super) fn import_policy() -> ColorManagement {
    ColorManagement::new_aces(OutputTransform::Aces2_0, Compositing::FootageOnly)
}

/// What an ACES import of `sequence` (record `record`) approximates.
pub(super) fn report_import(aces: SequenceAces, record: &str, omissions: &mut Vec<Omission>) {
    approximate(
        omissions,
        record,
        "Premiere ACEScct working colour space imported as FX colorManagement acescct / aces2.0 / footageOnly: grading runs on ACEScct in both, but Premiere renders ACEScct to its Rec.709 output through its own tone mapping and input gamut compression, FX through the ACES 2.0 output transform, a documented approximation: on an in-house chart and S-Log3 clip Premiere's SDR output differs from the ACES 2.0 SDR output by 23.5 and 24.6 8-bit codes (mean absolute difference; Premiere keeps SDR input display-preserving, ACES 2.0 renders it darker), and Auto Tone Mapping / Auto Input Gamut Compression do not change it; graphics and text stay display-referred under footageOnly",
    );
    if let Some(transfer) = aces.hdr_output {
        let flag = match transfer {
            fx_conv::HdrTransfer::Pq => "pq",
            fx_conv::HdrTransfer::Hlg => "hlg",
        };
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("hint: the ACEScct sequence output is Rec.2100 {transfer:?}; FX documents carry no HDR delivery target, so use `tsrct export --hdr {flag}` to render the ACES 2.0 HDR output"),
        );
    }
}

/// The `source.inputTransform` of a layer showing a video with the
/// camera-log Color Space Override `camera`, or the report of why the
/// document (`aces`) cannot hold it. Reports are deduplicated per media.
pub(super) fn camera_override_input_transform(
    camera: &PrCameraOverride,
    aces: bool,
    omissions: &mut Vec<Omission>,
) -> Result<Option<PersistedInputTransform>, String> {
    let reason = if !aces {
        legacy_camera_override_reason(&camera.label, camera.encoding)
    } else if fx_conv::aces::is_aces_camera_input(camera.encoding) {
        return Ok(Some(camera_input(camera.encoding)?));
    } else {
        format!(
            "Interpret Footage Color Space Override {:?} not converted: FX ACES has no reference input transform for {} (cameraInput supports only the cameras of the ACES reference config), so the clip plays uninterpreted; a vendor lut3d Input Transform on a renderDomain display layer is the FX equivalent; original log bytes and placement retained",
            camera.label, camera.encoding
        )
    };
    let reported = omissions
        .iter()
        .any(|omission| omission.record == camera.media_record && omission.reason == reason);
    if !reported {
        omit(
            omissions,
            OmissionScope::Feature,
            &camera.media_record,
            reason,
        );
    }
    Ok(None)
}

/// A LUT-less ACES camera Input Transform.
fn camera_input(encoding: &str) -> Result<PersistedInputTransform, String> {
    Ok(PersistedInputTransform::supported(InputTransform::v1(
        InputColorTransform::CameraInput {
            input_encoding: ColorEncodingId::new(encoding).map_err(|error| error.to_string())?,
        },
    )))
}

/// The ACEScct Color Setup of the Premiere sequence exported from `document`,
/// reporting what it approximates; `None` for a legacy document. A Rec.2100
/// export target (`hdr`) becomes the sequence output colour space.
pub(super) fn export_policy(
    document: &fx_schema::EditableFxCompositionDocument,
    hdr: Option<fx_conv::HdrTransfer>,
    omissions: &mut dyn crate::export_loss::OmissionSink,
) -> Option<SequenceAces> {
    let policy = document.color_management().aces()?;
    approximate(
        omissions,
        "document colorManagement",
        "FX ACES document exported as a Premiere sequence with the ACEScct working colour space: grading runs on ACEScct in both, but Premiere renders ACEScct to its output through its own tone mapping and input gamut compression, not the ACES 2.0 output transform, a documented approximation: on an in-house chart and S-Log3 clip Premiere's SDR output differs from the ACES 2.0 SDR output by 23.5 and 24.6 8-bit codes (mean absolute difference); camera inputs are written as clip Color Space Overrides",
    );
    if policy.compositing != Compositing::FootageOnly {
        approximate(
            omissions,
            "document colorManagement",
            format!("FX ACES compositing {:?} not exported: Premiere has no per-layer colour domain, so its ACEScct sequence converts every layer the same way", policy.compositing),
        );
    }
    if hdr.is_some() {
        approximate(
            omissions,
            "document colorManagement",
            "ACEScct sequence with a Rec.2100 output colour space: Premiere's HDR output tone mapping replaces the ACES 2.0 1000-nit P3-D65 output transform; the route has no native fixture",
        );
    }
    Some(SequenceAces { hdr_output: hdr })
}
