use crate::TesseractFileError;
use fx_schema::{validate_embedded_font_registry, FontAssetProperties};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub(crate) const FORMAT_NAME: &str = "tesseract";
pub(crate) const FORMAT_VERSION: u32 = 2;
pub(crate) const METADATA_PATH: &str = "metadata.json";
pub(crate) const PROJECT_PATH: &str = "project.json";
pub(crate) const METADATA_SCHEMA_URL: &str = "urn:tesseract:file:metadata:v2";
const MAX_ZIP_ENTRY_NAME_BYTES: usize = u16::MAX as usize;

/// Metadata stored at the root of every `.tsrct` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TesseractFileMetadata {
    /// Stable JSON Schema identifier for this metadata major version.
    #[serde(rename = "$schema")]
    pub schema: String,
    /// Container discriminator; always `tesseract` in v2.
    pub format: String,
    /// Container compatibility version, independent of the writer's engine version.
    pub format_version: u32,
    /// FX schema revision of the last writer; absent in older archives.
    /// Informational only: this is not a reader compatibility gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fx_schema_version: Option<u8>,
    /// Stable identity retained across saves and copies.
    pub document_id: String,
    /// RFC 3339 package creation time.
    pub created_at: String,
    /// RFC 3339 time of the last successful save.
    pub modified_at: String,
    /// Runtime that last wrote the file.
    pub generator: Generator,
    /// Location and integrity of the authoritative editable FX JSON.
    pub project: ProjectDescriptor,
    /// Exact project asset ID to user-asset entry mapping.
    pub assets: BTreeMap<String, AssetDescriptor>,
    /// Authoring metadata for embedded fonts, keyed by the same opaque key as
    /// `assets`. Kept separate so [`AssetDescriptor`] remains transport-only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fonts: BTreeMap<String, FontAssetProperties>,
}

/// Identifies the runtime that last wrote a `.tsrct` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Generator {
    /// Name of the writing application or library.
    pub name: String,
    /// Version of the writing application or library.
    pub version: String,
    /// Engine/release version used by the writer.
    pub engine_version: String,
    /// Optional source revision for development and reproducibility diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_revision: Option<String>,
}

/// Integrity metadata for the editable FX project JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectDescriptor {
    /// Root archive path; fixed to `project.json` in v2.
    pub path: String,
    /// Editable FX project JSON media type.
    pub content_type: String,
    /// Exact JSON byte length.
    pub byte_length: u64,
    /// Lowercase SHA-256 content digest written by canonical writers.
    pub sha256: String,
}

/// A user-provided asset's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    /// Video media.
    Video,
    /// Still-image media.
    Image,
    /// Audio media.
    Audio,
    /// PAG animation data.
    Pag,
    /// User-provided font data.
    Font,
    /// Plain-text data.
    Text,
    /// A user asset not covered by a more specific v2 kind.
    Other,
}

/// Maps one project asset ID to one archive entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetDescriptor {
    /// Validated archive path below `assets/`.
    pub path: String,
    /// Coarse decoder-routing type.
    pub kind: AssetKind,
    /// Descriptive MIME content type; decoders still inspect file signatures.
    pub content_type: String,
    /// Exact uncompressed byte length.
    pub byte_length: u64,
    /// SHA-256 content identity used by the materialization cache.
    pub sha256: String,
}

impl TesseractFileMetadata {
    /// Describes an FX schema revision difference without rejecting the archive.
    /// A missing revision is unknown, not an inferred numeric value.
    pub fn fx_schema_version_notice(&self, reader: &str) -> Option<String> {
        let converter_version = fx_schema::FX_SCHEMA_REVISION;
        match self.fx_schema_version {
            Some(version) if version == converter_version => None,
            version => Some(format!(
                "info: FX schema versions: Tesseract file fxSchemaVersion={}, {reader} fxSchemaVersion={converter_version}. A schema difference may be relevant when investigating conversion or editing failures.",
                version.map_or_else(|| "unknown (missing)".to_owned(), |version| version.to_string())
            )),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), TesseractFileError> {
        if self.schema != METADATA_SCHEMA_URL {
            return Err(invalid(format!(
                "unsupported metadata schema {:?}",
                self.schema
            )));
        }
        if self.format != FORMAT_NAME {
            return Err(invalid(format!("unsupported format {:?}", self.format)));
        }
        if self.format_version != FORMAT_VERSION {
            return Err(invalid(format!(
                "unsupported formatVersion {}; expected {FORMAT_VERSION}",
                self.format_version
            )));
        }
        uuid::Uuid::parse_str(&self.document_id)
            .map_err(|_| invalid("documentId must be a UUID"))?;
        validate_timestamp("createdAt", &self.created_at)?;
        validate_timestamp("modifiedAt", &self.modified_at)?;
        validate_generator(&self.generator)?;
        validate_digest(&self.project.sha256)?;
        if self.project.path != PROJECT_PATH {
            return Err(invalid("project.path must be project.json"));
        }
        if self.project.content_type != "application/vnd.tesseract.fx-composition+json" {
            return Err(invalid("project.contentType is unsupported"));
        }
        for asset_id in self.fonts.keys() {
            match self.assets.get(asset_id) {
                Some(descriptor) if descriptor.kind == AssetKind::Font => {}
                Some(_) => {
                    return Err(invalid(format!(
                        "font registry key {asset_id:?} does not identify a font asset"
                    )));
                }
                None => {
                    return Err(invalid(format!(
                        "font registry key {asset_id:?} is missing from assets"
                    )));
                }
            }
        }
        validate_embedded_font_registry(
            self.fonts
                .iter()
                .map(|(asset_id, properties)| (asset_id.as_str(), properties)),
        )
        .map_err(|error| invalid(format!("invalid font registry: {error}")))?;

        let mut total_bytes = 0_u64;
        let mut paths = HashSet::with_capacity(self.assets.len());
        let mut folded_paths = HashSet::with_capacity(self.assets.len());
        for (asset_id, descriptor) in &self.assets {
            validate_asset_id(asset_id)?;
            validate_archive_path(&descriptor.path)?;
            if descriptor.path == METADATA_PATH || descriptor.path == PROJECT_PATH {
                return Err(invalid(format!(
                    "asset {asset_id:?} uses reserved path {:?}",
                    descriptor.path
                )));
            }
            if !descriptor.path.starts_with("assets/") {
                return Err(invalid(format!(
                    "asset {asset_id:?} path must be under assets/"
                )));
            }
            if !paths.insert(descriptor.path.as_str())
                || !folded_paths.insert(descriptor.path.to_ascii_lowercase())
            {
                return Err(invalid(format!(
                    "duplicate or case-colliding asset path {:?}",
                    descriptor.path
                )));
            }
            if !valid_content_type(&descriptor.content_type) {
                return Err(invalid(format!(
                    "asset {asset_id:?} has an invalid contentType"
                )));
            }
            validate_digest(&descriptor.sha256)?;
            total_bytes = total_bytes
                .checked_add(descriptor.byte_length)
                .ok_or_else(|| invalid("total asset byte length overflow"))?;
        }
        Ok(())
    }
}

pub(crate) fn serialize_metadata(
    metadata: &TesseractFileMetadata,
) -> Result<Vec<u8>, TesseractFileError> {
    Ok(serde_json::to_vec_pretty(metadata)?)
}

pub(crate) fn validate_generator(generator: &Generator) -> Result<(), TesseractFileError> {
    if generator.name.is_empty()
        || generator.version.is_empty()
        || generator.engine_version.is_empty()
    {
        return Err(invalid("generator fields must not be empty"));
    }
    if generator.git_revision.as_ref().is_some_and(|revision| {
        !(7..=64).contains(&revision.len())
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(invalid(
            "generator.gitRevision must be 7-64 hexadecimal characters",
        ));
    }
    Ok(())
}

pub(crate) fn validate_archive_path(path: &str) -> Result<(), TesseractFileError> {
    if path.is_empty()
        || path.len() > MAX_ZIP_ENTRY_NAME_BYTES
        || path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
    {
        return Err(invalid(format!("unsafe archive path {path:?}")));
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(invalid(format!("non-normalized archive path {path:?}")));
    }
    Ok(())
}

fn validate_asset_id(asset_id: &str) -> Result<(), TesseractFileError> {
    validate_archive_path(asset_id).map_err(|_| invalid(format!("invalid asset ID {asset_id:?}")))
}

fn valid_content_type(content_type: &str) -> bool {
    content_type.split_once('/').is_some_and(|(kind, subtype)| {
        !kind.is_empty()
            && !subtype.is_empty()
            && !content_type.chars().any(char::is_whitespace)
            && !subtype.contains('/')
    })
}

fn validate_digest(digest: &str) -> Result<(), TesseractFileError> {
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("sha256 must contain 64 hexadecimal characters"));
    }
    Ok(())
}

fn validate_timestamp(field: &str, timestamp: &str) -> Result<(), TesseractFileError> {
    time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
        .map_err(|_| invalid(format!("{field} must be an RFC 3339 timestamp")))?;
    Ok(())
}

pub(crate) fn invalid(message: impl Into<String>) -> TesseractFileError {
    TesseractFileError::Invalid(message.into())
}
