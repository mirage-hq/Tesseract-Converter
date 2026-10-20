use super::*;

#[test]
fn venetian_blinds_maps_native_pixel_centers_before_direction_projection() {
    let owner = venetian_owner();
    let imported = import(
        &ExpressionSamples::default(),
        RIPPLE_COMPOSITION,
        &owner,
        PLANE,
        PLANE,
        &mut 100,
        &mut Default::default(),
    );
    assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
    let value = serde_json::to_value(&imported.effects[0]).unwrap();
    let effect = &value["effect"];
    let parameters = effect["params"].as_array().unwrap();
    let offset = parameters
        .iter()
        .find(|parameter| parameter["name"] == "pixelCenterOffset")
        .expect("native pixel-center mapping")["default"]
        .as_f64()
        .unwrap();
    assert_eq!(offset, -0.5);
    assert_eq!(parameters[0]["default"], 80.);
    assert_eq!(parameters[2]["default"], 20.);
    let shader = effect["wgsl"].as_str().unwrap();
    assert!(shader.contains("+ vec2<f32>(params.pixel_center_offset)"));

    // Native integer centers put half coverage on both endpoints of a 4px band.
    let integral = |position: f64| {
        let turn = (position / 20.).floor();
        turn * 4. + (position - turn * 20. - 16.).max(0.)
    };
    for (pixel, expected) in [(0, 0.5), (15, 0.), (16, 0.5), (17, 1.), (19, 1.), (20, 0.5)] {
        let position = f64::from(pixel) + 0.5 + offset;
        let coverage = integral(position + 0.5) - integral(position - 0.5);
        assert_eq!(coverage, expected, "pixel {pixel}");
    }
}

fn animated_owner(control: &str) -> Layer {
    let mut owner = venetian_owner();
    let mut donor = native_effect("ADBE Gaussian Blur 2", &[("-0001", 10.)]);
    donor
        .properties
        .iter_mut()
        .find(|p| p.match_name.ends_with("-0001"))
        .unwrap()
        .animation = Some(NumericTrack {
        keys: [
            (1000, 10., KeyframeEasing::Linear),
            (2000, 90., KeyframeEasing::Hold),
        ]
        .into_iter()
        .map(|(time_millis, value, easing)| NumericKeyframe {
            time_millis,
            values: vec![value],
            easing: vec![easing],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        })
        .collect(),
    });
    let parade = effect_parade(&[donor], owner.record.id(), PLANE.map(f64::from)).unwrap();
    let descriptor = crate::properties::unique_list(
        crate::properties::runs(parade.children().unwrap()).unwrap()[0].1,
        *b"sspc",
    )
    .unwrap();
    let controls =
        crate::properties::runs(crate::properties::unique_list(descriptor, *b"tdgp").unwrap())
            .unwrap();
    let leaf = crate::properties::unique_list(
        controls
            .iter()
            .find(|(name, _)| *name == "ADBE Gaussian Blur 2-0001")
            .unwrap()
            .1,
        *b"tdbs",
    )
    .unwrap();
    let replacement = Chunk::list(*b"tdbs", leaf.to_vec());
    fn replace(chunks: &mut [Chunk], control: &str, replacement: &Chunk) -> bool {
        for index in 0..chunks.len() {
            if chunk_match_name(&chunks[index]) == Some(control) {
                chunks[index + 1] = replacement.clone();
                return true;
            }
            if let Some(children) = chunks[index].children_mut()
                && replace(children, control, replacement)
            {
                return true;
            }
        }
        false
    }
    assert!(replace(&mut owner.content, control, &replacement));
    owner
}

#[test]
fn animated_venetian_blinds_retains_editable_completion_keys() {
    let owner = animated_owner("ADBE Venetian Blinds-0001");
    let mut next = 100;
    let imported = import(
        &ExpressionSamples::default(),
        RIPPLE_COMPOSITION,
        &owner,
        PLANE,
        PLANE,
        &mut next,
        &mut AnimationBudget::default(),
    );
    assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
    assert_eq!(next, 101);
    assert_eq!(imported.native_ordinals, [1]);
    let value = serde_json::to_value(&imported.effects[0]).unwrap();
    assert_eq!(value["effect"]["params"][0]["default"], 10.);
    assert_eq!(imported.animations.len(), 1, "{:?}", imported.warnings);
    let entry = &imported.animations[0];
    assert_eq!(
        entry.target,
        PropertyTarget::effect_param(EffectId::new(100), "completion")
    );
    let track = entry.animator.keyframe_track().unwrap();
    for (key, expected) in track.keyframes().iter().zip([10., 90.]) {
        assert_eq!(key.value(), &PropertyValue::Float(expected));
    }
    assert_eq!(track.keyframes().len(), 2);
    assert_eq!(track.keyframes()[0].layer_time().as_millis(), 1000);
    assert_eq!(track.keyframes()[1].layer_time().as_millis(), 2000);
    assert_eq!(
        track.keyframes()[0].easing(),
        PropertyKeyframeEasing::Linear
    );
    assert_eq!(track.keyframes()[1].easing(), PropertyKeyframeEasing::Hold);
}

#[test]
fn animated_venetian_blinds_controls_keep_stretched_owner_clock() {
    for (suffix, param) in [
        ("0001", "completion"),
        ("0002", "direction"),
        ("0003", "width"),
        ("0004", "feather"),
    ] {
        let mut owner = animated_owner(&format!("ADBE Venetian Blinds-{suffix}"));
        let mut bytes = owner.record.encode();
        bytes[12..16].copy_from_slice(&2_i32.to_be_bytes());
        bytes[16..20].copy_from_slice(&1_u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&2_i32.to_be_bytes());
        bytes[108..112].copy_from_slice(&1_u32.to_be_bytes());
        owner.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
        let mut budget = AnimationBudget::default();
        let imported = import(
            &ExpressionSamples::default(),
            RIPPLE_COMPOSITION,
            &owner,
            PLANE,
            PLANE,
            &mut 100,
            &mut budget,
        );
        assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
        assert_eq!(imported.animations.len(), 1);
        let entry = &imported.animations[0];
        assert_eq!(
            entry.target,
            PropertyTarget::effect_param(EffectId::new(100), param)
        );
        let track = entry.animator.keyframe_track().unwrap();
        assert_eq!(track.keyframes()[0].layer_time().as_millis(), 4000);
        assert_eq!(track.keyframes()[1].layer_time().as_millis(), 6000);
        assert_eq!(
            budget.used(),
            committed_entry_reservation_bytes(entry).unwrap()
        );
    }
}

#[test]
fn animated_venetian_blinds_rejection_does_not_leak_ids_or_budget() {
    let owner = animated_owner("ADBE Venetian Blinds-0001");
    let mut budget = AnimationBudget::with_limit(1);
    let mut next = 100;
    let imported = import(
        &ExpressionSamples::default(),
        RIPPLE_COMPOSITION,
        &owner,
        PLANE,
        PLANE,
        &mut next,
        &mut budget,
    );
    assert!(imported.effects.is_empty());
    assert!(imported.animations.is_empty());
    assert_eq!(next, 100);
    assert_eq!(budget.used(), 0);
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("effect omitted"))
    );
}
