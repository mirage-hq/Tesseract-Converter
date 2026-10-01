//! Owned staging and no-replace publication for AEP media packages.

mod metadata;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
};

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
}

/// Verifies and stages every requested archive asset below an owned temporary
/// directory. Archive entry paths are never reused as original AEP paths.
pub(super) fn prepare_media(
    archive: &TesseractFile,
    requests: &[MediaRequest],
    staging: &Path,
    _canvas: Dimensions,
) -> Result<PreparedMedia, AepConversionError> {
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

    let mut sources = BTreeMap::new();
    let mut files = Vec::new();
    let mut unsupported = Vec::new();
    for (ordinal, (asset_id, (request, kinds))) in grouped.into_iter().enumerate() {
        if kinds & IMAGE_KIND != 0 && kinds != IMAGE_KIND {
            return Err(AepConversionError::Input(
                "one archive asset is requested with incompatible media kinds",
            ));
        }
        let asset = archive.asset(asset_id)?;
        let asset_kind = asset.descriptor().kind;
        ensure_kind(asset_kind, kinds)?;
        let materialized = asset.materialize(&cache)?;
        let interpreted = match interpret(InterpretRequest {
            request,
            requested_kinds: kinds,
            asset_kind,
            archive_path: asset.descriptor().path.as_str(),
            content_type: asset.descriptor().content_type.as_str(),
            materialized_path: materialized.path(),
            byte_length: asset.descriptor().byte_length,
            ordinal,
        }) {
            Ok(source) => source,
            Err(InterpretError::Unsupported(reason)) => {
                unsupported.push((asset_id.to_owned(), reason.to_owned()));
                continue;
            }
            Err(InterpretError::Fatal(error)) => return Err(error),
        };
        let target = staging.join(interpreted.path.as_str());
        fs::hard_link(materialized.path(), &target)
            .map_err(|source| AepConversionError::io("stage verified media", &target, source))?;
        files.push(interpreted.path.clone());
        sources.insert(asset_id.to_owned(), interpreted);
    }
    private_cache.remove()?;
    Ok(PreparedMedia {
        sources,
        files,
        unsupported,
    })
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
    if requested_kinds & (VIDEO_KIND | AUDIO_KIND) != 0
        && (content_type == "video/quicktime" || is_extension(archive_path, "mov"))
    {
        let movie = metadata::quicktime_from_reader(&mut input, byte_length)
            .map_err(|error| interpret_metadata_error(error, materialized_path))?;
        if requested_kinds == AUDIO_KIND && movie.audio_sample_rate == 0.0 {
            return Err(InterpretError::Unsupported(
                "QuickTime source is requested only as audio but has no supported enabled audio track",
            ));
        }
        return Ok(ResolvedMediaSource {
            asset_id: request.asset_id.clone(),
            path: package_path(ordinal, request.asset_id.as_str(), "mov")
                .map_err(InterpretError::Fatal)?,
            format: NativeSourceFormat::QuickTime,
            dimensions: movie.dimensions,
            duration_millis: movie.duration_millis,
            frame_rate: movie.frame_rate,
            audio_sample_rate: movie.audio_sample_rate,
            wave_metadata: None,
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
                frame_rate: NativeFrameRate::integer(0),
                audio_sample_rate: 0.0,
                wave_metadata: None,
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
                frame_rate: NativeFrameRate::integer(0),
                audio_sample_rate: f64::from(wave.sample_rate),
                wave_metadata: Some(NativeWaveMetadata {
                    sample_frames: wave.sample_frames,
                    file_length: wave.file_length,
                }),
            })
        }
        FootageKind::Video => Err(InterpretError::Unsupported(
            "video asset is not a source-identified QuickTime .mov profile",
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

/// Publishes a staged `project.aep` plus media files without replacement.
/// Rollback and cancellation-by-drop remove only links/directories created by
/// this invocation; caller-owned entries are never recursively deleted.
pub(super) fn publish_package(
    staging: &Path,
    destination: &Path,
    media: &[RelativeMediaPath],
) -> Result<(), AepConversionError> {
    let mut paths = Vec::with_capacity(media.len() + 1);
    paths.push(PathBuf::from("project.aep"));
    paths.extend(media.iter().map(|path| PathBuf::from(path.as_str())));
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
        fs::hard_link(&source, &target).map_err(|error| {
            AepConversionError::io("publish converted package entry", &target, error)
        })?;
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
