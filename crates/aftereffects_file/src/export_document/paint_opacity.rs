//! Lower overrange paint factors into native opacity controls.
//!
//! A single leaf paint can transfer its uniform overrange factor to static or
//! bounded scalar layer opacity before native clamping. Unsupported tracks and
//! multiple paints retain the diagnosed, post-composite fallback.

use fx_schema::layer::ShapePaint;

use crate::writer::{KeyframeEasing, NumericTrack, VectorContent, VectorPaintSpec};

pub(super) const SATURATION_DIAGNOSTIC: &str = "Overrange paint opacity was combined with solid alpha and clamped to native paint Opacity. Layer opacity remains post-composite; antialiased saturation fidelity remains unverified.";

pub(super) const NORMALIZATION_DIAGNOSTIC: &str = "Single-paint Shape overrange opacity was transferred to static or upper-bounded scalar layer Opacity (including cubic keys with negative troughs) before native clamping to preserve the FX leaf paint/layer product. Both FX and native layer opacity clip negative values to zero; upper overshoot is not admitted. Paint and layer controls are normalized; independent Adobe RGB/alpha fidelity remains unverified.";

/// FX leaf tessellation multiplies paint opacity by layer opacity before
/// drawing. Clamping the paint first loses this product (100 × 0.999% becomes
/// 0.999% rather than 99.9%). One static paint can instead carry its uniform
/// factor on the layer without changing paint order or gradient-stop alpha.
/// Linear/Hold tracks cannot overshoot their endpoints; cubic segments are
/// admitted when their upper control hull stays in range. Negative cubic
/// interiors are safe because both FX and native layer opacity clip at zero:
/// factor * max(0, x) == max(0, factor * x) for positive factors. Native layer keys
/// use fractions, unlike the static percentage; when keyed, the writer's static
/// base is ignored, so use the first scaled key for its representable base.
/// The caller excludes compositing/effect boundaries and paths that reconstruct
/// keys from unnormalized FX.
pub(super) fn normalize_single_paint(
    contents: &mut [VectorContent],
    layer_opacity: f64,
    opacity_track: Option<&mut NumericTrack>,
) -> Option<f64> {
    let mut paints = contents.iter_mut().filter_map(|content| match content {
        VectorContent::Paint(VectorPaintSpec::Fill {
            opacity,
            blend_mode,
            animations,
            ..
        })
        | VectorContent::Paint(VectorPaintSpec::Stroke {
            opacity,
            blend_mode,
            animations,
            ..
        }) => Some((opacity, blend_mode, animations)),
        _ => None,
    });
    let (opacity, blend_mode, animations) = paints.next()?;
    if paints.next().is_some()
        || *blend_mode != Default::default()
        || animations.opacity.is_some()
        || animations.color.is_some()
        || !valid_derived_value(*opacity)
        || *opacity <= 100.0
    {
        return None;
    }
    let factor = *opacity / 100.0;
    let product = if let Some(track) = opacity_track.as_deref() {
        if !bounded_opacity_track(track, factor) {
            return None;
        }
        // Animated native Opacity reads keys, not the FX static base. A base of
        // 100% must not prevent scaling keys that all remain within 0..100%.
        track.keys[0].values[0] * factor * 100.0
    } else {
        layer_opacity * factor
    };
    if !product.is_finite() || !(0.0..=100.0).contains(&product) {
        return None;
    }
    // Validate the entire track before changing either paint or keys so a
    // rejected curve retains its original diagnosed fallback, atomically.
    if let Some(track) = opacity_track {
        for key in &mut track.keys {
            key.values[0] *= factor;
        }
    }
    *opacity = 100.0;
    Some(product)
}

fn bounded_opacity_track(track: &NumericTrack, factor: f64) -> bool {
    if track.keys.is_empty()
        || !track.keys.iter().all(|key| {
            let [value] = key.values.as_slice() else {
                return false;
            };
            let product = value * factor;
            product.is_finite()
                && (0.0..=1.0).contains(&product)
                && match key.easing.as_slice() {
                    [KeyframeEasing::Linear | KeyframeEasing::Hold] => true,
                    [KeyframeEasing::CubicBezier { x1, y1, x2, y2 }] => {
                        [*x1, *y1, *x2, *y2].into_iter().all(f64::is_finite)
                            && (0.0..=1.0).contains(x1)
                            && (0.0..=1.0).contains(x2)
                            && (*x1 != 0.0 || *y1 == 0.0)
                            && (*x2 != 1.0 || *y2 == 1.0)
                    }
                    _ => false,
                }
                && key.spatial_in.is_empty()
                && key.spatial_out.is_empty()
        })
    {
        return false;
    }
    track.keys.windows(2).all(|pair| {
        let [previous, current] = pair else {
            return false;
        };
        let Some(elapsed) = current.time_millis.checked_sub(previous.time_millis) else {
            return false;
        };
        if elapsed <= 0 {
            return false;
        }
        let KeyframeEasing::CubicBezier { x1, y1, x2, y2 } = current.easing[0] else {
            return true;
        };
        let duration = elapsed as f64 / 1000.0;
        let start = previous.values[0] * factor;
        let delta = current.values[0] * factor - start;
        // The cubic is a convex combination of four control values. Its
        // upper hull must fit; negative troughs clip to zero on both sides
        // of this positive paint-factor transfer, without altering handles.
        [start + delta * y1, start + delta * y2]
            .into_iter()
            .all(|value| value.is_finite() && value <= 1.0)
            && (x1 == 0.0 || (y1 / x1 * delta / duration).is_finite())
            && (x2 == 1.0 || ((1.0 - y2) / (1.0 - x2) * delta / duration).is_finite())
    })
}

pub(super) fn needs_folding(paints: &super::paint_controls::PaintMaterialization) -> bool {
    paints.fills().iter().any(|paint| paint.opacity > 1.0)
        || paints.strokes().iter().any(|paint| paint.opacity > 1.0)
}

pub(super) fn fold(contents: &mut [VectorContent]) -> Result<bool, &'static str> {
    let mut saturation = false;
    for content in contents {
        if let VectorContent::Group(group) | VectorContent::AnimatedGroup(group, _) = content {
            saturation |= fold(&mut group.contents)?;
            continue;
        }
        let (paint, opacity, animations) = match content {
            VectorContent::Paint(VectorPaintSpec::Fill {
                paint,
                opacity,
                animations,
                ..
            })
            | VectorContent::Paint(VectorPaintSpec::Stroke {
                paint,
                opacity,
                animations,
                ..
            }) => (paint, opacity, animations),
            _ => continue,
        };
        if animations.opacity.is_some() {
            return Err(
                "Independent paint Opacity animation cannot be combined with overrange paint folding",
            );
        }
        if animations.color.is_some() {
            return Err(
                "Animated solid paint color cannot be combined with overrange paint folding",
            );
        }
        if !valid_derived_value(*opacity) {
            return Err("Shape paint opacity cannot be folded into native paint Opacity");
        }
        let alpha = match paint {
            ShapePaint::Solid { color } => {
                let alpha = color[3];
                if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
                    return Err("Shape paint alpha cannot be folded into native paint Opacity");
                }
                color[3] = 1.0;
                alpha
            }
            ShapePaint::Gradient { stops, .. } => {
                if stops
                    .iter()
                    .any(|stop| !stop.color[3].is_finite() || !(0.0..=1.0).contains(&stop.color[3]))
                {
                    return Err(
                        "Gradient stop alpha cannot be combined with overrange paint folding",
                    );
                }
                // Spatially varying alpha stays on each stop. Only the uniform
                // paint opacity is clamped here, before layer compositing.
                1.0
            }
        };
        let product = *opacity * alpha;
        if !valid_derived_value(product) {
            return Err("Shape paint opacity product exceeds the native numeric domain");
        }
        saturation |= product > 100.0;
        *opacity = product.clamp(0.0, 100.0);
    }
    Ok(saturation)
}

fn valid_derived_value(value: f64) -> bool {
    value.is_finite() && value >= 0.0 && value <= f64::from(f32::MAX)
}

#[cfg(test)]
#[path = "paint_opacity_tests.rs"]
mod tests;
