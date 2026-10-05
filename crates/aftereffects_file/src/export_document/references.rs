//! Whole-document typed layer-reference analysis for source-variant planning.
//!
//! This walker reads only canonical `fx_schema` fields. It deliberately does
//! not inspect arbitrary JSON strings for values that merely resemble layer
//! identifiers.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use fx_schema::{
    EffectData, EffectId, FxItemId, Layer, LayerData, LayerId, PropertyTarget,
    animator::AnimationGraphEntry,
};

/// Reference facts that decide whether one source layer may become multiple
/// native occurrences without changing another canonical relationship.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::export_document) struct SourceVariantEligibility {
    pub nested_or_parented: bool,
    pub owns_masks_or_matte: bool,
    pub referenced_as_parent: bool,
    /// Excludes direct child containment, which travels with a Group subtree.
    pub referenced_as_noncontainer_parent: bool,
    pub referenced_as_matte: bool,
    pub referenced_as_mask_guide: bool,
    pub referenced_as_text_guide: bool,
    pub referenced_as_ai_edit_source: bool,
    pub referenced_as_segment: bool,
    pub referenced_by_animation_dependency: bool,
    pub referenced_by_animation_layer_ref: bool,
    /// A dependency addressed an effect/FX-item whose typed owner could not be
    /// established. The planner must not assume that it belongs elsewhere.
    pub has_unresolved_animation_dependency: bool,
}

/// Malformed identity data that makes occurrence allocation unsafe for the
/// complete document, rather than merely unsupported for one layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::export_document) enum ReferenceAnalysisError {
    Layer(LayerId),
    Effect(EffectId),
    MaskItem(FxItemId),
}

impl fmt::Display for ReferenceAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layer(id) => write!(formatter, "duplicate FX layer identity {id}"),
            Self::Effect(id) => write!(formatter, "duplicate FX effect identity {id}"),
            Self::MaskItem(id) => {
                write!(formatter, "duplicate FX mask-item identity {id}")
            }
        }
    }
}

/// Canonical inbound-reference sets for one complete FX composition.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DocumentReferenceFacts {
    layer_ids: BTreeSet<LayerId>,
    nested_or_parented: BTreeSet<LayerId>,
    owns_masks_or_matte: BTreeSet<LayerId>,
    parents: BTreeSet<LayerId>,
    noncontainer_parents: BTreeSet<LayerId>,
    mattes: BTreeSet<LayerId>,
    mask_guides: BTreeSet<LayerId>,
    text_guides: BTreeSet<LayerId>,
    ai_edit_sources: BTreeSet<LayerId>,
    segments: BTreeSet<LayerId>,
    animation_dependencies: BTreeSet<LayerId>,
    animation_layer_refs: BTreeSet<LayerId>,
    // Includes dangling targets: allocating a helper with one of these IDs
    // would turn an unresolved animator/reference into an unrelated binding.
    animation_reserved_ids: BTreeSet<LayerId>,
    has_unresolved_animation_dependency: bool,
}

impl DocumentReferenceFacts {
    /// Walks the actual typed tree and graph before any occurrence identity is
    /// allocated. Duplicate identities are fatal because reminting around an
    /// ambiguous original cannot preserve references deterministically.
    pub(super) fn analyze(
        layers: &[Layer],
        segment_layer_ids: &[LayerId],
        dynamics: &[AnimationGraphEntry],
    ) -> Result<Self, ReferenceAnalysisError> {
        let mut facts = Self::default();
        let mut effect_owners = BTreeMap::new();
        let mut mask_item_owners = BTreeMap::new();
        collect_layers(
            layers,
            None,
            &mut facts,
            &mut effect_owners,
            &mut mask_item_owners,
        )?;
        facts.segments.extend(segment_layer_ids.iter().copied());

        for entry in dynamics {
            facts.animation_reserved_ids.extend(
                std::iter::once(&entry.target)
                    .chain(entry.dependencies.iter())
                    .chain(entry.random_seed_target.iter())
                    .filter_map(PropertyTarget::layer_id),
            );
            for target in entry
                .dependencies
                .iter()
                .chain(entry.random_seed_target.iter())
            {
                match target_owner(target, &effect_owners, &mask_item_owners) {
                    Some(layer_id) if facts.layer_ids.contains(&layer_id) => {
                        facts.animation_dependencies.insert(layer_id);
                    }
                    Some(_) | None => facts.has_unresolved_animation_dependency = true,
                }
            }
            facts.animation_layer_refs.extend(
                entry
                    .layer_refs
                    .values()
                    .map(|reference| reference.layer_id),
            );
        }
        Ok(facts)
    }

    pub(super) fn occupied_layer_ids(&self) -> BTreeSet<LayerId> {
        // Reserve references as well as definitions, without pretending that
        // dangling IDs have owners for source-variant eligibility analysis.
        [
            &self.layer_ids,
            &self.parents,
            &self.mattes,
            &self.mask_guides,
            &self.text_guides,
            &self.ai_edit_sources,
            &self.segments,
            &self.animation_layer_refs,
            &self.animation_reserved_ids,
        ]
        .into_iter()
        .flat_map(|ids| ids.iter().copied())
        .collect()
    }

    pub(super) fn eligibility(&self, layer_id: LayerId) -> SourceVariantEligibility {
        SourceVariantEligibility {
            nested_or_parented: self.nested_or_parented.contains(&layer_id),
            owns_masks_or_matte: self.owns_masks_or_matte.contains(&layer_id),
            referenced_as_parent: self.parents.contains(&layer_id),
            referenced_as_noncontainer_parent: self.noncontainer_parents.contains(&layer_id),
            referenced_as_matte: self.mattes.contains(&layer_id),
            referenced_as_mask_guide: self.mask_guides.contains(&layer_id),
            referenced_as_text_guide: self.text_guides.contains(&layer_id),
            referenced_as_ai_edit_source: self.ai_edit_sources.contains(&layer_id),
            referenced_as_segment: self.segments.contains(&layer_id),
            referenced_by_animation_dependency: self.animation_dependencies.contains(&layer_id),
            referenced_by_animation_layer_ref: self.animation_layer_refs.contains(&layer_id),
            has_unresolved_animation_dependency: self.has_unresolved_animation_dependency,
        }
    }
}

fn collect_layers(
    layers: &[Layer],
    container: Option<LayerId>,
    facts: &mut DocumentReferenceFacts,
    effect_owners: &mut BTreeMap<EffectId, LayerId>,
    mask_item_owners: &mut BTreeMap<FxItemId, LayerId>,
) -> Result<(), ReferenceAnalysisError> {
    for layer in layers {
        let layer_id = layer.id();
        if !facts.layer_ids.insert(layer_id) {
            return Err(ReferenceAnalysisError::Layer(layer_id));
        }
        if let Some(parent) = layer.parent_id() {
            facts.nested_or_parented.insert(layer_id);
            facts.parents.insert(parent);
            if Some(parent) != container {
                facts.noncontainer_parents.insert(parent);
            }
        }
        for effect in layer.effects() {
            if let EffectData::Identified { id, .. } = effect.data()
                && effect_owners.insert(*id, layer_id).is_some()
            {
                return Err(ReferenceAnalysisError::Effect(*id));
            }
        }
        let (matte, masks) = matte_and_masks(layer);
        if matte.is_some() || !masks.is_empty() {
            facts.owns_masks_or_matte.insert(layer_id);
        }
        if let Some(matte) = matte {
            facts.mattes.insert(matte);
        }
        for (item_id, guide) in masks {
            if mask_item_owners.insert(item_id, layer_id).is_some() {
                return Err(ReferenceAnalysisError::MaskItem(item_id));
            }
            if let Some(guide) = guide {
                facts.mask_guides.insert(guide);
            }
        }
        match layer.data() {
            LayerData::Text(text) => {
                if let Some(path) = &text.path_options {
                    facts.text_guides.insert(path.path_layer);
                }
            }
            LayerData::AiEdit(ai_edit) => {
                facts.ai_edit_sources.insert(ai_edit.source_layer_id);
            }
            _ => {}
        }
        if let Some(children) = layer.child_layers() {
            collect_layers(
                children,
                Some(layer_id),
                facts,
                effect_owners,
                mask_item_owners,
            )?;
        }
    }
    Ok(())
}

fn matte_and_masks(layer: &Layer) -> (Option<LayerId>, Vec<(FxItemId, Option<LayerId>)>) {
    macro_rules! refs {
        ($value:expr) => {{
            (
                $value.track_matte.as_ref().map(|matte| matte.layer),
                $value
                    .masks
                    .iter()
                    .map(|mask| (mask.id, mask.layer))
                    .collect(),
            )
        }};
    }
    match layer.data() {
        LayerData::Media(value) => refs!(value),
        LayerData::Text(value) => refs!(value),
        LayerData::Video(value) => refs!(value),
        LayerData::Image(value) => refs!(value),
        LayerData::Rect(value) => refs!(value),
        LayerData::Shape(value) => refs!(value),
        LayerData::Group(value) => refs!(value),
        LayerData::BooleanOperation(value) => refs!(value),
        LayerData::Adjustment(value) => refs!(value),
        LayerData::Pag(_) | LayerData::Audio(_) | LayerData::AiEdit(_) => (None, Vec::new()),
    }
}

fn target_owner(
    target: &PropertyTarget,
    effect_owners: &BTreeMap<EffectId, LayerId>,
    mask_item_owners: &BTreeMap<FxItemId, LayerId>,
) -> Option<LayerId> {
    target
        .layer_id()
        .or_else(|| {
            target
                .effect_id()
                .and_then(|id| effect_owners.get(&id).copied())
        })
        .or_else(|| {
            target
                .fx_item_id()
                .and_then(|id| mask_item_owners.get(&id).copied())
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use fx_schema::{
        LayerRef, PropType, PropertyTarget, PropertyValue,
        animator::{AnimationGraphEntry, PropertyAnimator},
    };
    use serde_json::json;

    use super::*;

    fn image(id: u64, parent: Option<u64>, extra: serde_json::Value) -> Layer {
        let mut value = json!({
            "type": "Image", "id": id, "name": format!("image-{id}"),
            "parent": parent,
            "activeRange": {"start": 0, "duration": 1000},
            "transform": {
                "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
                "rotation": 0, "opacity": 100
            },
            "source": {"assetId": format!("asset-{id}"), "fit": "contain"}
        });
        for (key, field) in extra.as_object().unwrap() {
            value[key] = field.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn canonical_tree_and_graph_references_are_classified_by_typed_ids() {
        let parent = image(1, None, json!({}));
        let child = image(
            2,
            Some(1),
            json!({
                "layerMask": {"mode": "alpha", "layer": 3},
                "masks": [{"id": 91, "mode": "add", "layer": 4}]
            }),
        );
        let matte = image(3, None, json!({}));
        let guide = image(4, None, json!({}));
        let mut layer_refs = BTreeMap::new();
        layer_refs.insert(
            "source".to_owned(),
            LayerRef {
                layer_id: LayerId::new(4),
            },
        );
        let dynamics = vec![AnimationGraphEntry {
            target: PropertyTarget::layer(LayerId::new(2), PropType::Opacity),
            animator: PropertyAnimator::constant(PropertyValue::Float(1.0)).unwrap(),
            dependencies: vec![PropertyTarget::layer(LayerId::new(3), PropType::Opacity)],
            random_seed_target: None,
            layer_refs,
        }];
        let facts = DocumentReferenceFacts::analyze(
            &[parent, child, matte, guide],
            &[LayerId::new(2)],
            &dynamics,
        )
        .unwrap();

        assert!(facts.eligibility(LayerId::new(1)).referenced_as_parent);
        let child = facts.eligibility(LayerId::new(2));
        assert!(
            child.nested_or_parented && child.owns_masks_or_matte && child.referenced_as_segment
        );
        let matte = facts.eligibility(LayerId::new(3));
        assert!(matte.referenced_as_matte && matte.referenced_by_animation_dependency);
        let guide = facts.eligibility(LayerId::new(4));
        assert!(guide.referenced_as_mask_guide && guide.referenced_by_animation_layer_ref);
    }

    #[test]
    fn twirl_plane_parent_admission_distinguishes_containment_from_external_edges() {
        let native = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/effects_coverage/native_static_controls.aep"
        ))
        .unwrap();
        let imported =
            crate::structure_document::to_structural_fx_document(&native, Some(625)).unwrap();
        let composition = imported.document.composition();
        let LayerData::Group(root) = composition.layers()[0].data() else {
            panic!("root Group");
        };
        let owner = root.layers[0].id();
        let facts = DocumentReferenceFacts::analyze(
            composition.layers(),
            &[],
            composition.dynamics().entries(),
        )
        .unwrap();
        assert!(facts.eligibility(owner).referenced_as_parent);
        assert!(
            !facts.eligibility(owner).referenced_as_noncontainer_parent,
            "source clock containment stays inside the Twirl carrier"
        );

        let mut layers = composition.layers().to_vec();
        layers.push(image(90000, Some(owner.value()), json!({})));
        let facts = DocumentReferenceFacts::analyze(&layers, &[], composition.dynamics().entries())
            .unwrap();
        assert!(
            facts.eligibility(owner).referenced_as_noncontainer_parent,
            "a real external parent consumer must still block carrier admission"
        );
    }

    #[test]
    fn text_path_and_ai_edit_source_fields_are_walked_without_string_guessing() {
        let guide = image(11, None, json!({}));
        let text: Layer = serde_json::from_value(json!({
            "type": "Text", "id": 12, "name": "path text", "parent": null,
            "activeRange": {"start": 0, "duration": 1000},
            "transform": {
                "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
                "rotation": 0, "opacity": 100
            },
            "sourceText": {
                "text": "typed id 999 is not a reference", "fontFamily": "Inter",
                "fontSize": 20, "fillColor": [1, 1, 1, 1]
            },
            "pathOptions": {"id": 92, "pathLayer": 11}
        }))
        .unwrap();
        let source = image(21, Some(20), json!({}));
        let ai_edit: Layer = serde_json::from_value(json!({
            "type": "AiEdit", "id": 20, "name": "segment", "parent": null,
            "activeRange": {"start": 0, "duration": 1000},
            "styleId": "style", "sourceLayerId": 21, "layers": [source]
        }))
        .unwrap();
        let facts = DocumentReferenceFacts::analyze(&[guide, text, ai_edit], &[], &[]).unwrap();
        assert!(facts.occupied_layer_ids().contains(&LayerId::new(11)));
        assert!(facts.occupied_layer_ids().contains(&LayerId::new(21)));
        assert!(facts.eligibility(LayerId::new(11)).referenced_as_text_guide);
        assert!(
            facts
                .eligibility(LayerId::new(21))
                .referenced_as_ai_edit_source
        );
        assert!(
            !facts
                .eligibility(LayerId::new(999))
                .referenced_as_text_guide
        );
    }

    #[test]
    fn review_audit_all_referenced_layer_ids_are_reserved_even_when_dangling() {
        let layer = image(
            1,
            Some(2),
            json!({
                "layerMask": {"mode": "alpha", "layer": 3},
                "masks": [{"id": 91, "mode": "add", "layer": 4}]
            }),
        );
        let entry = AnimationGraphEntry {
            target: PropertyTarget::layer(LayerId::new(5), PropType::Opacity),
            animator: PropertyAnimator::constant(PropertyValue::Float(1.0)).unwrap(),
            dependencies: vec![PropertyTarget::layer(LayerId::new(6), PropType::Opacity)],
            random_seed_target: Some(PropertyTarget::layer(LayerId::new(7), PropType::Opacity)),
            layer_refs: BTreeMap::from([(
                "source".into(),
                LayerRef {
                    layer_id: LayerId::new(8),
                },
            )]),
        };
        let facts =
            DocumentReferenceFacts::analyze(&[layer], &[LayerId::new(9)], &[entry]).unwrap();
        assert_eq!(
            facts.occupied_layer_ids(),
            (1..=9).map(LayerId::new).collect()
        );
        assert!(
            facts
                .eligibility(LayerId::new(1))
                .has_unresolved_animation_dependency
        );
    }

    #[test]
    fn duplicate_layer_identity_is_a_fatal_preflight_error() {
        let layers = [image(7, None, json!({})), image(7, None, json!({}))];
        assert_eq!(
            DocumentReferenceFacts::analyze(&layers, &[], &[]),
            Err(ReferenceAnalysisError::Layer(LayerId::new(7)))
        );
    }

    #[test]
    fn unresolved_dynamic_owner_is_conservative_for_every_candidate() {
        let layer = image(8, None, json!({}));
        let dynamics = vec![AnimationGraphEntry {
            target: PropertyTarget::layer(LayerId::new(8), PropType::Opacity),
            animator: PropertyAnimator::constant(PropertyValue::Float(1.0)).unwrap(),
            dependencies: vec![PropertyTarget::fx_item(FxItemId::new(404), "opacity")],
            random_seed_target: None,
            layer_refs: BTreeMap::new(),
        }];
        let facts = DocumentReferenceFacts::analyze(&[layer], &[], &dynamics).unwrap();
        assert!(
            facts
                .eligibility(LayerId::new(8))
                .has_unresolved_animation_dependency
        );
    }
}
