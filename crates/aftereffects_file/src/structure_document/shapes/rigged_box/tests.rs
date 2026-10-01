use super::*;
use crate::{
    structure::{ItemKind, read_project},
    structure_document::{
        animation_budget::AnimationBudget,
        group,
        shapes::{OutputBudget, full_active_range, import_with_composition},
    },
};
use fx_schema::{Layer, LayerData, LayerId, Position, PropType, PropertyTarget, PropertyValue};
use sha2::{Digest, Sha256};

fn scalar_curve(values: &[(f64, f64)]) -> NumericProperty {
    NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: values
            .iter()
            .map(|&(time, value)| constant_key(time, value))
            .collect(),
        value_kind: NumericValueKind::Continuous,
    }
}

#[test]
fn stock_expression_grammar_rejects_suffixes_and_renamed_effects() {
    assert_eq!(
        canonical(SIZE_EXPRESSION),
        canonical(&format!(
            "// Rigged Box 3.0 - Size\r{}",
            SIZE_EXPRESSION.replace('\n', "\r")
        ))
    );
    assert_ne!(
        canonical(SIZE_EXPRESSION),
        canonical(&SIZE_EXPRESSION.replace("Rigged Box", "RiggedBox"))
    );
    assert_ne!(
        canonical(SIZE_EXPRESSION),
        canonical(&format!("{SIZE_EXPRESSION}value;"))
    );
    assert_ne!(
        canonical(SIZE_EXPRESSION),
        canonical(&SIZE_EXPRESSION.replace("Rigged Box", "Other Box"))
    );
}

#[test]
fn independent_size_axes_merge_only_across_constant_segments() {
    let x = scalar_curve(&[(0.0, 45.0), (1.0 / 24.0, 40.0), (19.0 / 24.0, 40.0)]);
    let y = scalar_curve(&[
        (0.0, 35.0),
        (1.0 / 24.0, 45.0),
        (10.0 / 24.0, 40.0),
        (19.0 / 24.0, 45.0),
        (20.0 / 24.0, 40.0),
    ]);
    let merged = merge_axes(&x, &y).unwrap();
    assert_eq!(merged.keyframes.len(), 5);
    assert_eq!(merged.keyframes[0].values, [45.0, 35.0]);
    assert_eq!(merged.keyframes[2].values, [40.0, 40.0]);
    assert_eq!(merged.keyframes[4].values, [40.0, 40.0]);

    let incompatible = scalar_curve(&[(0.0, 45.0), (1.0, 50.0)]);
    assert!(merge_axes(&incompatible, &y).is_err());

    let mut held = scalar_curve(&[(0.0, 10.0), (1.0, 20.0)]);
    held.keyframes[0].out_interpolation = 3;
    let changing = scalar_curve(&[(0.0, 30.0), (1.0, 40.0)]);
    assert!(merge_axes(&held, &changing).is_err());
    let constant = scalar_curve(&[(0.0, 30.0), (1.0, 30.0)]);
    assert_eq!(
        merge_axes(&held, &constant).unwrap().keyframes[0].out_interpolation,
        3
    );
    let mut overshoot = constant.clone();
    overshoot.keyframes[0].out_interpolation = 2;
    overshoot.keyframes[0].out_speed = vec![5.0];
    assert!(merge_axes(&overshoot, &changing).is_err());
}

fn find_rect(layers: &[LayerData]) -> Option<&fx_schema::RectLayer> {
    layers.iter().find_map(|layer| match layer {
        LayerData::Rect(rect) => Some(rect),
        LayerData::Group(group) => find_stored_rect(&group.layers),
        _ => None,
    })
}

fn find_stored_rect(layers: &[Layer]) -> Option<&fx_schema::RectLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Rect(rect) => Some(rect),
        LayerData::Group(group) => find_stored_rect(&group.layers),
        _ => None,
    })
}

#[test]
#[ignore = "requires local licensed AEP_RIGGED_BOX_SOURCE, which cannot be redistributed"]
fn local_external_source_imports_comp538_layer539_as_rigged_box_rect() {
    let path = std::env::var_os("AEP_RIGGED_BOX_SOURCE").expect("local licensed source path");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(composition) = &project.item(538).unwrap().kind else {
        panic!("composition 538")
    };
    let source_items = project.items.iter().map(|item| (item.id, item)).collect();
    let layer = composition
        .layers
        .iter()
        .find(|layer| layer.record.id() == 539)
        .unwrap();
    let parent = group(LayerId::new(1), "proof".into(), None, full_active_range());
    let imported = import_with_composition(
        layer,
        composition,
        &source_items,
        true,
        &parent,
        8,
        &mut 10_000,
        &mut OutputBudget::default(),
        &mut AnimationBudget::default(),
    )
    .unwrap();
    let rect = find_rect(&imported.layers)
        .unwrap_or_else(|| panic!("typed editable Rigged Box Rect: {:?}", imported.warnings));
    assert_eq!(rect.rect.size, [45.0, 35.0]);
    assert_eq!(rect.rect.roundness, 20.0);
    assert_eq!(rect.transform.anchor_point, [22.5, 17.5]);
    assert_eq!(rect.transform.position, Position::TwoD([0.0, -17.5]));
    assert!(!rect.rect.fill_enabled);
    assert!(rect.rect.stroke_enabled);

    let entry = imported
        .animations
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(rect.id, PropType::RectSize))
        .expect("typed size curve");
    assert!(!entry.animator.is_js_script());
    let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
        panic!("typed keyframes")
    };
    let keys = track.keyframes();
    assert_eq!(keys.len(), 5);
    assert_eq!(keys[0].value(), &PropertyValue::Vector2([45.0, 35.0]));
    assert_eq!(keys[4].value(), &PropertyValue::Vector2([40.0, 40.0]));
    for property in [PropType::PositionX, PropType::PositionY] {
        assert!(imported.animations.iter().any(|entry| {
            entry.target == PropertyTarget::layer(rect.id, property)
                && !entry.animator.is_js_script()
        }));
    }
}
