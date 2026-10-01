//! Portable, editable `.tsrct` files.
//!
//! A file is a strict ZIP64-compatible container with root `metadata.json`,
//! root `project.json`, and Stored user-asset entries. The editable FX JSON is
//! the only authoritative project representation and is safe for direct edits.

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
mod cache;
#[cfg(target_arch = "wasm32")]
pub use browser::BrowserTesseractFile;
mod error;
mod legacy_timing;
mod metadata;
#[cfg(test)]
mod tests;
mod writer;

#[cfg(not(target_arch = "wasm32"))]
pub use cache::{MaterializationCache, MaterializedAsset};
pub use error::TesseractFileError;
pub use metadata::{
    AssetDescriptor, AssetKind, Generator, ProjectDescriptor, TesseractFileMetadata,
};

use crate::error::IoContext;
use crate::metadata::{
    invalid, validate_archive_path, validate_generator, FORMAT_NAME, FORMAT_VERSION, METADATA_PATH,
    METADATA_SCHEMA_URL, PROJECT_PATH,
};
use fx_schema::{asset_refs::AssetRef, EditableFxCompositionDocument, FontAssetProperties};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use zip::{CompressionMethod, ZipArchive};

/// The JSON Schema for root `metadata.json`.
pub const METADATA_JSON_SCHEMA: &str = include_str!("../schema/metadata.schema.json");

#[derive(Debug, Clone)]
struct EntryInfo {
    data_start: u64,
    size: u64,
    crc32: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceIdentity {
    byte_length: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingAsset {
    source: PathBuf,
    descriptor: AssetDescriptor,
    crc32: u32,
    integrity_ready: bool,
}

/// Strategy used to publish a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStrategy {
    /// Copy-on-write clone with changed entries and a replacement central directory.
    ReflinkAppend,
    /// Safe fallback that raw-copies Stored assets without decoding or recompressing them.
    RawCopyRewrite,
}

/// Observable work performed by a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveReport {
    /// Save strategy selected for the destination filesystem.
    pub strategy: SaveStrategy,
    /// Bytes emitted after the reflink boundary, or total fallback archive bytes.
    pub bytes_written: u64,
    /// Number of active asset entries retained without decoding or recompression.
    pub reused_assets: usize,
}

/// An opened and editable `.tsrct` file.
#[derive(Debug)]
pub struct TesseractFile {
    path: PathBuf,
    metadata: TesseractFileMetadata,
    project: EditableFxCompositionDocument,
    project_bytes: Vec<u8>,
    entries: HashMap<String, EntryInfo>,
    source_identity: SourceIdentity,
    writer_generator: Generator,
    pending_assets: BTreeMap<String, PendingAsset>,
}

impl TesseractFile {
    /// Opens and validates a `.tsrct` file without reading asset payloads.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TesseractFileError> {
        let path = path.as_ref().to_path_buf();
        let source_identity = source_identity(&path)?;
        let file = File::open(&path).at(&path)?;
        let mut archive = ZipArchive::new(file)?;

        let data_range_end = archive.central_directory_start();
        let mut names = HashSet::with_capacity(archive.len());
        let mut folded_names = HashSet::with_capacity(archive.len());
        let mut entries = HashMap::with_capacity(archive.len());
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            let name = std::str::from_utf8(entry.name_raw())
                .map_err(|_| invalid("archive entry names must be UTF-8"))?
                .to_string();
            validate_archive_path(&name)?;
            if !names.insert(name.clone()) || !folded_names.insert(name.to_ascii_lowercase()) {
                return Err(invalid(format!(
                    "duplicate or case-colliding entry {name:?}"
                )));
            }
            if entry.is_dir() || entry.is_symlink() {
                return Err(invalid(format!("entry {name:?} must be a regular file")));
            }
            if entry.encrypted() {
                return Err(invalid(format!("encrypted entry {name:?} is unsupported")));
            }
            if entry.compression() != CompressionMethod::Stored {
                return Err(invalid(format!(
                    "entry {name:?} must use ZIP Stored compression"
                )));
            }
            if entry.compressed_size() != entry.size() {
                return Err(invalid(format!("entry {name:?} ZIP Stored sizes differ")));
            }
            let data_end = entry
                .data_start()
                .checked_add(entry.size())
                .ok_or_else(|| invalid(format!("entry {name:?} data range overflows")))?;
            if data_end > data_range_end {
                return Err(invalid(format!(
                    "entry {name:?} is outside the physical ZIP data range"
                )));
            }
            entries.insert(
                name,
                EntryInfo {
                    data_start: entry.data_start(),
                    size: entry.size(),
                    crc32: entry.crc32(),
                },
            );
        }

        let metadata_bytes = read_document_entry(&mut archive, METADATA_PATH, None)?;
        let metadata: TesseractFileMetadata = serde_json::from_slice(&metadata_bytes)?;
        metadata.validate()?;
        let project_bytes = read_document_entry(
            &mut archive,
            PROJECT_PATH,
            Some(metadata.project.byte_length),
        )?;
        let project = parse_project(&metadata, &project_bytes, &entries)?;
        Ok(Self {
            path,
            metadata,
            project,
            project_bytes,
            entries,
            source_identity,
            writer_generator: current_generator(),
            pending_assets: BTreeMap::new(),
        })
    }

    /// Returns validated root metadata.
    pub fn metadata(&self) -> &TesseractFileMetadata {
        &self.metadata
    }

    /// Returns the authoritative editable FX project document.
    pub fn project(&self) -> &EditableFxCompositionDocument {
        &self.project
    }

    /// Returns the exact project JSON bytes currently staged for the next save.
    ///
    /// Direct file commits preserve their formatting and key order. Typed
    /// project replacement serializes the candidate to canonical JSON.
    pub fn project_json_bytes(&self) -> &[u8] {
        &self.project_bytes
    }

    /// Consumes a fully verified archive, transferring its original JSON bytes
    /// without retaining or copying the validated editable document.
    #[must_use]
    pub fn into_project_json_bytes(self) -> Vec<u8> {
        self.project_bytes
    }

    /// Returns a parsed JSON value for inspection.
    pub fn project_json(&self) -> Result<Value, TesseractFileError> {
        self.project
            .to_json_value()
            .map_err(TesseractFileError::from)
    }

    /// Checks out the exact `project.json` bytes to a normal editable file.
    ///
    /// The destination is atomically replaced; the `.tsrct` file is unchanged
    /// until [`Self::commit_project_json`] and [`Self::save`] succeed.
    pub fn checkout_project_json(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<(), TesseractFileError> {
        atomic_write_file(destination.as_ref(), &self.project_bytes)
    }

    /// Commits an agent-edited JSON file when every referenced asset is packaged.
    pub fn commit_project_json(
        &mut self,
        source: impl AsRef<Path>,
    ) -> Result<(), TesseractFileError> {
        self.commit_project_json_with_runtime_assets(source, |_| false)
    }

    /// Commits an agent-edited JSON file while admitting runtime-owned assets.
    ///
    /// Validation is transactional. On success, the source file's exact bytes
    /// become the next `project.json`, preserving formatting and key order.
    pub fn commit_project_json_with_runtime_assets(
        &mut self,
        source: impl AsRef<Path>,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<(), TesseractFileError> {
        let project_bytes = read_project_file(source.as_ref())?;
        let candidate = EditableFxCompositionDocument::from_json_slice(&project_bytes)?;
        validate_project_asset_refs(&candidate, &self.metadata, is_runtime_owned)?;
        self.project = candidate;
        self.project_bytes = project_bytes;
        Ok(())
    }

    /// Returns an asset view resolved by exact project asset ID.
    pub fn asset(&self, asset_id: &str) -> Result<TesseractAsset<'_>, TesseractFileError> {
        let descriptor = self
            .metadata
            .assets
            .get(asset_id)
            .ok_or_else(|| invalid(format!("asset ID {asset_id:?} is not in metadata.json")))?;
        let entry = self
            .entries
            .get(&descriptor.path)
            .ok_or_else(|| invalid(format!("asset entry {:?} is unavailable", descriptor.path)))?;
        Ok(TesseractAsset {
            file: self,
            descriptor,
            entry,
        })
    }

    /// Replaces the project with a validated typed candidate.
    ///
    /// The candidate is serialized to canonical JSON. Validation is
    /// transactional: an invalid candidate leaves the staged project unchanged.
    pub fn replace_project(
        &mut self,
        candidate: EditableFxCompositionDocument,
    ) -> Result<(), TesseractFileError> {
        self.replace_project_with_runtime_assets(candidate, |_| false)
    }

    /// Replaces the project while admitting runtime-owned asset references.
    ///
    /// The candidate is serialized to canonical JSON. Validation is
    /// transactional: an invalid candidate leaves the staged project unchanged.
    pub fn replace_project_with_runtime_assets(
        &mut self,
        candidate: EditableFxCompositionDocument,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<(), TesseractFileError> {
        let project_bytes = candidate.to_json_vec()?;
        let candidate = EditableFxCompositionDocument::from_json_slice(&project_bytes)?;
        validate_project_asset_refs(&candidate, &self.metadata, is_runtime_owned)?;
        self.project = candidate;
        self.project_bytes = project_bytes;
        Ok(())
    }

    /// Sets the writer identity recorded by the next successful save.
    pub fn set_generator(&mut self, generator: Generator) -> Result<(), TesseractFileError> {
        let mut candidate = self.metadata.clone();
        candidate.generator = generator.clone();
        candidate.validate()?;
        self.writer_generator = generator;
        Ok(())
    }

    /// Adds or replaces one user-provided asset mapping.
    pub fn add_asset(
        &mut self,
        asset_id: impl Into<String>,
        source: impl AsRef<Path>,
        kind: AssetKind,
    ) -> Result<(), TesseractFileError> {
        let asset_id = asset_id.into();
        let pending = prepare_asset(&asset_id, source.as_ref(), kind, true)?;
        let mut candidate = self.metadata.clone();
        candidate
            .assets
            .insert(asset_id.clone(), pending.descriptor.clone());
        candidate.validate()?;
        self.metadata = candidate;
        self.pending_assets.insert(asset_id, pending);
        Ok(())
    }

    /// Persists inspected authoring metadata for an embedded font asset.
    ///
    /// The opaque `asset_id` selects bytes through `assets`; semantic family,
    /// style, face, and variation data remain in the separate font registry.
    pub fn set_font_properties(
        &mut self,
        asset_id: &str,
        properties: FontAssetProperties,
    ) -> Result<(), TesseractFileError> {
        let mut candidate = self.metadata.clone();
        candidate.fonts.insert(asset_id.to_owned(), properties);
        candidate.validate()?;
        self.metadata = candidate;
        Ok(())
    }

    /// Removes an asset mapping. This does not rewrite project references.
    pub fn remove_asset(&mut self, asset_id: &str) -> Result<(), TesseractFileError> {
        if self.metadata.assets.remove(asset_id).is_none() {
            return Err(invalid(format!(
                "asset ID {asset_id:?} is not in metadata.json"
            )));
        }
        self.pending_assets.remove(asset_id);
        self.metadata.fonts.remove(asset_id);
        Ok(())
    }

    /// Atomically saves over the opened path when every asset is packaged.
    pub fn save(&mut self) -> Result<SaveReport, TesseractFileError> {
        self.save_with_runtime_assets(|_| false)
    }

    /// Saves while explicitly admitting runtime-owned assets.
    pub fn save_with_runtime_assets(
        &mut self,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<SaveReport, TesseractFileError> {
        let path = self.path.clone();
        self.save_to(&path, is_runtime_owned)
    }

    /// Rewrites only active entries, removing unreachable records from prior saves.
    pub fn optimize(&mut self) -> Result<SaveReport, TesseractFileError> {
        self.optimize_with_runtime_assets(|_| false)
    }

    /// Optimizes while explicitly admitting runtime-owned assets.
    pub fn optimize_with_runtime_assets(
        &mut self,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<SaveReport, TesseractFileError> {
        self.ensure_source_unchanged()?;
        let project_bytes = self.prepare_save_metadata(is_runtime_owned)?;
        let report = writer::save_compacted(
            &self.path,
            &self.path,
            &self.metadata,
            &project_bytes,
            &self.pending_assets,
        )?;
        *self = Self::open(&self.path)?;
        Ok(report)
    }

    /// Atomically saves to another path when every asset is packaged.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<SaveReport, TesseractFileError> {
        self.save_as_with_runtime_assets(path, |_| false)
    }

    /// Saves to another path while explicitly admitting runtime assets.
    pub fn save_as_with_runtime_assets(
        &mut self,
        path: impl AsRef<Path>,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<SaveReport, TesseractFileError> {
        self.save_to(path.as_ref(), is_runtime_owned)
    }

    fn save_to(
        &mut self,
        destination: &Path,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<SaveReport, TesseractFileError> {
        self.ensure_source_unchanged()?;
        let project_bytes = self.prepare_save_metadata(is_runtime_owned)?;
        let report = writer::save(
            &self.path,
            destination,
            &self.metadata,
            &project_bytes,
            &self.pending_assets,
        )?;
        *self = Self::open(destination)?;
        Ok(report)
    }

    fn prepare_save_metadata(
        &mut self,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<Vec<u8>, TesseractFileError> {
        validate_project_asset_refs(&self.project, &self.metadata, is_runtime_owned)?;
        let project_bytes = self.project_bytes.clone();
        self.metadata.project.byte_length = project_bytes.len() as u64;
        self.metadata.project.sha256 = sha256_bytes(&project_bytes);
        self.metadata.modified_at = now_rfc3339()?;
        self.metadata.generator = self.writer_generator.clone();
        self.metadata.fx_schema_version = Some(fx_schema::FX_SCHEMA_REVISION);
        self.metadata.validate()?;
        Ok(project_bytes)
    }

    fn ensure_source_unchanged(&self) -> Result<(), TesseractFileError> {
        if source_identity(&self.path)? != self.source_identity {
            return Err(invalid(format!(
                "source .tsrct file changed after it was opened: {}",
                self.path.display()
            )));
        }
        Ok(())
    }
}

/// A lazily opened user asset in a `.tsrct` file.
#[derive(Debug)]
pub struct TesseractAsset<'file> {
    file: &'file TesseractFile,
    descriptor: &'file AssetDescriptor,
    entry: &'file EntryInfo,
}

impl TesseractAsset<'_> {
    /// Returns the validated metadata descriptor.
    pub fn descriptor(&self) -> &AssetDescriptor {
        self.descriptor
    }

    /// Opens a bounded seekable reader over this Stored ZIP entry.
    pub fn open(&self) -> Result<AssetReader, TesseractFileError> {
        let file = File::open(&self.file.path).at(&self.file.path)?;
        Ok(AssetReader {
            file,
            archive_path: self.file.path.clone(),
            start: self.entry.data_start,
            length: self.entry.size,
            position: 0,
        })
    }

    /// Reads and verifies this asset into memory under a caller-selected bound.
    pub fn read_verified_bytes(&self, max_byte_length: u64) -> Result<Vec<u8>, TesseractFileError> {
        if self.descriptor.byte_length > max_byte_length {
            return Err(invalid(format!(
                "asset {:?} exceeds the {} byte read limit",
                self.descriptor.path, max_byte_length
            )));
        }
        let mut bytes = Vec::with_capacity(self.descriptor.byte_length as usize);
        self.open()?.read_to_end(&mut bytes).at(&self.file.path)?;
        verify_descriptor(
            &self.descriptor.path,
            &bytes,
            self.descriptor.byte_length,
            &self.descriptor.sha256,
        )?;
        Ok(bytes)
    }

    /// Materializes and verifies this asset for a path-only consumer.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn materialize(
        &self,
        cache: &MaterializationCache,
    ) -> Result<MaterializedAsset, TesseractFileError> {
        cache.materialize(self)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn crc32(&self) -> u32 {
        self.entry.crc32
    }
}

/// A bounded reader and seeker over one Stored asset entry.
#[derive(Debug)]
pub struct AssetReader {
    file: File,
    archive_path: PathBuf,
    start: u64,
    length: u64,
    position: u64,
}

impl Read for AssetReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.length.saturating_sub(self.position);
        if remaining == 0 || buffer.is_empty() {
            return Ok(0);
        }
        let length = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        self.file
            .seek(SeekFrom::Start(self.start + self.position))?;
        let read = self.file.read(&mut buffer[..length])?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for AssetReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let target = match position {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.length) + i128::from(offset),
        };
        if !(0..=i128::from(self.length)).contains(&target) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "seek outside asset entry in {}",
                    self.archive_path.display()
                ),
            ));
        }
        self.position = target as u64;
        Ok(self.position)
    }
}

/// Streaming builder for a new `.tsrct` file.
#[derive(Debug)]
pub struct TesseractFileBuilder {
    project: EditableFxCompositionDocument,
    project_bytes: Option<Vec<u8>>,
    document_id: String,
    generator: Generator,
    assets: BTreeMap<String, PendingAsset>,
    fonts: BTreeMap<String, FontAssetProperties>,
}

impl TesseractFileBuilder {
    /// Starts a new file from a canonical editable FX project document.
    pub fn new(project: EditableFxCompositionDocument) -> Self {
        Self::from_document(project)
    }

    /// Validates typed document serialization without writing files.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn try_new(project: EditableFxCompositionDocument) -> Result<Self, TesseractFileError> {
        let bytes = project.to_json_vec()?;
        let mut builder = Self::from_document(project);
        builder.project_bytes = Some(bytes);
        Ok(builder)
    }

    /// Starts a new file from validated editable FX JSON bytes.
    pub fn from_project_json(bytes: &[u8]) -> Result<Self, TesseractFileError> {
        let project = EditableFxCompositionDocument::from_json_slice(bytes)?;
        let mut builder = Self::from_document(project);
        builder.project_bytes = Some(bytes.to_vec());
        Ok(builder)
    }

    /// Sets the writer identity stored in `metadata.json`.
    pub fn generator(mut self, generator: Generator) -> Result<Self, TesseractFileError> {
        validate_generator(&generator)?;
        self.generator = generator;
        Ok(self)
    }

    /// Adds one user-provided asset.
    pub fn add_asset(
        mut self,
        asset_id: impl Into<String>,
        source: impl AsRef<Path>,
        kind: AssetKind,
    ) -> Result<Self, TesseractFileError> {
        let asset_id = asset_id.into();
        if self.assets.contains_key(&asset_id) {
            return Err(invalid(format!("duplicate asset ID {asset_id:?}")));
        }
        self.assets.insert(
            asset_id.clone(),
            prepare_asset(&asset_id, source.as_ref(), kind, false)?,
        );
        Ok(self)
    }

    /// Adds one font asset together with its persisted authoring metadata.
    pub fn add_font_asset(
        mut self,
        asset_id: impl Into<String>,
        source: impl AsRef<Path>,
        properties: FontAssetProperties,
    ) -> Result<Self, TesseractFileError> {
        let asset_id = asset_id.into();
        if self.assets.contains_key(&asset_id) {
            return Err(invalid(format!("duplicate asset ID {asset_id:?}")));
        }
        self.assets.insert(
            asset_id.clone(),
            prepare_asset(&asset_id, source.as_ref(), AssetKind::Font, false)?,
        );
        self.fonts.insert(asset_id, properties);
        Ok(self)
    }

    /// Returns the serialized project size without writing a file.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn project_byte_len(&self) -> Result<u64, TesseractFileError> {
        Ok(match &self.project_bytes {
            Some(bytes) => bytes.len() as u64,
            None => self.project.to_json_vec()?.len() as u64,
        })
    }

    /// Validates that every referenced asset is packaged without writing a file.
    ///
    /// # Errors
    /// Returns an error if the project references an asset that was not added.
    pub fn validate(&self) -> Result<(), TesseractFileError> {
        validate_project_asset_ids(
            &self.project,
            |asset_id| self.assets.contains_key(asset_id),
            |_| false,
        )
    }

    /// Atomically writes a new file when every referenced asset is packaged.
    pub fn write(self, path: impl AsRef<Path>) -> Result<TesseractFile, TesseractFileError> {
        self.write_with_runtime_assets(path, |_| false)
    }

    /// Writes a new file while explicitly admitting runtime-owned assets.
    pub fn write_with_runtime_assets(
        self,
        path: impl AsRef<Path>,
        is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
    ) -> Result<TesseractFile, TesseractFileError> {
        let project_bytes = match self.project_bytes {
            Some(bytes) => bytes,
            None => self.project.to_json_vec()?,
        };
        let writer_generator = self.generator.clone();
        let metadata = archive_metadata(
            &project_bytes,
            self.document_id,
            self.generator,
            &self.assets,
            self.fonts,
        )?;
        validate_project_asset_ids(
            &self.project,
            |asset_id| self.assets.contains_key(asset_id),
            is_runtime_owned,
        )?;
        // The written archive is reopened below. Release the validated document
        // before that read would otherwise keep two full FX trees alive.
        drop(self.project);
        writer::write_new(path.as_ref(), metadata, &project_bytes, self.assets)?;
        let mut file = TesseractFile::open(path)?;
        file.writer_generator = writer_generator;
        Ok(file)
    }

    fn from_document(project: EditableFxCompositionDocument) -> Self {
        Self {
            project,
            project_bytes: None,
            document_id: uuid::Uuid::new_v4().to_string(),
            generator: current_generator(),
            assets: BTreeMap::new(),
            fonts: BTreeMap::new(),
        }
    }
}

fn archive_metadata(
    project_bytes: &[u8],
    document_id: String,
    generator: Generator,
    assets: &BTreeMap<String, PendingAsset>,
    fonts: BTreeMap<String, FontAssetProperties>,
) -> Result<TesseractFileMetadata, TesseractFileError> {
    let now = now_rfc3339()?;
    Ok(TesseractFileMetadata {
        schema: METADATA_SCHEMA_URL.to_string(),
        format: FORMAT_NAME.to_string(),
        format_version: FORMAT_VERSION,
        fx_schema_version: Some(fx_schema::FX_SCHEMA_REVISION),
        document_id,
        created_at: now.clone(),
        modified_at: now,
        generator,
        project: ProjectDescriptor {
            path: PROJECT_PATH.to_string(),
            content_type: "application/vnd.tesseract.fx-composition+json".to_string(),
            byte_length: project_bytes.len() as u64,
            sha256: sha256_bytes(project_bytes),
        },
        assets: assets
            .iter()
            .map(|(id, asset)| (id.clone(), asset.descriptor.clone()))
            .collect(),
        fonts,
    })
}

/// Archive writer for an already loaded FX project. It serializes a
/// borrowed composition rather than decoding a second full tree from `project.json`.
pub struct TesseractExportBuilder {
    project_bytes: Vec<u8>,
    document_id: String,
    generator: Generator,
    assets: BTreeMap<String, PendingAsset>,
    fonts: BTreeMap<String, FontAssetProperties>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportDocument<'a, C> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    format_version: u8,
    dimensions: ExportDimensions,
    duration: f64,
    composition: &'a C,
}

#[derive(Serialize)]
struct ExportDimensions {
    width: u32,
    height: u32,
}

impl TesseractExportBuilder {
    /// Serializes a caller-validated composition into its portable document
    /// once, on the calling thread. The caller selects any serialization
    /// setting that the composition depends on. The written archive must
    /// subsequently be reopened for on-disk integrity verification before it
    /// is published to an asset service.
    pub fn new(
        composition: &impl Serialize,
        width: u32,
        height: u32,
        duration_seconds: f64,
    ) -> Result<Self, TesseractFileError> {
        if width == 0 || height == 0 || !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return Err(invalid(
                "invalid editable FX document dimensions or duration",
            ));
        }
        let document = ExportDocument {
            schema: fx_schema::EDITABLE_FX_DOCUMENT_SCHEMA_URL,
            format_version: fx_schema::EDITABLE_FX_DOCUMENT_FORMAT_VERSION,
            dimensions: ExportDimensions { width, height },
            duration: duration_seconds,
            composition,
        };
        let project_bytes = serde_json::to_vec(&document)?;
        Ok(Self {
            project_bytes,
            document_id: uuid::Uuid::new_v4().to_string(),
            generator: current_generator(),
            assets: BTreeMap::new(),
            fonts: BTreeMap::new(),
        })
    }

    /// Records the identity of the archive writer.
    pub fn generator(mut self, generator: Generator) -> Result<Self, TesseractFileError> {
        validate_generator(&generator)?;
        self.generator = generator;
        Ok(self)
    }

    /// Adds media from a validated local path without loading its bytes into memory.
    pub fn add_asset(
        mut self,
        asset_id: impl Into<String>,
        source: impl AsRef<Path>,
        kind: AssetKind,
    ) -> Result<Self, TesseractFileError> {
        let asset_id = asset_id.into();
        if self.assets.contains_key(&asset_id) {
            return Err(invalid(format!("duplicate asset ID {asset_id:?}")));
        }
        self.assets.insert(
            asset_id.clone(),
            prepare_asset(&asset_id, source.as_ref(), kind, false)?,
        );
        Ok(self)
    }

    /// Adds a font asset and its validated authoring metadata.
    pub fn add_font_asset(
        mut self,
        asset_id: impl Into<String>,
        source: impl AsRef<Path>,
        properties: FontAssetProperties,
    ) -> Result<Self, TesseractFileError> {
        let asset_id = asset_id.into();
        if self.assets.contains_key(&asset_id) {
            return Err(invalid(format!("duplicate asset ID {asset_id:?}")));
        }
        self.assets.insert(
            asset_id.clone(),
            prepare_asset(&asset_id, source.as_ref(), AssetKind::Font, false)?,
        );
        self.fonts.insert(asset_id, properties);
        Ok(self)
    }

    /// Atomically writes the ZIP after checking that every asset ID the
    /// serialized composition references is packaged. The caller supplies
    /// those IDs and must validate the completed archive before uploading it.
    pub fn write<'a>(
        self,
        path: impl AsRef<Path>,
        referenced_asset_ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), TesseractFileError> {
        self.write_with_runtime_assets(path, referenced_asset_ids, |_| false)
    }

    /// Writes a new archive while explicitly admitting runtime-owned references.
    pub fn write_with_runtime_assets<'a>(
        self,
        path: impl AsRef<Path>,
        referenced_asset_ids: impl IntoIterator<Item = &'a str>,
        is_runtime_owned: impl Fn(&str) -> bool,
    ) -> Result<(), TesseractFileError> {
        for asset_id in referenced_asset_ids {
            if !asset_id.is_empty()
                && !self.assets.contains_key(asset_id)
                && !is_runtime_owned(asset_id)
            {
                return Err(invalid(format!(
                    "project asset ID {asset_id:?} is neither packaged nor runtime-owned"
                )));
            }
        }
        let metadata = archive_metadata(
            &self.project_bytes,
            self.document_id,
            self.generator,
            &self.assets,
            self.fonts,
        )?;
        writer::write_new(path.as_ref(), metadata, &self.project_bytes, self.assets).map(|_| ())
    }
}

fn validate_project_asset_refs(
    project: &EditableFxCompositionDocument,
    metadata: &TesseractFileMetadata,
    is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
) -> Result<(), TesseractFileError> {
    validate_project_asset_ids(
        project,
        |asset_id| metadata.assets.contains_key(asset_id),
        is_runtime_owned,
    )
}

fn validate_project_asset_ids(
    project: &EditableFxCompositionDocument,
    contains: impl Fn(&str) -> bool,
    is_runtime_owned: impl Fn(AssetRef<'_>) -> bool,
) -> Result<(), TesseractFileError> {
    for asset in project.composition().asset_refs() {
        if !asset.asset_id.is_empty() && !contains(asset.asset_id) && !is_runtime_owned(asset) {
            return Err(invalid(format!(
                "project asset ID {:?} is neither packaged nor runtime-owned",
                asset.asset_id
            )));
        }
    }
    Ok(())
}

/// Check that a source file name can be stored as an archive asset.
///
/// Adding an asset applies the same check. Callers can use this to reject a
/// source before they build a project.
///
/// # Errors
/// Returns an error if the file name is empty, is not UTF-8, contains control
/// characters, or cannot form a safe archive path.
pub fn validate_asset_source_name(source: impl AsRef<Path>) -> Result<(), TesseractFileError> {
    asset_archive_path(source.as_ref()).map(drop)
}

// The random directory segment has a fixed length and alphabet, so it does not
// change whether the path is valid.
fn asset_archive_path(source: &Path) -> Result<String, TesseractFileError> {
    let basename = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !name.chars().any(char::is_control))
        .ok_or_else(|| invalid("asset source must have a safe UTF-8 filename"))?;
    let archive_path = format!("assets/{}/{basename}", uuid::Uuid::new_v4());
    validate_archive_path(&archive_path)?;
    Ok(archive_path)
}

fn prepare_asset(
    asset_id: &str,
    source: &Path,
    kind: AssetKind,
    calculate_integrity: bool,
) -> Result<PendingAsset, TesseractFileError> {
    validate_archive_path(asset_id)
        .map_err(|_| invalid(format!("invalid asset ID {asset_id:?}")))?;
    let metadata = source.metadata().at(source)?;
    if !metadata.is_file() {
        return Err(invalid(format!(
            "asset source is not a file: {}",
            source.display()
        )));
    }
    let archive_path = asset_archive_path(source)?;
    let (byte_length, sha256, crc32) = if calculate_integrity {
        hash_file(source)?
    } else {
        (metadata.len(), "0".repeat(64), 0)
    };
    Ok(PendingAsset {
        source: source.to_path_buf(),
        descriptor: AssetDescriptor {
            path: archive_path,
            kind,
            content_type: content_type(source, kind).to_string(),
            byte_length,
            sha256,
        },
        crc32,
        integrity_ready: calculate_integrity,
    })
}

fn atomic_write_file(destination: &Path, bytes: &[u8]) -> Result<(), TesseractFileError> {
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).at(parent)?;
    temporary.as_file_mut().write_all(bytes).at(destination)?;
    temporary.as_file_mut().sync_all().at(destination)?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)
        .at(destination)?;
    Ok(())
}

fn read_project_file(path: &Path) -> Result<Vec<u8>, TesseractFileError> {
    let path_metadata = std::fs::symlink_metadata(path).at(path)?;
    if !path_metadata.file_type().is_file() {
        return Err(invalid(format!(
            "project.json source must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    let mut file = File::open(path).at(path)?;
    let opened = file.metadata().at(path)?;
    if !opened.is_file() {
        return Err(invalid("opened project.json source is not a regular file"));
    }
    let bytes = read_document_snapshot(&mut file, opened.len(), path)?;
    let after = file.metadata().at(path)?;
    if opened.len() != after.len() || opened.modified().at(path)? != after.modified().at(path)? {
        return Err(invalid("project.json changed while reading"));
    }
    Ok(bytes)
}

fn read_document_snapshot(
    reader: &mut impl Read,
    size: u64,
    path: &Path,
) -> Result<Vec<u8>, TesseractFileError> {
    let len = usize::try_from(size).map_err(|_| invalid("document size overflow"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|error| invalid(format!("cannot allocate document buffer: {error}")))?;
    bytes.resize(len, 0);
    reader.read_exact(&mut bytes).at(path)?;
    if reader.read(&mut [0]).at(path)? != 0 {
        return Err(invalid("document length changed while reading"));
    }
    Ok(bytes)
}

fn read_document_entry(
    archive: &mut ZipArchive<File>,
    name: &str,
    expected_size: Option<u64>,
) -> Result<Vec<u8>, TesseractFileError> {
    let mut entry = archive.by_name(name)?;
    if expected_size.is_some_and(|size| entry.size() != size) {
        return Err(invalid(format!(
            "{name} length does not match its descriptor"
        )));
    }
    let size = entry.size();
    read_document_snapshot(&mut entry, size, Path::new(name))
}

fn parse_project(
    metadata: &TesseractFileMetadata,
    project_bytes: &[u8],
    entries: &HashMap<String, EntryInfo>,
) -> Result<EditableFxCompositionDocument, TesseractFileError> {
    verify_descriptor(
        PROJECT_PATH,
        project_bytes,
        metadata.project.byte_length,
        &metadata.project.sha256,
    )?;
    let project = legacy_timing::read(project_bytes)?;
    validate_entry_inventory(metadata, entries)?;
    Ok(project)
}

fn validate_entry_inventory(
    metadata: &TesseractFileMetadata,
    entries: &HashMap<String, EntryInfo>,
) -> Result<(), TesseractFileError> {
    let mut expected = HashSet::with_capacity(metadata.assets.len() + 2);
    expected.insert(METADATA_PATH);
    expected.insert(PROJECT_PATH);
    for descriptor in metadata.assets.values() {
        expected.insert(descriptor.path.as_str());
        let entry = entries
            .get(&descriptor.path)
            .ok_or_else(|| invalid(format!("missing asset entry {:?}", descriptor.path)))?;
        if entry.size != descriptor.byte_length {
            return Err(invalid(format!(
                "asset entry {:?} length does not match metadata",
                descriptor.path
            )));
        }
    }
    for name in entries.keys() {
        if !expected.contains(name.as_str()) {
            return Err(invalid(format!("unreferenced archive entry {name:?}")));
        }
    }
    Ok(())
}

fn verify_descriptor(
    name: &str,
    bytes: &[u8],
    expected_length: u64,
    expected_sha256: &str,
) -> Result<(), TesseractFileError> {
    if bytes.len() as u64 != expected_length
        || !sha256_bytes(bytes).eq_ignore_ascii_case(expected_sha256)
    {
        return Err(invalid(format!(
            "{name} does not match metadata integrity fields"
        )));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(u64, String, u32), TesseractFileError> {
    let mut file = File::open(path).at(path)?;
    let mut hasher = Sha256::new();
    let mut crc32 = crc32fast::Hasher::new();
    let mut length = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).at(path)?;
        if read == 0 {
            break;
        }
        length = length
            .checked_add(read as u64)
            .ok_or_else(|| invalid("asset length overflow"))?;
        hasher.update(&buffer[..read]);
        crc32.update(&buffer[..read]);
    }
    Ok((length, format!("{:x}", hasher.finalize()), crc32.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn source_identity(path: &Path) -> Result<SourceIdentity, TesseractFileError> {
    let metadata = std::fs::symlink_metadata(path).at(path)?;
    if !metadata.file_type().is_file() {
        return Err(invalid(format!(
            ".tsrct source must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    Ok(SourceIdentity {
        byte_length: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn now_rfc3339() -> Result<String, TesseractFileError> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| invalid(format!("failed to format current timestamp: {error}")))
}

fn current_generator() -> Generator {
    Generator {
        name: "tesseract".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        engine_version: option_env!("JERBOA_VERSION")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .to_string(),
        git_revision: option_env!("JERBOA_GIT_REVISION").map(str::to_string),
    }
}

fn content_type(path: &Path, kind: AssetKind) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "aac" => "audio/aac",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "pag" => "application/vnd.libpag",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "ttc" => "font/collection",
        "txt" => "text/plain",
        _ => match kind {
            AssetKind::Video => "video/unknown",
            AssetKind::Image => "image/unknown",
            AssetKind::Audio => "audio/unknown",
            AssetKind::Font => "font/unknown",
            _ => "application/octet-stream",
        },
    }
}
