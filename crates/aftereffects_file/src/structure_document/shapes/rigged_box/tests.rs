use super::*;

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
