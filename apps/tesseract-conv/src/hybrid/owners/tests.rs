use super::*;
use fx_schema::EditableFxCompositionDocument;
use serde_json::{json, Value};
use tesseract_file::{TesseractFile, TesseractFileBuilder};

fn document(duplicate_alias: bool) -> EditableFxCompositionDocument {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
    ))
    .unwrap();
    let mut first = value["composition"]["layers"][0].clone();
    first["id"] = json!(1);
    first["effects"] = json!([{
        "id": 10, "effect": {"type": "dropShadow"},
        "legacySource": {"kind": "layerStyle", "itemId": 50}
    }]);
    let mut second = first.clone();
    second["id"] = json!(2);
    second["effects"] = if duplicate_alias {
        json!([{
            "id": 11, "effect": {"type": "dropShadow"},
            "legacySource": {"kind": "layerStyle", "itemId": 50}
        }])
    } else {
        json!([])
    };
    value["composition"]["layers"] = json!([first, second]);
    value["composition"]["dynamics"]["entries"] = json!([{
        "target": {"kind": "fxItemProperty", "itemId": 50, "propertyName": "offset"},
        "animator": {"type": "jsScript", "layerTimeJsCode": "return [3, 3];"},
        "dependencies": [{"kind": "layer", "layerId": 2, "propertyType": "opacity"}]
    }, {
        "target": {"kind": "layer", "layerId": 2, "propertyType": "opacity"},
        "animator": {"type": "jsScript", "layerTimeJsCode": "return 100;"},
        "dependencies": []
    }]);
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

#[test]
fn review_legacy_style_archive_owner_and_dependency_routing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.tsrct");
    TesseractFileBuilder::new(document(false))
        .write(&path)
        .unwrap();
    let archive = TesseractFile::open(&path).unwrap();
    let document = archive.project();
    let effect = &document.composition().layers()[0].effects()[0];
    assert_eq!(effect.wire_value()["legacySource"]["itemId"], json!(50));
    let owners = Owners::new(document.composition().layers()).unwrap();
    let target = &document.composition().dynamics().entries()[0].target;
    assert_eq!(owners.target(target).unwrap(), 0);
    let effect_target: PropertyTarget = serde_json::from_value(json!({
        "kind": "effectProperty", "effectId": 10, "paramName": "offset"
    }))
    .unwrap();
    assert_eq!(owners.target(&effect_target).unwrap(), 0);
    let graph = super::super::dependencies::GraphDependencies::new(document, &owners).unwrap();
    assert_eq!(graph.connected(0), [0, 1].into_iter().collect());
}

#[test]
fn review_duplicate_legacy_style_owner_is_rejected() {
    let document = document(true);
    let error = Owners::new(document.composition().layers()).err().unwrap();
    assert!(error.to_string().contains("duplicate FX-item ID"));
}
