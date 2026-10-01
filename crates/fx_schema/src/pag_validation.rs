//! Checked PAG-layer invariants shared by canonical document readers and runtime mutations.
use crate::LayerData;

use std::collections::{BTreeMap, BTreeSet};

use crate::{Layer, LayerId};

/// A structural PAG-layer invariant violation.
///
/// The variants preserve the canonical validation order and let mutation APIs
/// translate failures without duplicating PAG validation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PagLayerInvariantError {
    #[error(
        "PAG layer {0} must remain a root or direct child of a root AI Edit layer until host PAG rendering supports container compositing"
    )]
    ParentUnsupported(LayerId),
    #[error("PAG layer {0} opacity must be finite and between 0 and 1")]
    InvalidOpacity(LayerId),
    #[error(
        "PAG layer {0} insert IDs must be non-empty and unique per type, and each slot must reference an insert of the matching type"
    )]
    InvalidInsertSlots(LayerId),
    #[error("PAG layer {layer_id} {reason}")]
    InvalidContent {
        layer_id: LayerId,
        reason: &'static str,
    },
    #[error("PAG layer {pag_layer_id} references missing layer {referenced_layer_id}")]
    SourceNotFound {
        pag_layer_id: LayerId,
        referenced_layer_id: LayerId,
    },
    #[error("PAG layer {pag_layer_id} must not reference itself as source media")]
    SelfReference { pag_layer_id: LayerId },
    #[error(
        "PAG layer {pag_layer_id} source {referenced_layer_id} must be a Video or Image layer"
    )]
    SourceNotMedia {
        pag_layer_id: LayerId,
        referenced_layer_id: LayerId,
    },
    #[error(
        "PAG layer {pag_layer_id} source {referenced_layer_id} is not addressable by the media-frame renderer"
    )]
    SourceNotAddressable {
        pag_layer_id: LayerId,
        referenced_layer_id: LayerId,
    },
    #[error(
        "PAG layer {pag_layer_id} transition sound {referenced_layer_id} must be an Audio layer"
    )]
    SoundLayerNotAudio {
        pag_layer_id: LayerId,
        referenced_layer_id: LayerId,
    },
}

/// Checks PAG source-layer references against a complete candidate layer index.
pub fn validate_pag_layer_references(
    layer: &Layer,
    available_layers: &BTreeMap<LayerId, &Layer>,
    source_addressable_layer_ids: &BTreeSet<LayerId>,
) -> Result<(), PagLayerInvariantError> {
    if let LayerData::Pag(pag) = layer.data() {
        for referenced_layer_id in pag.media_source_layer_ids() {
            if referenced_layer_id == pag.id {
                return Err(PagLayerInvariantError::SelfReference {
                    pag_layer_id: pag.id,
                });
            }
            match available_layers
                .get(&referenced_layer_id)
                .copied()
                .map(Layer::data)
            {
                None => {
                    return Err(PagLayerInvariantError::SourceNotFound {
                        pag_layer_id: pag.id,
                        referenced_layer_id,
                    });
                }
                Some(LayerData::Video(_) | LayerData::Image(_) | LayerData::Media(_))
                    if source_addressable_layer_ids.contains(&referenced_layer_id) => {}
                Some(LayerData::Video(_) | LayerData::Image(_) | LayerData::Media(_)) => {
                    return Err(PagLayerInvariantError::SourceNotAddressable {
                        pag_layer_id: pag.id,
                        referenced_layer_id,
                    });
                }
                Some(_) => {
                    return Err(PagLayerInvariantError::SourceNotMedia {
                        pag_layer_id: pag.id,
                        referenced_layer_id,
                    });
                }
            }
        }
        if let Some(referenced_layer_id) = pag
            .transition
            .as_ref()
            .and_then(|transition| transition.sound_layer_id)
        {
            if referenced_layer_id == pag.id {
                return Err(PagLayerInvariantError::SelfReference {
                    pag_layer_id: pag.id,
                });
            }
            match available_layers
                .get(&referenced_layer_id)
                .copied()
                .map(Layer::data)
            {
                None => {
                    return Err(PagLayerInvariantError::SourceNotFound {
                        pag_layer_id: pag.id,
                        referenced_layer_id,
                    });
                }
                Some(LayerData::Audio(_)) => {}
                Some(_) => {
                    return Err(PagLayerInvariantError::SoundLayerNotAudio {
                        pag_layer_id: pag.id,
                        referenced_layer_id,
                    });
                }
            }
        }
    }
    if let Some(children) = layer.child_layers() {
        children.iter().try_for_each(|child| {
            validate_pag_layer_references(child, available_layers, source_addressable_layer_ids)
        })?;
    }
    Ok(())
}

/// Checks PAG placement and payload invariants for a candidate layer tree.
pub fn validate_pag_layer_tree(
    layer: &Layer,
    is_root: bool,
    parent_is_ai_edit: bool,
) -> Result<(), PagLayerInvariantError> {
    if let LayerData::Pag(pag) = layer.data() {
        if !is_root && !parent_is_ai_edit {
            return Err(PagLayerInvariantError::ParentUnsupported(pag.id));
        }
        if !pag.opacity.is_finite() || !(0.0..=1.0).contains(&pag.opacity) {
            return Err(PagLayerInvariantError::InvalidOpacity(pag.id));
        }
        if !pag.insert_slot_links_are_valid() {
            return Err(PagLayerInvariantError::InvalidInsertSlots(pag.id));
        }
        validate_pag_layer_content(pag)?;
    }
    if let Some(children) = layer.child_layers() {
        let is_ai_edit = matches!(layer.data(), LayerData::AiEdit(_));
        children
            .iter()
            .try_for_each(|child| validate_pag_layer_tree(child, false, is_ai_edit))?;
    }
    Ok(())
}

fn validate_pag_layer_content(pag: &crate::PagLayer) -> Result<(), PagLayerInvariantError> {
    let invalid = |reason| PagLayerInvariantError::InvalidContent {
        layer_id: pag.id,
        reason,
    };

    if pag
        .playback
        .as_ref()
        .is_some_and(|playback| !pag.playback_is_valid(playback))
    {
        return Err(invalid(
            "playback must be a bounded linear 1x trim within the authored activeRange",
        ));
    }

    if pag.items.is_empty() {
        return Err(invalid("must contain at least one PAG item"));
    }

    for item in &pag.items {
        if item.asset_id.as_str().is_empty() {
            return Err(invalid("PAG item asset IDs must not be empty"));
        }
        if let Some(duration) = &item.duration {
            match duration {
                crate::FxPagItemDuration::Custom { custom_seconds }
                    if custom_seconds.is_finite() => {}
                crate::FxPagItemDuration::Standard {
                    duration_type:
                        crate::FxPagItemDurationType::Duration
                        | crate::FxPagItemDurationType::DurationLoop,
                } => {}
                crate::FxPagItemDuration::Custom { .. }
                | crate::FxPagItemDuration::Standard { .. } => {
                    return Err(invalid(
                        "item durations must use a supported mode and custom durations must be finite",
                    ));
                }
            }
        }
        for entry in &item.configuration {
            if entry.key.is_empty() {
                return Err(invalid("configuration keys must not be empty"));
            }
            let config = &entry.pag_layer_config;
            if config
                .image
                .as_ref()
                .is_some_and(|image| image.asset_id.as_str().is_empty())
                || config
                    .video
                    .as_ref()
                    .is_some_and(|video| video.asset_id.as_str().is_empty())
            {
                return Err(invalid("configuration asset IDs must not be empty"));
            }
            if config.video.as_ref().is_some_and(|video| {
                video
                    .scale_mode
                    .is_some_and(|mode| !valid_pag_image_scale_mode(mode))
            }) {
                return Err(invalid("configuration scale modes must be supported"));
            }
            if config.text.as_ref().is_some_and(|text| {
                invalid_optional_non_negative(text.font_size)
                    || invalid_optional_non_negative(text.stroke_width)
                    || text.color.as_ref().is_some_and(invalid_pag_color)
                    || text.stroke_color.as_ref().is_some_and(invalid_pag_color)
            }) {
                return Err(invalid(
                    "configuration colors and text metrics must be finite and valid",
                ));
            }
            if config
                .solid
                .as_ref()
                .is_some_and(|solid| solid.color.as_ref().is_some_and(invalid_pag_color))
            {
                return Err(invalid("configuration colors must be finite and valid"));
            }
        }
    }

    for insert in &pag.image_inserts {
        let source_kind_count = usize::from(insert.image.is_some())
            + usize::from(insert.video.is_some())
            + usize::from(!insert.source_layer_ids.is_empty());
        if source_kind_count != 1 {
            return Err(invalid(
                "image inserts must specify exactly one of image, video, or sourceLayerIds",
            ));
        }
        if insert
            .image
            .as_ref()
            .is_some_and(|image| image.asset_id.as_str().is_empty())
            || insert
                .video
                .as_ref()
                .is_some_and(|video| video.asset_id.as_str().is_empty())
        {
            return Err(invalid("image insert asset IDs must not be empty"));
        }
        if insert
            .scale_mode
            .is_some_and(|mode| !valid_pag_image_scale_mode(mode))
            || insert.video.as_ref().is_some_and(|video| {
                video
                    .scale_mode
                    .is_some_and(|mode| !valid_pag_image_scale_mode(mode))
            })
        {
            return Err(invalid("image insert scale modes must be supported"));
        }
    }

    if pag
        .color_inserts
        .iter()
        .filter_map(|insert| insert.color.as_ref())
        .any(invalid_pag_color)
    {
        return Err(invalid("color insert colors must be finite and valid"));
    }
    if pag.text_inserts.iter().any(|insert| {
        invalid_optional_non_negative(insert.font_size)
            || invalid_optional_non_negative(insert.stroke_width)
            || insert.fill_color.as_ref().is_some_and(invalid_pag_color)
            || insert.stroke_color.as_ref().is_some_and(invalid_pag_color)
            || insert
                .background_color
                .as_ref()
                .is_some_and(invalid_pag_color)
    }) {
        return Err(invalid(
            "text insert colors and metrics must be finite and valid",
        ));
    }

    Ok(())
}

fn invalid_optional_non_negative(value: Option<f64>) -> bool {
    value.is_some_and(|value| !value.is_finite() || value < 0.0)
}

fn invalid_pag_color(color: &crate::Color) -> bool {
    color
        .iter()
        .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
}

const fn valid_pag_image_scale_mode(mode: crate::PagImageScaleMode) -> bool {
    matches!(
        mode,
        crate::PagImageScaleMode::Stretch
            | crate::PagImageScaleMode::Letterbox
            | crate::PagImageScaleMode::Zoom
    )
}
