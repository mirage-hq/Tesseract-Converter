//! Typed controls of the measured default Dissolve, not a general Ease law.
use super::film_impact::{
    self, point, popup, read_profile_with, scalar, ui, version, ProfileParam, Rule,
};
use crate::{
    error::Result,
    format::{Graph, Record},
};

// Native Film Impact component 1610: all 27 observed profiles have these controls.
const PROFILE: [ProfileParam; 30] = [
    film_impact::ERROR_OCCURRED,
    film_impact::TRANSITION_TIMING_8120,
    film_impact::START,
    film_impact::END,
    film_impact::TRANSITION_TIMING_8121,
    ui("19", Some("Controls"), "11"),
    film_impact::CONTROL_8040,
    scalar(
        "22",
        Some("Seed"),
        Rule::Number(0.0),
        Some("0"),
        Some("99999"),
    ),
    film_impact::VISUAL_CURVE_EDITOR_8020,
    film_impact::CURVE_GRAPH,
    scalar(
        "16",
        Some("Ease In"),
        Rule::Number(34.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "17",
        Some("Ease Out"),
        Rule::Number(34.0),
        Some("0"),
        Some("100"),
    ),
    film_impact::VISUAL_CURVE_EDITOR_8021,
    popup("5", "Type", 0.0, "4"),
    popup("34", "Type", 0.0, "3"),
    scalar(
        "31",
        Some("Focus Amount"),
        Rule::Number(0.0),
        Some("0"),
        Some("100"),
    ),
    point("32", "Focus Position", [0.5, 0.5]),
    scalar("33", Some("Invert Focus"), Rule::Boolean(false), None, None),
    ui("20", Some("Controls"), "12"),
    film_impact::OVERLAY_MODE,
    film_impact::OVERLAY_INFO,
    film_impact::CONTROL_8141,
    version("24"),
    film_impact::CONTROL_8300,
    film_impact::CONTROL_8301,
    film_impact::SOURCE_B_LAYER,
    film_impact::OVERLAY_ENABLED,
    film_impact::SEQUENCE_WIDTH,
    film_impact::SEQUENCE_HEIGHT,
    film_impact::SEQUENCE_PIXEL_RATIO,
];

pub(super) fn read_profile(
    graph: &Graph<'_>,
    record: Record<'_>,
    omissions: &mut Vec<crate::Omission>,
) -> Result<()> {
    read_profile_with(graph, record, "AE.AE_Impact_Dissolve", &PROFILE, omissions).map(|_| ())
}
