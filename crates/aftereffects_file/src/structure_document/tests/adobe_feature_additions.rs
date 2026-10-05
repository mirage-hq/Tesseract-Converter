//! Supplemental assertions live inside the existing canonical CaseBatch closures.
//! New independent sources below are pending Adobe authoring and are deliberately UNRUN.

use super::*;

pub(super) fn assert_text_animator_owner(
    text: &Value,
    editable: &Value,
    property: &str,
    keyed: bool,
) {
    assert_eq!(text["sourceText"]["text"], "Editable motion\nSecond line");
    let animator = &text["animators"][0];
    assert_eq!(animator["name"], "Animator 1", "named editable animator");
    assert_eq!(
        animator["selectors"]
            .as_array()
            .expect("range selector")
            .len(),
        1
    );
    if keyed {
        let matches: Vec<_> = editable["composition"]["dynamics"]["entries"]
            .as_array()
            .expect("editable dynamics")
            .iter()
            .filter(|entry| {
                entry["target"]["kind"] == "fxItemProperty"
                    && entry["target"]["itemId"] == animator["id"]
                    && entry["target"]["propertyName"] == property
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "keyed channel must target the named animator"
        );
        let keys = matches[0]["animator"]["keyframes"]
            .as_array()
            .expect("keyframes");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["layerTime"], 500);
        assert_eq!(keys[1]["layerTime"], 2000);
    } else {
        assert!(animator[property].is_number() || animator[property].is_array());
    }
}

pub(super) fn assert_mask_key_owner(
    converted: &StructuralConversion,
    property: &str,
    initial: Value,
) {
    fn target(group: &GroupLayer) -> Option<&GroupLayer> {
        if group.name == "target" {
            return Some(group);
        }
        group.layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Group(child) => target(child),
            _ => None,
        })
    }
    let owner = target(root(converted)).expect("named target owner");
    let [mask] = owner.masks.as_slice() else {
        panic!("one editable authored mask")
    };
    let guide = mask.layer.expect("editable static guide identity");
    assert!(
        owner.layers.iter().any(|layer| layer.id() == guide),
        "mask guide remains on target"
    );
    let mask_json = serde_json::to_value(mask).expect("editable mask JSON");
    let editable: Value =
        serde_json::from_slice(&converted.document.to_json_vec().expect("editable JSON"))
            .expect("editable document JSON");
    let matches: Vec<_> = editable["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("editable dynamics")
        .iter()
        .filter(|entry| entry["target"]["propertyName"] == property)
        .collect();
    assert_eq!(matches.len(), 1, "one selected mask control track");
    assert_eq!(
        matches[0]["target"]["itemId"], mask_json["id"],
        "key must target the retained mask"
    );
    let keys = matches[0]["animator"]["keyframes"]
        .as_array()
        .expect("typed mask keys");
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["value"]["value"], initial);
    assert_ne!(
        keys[0]["value"], keys[1]["value"],
        "authored control changes"
    );
}

pub(super) fn assert_audio_gain_and_switch(converted: &StructuralConversion, id: u32) {
    let editable: Value =
        serde_json::from_slice(&converted.document.to_json_vec().expect("editable JSON"))
            .expect("editable document JSON");
    let serialized = editable.to_string();
    assert!(
        serialized.contains("audio-stereo.wav"),
        "original media reference retained"
    );
    if id == 63 {
        let gain: Vec<_> = editable["composition"]["dynamics"]["entries"]
            .as_array()
            .expect("editable dynamics")
            .iter()
            .filter(|entry| entry["target"]["propertyType"] == "audioVolume")
            .collect();
        assert_eq!(gain.len(), 1, "one native gain becomes one editable track");
        assert_eq!(
            gain[0]["animator"]["keyframes"]
                .as_array()
                .expect("gain keys")
                .len(),
            2
        );
    }
    if id == 78 {
        assert!(
            serialized.contains("\"isHidden\":true"),
            "audio-off must remain inaudible"
        );
    }
}

// This draft is a *pending* source pin, not evidence that a native file or reference exists.
// The parent must replace each null identity from an independent Adobe authoring receipt.
fn pending_native(case_id: &str) -> (StructuralProject, u32, Option<u32>) {
    let registry: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/adobe_feature_additions/registry-additions.json"
    ))
    .expect("pending registry JSON");
    let case = registry["new_cases"]
        .as_array()
        .expect("new native cases")
        .iter()
        .find(|case| case["case_id"] == case_id)
        .unwrap_or_else(|| panic!("missing native case {case_id}"));
    let sha = case["source_sha256"]
        .as_str()
        .unwrap_or_else(|| panic!("{case_id}: native SHA pending Adobe authoring receipt"));
    let id = case["composition_id"]
        .as_u64()
        .and_then(|id| u32::try_from(id).ok())
        .unwrap_or_else(|| panic!("{case_id}: composition ID pending Adobe readback"));
    let relative = case["source_path"].as_str().expect("native source path");
    let relative = relative
        .strip_prefix("crates/aftereffects_file/tests/fixtures/")
        .expect("fixture-relative source path");
    let bytes = fs::read(fixture_dir().join(relative)).unwrap_or_else(|error| {
        panic!("{case_id}: independently authored source missing: {error}")
    });
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        sha,
        "{case_id} source pin"
    );
    let project = read_project(&bytes).expect("pinned native AEP must parse");
    assert_eq!(
        project.item(id).expect("native composition item").name,
        case["composition_name"].as_str().expect("composition name")
    );
    let support_id = case["support_composition_id"]
        .as_u64()
        .and_then(|id| u32::try_from(id).ok());
    (project, id, support_id)
}

fn fresh_pending_import(project: &StructuralProject, id: u32, name: &str) -> StructuralConversion {
    let converted = to_structural_fx_document(project, Some(id)).expect("fresh native import");
    assert_imported_canvas_matches_source(composition(project, id), &converted, name);
    assert_eq!(root(&converted).name, name);
    assert!(
        converted
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .all(|entry| !entry.animator.is_js_script()),
        "no generated JavaScript"
    );
    converted
}

fn named_group<'a>(group: &'a GroupLayer, name: &str) -> &'a GroupLayer {
    find_group(group, name).unwrap_or_else(|| panic!("missing editable group {name}"))
}

fn find_group<'a>(group: &'a GroupLayer, name: &str) -> Option<&'a GroupLayer> {
    if group.name == name {
        return Some(group);
    }
    group.layers.iter().find_map(|layer| match layer.data() {
        FxLayer::Group(child) => find_group(child, name),
        _ => None,
    })
}

fn has_editable_rect(group: &GroupLayer) -> bool {
    group.layers.iter().any(|layer| match layer.data() {
        FxLayer::Rect(_) => true,
        FxLayer::Group(child) => has_editable_rect(child),
        _ => false,
    })
}

fn gaussian_blurriness(group: &GroupLayer) -> Vec<Value> {
    group
        .effects
        .iter()
        .filter_map(|record| {
            let value = serde_json::to_value(record).expect("editable effect");
            (value["effect"]["type"] == "gaussianBlur")
                .then(|| value["effect"]["blurriness"].clone())
        })
        .collect()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_nonlinear_remap_retains_four_editable_clock_keys_and_sibling() {
    let case_id = "aep-adobe-feature-additions-native-nonlinear-remap-independent-c1";
    let (project, id, _) = pending_native(case_id);
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run("crates/aftereffects_file/tests/fixtures/adobe_feature_additions/native/nonlinear-remap-independent.aep", id, || {
        let converted = fresh_pending_import(&project, id, "IMPORT_NONLINEAR_REMAP_INDEPENDENT");
        let owner = named_group(root(&converted), "PIECEWISE_REMAP_OWNER");
        let remap = find_group(owner, "Authored source remap").expect("editable remap carrier");
        let playback = remap.playback.time_remap().expect("nonaffine remap must retain editable keys");
        let keys = playback.keyframes();
        assert_eq!(keys.len(), 4, "four independent nonlinear samples, not a flattened clip");
        for (key, (at, value)) in keys.iter().zip([(0.0, 0.0), (0.5, 1.25), (1.5, 0.5), (2.0, 2.0)]) {
            assert_eq!((key.time, key.value, key.easing),
                (Time::from_secs(at), Time::from_secs(value), PropertyKeyframeEasing::Linear));
        }
        assert!(has_editable_rect(named_group(root(&converted), "EDITABLE_SIBLING_RED")));
    });
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_adjustment_omits_cross_layer_pixels_but_keeps_effect_and_sibling() {
    let case_id = "aep-adobe-feature-additions-native-adjustment-effect-sibling-c1";
    let (project, id, _) = pending_native(case_id);
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run("crates/aftereffects_file/tests/fixtures/adobe_feature_additions/native/adjustment-effect-sibling.aep", id, || {
        let source = composition(&project, id);
        let adjustment = source.layers.iter().find(|layer| layer.name.as_ref() == "ADJUSTMENT_BLUR_OWNER")
            .expect("native adjustment owner");
        assert!(adjustment.record.flags().adjustment_layer, "native switch must be set");
        let converted = fresh_pending_import(&project, id, "IMPORT_ADJUSTMENT_GAUSSIAN_SIBLING");
        let owner = named_group(root(&converted), "ADJUSTMENT_BLUR_OWNER");
        assert_eq!(gaussian_blurriness(owner), [serde_json::json!(22.0)]);
        assert!(!has_editable_rect(owner), "do not substitute opaque white pixels for omitted adjustment");
        assert!(has_editable_rect(named_group(root(&converted), "RED_EDITABLE_SIBLING")));
        assert!(!converted.diagnostics.iter().any(|item| item.message.contains("adjustment contribution omitted")
            || item.message.contains("adjustment-layer cross-layer compositing has no existing FX equivalent")),
            "do not falsely diagnose retained Adjustment effects as omitted");
    });
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_nested_effect_stays_on_inner_owner_and_support_composition() {
    let case_id = "aep-adobe-feature-additions-native-nested-effect-stack-c1";
    let (project, id, support_id) = pending_native(case_id);
    let support_id = support_id.expect("support composition ID pending independent Adobe readback");
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    let source_path = "crates/aftereffects_file/tests/fixtures/adobe_feature_additions/native/nested-effect-stack.aep";
    cases.run(source_path, support_id, || {
        let support = fresh_pending_import(&project, support_id, "NESTED_EFFECT_SOURCE");
        assert_eq!(
            gaussian_blurriness(named_group(root(&support), "NESTED_PAINTED_OWNER")),
            [serde_json::json!(17.0)]
        );
    });
    cases.run(source_path, id, || {
        let converted = fresh_pending_import(&project, id, "IMPORT_NESTED_EFFECT_STACK");
        let instance = named_group(root(&converted), "NESTED_EFFECT_INSTANCE");
        assert!(
            gaussian_blurriness(instance).is_empty(),
            "inner effect must not be hoisted"
        );
        assert_eq!(
            gaussian_blurriness(named_group(instance, "NESTED_PAINTED_OWNER")),
            [serde_json::json!(17.0)]
        );
        let sibling = named_group(root(&converted), "BLUE_EDITABLE_SIBLING");
        assert!(
            gaussian_blurriness(sibling).is_empty(),
            "effect must not leak to sibling"
        );
        assert!(has_editable_rect(sibling));
    });
    cases.finish();
}
