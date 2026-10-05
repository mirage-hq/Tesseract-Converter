//! Pure lowering from current editable FX media values to typed native footage.
//!
//! Archive lookup and byte publication stay in the adapter. This module never
//! treats an archive path as original-AEP provenance and never reads historical
//! AEP records.

use fx_schema::{
    AssetId, AudioLayer, Dimensions, FrameBlendingMode, ImageLayer, ImageSource, Layer, LayerData,
    LayerId, MediaFit, MediaSourceKind, Position, RectBounds, TimeRangeProperty, TimeRemapProperty,
    Transform, VideoLayer,
    layer::{BlendMode, FrameBlendingData, LegacyMediaData, MediaFitData, TrackMatteType},
};

use crate::writer::{
    AepWriteError, NativeMaskSpec, SolidLayerSpec, SolidTransform,
    footage::{
        FootageClock, FootageKind, FootageSpec, NativeFrameBlending, NativeFrameRate, NativeSource,
        NativeSourceFormat, NativeWaveMetadata, RelativeMediaPath, SourceGeometry,
    },
};

use super::media_clock;

/// Asset and semantic facts needed before archive bytes are staged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MediaRequest {
    pub(crate) layer_id: LayerId,
    pub(crate) asset_id: AssetId,
    pub(crate) kind: FootageKind,
    pub(crate) preferred_name: String,
}

/// Metadata established from current FX plus a validated archive asset.
///
/// The adapter must derive these values from the selected source format; it
/// must not fill unknown values with generic defaults.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedMediaSource {
    pub(crate) asset_id: AssetId,
    pub(crate) path: RelativeMediaPath,
    pub(crate) format: NativeSourceFormat,
    pub(crate) dimensions: [u16; 2],
    pub(crate) duration_millis: u64,
    /// Lower integer bound of the same interpreted source duration, not a tolerance.
    pub(crate) duration_millis_floor: u64,
    /// Exact QuickTime duration in 24576 Hz source ticks, when representable.
    pub(crate) duration_native_ticks: Option<u64>,
    /// Exact native integer-plus-16-bit-fraction source rate. E2 must preserve
    /// this from bounded QuickTime metadata rather than rounding to an integer.
    pub(crate) frame_rate: NativeFrameRate,
    pub(crate) audio_sample_rate: f64,
    /// Original RIFF/WAVE sample and byte counts; absent for all other media.
    pub(crate) wave_metadata: Option<NativeWaveMetadata>,
    /// Independently established native movie source duration, without ms rounding.
    pub(crate) native_duration: Option<crate::media::MediaDuration>,
}

/// Returns the archive asset requested by a supported current media layer.
pub(crate) fn request(layer: &Layer) -> Option<MediaRequest> {
    let (asset_id, kind, name) = match layer.data() {
        LayerData::Image(image) => {
            let ImageSource::Asset(source) = &image.source;
            (&source.asset_id, FootageKind::Image, &image.name)
        }
        LayerData::Video(video) => (&video.source.asset_id, FootageKind::Video, &video.name),
        LayerData::Audio(audio) => (&audio.source.asset_id, FootageKind::Audio, &audio.name),
        LayerData::Media(media) => (
            &media.source.asset_id,
            match media.source.kind {
                MediaSourceKind::Image => FootageKind::Image,
                MediaSourceKind::Video => FootageKind::Video,
            },
            &media.name,
        ),
        _ => return None,
    };
    Some(MediaRequest {
        layer_id: layer.id(),
        asset_id: asset_id.clone(),
        kind,
        preferred_name: name.clone(),
    })
}

/// Lowers one current FX layer after its archive asset has been resolved and
/// interpreted. Caller-owned IDs are allocated later by the shared writer.
pub(crate) fn lower(
    layer: &Layer,
    source: &ResolvedMediaSource,
    canvas: Dimensions,
) -> Result<FootageSpec, &'static str> {
    let selected = match layer.data() {
        LayerData::Image(value) => Some(&value.transform),
        LayerData::Video(value) => Some(&value.transform),
        LayerData::Media(value) => Some(&value.transform),
        LayerData::Audio(_) => None,
        _ => return Err("layer is not source-backed media"),
    };
    lower_inner(layer, source, canvas, selected)
}

/// Lowers media with the transform selected by hierarchy ownership.
///
/// Native 3D fields are intentionally tolerated here because the shared core
/// owns them in the typed 3D sidecar. This function authors only the 2D base
/// needed to construct footage and returns the real source geometry.
pub(crate) fn lower_with_transform(
    layer: &Layer,
    source: &ResolvedMediaSource,
    canvas: Dimensions,
    selected_transform: &Transform,
) -> Result<FootageSpec, &'static str> {
    lower_inner(layer, source, canvas, Some(selected_transform))
}

fn lower_inner(
    layer: &Layer,
    source: &ResolvedMediaSource,
    canvas: Dimensions,
    selected_transform: Option<&Transform>,
) -> Result<FootageSpec, &'static str> {
    check_resolved_source(layer, source)?;
    match layer.data() {
        LayerData::Image(image) => lower_image(
            image,
            source,
            selected_transform.ok_or("visual media has no selected Transform")?,
        ),
        LayerData::Video(video) => lower_video(
            video,
            source,
            selected_transform.ok_or("visual media has no selected Transform")?,
        ),
        LayerData::Audio(audio) => lower_audio(audio, source, canvas),
        LayerData::Media(media) => lower_legacy(
            media,
            source,
            selected_transform.ok_or("visual media has no selected Transform")?,
        ),
        _ => Err("layer is not source-backed media"),
    }
}

/// Returns the source-local rectangle mask required by this media occurrence.
///
/// The returned mask is `Add`, which is correct when it is the only mask. A
/// caller combining it with authored masks must change it to `Intersect` and
/// append it last in the same mask parade.
pub(crate) fn crop_mask(
    layer: &Layer,
    source: &ResolvedMediaSource,
) -> Result<Option<NativeMaskSpec>, &'static str> {
    check_resolved_source(layer, source)?;
    let geometry = match layer.data() {
        LayerData::Image(image) => {
            let ImageSource::Asset(image_source) = &image.source;
            source_geometry(
                source.dimensions,
                image_source.frame_rect.map(|frame| frame.get()),
                image_source.fit,
            )?
        }
        LayerData::Video(video) => source_geometry(
            source.dimensions,
            video.source.frame_rect.map(|frame| frame.get()),
            video.source.fit,
        )?,
        LayerData::Media(media) => source_geometry(
            source.dimensions,
            media.source.source_rect.map(|frame| frame.get()),
            legacy_fit(media.source.fit.as_ref())?,
        )?,
        LayerData::Audio(_) => return Ok(None),
        _ => return Err("layer is not source-backed media"),
    };
    geometry.crop_rect.map_or(Ok(None), |rect| {
        NativeMaskSpec::crop_rectangle("Media Crop", source.dimensions.map(u32::from), rect)
            .map(Some)
    })
}

fn check_resolved_source(layer: &Layer, source: &ResolvedMediaSource) -> Result<(), &'static str> {
    let requested = request(layer).ok_or("layer is not source-backed media")?;
    if requested.asset_id == source.asset_id {
        Ok(())
    } else {
        Err("resolved archive asset does not match current FX source")
    }
}

fn lower_image(
    layer: &ImageLayer,
    source: &ResolvedMediaSource,
    selected_transform: &Transform,
) -> Result<FootageSpec, &'static str> {
    let ImageSource::Asset(image_source) = &layer.source;
    // Still visibility, blend, motion blur and matte belong to the shared
    // NativeLayerOptions envelope, including during bounds lowering. Video
    // and legacy media retain their separate source-admission guards.
    check_visual_options(
        false,
        false,
        true,
        // NativeLayerOptions already writes this image's editable matte link.
        true,
        layer.placement.is_none(),
        layer.captions_enabled.is_none(),
        layer.corner_radius,
    )?;
    if image_source.input_transform.is_some() {
        return Err("Image Input Transform is a color LUT and has no native effect mapping");
    }
    // Image source timeRemap is retained legacy metadata that current FX image
    // evaluation ignores. Keep ignoring it rather than changing still semantics.
    if !source.format.is_still()
        || source.duration_millis != 0
        || !source.frame_rate.is_zero()
        || source.audio_sample_rate != 0.0
    {
        return Err("resolved image does not have a pinned native still interpretation");
    }
    let geometry = source_geometry(
        source.dimensions,
        image_source.frame_rect.map(|frame| frame.get()),
        image_source.fit,
    )?;
    media_spec(
        &layer.name,
        FootageKind::Image,
        source,
        selected_transform,
        geometry.source,
        layer.active_range,
        None,
        None,
        None,
        None,
        NativeFrameBlending::Disabled,
        false,
        1.0,
    )
}

fn matches_source_intrinsic(authored: u64, source: &ResolvedMediaSource) -> bool {
    authored > 0
        && source.duration_millis_floor <= source.duration_millis
        && source.duration_millis - source.duration_millis_floor <= 1
        && (source.duration_millis_floor..=source.duration_millis).contains(&authored)
}

// Stale intrinsic metadata cannot affect this finite selection: every reachable
// source sample is inside both domains, with no source-owned effects/remap.
fn finite_video_selection_has_closed_source_domain(
    layer: &VideoLayer,
    source: &ResolvedMediaSource,
) -> bool {
    matches!(
        layer.playback.mapping(),
        fx_schema::LayerPlaybackMapping::Linear { .. }
    ) && layer.source.time_remap.is_none()
        && layer.effects.is_empty()
        && layer.source_range.duration.as_millis() > 0
        && layer.source_range.end().as_millis() <= layer.source_intrinsic_duration.as_millis()
        && layer.source_range.end().as_millis() <= source.duration_millis_floor
        && media_clock::plan_layer(
            &layer.playback,
            layer.source_range,
            None,
            source.duration_millis_floor,
        )
        .is_ok()
}

fn lower_video(
    layer: &VideoLayer,
    source: &ResolvedMediaSource,
    selected_transform: &Transform,
) -> Result<FootageSpec, &'static str> {
    check_visual_options(
        layer.is_hidden,
        // The shared NativeLayerOptions envelope authors Video motion blur,
        // including when this lowering is used to establish source bounds.
        false,
        matches!(
            layer.blend_mode,
            BlendMode::Normal | BlendMode::Multiply | BlendMode::Overlay | BlendMode::Screen
        ),
        // The shared NativeLayerOptions envelope writes the editable provider
        // link and matte mode. These additional Video profiles have pinned
        // independent native controls; source clocks remain guarded below.
        layer.track_matte.as_ref().is_none_or(|matte| {
            matches!(
                matte.mode,
                TrackMatteType::Alpha | TrackMatteType::Luma | TrackMatteType::LumaInverted
            )
        }),
        layer.placement.is_none(),
        layer.captions_enabled.is_none() && layer.caption_presentation.is_none(),
        layer.corner_radius,
    )?;
    check_start_time(layer.start_time, layer.source_range)?;
    if layer.preserve_audio_pitch {
        return Err("Video preserveAudioPitch has no established native footage clock option");
    }
    if layer.source.eye_contact.is_some() || layer.source.audio_enhancement.is_some() {
        return Err("Video generated-source substitutions have no native source-selection mapping");
    }
    if layer.source.input_transform.is_some() {
        return Err("Video Input Transform is a color LUT and has no native effect mapping");
    }
    if !matches!(
        source.format,
        NativeSourceFormat::QuickTime | NativeSourceFormat::QuickTimeProRes4444
    ) || source.duration_millis == 0
        || source.frame_rate.is_zero()
    {
        return Err("resolved video does not have the bounded QuickTime interpretation");
    }
    if !matches_source_intrinsic(layer.source_intrinsic_duration.as_millis(), source)
        && !finite_video_selection_has_closed_source_domain(layer, source)
    {
        return Err("FX video intrinsic duration differs from interpreted archive duration");
    }
    // Keep established ceil-ms occurrence admission. Only newly recognized
    // floor aliases need the additional exact physical sampling proof.
    if layer.source_intrinsic_duration.as_millis() != source.duration_millis
        && let Some(ticks) = source.duration_native_ticks
    {
        media_clock::require_exact_video_source_domain(
            layer.source_range,
            Some(&layer.playback),
            None,
            layer.source.time_remap,
            ticks,
        )
        .map_err(clock_error)?;
    }
    let audio_enabled = layer.volume.is_some() && source.audio_sample_rate > 0.0;
    // Gain cannot create audio on an intrinsically silent source. Keep its
    // picture at every authored gain and leave the native audio switch off.
    let geometry = source_geometry(
        source.dimensions,
        layer.source.frame_rect.map(|frame| frame.get()),
        layer.source.fit,
    )?;
    media_spec(
        &layer.name,
        FootageKind::Video,
        source,
        selected_transform,
        geometry.source,
        layer.playback.input_range(),
        Some(layer.source_range),
        Some(&layer.playback),
        None,
        layer.source.time_remap,
        native_frame_blending(layer.frame_blending.as_ref()),
        audio_enabled,
        layer.volume.map_or(1.0, |gain| gain.as_f64()),
    )
}

fn lower_audio(
    layer: &AudioLayer,
    source: &ResolvedMediaSource,
    canvas: Dimensions,
) -> Result<FootageSpec, &'static str> {
    check_start_time(layer.start_time, layer.source_range)?;
    if layer.preserve_audio_pitch {
        return Err("Audio preserveAudioPitch has no established native footage clock option");
    }
    if layer.auto_ducking.is_some()
        || layer.metadata.is_some()
        || layer.captions_enabled == Some(true)
        || layer.caption_presentation.is_some()
        || layer.source.enhancement.is_some()
    {
        return Err(
            "Audio ducking, semantic metadata, captions or enhancement require unsupported native records",
        );
    }
    let wave = source.format == NativeSourceFormat::Wave
        && source.dimensions == [0, 0]
        && source.frame_rate.is_zero();
    let quicktime = matches!(
        source.format,
        NativeSourceFormat::QuickTime | NativeSourceFormat::QuickTimeProRes4444
    ) && !source.dimensions.contains(&0)
        && !source.frame_rate.is_zero();
    if (!wave && !quicktime) || source.duration_millis == 0 || source.audio_sample_rate <= 0.0 {
        return Err("resolved source has no supported native audio interpretation");
    }
    let authored_duration = layer.source_intrinsic_duration.as_millis();
    if authored_duration != source.duration_millis
        && !(wave
            && authored_duration.checked_add(1) == Some(source.duration_millis)
            && layer.source_range.end().as_millis() <= authored_duration)
    {
        return Err("FX audio intrinsic duration differs from interpreted archive duration");
    }
    let transform = Transform {
        anchor_point: [0.5, 0.5],
        position: Position::TwoD([
            f64::from(canvas.width) / 2.0,
            f64::from(canvas.height) / 2.0,
        ]),
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0; 3],
        opacity: fx_schema::PercentageProperty::new(100.0).expect("100 is a valid percentage"),
    };
    media_spec(
        &layer.name,
        FootageKind::Audio,
        source,
        &transform,
        SourceGeometry::default(),
        layer.playback.input_range(),
        Some(layer.source_range),
        Some(&layer.playback),
        None,
        None,
        NativeFrameBlending::Disabled,
        !layer.is_hidden,
        layer.volume.as_f64(),
    )
}

fn lower_legacy(
    layer: &LegacyMediaData,
    source: &ResolvedMediaSource,
    selected_transform: &Transform,
) -> Result<FootageSpec, &'static str> {
    check_visual_options(
        layer.is_hidden,
        layer.motion_blur,
        layer.blend_mode == Default::default(),
        layer.track_matte.is_none(),
        layer.placement.is_none(),
        layer.captions_enabled.is_none() && layer.caption_presentation.is_none(),
        layer.corner_radius,
    )?;
    if layer.source.input_transform.is_some() {
        return Err("Legacy Media Input Transform has no native LUT effect mapping");
    }
    let fit = legacy_fit(layer.source.fit.as_ref())?;
    let geometry = source_geometry(
        source.dimensions,
        layer.source.source_rect.map(|frame| frame.get()),
        fit,
    )?;
    match layer.source.kind {
        MediaSourceKind::Image => {
            if native_frame_blending(layer.frame_blending.as_ref()) != NativeFrameBlending::Disabled
            {
                return Err("legacy image frame blending has no moving source semantics");
            }
            if !source.format.is_still()
                || source.duration_millis != 0
                || !source.frame_rate.is_zero()
                || source.audio_sample_rate != 0.0
            {
                return Err("legacy image source is not a pinned native still");
            }
            if layer.source_range.is_some() || layer.source_intrinsic_duration.is_some() {
                return Err("legacy image carries unexplained moving-source clocks");
            }
            media_spec(
                &layer.name,
                FootageKind::Image,
                source,
                selected_transform,
                geometry.source,
                layer.active_range,
                None,
                None,
                None,
                None,
                NativeFrameBlending::Disabled,
                false,
                1.0,
            )
        }
        MediaSourceKind::Video => {
            if !matches!(
                source.format,
                NativeSourceFormat::QuickTime | NativeSourceFormat::QuickTimeProRes4444
            ) || source.duration_millis == 0
                || source.frame_rate.is_zero()
            {
                return Err("legacy video source is not bounded QuickTime");
            }
            let source_range = layer
                .source_range
                .ok_or("legacy video has no explicit source range")?;
            let intrinsic = layer
                .source_intrinsic_duration
                .ok_or("legacy video has no explicit intrinsic duration")?;
            if !matches_source_intrinsic(intrinsic.as_millis(), source) {
                return Err("legacy video intrinsic duration differs from archive duration");
            }
            check_start_time(layer.start_time, source_range)?;
            if intrinsic.as_millis() != source.duration_millis
                && let Some(ticks) = source.duration_native_ticks
            {
                media_clock::require_exact_video_source_domain(
                    source_range,
                    None,
                    layer.playback.as_ref(),
                    layer.source.time_remap,
                    ticks,
                )
                .map_err(clock_error)?;
            }
            let audio_enabled = layer.volume.is_some();
            if audio_enabled && source.audio_sample_rate <= 0.0 {
                return Err("legacy video enables audio without a supported audio track");
            }
            media_spec(
                &layer.name,
                FootageKind::Video,
                source,
                selected_transform,
                geometry.source,
                layer.active_range,
                Some(source_range),
                None,
                layer.playback.as_ref(),
                layer.source.time_remap,
                native_frame_blending(layer.frame_blending.as_ref()),
                audio_enabled,
                layer.volume.map_or(1.0, |gain| gain.as_f64()),
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_visual_options(
    hidden: bool,
    motion_blur: bool,
    normal_blend: bool,
    no_track_matte: bool,
    no_placement: bool,
    no_captions: bool,
    corner_radius: Option<f64>,
) -> Result<(), &'static str> {
    if hidden || motion_blur || !normal_blend || !no_track_matte || !no_captions {
        return Err("Media visibility, compositing or captions require other native records");
    }
    if !no_placement {
        return Err(
            "Media takeover placement needs a 0.12-second linear occurrence slide composed after Transform; placed Cover images can also animate content inside the static frame, and video slide-out tails hold the end sample",
        );
    }
    if corner_radius.is_some_and(|radius| radius != 0.0) {
        return Err("Media corner radius needs C2's typed same-layer native mask");
    }
    Ok(())
}

fn legacy_fit(fit: Option<&MediaFitData>) -> Result<MediaFit, &'static str> {
    match fit {
        Some(MediaFitData::Contain) => Ok(MediaFit::Contain),
        Some(MediaFitData::Cover) => Ok(MediaFit::Cover),
        Some(MediaFitData::Stretch) => Ok(MediaFit::Stretch),
        Some(MediaFitData::Custom {
            scale,
            content_center,
        }) => MediaFit::custom(scale.get(), content_center.get()),
        Some(MediaFitData::None) | None => {
            Err("legacy media fit is absent or retained as uninterpreted none")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MediaGeometry {
    source: SourceGeometry,
    /// Frame bounds mapped through the exact inverse source geometry.
    crop_rect: Option<[f64; 4]>,
}

fn source_geometry(
    natural: [u16; 2],
    frame: Option<RectBounds>,
    fit: MediaFit,
) -> Result<MediaGeometry, &'static str> {
    if natural.contains(&0) {
        return Err("visual media has zero natural dimensions");
    }
    let natural = [f64::from(natural[0]), f64::from(natural[1])];
    let frame = frame.unwrap_or_else(|| RectBounds::from_size(natural[0], natural[1]));
    let frame_center = [frame.x + frame.width / 2.0, frame.y + frame.height / 2.0];
    let (scale, center, may_crop) = match fit {
        MediaFit::Contain => {
            let scale = (frame.width / natural[0]).min(frame.height / natural[1]);
            ([scale; 2], frame_center, false)
        }
        MediaFit::Cover => {
            let scale = (frame.width / natural[0]).max(frame.height / natural[1]);
            ([scale; 2], frame_center, true)
        }
        MediaFit::Stretch => (
            [frame.width / natural[0], frame.height / natural[1]],
            frame_center,
            false,
        ),
        MediaFit::Custom {
            scale,
            content_center,
        } => (scale.get(), content_center.get(), true),
        MediaFit::None => return Err("media fit none is retained without interpretation"),
    };
    let size = [natural[0] * scale[0], natural[1] * scale[1]];
    let origin = [center[0] - size[0] / 2.0, center[1] - size[1] / 2.0];
    let frame_right = frame.x + frame.width;
    let frame_bottom = frame.y + frame.height;
    let content_right = origin[0] + size[0];
    let content_bottom = origin[1] + size[1];
    if scale
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || origin.iter().any(|value| !value.is_finite())
        || !frame_right.is_finite()
        || !frame_bottom.is_finite()
        || !content_right.is_finite()
        || !content_bottom.is_finite()
    {
        return Err("media fit geometry is non-finite or non-positive");
    }
    let clips_content = may_crop
        && (origin[0] < frame.x
            || origin[1] < frame.y
            || content_right > frame_right
            || content_bottom > frame_bottom);
    let crop_rect = clips_content.then(|| {
        [
            (frame.x - origin[0]) / scale[0],
            (frame.y - origin[1]) / scale[1],
            frame.width / scale[0],
            frame.height / scale[1],
        ]
    });
    if crop_rect.is_some_and(|rect| {
        rect.iter().any(|value| !value.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0
    }) {
        return Err("media crop inverse geometry is non-finite or non-positive");
    }
    Ok(MediaGeometry {
        source: SourceGeometry { origin, scale },
        crop_rect,
    })
}

#[allow(clippy::too_many_arguments)]
fn media_spec(
    name: &str,
    kind: FootageKind,
    source: &ResolvedMediaSource,
    transform: &Transform,
    source_geometry: SourceGeometry,
    active_range: TimeRangeProperty,
    source_range: Option<TimeRangeProperty>,
    playback: Option<&fx_schema::LayerPlayback>,
    legacy_playback: Option<&TimeRemapProperty>,
    static_source_time_secs: Option<f64>,
    frame_blending: NativeFrameBlending,
    audio_enabled: bool,
    gain: f64,
) -> Result<FootageSpec, &'static str> {
    if name.is_empty() || name.len() > 255 || name.contains('\0') {
        return Err("Native footage name must be 1..=255 UTF-8 bytes without NUL");
    }
    let position = match transform.position {
        Position::TwoD(value) => value,
        Position::ThreeD([x, y, _]) => [x, y],
    };
    if transform.skew != 0.0 || transform.skew_axis != 0.0 {
        return Err("Skew media Transform export is not implemented");
    }
    let (clock, static_source_time_secs, time_remap_requires_source_owned_transform) =
        if let Some(source_range) = source_range {
            let plan = if let Some(playback) = playback {
                if kind == FootageKind::Audio {
                    if static_source_time_secs.is_some() {
                        return Err("Audio cannot own a static source Time Remap");
                    }
                    media_clock::plan_audio_layer(playback, source_range, source.duration_millis)
                } else {
                    media_clock::plan_layer(
                        playback,
                        source_range,
                        static_source_time_secs,
                        source.duration_millis,
                    )
                }
            } else if kind == FootageKind::Audio {
                media_clock::plan_audio(
                    active_range,
                    source_range,
                    legacy_playback,
                    source.duration_millis,
                )
            } else {
                media_clock::plan(
                    active_range,
                    source_range,
                    legacy_playback,
                    static_source_time_secs,
                    source.duration_millis,
                )
            }
            .map_err(clock_error)?;
            (
                FootageClock::Source(plan.source_clock),
                plan.static_source_time_secs,
                plan.requires_source_owned_transform,
            )
        } else {
            if playback.is_some() || legacy_playback.is_some() || static_source_time_secs.is_some()
            {
                return Err("still media cannot own a moving source clock");
            }
            (
                FootageClock::Still {
                    start_millis: active_range.start.as_millis(),
                    duration_millis: active_range.duration.as_millis(),
                },
                None,
                false,
            )
        };
    let db = gain_to_db(gain)?;
    let dimensions = match kind {
        FootageKind::Audio => [1, 1],
        FootageKind::Image | FootageKind::Video => source.dimensions,
    };
    Ok(FootageSpec {
        name: name.to_owned(),
        kind,
        source: NativeSource {
            path: source.path.clone(),
            format: source.format,
            dimensions: source.dimensions,
            duration_millis: source.duration_millis,
            duration_native_ticks: source.duration_native_ticks,
            frame_rate: source.frame_rate,
            audio_sample_rate: source.audio_sample_rate,
            wave_metadata: source.wave_metadata,
            native_duration: source.native_duration,
        },
        source_geometry,
        transform: SolidLayerSpec {
            name: name.to_owned(),
            width: dimensions[0],
            height: dimensions[1],
            color: [0.0; 3],
            transform: SolidTransform {
                // Keep the current FX basis here so parent-owned graph lowering
                // receives the correct defaults. The writer composes this basis
                // with `source_geometry` for static values and native keys.
                anchor: transform.anchor_point,
                position,
                scale: transform.scale,
                rotation: transform.rotation,
                opacity: transform.opacity.value(),
            },
        },
        clock,
        static_source_time_secs,
        time_remap_requires_source_owned_transform,
        frame_blending,
        audio_enabled,
        audio_levels_db: [db; 2],
        audio_levels_animation: None,
    })
}

pub(super) fn check_start_time(
    start_time_secs: Option<f64>,
    source_range: TimeRangeProperty,
) -> Result<(), &'static str> {
    let Some(start_time_secs) = start_time_secs else {
        return Ok(());
    };
    let source_start_secs = source_range.start.as_millis() as f64 / 1_000.0;
    if start_time_secs.is_finite() && start_time_secs == source_start_secs {
        Ok(())
    } else {
        Err("media startTime disagrees with the sourceRange in-point")
    }
}

fn native_frame_blending(value: Option<&FrameBlendingData>) -> NativeFrameBlending {
    match value {
        None | Some(FrameBlendingData::Boolean(false)) => NativeFrameBlending::Disabled,
        Some(FrameBlendingData::Boolean(true))
        | Some(FrameBlendingData::Mode(FrameBlendingMode::Simple)) => NativeFrameBlending::FrameMix,
        Some(FrameBlendingData::Mode(FrameBlendingMode::OpticalFlow)) => {
            NativeFrameBlending::PixelMotion
        }
    }
}

fn clock_error(error: AepWriteError) -> &'static str {
    match error {
        AepWriteError::Invalid(message) => message,
        AepWriteError::NoConvertiblePicture(_) => "selected AEP scope has no convertible picture",
        AepWriteError::InvalidDocument(_) => {
            "media source clock encountered invalid document planning"
        }
        AepWriteError::Binary(_) | AepWriteError::Record(_) => {
            "media source clock could not be represented by the native writer"
        }
    }
}

pub(super) fn gain_to_db(gain: f64) -> Result<f64, &'static str> {
    if !gain.is_finite() || gain < 0.0 {
        return Err("audio gain must be finite and non-negative");
    }
    // AE's finite floor is also the importer's silence threshold. Keep tiny
    // positive gains from inverting a fade relative to mathematical zero.
    Ok(if gain == 0.0 {
        -192.0
    } else {
        (20.0 * gain.log10()).max(-192.0)
    })
}

/// Converts writer validation into a stable contextual diagnostic boundary.
pub(crate) fn validate_native(spec: &FootageSpec) -> Result<(), AepWriteError> {
    crate::writer::footage::validate(spec)
}

#[cfg(test)]
#[path = "media/audio_captions.rs"]
mod audio_captions;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_video_intrinsic_only_admits_a_closed_finite_selection() {
        let layer: Layer = serde_json::from_value(serde_json::json!({
            "type":"Video", "id":10, "name":"finite selection", "source":{"assetId":"clip","fit":"contain"},
            "playback":{"type":"windowed","inputRange":{"start":2600,"duration":9000},"inputOffsetMs":0,
                    "mapping":{"type":"linear","input":{"start":2600,"duration":9000},"output":{"start":0,"duration":9000}}},
            "sourceRange":{"start":0,"duration":9000}, "sourceIntrinsicDuration":10125,
            "transform":{"position":[0,0],"anchorPoint":[0,0],"scale":[100,100],"rotation":0,"opacity":100}
        })).unwrap();
        let LayerData::Video(mut video) = layer.data().clone() else {
            panic!("video required")
        };
        let source = ResolvedMediaSource {
            asset_id: video.source.asset_id.clone(),
            path: RelativeMediaPath::new("media/clip.mp4").unwrap(),
            format: NativeSourceFormat::QuickTime,
            dimensions: [3840, 2160],
            duration_millis: 10042,
            duration_millis_floor: 10041,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(24),
            native_duration: None,
            audio_sample_rate: 0.0,
            wave_metadata: None,
        };
        assert!(lower_video(&video, &source, &video.transform).is_ok());
        video.source_intrinsic_duration = fx_schema::Duration::from_millis(8000);
        assert!(lower_video(&video, &source, &video.transform).is_err());
        video.source_intrinsic_duration = fx_schema::Duration::from_millis(10125);
        let mut short = source.clone();
        short.duration_millis = 8000;
        short.duration_millis_floor = 8000;
        assert!(lower_video(&video, &short, &video.transform).is_err());
        video.source.time_remap = Some(1.0);
        assert!(lower_video(&video, &source, &video.transform).is_err());
    }

    #[test]
    fn video_motion_blur_reaches_the_existing_native_options() {
        let layer: Layer = serde_json::from_value(serde_json::json!({
            "type":"Video", "id":10, "name":"motion-blurred portal",
            "source":{"assetId":"clip","fit":"cover"},
            "playback":{"type":"windowed","inputRange":{"start":0,"duration":1000},"inputOffsetMs":0,
                "mapping":{"type":"linear","input":{"start":0,"duration":1000},"output":{"start":0,"duration":1000}}},
            "sourceRange":{"start":0,"duration":1000}, "sourceIntrinsicDuration":1000,
            "motionBlur":true, "trackMatte":{"mode":"alpha","layer":11},
            "transform":{"position":[0,0],"anchorPoint":[0,0],"scale":[100,100],"rotation":0,"opacity":100}
        })).unwrap();
        let LayerData::Video(mut video) = layer.data().clone() else {
            panic!("video required")
        };
        let source = ResolvedMediaSource {
            asset_id: video.source.asset_id.clone(),
            path: RelativeMediaPath::new("media/clip.mp4").unwrap(),
            format: NativeSourceFormat::QuickTime,
            dimensions: [320, 180],
            duration_millis: 1000,
            duration_millis_floor: 1000,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(30),
            native_duration: None,
            audio_sample_rate: 0.0,
            wave_metadata: None,
        };
        assert!(lower_video(&video, &source, &video.transform).is_ok());
        let options = super::super::native_layer_options(&layer).unwrap();
        assert!(options.motion_blur);
        let matte = options.matte.unwrap();
        assert_eq!(matte.layer, 11.into());
        assert_eq!(matte.mode, 1);

        video.is_hidden = true;
        assert!(lower_video(&video, &source, &video.transform).is_err());
    }

    #[test]
    fn p004_silent_screen_video_retains_its_closed_source_selection() {
        let layer: Layer = serde_json::from_value(serde_json::json!({
            "type":"Video", "id":66, "name":"public flare selection",
            "source":{"assetId":"public-flare", "fit":"stretch"},
            "playback":{"type":"windowed", "inputRange":{"start":1667,"duration":300},"inputOffsetMs":0,
                "mapping":{"type":"linear","input":{"start":1667,"duration":300},"output":{"start":0,"duration":366}}},
            "sourceRange":{"start":0,"duration":366}, "sourceIntrinsicDuration":366,
            "blendMode":"screen", "motionBlur":true, "frameBlending":true, "volume":0.0,
            "transform":{"position":[0,0],"anchorPoint":[0,0],"scale":[100,100],"rotation":0,"opacity":100}
        })).unwrap();
        let LayerData::Video(mut video) = layer.data().clone() else {
            panic!("video required")
        };
        let source = ResolvedMediaSource {
            asset_id: video.source.asset_id.clone(),
            path: RelativeMediaPath::new("media/public-flare.mp4").unwrap(),
            format: NativeSourceFormat::QuickTime,
            dimensions: [1080, 1920],
            duration_millis: 375,
            duration_millis_floor: 375,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(120),
            native_duration: None,
            audio_sample_rate: 0.0,
            wave_metadata: None,
        };
        let footage = lower_video(&video, &source, &video.transform).unwrap();
        assert!(!footage.audio_enabled);
        assert_eq!(footage.frame_blending, NativeFrameBlending::FrameMix);
        let options = super::super::native_layer_options(&layer).unwrap();
        assert_eq!(options.blend_mode, 6);
        assert!(options.motion_blur);
        video.source_intrinsic_duration = fx_schema::Duration::from_millis(300);
        assert!(lower_video(&video, &source, &video.transform).is_err());
        video.source_intrinsic_duration = fx_schema::Duration::from_millis(366);
        let mut short = source.clone();
        short.duration_millis_floor = 300;
        short.duration_millis = 300;
        assert!(lower_video(&video, &short, &video.transform).is_err());
        video.source.time_remap = Some(0.0);
        assert!(lower_video(&video, &source, &video.transform).is_err());
        video.source.time_remap = None;
        // Normal motion blur is already supported on main; keep an unsupported blend guard.
        video.blend_mode = BlendMode::Difference;
        assert!(lower_video(&video, &source, &video.transform).is_err());
    }

    #[test]
    fn contain_frame_becomes_exact_editable_source_trs() {
        let geometry = source_geometry(
            [200, 100],
            Some(RectBounds {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 100.0,
            }),
            MediaFit::Contain,
        )
        .unwrap();
        assert_eq!(geometry.source.origin, [10.0, 45.0]);
        assert_eq!(geometry.source.scale, [0.5, 0.5]);
        assert_eq!(geometry.crop_rect, None);
    }

    #[test]
    fn cover_crop_is_mapped_through_inverse_source_geometry() {
        let geometry = source_geometry(
            [200, 100],
            Some(RectBounds::from_size(100.0, 100.0)),
            MediaFit::Cover,
        )
        .unwrap();
        assert_eq!(geometry.source.origin, [-50.0, 0.0]);
        assert_eq!(geometry.source.scale, [1.0, 1.0]);
        assert_eq!(geometry.crop_rect, Some([50.0, 0.0, 100.0, 100.0]));
    }

    #[test]
    fn custom_crop_preserves_nonuniform_inverse_geometry() {
        let geometry = source_geometry(
            [100, 80],
            Some(RectBounds {
                x: 10.0,
                y: 20.0,
                width: 50.0,
                height: 40.0,
            }),
            MediaFit::custom([2.0, 0.5], [-20.0, 25.0]).unwrap(),
        )
        .unwrap();
        assert_eq!(geometry.source.origin, [-120.0, 5.0]);
        assert_eq!(geometry.source.scale, [2.0, 0.5]);
        assert_eq!(geometry.crop_rect, Some([65.0, 30.0, 25.0, 80.0]));
    }

    #[test]
    fn static_gain_conversion_keeps_mute_and_unity_exact() {
        assert_eq!(gain_to_db(0.0), Ok(-192.0));
        assert_eq!(gain_to_db(1.0), Ok(0.0));
    }

    #[test]
    fn source_known_frame_blending_lowers_to_typed_native_modes() {
        assert_eq!(native_frame_blending(None), NativeFrameBlending::Disabled);
        assert_eq!(
            native_frame_blending(Some(&FrameBlendingData::Boolean(true))),
            NativeFrameBlending::FrameMix
        );
        assert_eq!(
            native_frame_blending(Some(&FrameBlendingData::Mode(
                FrameBlendingMode::OpticalFlow,
            ))),
            NativeFrameBlending::PixelMotion
        );
    }

    #[test]
    fn retained_start_time_is_only_accepted_as_the_same_source_in_point() {
        let source = TimeRangeProperty::new(
            fx_schema::Time::from_millis(500),
            fx_schema::Duration::from_millis(1_000),
        );
        assert!(check_start_time(Some(0.5), source).is_ok());
        assert!(check_start_time(Some(0.75), source).is_err());
    }
}
