//! A media's Interpret Footage Input LUT ([`crate::schema::PrClipInputLut`]) as the FX
//! layer's `source.inputTransform` `lut3d`: Premiere applies it to the
//! source picture before every clip effect, as the FX source transform does.
//! The embedded samples are packaged as a `.cube` asset per LUT GUID
//! ([`crate::schema::PrClipInputLut::asset_id`], written by `tesseract_output`).
use super::input_transform::lut3d_input_transform;
use crate::{approximate, schema::PrVideoStream, Omission};
use fx_schema::PersistedInputTransform;

/// The `source.inputTransform` of a layer showing `source`, if its media has
/// an Input LUT. Without a Color Space Override naming the LUT's input, the
/// LUT is declared to read Rec. 709 display values and that is reported.
/// Without a LUT, a camera-log Color Space Override is a `cameraInput` in an
/// ACES document (`aces`), see [`super::color_management`].
pub(super) fn input_transform(
    source: &PrVideoStream,
    record: &str,
    aces: bool,
    omissions: &mut Vec<Omission>,
) -> Result<Option<PersistedInputTransform>, String> {
    let Some(lut) = &source.input_lut else {
        return match &source.camera_override {
            Some(camera) => {
                super::color_management::camera_override_input_transform(camera, aces, omissions)
            }
            None => Ok(None),
        };
    };
    approximate(
        omissions,
        record,
        format!(
            "Interpret Footage Input LUT: {}",
            crate::cube_lut::PREMIERE_LUT_INTERPOLATION
        ),
    );
    lut3d_input_transform(lut.asset_id(), lut.input_encoding, record, omissions).map(Some)
}
