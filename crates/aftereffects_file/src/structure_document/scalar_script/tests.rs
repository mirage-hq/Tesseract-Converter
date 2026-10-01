use super::*;
use crate::properties::{NumericKeyframe, NumericValueKind};

fn numeric() -> NumericProperty {
    NumericProperty {
        values: vec![37.0, 42.0],
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        value_kind: NumericValueKind::Continuous,
        keyframes: [0.0, 2.0]
            .into_iter()
            .map(|t| NumericKeyframe {
                time_secs: t,
                values: vec![100.0 + t * 50.0, 80.0],
                in_interpolation: 1,
                out_interpolation: 1,
                in_speed: vec![],
                out_speed: vec![],
                in_influence: vec![],
                out_influence: vec![],
                spatial_in: vec![],
                spatial_out: vec![],
            })
            .collect(),
    }
}

#[test]
fn scalar_channel_retains_keys_above_former_count_limit() {
    let mut numeric = numeric();
    numeric.keyframes = (0..10_001)
        .map(|index| {
            let mut key = numeric.keyframes[0].clone();
            key.time_secs = f64::from(index);
            key.values[0] = f64::from(index);
            key
        })
        .collect();
    let mut warnings = Vec::new();
    let channel = ScalarChannel::decode("size", &numeric, 0, 37.0, &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(channel.keys.len(), 10_001);
    assert_eq!(channel.keys.last().unwrap().v, 10_000.0);
}

#[test]
fn malformed_channel_keeps_base_without_disabling_valid_sibling() {
    let mut numeric = numeric();
    numeric.keyframes[1].values[0] = f64::NAN;
    let mut warnings = Vec::new();
    let broken = ScalarChannel::decode("size", &numeric, 0, 37.0, &mut warnings);
    let valid = ScalarChannel::decode("size", &numeric, 1, 42.0, &mut warnings);
    assert!(!broken.animated());
    assert_eq!(broken.base, 37.0);
    assert!(valid.animated());
    assert_eq!(valid.keys.len(), 2);
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("non-finite"))
    );
}

#[test]
fn expressions_and_nonascending_keys_do_not_enter_generated_code() {
    let mut numeric = numeric();
    let mut warnings = Vec::new();
    numeric.expression_enabled = true;
    assert!(!ScalarChannel::decode("size", &numeric, 0, 37.0, &mut warnings).animated());
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("expression is not executed"))
    );
    numeric.expression_enabled = false;
    numeric.keyframes[1].time_secs = numeric.keyframes[0].time_secs;
    assert!(!ScalarChannel::decode("size", &numeric, 0, 37.0, &mut warnings).animated());
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("nonascending"))
    );
}
