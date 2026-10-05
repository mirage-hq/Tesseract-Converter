use super::*;
use crate::adapter::export::script_bake::document::BakedDocument;

#[test]
fn encoded_replacement_matches_value_pipeline_and_preserves_unrelated_fields() {
    let original = modify(&document("return input.time.milliseconds;"), |raw| {
        raw["futureEnvelope"] = json!({"integer": u64::MAX, "values": [null, 1.25]});
        raw["composition"]["futureComposition"] = json!({"enabled": false});
        raw["composition"]["layers"][0]["futureLayer"] = json!(["retain", {"nested": 17}]);
        let graph = &mut raw["composition"]["dynamics"];
        graph["futureGraph"] = json!({"value": [1, 2, 3]});
        graph["entries"][0]["futureEntry"] = json!({"retain": true});
        graph["entries"][0]["animator"]["futureAnimator"] = json!("replaced, as before");
        graph["entries"].as_array_mut().unwrap().push(json!({
            "target": {"kind": "layer", "layerId": 7, "propertyType": "positionY"},
            "animator": {"type": "jsScript", "layerTimeJsCode": "throw 'unsupported';", "futureAnimator": [null, 42]},
            "futureEntry": {"untouched": true}
        }));
    });
    let before = original.to_json_value().unwrap();
    let animator = PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
            fx_schema::KeyframeId::new("encoded-x"),
            TimeOffset::from_millis(0),
            PropertyValue::Float(12.5),
            PropertyKeyframeEasing::Hold,
        )])
        .unwrap(),
    );
    let mut expected = before.clone();
    expected["composition"]["dynamics"]["entries"][0]["animator"] = animator.known_value();
    let expected = EditableFxCompositionDocument::from_json_value(expected).unwrap();
    let mut encoded = BakedDocument::new(&original).unwrap();
    encoded.replace_animator(0, &animator).unwrap();
    assert_eq!(
        encoded.finish().unwrap().to_json_value().unwrap(),
        expected.to_json_value().unwrap()
    );
    assert_eq!(original.to_json_value().unwrap(), before);
}

#[test]
fn encoded_compound_path_keys_match_the_previous_document_reader() {
    let original = modify(&document("return {commands:[]};"), |raw| {
        raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] = json!("shapePath");
    });
    let commands: Vec<_> = (0..64)
        .flat_map(|index| {
            [
                json!({"type": "moveTo", "x": index, "y": 0}),
                json!({"type": "lineTo", "x": index + 1, "y": 2}),
                json!({"type": "close"}),
            ]
        })
        .collect();
    let keys: Vec<_> = (0..32)
        .map(|index| {
            json!({
                "id": format!("encoded-path-{index}"), "layerTime": index * 3,
                "value": {"type": "path", "value": {"commands": commands}},
                "easing": {"type": "hold"}
            })
        })
        .collect();
    let animator: PropertyAnimator = serde_json::from_value(json!({
        "type": "keyframes", "enabled": true, "keyframes": keys
    }))
    .unwrap();
    let mut expected = original.to_json_value().unwrap();
    expected["composition"]["dynamics"]["entries"][0]["animator"] = animator.known_value();
    let expected = EditableFxCompositionDocument::from_json_value(expected).unwrap();
    let mut encoded = BakedDocument::new(&original).unwrap();
    encoded.replace_animator(0, &animator).unwrap();
    assert_eq!(
        encoded.finish().unwrap().to_json_value().unwrap(),
        expected.to_json_value().unwrap()
    );
}

#[test]
fn encoded_replacement_still_rejects_invalid_final_graphs() {
    let original = modify(&document("return 1;"), |raw| {
        let mut second = raw["composition"]["dynamics"]["entries"][0].clone();
        second["target"]["propertyType"] = json!("positionY");
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(second);
    });
    let animator = PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
            fx_schema::KeyframeId::new("duplicate-key"),
            TimeOffset::from_millis(0),
            PropertyValue::Float(1.0),
            PropertyKeyframeEasing::Hold,
        )])
        .unwrap(),
    );
    let mut encoded = BakedDocument::new(&original).unwrap();
    encoded.replace_animator(0, &animator).unwrap();
    encoded.replace_animator(1, &animator).unwrap();
    let error = encoded.finish().unwrap_err().to_string();
    assert!(error.contains("duplicate-key"), "{error}");
}
