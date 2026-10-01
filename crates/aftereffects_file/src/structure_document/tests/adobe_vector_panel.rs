use super::*;

const SOURCE_PATH: &str =
    "crates/aftereffects_file/tests/fixtures/vectors/fx_export_panel/vector-rect-size.aep";
const SOURCE_SHA256: &str = "ba0abcd0916165f6a62db0103d947362e75a27f37c5dd07e80047e4bf6f81b0c";

fn find_rect(layers: &[fx_schema::Layer]) -> Vec<&fx_schema::RectLayer> {
    let mut rects = Vec::new();
    for layer in layers {
        match layer.data() {
            FxLayer::Rect(rect) => rects.push(rect),
            FxLayer::Group(group) => rects.extend(find_rect(&group.layers)),
            FxLayer::BooleanOperation(boolean) => rects.extend(find_rect(&boolean.layers)),
            _ => {}
        }
    }
    rects
}

fn assert_linear_track(
    json: &Value,
    owner: fx_schema::LayerId,
    property: &str,
    values: [Value; 2],
) {
    let entries = json["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("fresh import must expose editable dynamics entries");
    let matching: Vec<_> = entries
        .iter()
        .filter(|entry| entry["target"]["propertyType"] == property)
        .collect();
    assert_eq!(matching.len(), 1, "expected one editable {property} track");
    assert_eq!(matching[0]["target"]["layerId"], serde_json::json!(owner));
    let keys = matching[0]["animator"]["keyframes"]
        .as_array()
        .expect("Rectangle animation must use typed keyframes");
    assert_eq!(keys.len(), 2, "{property}");
    for (index, (key, expected)) in keys.iter().zip(values).enumerate() {
        assert_eq!(key["layerTime"], index as u64 * 1_000, "{property} clock");
        assert_eq!(key["value"], expected, "{property} key {index}");
        assert_eq!(key["easing"]["type"], "linear", "{property} easing");
    }
}

#[test]
fn adobe_vector_panel_rect_size_imports_editable_geometry_and_owner_clock() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(SOURCE_PATH, 1, || {
        let bytes =
            include_bytes!("../../../tests/fixtures/vectors/fx_export_panel/vector-rect-size.aep");
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), SOURCE_SHA256);

        let project = read_project(bytes).expect("read independently Adobe-authored Rectangle");
        let source = composition(&project, 1);
        assert_eq!((source.width, source.height), (320, 180));
        assert_eq!(source.frame_rate, 24.0);
        assert_eq!(source.layers.len(), 1);

        let converted = to_structural_fx_document(&project, Some(1))
            .expect("fresh Rectangle import must remain editable");
        assert_imported_canvas_matches_source(source, &converted, "vector-rect-size.aep:1");
        let root = root(&converted);
        assert_eq!(root.name, "vector-rect-size");
        assert_eq!(root.layers.len(), 1);
        let owner = as_group(&root.layers[0]);
        assert_eq!(owner.name, "Edited Rectangle");
        assert_eq!(owner.transform.anchor_point, [0.0, 0.0]);
        assert_eq!(
            owner.transform.position,
            fx_composition::Position::TwoD([100.0, 70.0])
        );

        let rects = find_rect(&owner.layers);
        let [rect] = rects.as_slice() else {
            panic!("native Rectangle must import as exactly one editable Rect, not flattened media")
        };
        assert_eq!(rect.rect.size, [80.0, 40.0]);
        assert_eq!(rect.rect.position, [0.0, 0.0]);
        assert_eq!(rect.rect.roundness, 2.0);
        // Adobe readback and source storage narrow these colors to f32.
        assert_eq!(
            rect.rect.fill_color,
            [0.2_f32, 0.4, 0.6, 1.0].map(f64::from)
        );
        assert!(rect.rect.fill_enabled);
        assert!(!rect.rect.stroke_enabled);
        assert_eq!(rect.transform.anchor_point, [40.0, 20.0]);
        assert_eq!(
            rect.transform.position,
            fx_composition::Position::TwoD([47.0, 17.0])
        );

        let editable = converted
            .document
            .to_json_vec()
            .expect("serialize editable FX");
        assert!(
            !editable
                .windows(b"jsScript".len())
                .any(|bytes| bytes == b"jsScript" || bytes == b"JsScript")
        );
        let json: Value = serde_json::from_slice(&editable).expect("parse editable FX JSON");
        assert_linear_track(
            &json,
            rect.id,
            "rectSize",
            [
                serde_json::json!({"type":"vector2","value":[80.0,40.0]}),
                serde_json::json!({"type":"vector2","value":[150.0,70.0]}),
            ],
        );
        assert_linear_track(
            &json,
            rect.id,
            "anchorPointX",
            [
                serde_json::json!({"type":"float","value":40.0}),
                serde_json::json!({"type":"float","value":75.0}),
            ],
        );
        assert_linear_track(
            &json,
            rect.id,
            "anchorPointY",
            [
                serde_json::json!({"type":"float","value":20.0}),
                serde_json::json!({"type":"float","value":35.0}),
            ],
        );
        assert_linear_track(
            &json,
            rect.id,
            "positionX",
            [
                serde_json::json!({"type":"float","value":47.0}),
                serde_json::json!({"type":"float","value":82.0}),
            ],
        );
        assert_linear_track(
            &json,
            rect.id,
            "positionY",
            [
                serde_json::json!({"type":"float","value":17.0}),
                serde_json::json!({"type":"float","value":32.0}),
            ],
        );
        EditableFxCompositionDocument::from_json_slice(&editable)
            .expect("fresh import must remain a valid editable FX document");
    });
    cases.finish();
}
