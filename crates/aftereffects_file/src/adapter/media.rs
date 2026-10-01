//! Local-only media preflight shared by check and write conversions.

mod collected;
mod psd;
mod video;

#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, BufReader, Read, Seek},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use fx_conv::{InspectedMedia, MediaKind, MediaRemediation, MediaStatus, ValidatedMediaMap};
use fx_schema::AssetId;
use sha2::{Digest, Sha256};
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::AepConversionError;
use crate::{
    alias::RelativeLocation,
    diagnostic::{ImportDiagnostic, Limitation},
    media::PhotoshopSource,
    structure::StructuralProject,
    structure_document::{MediaAssetKind, MediaAssetRequest, MediaResolution},
    vector_media::{self, Artwork},
};

enum ResolvedSource {
    Asset {
        path: PathBuf,
        dimensions: Option<[u32; 2]>,
        source: SourceFile,
    },
    Vector {
        artwork: Arc<Artwork>,
        source: SourceFile,
    },
}

/// The local file that preflight resolved for one request, and how it chose it.
#[derive(Debug)]
pub(super) struct SourceFile {
    /// The file that preflight read, or that the archive packages as it is.
    pub(super) path: PathBuf,
    /// Higher-priority authored/alias paths that must remain missing after relinking.
    pub(super) missing_paths: Vec<PathBuf>,
    /// The SHA-256, as lowercase hex, of the bytes that preflight decoded (a
    /// PSD or AI source); `None` for a file that is packaged as it is.
    pub(super) decoded_sha256: Option<String>,
}

/// One archive asset of a conversion: the preflighted file that packages it,
/// and the local source that it was resolved from.
pub(super) struct UsedAsset<'p> {
    pub(super) request: &'p MediaAssetRequest,
    pub(super) path: &'p Path,
    pub(super) kind: AssetKind,
    pub(super) source: &'p SourceFile,
}

#[derive(Clone, Default)]
struct Selection {
    authored: Option<PathBuf>,
    original: Option<PathBuf>,
    selected: Option<PathBuf>,
    codec: Option<String>,
    unavailable_reason: Option<String>,
}

pub(super) struct MediaPreflight<'a> {
    input: &'a Path,
    base: &'a Path,
    media_map: Option<&'a ValidatedMediaMap>,
    collected: collected::CollectedPaths,
    resolved: HashMap<AssetId, (MediaAssetRequest, ResolvedSource)>,
    missing: HashMap<AssetId, MediaAssetRequest>,
    selections: HashMap<AssetId, Selection>,
    // Keep normalized bytes alive until the builder has finished publication.
    normalized: Vec<tempfile::NamedTempFile>,
    pub(super) diagnostics: Vec<ImportDiagnostic>,
    pub(super) failure: Option<AepConversionError>,
}

impl<'a> MediaPreflight<'a> {
    #[cfg(test)]
    pub(super) fn new(input: &'a Path) -> Self {
        Self::with_media_map(input, None)
    }

    pub(super) fn with_media_map(
        input: &'a Path,
        media_map: Option<&'a ValidatedMediaMap>,
    ) -> Self {
        Self {
            input,
            base: input.parent().unwrap_or(Path::new(".")),
            media_map,
            collected: collected::CollectedPaths::default(),
            resolved: HashMap::new(),
            missing: HashMap::new(),
            selections: HashMap::new(),
            normalized: Vec::new(),
            diagnostics: Vec::new(),
            failure: None,
        }
    }

    /// Precompute exact Adobe-collected paths from native project-panel ancestry.
    pub(super) fn with_project(
        input: &'a Path,
        media_map: Option<&'a ValidatedMediaMap>,
        project: &StructuralProject,
    ) -> Self {
        Self {
            collected: collected::CollectedPaths::new(project),
            ..Self::with_media_map(input, media_map)
        }
    }

    pub(super) fn require(
        &mut self,
        request: &MediaAssetRequest,
    ) -> Result<(), AepConversionError> {
        let resolution = self.resolve(request)?;
        if matches!(resolution, MediaResolution::Unavailable) {
            return Ok(());
        }
        Ok(())
    }

    pub(super) fn inspect(
        &mut self,
        request: &MediaAssetRequest,
        id: String,
        name: String,
        references: Vec<String>,
    ) -> InspectedMedia {
        self.failure = None;
        let resolution = self.resolve(request);
        let failure = resolution.as_ref().err();
        let mut selection = self
            .selections
            .get(&request.logical_id)
            .cloned()
            .unwrap_or_default();
        let (status, reason, remediation) = match failure {
            None if matches!(resolution, Ok(MediaResolution::Unavailable)) => (
                MediaStatus::Missing,
                selection.unavailable_reason.clone(),
                MediaRemediation::Unknown,
            ),
            None
                if request.kind == MediaAssetKind::Audio
                    && !selection.selected.as_deref().is_some_and(|path| {
                        path.extension().and_then(|extension| extension.to_str()).is_some_and(
                            |extension| matches!(extension.to_ascii_lowercase().as_str(),
                                "mp4" | "m4a" | "mp3" | "wav" | "flac" | "mov"),
                        )
                    }) => (
                MediaStatus::RequiresTranscode,
                Some("standalone audio filename is not supported by playback; explicitly prepare compatible WAV media".into()),
                MediaRemediation::TranscodeCandidate,
            ),
            None => (MediaStatus::Supported, None, MediaRemediation::None),
            Some(AepConversionError::VideoMedia { reason, .. }) => {
                if let Some(codec) = reason
                    .strip_prefix("unsupported video codec \"")
                    .and_then(|value| value.split('"').next())
                {
                    selection.codec = Some(codec.to_owned());
                }
                let (status, remediation) = if reason.starts_with("cannot classify SWF:") {
                    (MediaStatus::InvalidMedia, MediaRemediation::Unknown)
                } else if reason.starts_with("SWF external render required:") {
                    (
                        MediaStatus::RequiresTranscode,
                        MediaRemediation::ExternalRenderRequired,
                    )
                } else if reason.starts_with("SWF unassessed:") {
                    (MediaStatus::Unassessed, MediaRemediation::Unknown)
                } else if reason.starts_with("cannot read video container")
                    || reason == "container has no video track"
                {
                    (MediaStatus::InvalidMedia, MediaRemediation::Unknown)
                } else {
                    (
                        MediaStatus::RequiresTranscode,
                        MediaRemediation::TranscodeCandidate,
                    )
                };
                (status, Some(reason.clone()), remediation)
            }
            Some(AepConversionError::Io { source, .. }) => (
                if is_missing(source) {
                    MediaStatus::Missing
                } else {
                    MediaStatus::Unreadable
                },
                Some(source.to_string()),
                MediaRemediation::Unknown,
            ),
            Some(error) => (
                MediaStatus::Unassessed,
                Some(error.to_string()),
                MediaRemediation::Unknown,
            ),
        };
        let container = selection
            .selected
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        InspectedMedia {
            owner: self.input.to_owned(),
            id,
            name,
            kind: match request.kind {
                MediaAssetKind::Video => MediaKind::Video,
                MediaAssetKind::Audio => MediaKind::Audio,
                MediaAssetKind::Image | MediaAssetKind::SequenceImage => {
                    unreachable!("inspection inventories only time-based media")
                }
            },
            authored: selection.authored,
            original: selection.original,
            selected: selection.selected,
            references,
            status,
            container,
            codec: selection.codec,
            reason,
            remediation,
        }
    }

    #[cfg(test)]
    pub(super) fn available(&mut self, request: &MediaAssetRequest) -> bool {
        matches!(
            self.resolve_media(request),
            MediaResolution::Asset | MediaResolution::AssetDimensions(_)
        )
    }

    pub(super) fn resolve_media(&mut self, request: &MediaAssetRequest) -> MediaResolution {
        if self.failure.is_some() {
            return MediaResolution::Unavailable;
        }
        match self.resolve(request) {
            Ok(resolution) => resolution,
            Err(error) => {
                self.failure = Some(error);
                MediaResolution::Unavailable
            }
        }
    }

    fn resolve(
        &mut self,
        request: &MediaAssetRequest,
    ) -> Result<MediaResolution, AepConversionError> {
        if let Some((existing, source)) = self.resolved.get(&request.logical_id) {
            if existing != request {
                return Err(AepConversionError::Input(
                    "conflicting local media identity",
                ));
            }
            return Ok(match source {
                ResolvedSource::Asset {
                    dimensions: Some(dimensions),
                    ..
                } => MediaResolution::AssetDimensions(*dimensions),
                ResolvedSource::Asset {
                    dimensions: None, ..
                } => MediaResolution::Asset,
                ResolvedSource::Vector { artwork, .. } => MediaResolution::Vector(artwork.clone()),
            });
        }
        if let Some(existing) = self.missing.get(&request.logical_id) {
            if existing != request {
                return Err(AepConversionError::Input(
                    "conflicting local media identity",
                ));
            }
            return Ok(MediaResolution::Unavailable);
        }
        let spelling = &request.authored_path;
        let windows_drive = spelling.as_bytes().get(1) == Some(&b':')
            && spelling.as_bytes()[0].is_ascii_alphabetic();
        if spelling.is_empty()
            || spelling.contains('\0')
            || spelling.contains("://")
            || spelling.starts_with("\\\\")
            || spelling.starts_with("//")
            || (windows_drive && !cfg!(windows))
        {
            self.omit(request, "not a native local-file path; no URL, network-share or platform-path guessing was attempted");
            return Ok(MediaResolution::Unavailable);
        }
        let authored = Path::new(spelling);
        self.selections.insert(
            request.logical_id.clone(),
            Selection {
                authored: Some(authored.to_owned()),
                ..Selection::default()
            },
        );
        let mut path = if authored.is_absolute() {
            authored.to_owned()
        } else {
            self.base.join(authored)
        };
        let mut metadata = fs::metadata(&path);
        let mut missing = "local source is missing; media content omitted".to_owned();
        let mut missing_paths = Vec::new();
        // An existing authored file always wins. AE relinks a moved project
        // from its alias hint only when that absolute file is missing.
        if authored.is_absolute()
            && metadata.as_ref().is_err_and(is_missing)
            && let Some(location) = request.relative_location
        {
            match self.relocated(authored, location) {
                Ok(candidate) => {
                    missing = format!(
                        "local source is missing at its authored path and at its native relative location {candidate:?}; media content omitted"
                    );
                    metadata = fs::metadata(&candidate);
                    missing_paths.push(path);
                    path = candidate;
                }
                Err(reason) => {
                    missing = format!("local source is missing; {reason}; media content omitted");
                }
            }
        }
        if metadata.as_ref().is_err_and(is_missing)
            && let Some(collected) = self.collected.candidate(request)
        {
            match collected {
                Ok(relative) => {
                    let candidate = self.base.join(relative);
                    match fs::canonicalize(&candidate) {
                        Ok(canonical) => {
                            let base = fs::canonicalize(self.base).map_err(|error| {
                                AepConversionError::io("resolve AEP directory", self.base, error)
                            })?;
                            let root = base.join("(Footage)");
                            let actual_root = fs::canonicalize(&root).map_err(|error| {
                                AepConversionError::io("resolve collected directory", &root, error)
                            })?;
                            if actual_root != root || !canonical.starts_with(&root) {
                                missing = format!(
                                    "collected source {candidate:?} escapes its adjacent directory; media content omitted"
                                );
                            } else {
                                missing_paths.push(path);
                                path = candidate;
                                metadata = fs::metadata(&path);
                                missing = format!(
                                    "collected source missing at {path:?}; media content omitted"
                                );
                                if metadata.as_ref().is_ok_and(|metadata| metadata.is_file()) {
                                    self.diagnostics.push(ImportDiagnostic {
                                        limitation: Limitation::Placeholder,
                                        composition_id: None,
                                        layer_id: None,
                                        message: format!("media {} at {:?}: resolved at adjacent collected path {path:?} from native project folders; file identity and rendering against Adobe are unverified", request.logical_id, request.authored_path),
                                    });
                                }
                            }
                        }
                        Err(error) if is_missing(&error) => {
                            missing = format!(
                                "{missing}; collected source also missing at {candidate:?}"
                            );
                        }
                        Err(error) => {
                            return Err(AepConversionError::io(
                                "resolve collected media",
                                &candidate,
                                error,
                            ));
                        }
                    }
                }
                Err(reason) => missing = format!("{missing}; collected source rejected: {reason}"),
            }
        }
        let metadata = match metadata {
            Ok(metadata) => metadata,
            Err(error) if is_missing(&error) => {
                self.omit(request, &missing);
                return Ok(MediaResolution::Unavailable);
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidFilename => {
                self.omit(
                    request,
                    "local source filename is invalid on this host; media content omitted",
                );
                return Ok(MediaResolution::Unavailable);
            }
            Err(error) => return Err(AepConversionError::io("inspect media", &path, error)),
        };
        if !metadata.is_file() {
            self.omit(
                request,
                "source is not a regular file; media content omitted",
            );
            return Ok(MediaResolution::Unavailable);
        }
        if let Some(selection) = self.selections.get_mut(&request.logical_id) {
            selection.original = Some(path.clone());
        }
        if let Some(media_map) = self.media_map
            && let Some(replacement) = media_map.replacement_for(&path)?
        {
            path = replacement.to_owned();
        }
        if let Some(selection) = self.selections.get_mut(&request.logical_id) {
            selection.selected = Some(path.clone());
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| AepConversionError::io("inspect selected media", &path, error))?;
        if !metadata.is_file() {
            return Err(AepConversionError::Input(
                "selected media is not a regular file",
            ));
        }
        // Check performs the same readability preflight as Write. Do not turn
        // permission or other operational failures into best-effort omissions.
        let mut file = File::open(&path)
            .map_err(|error| AepConversionError::io("open media", &path, error))?;
        if !file
            .metadata()
            .map_err(|error| AepConversionError::io("inspect open media", &path, error))?
            .is_file()
        {
            return Err(AepConversionError::Input(
                "media changed to a non-file during preflight",
            ));
        }
        if request.kind == MediaAssetKind::Video {
            let codec = video::validate(&mut file, &path, &request.logical_id)?;
            if let Some(selection) = self.selections.get_mut(&request.logical_id) {
                selection.codec = Some(codec);
            }
        }
        if request.kind == MediaAssetKind::SequenceImage {
            let reader = image::ImageReader::new(BufReader::new(file))
                .with_guessed_format()
                .map_err(|error| AepConversionError::io("identify sequence image", &path, error))?;
            if reader.format() != Some(image::ImageFormat::Png) {
                self.omit(
                    request,
                    "sequence frame is not a PNG image; frame content omitted",
                );
                return Ok(MediaResolution::Unavailable);
            }
            let dimensions = match reader.into_dimensions() {
                Ok((width, height)) if width > 0 && height > 0 => [width, height],
                Ok(_) => {
                    self.omit(
                        request,
                        "sequence frame has zero dimensions; frame content omitted",
                    );
                    return Ok(MediaResolution::Unavailable);
                }
                Err(error) => {
                    self.omit(
                        request,
                        &format!(
                            "sequence frame PNG dimensions cannot be decoded: {error}; frame content omitted"
                        ),
                    );
                    return Ok(MediaResolution::Unavailable);
                }
            };
            if request.dimensions.contains(&0) {
                self.omit(
                    request,
                    "sequence frame has no positive native fixed footage canvas; frame content omitted",
                );
                return Ok(MediaResolution::Unavailable);
            }
            if dimensions != request.dimensions {
                self.omit(
                    request,
                    &format!(
                        "sequence frame is {}x{} but the native fixed footage canvas is {}x{}; mixed-size placement/scaling is unverified, so no guessed normalization was emitted",
                        dimensions[0], dimensions[1], request.dimensions[0], request.dimensions[1]
                    ),
                );
                return Ok(MediaResolution::Unavailable);
            }
            let source = SourceFile {
                path: path.clone(),
                missing_paths,
                decoded_sha256: None,
            };
            self.resolved.insert(
                request.logical_id.clone(),
                (
                    request.clone(),
                    ResolvedSource::Asset {
                        path,
                        dimensions: Some(dimensions),
                        source,
                    },
                ),
            );
            return Ok(MediaResolution::AssetDimensions(dimensions));
        }
        if request.kind == MediaAssetKind::Image {
            let mut signature = [0; 4];
            let identified = match file.read_exact(&mut signature) {
                Ok(()) => true,
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => false,
                Err(error) => return Err(AepConversionError::io("identify image", &path, error)),
            };
            file.rewind()
                .map_err(|error| AepConversionError::io("rewind image", &path, error))?;
            let psd_path = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("psd"));
            if request.photoshop_source.is_some()
                || psd_path
                || (identified && signature == *b"8BPS")
            {
                return self
                    .normalize_psd(request, &path, missing_paths, file)
                    .map(|available| {
                        if available {
                            MediaResolution::Asset
                        } else {
                            MediaResolution::Unavailable
                        }
                    });
            }
        }
        let is_ai = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ai"));
        if is_ai {
            let mut signature = [0; 5];
            match file.read_exact(&mut signature) {
                Ok(()) if signature == *b"%PDF-" => {}
                Ok(()) => {
                    self.omit(
                        request,
                        "AI source is not PDF-compatible; no PNG, raw AI asset or guessed selector fallback emitted",
                    );
                    return Ok(MediaResolution::Unavailable);
                }
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                    self.omit(
                        request,
                        "AI source is not PDF-compatible; no PNG, raw AI asset or guessed selector fallback emitted",
                    );
                    return Ok(MediaResolution::Unavailable);
                }
                Err(error) => {
                    return Err(AepConversionError::io(
                        "identify PDF-compatible AI media",
                        &path,
                        error,
                    ));
                }
            }
            file.rewind().map_err(|error| {
                AepConversionError::io("rewind PDF-compatible AI media", &path, error)
            })?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(|error| {
                AepConversionError::io("read PDF-compatible AI media", &path, error)
            })?;
            let artwork = match vector_media::decode(&bytes) {
                Ok(artwork) => Arc::new(artwork),
                Err(vector_media::VectorMediaError::Interrupted(reason)) => {
                    return Err(AepConversionError::Input(reason));
                }
                Err(error) => {
                    self.omit(
                        request,
                        &format!(
                            "PDF-compatible AI could not be lowered to editable Shapes: {error}; no PNG, raw AI asset or guessed selector fallback emitted"
                        ),
                    );
                    return Ok(MediaResolution::Unavailable);
                }
            };
            let source = SourceFile {
                path,
                missing_paths,
                decoded_sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
            };
            self.resolved.insert(
                request.logical_id.clone(),
                (
                    request.clone(),
                    ResolvedSource::Vector {
                        artwork: artwork.clone(),
                        source,
                    },
                ),
            );
            Ok(MediaResolution::Vector(artwork))
        } else {
            let source = SourceFile {
                path: path.clone(),
                missing_paths,
                decoded_sha256: None,
            };
            self.resolved.insert(
                request.logical_id.clone(),
                (
                    request.clone(),
                    ResolvedSource::Asset {
                        path,
                        dimensions: None,
                        source,
                    },
                ),
            );
            Ok(MediaResolution::Asset)
        }
    }

    /// Where AE finds `authored` in this moved project: `location`'s ancestor
    /// of the AEP joined with the authored path's last components. Or why the
    /// hint names no such path; nothing else is searched.
    fn relocated(&self, authored: &Path, location: RelativeLocation) -> Result<PathBuf, String> {
        let unfit = || {
            format!(
                "its native relative location (ascend {}, {} trailing components) does not fit this AEP's location",
                location.ascend(),
                location.components()
            )
        };
        let directory = fs::canonicalize(self.base).map_err(|error| {
            format!(
                "the AEP directory cannot be resolved for its native relative location: {error}"
            )
        })?;
        let levels = usize::try_from(location.ascend() - 1).map_err(|_| unfit())?;
        let ancestor = directory.ancestors().nth(levels).ok_or_else(unfit)?;
        let components: Vec<_> = authored.components().collect();
        let count = usize::try_from(location.components()).map_err(|_| unfit())?;
        let tail = components
            .len()
            .checked_sub(count)
            .map(|start| &components[start..])
            .filter(|tail| tail.iter().all(|part| matches!(part, Component::Normal(_))))
            .ok_or_else(unfit)?;
        Ok(ancestor.join(tail.iter().collect::<PathBuf>()))
    }

    fn normalize_psd(
        &mut self,
        request: &MediaAssetRequest,
        path: &Path,
        missing_paths: Vec<PathBuf>,
        mut file: File,
    ) -> Result<bool, AepConversionError> {
        let Some(source) = request.photoshop_source else {
            self.omit(request, "PSD has no validated native Photoshop selector; refusing a whole-image substitution");
            return Ok(false);
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| AepConversionError::io("read PSD", path, error))?;
        let selection = match source {
            PhotoshopSource::Merged => psd::Selection::Merged,
            PhotoshopSource::Layer { id, index } => psd::Selection::Layer { id, index },
        };
        let image = match psd::decode(&bytes, selection, request.dimensions) {
            Ok(image) => image,
            Err(error) => {
                self.omit(request, &format!("PSD {source:?} cannot be normalized: {error}; no whole-image substitution emitted"));
                return Ok(false);
            }
        };
        let psd = SourceFile {
            path: path.to_owned(),
            missing_paths,
            decoded_sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
        };
        let mut png = tempfile::Builder::new()
            .prefix("aep-psd-")
            .suffix(".png")
            .tempfile()
            .map_err(|error| AepConversionError::io("create normalized PSD", path, error))?;
        image::DynamicImage::ImageRgba8(image)
            .write_to(png.as_file_mut(), image::ImageFormat::Png)?;
        self.resolved.insert(
            request.logical_id.clone(),
            (
                request.clone(),
                ResolvedSource::Asset {
                    path: png.path().to_owned(),
                    dimensions: Some(request.dimensions),
                    source: psd,
                },
            ),
        );
        self.normalized.push(png);
        self.diagnostics.push(ImportDiagnostic {
            limitation: Limitation::Properties,
            composition_id: None,
            layer_id: None,
            message: format!("media {} at {:?}: PSD {source:?} rasterized to PNG; AE layer controls remain editable, Photoshop internals do not; embedded color profiles are not converted and Adobe alpha/color fidelity is unverified", request.logical_id, request.authored_path),
        });
        Ok(true)
    }

    fn omit(&mut self, request: &MediaAssetRequest, reason: &str) {
        self.selections
            .entry(request.logical_id.clone())
            .or_default()
            .unavailable_reason = Some(reason.to_owned());
        self.missing
            .insert(request.logical_id.clone(), request.clone());
        self.diagnostics.push(ImportDiagnostic {
            limitation: Limitation::Placeholder,
            composition_id: None,
            layer_id: None,
            message: format!(
                "media {} at {:?}: {reason}",
                request.logical_id, request.authored_path
            ),
        });
    }

    pub(super) fn add_used_assets(
        &self,
        mut builder: TesseractFileBuilder,
        requests: &[MediaAssetRequest],
    ) -> Result<TesseractFileBuilder, AepConversionError> {
        for asset in self.used_assets(requests)? {
            builder =
                builder.add_asset(asset.request.logical_id.as_str(), asset.path, asset.kind)?;
        }
        Ok(builder)
    }

    /// Each archive asset that `requests` reference, once, with the
    /// preflighted file that packages it.
    pub(super) fn used_assets(
        &self,
        requests: &[MediaAssetRequest],
    ) -> Result<Vec<UsedAsset<'_>>, AepConversionError> {
        let mut added = HashSet::new();
        let mut assets = Vec::new();
        for request in requests {
            if !added.insert(&request.logical_id) {
                continue;
            }
            let Some((original, ResolvedSource::Asset { path, source, .. })) =
                self.resolved.get(&request.logical_id)
            else {
                return Err(AepConversionError::Input(
                    "media reference was not preflighted as an archive asset",
                ));
            };
            if original != request {
                return Err(AepConversionError::Input(
                    "media request changed after preflight",
                ));
            }
            let kind = match request.kind {
                MediaAssetKind::Image | MediaAssetKind::SequenceImage => AssetKind::Image,
                MediaAssetKind::Video => AssetKind::Video,
                MediaAssetKind::Audio => AssetKind::Audio,
            };
            assets.push(UsedAsset {
                request: original,
                path,
                kind,
                source,
            });
        }
        Ok(assets)
    }

    /// The local sources of the artwork that preflight lowered to vectors.
    pub(super) fn vector_sources(&self) -> impl Iterator<Item = &SourceFile> {
        self.resolved
            .values()
            .filter_map(|(_, resolved)| match resolved {
                ResolvedSource::Vector { source, .. } => Some(source),
                ResolvedSource::Asset { .. } => None,
            })
    }

    /// The normalized files that back resolved assets; they must outlive the
    /// archive written from them.
    pub(super) fn into_normalized(self) -> Vec<tempfile::NamedTempFile> {
        self.normalized
    }
}

/// The SHA-256, as lowercase hex, of the file at `path`: the digest that the
/// archive writer records for a packaged copy of it.
pub(super) fn sha256_file(path: &Path) -> Result<String, AepConversionError> {
    let mut file =
        File::open(path).map_err(|error| AepConversionError::io("open media", path, error))?;
    let mut digest = Sha256::new();
    io::copy(&mut file, &mut digest)
        .map_err(|error| AepConversionError::io("hash media", path, error))?;
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn is_missing(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}
