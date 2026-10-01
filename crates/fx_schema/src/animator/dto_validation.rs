//! Structural constraints shared by stored graph entries.

use super::dto::{AnimatorData, PropertyAnimator};
use super::dto_graph::AnimationGraphEntry;
use super::keyframes::validate_disabled_keyframe_value;
use super::{AnimationGraphError, PropertyKeyframeError};
use crate::{
    asset_metadata::{LayerRefMap, MAX_ANIMATOR_LAYER_REFS},
    PropType, PropertyTarget,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate_entries(entries: &[AnimationGraphEntry]) -> Result<(), AnimationGraphError> {
    for entry in entries {
        ensure_enumerable_animator(&entry.target, &entry.animator)?;
        validate_layer_refs(&entry.target, &entry.animator, &entry.layer_refs)?;
    }
    validate_keyframe_invariants(entries)?;
    validate_asset_metadata_source_invariants(entries)
}

fn ensure_enumerable_animator(
    target: &PropertyTarget,
    animator: &PropertyAnimator,
) -> Result<(), AnimationGraphError> {
    // Only fixed layer properties carry the read-only / finite-range
    // classification. An effect param is always a writable `Float` scalar, so it
    // bypasses both gates — there is no `PropType` to consult.
    let Some(property) = target.as_property() else {
        return Ok(());
    };
    // Read-only properties (derived sources and timeline facts) are valid as
    // *dependencies* only — they are seeded externally, never produced by a
    // graph node. Reject an animator that targets one before any range checks.
    if property.property_type().is_read_only() {
        return Err(AnimationGraphError::ReadOnlyProperty(target.clone()));
    }
    let Some(required_kind) = property.property_type().finite_range_value_kind() else {
        return Ok(());
    };
    let Some(range) = animator.finite_value_range() else {
        return Err(AnimationGraphError::UnboundedAnimator {
            property: target.clone(),
            animator_kind: animator.kind_label(),
        });
    };
    if let Some(mismatch) = range.iter().find(|value| value.kind() != required_kind) {
        return Err(AnimationGraphError::NonEnumerableValue {
            property: target.clone(),
            expected: required_kind.label(),
            found: mismatch.kind().label(),
        });
    }
    Ok(())
}

fn validate_keyframe_invariants(
    entries: &[AnimationGraphEntry],
) -> Result<(), AnimationGraphError> {
    let mut ids = BTreeMap::<&str, &PropertyTarget>::new();
    for entry in entries {
        let AnimatorData::Keyframes {
            track,
            enabled,
            disabled_value,
            ..
        } = entry.animator.data()
        else {
            continue;
        };
        if !entry.dependencies.is_empty() {
            return Err(AnimationGraphError::InvalidKeyframes {
                property: entry.target.clone(),
                source: PropertyKeyframeError::UnexpectedDependencies,
            });
        }
        match (*enabled, disabled_value) {
            (true, None) => {}
            (false, Some(value)) => validate_disabled_keyframe_value(&entry.target, value)
                .map_err(|source| AnimationGraphError::InvalidKeyframes {
                    property: entry.target.clone(),
                    source,
                })?,
            (true, Some(_)) => {
                return Err(AnimationGraphError::InvalidKeyframes {
                    property: entry.target.clone(),
                    source: PropertyKeyframeError::UnexpectedDisabledValue,
                });
            }
            (false, None) => {
                return Err(AnimationGraphError::InvalidKeyframes {
                    property: entry.target.clone(),
                    source: PropertyKeyframeError::DisabledValueRequired,
                });
            }
        }
        track.validate_for_target(&entry.target).map_err(|source| {
            AnimationGraphError::InvalidKeyframes {
                property: entry.target.clone(),
                source,
            }
        })?;
        for keyframe in track.keyframes() {
            if let Some(first_property) = ids.insert(keyframe.id().as_str(), &entry.target) {
                return Err(AnimationGraphError::DuplicateKeyframeId {
                    keyframe_id: keyframe.id().as_str().to_owned(),
                    first_property: first_property.clone(),
                    second_property: entry.target.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_layer_refs(
    target: &PropertyTarget,
    animator: &PropertyAnimator,
    layer_refs: &LayerRefMap,
) -> Result<(), AnimationGraphError> {
    if layer_refs.len() > MAX_ANIMATOR_LAYER_REFS {
        return Err(AnimationGraphError::TooManyLayerRefs {
            property: target.clone(),
            actual: layer_refs.len(),
            maximum: MAX_ANIMATOR_LAYER_REFS,
        });
    }
    if !layer_refs.is_empty() && !animator.is_js_script() {
        return Err(AnimationGraphError::LayerRefsRequireScript {
            property: target.clone(),
        });
    }
    Ok(())
}

fn validate_asset_metadata_source_invariants(
    entries: &[AnimationGraphEntry],
) -> Result<(), AnimationGraphError> {
    let animated_source_layers = entries
        .iter()
        .filter_map(|entry| entry.target.as_property())
        .filter(|property| {
            matches!(
                property.property_type(),
                PropType::MediaSourceAssetId | PropType::AudioSourceAssetId
            )
        })
        .map(|property| property.layer_id())
        .collect::<BTreeSet<_>>();
    entries
        .iter()
        .flat_map(|entry| entry.layer_refs.values())
        .find(|layer_ref| animated_source_layers.contains(&layer_ref.layer_id))
        .map_or(Ok(()), |layer_ref| {
            Err(AnimationGraphError::AnimatedLayerRefSource {
                layer_id: layer_ref.layer_id,
            })
        })
}
