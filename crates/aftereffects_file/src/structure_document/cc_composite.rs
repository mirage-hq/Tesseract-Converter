//! Bounded original-source restoration at a native CC Composite stage.
use crate::{effects::native, properties, structure::Layer};
use fx_schema::{GroupLayer, LayerData, LayerId, TimeRemapProperty, animator::KeyframeId};

use super::{group, remapped_playback, stored_layers};
const NAME: &str = "CC Composite";

pub(super) fn profile(
    layer: &Layer,
    size: [u16; 2],
    effect_definitions: &[crate::rifx::Chunk],
) -> Result<Option<usize>, String> {
    let (effects, _) = native::read_effects(&layer.content, size.map(f64::from));
    let mut composites = effects.iter().filter(|e| e.match_name == NAME && e.enabled);
    let Some(effect) = composites.next() else {
        return Ok(None);
    };
    if !layer.record.flags().effects_active {
        return Ok(None);
    }
    if composites.next().is_some() {
        return Err("multiple CC Composite stages are outside the bounded source graph".into());
    }
    // Other graph-lowered effects need independent placement around this stage.
    if effects.iter().any(|e| {
        e.enabled
            && !matches!(
                e.match_name.as_str(),
                NAME | "ADBE Fill"
                    | "ADBE Tint"
                    | "ADBE Gaussian Blur 2"
                    | "CC RepeTile"
                    | "ADBE Laser"
                    | "ADBE Color Control"
                    | "ADBE Slider Control"
                    | "ADBE Checkbox Control"
                    | "ADBE Point Control"
                    | "ADBE Angle Control"
            )
    }) {
        return Err(
            "mixed spatial/generator stages outside the bounded CC Composite profile".into(),
        );
    }
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let (_, parade) = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Parade")
        .ok_or("missing Effect Parade")?;
    let occurrences =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let (_, run) = occurrences
        .get(effect.index - 1)
        .ok_or("missing CC Composite occurrence")?;
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    properties::group_enabled(descriptor).map_err(|e| e.to_string())?;
    let controls =
        properties::runs(properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    for (name, run) in &controls {
        if !seen.insert(name) {
            return Err("duplicate CC Composite control".into());
        }
        match *name {
            "CC Composite-0000" | "CC Composite-0001" | "CC Composite-0002"
            | "CC Composite-0003" | "ADBE Group End" => {}
            "ADBE Effect Built In Params" => {
                let options = properties::runs(
                    properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                    return Err(
                        "explicit CC Composite wet/dry or mask options are unsupported".into(),
                    );
                }
            }
            _ => return Err(format!("unknown CC Composite control {name}")),
        }
    }
    let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
    for (suffix, expected) in [(1, 100.), (2, 3.), (3, 1.)] {
        let name = format!("{NAME}-{suffix:04}");
        // Only absent controls in an empty instance table may inherit file defaults.
        let shared;
        let numeric = if table.is_empty() && !controls.iter().any(|(id, _)| *id == name) {
            let declarations = crate::essential::file_effect_declarations(effect_definitions, NAME)
                .map_err(|warning| warning.message)?;
            let declarations = properties::runs(declarations).map_err(|e| e.to_string())?;
            let matches: Vec<_> = declarations.iter().filter(|(id, _)| *id == name).collect();
            let [(_, declaration)] = matches.as_slice() else {
                return Err(format!("{name}: unique shared declaration required"));
            };
            shared = native::default_numeric(declaration, size.map(f64::from))
                .map_err(|e| e.to_string())?;
            &shared
        } else {
            effect
                .parameters
                .iter()
                .find(|p| p.match_name == name)
                .and_then(|p| p.numeric.as_ref().ok())
                .ok_or_else(|| format!("{name}: missing or malformed native control/default"))?
        };
        if numeric.animated
            || !numeric.keyframes.is_empty()
            || numeric.expression_enabled
            || numeric.expression_present
            || numeric.dimensions_separated
            || numeric.values != [expected]
        {
            return Err("requires static Opacity 100, In front and RGB Only".into());
        }
    }
    Ok(Some(effect.index))
}

/// Copy only an empty source clock, not source paint or its graph identities.
pub(super) fn clock(
    source: &GroupLayer,
    id: LayerId,
    parent: LayerId,
) -> Result<(GroupLayer, Option<TimeRemapProperty>), String> {
    let mut copy = source.clone();
    copy.id = id;
    copy.parent = Some(parent);
    copy.layers.clear();
    let remap = source
        .playback
        .time_remap()
        .map(|remap| {
            let keys = remap
                .keyframes()
                .iter()
                .enumerate()
                .map(|(index, key)| {
                    let mut key = key.clone();
                    key.id = KeyframeId::new(format!("aep-composite-{id}-{index}"));
                    key
                })
                .collect();
            TimeRemapProperty::new(keys, remap.before(), remap.after()).map_err(|e| e.to_string())
        })
        .transpose()?;
    if let Some(remap) = &remap {
        copy.playback = remapped_playback(source.playback.input_range(), remap.clone());
    }
    Ok((copy, remap))
}

pub(super) fn assemble(
    owner: &mut GroupLayer,
    mut original: GroupLayer,
    prefix: usize,
    output_id: LayerId,
    processed_id: LayerId,
) -> Result<(), serde_json::Error> {
    let mut output = group(
        output_id,
        "CC Composite output".into(),
        Some(owner.id),
        owner.playback.input_range(),
    );
    let mut processed = group(
        processed_id,
        "CC Composite processed input".into(),
        Some(output_id),
        owner.playback.input_range(),
    );
    processed.effects = owner.effects.drain(..prefix).collect();
    processed.layers = std::mem::take(&mut owner.layers);
    for child in &mut processed.layers {
        let mut data = child.data().clone();
        let LayerData::Group(group) = &mut data else {
            unreachable!("the importer publishes source content in a Group")
        };
        group.parent = Some(processed_id);
        *child = fx_schema::Layer::from_data(&data)?;
    }
    original.name = "CC Composite original source clock".into();
    original.parent = Some(output_id);
    // Original is restored above the processed image; owner transforms/styles run once.
    output.layers = stored_layers(vec![
        LayerData::Group(original),
        LayerData::Group(processed),
    ])?;
    owner.layers = stored_layers(vec![LayerData::Group(output)])?;
    Ok(())
}
