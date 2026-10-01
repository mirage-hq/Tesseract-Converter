//! Pure mapping from decoded AE file footage to existing editable FX media layers.
//! Filesystem resolution and archive publication belong to the adapter boundary.

use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use fx_schema::animator::AnimationGraphEntry;
use fx_schema::layer::{AudioLayer, AudioSource, FrameBlendingMode, ImageLayer, VideoLayer};
use fx_schema::{
    AssetId, Duration, GroupLayer, LayerData as FxLayer, LayerId, LinearGain, MediaFit,
    PositiveRect, PropType, PropertyTarget, RectBounds, Time, TimeRangeProperty, Transform,
    VideoSource,
};
use thiserror::Error;

use super::{
    animation::{self, NumericAnimationTarget},
    animation_budget::AnimationBudget,
};
use crate::{
    alias::RelativeLocation,
    media::{MediaDescriptor, MediaKind, PhotoshopSource},
    properties::{NumericKeyframe, NumericProperty, read_numeric, root_runs, runs, unique_list},
    structure::{Layer, ProjectItem},
    vector_media::Artwork,
};

mod vector;

/// Archive asset category requested by the pure converter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaAssetKind {
    Image,
    SequenceImage,
    Video,
    Audio,
}

/// One local authored source that the parent adapter must resolve and package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MediaAssetRequest {
    pub(crate) logical_id: AssetId,
    /// Native source identity, independent of the archive asset namespace.
    pub(crate) source_item_id: u32,
    pub(crate) authored_path: String,
    /// AE's hint for `authored_path` after the project moved, when it has one.
    pub(crate) relative_location: Option<RelativeLocation>,
    pub(crate) kind: MediaAssetKind,
    pub(crate) photoshop_source: Option<PhotoshopSource>,
    pub(crate) dimensions: [u32; 2],
}

/// The archive asset ids of one source AEP in the document that packages its
/// media: standalone import names them `aep-local-item-<id>`; content embedded
/// in a host document takes the namespace that the host gives its resolved AEP.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AssetNamespace<'a>(&'a str);

impl<'a> AssetNamespace<'a> {
    pub(crate) const STANDALONE: AssetNamespace<'static> = AssetNamespace("aep-local");

    pub(crate) fn new(prefix: &'a str) -> Self {
        Self(prefix)
    }

    /// The namespace's own name, which each of its asset ids begins with.
    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }

    /// The asset of one footage item.
    fn item(self, item: u32) -> Result<AssetId, fx_schema::InvalidAssetId> {
        AssetId::new(format!("{}-item-{item}", self.0))
    }

    /// The asset of one frame of an image-sequence footage item.
    fn sequence_frame(self, item: u32, frame: u32) -> Result<AssetId, fx_schema::InvalidAssetId> {
        AssetId::new(format!("{}-item-{item}-sequence-frame-{frame}", self.0))
    }
}

/// Adapter resolution for one reached local source.
#[derive(Clone)]
pub(crate) enum MediaResolution {
    Unavailable,
    Asset,
    AssetDimensions([u32; 2]),
    Vector(Arc<Artwork>),
}

/// Pure conversion output. No request implies no FX asset reference was emitted.
#[derive(Debug)]
pub(super) struct MediaLayerConversion {
    pub(super) layers: Vec<FxLayer>,
    pub(super) animations: Vec<AnimationGraphEntry>,
    pub(super) next_id: u64,
    pub(super) warnings: Vec<String>,
    pub(super) assets: Vec<MediaAssetRequest>,
}

/// A destination invariant prevented construction of otherwise valid media.
#[derive(Debug, Error)]
pub(super) enum MediaConversionError {
    #[error("invalid archive-local media asset id: {0}")]
    AssetId(#[from] fx_schema::InvalidAssetId),
    #[error("media duration cannot be represented")]
    Duration,
    #[error("media layer IDs are exhausted")]
    IdExhausted,
}

impl MediaLayerConversion {
    fn placeholder(next_id: u64, warning: impl Into<String>) -> Self {
        Self {
            layers: Vec::new(),
            animations: Vec::new(),
            next_id,
            warnings: vec![warning.into()],
            assets: Vec::new(),
        }
    }
}

pub(crate) fn asset_request_for_source(
    source: &ProjectItem,
    asset_namespace: AssetNamespace<'_>,
) -> Option<MediaAssetRequest> {
    let descriptor = source.media.as_ref()?.as_ref().ok()?;
    let kind = match descriptor.kind {
        MediaKind::Video | MediaKind::AudioVideo => MediaAssetKind::Video,
        MediaKind::Audio => MediaAssetKind::Audio,
        MediaKind::StillImage | MediaKind::ImageSequence => return None,
    };
    Some(MediaAssetRequest {
        logical_id: asset_namespace.item(source.id).ok()?,
        source_item_id: source.id,
        authored_path: descriptor.authored_path.clone(),
        relative_location: descriptor.relative_location,
        kind,
        photoshop_source: descriptor.photoshop_source,
        dimensions: [u32::from(descriptor.width), u32::from(descriptor.height)],
    })
}

fn identity_transform() -> Transform {
    Transform {
        anchor_point: [0.0, 0.0],
        position: fx_schema::Position::TwoD([0.0, 0.0]),
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0, 0.0, 0.0],
        opacity: fx_schema::PercentageProperty::new(100.0)
            .expect("100 is a finite percentage in 0..=100"),
    }
}

fn frame_blending(layer: &Layer, composition_enabled: bool) -> Option<FrameBlendingMode> {
    if !composition_enabled {
        return None;
    }
    match layer.record.frame_blending_type() {
        1 => Some(FrameBlendingMode::Simple),
        2 => Some(FrameBlendingMode::OpticalFlow),
        _ => None,
    }
}

fn source_durations(source: &ProjectItem) -> Result<(Duration, Duration), MediaConversionError> {
    let descriptor = source
        .media
        .as_ref()
        .and_then(|media| media.as_ref().ok())
        .ok_or(MediaConversionError::Duration)?;
    let interpreted_secs = descriptor.duration.seconds();
    if !interpreted_secs.is_finite() || interpreted_secs <= 0.0 {
        return Err(MediaConversionError::Duration);
    }
    let native_rate = descriptor.native_frame_rate.as_f64();
    let conform_rate = descriptor.conform_frame_rate.as_f64();
    let asset_secs = if native_rate > 0.0 && conform_rate > 0.0 {
        interpreted_secs * conform_rate / native_rate
    } else {
        interpreted_secs
    };
    if !asset_secs.is_finite() || asset_secs <= 0.0 {
        return Err(MediaConversionError::Duration);
    }
    Ok((
        Duration::from_secs(interpreted_secs),
        Duration::from_secs(asset_secs),
    ))
}

fn safe_sequence_filename(name: &str) -> bool {
    if name.is_empty() || name.contains(['/', '\\', '\0']) {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn sequence_frame_count(descriptor: &MediaDescriptor) -> Result<u32, String> {
    descriptor
        .sequence_end_frame
        .checked_sub(descriptor.sequence_start_frame)
        .and_then(|span| span.checked_add(1))
        .ok_or_else(|| {
            format!(
                "invalid image sequence frame range {}..{}",
                descriptor.sequence_start_frame, descriptor.sequence_end_frame
            )
        })
}

fn png_sequence_paths(
    source: &ProjectItem,
    descriptor: &MediaDescriptor,
) -> Result<Vec<(u32, String)>, String> {
    let count = sequence_frame_count(descriptor)?;
    if descriptor.sequence_frame_padding == 0 {
        return Err("image sequence has zero filename padding".into());
    }
    let expected_names =
        usize::try_from(count).map_err(|_| "image sequence frame count cannot be represented")?;
    if !descriptor.sequence_names.is_empty() && descriptor.sequence_names.len() != expected_names {
        return Err(format!(
            "image sequence range contains {count} frames but native metadata names {} files",
            descriptor.sequence_names.len()
        ));
    }

    let base = if descriptor.target_is_folder {
        PathBuf::from(&descriptor.authored_path)
    } else {
        Path::new(&descriptor.authored_path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_owned()
    };
    let base_len = base
        .to_str()
        .ok_or_else(|| "image sequence base path is not valid UTF-8".to_owned())?
        .len();

    let names = if descriptor.sequence_names.is_empty() {
        let (prefix, extension) = if descriptor.target_is_folder {
            if !safe_sequence_filename(&source.name) {
                return Err(format!(
                    "image sequence source name {:?} is not a safe filename stem",
                    source.name
                ));
            }
            (format!("{}_", source.name), "png".to_owned())
        } else {
            let authored = Path::new(&descriptor.authored_path);
            let filename = authored
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "image sequence authored path has no UTF-8 filename".to_owned())?;
            if !safe_sequence_filename(filename) {
                return Err(format!(
                    "image sequence authored filename {filename:?} is unsafe"
                ));
            }
            let extension = authored
                .extension()
                .and_then(|extension| extension.to_str())
                .filter(|extension| extension.eq_ignore_ascii_case("png"))
                .ok_or_else(|| "only PNG image sequences can be packaged".to_owned())?;
            let stem = authored
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| "image sequence filename has no UTF-8 stem".to_owned())?;
            let digit_start = stem
                .char_indices()
                .rev()
                .find(|(_, character)| !character.is_ascii_digit())
                .map_or(0, |(index, character)| index + character.len_utf8());
            let (prefix, digits) = stem.split_at(digit_start);
            let padding = usize::try_from(descriptor.sequence_frame_padding)
                .map_err(|_| "image sequence filename padding cannot be represented")?;
            if digits.len() != padding
                || digits.parse::<u32>().ok() != Some(descriptor.sequence_start_frame)
            {
                return Err(format!(
                    "authored image sequence filename {filename:?} does not identify first frame {} with padding {}",
                    descriptor.sequence_start_frame, descriptor.sequence_frame_padding
                ));
            }
            (prefix.to_owned(), extension.to_owned())
        };
        let padding = usize::try_from(descriptor.sequence_frame_padding)
            .map_err(|_| "image sequence filename padding cannot be represented")?;
        let digit_count = padding.max(descriptor.sequence_end_frame.to_string().len());
        let filename_len = prefix
            .len()
            .checked_add(digit_count)
            .and_then(|length| length.checked_add(1))
            .and_then(|length| length.checked_add(extension.len()))
            .ok_or_else(|| "image sequence filename length cannot be represented".to_owned())?;
        let path_len = base_len
            .checked_add(usize::from(!base.as_os_str().is_empty()))
            .and_then(|length| length.checked_add(filename_len))
            .ok_or_else(|| "image sequence path length cannot be represented".to_owned())?;
        // Retain the prior generated-name allocation safeguard until filename
        // construction can validate host limits without materializing padding.
        // This is not an AE format constraint or a limit on read alias strings.
        const MAX_GENERATED_PATH_BYTES: usize = 64 * 1024;
        if filename_len > MAX_GENERATED_PATH_BYTES || path_len > MAX_GENERATED_PATH_BYTES {
            return Err(format!(
                "image sequence generated filename/path exceeds the {MAX_GENERATED_PATH_BYTES}-byte allocation safeguard"
            ));
        }
        (descriptor.sequence_start_frame..=descriptor.sequence_end_frame)
            .map(|frame| format!("{prefix}{frame:0padding$}.{extension}"))
            .collect::<Vec<_>>()
    } else {
        let mut names = Vec::with_capacity(descriptor.sequence_names.len());
        for name in &descriptor.sequence_names {
            if !safe_sequence_filename(name)
                || !Path::new(name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
            {
                return Err(format!(
                    "image sequence filename {name:?} is unsafe or is not PNG"
                ));
            }
            names.push(name.clone());
        }
        names
    };

    names
        .into_iter()
        .zip(descriptor.sequence_start_frame..=descriptor.sequence_end_frame)
        .map(|(name, frame)| {
            base.join(name)
                .to_str()
                .map(|path| (frame, path.to_owned()))
                .ok_or_else(|| format!("image sequence frame {frame} path is not valid UTF-8"))
        })
        .collect()
}

fn convert_image_sequence(
    source: &ProjectItem,
    occurrence: &GroupLayer,
    descriptor: &MediaDescriptor,
    next_id: u64,
    asset_namespace: AssetNamespace<'_>,
    resolve_media: &mut impl FnMut(&MediaAssetRequest) -> MediaResolution,
) -> Result<MediaLayerConversion, MediaConversionError> {
    let count = match sequence_frame_count(descriptor) {
        Ok(count) => u64::from(count),
        Err(reason) => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                format!(
                    "image sequence metadata is unsupported: {reason}; sequence content omitted"
                ),
            ));
        }
    };
    let mut next_available_id = next_id;
    let Some(first_id) = super::reserve_ids(&mut next_available_id, count) else {
        return Ok(MediaLayerConversion::placeholder(
            next_id,
            format!(
                "image sequence frame count {count} exceeds the remaining generated-layer identity range; sequence content omitted"
            ),
        ));
    };
    let paths = match png_sequence_paths(source, descriptor) {
        Ok(paths) => paths,
        Err(reason) => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                format!(
                    "image sequence metadata is unsupported: {reason}; sequence content omitted"
                ),
            ));
        }
    };
    let rate = {
        let conform = descriptor.conform_frame_rate.as_f64();
        if conform > 0.0 {
            conform
        } else {
            descriptor.native_frame_rate.as_f64()
        }
    };
    if !rate.is_finite() || rate <= 0.0 {
        return Ok(MediaLayerConversion::placeholder(
            next_id,
            "image sequence has no finite positive native/conform frame rate; sequence content omitted",
        ));
    }
    let frame = match PositiveRect::new(RectBounds::from_size(
        f64::from(descriptor.width),
        f64::from(descriptor.height),
    )) {
        Some(frame) => frame,
        None => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                "image sequence has no positive fixed footage canvas; sequence content omitted",
            ));
        }
    };
    let mut layers = Vec::with_capacity(paths.len());
    let mut assets = Vec::with_capacity(paths.len());
    let mut warnings = Vec::new();
    if descriptor.pixel_aspect.0 != descriptor.pixel_aspect.1 {
        warnings.push(format!(
            "image-sequence pixel aspect {:?} is represented with square FX pixels",
            descriptor.pixel_aspect
        ));
    }
    if descriptor.conform_frame_rate.as_f64() > 0.0
        && descriptor.conform_frame_rate != descriptor.native_frame_rate
    {
        warnings.push(format!(
            "image sequence conform rate {} differs from native {}; frame active ranges use the conformed source clock",
            descriptor.conform_frame_rate.as_f64(),
            descriptor.native_frame_rate.as_f64()
        ));
    }
    let motion_blur = occurrence.motion_blur;
    // Frames are the aliased folder's children or the aliased first file's
    // siblings, so they relink one level deeper or at the same depth.
    let relative_location = if descriptor.target_is_folder {
        descriptor
            .relative_location
            .and_then(RelativeLocation::inside_folder)
    } else {
        descriptor.relative_location
    };
    for (ordinal, (source_frame, authored_path)) in paths.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| MediaConversionError::IdExhausted)?;
        let next_ordinal = ordinal
            .checked_add(1)
            .ok_or(MediaConversionError::IdExhausted)?;
        let start = Time::from_millis_f64(f64::from(ordinal) * 1_000.0 / rate);
        let end = Time::from_millis_f64(f64::from(next_ordinal) * 1_000.0 / rate);
        let Some(duration) = end.checked_sub(start) else {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                format!(
                    "image sequence frame {source_frame} clock cannot be represented at millisecond precision; sequence content omitted"
                ),
            ));
        };
        if duration.is_zero() {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                format!(
                    "image sequence frame {source_frame} has a zero-length millisecond range; sequence content omitted"
                ),
            ));
        }
        let logical_id = asset_namespace.sequence_frame(source.id, source_frame)?;
        let request = MediaAssetRequest {
            logical_id: logical_id.clone(),
            source_item_id: source.id,
            authored_path,
            relative_location,
            kind: MediaAssetKind::SequenceImage,
            photoshop_source: None,
            dimensions: [u32::from(descriptor.width), u32::from(descriptor.height)],
        };
        match resolve_media(&request) {
            MediaResolution::Asset => {}
            MediaResolution::AssetDimensions(dimensions) if dimensions == request.dimensions => {}
            MediaResolution::AssetDimensions(dimensions) => {
                warnings.push(format!(
                    "image sequence frame {source_frame} keeps original {}x{} PNG bytes but differs from the native fixed footage canvas {}x{}; unverified placement/scaling was not guessed, so this frame is omitted while matching siblings are preserved",
                    dimensions[0], dimensions[1], request.dimensions[0], request.dimensions[1]
                ));
                continue;
            }
            MediaResolution::Unavailable => {
                warnings.push(format!(
                    "image sequence frame {source_frame} at {:?} is unavailable; its active range remains blank while other frames are preserved",
                    request.authored_path
                ));
                continue;
            }
            MediaResolution::Vector(_) => {
                warnings.push(format!(
                    "image sequence frame {source_frame} resolved as vector content unexpectedly; its active range remains blank"
                ));
                continue;
            }
        }
        let id = LayerId::new(first_id + u64::from(ordinal));
        layers.push(FxLayer::Image(ImageLayer {
            id,
            name: format!("{} frame {source_frame}", occurrence.name),
            description: format!(
                "Editable AE PNG sequence frame {source_frame} from source item {}",
                source.id
            ),
            is_hidden: false,
            parent: Some(occurrence.id),
            blend_mode: Default::default(),
            track_matte: None,
            masks: Vec::new(),
            corner_radius: None,
            active_range: TimeRangeProperty::new(start, duration),
            effects: Vec::new(),
            placement: None,
            captions_enabled: None,
            motion_blur,
            transform: identity_transform(),
            source: fx_schema::ImageSource::from_asset(logical_id, Some(frame), MediaFit::Stretch),
        }));
        assets.push(request);
    }

    Ok(MediaLayerConversion {
        layers,
        animations: Vec::new(),
        next_id: next_available_id,
        warnings,
        assets,
    })
}

fn d_b_to_gain(value: f64) -> Option<f64> {
    let gain = 10.0_f64.powf(value / 20.0);
    (value.is_finite() && gain.is_finite() && gain >= 0.0).then_some(gain)
}

fn audio_levels(layer: &Layer) -> Result<Option<NumericProperty>, String> {
    let roots = root_runs(&layer.content).map_err(|error| error.to_string())?;
    let mut audio_groups = roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Audio Group");
    let Some((_, group_run)) = audio_groups.next() else {
        return Ok(None);
    };
    if audio_groups.next().is_some() {
        return Err("duplicate Audio property groups".into());
    }
    let group = unique_list(group_run, *b"tdgp").map_err(|error| error.to_string())?;
    let leaves = runs(group).map_err(|error| error.to_string())?;
    let mut levels = leaves
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Audio Levels");
    let Some((_, run)) = levels.next() else {
        return Ok(None);
    };
    if levels.next().is_some() {
        return Err("duplicate Audio Levels properties".into());
    }
    let numeric = unique_list(run, *b"tdbs")
        .and_then(read_numeric)
        .map_err(|error| error.to_string())?;
    Ok(Some(numeric))
}

fn stereo_gain(values: &[f64], warnings: &mut Vec<String>) -> Option<f64> {
    let left = *values.first()?;
    let right = values.get(1).copied().unwrap_or(left);
    if left != right {
        warnings.push(format!(
            "Audio Levels channels differ ({left} dB left, {right} dB right); mono FX gain uses the quieter channel"
        ));
    }
    d_b_to_gain(left.min(right))
}

fn gain_and_animation(
    layer: &Layer,
    target: LayerId,
    enabled: bool,
    budget: &mut AnimationBudget,
) -> (LinearGain, Vec<AnimationGraphEntry>, Vec<String>) {
    if !enabled {
        return (LinearGain::ZERO, Vec::new(), Vec::new());
    }
    let mut warnings = Vec::new();
    let numeric = match audio_levels(layer) {
        Ok(Some(numeric)) => numeric,
        Ok(None) => return (LinearGain::UNITY, Vec::new(), warnings),
        Err(error) => {
            warnings.push(format!(
                "Audio Levels could not be decoded: {error}; unity gain used"
            ));
            return (LinearGain::UNITY, Vec::new(), warnings);
        }
    };
    if numeric.expression_enabled {
        warnings.push(
            "Audio Levels expression requires the AE environment; unity gain and no animation used"
                .into(),
        );
        return (LinearGain::UNITY, Vec::new(), warnings);
    }
    let base = if numeric.animated {
        numeric
            .keyframes
            .first()
            .and_then(|key| stereo_gain(&key.values, &mut warnings))
    } else {
        stereo_gain(&numeric.values, &mut warnings)
    }
    .and_then(|gain| LinearGain::new(gain).ok())
    .unwrap_or_else(|| {
        warnings.push("Audio Levels contained no finite dB value; unity gain used".into());
        LinearGain::UNITY
    });

    if numeric.keyframes.is_empty() {
        return (base, Vec::new(), warnings);
    }
    let mut unequal_key = false;
    for key in &numeric.keyframes {
        let Some(left) = key.values.first().copied() else {
            warnings.push("Audio Levels key has no channel value; animation omitted".into());
            return (base, Vec::new(), warnings);
        };
        let right = key.values.get(1).copied().unwrap_or(left);
        unequal_key |= left != right;
        if d_b_to_gain(left.min(right)).is_none() {
            warnings.push("Audio Levels key has non-finite dB value; animation omitted".into());
            return (base, Vec::new(), warnings);
        }
    }
    if unequal_key {
        warnings.push("animated unequal stereo Audio Levels are approximated by the quieter channel at each key; channel crossover between keys is not preserved".into());
    }
    let project = |source: &NumericKeyframe| {
        let mut key = source.clone();
        let left = key.values[0];
        let right = key.values.get(1).copied().unwrap_or(left);
        let gain = d_b_to_gain(left.min(right)).ok_or_else(|| {
            "Audio Levels key has non-finite dB value; animation omitted".to_owned()
        })?;
        let channel = usize::from(right < left);
        let speed = key.out_speed.get(channel).copied().unwrap_or(0.0);
        let in_speed = key.in_speed.get(channel).copied().unwrap_or(speed);
        let derivative = std::f64::consts::LN_10 / 20.0 * gain;
        key.values = vec![gain];
        key.in_speed = vec![in_speed * derivative];
        key.out_speed = vec![speed * derivative];
        key.in_influence = vec![key.in_influence.get(channel).copied().unwrap_or(33.333)];
        key.out_influence = vec![key.out_influence.get(channel).copied().unwrap_or(33.333)];
        key.spatial_in.clear();
        key.spatial_out.clear();
        Ok(key)
    };
    let (entries, animation_warnings) = animation::projected_numeric_entries(
        "Audio Levels",
        &numeric,
        NumericAnimationTarget::float(PropertyTarget::layer(target, PropType::AudioVolume), 0, 1.0),
        project,
        budget,
    );
    warnings.extend(animation_warnings);
    warnings.push("Audio Levels temporal interpolation is converted through the existing FX keyframe approximation; independent audio fidelity is unverified".into());
    (base, entries, warnings)
}

/// Map one decoded source occurrence without reading or resolving its path.
pub(super) struct MediaImportOptions<'a> {
    pub next_id: u64,
    pub frame_blending_enabled: bool,
    pub asset_namespace: AssetNamespace<'a>,
}

#[cfg(test)]
pub(super) fn convert(
    source: &ProjectItem,
    layer: &Layer,
    occurrence: &GroupLayer,
    options: MediaImportOptions<'_>,
    mut asset_available: impl FnMut(&MediaAssetRequest) -> bool,
    budget: &mut AnimationBudget,
) -> Result<MediaLayerConversion, MediaConversionError> {
    let mut shape_budget = super::shapes::OutputBudget::default();
    convert_resolved(
        source,
        layer,
        occurrence,
        options,
        |request| {
            if asset_available(request) {
                MediaResolution::Asset
            } else {
                MediaResolution::Unavailable
            }
        },
        budget,
        &mut shape_budget,
    )
}

pub(super) fn convert_resolved(
    source: &ProjectItem,
    layer: &Layer,
    occurrence: &GroupLayer,
    options: MediaImportOptions<'_>,
    mut resolve_media: impl FnMut(&MediaAssetRequest) -> MediaResolution,
    budget: &mut AnimationBudget,
    shape_budget: &mut super::shapes::OutputBudget,
) -> Result<MediaLayerConversion, MediaConversionError> {
    let MediaImportOptions {
        next_id,
        frame_blending_enabled,
        asset_namespace,
    } = options;
    let descriptor = match source.media.as_ref() {
        Some(Ok(descriptor)) => descriptor,
        Some(Err(error)) => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                format!(
                    "file source cannot be decoded: {error}; non-rendering placeholder retained"
                ),
            ));
        }
        None => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                "source has no decoded main file descriptor; non-rendering placeholder retained",
            ));
        }
    };
    if descriptor.kind == MediaKind::ImageSequence {
        return convert_image_sequence(
            source,
            occurrence,
            descriptor,
            next_id,
            asset_namespace,
            &mut resolve_media,
        );
    }
    let logical_id = asset_namespace.item(source.id)?;
    let asset_kind = match descriptor.kind {
        MediaKind::StillImage => MediaAssetKind::Image,
        MediaKind::Video | MediaKind::AudioVideo => MediaAssetKind::Video,
        MediaKind::Audio => MediaAssetKind::Audio,
        MediaKind::ImageSequence => unreachable!("handled before asset request"),
    };
    let asset_request = MediaAssetRequest {
        logical_id: logical_id.clone(),
        source_item_id: source.id,
        authored_path: descriptor.authored_path.clone(),
        relative_location: descriptor.relative_location,
        kind: asset_kind,
        photoshop_source: descriptor.photoshop_source,
        dimensions: [u32::from(descriptor.width), u32::from(descriptor.height)],
    };
    let required_ids = if descriptor.kind == MediaKind::AudioVideo {
        2
    } else {
        1
    };
    match resolve_media(&asset_request) {
        MediaResolution::Vector(artwork) if descriptor.kind == MediaKind::StillImage => {
            let expected = artwork.dimensions;
            let actual = [f64::from(descriptor.width), f64::from(descriptor.height)];
            if (expected[0] - actual[0]).abs() > 0.01 || (expected[1] - actual[1]).abs() > 0.01 {
                return Ok(MediaLayerConversion::placeholder(
                    next_id,
                    format!(
                        "PDF page footprint {}x{} differs from AEP source dimensions {}x{}; page/layer/crop interpretation is unproven, so AI content was omitted rather than guessed",
                        expected[0], expected[1], actual[0], actual[1]
                    ),
                ));
            }
            let mut imported = vector::lower(&artwork, occurrence, next_id, shape_budget);
            if descriptor.pixel_aspect.0 != descriptor.pixel_aspect.1 {
                imported.warnings.push(format!(
                    "AI media pixel aspect {:?} is represented with square FX pixels",
                    descriptor.pixel_aspect
                ));
            }
            return Ok(imported);
        }
        MediaResolution::Vector(_) => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                "PDF-compatible AI resolution was rejected because the AEP descriptor is not a still-image source; no guessed media replacement emitted",
            ));
        }
        MediaResolution::AssetDimensions(_) if descriptor.kind == MediaKind::StillImage => {}
        MediaResolution::AssetDimensions(_) => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                "unexpected dimension-bearing media resolution for a non-image source; content omitted",
            ));
        }
        MediaResolution::Unavailable => {
            return Ok(MediaLayerConversion::placeholder(
                next_id,
                format!(
                    "authored media path {:?} was not resolved to validated local content{}; no FX reference or animation emitted",
                    descriptor.authored_path,
                    if descriptor.missing_at_save {
                        " (source was also offline when the AEP was saved)"
                    } else {
                        ""
                    }
                ),
            ));
        }
        MediaResolution::Asset => {}
    }

    let mut next_available_id = next_id;
    let id = LayerId::new(
        super::reserve_ids(&mut next_available_id, required_ids)
            .ok_or(MediaConversionError::IdExhausted)?,
    );
    let next_id = next_available_id;
    let active_max = TimeRangeProperty::new(Time::ZERO, Duration::from_secs(super::MAX_TIME_SECS));
    let flags = layer.record.flags();
    let motion_blur = occurrence.motion_blur;
    let mut warnings = Vec::new();
    if descriptor.missing_at_save {
        warnings.push("source was offline when the AEP was saved, but the authored path now resolved to a validated local asset".into());
    }
    if descriptor.pixel_aspect.0 != descriptor.pixel_aspect.1 {
        warnings.push(format!(
            "media pixel aspect {:?} is represented with square FX pixels",
            descriptor.pixel_aspect
        ));
    }
    if descriptor.conform_frame_rate.as_f64() > 0.0
        && descriptor.conform_frame_rate != descriptor.native_frame_rate
    {
        warnings.push(format!(
            "conformed source rate {} differs from native {}; active/source ranges carry the affine rate conversion",
            descriptor.conform_frame_rate.as_f64(),
            descriptor.native_frame_rate.as_f64()
        ));
    }
    let frame_blending = frame_blending(layer, frame_blending_enabled);
    if matches!(
        frame_blending.as_ref(),
        Some(FrameBlendingMode::OpticalFlow)
    ) {
        warnings.push("AE Pixel Motion is represented by FX optical-flow frame blending; the editable mode is retained, but synthesized fractional source frames can differ from Adobe".into());
    }

    let (layers, animations) = match descriptor.kind {
        MediaKind::StillImage => {
            let image = ImageLayer {
                id,
                name: occurrence.name.clone(),
                description: format!("Editable AE still image from source item {}", source.id),
                is_hidden: false,
                parent: Some(occurrence.id),
                blend_mode: Default::default(),
                track_matte: None,
                masks: Vec::new(),
                corner_radius: None,
                active_range: active_max,
                effects: Vec::new(),
                placement: None,
                captions_enabled: None,
                motion_blur,
                transform: identity_transform(),
                source: fx_schema::ImageSource::from_asset(
                    logical_id.clone(),
                    None,
                    MediaFit::Contain,
                ),
            };
            (vec![FxLayer::Image(image)], Vec::new())
        }
        MediaKind::Video | MediaKind::AudioVideo => {
            let (interpreted_duration, asset_duration) = source_durations(source)?;
            let active_range = TimeRangeProperty::new(Time::ZERO, interpreted_duration);
            let source_range = TimeRangeProperty::new(Time::ZERO, asset_duration);
            let audio_id = LayerId::new(next_id - 1);
            let (gain, animations, gain_warnings) = gain_and_animation(
                layer,
                audio_id,
                flags.audio_enabled && descriptor.kind == MediaKind::AudioVideo,
                budget,
            );
            warnings.extend(gain_warnings);
            let video = VideoLayer {
                id,
                name: occurrence.name.clone(),
                description: format!("Editable AE video from source item {}", source.id),
                metadata: None,
                is_hidden: false,
                parent: Some(occurrence.id),
                start_time: Some(0.0),
                blend_mode: Default::default(),
                track_matte: None,
                masks: Vec::new(),
                corner_radius: None,
                source_range,
                playback: fx_schema::LayerPlayback::linear(
                    active_range,
                    active_range,
                    source_range,
                    0,
                )
                .expect("validated media ranges form a finite playback mapping"),
                preserve_audio_pitch: false,
                source_intrinsic_duration: asset_duration,
                // AV audio is a separate AudioLayer so AE's video and audio
                // switches remain independently editable and renderable.
                volume: None,
                effects: Vec::new(),
                placement: None,
                captions_enabled: None,
                caption_presentation: None,
                frame_blending: frame_blending.map(fx_schema::layer::FrameBlendingData::Mode),
                motion_blur,
                transform: identity_transform(),
                source: VideoSource::from_asset(logical_id.clone(), None, MediaFit::Contain),
            };
            let mut layers = vec![FxLayer::Video(video)];
            if descriptor.kind == MediaKind::AudioVideo {
                layers.push(FxLayer::Audio(AudioLayer {
                    id: audio_id,
                    name: format!("{} audio", occurrence.name),
                    description: format!(
                        "Editable AE embedded audio from source item {}",
                        source.id
                    ),
                    is_hidden: false,
                    parent: Some(occurrence.id),
                    start_time: Some(0.0),
                    volume: gain,
                    auto_ducking: None,
                    window_ms: fx_schema::default_audio_window_ms(),
                    source: AudioSource {
                        asset_id: logical_id.clone(),
                        enhancement: None,
                    },
                    metadata: None,
                    captions_enabled: None,
                    caption_presentation: None,
                    source_range,
                    playback: fx_schema::LayerPlayback::linear(
                        active_range,
                        active_range,
                        source_range,
                        0,
                    )
                    .expect("validated media ranges form a finite playback mapping"),
                    preserve_audio_pitch: false,
                    source_intrinsic_duration: asset_duration,
                }));
            }
            (layers, animations)
        }
        MediaKind::Audio => {
            let (interpreted_duration, asset_duration) = source_durations(source)?;
            let active_range = TimeRangeProperty::new(Time::ZERO, interpreted_duration);
            let source_range = TimeRangeProperty::new(Time::ZERO, asset_duration);
            let (gain, animations, gain_warnings) =
                gain_and_animation(layer, id, flags.audio_enabled, budget);
            warnings.extend(gain_warnings);
            let audio = AudioLayer {
                id,
                name: occurrence.name.clone(),
                description: format!("Editable AE audio from source item {}", source.id),
                is_hidden: false,
                parent: Some(occurrence.id),
                start_time: Some(0.0),
                volume: gain,
                auto_ducking: None,
                window_ms: fx_schema::default_audio_window_ms(),
                source: AudioSource {
                    asset_id: logical_id.clone(),
                    enhancement: None,
                },
                metadata: None,
                captions_enabled: None,
                caption_presentation: None,
                source_range,
                playback: fx_schema::LayerPlayback::linear(
                    active_range,
                    active_range,
                    source_range,
                    0,
                )
                .expect("validated media ranges form a finite playback mapping"),
                preserve_audio_pitch: false,
                source_intrinsic_duration: asset_duration,
            };
            (vec![FxLayer::Audio(audio)], animations)
        }
        MediaKind::ImageSequence => unreachable!("handled before asset allocation"),
    };

    Ok(MediaLayerConversion {
        layers,
        animations,
        next_id,
        warnings,
        assets: vec![asset_request],
    })
}

#[cfg(test)]
mod tests;
