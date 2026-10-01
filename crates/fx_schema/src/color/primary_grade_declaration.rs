//! Primary-grade controls and validation, with reader-selected field strictness.

/// Share grade data without weakening the normalized action reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_primary_grade_schema {
    ($($strict:meta)?) => {
        /// Canonical v1 controls evaluated on straight SDR Rec.709 code-value RGB.
        ///
        /// Controls are intentionally plain scalars on the generated wire API, while
        /// custom deserialization enforces the finite v1 ranges for every persisted
        /// payload. Evaluation order is white balance, exposure, contrast, tonal
        /// offsets, saturation, then vibrance.
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct PrimaryGrade {
            /// Version fixing operation order, equations, encoding, and alpha behavior.
            #[ts(type = "number")]
            pub semantic_version: PrimaryGradeSemanticVersion,
            /// Red-versus-blue gain, normalized to -1..=1.
            pub temperature: f64,
            /// Green-versus-magenta gain, normalized to -1..=1.
            pub tint: f64,
            /// Exposure in stops, -5..=5.
            pub exposure: f64,
            /// Contrast in stops around the 0.5 code-value pivot, -2..=2.
            pub contrast: f64,
            /// Highlight-window offset, normalized to -1..=1.
            pub highlights: f64,
            /// Shadow-window offset, normalized to -1..=1.
            pub shadows: f64,
            /// White-window offset, normalized to -1..=1.
            pub whites: f64,
            /// Black-window offset, normalized to -1..=1.
            pub blacks: f64,
            /// Rec.709-luma saturation adjustment, normalized to -1..=1.
            pub saturation: f64,
            /// Chroma-attenuated saturation adjustment, normalized to -1..=1.
            pub vibrance: f64,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase" $(, $strict)?)]
        struct PrimaryGradeWire {
            semantic_version: PrimaryGradeSemanticVersion,
            temperature: f64,
            tint: f64,
            exposure: f64,
            contrast: f64,
            highlights: f64,
            shadows: f64,
            whites: f64,
            blacks: f64,
            saturation: f64,
            vibrance: f64,
        }

        impl<'de> Deserialize<'de> for PrimaryGrade {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let wire = PrimaryGradeWire::deserialize(deserializer)?;
                Self::new(
                    wire.semantic_version,
                    [
                        wire.temperature,
                        wire.tint,
                        wire.exposure,
                        wire.contrast,
                        wire.highlights,
                        wire.shadows,
                        wire.whites,
                        wire.blacks,
                        wire.saturation,
                        wire.vibrance,
                    ],
                )
                .map_err(serde::de::Error::custom)
            }
        }

        impl Default for PrimaryGrade {
            fn default() -> Self {
                Self {
                    semantic_version: PrimaryGradeSemanticVersion::V1,
                    temperature: 0.0,
                    tint: 0.0,
                    exposure: 0.0,
                    contrast: 0.0,
                    highlights: 0.0,
                    shadows: 0.0,
                    whites: 0.0,
                    blacks: 0.0,
                    saturation: 0.0,
                    vibrance: 0.0,
                }
            }
        }

        impl PrimaryGrade {
            const NAMES: [&'static str; 10] = [
                "temperature",
                "tint",
                "exposure",
                "contrast",
                "highlights",
                "shadows",
                "whites",
                "blacks",
                "saturation",
                "vibrance",
            ];
            const MINIMA: [f64; 10] = [-1.0, -1.0, -5.0, -2.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0];
            const MAXIMA: [f64; 10] = [1.0, 1.0, 5.0, 2.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

            /// Validate and construct a v1 primary grade from controls in catalog order.
            pub fn new(
                semantic_version: PrimaryGradeSemanticVersion,
                controls: [f64; 10],
            ) -> Result<Self, PrimaryGradeError> {
                for (index, value) in controls.iter().copied().enumerate() {
                    if !value.is_finite() || !(Self::MINIMA[index]..=Self::MAXIMA[index]).contains(&value) {
                        return Err(PrimaryGradeError {
                            control: Self::NAMES[index],
                            value,
                            min: Self::MINIMA[index],
                            max: Self::MAXIMA[index],
                        });
                    }
                }
                Ok(Self {
                    semantic_version,
                    temperature: controls[0],
                    tint: controls[1],
                    exposure: controls[2],
                    contrast: controls[3],
                    highlights: controls[4],
                    shadows: controls[5],
                    whites: controls[6],
                    blacks: controls[7],
                    saturation: controls[8],
                    vibrance: controls[9],
                })
            }

            /// Whether all controls are neutral and the renderer can bypass exactly.
            #[must_use]
            pub fn is_neutral(self) -> bool {
                self.controls().iter().all(|value| *value == 0.0)
            }

            fn controls(self) -> [f64; 10] {
                [
                    self.temperature,
                    self.tint,
                    self.exposure,
                    self.contrast,
                    self.highlights,
                    self.shadows,
                    self.whites,
                    self.blacks,
                    self.saturation,
                    self.vibrance,
                ]
            }
        }

        /// A non-finite or out-of-range primary-grade control.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct PrimaryGradeError {
            control: &'static str,
            value: f64,
            min: f64,
            max: f64,
        }

        impl fmt::Display for PrimaryGradeError {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "primary-grade {} {} must be finite and in {}..={}",
                    self.control, self.value, self.min, self.max
                )
            }
        }

        impl std::error::Error for PrimaryGradeError {}
    };
}
