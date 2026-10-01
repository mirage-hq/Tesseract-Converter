//! After Effects envelope policy layered on generic RIFX framing.

use crate::rifx::{self, Chunk, Rifx, RifxError};

/// An AEP `RIFX` / `Egg!` envelope and trailing XMP bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// Top-level chunks in their original order.
    pub chunks: Vec<Chunk>,
    /// Trailing XMP bytes, deliberately not decoded or normalized.
    pub xmp: Vec<u8>,
}

impl Project {
    /// Parses a bounded AEP envelope. `btdk` LIST payloads remain opaque.
    pub fn parse(input: &[u8]) -> Result<Self, RifxError> {
        let rifx = Rifx::parse_with(input, |kind| kind == *b"btdk")?;
        if rifx.form() != *b"Egg!" {
            return Err(RifxError::Invalid("unexpected RIFX form"));
        }
        let (_, chunks, xmp) = rifx.into_parts();
        Ok(Self { chunks, xmp })
    }

    /// Encodes the AEP envelope and trailing XMP.
    pub fn encode(&self) -> Result<Vec<u8>, RifxError> {
        rifx::encode(*b"Egg!", &self.chunks, &self.xmp)
    }
}

#[cfg(test)]
mod tests {
    use super::Project;
    use crate::rifx::{Chunk, Rifx, RifxError};

    #[test]
    fn generic_non_egg_is_not_an_aep() {
        let bytes = Rifx::new(*b"TEST", vec![], vec![]).encode().unwrap();
        assert_eq!(
            Project::parse(&bytes),
            Err(RifxError::Invalid("unexpected RIFX form"))
        );
    }

    #[test]
    fn all_fixtures_roundtrip_exactly() {
        for bytes in [
            &include_bytes!("../tests/fixtures/ae26_one_comp.aep")[..],
            &include_bytes!("../tests/fixtures/ae26_rust_tall_resaved.aep")[..],
            &include_bytes!("../tests/fixtures/ae26_rust_wide_resaved.aep")[..],
            &include_bytes!("../tests/fixtures/compositions.aep")[..],
            &include_bytes!("../tests/fixtures/empty.aep")[..],
        ] {
            assert_eq!(Project::parse(bytes).unwrap().encode().unwrap(), bytes);
        }
    }

    #[test]
    fn opaque_btdk_and_xmp_are_preserved() {
        let project = Project {
            chunks: vec![Chunk::opaque_list(*b"btdk", b"not chunks".to_vec())],
            xmp: b"<xmp/>".to_vec(),
        };
        assert_eq!(Project::parse(&project.encode().unwrap()).unwrap(), project);
    }
}
