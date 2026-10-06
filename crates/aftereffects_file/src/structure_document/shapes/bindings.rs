//! Native editable controls for separately owned vector paints.

#[cfg(test)]
mod tests;
use fx_schema::animator::{
    AnimationGraphEntry, AnimatorData, KeyframeId, PropertyKeyframe, PropertyKeyframeTrack,
};
use fx_schema::{LayerId, PropType, PropertyAnimator, PropertyTarget, PropertyValue};
use serde::ser::Error as _;

use crate::structure_document::animation_budget::{
    AnimationBudget, constant_entry_reservation_bytes, copied_animator_entry_reservation_bytes,
    path_constant_entry_reservation_bytes,
};

/// Copied controls need independent graph-wide key identities, not only a new target.
/// These animators are importer-authored; preserve their typed state and timing.
pub(in crate::structure_document) fn copy_animator(
    animator: &PropertyAnimator,
    target: &PropertyTarget,
    budget: &mut AnimationBudget,
) -> Result<Option<PropertyAnimator>, serde_json::Error> {
    let Ok(reservation) = copied_animator_entry_reservation_bytes(target, animator) else {
        return Ok(None);
    };
    let checkpoint = budget.checkpoint();
    if budget.reserve(reservation).is_err() {
        return Ok(None);
    }

    let mut data = animator.data().clone();
    let AnimatorData::Keyframes { track, .. } = &mut data else {
        return Ok(Some(animator.clone()));
    };
    let keys = track
        .keyframes()
        .iter()
        .enumerate()
        .map(|(index, key)| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("aep-copy-{target}-{index}")),
                key.layer_time(),
                key.value().clone(),
                key.easing(),
            )
            .with_spatial_tangents(key.spatial_in_tangent(), key.spatial_out_tangent())
        })
        .collect();
    *track = match PropertyKeyframeTrack::new(keys) {
        Ok(track) => track,
        Err(error) => {
            budget.rollback(checkpoint);
            return Err(serde_json::Error::custom(error));
        }
    };
    match PropertyAnimator::from_data(&data) {
        Ok(animator) => Ok(Some(animator)),
        Err(error) => {
            budget.rollback(checkpoint);
            Err(error)
        }
    }
}

/// Copy a semantically coupled visible-control batch as one budget transaction.
pub(super) fn copy_animators_atomically<'a>(
    animators: impl IntoIterator<Item = (&'a PropertyAnimator, PropertyTarget)>,
    budget: &mut AnimationBudget,
) -> Result<Option<Vec<AnimationGraphEntry>>, serde_json::Error> {
    let checkpoint = budget.checkpoint();
    let mut copied = Vec::new();
    for (animator, target) in animators {
        match copy_animator(animator, &target, budget) {
            Ok(Some(animator)) => copied.push(entry(target, animator, Vec::new())),
            Ok(None) => {
                budget.rollback(checkpoint);
                return Ok(None);
            }
            Err(error) => {
                budget.rollback(checkpoint);
                return Err(error);
            }
        }
    }
    Ok(Some(copied))
}

pub(super) fn constant(
    entries: &mut Vec<AnimationGraphEntry>,
    target: PropertyTarget,
    value: &PropertyValue,
    source_start: usize,
    budget: &mut AnimationBudget,
) -> Result<bool, serde_json::Error> {
    // Each fresh helper owns a contiguous suffix. Never rescan earlier layers
    // when deciding whether its native producer supersedes a static value.
    if entries[source_start..]
        .iter()
        .any(|entry| entry.target == target)
    {
        return Ok(true);
    }
    let Ok(reservation) = constant_entry_reservation_bytes(&target, value) else {
        return Ok(false);
    };
    let checkpoint = budget.checkpoint();
    if budget.reserve(reservation).is_err() {
        return Ok(false);
    }
    let animator = match PropertyAnimator::constant(value.clone()) {
        Ok(animator) => animator,
        Err(error) => {
            budget.rollback(checkpoint);
            return Err(error);
        }
    };
    entries.push(entry(target, animator, Vec::new()));
    Ok(true)
}

/// Preserve native editable parametric controls without a scripted outline.
/// ShapeContent.path is empty for ellipse and poly-star sources; a paint must
/// use their native shape fields or explicitly diagnose unsupported transport.
pub(super) fn outline(
    entries: &mut Vec<AnimationGraphEntry>,
    shape: &fx_schema::layer::ShapeLayer,
    source_start: usize,
    budget: &mut AnimationBudget,
) -> Result<bool, serde_json::Error> {
    let target = PropertyTarget::layer(shape.id, PropType::ShapePath);
    if entries[source_start..]
        .iter()
        .any(|entry| entry.target == target)
    {
        return Ok(true);
    }
    let controls = if let Some(ellipse) = &shape.shape.ellipse {
        vec![
            (PropType::EllipseSize, PropertyValue::Vector2(ellipse.size)),
            (
                PropType::EllipsePosition,
                PropertyValue::Vector2(ellipse.position),
            ),
        ]
    } else if let Some(star) = &shape.shape.poly_star {
        vec![
            (
                PropType::PolyStarPosition,
                PropertyValue::Vector2(star.position),
            ),
            (PropType::PolyStarPoints, PropertyValue::Float(star.points)),
            (
                PropType::PolyStarRotation,
                PropertyValue::Float(star.rotation),
            ),
            (
                PropType::PolyStarOuterRadius,
                PropertyValue::Float(star.outer_radius),
            ),
            (
                PropType::PolyStarInnerRadius,
                PropertyValue::Float(star.inner_radius),
            ),
            (
                PropType::PolyStarOuterRoundness,
                PropertyValue::Float(star.outer_roundness),
            ),
            (
                PropType::PolyStarInnerRoundness,
                PropertyValue::Float(star.inner_roundness),
            ),
        ]
    } else {
        let Ok(reservation) = path_constant_entry_reservation_bytes(&target, &shape.shape.path)
        else {
            return Ok(false);
        };
        let checkpoint = budget.checkpoint();
        if budget.reserve(reservation).is_err() {
            return Ok(false);
        }
        let animator =
            match PropertyAnimator::constant(PropertyValue::Path(shape.shape.path.clone())) {
                Ok(animator) => animator,
                Err(error) => {
                    budget.rollback(checkpoint);
                    return Err(error);
                }
            };
        entries.push(entry(target, animator, Vec::new()));
        return Ok(true);
    };
    for (property, value) in controls {
        if !constant(
            entries,
            PropertyTarget::layer(shape.id, property),
            &value,
            source_start,
            budget,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn mirror(
    entries: &mut Vec<AnimationGraphEntry>,
    target: PropertyTarget,
    source: PropertyTarget,
    value: PropertyValue,
    warnings: &mut Vec<String>,
    budget: &mut AnimationBudget,
) -> Result<bool, serde_json::Error> {
    // FX has only Constant/Keyframes/JsScript animators, not a native reference
    // animator. Copy native keys to the visible target rather than emit JS.
    let source_entry = entries.iter().rev().find(|entry| entry.target == source);
    if let Some(source_entry) = source_entry {
        if source_entry.animator.is_js_script() || !source_entry.dependencies.is_empty() {
            warnings.push(format!(
                "{source:?} has an animator that cannot be copied to {target:?} as an editable native control; required mirrored control set omitted"
            ));
            return Ok(false);
        }
        match copy_animator(&source_entry.animator, &target, budget) {
            Ok(Some(animator)) => {
                let message = "Native FX lacks a reference animator; source keys are copied to the visible target, so later edits to the hidden producer will not propagate";
                if !warnings.iter().any(|warning| warning == message) {
                    warnings.push(message.into());
                }
                entries.push(entry(target, animator, Vec::new()));
                Ok(true)
            }
            Ok(None) => {
                warnings.push(format!(
                    "{source:?} native keys could not be copied to {target:?} within the generated-animation allowance; required mirrored control set omitted"
                ));
                Ok(false)
            }
            Err(error) => {
                warnings.push(format!(
                    "{source:?} native keys could not be copied to {target:?}: {error}; required mirrored control set omitted"
                ));
                Ok(false)
            }
        }
    } else {
        warnings.push(format!(
            "{source:?} has no editable native animator to bind to {target:?}; current static value retained"
        ));
        let source_start = entries.len();
        constant(entries, target, &value, source_start, budget)
    }
}

pub(super) fn entry(
    target: PropertyTarget,
    animator: PropertyAnimator,
    dependencies: Vec<PropertyTarget>,
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target,
        animator,
        dependencies,
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

/// A contour and the inner-to-outer vector-group transforms between its owner
/// and the paint owner. Opacity deliberately isn't part of exported geometry.
pub(super) struct OutlineInput {
    pub transforms: Vec<LayerId>,
    /// Round-only stages in each scope, before that scope's outgoing transform.
    pub rounds: Vec<Vec<LayerId>>,
}

pub(super) fn append(
    target: LayerId,
    inputs: &[OutlineInput],
    path: &fx_schema::layer::ShapePath,
    budget: &mut AnimationBudget,
) -> Result<Option<AnimationGraphEntry>, String> {
    if inputs.iter().any(|input| {
        !input.transforms.is_empty() || input.rounds.iter().any(|stage| !stage.is_empty())
    }) {
        return Err("paint has transformed or round-staged shared outlines that cannot be represented by native FX Path tracks; paint omitted".into());
    }
    let target = PropertyTarget::layer(target, PropType::ShapePath);
    let Ok(reservation) = path_constant_entry_reservation_bytes(&target, path) else {
        return Ok(None);
    };
    let checkpoint = budget.checkpoint();
    if budget.reserve(reservation).is_err() {
        return Ok(None);
    }
    let animator = match PropertyAnimator::constant(PropertyValue::Path(path.clone())) {
        Ok(animator) => animator,
        Err(error) => {
            budget.rollback(checkpoint);
            return Err(format!(
                "static compound Path cannot be represented: {error}"
            ));
        }
    };
    Ok(Some(entry(target, animator, Vec::new())))
}
