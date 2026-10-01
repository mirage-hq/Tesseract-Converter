//! Defaults for omitted native vector leaves, not for malformed present values.
//!
//! Pinned oracle: py-aep e12a451c35bacd3f34a080090265f9370e66162b,
//! synthesis/property.py vector PropSpecs. Unlike timeline Scale/Opacity,
//! vector percentages are stored in their displayed 0..100 units.

pub(super) fn numeric(name: &str) -> Option<&'static [f64]> {
    Some(match name {
        "ADBE Vector Blend Mode"
        | "ADBE Vector Composite Order"
        | "ADBE Vector Fill Rule"
        | "ADBE Vector Grad Type"
        | "ADBE Vector Stroke Line Cap"
        | "ADBE Vector Stroke Line Join"
        | "ADBE Vector Shape Direction"
        | "ADBE Vector Star Type"
        | "ADBE Vector Merge Type"
        | "ADBE Vector Offset Line Join"
        | "ADBE Vector Offset Copies"
        | "ADBE Vector Offset Copy Offset"
        | "ADBE Vector Trim Type" => &[1.0],
        "ADBE Vector Fill Opacity"
        | "ADBE Vector Stroke Opacity"
        | "ADBE Vector Group Opacity"
        | "ADBE Vector Star Outer Radius"
        | "ADBE Vector Trim End" => &[100.0],
        "ADBE Vector Grad HiLite Length"
        | "ADBE Vector Grad HiLite Angle"
        | "ADBE Vector Grad Rotation"
        | "ADBE Vector Star Rotation"
        | "ADBE Vector Star Inner Roundess"
        | "ADBE Vector Star Outer Roundess"
        | "ADBE Vector Stroke Offset"
        | "ADBE Vector Skew"
        | "ADBE Vector Skew Axis"
        | "ADBE Vector Rotation"
        | "ADBE Vector Rect Roundness"
        | "ADBE Vector Trim Start"
        | "ADBE Vector Trim Offset" => &[0.0],
        "ADBE Vector Grad Start Pt"
        | "ADBE Vector Star Position"
        | "ADBE Vector Anchor"
        | "ADBE Vector Position"
        | "ADBE Vector Ellipse Position"
        | "ADBE Vector Rect Position" => &[0.0, 0.0],
        "ADBE Vector Grad End Pt" => &[100.0, 0.0],
        "ADBE Vector Grad Scale"
        | "ADBE Vector Scale"
        | "ADBE Vector Ellipse Size"
        | "ADBE Vector Rect Size" => &[100.0, 100.0],
        "ADBE Vector Stroke Width" => &[2.0],
        "ADBE Vector Stroke Miter Limit" | "ADBE Vector Offset Miter Limit" => &[4.0],
        "ADBE Vector Star Points" => &[5.0],
        "ADBE Vector Star Inner Radius" => &[50.0],
        "ADBE Vector Offset Amount" | "ADBE Vector RoundCorner Radius" => &[10.0],
        "ADBE Vector Fill Color" => &[1.0, 0.0, 0.0, 1.0],
        "ADBE Vector Stroke Color" => &[1.0, 1.0, 1.0, 1.0],
        _ => return None,
    })
}
