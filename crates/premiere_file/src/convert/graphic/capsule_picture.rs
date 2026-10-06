//! Full native Capsule pictures, on a placement/source clock pair.

use super::super::premiere_to_tesseract::{motion_tracks, motion_transform, CompositionShutter};
use super::*;
use crate::{capsule::CapsuleError, error::BuildError, schema::PrCapsule};
use fx_conv::{ConversionDiagnostic, DiagnosticKind};

/// A valid template without mapped picture content omits only this occurrence.
pub(in crate::convert) fn import_capsule(
    capsule: &PrCapsule,
    canvas: [u32; 2],
    layer_id: LayerId,
    index: usize,
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Option<(Layer, Option<Layer>)>> {
    capsule.validate()?;
    let placement = &capsule.placement;
    let record = placement.id().unwrap_or("Capsule");
    let fail = |source: CapsuleError| BuildError::Capsule {
        context: record.into(),
        source: Box::new(source),
    };
    let source_id = LayerId::new((*scope.next_index as u64 + 1).max(scope.effect_ids.next()));
    let namespace = format!("premiere-capsule-{}", layer_id.value());
    let mut picture = capsule
        .source
        .import(source_id.value() + 1, &namespace)
        .map_err(fail)?;
    for diagnostic in &picture.diagnostics {
        let reason = format!("Capsule native picture: {diagnostic}");
        if diagnostic.diagnostic().kind == DiagnosticKind::Approximation {
            approximate(omissions, record, reason);
        } else {
            omit(omissions, OmissionScope::Feature, record, reason);
        }
    }
    let document = picture
        .take_document()
        .map_err(|error| fail(crate::capsule::CapsuleError::Template(error.into())))?;
    fn meaningful(layer: &Layer) -> bool {
        match layer.data() {
            LayerData::Text(text) => !text.source_text.text.is_empty(),
            LayerData::Shape(_)
            | LayerData::Rect(_)
            | LayerData::Image(_)
            | LayerData::Video(_)
            | LayerData::Media(_) => true,
            LayerData::Group(group) => group.layers.iter().any(meaningful),
            _ => false,
        }
    }
    let roots = document.composition().layers();
    let [root] = roots else {
        return Err(unsupported(
            "Capsule picture mapper must return one native composition root",
        ));
    };
    let LayerData::Group(root) = root.data() else {
        return Err(unsupported("Capsule picture root must be a Group"));
    };
    if !roots.iter().any(meaningful) {
        omit(
            omissions,
            OmissionScope::Occurrence,
            record,
            "Capsule occurrence omitted: native template has no editable picture content",
        );
        return Ok(None);
    }
    let mut root = root.clone();
    root.parent = Some(source_id);
    let native_canvas = [document.dimensions().width, document.dimensions().height];
    let mut entries = dynamics.entries().to_vec();
    entries.extend_from_slice(document.composition().dynamics().entries());
    *dynamics = AnimationGraph::from_entries(entries)
        .map_err(super::super::premiere_to_tesseract::map_animation_graph_error)?;
    *scope.next_index = usize::try_from(picture.next_id - 1)
        .map_err(|_| unsupported("Capsule picture IDs exceed host index range"))?;
    scope.effect_ids.skip_to(picture.next_id);
    let settings = document.composition().motion_blur();
    if settings.enabled {
        CompositionShutter::request(scope.composition_shutter, settings, record, omissions);
    }
    let canvas_guide = next_layer_id(scope);
    let canvas_mask = FxItemId::new(*scope.next_index as u64 + 1);
    *scope.next_index += 1;
    let content = super::super::after_effects::clip_to_canvas(
        Layer::from_data(&LayerData::Group(root))?,
        native_canvas,
        canvas_guide,
        canvas_mask,
    )?;
    let window = tick_range(placement.start_ticks, placement.end_ticks)?;
    let local = TimeRangeProperty::new(Time::ZERO, window.duration);
    let saved_window = tick_range(placement.in_ticks, capsule.source_out_ticks)?;
    let template_window = TimeRangeProperty::new(Time::ZERO, document.duration());
    let source_window = if saved_window.start >= template_window.end()
        || saved_window.duration == Duration::ZERO
    {
        approximate(omissions, record, "Capsule source window is outside its native composition or below target clock precision; full bounded native composition retained as the linear source fallback");
        template_window
    } else if saved_window.end() > template_window.end() {
        approximate(omissions, record, "Capsule source window extends beyond its native composition; linear source window bounded to native duration");
        TimeRangeProperty::new(
            saved_window.start,
            Duration::from_millis(
                template_window.end().as_millis() - saved_window.start.as_millis(),
            ),
        )
    } else {
        saved_window
    };
    let source = GroupLayer {
        parent: Some(layer_id),
        playback: fx_schema::LayerPlayback::linear(local, local, source_window, 0)
            .map_err(unsupported)?,
        ..plain_group(
            source_id,
            "Capsule native source clock".into(),
            local,
            identity_transform(),
            content.to_vec(),
        )?
    };
    let tracks = motion_tracks(
        &capsule.motion_animations,
        placement.in_ticks,
        None,
        layer_id,
        None,
        native_canvas,
        canvas,
        record,
        omissions,
    );
    set_tracks(dynamics, tracks)?;
    set_tracks(
        dynamics,
        object_tracks(
            &placement.animations,
            true,
            placement.in_ticks,
            layer_id,
            canvas,
            record,
            omissions,
        ),
    )?;
    let mut transform = motion_transform(&placement.clip_motion, native_canvas, canvas);
    transform.opacity = PercentageProperty::new(placement.opacity)
        .ok_or_else(|| unsupported("Capsule opacity must be finite and between 0 and 100"))?;
    let (mask, guide) =
        clip_opacity_mask(placement, canvas, index, scope, dynamics, omissions)?.unzip();
    let root = GroupLayer {
        parent: scope.parent,
        is_hidden: !placement.enabled,
        blend_mode: placement.blend_mode.fx_mode(),
        masks: mask.into_iter().collect(),
        playback: fx_schema::LayerPlayback::linear(window, window, local, 0)
            .map_err(unsupported)?,
        ..plain_group(
            layer_id,
            format!("Premiere Capsule {}", index + 1),
            window,
            transform,
            vec![Layer::from_data(&LayerData::Group(source))?],
        )?
    };
    scope.effect_ids.skip_to(*scope.next_index as u64 + 1);
    capsule.source.keep_picture(picture);
    Ok(Some((Layer::from_data(&LayerData::Group(root))?, guide)))
}
