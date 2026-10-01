//! Adjustment-layer lowering into a direct sibling stack gate.

use fx_schema::{AdjustmentLayer, LayerData as FxLayer, PropType, TimeRangeProperty};

use super::{
    Converter, GroupLayer, LayerContext, LayerPurpose, Limitation, compositing, source_dimensions,
    transform,
};
use crate::{document::DocumentError, effects::definitions, rifx::Chunk, structure::Layer};

#[cfg(test)]
mod tests;

impl Converter<'_> {
    pub(super) fn adjustment_layer(
        &mut self,
        context: &LayerContext<'_>,
        layer: &Layer,
    ) -> Result<(AdjustmentLayer, Vec<FxLayer>), DocumentError> {
        let animation_start = self.animations.len();
        let mut normalized = layer.clone();
        for problem in normalize_adjustment_mask_feather(&mut normalized.content) {
            self.warn(Limitation::Properties, Some(context.comp_id), Some(layer.record.id()),
                format!("Adjustment Mask Feather: {problem}; native record left untouched for normal reader diagnostics"));
        }
        if let Err(error) = restore_sparse_effect_definitions(&mut normalized.content, None) {
            self.warn(
                Limitation::Properties,
                Some(context.comp_id),
                Some(layer.record.id()),
                format!("Adjustment sparse Effect Parade definitions could not be restored: {error}; available controls retain their imported values"),
            );
        }
        let (outer, effect_opacity) =
            self.layer_with_effect_gate(context, &normalized, LayerPurpose::Adjustment)?;
        let actual = deepest_named_group(&outer, &outer.name);
        let active_range = source_active_range(actual).unwrap_or(actual.playback.input_range());
        let direct_parent_transform = context
            .comp
            .layers
            .iter()
            .find(|candidate| candidate.record.id() == layer.record.parent_id())
            .map(|parent| {
                let source = self.items.get(&parent.record.source_id()).copied();
                let size = source_dimensions(source, parent);
                let (mut transform, warnings) =
                    transform::static_transform(parent, size, context.comp);
                if parent.record.flags().null_layer {
                    transform
                        .anchor_point
                        .iter_mut()
                        .for_each(|component| *component *= 100.0);
                }
                transform.opacity = fx_schema::PercentageProperty::new(100.0)
                    .expect("100 is a valid parent-guide opacity");
                for warning in warnings {
                    self.warn(
                        Limitation::Parenting,
                        Some(context.comp_id),
                        Some(layer.record.id()),
                        format!("Adjustment guide parent {}: {warning}", parent.record.id()),
                    );
                }
                transform
            });
        let parent_problems =
            parent_geometry_limitations(&context.comp.layers, layer.record.parent_id());
        for problem in &parent_problems {
            self.warn(
                Limitation::Parenting,
                Some(context.comp_id),
                Some(layer.record.id()),
                problem.clone(),
            );
        }
        let (guide_transform, mut guide_geometry_exact) = flattened_guide_transform(
            direct_parent_transform.unwrap_or(outer.transform),
            actual.transform,
            outer.id == actual.id,
        );
        guide_geometry_exact &= parent_problems.is_empty();
        let mut guides = Vec::new();
        for stored in &actual.layers {
            let FxLayer::Shape(guide) = stored.data() else {
                continue;
            };
            if !guide.name.contains(" — Mask ") {
                continue;
            }
            let mut guide = guide.clone();
            guide.parent = Some(context.parent);
            guide.transform = guide_transform;
            guide.transform.opacity =
                fx_schema::PercentageProperty::new(100.0).expect("100 is a valid guide opacity");
            guides.push(FxLayer::Shape(guide));
        }

        let offset_millis = i64::try_from(active_range.start.as_millis()).unwrap_or(i64::MAX);
        let adjustment_id = actual.id;
        let mut rebase_error = None;
        let mut adjustment_animations = self.animations.split_off(animation_start);
        // Own geometry tracks were already pruned by the Adjustment budget
        // probe. Inspect the native controls rather than losing their diagnostic.
        let animated_own_geometry =
            crate::properties::read_transform(&layer.content).is_ok_and(|properties| {
                properties.iter().any(|property| {
                    property.match_name != "ADBE Opacity"
                        && property
                            .numeric
                            .as_ref()
                            .is_ok_and(|value| value.animated || value.expression_present)
                })
            });
        if !guides.is_empty()
            && (animated_own_geometry
                || adjustment_animations.iter().any(|entry| {
                    entry
                        .target
                        .as_property()
                        .is_some_and(|property| property.property_type() != PropType::Opacity)
                }))
        {
            guide_geometry_exact = false;
            self.warn(Limitation::Properties, Some(context.comp_id), Some(layer.record.id()),
                "Animated Adjustment/parent gate transforms are not mapped to same-parent guides; initial guide geometry retained (converter gap, not an absent FX capability)".into());
        }
        let mut retained_animations = Vec::with_capacity(adjustment_animations.len());
        let mut discarded_reservations = Vec::new();
        for mut entry in adjustment_animations.drain(..) {
            let original_reservation =
                match super::animation_budget::committed_entry_reservation_bytes(&entry) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        rebase_error = Some(format!(
                            "original animation reservation could not be measured: {error}"
                        ));
                        continue;
                    }
                };
            if let Some(property) = entry.target.as_property()
                && (property.layer_id() != adjustment_id
                    || property.property_type() != PropType::Opacity)
            {
                discarded_reservations.push(original_reservation);
                continue;
            }
            if let Err(error) = rebase_entry(&mut entry, offset_millis) {
                rebase_error = Some(error);
                discarded_reservations.push(original_reservation);
                continue;
            }
            let rebased_reservation =
                match super::animation_budget::committed_entry_reservation_bytes(&entry) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        rebase_error = Some(format!(
                            "rebased animation reservation could not be measured: {error}"
                        ));
                        discarded_reservations.push(original_reservation);
                        continue;
                    }
                };
            if rebased_reservation > original_reservation {
                let additional = rebased_reservation - original_reservation;
                if let Err(error) = self.animation_budget.reserve(additional) {
                    rebase_error = Some(format!(
                        "rebased animation exceeds the remaining allowance: {error}"
                    ));
                    discarded_reservations.push(original_reservation);
                    continue;
                }
            } else if original_reservation > rebased_reservation {
                let released = original_reservation - rebased_reservation;
                if let Err(error) = self.animation_budget.release(released) {
                    rebase_error = Some(format!(
                        "rebased animation reservation could not release {released} bytes: {error}"
                    ));
                }
            }
            retained_animations.push(entry);
        }
        match discarded_reservations
            .into_iter()
            .try_fold(0_usize, usize::checked_add)
        {
            Some(bytes) => {
                if let Err(error) = self.animation_budget.release(bytes) {
                    self.warn(
                        Limitation::ExpansionLimit,
                        Some(context.comp_id),
                        Some(layer.record.id()),
                        format!(
                            "discarded Adjustment animation could not release its recorded reservation: {error}; the allowance remains conservatively charged"
                        ),
                    );
                }
            }
            None => self.warn(
                Limitation::ExpansionLimit,
                Some(context.comp_id),
                Some(layer.record.id()),
                "discarded Adjustment animation reservations overflowed during reclamation; the allowance remains conservatively charged".into(),
            ),
        }
        adjustment_animations = retained_animations;
        let has_opacity_animation = adjustment_animations.iter().any(|entry| {
            entry.target.as_property().is_some_and(|property| {
                property.layer_id() == adjustment_id
                    && property.property_type() == PropType::Opacity
            })
        });
        let mut transform = actual.transform;
        if let Some(opacity) = effect_opacity {
            transform.opacity = opacity;
        }
        if let Some(opacity) = initial_opacity(&adjustment_animations, adjustment_id)
            && let Some(value) = fx_schema::PercentageProperty::new(opacity)
        {
            transform.opacity = value;
        }
        self.animations.append(&mut adjustment_animations);
        if let Some(error) = rebase_error {
            self.warn(
                Limitation::Properties,
                Some(context.comp_id),
                Some(layer.record.id()),
                format!("Adjustment owner-local animation rebase failed; affected track omitted: {error}"),
            );
        }
        if layer.record.parent_id() != 0 {
            let message = if guide_geometry_exact {
                "Adjustment parent/static affine is carried exactly by same-parent editable mask guides; the gate remains a direct sibling and does not transform lower content"
            } else {
                "Adjustment parent/gate geometry is only partially mapped; initial direct-parent guide approximation retained. Ancestor, animation, or transform-combination limitations are reported separately; lower siblings remain untransformed"
            };
            self.warn(
                Limitation::Parenting,
                Some(context.comp_id),
                Some(layer.record.id()),
                message.into(),
            );
            if !actual.masks.is_empty()
                && (guide_transform.scale[0] != guide_transform.scale[1]
                    || guide_transform.rotation != 0.0)
            {
                self.warn(
                    Limitation::Properties,
                    Some(context.comp_id),
                    Some(layer.record.id()),
                    "Adjustment mask path geometry retains the native parent affine, but FX feather and expansion remain source-plane scalars; AE's anisotropically scaled/rotated feather falloff and expansion cannot be represented exactly"
                        .into(),
                );
            }
        }
        if layer.record.flags().three_d_layer {
            self.warn(
                Limitation::Properties,
                Some(context.comp_id),
                Some(layer.record.id()),
                "3D Adjustment projection has no exact FX gate representation; supported opacity/effects and static mask guides are retained, unsupported 3D geometry is omitted"
                    .into(),
            );
        }
        if !actual.masks.is_empty()
            || compositing::matte_layer(&layer.record).is_some()
            || transform.opacity.value() < 100.0
            || has_opacity_animation
        {
            self.warn(Limitation::Properties, Some(context.comp_id), Some(layer.record.id()),
                "Gated Adjustment controls are editable, but existing FX dry-plus-wet alpha compositing is not pixel-equivalent to AE replacement/interpolation when effects change alpha; mask, matte and opacity fidelity is not established".into());
        }
        self.warn(
            Limitation::Properties,
            Some(context.comp_id),
            Some(layer.record.id()),
            "native Adjustment lowered to a direct FX Adjustment sibling; Transform opacity, effects, timing, masks and supported matte relationships are retained, while non-opacity Transform geometry is represented only by editable mask guides"
                .into(),
        );

        Ok((
            AdjustmentLayer {
                id: actual.id,
                name: actual.name.clone(),
                description: actual.description.clone(),
                is_hidden: actual.is_hidden,
                parent: Some(context.parent),
                blend_mode: outer.blend_mode,
                track_matte: None,
                masks: actual.masks.clone(),
                active_range,
                effects: actual.effects.clone(),
                transform,
            },
            guides,
        ))
    }
}

fn parent_geometry_limitations(layers: &[Layer], mut parent_id: u32) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while parent_id != 0 {
        if seen.len() >= super::MAX_GROUP_DEPTH || !seen.insert(parent_id) {
            problems.push("Adjustment parent chain is cyclic or over depth; initial direct-parent guide retained".into());
            break;
        }
        let Some(parent) = layers.iter().find(|layer| layer.record.id() == parent_id) else {
            problems.push(format!(
                "Adjustment parent {parent_id} is missing; initial guide geometry retained"
            ));
            break;
        };
        if seen.len() > 1 {
            problems.push(format!("Adjustment ancestor {parent_id} above the direct parent is not mapped to the guide; initial direct-parent affine retained (converter gap)"));
        }
        if parent.record.flags().three_d_layer {
            problems.push(format!("Adjustment parent {parent_id} 3D projection is not mapped to the guide; planar approximation retained"));
        }
        match crate::properties::read_transform(&parent.content) {
            Ok(properties) => {
                if properties.iter().any(|property| property.numeric.as_ref().is_ok_and(|value| value.animated || value.expression_present)) {
                    problems.push(format!("Adjustment parent {parent_id} animated/expression transform is not mapped to the guide; initial values retained (converter gap; AE expressions remain unsupported)"));
                }
                if properties.iter().any(|property| property.numeric.is_err()) {
                    problems.push(format!("Adjustment parent {parent_id} has unreadable transform controls; exact guide geometry is unverified"));
                }
            }
            Err(error) => problems.push(format!("Adjustment parent {parent_id} transform is unreadable: {error}; exact guide geometry is unverified")),
        }
        parent_id = parent.record.parent_id();
    }
    problems
}

fn flattened_guide_transform(
    outer: fx_schema::Transform,
    actual: fx_schema::Transform,
    same_group: bool,
) -> (fx_schema::Transform, bool) {
    if same_group {
        return (actual, true);
    }
    let fx_schema::Transform {
        anchor_point,
        position,
        scale,
        rotation,
        skew,
        skew_axis,
        rotation_x,
        rotation_y,
        orientation,
        ..
    } = actual;
    let fx_schema::Position::TwoD(position) = position else {
        return (outer, false);
    };
    if scale != [100.0, 100.0]
        || rotation != 0.0
        || skew != 0.0
        || skew_axis != 0.0
        || rotation_x != 0.0
        || rotation_y != 0.0
        || orientation != [0.0, 0.0, 0.0]
    {
        return (outer, false);
    }
    let mut flattened = outer;
    for component in 0..2 {
        flattened.anchor_point[component] += anchor_point[component] - position[component];
    }
    (flattened, true)
}

fn normalize_adjustment_mask_feather(chunks: &mut [Chunk]) -> Vec<&'static str> {
    let mut problems = Vec::new();
    let mut feather = false;
    for chunk in chunks {
        if chunk.id() == *b"tdmn" {
            feather = chunk
                .data_payload()
                .and_then(|bytes| bytes.split(|byte| *byte == 0).next())
                == Some(b"ADBE Mask Feather");
        }
        if feather && chunk.list_kind() == Some(*b"tdbs") {
            let metadata = chunk
                .children_mut()
                .and_then(|children| children.iter_mut().find(|chunk| chunk.id() == *b"tdb4"));
            if let Some(metadata) = metadata {
                if let Some(bytes) = metadata
                    .data_payload()
                    .filter(|bytes| bytes.len() == 124 && bytes[..4] == [0xdb, 0x99, 0, 2])
                {
                    if bytes[59] == 4 && bytes[60] == 6 {
                        // This native mask-pair unit marker is not a separated-dimension flag.
                        let mut bytes = bytes.to_vec();
                        bytes[59] = 0;
                        match Chunk::data(*b"tdb4", bytes) {
                            Ok(replacement) => *metadata = replacement,
                            Err(_) => problems.push("descriptor normalization failed"),
                        }
                    }
                } else {
                    problems.push("unsupported descriptor layout");
                }
            } else {
                problems.push("missing descriptor");
            }
        }
        if let Some(children) = chunk.children_mut() {
            problems.extend(normalize_adjustment_mask_feather(children));
        }
    }
    problems
}

fn restore_sparse_effect_definitions(
    chunks: &mut [Chunk],
    enclosing_name: Option<&str>,
) -> Result<(), String> {
    for index in 0..chunks.len() {
        let name = (index > 0 && chunks[index - 1].id() == *b"tdmn")
            .then(|| match_name(&chunks[index - 1]))
            .transpose()?
            .flatten()
            .or_else(|| enclosing_name.map(str::to_owned));
        if chunks[index].list_kind() == Some(*b"sspc")
            && matches!(
                name.as_deref(),
                Some("ADBE Gaussian Blur 2" | "ADBE Pro Levels2")
            )
            && let Some(definition) = name.as_deref().and_then(definitions::definition)
            && let Some(children) = chunks[index].children_mut()
            && let Some(table) = children
                .iter_mut()
                .find(|child| child.list_kind() == Some(*b"parT"))
            && table.children().is_some_and(|entries| entries.is_empty())
        {
            let mut entries = vec![
                Chunk::data(
                    *b"parn",
                    u32::try_from(definition.parameters.len())
                        .map_err(|_| "effect parameter count exceeds u32".to_owned())?
                        .to_be_bytes(),
                )
                .map_err(|error| error.to_string())?,
            ];
            for parameter in &definition.parameters {
                entries.push(effect_match_name(&parameter.match_name)?);
                entries.push(
                    definitions::encode_parameter(parameter).map_err(|error| error.to_string())?,
                );
            }
            *table = Chunk::list(*b"parT", entries);
        }
        if let Some(children) = chunks[index].children_mut() {
            restore_sparse_effect_definitions(children, name.as_deref())?;
        }
    }
    Ok(())
}

fn match_name(chunk: &Chunk) -> Result<Option<String>, String> {
    let Some(payload) = chunk.data_payload() else {
        return Ok(None);
    };
    let bytes = payload.split(|byte| *byte == 0).next().unwrap_or_default();
    std::str::from_utf8(bytes)
        .map(|value| Some(value.to_owned()))
        .map_err(|error| error.to_string())
}

fn effect_match_name(value: &str) -> Result<Chunk, String> {
    if value.len() > 39 || value.contains('\0') {
        return Err("invalid effect parameter match name".into());
    }
    let mut bytes = vec![0; 40];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    Chunk::data(*b"tdmn", bytes).map_err(|error| error.to_string())
}

fn deepest_named_group<'a>(group: &'a GroupLayer, name: &str) -> &'a GroupLayer {
    group
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            FxLayer::Group(child) if child.name == name => Some(deepest_named_group(child, name)),
            _ => None,
        })
        .unwrap_or(group)
}

fn source_active_range(group: &GroupLayer) -> Option<TimeRangeProperty> {
    group.layers.iter().find_map(|layer| match layer.data() {
        FxLayer::Group(child) if child.name == "Source content clock" => {
            Some(child.playback.input_range())
        }
        FxLayer::Group(child) => source_active_range(child),
        _ => None,
    })
}

fn initial_opacity(
    entries: &[fx_schema::animator::AnimationGraphEntry],
    adjustment_id: fx_schema::LayerId,
) -> Option<f64> {
    entries.iter().find_map(|entry| {
        let property = entry.target.as_property()?;
        if property.layer_id() != adjustment_id || property.property_type() != PropType::Opacity {
            return None;
        }
        let value = serde_json::to_value(&entry.animator).ok()?["keyframes"]
            .as_array()?
            .first()?["value"]["value"]
            .as_f64()?;
        let rounded = value.round();
        Some(if (value - rounded).abs() < 1.0e-12 {
            rounded
        } else {
            value
        })
    })
}

fn rebase_entry(
    entry: &mut fx_schema::animator::AnimationGraphEntry,
    offset_millis: i64,
) -> Result<(), String> {
    let mut value = serde_json::to_value(&entry.animator).map_err(|error| error.to_string())?;
    let Some(keys) = value
        .get_mut("keyframes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(());
    };
    for key in keys {
        let time = key["layerTime"]
            .as_i64()
            .ok_or_else(|| "keyframe layerTime is not an integer".to_owned())?;
        key["layerTime"] = serde_json::json!(time.saturating_sub(offset_millis));
    }
    entry.animator = serde_json::from_value(value).map_err(|error| error.to_string())?;
    Ok(())
}
