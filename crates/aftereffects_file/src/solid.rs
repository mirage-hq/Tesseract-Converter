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
    /// Straight nonnegative RGB components from `Pin /opti`: 0..=1, or above 1.0
    /// for a 32 bpc super-white. Paint consumers must check the range.
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
    // Revision 8 uses a 64-byte name (90 bytes total); revision 9 uses
    // 256 bytes (282 total, py-aep e12a451). RGB offsets are unchanged.
    // Check the revision/size pair, not just a shortened common prefix.
    if opti.len() < 6 || &opti[..4] != b"Soli" {
        return Err(SolidDecodeError::Invalid("opti layout"));
    }
    let revision = u16::from_be_bytes([opti[4], opti[5]]);
    if !matches!((revision, opti.len()), (8, 90) | (9, 282..)) {
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
        .any(|component| !component.is_finite() || *component < 0.0)
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
    fn decodes_legacy_revision_eight_solid() {
        // Revision 8 has a 64-byte name, rather than revision 9's 256 bytes.
        // Both retain the same source RGB, dimensions and pixel-aspect offsets.
        let mut opti = vec![0; 90];
        opti[..4].copy_from_slice(b"Soli");
        opti[4..6].copy_from_slice(&8u16.to_be_bytes());
        opti[6..10].copy_from_slice(&90u32.to_be_bytes());
        opti[10..14].copy_from_slice(&1.0f32.to_be_bytes());
        for (offset, value) in [(14, 0.25f32), (18, 0.5), (22, 0.75)] {
            opti[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        let mut sspc = vec![0; 286];
        sspc[22..26].copy_from_slice(b"Soli");
        sspc[32..34].copy_from_slice(&1920u16.to_be_bytes());
        sspc[36..38].copy_from_slice(&1080u16.to_be_bytes());
        sspc[136..140].copy_from_slice(&1u32.to_be_bytes());
        sspc[140..144].copy_from_slice(&1u32.to_be_bytes());
        let pin = Chunk::list(
            *b"Pin ",
            vec![
                Chunk::data(*b"opti", opti).unwrap(),
                Chunk::data(*b"sspc", sspc).unwrap(),
            ],
        );
        let solid = decode(&pin).unwrap();
        assert_eq!((solid.width, solid.height), (1920, 1080));
        assert_eq!(solid.pixel_aspect, (1, 1));
        assert_eq!(solid.color, [0.25, 0.5, 0.75]);
    }

    fn revision_nine_solid(color: [f32; 3]) -> Chunk {
        let mut opti = vec![0; 282];
        opti[..4].copy_from_slice(b"Soli");
        opti[4..6].copy_from_slice(&9u16.to_be_bytes());
        for (offset, value) in [14, 18, 22].into_iter().zip(color) {
            opti[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        let mut sspc = vec![0; 144];
        sspc[22..26].copy_from_slice(b"Soli");
        sspc[32..34].copy_from_slice(&100u16.to_be_bytes());
        sspc[36..38].copy_from_slice(&50u16.to_be_bytes());
        sspc[136..140].copy_from_slice(&1u32.to_be_bytes());
        sspc[140..144].copy_from_slice(&1u32.to_be_bytes());
        Chunk::list(
            *b"Pin ",
            vec![
                Chunk::data(*b"opti", opti).unwrap(),
                Chunk::data(*b"sspc", sspc).unwrap(),
            ],
        )
    }

    #[test]
    fn decodes_super_white_and_rejects_negative_solid_colors() {
        // 32 bpc projects store super-white components above 1.0; the geometry
        // stays usable and paint consumers decide what the colour means.
        let solid = decode(&revision_nine_solid([2.0, 2.0, 2.0])).unwrap();
        assert_eq!(solid.color, [2.0, 2.0, 2.0]);
        assert_eq!((solid.width, solid.height), (100, 50));
        assert_eq!(
            decode(&revision_nine_solid([0.5, -0.1, 0.5])),
            Err(SolidDecodeError::Invalid("color"))
        );
    }

    #[test]
    fn rejects_unknown_and_truncated_solid_layouts() {
        for (revision, length) in [(8u16, 89), (8, 282), (9, 90), (10, 282)] {
            let mut opti = vec![0; length];
            opti[..4].copy_from_slice(b"Soli");
            opti[4..6].copy_from_slice(&revision.to_be_bytes());
            let pin = Chunk::list(*b"Pin ", vec![Chunk::data(*b"opti", opti).unwrap()]);
            assert_eq!(decode(&pin), Err(SolidDecodeError::Invalid("opti layout")));
        }
    }

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
