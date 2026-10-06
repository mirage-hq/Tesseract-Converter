use super::*;
use crate::export_document::{identity_fx_transform, track, transform_animations_partitioned};
use fx_schema::{EffectId, PropType, PropertyTarget, PropertyValue, animator::PropertyAnimator};

fn entry(layer: u64, property: PropType, value: f64) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(LayerId::new(layer), property),
        animator: PropertyAnimator::constant(PropertyValue::Float(value)).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

#[test]
fn indexed_queries_match_linear_queries_and_borrow_the_original_records() {
    let properties = [PropType::Opacity, PropType::PositionX, PropType::PositionY];
    let mut entries = (0..4096)
        .map(|position| {
            entry(
                position % 128,
                properties[position as usize % 3],
                position as f64,
            )
        })
        .collect::<Vec<_>>();
    let mut effect = entry(0, PropType::Opacity, 99.0);
    effect.target = PropertyTarget::effect_param(EffectId::new(17), "amount");
    entries.insert(17, effect);
    let index = AnimationIndex::new(&entries);
    assert_eq!(index.iter().count(), entries.len());
    for (indexed, original) in (&index).into_iter().zip(&entries) {
        assert!(std::ptr::eq(indexed, original));
    }
    // Include a missing owner and property as well as interleaved duplicates.
    for layer in 0..=128 {
        let id = LayerId::new(layer);
        let expected = entries
            .iter()
            .filter(|entry| entry.target.layer_id() == Some(id))
            .collect::<Vec<_>>();
        let actual = index.for_layer(id).collect::<Vec<_>>();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(std::ptr::eq(actual, expected));
        }
        for property in properties.into_iter().chain([PropType::Rotation]) {
            let property = Property::new(id, property);
            let expected = entries
                .iter()
                .find(|entry| entry.target.as_property() == Some(property));
            let actual = index.first(property);
            assert_eq!(
                actual.map(std::ptr::from_ref),
                expected.map(std::ptr::from_ref)
            );
        }
    }
}

#[test]
fn first_invalid_duplicate_is_not_replaced_by_a_later_exportable_entry() {
    let mut invalid = entry(7, PropType::Opacity, 0.25);
    invalid.random_seed_target = Some(invalid.target.clone());
    let entries = [invalid, entry(7, PropType::Opacity, 0.75)];
    let index = AnimationIndex::new(&entries);
    assert_eq!(
        track(&index, LayerId::new(7), PropType::Opacity).err(),
        Some("Dependent animator cannot be represented as a native numeric keyframe track")
    );
    assert!(std::ptr::eq(
        index
            .first(Property::new(LayerId::new(7), PropType::Opacity))
            .unwrap(),
        &entries[0]
    ));
}

#[test]
fn each_derived_graph_uses_its_own_records_and_partition_validation() {
    let root_entries = [
        entry(7, PropType::Opacity, 0.25),
        entry(99, PropType::AudioVolume, 1.0),
    ];
    let derived_entries = [
        entry(7, PropType::Opacity, 0.75),
        entry(7, PropType::AudioVolume, 1.0),
    ];
    let root = AnimationIndex::new(&root_entries);
    let derived = AnimationIndex::new(&derived_entries);
    let property = Property::new(LayerId::new(7), PropType::Opacity);
    assert!(std::ptr::eq(
        root.first(property).unwrap(),
        &root_entries[0]
    ));
    assert!(std::ptr::eq(
        derived.first(property).unwrap(),
        &derived_entries[0]
    ));
    let base = identity_fx_transform();
    assert!(
        transform_animations_partitioned(
            &root,
            LayerId::new(7),
            &base,
            LayerId::new(7),
            false,
            false
        )
        .is_ok()
    );
    assert_eq!(
        transform_animations_partitioned(
            &derived,
            LayerId::new(7),
            &base,
            LayerId::new(7),
            false,
            false
        )
        .err(),
        Some("Layer has animator targets outside native 2D Transform support")
    );
}
