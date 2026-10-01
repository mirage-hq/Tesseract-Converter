//! Inverse of the writer's canonical root-camera precomposition translation.
//!
//! This is deliberately a private converter convention, not a general AE
//! camera model. A camera must match every field emitted by `writer::camera`
//! before its composition is normalized.

use std::collections::HashMap;

use fx_schema::animator::AnimationGraphEntry;
use fx_schema::{LayerId, Position, PropType, PropertyTarget, Transform};

use crate::{
    properties::{NumericProperty, group_enabled, read_numeric, root_runs, runs, unique_list},
    schema::layer_records::LayerRecord,
    structure::{Composition, ItemKind, Layer, StructuralProject},
    timing::Duration24,
};

use super::{animation, animation_budget::AnimationBudget};

const GENERATED_CAMERA_NAME: &str = "FX root projection";
const CAMERA_DISTANCE_RATIO: f64 = 1.388;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CompositionNormalization {
    pub camera_layer_id: u32,
    pub offset: [f64; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct LayerCorrection {
    pub position: Option<[f64; 2]>,
    pub anchor: Option<[f64; 2]>,
}

impl LayerCorrection {
    pub fn is_identity(self) -> bool {
        self.position.is_none() && self.anchor.is_none()
    }
}

pub(super) fn find(
    project: &StructuralProject,
    root_width: u16,
    root_height: u16,
) -> HashMap<u32, CompositionNormalization> {
    let root_center = [f64::from(root_width) * 0.5, f64::from(root_height) * 0.5];
    let distance = f64::from(root_width) * CAMERA_DISTANCE_RATIO;
    project
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Composition(composition) = &item.kind else {
                return None;
            };
            canonical_camera(composition, distance).map(|camera_layer_id| {
                let local_center = [
                    f64::from(composition.width) * 0.5,
                    f64::from(composition.height) * 0.5,
                ];
                (
                    item.id,
                    CompositionNormalization {
                        camera_layer_id,
                        offset: [
                            root_center[0] - local_center[0],
                            root_center[1] - local_center[1],
                        ],
                    },
                )
            })
        })
        .collect()
}

pub(super) fn has_generated_camera_name(composition: &Composition) -> bool {
    composition
        .layers
        .iter()
        .any(|layer| layer.record.layer_type() == 2 && layer.name.as_ref() == GENERATED_CAMERA_NAME)
}

fn canonical_camera(composition: &Composition, distance: f64) -> Option<u32> {
    let mut cameras = composition
        .layers
        .iter()
        .filter(|layer| layer.record.layer_type() == 2);
    let camera = cameras.next()?;
    if cameras.next().is_some() || camera.name.as_ref() != GENERATED_CAMERA_NAME {
        return None;
    }
    let (duration_ticks, duration_denominator) = composition.record.duration_fraction().ok()?;
    if duration_denominator != 24_576 {
        return None;
    }
    let duration = Duration24::from_ticks(duration_ticks).ok()?;
    if camera.record != LayerRecord::camera_ae26(camera.record.id(), duration).ok()? {
        return None;
    }

    let center = [
        f64::from(composition.width) * 0.5,
        f64::from(composition.height) * 0.5,
    ];
    let is_camera = |index: usize| composition.layers[index].record.id() == camera.record.id();
    if composition.layers.iter().enumerate().any(|(index, layer)| {
        layer.record.parent_id() == camera.record.id()
            || layer.record.matte_layer_id_raw() == Some(camera.record.id())
            || super::compositing::matte_source(composition, index)
                .is_ok_and(|matte| matte.is_some_and(|(source, _)| is_camera(source)))
    }) {
        return None;
    }
    canonical_camera_properties(camera, center, distance).then_some(camera.record.id())
}

fn canonical_camera_properties(layer: &Layer, center: [f64; 2], distance: f64) -> bool {
    let Ok(roots) = root_runs(&layer.content) else {
        return false;
    };
    if roots.len() != 2
        || roots[0].0 != "ADBE Transform Group"
        || roots[1].0 != "ADBE Camera Options Group"
    {
        return false;
    }
    if group_enabled(roots[0].1) != Ok(true) || group_enabled(roots[1].1) != Ok(true) {
        return false;
    }
    let Ok(transform_group) = unique_list(roots[0].1, *b"tdgp") else {
        return false;
    };
    let Ok(transform_runs) = runs(transform_group) else {
        return false;
    };
    if transform_runs.len() != 2
        || transform_runs[0].0 != "ADBE Anchor Point"
        || transform_runs[1].0 != "ADBE Position"
    {
        return false;
    }
    let Ok(anchor_group) = unique_list(transform_runs[0].1, *b"tdbs") else {
        return false;
    };
    let Ok(position_group) = unique_list(transform_runs[1].1, *b"tdbs") else {
        return false;
    };
    if !is_static_numeric(read_numeric(anchor_group), &[center[0], center[1], 0.0])
        || !is_static_numeric(
            read_numeric(position_group),
            &[center[0], center[1], -distance],
        )
    {
        return false;
    }

    let Ok(options_group) = unique_list(roots[1].1, *b"tdgp") else {
        return false;
    };
    let Ok(option_runs) = runs(options_group) else {
        return false;
    };
    if option_runs.len() != 1 || option_runs[0].0 != "ADBE Camera Zoom" {
        return false;
    }
    let Ok(zoom_group) = unique_list(option_runs[0].1, *b"tdbs") else {
        return false;
    };
    is_static_numeric(read_numeric(zoom_group), &[distance])
}

fn is_static_numeric(
    property: Result<NumericProperty, crate::properties::PropertyError>,
    expected: &[f64],
) -> bool {
    let Ok(property) = property else {
        return false;
    };
    !property.animated
        && !property.expression_enabled
        && !property.expression_present
        && !property.dimensions_separated
        && property.keyframes.is_empty()
        && property.values.len() == expected.len()
        && property
            .values
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
}

pub(super) fn apply_static(
    transform: &mut Transform,
    correction: LayerCorrection,
) -> Result<(), &'static str> {
    let mut translated = *transform;
    if let Some(offset) = correction.position {
        let position = match &mut translated.position {
            Position::TwoD(position) => &mut position[..],
            Position::ThreeD(position) => &mut position[..2],
        };
        translate_pair(position, offset)?;
    }
    if let Some(offset) = correction.anchor {
        translate_pair(&mut translated.anchor_point, offset)?;
    }
    *transform = translated;
    Ok(())
}

fn translate_pair(values: &mut [f64], offset: [f64; 2]) -> Result<(), &'static str> {
    for (value, delta) in values.iter_mut().zip(offset) {
        *value += delta;
        if !value.is_finite() {
            return Err("inverse camera-origin translation produced a non-finite value");
        }
    }
    Ok(())
}

pub(super) fn corrected_transform_entries(
    layer: &Layer,
    composition: &Composition,
    target_id: LayerId,
    correction: LayerCorrection,
    anchor_scale: [f64; 2],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let (properties, mut warnings) =
        match super::control_links::read_layer_transform(layer, composition) {
            Ok(result) => result,
            Err(error) => {
                return (
                    Vec::new(),
                    vec![format!("Transform group: {error}; animation omitted")],
                );
            }
        };
    let clock = match animation::NumericAnimationClock::parent_identity(layer) {
        Ok(clock) => clock,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!(
                    "Transform animation: {error}; wrapper animation omitted"
                )],
            );
        }
    };
    let separated = properties
        .iter()
        .find(|property| property.match_name == "ADBE Position")
        .and_then(|property| property.numeric.as_ref().ok())
        .is_some_and(|property| property.dimensions_separated);
    let three_d = layer.record.flags().three_d_layer;
    let mut entries = Vec::new();
    for property in properties {
        let name = property.match_name.as_str();
        if (name == "ADBE Position" && separated)
            || (name.starts_with("ADBE Position_") && !separated)
        {
            continue;
        }
        let mut numeric = match property.numeric {
            Ok(numeric) => numeric,
            Err(error) => {
                warnings.push(format!("{name}: {error}; animation omitted"));
                continue;
            }
        };
        if numeric.keyframes.is_empty() {
            continue;
        }
        if let Err(error) = correct_numeric(name, &mut numeric, correction) {
            warnings.push(format!("{name}: {error}; animation omitted"));
            continue;
        }
        if three_d
            && matches!(name, "ADBE Anchor Point" | "ADBE Scale")
            && numeric.keyframes.iter().any(|key| key.values.len() >= 3)
        {
            warnings.push(format!(
                "{name}: Z component has no destination transform property; X/Y animation imported"
            ));
        }
        if name == "ADBE Orientation" {
            warnings.push("ADBE Orientation: AE interpolates authored orientations as quaternions; editable component-wise Euler tracks preserve key values but may differ between keys".into());
        }
        let (mut converted, converted_warnings) = animation::numeric_entries(
            name,
            &numeric,
            &targets(name, three_d, target_id, anchor_scale),
            clock,
            budget,
        );
        entries.append(&mut converted);
        warnings.extend(converted_warnings);
    }
    (entries, warnings)
}

fn correct_numeric(
    name: &str,
    numeric: &mut NumericProperty,
    correction: LayerCorrection,
) -> Result<(), &'static str> {
    for key in &mut numeric.keyframes {
        match name {
            "ADBE Anchor Point" => {
                if let Some(offset) = correction.anchor {
                    translate_pair(&mut key.values, offset)?;
                }
            }
            "ADBE Position" => {
                if let Some(offset) = correction.position {
                    translate_pair(&mut key.values, offset)?;
                }
            }
            "ADBE Position_0" => {
                if let Some(offset) = correction.position {
                    translate_component(&mut key.values, 0, offset[0])?;
                }
            }
            "ADBE Position_1" => {
                if let Some(offset) = correction.position {
                    translate_component(&mut key.values, 0, offset[1])?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn translate_component(
    values: &mut [f64],
    component: usize,
    delta: f64,
) -> Result<(), &'static str> {
    let Some(value) = values.get_mut(component) else {
        return Err("inverse camera-origin translation found a missing component");
    };
    *value += delta;
    if !value.is_finite() {
        return Err("inverse camera-origin translation produced a non-finite key");
    }
    Ok(())
}

fn targets(
    name: &str,
    three_d: bool,
    target_id: LayerId,
    anchor_scale: [f64; 2],
) -> Vec<animation::NumericAnimationTarget> {
    let float = |component, property| {
        animation::NumericAnimationTarget::float(
            PropertyTarget::layer(target_id, property),
            component,
            1.0,
        )
    };
    match name {
        "ADBE Anchor Point" => vec![
            animation::NumericAnimationTarget::float(
                PropertyTarget::layer(target_id, PropType::AnchorPointX),
                0,
                anchor_scale[0],
            ),
            animation::NumericAnimationTarget::float(
                PropertyTarget::layer(target_id, PropType::AnchorPointY),
                1,
                anchor_scale[1],
            ),
        ],
        "ADBE Position" => {
            let mut result = vec![float(0, PropType::PositionX), float(1, PropType::PositionY)];
            if three_d {
                result.push(float(2, PropType::PositionZ));
            }
            result
        }
        "ADBE Position_0" => vec![float(0, PropType::PositionX)],
        "ADBE Position_1" => vec![float(0, PropType::PositionY)],
        "ADBE Position_2" if three_d => vec![float(0, PropType::PositionZ)],
        super::control_links::SCALE_X => vec![animation::NumericAnimationTarget::float(
            PropertyTarget::layer(target_id, PropType::ScaleX),
            0,
            100.0,
        )],
        super::control_links::SCALE_Y => vec![animation::NumericAnimationTarget::float(
            PropertyTarget::layer(target_id, PropType::ScaleY),
            0,
            100.0,
        )],
        "ADBE Scale" => vec![
            animation::NumericAnimationTarget::float(
                PropertyTarget::layer(target_id, PropType::ScaleX),
                0,
                100.0,
            ),
            animation::NumericAnimationTarget::float(
                PropertyTarget::layer(target_id, PropType::ScaleY),
                1,
                100.0,
            ),
        ],
        "ADBE Rotate Z" => vec![float(0, PropType::Rotation)],
        "ADBE Rotate X" => vec![float(0, PropType::RotationX)],
        "ADBE Rotate Y" => vec![float(0, PropType::RotationY)],
        "ADBE Orientation" => vec![
            float(0, PropType::OrientationX),
            float(1, PropType::OrientationY),
            float(2, PropType::OrientationZ),
        ],
        "ADBE Opacity" => vec![animation::NumericAnimationTarget::float(
            PropertyTarget::layer(target_id, PropType::Opacity),
            0,
            100.0,
        )],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        structure::read_project,
        writer::{CompositionSpec, LayerSpec, NativeCameraSpec, write_composition},
    };

    #[test]
    fn recognizes_only_fresh_writer_root_camera() {
        let spec = CompositionSpec {
            name: "Camera import".into(),
            width: 640,
            height: 360,
            duration_frames: 24,
        };
        let bytes = write_composition(
            &spec,
            &[LayerSpec::Camera(NativeCameraSpec::root(640, 360))],
        )
        .unwrap();
        let mut project = read_project(&bytes).unwrap();
        let found = find(&project, 640, 360);
        let ItemKind::Composition(composition) = &project.items[0].kind else {
            panic!("composition")
        };
        let camera_id = composition.layers[0].record.id();
        assert_eq!(
            found.get(&1),
            Some(&CompositionNormalization {
                camera_layer_id: camera_id,
                offset: [0.0, 0.0],
            })
        );
        let converted = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
        let root = converted.document.composition().layers()[0].data();
        let fx_schema::LayerData::Group(root) = root else {
            panic!("root group")
        };
        assert!(
            root.layers.is_empty(),
            "canonical camera is not an FX layer"
        );

        let ItemKind::Composition(composition) = &mut project.items[0].kind else {
            panic!("composition")
        };
        composition.layers[0].name = "User camera".into();
        assert!(find(&project, 640, 360).is_empty());
        let converted = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
        assert!(converted.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("unsupported solid/media/text/shape/light/camera")
        }));
    }
}
