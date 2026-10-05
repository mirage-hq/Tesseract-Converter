//! The editable picture of a linked After Effects composition's clip.
//!
//! A video clip of linked media imports as any video clip does: its Motion,
//! Opacity, masks, effects, matte and clocks follow the clip rules. Only its
//! picture layer differs: in place of a video layer of a packaged asset, a
//! group on the same clip clock whose child is the composition's editable root
//! Group, imported by [`LinkedCompositions`] or supplied by the caller's
//! importer, and clipped to the composition's canvas ([`clip_to_canvas`]).
//! When the clip shows the composition from a source In, at another speed or
//! time-remapped, a source group between them plays the root on the
//! composition clock. On the document clock the clip group also seeds the
//! runtime's remap chain (`document_clock_seed`); under a stage group or nest
//! that starts later it cannot, and time-remapped content is reported
//! ([`offset_clock_note`]).

use super::{
    background::{black_shape, identity_transform, plain_group},
    nested::LayerScope,
    premiere_to_tesseract::{guide_layer, guide_mask, map_animation_graph_error},
};
use crate::{
    approximate,
    error::{unsupported, Result},
    format::MediaId,
    omit, Omission, OmissionScope,
};
use fx_conv::{ConversionDiagnostic, DiagnosticKind};
use fx_schema::{
    AnimationGraph, Duration, FxItemId, GroupLayer, Layer, LayerData, LayerId, MotionBlurSettings,
    Time, TimeRangeProperty, TimeRemapProperty,
};

/// The editable root of the linked composition of `media` for a clip that
/// shows it up to `source_end`, as a child of `parent`, with identities after
/// every one that `scope` has given out, and the motion blur that the
/// composition enables, which the clip requests of the FX composition's
/// shutter once its picture forms. Its animation joins `dynamics` and its
/// import notes `omissions` (once per media). `Ok(Err(reason))` is a
/// composition that forms no editable picture of the clip.
pub(super) fn linked_root(
    media: &MediaId,
    parent: LayerId,
    source_end: Time,
    placement: std::result::Result<f64, &'static str>,
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<std::result::Result<(Layer, Option<MotionBlurSettings>), String>> {
    // After Effects draws layer, item and effect ids from one counter.
    let first_id = (*scope.next_index as u64 + 1).max(scope.effect_ids.next());
    let mut picture = match scope.linked.picture(media, parent, first_id, source_end)? {
        Ok(picture) => picture,
        Err(reason) => return Ok(Err(reason)),
    };
    match placement {
        Ok(factor) if factor != 1. => match super::linked_shadow_scale::apply(&mut picture.root, &mut picture.animations, factor) {
            Ok(report) => {
                if report.compensated>0 { approximate(omissions,media.as_str(),format!("linked After Effects DropShadow offset/blur/spread and SimpleChoker signed radius in screen pixels, with their native tracks, compensated for source-declared static uniform placement factor {factor}; GaussianBlur already inherits placement scale")); }
                if report.declined>0 { omit(omissions,OmissionScope::Feature,media.as_str(),format!("{} linked screen-pixel effect stages retained without placement compensation: nonidentity/animated internal scale, unsupported graph references or SimpleChoker radius beyond ±10 render pixels", report.declined)); }
            }
            Err(reason) => omit(omissions,OmissionScope::Feature,media.as_str(),format!("linked screen-pixel placement compensation declined: {reason}; original picture retained")),
        },
        Err(reason) => {
            fn shadows(layer:&Layer) -> bool { layer.effects().iter().any(|record| matches!(record.data(),fx_schema::EffectData::Identified {effect:fx_schema::EffectPayload::Known(fx_schema::LayerEffect::DropShadow(_)|fx_schema::LayerEffect::SimpleChoker{..}),..})) || layer.child_layers().is_some_and(|children| children.iter().any(shadows)) }
            if shadows(&picture.root) { omit(omissions,OmissionScope::Feature,media.as_str(),reason); }
        },
        Ok(_) => {},
    }
    *scope.next_index = usize::try_from(picture.next_id - 1)
        .map_err(|_| unsupported("linked composition identities exceed the index range"))?;
    scope.effect_ids.skip_to(picture.next_id);
    if !picture.animations.is_empty() {
        let mut entries = dynamics.entries().to_vec();
        entries.extend(picture.animations);
        *dynamics = AnimationGraph::from_entries(entries).map_err(map_animation_graph_error)?;
    }
    for diagnostic in &picture.diagnostics {
        let reason = format!("linked After Effects composition: {diagnostic}");
        match diagnostic.diagnostic().kind {
            DiagnosticKind::Approximation => approximate(omissions, media.as_str(), reason),
            _ => omit(omissions, OmissionScope::Feature, media.as_str(), reason),
        }
    }
    let motion_blur = picture.motion_blur.enabled.then_some(picture.motion_blur);
    Ok(Ok((picture.root, motion_blur)))
}

/// `root`, a linked composition's picture, clipped to the composition's
/// `canvas` as After Effects renders it, and the guide of that clip: the
/// canvas rect `guide_id`, a sibling of `root` over its range, whose shape the
/// Add mask `mask_id` on `root` takes. Premiere places that frame, so the
/// clip's own Motion, masks and effects apply to the clipped picture above.
pub(super) fn clip_to_canvas(
    root: Layer,
    canvas: [u32; 2],
    guide_id: LayerId,
    mask_id: FxItemId,
) -> Result<[Layer; 2]> {
    let LayerData::Group(mut group) = root.data().clone() else {
        return Err(unsupported(
            "a linked composition's picture root is not a Group",
        ));
    };
    let guide = guide_layer(
        guide_id,
        "Linked composition canvas".into(),
        group.parent,
        group.playback.input_range(),
        identity_transform(),
        black_shape(canvas[0], canvas[1]),
    );
    group.masks.push(guide_mask(mask_id, guide_id, 0.0));
    Ok([
        Layer::from_data(&LayerData::Group(group))?,
        Layer::from_data(&LayerData::Rect(guide))?,
    ])
}

/// The approximation of `subject`, a linked composition whose picture group
/// is under a stage group or nest that starts after the document start and
/// whose content is time-remapped: the picture group cannot seed the runtime's
/// remap chain there (`document_clock_seed` in `premiere_to_tesseract`).
pub(super) fn offset_clock_note(subject: &str) -> String {
    format!(
        "{subject} under a stage group or nest that starts after the document start: the FX runtime evaluates its time-remapped After Effects animation on the document clock, early by that start; placement, visibility and media times are kept"
    )
}

/// Whether `layer`, or a layer under it, has `playback`.
pub(super) fn contains_playback(layer: &Layer) -> bool {
    let own = match layer.data() {
        LayerData::Group(group) => group.playback.time_remap().is_some(),
        LayerData::Video(video) => video.playback.time_remap().is_some(),
        LayerData::Audio(audio) => audio.playback.time_remap().is_some(),
        LayerData::Pag(pag) => pag.playback.is_some(),
        _ => false,
    };
    own || layer
        .child_layers()
        .into_iter()
        .flatten()
        .any(contains_playback)
}

/// Whether `matte`, a clip's root layer, is a linked picture group with its
/// document-clock seed and time-remapped content under it.
pub(super) fn remaps_linked_content(matte: &Layer) -> bool {
    matches!(matte.data(), LayerData::Group(group) if group.playback.time_remap().is_some())
        && matte
            .child_layers()
            .into_iter()
            .flatten()
            .any(contains_playback)
}

/// The source group `id` under clip group `parent`, which plays the linked
/// composition's `picture` (its clipped root and canvas guide) on the
/// composition clock: `playback` maps the clip clock, on which the group spans
/// `duration`, onto the composition clock by the clip's source In, speed or
/// Time Remapping. It carries nothing else, so the clip's own keys stay on the
/// clip clock of `parent`.
pub(super) fn source_group(
    id: LayerId,
    name: String,
    parent: LayerId,
    duration: Duration,
    playback: TimeRemapProperty,
    picture: Vec<Layer>,
) -> Result<Layer> {
    Ok(Layer::from_data(&LayerData::Group(GroupLayer {
        parent: Some(parent),
        playback: fx_schema::LayerPlayback::remapped(
            TimeRangeProperty::new(Time::ZERO, duration),
            playback,
            0,
        )
        .map_err(unsupported)?,
        ..plain_group(
            id,
            name,
            TimeRangeProperty::new(Time::ZERO, duration),
            identity_transform(),
            picture,
        )?
    }))?)
}
