//! Interpret Footage colour fields of one native video stream: the per-clip
//! Color Space Override and Input LUT.
//!
//! Committed Premiere projects save these on `VideoStream`: Version 19 writes
//! `SelectedColorSpace` / `OverriddenColorSpace` (`{}` when unset),
//! `InputLUTSpecified` and `InputLUTID` (a nil GUID when unset); Version 21
//! writes only `InputLUTSpecified`; Premiere 26.x Version 22/23 streams omit
//! all of them at their defaults. Premiere 26.5.2 saves an active override as
//! `OverriddenColorSpace` `{"baseColorProfile":{"colorProfileName":<label>},
//! "baseProfileType":1}` with `IsColorSpacOverridden` `true`
//! (`color_clip_colorspace_override.prproj`), and a browsed Input LUT as an
//! `InputLUTAsset` edge to a `LUTAsset` record that embeds the samples
//! (`color_clip_input_lut.prproj`, see [`crate::cube_lut`]).
//!
//! An Input LUT with embedded samples is returned for the layer's
//! `source.inputTransform` `lut3d`; its input encoding is the camera encoding
//! that a Color Space Override names, and the override is then not reported.
//! A video's override that names a registered camera encoding without a LUT
//! is returned as a [`PrCameraOverride`]: an ACES document imports it as a
//! `cameraInput`, and a legacy document, which holds an input colour space
//! only through a LUT, reports it ([`legacy_camera_override_reason`]). Other
//! overrides are reported here; the original picture bytes and placement stay. A Rec.709 override is
//! silent only on an SDR source; on an HLG/PQ source it changes Premiere's
//! picture and is reported. An Input LUT without decodable samples (older
//! saves name only `InputLUTSpecified` / `InputLUTID`) is reported.

use crate::{
    cube_lut::Lut3d,
    error::Result,
    format::{Graph, Located},
    omit,
    schema::{
        legacy_camera_override_reason,
        native::{LutAsset, VideoStream},
        records, ClipColorSpaceOverride, ColorSpace, PrCameraOverride, PrClipInputLut,
    },
    Omission, OmissionScope,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use fx_schema::SDR_REC709_DISPLAY_ENCODING_ID;

const NIL_GUID: &str = "00000000-0000-0000-0000-000000000000";

/// The media's Interpret Footage Input LUT and, for a video (`video`), its
/// camera-log Color Space Override without a LUT, reporting the saved colour
/// state of `stream` (of the media record `record`) that FX cannot hold.
/// Bytes and placement stay.
pub(super) fn read(
    graph: &Graph<'_>,
    stream: &Located<VideoStream>,
    record: &str,
    video: bool,
    omissions: &mut Vec<Omission>,
) -> Result<(Option<PrClipInputLut>, Option<PrCameraOverride>)> {
    let embedded = match &stream.value.input_lut_asset {
        Some(reference) => {
            let asset = graph.follow::<LutAsset>(reference, &stream.identity)?;
            Some(embedded_lut(graph, &asset)?)
        }
        None => None,
    };
    let stream = &stream.value;
    let source_hdr = stream
        .original_color_space
        .as_deref()
        .and_then(|encoded| serde_json::from_str::<ColorSpace>(encoded).ok())
        .and_then(|space| space.source_hdr_transfer());
    // An explicit `false` keeps a saved label inactive.
    let override_space = stream
        .overridden_color_space
        .as_deref()
        .filter(|_| stream.is_color_space_overridden.as_deref() != Some("false"))
        .map(|encoded| ClipColorSpaceOverride::parse(encoded).map_err(|error| (encoded, error)));
    let camera_encoding = match &override_space {
        Some(Ok(Some(ClipColorSpaceOverride::Other {
            camera_encoding, ..
        }))) => *camera_encoding,
        _ => None,
    };
    let lut = match embedded {
        Some(Ok(mut lut)) => {
            lut.input_encoding = match &override_space {
                Some(Ok(Some(ClipColorSpaceOverride::Rec709))) if source_hdr.is_none() => {
                    Some(SDR_REC709_DISPLAY_ENCODING_ID)
                }
                _ => camera_encoding,
            };
            Some(lut)
        }
        Some(Err(reason)) => {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("Interpret Footage Input LUT not converted: {reason}; original bytes and placement retained without the LUT"),
            );
            None
        }
        None => None,
    };
    let mut camera = None;
    match override_space {
        // The override names the encoding that the converted LUT reads.
        Some(Ok(Some(ClipColorSpaceOverride::Other {
            camera_encoding: Some(_),
            ..
        }))) if lut.is_some() => {}
        None | Some(Ok(None)) => {}
        Some(Ok(Some(ClipColorSpaceOverride::Rec709))) => {
            if let Some(transfer) = source_hdr {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!("Interpret Footage Color Space Override BT.709 on a BT.2100 {transfer:?} source not converted: Premiere interprets the clip's HDR bytes as Rec.709 display-referred, while legacy FX has no input colour-space field and decodes the original bytes with their own saved BT.2100 {transfer:?} colour, so the picture differs; original bytes and placement retained"),
                );
            }
        }
        Some(Ok(Some(ClipColorSpaceOverride::Rec2100(transfer)))) if source_hdr == Some(transfer) => {}
        Some(Ok(Some(ClipColorSpaceOverride::Rec2100(transfer)))) => omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("Interpret Footage Color Space Override BT.2100 {transfer:?} not converted: the source's own saved colour space is not the same BT.2100 profile, and legacy FX has no input colour-space field; original bytes play with their own declared colour"),
        ),
        Some(Ok(Some(ClipColorSpaceOverride::Other {
            name,
            camera_encoding: Some(encoding),
        }))) if video => {
            camera = Some(PrCameraOverride {
                label: name,
                encoding,
                media_record: record.to_owned(),
            });
        }
        Some(Ok(Some(ClipColorSpaceOverride::Other {
            name,
            camera_encoding: Some(encoding),
        }))) => omit(
            omissions,
            OmissionScope::Feature,
            record,
            legacy_camera_override_reason(&name, encoding),
        ),
        Some(Ok(Some(ClipColorSpaceOverride::Other { name, camera_encoding: None }))) => omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("Interpret Footage Color Space Override {name:?} not converted: no registered Tesseract camera encoding has the same curve and gamut, and legacy FX has no input colour-space field; original bytes and placement retained"),
        ),
        Some(Err((encoded, error))) => omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("unrecognized Interpret Footage OverriddenColorSpace {encoded:?} not converted ({error}); original bytes and placement retained"),
        ),
    }
    if let Some(selected) = stream
        .selected_color_space
        .as_deref()
        .filter(|selected| !is_empty_object(selected))
    {
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("Interpret Footage SelectedColorSpace {selected:?} not converted: its saved meaning is not pinned by a native fixture; original bytes and placement retained"),
        );
    }
    let lut_id = stream.input_lut_id.as_deref().filter(|id| *id != NIL_GUID);
    if stream.input_lut_asset.is_none()
        && (stream.input_lut_specified.as_deref() == Some("true") || lut_id.is_some())
    {
        let encoding = camera_encoding.map_or_else(
            || "jerboa:sdr-rec709-display:v1 (no camera override to name the LUT input)".to_owned(),
            str::to_owned,
        );
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("Interpret Footage Input LUT (InputLUTSpecified, InputLUTID {}) not converted: the native record names no LUTAsset with embedded samples, so no LUT asset can be packaged; the editable equivalent is source.inputTransform lut3d with inputEncoding {encoding}; original bytes and placement retained without the LUT", lut_id.unwrap_or("absent")),
        );
    }
    Ok((lut, camera))
}

/// The LUT that `asset` embeds, or why it cannot be used. A malformed record
/// is reported by the caller; the picture keeps converting without it.
fn embedded_lut(
    graph: &Graph<'_>,
    asset: &Located<LutAsset>,
) -> Result<std::result::Result<PrClipInputLut, String>> {
    let identity = &asset.identity;
    let value = &asset.value;
    let Some(blob) = &value.blob else {
        return Ok(Err(format!(
            "{identity} has no embedded LUT samples (Blob)"
        )));
    };
    if blob.encoding != records::ENCODING {
        return Ok(Err(format!(
            "{identity} Blob encoding {:?} is not base64",
            blob.encoding
        )));
    }
    let stored = graph.binary_value(&blob.binary_hash, identity)?;
    let text = if blob.value.trim().is_empty() {
        match stored {
            Some(stored) => stored,
            None => return Ok(Err(format!("{identity} Blob stores no samples"))),
        }
    } else {
        &blob.value
    };
    let encoded: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let table = match STANDARD.decode(encoded) {
        Ok(bytes) => match Lut3d::from_itx3(&bytes) {
            Ok(table) => table,
            Err(reason) => return Ok(Err(format!("{identity}: {reason}"))),
        },
        Err(error) => return Ok(Err(format!("{identity} Blob is not base64: {error}"))),
    };
    let (Some(id), Some(file_name)) = (&value.id, &value.file_name) else {
        return Ok(Err(format!("{identity} has no Id or Filename")));
    };
    Ok(Ok(PrClipInputLut {
        id: id.clone(),
        display_name: value
            .display_name
            .clone()
            .unwrap_or_else(|| file_name.clone()),
        file_name: file_name.clone(),
        input_encoding: None,
        table: std::sync::Arc::new(table),
    }))
}

fn is_empty_object(encoded: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(encoded)
        .ok()
        .and_then(|value| value.as_object().map(serde_json::Map::is_empty))
        .unwrap_or(false)
}
