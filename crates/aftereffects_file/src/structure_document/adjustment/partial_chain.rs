//! Narrow safety fallback for an output-defining unsupported image effect.

use crate::effects::native::DecodedEffect;
use fx_schema::BlendMode;

pub(super) fn unsafe_partial_chain(
    adjustment: bool,
    effects_active: bool,
    blend: BlendMode,
    effects: &[DecodedEffect],
) -> bool {
    if !adjustment || !effects_active || blend != BlendMode::HardLight {
        return false;
    }
    let active: Vec<_> = effects.iter().filter(|effect| effect.enabled).collect();
    let [prefix @ .., tint, emboss] = active.as_slice() else {
        return false;
    };
    // Pseudo effects are controller-only. No unknown image stage or later
    // supported stage may be swallowed by this fallback.
    if prefix
        .iter()
        .any(|effect| !effect.match_name.starts_with("Pseudo/"))
        || tint.match_name != "ADBE Tint"
        || emboss.match_name != "ADBE Emboss"
    {
        return false;
    }
    let mut controls = emboss
        .parameters
        .iter()
        .filter(|parameter| parameter.match_name == "ADBE Emboss-0004");
    let Some(control) = controls.next() else {
        return false;
    };
    if controls.next().is_some() || control.declared_kind.is_err() || control.unset_path {
        return false;
    }
    control.numeric.as_ref().is_ok_and(|value| {
        value.values.as_slice() == [0.0]
            && !value.animated
            && value.keyframes.is_empty()
            && !value.expression_enabled
            && !value.dimensions_separated
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        effects::native::{Declarations, DecodedParameter},
        properties::{NumericProperty, NumericValueKind, PropertyError},
    };

    fn effect(name: &str) -> DecodedEffect {
        DecodedEffect {
            match_name: name.into(),
            index: 1,
            enabled: true,
            declarations: Declarations::Readable,
            parameters: Vec::new(),
        }
    }

    fn chain() -> Vec<DecodedEffect> {
        let mut emboss = effect("ADBE Emboss");
        emboss.index = 2;
        emboss.parameters.push(DecodedParameter {
            match_name: "ADBE Emboss-0004".into(),
            declared_kind: Ok(Some(2)),
            unset_path: false,
            numeric: Ok(NumericProperty {
                values: vec![0.0],
                animated: false,
                expression_enabled: false,
                expression_present: false,
                dimensions_separated: false,
                keyframes: Vec::new(),
                value_kind: NumericValueKind::Continuous,
            }),
        });
        vec![effect("ADBE Tint"), emboss]
    }

    #[test]
    fn unsafe_partial_emboss_chain_requires_exact_profile() {
        let effects = chain();
        assert!(unsafe_partial_chain(
            true,
            true,
            BlendMode::HardLight,
            &effects
        ));
        for blend in [BlendMode::Normal, BlendMode::Add] {
            assert!(!unsafe_partial_chain(true, true, blend, &effects));
        }
        assert!(!unsafe_partial_chain(
            false,
            true,
            BlendMode::HardLight,
            &effects
        ));
        assert!(!unsafe_partial_chain(
            true,
            false,
            BlendMode::HardLight,
            &effects
        ));
        for index in 0..2 {
            let mut changed = effects.clone();
            changed[index].enabled = false;
            assert!(!unsafe_partial_chain(
                true,
                true,
                BlendMode::HardLight,
                &changed
            ));
        }
        let mut controlled = effects.clone();
        controlled.insert(0, effect("Pseudo/custom-controller"));
        assert!(unsafe_partial_chain(
            true,
            true,
            BlendMode::HardLight,
            &controlled
        ));
        let mut changed = effects.clone();
        let duplicate = changed[1].parameters[0].clone();
        changed[1].parameters.push(duplicate);
        assert!(!unsafe_partial_chain(
            true,
            true,
            BlendMode::HardLight,
            &changed
        ));
        let mut changed = effects.clone();
        changed.reverse();
        assert!(!unsafe_partial_chain(
            true,
            true,
            BlendMode::HardLight,
            &changed
        ));
        let mut changed = effects.clone();
        changed.push(effect("ADBE Gaussian Blur 2"));
        assert!(!unsafe_partial_chain(
            true,
            true,
            BlendMode::HardLight,
            &changed
        ));
        let mut changed = effects.clone();
        changed.insert(0, effect("Unknown Image Effect"));
        assert!(!unsafe_partial_chain(
            true,
            true,
            BlendMode::HardLight,
            &changed
        ));
    }

    #[test]
    fn unsafe_partial_emboss_chain_rejects_unset_original_mix() {
        let mut effects = chain();
        effects[1].parameters[0].unset_path = true;
        assert!(!unsafe_partial_chain(
            true,
            true,
            BlendMode::HardLight,
            &effects
        ));
    }

    #[test]
    fn unsafe_partial_emboss_chain_rejects_unproven_original_mix() {
        for state in 0..7 {
            let mut effects = chain();
            let parameter = &mut effects[1].parameters[0];
            if state == 0 {
                parameter.numeric = Err(PropertyError::Layout("unreadable"));
            } else {
                let numeric = parameter.numeric.as_mut().unwrap();
                match state {
                    1 => numeric.values = vec![1.0],
                    2 => numeric.animated = true,
                    3 => numeric.expression_enabled = true,
                    4 => numeric.values.clear(),
                    5 => numeric.dimensions_separated = true,
                    _ => numeric.values = vec![f64::NAN],
                }
            }
            assert!(
                !unsafe_partial_chain(true, true, BlendMode::HardLight, &effects),
                "state {state}"
            );
        }
    }
}
