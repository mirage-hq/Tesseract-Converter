//! Lumetri Vignette (Amount, Midpoint, Feather) ↔ FX `vignette`.
//!
//! Measured in After Effects 26.5 (8 bpc, working space None) on a flat grey
//! 160 frame, 1920×1080, 16 settings (Roundness 0): Lumetri leaves the centre
//! unchanged and multiplies display codes by a radial gain whose corner value
//! depends only on Amount, and whose ramp start and end depend only on
//! Midpoint and Feather. The FX vignette (`1 − amount · smoothstep(radius −
//! feather, radius, d)`, `d` the distance from the centre in normalised layer
//! coordinates) follows each measured gain map within 0.02–2.7 mean 8-bit
//! codes when fitted per setting; the closed form below is within 1.69 mean
//! codes over all 16 (4.1 for a brightening Amount +2). Premiere Pro runs the
//! same plug-in (its renders of Amount −2, Midpoint 50, Feather 50 darken the
//! corners to ×0.56–0.67, as here).

/// FX amount (corner darkening) per Lumetri Amount, measured; linear between
/// and beyond the samples, clamped to −1..=1.
const AMOUNTS: [(f64, f64); 6] = [
    (-3.0, 0.612),
    (-2.0, 0.444),
    (-1.0, 0.256),
    (0.0, 0.0),
    (1.0, -0.231),
    (2.0, -0.369),
];
/// Ramp centre `c = CENTRE[0] + CENTRE[1]·Midpoint + CENTRE[2]·Feather`.
const CENTRE: [f64; 3] = [0.31, 0.0038, 0.00048];
/// Ramp half-width `h = Feather · (HALF[0] + HALF[1]·Midpoint)`.
const HALF: [f64; 2] = [0.00201, 0.000_036_7];
/// Smallest FX radius/feather the importer writes.
const FLOOR: f64 = 0.001;

/// Why the mapping is an approximation, stated on import and export.
pub const VIGNETTE_APPROXIMATION: &str = "Lumetri Vignette mapped to FX vignette by a fit to After Effects 26.5 renders of 16 settings on a flat grey frame (amount from a measured table of Amount, radius and feather from the ramp Midpoint/Feather set: centre 0.31 + 0.0038 Midpoint + 0.00048 Feather, half-width Feather x (0.00201 + 0.0000367 Midpoint)); within 1.69 mean 8-bit codes over the 16 settings (4.1 for a brightening Amount +2); Roundness and Highlight are not mapped and the falloff follows the layer aspect";

fn interpolate(points: impl Iterator<Item = (f64, f64)> + Clone, x: f64) -> f64 {
    let points: Vec<_> = points.collect();
    let pair = points
        .windows(2)
        .find(|pair| x <= pair[1].0)
        .unwrap_or(&points[points.len() - 2..]);
    let [(x0, y0), (x1, y1)] = [pair[0], pair[1]];
    y0 + (x - x0) * (y1 - y0) / (x1 - x0)
}

/// FX `[amount, radius, feather]` for Lumetri `[Amount, Midpoint, Feather]`.
#[must_use]
pub fn to_fx([amount, midpoint, feather]: [f64; 3]) -> [f64; 3] {
    let centre = CENTRE[0] + CENTRE[1] * midpoint + CENTRE[2] * feather;
    let half = feather * (HALF[0] + HALF[1] * midpoint);
    [
        interpolate(AMOUNTS.iter().copied(), amount).clamp(-1.0, 1.0),
        (centre + half).max(FLOOR),
        (2.0 * half).max(FLOOR),
    ]
}

/// Lumetri `[Amount, Midpoint, Feather]` for FX `[amount, radius, feather]`,
/// the inverse of [`to_fx`]. Values outside the saved ranges (Amount −5..=5,
/// Midpoint and Feather 0..=100) are returned unclamped for the caller to
/// clamp and report: an FX ramp nearer the centre than Midpoint 0 allows
/// gives a negative Midpoint.
#[must_use]
pub fn from_fx([amount, radius, feather]: [f64; 3]) -> [f64; 3] {
    // AMOUNTS decreases with Amount: interpolate with the axes swapped.
    let lumetri_amount = interpolate(
        AMOUNTS.iter().rev().map(|(native, fx)| (*fx, *native)),
        amount,
    );
    let half = if feather <= FLOOR { 0.0 } else { feather / 2.0 };
    let centre = radius - half;
    let mut midpoint = (centre - CENTRE[0]) / CENTRE[1];
    let mut native_feather = 0.0;
    for _ in 0..32 {
        native_feather = half / (HALF[0] + HALF[1] * midpoint.clamp(0.0, 100.0));
        midpoint = (centre - CENTRE[0] - CENTRE[2] * native_feather) / CENTRE[1];
    }
    [lumetri_amount, midpoint, native_feather]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-setting fits of the FX vignette to After Effects 26.5 renders
    /// (headless-adobe job 11977): Lumetri (Amount, Midpoint, Feather) and the
    /// fitted FX (amount, ramp start, ramp end).
    const MEASURED: [([f64; 3], [f64; 3]); 6] = [
        ([-2.0, 50.0, 50.0], [0.444, 0.339, 0.704]),
        ([-1.0, 50.0, 50.0], [0.256, 0.333, 0.686]),
        ([-2.0, 20.0, 50.0], [0.444, 0.267, 0.540]),
        ([-2.0, 80.0, 50.0], [0.444, 0.399, 0.892]),
        ([-2.0, 50.0, 100.0], [0.444, 0.166, 0.929]),
        ([-2.0, 80.0, 0.0], [0.444, 0.608, 0.619]),
    ];

    #[test]
    fn closed_form_follows_the_measured_after_effects_falloffs() {
        for (native, [amount, start, end]) in MEASURED {
            let [fx_amount, radius, feather] = to_fx(native);
            assert!((fx_amount - amount).abs() < 0.01, "{native:?}");
            assert!((radius - end).abs() < 0.035, "{native:?}: radius {radius}");
            assert!(
                ((radius - feather) - start).abs() < 0.04,
                "{native:?}: start {}",
                radius - feather
            );
        }
    }

    #[test]
    fn export_inverts_import_inside_the_native_ranges() {
        for native in [
            [-2.0, 50.0, 50.0],
            [1.5, 20.0, 80.0],
            [-4.0, 90.0, 10.0],
            [0.0, 50.0, 0.0],
        ] {
            let back = from_fx(to_fx(native));
            for (axis, (got, want)) in back.iter().zip(native).enumerate() {
                assert!(
                    (got - want).abs() < 0.05,
                    "{native:?} axis {axis}: {back:?}"
                );
            }
        }
        // An FX ramp nearer the centre than Midpoint 0 allows is reported
        // below the saved range for the caller to clamp.
        assert!(from_fx([0.5, 0.1, 0.001])[1] < 0.0);
    }
}
