//! Lumetri Color's 24 arbitrary-data controls (`pard` kind 11).
//!
//! AE 26.5 saves every Lumetri instance with one default payload after each
//! of these declarations and one static `tdbs` + `aRbs` leaf per control in
//! the instance. The native catalog (`color_catalog.aep`) and the graded
//! `color_ae_lumetri_basic.aep` hold byte-identical payloads in both places:
//! the Basic/Creative scalars carry the grade, while this state (Blob sentinel,
//! curves, wheels, HSL, LUT slots, Auto Tone analytics) stays at its default.
//! The payloads are written verbatim so AE can load the instance; their
//! internal layouts are not interpreted, and no FX grade is encoded in them.

use crate::effects::definitions::ParameterDefinition;

pub(super) const MATCH_NAME: &str = crate::effects::color_grades::LUMETRI;

/// Size of one native curve block: header, two points and zero padding.
const CURVE_BLOCK: usize = 520;

/// One verbatim default payload, described by its observed structure.
enum DefaultPayload {
    /// The two-byte `FE FE` sentinel AE also saves for Premiere's Lumetri Blob.
    Sentinel,
    Zeros(usize),
    Bytes(&'static [u8]),
    /// `blocks` curve blocks: two little-endian header words and the identity
    /// points (0, 0), (1, 1) as little-endian f64, zero padded.
    Curves {
        header: [u32; 2],
        blocks: usize,
    },
    /// `count` wheels of little-endian f64 (0, 0.5, 0).
    Wheels(usize),
    /// UTF-16LE text without terminator.
    Utf16(&'static str),
}

/// Native declaration order of the kind-11 controls and their defaults.
const DEFAULTS: [(&str, DefaultPayload); 24] = [
    ("ADBE Lumetri-0001", DefaultPayload::Sentinel),
    ("ADBE Lumetri-0004", DefaultPayload::Sentinel),
    ("ADBE Lumetri-0095", DefaultPayload::Sentinel),
    ("ADBE Lumetri-0024", DefaultPayload::Sentinel),
    ("ADBE Lumetri-0096", DefaultPayload::Sentinel),
    (
        "ADBE Lumetri-0129",
        DefaultPayload::Bytes(&[
            0x34, 0x91, 0xc4, 0xcd, 0x1e, 0x21, 0x2a, 0xf0, 0x9a, 0xf0, 0xa4, 0xe1, 0x19, 0x76,
            0x81, 0x73,
        ]),
    ),
    ("ADBE Lumetri-0032", DefaultPayload::Zeros(32)),
    (
        "ADBE Lumetri-0039",
        DefaultPayload::Curves {
            header: [0, 2],
            blocks: 4,
        },
    ),
    (
        "ADBE Lumetri-0042",
        DefaultPayload::Curves {
            header: [3, 0],
            blocks: 1,
        },
    ),
    (
        "ADBE Lumetri-0106",
        DefaultPayload::Curves {
            header: [3, 0],
            blocks: 1,
        },
    ),
    (
        "ADBE Lumetri-0108",
        DefaultPayload::Curves {
            header: [3, 0],
            blocks: 1,
        },
    ),
    (
        "ADBE Lumetri-0110",
        DefaultPayload::Curves {
            header: [2, 0],
            blocks: 1,
        },
    ),
    (
        "ADBE Lumetri-0112",
        DefaultPayload::Curves {
            header: [2, 0],
            blocks: 1,
        },
    ),
    ("ADBE Lumetri-0047", DefaultPayload::Wheels(4)),
    (
        "ADBE Lumetri-0073",
        DefaultPayload::Bytes(&[
            0x00, 0x00, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ]),
    ),
    ("ADBE Lumetri-0085", DefaultPayload::Wheels(8)),
    ("ADBE Lumetri-0093", DefaultPayload::Zeros(4)),
    ("ADBE Lumetri-0060", DefaultPayload::Sentinel),
    ("ADBE Lumetri-0098", DefaultPayload::Sentinel),
    ("ADBE Lumetri-0099", DefaultPayload::Zeros(4)),
    ("ADBE Lumetri-0100", DefaultPayload::Sentinel),
    (
        "ADBE Lumetri-0122",
        DefaultPayload::Utf16(
            r#"{"mAutoTonePressed":false,"mBlacks":0,"mContrast":0,"mExposure":0,"mHighlights":0,"mLastPressedAutoToneButton":0,"mResetPressed":false,"mSaturation":0,"mShadows":0,"mTemperature":0,"mTint":0,"mWhites":0}"#,
        ),
    ),
    ("ADBE Lumetri-0125", DefaultPayload::Sentinel),
    ("ADBE Lumetri-0126", DefaultPayload::Sentinel),
];

impl DefaultPayload {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Sentinel => vec![0xfe, 0xfe],
            Self::Zeros(length) => vec![0; *length],
            Self::Bytes(bytes) => bytes.to_vec(),
            Self::Curves { header, blocks } => {
                let mut block = Vec::with_capacity(CURVE_BLOCK);
                header
                    .iter()
                    .for_each(|word| block.extend(word.to_le_bytes()));
                [0.0_f64, 0.0, 1.0, 1.0]
                    .iter()
                    .for_each(|value| block.extend(value.to_le_bytes()));
                block.resize(CURVE_BLOCK, 0);
                block.repeat(*blocks)
            }
            Self::Wheels(count) => [0.0_f64, 0.5, 0.0]
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
                .repeat(*count),
            Self::Utf16(text) => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        }
    }
}

/// AE 26.5 saves each Lumetri float slider's current value in its own `pard`
/// as a big-endian f64 in the first two payload words (the graded fixture's
/// Temperature 30 is `0x403E0000 0`, the catalog's Saturation default 100 is
/// `0x40590000 0`). Mirror the instance value there, as AE does.
pub(super) fn mirror_slider_value(parameter: &mut ParameterDefinition, value: f64) {
    let bits = value.to_bits();
    parameter.payload_words[0] = (bits >> 32) as u32;
    parameter.payload_words[1] = bits as u32;
}

/// The native default payload of one Lumetri arbitrary-data control.
pub(super) fn default_payload(control: &str) -> Option<Vec<u8>> {
    DEFAULTS
        .iter()
        .find(|(name, _)| *name == control)
        .map(|(_, payload)| payload.encode())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lumetri_defaults_cover_exactly_the_registry_arbitrary_controls() {
        let definition = crate::effects::definitions::definition(MATCH_NAME).unwrap();
        let arbitrary: Vec<_> = definition
            .parameters
            .iter()
            .filter(|parameter| parameter.kind == 11)
            .map(|parameter| parameter.match_name.as_str())
            .collect();
        let defaults: Vec<_> = DEFAULTS.iter().map(|(name, _)| *name).collect();
        assert_eq!(arbitrary, defaults);
        assert_eq!(default_payload("ADBE Lumetri-0039").unwrap().len(), 2080);
        assert_eq!(default_payload("ADBE Lumetri-0122").unwrap().len(), 406);
        assert_eq!(default_payload("ADBE Lumetri-0007"), None);
    }
}
