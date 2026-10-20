//! Point-colour meaning at the FX display-sRGB ↔ native BT.2100 boundary.
//!
//! This does not transform footage, gradients, effects or interpolation domains.
//! Callers must report those limitations and bind the returned values to a
//! matching native working profile; merely assigning a profile is insufficient.

use thiserror::Error;

/// Explicit native export transfer, independent of persisted FX policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdrTransfer {
    /// ST 2084, with 1.0 corresponding to 10000 cd/m².
    Pq,
    /// BT.2100 HLG, matching the existing FX reference-white normalization.
    Hlg,
}

/// A point colour cannot be represented by the bounded native bridge.
#[derive(Debug, Error, PartialEq)]
pub enum HdrColorError {
    /// Invalid graphics-white intent must not silently use a different gain.
    #[error("HDR graphics white must be finite and in 100..=1000 cd/m²")]
    GraphicsWhite,
    /// No clipping is performed to make unsupported colour coordinates fit.
    #[error("{0}")]
    Domain(&'static str),
}

// Keep these identical to the existing renderer; do not change its semantics.
const TO_2020: [[f64; 3]; 3] = [
    [0.627_403_9, 0.329_283, 0.043_313_1],
    [0.069_097_3, 0.919_540_4, 0.011_362_3],
    [0.016_391_4, 0.088_013_3, 0.895_595_3],
];
const HLG_WHITE_SCALE: f64 = 3.774_117_6;
const A: f64 = 0.178_832_77;
const B: f64 = 0.284_668_92;
const C: f64 = 0.559_910_73;
const M1: f64 = 2610.0 / 16384.0;
const M2: f64 = 2523.0 / 32.0;
const C1: f64 = 3424.0 / 4096.0;
const C2: f64 = 2413.0 / 128.0;
const C3: f64 = 2392.0 / 128.0;

/// Converter-local colour context. Missing FX graphics white means 609, not 203.
#[derive(Debug, Clone, Copy)]
pub struct HdrColorBridge {
    transfer: HdrTransfer,
    white: f64,
}

impl HdrColorBridge {
    /// Validate the current FX graphics-white range without modifying FX data.
    pub fn new(transfer: HdrTransfer, white: Option<f64>) -> Result<Self, HdrColorError> {
        let white = white.unwrap_or(609.0);
        if !white.is_finite() || !(100.0..=1000.0).contains(&white) {
            return Err(HdrColorError::GraphicsWhite);
        }
        Ok(Self { transfer, white })
    }

    /// Convert one nonnegative extended-sRGB colour; coverage alpha is unchanged.
    pub fn to_native(self, color: [f64; 4]) -> Result<[f64; 4], HdrColorError> {
        validate(color)?;
        let linear = [color[0], color[1], color[2]].map(srgb_decode);
        let rgb = TO_2020.map(|row| row.iter().zip(&linear).map(|(a, b)| a * b).sum::<f64>());
        let mut result = color;
        for (out, value) in result[..3].iter_mut().zip(rgb) {
            let normalized = match self.transfer {
                HdrTransfer::Pq => value * self.white / 10000.0,
                HdrTransfer::Hlg => value * self.white / (203.0 * HLG_WHITE_SCALE),
            };
            if !normalized.is_finite() || !(0.0..=1.0).contains(&normalized) {
                return Err(HdrColorError::Domain(
                    "FX colour exceeds the nominal native HDR transfer domain",
                ));
            }
            *out = match self.transfer {
                HdrTransfer::Pq => {
                    let p = normalized.powf(M1);
                    ((C1 + C2 * p) / (1.0 + C3 * p)).powf(M2)
                }
                HdrTransfer::Hlg if normalized <= 1.0 / 12.0 => (3.0 * normalized).sqrt(),
                HdrTransfer::Hlg => A * (12.0 * normalized - B).ln() + C,
            };
        }
        Ok(result)
    }

    /// Invert an explicitly identified native working transfer to editable FX.
    /// Out-of-BT.709-gamut colours reject, rather than silently losing saturation.
    pub fn to_fx(self, color: [f64; 4]) -> Result<[f64; 4], HdrColorError> {
        let linear = self.native_linear(color)?;
        if linear.iter().any(|v| *v < 0.0) {
            return Err(HdrColorError::Domain(
                "Native colour lies outside the nonnegative editable BT.709 gamut",
            ));
        }
        Ok(encode_fx(linear, color[3]))
    }

    /// Import with an explicit linear-BT.709 gamut approximation.
    ///
    /// Preserve luminance and the RGB chroma direction around the achromatic
    /// axis, reducing chroma only until the first channel reaches zero. Extended
    /// above-white values and coverage survive. The boolean requires the caller
    /// to diagnose saturation loss; this is not a perceptual hue/fidelity claim.
    pub fn to_fx_gamut_mapped(self, color: [f64; 4]) -> Result<([f64; 4], bool), HdrColorError> {
        let linear = self.native_linear(color)?;
        let minimum = linear.into_iter().fold(f64::INFINITY, f64::min);
        if minimum >= 0.0 {
            return Ok((encode_fx(linear, color[3]), false));
        }
        let luminance: f64 = [0.2126, 0.7152, 0.0722]
            .into_iter()
            .zip(linear)
            .map(|(weight, channel)| weight * channel)
            .sum();
        if !luminance.is_finite() || luminance <= 0.0 {
            return Err(HdrColorError::Domain(
                "Native gamut mapping requires positive luminance",
            ));
        }
        let chroma = luminance / (luminance - minimum);
        // The analytical boundary is zero. max removes only its floating-point
        // roundoff; there is no channel-wise gamut clip or upper-white clamp.
        let mapped = linear.map(|channel| (luminance + chroma * (channel - luminance)).max(0.0));
        Ok((encode_fx(mapped, color[3]), true))
    }

    fn native_linear(self, color: [f64; 4]) -> Result<[f64; 3], HdrColorError> {
        validate(color)?;
        if color[..3].iter().any(|v| *v > 1.0) {
            return Err(HdrColorError::Domain(
                "Native HDR colour exceeds the nominal transfer domain",
            ));
        }
        let rgb = [color[0], color[1], color[2]].map(|v| match self.transfer {
            HdrTransfer::Pq => {
                let p = v.powf(1.0 / M2);
                ((p - C1).max(0.0) / (C2 - C3 * p)).powf(1.0 / M1) * 10000.0 / self.white
            }
            HdrTransfer::Hlg => {
                let scene = if v <= 0.5 {
                    v * v / 3.0
                } else {
                    ((v - C) / A).exp() / 12.0 + B / 12.0
                };
                scene * 203.0 * HLG_WHITE_SCALE / self.white
            }
        });
        let linear = inverse_multiply(rgb);
        if linear.iter().any(|v| !v.is_finite()) {
            return Err(HdrColorError::Domain(
                "Native colour produced nonfinite linear RGB",
            ));
        }
        Ok(linear)
    }
}

fn encode_fx(linear: [f64; 3], alpha: f64) -> [f64; 4] {
    [
        srgb_encode(linear[0]),
        srgb_encode(linear[1]),
        srgb_encode(linear[2]),
        alpha,
    ]
}

fn validate(color: [f64; 4]) -> Result<(), HdrColorError> {
    if color.iter().any(|v| !v.is_finite())
        || color[..3].iter().any(|v| *v < 0.0)
        || !(0.0..=1.0).contains(&color[3])
    {
        return Err(HdrColorError::Domain(
            "Colour requires finite nonnegative RGB and coverage alpha in 0..=1",
        ));
    }
    Ok(())
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn inverse_multiply(rgb: [f64; 3]) -> [f64; 3] {
    let [a, b, c] = TO_2020;
    let cross = |u: [f64; 3], v: [f64; 3]| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let columns = [cross(b, c), cross(c, a), cross(a, b)];
    let determinant: f64 = a.iter().zip(columns[0]).map(|(a, b)| a * b).sum();
    std::array::from_fn(|i| (0..3).map(|j| columns[j][i] * rgb[j]).sum::<f64>() / determinant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pq_white_uses_physical_nits_not_profile_assignment() {
        for (white, signal) in [(203.0, 0.5806888810416109), (1000.0, 0.751827096247041)] {
            let bridge = HdrColorBridge::new(HdrTransfer::Pq, Some(white)).unwrap();
            let result = bridge.to_native([1.0, 1.0, 1.0, 0.25]).unwrap();
            assert!((result[0] - signal).abs() < 1e-10);
            assert_eq!(result[3], 0.25);
        }
    }

    #[test]
    fn hlg_white_matches_existing_shader_normalization() {
        let bridge = HdrColorBridge::new(HdrTransfer::Hlg, Some(203.0)).unwrap();
        let result = bridge.to_native([1.0, 1.0, 1.0, 1.0]).unwrap();
        assert!((result[0] - 0.75).abs() < 1e-7);
        let default = HdrColorBridge::new(HdrTransfer::Hlg, None).unwrap();
        assert!(default.to_native([1.0; 4]).unwrap()[0] > 0.95);
        assert!(default.to_native([2.0, 2.0, 2.0, 1.0]).is_err());
    }

    #[test]
    fn extended_colour_and_alpha_roundtrip_is_supplementary_only() {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            let bridge = HdrColorBridge::new(transfer, Some(203.0)).unwrap();
            for color in [
                [0.0, 0.0, 0.0, 0.0],
                [0.25, 0.5, 0.75, 0.4],
                [1.2, 1.1, 0.9, 1.0],
            ] {
                let result = bridge.to_fx(bridge.to_native(color).unwrap()).unwrap();
                for (a, b) in color.into_iter().zip(result) {
                    assert!((a - b).abs() < 1e-10, "{a} {b}");
                }
            }
        }
    }

    #[test]
    fn gamut_mapping_preserves_linear_luminance_chroma_direction_and_headroom() {
        for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
            let bridge = HdrColorBridge::new(transfer, None).unwrap();
            for rgb in [
                [0.75, 0.0, 0.0],
                [0.0, 0.75, 0.0],
                [0.0, 0.0, 0.75],
                [0.75, 0.75, 0.0],
                [0.0, 0.75, 0.75],
                [0.75, 0.0, 0.75],
            ] {
                let color = [rgb[0], rgb[1], rgb[2], 0.4];
                let original = bridge.native_linear(color).unwrap();
                let (mapped, approximated) = bridge.to_fx_gamut_mapped(color).unwrap();
                assert!(approximated);
                let decoded = [mapped[0], mapped[1], mapped[2]].map(srgb_decode);
                let luma = |rgb: [f64; 3]| rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
                let y = luma(original);
                assert!((luma(decoded) - y).abs() < 1e-12);
                assert!(decoded.iter().all(|v| *v >= 0.0));
                assert!(decoded.iter().any(|v| *v < 1e-12));
                let scale = y / (y - original.into_iter().fold(f64::INFINITY, f64::min));
                for (before, after) in original.into_iter().zip(decoded) {
                    assert!((after - y - scale * (before - y)).abs() < 1e-12);
                }
                assert_eq!(mapped[3], 0.4);
            }
            let native = bridge.to_native([1.1, 1.1, 1.1, 0.25]).unwrap();
            let (mapped, approximated) = bridge.to_fx_gamut_mapped(native).unwrap();
            assert!(!approximated);
            assert!(mapped[..3].iter().all(|v| *v > 1.0));
            assert_eq!(mapped[3], 0.25);
            assert!(bridge.to_fx_gamut_mapped([1.1, 0.0, 0.0, 1.0]).is_err());
        }
    }

    #[test]
    fn domain_failures_never_clip_or_guess() {
        assert!(HdrColorBridge::new(HdrTransfer::Pq, Some(f64::NAN)).is_err());
        let bridge = HdrColorBridge::new(HdrTransfer::Pq, None).unwrap();
        for color in [
            [-0.1, 0.0, 0.0, 1.0],
            [100.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 2.0],
            [f64::INFINITY, 0.0, 0.0, 1.0],
        ] {
            assert!(bridge.to_native(color).is_err());
        }
        assert!(bridge.to_fx([1.1, 0.0, 0.0, 1.0]).is_err());
        assert!(bridge.to_fx([0.7, 0.0, 0.0, 1.0]).is_err());
    }
}
