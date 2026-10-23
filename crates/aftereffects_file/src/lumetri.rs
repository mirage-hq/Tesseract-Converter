//! Lumetri Color arbitrary-data values shared by Premiere Pro and After Effects.
//!
//! Both hosts run the same Lumetri Color plug-in and save its curves and
//! colour wheels in the same layout under the same `ParameterID`s (Premiere
//! `ParameterID` N is After Effects `ADBE Lumetri-00NN`). Premiere 26.5.2
//! renders measured the conversions in [`rgb_curves`] and [`wheels`]; After
//! Effects 26.5 renders the same RGB Curves payload within 1.16 mean 8-bit
//! codes of Premiere.

pub mod hue_curves;
pub mod luts;
pub mod rgb_curves;
pub mod vignette;
pub mod wheels;

/// Rec. 709 luma weights, which Lumetri's chroma operations keep constant.
const REC709: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// Byte length of one saved Lumetri curve ([`LumetriCurve`]).
pub const CURVE_BYTES: usize = 520;
/// Most points one saved Lumetri curve holds.
const CURVE_POINTS: usize = 32;

/// `ParameterID`s of the Hue Saturation Curves: Hue vs Sat, Hue vs Hue, Hue
/// vs Luma, Luma vs Sat and Sat vs Sat (Premiere 22/23 saves only the first).
pub const HUE_CURVE_IDS: [usize; 5] = [42, 106, 108, 110, 112];

/// One curve of a Lumetri arbitrary-data value, as saved by Premiere 23 and
/// 26.5.2: 520 bytes holding a little-endian `u32` curve kind, a `u32` point
/// count, then that many little-endian `(f64 x, f64 y)` points, zero padded
/// to [`CURVE_POINTS`] slots. The saved defaults hold the RGB curve
/// diagonal `(0, 0), (1, 1)` with kind 0 and the hue curves with no points
/// (kinds 1, 2 and 3 in the saves seen); a curve without points changes no
/// picture. Slots beyond the point count are not interpreted: Premiere 23
/// leaves the diagonal in the first two slots of its pointless Hue vs Sat
/// default. The kind of a hue curve is not interpreted.
#[derive(Debug, Clone, PartialEq)]
pub struct LumetriCurve {
    pub kind: u32,
    pub points: Vec<[f64; 2]>,
}

impl LumetriCurve {
    /// The curve saved in `bytes`, or none when they are not one curve: a
    /// wrong length, more than 32 points or a non-finite coordinate.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CURVE_BYTES {
            return None;
        }
        let word = |at: usize| {
            bytes
                .get(at..at + 4)?
                .try_into()
                .ok()
                .map(u32::from_le_bytes)
        };
        let float = |at: usize| {
            bytes
                .get(at..at + 8)?
                .try_into()
                .ok()
                .map(f64::from_le_bytes)
        };
        let kind = word(0)?;
        let count = usize::try_from(word(4)?).ok()?;
        if count > CURVE_POINTS {
            return None;
        }
        let points = (0..count)
            .map(|index| {
                let at = 8 + 16 * index;
                Some([float(at)?, float(at + 8)?])
                    .filter(|point| point.iter().all(|c| c.is_finite()))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { kind, points })
    }

    /// The saved bytes of this curve, the inverse of [`Self::decode`]; none
    /// when it holds more than [`CURVE_POINTS`] points.
    pub fn encode(&self) -> Option<Vec<u8>> {
        if self.points.len() > CURVE_POINTS {
            return None;
        }
        let mut bytes = Vec::with_capacity(CURVE_BYTES);
        bytes.extend(self.kind.to_le_bytes());
        bytes.extend(u32::try_from(self.points.len()).ok()?.to_le_bytes());
        for [x, y] in &self.points {
            bytes.extend(x.to_le_bytes());
            bytes.extend(y.to_le_bytes());
        }
        bytes.resize(CURVE_BYTES, 0);
        Some(bytes)
    }
}

/// `ParameterID` of Lumetri RGB Curves: four [`LumetriCurve`]s of kind 0.
pub const RGB_CURVES_ID: usize = 39;

/// Lumetri RGB Curves: the saved points of the Luma (master), Red, Green and
/// Blue curves, in saved order, each with at least two points, its first at
/// x 0 and its last at x 1, x strictly increasing and every coordinate in
/// 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub struct RgbCurves {
    pub curves: [Vec<[f64; 2]>; 4],
}

impl RgbCurves {
    /// The curves saved in the RGB Curves value `bytes`, or why they are not
    /// representable.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 4 * CURVE_BYTES {
            return Err(format!(
                "RGB Curves value has {} bytes, not 2080",
                bytes.len()
            ));
        }
        let mut curves: [Vec<[f64; 2]>; 4] = Default::default();
        for (index, (chunk, curve)) in bytes.chunks(CURVE_BYTES).zip(curves.iter_mut()).enumerate()
        {
            let saved = LumetriCurve::decode(chunk)
                .ok_or_else(|| format!("RGB curve {index} is not a saved curve"))?;
            let points = saved.points;
            let valid = saved.kind == 0
                && points.len() >= 2
                && points.first().is_some_and(|point| point[0] == 0.0)
                && points.last().is_some_and(|point| point[0] == 1.0)
                && points
                    .iter()
                    .all(|point| point.iter().all(|c| (0.0..=1.0).contains(c)))
                && points.windows(2).all(|pair| pair[0][0] < pair[1][0]);
            if !valid {
                return Err(format!(
                    "RGB curve {index} is not a kind-0 curve from x 0 to x 1 with increasing x and coordinates in 0 to 1"
                ));
            }
            *curve = points;
        }
        Ok(Self { curves })
    }

    /// Whether every curve is the diagonal.
    pub fn is_identity(&self) -> bool {
        self.curves
            .iter()
            .all(|curve| curve.iter().all(|point| point[0] == point[1]))
    }

    /// The saved RGB Curves value: four kind-0 curves ([`LumetriCurve::encode`]);
    /// none when a curve has more than 32 points.
    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut bytes = Vec::with_capacity(4 * CURVE_BYTES);
        for points in &self.curves {
            let curve = LumetriCurve {
                kind: 0,
                points: points.clone(),
            };
            bytes.extend(curve.encode()?);
        }
        Some(bytes)
    }
}

/// `ParameterID` of the Lumetri Color Wheels value ([`Wheels`]).
pub const COLOR_WHEELS_ID: usize = 47;

/// Lumetri Color Wheels as Premiere 23 and 26.5.2 save them: 96 bytes of
/// four little-endian `(f64 hue in degrees, f64 luma slider, f64 amount)`
/// triples for the Shadows, Midtones and Highlights wheels and a fourth
/// wheel, neutral at `(0, 0.5, 0)`. Hue 0 pushes red and 120 green; a
/// default (three-wheel) Lumetri renders the fourth triple unchanged
/// (Premiere 26.5.2 probe renders), so it is kept but never interpreted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wheels {
    pub wheels: [[f64; 3]; 4],
}

impl Wheels {
    /// The wheels saved in `bytes`, or why they are not representable: a
    /// wrong length, a non-finite value, or a luma slider or amount
    /// outside 0 to 1.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 96 {
            return Err(format!(
                "Color Wheels value has {} bytes, not 96",
                bytes.len()
            ));
        }
        let mut wheels = [[0.0; 3]; 4];
        for (index, chunk) in bytes.as_chunks::<8>().0.iter().enumerate() {
            let value = f64::from_le_bytes(*chunk);
            let valid = value.is_finite() && (index % 3 == 0 || (0.0..=1.0).contains(&value));
            if !valid {
                return Err(format!(
                    "Color Wheels value {index} ({value}) is not a finite hue or a luma/amount in 0 to 1"
                ));
            }
            wheels[index / 3][index % 3] = value;
        }
        Ok(Self { wheels })
    }

    /// Whether the Shadows, Midtones and Highlights wheels change no picture.
    pub fn is_neutral(&self) -> bool {
        self.wheels[..3]
            .iter()
            .all(|[_, luma, amount]| *luma == 0.5 && *amount == 0.0)
    }

    /// The saved Color Wheels value, the inverse of [`Self::decode`].
    pub fn encode(&self) -> Vec<u8> {
        self.wheels
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }
}
