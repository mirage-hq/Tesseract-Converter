//! Preserve vector paint colors when native shared Color easing is unsupported.

use super::{ExportDiagnostic, KeyframeEasing, LayerId, LayerSpec, VectorContent, VectorPaintSpec};

pub(super) fn retain_supported_tracks(
    layers: &mut [LayerSpec],
    diagnostics: &mut Vec<ExportDiagnostic>,
    preserve_extended: bool,
) {
    for layer in layers {
        normalize_layer(layer, None, diagnostics, preserve_extended);
    }
}

fn normalize_layer(
    layer: &mut LayerSpec,
    owner: Option<LayerId>,
    diagnostics: &mut Vec<ExportDiagnostic>,
    preserve_extended: bool,
) {
    match layer {
        LayerSpec::Options(inner, options) => {
            normalize_layer(inner, Some(options.fx_id), diagnostics, preserve_extended);
        }
        LayerSpec::Timed(inner, _) => normalize_layer(inner, owner, diagnostics, preserve_extended),
        LayerSpec::Precomposition(spec) => {
            retain_supported_tracks(&mut spec.layers, diagnostics, preserve_extended)
        }
        LayerSpec::VectorProgram(spec) => {
            normalize_contents(&mut spec.contents, owner, diagnostics, preserve_extended)
        }
        LayerSpec::Solid(spec) | LayerSpec::AnimatedSolid(spec, _) => {
            if !preserve_extended && spec.color.iter().any(|channel| *channel > 1.0) {
                spec.color = spec.color.map(|channel| channel.min(1.0));
                warn_clipped(owner, "Solid", diagnostics);
            }
        }
        LayerSpec::Rect(spec) | LayerSpec::AnimatedRect(spec, _) if !preserve_extended => {
            clip_optional_colors(
                &mut spec.fill_color,
                &mut spec.stroke_color,
                owner,
                diagnostics,
            );
        }
        LayerSpec::Shape(spec) if !preserve_extended => {
            clip_optional_colors(
                &mut spec.appearance.fill_color,
                &mut spec.appearance.stroke_color,
                owner,
                diagnostics,
            );
        }
        LayerSpec::Boolean(spec) if !preserve_extended => {
            clip_optional_colors(
                &mut spec.appearance.fill_color,
                &mut spec.appearance.stroke_color,
                owner,
                diagnostics,
            );
        }
        LayerSpec::Text(spec) if !preserve_extended => {
            for key in &mut spec.documents.keys {
                let mut clipped = clip_rgb(&mut key.document.fill_color);
                if let Some(color) = &mut key.document.stroke_color {
                    clipped |= clip_rgb(color);
                }
                if clipped {
                    warn_clipped(
                        owner,
                        &format!("Source Text at {}ms", key.time_millis),
                        diagnostics,
                    );
                }
            }
        }
        LayerSpec::Footage(_, _)
        | LayerSpec::Null(_)
        | LayerSpec::Camera(_)
        | LayerSpec::Rect(_)
        | LayerSpec::AnimatedRect(_, _)
        | LayerSpec::Shape(_)
        | LayerSpec::Boolean(_)
        | LayerSpec::Text(_) => {}
    }
}

fn warn_clipped(owner: Option<LayerId>, kind: &str, diagnostics: &mut Vec<ExportDiagnostic>) {
    diagnostics.push(ExportDiagnostic {
        layer_id: owner,
        message: format!("Extended-sRGB {kind} RGB above 1 was clamped to native 0..=1; HDR brightness and channel ratios are not retained. Alpha, editable paint and authored key timing are retained."),
    });
}

fn clip_rgb(color: &mut [f64; 4]) -> bool {
    let clipped = color[..3].iter().any(|channel| *channel > 1.0);
    for channel in &mut color[..3] {
        *channel = channel.min(1.0);
    }
    clipped
}

fn clip_optional_colors(
    fill: &mut Option<[f64; 4]>,
    stroke: &mut Option<[f64; 4]>,
    owner: Option<LayerId>,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    for (kind, color) in [("Fill", fill), ("Stroke", stroke)] {
        if color.as_mut().is_some_and(clip_rgb) {
            warn_clipped(owner, kind, diagnostics);
        }
    }
}

fn clip_paint(paint: &mut fx_schema::ShapePaint) -> bool {
    match paint {
        fx_schema::ShapePaint::Solid { color } => clip_rgb(color),
        fx_schema::ShapePaint::Gradient { stops, .. } => {
            let mut clipped = false;
            for stop in stops {
                clipped |= clip_rgb(&mut stop.color);
            }
            clipped
        }
    }
}

fn normalize_contents(
    contents: &mut [VectorContent],
    owner: Option<LayerId>,
    diagnostics: &mut Vec<ExportDiagnostic>,
    preserve_extended: bool,
) {
    for content in contents {
        match content {
            VectorContent::Group(group) | VectorContent::AnimatedGroup(group, _) => {
                normalize_contents(&mut group.contents, owner, diagnostics, preserve_extended);
            }
            VectorContent::Paint(paint) => {
                let (kind, paint, animations) = match paint {
                    VectorPaintSpec::Fill {
                        paint, animations, ..
                    } => ("Fill", paint, animations),
                    VectorPaintSpec::Stroke {
                        paint, animations, ..
                    } => ("Stroke", paint, animations),
                };
                let mut clipped = !preserve_extended && clip_paint(paint);
                if let Some(track) = &mut animations.color {
                    for key in &mut track.keys {
                        for channel in key.values.iter_mut().take(3) {
                            if !preserve_extended && *channel > 1.0 {
                                *channel = 1.0;
                                clipped = true;
                            }
                        }
                    }
                }
                if clipped {
                    warn_clipped(owner, kind, diagnostics);
                }
                let approximated = animations.color.as_mut().is_some_and(|track| {
                    let mut approximated = false;
                    for key in &mut track.keys {
                        for easing in &mut key.easing {
                            if matches!(easing, KeyframeEasing::CubicBezier { .. }) {
                                *easing = KeyframeEasing::Linear;
                                approximated = true;
                            }
                        }
                    }
                    approximated
                });
                if approximated {
                    diagnostics.push(ExportDiagnostic {
                        layer_id: owner,
                        message: format!("Cubic paint color easing approximated as Linear for {kind}: authored color keys, times, owner and other supported tracks retained."),
                    });
                }
            }
            VectorContent::Geometry { .. }
            | VectorContent::Modifier(_)
            | VectorContent::Merge(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::clip_paint;

    #[test]
    fn extended_native_gradient_clips_rgb_and_preserves_stop_alpha() {
        let mut paint: fx_schema::ShapePaint = serde_json::from_value(serde_json::json!({
            "type":"gradient", "gradientType":"linear", "start":[0,0], "end":[64,0],
            "stops":[{"offset":0,"color":[2,0.5,0.25,0.25]}, {"offset":1,"color":[64,2,1,0.5]}]
        }))
        .unwrap();
        assert!(clip_paint(&mut paint));
        let fx_schema::ShapePaint::Gradient { stops, .. } = paint else {
            panic!("gradient")
        };
        assert_eq!(stops[0].color, [1.0, 0.5, 0.25, 0.25]);
        assert_eq!(stops[1].color, [1.0, 1.0, 1.0, 0.5]);
    }
}
