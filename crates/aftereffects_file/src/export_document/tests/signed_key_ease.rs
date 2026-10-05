use super::*;
use fx_schema::animator::{
    AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
};

#[test]
fn canonical_signed_short_opacity_keys_keep_edited_normalized_ease() {
    for fps in [24.0, 29.97] {
        for end_value in [100.0, 40.0] {
            let mut value = imported();
            value["duration"] = json!(1.0);
            value["composition"]["layers"] = json!([rect(&value, 9901)]);
            let track = PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    KeyframeId::new("first"),
                    fx_schema::TimeOffset::from_millis(-1),
                    PropertyValue::Float(0.0),
                    PropertyKeyframeEasing::Linear,
                ),
                PropertyKeyframe::new(
                    KeyframeId::new("edited"),
                    fx_schema::TimeOffset::from_millis(1),
                    PropertyValue::Float(end_value),
                    PropertyKeyframeEasing::CubicBezier {
                        x1: 0.25,
                        y1: 0.25,
                        x2: 0.75,
                        y2: 0.75,
                    },
                ),
            ])
            .unwrap();
            value["composition"]["dynamics"]["entries"] = json!([AnimationGraphEntry {
                target: fx_schema::PropertyTarget::layer(LayerId::new(9901), PropType::Opacity),
                animator: PropertyAnimator::keyframes(track),
                dependencies: vec![],
                random_seed_target: None,
                layer_refs: Default::default(),
            }]);
            let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
            let output = to_aep_with_fps(&document, fps).unwrap();
            let native = read_project(&output.bytes).unwrap();
            let layer = layers(&native)
                .iter()
                .find(|layer| layer.name.as_ref() == "Current solid 9901")
                .unwrap();
            let opacity = super::stroke_keys::numeric(&layer.content, "ADBE Opacity").unwrap();
            assert_eq!(opacity.keyframes.len(), 2, "{:?}", output.diagnostics);
            let first = &opacity.keyframes[0];
            let last = &opacity.keyframes[1];
            assert!(first.time_secs < 0.0 && last.time_secs > 0.0);
            assert!((last.values[0] - end_value / 100.0).abs() < 1e-12);
            let delta = last.values[0] - first.values[0];
            let duration = last.time_secs - first.time_secs;
            // Native speeds must reconstruct the authored normalized Y handles
            // on the exact saved tick span, including after an input value edit.
            assert!(
                (first.out_speed[0] * duration / delta * first.out_influence[0] / 100.0 - 0.25)
                    .abs()
                    < 1e-10
            );
            assert!(
                (last.in_speed[0] * duration / delta * last.in_influence[0] / 100.0 - 0.25).abs()
                    < 1e-10
            );
        }
    }
}
