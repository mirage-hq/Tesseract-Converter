//! Typed controls of the measured default Pop; geometry stays explicitly opt-in.
use super::film_impact::{
    self, point, popup, read_profile_with, scalar, ui, version, ProfileParam, Rule,
};
use crate::{
    error::Result,
    format::{Graph, Record},
};

// Native Film Impact component 1607. UI expansion is inert; every rendered control
// retains its measured default. The private curve payload must remain empty.
const PROFILE: [ProfileParam; 43] = [
    film_impact::ERROR_OCCURRED,
    film_impact::TRANSITION_TIMING_8120,
    film_impact::START,
    film_impact::END,
    film_impact::TRANSITION_TIMING_8121,
    popup("5", "Presets", 0.0, "7"),
    ui("38", Some("Controls"), "11"),
    film_impact::CONTROL_8040,
    scalar(
        "45",
        Some("Seed"),
        Rule::Number(0.0),
        Some("0"),
        Some("99999"),
    ),
    film_impact::VISUAL_CURVE_EDITOR_8020,
    film_impact::CURVE_GRAPH,
    popup("8", "Type", 1.0, "2"),
    scalar(
        "9",
        Some("Bounces"),
        Rule::Number(3.0),
        Some("1"),
        Some("20"),
    ),
    scalar(
        "10",
        Some("Initial Velocity"),
        Rule::Number(8.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "11",
        Some("Elasticity"),
        Rule::Number(10.0),
        Some("0"),
        Some("50"),
    ),
    scalar(
        "12",
        Some("Frequency"),
        Rule::Number(2.0),
        Some("1"),
        Some("20"),
    ),
    scalar(
        "13",
        Some("Amplitude"),
        Rule::Number(60.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "14",
        Some("Decay"),
        Rule::Number(100.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "37",
        Some("Prevent Undershoot"),
        Rule::Boolean(false),
        None,
        None,
    ),
    scalar(
        "35",
        Some("Ease In"),
        Rule::Number(0.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "36",
        Some("Ease Out"),
        Rule::Number(70.0),
        Some("0"),
        Some("100"),
    ),
    scalar(
        "54",
        Some("Fade"),
        Rule::Number(25.0),
        Some("0"),
        Some("100"),
    ),
    film_impact::VISUAL_CURVE_EDITOR_8021,
    ui("40", Some("Animation Controls"), "11"),
    point("26", "Origin", [0.0, 0.0]),
    ui("41", Some("Animation Controls"), "12"),
    ui("42", Some("Motion Blur Engine"), "11"),
    scalar(
        "19",
        Some("Enable Motion Blur"),
        Rule::Boolean(true),
        None,
        None,
    ),
    scalar(
        "20",
        Some("Motion Blur"),
        Rule::Number(45.0),
        Some("0"),
        Some("100"),
    ),
    ui("43", Some("Motion Blur Engine"), "12"),
    ui("39", Some("Controls"), "12"),
    film_impact::OVERLAY_MODE,
    film_impact::OVERLAY_INFO,
    film_impact::CONTROL_8141,
    version("47"),
    ProfileParam {
        control: Some("1"),
        ..scalar(
            "27",
            Some("_ Old Applied Version"),
            Rule::Range {
                min: 0.0,
                max: 99.0,
            },
            Some("0"),
            Some("99"),
        )
    },
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
    read_profile_with(graph, record, "AE.AE_Impact_Pop", &PROFILE, omissions).map(|_| ())
}
