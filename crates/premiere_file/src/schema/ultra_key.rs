//! Import-only static Ultra Key surrogate. Native matte algorithms are proprietary.
use super::{records, EffectParamSpec, EffectSpec, PrColour, LEGACY_LUMA_THRESHOLD};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PrUltraKey {
    pub(crate) color: PrColour,
    pub(crate) tolerance: f64,
    pub(crate) pedestal: f64,
    pub(crate) transparency: f64,
}

pub(crate) const APPROXIMATION: &str = "Ultra Key replaced by an editable RGB-distance CustomShader keyer using saved Key Color, Tolerance, Pedestal and Transparency; radius = 0.2*Tolerance/100 + 0.1*Pedestal/100, softness = max(0.001, 0.1*Transparency/100). Native matte generation (Highlight/Shadow), cleanup, spill suppression, presets and color correction are not reproduced; their static values are omitted. RGB/alpha edge fidelity is unverified; animated controls and non-Composite Output remain unsupported";

const fn slider(id: usize, name: &'static str) -> EffectParamSpec {
    EffectParamSpec {
        id,
        name,
        label: name,
        record: records::VIDEO_FILTER_AMOUNT_PARAM,
        control: "8",
        binding: None,
        ..LEGACY_LUMA_THRESHOLD
    }
}
const fn popup(id: usize, name: &'static str, upper: &'static str) -> EffectParamSpec {
    EffectParamSpec {
        record: records::VIDEO_POPUP_PARAM,
        control: "7",
        upper_bound: upper,
        discontinuous_interpolate: true,
        ..slider(id, name)
    }
}
const fn group(id: usize, name: &'static str, control: &'static str) -> EffectParamSpec {
    EffectParamSpec {
        record: records::VIDEO_BOOL_COMPONENT_PARAM,
        control,
        lower_bound: "false",
        upper_bound: "false",
        ..slider(id, name)
    }
}
pub(crate) const ULTRA_KEY: EffectSpec = EffectSpec {
    match_name: "AE.ADBE Ultra Key",
    display_name: "Ultra Key",
    filter_type: "2",
    premiere_native: false,
    opaque_private_data: false,
    params: &[
        popup(1, "Output", "2"),
        popup(2, "Setting", "3"),
        EffectParamSpec {
            record: records::VIDEO_COLOR_PARAM,
            control: "5",
            upper_bound: "18446744073709551615",
            ..slider(3, "Key Color")
        },
        group(4, "Matte Generation", "11"),
        slider(5, "Transparency"),
        slider(6, "Highlight"),
        slider(7, "Shadow"),
        slider(8, "Tolerance"),
        slider(9, "Pedestal"),
        group(10, "", "12"),
        group(11, "Matte Cleanup", "11"),
        slider(12, "Choke"),
        slider(13, "Soften"),
        slider(14, "Contrast"),
        slider(15, "Mid Point"),
        group(16, "", "12"),
        group(17, "Spill Suppression", "11"),
        slider(18, "Desaturate"),
        slider(19, "Range"),
        slider(20, "Spill"),
        slider(21, "Luma"),
        group(22, "", "12"),
        group(23, "Color Correction", "11"),
        EffectParamSpec {
            upper_bound: "200",
            ..slider(24, "Saturation")
        },
        EffectParamSpec {
            lower_bound: "-180",
            upper_bound: "180",
            ..slider(25, "Hue")
        },
        EffectParamSpec {
            upper_bound: "200",
            ..slider(26, "Luminance")
        },
        group(27, "", "12"),
    ],
};
