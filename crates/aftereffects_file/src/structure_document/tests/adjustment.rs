//! Fresh-import contracts for the independently Adobe-authored Adjustment panel.

use super::*;

const SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/adjustment/native_controls.aep");
const SOURCE_PATH: &str = "crates/aftereffects_file/tests/fixtures/adjustment/native_controls.aep";
const SOURCE_LEN: usize = 1_304_431;
const SOURCE_SHA256: &str = "ed45319ed014ba979a9e0b4868aa635775f288fe851c39d9242aab097020f26f";

fn fresh_import(id: u32, name: &str) -> StructuralConversion {
    let mut converted = None;
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    batch.run(SOURCE_PATH, id, || {
        assert_eq!(
            SOURCE.len(),
            SOURCE_LEN,
            "pinned Adjustment source byte length"
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(SOURCE)),
            SOURCE_SHA256,
            "pinned Adjustment source SHA-256"
        );
        let project = read_project(SOURCE).expect("independently Adobe-authored Adjustment source");
        let source = composition(&project, id);
        assert_eq!(project.item(id).expect("pinned item").name, name);
        assert_eq!((source.width, source.height), (320, 180));
        assert_eq!(source.frame_rate, 24.0);
        assert_eq!(source.duration_secs, 2.0);

        let imported =
            to_structural_fx_document(&project, Some(id)).expect("fresh pinned Adjustment import");
        assert_imported_canvas_matches_source(source, &imported, name);
        assert_eq!(root(&imported).name, name);
        let json = imported
            .document
            .to_json_value()
            .expect("editable Adjustment JSON");
        let wire = json.to_string();
        assert!(!wire.contains("JsScript"), "{name}: no generated JsScript");
        assert!(!wire.contains("jsScript"), "{name}: no generated jsScript");
        converted = Some(imported);
    });
    batch.finish();
    converted.expect("successful Adjustment case callback returns the fresh import")
}

fn direct_names(group: &GroupLayer) -> Vec<&str> {
    group
        .layers
        .iter()
        .filter_map(|layer| {
            let name = layer.data().name();
            (!name.contains(" — Mask ") && !name.ends_with(" (matte source)")).then_some(name)
        })
        .collect()
}

fn assert_source_order(group: &GroupLayer, expected: &[&str]) {
    assert_eq!(
        direct_names(group),
        expected,
        "native top-to-bottom layer order"
    );
}

fn direct_adjustment<'a>(group: &'a GroupLayer, name: &str) -> &'a fx_schema::AdjustmentLayer {
    group
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Adjustment(adjustment) if adjustment.name == name => Some(adjustment),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!("{name}: Adjustment must be a direct sibling, not an empty Group")
        })
}

fn find_adjustment<'a>(
    group: &'a GroupLayer,
    name: &str,
) -> (&'a GroupLayer, &'a fx_schema::AdjustmentLayer) {
    fn visit<'a>(
        group: &'a GroupLayer,
        name: &str,
    ) -> Option<(&'a GroupLayer, &'a fx_schema::AdjustmentLayer)> {
        if let Some(adjustment) = group.layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Adjustment(adjustment) if adjustment.name == name => Some(adjustment),
            _ => None,
        }) {
            return Some((group, adjustment));
        }
        group.layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Group(child) => visit(child, name),
            _ => None,
        })
    }

    visit(group, name).unwrap_or_else(|| {
        panic!("missing direct Adjustment {name:?} in its containing composition")
    })
}

fn effect_json(adjustment: &fx_schema::AdjustmentLayer) -> Vec<Value> {
    serde_json::to_value(&adjustment.effects)
        .expect("editable effects")
        .as_array()
        .expect("effect array")
        .clone()
}

fn assert_gaussian(effect: &Value, blurriness: f64) {
    assert_eq!(effect["enabled"], true);
    assert_eq!(effect["effect"]["type"], "gaussianBlur");
    assert_eq!(effect["effect"]["blurriness"], blurriness);
    assert_eq!(effect["effect"]["repeatEdgePixels"], true);
}

fn assert_levels(effect: &Value, gamma: f64) {
    assert_eq!(effect["enabled"], true);
    assert_eq!(effect["effect"]["type"], "levels");
    for (field, expected) in [
        ("inputBlack", 20.399_999_544_524_3),
        ("inputWhite", 209.099_998_175_096_5),
        ("gamma", gamma),
        ("outputBlack", 10.199_999_772_262_15),
        ("outputWhite", 237.150_001_823_902_8),
    ] {
        let actual = effect["effect"][field]
            .as_f64()
            .unwrap_or_else(|| panic!("missing Levels {field}"));
        assert!(
            (actual - expected).abs() < 0.002,
            "Levels {field}: {actual} != {expected}"
        );
    }
}

fn dynamics(converted: &StructuralConversion) -> Vec<Value> {
    converted
        .document
        .to_json_value()
        .expect("editable dynamics JSON")["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("dynamics entries")
        .clone()
}

fn assert_linear_keys(entry: &Value, expected: &[(i64, f64)]) {
    let keys = entry["animator"]["keyframes"]
        .as_array()
        .expect("keyframes");
    assert_eq!(keys.len(), expected.len(), "no sampled or baked keys");
    for (key, (time, value)) in keys.iter().zip(expected) {
        assert_eq!(key["layerTime"].as_i64(), Some(*time));
        let actual = key["value"]["value"].as_f64().expect("float key value");
        assert!(
            (actual - value).abs() < 0.002,
            "key value {actual} != {value}"
        );
        assert_eq!(key["easing"]["type"], "linear");
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_scope() {
    let converted = fresh_import(1, "adjustment-scope");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "scope-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let adjustment = direct_adjustment(root, "scope-adjustment");
    assert_eq!(adjustment.parent, Some(root.id));
    assert_eq!(adjustment.transform.opacity.value(), 100.0);
    let effects = effect_json(adjustment);
    assert_eq!(effects.len(), 1);
    assert_gaussian(&effects[0], 19.0);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_effect_order() {
    let converted = fresh_import(44, "adjustment-effect-order");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "effect-order-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let effects = effect_json(direct_adjustment(root, "effect-order-adjustment"));
    assert_eq!(effects.len(), 2, "exact native Effect Parade");
    assert_gaussian(&effects[0], 21.0);
    assert_levels(&effects[1], 2.349_999_904_632_57);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_disabled() {
    let converted = fresh_import(84, "adjustment-disabled");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "enabled-empty-adjustment",
            "disabled-adjustment-with-effect",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let empty = direct_adjustment(root, "enabled-empty-adjustment");
    assert!(!empty.is_hidden);
    assert!(
        empty.effects.is_empty(),
        "enabled empty stack remains a no-op gate"
    );
    let disabled = direct_adjustment(root, "disabled-adjustment-with-effect");
    assert!(disabled.is_hidden);
    let effects = effect_json(disabled);
    assert_eq!(effects.len(), 1);
    assert_gaussian(&effects[0], 27.0);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_span() {
    let converted = fresh_import(106, "adjustment-span");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "finite-span-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let adjustment = direct_adjustment(root, "finite-span-adjustment");
    assert_eq!(adjustment.active_range.start.as_secs(), 0.5);
    assert_eq!(adjustment.active_range.end().as_secs(), 1.5);
    let effects = effect_json(adjustment);
    assert_eq!(effects.len(), 1);
    assert_levels(&effects[0], 2.400_000_095_367_43);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_stack_order() {
    let converted = fresh_import(22, "adjustment-stack-order");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "stack-blur-upper",
            "stack-levels-lower",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let blur = effect_json(direct_adjustment(root, "stack-blur-upper"));
    let levels = effect_json(direct_adjustment(root, "stack-levels-lower"));
    assert_eq!((blur.len(), levels.len()), (1, 1));
    assert_gaussian(&blur[0], 23.0);
    assert_levels(&levels[0], 2.200_000_047_683_72);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_keys() {
    let converted = fresh_import(64, "adjustment-keys");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "keyed-adjustment-owner-offset",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let adjustment = direct_adjustment(root, "keyed-adjustment-owner-offset");
    assert_eq!(adjustment.active_range.start.as_secs(), 0.25);
    assert_eq!(adjustment.active_range.end().as_secs(), 1.75);
    assert_eq!(adjustment.transform.opacity.value(), 28.0);
    let effects = effect_json(adjustment);
    assert_eq!(effects.len(), 1);
    assert_gaussian(&effects[0], 4.0);
    let entries = dynamics(&converted);
    let opacity = entries
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == serde_json::to_value(adjustment.id).unwrap()
                && entry["target"]["propertyType"] == "opacity"
        })
        .expect("owner-local Adjustment opacity track");
    assert_linear_keys(opacity, &[(500, 28.0), (1_250, 86.0)]);
    let effect_id = effects[0]["id"].clone();
    let blur = entries
        .iter()
        .find(|entry| {
            entry["target"]["kind"] == "effectProperty"
                && entry["target"]["effectId"] == effect_id
                && entry["target"]["paramName"] == "blurriness"
        })
        .expect("owner-local Adjustment effect track");
    assert_linear_keys(blur, &[(250, 4.0), (1_000, 31.0)]);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_masks() {
    let converted = fresh_import(163, "adjustment-masks");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "masked-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let adjustment = direct_adjustment(root, "masked-adjustment");
    assert_eq!(adjustment.masks.len(), 1);
    let mask = &adjustment.masks[0];
    assert_eq!(serde_json::to_value(mask.mode).unwrap(), "add");
    assert!(!mask.inverted);
    assert_eq!(mask.feather, [9.0, 4.0]);
    assert!((mask.opacity.value() - 0.68).abs() < 0.000_1);
    assert_eq!(mask.expansion, 6.0);
    let guide_id = mask.layer.expect("editable asymmetric mask guide");
    let guide = root
        .layers
        .iter()
        .find(|layer| layer.data().id() == guide_id)
        .expect("mask guide is a direct sibling of Adjustment");
    let FxLayer::Shape(guide) = guide.data() else {
        panic!("Adjustment mask guide must remain editable Shape geometry")
    };
    assert_eq!(guide.name, "masked-adjustment — Mask 1 guide");
    assert_eq!(guide.parent, Some(root.id));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_matte() {
    let converted = fresh_import(183, "adjustment-matte");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "independent-alpha-matte-provider-119x67",
            "matted-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let adjustment = direct_adjustment(root, "matted-adjustment");
    let matte = adjustment
        .track_matte
        .as_ref()
        .expect("native Adjustment matte");
    assert_eq!(serde_json::to_value(matte.mode).unwrap(), "alphaInverted");
    let provider = root
        .layers
        .iter()
        .find(|layer| layer.data().id() == matte.layer)
        .expect("independent matte provider is a direct sibling");
    let FxLayer::Group(provider) = provider.data() else {
        panic!("matte provider must retain editable source geometry")
    };
    assert_eq!(
        provider.name,
        "independent-alpha-matte-provider-119x67 (matte source)"
    );
    assert!(
        !provider.is_hidden,
        "sample copy remains available to the matte"
    );
    assert_eq!(
        provider.transform.position,
        fx_composition::Position::TwoD([103.0, 83.0])
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_nested() {
    let support = fresh_import(126, "adjustment-nested__support");
    let support_root = root(&support);
    assert_source_order(
        support_root,
        &[
            "upper-sentinel-37x23",
            "nested-local-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let support_adjustment = direct_adjustment(support_root, "nested-local-adjustment");
    assert_gaussian(&effect_json(support_adjustment)[0], 25.0);

    let converted = fresh_import(146, "adjustment-nested");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "linked-nested-support",
            "outer-lower-sibling-91x43",
        ],
    );
    assert!(
        root.layers
            .iter()
            .all(|layer| !matches!(layer.data(), FxLayer::Adjustment(_))),
        "nested Adjustment must not escape into the outer stack"
    );
    let (container, adjustment) = find_adjustment(root, "nested-local-adjustment");
    assert_source_order(
        container,
        &[
            "upper-sentinel-37x23",
            "nested-local-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    assert_eq!(adjustment.parent, Some(container.id));
    let effects = effect_json(adjustment);
    assert_eq!(effects.len(), 1);
    assert_gaussian(&effects[0], 25.0);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_import_parent() {
    let converted = fresh_import(205, "adjustment-parent");
    let root = root(&converted);
    assert_source_order(
        root,
        &[
            "upper-sentinel-37x23",
            "gate-parent-null",
            "parented-masked-adjustment",
            "lower-amber-83x117",
            "lower-cobalt-127x71",
        ],
    );
    let adjustment = direct_adjustment(root, "parented-masked-adjustment");
    assert_eq!(adjustment.transform.opacity.value(), 100.0);
    assert_eq!(adjustment.masks.len(), 1);
    let guide_id = adjustment.masks[0].layer.expect("parented gate guide");
    let guide = root
        .layers
        .iter()
        .find(|layer| layer.data().id() == guide_id)
        .expect("parented gate geometry is a direct guide sibling");
    let FxLayer::Shape(guide) = guide.data() else {
        panic!("parented Adjustment geometry must be an editable guide")
    };
    assert_eq!(
        (
            guide.transform.anchor_point,
            guide.transform.position,
            guide.transform.scale,
            guide.transform.rotation,
        ),
        (
            [203.0, 119.0],
            fx_composition::Position::TwoD([184.0, 102.0]),
            [73.0, 119.0],
            23.0,
        )
    );
    for name in ["lower-amber-83x117", "lower-cobalt-127x71"] {
        let lower = root
            .layers
            .iter()
            .find(|layer| layer.data().name() == name)
            .expect("lower image sibling");
        assert_eq!(
            lower.data().parent_id(),
            Some(root.id),
            "Adjustment parent must not transform {name}"
        );
    }
    let effects = effect_json(adjustment);
    assert_eq!(effects.len(), 1);
    assert_levels(&effects[0], 2.299_999_952_316_28);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_multilevel_parent_is_not_claimed_exact() {
    // Supplementary mutation of the pinned source, not a new Adobe oracle.
    let mut project = read_project(SOURCE).unwrap();
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 205)
        .unwrap();
    let ItemKind::Composition(comp) = &mut item.kind else {
        panic!("parent composition");
    };
    let ancestor = comp
        .layers
        .iter()
        .find(|layer| !layer.record.flags().null_layer && !layer.record.flags().adjustment_layer)
        .unwrap()
        .record
        .id();
    let parent = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.flags().null_layer)
        .unwrap();
    parent.record = parent
        .record
        .clone()
        .with_export_options(true, false, 0, ancestor, 0, 0)
        .unwrap();
    let imported = to_structural_fx_document(&project, Some(205)).unwrap();
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("ancestor")
                && diagnostic.message.contains("not mapped"))
    );
    assert!(
        !imported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("carried exactly"))
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_matte_discloses_gated_compositing_limitation() {
    let project = read_project(SOURCE).unwrap();
    let imported = to_structural_fx_document(&project, Some(183)).unwrap();
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("dry-plus-wet")
                && diagnostic.message.contains("not pixel-equivalent"))
    );
}
