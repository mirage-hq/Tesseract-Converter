use fx_schema::{ColorManagement, EditableFxCompositionDocument, FxComposition};

#[test]
fn any_version_is_ignored_then_dropped() {
    for policy in [
        json!({"workingSpace":"legacyDisplay"}),
        json!({"version":2,"workingSpace":"legacyDisplay"}),
        json!({"version":"1","workingSpace":"legacyDisplay"}),
        json!({"version":null,"workingSpace":"legacyDisplay"}),
        json!({"version":1.5,"workingSpace":"legacyDisplay"}),
        json!({"version":{"future":true},"workingSpace":"legacyDisplay"}),
        json!({"version":1,"workingSpace":"legacyDisplay"}),
    ] {
        let parsed = ColorManagement::from_value(policy).unwrap();
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            json!({"workingSpace":"legacyDisplay"})
        );
    }
}
use serde_json::{json, Value};

fn editable() -> Value {
    json!({"$schema": fx_schema::EDITABLE_FX_DOCUMENT_SCHEMA_URL, "formatVersion":1,
        "dimensions":{"width":32,"height":32},"duration":1,
        "composition":{"id":"main","name":"Main","layers":[]}})
}
fn baked() -> Value {
    json!({"version":75,"composition":{"id":"main","name":"Main","width":32,"height":32,"duration":1,"layers":[]}})
}

#[test]
fn acescct_editable_and_baked_carriers_preserve_controls_and_domains() {
    for compositing in ["footageOnly", "wholeProgram", "perClip"] {
        let policy =
            json!({"workingSpace":"acescct","outputTransform":"aces2.0","compositing":compositing});
        let transform = json!({"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100});
        let mut value = editable();
        value["colorManagement"] = policy.clone();
        value["hdrGraphicsWhiteNits"] = json!(203);
        value["composition"]["layers"] = json!([
            {"id":1,"name":"image","type":"Image","renderDomain":"display",
             "activeRange":{"start":0,"duration":1000},"transform":transform.clone(),
             "source":{"assetId":"image","sourceRect":{"x":0,"y":0,"width":32,"height":32},"fit":"contain"}},
            {"id":2,"name":"video","type":"Video","renderDomain":"scene",
             "playback":{"type":"windowed","inputRange":{"start":0,"duration":1000},
                 "mapping":{"type":"linear","input":{"start":0,"duration":1000},"output":{"start":0,"duration":1000}},
                 "inputOffsetMs":0},
             "sourceRange":{"start":0,"duration":1000},"sourceIntrinsicDuration":1000,"transform":transform,
             "source":{"assetId":"video","sourceRect":{"x":0,"y":0,"width":32,"height":32},"fit":"contain"}}
        ]);
        let mut document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let saved = document.to_json_value().unwrap();
        assert_eq!(saved["colorManagement"], policy);
        assert_eq!(saved["hdrGraphicsWhiteNits"].as_f64(), Some(203.0));
        assert_eq!(saved["composition"]["layers"][0]["renderDomain"], "display");
        assert_eq!(saved["composition"]["layers"][1]["renderDomain"], "scene");
        document.render_preflight().unwrap();
        document.set_color_management(ColorManagement::default());
        assert!(document.render_preflight().is_ok());
        assert_eq!(
            document.to_json_value().unwrap()["composition"],
            saved["composition"]
        );
        document.set_color_management(ColorManagement::from_value(policy.clone()).unwrap());
        assert_eq!(document.to_json_value().unwrap(), saved);
        let mut snapshot = baked();
        snapshot["colorManagement"] = policy.clone();
        snapshot["composition"]["layers"] = saved["composition"]["layers"].clone();
        let document: FxComposition = serde_json::from_value(snapshot).unwrap();
        document.render_preflight().unwrap();
        let saved = serde_json::to_value(document).unwrap();
        assert_eq!(saved["colorManagement"], policy);
        assert_eq!(saved["composition"]["layers"][0]["renderDomain"], "display");
    }
}

#[test]
fn absent_policy_is_always_written_at_each_document_boundary() {
    let expected = json!({"workingSpace":"legacyDisplay"});
    let document = EditableFxCompositionDocument::from_json_value(editable()).unwrap();
    assert_eq!(document.color_management(), ColorManagement::default());
    assert_eq!(
        document.to_json_value().unwrap()["colorManagement"],
        expected
    );
    let mut historical = baked();
    historical["colorManagement"] = json!({"version":1,"workingSpace":"legacyDisplay"});
    let document: FxComposition = serde_json::from_value(historical).unwrap();
    assert_eq!(
        serde_json::to_value(document).unwrap()["colorManagement"],
        expected
    );
}
#[test]
fn explicit_policy_roundtrips_and_preserves_unrelated_extensions() {
    let mut value = editable();
    value["colorManagement"] = json!({"version":1,"workingSpace":"legacyDisplay"});
    value["future"] = json!({"colorManagement":"metadata, not a policy"});
    value["composition"]["metadata"] = json!({"colorManagement":null});
    let document = EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    value["colorManagement"] = json!({"workingSpace":"legacyDisplay"});
    assert_eq!(document.to_json_value().unwrap(), value);
}
#[test]
fn future_policy_never_falls_back_to_legacy() {
    for policy in [
        json!({"version":1,"workingSpace":"futureSpace"}),
        json!({"version":1,"workingSpace":"acescg"}),
        json!({"version":1,"workingSpace":"LegacyDisplay"}),
        json!({"version":1,"workingSpace":"legacyDisplay","output":{}}),
    ] {
        let mut value = editable();
        value["colorManagement"] = policy.clone();
        let error = EditableFxCompositionDocument::from_json_value(value)
            .unwrap_err()
            .to_string();
        assert!(error.contains("requires a newer Jerboa"), "{error}");
        let mut value = baked();
        value["colorManagement"] = policy;
        assert!(serde_json::from_value::<FxComposition>(value)
            .unwrap_err()
            .to_string()
            .contains("requires a newer Jerboa"));
    }
}
#[test]
fn corrupt_policy_is_not_an_absent_policy() {
    for policy in [
        Value::Null,
        json!({}),
        json!({"version":1}),
        json!({"version":1,"workingSpace":7}),
    ] {
        let mut value = editable();
        value["colorManagement"] = policy.clone();
        assert!(EditableFxCompositionDocument::from_json_value(value).is_err());
        assert!(serde_json::from_value::<ColorManagement>(policy).is_err());
    }
}
#[test]
fn bare_baked_composition_rejects_document_policy() {
    let mut value = baked()["composition"].clone();
    value["colorManagement"] = json!({"version":1,"workingSpace":"legacyDisplay"});
    assert!(serde_json::from_value::<fx_schema::Composition>(value)
        .unwrap_err()
        .to_string()
        .contains("document-scoped"));
}

#[test]
fn nested_policy_is_rejected_without_scanning_metadata() {
    let mut value = editable();
    value["composition"]["colorManagement"] = json!({"version":1,"workingSpace":"legacyDisplay"});
    assert!(EditableFxCompositionDocument::from_json_value(value)
        .unwrap_err()
        .to_string()
        .contains("document-scoped"));
    for kind in ["Group", "AiEdit", "Adjustment"] {
        let mut value = editable();
        value["composition"]["layers"] = json!([{"type":kind,"colorManagement":null}]);
        assert!(EditableFxCompositionDocument::from_json_value(value)
            .unwrap_err()
            .to_string()
            .contains("document-scoped"));
    }
}
