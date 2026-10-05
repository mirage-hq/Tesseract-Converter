//! Shared bounded SDR tonal-color data; product readers opt into strict fields.
#[doc(hidden)]
#[macro_export]
macro_rules! define_tonal_color_schema {
    ($($strict:meta)?) => {
        /// Jerboa SDR tonal-color v1, twelve additive code-value components.
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct TonalColor {
            /// Version fixing weights, chroma projection, gamut fit and alpha behavior.
            #[ts(type = "number")]
            pub semantic_version: TonalColorSemanticVersion,
            /// Signed normalized SDR global red offset, -1..=1; zero neutral.
            pub global_red: f64,
            /// Signed normalized SDR global green offset, -1..=1; zero neutral.
            pub global_green: f64,
            /// Signed normalized SDR global blue offset, -1..=1; zero neutral.
            pub global_blue: f64,
            /// Signed normalized SDR shadows red offset, -1..=1; zero neutral.
            pub shadows_red: f64,
            /// Signed normalized SDR shadows green offset, -1..=1; zero neutral.
            pub shadows_green: f64,
            /// Signed normalized SDR shadows blue offset, -1..=1; zero neutral.
            pub shadows_blue: f64,
            /// Signed normalized SDR midtones red offset, -1..=1; zero neutral.
            pub midtones_red: f64,
            /// Signed normalized SDR midtones green offset, -1..=1; zero neutral.
            pub midtones_green: f64,
            /// Signed normalized SDR midtones blue offset, -1..=1; zero neutral.
            pub midtones_blue: f64,
            /// Signed normalized SDR highlights red offset, -1..=1; zero neutral.
            pub highlights_red: f64,
            /// Signed normalized SDR highlights green offset, -1..=1; zero neutral.
            pub highlights_green: f64,
            /// Signed normalized SDR highlights blue offset, -1..=1; zero neutral.
            pub highlights_blue: f64,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase" $(, $strict)?)]
        struct TonalColorWire {
            semantic_version: TonalColorSemanticVersion,
            global_red: f64,
            global_green: f64,
            global_blue: f64,
            shadows_red: f64,
            shadows_green: f64,
            shadows_blue: f64,
            midtones_red: f64,
            midtones_green: f64,
            midtones_blue: f64,
            highlights_red: f64,
            highlights_green: f64,
            highlights_blue: f64,
        }
        impl<'de> Deserialize<'de> for TonalColor {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where D: serde::Deserializer<'de> {
                let wire = TonalColorWire::deserialize(deserializer)?;
                Self::new(wire.semantic_version, [
                    wire.global_red,
                    wire.global_green,
                    wire.global_blue,
                    wire.shadows_red,
                    wire.shadows_green,
                    wire.shadows_blue,
                    wire.midtones_red,
                    wire.midtones_green,
                    wire.midtones_blue,
                    wire.highlights_red,
                    wire.highlights_green,
                    wire.highlights_blue,
                ]).map_err(serde::de::Error::custom)
            }
        }
        impl Default for TonalColor {
            fn default() -> Self {
                Self { semantic_version: TonalColorSemanticVersion::V1,
                    global_red: 0.0,
                    global_green: 0.0,
                    global_blue: 0.0,
                    shadows_red: 0.0,
                    shadows_green: 0.0,
                    shadows_blue: 0.0,
                    midtones_red: 0.0,
                    midtones_green: 0.0,
                    midtones_blue: 0.0,
                    highlights_red: 0.0,
                    highlights_green: 0.0,
                    highlights_blue: 0.0,
                }
            }
        }
        impl TonalColor {
            /// Stable scalar property/uniform order; no RGB-vector animation ABI is added.
            pub const CONTROL_NAMES: [&'static str; 12] = [
                "globalRed",
                "globalGreen",
                "globalBlue",
                "shadowsRed",
                "shadowsGreen",
                "shadowsBlue",
                "midtonesRed",
                "midtonesGreen",
                "midtonesBlue",
                "highlightsRed",
                "highlightsGreen",
                "highlightsBlue",
            ];
            /// Construct a finite bounded v1 operation in CONTROL_NAMES order.
            pub fn new(semantic_version: TonalColorSemanticVersion, controls: [f64; 12]) -> Result<Self, TonalColorError> {
                for (index, value) in controls.iter().copied().enumerate() {
                    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                        return Err(TonalColorError { control: Self::CONTROL_NAMES[index], value });
                    }
                }
                Ok(Self { semantic_version,
                    global_red: controls[0],
                    global_green: controls[1],
                    global_blue: controls[2],
                    shadows_red: controls[3],
                    shadows_green: controls[4],
                    shadows_blue: controls[5],
                    midtones_red: controls[6],
                    midtones_green: controls[7],
                    midtones_blue: controls[8],
                    highlights_red: controls[9],
                    highlights_green: controls[10],
                    highlights_blue: controls[11],
                })
            }
            /// Components in the stable property and renderer uniform order.
            #[must_use]
            pub fn controls(self) -> [f64; 12] { [
                self.global_red,
                self.global_green,
                self.global_blue,
                self.shadows_red,
                self.shadows_green,
                self.shadows_blue,
                self.midtones_red,
                self.midtones_green,
                self.midtones_blue,
                self.highlights_red,
                self.highlights_green,
                self.highlights_blue,
            ] }
            /// Exact bypass, before alpha handling or SDR containment.
            #[must_use]
            pub fn is_neutral(self) -> bool { self.controls().iter().all(|v| *v == 0.0) }
        }
        /// A nonfinite or out-of-range tonal-color component.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct TonalColorError { control: &'static str, value: f64 }
        impl fmt::Display for TonalColorError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "tonal-color {} {} must be finite and in -1..=1", self.control, self.value)
            }
        }
        impl std::error::Error for TonalColorError {}
    };
}
