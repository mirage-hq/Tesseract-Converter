//! Fresh file-footage records backed by files packaged beside the generated AEP.
//!
//! The bounded layouts below are derived from the crate's typed `idta`/`ldta`
//! readers and the pinned native still/audio fixtures. They never retain source
//! project chunks. Newly generated output still requires independent Adobe
//! acceptance before it can be described as native-render verified.

use std::path::{Component, Path};

use crate::{
    rifx::Chunk,
    schema::{ItemRecord, layer_records::LayerRecord, panel_records::EmptyListHeader},
    timing::Duration24,
};

use super::{
    AepWriteError, NumericTrack,
    keyframes::PropertyClock,
    solids::{self, SolidLayerSpec, TransformAnimations},
    source_clock::SourceClockPlan,
    views::{self, ValueKind},
};

const TICKS_PER_SECOND: i128 = 24_576;
const MILLIS_PER_SECOND: i128 = 1_000;

/// A package-relative media path safe to resolve below the export directory.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RelativeMediaPath(String);

impl RelativeMediaPath {
    /// Validates a normalized slash-separated path rooted below `media/`.
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, AepWriteError> {
        let value = value.into();
        let path = Path::new(&value);
        let mut components = path.components();
        if value.contains('\0')
            || value.contains('\\')
            || path.is_absolute()
            || !matches!(components.next(), Some(Component::Normal(value)) if value == "media")
            || components.clone().next().is_none()
            || components.any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(AepWriteError::Invalid(
                "media path must be a normalized relative path below media/",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the normalized path encoded into the native alias record.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// File interpretations whose native source FourCC is established by a pinned
/// source or the existing bounded media reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeSourceFormat {
    /// OpenEXR still footage (`footage_not_missing.aep`).
    OpenExr,
    /// RIFF/WAVE audio footage (`audioEnabled.aep`).
    Wave,
    /// QuickTime movie footage (`MOoV` in the bounded reader corpus).
    QuickTime,
}

impl NativeSourceFormat {
    const fn fourcc(self) -> [u8; 4] {
        match self {
            Self::OpenExr => *b"oEXR",
            Self::Wave => *b"WAVE",
            Self::QuickTime => *b"MOoV",
        }
    }
}

/// Exact AE integer-plus-16-bit-fraction source frame rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeFrameRate {
    pub(crate) integer: u32,
    pub(crate) fractional: u16,
}

impl NativeFrameRate {
    /// Constructs the exact representation used by currently integrated
    /// package metadata, while leaving the fractional field available to E2.
    pub(crate) const fn integer(value: u32) -> Self {
        Self {
            integer: value,
            fractional: 0,
        }
    }

    pub(crate) const fn is_zero(self) -> bool {
        self.integer == 0 && self.fractional == 0
    }
}

/// Renderable channels requested from one packaged source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FootageKind {
    Image,
    Video,
    Audio,
}

/// Verified RIFF/WAVE facts that cannot be recovered from rounded FX durations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeWaveMetadata {
    pub(crate) sample_frames: u32,
    pub(crate) file_length: u32,
}

/// Native source interpretation authored into `Pin `/`sspc`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeSource {
    pub(crate) path: RelativeMediaPath,
    pub(crate) format: NativeSourceFormat,
    /// Natural square-pixel dimensions. Audio-only files use `[0, 0]`.
    pub(crate) dimensions: [u16; 2],
    /// Intrinsic source duration; still images use zero.
    pub(crate) duration_millis: u64,
    /// Exact native frame rate. Stills and audio-only files use zero.
    pub(crate) frame_rate: NativeFrameRate,
    /// Audio sample rate. Silent visual sources use zero.
    pub(crate) audio_sample_rate: f64,
    /// Exact facts from a validated RIFF/WAVE file, if available.
    pub(crate) wave_metadata: Option<NativeWaveMetadata>,
}

/// Static source-to-layer geometry absorbed into editable native Transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SourceGeometry {
    /// Layer-local location of the natural source's top-left pixel.
    pub(crate) origin: [f64; 2],
    /// Positive scale from natural source pixels to layer-local pixels.
    pub(crate) scale: [f64; 2],
}

impl Default for SourceGeometry {
    fn default() -> Self {
        Self {
            origin: [0.0, 0.0],
            scale: [1.0, 1.0],
        }
    }
}

/// A finalized native occurrence clock. Moving media uses the shared exact
/// source-clock contract; stills use an occurrence-local active span.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FootageClock {
    Still {
        start_millis: u64,
        duration_millis: u64,
    },
    Source(SourceClockPlan),
}

/// Typed layer-level frame blending. The composition master is separate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NativeFrameBlending {
    #[default]
    Disabled,
    FrameMix,
    PixelMotion,
}

impl NativeFrameBlending {
    const fn layer_flags(self) -> (bool, bool) {
        match self {
            Self::Disabled => (false, false),
            Self::FrameMix => (true, false),
            Self::PixelMotion => (true, true),
        }
    }

    /// The shared composition owner must set `cdta` flags byte 1 bit 4 when
    /// any occurrence returns true. This bit is established by the native
    /// import path (`CompositionRecord::flags()[1] & 16`).
    pub(crate) const fn requires_composition_master(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// One fresh source-backed image, video, or audio layer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FootageSpec {
    pub(crate) name: String,
    /// Occurrence role, intentionally separate from shared source identity.
    pub(crate) kind: FootageKind,
    pub(crate) source: NativeSource,
    pub(crate) source_geometry: SourceGeometry,
    pub(crate) transform: SolidLayerSpec,
    pub(crate) clock: FootageClock,
    /// Static source seconds for a non-animated `ADBE Time Remapping` leaf.
    /// Dynamic remaps are owned by `clock`'s `SourceClockPlan`.
    pub(crate) static_source_time_secs: Option<f64>,
    /// A native Time Remap owns the source clock; keyed occurrence Transform
    /// properties cannot follow it and must fail closed.
    pub(crate) time_remap_requires_source_owned_transform: bool,
    pub(crate) frame_blending: NativeFrameBlending,
    pub(crate) audio_enabled: bool,
    /// Static native Audio Levels, in decibels per channel.
    pub(crate) audio_levels_db: [f64; 2],
    pub(crate) audio_levels_animation: Option<NumericTrack>,
}

/// Validates the complete typed footage contract before IDs are published.
pub(crate) fn validate(spec: &FootageSpec, duration: Duration24) -> Result<(), AepWriteError> {
    solids::validate(&spec.transform)?;
    if spec.name != spec.transform.name {
        return Err(AepWriteError::Invalid(
            "footage source and layer names must agree",
        ));
    }
    let composition_end = match &spec.clock {
        FootageClock::Still {
            start_millis,
            duration_millis,
        } => {
            if *duration_millis == 0 {
                return Err(AepWriteError::Invalid("footage active range is empty"));
            }
            start_millis
                .checked_add(*duration_millis)
                .ok_or(AepWriteError::Invalid("footage active range overflowed"))?
        }
        FootageClock::Source(plan) => plan.active_range.end().as_millis(),
    };
    if ticks_from_millis_unsigned(composition_end)? > duration.signed_ticks() {
        return Err(AepWriteError::Invalid(
            "footage out-point exceeds composition duration",
        ));
    }
    if spec.static_source_time_secs.is_some() && !matches!(&spec.clock, FootageClock::Source(_)) {
        return Err(AepWriteError::Invalid(
            "static Time Remap requires a finalized source clock",
        ));
    }
    if spec.frame_blending.requires_composition_master() && spec.kind != FootageKind::Video {
        return Err(AepWriteError::Invalid(
            "frame blending is only valid for video footage",
        ));
    }
    if spec.audio_levels_db.iter().any(|value| !value.is_finite()) {
        return Err(AepWriteError::Invalid("non-finite audio level"));
    }
    if !spec.source.audio_sample_rate.is_finite() || spec.source.audio_sample_rate < 0.0 {
        return Err(AepWriteError::Invalid("invalid audio sample rate"));
    }
    if spec.source.format == NativeSourceFormat::Wave {
        wave_sample_clock(&spec.source)?;
    } else if spec.source.wave_metadata.is_some() {
        return Err(AepWriteError::Invalid(
            "exact WAVE metadata belongs only to WAVE sources",
        ));
    }
    if spec
        .source_geometry
        .origin
        .iter()
        .chain(&spec.source_geometry.scale)
        .any(|value| !value.is_finite())
        || spec.source_geometry.scale.iter().any(|value| *value <= 0.0)
    {
        return Err(AepWriteError::Invalid("invalid footage source geometry"));
    }
    let source_valid = match spec.source.format {
        NativeSourceFormat::OpenExr => {
            !spec.source.dimensions.contains(&0)
                && spec.source.duration_millis == 0
                && spec.source.frame_rate.is_zero()
                && spec.source.audio_sample_rate == 0.0
        }
        NativeSourceFormat::Wave => {
            spec.source.dimensions == [0, 0]
                && spec.source.duration_millis > 0
                && spec.source.frame_rate.is_zero()
                && spec.source.audio_sample_rate > 0.0
        }
        NativeSourceFormat::QuickTime => {
            !spec.source.dimensions.contains(&0)
                && spec.source.duration_millis > 0
                && !spec.source.frame_rate.is_zero()
        }
    };
    if !source_valid {
        return Err(AepWriteError::Invalid(
            "invalid native source interpretation",
        ));
    }
    let occurrence_valid = match spec.kind {
        FootageKind::Image => {
            spec.source.format == NativeSourceFormat::OpenExr && !spec.audio_enabled
        }
        FootageKind::Video => {
            spec.source.format == NativeSourceFormat::QuickTime
                && (!spec.audio_enabled || spec.source.audio_sample_rate > 0.0)
        }
        FootageKind::Audio => {
            matches!(
                spec.source.format,
                NativeSourceFormat::Wave | NativeSourceFormat::QuickTime
            ) && spec.source.audio_sample_rate > 0.0
        }
    };
    if occurrence_valid {
        Ok(())
    } else {
        Err(AepWriteError::Invalid(
            "native source cannot serve requested footage channels",
        ))
    }
}

/// Encodes a fresh footage project item with a packaged relative alias.
fn wave_sample_clock(source: &NativeSource) -> Result<(u32, u32), AepWriteError> {
    let sample_rate = source.audio_sample_rate;
    if !sample_rate.is_finite() || sample_rate <= 0.0 || sample_rate.fract() != 0.0 {
        return Err(AepWriteError::Invalid(
            "WAVE source sample rate must be integral",
        ));
    }
    if sample_rate > f64::from(u32::MAX) {
        return Err(AepWriteError::Invalid(
            "WAVE source sample rate exceeds native field",
        ));
    }
    let sample_rate = sample_rate as u32; // Integral and bounded by the native field above.
    let samples = if let Some(metadata) = source.wave_metadata {
        // At least 44 bytes for the minimal RIFF/WAVE PCM header plus one
        // byte per frame. The package parser has already checked block align.
        if metadata.sample_frames == 0
            || u64::from(metadata.file_length) < u64::from(metadata.sample_frames) + 44
        {
            return Err(AepWriteError::Invalid(
                "exact WAVE metadata has inconsistent file length",
            ));
        }
        let rounded_millis =
            (u128::from(metadata.sample_frames) * 1_000).div_ceil(u128::from(sample_rate));
        if rounded_millis != u128::from(source.duration_millis) {
            return Err(AepWriteError::Invalid(
                "exact WAVE metadata disagrees with source duration",
            ));
        }
        metadata.sample_frames
    } else {
        // Direct writer callers have only rounded FX milliseconds, not the
        // verified PCM frame count. This best-effort fallback can overstate
        // a 44.1-kHz source by up to 44 frames; no exact fidelity is implied.
        let samples = u128::from(source.duration_millis) * u128::from(sample_rate) / 1_000;
        u32::try_from(samples)
            .map_err(|_| AepWriteError::Invalid("WAVE source sample count exceeds native field"))?
    };
    if samples == 0 {
        return Err(AepWriteError::Invalid(
            "WAVE source duration rounds to zero samples",
        ));
    }
    Ok((samples, sample_rate))
}

pub(crate) fn source_item(spec: &FootageSpec, id: u32) -> Result<Chunk, AepWriteError> {
    if spec.source.format != NativeSourceFormat::Wave && spec.source.wave_metadata.is_some() {
        return Err(AepWriteError::Invalid(
            "exact WAVE metadata belongs only to WAVE sources",
        ));
    }
    let mut settings = [0_u8; 222];
    settings[22..26].copy_from_slice(&spec.source.format.fourcc());
    settings[32..34].copy_from_slice(&spec.source.dimensions[0].to_be_bytes());
    settings[36..38].copy_from_slice(&spec.source.dimensions[1].to_be_bytes());
    let (duration_units, duration_base) = if spec.source.format == NativeSourceFormat::Wave {
        wave_sample_clock(&spec.source)?
    } else {
        (
            u32::try_from(ticks_from_millis_unsigned(spec.source.duration_millis)?)
                .map_err(|_| AepWriteError::Invalid("source duration exceeds native field"))?,
            24_576,
        )
    };
    settings[38..42].copy_from_slice(&duration_units.to_be_bytes());
    settings[42..46].copy_from_slice(&duration_base.to_be_bytes());
    if let Some(metadata) = spec.source.wave_metadata {
        settings[208..212].copy_from_slice(&metadata.file_length.to_be_bytes());
    }
    settings[56..60].copy_from_slice(&spec.source.frame_rate.integer.to_be_bytes());
    settings[60..62].copy_from_slice(&spec.source.frame_rate.fractional.to_be_bytes());
    settings[136..140].copy_from_slice(&1_u32.to_be_bytes());
    settings[140..144].copy_from_slice(&1_u32.to_be_bytes());
    settings[152..154].copy_from_slice(
        &u16::try_from(spec.source.frame_rate.integer)
            .map_err(|_| AepWriteError::Invalid("frame rate exceeds native field"))?
            .to_be_bytes(),
    );
    settings[154..156].copy_from_slice(&spec.source.frame_rate.fractional.to_be_bytes());
    settings[160..168].copy_from_slice(&spec.source.audio_sample_rate.to_be_bytes());
    settings[188..196].fill(255); // No Photoshop layer ID/index.
    if spec.source.format == NativeSourceFormat::Wave {
        // Two independently Adobe-authored WAVE sources (audio_e2e/audio_cases
        // and media/audioEnabled) agree on these file-source defaults. The
        // native media reader only uses a subset of them; AE needs the full
        // source envelope, not the sparse reader-compatible synthetic header.
        settings[52..54].copy_from_slice(&600_u16.to_be_bytes());
        settings[64..66].copy_from_slice(&[1, 1]);
        settings[73] = 3;
        settings[79] = 1;
        settings[96..102].copy_from_slice(&[0, 3, 0, 4, 0, 2]);
        settings[112] = 8;
        settings[124..126].copy_from_slice(&12_u16.to_be_bytes());
        settings[128..130].copy_from_slice(&1_u16.to_be_bytes());
        settings[158..160].copy_from_slice(&8_u16.to_be_bytes());
        settings[196..200].copy_from_slice(&1_u32.to_be_bytes());
        settings[200..202].copy_from_slice(&2_u16.to_be_bytes());
        settings[212] = 1;
    }

    let alias = if spec.source.format == NativeSourceFormat::Wave {
        // AE's WAVE sources on macOS and Windows carry these alias metadata
        // keys even though our best-effort reader only needs `fullpath`. Do
        // not borrow an independent source's machine-specific path/server.
        serde_json::json!({
            "ascendcount_base": 1,
            // AE rebuilds the relative location from this many trailing
            // target components. One would discard `media/` and silently
            // mark the staged WAVE missing unless copied beside the AEP.
            "ascendcount_target": spec.source.path.as_str().split('/').count(),
            // Without the explicit relative prefix, AE parses `media` as a
            // macOS volume name (/Volumes/media), not a packaged directory.
            "fullpath": format!("./{}", spec.source.path.as_str()),
            "platform": 2,
            "server_name": "",
            "server_volume_name": "",
            "target_is_folder": false,
        })
    } else {
        serde_json::json!({
            "fullpath": spec.source.path.as_str(),
            "target_is_folder": false,
        })
    };
    let alias = serde_json::to_vec(&alias)
        .map_err(|_| AepWriteError::Invalid("media alias cannot be encoded"))?;
    let pin_children = if spec.source.format == NativeSourceFormat::Wave {
        // WAVE opti is an AE-owned 58-byte import-options descriptor, not an
        // optional empty marker. Encode the fields shared by both native
        // sources; omit file-specific hashes/stamps and profile bytes.
        let mut options = [0_u8; 58];
        options[..4].copy_from_slice(b"WAVE");
        options[4..6].copy_from_slice(&5_u16.to_be_bytes());
        options[6..10].copy_from_slice(&58_u32.to_be_bytes());
        options[30..34].copy_from_slice(b"EVAW");
        options[34..38].fill(255);
        options[46] = 1;
        vec![
            raw(*b"sspc", settings),
            raw(*b"Utf8", Vec::new()),
            Chunk::list(*b"Als2", vec![raw(*b"alas", alias)]),
            raw(*b"opti", options),
            raw(*b"pgui", [0; 16]),
            Chunk::list(
                *b"CLRS",
                vec![
                    raw(*b"epid", [255; 16]),
                    raw(*b"apid", [255; 16]),
                    raw(*b"linl", 2_u32.to_le_bytes()),
                    raw(*b"embp", [1]),
                    raw(*b"ipws", [0]),
                    raw(*b"Mcsp", [1]),
                    raw(*b"Utf8", Vec::new()),
                    raw(*b"ocsp", [1]),
                    raw(*b"Utf8", Vec::new()),
                    raw(*b"hdrm", [1]),
                    raw(*b"Utf8", b"{}".to_vec()),
                ],
            ),
            Chunk::list(
                *b"mnfo",
                vec![raw(*b"strt", [0, 0, 0, 0, 30, 0, 0, 0]), raw(*b"drop", [1])],
            ),
            raw(*b"Utf8", Vec::new()),
        ]
    } else {
        vec![
            raw(*b"sspc", settings),
            raw(*b"opti", Vec::new()),
            Chunk::list(*b"Als2", vec![raw(*b"alas", alias)]),
        ]
    };
    let mut item = vec![
        raw(*b"iide", id.to_le_bytes()),
        raw(*b"idpc", 0_u64.to_be_bytes()),
    ];
    let mut record = ItemRecord::solid_ae26(id)?.encode();
    if spec.source.format == NativeSourceFormat::Wave {
        // Native WAVE items use the real-footage identity (not Solid's
        // synthetic-source identity) on both independent fixture revisions.
        record[20..24].copy_from_slice(&4_u32.to_be_bytes());
        // Both independent WAVE fixtures use the same file-item capability
        // flags and report `hasAudio = true` in Adobe readback. The accepted
        // v5 probe instead put 7 in the neighboring byte and rendered a
        // composition with no output audio stream.
        record[58] = 7;
        record[60] = 0;
    }
    item.extend([
        raw(*b"idta", record),
        raw(*b"Utf8", spec.name.as_bytes().to_vec()),
        Chunk::list(*b"Pin ", pin_children),
    ]);
    if spec.source.format == NativeSourceFormat::Wave {
        item.extend([
            raw(
                *b"ftgi",
                [0_u32, 1, u32::MAX, 600]
                    .into_iter()
                    .flat_map(u32::to_be_bytes)
                    .collect::<Vec<_>>(),
            ),
            raw(*b"Utf8", Vec::new()),
            // File-backed WAVE sources carry an empty per-item guide list in
            // both independent native fixtures. The zero gdta status differs
            // from view-layer guides, while the lhd3 type is shared.
            Chunk::list(
                *b"Gide",
                vec![
                    raw(*b"gdta", [0; 8]),
                    Chunk::list(
                        *b"list",
                        vec![raw(*b"lhd3", EmptyListHeader::guides().encode())],
                    ),
                ],
            ),
        ]);
    }
    Ok(Chunk::list(*b"Item", item))
}

/// Encodes one editable AV layer, including source link, clocks, switches,
/// transform, and static/animated Audio Levels.
#[cfg(test)]
pub(crate) fn timeline_layer(
    spec: &FootageSpec,
    id: u32,
    source_id: u32,
    duration: Duration24,
    transform_animations: Option<&TransformAnimations>,
) -> Result<Chunk, AepWriteError> {
    timeline_layer_with_clock(
        spec,
        id,
        source_id,
        duration,
        transform_animations,
        PropertyClock::DEFAULT,
    )
}

pub(super) fn timeline_layer_with_clock(
    spec: &FootageSpec,
    id: u32,
    source_id: u32,
    duration: Duration24,
    transform_animations: Option<&TransformAnimations>,
    property_clock: PropertyClock,
) -> Result<Chunk, AepWriteError> {
    validate(spec, duration)?;
    let transformed_layer = transform_layer_geometry(&spec.transform, spec.source_geometry);
    solids::validate(&transformed_layer)?;
    let transformed_animations = transform_animations
        .map(|animations| {
            let mut animations = transform_source_geometry(animations, spec.source_geometry)?;
            if spec.time_remap_requires_source_owned_transform && has_transform_keys(&animations) {
                return Err(AepWriteError::Invalid(
                    "native Time Remap cannot drive occurrence-owned Transform keys",
                ));
            }
            if let FootageClock::Source(plan) = &spec.clock {
                rebase_transform_key_times(&mut animations, plan)?;
            }
            Ok(animations)
        })
        .transpose()?;

    let audio_levels_animation = spec
        .audio_levels_animation
        .as_ref()
        .map(|track| {
            if spec.time_remap_requires_source_owned_transform && track.keys.len() > 1 {
                return Err(AepWriteError::Invalid(
                    "native Time Remap cannot drive occurrence-owned Audio Levels keys",
                ));
            }
            let mut track = track.clone();
            if let FootageClock::Source(plan) = &spec.clock {
                plan.rebase_track_times(&mut track)?;
            }
            Ok(track)
        })
        .transpose()?;

    // Finalize the checked record before any keyed property is serialized.
    // The shared record owner supplies these two preserving setters.
    let record = match &spec.clock {
        FootageClock::Still {
            start_millis,
            duration_millis,
        } => LayerRecord::solid_ae26(id, source_id, duration)?.with_active_range(
            ticks_from_millis_unsigned(*start_millis)?,
            ticks_from_millis_unsigned(*duration_millis)?,
        )?,
        FootageClock::Source(plan) => {
            LayerRecord::solid_ae26(id, source_id, duration)?.with_source_clock(plan.record)?
        }
    };
    // Native WAVE layers retain the general AV switch and use the dedicated
    // audio switch for mute. Audio-only QuickTime occurrences instead clear
    // the general switch so their video channel stays disabled.
    let enabled = spec.kind != FootageKind::Audio || spec.source.format == NativeSourceFormat::Wave;
    let record = set_channels(record, enabled, spec.audio_enabled)?;
    let (frame_blending, pixel_motion) = spec.frame_blending.layer_flags();
    let record = record.with_frame_blending(frame_blending, pixel_motion)?;

    let properties = footage_properties(
        &transformed_layer,
        transformed_animations.as_ref(),
        &spec.audio_levels_db,
        audio_levels_animation.as_ref(),
        property_clock,
    )?;
    let mut layer = Chunk::list(
        *b"Layr",
        vec![
            raw(*b"ldta", record.encode()),
            raw(*b"Utf8", spec.name.as_bytes().to_vec()),
            properties,
        ],
    );
    if let FootageClock::Source(plan) = &spec.clock {
        plan.append_time_remap_property_with_clock(&mut layer, property_clock)?;
    }
    if let Some(source_secs) = spec.static_source_time_secs {
        append_static_time_remap(&mut layer, source_secs, property_clock)?;
    }
    Ok(layer)
}

fn set_channels(
    record: LayerRecord,
    enabled: bool,
    audio_enabled: bool,
) -> Result<LayerRecord, AepWriteError> {
    let mut bytes = record.encode();
    bytes[39] = (bytes[39] & !0b0000_0011) | u8::from(enabled) | (u8::from(audio_enabled) << 1);
    Ok(LayerRecord::decode(&bytes)?)
}

fn has_transform_keys(animations: &TransformAnimations) -> bool {
    animations.anchor.is_some()
        || animations.position.is_some()
        || animations.scale.is_some()
        || animations.rotation.is_some()
        || animations.opacity.is_some()
}

fn footage_properties(
    layer: &SolidLayerSpec,
    animations: Option<&TransformAnimations>,
    audio_levels_db: &[f64; 2],
    audio_levels_animation: Option<&NumericTrack>,
    property_clock: PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let transform = &layer.transform;
    let transform_group = views::group(
        1,
        "-_0_/-",
        vec![
            (
                "ADBE Anchor Point",
                views::property_with_clock(
                    ValueKind::Spatial,
                    &[transform.anchor[0], transform.anchor[1], 0.0],
                    None,
                    animations.and_then(|value| value.anchor.as_ref()),
                    property_clock,
                )?,
            ),
            (
                "ADBE Position",
                views::property_with_clock(
                    ValueKind::Spatial,
                    &[transform.position[0], transform.position[1], 0.0],
                    None,
                    animations.and_then(|value| value.position.as_ref()),
                    property_clock,
                )?,
            ),
            (
                "ADBE Scale",
                views::property_with_clock(
                    ValueKind::Scale,
                    &[transform.scale[0] / 100.0, transform.scale[1] / 100.0, 1.0],
                    Some((0.0, 0.0)),
                    animations.and_then(|value| value.scale.as_ref()),
                    property_clock,
                )?,
            ),
            (
                "ADBE Rotate Z",
                views::property_with_clock(
                    ValueKind::Angle,
                    &[transform.rotation],
                    None,
                    animations.and_then(|value| value.rotation.as_ref()),
                    property_clock,
                )?,
            ),
            (
                "ADBE Opacity",
                views::property_with_clock(
                    ValueKind::Scalar,
                    &[transform.opacity / 100.0],
                    Some((0.0, 100.0)),
                    animations.and_then(|value| value.opacity.as_ref()),
                    property_clock,
                )?,
            ),
        ],
    )?;
    views::group(
        1,
        "",
        vec![
            ("ADBE Transform Group", transform_group),
            (
                "ADBE Audio Group",
                views::group(
                    1,
                    "-_0_/-",
                    vec![(
                        "ADBE Audio Levels",
                        audio_levels_property(
                            audio_levels_db,
                            audio_levels_animation,
                            property_clock,
                        )?,
                    )],
                )?,
            ),
        ],
    )
    .map_err(AepWriteError::from)
}

// The pinned native WAVE Audio Levels control uses the ordinary pair value
// layout, but its envelope has discriminator 1, bounds 0..0, and 0x1ffff
// descriptor flags. The generic Pair recipe emits discriminator 3 and
// unbounded flags.
// Keep this local to file-footage rather than changing other pair properties.
fn audio_levels_property(
    values: &[f64; 2],
    animation: Option<&NumericTrack>,
    clock: PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut property =
        views::property_with_clock(ValueKind::Pair, values, Some((0.0, 0.0)), animation, clock)?;
    let children = property.children_mut().ok_or(AepWriteError::Invalid(
        "Audio Levels property is not a LIST",
    ))?;
    let discriminator = children
        .iter()
        .position(|chunk| chunk.id() == *b"tdsb")
        .ok_or(AepWriteError::Invalid("Audio Levels discriminator missing"))?;
    children[discriminator] = raw(*b"tdsb", 1_u32.to_be_bytes());
    let descriptor = children
        .iter()
        .position(|chunk| chunk.id() == *b"tdb4")
        .ok_or(AepWriteError::Invalid("Audio Levels descriptor missing"))?;
    let mut bytes = children[descriptor]
        .data_payload()
        .ok_or(AepWriteError::Invalid(
            "Audio Levels descriptor is not data",
        ))?
        .to_vec();
    bytes
        .get_mut(8..12)
        .ok_or(AepWriteError::Invalid(
            "Audio Levels descriptor is too short",
        ))?
        .copy_from_slice(&0x1ffff_u32.to_be_bytes());
    children[descriptor] = raw(*b"tdb4", bytes);
    Ok(property)
}

fn append_static_time_remap(
    layer: &mut Chunk,
    source_secs: f64,
    property_clock: PropertyClock,
) -> Result<(), AepWriteError> {
    if !source_secs.is_finite() || source_secs < 0.0 {
        return Err(AepWriteError::Invalid("invalid static source Time Remap"));
    }
    let children = layer.children_mut().ok_or(AepWriteError::Invalid(
        "static Time Remap target is not a fresh layer",
    ))?;
    let root = children
        .iter_mut()
        .find(|child| child.list_kind() == Some(*b"tdgp"))
        .ok_or(AepWriteError::Invalid(
            "static Time Remap target has no property root",
        ))?;
    let properties = root.children_mut().ok_or(AepWriteError::Invalid(
        "static Time Remap property root is opaque",
    ))?;
    let index = properties
        .len()
        .checked_sub(1)
        .ok_or(AepWriteError::Invalid(
            "static Time Remap property root is empty",
        ))?;
    properties.insert(index, views::name_payload("ADBE Time Remapping")?);
    properties.insert(
        index + 1,
        views::property_with_clock(
            ValueKind::Scalar,
            &[source_secs],
            None,
            None,
            property_clock,
        )?,
    );
    Ok(())
}

fn rebase_transform_key_times(
    animations: &mut TransformAnimations,
    clock: &SourceClockPlan,
) -> Result<(), AepWriteError> {
    for track in [
        &mut animations.anchor,
        &mut animations.position,
        &mut animations.scale,
        &mut animations.rotation,
        &mut animations.opacity,
    ]
    .into_iter()
    .flatten()
    {
        clock.rebase_track_times(track)?;
    }
    Ok(())
}

fn transform_layer_geometry(layer: &SolidLayerSpec, geometry: SourceGeometry) -> SolidLayerSpec {
    let mut layer = layer.clone();
    for axis in 0..2 {
        layer.transform.anchor[axis] =
            (layer.transform.anchor[axis] - geometry.origin[axis]) / geometry.scale[axis];
        layer.transform.scale[axis] *= geometry.scale[axis];
    }
    layer
}

fn transform_source_geometry(
    animations: &TransformAnimations,
    geometry: SourceGeometry,
) -> Result<TransformAnimations, AepWriteError> {
    if [&animations.anchor, &animations.scale]
        .into_iter()
        .flatten()
        .flat_map(|track| &track.keys)
        .any(|key| key.values.len() < 2)
    {
        return Err(AepWriteError::Invalid(
            "footage source-geometry Transform keys need at least two values",
        ));
    }
    let mut animations = animations.clone();
    if let Some(track) = &mut animations.anchor {
        for key in &mut track.keys {
            for axis in 0..2 {
                key.values[axis] =
                    (key.values[axis] - geometry.origin[axis]) / geometry.scale[axis];
                if let Some(value) = key.spatial_in.get_mut(axis) {
                    *value /= geometry.scale[axis];
                }
                if let Some(value) = key.spatial_out.get_mut(axis) {
                    *value /= geometry.scale[axis];
                }
            }
        }
    }
    if let Some(track) = &mut animations.scale {
        for key in &mut track.keys {
            for axis in 0..2 {
                key.values[axis] *= geometry.scale[axis];
            }
        }
    }
    Ok(animations)
}

fn ticks_from_millis_signed(millis: i64) -> Result<i32, AepWriteError> {
    let ticks = i128::from(millis)
        .checked_mul(TICKS_PER_SECOND)
        .ok_or(AepWriteError::Invalid("native clock overflow"))?
        / MILLIS_PER_SECOND;
    i32::try_from(ticks).map_err(|_| AepWriteError::Invalid("native clock exceeds i32"))
}

fn ticks_from_millis_unsigned(millis: u64) -> Result<i32, AepWriteError> {
    let millis =
        i64::try_from(millis).map_err(|_| AepWriteError::Invalid("native clock exceeds i64"))?;
    ticks_from_millis_signed(millis)
}

fn raw(tag: [u8; 4], bytes: impl Into<Vec<u8>>) -> Chunk {
    Chunk::data(tag, bytes).expect("footage writer raw tags are never LIST")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::read_project;
    use crate::writer::SolidTransform;

    #[test]
    fn package_paths_are_bounded_below_media() {
        assert!(RelativeMediaPath::new("media/clip.mov").is_ok());
        for invalid in [
            "clip.mov",
            "media",
            "media/../clip.mov",
            "/media/clip.mov",
            "media\\clip.mov",
        ] {
            assert!(RelativeMediaPath::new(invalid).is_err(), "{invalid}");
        }
    }

    fn quicktime_spec(kind: FootageKind, audio_enabled: bool) -> FootageSpec {
        let dimensions = if kind == FootageKind::Audio {
            [1, 1]
        } else {
            [1920, 1080]
        };
        FootageSpec {
            name: "shared.mov".to_owned(),
            kind,
            source: NativeSource {
                path: RelativeMediaPath::new("media/shared.mov").unwrap(),
                format: NativeSourceFormat::QuickTime,
                dimensions: [1920, 1080],
                duration_millis: 1_000,
                frame_rate: NativeFrameRate::integer(24),
                audio_sample_rate: 48_000.0,
                wave_metadata: None,
            },
            source_geometry: SourceGeometry::default(),
            transform: SolidLayerSpec {
                name: "shared.mov".to_owned(),
                width: dimensions[0],
                height: dimensions[1],
                color: [0.0; 3],
                transform: SolidTransform {
                    anchor: [0.0, 0.0],
                    position: [0.0, 0.0],
                    scale: [100.0, 100.0],
                    rotation: 0.0,
                    opacity: 100.0,
                },
            },
            clock: FootageClock::Source(
                SourceClockPlan::affine(
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
            frame_blending: NativeFrameBlending::Disabled,
            audio_enabled,
            audio_levels_db: [0.0; 2],
            audio_levels_animation: None,
        }
    }

    #[test]
    fn source_format_selects_native_audio_channel_switches() {
        fn flags(spec: &FootageSpec) -> crate::schema::layer_records::LayerFlags {
            let layer =
                timeline_layer(spec, 3, 2, Duration24::from_frames(48).unwrap(), None).unwrap();
            let record = layer
                .children()
                .unwrap()
                .iter()
                .find(|chunk| chunk.id() == *b"ldta")
                .unwrap();
            LayerRecord::decode(record.data_payload().unwrap())
                .unwrap()
                .flags()
        }

        let video = quicktime_spec(FootageKind::Video, false);
        let audio = quicktime_spec(FootageKind::Audio, true);
        assert_eq!(video.source, audio.source);

        let quicktime_audio = flags(&audio);
        assert!(!quicktime_audio.enabled);
        assert!(quicktime_audio.audio_enabled);

        let mut wave = audio;
        wave.source.format = NativeSourceFormat::Wave;
        wave.source.dimensions = [0, 0];
        wave.source.frame_rate = NativeFrameRate::integer(0);
        let audible_wave = flags(&wave);
        assert!(audible_wave.enabled);
        assert!(audible_wave.audio_enabled);

        wave.audio_enabled = false;
        let muted_wave = flags(&wave);
        assert!(muted_wave.enabled);
        assert!(!muted_wave.audio_enabled);

        fn source_flags(chunks: &[Chunk], source_id: u32, output: &mut Vec<(bool, bool)>) {
            for chunk in chunks {
                if chunk.list_kind() == Some(*b"Layr")
                    && let Some(record) = chunk
                        .children()
                        .and_then(|children| children.iter().find(|child| child.id() == *b"ldta"))
                    && let Some(bytes) = record.data_payload()
                    && let Ok(record) = LayerRecord::decode(bytes)
                    && record.source_id() == source_id
                {
                    let flags = record.flags();
                    output.push((flags.enabled, flags.audio_enabled));
                }
                if let Some(children) = chunk.children() {
                    source_flags(children, source_id, output);
                }
            }
        }

        // Independent WAVE occurrences retain the general switch whether the
        // audio switch is on or off.
        let native_wave = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/media/audioEnabled.aep"),
            |kind| kind == *b"tdgp",
        )
        .unwrap();
        let mut native_wave_flags = Vec::new();
        source_flags(native_wave.chunks(), 13, &mut native_wave_flags);
        native_wave_flags.sort_unstable();
        assert_eq!(native_wave_flags, vec![(true, false), (true, true)]);

        // Independently authored audio-only QuickTime occurrences clear only
        // the general switch, preserving their audio channel.
        let native_movie = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/audio_e2e/audio_cases.aep"),
            |kind| kind == *b"tdgp",
        )
        .unwrap();
        let mut native_movie_flags = Vec::new();
        source_flags(native_movie.chunks(), 3, &mut native_movie_flags);
        native_movie_flags.sort_unstable();
        assert_eq!(native_movie_flags, vec![(false, true), (false, true)]);
    }

    #[test]
    fn audio_levels_static_record_uses_native_control_metadata() {
        fn audio_levels(chunk: &Chunk) -> Option<&Chunk> {
            let children = chunk.children()?;
            for pair in children.windows(2) {
                if pair[0].id() == *b"tdmn"
                    && pair[0]
                        .data_payload()
                        .is_some_and(|name| name.starts_with(b"ADBE Audio Levels"))
                {
                    return Some(&pair[1]);
                }
            }
            children.iter().find_map(audio_levels)
        }
        // Parse tdgp rather than keeping it opaque, to inspect an independently
        // authored Audio Levels control envelope directly.
        let native = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/audio_e2e/audio_cases.aep"),
            |_| false,
        )
        .unwrap();
        fn find_static(chunks: &[Chunk]) -> Option<&Chunk> {
            for chunk in chunks {
                if let Some(control) = audio_levels(chunk)
                    && control
                        .children()
                        .is_some_and(|children| children.iter().any(|child| child.id() == *b"cdat"))
                {
                    return Some(control);
                }
                if let Some(children) = chunk.children()
                    && let Some(found) = find_static(children)
                {
                    return Some(found);
                }
            }
            None
        }
        let expected = find_static(native.chunks()).expect("native source has static Audio Levels");
        let fresh = timeline_layer(
            &quicktime_spec(FootageKind::Audio, true),
            3,
            2,
            Duration24::from_frames(48).unwrap(),
            None,
        )
        .unwrap();
        let actual = audio_levels(&fresh).expect("fresh audio layer has Audio Levels");
        let children = actual.children().unwrap();
        let reference = expected.children().unwrap();
        for id in [*b"tdsb", *b"tdb4", *b"tdum", *b"tduM"] {
            let got = children
                .iter()
                .find(|chunk| chunk.id() == id)
                .and_then(Chunk::data_payload);
            let want = reference
                .iter()
                .find(|chunk| chunk.id() == id)
                .and_then(Chunk::data_payload);
            if id == *b"tdb4" {
                assert_eq!(got.map(|data| &data[..12]), want.map(|data| &data[..12]));
            } else {
                assert_eq!(got, want, "native Audio Levels {id:?}");
            }
        }
    }

    #[test]
    fn footage_properties_and_static_remap_use_composition_clock() {
        let mut spec = quicktime_spec(FootageKind::Audio, true);
        spec.static_source_time_secs = Some(0.5);
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let layer = timeline_layer_with_clock(
            &spec,
            3,
            2,
            Duration24::from_frames(48).unwrap(),
            None,
            clock,
        )
        .unwrap();
        fn collect_clocks(chunk: &Chunk, clocks: &mut Vec<u32>) {
            if chunk.id() == *b"tdb4" {
                let bytes = chunk.data_payload().unwrap();
                clocks.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
            }
            if let Some(children) = chunk.children() {
                for child in children {
                    collect_clocks(child, clocks);
                }
            }
        }
        let mut clocks = Vec::new();
        collect_clocks(&layer, &mut clocks);
        // Transform (5), audio levels (1) and static Time Remap (1).
        assert_eq!(clocks, vec![30_720; 7]);
        assert_ne!(
            clock.ticks(),
            super::super::keyframes::PropertyClock::DEFAULT.ticks()
        );
    }

    #[test]
    fn transform_keys_are_rebased_before_property_serialization() {
        let clock = SourceClockPlan::affine(
            fx_schema::TimeRangeProperty::new(
                fx_schema::Time::from_millis(1_000),
                fx_schema::Duration::from_millis(2_000),
            ),
            fx_schema::Time::from_millis(500),
            fx_schema::Time::from_millis(1_500),
            2_000,
        )
        .unwrap();
        assert_eq!(clock.source_time_millis(0).unwrap(), 500);
        assert_eq!(clock.source_time_millis(1_000).unwrap(), 1_000);
    }

    #[test]
    fn source_geometry_rejects_short_transform_tracks_with_typed_error() {
        let spec = quicktime_spec(FootageKind::Video, false);
        let duration = Duration24::from_frames(48).unwrap();
        let malformed_track = || NumericTrack {
            keys: vec![crate::writer::NumericKeyframe {
                time_millis: 0,
                values: vec![1.0],
                easing: vec![crate::writer::KeyframeEasing::Linear],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            }],
        };

        for animations in [
            TransformAnimations {
                anchor: Some(malformed_track()),
                ..Default::default()
            },
            TransformAnimations {
                scale: Some(malformed_track()),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                timeline_layer(&spec, 3, 2, duration, Some(&animations)),
                Err(AepWriteError::Invalid(_))
            ));
        }
    }

    #[test]
    fn frame_blending_modes_require_the_composition_master() {
        assert!(!NativeFrameBlending::Disabled.requires_composition_master());
        assert!(NativeFrameBlending::FrameMix.requires_composition_master());
        assert_eq!(NativeFrameBlending::PixelMotion.layer_flags(), (true, true));
    }

    #[test]
    fn fractional_source_rate_uses_established_sspc_fields() {
        let mut spec = quicktime_spec(FootageKind::Video, false);
        spec.source.frame_rate = NativeFrameRate {
            integer: 23,
            fractional: 63_963,
        };
        let item = source_item(&spec, 2).unwrap();
        let settings = item
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Pin "))
            .unwrap()
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.id() == *b"sspc")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&settings[56..60], &23_u32.to_be_bytes());
        assert_eq!(&settings[60..62], &63_963_u16.to_be_bytes());
        assert_eq!(&settings[154..156], &63_963_u16.to_be_bytes());
    }

    #[test]
    fn wave_alias_preserves_packaged_directory_components() {
        fn alias(chunk: &Chunk) -> Option<serde_json::Value> {
            if chunk.id() == *b"alas" {
                return Some(serde_json::from_slice(chunk.data_payload().unwrap()).unwrap());
            }
            chunk.children()?.iter().find_map(alias)
        }
        for (path, depth) in [
            ("media/sound.wav", 2),
            ("media/Sound FX/clip é.wav", 3),
            ("media/a/b/sound.wav", 4),
        ] {
            let mut spec = quicktime_spec(FootageKind::Audio, true);
            spec.source.path = RelativeMediaPath::new(path).unwrap();
            spec.source.format = NativeSourceFormat::Wave;
            spec.source.dimensions = [0, 0];
            spec.source.frame_rate = NativeFrameRate::integer(0);
            let item = source_item(&spec, 42).unwrap();
            let alias = alias(&item).unwrap();
            assert_eq!(alias["ascendcount_base"], 1);
            assert_eq!(
                alias["ascendcount_target"], depth,
                "{path}: target=1 searches beside the AEP"
            );
            assert_eq!(alias["fullpath"], format!("./{path}"));
            assert_eq!(alias["target_is_folder"], false);
        }
    }

    #[test]
    fn wave_source_records_match_two_independent_native_headers() {
        let mut spec = quicktime_spec(FootageKind::Audio, true);
        spec.source.path = RelativeMediaPath::new("media/sound.wav").unwrap();
        spec.source.format = NativeSourceFormat::Wave;
        spec.source.dimensions = [0, 0];
        spec.source.duration_millis = 4_000;
        spec.source.frame_rate = NativeFrameRate::integer(0);
        let fresh = source_item(&spec, 42).unwrap();
        let native = [
            include_bytes!("../../tests/fixtures/audio_e2e/audio_cases.aep").as_slice(),
            include_bytes!("../../tests/fixtures/media/audioEnabled.aep").as_slice(),
        ];
        let reference_settings: Vec<_> = native
            .iter()
            .map(|bytes| {
                let rifx = crate::rifx::Rifx::parse_with(bytes, |kind| kind == *b"tdgp").unwrap();
                fn first_wave(chunks: &[Chunk]) -> Option<(&Chunk, &[u8])> {
                    for chunk in chunks {
                        if chunk.list_kind() == Some(*b"Item")
                            && let Some(item) = chunk.children()
                            && let Some(pin) = item.iter().find(|child| {
                                child.list_kind() == Some(*b"Pin ")
                                    && child.children().is_some_and(|children| {
                                        children.iter().any(|child| {
                                            child.id() == *b"sspc"
                                                && child.data_payload().is_some_and(|data| {
                                                    data.len() >= 26 && &data[22..26] == b"WAVE"
                                                })
                                        })
                                    })
                            })
                            && let Some(record) = item
                                .iter()
                                .find(|child| child.id() == *b"idta")
                                .and_then(Chunk::data_payload)
                        {
                            return Some((pin, record));
                        }
                        if let Some(children) = chunk.children()
                            && let Some(found) = first_wave(children)
                        {
                            return Some(found);
                        }
                    }
                    None
                }
                let (pin, record) =
                    first_wave(rifx.chunks()).expect("pinned source has WAVE footage");
                let children = pin.children().unwrap();
                let settings = children
                    .iter()
                    .find(|child| child.id() == *b"sspc")
                    .unwrap()
                    .data_payload()
                    .unwrap();
                let options = children
                    .iter()
                    .find(|child| child.id() == *b"opti")
                    .unwrap()
                    .data_payload()
                    .unwrap();
                (settings.to_vec(), options.to_vec(), record.to_vec())
            })
            .collect();
        let pin = fresh
            .children()
            .unwrap()
            .iter()
            .find(|child| child.list_kind() == Some(*b"Pin "))
            .unwrap();
        let children = pin.children().unwrap();
        let settings = children
            .iter()
            .find(|c| c.id() == *b"sspc")
            .unwrap()
            .data_payload()
            .unwrap();
        let options = children
            .iter()
            .find(|c| c.id() == *b"opti")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&settings[38..46], &reference_settings[0].0[38..46]);
        assert_eq!(
            include_bytes!("../../tests/fixtures/audio_e2e/sound.wav").len(),
            768_044
        );
        spec.source.wave_metadata = Some(NativeWaveMetadata {
            sample_frames: 192_000,
            file_length: 768_044,
        });
        let exact = source_item(&spec, 42).unwrap();
        let exact_settings = exact
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Pin "))
            .unwrap()
            .children()
            .unwrap()
            .iter()
            .find(|child| child.id() == *b"sspc")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&exact_settings[38..46], &reference_settings[0].0[38..46]);
        assert_eq!(
            &exact_settings[208..212],
            &reference_settings[0].0[208..212]
        );
        for (native_settings, native_options, _) in &reference_settings {
            for range in [
                52..54,
                64..66,
                72..74,
                78..80,
                96..102,
                112..114,
                124..126,
                128..130,
                156..160,
                196..202,
                212..213,
            ] {
                assert_eq!(
                    &settings[range.clone()],
                    &native_settings[range.clone()],
                    "sspc {range:?}"
                );
            }
            assert_eq!(options, native_options);
        }
        assert_eq!(
            children.iter().map(Chunk::id).collect::<Vec<_>>(),
            vec![
                *b"sspc", *b"Utf8", *b"LIST", *b"opti", *b"pgui", *b"LIST", *b"LIST", *b"Utf8"
            ]
        );
        assert_eq!(
            children
                .iter()
                .filter_map(Chunk::list_kind)
                .collect::<Vec<_>>(),
            vec![*b"Als2", *b"CLRS", *b"mnfo"]
        );
        let colors = children
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"CLRS"))
            .unwrap()
            .children()
            .unwrap();
        assert_eq!(
            colors
                .iter()
                .find(|chunk| chunk.id() == *b"ipws")
                .unwrap()
                .data_payload(),
            Some([0_u8].as_slice())
        );
        let item = fresh.children().unwrap();
        assert!(item.iter().any(|chunk| chunk.id() == *b"ftgi"));
        // Both independently authored WAVE sources carry a complete alias
        // metadata envelope and empty source guide records. An own-reader
        // success does not establish that AE accepts absent native keys.
        let alias = children
            .iter()
            .find(|child| child.list_kind() == Some(*b"Als2"))
            .unwrap()
            .children()
            .unwrap()
            .iter()
            .find(|child| child.id() == *b"alas")
            .unwrap()
            .data_payload()
            .unwrap();
        let alias: serde_json::Value = serde_json::from_slice(alias).unwrap();
        for key in [
            "ascendcount_base",
            "ascendcount_target",
            "platform",
            "server_name",
            "server_volume_name",
            "target_is_folder",
        ] {
            assert!(alias.get(key).is_some(), "missing native alias key {key}");
        }
        assert_eq!(alias["platform"], 2); // Current writer emits macOS AEPs.
        let guides = item
            .iter()
            .find(|child| child.list_kind() == Some(*b"Gide"))
            .expect("WAVE footage has a native source guide list");
        assert_eq!(
            guides
                .children()
                .unwrap()
                .iter()
                .map(Chunk::id)
                .collect::<Vec<_>>(),
            vec![*b"gdta", *b"LIST"]
        );
        let guide_header = guides.children().unwrap()[1].children().unwrap()[0]
            .data_payload()
            .unwrap();
        assert_eq!(guide_header.len(), 52);
        assert_eq!(&guide_header[..4], &0x00d0_0bee_u32.to_be_bytes());
        let record = item
            .iter()
            .find(|chunk| chunk.id() == *b"idta")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&record[20..24], &4_u32.to_be_bytes());
        for (_, _, native_record) in &reference_settings {
            assert_eq!(
                &record[58..61],
                &native_record[58..61],
                "WAVE source item capability/stream flags"
            );
        }
        assert_eq!(&record[58..61], &[7, 0, 0]);

        // A 24.920-second, 48-kHz source contains exactly 1,196,160 samples;
        // duration must use the WAV sample clock. This exercises the bounded
        // direct-writer fallback rather than retaining the previous fixture's
        // exact 4-second metadata.
        spec.source.wave_metadata = None;
        spec.source.duration_millis = 24_920;
        let music = source_item(&spec, 43).unwrap();
        let music_pin = music
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Pin "))
            .unwrap();
        let music_settings = music_pin
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.id() == *b"sspc")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&music_settings[38..42], &1_196_160_u32.to_be_bytes());
        assert_eq!(&music_settings[42..46], &48_000_u32.to_be_bytes());
        // The model stores rounded milliseconds, not exact PCM frames. A
        // valid 44.1-kHz WAVE can end between integer milliseconds: rejecting
        // its source outright prevents otherwise editable audio export.
        spec.source.audio_sample_rate = 44_100.0;
        spec.source.duration_millis = 5_944;
        let fractional = source_item(&spec, 44).unwrap();
        let fractional_pin = fractional
            .children()
            .unwrap()
            .iter()
            .find(|child| child.list_kind() == Some(*b"Pin "))
            .unwrap();
        let fractional_settings = fractional_pin
            .children()
            .unwrap()
            .iter()
            .find(|child| child.id() == *b"sspc")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&fractional_settings[38..42], &262_130_u32.to_be_bytes());
        assert_eq!(&fractional_settings[42..46], &44_100_u32.to_be_bytes());
        // The pinned media/audioEnabled source has 262,094 PCM frames;
        // this shows the remaining 36-frame uncertainty from ceil-ms metadata.
    }

    #[test]
    fn exact_wave_metadata_preserves_unrounded_sample_count_and_file_length() {
        let mut spec = quicktime_spec(FootageKind::Audio, true);
        spec.source.format = NativeSourceFormat::Wave;
        spec.source.dimensions = [0, 0];
        spec.source.frame_rate = NativeFrameRate::integer(0);
        spec.source.audio_sample_rate = 44_100.0;
        spec.source.duration_millis = 5_944;
        spec.source.wave_metadata = Some(NativeWaveMetadata {
            sample_frames: 262_094,
            file_length: 524_232,
        });
        let item = source_item(&spec, 12).unwrap();
        let settings = item
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Pin "))
            .unwrap()
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.id() == *b"sspc")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&settings[38..42], &262_094_u32.to_be_bytes());
        assert_eq!(&settings[42..46], &44_100_u32.to_be_bytes());
        assert_eq!(&settings[208..212], &524_232_u32.to_be_bytes());

        spec.source.wave_metadata.as_mut().unwrap().sample_frames = 260_000;
        assert!(source_item(&spec, 12).is_err());
        spec.source.wave_metadata.as_mut().unwrap().sample_frames = 262_094;
        spec.source.wave_metadata.as_mut().unwrap().file_length = 44;
        assert!(source_item(&spec, 12).is_err());
    }

    #[test]
    fn pinned_native_sources_establish_emitted_fourccs() {
        let still = read_project(include_bytes!(
            "../../tests/fixtures/media/footage_not_missing.aep"
        ))
        .unwrap();
        let native_exr_fourcc = still
            .item(1)
            .unwrap()
            .media
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .source_format;
        assert_eq!(native_exr_fourcc, NativeSourceFormat::OpenExr.fourcc());

        let mut fresh_exr = quicktime_spec(FootageKind::Video, false);
        fresh_exr.name = "still.exr".into();
        fresh_exr.source.path = RelativeMediaPath::new("media/still.exr").unwrap();
        fresh_exr.source.format = NativeSourceFormat::OpenExr;
        fresh_exr.source.audio_sample_rate = 0.0;
        let fresh_item = source_item(&fresh_exr, 2).unwrap();
        let pin = fresh_item
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"Pin "))
            .unwrap();
        let settings = pin
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.id() == *b"sspc")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(&settings[22..26], native_exr_fourcc.as_slice());

        let audio = read_project(include_bytes!(
            "../../tests/fixtures/media/audioEnabled.aep"
        ))
        .unwrap();
        assert_eq!(
            audio
                .item(13)
                .unwrap()
                .media
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .source_format,
            NativeSourceFormat::Wave.fourcc()
        );
    }
}
