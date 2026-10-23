//! Lumetri Color custom `.cube` LUTs as Premiere Pro 26.5.2 and After
//! Effects 26.5 save a file browsed in the Basic Correction Input LUT or the
//! Creative Look: both hosts save the same values under the same
//! `ParameterID`s (the UI-authored fixtures `color_lumetri_look_cube.prproj`
//! and `color_ae_lumetri_custom_lut.aep`).
//!
//! - The path slot ([`INPUT_LUT_PATH`], [`LOOK_PATH`]) holds the file's
//!   absolute path as UTF-16LE with one NUL terminator
//!   ([`decode_path_slot`]); the empty default is `FE FE`.
//! - The `Blob` (ParameterID 1) is a Lumetri shader graph naming the same
//!   path in its `LUT` shaders and mapping each `<embeddedlut>` id to the
//!   file it copies ([`parse_blob_luts`]).
//! - `Embedded LUTs` (ParameterID 98, [`EMBEDDED_LUTS`]) holds a copy of the
//!   samples ([`embedded_entries`], [`decode_itx3`]). In both fixtures that
//!   copy is the `.cube`'s values as `f32`, exactly.

/// Basic Correction Input LUT path slot.
pub const INPUT_LUT_PATH: usize = 4;
/// Creative Look path slot.
pub const LOOK_PATH: usize = 24;
/// Creative Look Intensity, 0 to 200 (100 applies the LUT once).
pub const LOOK_INTENSITY: usize = 26;
/// Embedded LUT samples saved beside a browsed LUT.
pub const EMBEDDED_LUTS: usize = 98;

/// The LUT paths that a `Blob` shader graph applies.
#[derive(Debug, Default, PartialEq)]
pub struct BlobLuts {
    /// The Input LUT: a `LUT` shader before `BasicCorrection3`.
    pub input: Option<String>,
    /// The Look: the `LUT` shader beside `LookColorSpace`, and the
    /// `__Intensity` of the group that holds it.
    pub look: Option<(String, Option<f64>)>,
    /// The `<embeddedlut>` entries: the id (hex) of an `Embedded LUTs`
    /// entry and the path of the file it copies.
    pub embedded: Vec<(String, String)>,
}

/// Parse the LUTs of a Lumetri `Blob` shader graph; `None` when the `Blob`
/// is not a graph (the `FE FE` sentinel) or does not parse.
pub fn parse_blob_luts(text: &str) -> Option<BlobLuts> {
    let document = roxmltree::Document::parse(text).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "Lumetri" {
        return None;
    }
    let keyframe = root.children().find(|node| node.has_tag_name("keyframe"))?;
    let unquote = |text: Option<&str>| {
        text.map(str::trim)
            .and_then(|text| text.strip_prefix('"')?.strip_suffix('"'))
            .map(str::to_owned)
    };
    let mut luts = BlobLuts::default();
    for entry in root
        .children()
        .filter(|node| node.has_tag_name("embeddedlut"))
    {
        let text = |name: &str| {
            entry
                .children()
                .find(|node| node.has_tag_name(name))
                .and_then(|node| node.text())
                .map(|text| text.trim().to_owned())
        };
        if let (Some(hash), Some(filename)) = (text("hash"), text("filename")) {
            luts.embedded.push((hash, filename));
        }
    }
    let mut seen_basic = false;
    let mut look_group = false;
    for shader in keyframe
        .children()
        .filter(|node| node.has_tag_name("shader"))
    {
        let child = |name: &str| shader.children().find(|node| node.has_tag_name(name));
        let name = unquote(child("name").and_then(|node| node.text()));
        let parameters = child("parameters");
        let parameter = |key: &str| {
            unquote(
                parameters
                    .and_then(|node| node.children().find(|node| node.has_tag_name(key)))
                    .and_then(|node| node.text()),
            )
        };
        match name.as_deref() {
            Some("BasicCorrection3") => seen_basic = true,
            Some("__GroupBegin") if parameter("__Preset").is_some() => look_group = true,
            Some("__GroupEnd") if look_group => {
                if let Some((_, intensity)) = &mut luts.look {
                    *intensity = parameter("__Intensity").and_then(|value| value.parse().ok());
                }
                look_group = false;
            }
            Some("LUT") => {
                let path = parameter("LUT").filter(|path| !path.is_empty());
                if parameter("LookColorSpace").is_some() {
                    if let Some(path) = path {
                        luts.look = Some((path, None));
                    }
                } else if !seen_basic {
                    luts.input = path;
                }
            }
            _ => {}
        }
    }
    Some(luts)
}

/// The entries of an `Embedded LUTs` value: a 16-byte id, a little-endian
/// `u32` byte length and that many payload bytes, repeated. Empty when the
/// bytes are not a whole entry list.
pub fn embedded_entries(bytes: &[u8]) -> Vec<([u8; 16], Vec<u8>)> {
    let mut entries = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let Some((id, tail)) = rest.split_first_chunk::<16>() else {
            return Vec::new();
        };
        let Some((length, tail)) = tail.split_first_chunk::<4>() else {
            return Vec::new();
        };
        let length = u32::from_le_bytes(*length) as usize;
        if tail.len() < length {
            return Vec::new();
        }
        entries.push((*id, tail[..length].to_vec()));
        rest = &tail[length..];
    }
    entries
}

/// An `Embedded LUTs` id as the `Blob`'s `<embeddedlut><hash>` spells it
/// (compare without case).
fn hex_id(id: &[u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The decoded `ITX3` copy of the `.cube` that the `Blob` maps to `path`,
/// if `embedded` holds one in the pinned layout.
pub fn embedded_copy(
    blob: &BlobLuts,
    embedded: &[([u8; 16], Vec<u8>)],
    path: &str,
) -> Option<EmbeddedLut> {
    let (hash, _) = blob
        .embedded
        .iter()
        .find(|(_, filename)| filename == path)?;
    let (_, payload) = embedded
        .iter()
        .find(|(id, _)| hex_id(id).eq_ignore_ascii_case(hash))?;
    decode_itx3(payload)
}

/// LUT samples embedded for a browsed `.cube`: an `ITX3` table (`u32` size,
/// `u32` 3 channels, 16 zero bytes, `f32` domain maximum 1, 1, 1, then size³
/// RGB `f32` samples in `.cube` order, red fastest). Only that layout is read.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedLut {
    pub size: usize,
    pub samples: Vec<[f32; 3]>,
}

impl EmbeddedLut {
    /// The samples as a `.cube` text: `LUT_3D_SIZE`, domain 0..1, and each
    /// sample at its shortest exact `f32` spelling.
    pub fn cube_text(&self) -> String {
        let mut text = format!(
            "LUT_3D_SIZE {}\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n",
            self.size
        );
        for [red, green, blue] in &self.samples {
            text.push_str(&format!("{red} {green} {blue}\n"));
        }
        text
    }
}

/// An `ITX3` table in the pinned layout ([`EmbeddedLut`]); `None` otherwise.
fn decode_itx3(payload: &[u8]) -> Option<EmbeddedLut> {
    let word = |at: usize| -> Option<[u8; 4]> { payload.get(at..at + 4)?.try_into().ok() };
    if payload.get(..4)? != b"ITX3" || u32::from_le_bytes(word(8)?) != 3 {
        return None;
    }
    let size = u32::from_le_bytes(word(4)?) as usize;
    if !(2..=256).contains(&size)
        || payload.get(12..28)?.iter().any(|byte| *byte != 0)
        || (0..3).any(|index| word(28 + 4 * index).map(f32::from_le_bytes) != Some(1.0))
    {
        return None;
    }
    let data = payload.get(40..)?;
    if data.len() != size.pow(3) * 12 {
        return None;
    }
    let samples: Vec<[f32; 3]> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| f32::from_le_bytes(*word))
        .collect::<Vec<_>>()
        .as_chunks::<3>()
        .0
        .to_vec();
    samples
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(EmbeddedLut { size, samples })
}

/// The path of a static path-slot value (ParameterID `id`): `None` for the
/// empty `FE FE` default or an empty path, else the UTF-16LE text before its
/// NUL terminator, which must name a `.cube` file. Premiere 26.5.2 renders a
/// slot saved without its terminator (the form a scripted path write saves)
/// without the LUT, so that form is an error.
pub fn decode_path_slot(bytes: &[u8], id: usize) -> Result<Option<String>, String> {
    if bytes.is_empty() || bytes == [0xFE, 0xFE] {
        return Ok(None);
    }
    if !bytes.len().is_multiple_of(2) {
        return Err(format!(
            "has an odd-length UTF-16 path slot ParameterID {id}"
        ));
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let Some(end) = units.iter().position(|unit| *unit == 0) else {
        return Err(format!(
            "saves its path in ParameterID {id} without the NUL terminator, which Premiere renders without the LUT"
        ));
    };
    if units[end..].iter().any(|unit| *unit != 0) {
        return Err(format!(
            "has data after the path terminator in ParameterID {id}"
        ));
    }
    let path = String::from_utf16(&units[..end])
        .map_err(|_| format!("has an invalid UTF-16 path in ParameterID {id}"))?;
    if path.is_empty() {
        return Ok(None);
    }
    if !path.to_ascii_lowercase().ends_with(".cube") {
        return Err(format!("names {path:?}, which is not a .cube file"));
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn path_slots_need_their_terminator_and_a_cube_file() {
        assert_eq!(decode_path_slot(&[0xFE, 0xFE], 24), Ok(None));
        assert_eq!(
            decode_path_slot(&utf16("/a/look.cube\0"), 24),
            Ok(Some("/a/look.cube".to_owned()))
        );
        assert!(
            decode_path_slot(&utf16("/a/look.cube"), 24)
                .unwrap_err()
                .contains("without the NUL terminator")
        );
        assert!(
            decode_path_slot(&utf16("/a/look.3dl\0"), 4)
                .unwrap_err()
                .contains("not a .cube file")
        );
        assert!(
            decode_path_slot(&utf16("/a/look.cube\0x"), 4)
                .unwrap_err()
                .contains("after the path terminator")
        );
    }

    #[test]
    fn an_itx3_copy_is_found_through_the_blob_and_decoded() {
        let mut table = b"ITX3".to_vec();
        table.extend(2u32.to_le_bytes());
        table.extend(3u32.to_le_bytes());
        table.extend([0; 16]);
        for _ in 0..3 {
            table.extend(1f32.to_le_bytes());
        }
        let samples: Vec<[f32; 3]> = (0..8)
            .map(|index| [index & 1, (index >> 1) & 1, index >> 2].map(|bit| bit as f32 * 0.5))
            .collect();
        for value in samples.iter().flatten() {
            table.extend(value.to_le_bytes());
        }
        let id = [0xA0; 16];
        let mut value = id.to_vec();
        value.extend((table.len() as u32).to_le_bytes());
        value.extend(&table);
        let entries = embedded_entries(&value);
        assert_eq!(entries.len(), 1);
        let blob = parse_blob_luts(
            r#"<Lumetri><keyframe>
              <shader><name>"LUT"</name><parameters><LUT>"/a/in.cube"</LUT></parameters></shader>
              <shader><name>"BasicCorrection3"</name></shader>
              <shader><name>"__GroupBegin"</name><parameters><__Preset>"/a/look.cube"</__Preset></parameters></shader>
              <shader><name>"LUT"</name><parameters><LookColorSpace>"x"</LookColorSpace><LUT>"/a/look.cube"</LUT></parameters></shader>
              <shader><name>"__GroupEnd"</name><parameters><__Intensity>"80"</__Intensity></parameters></shader>
            </keyframe><embeddedlut><hash>A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0</hash><filename>/a/look.cube</filename></embeddedlut></Lumetri>"#,
        )
        .unwrap();
        assert_eq!(blob.input.as_deref(), Some("/a/in.cube"));
        assert_eq!(blob.look, Some(("/a/look.cube".to_owned(), Some(80.0))));
        let copy = embedded_copy(&blob, &entries, "/a/look.cube").unwrap();
        assert_eq!(copy, EmbeddedLut { size: 2, samples });
        assert!(embedded_copy(&blob, &entries, "/a/in.cube").is_none());
        assert!(embedded_entries(&value[..value.len() - 1]).is_empty());
    }
}
