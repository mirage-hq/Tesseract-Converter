//! Fresh source-less native Shape Layer containing one parametric Rectangle.
//! The AE26 record layout is experimental until Adobe opens nonempty output.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    rifx::Chunk,
    schema::{CompositionRecord, layer_records::LayerRecord},
    timing::Duration24,
};
use fx_schema::{
    LayerId,
    layer::{ShapeFillRule, ShapeLineCap, ShapeLineJoin},
};

use super::{AepWriteError, NativeLayerOptions, StrokeDashes, layer_options, root, solids, views};
#[cfg(test)]
use super::{CompositionSpec, checked_duration};
use solids::{SolidLayerSpec, SolidTransform, TransformAnimations};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RectAnimations {
    pub transform: TransformAnimations,
    pub size: Option<super::NumericTrack>,
    pub position: Option<super::NumericTrack>,
    pub roundness: Option<super::NumericTrack>,
    pub stroke: super::StrokeAnimations,
}
use views::ValueKind;

/// A full-span, singly painted editable native vector Rectangle.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VectorRectSpec {
    pub name: String,
    pub stroke_dashes: StrokeDashes,
    pub size: [f64; 2],
    /// Rectangle's own center position in the Shape Layer's local coordinates.
    pub position: [f64; 2],
    pub roundness: f64,
    pub fill_color: Option<[f64; 4]>,
    pub stroke_color: Option<[f64; 4]>,
    pub stroke_width: f64,
    pub stroke_join: ShapeLineJoin,
    pub stroke_miter_limit: f64,
    pub transform: SolidTransform,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VectorAppearance {
    pub name: String,
    pub stroke_dashes: StrokeDashes,
    /// Native paint percentage, independent of Transform opacity and RGBA alpha.
    pub paint_opacity: f64,
    pub fill_color: Option<[f64; 4]>,
    pub fill_rule: ShapeFillRule,
    pub stroke_color: Option<[f64; 4]>,
    pub stroke_cap: ShapeLineCap,
    pub stroke_width: f64,
    pub stroke_join: ShapeLineJoin,
    pub stroke_miter_limit: f64,
    pub transform: SolidTransform,
}

impl From<&VectorRectSpec> for VectorAppearance {
    fn from(rect: &VectorRectSpec) -> Self {
        Self {
            name: rect.name.clone(),
            stroke_dashes: rect.stroke_dashes.clone(),
            paint_opacity: 100.0,
            fill_color: rect.fill_color,
            fill_rule: ShapeFillRule::NonZeroWinding,
            stroke_color: rect.stroke_color,
            stroke_cap: ShapeLineCap::Butt,
            stroke_width: rect.stroke_width,
            stroke_join: rect.stroke_join,
            stroke_miter_limit: rect.stroke_miter_limit,
            transform: rect.transform.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerTiming {
    pub start_millis: i64,
    pub end_millis: i64,
}

/// One fresh source-backed Null transform carrier.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NullLayerSpec {
    pub name: String,
    pub transform: SolidTransform,
    pub transform_animations: TransformAnimations,
}

/// One fresh nested composition and its source-backed AV occurrence.
///
/// `composition_record` is optional so the composition-options lowering can
/// provide a checked record carrying current motion-blur settings. `None`
/// retains the fixed AE26 24fps/square-pixel defaults.
pub(crate) struct PrecompositionSpec {
    pub name: String,
    pub collapse_transformations: bool,
    pub width: u16,
    pub height: u16,
    pub duration: Duration24,
    pub transform: SolidTransform,
    pub transform_animations: TransformAnimations,
    pub layers: Vec<LayerSpec>,
    pub composition_record: Option<CompositionRecord>,
}

pub(crate) enum LayerSpec {
    Options(Box<LayerSpec>, NativeLayerOptions),
    Timed(Box<LayerSpec>, LayerTiming),
    Solid(SolidLayerSpec),
    AnimatedSolid(SolidLayerSpec, TransformAnimations),
    Rect(VectorRectSpec),
    AnimatedRect(VectorRectSpec, RectAnimations),
    // Legacy direct-shape layers remain available to writer compatibility tests.
    #[cfg_attr(not(test), allow(dead_code))]
    Shape(Box<super::VectorShapeSpec>),
    #[cfg_attr(not(test), allow(dead_code))]
    Boolean(super::VectorBooleanSpec),
    VectorProgram(super::VectorLayerSpec),
    Footage(super::footage::FootageSpec, TransformAnimations),
    Text(super::text::TextSpec),
    Null(NullLayerSpec),
    Camera(super::NativeCameraSpec),
    Precomposition(PrecompositionSpec),
}

/// Composition-local identity and provider edges carried by one emitted root.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayerReferenceFacts {
    /// Current FX identity assigned to this emitted composition-local root.
    pub layer_id: Option<LayerId>,
    /// Optional transform-parent provider in the same native composition.
    pub parent: Option<LayerId>,
    /// Optional track-matte provider in the same native composition.
    pub matte: Option<LayerId>,
}

impl LayerSpec {
    pub(crate) fn envelope(
        &self,
    ) -> Result<(&Self, Option<&LayerTiming>, Option<&NativeLayerOptions>), AepWriteError> {
        let mut layer = self;
        let mut timing = None;
        let mut options = None;
        loop {
            match layer {
                Self::Timed(inner, value) if timing.is_none() => {
                    timing = Some(value);
                    layer = inner;
                }
                Self::Options(inner, value) if options.is_none() => {
                    options = Some(value);
                    layer = inner;
                }
                Self::Timed(_, _) | Self::Options(_, _) => {
                    return Err(AepWriteError::Invalid("duplicate native layer envelope"));
                }
                _ => return Ok((layer, timing, options)),
            }
        }
    }

    /// Returns only composition-local identity edges. Nested compositions keep
    /// their own strict validation boundary and are not flattened into this set.
    pub(crate) fn local_reference_facts(&self) -> Result<LayerReferenceFacts, AepWriteError> {
        let (_, _, options) = self.envelope()?;
        Ok(
            options.map_or_else(LayerReferenceFacts::default, |options| {
                LayerReferenceFacts {
                    layer_id: Some(options.fx_id),
                    parent: options.parent,
                    matte: options.matte.map(|matte| matte.layer),
                }
            }),
        )
    }

    fn collect_emitted_media_paths(&self, paths: &mut BTreeSet<super::footage::RelativeMediaPath>) {
        match self {
            Self::Options(inner, _) | Self::Timed(inner, _) => {
                inner.collect_emitted_media_paths(paths);
            }
            Self::Footage(spec, _) => {
                paths.insert(spec.source.path.clone());
            }
            Self::Precomposition(spec) => {
                for layer in &spec.layers {
                    layer.collect_emitted_media_paths(paths);
                }
            }
            Self::Solid(_)
            | Self::AnimatedSolid(_, _)
            | Self::Rect(_)
            | Self::AnimatedRect(_, _)
            | Self::Shape(_)
            | Self::Boolean(_)
            | Self::VectorProgram(_)
            | Self::Text(_)
            | Self::Null(_)
            | Self::Camera(_) => {}
        }
    }

    /// Whether this emitted root, rather than only its nested source, uses
    /// the composition's perspective camera.
    pub(crate) fn has_three_d_root(&self) -> Result<bool, AepWriteError> {
        let (_, _, options) = self.envelope()?;
        Ok(options.is_some_and(|value| {
            value
                .transform_3d
                .as_ref()
                .is_some_and(|(transform, _)| transform.is_three_d)
        }))
    }

    /// Assigns a preallocated native transform parent to one emitted root.
    pub(crate) fn assign_root_parent(&mut self, parent: LayerId) -> Result<(), AepWriteError> {
        // A flattened child batch can contain its own Null hierarchy. Only its
        // roots acquire the outer parent; descendants keep their existing chain.
        let (_, _, options) = self.envelope()?;
        if options.is_some_and(|value| value.parent.is_some()) {
            return Ok(());
        }
        let mut layer = self;
        loop {
            match layer {
                Self::Options(_, options) => {
                    *options = options.clone().with_parent(parent)?;
                    return Ok(());
                }
                Self::Timed(inner, _) => layer = inner,
                _ => {
                    return Err(AepWriteError::Invalid(
                        "native parent assignment requires typed layer options",
                    ));
                }
            }
        }
    }

    /// In the consumer-only 3D viewport, world-space 3D roots must remain
    /// bit-for-bit stationary. Only planar roots follow the changed principal
    /// point; moving a world coordinate changes AE's f32 projection rounding.
    pub(crate) fn translate_planar_composition_root(
        &mut self,
        offset: [f64; 2],
    ) -> Result<(), AepWriteError> {
        if self.has_three_d_root()? || matches!(self.envelope()?.0, Self::Camera(_)) {
            return Ok(());
        }
        self.translate_composition_root(offset)
    }

    /// Translates one composition-root layer. A layer already parented to an
    /// emitted transform carrier stays in that parent's coordinate system and
    /// must not receive the precomposition-origin shift a second time.
    pub(crate) fn translate_composition_root(
        &mut self,
        offset: [f64; 2],
    ) -> Result<(), AepWriteError> {
        let (_, _, options) = self.envelope()?;
        if options.is_some_and(|value| value.parent.is_some()) {
            return Ok(());
        }
        translate_layer(self, offset)
    }
}

/// Exact package-relative paths referenced by retained native footage layers.
pub(crate) fn emitted_media_paths(
    layers: &[LayerSpec],
) -> BTreeSet<super::footage::RelativeMediaPath> {
    let mut paths = BTreeSet::new();
    for layer in layers {
        layer.collect_emitted_media_paths(&mut paths);
    }
    paths
}

fn translate_layer(layer: &mut LayerSpec, offset: [f64; 2]) -> Result<(), AepWriteError> {
    match layer {
        LayerSpec::Options(inner, options) => {
            if let Some((transform, animations)) = &mut options.transform_3d {
                transform.position[0] += offset[0];
                transform.position[1] += offset[1];
                if let Some(track) = &mut animations.position {
                    translate_numeric_track(track, &[offset[0], offset[1], 0.0])?;
                }
                if let Some(tracks) = &mut animations.position_separated {
                    for (track, delta) in tracks.iter_mut().zip(offset) {
                        if let Some(track) = track {
                            translate_numeric_track(track, &[delta])?;
                        }
                    }
                }
            }
            translate_layer(inner, offset)
        }
        LayerSpec::Timed(inner, _) => translate_layer(inner, offset),
        LayerSpec::Camera(camera) => camera.translate(offset),
        LayerSpec::Solid(spec) => translate_transform(&mut spec.transform, None, offset),
        LayerSpec::AnimatedSolid(spec, animations) => {
            translate_transform(&mut spec.transform, Some(animations), offset)
        }
        LayerSpec::Rect(spec) => translate_transform(&mut spec.transform, None, offset),
        LayerSpec::AnimatedRect(spec, animations) => {
            translate_transform(&mut spec.transform, Some(&mut animations.transform), offset)
        }
        LayerSpec::Shape(spec) => translate_transform(
            &mut spec.appearance.transform,
            Some(&mut spec.animations),
            offset,
        ),
        LayerSpec::Boolean(spec) => translate_transform(
            &mut spec.appearance.transform,
            Some(&mut spec.animations),
            offset,
        ),
        LayerSpec::VectorProgram(spec) => translate_transform(
            &mut spec.transform,
            Some(&mut spec.transform_animations),
            offset,
        ),
        LayerSpec::Footage(spec, animations) => {
            translate_transform(&mut spec.transform.transform, Some(animations), offset)
        }
        LayerSpec::Text(spec) => translate_transform(
            &mut spec.transform,
            Some(&mut spec.transform_animations),
            offset,
        ),
        LayerSpec::Null(spec) => translate_transform(
            &mut spec.transform,
            Some(&mut spec.transform_animations),
            offset,
        ),
        LayerSpec::Precomposition(spec) => translate_transform(
            &mut spec.transform,
            Some(&mut spec.transform_animations),
            offset,
        ),
    }
}

/// Translate authored values only; temporal curves and relative spatial tangents
/// are invariant under a change of composition origin.
pub(crate) fn translate_numeric_track(
    track: &mut super::NumericTrack,
    offset: &[f64],
) -> Result<(), AepWriteError> {
    if track.keys.iter().any(|key| {
        key.values.len() != offset.len()
            || key
                .values
                .iter()
                .zip(offset)
                .any(|(value, delta)| !(value + delta).is_finite())
    }) {
        return Err(AepWriteError::Invalid(
            "Invalid translated native keyframe values",
        ));
    }
    for key in &mut track.keys {
        for (value, delta) in key.values.iter_mut().zip(offset) {
            *value += delta;
        }
    }
    Ok(())
}

fn translate_transform(
    transform: &mut SolidTransform,
    animations: Option<&mut TransformAnimations>,
    offset: [f64; 2],
) -> Result<(), AepWriteError> {
    for (value, delta) in transform.position.iter_mut().zip(offset) {
        *value += delta;
        if !value.is_finite() {
            return Err(AepWriteError::Invalid(
                "translated native position is non-finite",
            ));
        }
    }
    if let Some(track) = animations.and_then(|value| value.position.as_mut()) {
        for key in &mut track.keys {
            if key.values.len() < 2 {
                return Err(AepWriteError::Invalid(
                    "native position key has fewer than two dimensions",
                ));
            }
            for (value, delta) in key.values.iter_mut().zip(offset) {
                *value += delta;
                if !value.is_finite() {
                    return Err(AepWriteError::Invalid(
                        "translated native position key is non-finite",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn ticks(millis: i64) -> Result<i32, AepWriteError> {
    let ticks = (i128::from(millis) * 24_576 + 500) / 1_000;
    i32::try_from(ticks)
        .map_err(|_| AepWriteError::Invalid("native layer time exceeds signed tick range"))
}

pub(super) fn checked_timing(
    timing: LayerTiming,
    duration: Duration24,
) -> Result<(i32, i32), AepWriteError> {
    let start = ticks(timing.start_millis)?;
    let end = ticks(timing.end_millis)?;
    if start < 0 || end <= start || end > duration.signed_ticks() {
        return Err(AepWriteError::Invalid(
            "native layer active range is outside the composition",
        ));
    }
    Ok((start, end))
}

fn disable_native_audio_switch(layer: &mut Chunk) -> Result<(), AepWriteError> {
    let records = layer.children_mut().ok_or(AepWriteError::Invalid(
        "native timeline layer is not a LIST",
    ))?;
    let record = records
        .iter_mut()
        .find(|child| child.id() == *b"ldta")
        .ok_or(AepWriteError::Invalid("native timeline layer has no ldta"))?;
    let bytes = record
        .data_payload()
        .ok_or(AepWriteError::Invalid("invalid native ldta"))?;
    let updated = LayerRecord::decode(bytes)?.with_audio_enabled(false)?;
    *record = Chunk::data(*b"ldta", updated.encode())?;
    Ok(())
}

fn apply_timing(
    layer: &mut Chunk,
    timing: LayerTiming,
    duration: Duration24,
) -> Result<(), AepWriteError> {
    let (start, end) = checked_timing(timing, duration)?;
    let Some(records) = layer.children_mut() else {
        return Err(AepWriteError::Invalid(
            "native timeline layer is not a LIST",
        ));
    };
    let Some(record) = records.iter_mut().find(|child| child.id() == *b"ldta") else {
        return Err(AepWriteError::Invalid("native timeline layer has no ldta"));
    };
    let bytes = record
        .data_payload()
        .ok_or(AepWriteError::Invalid("invalid native ldta"))?;
    let updated = LayerRecord::decode(bytes)?.with_active_range(start, end - start)?;
    *record = Chunk::data(*b"ldta", updated.encode())?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn write_composition(
    spec: &CompositionSpec,
    layers: &[LayerSpec],
) -> Result<Vec<u8>, AepWriteError> {
    write_composition_at_rate(
        &spec.name,
        spec.width,
        spec.height,
        layers,
        crate::timing::FrameRate::new(24.0)?,
        checked_duration(spec)?,
        None,
    )
}

pub(crate) fn write_composition_at_rate(
    name: &str,
    width: u16,
    height: u16,
    layers: &[LayerSpec],
    frame_rate: crate::timing::FrameRate,
    duration: Duration24,
    composition_options: Option<super::CompositionOptions>,
) -> Result<Vec<u8>, AepWriteError> {
    write_composition_at_rate_with_audio_policy(
        name,
        [width, height],
        layers,
        frame_rate,
        duration,
        composition_options,
        AudioSwitchPolicy::Preserve,
    )
}

pub(crate) fn write_picture_only_composition_at_rate(
    name: &str,
    width: u16,
    height: u16,
    layers: &[LayerSpec],
    frame_rate: crate::timing::FrameRate,
    duration: Duration24,
    composition_options: Option<super::CompositionOptions>,
) -> Result<Vec<u8>, AepWriteError> {
    write_composition_at_rate_with_audio_policy(
        name,
        [width, height],
        layers,
        frame_rate,
        duration,
        composition_options,
        AudioSwitchPolicy::Disable,
    )
}

fn write_composition_at_rate_with_audio_policy(
    name: &str,
    composition_size: [u16; 2],
    layers: &[LayerSpec],
    frame_rate: crate::timing::FrameRate,
    duration: Duration24,
    composition_options: Option<super::CompositionOptions>,
    audio_switch_policy: AudioSwitchPolicy,
) -> Result<Vec<u8>, AepWriteError> {
    let [width, height] = composition_size;
    if name.len() > 65_535 {
        return Err(AepWriteError::Invalid("name exceeds 65535 UTF-8 bytes"));
    }
    let timeline = build_timeline_at_rate_with_audio_policy(
        layers,
        duration,
        [width, height],
        frame_rate,
        audio_switch_policy,
    )?;
    let views = views::build_views_with_clock(
        width,
        height,
        duration,
        super::keyframes::PropertyClock::for_rate(frame_rate)?,
    )?;
    let bytes = root::build_project_with_timeline_options(
        name,
        width,
        height,
        duration,
        views,
        timeline,
        composition_options,
        frame_rate,
    )?
    .encode()?;
    Ok(bytes)
}

/// Exercises one layer's native payload and envelope encoder without requiring
/// its composition-local parent or matte target to be present in the probe.
/// Nested precompositions remain complete composition boundaries and retain
/// full reference validation.
pub(crate) fn validate_layer_payload(
    layer: &LayerSpec,
    duration: Duration24,
) -> Result<(), AepWriteError> {
    build_timeline_with_scope(
        std::slice::from_ref(layer),
        duration,
        ReferenceScope::PayloadOnly,
        [1, 1],
    )
    .map(|_| ())
}

/// Exercises a complete composition-local batch, including parent and matte
/// reference integrity. No project serialization or publication happens.
pub(crate) fn validate_layers(
    layers: &[LayerSpec],
    duration: Duration24,
) -> Result<(), AepWriteError> {
    build_timeline(layers, duration, [1, 1]).map(|_| ())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReferenceScope {
    Full,
    PayloadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioSwitchPolicy {
    Preserve,
    Disable,
}

struct TimelinePlan {
    layers: Vec<LayerPlan>,
}

struct LayerPlan {
    source_id: Option<u32>,
    layer_id: u32,
    timing: Option<LayerTiming>,
    options: Option<NativeLayerOptions>,
    nested: Option<Box<TimelinePlan>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NativeSourceKey {
    path: super::footage::RelativeMediaPath,
    format: u8,
    dimensions: [u16; 2],
    duration_millis: u64,
    frame_rate_integer: u32,
    frame_rate_fractional: u16,
    audio_sample_rate_bits: u64,
    wave_metadata: Option<(u32, u32)>,
}

impl From<&super::footage::NativeSource> for NativeSourceKey {
    fn from(source: &super::footage::NativeSource) -> Self {
        let format = match source.format {
            super::footage::NativeSourceFormat::OpenExr => 0,
            super::footage::NativeSourceFormat::Wave => 1,
            super::footage::NativeSourceFormat::QuickTime => 2,
        };
        Self {
            path: source.path.clone(),
            format,
            dimensions: source.dimensions,
            duration_millis: source.duration_millis,
            frame_rate_integer: source.frame_rate.integer,
            frame_rate_fractional: source.frame_rate.fractional,
            // Validation rejects non-finite rates before planning. For finite
            // f64 values, canonicalizing signed zero preserves `PartialEq`.
            audio_sample_rate_bits: if source.audio_sample_rate == 0.0 {
                0.0_f64.to_bits()
            } else {
                source.audio_sample_rate.to_bits()
            },
            wave_metadata: source
                .wave_metadata
                .map(|metadata| (metadata.sample_frames, metadata.file_length)),
        }
    }
}

struct ReservationState {
    next_id: u32,
    footage_sources: BTreeMap<NativeSourceKey, u32>,
}

impl ReservationState {
    fn allocate(&mut self) -> Result<u32, AepWriteError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(AepWriteError::Invalid("native ID overflow"))?;
        Ok(id)
    }
}

struct EmissionState {
    sources: Vec<Chunk>,
    emitted_source_ids: BTreeSet<u32>,
    // One policy covers every emitted composition and source variant.
    audio_switch_policy: AudioSwitchPolicy,
}

fn build_timeline(
    layers: &[LayerSpec],
    duration: Duration24,
    composition_size: [u16; 2],
) -> Result<root::Timeline, AepWriteError> {
    build_timeline_at_rate(
        layers,
        duration,
        composition_size,
        crate::timing::FrameRate::new(24.0)?,
    )
}

fn build_timeline_at_rate(
    layers: &[LayerSpec],
    duration: Duration24,
    composition_size: [u16; 2],
    frame_rate: crate::timing::FrameRate,
) -> Result<root::Timeline, AepWriteError> {
    build_timeline_at_rate_with_audio_policy(
        layers,
        duration,
        composition_size,
        frame_rate,
        AudioSwitchPolicy::Preserve,
    )
}

fn build_timeline_at_rate_with_audio_policy(
    layers: &[LayerSpec],
    duration: Duration24,
    composition_size: [u16; 2],
    frame_rate: crate::timing::FrameRate,
    audio_switch_policy: AudioSwitchPolicy,
) -> Result<root::Timeline, AepWriteError> {
    build_timeline_with_scope_at_rate(
        layers,
        duration,
        ReferenceScope::Full,
        composition_size,
        frame_rate,
        audio_switch_policy,
    )
}

fn build_timeline_with_scope(
    layers: &[LayerSpec],
    duration: Duration24,
    scope: ReferenceScope,
    composition_size: [u16; 2],
) -> Result<root::Timeline, AepWriteError> {
    build_timeline_with_scope_at_rate(
        layers,
        duration,
        scope,
        composition_size,
        crate::timing::FrameRate::new(24.0)?,
        AudioSwitchPolicy::Preserve,
    )
}

fn build_timeline_with_scope_at_rate(
    layers: &[LayerSpec],
    duration: Duration24,
    scope: ReferenceScope,
    composition_size: [u16; 2],
    frame_rate: crate::timing::FrameRate,
    audio_switch_policy: AudioSwitchPolicy,
) -> Result<root::Timeline, AepWriteError> {
    let first_id = root::Timeline::default().next_id;
    let mut reservations = ReservationState {
        next_id: first_id,
        footage_sources: BTreeMap::new(),
    };
    let frame_blending_master = requires_frame_blending_master(layers)?;
    let plan = reserve_timeline(layers, duration, 0, scope, &mut reservations)?;
    let mut emission = EmissionState {
        sources: Vec::new(),
        emitted_source_ids: BTreeSet::new(),
        audio_switch_policy,
    };
    let output_layers = emit_timeline(
        layers,
        &plan,
        duration,
        scope,
        composition_size,
        frame_rate,
        &mut emission,
    )?;
    Ok(root::Timeline {
        sources: emission.sources,
        layers: output_layers,
        next_id: reservations.next_id,
        frame_blending_master,
    })
}

fn requires_frame_blending_master(layers: &[LayerSpec]) -> Result<bool, AepWriteError> {
    for spec in layers {
        let (layer, _, _) = spec.envelope()?;
        let required = match layer {
            LayerSpec::Footage(footage, _) => footage.frame_blending.requires_composition_master(),
            LayerSpec::Precomposition(precomposition) => {
                requires_frame_blending_master(&precomposition.layers)?
            }
            _ => false,
        };
        if required {
            return Ok(true);
        }
    }
    Ok(false)
}

fn reserve_timeline(
    layers: &[LayerSpec],
    duration: Duration24,
    depth: usize,
    scope: ReferenceScope,
    state: &mut ReservationState,
) -> Result<TimelinePlan, AepWriteError> {
    if depth >= 48 {
        return Err(AepWriteError::Invalid(
            "native precomposition nesting limit is 48",
        ));
    }
    let mut plans = Vec::with_capacity(layers.len());
    for spec in layers {
        let (layer, timing, options) = spec.envelope()?;
        if let Some(timing) = timing {
            checked_timing(*timing, duration)?;
        }
        if options.is_some_and(|value| value.source_clock.is_some()) {
            if timing.is_some() {
                return Err(AepWriteError::Invalid(
                    "source-clock occurrence cannot also use a generic timed envelope",
                ));
            }
            if !matches!(layer, LayerSpec::Precomposition(_)) {
                return Err(AepWriteError::Invalid(
                    "typed source clock is only valid on a precomposition occurrence",
                ));
            }
        }
        validate_base_layer(layer, duration)?;
        let mut nested = None;
        let source_id = match layer {
            LayerSpec::Solid(_) | LayerSpec::AnimatedSolid(_, _) | LayerSpec::Null(_) => {
                Some(state.allocate()?)
            }
            LayerSpec::Footage(footage, _) => {
                let key = NativeSourceKey::from(&footage.source);
                Some(match state.footage_sources.get(&key).copied() {
                    Some(id) => id,
                    None => {
                        let id = state.allocate()?;
                        state.footage_sources.insert(key, id);
                        id
                    }
                })
            }
            LayerSpec::Precomposition(precomposition) => {
                let id = state.allocate()?;
                nested = Some(Box::new(reserve_timeline(
                    &precomposition.layers,
                    precomposition.duration,
                    depth + 1,
                    ReferenceScope::Full,
                    state,
                )?));
                Some(id)
            }
            _ => None,
        };
        plans.push(LayerPlan {
            source_id,
            layer_id: state.allocate()?,
            timing: timing.copied(),
            options: options.cloned(),
            nested,
        });
    }
    let mut fx_ids = BTreeMap::new();
    for plan in &plans {
        if let Some(options) = &plan.options
            && fx_ids.insert(options.fx_id, plan.layer_id).is_some()
        {
            return Err(AepWriteError::Invalid(
                "duplicate FX layer identity in native composition plan",
            ));
        }
    }
    if scope == ReferenceScope::Full {
        for plan in &plans {
            if let Some(options) = &plan.options {
                for target in options
                    .parent
                    .into_iter()
                    .chain(options.matte.map(|matte| matte.layer))
                {
                    if !fx_ids.contains_key(&target) {
                        return Err(AepWriteError::Invalid(
                            "native layer reference crosses or escapes its composition",
                        ));
                    }
                }
            }
        }
    }
    Ok(TimelinePlan { layers: plans })
}

fn validate_base_layer(layer: &LayerSpec, duration: Duration24) -> Result<(), AepWriteError> {
    match layer {
        LayerSpec::Solid(solid) | LayerSpec::AnimatedSolid(solid, _) => solids::validate(solid),
        LayerSpec::Rect(rect) | LayerSpec::AnimatedRect(rect, _) => validate(rect),
        LayerSpec::Shape(shape) => super::shapes::validate(shape),
        LayerSpec::Boolean(boolean) => super::shapes::validate_boolean(boolean),
        LayerSpec::VectorProgram(program) => super::shapes::validate_program(program),
        LayerSpec::Footage(footage, _) => super::footage::validate(footage, duration),
        LayerSpec::Text(text) => super::text::validate(text),
        LayerSpec::Null(null) => validate_null(null),
        LayerSpec::Camera(camera) => camera.validate(),
        LayerSpec::Precomposition(precomposition) => validate_precomposition(precomposition),
        LayerSpec::Timed(_, _) | LayerSpec::Options(_, _) => {
            Err(AepWriteError::Invalid("nested native layer envelope"))
        }
    }
}

fn validate_null(spec: &NullLayerSpec) -> Result<(), AepWriteError> {
    validate_hierarchy_transform(&spec.name, &spec.transform)
}

fn validate_precomposition(spec: &PrecompositionSpec) -> Result<(), AepWriteError> {
    if spec.width == 0 || spec.height == 0 {
        return Err(AepWriteError::Invalid(
            "zero native precomposition dimension",
        ));
    }
    validate_hierarchy_transform(&spec.name, &spec.transform)?;
    if let Some(record) = &spec.composition_record
        && (record.dimensions() != (spec.width, spec.height)
            || record.frame_rate() != 24.0
            || record.pixel_aspect_fraction() != (1, 1)
            || record.duration_fraction()? != (spec.duration.ticks(), 24_576))
    {
        return Err(AepWriteError::Invalid(
            "precomposition record disagrees with typed canvas/clock",
        ));
    }
    Ok(())
}

fn validate_hierarchy_transform(
    name: &str,
    transform: &SolidTransform,
) -> Result<(), AepWriteError> {
    if name.is_empty() || name.len() > 255 || name.contains('\0') {
        return Err(AepWriteError::Invalid(
            "hierarchy layer name must be 1..=255 UTF-8 bytes without NUL",
        ));
    }
    if transform
        .anchor
        .iter()
        .chain(&transform.position)
        .chain(&transform.scale)
        .any(|value| !value.is_finite())
        || !transform.rotation.is_finite()
        || !transform.opacity.is_finite()
        || !(0.0..=100.0).contains(&transform.opacity)
    {
        return Err(AepWriteError::Invalid("invalid native hierarchy Transform"));
    }
    Ok(())
}

fn emit_timeline(
    layers: &[LayerSpec],
    plan: &TimelinePlan,
    duration: Duration24,
    scope: ReferenceScope,
    composition_size: [u16; 2],
    frame_rate: crate::timing::FrameRate,
    state: &mut EmissionState,
) -> Result<Vec<Chunk>, AepWriteError> {
    let fx_ids: BTreeMap<_, _> = plan
        .layers
        .iter()
        .filter_map(|entry| {
            entry
                .options
                .as_ref()
                .map(|value| (value.fx_id, entry.layer_id))
        })
        .collect();
    let mut output_layers = Vec::with_capacity(layers.len() * 16);
    for (spec, planned) in layers.iter().zip(&plan.layers) {
        let (layer, _, _) = spec.envelope()?;
        let mut output = emit_layer(layer, planned, duration, frame_rate, state)?;
        if let Some(options) = &planned.options {
            if let Some((transform, animations)) = &options.transform_3d {
                let mut animations = finalized_transform3d_animations(layer, options, animations)?;
                let mut transform = transform.clone();
                let source_dimensions = match layer {
                    LayerSpec::Precomposition(precomposition) => {
                        Some([precomposition.width, precomposition.height])
                    }
                    LayerSpec::Null(null) => {
                        let source = null_source(null);
                        Some([source.width, source.height])
                    }
                    _ => None,
                };
                if let Some(dimensions) = source_dimensions {
                    normalize_av_anchor(
                        dimensions,
                        &mut transform.anchor,
                        animations.anchor.as_mut(),
                    );
                }
                super::transform3d::replace_fresh_layer_transform_with_clock(
                    &mut output,
                    &transform,
                    &animations,
                    super::keyframes::PropertyClock::for_rate(frame_rate)?,
                )?;
            }
            super::masks::apply_with_clock(
                &mut output,
                &options.masks,
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )?;
            super::effects::apply_with_clock(
                &mut output,
                &options.effects,
                effect_owner_size(layer, composition_size),
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )?;
            let mut styles = options.styles.clone();
            if let Some(clock) = &options.source_clock {
                super::layer_styles::rebase_animations(&mut styles, clock)?;
            }
            super::layer_styles::apply_with_clock(
                &mut output,
                &styles,
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )?;
            let resolve = |target| {
                fx_ids
                    .get(&target)
                    .copied()
                    .or((scope == ReferenceScope::PayloadOnly).then_some(u32::MAX))
                    .ok_or(AepWriteError::Invalid(
                        "native layer reference target was not emitted",
                    ))
            };
            let parent_id = options.parent.map(&resolve).transpose()?.unwrap_or(0);
            let matte_id = options
                .matte
                .map(|matte| resolve(matte.layer))
                .transpose()?
                .unwrap_or(0);
            layer_options::apply(&mut output, options, parent_id, matte_id)?;
        }
        if let Some(timing) = planned.timing {
            apply_timing(&mut output, timing, duration)?;
        }
        if state.audio_switch_policy == AudioSwitchPolicy::Disable {
            disable_native_audio_switch(&mut output)?;
        }
        output_layers.push(output);
        output_layers.push(Chunk::list(*b"Ewst", Vec::new()));
        output_layers.extend(root::item_envelope_tail());
        output_layers.extend(root::item_envelope_tail());
    }
    Ok(output_layers)
}

fn finalized_transform3d_animations(
    layer: &LayerSpec,
    options: &NativeLayerOptions,
    animations: &super::Transform3dAnimations,
) -> Result<super::Transform3dAnimations, AepWriteError> {
    let mut animations = animations.clone();
    let Some((clock, nonlinear_occurrence_transform)) = (match layer {
        LayerSpec::Footage(footage, _) => match &footage.clock {
            super::footage::FootageClock::Still { .. } => None,
            super::footage::FootageClock::Source(clock) => {
                Some((clock, footage.time_remap_requires_source_owned_transform))
            }
        },
        LayerSpec::Precomposition(_) => options
            .source_clock
            .as_ref()
            .map(|clock| (clock, clock.has_time_remap())),
        _ => None,
    }) else {
        return Ok(animations);
    };

    if nonlinear_occurrence_transform && transform3d_has_keys(&animations) {
        return Err(AepWriteError::Invalid(
            "native Time Remap cannot drive occurrence-owned 3D Transform keys",
        ));
    }
    for track in [
        &mut animations.anchor,
        &mut animations.position,
        &mut animations.scale,
        &mut animations.orientation,
        &mut animations.rotation_x,
        &mut animations.rotation_y,
        &mut animations.rotation_z,
        &mut animations.opacity,
    ]
    .into_iter()
    .flatten()
    {
        clock.rebase_track_times(track)?;
    }
    if let Some(followers) = &mut animations.position_separated {
        for track in followers.iter_mut().flatten() {
            clock.rebase_track_times(track)?;
        }
    }
    Ok(animations)
}

fn transform3d_has_keys(animations: &super::Transform3dAnimations) -> bool {
    animations.anchor.is_some()
        || animations.position.is_some()
        || animations
            .position_separated
            .as_ref()
            .is_some_and(|followers| followers.iter().any(Option::is_some))
        || animations.scale.is_some()
        || animations.orientation.is_some()
        || animations.rotation_x.is_some()
        || animations.rotation_y.is_some()
        || animations.rotation_z.is_some()
        || animations.opacity.is_some()
}

fn effect_owner_size(layer: &LayerSpec, composition_size: [u16; 2]) -> [f64; 2] {
    match layer {
        LayerSpec::Solid(solid) | LayerSpec::AnimatedSolid(solid, _) => {
            [f64::from(solid.width), f64::from(solid.height)]
        }
        LayerSpec::Footage(footage, _) => footage.source.dimensions.map(f64::from),
        LayerSpec::Precomposition(precomposition) => [
            f64::from(precomposition.width),
            f64::from(precomposition.height),
        ],
        LayerSpec::Rect(_)
        | LayerSpec::AnimatedRect(_, _)
        | LayerSpec::Shape(_)
        | LayerSpec::Boolean(_)
        | LayerSpec::VectorProgram(_)
        | LayerSpec::Text(_)
        | LayerSpec::Camera(_) => composition_size.map(f64::from),
        LayerSpec::Null(_) => [100.0, 100.0],
        LayerSpec::Timed(inner, _) | LayerSpec::Options(inner, _) => {
            effect_owner_size(inner, composition_size)
        }
    }
}

fn emit_layer(
    layer: &LayerSpec,
    plan: &LayerPlan,
    duration: Duration24,
    frame_rate: crate::timing::FrameRate,
    state: &mut EmissionState,
) -> Result<Chunk, AepWriteError> {
    let id = plan.layer_id;
    match layer {
        LayerSpec::Solid(solid) | LayerSpec::AnimatedSolid(solid, _) => {
            let source = required_source_id(plan)?;
            state.sources.push(solids::source_item(solid, source)?);
            let animations = match layer {
                LayerSpec::AnimatedSolid(_, value) => Some(value),
                _ => None,
            };
            solids::timeline_layer_with_clock(
                solid,
                id,
                source,
                duration,
                animations,
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )
        }
        LayerSpec::Rect(rect) | LayerSpec::AnimatedRect(rect, _) => {
            let animations = match layer {
                LayerSpec::AnimatedRect(_, value) => Some(value),
                _ => None,
            };
            timeline_rect(
                rect,
                id,
                duration,
                animations,
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )
        }
        LayerSpec::Shape(shape) => super::shapes::timeline_shape_with_clock(
            shape,
            id,
            duration,
            super::keyframes::PropertyClock::for_rate(frame_rate)?,
        ),
        LayerSpec::Boolean(boolean) => super::shapes::timeline_boolean_with_clock(
            boolean,
            id,
            duration,
            super::keyframes::PropertyClock::for_rate(frame_rate)?,
        ),
        LayerSpec::VectorProgram(program) => super::shapes::timeline_program_with_clock(
            program,
            id,
            duration,
            super::keyframes::PropertyClock::for_rate(frame_rate)?,
        ),
        LayerSpec::Footage(footage, animations) => {
            let source = required_source_id(plan)?;
            if state.emitted_source_ids.insert(source) {
                state
                    .sources
                    .push(super::footage::source_item(footage, source)?);
            }
            super::footage::timeline_layer_with_clock(
                footage,
                id,
                source,
                duration,
                (animations != &TransformAnimations::default()).then_some(animations),
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )
        }
        LayerSpec::Text(text) => super::text::timeline_layer_with_clock(
            text,
            id,
            duration,
            super::keyframes::PropertyClock::for_rate(frame_rate)?,
        ),
        LayerSpec::Camera(camera) => super::camera::timeline_layer_with_clock(
            camera,
            id,
            duration,
            super::keyframes::PropertyClock::for_rate(frame_rate)?,
        ),
        LayerSpec::Null(null) => {
            let source = required_source_id(plan)?;
            let source_spec = null_source(null);
            state
                .sources
                .push(solids::source_item(&source_spec, source)?);
            let mut transform = null.transform.clone();
            let mut animations = null.transform_animations.clone();
            normalize_av_anchor(
                [source_spec.width, source_spec.height],
                &mut transform.anchor,
                animations.anchor.as_mut(),
            );
            timeline_hierarchy_layer(
                &null.name,
                (&transform, &animations),
                LayerRecord::null_ae26(id, source, duration)?,
                None,
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )
        }
        LayerSpec::Precomposition(precomposition) => {
            let source = required_source_id(plan)?;
            let nested_plan = plan
                .nested
                .as_deref()
                .ok_or(AepWriteError::Invalid("precomposition plan is absent"))?;
            let nested_layers = emit_timeline(
                &precomposition.layers,
                nested_plan,
                precomposition.duration,
                ReferenceScope::Full,
                [precomposition.width, precomposition.height],
                frame_rate,
                state,
            )?;
            let mut record = match &precomposition.composition_record {
                Some(record) => record.clone(),
                None => CompositionRecord::empty_ae26(
                    precomposition.width,
                    precomposition.height,
                    precomposition.duration,
                )?,
            };
            record.set_frame_rate(frame_rate);
            record.set_frame_blending(requires_frame_blending_master(&precomposition.layers)?);
            state.sources.push(root::composition_item(
                source,
                &precomposition.name,
                record,
                nested_layers,
                views::build_views_with_clock(
                    precomposition.width,
                    precomposition.height,
                    precomposition.duration,
                    super::keyframes::PropertyClock::for_rate(frame_rate)?,
                )?,
            )?);
            let mut transform = precomposition.transform.clone();
            let mut animations = precomposition.transform_animations.clone();
            normalize_av_anchor(
                [precomposition.width, precomposition.height],
                &mut transform.anchor,
                animations.anchor.as_mut(),
            );
            timeline_hierarchy_layer(
                &precomposition.name,
                (&transform, &animations),
                LayerRecord::solid_ae26(id, source, duration)?
                    .with_collapse_transformations(precomposition.collapse_transformations)?,
                plan.options
                    .as_ref()
                    .and_then(|options| options.source_clock.as_ref()),
                super::keyframes::PropertyClock::for_rate(frame_rate)?,
            )
        }
        LayerSpec::Timed(_, _) | LayerSpec::Options(_, _) => {
            Err(AepWriteError::Invalid("nested native layer envelope"))
        }
    }
}

/// Null and precomposition transforms author anchors in pixels.
/// Native AV storage normalizes XY by source size, but keeps Z unchanged.
fn normalize_av_anchor(
    dimensions: [u16; 2],
    anchor: &mut [f64],
    track: Option<&mut super::NumericTrack>,
) {
    let dimensions = dimensions.map(f64::from);
    for (value, dimension) in anchor.iter_mut().zip(dimensions) {
        *value /= dimension;
    }
    if let Some(track) = track {
        for key in &mut track.keys {
            for values in [&mut key.values, &mut key.spatial_in, &mut key.spatial_out] {
                for (value, dimension) in values.iter_mut().zip(dimensions) {
                    *value /= dimension;
                }
            }
        }
    }
}

fn required_source_id(plan: &LayerPlan) -> Result<u32, AepWriteError> {
    plan.source_id.ok_or(AepWriteError::Invalid(
        "source-backed layer has no reserved source ID",
    ))
}

fn null_source(spec: &NullLayerSpec) -> SolidLayerSpec {
    SolidLayerSpec {
        name: spec.name.clone(),
        width: 100,
        height: 100,
        color: [1.0, 1.0, 1.0],
        transform: spec.transform.clone(),
    }
}

fn timeline_hierarchy_layer(
    name: &str,
    transform: (&SolidTransform, &TransformAnimations),
    record: LayerRecord,
    source_clock: Option<&super::source_clock::SourceClockPlan>,
    property_clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let (transform, animations) = transform;
    let record = match source_clock {
        Some(clock) => record.with_source_clock(clock.record)?,
        None => record,
    };

    let mut rebased_animations = animations.clone();
    if let Some(clock) = source_clock {
        if clock.has_time_remap() && transform_animations_have_keys(animations) {
            return Err(AepWriteError::Invalid(
                "native Time Remap cannot drive occurrence-owned Transform keys",
            ));
        }
        for track in [
            &mut rebased_animations.anchor,
            &mut rebased_animations.position,
            &mut rebased_animations.scale,
            &mut rebased_animations.rotation,
            &mut rebased_animations.opacity,
        ]
        .into_iter()
        .flatten()
        {
            clock.rebase_track_times(track)?;
        }
    }
    let properties = views::group(
        1,
        "",
        vec![(
            "ADBE Transform Group",
            layer_transform_with_clock(
                transform,
                (rebased_animations != TransformAnimations::default())
                    .then_some(&rebased_animations),
                property_clock,
            )?,
        )],
    )?;
    let mut layer = Chunk::list(
        *b"Layr",
        vec![
            Chunk::data(*b"ldta", record.encode())?,
            Chunk::data(*b"Utf8", name.as_bytes().to_vec())?,
            properties,
        ],
    );
    if let Some(clock) = source_clock {
        clock.append_time_remap_property_with_clock(&mut layer, property_clock)?;
    }
    Ok(layer)
}

fn transform_animations_have_keys(animations: &TransformAnimations) -> bool {
    animations.anchor.is_some()
        || animations.position.is_some()
        || animations.scale.is_some()
        || animations.rotation.is_some()
        || animations.opacity.is_some()
}

fn validate(rect: &VectorRectSpec) -> Result<(), AepWriteError> {
    if rect.name.is_empty() || rect.name.len() > 255 || rect.name.contains('\0') {
        return Err(AepWriteError::Invalid(
            "shape name must be 1..=255 UTF-8 bytes without NUL",
        ));
    }
    if rect.fill_color.is_some() == rect.stroke_color.is_some() {
        return Err(AepWriteError::Invalid(
            "native Rectangle needs exactly one Fill or Stroke",
        ));
    }
    if rect
        .size
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0 || *v > 65535.0)
        || rect.position.iter().any(|v| !v.is_finite())
        || !rect.roundness.is_finite()
        || !(0.0..=100000.0).contains(&rect.roundness)
        || !rect.stroke_width.is_finite()
        || rect.stroke_width < 0.0
        || !rect.stroke_miter_limit.is_finite()
        || rect.stroke_miter_limit < 0.0
    {
        return Err(AepWriteError::Invalid(
            "invalid native Rectangle geometry or stroke",
        ));
    }
    if rect
        .fill_color
        .iter()
        .chain(&rect.stroke_color)
        .flat_map(|color| color.iter())
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(AepWriteError::Invalid(
            "native Rectangle color lies outside 0..=1",
        ));
    }
    let t = &rect.transform;
    if t.anchor
        .iter()
        .chain(&t.position)
        .chain(&t.scale)
        .any(|v| !v.is_finite())
        || !t.rotation.is_finite()
        || !t.opacity.is_finite()
        || !(0.0..=100.0).contains(&t.opacity)
    {
        return Err(AepWriteError::Invalid(
            "invalid native Shape Layer Transform",
        ));
    }
    Ok(())
}

fn timeline_rect(
    rect: &VectorRectSpec,
    id: u32,
    duration: Duration24,
    animations: Option<&RectAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let rectangle = rectangle_geometry_with_clock(
        &rect.size,
        &rect.position,
        rect.roundness,
        animations,
        "Rectangle Path 1",
        clock,
    )?;
    timeline_vector_with_clock(
        &VectorAppearance::from(rect),
        id,
        duration,
        ("ADBE Vector Shape - Rect", rectangle),
        animations.map(|value| &value.transform),
        animations.map(|value| &value.stroke),
        clock,
    )
}

#[cfg(test)]
pub(super) fn rectangle_geometry(
    size: &[f64; 2],
    position: &[f64; 2],
    roundness: f64,
    animations: Option<&RectAnimations>,
    display_name: &str,
) -> Result<Chunk, AepWriteError> {
    rectangle_geometry_with_clock(
        size,
        position,
        roundness,
        animations,
        display_name,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn rectangle_geometry_with_clock(
    size: &[f64; 2],
    position: &[f64; 2],
    roundness: f64,
    animations: Option<&RectAnimations>,
    display_name: &str,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    Ok(views::group(
        1,
        display_name,
        vec![
            (
                "ADBE Vector Rect Size",
                views::property_with_clock(
                    ValueKind::VectorPair,
                    size,
                    // AE requires the range records even when the value is in range.
                    Some((-32000.0, 32000.0)),
                    animations.and_then(|value| value.size.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Vector Rect Position",
                views::property_with_clock(
                    ValueKind::VectorSpatial,
                    position,
                    None,
                    animations.and_then(|value| value.position.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Vector Rect Roundness",
                views::property_with_clock(
                    ValueKind::VectorScalar,
                    &[roundness],
                    Some((0.0, 100.0)),
                    animations.and_then(|value| value.roundness.as_ref()),
                    clock,
                )?,
            ),
        ],
    )?)
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn timeline_vector(
    appearance: &VectorAppearance,
    id: u32,
    duration: Duration24,
    geometry: (&str, Chunk),
    animations: Option<&TransformAnimations>,
    stroke: Option<&super::StrokeAnimations>,
) -> Result<Chunk, AepWriteError> {
    timeline_vector_with_clock(
        appearance,
        id,
        duration,
        geometry,
        animations,
        stroke,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn timeline_vector_with_clock(
    appearance: &VectorAppearance,
    id: u32,
    duration: Duration24,
    geometry: (&str, Chunk),
    animations: Option<&TransformAnimations>,
    stroke: Option<&super::StrokeAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    timeline_vector_contents_with_clock(
        appearance,
        id,
        duration,
        vec![geometry],
        animations,
        stroke,
        clock,
    )
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn timeline_vector_contents(
    appearance: &VectorAppearance,
    id: u32,
    duration: Duration24,
    contents: Vec<(&str, Chunk)>,
    animations: Option<&TransformAnimations>,
    stroke: Option<&super::StrokeAnimations>,
) -> Result<Chunk, AepWriteError> {
    timeline_vector_contents_with_clock(
        appearance,
        id,
        duration,
        contents,
        animations,
        stroke,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn timeline_vector_contents_with_clock(
    appearance: &VectorAppearance,
    id: u32,
    duration: Duration24,
    mut contents: Vec<(&str, Chunk)>,
    animations: Option<&TransformAnimations>,
    stroke: Option<&super::StrokeAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    if let Some(stroke) = stroke {
        stroke.validate(appearance.stroke_color.is_some())?;
    }
    let property = |kind, values: &[f64], bounds| {
        views::property_with_clock(kind, values, bounds, None, clock)
    };
    let animated = |kind, values: &[f64], bounds, animation| {
        views::property_with_clock(kind, values, bounds, animation, clock)
    };
    let (paint_name, paint) = if let Some(color) = appearance.fill_color {
        let rule = match appearance.fill_rule {
            ShapeFillRule::NonZeroWinding => 1.0,
            ShapeFillRule::EvenOdd => 2.0,
        };
        (
            "ADBE Vector Graphic - Fill",
            views::group(
                1,
                "Fill 1",
                vec![
                    (
                        "ADBE Vector Fill Color",
                        property(ValueKind::VectorColor, &native_color(color), None)?,
                    ),
                    (
                        "ADBE Vector Fill Opacity",
                        property(
                            ValueKind::VectorScalar,
                            &[appearance.paint_opacity],
                            Some((0.0, 100.0)),
                        )?,
                    ),
                    (
                        "ADBE Vector Fill Rule",
                        property(ValueKind::VectorEnum, &[rule], None)?,
                    ),
                ],
            )?,
        )
    } else {
        let color = appearance.stroke_color.expect("validated single paint");
        let cap = match appearance.stroke_cap {
            ShapeLineCap::Butt => 1.0,
            ShapeLineCap::Round => 2.0,
            ShapeLineCap::Square => 3.0,
        };
        let join = match appearance.stroke_join {
            ShapeLineJoin::Miter => 1.0,
            ShapeLineJoin::Round => 2.0,
            ShapeLineJoin::Bevel => 3.0,
        };
        let mut entries = vec![
            (
                "ADBE Vector Stroke Color",
                property(ValueKind::VectorColor, &native_color(color), None)?,
            ),
            (
                "ADBE Vector Stroke Opacity",
                property(
                    ValueKind::VectorScalar,
                    &[appearance.paint_opacity],
                    Some((0.0, 100.0)),
                )?,
            ),
            (
                "ADBE Vector Stroke Width",
                animated(
                    ValueKind::VectorScalar,
                    &[appearance.stroke_width],
                    Some((0.0, 100.0)),
                    stroke.and_then(|keys| keys.width.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Stroke Line Cap",
                property(ValueKind::VectorEnum, &[cap], None)?,
            ),
            (
                "ADBE Vector Stroke Line Join",
                animated(
                    ValueKind::VectorEnum,
                    &[join],
                    None,
                    stroke.and_then(|tracks| tracks.join.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Stroke Miter Limit",
                animated(
                    ValueKind::VectorScalar,
                    &[appearance.stroke_miter_limit],
                    Some((0.0, 100.0)),
                    stroke.and_then(|keys| keys.miter_limit.as_ref()),
                )?,
            ),
        ];
        if let Some(dashes) = appearance.stroke_dashes.native_group_with_clock(clock)? {
            entries.push(("ADBE Vector Stroke Dashes", dashes));
        }
        (
            "ADBE Vector Graphic - Stroke",
            views::group(1, "Stroke 1", entries)?,
        )
    };
    contents.push((paint_name, paint));
    let contents = views::indexed_group("Contents", contents)?;
    let vector = views::group(
        1,
        "Group 1",
        vec![
            ("ADBE Vectors Group", contents),
            (
                "ADBE Vector Transform Group",
                vector_transform_with_clock(clock)?,
            ),
        ],
    )?;
    let root_vectors = views::indexed_group("Contents", vec![("ADBE Vector Group", vector)])?;
    let properties = views::group(
        1,
        "",
        vec![
            (
                "ADBE Transform Group",
                layer_transform_with_clock(&appearance.transform, animations, clock)?,
            ),
            ("ADBE Root Vectors Group", root_vectors),
        ],
    )?;
    Ok(Chunk::list(
        *b"Layr",
        vec![
            Chunk::data(*b"ldta", LayerRecord::shape_ae26(id, duration)?.encode())?,
            Chunk::data(*b"Utf8", appearance.name.as_bytes().to_vec())?,
            properties,
        ],
    ))
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn timeline_vector_program(
    name: &str,
    transform: &SolidTransform,
    id: u32,
    duration: Duration24,
    contents: Vec<(&str, Chunk)>,
    animations: Option<&TransformAnimations>,
) -> Result<Chunk, AepWriteError> {
    timeline_vector_program_with_clock(
        name,
        transform,
        id,
        duration,
        contents,
        animations,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(super) fn timeline_vector_program_with_clock(
    name: &str,
    transform: &SolidTransform,
    id: u32,
    duration: Duration24,
    contents: Vec<(&str, Chunk)>,
    animations: Option<&TransformAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let root_vectors = views::indexed_group("Contents", contents)?;
    let properties = views::group(
        1,
        "",
        vec![
            (
                "ADBE Transform Group",
                layer_transform_with_clock(transform, animations, clock)?,
            ),
            ("ADBE Root Vectors Group", root_vectors),
        ],
    )?;
    Ok(Chunk::list(
        *b"Layr",
        vec![
            Chunk::data(*b"ldta", LayerRecord::shape_ae26(id, duration)?.encode())?,
            Chunk::data(*b"Utf8", name.as_bytes().to_vec())?,
            properties,
        ],
    ))
}

/// AE static Color cdat stores 0..255 A,R,G,B, not FX normalized RGBA.
pub(super) fn native_color(color: [f64; 4]) -> [f64; 4] {
    [
        color[3] * 255.0,
        color[0] * 255.0,
        color[1] * 255.0,
        color[2] * 255.0,
    ]
}

#[cfg(test)]
pub(super) fn vector_transform() -> Result<Chunk, AepWriteError> {
    vector_transform_with_clock(super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn vector_transform_with_clock(
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let property = |kind, values: &[f64], bounds| {
        views::property_with_clock(kind, values, bounds, None, clock)
    };
    Ok(views::group(
        1,
        "Transform",
        vec![
            (
                "ADBE Vector Anchor",
                property(ValueKind::VectorSpatial, &[0.0, 0.0], None)?,
            ),
            (
                "ADBE Vector Position",
                property(ValueKind::VectorSpatial, &[0.0, 0.0], None)?,
            ),
            (
                "ADBE Vector Scale",
                property(
                    ValueKind::VectorPair,
                    &[100.0, 100.0],
                    Some((-32000.0, 32000.0)),
                )?,
            ),
            (
                "ADBE Vector Rotation",
                property(ValueKind::VectorAngle, &[0.0], None)?,
            ),
            (
                "ADBE Vector Group Opacity",
                property(ValueKind::VectorScalar, &[100.0], Some((0.0, 100.0)))?,
            ),
        ],
    )?)
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
fn layer_transform(
    t: &SolidTransform,
    animations: Option<&TransformAnimations>,
) -> Result<Chunk, AepWriteError> {
    layer_transform_with_clock(t, animations, super::keyframes::PropertyClock::DEFAULT)
}

fn layer_transform_with_clock(
    t: &SolidTransform,
    animations: Option<&TransformAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    Ok(views::group(
        1,
        "-_0_/-",
        vec![
            (
                "ADBE Anchor Point",
                views::property_with_clock(
                    ValueKind::Spatial,
                    &[t.anchor[0], t.anchor[1], 0.0],
                    None,
                    animations.and_then(|value| value.anchor.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Position",
                views::property_with_clock(
                    ValueKind::Spatial,
                    &[t.position[0], t.position[1], 0.0],
                    None,
                    animations.and_then(|value| value.position.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Scale",
                views::property_with_clock(
                    ValueKind::Scale,
                    &[t.scale[0] / 100.0, t.scale[1] / 100.0, 1.0],
                    Some((0.0, 0.0)),
                    animations.and_then(|value| value.scale.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Rotate Z",
                views::property_with_clock(
                    ValueKind::Angle,
                    &[t.rotation],
                    None,
                    animations.and_then(|value| value.rotation.as_ref()),
                    clock,
                )?,
            ),
            (
                "ADBE Opacity",
                views::property_with_clock(
                    ValueKind::Scalar,
                    &[t.opacity / 100.0],
                    Some((0.0, 100.0)),
                    animations.and_then(|value| value.opacity.as_ref()),
                    clock,
                )?,
            ),
        ],
    )?)
}

#[cfg(test)]
mod hierarchy_writer_tests {
    use super::*;

    #[test]
    fn native_two_layer_envelope_boundaries_are_preserved() {
        use sha2::{Digest, Sha256};
        fn containing_layers(chunks: &[Chunk]) -> Option<&[Chunk]> {
            if chunks
                .iter()
                .any(|chunk| chunk.list_kind() == Some(*b"Layr"))
            {
                return Some(chunks);
            }
            chunks
                .iter()
                .filter_map(Chunk::children)
                .find_map(containing_layers)
        }
        fn first_boundary(chunks: &[Chunk]) -> &[Chunk] {
            let indices: Vec<_> = chunks
                .iter()
                .enumerate()
                .filter(|(_, chunk)| chunk.list_kind() == Some(*b"Layr"))
                .map(|(index, _)| index)
                .collect();
            assert_eq!(indices.len(), 2);
            &chunks[indices[0] + 1..indices[1]]
        }
        let source = include_bytes!("../../tests/fixtures/render/export_add_blend.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(source)),
            "d04ffbca88577b14b1702e24a003c3a92aecf038a741a478df4e1fb8c9169aee"
        );
        let native = crate::aep::Project::parse(source).unwrap();
        let duration = Duration24::from_frames(48).unwrap();
        let source = SolidLayerSpec {
            name: "First editable solid".into(),
            width: 480,
            height: 270,
            color: [0.5, 0.125, 0.125],
            transform: SolidTransform {
                anchor: [240.0, 135.0],
                position: [960.0, 540.0],
                scale: [100.0; 2],
                rotation: 0.0,
                opacity: 100.0,
            },
        };
        let mut second = source.clone();
        second.name = "Second editable solid".into();
        let generated = build_timeline(
            &[LayerSpec::Solid(source), LayerSpec::Solid(second)],
            duration,
            [1920, 1080],
        )
        .unwrap();
        assert_eq!(
            first_boundary(&generated.layers),
            first_boundary(containing_layers(&native.chunks).unwrap())
        );
    }

    #[test]
    fn native_shape_size_and_scale_bounds_are_required_for_adobe_acceptance() {
        fn property<'a>(chunks: &'a [Chunk], name: &str) -> Option<&'a [Chunk]> {
            for pair in chunks.windows(2) {
                if pair[0].id() == *b"tdmn"
                    && pair[0].data_payload()?.split(|byte| *byte == 0).next()
                        == Some(name.as_bytes())
                {
                    return pair[1].children();
                }
            }
            chunks
                .iter()
                .find_map(|chunk| property(chunk.children()?, name))
        }
        let project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(composition) = &project.item(1).unwrap().kind
        else {
            panic!("pinned native composition");
        };
        let geometry =
            rectangle_geometry(&[120.0, 80.0], &[60.0, 40.0], 0.0, None, "Rectangle").unwrap();
        let transform = vector_transform().unwrap();
        for (name, generated) in [
            ("ADBE Vector Rect Size", geometry),
            ("ADBE Vector Scale", transform),
        ] {
            let native = property(&composition.layers[0].content, name).unwrap();
            let fresh = property(generated.children().unwrap(), name).unwrap();
            for (tag, expected) in [(*b"tdum", -32000.0_f64), (*b"tduM", 32000.0_f64)] {
                let native_bound = crate::properties::data(native, tag).unwrap();
                assert_eq!(native_bound, expected.to_be_bytes());
                assert_eq!(
                    crate::properties::data(fresh, tag).unwrap(),
                    native_bound,
                    "{name}"
                );
            }
        }
    }

    fn identity_transform() -> SolidTransform {
        SolidTransform {
            anchor: [0.0, 0.0],
            position: [0.0, 0.0],
            scale: [100.0, 100.0],
            rotation: 0.0,
            opacity: 100.0,
        }
    }

    #[test]
    fn null_writer_emits_source_backed_flagged_av_layer() {
        let spec = CompositionSpec {
            name: "Root".into(),
            width: 320,
            height: 180,
            duration_frames: 24,
        };
        let bytes = write_composition(
            &spec,
            &[LayerSpec::Null(NullLayerSpec {
                name: "Parent".into(),
                transform: identity_transform(),
                transform_animations: TransformAnimations::default(),
            })],
        )
        .unwrap();
        let project = crate::structure::read_project(&bytes).unwrap();
        let root = project.item(1).unwrap();
        let crate::structure::ItemKind::Composition(composition) = &root.kind else {
            panic!("root must be a composition");
        };
        assert!(composition.layers[0].record.flags().null_layer);
        let source = project
            .item(composition.layers[0].record.source_id())
            .unwrap();
        assert_eq!(
            source.solid.as_ref().unwrap().as_ref().unwrap().color,
            [1.0; 3]
        );
    }

    #[test]
    fn null_anchor_uses_native_av_source_units() {
        use sha2::{Digest, Sha256};

        // Independent Adobe AV storage: pixel anchor [61, 41] on a 120x80 source.
        let native = include_bytes!("../../tests/fixtures/effects/transform_probe.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(native)),
            "d669d0ca3d505ebaca545d6866185f6f6e69fe00256024bd0cb9ee22ba1a67d7"
        );
        let project = crate::structure::read_project(native).unwrap();
        let crate::structure::ItemKind::Composition(composition) = &project.item(1).unwrap().kind
        else {
            panic!("native composition");
        };
        let properties = crate::properties::read_transform(&composition.layers[0].content).unwrap();
        let native_anchor = properties
            .iter()
            .find(|property| property.match_name == "ADBE Anchor Point")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(native_anchor.values[..2], [61.0 / 120.0, 41.0 / 80.0]);

        let mut null = NullLayerSpec {
            name: "Offset parent".into(),
            transform: SolidTransform {
                anchor: [464.0, 110.0],
                ..identity_transform()
            },
            transform_animations: TransformAnimations::default(),
        };
        let numeric = sidecar_anchor_property(&LayerSpec::Null(null.clone()));
        assert_eq!(numeric.values[..2], [4.64, 1.1]);
        null.transform_animations.anchor = Some(super::super::keyframes::Track {
            keys: vec![super::super::keyframes::Keyframe {
                time_millis: 250,
                values: vec![464.0, 110.0, 3.0],
                easing: vec![super::super::keyframes::Easing::Linear],
                spatial_in: vec![10.0, 20.0, 1.0],
                spatial_out: vec![30.0, 40.0, 2.0],
            }],
        });
        let numeric = sidecar_anchor_property(&LayerSpec::Null(null.clone()));
        let key = &numeric.keyframes[0];
        assert_eq!(key.time_secs, 0.25);
        assert_eq!(key.values, [4.64, 1.1, 3.0]);
        assert_eq!(key.spatial_in, [0.1, 0.2, 1.0]);
        assert_eq!(key.spatial_out, [0.3, 0.4, 2.0]);
        assert_eq!(null.transform.anchor, [464.0, 110.0]);
        assert_eq!(
            null.transform_animations.anchor.unwrap().keys[0].values,
            [464.0, 110.0, 3.0]
        );
    }

    #[test]
    fn precomposition_is_a_distinct_source_item() {
        let duration = Duration24::from_frames(24).unwrap();
        let spec = CompositionSpec {
            name: "Root".into(),
            width: 320,
            height: 180,
            duration_frames: 24,
        };
        let bytes = write_composition(
            &spec,
            &[LayerSpec::Precomposition(PrecompositionSpec {
                collapse_transformations: false,
                name: "Nested".into(),
                width: 100,
                height: 80,
                duration,
                transform: identity_transform(),
                transform_animations: TransformAnimations::default(),
                layers: Vec::new(),
                composition_record: None,
            })],
        )
        .unwrap();
        let project = crate::structure::read_project(&bytes).unwrap();
        let crate::structure::ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
            panic!("root must be a composition");
        };
        let source_id = root.layers[0].record.source_id();
        assert_ne!(source_id, 1);
        assert!(matches!(
            project.item(source_id).unwrap().kind,
            crate::structure::ItemKind::Composition(_)
        ));
    }

    #[test]
    fn c17_fresh_solid_scale_hold_uses_exact_thirty_fps_wire_units() {
        use super::super::keyframes::{Easing, Keyframe, Track};
        let duration = Duration24::from_frames(48).unwrap();
        let solid = SolidLayerSpec {
            name: "independent c17 Scale Hold".into(),
            width: 100,
            height: 100,
            color: [1.0, 0.0, 0.0],
            transform: identity_transform(),
        };
        let keys = [250, 1100, 1650]
            .into_iter()
            .map(|time_millis| Keyframe {
                time_millis,
                values: vec![1.51, 0.64, 1.0],
                easing: vec![Easing::Hold; 3],
                spatial_in: vec![],
                spatial_out: vec![],
            })
            .collect();
        let layer = LayerSpec::AnimatedSolid(
            solid,
            TransformAnimations {
                scale: Some(Track { keys }),
                ..TransformAnimations::default()
            },
        );
        let bytes = write_composition_at_rate(
            "c17",
            320,
            180,
            &[layer],
            crate::timing::FrameRate::new(30.0).unwrap(),
            duration,
            None,
        )
        .unwrap();
        let project = crate::structure::read_project(&bytes).unwrap();
        let comp = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                crate::structure::ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .unwrap();
        fn scale(chunks: &[Chunk]) -> Option<&[Chunk]> {
            for pair in chunks.windows(2) {
                if pair[0].id() == *b"tdmn"
                    && pair[0].data_payload()?.split(|b| *b == 0).next() == Some(b"ADBE Scale")
                {
                    return pair[1].children();
                }
            }
            chunks.iter().find_map(|chunk| scale(chunk.children()?))
        }
        fn assert_property_clocks(chunks: &[Chunk]) {
            for chunk in chunks {
                if chunk.id() == *b"tdb4" {
                    let data = chunk.data_payload().unwrap();
                    assert_eq!(u32::from_be_bytes(data[12..16].try_into().unwrap()), 30_720);
                }
                if let Some(children) = chunk.children() {
                    assert_property_clocks(children);
                }
            }
        }
        assert_property_clocks(&comp.layers[0].content);
        let property = scale(&comp.layers[0].content).unwrap();
        let descriptor = crate::properties::data(property, *b"tdb4").unwrap();
        assert_eq!(
            u32::from_be_bytes(descriptor[12..16].try_into().unwrap()),
            30_720
        );
        let list = crate::properties::unique_list(property, *b"list").unwrap();
        let data = crate::properties::data(list, *b"ldat").unwrap();
        let stride = data.len() / 3;
        assert_eq!(
            (0..3)
                .map(|i| i32::from_be_bytes(data[i * stride..i * stride + 4].try_into().unwrap()))
                .collect::<Vec<_>>(),
            [7680, 33792, 50688]
        );
    }

    #[test]
    fn selected_rate_reaches_root_and_nested_vector_property_clocks() {
        use super::super::{
            GeometryAnimations, KeyframeEasing, NumericKeyframe, NumericTrack, VectorContent,
            VectorGeometry, VectorLayerSpec,
        };

        let vector = VectorLayerSpec {
            name: "Owning clock vector".into(),
            transform: identity_transform(),
            transform_animations: TransformAnimations::default(),
            contents: vec![VectorContent::Geometry {
                geometry: VectorGeometry::Ellipse(Default::default()),
                animations: GeometryAnimations {
                    ellipse_size: Some(NumericTrack {
                        keys: vec![NumericKeyframe {
                            time_millis: 1100,
                            values: vec![80.0, 40.0],
                            easing: vec![KeyframeEasing::Hold; 2],
                            spatial_in: vec![],
                            spatial_out: vec![],
                        }],
                    }),
                    ..Default::default()
                },
            }],
        };
        fn check(chunks: &[Chunk], keyed: &mut usize) {
            for chunk in chunks {
                if let Some(children) = chunk.children() {
                    if chunk.list_kind() == Some(*b"tdbs") {
                        if let Some(descriptor) = children.iter().find(|c| c.id() == *b"tdb4") {
                            assert_eq!(
                                &descriptor.data_payload().unwrap()[12..16],
                                &30_720_u32.to_be_bytes()
                            );
                        }
                        if let Some(list) =
                            children.iter().find(|c| c.list_kind() == Some(*b"list"))
                        {
                            let data = crate::properties::data(list.children().unwrap(), *b"ldat")
                                .unwrap();
                            assert_eq!(&data[..4], &33_792_i32.to_be_bytes());
                            *keyed += 1;
                        }
                    }
                    check(children, keyed);
                }
            }
        }
        let duration = Duration24::from_frames(48).unwrap();
        let mut nested = empty_precomposition(duration);
        nested.layers = vec![LayerSpec::VectorProgram(vector.clone())];
        for layer in [
            LayerSpec::VectorProgram(vector),
            LayerSpec::Precomposition(nested),
        ] {
            let timeline = build_timeline_at_rate(
                &[layer],
                duration,
                [320, 180],
                crate::timing::FrameRate::new(30.0).unwrap(),
            )
            .unwrap();
            let mut keyed = 0;
            check(&timeline.layers, &mut keyed);
            check(&timeline.sources, &mut keyed);
            assert_eq!(keyed, 1);
        }
    }

    #[test]
    fn selected_rate_reaches_camera_mask_and_style_descriptors() {
        fn clocks(chunks: &[Chunk], found: &mut Vec<u32>) {
            for chunk in chunks {
                if chunk.id() == *b"tdb4" {
                    let data = chunk.data_payload().unwrap();
                    found.push(u32::from_be_bytes(data[12..16].try_into().unwrap()));
                }
                if let Some(children) = chunk.children() {
                    clocks(children, found);
                }
            }
        }
        let mut options = native_options(None);
        options.masks.push(
            super::super::NativeMaskSpec::crop_rectangle(
                "Clocked static mask",
                [100, 100],
                [10.0, 10.0, 80.0, 80.0],
            )
            .unwrap(),
        );
        options
            .styles
            .push(crate::layer_styles::NativeLayerStyle::Stroke(
                crate::layer_styles::NativeStroke {
                    enabled: true,
                    color: [0.1, 0.2, 0.3, 1.0],
                    size: 8.0,
                    position: fx_schema::LayerStrokePosition::Center,
                    blend_mode: fx_schema::BlendMode::Normal,
                    animations: Vec::new(),
                },
            ));
        let layer = LayerSpec::Options(
            Box::new(LayerSpec::Null(NullLayerSpec {
                name: "Clocked envelope".into(),
                transform: identity_transform(),
                transform_animations: TransformAnimations::default(),
            })),
            options,
        );
        let camera = LayerSpec::Camera(super::super::camera::NativeCameraSpec::root(320, 180));
        let timeline = build_timeline_at_rate(
            &[layer, camera],
            Duration24::from_frames(48).unwrap(),
            [320, 180],
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let mut found = Vec::new();
        clocks(&timeline.layers, &mut found);
        assert!(!found.is_empty());
        assert!(found.iter().all(|clock| *clock == 30_720), "{found:?}");
    }

    #[test]
    fn selected_export_rate_propagates_to_nested_precompositions() {
        let duration = Duration24::from_frames(24).unwrap();
        let nested = empty_precomposition(duration);
        let mut outer = empty_precomposition(duration);
        outer.name = "Outer".into();
        outer.composition_record =
            Some(CompositionRecord::empty_ae26(outer.width, outer.height, duration).unwrap());
        outer.layers.push(LayerSpec::Precomposition(nested));

        let bytes = write_composition_at_rate(
            "Root",
            320,
            180,
            &[LayerSpec::Precomposition(outer)],
            crate::timing::FrameRate::new(30.0).unwrap(),
            duration,
            None,
        )
        .unwrap();
        let project = crate::structure::read_project(&bytes).unwrap();
        let rates: Vec<_> = project
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                crate::structure::ItemKind::Composition(composition) => {
                    Some(composition.frame_rate)
                }
                _ => None,
            })
            .collect();
        assert_eq!(rates, [30.0, 30.0, 30.0]);
    }

    #[test]
    fn precomposition_anchor_keys_and_handles_use_source_units() {
        let duration = Duration24::from_frames(48).unwrap();
        let mut nested = empty_precomposition(duration);
        nested.width = 100;
        nested.height = 80;
        nested.transform.anchor = [50.0, 40.0];
        nested.transform_animations.anchor = Some(super::super::keyframes::Track {
            keys: vec![super::super::keyframes::Keyframe {
                time_millis: 250,
                values: vec![50.0, 40.0, 3.0],
                easing: vec![super::super::keyframes::Easing::Linear],
                spatial_in: vec![10.0, 8.0, 1.0],
                spatial_out: vec![20.0, 16.0, 2.0],
            }],
        });
        let layers = [LayerSpec::Precomposition(nested)];
        let timeline = build_timeline(&layers, duration, [320, 180]).unwrap();
        let layer = timeline
            .layers
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Layr"))
            .unwrap();
        let roots = crate::properties::root_runs(layer.children().unwrap()).unwrap();
        let transform = roots
            .iter()
            .find(|(name, _)| *name == "ADBE Transform Group")
            .unwrap()
            .1;
        let properties =
            crate::properties::runs(crate::properties::unique_list(transform, *b"tdgp").unwrap())
                .unwrap();
        let anchor = properties
            .iter()
            .find(|(name, _)| *name == "ADBE Anchor Point")
            .unwrap()
            .1;
        let numeric = crate::properties::read_numeric(
            crate::properties::unique_list(anchor, *b"tdbs").unwrap(),
        )
        .unwrap();
        let key = &numeric.keyframes[0];
        assert_eq!(key.time_secs, 0.25);
        assert_eq!(key.values, [0.5, 0.5, 3.0]);
        assert_eq!(key.spatial_in, [0.1, 0.1, 1.0]);
        assert_eq!(key.spatial_out, [0.2, 0.2, 2.0]);
        let LayerSpec::Precomposition(nested) = &layers[0] else {
            panic!("precomposition input");
        };
        assert_eq!(nested.transform.anchor, [50.0, 40.0]);
        assert_eq!(
            nested.transform_animations.anchor.as_ref().unwrap().keys[0].values,
            [50.0, 40.0, 3.0]
        );
    }

    fn sidecar_anchor_property(layer: &LayerSpec) -> crate::properties::NumericProperty {
        let timeline = build_timeline(
            std::slice::from_ref(layer),
            Duration24::from_frames(48).unwrap(),
            [320, 180],
        )
        .unwrap();
        let layer = timeline
            .layers
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Layr"))
            .unwrap();
        let roots = crate::properties::root_runs(layer.children().unwrap()).unwrap();
        let transform = roots
            .iter()
            .find(|(name, _)| *name == "ADBE Transform Group")
            .unwrap()
            .1;
        let properties =
            crate::properties::runs(crate::properties::unique_list(transform, *b"tdgp").unwrap())
                .unwrap();
        let anchor = properties
            .iter()
            .find(|(name, _)| *name == "ADBE Anchor Point")
            .unwrap()
            .1;
        crate::properties::read_numeric(crate::properties::unique_list(anchor, *b"tdbs").unwrap())
            .unwrap()
    }

    fn anchor_sidecar(is_three_d: bool) -> NativeLayerOptions {
        let mut options = native_options(None);
        options.transform_3d = Some((
            super::super::NativeTransform3d {
                is_three_d,
                anchor: [50.0, 40.0, 3.0],
                position: [160.0, 90.0, 7.0],
                scale: [1.0; 3],
                orientation: [0.0; 3],
                rotation_x: 0.0,
                rotation_y: 0.0,
                rotation_z: 0.0,
                opacity: 1.0,
            },
            super::super::Transform3dAnimations {
                // Planar sidecars also carry independent Position followers.
                position_separated: Some([None, None, None]),
                ..Default::default()
            },
        ));
        options
    }

    #[test]
    fn null_sidecar_anchor_uses_source_units_without_mutating_input() {
        for is_three_d in [false, true] {
            let mut options = anchor_sidecar(is_three_d);
            for animated in [false, true] {
                if animated {
                    options.transform_3d.as_mut().unwrap().1.anchor =
                        Some(super::super::NumericTrack {
                            keys: vec![super::super::NumericKeyframe {
                                time_millis: 250,
                                values: vec![50.0, 40.0, 3.0],
                                easing: vec![super::super::KeyframeEasing::Linear],
                                spatial_in: vec![10.0, 8.0, 1.0],
                                spatial_out: vec![20.0, 16.0, 2.0],
                            }],
                        });
                }
                let original = options.clone();
                let layer = LayerSpec::Options(
                    Box::new(LayerSpec::Null(NullLayerSpec {
                        name: "Source-normalized Null sidecar".into(),
                        transform: identity_transform(),
                        transform_animations: TransformAnimations::default(),
                    })),
                    options.clone(),
                );
                let property = sidecar_anchor_property(&layer);
                if !animated {
                    assert_eq!(property.values, [0.5, 0.4, 3.0]);
                }
                if animated {
                    let key = &property.keyframes[0];
                    assert_eq!(key.time_secs, 0.25);
                    assert_eq!(key.values, [0.5, 0.4, 3.0]);
                    assert_eq!(key.spatial_in, [0.1, 0.08, 1.0]);
                    assert_eq!(key.spatial_out, [0.2, 0.16, 2.0]);
                }
                let LayerSpec::Options(_, written_options) = &layer else {
                    unreachable!()
                };
                assert_eq!(*written_options, original);
            }
        }
    }

    #[test]
    fn precomposition_sidecar_static_anchor_uses_source_units() {
        for is_three_d in [false, true] {
            let options = anchor_sidecar(is_three_d);
            let original = options.clone();
            let layer = LayerSpec::Options(
                Box::new(LayerSpec::Precomposition(empty_precomposition(
                    Duration24::from_frames(48).unwrap(),
                ))),
                options,
            );
            assert_eq!(sidecar_anchor_property(&layer).values, [0.5, 0.5, 3.0]);
            let LayerSpec::Options(_, options) = &layer else {
                unreachable!()
            };
            assert_eq!(*options, original);
        }
    }

    #[test]
    fn precomposition_sidecar_anchor_keys_and_handles_use_source_units() {
        for is_three_d in [false, true] {
            let mut options = anchor_sidecar(is_three_d);
            options.transform_3d.as_mut().unwrap().1.anchor = Some(super::super::NumericTrack {
                keys: vec![super::super::NumericKeyframe {
                    time_millis: 250,
                    values: vec![50.0, 40.0, 3.0],
                    easing: vec![super::super::KeyframeEasing::Linear],
                    spatial_in: vec![10.0, 8.0, 1.0],
                    spatial_out: vec![20.0, 16.0, 2.0],
                }],
            });
            let original = options.clone();
            let layer = LayerSpec::Options(
                Box::new(LayerSpec::Precomposition(empty_precomposition(
                    Duration24::from_frames(48).unwrap(),
                ))),
                options,
            );
            let property = sidecar_anchor_property(&layer);
            let key = &property.keyframes[0];
            assert_eq!(key.time_secs, 0.25);
            assert_eq!(key.values, [0.5, 0.5, 3.0]);
            assert_eq!(key.spatial_in, [0.1, 0.1, 1.0]);
            assert_eq!(key.spatial_out, [0.2, 0.2, 2.0]);
            let LayerSpec::Options(_, options) = &layer else {
                unreachable!()
            };
            assert_eq!(*options, original);
        }
    }

    #[test]
    fn shape_sidecar_anchor_keeps_pixel_units() {
        for is_three_d in [false, true] {
            let layer = LayerSpec::Options(
                Box::new(LayerSpec::Rect(VectorRectSpec {
                    name: "Shape anchor control".into(),
                    stroke_dashes: StrokeDashes::default(),
                    size: [100.0, 80.0],
                    position: [0.0; 2],
                    roundness: 0.0,
                    fill_color: Some([1.0; 4]),
                    stroke_color: None,
                    stroke_width: 0.0,
                    stroke_join: ShapeLineJoin::Miter,
                    stroke_miter_limit: 4.0,
                    transform: identity_transform(),
                })),
                anchor_sidecar(is_three_d),
            );
            assert_eq!(sidecar_anchor_property(&layer).values, [50.0, 40.0, 3.0]);
        }
    }

    fn source_clock() -> super::super::source_clock::SourceClockPlan {
        super::super::source_clock::SourceClockPlan::affine(
            fx_schema::TimeRangeProperty::new(
                fx_schema::Time::from_millis(1_000),
                fx_schema::Duration::from_millis(2_000),
            ),
            fx_schema::Time::from_millis(500),
            fx_schema::Time::from_millis(1_500),
            2_000,
        )
        .unwrap()
    }

    fn native_options(
        clock: Option<super::super::source_clock::SourceClockPlan>,
    ) -> NativeLayerOptions {
        NativeLayerOptions {
            fx_id: LayerId::new(41),
            parent: None,
            matte: None,
            enabled: true,
            adjustment_layer: false,
            motion_blur: false,
            blend_mode: 2,
            masks: Vec::new(),
            effects: Vec::new(),
            styles: Vec::new(),
            source_clock: clock,
            transform_3d: None,
        }
    }

    fn planned_video(dimensions: [u16; 2], audio_sample_rate: f64) -> LayerSpec {
        let LayerSpec::Footage(mut footage, animations) = frame_blended_video() else {
            unreachable!("frame_blended_video always returns footage")
        };
        footage.source.dimensions = dimensions;
        footage.source.audio_sample_rate = audio_sample_rate;
        footage.transform.width = dimensions[0];
        footage.transform.height = dimensions[1];
        LayerSpec::Footage(footage, animations)
    }

    fn referenced_layer(
        layer: LayerSpec,
        fx_id: u64,
        parent: Option<u64>,
        matte: Option<u64>,
    ) -> LayerSpec {
        let mut options = native_options(None);
        options.fx_id = LayerId::new(fx_id);
        options.parent = parent.map(LayerId::new);
        options.matte = matte.map(|layer| super::super::NativeMatteRef {
            layer: LayerId::new(layer),
            mode: 1,
        });
        LayerSpec::Options(Box::new(layer), options)
    }

    fn planner_regression_layers() -> Vec<LayerSpec> {
        vec![
            referenced_layer(
                LayerSpec::Null(NullLayerSpec {
                    name: "Parent".into(),
                    transform: identity_transform(),
                    transform_animations: TransformAnimations::default(),
                }),
                10,
                None,
                None,
            ),
            referenced_layer(planned_video([1920, 1080], 0.0), 11, Some(10), None),
            referenced_layer(planned_video([1920, 1080], -0.0), 12, Some(10), Some(11)),
            // A shared path is insufficient for deduplication: dimensions are
            // part of the complete native source interpretation.
            referenced_layer(planned_video([1280, 720], 0.0), 13, None, None),
        ]
    }

    #[test]
    fn planner_deduplicates_full_sources_and_resolves_references_deterministically() {
        let spec = CompositionSpec {
            name: "Planner regression".into(),
            width: 320,
            height: 180,
            duration_frames: 48,
        };
        let first = write_composition(&spec, &planner_regression_layers()).unwrap();
        let second = write_composition(&spec, &planner_regression_layers()).unwrap();
        assert_eq!(first, second);

        let project = crate::structure::read_project(&first).unwrap();
        let crate::structure::ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
            panic!("root must be a composition");
        };
        let parent_id = root.layers[0].record.id();
        let matte_id = root.layers[1].record.id();
        assert_eq!(root.layers[1].record.parent_id(), parent_id);
        assert_eq!(root.layers[2].record.parent_id(), parent_id);
        assert_eq!(root.layers[2].record.matte_layer_id(), Some(matte_id));
        assert_eq!(
            root.layers[1].record.source_id(),
            root.layers[2].record.source_id(),
            "finite +0.0 and -0.0 source fields remain equal"
        );
        assert_ne!(
            root.layers[1].record.source_id(),
            root.layers[3].record.source_id(),
            "same path with different source metadata must not deduplicate"
        );
    }

    #[test]
    fn planner_rejects_conflicting_fx_ids() {
        let layers = [
            referenced_layer(
                LayerSpec::Null(NullLayerSpec {
                    name: "First".into(),
                    transform: identity_transform(),
                    transform_animations: TransformAnimations::default(),
                }),
                10,
                None,
                None,
            ),
            referenced_layer(planned_video([1920, 1080], 0.0), 10, None, None),
        ];
        assert!(matches!(
            validate_layers(&layers, Duration24::from_frames(48).unwrap()),
            Err(AepWriteError::Invalid(
                "duplicate FX layer identity in native composition plan"
            ))
        ));
    }

    #[test]
    fn planar_transform_sidecar_does_not_request_perspective_camera() {
        let transform = super::super::transform3d::NativeTransform3d {
            is_three_d: false,
            anchor: [0.0; 3],
            position: [0.0; 3],
            scale: [1.0; 3],
            orientation: [0.0; 3],
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            opacity: 1.0,
        };
        let mut options = native_options(None);
        options.transform_3d = Some((transform, Default::default()));
        let layer = |options| {
            LayerSpec::Options(
                Box::new(LayerSpec::Solid(SolidLayerSpec {
                    name: "Planar".into(),
                    width: 100,
                    height: 80,
                    color: [0.5; 3],
                    transform: identity_transform(),
                })),
                options,
            )
        };
        assert!(!layer(options.clone()).has_three_d_root().unwrap());
        options.transform_3d.as_mut().unwrap().0.is_three_d = true;
        assert!(layer(options).has_three_d_root().unwrap());
    }

    fn empty_precomposition(duration: Duration24) -> PrecompositionSpec {
        PrecompositionSpec {
            collapse_transformations: false,
            name: "Nested".into(),
            width: 100,
            height: 80,
            duration,
            transform: identity_transform(),
            transform_animations: TransformAnimations::default(),
            layers: Vec::new(),
            composition_record: None,
        }
    }

    #[test]
    fn precomposition_source_clock_reaches_the_emitted_layer_record() {
        let spec = CompositionSpec {
            name: "Root".into(),
            width: 320,
            height: 180,
            duration_frames: 96,
        };
        let layer = LayerSpec::Options(
            Box::new(LayerSpec::Precomposition(empty_precomposition(
                Duration24::from_frames(48).unwrap(),
            ))),
            native_options(Some(source_clock())),
        );
        let bytes = write_composition(&spec, &[layer]).unwrap();
        let project = crate::structure::read_project(&bytes).unwrap();
        let crate::structure::ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
            panic!("root must be a composition");
        };
        let record = &root.layers[0].record;
        assert_eq!(record.stretch_fraction(), (2, 1));
        assert_eq!(record.start_time_fraction(), (0, 1));
        assert_eq!(record.in_point_fraction(), (1, 2));
        assert_eq!(record.out_point_fraction(), (3, 2));
    }

    #[test]
    fn source_clock_and_generic_timed_envelope_are_mutually_exclusive() {
        let duration = Duration24::from_frames(96).unwrap();
        let layer = LayerSpec::Options(
            Box::new(LayerSpec::Timed(
                Box::new(LayerSpec::Precomposition(empty_precomposition(
                    Duration24::from_frames(48).unwrap(),
                ))),
                LayerTiming {
                    start_millis: 1_000,
                    end_millis: 3_000,
                },
            )),
            native_options(Some(source_clock())),
        );

        assert!(validate_layer_payload(&layer, duration).is_err());
    }

    #[test]
    fn payload_validation_defers_only_composition_local_references() {
        let duration = Duration24::from_frames(24).unwrap();
        let parent = LayerSpec::Options(
            Box::new(LayerSpec::Null(NullLayerSpec {
                name: "Parent".into(),
                transform: identity_transform(),
                transform_animations: TransformAnimations::default(),
            })),
            native_options(None),
        );
        let mut child_options = native_options(None);
        child_options.fx_id = LayerId::new(42);
        child_options.parent = Some(LayerId::new(41));
        let child = LayerSpec::Options(
            Box::new(LayerSpec::Null(NullLayerSpec {
                name: "Child".into(),
                transform: identity_transform(),
                transform_animations: TransformAnimations::default(),
            })),
            child_options,
        );

        assert!(validate_layer_payload(&child, duration).is_ok());
        assert!(validate_layers(std::slice::from_ref(&child), duration).is_err());
        assert!(validate_layers(&[parent, child], duration).is_ok());
    }

    #[test]
    fn payload_validation_keeps_nested_composition_references_authoritative() {
        let duration = Duration24::from_frames(24).unwrap();
        let mut child_options = native_options(None);
        child_options.fx_id = LayerId::new(42);
        child_options.parent = Some(LayerId::new(41));
        let nested_child = LayerSpec::Options(
            Box::new(LayerSpec::Null(NullLayerSpec {
                name: "Nested Child".into(),
                transform: identity_transform(),
                transform_animations: TransformAnimations::default(),
            })),
            child_options,
        );
        let precomposition = LayerSpec::Precomposition(PrecompositionSpec {
            layers: vec![nested_child],
            ..empty_precomposition(duration)
        });

        assert!(validate_layer_payload(&precomposition, duration).is_err());
    }

    #[test]
    fn source_clock_rebases_every_three_d_track_including_orientation() {
        let key = crate::writer::NumericKeyframe {
            time_millis: 0,
            values: vec![1.0, 2.0, 3.0],
            easing: vec![crate::writer::KeyframeEasing::Linear; 3],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        let track = crate::writer::NumericTrack { keys: vec![key] };
        let animations = crate::writer::Transform3dAnimations {
            anchor: Some(track.clone()),
            position: Some(track.clone()),
            position_separated: Some([
                Some(track.clone()),
                Some(track.clone()),
                Some(track.clone()),
            ]),
            scale: Some(track.clone()),
            orientation: Some(track.clone()),
            rotation_x: Some(track.clone()),
            rotation_y: Some(track.clone()),
            rotation_z: Some(track.clone()),
            opacity: Some(track),
        };
        let duration = Duration24::from_frames(48).unwrap();
        let layer = LayerSpec::Precomposition(empty_precomposition(duration));
        let options = native_options(Some(source_clock()));
        let rebased = finalized_transform3d_animations(&layer, &options, &animations).unwrap();

        for track in [
            rebased.anchor,
            rebased.position,
            rebased.scale,
            rebased.orientation,
            rebased.rotation_x,
            rebased.rotation_y,
            rebased.rotation_z,
            rebased.opacity,
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(track.keys[0].time_millis, 500);
        }
        for track in rebased.position_separated.unwrap().into_iter().flatten() {
            assert_eq!(track.keys[0].time_millis, 500);
        }
    }

    fn frame_blended_video() -> LayerSpec {
        let source = super::super::footage::NativeSource {
            path: super::super::footage::RelativeMediaPath::new("media/clip.mov").unwrap(),
            format: super::super::footage::NativeSourceFormat::QuickTime,
            dimensions: [1920, 1080],
            duration_millis: 1_000,
            frame_rate: super::super::footage::NativeFrameRate::integer(24),
            audio_sample_rate: 0.0,
            wave_metadata: None,
        };
        LayerSpec::Footage(
            super::super::footage::FootageSpec {
                name: "clip.mov".into(),
                kind: super::super::footage::FootageKind::Video,
                source,
                source_geometry: super::super::footage::SourceGeometry::default(),
                transform: SolidLayerSpec {
                    name: "clip.mov".into(),
                    width: 1920,
                    height: 1080,
                    color: [0.0; 3],
                    transform: identity_transform(),
                },
                clock: super::super::footage::FootageClock::Source(
                    super::super::source_clock::SourceClockPlan::affine(
                        fx_schema::TimeRangeProperty::new(
                            fx_schema::Time::ZERO,
                            fx_schema::Duration::from_millis(1_000),
                        ),
                        fx_schema::Time::ZERO,
                        fx_schema::Time::from_millis(1_000),
                        1_000,
                    )
                    .unwrap(),
                ),
                static_source_time_secs: None,
                time_remap_requires_source_owned_transform: false,
                frame_blending: super::super::footage::NativeFrameBlending::PixelMotion,
                audio_enabled: false,
                audio_levels_db: [0.0; 2],
                audio_levels_animation: None,
            },
            TransformAnimations::default(),
        )
    }

    #[test]
    fn native_source_dedup_includes_exact_wave_metadata() {
        let LayerSpec::Footage(mut footage, _) = frame_blended_video() else {
            panic!("footage fixture");
        };
        let source = &mut footage.source;
        source.format = super::super::footage::NativeSourceFormat::Wave;
        source.path = super::super::footage::RelativeMediaPath::new("media/sound.wav").unwrap();
        source.dimensions = [0, 0];
        source.duration_millis = 5944;
        source.frame_rate = super::super::footage::NativeFrameRate::integer(0);
        source.audio_sample_rate = 44_100.0;
        let approximate = NativeSourceKey::from(&*source);
        source.wave_metadata = Some(super::super::footage::NativeWaveMetadata {
            sample_frames: 262_094,
            file_length: 1_048_558,
        });
        let exact = NativeSourceKey::from(&*source);
        assert!(
            approximate != exact,
            "an approximate header cannot alias an exact source"
        );
        source.wave_metadata.as_mut().unwrap().sample_frames += 1;
        assert!(
            exact != NativeSourceKey::from(&*source),
            "ceil milliseconds lose sample identity"
        );
        source.wave_metadata.as_mut().unwrap().sample_frames -= 1;
        source.wave_metadata.as_mut().unwrap().file_length += 4;
        assert!(
            exact != NativeSourceKey::from(&*source),
            "file length is serialized source data"
        );
    }

    #[test]
    fn selected_rate_reaches_footage_and_precomposition_time_remap() {
        use fx_schema::{
            Duration, KeyframeId, PropertyKeyframeEasing, Time, TimeRangeProperty,
            TimeRemapExtrapolation, TimeRemapKeyframe, TimeRemapProperty,
        };
        let rate = crate::timing::FrameRate::new(30.0).unwrap();
        let clock = super::super::keyframes::PropertyClock::for_rate(rate).unwrap();
        let remap = TimeRemapProperty::new(
            [(0, 0), (1100, 700), (3000, 900)]
                .into_iter()
                .map(|(time, value)| TimeRemapKeyframe {
                    id: KeyframeId::new(time.to_string()),
                    time: Time::from_millis(time),
                    value: Time::from_millis(value),
                    easing: PropertyKeyframeEasing::Hold,
                })
                .collect(),
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap();
        let plan = super::super::source_clock::SourceClockPlan::time_remap_with_clock(
            TimeRangeProperty::new(Time::from_millis(1000), Duration::from_millis(1000)),
            &remap,
            1000,
            clock,
        )
        .unwrap();
        let mut video = frame_blended_video();
        let LayerSpec::Footage(spec, _) = &mut video else {
            panic!("footage fixture")
        };
        spec.clock = super::super::footage::FootageClock::Source(plan.clone());
        let mut options = native_options(None);
        options.source_clock = Some(plan);
        let duration = Duration24::from_frames(48).unwrap();
        let precomp = LayerSpec::Options(
            Box::new(LayerSpec::Precomposition(empty_precomposition(duration))),
            options,
        );
        let timeline =
            build_timeline_at_rate(&[video, precomp], duration, [320, 180], rate).unwrap();
        fn remap_property(chunks: &[Chunk]) -> Option<&[Chunk]> {
            if let Ok(runs) = crate::properties::runs(chunks) {
                for (name, run) in runs {
                    if name == "ADBE Time Remapping" {
                        return Some(crate::properties::unique_list(run, *b"tdbs").unwrap());
                    }
                }
            }
            chunks
                .iter()
                .filter_map(Chunk::children)
                .find_map(remap_property)
        }
        let layers = timeline
            .layers
            .iter()
            .filter(|chunk| chunk.list_kind() == Some(*b"Layr"))
            .collect::<Vec<_>>();
        assert_eq!(layers.len(), 2);
        for (index, layer) in layers.into_iter().enumerate() {
            let property = remap_property(layer.children().unwrap())
                .unwrap_or_else(|| panic!("missing Time Remap on root layer {index}"));
            let descriptor = property
                .iter()
                .find(|chunk| chunk.id() == *b"tdb4")
                .unwrap()
                .data_payload()
                .unwrap();
            assert_eq!(
                u32::from_be_bytes(descriptor[12..16].try_into().unwrap()),
                30_720
            );
            let numeric = crate::properties::read_numeric(property).unwrap();
            assert_eq!(numeric.keyframes.len(), 3);
            assert!((numeric.keyframes[1].time_secs - 0.1).abs() < 1e-9);
            assert_eq!(numeric.keyframes[1].values, [0.7]);
        }
    }

    #[test]
    fn frame_blending_sets_layer_and_owning_ancestor_composition_masters() {
        let nested_duration = Duration24::from_frames(24).unwrap();
        let spec = CompositionSpec {
            name: "Root".into(),
            width: 320,
            height: 180,
            duration_frames: 48,
        };
        let bytes = write_composition(
            &spec,
            &[LayerSpec::Precomposition(PrecompositionSpec {
                layers: vec![frame_blended_video()],
                ..empty_precomposition(nested_duration)
            })],
        )
        .unwrap();
        let project = crate::structure::read_project(&bytes).unwrap();
        let crate::structure::ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
            panic!("root must be a composition");
        };
        assert_ne!(root.record.flags()[1] & 16, 0);
        let source_id = root.layers[0].record.source_id();
        let crate::structure::ItemKind::Composition(nested) =
            &project.item(source_id).unwrap().kind
        else {
            panic!("source must be a composition");
        };
        assert_ne!(nested.record.flags()[1] & 16, 0);
        assert_eq!(nested.layers[0].record.frame_blending_type(), 2);
    }
}

#[cfg(test)]
mod tests;
