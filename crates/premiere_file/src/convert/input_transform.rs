//! FX source `inputTransform` LUTs shared by the Premiere sources of one:
//! the Lumetri Basic Correction Input LUT (`lumetri_lut`) and, when its
//! saved grammar is pinned, the clip Interpret Footage Input LUT.
use crate::{approximate, Omission};
use fx_schema::{
    AssetId, ColorEncodingId, InputColorTransform, InputTransform, PersistedInputTransform,
    SDR_REC709_DISPLAY_ENCODING_ID,
};

/// A semantic-version-1 `lut3d` Input Transform from `input_encoding` into
/// Rec. 709 display values, trilinear. Without a known `input_encoding` the
/// LUT is declared to read Rec. 709 display values, reported on `record`.
pub(super) fn lut3d_input_transform(
    asset_id: AssetId,
    input_encoding: Option<&'static str>,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<PersistedInputTransform, String> {
    let input = input_encoding.unwrap_or_else(|| {
        approximate(
            omissions,
            record,
            format!("Input LUT source encoding is unknown: the LUT is declared to read {SDR_REC709_DISPLAY_ENCODING_ID} values"),
        );
        SDR_REC709_DISPLAY_ENCODING_ID
    });
    Ok(PersistedInputTransform::supported(InputTransform::v1(
        InputColorTransform::lut3d(
            asset_id,
            encoding(input)?,
            encoding(SDR_REC709_DISPLAY_ENCODING_ID)?,
        ),
    )))
}

/// A registered encoding identifier.
pub(super) fn encoding(id: &str) -> Result<ColorEncodingId, String> {
    ColorEncodingId::new(id).map_err(|error| error.to_string())
}
