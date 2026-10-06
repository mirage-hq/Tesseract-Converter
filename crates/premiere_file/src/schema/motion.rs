//! Intrinsic Motion parameter layouts: the Premiere 26.3 layout that the XML
//! reader and writer share, and the Premiere 26.5 layout that only the reader accepts.

use super::{
    records::{self, XmlRecordDefinition},
    PrAnimatedProperty,
};

pub(crate) struct MotionParamSpec {
    pub id: usize,
    pub name: &'static str,
    pub record: XmlRecordDefinition,
    pub control: Option<&'static str>,
    pub initial: &'static str,
    pub bounds: Option<(&'static str, &'static str, Option<&'static str>)>,
    pub animation: Option<PrAnimatedProperty>,
}

impl MotionParamSpec {
    pub(crate) fn is_point(&self) -> bool {
        self.record.tag == records::POINT_COMPONENT_PARAM.tag
    }

    /// The edge that Motion Crop Left, Top, Right or Bottom crops, as an index
    /// into `[left, top, right, bottom]` (the edge order of `PrStaticCrop`);
    /// the modern layout uses them, older eleven-control saves keep them inert.
    pub(crate) fn crop_edge(&self) -> Option<usize> {
        self.id.checked_sub(8).filter(|edge| *edge < 4)
    }

    /// Whether `value` is finite and inside this parameter's native bounds.
    pub(crate) fn holds(&self, value: f64) -> bool {
        let bound = |bound: &str| bound.parse::<f64>().ok();
        value.is_finite()
            && self.bounds.is_none_or(|(lower, upper, _)| {
                bound(lower).is_none_or(|lower| value >= lower)
                    && bound(upper).is_none_or(|upper| value <= upper)
            })
    }

    /// Match static numeric defaults without imposing the writer's number formatting.
    pub(crate) fn accepts_default(&self, value: &str) -> bool {
        if self.is_point() || self.initial == "true" {
            value == self.initial
        } else {
            value.parse::<f64>().ok() == self.initial.parse::<f64>().ok()
        }
    }
}

pub(crate) const MOTION_PARAM_COUNT: usize = 7;

pub(crate) const MOTION_PARAMS: [MotionParamSpec; MOTION_PARAM_COUNT] = [
    MotionParamSpec {
        id: 1,
        name: "Position",
        record: records::POINT_COMPONENT_PARAM,
        control: Some("6"),
        initial: "0.5:0.5",
        bounds: None,
        animation: Some(PrAnimatedProperty::Position),
    },
    MotionParamSpec {
        id: 2,
        name: "Scale",
        record: records::VIDEO_COMPONENT_PARAM,
        control: Some("2"),
        initial: "100.",
        bounds: Some(("0", "10000", Some("200"))),
        animation: Some(PrAnimatedProperty::UniformScale),
    },
    MotionParamSpec {
        id: 3,
        name: "Scale Width",
        record: records::VIDEO_COMPONENT_PARAM,
        control: Some("2"),
        initial: "100.",
        bounds: Some(("0", "10000", Some("200"))),
        animation: Some(PrAnimatedProperty::ScaleWidth),
    },
    MotionParamSpec {
        id: 4,
        name: " ",
        record: records::VIDEO_BOOL_COMPONENT_PARAM,
        control: Some("4"),
        initial: "true",
        bounds: Some(("false", "true", None)),
        animation: None,
    },
    MotionParamSpec {
        id: 5,
        name: "Rotation",
        record: records::VIDEO_COMPONENT_PARAM,
        control: Some("3"),
        initial: "0.",
        bounds: Some(("-32768", "32767", None)),
        animation: Some(PrAnimatedProperty::Rotation),
    },
    MotionParamSpec {
        id: 6,
        name: "Anchor Point",
        record: records::POINT_COMPONENT_PARAM,
        control: Some("6"),
        initial: "0.5:0.5",
        bounds: None,
        animation: Some(PrAnimatedProperty::AnchorPoint),
    },
    MotionParamSpec {
        id: 7,
        name: "Anti-flicker Filter",
        record: records::VIDEO_FILTER_AMOUNT_PARAM,
        control: Some("8"),
        initial: "0.",
        bounds: Some(("0", "1", None)),
        animation: None,
    },
];

// Premiere 26.5 writes the parameter classes of the 26.3 layout one version later.
const SCALAR_26_5: XmlRecordDefinition = XmlRecordDefinition::new(
    records::VIDEO_COMPONENT_PARAM.tag,
    records::VIDEO_COMPONENT_PARAM.class_id,
    "10",
);
const BOOL_26_5: XmlRecordDefinition = XmlRecordDefinition::new(
    records::VIDEO_BOOL_COMPONENT_PARAM.tag,
    records::VIDEO_BOOL_COMPONENT_PARAM.class_id,
    "10",
);
const FILTER_AMOUNT_26_5: XmlRecordDefinition = XmlRecordDefinition::new(
    records::VIDEO_FILTER_AMOUNT_PARAM.tag,
    records::VIDEO_FILTER_AMOUNT_PARAM.class_id,
    "10",
);
const POINT_26_5: XmlRecordDefinition = XmlRecordDefinition::new(
    records::POINT_COMPONENT_PARAM.tag,
    records::POINT_COMPONENT_PARAM.class_id,
    "4",
);

/// The Premiere 26.5 layout: the ids, names, defaults and animations of
/// [`MOTION_PARAMS`], a control type on Rotation only, no bounds on the
/// uniform-scale flag, and Motion Crop Left, Top, Right and Bottom as ids 8-11.
pub(crate) const MOTION_PARAMS_26_5: [MotionParamSpec; 11] = [
    MotionParamSpec {
        id: 1,
        name: "Position",
        record: POINT_26_5,
        control: None,
        initial: "0.5:0.5",
        bounds: None,
        animation: Some(PrAnimatedProperty::Position),
    },
    MotionParamSpec {
        id: 2,
        name: "Scale",
        record: SCALAR_26_5,
        control: None,
        initial: "100.",
        bounds: Some(("0", "10000", Some("200"))),
        animation: Some(PrAnimatedProperty::UniformScale),
    },
    MotionParamSpec {
        id: 3,
        name: "Scale Width",
        record: SCALAR_26_5,
        control: None,
        initial: "100.",
        bounds: Some(("0", "10000", Some("200"))),
        animation: Some(PrAnimatedProperty::ScaleWidth),
    },
    MotionParamSpec {
        id: 4,
        name: " ",
        record: BOOL_26_5,
        control: None,
        initial: "true",
        bounds: None,
        animation: None,
    },
    MotionParamSpec {
        id: 5,
        name: "Rotation",
        record: SCALAR_26_5,
        control: Some("3"),
        initial: "0.",
        bounds: Some(("-32768", "32767", None)),
        animation: Some(PrAnimatedProperty::Rotation),
    },
    MotionParamSpec {
        id: 6,
        name: "Anchor Point",
        record: POINT_26_5,
        control: None,
        initial: "0.5:0.5",
        bounds: None,
        animation: Some(PrAnimatedProperty::AnchorPoint),
    },
    MotionParamSpec {
        id: 7,
        name: "Anti-flicker Filter",
        record: FILTER_AMOUNT_26_5,
        control: None,
        initial: "0.",
        bounds: Some(("0", "1", None)),
        animation: None,
    },
    MotionParamSpec {
        id: 8,
        name: "Crop Left",
        record: SCALAR_26_5,
        control: None,
        initial: "0.",
        bounds: Some(("0", "100", None)),
        animation: None,
    },
    MotionParamSpec {
        id: 9,
        name: "Crop Top",
        record: SCALAR_26_5,
        control: None,
        initial: "0.",
        bounds: Some(("0", "100", None)),
        animation: None,
    },
    MotionParamSpec {
        id: 10,
        name: "Crop Right",
        record: SCALAR_26_5,
        control: None,
        initial: "0.",
        bounds: Some(("0", "100", None)),
        animation: None,
    },
    MotionParamSpec {
        id: 11,
        name: "Crop Bottom",
        record: SCALAR_26_5,
        control: None,
        initial: "0.",
        bounds: Some(("0", "100", None)),
        animation: None,
    },
];

#[cfg(test)]
mod tests {
    use super::{MOTION_PARAMS, MOTION_PARAMS_26_5, MOTION_PARAM_COUNT};
    use std::collections::BTreeSet;

    #[test]
    fn motion_parameter_layout_is_dense_and_defaults_are_compatible() {
        let ids: BTreeSet<_> = MOTION_PARAMS.iter().map(|spec| spec.id).collect();
        assert_eq!(ids, (1..=7).collect());
        for spec in &MOTION_PARAMS {
            assert!(spec.accepts_default(spec.initial));
            assert!(!spec.accepts_default("not a default"));
            assert_eq!(spec.is_point(), matches!(spec.id, 1 | 6));
            assert_eq!(
                spec.animation.is_some(),
                matches!(spec.id, 1 | 2 | 3 | 5 | 6)
            );
        }
        let scale = &MOTION_PARAMS[1];
        assert!(scale.accepts_default("100"));
        assert!(scale.accepts_default("100.0"));
        assert!(!scale.accepts_default("101"));
    }

    #[test]
    fn premiere_26_5_layout_keeps_the_26_3_mapping_and_appends_motion_crop() {
        let ids: Vec<_> = MOTION_PARAMS_26_5.iter().map(|spec| spec.id).collect();
        assert_eq!(ids, (1..=11).collect::<Vec<_>>());
        // The reader maps ids 1-7 of both layouts with the same code.
        for (layout_26_3, layout_26_5) in MOTION_PARAMS.iter().zip(&MOTION_PARAMS_26_5) {
            assert_eq!(
                (
                    layout_26_5.name,
                    layout_26_5.record.tag,
                    layout_26_5.initial,
                    layout_26_5.animation
                ),
                (
                    layout_26_3.name,
                    layout_26_3.record.tag,
                    layout_26_3.initial,
                    layout_26_3.animation
                )
            );
        }
        let crop_edges: Vec<_> = MOTION_PARAMS_26_5
            .iter()
            .filter_map(|spec| Some((spec.crop_edge()?, spec.name)))
            .collect();
        assert_eq!(
            crop_edges,
            [
                (0, "Crop Left"),
                (1, "Crop Top"),
                (2, "Crop Right"),
                (3, "Crop Bottom")
            ]
        );
        for spec in &MOTION_PARAMS_26_5 {
            assert_eq!(spec.crop_edge().is_some(), spec.id > MOTION_PARAM_COUNT);
        }
        assert!(MOTION_PARAMS.iter().all(|spec| spec.crop_edge().is_none()));
    }
}
