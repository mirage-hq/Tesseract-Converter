//! Native-source regressions for the conversion stages before video comparison.
use super::*;
use crate::properties;
use crate::timing::FrameRate;
use sha2::{Digest, Sha256};

#[test]
fn large_timeline_export_does_not_omit_the_513th_layer() {
    let mut value = imported();
    value["composition"]["layers"] =
        Value::Array((0..513).map(|index| rect(&value, 20_000 + index)).collect());
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value);
    let project = read_project(&output.bytes).unwrap();
    let exported = layers(&project);
    assert_eq!(exported.len(), 513);
    for (index, layer) in exported.iter().enumerate() {
        assert_eq!(
            layer.name.as_ref(),
            format!("Current solid {}", 20_000 + index)
        );
    }
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("limit reached"))
    );
}

fn native_solid_document() -> Value {
    let bytes =
        include_bytes!("../../../tests/fixtures/pr4442_native/sources/transform2d_solid.aep");
    assert_eq!(bytes.len(), 85_931);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "4899cd41a07e3a101809d3ccd0ea9c053082b01510f68719a94064283f23281e"
    );
    let source = read_project(bytes).unwrap();
    assert_eq!(source.item(1).unwrap().name, "PR4442_TRANSFORM2D_SOLID");
    let value = to_structural_fx_document(&source, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let occurrence = &value["composition"]["layers"][0]["layers"][0];
    assert_eq!(occurrence["transform"]["anchorPoint"], json!([110.0, 70.0]));
    assert_eq!(occurrence["transform"]["position"], json!([320.0, 180.0]));
    let clock = &occurrence["layers"][0];
    assert_eq!(
        clock["playback"]["mapping"]["property"]["keyframes"][0]["value"],
        json!(0)
    );
    assert_eq!(
        clock["playback"]["mapping"]["property"]["keyframes"][1]["value"],
        json!(2000)
    );
    assert_eq!(clock["layers"][0]["rect"]["size"], json!([220.0, 140.0]));
    value
}

// A mutated native import isolates the serializer boundary; it is not a new
// independently Adobe-authored/rendered timing oracle.
fn short_source_clock() -> GroupLayer {
    let mut value = native_solid_document();
    let clock = &mut value["composition"]["layers"][0]["layers"][0]["layers"][0];
    clock["playback"]["inputRange"]["duration"] = json!(40);
    clock["playback"]["mapping"]["property"]["keyframes"][1]["time"] = json!(40);
    clock["playback"]["mapping"]["property"]["keyframes"][1]["value"] = json!(40);
    serde_json::from_value(clock.clone()).unwrap()
}

#[test]
fn finite_identity_group_does_not_materialize_unbounded_child_lifetimes() {
    let group = short_source_clock();
    assert_eq!(
        group.layers[0].active_range().duration.as_millis(),
        1_000_000_000_000
    );
    let original = serde_json::to_value(&group).unwrap();
    let domains = group_clock_domains(&group).unwrap();
    assert_eq!(domains.lifetime_domain.duration.as_millis(), 40);
    let clock = hierarchy_clock::plan(&group, &[], domains, false).unwrap();
    assert_eq!(clock.source_duration_millis, 40);
    assert!(!clock.occurrence_clock.has_time_remap());
    for time in [0, 1, 20, 39, 40] {
        assert_eq!(
            clock.occurrence_clock.source_time_millis(time).unwrap(),
            time
        );
    }
    assert_eq!(serde_json::to_value(&group).unwrap(), original);
    assert_eq!(
        clock.geometry().layers[0].active_range(),
        group.layers[0].active_range()
    );
}

#[test]
fn finite_identity_group_exports_and_reimports_its_painted_child() {
    let mut document = native_solid_document();
    document["composition"]["layers"][0]["layers"][0]["layers"][0] =
        serde_json::to_value(LayerData::Group(short_source_clock())).unwrap();
    let output = export(document);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let reopened = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn painted_rects(layers: &[Value]) -> usize {
        layers
            .iter()
            .map(|layer| {
                usize::from(
                    layer["type"] == "Rect"
                        && layer["rect"]["size"] == json!([220.0, 140.0])
                        && layer["rect"]["fillColor"]
                            .as_array()
                            .is_some_and(|color| color[3] == 1.0),
                ) + layer["layers"]
                    .as_array()
                    .map_or(0, |children| painted_rects(children))
            })
            .sum()
    }
    assert_eq!(
        painted_rects(reopened["composition"]["layers"].as_array().unwrap()),
        1
    );
}

#[test]
fn nonidentity_or_partial_group_clocks_keep_the_explicit_child_domain() {
    for (time, value) in [(40, 80), (20, 20)] {
        let mut json = serde_json::to_value(short_source_clock()).unwrap();
        json["playback"]["mapping"]["property"]["keyframes"][1]["time"] = json!(time);
        json["playback"]["mapping"]["property"]["keyframes"][1]["value"] = json!(value);
        let group: GroupLayer = serde_json::from_value(json).unwrap();
        let domains = group_clock_domains(&group).unwrap();
        assert_eq!(
            domains.lifetime_domain.duration.as_millis(),
            1_000_000_000_000
        );
        assert!(hierarchy_clock::plan(&group, &[], domains, false).is_err());
    }
}

/// A native-derived clock Group like an imported offset occurrence: two linear
/// keys exactly on its active interval `[250, 1400)` ms, mapping 650 -> 1800 ms
/// of a bounded 2 s source.
fn offset_occurrence_clock() -> Value {
    let mut json = serde_json::to_value(short_source_clock()).unwrap();
    json["playback"]["inputRange"] = json!({"start": 250, "duration": 1150});
    for (key, (time, value)) in [(250, 650), (1400, 1800)].into_iter().enumerate() {
        json["playback"]["mapping"]["property"]["keyframes"][key]["time"] = json!(time);
        json["playback"]["mapping"]["property"]["keyframes"][key]["value"] = json!(value);
    }
    json["layers"][0]["activeRange"] = json!({"start": 0, "duration": 2000});
    json
}

fn fraction(value: crate::schema::layer_records::NativeRational) -> (i32, u32) {
    (value.numerator, value.denominator)
}

#[test]
fn two_key_linear_group_clock_is_an_affine_record_while_its_transform_is_static() {
    let group: GroupLayer = serde_json::from_value(offset_occurrence_clock()).unwrap();
    let domains = group_clock_domains(&group).unwrap();
    let clock = hierarchy_clock::plan(&group, &[], domains, false)
        .unwrap()
        .occurrence_clock;
    assert!(!clock.has_time_remap());
    let record = clock.record;
    assert_eq!(
        [
            record.start_time,
            record.in_point,
            record.out_point,
            record.stretch
        ]
        .map(fraction),
        [(-2, 5), (13, 20), (9, 5), (1, 1)]
    );
    for (occurrence, source) in [(0, 650), (575, 1_225), (1_150, 1_800)] {
        assert_eq!(clock.source_time_millis(occurrence).unwrap(), source);
    }

    // FX evaluates Group-owned keys on the remapped content clock; the record
    // would place them on the parent clock. Any keyed Transform channel,
    // 3D included, is rejected; audio renders no transform and is unchanged.
    for property in [PropType::PositionX, PropType::RotationX] {
        let keyed = [keyed_entry(
            group.id,
            property,
            [
                (0, PropertyValue::Float(0.0)),
                (500, PropertyValue::Float(9.0)),
            ],
        )];
        assert!(
            hierarchy_clock::plan(&group, &keyed, domains, false).is_err(),
            "{property:?}"
        );
        assert!(
            hierarchy_clock::plan(&group, &keyed, domains, true).is_ok(),
            "{property:?}"
        );
    }

    // A non-Linear arrival, a reversed source or a source end past the
    // explicit domain is not this affine map and stays unrepresented.
    type ClockEdit = fn(&mut Value);
    let unrepresented: [(&str, ClockEdit); 3] = [
        ("Hold arrival", |json| {
            json["playback"]["mapping"]["property"]["keyframes"][1]["easing"] =
                json!({"type": "hold"});
        }),
        ("reversed source", |json| {
            json["playback"]["mapping"]["property"]["keyframes"][1]["value"] = json!(100);
        }),
        ("source end past the domain", |json| {
            json["playback"]["mapping"]["property"]["keyframes"][1]["value"] = json!(2_001);
        }),
    ];
    for (case, edit) in unrepresented {
        let mut json = offset_occurrence_clock();
        edit(&mut json);
        let group: GroupLayer = serde_json::from_value(json).unwrap();
        let domains = group_clock_domains(&group).unwrap();
        assert!(
            hierarchy_clock::plan(&group, &[], domains, false).is_err(),
            "{case}"
        );
    }
}

struct TimingSource {
    name: &'static str,
    bytes: &'static [u8],
    len: usize,
    sha256: &'static str,
    /// Exact `(start, in, out, stretch)` of the fresh occurrence record.
    exported: [(i32, u32); 4],
}

const TIMING_SOURCES: [TimingSource; 3] = [
    TimingSource {
        name: "timing_precomp_source_range",
        bytes: include_bytes!(
            "../../../tests/fixtures/pr4442_native/sources/timing_precomp_source_range.aep"
        ),
        len: 149_777,
        sha256: "87d30081465e708afc7888ad9ea3963e32c9f3b6bd7ece57780d48c49d2441a2",
        exported: [(-2, 5), (13, 20), (9, 5), (1, 1)],
    },
    TimingSource {
        name: "timing_trim",
        bytes: include_bytes!("../../../tests/fixtures/pr4442_native/sources/timing_trim.aep"),
        len: 149_745,
        sha256: "eb2c946bb63bafcd32433bde32fddc4f7c5f893f520ec92c6c03d6a0a010d80d",
        exported: [(0, 1), (1, 2), (7, 4), (1, 1)],
    },
    TimingSource {
        name: "timing_stretch",
        bytes: include_bytes!("../../../tests/fixtures/pr4442_native/sources/timing_stretch.aep"),
        len: 149_751,
        sha256: "aba7bab225f3ec06d160ebd06f13fe1ff782e1aff78b91bb41d3278dcb61d0fb",
        // Import stores the 1.266667 s source end as 1267 ms, so AE's 3/2
        // stretch returns as 1900/1267: 0.33 ms late at the source end.
        exported: [(1, 10), (0, 1), (1_267, 1_000), (1_900, 1_267)],
    },
];

fn seconds((numerator, denominator): (i32, u32)) -> f64 {
    f64::from(numerator) / f64::from(denominator)
}

fn clock_fields(record: &crate::schema::layer_records::LayerRecord) -> [(i32, u32); 4] {
    [
        record.start_time_fraction(),
        record.in_point_fraction(),
        record.out_point_fraction(),
        record.stretch_fraction(),
    ]
}

/// Independently AE-authored offset/trimmed, trimmed and 150%-stretched
/// precomposition occurrences. Import stores each clock as two linear keys on
/// its active interval; export preserves the corresponding affine clock.
#[test]
fn native_offset_trim_and_stretch_occurrences_export_their_affine_clocks() {
    for source in &TIMING_SOURCES {
        assert_eq!(source.bytes.len(), source.len, "{}", source.name);
        assert_eq!(
            format!("{:x}", Sha256::digest(source.bytes)),
            source.sha256,
            "{}",
            source.name
        );
        let native = read_project(source.bytes).unwrap();
        let native_occurrence = layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == source.name)
            .unwrap();
        let native_fields = clock_fields(&native_occurrence.record);

        let imported = to_structural_fx_document(&native, Some(1)).unwrap();
        let output = to_aep(&imported.document).unwrap();
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
            "{}: {:?}",
            source.name,
            output.diagnostics
        );
        let exported = read_project(&output.bytes).unwrap();
        let composition = |id: u32| match &exported.item(id).unwrap().kind {
            ItemKind::Composition(composition) => composition,
            _ => panic!("{}: composition {id}", source.name),
        };
        // The AE layer (its marker is animated) is a precomposition whose
        // source holds the clocked occurrence of the nested composition.
        let wrapper = layers(&exported)
            .iter()
            .find(|layer| layer.name.as_ref() == source.name)
            .unwrap();
        let [occurrence] = &composition(wrapper.record.source_id()).layers[..] else {
            panic!("{}: one clocked occurrence", source.name);
        };
        let fields = clock_fields(&occurrence.record);
        assert_eq!(fields, source.exported, "{}", source.name);
        // Within the importer's millisecond quantization of Adobe's ticks.
        for (field, (ours, adobe)) in ["start", "in", "out"]
            .into_iter()
            .zip(fields.into_iter().zip(native_fields))
        {
            assert!(
                (seconds(ours) - seconds(adobe)).abs() < 0.000_5,
                "{} {field}: {ours:?} vs {adobe:?}",
                source.name
            );
        }
        let stretch = seconds(fields[3]) / seconds(native_fields[3]);
        assert!((stretch - 1.0).abs() < 0.001, "{}", source.name);
        assert!(
            composition(occurrence.record.source_id())
                .layers
                .iter()
                .any(|layer| layer.name.as_ref() == "MOVING_SOURCE_MARKER"),
            "{}",
            source.name
        );
    }
}

fn rect_sizes(chunks: &[crate::rifx::Chunk], sizes: &mut Vec<properties::NumericProperty>) {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == "ADBE Vector Rect Size" {
                let storage = properties::unique_list(run, *b"tdbs").unwrap();
                sizes.push(properties::read_numeric(storage).unwrap());
            }
        }
    }
    for chunk in chunks {
        if let Some(children) = chunk.children() {
            rect_sizes(children, sizes);
        }
    }
}

#[test]
fn native_identity_source_clocks_export_edited_content_without_emptying_the_group() {
    let mut value = native_solid_document();
    value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0]["rect"]["size"] =
        json!([230.0, 150.0]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let mut sizes = Vec::new();
    rect_sizes(&layers(&native)[0].content, &mut sizes);
    assert!(
        sizes.iter().any(|size| size.values == [230.0, 150.0]),
        "{sizes:?}"
    );
    assert!(
        sizes.iter().any(|size| size.values == [640.0, 360.0]),
        "{sizes:?}"
    );
    to_structural_fx_document(&native, Some(1)).unwrap();
}

#[test]
fn native_keyed_rect_uses_existing_hierarchy_export_instead_of_omitting_the_root() {
    let bytes =
        include_bytes!("../../../tests/fixtures/pr4442_native/sources/geometry_rect_size.aep");
    assert_eq!(bytes.len(), 85_483);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "765a6fcd8550999c89f1c2ae19fb40a1118a41fde59132a42f61155c6ebfe398"
    );
    let source = read_project(bytes).unwrap();
    let document = to_structural_fx_document(&source, Some(1))
        .unwrap()
        .document;
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let mut sizes = Vec::new();
    for item in &native.items {
        if let ItemKind::Composition(comp) = &item.kind {
            for layer in &comp.layers {
                rect_sizes(&layer.content, &mut sizes);
            }
        }
    }
    let keyed = sizes
        .iter()
        .find(|size| size.animated)
        .unwrap_or_else(|| panic!("missing native Size keys: {:?}", output.diagnostics));
    assert_eq!(keyed.keyframes.len(), 2);
    assert_eq!(keyed.keyframes[0].time_secs, 0.25);
    assert_eq!(keyed.keyframes[0].values, [120.0, 80.0]);
    assert_eq!(keyed.keyframes[1].time_secs, 1.5);
    assert_eq!(keyed.keyframes[1].values, [280.0, 180.0]);
    // The background must survive, but may remain actual Solid footage when
    // the hierarchy path preserves native occurrences rather than inlining
    // everything into one vector layer. Check the referenced source's complete
    // independent native descriptor, not an orphan item or just layer count.
    let expected_background = source
        .item(14)
        .unwrap()
        .solid
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    let referenced_background = native.items.iter().any(|item| {
        let ItemKind::Composition(comp) = &item.kind else {
            return false;
        };
        comp.layers.iter().any(|layer| {
            native.item(layer.record.source_id()).is_some_and(
                |source| matches!(&source.solid, Some(Ok(solid)) if solid == expected_background),
            )
        })
    });
    assert!(sizes.iter().any(|size| size.values == [640.0, 360.0]) || referenced_background);
}

#[test]
fn native_layer_only_blends_do_not_abort_export_through_vector_paint_encoding() {
    let bytes =
        include_bytes!("../../../tests/fixtures/compositing/import_remaining_blend_modes.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "537c9e21cff85ea10f39dcd9a2c7b3038987495b5b1dfac8ffa60267d5b732d6"
    );
    let source = read_project(bytes).unwrap();
    for id in [194, 178, 98, 402, 386] {
        let document = to_structural_fx_document(&source, Some(id))
            .unwrap()
            .document;
        let output = to_aep(&document).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let ItemKind::Composition(source_comp) = &source.item(id).unwrap().kind else {
            panic!("native composition {id}");
        };
        let blends: Vec<_> = native
            .items
            .iter()
            .filter_map(|item| {
                if let ItemKind::Composition(comp) = &item.kind {
                    Some(comp)
                } else {
                    None
                }
            })
            .flat_map(|comp| comp.layers.iter().map(|layer| layer.record.blend_mode()))
            .collect();
        for layer in &source_comp.layers {
            assert!(
                blends.contains(&layer.record.blend_mode()),
                "comp {id}: {blends:?}, {:?}",
                output.diagnostics
            );
        }
        to_structural_fx_document(&native, Some(1)).unwrap();
    }
}

#[test]
fn native_point_text_survives_single_child_transform_wrappers() {
    let bytes =
        include_bytes!("../../../tests/fixtures/pr4442_native/sources/text_document_point.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "bc32cae3896c3aaf2e7a02f5283ce9ef31c9b0ef53f8c8a30a2b6c30b69eea34"
    );
    let source = read_project(bytes).unwrap();
    let document = to_structural_fx_document(&source, Some(1))
        .unwrap()
        .document;
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let reopened = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document;
    fn texts(layers: &[Layer], output: &mut Vec<String>) {
        for layer in layers {
            match layer.data() {
                LayerData::Text(text) => output.push(text.source_text.text.clone()),
                LayerData::Group(group) => texts(&group.layers, output),
                _ => {}
            }
        }
    }
    let mut content = Vec::new();
    texts(reopened.composition().layers(), &mut content);
    assert_eq!(
        content,
        ["Editable AEP\nPR 4442"],
        "{:?}",
        output.diagnostics
    );
    let timeline = layers(&native);
    let text = timeline
        .iter()
        .find(|layer| layer.record.layer_type() == 3)
        .unwrap();
    let clock = timeline
        .iter()
        .find(|layer| layer.record.id() == text.record.parent_id())
        .unwrap();
    let occurrence = timeline
        .iter()
        .find(|layer| layer.record.id() == clock.record.parent_id())
        .unwrap();
    assert_eq!(clock.name.as_ref(), "Source content clock");
    assert_eq!(occurrence.name.as_ref(), "text_document_point");
    assert_ne!(occurrence.record.parent_id(), 0);
}

#[test]
fn hierarchy_parenting_never_discards_group_opacity_keys() {
    let value = native_solid_document();
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("root group");
    };
    let dynamics = [keyed_entry(
        group.id,
        PropType::Opacity,
        [
            (0, PropertyValue::Float(100.0)),
            (500, PropertyValue::Float(50.0)),
        ],
    )];
    let plan = hierarchy::classify(
        group,
        group.playback.input_range().end(),
        FrameRate::new(24.0)
            .unwrap()
            .duration(group.playback.input_range().end().as_millis())
            .unwrap()
            .1,
        &dynamics,
        &BTreeMap::new(),
        document.dimensions(),
    );
    assert!(
        !matches!(plan, Ok(hierarchy::HierarchyPlan::Parent(_))),
        "Null parenting does not inherit Group opacity"
    );
}

#[test]
fn structural_parent_identity_group_exports_same_rect_as_isolated() {
    let mut value = imported();
    let leaf = rect(&value, 71_001);
    value["composition"]["dynamics"] = json!({"entries": []});
    value["composition"]["layers"] = json!([leaf.clone()]);
    let isolated = export(value.clone());
    let isolated_native = read_project(&isolated.bytes).unwrap();
    assert_eq!(
        layers(&isolated_native).len(),
        1,
        "{:?}",
        isolated.diagnostics
    );

    let mut group = imported()["composition"]["layers"][0].clone();
    group["layers"] = json!([leaf]);
    value["composition"]["layers"] = json!([group]);
    let nested = export(value);
    let nested_native = read_project(&nested.bytes).unwrap();
    assert_eq!(layers(&nested_native).len(), 1, "{:?}", nested.diagnostics);
    // The isolated Rect uses native Solid footage; the inline Group uses
    // native vector Rectangle geometry. Compare their actual dimensions, not
    // a vector-only record which cannot exist on the Solid representation.
    let solid = isolated_native
        .item(layers(&isolated_native)[0].record.source_id())
        .unwrap()
        .solid
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    let mut nested_sizes = Vec::new();
    rect_sizes(&layers(&nested_native)[0].content, &mut nested_sizes);
    assert_eq!(nested_sizes.len(), 1, "{:?}", nested.diagnostics);
    assert_eq!(
        nested_sizes[0].values,
        [f64::from(solid.width), f64::from(solid.height)]
    );
    assert!(nested_sizes[0].keyframes.is_empty());
}

#[test]
fn structural_parent_overrides_foreign_explicit_parent_in_transformed_group() {
    let mut value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    group["transform"]["position"] = json!([20.0, -10.0]);
    let mut child = rect(&value, 71_002);
    child["parent"] = json!(71_003);
    // A different child clock prevents the vector-inline path: the hierarchy
    // must export the actual child under the structural Group parent.
    child["activeRange"] = json!({"start": 250, "duration": 1000});
    group["layers"] = json!([child]);
    let sibling = rect(&value, 71_003);
    value["composition"]["layers"] = json!([group, sibling]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let timeline = layers(&native);
    let child = timeline
        .iter()
        .find(|layer| layer.name.as_ref() == "Current solid 71002")
        .unwrap_or_else(|| panic!("missing nested Rect: {:?}", output.diagnostics));
    let group = timeline
        .iter()
        .find(|layer| layer.record.id() == child.record.parent_id())
        .expect("nested Rect keeps its structural parent in the native timeline");
    assert_ne!(group.record.id(), 0);
    let transform = properties::read_transform(&group.content).unwrap();
    assert_eq!(
        transform
            .iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values,
        [20.0, -10.0, 0.0]
    );
    assert!(
        timeline
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 71003")
    );
    assert!(!output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(71_002))
            && diagnostic
                .message
                .contains("Non-containment transform parent")
    }));
}

#[test]
fn root_explicit_foreign_parent_is_rejected_without_dropping_sibling() {
    let mut value = imported();
    let mut rejected = rect(&value, 71_004);
    rejected["parent"] = json!(71_005);
    value["composition"]["layers"] = json!([rejected, rect(&value, 71_005)]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 71005");
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(71_004))
            && diagnostic
                .message
                .contains("Non-containment transform parent")
    }));
}

#[test]
fn native_vector_clock_normalization_rejects_changed_clock_or_child_visibility() {
    for (field, replacement) in [
        ("playback", json!(1000)),
        ("activeRange", json!({"start":100,"duration":1900})),
        ("activeRange", json!({"start":0,"duration":1000})),
    ] {
        let mut value = native_solid_document();
        let clock = &mut value["composition"]["layers"][0]["layers"][0]["layers"][0];
        if field == "playback" {
            // Keep the native keyframe representation, but change its clock.
            clock["playback"]["mapping"]["property"]["keyframes"][1]["value"] = replacement;
        } else {
            clock["layers"][0][field] = replacement;
        }
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let LayerData::Group(root) = document.composition().layers()[0].data() else {
            panic!("native root Group");
        };
        assert!(vector_group_program(root, &root.transform, &[], 0, &mut Vec::new()).is_err());
    }
}
