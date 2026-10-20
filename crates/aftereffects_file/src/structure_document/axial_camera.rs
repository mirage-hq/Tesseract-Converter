//! Bounded explicit-camera projection for an untilted composition-space plane.

mod sampled_depth;
pub(super) use sampled_depth::prepare as prepare_sampled_depth;

#[cfg(test)]
mod tests;

use fx_schema::animator::AnimationGraphEntry;
use fx_schema::{GroupLayer, LayerId, Position, PropType, PropertyTarget};

use super::{
    animation::{self, NumericAnimationClock, NumericAnimationTarget},
    animation_budget::AnimationBudget,
};
use crate::{
    properties::{
        NumericProperty, TransformProperty, read_numeric, read_transform, root_runs, runs,
        unique_list,
    },
    structure::{Composition, Layer},
};

pub(super) struct Projection {
    center: [f64; 2],
    scale: NumericProperty,
    clock: NumericAnimationClock,
    samples: Option<crate::expression_samples::EvaluatedProperty>,
}

fn properties(layer: &Layer) -> Result<Vec<TransformProperty>, String> {
    read_transform(&layer.content).map_err(|error| error.to_string())
}

fn property<'a>(
    properties: &'a [TransformProperty],
    name: &str,
) -> Result<Option<&'a NumericProperty>, String> {
    properties
        .iter()
        .find(|property| property.match_name == name)
        .map(|property| property.numeric.as_ref().map_err(|error| error.to_string()))
        .transpose()
}

fn static_value(
    properties: &[TransformProperty],
    name: &str,
    default: &[f64],
) -> Result<Vec<f64>, String> {
    let Some(property) = property(properties, name)? else {
        return Ok(default.to_vec());
    };
    if property.animated
        || property.expression_enabled
        || property.dimensions_separated
        || !property.keyframes.is_empty()
        || property.values.iter().any(|value| !value.is_finite())
    {
        return Err(format!("{name} is not a finite static camera control"));
    }
    Ok(property.values.clone())
}

fn identity_orientation(properties: &[TransformProperty]) -> Result<(), String> {
    for name in ["ADBE Rotate X", "ADBE Rotate Y", "ADBE Orientation"] {
        if static_value(properties, name, &[0.0])?
            .iter()
            .any(|value| *value != 0.0)
        {
            return Err(format!("{name} tilts the supported plane/camera"));
        }
    }
    Ok(())
}

fn planar(layer: &Layer) -> Result<(), String> {
    if layer.record.auto_orient() != 0 {
        return Err("auto-oriented paint plane".into());
    }
    let properties = properties(layer)?;
    identity_orientation(&properties)?;
    let separated =
        property(&properties, "ADBE Position")?.is_some_and(|value| value.dimensions_separated);
    for name in ["ADBE Anchor Point", "ADBE Position"] {
        let Some(property) = property(&properties, name)? else {
            continue;
        };
        if name == "ADBE Position" && separated {
            continue;
        }
        if property.expression_enabled
            || property.values.get(2).is_some_and(|z| *z != 0.0)
            || property.keyframes.iter().any(|key| {
                key.values.get(2).is_some_and(|z| *z != 0.0)
                    || key.spatial_in.get(2).is_some_and(|z| *z != 0.0)
                    || key.spatial_out.get(2).is_some_and(|z| *z != 0.0)
            })
        {
            return Err(format!("{name} leaves the static Z=0 plane"));
        }
    }
    if separated
        && property(&properties, "ADBE Position_2")?.is_some_and(|property| {
            property.expression_enabled
                || property.values.iter().any(|value| *value != 0.0)
                || property
                    .keyframes
                    .iter()
                    .any(|key| key.values.iter().any(|value| *value != 0.0))
        })
    {
        return Err("separated Z position leaves the plane".into());
    }
    Ok(())
}

pub(super) fn prepare(
    comp: &Composition,
    owner: &Layer,
    ancestors: &[&Layer],
) -> Result<Option<Projection>, String> {
    if !owner.record.flags().three_d_layer
        || owner.record.flags().null_layer
        || !matches!(owner.record.layer_type(), 0 | 3 | 4)
    {
        return Ok(None);
    }
    let mut cameras = comp
        .layers
        .iter()
        .filter(|layer| layer.record.layer_type() == 2 && layer.record.flags().enabled);
    let Some(camera) = cameras.next() else {
        return Ok(None);
    };
    if cameras.next().is_some() {
        return Err("multiple enabled cameras".into());
    }
    planar(owner)?;
    for parent in ancestors {
        planar(parent)?;
    }
    let center = [f64::from(comp.width) / 2.0, f64::from(comp.height) / 2.0];
    let clock = NumericAnimationClock::parent_identity(camera)?;
    let input = camera.record.in_point().ok_or("invalid camera in point")?;
    let output = camera
        .record
        .out_point()
        .ok_or("invalid camera out point")?;
    if clock.reversed()
        || clock.seconds(input) > 0.001
        || clock.seconds(output) + 0.001 < comp.duration_secs
    {
        return Err("camera does not cover the full composition".into());
    }
    let controls = properties(camera)?;
    identity_orientation(&controls)?;
    if static_value(&controls, "ADBE Rotate Z", &[0.0])? != [0.0]
        || static_value(&controls, "ADBE Scale", &[1.0; 3])? != [1.0; 3]
    {
        return Err("camera rotation/scale is not identity".into());
    }
    let position = static_value(&controls, "ADBE Position", &[])?;
    let aim = static_value(&controls, "ADBE Anchor Point", &[center[0], center[1], 0.0])?;
    if position.len() != 3 || aim != [position[0], position[1], 0.0] || position[2] >= 0.0 {
        return Err("camera is not aimed along positive Z at the plane".into());
    }
    let zoom = camera_zoom(camera, comp.width)?;
    let (mut translation, translation_clock) = if camera.record.parent_id() == 0 {
        if position[..2] != center {
            return Err("off-center camera".into());
        }
        (constant(0.0), clock)
    } else {
        let parent = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == camera.record.parent_id())
            .ok_or("missing camera parent")?;
        if parent.record.parent_id() != 0 {
            return Err("multilevel camera parenting".into());
        }
        if parent.record.auto_orient() != 0 {
            return Err("auto-oriented camera parent".into());
        }
        let controls = properties(parent)?;
        identity_orientation(&controls)?;
        if static_value(&controls, "ADBE Rotate Z", &[0.0])? != [0.0]
            || static_value(&controls, "ADBE Scale", &[1.0; 3])? != [1.0; 3]
            || static_value(&controls, "ADBE Anchor Point", &[0.0; 3])? != [0.0; 3]
        {
            return Err("camera parent has nonidentity rotation/scale/anchor".into());
        }
        let parent_position =
            property(&controls, "ADBE Position")?.ok_or("missing camera-parent position")?;
        (
            axial_translation(
                parent_position,
                [center[0] - position[0], center[1] - position[1]],
            )?,
            NumericAnimationClock::parent_identity(parent)?,
        )
    };
    if translation_clock.reversed() {
        return Err("reverse camera-parent clock".into());
    }
    project_magnification(&mut translation, position[2], zoom)?;
    Ok(Some(Projection {
        center,
        scale: translation,
        clock: translation_clock,
        samples: None,
    }))
}

fn camera_zoom(camera: &Layer, width: u16) -> Result<f64, String> {
    // Sparse native camera options use AE's 50mm / 36mm horizontal film default.
    let mut zoom = f64::from(width) * 50.0 / 36.0;
    for (name, run) in root_runs(&camera.content).map_err(|error| error.to_string())? {
        if name != "ADBE Camera Options Group" {
            continue;
        }
        let options = unique_list(run, *b"tdgp").map_err(|error| error.to_string())?;
        for (name, run) in runs(options).map_err(|error| error.to_string())? {
            if !matches!(name, "ADBE Camera Zoom" | "ADBE Camera Depth of Field") {
                continue;
            }
            let property =
                read_numeric(unique_list(run, *b"tdbs").map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            if property.animated
                || property.expression_enabled
                || property.values.len() != 1
                || !property.values[0].is_finite()
            {
                return Err(format!("unsupported {name}"));
            }
            if name == "ADBE Camera Zoom" {
                zoom = property.values[0];
            } else if property.values[0] != 0.0 {
                return Err("camera depth of field is enabled".into());
            }
        }
    }
    if zoom <= 0.0 {
        return Err("nonpositive camera zoom".into());
    }
    Ok(zoom)
}

fn constant(value: f64) -> NumericProperty {
    NumericProperty {
        values: vec![value],
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind: crate::properties::NumericValueKind::Continuous,
    }
}

fn axial_translation(source: &NumericProperty, xy: [f64; 2]) -> Result<NumericProperty, String> {
    if source.expression_enabled || source.dimensions_separated {
        return Err("expressed/separated camera translation".into());
    }
    let valid = |values: &[f64]| {
        values.len() == 3 && values[..2] == xy && values.iter().all(|value| value.is_finite())
    };
    if !source.animated {
        return valid(&source.values)
            .then(|| constant(source.values[2]))
            .ok_or_else(|| "off-axis static camera translation".into());
    }
    if source.keyframes.is_empty() || source.keyframes.iter().any(|key| !valid(&key.values)) {
        return Err("off-axis camera keys".into());
    }
    for pair in source.keyframes.windows(2) {
        let (from, to) = (&pair[0], &pair[1]);
        let low = from.values[2].min(to.values[2]);
        let high = from.values[2].max(to.values[2]);
        for (key, handles) in [(from, &from.spatial_out), (to, &to.spatial_in)] {
            if !handles.is_empty()
                && (handles.len() != 3
                    || handles[..2] != [0.0; 2]
                    || !handles[2].is_finite()
                    || !(low..=high).contains(&(key.values[2] + handles[2])))
            {
                return Err("camera path is not a monotone straight Z segment".into());
            }
        }
    }
    let mut result = source.clone();
    result.values.clear();
    for index in 0..result.keyframes.len() {
        let key = &source.keyframes[index];
        let shared = !key.spatial_in.is_empty() || !key.spatial_out.is_empty();
        let signed_speed =
            |speed: &[f64], other: usize, incoming: bool| -> Result<Vec<f64>, String> {
                if shared {
                    if speed.len() != 1 || !speed[0].is_finite() || speed[0] < 0.0 {
                        return Err("invalid camera path speed".into());
                    }
                    let delta = if incoming {
                        key.values[2] - source.keyframes[other].values[2]
                    } else {
                        source.keyframes[other].values[2] - key.values[2]
                    };
                    Ok(vec![speed[0] * delta.signum()])
                } else {
                    speed
                        .get(2)
                        .copied()
                        .map(|value| vec![value])
                        .ok_or_else(|| "missing camera Z speed".into())
                }
            };
        let target = &mut result.keyframes[index];
        target.values = vec![key.values[2]];
        target.in_speed = signed_speed(&key.in_speed, index.saturating_sub(1), true)?;
        target.out_speed = signed_speed(
            &key.out_speed,
            (index + 1).min(source.keyframes.len() - 1),
            false,
        )?;
        target.in_influence = vec![
            *key.in_influence
                .get(if shared { 0 } else { 2 })
                .ok_or("missing camera in influence")?,
        ];
        target.out_influence = vec![
            *key.out_influence
                .get(if shared { 0 } else { 2 })
                .ok_or("missing camera out influence")?,
        ];
        target.spatial_in.clear();
        target.spatial_out.clear();
    }
    Ok(result)
}

fn project_magnification(
    property: &mut NumericProperty,
    local_z: f64,
    zoom: f64,
) -> Result<(), String> {
    let project = |z: f64| {
        let distance = -local_z - z;
        if !distance.is_finite() || distance <= 0.0 {
            return Err("camera reaches/crosses the paint plane".to_owned());
        }
        let scale = 100.0 * zoom / distance;
        let derivative = scale / distance;
        if !scale.is_finite() || !derivative.is_finite() {
            return Err("camera magnification overflows".to_owned());
        }
        Ok((scale, derivative))
    };
    for pair in property.keyframes.windows(2) {
        let duration = pair[1].time_secs - pair[0].time_secs;
        if !duration.is_finite() || duration <= 0.0 {
            return Err("invalid camera key times".into());
        }
        for (key, sign, interpolation, speed, influence) in [
            (
                &pair[0],
                1.0,
                pair[0].out_interpolation,
                &pair[0].out_speed,
                &pair[0].out_influence,
            ),
            (
                &pair[1],
                -1.0,
                pair[1].in_interpolation,
                &pair[1].in_speed,
                &pair[1].in_influence,
            ),
        ] {
            if interpolation == 2 {
                let control = key.values[0] + sign * speed[0] * duration * influence[0] / 100.0;
                project(control)?;
            }
        }
    }
    if let Some(value) = property.values.first_mut() {
        *value = project(*value)?.0;
    }
    for key in &mut property.keyframes {
        let (scale, derivative) = project(key.values[0])?;
        key.values[0] = scale;
        for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
            *speed *= derivative;
        }
    }
    Ok(())
}

impl Projection {
    pub(super) fn has_sampled_depth(&self) -> bool {
        self.samples.is_some()
    }

    pub(super) fn stage(
        self,
        parent: Option<LayerId>,
        id: LayerId,
        budget: &mut AnimationBudget,
    ) -> Result<(GroupLayer, Vec<AnimationGraphEntry>), String> {
        let base = self
            .scale
            .values
            .first()
            .copied()
            .or_else(|| self.scale.keyframes.first().map(|key| key.values[0]))
            .ok_or("missing magnification")?;
        let checkpoint = budget.checkpoint();
        let targets = [PropType::ScaleX, PropType::ScaleY].map(|property| {
            NumericAnimationTarget::float(PropertyTarget::layer(id, property), 0, 1.0)
        });
        let (entries, warnings) = if let Some(samples) = &self.samples {
            animation::evaluated_numeric_entries(
                "axial camera depth compensation",
                samples,
                &targets,
                &[0.0; 2],
                budget,
            )
        } else {
            animation::numeric_entries(
                "axial camera magnification",
                &self.scale,
                &targets,
                self.clock,
                budget,
            )
        };
        if (self.scale.animated || self.samples.is_some()) && entries.len() != 2 {
            budget.rollback(checkpoint);
            return Err(warnings.join("; "));
        }
        let mut wrapper = super::group(
            id,
            "Axial camera projection".into(),
            parent,
            fx_schema::TimeRangeProperty::new(
                fx_schema::Time::ZERO,
                fx_schema::Duration::from_secs(super::MAX_TIME_SECS),
            ),
        );
        wrapper.transform.anchor_point = self.center;
        wrapper.transform.position = Position::TwoD(self.center);
        wrapper.transform.scale = [base; 2];
        Ok((wrapper, entries))
    }
}
