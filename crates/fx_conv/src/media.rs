//! Explicit, source-bound media substitutions. This module never transcodes.

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufReader, Read},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Version-one sidecar published alongside prepared media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaMap {
    pub version: u32,
    pub source: MediaMapSource,
    pub replacements: Vec<MediaReplacement>,
}

/// Native project bytes and selected scene to which replacements apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaMapSource {
    pub format: String,
    pub sha256: String,
    pub target: String,
}

/// Original identity and the prepared file, not a promise of codec support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaReplacement {
    /// Absolute original path, resolved using the native format's ordinary rules.
    pub original: PathBuf,
    pub original_sha256: String,
    /// Relative to the sidecar directory; may not escape it through a symlink.
    pub replacement: PathBuf,
    pub replacement_sha256: String,
}

/// Invalid or stale substitutions must never bypass normal media validation.
#[derive(Debug, thiserror::Error)]
pub enum MediaMapError {
    #[error("media map I/O at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid media map JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid media map: {0}")]
    Invalid(String),
}

impl MediaMapError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            source,
        }
    }
}

#[derive(Debug)]
struct ResolvedReplacement {
    original: PathBuf,
    replacement: PathBuf,
}

/// Immutable map snapshot with checked local paths. Revalidate before publication.
#[derive(Debug)]
pub struct ValidatedMediaMap {
    map: MediaMap,
    base: PathBuf,
    entries: Vec<ResolvedReplacement>,
    by_original: HashMap<PathBuf, usize>,
}

impl ValidatedMediaMap {
    /// Read a map and verify both the original and prepared media identities.
    pub fn load(path: &Path) -> Result<Self, MediaMapError> {
        let file = File::open(path).map_err(|error| MediaMapError::io(path, error))?;
        let map: MediaMap = serde_json::from_reader(BufReader::new(file))?;
        if map.version != 1 {
            return Err(MediaMapError::Invalid(format!(
                "unsupported version {}",
                map.version
            )));
        }
        if !matches!(map.source.format.as_str(), "after-effects" | "premiere")
            || map.source.target.is_empty()
        {
            return Err(MediaMapError::Invalid(
                "missing or unsupported native source identity".into(),
            ));
        }
        validate_hash(&map.source.sha256)?;
        let base = canonical(path.parent().unwrap_or(Path::new(".")))?;
        let mut entries = Vec::with_capacity(map.replacements.len());
        let mut by_original = HashMap::new();
        for entry in &map.replacements {
            if !entry.original.is_absolute() {
                return Err(MediaMapError::Invalid(
                    "original paths must be absolute".into(),
                ));
            }
            validate_hash(&entry.original_sha256)?;
            validate_hash(&entry.replacement_sha256)?;
            let original = canonical(&entry.original)?;
            let replacement = confined_replacement(&base, &entry.replacement)?;
            if original == replacement {
                return Err(MediaMapError::Invalid(
                    "a replacement must not overwrite its original".into(),
                ));
            }
            if by_original
                .insert(original.clone(), entries.len())
                .is_some()
            {
                return Err(MediaMapError::Invalid(format!(
                    "duplicate original {original:?}"
                )));
            }
            entries.push(ResolvedReplacement {
                original,
                replacement,
            });
        }
        let validated = Self {
            map,
            base,
            entries,
            by_original,
        };
        validated.verify_files()?;
        Ok(validated)
    }

    /// Bind the map to this conversion and recheck every file before publication.
    pub fn validate_for(
        &self,
        input: &Path,
        format: &str,
        target: &str,
    ) -> Result<(), MediaMapError> {
        if self.map.source.format != format || self.map.source.target != target {
            return Err(MediaMapError::Invalid(
                "source format or selected target differs".into(),
            ));
        }
        verify_hash(input, &self.map.source.sha256)?;
        self.verify_files()
    }

    /// Resolve only after native path relocation; basename matching is forbidden.
    /// The returned file still requires the format's normal media admission checks.
    pub fn replacement_for(&self, original: &Path) -> Result<Option<&Path>, MediaMapError> {
        let original = canonical(original)?;
        Ok(self
            .by_original
            .get(&original)
            .map(|&index| self.entries[index].replacement.as_path()))
    }

    fn verify_files(&self) -> Result<(), MediaMapError> {
        for (entry, resolved) in self.map.replacements.iter().zip(&self.entries) {
            if canonical(&entry.original)? != resolved.original
                || confined_replacement(&self.base, &entry.replacement)? != resolved.replacement
            {
                return Err(MediaMapError::Invalid(
                    "a mapped path changed its resolved identity".into(),
                ));
            }
            verify_hash(&resolved.original, &entry.original_sha256)?;
            verify_hash(&resolved.replacement, &entry.replacement_sha256)?;
        }
        Ok(())
    }
}

fn canonical(path: &Path) -> Result<PathBuf, MediaMapError> {
    fs::canonicalize(path).map_err(|error| MediaMapError::io(path, error))
}

fn confined_replacement(base: &Path, relative: &Path) -> Result<PathBuf, MediaMapError> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(MediaMapError::Invalid(
            "replacement must be a confined relative path".into(),
        ));
    }
    let path = canonical(&base.join(relative))?;
    if !path.starts_with(base) {
        return Err(MediaMapError::Invalid(
            "replacement escapes the map directory".into(),
        ));
    }
    Ok(path)
}

fn validate_hash(value: &str) -> Result<(), MediaMapError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(MediaMapError::Invalid(
            "SHA-256 must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

fn verify_hash(path: &Path, expected: &str) -> Result<(), MediaMapError> {
    if sha256_file(path)? != expected {
        return Err(MediaMapError::Invalid(format!("file changed: {path:?}")));
    }
    Ok(())
}

/// Stream a regular local file without a fixed source-size admission limit.
pub fn sha256_file(path: &Path) -> Result<String, MediaMapError> {
    let metadata = fs::metadata(path).map_err(|error| MediaMapError::io(path, error))?;
    if !metadata.is_file() {
        return Err(MediaMapError::Invalid(format!(
            "not a regular file: {path:?}"
        )));
    }
    let mut file = File::open(path).map_err(|error| MediaMapError::io(path, error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| MediaMapError::io(path, error))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests;
