use crate::error::IoContext;
use crate::{AssetReader, TesseractAsset, TesseractFileError};
use crc32fast::Hasher as Crc32;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Root directory used to materialize archive assets for path-only consumers.
#[derive(Debug, Clone)]
pub struct MaterializationCache {
    root: PathBuf,
}

impl MaterializationCache {
    /// Creates a cache rooted at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { root: path.into() }
    }

    pub(crate) fn materialize(
        &self,
        asset: &TesseractAsset<'_>,
    ) -> Result<MaterializedAsset, TesseractFileError> {
        let descriptor = asset.descriptor();
        let digest = descriptor.sha256.to_ascii_lowercase();
        let directory = self.root.join(&digest);
        let content_directory = directory.join("content");
        let marker_directory = directory.join("markers");
        let lock_directory = directory.join("locks");
        std::fs::create_dir_all(&content_directory).at(&content_directory)?;
        std::fs::create_dir_all(&marker_directory).at(&marker_directory)?;
        std::fs::create_dir_all(&lock_directory).at(&lock_directory)?;
        let lock_path = lock_directory.join(".materialize.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .at(&lock_path)?;
        lock.lock_exclusive().at(&lock_path)?;

        let filename = materialization_filename(&descriptor.path);
        let destination = content_directory.join(&filename);
        let marker = marker_directory.join(format!("{filename}.verified"));
        let expected_marker = format!("{}\n{}\n", descriptor.byte_length, digest);

        if destination
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == descriptor.byte_length)
            && std::fs::read_to_string(&marker).is_ok_and(|value| value == expected_marker)
        {
            return Ok(MaterializedAsset { path: destination });
        }

        let mut source = asset.open()?;
        let mut temporary = tempfile::NamedTempFile::new_in(&directory).at(&directory)?;
        copy_and_verify(
            &mut source,
            temporary.as_file_mut(),
            descriptor.byte_length,
            &descriptor.sha256,
            asset.crc32(),
        )?;
        temporary.as_file_mut().sync_all().at(temporary.path())?;
        if destination.exists() {
            std::fs::remove_file(&destination).at(&destination)?;
        }
        temporary
            .persist(&destination)
            .map_err(|error| TesseractFileError::Io {
                path: destination.clone(),
                source: error.error,
            })?;

        let marker_temporary = tempfile::NamedTempFile::new_in(&directory).at(&directory)?;
        std::fs::write(marker_temporary.path(), expected_marker.as_bytes())
            .at(marker_temporary.path())?;
        if marker.exists() {
            std::fs::remove_file(&marker).at(&marker)?;
        }
        marker_temporary
            .persist(&marker)
            .map_err(|error| TesseractFileError::Io {
                path: marker.clone(),
                source: error.error,
            })?;

        Ok(MaterializedAsset { path: destination })
    }
}

/// Handle to a verified materialized asset path.
#[derive(Debug)]
pub struct MaterializedAsset {
    path: PathBuf,
}

impl MaterializedAsset {
    /// Returns the normal filesystem path accepted by FFmpeg and native decoders.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// File name inside the per-digest `content/` directory: a short hash of the
/// archive path (assets with identical bytes at different paths coexist) plus
/// the original extension for path-only consumers that sniff it. The content
/// digest already names the directory, so it is not repeated here: with it,
/// a font in a Windows temp directory reached the 260-character path limit.
fn materialization_filename(path: &str) -> String {
    let path_digest = format!("{:x}", Sha256::digest(path.as_bytes()));
    let mut filename = path_digest[..16].to_owned();
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|basename| basename.rsplit_once('.'))
        .and_then(|(stem, extension)| (!stem.is_empty()).then_some(extension))
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 32
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
    if let Some(extension) = extension {
        filename.push('.');
        filename.push_str(extension);
    }
    filename
}

fn copy_and_verify(
    source: &mut AssetReader,
    destination: &mut impl Write,
    expected_size: u64,
    expected_sha256: &str,
    expected_crc32: u32,
) -> Result<(), TesseractFileError> {
    let mut sha256 = Sha256::new();
    let mut crc32 = Crc32::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|source| TesseractFileError::Io {
                path: PathBuf::from(".tsrct asset entry"),
                source,
            })?;
        if read == 0 {
            break;
        }
        total += read as u64;
        sha256.update(&buffer[..read]);
        crc32.update(&buffer[..read]);
        destination
            .write_all(&buffer[..read])
            .map_err(|source| TesseractFileError::Io {
                path: PathBuf::from("materialization cache temporary file"),
                source,
            })?;
    }
    let actual_sha256 = format!("{:x}", sha256.finalize());
    if total != expected_size
        || !actual_sha256.eq_ignore_ascii_case(expected_sha256)
        || crc32.finalize() != expected_crc32
    {
        return Err(TesseractFileError::Invalid(
            "asset bytes do not match metadata integrity fields".to_string(),
        ));
    }
    Ok(())
}
