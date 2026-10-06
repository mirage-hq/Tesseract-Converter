use super::*;

#[test]
fn simultaneous_gain_curves_keep_both_envelopes_and_the_hold_boundary() {
    fn track(first: f64, last: f64, easing: PropertyKeyframeEasing) -> PropertyKeyframeTrack {
        PropertyKeyframeTrack::new(vec![
            PropertyKeyframe::new(
                KeyframeId::new("first"),
                TimeOffset::from_millis(0),
                PropertyValue::Float(first),
                PropertyKeyframeEasing::Linear,
            ),
            PropertyKeyframe::new(
                KeyframeId::new("last"),
                TimeOffset::from_millis(1000),
                PropertyValue::Float(last),
                easing,
            ),
        ])
        .unwrap()
    }
    let inner = track(0.5, 1.0, PropertyKeyframeEasing::Linear);
    let outer = track(0.25, 0.75, PropertyKeyframeEasing::Linear);
    let product = product_track(&inner, &outer, LayerId::new(1)).unwrap();
    let AnimatorData::Keyframes { track: product, .. } = product.data() else {
        panic!("expected editable keys")
    };
    assert_eq!(
        super::super::audio::sample_volume(product, 500),
        Some(0.375)
    );
    assert_eq!(
        super::super::audio::sample_volume(product, 1000),
        Some(0.75)
    );
    let outer = track(0.25, 0.75, PropertyKeyframeEasing::Hold);
    let product = product_track(&inner, &outer, LayerId::new(1)).unwrap();
    let AnimatorData::Keyframes { track: product, .. } = product.data() else {
        panic!("expected editable keys")
    };
    assert_eq!(
        super::super::audio::sample_volume(product, 999),
        Some(0.9995 * 0.25)
    );
    assert_eq!(
        super::super::audio::sample_volume(product, 1000),
        Some(0.75)
    );
    assert_eq!(
        product.keyframes().last().unwrap().easing(),
        PropertyKeyframeEasing::Hold
    );
}
