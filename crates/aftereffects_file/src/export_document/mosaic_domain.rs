//! Sampling-domain proof, not native painted-output bounds or a fidelity claim.
use std::collections::BTreeMap;

use fx_schema::{EffectData, EffectPayload, Layer, LayerData, LayerEffect, Position, Transform};

use super::{AnimationIndex, hierarchy, media};

/// Constructible only after certifying the renderer's raw root canvas domain.
pub(super) struct RootCanvas(());

pub(super) fn prove_root_canvas(
    owner: &Layer,
    siblings: &[Layer],
    dynamics: &AnimationIndex<'_>,
    canvas: fx_schema::Dimensions,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
) -> Option<RootCanvas> {
    let LayerData::Adjustment(adjustment) = owner.data() else {
        return None;
    };
    // TwoD owner geometry is inert: FX uses an identity effect node and native
    // export fixes the solid geometry. A ThreeD position (even z=0) instead
    // joins depth sorting before backdrop capture, invalidating the declared
    // lower-sibling suffix used by this proof.
    if owner.parent_id().is_some()
        || adjustment.is_hidden
        || adjustment.blend_mode != Default::default()
        || adjustment.transform.opacity.value() != 100.0
        || !matches!(adjustment.transform.position, Position::TwoD(_))
        || !adjustment.masks.is_empty()
        || adjustment.track_matte.is_some()
        || dynamics.for_layer(owner.id()).next().is_some()
        || canvas.width == 0
        || canvas.height == 0
        || canvas.width > 16_384
        || canvas.height > 16_384
    {
        return None;
    }
    // Other active stages may change the sampled plane. Bypassed siblings
    // remain editable and in place; no control or effect order is rewritten.
    if adjustment.effects.iter().any(|record| match record.data() {
        EffectData::Identified { enabled: false, .. } => false,
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => {
            !matches!(effect, EffectPayload::Known(LayerEffect::Mosaic { .. }))
        }
    }) {
        return None;
    }
    let index = siblings.iter().position(|layer| layer.id() == owner.id())?;
    let mut measurable = false;
    for layer in &siblings[index + 1..] {
        // This deliberately excludes clocks/parents/unknown or projected
        // geometry. Native masked support must never substitute for raw paths.
        if layer.parent_id().is_some()
            || dynamics.for_layer(layer.id()).next().is_some()
            || !layer.data().effects().is_empty()
        {
            return None;
        }
        let transform = match layer.data() {
            LayerData::Rect(rect)
                if !rect.is_hidden
                    && rect.masks.is_empty()
                    && rect.track_matte.is_none()
                    && rect.rect.fill_enabled
                    && !rect.rect.stroke_enabled
                    && rect.rect.roundness == 0.0
                    // Unrounded integer Rect arithmetic is exact at this
                    // canvas range in the renderer's f32 path lowering.
                    && rect.rect.position.into_iter().chain(rect.rect.size)
                        .all(|value| value.is_finite() && value.fract() == 0.0) =>
            {
                &rect.transform
            }
            LayerData::Shape(shape)
                if !shape.is_hidden
                    && shape.masks.is_empty()
                    && shape.track_matte.is_none()
                    && shape.shape.ellipse.is_none()
                    && shape.shape.poly_star.is_none()
                    && shape.shape.round_corners.is_none()
                    && shape.shape.offset_paths.is_none()
                    && shape.shape.trim.is_none()
                    && shape.shape.strokes.is_empty()
                    && !shape.shape.fills.is_empty()
                    && shape
                        .shape
                        .path
                        .commands
                        .iter()
                        .all(|command| command.corner_radius().is_none()) =>
            {
                &shape.transform
            }
            _ => return None,
        };
        // No transformed/nested plane equivalence is inferred. With identity
        // affine geometry and no generators/modifiers, the existing enclosure
        // includes every raw path control point used by the renderer. f32
        // coordinate casts are monotone against the exact integer canvas edges.
        if !identity_geometry(transform) {
            return None;
        }
        let bounds =
            hierarchy::all_time_layer_bounds(layer, dynamics, resolved_media, canvas).ok()??;
        if bounds
            .min
            .into_iter()
            .chain(bounds.max)
            .any(|value| !value.is_finite())
            || bounds.min[0] < 0.0
            || bounds.min[1] < 0.0
            || bounds.max[0] > f64::from(canvas.width)
            || bounds.max[1] > f64::from(canvas.height)
            || bounds.max[0] <= bounds.min[0]
            || bounds.max[1] <= bounds.min[1]
        {
            return None;
        }
        measurable = true;
    }
    measurable.then_some(RootCanvas(()))
}

fn identity_geometry(transform: &Transform) -> bool {
    transform.anchor_point == [0.0; 2]
        && transform.position == fx_schema::Position::TwoD([0.0; 2])
        && transform.scale == [100.0; 2]
        && transform.rotation == 0.0
        && transform.rotation_x == 0.0
        && transform.rotation_y == 0.0
        && transform.orientation == [0.0; 3]
        && transform.skew == 0.0
        && transform.skew_axis == 0.0
}

pub(super) fn lower_checkbox(
    effect: &LayerEffect,
    native: &mut crate::writer::effects::NativeEffect,
    proof: Option<&RootCanvas>,
) -> Option<String> {
    let LayerEffect::Mosaic { sharp_colors, .. } = effect else {
        return None;
    };
    if proof.is_none() {
        return Some("Mosaic raw sampling domain equivalence is unproved (requires an ungated root canvas Adjustment over contained static identity-affine Rect/Path geometry); authored Sharp Colors retained. FX still samples centers even with false; native averaging/grid semantics remain unsupported, not faithful preservation.".into());
    }
    if !sharp_colors {
        let checkbox = native
            .properties
            .iter_mut()
            .find(|property| property.match_name == "ADBE Mosaic-0003")?;
        checkbox.values[0] = 1.0;
        return Some("Sharp Colors set to on for proven raw root canvas-domain FX block-center sampling; counts, keys, bypass and order retained. The saved false checkbox is normalized; native render fidelity remains unmeasured.".into());
    }
    None
}
