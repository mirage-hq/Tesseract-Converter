use super::*;

#[test]
fn p033_anchor_independent_native_control_has_union_keys() {
    let project = crate::structure::read_project(include_bytes!(
        "../../../../tests/fixtures/properties/p033_anchor_union.aep"
    ))
    .unwrap();
    let crate::structure::ItemKind::Composition(composition) = &project.item(1).unwrap().kind
    else {
        panic!("pinned native composition 1")
    };
    assert_eq!(composition.layers.len(), 1);
    let properties = crate::properties::read_transform(&composition.layers[0].content).unwrap();
    let anchor = properties
        .iter()
        .find(|property| property.match_name == "ADBE Anchor Point")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(
        anchor
            .keyframes
            .iter()
            .map(|key| key.time_secs)
            .collect::<Vec<_>>(),
        [0.0, 0.5, 1.0]
    );
    assert_eq!(
        anchor
            .keyframes
            .iter()
            .map(|key| key.values.clone())
            .collect::<Vec<_>>(),
        [
            vec![0.0, 0.0, 0.0],
            // Native solid Anchor storage is source-normalized (160x120).
            vec![40.0 / 160.0, 10.0 / 120.0, 0.0],
            vec![40.0 / 160.0, 20.0 / 120.0, 0.0]
        ]
    );
}

fn entries(endpoint: f64) -> Vec<AnimationGraphEntry> {
    let owner = LayerId::new(100);
    [
        (PropType::AnchorPointX, vec![(0, 0.0), (500, endpoint)]),
        (PropType::AnchorPointY, vec![(0, 0.0), (1_000, 20.0)]),
        (PropType::PositionX, vec![(0, 80.0), (500, 90.0)]),
        (PropType::PositionY, vec![(0, 60.0), (1_000, 70.0)]),
    ]
    .into_iter()
    .map(|(property, keys)| {
        keyed_entry(
            owner,
            property,
            &keys
                .into_iter()
                .map(|(time, value)| (time, value, PropertyKeyframeEasing::Linear))
                .collect::<Vec<_>>(),
        )
    })
    .collect()
}

#[test]
fn p033_anchor_independent_knots_keep_planar_owner_and_editable_union() {
    // The actual49 Group100 has independently fitted Anchor and Position knots.
    // This minimized curve is controlled independently in native AE, rather than
    // obtained by round-tripping converter output into its own reference.
    for endpoint in [40.0, 60.0] {
        let entries = entries(endpoint);
        let lowered = lower(
            &crate::export_document::AnimationIndex::new(&entries),
            &transform_2d([80.0, 60.0]),
            LayerId::new(100),
            Native2dGeometry::IDENTITY,
        )
        .expect("independent Linear Anchor knots must not omit their owner")
        .unwrap();
        assert!(!lowered.transform.is_three_d);
        assert!(lowered.animations.position_separated.is_some());
        let anchor = lowered.animations.anchor.unwrap();
        assert_eq!(
            anchor
                .keys
                .iter()
                .map(|key| key.time_millis)
                .collect::<Vec<_>>(),
            [0, 500, 1_000]
        );
        assert_eq!(
            anchor
                .keys
                .iter()
                .map(|key| key.values.clone())
                .collect::<Vec<_>>(),
            [
                vec![0.0, 0.0, 0.0],
                vec![endpoint, 10.0, 0.0],
                vec![endpoint, 20.0, 0.0]
            ]
        );
        for key in &anchor.keys {
            assert_eq!(key.easing, [KeyframeEasing::Linear]);
            assert_eq!(key.spatial_in, [0.0; 3]);
            assert_eq!(key.spatial_out, [0.0; 3]);
        }
    }
}

#[test]
fn p033_anchor_rejects_simultaneous_hold_and_continuous_changes() {
    let mut entries = entries(40.0);
    entries[0] = keyed_entry(
        LayerId::new(100),
        PropType::AnchorPointX,
        &[
            (0, 0.0, PropertyKeyframeEasing::Hold),
            (500, 40.0, PropertyKeyframeEasing::Hold),
        ],
    );
    assert!(
        lower(
            &crate::export_document::AnimationIndex::new(&entries),
            &transform_2d([80.0, 60.0]),
            LayerId::new(100),
            Native2dGeometry::IDENTITY
        )
        .is_err()
    );
}

#[test]
fn p033_anchor_does_not_admit_cubic_or_authored_spatial_subdivision() {
    let owner = LayerId::new(100);
    let cubic = PropertyKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
    };
    for spatial in [false, true] {
        let mut entries = entries(40.0);
        if spatial {
            entries[0].animator = PropertyAnimator::keyframes(
                PropertyKeyframeTrack::new(vec![
                    PropertyKeyframe::new(
                        KeyframeId::new("anchor-start"),
                        TimeOffset::from_millis(0),
                        PropertyValue::Float(0.0),
                        PropertyKeyframeEasing::Linear,
                    )
                    .with_spatial_tangents(None, Some(4.0)),
                    PropertyKeyframe::new(
                        KeyframeId::new("anchor-end"),
                        TimeOffset::from_millis(500),
                        PropertyValue::Float(40.0),
                        PropertyKeyframeEasing::Linear,
                    )
                    .with_spatial_tangents(Some(4.0), None),
                ])
                .unwrap(),
            );
        } else {
            entries[0] = keyed_entry(
                owner,
                PropType::AnchorPointX,
                &[(0, 0.0, cubic), (500, 40.0, cubic)],
            );
        }
        assert!(
            lower(
                &crate::export_document::AnimationIndex::new(&entries),
                &transform_2d([80.0, 60.0]),
                owner,
                Native2dGeometry::IDENTITY
            )
            .is_err()
        );
    }
}
