//! Bounded Set Matte lowering. These are independent editable Alpha/Luma samples,
//! not an implementation of AE's general pre-transform effect input pipeline.
use std::collections::HashSet;

use fx_schema::{
    GroupLayer, LayerData as FxLayer, Time, TimeRangeProperty, TrackMatte, TrackMatteType,
};

use super::{
    Converter, DocumentError, LayerContext, LayerPurpose, MAX_GROUP_DEPTH, compositing, group,
    mute_audio, stored_layers,
};
use crate::{
    diagnostic::Limitation,
    properties,
    rifx::Chunk,
    structure::{ItemKind, Layer},
};

const MATCH_NAME: &str = "ADBE Set Matte3";

/// Native render stage for the layer selected by a Set Matte layer parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MatteSampleStage {
    /// Sample source pixels before occurrence masks, effects, and layer styles.
    Source,
    /// Sample the occurrence through all of its masks, effects, and layer styles.
    AllEffects,
}

impl MatteSampleStage {
    /// Whether the helper includes the provider occurrence's post-source pipeline.
    pub(super) fn includes_occurrence_pipeline(self) -> bool {
        self == Self::AllEffects
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MatteSource {
    layer_id: u32,
    stage: MatteSampleStage,
    mode: TrackMatteType,
}

/// The independently inspected native Alpha profile. Do not treat arbitrary
/// plugin tables as empty: Invert, Stretch and Composite change matte semantics.
pub(super) fn validate_alpha_defaults(definitions: &[Chunk]) -> Result<(), String> {
    let rows = properties::runs(definitions).map_err(|error| error.to_string())?;
    let profile = [
        (0_u32, 0_u32),
        (0, 0),
        (7, 4),
        (4, 0),
        (4, 1),
        (4, 1),
        (4, 1),
        (9, 0),
    ];
    if rows.len() != profile.len() {
        return Err("Set Matte defaults are not the complete bounded Alpha profile".into());
    }
    for (index, (kind, value)) in profile.into_iter().enumerate() {
        let name = if index == 7 {
            "ADBE Effect Built In Params".to_owned()
        } else {
            format!("ADBE Set Matte3-{index:04}")
        };
        let (_, definition) = rows
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .ok_or_else(|| format!("Set Matte default {name} is missing or ambiguous"))?;
        let bytes = properties::data(definition, *b"pard").map_err(|error| error.to_string())?;
        if bytes.len() != 148
            || bytes[12..16] != kind.to_be_bytes()
            || bytes[56..60] != value.to_be_bytes()
        {
            return Err(format!(
                "Set Matte default {name} is outside the bounded Alpha profile"
            ));
        }
    }
    Ok(())
}

fn sources(layer: &Layer) -> Result<Vec<MatteSource>, String> {
    if !layer.record.flags().effects_active {
        return Ok(Vec::new());
    }
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let mut parades = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade");
    let Some((_, parade)) = parades.next() else {
        return Ok(Vec::new());
    };
    if parades.next().is_some() {
        return Err("duplicate Effect Parade roots; ambiguous Set Matte omitted".into());
    }
    let instances = properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?;
    let runs = properties::runs(instances).map_err(|e| e.to_string())?;
    if !runs.iter().any(|(name, _)| *name == MATCH_NAME) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (name, run) in runs {
        let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
        let mut warnings = Vec::new();
        let enabled = properties::group_enabled_or_warn(descriptor, name, &mut warnings);
        if !warnings.is_empty() {
            return Err(warnings.join("; "));
        }
        if !enabled {
            continue;
        }
        if name != MATCH_NAME {
            return Err("mixed effect order is outside the bounded Set Matte profile".into());
        }
        let definitions =
            properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
        if !definitions.is_empty() {
            validate_alpha_defaults(definitions)?;
        }
        let body = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
        let controls = properties::runs(body).map_err(|e| e.to_string())?;
        if controls.iter().any(|(name, _)| {
            !matches!(
                *name,
                "ADBE Set Matte3-0000"
                    | "ADBE Set Matte3-0001"
                    | "ADBE Set Matte3-0002"
                    | "ADBE Effect Built In Params"
            )
        }) {
            return Err("unknown explicit Set Matte controls are unsupported".into());
        }
        // Nondefault compositing options would change both coverage and order.
        if let Some((_, options)) = controls
            .iter()
            .find(|(name, _)| *name == "ADBE Effect Built In Params")
        {
            let options = properties::unique_list(options, *b"tdgp").map_err(|e| e.to_string())?;
            if !properties::runs(options)
                .map_err(|e| e.to_string())?
                .is_empty()
            {
                return Err("effect compositing options are unsupported".into());
            }
        }
        let references: Vec<_> = controls
            .iter()
            .filter(|(name, _)| *name == "ADBE Set Matte3-0001")
            .collect();
        let [(_, reference)] = references.as_slice() else {
            return Err("missing or duplicate matte source control".into());
        };
        let property = properties::unique_list(reference, *b"tdbs").map_err(|e| e.to_string())?;
        let numeric = properties::read_numeric(property).map_err(|e| e.to_string())?;
        if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
            return Err("animated or expression-driven matte selection is unsupported".into());
        }
        let reference = layer_reference(property)?;
        let channel_controls: Vec<_> = controls
            .iter()
            .filter(|(name, _)| *name == "ADBE Set Matte3-0002")
            .collect();
        // This native table proves its default Alpha channel, not other popup
        // selections or the older sparse explicit-channel representation.
        if !definitions.is_empty() && !channel_controls.is_empty() {
            return Err(
                "explicit channel overrides of native Alpha defaults are unsupported".into(),
            );
        }
        let mode = match channel_controls.as_slice() {
            [] => TrackMatteType::Alpha,
            [(_, channel)] => {
                let property =
                    properties::unique_list(channel, *b"tdbs").map_err(|e| e.to_string())?;
                let numeric = properties::read_numeric(property).map_err(|e| e.to_string())?;
                if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
                    return Err("animated or expression-driven matte channel is unsupported".into());
                }
                let [value] = numeric.values.as_slice() else {
                    return Err("Set Matte channel must contain one static value".into());
                };
                match *value {
                    1.0 => TrackMatteType::Alpha,
                    2.0 => TrackMatteType::Luma,
                    value => {
                        return Err(format!(
                            "Set Matte Use For Matte value {value} is unsupported"
                        ));
                    }
                }
            }
            _ => return Err("duplicate Set Matte channel controls are unsupported".into()),
        };
        result.push(MatteSource {
            layer_id: reference.layer_id,
            stage: reference.stage,
            mode,
        });
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LayerReference {
    layer_id: u32,
    stage: MatteSampleStage,
}

fn layer_reference(property: &[Chunk]) -> Result<LayerReference, String> {
    let read = |tag| -> Result<[u8; 4], String> {
        let chunks: Vec<_> = property.iter().filter(|chunk| chunk.id() == tag).collect();
        let [chunk] = chunks.as_slice() else {
            return Err("missing or duplicate Set Matte layer reference metadata".into());
        };
        chunk
            .data_payload()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| "malformed Set Matte layer reference metadata".into())
    };
    let stage = match i32::from_be_bytes(read(*b"tdps")?) {
        0 => MatteSampleStage::Source,
        -1 => MatteSampleStage::AllEffects,
        stage => {
            return Err(format!(
                "Set Matte source sampling stage {stage} is unsupported"
            ));
        }
    };
    let layer_id = u32::from_be_bytes(read(*b"tdpi")?);
    if layer_id == 0 {
        return Err("Set Matte source is not a concrete layer".into());
    }
    Ok(LayerReference { layer_id, stage })
}

fn independently_duplicable_kind(layer_type: u8, source_is_composition: bool) -> bool {
    layer_type == 4 || (layer_type == 0 && source_is_composition)
}

fn depth(layer: &GroupLayer) -> usize {
    1 + layer
        .layers
        .iter()
        .map(|layer| match layer.data() {
            FxLayer::Group(group) => depth(group),
            _ => 1,
        })
        .max()
        .unwrap_or(0)
}

impl Converter<'_> {
    pub(super) fn apply_set_mattes(
        &mut self,
        context: &LayerContext<'_>,
        source_indices: &[usize],
        layers: &mut [FxLayer],
    ) -> Result<(), DocumentError> {
        for (index, &source_index) in source_indices.iter().enumerate() {
            let source = &context.comp.layers[source_index];
            let links = match sources(source) {
                Ok(links) if links.is_empty() => continue,
                Ok(links) => links,
                Err(message) => {
                    self.warn(
                        Limitation::TrackMatte,
                        Some(context.comp_id),
                        Some(source.record.id()),
                        format!("Set Matte not lowered: {message}; existing content retained"),
                    );
                    continue;
                }
            };
            let FxLayer::Group(current) = &layers[index] else {
                continue;
            };
            let providers: Option<Vec<_>> = links
                .iter()
                .copied()
                .map(|link| {
                    let mut matches = context
                        .comp
                        .layers
                        .iter()
                        .filter(|layer| layer.record.id() == link.layer_id);
                    let provider = matches.next()?;
                    (matches.next().is_none()
                        && link.layer_id != source.record.id()
                        && self.is_independently_duplicable_2d(provider)
                        // Source sampling precedes the provider's composition
                        // matte. AllEffects copies must still reject that edge.
                        && (compositing::matte_layer(&provider.record).is_none()
                            || (provider.record.track_matte_type() == 1
                                && link.stage == MatteSampleStage::Source
                                && link.mode == TrackMatteType::Alpha))
                        && sources(provider).is_ok_and(|sources| sources.is_empty()))
                    .then_some((provider, link))
                })
                .collect();
            // Only an already-resolved native Alpha binding can be nested in
            // the new gate. In particular, provider copies processed before
            // apply_mattes assigns their bindings remain ineligible.
            let resolved_alpha = source.record.track_matte_type() == 1
                && current
                    .track_matte
                    .as_ref()
                    .is_some_and(|matte| matte.mode == TrackMatteType::Alpha)
                && matches!(source.record.blend_mode(), 0 | 2)
                && links.iter().all(|link| {
                    link.mode == TrackMatteType::Alpha && link.stage == MatteSampleStage::Source
                });
            let eligible = self.is_independently_duplicable_2d(source)
                && ((compositing::matte_layer(&source.record).is_none()
                    && current.track_matte.is_none())
                    || resolved_alpha)
                && context.depth + depth(current) + links.len() < MAX_GROUP_DEPTH;
            let Some(providers) = providers.filter(|_| eligible) else {
                self.warn(Limitation::TrackMatte, Some(context.comp_id), Some(source.record.id()), "Set Matte needs unique independent 2D shape or precomposition providers, an absent or resolved supported Alpha matte, and bounded nesting; unsupported dependency retained without lowering".into());
                continue;
            };
            // Prepare all helpers before replacing any existing content. Converter
            // allocation and animation/shape budgets also apply to these copies.
            let helper_context = LayerContext {
                depth: context.depth + links.len(),
                ..*context
            };
            let mut helpers = Vec::with_capacity(providers.len());
            for (provider, link) in providers {
                let mut helper = self.layer(
                    &helper_context,
                    provider,
                    LayerPurpose::MatteSample(link.stage),
                )?;
                let mut muted = HashSet::new();
                mute_audio(&mut helper, &mut muted)?;
                if !muted.is_empty()
                    && let Err(error) =
                        self.discard_animations(|entry| muted.contains(&entry.target))
                {
                    self.warn(
                        Limitation::ExpansionLimit,
                        Some(context.comp_id),
                        Some(provider.record.id()),
                        format!(
                            "discarded Set Matte-helper audio animation could not release its exact reservation: {error}; the allowance remains conservatively charged"
                        ),
                    );
                }
                helper.name.push_str(" (Set Matte sample)");
                helpers.push((helper, link.mode));
            }
            for (mut helper, mode) in helpers {
                let id = self.allocate_id()?;
                let mut wrapper = group(
                    id,
                    format!("{} (Set Matte)", source.name),
                    Some(context.parent),
                    TimeRangeProperty::new(
                        Time::ZERO,
                        self.duration(context.comp_id, context.comp.duration_secs),
                    ),
                );
                wrapper.track_matte = Some(TrackMatte {
                    mode,
                    layer: helper.id,
                });
                helper.parent = Some(id);
                let previous = std::mem::replace(&mut layers[index], FxLayer::Group(wrapper));
                let FxLayer::Group(mut previous) = previous else {
                    unreachable!("checked Group above")
                };
                previous.parent = Some(id);
                let children =
                    stored_layers(vec![FxLayer::Group(previous), FxLayer::Group(helper)])?;
                if let FxLayer::Group(wrapper) = &mut layers[index] {
                    wrapper.layers = children;
                }
            }
            self.warn(Limitation::TrackMatte, Some(context.comp_id), Some(source.record.id()),
                format!("Set Matte3 lowered to {} independent editable gates from native samples {links:?}; only static Alpha/Luma with sparse noninverted/composite-original defaults are supported. Composition-space sampling approximates AE pre-transform/stretch semantics; provider edits are not linked, and unsupported dynamic paths retain their diagnosed initial outlines. Native-effect omission diagnostics refer to the effect adapter, not these structural gates; Adobe fidelity is unverified", links.len()));
        }
        Ok(())
    }

    fn is_independently_duplicable_2d(&self, layer: &Layer) -> bool {
        let record = &layer.record;
        let flags = record.flags();
        let source_is_composition = self
            .items
            .get(&record.source_id())
            .is_some_and(|item| matches!(&item.kind, ItemKind::Composition(_)));
        independently_duplicable_kind(record.layer_type(), source_is_composition)
            && !flags.three_d_layer
            && !flags.adjustment_layer
    }
}

#[cfg(test)]
mod tests;
