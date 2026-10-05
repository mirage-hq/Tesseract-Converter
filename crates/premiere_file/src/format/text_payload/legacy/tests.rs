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

// These are reduced, author-owned inputs, not new Adobe-native fixtures.
#[test]
fn legacy_box_leading_and_stroke_preserve_actual_text_and_supported_style() {
    use crate::schema::text::{PrRgb, PrTextFrame, PrTextStroke, PrVerticalAlign};

    let mut text = legacy_source_text();
    text["mTextParam"]["mWidth"] = json!(56);
    text["mTextParam"]["mHeight"] = json!(10);
    text["mTextParam"]["mLeading"] = json!(7.5);
    let style = &mut text["mTextParam"]["mStyleSheet"];
    style["mStrokeVisible"] = legacy_run(json!(true));
    style["mStrokeColor"] = legacy_run(json!(0xff_ffff));
    style["mStrokeWidth"] = legacy_run(json!(3));
    style["mFillOverStroke"] = legacy_run(json!(true));

    let decoded = read(&text).expect("supported box, leading and stroke must not discard text");
    assert!(decoded.omitted.is_empty());
    let actual = decoded.uniform().unwrap().document;
    let mut expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    expected.frame = PrTextFrame::Box {
        width: 56.0,
        height: 10.0,
        vertical: PrVerticalAlign::Top,
    };
    expected.leading = 7.5;
    expected.stroke = Some(PrTextStroke {
        color: PrRgb([255; 3]),
        width: 3.0,
    });
    assert_eq!(actual, expected);
}

#[test]
fn legacy_unmapped_character_controls_preserve_text_with_contextual_diagnostics() {
    let expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    for (field, value) in [
        ("mFauxBold", json!(true)),
        ("mFauxItalic", json!(true)),
        ("mBaselineShift", json!(4)),
        ("mKerning", json!(20)),
        ("mTsumi", json!(1)),
    ] {
        let mut text = legacy_source_text();
        text["mTextParam"]["mStyleSheet"][field] = legacy_run(value);
        let actual = read(&text).unwrap_or_else(|error| {
            panic!("{field} must not discard actual text: {error}");
        });
        assert!(
            actual
                .omitted
                .iter()
                .any(|feature| feature.to_string().contains(field)),
            "missing diagnostic for {field}"
        );
        assert_eq!(actual.uniform().unwrap().document, expected, "{field}");
    }
}

#[test]
fn legacy_optional_inactive_decoration_fields_do_not_require_a_complete_profile() {
    let expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    let mut text = legacy_source_text();
    text["mTextParam"]["mBackFillVisible"] = json!(false);
    text["mTextParam"]["mLineCapType"] = json!(1);
    text["mTextParam"]["mStyleSheet"]["mUnderline"] = legacy_run(json!(false));
    let actual = read(&text).expect("optional inactive fields must not discard text");
    assert_eq!(actual.uniform().unwrap().document, expected);
}

#[test]
fn legacy_underline_is_omitted_without_losing_actual_text_or_paint() {
    let expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    let mut text = inactive_decorations();
    text["mTextParam"]["mStyleSheet"]["mUnderline"] = legacy_run(json!(true));
    let actual = read(&text).expect("unsupported underline must not discard actual text");
    assert!(actual
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("mUnderline")));
    assert_eq!(actual.uniform().unwrap().document, expected);
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
fn active_legacy_masks_still_fail_without_exposing_concealed_content() {
    for pointer in ["/mTextParam/mIsMask", "/mTextParam/mIsMaskInverted"] {
        for value in [json!(true), Value::Null, json!("false")] {
            let mut text = inactive_decorations();
            *text.pointer_mut(pointer).unwrap() = value;
            assert!(read(&text).is_err(), "{pointer}");
        }
    }
}

#[test]
fn legacy_stroke_recovery_preserves_text_and_reports_only_changed_details() {
    use crate::schema::text::{PrRgb, PrTextStroke};
    let mut text = inactive_decorations();
    let style = &mut text["mTextParam"]["mStyleSheet"];
    style["mStrokeVisible"] = legacy_run(json!(true));
    style["mStrokeColor"] = legacy_run(json!(0x303030));
    style["mStrokeWidth"] = legacy_run(json!(3));
    style["mFillOverStroke"] = legacy_run(json!(true));
    let expected = read(&text).unwrap().uniform().unwrap().document;
    assert_eq!(
        expected.stroke,
        Some(PrTextStroke {
            color: PrRgb([48; 3]),
            width: 3.0
        })
    );
    text["mTextParam"]["mStyleSheet"]["mFillOverStroke"] = legacy_run(json!(false));
    let actual = read(&text).unwrap();
    assert!(actual
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("mFillOverStroke")));
    assert_eq!(actual.uniform().unwrap().document, expected);
    text["mTextParam"]["mStyleSheet"]["mFillOverStroke"] = legacy_run(json!(true));
    text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x123456));
    let actual = read(&text).unwrap();
    assert!(actual
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("mStrokeColor")));
    let mut white_stroke = expected.clone();
    white_stroke.stroke.as_mut().unwrap().color = PrRgb([255; 3]);
    assert_eq!(actual.uniform().unwrap().document, white_stroke);
    text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x303030));
    text["mTextParam"]["mStyleSheet"]["mStrokeWidth"] = legacy_run(json!(-1));
    let actual = read(&text).unwrap();
    assert!(actual
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("mStrokeWidth")));
    assert_eq!(
        actual.uniform().unwrap().document,
        crate::schema::text::PrTextDocument {
            stroke: None,
            ..expected
        }
    );
}

#[test]
fn inactive_decorations_keep_text_when_optional_fields_are_absent_or_unusable() {
    let expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    for key in [
        "mBackFillColor",
        "mBackFillOpacity",
        "mBackFillSize",
        "mShadowColor",
        "mMiterLimit",
        "mLineCapType",
    ] {
        for value in [
            None,
            Some(Value::Null),
            Some(json!("not a consumed value")),
            Some(json!(-1)),
        ] {
            let mut text = inactive_decorations();
            let paragraph = text["mTextParam"].as_object_mut().unwrap();
            if let Some(value) = value {
                paragraph.insert(key.into(), value);
            } else {
                paragraph.remove(key);
            }
            assert_eq!(
                read(&text).unwrap().uniform().unwrap().document,
                expected,
                "{key}"
            );
        }
    }
}

#[test]
fn legacy_unmapped_controls_and_versions_do_not_drop_known_content() {
    let expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    let mut text = legacy_source_text();
    text["mVersion"] = json!(2);
    text["mTextParam"]["mUnknownDecoration"] = json!({"saved": true});
    text["mTextParam"]["mStyleSheet"]["mUnderline"] =
        json!({"mParamValues": [[0, false], [1, true]]});
    let actual = read(&text).unwrap();
    for field in ["mUnknownDecoration", "mUnderline"] {
        assert!(
            actual
                .omitted
                .iter()
                .any(|feature| feature.to_string().contains(field)),
            "{field}"
        );
    }
    assert_eq!(actual.uniform().unwrap().document, expected);
}

#[test]
fn legacy_layout_maps_alignment_caps_and_recovers_unusable_leading() {
    use crate::schema::text::PrJustification;
    let mut text = legacy_source_text();
    text["mTextParam"]["mAlignment"] = json!(1);
    text["mTextParam"]["mStyleSheet"]["mCapsOption"] = legacy_run(json!(2));
    text["mTextParam"]["mLeading"] = json!(-100);
    let actual = read(&text).unwrap();
    assert!(actual
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("mLeading")));
    let document = actual.uniform().unwrap().document;
    assert_eq!(document.justification, PrJustification::Right);
    assert!(document.all_caps);
    assert_eq!(document.leading, 0.0);
    assert_eq!(document.text, "Night\nMarket \u{2713} \u{1f525}");
    document.validate().unwrap();
}
