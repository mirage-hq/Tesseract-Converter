//! Static eight-bit Curves tables; native spline controls remain an approximation.

use fx_schema::{
    ColorCurve, ColorCurvePoint, ColorCurves, ColorCurvesSemanticVersion, LayerEffect,
    MAX_COLOR_CURVE_POINTS,
};

use crate::{
    properties::{self, PropertyError},
    rifx::Chunk,
};

pub(crate) const MATCH_NAME: &str = "ADBE CurvesCustom";
const CONTROL: &str = "ADBE CurvesCustom-0001";
const SAMPLES: usize = 256;

pub(crate) fn lower(content: &[Chunk], ordinal: usize) -> Result<LayerEffect, PropertyError> {
    let roots = properties::root_runs(content)?;
    let parade = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade")
        .collect::<Vec<_>>();
    let [(_, parade)] = parade.as_slice() else {
        return Err(PropertyError::Layout("ambiguous Effect Parade"));
    };
    let instances = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let (name, instance) = instances
        .get(
            ordinal
                .checked_sub(1)
                .ok_or(PropertyError::Layout("invalid Curves ordinal"))?,
        )
        .ok_or(PropertyError::Layout("missing Curves occurrence"))?;
    if *name != MATCH_NAME {
        return Err(PropertyError::Layout("Curves occurrence identity mismatch"));
    }
    let plugin = properties::unique_list(instance, *b"sspc")?;
    let controls = properties::runs(properties::unique_list(plugin, *b"tdgp")?)?;
    let matches = controls
        .iter()
        .filter(|(name, _)| *name == CONTROL)
        .collect::<Vec<_>>();
    let [(_, run)] = matches.as_slice() else {
        return Err(PropertyError::Layout(
            "missing/duplicate packed Curves control",
        ));
    };
    let leaf = properties::unique_list(run, *b"tdbs")?;
    let metadata = properties::read_path_metadata(leaf)?;
    if metadata.animated || metadata.expression_present || !metadata.keyframes.is_empty() {
        return Err(PropertyError::Layout(
            "animated/expression Curves unsupported",
        ));
    }
    let meta = properties::data(leaf, *b"tdb4")?;
    if meta.len() < 60 || meta[2..4] != [0, 1] || meta[57] != 1 || meta[59] != 8 {
        return Err(PropertyError::Layout("unknown Curves property type"));
    }
    let arbitrary = properties::unique_list(run, *b"aRbs")?;
    if arbitrary.len() != 1 {
        return Err(PropertyError::Layout("ambiguous Curves arbitrary records"));
    }
    from_bytes(properties::data(arbitrary, *b"aRbp")?)
}

fn from_bytes(bytes: &[u8]) -> Result<LayerEffect, PropertyError> {
    // Version 1 has five 256-byte tables followed by native editor point state.
    if bytes.len() != 1644 || bytes[..4] != [0, 1, 0, 1] {
        return Err(PropertyError::Layout("unknown Curves table size/version"));
    }
    let tables: Vec<_> = bytes[4..4 + 5 * SAMPLES]
        .as_chunks::<SAMPLES>()
        .0
        .iter()
        .collect();
    let identity = |table: &[u8]| table.iter().enumerate().all(|(x, y)| usize::from(*y) == x);
    if !identity(tables[4]) {
        return Err(PropertyError::Layout(
            "nonidentity Curves alpha table unsupported",
        ));
    }
    if !identity(tables[0]) && tables[1..4].iter().any(|table| !identity(*table)) {
        return Err(PropertyError::Layout(
            "combined master/channel Curves order unverified",
        ));
    }
    Ok(LayerEffect::ColorCurves {
        semantic_version: ColorCurvesSemanticVersion::V1,
        curves: ColorCurves {
            master: fit(tables[0])?,
            red: fit(tables[1])?,
            green: fit(tables[2])?,
            blue: fit(tables[3])?,
        },
    })
}

fn fit(table: &[u8]) -> Result<ColorCurve, PropertyError> {
    let mut knots = vec![0, SAMPLES - 1];
    loop {
        let mut worst = None;
        for segment in knots.windows(2) {
            let [start, end] = [segment[0], segment[1]];
            for x in start + 1..end {
                let fraction = (x - start) as f64 / (end - start) as f64;
                let predicted = f64::from(table[start])
                    + fraction * (f64::from(table[end]) - f64::from(table[start]));
                let error = (predicted - f64::from(table[x])).abs();
                if error > 1.0 && worst.is_none_or(|(_, previous)| error > previous) {
                    worst = Some((x, error));
                }
            }
        }
        let Some((x, _)) = worst else {
            break;
        };
        if knots.len() == MAX_COLOR_CURVE_POINTS {
            return Err(PropertyError::Layout(
                "Curves table exceeds 32-point fit at one-code tolerance",
            ));
        }
        let index = knots
            .binary_search(&x)
            .expect_err("fitted knot lies strictly inside a segment");
        knots.insert(index, x);
    }
    ColorCurve::new(
        knots
            .into_iter()
            .map(|x| ColorCurvePoint::new(x as f64 / 255.0, f64::from(table[x]) / 255.0))
            .collect(),
    )
    .map_err(|_| PropertyError::Layout("Curves table fit is not representable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Vec<u8> {
        let mut bytes = vec![0, 1, 0, 1];
        for _ in 0..5 {
            bytes.extend(0..=255_u8);
        }
        bytes.resize(1644, 0);
        bytes
    }

    #[test]
    fn packed_curves_guards_unknown_alpha_order_and_unfittable_tables() {
        let original = payload();
        assert!(from_bytes(&original).is_ok());
        assert!(from_bytes(&original[..1643]).is_err());
        let mut bytes = original.clone();
        bytes[0] = 1;
        assert!(from_bytes(&bytes).is_err());
        let mut bytes = original.clone();
        bytes[4 + 4 * SAMPLES] = 1;
        assert!(from_bytes(&bytes).is_err());
        let mut bytes = original.clone();
        bytes[4] = 10;
        bytes[4 + SAMPLES] = 10;
        assert!(from_bytes(&bytes).is_err());
        let mut bytes = original;
        for x in 0..SAMPLES {
            bytes[4 + x] = if x % 2 == 0 { 0 } else { 255 };
        }
        assert!(from_bytes(&bytes).is_err());
    }

    #[test]
    fn packed_curves_channel_fit_checks_every_native_sample() {
        let mut bytes = payload();
        for x in 0..SAMPLES {
            bytes[4 + SAMPLES + x] = ((x as f64 / 255.0).powi(2) * 255.0).round() as u8;
        }
        let LayerEffect::ColorCurves { curves, .. } = from_bytes(&bytes).unwrap() else {
            panic!("ColorCurves")
        };
        assert!(
            curves.master.is_identity() && curves.green.is_identity() && curves.blue.is_identity()
        );
        for x in 0..SAMPLES {
            let input = x as f64 / 255.0;
            let pair = curves
                .red
                .points()
                .windows(2)
                .find(|p| input >= p[0].x() && input <= p[1].x())
                .unwrap();
            let fraction = (input - pair[0].x()) / (pair[1].x() - pair[0].x());
            let output = pair[0].y() + fraction * (pair[1].y() - pair[0].y());
            assert!((output * 255.0 - f64::from(bytes[4 + SAMPLES + x])).abs() <= 1.000001);
        }
    }
}
