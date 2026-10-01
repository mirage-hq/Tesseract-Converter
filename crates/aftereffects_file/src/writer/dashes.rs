//! Bounded static native Dash/Gap pairs; unsupported patterns are not normalized.

use crate::rifx::Chunk;

use super::{
    AepWriteError,
    views::{self, ValueKind},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StrokeDashes {
    lengths: Vec<f64>,
    offset: f64,
}

impl StrokeDashes {
    pub(crate) fn new(
        lengths: impl IntoIterator<Item = f64>,
        offset: f64,
    ) -> Result<Self, &'static str> {
        // Seven values suffice to reject everything beyond the native names
        // Dash/Gap 1..3 without collecting an arbitrarily large FX pattern.
        let lengths: Vec<_> = lengths.into_iter().take(7).collect();
        if !offset.is_finite() {
            return Err("Stroke dash offset must be finite");
        }
        if lengths.is_empty() {
            return if offset == 0.0 {
                Ok(Self::default())
            } else {
                Err("Stroke dash offset without a pattern is not exported")
            };
        }
        if !matches!(lengths.len(), 2 | 4 | 6)
            || lengths
                .iter()
                .any(|length| !length.is_finite() || *length <= 0.0)
            || !lengths.iter().sum::<f64>().is_finite()
        {
            return Err(
                "Native stroke export requires 1..=3 complete positive finite Dash/Gap pairs; odd, zero-length or longer patterns are not approximated",
            );
        }
        Ok(Self { lengths, offset })
    }

    #[cfg(test)]
    pub(super) fn native_group(&self) -> Result<Option<Chunk>, AepWriteError> {
        self.native_group_with_clock(super::keyframes::PropertyClock::DEFAULT)
    }

    pub(super) fn native_group_with_clock(
        &self,
        clock: super::keyframes::PropertyClock,
    ) -> Result<Option<Chunk>, AepWriteError> {
        self.native_group_with_offset_and_clock(None, clock)
    }

    pub(super) fn native_group_with_offset(
        &self,
        offset_animation: Option<&super::NumericTrack>,
    ) -> Result<Option<Chunk>, AepWriteError> {
        self.native_group_with_offset_and_clock(
            offset_animation,
            super::keyframes::PropertyClock::DEFAULT,
        )
    }

    pub(super) fn native_group_with_offset_and_clock(
        &self,
        offset_animation: Option<&super::NumericTrack>,
        clock: super::keyframes::PropertyClock,
    ) -> Result<Option<Chunk>, AepWriteError> {
        if self.lengths.is_empty() {
            if offset_animation.is_some() {
                return Err(AepWriteError::Invalid(
                    "dash-offset animation requires an exported dash pattern",
                ));
            }
            return Ok(None);
        }
        const NAMES: [&str; 6] = [
            "ADBE Vector Stroke Dash 1",
            "ADBE Vector Stroke Gap 1",
            "ADBE Vector Stroke Dash 2",
            "ADBE Vector Stroke Gap 2",
            "ADBE Vector Stroke Dash 3",
            "ADBE Vector Stroke Gap 3",
        ];
        let mut entries = Vec::with_capacity(self.lengths.len() + 1);
        for (name, length) in NAMES.into_iter().zip(&self.lengths) {
            entries.push((
                name,
                views::property_with_clock(
                    ValueKind::VectorScalar,
                    &[*length],
                    Some((0.0, 100.0)),
                    None,
                    clock,
                )?,
            ));
        }
        if self.offset != 0.0 || offset_animation.is_some() {
            entries.push((
                "ADBE Vector Stroke Offset",
                views::property_with_clock(
                    ValueKind::VectorScalar,
                    &[self.offset],
                    Some((0.0, 100.0)),
                    offset_animation,
                    clock,
                )?,
            ));
        }
        Ok(Some(views::group(1, "Dashes", entries)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_dash_descriptors_match_independent_adobe_source() {
        use crate::{
            properties,
            structure::{ItemKind, read_project},
        };

        fn dashes(chunks: &[Chunk]) -> Option<&[Chunk]> {
            if let Ok(runs) = properties::runs(chunks) {
                for (name, run) in runs {
                    if name == "ADBE Vector Stroke Dashes" {
                        return properties::unique_list(run, *b"tdgp").ok();
                    }
                }
            }
            chunks.iter().filter_map(Chunk::children).find_map(dashes)
        }

        // Adobe-authored DASH_GAP, composition 122, has only one Dash/Gap pair
        // and no Offset. An own-reader roundtrip accepted the invalid generic
        // scalar descriptors, but Adobe rejected the resulting project.
        let native = read_project(include_bytes!(
            "../../tests/fixtures/shapes/import_stroke_dash_caps.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &native.item(122).unwrap().kind else {
            panic!("DASH_GAP composition");
        };
        let authored = comp
            .layers
            .iter()
            .find_map(|layer| dashes(&layer.content))
            .unwrap();
        let generated = StrokeDashes::new([6.0, 6.0], 0.0)
            .unwrap()
            .native_group()
            .unwrap()
            .unwrap();
        let authored = properties::runs(authored).unwrap();
        let written = properties::runs(generated.children().unwrap()).unwrap();
        assert_eq!(
            written.len(),
            authored.len(),
            "a zero phase must not add an Offset"
        );
        for ((expected_name, expected), (actual_name, actual)) in authored.iter().zip(written) {
            assert_eq!(*expected_name, actual_name);
            let expected = properties::unique_list(expected, *b"tdbs").unwrap();
            let actual = properties::unique_list(actual, *b"tdbs").unwrap();
            assert_eq!(
                actual.iter().map(Chunk::id).collect::<Vec<_>>(),
                expected.iter().map(Chunk::id).collect::<Vec<_>>(),
                "native Dash/Gap descriptor and range chunks"
            );
            for (expected, actual) in expected.iter().zip(actual) {
                if [*b"tdsb", *b"tdb4", *b"tdum", *b"tduM"].contains(&expected.id()) {
                    assert_eq!(actual.data_payload(), expected.data_payload());
                }
            }
        }
    }

    #[test]
    fn rejects_patterns_without_bounded_exact_pair_mapping() {
        for lengths in [
            vec![4.0],
            vec![4.0, 2.0, 1.0],
            vec![1.0; 8],
            vec![0.0, 2.0],
            vec![-1.0, 2.0],
            vec![f64::NAN, 2.0],
            vec![f64::INFINITY, 2.0],
            vec![f64::MAX, f64::MAX],
        ] {
            assert!(StrokeDashes::new(lengths, 0.0).is_err());
        }
        assert!(StrokeDashes::new([], 1.0).is_err());
        assert!(StrokeDashes::new([4.0, 2.0], f64::NAN).is_err());
        assert!(StrokeDashes::new([4.0, 2.0], f64::INFINITY).is_err());
        assert_eq!(StrokeDashes::new([], 0.0).unwrap(), StrokeDashes::default());
        assert!(StrokeDashes::default().native_group().unwrap().is_none());
    }
}
