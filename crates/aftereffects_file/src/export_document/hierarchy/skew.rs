//! Exact two-transform lowering for static planar Group skew.

use fx_schema::{GroupLayer, LayerId, Position, PropType, Transform};

use crate::writer::{NativeLayerOptions, NullLayerSpec, SolidTransform, TransformAnimations};

#[derive(Clone, Debug)]
pub(super) struct Lowering {
    pub outer: SolidTransform,
    pub outer_animations: TransformAnimations,
    pub inner: NullLayerSpec,
    pub helper_id: LayerId,
}

pub(super) fn is_present(transform: &Transform) -> bool {
    transform.skew != 0.0 || transform.skew_axis != 0.0
}

pub(super) fn validate_source(group: &GroupLayer) -> Result<(), &'static str> {
    if !group.effects.is_empty() || !group.masks.is_empty() {
        return Err("Static Group skew cannot move Group effects or masks across affine helpers");
    }
    if !group.fills.is_empty()
        || [
            group.padding_top.value(),
            group.padding_right.value(),
            group.padding_bottom.value(),
            group.padding_left.value(),
            group.corner_radius_top_left.value(),
            group.corner_radius_top_right.value(),
            group.corner_radius_bottom_right.value(),
            group.corner_radius_bottom_left.value(),
        ]
        .into_iter()
        .any(|value| value != 0.0)
    {
        return Err("Static Group skew requires no Group background, padding, or corners");
    }
    Ok(())
}

pub(super) fn lower(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    helper_id: LayerId,
) -> Result<Lowering, &'static str> {
    if !is_present(&group.transform) {
        return Err("Static skew helper requires an authored Group skew");
    }
    validate_source(group)?;
    if super::subtree_needs_projection(&group.layers, dynamics) {
        return Err("Static Group skew helper does not change a projected 3D subtree");
    }
    let transform = &group.transform;
    let Position::TwoD(position) = transform.position else {
        return Err("Static Group skew helper requires a planar position");
    };
    if transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
    {
        return Err("Static Group skew helper cannot combine with a 3D Group Transform");
    }

    // At a zero base Scale the current matrix is singular, even when authored
    // uniform Scale keys make the rendered animation nonsingular. Factor the
    // fixed skew/rotation at unit Scale and put the zero on the outer Scale.
    let zero_base = transform.scale == [0.0; 2];
    let matrix = if zero_base {
        matrix_components(
            [100.0; 2],
            transform.rotation,
            transform.skew,
            transform.skew_axis,
        )?
    } else {
        matrix(transform)?
    };
    let factors = Factors::from_matrix(matrix)?;
    factors.verify(matrix)?;
    let outer_animations =
        uniform_scale_animations(group, dynamics, [factors.first, factors.second])?;
    if zero_base && outer_animations.scale.is_none() {
        return Err("Static Group skew matrix is singular");
    }
    let anchor = transform.anchor_point;
    if anchor
        .into_iter()
        .chain(position)
        .any(|value| !value.is_finite())
        || !transform.opacity.value().is_finite()
    {
        return Err("Static Group skew Transform is non-finite");
    }

    Ok(Lowering {
        outer: SolidTransform {
            anchor,
            position,
            scale: if zero_base {
                [0.0; 2]
            } else {
                [factors.first * 100.0, factors.second * 100.0]
            },
            rotation: factors.outer_rotation.to_degrees(),
            opacity: transform.opacity.value(),
        },
        outer_animations,
        inner: NullLayerSpec {
            name: format!("{} — Skew basis", group.name),
            transform: SolidTransform {
                anchor,
                position: anchor,
                scale: [100.0; 2],
                rotation: factors.inner_rotation.to_degrees(),
                opacity: 100.0,
            },
            transform_animations: TransformAnimations::default(),
        },
        helper_id,
    })
}

fn uniform_scale_animations(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    factored_scale: [f64; 2],
) -> Result<TransformAnimations, &'static str> {
    if !super::super::has_transform_entries(dynamics, group.id) {
        return Ok(TransformAnimations::default());
    }
    let unsupported = "Animated Group skew or Transform requires an exact animated affine lowering; only identical uniform Scale curves and planar Position/Opacity are supported";
    if dynamics.for_layer(group.id).any(|entry| {
        entry.target.as_property().is_none_or(|property| {
            !matches!(
                property.property_type(),
                PropType::ScaleX
                    | PropType::ScaleY
                    | PropType::PositionX
                    | PropType::PositionY
                    | PropType::Opacity
            )
        })
    }) {
        return Err(unsupported);
    }
    let mut animations =
        super::super::transform_animations(dynamics, group.id, &group.transform, group.id)?;
    let Some(scale) = animations.scale.as_mut() else {
        return if group.transform.scale == [0.0; 2] {
            Err(unsupported)
        } else {
            Ok(animations)
        };
    };
    if group.transform.scale[0] != group.transform.scale[1] {
        return Err(unsupported);
    }
    let basis = if group.transform.scale[0] == 0.0 {
        1.0
    } else {
        group.transform.scale[0] / 100.0
    };
    for key in &mut scale.keys {
        if key.values.len() != 3
            || key.easing.len() != 3
            || key.values[0] != key.values[1]
            || key.easing[0] != key.easing[1]
            || !key.values.iter().all(|value| value.is_finite())
            || !key.spatial_in.is_empty()
            || !key.spatial_out.is_empty()
        {
            return Err(unsupported);
        }
        // A uniform scalar commutes with the fixed SVD rotations. Preserve
        // authored knots/easing and Z; only multiply the two outer scale axes.
        for (value, factor) in key.values[..2].iter_mut().zip(factored_scale) {
            *value *= factor / basis;
            if !value.is_finite() {
                return Err(unsupported);
            }
        }
    }
    Ok(animations)
}

/// Factor a static drawable's affine transform without inventing source bounds.
/// Null parenting carries geometry, not opacity: this profile requires full opacity.
pub(in crate::export_document) fn lower_static_text(
    transform: &Transform,
    name: &str,
) -> Result<(SolidTransform, NullLayerSpec), &'static str> {
    let Position::TwoD(position) = transform.position else {
        return Err("Static Text skew requires a planar position");
    };
    if transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
        || transform.opacity.value() != 100.0
    {
        return Err(
            "Static Text skew requires planar full-opacity geometry; Null parents do not inherit opacity",
        );
    }
    if transform
        .anchor_point
        .into_iter()
        .chain(position)
        .any(|value| !value.is_finite())
    {
        return Err("Static Text skew Transform is non-finite");
    }
    let matrix = matrix(transform)?;
    let factors = Factors::from_matrix(matrix)?;
    factors.verify(matrix)?;
    let anchor = transform.anchor_point;
    Ok((
        SolidTransform {
            anchor,
            position,
            scale: [factors.first * 100.0, factors.second * 100.0],
            rotation: factors.outer_rotation.to_degrees(),
            opacity: 100.0,
        },
        NullLayerSpec {
            name: format!("{name} — Skew basis"),
            transform: SolidTransform {
                anchor,
                position: anchor,
                scale: [100.0; 2],
                rotation: factors.inner_rotation.to_degrees(),
                opacity: 100.0,
            },
            transform_animations: TransformAnimations::default(),
        },
    ))
}

pub(in crate::export_document) fn helper_options(helper_id: LayerId) -> NativeLayerOptions {
    NativeLayerOptions {
        fx_id: helper_id,
        parent: None,
        matte: None,
        enabled: true,
        adjustment_layer: false,
        motion_blur: false,
        blend_mode: 2,
        masks: Vec::new(),
        effects: Vec::new(),
        styles: Vec::new(),
        source_clock: None,
        transform_3d: None,
    }
}

/// Matches FX's `Affine::from_components_with_skew` linear matrix.
pub(super) fn matrix(transform: &Transform) -> Result<[f64; 4], &'static str> {
    matrix_components(
        transform.scale,
        transform.rotation,
        transform.skew,
        transform.skew_axis,
    )
}

pub(super) fn matrix_components(
    scale: [f64; 2],
    rotation: f64,
    skew: f64,
    skew_axis: f64,
) -> Result<[f64; 4], &'static str> {
    if scale
        .into_iter()
        .chain([rotation, skew, skew_axis])
        .any(|value| !value.is_finite())
    {
        return Err("Group Transform matrix is non-finite");
    }
    let rotation = rotation.to_radians();
    let axis = skew_axis.to_radians();
    let shear = -skew.clamp(-89.9, 89.9).to_radians().tan();
    let (axis_sin, axis_cos) = axis.sin_cos();
    let (outer_sin, outer_cos) = (rotation - axis).sin_cos();
    let scale = [scale[0] / 100.0, scale[1] / 100.0];
    let rotated_scale = [
        axis_cos * scale[0],
        -axis_sin * scale[1],
        axis_sin * scale[0],
        axis_cos * scale[1],
    ];
    let sheared = [
        rotated_scale[0] + shear * rotated_scale[2],
        rotated_scale[1] + shear * rotated_scale[3],
        rotated_scale[2],
        rotated_scale[3],
    ];
    let matrix = [
        outer_cos * sheared[0] - outer_sin * sheared[2],
        outer_cos * sheared[1] - outer_sin * sheared[3],
        outer_sin * sheared[0] + outer_cos * sheared[2],
        outer_sin * sheared[1] + outer_cos * sheared[3],
    ];
    if matrix.into_iter().all(f64::is_finite) {
        Ok(matrix)
    } else {
        Err("Group Transform matrix is non-finite")
    }
}

#[derive(Clone, Copy, Debug)]
struct Factors {
    outer_rotation: f64,
    first: f64,
    second: f64,
    inner_rotation: f64,
}

impl Factors {
    fn from_matrix([a, b, c, d]: [f64; 4]) -> Result<Self, &'static str> {
        let h00 = a.mul_add(a, c * c);
        let h01 = a.mul_add(b, c * d);
        let h11 = b.mul_add(b, d * d);
        let right_rotation = 0.5 * (2.0 * h01).atan2(h00 - h11);
        let (sin, cos) = right_rotation.sin_cos();
        let first_column = [a.mul_add(cos, b * sin), c.mul_add(cos, d * sin)];
        let first = first_column[0].hypot(first_column[1]);
        let magnitude = [a, b, c, d].into_iter().map(f64::abs).fold(0.0, f64::max);
        let determinant = a.mul_add(d, -(b * c));
        if !first.is_finite()
            || !determinant.is_finite()
            || first <= f64::EPSILON * magnitude.max(1.0)
            || determinant.abs() <= f64::EPSILON * magnitude.mul_add(magnitude, 1.0)
        {
            return Err("Static Group skew matrix is singular");
        }
        let result = Self {
            outer_rotation: first_column[1].atan2(first_column[0]),
            first,
            second: determinant / first,
            inner_rotation: -right_rotation,
        };
        if [
            result.outer_rotation,
            result.first,
            result.second,
            result.inner_rotation,
        ]
        .into_iter()
        .all(f64::is_finite)
        {
            Ok(result)
        } else {
            Err("Static Group skew decomposition is non-finite")
        }
    }

    fn matrix(self) -> [f64; 4] {
        let (outer_sin, outer_cos) = self.outer_rotation.sin_cos();
        let (inner_sin, inner_cos) = self.inner_rotation.sin_cos();
        [
            outer_cos * self.first * inner_cos - outer_sin * self.second * inner_sin,
            -outer_cos * self.first * inner_sin - outer_sin * self.second * inner_cos,
            outer_sin * self.first * inner_cos + outer_cos * self.second * inner_sin,
            -outer_sin * self.first * inner_sin + outer_cos * self.second * inner_cos,
        ]
    }

    fn verify(self, expected: [f64; 4]) -> Result<(), &'static str> {
        let actual = self.matrix();
        let scale = expected.into_iter().map(f64::abs).fold(1.0, f64::max);
        let tolerance = 256.0 * f64::EPSILON * scale;
        if actual
            .into_iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= tolerance)
        {
            Ok(())
        } else {
            Err("Static Group skew decomposition did not reconstruct its affine matrix")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_matrix_close(actual: [f64; 4], expected: [f64; 4]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
        }
    }

    #[test]
    fn signed_svd_reconstructs_shear_rotation_and_negative_determinant() {
        for matrix in [
            [1.2, 0.35, -0.4, 0.8],
            [-1.1, 0.2, 0.3, 0.9],
            [0.0, -2.0, 3.0, 0.0],
        ] {
            let factors = Factors::from_matrix(matrix).unwrap();
            factors.verify(matrix).unwrap();
            assert_matrix_close(factors.matrix(), matrix);
            assert_eq!(
                factors.first.signum() * factors.second.signum(),
                (matrix[0] * matrix[3] - matrix[1] * matrix[2]).signum()
            );
        }
    }

    #[test]
    fn static_text_skew_helpers_preserve_full_anchor_position_and_affine_mapping() {
        let map = |transform: &SolidTransform, point: [f64; 2]| {
            let angle = transform.rotation.to_radians();
            let x = (point[0] - transform.anchor[0]) * transform.scale[0] / 100.0;
            let y = (point[1] - transform.anchor[1]) * transform.scale[1] / 100.0;
            [
                transform.position[0] + angle.cos() * x - angle.sin() * y,
                transform.position[1] + angle.sin() * x + angle.cos() * y,
            ]
        };
        for skew in [1.0, -0.5, -2.5] {
            let mut source = crate::export_document::identity_fx_transform();
            source.anchor_point = [39.032, -32.125];
            source.position = Position::TwoD([540.575, 1064.583]);
            source.scale = [116.63, 97.804];
            source.rotation = 13.0;
            source.skew = skew;
            let (outer, inner) = lower_static_text(&source, "n").unwrap();
            let expected_matrix = matrix(&source).unwrap();
            for point in [[0.0, 0.0], [39.032, -32.125], [700.25, -49.5]] {
                let actual = map(&outer, map(&inner.transform, point));
                let delta = [
                    point[0] - source.anchor_point[0],
                    point[1] - source.anchor_point[1],
                ];
                let Position::TwoD(position) = source.position else {
                    panic!("planar");
                };
                let expected = [
                    position[0] + expected_matrix[0] * delta[0] + expected_matrix[1] * delta[1],
                    position[1] + expected_matrix[2] * delta[0] + expected_matrix[3] * delta[1],
                ];
                assert!(
                    actual
                        .into_iter()
                        .zip(expected)
                        .all(|(a, b)| (a - b).abs() < 1e-9)
                );
            }
        }
    }

    #[test]
    fn fixed_shear_factorization_is_exact_with_zero_overshoot_and_signed_nonuniform_scale() {
        let multiply = |a: [f64; 4], b: [f64; 4]| {
            [
                a[0] * b[0] + a[1] * b[2],
                a[0] * b[1] + a[1] * b[3],
                a[2] * b[0] + a[3] * b[2],
                a[2] * b[1] + a[3] * b[3],
            ]
        };
        for skew in [1.0, -0.5, -2.5] {
            let mut shear = crate::export_document::identity_fx_transform();
            shear.skew = skew;
            shear.skew_axis = 27.0;
            let (first, second) = lower_static_text(&shear, "fixed K").unwrap();
            let factors = multiply(
                matrix_components(first.scale, first.rotation, 0.0, 0.0).unwrap(),
                matrix_components(second.transform.scale, second.transform.rotation, 0.0, 0.0)
                    .unwrap(),
            );
            for scale in [[0.0, 0.0], [0.0, 180.0], [137.0, 82.0], [-40.0, 110.0]] {
                for rotation in [-14.0, -6.25, 0.0, 33.0] {
                    let actual = multiply(
                        matrix_components([100.0; 2], rotation, 0.0, 0.0).unwrap(),
                        multiply(factors, matrix_components(scale, 0.0, 0.0, 0.0).unwrap()),
                    );
                    assert_matrix_close(
                        actual,
                        matrix_components(scale, rotation, skew, 27.0).unwrap(),
                    );
                }
            }
        }
    }

    #[test]
    fn singular_matrix_fails_instead_of_approximating() {
        assert!(Factors::from_matrix([1.0, 2.0, 2.0, 4.0]).is_err());
    }
}
