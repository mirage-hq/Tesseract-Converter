//! Text export contracts for edited FX documents.
//!
//! Inputs in this module are explicitly authored editable FX documents. Native
//! re-import and record checks are supplementary structural evidence only.
//! Separate Adobe acceptance/render provenance is recorded in the support ledger.

use std::collections::{BTreeSet, HashSet};

use fx_schema::{
    FxItemId, PropertyAnimator, PropertyTarget, TextLayer,
    animator::{AnimatorData, KeyframeId, PropertyKeyframe, PropertyKeyframeTrack},
};
use sha2::{Digest, Sha256};

use crate::structure_document::text::cos::{self, Value as CosValue};

use super::*;

#[test]
fn range_enum_full_export_matches_native_descriptors_and_edited_input() {
    fn property<'a>(
        chunks: &'a [crate::rifx::Chunk],
        name: &str,
    ) -> Option<&'a [crate::rifx::Chunk]> {
        if let Ok(records) = crate::properties::runs(chunks)
            && let Some((_, record)) = records.into_iter().find(|(key, _)| *key == name)
        {
            return Some(record);
        }
        chunks.iter().find_map(|chunk| {
            chunk
                .children()
                .and_then(|children| property(children, name))
        })
    }
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/text/selector-enums/source.aep"
    ))
    .unwrap();
    let native_layer = &layers(&native)[0];
    let texts = imported_texts(&native);
    let selector = &texts[0].animators[0].selectors[0];
    assert_eq!(
        selector.units,
        fx_schema::text_animator::SelectorUnits::Index
    );
    assert_eq!(
        selector.based_on,
        fx_schema::text_animator::SelectorBasis::Lines
    );
    assert_eq!(
        selector.mode,
        fx_schema::text_animator::SelectorMode::Subtract
    );
    assert_eq!(
        selector.shape,
        fx_schema::text_animator::SelectorShape::Triangle
    );
    for (basis, ordinal) in [("lines", 4.0), ("words", 3.0)] {
        let mut text = all_channel_text();
        let selected = &mut text["animators"][0]["selectors"][0];
        selected["units"] = json!("index");
        selected["basedOn"] = json!(basis);
        selected["mode"] = json!("subtract");
        let document = explicit_document(vec![guide_layer(5299), text], Vec::new());
        let output = to_aep(&document).unwrap();
        let generated = read_project(&output.bytes).unwrap();
        for (name, value) in [
            ("ADBE Text Range Units", 2.0),
            ("ADBE Text Range Type2", ordinal),
            ("ADBE Text Selector Mode", 2.0),
            ("ADBE Text Range Shape", 4.0),
        ] {
            let expected = crate::properties::unique_list(
                property(&native_layer.content, name).unwrap(),
                *b"tdbs",
            )
            .unwrap();
            let actual = crate::properties::unique_list(
                property(&layers(&generated)[0].content, name).unwrap(),
                *b"tdbs",
            )
            .unwrap();
            let mut descriptor = crate::properties::data(expected, *b"tdb4")
                .unwrap()
                .to_vec();
            descriptor[12..16].copy_from_slice(&24576_u32.to_be_bytes());
            assert_eq!(
                crate::properties::data(actual, *b"tdb4").unwrap(),
                descriptor,
                "{name}"
            );
            assert_eq!(
                crate::properties::data(actual, *b"tdsb").unwrap(),
                crate::properties::data(expected, *b"tdsb").unwrap(),
                "{name}"
            );
            assert_eq!(
                crate::properties::read_numeric(actual).unwrap().values,
                [value],
                "{name}"
            );
            if basis == "lines" {
                assert_eq!(
                    crate::properties::data(actual, *b"cdat").unwrap(),
                    crate::properties::data(expected, *b"cdat").unwrap(),
                    "{name}"
                );
            }
        }
    }
}

// Literal editable subtree from private HUD case SHA-256
// 0d55ef5da45bff2e2166d5df5cd1b92618f4028f1a674f80b05f6de3c44a7504.
// This public structural description is not an Adobe-native/render oracle.
fn launch_hud_value() -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/text/launch_hud_structural.json"
    ))
    .unwrap();
    // Stored activeRange is expressed explicitly as the equivalent editable
    // windowed playback; the root's offset clock remains source-local zero.
    let root = &mut value["composition"]["layers"][0];
    root.as_object_mut().unwrap().remove("activeRange");
    root["playback"] = affine_playback([60_400, 64_400], [0, 4_000]);
    root["layers"][0]
        .as_object_mut()
        .unwrap()
        .remove("activeRange");
    root["layers"][0]["playback"] = identity_playback(4_000);
    value
}

#[test]
fn launch_hud_root_text_retention_preserves_source_structure_and_direct_text() {
    for direct in [false, true] {
        let mut value = launch_hud_value();
        if direct {
            let children = value["composition"]["layers"][0]["layers"][0]["layers"].take();
            value["composition"]["layers"][0]["layers"] = children;
        }
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = to_aep(&document).unwrap();
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let root = native_named(&native, "SCENE S24 · s24_fourier");
        assert_eq!(comp_interval(root), (60.4, 64.4));
        let children = composition_layers(&native, root.record.source_id());
        let label = children
            .iter()
            .find(|layer| layer.name.as_ref() == "spec label · 1080P / 30 FPS")
            .unwrap();
        assert_eq!(comp_interval(label), (0.0, 4.0));
        let position = crate::properties::read_transform(&label.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .unwrap()
            .values;
        assert_eq!(position, vec![1516.5, 61.0, 0.0]);
        if direct {
            assert_eq!(label.record.parent_id(), 0);
        } else {
            let hud = children
                .iter()
                .find(|layer| layer.name.as_ref() == "HUD")
                .unwrap();
            assert_eq!(label.record.parent_id(), hud.record.id());
            assert_eq!(comp_interval(hud), (0.0, 4.0));
        }
        let names = children
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>();
        assert!(names.contains(&"crosshairs 22 px #CFF2E4"));
        assert!(names.contains(&"HUD frame 2 px #414141 (inset 33)"));
        let text = imported_texts(&native)
            .into_iter()
            .find(|text| text.source_text.text == "1080P / 30 FPS")
            .unwrap();
        assert_eq!(text.source_text.font_family.as_ref(), "Menlo");
        assert_eq!(text.source_text.font_style.as_ref(), "Regular");
        assert_eq!(text.source_text.font_size.value(), 18.0);
        assert_eq!(text.source_text.box_size, Some([306.5, 46.8]));
        assert_eq!(text.source_text.box_position, Some([0.0, 0.0]));
        assert_eq!(
            text.source_text.justification,
            fx_schema::Justification::Right
        );
        assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
    }
}

#[test]
fn launch_hud_root_text_retention_keeps_unsafe_text_guards() {
    for unsafe_case in [
        "root_transform",
        "text_effect",
        "ancestor_effect",
        "ancestor_blend_mode",
        "motion_blur",
    ] {
        let mut value = launch_hud_value();
        let root = &mut value["composition"]["layers"][0];
        match unsafe_case {
            "root_transform" => root["transform"]["position"] = json!([1.0, 0.0]),
            "text_effect" => {
                root["layers"][0]["layers"][0]["effects"] =
                    json!([{"type": "gaussianBlur", "blurriness": 12.0}])
            }
            "ancestor_effect" => {
                root["layers"][0]["effects"] = json!([{"type": "gaussianBlur", "blurriness": 12.0}])
            }
            "ancestor_blend_mode" => root["layers"][0]["blendMode"] = json!("multiply"),
            "motion_blur" => root["layers"][0]["layers"][0]["motionBlur"] = json!(true),
            _ => unreachable!(),
        }
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        if unsafe_case != "root_transform" {
            let LayerData::Group(root) = document.composition().layers()[0].data() else {
                panic!("fixture root must be a Group");
            };
            assert!(
                !root_plain_text_bounds_layer_is_safe(
                    &root.layers[0],
                    Time::from_millis(4_000),
                    &AnimationIndex::new(&[]),
                ),
                "{unsafe_case}: unsafe Text ancestry cannot be certified by bounds projection"
            );
        }
        let output = to_aep(&document).unwrap();
        // Other existing profiles may now export these controls. They must not
        // be accepted by this narrower final-root bounds-only projection.
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("retains plain Text")),
            "{unsafe_case}"
        );
    }
}

#[test]
fn launch_hud_root_text_retention_validates_nontext_geometry() {
    let mut value = launch_hud_value();
    value["composition"]["layers"][0]["layers"][0]["layers"][1]["shape"]["path"]["commands"] =
        json!([]);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = to_aep(&document).unwrap();
    assert!(
        output.omitted_layer_ids.contains(&LayerId::new(2_400_000)),
        "{:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("proved Shape render enclosure")),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn static_text_skew_retains_native_drawable_document_and_distinct_helpers() {
    for skew in [1.0, -0.5, -2.5] {
        let mut source = text_layer(2509, "Editable skew", source_text("n", false));
        source["transform"]["skew"] = json!(skew);
        source["transform"]["anchorPoint"] = json!([12.0, -7.0]);
        let document = explicit_document(vec![source], Vec::new());
        let output = to_aep(&document).unwrap();
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 4, "{:?}", output.diagnostics);
        let ids: HashSet<_> = layers(&native)
            .iter()
            .map(|layer| layer.record.id())
            .collect();
        assert_eq!(ids.len(), 4);
        let drawable = native_named(&native, "Editable skew");
        let placement = native_named(&native, "Editable skew — Skew placement");
        let factor = native_named(&native, "Editable skew — Skew factor");
        let basis = native_named(&native, "Editable skew — Skew basis");
        assert_eq!(drawable.record.layer_type(), 3);
        assert_eq!(drawable.record.parent_id(), basis.record.id());
        assert_eq!(basis.record.parent_id(), factor.record.id());
        assert_eq!(factor.record.parent_id(), placement.record.id());
        for layer in [drawable, placement, factor, basis] {
            assert_eq!(comp_interval(layer), (0.5, 3.0));
        }
        // The importer gives partial-lifetime Text an occurrence wrapper;
        // the generated source-local content owns a separate source clock.
        let converted = to_structural_fx_document(&native, Some(1)).unwrap();
        let owner = find_layer_named(converted.document.composition().layers(), "Editable skew")
            .expect("native semantic name remains on the occurrence owner");
        assert!(matches!(owner.data(), LayerData::Group(_)));
        let texts = imported_texts(&native);
        assert_eq!(texts.len(), 1);
        let text = &texts[0];
        assert_eq!(text.name, "Source content clock");
        let properties = crate::properties::read_transform(&drawable.content).unwrap();
        let anchor = properties
            .iter()
            .find(|property| property.match_name == "ADBE Anchor Point")
            .unwrap();
        assert_eq!(&anchor.numeric.as_ref().unwrap().values[..2], &[12.0, -7.0]);
        assert_eq!(text.transform.anchor_point, [0.0; 2]);
        assert_eq!(text.source_text.text, "n");
        assert_eq!(text.source_text.stroke_width.value(), 3.0);
        assert_eq!(text.transform.scale, [100.0; 2]);
        assert_eq!(text.transform.rotation, 0.0);
        assert_eq!(text.transform.opacity.value(), 100.0);
        assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
    }
}

#[test]
fn p046_grouped_text_skew_keeps_helper_parents_inside_their_composition() {
    // P046 Group7140 has animated opacity: the Group is a precomposition,
    // not a native Null in its own source. Its Text placement helpers must
    // not reference that outer occurrence from inside the source.
    for precomposition in [true, false] {
        let mut text = text_layer(7358, "Grouped skew", source_text("m", false));
        text["transform"]["skew"] = json!(6.5);
        let group = json!({
            "type": "Group", "id": 7140, "name": "Skew group",
            "playback": identity_playback(4000),
            "transform": identity_transform(),
            "layers": [text]
        });
        let entries = if precomposition {
            vec![layer_entry(
                LayerId::new(7140),
                PropType::Opacity,
                [
                    (500, PropertyValue::Float(0.0)),
                    (1250, PropertyValue::Float(100.0)),
                ],
                PropertyKeyframeEasing::Linear,
            )]
        } else {
            Vec::new()
        };
        let mut value = imported();
        value["duration"] = json!(4.0);
        value["composition"]["layers"] = json!([group]);
        value["composition"]["dynamics"] = json!({"entries": entries});
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = to_aep(&document).unwrap();
        assert!(
            !output.omitted_layer_ids.contains(&LayerId::new(7140)),
            "{:?}",
            output.diagnostics
        );
        assert!(
            !output.omitted_layer_ids.contains(&LayerId::new(7358)),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let group = native_named(&native, "Skew group");
        let source = if precomposition {
            composition_layers(&native, group.record.source_id())
        } else {
            layers(&native)
        };
        let named = |name: &str| {
            source
                .iter()
                .find(|layer| layer.name.as_ref() == name)
                .unwrap_or_else(|| panic!("missing {name}, precomposition={precomposition}, layers={:?}, diagnostics={:?}", source.iter().map(|layer| layer.name.as_ref()).collect::<Vec<_>>(), output.diagnostics))
        };
        let placement = named("Grouped skew — Skew placement");
        let factor = named("Grouped skew — Skew factor");
        let basis = named("Grouped skew — Skew basis");
        let drawable = named("Grouped skew");
        assert_eq!(drawable.record.layer_type(), 3);
        assert_eq!(drawable.record.parent_id(), basis.record.id());
        assert_eq!(basis.record.parent_id(), factor.record.id());
        assert_eq!(factor.record.parent_id(), placement.record.id());
        assert_eq!(
            placement.record.parent_id(),
            if precomposition { 0 } else { group.record.id() }
        );
        if precomposition {
            let source = composition_layers(&native, group.record.source_id());
            assert_eq!(source.len(), 4);
            assert!(
                source
                    .iter()
                    .all(|layer| layer.record.parent_id() != group.record.id())
            );
            let properties = crate::properties::read_transform(&group.content).unwrap();
            let opacity = properties
                .iter()
                .find(|property| property.match_name == "ADBE Opacity")
                .unwrap();
            assert_eq!(opacity.numeric.as_ref().unwrap().keyframes.len(), 2);
        } else {
            assert!(group.record.flags().null_layer);
        }
        assert_eq!(generated_document_texts(&output.bytes), vec!["m\r"]);
    }
}

#[test]
fn fixed_text_skew_keeps_animated_placement_and_drawable_tracks_separate() {
    let id = LayerId::new(2509);
    let mut source = text_layer(
        id.value(),
        "Editable animated skew",
        source_text("n", false),
    );
    source["transform"]["skew"] = json!(1.0);
    source["transform"]["anchorPoint"] = json!([12.0, -7.0]);
    let entries = [
        (PropType::PositionY, 180.0, 240.0),
        (PropType::Rotation, -15.0, 35.0),
        (PropType::ScaleX, 0.0, 150.0),
        (PropType::ScaleY, 40.0, 80.0),
        (PropType::Opacity, 20.0, 75.0),
    ]
    .into_iter()
    .map(|(property, first, last)| {
        layer_entry(
            id,
            property,
            [
                (500, PropertyValue::Float(first)),
                (1250, PropertyValue::Float(last)),
            ],
            PropertyKeyframeEasing::Linear,
        )
    })
    .collect();
    let output = to_aep(&explicit_document(vec![source], entries)).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 4, "{:?}", output.diagnostics);
    let drawable = native_named(&native, "Editable animated skew");
    let placement = native_named(&native, "Editable animated skew — Skew placement");
    let factor = native_named(&native, "Editable animated skew — Skew factor");
    let basis = native_named(&native, "Editable animated skew — Skew basis");
    assert_eq!(drawable.record.layer_type(), 3);
    assert_eq!(drawable.record.parent_id(), basis.record.id());
    assert_eq!(basis.record.parent_id(), factor.record.id());
    assert_eq!(factor.record.parent_id(), placement.record.id());
    for layer in [placement, factor, basis] {
        assert_eq!(layer.record.layer_type(), 0);
        assert!(layer.record.flags().null_layer);
    }
    for layer in [drawable, placement, factor, basis] {
        assert_eq!(comp_interval(layer), (0.5, 3.0));
        // Active-range clipping does not shift owner-local animation keys.
        // with_active_range sets the timeline start to 0.5 and the native
        // source interval to 0..2.5; property keys retain composition time.
        let clock = record_clock(layer)
            .map(|(numerator, denominator)| f64::from(numerator) / f64::from(denominator));
        assert_eq!(clock, [0.5, 0.0, 2.5, 1.0]);
    }
    let placement_properties = crate::properties::read_transform(&placement.content).unwrap();
    let drawable_properties = crate::properties::read_transform(&drawable.content).unwrap();
    for (properties, name, expected) in [
        (
            &placement_properties,
            "ADBE Position",
            vec![vec![320.0, 180.0, 0.0], vec![320.0, 240.0, 0.0]],
        ),
        (
            &placement_properties,
            "ADBE Rotate Z",
            vec![vec![-15.0], vec![35.0]],
        ),
        (
            &drawable_properties,
            "ADBE Scale",
            vec![vec![0.0, 0.4, 1.0], vec![1.5, 0.8, 1.0]],
        ),
        (
            &drawable_properties,
            "ADBE Opacity",
            vec![vec![0.2], vec![0.75]],
        ),
    ] {
        let property = properties
            .iter()
            .find(|property| property.match_name == name)
            .unwrap_or_else(|| panic!("missing native {name}"));
        let numeric = property.numeric.as_ref().unwrap();
        assert!(numeric.animated, "{name}");
        assert_eq!(numeric.keyframes.len(), 2, "{name}");
        for ((key, values), time) in numeric.keyframes.iter().zip(expected).zip([0.5, 1.25]) {
            assert_eq!(key.values, values, "{name}");
            assert_eq!(key.time_secs, time, "{name}");
            assert_eq!(key.in_interpolation, 1, "{name}");
            assert_eq!(key.out_interpolation, 1, "{name}");
        }
    }
    for (properties, names) in [
        (&placement_properties, &["ADBE Scale", "ADBE Opacity"][..]),
        (
            &drawable_properties,
            &[
                "ADBE Position",
                "ADBE Position_0",
                "ADBE Position_1",
                "ADBE Rotate Z",
            ][..],
        ),
    ] {
        for name in names {
            for property in properties
                .iter()
                .filter(|property| property.match_name == *name)
            {
                let numeric = property.numeric.as_ref().unwrap();
                assert!(
                    !numeric.animated,
                    "{name} must not be duplicated across helpers"
                );
                assert!(numeric.keyframes.is_empty(), "{name}");
            }
        }
    }
    let texts = imported_texts(&native);
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].source_text.text, "n");
    assert_eq!(texts[0].source_text.stroke_width.value(), 3.0);
    let anchor = drawable_properties
        .iter()
        .find(|property| property.match_name == "ADBE Anchor Point")
        .unwrap();
    assert_eq!(&anchor.numeric.as_ref().unwrap().values[..2], &[12.0, -7.0]);
    assert_eq!(texts[0].transform.anchor_point, [0.0; 2]);
    let cos = source_text_cos(drawable);
    assert!(!cos.is_empty());
    for helper in [factor, basis] {
        let properties = crate::properties::read_transform(&helper.content).unwrap();
        assert!(
            properties
                .iter()
                .filter_map(|property| property.numeric.as_ref().ok())
                .all(|numeric| !numeric.animated && numeric.keyframes.is_empty())
        );
    }
    assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
}

#[test]
fn fixed_text_skew_preserves_independent_position_knots_on_placement() {
    for constant_x in [false, true] {
        let id = LayerId::new(12141);
        let name = "Independent skew Position";
        let mut source = text_layer(id.value(), name, source_text("m", false));
        source["transform"]["skew"] = json!(5.0);
        source["transform"]["anchorPoint"] = json!([49.359, -28.125]);
        let x_values = if constant_x {
            [70.858, 70.858]
        } else {
            [70.858, 82.858]
        };
        let y_points = [
            (418, 229.704),
            (508, 141.704),
            (588, 159.704),
            (896, 159.704),
            (946, 195.704),
            (1036, 349.704),
        ];
        let cubic = PropertyKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.0,
            x2: 0.1,
            y2: 1.0,
        };
        let mut y_entry = layer_entry(
            id,
            PropType::PositionY,
            [
                (418, PropertyValue::Float(229.704)),
                (1036, PropertyValue::Float(349.704)),
            ],
            cubic,
        );
        y_entry.animator = PropertyAnimator::keyframes(track(
            "independent-y",
            y_points
                .into_iter()
                .map(|(time, value)| (time, PropertyValue::Float(value)))
                .collect(),
            cubic,
        ));
        let entries = vec![
            layer_entry(
                id,
                PropType::PositionX,
                [
                    (896, PropertyValue::Float(x_values[0])),
                    (1036, PropertyValue::Float(x_values[1])),
                ],
                cubic,
            ),
            y_entry,
            layer_entry(
                id,
                PropType::Rotation,
                [
                    (500, PropertyValue::Float(-15.0)),
                    (1250, PropertyValue::Float(35.0)),
                ],
                PropertyKeyframeEasing::Linear,
            ),
            layer_entry(
                id,
                PropType::ScaleX,
                [
                    (500, PropertyValue::Float(96.276)),
                    (710, PropertyValue::Float(120.0)),
                ],
                cubic,
            ),
            layer_entry(
                id,
                PropType::Opacity,
                [
                    (500, PropertyValue::Float(20.0)),
                    (1250, PropertyValue::Float(75.0)),
                ],
                PropertyKeyframeEasing::Linear,
            ),
        ];
        let output = to_aep(&explicit_document(vec![source], entries)).unwrap();
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 4, "{:?}", output.diagnostics);
        let drawable = native_named(&native, name);
        let placement = native_named(&native, "Independent skew Position — Skew placement");
        let factor = native_named(&native, "Independent skew Position — Skew factor");
        let basis = native_named(&native, "Independent skew Position — Skew basis");
        assert_eq!(drawable.record.layer_type(), 3);
        assert_eq!(drawable.record.parent_id(), basis.record.id());
        assert_eq!(basis.record.parent_id(), factor.record.id());
        assert_eq!(factor.record.parent_id(), placement.record.id());
        let properties = crate::properties::read_transform(&placement.content).unwrap();
        for (name, expected) in [
            (
                "ADBE Position_0",
                vec![(896, x_values[0]), (1036, x_values[1])],
            ),
            ("ADBE Position_1", y_points.to_vec()),
            ("ADBE Rotate Z", vec![(500, -15.0), (1250, 35.0)]),
        ] {
            let property = properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap();
            let numeric = property.numeric.as_ref().unwrap();
            assert_eq!(numeric.keyframes.len(), expected.len(), "{name}");
            for (key, (millis, value)) in numeric.keyframes.iter().zip(expected) {
                // The established 24fps native property clock has 1024 ticks/frame.
                let expected_time = (millis as f64 * 24_576.0 / 1000.0).round() / 24_576.0;
                assert_eq!(key.time_secs, expected_time, "{name}");
                assert_eq!(key.values, vec![value], "{name}");
            }
        }
        let drawable_properties = crate::properties::read_transform(&drawable.content).unwrap();
        for name in [
            "ADBE Position",
            "ADBE Position_0",
            "ADBE Position_1",
            "ADBE Rotate Z",
        ] {
            for property in drawable_properties
                .iter()
                .filter(|property| property.match_name == name)
            {
                assert!(
                    property.numeric.as_ref().unwrap().keyframes.is_empty(),
                    "{name}"
                );
            }
        }
        for name in ["ADBE Scale", "ADBE Opacity"] {
            let property = drawable_properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap();
            assert_eq!(
                property.numeric.as_ref().unwrap().keyframes.len(),
                2,
                "{name}"
            );
            let helper_property = properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap();
            assert!(
                helper_property
                    .numeric
                    .as_ref()
                    .unwrap()
                    .keyframes
                    .is_empty(),
                "{name}"
            );
        }
        assert_eq!(imported_texts(&native)[0].source_text.text, "m");
        assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
    }
}

#[test]
fn fixed_text_skew_independent_position_keeps_spatial_tangent_guard() {
    let id = LayerId::new(12141);
    let mut source = text_layer(
        id.value(),
        "Unsupported spatial skew",
        source_text("m", false),
    );
    source["transform"]["skew"] = json!(5.0);
    let mut x = serde_json::to_value(layer_entry(
        id,
        PropType::PositionX,
        [
            (500, PropertyValue::Float(10.0)),
            (1000, PropertyValue::Float(20.0)),
        ],
        PropertyKeyframeEasing::Linear,
    ))
    .unwrap();
    x["animator"]["keyframes"][0]["spatialOutTangent"] = json!(5.0);
    let entries = vec![
        serde_json::from_value(x).unwrap(),
        layer_entry(
            id,
            PropType::PositionY,
            [
                (600, PropertyValue::Float(30.0)),
                (1200, PropertyValue::Float(40.0)),
            ],
            PropertyKeyframeEasing::Linear,
        ),
    ];
    let output = to_aep(&explicit_document(
        vec![
            source,
            text_layer(12142, "Retained sibling", source_text("n", false)),
        ],
        entries,
    ))
    .unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(id)
                && diagnostic.message.contains("different key times")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(imported_texts(&native).len(), 1);
    assert_eq!(imported_texts(&native)[0].source_text.text, "n");
}

#[test]
fn fixed_text_skew_rejections_preserve_convertible_text_sibling() {
    for (field, value) in [
        ("motionBlur", json!(true)),
        ("parent", json!(2607)),
        ("rotationY", json!(1.0)),
        ("rotationX", json!(1.0)),
        (
            "effects",
            json!([{"id": 9001, "enabled": true, "effect": {
                "type": "exposure", "exposure": 1, "offset": 0, "gammaCorrection": 1
            }}]),
        ),
        ("animatedSkew", json!(null)),
    ] {
        let mut source = text_layer(2509, "Rejected skew", source_text("n", false));
        source["transform"]["skew"] = json!(1.0);
        let mut entries = Vec::new();
        if field == "animatedSkew" {
            entries.push(layer_entry(
                LayerId::new(2509),
                PropType::Skew,
                [
                    (500, PropertyValue::Float(1.0)),
                    (1250, PropertyValue::Float(2.0)),
                ],
                PropertyKeyframeEasing::Linear,
            ));
        } else if matches!(field, "motionBlur" | "effects" | "parent") {
            source[field] = value;
        } else {
            source["transform"][field] = value;
        }
        let sibling = text_layer(2607, "Retained sibling", source_text("d", false));
        let output = to_aep(&explicit_document(vec![source, sibling], entries)).unwrap();
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
        let sibling = native_named(&native, "Retained sibling");
        assert_eq!(sibling.record.layer_type(), 3);
        assert_eq!(comp_interval(sibling), (0.5, 3.0));
        let converted = to_structural_fx_document(&native, Some(1)).unwrap();
        let owner = find_layer_named(
            converted.document.composition().layers(),
            "Retained sibling",
        )
        .expect("plain partial Text retains its native occurrence name");
        assert!(matches!(owner.data(), LayerData::Group(_)));
        let texts = imported_texts(&native);
        assert_eq!(texts[0].name, "Source content clock");
        assert_eq!(texts[0].source_text.text, "d");
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(2509)))
        );
    }
}

fn identity_transform() -> Value {
    json!({
        "anchorPoint": [0.0, 0.0],
        "position": [320.0, 180.0],
        "scale": [100.0, 100.0],
        "rotation": 0.0,
        "opacity": 100.0
    })
}

fn source_text(text: &str, box_text: bool) -> Value {
    let mut value = json!({
        "text": text,
        "fontFamily": "Inter-Regular",
        "fontStyle": "Regular",
        "fontSize": 42.0,
        "applyFill": true,
        "fillColor": [0.1, 0.2, 0.3, 0.9],
        "applyStroke": true,
        "strokeColor": [0.8, 0.7, 0.6, 0.5],
        "strokeWidth": 3.0,
        "strokeOverFill": true,
        "justification": "center",
        "tracking": 24.0,
        "leading": 54.0,
        "baselineShift": -2.0,
        "boxText": box_text,
        "allCaps": true
    });
    if box_text {
        value["boxSize"] = json!([420.0, 180.0]);
        value["boxPosition"] = json!([-210.0, -90.0]);
    }
    value
}

fn text_layer(id: u64, name: &str, source: Value) -> Value {
    json!({
        "type": "Text",
        "id": id,
        "name": name,
        "parent": null,
        "activeRange": {"start": 500, "duration": 2500},
        "transform": identity_transform(),
        "sourceText": source
    })
}

fn guide_layer(id: u64) -> Value {
    json!({
        "type": "Shape",
        "id": id,
        "name": "Fresh text path guide",
        "parent": null,
        "activeRange": {"start": 500, "duration": 2500},
        "transform": identity_transform(),
        "shape": {
            "path": {"commands": [
                {"type": "moveTo", "x": -160.0, "y": 0.0},
                {"type": "lineTo", "x": 160.0, "y": 0.0}
            ]}
        }
    })
}

fn explicit_document(
    layers: Vec<Value>,
    entries: Vec<AnimationGraphEntry>,
) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": entries});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn track(
    prefix: &str,
    values: Vec<(i64, PropertyValue)>,
    easing: PropertyKeyframeEasing,
) -> PropertyKeyframeTrack {
    PropertyKeyframeTrack::new(
        values
            .into_iter()
            .enumerate()
            .map(|(index, (time, value))| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("pr4442-text-{prefix}-{time}-{index}")),
                    fx_schema::TimeOffset::from_millis(time),
                    value,
                    easing,
                )
            })
            .collect(),
    )
    .unwrap()
}

fn item_entry(
    id: FxItemId,
    property: &'static str,
    values: [(i64, PropertyValue); 2],
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::fx_item(id, property),
        animator: PropertyAnimator::keyframes(track(
            &format!("item-{}-{property}", id.value()),
            values.into_iter().collect(),
            PropertyKeyframeEasing::Linear,
        )),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn layer_entry(
    id: LayerId,
    property: PropType,
    values: [(i64, PropertyValue); 2],
    easing: PropertyKeyframeEasing,
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(id, property),
        animator: PropertyAnimator::keyframes(track(
            &format!("layer-{}-{property:?}", id.value()),
            values.into_iter().collect(),
            easing,
        )),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn constant_layer_entry(
    id: LayerId,
    property: PropType,
    value: PropertyValue,
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(id, property),
        animator: PropertyAnimator::constant(value).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn disabled_layer_entry(
    id: LayerId,
    property: PropType,
    values: [(i64, PropertyValue); 2],
    disabled_value: PropertyValue,
) -> AnimationGraphEntry {
    let target = PropertyTarget::layer(id, property);
    let animator = PropertyAnimator::keyframes(track(
        &format!("disabled-{}-{property:?}", id.value()),
        values.into_iter().collect(),
        PropertyKeyframeEasing::Hold,
    ));
    let mut data = animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value: stored_disabled_value,
        ..
    } = &mut data
    else {
        panic!("keyed text fixture")
    };
    *enabled = false;
    *stored_disabled_value = Some(disabled_value);
    AnimationGraphEntry {
        target,
        animator: PropertyAnimator::from_data(&data).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn collect_text<'a>(layers: &'a [fx_schema::Layer], output: &mut Vec<&'a TextLayer>) {
    for layer in layers {
        match layer.data() {
            LayerData::Text(text) => output.push(text),
            LayerData::Group(group) => collect_text(&group.layers, output),
            LayerData::BooleanOperation(boolean) => collect_text(&boolean.layers, output),
            _ => {}
        }
    }
}

fn find_layer_named<'a>(
    layers: &'a [fx_schema::Layer],
    name: &str,
) -> Option<&'a fx_schema::Layer> {
    layers.iter().find_map(|layer| {
        if layer.data().name() == name {
            return Some(layer);
        }
        match layer.data() {
            LayerData::Group(group) => find_layer_named(&group.layers, name),
            LayerData::BooleanOperation(boolean) => find_layer_named(&boolean.layers, name),
            _ => None,
        }
    })
}

fn imported_texts(project: &StructuralProject) -> Vec<TextLayer> {
    let converted = to_structural_fx_document(project, Some(1)).unwrap();
    let mut texts = Vec::new();
    collect_text(converted.document.composition().layers(), &mut texts);
    texts.into_iter().cloned().collect()
}

fn cos_at<'a>(mut value: &'a CosValue, keys: &[&str]) -> &'a CosValue {
    for key in keys {
        value = value.get(key).unwrap();
    }
    value
}

fn generated_cos(bytes: &[u8]) -> Vec<CosValue> {
    let project = read_project(bytes).unwrap();
    project
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Composition(composition) = &item.kind else {
                return None;
            };
            Some(&composition.layers)
        })
        .flatten()
        .filter(|layer| layer.record.layer_type() == 3)
        .map(|layer| cos::parse(source_text_cos(layer)).unwrap())
        .collect()
}

fn generated_document_texts(bytes: &[u8]) -> Vec<String> {
    generated_cos(bytes)
        .iter()
        .flat_map(|value| {
            cos_at(value, &["1", "1"])
                .as_array()
                .unwrap()
                .iter()
                .map(|document| cos_at(document, &["0", "0"]).as_str().unwrap().to_owned())
        })
        .collect()
}

fn generated_font_names(bytes: &[u8]) -> Vec<String> {
    generated_cos(bytes)
        .iter()
        .flat_map(|value| {
            cos_at(value, &["0", "1", "0"])
                .as_array()
                .unwrap()
                .iter()
                .map(|font| cos_at(font, &["0", "0", "0"]).as_str().unwrap().to_owned())
        })
        .collect()
}

fn utf16_literal(value: &str) -> Vec<u8> {
    // These PostScript-name controls contain no literal-string escape bytes.
    // Fresh native envelopes use the native UTF-16 literal form, not legacy hex.
    let mut output = b"(\xFE\xFF".to_vec();
    output.extend(value.encode_utf16().flat_map(u16::to_be_bytes));
    output.push(b')');
    output
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_fresh_point_and_box_cos_text_preserves_whole_document_fields() {
    let document = explicit_document(
        vec![
            text_layer(5_001, "Fresh point", source_text("Point Ω\nSecond", false)),
            text_layer(5_002, "Fresh box", source_text("Box 世界\nSecond", true)),
        ],
        Vec::new(),
    );
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert!(
        layers(&native)
            .iter()
            .all(|layer| { layer.record.layer_type() == 3 && layer.record.source_id() == 0 })
    );

    let texts = imported_texts(&native);
    assert_eq!(texts.len(), 2);
    let point = texts
        .iter()
        .find(|text| text.name == "Fresh point")
        .unwrap();
    assert_eq!(point.source_text.text, "Point Ω\nSecond");
    assert_eq!(point.source_text.font_family.as_ref(), "Inter");
    assert_eq!(point.source_text.font_style.as_ref(), "Regular");
    assert_eq!(point.source_text.font_size.value(), 42.0);
    assert_eq!(point.source_text.fill_color, [0.1, 0.2, 0.3, 0.9]);
    assert_eq!(point.source_text.stroke_color, Some([0.8, 0.7, 0.6, 0.5]));
    assert_eq!(point.source_text.stroke_width.value(), 3.0);
    assert!(point.source_text.stroke_over_fill);
    assert_eq!(point.source_text.tracking, 24.0);
    assert_eq!(point.source_text.leading.unwrap().value(), 54.0);
    assert_eq!(point.source_text.baseline_shift, -2.0);
    assert!(point.source_text.all_caps);
    assert!(!point.source_text.box_text);

    let boxed = texts.iter().find(|text| text.name == "Fresh box").unwrap();
    assert!(boxed.source_text.box_text);
    assert_eq!(boxed.source_text.box_size, Some([420.0, 180.0]));
    assert_eq!(boxed.source_text.box_position, Some([-210.0, -90.0]));
    assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
}

/// Whole-frame Box alignment, not a character-style field or API enum ordinal.
fn box_vertical_alignment_code(value: &CosValue) -> Option<i64> {
    let frames = cos_at(value, &["0", "8", "0"]).as_array().unwrap();
    cos_at(&frames[0], &["0", "2"])
        .get("13")
        .and_then(CosValue::as_i64)
}

#[test]
fn box_vertical_alignment_matches_independent_native_controls_and_input_edits() {
    // Independently authored, saved/reopened native controls pin the COS codes,
    // not the unrelated ExtendScript BoxVerticalAlignment API ordinals.
    let native = generated_cos(include_bytes!(
        "../../../tests/fixtures/box_vertical_alignment/native_controls.aep"
    ));
    assert_eq!(native.len(), 3);
    let native_codes = native
        .iter()
        .map(box_vertical_alignment_code)
        .collect::<Vec<_>>();
    assert_eq!(native_codes, [Some(2), Some(1), None]);

    for (alignment, native_index) in [("center", 1), ("bottom", 0), ("top", 2)] {
        let mut source = source_text("Vertical alignment", true);
        source["fontFamily"] = json!("ArialMT");
        source["fontStyle"] = json!("");
        source["fontSize"] = json!(48.0);
        source["leading"] = json!(52.0);
        source["tracking"] = json!(0.0);
        source["baselineShift"] = json!(0.0);
        source["justification"] = json!("left");
        source["fillColor"] = json!([1.0, 1.0, 1.0, 1.0]);
        source["applyStroke"] = json!(false);
        source["allCaps"] = json!(false);
        source["boxSize"] = json!([360.0, 400.0]);
        source["boxPosition"] = json!([-180.0, -200.0]);
        source["verticalAlign"] = json!(alignment);
        let document = explicit_document(
            vec![text_layer(5_003, "Edited vertical alignment", source)],
            Vec::new(),
        );
        let output = to_aep(&document).unwrap();
        let generated = generated_cos(&output.bytes);
        assert_eq!(generated.len(), 1, "{:?}", output.diagnostics);
        assert_eq!(
            box_vertical_alignment_code(&generated[0]),
            native_codes[native_index],
            "authored {alignment} must survive as the independently verified native Box control"
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Box vertical alignment")),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn point_text_vertical_alignment_remains_inapplicable_and_diagnosed() {
    let mut source = source_text("Point control", false);
    source["verticalAlign"] = json!("center");
    let document = explicit_document(vec![text_layer(5_004, "Point control", source)], Vec::new());
    let output = to_aep(&document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("not applicable to Point Text") })
    );
    let generated = generated_cos(&output.bytes);
    assert_eq!(generated.len(), 1);
    assert_eq!(box_vertical_alignment_code(&generated[0]), None);
}

/// Independently AE-authored All Caps sources store caps code 2 in field 12 of
/// the character style and normal text stores 0. The fresh export must write
/// those native bytes, not merely values our own reader accepts.
#[test]
fn fresh_all_caps_text_exports_the_native_caps_code_and_reimports_all_caps() {
    let mut normal = source_text("Normal case", false);
    normal["allCaps"] = json!(false);
    let document = explicit_document(
        vec![
            text_layer(5_011, "Fresh all caps", source_text("Mixed Case", false)),
            text_layer(5_012, "Fresh normal caps", normal),
        ],
        Vec::new(),
    );
    let output = to_aep(&document).unwrap();
    let payloads = generated_cos(&output.bytes);
    assert_eq!(payloads.len(), 2);
    let caps = payloads
        .iter()
        .map(|value| {
            let document = cos_at(value, &["1", "1"]).index(0).unwrap();
            let text = cos_at(document, &["0", "0"]).as_str().unwrap();
            let run = cos_at(document, &["0", "6", "0"]).index(0).unwrap();
            let code = cos_at(run, &["0", "0", "6", "12"]).as_i64().unwrap();
            (text, code)
        })
        .collect::<Vec<_>>();
    assert!(caps.contains(&("Mixed Case\r", 2)));
    assert!(caps.contains(&("Normal case\r", 0)));

    let texts = imported_texts(&read_project(&output.bytes).unwrap());
    let all_caps = |text: &str| {
        texts
            .iter()
            .find(|layer| layer.source_text.text == text)
            .unwrap_or_else(|| panic!("{text:?} was not reimported as editable Text"))
            .source_text
            .all_caps
    };
    assert!(all_caps("Mixed Case"));
    assert!(!all_caps("Normal case"));
}

/// The opaque COS payload of `layer`'s Source Text.
fn source_text_cos(layer: &crate::structure::Layer) -> &[u8] {
    fn find(chunks: &[crate::rifx::Chunk]) -> Option<&[u8]> {
        chunks.iter().find_map(|chunk| {
            if chunk.list_kind() == Some(*b"btdk") {
                chunk.opaque_payload()
            } else {
                chunk.children().and_then(find)
            }
        })
    }
    find(&layer.content).expect("Source Text COS payload")
}

/// Every font-set entry of the 76 public AE-authored Text fixtures is
/// `<< /0 << /99 /CoolTypeFont /0 << /0 (name) /2 n [/5 (version)] >> >> >>`,
/// and the reader takes the name from that depth. The former shallow
/// `<< /0 << /0 name >> >>` entry lost the font on reimport (`sans`/`serif`).
#[test]
fn fresh_font_entry_uses_the_native_cool_type_dictionary_and_reimports_its_name() {
    let bytes = include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/text_document_font_style.aep"
    );
    assert_eq!(bytes.len(), 94_035);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "8ef0ba48c93f95bf4a50fdb1c73b4e3df172856b88535aa7909a084dd2dd1cba"
    );
    let source = read_project(bytes).unwrap();
    let [text] = layers(&source)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .collect::<Vec<_>>()[..]
    else {
        panic!("one native Text layer");
    };
    let mut native_entry = b"<< /0 << /99 /CoolTypeFont /0 << /0 (\xFE\xFF".to_vec();
    native_entry.extend("Arial-BoldMT".encode_utf16().flat_map(u16::to_be_bytes));
    native_entry.extend(b") /2 1 /5 (");
    assert_eq!(occurrences(source_text_cos(text), &native_entry), 1);

    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    let output = to_aep(&imported.document).unwrap();
    let payloads = generated_cos(&output.bytes);
    assert_eq!(payloads.len(), 1);
    let first_font = cos_at(&payloads[0], &["0", "1", "0"]).index(0).unwrap();
    assert_eq!(
        cos_at(first_font, &["0", "99"]).as_str(),
        Some("CoolTypeFont")
    );
    let identity = cos_at(first_font, &["0", "0"]);
    assert_eq!(identity.get("0").unwrap().as_str(), Some("Arial-BoldMT"));
    // The FX document holds no face format or version; never guess those fields.
    assert!(identity.get("2").is_none() && identity.get("5").is_none());

    let texts = imported_texts(&read_project(&output.bytes).unwrap());
    let fonts = texts
        .iter()
        .map(|text| {
            (
                text.source_text.font_family.as_ref(),
                text.source_text.font_style.as_ref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(fonts, [("Arial", "BoldMT")]);
}

#[test]
fn basic_text_bold_postscript_identity_exports_without_a_second_style_suffix() {
    let bytes = include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/text_document_font_style.aep"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "8ef0ba48c93f95bf4a50fdb1c73b4e3df172856b88535aa7909a084dd2dd1cba"
    );
    let native = read_project(bytes).unwrap();
    let native_text = layers(&native)
        .iter()
        .find(|layer| layer.record.layer_type() == 3)
        .unwrap();
    let mut native_name = b"(\xFE\xFF".to_vec();
    native_name.extend("Arial-BoldMT".encode_utf16().flat_map(u16::to_be_bytes));
    native_name.push(b')');
    assert_eq!(occurrences(source_text_cos(native_text), &native_name), 1);

    // Basic Text's authoritative packed face uses whole PostScript + empty
    // style, unlike ordinary Source Text's family / style split.
    let mut source = source_text("Edited legacy Basic Text", false);
    source["fontFamily"] = json!("Arial-BoldMT");
    source["fontStyle"] = json!("");
    let document = explicit_document(vec![text_layer(1, "Basic Text face", source)], Vec::new());
    let output = to_aep(&document).unwrap();
    assert_eq!(
        occurrences(&output.bytes, &utf16_literal("Arial-BoldMT")),
        1
    );
    assert_eq!(
        occurrences(&output.bytes, &utf16_literal("Arial-BoldMT-Bold")),
        0
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("deterministic candidate") }),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn whole_postscript_style_edits_retain_unverified_candidate_diagnostics() {
    for (family, style, expected) in [
        ("Arial-BoldMT", "Bold", "Arial-BoldMT-Bold"),
        ("Arial-BoldMT", "Italic", "Arial-BoldMT-Italic"),
        ("Example-BoldMT", "Bold", "Example-BoldMT-Bold"),
    ] {
        let mut source = source_text("Unverified font pair", false);
        source["fontFamily"] = json!(family);
        source["fontStyle"] = json!(style);
        let document = explicit_document(vec![text_layer(1, "Font candidate", source)], Vec::new());
        let output = to_aep(&document).unwrap();
        assert_eq!(occurrences(&output.bytes, &utf16_literal(expected)), 1);
        assert!(
            output.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .message
                    .contains("host font resolution remains unverified")
                    && diagnostic
                        .message
                        .contains(&format!("authored deterministic candidate {expected:?}"))
            }),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn whole_postscript_empty_style_exports_edited_static_and_keyed_faces() {
    for face in ["ExamplePS", "Example-BoldMT", "Neighbor-Semibold"] {
        let mut source = source_text("Edited face", false);
        source["fontFamily"] = json!(face);
        source["fontStyle"] = json!("");
        let document = explicit_document(
            vec![text_layer(1, "Edited face", source.clone())],
            Vec::new(),
        );
        let output = to_aep(&document).unwrap();
        assert_eq!(occurrences(&output.bytes, &utf16_literal(face)), 1);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("deterministic candidate"))
        );
        let entries = vec![layer_entry(
            LayerId::new(1),
            PropType::FontFamily,
            [
                (0, PropertyValue::String(face.into())),
                (500, PropertyValue::String("EditedPS".into())),
            ],
            PropertyKeyframeEasing::Hold,
        )];
        let keyed = explicit_document(vec![text_layer(1, "Keyed face", source)], entries);
        let output = to_aep(&keyed).unwrap();
        assert!(occurrences(&output.bytes, &utf16_literal(face)) > 0);
        assert!(occurrences(&output.bytes, &utf16_literal("EditedPS")) > 0);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("deterministic candidate"))
        );
    }
}

#[test]
fn native_font_source_stages_byte_backed_archive_identity_and_format() {
    use crate::adapter::{AfterEffects, AfterEffectsExportOptions};
    use tesseract_file::TesseractFileBuilder;
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/text_document_font_style.aep"
    ))
    .unwrap();
    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    let mut edited = imported.document.to_json_value().unwrap();
    fn use_physical_style(value: &mut Value) {
        match value {
            Value::Object(object) => {
                if object.contains_key("fontFamily") {
                    object.insert("fontStyle".into(), json!("Bold"));
                }
                for child in object.values_mut() {
                    use_physical_style(child);
                }
            }
            Value::Array(array) => {
                for child in array {
                    use_physical_style(child);
                }
            }
            _ => {}
        }
    }
    use_physical_style(&mut edited);
    let document = EditableFxCompositionDocument::from_json_value(edited).unwrap();
    let properties = serde_json::from_value(json!({"faces": [{
        "postscriptName": "Arial-BoldMT", "familyName": "Arial", "styleName": "Bold",
        "fullName": "Arial Bold", "weight": 700, "width": 5,
        "selectionNames": ["Arial/Bold"]
    }]}))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/references/premiere/fonts/Arial-BoldMT.ttf");
    let archive = TesseractFileBuilder::try_new(document)
        .unwrap()
        .add_font_asset("font", font, properties)
        .unwrap()
        .write(directory.path().join("input.tsrct"))
        .unwrap();
    let stage = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            directory.path(),
            &AfterEffectsExportOptions::default(),
        )
        .unwrap();
    let bytes = std::fs::read(stage.directory().join("project.aep")).unwrap();
    let payloads = generated_cos(&bytes);
    assert_eq!(payloads.len(), 1);
    let font = cos_at(&payloads[0], &["0", "1", "0"]).index(0).unwrap();
    let identity = cos_at(font, &["0", "0"]);
    assert_eq!(identity.get("0").unwrap().as_str(), Some("Arial-BoldMT"));
    assert_eq!(identity.get("2").unwrap().as_i64(), Some(1));
    assert!(identity.get("5").is_none());
    assert_eq!(imported_texts(&read_project(&bytes).unwrap()).len(), 1);
}

fn font_identities(texts: &[TextLayer]) -> Vec<(&str, &str)> {
    texts
        .iter()
        .map(|text| {
            (
                text.source_text.font_family.as_ref(),
                text.source_text.font_style.as_ref(),
            )
        })
        .collect()
}

/// A dash-less PostScript name has no style to split off. Import keeps it whole
/// with an empty style, so export writes the exact name instead of the guessed
/// `ArialMT-Regular`, and a reimport returns the same identity.
#[test]
fn dashless_postscript_font_round_trips_exactly_with_an_empty_style() {
    let bytes =
        include_bytes!("../../../tests/fixtures/pr4442_native/sources/text_document_point.aep");
    assert_eq!(bytes.len(), 93_997);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "bc32cae3896c3aaf2e7a02f5283ce9ef31c9b0ef53f8c8a30a2b6c30b69eea34"
    );
    let source = read_project(bytes).unwrap();
    let [text] = layers(&source)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .collect::<Vec<_>>()[..]
    else {
        panic!("one native Text layer");
    };
    let mut native_entry = b"<< /0 << /99 /CoolTypeFont /0 << /0 (\xFE\xFF".to_vec();
    native_entry.extend("ArialMT".encode_utf16().flat_map(u16::to_be_bytes));
    native_entry.extend(b") /2 1 /5 (");
    assert_eq!(occurrences(source_text_cos(text), &native_entry), 1);
    assert_eq!(font_identities(&imported_texts(&source)), [("ArialMT", "")]);

    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    let output = to_aep(&imported.document).unwrap();
    let fonts = generated_font_names(&output.bytes);
    assert_eq!(fonts[0], "ArialMT");
    assert!(!fonts.iter().any(|font| font == "ArialMT-Regular"));
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("deterministic candidate")),
        "{:?}",
        output.diagnostics
    );
    let reimported = imported_texts(&read_project(&output.bytes).unwrap());
    assert_eq!(font_identities(&reimported), [("ArialMT", "")]);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_hold_source_text_uses_signed_authored_time_union_and_deduplicated_fonts() {
    let id = LayerId::new(5_100);
    let layer = text_layer(
        id.value(),
        "Fresh Hold documents",
        source_text("base", false),
    );
    let entries = vec![
        layer_entry(
            id,
            PropType::TextContent,
            [
                (-500, PropertyValue::String("before".into())),
                (500, PropertyValue::String("after".into())),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        layer_entry(
            id,
            PropType::FontFamily,
            [
                (-250, PropertyValue::String("Inter".into())),
                (750, PropertyValue::String("Roboto".into())),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        layer_entry(
            id,
            PropType::FontStyle,
            [
                (-250, PropertyValue::String("Regular".into())),
                (750, PropertyValue::String("Bold".into())),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        layer_entry(
            id,
            PropType::FontSize,
            [
                (0, PropertyValue::Float(42.0)),
                (1_000, PropertyValue::Float(64.0)),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        constant_layer_entry(id, PropType::Tracking, PropertyValue::Float(18.0)),
        disabled_layer_entry(
            id,
            PropType::Leading,
            [
                (0, PropertyValue::Float(60.0)),
                (500, PropertyValue::Float(90.0)),
            ],
            PropertyValue::Float(54.0),
        ),
    ];
    let document = explicit_document(vec![layer], entries.clone());
    let LayerData::Text(text_layer) = document.composition().layers()[0].data() else {
        panic!("fresh Text input")
    };
    let lowered = text::lower(
        text_layer,
        &text_layer.transform,
        text_layer.id,
        &crate::export_document::AnimationIndex::new(&entries),
        None,
    )
    .unwrap();
    assert_eq!(
        lowered
            .spec
            .documents
            .keys
            .iter()
            .map(|key| key.time_millis)
            .collect::<Vec<_>>(),
        [-500, -250, 0, 500, 750, 1_000]
    );
    assert!(lowered.spec.documents.keyed);
    assert_eq!(lowered.spec.documents.keys[0].document.text, "before");
    assert_eq!(
        lowered.spec.documents.keys.last().unwrap().document.text,
        "after"
    );
    assert_eq!(lowered.spec.documents.keys[0].document.leading, Some(54.0));
    assert!(
        lowered
            .diagnostics
            .iter()
            .any(|message| message.contains("Disabled Leading"))
    );
    let fonts = lowered
        .spec
        .documents
        .keys
        .iter()
        .map(|key| key.document.font_postscript.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(fonts, BTreeSet::from(["Inter-Regular", "Roboto-Bold"]));

    let output = to_aep(&document).unwrap();
    let fonts = generated_font_names(&output.bytes);
    assert_eq!(
        fonts.iter().filter(|font| *font == "Inter-Regular").count(),
        1
    );
    assert_eq!(
        fonts.iter().filter(|font| *font == "Roboto-Bold").count(),
        1
    );
    assert!(
        layers(&read_project(&output.bytes).unwrap())
            .iter()
            .all(|layer| { layer.record.layer_type() == 3 && layer.record.source_id() == 0 })
    );
}

fn all_channel_entries() -> Vec<AnimationGraphEntry> {
    let animator = FxItemId::new(5_201);
    let range = FxItemId::new(5_202);
    let wiggly = FxItemId::new(5_203);
    let more = FxItemId::new(5_204);
    let path = FxItemId::new(5_205);
    let mut entries = Vec::new();
    for (property, first, second) in [
        (
            "anchorPoint",
            PropertyValue::Vector2([1.0, 2.0]),
            PropertyValue::Vector2([3.0, 4.0]),
        ),
        (
            "position",
            PropertyValue::Vector2([5.0, 6.0]),
            PropertyValue::Vector2([7.0, 8.0]),
        ),
        (
            "scale",
            PropertyValue::Vector2([100.0, 100.0]),
            PropertyValue::Vector2([120.0, 80.0]),
        ),
        (
            "blur",
            PropertyValue::Vector2([0.0, 0.0]),
            PropertyValue::Vector2([4.0, 6.0]),
        ),
        (
            "fillColor",
            PropertyValue::Color([0.1, 0.2, 0.3, 1.0]),
            PropertyValue::Color([0.3, 0.4, 0.5, 1.0]),
        ),
        (
            "strokeColor",
            PropertyValue::Color([0.6, 0.5, 0.4, 1.0]),
            PropertyValue::Color([0.4, 0.3, 0.2, 1.0]),
        ),
    ] {
        entries.push(item_entry(
            animator,
            property,
            [(-250, first), (750, second)],
        ));
    }
    for (property, first, second) in [
        ("rotation", 0.0, 25.0),
        ("skew", 0.0, 12.0),
        ("skewAxis", 0.0, 45.0),
        ("tracking", 0.0, 30.0),
        ("strokeWidth", 1.0, 5.0),
        ("opacity", 100.0, 40.0),
        ("lineSpacing", 0.0, 20.0),
        ("lineAnchor", 0.0, 50.0),
        ("characterOffset", 0.0, 2.0),
        ("characterValue", 65.0, 90.0),
    ] {
        entries.push(item_entry(
            animator,
            property,
            [
                (-250, PropertyValue::Float(first)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    for (property, first, second) in [
        ("start", 0.1, 0.2),
        ("end", 0.9, 0.8),
        ("offset", 0.0, 0.25),
        ("amount", 1.0, 0.6),
        ("easeHigh", 0.0, 0.4),
        ("easeLow", 0.0, -0.3),
        ("randomSeed", 3.0, 7.0),
    ] {
        entries.push(item_entry(
            range,
            property,
            [
                (-250, PropertyValue::Float(first)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    for (property, first, second) in [
        ("speed", 2.0, 4.0),
        ("amount", 100.0, 55.0),
        ("seed", 1.0, 9.0),
    ] {
        entries.push(item_entry(
            wiggly,
            property,
            [
                (-250, PropertyValue::Float(first)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    entries.push(item_entry(
        more,
        "groupingAlignment",
        [
            (-250, PropertyValue::Vector2([0.0, 0.0])),
            (750, PropertyValue::Vector2([40.0, -30.0])),
        ],
    ));
    for (property, second) in [("firstMargin", 120.0), ("lastMargin", 80.0)] {
        entries.push(item_entry(
            path,
            property,
            [
                (-250, PropertyValue::Float(0.0)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    entries
}

fn all_channel_text() -> Value {
    let mut text = text_layer(
        5_200,
        "Fresh all Text controls",
        source_text("Editable", false),
    );
    text["animators"] = json!([{
        "id": 5201,
        "name": "All sixteen",
        "selectors": [{
            "id": 5202, "start": 0.1, "end": 0.9, "offset": 0.0,
            "units": "percentage", "basedOn": "words", "mode": "intersect",
            "amount": 1.0, "shape": "triangle", "easeHigh": 0.0,
            "easeLow": 0.0, "randomizeOrder": true, "randomSeed": 3.0
        }],
        "wigglySelectors": [{
            "id": 5203, "mode": "subtract", "speed": 2.0,
            "amount": 100.0, "seed": 1.0
        }],
        "anchorPoint": [1.0, 2.0], "position": [5.0, 6.0],
        "scale": [100.0, 100.0], "rotation": 0.0, "skew": 0.0,
        "skewAxis": 0.0, "tracking": 0.0, "strokeWidth": 1.0,
        "blur": [0.0, 0.0], "opacity": 100.0,
        "fillColor": [0.1, 0.2, 0.3, 1.0],
        "strokeColor": [0.6, 0.5, 0.4, 1.0],
        "lineSpacing": 0.0, "lineAnchor": 0.0,
        "characterOffset": 0.0, "characterValue": 65.0
    }]);
    text["anchorOptions"] = json!({
        "id": 5204, "anchorPointGrouping": "word",
        "groupingAlignment": [0.0, 0.0]
    });
    text["pathOptions"] = json!({
        "id": 5205, "pathLayer": 5299, "firstMargin": 0.0,
        "lastMargin": 0.0, "perpendicularToPath": true,
        "reversePath": true, "forceAlignment": true
    });
    text
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_all_animator_selector_more_and_path_channels_export_signed_native_keys() {
    let entries = all_channel_entries();
    assert_eq!(entries.len(), 29);
    let expected_entries = entries.clone();
    let document = explicit_document(vec![guide_layer(5_299), all_channel_text()], entries);
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native).len(),
        1,
        "guide is consumed into Text mask: {:?}",
        output.diagnostics
    );
    assert_eq!(layers(&native)[0].record.layer_type(), 3);
    assert_eq!(layers(&native)[0].record.source_id(), 0);

    let converted = to_structural_fx_document(&native, Some(1)).unwrap();
    let mut texts = Vec::new();
    collect_text(converted.document.composition().layers(), &mut texts);
    let [text] = texts.as_slice() else {
        panic!("one fresh Text layer")
    };
    let animator = &text.animators[0];
    assert_eq!(text.animators.len(), 1);
    assert_eq!(animator.selectors.len(), 1);
    assert_eq!(animator.wiggly_selectors.len(), 1);
    // Fresh native import remints identities; compare controls independently of IDs.
    let mut animator_json = serde_json::to_value(animator).unwrap();
    animator_json["id"] = json!(5201);
    // Check the authored name separately, after all channel assertions run.
    animator_json["name"] = json!("All sixteen");
    animator_json["selectors"][0]["id"] = json!(5202);
    animator_json["wigglySelectors"][0]["id"] = json!(5203);
    assert_eq!(
        animator_json,
        json!({
            "id": 5201,
            "name": "All sixteen",
            "selectors": [{
                "id": 5202, "start": 0.1, "end": 0.9, "offset": 0.0,
                "units": "percentage", "basedOn": "words", "mode": "intersect",
                "amount": 1.0, "shape": "triangle", "easeHigh": 0.0,
                "easeLow": 0.0, "randomizeOrder": true, "randomSeed": 3.0
            }],
            "wigglySelectors": [{
                "id": 5203, "mode": "subtract", "speed": 2.0,
                "amount": 100.0, "seed": 1.0
            }],
            "anchorPoint": [1.0, 2.0], "position": [5.0, 6.0],
            "scale": [100.0, 100.0], "rotation": 0.0, "skew": 0.0,
            "skewAxis": 0.0, "tracking": 0.0, "strokeWidth": 1.0,
            "blur": [0.0, 0.0], "opacity": 100.0,
            "fillColor": [0.1, 0.2, 0.3, 1.0],
            "strokeColor": [0.6, 0.5, 0.4, 1.0],
            "lineSpacing": 0.0, "lineAnchor": 0.0,
            "characterOffset": 0.0, "characterValue": 65.0
        })
    );
    let more = text.anchor_options.as_ref().unwrap();
    let mut more_json = serde_json::to_value(more).unwrap();
    more_json["id"] = json!(5204);
    assert_eq!(
        more_json,
        json!({
            "id": 5204,
            "anchorPointGrouping": "word",
            "groupingAlignment": [0.0, 0.0]
        })
    );
    let path = text.path_options.as_ref().unwrap();
    let guide = find_layer_named(
        converted.document.composition().layers(),
        "Fresh all Text controls — Mask 1 guide",
    )
    .expect("fresh native import retains the named text path guide");
    assert_eq!(path.path_layer, guide.id());
    let mut path_json = serde_json::to_value(path).unwrap();
    path_json["id"] = json!(5205);
    path_json["pathLayer"] = json!(5299);
    assert_eq!(
        path_json,
        json!({
            "id": 5205, "pathLayer": 5299, "firstMargin": 0.0,
            "lastMargin": 0.0, "perpendicularToPath": true,
            "reversePath": true, "forceAlignment": true
        })
    );

    let imported_ids = [
        (FxItemId::new(5201), animator.id),
        (FxItemId::new(5202), animator.selectors[0].id),
        (FxItemId::new(5203), animator.wiggly_selectors[0].id),
        (FxItemId::new(5204), more.id),
        (FxItemId::new(5205), path.id),
    ];
    let graph = converted.document.composition().dynamics().entries();
    assert_eq!(graph.len(), expected_entries.len());
    for expected in &expected_entries {
        let PropertyTarget::FxItemProperty(expected_target) = &expected.target else {
            panic!("all-channel fixture uses item-property tracks")
        };
        let (_, imported_id) = imported_ids
            .iter()
            .find(|(authored, _)| *authored == expected_target.item_id())
            .expect("known authored item");
        let matches = graph
            .iter()
            .filter(|entry| {
                matches!(
                    &entry.target,
                    PropertyTarget::FxItemProperty(target)
                        if target.item_id() == *imported_id
                            && target.property_name() == expected_target.property_name()
                )
            })
            .collect::<Vec<_>>();
        let [actual] = matches.as_slice() else {
            panic!(
                "expected one fresh native Text track for item {} property {}, got {}",
                expected_target.item_id(),
                expected_target.property_name(),
                matches.len()
            )
        };
        let expected_keys = expected.animator.keyframe_track().unwrap().keyframes();
        let actual_keys = actual.animator.keyframe_track().unwrap().keyframes();
        assert_eq!(actual_keys.len(), expected_keys.len());
        for (actual, expected) in actual_keys.iter().zip(expected_keys) {
            assert_eq!(actual.layer_time(), expected.layer_time());
            assert_eq!(actual.value(), expected.value());
            assert_eq!(actual.easing(), expected.easing());
        }
    }
    assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
    assert_eq!(animator.name, "All sixteen", "authored Animator name");
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_unsupported_text_layout_is_diagnosed_while_supported_sibling_exports() {
    let mut unsupported = text_layer(5_400, "Approximated text", source_text("kept", true));
    unsupported["sourceText"]["fontVariations"] = json!({"id": 5401, "axes": {"wght": 700.0}});
    unsupported["sourceText"]["underline"] = json!(true);
    unsupported["sourceText"]["strikethrough"] = json!(true);
    unsupported["sourceText"]["verticalAlign"] = json!("center");
    unsupported["sourceText"]["scaleBoxTextWithTransform"] = json!(true);
    let sibling = text_layer(
        5_402,
        "Supported text sibling",
        source_text("sibling", false),
    );
    let entries = vec![layer_entry(
        LayerId::new(5_400),
        PropType::TextContent,
        [
            (0, PropertyValue::String("first".into())),
            (500, PropertyValue::String("continuous".into())),
        ],
        PropertyKeyframeEasing::Linear,
    )];
    let document = explicit_document(vec![unsupported, sibling], entries);
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let messages = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    for expected in [
        "Variable-font axes",
        "Underline/strikethrough",
        "scaleBoxTextWithTransform",
        "continuous interpolation",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{expected}: {messages:?}"
        );
    }
    let names = layers(&native)
        .iter()
        .map(|layer| layer.name.as_ref())
        .collect::<HashSet<_>>();
    assert!(names.contains("Approximated text"));
    assert!(names.contains("Supported text sibling"));
    let boxed = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Approximated text")
        .unwrap();
    let boxed = cos::parse(source_text_cos(boxed)).unwrap();
    assert_eq!(box_vertical_alignment_code(&boxed), Some(1));
    assert!(
        !messages
            .iter()
            .any(|message| message.contains("Box vertical alignment"))
    );
    assert!(
        layers(&native)
            .iter()
            .all(|layer| layer.record.source_id() == 0)
    );
    // Multi-style character/paragraph runs have no explicit FX input shape;
    // unlike the diagnosed fields above, they remain a missing export oracle
    // rather than being silently claimed by this structural case.
}

fn zero_transform() -> Value {
    let mut transform = identity_transform();
    transform["position"] = json!([0.0, 0.0]);
    transform
}

fn identity_playback(span: u64) -> Value {
    affine_playback([0, span], [0, span])
}

fn affine_playback(
    [input_start, input_end]: [u64; 2],
    [output_start, output_end]: [u64; 2],
) -> Value {
    let input = json!({"start": input_start, "duration": input_end - input_start});
    json!({
        "type": "windowed",
        "inputRange": input,
        "mapping": {
            "type": "linear",
            "input": input,
            "output": {"start": output_start, "duration": output_end - output_start}
        },
        "inputOffsetMs": 0
    })
}

fn linear_playback(keys: &[(u64, u64)]) -> Value {
    let start = keys.first().unwrap().0;
    let end = keys.last().unwrap().0;
    json!({
        "type": "windowed",
        "inputRange": {"start": start, "duration": end - start},
        "mapping": {"type": "timeRemap", "property": {
            "before": "inactive",
            "after": "inactive",
            "keyframes": keys
                .iter()
                .enumerate()
                .map(|(index, (time, value))| json!({
                    "id": format!("held-text-clock-{index}"),
                    "time": time,
                    "value": value,
                    "easing": {"type": "linear"}
                }))
                .collect::<Vec<_>>()
        }},
        "inputOffsetMs": 0
    })
}

/// The importer's shape for one AE text layer with held Source Text: a layer
/// Group (`id`) holding a "Source content clock" Group (`id + 1`) with
/// `playback`, holding one Text per held value.
fn held_text_layer(id: u64, name: &str, playback: Value, segments: &[(&str, u64, u64)]) -> Value {
    let clock = id + 1;
    let texts = segments
        .iter()
        .zip(clock + 1..)
        .map(|((text, start, duration), text_id)| {
            let mut layer =
                text_layer(text_id, &format!("{name} {text}"), source_text(text, false));
            layer["parent"] = json!(clock);
            layer["activeRange"] = json!({"start": start, "duration": duration});
            layer["transform"] = zero_transform();
            layer
        })
        .collect::<Vec<_>>();
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "playback": identity_playback(2000),
        "transform": identity_transform(),
        "layers": [{
            "type": "Group",
            "id": clock,
            "name": "Source content clock",
            "parent": id,
            "transform": zero_transform(),
            "playback": playback,
            "layers": texts
        }]
    })
}

fn two_second_document(layers: Vec<Value>) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["duration"] = json!(2.0);
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": []});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn native_named<'a>(project: &'a StructuralProject, name: &str) -> &'a crate::structure::Layer {
    layers(project)
        .iter()
        .find(|layer| layer.name.as_ref() == name)
        .unwrap_or_else(|| panic!("native layer {name:?}"))
}

/// Comp-time `(start, end)` of one native AV layer, from its exact record
/// fields: `start + point * stretch`.
fn comp_interval(layer: &crate::structure::Layer) -> (f64, f64) {
    let record = &layer.record;
    let start = record.start_time().unwrap();
    let stretch = record.stretch().unwrap();
    (
        start + record.in_point().unwrap() * stretch,
        start + record.out_point().unwrap() * stretch,
    )
}

/// An identity-clock AE text layer holds several held segments. Its single
/// child is a text-only branch, so Null parenting keeps each segment as an
/// editable native Text layer instead of omitting the subtree for want of
/// glyph bounds.
#[test]
fn identity_wrapper_of_held_text_segments_exports_each_segment_under_null_parents() {
    let document = two_second_document(vec![held_text_layer(
        5_600,
        "Held",
        linear_playback(&[(0, 0), (2_000, 2_000)]),
        &[("0%", 0, 1_500), ("100%", 1_500, 999_999_998_500)],
    )]);
    let output = to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let texts = layers(&native)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .map(|layer| (layer.name.as_ref(), comp_interval(layer)))
        .collect::<Vec<_>>();
    assert_eq!(texts, [("Held 0%", (0.0, 1.5)), ("Held 100%", (1.5, 2.0))]);
    for text in ["0%\r", "100%\r"] {
        assert_eq!(
            generated_document_texts(&output.bytes)
                .iter()
                .filter(|value| *value == text)
                .count(),
            1,
            "{text:?}"
        );
    }
    let clock = native_named(&native, "Source content clock");
    let wrapper = native_named(&native, "Held");
    assert!(clock.record.flags().null_layer && wrapper.record.flags().null_layer);
    assert_eq!(clock.record.parent_id(), wrapper.record.id());
    for text in ["Held 0%", "Held 100%"] {
        assert_eq!(
            native_named(&native, text).record.parent_id(),
            clock.record.id()
        );
    }
}

/// A Null cannot pass its blend/opacity/motion controls to its children.
/// Root output clipping now admits an editable text precomposition for
/// pointwise blend/opacity; motion blur still has no finite crop certificate.
#[test]
fn text_only_wrapper_keeps_the_null_parent_guards() {
    for (field, value) in [
        ("blendMode", json!("multiply")),
        ("motionBlur", json!(true)),
        ("opacity", json!(50.0)),
    ] {
        let mut wrapper = held_text_layer(
            5_700,
            "Guarded",
            linear_playback(&[(0, 0), (2_000, 2_000)]),
            &[("0%", 0, 1_500), ("100%", 1_500, 999_999_998_500)],
        );
        if field == "opacity" {
            wrapper["transform"]["opacity"] = value;
        } else {
            wrapper[field] = value;
        }
        let output = to_aep(&two_second_document(vec![wrapper])).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let text_count = native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(comp.layers.as_slice()),
                _ => None,
            })
            .flatten()
            .filter(|layer| layer.record.layer_type() == 3)
            .count();
        if field == "motionBlur" {
            assert!(output.omitted_layer_ids.contains(&LayerId::new(5_700)));
            assert_eq!(text_count, 0, "{field}");
        } else {
            assert!(
                !output.omitted_layer_ids.contains(&LayerId::new(5_700)),
                "{field}"
            );
            assert_eq!(text_count, 2, "{field}");
        }
    }
}

/// One occurrence of a nested composition that holds only a held-text layer,
/// in the importer's shape: a full-span Group (`id`) holding its source-clock
/// Group (`id + 1`), whose two linear keys on its active interval map the
/// parent to the 2 s source. The text layer takes `id + 10` onwards.
fn text_occurrence(id: u64, name: &str, clock: [(u64, u64); 2]) -> Value {
    let mut text = held_text_layer(
        id + 10,
        &format!("{name} text"),
        linear_playback(&[(0, 0), (2_000, 2_000)]),
        &[("0%", 0, 1_500), ("100%", 1_500, 999_999_998_500)],
    );
    text["parent"] = json!(id + 1);
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "playback": identity_playback(2000),
        "transform": identity_transform(),
        "layers": [{
            "type": "Group",
            "id": id + 1,
            "name": format!("{name} clock"),
            "parent": id,
            "transform": zero_transform(),
            "playback": linear_playback(&clock),
            "layers": [text]
        }]
    })
}

fn composition_layers(project: &StructuralProject, id: u32) -> &[crate::structure::Layer] {
    let ItemKind::Composition(composition) = &project.item(id).unwrap().kind else {
        panic!("composition {id}")
    };
    &composition.layers
}

/// Text must precompose under an offset or negative-start source clock, but
/// has no glyph bounds to size a source canvas. The precomposition uses native
/// collapse transformations on the root canvas; both occurrences keep their
/// exact clocks and every held segment. Adobe rendering is unverified.
#[test]
fn clocked_text_occurrences_export_as_collapsed_precompositions_with_exact_clocks() {
    let document = two_second_document(vec![
        text_occurrence(5_800, "Late", [(500, 0), (2_000, 1_500)]),
        text_occurrence(5_900, "Early", [(0, 500), (1_500, 2_000)]),
    ]);
    let dimensions = document.to_json_value().unwrap()["dimensions"].clone();
    let output = to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    for (name, id, clock) in [
        ("Late", 5_801, [(1, 2), (0, 1), (3, 2), (1, 1)]),
        ("Early", 5_901, [(-1, 2), (1, 2), (2, 1), (1, 1)]),
    ] {
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(id))
                && diagnostic.message.contains("no FX glyph bounds")
        }));
        let occurrence = native_named(&native, &format!("{name} clock"));
        assert_eq!(
            occurrence.record.parent_id(),
            native_named(&native, name).record.id()
        );
        assert!(occurrence.record.flags().collapse_transformation, "{name}");
        let record = &occurrence.record;
        assert_eq!(
            [
                record.start_time_fraction(),
                record.in_point_fraction(),
                record.out_point_fraction(),
                record.stretch_fraction(),
            ],
            clock,
            "{name}"
        );
        let ItemKind::Composition(source) = &native.item(record.source_id()).unwrap().kind else {
            panic!("{name}: occurrence source");
        };
        assert_eq!(
            json!({"width": source.width, "height": source.height}),
            dimensions,
            "{name}: nominal root canvas"
        );
        let held = composition_layers(&native, record.source_id())
            .iter()
            .filter(|layer| layer.record.layer_type() == 3)
            .map(|layer| (layer.name.as_ref().to_owned(), comp_interval(layer)))
            .collect::<Vec<_>>();
        assert_eq!(
            held,
            [
                (format!("{name} text 0%"), (0.0, 1.5)),
                (format!("{name} text 100%"), (1.5, 2.0)),
            ]
        );
    }
}

/// A clocked root scene with an ordinary HUD Text and vector siblings must
/// retain the editable vector scene when the Text has no proven bounds. The
/// Text omission is explicit; native mixed-text admission remains unverified.
#[test]
fn clocked_root_hud_text_keeps_its_vector_scene() {
    let value = imported();
    let mut text = text_layer(6_002, "HUD label", source_text("1080P / 30 FPS", true));
    text["parent"] = json!(6_001);
    text["activeRange"] = json!({"start": 0, "duration": 1_500});
    text["transform"] = zero_transform();
    let mut hud = json!({
        "type": "Group", "id": 6_001, "name": "HUD", "parent": 6_000,
        "playback": identity_playback(1_500), "transform": zero_transform(),
        "layers": [text]
    });
    hud["isHidden"] = json!(false);
    let mut vector = rect(&value, 6_003);
    vector["name"] = json!("Scene vector");
    vector["parent"] = json!(6_000);
    vector["playback"] = identity_playback(1_500);
    vector["activeRange"] = json!({"start": 0, "duration": 1_500});
    let scene = json!({
        "type": "Group", "id": 6_000, "name": "Clocked scene", "parent": null,
        "playback": linear_playback(&[(500, 0), (2_000, 1_500)]),
        "transform": zero_transform(), "layers": [hud, vector]
    });
    // The root-output viewport certificate excludes both translated and
    // scaled owner occurrences, even when their direct Rect paints locally.
    for (name, position, scale) in [
        ("translated", [10_000.0, 10_000.0], [100.0, 100.0]),
        ("scaled", [0.0, 0.0], [150.0, 100.0]),
    ] {
        let mut transformed = scene.clone();
        transformed["transform"]["position"] = json!(position);
        transformed["transform"]["scale"] = json!(scale);
        if name == "translated" {
            // The Text returns to the output canvas while the Rect is shifted away.
            transformed["layers"][0]["layers"][0]["transform"]["position"] =
                json!([-10_000.0, -10_000.0]);
        }
        let output = to_aep(&two_second_document(vec![transformed])).unwrap();
        assert!(
            output.diagnostics.iter().all(|diagnostic| {
                diagnostic.layer_id != Some(LayerId::new(6_002))
                    || !diagnostic
                        .message
                        .contains("Text omitted from clocked root")
            }),
            "{name}: {:?}",
            output.diagnostics
        );
    }
    let output = to_aep(&two_second_document(vec![scene])).unwrap();
    assert!(
        output.diagnostics.iter().all(|diagnostic| {
            diagnostic.layer_id != Some(LayerId::new(6_000))
                || !diagnostic.message.contains("subtree omitted")
        }),
        "{:?}",
        output.diagnostics
    );
    // This text-only branch has explicit parent references, so the existing
    // reference-closure guard still declines the bounds-only projection.
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(6_002))
            && diagnostic
                .message
                .contains("Text omitted from clocked root")
    }));
    let native = read_project(&output.bytes).unwrap();
    let root = native_named(&native, "Clocked scene");
    assert_eq!(comp_interval(root), (0.5, 2.0));
    let names = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some(composition.layers.iter()),
            _ => None,
        })
        .flatten()
        .map(|layer| layer.name.as_ref())
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"Scene vector"),
        "vector missing from {names:?}"
    );
    assert!(
        !names.contains(&"HUD label"),
        "Referenced Text branch unexpectedly authored: {names:?}"
    );
}

/// An off-canvas sibling cannot authorize losing the only on-canvas Text.
#[test]
fn offscreen_rect_does_not_prune_clocked_root_text() {
    let value = imported();
    let mut text = text_layer(6_121, "Visible HUD", source_text("VISIBLE", true));
    text["parent"] = json!(6_120);
    text["activeRange"] = json!({"start": 0, "duration": 1_500});
    let mut vector = rect(&value, 6_122);
    vector["parent"] = json!(6_120);
    vector["activeRange"] = json!({"start": 0, "duration": 1_500});
    vector["rect"]["fillEnabled"] = json!(true);
    vector["rect"]["fillColor"] = json!([1.0, 0.0, 0.0, 1.0]);
    vector["rect"]["strokeEnabled"] = json!(false);
    vector["transform"]["position"] = json!([10_000.0, 10_000.0]);
    let scene = json!({
        "type": "Group", "id": 6_120, "name": "Offscreen sibling scene", "parent": null,
        "playback": linear_playback(&[(500, 0), (2_000, 1_500)]),
        "transform": zero_transform(), "layers": [text, vector]
    });
    let output = to_aep(&two_second_document(vec![scene])).unwrap();
    assert!(
        output.diagnostics.iter().all(|diagnostic| {
            diagnostic.layer_id != Some(LayerId::new(6_121))
                || !diagnostic
                    .message
                    .contains("Text omitted from clocked root")
        }),
        "{:?}",
        output.diagnostics
    );
}

/// A hidden vector is not evidence that dropping Text saves any visible paint.
#[test]
fn hidden_vector_does_not_justify_clocked_root_text_omission() {
    let value = imported();
    let mut vector = rect(&value, 6_105);
    vector["isHidden"] = json!(true);
    vector["activeRange"] = json!({"start": 0, "duration": 1_500});
    let hidden: Layer = serde_json::from_value(vector.clone()).unwrap();
    let dynamics = AnimationIndex::new(&[]);
    let canvas = fx_schema::Dimensions::new(1_920, 1_080);
    let visible = |layer| has_visual_descendant(layer, 0, 1_500, &dynamics, canvas);
    assert!(!visible(&hidden));

    vector["isHidden"] = json!(false);
    vector["activeRange"] = json!({"start": 1_500, "duration": 500});
    let outside: Layer = serde_json::from_value(vector).unwrap();
    assert!(!visible(&outside));
}

/// A vector behind an opaque-off Group, or outside its actual source window,
/// cannot justify omitting the only visible Text in a clocked scene.
#[test]
fn nonpainting_groups_do_not_justify_clocked_root_text_omission() {
    let value = imported();
    let mut vector = rect(&value, 6_106);
    vector["activeRange"] = json!({"start": 0, "duration": 500});
    let mut group = json!({
        "type": "Group", "id": 6_107, "name": "Nonpainting sibling", "parent": null,
        "playback": identity_playback(1_500), "transform": zero_transform(),
        "layers": [vector]
    });
    group["transform"]["opacity"] = json!(0.0);
    let invisible: Layer = serde_json::from_value(group.clone()).unwrap();
    let dynamics = AnimationIndex::new(&[]);
    let canvas = fx_schema::Dimensions::new(1_920, 1_080);
    assert!(!has_visual_descendant(
        &invisible, 0, 1_500, &dynamics, canvas
    ));

    group["transform"]["opacity"] = json!(100.0);
    let animated: Layer = serde_json::from_value(group.clone()).unwrap();
    let entries = [constant_layer_entry(
        LayerId::new(6_107),
        PropType::Opacity,
        PropertyValue::Float(0.0),
    )];
    let animated_opacity = AnimationIndex::new(&entries);
    assert!(!has_visual_descendant(
        &animated,
        0,
        1_500,
        &animated_opacity,
        canvas
    ));

    group["playback"] = affine_playback([500, 1_500], [500, 1_500]);
    let outside: Layer = serde_json::from_value(group).unwrap();
    assert!(!has_visual_descendant(
        &outside, 0, 1_500, &dynamics, canvas
    ));
}

/// Paint and masks must prove a surviving sibling draws pixels before Text
/// may be omitted from a clocked scene.
#[test]
fn transparent_paint_and_empty_masks_do_not_justify_text_omission() {
    let value = imported();
    let dynamics = AnimationIndex::new(&[]);
    let canvas = fx_schema::Dimensions::new(1_920, 1_080);
    let visible = |layer| has_visual_descendant(layer, 0, 1_500, &dynamics, canvas);
    let mut rect = rect(&value, 6_108);
    rect["activeRange"] = json!({"start": 0, "duration": 1_500});
    rect["rect"]["fillEnabled"] = json!(true);
    rect["rect"]["fillColor"] = json!([1.0, 0.0, 0.0, 0.0]);
    rect["rect"]["strokeEnabled"] = json!(false);
    let transparent: Layer = serde_json::from_value(rect.clone()).unwrap();
    assert!(!visible(&transparent));

    rect["rect"]["fillColor"] = json!([1.0, 0.0, 0.0, 1.0]);
    rect["masks"] = json!([{"id": 6_109, "mode": "add", "opacity": 100,
        "path": {"commands": []}}]);
    let masked: Layer = serde_json::from_value(rect.clone()).unwrap();
    assert!(!visible(&masked));
    rect["masks"] = json!([]);
    let painted: Layer = serde_json::from_value(rect.clone()).unwrap();
    assert!(visible(&painted));
    let mut partial = rect.clone();
    partial["activeRange"] = json!({"start": 500, "duration": 500});
    let partial: Layer = serde_json::from_value(partial).unwrap();
    assert!(!visible(&partial));
    let mut rotated = rect.clone();
    rotated["transform"]["rotation"] = json!(45.0);
    let rotated: Layer = serde_json::from_value(rotated).unwrap();
    assert!(!visible(&rotated));
    let mut group_child = rect.clone();
    group_child["parent"] = json!(6_112);
    let offscreen_group = json!({
        "type": "Group", "id": 6_112, "name": "Offscreen ancestor", "parent": null,
        "playback": identity_playback(1_500),
        "transform": {"anchorPoint": [0, 0], "position": [10_000, 10_000],
            "scale": [100, 100], "rotation": 0, "opacity": 100},
        "layers": [group_child]
    });
    let offscreen_group: Layer = serde_json::from_value(offscreen_group).unwrap();
    assert!(!visible(&offscreen_group));
    rect["transform"]["position"] = json!([10_000.0, 10_000.0]);
    let offscreen: Layer = serde_json::from_value(rect).unwrap();
    assert!(!visible(&offscreen));

    let mut shape = guide_layer(6_110);
    shape["activeRange"] = json!({"start": 0, "duration": 1_500});
    shape["shape"]["ellipse"] = json!({"size": [100.0, 100.0]});
    shape["shape"]["fills"] = json!([{
        "paint": {"type": "solid", "color": [1.0, 0.0, 0.0, 0.0]},
        "opacity": 1.0
    }]);
    let transparent: Layer = serde_json::from_value(shape.clone()).unwrap();
    assert!(!visible(&transparent));
    shape["shape"]["fills"][0]["paint"]["color"] = json!([1.0, 0.0, 0.0, 1.0]);
    let painted: Layer = serde_json::from_value(shape).unwrap();
    // Generated Shape bounds lack this root-viewport proof, even when painted.
    assert!(!visible(&painted));
}

/// Root-viewport certification must keep the native Text matte provider
/// alongside its consumer rather than pruning either part of the dependency.
#[test]
fn clocked_root_matte_text_is_not_pruned() {
    let value = imported();
    let mut text = text_layer(6_102, "Matte text", source_text("MASK", true));
    text["parent"] = json!(6_100);
    text["activeRange"] = json!({"start": 0, "duration": 1_500});
    let mut vector = rect(&value, 6_103);
    vector["parent"] = json!(6_100);
    vector["activeRange"] = json!({"start": 0, "duration": 1_500});
    vector["trackMatte"] = json!({"mode": "alpha", "layer": 6_102});
    let scene = json!({
        "type": "Group", "id": 6_100, "name": "Text matte scene", "parent": null,
        "playback": linear_playback(&[(500, 0), (2_000, 1_500)]),
        "transform": zero_transform(), "layers": [vector, text]
    });
    let output = to_aep(&two_second_document(vec![scene])).unwrap();
    for id in [6_100, 6_102, 6_103] {
        assert!(!output.omitted_layer_ids.contains(&LayerId::new(id)));
    }
    let native = read_project(&output.bytes).unwrap();
    let pair = native.items.iter().find_map(|item| {
        let ItemKind::Composition(comp) = &item.kind else {
            return None;
        };
        let text = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Matte text")?;
        let painted = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Current solid 6103")?;
        Some((text, painted))
    });
    let (text, painted) = pair.expect("Text provider and matted paint share a composition");
    assert_eq!(text.record.layer_type(), 3);
    assert_eq!(painted.record.track_matte_type(), 1);
    assert_eq!(painted.record.matte_layer_id(), Some(text.record.id()));
}

/// A native-derived contour supplies the crop, with an explicitly edited clock.
/// Equivalent guide representations must emit identical native mask payloads.
#[test]
fn native_rectangle_shape_guide_retains_edited_masked_text_with_an_explicit_matching_clock() {
    let bytes = include_bytes!("../../../tests/fixtures/masks/import_mask_controls.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "01380c1f8c5ebe486cd068e5dee50447870b86e4864fc842590a99386dd9b417"
    );
    let native = read_project(bytes).unwrap();
    let imported = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document;
    let guide = find_layer_named(imported.composition().layers(), "target — Mask 1 guide").unwrap();
    let LayerData::Shape(shape) = guide.data() else {
        panic!("native rectangular mask guide")
    };
    assert_eq!(shape.shape.path.commands.len(), 6);
    assert_ne!(
        serde_json::to_value(guide.active_range()).unwrap()["duration"],
        json!(2_000)
    );
    let mut edited_guide = serde_json::to_value(guide).unwrap();
    edited_guide["id"] = json!(70_002);
    edited_guide["parent"] = json!(70_000);
    // Explicit edited FX input, not a claim that the unchanged native import
    // has the same clock. Geometry remains the source-derived rectangle.
    edited_guide["activeRange"] = json!({"start":0,"duration":2_000});
    edited_guide["transform"] = zero_transform();
    let mut title = text_layer(
        70_011,
        "Native rectangle edited Text",
        source_text("U", false),
    );
    title["parent"] = json!(70_010);
    title["activeRange"] = json!({"start":0,"duration":2_000});
    title["transform"] = zero_transform();
    title["transform"]["position"] = json!([160.0, 100.0]);
    let mut paint = rect(&super::imported(), 70_012);
    paint["parent"] = json!(70_010);
    paint["activeRange"] = json!({"start":0,"duration":2_000});
    let masked = json!({
        "type":"Group","id":70_010,"name":"Edited rectangular crop",
        "parent":70_000,"playback":identity_playback(2_000),
        "transform":zero_transform(),"layers":[title,paint],
        "masks":[{"id":70_014,"layer":70_002,"mode":"add",
            "feather":[0.0,0.0],"opacity":1.0,"expansion":0.0}]
    });
    let scene = json!({"type":"Group","id":70_000,"name":"Native rectangle edited scene",
        "parent":null,"playback":identity_playback(2_000),
        "transform":zero_transform(),"layers":[masked,edited_guide]});
    let output = to_aep(&two_second_document(vec![scene.clone()])).unwrap();
    let mut equivalent = scene.clone();
    let replacement = &mut equivalent["layers"][1];
    replacement.as_object_mut().unwrap().remove("shape");
    replacement["type"] = json!("Rect");
    let left = shape.shape.path.commands[0].endpoint().unwrap();
    let right = shape.shape.path.commands[2].endpoint().unwrap();
    replacement["rect"] = rect(&super::imported(), 70_002)["rect"].clone();
    replacement["rect"]["position"] = json!([left.0, left.1]);
    replacement["rect"]["size"] = json!([right.0 - left.0, right.1 - left.1]);
    replacement["rect"]["roundness"] = json!(0.0);
    replacement["rect"]["fillEnabled"] = json!(false);
    replacement["rect"]["strokeEnabled"] = json!(false);
    let rect_output = to_aep(&two_second_document(vec![equivalent])).unwrap();
    assert_eq!(
        exported_crop_payloads(&output.bytes),
        exported_crop_payloads(&rect_output.bytes),
        "equivalent Shape and Rect native mask geometry/controls/clock"
    );
    let mut implicit_close = scene.clone();
    implicit_close["layers"][1]["shape"]["path"]["commands"]
        .as_array_mut()
        .unwrap()
        .remove(4);
    let implicit_output = to_aep(&two_second_document(vec![implicit_close])).unwrap();
    assert_eq!(
        exported_crop_payloads(&output.bytes),
        exported_crop_payloads(&implicit_output.bytes)
    );
    for case in [
        "near rectangle",
        "rounded",
        "mirror",
        "clock",
        "feather",
        "external",
        "degenerate",
        "curved",
        "multicontour",
        "wrong closure",
        "modifier",
    ] {
        let mut bad = scene.clone();
        match case {
            "near rectangle" => {
                bad["layers"][1]["shape"]["path"]["commands"][2]["x"] = json!(381.0)
            }
            "rounded" => {
                bad["layers"][1]["shape"]["path"]["commands"][0]["cornerRadius"] = json!(1.0)
            }
            "mirror" => {
                bad["layers"][1]["shape"]["path"]["commands"][0]["mirror"] = json!("straight")
            }
            "clock" => bad["layers"][1]["activeRange"]["duration"] = json!(3_000),
            "feather" => bad["layers"][0]["masks"][0]["feather"] = json!([1.0, 0.0]),
            "external" => bad["layers"][1]["parent"] = Value::Null,
            "curved" => {
                bad["layers"][1]["shape"]["path"]["commands"][1] = json!({
                "type":"cubicTo","x":right.0,"y":left.1,
                "c1x":left.0,"c1y":left.1,"c2x":right.0,"c2y":left.1})
            }
            "multicontour" => bad["layers"][1]["shape"]["path"]["commands"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"moveTo","x":0,"y":0})),
            "wrong closure" => bad["layers"][1]["shape"]["path"]["commands"][4]["x"] = json!(61.0),
            "modifier" => bad["layers"][1]["shape"]["roundCorners"] = json!({"radius":1.0}),
            _ => bad["layers"][1]["shape"]["path"]["commands"][1]["x"] = json!(60.0),
        }
        let rejected = to_aep(&two_second_document(vec![bad])).unwrap();
        assert!(
            rejected.omitted_layer_ids.contains(&LayerId::new(70_010)),
            "{case}: {:?}",
            rejected.diagnostics
        );
    }
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(70_010)),
        "{:?}",
        output.diagnostics
    );
    let restored = read_project(&output.bytes).unwrap();
    assert!(restored.items.iter().any(|item| {
        match &item.kind {
            ItemKind::Composition(comp) => comp.layers.iter().any(|layer| {
                layer.record.layer_type() == 3
                    && layer.name.as_ref() == "Native rectangle edited Text"
            }),
            _ => false,
        }
    }));
}

fn exported_crop_payloads(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    fn collect(chunks: &[crate::rifx::Chunk], output: &mut Vec<([u8; 4], Vec<u8>)>) {
        for chunk in chunks {
            if let Some(data) = chunk.data_payload() {
                output.push((chunk.id(), data.to_vec()));
            }
            if let Some(children) = chunk.children() {
                collect(children, output);
            }
        }
    }
    let native = read_project(bytes).unwrap();
    let layer = native
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(comp) => comp
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == "Edited rectangular crop"),
            _ => None,
        })
        .unwrap();
    let roots = crate::properties::root_runs(&layer.content).unwrap();
    let parade = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Mask Parade")
        .unwrap()
        .1;
    let mut result = Vec::new();
    collect(parade, &mut result);
    assert!(!result.is_empty());
    result
}

/// Collapse admits plain 2D Text only. Mixed content keeps the vector rules,
/// and occurrence masks, child blend modes, motion blur or 3D would rasterize
/// or re-blend the collapsed layer, so each stays a diagnosed omission.
#[test]
fn static_hard_rect_guide_preserves_masked_mixed_text_without_admitting_soft_or_rounded_masks() {
    let mut guide = rect(&imported(), 70_002);
    guide["name"] = json!("LOUD bottom mask guide");
    guide["parent"] = json!(70_000);
    guide["activeRange"] = json!({"start": 0, "duration": 2_000});
    guide["rect"]["size"] = json!([1080.0, 1010.0]);
    guide["rect"]["roundness"] = json!(0.0);
    let mut title = text_layer(70_011, "LOUD U", source_text("U", true));
    title["parent"] = json!(70_010);
    title["activeRange"] = json!({"start": 0, "duration": 2_000});
    let mut paint = rect(&imported(), 70_012);
    paint["parent"] = json!(70_010);
    paint["activeRange"] = json!({"start": 0, "duration": 2_000});
    let masked = json!({
        "type":"Group", "id":70_010, "name":"LOUD masked title",
        "parent":70_000, "playback":identity_playback(2_000),
        "transform":zero_transform(), "layers":[title,paint],
        "effects":[], // Mask proof uses a supported owner, not omitted shader rendering.
        "masks":[{"id":70_014,"layer":70_002,"mode":"add",
            "feather":[0.0,0.0],"opacity":1.0,"expansion":0.0}]
    });
    let scene = json!({
        "type":"Group", "id":70_000, "name":"B02 masked text scene",
        "parent":null, "playback":identity_playback(2_000),
        "transform":zero_transform(), "layers":[masked,guide]
    });
    let check = |scene: Value| to_aep(&two_second_document(vec![scene])).unwrap();
    let output = check(scene.clone());
    for id in [70_000, 70_010, 70_011, 70_012] {
        assert!(
            !output.omitted_layer_ids.contains(&LayerId::new(id)),
            "{id}: {:?}",
            output.diagnostics
        );
    }
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(70_010))
            && diagnostic.message.contains("guide layer 70002 was copied")
    }));
    let native = read_project(&output.bytes).unwrap();
    assert!(native.items.iter().any(|item| {
        match &item.kind {
            ItemKind::Composition(comp) => comp
                .layers
                .iter()
                .any(|layer| layer.record.layer_type() == 3 && layer.name.as_ref() == "LOUD U"),
            _ => false,
        }
    }));

    for (case, mut bad) in [
        ("rounded guide", scene.clone()),
        ("feathered Add mask", scene.clone()),
        ("external guide", scene),
    ] {
        match case {
            "rounded guide" => bad["layers"][1]["rect"]["roundness"] = json!(8.0),
            "feathered Add mask" => bad["layers"][0]["masks"][0]["feather"] = json!([5.0, 0.0]),
            _ => bad["layers"][1]["parent"] = Value::Null,
        }
        let rejected = check(bad);
        assert!(
            rejected.omitted_layer_ids.contains(&LayerId::new(70_010)),
            "{case}: {:?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn clocked_text_collapse_rejects_mixed_masked_blended_blurred_or_3d_content() {
    let base = || text_occurrence(5_800, "Late", [(500, 0), (2_000, 1_500)]);
    let mut mixed = base();
    let mut vector = rect(&imported(), 5_890);
    vector["parent"] = json!(5_801);
    mixed["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .push(vector);
    let mut masked = base();
    masked["layers"][0]["masks"] = json!([{
        "id": 5_891, "mode": "add", "opacity": 100,
        "path": {"commands": [
            {"type": "moveTo", "x": 0, "y": 0},
            {"type": "lineTo", "x": 100, "y": 0},
            {"type": "lineTo", "x": 100, "y": 100},
            {"type": "close"}
        ]}
    }]);
    let segment = |edit: &dyn Fn(&mut Value)| {
        let mut value = base();
        edit(&mut value["layers"][0]["layers"][0]["layers"][0]["layers"][0]);
        value
    };
    for (case, occurrence) in [
        ("mixed Text and vector", mixed),
        ("occurrence mask", masked),
        (
            "child blend mode",
            segment(&|text| text["blendMode"] = json!("multiply")),
        ),
        (
            "child motion blur",
            segment(&|text| text["motionBlur"] = json!(true)),
        ),
        (
            "child 3D rotation",
            segment(&|text| {
                text["transform"]["rotationX"] = json!(20.0);
            }),
        ),
    ] {
        let output = to_aep(&two_second_document(vec![occurrence])).unwrap();
        // 3D already rules out the occurrence's own Null parent.
        assert!(
            output.diagnostics.iter().any(|diagnostic| {
                [Some(LayerId::new(5_800)), Some(LayerId::new(5_801))]
                    .contains(&diagnostic.layer_id)
                    && diagnostic.message.contains("subtree omitted")
            }),
            "{case}: {:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        assert!(
            native.items.iter().all(|item| match &item.kind {
                ItemKind::Composition(composition) => composition
                    .layers
                    .iter()
                    .all(|layer| layer.record.layer_type() != 3),
                _ => true,
            }),
            "{case}"
        );
    }
}

/// A document on its own canvas and duration.
fn canvas_document(
    [width, height]: [u32; 2],
    duration_millis: u64,
    layers: Vec<Value>,
) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["dimensions"] = json!({"width": width, "height": height});
    value["duration"] = json!(duration_millis as f64 / 1_000.0);
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": []});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

/// [`held_text_layer`] over a `span`-millisecond source whose "Source content
/// clock" keeps the importer's explicit identity keys.
fn held_text_source(id: u64, name: &str, span: u64, segments: &[(&str, u64, u64)]) -> Value {
    let mut layer = held_text_layer(id, name, linear_playback(&[(0, 0), (span, span)]), segments);
    layer["playback"] = identity_playback(span);
    layer
}

/// A wrapper Group (`id`) over `span` holding one occurrence Group (`id + 1`)
/// that maps its `[start, end)` parent interval through `playback` into
/// `content`.
fn clocked_occurrence(
    id: u64,
    name: &str,
    span: u64,
    [start, end]: [u64; 2],
    playback: Value,
    mut content: Value,
) -> Value {
    content["parent"] = json!(id + 1);
    assert_eq!(
        playback["inputRange"],
        json!({"start": start, "duration": end - start})
    );
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "playback": identity_playback(span),
        "transform": identity_transform(),
        "layers": [{
            "type": "Group",
            "id": id + 1,
            "name": format!("{name} clock"),
            "parent": id,
            "transform": zero_transform(),
            "playback": playback,
            "layers": [content]
        }]
    })
}

/// A hidden plain Group (`id`) holding one Text per segment from `id + 1`. A
/// hidden Group cannot use a Null parent, so its Text must precompose.
fn hidden_text_owner(
    id: u64,
    name: &str,
    playback: &Value,
    segments: &[(&str, u64, u64)],
) -> Value {
    let texts = segments
        .iter()
        .zip(id + 1..)
        .map(|((text, start, duration), text_id)| {
            let mut layer =
                text_layer(text_id, &format!("{name} {text}"), source_text(text, false));
            layer["parent"] = json!(id);
            layer["activeRange"] = json!({"start": start, "duration": duration});
            layer["transform"] = zero_transform();
            layer
        })
        .collect::<Vec<_>>();
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "isHidden": true,
        "playback": playback,
        "transform": identity_transform(),
        "layers": texts
    })
}

/// The collapsed occurrence named `name` in `composition` and its source
/// composition, whose nominal size must be the root `canvas`.
fn collapsed_occurrence<'a>(
    project: &'a StructuralProject,
    composition: u32,
    name: &str,
    canvas: [u32; 2],
) -> (&'a crate::structure::Layer, u32) {
    let occurrence = composition_layers(project, composition)
        .iter()
        .find(|layer| layer.name.as_ref() == name)
        .unwrap_or_else(|| panic!("native occurrence {name:?} in composition {composition}"));
    let record = &occurrence.record;
    assert!(record.flags().collapse_transformation, "{name}");
    let ItemKind::Composition(source) = &project.item(record.source_id()).unwrap().kind else {
        panic!("{name}: occurrence source");
    };
    assert_eq!(
        [u32::from(source.width), u32::from(source.height)],
        canvas,
        "{name}: nominal root canvas"
    );
    (occurrence, record.source_id())
}

/// Exact native record clock `[start, in, out, stretch]`.
fn record_clock(layer: &crate::structure::Layer) -> [(i32, u32); 4] {
    let record = &layer.record;
    [
        record.start_time_fraction(),
        record.in_point_fraction(),
        record.out_point_fraction(),
        record.stretch_fraction(),
    ]
}

/// Native Text layers of `composition` as `(name, comp-time interval)`.
fn held_texts(project: &StructuralProject, composition: u32) -> Vec<(String, (f64, f64))> {
    composition_layers(project, composition)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .map(|layer| (layer.name.as_ref().to_owned(), comp_interval(layer)))
        .collect()
}

/// The authored source intervals of the held `segments` of `name`.
fn authored_intervals(name: &str, segments: &[(&str, u64, u64)]) -> Vec<(String, (f64, f64))> {
    segments
        .iter()
        .map(|&(text, start, duration)| {
            (
                format!("{name} {text}"),
                (start as f64 / 1_000.0, (start + duration) as f64 / 1_000.0),
            )
        })
        .collect()
}

fn omitted_subtree(output: &ExportedDocument, id: u64) -> bool {
    output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(id))
            && diagnostic.message.contains("subtree omitted")
    })
}

/// Every occurrence clock the writer represents exactly keeps a plain-Text
/// occurrence collapsed. Each expected record follows by hand from AE's
/// `parent = start + source * stretch` for the authored FX map, and each held
/// value keeps its authored source interval. Text boundaries are multiples of
/// 125 ms, which native 1/24576 s ticks store exactly.
#[test]
fn clocked_text_collapse_keeps_each_supported_occurrence_clock_exact() {
    struct Case {
        canvas: [u32; 2],
        duration: u64,
        id: u64,
        name: &'static str,
        active: [u64; 2],
        playback: Value,
        span: u64,
        segments: &'static [(&'static str, u64, u64)],
        clock: [(i32, u32); 4],
    }
    let cases = [
        // Offset: 750 -> 0 and 3000 -> 2250, so stretch 1 and start 0.75 s.
        Case {
            canvas: [1280, 720],
            duration: 3_000,
            id: 6_100,
            name: "Countdown",
            active: [750, 3_000],
            playback: linear_playback(&[(750, 0), (3_000, 2_250)]),
            span: 2_250,
            segments: &[("Three", 0, 750), ("Two", 750, 750), ("One", 1_500, 750)],
            clock: [(3, 4), (0, 1), (9, 4), (1, 1)],
        },
        // Negative start with a trimmed head: 0 -> 600 and 1400 -> 2000, so
        // stretch 1 and start 0 - 0.6 = -0.6 s.
        Case {
            canvas: [1080, 1920],
            duration: 2_000,
            id: 6_200,
            name: "Ticker",
            active: [0, 1_400],
            playback: linear_playback(&[(0, 600), (1_400, 2_000)]),
            span: 2_000,
            segments: &[("Alpha", 0, 875), ("Beta", 875, 1_125)],
            clock: [(-3, 5), (3, 5), (2, 1), (1, 1)],
        },
        // Trimmed at both ends: 200 -> 450 and 1700 -> 1950, so stretch 1 and
        // start 0.2 - 0.45 = -0.25 s.
        Case {
            canvas: [640, 360],
            duration: 2_500,
            id: 6_300,
            name: "Lower third",
            active: [200, 1_700],
            playback: linear_playback(&[(200, 450), (1_700, 1_950)]),
            span: 2_500,
            segments: &[("first line", 0, 1_250), ("second line", 1_250, 1_250)],
            clock: [(-1, 4), (9, 20), (39, 20), (1, 1)],
        },
        // Slow keys: 0 -> 0 and 2000 -> 800, so stretch 2000/800 = 5/2.
        Case {
            canvas: [1920, 1080],
            duration: 2_000,
            id: 6_400,
            name: "Slow title",
            active: [0, 2_000],
            playback: linear_playback(&[(0, 0), (2_000, 800)]),
            span: 2_000,
            segments: &[("Dawn", 0, 375), ("Dusk", 375, 1_625)],
            clock: [(0, 1), (0, 1), (4, 5), (5, 2)],
        },
        // Fast keys: 500 -> 0 and 1500 -> 2000, so stretch 1000/2000 = 1/2 and
        // start 0.5 s.
        Case {
            canvas: [720, 720],
            duration: 2_000,
            id: 6_500,
            name: "Fast badge",
            active: [500, 1_500],
            playback: linear_playback(&[(500, 0), (1_500, 2_000)]),
            span: 2_000,
            segments: &[("left", 0, 1_000), ("right", 1_000, 1_000)],
            clock: [(1, 2), (0, 1), (2, 1), (1, 2)],
        },
        // Half rate from 400 ms: 1600 ms of parent time reaches source 800 ms,
        // so stretch 1600/800 = 2 and start 0.4 s.
        Case {
            canvas: [800, 600],
            duration: 2_000,
            id: 6_600,
            name: "Half rate",
            active: [400, 2_000],
            playback: affine_playback([400, 2_000], [0, 800]),
            span: 1_625,
            segments: &[("half one", 0, 500), ("half two", 500, 1_125)],
            clock: [(2, 5), (0, 1), (4, 5), (2, 1)],
        },
        // Double rate from 0: 900 ms of parent time reaches source 1800 ms, so
        // stretch 900/1800 = 1/2.
        Case {
            canvas: [1024, 768],
            duration: 2_000,
            id: 6_700,
            name: "Double rate",
            active: [0, 900],
            playback: affine_playback([0, 900], [0, 1_800]),
            span: 1_875,
            segments: &[("double one", 0, 1_000), ("double two", 1_000, 875)],
            clock: [(0, 1), (0, 1), (9, 5), (1, 2)],
        },
    ];
    for case in cases {
        let text = format!("{} text", case.name);
        let document = canvas_document(
            case.canvas,
            case.duration,
            vec![clocked_occurrence(
                case.id,
                case.name,
                case.duration,
                case.active,
                case.playback,
                held_text_source(case.id + 10, &text, case.span, case.segments),
            )],
        );
        let output = to_aep(&document).unwrap();
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
            "{}: {:?}",
            case.name,
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let (occurrence, source) =
            collapsed_occurrence(&native, 1, &format!("{} clock", case.name), case.canvas);
        assert_eq!(record_clock(occurrence), case.clock, "{}", case.name);
        assert_eq!(
            held_texts(&native, source),
            authored_intervals(&text, case.segments),
            "{}",
            case.name
        );
        for (value, ..) in case.segments {
            assert_eq!(
                generated_document_texts(&output.bytes)
                    .iter()
                    .filter(|text| *text == &format!("{value}\r"))
                    .count(),
                1,
                "{}: {value:?}",
                case.name
            );
        }
    }
}

/// A clocked Text occurrence inside another keeps both clocks: each clocked
/// Group is classified on its own, so each collapses with its own record.
/// Outer: 1000 -> 0 and 3000 -> 2000, so start 1 s. Inner, in the outer
/// source: 0 -> 500 and 1000 -> 1500, so start -0.5 s. Both have stretch 1.
#[test]
fn nested_clocked_text_occurrences_each_keep_their_own_exact_clock() {
    const SEGMENTS: &[(&str, u64, u64)] = &[("Inner first", 0, 625), ("Inner second", 625, 875)];
    let inner = clocked_occurrence(
        6_850,
        "Inner",
        2_000,
        [0, 1_000],
        linear_playback(&[(0, 500), (1_000, 1_500)]),
        held_text_source(6_860, "Inner text", 1_500, SEGMENTS),
    );
    let document = canvas_document(
        [1600, 900],
        3_000,
        vec![clocked_occurrence(
            6_800,
            "Outer",
            3_000,
            [1_000, 3_000],
            linear_playback(&[(1_000, 0), (3_000, 2_000)]),
            inner,
        )],
    );
    let output = to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let (outer, outer_source) = collapsed_occurrence(&native, 1, "Outer clock", [1600, 900]);
    assert_eq!(record_clock(outer), [(1, 1), (0, 1), (2, 1), (1, 1)]);
    let (inner, inner_source) =
        collapsed_occurrence(&native, outer_source, "Inner clock", [1600, 900]);
    assert_eq!(record_clock(inner), [(-1, 2), (1, 2), (3, 2), (1, 1)]);
    assert!(held_texts(&native, outer_source).is_empty());
    assert_eq!(
        held_texts(&native, inner_source),
        authored_intervals("Inner text", SEGMENTS)
    );
}

/// Hidden plain-Text owners, at the root and inside a 0.5 s-late occurrence
/// source, both carrying `clocks`. Each must precompose and collapse with an
/// identity record, keeping its held values.
fn assert_hidden_text_owners_collapse(clocks: [Value; 2]) {
    const ROOT: &[(&str, u64, u64)] = &[("Standby", 0, 1_250), ("On air", 1_250, 1_250)];
    const NESTED: &[(&str, u64, u64)] = &[("Relay open", 0, 500), ("Relay closed", 500, 1_500)];
    let [root_clock, nested_clock] = clocks;
    let mut relay = clocked_occurrence(
        7_200,
        "Relay",
        2_500,
        [500, 2_500],
        linear_playback(&[(500, 0), (2_500, 2_000)]),
        hidden_text_owner(7_210, "Relay monitor", &nested_clock, NESTED),
    );
    // Hidden-only content has no render bounds, so the clocked source also
    // holds a visible caption.
    let mut caption = text_layer(7_220, "Relay caption", source_text("Relay caption", false));
    caption["parent"] = json!(7_201);
    caption["activeRange"] = json!({"start": 0, "duration": 2_000});
    caption["transform"] = zero_transform();
    relay["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .push(caption);
    let document = canvas_document(
        [1366, 768],
        2_500,
        vec![hidden_text_owner(7_100, "Studio", &root_clock, ROOT), relay],
    );
    let output = to_aep(&document).unwrap();
    let omitted = [7_100, 7_201, 7_210]
        .into_iter()
        .filter(|&id| omitted_subtree(&output, id))
        .collect::<Vec<_>>();
    assert!(omitted.is_empty(), "{omitted:?}: {:?}", output.diagnostics);
    let native = read_project(&output.bytes).unwrap();
    let (studio, studio_source) = collapsed_occurrence(&native, 1, "Studio", [1366, 768]);
    assert!(!studio.record.flags().enabled, "hidden root owner");
    assert_eq!(comp_interval(studio), (0.0, 2.5));
    assert_eq!(
        held_texts(&native, studio_source),
        authored_intervals("Studio", ROOT)
    );
    let (relay, relay_source) = collapsed_occurrence(&native, 1, "Relay clock", [1366, 768]);
    assert_eq!(record_clock(relay), [(1, 2), (0, 1), (2, 1), (1, 1)]);
    assert_eq!(
        held_texts(&native, relay_source),
        [("Relay caption".to_owned(), (0.0, 2.0))]
    );
    let (monitor, monitor_source) =
        collapsed_occurrence(&native, relay_source, "Relay monitor", [1366, 768]);
    assert!(!monitor.record.flags().enabled, "hidden nested owner");
    assert_eq!(comp_interval(monitor), (0.0, 2.0));
    assert_eq!(
        held_texts(&native, monitor_source),
        authored_intervals("Relay monitor", NESTED)
    );
}

#[test]
fn hidden_text_owner_with_canonical_identity_collapses_at_the_root_and_in_a_clocked_source() {
    assert_hidden_text_owners_collapse([identity_playback(2_500), identity_playback(2_000)]);
}

/// Text collapse uses the owner clock its export caller already validated,
/// independently of the mapping domain's endpoints. Both identity mappings
/// cover more than each owner's visible window.
#[test]
fn hidden_text_owner_with_a_wider_identity_mapping_collapses_like_canonical_identity() {
    let mut root = identity_playback(5_000);
    root["inputRange"]["duration"] = json!(2_500);
    let mut nested = identity_playback(4_000);
    nested["inputRange"]["duration"] = json!(2_000);
    assert_hidden_text_owners_collapse([root, nested]);
}

/// Identity keys over each owner's whole span keep the same native record as
/// the canonical linear identity mapping.
#[test]
fn hidden_text_owner_with_explicit_identity_keys_collapses_like_canonical_identity() {
    assert_hidden_text_owners_collapse([
        linear_playback(&[(0, 0), (2_500, 2_500)]),
        linear_playback(&[(0, 0), (2_000, 2_000)]),
    ]);
}

/// Occurrence effects and masks would rasterize a collapsed Text layer, so
/// those clocked occurrences stay diagnosed omissions while their supported
/// sibling keeps its exact clock and held value.
#[test]
fn clocked_text_exclusions_keep_a_supported_sibling_collapsed() {
    const KEPT: &[(&str, u64, u64)] = &[("kept value", 0, 1_750)];
    // 250 -> 0 and 2000 -> 1750, so stretch 1 and start 0.25 s.
    let occurrence = |id: u64, name: &str, value: &'static str| {
        clocked_occurrence(
            id,
            name,
            2_000,
            [250, 2_000],
            linear_playback(&[(250, 0), (2_000, 1_750)]),
            held_text_source(
                id + 10,
                &format!("{name} text"),
                1_750,
                &[(value, 0, 1_750)],
            ),
        )
    };
    let mut exposed = occurrence(6_920, "Exposed", "exposed value");
    exposed["layers"][0]["effects"] = json!([{"type": "exposure", "exposure": 1.0}]);
    let mut masked = occurrence(6_940, "Masked", "masked value");
    masked["layers"][0]["masks"] = json!([{
        "id": 6_959, "mode": "add", "opacity": 100,
        "path": {"commands": [
            {"type": "moveTo", "x": 0, "y": 0},
            {"type": "lineTo", "x": 200, "y": 0},
            {"type": "lineTo", "x": 200, "y": 100},
            {"type": "close"}
        ]}
    }]);
    let document = canvas_document(
        [960, 540],
        2_000,
        vec![exposed, occurrence(6_900, "Kept", "kept value"), masked],
    );
    let output = to_aep(&document).unwrap();
    // Pointwise exposure has a finite consumer-domain source; it must not use
    // the Text-only collapse route, but no longer needs to omit its content.
    assert!(!omitted_subtree(&output, 6_921), "{:?}", output.diagnostics);
    assert!(
        output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(6_941))
                && diagnostic.message.contains(
                    "Collapsed Text source requires a 2D occurrence without masks or matte consumers",
                )
        }),
        "{:?}",
        output.diagnostics
    );
    assert!(!omitted_subtree(&output, 6_901), "{:?}", output.diagnostics);
    let native = read_project(&output.bytes).unwrap();
    let (kept, source) = collapsed_occurrence(&native, 1, "Kept clock", [960, 540]);
    assert_eq!(record_clock(kept), [(1, 4), (0, 1), (7, 4), (1, 1)]);
    assert_eq!(
        held_texts(&native, source),
        authored_intervals("Kept text", KEPT)
    );
    assert_eq!(
        generated_document_texts(&output.bytes)
            .iter()
            .filter(|text| *text == "exposed value\r")
            .count(),
        1,
    );
    assert_eq!(
        generated_document_texts(&output.bytes)
            .iter()
            .filter(|text| *text == "masked value\r")
            .count(),
        0,
    );
}

#[test]
fn keyed_range_full_export_matches_native_envelopes_and_edited_input() {
    fn property<'a>(
        chunks: &'a [crate::rifx::Chunk],
        name: &str,
    ) -> Option<&'a [crate::rifx::Chunk]> {
        if let Ok(records) = crate::properties::runs(chunks)
            && let Some((_, record)) = records.into_iter().find(|(key, _)| *key == name)
        {
            return Some(record);
        }
        chunks.iter().find_map(|chunk| {
            chunk
                .children()
                .and_then(|children| property(children, name))
        })
    }
    let source = include_bytes!("../../../tests/fixtures/text/import_selector_keyed_float.aep");
    let native = read_project(source).unwrap();
    let native_layer = &layers(&native)[0];
    let texts = imported_texts(&native);
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].animators[0].selectors.len(), 1);
    let selector = &texts[0].animators[0].selectors[0];
    let imported = to_structural_fx_document(&native, Some(1)).unwrap();
    for (name, values) in [
        ("start", [0.0, 0.3]),
        ("end", [0.8, 1.0]),
        ("offset", [-0.2, 0.55]),
        ("amount", [0.25, 0.75]),
    ] {
        let entry = imported.document.composition().dynamics().entries().iter().find(|entry| matches!(&entry.target, PropertyTarget::FxItemProperty(target) if target.item_id() == selector.id && target.property_name() == name)).unwrap();
        let keys = entry.animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.len(), 2);
        for (key, (millis, value)) in keys.iter().zip([0, 500].into_iter().zip(values)) {
            assert_eq!(key.layer_time(), fx_schema::TimeOffset::from_millis(millis));
            assert_eq!(key.value(), &PropertyValue::Float(value));
            assert_eq!(key.easing(), PropertyKeyframeEasing::Linear);
        }
    }
    for edited_offset in [0.55, 0.7] {
        let channels = [
            ("start", "ADBE Text Percent Start", [0.0, 0.3]),
            ("end", "ADBE Text Percent End", [0.8, 1.0]),
            ("offset", "ADBE Text Percent Offset", [-0.2, edited_offset]),
            ("amount", "ADBE Text Selector Max Amount", [0.25, 0.75]),
        ];
        let entries = channels
            .iter()
            .map(|(name, _, values)| {
                item_entry(
                    FxItemId::new(5202),
                    name,
                    [
                        (0, PropertyValue::Float(values[0])),
                        (500, PropertyValue::Float(values[1])),
                    ],
                )
            })
            .collect();
        let document = explicit_document(vec![guide_layer(5299), all_channel_text()], entries);
        let output = to_aep(&document).unwrap();
        let generated = read_project(&output.bytes).unwrap();
        for (_, name, values) in channels {
            let expected = crate::properties::unique_list(
                property(&native_layer.content, name).unwrap(),
                *b"tdbs",
            )
            .unwrap();
            let actual = crate::properties::unique_list(
                property(&layers(&generated)[0].content, name).unwrap(),
                *b"tdbs",
            )
            .unwrap();
            assert_eq!(
                crate::properties::data(actual, *b"tdsb").unwrap(),
                crate::properties::data(expected, *b"tdsb").unwrap()
            );
            let mut descriptor = crate::properties::data(expected, *b"tdb4")
                .unwrap()
                .to_vec();
            descriptor[12..16].copy_from_slice(&24576_u32.to_be_bytes());
            assert_eq!(
                crate::properties::data(actual, *b"tdb4").unwrap(),
                descriptor
            );
            let numeric = crate::properties::read_numeric(actual).unwrap();
            assert_eq!(numeric.keyframes.len(), 2);
            for (key, (time, value)) in numeric
                .keyframes
                .iter()
                .zip([0.0, 0.5].into_iter().zip(values))
            {
                assert_eq!(key.time_secs, time);
                assert!((key.values[0] - value * 100.0).abs() < 1e-10);
                assert_eq!((key.in_interpolation, key.out_interpolation), (1, 1));
            }
        }
    }
}
