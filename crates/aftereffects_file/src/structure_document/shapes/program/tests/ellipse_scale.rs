use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::to_structural_fx_document;
use fx_schema::{Layer, LayerData, PropType, PropertyTarget, PropertyValue, ShapeLayer};

fn paint(layers: &[Layer]) -> Option<&ShapeLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Shape(shape) if !shape.is_hidden && !shape.shape.strokes.is_empty() => {
            Some(shape)
        }
        LayerData::Group(group) => paint(&group.layers),
        _ => None,
    })
}

fn set_speeds(chunks: &mut [Chunk], outgoing: f64) {
    for chunk in chunks {
        if chunk.id() == *b"ldat" {
            let mut data = chunk.data_payload().unwrap().to_vec();
            // Native vector influences are fractions, not UI percentages.
            for key in 0..2 {
                for offset in [40, 48, 72, 80] {
                    let start = key * 88 + offset;
                    data[start..start + 8].copy_from_slice(&(1.0_f64 / 3.0).to_be_bytes());
                }
            }
            for axis in 0..2 {
                data[56 + axis * 8..64 + axis * 8].copy_from_slice(&outgoing.to_be_bytes());
                data[112 + axis * 8..120 + axis * 8].copy_from_slice(&720.0_f64.to_be_bytes());
            }
            *chunk = Chunk::data(*b"ldat", data).unwrap();
        } else if let Some(children) = chunk.children_mut() {
            set_speeds(children, outgoing);
        }
    }
}

#[test]
fn imported_outer_ellipse_stroke_accepts_positive_scale_with_negative_ease_handles() {
    for outgoing in [-120.0, -900.0] {
        // Supplemental controls on an existing native Shape owner, not new Adobe fidelity evidence.
        let mut project = read_project(include_bytes!(
            "../../../../../tests/fixtures/pr4442_native/sources/vector_transform_scale.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!("native composition")
        };
        comp.layers.retain(|layer| layer.record.id() == 16);
        let mut scale = keyed_pair(
            "ADBE Vector Scale",
            &[(0, [100.0, 100.0]), (1_000, [300.0, 300.0])],
        );
        set_speeds(&mut scale, outgoing);
        let contents = entries(vec![
            entry(
                "ADBE Vector Group",
                entries(vec![
                    entry(
                        "ADBE Vectors Group",
                        entry(
                            "ADBE Vector Shape - Ellipse",
                            number("ADBE Vector Ellipse Size", &[100.0, 80.0]),
                        ),
                    ),
                    entry("ADBE Vector Transform Group", scale),
                ]),
            ),
            entry(
                "ADBE Vector Graphic - Stroke",
                entries(vec![
                    number("ADBE Vector Stroke Width", &[10.0]),
                    entry(
                        "ADBE Vector Stroke Dashes",
                        entries(vec![
                            number("ADBE Vector Stroke Dash 1", &[20.0]),
                            number("ADBE Vector Stroke Gap 1", &[12.0]),
                        ]),
                    ),
                ]),
            ),
        ]);
        let root = comp.layers[0]
            .content
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap();
        *root = Chunk::list(*b"tdgp", entry("ADBE Root Vectors Group", contents));
        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        let paint =
            paint(converted.document.composition().layers()).expect("editable outer stroke");
        let entries = converted.document.composition().dynamics().entries();
        let size = entries
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(paint.id, PropType::EllipseSize))
            .unwrap();
        assert_eq!(paint.shape.strokes[0].width.value(), 10.0);
        assert_eq!(
            paint.shape.strokes[0]
                .dashes
                .iter()
                .map(|v| v.value())
                .collect::<Vec<_>>(),
            [20.0, 12.0]
        );
        if outgoing == -120.0 {
            assert_eq!(paint.transform.scale, [100.0, 100.0]);
            let track = size
                .animator
                .keyframe_track()
                .expect("Size must carry the scale keys");
            assert_eq!(track.keyframes().len(), 2);
            assert_eq!(
                track.keyframes()[0].value(),
                &PropertyValue::Vector2([100.0, 80.0])
            );
            assert_eq!(
                track.keyframes()[1].value(),
                &PropertyValue::Vector2([300.0, 240.0])
            );
            assert_eq!(track.keyframes()[1].layer_time().as_millis(), 1_000);
            let fx_schema::animator::PropertyKeyframeEasing::CubicBezier { y1, y2, .. } =
                track.keyframes()[1].easing()
            else {
                panic!("native speed easing")
            };
            assert!((y1 + 0.2).abs() < 1e-6 && (y2 + 0.2).abs() < 1e-6);
            assert!(
                !entries
                    .iter()
                    .any(|entry| entry.target.layer_id() == Some(paint.id)
                        && matches!(
                            entry.target.as_property().unwrap().property_type(),
                            PropType::ScaleX | PropType::ScaleY
                        ))
            );
        } else {
            assert!(
                size.animator.keyframe_track().is_none(),
                "possibly negative scale must keep the diagnosed fallback"
            );
        }
        assert!(entries.iter().all(|entry| !entry.animator.is_js_script()));
    }
}
