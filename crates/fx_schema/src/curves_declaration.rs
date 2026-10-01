//! Shared validated curve wire declarations for lossless and strict readers.

/// Instantiate the same curve fields with an optional strict known-field policy.
#[doc(hidden)]
#[macro_export]
macro_rules! define_color_curves_schema {
    ($($strict:meta)?) => {
    /// Maximum number of control points in one version-1 curve.
    pub const MAX_COLOR_CURVE_POINTS: usize = 32;

    /// Version fixing the curve encoding, interpolation, order, and alpha policy.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    #[serde(transparent)]
    pub struct ColorCurvesSemanticVersion(u32);

    impl ColorCurvesSemanticVersion {
        /// The only supported curves semantics.
        pub const V1: Self = Self(1);
    }

    impl<'de> Deserialize<'de> for ColorCurvesSemanticVersion {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let value = u32::deserialize(deserializer)?;
            if value == Self::V1.0 {
                Ok(Self::V1)
            } else {
                Err(serde::de::Error::custom(format!(
                    "unsupported color curves semanticVersion {value}; expected 1"
                )))
            }
        }
    }

    /// One normalized control point in a color curve.
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
    #[serde(rename_all = "camelCase" $(, $strict)?)]
    #[ts(export_to = "project_types.d.ts")]
    pub struct ColorCurvePoint {
        x: f64,
        y: f64,
    }

    impl ColorCurvePoint {
        /// Construct a point. Full curve validation occurs in [`ColorCurve::new`].
        #[must_use]
        pub const fn new(x: f64, y: f64) -> Self {
            Self { x, y }
        }

        /// Normalized input coordinate.
        #[must_use]
        pub const fn x(self) -> f64 {
            self.x
        }

        /// Normalized output coordinate.
        #[must_use]
        pub const fn y(self) -> f64 {
            self.y
        }
    }

    /// A validated normalized piecewise-linear curve.
    #[derive(Debug, Clone, PartialEq, Serialize, TS)]
    #[serde(transparent)]
    #[ts(export_to = "project_types.d.ts")]
    pub struct ColorCurve(Vec<ColorCurvePoint>);

    impl ColorCurve {
        /// Validate and construct a version-1 curve.
        pub fn new(points: Vec<ColorCurvePoint>) -> Result<Self, ColorCurveError> {
            if !(2..=MAX_COLOR_CURVE_POINTS).contains(&points.len()) {
                return Err(ColorCurveError::PointCount(points.len()));
            }
            for (index, point) in points.iter().enumerate() {
                if !point.x.is_finite() || !point.y.is_finite() {
                    return Err(ColorCurveError::NonFinite { index });
                }
                if !(0.0..=1.0).contains(&point.x) || !(0.0..=1.0).contains(&point.y) {
                    return Err(ColorCurveError::OutOfRange { index });
                }
                // GPU interpolation uses f32. Reject pairs that collapse to one
                // x-coordinate after conversion, or the shader would divide by zero.
                if index > 0 && (point.x as f32) <= (points[index - 1].x as f32) {
                    return Err(ColorCurveError::NonIncreasingX { index });
                }
            }
            if points[0].x != 0.0 || points[points.len() - 1].x != 1.0 {
                return Err(ColorCurveError::Endpoints);
            }
            Ok(Self(points))
        }

        /// The canonical identity curve.
        #[must_use]
        pub fn identity() -> Self {
            Self(vec![
                ColorCurvePoint::new(0.0, 0.0),
                ColorCurvePoint::new(1.0, 1.0),
            ])
        }

        /// Borrow the validated points.
        #[must_use]
        pub fn points(&self) -> &[ColorCurvePoint] {
            &self.0
        }

        /// Whether every segment lies exactly on `y = x`.
        #[must_use]
        pub fn is_identity(&self) -> bool {
            self.0.iter().all(|point| point.x == point.y)
        }
    }

    impl<'de> Deserialize<'de> for ColorCurve {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let points = Vec::<ColorCurvePoint>::deserialize(deserializer)?;
            Self::new(points).map_err(serde::de::Error::custom)
        }
    }

    /// Invalid version-1 curve payload.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ColorCurveError {
        /// Curves require two through 32 points.
        PointCount(usize),
        /// A coordinate is NaN or infinite.
        NonFinite { index: usize },
        /// A coordinate lies outside the normalized interval.
        OutOfRange { index: usize },
        /// Point x coordinates are not strictly increasing.
        NonIncreasingX { index: usize },
        /// First/last x coordinates are not exactly zero/one.
        Endpoints,
    }

    impl fmt::Display for ColorCurveError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::PointCount(count) => write!(
                    formatter,
                    "color curve must contain 2..={MAX_COLOR_CURVE_POINTS} points, got {count}"
                ),
                Self::NonFinite { index } => {
                    write!(formatter, "color curve point {index} must be finite")
                }
                Self::OutOfRange { index } => write!(
                    formatter,
                    "color curve point {index} coordinates must be in 0..=1"
                ),
                Self::NonIncreasingX { index } => write!(
                    formatter,
                    "color curve point {index} x must be greater than the previous x"
                ),
                Self::Endpoints => {
                    formatter.write_str("color curve endpoint x coordinates must be exactly 0 and 1")
                }
            }
        }
    }

    impl std::error::Error for ColorCurveError {}

    pub(crate) fn has_unsupported_curves_semantics(payload: &serde_json::Value) -> bool {
        payload
            .get("semanticVersion")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|version| version != 1)
    }

    /// Four curves evaluated as Master followed by the matching RGB channel.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
    #[serde(rename_all = "camelCase" $(, $strict)?)]
    #[ts(export_to = "project_types.d.ts")]
    pub struct ColorCurves {
        /// Shared curve evaluated first on all channels.
        pub master: ColorCurve,
        /// Red-only curve evaluated after Master.
        pub red: ColorCurve,
        /// Green-only curve evaluated after Master.
        pub green: ColorCurve,
        /// Blue-only curve evaluated after Master.
        pub blue: ColorCurve,
    }

    impl ColorCurves {
        /// Whether all four curves are mathematically exact identities.
        #[must_use]
        pub fn is_identity(&self) -> bool {
            self.master.is_identity()
                && self.red.is_identity()
                && self.green.is_identity()
                && self.blue.is_identity()
        }
    }

    };
}
