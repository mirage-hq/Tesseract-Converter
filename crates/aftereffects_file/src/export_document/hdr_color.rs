//! Explicit generated-native colour bindings; never walks persisted/opaque FX JSON.

use fx_conv::HdrColorBridge;
use fx_schema::{LayerId, Linkable, ShapePaint};

use super::{ExportDiagnostic, LayerSpec, VectorContent, VectorPaintSpec};
use crate::{
    layer_styles::NativeLayerStyle,
    writer::{NumericTrack, ValueKind},
};

pub(super) fn bind(
    layers: &mut [LayerSpec],
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    for layer in layers {
        bind_layer(layer, None, bridge, diagnostics);
    }
}

fn warn(
    owner: Option<LayerId>,
    message: impl Into<String>,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    diagnostics.push(ExportDiagnostic {
        layer_id: owner,
        message: message.into(),
    });
}

fn color(
    value: &mut [f64; 4],
    owner: Option<LayerId>,
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    match bridge.to_native(*value) {
        Ok(mapped) => {
            if mapped[..3].iter().any(|channel| *channel > 1.0) {
                warn(
                    owner,
                    format!(
                        "HDR point colour lies above the transfer's peak; written as 32 bpc super-white code values {:?} continuing the transfer linearly past the peak code 1.0 (the import's inverse). Output encoding of super-white headroom is the host's.",
                        &mapped[..3]
                    ),
                    diagnostics,
                );
            }
            *value = mapped;
        }
        Err(error) => {
            // A point outside this bounded mapping must not silently pass through
            // as though FX coordinates already belonged to the target profile.
            *value = [0.0, 0.0, 0.0, value[3]];
            warn(
                owner,
                format!(
                    "HDR point-colour mapping unsupported: {error}; replaced RGB with black, coverage alpha and convertible siblings retained."
                ),
                diagnostics,
            );
        }
    }
}

fn optional(
    value: &mut Option<[f64; 4]>,
    owner: Option<LayerId>,
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    if let Some(value) = value {
        color(value, owner, bridge, diagnostics);
    }
}

fn track(
    track: &mut NumericTrack,
    owner: Option<LayerId>,
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    for key in &mut track.keys {
        if let Ok(mut value) = <[f64; 4]>::try_from(key.values.as_slice()) {
            color(&mut value, owner, bridge, diagnostics);
            key.values.copy_from_slice(&value);
        } else {
            warn(
                owner,
                "HDR native colour key has unsupported dimensions; not reinterpreted.",
                diagnostics,
            );
        }
    }
    if track.keys.len() > 1 {
        warn(
            owner,
            "HDR colour key endpoints converted; native nonlinear-transfer interpolation and temporal easing differ from FX display-sRGB interpolation. Keys/times retained; animated fidelity unproved.",
            diagnostics,
        );
    }
}

fn paint(
    paint: &mut ShapePaint,
    owner: Option<LayerId>,
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    match paint {
        ShapePaint::Solid {
            color: Linkable::Value(value),
        } => color(value, owner, bridge, diagnostics),
        ShapePaint::Solid {
            color: Linkable::Linked(_),
        } => {}
        ShapePaint::Gradient { stops, .. } => {
            for stop in stops {
                color(&mut stop.color, owner, bridge, diagnostics);
            }
            warn(
                owner,
                "HDR gradient stop endpoints converted; interpolation in native working transfer differs from FX display-sRGB. Editable stops retained; gradient fidelity unproved.",
                diagnostics,
            );
        }
    }
}

fn contents(
    items: &mut [VectorContent],
    owner: Option<LayerId>,
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    for item in items {
        match item {
            VectorContent::Group(group) | VectorContent::AnimatedGroup(group, _) => {
                contents(&mut group.contents, owner, bridge, diagnostics)
            }
            VectorContent::Paint(spec) => {
                let (value, animations) = match spec {
                    VectorPaintSpec::Fill {
                        paint, animations, ..
                    }
                    | VectorPaintSpec::Stroke {
                        paint, animations, ..
                    } => (paint, animations),
                };
                paint(value, owner, bridge, diagnostics);
                if let Some(value) = &mut animations.color {
                    track(value, owner, bridge, diagnostics);
                }
            }
            VectorContent::Geometry { .. }
            | VectorContent::Modifier(_)
            | VectorContent::Merge(_) => {}
        }
    }
}

fn styles(
    styles: &mut [NativeLayerStyle],
    owner: Option<LayerId>,
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    for style in styles {
        let animations = match style {
            NativeLayerStyle::DropShadow(value) => {
                color(&mut value.color, owner, bridge, diagnostics);
                &mut value.animations
            }
            NativeLayerStyle::InnerShadow(value) => {
                color(&mut value.color, owner, bridge, diagnostics);
                &mut value.animations
            }
            NativeLayerStyle::OuterGlow(value) => {
                color(&mut value.color, owner, bridge, diagnostics);
                &mut value.animations
            }
            NativeLayerStyle::InnerGlow(value) => {
                color(&mut value.color, owner, bridge, diagnostics);
                &mut value.animations
            }
            NativeLayerStyle::BevelEmboss(value) => {
                color(&mut value.highlight_color, owner, bridge, diagnostics);
                color(&mut value.shadow_color, owner, bridge, diagnostics);
                &mut value.animations
            }
            NativeLayerStyle::Satin(value) => {
                color(&mut value.color, owner, bridge, diagnostics);
                &mut value.animations
            }
            NativeLayerStyle::ColorOverlay(value) => {
                color(&mut value.color, owner, bridge, diagnostics);
                &mut value.animations
            }
            NativeLayerStyle::GradientOverlay(value) => {
                for stop in &mut value.stops {
                    color(&mut stop.color, owner, bridge, diagnostics);
                }
                warn(
                    owner,
                    "HDR native gradient-overlay endpoints converted; interpolation fidelity unproved.",
                    diagnostics,
                );
                &mut value.animations
            }
            NativeLayerStyle::Stroke(value) => {
                color(&mut value.color, owner, bridge, diagnostics);
                &mut value.animations
            }
        };
        for animation in animations {
            if matches!(
                animation.property.rsplit('/').next(),
                Some("color" | "highlightColor" | "shadowColor")
            ) {
                track(&mut animation.track, owner, bridge, diagnostics);
            }
        }
    }
}

fn bind_layer(
    layer: &mut LayerSpec,
    owner: Option<LayerId>,
    bridge: HdrColorBridge,
    diagnostics: &mut Vec<ExportDiagnostic>,
) {
    match layer {
        LayerSpec::Options(inner, options) => {
            let owner = Some(options.fx_id);
            for effect in &mut options.effects {
                for property in &mut effect.properties {
                    if matches!(
                        property.kind,
                        ValueKind::Color | ValueKind::EffectColor | ValueKind::VectorColor
                    ) {
                        if let Ok(mut value) = <[f64; 4]>::try_from(property.values.as_slice()) {
                            color(&mut value, owner, bridge, diagnostics);
                            property.values.copy_from_slice(&value);
                        }
                        if let Some(value) = &mut property.animation {
                            track(value, owner, bridge, diagnostics);
                        }
                    }
                }
            }
            styles(&mut options.styles, owner, bridge, diagnostics);
            if !options.effects.is_empty() || !options.styles.is_empty() || options.blend_mode != 0
            {
                warn(
                    owner,
                    "HDR effects/styles/blends keep editable controls but native working-domain arithmetic is not FX display-sRGB arithmetic; fidelity unproved. Existing effect colour clipping diagnostics still apply.",
                    diagnostics,
                );
            }
            bind_layer(inner, owner, bridge, diagnostics);
        }
        LayerSpec::Timed(inner, _) => bind_layer(inner, owner, bridge, diagnostics),
        LayerSpec::Precomposition(value) => bind(&mut value.layers, bridge, diagnostics),
        LayerSpec::VectorProgram(value) => {
            contents(&mut value.contents, owner, bridge, diagnostics)
        }
        LayerSpec::Solid(value) | LayerSpec::AnimatedSolid(value, _) => {
            let mut rgba = [
                f64::from(value.color[0]),
                f64::from(value.color[1]),
                f64::from(value.color[2]),
                1.0,
            ];
            color(&mut rgba, owner, bridge, diagnostics);
            value.color = [rgba[0] as f32, rgba[1] as f32, rgba[2] as f32];
        }
        LayerSpec::Rect(value) | LayerSpec::AnimatedRect(value, _) => {
            optional(&mut value.fill_color, owner, bridge, diagnostics);
            optional(&mut value.stroke_color, owner, bridge, diagnostics);
        }
        LayerSpec::Shape(value) => {
            optional(&mut value.appearance.fill_color, owner, bridge, diagnostics);
            optional(
                &mut value.appearance.stroke_color,
                owner,
                bridge,
                diagnostics,
            );
        }
        LayerSpec::Boolean(value) => {
            optional(&mut value.appearance.fill_color, owner, bridge, diagnostics);
            optional(
                &mut value.appearance.stroke_color,
                owner,
                bridge,
                diagnostics,
            );
        }
        LayerSpec::Text(value) => {
            for key in &mut value.documents.keys {
                color(&mut key.document.fill_color, owner, bridge, diagnostics);
                optional(&mut key.document.stroke_color, owner, bridge, diagnostics);
                for value in std::iter::once(&mut key.document.fill_color)
                    .chain(key.document.stroke_color.as_mut())
                {
                    if value[..3].iter().any(|channel| *channel > 1.0) {
                        value[..3]
                            .iter_mut()
                            .for_each(|channel| *channel = channel.min(1.0));
                        warn(
                            owner,
                            "HDR Source Text colour above the transfer's peak clipped to the peak code 1.0: native Source Text colour stays in 0..=1.",
                            diagnostics,
                        );
                    }
                }
            }
            warn(
                owner,
                "HDR Source Text fill/stroke endpoints converted; native text-run/animator colour domains and interpolation unproved.",
                diagnostics,
            );
        }
        LayerSpec::Footage(_, _) => warn(
            owner,
            "HDR footage source interpretation is not transformed by the point-colour bridge; embedded source metadata retained where admitted, Adobe import interpretation/working conversion unproved.",
            diagnostics,
        ),
        LayerSpec::Null(_) | LayerSpec::Camera(_) => {}
    }
}
