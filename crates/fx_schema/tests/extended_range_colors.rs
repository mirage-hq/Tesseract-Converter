use fx_schema::{
    has_valid_extended_color, LayerEffect, PagTextInsert, RectShape, ShapePaint, TextDocument,
};
use serde_json::json;

#[test]
fn extended_shape_and_solid_colors_accept_headroom_and_reject_invalid_ranges() {
    for color in [[2.0, 1.0, 0.5, 1.0], [64.0, 0.0, 1.0, 0.5]] {
        let rect: RectShape =
            serde_json::from_value(json!({"size":[64,64], "fillColor":color})).unwrap();
        assert_eq!(rect.fill_color, color);
        let paint: ShapePaint =
            serde_json::from_value(json!({"type":"solid", "color":color})).unwrap();
        assert!(paint.has_valid_values());
    }
    for color in [
        [-0.1, 0.0, 0.0, 1.0],
        [65.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.1],
    ] {
        assert!(
            serde_json::from_value::<RectShape>(json!({"size":[64,64], "fillColor":color}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<ShapePaint>(json!({"type":"solid", "color":color})).is_err()
        );
    }
    for channel in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!has_valid_extended_color(&[channel, 0.0, 0.0, 1.0]));
    }
}

#[test]
fn extended_text_gradients_disabled_styles_and_pag_overrides_validate_coverage() {
    for (color, valid) in [
        ([2.0, 1.0, 0.5, 0.25], true),
        ([64.0, 2.0, 1.0, 1.0], true),
        ([65.0, 0.0, 0.0, 1.0], false),
        ([-1.0, 0.0, 0.0, 1.0], false),
        ([2.0, 0.0, 0.0, 1.1], false),
    ] {
        let constant = json!({"type":"color", "value":color});
        assert_eq!(
            serde_json::from_value::<fx_schema::PropertyValue>(constant).is_ok(),
            valid
        );
        let text = json!({"text":"HDR", "fontFamily":"Inter", "fontSize":32, "fillColor":color});
        assert_eq!(serde_json::from_value::<TextDocument>(text).is_ok(), valid);
        let style = json!({"type":"outerGlow", "enabled":false, "color":color});
        assert_eq!(serde_json::from_value::<LayerEffect>(style).is_ok(), valid);
        let insert = json!({"id":"text", "text":"HDR", "fillColor":color, "strokeColor":color, "backgroundColor":color});
        assert_eq!(
            serde_json::from_value::<PagTextInsert>(insert).is_ok(),
            valid
        );
        let gradient = json!({"type":"gradient", "gradientType":"linear", "start":[0,0], "end":[64,0],
            "stops":[{"offset":0,"color":color},{"offset":1,"color":[1,1,1,1]}]});
        assert_eq!(
            serde_json::from_value::<ShapePaint>(gradient).is_ok(),
            valid
        );
    }
}
