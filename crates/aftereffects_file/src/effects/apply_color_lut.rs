//! AE Apply Color LUT (`ADBE Apply Color LUT2`) ↔ FX `lookTransform` 3D LUTs.
//!
//! AE does not save the chosen `.cube` file or its path. When the LUT is
//! picked, AE converts it to an ICC v4 device-link profile (class `link`,
//! RGB → RGB) and stores that profile as the instance's sequence data
//! (`sspc/sdat`) behind a 28-byte header. The profile's only transform is an
//! A2B0 `mAB ` pipeline of identity A/M/B curves, an identity matrix and the
//! LUT as a 16-bit CLUT whose first input (red) varies slowest. Only the file
//! name survives: in the profile `desc` and in the instance's `-0001` group
//! label `Using: <name>`. Pinned by the human-authored AE 26.5 fixture
//! `tests/fixtures/color_ae_apply_color_lut.aep` (17³ LUT; the CLUT equals the
//! `.cube` samples within 16-bit quantization).
//!
//! Import decodes the embedded CLUT back into a `.cube` LUT asset; export
//! encodes an FX `.cube` asset into the same profile layout.

use fx_schema::{
    AssetId, ColorEncodingId, ColorTransform, ColorTransformSemanticVersion, LayerEffect,
    NormalizedColorMix, SDR_REC709_DISPLAY_ENCODING_ID,
};
use sha2::{Digest, Sha256};

use crate::{properties, rifx::Chunk};

/// How AE applies the LUT that FX does not, stated in every Apply Color LUT
/// diagnostic. Measured in AE 26.5x89 with the in-house channel-mixing
/// 17³ LUT (local evidence, not committed) over an in-house RGB sweep, against float
/// trilinear and tetrahedral predictions: with the project LUT Interpolation
/// Method (`lutInterpolationMethod`) 1, AE's default and the value the
/// writer saves, Apply Color LUT renders tetrahedrally (least-squares
/// position 0.99 between the predictions, 0 = trilinear). An FX `lut3d` is
/// trilinear (the only v1 interpolation), so the two agree exactly only for
/// tables that are affine within each lattice cell, such as per-channel
/// affine looks.
pub(crate) const LUT_INTERPOLATION: &str = "AE interpolates the LUT tetrahedrally at the project LUT Interpolation Method it has by default and that export writes (lutInterpolationMethod 1) while FX lut3d is trilinear: identical for LUTs affine within each lattice cell (such as per-channel affine looks); on an in-house channel-mixing 17-point LUT over an RGB sweep the two interpolations differ by up to 13 8-bit codes (mean 0.70)";

pub(crate) const MATCH_NAME: &str = "ADBE Apply Color LUT2";
/// The group-start control whose declared label names the applied file.
pub(crate) const FILE_LABEL_CONTROL: &str = "ADBE Apply Color LUT2-0001";
/// `pard` labels are NUL-terminated in 32 bytes.
const PARD_LABEL_BYTES: usize = 31;

/// Sequence-data header before the ICC profile: a constant prefix, the
/// profile length, eight zero bytes (as AE 26.5 saves it).
const HEADER_PREFIX: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
const HEADER_LEN: usize = 28;
const PROFILE_HEADER_LEN: usize = 128;
/// ICC D50 PCS illuminant (s15Fixed16 X, Y, Z).
const D50: [u8; 12] = [0, 0, 0xf6, 0xd6, 0, 1, 0, 0, 0, 0, 0xd3, 0x2d];
/// Largest grid an ICC CLUT dimension (one byte) can declare.
const MAX_GRID: usize = 255;
const MIN_GRID: usize = 2;

/// A 3D LUT in `.cube` sample order: red varies fastest, then green, then blue.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lut3d {
    pub size: usize,
    pub samples: Vec<[f64; 3]>,
}

/// The LUT embedded in one native Apply Color LUT instance.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EmbeddedLut {
    /// The applied file's name as AE recorded it (profile `desc`).
    pub file_name: String,
    pub lut: Lut3d,
}

impl EmbeddedLut {
    /// The `.cube` text packaged for FX.
    pub(crate) fn cube_text(&self) -> String {
        self.lut.to_cube_text(cube_title(&self.file_name))
    }

    /// A content-addressed archive asset: equal LUTs share one asset.
    pub(crate) fn asset_id(&self) -> AssetId {
        let digest = Sha256::digest(self.cube_text().as_bytes());
        let hex: String = digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        AssetId::from_trusted(format!("aep-lut-{hex}"))
    }

    /// The archive file name of the packaged asset: the recorded name when it
    /// is a plain `.cube` file name, else a generic one.
    pub(crate) fn package_file_name(&self) -> String {
        safe_cube_file_name(&self.file_name)
    }
}

/// A plain `<name>.cube` file name for `name`, or `apply-color-lut.cube`.
pub(crate) fn safe_cube_file_name(name: &str) -> String {
    let plain = !name.is_empty()
        && name.len() <= 200
        && name.to_ascii_lowercase().ends_with(".cube")
        && name.len() > ".cube".len()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '));
    if plain {
        name.to_owned()
    } else {
        "apply-color-lut.cube".to_owned()
    }
}

fn cube_title(file_name: &str) -> &str {
    let lower = file_name.to_ascii_lowercase();
    match lower.strip_suffix(".cube") {
        Some(stem) if !stem.is_empty() => &file_name[..stem.len()],
        _ => file_name,
    }
}

impl Lut3d {
    /// Parse a 3D `.cube` (Adobe/Resolve text). Only a unit domain is
    /// accepted, because the native profile maps the 0..1 code range.
    pub(crate) fn parse_cube(text: &str) -> Result<Self, String> {
        let mut size = None;
        let mut samples = Vec::new();
        for (number, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let mut words = line.split_whitespace();
            let keyword = words.next().unwrap_or_default();
            match keyword {
                "TITLE" => {}
                "LUT_3D_SIZE" => {
                    let value = words
                        .next()
                        .and_then(|word| word.parse::<usize>().ok())
                        .filter(|value| (MIN_GRID..=MAX_GRID).contains(value))
                        .ok_or_else(|| {
                            format!(
                                "line {}: LUT_3D_SIZE must be {MIN_GRID}..={MAX_GRID}",
                                number + 1
                            )
                        })?;
                    if size.replace(value).is_some() {
                        return Err("duplicate LUT_3D_SIZE".into());
                    }
                }
                "LUT_1D_SIZE" | "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {
                    return Err(format!(
                        "{keyword} is not supported; only 3D LUTs on the unit domain map to Apply Color LUT"
                    ));
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let expected = if keyword == "DOMAIN_MIN" { 0.0 } else { 1.0 };
                    let values: Vec<f64> = words.filter_map(|word| word.parse().ok()).collect();
                    if values.len() != 3 || values.iter().any(|value| *value != expected) {
                        return Err(format!(
                            "{keyword} {values:?} is not the unit domain; the native profile maps only 0..1"
                        ));
                    }
                }
                _ => {
                    let values: Vec<f64> = line
                        .split_whitespace()
                        .map(str::parse)
                        .collect::<Result<_, _>>()
                        .map_err(|_| {
                            format!("line {}: unknown .cube keyword {keyword:?}", number + 1)
                        })?;
                    let [red, green, blue] = values.as_slice() else {
                        return Err(format!("line {}: a sample needs three values", number + 1));
                    };
                    if ![red, green, blue].iter().all(|value| value.is_finite()) {
                        return Err(format!("line {}: non-finite sample", number + 1));
                    }
                    samples.push([*red, *green, *blue]);
                }
            }
        }
        let size = size.ok_or("no LUT_3D_SIZE")?;
        if samples.len() != size.pow(3) {
            return Err(format!(
                "{} samples for LUT_3D_SIZE {size} (expected {})",
                samples.len(),
                size.pow(3)
            ));
        }
        Ok(Self { size, samples })
    }

    /// `.cube` text with six decimals (the 16-bit native samples need five).
    pub(crate) fn to_cube_text(&self, title: &str) -> String {
        let title: String = title
            .chars()
            .filter(|c| !c.is_control() && *c != '"')
            .collect();
        let mut text = format!(
            "TITLE \"{title}\"\nLUT_3D_SIZE {}\nDOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0\n",
            self.size
        );
        for [red, green, blue] in &self.samples {
            text.push_str(&format!("{red:.6} {green:.6} {blue:.6}\n"));
        }
        text
    }
}

/// The FX effect for a decoded native LUT packaged as `asset_id`.
pub(crate) fn look_transform(asset_id: AssetId) -> LayerEffect {
    let display =
        ColorEncodingId::new(SDR_REC709_DISPLAY_ENCODING_ID).expect("canonical SDR encoding id");
    LayerEffect::LookTransform {
        semantic_version: ColorTransformSemanticVersion::V1,
        transform: Some(ColorTransform::lut3d(asset_id, display.clone(), display)),
        mix: NormalizedColorMix::FULL,
    }
}

/// The `Using: <name>` label AE gives the file group, truncated to the
/// `pard` label field at a character boundary.
pub(crate) fn file_label(file_name: &str) -> String {
    let mut label = format!("Using: {file_name}");
    while label.len() > PARD_LABEL_BYTES {
        label.pop();
    }
    label
}

/// Read the sequence data of the `ordinal`th (one-based) Effect Parade
/// instance, which must be Apply Color LUT.
pub(crate) fn instance_sequence_data(content: &[Chunk], ordinal: usize) -> Result<&[u8], String> {
    let roots = properties::root_runs(content).map_err(|error| error.to_string())?;
    let parades: Vec<_> = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade")
        .collect();
    let [(_, parade)] = parades.as_slice() else {
        return Err("ambiguous Effect Parade".into());
    };
    let group = properties::unique_list(parade, *b"tdgp").map_err(|error| error.to_string())?;
    let instances = properties::runs(group).map_err(|error| error.to_string())?;
    let (name, instance) = ordinal
        .checked_sub(1)
        .and_then(|index| instances.get(index))
        .ok_or("missing Apply Color LUT occurrence")?;
    if *name != MATCH_NAME {
        return Err("Apply Color LUT occurrence identity mismatch".into());
    }
    let plugin = properties::unique_list(instance, *b"sspc").map_err(|error| error.to_string())?;
    let mut records = plugin.iter().filter(|chunk| chunk.id() == *b"sdat");
    let data = records
        .next()
        .and_then(Chunk::data_payload)
        .ok_or("no embedded LUT (sequence data missing): AE saved no LUT for this instance")?;
    if records.next().is_some() {
        return Err("duplicate sequence data".into());
    }
    Ok(data)
}

/// Lower one native instance: the FX effect and the LUT to package.
pub(crate) fn lower(
    content: &[Chunk],
    ordinal: usize,
) -> Result<(LayerEffect, EmbeddedLut), String> {
    let embedded = decode_sequence_data(instance_sequence_data(content, ordinal)?)?;
    Ok((look_transform(embedded.asset_id()), embedded))
}

fn be_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|word| u32::from_be_bytes(word.try_into().expect("four bytes")))
        .ok_or_else(|| format!("profile truncated at byte {at}"))
}

fn be_i32(bytes: &[u8], at: usize) -> Result<i32, String> {
    be_u32(bytes, at).map(|word| i32::from_be_bytes(word.to_be_bytes()))
}

fn range(bytes: &[u8], at: usize, len: usize) -> Result<&[u8], String> {
    at.checked_add(len)
        .and_then(|end| bytes.get(at..end))
        .ok_or_else(|| format!("profile truncated at byte {at}"))
}

/// Decode AE's sequence data into the embedded LUT. Any profile other than
/// the pinned identity-curve device link is reported, never approximated.
pub(crate) fn decode_sequence_data(bytes: &[u8]) -> Result<EmbeddedLut, String> {
    if bytes.len() < HEADER_LEN + PROFILE_HEADER_LEN || bytes[..16] != HEADER_PREFIX {
        return Err("unknown Apply Color LUT sequence-data header".into());
    }
    let declared = be_u32(bytes, 16)? as usize;
    let profile = &bytes[HEADER_LEN..];
    if declared != profile.len() || be_u32(profile, 0)? as usize != profile.len() {
        return Err("embedded profile length does not match its sequence data".into());
    }
    if &profile[12..16] != b"link" || &profile[16..20] != b"RGB " || &profile[20..24] != b"RGB " {
        return Err("embedded profile is not an RGB device link".into());
    }
    if &profile[36..40] != b"acsp" {
        return Err("embedded profile lacks the ICC signature".into());
    }
    let count = be_u32(profile, PROFILE_HEADER_LEN)? as usize;
    let mut a2b0 = None;
    let mut description = None;
    for index in 0..count.min(64) {
        let entry = range(profile, PROFILE_HEADER_LEN + 4 + 12 * index, 12)?;
        let offset = u32::from_be_bytes(entry[4..8].try_into().expect("four bytes")) as usize;
        let size = u32::from_be_bytes(entry[8..12].try_into().expect("four bytes")) as usize;
        let tag = range(profile, offset, size)?;
        match &entry[..4] {
            b"A2B0" => a2b0 = Some(tag),
            b"desc" => description = Some(tag),
            _ => {}
        }
    }
    let file_name = description
        .map(decode_mluc)
        .transpose()?
        .unwrap_or_default();
    let lut = decode_lut_a_to_b(a2b0.ok_or("embedded profile has no A2B0 transform")?)?;
    Ok(EmbeddedLut { file_name, lut })
}

fn decode_mluc(tag: &[u8]) -> Result<String, String> {
    if tag.get(..4) != Some(b"mluc") {
        return Err("profile description is not multiLocalizedUnicode".into());
    }
    if be_u32(tag, 8)? == 0 {
        return Ok(String::new());
    }
    let length = be_u32(tag, 20)? as usize;
    let offset = be_u32(tag, 24)? as usize;
    let units: Vec<u16> = range(tag, offset, length)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16(&units).map_err(|_| "profile description is not UTF-16".into())
}

/// One curve set of an `mAB ` element: must be the identity.
fn identity_curves(tag: &[u8], mut at: usize, element: &str) -> Result<(), String> {
    for _ in 0..3 {
        let kind = range(tag, at, 4)?;
        let length = match kind {
            b"curv" => {
                let count = be_u32(tag, at + 8)? as usize;
                let identity = count == 0 || (count == 1 && range(tag, at + 12, 2)? == [1, 0]);
                if !identity {
                    return Err(format!(
                        "embedded profile {element} curves are not the identity; only Apply Color LUT's identity-curve device link maps to a 3D LUT"
                    ));
                }
                12 + 2 * count
            }
            b"para" => {
                let function = u16::from_be_bytes(range(tag, at + 8, 2)?.try_into().expect("two"));
                let parameters = match function {
                    0 => 1,
                    1 => 3,
                    2 => 4,
                    3 => 5,
                    4 => 7,
                    _ => return Err(format!("unknown parametric {element} curve")),
                };
                let values: Vec<i32> = (0..parameters)
                    .map(|index| be_i32(tag, at + 12 + 4 * index))
                    .collect::<Result<_, _>>()?;
                // g = 1, a = 1, b = 0 (and c, d, e, f = 0 where present).
                let one = 0x1_0000;
                let identity = values[0] == one
                    && values.get(1).is_none_or(|a| *a == one)
                    && values.iter().skip(2).all(|value| *value == 0);
                if !identity {
                    return Err(format!(
                        "embedded profile {element} curves are not the identity; only Apply Color LUT's identity-curve device link maps to a 3D LUT"
                    ));
                }
                12 + 4 * parameters
            }
            _ => return Err(format!("unknown {element} curve type")),
        };
        at = (at + length).next_multiple_of(4);
    }
    Ok(())
}

fn decode_lut_a_to_b(tag: &[u8]) -> Result<Lut3d, String> {
    if tag.get(..4) != Some(b"mAB ") {
        return Err("embedded A2B0 is not an lutAtoB (mAB) transform".into());
    }
    if range(tag, 8, 2)? != [3, 3] {
        return Err("embedded A2B0 is not three-channel".into());
    }
    let offset = |index: usize| be_u32(tag, 12 + 4 * index).map(|value| value as usize);
    let (b_curves, matrix, m_curves, clut, a_curves) =
        (offset(0)?, offset(1)?, offset(2)?, offset(3)?, offset(4)?);
    if b_curves == 0 || clut == 0 {
        return Err("embedded A2B0 has no CLUT".into());
    }
    identity_curves(tag, b_curves, "B")?;
    if m_curves != 0 {
        identity_curves(tag, m_curves, "M")?;
    }
    if a_curves != 0 {
        identity_curves(tag, a_curves, "A")?;
    }
    if matrix != 0 {
        let identity = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0].map(|value| value * 0x1_0000);
        for (index, expected) in identity.iter().enumerate() {
            if be_i32(tag, matrix + 4 * index)? != *expected {
                return Err("embedded A2B0 matrix is not the identity".into());
            }
        }
    }
    let grid = range(tag, clut, 16)?;
    let size = usize::from(grid[0]);
    if grid[1..3] != [grid[0], grid[0]] || grid[3..].iter().any(|value| *value != 0) {
        return Err("embedded CLUT grid is not a cube".into());
    }
    if size < MIN_GRID {
        return Err("embedded CLUT grid is degenerate".into());
    }
    let precision = usize::from(range(tag, clut + 16, 1)?[0]);
    if precision != 1 && precision != 2 {
        return Err("embedded CLUT precision is not 8 or 16 bits".into());
    }
    let points = size.pow(3);
    let data = range(tag, clut + 20, points * 3 * precision)?;
    let max = if precision == 2 { 65_535.0 } else { 255.0 };
    let sample = |index: usize| -> f64 {
        let value = if precision == 2 {
            u16::from_be_bytes([data[2 * index], data[2 * index + 1]])
        } else {
            u16::from(data[index])
        };
        f64::from(value) / max
    };
    let mut samples = vec![[0.0; 3]; points];
    // ICC: the first input (red) varies slowest; .cube: red varies fastest.
    for red in 0..size {
        for green in 0..size {
            for blue in 0..size {
                let icc = ((red * size + green) * size + blue) * 3;
                samples[(blue * size + green) * size + red] =
                    [sample(icc), sample(icc + 1), sample(icc + 2)];
            }
        }
    }
    Ok(Lut3d { size, samples })
}

fn mluc(text: &str, terminated: bool) -> Vec<u8> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    if terminated {
        units.push(0);
    }
    let mut tag = b"mluc".to_vec();
    tag.extend([0; 4]);
    tag.extend(1_u32.to_be_bytes());
    tag.extend(12_u32.to_be_bytes());
    tag.extend(b"enUS");
    tag.extend(
        u32::try_from(units.len() * 2)
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    tag.extend(28_u32.to_be_bytes());
    tag.extend(units.iter().flat_map(|unit| unit.to_be_bytes()));
    tag
}

/// The empty two-entry profile sequence AE writes.
fn profile_sequence() -> Vec<u8> {
    let empty_mluc = |tag: &mut Vec<u8>| {
        tag.extend(b"mluc");
        tag.extend([0; 8]);
        tag.extend(12_u32.to_be_bytes());
    };
    let mut tag = b"pseq".to_vec();
    tag.extend([0; 4]);
    tag.extend(2_u32.to_be_bytes());
    for _ in 0..2 {
        tag.extend([0; 20]);
        empty_mluc(&mut tag);
        empty_mluc(&mut tag);
    }
    tag
}

fn identity_curve_set(tag: &mut Vec<u8>, parametric: bool) {
    for _ in 0..3 {
        if parametric {
            // Function type 3 with g = 1, a = 1, b = c = d = 0, as AE writes A curves.
            tag.extend(b"para");
            tag.extend([0; 4]);
            tag.extend([0, 3, 0, 0]);
            tag.extend(0x1_0000_u32.to_be_bytes());
            tag.extend(0x1_0000_u32.to_be_bytes());
            tag.extend([0; 12]);
        } else {
            tag.extend(b"curv");
            tag.extend([0; 8]);
        }
    }
}

fn lut_a_to_b(lut: &Lut3d) -> (Vec<u8>, usize) {
    let size = lut.size;
    let mut tag = b"mAB ".to_vec();
    tag.extend([0; 4]);
    tag.extend([3, 3, 0, 0]);
    let offsets_at = tag.len();
    tag.extend([0; 20]);
    let b_curves = tag.len();
    identity_curve_set(&mut tag, false);
    let matrix = tag.len();
    for value in [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0] {
        tag.extend((value * 0x1_0000_i32).to_be_bytes());
    }
    let m_curves = tag.len();
    identity_curve_set(&mut tag, false);
    let clut = tag.len();
    let mut grid = [0_u8; 16];
    let grid_size = u8::try_from(size).expect("grid size validated by the parser");
    grid[..3].fill(grid_size);
    tag.extend(grid);
    tag.extend([2, 0, 0, 0]);
    let mut clamped = 0;
    for red in 0..size {
        for green in 0..size {
            for blue in 0..size {
                for value in lut.samples[(blue * size + green) * size + red] {
                    if !(0.0..=1.0).contains(&value) {
                        clamped += 1;
                    }
                    let code = (value.clamp(0.0, 1.0) * 65_535.0).round() as u16;
                    tag.extend(code.to_be_bytes());
                }
            }
        }
    }
    tag.resize(tag.len().next_multiple_of(4), 0);
    let a_curves = tag.len();
    identity_curve_set(&mut tag, true);
    for (index, offset) in [b_curves, matrix, m_curves, clut, a_curves]
        .into_iter()
        .enumerate()
    {
        let at = offsets_at + 4 * index;
        tag[at..at + 4].copy_from_slice(&(offset as u32).to_be_bytes());
    }
    (tag, clamped)
}

/// Encode `lut` as AE's Apply Color LUT sequence data: the identity-curve
/// device link with a 16-bit CLUT. Returns the number of samples clamped
/// into the 0..1 code range.
pub(crate) fn encode_sequence_data(lut: &Lut3d, file_name: &str) -> (Vec<u8>, usize) {
    let (a2b0, clamped) = lut_a_to_b(lut);
    let tags: [(&[u8; 4], Vec<u8>); 4] = [
        (b"desc", mluc(file_name, true)),
        (b"cprt", mluc("Converted LUT", false)),
        (b"pseq", profile_sequence()),
        (b"A2B0", a2b0),
    ];
    let mut table = (tags.len() as u32).to_be_bytes().to_vec();
    let mut body = Vec::new();
    let mut offset = PROFILE_HEADER_LEN + 4 + 12 * tags.len();
    for (signature, tag) in &tags {
        table.extend(*signature);
        table.extend((offset as u32).to_be_bytes());
        table.extend((tag.len() as u32).to_be_bytes());
        let padded = tag.len().next_multiple_of(4);
        body.extend(tag);
        body.resize(body.len() + padded - tag.len(), 0);
        offset += padded;
    }
    // A tag may end unpadded at the end of the profile.
    let unpadded = tags
        .last()
        .map_or(0, |(_, tag)| tag.len().next_multiple_of(4) - tag.len());
    body.truncate(body.len() - unpadded);
    let total = PROFILE_HEADER_LEN + table.len() + body.len();
    let mut profile = Vec::with_capacity(total);
    profile.extend((total as u32).to_be_bytes());
    profile.extend([0; 4]); // preferred CMM: none
    profile.extend([4, 0, 0, 0]); // ICC v4.0
    profile.extend(b"link");
    profile.extend(b"RGB ");
    profile.extend(b"RGB ");
    // Fixed creation date (2026-01-01 00:00:00) keeps exports reproducible.
    for value in [2026_u16, 1, 1, 0, 0, 0] {
        profile.extend(value.to_be_bytes());
    }
    profile.extend(b"acsp");
    profile.extend([0; 24]); // platform, flags, manufacturer, model, attributes
    profile.extend([0; 4]); // perceptual intent
    profile.extend(D50);
    profile.extend([0; 4]); // creator
    profile.extend([0; 16]); // profile ID not calculated
    profile.extend([0; 28]);
    debug_assert_eq!(profile.len(), PROFILE_HEADER_LEN);
    profile.extend(table);
    profile.extend(body);
    let mut data = HEADER_PREFIX.to_vec();
    data.extend((profile.len() as u32).to_be_bytes());
    data.extend([0; 8]);
    data.extend(profile);
    (data, clamped)
}

#[cfg(test)]
mod tests;
