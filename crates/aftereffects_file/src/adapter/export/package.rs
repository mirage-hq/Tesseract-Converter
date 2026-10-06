//! Owned staging and no-replace publication for AEP media packages.

pub(super) mod aliases;
pub(super) mod fonts;
mod metadata;
mod png;
mod wave;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
};

use super::staging::AepPreparationControl;
use fx_schema::Dimensions;
use tesseract_file::{AssetKind, MaterializationCache, TesseractFile};

use super::super::AepConversionError;
use crate::{
    export_document::media::{MediaRequest, ResolvedMediaSource},
    writer::footage::{
        FootageKind, NativeFrameRate, NativeSourceFormat, NativeWaveMetadata, RelativeMediaPath,
    },
};

/// Media metadata plus exact owned staging paths, keyed by current asset ID.
pub(super) struct PreparedMedia {
    pub(super) sources: BTreeMap<String, ResolvedMediaSource>,
    pub(super) files: Vec<RelativeMediaPath>,
    /// Valid assets whose native source profile cannot be represented yet.
    /// The adapter reports these per asset while shared lowering omits layers
    /// whose source is absent.
    pub(super) unsupported: Vec<(String, String)>,
    /// Prepared sources whose samples or colour interpretation may differ.
    pub(super) approximations: Vec<(String, &'static str)>,
    pub(super) preparations: Vec<(String, String)>,
    /// A verified video failed original admission, but its scope withheld
    /// destination preparation, so the media engine did not run for it.
    pub(super) preparation_withheld: bool,
}

/// Destination preparation for a selected video that fails original admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VideoPreparation {
    /// Prepare the whole source, then revalidate it with the native parser.
    Run,
    /// Record the need without invoking the media engine. All originals still
    /// pass the same archive, integrity and metadata checks.
    Withhold,
    /// Keep the original bytes or report unsupported native interpretation;
    /// never invoke video remuxing or encoding for this scope.
    PreserveOriginal,
}

/// Verifies and stages every requested archive asset below an owned temporary
/// directory. Archive entry paths are never reused as original AEP paths.
pub(super) fn prepare_media(
    archive: &TesseractFile,
    requests: &[MediaRequest],
    staging: &Path,
    _canvas: Dimensions,
    control: AepPreparationControl<'_>,
) -> Result<PreparedMedia, AepConversionError> {
    prepare_media_impl(archive, requests, staging, None, control)
}

/// Hybrid picture scopes opt into destination preparation, never ordinary admission.
pub(super) fn prepare_picture_scope_media(
    archive: &TesseractFile,
    requests: &[MediaRequest],
    staging: &Path,
    preparation: VideoPreparation,
    control: AepPreparationControl<'_>,
) -> Result<PreparedMedia, AepConversionError> {
    prepare_media_impl(archive, requests, staging, Some(preparation), control)
}

/// Without `preparation`, ordinary export reports unsupported video as omitted.
fn prepare_media_impl(
    archive: &TesseractFile,
    requests: &[MediaRequest],
    staging: &Path,
    preparation: Option<VideoPreparation>,
    control: AepPreparationControl<'_>,
) -> Result<PreparedMedia, AepConversionError> {
    control.check_cancelled()?;
    let progress = control.progress;
    let uncancelled = std::sync::atomic::AtomicBool::new(false);
    let media_dir = staging.join("media");
    fs::create_dir(&media_dir)
        .map_err(|source| AepConversionError::io("create media staging", &media_dir, source))?;
    let private_cache = PrivateCache::new(staging.join(".asset-materialization-cache"));
    let cache = MaterializationCache::new(private_cache.path());
    let mut grouped = BTreeMap::<&str, (&MediaRequest, u8)>::new();
    for request in requests {
        let asset_id = request.asset_id.as_str();
        let mask = kind_mask(request.kind);
        if let Some((_, kinds)) = grouped.get_mut(asset_id) {
            *kinds |= mask;
        } else {
            grouped.insert(asset_id, (request, mask));
        }
    }

    let phase = progress.phase("prepare AEP media", "assets", grouped.len());
    let mut sources = BTreeMap::new();
    let mut files = Vec::new();
    let mut unsupported = Vec::new();
    let mut approximations = Vec::new();
    let mut preparations = Vec::new();
    let mut preparation_withheld = false;
    for (ordinal, (asset_id, (request, kinds))) in grouped.into_iter().enumerate() {
        control.check_cancelled()?;
        if kinds & IMAGE_KIND != 0 && kinds != IMAGE_KIND {
            return Err(AepConversionError::Input(
                "one archive asset is requested with incompatible media kinds",
            ));
        }
        let asset = archive.asset(asset_id)?;
        let asset_kind = asset.descriptor().kind;
        ensure_kind(asset_kind, kinds)?;
        let materialized = asset.materialize(&cache)?;
        if request.kind == FootageKind::Image
            && is_preparable_image(
                asset.descriptor().path.as_str(),
                asset.descriptor().content_type.as_str(),
            )
        {
            if (asset.descriptor().content_type == "image/png"
                || is_extension(asset.descriptor().path.as_str(), "png"))
                && let Some((format, dimensions)) = png::native_profile(materialized.path())?
            {
                let path = package_path(ordinal, asset_id, "png")?;
                let target = staging.join(path.as_str());
                crate::adapter::publication::publish_file(
                    materialized.path(),
                    &target,
                    fs::hard_link,
                )
                .map_err(|error| {
                    AepConversionError::io("stage byte-preserved PNG", &target, error)
                })?;
                files.push(path.clone());
                sources.insert(
                    asset_id.to_owned(),
                    ResolvedMediaSource {
                        asset_id: request.asset_id.clone(),
                        path,
                        format,
                        dimensions,
                        duration_millis: 0,
                        duration_millis_floor: 0,
                        duration_native_ticks: None,
                        frame_rate: NativeFrameRate::integer(0),
                        audio_sample_rate: 0.0,
                        wave_metadata: None,
                        native_duration: None,
                    },
                );
                approximations.push((asset_id.to_owned(), "PNG RGB8/RGBA8 staged byte-preserved with native PNG interpretation; embedded colour metadata and alpha bytes are retained, but general tagged-colour and alpha fidelity remain unverified"));
                control.check_cancelled()?;
                phase.update(ordinal + 1);
                continue;
            }
            let path = package_path(ordinal, request.asset_id.as_str(), "exr")?;
            let target = staging.join(path.as_str());
            normalize_image_to_exr(materialized.path(), &target)?;
            let byte_length = fs::metadata(&target)
                .map_err(|source| {
                    AepConversionError::io("read prepared image metadata", &target, source)
                })?
                .len();
            let interpreted = interpret(InterpretRequest {
                request,
                requested_kinds: kinds,
                asset_kind,
                archive_path: "prepared.exr",
                content_type: "image/x-exr",
                materialized_path: &target,
                byte_length,
                ordinal,
            })
            .map_err(|error| match error {
                InterpretError::Unsupported(_) => AepConversionError::Input(
                    "prepared image does not satisfy the native OpenEXR profile",
                ),
                InterpretError::Fatal(error) => error,
            })?;
            files.push(interpreted.path.clone());
            sources.insert(asset_id.to_owned(), interpreted);
            approximations.push((asset_id.to_owned(), "PNG/JPEG decoded to OpenEXR for native AEP; decoded pixels and alpha are retained as float samples, but embedded colour profiles and AE colour management are unverified"));
            control.check_cancelled()?;
            phase.update(ordinal + 1);
            continue;
        }
        if kinds == AUDIO_KIND
            && asset_kind == AssetKind::Audio
            && asset.descriptor().byte_length >= 60
            && (matches!(
                asset.descriptor().content_type.as_str(),
                "audio/wav" | "audio/wave"
            ) || is_extension(asset.descriptor().path.as_str(), "wav"))
        {
            let path = package_path(ordinal, request.asset_id.as_str(), "wav")?;
            let target = staging.join(path.as_str());
            match wave::normalize_extensible_pcm24(
                materialized.path(),
                &target,
                asset.descriptor().byte_length,
            ) {
                Ok(true) => {
                    let length = fs::metadata(&target)
                        .map_err(|error| {
                            AepConversionError::io("stat normalized WAVE", &target, error)
                        })?
                        .len();
                    let interpreted = match interpret(InterpretRequest {
                        request,
                        requested_kinds: kinds,
                        asset_kind,
                        archive_path: "normalized.wav",
                        content_type: "audio/wav",
                        materialized_path: &target,
                        byte_length: length,
                        ordinal,
                    }) {
                        Ok(source) => source,
                        Err(InterpretError::Unsupported(reason)) => {
                            fs::remove_file(&target).map_err(|error| {
                                AepConversionError::io(
                                    "remove unsupported normalized WAVE",
                                    &target,
                                    error,
                                )
                            })?;
                            unsupported.push((asset_id.to_owned(), reason.to_owned()));
                            control.check_cancelled()?;
                            phase.update(ordinal + 1);
                            continue;
                        }
                        Err(InterpretError::Fatal(error)) => return Err(error),
                    };
                    files.push(interpreted.path.clone());
                    sources.insert(asset_id.to_owned(), interpreted);
                    approximations.push((asset_id.to_owned(), "WAVE_EXTENSIBLE mono/stereo PCM24 envelope normalized to RIFF/WAVE PCM24 without modifying any audio sample bytes; Adobe source decoding remains unverified"));
                    control.check_cancelled()?;
                    phase.update(ordinal + 1);
                    continue;
                }
                Ok(false) => {}
                Err(InterpretError::Unsupported(reason)) => {
                    unsupported.push((asset_id.to_owned(), reason.to_owned()));
                    control.check_cancelled()?;
                    phase.update(ordinal + 1);
                    continue;
                }
                Err(InterpretError::Fatal(error)) => return Err(error),
            }
        }
        if kinds == AUDIO_KIND
            && asset_kind == AssetKind::Audio
            && (asset.descriptor().content_type == "audio/mpeg"
                || is_extension(asset.descriptor().path.as_str(), "mp3"))
        {
            let path = package_path(ordinal, request.asset_id.as_str(), "wav")?;
            let target = staging.join(path.as_str());
            let prepared = transcode_audio(
                materialized.path(),
                &target,
                control.cancelled.unwrap_or(&uncancelled),
            );
            match prepared {
                Ok(_) => {}
                Err(media_transcode::TranscodeError::Cancelled) => {
                    return Err(AepConversionError::Cancelled);
                }
                Err(media_transcode::TranscodeError::Policy(reason)) => {
                    unsupported.push((
                        asset_id.to_owned(),
                        format!("bounded MP3 audio preparation unsupported: {reason}"),
                    ));
                    control.check_cancelled()?;
                    phase.update(ordinal + 1);
                    continue;
                }
                Err(source) => {
                    return Err(AepConversionError::AudioPreparation {
                        asset_id: asset_id.to_owned(),
                        source,
                    });
                }
            }
            let length = fs::metadata(&target)
                .map_err(|error| AepConversionError::io("stat prepared MP3 WAVE", &target, error))?
                .len();
            let interpreted = match interpret(InterpretRequest {
                request,
                requested_kinds: kinds,
                asset_kind,
                archive_path: "prepared.wav",
                content_type: "audio/wav",
                materialized_path: &target,
                byte_length: length,
                ordinal,
            }) {
                Ok(source) => source,
                Err(InterpretError::Unsupported(reason)) => {
                    fs::remove_file(&target).map_err(|error| {
                        AepConversionError::io(
                            "remove unsupported prepared MP3 WAVE",
                            &target,
                            error,
                        )
                    })?;
                    unsupported.push((asset_id.to_owned(), reason.to_owned()));
                    control.check_cancelled()?;
                    phase.update(ordinal + 1);
                    continue;
                }
                Err(InterpretError::Fatal(error)) => return Err(error),
            };
            files.push(interpreted.path.clone());
            sources.insert(asset_id.to_owned(), interpreted);
            approximations.push((asset_id.to_owned(), "Bounded MP3 decoder priming normalized to a zero-start PCM WAVE. Output samples and duration are independently re-probed, but MP3 decoder timing versus the FX player and Adobe playback have not been compared"));
            control.check_cancelled()?;
            phase.update(ordinal + 1);
            continue;
        }
        let original = interpret(InterpretRequest {
            request,
            requested_kinds: kinds,
            asset_kind,
            archive_path: asset.descriptor().path.as_str(),
            content_type: asset.descriptor().content_type.as_str(),
            materialized_path: materialized.path(),
            byte_length: asset.descriptor().byte_length,
            ordinal,
        });
        let interpreted = match original {
            Ok(source) => {
                let target = staging.join(source.path.as_str());
                crate::adapter::publication::publish_file(
                    materialized.path(),
                    &target,
                    fs::hard_link,
                )
                .map_err(|source| {
                    AepConversionError::io("stage verified media", &target, source)
                })?;
                source
            }
            Err(InterpretError::Unsupported(reason))
                if preparation == Some(VideoPreparation::PreserveOriginal)
                    && kinds & VIDEO_KIND != 0 =>
            {
                unsupported.push((
                    asset_id.to_owned(),
                    format!(
                        "{reason}; original video bytes required, destination preparation disabled"
                    ),
                ));
                control.check_cancelled()?;
                phase.update(ordinal + 1);
                continue;
            }
            Err(InterpretError::Unsupported(_))
                if preparation == Some(VideoPreparation::Withhold) && kinds & VIDEO_KIND != 0 =>
            {
                preparation_withheld = true;
                control.check_cancelled()?;
                phase.update(ordinal + 1);
                continue;
            }
            Err(InterpretError::Unsupported(_))
                if preparation == Some(VideoPreparation::Run) && kinds & VIDEO_KIND != 0 =>
            {
                let relative = package_path(ordinal, asset_id, "mov")?;
                let target = staging.join(relative.as_str());
                let result = media_transcode::run_for_after_effects(
                    media_transcode::TranscodeRequest {
                        input: materialized.path(),
                        output: &target,
                        backend: media_transcode::Backend::Library,
                        cancelled: control.cancelled.unwrap_or(&uncancelled),
                    },
                    &mut |_| {},
                );
                let result = match result {
                    Ok(result) => result,
                    Err(media_transcode::TranscodeError::Cancelled) => {
                        return Err(AepConversionError::Cancelled);
                    }
                    Err(media_transcode::TranscodeError::Policy(reason)) => {
                        unsupported.push((
                            asset_id.to_owned(),
                            format!("AE destination preparation unsupported: {reason}"),
                        ));
                        control.check_cancelled()?;
                        phase.update(ordinal + 1);
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                };
                let byte_length = fs::metadata(&target)
                    .map_err(|source| {
                        AepConversionError::io("inspect prepared AE media", &target, source)
                    })?
                    .len();
                let source = match interpret(InterpretRequest {
                    request,
                    requested_kinds: kinds,
                    asset_kind,
                    archive_path: "prepared.mov",
                    content_type: "video/quicktime",
                    materialized_path: &target,
                    byte_length,
                    ordinal,
                }) {
                    Ok(source) => source,
                    Err(InterpretError::Unsupported(reason)) => {
                        unsupported.push((
                            asset_id.to_owned(),
                            format!(
                                "prepared AE media failed native profile revalidation: {reason}"
                            ),
                        ));
                        control.check_cancelled()?;
                        phase.update(ordinal + 1);
                        continue;
                    }
                    Err(InterpretError::Fatal(error)) => return Err(error),
                };
                preparations.push((asset_id.to_owned(), format!(
                    "AE destination media {:?}: {} -> {}; source timing and alpha presence revalidated.{}",
                    result.operation, result.input_sha256, result.output_sha256,
                    if result.operation == media_transcode::Operation::Transcode { " RGB encoding is approximate; native render fidelity remains unverified." } else { " Compressed video packets retained without re-encoding; native render fidelity remains unverified." },
                )));
                source
            }
            Err(InterpretError::Unsupported(reason)) => {
                unsupported.push((asset_id.to_owned(), reason.to_owned()));
                control.check_cancelled()?;
                phase.update(ordinal + 1);
                continue;
            }
            Err(InterpretError::Fatal(error)) => return Err(error),
        };
        files.push(interpreted.path.clone());
        sources.insert(asset_id.to_owned(), interpreted);
        control.check_cancelled()?;
        phase.update(ordinal + 1);
    }
    control.check_cancelled()?;
    private_cache.remove()?;
    Ok(PreparedMedia {
        sources,
        files,
        unsupported,
        approximations,
        preparations,
        preparation_withheld,
    })
}

fn transcode_audio(
    input: &Path,
    output: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<media_transcode::TranscodeResult, media_transcode::TranscodeError> {
    media_transcode::run_for_after_effects_audio(
        media_transcode::TranscodeRequest {
            input,
            output,
            backend: media_transcode::Backend::Library,
            cancelled,
        },
        &mut |_| {},
    )
}

struct PrivateCache {
    path: PathBuf,
    removed: bool,
}

impl PrivateCache {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            removed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn remove(mut self) -> Result<(), AepConversionError> {
        match fs::remove_dir_all(&self.path) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AepConversionError::io(
                    "remove private asset cache",
                    &self.path,
                    source,
                ));
            }
        }
        self.removed = true;
        Ok(())
    }
}

impl Drop for PrivateCache {
    fn drop(&mut self) {
        if !self.removed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

const IMAGE_KIND: u8 = 1;
const VIDEO_KIND: u8 = 2;
const AUDIO_KIND: u8 = 4;

const fn kind_mask(kind: FootageKind) -> u8 {
    match kind {
        FootageKind::Image => IMAGE_KIND,
        FootageKind::Video => VIDEO_KIND,
        FootageKind::Audio => AUDIO_KIND,
    }
}

fn ensure_kind(actual: AssetKind, expected: u8) -> Result<(), AepConversionError> {
    let valid = match actual {
        AssetKind::Image => expected == IMAGE_KIND,
        AssetKind::Video => expected != 0 && expected & !(VIDEO_KIND | AUDIO_KIND) == 0,
        AssetKind::Audio => expected == AUDIO_KIND,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(AepConversionError::Input(
            "archive asset kind does not match current FX media layer",
        ))
    }
}

enum InterpretError {
    Unsupported(&'static str),
    Fatal(AepConversionError),
}

struct InterpretRequest<'a> {
    request: &'a MediaRequest,
    requested_kinds: u8,
    asset_kind: AssetKind,
    archive_path: &'a str,
    content_type: &'a str,
    materialized_path: &'a Path,
    byte_length: u64,
    ordinal: usize,
}

fn interpret(input: InterpretRequest<'_>) -> Result<ResolvedMediaSource, InterpretError> {
    let InterpretRequest {
        request,
        requested_kinds,
        asset_kind,
        archive_path,
        content_type,
        materialized_path,
        byte_length,
        ordinal,
    } = input;
    let mut input = File::open(materialized_path).map_err(|source| {
        InterpretError::Fatal(AepConversionError::io(
            "open verified media metadata",
            materialized_path,
            source,
        ))
    })?;
    let is_mp4 = content_type == "video/mp4" || is_extension(archive_path, "mp4");
    if requested_kinds & (VIDEO_KIND | AUDIO_KIND) != 0
        && (is_mp4 || content_type == "video/quicktime" || is_extension(archive_path, "mov"))
    {
        let movie = if is_mp4 {
            metadata::mp4_from_reader(&mut input, byte_length)
        } else {
            metadata::quicktime_from_reader(&mut input, byte_length)
        }
        .map_err(|error| interpret_metadata_error(error, materialized_path))?;
        if requested_kinds == AUDIO_KIND && movie.audio_sample_rate == 0.0 {
            return Err(InterpretError::Unsupported(
                "QuickTime source is requested only as audio but has no supported enabled audio track",
            ));
        }
        return Ok(ResolvedMediaSource {
            asset_id: request.asset_id.clone(),
            path: package_path(
                ordinal,
                request.asset_id.as_str(),
                if is_mp4 { "mp4" } else { "mov" },
            )
            .map_err(InterpretError::Fatal)?,
            format: match movie.video_codec {
                [b'a', b'v', b'c', b'1'] => NativeSourceFormat::QuickTime,
                [b'a', b'p', b'4', b'h'] => NativeSourceFormat::QuickTimeProRes4444,
                _ => {
                    return Err(InterpretError::Unsupported(
                        "unsupported QuickTime video codec",
                    ));
                }
            },
            dimensions: movie.dimensions,
            duration_millis: movie.duration_millis,
            duration_millis_floor: movie.duration_millis_floor,
            duration_native_ticks: movie.duration_native_ticks,
            frame_rate: movie.frame_rate,
            audio_sample_rate: movie.audio_sample_rate,
            wave_metadata: None,
            native_duration: movie.native_duration,
        });
    }

    match request.kind {
        FootageKind::Image if is_extension(archive_path, "exr") => {
            let dimensions = metadata::open_exr_dimensions(&mut input, byte_length)
                .map_err(|error| interpret_metadata_error(error, materialized_path))?;
            Ok(ResolvedMediaSource {
                asset_id: request.asset_id.clone(),
                path: package_path(ordinal, request.asset_id.as_str(), "exr")
                    .map_err(InterpretError::Fatal)?,
                format: NativeSourceFormat::OpenExr,
                dimensions,
                duration_millis: 0,
                duration_millis_floor: 0,
                duration_native_ticks: None,
                frame_rate: NativeFrameRate::integer(0),
                audio_sample_rate: 0.0,
                wave_metadata: None,
                native_duration: None,
            })
        }
        FootageKind::Audio
            if asset_kind == AssetKind::Audio
                && (content_type == "audio/wav"
                    || content_type == "audio/wave"
                    || is_extension(archive_path, "wav")) =>
        {
            let wave = metadata::wave_from_reader(&mut input, byte_length)
                .map_err(|error| interpret_metadata_error(error, materialized_path))?;
            Ok(ResolvedMediaSource {
                asset_id: request.asset_id.clone(),
                path: package_path(ordinal, request.asset_id.as_str(), "wav")
                    .map_err(InterpretError::Fatal)?,
                format: NativeSourceFormat::Wave,
                dimensions: [0, 0],
                duration_millis: wave.duration_millis,
                duration_millis_floor: wave.duration_millis,
                duration_native_ticks: None,
                frame_rate: NativeFrameRate::integer(0),
                audio_sample_rate: f64::from(wave.sample_rate),
                wave_metadata: Some(NativeWaveMetadata {
                    sample_frames: wave.sample_frames,
                    file_length: wave.file_length,
                }),
                native_duration: None,
            })
        }
        FootageKind::Video => Err(InterpretError::Unsupported(
            "video asset is not a source-identified QuickTime .mov or AVC .mp4 profile",
        )),
        FootageKind::Image => Err(InterpretError::Unsupported(
            "image asset does not use the source-backed OpenEXR native profile",
        )),
        FootageKind::Audio => Err(InterpretError::Unsupported(
            "audio asset does not use the source-backed RIFF/WAVE native profile",
        )),
    }
}

fn interpret_metadata_error(error: metadata::MetadataReadError, path: &Path) -> InterpretError {
    match error {
        metadata::MetadataReadError::Profile(metadata::MetadataError::Unsupported(reason)) => {
            InterpretError::Unsupported(reason)
        }
        metadata::MetadataReadError::Profile(metadata::MetadataError::Malformed(reason)) => {
            InterpretError::Fatal(AepConversionError::Input(reason))
        }
        metadata::MetadataReadError::Io(source) => InterpretError::Fatal(AepConversionError::io(
            "read verified media metadata",
            path,
            source,
        )),
    }
}

fn package_path(
    ordinal: usize,
    asset_id: &str,
    extension: &str,
) -> Result<RelativeMediaPath, AepConversionError> {
    const MAX_STEM_CHARS: usize = 48;

    let mut safe = String::with_capacity(asset_id.len().min(MAX_STEM_CHARS));
    for value in asset_id.chars().take(MAX_STEM_CHARS) {
        safe.push(
            if value.is_ascii_alphanumeric() || matches!(value, '-' | '_') {
                value
            } else {
                '_'
            },
        );
    }
    if safe.is_empty() {
        return Err(AepConversionError::Input("asset ID has no safe filename"));
    }
    RelativeMediaPath::new(format!("media/{ordinal:06}-{safe}.{extension}"))
        .map_err(AepConversionError::from)
}

fn is_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn is_preparable_image(path: &str, content_type: &str) -> bool {
    matches!(content_type, "image/png" | "image/jpeg")
        || ["png", "jpg", "jpeg"]
            .into_iter()
            .any(|extension| is_extension(path, extension))
}

fn normalize_image_to_exr(source: &Path, target: &Path) -> Result<(), AepConversionError> {
    let mut reader = image::ImageReader::open(source)
        .map_err(|error| AepConversionError::io("open image for dimension check", source, error))?
        .with_guessed_format()
        .map_err(|error| AepConversionError::io("identify image format", source, error))?;
    // Preserve source resolution without imposing decoder allocation policy.
    reader.no_limits();
    let image = reader.decode()?;
    if image.width() == 0 || image.height() == 0 {
        return Err(AepConversionError::Input("image has zero dimensions"));
    }
    image
        .to_rgba32f()
        .save_with_format(target, image::ImageFormat::OpenExr)?;
    Ok(())
}

/// Publishes a staged `project.aep` plus media files without replacement.
/// Rollback and cancellation-by-drop remove only files/directories created by
/// this invocation; caller-owned entries are never recursively deleted.
pub(super) fn publish_package(
    staging: &Path,
    destination: &Path,
    media: &[RelativeMediaPath],
    fonts: &[String],
) -> Result<(), AepConversionError> {
    let mut paths = Vec::with_capacity(media.len() + fonts.len() + 1);
    paths.push(PathBuf::from("project.aep"));
    paths.extend(media.iter().map(|path| PathBuf::from(path.as_str())));
    // Font paths are generated internally from verified SHA-256 values, never
    // archive filenames. Keep the existing media-only path contract intact.
    paths.extend(fonts.iter().map(PathBuf::from));
    let mut directories = BTreeSet::new();
    for path in &paths {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            directories.insert(parent.to_owned());
        }
    }

    fs::create_dir(destination).map_err(|source| {
        AepConversionError::io("create output package directory", destination, source)
    })?;
    let mut rollback = PublicationRollback::new(destination.to_owned());
    for directory in directories {
        let path = destination.join(directory);
        fs::create_dir(&path).map_err(|source| {
            AepConversionError::io("create output package subdirectory", &path, source)
        })?;
        rollback.directories.push(path);
    }
    for relative in paths {
        let source = staging.join(&relative);
        let target = destination.join(&relative);
        crate::adapter::publication::publish_file(&source, &target, fs::hard_link).map_err(
            |error| AepConversionError::io("publish converted package entry", &target, error),
        )?;
        rollback.files.push(target);
    }
    rollback.committed = true;
    Ok(())
}

struct PublicationRollback {
    root: PathBuf,
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
    committed: bool,
}

impl PublicationRollback {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            files: Vec::new(),
            directories: Vec::new(),
            committed: false,
        }
    }
}

impl Drop for PublicationRollback {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for path in self.files.iter().rev() {
            let _ = fs::remove_file(path);
        }
        for path in self.directories.iter().rev() {
            let _ = fs::remove_dir(path);
        }
        let _ = fs::remove_dir(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Seek, SeekFrom, Write};

    use super::*;

    struct ReadLimit<R> {
        inner: R,
        bytes_read: usize,
        limit: usize,
    }

    impl<R> ReadLimit<R> {
        fn new(inner: R, limit: usize) -> Self {
            Self {
                inner,
                bytes_read: 0,
                limit,
            }
        }
    }

    impl<R: Read> Read for ReadLimit<R> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes_read) {
                return Err(std::io::Error::other(
                    "metadata parser exceeded test read limit",
                ));
            }
            let read = self.inner.read(bytes)?;
            self.bytes_read += read;
            Ok(read)
        }
    }

    impl<R: Seek> Seek for ReadLimit<R> {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(position)
        }
    }

    #[test]
    fn audio_transcode_receives_caller_cancellation_before_opening_input() {
        let parent = tempfile::tempdir().unwrap();
        let sentinel = parent.path().join("keep.txt");
        fs::write(&sentinel, b"keep").unwrap();
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        let output = parent.path().join("prepared.wav");
        let error =
            transcode_audio(&parent.path().join("absent.mp3"), &output, &cancelled).unwrap_err();
        assert!(matches!(error, media_transcode::TranscodeError::Cancelled));
        assert!(!output.exists());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
        assert_eq!(fs::read(sentinel).unwrap(), b"keep");
    }

    #[test]
    fn png_is_normalized_to_revalidated_open_exr() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 128]))
            .save(&source)
            .unwrap();
        let target = root.path().join("prepared.exr");

        normalize_image_to_exr(&source, &target).unwrap();

        let length = fs::metadata(&target).unwrap().len();
        let mut file = File::open(&target).unwrap();
        assert_eq!(
            metadata::open_exr_dimensions(&mut file, length).unwrap(),
            [3, 2]
        );
        let pixel = image::open(&target).unwrap().to_rgba32f().get_pixel(0, 0).0;
        for (actual, expected) in pixel.into_iter().zip([10.0, 20.0, 30.0, 128.0]) {
            assert!((actual - expected / 255.0).abs() < 0.001);
        }
    }

    #[test]
    fn image_past_former_pixel_limit_preserves_dimensions_and_pixels() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("oversized.png");
        let target = root.path().join("prepared.exr");
        let mut pixels = image::RgbaImage::from_pixel(4000, 3001, image::Rgba([10, 20, 30, 128]));
        pixels.put_pixel(3999, 3000, image::Rgba([255, 64, 0, 255]));
        pixels.save(&source).unwrap();
        drop(pixels);
        normalize_image_to_exr(&source, &target).unwrap();
        let length = fs::metadata(&target).unwrap().len();
        let mut file = File::open(&target).unwrap();
        assert_eq!(
            metadata::open_exr_dimensions(&mut file, length).unwrap(),
            [4000, 3001]
        );
        let pixels = image::open(&target).unwrap().to_rgba32f();
        for (position, expected) in [
            ((0, 0), [10.0, 20.0, 30.0, 128.0]),
            ((3999, 3000), [255.0, 64.0, 0.0, 255.0]),
        ] {
            for (actual, expected) in pixels
                .get_pixel(position.0, position.1)
                .0
                .into_iter()
                .zip(expected)
            {
                assert!((actual - expected / 255.0).abs() < 0.001);
            }
        }
    }

    #[test]
    fn malformed_images_are_rejected_without_output() {
        let root = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("truncated.png", &b"\x89PNG\r\n\x1a\n"[..]),
            ("truncated.jpg", &b"\xff\xd8\xff"[..]),
        ] {
            let source = root.path().join(name);
            let target = root.path().join(format!("{name}.exr"));
            fs::write(&source, bytes).unwrap();
            assert!(normalize_image_to_exr(&source, &target).is_err());
            assert!(!target.exists());
        }
    }

    #[test]
    fn jpeg_is_normalized_to_revalidated_open_exr() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.jpg");
        image::RgbImage::from_pixel(2, 1, image::Rgb([10, 20, 30]))
            .save(&source)
            .unwrap();
        let target = root.path().join("prepared.exr");
        normalize_image_to_exr(&source, &target).unwrap();
        let length = fs::metadata(&target).unwrap().len();
        let mut file = File::open(&target).unwrap();
        assert_eq!(
            metadata::open_exr_dimensions(&mut file, length).unwrap(),
            [2, 1]
        );
        assert_eq!(
            image::open(&target).unwrap().to_rgba32f().get_pixel(0, 0).0[3],
            1.0
        );
    }

    fn parse_wave(bytes: &[u8]) -> Result<metadata::WaveMetadata, metadata::MetadataReadError> {
        metadata::wave_from_reader(&mut Cursor::new(bytes), u64::try_from(bytes.len()).unwrap())
    }

    fn wave_chunk(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(8 + payload.len() + (payload.len() & 1));
        chunk.extend_from_slice(&kind);
        chunk.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        chunk.extend_from_slice(payload);
        if payload.len() & 1 != 0 {
            chunk.push(0);
        }
        chunk
    }

    fn pcm_format() -> [u8; 16] {
        [1, 0, 1, 0, 0x40, 0x1f, 0, 0, 0x80, 0x3e, 0, 0, 2, 0, 16, 0]
    }

    fn wave_file(chunks: &[Vec<u8>]) -> Vec<u8> {
        let payload_length: usize = chunks.iter().map(Vec::len).sum();
        let mut bytes = Vec::with_capacity(12 + payload_length);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&u32::try_from(4 + payload_length).unwrap().to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        for chunk in chunks {
            bytes.extend_from_slice(chunk);
        }
        bytes
    }

    #[test]
    fn wave_metadata_retains_exact_frames_and_verified_file_length() {
        let original = include_bytes!("../../../tests/fixtures/audio_e2e/sound.wav");
        let native = parse_wave(original).unwrap();
        assert_eq!(native.sample_rate, 48_000);
        assert_eq!(native.sample_frames, 192_000);
        assert_eq!(native.file_length, 768_044);
        assert_eq!(native.duration_millis, 4_000);

        // 262,094 frames at 44.1 kHz last 5,943.17 ms: the persisted
        // millisecond duration rounds to 5,944, losing 36 frames if reversed.
        let mut fmt = pcm_format();
        fmt[4..8].copy_from_slice(&44_100_u32.to_le_bytes());
        fmt[8..12].copy_from_slice(&88_200_u32.to_le_bytes());
        let waveform = wave_file(&[
            wave_chunk(*b"fmt ", &fmt),
            wave_chunk(*b"data", &vec![0; 262_094 * 2]),
        ]);
        let parsed = parse_wave(&waveform).unwrap();
        assert_eq!(parsed.sample_frames, 262_094);
        assert_eq!(parsed.file_length, u32::try_from(waveform.len()).unwrap());
        assert_eq!(parsed.duration_millis, 5_944);
    }

    #[test]
    fn recognized_rf64_wave_is_unsupported_instead_of_malformed() {
        assert!(matches!(
            parse_wave(b"RF64\xff\xff\xff\xffWAVE"),
            Err(metadata::MetadataReadError::Profile(
                metadata::MetadataError::Unsupported(
                    "RF64/WAVE is outside the source-backed RIFF/WAVE profile"
                )
            ))
        ));
    }

    #[test]
    fn rejects_unpinned_wave_encodings_without_changing_malformed_errors() {
        let mut wave = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x06\0\x01\0\x40\x1f\0\0\x40\x1f\0\0\x01\0\x08\0data\0\0\0\0".to_vec();
        assert!(matches!(
            parse_wave(&wave),
            Err(metadata::MetadataReadError::Profile(
                metadata::MetadataError::Unsupported(_)
            ))
        ));
        wave[20..22].copy_from_slice(&1_u16.to_le_bytes());
        assert!(matches!(
            parse_wave(&wave),
            Err(metadata::MetadataReadError::Profile(
                metadata::MetadataError::Malformed(_)
            ))
        ));
    }

    #[test]
    fn duplicate_wave_format_and_missing_required_format_are_malformed() {
        let format = wave_chunk(*b"fmt ", &pcm_format());
        let data = wave_chunk(*b"data", &[0, 0]);
        let duplicate = wave_file(&[format.clone(), format, data.clone()]);
        assert!(matches!(
            parse_wave(&duplicate),
            Err(metadata::MetadataReadError::Profile(
                metadata::MetadataError::Malformed(
                    "RIFF/WAVE has malformed or duplicate format metadata"
                )
            ))
        ));
        let missing = wave_file(&[data]);
        assert!(matches!(
            parse_wave(&missing),
            Err(metadata::MetadataReadError::Profile(
                metadata::MetadataError::Malformed("RIFF/WAVE has no format metadata")
            ))
        ));
    }

    // These sparse-reader regressions target the Read + Seek boundary introduced
    // by this fix. There was no callable streaming helper to execute before it.
    #[test]
    fn sparse_wave_data_and_exr_pixels_are_skipped_under_a_tiny_read_limit() {
        let wave_data_length = 16 * 1024 * 1024_u32;
        let wave_length = 44_u64 + u64::from(wave_data_length);
        let mut wave_header = wave_file(&[
            wave_chunk(*b"fmt ", &pcm_format()),
            wave_chunk(*b"data", &[]),
        ]);
        wave_header[4..8].copy_from_slice(&u32::try_from(wave_length - 8).unwrap().to_le_bytes());
        wave_header[40..44].copy_from_slice(&wave_data_length.to_le_bytes());
        let mut wave = tempfile::tempfile().unwrap();
        wave.write_all(&wave_header).unwrap();
        wave.set_len(wave_length).unwrap();
        let mut wave = ReadLimit::new(wave, 64);
        let parsed = metadata::wave_from_reader(&mut wave, wave_length).unwrap();
        assert_eq!(parsed.sample_rate, 8_000);
        assert_eq!(parsed.sample_frames, wave_data_length / 2);
        assert_eq!(parsed.file_length, u32::try_from(wave_length).unwrap());
        assert_eq!(wave.bytes_read, 44);

        let mut exr_header = 20_000_630_u32.to_le_bytes().to_vec();
        exr_header.extend_from_slice(&2_u32.to_le_bytes());
        exr_header.extend_from_slice(b"dataWindow\0box2i\0");
        exr_header.extend_from_slice(&16_u32.to_le_bytes());
        exr_header.extend_from_slice(&0_i32.to_le_bytes());
        exr_header.extend_from_slice(&0_i32.to_le_bytes());
        exr_header.extend_from_slice(&31_i32.to_le_bytes());
        exr_header.extend_from_slice(&17_i32.to_le_bytes());
        exr_header.push(0);
        let exr_length = 32 * 1024 * 1024_u64;
        let mut exr = tempfile::tempfile().unwrap();
        exr.write_all(&exr_header).unwrap();
        exr.set_len(exr_length).unwrap();
        let mut exr = ReadLimit::new(exr, 64);
        assert_eq!(
            metadata::open_exr_dimensions(&mut exr, exr_length).unwrap(),
            [32, 18]
        );
        assert_eq!(exr.bytes_read, exr_header.len());
    }

    #[test]
    fn exr_metadata_past_former_byte_quota_skips_unknown_payload() {
        let payload_length = 8 * 1024 * 1024_u32 + 1;
        let mut header = 20_000_630_u32.to_le_bytes().to_vec();
        header.extend_from_slice(&2_u32.to_le_bytes());
        header.extend_from_slice(b"large\0blob\0");
        header.extend_from_slice(&payload_length.to_le_bytes());
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&header).unwrap();
        file.seek(SeekFrom::Start(
            u64::try_from(header.len()).unwrap() + u64::from(payload_length),
        ))
        .unwrap();
        let mut trailing = b"dataWindow\0box2i\0".to_vec();
        trailing.extend_from_slice(&16_u32.to_le_bytes());
        for value in [0_i32, 0, 31, 17] {
            trailing.extend_from_slice(&value.to_le_bytes());
        }
        trailing.push(0);
        file.write_all(&trailing).unwrap();
        let file_length = file.seek(SeekFrom::End(0)).unwrap();
        let mut file = ReadLimit::new(file, header.len() + trailing.len());
        assert_eq!(
            metadata::open_exr_dimensions(&mut file, file_length).unwrap(),
            [32, 18]
        );
        assert_eq!(file.bytes_read, header.len() + trailing.len());
    }
}
