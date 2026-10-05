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

/// The `OriginalColorSpace` profile that Premiere 26.5.1 saves on an HDR video
/// source, measured on a Rec. 709 sequence for a 10-bit BT.2020 HLG and a
/// 10-bit BT.2020 PQ `hvc1` source and an iPhone HLG capture with a Dolby
/// Vision box, which saves the HLG profile. Other
/// pass-through colours have no observed profile and keep the BT.709 text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HdrProfile {
    Hlg10Bit,
    Pq10Bit,
}

impl HdrProfile {
    const ALL: [Self; 2] = [Self::Hlg10Bit, Self::Pq10Bit];

    fn name(self) -> &'static str {
        match self {
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

    pub(crate) fn source_sdr() -> Self {
        Self::source("BT.709,32f,Display-Referred")
    }

    /// The saved profile of an HDR video source; the same data as the SDR one.
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

    /// Premiere writes the 8-bit sequence profile with its data and peak luminance,
    /// or without both when Premiere 26.5.1 upgrades or re-saves a project.
    pub(crate) fn is_sequence_sdr(&self) -> bool {
        self == &Self::sequence_sdr()
            || self.base_profile_type == 1
                && matches!(
                    (
                        self.base_color_profile.color_profile_name.as_str(),
                        self.base_color_profile.color_profile_data.as_deref(),
                        self.color_space_metadata
                            .as_ref()
                            .map(|metadata| metadata.peak_luminance),
                    ),
                    (
                        "BT.709,8-bit,Display-Referred",
                        Some("AQAAAGQAAAA="),
                        Some(100)
                    ) | ("BT.709,8-bit,Display-Referred", None, None)
                )
    }

    /// Premiere tags 8-bit SDR sources with the 8-bit profile name and the same profile data.
    /// Native 10-bit BT.709 sources also save a short form without profile data;
    /// neither form establishes codec support, which media inspection checks separately.
    /// Premiere 26.5.1 saves tag PNG stills with the `sequence_sdr` profile, and
    /// HDR video sources with an [`HdrProfile`].
    pub(crate) fn is_source(&self) -> bool {
        self == &Self::source_sdr()
            || self == &Self::source("BT.709,8-bit,Display-Referred")
            || (self.base_profile_type == 1
                && self.base_color_profile.color_profile_name == "BT.709,10-bit,Display-Referred"
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
