use super::decode;
use crate::tests::support::{legacy_run, legacy_source_text, legacy_source_text_payload};
use serde_json::{json, Value};

// Public reduced data: existing author-owned legacy text plus the inactive
// decoration layout observed in the external Resort titles. No corpus bytes.
fn inactive_decorations() -> Value {
    let mut text = legacy_source_text();
    let paragraph = text["mTextParam"].as_object_mut().unwrap();
    paragraph.extend([
        ("mBackFillColor".into(), json!(0x20_30_40)),
        ("mBackFillOpacity".into(), json!(33)),
        ("mBackFillSize".into(), json!(4)),
        ("mBackFillVisible".into(), json!(false)),
        ("mIsMask".into(), json!(false)),
        ("mIsMaskInverted".into(), json!(false)),
        ("mLineCapType".into(), json!(0)),
        ("mLineJoinType".into(), json!(0)),
        ("mMiterLimit".into(), json!(2.5)),
        ("mNumStrokes".into(), json!(1)),
    ]);
    let style = paragraph["mStyleSheet"].as_object_mut().unwrap();
    style.extend([
        ("mAdditionalStrokeColor".into(), json!([])),
        ("mAdditionalStrokeVisible".into(), json!([])),
        ("mAdditionalStrokeWidth".into(), json!([])),
        ("mUnderline".into(), legacy_run(json!(false))),
    ]);
    text
}

fn read(text: &Value) -> crate::error::Result<super::DecodedGraphicText> {
    decode(&legacy_source_text_payload(&text.to_string()))
}

#[test]
fn passive_decorations_keep_the_existing_legacy_text_document() {
    let expected = read(&legacy_source_text()).unwrap();
    let actual = read(&inactive_decorations()).unwrap();
    assert!(actual.omitted.is_empty() && actual.legacy_json);
    assert!(actual.mask_source.is_none());
    assert_eq!(
        actual.uniform().unwrap().document,
        expected.uniform().unwrap().document
    );
}

#[test]
fn passive_decorations_reject_active_features() {
    for (pointer, value) in [
        ("/mTextParam/mBackFillVisible", json!(true)),
        ("/mTextParam/mIsMask", json!(true)),
        ("/mTextParam/mIsMaskInverted", json!(true)),
        ("/mTextParam/mNumStrokes", json!(2)),
        (
            "/mTextParam/mStyleSheet/mUnderline",
            legacy_run(json!(true)),
        ),
        ("/mTextParam/mStyleSheet/mAdditionalStrokeColor", json!([0])),
        (
            "/mTextParam/mStyleSheet/mAdditionalStrokeVisible",
            json!([false]),
        ),
        ("/mTextParam/mStyleSheet/mAdditionalStrokeWidth", json!([0])),
        ("/mTextParam/mShadowVisible", json!(true)),
    ] {
        let mut text = inactive_decorations();
        *text.pointer_mut(pointer).unwrap() = value;
        assert!(read(&text).is_err(), "{pointer}");
    }
}

#[test]
fn legacy_gray_stroke_preserves_editable_width_and_fill_order() {
    let mut text = inactive_decorations();
    text["mTextParam"]["mStyleSheet"]["mStrokeVisible"] = legacy_run(json!(true));
    text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x303030));
    text["mTextParam"]["mStyleSheet"]["mStrokeWidth"] = legacy_run(json!(3));
    text["mTextParam"]["mStyleSheet"]["mFillOverStroke"] = legacy_run(json!(true));
    let actual = read(&text).unwrap().uniform().unwrap().document;
    assert_eq!(
        actual.stroke,
        Some(crate::schema::text::PrTextStroke {
            color: crate::schema::text::PrRgb([48; 3]),
            width: 3.0,
        })
    );
    text["mTextParam"]["mStyleSheet"]["mFillOverStroke"] = legacy_run(json!(false));
    assert!(read(&text).is_err());
    text["mTextParam"]["mStyleSheet"]["mFillOverStroke"] = legacy_run(json!(true));
    text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x123456));
    assert!(read(&text).is_err());
    text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x303030));
    text["mTextParam"]["mStyleSheet"]["mStrokeWidth"] = legacy_run(json!(-1));
    assert!(read(&text).is_err());
}

#[test]
fn passive_decorations_require_the_complete_observed_layout() {
    for key in [
        "mBackFillColor",
        "mBackFillOpacity",
        "mBackFillSize",
        "mBackFillVisible",
        "mIsMask",
        "mIsMaskInverted",
        "mLineCapType",
        "mLineJoinType",
        "mMiterLimit",
        "mNumStrokes",
    ] {
        let mut text = inactive_decorations();
        text["mTextParam"].as_object_mut().unwrap().remove(key);
        assert!(read(&text).is_err(), "missing {key}");
    }
    for key in [
        "mAdditionalStrokeColor",
        "mAdditionalStrokeVisible",
        "mAdditionalStrokeWidth",
        "mUnderline",
    ] {
        let mut text = inactive_decorations();
        text["mTextParam"]["mStyleSheet"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(read(&text).is_err(), "missing {key}");
    }
}

#[test]
fn passive_decorations_reject_nulls_bad_types_and_ranges() {
    for pointer in [
        "/mTextParam/mBackFillColor",
        "/mTextParam/mBackFillOpacity",
        "/mTextParam/mBackFillSize",
        "/mTextParam/mBackFillVisible",
        "/mTextParam/mIsMask",
        "/mTextParam/mIsMaskInverted",
        "/mTextParam/mLineCapType",
        "/mTextParam/mLineJoinType",
        "/mTextParam/mMiterLimit",
        "/mTextParam/mNumStrokes",
        "/mTextParam/mStyleSheet/mAdditionalStrokeColor",
        "/mTextParam/mStyleSheet/mAdditionalStrokeVisible",
        "/mTextParam/mStyleSheet/mAdditionalStrokeWidth",
        "/mTextParam/mStyleSheet/mUnderline",
    ] {
        for value in [Value::Null, json!("false")] {
            let mut text = inactive_decorations();
            *text.pointer_mut(pointer).unwrap() = value;
            assert!(read(&text).is_err(), "{pointer}");
        }
    }
    for (pointer, value) in [
        ("/mTextParam/mBackFillColor", json!(0xff00_0000_u32)),
        ("/mTextParam/mBackFillOpacity", json!(-1)),
        ("/mTextParam/mBackFillOpacity", json!(101)),
        ("/mTextParam/mBackFillSize", json!(-1)),
        ("/mTextParam/mLineCapType", json!(1)),
        ("/mTextParam/mLineJoinType", json!(1)),
        ("/mTextParam/mMiterLimit", json!(-1)),
        ("/mTextParam/mMiterLimit", json!(4)),
    ] {
        let mut text = inactive_decorations();
        *text.pointer_mut(pointer).unwrap() = value;
        assert!(read(&text).is_err(), "{pointer}");
    }
}

#[test]
fn passive_decorations_reject_unknown_duplicate_and_mixed_run_fields() {
    let mut text = inactive_decorations();
    text["mTextParam"]["mUnknownDecoration"] = json!(false);
    assert!(read(&text).is_err());
    let text = inactive_decorations();
    let duplicate = text.to_string().replace(
        "\"mBackFillVisible\":false",
        "\"mBackFillVisible\":false,\"mBackFillVisible\":false",
    );
    assert!(decode(&legacy_source_text_payload(&duplicate)).is_err());
    for value in [
        json!({"mParamValues": [[0, false], [1, false]]}),
        json!({"mParamValues": [[1, false]]}),
    ] {
        let mut text = inactive_decorations();
        text["mTextParam"]["mStyleSheet"]["mUnderline"] = value;
        assert!(read(&text).is_err());
    }
}
