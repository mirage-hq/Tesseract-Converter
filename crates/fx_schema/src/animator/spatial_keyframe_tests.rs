use super::*;
use serde_json::json;

#[test]
fn reader_keeps_scalar_wire_and_rejects_vector_controls() {
    let vector_wire = json!({"keyframes":[
        {"id":"a","layerTime":0,"value":{"type":"vector2","value":[0.0,0.0]},"easing":{"type":"linear"},"spatialOutTangent":[20.0,80.0]},
        {"id":"b","layerTime":1000,"value":{"type":"vector2","value":[100.0,0.0]},"easing":{"type":"linear"},"spatialInTangent":[-20.0,80.0]}
    ]});
    assert!(serde_json::from_value::<PropertyKeyframeTrack>(vector_wire).is_err());
    let wire = json!({"keyframes":[
        {"id":"a","layerTime":0,"value":{"type":"float","value":0.0},"easing":{"type":"linear"},"spatialOutTangent":20.0},
        {"id":"b","layerTime":1000,"value":{"type":"float","value":100.0},"easing":{"type":"linear"},"spatialInTangent":-20.0}
    ]});
    let track: PropertyKeyframeTrack = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(track).unwrap(), wire);
}
