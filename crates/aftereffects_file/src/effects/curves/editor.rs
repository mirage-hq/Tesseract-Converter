//! Curves editor control points, stored per channel after the five tables.
//!
//! Layout pinned by the AE 26.5 colour catalog (default diagonal) and the
//! UI-authored `color_ae_curves.aep` (one dragged RGB point): each 72-byte
//! block holds 16 point slots of big-endian `u16` (input, output) code pairs
//! in 0..=255, then a `u32` point count and an `i32` selected-point index
//! (`-1` when no point is selected). The authored table equals the
//! half-up-rounded natural cubic spline through the points in code space
//! (max error 0 codes on the UI-authored fixture).

pub(super) const BLOCK: usize = 72;
pub(super) const MAX_POINTS: usize = 16;
const COUNT_AT: usize = 64;
const SELECTED_AT: usize = 68;
const MAX_CODE: u16 = 255;

/// One editor point as (input, output) eight-bit codes.
pub(super) type Point = [u16; 2];

/// The diagonal AE writes for an untouched channel.
pub(super) const DIAGONAL: [Point; 2] = [[0, 0], [MAX_CODE, MAX_CODE]];

/// Decode one channel's editor points; `Err` names the unreadable layout.
pub(super) fn decode(block: &[u8]) -> Result<Vec<Point>, &'static str> {
    let block: &[u8; BLOCK] = block.try_into().map_err(|_| "editor block size")?;
    let word = |at: usize| u32::from_be_bytes(block[at..at + 4].try_into().expect("4 bytes"));
    let count = usize::try_from(word(COUNT_AT)).map_err(|_| "editor point count")?;
    if !(2..=MAX_POINTS).contains(&count) {
        return Err("editor point count outside 2..=16");
    }
    let points: Vec<Point> = block[..4 * count]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pair| {
            [
                u16::from_be_bytes([pair[0], pair[1]]),
                u16::from_be_bytes([pair[2], pair[3]]),
            ]
        })
        .collect();
    if points.iter().flatten().any(|code| *code > MAX_CODE) {
        return Err("editor point code above 255");
    }
    if points.windows(2).any(|pair| pair[0][0] >= pair[1][0]) {
        return Err("editor point inputs not strictly increasing");
    }
    Ok(points)
}

/// Encode `points` (2..=16, strictly increasing inputs, codes <= 255) with no
/// selected point, as AE saves an untouched channel.
pub(super) fn encode(points: &[Point]) -> [u8; BLOCK] {
    debug_assert!((2..=MAX_POINTS).contains(&points.len()));
    let mut block = [0; BLOCK];
    for (slot, point) in block.as_chunks_mut::<4>().0.iter_mut().zip(points) {
        slot[..2].copy_from_slice(&point[0].to_be_bytes());
        slot[2..].copy_from_slice(&point[1].to_be_bytes());
    }
    let count = u32::try_from(points.len()).expect("at most 16 points");
    block[COUNT_AT..COUNT_AT + 4].copy_from_slice(&count.to_be_bytes());
    block[SELECTED_AT..SELECTED_AT + 4].copy_from_slice(&(-1_i32).to_be_bytes());
    block
}

/// AE's table for `points`: the natural cubic spline through them in code
/// space, held constant outside the first/last input, rounded half up and
/// clamped to 0..=255. Only end points at inputs 0 and 255 are pinned natively.
pub(super) fn spline_table(points: &[Point]) -> [u8; 256] {
    let xs: Vec<f64> = points.iter().map(|point| f64::from(point[0])).collect();
    let ys: Vec<f64> = points.iter().map(|point| f64::from(point[1])).collect();
    let n = points.len();
    let h: Vec<f64> = xs.windows(2).map(|pair| pair[1] - pair[0]).collect();
    // Second derivatives with natural (zero) end conditions: Thomas algorithm.
    let mut diagonal = vec![1.0; n];
    let mut rhs = vec![0.0; n];
    let mut upper = vec![0.0; n];
    for i in 1..n - 1 {
        let lower = h[i - 1];
        diagonal[i] = 2.0 * (h[i - 1] + h[i]);
        upper[i] = h[i];
        rhs[i] = 6.0 * ((ys[i + 1] - ys[i]) / h[i] - (ys[i] - ys[i - 1]) / h[i - 1]);
        let w = lower / diagonal[i - 1];
        diagonal[i] -= w * upper[i - 1];
        rhs[i] -= w * rhs[i - 1];
    }
    let mut second = vec![0.0; n];
    for i in (1..n - 1).rev() {
        second[i] = (rhs[i] - upper[i] * second[i + 1]) / diagonal[i];
    }
    std::array::from_fn(|code| {
        let x = code as f64;
        let value = if x <= xs[0] {
            ys[0]
        } else if x >= xs[n - 1] {
            ys[n - 1]
        } else {
            let i = xs.windows(2).position(|pair| x < pair[1]).expect("inside");
            let (t, u, span) = (x - xs[i], xs[i + 1] - x, h[i]);
            (second[i] * u.powi(3) + second[i + 1] * t.powi(3)) / (6.0 * span)
                + (ys[i] / span - second[i] * span / 6.0) * u
                + (ys[i + 1] / span - second[i + 1] * span / 6.0) * t
        };
        (value + 0.5).floor().clamp(0.0, 255.0) as u8
    })
}

/// Largest absolute code difference between two tables.
pub(super) fn max_deviation(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

/// `(0,0) (127,162) (255,255)`.
pub(super) fn describe(points: &[Point]) -> String {
    points
        .iter()
        .map(|[x, y]| format!("({x},{y})"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_block_round_trips_and_rejects_malformed_layouts() {
        let points = [[0, 0], [127, 162], [255, 255]];
        let block = encode(&points);
        assert_eq!(decode(&block).unwrap(), points);
        assert_eq!(decode(&encode(&DIAGONAL)).unwrap(), DIAGONAL);
        assert!(decode(&[0; BLOCK]).is_err(), "count 0");
        assert!(decode(&block[..BLOCK - 1]).is_err());
        let mut bad = block;
        bad[4..6].copy_from_slice(&0_u16.to_be_bytes());
        assert!(decode(&bad).is_err(), "non-increasing inputs");
        let mut bad = block;
        bad[6..8].copy_from_slice(&256_u16.to_be_bytes());
        assert!(decode(&bad).is_err(), "code above 255");
        let mut bad = block;
        bad[COUNT_AT + 3] = 17;
        assert!(decode(&bad).is_err(), "more than 16 points");
    }

    #[test]
    fn diagonal_spline_is_the_identity_table() {
        let table = spline_table(&DIAGONAL);
        assert!(table.iter().enumerate().all(|(x, y)| usize::from(*y) == x));
    }
}
