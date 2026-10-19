use super::*;

// A root composite can rasterize directly in output coordinates: clipping there
// is the final document clip, not a guessed source-plane crop.
pub(super) fn eligible(
    group: &GroupLayer,
    end: Time,
    dynamics: &AnimationIndex<'_>,
    siblings: &[Layer],
) -> bool {
    group.parent.is_none()
        && playback_is_identity(&group.playback)
        && group.playback.input_range().start == Time::ZERO
        && group.playback.input_range().end() <= end
        && !group.motion_blur
        && !hierarchy::has_skew(group)
        && !hierarchy::subtree_needs_projection(&group.layers, dynamics)
        && group.effects.is_empty()
        && group.masks.is_empty()
        && group.track_matte.is_none()
        && group.fills.is_empty()
        && group_layout_is_empty(group)
        && contains_image(&group.layers)
        && !has_external_parent_reference(siblings, group.id)
        && !contains_adjustment(siblings)
        && !contains_motion_blur(&group.layers)
}

fn has_external_parent_reference(layers: &[Layer], owner: LayerId) -> bool {
    layers.iter().any(|layer| {
        // Explicit child-to-owner parenting is already represented by containment.
        layer.id() != owner
            && (layer.parent_id() == Some(owner)
                || matches!(layer.data(), LayerData::Group(group) if has_external_parent_reference(&group.layers, owner)))
    })
}

// Adjustment kernels can need pixels outside the final canvas.
fn contains_adjustment(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| match layer.data() {
        LayerData::Adjustment(_) => true,
        LayerData::Group(group) => contains_adjustment(&group.layers),
        _ => false,
    })
}

// Only descendants inherit the new geometry parent. Their motion blur must not
// start sampling an otherwise unblurred owner's geometry at shutter times.
fn contains_motion_blur(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| {
        !native_layer_options(layer).is_ok_and(|options| !options.motion_blur)
            || matches!(layer.data(), LayerData::Group(group) if contains_motion_blur(&group.layers))
    })
}

fn contains_image(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| match layer.data() {
        LayerData::Image(_) => true,
        LayerData::Group(group) => contains_image(&group.layers),
        _ => false,
    })
}

pub(super) fn lower(
    layer: LayerSpec,
    dimensions: fx_schema::Dimensions,
    helper_id: LayerId,
) -> Result<LayerSpec, &'static str> {
    let LayerSpec::Options(inner, mut options) = layer else {
        return Err("Root composite requires occurrence options");
    };
    let LayerSpec::Precomposition(mut source) = *inner else {
        return Err("Root composite requires a precomposition source");
    };
    if source.collapse_transformations
        || options
            .transform_3d
            .as_ref()
            .is_some_and(|(transform, _)| transform.is_three_d)
    {
        return Ok(LayerSpec::Options(
            Box::new(LayerSpec::Precomposition(source)),
            options,
        ));
    }
    let width = u16::try_from(dimensions.width)
        .map_err(|_| "Root composite width exceeds native dimensions")?;
    let height = u16::try_from(dimensions.height)
        .map_err(|_| "Root composite height exceeds native dimensions")?;
    source.width = width;
    source.height = height;
    if let Some(record) = &mut source.composition_record {
        record
            .set_dimensions(width, height)
            .map_err(|_| "Root composite dimensions are invalid")?;
    }

    let mut transform = source.transform.clone();
    let mut animations = std::mem::take(&mut source.transform_animations);
    let mut opacity = transform.opacity;
    let mut opacity_keys = animations.opacity.take();
    transform.opacity = 100.0;
    let mut sidecar = options.transform_3d.take();
    if let Some((transform, animations)) = &mut sidecar {
        opacity = transform.opacity * 100.0;
        opacity_keys = animations.opacity.take();
        transform.opacity = 1.0;
    }
    let mut helper_options = options.clone();
    helper_options.fx_id = helper_id;
    helper_options.enabled = true;
    helper_options.source_clock = None;
    helper_options.transform_3d = sidecar;
    for child in &mut source.layers {
        child
            .assign_root_parent(helper_id)
            .map_err(|_| "Root composite parenting failed")?;
    }
    source.layers.push(LayerSpec::Options(
        Box::new(LayerSpec::Null(crate::writer::NullLayerSpec {
            name: format!("{} geometry", source.name),
            transform,
            transform_animations: animations,
        })),
        helper_options,
    ));
    source.transform = SolidTransform {
        anchor: [0.0; 2],
        position: [0.0; 2],
        scale: [100.0; 2],
        rotation: 0.0,
        opacity,
    };
    source.transform_animations.opacity = opacity_keys;
    Ok(LayerSpec::Options(
        Box::new(LayerSpec::Precomposition(source)),
        options,
    ))
}
