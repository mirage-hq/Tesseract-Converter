use fx_schema::{
    FXComposition, Layer, LayerId, MotionBlurSettings, PropType, Property, TimeRemapProperty,
};
use serde_json::{json, Value};

fn playback() -> Value {
    json!({
        "type": "windowed", "inputRange": {"start": 0, "duration": 1000},
        "mapping": {
            "type": "linear", "input": {"start": 0, "duration": 1000},
            "output": {"start": 0, "duration": 1000}
        },
        "inputOffsetMs": 0
    })
}

fn media() -> Value {
    json!({
        "type": "Media", "id": 1, "name": "legacy", "activeRange": {"start": 0, "duration": 1000},
        "source": {"assetId": "source", "kind": "video", "fit": "none", "historicalCrop": {"x": 0.25}},
        "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100, "futureTransform": null},
        "effects": [{"type": "shaderPreset", "presetId": "historical"}], "futureLayer": [2, 1, 2]
    })
}

fn document() -> Value {
    json!({
        "version": 66, "id": "main", "name": "stored", "layers": [media()],
        "dynamics": {"entries": [{
            "property": Property::new(LayerId::new(1), PropType::Rotation),
            "animator": {"type": "jsScript", "code": "legacy text", "layerTimeJsCode": "different text", "futureAnimator": true},
            "futureEntry": null
        }], "futureGraph": [1, 1]},
        "futureComposition": {"enabled": true}
    })
}

#[test]
fn legacy_media_remains_an_asset_metadata_source_without_rewriting() {
    for kind in ["video", "image"] {
        let mut wire = document();
        wire["layers"][0]["source"]["kind"] = json!(kind);
        wire["dynamics"]["entries"][0]["layerRefs"] = json!({"source": {"layerId": 1}});
        let composition: FXComposition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(composition).unwrap(), wire);
    }
}

#[test]
fn non_media_layer_still_rejects_asset_metadata_references() {
    let mut wire = document();
    wire["layers"][0]["type"] = json!("Rect");
    wire["layers"][0]["rect"] = json!({"size": [100, 100], "fillColor": [0, 0, 0, 1]});
    wire["dynamics"]["entries"][0]["layerRefs"] = json!({"source": {"layerId": 1}});
    let error = serde_json::from_value::<FXComposition>(wire).unwrap_err();
    assert!(error
        .to_string()
        .contains("does not support read-only property"));
}

#[test]
fn incoming_auto_ducking_data_is_checked_and_retained() {
    let wire = json!({
        "type": "Audio", "id": 1, "name": "sound",
        "source": {"assetId": "sound"},
        "playback": playback(),
        "sourceRange": {"start": 0, "duration": 1000},
        "sourceIntrinsicDuration": 1000,
        "autoDucking": {"duckedGain": 0.2, "mergeGapMs": 2000, "futureControl": true}
    });
    let layer: Layer = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(layer).unwrap(), wire);
    let mut invalid = wire;
    invalid["autoDucking"]["mergeGapMs"] = json!(-1);
    assert!(serde_json::from_value::<Layer>(invalid).is_err());
}

#[test]
fn modern_source_extensions_and_legacy_fit_are_retained() {
    let mut wire = media();
    wire["type"] = json!("Video");
    wire["sourceRange"] = wire["activeRange"].clone();
    wire.as_object_mut().unwrap().remove("activeRange");
    wire["playback"] = playback();
    wire["sourceIntrinsicDuration"] = json!(1000);
    wire["metadata"] = json!({"futureMetadata": true});
    let layer: Layer = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&layer).unwrap(), wire);
    let fx_schema::LayerData::Video(data) = layer.data() else {
        panic!("video expected")
    };
    assert_eq!(data.source.fit, fx_schema::MediaFit::None);
    let mut invalid = data.clone();
    invalid.start_time = Some(f64::NAN);
    assert!(Layer::from_data(&fx_schema::LayerData::Video(invalid)).is_err());
}

#[test]
fn boolean_children_retain_static_and_graph_asset_references() {
    let mut child = media();
    child["parent"] = json!(2);
    let mut wire = document();
    wire["layers"] = json!([{
        "type": "BooleanOperation", "id": 2, "name": "group",
        "activeRange": child["activeRange"], "transform": child["transform"], "layers": [child]
    }]);
    wire["dynamics"]["entries"] = json!([{
        "property": Property::new(LayerId::new(1), PropType::MediaSourceAssetId),
        "animator": {"type": "constant", "value": {"type": "string", "value": "override"}}
    }]);
    let composition: FXComposition = serde_json::from_value(wire.clone()).unwrap();
    let refs = composition.asset_refs();
    assert!(refs.iter().any(|reference| reference.asset_id == "source"));
    assert!(refs
        .iter()
        .any(|reference| reference.asset_id == "override"));
    assert_eq!(serde_json::to_value(composition).unwrap(), wire);
}

#[test]
fn production_layer_reader_retains_legacy_media_without_migration() {
    let wire = media();
    let layer: Layer = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(layer.layer_type_name(), "Media");
    assert_eq!(serde_json::to_value(layer).unwrap(), wire);
}

#[test]
fn production_root_reader_does_not_rewrite_layers_scripts_or_effects() {
    let wire = document();
    let composition: FXComposition = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(composition.asset_refs()[0].asset_id, "source");
    assert_eq!(serde_json::to_value(composition).unwrap(), wire);
}

#[test]
fn root_edit_replaces_only_the_requested_record() {
    let wire = document();
    let mut composition: FXComposition = serde_json::from_value(wire.clone()).unwrap();
    composition
        .set_motion_blur(MotionBlurSettings {
            enabled: true,
            ..MotionBlurSettings::default()
        })
        .unwrap();
    let mut result = serde_json::to_value(composition).unwrap();
    assert_eq!(result["motionBlur"]["enabled"], true);
    result.as_object_mut().unwrap().remove("motionBlur");
    assert_eq!(result, wire);
}

#[test]
fn failed_root_edit_is_atomic() {
    let wire = document();
    let mut composition: FXComposition = serde_json::from_value(wire.clone()).unwrap();
    assert!(composition
        .set_motion_blur(MotionBlurSettings {
            shutter_phase: 361.0,
            ..MotionBlurSettings::default()
        })
        .is_err());
    assert_eq!(serde_json::to_value(composition).unwrap(), wire);
}

#[test]
fn malformed_known_media_and_instance_fields_are_rejected_by_production_reader() {
    let mut wire = document();
    wire["layers"][0]["source"]["sourceRect"] = json!({"x": 0, "y": 0, "width": 0, "height": 1});
    assert!(serde_json::from_value::<FXComposition>(wire).is_err());
    let mut wire = document();
    wire["layers"][0]["effects"] =
        json!([{"id": null, "type": "shaderPreset", "presetId": "historical"}]);
    assert!(serde_json::from_value::<FXComposition>(wire).is_err());
}

#[test]
fn malformed_script_field_is_not_hidden_by_another_script_field() {
    let mut wire = document();
    wire["dynamics"]["entries"][0]["animator"]["code"] = json!(7);
    assert!(serde_json::from_value::<FXComposition>(wire).is_err());
}

#[test]
fn malformed_canonical_dependency_cannot_fall_back_to_legacy_property() {
    let mut wire = document();
    let mut dependency =
        serde_json::to_value(Property::new(LayerId::new(1), PropType::ActiveRange)).unwrap();
    dependency["kind"] = Value::Null;
    wire["dynamics"]["entries"][0]["dependencies"] = json!([dependency]);
    assert!(serde_json::from_value::<FXComposition>(wire).is_err());
}

#[test]
fn standalone_time_remap_keeps_unknown_fields_without_execution() {
    let wire = json!({
        "keyframes": [
            {"id": "a", "time": 0, "value": 200, "easing": {"type": "linear"}, "futureKey": true},
            {"id": "b", "time": 1000, "value": 1200, "easing": {"type": "linear"}}
        ], "before": "inactive", "after": "inactive", "futureMapping": null
    });
    let remap: TimeRemapProperty = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(remap).unwrap(), wire);
}

#[test]
fn unknown_field_reporting_uses_known_data_not_lossless_serialization() {
    let composition: FXComposition = serde_json::from_value(document()).unwrap();
    let paths = composition
        .unknown_fields()
        .map(|(path, _)| path)
        .collect::<Vec<_>>();
    for path in [
        "futureComposition",
        "layers.0.futureLayer",
        "layers.0.transform.futureTransform",
        "dynamics.futureGraph",
        "dynamics.entries.0.animator.futureAnimator",
    ] {
        assert!(
            paths.iter().any(|value| value == path),
            "missing {path}: {paths:?}"
        );
    }
}
