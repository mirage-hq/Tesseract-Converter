//! Explicit native-record relocation, separate from prepared-media substitution.

use crate::{
    error::{ensure, Result},
    format::Graph,
    hash::hash,
    schema::native::Media,
    ConversionError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

/// Caller-authorized relocation for exactly one original project and sequence.
/// This does not establish that the chosen files were used by Adobe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaRelink {
    pub version: u32,
    pub source: fx_conv::MediaMapSource,
    pub bindings: Vec<MediaRelinkBinding>,
}

/// An exact native Media UID and FilePath, not a basename or suffix match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaRelinkBinding {
    pub media_uid: String,
    pub authored_path: String,
    /// An explicit absolute local file; ordinary format admission still applies.
    pub local_path: PathBuf,
    pub sha256: String,
}

/// Immutable, hash-checked relocations; revalidated before publication.
#[derive(Debug, Clone)]
pub struct ValidatedMediaRelink {
    manifest: MediaRelink,
    paths: BTreeMap<String, PathBuf>,
}

impl ValidatedMediaRelink {
    /// Load an explicit relocation manifest without modifying the native project.
    pub fn load(path: &Path) -> std::result::Result<Self, ConversionError> {
        let manifest = serde_json::from_reader(BufReader::new(
            File::open(path).map_err(crate::error::BuildError::from)?,
        ))
        .map_err(crate::error::BuildError::from)?;
        Self::new(manifest)
    }

    /// Validate caller-supplied local files. Native-record and target identity
    /// checks also run for each import, before any occurrence can be omitted.
    pub fn new(manifest: MediaRelink) -> std::result::Result<Self, ConversionError> {
        Ok(Self::validate(manifest)?)
    }

    fn validate(manifest: MediaRelink) -> Result<Self> {
        ensure!(
            manifest.version == 1,
            "media relink: unsupported version {}",
            manifest.version
        );
        ensure!(
            manifest.source.format == "premiere" && !manifest.source.target.is_empty(),
            "media relink: missing Premiere source/target identity"
        );
        validate_hash(&manifest.source.sha256)?;
        let mut paths = BTreeMap::new();
        for binding in &manifest.bindings {
            ensure!(
                !binding.media_uid.is_empty(),
                "media relink: empty Media UID"
            );
            ensure!(
                absolute_authored_path(&binding.authored_path),
                "media relink: authored FilePath must be a supported absolute path"
            );
            ensure!(
                binding.local_path.is_absolute() && binding.local_path.to_str().is_some(),
                "media relink: local_path must be an absolute UTF-8 path"
            );
            validate_hash(&binding.sha256)?;
            let path = binding.local_path.canonicalize()?;
            ensure!(
                path.is_file(),
                "media relink: local file is not a regular file"
            );
            ensure!(
                paths.insert(binding.media_uid.clone(), path).is_none(),
                "media relink: duplicate Media UID {:?}",
                binding.media_uid
            );
        }
        let relink = Self { manifest, paths };
        relink.verify_files()?;
        Ok(relink)
    }

    pub(crate) fn validate_source_bytes(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            crate::hash::hash_reader(bytes)? == self.manifest.source.sha256,
            "media relink: original project bytes differ"
        );
        Ok(())
    }

    pub(crate) fn validate_for(&self, source: &Path, target: &str) -> Result<()> {
        ensure!(
            self.manifest.source.target == target,
            "media relink: selected sequence differs"
        );
        ensure!(
            hash(source)? == self.manifest.source.sha256,
            "media relink: original project bytes differ"
        );
        self.verify_files()
    }

    fn verify_files(&self) -> Result<()> {
        for binding in &self.manifest.bindings {
            let current = binding.local_path.canonicalize()?;
            ensure!(
                self.paths.get(&binding.media_uid) == Some(&current),
                "media relink: local path changed its resolved identity"
            );
            ensure!(
                current.is_file() && hash(&current)? == binding.sha256,
                "media relink: local file bytes changed for {:?}",
                binding.media_uid
            );
        }
        Ok(())
    }

    /// Validate every supplied record, including ones unsupported placement
    /// readers would otherwise omit. A bad explicit binding is always fatal.
    pub(crate) fn paths_for_graph(&self, graph: &Graph<'_>) -> Result<BTreeMap<String, PathBuf>> {
        for binding in &self.manifest.bindings {
            let record = graph.locate_uid(&binding.media_uid, "media relink")?;
            let media = graph.decode_as::<Media>(record, "media relink")?;
            ensure!(
                media.value.file_path.as_deref() == Some(binding.authored_path.as_str()),
                "media relink: authored FilePath differs for {:?}",
                binding.media_uid
            );
            let aliases: Vec<_> = [
                media.value.file_path.as_deref(),
                media.value.actual_media_file_path.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect();
            for path in &aliases {
                ensure!(
                    absolute_authored_path(path),
                    "media relink: malformed native absolute media alias for {:?}",
                    binding.media_uid
                );
            }
            let windows = aliases
                .iter()
                .all(|path| windows_absolute_path(path) && !Path::new(path).is_absolute());
            ensure!(
                windows || aliases.iter().all(|path| Path::new(path).is_absolute()),
                "media relink: mixed native path platforms"
            );
            let mut package_local = std::collections::BTreeSet::new();
            for hint in &media.value.relative_paths {
                let hint = if windows {
                    ensure!(!hint.split(['\\', '/']).any(|part| matches!(part.as_bytes(), [drive, b':', ..] if drive.is_ascii_alphabetic())), "media relink: RelativePath names a Windows drive");
                    hint.replace('\\', "/")
                } else {
                    hint.clone()
                };
                ensure!(
                    !hint.is_empty() && !Path::new(&hint).is_absolute(),
                    "media relink: empty or absolute RelativePath"
                );
                if !Path::new(&hint)
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
                {
                    package_local.insert(hint);
                }
            }
            ensure!(
                package_local.len() <= 1,
                "media relink: several package-local RelativePath hints"
            );
        }
        Ok(self.paths.clone())
    }
}

fn validate_hash(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "media relink: SHA-256 must be 64 lowercase hexadecimal characters"
    );
    Ok(())
}

/// Only drive-absolute Windows aliases, including Premiere's extended spelling.
/// UNC, drive-relative and other device namespaces remain unsupported.
pub(crate) fn windows_absolute_path(path: &str) -> bool {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    matches!(path.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic())
}

fn absolute_authored_path(path: &str) -> bool {
    !path.is_empty()
        && !path.chars().any(char::is_control)
        && (Path::new(path).is_absolute() || windows_absolute_path(path))
}
