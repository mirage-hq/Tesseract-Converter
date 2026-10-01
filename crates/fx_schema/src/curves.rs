//! Versioned Master/RGB curves for display-referred SDR color correction.
//!
//! Version 1 evaluates normalized, piecewise-linear curves on straight RGB in
//! this fixed order: Master, then the matching red/green/blue channel curve.
//! Alpha is preserved and callers re-premultiply after evaluation. The curve
//! endpoints have fixed x coordinates (`0` and `1`) but adjustable y values.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[path = "curves_declaration.rs"]
mod declaration;

crate::define_color_curves_schema!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_bad_point_sets() {
        for points in [
            vec![(0.0, 0.0)],
            vec![(0.0, 0.0), (0.0, 0.5), (1.0, 1.0)],
            vec![(0.1, 0.0), (1.0, 1.0)],
            vec![(0.0, -0.1), (1.0, 1.0)],
            vec![(0.0, 0.0), (1.0, f64::NAN)],
        ] {
            assert!(ColorCurve::new(
                points
                    .into_iter()
                    .map(|(x, y)| ColorCurvePoint::new(x, y))
                    .collect()
            )
            .is_err());
        }
        assert!(ColorCurve::new(vec![ColorCurvePoint::new(0.0, 0.0); 33]).is_err());
        assert!(ColorCurve::new(vec![
            ColorCurvePoint::new(0.0, 0.0),
            ColorCurvePoint::new(0.5, 0.5),
            ColorCurvePoint::new(0.5 + f64::EPSILON, 0.7),
            ColorCurvePoint::new(1.0, 1.0),
        ])
        .is_err());
    }
}
