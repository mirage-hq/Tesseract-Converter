//! Optional AE solid footage descriptor. Malformed payloads are retained as an
//! item-local error so unrelated composition content can still be imported.

use thiserror::Error;

use crate::rifx::Chunk;

/// Decoded main-source solid settings (not the proxy source).
#[derive(Clone, Debug, PartialEq)]
pub struct SolidSource {
    /// Source canvas dimensions in pixels, from `Pin /sspc`.
    pub width: u16,
    /// Source canvas height in pixels, from `Pin /sspc`.
    pub height: u16,
    /// Big-endian pixel-aspect fraction from `Pin /sspc`.
    pub pixel_aspect: (u32, u32),
    /// Straight RGB components in the AE source's 0..=1 range, from `Pin /opti`.
    pub color: [f32; 3],
}

/// An unsupported or malformed optional solid descriptor; not a project framing error.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SolidDecodeError {
    /// An expected solid source chunk is absent or ambiguous.
    #[error("missing or duplicate solid {0} chunk")]
    Chunk(&'static str),
    /// Solid source settings or asset options have an unsupported layout.
    #[error("invalid solid {0}")]
    Invalid(&'static str),
}

fn unique_data(children: &[Chunk], id: [u8; 4]) -> Result<&[u8], SolidDecodeError> {
    let mut matches = children.iter().filter(|chunk| chunk.id() == id);
    let Some(chunk) = matches.next() else {
        return Err(SolidDecodeError::Chunk(if id == *b"sspc" {
            "sspc"
        } else {
            "opti"
        }));
    };
    if matches.next().is_some() {
        return Err(SolidDecodeError::Chunk(if id == *b"sspc" {
            "sspc"
        } else {
            "opti"
        }));
    }
    chunk
        .data_payload()
        .ok_or(SolidDecodeError::Invalid("chunk shape"))
}

/// Decode a known `Soli` main source; never fall back to proxy or another item.
pub(super) fn decode(pin: &Chunk) -> Result<SolidSource, SolidDecodeError> {
    let children = pin
        .children()
        .ok_or(SolidDecodeError::Invalid("Pin shape"))?;
    let opti = unique_data(children, *b"opti")?;
    // py-aep e12a451: SoliOptiChunk is 4+2+8+3*4+256 = 282 bytes.
    if opti.len() < 282 || &opti[..4] != b"Soli" || u16::from_be_bytes([opti[4], opti[5]]) != 9 {
        return Err(SolidDecodeError::Invalid("opti layout"));
    }
    let color = [14, 18, 22].map(|offset| {
        f32::from_be_bytes([
            opti[offset],
            opti[offset + 1],
            opti[offset + 2],
            opti[offset + 3],
        ])
    });
    if color
        .iter()
        .any(|component| !component.is_finite() || !(0.0..=1.0).contains(component))
    {
        return Err(SolidDecodeError::Invalid("color"));
    }
    let sspc = unique_data(children, *b"sspc")?;
    // py-aep SspcChunk: dimensions at 32/36; aspect dividend/divisor at 136/140.
    if sspc.len() < 144 || &sspc[22..26] != b"Soli" {
        return Err(SolidDecodeError::Invalid("sspc layout"));
    }
    let width = u16::from_be_bytes([sspc[32], sspc[33]]);
    let height = u16::from_be_bytes([sspc[36], sspc[37]]);
    let pixel_aspect = (
        u32::from_be_bytes([sspc[136], sspc[137], sspc[138], sspc[139]]),
        u32::from_be_bytes([sspc[140], sspc[141], sspc[142], sspc[143]]),
    );
    if width == 0 || height == 0 || pixel_aspect.0 == 0 || pixel_aspect.1 == 0 {
        return Err(SolidDecodeError::Invalid("dimensions or pixel aspect"));
    }
    Ok(SolidSource {
        width,
        height,
        pixel_aspect,
        color,
    })
}

#[cfg(test)]
mod tests {
    use super::{SolidDecodeError, decode};
    use crate::rifx::Chunk;

    #[test]
    fn rejects_short_and_invalid_optional_solid() {
        let short = Chunk::list(
            *b"Pin ",
            vec![Chunk::data(*b"opti", b"Soli".to_vec()).unwrap()],
        );
        assert_eq!(
            decode(&short),
            Err(SolidDecodeError::Invalid("opti layout"))
        );
        let mut opti = vec![0; 282];
        opti[..4].copy_from_slice(b"Soli");
        opti[4..6].copy_from_slice(&9u16.to_be_bytes());
        let mut sspc = vec![0; 144];
        sspc[22..26].copy_from_slice(b"Soli");
        sspc[32..34].copy_from_slice(&100u16.to_be_bytes());
        sspc[36..38].copy_from_slice(&100u16.to_be_bytes());
        // A zero aspect denominator must not reach a drawable source.
        let pin = Chunk::list(
            *b"Pin ",
            vec![
                Chunk::data(*b"opti", opti).unwrap(),
                Chunk::data(*b"sspc", sspc).unwrap(),
            ],
        );
        assert_eq!(
            decode(&pin),
            Err(SolidDecodeError::Invalid("dimensions or pixel aspect"))
        );
    }
}
