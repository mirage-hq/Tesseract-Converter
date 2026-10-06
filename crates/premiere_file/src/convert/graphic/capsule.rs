//! The saved-template preprocessor feeds the same ordinary text/shape objects
//! that Source Graphics import and export. No capsule content survives this seam.

use super::{
    super::{color_matte, tesseract_to_premiere},
    shape_object,
};
use crate::{
    approximate,
    error::{unsupported, Result},
    omit,
    schema::text::{PrGraphicObject, PrVectorMotion},
    Omission, OmissionScope,
};
use fx_schema::{AnimationGraph, GroupLayer, LayerData, ShapePathCommand, Transform};

pub(crate) fn template_objects(
    groups: &[GroupLayer],
    frame: [u32; 2],
    omissions: &mut Vec<Omission>,
    record: &str,
) -> Result<Vec<PrGraphicObject>> {
    let mut objects = Vec::new();
    for group in groups {
        if group.is_hidden {
            continue;
        }
        let mut stages = vec![group.transform];
        let mut parent = group.parent;
        let mut seen = std::collections::BTreeSet::from([group.id]);
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err(unsupported("cyclic template transform parenting"));
            }
            let ancestor = groups
                .iter()
                .find(|group| group.id == id)
                .ok_or_else(|| unsupported("missing template transform parent"))?;
            let mut matrix = ancestor.transform;
            // AE parenting inherits only geometry, not opacity/visibility.
            matrix.opacity = fx_schema::PercentageProperty::new(100.0).expect("100 is valid");
            stages.push(matrix);
            parent = ancestor.parent;
        }
        collect(
            &group.layers,
            &stages,
            frame,
            &mut objects,
            omissions,
            record,
        )?;
    }
    if objects.is_empty() {
        return Err(unsupported("template has no editable text/shape content"));
    }
    Ok(objects)
}

fn collect(
    layers: &[fx_schema::Layer],
    inherited: &[Transform],
    frame: [u32; 2],
    output: &mut Vec<PrGraphicObject>,
    omissions: &mut Vec<Omission>,
    record: &str,
) -> Result<()> {
    // FX and Premiere both list the topmost/front object first.
    for layer in layers {
        if let LayerData::Group(group) = layer.data() {
            if group.is_hidden {
                continue;
            }
            let mut stages = vec![group.transform];
            stages.extend_from_slice(inherited);
            collect(&group.layers, &stages, frame, output, omissions, record)?;
            continue;
        }
        let leaf_record = format!(
            "{record}: template layer {} ({:?})",
            layer.data().id(),
            layer.data().name()
        );
        let converted = (|| -> Result<Option<PrGraphicObject>> {
            let mut object = match layer.data() {
                LayerData::Text(text) => {
                    if text.is_hidden {
                        return Ok(None);
                    }
                    let font = if text.source_text.font_style.is_empty() {
                        text.source_text.font_family.to_string()
                    } else {
                        format!(
                            "{}-{}",
                            text.source_text.font_family, text.source_text.font_style
                        )
                    };
                    let mut current = text.clone();
                    if current.anchor_options.as_ref().is_some_and(|options| {
                        options.anchor_point_grouping != Default::default()
                            || options.grouping_alignment != [0.0, 0.0]
                    }) {
                        approximate(omissions,&leaf_record,"nondefault Text anchor grouping approximated by ordinary point/box text");
                    }
                    if current.path_options.is_some() {
                        approximate(omissions,&leaf_record,"Text Path Options omitted; current text becomes ordinary point/box text");
                    }
                    current.anchor_options = None;
                    current.path_options = None;
                    current.track_matte = None;
                    if super::stroke_width_animator(&current).is_err() {
                        approximate(
                            omissions,
                            &leaf_record,
                            "unsupported Text animator controls omitted; current editable text, font and paint retained",
                        );
                        current.animators.clear();
                    }
                    let document = &mut current.source_text;
                    for (present, field) in [
                        (document.font_variations.is_some(), "font variations"),
                        (document.baseline_shift != 0.0, "baseline shift"),
                        (document.underline, "underline"),
                        (document.strikethrough, "strikethrough"),
                        (document.box_first_baseline.is_some(), "box first baseline"),
                    ] {
                        if present {
                            approximate(
                                omissions,
                                &leaf_record,
                                format!("Text {field} omitted; current editable text and supported style retained"),
                            );
                        }
                    }
                    document.font_variations = None;
                    document.baseline_shift = 0.0;
                    document.underline = false;
                    document.strikethrough = false;
                    document.box_first_baseline = None;
                    if document.box_text
                        && document.leading.is_some()
                        && document.vertical_align != Some(fx_schema::VerticalAlign::Top)
                    {
                        approximate(
                            omissions,
                            &leaf_record,
                            "Text box with explicit leading approximated by top alignment; editable text, box and leading retained",
                        );
                        document.vertical_align = Some(fx_schema::VerticalAlign::Top);
                    }
                    if current.source_text.stroke_over_fill {
                        approximate(omissions,record,"AE Text stroke-over-fill is approximated with ordinary graphic stroke-under-fill; editable colours/width retained");
                        current.source_text.stroke_over_fill = false;
                    }
                    PrGraphicObject::Text(tesseract_to_premiere::text_object(
                        &current,
                        current.parent,
                        font,
                        &AnimationGraph::default(),
                        omissions,
                        record,
                    )?)
                }
                LayerData::Rect(rect) => {
                    if rect.is_hidden {
                        return Ok(None);
                    }
                    shape_object(&color_matte::rect_as_shape(rect), None)?.object
                }
                LayerData::Shape(shape) => {
                    if shape.is_hidden {
                        return Ok(None);
                    }
                    let mut current = shape.clone();
                    // A native static Rectangle fallback can store rounded vertices;
                    // ordinary graphics need explicit cubics, using the existing Rect
                    // outline owner rather than inventing a second geometry pipeline.
                    let commands = &current.shape.path.commands;
                    if commands
                        .iter()
                        .any(|command| command.corner_radius().is_some_and(|radius| radius > 0.0))
                    {
                        let points: Vec<_> = commands
                            .iter()
                            .filter_map(|command| match command {
                                ShapePathCommand::MoveTo { x, y, .. }
                                | ShapePathCommand::LineTo { x, y, .. } => Some([*x, *y]),
                                _ => None,
                            })
                            .collect();
                        let radius = commands[0].corner_radius().unwrap_or(0.0);
                        let left = points
                            .iter()
                            .map(|point| point[0])
                            .fold(f64::INFINITY, f64::min);
                        let right = points
                            .iter()
                            .map(|point| point[0])
                            .fold(f64::NEG_INFINITY, f64::max);
                        let top = points
                            .iter()
                            .map(|point| point[1])
                            .fold(f64::INFINITY, f64::min);
                        let bottom = points
                            .iter()
                            .map(|point| point[1])
                            .fold(f64::NEG_INFINITY, f64::max);
                        if points.len() == 4
                            && commands.len() == 5
                            && commands.last() == Some(&ShapePathCommand::Close)
                            && points.iter().all(|point| {
                                (point[0] == left || point[0] == right)
                                    && (point[1] == top || point[1] == bottom)
                            })
                            && commands[..4]
                                .iter()
                                .all(|command| command.corner_radius() == Some(radius))
                        {
                            current.shape.path = color_matte::rounded_rect_outline(
                                [left, top],
                                [right - left, bottom - top],
                                radius,
                            );
                        } else {
                            return Err(unsupported(
                                "template rounded path is not a single Rectangle",
                            ));
                        }
                    }
                    shape_object(&current, None)?.object
                }
                _ => {
                    approximate(
                        omissions,
                        record,
                        "unsupported template leaf omitted; editable Text/Shape siblings retained",
                    );
                    return Ok(None);
                }
            };
            for stage in inherited {
                if stage.skew != 0.0
                    || stage.skew_axis != 0.0
                    || stage.rotation_x != 0.0
                    || stage.rotation_y != 0.0
                    || stage.orientation != [0.0; 3]
                    || stage.position.z().is_some_and(|z| z != 0.0)
                {
                    approximate(omissions,&leaf_record,"template skew/3D transform fields omitted; editable 2D position, anchor, vertical scale, Z rotation and opacity retained");
                }
                if stage.scale[0] != stage.scale[1]
                    && (stage.scale[0] - stage.scale[1]).abs() > 1e-9
                {
                    approximate(
                        omissions,
                        record,
                        "nonuniform template group scale approximated by its vertical scale",
                    );
                }
                let motion = PrVectorMotion {
                    position: [stage.position.x(), stage.position.y()],
                    anchor: stage.anchor_point,
                    scale: stage.scale[1],
                    rotation: stage.rotation,
                    animations: Vec::new(),
                };
                match &mut object {
                    PrGraphicObject::Text(text) => {
                        text.compose_static_vector_motion(&motion, frame);
                        text.transform.opacity *= stage.opacity.value() / 100.0;
                    }
                    PrGraphicObject::Shape(shape) => {
                        shape.compose_static_vector_motion(&motion);
                        shape.transform.opacity *= stage.opacity.value() / 100.0;
                    }
                    _ => {
                        return Err(unsupported(
                            "template primitive produced nonprimitive objects",
                        ))
                    }
                }
            }
            object.validate()?;
            Ok(Some(object))
        })();
        match converted {
            Ok(Some(object)) => output.push(object),
            Ok(None) => {}
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                &leaf_record,
                format!("template object omitted: {error}; editable siblings retained"),
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_schema::{Layer, LayerId, PositiveProperty};

    fn text_group() -> GroupLayer {
        let text: fx_schema::TextLayer = serde_json::from_value(serde_json::json!({
            "id":1,"name":"sibling","activeRange":{"start":0,"duration":1000},
            "transform":crate::convert::background::identity_transform(),
            "sourceText":{"text":"convertible sibling","fontFamily":"ArialMT","fontStyle":"",
                "fontSize":24.0,"fillColor":[0.0,0.0,0.0,1.0]}
        }))
        .unwrap();
        crate::convert::background::plain_group(
            LayerId::new(2),
            "synthetic".into(),
            text.active_range,
            crate::convert::background::identity_transform(),
            vec![Layer::from_data(&LayerData::Text(text)).unwrap()],
        )
        .unwrap()
    }
    #[test]
    fn capsule_unsupported_text_animator_retains_current_text_and_style() {
        let mut group = text_group();
        let LayerData::Text(text) = group.layers[0].data() else {
            panic!()
        };
        let mut text = text.clone();
        text.animators.push(fx_schema::TextAnimator {
            id: fx_schema::FxItemId::new(90008),
            opacity: Some(50.0),
            ..Default::default()
        });
        group.layers = vec![Layer::from_data(&LayerData::Text(text)).unwrap()];
        let mut reports = Vec::new();
        let objects = template_objects(&[group], [1080, 500], &mut reports, "animator").unwrap();
        let [PrGraphicObject::Text(text)] = objects.as_slice() else {
            panic!("{objects:?}")
        };
        assert_eq!(text.document.text, "convertible sibling");
        assert_eq!(text.document.font, "ArialMT");
        assert_eq!(text.document.size, 24.0);
        assert!(text.document.fill.is_some());
        assert!(reports
            .iter()
            .any(|report| report.reason.contains("animator controls omitted")));
        assert!(!reports
            .iter()
            .any(|report| report.reason.contains("object omitted")));
    }

    #[test]
    fn capsule_optional_text_style_fields_retain_supported_document() {
        let mut group = text_group();
        let LayerData::Text(text) = group.layers[0].data() else {
            panic!()
        };
        let mut text = text.clone();
        text.source_text.baseline_shift = 5.0;
        text.source_text.underline = true;
        text.source_text.strikethrough = true;
        group.layers = vec![Layer::from_data(&LayerData::Text(text)).unwrap()];
        let mut reports = Vec::new();
        let objects = template_objects(&[group], [1080, 500], &mut reports, "style").unwrap();
        let [PrGraphicObject::Text(text)] = objects.as_slice() else {
            panic!("{objects:?}")
        };
        assert_eq!(text.document.text, "convertible sibling");
        assert_eq!(text.document.size, 24.0);
        for field in ["baseline shift", "underline", "strikethrough"] {
            assert!(
                reports.iter().any(|report| report.reason.contains(field)),
                "{reports:?}"
            );
        }
    }

    #[test]
    fn capsule_supported_static_width_animator_keeps_ordinary_mapping() {
        let mut group = text_group();
        let LayerData::Text(text) = group.layers[0].data() else {
            panic!()
        };
        let mut text = text.clone();
        text.source_text.apply_stroke = true;
        text.source_text.stroke_color = Some([1.0, 0.0, 0.0, 1.0]);
        text.animators.push(fx_schema::TextAnimator {
            id: fx_schema::FxItemId::new(90008),
            stroke_width: Some(2.0),
            ..Default::default()
        });
        let expected = tesseract_to_premiere::text_object(
            &text,
            text.parent,
            "ArialMT".into(),
            &AnimationGraph::default(),
            &mut Vec::new(),
            "width",
        )
        .unwrap();
        group.layers = vec![Layer::from_data(&LayerData::Text(text)).unwrap()];
        let mut reports = Vec::new();
        let objects = template_objects(&[group], [1080, 500], &mut reports, "width").unwrap();
        let [PrGraphicObject::Text(text)] = objects.as_slice() else {
            panic!("{objects:?}")
        };
        assert_eq!(text.document.stroke, expected.document.stroke);
        assert!(text.document.stroke.is_some());
        assert!(!reports
            .iter()
            .any(|report| report.reason.contains("animator controls omitted")));
    }

    #[test]
    fn review_capsule_children_keep_topmost_first_order() {
        let mut group = text_group();
        let LayerData::Text(text) = group.layers[0].data() else {
            panic!()
        };
        let mut front = text.clone();
        front.source_text.text = "front".into();
        let mut back = text.clone();
        back.id = LayerId::new(90001);
        back.source_text.text = "back".into();
        let rect: fx_schema::RectLayer = serde_json::from_value(serde_json::json!({
            "id":90007,"name":"middle shape","activeRange":{"start":0,"duration":1000},
            "transform":crate::convert::background::identity_transform(),
            "rect":crate::convert::background::black_shape(40,20)
        }))
        .unwrap();
        group.layers = vec![
            Layer::from_data(&LayerData::Text(front)).unwrap(),
            Layer::from_data(&LayerData::Rect(rect)).unwrap(),
            Layer::from_data(&LayerData::Text(back)).unwrap(),
        ];
        let mut nested = group.clone();
        nested.id = LayerId::new(90002);
        nested.parent = Some(group.id);
        group.layers = vec![Layer::from_data(&LayerData::Group(nested)).unwrap()];
        let objects = template_objects(&[group], [1080, 500], &mut Vec::new(), "order").unwrap();
        let names: Vec<_> = objects
            .iter()
            .map(|object| match object {
                PrGraphicObject::Text(text) => text.document.text.as_str(),
                PrGraphicObject::Shape(_) => "shape",
                _ => panic!(),
            })
            .collect();
        assert_eq!(names, ["front", "shape", "back"]);
    }
    #[test]
    fn review_capsule_unrepresentable_leaf_keeps_text_sibling() {
        let mut group = text_group();
        let LayerData::Text(text) = group.layers[0].data() else {
            panic!()
        };
        let mut unsupported = text.clone();
        unsupported.id = LayerId::new(90003);
        unsupported.name = "oversize".into();
        unsupported.source_text.font_size = PositiveProperty::new(1e100).unwrap();
        group
            .layers
            .push(Layer::from_data(&LayerData::Text(unsupported)).unwrap());
        let mut reports = Vec::new();
        let objects = template_objects(&[group], [1080, 500], &mut reports, "siblings").unwrap();
        assert_eq!(objects.len(), 1);
        assert!(
            matches!(&objects[0],PrGraphicObject::Text(text) if text.document.text=="convertible sibling")
        );
        assert!(
            reports
                .iter()
                .any(|report| report.record.contains("oversize")
                    && report.reason.contains("omitted")),
            "{reports:?}"
        );
    }
    #[test]
    fn review_capsule_path_anchor_and_3d_reductions_are_reported() {
        let mut group = text_group();
        let LayerData::Text(text) = group.layers[0].data() else {
            panic!()
        };
        let mut text = text.clone();
        text.anchor_options = Some(fx_schema::TextAnchorOptions {
            grouping_alignment: [25.0, 0.0],
            ..Default::default()
        });
        text.path_options = Some(
            serde_json::from_value(serde_json::json!({"id":90005,"pathLayer":90006})).unwrap(),
        );
        group.layers = vec![Layer::from_data(&LayerData::Text(text)).unwrap()];
        group.transform.skew = 20.0;
        group.transform.rotation_x = 10.0;
        group.transform.position =
            serde_json::from_value(serde_json::json!([40.0, 55.0, 9.0])).unwrap();
        let mut reports = Vec::new();
        assert_eq!(
            template_objects(&[group], [1080, 500], &mut reports, "options")
                .unwrap()
                .len(),
            1
        );
        for reason in ["anchor grouping", "Path Options", "skew/3D"] {
            assert!(
                reports.iter().any(|report| report.reason.contains(reason)),
                "missing {reason}: {reports:?}"
            );
        }
    }
}
