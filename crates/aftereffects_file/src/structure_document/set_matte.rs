//! Bounded Set Matte lowering. These are independent editable Alpha/Luma samples,
//! not an implementation of AE's general pre-transform effect input pipeline.
use std::collections::HashSet;

use fx_schema::{
    GroupLayer, LayerData as FxLayer, Time, TimeRangeProperty, TrackMatte, TrackMatteType,
    effect::ChannelSource,
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
    projection: Option<ChannelSource>,
}

/// The independently inspected native Alpha profile. Do not treat arbitrary
/// plugin tables as empty: Invert, Stretch and Composite change matte semantics.
pub(super) fn validate_alpha_defaults(definitions: &[Chunk]) -> Result<(), String> {
    validate_defaults(definitions, 4)
}

fn validate_defaults(definitions: &[Chunk], channel: u32) -> Result<(), String> {
    let rows = properties::runs(definitions).map_err(|error| error.to_string())?;
    let profile = [
        (0_u32, 0_u32),
        (0, 0),
        (7, channel),
        (4, 0),
        (4, 1),
        (4, 1),
        (4, 1),
        (9, 0),
    ];
    if rows.len() != profile.len() {
        return Err("Set Matte defaults are not the complete bounded Set Matte profile".into());
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
                "Set Matte default {name} is outside the bounded Set Matte profile"
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
        let declared_channel = if definitions.is_empty() {
            None
        } else {
            let rows = properties::runs(definitions).map_err(|e| e.to_string())?;
            let (_, channel) = rows
                .iter()
                .find(|(name, _)| *name == "ADBE Set Matte3-0002")
                .ok_or("missing Set Matte channel definition")?;
            let bytes = properties::data(channel, *b"pard").map_err(|e| e.to_string())?;
            let value = bytes
                .get(56..60)
                .and_then(|v| <[u8; 4]>::try_from(v).ok())
                .map(u32::from_be_bytes)
                .ok_or("malformed Set Matte channel definition")?;
            if !matches!(value, 1..=5) {
                return Err(format!(
                    "Set Matte native channel {value} is outside the bounded RGB/Alpha/Luma profile"
                ));
            }
            validate_defaults(definitions, value)?;
            Some(value)
        };
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
        let channel = match channel_controls.as_slice() {
            [] => declared_channel.unwrap_or(4),
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
                if !matches!(*value, 1.0 | 2.0 | 3.0 | 4.0 | 5.0) {
                    return Err(format!(
                        "Set Matte Use For Matte value {value} is unsupported"
                    ));
                }
                if declared_channel.is_some_and(|declared| f64::from(declared) != *value) {
                    return Err(
                        "Set Matte channel conflicts with its saved native definition".into(),
                    );
                }
                *value as u32
            }
            _ => return Err("duplicate Set Matte channel controls are unsupported".into()),
        };
        // Native Set Matte3 popup: RGB1/2/3, Alpha4, Luminance5. Sparse
        // instances keep their own explicit control, not another instance's default.
        let projection = match channel {
            1 => Some(ChannelSource::Red),
            2 => Some(ChannelSource::Green),
            3 => Some(ChannelSource::Blue),
            _ => None,
        };
        let mode = if channel == 4 {
            TrackMatteType::Alpha
        } else {
            TrackMatteType::Luma
        };
        result.push(MatteSource {
            layer_id: reference.layer_id,
            stage: reference.stage,
            mode,
            projection,
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
            // Keep every gate in an RGB chain within one sibling scope for
            // native export. The identity consumers preserve each prior subtree
            // without moving a gate onto its transformed/effected picture.
            let sibling_gates = links.iter().any(|link| link.projection.is_some());
            let consumer_carriers = if sibling_gates { links.len() } else { 0 };
            let eligible = self.is_independently_duplicable_2d(source)
                && ((compositing::matte_layer(&source.record).is_none()
                    && current.track_matte.is_none())
                    || resolved_alpha)
                && context.depth
                    + depth(current)
                    + links.len()
                    + links
                        .iter()
                        .filter(|link| link.projection.is_some())
                        .count()
                    + consumer_carriers
                    < MAX_GROUP_DEPTH;
            let Some(providers) = providers.filter(|_| eligible) else {
                self.warn(Limitation::TrackMatte, Some(context.comp_id), Some(source.record.id()), "Set Matte needs unique independent 2D shape or precomposition providers, an absent or resolved supported Alpha matte, and bounded nesting; unsupported dependency retained without lowering".into());
                continue;
            };
            // Prepare all helpers before replacing any existing content. Converter
            // allocation and animation/shape budgets also apply to these copies.
            let helper_context = LayerContext {
                depth: context.depth
                    + links.len()
                    + links
                        .iter()
                        .filter(|link| link.projection.is_some())
                        .count()
                    + consumer_carriers,
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
                if let Some(channel) = link.projection {
                    let projection_id = self.allocate_id()?;
                    let duration = TimeRangeProperty::new(
                        Time::ZERO,
                        self.duration(context.comp_id, context.comp.duration_secs),
                    );
                    let mut projection = group(
                        projection_id,
                        format!("Editable premultiplied {channel:?} matte"),
                        Some(context.parent),
                        duration,
                    );
                    let mut backing = super::transform::solid_rect(
                        &crate::structure::SolidSource {
                            width: context.comp.width,
                            height: context.comp.height,
                            pixel_aspect: (1, 1),
                            color: [0.0; 3],
                        },
                        &projection,
                        self.allocate_id()?,
                        projection.transform,
                    );
                    backing.name = "Matte black backing".into();
                    backing.description = "Opaque editable black backing prevents the Luma mask from multiplying sampled alpha twice".into();
                    backing.active_range = duration;
                    helper.parent = Some(projection_id);
                    projection.layers =
                        stored_layers(vec![FxLayer::Group(helper), FxLayer::Rect(backing)])?;
                    helper = projection;
                    // Luma sampling multiplies RGB by alpha. Compose the sampled
                    // premultiplied colour over opaque black first, then isolate
                    // one channel and HSV-desaturate: gray=C*A, output alpha=1.
                    for effect in [
                        fx_schema::LayerEffect::ShiftChannels {
                            take_red_from: if channel == ChannelSource::Red {
                                channel
                            } else {
                                ChannelSource::FullOff
                            },
                            take_green_from: if channel == ChannelSource::Green {
                                channel
                            } else {
                                ChannelSource::FullOff
                            },
                            take_blue_from: if channel == ChannelSource::Blue {
                                channel
                            } else {
                                ChannelSource::FullOff
                            },
                        },
                        fx_schema::LayerEffect::HueSaturation {
                            hue: 0.0,
                            saturation: -100.0,
                            lightness: 0.0,
                            colorize: false,
                            colorize_hue: 0.0,
                            colorize_saturation: 0.0,
                            colorize_lightness: 0.0,
                        },
                    ] {
                        let id = fx_schema::EffectId::new(self.allocate_id()?.value());
                        helper.effects.push(fx_schema::EffectRecord::from_data(
                            &fx_schema::EffectData::Identified {
                                id,
                                enabled: true,
                                effect: fx_schema::EffectPayload::Known(effect),
                            },
                        )?);
                    }
                    self.warn(Limitation::TrackMatte, Some(context.comp_id), Some(source.record.id()),
                        format!("Set Matte {channel:?} uses an opaque black backing, editable own/off ShiftChannels -> HSV desaturation -> Luma matte. Coverage is target alpha times sampled premultiplied {channel:?}. Composition-space sampling, source clipping and independent provider copies remain approximations; native Hue/Saturation export has a different grayscale transfer and does not reconstruct Set Matte. RGB of the picture is unchanged; this is not alpha inversion."));
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
                let matte = TrackMatte {
                    mode,
                    layer: helper.id,
                };
                if !sibling_gates {
                    wrapper.track_matte = Some(matte.clone());
                }
                helper.parent = Some(id);
                let previous = std::mem::replace(&mut layers[index], FxLayer::Group(wrapper));
                let FxLayer::Group(mut previous) = previous else {
                    unreachable!("checked Group above")
                };
                if sibling_gates {
                    let carrier_id = self.allocate_id()?;
                    let mut carrier = group(
                        carrier_id,
                        "Set Matte consumer".into(),
                        Some(id),
                        TimeRangeProperty::new(
                            Time::ZERO,
                            self.duration(context.comp_id, context.comp.duration_secs),
                        ),
                    );
                    previous.parent = Some(carrier_id);
                    carrier.layers = stored_layers(vec![FxLayer::Group(previous)])?;
                    previous = carrier;
                }
                previous.parent = Some(id);
                if sibling_gates {
                    // Same-parent consumer/provider keeps the native exported
                    // matte reference inside their shared composition boundary.
                    previous.track_matte = Some(matte);
                }
                let children =
                    stored_layers(vec![FxLayer::Group(previous), FxLayer::Group(helper)])?;
                if let FxLayer::Group(wrapper) = &mut layers[index] {
                    wrapper.layers = children;
                }
            }
            self.warn(Limitation::TrackMatte, Some(context.comp_id), Some(source.record.id()),
                format!("Set Matte3 lowered to {} independent editable gates from native samples {links:?}; only static Alpha/Luma or the bounded premultiplied RGB projection with noninverted/composite-original defaults are supported. Composition-space sampling approximates AE pre-transform/stretch semantics; provider edits are not linked, and unsupported dynamic paths retain their diagnosed initial outlines. Native-effect omission diagnostics refer to the effect adapter, not these structural gates; Adobe fidelity is unverified", links.len()));
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
