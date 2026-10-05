//! Source-derived clock patterns from P019/P028/P037; private assets/content omitted.
//! CPU structure contracts, not independent Adobe visual proof.
use super::*;

fn remap(start: u64, duration: u64, points: &[(u64, u64)]) -> Value {
    json!({"type":"windowed","inputRange":{"start":start,"duration":duration},
        "mapping":{"type":"timeRemap","property":{"before":"hold","after":"hold",
            "keyframes":points.iter().enumerate().map(|(i,(time,value))|json!({
                "id":format!("clock-{i}"),"time":time,"value":value,"easing":{"type":"linear"}
            })).collect::<Vec<_>>() }},"inputOffsetMs":0})
}
fn media_source(start: u64, duration: u64, code: &str) -> EditableFxCompositionDocument {
    modify(
        &media_on_clock(
            "Video",
            remap(
                2750,
                15250,
                &[(2750, start), (2850, start), (17950, start + duration - 1)],
            ),
        ),
        |raw| {
            raw["composition"]["layers"][0]["sourceRange"] =
                json!({"start":start,"duration":duration});
            raw["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(start + duration);
            raw["composition"]["dynamics"]["entries"][0]["animator"]["layerTimeJsCode"] =
                json!(code);
        },
    )
}
fn group_document(
    code: &str,
    playback: Value,
    child_duration: u64,
) -> EditableFxCompositionDocument {
    modify(&document(code), |raw| {
        let mut child = raw["composition"]["layers"][0].clone();
        child["activeRange"] = json!({"start":0,"duration":child_duration});
        child["parent"] = json!(8);
        let transform = child["transform"].clone();
        raw["composition"]["layers"] = json!([{"type":"Group","id":8,"name":"Clock container","playback":playback,"transform":transform,"layers":[child]}]);
    })
}
fn dependency_document() -> EditableFxCompositionDocument {
    modify(
        &document("return input.deps[0].value + input.deps[1].value;"),
        |raw| {
            let mut sibling = raw["composition"]["layers"][0].clone();
            sibling["id"] = json!(9);
            raw["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .push(sibling);
            raw["composition"]["dynamics"]["entries"][0]["dependencies"] = json!([
            {"kind":"layer","layerId":9,"propertyType":"scaleX"},
            {"kind":"layer","layerId":9,"propertyType":"scaleX"}]);
            raw["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({
            "target":{"kind":"layer","layerId":9,"propertyType":"scaleX"},
            "animator":{"type":"jsScript","layerTimeJsCode":"return 50 + input.time.milliseconds;"}}));
        },
    )
}
#[test]
fn playback_p028_source_domain_not_occurrence_duration() {
    let original = media_source(
        0,
        15041,
        "return 100*Math.max(0,Math.min(1,input.time.milliseconds/120));",
    );
    let before = original.to_json_value().unwrap();
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        clock::Clocks::default()
            .owner(&original.composition().layers()[0], &[])
            .0
            .end_ms(),
        15041
    );
    assert_eq!(
        track(&prepared, 0).keyframes().last().unwrap().value(),
        &PropertyValue::Float(100.0)
    );
    assert_eq!(original.to_json_value().unwrap(), before);
    assert_eq!(
        prepared.document.to_json_value().unwrap()["composition"]["layers"],
        before["composition"]["layers"]
    );
}
#[test]
fn playback_p019_source_clock_effect_curve() {
    let original = modify(
        &media_source(
            0,
            15041,
            "const u=Math.max(0,Math.min(1,(input.time.seconds-2.5)/3)); return 1920/(6-5*u*u*(3-2*u));",
        ),
        |raw| {
            raw["composition"]["layers"][0]["effects"] = json!([{"id":101,"enabled":true,"effect":{"type":"mosaic","horizontalBlocks":6,"verticalBlocks":6,"sharpColors":false}}]);
            raw["composition"]["dynamics"]["entries"][0]["target"] =
                json!({"kind":"effectProperty","effectId":101,"paramName":"horizontalBlocks"});
        },
    );
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0).keyframes()[0].value(),
        &PropertyValue::Float(320.0)
    );
}
#[test]
fn playback_nonzero_source_domain_uses_absolute_key_times() {
    let prepared = prepare(&media_source(2000, 200, "return input.time.milliseconds;"))
        .unwrap()
        .document
        .into_owned();
    let keys = prepared.composition().dynamics().entries()[0]
        .animator
        .keyframe_track()
        .unwrap()
        .keyframes();
    assert_eq!(keys[0].layer_time().as_millis(), 2000);
    assert_eq!(keys[0].value(), &PropertyValue::Float(2000.0));
    assert_eq!(keys.last().unwrap().layer_time().as_millis(), 2200);
}
#[test]
fn playback_linear_media_shift_remains_input_relative() {
    let original = media_on_clock("Video", media_clock(500, 1));
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0).keyframes()[0].layer_time().as_millis(),
        0
    );
    assert_eq!(
        track(&prepared, 0).keyframes()[0].value(),
        &PropertyValue::Float(0.5)
    );
}
#[test]
fn playback_delayed_parent_keeps_child_clock() {
    let original = group_document(
        "return input.time.milliseconds;",
        remap(500, 100, &[(500, 0), (600, 100)]),
        100,
    );
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0)
            .keyframes()
            .last()
            .unwrap()
            .layer_time()
            .as_millis(),
        100
    );
}
#[test]
fn playback_affine_group_speed_keeps_child_domain() {
    let original = group_document(
        "return input.time.milliseconds;",
        json!({"type":"windowed","inputRange":{"start":500,"duration":100},"mapping":{"type":"linear","input":{"start":500,"duration":100},"output":{"start":0,"duration":200}},"inputOffsetMs":0}),
        200,
    );
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0)
            .keyframes()
            .last()
            .unwrap()
            .layer_time()
            .as_millis(),
        200
    );
}
#[test]
fn playback_hold_reverse_mapping_does_not_retime_child_keys() {
    let original = group_document(
        "return input.time.milliseconds;",
        remap(0, 7250, &[(0, 3999), (3350, 3999), (6150, 0), (7250, 0)]),
        4000,
    );
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0)
            .keyframes()
            .last()
            .unwrap()
            .layer_time()
            .as_millis(),
        4000
    );
}
#[test]
fn playback_p037_freeze_preserves_distinct_owner_clamps() {
    for duration in [1350, 14500] {
        let original = group_document(
            "return input.time.milliseconds;",
            remap(0, 14500, &[(0, 14458), (2250, 14458)]),
            duration,
        );
        let prepared = prepare(&original).unwrap();
        assert_eq!(
            track(&prepared, 0)
                .keyframes()
                .last()
                .unwrap()
                .layer_time()
                .as_millis(),
            duration as i64
        );
    }
    // A linear Group under an explicit ancestor remap uses content_domain,
    // not its standalone affine played window (runtime clamp rule).
    let nested = modify(
        &group_document(
            "return input.time.milliseconds;",
            json!({"type":"windowed","inputRange":{"start":0,"duration":100},"mapping":{"type":"linear","input":{"start":0,"duration":100},"output":{"start":0,"duration":100}},"inputOffsetMs":0}),
            200,
        ),
        |raw| {
            let mut inner = raw["composition"]["layers"][0].clone();
            inner["parent"] = json!(10);
            let transform = inner["transform"].clone();
            raw["composition"]["layers"] = json!([{"type":"Group","id":10,"name":"Remapped parent","playback":remap(0,100,&[(0,50),(100,50)]),"transform":transform,"layers":[inner]}]);
            raw["composition"]["dynamics"]["entries"][0]["target"]["layerId"] = json!(8);
        },
    );
    let baked = prepare(&nested).unwrap();
    assert_eq!(
        track(&baked, 0)
            .keyframes()
            .last()
            .unwrap()
            .layer_time()
            .as_millis(),
        200
    );
}
#[test]
fn playback_same_clock_dependencies_preserve_multiplicity() {
    let original = dependency_document();
    let prepared = prepare(&original).unwrap();
    assert_eq!(
        track(&prepared, 0).keyframes()[0].value(),
        &PropertyValue::Float(100.0)
    );
    assert_eq!(
        track(&prepared, 0).keyframes().last().unwrap().value(),
        &PropertyValue::Float(300.0)
    );
    let seeded = modify(&dependency_document(), |raw| {
        raw["composition"]["dynamics"]["entries"][1]["animator"]["layerTimeJsCode"] =
            json!("return input.randomSeed;");
    });
    let baked = prepare(&seeded).unwrap();
    let dependency = &seeded.composition().dynamics().entries()[1];
    let expected = 2.0 * f64::from(random_seed(seed::prefix(&dependency.target), 0));
    assert_eq!(
        track(&baked, 0).keyframes()[0].value(),
        &PropertyValue::Float(expected)
    );
    for duration in [1350, 14500] {
        let nested = modify(&dependency_document(), |raw| {
            let mut target = raw["composition"]["layers"][0].clone();
            target["activeRange"] = json!({"start":0,"duration":duration});
            target["parent"] = json!(11);
            let transform = target["transform"].clone();
            let identity = |id: u64, parent: u64, layers: Value| json!({"type":"Group","id":id,"name":"Identity wrapper","parent":parent,"playback":{"type":"windowed","inputRange":{"start":0,"duration":duration},"mapping":{"type":"linear","input":{"start":0,"duration":duration},"output":{"start":0,"duration":duration}},"inputOffsetMs":0},"transform":transform,"layers":layers});
            let inner = identity(11, 10, json!([target]));
            let outer = identity(10, 8, json!([inner]));
            raw["composition"]["layers"] = json!([{"type":"Group","id":8,"name":"Frozen ancestor","playback":remap(0,14500,&[(0,14458),(2250,14458)]),"transform":transform,"layers":[outer]}]);
            raw["composition"]["dynamics"]["entries"][0]["animator"]["layerTimeJsCode"] = json!(
                "let s=1; for(const d of input.deps) s*=Math.abs(d.value)/100; return 3/Math.max(s,.001);"
            );
            raw["composition"]["dynamics"]["entries"][0]["dependencies"] = json!([{"kind":"layer","layerId":10,"propertyType":"scaleX"},{"kind":"layer","layerId":11,"propertyType":"scaleX"}]);
            raw["composition"]["dynamics"]["entries"][1]["target"]["layerId"] = json!(10);
            raw["composition"]["dynamics"]["entries"][1]["animator"]["layerTimeJsCode"] =
                json!("return 100;");
            raw["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({"target":{"kind":"layer","layerId":11,"propertyType":"scaleX"},"animator":{"type":"jsScript","layerTimeJsCode":"return 50;"}}));
        });
        let baked = prepare(&nested).unwrap();
        assert_eq!(
            track(&baked, 0).keyframes()[0].value(),
            &PropertyValue::Float(6.0)
        );
        assert!(
            baked.document.composition().dynamics().entries()[0]
                .dependencies
                .is_empty()
        );
    }
}
#[test]
fn playback_dependencies_are_removed_only_from_prepared_copy() {
    let original = dependency_document();
    let prepared = prepare(&original).unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .dependencies
            .is_empty()
    );
    assert_eq!(
        original.composition().dynamics().entries()[0]
            .dependencies
            .len(),
        2
    );
}
#[test]
fn playback_mixed_clock_dependencies_remain_diagnosed() {
    for variant in ["offset", "window", "posterize"] {
        let original = modify(&dependency_document(), |raw| match variant {
            "offset" => raw["composition"]["layers"][1]["activeRange"]["start"] = json!(501),
            "window" => raw["composition"]["layers"][1]["activeRange"]["duration"] = json!(101),
            "posterize" => {
                raw["composition"]["layers"][1]["effects"] = json!([{"id":101,"enabled":true,"effect":{"type":"posterizeTime","frameRate":8.0}}])
            }
            _ => unreachable!(),
        });
        let prepared = prepare(&original).unwrap();
        assert!(
            prepared.document.composition().dynamics().entries()[0]
                .animator
                .is_js_script(),
            "{variant}"
        );
    }
}
#[test]
fn playback_nonfinite_dependency_is_not_a_bounds_guess() {
    let original = modify(&dependency_document(), |raw| {
        raw["composition"]["dynamics"]["entries"][1]["animator"]["layerTimeJsCode"] =
            json!("return NaN;")
    });
    let prepared = prepare(&original).unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
}

#[test]
fn playback_dependency_producers_require_scalar_targets() {
    for property in ["textContent", "fillEnabled", "ellipseSize", "fillColor"] {
        let original = modify(&dependency_document(), |raw| {
            if property == "textContent" {
                let mut text = text_document("return 42;").to_json_value().unwrap()["composition"]
                    ["layers"][0]
                    .clone();
                text["id"] = json!(9);
                text["activeRange"] = json!({"start":500,"duration":100});
                raw["composition"]["layers"][1] = text;
            }
            raw["composition"]["dynamics"]["entries"][0]["dependencies"] =
                json!([{"kind":"layer","layerId":9,"propertyType":property}]);
            raw["composition"]["dynamics"]["entries"][0]["animator"]["layerTimeJsCode"] =
                json!("return input.deps[0].value;");
            raw["composition"]["dynamics"]["entries"][1]["target"]["propertyType"] =
                json!(property);
            raw["composition"]["dynamics"]["entries"][1]["animator"]["layerTimeJsCode"] =
                json!("return 42;");
        });
        let baked = prepare(&original).unwrap();
        assert!(
            baked.document.composition().dynamics().entries()[0]
                .animator
                .is_js_script(),
            "{property}"
        );
        assert_eq!(
            original.composition().dynamics().entries()[0]
                .dependencies
                .len(),
            1
        );
    }
}

#[test]
fn playback_dependency_input_retains_runtime_float_type() {
    let original = modify(&dependency_document(), |raw| {
        raw["composition"]["dynamics"]["entries"][0]["animator"]["layerTimeJsCode"] =
            json!("return input.deps[0].type === 'float' ? input.deps[0].value : -1;");
    });
    let baked = prepare(&original).unwrap();
    assert_eq!(
        track(&baked, 0).keyframes()[0].value(),
        &PropertyValue::Float(50.0)
    );
}

#[test]
fn playback_dependency_closure_and_consumer_share_graph_runtime() {
    let original = modify(&dependency_document(), |raw| {
        raw["composition"]["dynamics"]["entries"][1]["animator"]["layerTimeJsCode"] =
            json!("globalThis.shared = 7; return 1;");
        raw["composition"]["dynamics"]["entries"][0]["animator"]["layerTimeJsCode"] =
            json!("return globalThis.shared || input.deps[0].value;");
    });
    let baked = prepare(&original).unwrap();
    assert_eq!(
        track(&baked, 0).keyframes()[0].value(),
        &PropertyValue::Float(7.0)
    );
    // Dependency slots may be authored in reverse order. Global script order
    // still follows stable topological readiness and serialized entry order.
    let ordered = modify(&original, |raw| {
        let mut producer = raw["composition"]["layers"][1].clone();
        producer["id"] = json!(10);
        raw["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(producer);
        raw["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({"target":{"kind":"layer","layerId":10,"propertyType":"scaleX"},"animator":{"type":"jsScript","layerTimeJsCode":"globalThis.shared = 9; return 2;"}}));
        raw["composition"]["dynamics"]["entries"][0]["dependencies"] = json!([{"kind":"layer","layerId":10,"propertyType":"scaleX"},{"kind":"layer","layerId":9,"propertyType":"scaleX"}]);
    });
    let baked = prepare(&ordered).unwrap();
    assert_eq!(
        track(&baked, 0).keyframes()[0].value(),
        &PropertyValue::Float(9.0)
    );
}

// Supplemental numerical graph oracle: the fixture's serialized A/B/C order
// is already topological. Use the existing VM and playback-shaped JSON input,
// independently of the converter's Program and direct-input builder.
fn review_domain_oracle(document: &EditableFxCompositionDocument, time: u64) -> Vec<f64> {
    let entries = document.composition().dynamics().entries();
    let mut runtime = ScriptRuntime::new().unwrap();
    let mut values = Vec::new();
    for entry in entries {
        let deps = entry
            .dependencies
            .iter()
            .map(|target| {
                let index = entries
                    .iter()
                    .position(|entry| &entry.target == target)
                    .unwrap();
                json!({"type":"float", "value":values[index]})
            })
            .collect::<Vec<_>>();
        let input = JsValue::from_json(&json!({
            "time":{"seconds":time as f64 / 1000.0,"milliseconds":time},
            "randomSeed":random_seed(seed::prefix(entry.random_seed_target.as_ref().unwrap_or(&entry.target)), time),
            "deps":deps,
        }), runtime.context_mut()).unwrap();
        let refs = JsValue::from_json(&json!({}), runtime.context_mut()).unwrap();
        let metadata = JsValue::from_json(&json!({}), runtime.context_mut()).unwrap();
        install_reference_tables(&input, refs, metadata, runtime.context_mut()).unwrap();
        let AnimatorData::JsScript {
            layer_time_js_code: Some(code),
            ..
        } = entry.animator.data()
        else {
            panic!("oracle fixture contains only layer-time scripts");
        };
        values.push(runtime.call(code, input).unwrap().as_number().unwrap());
    }
    values
}

fn review_fanout_document() -> EditableFxCompositionDocument {
    modify(&document("return 0;"), |raw| {
        let target = |property: &str| json!({"kind":"layer","layerId":7,"propertyType":property});
        raw["composition"]["dynamics"]["entries"] = json!([
            {"target":target("positionX"),"animator":{"type":"jsScript","layerTimeJsCode":"globalThis.shared = 1; return 10;"}},
            {"target":target("positionY"),"dependencies":[target("positionX")],"animator":{"type":"jsScript","layerTimeJsCode":"globalThis.shared = 2; return input.deps[0].value + 1;"}},
            {"target":target("opacity"),"dependencies":[target("positionX")],"animator":{"type":"jsScript","layerTimeJsCode":"return (globalThis.shared || 0) + input.deps[0].value;"}}
        ]);
    })
}

#[test]
fn review_script_domain_fanout_matches_full_graph_order() {
    let original = review_fanout_document();
    let baked = prepare(&original).unwrap();
    for time in [0, 50, 100] {
        let expected = review_domain_oracle(&original, time);
        assert_eq!(expected, [10.0, 11.0, 12.0]);
        for (index, value) in expected.into_iter().enumerate() {
            let keys = track(&baked, index).keyframes();
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].value(), &PropertyValue::Float(value));
        }
    }
    assert!(
        baked
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .all(|entry| entry.dependencies.is_empty())
    );
}

#[test]
fn review_script_domain_preserves_sources_seeds_slots_and_call_accounting() {
    let original = modify(&review_fanout_document(), |raw| {
        let entries = &mut raw["composition"]["dynamics"]["entries"];
        entries[0]["randomSeedTarget"] =
            json!({"kind":"effectProperty","effectId":101,"paramName":"blurriness"});
        entries[0]["animator"]["layerTimeJsCode"] = json!("return input.randomSeed;");
        entries[1]["animator"]["layerTimeJsCode"] =
            json!("return input.randomSeed + input.deps[0].value;");
        entries[2]["dependencies"] = json!([
            {"kind":"layer","layerId":7,"propertyType":"positionY"},
            {"kind":"layer","layerId":7,"propertyType":"positionX"},
            {"kind":"layer","layerId":7,"propertyType":"positionY"}
        ]);
        entries[2]["animator"]["layerTimeJsCode"] = json!(
            "return input.randomSeed + 3 * input.deps[0].value + input.deps[1].value + input.deps[2].value;"
        );
    });
    let entries = original.composition().dynamics().entries();
    let owner = Owner {
        id: LayerId::new(7),
        duration_ms: 100,
        start_ms: 0,
        unsupported_clock: false,
        clock_id: 0,
    };
    for time in [0, 50, 100] {
        let expected = review_domain_oracle(&original, time);
        for (index, entry) in entries.iter().enumerate() {
            let program =
                dependencies::Program::new(entry, entries, owner, &|_| Some(owner)).unwrap();
            let AnimatorData::JsScript {
                layer_time_js_code: Some(code),
                ..
            } = entry.animator.data()
            else {
                unreachable!()
            };
            let seed = seed::prefix(entry.random_seed_target.as_ref().unwrap_or(&entry.target));
            let mut runtime = ScriptRuntime::new().unwrap();
            let mut budget = Budget::default();
            assert_eq!(
                program
                    .evaluate(&mut runtime, code, seed, time, &mut budget)
                    .unwrap(),
                expected[index]
            );
            assert_eq!(budget.calls, 3);
        }
    }
}

#[test]
fn review_script_domain_unvalidated_sibling_is_diagnosed() {
    for variant in ["clock", "type", "script"] {
        let original = modify(&review_fanout_document(), |raw| {
            let mut sibling = raw["composition"]["layers"][0].clone();
            sibling["id"] = json!(9);
            raw["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .push(sibling);
            raw["composition"]["dynamics"]["entries"][1]["target"]["layerId"] = json!(9);
            match variant {
                "clock" => raw["composition"]["layers"][1]["activeRange"]["start"] = json!(501),
                "type" => {
                    raw["composition"]["dynamics"]["entries"][1]["target"]["propertyType"] =
                        json!("fillEnabled")
                }
                "script" => {
                    raw["composition"]["dynamics"]["entries"][1]["animator"] =
                        json!({"type":"jsScript","code":"return 11;"})
                }
                _ => unreachable!(),
            }
        });
        let baked = prepare(&original).unwrap();
        assert!(
            baked.document.composition().dynamics().entries()[2]
                .animator
                .is_js_script(),
            "{variant}"
        );
        assert!(
            baked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("was not baked")),
            "{variant}"
        );
    }
}

#[test]
fn playback_fractional_group_endpoints_match_runtime_domain_truncation() {
    for (window_start, window_duration, expected_end) in [(500, 2, 1), (501, 1, 1)] {
        let original = modify(
            &group_document(
                "return input.time.milliseconds;",
                json!({"type":"windowed","inputRange":{"start":window_start,"duration":window_duration},"mapping":{"type":"linear","input":{"start":500,"duration":3},"output":{"start":0,"duration":2}},"inputOffsetMs":0}),
                3,
            ),
            |raw| {
                raw["composition"]["dynamics"]["entries"][0]["target"]["layerId"] = json!(8);
            },
        );
        let baked = prepare(&original).unwrap();
        assert_eq!(track(&baked, 0).keyframes()[0].layer_time().as_millis(), 0);
        assert_eq!(
            track(&baked, 0)
                .keyframes()
                .last()
                .unwrap()
                .layer_time()
                .as_millis(),
            expected_end
        );
    }
}

#[test]
fn playback_remapped_group_input_window_is_not_erased_by_equal_content_windows() {
    let with_consumer = modify(
        &group_document(
            "return input.time.milliseconds;",
            remap(0, 200, &[(0, 0), (200, 200)]),
            200,
        ),
        |raw| {
            let transform = raw["composition"]["layers"][0]["transform"].clone();
            let rect = raw["composition"]["layers"][0]["layers"][0].clone();
            let group = |id: u64, duration: u64| {
                let mut child = rect.clone();
                child["id"] = json!(id + 10);
                child["parent"] = json!(id);
                json!({"type":"Group","id":id,"parent":8,"name":"Different input clamp","transform":transform,"playback":{"type":"windowed","inputRange":{"start":0,"duration":duration},"mapping":{"type":"linear","input":{"start":0,"duration":duration},"output":{"start":0,"duration":duration}},"inputOffsetMs":0},"layers":[child]})
            };
            raw["composition"]["layers"][0]["layers"] = json!([group(10, 100), group(11, 200)]);
            raw["composition"]["dynamics"]["entries"] = json!([
                {"target":{"kind":"layer","layerId":10,"propertyType":"opacity"},"dependencies":[{"kind":"layer","layerId":11,"propertyType":"scaleX"}],"animator":{"type":"jsScript","layerTimeJsCode":"return input.deps[0].value;"}},
                {"target":{"kind":"layer","layerId":11,"propertyType":"scaleX"},"animator":{"type":"jsScript","layerTimeJsCode":"return input.time.milliseconds;"}}
            ]);
        },
    );
    // The equal-content Groups keep distinct input windows, so the consumer and
    // producer clocks are not proven equivalent. Playback evaluates the whole
    // dependency-connected domain, so neither member may be baked alone.
    let baked = prepare(&with_consumer).unwrap();
    for entry in baked.document.composition().dynamics().entries() {
        assert!(entry.animator.is_js_script());
    }
    // Without the cross-clock consumer, the producer bakes over its own full
    // 200 ms window rather than the sibling's 100 ms one.
    let producer_only = modify(&with_consumer, |raw| {
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .remove(0);
    });
    let baked = prepare(&producer_only).unwrap();
    assert_eq!(
        track(&baked, 0)
            .keyframes()
            .last()
            .unwrap()
            .layer_time()
            .as_millis(),
        200
    );
}
