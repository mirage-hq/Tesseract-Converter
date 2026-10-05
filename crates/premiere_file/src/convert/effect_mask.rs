//! One standard effect's spatial scope, isolated from other timeline siblings.
//!
//! The video carries the prefix, an adjustment carries the masked effect, and
//! the enclosing group carries the suffix and the clip's Motion. FX clips the
//! adjustment input before filtering; Premiere clips the filtered output.

use super::{
    background::identity_transform,
    effects::{self, EffectHost, EffectIdAllocator},
    mask_animation,
    premiere_to_tesseract::{opacity_mask, shape_guide},
    tesseract_to_premiere::{
        graphic_mask, has_background, layer_animations, source_frame, WrittenAnimation,
    },
    timing,
};
use crate::{
    approximate,
    schema::{MaskBoundary, PrEffect, PrEffectParams, PrMask, PrMediaKind, PrVideoOccurrence},
    Omission, OmissionKind,
};
use fx_schema::{
    animator::PropertyKeyframeTrack, AdjustmentLayer, AnimationGraph, BlendMode, EffectRecord,
    FxItemId, GroupLayer, Layer, LayerData, LayerId, PropertyTarget, Time, TimeRangeProperty,
};

pub(super) const APPROXIMATION: &str = "effect mask retained on an isolated editable adjustment: FX clips its input before filtering, whereas Premiere masks output computed from the whole input; filter edges can differ";

fn supports_effect(effect: &PrEffect) -> bool {
    !matches!(
        effect.params,
        PrEffectParams::Transform(_) | PrEffectParams::PosterizeTime { .. }
    ) && !effect.requires_coverage()
}

/// Native admission happens before graph identities or animation are published.
/// Other clip pipelines retain their existing admission rules.
pub(super) fn native_scope(
    clip: &PrVideoOccurrence,
    kind: PrMediaKind,
    frame: [u32; 2],
    canvas: [u32; 2],
    document_canvas: [u32; 2],
) -> Result<Option<NativeScope>, String> {
    let mut masked = clip
        .effects
        .iter()
        .enumerate()
        .filter(|(_, effect)| effect.mask.is_some());
    let Some((index, effect)) = masked.next() else {
        return Ok(None);
    };
    if masked.next().is_some() {
        return Err("several masked effects require separate effect scopes".into());
    }
    if !matches!(kind, PrMediaKind::Video { .. }) || frame != canvas || canvas != document_canvas {
        return Err("effect masks require a physical video whose displayed frame matches the sequence and document canvas".into());
    }
    if clip.playback_rate != 1.0 || clip.time_remap.is_some() {
        return Err(
            "effect masks require unit forward playback without Time Remapping or a hold".into(),
        );
    }
    if clip.opacity_mask.is_some()
        || !clip.crop.is_default()
        || clip.linear_wipe.is_some()
        || clip.track_matte.is_some()
        || clip.stroke.is_some()
        || clip
            .source_effects
            .as_ref()
            .is_some_and(|stack| !stack.effects.is_empty())
        || clip.effects.iter().any(|effect| !supports_effect(effect))
    {
        return Err("effect masks do not combine with clip masks, mattes, Stroke, source effects, Transform, temporal effects or coverage-changing effects".into());
    }
    if effect
        .mask
        .as_ref()
        .is_some_and(|mask| !mask.path_keys.is_empty())
    {
        return Err("this effect-mask scope requires a static guide outline; numeric mask controls remain editable".into());
    }
    Ok(Some(NativeScope { index, kind }))
}

/// The single effect and physical media kind admitted in the shared frame.
#[derive(Clone, Copy)]
pub(super) struct NativeScope {
    index: usize,
    kind: PrMediaKind,
}

pub(super) struct Imported {
    pub(super) prefix: Vec<EffectRecord>,
    pub(super) suffix: Vec<EffectRecord>,
    pub(super) children: Vec<Layer>,
    pub(super) tracks: Vec<(PropertyTarget, PropertyKeyframeTrack)>,
}

/// Convert one stack segment in the identity source plane, before outer Motion.
fn import_segment(
    clip: &PrVideoOccurrence,
    segment: &[PrEffect],
    owner: LayerId,
    frame: [u32; 2],
    kind: PrMediaKind,
    ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> (
    Vec<EffectRecord>,
    Vec<(PropertyTarget, PropertyKeyframeTrack)>,
) {
    let mut local = clip.clone();
    local.effects = segment.to_vec();
    for effect in &mut local.effects {
        effect.mask = None;
    }
    local.transform = Default::default();
    local.animations.clear();
    effects::import_effects(
        &local,
        owner,
        false,
        MaskBoundary::Flat,
        false,
        false,
        kind,
        frame,
        frame,
        frame,
        ids,
        omissions,
    )
}

pub(super) fn import(
    clip: &PrVideoOccurrence,
    native: NativeScope,
    video: LayerId,
    group: LayerId,
    range: TimeRangeProperty,
    scope: &mut super::nested::LayerScope<'_, '_>,
    omissions: &mut Vec<Omission>,
) -> Result<Imported, String> {
    // Admission requires physical video in this same document-sized frame.
    let frame = scope.document_canvas;
    let NativeScope { index, kind } = native;
    let adjustment = LayerId::new(*scope.next_index as u64 + 1);
    let guide = LayerId::new(*scope.next_index as u64 + 2);
    let mask_id = FxItemId::new(*scope.next_index as u64 + 3);
    let mask = clip.effects[index]
        .mask
        .as_ref()
        .ok_or("effect scope has no mask")?;
    let mut notes = Vec::new();
    let (masked, mut tracks) = import_segment(
        clip,
        &clip.effects[index..=index],
        adjustment,
        frame,
        kind,
        scope.effect_ids,
        &mut notes,
    );
    if masked.len() != 1 || notes.iter().any(|note| note.kind == OmissionKind::Omitted) {
        return Err(format!(
            "masked effect cannot be retained atomically: {notes:?}"
        ));
    }
    let (prefix, prefix_tracks) = import_segment(
        clip,
        &clip.effects[..index],
        video,
        frame,
        kind,
        scope.effect_ids,
        &mut notes,
    );
    let (suffix, suffix_tracks) = import_segment(
        clip,
        &clip.effects[index + 1..],
        group,
        frame,
        kind,
        scope.effect_ids,
        &mut notes,
    );
    tracks.extend(prefix_tracks);
    tracks.extend(suffix_tracks);
    tracks.extend(mask_animation::import_tracks(mask, mask_id, clip.in_ticks)?);
    let (path_mask, outline) = opacity_mask(mask, mask_id, guide, frame, clip.record(), &mut notes)
        .map_err(|error| error.to_string())?;
    let children = vec![
        Layer::from_data(&LayerData::Adjustment(AdjustmentLayer {
            id: adjustment,
            name: "Premiere masked effect".into(),
            description: String::new(),
            is_hidden: false,
            parent: Some(group),
            blend_mode: BlendMode::Normal,
            track_matte: None,
            masks: vec![path_mask],
            active_range: range,
            effects: masked,
            transform: identity_transform(),
        }))
        .map_err(|error| error.to_string())?,
        Layer::from_data(&LayerData::Shape(shape_guide(
            guide,
            "Premiere effect mask".into(),
            Some(group),
            range,
            identity_transform(),
            outline,
        )))
        .map_err(|error| error.to_string())?,
    ];
    *scope.next_index += 3;
    approximate(&mut notes, clip.record(), APPROXIMATION);
    omissions.extend(notes);
    Ok(Imported {
        prefix,
        suffix,
        children,
        tracks,
    })
}

/// A recognized scope always owns one video, one adjustment and its one guide.
/// Private fields prevent other export paths from constructing an unchecked scope.
#[derive(Clone, Copy)]
pub(super) struct EffectScope<'a> {
    group: &'a GroupLayer,
    video: &'a Layer,
    adjustment: &'a AdjustmentLayer,
    canvas: [u32; 2],
}

impl<'a> EffectScope<'a> {
    pub(super) fn group(self) -> &'a GroupLayer {
        self.group
    }
    pub(super) fn video(self) -> &'a Layer {
        self.video
    }

    /// `None` means unrelated; `Err` means a scope-shaped group cannot export
    /// atomically. Callers must not route the latter through generic nests.
    pub(super) fn recognize(
        group: &'a GroupLayer,
        dynamics: &AnimationGraph,
        canvas: [u32; 2],
    ) -> Result<Option<Self>, String> {
        let adjustment = group.layers.iter().find_map(|layer| match layer.data() {
            LayerData::Adjustment(adjustment) if !adjustment.masks.is_empty() => Some(adjustment),
            _ => None,
        });
        let video = group
            .layers
            .iter()
            .find(|layer| matches!(super::video_data(layer), Ok(Some(_))));
        let (Some(adjustment), Some(video)) = (adjustment, video) else {
            return Ok(None);
        };
        // Keep the existing measured rectangle-adjustment path. A path-mask
        // scope (or a broken one) must not fall through to a nest that drops it.
        if super::adjustment::unexported_reason(adjustment, &group.layers, dynamics, canvas)
            .is_none()
        {
            return Ok(None);
        }
        let scope = Self {
            group,
            video,
            adjustment,
            canvas,
        };
        scope.validate(dynamics)?;
        Ok(Some(scope))
    }

    fn validate(self, dynamics: &AnimationGraph) -> Result<(), String> {
        let Self {
            group,
            video: layer,
            adjustment,
            canvas,
        } = self;
        let video = super::video_data(layer)
            .map_err(|error| error.to_string())?
            .ok_or("effect scope has no video")?;
        let range = TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration);
        let source =
            timing::linear_source_range(&video.playback).map_err(|error| error.to_string())?;
        let input = video.playback.input_range();
        let owner_keys = |id| layer_animations(dynamics, id).next().is_some();
        let invalid = [
            (
                group.layers.len() != 3,
                "effect scope must contain exactly one video, adjustment and guide",
            ),
            (
                !timing::is_plain_group_playback(&group.playback),
                "effect scope has a nonidentity content clock",
            ),
            (
                has_background(group) || !group.masks.is_empty() || group.track_matte.is_some(),
                "effect scope has extra coverage",
            ),
            (
                video.parent != Some(group.id) || adjustment.parent != Some(group.id),
                "effect scope children have another parent",
            ),
            (
                video.transform != identity_transform()
                    || adjustment.transform != identity_transform(),
                "effect scope children have independent transforms or opacity",
            ),
            (
                video.is_hidden
                    || adjustment.is_hidden
                    || video.blend_mode != BlendMode::Normal
                    || adjustment.blend_mode != BlendMode::Normal,
                "effect scope children have independent visibility or blend modes",
            ),
            (
                !video.masks.is_empty()
                    || video.track_matte.is_some()
                    || adjustment.track_matte.is_some()
                    || video.motion_blur,
                "effect scope children have extra masks, mattes or motion blur",
            ),
            (
                input != range
                    || adjustment.active_range != range
                    || source.duration != range.duration
                    || video.playback.input_offset_ms() != 0,
                "effect scope children must share the full local unit-speed clock",
            ),
            (
                video.source_range != source
                    || video.start_time.is_some()
                    || video.placement.is_some()
                    || video.corner_radius.is_some(),
                "effect scope video has independent source placement or coverage",
            ),
            (
                owner_keys(video.id) || owner_keys(adjustment.id),
                "effect scope children have independent layer keys",
            ),
            (
                adjustment.effects.len() != 1 || adjustment.masks.len() != 1,
                "effect scope adjustment must own exactly one effect and mask",
            ),
        ];
        if let Some((_, reason)) = invalid.into_iter().find(|(invalid, _)| *invalid) {
            return Err(reason.into());
        }
        if source_frame(video.source.frame_rect).map_err(|error| error.to_string())? != canvas {
            return Err("effect scope source frame must match the sequence canvas".into());
        }
        let position = |id| group.layers.iter().position(|layer| layer.id() == id);
        if position(adjustment.id) >= position(video.id) {
            return Err("effect scope adjustment must be above its video".into());
        }
        self.native_mask(dynamics, 0)?;
        // Export admission and media discovery use the same native-effect check.
        self.export_tail(dynamics, 0)?;
        Ok(())
    }

    fn native_mask(self, dynamics: &AnimationGraph, source_in: i64) -> Result<PrMask, String> {
        let adjustment = self.adjustment;
        let mut mask = graphic_mask(
            &adjustment.masks,
            &self.group.layers,
            ("effect", Some(self.group.id), adjustment.active_range),
            dynamics,
            self.canvas,
        )?
        .ok_or("effect scope has no mask")?;
        mask_animation::export_tracks(&mut mask, adjustment.masks[0].id, dynamics, source_in)?;
        Ok(mask)
    }

    /// Return current edited native values and provisional key accounting.
    /// The caller commits the accounting only when it retains the whole clip.
    pub(super) fn export_tail(
        self,
        dynamics: &AnimationGraph,
        source_in: i64,
    ) -> Result<(Vec<PrEffect>, WrittenAnimation, Vec<Omission>), String> {
        let mut written = WrittenAnimation::default();
        let mut notes = Vec::new();
        let transform = identity_transform();
        let host = |layer| EffectHost {
            layer,
            still: false,
            staged: false,
            nested: false,
            in_nest: false,
            transform: &transform,
            source_in,
            video_keys: None,
            static_parameters_reason: None,
            frame: self.canvas,
            canvas: self.canvas,
        };
        let video = super::video_data(self.video)
            .map_err(|error| error.to_string())?
            .ok_or("effect scope has no video")?;
        // Validate the prefix here too, so discovery cannot admit a scope that
        // only its later video export would reject. Its accounting stays with
        // that export, not this provisional tail.
        export_segment(
            &video.effects,
            dynamics,
            host(video.id),
            &mut WrittenAnimation::default(),
            "effect-mask prefix",
            &mut Vec::new(),
        )?;
        let mut masked = export_segment(
            &self.adjustment.effects,
            dynamics,
            host(self.adjustment.id),
            &mut written,
            "effect mask",
            &mut notes,
        )?;
        if masked.len() != 1 {
            return Err(format!(
                "masked effect cannot be exported atomically: {notes:?}"
            ));
        }
        let mask = self.native_mask(dynamics, source_in)?;
        for warning in mask.approximations() {
            approximate(&mut notes, "effect mask", warning);
        }
        written.record_mask(self.adjustment.masks[0].id, &mask);
        masked[0].mask = Some(mask);
        let suffix = export_segment(
            &self.group.effects,
            dynamics,
            host(self.group.id),
            &mut written,
            "effect-mask suffix",
            &mut notes,
        )?;
        masked.extend(suffix);
        approximate(
            &mut notes,
            format!("layer {}", self.group.id),
            APPROXIMATION,
        );
        Ok((masked, written, notes))
    }
}

fn export_segment(
    effects: &[EffectRecord],
    dynamics: &AnimationGraph,
    host: EffectHost<'_>,
    written: &mut WrittenAnimation,
    record: &str,
    notes: &mut Vec<Omission>,
) -> Result<Vec<PrEffect>, String> {
    let first_note = notes.len();
    let converted = effects::export_effects(effects, dynamics, host, written, record, notes);
    if converted.iter().any(|effect| !supports_effect(effect)) {
        return Err(
            "effect mask scopes do not support Transform, temporal or coverage-changing effects"
                .into(),
        );
    }
    if notes[first_note..]
        .iter()
        .any(|note| note.kind == OmissionKind::Omitted)
    {
        return Err(format!(
            "{record} cannot be exported atomically: {:?}",
            &notes[first_note..]
        ));
    }
    Ok(converted)
}
