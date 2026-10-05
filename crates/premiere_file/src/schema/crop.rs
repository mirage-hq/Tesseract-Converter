//! Supported static Premiere Crop effect layouts: the Premiere 26.3 layout that
//! the reader and writer share, and the Premiere 26.5.1 layout that only the
//! reader accepts.

use crate::format::{ensure_valid, Result};

/// Premiere's static Crop effect, expressed as edge percentages and pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct PrStaticCrop {
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) right: f64,
    pub(crate) bottom: f64,
    pub(crate) edge_feather: f64,
}

impl PrStaticCrop {
    pub(crate) fn validate(self) -> Result<()> {
        ensure_valid!(
            [self.left, self.top, self.right, self.bottom]
                .iter()
                .all(|value| value.is_finite() && (0.0..=100.0).contains(value)),
            "Crop edge percentages must be finite and within 0..=100"
        );
        ensure_valid!(
            self.left + self.right < 100.0 && self.top + self.bottom < 100.0,
            "opposing Crop edges must leave a positive visible area"
        );
        ensure_valid!(
            self.edge_feather.is_finite() && (-30_000.0..=30_000.0).contains(&self.edge_feather),
            "Crop Edge Feather must be finite and within -30000..=30000"
        );
        Ok(())
    }

    pub(crate) fn is_default(self) -> bool {
        self == Self::default()
    }
}

/// One Crop parameter record of a layout. A `None` field is absent from the
/// record.
pub(crate) struct CropParamSpec {
    pub(crate) id: usize,
    pub(crate) name: &'static str,
    pub(crate) class_id: &'static str,
    pub(crate) control: Option<&'static str>,
    pub(crate) lower: Option<&'static str>,
    pub(crate) upper: Option<&'static str>,
    pub(crate) lower_ui: Option<&'static str>,
    pub(crate) upper_ui: Option<&'static str>,
    pub(crate) is_time_varying: Option<&'static str>,
}

pub(crate) const CROP_PARAM_COUNT: usize = 6;

/// Exact static layout observed in the pinned Adobe-authored Crop sources.
pub(crate) const CROP_PARAMS: [CropParamSpec; CROP_PARAM_COUNT] = [
    CropParamSpec {
        id: 1,
        name: "Left",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: Some("2"),
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: Some("false"),
    },
    CropParamSpec {
        id: 2,
        name: "Top",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: Some("2"),
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: Some("false"),
    },
    CropParamSpec {
        id: 3,
        name: "Right",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: Some("2"),
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: Some("false"),
    },
    CropParamSpec {
        id: 4,
        name: "Bottom",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: Some("2"),
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: Some("false"),
    },
    CropParamSpec {
        id: 5,
        name: " ",
        class_id: "cc12343e-f113-4d3b-ae05-b287db77d461",
        control: Some("4"),
        lower: Some("false"),
        upper: Some("true"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: Some("false"),
    },
    CropParamSpec {
        id: 6,
        name: "Edge Feather",
        class_id: "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
        control: Some("1"),
        lower: Some("-30000"),
        upper: Some("30000"),
        lower_ui: Some("-100"),
        upper_ui: Some("100"),
        is_time_varying: Some("false"),
    },
];

/// The Premiere 26.5.1 layout, which only the reader accepts: the ids, names, ClassIDs and bounds of
/// [`CROP_PARAMS`], with `ParameterControlType` only on Edge Feather, no bounds
/// on the Zoom checkbox (id 5) and no `IsTimeVarying`. Its component has no
/// `Bypass` or `Intrinsic`.
pub(crate) const CROP_PARAMS_26_5: [CropParamSpec; CROP_PARAM_COUNT] = [
    CropParamSpec {
        id: 1,
        name: "Left",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: None,
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: None,
    },
    CropParamSpec {
        id: 2,
        name: "Top",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: None,
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: None,
    },
    CropParamSpec {
        id: 3,
        name: "Right",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: None,
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: None,
    },
    CropParamSpec {
        id: 4,
        name: "Bottom",
        class_id: "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
        control: None,
        lower: Some("0"),
        upper: Some("100"),
        lower_ui: None,
        upper_ui: None,
        is_time_varying: None,
    },
    CropParamSpec {
        id: 5,
        name: " ",
        class_id: "cc12343e-f113-4d3b-ae05-b287db77d461",
        control: None,
        lower: None,
        upper: None,
        lower_ui: None,
        upper_ui: None,
        is_time_varying: None,
    },
    CropParamSpec {
        id: 6,
        name: "Edge Feather",
        class_id: "6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8",
        control: Some("1"),
        lower: Some("-30000"),
        upper: Some("30000"),
        lower_ui: Some("-100"),
        upper_ui: Some("100"),
        is_time_varying: None,
    },
];

#[cfg(test)]
mod tests {
    use super::{PrStaticCrop, CROP_PARAMS};

    #[test]
    fn written_layout_has_every_field_the_writer_emits() {
        // `crop_records` writes the 26.3 layout: a `None` here would silently
        // drop an element that the reader then requires.
        for spec in &CROP_PARAMS {
            assert!(
                spec.control.is_some() && spec.lower.is_some() && spec.upper.is_some(),
                "{}",
                spec.name
            );
            // The writer writes `IsTimeVarying` false on every parameter.
            assert_eq!(spec.is_time_varying, Some("false"), "{}", spec.name);
        }
    }

    #[test]
    fn crop_bounds_reject_empty_or_nonfinite_geometry_and_out_of_range_feather() {
        for crop in [
            PrStaticCrop {
                left: 50.0,
                right: 50.0,
                ..PrStaticCrop::default()
            },
            PrStaticCrop {
                top: f64::NAN,
                ..PrStaticCrop::default()
            },
            PrStaticCrop {
                edge_feather: -30_001.0,
                ..PrStaticCrop::default()
            },
            PrStaticCrop {
                edge_feather: 30_001.0,
                ..PrStaticCrop::default()
            },
        ] {
            assert!(crop.validate().is_err());
        }
        for edge_feather in [-30_000.0, -1.0, 0.0, 30_000.0] {
            assert!(PrStaticCrop {
                edge_feather,
                ..PrStaticCrop::default()
            }
            .validate()
            .is_ok());
        }
    }
}
