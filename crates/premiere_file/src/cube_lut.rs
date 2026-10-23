//! 3D colour lookup tables in the two forms the Premiere converter meets:
//! Adobe `.cube` text (FX `lut3d` assets) and Premiere's embedded `ITX3`
//! float table.
//!
//! Premiere 26.5.2 embeds the samples of a clip Interpret Footage Input LUT
//! in its `LUTAsset` record (`color_clip_input_lut.prproj`) as base64 of:
//! `ITX3`, `u32` edge size, `u32` channel count (3), `u32` 0, `f32` ×3 domain
//! minimum, `f32` ×3 domain maximum, then `size³` RGB `f32` triples in
//! `.cube` row order (red fastest), all little-endian. The saved table of
//! `color_warm_tint.cube` equals the file's rows to `f32` precision. Only
//! this layout is admitted; any other header is rejected, never guessed.

/// How Premiere applies a 3D LUT that FX does not, stated in every LUT
/// diagnostic. Measured in Premiere 26.5.2 with the in-house channel-mixing
/// 17³ LUT (local evidence, not committed) over an in-house RGB sweep, against float
/// trilinear and tetrahedral predictions: with the project's LUT
/// Interpolation (`lutInterpolationMethod`) at 1, Premiere's default and the
/// value this converter writes, a Lumetri Look renders tetrahedrally (least-
/// squares position 0.93 between the predictions, 0 = trilinear); with 0 it
/// renders trilinearly (0.02). An FX `lut3d` is trilinear (the only v1
/// interpolation), so the two agree exactly only for tables that are affine
/// within each lattice cell, such as per-channel affine looks.
pub(crate) const PREMIERE_LUT_INTERPOLATION: &str = "Premiere interpolates the LUT tetrahedrally at the project LUT Interpolation it has by default and that export writes (lutInterpolationMethod 1; 0 is trilinear) while FX lut3d is trilinear: identical for LUTs affine within each lattice cell (such as per-channel affine looks); on an in-house channel-mixing 17-point LUT over an RGB sweep the two interpolations differ by up to 13 8-bit codes (mean 0.70)";

const ITX3_MAGIC: &[u8; 4] = b"ITX3";
const ITX3_HEADER_BYTES: usize = 40;
/// The largest edge either form admits: `.cube` caps `LUT_3D_SIZE` at 256.
const MAX_EDGE: usize = 256;

/// One RGB 3D LUT: `size³` output triples in `.cube` order (red fastest).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lut3d {
    size: usize,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    samples: Vec<[f32; 3]>,
}

// Construction admits only finite values, so equality is reflexive.
impl Eq for Lut3d {}

impl Lut3d {
    /// Decode Premiere's embedded `ITX3` table.
    pub(crate) fn from_itx3(bytes: &[u8]) -> Result<Self, String> {
        let word = |index: usize| -> [u8; 4] {
            let start = index * 4;
            bytes[start..start + 4]
                .try_into()
                .expect("four bytes were sliced")
        };
        if bytes.len() < ITX3_HEADER_BYTES || &word(0) != ITX3_MAGIC {
            return Err("the embedded LUT is not an ITX3 table".to_owned());
        }
        let size = u32::from_le_bytes(word(1)) as usize;
        let channels = u32::from_le_bytes(word(2));
        let reserved = u32::from_le_bytes(word(3));
        if channels != 3 || reserved != 0 {
            return Err(format!(
                "the embedded ITX3 table declares {channels} channels and header word {reserved}; only the pinned 3-channel layout (word 0) is known"
            ));
        }
        if !(2..=MAX_EDGE).contains(&size) {
            return Err(format!(
                "the embedded ITX3 table edge {size} is outside 2..={MAX_EDGE}"
            ));
        }
        let float = |index: usize| f32::from_le_bytes(word(index));
        let domain_min = [float(4), float(5), float(6)];
        let domain_max = [float(7), float(8), float(9)];
        let expected = ITX3_HEADER_BYTES + size.pow(3) * 12;
        if bytes.len() != expected {
            return Err(format!(
                "the embedded ITX3 table holds {} bytes, but edge {size} needs {expected}",
                bytes.len()
            ));
        }
        let (triples, _) = bytes[ITX3_HEADER_BYTES..].as_chunks::<12>();
        let samples = triples
            .iter()
            .map(|triple| {
                let channel = |offset: usize| {
                    f32::from_le_bytes(
                        triple[offset..offset + 4]
                            .try_into()
                            .expect("four bytes were sliced"),
                    )
                };
                [channel(0), channel(4), channel(8)]
            })
            .collect();
        Self::new(size, domain_min, domain_max, samples)
    }

    /// Parse Adobe `.cube` text with a `LUT_3D_SIZE`. 1D tables are rejected.
    pub(crate) fn from_cube(text: &str) -> Result<Self, String> {
        let mut size = None;
        let mut domain_min = [0.0; 3];
        let mut domain_max = [1.0; 3];
        let mut samples = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            let mut words = line.split_whitespace();
            let Some(first) = words.next() else {
                continue;
            };
            let triple = |words: std::str::SplitWhitespace<'_>| -> Result<[f32; 3], String> {
                let values = words
                    .map(|word| {
                        word.parse::<f32>()
                            .ok()
                            .filter(|value| value.is_finite())
                            .ok_or_else(|| {
                                format!("the .cube value {word:?} is not a finite number")
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                <[f32; 3]>::try_from(values)
                    .map_err(|_| format!("the .cube line {line:?} is not an RGB triple"))
            };
            match first {
                _ if first.starts_with('#') => {}
                "TITLE" => {}
                "LUT_1D_SIZE" => return Err("1D .cube LUTs are not 3D LUTs".to_owned()),
                "LUT_3D_SIZE" => {
                    let edge = words
                        .next()
                        .and_then(|word| word.parse::<usize>().ok())
                        .ok_or_else(|| format!("invalid .cube line {line:?}"))?;
                    size = Some(edge);
                }
                "DOMAIN_MIN" => domain_min = triple(words)?,
                "DOMAIN_MAX" => domain_max = triple(words)?,
                _ if first.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') => {
                    samples.push(triple(line.split_whitespace())?);
                }
                _ => return Err(format!("unknown .cube keyword {first:?}")),
            }
        }
        let size = size.ok_or_else(|| "the .cube file has no LUT_3D_SIZE".to_owned())?;
        Self::new(size, domain_min, domain_max, samples)
    }

    fn new(
        size: usize,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
        samples: Vec<[f32; 3]>,
    ) -> Result<Self, String> {
        if !(2..=MAX_EDGE).contains(&size) {
            return Err(format!("LUT edge {size} is outside 2..={MAX_EDGE}"));
        }
        if samples.len() != size.pow(3) {
            return Err(format!(
                "LUT edge {size} needs {} samples, found {}",
                size.pow(3),
                samples.len()
            ));
        }
        let finite = |values: &[f32]| values.iter().all(|value| value.is_finite());
        if !finite(&domain_min)
            || !finite(&domain_max)
            || (0..3).any(|channel| domain_min[channel] >= domain_max[channel])
            || !samples.iter().all(|sample| finite(sample))
        {
            return Err("LUT domain or samples are not finite and increasing".to_owned());
        }
        Ok(Self {
            size,
            domain_min,
            domain_max,
            samples,
        })
    }

    /// Premiere's embedded `ITX3` bytes of this table.
    pub(crate) fn to_itx3(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(ITX3_HEADER_BYTES + self.samples.len() * 12);
        bytes.extend_from_slice(ITX3_MAGIC);
        let edge = u32::try_from(self.size).expect("the edge is at most 256");
        for word in [edge, 3, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for value in self
            .domain_min
            .iter()
            .chain(&self.domain_max)
            .chain(self.samples.iter().flatten())
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// `.cube` text of this table. Each value is the shortest decimal that
    /// reads back as the same `f32`, so the text reproduces the table exactly.
    pub(crate) fn to_cube(&self, title: &str) -> String {
        let title: String = title
            .chars()
            .filter(|c| *c != '"' && !c.is_control())
            .collect();
        let triple = |values: &[f32; 3]| format!("{} {} {}", values[0], values[1], values[2]);
        let mut text = format!(
            "TITLE \"{title}\"\nLUT_3D_SIZE {}\nDOMAIN_MIN {}\nDOMAIN_MAX {}\n",
            self.size,
            triple(&self.domain_min),
            triple(&self.domain_max),
        );
        for sample in &self.samples {
            text.push_str(&triple(sample));
            text.push('\n');
        }
        text
    }
}

/// The `.cube` assets that Premiere export read, by FX asset ID: the table,
/// or why the asset cannot be written as a clip Input LUT.
pub(crate) type InputLutAssets = std::collections::BTreeMap<String, Result<PackagedLut, String>>;

/// One readable `.cube` asset of a Tesseract archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackagedLut {
    /// The packaged file's name, which Premiere shows and saves.
    pub(crate) file_name: String,
    pub(crate) table: std::sync::Arc<Lut3d>,
}

impl PackagedLut {
    /// Parse a packaged `.cube` asset named `file_name`.
    pub(crate) fn parse(file_name: &str, bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "is not UTF-8 .cube text".to_owned())?;
        let table =
            Lut3d::from_cube(text).map_err(|reason| format!("is not a 3D .cube LUT ({reason})"))?;
        Ok(Self {
            file_name: file_name.to_owned(),
            table: std::sync::Arc::new(table),
        })
    }

    /// The clip Input LUT that writes this table. Its registry GUID is
    /// derived from the samples, so one table keeps one GUID across exports.
    pub(crate) fn clip_input_lut(&self) -> crate::schema::PrClipInputLut {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(self.table.to_itx3());
        let mut bytes: [u8; 16] = digest[..16]
            .try_into()
            .expect("a SHA-256 digest has 32 bytes");
        // RFC 4122 version 8 (custom) and variant bits.
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let display_name = self
            .file_name
            .strip_suffix(".cube")
            .or_else(|| self.file_name.strip_suffix(".CUBE"))
            .unwrap_or(&self.file_name)
            .to_owned();
        crate::schema::PrClipInputLut {
            id: uuid::Uuid::from_bytes(bytes).to_string(),
            display_name,
            file_name: self.file_name.clone(),
            input_encoding: None,
            table: self.table.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Lut3d;

    fn identity(size: usize) -> Lut3d {
        let step = |index: usize| index as f32 / (size - 1) as f32;
        let samples = (0..size.pow(3))
            .map(|index| {
                [
                    step(index % size),
                    step(index / size % size),
                    step(index / size / size),
                ]
            })
            .collect();
        Lut3d::new(size, [0.0; 3], [1.0; 3], samples).unwrap()
    }

    #[test]
    fn itx3_and_cube_forms_round_trip_the_same_table() {
        let mut lut = identity(3);
        lut.samples[1] = [0.1, 0.0, 0.33333334];
        let itx3 = lut.to_itx3();
        assert_eq!(&itx3[..4], b"ITX3");
        assert_eq!(itx3.len(), 40 + 27 * 12);
        assert_eq!(Lut3d::from_itx3(&itx3).unwrap(), lut);
        let cube = lut.to_cube("warm \"tint\"");
        assert!(cube.starts_with("TITLE \"warm tint\"\nLUT_3D_SIZE 3\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n0 0 0\n0.1 0 0.33333334\n"));
        assert_eq!(Lut3d::from_cube(&cube).unwrap(), lut);
    }

    #[test]
    fn channel_mixing_lut_round_trips_through_itx3_exactly() {
        let size: usize = 5;
        let step = |index: usize| index as f32 / (size - 1) as f32;
        let samples = (0..size.pow(3))
            .map(|index| {
                let (r, g, b) = (
                    step(index % size),
                    step(index / size % size),
                    step(index / size / size),
                );
                [g * b, r, ((r + g + b) / 3.0).powi(2)]
            })
            .collect();
        let lut = Lut3d::new(size, [0.0; 3], [1.0; 3], samples).unwrap();
        let itx3 = lut.to_itx3();
        assert_eq!(itx3.len(), 40 + size.pow(3) * 12);
        let back = Lut3d::from_itx3(&itx3).unwrap();
        assert_eq!(back, lut);
        assert_eq!(Lut3d::from_cube(&back.to_cube("mix")).unwrap(), lut);
    }

    #[test]
    fn unknown_itx3_layouts_and_malformed_cubes_are_rejected() {
        let mut itx3 = identity(2).to_itx3();
        itx3[8] = 1;
        assert!(Lut3d::from_itx3(&itx3).unwrap_err().contains("1 channels"));
        let mut short = identity(2).to_itx3();
        short.pop();
        assert!(Lut3d::from_itx3(&short).unwrap_err().contains("needs"));
        assert!(Lut3d::from_itx3(b"ITX1").is_err());
        assert!(Lut3d::from_cube("LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").is_err());
        assert!(Lut3d::from_cube("LUT_3D_SIZE 2\n0 0 0\n")
            .unwrap_err()
            .contains("needs 8"));
        assert!(Lut3d::from_cube("0 0 0\n")
            .unwrap_err()
            .contains("LUT_3D_SIZE"));
        assert!(Lut3d::from_cube("LUT_3D_SIZE 2\nDOMAIN_MIN 1 1 1\nDOMAIN_MAX 0 0 0\n").is_err());
    }
}
