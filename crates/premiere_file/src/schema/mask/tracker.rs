//! The saved affine Tracker samples in Premiere 26.5's vector mask.
//!
//! A sample is version 1 followed by 25 little-endian float32 values. The first
//! column-major 3x3 matrix maps the original outline in normalized source-frame
//! coordinates. The second repeats its linear part and records the transformed
//! centre. The final values repeat the original centre/anchor and unit scale.
//! These redundant fields constrain the supported layout; this is not a decoder
//! for Object Mask selection data or a live tracker.

use crate::error::{ensure, Result};
use crate::schema::text::PrShapePath;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MaskTrackerTransform {
    x_axis: [f64; 2],
    y_axis: [f64; 2],
    translation: [f64; 2],
}

impl MaskTrackerTransform {
    pub(crate) const IDENTITY: Self = Self {
        x_axis: [1.0, 0.0],
        y_axis: [0.0, 1.0],
        translation: [0.0, 0.0],
    };

    pub(crate) fn apply(self, path: &PrShapePath) -> PrShapePath {
        let mut path = path.clone();
        for vertex in &mut path.vertices {
            // Native path tangents are absolute Bézier control points, not
            // offsets from the anchor. Translate all three positions; a handle
            // collapsed onto its anchor must stay there after tracking.
            for point in [
                &mut vertex.point,
                &mut vertex.in_tangent,
                &mut vertex.out_tangent,
            ] {
                let [x, y] = point.map(f64::from);
                *point = [
                    (self.x_axis[0] * x + self.y_axis[0] * y + self.translation[0]) as f32,
                    (self.x_axis[1] * x + self.y_axis[1] * y + self.translation[1]) as f32,
                ];
            }
        }
        path
    }
}

pub(crate) fn decode_mask_tracker(payload: &[u8]) -> Result<MaskTrackerTransform> {
    ensure!(
        payload.len() == 104 && payload[..4] == 1_u32.to_le_bytes(),
        "unsupported vector mask Tracker sample layout"
    );
    let values: [f64; 25] = std::array::from_fn(|i| {
        let start = 4 + i * 4;
        f64::from(f32::from_le_bytes(
            payload[start..start + 4]
                .try_into()
                .expect("sample length checked above"),
        ))
    });
    ensure!(
        values.iter().all(|value| value.is_finite())
            && [2, 5, 11, 14, 24].into_iter().all(|i| values[i] == 0.0)
            && [8, 17, 22, 23].into_iter().all(|i| values[i] == 1.0)
            && [0, 1, 3, 4].into_iter().all(|i| values[i] == values[i + 9])
            && values[18..20] == values[20..22],
        "unsupported vector mask Tracker matrix or reference controls"
    );
    let transform = MaskTrackerTransform {
        x_axis: [values[0], values[1]],
        y_axis: [values[3], values[4]],
        translation: [values[6], values[7]],
    };
    for axis in 0..2 {
        let x = transform.x_axis[axis] * values[18];
        let y = transform.y_axis[axis] * values[19];
        let offset = transform.translation[axis];
        let centre = values[15 + axis];
        // Each saved input is float32. Allow its rounding, not a different
        // transform composition or a different tracker record family.
        let tolerance = f64::from(f32::EPSILON) * (x.abs() + y.abs() + offset.abs()).max(1.0);
        ensure!(
            (0.0..=tolerance).contains(&(x + y + offset - centre).abs()),
            "vector mask Tracker matrices disagree about the transformed centre"
        );
    }
    Ok(transform)
}
