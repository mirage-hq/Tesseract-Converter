//! Preserve vector paint owners when the native shared Color ease is unsupported.

use super::{ExportDiagnostic, KeyframeEasing, LayerId, LayerSpec, VectorContent, VectorPaintSpec};

pub(super) fn retain_supported_tracks(
    layers: &mut [LayerSpec],
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    for layer in layers {
        normalize_layer(layer, None, diagnostics);
    }
}

fn normalize_layer(
    layer: &mut LayerSpec,
    owner: Option<LayerId>,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    match layer {
        LayerSpec::Options(inner, options) => {
            normalize_layer(inner, Some(options.fx_id), diagnostics);
        }
        LayerSpec::Timed(inner, _) => normalize_layer(inner, owner, diagnostics),
        LayerSpec::Precomposition(spec) => retain_supported_tracks(&mut spec.layers, diagnostics),
        LayerSpec::VectorProgram(spec) => {
            normalize_contents(&mut spec.contents, owner, diagnostics)
        }
        LayerSpec::Solid(_)
        | LayerSpec::AnimatedSolid(_, _)
        | LayerSpec::Rect(_)
        | LayerSpec::AnimatedRect(_, _)
        | LayerSpec::Shape(_)
        | LayerSpec::Boolean(_)
        | LayerSpec::Footage(_, _)
        | LayerSpec::Text(_)
        | LayerSpec::Null(_)
        | LayerSpec::Camera(_) => {}
    }
}

fn normalize_contents(
    contents: &mut [VectorContent],
    owner: Option<LayerId>,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    for content in contents {
        match content {
            VectorContent::Group(group) | VectorContent::AnimatedGroup(group, _) => {
                normalize_contents(&mut group.contents, owner, diagnostics);
            }
            VectorContent::Paint(paint) => {
                let (kind, animations) = match paint {
                    VectorPaintSpec::Fill { animations, .. } => ("Fill", animations),
                    VectorPaintSpec::Stroke { animations, .. } => ("Stroke", animations),
                };
                if animations.color.as_ref().is_some_and(|track| {
                    track.keys.iter().any(|key| {
                        key.easing
                            .iter()
                            .any(|ease| matches!(ease, KeyframeEasing::CubicBezier { .. }))
                    })
                }) {
                    animations.color = None;
                    diagnostics.push(ExportDiagnostic {
                        layer_id: owner,
                        message: format!("Cubic paint color animation omitted for {kind}: native shared Color easing is not established; authored paint base, owner and other supported tracks retained."),
                    });
                }
            }
            VectorContent::Geometry { .. }
            | VectorContent::Modifier(_)
            | VectorContent::Merge(_) => {}
        }
    }
}
