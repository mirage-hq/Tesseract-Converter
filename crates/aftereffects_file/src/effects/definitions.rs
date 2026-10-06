//! Adobe AE 26.5 parameter ABI declarations, not recorded effect instances.
//! The checked-in registry retains typed `pard` constants and independently
//! read Adobe UI values or pinned native descriptor defaults (see provenance);
//! it contains no original layer values or keyframes.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::rifx::{Chunk, RifxError};

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ParameterDefinition {
    pub match_name: String,
    pub label: String,
    /// Three big-endian words at offsets 0..12.
    pub header_flags: [u32; 3],
    /// Native parameter discriminator at offset 12.
    pub kind: u32,
    /// Two big-endian words at offsets 48..56.
    pub reserved: [u32; 2],
    /// 23 big-endian ABI constant words at offsets 56..148.
    pub payload_words: Vec<u32>,
    /// Optional UTF-8 popup choices from the native `pdnm` declaration.
    pub popup: Option<String>,
    /// Proven numeric defaults in Adobe units; absent for nonnumeric controls.
    pub defaults: Vec<f64>,
    /// Point ABI defaults are relative to the source canvas, not absolute pixels.
    pub point_relative: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct EffectDefinition {
    pub match_name: String,
    pub name: String,
    pub parameters: Vec<ParameterDefinition>,
}

#[derive(Deserialize)]
struct Registry {
    #[allow(dead_code)]
    provenance: serde_json::Value,
    effects: Vec<EffectDefinition>,
}

/// Canonical AE 26.5 effect declarations observed in the independent catalog.
pub(crate) fn registry() -> &'static [EffectDefinition] {
    static REGISTRY: OnceLock<Vec<EffectDefinition>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        serde_json::from_str::<Registry>(include_str!("definitions.json"))
            .expect("checked-in AE parameter definitions must be valid JSON")
            .effects
    })
}

/// Look up a canonical definition by the native effect match name.
pub(crate) fn definition(match_name: &str) -> Option<&'static EffectDefinition> {
    registry()
        .iter()
        .find(|effect| effect.match_name == match_name)
}

impl ParameterDefinition {
    /// Return numeric defaults in Adobe units; relative point ABI values need
    /// the actual source size to become pixel coordinates.
    pub(crate) fn values(&self, size: [f64; 2]) -> Option<Vec<f64>> {
        if self.kind == 6 && self.point_relative && size.iter().all(|v| v.is_finite() && *v >= 0.0)
        {
            let coords: Vec<_> = self
                .payload_words
                .get(..2)?
                .iter()
                .zip(size)
                .map(|(&raw, extent)| {
                    f64::from(i32::from_be_bytes(raw.to_be_bytes())) / 65_536.0 * extent
                })
                .collect();
            return coords.iter().all(|v| v.is_finite()).then_some(coords);
        }
        match self.kind {
            1 | 2 | 3 | 4 | 5 | 7 | 10
                if !self.defaults.is_empty() && self.defaults.iter().all(|v| v.is_finite()) =>
            {
                Some(self.defaults.clone())
            }
            _ => None,
        }
    }
}

/// Rebuild a `pard` from explicitly typed ABI declarations, never from a
/// source chunk or a layer-side `sspc` copy.
pub(crate) fn encode_parameter(parameter: &ParameterDefinition) -> Result<Chunk, RifxError> {
    if parameter.payload_words.len() != 23 {
        return Err(RifxError::Invalid(
            "effect parameter must contain 23 ABI payload words",
        ));
    }
    let label = parameter.label.as_bytes();
    if label.len() > 32 {
        return Err(RifxError::Limit("effect parameter label exceeds 32 bytes"));
    }
    if label.contains(&0) {
        return Err(RifxError::Invalid("effect parameter label contains NUL"));
    }
    let mut bytes = Vec::with_capacity(148);
    for word in parameter.header_flags {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes.extend_from_slice(&parameter.kind.to_be_bytes());
    bytes.extend_from_slice(label);
    bytes.resize(48, 0);
    for word in parameter.reserved {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    for &word in &parameter.payload_words {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    Chunk::data(*b"pard", bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_catalog_contains_hidden_root_and_builtin_group() {
        assert_eq!(registry().len(), 34);
        let blur = definition("ADBE Gaussian Blur 2").unwrap();
        assert_eq!(blur.parameters.len(), 5);
        assert_eq!(blur.parameters[0].match_name, "ADBE Gaussian Blur 2-0000");
        assert!(blur.parameters[0].values([120.0, 80.0]).is_none());
        assert_eq!(blur.parameters[1].values([120.0, 80.0]), Some(vec![25.0]));
        assert_eq!(blur.parameters[4].match_name, "ADBE Effect Built In Params");
        assert_eq!(blur.parameters[4].kind, 9);
        assert!(blur.parameters[4].values([120.0, 80.0]).is_none());
        assert!(definition("not an Adobe match name").is_none());
    }

    #[test]
    fn invert_catalog_matches_pinned_native_parameter_declarations() {
        fn parameter<'a>(chunks: &'a [Chunk], target: &str) -> Option<&'a [u8]> {
            for chunk in chunks {
                if let Some(children) = chunk.children() {
                    if chunk.list_kind() == Some(*b"parT") {
                        for (name, run) in crate::properties::runs(children).ok()? {
                            if name == target {
                                return crate::properties::data(run, *b"pard").ok();
                            }
                        }
                    }
                    if let Some(bytes) = parameter(children, target) {
                        return Some(bytes);
                    }
                }
            }
            None
        }
        let native = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/cosmic-invert-controls.rifx"),
            |_| false,
        )
        .unwrap();
        let invert = definition("ADBE Invert").unwrap();
        for control in &invert.parameters {
            let encoded = encode_parameter(control).unwrap();
            assert_eq!(
                encoded.data_payload(),
                parameter(native.chunks(), &control.match_name)
            );
        }
        assert_eq!(invert.parameters[1].kind, 7);
        assert_eq!(
            invert.parameters[1].values([1920.0, 1080.0]),
            Some(vec![1.0])
        );
        assert_eq!(invert.parameters[2].kind, 2);
        assert_eq!(
            invert.parameters[2].values([1920.0, 1080.0]),
            Some(vec![0.0])
        );
    }

    #[test]
    fn point_defaults_scale_against_source_canvas() {
        let corner = definition("ADBE Corner Pin").unwrap();
        let point = corner
            .parameters
            .iter()
            .find(|p| p.match_name == "ADBE Corner Pin-0004")
            .unwrap();
        assert_eq!(point.values([120.0, 80.0]), Some(vec![120.0, 80.0]));
        assert_eq!(point.values([1920.0, 1080.0]), Some(vec![1920.0, 1080.0]));
        assert_eq!(point.values([f64::NAN, 80.0]), None);
    }

    #[test]
    fn radial_wipe_canonical_defaults_scale_to_source_canvas() {
        let wipe = definition("ADBE Radial Wipe").unwrap();
        assert_eq!(wipe.parameters.len(), 7);
        let center = &wipe.parameters[3];
        assert_eq!(center.match_name, "ADBE Radial Wipe-0003");
        assert_eq!(center.values([96.0, 64.0]), Some(vec![48.0, 32.0]));
        assert_eq!(center.values([3840.0, 1600.0]), Some(vec![1920.0, 800.0]));
        assert_eq!(center.values([f64::NAN, 64.0]), None);
    }

    #[test]
    fn every_parameter_reconstructs_a_bounded_148_byte_pard() {
        for effect in registry() {
            for parameter in &effect.parameters {
                let chunk = encode_parameter(parameter).unwrap();
                let bytes = chunk.data_payload().unwrap();
                assert_eq!(chunk.id(), *b"pard");
                assert_eq!(bytes.len(), 148);
                assert_eq!(&bytes[12..16], &parameter.kind.to_be_bytes());
                assert_eq!(
                    &bytes[16..16 + parameter.label.len()],
                    parameter.label.as_bytes()
                );
                assert_eq!(&bytes[56..60], &parameter.payload_words[0].to_be_bytes());
            }
        }
        let mut bad = registry()[0].parameters[0].clone();
        bad.payload_words.clear();
        assert!(encode_parameter(&bad).is_err());
        bad.payload_words.resize(23, 0);
        bad.label = "a".repeat(33);
        assert!(encode_parameter(&bad).is_err());
    }
}
