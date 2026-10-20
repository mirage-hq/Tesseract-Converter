//! Camera/implicit-camera magnification ratio for independently sampled XYZ paint.

use super::*;
use crate::expression_samples::{ExpressionSamples, PropertyIdentity};

pub(in crate::structure_document) fn prepare(
    (comp_id, comp): (u32, &Composition),
    owner: &Layer,
    samples: &ExpressionSamples,
    root_canvas: Option<[u16; 2]>,
    emitted_depth: bool,
) -> Result<Projection, String> {
    let flags = owner.record.flags();
    if !emitted_depth
        || root_canvas != Some([comp.width, comp.height])
        || !flags.three_d_layer
        || flags.null_layer
        || flags.collapse_transformation
        || !matches!(owner.record.layer_type(), 0 | 3 | 4)
        || owner.record.parent_id() != 0
        || owner.record.auto_orient() != 0
    {
        return Err(
            "sampled depth requires unparented untilted paint on the root-sized canvas".into(),
        );
    }
    let controls = properties(owner)?;
    identity_orientation(&controls)?;
    if !property(&controls, "ADBE Position")?
        .is_some_and(|position| position.expression_enabled && !position.dimensions_separated)
    {
        return Err("sampled depth requires an enabled unseparated Position expression".into());
    }
    let anchor = static_value(&controls, "ADBE Anchor Point", &[0.0; 3])?;
    if anchor.get(2).is_some_and(|z| *z != 0.0) {
        return Err("nonzero paint Anchor Z".into());
    }
    let identity = PropertyIdentity::Transform {
        match_name: "ADBE Position".into(),
    };
    let samples = samples
        .lookup(comp_id, owner.record.id(), &identity)
        .ok_or("no complete evaluated XYZ position")?;
    if samples.values.is_empty() || samples.values.len() > 250_000 {
        return Err("invalid sampled-depth observation count".into());
    }
    let mut cameras = comp
        .layers
        .iter()
        .filter(|layer| layer.record.layer_type() == 2 && layer.record.flags().enabled);
    let camera = cameras.next().ok_or("no enabled camera")?;
    if cameras.next().is_some()
        || camera.record.parent_id() != 0
        || !matches!(camera.record.auto_orient(), 0 | 2)
    {
        return Err("sampled depth requires one unparented camera".into());
    }
    let NumericAnimationClock::ParentIdentity { start, stretch } =
        NumericAnimationClock::parent_identity(camera)?
    else {
        return Err("invalid camera clock".into());
    };
    if stretch <= 0.0
        || start + stretch * camera.record.in_point().ok_or("invalid camera in point")? > 0.001
        || start
            + stretch
                * camera
                    .record
                    .out_point()
                    .ok_or("invalid camera out point")?
            + 0.001
            < comp.duration_secs
    {
        return Err("camera does not cover the full forward composition".into());
    }
    let center = [f64::from(comp.width) / 2.0, f64::from(comp.height) / 2.0];
    let controls = properties(camera)?;
    identity_orientation(&controls)?;
    if static_value(&controls, "ADBE Anchor Point", &[center[0], center[1], 0.0])?
        != [center[0], center[1], 0.0]
        || static_value(&controls, "ADBE Rotate Z", &[0.0])? != [0.0]
        || static_value(&controls, "ADBE Scale", &[1.0; 3])? != [1.0; 3]
    {
        return Err("camera aim/rotation/scale is not axial".into());
    }
    let position = property(&controls, "ADBE Position")?.ok_or("missing camera position")?;
    let depth = axial_translation(position, center)?;
    let zoom = camera_zoom(camera, comp.width)?;
    let d = f64::from(comp.width) * fx_schema::DEFAULT_CAMERA_DISTANCE_PER_WIDTH;
    let mut projected = samples.clone();
    if projected.sample_times_seconds.is_empty() {
        projected.sample_times_seconds = (0..samples.values.len())
            .map(|index| (samples.start_ms as f64 + index as f64) / 1000.0)
            .collect();
    }
    if projected.sample_times_seconds.len() != samples.values.len() {
        return Err("incomplete depth sample times".into());
    }
    for (values, &time) in projected
        .values
        .iter_mut()
        .zip(&projected.sample_times_seconds)
    {
        let [_, _, z] = values.as_slice() else {
            return Err("position samples are not XYZ".into());
        };
        let camera_z = depth_at(&depth, (time - start) / stretch)?;
        let distance = z - camera_z;
        if !distance.is_finite() || distance <= 0.0 || d + z <= 0.0 || camera_z >= 0.0 {
            return Err("sampled paint reaches/crosses a camera plane".into());
        }
        // The inner XYZ transform already applies D/(D+z); compensate, do not project twice.
        let ratio = 100.0 * zoom * (d + z) / (d * distance);
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err("nonfinite sampled camera magnification".into());
        }
        *values = vec![ratio];
    }
    projected.frame_sampled = true;
    let base = projected.values[0][0];
    Ok(Projection {
        center,
        scale: constant(base),
        clock: NumericAnimationClock::source_local(),
        samples: Some(projected),
    })
}

fn depth_at(property: &NumericProperty, time: f64) -> Result<f64, String> {
    let keys = &property.keyframes;
    let next = keys.partition_point(|key| key.time_secs <= time);
    let values = if keys.is_empty() {
        property.values.clone()
    } else if next == 0 {
        keys[0].values.clone()
    } else if next == keys.len() {
        keys[next - 1].values.clone()
    } else {
        animation::evaluate_numeric_segment(&keys[next - 1], &keys[next], time)?
    };
    values
        .first()
        .copied()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "missing camera depth".into())
}
