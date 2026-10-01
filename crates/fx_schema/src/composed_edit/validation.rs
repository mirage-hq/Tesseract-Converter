//! Pure structural validation for canonical editable compositions.
use crate::LayerData;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    pag_validation::{validate_pag_layer_references, validate_pag_layer_tree},
    property::{PropType, Property, PropertyTarget},
    Layer, LayerId,
};

use super::{FXComposition, ValidationError};

pub(super) fn validate(composition: &FXComposition) -> Result<(), ValidationError> {
    composition
        .layers()
        .iter()
        .try_for_each(Layer::validate_timing_ranges)
        .map_err(ValidationError::LayerTiming)?;
    validate_layer_hierarchy(composition)?;
    validate_media_placements_root(composition.layers())?;
    validate_graph_dependencies(composition)?;
    validate_graph_asset_metadata_sources(composition)?;
    Ok(())
}

fn validate_layer_hierarchy(composition: &FXComposition) -> Result<(), ValidationError> {
    fn count_source_media(layers: &[Layer], source_id: LayerId) -> (usize, usize) {
        let mut matching_layers = 0;
        let mut matching_media = 0;
        for layer in layers {
            if layer.id() == source_id {
                matching_layers += 1;
                if matches!(
                    layer.data(),
                    LayerData::Video(_) | LayerData::Image(_) | LayerData::Media(_)
                ) {
                    matching_media += 1;
                }
            }
            if let Some(children) = layer.child_layers() {
                let (child_layers, child_media) = count_source_media(children, source_id);
                matching_layers += child_layers;
                matching_media += child_media;
            }
        }
        (matching_layers, matching_media)
    }

    fn validate_child_parents(parent_id: LayerId, layers: &[Layer]) -> Result<(), String> {
        for layer in layers {
            if layer.parent_id() != Some(parent_id) {
                return Err(format!(
                    "AI Edit child layer {} must store parent {}",
                    layer.id(),
                    parent_id
                ));
            }
            if let Some(children) = layer.child_layers() {
                validate_child_parents(layer.id(), children)?;
            }
        }
        Ok(())
    }

    fn validate_ai_edit(layer: &Layer, is_root: bool) -> Result<(), String> {
        if let LayerData::AiEdit(ai_edit) = layer.data() {
            if !is_root {
                return Err(format!(
                    "AI Edit layer {} must remain a root layer until semantic-folder rendering supports container compositing",
                    ai_edit.id
                ));
            }
            if ai_edit.style_id.is_empty() {
                return Err(format!(
                    "AI Edit layer {} requires a non-empty styleId",
                    ai_edit.id
                ));
            }
            if ai_edit
                .shot_style_id
                .as_ref()
                .is_some_and(|shot_style_id| shot_style_id.trim().is_empty())
            {
                return Err(format!(
                    "AI Edit layer {} requires shotStyleId to be non-empty when present",
                    ai_edit.id
                ));
            }
            let (matching_layers, matching_media) =
                count_source_media(&ai_edit.layers, ai_edit.source_layer_id);
            if matching_layers != 1 || matching_media != 1 {
                return Err(format!(
                    "AI Edit layer {} sourceLayerId {} must identify exactly one nested Video or Image layer",
                    ai_edit.id, ai_edit.source_layer_id
                ));
            }
            validate_child_parents(ai_edit.id, &ai_edit.layers)?;
        }
        if let Some(children) = layer.child_layers() {
            children
                .iter()
                .try_for_each(|child| validate_ai_edit(child, false))?;
        }
        Ok(())
    }

    let mut available_layers = Vec::new();
    collect_all_layers(composition.layers(), &mut available_layers);
    let available_layers = available_layers
        .into_iter()
        .map(|layer| (layer.id(), layer))
        .collect::<BTreeMap<_, _>>();
    let mut source_addressable_layer_ids = BTreeSet::new();
    collect_soloable_layer_ids(composition.layers(), &mut source_addressable_layer_ids, 0);

    composition.layers().iter().try_for_each(|layer| {
        validate_pag_layer_tree(layer, true, false)
            .map_err(|error| ValidationError::LayerHierarchy(error.to_string()))?;
        validate_pag_layer_references(layer, &available_layers, &source_addressable_layer_ids)
            .map_err(|error| ValidationError::LayerHierarchy(error.to_string()))?;
        validate_ai_edit(layer, true).map_err(ValidationError::LayerHierarchy)
    })
}

fn collect_all_layers<'a>(layers: &'a [Layer], collected: &mut Vec<&'a Layer>) {
    for layer in layers {
        collected.push(layer);
        if let Some(children) = layer.child_layers() {
            collect_all_layers(children, collected);
        }
    }
}

fn collect_soloable_layer_ids(layers: &[Layer], ids: &mut BTreeSet<LayerId>, depth: usize) {
    const MAX_GROUP_DEPTH: usize = 256;

    for layer in layers {
        ids.insert(layer.id());
        let children = match layer.data() {
            LayerData::Group(group) => Some(group.layers.as_slice()),
            LayerData::AiEdit(ai_edit) => Some(ai_edit.layers.as_slice()),
            _ => None,
        };
        if let Some(children) = children {
            if depth < MAX_GROUP_DEPTH {
                collect_soloable_layer_ids(children, ids, depth + 1);
            }
        }
    }
}

fn validate_media_placements_root(layers: &[Layer]) -> Result<(), ValidationError> {
    fn reject_nested(layer: &Layer) -> Result<(), ValidationError> {
        if matches!(layer.data(), LayerData::Video(video) if video.placement.is_some())
            || matches!(layer.data(), LayerData::Image(image) if image.placement.is_some())
        {
            return Err(ValidationError::NestedMediaPlacement(layer.id()));
        }
        if let Some(children) = layer.child_layers() {
            children.iter().try_for_each(reject_nested)?;
        }
        Ok(())
    }

    for root in layers {
        if let Some(children) = root.child_layers() {
            children.iter().try_for_each(reject_nested)?;
        }
    }
    Ok(())
}

fn find_layer(layers: &[Layer], layer_id: LayerId) -> Option<&Layer> {
    for layer in layers {
        if layer.id() == layer_id {
            return Some(layer);
        }
        if let Some(found) = layer
            .child_layers()
            .and_then(|children| find_layer(children, layer_id))
        {
            return Some(found);
        }
    }
    None
}

fn validate_graph_dependencies(composition: &FXComposition) -> Result<(), ValidationError> {
    composition
        .dynamics()
        .entries()
        .iter()
        .flat_map(|entry| entry.dependencies.iter())
        .filter_map(PropertyTarget::as_property)
        .try_for_each(|dependency| validate_read_only_dependency(composition, dependency))
}

fn validate_read_only_dependency(
    composition: &FXComposition,
    dependency: Property,
) -> Result<(), ValidationError> {
    if dependency.property_type() != PropType::SourceRange {
        return Ok(());
    }
    let layer_id = dependency.layer_id();
    let layer = find_layer(composition.layers(), layer_id)
        .ok_or(ValidationError::MissingLayer(layer_id))?;
    if layer.supports_source_range() {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedReadOnlyProperty {
            layer_id,
            layer_type: layer.layer_type_name(),
            property_type: dependency.property_type(),
        })
    }
}

fn validate_graph_asset_metadata_sources(
    composition: &FXComposition,
) -> Result<(), ValidationError> {
    composition
        .dynamics()
        .entries()
        .iter()
        .flat_map(|entry| entry.layer_refs.values())
        .try_for_each(|input| {
            let layer = find_layer(composition.layers(), input.layer_id)
                .ok_or(ValidationError::MissingLayer(input.layer_id))?;
            if matches!(
                layer.data(),
                LayerData::Video(_)
                    | LayerData::Audio(_)
                    | LayerData::Image(_)
                    | LayerData::Media(_)
            ) {
                Ok(())
            } else {
                Err(ValidationError::UnsupportedAssetMetadataSource {
                    layer_id: input.layer_id,
                    layer_type: layer.layer_type_name(),
                })
            }
        })
}
