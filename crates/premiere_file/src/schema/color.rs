//! Typed color and tone-map JSON embedded in Premiere XML records.

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToneMapSettings {
    peak: i64,
    version: u32,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ColorSpace {
    base_color_profile: ColorProfile,
    base_profile_type: u32,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    color_space_metadata: Option<ColorSpaceMetadata>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ColorProfile {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    color_profile_data: Option<String>,
    color_profile_name: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ColorSpaceMetadata {
    peak_luminance: u32,
}

// An absent optional field is supported; an explicit null is not a native profile.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Premiere input colour spaces whose curve and gamut equal a registered
/// Tesseract camera encoding. Names are Premiere's Interpret Footage → Color
/// Space Override labels; a committed project saves log interpretations in this
/// `<curve>/<gamut>` form (`Canon Log2/Cinema Gamut`), but the saved spelling
/// of each listed label is not individually pinned by a native fixture.
/// Near-names with a different curve (F-Log vs F-Log2, D-Log vs D-Log M,
/// Canon Log2 vs Log3) are deliberately absent, as are the registered labels
/// without a confirmed Premiere label (`fujifilm-flog2`, `nikon-nlog`,
/// `dji-dlog-m`, `blackmagic-film-gen5`).
const CAMERA_ENCODINGS: [(&str, &str); 8] = [
    ("Apple Log/Rec. 2020", "camera:apple-log:v1"),
    ("ARRI LogC3/Wide Gamut3", "camera:arri-logc3:v1"),
    ("ARRI LogC4/Wide Gamut4", "camera:arri-logc4:v1"),
    (
        "Canon Log3/Cinema Gamut",
        "camera:canon-log3-cinema-gamut:v1",
    ),
    ("Sony S-Log3/S-Gamut3", "camera:sony-slog3-sgamut3:v1"),
    (
        "Sony S-Log3/S-Gamut3.Cine",
        "camera:sony-slog3-sgamut3-cine:v1",
    ),
    ("Panasonic V-Log/V-Gamut", "camera:panasonic-vlog:v1"),
    ("Red Log3G10/Wide Gamut", "camera:red-log3g10:v1"),
];

/// The registered Tesseract camera encoding equal to Premiere input colour
/// space `name`, if any.
pub(crate) fn camera_encoding_for_premiere_space(name: &str) -> Option<&'static str> {
    CAMERA_ENCODINGS
        .iter()
        .find(|(premiere, _)| *premiere == name)
        .map(|(_, encoding)| *encoding)
}

/// The Premiere Color Space Override label equal to Tesseract camera
/// encoding `encoding`, if any.
pub(crate) fn premiere_space_for_camera_encoding(encoding: &str) -> Option<&'static str> {
    CAMERA_ENCODINGS
        .iter()
        .find(|(_, camera)| *camera == encoding)
        .map(|(premiere, _)| *premiere)
}

/// A clip's saved Interpret Footage colour-space override, classified by what
/// conversion can do with it. Legacy FX has no input colour-space field
/// without a LUT (`cameraInput` is ACES-only), so only the no-op classes
/// convert exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipColorSpaceOverride {
    /// A BT.709 display-referred interpretation: the legacy FX default.
    Rec709,
    /// A BT.2100 interpretation; exact only when it repeats the source's own
    /// saved HDR profile.
    Rec2100(fx_conv::HdrTransfer),
    /// Any other input space, with its equal registered camera encoding.
    Other {
        name: String,
        camera_encoding: Option<&'static str>,
    },
}

impl ClipColorSpaceOverride {
    /// Classify a saved `OverriddenColorSpace` JSON value; `{}` is no override.
    pub(crate) fn parse(encoded: &str) -> serde_json::Result<Option<Self>> {
        let value: serde_json::Value = serde_json::from_str(encoded)?;
        if value.as_object().is_some_and(serde_json::Map::is_empty) {
            return Ok(None);
        }
        let space: ColorSpace = serde_json::from_value(value)?;
        let name = space.base_color_profile.color_profile_name;
        Ok(Some(if name.starts_with("BT.709") || name == "Rec. 709" {
            Self::Rec709
        } else if name.starts_with("BT.2100 PQ") || name == "Rec. 2100 PQ" {
            Self::Rec2100(fx_conv::HdrTransfer::Pq)
        } else if name.starts_with("BT.2100 HLG") || name == "Rec. 2100 HLG" {
            Self::Rec2100(fx_conv::HdrTransfer::Hlg)
        } else {
            Self::Other {
                camera_encoding: camera_encoding_for_premiere_space(&name),
                name,
            }
        }))
    }
}

/// A media's Interpret Footage Input LUT, as Premiere 26.5.2 saves it
/// (`color_clip_input_lut.prproj`): the `VideoStream` names a `LUTAsset`
/// record by `InputLUTAsset`, repeats its registry GUID in `InputLUTID` and
/// its names in `InputLUTDisplayName` / `InputLUTFileName`, and the record
/// embeds the LUT samples (no file path is saved). Premiere applies it once
/// per media, before every effect of every clip of that media.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrClipInputLut {
    /// Premiere's LUT registry GUID (`InputLUTID`, `LUTAsset/Id`).
    pub(crate) id: String,
    /// The name Premiere shows (`InputLUTDisplayName`, `LUTAsset/Displayname`).
    pub(crate) display_name: String,
    /// The browsed file's name (`InputLUTFileName`, `LUTAsset/Filename`).
    pub(crate) file_name: String,
    /// The registered encoding the LUT reads: import takes it from a Color
    /// Space Override of the same clip; `None` when nothing declares it.
    pub(crate) input_encoding: Option<&'static str>,
    pub(crate) table: std::sync::Arc<crate::cube_lut::Lut3d>,
}

impl PrClipInputLut {
    /// The packaged `.cube` asset of this LUT on import; one per LUT GUID.
    pub(crate) fn asset_id(&self) -> fx_schema::AssetId {
        let id: String = self
            .id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        fx_schema::AssetId::from_trusted(format!("premiere-input-lut-{id}"))
    }

    /// The `.cube` file name the package uses: the saved name when it is a
    /// plain `.cube` file name, else one derived from the asset.
    pub(crate) fn package_file_name(&self) -> String {
        let plain = !self.file_name.is_empty()
            && !self.file_name.contains(['/', '\\'])
            && !self.file_name.chars().any(char::is_control)
            && self.file_name.to_ascii_lowercase().ends_with(".cube");
        if plain {
            self.file_name.clone()
        } else {
            format!("{}.cube", self.asset_id().as_str())
        }
    }
}

/// An active Interpret Footage Color Space Override of a video without an
/// Input LUT that names a registered FX camera encoding. Import maps it to the
/// layer's `source.inputTransform` `cameraInput` in an ACES document and
/// reports it on `media_record` in a legacy one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrCameraOverride {
    /// Premiere's override label, as saved.
    pub(crate) label: String,
    pub(crate) encoding: &'static str,
    pub(crate) media_record: String,
}

/// Why a legacy (non-ACES) document cannot hold the camera-log Color Space
/// Override `name` of registered camera encoding `encoding`.
pub(crate) fn legacy_camera_override_reason(name: &str, encoding: &str) -> String {
    format!("Interpret Footage Color Space Override {name:?} not converted: legacy FX has no input colour-space field without a LUT (cameraInput is ACES-only, and the sequence working colour space is not ACEScct); the equal Tesseract camera encoding is {encoding}, so add a {encoding} to Rec.709 .cube as source.inputTransform lut3d (inputEncoding {encoding}) to restore it; original log bytes and placement retained")
}

/// A sequence Color Setup whose working colour space is ACEScct, the native
/// equal of an FX ACES document (`colorManagement` `acescct` / `aces2.0` /
/// `footageOnly`). Premiere 26.5.2 saves it on the `VideoTrackGroup` as
/// `WorkingColorSpace` `{"baseColorProfile":{"colorProfileName":"ACEScct"},
/// "baseProfileType":1}` with `WorkingColorSpaceConfiguration` id `ACEScct`
/// and no `OutputColorSpace` for its default Rec.709 output
/// (`color_aces_pr_sequence.prproj`). `hdr_output` is a Rec.2100 output colour
/// space, which an FX document cannot hold (`tsrct export --hdr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SequenceAces {
    pub(crate) hdr_output: Option<fx_conv::HdrTransfer>,
}

impl SequenceAces {
    /// The Premiere working colour space name and configuration id.
    pub(crate) const WORKING_NAME: &'static str = "ACEScct";

    pub(crate) fn working_color_space() -> ColorSpace {
        ColorSpace {
            base_color_profile: ColorProfile {
                color_profile_data: None,
                color_profile_name: Self::WORKING_NAME.into(),
            },
            base_profile_type: 1,
            color_space_metadata: None,
        }
    }
}

/// Explicit generated sequence target; not inferred from source media.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SequenceHdr {
    pub(crate) transfer: fx_conv::HdrTransfer,
    pub(crate) graphics_white: f64,
}

impl SequenceHdr {
    /// `graphicsWhiteLuminance` as Premiere saves it: an integral white is a
    /// JSON integer (`203`, `color_hdr_pq_sequence.prproj`), not `203.0`.
    pub(crate) fn native_graphics_white(white: f64) -> serde_json::Value {
        if white.fract() == 0.0 && white.abs() <= f64::from(u32::MAX) {
            // Exact: integral and in range.
            serde_json::Value::from(white as i64)
        } else {
            serde_json::Value::from(white)
        }
    }

    pub(crate) fn working_name(self) -> &'static str {
        match self.transfer {
            fx_conv::HdrTransfer::Pq => "BT.2100 PQ RGB Full",
            fx_conv::HdrTransfer::Hlg => "BT.2100 HLG RGB Full",
        }
    }
}

/// Saved source profiles for 10-bit BT.709 and BT.2020 HLG/PQ video.
/// Other pass-through colours keep the existing SDR profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HdrProfile {
    Bt709,
    Hlg10Bit,
    Pq10Bit,
}

impl HdrProfile {
    const ALL: [Self; 2] = [Self::Hlg10Bit, Self::Pq10Bit];

    fn name(self) -> &'static str {
        match self {
            Self::Bt709 => "BT.709,10-bit,Display-Referred",
            Self::Hlg10Bit => "BT.2100 HLG,10-bit,Display-Referred",
            Self::Pq10Bit => "BT.2100 PQ,10-bit,Display-Referred",
        }
    }
}

impl ToneMapSettings {
    pub(crate) const DEFAULT: Self = Self {
        peak: -1,
        version: 3,
    };
}

impl ColorSpace {
    pub(crate) fn sequence_sdr() -> Self {
        Self {
            base_color_profile: ColorProfile {
                color_profile_data: None,
                color_profile_name: "BT.709 RGB Full".into(),
            },
            base_profile_type: 1,
            color_space_metadata: None,
        }
    }

    pub(crate) fn working_hdr(settings: SequenceHdr) -> Self {
        Self::rgb_hdr(settings.transfer)
    }

    /// Exact nonlinear native RGB source/working profile; not a video output profile.
    pub(crate) fn rgb_hdr(transfer: fx_conv::HdrTransfer) -> Self {
        Self {
            base_color_profile: ColorProfile {
                color_profile_data: None,
                color_profile_name: match transfer {
                    fx_conv::HdrTransfer::Pq => "BT.2100 PQ RGB Full",
                    fx_conv::HdrTransfer::Hlg => "BT.2100 HLG RGB Full",
                }
                .into(),
            },
            base_profile_type: 1,
            color_space_metadata: None,
        }
    }

    pub(crate) fn rgb_hdr_transfer(&self) -> Option<fx_conv::HdrTransfer> {
        [fx_conv::HdrTransfer::Pq, fx_conv::HdrTransfer::Hlg]
            .into_iter()
            .find(|transfer| self == &Self::rgb_hdr(*transfer))
    }

    pub(crate) fn output_hdr_transfer(&self) -> Option<fx_conv::HdrTransfer> {
        [fx_conv::HdrTransfer::Pq, fx_conv::HdrTransfer::Hlg]
            .into_iter()
            .find(|transfer| {
                self == &Self::sequence_hdr(SequenceHdr {
                    transfer: *transfer,
                    graphics_white: 609.0,
                })
            })
    }

    pub(crate) fn sequence_hdr(settings: SequenceHdr) -> Self {
        Self {
            base_color_profile: ColorProfile {
                color_profile_data: Some("AQAAAOgDAAA=".into()),
                color_profile_name: match settings.transfer {
                    fx_conv::HdrTransfer::Pq => HdrProfile::Pq10Bit.name(),
                    fx_conv::HdrTransfer::Hlg => HdrProfile::Hlg10Bit.name(),
                }
                .into(),
            },
            base_profile_type: 1,
            color_space_metadata: Some(ColorSpaceMetadata {
                peak_luminance: 1000,
            }),
        }
    }

    /// The saved `OverriddenColorSpace` of an Interpret Footage Color Space
    /// Override named `name`: Premiere 26.5.2 saves only the label, with no
    /// profile data (`color_clip_colorspace_override.prproj`).
    pub(crate) fn clip_override(name: &str) -> Self {
        Self {
            base_color_profile: ColorProfile {
                color_profile_data: None,
                color_profile_name: name.into(),
            },
            base_profile_type: 1,
            color_space_metadata: None,
        }
    }

    pub(crate) fn source_sdr() -> Self {
        Self::source("BT.709,32f,Display-Referred")
    }

    /// Saved 10-bit source profile; its payload is shared with SDR.
    pub(crate) fn source_hdr(profile: HdrProfile) -> Self {
        Self::source(profile.name())
    }

    /// The BT.2100 transfer of a saved 10-bit HLG/PQ source profile.
    pub(crate) fn source_hdr_transfer(&self) -> Option<fx_conv::HdrTransfer> {
        [
            (HdrProfile::Hlg10Bit, fx_conv::HdrTransfer::Hlg),
            (HdrProfile::Pq10Bit, fx_conv::HdrTransfer::Pq),
        ]
        .into_iter()
        .find(|(profile, _)| self == &Self::source_hdr(*profile))
        .map(|(_, transfer)| transfer)
    }

    fn source(name: &str) -> Self {
        Self {
            base_color_profile: ColorProfile {
                color_profile_data: Some("AQAAAP////8=".into()),
                color_profile_name: name.into(),
            },
            base_profile_type: 1,
            color_space_metadata: None,
        }
    }

    /// Saved payload, peak metadata and working precision do not change a named
    /// BT.709 SDR working space. HDR working spaces still require another mapping.
    pub(crate) fn is_sequence_sdr(&self) -> bool {
        self.base_profile_type == 1
            && matches!(
                self.base_color_profile.color_profile_name.as_str(),
                "BT.709 RGB Full"
                    | "BT.709,8-bit,Display-Referred"
                    | "BT.709,10-bit,Display-Referred"
                    | "BT.709,32f,Display-Referred"
            )
    }

    /// Numeric CICP 6/6/6 describes limited-range SMPTE 170M SDR, not BT.709.
    pub(crate) fn is_smpte170m_video(&self) -> bool {
        self == &Self::source("6,6,6,Narrow,YCC,8-bit,Display-Referred")
    }

    /// Premiere tags 8-bit SDR sources with the 8-bit profile name and the same profile data.
    /// Native 8-bit and 10-bit BT.709 sources also save a short form without profile data;
    /// neither form establishes codec support, which media inspection checks separately.
    /// Premiere 26.5.1 saves tag PNG stills with the `sequence_sdr` profile, and
    /// HDR video sources with an [`HdrProfile`].
    pub(crate) fn is_source(&self) -> bool {
        self == &Self::source_sdr()
            || (self.base_profile_type == 1
                && matches!(
                    self.base_color_profile.color_profile_name.as_str(),
                    "BT.709,8-bit,Display-Referred" | "BT.709,10-bit,Display-Referred"
                )
                && matches!(
                    self.base_color_profile.color_profile_data.as_deref(),
                    None | Some("AQAAAP////8=")
                )
                && self.color_space_metadata.is_none())
            || self == &Self::sequence_sdr()
            || HdrProfile::ALL
                .iter()
                .any(|profile| self == &Self::source_hdr(*profile))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_encoding_table_is_one_to_one_and_omits_different_curves() {
        for (premiere, encoding) in CAMERA_ENCODINGS {
            assert_eq!(camera_encoding_for_premiere_space(premiere), Some(encoding));
            assert_eq!(premiere_space_for_camera_encoding(encoding), Some(premiere));
        }
        for near_name in [
            "Fuji F-Log/Rec. 2020",
            "DJI D-Log/D-Gamut",
            "Canon Log2/Cinema Gamut",
            "Rec. 709",
        ] {
            assert_eq!(camera_encoding_for_premiere_space(near_name), None);
        }
        for unmapped in [
            "camera:fujifilm-flog2:v1",
            "camera:nikon-nlog:v1",
            "camera:dji-dlog-m:v1",
            "camera:blackmagic-film-gen5:v1",
        ] {
            assert_eq!(premiere_space_for_camera_encoding(unmapped), None);
        }
    }

    #[test]
    fn clip_override_classifies_saved_profiles() {
        let saved = |name: &str| {
            format!(r#"{{"baseColorProfile":{{"colorProfileName":"{name}"}},"baseProfileType":1}}"#)
        };
        assert_eq!(ClipColorSpaceOverride::parse("{}").unwrap(), None);
        assert_eq!(
            ClipColorSpaceOverride::parse(&saved("BT.709,10-bit,Display-Referred")).unwrap(),
            Some(ClipColorSpaceOverride::Rec709)
        );
        assert_eq!(
            ClipColorSpaceOverride::parse(&saved("BT.2100 HLG,10-bit,Display-Referred")).unwrap(),
            Some(ClipColorSpaceOverride::Rec2100(fx_conv::HdrTransfer::Hlg))
        );
        assert_eq!(
            ClipColorSpaceOverride::parse(&saved("ARRI LogC4/Wide Gamut4")).unwrap(),
            Some(ClipColorSpaceOverride::Other {
                name: "ARRI LogC4/Wide Gamut4".into(),
                camera_encoding: Some("camera:arri-logc4:v1"),
            })
        );
        assert!(ClipColorSpaceOverride::parse("[]").is_err());
        assert!(ClipColorSpaceOverride::parse(r#"{"future":1}"#).is_err());
    }

    #[test]
    fn source_hdr_transfer_matches_only_saved_hdr_source_profiles() {
        assert_eq!(
            ColorSpace::source_hdr(HdrProfile::Pq10Bit).source_hdr_transfer(),
            Some(fx_conv::HdrTransfer::Pq)
        );
        assert_eq!(
            ColorSpace::source_hdr(HdrProfile::Hlg10Bit).source_hdr_transfer(),
            Some(fx_conv::HdrTransfer::Hlg)
        );
        assert_eq!(ColorSpace::source_sdr().source_hdr_transfer(), None);
    }
}
