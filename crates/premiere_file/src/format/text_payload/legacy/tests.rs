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

fn shadow_text() -> Value {
    let mut text = legacy_source_text();
    text["mTextParam"].as_object_mut().unwrap().extend([
        ("mShadowVisible".into(), json!(true)),
        ("mShadowColor".into(), json!(0x303030)),
        ("mShadowOpacity".into(), json!(60)),
        ("mShadowAngle".into(), json!(135)),
        ("mShadowOffset".into(), json!(7)),
        ("mShadowSize".into(), json!(2)),
        ("mShadowBlur".into(), json!(40)),
    ]);
    text
}

#[test]
fn legacy_text_shadow_decodes_saved_values_without_guessing_defaults() {
    use crate::schema::{text::PrRgb, text_shadow::PrTextShadow};

    let decoded = read(&shadow_text()).unwrap().uniform().unwrap();
    assert_eq!(
        decoded.document.shadow,
        Some(PrTextShadow {
            color: PrRgb([48; 3]),
            opacity: 60.0,
            angle: 135.0,
            distance: 7.0,
            size: 2.0,
            blur: 40.0,
        })
    );
    assert!(decoded
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("legacy shadow")));
    assert!(!decoded.omitted.iter().any(|feature| {
        let message = feature.to_string();
        message.contains("mShadow") && message.contains("unmapped paragraph control omitted")
    }));
}

#[test]
fn legacy_text_shadow_invalid_optional_controls_omit_only_shadow() {
    let expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    for (field, invalid) in [
        ("mShadowVisible", json!("true")),
        ("mShadowColor", json!(0x123456)),
        ("mShadowColor", json!(-1)),
        ("mShadowColor", json!(4294967296_u64)),
        ("mShadowOpacity", json!(101)),
        ("mShadowAngle", Value::Null),
        ("mShadowOffset", json!(-1)),
        ("mShadowSize", json!(-1)),
        ("mShadowBlur", json!(-1)),
        ("mShadowBlur", json!(1e100)),
    ] {
        let mut text = shadow_text();
        text["mTextParam"][field] = invalid;
        let decoded = read(&text).unwrap().uniform().unwrap();
        assert_eq!(decoded.document, expected, "{field}");
        assert!(
            decoded
                .omitted
                .iter()
                .any(|feature| feature.to_string().contains("shadow omitted")),
            "{field}"
        );
    }
    for field in [
        "mShadowColor",
        "mShadowOpacity",
        "mShadowAngle",
        "mShadowOffset",
        "mShadowSize",
        "mShadowBlur",
    ] {
        let mut text = shadow_text();
        text["mTextParam"].as_object_mut().unwrap().remove(field);
        let decoded = read(&text).unwrap().uniform().unwrap();
        assert_eq!(decoded.document, expected, "{field}");
        assert!(
            decoded
                .omitted
                .iter()
                .any(|feature| feature.to_string().contains(field)),
            "{field}"
        );
        text["mTextParam"]["mShadowVisible"] = json!(false);
        assert_eq!(read(&text).unwrap().uniform().unwrap().document, expected);
    }
}

#[test]
fn legacy_stale_byte_count_retains_validated_editable_text() {
    let text = legacy_source_text();
    let expected = read(&text).unwrap().uniform().unwrap().document;
    let valid = legacy_source_text_payload(&text.to_string());
    let actual_count = u64::try_from(valid.len() - 8).unwrap();
    for count in [actual_count - 74, actual_count + 74] {
        let mut payload = valid.clone();
        payload[..8].copy_from_slice(&count.to_le_bytes());
        let decoded = decode(&payload).expect("a stale count must not discard complete valid text");
        assert!(decoded.omitted.iter().any(|feature| {
            let diagnostic = feature.to_string();
            diagnostic.contains("byte count") && diagnostic.contains("complete validated JSON")
        }));
        assert_eq!(decoded.uniform().unwrap().document, expected);
    }
    // A stale count must never admit a truncated document or trailing bytes.
    for payload in [
        valid[..valid.len() - 2].to_vec(),
        [valid.as_slice(), b"\0\0"].concat(),
    ] {
        assert!(decode(&payload).is_err());
    }
}

#[test]
fn legacy_missing_caps_retains_actual_text_and_supported_style() {
    let text = legacy_source_text();
    let expected = read(&text).unwrap().uniform().unwrap().document;
    let mut without_caps = text.clone();
    without_caps["mTextParam"]["mStyleSheet"]
        .as_object_mut()
        .unwrap()
        .remove("mCapsOption");
    let actual =
        read(&without_caps).expect("an absent optional caps control must not discard text");
    assert!(actual.omitted.iter().any(|feature| {
        let diagnostic = feature.to_string();
        diagnostic.contains("mCapsOption") && diagnostic.contains("normal case")
    }));
    assert_eq!(actual.uniform().unwrap().document, expected);

    // Absence is not a license to ignore malformed saved runs.
    for value in [
        Value::Null,
        json!(0),
        json!({"mParamValues": []}),
        json!({"mParamValues": [[0, 0], [0, 2]]}),
        json!({"mParamValues": [[0, 0], [2, 2], [1, 0]]}),
        json!({"mParamValues": [[1, 0]]}),
    ] {
        let mut malformed = text.clone();
        malformed["mTextParam"]["mStyleSheet"]["mCapsOption"] = value;
        assert!(read(&malformed).is_err());
    }
}

#[test]
fn legacy_multiple_runs_use_first_values_with_field_diagnostics() {
    let text = legacy_source_text();
    let expected = read(&text).unwrap().uniform().unwrap().document;
    for (field, second) in [
        ("mCapsOption", json!(2)),
        ("mFontName", json!("OtherFont")),
        ("mFontSize", json!(30.0)),
        ("mFillVisible", json!(true)),
        ("mFillOverStroke", json!(false)),
        ("mStrokeColor", json!(0)),
        ("mStrokeWidth", json!(5.0)),
        ("mStrokeVisible", json!(false)),
        ("mTracking", json!(10.0)),
    ] {
        let mut mixed = text.clone();
        mixed["mTextParam"]["mStyleSheet"][field]["mParamValues"]
            .as_array_mut()
            .unwrap()
            .push(json!([2, second]));
        let actual = read(&mixed).unwrap();
        assert!(
            actual.omitted.iter().any(|feature| {
                let diagnostic = feature.to_string();
                diagnostic.contains(field) && diagnostic.contains("first style value")
            }),
            "{field}"
        );
        assert_eq!(actual.uniform().unwrap().document, expected, "{field}");
    }
}

#[test]
fn legacy_fill_runs_normalize_caps_and_boundaries_without_guessing() {
    use crate::schema::text::{PrRgb, PrTextFillSpan};
    let mut text = legacy_source_text();
    let style = &mut text["mTextParam"]["mStyleSheet"];
    style["mText"] = json!("aß😀b");
    style["mCapsOption"] = legacy_run(json!(2));
    style["mFillColor"]["mParamValues"] = json!([
        [0, 0],
        [1, 16777215],
        [2, 16777215],
        [3, 16777215],
        [4, 0],
        [5, 255]
    ]);
    let decoded = read(&text).unwrap();
    assert!(decoded
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("insertion defaults")));
    assert_eq!(
        decoded.uniform().unwrap().document.fill_spans,
        vec![PrTextFillSpan {
            range: 1..4,
            color: PrRgb([255; 3]),
        }]
    );
    text["mTextParam"]["mStyleSheet"]["mFillColor"]["mParamValues"] =
        json!([[0, 0], [3, 16777215]]);
    let decoded = read(&text).unwrap();
    assert!(decoded
        .omitted
        .iter()
        .any(|feature| feature.to_string().contains("splits a surrogate")));
    assert!(decoded.uniform().unwrap().document.fill_spans.is_empty());
}

#[test]
fn legacy_changing_visibility_is_rejected_without_exposing_concealed_characters() {
    for field in ["mFillVisible", "mStrokeVisible"] {
        for (first, second) in [(true, false), (false, true)] {
            let mut text = legacy_source_text();
            let style = &mut text["mTextParam"]["mStyleSheet"];
            style["mFillVisible"] = legacy_run(json!(false));
            style["mStrokeVisible"] = legacy_run(json!(false));
            style[field]["mParamValues"] = json!([[0, first], [2, second]]);
            let error = match read(&text) {
                Ok(_) => panic!("{field}: changing visibility must not expose hidden text"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains("preserve concealment"),
                "{error}"
            );
        }
    }
}

#[test]
fn legacy_outline_only_changing_width_is_rejected_to_preserve_concealment() {
    for widths in [json!([[0, 5.0], [2, 0.0]]), json!([[0, 0.0], [2, 5.0]])] {
        let mut text = legacy_source_text();
        let style = &mut text["mTextParam"]["mStyleSheet"];
        style["mFillVisible"] = legacy_run(json!(false));
        style["mStrokeVisible"] = legacy_run(json!(true));
        style["mStrokeWidth"]["mParamValues"] = widths;
        let error = match read(&text) {
            Ok(_) => panic!("changing outline width must not reveal concealed characters"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("preserve concealment"),
            "{error}"
        );

        text["mTextParam"]["mStyleSheet"]["mFillVisible"] = legacy_run(json!(true));
        let filled = read(&text).expect("visible fill permits the first-width approximation");
        assert!(filled.omitted.iter().any(|feature| {
            let diagnostic = feature.to_string();
            diagnostic.contains("mStrokeWidth") && diagnostic.contains("first style value")
        }));
    }
    let mut text = legacy_source_text();
    let style = &mut text["mTextParam"]["mStyleSheet"];
    style["mFillVisible"] = legacy_run(json!(false));
    style["mStrokeVisible"] = legacy_run(json!(true));
    style["mStrokeWidth"]["mParamValues"] = json!([[0, 5.0], [2, 5.0]]);
    let document = read(&text).unwrap().uniform().unwrap().document;
    assert!(document.fill.is_none());
    assert_eq!(document.stroke.unwrap().width, 5.0);
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
fn legacy_run_metadata_is_diagnosed_without_losing_actual_text_or_style() {
    let expected = read(&legacy_source_text())
        .unwrap()
        .uniform()
        .unwrap()
        .document;
    let mut text = legacy_source_text();
    text["mTextParam"]["mStyleSheet"]["mFontSize"]["unknownControl"] = json!(true);

    let actual = read(&text)
        .expect("unmapped run metadata must not discard known text styling")
        .uniform()
        .unwrap();
    assert_eq!(actual.document, expected);
    assert_eq!(actual.omitted.len(), 1);
    assert!(actual.omitted[0]
        .to_string()
        .contains("mFontSize.unknownControl"));
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
    text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x1_123456));
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
