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

/// Explicit generated sequence target; not inferred from source media.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SequenceHdr {
    pub(crate) transfer: fx_conv::HdrTransfer,
    pub(crate) graphics_white: f64,
}

impl SequenceHdr {
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

    pub(crate) fn source_sdr() -> Self {
        Self::source("BT.709,32f,Display-Referred")
    }

    /// Saved 10-bit source profile; its payload is shared with SDR.
    pub(crate) fn source_hdr(profile: HdrProfile) -> Self {
        Self::source(profile.name())
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
