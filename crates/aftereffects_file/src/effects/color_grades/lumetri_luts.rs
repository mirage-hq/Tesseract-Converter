//! Lumetri Color custom LUTs → FX `lookTransform`: a `.cube` browsed in the
//! Basic Correction Input LUT or the Creative Look.
//!
//! After Effects 26.5 saves a browsed LUT as Premiere 26.5.2 does
//! ([`crate::lumetri::luts`]): the path in `ADBE Lumetri-0004` / `-0024`, the
//! `Blob` (`-0001`) shader graph naming it, and an `ITX3` copy of the samples
//! in `Embedded LUTs` (`-0098`). The UI-authored fixture
//! `color_ae_lumetri_custom_lut.aep` pins this: its embedded copy is the
//! `.cube`'s values as `f32`, and AE 26.5 renders it identically when no file
//! exists at the saved path (a copy whose paths name no file). The popup
//! (`-0005` Input LUT, `-0025` Look) does not decide: AE saves it only in an
//! instance's own declaration table (2 for the fixture's Input LUT; its Look
//! instance has no table), reads back 2 when the file exists and 1 (None)
//! when it does not, and renders the LUT in both cases. The path slot and the
//! `Blob` decide; a saved popup that selects anything else (Browse, a
//! separator or a built-in LUT) is a conflict.
//!
//! Import packages the embedded copy, never the file: AE keeps rendering that
//! copy without the file. Both LUTs read and write display values (AE 26.5,
//! 8 bpc, working space None: both fixture comps render the chart through the
//! `.cube` once, within 0.65 mean 8-bit codes of trilinear interpolation in
//! display codes).

use fx_schema::{
    ColorEncodingId, ColorTransform, ColorTransformSemanticVersion, LayerEffect,
    NormalizedColorMix, SDR_REC709_DISPLAY_ENCODING_ID,
};

use super::{
    LUMETRI_BASIC_ENABLE, LUMETRI_CREATIVE_ENABLE, checkbox, initial, lumetri_control,
    lumetri_payload, numeric,
};
use crate::{
    effects::{
        apply_color_lut::{EmbeddedLut, Lut3d},
        native::DecodedEffect,
    },
    lumetri::luts,
};

/// The popup values that AE 26.5 saves or reads back beside a browsed file:
/// 1 (None, when the file is missing) and 2 (the file).
const BROWSED_POPUPS: [f64; 2] = [1.0, 2.0];
/// The Look Color Space value (`-0130`) that AE 26.5 saves in the fixture
/// (8 bpc, working space None).
const LOOK_COLOR_SPACE: &str = "ADBE Lumetri-0130";
const LOOK_COLOR_SPACE_FIXTURE: f64 = 29.0;

/// How the imported LUTs are applied, stated once per imported LUT.
pub(super) const LUMETRI_LUT_APPROXIMATION: &str = "Lumetri custom LUT imported as an FX lookTransform of the copy After Effects embeds (Embedded LUTs, the .cube's values as f32); a file changed after the save is not re-read. FX applies it with trilinear interpolation to display values; After Effects 26.5 (8 bpc, working space None) renders the fixture's per-channel LUT within 0.65 mean 8-bit codes of that, and its interpolation of LUTs that mix channels is unmeasured";

/// One of Lumetri's two custom-LUT slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Basic Correction > Input LUT, applied before the Basic Correction.
    Input,
    /// Creative > Look, applied after the Basic Correction at an Intensity.
    Look,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Self::Input => "Input LUT",
            Self::Look => "Creative Look",
        }
    }

    const fn path(self) -> usize {
        match self {
            Self::Input => luts::INPUT_LUT_PATH,
            Self::Look => luts::LOOK_PATH,
        }
    }

    const fn popup(self) -> &'static str {
        match self {
            Self::Input => "ADBE Lumetri-0005",
            Self::Look => "ADBE Lumetri-0025",
        }
    }

    const fn section(self) -> (&'static str, &'static str) {
        match self {
            Self::Input => (LUMETRI_BASIC_ENABLE, "Basic Correction"),
            Self::Look => (LUMETRI_CREATIVE_ENABLE, "Creative"),
        }
    }
}

/// An imported custom LUT: the samples to package and the FX mix.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BrowsedLut {
    pub lut: EmbeddedLut,
    pub mix: f64,
    /// Approximations of this LUT beyond [`LUMETRI_LUT_APPROXIMATION`].
    pub notes: Vec<String>,
}

impl BrowsedLut {
    /// The FX effect, its LUT packaged as [`EmbeddedLut::asset_id`].
    pub(crate) fn effect(&self) -> Result<LayerEffect, String> {
        let display = ColorEncodingId::new(SDR_REC709_DISPLAY_ENCODING_ID)
            .map_err(|error| error.to_string())?;
        Ok(LayerEffect::LookTransform {
            semantic_version: ColorTransformSemanticVersion::V1,
            transform: Some(ColorTransform::lut3d(
                self.lut.asset_id(),
                display.clone(),
                display,
            )),
            mix: NormalizedColorMix::new(self.mix).map_err(|error| error.to_string())?,
        })
    }
}

/// The Input LUT and the Look of one Lumetri instance: `None` when the slot
/// does not select a browsed file, else the LUT or why it is not imported.
pub(crate) fn browsed(source: &DecodedEffect) -> [Option<Result<BrowsedLut, String>>; 2] {
    [Slot::Input, Slot::Look].map(|slot| {
        read(source, slot).transpose().map(|result| {
            result.map_err(|reason| {
                format!(
                    "{} ({}) not imported: {reason}",
                    slot.name(),
                    lumetri_control(slot.path())
                )
            })
        })
    })
}

fn read(source: &DecodedEffect, slot: Slot) -> Result<Option<BrowsedLut>, String> {
    let control = lumetri_control(slot.path());
    let Some(bytes) = lumetri_payload(source, slot.path()).transpose()? else {
        return Ok(None);
    };
    let Some(path) = luts::decode_path_slot(bytes, slot.path())? else {
        return Ok(None);
    };
    let blob = lumetri_payload(source, 1)
        .and_then(Result::ok)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(luts::parse_blob_luts)
        .ok_or_else(|| {
            format!("the Lumetri Blob shader graph (ADBE Lumetri-0001) is missing or unreadable, so the embedded copy of {path:?} cannot be located (the file itself is not read)")
        })?;
    let applied = match slot {
        Slot::Input => blob.input.as_deref(),
        Slot::Look => blob.look.as_ref().map(|(path, _)| path.as_str()),
    };
    if applied != Some(path.as_str()) {
        return Err(format!(
            "{control} names {path:?} but the Lumetri Blob shader graph applies {applied:?} there, so which LUT After Effects renders is unverified"
        ));
    }
    // Only a popup that the instance declares itself was saved.
    let declared_popup = source
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == slot.popup())
        .filter(|parameter| matches!(parameter.declared_kind, Ok(Some(_))))
        .and_then(|parameter| parameter.numeric.as_ref().ok())
        .and_then(initial);
    if let Some(popup) = declared_popup
        && !BROWSED_POPUPS.contains(&popup)
    {
        return Err(format!(
            "popup {popup} ({}) selects no browsed file beside {path:?}, so which LUT applies is unverified",
            slot.popup()
        ));
    }
    let (section, section_name) = slot.section();
    let mut ignored = Vec::new();
    if !checkbox(source, section, &mut ignored).unwrap_or(true) {
        return Err(format!("its {section_name} section is off"));
    }
    let entries = lumetri_payload(source, luts::EMBEDDED_LUTS)
        .and_then(Result::ok)
        .map(luts::embedded_entries)
        .unwrap_or_default();
    let copy = luts::embedded_copy(&blob, &entries, &path).ok_or_else(|| {
        format!("After Effects saved no readable embedded copy of {path:?} (Embedded LUTs, ADBE Lumetri-0098; the file itself is not read)")
    })?;
    let mut notes = Vec::new();
    let mix = match slot {
        Slot::Input => 1.0,
        Slot::Look => {
            let property = numeric(source, &lumetri_control(luts::LOOK_INTENSITY), None)?;
            let intensity = initial(property).ok_or("Look Intensity is not a scalar")?;
            if property.animated || property.expression_enabled {
                notes.push(format!(
                    "Look Intensity keys or expression omitted: FX lookTransform mix is static; the initial Intensity {intensity} is kept"
                ));
            }
            if !(0.0..=200.0).contains(&intensity) {
                return Err(format!("Look Intensity {intensity} is outside 0 to 200"));
            }
            if let Some((_, Some(saved))) = &blob.look
                && (saved - intensity).abs() > 1e-6
            {
                return Err(format!(
                    "Look Intensity {intensity} differs from the Blob's __Intensity {saved}"
                ));
            }
            if intensity > 100.0 {
                notes.push(format!(
                    "Lumetri Creative Look Intensity {intensity} exceeds 100: FX lookTransform mix is at most 1, so it was clamped to 1 (Lumetri extrapolates the LUT beyond a full application)"
                ));
            }
            if let Some(space) = numeric(source, LOOK_COLOR_SPACE, None)
                .ok()
                .and_then(initial)
                && space != LOOK_COLOR_SPACE_FIXTURE
            {
                notes.push(format!(
                    "Lumetri Creative Look Color Space {space} ({LOOK_COLOR_SPACE}) is not the value {LOOK_COLOR_SPACE_FIXTURE} of the measured fixture (8 bpc, working space None): the LUT is imported as a lookTransform on display values, which is unmeasured for this colour space"
                ));
            }
            (intensity / 100.0).clamp(0.0, 1.0)
        }
    };
    let file_name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path.as_str())
        .to_owned();
    Ok(Some(BrowsedLut {
        lut: EmbeddedLut {
            file_name,
            lut: Lut3d {
                size: copy.size,
                samples: copy
                    .samples
                    .iter()
                    .map(|sample| sample.map(f64::from))
                    .collect(),
            },
        },
        mix,
        notes,
    }))
}
