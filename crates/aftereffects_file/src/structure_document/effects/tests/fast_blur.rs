//! Synthetic legacy profiles on the existing catalog owner, not Adobe-native proof.
use super::*;
use fx_schema::{EffectData, EffectPayload};

fn owner(blur: f64, dimensions: f64, repeat: f64, animated: bool) -> Layer {
    let mut effect = native_effect(
        "ADBE Gaussian Blur 2",
        &[("-0001", blur), ("-0002", dimensions), ("-0003", repeat)],
    );
    if animated {
        effect
            .properties
            .iter_mut()
            .find(|p| p.match_name.ends_with("-0001"))
            .unwrap()
            .animation = Some(NumericTrack {
            keys: [(0, blur), (1000, 0.)]
                .into_iter()
                .map(|(time_millis, value)| NumericKeyframe {
                    time_millis,
                    values: vec![value],
                    easing: vec![KeyframeEasing::Linear],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
                .collect(),
        });
    }
    let mut owner = catalog_owner(&[effect, native_effect("ADBE Exposure2", &[])]);
    fn rename(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"tdmn" {
                let name = chunk_match_name(chunk)
                    .unwrap()
                    .replace("ADBE Gaussian Blur 2", "ADBE Fast Blur");
                let mut bytes = name.into_bytes();
                bytes.resize(40, 0);
                *chunk = Chunk::data(*b"tdmn", bytes).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                rename(children);
            }
        }
    }
    rename(&mut owner.content);
    owner
}

#[test]
fn fast_blur_import_keeps_editable_radius_keys_and_repeat_bounds() {
    for (repeat, animated) in [(0., false), (1., true)] {
        let owner = owner(241., 1., repeat, animated);
        let mut id = 100;
        let result = import(
            &ExpressionSamples::default(),
            1,
            &owner,
            PLANE,
            PLANE,
            &mut id,
            &mut AnimationBudget::default(),
        );
        assert_eq!(result.effects.len(), 2, "{:?}", result.warnings);
        assert_eq!(result.native_ordinals, vec![1, 2]);
        let EffectData::Identified {
            effect: EffectPayload::Known(effect),
            ..
        } = result.effects[0].data()
        else {
            panic!()
        };
        let value = serde_json::to_value(effect).unwrap();
        assert_eq!(value["type"], "gaussianBlur");
        assert_eq!(value["blurriness"], 241.);
        assert_eq!(value["repeatEdgePixels"], repeat != 0.);
        if repeat != 0. {
            assert_eq!(value["layerSize"], serde_json::json!(PLANE.map(f64::from)));
        } else {
            assert!(value.get("layerSize").is_none_or(Value::is_null));
        }
        assert_eq!(result.animations.len(), usize::from(animated));
        if animated {
            assert_eq!(
                result.animations[0].target,
                PropertyTarget::effect_param(EffectId::new(100), "blurriness")
            );
            let values: Vec<_> = keys(&result.animations[0])
                .into_iter()
                .map(|(_, v, _)| v)
                .collect();
            assert_eq!(values, vec![241., 0.]);
        }
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Fast Blur") && w.contains("kernel"))
        );
    }
}

#[test]
fn fast_blur_unsupported_controls_keep_sibling_effect() {
    for (blur, dimensions, repeat) in [(-1., 1., 0.), (12., 2., 0.), (12., 3., 0.), (12., 1., 2.)] {
        let owner = owner(blur, dimensions, repeat, false);
        let mut id = 100;
        let result = import(
            &ExpressionSamples::default(),
            1,
            &owner,
            PLANE,
            PLANE,
            &mut id,
            &mut AnimationBudget::default(),
        );
        assert_eq!(result.effects.len(), 1, "{:?}", result.warnings);
        assert_eq!(result.native_ordinals, vec![2]);
        assert_eq!(id, 101, "omitted blur must not consume an effect ID");
        assert!(result.animations.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Fast Blur") && w.contains("effect omitted"))
        );
    }
}
