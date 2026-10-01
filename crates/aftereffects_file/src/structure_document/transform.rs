//! Static 2D Transform conversion. Dynamic values are never sampled implicitly.

use std::collections::HashMap;

use fx_schema::{PercentageProperty, Position, Transform};

use crate::{
    properties::NumericProperty,
    structure::{Composition, Layer, ProjectItem},
};

pub(super) fn static_transform(
    layer: &Layer,
    size: [u16; 2],
    composition: &Composition,
) -> (Transform, Vec<String>) {
    static_transform_inner(layer, size, composition, None)
}

pub(super) fn static_transform_with_sources(
    layer: &Layer,
    size: [u16; 2],
    composition_id: u32,
    composition: &Composition,
    items: &HashMap<u32, &ProjectItem>,
) -> (Transform, Vec<String>) {
    static_transform_inner(layer, size, composition, Some((composition_id, items)))
}

fn static_transform_inner(
    layer: &Layer,
    size: [u16; 2],
    composition: &Composition,
    sources: Option<(u32, &HashMap<u32, &ProjectItem>)>,
) -> (Transform, Vec<String>) {
    let canvas = [composition.width, composition.height];
    let three_d = layer.record.flags().three_d_layer;
    let mut result = Transform {
        anchor_point: [f64::from(size[0]) / 2.0, f64::from(size[1]) / 2.0],
        position: if three_d {
            Position::ThreeD([f64::from(canvas[0]) / 2.0, f64::from(canvas[1]) / 2.0, 0.0])
        } else {
            Position::TwoD([f64::from(canvas[0]) / 2.0, f64::from(canvas[1]) / 2.0])
        },
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0; 3],
        opacity: PercentageProperty::new(if layer.record.flags().null_layer {
            0.0
        } else {
            100.0
        })
        .expect("native default opacity is valid"),
    };
    let mut warnings = Vec::new();
    let properties = match sources.map_or_else(
        || super::control_links::read_layer_transform(layer, composition),
        |(composition_id, items)| {
            super::control_links::read_layer_transform_with_sources(
                layer,
                composition_id,
                composition,
                items,
            )
        },
    ) {
        Ok((properties, messages)) => {
            warnings.extend(messages);
            properties
        }
        Err(error) => {
            warnings.push(format!("Transform group: {error}; source-size anchor, comp-center position, 100% scale, native default opacity and zero rotation used"));
            return (result, warnings);
        }
    };
    let anchor_scale = sources.map_or([1.0; 2], |(_, items)| {
        let source = items.get(&layer.record.source_id()).copied();
        super::solid_anchor_scale(source, super::source_anchor_dimensions(source, layer))
    });
    let separated = properties
        .iter()
        .find(|p| p.match_name == "ADBE Position")
        .and_then(|p| p.numeric.as_ref().ok())
        .is_some_and(|p| p.dimensions_separated);
    for name in [
        "ADBE Anchor Point",
        "ADBE Position",
        "ADBE Scale",
        super::control_links::SCALE_X,
        super::control_links::SCALE_Y,
        "ADBE Rotate Z",
        "ADBE Rotate X",
        "ADBE Rotate Y",
        "ADBE Orientation",
        "ADBE Opacity",
        "ADBE Position_0",
        "ADBE Position_1",
        "ADBE Position_2",
    ] {
        if (name.starts_with("ADBE Position_") && !separated)
            || (name == "ADBE Position_2" && !three_d)
        {
            continue;
        }
        if name == "ADBE Position" && separated {
            continue;
        }
        let Some(property) = properties.iter().find(|p| p.match_name == name) else {
            if matches!(
                name,
                super::control_links::SCALE_X | super::control_links::SCALE_Y
            ) {
                continue;
            }
            warnings.push(format!("{name}: absent; source-size anchor/comp-center position or identity component default used"));
            continue;
        };
        let value = match &property.numeric {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!("{name}: {error}; default component used"));
                continue;
            }
        };
        if value.expression_enabled {
            if !value.animated
                && !value.values.is_empty()
                && matches!(
                    name,
                    "ADBE Position" | "ADBE Position_0" | "ADBE Position_1" | "ADBE Position_2"
                )
            {
                warnings.push(format!(
                    "{name}: AE expression not evaluated; stored pre-expression position retained as an editable approximation unless matching captured samples override it"
                ));
            } else {
                warnings.push(format!(
                    "{name}: enabled AE expression requires matching captured AE samples; static default component retained"
                ));
                continue;
            }
        }
        if value.animated {
            if value.keyframes.is_empty() {
                warnings.push(format!(
                    "{name}: animated property has no supported native keys; default component used"
                ));
            } else {
                warnings.push(format!(
                    "{name}: editable native keyframes imported; static base remains the identity/default value and the animation graph supplies authored values"
                ));
            }
            continue;
        }
        if value.expression_present && !value.expression_enabled {
            warnings.push(format!(
                "{name}: disabled expression omitted; stored static value used"
            ));
        }
        if three_d && matches!(name, "ADBE Anchor Point" | "ADBE Scale") && value.values.len() >= 3
        {
            warnings.push(format!(
                "{name}: Z component has no destination transform field; X/Y value imported"
            ));
        }
        let applied = match name {
            "ADBE Anchor Point" => pair(value).and_then(|v| {
                let anchor = [v[0] * anchor_scale[0], v[1] * anchor_scale[1]];
                anchor
                    .iter()
                    .all(|v| v.is_finite())
                    .then(|| result.anchor_point = anchor)
            }),
            "ADBE Position" if three_d => {
                triple(value).map(|v| result.position = Position::ThreeD(v))
            }
            "ADBE Position" => pair(value).map(|v| result.position = Position::TwoD(v)),
            "ADBE Scale" => pair(value).and_then(|v| {
                let v = v.map(|v| v * 100.0);
                v.iter().all(|v| v.is_finite()).then(|| result.scale = v)
            }),
            super::control_links::SCALE_X | super::control_links::SCALE_Y => scalar(value)
                .and_then(|v| {
                    let percent = v * 100.0;
                    percent.is_finite().then(|| {
                        result.scale[usize::from(name == super::control_links::SCALE_Y)] = percent
                    })
                }),
            "ADBE Rotate Z" => scalar(value).map(|v| result.rotation = v),
            "ADBE Rotate X" => scalar(value).map(|v| result.rotation_x = v),
            "ADBE Rotate Y" => scalar(value).map(|v| result.rotation_y = v),
            "ADBE Orientation" => triple(value).map(|v| result.orientation = v),
            "ADBE Opacity" => scalar(value)
                .and_then(|v| PercentageProperty::new(v * 100.0))
                .map(|v| result.opacity = v),
            "ADBE Position_0" | "ADBE Position_1" | "ADBE Position_2" => {
                scalar(value).and_then(|v| {
                    let component = name
                        .as_bytes()
                        .last()
                        .and_then(|digit| digit.checked_sub(b'0'))
                        .map(usize::from)?;
                    match &mut result.position {
                        Position::TwoD(xy) if component < xy.len() => xy[component] = v,
                        Position::ThreeD(xyz) if component < xyz.len() => xyz[component] = v,
                        _ => return None,
                    }
                    Some(())
                })
            }
            _ => None,
        };
        if applied.is_none() {
            warnings.push(format!(
                "{name}: unrepresentable dimensions/value; default component used"
            ));
        }
    }
    (result, warnings)
}

pub(super) fn solid_rect(
    source: &crate::structure::SolidSource,
    occurrence: &fx_schema::GroupLayer,
    id: fx_schema::LayerId,
    transform: Transform,
) -> fx_schema::layer::RectLayer {
    fx_schema::layer::RectLayer {
        id,
        name: occurrence.name.clone(),
        description: "Editable AE solid; static source fill and Transform, no flattened media"
            .into(),
        is_hidden: false,
        parent: Some(occurrence.id),
        blend_mode: Default::default(),
        track_matte: None,
        masks: Vec::new(),
        effects: Vec::new(),
        motion_blur: false,
        // The occurrence owns trimming/stretch; static content must remain visible
        // throughout its supported source clock, not just the parent comp duration.
        active_range: fx_schema::TimeRangeProperty::new(
            fx_schema::Time::ZERO,
            fx_schema::Duration::from_secs(super::MAX_TIME_SECS),
        ),
        transform,
        rect: fx_schema::RectShape {
            size: [f64::from(source.width), f64::from(source.height)],
            // RectShape.position is the top-left of its content box in FX;
            // the AE anchor is also measured from the source top-left.
            position: [0.0, 0.0],
            roundness: 0.0,
            fill_enabled: true,
            fill_color: [
                f64::from(source.color[0]),
                f64::from(source.color[1]),
                f64::from(source.color[2]),
                1.0,
            ],
            fill_paint: None,
            fill_blend_mode: None,
            stroke_enabled: false,
            stroke_color: None,
            stroke_width: Default::default(),
            stroke_dashes: Vec::new(),
            stroke_dash_offset: 0.0,
            stroke_join: Default::default(),
            stroke_miter_limit: 4.0,
        },
    }
}

fn pair(value: &NumericProperty) -> Option<[f64; 2]> {
    match value.values.as_slice() {
        [x, y] | [x, y, _] => Some([*x, *y]),
        _ => None,
    }
}
fn triple(value: &NumericProperty) -> Option<[f64; 3]> {
    match value.values.as_slice() {
        [x, y, z] => Some([*x, *y, *z]),
        _ => None,
    }
}

fn scalar(value: &NumericProperty) -> Option<f64> {
    match value.values.as_slice() {
        [v] => Some(*v),
        _ => None,
    }
}
