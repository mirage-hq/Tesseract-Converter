use super::{map_animation_graph_error, set_tracks};
use crate::error::Result;
use fx_schema::{
    animator::{PropertyKeyframe, PropertyKeyframeEasing, PropertyKeyframeTrack},
    AnimationGraph, KeyframeId, LayerId, PropType, Property, PropertyAnimator, PropertyValue,
    TimeOffset,
};
use serde_json::json;

fn track(id: &str, value: f64) -> PropertyKeyframeTrack {
    PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
        KeyframeId::new(id),
        TimeOffset::from_millis(0),
        PropertyValue::Float(value),
        PropertyKeyframeEasing::Linear,
    )])
    .unwrap()
}

fn property(layer: u64, property_type: PropType) -> Property {
    Property::new(LayerId::new(layer), property_type)
}

fn legacy_set_tracks(
    dynamics: &mut AnimationGraph,
    tracks: Vec<(Property, PropertyKeyframeTrack)>,
) -> Result<()> {
    for (property, track) in tracks {
        dynamics
            .set_property(property, PropertyAnimator::keyframes(track), Vec::new())
            .map_err(map_animation_graph_error)?;
    }
    Ok(())
}

fn assert_matches_legacy(initial: AnimationGraph, tracks: Vec<(Property, PropertyKeyframeTrack)>) {
    let mut legacy = initial.clone();
    let mut batched = initial;
    let legacy_result = legacy_set_tracks(&mut legacy, tracks.clone());
    let batched_result = set_tracks(&mut batched, tracks);

    assert_eq!(
        legacy_result.as_ref().err().map(ToString::to_string),
        batched_result.as_ref().err().map(ToString::to_string)
    );
    assert_eq!(legacy.wire_value(), batched.wire_value());
}

#[test]
fn batched_tracks_match_legacy_for_disjoint_replaced_and_repeated_targets() {
    let first = property(1, PropType::PositionX);
    let second = property(2, PropType::Opacity);
    let tracks = vec![
        (first, track("first-old", 10.0)),
        (second, track("second", 20.0)),
        (first, track("first-new", 30.0)),
        (first, track("first-final", 40.0)),
    ];

    assert_matches_legacy(AnimationGraph::new(), tracks);
}

#[test]
fn batched_tracks_match_legacy_duplicate_key_error_and_partial_success() {
    let initial_property = property(1, PropType::PositionX);
    let mut initial = AnimationGraph::new();
    initial
        .set_property(
            initial_property,
            PropertyAnimator::keyframes(track("shared", 1.0)),
            Vec::new(),
        )
        .unwrap();
    let tracks = vec![
        (property(2, PropType::Opacity), track("prior-success", 2.0)),
        (property(3, PropType::ScaleX), track("shared", 3.0)),
    ];

    assert_matches_legacy(initial, tracks);
}

#[test]
fn batched_tracks_reject_an_invalid_intermediate_even_if_a_later_replacement_would_fix_it() {
    let first = property(1, PropType::PositionX);
    let repeated = property(2, PropType::Opacity);
    let mut initial = AnimationGraph::new();
    initial
        .set_property(
            first,
            PropertyAnimator::keyframes(track("shared", 1.0)),
            Vec::new(),
        )
        .unwrap();
    let tracks = vec![
        (repeated, track("shared", 2.0)),
        (repeated, track("replacement", 3.0)),
    ];

    assert_matches_legacy(initial, tracks);
}

#[test]
fn empty_batch_leaves_a_graph_without_entries_exactly_untouched() {
    let initial: AnimationGraph = serde_json::from_value(json!({"future": 1})).unwrap();

    assert_matches_legacy(initial, Vec::new());
}

#[test]
fn first_failed_insertion_leaves_a_graph_without_entries_exactly_untouched() {
    let initial: AnimationGraph = serde_json::from_value(json!({"future": 1})).unwrap();
    let tracks = vec![
        (property(1, PropType::ActiveRange), track("read-only", 1.0)),
        (property(2, PropType::Opacity), track("unreached", 2.0)),
    ];

    assert_matches_legacy(initial, tracks);
}

#[test]
fn batched_tracks_preserve_unknown_graph_entry_and_animator_fields() {
    let mut initial = AnimationGraph::new();
    initial
        .set_property(
            property(1, PropType::PositionX),
            PropertyAnimator::keyframes(track("existing", 1.0)),
            Vec::new(),
        )
        .unwrap();
    let mut wire = initial.wire_value().clone();
    wire["futureGraph"] = json!({"kept": true});
    wire["entries"][0]["futureEntry"] = json!([1, 2, 3]);
    wire["entries"][0]["animator"]["futureAnimator"] = json!("kept");
    let initial: AnimationGraph = serde_json::from_value(wire).unwrap();

    assert_matches_legacy(
        initial,
        vec![
            (property(2, PropType::Opacity), track("added", 2.0)),
            (property(3, PropType::Rotation), track("also-added", 3.0)),
        ],
    );
}
