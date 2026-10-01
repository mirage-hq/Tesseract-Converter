//! Bounded 50% Radial Wipe as a source-local editable half-plane mask.

use fx_schema::animator::AnimationGraphEntry;
use fx_schema::layer::{MaskMode, PathMask, ShapeLayer};
use fx_schema::{
    Duration, FxItemId, GroupLayer, LayerData, LayerId, NonNegativeProperty, PercentageProperty,
    Position, PropertyTarget, ShapeContent, ShapePath, ShapePathCommand, Time, TimeRangeProperty,
    Transform,
};

use super::animation::{self, NumericAnimationClock, NumericAnimationTarget};
use super::animation_budget::AnimationBudget;
use super::control_links;
use crate::effects::native::{self, DecodedEffect};
use crate::properties::{self, NumericProperty};
use crate::structure::{Composition, ItemKind, Layer, ProjectItem};

const MATCH_NAME: &str = "ADBE Radial Wipe";

fn parameter<'a>(effect: &'a DecodedEffect, suffix: &str) -> Result<&'a NumericProperty, String> {
    let name = format!("{MATCH_NAME}-{suffix}");
    let mut matches = effect.parameters.iter().filter(|p| p.match_name == name);
    let value = matches.next().ok_or_else(|| format!("missing {name}"))?;
    if matches.next().is_some() {
        return Err(format!("ambiguous {name}"));
    }
    value.numeric.as_ref().map_err(|error| error.to_string())
}

fn static_values<'a>(
    effect: &'a DecodedEffect,
    suffix: &str,
    dimensions: usize,
) -> Result<&'a [f64], String> {
    let value = parameter(effect, suffix)?;
    if value.animated
        || value.expression_enabled
        || !value.keyframes.is_empty()
        || value.values.len() != dimensions
        || value.values.iter().any(|v| !v.is_finite())
    {
        return Err(format!(
            "{suffix} must be a finite static {dimensions}-component control"
        ));
    }
    Ok(&value.values)
}

fn descriptor(
    layer: &Layer,
    index: usize,
) -> Result<&[crate::rifx::Chunk], properties::PropertyError> {
    let roots = properties::root_runs(&layer.content)?;
    let parade = control_links::unique_run(&roots, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let (name, effect) = effects
        .get(
            index
                .checked_sub(1)
                .ok_or(properties::PropertyError::Layout("invalid effect index"))?,
        )
        .ok_or(properties::PropertyError::Layout(
            "missing effect occurrence",
        ))?;
    if *name != MATCH_NAME {
        return Err(properties::PropertyError::Layout("wrong effect occurrence"));
    }
    properties::unique_list(effect, *b"sspc")
}

fn angle_expression(layer: &Layer, index: usize) -> Result<&str, String> {
    let read = || {
        let controls = properties::runs(properties::unique_list(
            descriptor(layer, index)?,
            *b"tdgp",
        )?)?;
        let angle = control_links::unique_run(&controls, "ADBE Radial Wipe-0002")?;
        control_links::expression(properties::unique_list(angle, *b"tdbs")?)
    };
    read().map_err(|error| error.to_string())
}

fn center(layer: &Layer, effect: &DecodedEffect, size: [u16; 2]) -> Result<[f64; 2], String> {
    let values = static_values(effect, "0003", 2)?;
    let read = || {
        let descriptor = descriptor(layer, effect.index)?;
        let controls = properties::runs(properties::unique_list(descriptor, *b"tdgp")?)?;
        if controls
            .iter()
            .any(|(name, _)| *name == "ADBE Radial Wipe-0003")
        {
            return Ok([values[0], values[1]]);
        }
        // PF_Point defaults are signed 16:16 PERCENTAGES, unlike the explicit
        // point's normalized coordinates. Keep this correction local to the
        // independently proved Radial Wipe profile, not other effect mappings.
        let definitions = properties::runs(properties::unique_list(descriptor, *b"parT")?)?;
        let definition = control_links::unique_run(&definitions, "ADBE Radial Wipe-0003")?;
        let bytes = properties::data(definition, *b"pard")?;
        if bytes.len() != 148 || bytes[12..16] != 6_u32.to_be_bytes() {
            return Err(properties::PropertyError::Layout(
                "unsupported Wipe Center default",
            ));
        }
        let coordinate = |offset, dimension| {
            let raw = i32::from_be_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("validated point default"),
            );
            f64::from(raw) / 65_536.0 / 100.0 * f64::from(dimension)
        };
        Ok([coordinate(56, size[0]), coordinate(60, size[1])])
    };
    read().map_err(|error| error.to_string())
}

fn half_plane_path(size: [u16; 2], center: [f64; 2]) -> Result<ShapePath, String> {
    let x = center[0].abs().max((f64::from(size[0]) - center[0]).abs());
    let y = center[1].abs().max((f64::from(size[1]) - center[1]).abs());
    let d = x.hypot(y) + 1.0;
    // Float32 is the runtime geometry plane; keep both extent and the extra
    // pixel representable instead of admitting effectively infinite masks.
    if !d.is_finite() || d > 1_000_000.0 {
        return Err("half-plane extent exceeds the bounded geometry plane".into());
    }
    let line = |x, y| ShapePathCommand::LineTo {
        x,
        y,
        mirror: None,
        corner_radius: None,
    };
    Ok(ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: -d,
                y: -d,
                mirror: None,
                corner_radius: None,
            },
            line(0.0, -d),
            line(0.0, d),
            line(-d, d),
            ShapePathCommand::Close,
        ],
    })
}

/// No occurrence mutation, generated ID consumption, or animation reservation
/// survives a rejected profile. Source-stage matte copies never call this hook.
pub(super) fn apply(
    layer: &Layer,
    comp: &Composition,
    source: Option<&ProjectItem>,
    occurrence: &mut GroupLayer,
    next_id: &mut u64,
    budget: &mut AnimationBudget,
    supported_parent_plane: bool,
) -> Result<Option<Vec<AnimationGraphEntry>>, String> {
    if !layer.record.flags().effects_active {
        return Ok(None);
    }
    let (effects, warnings) = native::read_effects(
        &layer.content,
        [f64::from(comp.width), f64::from(comp.height)],
    );
    let enabled: Vec<_> = effects.iter().filter(|e| e.enabled).collect();
    if !enabled.iter().any(|e| e.match_name == MATCH_NAME) {
        return Ok(None);
    }
    if !warnings.is_empty() {
        return Err(format!("ambiguous native effects: {}", warnings.join("; ")));
    }
    let Some(effect) = enabled.last().filter(|e| e.match_name == MATCH_NAME) else {
        return Err("wipe must be the final enabled effect".into());
    };
    // FX path masks precede effects. Only alpha-preserving Tint/Fill may
    // commute with the half-plane; terminal placement alone is insufficient.
    if enabled[..enabled.len() - 1]
        .iter()
        .any(|e| !matches!(e.match_name.as_str(), "ADBE Fill" | "ADBE Tint"))
    {
        return Err("only alpha-preserving Fill/Tint may precede one wipe".into());
    }
    let flags = layer.record.flags();
    let Some(ProjectItem {
        kind: ItemKind::Composition(source_comp),
        ..
    }) = source
    else {
        return Err("only a composition-sized precomposition source is supported".into());
    };
    if !supported_parent_plane
        || flags.three_d_layer
        || flags.collapse_transformation
        || flags.adjustment_layer
        || source_comp.width != comp.width
        || source_comp.height != comp.height
        || comp.width == 0
        || comp.height == 0
        || comp.pixel_aspect.0 != comp.pixel_aspect.1
        || source_comp.pixel_aspect.0 != source_comp.pixel_aspect.1
    {
        return Err(
            "unsupported dimensions, pixel aspect, depth, or transformed source plane".into(),
        );
    }
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    if roots.iter().any(|(name, _)| *name == "ADBE Mask Parade") || !occurrence.masks.is_empty() {
        return Err("authored masks cannot be reordered across the wipe".into());
    }
    if static_values(effect, "0001", 1)? != [50.0]
        || static_values(effect, "0004", 1)? != [1.0]
        || static_values(effect, "0005", 1)? != [0.0]
    {
        return Err("requires exactly 50% completion, direction 1, and zero feather".into());
    }
    let center = center(layer, effect, [comp.width, comp.height])?;
    let path = half_plane_path([comp.width, comp.height], center)?;
    let mut angle = parameter(effect, "0002")?.clone();
    let linked = angle.expression_enabled;
    if linked {
        let text = angle_expression(layer, effect.index)?;
        control_links::lower_sibling_rotation_expression(layer, comp, &mut angle, text)
            .ok_or("Start Angle expression is outside bounded sibling Rotation+constant grammar")?
            .map_err(|e| e.to_string())?;
    }
    // Direct native Angle keys and lowered Rotation links share the bounded
    // numeric clock/easing/budget checks below; neither path samples or bakes.
    let initial = angle
        .keyframes
        .first()
        .map_or(angle.values.as_slice(), |key| &key.values);
    let [rotation] = initial else {
        return Err("Start Angle must be scalar".into());
    };
    if !rotation.is_finite() {
        return Err("nonfinite Start Angle".into());
    }
    let clock = NumericAnimationClock::parent_identity(layer).map_err(|e| e.to_string())?;
    let mut candidate_id = *next_id;
    let raw =
        super::reserve_ids(&mut candidate_id, 2).ok_or("generated identifier space exhausted")?;
    let guide_id = LayerId::new(raw + 1);
    let checkpoint = budget.checkpoint();
    let generated = (|| {
        let target = NumericAnimationTarget::float(
            PropertyTarget::layer(guide_id, fx_schema::PropType::Rotation),
            0,
            1.0,
        );
        let (animations, warnings) =
            animation::numeric_entries("Radial Wipe Start Angle", &angle, &[target], clock, budget);
        if !warnings.is_empty() || (angle.animated && animations.is_empty()) {
            return Err(format!(
                "Start Angle animation was not admitted atomically: {}",
                warnings.join("; ")
            ));
        }
        let transform = Transform {
            anchor_point: [0.0, 0.0],
            position: Position::TwoD(center),
            scale: [100.0, 100.0],
            rotation: *rotation,
            skew: 0.0,
            skew_axis: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            orientation: [0.0, 0.0, 0.0],
            opacity: PercentageProperty::new(100.0).expect("100 is valid"),
        };
        let guide = fx_schema::Layer::from_data(&LayerData::Shape(ShapeLayer {
            id: guide_id, parent: Some(occurrence.id), name: "Radial Wipe half-plane guide".into(),
            description: "Independent editable 50% hard-edge Radial Wipe; native angle 0 retains left, 90 retains top; antialiasing fidelity unverified".into(),
            is_hidden: false, blend_mode: Default::default(), track_matte: None, masks: vec![],
            active_range: TimeRangeProperty::new(Time::ZERO, Duration::from_secs(super::MAX_TIME_SECS)),
            effects: vec![], motion_blur: false, transform,
            shape: ShapeContent { path, fills: vec![], strokes: vec![], round_corners: None, offset_paths: None, trim: None, poly_star: None, ellipse: None },
        })).map_err(|e| e.to_string())?;
        Ok((guide, animations))
    })();
    let (guide, animations) = match generated {
        Ok(value) => value,
        Err(error) => {
            budget.rollback(checkpoint);
            return Err(error);
        }
    };
    occurrence.layers.push(guide);
    occurrence.masks.push(PathMask {
        id: FxItemId::new(raw),
        mode: MaskMode::Add,
        inverted: false,
        layer: Some(guide_id),
        legacy_path: None,
        feather: [0.0, 0.0],
        expansion: 0.0,
        opacity: NonNegativeProperty::new(1.0).expect("1 is non-negative"),
    });
    *next_id = candidate_id;
    Ok(Some(animations))
}

#[cfg(test)]
mod tests;
