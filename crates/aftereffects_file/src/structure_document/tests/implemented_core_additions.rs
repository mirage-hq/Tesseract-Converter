//! Opt-in structural assertions for implemented core import mappings.
//!
//! The native projects and Adobe 30 fps references are pinned in
//! `tests/fixtures/aep_video_references.json`. Expected values in this module
//! come either from their independent authoring/readback contract or directly
//! from the typed native source fields and the documented native-to-FX clock
//! and asset-identity contracts. These tests were added without being run and
//! do not claim render fidelity.

use std::collections::BTreeSet;

use crate::{
    media::MediaKind,
    properties::{NumericProperty, read_numeric, runs, unique_list},
    rifx::Chunk,
};

use super::*;

struct PinnedSource {
    label: &'static str,
    bytes: &'static [u8],
    byte_len: usize,
    sha256: &'static str,
}

macro_rules! pinned_source {
    ($name:ident, $path:literal, $data:expr, $len:literal, $sha:literal) => {
        const $name: PinnedSource = PinnedSource {
            label: $path,
            bytes: $data,
            byte_len: $len,
            sha256: $sha,
        };
    };
}

pinned_source!(
    TIMING_CONTROLS,
    "layers/import_timing_controls.aep",
    include_bytes!("../../../tests/fixtures/layers/import_timing_controls.aep"),
    396_537,
    "4760111efc7a5b428e7a534bd66b368c73026614fbefb72660f40edd50048cee"
);
pinned_source!(
    TEMPORAL_CLOCKS,
    "properties/import_temporal_clock_cases.aep",
    include_bytes!("../../../tests/fixtures/properties/import_temporal_clock_cases.aep"),
    332_969,
    "d80216eacadb2ba319ea6434184ef6253f3337317e03d67720137edf92ac9cf9"
);
pinned_source!(
    VISIBLE_REVERSE_CLOCK,
    "pr4442_native/sources/timing_reverse_v2.aep",
    include_bytes!("../../../tests/fixtures/pr4442_native/sources/timing_reverse_v2.aep"),
    153_035,
    "4a011ce75fcd5a99f6c1b87436387ce6a0addb4ff09d3b4b75f163f00b9d2a43"
);
pinned_source!(
    ESSENTIAL_NESTED,
    "essential/import_nested_override_precedence.aep",
    include_bytes!("../../../tests/fixtures/essential/import_nested_override_precedence.aep"),
    214_983,
    "abc4dbd7c1d1efd72ee8d898e4edd40c553a980e582e13b6344acfd03cfa7fbd"
);
pinned_source!(
    IMAGE_SOURCES,
    "media/import_image_source_controls.aep",
    include_bytes!("../../../tests/fixtures/media/import_image_source_controls.aep"),
    233_425,
    "450f242e0287689989a5b99f2dc67830b43bb2a787dd627f75943b14397e8c04"
);
pinned_source!(
    IMPLEMENTED_CLOCKS,
    "implemented_additions/native_clock_and_3d_controls.aep",
    include_bytes!(
        "../../../tests/fixtures/implemented_additions/native_clock_and_3d_controls.aep"
    ),
    627_789,
    "d9924424998759ca8abed0eb75744dcfafa16e0dc530c007bbfe1ffe3d453c55"
);
pinned_source!(
    IMPLEMENTED_EDITS,
    "implemented_additions/native_export_edit_controls.aep",
    include_bytes!("../../../tests/fixtures/implemented_additions/native_export_edit_controls.aep"),
    527_923,
    "c961d610151a74279f85b3fb54583c90379e870a375e01b65ad5ec0e2b5eb7ee"
);
pinned_source!(
    IMPLEMENTED_RANGES,
    "implemented_additions/native_layer_ranges_and_order.aep",
    include_bytes!(
        "../../../tests/fixtures/implemented_additions/native_layer_ranges_and_order.aep"
    ),
    225_273,
    "6d7506c4f465690ff9e98341a2d08b3e7ada946f9cd32e976f9e8cc1e65d12bb"
);
pinned_source!(
    IMPLEMENTED_TRANSFORM_KEYS,
    "implemented_additions/native_transform_keys.aep",
    include_bytes!("../../../tests/fixtures/implemented_additions/native_transform_keys.aep"),
    462_845,
    "09674f30f1d4544885f355a210c89bb51c05c4cbb1e875fa545c955f1097d43e"
);

fn pinned_project(source: &PinnedSource) -> StructuralProject {
    assert_eq!(
        source.bytes.len(),
        source.byte_len,
        "{} bytes",
        source.label
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(source.bytes)),
        source.sha256,
        "{} SHA-256",
        source.label
    );
    read_project(source.bytes).unwrap_or_else(|error| panic!("{}: {error}", source.label))
}

fn fresh_import(
    project: &StructuralProject,
    composition_id: u32,
    composition_name: &str,
    with_assets: bool,
) -> StructuralConversion {
    let source = composition(project, composition_id);
    assert_eq!(project.item(composition_id).unwrap().name, composition_name);
    assert_eq!(source.frame_rate, 24.0, "{composition_name} source fps");
    let converted = if with_assets {
        to_structural_fx_document_with_assets(project, Some(composition_id), &mut |_| true).unwrap()
    } else {
        to_structural_fx_document(project, Some(composition_id)).unwrap()
    };
    assert_imported_canvas_matches_source(source, &converted, composition_name);
    assert_eq!(root(&converted).name, composition_name);
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
    converted
}

fn all_groups(group: &GroupLayer) -> Vec<&GroupLayer> {
    fn visit<'a>(group: &'a GroupLayer, output: &mut Vec<&'a GroupLayer>) {
        output.push(group);
        for layer in &group.layers {
            if let FxLayer::Group(child) = layer.data() {
                visit(child, output);
            }
        }
    }

    let mut output = Vec::new();
    visit(group, &mut output);
    output
}

fn named_group<'a>(group: &'a GroupLayer, name: &str) -> &'a GroupLayer {
    all_groups(group)
        .into_iter()
        .find(|candidate| candidate.name == name)
        .unwrap_or_else(|| panic!("missing editable Group {name:?}"))
}

fn source_clock(group: &GroupLayer) -> &GroupLayer {
    group
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Group(child) if child.name == "Source content clock" => Some(child),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{} has no source-content clock", group.name))
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_timing_controls_follow_exact_layer_record_clock_contract() {
    // Independent oracle: aep-author-batch2.jsx/status. The source record stores
    // layer-relative in/out values; parent time is start + local * stretch.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, composition_name) in [
        (1, "TIMING_START"),
        (18, "TIMING_IN"),
        (34, "TIMING_OUT"),
        (50, "TIMING_STRETCH"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/layers/import_timing_controls.aep",
            composition_id,
            || {
                let project = pinned_project(&TIMING_CONTROLS);
                let source = composition(&project, composition_id);
                let native = source
                    .layers
                    .iter()
                    .find(|layer| layer.name.as_ref() == "timed_target")
                    .unwrap_or_else(|| panic!("{composition_name} missing native timed_target"));
                let start = native
                    .record
                    .start_time()
                    .expect("authored start denominator");
                let input = native
                    .record
                    .in_point()
                    .expect("authored in-point denominator");
                let output = native
                    .record
                    .out_point()
                    .expect("authored out-point denominator");
                let stretch = native
                    .record
                    .stretch()
                    .expect("authored stretch denominator");
                assert_ne!(stretch, 0.0, "{composition_name} authored stretch");
                let parent_in = start + input * stretch;
                let parent_out = start + output * stretch;
                assert!(
                    parent_in < parent_out,
                    "{composition_name} authored parent range"
                );
                let visible_in = parent_in.max(0.0);
                let source_in = (visible_in - start) / stretch;
                let source_out = (parent_out - start) / stretch;
                assert!(
                    source_in >= 0.0 && source_out >= 0.0,
                    "{composition_name} authored source range"
                );

                let converted = fresh_import(&project, composition_id, composition_name, false);
                let occurrence = named_group(root(&converted), "timed_target");
                let clock = source_clock(occurrence);
                assert_eq!(
                    clock.playback.input_range().start,
                    Time::from_secs(visible_in)
                );
                assert_eq!(
                    clock.playback.input_range().end(),
                    Time::from_secs(parent_out)
                );
                let playback = clock
                    .playback
                    .time_remap()
                    .expect("authored affine source clock");
                let keys = playback.keyframes();
                assert_eq!(keys.len(), 2, "{composition_name} source-clock keys");
                assert_eq!(
                    (keys[0].time, keys[0].value, keys[0].easing),
                    (
                        Time::from_secs(visible_in),
                        Time::from_secs(source_in),
                        PropertyKeyframeEasing::Linear,
                    ),
                    "{composition_name} source-clock start"
                );
                assert_eq!(
                    (keys[1].time, keys[1].value, keys[1].easing),
                    (
                        Time::from_secs(parent_out),
                        Time::from_secs(source_out),
                        PropertyKeyframeEasing::Linear,
                    ),
                    "{composition_name} source-clock end"
                );
                assert!(
                    source_in < source_out,
                    "forward playback must stay ascending"
                );
            },
        );
    }
    cases.finish();
}

fn collect_animated_numeric(chunks: &[Chunk], output: &mut Vec<(String, NumericProperty)>) {
    if let Ok(named_runs) = runs(chunks) {
        for (name, run) in named_runs {
            if let Ok(storage) = unique_list(run, *b"tdbs")
                && let Ok(numeric) = read_numeric(storage)
                && !numeric.keyframes.is_empty()
            {
                output.push((name.to_owned(), numeric));
            }
        }
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        collect_animated_numeric(children, output);
    }
}

fn document_json(converted: &StructuralConversion) -> Value {
    serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_temporal_clock_tracks_use_the_declared_target_clock() {
    // Independent oracle: aep-author-image-gradient-clocks.jsx/readback. Native
    // key times are source-layer-local. Transform/mask targets live on an
    // identity parent clock; intrinsic shape controls remain source-local.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, composition_name, source_local) in [
        (1, "CLOCK_IDENTITY", false),
        (16, "CLOCK_START_STRETCH", false),
        (30, "CLOCK_REVERSE_BEZIER", false),
        (44, "CLOCK_SHAPE_START_STRETCH", true),
        (57, "CLOCK_MASK_START_STRETCH", false),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/properties/import_temporal_clock_cases.aep",
            composition_id,
            || {
                let project = pinned_project(&TEMPORAL_CLOCKS);
                let source = composition(&project, composition_id);
                let mut native_tracks = Vec::new();
                let mut owner = None;
                for layer in &source.layers {
                    let mut tracks = Vec::new();
                    collect_animated_numeric(&layer.content, &mut tracks);
                    if !tracks.is_empty() {
                        assert!(
                            owner.is_none(),
                            "{composition_name} has multiple animated layers"
                        );
                        owner = Some(layer);
                        native_tracks = tracks;
                    }
                }
                let owner =
                    owner.unwrap_or_else(|| panic!("{composition_name} lacks its native control"));
                assert_eq!(
                    native_tracks.len(),
                    1,
                    "{composition_name} must isolate one native animated control"
                );
                let (match_name, native) = &native_tracks[0];
                assert_eq!(native.keyframes.len(), 2, "{composition_name} {match_name}");
                let start = owner.record.start_time().expect("authored clock start");
                let stretch = owner.record.stretch().expect("authored clock stretch");
                assert_ne!(stretch, 0.0, "{composition_name} authored clock stretch");
                let mut expected_times: Vec<_> = native
                    .keyframes
                    .iter()
                    .map(|key| {
                        let seconds = if source_local {
                            key.time_secs
                        } else {
                            start + key.time_secs * stretch
                        };
                        fx_schema::TimeOffset::from_millis_f64(seconds * 1000.0).as_millis()
                    })
                    .collect();
                expected_times.sort_unstable();

                let converted = fresh_import(&project, composition_id, composition_name, false);
                let json = document_json(&converted);
                let entries = json["composition"]["dynamics"]["entries"]
                    .as_array()
                    .unwrap();
                assert!(!entries.is_empty(), "{composition_name} editable dynamics");
                for entry in entries {
                    let keys = entry["animator"]["keyframes"]
                        .as_array()
                        .unwrap_or_else(|| panic!("{composition_name} non-keyframe animator"));
                    let actual_times: Vec<_> = keys
                        .iter()
                        .map(|key| key["layerTime"].as_i64().expect("integer FX milliseconds"))
                        .collect();
                    assert_eq!(
                        actual_times, expected_times,
                        "{composition_name} {match_name}"
                    );
                }

                if composition_name == "CLOCK_REVERSE_BEZIER" {
                    assert!(
                        stretch < 0.0,
                        "reverse case must retain negative native stretch"
                    );
                    assert_eq!(entries.len(), 1, "reverse fixture isolates one FX target");
                    let keys = entries[0]["animator"]["keyframes"].as_array().unwrap();
                    assert_eq!(keys[1]["easing"]["type"], "cubicBezier");
                    let value_scale = match match_name.as_str() {
                        "ADBE Opacity" | "ADBE Scale" => 100.0,
                        _ => 1.0,
                    };
                    assert_eq!(
                        keys[0]["value"]["value"].as_f64(),
                        native.keyframes[1]
                            .values
                            .first()
                            .map(|value| value * value_scale),
                        "reverse clock must reverse native values with native unit mapping"
                    );
                    assert_eq!(
                        keys[1]["value"]["value"].as_f64(),
                        native.keyframes[0]
                            .values
                            .first()
                            .map(|value| value * value_scale),
                        "reverse clock must not silently substitute forward values"
                    );
                }
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_timing_reverse_descending_bounds_are_inactive() {
    let project = pinned_project(&TIMING_CONTROLS);
    let source = composition(&project, 66);
    let native = source
        .layers
        .iter()
        .find(|layer| layer.record.id() == 81)
        .expect("pinned reverse timing layer 81");
    assert_eq!(native.record.start_time_fraction(), (73_728, 24_576));
    assert_eq!(native.record.in_point_fraction(), (9_214_976, 3_072_000));
    assert_eq!(native.record.out_point_fraction(), (-1_024, 3_072_000));
    assert_eq!(native.record.stretch_fraction(), (-1, 1));

    let converted = fresh_import(&project, 66, "TIMING_REVERSE", false);
    let clock = source_clock(named_group(root(&converted), "timed_target"));
    assert!(clock.is_hidden);
    assert_eq!(clock.playback.input_range().start, Time::ZERO);
    assert_eq!(
        clock.playback.input_range().duration,
        Duration::from_secs(3.0)
    );
    assert!(clock.playback.time_remap().is_none());
    assert!(!clock.layers.is_empty(), "editable source content survives");
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.composition_id == Some(66)
            && diagnostic.layer_id == Some(81)
            && diagnostic.message
                == "negative stretch with non-ascending native source bounds is inactive; editable source content retained hidden over its positive parent span"
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn descending_reverse_bounds_retain_hidden_content_and_editable_opacity() {
    let project = pinned_project(&TEMPORAL_CLOCKS);
    let source = composition(&project, 30);
    let native = source
        .layers
        .iter()
        .find(|layer| layer.record.id() == 43)
        .expect("pinned reverse-clock layer 43");
    assert_eq!(native.record.start_time_fraction(), (73_728, 24_576));
    assert_eq!(native.record.in_point_fraction(), (9_214_976, 3_072_000));
    assert_eq!(native.record.out_point_fraction(), (-1_024, 3_072_000));
    assert_eq!(native.record.stretch_fraction(), (-1, 1));

    let converted = fresh_import(&project, 30, "CLOCK_REVERSE_BEZIER", false);
    let occurrence = named_group(root(&converted), "target");
    let clock = source_clock(occurrence);
    assert!(clock.is_hidden);
    assert_eq!(clock.playback.input_range().start, Time::ZERO);
    assert_eq!(
        clock.playback.input_range().duration,
        Duration::from_secs(3.0)
    );
    assert!(clock.playback.time_remap().is_none());
    assert!(!clock.layers.is_empty(), "editable source content survives");

    let json = document_json(&converted);
    let opacity = json["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("editable dynamics")
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == occurrence.id.value()
                && entry["target"]["propertyType"] == "opacity"
        })
        .expect("occurrence opacity track");
    let keys = opacity["animator"]["keyframes"]
        .as_array()
        .expect("opacity keyframes");
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["layerTime"], 1_000);
    assert_eq!(keys[0]["value"]["value"], 90.0);
    assert_eq!(keys[0]["easing"]["type"], "linear");
    assert_eq!(keys[1]["layerTime"], 2_500);
    assert_eq!(keys[1]["value"]["value"], 20.0);
    assert_eq!(
        keys[1]["easing"],
        serde_json::json!({
            "type": "cubicBezier",
            "x1": 0.6,
            "y1": 0.0,
            "x2": 0.4,
            "y2": 1.0,
        })
    );
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.composition_id == Some(30)
            && diagnostic.layer_id == Some(43)
            && diagnostic.message
                == "negative stretch with non-ascending native source bounds is inactive; editable source content retained hidden over its positive parent span"
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ascending_reverse_bounds_keep_visible_reverse_playback_and_siblings() {
    let project = pinned_project(&VISIBLE_REVERSE_CLOCK);
    let source = composition(&project, 1);
    let native = source
        .layers
        .iter()
        .find(|layer| layer.record.id() == 30)
        .expect("pinned visible reverse-clock layer 30");
    assert_eq!(native.record.start_time_fraction(), (98_304, 49_152));
    assert_eq!(native.record.in_point_fraction(), (0, 24_576));
    assert_eq!(native.record.out_point_fraction(), (49_152, 24_576));
    assert_eq!(native.record.stretch_fraction(), (-1, 1));

    let converted = fresh_import(&project, 1, "PR4442_TIMING_REVERSE_V2", false);
    let occurrence = named_group(root(&converted), "timing_reverse_v2");
    let clock = source_clock(occurrence);
    assert!(!clock.is_hidden);
    assert_eq!(clock.playback.input_range().start, Time::ZERO);
    assert_eq!(
        clock.playback.input_range().duration,
        Duration::from_secs(2.0)
    );
    let playback = clock
        .playback
        .time_remap()
        .expect("visible reverse affine source clock");
    let keys = playback.keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(
        (keys[0].time, keys[0].value, keys[0].easing),
        (
            Time::ZERO,
            Time::from_secs(2.0),
            PropertyKeyframeEasing::Linear,
        )
    );
    assert_eq!(
        (keys[1].time, keys[1].value, keys[1].easing),
        (
            Time::from_secs(2.0),
            Time::ZERO,
            PropertyKeyframeEasing::Linear,
        )
    );

    let backdrop = named_group(root(&converted), "SUPPORT_BACKDROP");
    assert!(!backdrop.is_hidden);
    assert!(!source_clock(backdrop).is_hidden);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn reverse_clock_wholly_before_source_zero_retains_hidden_editable_content() {
    let mut project = pinned_project(&TEMPORAL_CLOCKS);
    let layer = composition_mut(&mut project, 30)
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 43)
        .expect("pinned reverse-clock layer 43");
    patch(layer, 12, &0_i32.to_be_bytes());
    patch(layer, 20, &(-3_072_000_i32).to_be_bytes());
    patch(layer, 28, &(-6_144_000_i32).to_be_bytes());

    let converted = fresh_import(&project, 30, "CLOCK_REVERSE_BEZIER", false);
    let clock = source_clock(named_group(root(&converted), "target"));
    assert!(clock.is_hidden);
    assert_eq!(clock.playback.input_range().start, Time::from_secs(1.0));
    assert_eq!(
        clock.playback.input_range().duration,
        Duration::from_secs(1.0)
    );
    assert!(clock.playback.time_remap().is_none());
    assert!(
        !clock.layers.is_empty(),
        "hidden source content stays editable"
    );
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.composition_id == Some(30)
            && diagnostic.layer_id == Some(43)
            && diagnostic.message
                == "negative stretch with non-ascending native source bounds is inactive; editable source content retained hidden over its positive parent span"
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn reverse_clock_oversized_parent_end_falls_back_to_composition_span() {
    let mut project = pinned_project(&TEMPORAL_CLOCKS);
    let layer = composition_mut(&mut project, 30)
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 43)
        .expect("pinned reverse-clock layer 43");
    patch(layer, 12, &1_500_000_000_i32.to_be_bytes());
    patch(layer, 16, &1_i32.to_be_bytes());
    patch(layer, 20, &0_i32.to_be_bytes());
    patch(layer, 28, &(-3_072_000_i32).to_be_bytes());
    assert!(layer.record.start_time().unwrap() > 1_000_000_000.0);
    assert!(layer.record.in_point().unwrap() > layer.record.out_point().unwrap());
    assert!(layer.record.stretch().unwrap() < 0.0);

    let converted = fresh_import(&project, 30, "CLOCK_REVERSE_BEZIER", false);
    let clock = source_clock(named_group(root(&converted), "target"));
    assert!(clock.is_hidden);
    assert_eq!(clock.playback.input_range().start, Time::ZERO);
    assert_eq!(
        clock.playback.input_range().duration,
        root(&converted).playback.input_range().duration
    );
    assert!(clock.playback.time_remap().is_none());
    assert!(!clock.layers.is_empty());
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic.composition_id == Some(30)
            && diagnostic.layer_id == Some(43)
            && diagnostic
                .message
                .contains("unrepresentable or inactive timing")
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_mask_opacity_keeps_normalized_values_parent_clock_and_geometry() {
    let project = pinned_project(&TEMPORAL_CLOCKS);
    let source = composition(&project, 57);
    let native_layer = source
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "target")
        .expect("CLOCK_MASK_START_STRETCH native target");
    let mut native_tracks = Vec::new();
    collect_animated_numeric(&native_layer.content, &mut native_tracks);
    let [(match_name, native)] = native_tracks.as_slice() else {
        panic!("CLOCK_MASK_START_STRETCH must isolate one native numeric track")
    };
    assert_eq!(match_name, "ADBE Mask Opacity");
    assert_eq!(
        native
            .values
            .first()
            .or_else(|| native.keyframes.first().and_then(|key| key.values.first())),
        Some(&0.2)
    );
    assert_eq!(native.keyframes.len(), 2);
    assert_eq!(native.keyframes[0].time_secs, 0.5);
    assert_eq!(native.keyframes[0].values, [0.2]);
    assert_eq!(native.keyframes[1].time_secs, 2.0);
    assert_eq!(native.keyframes[1].values, [0.9]);

    let converted = fresh_import(&project, 57, "CLOCK_MASK_START_STRETCH", false);
    let target = named_group(root(&converted), "target");
    let [mask] = target.masks.as_slice() else {
        panic!("CLOCK_MASK_START_STRETCH must retain one editable mask")
    };
    assert_eq!(mask.opacity.value(), 0.2);

    let clock = source_clock(target);
    assert_eq!(clock.playback.input_range().start, Time::from_millis(250));
    assert_eq!(clock.playback.input_range().end(), Time::from_millis(4_750));
    let source_rect = clock
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Rect(rect) => Some(rect),
            _ => None,
        })
        .expect("CLOCK_MASK_START_STRETCH editable source rectangle");
    assert_eq!(source_rect.rect.size, [480.0, 270.0]);

    let guide_id = mask.layer.expect("mask guide layer");
    let guide = target
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Shape(shape) if shape.id == guide_id => Some(shape),
            _ => None,
        })
        .expect("CLOCK_MASK_START_STRETCH editable mask guide");
    assert!(
        !guide.is_hidden,
        "path-mask resolver requires a visible guide"
    );
    assert!(guide.description.contains("consumed as a mask source"));
    assert!(guide.shape.fills.is_empty());
    assert!(guide.shape.strokes.is_empty());
    let endpoints: Vec<_> = guide
        .shape
        .path
        .commands
        .iter()
        .filter_map(|command| command.endpoint())
        .collect();
    let expected = [
        (40.0, 40.0),
        (440.0, 40.0),
        (440.0, 230.0),
        (40.0, 230.0),
        (40.0, 40.0),
    ];
    assert_eq!(endpoints.len(), expected.len());
    for (actual, expected) in endpoints.into_iter().zip(expected) {
        assert!((actual.0 - expected.0).abs() < 0.0001, "mask x coordinate");
        assert!((actual.1 - expected.1).abs() < 0.0001, "mask y coordinate");
    }

    let json = document_json(&converted);
    let entry = json["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("editable dynamics")
        .iter()
        .find(|entry| entry["target"]["propertyName"] == "opacity")
        .expect("mask opacity dynamics");
    let keys = entry["animator"]["keyframes"]
        .as_array()
        .expect("mask opacity keyframes");
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["layerTime"], 1_000);
    assert_eq!(keys[0]["value"]["value"], 0.2);
    assert_eq!(keys[1]["layerTime"], 3_250);
    assert_eq!(keys[1]["value"]["value"], 0.9);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_nested_essential_overrides_use_outermost_occurrence_precedence() {
    // Independent oracle: aep-author-gradient-nested.jsx/readback authored the
    // source at 100%, its middle occurrence at 55%, and the outer occurrence at
    // 25%. The outer value must replace, not multiply with or lose to, 55%.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, composition_name, expected_non_default) in [
        (1, "NESTED_SOURCE", Vec::<f64>::new()),
        (16, "MIDDLE_OVERRIDE_55", vec![55.0]),
        (29, "OUTER_OVERRIDE_25", vec![25.0]),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/essential/import_nested_override_precedence.aep",
            composition_id,
            || {
                let project = pinned_project(&ESSENTIAL_NESTED);
                let converted =
                    fresh_import(&project, composition_id, composition_name, false);
                let mut actual: Vec<_> = all_groups(root(&converted))
                    .into_iter()
                    .map(|group| group.transform.opacity.value())
                    .filter(|opacity| *opacity != 100.0)
                    .collect();
                actual.sort_by(f64::total_cmp);
                assert_eq!(
                    actual, expected_non_default,
                    "{composition_name} precedence"
                );
                if composition_id == 29 {
                    assert!(
                        !actual.contains(&55.0),
                        "outer 25% must supersede middle 55%"
                    );
                }
            },
        );
    }
    cases.finish();
}

fn native_still_occurrences(
    project: &StructuralProject,
    composition_id: u32,
    stack: &mut Vec<u32>,
    output: &mut Vec<u32>,
) {
    assert!(
        !stack.contains(&composition_id),
        "unexpected cycle in pinned image fixture"
    );
    stack.push(composition_id);
    for layer in &composition(project, composition_id).layers {
        let source_id = layer.record.source_id();
        let Some(source) = project.item(source_id) else {
            continue;
        };
        match &source.kind {
            ItemKind::Composition(_) if layer.record.layer_type() == 0 => {
                native_still_occurrences(project, source_id, stack, output);
            }
            ItemKind::Footage if layer.record.layer_type() == 0 => {
                if source
                    .media
                    .as_ref()
                    .and_then(|descriptor| descriptor.as_ref().ok())
                    .is_some_and(|descriptor| descriptor.kind == MediaKind::StillImage)
                {
                    output.push(source_id);
                }
            }
            _ => {}
        }
    }
    stack.pop();
}

fn editable_rect_in(group: &GroupLayer) -> Option<&fx_schema::RectLayer> {
    group.layers.iter().find_map(|layer| match layer.data() {
        FxLayer::Rect(rect) => Some(rect),
        FxLayer::Group(child) => editable_rect_in(child),
        _ => None,
    })
}

fn assert_exact_float_track(
    converted: &StructuralConversion,
    owner_name: &str,
    property: &str,
    expected_values: [f64; 3],
    expected_easing: &str,
) {
    let owner = named_group(root(converted), owner_name);
    let json = document_json(converted);
    let matching: Vec<_> = json["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("editable dynamics entries")
        .iter()
        .filter(|entry| entry["target"]["propertyType"] == property)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "{owner_name} must own exactly one {property} track"
    );
    let entry = matching[0];
    assert_eq!(
        entry["target"]["layerId"],
        serde_json::to_value(owner.id).expect("serialize expected owner ID"),
        "{owner_name} {property} owner"
    );
    let keys = entry["animator"]["keyframes"]
        .as_array()
        .expect("editable keyframes");
    assert_eq!(keys.len(), 3, "{owner_name} {property} key count");
    for (index, (key, expected_value)) in keys.iter().zip(expected_values).enumerate() {
        assert_eq!(
            key["layerTime"],
            index as u64 * 1_000,
            "{owner_name} {property} key {index} time"
        );
        assert_eq!(
            key["value"],
            serde_json::json!({"type": "float", "value": expected_value}),
            "{owner_name} {property} key {index} value"
        );
        let expected_key_easing = if index == 0 {
            "linear"
        } else {
            expected_easing
        };
        assert_eq!(
            key["easing"]["type"], expected_key_easing,
            "{owner_name} {property} key {index} easing"
        );
    }
}

fn imported_image_asset_ids(group: &GroupLayer, output: &mut Vec<String>) {
    for layer in &group.layers {
        match layer.data() {
            FxLayer::Group(child) => imported_image_asset_ids(child, output),
            FxLayer::Image(image) => output.push(
                image
                    .source
                    .asset()
                    .expect("AE still image must remain asset-backed")
                    .asset_id
                    .as_str()
                    .to_owned(),
            ),
            _ => {}
        }
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_still_images_keep_source_item_identity_per_occurrence() {
    // Independent source/reference inventory pins both authored PNG paths and
    // hashes. The FX contract derives logical identity only from the native
    // project item ID: repeated occurrences share it; distinct items do not.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, composition_name, expected_occurrences, expected_distinct) in [
        (3, "STILL_IMAGE", 1, 1),
        (16, "SHARED_IMAGE_INSTANCES", 2, 1),
        (30, "DISTINCT_IMAGES", 2, 2),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/media/import_image_source_controls.aep",
            composition_id,
            || {
                let project = pinned_project(&IMAGE_SOURCES);
                let mut native_ids = Vec::new();
                native_still_occurrences(
                    &project,
                    composition_id,
                    &mut Vec::new(),
                    &mut native_ids,
                );
                assert_eq!(native_ids.len(), expected_occurrences, "{composition_name}");
                let native_distinct: BTreeSet<_> = native_ids.iter().copied().collect();
                assert_eq!(
                    native_distinct.len(),
                    expected_distinct,
                    "{composition_name}"
                );
                for source_id in &native_distinct {
                    let descriptor = project
                        .item(*source_id)
                        .and_then(|item| item.media.as_ref())
                        .and_then(|descriptor| descriptor.as_ref().ok())
                        .expect("pinned still-image descriptor");
                    assert_eq!(descriptor.kind, MediaKind::StillImage);
                    assert!(descriptor.width > 0 && descriptor.height > 0);
                    assert!(
                        descriptor
                            .authored_path
                            .ends_with("active_bold_weight/frame_0_expected.png")
                            || descriptor
                                .authored_path
                                .ends_with("active_color_persist/frame_0_expected.png"),
                        "unexpected independently pinned image path {:?}",
                        descriptor.authored_path
                    );
                }

                let converted = fresh_import(&project, composition_id, composition_name, true);
                let mut actual_ids = Vec::new();
                imported_image_asset_ids(root(&converted), &mut actual_ids);
                actual_ids.sort();
                let mut expected_ids: Vec<_> = native_ids
                    .iter()
                    .map(|source_id| format!("aep-local-item-{source_id}"))
                    .collect();
                expected_ids.sort();
                assert_eq!(
                    actual_ids, expected_ids,
                    "{composition_name} image identities"
                );

                let requested: BTreeSet<_> = converted
                    .assets
                    .iter()
                    .map(|request| request.logical_id.as_str().to_owned())
                    .collect();
                let expected_distinct_ids: BTreeSet<_> = native_distinct
                    .iter()
                    .map(|source_id| format!("aep-local-item-{source_id}"))
                    .collect();
                assert_eq!(
                    requested, expected_distinct_ids,
                    "{composition_name} asset requests"
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_time_remap_cases_keep_authored_direction_and_interpolation() {
    // Independent oracle: authoring readback removed by 2b6a580d records the
    // exact Adobe layer IDs and three-key controls. These values are not
    // derived from this reader or from converter output.
    let source_path = "crates/aftereffects_file/tests/fixtures/implemented_additions/native_clock_and_3d_controls.aep";
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(source_path, 1, || {
        let project = pinned_project(&IMPLEMENTED_CLOCKS);
        let support = composition(&project, 1);
        assert_eq!(
            project.item(1).unwrap().name,
            "SUPPORT_ANIMATED_CLOCK_SOURCE"
        );
        let support_owner = support
            .layers
            .iter()
            .find(|layer| layer.record.id() == 13)
            .expect("independently authored support layer 13");
        assert_eq!(support_owner.name.as_ref(), "Source moving rectangle");
        let mut support_tracks = Vec::new();
        collect_animated_numeric(&support_owner.content, &mut support_tracks);
        let support_position = support_tracks
            .iter()
            .find_map(|(name, track)| (name == "ADBE Position").then_some(track))
            .expect("support layer's authored Position keys");
        for (key, (time, value)) in support_position.keyframes.iter().zip([
            (0.0, [120.0, 180.0, 0.0]),
            (1.0, [320.0, 180.0, 0.0]),
            (2.0, [520.0, 180.0, 0.0]),
        ]) {
            assert_eq!(
                (key.time_secs, key.values.as_slice()),
                (time, value.as_slice())
            );
        }
        let converted = fresh_import(&project, 1, "SUPPORT_ANIMATED_CLOCK_SOURCE", false);
        assert_exact_float_track(
            &converted,
            "Source moving rectangle",
            "positionX",
            [120.0, 320.0, 520.0],
            "linear",
        );
        assert!(
            editable_rect_in(named_group(root(&converted), "Source moving rectangle")).is_some(),
            "SUPPORT_ANIMATED_CLOCK_SOURCE must retain editable pixels"
        );
    });

    for (composition_id, composition_name, layer_id, expected_values, expected_easing) in [
        (14, "I09_TIME_REMAP_LINEAR", 26, [0.25, 1.5, 2.75], "linear"),
        (
            30,
            "I09_TIME_REMAP_REVERSE",
            42,
            [2.75, 1.5, 0.25],
            "linear",
        ),
        (45, "I09_TIME_REMAP_HOLD", 57, [0.25, 2.75, 1.5], "hold"),
        (
            60,
            "I09_TIME_REMAP_BEZIER",
            72,
            [0.25, 1.5, 2.75],
            "cubicBezier",
        ),
    ] {
        cases.run(source_path, composition_id, || {
            let project = pinned_project(&IMPLEMENTED_CLOCKS);
            let source = composition(&project, composition_id);
            let owner = source
                .layers
                .iter()
                .find(|layer| layer.record.id() == layer_id)
                .unwrap_or_else(|| panic!("{composition_name} missing Adobe layer {layer_id}"));
            assert_eq!(owner.name.as_ref(), "Remapped source", "{composition_name}");
            assert_eq!(
                owner.record.source_id(),
                1,
                "{composition_name} support link"
            );
            let mut native_tracks = Vec::new();
            collect_animated_numeric(&owner.content, &mut native_tracks);
            let native = native_tracks
                .iter()
                .find_map(|(name, track)| (name == "ADBE Time Remapping").then_some(track))
                .unwrap_or_else(|| panic!("{composition_name} missing authored Time Remap"));
            for (index, (key, expected_value)) in
                native.keyframes.iter().zip(expected_values).enumerate()
            {
                assert_eq!(
                    key.time_secs, index as f64,
                    "{composition_name} native time"
                );
                assert_eq!(
                    key.values.as_slice(),
                    [expected_value],
                    "{composition_name} native value"
                );
            }

            let converted = fresh_import(&project, composition_id, composition_name, false);
            let imported_owner = named_group(root(&converted), "Remapped source");
            let remap = all_groups(imported_owner)
                .into_iter()
                .find(|group| group.name == "Authored source remap")
                .unwrap_or_else(|| panic!("{composition_name} missing editable authored remap"));
            let playback = remap
                .playback
                .time_remap()
                .unwrap_or_else(|| panic!("{composition_name} missing editable remap keys"));
            let keys = playback.keyframes();
            assert_eq!(keys.len(), 3, "{composition_name} editable key count");
            for (index, (key, expected_value)) in keys.iter().zip(expected_values).enumerate() {
                assert_eq!(
                    key.time,
                    Time::from_secs(index as f64),
                    "{composition_name} key time"
                );
                assert_eq!(
                    key.value,
                    Time::from_secs(expected_value),
                    "{composition_name} key value"
                );
                let easing = match key.easing {
                    PropertyKeyframeEasing::Hold => "hold",
                    PropertyKeyframeEasing::Linear => "linear",
                    PropertyKeyframeEasing::CubicBezier { .. } => "cubicBezier",
                };
                let expected_key_easing = if index == 0 {
                    "linear"
                } else {
                    expected_easing
                };
                assert_eq!(
                    easing, expected_key_easing,
                    "{composition_name} key {index} easing"
                );
            }
        });
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn unsupported_native_anchor_and_scale_z_are_contextual_and_keep_2d_siblings() {
    // Anchor Z and Scale Z have no current FX destination. Their independently
    // authored static/keyed controls must be diagnosed without dropping the 2D
    // solid sibling or inventing an editable Z target.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, composition_name, control, keyed) in [
        (75, "I05_ANCHOR_Z_STATIC", "Anchor Point", false),
        (88, "I05_ANCHOR_Z_KEYED", "Anchor Point", true),
        (101, "I05_SCALE_Z_STATIC", "Scale", false),
        (114, "I05_SCALE_Z_KEYED", "Scale", true),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_clock_and_3d_controls.aep",
            composition_id,
            || {
                let project = pinned_project(&IMPLEMENTED_CLOCKS);
                let converted =
                    fresh_import(&project, composition_id, composition_name, false);
                assert!(
                    editable_rect_in(root(&converted)).is_some(),
                    "{composition_name} must keep its editable 2D sibling"
                );
                assert!(
                    converted.diagnostics.iter().any(|diagnostic| {
                        diagnostic.composition_id == Some(composition_id)
                            && diagnostic.message.contains(control)
                            && diagnostic.message.contains('Z')
                            && (!keyed || diagnostic.message.contains("animation"))
                    }),
                    "{composition_name} requires a contextual unsupported-Z diagnostic"
                );
                let serialized = document_json(&converted).to_string();
                assert!(
                    !serialized.contains("anchorPointZ") && !serialized.contains("scaleZ"),
                    "{composition_name} must not claim an unsupported editable Z target"
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_transform_key_controls_import_feature_specific_editable_tracks() {
    // Exact values, owner IDs/names and interpolation come from the independent
    // Adobe authoring readback removed by fixture cleanup commit 2b6a580d.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, composition_name, layer_id, owner_name, property, values, easing) in [
        (
            1,
            "E08_ANCHOR_KEYS",
            13,
            "Animated anchor",
            "anchorPointX",
            [10.0, 20.0, 30.0],
            "hold",
        ),
        (
            14,
            "E08_POSITION_KEYS",
            26,
            "Animated position",
            "positionX",
            [100.0, 140.0, 180.0],
            "cubicBezier",
        ),
        (
            27,
            "E08_SCALE_KEYS",
            39,
            "Animated scale",
            "scaleY",
            [100.0, 120.0, 140.0],
            "linear",
        ),
        (
            40,
            "E08_ROTATION_KEYS",
            52,
            "Animated rotation",
            "rotation",
            [0.0, 22.5, 45.0],
            "cubicBezier",
        ),
        (
            53,
            "E08_OPACITY_KEYS",
            65,
            "Animated opacity",
            "opacity",
            [100.0, 65.0, 30.0],
            "linear",
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_transform_keys.aep",
            composition_id,
            || {
                let project = pinned_project(&IMPLEMENTED_TRANSFORM_KEYS);
                let source = composition(&project, composition_id);
                let native_owner = source
                    .layers
                    .iter()
                    .find(|layer| layer.record.id() == layer_id)
                    .unwrap_or_else(|| {
                        panic!("{composition_name} missing Adobe layer {layer_id}")
                    });
                assert_eq!(native_owner.name.as_ref(), owner_name, "{composition_name}");
                let converted =
                    fresh_import(&project, composition_id, composition_name, false);
                assert_exact_float_track(&converted, owner_name, property, values, easing);
                assert!(
                    editable_rect_in(named_group(root(&converted), owner_name)).is_some(),
                    "{composition_name} must retain editable pixels on {owner_name}"
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_export_edit_transform_controls_import_current_values_and_tracks() {
    // Exact values and owner names come from the independent Adobe authoring
    // readback removed by fixture cleanup commit 2b6a580d.
    let source_path = "crates/aftereffects_file/tests/fixtures/implemented_additions/native_export_edit_controls.aep";
    let mut cases = crate::adobe_test_support::CaseBatch::new();

    cases.run(source_path, 16, || {
        let project = pinned_project(&IMPLEMENTED_EDITS);
        let geometry = fresh_import(
            &project,
            16,
            "EDIT_RECT_SIZE_POSITION_ROUNDNESS_EDIT",
            false,
        );
        let geometry_owner = named_group(
            root(&geometry),
            "Isolated RECT_SIZE_POSITION_ROUNDNESS_EDIT",
        );
        let geometry_rect = editable_rect_in(geometry_owner).expect(
            "EDIT_RECT_SIZE_POSITION_ROUNDNESS_EDIT must retain editable Rectangle geometry",
        );
        assert_eq!(geometry_rect.rect.size, [185.0, 95.0]);
        assert_eq!(geometry_rect.rect.position, [0.0, 0.0]);
        assert_eq!(geometry_rect.rect.roundness, 18.0);
        assert_eq!(geometry_rect.transform.anchor_point, [92.5, 47.5]);
        assert_eq!(
            geometry_rect.transform.position,
            fx_composition::Position::TwoD([23.0, -17.0])
        );
    });

    for (composition_id, composition_name, owner_name, stroke, expected_alpha) in [
        (
            42,
            "EDIT_FILL_AND_LAYER_OPACITY",
            "Isolated FILL_AND_LAYER_OPACITY",
            false,
            0.4,
        ),
        (
            55,
            "EDIT_STROKE_AND_LAYER_OPACITY",
            "Isolated STROKE_AND_LAYER_OPACITY",
            true,
            0.45,
        ),
    ] {
        cases.run(source_path, composition_id, || {
            let project = pinned_project(&IMPLEMENTED_EDITS);
            let converted = fresh_import(&project, composition_id, composition_name, false);
            let owner = named_group(root(&converted), owner_name);
            let rect = editable_rect_in(owner)
                .unwrap_or_else(|| panic!("{composition_name} must retain editable paint"));
            assert_eq!(
                owner.transform.opacity.value(),
                65.0,
                "{composition_name} layer opacity"
            );
            let paint_alpha = if stroke {
                rect.rect
                    .stroke_color
                    .as_ref()
                    .expect("authored Stroke color")[3]
            } else {
                rect.rect.fill_color[3]
            };
            assert_eq!(
                paint_alpha, expected_alpha,
                "{composition_name} paint opacity"
            );
        });
    }

    cases.run(source_path, 29, || {
        let project = pinned_project(&IMPLEMENTED_EDITS);
        let signed = fresh_import(&project, 29, "EDIT_SIGNED_NONUNIFORM_SCALE", false);
        assert_eq!(
            named_group(root(&signed), "Isolated SIGNED_NONUNIFORM_SCALE")
                .transform
                .scale,
            [-125.0, 75.0]
        );
    });

    cases.run(source_path, 68, || {
        let project = pinned_project(&IMPLEMENTED_EDITS);
        let combined = fresh_import(&project, 68, "EDIT_COMBINED_2D_TRANSFORM", false);
        let combined_transform =
            named_group(root(&combined), "Isolated COMBINED_2D_TRANSFORM").transform;
        assert_eq!(combined_transform.anchor_point, [20.0, -10.0]);
        assert_eq!(
            combined_transform.position,
            fx_composition::Position::TwoD([290.0, 175.0])
        );
        assert_eq!(combined_transform.scale, [130.0, 75.0]);
        assert_eq!(combined_transform.rotation, 27.0);
        assert_eq!(combined_transform.opacity.value(), 60.0);
    });

    for (composition_id, composition_name, owner_name, property, values, absent_property) in [
        (
            81,
            "EDIT_SEPARATED_POSITION_X_KEYS",
            "Isolated SEPARATED_POSITION_X_KEYS",
            "positionX",
            [160.0, 320.0, 480.0],
            "positionY",
        ),
        (
            94,
            "EDIT_SEPARATED_POSITION_Y_KEYS",
            "Isolated SEPARATED_POSITION_Y_KEYS",
            "positionY",
            [80.0, 180.0, 280.0],
            "positionX",
        ),
    ] {
        cases.run(source_path, composition_id, || {
            let project = pinned_project(&IMPLEMENTED_EDITS);
            let converted = fresh_import(&project, composition_id, composition_name, false);
            assert_exact_float_track(&converted, owner_name, property, values, "linear");
            let serialized = document_json(&converted).to_string();
            assert!(
                !serialized.contains(&format!("\"propertyType\":\"{absent_property}\"")),
                "{composition_name} must not animate the unseparated sibling axis"
            );
        });
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_layer_ranges_and_mixed_order_preserve_authored_structure() {
    // The pinned source isolates one bounded leaf range and one mixed stack.
    // Native layer names/order are the source oracle; conversion must not sort
    // unlike Solid/Rect/Shape/Boolean owners or widen the bounded occurrence.
    let source_path = "crates/aftereffects_file/tests/fixtures/implemented_additions/native_layer_ranges_and_order.aep";
    let mut cases = crate::adobe_test_support::CaseBatch::new();

    cases.run(source_path, 1, || {
        let project = pinned_project(&IMPLEMENTED_RANGES);
        let ranged = fresh_import(&project, 1, "E15_LEAF_IN_OUT_RANGES", false);
        let ranged_groups = all_groups(root(&ranged));
        assert!(
            ranged_groups.iter().any(|group| {
                group.playback.input_range().start > Time::ZERO
                    && group.playback.input_range().end() < Time::from_secs(2.0)
                    && !group.layers.is_empty()
            }),
            "E15_LEAF_IN_OUT_RANGES must retain its authored finite in/out range"
        );
    });

    cases.run(source_path, 17, || {
        let project = pinned_project(&IMPLEMENTED_RANGES);
        let source = composition(&project, 17);
        let expected_names: Vec<_> = source
            .layers
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect();
        assert!(
            expected_names.len() >= 4,
            "E13_MIXED_LAYER_ORDER source stack"
        );
        let mixed = fresh_import(&project, 17, "E13_MIXED_LAYER_ORDER", false);
        let actual_names: Vec<_> = root(&mixed)
            .layers
            .iter()
            .map(|layer| layer.data().name())
            .collect();
        assert_eq!(
            actual_names, expected_names,
            "E13_MIXED_LAYER_ORDER stack order"
        );
        let json = document_json(&mixed).to_string();
        for layer_type in ["Rect", "Shape", "BooleanOperation"] {
            assert!(
                json.contains(&format!("\"type\":\"{layer_type}\"")),
                "E13_MIXED_LAYER_ORDER missing editable {layer_type}"
            );
        }
    });
    cases.finish();
}
